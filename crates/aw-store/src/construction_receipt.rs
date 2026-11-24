//! Existing immutable construction receipts, shared by task and source custody.
//! Indexed replay lookup; no separate operation ledger or public resource.
use crate::{
    agent_owner::{self, AgentKey, AgentPermission},
    audit_with_security_metadata, db, label,
    resources::SessionCandidateIds,
    source_compatibility::{compatibility, SourceCompatibility},
    user_for_principal, Error, Result, SessionStore, SourceMutationBarrier, SourceMutationOutcome,
};
use aw_agent_core::{
    ExternalEvidence, NamedPreparedSource, PreparedSourceSet, PrincipalId, SourceDriverRegistry,
    SourceDriverRegistryReader, SourceOperationKind, SourcePublicationFence,
    SourcePublicationProof, SourceSetLimit, HARD_MAX_SOURCES_PER_SANDBOX,
    MAX_SOURCE_ANCHOR_FIELDS_BYTES,
};
use rusqlite::{params, Connection, Row, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

// Each descriptor's bounded fields fit within the same encoded allowance as
// an anchor. Decode at most one receipt at a time, including after restart.
const MAX_RECEIPT_BYTES: usize =
    HARD_MAX_SOURCES_PER_SANDBOX * (MAX_SOURCE_ANCHOR_FIELDS_BYTES + 8192) + 16384;
const ACTION: &str = "construction.preparation";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConstructionOperation {
    #[default]
    AgentCreate,
    CheckpointCreate,
    CheckpointRestore,
    SandboxCreate,
}

impl ConstructionOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::AgentCreate => "agent_create",
            Self::CheckpointCreate => "checkpoint_create",
            Self::CheckpointRestore => "checkpoint_restore",
            Self::SandboxCreate => "sandbox_create",
        }
    }

    pub(crate) fn scope(self, creator: &str, origin: AgentKey<'_>) -> String {
        match self {
            // Preserve the existing task receipt's authenticated origin scope.
            Self::AgentCreate => format!("{}/{}", origin.session, origin.branch),
            // Host checkpoint-create shares the published checkpoint's caller scope.
            _ => creator.to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PreparationState {
    Prepared,
    Applied,
    Rejected,
    Unknown,
}

impl PreparationState {
    pub(crate) fn unresolved(self) -> bool {
        matches!(self, Self::Prepared | Self::Unknown)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InitialTask {
    pub(crate) queue_request_id: String,
    pub(crate) payload_ref: String,
    pub(crate) payload_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ConstructionCandidate {
    Agent {
        session: String,
        root: String,
        branch: String,
        // Only an operation-owned fresh Sandbox, never an existing caller-owned
        // Sandbox to be discarded. Existing selection belongs to semantic input.
        sandbox: Option<String>,
    },
    Checkpoint {
        checkpoint: String,
    },
    Sandbox {
        sandbox: String,
    },
}

impl ConstructionCandidate {
    pub(crate) fn agent_ids(&self) -> Result<SessionCandidateIds> {
        match self {
            Self::Agent {
                session,
                root,
                branch,
                ..
            } => Ok(SessionCandidateIds {
                session: session.clone(),
                root: root.clone(),
                branch: branch.clone(),
            }),
            _ => Err(Error::Storage(
                "construction receipt has no Agent candidate",
            )),
        }
    }

    pub(crate) fn prepare_id(&self) -> &str {
        match self {
            Self::Agent { session, .. } => session,
            Self::Checkpoint { checkpoint } => checkpoint,
            Self::Sandbox { sandbox } => sandbox
                .rsplit_once("/s:")
                .map_or(sandbox, |(_, handle)| handle),
        }
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Agent {
                session,
                root,
                branch,
                sandbox,
            } => {
                for id in [session, root, branch] {
                    label(id, 128)?;
                }
                if let Some(sandbox) = sandbox {
                    label(sandbox, 2048)?;
                }
            }
            Self::Checkpoint { checkpoint } => label(checkpoint, 128)?,
            Self::Sandbox { sandbox } => label(sandbox, 2048)?,
        }
        Ok(())
    }
}

/// Complete private candidate and operation-owned external recovery identity.
/// Schema-specific source details stay in the shared core descriptor vocabulary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConstructionPreparation {
    pub(crate) operation: ConstructionOperation,
    pub(crate) scope: String,
    pub(crate) creator: String,
    pub(crate) parent_session_id: String,
    pub(crate) parent_branch_id: String,
    pub(crate) request_id: String,
    pub(crate) fingerprint: String,
    pub(crate) state: PreparationState,
    pub(crate) candidate: ConstructionCandidate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) task: Option<InitialTask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) sources: Option<PreparedSourceSet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) capture_boundary: Option<crate::checkpoint_owner::capture::CaptureBoundary>,
    pub(crate) result_ref: Option<String>,
    pub(crate) note: Option<String>,
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl ConstructionPreparation {
    pub(crate) fn origin(&self) -> AgentKey<'_> {
        AgentKey::new(&self.parent_session_id, &self.parent_branch_id)
    }

    pub(crate) fn require_actor(&self, connection: &Connection, actor: &PrincipalId) -> Result<()> {
        if user_for_principal(connection, actor)?.as_deref() != Some(&self.creator) {
            return Err(Error::Denied(
                "prior construction invocation is non-retryable; result unavailable",
            ));
        }
        Ok(())
    }

    pub(crate) fn initial_task(&self) -> Result<&InitialTask> {
        self.task
            .as_ref()
            .ok_or(Error::Storage("construction receipt has no initial task"))
    }

    fn validate(&self) -> Result<()> {
        for id in [&self.parent_session_id, &self.parent_branch_id] {
            label(id, 128)?;
        }
        crate::stable_user_principal(&self.creator)?;
        label(&self.request_id, 256)?;
        if self.scope != self.operation.scope(&self.creator, self.origin())
            || !digest(&self.fingerprint)
            || (self.state == PreparationState::Applied) != self.result_ref.is_some()
            || self.note.as_ref().is_some_and(|note| note.len() > 8192)
        {
            return Err(Error::Storage(
                "invalid construction receipt identity or state",
            ));
        }
        if let Some(result) = &self.result_ref {
            label(result, 2048)?;
        }
        self.candidate.validate()?;
        let candidate_matches = matches!(
            (&self.operation, &self.candidate),
            (
                ConstructionOperation::AgentCreate | ConstructionOperation::CheckpointRestore,
                ConstructionCandidate::Agent { .. }
            ) | (
                ConstructionOperation::CheckpointCreate,
                ConstructionCandidate::Checkpoint { .. }
            ) | (
                ConstructionOperation::SandboxCreate,
                ConstructionCandidate::Sandbox { .. }
            )
        );
        if !candidate_matches
            || (self.task.is_some()
                && !matches!(self.candidate, ConstructionCandidate::Agent { .. }))
        {
            return Err(Error::Storage(
                "construction receipt candidate does not match operation",
            ));
        }
        if let Some(task) = &self.task {
            label(&task.queue_request_id, 128)?;
            label(&task.payload_ref, 2048)?;
            if !digest(&task.payload_sha256) || task.payload_ref.contains("://") {
                return Err(Error::Storage("invalid initial task custody identity"));
            }
        }
        if let Some(boundary) = &self.capture_boundary {
            if self.operation != ConstructionOperation::CheckpointCreate {
                return Err(Error::Storage(
                    "capture boundary belongs to checkpoint creation",
                ));
            }
            boundary.validate(
                self.sources
                    .as_ref()
                    .ok_or(Error::Storage("capture descriptors missing"))?,
            )?;
        }
        if let Some(sources) = &self.sources {
            sources.validate()?;
            if matches!(
                self.candidate,
                ConstructionCandidate::Agent { sandbox: None, .. }
            ) {
                return Err(Error::Storage(
                    "source-bearing Agent candidate requires a fresh Sandbox identity",
                ));
            }
            let kind = if self.operation == ConstructionOperation::CheckpointCreate {
                SourceOperationKind::Capture
            } else {
                SourceOperationKind::Restore
            };
            if sources.operation != kind || sources.prepare_id != self.candidate.prepare_id() {
                return Err(Error::Conflict(
                    "prepared source set does not belong to construction candidate",
                ));
            }
        } else if self.operation != ConstructionOperation::AgentCreate || self.task.is_none() {
            return Err(Error::Storage(
                "external construction receipt has no custody",
            ));
        }
        Ok(())
    }

    fn admit(&self, registry: &SourceDriverRegistry, limit: SourceSetLimit) -> Result<()> {
        if let Some(sources) = &self.sources {
            limit.admit(sources.sources.len())?;
        }
        self.validate()?;
        if let Some(sources) = &self.sources {
            let reader = registry.reader()?;
            for source in &sources.sources {
                reader.resolve(&source.prepared.descriptor.pin()?)?;
            }
            if let Some(boundary) = &self.capture_boundary {
                for binding in &boundary.bindings {
                    reader.resolve(&binding.binding.pin()?)?;
                }
            }
        }
        Ok(())
    }
}

