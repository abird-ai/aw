//! Restore preparation uses the immutable checkpoint owner and ordinary
//! exact-history fork authority. Candidates are private until publication.
//! Source-bearing execution stays held until realization is admitted.
use super::{history_head_in, row_in, sources_in, CheckpointLifecycle};
use crate::{
    agent_host::{authorize_project_create, parse_project_ref, snapshot_in},
    agent_owner::{self, AgentKey, AgentPermission},
    construction_receipt::{
        self, ConstructionCandidate, ConstructionOperation, ConstructionPreparation,
        PreparationState,
    },
    db, history_owner, id, label, resources, sandbox_owner, stable_user_principal,
    store_scope::StoreScope,
    user_for_principal, workspace_writer, AgentHistoryForkAccess, AgentSnapshot, Error, Result,
    SessionBinding, SessionStore, SourceMutationBarrier, SourceMutationSet,
};
use aw_agent_core::{
    ExternalEvidence, PreparedSourceSet, PrincipalId, RecoveryInspection, SourceBinding,
    SourceCoverage, SourceDriverRegistryReader, SourceOperationKind, SourcePublicationFence,
    SourcePublicationProof, SourceSet, SourceSetLimit,
};
use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Trusted lower source-bearing restore request. The destination layout is
/// independently selected by the host. Initial task and custom access will
/// use the common Agent constructor; neither is accepted by this seam yet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointRestoreRequest {
    pub checkpoint: String,
    pub project: Option<String>,
    pub title: Option<String>,
    pub access: AgentHistoryForkAccess,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointRestoreResult {
    pub agent: AgentSnapshot,
    pub source_coverage: SourceCoverage,
}

fn fingerprint<T: Serialize>(domain: &[u8], value: &T) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(crate::json(value)?.as_bytes());
    Ok(format!("{:x}", hash.finalize()))
}

fn binding_digest(binding: &SessionBinding) -> Result<String> {
    crate::validate_binding(binding)?;
    fingerprint(b"aw-restore-layout-v1\0", binding)
}

fn source_set_digest(set: &SourceSet) -> Result<String> {
    fingerprint(b"aw-restore-source-set-v1\0", set)
}

/// Reference the immutable checkpoint owner, not a second copy of its
/// retained anchors. The unresolved receipt is a retention dependency.
/// The layout digest binds trusted host selection without persisting prompt
/// content or credentials in recovery metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RestoreBoundary {
    checkpoint: String,
    source_agent_ref: String,
    history_base_id: String,
    history_head: String,
    coverage: SourceCoverage,
    source_set_digest: String,
    destination_project: String,
    destination_writer_epoch: u64,
    title: String,
    access: AgentHistoryForkAccess,
    binding_digest: String,
}

impl RestoreBoundary {
    pub(crate) fn checkpoint(&self) -> &str {
        &self.checkpoint
    }

    pub(crate) fn validate(
        &self,
        candidate: &ConstructionCandidate,
        sources: &PreparedSourceSet,
    ) -> Result<()> {
        for value in [&self.checkpoint, &self.history_base_id, &self.history_head] {
            label(value, 128)?;
        }
        label(&self.source_agent_ref, 2048)?;
        label(&self.title, 160)?;
        let project = parse_project_ref(&self.destination_project)?;
        let ConstructionCandidate::Agent {
            sandbox: Some(sandbox),
            ..
        } = candidate
        else {
            return Err(Error::Conflict(
                "restore requires fresh Agent and Sandbox candidates",
            ));
        };
        let (sandbox_project, handle) = sandbox
            .rsplit_once("/s:")
            .ok_or(Error::Storage("invalid restore Sandbox candidate"))?;
        crate::validate_agent_handle_component(handle)?;
        let valid_digest = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        if project.canonical() != sandbox_project
            || self.destination_writer_epoch == 0
            || self.coverage == SourceCoverage::None
            || !valid_digest(&self.source_set_digest)
            || !valid_digest(&self.binding_digest)
            || sources.operation != SourceOperationKind::Restore
            || sources.prepare_id != candidate.prepare_id()
        {
            return Err(Error::Conflict("invalid checkpoint restore boundary"));
        }
        Ok(())
    }

    /// Resolve the exact retained immutable owner. Retired rows can still
    /// carry recovery dependencies, but never authorize fresh publication.
    pub(crate) fn retained_sources_in(
        &self,
        connection: &Connection,
        receipt: &ConstructionPreparation,
    ) -> Result<SourceSet> {
        let row = row_in(connection, &self.checkpoint)?;
        let snapshot = &row.snapshot;
        if receipt.origin() != AgentKey::new(&row.session, &row.branch)
            || snapshot.source_agent_ref != self.source_agent_ref
            || snapshot.history_base_id != self.history_base_id
            || snapshot.history_head != self.history_head
            || snapshot.source_coverage != self.coverage
        {
            return Err(Error::Storage(
                "restore receipt no longer matches its retained checkpoint",
            ));
        }
        let (set, _) = sources_in(connection, &self.checkpoint, self.coverage)?;
        if set.anchors.is_empty() || source_set_digest(&set)? != self.source_set_digest {
            return Err(Error::Storage("restore checkpoint source set changed"));
        }
        let prepared = receipt
            .sources
            .as_ref()
            .ok_or(Error::Storage("restore descriptors missing"))?;
        require_matching_sources(&set, prepared)?;
        Ok(set)
    }
}

fn require_matching_sources(set: &SourceSet, prepared: &PreparedSourceSet) -> Result<()> {
    prepared.validate()?;
    if prepared.operation != SourceOperationKind::Restore
        || set.anchors.len() != prepared.sources.len()
        || set
            .anchors
            .iter()
            .zip(&prepared.sources)
            .any(|(anchor, source)| {
                anchor.name != source.name || anchor.driver != source.prepared.descriptor.driver
            })
    {
        return Err(Error::Conflict(
            "restore descriptors do not match the complete checkpoint SourceSet",
        ));
    }
    Ok(())
}

struct RestoreObservation {
    session: String,
    branch: String,
    boundary: RestoreBoundary,
    sources: SourceSet,
    history: history_owner::ResolvedHistorySource,
}

fn observe_in(
    connection: &Connection,
    actor: &PrincipalId,
    request: &CheckpointRestoreRequest,
    binding_digest: &str,
    limit: SourceSetLimit,
    registry: &SourceDriverRegistryReader,
) -> Result<RestoreObservation> {
    let row = row_in(connection, &request.checkpoint)?;
    let origin = AgentKey::new(&row.session, &row.branch);
    agent_owner::authorize_permission(connection, actor, origin, AgentPermission::View)?;
    let snapshot = row.snapshot;
    if snapshot.lifecycle != CheckpointLifecycle::Live
        || agent_owner::canonical_ref(connection, origin)? != snapshot.source_agent_ref
        || history_head_in(connection, &snapshot.history_base_id)? != snapshot.history_head
    {
        return Err(Error::Conflict(
            "checkpoint restore requires an exact live history boundary",
        ));
    }
    let (source_project, _) =
        snapshot
            .source_agent_ref
            .rsplit_once("/a:")
            .ok_or(Error::Storage(
                "checkpoint source Agent has invalid identity",
            ))?;
    let project = parse_project_ref(request.project.as_deref().unwrap_or(source_project))?;
    authorize_project_create(connection, actor, &project)?;
    // Group handles are Domain-scoped. Do not reinterpret inherited ACL
    // identities in a different Domain.
    if request.access == AgentHistoryForkAccess::Inherit
        && parse_project_ref(source_project)?.domain != project.domain
    {
        return Err(Error::Denied(
            "inherited checkpoint access requires the source Domain",
        ));
    }
    let writer = workspace_writer::writer_row(connection, &project.project)?;
    let source = history_owner::resolve_retained_history_source_in(
        connection,
        actor,
        origin,
        &snapshot.history_base_id,
    )?;
    history_owner::preflight_resolved_history_in_project(
        connection,
        actor,
        &source,
        request.access,
        &stable_user_principal(&project.identity)?,
        &project.project,
    )?;

    let (sources, _) = sources_in(connection, &request.checkpoint, snapshot.source_coverage)?;
    sources.admit(limit)?;
    if sources.anchors.is_empty() {
        return Err(Error::Conflict(
            "history-only checkpoint requires fresh baseline restore",
        ));
    }
    for anchor in &sources.anchors {
        registry.resolve(&anchor.pin()?)?;
    }
    let title = request.title.as_deref().unwrap_or("Restored Agent");
    label(title, 160)?;
    let boundary = RestoreBoundary {
        checkpoint: request.checkpoint.clone(),
        source_agent_ref: snapshot.source_agent_ref,
        history_base_id: snapshot.history_base_id,
        history_head: snapshot.history_head,
        coverage: sources.coverage,
        source_set_digest: source_set_digest(&sources)?,
        destination_project: project.canonical(),
        destination_writer_epoch: writer.writer_epoch,
        title: title.to_owned(),
        access: request.access,
        binding_digest: binding_digest.to_owned(),
    };
    Ok(RestoreObservation {
        session: row.session,
        branch: row.branch,
        boundary,
        sources,
        history: source,
    })
}

