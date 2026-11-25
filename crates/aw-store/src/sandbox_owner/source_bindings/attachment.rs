//! Attach one retained version to an existing managed Sandbox.
//! Same Project receipt, source coordinator and binding publication owner.
use super::{
    insert_at_revision, next_source_rev, publish_source_change_in,
    release::{observe_managed_in, release_event_id, ManagedMutation},
    remove_binding_in, retained_in, SourceChange, VersionedSourceBinding,
};
use crate::{
    construction_receipt::{
        self, ConstructionCandidate, ConstructionOperation, ConstructionPreparation,
        PreparationState,
    },
    db, id, label, sandbox_owner,
    store_scope::StoreScope,
    user_for_principal, Error, PrincipalId, Result, SandboxLifecycle, SandboxSnapshot,
    SessionStore, SourceMutationBarrier, SourceMutationSet, StoreMode,
};
use aw_agent_core::{
    validate_source_set_hard_limit, ExternalEvidence, NamedPreparedSource, PreparedSourceOperation,
    PreparedSourceSet, RecoveryInspection, SourceAnchor, SourceBinding, SourceCoverage,
    SourceDriverRegistryReader, SourceName, SourceOperationKind, SourcePublicationFence,
    SourcePublicationProof, SourceSet,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttachmentBoundary {
    pub(crate) project_ref: String,
    expected_rev: u64,
    writer_epoch: u64,
    layout_digest: String,
    bindings: Vec<VersionedSourceBinding>,
    baseline: SourceSet,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    replace: bool,
}

fn matches_anchor(anchor: &SourceAnchor, binding: &SourceBinding) -> bool {
    binding.name == anchor.name
        && binding.driver == anchor.driver
        && binding.source_ref == anchor.source_ref
        && binding.access == anchor.access
        && binding.checkpoint_policy == anchor.checkpoint_policy
}

impl AttachmentBoundary {
    pub(crate) fn anchor(&self) -> &SourceAnchor {
        &self.baseline.anchors[0]
    }

    pub(crate) fn validate(
        &self,
        sandbox: &str,
        sources: Option<&PreparedSourceSet>,
    ) -> Result<()> {
        let key = sandbox_owner::parse_sandbox_ref(sandbox)?;
        if key.canonical() != sandbox
            || key.project.canonical() != self.project_ref
            || self.expected_rev == 0
            || self.writer_epoch == 0
            || self.layout_digest.len() != 64
            || !self
                .layout_digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.bindings.is_empty()
            || self.baseline.anchors.len() != 1
            || self.baseline.coverage != SourceCoverage::Selected
        {
            return Err(Error::Storage("invalid Sandbox attachment boundary"));
        }
        next_source_rev(self.expected_rev)?;
        self.baseline.validate()?;
        validate_source_set_hard_limit(self.bindings.len() + usize::from(!self.replace))?;
        if self
            .bindings
            .iter()
            .any(|binding| binding.binding.name == self.anchor().name)
            != self.replace
        {
            return Err(Error::Conflict(
                "materialization selection differs from old named bindings",
            ));
        }
        for binding in &self.bindings {
            binding.validate()?;
            if binding.binding_rev > self.expected_rev {
                return Err(Error::Conflict("materialization binding revision changed"));
            }
        }
        if self
            .bindings
            .windows(2)
            .any(|pair| pair[0].binding.name.as_str() >= pair[1].binding.name.as_str())
        {
            return Err(Error::Storage("invalid attachment source ordering"));
        }
        construction_receipt::require_matching_sources(
            &self.baseline,
            sources.ok_or(Error::Storage("attachment has no original descriptor"))?,
        )
    }

    pub(crate) fn authorize(
        &self,
        c: &Connection,
        actor: &PrincipalId,
        sandbox: &str,
    ) -> Result<()> {
        let key = sandbox_owner::parse_sandbox_ref(sandbox)?;
        if key.project.canonical() != self.project_ref {
            return Err(Error::Conflict("attachment Project changed"));
        }
        sandbox_owner::authorize_project(c, actor, &key.project, true)
    }

    pub(crate) fn require_compatible(&self, registry: &SourceDriverRegistryReader) -> Result<()> {
        registry.resolve(&self.anchor().pin()?)?;
        for binding in &self.bindings {
            registry.resolve(&binding.binding.pin()?)?;
        }
        Ok(())
    }

    pub(crate) fn require_unpublished_in(&self, c: &Connection, sandbox: &str) -> Result<()> {
        let (snapshot, bindings) = retained_in(c, sandbox)?;
        if snapshot.lifecycle != SandboxLifecycle::Open
            || snapshot.rev != self.expected_rev
            || snapshot.project_ref != self.project_ref
            || snapshot.layout_digest != self.layout_digest
            || bindings != self.bindings
        {
            return Err(Error::Conflict("unpublished attachment layout changed"));
        }
        Ok(())
    }

    pub(crate) fn require_published_in(
        &self,
        c: &Connection,
        sandbox: &str,
        prepare_id: &str,
    ) -> Result<()> {
        let (snapshot, bindings) = retained_in(c, sandbox)?;
        let rev = next_source_rev(self.expected_rev)?;
        if snapshot.rev != rev
            || snapshot.lifecycle != SandboxLifecycle::Open
            || snapshot.layout_digest != self.layout_digest
            || snapshot.project_ref != self.project_ref
            || bindings.len() != self.bindings.len() + usize::from(!self.replace)
            || !bindings.iter().any(|binding| {
                binding.binding_rev == rev && matches_anchor(self.anchor(), &binding.binding)
            })
            || !bindings
                .iter()
                .filter(|binding| binding.binding.name != self.anchor().name)
                .eq(self
                    .bindings
                    .iter()
                    .filter(|binding| binding.binding.name != self.anchor().name))
            || attached_id_in(c, sandbox, &self.anchor().name, rev)?.as_deref() != Some(prepare_id)
        {
            return Err(Error::Conflict(
                "attachment did not publish its exact candidate",
            ));
        }
        Ok(())
    }

    fn mutation_set(&self) -> Result<SourceMutationSet> {
        SourceMutationSet::with_incoming_anchor(
            self.bindings.iter().map(|b| &b.binding),
            self.anchor(),
        )
    }
}

fn attachment_event_id(sandbox: &str, name: &SourceName, rev: u64) -> Result<String> {
    let identity = crate::json(&(sandbox, name, rev))?;
    let mut hash = Sha256::new();
    hash.update(b"aw-sandbox-source-attachment-v1\0");
    hash.update(identity.as_bytes());
    Ok(format!("sbx-attach:{:x}", hash.finalize()))
}

fn attached_id_in(
    c: &Connection,
    sandbox: &str,
    name: &SourceName,
    rev: u64,
) -> Result<Option<String>> {
    db(c.query_row(
        "SELECT json_extract(payload_json,'$.materialization_prepare_id') FROM commit_log
         WHERE event_id=?1 AND kind='sandbox.sources.changed' AND entity_type='sandbox' AND entity_id=?2
           AND json_extract(payload_json,'$.source_name')=?3 AND json_extract(payload_json,'$.rev')=?4",
        params![attachment_event_id(sandbox, name, rev)?, sandbox, name.as_str(), rev],
        |row| row.get(0),
    ).optional())
}

/// Resolve a later binding's exact original Applied descriptor via the
/// existing changed fact and unique Prepared index, not its current path.
pub(super) fn attached_preparation_in(
    c: &Connection,
    sandbox: &str,
    binding: &VersionedSourceBinding,
) -> Result<NamedPreparedSource> {
    let prepare_id = attached_id_in(c, sandbox, &binding.binding.name, binding.binding_rev)?
        .ok_or(Error::Conflict(
            "binding has no managed attachment provenance",
        ))?;
    let receipt = construction_receipt::source_claim_by_id_in(c, &prepare_id)?;
    let ConstructionCandidate::SandboxAttachment {
        sandbox: original,
        boundary,
        ..
    } = &receipt.candidate
    else {
        return Err(Error::Storage("attachment provenance is not an attachment"));
    };
    if receipt.operation != ConstructionOperation::SandboxSourceAttach
        || receipt.state != PreparationState::Applied
        || original != sandbox
        || receipt.result_ref.as_deref() != Some(sandbox)
        || next_source_rev(boundary.expected_rev)? != binding.binding_rev
        || !matches_anchor(boundary.anchor(), &binding.binding)
    {
        return Err(Error::Conflict(
            "binding differs from original Applied attachment",
        ));
    }
    let sources = receipt
        .sources
        .ok_or(Error::Storage("attachment descriptor missing"))?;
    sources
        .sources
        .into_iter()
        .next()
        .ok_or(Error::Storage("attachment descriptor missing"))
}

/// Read-only trusted input to the independent physical authority owner.
/// Old sources are retained; the one incoming source is not yet published.
pub struct SandboxAttachment {
    layout: ManagedMutation,
    baseline: SourceSet,
}

impl SandboxAttachment {
    pub fn snapshot(&self) -> &SandboxSnapshot {
        &self.layout.snapshot
    }
    pub fn bindings(&self) -> &[VersionedSourceBinding] {
        &self.layout.bindings
    }
    pub fn preparations(&self) -> &[NamedPreparedSource] {
        &self.layout.preparations
    }
    pub fn anchor(&self) -> &SourceAnchor {
        &self.baseline.anchors[0]
    }

    /// The original selected binding and exact Applied descriptor for a
    /// replacement; absent for an attachment. Neither grants disposal.
    pub fn replaced(&self) -> Option<(&VersionedSourceBinding, &NamedPreparedSource)> {
        self.layout
            .bindings
            .iter()
            .zip(&self.layout.preparations)
            .find(|(binding, _)| binding.binding.name == self.anchor().name)
    }
}

fn fingerprint(sandbox: &str, rev: u64, anchor: &SourceAnchor, replace: bool) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(if replace {
        b"aw-sandbox-replace-version-v1\0".as_slice()
    } else {
        b"aw-sandbox-attach-version-v1\0".as_slice()
    });
    hash.update(crate::json(&(sandbox, rev, anchor))?.as_bytes());
    Ok(format!("{:x}", hash.finalize()))
}