// Decode the exact historical task-only schema, without rewriting audit facts.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyTaskPreparation {
    request_id: String,
    fingerprint: String,
    state: PreparationState,
    parent_branch_id: String,
    candidate_session_id: String,
    candidate_root_id: String,
    candidate_branch_id: String,
    queue_request_id: String,
    payload_ref: String,
    payload_sha256: String,
    result_ref: Option<String>,
    note: Option<String>,
}

fn decode_receipt(connection: &Connection, row: &Row<'_>) -> Result<ConstructionPreparation> {
    let session: String = db(row.get(0))?;
    let action: String = db(row.get(1))?;
    let data = db(row.get_ref(3))?;
    let data = data
        .as_str()
        .map_err(|_| Error::Storage("invalid construction receipt encoding"))?;
    if data.len() > MAX_RECEIPT_BYTES {
        return Err(Error::Limit("construction receipt exceeds encoded bound"));
    }
    let receipt = if action == ACTION {
        crate::decode::<ConstructionPreparation>(data)?
    } else if action == "agent.create.preparation" {
        let legacy: LegacyTaskPreparation = crate::decode(data)?;
        // The original security audit fact captured the stable user. Do not
        // reinterpret an old raw principal through a possibly changed mapping.
        let created_at: i64 = db(row.get(4))?;
        let mut creators = db(connection.prepare_cached(
            "SELECT DISTINCT actor_user_id FROM security_audit_events
             WHERE action='agent.create.preparation' AND resource_type='session'
               AND resource_id=?1 AND created_at=?2 AND metadata_json=?3
             LIMIT 2",
        ))?;
        let mut matches = db(creators.query(params![session, created_at, data]))?;
        let creator: String = db(matches.next())?
            .ok_or(Error::Denied("legacy construction caller unavailable"))
            .and_then(|row| db(row.get::<_, Option<String>>(0)))?
            .ok_or(Error::Denied("legacy construction caller unavailable"))?;
        if db(matches.next())?.is_some() {
            return Err(Error::Denied("ambiguous legacy construction caller"));
        }
        let scope = ConstructionOperation::AgentCreate
            .scope(&creator, AgentKey::new(&session, &legacy.parent_branch_id));
        ConstructionPreparation {
            operation: ConstructionOperation::AgentCreate,
            scope,
            creator,
            parent_session_id: session.clone(),
            parent_branch_id: legacy.parent_branch_id,
            request_id: legacy.request_id,
            fingerprint: legacy.fingerprint,
            state: legacy.state,
            candidate: ConstructionCandidate::Agent {
                session: legacy.candidate_session_id,
                root: legacy.candidate_root_id,
                branch: legacy.candidate_branch_id,
                sandbox: None,
            },
            task: Some(InitialTask {
                queue_request_id: legacy.queue_request_id,
                payload_ref: legacy.payload_ref,
                payload_sha256: legacy.payload_sha256,
            }),
            sources: None,
            capture_boundary: None,
            result_ref: legacy.result_ref,
            note: legacy.note,
        }
    } else {
        return Err(Error::Storage("invalid construction receipt action"));
    };
    receipt.validate()?;
    if receipt.parent_session_id != session {
        return Err(Error::Storage("construction receipt origin changed"));
    }
    Ok(receipt)
}