fn require_new_request(
    connection: &Connection,
    creator: &str,
    request_id: &str,
    fingerprint: &str,
) -> Result<()> {
    if let Some(receipt) = construction_receipt::lookup_in(
        connection,
        ConstructionOperation::CheckpointRestore,
        creator,
        request_id,
    )? {
        if receipt.fingerprint != fingerprint {
            return Err(Error::Denied(
                "checkpoint restore request identity reused with different semantics",
            ));
        }
        return Err(Error::Conflict(
            "checkpoint restore is non-retryable; inspect original receipt",
        ));
    }
    Ok(())
}

fn require_candidate_absent(
    connection: &Connection,
    candidate: &ConstructionCandidate,
) -> Result<()> {
    let ConstructionCandidate::Agent {
        session,
        root,
        branch,
        sandbox: Some(sandbox),
    } = candidate
    else {
        return Err(Error::Storage(
            "restore candidate is not a fresh Agent and Sandbox",
        ));
    };
    let exists: bool = db(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1)
             OR EXISTS(SELECT 1 FROM nodes WHERE id=?2)
             OR EXISTS(SELECT 1 FROM branches WHERE id=?3)
             OR EXISTS(SELECT 1 FROM sandboxes_current WHERE sandbox_id=?4)",
        params![session, root, branch, sandbox],
        |row| row.get(0),
    ))?;
    if exists {
        return Err(Error::Conflict("restore candidate already exists"));
    }
    Ok(())
}

/// Live Prepared restore. This is recovery custody, not permission to
/// execute arbitrary source effects. The host must separately authorize
/// the complete anchor set and exact destination materializations.
#[must_use = "retain the prepared restore until publication or recovery"]
pub struct PreparedCheckpointRestore<'a> {
    store: &'a SessionStore,
    actor: PrincipalId,
    receipt: ConstructionPreparation,
    anchors: SourceSet,
    barrier: SourceMutationBarrier<'a>,
}

impl<'a> PreparedCheckpointRestore<'a> {
    fn boundary(&self) -> &RestoreBoundary {
        self.receipt
            .restore_boundary
            .as_ref()
            .expect("constructed with restore boundary")
    }

    pub fn prepare_id(&self) -> &str {
        self.receipt.candidate.prepare_id()
    }

    pub fn sandbox_ref(&self) -> &str {
        match &self.receipt.candidate {
            ConstructionCandidate::Agent {
                sandbox: Some(sandbox),
                ..
            } => sandbox,
            _ => unreachable!("constructed with fresh Sandbox"),
        }
    }

    pub fn anchors(&self) -> &SourceSet {
        &self.anchors
    }

    pub fn sources(&self) -> &PreparedSourceSet {
        self.receipt
            .sources
            .as_ref()
            .expect("constructed with sources")
    }

    /// Check the exact retained history, destination layout, current
    /// authorization and complete lease set. Publication must repeat this
    /// inside its own final transaction.
    pub fn revalidate(&self, binding: &SessionBinding) -> Result<()> {
        let mut connection = self.store.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        self.require_current_in(&tx, binding)?;
        db(tx.rollback())
    }

    fn require_current_in(
        &self,
        tx: &Transaction<'_>,
        binding: &SessionBinding,
    ) -> Result<SourceDriverRegistryReader> {
        construction_receipt::require_current_sources_in(
            tx,
            &self.actor,
            &self.receipt,
            &self.barrier,
        )?;
        let registry = self.store.inner.source_drivers.reader()?;
        self.require_current_boundary_in(tx, binding, &registry)?;
        Ok(registry)
    }

    fn require_current_boundary_in(
        &self,
        tx: &Transaction<'_>,
        binding: &SessionBinding,
        registry: &SourceDriverRegistryReader,
    ) -> Result<history_owner::ResolvedHistorySource> {
        let boundary = self.boundary();
        let observed = observe_in(
            tx,
            &self.actor,
            &CheckpointRestoreRequest {
                checkpoint: boundary.checkpoint.clone(),
                project: Some(boundary.destination_project.clone()),
                title: Some(boundary.title.clone()),
                access: boundary.access,
            },
            &binding_digest(binding)?,
            self.store.inner.source_limit,
            registry,
        )?;
        if observed.boundary != *boundary
            || observed.sources != self.anchors
            || self.receipt.origin() != AgentKey::new(&observed.session, &observed.branch)
        {
            return Err(Error::Conflict("checkpoint restore boundary changed"));
        }
        for source in &self.sources().sources {
            registry.resolve(&source.prepared.descriptor.pin()?)?;
        }
        require_candidate_absent(tx, &self.receipt.candidate)?;
        sandbox_owner::require_fresh_layout_in(tx, &boundary.destination_project, binding)?;
        Ok(observed.history)
    }

    pub fn mark_unknown(self) -> Result<()> {
        construction_receipt::mark_sources_unknown(
            self.store,
            &self.actor,
            &self.receipt,
            &self.barrier,
        )
    }

    fn inspection_registry(&self) -> Result<SourceDriverRegistryReader> {
        let mut connection = self.store.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        construction_receipt::require_current_sources_in(
            &tx,
            &self.actor,
            &self.receipt,
            &self.barrier,
        )?;
        let registry = self.store.inner.source_drivers.reader()?;
        let anchors = inspection_sources_in(&tx, &self.actor, &self.receipt, &registry)?;
        if anchors != self.anchors {
            return Err(Error::Storage("restore inspection source set changed"));
        }
        db(tx.commit())?;
        Ok(registry)
    }

    /// Inspect the original private materializations. No execution or
    /// retry occurs here. A complete result must preserve captured layout
    /// metadata and exact stable source identities.
    pub fn inspect(
        self,
        inspect: impl FnOnce(
            &SourceDriverRegistryReader,
            &SourceSet,
            &PreparedSourceSet,
        ) -> Result<Vec<RecoveryInspection<SourceBinding>>>,
    ) -> Result<InspectedCheckpointRestore<'a>> {
        let result = (|| {
            let registry = self.inspection_registry()?;
            let results = inspect(&registry, &self.anchors, self.sources())?;
            if results.len() != self.anchors.anchors.len() {
                return Err(Error::Conflict(
                    "restore inspection requires the complete source set",
                ));
            }
            let mut bindings = Vec::with_capacity(results.len());
            let mut evidence = Vec::with_capacity(results.len());
            for (anchor, result) in self.anchors.anchors.iter().zip(results) {
                let RecoveryInspection::Completed {
                    result: binding,
                    evidence: proof,
                } = result
                else {
                    return Err(Error::Conflict(
                        "restore materialization is unresolved; inspect before retry",
                    ));
                };
                binding.validate()?;
                proof.validate()?;
                if binding.name != anchor.name
                    || binding.driver != anchor.driver
                    || binding.source_ref != anchor.source_ref
                    || binding.access != anchor.access
                    || binding.checkpoint_policy != anchor.checkpoint_policy
                {
                    return Err(Error::Conflict(
                        "restored binding differs from its checkpoint anchor",
                    ));
                }
                registry.resolve(&binding.pin()?)?;
                bindings.push(binding);
                evidence.push(proof);
            }
            let registry = self.inspection_registry()?;
            for binding in &bindings {
                registry.resolve(&binding.pin()?)?;
            }
            Ok((bindings, evidence))
        })();
        match result {
            Ok((bindings, evidence)) => Ok(InspectedCheckpointRestore {
                restore: self,
                bindings,
                evidence,
            }),
            Err(error) => {
                self.mark_unknown()?;
                Err(error)
            }
        }
    }
}