fn owner(store: &SessionStore) -> Result<()> {
    if store.inner.mode != StoreMode::OwnerLock {
        return Err(Error::Denied(
            "effectful attachment requires exclusive Store owner",
        ));
    }
    Ok(())
}

fn prior(
    c: &Connection,
    actor: &PrincipalId,
    sandbox: &str,
    request_id: &str,
) -> Result<(String, Option<ConstructionPreparation>)> {
    label(request_id, 256)?;
    let key = sandbox_owner::parse_sandbox_ref(sandbox)?;
    sandbox_owner::authorize_project(c, actor, &key.project, false)?;
    if key.canonical() != sandbox {
        return Err(Error::Denied("attachment requires canonical Sandbox"));
    }
    let creator =
        user_for_principal(c, actor)?.ok_or(Error::Denied("attachment caller unavailable"))?;
    let receipt = construction_receipt::lookup_in(
        c,
        ConstructionOperation::SandboxSourceAttach,
        &creator,
        request_id,
    )?;
    if let Some(receipt) = &receipt {
        receipt.require_actor(c, actor)?;
        if !matches!(&receipt.candidate, ConstructionCandidate::SandboxAttachment { sandbox: original, .. } if original == sandbox)
        {
            return Err(Error::Conflict(
                "attachment request belongs to another Sandbox",
            ));
        }
    }
    Ok((creator, receipt))
}