/// The expression index in v57 covers both historical and current encodings.
pub(crate) fn lookup_in(
    connection: &Connection,
    operation: ConstructionOperation,
    scope: &str,
    request_id: &str,
) -> Result<Option<ConstructionPreparation>> {
    let mut query = db(connection.prepare_cached(
        "SELECT session_id,action,actor,data_json,created_at FROM audit
         WHERE action IN ('agent.create.preparation','construction.preparation')
           AND COALESCE(json_extract(data_json,'$.scope'),session_id||'/'||json_extract(data_json,'$.parent_branch_id'))=?1
           AND COALESCE(json_extract(data_json,'$.operation'),'agent_create')=?2
           AND json_extract(data_json,'$.request_id')=?3
         ORDER BY seq DESC LIMIT 1",
    ))?;
    let mut rows = db(query.query(params![scope, operation.as_str(), request_id]))?;
    let Some(row) = db(rows.next())? else {
        return Ok(None);
    };
    let receipt = decode_receipt(connection, row)?;
    if receipt.operation != operation || receipt.scope != scope || receipt.request_id != request_id
    {
        return Err(Error::Storage("construction receipt replay key changed"));
    }
    Ok(Some(receipt))
}

fn append_in(
    tx: &Transaction<'_>,
    actor: &PrincipalId,
    receipt: &ConstructionPreparation,
) -> Result<()> {
    let data = serde_json::to_value(receipt)
        .map_err(|_| Error::Storage("cannot encode construction receipt"))?;
    // Recovery locators remain in the owner-private receipt, not security audit
    // presentation metadata. No model/tool result contains this receipt.
    audit_with_security_metadata(
        tx,
        &receipt.parent_session_id,
        actor,
        ACTION,
        data,
        Some(serde_json::json!({"operation": receipt.operation, "state": receipt.state})),
    )
}