/// Exact inspected candidate. The evidence is not a live external fence.
/// Publication requires a trusted verifier; the resulting Agent starts
/// held until source-aware execution admission can be enabled.
#[must_use = "retain the inspected restore until publication or recovery"]
pub struct InspectedCheckpointRestore<'a> {
    restore: PreparedCheckpointRestore<'a>,
    bindings: Vec<SourceBinding>,
    evidence: Vec<ExternalEvidence>,
}

impl InspectedCheckpointRestore<'_> {
    pub fn prepare_id(&self) -> &str {
        self.restore.prepare_id()
    }
    pub fn sandbox_ref(&self) -> &str {
        self.restore.sandbox_ref()
    }
    pub fn bindings(&self) -> &[SourceBinding] {
        &self.bindings
    }
    pub fn evidence(&self) -> &[ExternalEvidence] {
        &self.evidence
    }
    pub fn revalidate(&self, binding: &SessionBinding) -> Result<()> {
        self.restore.revalidate(binding)
    }
    pub fn mark_unknown(self) -> Result<()> {
        self.restore.mark_unknown()
    }

    /// Publish the exact fresh Agent and restored Sandbox in the common
    /// construction transaction. The verifier must revalidate every
    /// original descriptor, fresh materialization, destination isolation and
    /// current external source authority, returning actual immutable proof or
    /// provider guards valid through commit. SourceBinding metadata alone
    /// never grants execution, and no credentials or prior grants are copied.
    pub fn publish<F: SourcePublicationFence>(
        self,
        binding: &SessionBinding,
        verify: impl FnOnce(
            &SourceDriverRegistryReader,
            &SourceSet,
            &PreparedSourceSet,
            &[SourceBinding],
            &[ExternalEvidence],
        ) -> Result<Vec<SourcePublicationProof<F>>>,
    ) -> Result<CheckpointRestoreResult> {
        let result = (|| {
            let restore = &self.restore;
            let registry = {
                let mut connection = restore.store.connection()?;
                let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
                let registry = restore.require_current_in(&tx, binding)?;
                db(tx.rollback())?;
                registry
            };
            let proofs = verify(
                &registry,
                &restore.anchors,
                restore.sources(),
                &self.bindings,
                &self.evidence,
            )?;
            construction_receipt::publish_sources(
                restore.store,
                &restore.actor,
                &restore.receipt,
                &restore.barrier,
                &self.evidence,
                proofs,
                |tx| {
                    let history = restore.require_current_boundary_in(tx, binding, &registry)?;
                    let boundary = restore.boundary();
                    let project = parse_project_ref(&boundary.destination_project)?;
                    let fresh = sandbox_owner::publish_fresh_in(
                        tx,
                        &restore.actor,
                        restore.sandbox_ref(),
                        binding,
                        &self.bindings,
                        &restore.store.inner.source_drivers,
                        restore.store.inner.source_limit,
                    )?;
                    let (session, branch) =
                        history_owner::create_agent_from_resolved_history_with_ids_in(
                            tx,
                            &restore.actor,
                            &history,
                            &boundary.title,
                            binding,
                            boundary.access,
                            &stable_user_principal(&project.identity)?,
                            &project.project,
                            restore.receipt.candidate.agent_ids()?,
                            Some(&fresh),
                        )?;
                    let reference =
                        agent_owner::canonical_ref(tx, AgentKey::new(&session.id, &branch.id))?;
                    let agent = snapshot_in(tx, &restore.actor, &reference)?;
                    Ok((
                        reference,
                        CheckpointRestoreResult {
                            agent,
                            source_coverage: boundary.coverage,
                        },
                    ))
                },
            )
        })();
        match result {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                self.restore.mark_unknown()?;
                Err(error)
            }
        }
    }
}

fn inspection_sources_in(
    tx: &Transaction<'_>,
    actor: &PrincipalId,
    receipt: &ConstructionPreparation,
    registry: &SourceDriverRegistryReader,
) -> Result<SourceSet> {
    receipt.require_actor(tx, actor)?;
    if receipt.operation != ConstructionOperation::CheckpointRestore || !receipt.state.unresolved()
    {
        return Err(Error::Conflict("checkpoint restore is not unresolved"));
    }
    let boundary = receipt
        .restore_boundary
        .as_ref()
        .ok_or(Error::Conflict("restore has no exact recovery boundary"))?;
    agent_owner::authorize_permission(tx, actor, receipt.origin(), AgentPermission::View)?;
    if agent_owner::canonical_ref(tx, receipt.origin())? != boundary.source_agent_ref {
        return Err(Error::Conflict("restore source identity changed"));
    }
    authorize_project_create(
        tx,
        actor,
        &parse_project_ref(&boundary.destination_project)?,
    )?;
    let anchors = boundary.retained_sources_in(tx, receipt)?;
    for (anchor, source) in anchors.anchors.iter().zip(
        &receipt
            .sources
            .as_ref()
            .ok_or(Error::Storage("restore descriptors missing"))?
            .sources,
    ) {
        registry.resolve(&anchor.pin()?)?;
        registry.resolve(&source.prepared.descriptor.pin()?)?;
    }
    require_candidate_absent(tx, &receipt.candidate)?;
    Ok(anchors)
}