fn new_request(
    c: &Connection,
    actor: &PrincipalId,
    sandbox: &str,
    request_id: &str,
) -> Result<String> {
    let (creator, receipt) = prior(c, actor, sandbox, request_id)?;
    if receipt.is_some() {
        return Err(Error::Conflict(
            "attachment already claimed; replay or inspect",
        ));
    }
    Ok(creator)
}

#[must_use = "retain original attachment custody through publication or reconciliation"]
pub struct PreparedSandboxAttachment<'a> {
    store: &'a SessionStore,
    actor: PrincipalId,
    receipt: ConstructionPreparation,
    barrier: SourceMutationBarrier,
    observation: SandboxAttachment,
}

impl<'a> PreparedSandboxAttachment<'a> {
    fn boundary(&self) -> &AttachmentBoundary {
        match &self.receipt.candidate {
            ConstructionCandidate::SandboxAttachment { boundary, .. } => boundary,
            _ => unreachable!("validated attachment"),
        }
    }
    pub fn observation(&self) -> &SandboxAttachment {
        &self.observation
    }
    pub fn source(&self) -> &PreparedSourceOperation {
        &self
            .receipt
            .sources
            .as_ref()
            .expect("validated attachment")
            .sources[0]
            .prepared
    }

    fn require_current_in(
        &self,
        tx: &Transaction<'_>,
        publication: bool,
    ) -> Result<SourceDriverRegistryReader> {
        construction_receipt::require_unpublished_sources_in(
            tx,
            &self.actor,
            &self.receipt,
            &self.barrier,
        )?;
        let boundary = self.boundary();
        let sandbox = &self.observation.snapshot().canonical_ref;
        let current =
            observe_managed_in(self.store, tx, &self.actor, sandbox, boundary.expected_rev)?;
        if current != self.observation.layout
            || (publication && current.writer_epoch != boundary.writer_epoch)
        {
            return Err(Error::Conflict(
                "attachment authority or retained layout changed",
            ));
        }
        let registry = self.store.inner.source_drivers.reader()?;
        boundary.require_compatible(&registry)?;
        registry.resolve(&self.source().descriptor.pin()?)?;
        Ok(registry)
    }

