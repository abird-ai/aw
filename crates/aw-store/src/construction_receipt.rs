//! Existing immutable construction receipts, shared by task and source custody.
//! Indexed replay lookup; no separate operation ledger or public resource.
use crate::{
    actor_queue::{self, PreparedQueueCustody},
    agent_owner::{self, AgentKey, AgentPermission},
    append_audit, db, label,
    resources::SessionCandidateIds,
    source_compatibility::{compatibility, SourceCompatibility},
    user_for_principal, workspace_writer, ActorQueueRequest, AuditOrigin, Error, QueueTarget,
    Result, SessionStore, SourceMutationBarrier, SourceMutationOutcome,
};
use aw_agent_core::{
    ExternalEvidence, NamedPreparedSource, PreparedSourceSet, PrincipalId, RecoveryInspection,
    SourceBinding, SourceDriverRegistry, SourceDriverRegistryReader, SourceOperationKind,
    SourcePublicationFence, SourcePublicationProof, SourceSet, SourceSetLimit,
    HARD_MAX_SOURCES_PER_SANDBOX, MAX_SOURCE_ANCHOR_FIELDS_BYTES,
};
use rusqlite::{params, Connection, Row, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
    // Old task-only receipts did not record the trusted site. Keep their
    // exact encoding; new preparations pin it for restart publication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) execution_site: Option<String>,
}