/// Claim once in the existing receipt owner, before any external effect.
/// The caller still owns operation-specific source/capability preflight.
pub(crate) fn claim_in(
    tx: &Transaction<'_>,
    actor: &PrincipalId,
    registry: &SourceDriverRegistry,
    limit: SourceSetLimit,
    receipt: &ConstructionPreparation,
) -> Result<()> {
    if receipt.state != PreparationState::Prepared || receipt.note.is_some() {
        return Err(Error::Conflict("construction claim must begin Prepared"));
    }
    receipt.require_actor(tx, actor)?;
    if lookup_in(tx, receipt.operation, &receipt.scope, &receipt.request_id)?.is_some() {
        return Err(Error::Conflict(
            "construction request already claimed; inspect original receipt",
        ));
    }
    receipt.admit(registry, limit)?;
    if receipt.operation == ConstructionOperation::CheckpointCreate {
        let published: bool = db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM checkpoints WHERE created_by=?1 AND request_id=?2)",
            params![receipt.creator, receipt.request_id],
            |row| row.get(0),
        ))?;
        if published {
            return Err(Error::Conflict("checkpoint request already published"));
        }
    }
    agent_owner::authorize_permission(tx, actor, receipt.origin(), AgentPermission::View)?;
    append_in(tx, actor, receipt)
}

fn current_unresolved_in(
    tx: &Transaction<'_>,
    actor: &PrincipalId,
    expected: &ConstructionPreparation,
) -> Result<ConstructionPreparation> {
    expected.require_actor(tx, actor)?;
    let current = lookup_in(
        tx,
        expected.operation,
        &expected.scope,
        &expected.request_id,
    )?
    .ok_or(Error::Storage("construction receipt disappeared"))?;
    if &current != expected || !current.state.unresolved() {
        return Err(Error::Conflict(
            "construction receipt changed; inspect before retry",
        ));
    }
    Ok(current)
}

/// CAS on the exact previous receipt. Source-bearing terminal transitions
/// must use the fenced publication or reconciled cleanup path.
pub(crate) fn transition_in(
    tx: &Transaction<'_>,
    actor: &PrincipalId,
    expected: &ConstructionPreparation,
    state: PreparationState,
    result_ref: Option<String>,
    note: Option<String>,
) -> Result<ConstructionPreparation> {
    let current = current_unresolved_in(tx, actor, expected)?;
    if state == PreparationState::Prepared
        || (expected.sources.is_some() && state != PreparationState::Unknown)
    {
        return Err(Error::Conflict(
            "source construction requires reconciled publication or cleanup",
        ));
    }
    let next = ConstructionPreparation {
        state,
        result_ref,
        note,
        ..current
    };
    next.validate()?;
    if &next != expected {
        append_in(tx, actor, &next)?;
    }
    Ok(next)
}