    fn inspection_registry(&self, publication: bool) -> Result<SourceDriverRegistryReader> {
        let mut connection = self.store.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let registry = self.require_current_in(&tx, publication)?;
        db(tx.commit())?;
        Ok(registry)
    }

    pub fn revalidate(&self) -> Result<()> {
        self.inspection_registry(true).map(|_| ())
    }

    pub fn mark_unknown(self) -> Result<()> {
        construction_receipt::mark_sources_unknown(
            self.store,
            &self.actor,
            &self.receipt,
            &self.barrier,
        )
    }

    pub fn inspect(
        self,
        inspect: impl FnOnce(
            &SourceDriverRegistryReader,
            &SandboxAttachment,
            &PreparedSourceOperation,
        ) -> Result<RecoveryInspection<SourceBinding>>,
    ) -> Result<InspectedSandboxAttachment<'a>> {
        let result = (|| {
            let registry = self.inspection_registry(false)?;
            let result = inspect(&registry, &self.observation, self.source())?;
            let (mut bindings, mut evidence) = construction_receipt::validate_inspected_bindings(
                &registry,
                &self.boundary().baseline,
                vec![result],
            )?;
            self.inspection_registry(false)?;
            Ok((
                bindings.pop().expect("one inspected binding"),
                evidence.pop().expect("one inspected proof"),
            ))
        })();
        match result {
            Ok((binding, evidence)) => Ok(InspectedSandboxAttachment {
                attach: self,
                binding,
                evidence,
            }),
            Err(error) => {
                self.mark_unknown()?;
                Err(error)
            }
        }
    }

    /// Reconcile the private candidate. For replacement, additionally
    /// prove the original published materialization is intact; an
    /// unresolved post-release gap cannot be rejected.
    pub fn abandon<F>(
        self,
        reconcile: impl FnOnce(
            &SourceDriverRegistryReader,
            &SandboxAttachment,
            &PreparedSourceOperation,
        ) -> Result<F>,
    ) -> Result<()> {
        construction_receipt::reconcile_unpublished_sources(
            self.store,
            &self.actor,
            &self.receipt,
            &self.barrier,
            || {
                let registry = self.inspection_registry(false)?;
                reconcile(&registry, &self.observation, self.source())
            },
        )
    }
}