impl SessionStore {
    /// Lookup the original restore invocation before contextual resolution
    /// or current effect admission. Only current destination Agent view
    /// authorizes the protected result; unresolved custody never replans.
    pub fn checkpoint_replay_restore(
        &self,
        actor: &PrincipalId,
        request: &CheckpointRestoreRequest,
        request_id: &str,
    ) -> Result<Option<CheckpointRestoreResult>> {
        label(&request.checkpoint, 128)?;
        label(request_id, 256)?;
        if let Some(project) = &request.project {
            label(project, 2048)?;
        }
        if let Some(title) = &request.title {
            label(title, 160)?;
        }
        let fingerprint = fingerprint(b"aw-checkpoint-restore-v1\0", request)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction())?;
        let creator = user_for_principal(&tx, actor)?
            .ok_or(Error::Denied("checkpoint restore caller unavailable"))?;
        let Some(receipt) = construction_receipt::lookup_in(
            &tx,
            ConstructionOperation::CheckpointRestore,
            &creator,
            request_id,
        )?
        else {
            return Ok(None);
        };
        if receipt.fingerprint != fingerprint {
            return Err(Error::Denied(
                "checkpoint restore request identity reused with different semantics",
            ));
        }
        if receipt.state != PreparationState::Applied {
            return Err(Error::Conflict(
                "checkpoint restore is non-retryable; inspect original receipt",
            ));
        }
        let reference = receipt
            .result_ref
            .as_deref()
            .ok_or(Error::Storage("applied restore lost its Agent identity"))?;
        let agent = match snapshot_in(&tx, actor, reference) {
            Err(Error::Denied(_)) => {
                return Err(Error::Denied(
                    "prior invocation is non-retryable; result unavailable",
                ))
            }
            result => result?,
        };
        let boundary = receipt
            .restore_boundary
            .as_ref()
            .ok_or(Error::Storage("applied restore lost its source coverage"))?;
        let ConstructionCandidate::Agent { sandbox, .. } = &receipt.candidate else {
            return Err(Error::Storage("applied restore lost its candidate"));
        };
        if &agent.sandbox_ref != sandbox {
            return Err(Error::Storage("restored Agent Sandbox identity changed"));
        }
        let result = CheckpointRestoreResult {
            agent,
            source_coverage: boundary.coverage,
        };
        db(tx.commit())?;
        Ok(Some(result))
    }

    /// Trusted read-only planning followed by an atomic Prepared claim and
    /// complete source barrier. The planner receives the private candidate
    /// session/preparation ID, fresh Sandbox ref and exact retained anchors.
    /// It must independently authorize source disclosure/materialization
    /// before planning. No external effect may begin until this returns.
    pub fn checkpoint_prepare_restore(
        &self,
        actor: &PrincipalId,
        request: &CheckpointRestoreRequest,
        binding: &SessionBinding,
        request_id: &str,
        plan: impl FnOnce(&str, &str, &SourceSet) -> Result<PreparedSourceSet>,
    ) -> Result<PreparedCheckpointRestore<'_>> {
        label(&request.checkpoint, 128)?;
        label(request_id, 256)?;
        if let Some(project) = &request.project {
            label(project, 2048)?;
        }
        if let Some(title) = &request.title {
            label(title, 160)?;
        }
        if self.inner.mode != crate::StoreMode::OwnerLock {
            return Err(Error::Denied(
                "checkpoint restore requires the exclusive persistent Store owner",
            ));
        }
        let fingerprint = fingerprint(b"aw-checkpoint-restore-v1\0", request)?;
        let limit = self.inner.source_limit;
        let (creator, observed, store_scope) = {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let creator = user_for_principal(&tx, actor)?
                .ok_or(Error::Denied("checkpoint restore caller unavailable"))?;
            require_new_request(&tx, &creator, request_id, &fingerprint)?;
            let registry = self.inner.source_drivers.reader()?;
            let observed = observe_in(
                &tx,
                actor,
                request,
                &binding_digest(binding)?,
                limit,
                &registry,
            )?;
            sandbox_owner::require_fresh_layout_in(
                &tx,
                &observed.boundary.destination_project,
                binding,
            )?;
            SourceMutationSet::from_anchors(&observed.sources.anchors)?.require_unleased_in(&tx)?;
            let store_scope = StoreScope::load(&tx)?;
            db(tx.commit())?;
            (creator, observed, store_scope)
        };
        let ids = resources::mint_session_candidate_ids()?;
        let sandbox = format!("{}/s:{}", observed.boundary.destination_project, id("sbx")?);
        let mut sources = plan(&ids.session, &sandbox, &observed.sources)?;
        sources.admit(limit)?;
        sources
            .sources
            .sort_unstable_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
        require_matching_sources(&observed.sources, &sources)?;
        let candidate = ConstructionCandidate::Agent {
            session: ids.session,
            root: ids.root,
            branch: ids.branch,
            sandbox: Some(sandbox),
        };
        observed.boundary.validate(&candidate, &sources)?;
        let set = SourceMutationSet::from_anchors(&observed.sources.anchors)?;
        let receipt = ConstructionPreparation {
            operation: ConstructionOperation::CheckpointRestore,
            scope: creator.clone(),
            creator,
            parent_session_id: observed.session,
            parent_branch_id: observed.branch,
            request_id: request_id.to_owned(),
            fingerprint,
            state: PreparationState::Prepared,
            candidate,
            task: None,
            sources: Some(sources),
            capture_boundary: None,
            restore_boundary: Some(observed.boundary),
            result_ref: None,
            note: None,
        };

        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        if StoreScope::load(&tx)? != store_scope {
            return Err(Error::Conflict(
                "Store generation changed during restore planning",
            ));
        }
        receipt.require_actor(&tx, actor)?;
        require_new_request(&tx, &receipt.creator, request_id, &receipt.fingerprint)?;
        let registry = self.inner.source_drivers.reader()?;
        let current = observe_in(
            &tx,
            actor,
            request,
            &binding_digest(binding)?,
            limit,
            &registry,
        )?;
        if receipt.restore_boundary.as_ref() != Some(&current.boundary)
            || observed.sources != current.sources
            || receipt.origin() != AgentKey::new(&current.session, &current.branch)
        {
            return Err(Error::Conflict(
                "checkpoint restore boundary changed during planning",
            ));
        }
        require_candidate_absent(&tx, &receipt.candidate)?;
        sandbox_owner::require_fresh_layout_in(
            &tx,
            &current.boundary.destination_project,
            binding,
        )?;
        let barrier = construction_receipt::claim_sources_in(self, &tx, actor, &receipt, &set)?;
        db(tx.commit())?;
        Ok(PreparedCheckpointRestore {
            store: self,
            actor: actor.clone(),
            receipt,
            anchors: observed.sources,
            barrier,
        })
    }

    /// Recover only the original operation's exact hazard set and inspect
    /// its retained descriptors. Never create a new candidate on retry.
    pub fn checkpoint_inspect_restore(
        &self,
        actor: &PrincipalId,
        request_id: &str,
        inspect: impl FnOnce(
            &SourceDriverRegistryReader,
            &SourceSet,
            &PreparedSourceSet,
        ) -> Result<Vec<RecoveryInspection<SourceBinding>>>,
    ) -> Result<InspectedCheckpointRestore<'_>> {
        label(request_id, 256)?;
        if self.inner.mode != crate::StoreMode::OwnerLock {
            return Err(Error::Denied(
                "checkpoint restore recovery requires the exclusive Store owner",
            ));
        }
        let restore = {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let creator = user_for_principal(&tx, actor)?
                .ok_or(Error::Denied("checkpoint restore caller unavailable"))?;
            let receipt = construction_receipt::lookup_in(
                &tx,
                ConstructionOperation::CheckpointRestore,
                &creator,
                request_id,
            )?
            .ok_or(Error::Denied("checkpoint restore receipt unavailable"))?;
            let registry = self.inner.source_drivers.reader()?;
            let anchors = inspection_sources_in(&tx, actor, &receipt, &registry)?;
            let set = SourceMutationSet::from_anchors(&anchors.anchors)?;
            let barrier =
                self.recover_source_mutation_barrier_in(&tx, &set, receipt.candidate.prepare_id())?;
            db(tx.commit())?;
            PreparedCheckpointRestore {
                store: self,
                actor: actor.clone(),
                receipt,
                anchors,
                barrier,
            }
        };
        restore.inspect(inspect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint_owner::{publish_in, Publication};
    use aw_agent_core::{
        AccountOwner, BillingOwner, CheckpointPolicy, CredentialCustody, DriverKey,
        InvocationIdentity, NamedPreparedSource, Outcome, PreparedSourceOperation,
        RecoveryDescriptor, RecoveryField, RecoveryFieldKind, SchemaSupport, SourceAccess,
        SourceAnchor, SourceDriverRegistry, SourceDriverSchemas, SourceName, Turn, TurnJournal,
        TurnStart,
    };
    use std::path::PathBuf;

    struct PrivateTemp(PathBuf);
    impl Drop for PrivateTemp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn plan(prepare_id: &str, sandbox: &str, anchors: &SourceSet) -> Result<PreparedSourceSet> {
        Ok(PreparedSourceSet {
            prepare_id: prepare_id.to_owned(),
            operation: SourceOperationKind::Restore,
            sources: anchors
                .anchors
                .iter()
                .rev()
                .map(|anchor| {
                    Ok(NamedPreparedSource {
                        name: anchor.name.clone(),
                        prepared: PreparedSourceOperation::new(
                            SourceOperationKind::Restore,
                            RecoveryDescriptor::new(
                                anchor.driver.clone(),
                                1,
                                prepare_id,
                                format!("{prepare_id}:{}", anchor.name.as_str()),
                                vec![RecoveryField::new(
                                    "target",
                                    RecoveryFieldKind::Reference,
                                    format!("{sandbox}:{}", anchor.name.as_str()),
                                )?],
                            )?,
                        )?,
                    })
                })
                .collect::<Result<Vec<_>>>()?,
        })
    }

    #[tokio::test]
    async fn restore_preparation_and_recovery_retain_exact_checkpoint_and_private_candidates() {
        let temp = PrivateTemp(std::env::temp_dir().join(id("aw-restore-prepare").unwrap()));
        let actor = PrincipalId::new("internal:fixture:restore-owner").unwrap();
        let driver = DriverKey::builtin("fixture-restore").unwrap();
        let schema = SchemaSupport::new(1, 1).unwrap();
        let registry = SourceDriverRegistry::new();
        registry
            .register(
                driver.clone(),
                SourceDriverSchemas {
                    binding: schema,
                    anchor: schema,
                    recovery: schema,
                },
                (),
            )
            .unwrap();
        let limit = SourceSetLimit::new(2).unwrap();
        let store = SessionStore::open_with_sources(&temp.0, registry.clone(), limit).unwrap();
        let layout = SessionBinding {
            working_directory: PathBuf::from("/tmp"),
            tool_root: PathBuf::from("/tmp"),
            system_prompt: "system".into(),
            profile_hint: "fixture".into(),
            model_hint: "fixture".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (session, branch) = store
            .create(&actor, "restore origin", layout.clone())
            .unwrap();
        let origin = AgentKey::new(&session.id, &branch.id);
        let reference = agent_owner::canonical_ref(&store.connection().unwrap(), origin).unwrap();
        let original_sandbox = store
            .agent_get(&actor, &reference)
            .unwrap()
            .sandbox_ref
            .unwrap();
        let (_, handle) = store.activate(&actor, &session.id, &branch.id).unwrap();
        let identity = InvocationIdentity {
            actor: actor.clone(),
            account_id: "fixture".into(),
            account_owner: AccountOwner::User(actor.clone()),
            payer: BillingOwner::User(actor.clone()),
            credential_custody: CredentialCustody::UserDevice("fixture".into()),
            credential_version: None,
            inference_executor: "local".into(),
            model: "fixture".into(),
            grant_revision: 1,
        };
        let finish = |turn_id| Turn {
            id: turn_id,
            prompt: format!("turn {turn_id}"),
            answer: format!("answer {turn_id}"),
            model: identity.model.clone(),
            profile: identity.account_id.clone(),
            outcome: Outcome::Completed,
            meta: None,
            error: None,
            actor: Some(actor.clone()),
            tool_records: Vec::new(),
            invocations: Vec::new(),
            context_messages: Vec::new(),
            needs_reconciliation: false,
        };
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "turn 1".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        handle.finish(finish(1)).await.unwrap();
        let checkpoint = id("cp").unwrap();
        let anchors = SourceSet {
            coverage: SourceCoverage::Selected,
            anchors: ["code", "data"]
                .into_iter()
                .map(|name| SourceAnchor {
                    name: SourceName::new(name).unwrap(),
                    driver: driver.clone(),
                    anchor_schema: 1,
                    source_ref: format!("source:{name}"),
                    access: SourceAccess::ReadOnly,
                    checkpoint_policy: if name == "data" {
                        CheckpointPolicy::Ephemeral
                    } else {
                        CheckpointPolicy::Required
                    },
                    identity: format!("version:{name}"),
                    retention: format!("retention:{name}"),
                    fields: Vec::new(),
                })
                .collect(),
        };
        let (base, creator) = {
            let mut connection = store.connection().unwrap();
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            let base = history_owner::seal_in(&tx, &session.id, &branch.id).unwrap();
            let creator = user_for_principal(&tx, &actor).unwrap().unwrap();
            publish_in(
                &tx,
                &registry.reader().unwrap(),
                limit,
                Publication {
                    actor: &actor,
                    checkpoint: &checkpoint,
                    source: origin,
                    history_base_id: &base.id,
                    sources: &anchors,
                    note: None,
                    creator: &creator,
                    request_id: "host:checkpoint",
                    request_fingerprint: &"1".repeat(64),
                },
            )
            .unwrap();
            tx.commit().unwrap();
            (base, creator)
        };
        handle
            .begin(TurnStart {
                turn_id: 2,
                prompt: "turn 2".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        handle.finish(finish(2)).await.unwrap();
        drop(handle);
        let current = store
            .checkpoint_create_history_only(&actor, &reference, None, "host:later")
            .unwrap();
        assert_ne!(current.history_base_id, base.id);

        let restored_root = temp.0.join("restored-layout");
        std::fs::create_dir(&restored_root).unwrap();
        let layout = SessionBinding {
            working_directory: restored_root.clone(),
            tool_root: restored_root,
            ..layout
        };
        let request = CheckpointRestoreRequest {
            checkpoint: checkpoint.clone(),
            project: None,
            title: Some("restored".into()),
            access: AgentHistoryForkAccess::Private,
        };
        let viewer = PrincipalId::new("internal:fixture:restore-viewer").unwrap();
        let domain = store.default_domain(&actor).unwrap();
        let viewer_ref = {
            let connection = store.connection().unwrap();
            let user = crate::ensure_user_for_principal(&connection, &viewer).unwrap();
            format!("/d:{}/i:{user}", domain.id)
        };
        let rev = store
            .agent_public_revision(&actor, &session.id, &branch.id)
            .unwrap();
        store
            .agent_access_set(
                &actor,
                &session.id,
                &branch.id,
                &viewer_ref,
                Some(crate::AgentPermissions::VIEW_ONLY),
                rev,
            )
            .unwrap();
        assert!(store
            .checkpoint_prepare_restore(
                &viewer,
                &request,
                &layout,
                "host:denied",
                |_, _, _| panic!("view is not derive/destination/source authority")
            )
            .is_err());
        let mut history_only = request.clone();
        history_only.checkpoint = current.id;
        assert!(store
            .checkpoint_prepare_restore(
                &actor,
                &history_only,
                &layout,
                "host:no-sources",
                |_, _, _| panic!("no implicit source-state fallback")
            )
            .is_err());
        assert!(store
            .checkpoint_prepare_restore(
                &actor,
                &request,
                &layout,
                "host:partial",
                |candidate, sandbox, selected| {
                    let mut prepared = plan(candidate, sandbox, selected)?;
                    prepared.sources.pop();
                    Ok(prepared)
                }
            )
            .is_err());

        let prepared = store
            .checkpoint_prepare_restore(
                &actor,
                &request,
                &layout,
                "host:restore",
                |candidate, sandbox, selected| {
                    assert_eq!(selected, &anchors);
                    assert_ne!(sandbox, original_sandbox);
                    plan(candidate, sandbox, selected)
                },
            )
            .unwrap();
        prepared.revalidate(&layout).unwrap();
        // The original unresolved restore pins both retained anchors and
        // history. Retirement must not remove its last durable history root.
        assert!(store.checkpoint_delete(&actor, &checkpoint).is_err());
        let receipt = prepared.receipt.clone();
        let candidate = prepared.prepare_id().to_owned();
        let sandbox = prepared.sandbox_ref().to_owned();
        assert_ne!(candidate, session.id);
        assert_eq!(prepared.boundary().history_base_id, base.id);
        assert_eq!(prepared.anchors(), &anchors);
        assert!(store
            .checkpoint_prepare_restore(
                &actor,
                &request,
                &layout,
                "host:restore",
                |_, _, _| panic!("restore replay cannot replan")
            )
            .is_err());
        assert!(store
            .checkpoint_inspect_restore(&actor, "host:restore", |_, _, _| panic!(
                "live restore barrier cannot be stolen"
            ))
            .is_err());

        // Exercise the shared exact-history constructor with the retained
        // checkpoint base inside a rolled-back transaction. No restore-only
        // Agent constructor or public source-bearing association is added.
        {
            let mut connection = store.connection().unwrap();
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            let source =
                history_owner::resolve_retained_history_source_in(&tx, &actor, origin, &base.id)
                    .unwrap();
            let project = parse_project_ref(&prepared.boundary().destination_project).unwrap();
            let (created, branch) = history_owner::create_agent_from_resolved_history_with_ids_in(
                &tx,
                &actor,
                &source,
                "fixture",
                &layout,
                AgentHistoryForkAccess::Private,
                &stable_user_principal(&project.identity).unwrap(),
                &project.project,
                receipt.candidate.agent_ids().unwrap(),
                None,
            )
            .unwrap();
            assert_eq!(created.id, candidate);
            assert_eq!(created.next_turn, base.next_turn);
            let inherited: String = tx.query_row(
                "SELECT history_base_id FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
                params![created.id, branch.id], |row| row.get(0),
            ).unwrap();
            assert_eq!(inherited, base.id);
            tx.rollback().unwrap();
        }
        prepared.revalidate(&layout).unwrap();
        let mut changed = layout.clone();
        changed.system_prompt = "different layout context".into();
        assert!(prepared.revalidate(&changed).is_err());

        let marker = |name: &str| temp.0.join(format!("fixture-restore-{name}"));
        let write_effect = |source: &NamedPreparedSource| {
            std::fs::write(
                marker(source.name.as_str()),
                &source.prepared.descriptor.prepared_identity,
            )
            .unwrap();
        };
        write_effect(&prepared.sources().sources[0]);
        drop(prepared);
        drop(store);

        // Reopen without the driver; the exact retained anchor/recovery pins
        // survive and block inspection before any source data is dispatched.
        let store = SessionStore::open(&temp.0).unwrap();
        assert!(store
            .checkpoint_inspect_restore(&actor, "host:restore", |_, _, _| panic!(
                "missing driver must fail closed"
            ))
            .is_err());
        drop(store);
        let store = SessionStore::open_with_sources(&temp.0, registry.clone(), limit).unwrap();
        let inspect = |registry: &SourceDriverRegistryReader,
                       selected: &SourceSet,
                       prepared: &PreparedSourceSet| {
            assert_eq!(selected, &anchors);
            assert_eq!(prepared, receipt.sources.as_ref().unwrap());
            selected
                .anchors
                .iter()
                .zip(&prepared.sources)
                .map(|(anchor, source)| {
                    registry.resolve(&source.prepared.descriptor.pin()?)?;
                    let Ok(materialization) = std::fs::read_to_string(marker(source.name.as_str()))
                    else {
                        return Ok(RecoveryInspection::Unknown);
                    };
                    assert_eq!(
                        materialization,
                        source.prepared.descriptor.prepared_identity
                    );
                    RecoveryInspection::completed(
                        SourceBinding {
                            name: anchor.name.clone(),
                            driver: anchor.driver.clone(),
                            binding_schema: 1,
                            source_ref: anchor.source_ref.clone(),
                            materialization,
                            access: anchor.access,
                            checkpoint_policy: anchor.checkpoint_policy,
                        },
                        ExternalEvidence::held_fence(
                            "fixture",
                            &source.prepared.descriptor.prepared_identity,
                        )?,
                    )
                })
                .collect::<Result<Vec<_>>>()
        };
        assert!(store
            .checkpoint_inspect_restore(&actor, "host:restore", inspect)
            .is_err());
        write_effect(&receipt.sources.as_ref().unwrap().sources[1]);
        let inspected = store
            .checkpoint_inspect_restore(&actor, "host:restore", inspect)
            .unwrap();
        assert_eq!(inspected.prepare_id(), candidate);
        assert_eq!(inspected.sandbox_ref(), sandbox);
        assert_eq!(inspected.bindings().len(), 2);
        assert_eq!(
            inspected.bindings()[1].checkpoint_policy,
            CheckpointPolicy::Ephemeral
        );
        inspected.revalidate(&layout).unwrap();
        // Actual fixture provider guard held through the shared publication
        // transaction. The previous capture regression qualifies its lifetime.
        struct FixtureFence<'a> {
            evidence: ExternalEvidence,
            _guard: std::sync::RwLockReadGuard<'a, ()>,
        }
        impl SourcePublicationFence for FixtureFence<'_> {
            fn evidence(&self) -> &ExternalEvidence {
                &self.evidence
            }
        }
        let provider = std::sync::RwLock::new(());
        let verify = |registry: &SourceDriverRegistryReader,
                      selected: &SourceSet,
                      prepared: &PreparedSourceSet,
                      bindings: &[SourceBinding],
                      evidence: &[ExternalEvidence]| {
            let inspected = inspect(registry, selected, prepared)?;
            assert_eq!(bindings.len(), inspected.len());
            inspected
                .into_iter()
                .zip(bindings)
                .zip(evidence)
                .map(|((result, binding), expected)| {
                    let RecoveryInspection::Completed { result, evidence } = result else {
                        return Err(Error::Conflict("fixture materialization disappeared"));
                    };
                    assert_eq!(&result, binding);
                    assert_eq!(&evidence, expected);
                    Ok(SourcePublicationProof::Held(FixtureFence {
                        evidence,
                        _guard: provider.read().unwrap(),
                    }))
                })
                .collect::<Result<Vec<_>>>()
        };
        let sandbox_count: usize;
        {
            let connection = store.connection().unwrap();
            require_candidate_absent(&connection, &receipt.candidate).unwrap();
            sandbox_count = connection
                .query_row("SELECT count(*) FROM sandboxes_current", [], |row| {
                    row.get(0)
                })
                .unwrap();
            // Fail after the fresh Sandbox and Agent exist inside the
            // transaction, before the HistoryGrant is attached.
            connection
                .execute_batch(
                    "CREATE TEMP TRIGGER fixture_fail_restore BEFORE INSERT ON history_grants
                 BEGIN SELECT RAISE(ABORT,'fixture restore failure'); END;",
                )
                .unwrap();
        }
        assert!(inspected.publish(&layout, verify).is_err());
        {
            let connection = store.connection().unwrap();
            connection
                .execute_batch("DROP TRIGGER fixture_fail_restore")
                .unwrap();
            let current = construction_receipt::lookup_in(
                &connection,
                receipt.operation,
                &creator,
                &receipt.request_id,
            )
            .unwrap()
            .unwrap();
            assert_eq!(current.state, PreparationState::Unknown);
            assert_eq!(current.candidate, receipt.candidate);
            require_candidate_absent(&connection, &receipt.candidate).unwrap();
            assert_eq!(
                connection
                    .query_row("SELECT count(*) FROM sandboxes_current", [], |row| row
                        .get::<_, usize>(0),)
                    .unwrap(),
                sandbox_count
            );
            assert_eq!(connection.query_row(
                "SELECT count(*) FROM local_coordination_leases WHERE run_id=?1 AND state='hazard'",
                [&candidate], |row| row.get::<_, usize>(0),
            ).unwrap(), 2);
        }
        let restored = store
            .checkpoint_inspect_restore(&actor, "host:restore", inspect)
            .unwrap()
            .publish(&layout, verify)
            .unwrap();
        assert_eq!(restored.source_coverage, SourceCoverage::Selected);
        assert_eq!(
            restored.agent.sandbox_ref.as_deref(),
            Some(sandbox.as_str())
        );
        assert_eq!(
            restored.agent.history_content_digest.as_deref(),
            Some(base.content_digest.as_str())
        );
        assert_eq!(restored.agent.admission, crate::AgentAdmission::Held);
        assert_eq!(restored.agent.rev, 1);
        let restored_ref = restored.agent.canonical_ref.clone();
        let ids = receipt.candidate.agent_ids().unwrap();
        let snapshot = store.sandbox_get(&actor, &sandbox).unwrap();
        assert_eq!(snapshot.sources.len(), 2);
        assert_eq!(
            snapshot.sources[1].checkpoint_policy,
            CheckpointPolicy::Ephemeral
        );
        let view = store
            .read_agent_branch_view(&actor, &ids.session, &ids.branch)
            .unwrap();
        assert_eq!(view.history.next_turn_id, base.next_turn);
        assert_eq!(view.history.turns.len(), 1);
        assert_eq!(view.history.turns[0].prompt, "turn 1");
        assert_eq!(
            store
                .agent_get(&actor, &reference)
                .unwrap()
                .sandbox_ref
                .as_deref(),
            Some(original_sandbox.as_str())
        );
        assert!(store.activate(&actor, &ids.session, &ids.branch).is_err());
        assert!(store
            .agent_admission(&actor, &restored_ref, crate::AgentAdmission::Open, None)
            .is_err());
        assert!(store
            .actor_queue_claim_next(&actor, &ids.session, &ids.branch, "local", "fixture", 1,)
            .is_err());
        {
            let connection = store.connection().unwrap();
            let current = construction_receipt::lookup_in(
                &connection,
                receipt.operation,
                &creator,
                &receipt.request_id,
            )
            .unwrap()
            .unwrap();
            assert_eq!(current.state, PreparationState::Applied);
            assert_eq!(current.result_ref.as_deref(), Some(restored_ref.as_str()));
            assert_eq!(
                connection
                    .query_row("SELECT count(*) FROM sandboxes_current", [], |row| row
                        .get::<_, usize>(0),)
                    .unwrap(),
                sandbox_count + 1
            );
            assert_eq!(connection.query_row(
                "SELECT count(*) FROM local_coordination_leases WHERE run_id=?1 AND state='released'",
                [&candidate], |row| row.get::<_, usize>(0),
            ).unwrap(), 2);
            assert!(crate::context::authorize_model_context(
                &connection,
                &ids.session,
                &view.branch,
                &identity,
            )
            .is_err());
        }
        store.checkpoint_delete(&actor, &checkpoint).unwrap();
        assert!(store
            .checkpoint_prepare_restore(
                &actor,
                &request,
                &layout,
                "host:retired",
                |_, _, _| panic!("retired checkpoint cannot authorize new restore")
            )
            .is_err());
        assert_eq!(
            store
                .checkpoint_replay_restore(&actor, &request, "host:restore")
                .unwrap(),
            Some(restored.clone())
        );
        assert!(store
            .checkpoint_inspect_restore(&actor, "host:restore", |_, _, _| panic!(
                "Applied restore cannot inspect again"
            ))
            .is_err());
        drop(store);

        // Bound but held source-bearing state now survives owner restart.
        // A missing driver degrades the source metadata, not the Agent's
        // visibility and never its execution gate.
        let store = SessionStore::open(&temp.0).unwrap();
        assert_eq!(
            store
                .checkpoint_replay_restore(&actor, &request, "host:restore")
                .unwrap(),
            Some(restored.clone())
        );
        assert!(store
            .sandbox_get(&actor, &sandbox)
            .unwrap()
            .sources
            .iter()
            .all(|source| source.compatibility == crate::SourceCompatibility::RecoveryRequired));
        assert!(store.activate(&actor, &ids.session, &ids.branch).is_err());
        drop(store);
        let store = SessionStore::open_with_sources(&temp.0, registry.clone(), limit).unwrap();
        assert_eq!(store.sandbox_get(&actor, &sandbox).unwrap(), snapshot);
        assert_eq!(
            store
                .checkpoint_replay_restore(&actor, &request, "host:restore")
                .unwrap(),
            Some(restored)
        );
        assert!(store
            .agent_admission(&actor, &restored_ref, crate::AgentAdmission::Open, None)
            .is_err());

        // Logical close of the actual restored Agent/Sandbox uses their
        // original lifecycle owners. No driver call or filesystem cleanup.
        assert!(store.sandbox_restore_preparation(&actor, &sandbox).is_err());
        assert!(store.sandbox_close(&actor, &sandbox, None).is_err());
        store
            .agent_request_close(&actor, &restored_ref, None)
            .unwrap();
        assert!(store.sandbox_close(&actor, &sandbox, None).is_err());
        let closed_agent = store.agent_finish_close(&actor, &restored_ref).unwrap();

        // The same tool root with a different cwd must not be silently
        // quarantined while a legacy Agent is still live.
        let alias_layout = SessionBinding {
            working_directory: temp.0.join("different-cwd"),
            ..layout.clone()
        };
        std::fs::create_dir(&alias_layout.working_directory).unwrap();
        let (alias, alias_branch) = store
            .create(&actor, "same root", alias_layout.clone())
            .unwrap();
        let alias_ref = agent_owner::canonical_ref(
            &store.connection().unwrap(),
            AgentKey::new(&alias.id, &alias_branch.id),
        )
        .unwrap();
        let alias_agent = store.agent_get(&actor, &alias_ref).unwrap();
        let alias_sandbox = alias_agent.sandbox_ref.unwrap();
        let unused_alias = store
            .sandbox_create_fresh(&actor, &snapshot.project_ref, &alias_layout)
            .unwrap();
        assert!(store.sandbox_close(&actor, &sandbox, None).is_err());
        store.agent_request_close(&actor, &alias_ref, None).unwrap();
        store.agent_finish_close(&actor, &alias_ref).unwrap();

        let set = SourceMutationSet::from_anchors(&anchors.anchors).unwrap();
        let barrier = store
            .acquire_source_mutation_barrier(&set, "fixture:close")
            .unwrap();
        assert!(store.sandbox_close(&actor, &sandbox, None).is_err());
        barrier
            .settle(crate::SourceMutationOutcome::Unknown)
            .unwrap();
        assert!(store.sandbox_close(&actor, &sandbox, None).is_err());
        {
            let mut connection = store.connection().unwrap();
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            let barrier = store
                .recover_source_mutation_barrier_in(&tx, &set, "fixture:close")
                .unwrap();
            db(tx.commit()).unwrap();
            drop(connection);
            barrier
                .settle(crate::SourceMutationOutcome::Settled)
                .unwrap();
        }

        // A missing driver does not make metadata retirement impossible.
        // Reopen with no implementations, then close without touching
        // the physical marker files or dropping any durable dependency.
        drop(store);
        let store = SessionStore::open(&temp.0).unwrap();
        let before = store.sandbox_get(&actor, &sandbox).unwrap();
        let closed = store
            .sandbox_close(&actor, &sandbox, Some(before.rev))
            .unwrap();
        assert_eq!(closed.lifecycle, crate::SandboxLifecycle::Closed);
        assert_eq!(closed.rev, before.rev + 1);
        assert_eq!(closed.sources, before.sources);
        assert!(store.sandbox_restore_preparation(&actor, &sandbox).is_err());
        assert_eq!(store.sandbox_close(&actor, &sandbox, None).unwrap(), closed);
        assert!(store
            .sandbox_close(&actor, &sandbox, Some(before.rev))
            .is_err());
        assert_eq!(
            store.agent_get(&actor, &restored_ref).unwrap(),
            closed_agent
        );
        for source in &anchors.anchors {
            assert!(marker(source.name.as_str()).is_file());
        }
        // Exact retained paths are fenced across construction and execution,
        // including an alternate cwd and a pre-existing empty Sandbox.
        let project = closed.project_ref.clone();
        assert!(store
            .create(&actor, "retained root", layout.clone())
            .is_err());
        assert!(store
            .create(&actor, "retained alternate cwd", alias_layout.clone())
            .is_err());
        assert!(store
            .sandbox_create_fresh(&actor, &project, &alias_layout)
            .is_err());
        {
            let connection = store.connection().unwrap();
            assert!(sandbox_owner::require_execution_in(&connection, &alias.id).is_err());
            assert!(
                sandbox_owner::require_fresh_layout_in(&connection, &project, &layout).is_err()
            );
            let binding =
                crate::agent_binding_in(&connection, &actor, &alias.id, &alias_branch.id).unwrap();
            assert!(sandbox_owner::preflight_initial_selection(
                &connection,
                &actor,
                &binding.domain_handle,
                &binding.identity_handle,
                &binding.project_handle,
                &alias_layout,
                Some(&alias_sandbox),
            )
            .is_err());
        }
        drop(store);
        // Closed dependencies are still validated on every owner restart.
        let store = SessionStore::open_with_sources(&temp.0, registry.clone(), limit).unwrap();
        let revalidated = store.sandbox_get(&actor, &sandbox).unwrap();
        assert_eq!(revalidated.lifecycle, crate::SandboxLifecycle::Closed);
        assert_eq!(revalidated.rev, closed.rev);
        assert_eq!(revalidated.sources, snapshot.sources);
        // Original Applied restore provenance survives checkpoint anchor
        // release. No inference from the current binding or Git marker.
        store
            .checkpoint_reconcile_retired_sources(&actor, &checkpoint, |set| {
                assert_eq!(set, &anchors);
                Ok(())
            })
            .unwrap();
        assert_eq!(
            store.sandbox_restore_preparation(&actor, &sandbox).unwrap(),
            *receipt.sources.as_ref().unwrap()
        );
        assert!(store
            .sandbox_restore_preparation(&viewer, &sandbox)
            .is_err());
        let anchor = &anchors.anchors[0];
        let incoming = SourceBinding {
            name: anchor.name.clone(),
            driver: anchor.driver.clone(),
            binding_schema: 1,
            source_ref: anchor.source_ref.clone(),
            materialization: "fixture:retained".into(),
            access: anchor.access,
            checkpoint_policy: anchor.checkpoint_policy,
        };
        assert!(matches!(
            store.sandbox_sources_set(
                &actor,
                &unused_alias.canonical_ref,
                &incoming.name,
                Some(&incoming),
                unused_alias.rev
            ),
            Err(Error::Conflict(
                "Sandbox layout retains closed source custody"
            ))
        ));
        assert_eq!(
            store
                .sandbox_get(&actor, &unused_alias.canonical_ref)
                .unwrap(),
            unused_alias
        );
        assert!(store.activate(&actor, &ids.session, &ids.branch).is_err());

        // A separate open Sandbox retaining this exact materialization
        // protects it even without an active Agent or source claim.
        let original_bindings: Vec<_> = anchors
            .anchors
            .iter()
            .map(|anchor| SourceBinding {
                name: anchor.name.clone(),
                driver: anchor.driver.clone(),
                binding_schema: 1,
                source_ref: anchor.source_ref.clone(),
                materialization: std::fs::read_to_string(marker(anchor.name.as_str())).unwrap(),
                access: anchor.access,
                checkpoint_policy: anchor.checkpoint_policy,
            })
            .collect();
        let other_layout = SessionBinding {
            working_directory: temp.0.join("shared-materialization"),
            tool_root: temp.0.join("shared-materialization"),
            ..layout.clone()
        };
        let shared = store
            .sandbox_create_fresh_with_sources(&actor, &project, &other_layout, &original_bindings)
            .unwrap();
        assert!(store
            .sandbox_reconcile_closed_sources::<()>(&actor, &sandbox, |_, _| panic!(
                "live materialization dependency must be retained"
            ))
            .is_err());
        assert!(store
            .sandbox_reconcile_closed_sources::<()>(&actor, &shared.canonical_ref, |_, _| panic!(
                "open Sandbox must not release its sources"
            ))
            .is_err());
        store
            .sandbox_close(&actor, &shared.canonical_ref, None)
            .unwrap();
        assert!(store
            .sandbox_restore_preparation(&actor, &shared.canonical_ref)
            .is_err());
        let held = store
            .acquire_source_mutation_barrier(&set, "fixture:busy-cleanup")
            .unwrap();
        assert!(store
            .sandbox_reconcile_closed_sources::<()>(&actor, &sandbox, |_, _| panic!(
                "busy source must not release"
            ))
            .is_err());
        held.settle(crate::SourceMutationOutcome::Settled).unwrap();

        let assert_retained =
            |current: &crate::SandboxSnapshot, retained: &[crate::VersionedSourceBinding]| {
                assert_eq!(current, &revalidated);
                assert_eq!(retained.len(), original_bindings.len());
                assert!(retained
                    .iter()
                    .zip(&original_bindings)
                    .all(
                        |(actual, original)| &actual.binding == original && actual.binding_rev == 1
                    ));
            };
        // A real partial release in the isolated fixture leaves the
        // original complete set and layout reserved, not a partial success.
        assert!(store
            .sandbox_reconcile_closed_sources::<()>(&actor, &sandbox, |current, retained| {
                assert_retained(current, retained);
                assert!(store
                    .acquire_source_mutation_barrier(&set, "fixture:while-cleanup")
                    .is_err());
                assert!(store
                    .create(&actor, "during cleanup", layout.clone())
                    .is_err());
                std::fs::remove_file(marker(retained[0].binding.name.as_str()))?;
                Err(Error::Conflict("fixture interrupted physical retirement"))
            })
            .is_err());
        assert_eq!(store.sandbox_get(&actor, &sandbox).unwrap(), revalidated);
        assert!(store
            .create(&actor, "after partial cleanup", layout.clone())
            .is_err());
        drop(store);

        let missing = SessionStore::open(&temp.0).unwrap();
        assert!(missing
            .sandbox_reconcile_closed_sources::<()>(&actor, &sandbox, |_, _| panic!(
                "missing driver must retain cleanup custody"
            ))
            .is_err());
        assert!(missing
            .acquire_source_mutation_barrier(&set, "fixture:restart-cleanup")
            .is_err());
        drop(missing);
        // Lower the limit below the retained set. Recovery is not new
        // admission and must still settle the original complete custody.
        let store = SessionStore::open_with_sources(
            &temp.0,
            registry.clone(),
            SourceSetLimit::new(1).unwrap(),
        )
        .unwrap();
        assert_eq!(
            store.sandbox_restore_preparation(&actor, &sandbox).unwrap(),
            *receipt.sources.as_ref().unwrap()
        );
        let released = store
            .sandbox_reconcile_closed_sources(&actor, &sandbox, |current, retained| {
                assert_retained(current, retained);
                for binding in retained {
                    match std::fs::remove_file(marker(binding.binding.name.as_str())) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error.into()),
                    }
                }
                // The entire fixture root is now empty and safely absent. It
                // contains no shared backing. Never recursively remove it.
                std::fs::remove_dir(&current.tool_root)?;
                Ok(())
            })
            .unwrap();
        assert_eq!(released.lifecycle, crate::SandboxLifecycle::Closed);
        assert_eq!(released.rev, revalidated.rev + 1);
        assert!(released.sources.is_empty());
        assert!(store.sandbox_restore_preparation(&actor, &sandbox).is_err());
        assert_eq!(
            store
                .sandbox_reconcile_closed_sources::<()>(&actor, &sandbox, |_, _| panic!(
                    "released materializations must not be reinspected"
                ))
                .unwrap(),
            released
        );
        assert_eq!(
            store.agent_get(&actor, &restored_ref).unwrap(),
            closed_agent
        );
        assert!(store.activate(&actor, &ids.session, &ids.branch).is_err());
        // The other retired owner still has its own retained dependency.
        assert_eq!(
            store
                .sandbox_get(&actor, &shared.canonical_ref)
                .unwrap()
                .sources,
            shared.sources
        );
        // An independently recreated clean root may now be used by a
        // fresh Agent, never by the old closed source-bearing Session.
        std::fs::create_dir(&layout.tool_root).unwrap();
        let (fresh, fresh_branch) = store
            .create(&actor, "clean new baseline", layout.clone())
            .unwrap();
        assert!(store.activate(&actor, &fresh.id, &fresh_branch.id).is_ok());
        drop(store);
        let store = SessionStore::open_with_sources(&temp.0, registry, limit).unwrap();
        assert_eq!(store.sandbox_get(&actor, &sandbox).unwrap(), released);
        assert!(store.activate(&actor, &ids.session, &ids.branch).is_err());
        assert!(store.activate(&actor, &fresh.id, &fresh_branch.id).is_ok());
    }
}