/// The common source-bearing publication boundary. Resource owners supply
/// their exact in-transaction authority revalidation and publication using
/// their existing lower constructors. All external guards are owned here
/// until SQLite has finished committing (or rolling back).
pub(crate) fn publish_sources<F: SourcePublicationFence, T>(
    store: &SessionStore,
    actor: &PrincipalId,
    expected: &ConstructionPreparation,
    barrier: &SourceMutationBarrier<'_>,
    evidence: &[ExternalEvidence],
    proofs: Vec<SourcePublicationProof<F>>,
    publish: impl FnOnce(&Transaction<'_>) -> Result<(String, T)>,
) -> Result<T> {
    let sources = expected.sources.as_ref().ok_or(Error::Storage(
        "source construction has no prepared descriptors",
    ))?;
    if evidence.len() != sources.sources.len() || proofs.len() != evidence.len() {
        return Err(Error::Conflict(
            "source publication requires the complete evidence set",
        ));
    }
    for (proof, inspected) in proofs.iter().zip(evidence) {
        proof.require_evidence(inspected)?;
    }

    let mut connection = store.connection()?;
    let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let current = current_unresolved_in(&tx, actor, expected)?;
    barrier.require_preparation_in(&tx, &sources.prepare_id)?;
    let (result_ref, result) = publish(&tx)?;
    // The resource constructor must have published the operation-owned
    // candidate, not an existing or substituted resource.
    let candidate_ref = match &current.candidate {
        ConstructionCandidate::Checkpoint { checkpoint } => {
            let published: bool = db(tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM checkpoints WHERE checkpoint_id=?1
                    AND created_by=?2 AND request_id=?3 AND request_fingerprint=?4)",
                params![
                    checkpoint,
                    current.creator,
                    current.request_id,
                    current.fingerprint
                ],
                |row| row.get(0),
            ))?;
            if !published {
                return Err(Error::Conflict(
                    "prepared checkpoint candidate was not published",
                ));
            }
            checkpoint.clone()
        }
        ConstructionCandidate::Agent {
            session,
            branch,
            sandbox,
            ..
        } => {
            let actual_sandbox: Option<String> = db(tx.query_row(
                "SELECT sandbox_id FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
                params![session, branch],
                |row| row.get(0),
            ))?;
            if &actual_sandbox != sandbox {
                return Err(Error::Conflict(
                    "prepared Agent Sandbox differs from publication",
                ));
            }
            agent_owner::canonical_ref(&tx, AgentKey::new(session, branch))?
        }
        ConstructionCandidate::Sandbox { sandbox } => {
            let published: bool = db(tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM sandboxes_current WHERE sandbox_id=?1 AND lifecycle='open')",
                [sandbox], |row| row.get(0),
            ))?;
            if !published {
                return Err(Error::Conflict(
                    "prepared Sandbox candidate was not published",
                ));
            }
            sandbox.clone()
        }
    };
    if result_ref != candidate_ref {
        return Err(Error::Conflict(
            "publication result differs from prepared candidate",
        ));
    }
    let applied = ConstructionPreparation {
        state: PreparationState::Applied,
        result_ref: Some(result_ref),
        note: None,
        ..current
    };
    applied.validate()?;
    append_in(&tx, actor, &applied)?;
    barrier.settle_in(&tx, SourceMutationOutcome::Settled)?;
    db(tx.commit())?;
    // Provider guard destructors may need Store access.
    drop(connection);
    drop(proofs);
    Ok(result)
}