#[must_use = "publish or reconcile original attachment"]
pub struct InspectedSandboxAttachment<'a> {
    attach: PreparedSandboxAttachment<'a>,
    binding: SourceBinding,
    evidence: ExternalEvidence,
}

impl InspectedSandboxAttachment<'_> {
    /// Verify the exact incoming materialization and, for replacement,
    /// positive release of the original descriptor. The returned live
    /// whole-set fence covers both through the one binding publication.
    pub fn publish<F: SourcePublicationFence>(
        self,
        verify: impl FnOnce(
            &SourceDriverRegistryReader,
            &SandboxAttachment,
            &PreparedSourceOperation,
            &SourceBinding,
            &ExternalEvidence,
        ) -> Result<SourcePublicationProof<F>>,
    ) -> Result<SandboxSnapshot> {
        let attach = &self.attach;
        let result = (|| {
            let registry = attach.inspection_registry(true)?;
            let proof = verify(
                &registry,
                &attach.observation,
                attach.source(),
                &self.binding,
                &self.evidence,
            )?;
            construction_receipt::publish_sources(
                attach.store,
                &attach.actor,
                &attach.receipt,
                &attach.barrier,
                std::slice::from_ref(&self.evidence),
                vec![proof],
                |tx| {
                    attach.require_current_in(tx, true)?;
                    let boundary = attach.boundary();
                    let sandbox = &attach.observation.snapshot().canonical_ref;
                    let rev = next_source_rev(boundary.expected_rev)?;
                    registry.resolve(&self.binding.pin()?)?;
                    let released = if let Some((old, original)) = attach.observation.replaced() {
                        remove_binding_in(tx, sandbox, &old.binding.name, old.binding_rev)?;
                        Some(release_event_id(sandbox, original)?)
                    } else {
                        None
                    };
                    insert_at_revision(tx, sandbox, std::slice::from_ref(&self.binding), rev)?;
                    let event = attachment_event_id(sandbox, &self.binding.name, rev)?;
                    publish_source_change_in(
                        tx,
                        &attach.actor,
                        sandbox,
                        &self.binding.name,
                        boundary.expected_rev,
                        SourceChange::Attached {
                            event_id: &event,
                            prepare_id: attach.receipt.candidate.prepare_id(),
                            replaced_materialization: released.as_deref(),
                        },
                    )?;
                    Ok((
                        sandbox.clone(),
                        sandbox_owner::snapshot_with_sources(tx, sandbox)?,
                    ))
                },
            )
        })();
        match result {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                self.attach.mark_unknown()?;
                Err(error)
            }
        }
    }
}

impl SessionStore {
    pub fn sandbox_replay_attachment(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        expected_rev: u64,
        request_id: &str,
        anchor: &SourceAnchor,
    ) -> Result<Option<SandboxSnapshot>> {
        self.replay_materialization(actor, sandbox, expected_rev, request_id, anchor, false)
    }

    pub fn sandbox_replay_replacement(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        expected_rev: u64,
        request_id: &str,
        anchor: &SourceAnchor,
    ) -> Result<Option<SandboxSnapshot>> {
        self.replay_materialization(actor, sandbox, expected_rev, request_id, anchor, true)
    }