impl InitialTask {
    pub(crate) fn mint(
        request_id: &str,
        session: &str,
        payload: &str,
        execution_site: &str,
    ) -> Result<Self> {
        crate::agent_host::validate_execution_site(execution_site)?;
        // Preserve the original candidate-local initial input namespace.
        let mut hash = Sha256::new();
        hash.update(b"aw-host-agent-create-initial-task-v1\0");
        for part in [request_id.as_bytes(), session.as_bytes()] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part);
        }
        let queue_request_id = format!("hq_{:x}", hash.finalize());
        let custody =
            actor_queue::mint_prepared_queue_custody(session, &queue_request_id, payload)?;
        Ok(Self {
            queue_request_id,
            payload_ref: custody.payload_ref,
            payload_sha256: custody.payload_sha256,
            execution_site: Some(execution_site.to_owned()),
        })
    }

    pub(crate) fn custody(&self) -> PreparedQueueCustody {
        PreparedQueueCustody {
            payload_ref: self.payload_ref.clone(),
            payload_sha256: self.payload_sha256.clone(),
        }
    }

    pub(crate) fn publish_in(
        &self,
        connection: &Connection,
        owner: &PrincipalId,
        session: &str,
        branch: &str,
    ) -> Result<()> {
        let site = self
            .execution_site
            .as_ref()
            .ok_or(Error::Storage("initial task has no pinned execution site"))?;
        let writer = workspace_writer::require_workspace_writer_for_session(connection, session)?;
        actor_queue::publish_prepared_queue_in(
            connection,
            owner,
            ActorQueueRequest {
                request_id: self.queue_request_id.clone(),
                session_id: session.to_owned(),
                branch_id: branch.to_owned(),
                recipient_site: site.clone(),
                sender: "host:create".into(),
                message_id: 1,
                writer_epoch: writer.writer_epoch,
                target: QueueTarget::Auto,
                priority: 0,
            },
            &self.custody(),
        )?;
        Ok(())
    }
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

    /// Candidate identities are private until the resource transaction.
    /// Never clean up a candidate that already became addressable.
    pub(crate) fn require_absent_in(&self, connection: &Connection) -> Result<()> {
        let exists: bool = match self {
            Self::Agent {
                session,
                root,
                branch,
                sandbox,
            } => db(connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1)
                     OR EXISTS(SELECT 1 FROM nodes WHERE id=?2)
                     OR EXISTS(SELECT 1 FROM branches WHERE id=?3)
                     OR EXISTS(SELECT 1 FROM sandboxes_current WHERE sandbox_id=?4)",
                params![session, root, branch, sandbox],
                |row| row.get(0),
            ))?,
            Self::Checkpoint { checkpoint } => db(connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM checkpoints WHERE checkpoint_id=?1)",
                [checkpoint],
                |row| row.get(0),
            ))?,
            Self::Sandbox { sandbox } => db(connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM sandboxes_current WHERE sandbox_id=?1)",
                [sandbox],
                |row| row.get(0),
            ))?,
        };
        if exists {
            return Err(Error::Conflict("construction candidate already exists"));
        }
        Ok(())
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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) parent_session_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) restore_boundary: Option<crate::checkpoint_owner::restore::RestoreBoundary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) sandbox_boundary: Option<crate::sandbox_owner::construction::SandboxCreateBoundary>,
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
        let expected_scope = if self.operation == ConstructionOperation::SandboxCreate {
            if !self.parent_session_id.is_empty() || !self.parent_branch_id.is_empty() {
                return Err(Error::Storage(
                    "Sandbox construction must not borrow an Agent origin",
                ));
            }
            let boundary = self.sandbox_boundary.as_ref().ok_or(Error::Storage(
                "Sandbox construction has no Project boundary",
            ))?;
            boundary.validate(&self.candidate, self.sources.as_ref())?;
            self.creator.clone()
        } else {
            for id in [&self.parent_session_id, &self.parent_branch_id] {
                label(id, 128)?;
            }
            if self.sandbox_boundary.is_some() {
                return Err(Error::Storage(
                    "Project construction boundary belongs to Sandbox creation",
                ));
            }
            self.operation.scope(&self.creator, self.origin())
        };
        crate::stable_user_principal(&self.creator)?;
        label(&self.request_id, 256)?;
        if self.scope != expected_scope
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
            if let Some(site) = &task.execution_site {
                crate::agent_host::validate_execution_site(site)?;
            }
            if self.operation == ConstructionOperation::CheckpointRestore
                && (task.execution_site.is_none()
                    || (self.sources.is_none()
                        && !self
                            .restore_boundary
                            .as_ref()
                            .is_some_and(|boundary| boundary.has_baseline_selection()))
                    || task.payload_ref.len() != 68
                    || !task.payload_ref.starts_with("sec_")
                    || !task.payload_ref[4..].bytes().all(|b| b.is_ascii_hexdigit()))
            {
                return Err(Error::Storage(
                    "restore task requires exact prepared custody",
                ));
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
        if let Some(boundary) = &self.restore_boundary {
            if self.operation != ConstructionOperation::CheckpointRestore {
                return Err(Error::Storage(
                    "restore boundary belongs to checkpoint restoration",
                ));
            }
            boundary.validate(&self.candidate, self.sources.as_ref())?;
            if boundary.has_baseline_selection() && self.task.is_none() {
                return Err(Error::Storage("prepared baseline lost task custody"));
            }
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
            let kind = match &self.sandbox_boundary {
                Some(boundary) => boundary.operation(),
                None if self.operation == ConstructionOperation::CheckpointCreate => {
                    SourceOperationKind::Capture
                }
                None => SourceOperationKind::Restore,
            };
            if sources.operation != kind || sources.prepare_id != self.candidate.prepare_id() {
                return Err(Error::Conflict(
                    "prepared source set does not belong to construction candidate",
                ));
            }
        } else if !((self.operation == ConstructionOperation::AgentCreate && self.task.is_some())
            || (self.operation == ConstructionOperation::CheckpointRestore
                && self.restore_boundary.is_some()
                && ((self.state == PreparationState::Applied && self.task.is_none())
                    || (self.task.is_some()
                        && self
                            .restore_boundary
                            .as_ref()
                            .is_some_and(|boundary| boundary.has_baseline_selection())))))
        {
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
    let session: Option<String> = db(row.get(0))?;
    let project: Option<String> = db(row.get(5))?;
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
        let session = session
            .as_ref()
            .ok_or(Error::Storage("legacy receipt has no Session origin"))?;
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
            .scope(&creator, AgentKey::new(session, &legacy.parent_branch_id));
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
                execution_site: None,
            }),
            sources: None,
            capture_boundary: None,
            restore_boundary: None,
            sandbox_boundary: None,
            result_ref: legacy.result_ref,
            note: legacy.note,
        }
    } else {
        return Err(Error::Storage("invalid construction receipt action"));
    };
    receipt.validate()?;
    let matches_origin = match &receipt.sandbox_boundary {
        Some(boundary) => {
            session.is_none() && project.as_deref() == Some(boundary.project_ref.as_str())
        }
        None => project.is_none() && session.as_deref() == Some(receipt.parent_session_id.as_str()),
    };
    if !matches_origin {
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
        "SELECT session_id,action,actor,data_json,created_at,project_ref FROM audit
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
    let origin = match &receipt.sandbox_boundary {
        Some(boundary) => AuditOrigin::Project(&boundary.project_ref),
        None => AuditOrigin::Session(&receipt.parent_session_id),
    };
    append_audit(
        tx,
        origin,
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
    // Leave room for terminal result/diagnostic metadata. Never persist a
    // new claim whose exact receipt cannot be decoded after restart.
    if crate::json(receipt)?.len() > MAX_RECEIPT_BYTES - 16384 {
        return Err(Error::Limit("construction receipt exceeds encoded bound"));
    }
    if let Some(boundary) = &receipt.restore_boundary {
        let set = boundary.materialization_sources_in(tx, receipt)?;
        let reader = registry.reader()?;
        for anchor in &set.anchors {
            reader.resolve(&anchor.pin()?)?;
        }
    }
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
    if let Some(boundary) = &receipt.sandbox_boundary {
        boundary.authorize(tx, actor, true)?;
        boundary.require_compatible(&registry.reader()?)?;
    } else {
        agent_owner::authorize_permission(tx, actor, receipt.origin(), AgentPermission::View)?;
    }
    append_in(tx, actor, receipt)
}

pub(crate) fn claim_sources_in(
    store: &SessionStore,
    tx: &Transaction<'_>,
    actor: &PrincipalId,
    receipt: &ConstructionPreparation,
    set: &crate::SourceMutationSet,
) -> Result<SourceMutationBarrier> {
    let barrier =
        store.acquire_source_mutation_barrier_in(tx, set, receipt.candidate.prepare_id())?;
    claim_in(
        tx,
        actor,
        &store.inner.source_drivers,
        store.inner.source_limit,
        receipt,
    )?;
    Ok(barrier)
}

pub(crate) fn require_current_sources_in(
    tx: &Transaction<'_>,
    actor: &PrincipalId,
    receipt: &ConstructionPreparation,
    barrier: &SourceMutationBarrier,
) -> Result<()> {
    current_unresolved_in(tx, actor, receipt)?;
    barrier.require_preparation_in(tx, receipt.candidate.prepare_id())
}

pub(crate) fn mark_sources_unknown(
    store: &SessionStore,
    actor: &PrincipalId,
    receipt: &ConstructionPreparation,
    barrier: &SourceMutationBarrier,
) -> Result<()> {
    let mut connection = store.connection()?;
    let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
    barrier.settle_in(&tx, SourceMutationOutcome::Unknown)?;
    transition_in(
        &tx,
        actor,
        receipt,
        PreparationState::Unknown,
        None,
        Some("source effect outcome requires inspection".into()),
    )?;
    db(tx.commit())
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

/// Exact initial task-only restore claim. The existing replay index
/// finds the original Prepared fact without a new operation owner.
pub(crate) fn require_task_baseline_claim_in(
    connection: &Connection,
    actor: &PrincipalId,
    receipt: &ConstructionPreparation,
) -> Result<()> {
    receipt.require_actor(connection, actor)?;
    if receipt.operation != ConstructionOperation::CheckpointRestore
        || receipt.sources.is_some()
        || receipt.task.is_none()
        || !receipt
            .restore_boundary
            .as_ref()
            .is_some_and(|boundary| boundary.has_baseline_selection())
    {
        return Err(Error::Conflict("not a task-only baseline receipt"));
    }
    let mut query = db(connection.prepare_cached(
        "SELECT session_id,action,actor,data_json,created_at,project_ref FROM audit
         WHERE action IN ('agent.create.preparation','construction.preparation')
           AND COALESCE(json_extract(data_json,'$.scope'),session_id||'/'||json_extract(data_json,'$.parent_branch_id'))=?1
           AND COALESCE(json_extract(data_json,'$.operation'),'agent_create')='checkpoint_restore'
           AND json_extract(data_json,'$.request_id')=?2
           AND json_extract(data_json,'$.state')='prepared'
         LIMIT 2",
    ))?;
    let mut rows = db(query.query(params![receipt.scope, receipt.request_id]))?;
    let claim = decode_receipt(
        connection,
        db(rows.next())?.ok_or(Error::Storage(
            "original task-only baseline claim unavailable",
        ))?,
    )?;
    if db(rows.next())?.is_some() || current_claim_in(connection, &claim)? != *receipt {
        return Err(Error::Storage("original task-only baseline claim changed"));
    }
    Ok(())
}

/// Keep the immutable history root alive while a task-only restore is
/// unresolved, even though there are no captured-source dependencies.
pub(crate) fn require_no_unresolved_task_restore_in(
    connection: &Connection,
    checkpoint: &str,
) -> Result<()> {
    let mut query = db(connection.prepare_cached(
        "SELECT session_id,action,actor,data_json,created_at,project_ref FROM audit
         WHERE action='construction.preparation'
           AND json_extract(data_json,'$.operation')='checkpoint_restore'
           AND json_extract(data_json,'$.state')='prepared'
           AND json_type(data_json,'$.task')='object'
           AND json_type(data_json,'$.restore_boundary.baseline_digest')='text'
           AND json_extract(data_json,'$.restore_boundary.checkpoint')=?1",
    ))?;
    let mut rows = db(query.query([checkpoint]))?;
    while let Some(row) = db(rows.next())? {
        let claim = decode_receipt(connection, row)?;
        if current_claim_in(connection, &claim)?.state.unresolved() {
            return Err(Error::Conflict(
                "checkpoint has an unresolved task-only restore",
            ));
        }
    }
    Ok(())
}

/// Validate the original indexed Prepared fact, not just the latest
/// receipt. Cleanup never adopts a replacement descriptor.
pub(crate) fn is_reconciled_in(
    connection: &Connection,
    receipt: &ConstructionPreparation,
) -> Result<bool> {
    if receipt.state != PreparationState::Rejected {
        return Ok(false);
    }
    require_original_source_claim_in(connection, receipt)?;
    receipt.candidate.require_absent_in(connection)?;
    Ok(true)
}

fn require_original_source_claim_in(
    connection: &Connection,
    receipt: &ConstructionPreparation,
) -> Result<()> {
    let mut query = db(connection.prepare_cached(
        "SELECT session_id,action,actor,data_json,created_at,project_ref FROM audit
         WHERE action='construction.preparation'
           AND json_extract(data_json,'$.state')='prepared'
           AND json_type(data_json,'$.sources')='object'
           AND json_extract(data_json,'$.sources.prepare_id')=?1",
    ))?;
    let mut rows = db(query.query([receipt.candidate.prepare_id()]))?;
    let claim = decode_receipt(
        connection,
        db(rows.next())?.ok_or(Error::Storage("original source claim unavailable"))?,
    )?;
    if db(rows.next())?.is_some() || current_claim_in(connection, &claim)? != *receipt {
        return Err(Error::Storage("original source claim changed"));
    }
    Ok(())
}

pub(crate) fn require_unpublished_sources_in(
    tx: &Transaction<'_>,
    actor: &PrincipalId,
    receipt: &ConstructionPreparation,
    barrier: &SourceMutationBarrier,
) -> Result<()> {
    require_current_sources_in(tx, actor, receipt, barrier)?;
    require_original_source_claim_in(tx, receipt)?;
    receipt.candidate.require_absent_in(tx)?;
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
    Ok(())
}

/// Trusted reconciliation of the original private candidate. The closure
/// must independently authorize and account for every external effect;
/// success proves all operation-owned resources released or absent, with
/// no remaining live dependencies. Retain its fence through final commit.
/// Rejection is a terminal replay fact, never permission to retry prepare.
pub(crate) fn reconcile_unpublished_sources<F>(
    store: &SessionStore,
    actor: &PrincipalId,
    receipt: &ConstructionPreparation,
    barrier: &SourceMutationBarrier,
    reconcile: impl FnOnce() -> Result<F>,
) -> Result<()> {
    let result = (|| {
        // Check absence/ownership before any trusted cleanup is invoked.
        {
            let mut connection = store.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            require_unpublished_sources_in(&tx, actor, receipt, barrier)?;
            db(tx.commit())?;
        }
        let fence = reconcile()?;
        // The source barrier also excludes recovery of this operation's
        // private task. Never discard an Applied candidate's queue input.
        if let Some(task) = &receipt.task {
            store.discard_prepared_queue_custody(&task.custody())?;
        }
        let finish = (|| {
            let mut connection = store.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            require_unpublished_sources_in(&tx, actor, receipt, barrier)?;
            let rejected = ConstructionPreparation {
                state: PreparationState::Rejected,
                result_ref: None,
                note: Some("unpublished source candidate reconciled".into()),
                ..receipt.clone()
            };
            rejected.validate()?;
            append_in(&tx, actor, &rejected)?;
            barrier.settle_in(&tx, SourceMutationOutcome::Settled)?;
            db(tx.commit())
        })();
        drop(fence);
        finish
    })();
    if result.is_err() {
        mark_sources_unknown(store, actor, receipt, barrier)?;
    }
    result
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

fn require_published_candidate_in(
    tx: &Transaction<'_>,
    receipt: &ConstructionPreparation,
    result_ref: &str,
) -> Result<()> {
    // The resource constructor must have published the operation-owned
    // candidate, not an existing or substituted resource.
    let candidate_ref = match &receipt.candidate {
        ConstructionCandidate::Checkpoint { checkpoint } => {
            let published: bool = db(tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM checkpoints WHERE checkpoint_id=?1
                    AND created_by=?2 AND request_id=?3 AND request_fingerprint=?4)",
                params![
                    checkpoint,
                    receipt.creator,
                    receipt.request_id,
                    receipt.fingerprint
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
            agent_owner::canonical_ref(tx, AgentKey::new(session, branch))?
        }
        ConstructionCandidate::Sandbox { sandbox } => {
            let parent = receipt
                .sandbox_boundary
                .as_ref()
                .and_then(|boundary| boundary.parent_ref());
            let published: bool = db(tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM sandboxes_current WHERE sandbox_id=?1 AND lifecycle='open'
                 AND parent_sandbox_id IS ?2)",
                params![sandbox, parent], |row| row.get(0),
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
    Ok(())
}

/// Publish the task-only candidate's factual result in the same
/// transaction as its original Pending input. The Store connection owner
/// serializes its local vault effect and recovery; no source lease.
pub(crate) fn publish_task_baseline_in(
    tx: &Transaction<'_>,
    actor: &PrincipalId,
    receipt: &ConstructionPreparation,
    result_ref: String,
) -> Result<()> {
    require_task_baseline_claim_in(tx, actor, receipt)?;
    require_published_candidate_in(tx, receipt, &result_ref)?;
    transition_in(
        tx,
        actor,
        receipt,
        PreparationState::Applied,
        Some(result_ref),
        None,
    )?;
    Ok(())
}

/// A metadata-only restore has no external effects to prepare or recover.
/// Publish its original replay identity in the resource transaction.
pub(crate) fn record_atomic_restore_in(
    tx: &Transaction<'_>,
    actor: &PrincipalId,
    receipt: &ConstructionPreparation,
) -> Result<()> {
    if receipt.operation != ConstructionOperation::CheckpointRestore
        || receipt.state != PreparationState::Applied
        || receipt.sources.is_some()
        || receipt.task.is_some()
    {
        return Err(Error::Conflict(
            "atomic restore receipt requires no external custody",
        ));
    }
    receipt.require_actor(tx, actor)?;
    receipt.validate()?;
    if lookup_in(tx, receipt.operation, &receipt.scope, &receipt.request_id)?.is_some() {
        return Err(Error::Conflict("construction request already claimed"));
    }
    require_published_candidate_in(
        tx,
        receipt,
        receipt
            .result_ref
            .as_deref()
            .ok_or(Error::Storage("atomic restore result missing"))?,
    )?;
    append_in(tx, actor, receipt)
}

pub(crate) fn require_matching_sources(
    set: &SourceSet,
    prepared: &PreparedSourceSet,
) -> Result<()> {
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
            "restore descriptors do not match the complete SourceSet",
        ));
    }
    Ok(())
}

/// Shared restore inspection result validation, independent of which
/// resource constructor owns the original Prepared materializations.
pub(crate) fn validate_inspected_bindings(
    registry: &SourceDriverRegistryReader,
    anchors: &SourceSet,
    results: Vec<RecoveryInspection<SourceBinding>>,
) -> Result<(Vec<SourceBinding>, Vec<ExternalEvidence>)> {
    validate_materialized_bindings(
        registry,
        anchors.anchors.len(),
        results,
        |index, binding| {
            let anchor = &anchors.anchors[index];
            if binding.name != anchor.name
                || binding.driver != anchor.driver
                || binding.source_ref != anchor.source_ref
                || binding.access != anchor.access
                || binding.checkpoint_policy != anchor.checkpoint_policy
            {
                return Err(Error::Conflict(
                    "restored binding differs from its source anchor",
                ));
            }
            Ok(())
        },
    )
}

pub(crate) fn validate_materialized_bindings(
    registry: &SourceDriverRegistryReader,
    count: usize,
    results: Vec<RecoveryInspection<SourceBinding>>,
    mut require_binding: impl FnMut(usize, &SourceBinding) -> Result<()>,
) -> Result<(Vec<SourceBinding>, Vec<ExternalEvidence>)> {
    if results.len() != count {
        return Err(Error::Conflict(
            "materialization inspection requires the complete source set",
        ));
    }
    let mut bindings = Vec::with_capacity(count);
    let mut evidence = Vec::with_capacity(count);
    for (index, result) in results.into_iter().enumerate() {
        let RecoveryInspection::Completed {
            result: binding,
            evidence: proof,
        } = result
        else {
            return Err(Error::Conflict(
                "source materialization is unresolved; inspect before retry",
            ));
        };
        binding.validate()?;
        proof.validate()?;
        require_binding(index, &binding)?;
        registry.resolve(&binding.pin()?)?;
        bindings.push(binding);
        evidence.push(proof);
    }
    Ok((bindings, evidence))
}

/// The common source-bearing publication boundary. Resource owners supply
/// their exact in-transaction authority revalidation and publication using
/// their existing lower constructors. All external guards are owned here
/// until SQLite has finished committing (or rolling back).
pub(crate) fn publish_sources<F: SourcePublicationFence, T>(
    store: &SessionStore,
    actor: &PrincipalId,
    expected: &ConstructionPreparation,
    barrier: &SourceMutationBarrier,
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
    require_published_candidate_in(&tx, &current, &result_ref)?;
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

/// Re-read the same immutable initial source claim through its indexed
/// current receipt, not a second provenance copy.
fn current_claim_in(
    connection: &Connection,
    claim: &ConstructionPreparation,
) -> Result<ConstructionPreparation> {
    let receipt = lookup_in(connection, claim.operation, &claim.scope, &claim.request_id)?.ok_or(
        Error::Storage("prepared source claim lost its current receipt"),
    )?;
    if claim.creator != receipt.creator
        || claim.parent_session_id != receipt.parent_session_id
        || claim.parent_branch_id != receipt.parent_branch_id
        || claim.fingerprint != receipt.fingerprint
        || claim.candidate != receipt.candidate
        || claim.task != receipt.task
        || claim.sources != receipt.sources
        || claim.capture_boundary != receipt.capture_boundary
        || claim.restore_boundary != receipt.restore_boundary
        || claim.sandbox_boundary != receipt.sandbox_boundary
    {
        return Err(Error::Storage(
            "prepared source receipt changed its recovery identity",
        ));
    }
    Ok(receipt)
}

/// Exact original materialization provenance of a published Sandbox.
/// Both Agent restore and standalone Project construction retain the
/// original Prepared identity; sharing Agents cannot substitute it.
pub(crate) fn restored_sources_for_sandbox_in(
    connection: &Connection,
    sandbox: &str,
) -> Result<PreparedSourceSet> {
    original_materializations_in(connection, sandbox)?.ok_or(Error::Denied(
        "original Sandbox restore preparation unavailable",
    ))
}

/// The original Applied materialization descriptors remain authoritative
/// after individual sources are retired; never reconstruct from a path.
pub(crate) fn original_materializations_in(
    connection: &Connection,
    sandbox: &str,
) -> Result<Option<PreparedSourceSet>> {
    let mut query = db(connection.prepare_cached(
        "SELECT session_id,action,actor,data_json,created_at,project_ref FROM audit
         WHERE action='construction.preparation'
           AND json_extract(data_json,'$.state')='prepared'
           AND json_type(data_json,'$.sources')='object'
           AND json_extract(data_json,'$.candidate.sandbox')=?1
           AND (
               json_extract(data_json,'$.operation')='sandbox_create'
               OR (json_extract(data_json,'$.operation')='checkpoint_restore'
                   AND json_extract(data_json,'$.sources.prepare_id') IN (
                       SELECT session_id FROM agent_state_current WHERE sandbox_id=?1
                   ))
           )
         LIMIT 2",
    ))?;
    let mut rows = db(query.query([sandbox]))?;
    let Some(row) = db(rows.next())? else {
        return Ok(None);
    };
    let claim = decode_receipt(connection, row)?;
    if db(rows.next())?.is_some() {
        return Err(Error::Storage(
            "ambiguous original Sandbox restore preparation",
        ));
    }
    let receipt = current_claim_in(connection, &claim)?;
    if let ConstructionCandidate::Sandbox { sandbox: original } = &receipt.candidate {
        if receipt.operation != ConstructionOperation::SandboxCreate
            || receipt.state != PreparationState::Applied
            || receipt.sandbox_boundary.is_none()
            || original != sandbox
            || receipt.result_ref.as_deref() != Some(sandbox)
        {
            return Err(Error::Denied(
                "standalone Sandbox construction has not been applied",
            ));
        }
        return receipt
            .sources
            .map(Some)
            .ok_or(Error::Storage("original Sandbox source custody missing"));
    }
    let ConstructionCandidate::Agent {
        session,
        branch,
        sandbox: Some(original),
        ..
    } = &receipt.candidate
    else {
        return Err(Error::Storage("original restore has no Sandbox candidate"));
    };
    if receipt.state != PreparationState::Applied
        || receipt.operation != ConstructionOperation::CheckpointRestore
        || receipt.restore_boundary.is_none()
        || original != sandbox
    {
        return Err(Error::Denied("Sandbox restore has not been applied"));
    }
    let bound: bool = db(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_state_current
         WHERE session_id=?1 AND branch_id=?2 AND sandbox_id=?3)",
        params![session, branch, sandbox],
        |row| row.get(0),
    ))?;
    if !bound
        || receipt.result_ref.as_deref()
            != Some(
                agent_owner::canonical_ref(connection, AgentKey::new(session, branch))?.as_str(),
            )
    {
        return Err(Error::Storage(
            "original restore publication identity changed",
        ));
    }
    receipt
        .sources
        .map(Some)
        .ok_or(Error::Storage("original restore source custody missing"))
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
        "SELECT session_id,action,actor,data_json,created_at,project_ref FROM audit
         WHERE action='construction.preparation'
           AND json_extract(data_json,'$.state')='prepared'
           AND json_type(data_json,'$.sources')='object'",
    ))?;
    let mut rows = db(query.query([]))?;
    while let Some(row) = db(rows.next())? {
        let claim = decode_receipt(connection, row)?;
        let receipt = current_claim_in(connection, &claim)?;
        if !receipt.state.unresolved() {
            continue;
        }
        let sources = receipt
            .sources
            .as_ref()
            .ok_or(Error::Storage("source custody missing"))?;
        let restore_set = receipt
            .restore_boundary
            .as_ref()
            .map(|boundary| boundary.materialization_sources_in(connection, &receipt))
            .transpose()?;
        for (index, source) in sources.sources.iter().enumerate() {
            let mut status = compatibility(registry, &source.prepared.descriptor.pin()?)?;
            if let Some(boundary) = &receipt.capture_boundary {
                if compatibility(registry, &boundary.bindings[index].binding.pin()?)?
                    == SourceCompatibility::RecoveryRequired
                {
                    status = SourceCompatibility::RecoveryRequired;
                }
            }
            let input_pin = if let Some(set) = &restore_set {
                Some(set.anchors[index].pin()?)
            } else if let Some(boundary) = &receipt.sandbox_boundary {
                Some(boundary.source_pin(index)?)
            } else {
                None
            };
            if let Some(pin) = input_pin {
                if compatibility(registry, &pin)? == SourceCompatibility::RecoveryRequired {
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
        let original_legacy_fact: (u64, String, String, i64) = {
            let connection = store.connection().unwrap();
            db(connection.query_row(
                "SELECT seq,actor,data_json,created_at FROM audit
                 WHERE action='agent.create.preparation' AND session_id=?1",
                [&session.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            ))
            .unwrap()
        };
        drop(store);
        {
            let connection = Connection::open(temp.0.join("sessions.sqlite3")).unwrap();
            connection
                .execute_batch(
                    "DROP TABLE agent_vault_grants_current;
                 DROP TABLE agent_sandbox_grants_current;
                 DROP INDEX construction_legacy_actor;
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
                crate::LATEST_SCHEMA_VERSION
            );
            let migrated_legacy_fact: (u64, String, String, i64) = db(connection.query_row(
                "SELECT seq,actor,data_json,created_at FROM audit
                 WHERE action='agent.create.preparation' AND session_id=?1 AND project_ref IS NULL",
                [&session.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            ))
            .unwrap();
            assert_eq!(original_legacy_fact, migrated_legacy_fact);
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
            restore_boundary: None,
            sandbox_boundary: None,
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