/// Stream only the current source-bearing unresolved receipts. Missing schemas
/// remain a diagnostic, never a reason to delete or reinterpret the descriptor.
/// The same read-side registry snapshot is used by all durable source owners.
pub(crate) fn visit_unresolved_sources(
    connection: &Connection,
    registry: &SourceDriverRegistryReader,
    mut visit: impl FnMut(
        &ConstructionPreparation,
        &NamedPreparedSource,
        SourceCompatibility,
    ) -> Result<()>,
) -> Result<()> {
    // The initial Prepared claim owns the dependency, even if a later
    // current receipt is malformed. The unique preparation index bounds
    // this scan to source-bearing claims, not the Session audit history.
    let mut query = db(connection.prepare(
        "SELECT session_id,action,actor,data_json,created_at FROM audit
         WHERE action='construction.preparation'
           AND json_extract(data_json,'$.state')='prepared'
           AND json_type(data_json,'$.sources')='object'",
    ))?;
    let mut rows = db(query.query([]))?;
    while let Some(row) = db(rows.next())? {
        let claim = decode_receipt(connection, row)?;
        let receipt = lookup_in(connection, claim.operation, &claim.scope, &claim.request_id)?
            .ok_or(Error::Storage(
                "prepared source claim lost its current receipt",
            ))?;
        if claim.creator != receipt.creator
            || claim.parent_session_id != receipt.parent_session_id
            || claim.parent_branch_id != receipt.parent_branch_id
            || claim.fingerprint != receipt.fingerprint
            || claim.candidate != receipt.candidate
            || claim.task != receipt.task
            || claim.sources != receipt.sources
            || claim.capture_boundary != receipt.capture_boundary
        {
            return Err(Error::Storage(
                "prepared source receipt changed its recovery identity",
            ));
        }
        if !receipt.state.unresolved() {
            continue;
        }
        let sources = receipt
            .sources
            .as_ref()
            .ok_or(Error::Storage("source custody missing"))?;
        for (index, source) in sources.sources.iter().enumerate() {
            let mut status = compatibility(registry, &source.prepared.descriptor.pin()?)?;
            if let Some(boundary) = &receipt.capture_boundary {
                if compatibility(registry, &boundary.bindings[index].binding.pin()?)?
                    == SourceCompatibility::RecoveryRequired
                {
                    status = SourceCompatibility::RecoveryRequired;
                }
            }
            visit(&receipt, source, status)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{id, SessionBinding, SessionStore};
    use aw_agent_core::{
        DriverKey, PreparedSourceOperation, RecoveryDescriptor, RecoveryField, RecoveryFieldKind,
        SchemaSupport, SourceDriverSchemas, SourceName,
    };
    use rusqlite::TransactionBehavior;
    use std::path::PathBuf;

    struct PrivateTemp(PathBuf);
    impl Drop for PrivateTemp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn schemas(recovery: u32) -> SourceDriverSchemas {
        SourceDriverSchemas {
            binding: SchemaSupport::new(1, 1).unwrap(),
            anchor: SchemaSupport::new(1, 1).unwrap(),
            recovery: SchemaSupport::new(1, recovery).unwrap(),
        }
    }

    fn prepared_sources(
        candidate: &ConstructionCandidate,
        operation: SourceOperationKind,
        drivers: &[(&str, &DriverKey, u32)],
    ) -> PreparedSourceSet {
        let prepare_id = candidate.prepare_id();
        PreparedSourceSet {
            prepare_id: prepare_id.into(),
            operation,
            sources: drivers
                .iter()
                .map(|(name, driver, schema)| NamedPreparedSource {
                    name: SourceName::new(*name).unwrap(),
                    prepared: PreparedSourceOperation::new(
                        operation,
                        RecoveryDescriptor::new(
                            (*driver).clone(),
                            *schema,
                            prepare_id,
                            format!("effect:{prepare_id}:{name}"),
                            vec![RecoveryField::new(
                                "target",
                                RecoveryFieldKind::Reference,
                                format!("protected-locator:{name}"),
                            )
                            .unwrap()],
                        )
                        .unwrap(),
                    )
                    .unwrap(),
                })
                .collect(),
        }
    }

    #[test]
    fn prepared_construction_retains_exact_named_source_dependencies() {
        let temp = PrivateTemp(std::env::temp_dir().join(id("aw-construction-test").unwrap()));
        let actor = PrincipalId::new("internal:fixture:construction-owner").unwrap();
        let store = SessionStore::open(&temp.0).unwrap();
        let (session, branch) = store
            .create(
                &actor,
                "origin",
                SessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let origin = AgentKey::new(&session.id, &branch.id);
        let (creator, reference) = {
            let connection = store.connection().unwrap();
            let creator = user_for_principal(&connection, &actor).unwrap().unwrap();
            let reference = agent_owner::canonical_ref(&connection, origin).unwrap();
            // Historical v56 task receipt, with its original stable-user
            // security audit fact. Migration must not rewrite either.
            let legacy = serde_json::json!({
                "request_id":"host:legacy",
                "fingerprint":"1".repeat(64),
                "state":"applied",
                "parent_branch_id":branch.id,
                "candidate_session_id":session.id,
                "candidate_root_id":id("n").unwrap(),
                "candidate_branch_id":branch.id,
                "queue_request_id":"hq_legacy",
                "payload_ref":"sec_legacy",
                "payload_sha256":"2".repeat(64),
                "result_ref":reference,
                "note":null,
            });
            crate::audit(
                &connection,
                &session.id,
                &actor,
                "agent.create.preparation",
                legacy,
            )
            .unwrap();
            (creator, reference)
        };
        drop(store);
        {
            let connection = Connection::open(temp.0.join("sessions.sqlite3")).unwrap();
            connection
                .execute_batch(
                    "DROP INDEX construction_legacy_actor;
                 DROP INDEX construction_source_prepare_id;
                 DROP INDEX construction_receipt_replay;
                 UPDATE store_meta SET value='56' WHERE key='schema_epoch';
                 UPDATE schema_meta SET schema_epoch=56,activation_epoch=56 WHERE singleton=1;
                 UPDATE migration_lease SET current_epoch=56 WHERE singleton=1;
                 PRAGMA user_version=56;",
                )
                .unwrap();
        }
        let git = DriverKey::builtin("fixture-git").unwrap();
        let data = DriverKey::extension("fixture", "data").unwrap();
        let registry = SourceDriverRegistry::new();
        registry.register(git.clone(), schemas(3), ()).unwrap();
        registry.register(data.clone(), schemas(1), ()).unwrap();
        let limit = SourceSetLimit::new(2).unwrap();
        let store = SessionStore::open_with_sources(&temp.0, registry, limit).unwrap();
        {
            let connection = store.connection().unwrap();
            assert_eq!(
                connection
                    .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                    .unwrap(),
                57
            );
            let scope = ConstructionOperation::AgentCreate.scope(&creator, origin);
            let legacy = lookup_in(
                &connection,
                ConstructionOperation::AgentCreate,
                &scope,
                "host:legacy",
            )
            .unwrap()
            .unwrap();
            legacy.require_actor(&connection, &actor).unwrap();
            assert_eq!(legacy.state, PreparationState::Applied);
            assert_eq!(legacy.result_ref.as_deref(), Some(reference.as_str()));
            assert_eq!(legacy.initial_task().unwrap().payload_ref, "sec_legacy");
            assert!(legacy.sources.is_none());
        }

        let make = |operation, request_id: &str, candidate, sources| ConstructionPreparation {
            operation,
            scope: operation.scope(&creator, origin),
            creator: creator.clone(),
            parent_session_id: session.id.clone(),
            parent_branch_id: branch.id.clone(),
            request_id: request_id.into(),
            fingerprint: "3".repeat(64),
            state: PreparationState::Prepared,
            candidate,
            task: None,
            sources: Some(sources),
            capture_boundary: None,
            result_ref: None,
            note: None,
        };
        let candidate = ConstructionCandidate::Checkpoint {
            checkpoint: id("cp").unwrap(),
        };
        let sources = prepared_sources(
            &candidate,
            SourceOperationKind::Capture,
            &[("code", &git, 2), ("data", &data, 1)],
        );
        let capture = make(
            ConstructionOperation::CheckpointCreate,
            "host:shared",
            candidate,
            sources,
        );
        let claim = |store: &SessionStore, receipt: &ConstructionPreparation| {
            let mut connection = store.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            claim_in(
                &tx,
                &actor,
                &store.inner.source_drivers,
                store.inner.source_limit,
                receipt,
            )?;
            db(tx.commit())
        };
        let mut invalid = capture.clone();
        let sources = invalid.sources.as_mut().unwrap();
        sources.sources[1].name = sources.sources[0].name.clone();
        assert!(claim(&store, &invalid).is_err());
        invalid = capture.clone();
        invalid.sources.as_mut().unwrap().sources[1]
            .prepared
            .descriptor
            .prepare_id = "another-operation".into();
        assert!(claim(&store, &invalid).is_err());
        invalid = capture.clone();
        invalid.sources.as_mut().unwrap().sources[1]
            .prepared
            .descriptor
            .recovery_schema = 2;
        assert!(matches!(claim(&store, &invalid), Err(Error::Denied(_))));
        claim(&store, &capture).unwrap();
        assert!(claim(&store, &capture).is_err());
        let mut reused = capture.clone();
        reused.request_id = "host:other".into();
        assert!(claim(&store, &reused).is_err());
        assert!(store
            .checkpoint_create_history_only(&actor, &reference, None, &capture.request_id)
            .is_err());
        assert!(store
            .checkpoint_create_history_only(&actor, "/unavailable", None, &capture.request_id)
            .is_err());
        let candidate = ConstructionCandidate::Agent {
            session: id("s").unwrap(),
            root: id("n").unwrap(),
            branch: id("b").unwrap(),
            sandbox: Some(format!(
                "{}/s:{}",
                reference.rsplit_once("/a:").unwrap().0,
                id("sbx").unwrap()
            )),
        };
        let sources = prepared_sources(
            &candidate,
            SourceOperationKind::Restore,
            &[("code", &git, 3)],
        );
        let restore = make(
            ConstructionOperation::CheckpointRestore,
            "host:shared",
            candidate,
            sources,
        );
        claim(&store, &restore).unwrap();

        let unknown = {
            let mut connection = store.connection().unwrap();
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            assert_eq!(
                lookup_in(&tx, capture.operation, &capture.scope, &capture.request_id).unwrap(),
                Some(capture.clone())
            );
            assert!(transition_in(
                &tx,
                &actor,
                &capture,
                PreparationState::Applied,
                Some("cp_false".into()),
                None
            )
            .is_err());
            assert!(transition_in(
                &tx,
                &actor,
                &capture,
                PreparationState::Rejected,
                None,
                None
            )
            .is_err());
            let next = transition_in(
                &tx,
                &actor,
                &capture,
                PreparationState::Unknown,
                None,
                Some("lost response".into()),
            )
            .unwrap();
            assert_eq!(next.sources, capture.sources);
            assert!(
                transition_in(&tx, &actor, &capture, PreparationState::Unknown, None, None)
                    .is_err()
            );
            tx.commit().unwrap();
            next
        };
        let check = |store: &SessionStore, expected: SourceCompatibility| {
            let connection = store.connection().unwrap();
            let reader = store.inner.source_drivers.reader().unwrap();
            let mut found = Vec::new();
            visit_unresolved_sources(&connection, &reader, |receipt, source, status| {
                assert_eq!(status, expected);
                assert!(receipt == &unknown || receipt == &restore);
                found.push((receipt.operation, source.clone()));
                Ok(())
            })
            .unwrap();
            assert_eq!(found.len(), 3);
            assert_eq!(
                lookup_in(
                    &connection,
                    unknown.operation,
                    &unknown.scope,
                    &unknown.request_id
                )
                .unwrap(),
                Some(unknown.clone())
            );
            assert_eq!(
                lookup_in(
                    &connection,
                    restore.operation,
                    &restore.scope,
                    &restore.request_id
                )
                .unwrap(),
                Some(restore.clone())
            );
            let count: i64 = connection
                .query_row("SELECT count(*) FROM checkpoints", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 0, "unpublished candidates are not public resources");
            let count: i64 = connection
                .query_row("SELECT count(*) FROM agent_state_current", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 1);
            let leaked: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM security_audit_events WHERE action='construction.preparation'
                 AND metadata_json LIKE '%protected-locator%')", [], |r| r.get(0)).unwrap();
            assert!(!leaked);
        };
        check(&store, SourceCompatibility::Compatible);
        drop(store);

        // Independent recovery schema pins survive missing/incompatible restart.
        // No driver or source effect is invoked, no receipt state is rewritten.
        let incompatible = SourceDriverRegistry::new();
        incompatible.register(git.clone(), schemas(1), ()).unwrap();
        let store = SessionStore::open_with_sources(&temp.0, incompatible, limit).unwrap();
        check(&store, SourceCompatibility::RecoveryRequired);
        assert!(
            claim(&store, &capture).is_err(),
            "an unavailable driver does not reopen a claim"
        );
        let backup = temp.0.join("receipt-backup.sqlite3");
        store.backup(&backup).unwrap();
        drop(store);

        let reinstalled = SourceDriverRegistry::new();
        reinstalled.register(data.clone(), schemas(1), ()).unwrap();
        reinstalled.register(git.clone(), schemas(3), ()).unwrap();
        let store =
            SessionStore::open_with_sources(&temp.0, reinstalled, SourceSetLimit::new(1).unwrap())
                .unwrap();
        check(&store, SourceCompatibility::Compatible);
        let candidate = ConstructionCandidate::Checkpoint {
            checkpoint: id("cp").unwrap(),
        };
        let sources = prepared_sources(
            &candidate,
            SourceOperationKind::Capture,
            &[("code", &git, 2), ("data", &data, 1)],
        );
        let over_limit = make(
            ConstructionOperation::CheckpointCreate,
            "host:over-limit",
            candidate,
            sources,
        );
        assert!(matches!(claim(&store, &over_limit), Err(Error::Limit(_))));
        store.restore_backup(&backup).unwrap();
        check(&store, SourceCompatibility::Compatible);
    }
}