    fn replay_materialization(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        expected_rev: u64,
        request_id: &str,
        anchor: &SourceAnchor,
        replace: bool,
    ) -> Result<Option<SandboxSnapshot>> {
        owner(self)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction())?;
        let (_, receipt) = prior(&tx, actor, sandbox, request_id)?;
        let result = match receipt {
            None => None,
            Some(receipt) => {
                if receipt.fingerprint != fingerprint(sandbox, expected_rev, anchor, replace)? {
                    return Err(Error::Conflict("attachment request differs from original"));
                }
                if receipt.state != PreparationState::Applied
                    || receipt.result_ref.as_deref() != Some(sandbox)
                {
                    return Err(Error::Conflict(
                        "attachment already claimed; inspect or abandon",
                    ));
                }
                Some(sandbox_owner::snapshot_with_sources(&tx, sandbox)?)
            }
        };
        db(tx.commit())?;
        Ok(result)
    }

    /// Initial realization: nonempty original managed layout and one
    /// absent named source. Read-only plan under independent authority;
    /// persist Prepared and complete old+incoming source claim together.
    pub fn sandbox_prepare_attachment(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        expected_rev: u64,
        request_id: &str,
        anchor: SourceAnchor,
        plan: impl FnOnce(&SandboxAttachment, &str) -> Result<PreparedSourceOperation>,
    ) -> Result<PreparedSandboxAttachment<'_>> {
        self.prepare_materialization(
            actor,
            sandbox,
            expected_rev,
            request_id,
            (anchor, false),
            plan,
        )
    }

    /// Replace the occupied name using the same original materialization
    /// receipt. The trusted host must reconcile disposal of the old exact
    /// descriptor and hold its proof together with the incoming proof.
    pub fn sandbox_prepare_replacement(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        expected_rev: u64,
        request_id: &str,
        anchor: SourceAnchor,
        plan: impl FnOnce(&SandboxAttachment, &str) -> Result<PreparedSourceOperation>,
    ) -> Result<PreparedSandboxAttachment<'_>> {
        self.prepare_materialization(
            actor,
            sandbox,
            expected_rev,
            request_id,
            (anchor, true),
            plan,
        )
    }

    fn prepare_materialization(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        expected_rev: u64,
        request_id: &str,
        selection: (SourceAnchor, bool),
        plan: impl FnOnce(&SandboxAttachment, &str) -> Result<PreparedSourceOperation>,
    ) -> Result<PreparedSandboxAttachment<'_>> {
        let (anchor, replace) = selection;
        owner(self)?;
        anchor.validate()?;
        let (creator, observation, scope) = {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let creator = new_request(&tx, actor, sandbox, request_id)?;
            let layout = observe_managed_in(self, &tx, actor, sandbox, expected_rev)?;
            self.source_set_limit()
                .admit(layout.bindings.len() + usize::from(!replace))?;
            if layout
                .bindings
                .iter()
                .any(|binding| binding.binding.name == anchor.name)
                != replace
            {
                return Err(Error::Conflict("source materialization requires the selected name to match its attach/replace mode"));
            }
            self.inner
                .source_drivers
                .reader()?
                .resolve(&anchor.pin()?)?;
            SourceMutationSet::with_incoming_anchor(
                layout.bindings.iter().map(|b| &b.binding),
                &anchor,
            )?
            .require_unleased_in(&tx)?;
            let scope = StoreScope::load(&tx)?;
            db(tx.commit())?;
            (
                creator,
                SandboxAttachment {
                    layout,
                    baseline: SourceSet {
                        coverage: SourceCoverage::Selected,
                        anchors: vec![anchor],
                    },
                },
                scope,
            )
        };
        let prepare_id = id("sbxattach")?;
        let prepared = plan(&observation, &prepare_id)?;
        let boundary = AttachmentBoundary {
            project_ref: observation.snapshot().project_ref.clone(),
            expected_rev,
            writer_epoch: observation.layout.writer_epoch,
            layout_digest: observation.snapshot().layout_digest.clone(),
            bindings: observation.bindings().to_vec(),
            baseline: observation.baseline.clone(),
            replace,
        };
        let receipt = ConstructionPreparation {
            operation: ConstructionOperation::SandboxSourceAttach,
            scope: creator.clone(),
            creator,
            parent_session_id: String::new(),
            parent_branch_id: String::new(),
            request_id: request_id.to_owned(),
            fingerprint: fingerprint(sandbox, expected_rev, boundary.anchor(), replace)?,
            state: PreparationState::Prepared,
            candidate: ConstructionCandidate::SandboxAttachment {
                sandbox: sandbox.to_owned(),
                prepare_id: prepare_id.clone(),
                boundary: Box::new(boundary),
            },
            task: None,
            sources: Some(PreparedSourceSet {
                prepare_id,
                operation: SourceOperationKind::Restore,
                sources: vec![NamedPreparedSource {
                    name: observation.anchor().name.clone(),
                    prepared,
                }],
            }),
            capture_boundary: None,
            restore_boundary: None,
            sandbox_boundary: None,
            result_ref: None,
            note: None,
        };
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        if StoreScope::load(&tx)? != scope
            || new_request(&tx, actor, sandbox, request_id)? != receipt.creator
            || observe_managed_in(self, &tx, actor, sandbox, expected_rev)? != observation.layout
        {
            return Err(Error::Conflict("attachment changed during planning"));
        }
        receipt.candidate.require_absent_in(&tx)?;
        let set = SourceMutationSet::with_incoming_anchor(
            observation.bindings().iter().map(|b| &b.binding),
            observation.anchor(),
        )?;
        let barrier = construction_receipt::claim_sources_in(self, &tx, actor, &receipt, &set)?;
        db(tx.commit())?;
        Ok(PreparedSandboxAttachment {
            store: self,
            actor: actor.clone(),
            receipt,
            barrier,
            observation,
        })
    }

    fn resume_attachment(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        request_id: &str,
        abandon: bool,
    ) -> Result<Option<PreparedSandboxAttachment<'_>>> {
        owner(self)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (_, receipt) = prior(&tx, actor, sandbox, request_id)?;
        let receipt = receipt.ok_or(Error::Denied("original attachment unavailable"))?;
        if abandon && receipt.state == PreparationState::Rejected {
            return Ok(None);
        }
        if !receipt.state.unresolved() {
            return Err(Error::Conflict("attachment already terminal"));
        }
        let ConstructionCandidate::SandboxAttachment { boundary, .. } = &receipt.candidate else {
            return Err(Error::Storage("attachment boundary unavailable"));
        };
        receipt.candidate.require_absent_in(&tx)?;
        let layout = observe_managed_in(self, &tx, actor, sandbox, boundary.expected_rev)?;
        let registry = self.inner.source_drivers.reader()?;
        boundary.require_compatible(&registry)?;
        let sources = receipt
            .sources
            .as_ref()
            .ok_or(Error::Storage("attachment descriptor missing"))?;
        registry.resolve(&sources.sources[0].prepared.descriptor.pin()?)?;
        let barrier = self.recover_source_mutation_barrier_in(
            &tx,
            &boundary.mutation_set()?,
            receipt.candidate.prepare_id(),
        )?;
        construction_receipt::require_unpublished_sources_in(&tx, actor, &receipt, &barrier)?;
        let observation = SandboxAttachment {
            layout,
            baseline: boundary.baseline.clone(),
        };
        db(tx.commit())?;
        Ok(Some(PreparedSandboxAttachment {
            store: self,
            actor: actor.clone(),
            receipt,
            barrier,
            observation,
        }))
    }

    /// Reclaim the original Prepared owner for explicit driver recovery.
    /// This is not permission to repeat an uncertain materialization.
    /// A continuation must prove exact absence of surviving effects first.
    pub fn sandbox_recover_attachment(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        request_id: &str,
    ) -> Result<PreparedSandboxAttachment<'_>> {
        self.resume_attachment(actor, sandbox, request_id, false)?
            .ok_or(Error::Conflict(
                "materialization has no unresolved candidate",
            ))
    }

    pub fn sandbox_inspect_attachment(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        request_id: &str,
        inspect: impl FnOnce(
            &SourceDriverRegistryReader,
            &SandboxAttachment,
            &PreparedSourceOperation,
        ) -> Result<RecoveryInspection<SourceBinding>>,
    ) -> Result<InspectedSandboxAttachment<'_>> {
        self.sandbox_recover_attachment(actor, sandbox, request_id)?
            .inspect(inspect)
    }

    pub fn sandbox_abandon_attachment<F>(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        request_id: &str,
        reconcile: impl FnOnce(
            &SourceDriverRegistryReader,
            &SandboxAttachment,
            &PreparedSourceOperation,
        ) -> Result<F>,
    ) -> Result<()> {
        match self.resume_attachment(actor, sandbox, request_id, true)? {
            Some(prepared) => prepared.abandon(reconcile),
            None => Ok(()),
        }
    }
}
