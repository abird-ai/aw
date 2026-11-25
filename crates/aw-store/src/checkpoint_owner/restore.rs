//! Restore preparation uses the immutable checkpoint owner and ordinary
//! exact-history fork authority. Candidates are private until publication.
//! Source-bearing execution stays held until realization is admitted.
use super::{history_head_in, row_in, sources_in, CheckpointLifecycle};
use crate::{
    agent_host::{authorize_project_create, parse_project_ref, snapshot_in},
    agent_owner::{self, AgentKey, AgentPermission},
    construction_receipt::{
        self, require_matching_sources, validate_inspected_bindings, ConstructionCandidate,
        ConstructionOperation, ConstructionPreparation, InitialTask, PreparationState,
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
use rusqlite::{Connection, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Trusted lower checkpoint restore request. The destination layout is
/// independently selected by the host. Access and optional initial task
/// use the existing history and encrypted queue construction owners.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointRestoreRequest {
    pub checkpoint: String,
    pub project: Option<String>,
    pub title: Option<String>,
    pub access: AgentHistoryForkAccess,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointRestoreResult {
    pub agent: AgentSnapshot,
    pub source_coverage: SourceCoverage,
}

/// Trusted layout selection for a Prepared restore. A nonempty, explicitly
/// selected baseline contains independently retained destination versions,
/// not anchors of the checkpoint being restored. None selects that
/// checkpoint's exact captured sources. No external effect during selection.
pub struct CheckpointRestoreLayout<'a> {
    pub binding: &'a SessionBinding,
    pub baseline: Option<SourceSet>,
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
    // Only task-only Prepared baselines need a durable selection digest.
    // Existing taskless/captured-source receipt encodings are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    baseline_digest: Option<String>,
    // Effectful current-baseline selection has no checkpoint-source owner.
    // Its original immutable version/retention inputs belong to this receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    baseline_sources: Option<SourceSet>,
}

impl RestoreBoundary {
    pub(crate) fn has_baseline_selection(&self) -> bool {
        self.baseline_digest.is_some()
    }

    fn request(&self) -> CheckpointRestoreRequest {
        CheckpointRestoreRequest {
            checkpoint: self.checkpoint.clone(),
            project: Some(self.destination_project.clone()),
            title: Some(self.title.clone()),
            access: self.access.clone(),
            task: None,
        }
    }

    pub(crate) fn checkpoint(&self) -> &str {
        &self.checkpoint
    }

    pub(crate) fn validate(
        &self,
        candidate: &ConstructionCandidate,
        sources: Option<&PreparedSourceSet>,
    ) -> Result<()> {
        for value in [&self.checkpoint, &self.history_base_id, &self.history_head] {
            label(value, 128)?;
        }
        label(&self.source_agent_ref, 2048)?;
        label(&self.title, 160)?;
        self.access.validate()?;
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
            || !valid_digest(&self.source_set_digest)
            || !valid_digest(&self.binding_digest)
            || self
                .baseline_digest
                .as_deref()
                .is_some_and(|digest| !valid_digest(digest))
        {
            return Err(Error::Conflict("invalid checkpoint restore boundary"));
        }
        if let Some(baseline) = &self.baseline_sources {
            require_versioned_baseline(self.coverage, baseline)?;
            if self.baseline_digest.is_some()
                || self.source_set_digest
                    != source_set_digest(&SourceSet {
                        coverage: self.coverage,
                        anchors: Vec::new(),
                    })?
            {
                return Err(Error::Conflict(
                    "baseline cannot replace captured checkpoint sources",
                ));
            }
            return match sources {
                Some(sources) if sources.prepare_id == candidate.prepare_id() => {
                    require_matching_sources(baseline, sources)
                }
                _ => Err(Error::Conflict(
                    "versioned baseline requires Prepared source custody",
                )),
            };
        }
        match sources {
            Some(sources)
                if self.coverage != SourceCoverage::None
                    && self.baseline_digest.is_none()
                    && sources.operation == SourceOperationKind::Restore
                    && sources.prepare_id == candidate.prepare_id() =>
            {
                Ok(())
            }
            None if self.coverage != SourceCoverage::Selected
                && self.source_set_digest
                    == source_set_digest(&SourceSet {
                        coverage: self.coverage,
                        anchors: Vec::new(),
                    })? =>
            {
                Ok(())
            }
            _ => Err(Error::Conflict(
                "restore source custody differs from checkpoint coverage",
            )),
        }
    }

    /// Resolve the exact immutable materialization inputs. Captured sources
    /// belong to the checkpoint; independently selected baseline versions
    /// belong to the original operation receipt. Retired history is never
    /// authority to publish a new Agent.
    pub(crate) fn materialization_sources_in(
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
        if source_set_digest(&set)? != self.source_set_digest {
            return Err(Error::Storage("restore checkpoint source set changed"));
        }
        let set = if let Some(baseline) = &self.baseline_sources {
            if !set.anchors.is_empty() {
                return Err(Error::Storage(
                    "baseline restore has captured checkpoint sources",
                ));
            }
            require_versioned_baseline(self.coverage, baseline)?;
            baseline.clone()
        } else {
            set
        };
        match &receipt.sources {
            Some(prepared) if !set.anchors.is_empty() => require_matching_sources(&set, prepared)?,
            None if set.anchors.is_empty()
                && self.has_baseline_selection()
                && receipt.task.is_some() => {}
            _ => {
                return Err(Error::Storage(
                    "restore descriptors differ from retained sources",
                ))
            }
        }
        Ok(set)
    }
}

fn require_versioned_baseline(coverage: SourceCoverage, baseline: &SourceSet) -> Result<()> {
    // Selected here describes the explicitly chosen destination inputs,
    // never the source checkpoint's coverage.
    if coverage == SourceCoverage::Selected
        || baseline.coverage != SourceCoverage::Selected
        || baseline
            .anchors
            .windows(2)
            .any(|pair| pair[0].name.as_str() >= pair[1].name.as_str())
    {
        return Err(Error::Conflict("invalid versioned baseline selection"));
    }
    baseline.validate()?;
    if coverage == SourceCoverage::AutoAllRequired
        && baseline
            .anchors
            .iter()
            .any(|anchor| anchor.checkpoint_policy != aw_agent_core::CheckpointPolicy::Ephemeral)
    {
        return Err(Error::Conflict(
            "empty Auto restore admits only current ephemeral sources",
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

impl RestoreObservation {
    fn materialization_sources(&self) -> &SourceSet {
        self.boundary
            .baseline_sources
            .as_ref()
            .unwrap_or(&self.sources)
    }
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
        &request.access,
        &stable_user_principal(&project.identity)?,
        &project.project,
    )?;

    let (sources, _) = sources_in(connection, &request.checkpoint, snapshot.source_coverage)?;
    sources.admit(limit)?;
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
        access: request.access.clone(),
        binding_digest: binding_digest.to_owned(),
        baseline_digest: None,
        baseline_sources: None,
    };
    Ok(RestoreObservation {
        session: row.session,
        branch: row.branch,
        boundary,
        sources,
        history: source,
    })
}

fn observe_prepared_in(
    connection: &Connection,
    actor: &PrincipalId,
    request: &CheckpointRestoreRequest,
    layout_digest: &str,
    limit: SourceSetLimit,
    registry: &SourceDriverRegistryReader,
    baseline: Option<&SourceSet>,
) -> Result<RestoreObservation> {
    let mut observed = observe_in(connection, actor, request, layout_digest, limit, registry)?;
    if let Some(baseline) = baseline {
        if !observed.sources.anchors.is_empty() {
            return Err(Error::Conflict(
                "captured sources cannot be replaced by a fresh baseline",
            ));
        }
        limit.admit(baseline.anchors.len())?;
        require_versioned_baseline(observed.boundary.coverage, baseline)?;
        for anchor in &baseline.anchors {
            registry.resolve(&anchor.pin()?)?;
        }
        observed.boundary.baseline_sources = Some(baseline.clone());
    } else if observed.sources.anchors.is_empty() {
        return Err(Error::Conflict(
            "empty checkpoint requires fresh baseline restore",
        ));
    }
    Ok(observed)
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
    if !matches!(
        candidate,
        ConstructionCandidate::Agent {
            sandbox: Some(_),
            ..
        }
    ) {
        return Err(Error::Storage(
            "restore candidate is not a fresh Agent and Sandbox",
        ));
    }
    candidate.require_absent_in(connection)
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
    barrier: SourceMutationBarrier,
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

    /// Exact materialization inputs, whether captured checkpoint sources
    /// or the independently selected versions of a fresh baseline.
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
        let observed = observe_prepared_in(
            tx,
            &self.actor,
            &boundary.request(),
            &binding_digest(binding)?,
            self.store.inner.source_limit,
            registry,
            boundary.baseline_sources.as_ref(),
        )?;
        if observed.boundary != *boundary
            || observed.materialization_sources() != &self.anchors
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

    /// Abandon this original unpublished candidate under its live custody.
    /// The trusted reconciler must prove every original materialization
    /// released or absent, preserving other live resources and returning
    /// any independent external guard needed through Store settlement.
    pub fn abandon<F>(
        self,
        reconcile: impl FnOnce(&SourceDriverRegistryReader, &SourceSet, &PreparedSourceSet) -> Result<F>,
    ) -> Result<()> {
        construction_receipt::reconcile_unpublished_sources(
            self.store,
            &self.actor,
            &self.receipt,
            &self.barrier,
            || {
                let registry = self.inspection_registry()?;
                reconcile(&registry, &self.anchors, self.sources())
            },
        )
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
            let (bindings, evidence) =
                validate_inspected_bindings(&registry, &self.anchors, results)?;
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
            if let Some(task) = &restore.receipt.task {
                restore
                    .store
                    .verify_prepared_queue_custody(&task.custody())?;
            }
            construction_receipt::publish_sources(
                restore.store,
                &restore.actor,
                &restore.receipt,
                &restore.barrier,
                &self.evidence,
                proofs,
                |tx| {
                    let history = restore.require_current_boundary_in(tx, binding, &registry)?;
                    publish_restored_agent_in(
                        tx,
                        restore.store,
                        &restore.actor,
                        &restore.receipt,
                        &history,
                        binding,
                        &self.bindings,
                    )
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

/// The same ordinary exact-history constructor and fresh Sandbox owner
/// publish both captured-source and current-baseline restores.
fn publish_restored_agent_in(
    tx: &Transaction<'_>,
    store: &SessionStore,
    actor: &PrincipalId,
    receipt: &ConstructionPreparation,
    history: &history_owner::ResolvedHistorySource,
    binding: &SessionBinding,
    sources: &[SourceBinding],
) -> Result<(String, CheckpointRestoreResult)> {
    let boundary = receipt
        .restore_boundary
        .as_ref()
        .ok_or(Error::Storage("restore publication lost its boundary"))?;
    let ConstructionCandidate::Agent {
        sandbox: Some(sandbox),
        ..
    } = &receipt.candidate
    else {
        return Err(Error::Storage("restore publication lost its fresh Sandbox"));
    };
    let project = parse_project_ref(&boundary.destination_project)?;
    let fresh = sandbox_owner::publish_fresh_in(
        tx,
        actor,
        sandbox,
        binding,
        sources,
        &store.inner.source_drivers,
        store.inner.source_limit,
    )?;
    let (session, branch) = history_owner::create_agent_from_resolved_history_with_ids_in(
        tx,
        actor,
        history,
        &boundary.title,
        binding,
        boundary.access.clone(),
        &stable_user_principal(&project.identity)?,
        &project.project,
        receipt.candidate.agent_ids()?,
        Some(&fresh),
    )?;
    if let Some(task) = &receipt.task {
        task.publish_in(
            tx,
            &stable_user_principal(&project.identity)?,
            &session.id,
            &branch.id,
        )?;
    }
    let reference = agent_owner::canonical_ref(tx, AgentKey::new(&session.id, &branch.id))?;
    let agent = snapshot_in(tx, actor, &reference)?;
    Ok((
        reference,
        CheckpointRestoreResult {
            agent,
            source_coverage: boundary.coverage,
        },
    ))
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
    let anchors = boundary.materialization_sources_in(tx, receipt)?;
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

fn restore_fingerprint(request: &CheckpointRestoreRequest, request_id: &str) -> Result<String> {
    label(&request.checkpoint, 128)?;
    label(request_id, 256)?;
    if let Some(project) = &request.project {
        label(project, 2048)?;
    }
    if let Some(title) = &request.title {
        label(title, 160)?;
    }
    request.access.validate()?;
    if let Some(task) = &request.task {
        // No plaintext task in the semantic receipt. Keep the original
        // no-task fingerprint byte-for-byte compatible.
        fingerprint(
            b"aw-checkpoint-restore-task-v1\0",
            &(
                &request.checkpoint,
                &request.project,
                &request.title,
                &request.access,
                format!("{:x}", Sha256::digest(task.as_bytes())),
            ),
        )
    } else {
        fingerprint(b"aw-checkpoint-restore-v1\0", request)
    }
}

fn replay_restore_in(
    connection: &Connection,
    actor: &PrincipalId,
    creator: &str,
    request_id: &str,
    fingerprint: &str,
) -> Result<Option<CheckpointRestoreResult>> {
    let Some(receipt) = construction_receipt::lookup_in(
        connection,
        ConstructionOperation::CheckpointRestore,
        creator,
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
    let agent = match snapshot_in(connection, actor, reference) {
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
    Ok(Some(CheckpointRestoreResult {
        agent,
        source_coverage: boundary.coverage,
    }))
}

fn require_baseline_owner(store: &SessionStore) -> Result<()> {
    if store.inner.mode != crate::StoreMode::OwnerLock {
        return Err(Error::Denied(
            "checkpoint restore requires the exclusive Store owner",
        ));
    }
    Ok(())
}

fn baseline_digest(sources: &[SourceBinding]) -> Result<String> {
    let mut ordered: Vec<_> = sources.iter().collect();
    ordered.sort_unstable_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
    fingerprint(b"aw-restore-baseline-v1\0", &ordered)
}

fn observe_baseline_in(
    store: &SessionStore,
    tx: &Transaction<'_>,
    actor: &PrincipalId,
    request: &CheckpointRestoreRequest,
    binding: &SessionBinding,
    sources: &[SourceBinding],
) -> Result<RestoreObservation> {
    let registry = store.inner.source_drivers.reader()?;
    let observed = observe_in(
        tx,
        actor,
        request,
        &binding_digest(binding)?,
        store.inner.source_limit,
        &registry,
    )?;
    if !observed.sources.anchors.is_empty() {
        return Err(Error::Conflict(
            "captured sources require exact Prepared restore",
        ));
    }
    if observed.sources.coverage == SourceCoverage::AutoAllRequired
        && sources
            .iter()
            .any(|source| source.checkpoint_policy != aw_agent_core::CheckpointPolicy::Ephemeral)
    {
        return Err(Error::Conflict(
            "empty Auto restore admits only current ephemeral sources",
        ));
    }
    sandbox_owner::validate_source_bindings(
        store.inner.source_limit,
        &store.inner.source_drivers,
        sources,
    )?;
    sandbox_owner::require_fresh_layout_in(tx, &observed.boundary.destination_project, binding)?;
    SourceMutationSet::from_bindings(sources)?.require_unleased_in(tx)?;
    Ok(observed)
}

fn task_baseline_receipt(
    connection: &Connection,
    actor: &PrincipalId,
    request_id: &str,
) -> Result<ConstructionPreparation> {
    label(request_id, 256)?;
    let creator = user_for_principal(connection, actor)?
        .ok_or(Error::Denied("checkpoint restore caller unavailable"))?;
    let receipt = construction_receipt::lookup_in(
        connection,
        ConstructionOperation::CheckpointRestore,
        &creator,
        request_id,
    )?
    .ok_or(Error::Denied("task-only baseline receipt unavailable"))?;
    construction_receipt::require_task_baseline_claim_in(connection, actor, &receipt)?;
    Ok(receipt)
}

fn publish_task_baseline(
    store: &SessionStore,
    connection: &mut Connection,
    actor: &PrincipalId,
    receipt: &ConstructionPreparation,
    binding: &SessionBinding,
    sources: &[SourceBinding],
    vault: &impl crate::SecretStore,
) -> Result<CheckpointRestoreResult> {
    let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
    construction_receipt::require_task_baseline_claim_in(&tx, actor, receipt)?;
    if !receipt.state.unresolved() {
        return Err(Error::Conflict("task-only baseline is not unresolved"));
    }
    require_candidate_absent(&tx, &receipt.candidate)?;
    let boundary = receipt
        .restore_boundary
        .as_ref()
        .ok_or(Error::Storage("baseline restore lost its boundary"))?;
    let mut observed =
        observe_baseline_in(store, &tx, actor, &boundary.request(), binding, sources)?;
    observed.boundary.baseline_digest = Some(baseline_digest(sources)?);
    if observed.boundary != *boundary
        || receipt.origin() != AgentKey::new(&observed.session, &observed.branch)
    {
        return Err(Error::Conflict(
            "original baseline restore boundary changed",
        ));
    }
    receipt.initial_task()?.custody().verify(vault)?;
    let (reference, result) = publish_restored_agent_in(
        &tx,
        store,
        actor,
        receipt,
        &observed.history,
        binding,
        sources,
    )?;
    construction_receipt::publish_task_baseline_in(&tx, actor, receipt, reference)?;
    db(tx.commit())?;
    Ok(result)
}

fn settle_task_baseline_error<T>(
    connection: &mut Connection,
    actor: &PrincipalId,
    receipt: &ConstructionPreparation,
    result: Result<T>,
) -> Result<T> {
    if result.is_err() {
        // A failed commit can be ambiguous. Never delete custody or
        // overwrite a factual Applied result merely from a returned error.
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current = construction_receipt::lookup_in(
            &tx,
            receipt.operation,
            &receipt.scope,
            &receipt.request_id,
        )?;
        if current.as_ref() == Some(receipt) && receipt.state.unresolved() {
            construction_receipt::transition_in(
                &tx,
                actor,
                receipt,
                PreparationState::Unknown,
                None,
                Some("baseline task outcome requires inspection".into()),
            )?;
        }
        db(tx.commit())?;
    }
    result
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
        let fingerprint = restore_fingerprint(request, request_id)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction())?;
        let creator = user_for_principal(&tx, actor)?
            .ok_or(Error::Denied("checkpoint restore caller unavailable"))?;
        let result = replay_restore_in(&tx, actor, &creator, request_id, &fingerprint)?;
        db(tx.commit())?;
        Ok(result)
    }

    /// Restore history into an independently authorized fresh baseline.
    /// Baseline bindings are already materialized, never operation-owned
    /// effects. An optional initial task uses ordinary encrypted custody.
    /// The trusted host keeps the complete physical baseline authority
    /// through publication, including on explicit recovery.
    pub fn checkpoint_restore_baseline(
        &self,
        actor: &PrincipalId,
        request: &CheckpointRestoreRequest,
        binding: &SessionBinding,
        baseline_sources: &[SourceBinding],
        request_id: &str,
        execution_site: &str,
    ) -> Result<CheckpointRestoreResult> {
        let fingerprint = restore_fingerprint(request, request_id)?;
        require_baseline_owner(self)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let creator = user_for_principal(&tx, actor)?
            .ok_or(Error::Denied("checkpoint restore caller unavailable"))?;
        if let Some(result) = replay_restore_in(&tx, actor, &creator, request_id, &fingerprint)? {
            db(tx.commit())?;
            return Ok(result);
        }
        let mut observed =
            observe_baseline_in(self, &tx, actor, request, binding, baseline_sources)?;
        let ids = resources::mint_session_candidate_ids()?;
        let task = request
            .task
            .as_deref()
            .map(|payload| InitialTask::mint(request_id, &ids.session, payload, execution_site))
            .transpose()?;
        if task.is_some() {
            observed.boundary.baseline_digest = Some(baseline_digest(baseline_sources)?);
        }
        let sandbox = format!("{}/s:{}", observed.boundary.destination_project, id("sbx")?);
        let candidate = ConstructionCandidate::Agent {
            session: ids.session,
            root: ids.root,
            branch: ids.branch,
            sandbox: Some(sandbox),
        };
        require_candidate_absent(&tx, &candidate)?;
        observed.boundary.validate(&candidate, None)?;
        let mut receipt = ConstructionPreparation {
            operation: ConstructionOperation::CheckpointRestore,
            scope: creator.clone(),
            creator,
            parent_session_id: observed.session,
            parent_branch_id: observed.branch,
            request_id: request_id.to_owned(),
            fingerprint,
            state: if task.is_some() {
                PreparationState::Prepared
            } else {
                PreparationState::Applied
            },
            candidate,
            task,
            sources: None,
            capture_boundary: None,
            restore_boundary: Some(observed.boundary),
            sandbox_boundary: None,
            result_ref: None,
            note: None,
        };
        if let Some(payload) = request.task.as_deref() {
            let custody = receipt.initial_task()?.custody();
            // Initialize/open the original vault before recording custody.
            // Retain the same Store connection guard through the bounded
            // local I/O and publication; no other recovery can steal it.
            let vault = self.local_secret_store_in(&tx)?;
            construction_receipt::claim_in(
                &tx,
                actor,
                &self.inner.source_drivers,
                self.inner.source_limit,
                &receipt,
            )?;
            db(tx.commit())?;
            let result = custody.write(&vault, payload).and_then(|()| {
                publish_task_baseline(
                    self,
                    &mut connection,
                    actor,
                    &receipt,
                    binding,
                    baseline_sources,
                    &vault,
                )
            });
            return settle_task_baseline_error(&mut connection, actor, &receipt, result);
        }
        let (reference, result) = publish_restored_agent_in(
            &tx,
            self,
            actor,
            &receipt,
            &observed.history,
            binding,
            baseline_sources,
        )?;
        receipt.result_ref = Some(reference);
        construction_receipt::record_atomic_restore_in(&tx, actor, &receipt)?;
        db(tx.commit())?;
        Ok(result)
    }

    /// Inspect the original encrypted input and publish the exact
    /// previously chosen baseline under current independent authority.
    /// Never rewrite missing ciphertext, rematerialize, or replan.
    pub fn checkpoint_resume_baseline(
        &self,
        actor: &PrincipalId,
        request_id: &str,
        binding: &SessionBinding,
        baseline_sources: &[SourceBinding],
    ) -> Result<CheckpointRestoreResult> {
        require_baseline_owner(self)?;
        let mut connection = self.connection()?;
        let receipt = task_baseline_receipt(&connection, actor, request_id)?;
        if receipt.state == PreparationState::Applied {
            return replay_restore_in(
                &connection,
                actor,
                &receipt.creator,
                request_id,
                &receipt.fingerprint,
            )?
            .ok_or(Error::Storage("applied baseline receipt disappeared"));
        }
        if !receipt.state.unresolved() {
            return Err(Error::Conflict("task-only baseline already rejected"));
        }
        let result = self.local_secret_store_in(&connection).and_then(|vault| {
            publish_task_baseline(
                self,
                &mut connection,
                actor,
                &receipt,
                binding,
                baseline_sources,
                &vault,
            )
        });
        settle_task_baseline_error(&mut connection, actor, &receipt, result)
    }

    /// Discard only this unpublished operation's initial encrypted task.
    /// No baseline materialization was owned by this operation. Never
    /// delete an Applied queue item or reopen a Rejected request.
    pub fn checkpoint_abandon_baseline(&self, actor: &PrincipalId, request_id: &str) -> Result<()> {
        require_baseline_owner(self)?;
        let mut connection = self.connection()?;
        let receipt = task_baseline_receipt(&connection, actor, request_id)?;
        if receipt.state == PreparationState::Applied {
            return Err(Error::Conflict("published baseline cannot be abandoned"));
        }
        receipt.candidate.require_absent_in(&connection)?;
        if receipt.state == PreparationState::Rejected {
            return Ok(());
        }
        let result = (|| {
            let vault = self.local_secret_store_in(&connection)?;
            receipt.initial_task()?.custody().discard(&vault)?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            construction_receipt::require_task_baseline_claim_in(&tx, actor, &receipt)?;
            receipt.candidate.require_absent_in(&tx)?;
            construction_receipt::transition_in(
                &tx,
                actor,
                &receipt,
                PreparationState::Rejected,
                None,
                Some("unpublished baseline task reconciled".into()),
            )?;
            db(tx.commit())
        })();
        settle_task_baseline_error(&mut connection, actor, &receipt, result)
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
        execution_site: &str,
        plan: impl FnOnce(&str, &str, &SourceSet) -> Result<PreparedSourceSet>,
    ) -> Result<PreparedCheckpointRestore<'_>> {
        self.checkpoint_prepare_restore_with_layout(
            actor,
            request,
            CheckpointRestoreLayout {
                binding,
                baseline: None,
            },
            request_id,
            execution_site,
            plan,
        )
    }

    /// The same Prepared constructor for captured sources or independently
    /// selected immutable destination baseline versions. A host must retain
    /// those versions independently and authorize their materialization.
    /// No baseline source is added to the historical checkpoint.
    pub fn checkpoint_prepare_restore_with_layout(
        &self,
        actor: &PrincipalId,
        request: &CheckpointRestoreRequest,
        mut layout: CheckpointRestoreLayout<'_>,
        request_id: &str,
        execution_site: &str,
        plan: impl FnOnce(&str, &str, &SourceSet) -> Result<PreparedSourceSet>,
    ) -> Result<PreparedCheckpointRestore<'_>> {
        let fingerprint = restore_fingerprint(request, request_id)?;
        let binding = layout.binding;
        if self.inner.mode != crate::StoreMode::OwnerLock {
            return Err(Error::Denied(
                "checkpoint restore requires the exclusive persistent Store owner",
            ));
        }
        let limit = self.inner.source_limit;
        let (creator, observed, store_scope) = {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let creator = user_for_principal(&tx, actor)?
                .ok_or(Error::Denied("checkpoint restore caller unavailable"))?;
            require_new_request(&tx, &creator, request_id, &fingerprint)?;
            if let Some(baseline) = &mut layout.baseline {
                limit.admit(baseline.anchors.len())?;
                baseline
                    .anchors
                    .sort_unstable_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
            }
            let registry = self.inner.source_drivers.reader()?;
            let observed = observe_prepared_in(
                &tx,
                actor,
                request,
                &binding_digest(binding)?,
                limit,
                &registry,
                layout.baseline.as_ref(),
            )?;
            sandbox_owner::require_fresh_layout_in(
                &tx,
                &observed.boundary.destination_project,
                binding,
            )?;
            SourceMutationSet::from_anchors(&observed.materialization_sources().anchors)?
                .require_unleased_in(&tx)?;
            let store_scope = StoreScope::load(&tx)?;
            db(tx.commit())?;
            (creator, observed, store_scope)
        };
        let ids = resources::mint_session_candidate_ids()?;
        let task = request
            .task
            .as_deref()
            .map(|payload| InitialTask::mint(request_id, &ids.session, payload, execution_site))
            .transpose()?;
        let sandbox = format!("{}/s:{}", observed.boundary.destination_project, id("sbx")?);
        let anchors = observed.materialization_sources().clone();
        let mut sources = plan(&ids.session, &sandbox, &anchors)?;
        sources.admit(limit)?;
        sources
            .sources
            .sort_unstable_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
        require_matching_sources(&anchors, &sources)?;
        let candidate = ConstructionCandidate::Agent {
            session: ids.session,
            root: ids.root,
            branch: ids.branch,
            sandbox: Some(sandbox),
        };
        observed.boundary.validate(&candidate, Some(&sources))?;
        let set = SourceMutationSet::from_anchors(&anchors.anchors)?;
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
            task,
            sources: Some(sources),
            capture_boundary: None,
            restore_boundary: Some(observed.boundary),
            sandbox_boundary: None,
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
        let current = observe_prepared_in(
            &tx,
            actor,
            request,
            &binding_digest(binding)?,
            limit,
            &registry,
            layout.baseline.as_ref(),
        )?;
        if receipt.restore_boundary.as_ref() != Some(&current.boundary)
            || &anchors != current.materialization_sources()
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
        drop(connection);
        let restore = PreparedCheckpointRestore {
            store: self,
            actor: actor.clone(),
            receipt,
            anchors,
            barrier,
        };
        if let (Some(task), Some(payload)) = (&restore.receipt.task, request.task.as_deref()) {
            let custody = task.custody();
            // Prepared and all source claims are durable before the first
            // external write, including the encrypted task payload.
            if let Err(error) = self
                .write_prepared_queue_custody(&custody, payload)
                .and_then(|()| self.verify_prepared_queue_custody(&custody))
            {
                restore.mark_unknown()?;
                return Err(error);
            }
        }
        Ok(restore)
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
        self.recover_checkpoint_restore(actor, request_id, None)?
            .ok_or(Error::Conflict(
                "checkpoint restore candidate already reconciled",
            ))?
            .inspect(inspect)
    }

    /// Exact captured-source recovery for a host with a current, explicitly
    /// selected request and layout. Reject changed semantics or layout before
    /// reclaiming custody or handing original descriptors to a driver.
    /// Independently selected versioned baselines use their own recovery path.
    pub fn checkpoint_inspect_captured_restore_exact(
        &self,
        actor: &PrincipalId,
        request: &CheckpointRestoreRequest,
        binding: &SessionBinding,
        request_id: &str,
        inspect: impl FnOnce(
            &SourceDriverRegistryReader,
            &SourceSet,
            &PreparedSourceSet,
        ) -> Result<Vec<RecoveryInspection<SourceBinding>>>,
    ) -> Result<InspectedCheckpointRestore<'_>> {
        let fingerprint = restore_fingerprint(request, request_id)?;
        let layout = binding_digest(binding)?;
        self.recover_checkpoint_restore(actor, request_id, Some((&fingerprint, &layout)))?
            .ok_or(Error::Conflict(
                "checkpoint restore candidate already reconciled",
            ))?
            .inspect(inspect)
    }

    /// Reconcile the original abandoned restore after a crash. Never steal
    /// active custody, plan a new candidate or inspect on terminal replay.
    pub fn checkpoint_abandon_restore<F>(
        &self,
        actor: &PrincipalId,
        request_id: &str,
        reconcile: impl FnOnce(&SourceDriverRegistryReader, &SourceSet, &PreparedSourceSet) -> Result<F>,
    ) -> Result<()> {
        match self.recover_checkpoint_restore(actor, request_id, None)? {
            Some(restore) => restore.abandon(reconcile),
            None => Ok(()),
        }
    }

    fn recover_checkpoint_restore(
        &self,
        actor: &PrincipalId,
        request_id: &str,
        exact_captured: Option<(&str, &str)>,
    ) -> Result<Option<PreparedCheckpointRestore<'_>>> {
        label(request_id, 256)?;
        if self.inner.mode != crate::StoreMode::OwnerLock {
            return Err(Error::Denied(
                "checkpoint restore recovery requires the exclusive Store owner",
            ));
        }
        {
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
            if let Some((fingerprint, layout)) = exact_captured {
                if receipt.fingerprint != fingerprint
                    || receipt.restore_boundary.as_ref().is_none_or(|boundary| {
                        boundary.binding_digest != layout
                            || boundary.baseline_sources.is_some()
                            || boundary.has_baseline_selection()
                    })
                {
                    return Err(Error::Denied(
                        "captured restore request or layout differs from original semantics",
                    ));
                }
            }
            if construction_receipt::is_reconciled_in(&tx, &receipt)? {
                return Ok(None);
            }
            let registry = self.inner.source_drivers.reader()?;
            let anchors = inspection_sources_in(&tx, actor, &receipt, &registry)?;
            let set = SourceMutationSet::from_anchors(&anchors.anchors)?;
            let barrier =
                self.recover_source_mutation_barrier_in(&tx, &set, receipt.candidate.prepare_id())?;
            db(tx.commit())?;
            Ok(Some(PreparedCheckpointRestore {
                store: self,
                actor: actor.clone(),
                receipt,
                anchors,
                barrier,
            }))
        }
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
    use rusqlite::params;
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
        let empty_auto = store
            .checkpoint_create_empty_auto(&actor, &reference, None, "host:empty-auto")
            .unwrap();
        assert_eq!(empty_auto.source_coverage, SourceCoverage::AutoAllRequired);
        assert_eq!(empty_auto.history_base_id, base.id);
        assert!(empty_auto.sources.is_empty());
        handle.finish(finish(2)).await.unwrap();
        drop(handle);
        assert_eq!(
            store
                .checkpoint_replay_capture(
                    &actor,
                    &reference,
                    super::super::CheckpointCaptureSelection::Auto,
                    None,
                    "host:empty-auto",
                )
                .unwrap(),
            Some(empty_auto.clone())
        );
        assert!(store
            .checkpoint_create_history_only(&actor, &reference, None, "host:empty-auto")
            .is_err());
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
            task: None,
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
        // The same custom ACL is admitted for both restore paths. Resolve
        // the actual current destination principal before any driver plan.
        let grant = crate::AgentCreateAccessGrant {
            principal: viewer_ref.clone(),
            permissions: crate::AgentPermissions::VIEW_ONLY,
        };
        for invalid in [
            vec![grant.clone(), grant.clone()],
            vec![crate::AgentCreateAccessGrant {
                principal: format!(
                    "/d:other-domain/i:{}",
                    viewer_ref.rsplit_once("/i:").unwrap().1
                ),
                ..grant.clone()
            }],
            vec![crate::AgentCreateAccessGrant {
                principal: store.agent_get(&actor, &reference).unwrap().owner_ref,
                ..grant.clone()
            }],
        ] {
            let invalid = CheckpointRestoreRequest {
                access: AgentHistoryForkAccess::Custom(invalid),
                ..request.clone()
            };
            assert!(store
                .checkpoint_prepare_restore(
                    &actor,
                    &invalid,
                    &layout,
                    "host:bad-acl",
                    "local",
                    |_, _, _| panic!("invalid custom audience must refuse before planning"),
                )
                .is_err());
        }
        let request = CheckpointRestoreRequest {
            access: AgentHistoryForkAccess::Custom(vec![grant]),
            ..request
        };
        assert!(store
            .checkpoint_prepare_restore(
                &viewer,
                &request,
                &layout,
                "host:denied",
                "local",
                |_, _, _| panic!("view is not derive/destination/source authority")
            )
            .is_err());
        let mut history_only = request.clone();
        history_only.checkpoint = current.id.clone();
        let baseline_root = temp.0.join("fresh-baseline");
        std::fs::create_dir(&baseline_root).unwrap();
        let baseline_layout = SessionBinding {
            tool_root: baseline_root.clone(),
            working_directory: baseline_root,
            ..layout.clone()
        };
        // Only history with an empty captured set can use this atomic path.
        // View alone cannot restore, and a known original root isn't fresh.
        assert!(store
            .checkpoint_restore_baseline(
                &viewer,
                &history_only,
                &baseline_layout,
                &[],
                "host:baseline-denied",
                "local",
            )
            .is_err());
        assert!(store
            .checkpoint_restore_baseline(
                &actor,
                &request,
                &baseline_layout,
                &[],
                "host:captured-refusal",
                "local",
            )
            .is_err());
        assert!(store
            .checkpoint_restore_baseline(
                &actor,
                &history_only,
                &session.binding,
                &[],
                "host:old-layout",
                "local",
            )
            .is_err());
        let task_text = "fixture: baseline initial input";
        let task_baseline = CheckpointRestoreRequest {
            task: Some(task_text.into()),
            ..history_only.clone()
        };
        let abandoned_root = temp.0.join("abandoned-baseline");
        std::fs::create_dir(&abandoned_root).unwrap();
        let abandoned_layout = SessionBinding {
            working_directory: abandoned_root.clone(),
            tool_root: abandoned_root.clone(),
            ..layout.clone()
        };
        let existing_file = abandoned_root.join("pre-existing");
        std::fs::write(&existing_file, b"keep").unwrap();
        // Fail the final receipt, after ordinary queue insertion. This
        // fixture also serves the empty Auto recovery below.
        let fail_publication = |store: &SessionStore| {
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "CREATE TEMP TRIGGER fixture_fail_task_baseline BEFORE INSERT ON audit
                 WHEN NEW.action='construction.preparation'
                   AND json_extract(NEW.data_json,'$.operation')='checkpoint_restore'
                   AND json_extract(NEW.data_json,'$.state')='applied'
                 BEGIN SELECT RAISE(ABORT,'fixture baseline failure'); END;",
                )
                .unwrap();
        };
        let clear_failure = |store: &SessionStore| {
            store
                .connection()
                .unwrap()
                .execute_batch("DROP TRIGGER fixture_fail_task_baseline")
                .unwrap();
        };
        fail_publication(&store);
        assert!(store
            .checkpoint_restore_baseline(
                &actor,
                &task_baseline,
                &abandoned_layout,
                &[],
                "host:abandoned-baseline",
                "local",
            )
            .is_err());
        clear_failure(&store);
        let abandoned = task_baseline_receipt(
            &store.connection().unwrap(),
            &actor,
            "host:abandoned-baseline",
        )
        .unwrap();
        assert_eq!(abandoned.state, PreparationState::Unknown);
        assert!(abandoned.sources.is_none());
        let abandoned_task = abandoned.initial_task().unwrap();
        store
            .verify_prepared_queue_custody(&abandoned_task.custody())
            .unwrap();
        assert!(store.checkpoint_delete(&actor, &current.id).is_err());
        {
            let connection = store.connection().unwrap();
            abandoned.candidate.require_absent_in(&connection).unwrap();
            assert!(
                crate::actor_queue::events(&connection, abandoned.candidate.prepare_id())
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                connection
                    .query_row(
                        "SELECT count(*) FROM local_coordination_leases WHERE run_id=?1",
                        [abandoned.candidate.prepare_id()],
                        |row| row.get::<_, usize>(0),
                    )
                    .unwrap(),
                0
            );
        }
        assert!(store
            .checkpoint_abandon_baseline(&viewer, "host:abandoned-baseline")
            .is_err());
        store
            .checkpoint_abandon_baseline(&actor, "host:abandoned-baseline")
            .unwrap();
        store
            .checkpoint_abandon_baseline(&actor, "host:abandoned-baseline")
            .unwrap();
        assert!(store
            .verify_prepared_queue_custody(&abandoned_task.custody())
            .is_err());
        assert_eq!(std::fs::read(&existing_file).unwrap(), b"keep");
        assert!(store
            .checkpoint_restore_baseline(
                &actor,
                &task_baseline,
                &abandoned_layout,
                &[],
                "host:abandoned-baseline",
                "local",
            )
            .is_err());

        // Source-free task-only restore uses ordinary execution admission.
        let task_root = temp.0.join("task-baseline");
        std::fs::create_dir(&task_root).unwrap();
        let task_layout = SessionBinding {
            working_directory: task_root.clone(),
            tool_root: task_root,
            ..layout.clone()
        };
        let task_restored = store
            .checkpoint_restore_baseline(
                &actor,
                &task_baseline,
                &task_layout,
                &[],
                "host:task-baseline",
                "fixture:baseline-site",
            )
            .unwrap();
        assert_eq!(task_restored.agent.admission, crate::AgentAdmission::Open);
        let (task_session, task_branch) = agent_owner::key_for_ref(
            &store.connection().unwrap(),
            &task_restored.agent.canonical_ref,
        )
        .unwrap();
        let task_queue = store.actor_queue_list(&actor, &task_session).unwrap();
        assert_eq!(task_queue.len(), 1);
        assert_eq!(task_queue[0].state, crate::QueueState::Pending);
        assert_eq!(
            task_queue[0].request.recipient_site,
            "fixture:baseline-site"
        );
        assert_eq!(task_queue[0].request.branch_id, task_branch);
        assert_eq!(
            crate::SecretStore::get(
                &store.local_secret_store().unwrap(),
                &task_queue[0].payload_ref
            )
            .unwrap()
            .as_slice(),
            task_text.as_bytes()
        );
        assert!(store.activate(&actor, &task_session, &task_branch).is_ok());
        assert!(store
            .checkpoint_abandon_baseline(&actor, "host:task-baseline")
            .is_err());
        let restored_history = store
            .checkpoint_restore_baseline(
                &actor,
                &history_only,
                &baseline_layout,
                &[],
                "host:history-baseline",
                "local",
            )
            .unwrap();
        assert_eq!(restored_history.source_coverage, SourceCoverage::None);
        assert_ne!(
            restored_history.agent.sandbox_ref.as_deref(),
            Some(original_sandbox.as_str())
        );
        assert_eq!(
            restored_history.agent.admission,
            crate::AgentAdmission::Open
        );
        let history_sandbox = restored_history.agent.sandbox_ref.as_deref().unwrap();
        assert!(store
            .sandbox_get(&actor, history_sandbox)
            .unwrap()
            .sources
            .is_empty());
        let (history_session, history_branch) = agent_owner::key_for_ref(
            &store.connection().unwrap(),
            &restored_history.agent.canonical_ref,
        )
        .unwrap();
        let history_view = store
            .read_agent_branch_view(&actor, &history_session, &history_branch)
            .unwrap();
        assert_eq!(history_view.history.turns.len(), 2);
        assert_eq!(history_view.session.binding, baseline_layout);
        let assert_custom_audience = |store: &SessionStore, agent: &AgentSnapshot, turns: usize| {
            let acl = store
                .agent_access_list_ref(&actor, &agent.canonical_ref)
                .unwrap();
            let entry = acl
                .entries
                .iter()
                .find(|entry| entry.principal_ref == viewer_ref)
                .unwrap();
            assert_eq!(entry.permissions, crate::AgentPermissions::VIEW_ONLY);
            assert_eq!(entry.grant_revision, 1);
            assert_eq!(acl.rev, 1);
            assert_eq!(acl.acl_rev, 1);
            let (session, branch) =
                agent_owner::key_for_ref(&store.connection().unwrap(), &agent.canonical_ref)
                    .unwrap();
            let visible = store
                .read_agent_branch_view(&viewer, &session, &branch)
                .unwrap();
            assert_eq!(visible.history.turns.len(), turns);
            assert_eq!(
                store
                    .agent_get(&viewer, &agent.canonical_ref)
                    .unwrap()
                    .caller_permissions,
                crate::AgentPermissions::VIEW_ONLY
            );
            let connection = store.connection().unwrap();
            assert_eq!(
                connection
                    .query_row(
                        "SELECT count(*) FROM history_grants WHERE destination_agent_ref=?1",
                        [&agent.canonical_ref],
                        |row| row.get::<_, usize>(0),
                    )
                    .unwrap(),
                1,
                "initial audience needs only one HistoryGrant"
            );
            assert_eq!(
                connection
                    .query_row(
                        "SELECT count(*) FROM agent_sandbox_grants_current WHERE session_id=?1 AND branch_id=?2",
                        params![session, branch],
                        |row| row.get::<_, usize>(0),
                    )
                    .unwrap(),
                0,
                "custom history ACL cannot mint source authority"
            );
        };
        assert_custom_audience(&store, &restored_history.agent, 2);
        // An empty Auto selection preserves its original earlier prefix.
        // Fresh policy may supply ephemeral, never uncaptured required, sources.
        let auto_root = temp.0.join("auto-baseline");
        std::fs::create_dir(&auto_root).unwrap();
        let auto_layout = SessionBinding {
            tool_root: auto_root.clone(),
            working_directory: auto_root,
            ..layout.clone()
        };
        let auto_request = CheckpointRestoreRequest {
            checkpoint: empty_auto.id.clone(),
            task: Some("fixture: empty Auto task".into()),
            ..request.clone()
        };
        let mut baseline_source = SourceBinding {
            name: SourceName::new("cache").unwrap(),
            driver: driver.clone(),
            binding_schema: 1,
            source_ref: "fixture:baseline-cache".into(),
            materialization: "fixture:current-cache".into(),
            access: SourceAccess::ReadOnly,
            checkpoint_policy: CheckpointPolicy::Required,
        };
        assert!(store
            .checkpoint_restore_baseline(
                &actor,
                &auto_request,
                &auto_layout,
                std::slice::from_ref(&baseline_source),
                "host:auto-required",
                "local",
            )
            .is_err());
        baseline_source.checkpoint_policy = CheckpointPolicy::Ephemeral;
        let baseline_set =
            SourceMutationSet::from_bindings(std::slice::from_ref(&baseline_source)).unwrap();
        let held = store
            .acquire_source_mutation_barrier(&baseline_set, "fixture:baseline-busy")
            .unwrap();
        assert!(store
            .checkpoint_restore_baseline(
                &actor,
                &auto_request,
                &auto_layout,
                std::slice::from_ref(&baseline_source),
                "host:auto-baseline",
                "local",
            )
            .is_err());
        held.settle(crate::SourceMutationOutcome::Settled).unwrap();
        fail_publication(&store);
        assert!(store
            .checkpoint_restore_baseline(
                &actor,
                &auto_request,
                &auto_layout,
                std::slice::from_ref(&baseline_source),
                "host:auto-baseline",
                "fixture:auto-site",
            )
            .is_err());
        clear_failure(&store);
        let original_auto =
            task_baseline_receipt(&store.connection().unwrap(), &actor, "host:auto-baseline")
                .unwrap();
        assert_eq!(original_auto.state, PreparationState::Unknown);
        let auto_task = original_auto.initial_task().unwrap().clone();
        assert!(store.checkpoint_delete(&actor, &empty_auto.id).is_err());
        drop(store);
        let store = SessionStore::open_with_sources(&temp.0, registry.clone(), limit).unwrap();
        assert!(store
            .checkpoint_resume_baseline(&actor, "host:auto-baseline", &auto_layout, &[],)
            .is_err());
        let restored_auto = store
            .checkpoint_resume_baseline(
                &actor,
                "host:auto-baseline",
                &auto_layout,
                std::slice::from_ref(&baseline_source),
            )
            .unwrap();
        let auto_pending = store
            .actor_queue_list(&actor, original_auto.candidate.prepare_id())
            .unwrap();
        assert_eq!(auto_pending.len(), 1);
        assert_eq!(auto_pending[0].request.recipient_site, "fixture:auto-site");
        assert_eq!(auto_pending[0].payload_ref, auto_task.payload_ref);
        assert_eq!(auto_pending[0].state, crate::QueueState::Pending);
        assert!(store
            .actor_queue_claim_next(
                &actor,
                original_auto.candidate.prepare_id(),
                &auto_pending[0].request.branch_id,
                "fixture:auto-site",
                "fixture",
                auto_pending[0].request.writer_epoch,
            )
            .is_err());
        assert_eq!(
            restored_auto.source_coverage,
            SourceCoverage::AutoAllRequired
        );
        assert_eq!(restored_auto.agent.admission, crate::AgentAdmission::Held);
        let auto_sandbox = restored_auto.agent.sandbox_ref.as_deref().unwrap();
        assert_eq!(
            store
                .sandbox_get(&actor, auto_sandbox)
                .unwrap()
                .sources
                .len(),
            1
        );
        let (auto_session, auto_branch) = agent_owner::key_for_ref(
            &store.connection().unwrap(),
            &restored_auto.agent.canonical_ref,
        )
        .unwrap();
        let auto_view = store
            .read_agent_branch_view(&actor, &auto_session, &auto_branch)
            .unwrap();
        assert_eq!(auto_view.history.turns.len(), 1);
        assert_eq!(auto_view.history.turns[0].prompt, "turn 1");
        assert!(store.activate(&actor, &auto_session, &auto_branch).is_err());
        assert!(store
            .checkpoint_create_empty_auto(
                &actor,
                &restored_auto.agent.canonical_ref,
                None,
                "host:ephemeral-only",
            )
            .unwrap()
            .sources
            .is_empty());
        // Replay requires neither a live checkpoint nor a new valid layout.
        store.checkpoint_delete(&actor, &empty_auto.id).unwrap();
        store.checkpoint_delete(&actor, &current.id).unwrap();
        assert_eq!(
            store
                .checkpoint_resume_baseline(&actor, "host:auto-baseline", &baseline_layout, &[])
                .unwrap(),
            restored_auto
        );
        assert_eq!(
            store
                .checkpoint_replay_restore(&actor, &task_baseline, "host:task-baseline")
                .unwrap(),
            Some(task_restored)
        );
        assert_eq!(
            store
                .actor_queue_list(&actor, original_auto.candidate.prepare_id())
                .unwrap(),
            auto_pending
        );
        let invalid_layout = SessionBinding {
            tool_root: PathBuf::new(),
            ..layout.clone()
        };
        assert_eq!(
            store
                .checkpoint_restore_baseline(
                    &actor,
                    &auto_request,
                    &invalid_layout,
                    &[],
                    "host:auto-baseline",
                    "local",
                )
                .unwrap(),
            restored_auto
        );
        assert_eq!(
            store
                .checkpoint_replay_restore(&actor, &history_only, "host:history-baseline")
                .unwrap(),
            Some(restored_history.clone())
        );
        let changed_request = CheckpointRestoreRequest {
            title: Some("changed".into()),
            ..auto_request.clone()
        };
        assert!(store
            .checkpoint_restore_baseline(
                &actor,
                &changed_request,
                &auto_layout,
                &[],
                "host:auto-baseline",
                "local",
            )
            .is_err());
        assert!(store
            .checkpoint_restore_baseline(
                &actor,
                &auto_request,
                &auto_layout,
                &[],
                "host:retired-auto",
                "local",
            )
            .is_err());
        assert!(store
            .checkpoint_prepare_restore(
                &actor,
                &history_only,
                &layout,
                "host:no-sources",
                "local",
                |_, _, _| panic!("no implicit source-state fallback")
            )
            .is_err());
        assert!(store
            .checkpoint_prepare_restore(
                &actor,
                &request,
                &layout,
                "host:partial",
                "local",
                |candidate, sandbox, selected| {
                    let mut prepared = plan(candidate, sandbox, selected)?;
                    prepared.sources.pop();
                    Ok(prepared)
                }
            )
            .is_err());

        // From here the captured-source restore also owns initial input
        // custody. The task is not a journal Turn or a source grant.
        let task_text = "fixture: restored initial input";
        let request = CheckpointRestoreRequest {
            task: Some(task_text.into()),
            ..request
        };
        assert!(store
            .checkpoint_prepare_restore(
                &actor,
                &request,
                &layout,
                "host:bad-task-site",
                "",
                |_, _, _| panic!("invalid task site must refuse before planning"),
            )
            .is_err());

        // Abandon an actual private candidate without touching the source
        // checkpoint or publishing either candidate identity.
        let abandoned = store
            .checkpoint_prepare_restore(
                &actor,
                &request,
                &layout,
                "host:abandon-restore",
                "local",
                plan,
            )
            .unwrap();
        let abandoned_id = abandoned.prepare_id().to_owned();
        let abandoned_sandbox = abandoned.sandbox_ref().to_owned();
        let abandoned_sources = abandoned.sources().clone();
        let abandoned_task = abandoned.receipt.initial_task().unwrap().clone();
        store
            .verify_prepared_queue_custody(&abandoned_task.custody())
            .unwrap();
        assert!(store
            .checkpoint_abandon_restore::<()>(&actor, "host:abandon-restore", |_, _, _| panic!(
                "live candidate cannot be stolen"
            ))
            .is_err());
        assert!(abandoned
            .abandon::<()>(|_, _, _| { Err(Error::Conflict("fixture interrupted source cleanup")) })
            .is_err());
        // Until every source is settled, retain the original input too.
        store
            .verify_prepared_queue_custody(&abandoned_task.custody())
            .unwrap();
        store
            .checkpoint_abandon_restore(&actor, "host:abandon-restore", |_, selected, sources| {
                assert_eq!(selected, &anchors);
                assert_eq!(sources, &abandoned_sources);
                assert!(store
                    .acquire_source_mutation_barrier(
                        &SourceMutationSet::from_anchors(&selected.anchors)?,
                        "during-abandon",
                    )
                    .is_err());
                // Trusted fixture reports that no materialization was started.
                Ok(())
            })
            .unwrap();
        assert!(store
            .verify_prepared_queue_custody(&abandoned_task.custody())
            .is_err());
        store
            .checkpoint_abandon_restore::<()>(&actor, "host:abandon-restore", |_, _, _| {
                panic!("rejected receipt must not reinspect")
            })
            .unwrap();
        assert!(store
            .checkpoint_prepare_restore(
                &actor,
                &request,
                &layout,
                "host:abandon-restore",
                "local",
                |_, _, _| panic!("rejected receipt must not replan"),
            )
            .is_err());
        assert!(store
            .checkpoint_replay_restore(&actor, &request, "host:abandon-restore")
            .is_err());
        assert!(store.sandbox_get(&actor, &abandoned_sandbox).is_err());
        {
            let connection = store.connection().unwrap();
            assert!(!connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1)",
                    [&abandoned_id],
                    |r| r.get::<_, bool>(0),
                )
                .unwrap());
        }

        let prepared = store
            .checkpoint_prepare_restore(
                &actor,
                &request,
                &layout,
                "host:restore",
                "fixture:restore-site",
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
        let task = receipt.initial_task().unwrap().clone();
        assert_eq!(task.execution_site.as_deref(), Some("fixture:restore-site"));
        store
            .verify_prepared_queue_custody(&task.custody())
            .unwrap();
        assert!(!serde_json::to_string(&receipt).unwrap().contains(task_text));
        let candidate = prepared.prepare_id().to_owned();
        let sandbox = prepared.sandbox_ref().to_owned();
        assert_ne!(candidate, session.id);
        assert!(store.actor_queue_list(&actor, &candidate).is_err());
        assert_eq!(prepared.boundary().history_base_id, base.id);
        assert_eq!(prepared.anchors(), &anchors);
        assert!(store
            .checkpoint_prepare_restore(
                &actor,
                &request,
                &layout,
                "host:restore",
                "local",
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
        assert_eq!(
            store
                .checkpoint_replay_restore(&actor, &auto_request, "host:auto-baseline")
                .unwrap(),
            Some(restored_auto)
        );
        assert_eq!(
            store
                .checkpoint_replay_restore(&actor, &history_only, "host:history-baseline")
                .unwrap(),
            Some(restored_history)
        );
        assert!(store
            .checkpoint_inspect_restore(&actor, "host:restore", |_, _, _| panic!(
                "missing driver must fail closed"
            ))
            .is_err());
        drop(store);
        let store = SessionStore::open_with_sources(&temp.0, registry.clone(), limit).unwrap();
        store
            .verify_prepared_queue_custody(&task.custody())
            .unwrap();
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
            // Fail the final receipt after history, ACL and queue publication
            // inside the same transaction. None may become visible alone.
            connection
                .execute_batch(
                    "CREATE TEMP TRIGGER fixture_fail_restore BEFORE INSERT ON audit
                     WHEN NEW.action='construction.preparation'
                      AND json_extract(NEW.data_json,'$.operation')='checkpoint_restore'
                      AND json_extract(NEW.data_json,'$.state')='applied'
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
            assert_eq!(current.task, receipt.task);
            require_candidate_absent(&connection, &receipt.candidate).unwrap();
            assert!(crate::actor_queue::events(&connection, &candidate)
                .unwrap()
                .is_empty());
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
        store
            .verify_prepared_queue_custody(&task.custody())
            .unwrap();
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
        assert_custom_audience(&store, &restored.agent, 1);
        let restored_ref = restored.agent.canonical_ref.clone();
        let ids = receipt.candidate.agent_ids().unwrap();
        let queue = store.actor_queue_list(&actor, &ids.session).unwrap();
        assert_eq!(queue.len(), 1);
        let input = &queue[0];
        assert_eq!(input.state, crate::QueueState::Pending);
        assert_eq!(input.request.branch_id, ids.branch);
        assert_eq!(input.request.recipient_site, "fixture:restore-site");
        assert_eq!(input.request.request_id, task.queue_request_id);
        assert_eq!(input.payload_ref, task.payload_ref);
        assert!(!serde_json::to_string(input).unwrap().contains(task_text));
        let plaintext =
            crate::SecretStore::get(&store.local_secret_store().unwrap(), &input.payload_ref)
                .unwrap();
        assert_eq!(plaintext.as_slice(), task_text.as_bytes());
        drop(plaintext);
        let changed_task = CheckpointRestoreRequest {
            task: Some("different initial input".into()),
            ..request.clone()
        };
        assert!(store
            .checkpoint_replay_restore(&actor, &changed_task, "host:restore")
            .is_err());
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
            .actor_queue_claim_next(
                &actor,
                &ids.session,
                &ids.branch,
                "fixture:restore-site",
                "fixture",
                1,
            )
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
        assert!(store
            .checkpoint_abandon_restore::<()>(&actor, "host:restore", |_, _, _| panic!(
                "Applied restore cannot be abandoned"
            ))
            .is_err());
        store.checkpoint_delete(&actor, &checkpoint).unwrap();
        assert!(store
            .checkpoint_prepare_restore(
                &actor,
                &request,
                &layout,
                "host:retired",
                "local",
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
        assert_eq!(store.actor_queue_list(&actor, &ids.session).unwrap(), queue);
        store
            .verify_prepared_queue_custody(&task.custody())
            .unwrap();
        // Normal pending-input deletion owns cancellation before terminal
        // close; restore does not invent a separate task lifecycle.
        let writer_epoch = queue[0].request.writer_epoch;
        store
            .actor_queue_delete(&actor, &ids.session, &task.queue_request_id, writer_epoch)
            .unwrap();
        assert!(store
            .verify_prepared_queue_custody(&task.custody())
            .is_err());
        assert!(store.sandbox_restore_preparation(&actor, &sandbox).is_err());
        assert!(store.sandbox_close(&actor, &sandbox, None).is_err());
        store
            .agent_request_close(&actor, &restored_ref, None)
            .unwrap();
        assert!(store.sandbox_close(&actor, &sandbox, None).is_err());
        let closed_agent = store.agent_finish_close(&actor, &restored_ref).unwrap();

        // Legacy ambient selection must not reinterpret the same
        // source-bearing tool root through a different cwd. A trusted
        // unused logical alias may still exist, but cannot admit an Agent.
        let alias_layout = SessionBinding {
            working_directory: temp.0.join("different-cwd"),
            ..layout.clone()
        };
        std::fs::create_dir(&alias_layout.working_directory).unwrap();
        assert!(store
            .create(&actor, "same root", alias_layout.clone())
            .is_err());
        let unused_alias = store
            .sandbox_create_fresh(&actor, &snapshot.project_ref, &alias_layout)
            .unwrap();
        {
            let connection = store.connection().unwrap();
            let binding =
                crate::agent_binding_in(&connection, &actor, &ids.session, &ids.branch).unwrap();
            assert!(sandbox_owner::preflight_initial_selection(
                &connection,
                &actor,
                &binding.domain_handle,
                &binding.identity_handle,
                &binding.project_handle,
                &alias_layout,
                Some(&unused_alias.canonical_ref),
            )
            .is_err());
        }

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
            assert!(sandbox_owner::require_execution_in(&connection, &ids.session).is_err());
            assert!(
                sandbox_owner::require_fresh_layout_in(&connection, &project, &layout).is_err()
            );
            let binding =
                crate::agent_binding_in(&connection, &actor, &ids.session, &ids.branch).unwrap();
            assert!(sandbox_owner::preflight_initial_selection(
                &connection,
                &actor,
                &binding.domain_handle,
                &binding.identity_handle,
                &binding.project_handle,
                &alias_layout,
                Some(&unused_alias.canonical_ref),
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
