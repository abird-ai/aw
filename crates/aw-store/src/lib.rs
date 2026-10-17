#![allow(clippy::type_complexity, clippy::too_many_arguments)]
// SQL row tuples and explicit durable/protocol API signatures intentionally mirror
// reviewed storage/wire contracts. Keep Clippy strict for all other lint classes.

//! SQLite is an optional host crate, never a dependency of aw-agent-core.
//! Owner/service mode remains the default exclusive local owner. Phase 35 also
//! supports an explicit trusted-local shared-lock direct mode whose processes
//! are generation/heartbeat/lease fenced. Immutable nodes store history, branch
//! refs select a path, durable start receipts precede effects.
mod actor_admission;
mod actor_control;
mod actor_custody;
mod actor_notice;
mod actor_peer;
mod actor_queue;
mod archive;
mod auth_session;
mod authority;
mod authority_replica;
#[cfg(target_os = "linux")]
mod cold_activation_consent;
#[cfg(target_os = "linux")]
mod cold_activation_dry_run;
#[cfg(target_os = "linux")]
mod cold_effect_reconciliation;
#[cfg(target_os = "linux")]
mod cold_quarantine_review;
#[cfg(unix)]
mod composite_staging;
pub mod context;
mod coordination;
pub mod durability;
mod extension_secret;
mod identity_continuity;
mod journal;
mod model_agent_pause;
mod model_process;
#[cfg(feature = "multiplayer")]
mod multiplayer;
mod oauth_connector;
mod oidc_login;
#[cfg(feature = "pipelines")]
mod pipelines;
#[cfg(all(feature = "pipelines", feature = "routing"))]
mod planner;
mod provider_credentials;
mod resources;
#[cfg(all(feature = "pipelines", feature = "routing"))]
mod reviews;
#[cfg(feature = "routing")]
mod routing;
#[cfg(feature = "pipelines")]
mod schedules;
mod secret_custody_inventory;
mod secret_store;
mod subagents;
#[cfg(feature = "pipelines")]
mod teams;
mod types;
mod workspace_replica;
mod workspace_writer;
pub use actor_admission::{ActorInputAdmission, ActorInputFact, InputAdmissionState};
pub use actor_control::{
    ActorControlAction, ActorControlAdmission, ActorControlFact, ActorControlState,
};
pub use actor_custody::CustodySweep;
pub use actor_notice::{ActorNoticeFact, ActorNoticeRequest, ActorNoticeState, ActorNoticeView};
pub use actor_peer::{ActorPeerGrant, ActorPeerGrantState, ActorPeerPermission};
pub use actor_queue::{
    model_queue_disposition_id, model_queue_request_id, queued_input_request_id, ActorQueueFact,
    ActorQueueRequest, ActorQueueView, ClaimedQueueInput, ModelQueueDispositionKind,
    ModelQueueDispositionRequest, QueueClaim, QueueState, QueueTarget,
};
pub use auth_session::*;
pub use authority::{
    domain_authority_vote_transcript, DomainAuthorityElectionInfo, DomainAuthorityGrantBundle,
    DomainAuthorityPeerGrantInfo, DomainAuthorityPeerVoteInfo, DomainAuthoritySetInfo,
    DomainAuthorityVoteInfo,
};
pub use authority_replica::{
    domain_authority_snapshot_transcript, DomainAuthorityControlSnapshot,
    DomainAuthorityPromotionInfo, DomainAuthorityReplicaElectionInfo,
    DomainAuthorityReplicaMembership, DomainAuthorityReplicaSnapshotInfo,
    DomainAuthorityReplicaUser,
};
use aw_agent_core::{
    AuxiliaryInput, AuxiliaryJournal, AuxiliaryResult, Conversation, Error, HistoryState,
    InvocationContext, InvocationIdentity, InvocationPurpose, JournalFuture, Outcome, PrincipalId,
    Result, Role,
};
pub use context::{
    ApplyContext, ApplyMode, ContextApplied, ContextJournal, ContextNode, ContextPlan,
    MaterializedContextProjection, MaterializedContextRequest, PlanKind, PrepareContext, Selection,
    SourceSelection, SummaryArtifact, SummaryOperation, TargetHead,
};
pub use coordination::{
    LocalCoordinationLease, LocalLeaseKind, LocalMaintenanceLease, LocalProcessInfo,
};
pub use durability::{
    CommitEvent, CommitEventPage, DurableOperation, RecoveryHint, RetryMode, RetryPolicy,
    SemanticMaterializationPolicy, SemanticMaterializationPolicyResult, SemanticMaterializeResult,
    SemanticMaterializedFact, SemanticSyncApply, SemanticSyncBatch,
};
pub use extension_secret::{SecretSettingScope, SecretSettingState, SecretSettingView};
use fs2::FileExt;
pub use identity_continuity::*;
pub use journal::SessionHandle;
pub use model_agent_pause::{
    model_agent_pause_id, ModelAgentPauseFact, ModelAgentPauseRequest, ModelAgentPauseState,
};
pub use model_process::{
    model_process_intent_id, ModelProcessAction, ModelProcessFact, ModelProcessRequest,
    ModelProcessState,
};
#[cfg(feature = "multiplayer")]
pub use multiplayer::*;
pub use oauth_connector::*;
pub use oidc_login::*;
#[cfg(feature = "pipelines")]
pub use pipelines::*;
#[cfg(all(feature = "pipelines", feature = "routing"))]
pub use planner::*;
pub use provider_credentials::*;
pub use resources::{
    AwNetPeerInfo, DomainFederationInfo, DomainInfo, FederationLeaseInfo, FleetInfo,
    FleetMemberInfo, FleetOperationInfo, FleetOperationMemberInfo, FleetOperationSnapshot,
    MachineInfo, PeerSessionPlacement, RemoteSessionExecutionInfo, RemoteSessionRef,
    SessionExecutionTarget, WorkspaceInfo, WorkspaceKind,
};
#[cfg(all(feature = "pipelines", feature = "routing"))]
pub use reviews::*;
#[cfg(feature = "routing")]
pub use routing::*;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
#[cfg(feature = "pipelines")]
pub use schedules::*;
pub use secret_custody_inventory::ActiveSecretCustodyInventory;
pub use secret_store::*;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex, MutexGuard},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
pub use subagents::*;
#[cfg(feature = "pipelines")]
pub use teams::*;
pub use types::*;
pub use workspace_replica::*;
pub use workspace_writer::{WorkspaceWriterEventInfo, WorkspaceWriterInfo};

pub const SCHEMA: &str = include_str!("../migrations/001_initial.sql");
pub const MAX_EXPORT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_SESSION_NODES: usize = 10_000;
pub const CONTEXT_SCHEMA: &str = include_str!("../migrations/002_context_flow.sql");
pub const DURABLE_RUNTIME_SCHEMA: &str = include_str!("../migrations/003_durable_runtime.sql");
pub const DOMAIN_WORKSPACE_SCHEMA: &str =
    include_str!("../migrations/004_domains_workspaces_fleets.sql");
pub const PEER_REMOTE_SESSION_SCHEMA: &str =
    include_str!("../migrations/005_peer_remote_sessions.sql");
pub const FLEET_OPERATION_SCHEMA: &str = include_str!("../migrations/006_fleet_operations.sql");
pub const FEDERATION_LEASE_SCHEMA: &str = include_str!("../migrations/007_federation_leases.sql");
pub const SEMANTIC_SYNC_SCHEMA: &str = include_str!("../migrations/008_semantic_sync.sql");
pub const REMOTE_SESSION_EXECUTION_SCHEMA: &str =
    include_str!("../migrations/009_remote_session_execution.sql");
pub const SEMANTIC_MATERIALIZATION_SCHEMA: &str =
    include_str!("../migrations/010_semantic_materialization.sql");
pub const DOMAIN_AUTHORITY_SCHEMA: &str = include_str!("../migrations/011_domain_authority.sql");
pub const DOMAIN_AUTHORITY_REPLICA_SCHEMA: &str =
    include_str!("../migrations/012_domain_authority_replica.sql");
pub const WORKSPACE_WRITER_SCHEMA: &str =
    include_str!("../migrations/013_workspace_writer_epochs.sql");
pub const WORKSPACE_REPLICA_SCHEMA: &str =
    include_str!("../migrations/014_workspace_replica_handoff.sql");
pub const IDENTITY_CONTINUITY_SCHEMA: &str =
    include_str!("../migrations/015_identity_continuity.sql");
pub const AUTH_SECRET_STORE_SCHEMA: &str = include_str!("../migrations/016_auth_secret_store.sql");
pub const OIDC_OAUTH_SCHEMA: &str = include_str!("../migrations/017_oidc_oauth.sql");
pub const MATERIALIZED_CONTEXT_SCHEMA: &str =
    include_str!("../migrations/018_materialized_context.sql");
pub const SQLITE_COORDINATION_SCHEMA: &str =
    include_str!("../migrations/019_sqlite_coordination.sql");
pub const TRUSTED_LOCAL_DIRECT_SCHEMA: &str =
    include_str!("../migrations/020_trusted_local_direct.sql");
pub const MULTIPLAYER_DRAFT_SCHEMA: &str = include_str!("../migrations/021_multiplayer_drafts.sql");
pub const PIPELINE_SCHEMA: &str = include_str!("../migrations/022_pipelines.sql");
pub const COORDINATION_SHAPE_REPAIR_SCHEMA: &str =
    include_str!("../migrations/023_coordination_shape_repair.sql");
pub const ROUTING_AFFINITY_SCHEMA: &str = include_str!("../migrations/024_routing_affinity.sql");
pub const SUBAGENT_SCHEMA: &str = include_str!("../migrations/025_subagents.sql");
pub const SUBAGENT_EXECUTION_SCHEMA: &str =
    include_str!("../migrations/026_subagent_execution_bindings.sql");
pub const DELEGATION_BUDGET_SCHEMA: &str = include_str!("../migrations/027_delegation_budgets.sql");
pub const RECURSIVE_SUBAGENT_SCHEMA: &str =
    include_str!("../migrations/028_recursive_subagents.sql");
pub const CHILD_ROUTING_SCHEMA: &str = include_str!("../migrations/029_child_routing.sql");
pub const TEAM_TOPOLOGY_SCHEMA: &str = include_str!("../migrations/030_team_topology.sql");
pub const PLANNER_DAGS_SCHEMA: &str = include_str!("../migrations/031_planner_dags.sql");
pub const GUARDIAN_REVIEW_SCHEMA: &str = include_str!("../migrations/032_guardian_reviews.sql");
pub const PIPELINE_SCHEDULE_SCHEMA: &str = include_str!("../migrations/033_pipeline_schedules.sql");
const APPLICATION_ID: i64 = 0x41575333;
const LATEST_SCHEMA_VERSION: i64 = 33;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StoreMode {
    InMemory,
    OwnerLock,
    TrustedLocalDirect,
}

#[derive(Clone, Debug)]
pub struct TrustedLocalDirectOptions {
    pub heartbeat_ttl: Duration,
}

impl Default for TrustedLocalDirectOptions {
    fn default() -> Self {
        Self {
            heartbeat_ttl: Duration::from_secs(15),
        }
    }
}

pub(crate) struct Inner {
    pub conn: Mutex<Connection>,
    _lock: Option<File>,
    pub state_dir: Option<PathBuf>,
    process: Mutex<Option<coordination::ProcessIdentity>>,
    /// AW-P6-10N: retain the original owner-private OS vault barrier for the
    /// FULL original maintenance lease lifetime (not merely SQL admission).
    /// Guarded with the existing SessionStore Arc so clones cannot silently
    /// reacquire/release each other's lease; the OS drops the File on process
    /// exit and all direct LocalSecretStore mutation handles check this lock.
    vault_maintenance: Mutex<Option<(LocalMaintenanceLease, File)>>,
    mode: StoreMode,
    heartbeat_stop: Option<mpsc::Sender<()>>,
    heartbeat_thread: Option<thread::JoinHandle<()>>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(stop) = self.heartbeat_stop.take() {
            let _ = stop.send(());
        }
        if let Some(handle) = self.heartbeat_thread.take() {
            let _ = handle.join();
        }
        let process = self.process.get_mut().ok().and_then(Option::take);
        if let (Some(process), Ok(conn)) = (process, self.conn.get_mut()) {
            let _ = coordination::stop_process(conn, &process);
        }
    }
}
#[derive(Clone)]
pub struct SessionStore {
    pub(crate) inner: Arc<Inner>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchNavigationInfo {
    pub id: String,
    pub name: String,
    pub parent_name: Option<String>,
}

pub(crate) struct StoreConnectionGuard<'a> {
    conn: MutexGuard<'a, Connection>,
    process: Option<coordination::ProcessIdentity>,
    access_lease: Option<LocalCoordinationLease>,
}

impl Deref for StoreConnectionGuard<'_> {
    type Target = Connection;
    fn deref(&self) -> &Self::Target {
        &self.conn
    }
}
impl DerefMut for StoreConnectionGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.conn
    }
}
impl Drop for StoreConnectionGuard<'_> {
    fn drop(&mut self) {
        if let (Some(process), Some(lease)) = (self.process.as_ref(), self.access_lease.as_ref()) {
            let _ = coordination::settle_scope_lease(&mut self.conn, process, lease, false);
        }
    }
}
fn sqlite_error_detail(error: &rusqlite::Error) -> String {
    fn bounded(value: &str) -> String {
        let mut out = value
            .chars()
            .filter(|ch| !ch.is_control())
            .take(240)
            .collect::<String>();
        if value.chars().filter(|ch| !ch.is_control()).count() > 240 {
            out.push('…');
        }
        out
    }

    match error {
        rusqlite::Error::SqliteFailure(code, message) => {
            let message = message
                .as_deref()
                .map(bounded)
                .filter(|value| !value.is_empty())
                .map(|value| format!(": {value}"))
                .unwrap_or_default();
            format!(
                "SQLite {:?} (extended code {}){message}; no automatic replay",
                code.code, code.extended_code
            )
        }
        rusqlite::Error::QueryReturnedNoRows => {
            "SQLite query returned no row where a durable record was required; no automatic replay"
                .into()
        }
        rusqlite::Error::InvalidColumnName(name) => format!(
            "SQLite stored schema is missing expected column {}; no automatic replay",
            bounded(name)
        ),
        rusqlite::Error::InvalidParameterName(name) => format!(
            "SQLite statement used invalid parameter {}; no automatic replay",
            bounded(name)
        ),
        rusqlite::Error::IntegralValueOutOfRange(index, _) => format!(
            "SQLite integer at column {index} is outside the supported range; no automatic replay"
        ),
        rusqlite::Error::FromSqlConversionFailure(index, ty, source) => format!(
            "SQLite value conversion failed at column {index} ({ty:?}): {}; no automatic replay",
            bounded(&source.to_string())
        ),
        rusqlite::Error::Utf8Error(_) =>
            "SQLite stored text is not valid UTF-8; no automatic replay".into(),
        rusqlite::Error::NulError(_) =>
            "SQLite value contained an unexpected NUL byte; no automatic replay".into(),
        rusqlite::Error::InvalidPath(_) =>
            "SQLite path could not be represented safely; no automatic replay".into(),
        rusqlite::Error::ExecuteReturnedResults =>
            "SQLite statement unexpectedly returned rows; no automatic replay".into(),
        rusqlite::Error::QueryReturnedMoreThanOneRow =>
            "SQLite query returned multiple rows where one durable record was required; no automatic replay".into(),
        rusqlite::Error::InvalidColumnIndex(index) => format!(
            "SQLite stored schema is missing expected column index {index}; no automatic replay"
        ),
        rusqlite::Error::InvalidColumnType(index, name, ty) => format!(
            "SQLite column {} at index {index} has unexpected type {ty:?}; no automatic replay",
            bounded(name)
        ),
        rusqlite::Error::StatementChangedRows(rows) => format!(
            "SQLite statement changed {rows} rows when exactly one durable row was expected; no automatic replay"
        ),
        rusqlite::Error::ToSqlConversionFailure(source) => format!(
            "SQLite parameter conversion failed: {}; no automatic replay",
            bounded(&source.to_string())
        ),
        rusqlite::Error::InvalidQuery =>
            "SQLite statement shape was invalid for this durable operation; no automatic replay".into(),
        rusqlite::Error::UnwindingPanic =>
            "SQLite user callback panicked; no automatic replay".into(),
        rusqlite::Error::MultipleStatement =>
            "SQLite durable operation unexpectedly contained multiple statements; no automatic replay".into(),
        rusqlite::Error::InvalidParameterCount(given, expected) => format!(
            "SQLite statement received {given} parameters but expected {expected}; no automatic replay"
        ),
        rusqlite::Error::SqliteSingleThreadedMode =>
            "SQLite library is configured for single-threaded use and cannot open this durable store safely; no automatic replay".into(),
        _ => format!(
            "SQLite operation failed ({}) before an external effect; no automatic replay",
            bounded(&error.to_string())
        ),
    }
}

pub(crate) fn db<T>(v: rusqlite::Result<T>) -> Result<T> {
    v.map_err(|e| match &e {
        rusqlite::Error::SqliteFailure(code, _)
            if matches!(
                code.code,
                rusqlite::ffi::ErrorCode::DatabaseBusy
                    | rusqlite::ffi::ErrorCode::DatabaseLocked
            ) => Error::Conflict(
                "SQLite busy/locked; retry transaction acquisition explicitly; no external effect was replayed",
            ),
        _ => Error::StorageDetail(sqlite_error_detail(&e)),
    })
}

#[cfg(test)]
mod sqlite_error_diagnostic_tests {
    use super::*;

    #[test]
    fn sqlite_constraint_errors_keep_bounded_actionable_detail() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t(id INTEGER PRIMARY KEY, value TEXT UNIQUE);")
            .unwrap();
        conn.execute("INSERT INTO t(value) VALUES('bird')", [])
            .unwrap();
        let error = conn
            .execute("INSERT INTO t(value) VALUES('bird')", [])
            .unwrap_err();
        let detail = sqlite_error_detail(&error);
        assert!(detail.starts_with("SQLite "));
        assert!(detail.contains("extended code"));
        assert!(detail.contains("no automatic replay"));
        assert!(detail.len() < 512);
        assert!(!detail.chars().any(char::is_control));
    }

    #[test]
    fn sqlite_non_native_conversion_errors_do_not_collapse_to_generic_message() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t(value TEXT); INSERT INTO t(value) VALUES('bird');")
            .unwrap();
        let error = conn
            .query_row("SELECT value FROM t", [], |row| row.get::<_, i64>(0))
            .unwrap_err();
        let detail = sqlite_error_detail(&error);
        assert!(detail.contains("conversion") || detail.contains("unexpected type"));
        assert!(!detail.starts_with("SQLite operation failed before"));
        assert!(detail.contains("no automatic replay"));
    }
}

#[cfg(test)]
mod workspace_navigation_regressions {
    use super::*;

    fn binding() -> SessionBinding {
        SessionBinding {
            working_directory: PathBuf::from("/fixture"),
            tool_root: PathBuf::from("/fixture"),
            system_prompt: String::new(),
            profile_hint: "fixture".into(),
            model_hint: "model".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    #[test]
    fn branch_navigation_infers_nested_fork_parentage() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("unix:fixture").unwrap();
        let (session, main) = store.create(&actor, "fixture", binding()).unwrap();
        let feature = store
            .fork(&actor, &session.id, "feature", &main.head)
            .unwrap();
        let feature_checkpoint = store
            .checkpoint(
                &actor,
                &session.id,
                &feature.id,
                feature.revision,
                "feature checkpoint",
            )
            .unwrap();
        store
            .fork(&actor, &session.id, "nested", &feature_checkpoint.head)
            .unwrap();
        let navigation = store.branch_navigation(&actor, &session.id).unwrap();
        let parent = |name: &str| {
            navigation
                .iter()
                .find(|branch| branch.name == name)
                .and_then(|branch| branch.parent_name.as_deref())
        };
        assert_eq!(parent("feature"), Some("main"));
        assert_eq!(parent("nested"), Some("feature"));
    }

    #[test]
    fn autonomous_session_classification_uses_durable_operation_kind() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("unix:fixture").unwrap();
        let (session, main) = store.create(&actor, "fixture", binding()).unwrap();
        assert!(!store.session_is_autonomous(&actor, &session.id).unwrap());
        {
            let c = store.connection().unwrap();
            c.execute(
                "INSERT INTO llm_operations(id,session_id,origin_branch_id,origin_head,origin_revision,prompt_node,actor,identity_json,retry_policy_json,kind,state,created_at,origin_writer_epoch) VALUES('op_fixture',?1,?2,?3,?4,NULL,?5,'{}','{}','autonomous','completed',?6,1)",
                params![session.id, main.id, main.head, main.revision, actor.as_str(), now()],
            )
            .unwrap();
        }
        assert!(store.session_is_autonomous(&actor, &session.id).unwrap());
    }
}

pub(crate) fn json<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| Error::Storage("cannot encode session record"))
}
pub(crate) fn decode<T: serde::de::DeserializeOwned>(s: &str) -> Result<T> {
    serde_json::from_str(s).map_err(|_| Error::Storage("invalid or unsupported stored record"))
}
pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
        .min(i64::MAX as u128) as i64
}
pub(crate) fn id(prefix: &str) -> Result<String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|_| Error::Storage("OS randomness unavailable"))?;
    Ok(format!(
        "{prefix}_{}",
        b.iter().map(|n| format!("{n:02x}")).collect::<String>()
    ))
}
pub(crate) fn label(s: &str, max: usize) -> Result<()> {
    if s.trim().is_empty() || s.len() > max || s.chars().any(char::is_control) {
        Err(Error::Config("invalid bounded display label".into()))
    } else {
        Ok(())
    }
}
pub(crate) fn key(binding: &SessionBinding) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(json(&binding.tool_root)?.as_bytes())
    ))
}
pub fn stable_user_principal(user_id: &str) -> Result<PrincipalId> {
    if !user_id.starts_with("usr_")
        || user_id.len() > 96
        || !user_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(Error::Config("invalid stable aw user ID".into()));
    }
    PrincipalId::new(format!("user:{user_id}"))
}

pub(crate) fn user_binding_for_principal(
    conn: &Connection,
    principal: &PrincipalId,
) -> Result<Option<(String, bool)>> {
    if let Some(user_id) = principal.as_str().strip_prefix("user:") {
        return db(conn
            .query_row(
                "SELECT user_id,status='active' FROM users WHERE user_id=?1",
                [user_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional());
    }
    db(conn
        .query_row(
            "SELECT i.user_id,(i.status='active' AND u.status='active') FROM user_identities i JOIN users u ON u.user_id=i.user_id WHERE i.external_key=?1",
            [principal.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional())
}

pub(crate) fn user_for_principal(
    conn: &Connection,
    principal: &PrincipalId,
) -> Result<Option<String>> {
    Ok(user_binding_for_principal(conn, principal)?
        .and_then(|(user, active)| active.then_some(user)))
}

pub(crate) fn same_user(conn: &Connection, a: &PrincipalId, b: &PrincipalId) -> Result<bool> {
    if a == b {
        return Ok(match user_binding_for_principal(conn, a)? {
            Some((_user, active)) => active,
            None => true,
        });
    }
    let a_user = user_for_principal(conn, a)?;
    let b_user = user_for_principal(conn, b)?;
    Ok(a_user.is_some() && a_user == b_user)
}

pub(crate) fn session_owner_user(conn: &Connection, session: &str) -> Result<String> {
    db(conn
        .query_row(
            "SELECT owner_user_id FROM sessions WHERE id=?1",
            [session],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional())?
    .flatten()
    .ok_or(Error::Denied("session unavailable or not permitted"))
}

pub(crate) fn principal_is_session_owner(
    conn: &Connection,
    session: &str,
    actor: &PrincipalId,
) -> Result<bool> {
    let Some(actor_user) = user_for_principal(conn, actor)? else {
        return Ok(false);
    };
    Ok(session_owner_user(conn, session)? == actor_user)
}

pub(crate) fn authorize(
    conn: &Connection,
    session: &str,
    actor: &PrincipalId,
    append: bool,
) -> Result<()> {
    let row: Option<(String, Option<String>)> = db(conn
        .query_row(
            "SELECT owner,owner_user_id FROM sessions WHERE id=?1",
            [session],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional())?;
    let Some((owner, owner_user_id)) = row else {
        return Err(Error::Denied("session unavailable or not permitted"));
    };
    let actor_user = user_for_principal(conn, actor)?;
    if match owner_user_id.as_deref() {
        Some(owner_user) => actor_user.as_deref() == Some(owner_user),
        None => owner == actor.as_str(),
    } {
        return Ok(());
    }
    let can: Option<i64> = if let Some(user_id) = actor_user {
        db(conn
            .query_row(
                "SELECT max(can_append) FROM members WHERE session_id=?1 AND (principal_user_id=?2 OR (principal_user_id IS NULL AND principal=?3))",
                params![session, user_id, actor.as_str()],
                |r| r.get(0),
            )
            .optional())?
            .flatten()
    } else {
        db(conn
            .query_row(
                "SELECT can_append FROM members WHERE session_id=?1 AND principal=?2 AND principal_user_id IS NULL",
                params![session, actor.as_str()],
                |r| r.get(0),
            )
            .optional())?
    };
    if can.is_some_and(|v| !append || v == 1) {
        return Ok(());
    }
    #[cfg(feature = "multiplayer")]
    if !append && multiplayer::draft_read_authorized_in(conn, session, actor)? {
        return Ok(());
    }
    Err(Error::Denied("session unavailable or not permitted"))
}

pub(crate) fn ensure_user_for_principal(
    conn: &Connection,
    principal: &PrincipalId,
) -> Result<String> {
    if let Some((existing, active)) = user_binding_for_principal(conn, principal)? {
        if active {
            return Ok(existing);
        }
        return Err(Error::Denied(
            "principal is bound to an inactive or revoked stable aw user",
        ));
    }
    if principal.as_str().starts_with("user:") {
        return Err(Error::Denied(
            "stable aw user principal is unknown on this authority",
        ));
    }
    let user = id("usr")?;
    let identity = id("ident")?;
    let realm = principal
        .as_str()
        .strip_prefix("internal:")
        .and_then(|s| s.split(':').next())
        .filter(|s| !s.is_empty())
        .unwrap_or("local");
    db(conn.execute("INSERT INTO users(user_id,realm_id,status,display_json,security_epoch,created_at) VALUES(?1,?2,'active',?3,1,?4)",params![user,realm,json(&serde_json::json!({"principal":principal.as_str()}))?,now()]))?;
    db(conn.execute("INSERT INTO user_identities(identity_id,user_id,kind,external_key,verified_at,status) VALUES(?1,?2,'legacy_principal',?3,?4,'active')",params![identity,user,principal.as_str(),now()]))?;
    Ok(user)
}

pub(crate) fn audit(
    conn: &Connection,
    session: &str,
    actor: &PrincipalId,
    action: &str,
    data: serde_json::Value,
) -> Result<()> {
    let created = now();
    db(conn.execute(
        "INSERT INTO audit(session_id,actor,action,data_json,created_at) VALUES(?1,?2,?3,?4,?5)",
        params![session, actor.as_str(), action, json(&data)?, created],
    ))?;
    let user = ensure_user_for_principal(conn, actor)?;
    let store: String = db(conn.query_row(
        "SELECT value FROM store_meta WHERE key='store_id'",
        [],
        |r| r.get(0),
    ))?;
    db(conn.execute("INSERT INTO security_audit_events(audit_id,store_id,realm_id,actor_user_id,action,resource_type,resource_id,outcome,metadata_json,created_at) SELECT ?1,?2,u.realm_id,?3,?4,'session',?5,'allowed',?6,?7 FROM users u WHERE u.user_id=?3",params![id("audit")?,store,user,action,session,json(&data)?,created]))?;
    Ok(())
}

#[derive(Clone)]
pub struct AuxiliaryJournalHandle {
    store: SessionStore,
    actor: PrincipalId,
    session: String,
    branch: String,
    operation: Arc<Mutex<Option<String>>>,
}

impl AuxiliaryJournalHandle {
    pub fn new(
        store: SessionStore,
        actor: PrincipalId,
        session: impl Into<String>,
        branch: impl Into<String>,
    ) -> Self {
        Self {
            store,
            actor,
            session: session.into(),
            branch: branch.into(),
            operation: Arc::new(Mutex::new(None)),
        }
    }

    fn authorize_now(&self, identity: &InvocationIdentity) -> Result<String> {
        if identity.actor != self.actor {
            return Err(Error::Denied(
                "auxiliary inference identity does not match the active Session actor",
            ));
        }
        let connection = self.store.connection()?;
        context::authorize_tree(&connection, &self.session, &self.actor)?;
        let branch: Option<String> = db(connection
            .query_row(
                "SELECT id FROM branches WHERE session_id=?1 AND name=?2",
                params![self.session, self.branch],
                |row| row.get(0),
            )
            .optional())?;
        branch.ok_or(Error::Denied(
            "auxiliary inference branch is unavailable or not permitted",
        ))
    }
}

impl AuxiliaryJournal for AuxiliaryJournalHandle {
    fn authorize<'a>(
        &'a self,
        input: &'a AuxiliaryInput,
        identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            input.validate()?;
            self.authorize_now(identity)?;
            Ok(())
        })
    }

    fn begin<'a>(
        &'a self,
        input: &'a AuxiliaryInput,
        identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, InvocationContext> {
        Box::pin(async move {
            input.validate()?;
            let digest = input.digest()?;
            let mut connection = self.store.connection()?;
            context::authorize_tree(&connection, &self.session, &self.actor)?;
            if identity.actor != self.actor {
                return Err(Error::Denied(
                    "auxiliary inference identity does not match the active Session actor",
                ));
            }
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let branch_id: Option<String> = db(tx
                .query_row(
                    "SELECT id FROM branches WHERE session_id=?1 AND name=?2",
                    params![self.session, self.branch],
                    |row| row.get(0),
                )
                .optional())?;
            let branch_id = branch_id.ok_or(Error::Denied(
                "auxiliary inference branch is unavailable or not permitted",
            ))?;
            let operation_id = id("aux")?;
            let txid = durability::tx_id()?;
            durability::commit_event(
                &tx,
                &txid,
                "auxiliary.started",
                "session",
                &self.session,
                Some(&self.actor),
                serde_json::json!({
                    "session_id": self.session,
                    "operation_id": operation_id,
                    "branch_id": branch_id,
                    "kind": input.kind.label(),
                    "input_sha256": digest,
                    "identity": identity,
                }),
            )?;
            db(tx.commit())?;
            *self
                .operation
                .lock()
                .map_err(|_| Error::Storage("auxiliary journal unavailable"))? =
                Some(operation_id.clone());
            Ok(InvocationContext {
                purpose: InvocationPurpose::Auxiliary {
                    operation_id,
                    input_sha256: digest,
                    auxiliary_kind: input.kind.label().into(),
                },
                scope: Some(aw_agent_core::InvocationScope {
                    session_id: self.session.clone(),
                    branch_id,
                }),
                turn_id: 0,
                ordinal: 1,
                identity: identity.clone(),
            })
        })
    }

    fn finish<'a>(&'a self, result: &'a AuxiliaryResult) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            let expected = self
                .operation
                .lock()
                .map_err(|_| Error::Storage("auxiliary journal unavailable"))?
                .clone()
                .ok_or(Error::Conflict("auxiliary operation was never started"))?;
            if expected != result.operation_id {
                return Err(Error::Conflict(
                    "auxiliary settlement does not match the started operation",
                ));
            }
            let mut connection = self.store.connection()?;
            context::authorize_tree(&connection, &self.session, &self.actor)?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let txid = durability::tx_id()?;
            let output_sha256 = if result.text.is_empty() {
                None
            } else {
                Some(format!("{:x}", Sha256::digest(result.text.as_bytes())))
            };
            durability::commit_event(
                &tx,
                &txid,
                "auxiliary.finished",
                "session",
                &self.session,
                Some(&self.actor),
                serde_json::json!({
                    "session_id": self.session,
                    "operation_id": result.operation_id,
                    "kind": result.kind.label(),
                    "outcome": result.outcome,
                    "output_sha256": output_sha256,
                    "metadata": result.meta,
                    "error": result.error,
                }),
            )?;
            db(tx.commit())?;
            Ok(())
        })
    }
}

fn existing_private_state_directory(directory: &Path) -> Result<PathBuf> {
    if !directory.exists() {
        return Err(Error::Config(
            "trusted-local direct mode requires an existing initialized state directory".into(),
        ));
    }
    aw_platform::validate_private_dir(directory).map_err(|_| {
        Error::Denied("state directory requires platform-verified owner-only private custody")
    })?;
    directory.canonicalize().map_err(Into::into)
}

fn ensure_supported_local_filesystem(directory: &Path) -> Result<()> {
    if !aw_platform::trusted_direct_filesystem_supported(directory)? {
        return Err(Error::Denied(
            "trusted-local direct SQLite requires a supported local filesystem; use owner/service mode for network, FUSE, clustered or unknown filesystems",
        ));
    }
    Ok(())
}

/// AW-P6-10S: a cold-restored v1 composite is QUARANTINED before any
/// original owner Process registration, SQLite migration, recovery or model/
/// Tool/Queue admission. This is a private inert marker, not an executable
/// user grant or a stable restore/activation API. Refuse symlinked markers too.
fn reject_unapproved_cold_restore(directory: &Path) -> Result<()> {
    const QUARANTINED: &str = "cold-restored State requires independent owner activation review; no automatic Actor, Queue, paid or Tool replay";
    match fs::symlink_metadata(directory.join(".aw-cold-restore-pending")) {
        Ok(_) => return Err(Error::Denied(QUARANTINED)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(Error::Io(error)),
    }
    // AW-P6-10T: a removed FILE marker is not activation. Independently
    // refuse persistent original SQLite evidence BEFORE opening owner.lock,
    // registering a Process, migrating schema or running recovery. New empty
    // stores (no SQLite yet) and legitimately old schemas (no store_meta yet)
    // retain original initialization behavior. This is a READ-ONLY probe.
    let database = directory.join("sessions.sqlite3");
    match fs::symlink_metadata(&database) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(Error::Io(error)),
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err(Error::Denied(
                "unsafe owner SQLite file before quarantine check",
            ));
        }
        Ok(_) => {}
    }
    let handle = aw_platform::open_private_existing(&database, false)?;
    drop(handle);
    let connection = db(Connection::open_with_flags(
        &database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ))?;
    let has_meta: bool = db(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='store_meta')",
        [],
        |row| row.get(0),
    ))?;
    if has_meta {
        let marker: Option<String> = db(connection
            .query_row(
                "SELECT value FROM store_meta WHERE key='cold_restore_quarantine'",
                [],
                |row| row.get(0),
            )
            .optional())?;
        if marker.is_some() {
            return Err(Error::Denied(QUARANTINED));
        }
    }
    Ok(())
}

fn open_existing_private_state_file(directory: &Path, name: &str) -> Result<File> {
    aw_platform::open_private_existing(&directory.join(name), true).map_err(|_| {
        Error::Denied("state file requires platform-verified owner-only private custody")
    })
}

fn validate_private_sqlite_sidecars(directory: &Path) -> Result<()> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let path = directory.join(format!("sessions.sqlite3{suffix}"));
        match aw_platform::open_private_existing(&path, false) {
            Ok(file) => drop(file),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(Error::Denied("unsafe SQLite sidecar")),
        }
    }
    Ok(())
}

fn configure_connection(c: &Connection) -> Result<()> {
    db(c.busy_timeout(Duration::from_millis(500)))?;
    db(c.execute_batch(
        "PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF; PRAGMA synchronous=FULL; PRAGMA journal_mode=WAL;",
    ))?;
    Ok(())
}

fn validate_current_schema(c: &Connection) -> Result<()> {
    let version: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    let app: i64 = db(c.pragma_query_value(None, "application_id", |r| r.get(0)))?;
    if version != LATEST_SCHEMA_VERSION || app != APPLICATION_ID {
        return Err(Error::Storage(
            "trusted-local direct mode requires the current aw schema; open once in owner mode to migrate",
        ));
    }
    let schema_epoch: String = db(c.query_row(
        "SELECT value FROM store_meta WHERE key='schema_epoch'",
        [],
        |r| r.get(0),
    ))?;
    let coordination_schema: String = db(c.query_row(
        "SELECT value FROM store_meta WHERE key='coordination_schema'",
        [],
        |r| r.get(0),
    ))?;
    if schema_epoch != LATEST_SCHEMA_VERSION.to_string() || coordination_schema != "2" {
        return Err(Error::Storage(
            "trusted-local direct mode found inconsistent schema/coordination metadata",
        ));
    }
    Ok(())
}

fn start_direct_heartbeat(
    database: PathBuf,
    process: coordination::ProcessIdentity,
    ttl: Duration,
) -> Result<(mpsc::Sender<()>, thread::JoinHandle<()>)> {
    let interval = ttl
        .checked_div(3)
        .unwrap_or(Duration::from_secs(1))
        .max(Duration::from_millis(100));
    let (stop_tx, stop_rx) = mpsc::channel();
    let handle = thread::Builder::new()
        .name("aw-store-heartbeat".into())
        .spawn(move || {
            let Ok(conn) = Connection::open(&database) else {
                return;
            };
            if configure_connection(&conn).is_err() || validate_current_schema(&conn).is_err() {
                return;
            }
            loop {
                match stop_rx.recv_timeout(interval) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let deadline = std::time::Instant::now() + interval;
                        loop {
                            match coordination::heartbeat_process(&conn, &process) {
                                Ok(_) => break,
                                Err(Error::Conflict(message))
                                    if message.starts_with("SQLite busy/locked")
                                        && std::time::Instant::now() < deadline =>
                                {
                                    match stop_rx.recv_timeout(Duration::from_millis(50)) {
                                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                                            return
                                        }
                                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                                    }
                                }
                                Err(_) => return,
                            }
                        }
                    }
                }
            }
        })
        .map_err(|_| Error::Storage("cannot start trusted-local heartbeat thread"))?;
    Ok((stop_tx, handle))
}

impl SessionStore {
    /// Private directory must be owned by the current Unix user and inaccessible
    /// to other users. An advisory lock excludes a second aw owner. Not a defense
    /// against malicious same-UID code, network filesystems or a hostile OS.
    pub fn open(directory: &Path) -> Result<Self> {
        if !aw_platform::secure_private_files_supported() {
            return Err(Error::Config(
                "persistent store requires platform-verified owner-only private files".into(),
            ));
        }
        aw_platform::ensure_private_dir(directory).map_err(|_| {
            Error::Denied("state directory requires platform-verified owner-only private custody")
        })?;
        let directory = directory.canonicalize()?;
        // Must reject BEFORE owner.lock / SQLite file creation/registration.
        reject_unapproved_cold_restore(&directory)?;
        let lock = aw_platform::open_private_rw_create(&directory.join("owner.lock"))
            .map_err(|_| Error::Denied("unsafe owner.lock custody"))?;
        FileExt::try_lock_exclusive(&lock).map_err(|_| {
            Error::Conflict(
                "another aw owner holds this store; use it or a separate state directory",
            )
        })?;
        let database = aw_platform::open_private_rw_create(&directory.join("sessions.sqlite3"))
            .map_err(|_| Error::Denied("unsafe SQLite database custody"))?;
        drop(database);
        validate_private_sqlite_sidecars(&directory)?;
        let mut conn = db(Connection::open(directory.join("sessions.sqlite3")))?;
        initialize(&mut conn)?;
        let process = coordination::register_owner_process(&mut conn)?;
        let out = Self {
            inner: Arc::new(Inner {
                conn: Mutex::new(conn),
                _lock: Some(lock),
                state_dir: Some(directory),
                process: Mutex::new(Some(process)),
                vault_maintenance: Mutex::new(None),
                mode: StoreMode::OwnerLock,
                heartbeat_stop: None,
                heartbeat_thread: None,
            }),
        };
        out.recover()?;
        Ok(out)
    }

    /// Explicit trusted-local multi-process store mode.
    ///
    /// This is intentionally separate from `open()`: it takes `owner.lock`
    /// shared (owner/service takes it exclusively), never creates/migrates a
    /// store, and accepts only the exact current schema in an already-private
    /// local state directory.
    /// Ordinary store calls are conservatively serialized across all persistent
    /// processes by the Phase-35 local access lease. Remote/network-mounted
    /// SQLite remains unsupported.
    pub fn open_trusted_local_direct(
        directory: &Path,
        options: TrustedLocalDirectOptions,
    ) -> Result<Self> {
        if options.heartbeat_ttl < Duration::from_secs(3)
            || options.heartbeat_ttl > Duration::from_secs(5 * 60)
        {
            return Err(Error::Config(
                "trusted-local heartbeat TTL must be between 3s and 5m".into(),
            ));
        }
        let directory = existing_private_state_directory(directory)?;
        reject_unapproved_cold_restore(&directory)?;
        ensure_supported_local_filesystem(&directory)?;
        let lock = open_existing_private_state_file(&directory, "owner.lock")?;
        FileExt::try_lock_shared(&lock).map_err(|_| {
            Error::Conflict(
                "an aw owner/service process holds this store; trusted-local direct mode is a separate ownership mode",
            )
        })?;
        let database_file = open_existing_private_state_file(&directory, "sessions.sqlite3")?;
        drop(database_file);
        validate_private_sqlite_sidecars(&directory)?;
        let database_path = directory.join("sessions.sqlite3");
        let mut conn = db(Connection::open_with_flags(
            &database_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
        ))?;
        configure_connection(&conn)?;
        validate_current_schema(&conn)?;
        let process =
            coordination::register_trusted_process(&mut conn, options.heartbeat_ttl, true)?;
        let heartbeat =
            start_direct_heartbeat(database_path, process.clone(), options.heartbeat_ttl);
        let (heartbeat_stop, heartbeat_thread) = match heartbeat {
            Ok(parts) => parts,
            Err(error) => {
                let _ = coordination::stop_process(&conn, &process);
                return Err(error);
            }
        };
        let out = Self {
            inner: Arc::new(Inner {
                conn: Mutex::new(conn),
                _lock: Some(lock),
                state_dir: Some(directory),
                process: Mutex::new(Some(process)),
                vault_maintenance: Mutex::new(None),
                mode: StoreMode::TrustedLocalDirect,
                heartbeat_stop: Some(heartbeat_stop),
                heartbeat_thread: Some(heartbeat_thread),
            }),
        };
        out.recover()?;
        Ok(out)
    }

    /// In-memory backend for real SQLite tests; no process lock or disk guarantees.
    pub fn in_memory() -> Result<Self> {
        let mut c = db(Connection::open_in_memory())?;
        initialize(&mut c)?;
        Ok(Self {
            inner: Arc::new(Inner {
                conn: Mutex::new(c),
                _lock: None,
                state_dir: None,
                process: Mutex::new(None),
                vault_maintenance: Mutex::new(None),
                mode: StoreMode::InMemory,
                heartbeat_stop: None,
                heartbeat_thread: None,
            }),
        })
    }
    #[cfg(test)]
    fn open_coordinated_for_phase34_stress(directory: &Path, ttl: Duration) -> Result<Self> {
        // Test-only independent SQLite connection used for the Phase-34 raw
        // WAL/CAS adversarial matrix. Supported Phase-35 direct access uses
        // open_trusted_local_direct() and the shared outer owner.lock; this
        // helper intentionally bypasses that product boundary only under tests.
        let mut c = db(Connection::open(directory.join("sessions.sqlite3")))?;
        configure_connection(&c)?;
        validate_current_schema(&c)?;
        let process = coordination::register_stress_process(&mut c, ttl)?;
        Ok(Self {
            inner: Arc::new(Inner {
                conn: Mutex::new(c),
                _lock: None,
                state_dir: Some(directory.to_owned()),
                process: Mutex::new(Some(process)),
                vault_maintenance: Mutex::new(None),
                mode: StoreMode::TrustedLocalDirect,
                heartbeat_stop: None,
                heartbeat_thread: None,
            }),
        })
    }

    pub fn directory(&self) -> Option<&Path> {
        self.inner.state_dir.as_deref()
    }
    pub(crate) fn process_identity(&self) -> Result<Option<coordination::ProcessIdentity>> {
        self.inner.process.lock().map(|p| p.clone()).map_err(|_| {
            Error::Storage("local process identity lock poisoned; restart and inspect")
        })
    }
    pub(crate) fn trusted_local_direct(&self) -> bool {
        self.inner.mode == StoreMode::TrustedLocalDirect
    }
    fn coordination_connection(&self) -> Result<MutexGuard<'_, Connection>> {
        let process = self.process_identity()?;
        let conn = self
            .inner
            .conn
            .lock()
            .map_err(|_| Error::Storage("store lock poisoned; restart and inspect"))?;
        if let Some(process) = process.as_ref() {
            coordination::validate_process(&conn, process)?;
        }
        Ok(conn)
    }

    pub(crate) fn connection(&self) -> Result<StoreConnectionGuard<'_>> {
        let process = self.process_identity()?;
        let mut conn = self.coordination_connection()?;
        coordination::validate_ordinary_access(&conn)?;
        let access_lease = if let Some(process) = process.as_ref() {
            Some(coordination::acquire_scope_lease(
                &mut conn,
                process,
                LocalLeaseKind::Write,
                "store:ordinary-access",
                None,
                Duration::from_secs(30 * 60),
            )?)
        } else {
            None
        };
        Ok(StoreConnectionGuard {
            conn,
            process,
            access_lease,
        })
    }
    fn coordinated_process(&self) -> Result<coordination::ProcessIdentity> {
        self.process_identity()?.ok_or(Error::Denied(
            "local coordination requires a persistent store process",
        ))
    }

    pub fn local_process_info(&self) -> Result<LocalProcessInfo> {
        let process = self.coordinated_process()?;
        let c = self.coordination_connection()?;
        coordination::process_info(&c, &process)
    }

    pub fn heartbeat_local_process(&self) -> Result<LocalProcessInfo> {
        let process = self.coordinated_process()?;
        let c = self.coordination_connection()?;
        coordination::heartbeat_process(&c, &process)
    }

    pub fn acquire_local_write_lease(
        &self,
        scope_key: &str,
        ttl: Duration,
    ) -> Result<LocalCoordinationLease> {
        let process = self.coordinated_process()?;
        let mut c = self.coordination_connection()?;
        coordination::acquire_scope_lease(
            &mut c,
            &process,
            LocalLeaseKind::Write,
            scope_key,
            None,
            ttl,
        )
    }

    pub fn acquire_local_execution_lease(
        &self,
        scope_key: &str,
        run_id: Option<&str>,
        ttl: Duration,
    ) -> Result<LocalCoordinationLease> {
        let process = self.coordinated_process()?;
        let mut c = self.coordination_connection()?;
        coordination::acquire_scope_lease(
            &mut c,
            &process,
            LocalLeaseKind::Execution,
            scope_key,
            run_id,
            ttl,
        )
    }

    pub fn settle_local_lease(
        &self,
        lease: &LocalCoordinationLease,
        uncertain_external_effect: bool,
    ) -> Result<()> {
        if uncertain_external_effect && lease.kind != LocalLeaseKind::Execution {
            return Err(Error::Config(
                "only execution leases can preserve an unknown external-effect hazard".into(),
            ));
        }
        let process = self.coordinated_process()?;
        let mut c = self.coordination_connection()?;
        coordination::settle_scope_lease(&mut c, &process, lease, uncertain_external_effect)
    }

    pub fn resolve_local_execution_hazard(
        &self,
        scope_key: &str,
        expected_generation: u64,
        note: &str,
    ) -> Result<()> {
        let process = self.coordinated_process()?;
        let mut c = self.coordination_connection()?;
        coordination::resolve_execution_hazard(
            &mut c,
            &process,
            scope_key,
            expected_generation,
            note,
        )
    }

    pub fn acquire_local_maintenance_lease(
        &self,
        purpose: &str,
        ttl: Duration,
    ) -> Result<LocalMaintenanceLease> {
        let process = self.coordinated_process()?;
        let mut fence = self.inner.vault_maintenance.lock().map_err(|_| {
            Error::Storage("owner vault maintenance fence poisoned; restart and inspect")
        })?;
        if fence.is_some() {
            return Err(Error::Conflict(
                "original owner already holds a vault maintenance fence",
            ));
        }
        // Acquire the actual ORIGINAL file-vault gate BEFORE admitting the
        // SQLite maintenance lease. try_lock_exclusive is deliberately
        // nonblocking: an in-flight SecretStore put/delete holds a shared lock
        // until its actual write/sync is closed, so a hot/cross-resource
        // capture must fail rather than race the ciphertext files. Fresh and
        // cached direct vault handles alike must take that same stable inode.
        let vault_file = if let Some(root) = self.directory() {
            let file = secret_store::open_vault_mutation_file(root)?;
            FileExt::try_lock_exclusive(&file).map_err(|_| {
                Error::Conflict(
                    "SecretStore mutation in progress or another owner vault fence active",
                )
            })?;
            Some(file)
        } else {
            None
        };
        let mut c = self.coordination_connection()?;
        let lease = coordination::acquire_maintenance(&mut c, &process, purpose, ttl)?;
        if let Some(file) = vault_file {
            *fence = Some((lease.clone(), file));
        }
        Ok(lease)
    }

    pub fn release_local_maintenance_lease(&self, lease: &LocalMaintenanceLease) -> Result<()> {
        let mut fence = self.inner.vault_maintenance.lock().map_err(|_| {
            Error::Storage("owner vault maintenance fence poisoned; restart and inspect")
        })?;
        if self.directory().is_some()
            && !fence.as_ref().is_some_and(|(current, _)| current == lease)
        {
            return Err(Error::Denied(
                "original vault maintenance lease identity changed",
            ));
        }
        let process = self.coordinated_process()?;
        let mut c = self
            .inner
            .conn
            .lock()
            .map_err(|_| Error::Storage("store lock poisoned; restart and inspect"))?;
        coordination::release_maintenance(&mut c, &process, lease)?;
        // SQL maintenance release is committed BEFORE the OS vault lock is
        // dropped; there is no window where private blobs can change while
        // the old SQL writer still claims a quiesced composite capture.
        fence.take();
        Ok(())
    }

    pub fn create(
        &self,
        owner: &PrincipalId,
        title: &str,
        binding: SessionBinding,
    ) -> Result<(SessionInfo, BranchInfo)> {
        label(title, 160)?;
        validate_binding(&binding)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = ensure_user_for_principal(&tx, owner)?;
        let realm: String = db(tx.query_row(
            "SELECT realm_id FROM users WHERE user_id=?1",
            [&user],
            |r| r.get(0),
        ))?;
        let (_, workspace) = resources::ensure_default_domain_workspace(&tx, &user, &realm)?;
        let result =
            resources::create_in_workspace_tx(&tx, owner, title, &binding, &workspace, None)?;
        db(tx.commit())?;
        Ok(result)
    }
    pub fn session(&self, actor: &PrincipalId, session: &str) -> Result<SessionInfo> {
        let c = self.connection()?;
        context::authorize_tree(&c, session, actor)?;
        session_in(&c, session)
    }
    pub fn set_title(
        &self,
        actor: &PrincipalId,
        session: &str,
        title: &str,
    ) -> Result<SessionInfo> {
        label(title, 160)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        if !principal_is_session_owner(&tx, session, actor)? {
            return Err(Error::Denied("only session owner can change the title"));
        }
        let at = now();
        let changed = db(tx.execute(
            "UPDATE sessions SET title=?1,updated_at=?2 WHERE id=?3",
            params![title, at, session],
        ))?;
        if changed != 1 {
            return Err(Error::Denied("session unavailable or not permitted"));
        }
        audit(
            &tx,
            session,
            actor,
            "session.title_changed",
            serde_json::json!({"title": title}),
        )?;
        let info = session_in(&tx, session)?;
        db(tx.commit())?;
        Ok(info)
    }
    /// Per-user Sessions-list archive state. This is local application
    /// presentation state, not Session lifecycle or security-audit authority.
    /// Events are explicitly marked local-only so the semantic-sync cursor may
    /// advance across them without ever disclosing them to a peer.
    pub fn set_session_archived(
        &self,
        actor: &PrincipalId,
        session: &str,
        archived: bool,
    ) -> Result<()> {
        self.set_sessions_archived(actor, &[session.to_owned()], archived)
    }

    /// Atomically archive/restore a bounded visible Session selection for one
    /// viewer. All authorization and legacy-state reads happen before any new
    /// preference event is committed, so one user action cannot partially
    /// apply across its selected Sessions.
    pub fn set_sessions_archived(
        &self,
        actor: &PrincipalId,
        sessions: &[String],
        archived: bool,
    ) -> Result<()> {
        if sessions.is_empty() {
            return Ok(());
        }
        if sessions.len() > 128 {
            return Err(Error::Limit("at most 128 Sessions may be updated at once"));
        }
        let unique = sessions.iter().map(String::as_str).collect::<BTreeSet<_>>();
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let viewer = user_for_principal(&tx, actor)?
            .unwrap_or_else(|| format!("principal:{}", actor.as_str()));

        let mut changed = Vec::new();
        for session in unique {
            context::authorize_tree(&tx, session, actor)?;
            let current: Option<i64> = db(tx
                .query_row(
                    "SELECT CAST(json_extract(payload_json,'$.archived') AS INTEGER)
                     FROM commit_log
                     WHERE kind='session.view.changed' AND entity_type='session'
                       AND entity_id=?1
                       AND json_extract(payload_json,'$.viewer')=?2
                     ORDER BY seq DESC LIMIT 1",
                    params![session, viewer],
                    |row| row.get(0),
                )
                .optional())?;
            let current = if let Some(value) = current {
                value != 0
            } else if principal_is_session_owner(&tx, session, actor)? {
                let legacy: Option<String> = db(tx
                    .query_row(
                        "SELECT action FROM audit WHERE session_id=?1
                         AND action IN ('session.hidden','session.restored')
                         ORDER BY seq DESC LIMIT 1",
                        [session],
                        |row| row.get(0),
                    )
                    .optional())?;
                legacy.as_deref() == Some("session.hidden")
            } else {
                false
            };
            if current != archived {
                changed.push(session.to_owned());
            }
        }

        if changed.is_empty() {
            db(tx.commit())?;
            return Ok(());
        }
        let txid = durability::tx_id()?;
        for session in changed {
            durability::commit_event(
                &tx,
                &txid,
                "session.view.changed",
                "session",
                &session,
                Some(actor),
                serde_json::json!({
                    "viewer": viewer,
                    "archived": archived,
                    "sync_scope": "local"
                }),
            )?;
        }
        db(tx.commit())?;
        Ok(())
    }

    fn list_by_archive_state(
        &self,
        actor: &PrincipalId,
        archived: bool,
    ) -> Result<Vec<SessionInfo>> {
        let c = self.connection()?;
        let actor_user = user_for_principal(&c, actor)?;
        let viewer = actor_user
            .clone()
            .unwrap_or_else(|| format!("principal:{}", actor.as_str()));
        let mut q=db(c.prepare("SELECT DISTINCT s.id FROM sessions s LEFT JOIN members m ON m.session_id=s.id WHERE ((s.owner_user_id IS NULL AND s.owner=?1) OR (m.principal_user_id IS NULL AND m.principal=?1) OR (?2 IS NOT NULL AND (s.owner_user_id=?2 OR m.principal_user_id=?2))) AND COALESCE((SELECT CAST(json_extract(c.payload_json,'$.archived') AS INTEGER) FROM commit_log c WHERE c.kind='session.view.changed' AND c.entity_type='session' AND c.entity_id=s.id AND json_extract(c.payload_json,'$.viewer')=?3 ORDER BY c.seq DESC LIMIT 1),CASE WHEN ((s.owner_user_id IS NULL AND s.owner=?1) OR (?2 IS NOT NULL AND s.owner_user_id=?2)) THEN COALESCE((SELECT CASE a.action WHEN 'session.hidden' THEN 1 ELSE 0 END FROM audit a WHERE a.session_id=s.id AND a.action IN ('session.hidden','session.restored') ORDER BY a.seq DESC LIMIT 1),0) ELSE 0 END)=?4 ORDER BY s.updated_at DESC,s.id LIMIT 200"))?;
        let rows = db(q.query_map(
            params![actor.as_str(), actor_user, viewer, i64::from(archived)],
            |r| r.get::<_, String>(0),
        ))?;
        let ids = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        let mut visible = Vec::with_capacity(ids.len());
        for id in ids {
            match context::authorize_tree(&c, &id, actor) {
                Ok(()) => visible.push(session_in(&c, &id)?),
                Err(Error::Denied(_)) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(visible)
    }

    pub fn list(&self, actor: &PrincipalId) -> Result<Vec<SessionInfo>> {
        self.list_by_archive_state(actor, false)
    }

    pub fn list_archived(&self, actor: &PrincipalId) -> Result<Vec<SessionInfo>> {
        self.list_by_archive_state(actor, true)
    }
    pub fn branches(&self, actor: &PrincipalId, session: &str) -> Result<Vec<BranchInfo>> {
        let c = self.connection()?;
        context::authorize_tree(&c, session, actor)?;
        let mut q =
            db(c.prepare("SELECT id FROM branches WHERE session_id=?1 ORDER BY created_at,id"))?;
        let ids = db(db(q.query_map([session], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter().map(|id| branch_in(&c, session, id)).collect()
    }
    pub fn session_is_autonomous(&self, actor: &PrincipalId, session: &str) -> Result<bool> {
        let c = self.connection()?;
        context::authorize_tree(&c, session, actor)?;
        db(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM llm_operations WHERE session_id=?1 AND kind='autonomous')",
            [session],
            |r| r.get(0),
        ))
    }

    pub fn branch_navigation(
        &self,
        actor: &PrincipalId,
        session: &str,
    ) -> Result<Vec<BranchNavigationInfo>> {
        let c = self.connection()?;
        context::authorize_tree(&c, session, actor)?;
        let mut query = db(c.prepare(
            "SELECT id,name,head,created_at FROM branches WHERE session_id=?1 ORDER BY created_at,id",
        ))?;
        let branch_rows = db(query.query_map([session], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        }))?;
        let branches = db(branch_rows.collect::<rusqlite::Result<Vec<_>>>())?;

        let mut node_query = db(c.prepare("SELECT id,parent_id FROM nodes WHERE session_id=?1"))?;
        let node_rows = db(node_query.query_map([session], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        }))?;
        let parents = db(node_rows.collect::<rusqlite::Result<Vec<_>>>())?
            .into_iter()
            .collect::<BTreeMap<_, _>>();

        let mut creation_heads = BTreeMap::<String, String>::new();
        let mut event_query = db(c.prepare(
            "SELECT branch_id,old_head FROM branch_ref_events WHERE cause_type='branch_created' AND branch_id IN (SELECT id FROM branches WHERE session_id=?1) ORDER BY committed_at,event_id",
        ))?;
        let events = db(event_query.query_map([session], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        }))?;
        for event in events {
            let (branch, head) = db(event)?;
            creation_heads.entry(branch).or_insert(head);
        }

        let ancestry = |head: &str| {
            let mut out = BTreeSet::new();
            let mut cursor = Some(head.to_owned());
            while let Some(node) = cursor {
                if !out.insert(node.clone()) {
                    break;
                }
                cursor = parents.get(&node).cloned().flatten();
            }
            out
        };
        let current_ancestry = branches
            .iter()
            .map(|(id, _, head, _)| (id.clone(), ancestry(head)))
            .collect::<BTreeMap<_, _>>();

        let mut out = Vec::with_capacity(branches.len());
        for (id, name, head, created_at) in &branches {
            let fork_head = creation_heads.get(id).unwrap_or(head);
            let parent = branches
                .iter()
                .filter(|(candidate_id, _, _, candidate_created)| {
                    candidate_id != id && candidate_created <= created_at
                })
                .filter(|(candidate_id, _, _, _)| {
                    current_ancestry
                        .get(candidate_id)
                        .is_some_and(|nodes| nodes.contains(fork_head))
                })
                .max_by_key(|(_, _, _, candidate_created)| *candidate_created)
                .map(|(_, parent_name, _, _)| parent_name.clone());
            out.push(BranchNavigationInfo {
                id: id.clone(),
                name: name.clone(),
                parent_name: parent,
            });
        }
        Ok(out)
    }

    pub fn tree(&self, actor: &PrincipalId, session: &str) -> Result<Vec<Node>> {
        let c = self.connection()?;
        context::authorize_tree(&c, session, actor)?;
        nodes_in(&c, session)
    }
    /// Read one exact durable branch under the current read policy. The owned
    /// projection carries no journal/cursor and never activates an Agent.
    pub fn read_branch_view(
        &self,
        actor: &PrincipalId,
        session_id: &str,
        branch_id: &str,
    ) -> Result<SessionView> {
        let c = self.connection()?;
        read_branch_view_in(&c, actor, session_id, branch_id, true)
    }

    pub fn activate(
        &self,
        actor: &PrincipalId,
        session: &str,
        branch: &str,
    ) -> Result<(SessionView, SessionHandle)> {
        let c = self.connection()?;
        // Activation keeps its historical ID-or-name selector. Pinned readers
        // use exact IDs and never receive this effect-capable handle.
        let view = read_branch_view_in(&c, actor, session, branch, false)?;
        let handle = SessionHandle::new(
            self.clone(),
            actor.clone(),
            view.session.id.clone(),
            view.branch.clone(),
        );
        Ok((view, handle))
    }
    pub fn fork(
        &self,
        actor: &PrincipalId,
        session: &str,
        name: &str,
        at: &str,
    ) -> Result<BranchInfo> {
        label(name, 80)?;
        if name.starts_with("b_") {
            return Err(Error::Config(
                "branch names beginning b_ are reserved for IDs".into(),
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        authorize(&tx, session, actor, true)?;
        let _writer = workspace_writer::require_workspace_writer_for_session(&tx, session)?;
        context::authorize_node(&tx, session, at, actor)?;
        let kind: Option<String> = db(tx
            .query_row(
                "SELECT kind FROM nodes WHERE session_id=?1 AND id=?2",
                params![session, at],
                |r| r.get(0),
            )
            .optional())?;
        if matches!(kind.as_deref(), None | Some("prompt")) {
            return Err(Error::Conflict(
                "fork requires a root, finished attempt or checkpoint node",
            ));
        }
        let branches: i64 = db(tx.query_row(
            "SELECT count(*) FROM branches WHERE session_id=?1",
            [session],
            |r| r.get(0),
        ))?;
        if branches >= 1024 {
            return Err(Error::Limit("session branch limit reached"));
        }
        let branch = id("b")?;
        db(tx.execute(
            "INSERT INTO branches(id,session_id,name,head,created_at) VALUES(?1,?2,?3,?4,?5)",
            params![branch, session, name, at, now()],
        ))?;
        let txid = durability::tx_id()?;
        let created = branch_in(&tx, session, &branch)?;
        durability::ref_event(
            &tx,
            &BranchInfo {
                id: created.id.clone(),
                session_id: created.session_id.clone(),
                name: created.name.clone(),
                head: at.into(),
                revision: 0,
                active_run: None,
            },
            at,
            0,
            actor,
            "branch_created",
            None,
            &txid,
        )?;
        audit(
            &tx,
            session,
            actor,
            "branch.fork",
            serde_json::json!({"branch":branch,"at":at}),
        )?;
        durability::commit_event(
            &tx,
            &txid,
            "branch.created",
            "branch",
            &branch,
            Some(actor),
            serde_json::json!({"session_id":session,"head":at,"name":name}),
        )?;
        let out = branch_in(&tx, session, &branch)?;
        db(tx.commit())?;
        Ok(out)
    }
    /// Explicit human-authored summary: keeps all original nodes, cuts only the
    /// future context projection. No paid LLM summarization or hidden truncation.
    pub fn checkpoint(
        &self,
        actor: &PrincipalId,
        session: &str,
        branch: &str,
        expected: u64,
        summary: &str,
    ) -> Result<BranchInfo> {
        if summary.trim().is_empty() || summary.len() > aw_agent_core::limits::MAX_PROMPT_BYTES {
            return Err(Error::Limit("checkpoint needs 1..131072 UTF-8 bytes"));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        authorize(&tx, session, actor, true)?;
        let _writer = workspace_writer::require_workspace_writer_for_session(&tx, session)?;
        let b = branch_in(&tx, session, branch)?;
        context::authorize_node(&tx, session, &b.head, actor)?;
        if b.revision != expected || b.active_run.is_some() {
            return Err(Error::Conflict("branch moved or is busy"));
        }
        let count: i64 = db(tx.query_row(
            "SELECT count(*) FROM nodes WHERE session_id=?1",
            [session],
            |r| r.get(0),
        ))?;
        if count >= MAX_SESSION_NODES as i64 {
            return Err(Error::Limit(
                "session node limit reached; export and start another session",
            ));
        }
        let node = id("n")?;
        insert_node(
            &tx,
            session,
            &node,
            Some(&b.head),
            "checkpoint",
            actor,
            serde_json::json!({"summary":summary,"authored_by_user":true}),
        )?;
        let changed=db(tx.execute("UPDATE branches SET head=?1,revision=revision+1 WHERE id=?2 AND head=?3 AND revision=?4 AND active_run IS NULL",params![node,b.id,b.head,b.revision]))?;
        if changed != 1 {
            return Err(Error::Conflict("branch moved during checkpoint; reload"));
        }
        let txid = durability::tx_id()?;
        durability::ref_event(
            &tx,
            &b,
            &node,
            b.revision + 1,
            actor,
            "checkpoint",
            Some(&node),
            &txid,
        )?;
        db(tx.execute(
            "UPDATE sessions SET updated_at=?1 WHERE id=?2",
            params![now(), session],
        ))?;
        audit(
            &tx,
            session,
            actor,
            "context.checkpoint",
            serde_json::json!({"node":node,"previous_head":b.head}),
        )?;
        let out = branch_in(&tx, session, &b.id)?;
        db(tx.commit())?;
        Ok(out)
    }
    /// Only the session owner changes membership. No provider credential grant is
    /// copied with session access. Shipped CLI/web remain one local principal.
    pub fn grant(
        &self,
        owner: &PrincipalId,
        session: &str,
        member: &PrincipalId,
        append: Option<bool>,
    ) -> Result<()> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let _s = session_in(&tx, session)?;
        if !principal_is_session_owner(&tx, session, owner)? {
            return Err(Error::Denied("only session owner manages membership"));
        }
        if append.is_some() {
            context::authorize_tree(&tx, session, owner)?;
            context::authorize_dependencies_for_grant(&tx, session, member)?;
        }
        match append {
            Some(a) => {
                let member_user = ensure_user_for_principal(&tx, member)?;
                db(tx.execute("INSERT INTO members(session_id,principal,can_append,principal_user_id) VALUES(?1,?2,?3,?4) ON CONFLICT(session_id,principal) DO UPDATE SET can_append=excluded.can_append,principal_user_id=excluded.principal_user_id",params![session,member.as_str(),i64::from(a),member_user]))?;
            }
            None => {
                let member_user = user_for_principal(&tx, member)?;
                db(tx.execute(
                    "DELETE FROM members WHERE session_id=?1 AND (principal=?2 OR (?3 IS NOT NULL AND principal_user_id=?3))",
                    params![session, member.as_str(), member_user],
                ))?;
                db(tx.execute(
                    "DELETE FROM context_source_grants WHERE session_id=?1 AND (principal=?2 OR (?3 IS NOT NULL AND principal_user_id=?3))",
                    params![session, member.as_str(), member_user],
                ))?;
            }
        }
        audit(
            &tx,
            session,
            owner,
            "membership.changed",
            serde_json::json!({"member":member,"append":append}),
        )?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "membership.changed",
            "session",
            session,
            Some(owner),
            serde_json::json!({"session_id":session,"member":member.as_str(),"append":append}),
        )?;
        db(tx.commit())?;
        Ok(())
    }
    pub fn usage(&self, actor: &PrincipalId, session: &str) -> Result<StoredUsage> {
        let c = self.connection()?;
        context::authorize_tree(&c, session, actor)?;
        let mut q=db(c.prepare("SELECT i.identity_json,i.metadata_json FROM invocation_receipts i JOIN runs r ON r.id=i.run_id WHERE r.session_id=?1 ORDER BY r.started_at,r.turn_id,i.ordinal"))?;
        let rows = db(q.query_map([session], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        }))?;
        let mut out = StoredUsage {
            actor: actor.clone(),
            model_calls: 0,
            input_tokens: Some(0),
            output_tokens: Some(0),
            calls_without_complete_usage: 0,
        };
        for row in rows {
            let (identity, meta) = db(row)?;
            let who: aw_agent_core::InvocationIdentity = decode(&identity)?;
            if &who.actor != actor {
                continue;
            }
            let m: Option<aw_agent_core::CompletionMeta> =
                meta.as_deref().map(decode).transpose()?;
            let input = m.as_ref().and_then(|v| v.usage.input_tokens);
            let output = m.as_ref().and_then(|v| v.usage.output_tokens);
            out.model_calls += 1;
            out.calls_without_complete_usage += u64::from(input.is_none() || output.is_none());
            out.input_tokens = out
                .input_tokens
                .zip(input)
                .and_then(|(a, b)| a.checked_add(b));
            out.output_tokens = out
                .output_tokens
                .zip(output)
                .and_then(|(a, b)| a.checked_add(b));
        }
        drop(q);
        let mut q=db(c.prepare("SELECT o.identity_json,o.metadata_json FROM context_operations o JOIN context_plans p ON p.id=o.plan_id WHERE p.target_session=?1 AND o.actor=?2 AND o.identity_json IS NOT NULL ORDER BY o.created_at,o.id"))?;
        for row in db(q.query_map(params![session, actor.as_str()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        }))? {
            let (_, meta) = db(row)?;
            let m: Option<aw_agent_core::CompletionMeta> =
                meta.as_deref().map(decode).transpose()?;
            let input = m.as_ref().and_then(|m| m.usage.input_tokens);
            let output = m.as_ref().and_then(|m| m.usage.output_tokens);
            out.model_calls += 1;
            out.calls_without_complete_usage += u64::from(input.is_none() || output.is_none());
            out.input_tokens = out
                .input_tokens
                .zip(input)
                .and_then(|(a, b)| a.checked_add(b));
            out.output_tokens = out
                .output_tokens
                .zip(output)
                .and_then(|(a, b)| a.checked_add(b));
        }
        Ok(out)
    }
    pub fn backup(&self, destination: &Path) -> Result<()> {
        let process = self.process_identity()?;
        let maintenance = if process.is_some() {
            Some(self.acquire_local_maintenance_lease("backup", Duration::from_secs(30 * 60))?)
        } else {
            None
        };
        // AW-P6-10K: a failed exclusive create means the destination is NOT
        // ours. Never unlink a preexisting snapshot, private file or symlink
        // merely because the no-overwrite guard rejected the backup request.
        // Once this call has created its own temporary output, failures may
        // clean up that incomplete snapshot only.
        let mut created_by_this_backup = false;
        let result: Result<()> = (|| {
            let f = private_new(destination)?;
            created_by_this_backup = true;
            drop(f);
            let c = self
                .inner
                .conn
                .lock()
                .map_err(|_| Error::Storage("store lock poisoned; restart and inspect"))?;
            db(c.execute(
                "VACUUM INTO ?1",
                [destination
                    .to_str()
                    .ok_or_else(|| Error::Config("backup path must be UTF-8".into()))?],
            ))?;
            // SQLite VACUUM INTO yields a transactionally consistent image,
            // but does not promise an fsynced destination file. Keep the
            // original private-only custody and explicitly sync the file and
            // its directory before reporting a durable successful backup.
            let snapshot = aw_platform::open_private_existing(destination, false)?;
            snapshot.sync_all()?;
            aw_platform::sync_parent(destination)?;
            Ok(())
        })();
        if result.is_err() && created_by_this_backup {
            let _ = fs::remove_file(destination);
        }
        let release = if let Some(lease) = maintenance.as_ref() {
            self.release_local_maintenance_lease(lease)
        } else {
            Ok(())
        };
        result?;
        release
    }

    /// Restore a Phase-34 SQLite snapshot of this same store. owner.lock remains
    /// held for the entire operation. Restore is staged first: the staged image
    /// advances restore_generation and fences all pre-restore process/lease state
    /// before the SQLite backup API can atomically make that image live. Thus a
    /// crash during copy leaves the old database, while a crash after DONE leaves
    /// an already-fenced restored database.
    pub fn restore_backup(&self, source: &Path) -> Result<()> {
        let process = self.coordinated_process()?;
        if self.inner.mode != StoreMode::OwnerLock {
            return Err(Error::Denied(
                "database restore requires the authoritative owner/service mode",
            ));
        }
        let source_meta = fs::symlink_metadata(source)?;
        if !source_meta.is_file() || source_meta.file_type().is_symlink() {
            return Err(Error::Denied(
                "restore source must be a regular SQLite backup file",
            ));
        }
        let source_conn = db(Connection::open_with_flags(
            source,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ))?;
        db(source_conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF;"))?;
        let source_version: i64 =
            db(source_conn.pragma_query_value(None, "user_version", |r| r.get(0)))?;
        let source_app: i64 =
            db(source_conn.pragma_query_value(None, "application_id", |r| r.get(0)))?;
        if source_version != LATEST_SCHEMA_VERSION || source_app != APPLICATION_ID {
            return Err(Error::Storage(
                "restore source is not a current aw SQLite snapshot",
            ));
        }
        let source_check: String =
            db(source_conn.query_row("PRAGMA quick_check", [], |r| r.get(0)))?;
        let source_fk: i64 = db(source_conn.query_row(
            "SELECT count(*) FROM pragma_foreign_key_check",
            [],
            |r| r.get(0),
        ))?;
        if source_check != "ok" || source_fk != 0 {
            return Err(Error::Storage(
                "restore source failed SQLite integrity validation",
            ));
        }
        let source_store_id: String = db(source_conn.query_row(
            "SELECT value FROM store_meta WHERE key='store_id'",
            [],
            |r| r.get(0),
        ))?;
        let source_generation: u64 = db(source_conn.query_row(
            "SELECT value FROM store_meta WHERE key='restore_generation'",
            [],
            |r| r.get::<_, String>(0),
        ))?
        .parse()
        .map_err(|_| Error::Storage("invalid restore source generation"))?;
        let live_store_id = {
            let c = self.connection()?;
            db(c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get::<_, String>(0),
            ))?
        };
        if source_store_id != live_store_id {
            return Err(Error::Conflict(
                "restore source belongs to another store; semantic import is required instead",
            ));
        }

        let lease =
            self.acquire_local_maintenance_lease("restore", Duration::from_secs(30 * 60))?;
        let previous_live_generation = process.store_generation;
        let staged = (|| -> Result<(Connection, coordination::ProcessIdentity)> {
            let mut staged = db(Connection::open_in_memory())?;
            copy_sqlite_backup(&source_conn, &mut staged, "restore staging")?;
            let next = coordination::prepare_restore_snapshot(
                &mut staged,
                &process,
                previous_live_generation,
                source_generation,
            )?;
            let check: String = db(staged.query_row("PRAGMA quick_check", [], |r| r.get(0)))?;
            let fk: i64 = db(staged.query_row(
                "SELECT count(*) FROM pragma_foreign_key_check",
                [],
                |r| r.get(0),
            ))?;
            if check != "ok" || fk != 0 {
                return Err(Error::Storage(
                    "staged restored SQLite database failed integrity validation",
                ));
            }
            Ok((staged, next))
        })();

        let (staged, next) = match staged {
            Ok(value) => value,
            Err(error) => {
                let _ = self.release_local_maintenance_lease(&lease);
                return Err(error);
            }
        };

        let copy_result = (|| -> Result<()> {
            let mut destination = self
                .inner
                .conn
                .lock()
                .map_err(|_| Error::Storage("store lock poisoned; restart and inspect"))?;
            copy_sqlite_backup(&staged, &mut destination, "restore")?;
            Ok(())
        })();
        if let Err(error) = copy_result {
            // sqlite3_backup_finish rolls an incomplete destination copy back.
            let _ = self.release_local_maintenance_lease(&lease);
            return Err(error);
        }

        phase34_after_restore_commit_crash_hook();
        let mut slot = self.inner.process.lock().map_err(|_| {
            Error::Storage("local process identity lock poisoned; restart and inspect")
        })?;
        *slot = Some(next);
        // AW-P6-10N: the restored SQLite image already reset the old
        // maintenance lease to idle while fencing the old process generation.
        // Release only the corresponding ORIGINAL file-vault fence now that
        // the replacement authoritative Process identity is installed.
        drop(slot);
        let mut fence = self.inner.vault_maintenance.lock().map_err(|_| {
            Error::Storage("owner vault maintenance fence poisoned; restart and inspect")
        })?;
        if !fence.as_ref().is_some_and(|(current, _)| current == &lease) {
            return Err(Error::Denied(
                "restore vault fence changed during owner generation transition",
            ));
        }
        fence.take();
        Ok(())
    }

    pub fn context_info(
        &self,
        actor: &PrincipalId,
        session: &str,
        branch: &str,
    ) -> Result<ContextInfo> {
        let (view, _) = self.activate(actor, session, branch)?;
        let c =
            Conversation::from_history(view.session.binding.system_prompt.clone(), &view.history)?;
        let completed = view
            .history
            .turns
            .iter()
            .filter(|t| t.outcome == Outcome::Completed);
        let bytes = view.session.binding.system_prompt.len()
            + view.history.summary.as_ref().map_or(0, |s| s.len() + 80)
            + view
                .history
                .context_blocks
                .iter()
                .map(|b| b.message(0).context_bytes())
                .sum::<usize>()
            + completed
                .map(|t| {
                    if t.context_messages.is_empty() {
                        t.prompt.len() + t.answer.len()
                    } else {
                        t.context_messages
                            .iter()
                            .map(aw_agent_core::Message::context_bytes)
                            .sum()
                    }
                })
                .sum::<usize>();
        let _ = c; // restoration validation above is intentional, no provider request.
        Ok(ContextInfo {
            active_turns: view.history.turns.len(),
            completed_turns: view
                .history
                .turns
                .iter()
                .filter(|t| t.outcome == Outcome::Completed)
                .count(),
            context_bytes: bytes,
            history_bytes: json(&view.history)?.len(),
            checkpoint: view.history.summary.is_some(),
            budget_bytes: aw_agent_core::limits::MAX_CONTEXT_BYTES,
        })
    }
}

fn copy_sqlite_backup(
    from: &Connection,
    to: &mut Connection,
    operation: &'static str,
) -> Result<()> {
    use rusqlite::backup::{Backup, StepResult};
    let backup = db(Backup::new(from, to))?;
    let mut blocked_steps = 0u32;
    loop {
        match db(backup.step(256))? {
            StepResult::Done => return Ok(()),
            StepResult::More => blocked_steps = 0,
            StepResult::Busy | StepResult::Locked => {
                blocked_steps = blocked_steps.saturating_add(1);
                if blocked_steps > 100 {
                    return Err(match operation {
                        "restore" => Error::Conflict(
                            "SQLite restore stayed busy; no external effect was replayed",
                        ),
                        _ => Error::Conflict(
                            "SQLite backup copy stayed busy; no external effect was replayed",
                        ),
                    });
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                return Err(Error::Storage(
                    "unsupported SQLite backup step state during bounded copy",
                ));
            }
        }
    }
}

#[cfg(test)]
fn phase34_after_restore_commit_crash_hook() {
    let Ok(path) = std::env::var("AW_PHASE34_RESTORE_COMMIT_READY") else {
        return;
    };
    let _ = std::fs::write(path, b"restore_committed");
    std::thread::sleep(Duration::from_secs(60));
}
#[cfg(not(test))]
fn phase34_after_restore_commit_crash_hook() {}

fn initialize(c: &mut Connection) -> Result<()> {
    configure_connection(c)?;
    let version: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    let app: i64 = db(c.pragma_query_value(None, "application_id", |r| r.get(0)))?;
    if version == 0 {
        let count: i64 = db(c.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        ))?;
        if count != 0 || app != 0 {
            return Err(Error::Storage(
                "not an empty aw database; refusing migration",
            ));
        }
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        db(tx.execute_batch(SCHEMA))?;
        db(tx.pragma_update(None, "application_id", APPLICATION_ID))?;
        db(tx.commit())?;
    } else if !matches!(version, 1..=LATEST_SCHEMA_VERSION) || app != APPLICATION_ID {
        return Err(Error::Storage(
            "unsupported schema/application ID; preserve database and upgrade explicitly",
        ));
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 1 {
        migrate_context(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 2 {
        migrate_durable_runtime(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 3 {
        migrate_domains_workspaces(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 4 {
        migrate_peer_remote_sessions(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 5 {
        migrate_fleet_operations(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 6 {
        migrate_federation_leases(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 7 {
        migrate_semantic_sync(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 8 {
        migrate_remote_session_execution(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 9 {
        migrate_semantic_materialization(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 10 {
        migrate_domain_authority(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 11 {
        migrate_domain_authority_replica(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 12 {
        migrate_workspace_writer(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 13 {
        migrate_workspace_replica(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 14 {
        migrate_identity_continuity(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 15 {
        migrate_auth_secret_store(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 16 {
        migrate_oidc_oauth(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 17 {
        migrate_materialized_context(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 18 {
        migrate_sqlite_coordination(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 19 {
        migrate_trusted_local_direct(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 20 {
        migrate_multiplayer_drafts(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 21 {
        migrate_pipelines(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 22 {
        migrate_coordination_shape_repair(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 23 {
        migrate_routing_affinity(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 24 {
        migrate_subagents(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 25 {
        migrate_subagent_execution_bindings(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 26 {
        migrate_delegation_budgets(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 27 {
        migrate_recursive_subagents(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 28 {
        migrate_child_routing(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 29 {
        migrate_team_topology(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 30 {
        migrate_planner_dags(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 31 {
        migrate_guardian_reviews(c)?;
    }
    let current: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if current == 32 {
        migrate_pipeline_schedules(c)?;
    }
    let check: String = db(c.query_row("PRAGMA quick_check", [], |r| r.get(0)))?;
    if check != "ok" {
        return Err(Error::Storage("SQLite integrity check failed"));
    }
    Ok(())
}

fn table_has_column(c: &Connection, table: &str, column: &str) -> Result<bool> {
    let pragma = match table {
        "runs" => "pragma_table_info('runs')",
        "context_operations" => "pragma_table_info('context_operations')",
        "web_requests" => "pragma_table_info('web_requests')",
        _ => {
            return Err(Error::Config(
                "unsupported coordination repair table".into(),
            ))
        }
    };
    db(c.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {pragma} WHERE name=?1)"),
        [column],
        |r| r.get(0),
    ))
}

fn add_coordination_column_if_missing(
    c: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    if table_has_column(c, table, column)? {
        return Ok(());
    }
    let statement = format!("ALTER TABLE {table} ADD COLUMN {column} {definition}");
    db(c.execute_batch(&statement))?;
    Ok(())
}

fn migrate_pipeline_schedules(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let lease_changed = db(tx.execute(
        "UPDATE migration_lease
         SET holder_client_id='phase67-pipeline-schedules',generation=generation+1,
             target_epoch=33,lease_expires_at=?1,updated_at=?2
         WHERE singleton=1 AND holder_client_id IS NULL",
        params![now().saturating_add(60_000_000), now()],
    ))?;
    if lease_changed != 1 {
        return Err(Error::Conflict(
            "another migration lease is active; schema upgrade is excluded",
        ));
    }
    db(tx.execute_batch(PIPELINE_SCHEDULE_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "pipeline-schedule migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_guardian_reviews(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let lease_changed = db(tx.execute(
        "UPDATE migration_lease
         SET holder_client_id='phase50-guardian-reviews',generation=generation+1,
             target_epoch=32,lease_expires_at=?1,updated_at=?2
         WHERE singleton=1 AND holder_client_id IS NULL",
        params![now().saturating_add(60_000_000), now()],
    ))?;
    if lease_changed != 1 {
        return Err(Error::Conflict(
            "another migration lease is active; schema upgrade is excluded",
        ));
    }
    db(tx.execute_batch(GUARDIAN_REVIEW_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "guardian-review migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_planner_dags(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let lease_changed = db(tx.execute(
        "UPDATE migration_lease
         SET holder_client_id='phase49-planner-dags',generation=generation+1,
             target_epoch=31,lease_expires_at=?1,updated_at=?2
         WHERE singleton=1 AND holder_client_id IS NULL",
        params![now().saturating_add(60_000_000), now()],
    ))?;
    if lease_changed != 1 {
        return Err(Error::Conflict(
            "another migration lease is active; schema upgrade is excluded",
        ));
    }
    db(tx.execute_batch(PLANNER_DAGS_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "planner-dag migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_team_topology(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let lease_changed = db(tx.execute(
        "UPDATE migration_lease
         SET holder_client_id='phase46-team-topology',generation=generation+1,
             target_epoch=30,lease_expires_at=?1,updated_at=?2
         WHERE singleton=1 AND holder_client_id IS NULL",
        params![now().saturating_add(60_000_000), now()],
    ))?;
    if lease_changed != 1 {
        return Err(Error::Conflict(
            "another migration lease is active; schema upgrade is excluded",
        ));
    }
    db(tx.execute_batch(TEAM_TOPOLOGY_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "team-topology migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_child_routing(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let lease_changed = db(tx.execute(
        "UPDATE migration_lease
         SET holder_client_id='phase45-child-routing',generation=generation+1,
             target_epoch=29,lease_expires_at=?1,updated_at=?2
         WHERE singleton=1 AND holder_client_id IS NULL",
        params![now().saturating_add(60_000_000), now()],
    ))?;
    if lease_changed != 1 {
        return Err(Error::Conflict(
            "another migration lease is active; schema upgrade is excluded",
        ));
    }
    db(tx.execute_batch(CHILD_ROUTING_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "child-routing migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_recursive_subagents(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let lease_changed = db(tx.execute(
        "UPDATE migration_lease
         SET holder_client_id='phase44-recursive-subagents',generation=generation+1,
             target_epoch=28,lease_expires_at=?1,updated_at=?2
         WHERE singleton=1 AND holder_client_id IS NULL",
        params![now().saturating_add(60_000_000), now()],
    ))?;
    if lease_changed != 1 {
        return Err(Error::Conflict(
            "another migration lease is active; schema upgrade is excluded",
        ));
    }
    db(tx.execute_batch(RECURSIVE_SUBAGENT_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "recursive-subagent migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_delegation_budgets(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let lease_changed = db(tx.execute(
        "UPDATE migration_lease
         SET holder_client_id='phase43-delegation-budgets',generation=generation+1,
             target_epoch=27,lease_expires_at=?1,updated_at=?2
         WHERE singleton=1 AND holder_client_id IS NULL",
        params![now().saturating_add(60_000_000), now()],
    ))?;
    if lease_changed != 1 {
        return Err(Error::Conflict(
            "another migration lease is active; schema upgrade is excluded",
        ));
    }
    db(tx.execute_batch(DELEGATION_BUDGET_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "delegation-budget migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_subagent_execution_bindings(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let lease_changed = db(tx.execute(
        "UPDATE migration_lease
         SET holder_client_id='phase42-subagent-execution',generation=generation+1,
             target_epoch=26,lease_expires_at=?1,updated_at=?2
         WHERE singleton=1 AND holder_client_id IS NULL",
        params![now().saturating_add(60_000_000), now()],
    ))?;
    if lease_changed != 1 {
        return Err(Error::Conflict(
            "another migration lease is active; schema upgrade is excluded",
        ));
    }
    db(tx.execute_batch(SUBAGENT_EXECUTION_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "subagent-execution migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_subagents(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let lease_changed = db(tx.execute(
        "UPDATE migration_lease
         SET holder_client_id='phase41-subagents',generation=generation+1,
             target_epoch=25,lease_expires_at=?1,updated_at=?2
         WHERE singleton=1 AND holder_client_id IS NULL",
        params![now().saturating_add(60_000_000), now()],
    ))?;
    if lease_changed != 1 {
        return Err(Error::Conflict(
            "another migration lease is active; schema upgrade is excluded",
        ));
    }
    db(tx.execute_batch(SUBAGENT_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "subagent migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_routing_affinity(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let lease_changed = db(tx.execute(
        "UPDATE migration_lease
         SET holder_client_id='phase40-routing-affinity',generation=generation+1,
             target_epoch=24,lease_expires_at=?1,updated_at=?2
         WHERE singleton=1 AND holder_client_id IS NULL",
        params![now().saturating_add(60_000_000), now()],
    ))?;
    if lease_changed != 1 {
        return Err(Error::Conflict(
            "another migration lease is active; schema upgrade is excluded",
        ));
    }
    db(tx.execute_batch(ROUTING_AFFINITY_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "routing-affinity migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_coordination_shape_repair(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let lease_changed = db(tx.execute(
        "UPDATE migration_lease
         SET holder_client_id='post39-coordination-repair',generation=generation+1,
             target_epoch=23,lease_expires_at=?1,updated_at=?2
         WHERE singleton=1 AND holder_client_id IS NULL",
        params![now().saturating_add(60_000_000), now()],
    ))?;
    if lease_changed != 1 {
        return Err(Error::Conflict(
            "another migration lease is active; schema upgrade is excluded",
        ));
    }

    for table in ["runs", "context_operations", "web_requests"] {
        add_coordination_column_if_missing(
            &tx,
            table,
            "owner_process_id",
            "TEXT REFERENCES local_process_incarnations(process_id)",
        )?;
        add_coordination_column_if_missing(
            &tx,
            table,
            "owner_store_generation",
            "INTEGER CHECK(owner_store_generation IS NULL OR owner_store_generation>0)",
        )?;
    }

    db(tx.execute_batch(COORDINATION_SHAPE_REPAIR_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "coordination-shape repair foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_pipelines(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let lease_changed = db(tx.execute(
        "UPDATE migration_lease SET holder_client_id='phase39-owner-lock',generation=generation+1,target_epoch=22,lease_expires_at=?1,updated_at=?2 WHERE singleton=1 AND holder_client_id IS NULL",
        params![now().saturating_add(60_000_000), now()],
    ))?;
    if lease_changed != 1 {
        return Err(Error::Conflict(
            "another migration lease is active; schema upgrade is excluded",
        ));
    }
    db(tx.execute_batch(PIPELINE_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "pipeline migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_multiplayer_drafts(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let lease_changed = db(tx.execute(
        "UPDATE migration_lease SET holder_client_id='phase38-owner-lock',generation=generation+1,target_epoch=21,lease_expires_at=?1,updated_at=?2 WHERE singleton=1 AND holder_client_id IS NULL",
        params![now().saturating_add(60_000_000), now()],
    ))?;
    if lease_changed != 1 {
        return Err(Error::Conflict(
            "another migration lease is active; schema upgrade is excluded",
        ));
    }
    db(tx.execute_batch(MULTIPLAYER_DRAFT_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "multiplayer-draft migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_trusted_local_direct(c: &mut Connection) -> Result<()> {
    // The migration is reached only through SessionStore::open while owner.lock
    // is held. Recovering old running rows happens after process registration;
    // direct clients never migrate a store.
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let lease_changed = db(tx.execute(
        "UPDATE migration_lease
         SET holder_client_id='phase35-owner-lock',generation=generation+1,
             target_epoch=20,lease_expires_at=?1,updated_at=?2
         WHERE singleton=1 AND holder_client_id IS NULL",
        params![now().saturating_add(60_000_000), now()],
    ))?;
    if lease_changed != 1 {
        return Err(Error::Conflict(
            "another migration lease is active; schema upgrade is excluded",
        ));
    }
    db(tx.execute_batch(TRUSTED_LOCAL_DIRECT_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "trusted-local-direct migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_sqlite_coordination(c: &mut Connection) -> Result<()> {
    // v18 still has the Phase-12 placeholder coordination tables. Production
    // reaches this point only while owner.lock is held. Refuse to reinterpret
    // any dormant prototype runtime/lease rows as safe Phase-34 authority.
    let active_legacy: i64 = db(c.query_row(
        "SELECT
           (SELECT count(*) FROM client_runtimes WHERE stopped_at IS NULL) +
           (SELECT count(*) FROM execution_leases WHERE state IN ('active','hazard'))",
        [],
        |r| r.get(0),
    ))?;
    if active_legacy != 0 {
        return Err(Error::Storage(
            "Phase-34 migration found active legacy coordination rows; preserve the database and inspect them before upgrade",
        ));
    }
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let lease_changed = db(tx.execute(
        "UPDATE migration_lease
         SET holder_client_id='phase34-owner-lock',generation=generation+1,
             target_epoch=19,lease_expires_at=?1,updated_at=?2
         WHERE singleton=1 AND holder_client_id IS NULL",
        params![now().saturating_add(60_000_000), now()],
    ))?;
    if lease_changed != 1 {
        return Err(Error::Conflict(
            "another migration lease is active; schema upgrade is excluded",
        ));
    }
    db(tx.execute_batch(SQLITE_COORDINATION_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "SQLite coordination migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_materialized_context(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    db(tx.execute_batch(MATERIALIZED_CONTEXT_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "materialized-context migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_oidc_oauth(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    db(tx.execute_batch(OIDC_OAUTH_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "OIDC/OAuth migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_auth_secret_store(c: &mut Connection) -> Result<()> {
    let duplicate_refresh_hash: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM auth_sessions GROUP BY refresh_token_hash HAVING count(*)>1 LIMIT 1)",
        [],
        |r| r.get(0),
    ))?;
    if duplicate_refresh_hash {
        return Err(Error::Storage(
            "auth/SecretStore migration found duplicate refresh-token hashes; preserve the database and resolve the ambiguous dormant sessions before upgrade",
        ));
    }
    let duplicate_secret_ref: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM provider_accounts WHERE credential_custody='server_secret' AND secret_ref IS NOT NULL GROUP BY secret_ref HAVING count(*)>1 LIMIT 1)",
        [],
        |r| r.get(0),
    ))?;
    if duplicate_secret_ref {
        return Err(Error::Storage(
            "auth/SecretStore migration found duplicate provider secret refs; preserve the database and resolve the ambiguous metadata before upgrade",
        ));
    }
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    db(tx.execute_batch(AUTH_SECRET_STORE_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "auth/SecretStore migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_identity_continuity(c: &mut Connection) -> Result<()> {
    let duplicate_device_key: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM user_devices WHERE public_key IS NOT NULL AND status='active' GROUP BY public_key HAVING count(*)>1 LIMIT 1)",
        [],
        |r| r.get(0),
    ))?;
    if duplicate_device_key {
        return Err(Error::Storage(
            "identity continuity migration found duplicate active device public keys; resolve the ambiguous bindings before upgrade",
        ));
    }
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    db(tx.execute_batch(IDENTITY_CONTINUITY_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "identity continuity migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_workspace_replica(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    db(tx.execute_batch(WORKSPACE_REPLICA_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "Workspace replica migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_workspace_writer(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    db(tx.execute_batch(WORKSPACE_WRITER_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "Workspace writer migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_domain_authority_replica(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    db(tx.execute_batch(DOMAIN_AUTHORITY_REPLICA_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "Domain authority replica migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_domain_authority(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    db(tx.execute_batch(DOMAIN_AUTHORITY_SCHEMA))?;
    authority::backfill_domain_authority(&tx)?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "Domain authority migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_semantic_materialization(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    db(tx.execute_batch(SEMANTIC_MATERIALIZATION_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "semantic materialization migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_remote_session_execution(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    db(tx.execute_batch(REMOTE_SESSION_EXECUTION_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "remote Session execution migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_semantic_sync(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    db(tx.execute_batch(SEMANTIC_SYNC_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "semantic sync migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_federation_leases(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    db(tx.execute_batch(FEDERATION_LEASE_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "federation lease migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_fleet_operations(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    db(tx.execute_batch(FLEET_OPERATION_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "Fleet operation migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_peer_remote_sessions(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    db(tx.execute_batch(PEER_REMOTE_SESSION_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "peer/remote Session migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn migrate_domains_workspaces(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    db(tx.execute_batch(DOMAIN_WORKSPACE_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "Domain/Workspace migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}
fn migrate_durable_runtime(c: &mut Connection) -> Result<()> {
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    db(tx.execute_batch(DURABLE_RUNTIME_SCHEMA))?;
    let violations: i64 = db(tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |r| r.get(0),
    ))?;
    if violations != 0 {
        return Err(Error::Storage(
            "durable runtime migration foreign-key check failed; old transaction retained",
        ));
    }
    db(tx.commit())?;
    Ok(())
}
fn validate_binding(b: &SessionBinding) -> Result<()> {
    if !b.working_directory.is_absolute() || !b.tool_root.is_absolute() {
        return Err(Error::Config(
            "stored execution paths must be absolute".into(),
        ));
    }
    if b.system_prompt.len() > aw_agent_core::limits::MAX_PROMPT_BYTES {
        return Err(Error::Limit("system prompt too large"));
    }
    label(&b.profile_hint, 256)?;
    label(&b.model_hint, 256)?;
    if b.destination_fingerprint.len() != 64
        || !b
            .destination_fingerprint
            .bytes()
            .all(|x| x.is_ascii_hexdigit())
    {
        return Err(Error::Config("invalid destination fingerprint".into()));
    }
    Ok(())
}
pub(crate) fn insert_node(
    c: &Connection,
    session: &str,
    node: &str,
    parent: Option<&str>,
    kind: &str,
    actor: &PrincipalId,
    payload: serde_json::Value,
) -> Result<()> {
    db(c.execute("INSERT INTO nodes(id,session_id,parent_id,kind,actor,payload,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![node,session,parent,kind,actor.as_str(),json(&payload)?,now()]))?;
    Ok(())
}
pub(crate) fn create_in(
    c: &Connection,
    owner: &PrincipalId,
    title: &str,
    b: &SessionBinding,
) -> Result<(SessionInfo, BranchInfo)> {
    let user = ensure_user_for_principal(c, owner)?;
    let realm: String = db(c.query_row(
        "SELECT realm_id FROM users WHERE user_id=?1",
        [&user],
        |r| r.get(0),
    ))?;
    let (_, workspace) = resources::ensure_default_domain_workspace(c, &user, &realm)?;
    resources::create_in_workspace_tx(c, owner, title, b, &workspace, None)
}
pub(crate) fn session_in(c: &Connection, id: &str) -> Result<SessionInfo> {
    let row=db(c.query_row("SELECT owner,title,binding_json,root_node,next_turn,created_at,updated_at,workspace_id FROM sessions WHERE id=?1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,u64>(4)?,r.get::<_,i64>(5)?,r.get::<_,i64>(6)?,r.get::<_,Option<String>>(7)?))).optional())?;
    let Some((owner, title, binding, root_node, next_turn, created_at, updated_at, workspace_id)) =
        row
    else {
        return Err(Error::Denied("session unavailable or not permitted"));
    };
    let imported: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM audit WHERE session_id=?1 AND action='session.imported')",
        [id],
        |r| r.get(0),
    ))?;
    Ok(SessionInfo {
        id: id.into(),
        owner: PrincipalId::new(owner)?,
        title,
        imported,
        workspace_id: workspace_id.ok_or(Error::Storage("session missing workspace binding"))?,
        binding: decode(&binding)?,
        root_node,
        next_turn,
        created_at,
        updated_at,
    })
}
pub(crate) fn branch_in(c: &Connection, session: &str, name_or_id: &str) -> Result<BranchInfo> {
    db(c.query_row("SELECT id,session_id,name,head,revision,active_run FROM branches WHERE session_id=?1 AND (id=?2 OR name=?2) ORDER BY CASE WHEN id=?2 THEN 0 ELSE 1 END LIMIT 1",params![session,name_or_id],|r|Ok(BranchInfo{id:r.get(0)?,session_id:r.get(1)?,name:r.get(2)?,head:r.get(3)?,revision:r.get(4)?,active_run:r.get(5)?})).optional())?.ok_or(Error::Denied("branch unavailable or not permitted"))
}
pub(crate) fn node_in(c: &Connection, session: &str, id: &str) -> Result<Node> {
    let (parent, kind, actor, payload, created) = db(c.query_row(
        "SELECT parent_id,kind,actor,payload,created_at FROM nodes WHERE session_id=?1 AND id=?2",
        params![session, id],
        |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
            ))
        },
    ))?;
    Ok(Node {
        id: id.into(),
        parent_id: parent,
        kind,
        actor: PrincipalId::new(actor)?,
        payload: decode(&payload)?,
        created_at: created,
    })
}
pub(crate) fn nodes_in(c: &Connection, session: &str) -> Result<Vec<Node>> {
    let (count, bytes): (i64, i64) = db(c.query_row(
        "SELECT count(*),coalesce(sum(length(payload)),0) FROM nodes WHERE session_id=?1",
        [session],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ))?;
    if count > MAX_SESSION_NODES as i64 || bytes > MAX_EXPORT_BYTES as i64 {
        return Err(Error::Limit(
            "session tree exceeds current bounded export/view limit",
        ));
    }
    let mut q = db(c.prepare("SELECT id FROM nodes WHERE session_id=?1 ORDER BY seq"))?;
    let ids = db(db(q.query_map([session], |r| r.get::<_, String>(0)))?
        .collect::<rusqlite::Result<Vec<_>>>())?;
    ids.iter().map(|n| node_in(c, session, n)).collect()
}
fn read_branch_view_in(
    c: &Connection,
    actor: &PrincipalId,
    session: &str,
    branch: &str,
    exact_branch_id: bool,
) -> Result<SessionView> {
    authorize(c, session, actor, false)?;
    let s = session_in(c, session)?;
    let b = branch_in(c, session, branch)?;
    if exact_branch_id && b.id != branch {
        return Err(Error::Denied("pinned view requires an exact branch ID"));
    }
    context::authorize_node(c, session, &b.head, actor)?;
    if b.active_run.is_some() {
        return Err(Error::Conflict(
            "branch has an active or unreconciled owner run",
        ));
    }
    let history = history_in(c, &s, &b)?;
    Conversation::from_history(s.binding.system_prompt.clone(), &history)?;
    Ok(SessionView {
        session: s,
        branch: b,
        history,
    })
}

pub(crate) fn history_in(c: &Connection, s: &SessionInfo, b: &BranchInfo) -> Result<HistoryState> {
    context::projected_history(c, s, b)
}
pub(crate) fn private_new(path: &Path) -> Result<File> {
    aw_platform::open_private_new(path).map_err(Into::into)
}

/// SQLite's supported rebuild procedure. Foreign keys are restored after both
/// success and rollback; unknown application/schema versions never enter here.
fn migrate_context(c: &mut Connection) -> Result<()> {
    db(c.execute_batch("PRAGMA foreign_keys=OFF;"))?;
    let result = (|| {
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        db(tx.execute_batch(CONTEXT_SCHEMA))?;
        let violations: i64 = db(tx.query_row(
            "SELECT count(*) FROM pragma_foreign_key_check",
            [],
            |r| r.get(0),
        ))?;
        if violations != 0 {
            return Err(Error::Storage(
                "context migration foreign-key check failed; old transaction retained",
            ));
        }
        db(tx.commit())?;
        Ok(())
    })();
    let restore = db(c.execute_batch("PRAGMA foreign_keys=ON;"));
    result?;
    restore?;
    Ok(())
}

#[cfg(test)]
mod session_view_regressions {
    use super::*;

    fn binding() -> SessionBinding {
        SessionBinding {
            working_directory: PathBuf::from("/fixture"),
            tool_root: PathBuf::from("/fixture"),
            system_prompt: String::new(),
            profile_hint: "fixture".into(),
            model_hint: "m".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    fn read_fixture() -> (SessionStore, PrincipalId, SessionInfo, BranchInfo) {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("view-owner").unwrap();
        let (session, branch) = store.create(&actor, "same title", binding()).unwrap();
        (store, actor, session, branch)
    }

    #[test]
    fn branch_view_requires_exact_scoped_ids_and_keeps_activation_name_compatibility() {
        let (store, actor, session, branch) = read_fixture();
        let (other, other_branch) = store.create(&actor, "same title", binding()).unwrap();
        let fork = store
            .fork(&actor, &session.id, "review", &session.root_node)
            .unwrap();
        assert_eq!(
            store
                .read_branch_view(&actor, &session.id, &branch.id)
                .unwrap()
                .branch
                .id,
            branch.id
        );
        assert_eq!(
            store
                .read_branch_view(&actor, &session.id, &fork.id)
                .unwrap()
                .branch
                .id,
            fork.id
        );
        assert!(store.read_branch_view(&actor, &session.id, "main").is_err());
        assert!(store
            .read_branch_view(&actor, &session.id, "review")
            .is_err());
        assert!(store
            .read_branch_view(&actor, &session.id, &other_branch.id)
            .is_err());
        assert!(store
            .read_branch_view(&actor, &other.id, &branch.id)
            .is_err());
        assert_eq!(
            store
                .activate(&actor, &session.id, "main")
                .unwrap()
                .0
                .branch
                .id,
            branch.id
        );
    }

    #[test]
    fn branch_view_rechecks_read_membership_and_never_creates_effect_intent() {
        let (store, owner, session, branch) = read_fixture();
        let member = PrincipalId::new("view-member").unwrap();
        assert!(store
            .read_branch_view(&member, &session.id, &branch.id)
            .is_err());
        store
            .grant(&owner, &session.id, &member, Some(false))
            .unwrap();
        let counts = || {
            let c = store.connection().unwrap();
            [
                "sessions",
                "branches",
                "nodes",
                "runs",
                "invocation_receipts",
                "commit_log",
            ]
            .iter()
            .map(|table| {
                c.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap()
            })
            .collect::<Vec<_>>()
        };
        let before = counts();
        for _ in 0..3 {
            let view = store
                .read_branch_view(&member, &session.id, &branch.id)
                .unwrap();
            assert_eq!(view.session.id, session.id);
            assert_eq!(view.branch.revision, branch.revision);
            assert!(view.history.turns.is_empty());
        }
        assert_eq!(counts(), before);
        store.grant(&owner, &session.id, &member, None).unwrap();
        assert!(store
            .read_branch_view(&member, &session.id, &branch.id)
            .is_err());
    }

    #[test]
    fn branch_view_preserves_active_run_fence_and_inspectable_unreconciled_history() {
        let (store, actor, session, branch) = read_fixture();
        {
            let c = store.connection().unwrap();
            c.execute(
                "UPDATE branches SET active_run='unreconciled-owner-run' WHERE id=?1",
                [&branch.id],
            )
            .unwrap();
        }
        assert!(matches!(
            store.read_branch_view(&actor, &session.id, &branch.id),
            Err(Error::Conflict(_))
        ));
        assert!(store.branches(&actor, &session.id).unwrap()[0]
            .active_run
            .is_some());
        {
            let c = store.connection().unwrap();
            c.execute(
                "UPDATE branches SET active_run=NULL WHERE id=?1",
                [&branch.id],
            )
            .unwrap();
            c.execute(
                "UPDATE sessions SET import_review_needed=1 WHERE id=?1",
                [&session.id],
            )
            .unwrap();
        }
        assert!(
            store
                .read_branch_view(&actor, &session.id, &branch.id)
                .unwrap()
                .history
                .needs_reconciliation
        );
        assert!(
            store
                .activate(&actor, &session.id, &branch.id)
                .unwrap()
                .0
                .history
                .needs_reconciliation
        );
    }

    #[test]
    fn branch_view_does_not_reinstate_suspended_identity() {
        let (store, actor, session, branch) = read_fixture();
        let user = store.current_user(&actor).unwrap();
        {
            let c = store.connection().unwrap();
            c.execute(
                "UPDATE users SET status='suspended' WHERE user_id=?1",
                [&user.user_id],
            )
            .unwrap();
        }
        assert!(matches!(
            store.read_branch_view(&actor, &session.id, &branch.id),
            Err(Error::Denied(_))
        ));
    }

    #[test]
    fn branch_view_keeps_restored_history_validation() {
        for oversized in [false, true] {
            let (store, actor, session, branch) = read_fixture();
            let turn = aw_agent_core::Turn {
                id: 1,
                prompt: "prompt".into(),
                answer: if oversized {
                    "x".repeat(aw_agent_core::limits::MAX_OUTPUT_BYTES + 1)
                } else {
                    "answer".into()
                },
                model: "fixture".into(),
                profile: if oversized {
                    "fixture".into()
                } else {
                    String::new()
                },
                outcome: aw_agent_core::Outcome::Completed,
                meta: None,
                error: None,
                actor: None,
                tool_records: vec![],
                invocations: vec![],
                context_messages: vec![],
                needs_reconciliation: false,
            };
            {
                let c = store.connection().unwrap();
                c.execute("INSERT INTO nodes(id,session_id,parent_id,kind,actor,payload,created_at) VALUES('view-prompt',?1,?2,'prompt',?3,'{}',1)",
                    params![session.id, session.root_node, actor.as_str()]).unwrap();
                c.execute("INSERT INTO nodes(id,session_id,parent_id,kind,actor,payload,created_at) VALUES('view-turn',?1,'view-prompt','turn',?2,?3,1)",
                    params![session.id, actor.as_str(), json(&turn).unwrap()]).unwrap();
                c.execute(
                    "UPDATE branches SET head='view-turn',revision=revision+1 WHERE id=?1",
                    [&branch.id],
                )
                .unwrap();
                c.execute("UPDATE sessions SET next_turn=2 WHERE id=?1", [&session.id])
                    .unwrap();
            }
            assert!(store
                .read_branch_view(&actor, &session.id, &branch.id)
                .is_err());
            assert!(store.activate(&actor, &session.id, &branch.id).is_err());
        }
    }

    #[test]
    fn legacy_owner_hidden_audit_migrates_to_owner_archive_without_hiding_members() {
        let store = SessionStore::in_memory().unwrap();
        let owner = PrincipalId::new("owner").unwrap();
        let member = PrincipalId::new("member").unwrap();
        let (session, _) = store.create(&owner, "fixture", binding()).unwrap();
        store
            .grant(&owner, &session.id, &member, Some(true))
            .unwrap();

        {
            let mut connection = store.connection().unwrap();
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            audit(
                &tx,
                &session.id,
                &owner,
                "session.hidden",
                serde_json::json!({"visible_in_session_list": false}),
            )
            .unwrap();
            tx.commit().unwrap();
        }

        assert!(store.list(&owner).unwrap().is_empty());
        assert_eq!(store.list_archived(&owner).unwrap().len(), 1);
        assert_eq!(
            store.list(&member).unwrap().len(),
            1,
            "legacy owner presentation state must no longer hide a member's view"
        );
        assert!(store.list_archived(&member).unwrap().is_empty());

        store
            .set_session_archived(&owner, &session.id, false)
            .unwrap();
        assert_eq!(store.list(&owner).unwrap().len(), 1);
        assert!(store.list_archived(&owner).unwrap().is_empty());
    }
}

#[cfg(test)]
mod phase30_authorization_regressions {
    use super::*;

    fn fixture_binding() -> SessionBinding {
        SessionBinding {
            working_directory: PathBuf::from("/fixture"),
            tool_root: PathBuf::from("/fixture"),
            system_prompt: String::new(),
            profile_hint: "fixture".into(),
            model_hint: "m".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    #[test]
    fn stable_projection_blocks_historical_raw_principal_after_user_suspension() {
        let store = SessionStore::in_memory().unwrap();
        let owner = PrincipalId::new("unix:1000").unwrap();
        let (session, _) = store
            .create(&owner, "stable auth fixture", fixture_binding())
            .unwrap();
        let owner_user = store.current_user(&owner).unwrap();
        let stable = stable_user_principal(&owner_user.user_id).unwrap();

        {
            let c = store.connection().unwrap();
            c.execute(
                "UPDATE users SET status='suspended' WHERE user_id=?1",
                [&owner_user.user_id],
            )
            .unwrap();
        }

        assert!(matches!(
            store.session(&owner, &session.id),
            Err(Error::Denied(_))
        ));
        assert!(matches!(
            store.session(&stable, &session.id),
            Err(Error::Denied(_))
        ));
        assert!(store.list(&owner).unwrap().is_empty());
        let c = store.connection().unwrap();
        assert!(ensure_user_for_principal(&c, &owner).is_err());
        let bindings: i64 = c
            .query_row(
                "SELECT count(*) FROM user_identities WHERE external_key=?1",
                [owner.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bindings, 1);
    }

    #[test]
    fn suspended_member_cannot_fall_back_to_historical_member_principal() {
        let store = SessionStore::in_memory().unwrap();
        let owner = PrincipalId::new("unix:1000").unwrap();
        let member = PrincipalId::new("internal:fixture:member").unwrap();
        let (session, _) = store
            .create(&owner, "member auth fixture", fixture_binding())
            .unwrap();
        store
            .grant(&owner, &session.id, &member, Some(true))
            .unwrap();
        assert!(store.session(&member, &session.id).is_ok());
        let member_user = store.current_user(&member).unwrap();

        {
            let c = store.connection().unwrap();
            c.execute(
                "UPDATE users SET status='suspended' WHERE user_id=?1",
                [&member_user.user_id],
            )
            .unwrap();
        }

        assert!(matches!(
            store.session(&member, &session.id),
            Err(Error::Denied(_))
        ));
        assert!(store.list(&member).unwrap().is_empty());
    }
}

#[cfg(test)]
mod phase30_migration_regressions {
    use super::*;

    fn v14_connection() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF;")
            .unwrap();
        for schema in [
            SCHEMA,
            CONTEXT_SCHEMA,
            DURABLE_RUNTIME_SCHEMA,
            DOMAIN_WORKSPACE_SCHEMA,
            PEER_REMOTE_SESSION_SCHEMA,
            FLEET_OPERATION_SCHEMA,
            FEDERATION_LEASE_SCHEMA,
            SEMANTIC_SYNC_SCHEMA,
            REMOTE_SESSION_EXECUTION_SCHEMA,
            SEMANTIC_MATERIALIZATION_SCHEMA,
            DOMAIN_AUTHORITY_SCHEMA,
            DOMAIN_AUTHORITY_REPLICA_SCHEMA,
            WORKSPACE_WRITER_SCHEMA,
            WORKSPACE_REPLICA_SCHEMA,
        ] {
            c.execute_batch(schema).unwrap();
        }
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            14
        );
        c
    }

    #[test]
    fn v14_immutable_context_rows_backfill_to_v15_transactionally() {
        let mut c = v14_connection();
        let tx = c.transaction().unwrap();
        tx.execute(
            "INSERT INTO users(user_id,realm_id,status,display_json,security_epoch,created_at) VALUES('usr_fixture','local','active','{}',1,1)",
            [],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO user_identities(identity_id,user_id,kind,external_key,verified_at,status) VALUES('ident_fixture','usr_fixture','legacy_principal','unix:1000',1,'active')",
            [],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO sessions(id,owner,title,binding_json,root_key,root_node,next_turn,import_review_needed,created_at,updated_at,owner_user_id,workspace_id) VALUES('s_fixture','unix:1000','fixture','{}','rk','n_root',1,0,1,1,'usr_fixture',NULL)",
            [],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO nodes(id,session_id,parent_id,kind,actor,payload,created_at) VALUES('n_root','s_fixture',NULL,'root','unix:1000','{}',1)",
            [],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO context_plans(id,actor,target_session,body,digest,created_at) VALUES('cp_fixture','unix:1000','s_fixture','{}',?1,1)",
            ["0".repeat(64)],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO context_operations(id,actor,request_id,plan_id,fingerprint,turn_id,state,created_at) VALUES('cs_fixture','unix:1000','req','cp_fixture','fp',1,'failed',1)",
            [],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO context_artifacts(id,owner,plan_id,operation_id,body,created_at) VALUES('ca_fixture','unix:1000','cp_fixture',NULL,'{}',1)",
            [],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO context_applications(actor,request_id,fingerprint,artifact_id,result_json,created_at) VALUES('unix:1000','apply','fp','ca_fixture','{}',1)",
            [],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO context_source_grants(session_id,principal,accounts_json,grant_id) VALUES('s_fixture','unix:1000','[]','release_fixture')",
            [],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO context_derivations(node_id,source_session,requester,grant_id) VALUES('n_root','s_fixture','unix:1000','release_fixture')",
            [],
        )
        .unwrap();
        tx.commit().unwrap();

        migrate_identity_continuity(&mut c).unwrap();
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            15
        );
        for (table, column, predicate) in [
            ("context_plans", "actor_user_id", "id='cp_fixture'"),
            ("context_operations", "actor_user_id", "id='cs_fixture'"),
            ("context_artifacts", "owner_user_id", "id='ca_fixture'"),
            (
                "context_applications",
                "actor_user_id",
                "request_id='apply'",
            ),
            (
                "context_source_grants",
                "principal_user_id",
                "grant_id='release_fixture'",
            ),
            (
                "context_derivations",
                "requester_user_id",
                "grant_id='release_fixture'",
            ),
        ] {
            let sql = format!("SELECT {column} FROM {table} WHERE {predicate}");
            let value: String = c.query_row(&sql, [], |r| r.get(0)).unwrap();
            assert_eq!(value, "usr_fixture", "{table}.{column}");
        }

        // Migration restores the original immutable boundaries.
        assert!(c
            .execute(
                "UPDATE context_plans SET actor='unix:2000' WHERE id='cp_fixture'",
                []
            )
            .is_err());
        assert!(c
            .execute(
                "UPDATE context_applications SET actor='unix:2000' WHERE request_id='apply'",
                []
            )
            .is_err());
        assert!(c
            .execute(
                "UPDATE context_operations SET actor_user_id='usr_fixture' WHERE id='cs_fixture'",
                []
            )
            .is_err());
        assert!(c
            .execute(
                "UPDATE context_artifacts SET owner_user_id='usr_fixture' WHERE id='ca_fixture'",
                []
            )
            .is_err());
        assert!(c
            .execute("UPDATE context_derivations SET requester_user_id='usr_fixture' WHERE grant_id='release_fixture'", [])
            .is_err());

        // Legacy replica rows that could not be resolved during migration may
        // learn their stable user projection exactly once from later verified
        // semantic evidence.
        c.execute(
            "INSERT INTO users(user_id,realm_id,status,display_json,security_epoch,created_at) VALUES('usr_remote','remote','active','{}',1,99)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO context_artifacts(id,owner,plan_id,operation_id,body,created_at,owner_user_id) VALUES('ca_remote','unix:2000',NULL,NULL,'{}',2,NULL)",
            [],
        )
        .unwrap();
        c.execute(
            "UPDATE context_artifacts SET owner_user_id='usr_remote' WHERE id='ca_remote'",
            [],
        )
        .unwrap();
        assert!(c
            .execute(
                "UPDATE context_artifacts SET owner_user_id='usr_fixture' WHERE id='ca_remote'",
                [],
            )
            .is_err());

        // Stable requester identity is part of derivation inheritance now.
        c.execute(
            "INSERT INTO nodes(id,session_id,parent_id,kind,actor,payload,created_at) VALUES('n_child','s_fixture','n_root','checkpoint','unix:1000','{}',2)",
            [],
        )
        .unwrap();
        let inherited: String = c
            .query_row(
                "SELECT requester_user_id FROM context_derivations WHERE node_id='n_child' AND grant_id='release_fixture'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(inherited, "usr_fixture");
    }

    #[test]
    fn v15_migration_fails_closed_on_ambiguous_active_device_key() {
        let mut c = v14_connection();
        c.execute(
            "INSERT INTO users(user_id,realm_id,status,display_json,security_epoch,created_at) VALUES('usr_a','local','active','{}',1,1),('usr_b','local','active','{}',1,2)",
            [],
        )
        .unwrap();
        let key = "11".repeat(32);
        c.execute(
            "INSERT INTO user_devices(device_id,user_id,device_kind,public_key,status,capabilities_json,security_epoch,registered_at) VALUES('d_a','usr_a','native',?1,'active','{}',1,1),('d_b','usr_b','native',?1,'active','{}',1,2)",
            [&key],
        )
        .unwrap();
        assert!(matches!(
            migrate_identity_continuity(&mut c),
            Err(Error::Storage(message)) if message.contains("duplicate active device public keys")
        ));
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            14
        );
    }
}

#[cfg(test)]
mod phase31_migration_regressions {
    use super::*;

    fn v15_connection() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF;")
            .unwrap();
        for schema in [
            SCHEMA,
            CONTEXT_SCHEMA,
            DURABLE_RUNTIME_SCHEMA,
            DOMAIN_WORKSPACE_SCHEMA,
            PEER_REMOTE_SESSION_SCHEMA,
            FLEET_OPERATION_SCHEMA,
            FEDERATION_LEASE_SCHEMA,
            SEMANTIC_SYNC_SCHEMA,
            REMOTE_SESSION_EXECUTION_SCHEMA,
            SEMANTIC_MATERIALIZATION_SCHEMA,
            DOMAIN_AUTHORITY_SCHEMA,
            DOMAIN_AUTHORITY_REPLICA_SCHEMA,
            WORKSPACE_WRITER_SCHEMA,
            WORKSPACE_REPLICA_SCHEMA,
        ] {
            c.execute_batch(schema).unwrap();
        }
        migrate_identity_continuity(&mut c).unwrap();
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            15
        );
        c
    }

    fn add_user(c: &Connection) {
        c.execute("INSERT INTO users(user_id,realm_id,status,display_json,security_epoch,created_at) VALUES('usr_phase31','local','active','{}',1,1)",[]).unwrap();
    }

    #[test]
    fn v15_auth_and_provider_metadata_upgrade_fail_closed_to_v16() {
        let mut c = v15_connection();
        add_user(&c);
        c.execute("INSERT INTO auth_sessions(session_id,user_id,credential_id,auth_method,refresh_token_hash,rotation_family_id,issued_at,expires_at,last_seen_at,revoked_at,security_epoch_at_issue,client_json) VALUES('auth_legacy','usr_phase31',NULL,'unix','aa','fam_legacy',10,999,NULL,NULL,1,'{}')",[]).unwrap();
        c.execute("INSERT INTO provider_accounts(provider_account_id,owner_user_id,organization_id,provider_kind,account_owner,payer,credential_custody,executor_id,allowed_models_json,policy_json,status,secret_ref,secret_version,created_at,rotated_at,revoked_at) VALUES('pa_active',NULL,NULL,'openai','server:local','team','server_secret','local','[\"fixture\"]','{}','active','legacy-ref',NULL,10,NULL,NULL)",[]).unwrap();
        c.execute("INSERT INTO provider_accounts(provider_account_id,owner_user_id,organization_id,provider_kind,account_owner,payer,credential_custody,executor_id,allowed_models_json,policy_json,status,secret_ref,secret_version,created_at,rotated_at,revoked_at) VALUES('pa_revoked',NULL,NULL,'openai','server:local','team','server_secret','local','[\"fixture\"]','{}','revoked','legacy-ref-2',0,11,NULL,12)",[]).unwrap();

        migrate_auth_secret_store(&mut c).unwrap();
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            16
        );
        let revoked_at: Option<i64> = c
            .query_row(
                "SELECT revoked_at FROM auth_sessions WHERE session_id='auth_legacy'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(revoked_at, Some(10));
        let active:(String,u64)=c.query_row("SELECT status,secret_version FROM provider_accounts WHERE provider_account_id='pa_active'",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert_eq!(active, ("unavailable".into(), 1));
        let revoked:(String,u64)=c.query_row("SELECT status,secret_version FROM provider_accounts WHERE provider_account_id='pa_revoked'",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert_eq!(revoked, ("revoked".into(), 1));
        let mut q=c.prepare("SELECT provider_account_id,status FROM provider_secret_versions ORDER BY provider_account_id").unwrap();
        let statuses = q
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            statuses,
            vec![
                ("pa_active".into(), "unavailable".into()),
                ("pa_revoked".into(), "revoked".into())
            ]
        );
    }

    #[test]
    fn v16_migration_refuses_ambiguous_refresh_hashes() {
        let mut c = v15_connection();
        add_user(&c);
        for (sid, fam) in [("auth_a", "fam_a"), ("auth_b", "fam_b")] {
            c.execute("INSERT INTO auth_sessions(session_id,user_id,credential_id,auth_method,refresh_token_hash,rotation_family_id,issued_at,expires_at,last_seen_at,revoked_at,security_epoch_at_issue,client_json) VALUES(?1,'usr_phase31',NULL,'unix','duplicate-hash',?2,10,999,NULL,NULL,1,'{}')",params![sid,fam]).unwrap();
        }
        assert!(
            matches!(migrate_auth_secret_store(&mut c),Err(Error::Storage(message)) if message.contains("duplicate refresh-token hashes"))
        );
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            15
        );
    }

    #[test]
    fn v16_migration_refuses_ambiguous_legacy_secret_refs() {
        let mut c = v15_connection();
        add_user(&c);
        for id in ["pa_a", "pa_b"] {
            c.execute("INSERT INTO provider_accounts(provider_account_id,owner_user_id,organization_id,provider_kind,account_owner,payer,credential_custody,executor_id,allowed_models_json,policy_json,status,secret_ref,secret_version,created_at,rotated_at,revoked_at) VALUES(?1,NULL,NULL,'openai','server:local','team','server_secret','local','[\"fixture\"]','{}','active','duplicate-ref',NULL,10,NULL,NULL)",[id]).unwrap();
        }
        assert!(
            matches!(migrate_auth_secret_store(&mut c),Err(Error::Storage(message)) if message.contains("duplicate provider secret refs"))
        );
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            15
        );
    }
}

#[cfg(test)]
mod phase32_migration_regressions {
    use super::*;

    fn v16_connection() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF;")
            .unwrap();
        for schema in [
            SCHEMA,
            CONTEXT_SCHEMA,
            DURABLE_RUNTIME_SCHEMA,
            DOMAIN_WORKSPACE_SCHEMA,
            PEER_REMOTE_SESSION_SCHEMA,
            FLEET_OPERATION_SCHEMA,
            FEDERATION_LEASE_SCHEMA,
            SEMANTIC_SYNC_SCHEMA,
            REMOTE_SESSION_EXECUTION_SCHEMA,
            SEMANTIC_MATERIALIZATION_SCHEMA,
            DOMAIN_AUTHORITY_SCHEMA,
            DOMAIN_AUTHORITY_REPLICA_SCHEMA,
            WORKSPACE_WRITER_SCHEMA,
            WORKSPACE_REPLICA_SCHEMA,
        ] {
            c.execute_batch(schema).unwrap();
        }
        migrate_identity_continuity(&mut c).unwrap();
        migrate_auth_secret_store(&mut c).unwrap();
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            16
        );
        c
    }

    #[test]
    fn v16_manual_oidc_metadata_does_not_gain_login_authority_on_v17_upgrade() {
        let mut c = v16_connection();
        c.execute(
            "INSERT INTO users(user_id,realm_id,status,display_json,security_epoch,created_at) VALUES('usr_oidc_legacy','local','active','{}',1,1)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO user_identities(identity_id,user_id,kind,provider_id,issuer,subject,external_key,verified_at,status) VALUES('ident_oidc_legacy','usr_oidc_legacy','oidc','legacy','https://issuer.example.invalid','subject','oidc:legacy',1,'active')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO auth_token_families(family_id,user_id,credential_id,portable_binding_id,auth_method,status,current_session_id,security_epoch_at_issue,created_at,expires_at,client_json) VALUES('family_legacy','usr_oidc_legacy',NULL,NULL,'unix','active',NULL,1,1,999,'{}')",
            [],
        )
        .unwrap();

        migrate_oidc_oauth(&mut c).unwrap();
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            17
        );
        let login_verified: Option<i64> = c
            .query_row(
                "SELECT login_verified_at FROM user_identities WHERE identity_id='ident_oidc_legacy'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(login_verified.is_none());
        let oidc_family: Option<String> = c
            .query_row(
                "SELECT oidc_identity_id FROM auth_token_families WHERE family_id='family_legacy'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(oidc_family.is_none());
        for table in [
            "oidc_providers",
            "oidc_provider_secret_versions",
            "oauth_connector_accounts",
            "oauth_connector_client_secret_versions",
            "oauth_connector_token_versions",
        ] {
            let exists: bool = c
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exists, "missing {table}");
        }
    }
}

#[cfg(test)]
mod phase33_migration_regressions {
    use super::*;

    fn v17_connection() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF;")
            .unwrap();
        for schema in [
            SCHEMA,
            CONTEXT_SCHEMA,
            DURABLE_RUNTIME_SCHEMA,
            DOMAIN_WORKSPACE_SCHEMA,
            PEER_REMOTE_SESSION_SCHEMA,
            FLEET_OPERATION_SCHEMA,
            FEDERATION_LEASE_SCHEMA,
            SEMANTIC_SYNC_SCHEMA,
            REMOTE_SESSION_EXECUTION_SCHEMA,
            SEMANTIC_MATERIALIZATION_SCHEMA,
            DOMAIN_AUTHORITY_SCHEMA,
            DOMAIN_AUTHORITY_REPLICA_SCHEMA,
            WORKSPACE_WRITER_SCHEMA,
            WORKSPACE_REPLICA_SCHEMA,
        ] {
            c.execute_batch(schema).unwrap();
        }
        migrate_identity_continuity(&mut c).unwrap();
        migrate_auth_secret_store(&mut c).unwrap();
        migrate_oidc_oauth(&mut c).unwrap();
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            17
        );
        c
    }

    #[test]
    fn v17_context_survives_v18_materialized_context_upgrade() {
        let mut c = v17_connection();
        c.execute(
            "INSERT INTO users(user_id,realm_id,status,display_json,security_epoch,created_at) VALUES('usr_phase33','local','active','{}',1,1)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO user_identities(identity_id,user_id,kind,external_key,verified_at,status) VALUES('ident_phase33','usr_phase33','legacy_principal','unix:1000',1,'active')",
            [],
        )
        .unwrap();
        let store_id: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let machine_id: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='local_machine_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let domain = "dom_phase33".to_owned();
        c.execute(
            "INSERT INTO domains(domain_id,authority_store_id,name,legacy_realm,status,created_at,updated_at) VALUES(?1,?2,'Phase33','local','active',1,1)",
            params![domain,store_id],
        )
        .unwrap();
        c.execute(
            "INSERT INTO domain_memberships(domain_id,user_id,role,status,joined_at) VALUES(?1,'usr_phase33','owner','active',1)",
            [&domain],
        )
        .unwrap();
        let workspace = "ws_phase33".to_owned();
        c.execute(
            "INSERT INTO workspaces(workspace_id,domain_id,name,kind,origin_kind,host_machine_id,created_by_user_id,created_at,updated_at) VALUES(?1,?2,'local','hosted','builtin_local',?3,'usr_phase33',1,1)",
            params![workspace,domain,machine_id],
        )
        .unwrap();
        {
            let tx = c.transaction().unwrap();
            tx.execute(
                "INSERT INTO sessions(id,owner,title,binding_json,root_key,root_node,next_turn,import_review_needed,created_at,updated_at,owner_user_id,workspace_id) VALUES('s_phase33','unix:1000','fixture','{}','rk','n_phase33',1,0,1,1,'usr_phase33',?1)",
                [&workspace],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO nodes(id,session_id,parent_id,kind,actor,payload,created_at) VALUES('n_phase33','s_phase33',NULL,'root','unix:1000','{}',1)",
                [],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO branches(id,session_id,name,head,revision,created_at) VALUES('b_phase33','s_phase33','main','n_phase33',1,1)",
                [],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO context_artifacts(id,owner,plan_id,operation_id,body,created_at,owner_user_id) VALUES('ca_phase33','unix:1000',NULL,NULL,'{}',1,'usr_phase33')",
                [],
            )
            .unwrap();
            tx.commit().unwrap();
        }

        migrate_materialized_context(&mut c).unwrap();
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            18
        );
        let artifact: String = c
            .query_row(
                "SELECT id FROM context_artifacts WHERE id='ca_phase33'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(artifact, "ca_phase33");
        for table in [
            "materialized_context_projections",
            "context_artifact_materialized_facts",
            "node_materialized_facts",
        ] {
            let exists: bool = c
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exists, "missing {table}");
        }
    }
}

#[cfg(test)]
mod phase34_migration_regressions {
    use super::*;

    fn v18_connection() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF;")
            .unwrap();
        for schema in [
            SCHEMA,
            CONTEXT_SCHEMA,
            DURABLE_RUNTIME_SCHEMA,
            DOMAIN_WORKSPACE_SCHEMA,
            PEER_REMOTE_SESSION_SCHEMA,
            FLEET_OPERATION_SCHEMA,
            FEDERATION_LEASE_SCHEMA,
            SEMANTIC_SYNC_SCHEMA,
            REMOTE_SESSION_EXECUTION_SCHEMA,
            SEMANTIC_MATERIALIZATION_SCHEMA,
            DOMAIN_AUTHORITY_SCHEMA,
            DOMAIN_AUTHORITY_REPLICA_SCHEMA,
            WORKSPACE_WRITER_SCHEMA,
            WORKSPACE_REPLICA_SCHEMA,
        ] {
            c.execute_batch(schema).unwrap();
        }
        migrate_identity_continuity(&mut c).unwrap();
        migrate_auth_secret_store(&mut c).unwrap();
        migrate_oidc_oauth(&mut c).unwrap();
        migrate_materialized_context(&mut c).unwrap();
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            18
        );
        c
    }

    #[test]
    fn v18_upgrades_to_generation_fenced_v19_without_rewriting_store_identity() {
        let mut c = v18_connection();
        let store_id: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let restore_generation: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        migrate_sqlite_coordination(&mut c).unwrap();

        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            19
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            store_id
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            restore_generation
        );
        assert_eq!(
            c.query_row(
                "SELECT schema_epoch FROM schema_meta WHERE singleton=1",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            19
        );
        for table in [
            "local_process_incarnations",
            "local_coordination_leases",
            "local_maintenance_lease",
            "local_coordination_events",
        ] {
            let exists: bool = c
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exists, "missing {table}");
        }
    }

    #[test]
    fn v19_migration_refuses_live_phase12_placeholder_authority() {
        let mut c = v18_connection();
        c.execute(
            "INSERT INTO client_runtimes(client_id,principal,process_kind,generation,started_at,last_seen_at,stopped_at)
             VALUES('legacy-live','unix:1000','prototype',1,1,1,NULL)",
            [],
        )
        .unwrap();
        assert!(matches!(
            migrate_sqlite_coordination(&mut c),
            Err(Error::Storage(message)) if message.contains("active legacy coordination rows")
        ));
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            18
        );
    }
}
#[cfg(test)]
mod phase35_migration_regressions {
    use super::*;

    fn v19_connection() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF;")
            .unwrap();
        for schema in [
            SCHEMA,
            CONTEXT_SCHEMA,
            DURABLE_RUNTIME_SCHEMA,
            DOMAIN_WORKSPACE_SCHEMA,
            PEER_REMOTE_SESSION_SCHEMA,
            FLEET_OPERATION_SCHEMA,
            FEDERATION_LEASE_SCHEMA,
            SEMANTIC_SYNC_SCHEMA,
            REMOTE_SESSION_EXECUTION_SCHEMA,
            SEMANTIC_MATERIALIZATION_SCHEMA,
            DOMAIN_AUTHORITY_SCHEMA,
            DOMAIN_AUTHORITY_REPLICA_SCHEMA,
            WORKSPACE_WRITER_SCHEMA,
            WORKSPACE_REPLICA_SCHEMA,
        ] {
            c.execute_batch(schema).unwrap();
        }
        migrate_identity_continuity(&mut c).unwrap();
        migrate_auth_secret_store(&mut c).unwrap();
        migrate_oidc_oauth(&mut c).unwrap();
        migrate_materialized_context(&mut c).unwrap();
        migrate_sqlite_coordination(&mut c).unwrap();
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            19
        );
        c
    }

    #[test]
    fn v19_upgrades_to_v20_with_run_operation_and_web_process_provenance() {
        let mut c = v19_connection();
        let store_id: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let restore_generation: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        migrate_trusted_local_direct(&mut c).unwrap();

        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            20
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            store_id
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            restore_generation
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='coordination_schema'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            "2"
        );
        for (table, column) in [
            ("runs", "owner_process_id"),
            ("runs", "owner_store_generation"),
            ("context_operations", "owner_process_id"),
            ("context_operations", "owner_store_generation"),
            ("web_requests", "owner_process_id"),
            ("web_requests", "owner_store_generation"),
        ] {
            let exists: bool = c
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name=?2)",
                    params![table, column],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exists, "missing {table}.{column}");
        }
    }

    #[test]
    fn v20_upgrades_to_v21_with_shared_draft_tables_without_rewriting_store_identity() {
        let mut c = v19_connection();
        migrate_trusted_local_direct(&mut c).unwrap();
        let store_id: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let restore_generation: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        migrate_multiplayer_drafts(&mut c).unwrap();

        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            21
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='schema_epoch'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            "21"
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            store_id
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            restore_generation
        );
        for table in [
            "session_draft_policies",
            "session_draft_grants",
            "shared_drafts",
            "shared_draft_updates",
            "shared_draft_submissions",
        ] {
            let exists: bool = c
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exists, "missing {table}");
        }
    }

    #[test]
    fn legacy_v22_missing_coordination_columns_repairs_to_v23() {
        let mut c = v19_connection();
        migrate_trusted_local_direct(&mut c).unwrap();
        migrate_multiplayer_drafts(&mut c).unwrap();
        migrate_pipelines(&mut c).unwrap();

        for (index, table) in [
            ("runs_by_local_process", "runs"),
            ("context_operations_by_local_process", "context_operations"),
            ("web_requests_by_local_process", "web_requests"),
        ] {
            c.execute_batch(&format!(
                "DROP INDEX {index};
                 ALTER TABLE {table} DROP COLUMN owner_process_id;
                 ALTER TABLE {table} DROP COLUMN owner_store_generation;"
            ))
            .unwrap();
        }
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            22
        );
        assert!(!table_has_column(&c, "context_operations", "owner_process_id").unwrap());

        migrate_coordination_shape_repair(&mut c).unwrap();

        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            23
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='schema_epoch'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            "23"
        );
        for table in ["runs", "context_operations", "web_requests"] {
            assert!(table_has_column(&c, table, "owner_process_id").unwrap());
            assert!(table_has_column(&c, table, "owner_store_generation").unwrap());
        }
        c.execute(
            "INSERT INTO local_process_incarnations(
                process_id,store_id,store_generation,process_nonce,process_kind,os_pid,state,
                started_at,last_seen_at,lease_expires_at)
             SELECT 'repair-proc',value,1,'nonce','owner_lock',1,'active',1,1,NULL
             FROM store_meta WHERE key='store_id'",
            [],
        )
        .unwrap();
        c.execute(
            "UPDATE context_operations
             SET owner_process_id='repair-proc',owner_store_generation=1
             WHERE state='started'",
            [],
        )
        .unwrap();
    }

    #[test]
    fn v23_upgrades_to_v24_with_routing_tables_without_rewriting_store_identity() {
        let mut c = v19_connection();
        migrate_trusted_local_direct(&mut c).unwrap();
        migrate_multiplayer_drafts(&mut c).unwrap();
        migrate_pipelines(&mut c).unwrap();
        migrate_coordination_shape_repair(&mut c).unwrap();
        let store_id: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let restore_generation: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        migrate_routing_affinity(&mut c).unwrap();

        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            24
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='schema_epoch'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            "24"
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            store_id
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            restore_generation
        );
        for table in [
            "routing_strategies",
            "routing_strategy_revisions",
            "routing_bindings",
            "routing_operations",
            "routing_attempts",
            "routing_affinities",
        ] {
            let exists: bool = c
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exists, "missing {table}");
        }
        for trigger in [
            "routing_strategy_revisions_immutable_update",
            "routing_strategy_revisions_immutable_delete",
        ] {
            let exists: bool = c
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='trigger' AND name=?1)",
                    [trigger],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exists, "missing {trigger}");
        }
    }

    #[test]
    fn v24_upgrades_to_v25_with_subagent_relations_without_rewriting_store_identity() {
        let mut c = v19_connection();
        migrate_trusted_local_direct(&mut c).unwrap();
        migrate_multiplayer_drafts(&mut c).unwrap();
        migrate_pipelines(&mut c).unwrap();
        migrate_coordination_shape_repair(&mut c).unwrap();
        migrate_routing_affinity(&mut c).unwrap();
        let store_id: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let restore_generation: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        migrate_subagents(&mut c).unwrap();

        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            25
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='schema_epoch'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            "25"
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            store_id
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            restore_generation
        );
        let exists: bool = c
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='subagent_sessions')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(exists);
    }

    #[test]
    fn v25_upgrades_to_v26_with_subagent_execution_bindings_without_rewriting_store_identity() {
        let mut c = v19_connection();
        migrate_trusted_local_direct(&mut c).unwrap();
        migrate_multiplayer_drafts(&mut c).unwrap();
        migrate_pipelines(&mut c).unwrap();
        migrate_coordination_shape_repair(&mut c).unwrap();
        migrate_routing_affinity(&mut c).unwrap();
        migrate_subagents(&mut c).unwrap();
        let store_id: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let restore_generation: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        migrate_subagent_execution_bindings(&mut c).unwrap();

        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            26
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='schema_epoch'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            "26"
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            store_id
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            restore_generation
        );
        let exists: bool = c
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='subagent_execution_bindings')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(exists);
    }

    #[test]
    fn v26_upgrades_to_v27_with_delegation_budgets_without_rewriting_store_identity() {
        let mut c = v19_connection();
        migrate_trusted_local_direct(&mut c).unwrap();
        migrate_multiplayer_drafts(&mut c).unwrap();
        migrate_pipelines(&mut c).unwrap();
        migrate_coordination_shape_repair(&mut c).unwrap();
        migrate_routing_affinity(&mut c).unwrap();
        migrate_subagents(&mut c).unwrap();
        migrate_subagent_execution_bindings(&mut c).unwrap();
        let store_id: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let restore_generation: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        migrate_delegation_budgets(&mut c).unwrap();

        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            27
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='schema_epoch'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "27"
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            store_id
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            restore_generation
        );
        for table in [
            "delegation_roots",
            "delegation_child_allocations",
            "delegation_model_attempts",
        ] {
            let exists: bool = c
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exists, "missing {table}");
        }
    }

    #[test]
    fn v27_upgrades_to_v28_with_recursive_depth_defaults_without_rewriting_store_identity() {
        let mut c = v19_connection();
        migrate_trusted_local_direct(&mut c).unwrap();
        migrate_multiplayer_drafts(&mut c).unwrap();
        migrate_pipelines(&mut c).unwrap();
        migrate_coordination_shape_repair(&mut c).unwrap();
        migrate_routing_affinity(&mut c).unwrap();
        migrate_subagents(&mut c).unwrap();
        migrate_subagent_execution_bindings(&mut c).unwrap();
        migrate_delegation_budgets(&mut c).unwrap();
        let store_id: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let restore_generation: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        migrate_recursive_subagents(&mut c).unwrap();

        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            28
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='schema_epoch'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "28"
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            store_id
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            restore_generation
        );
        for (table, column) in [
            ("delegation_roots", "recursive_max_depth"),
            ("delegation_child_allocations", "recursive_depth"),
            ("delegation_child_allocations", "parent_allocation_id"),
            ("subagent_sessions", "recursive_depth"),
        ] {
            let exists: bool = c
                .query_row(
                    &format!(
                        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name=?1)"
                    ),
                    [column],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exists, "missing {table}.{column}");
        }
    }

    #[test]
    fn v28_upgrades_to_v29_with_child_routing_defaults_without_rewriting_store_identity() {
        let mut c = v19_connection();
        migrate_trusted_local_direct(&mut c).unwrap();
        migrate_multiplayer_drafts(&mut c).unwrap();
        migrate_pipelines(&mut c).unwrap();
        migrate_coordination_shape_repair(&mut c).unwrap();
        migrate_routing_affinity(&mut c).unwrap();
        migrate_subagents(&mut c).unwrap();
        migrate_subagent_execution_bindings(&mut c).unwrap();
        migrate_delegation_budgets(&mut c).unwrap();
        migrate_recursive_subagents(&mut c).unwrap();
        let store_id: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let restore_generation: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        migrate_child_routing(&mut c).unwrap();

        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            29
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='schema_epoch'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            "29"
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            store_id
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            restore_generation
        );
        for (table, column) in [
            ("delegation_roots", "child_routing_revision_id"),
            ("delegation_child_allocations", "routing_operation_id"),
            ("delegation_child_allocations", "routing_target_id"),
            ("delegation_child_allocations", "routing_selection_source"),
            ("delegation_child_allocations", "selected_identity_json"),
            ("delegation_child_allocations", "routing_bound_at"),
        ] {
            let exists: bool = c
                .query_row(
                    &format!(
                        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name=?1)"
                    ),
                    [column],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exists, "missing {table}.{column}");
        }
        let attempts: bool = c
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='delegation_routing_attempts')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(attempts);
        let routed_roots: i64 = c
            .query_row(
                "SELECT count(*) FROM delegation_roots WHERE child_routing_revision_id IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(routed_roots, 0);
    }

    #[test]
    fn v29_upgrades_to_v30_with_team_topology_without_rewriting_store_identity() {
        let mut c = v19_connection();
        migrate_trusted_local_direct(&mut c).unwrap();
        migrate_multiplayer_drafts(&mut c).unwrap();
        migrate_pipelines(&mut c).unwrap();
        migrate_coordination_shape_repair(&mut c).unwrap();
        migrate_routing_affinity(&mut c).unwrap();
        migrate_subagents(&mut c).unwrap();
        migrate_subagent_execution_bindings(&mut c).unwrap();
        migrate_delegation_budgets(&mut c).unwrap();
        migrate_recursive_subagents(&mut c).unwrap();
        migrate_child_routing(&mut c).unwrap();
        let store_id: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let restore_generation: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        migrate_team_topology(&mut c).unwrap();

        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            30
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='schema_epoch'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            "30"
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            store_id
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            restore_generation
        );
        for table in [
            "team_templates",
            "team_template_revisions",
            "team_runs",
            "team_run_members",
            "team_join_receipts",
        ] {
            let exists: bool = c
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exists, "missing {table}");
        }
    }

    #[test]
    fn v30_upgrades_to_v31_with_planner_dags_without_rewriting_store_identity() {
        let mut c = v19_connection();
        migrate_trusted_local_direct(&mut c).unwrap();
        migrate_multiplayer_drafts(&mut c).unwrap();
        migrate_pipelines(&mut c).unwrap();
        migrate_coordination_shape_repair(&mut c).unwrap();
        migrate_routing_affinity(&mut c).unwrap();
        migrate_subagents(&mut c).unwrap();
        migrate_subagent_execution_bindings(&mut c).unwrap();
        migrate_delegation_budgets(&mut c).unwrap();
        migrate_recursive_subagents(&mut c).unwrap();
        migrate_child_routing(&mut c).unwrap();
        migrate_team_topology(&mut c).unwrap();
        let store_id: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let restore_generation: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        migrate_planner_dags(&mut c).unwrap();

        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            31
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='schema_epoch'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            "31"
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            store_id
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            restore_generation
        );
        let planner_pin: bool = c
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('delegation_roots') WHERE name='planner_routing_revision_id')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(planner_pin);
        for table in [
            "planner_operations",
            "planner_task_runs",
            "planner_join_receipts",
        ] {
            let exists: bool = c
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exists, "missing {table}");
        }
        let planner_rows: i64 = c
            .query_row("SELECT count(*) FROM planner_operations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(planner_rows, 0);
    }

    #[test]
    fn v31_upgrades_to_v32_with_guardian_reviews_without_rewriting_store_identity() {
        let mut c = v19_connection();
        migrate_trusted_local_direct(&mut c).unwrap();
        migrate_multiplayer_drafts(&mut c).unwrap();
        migrate_pipelines(&mut c).unwrap();
        migrate_coordination_shape_repair(&mut c).unwrap();
        migrate_routing_affinity(&mut c).unwrap();
        migrate_subagents(&mut c).unwrap();
        migrate_subagent_execution_bindings(&mut c).unwrap();
        migrate_delegation_budgets(&mut c).unwrap();
        migrate_recursive_subagents(&mut c).unwrap();
        migrate_child_routing(&mut c).unwrap();
        migrate_team_topology(&mut c).unwrap();
        migrate_planner_dags(&mut c).unwrap();
        let store_id: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let restore_generation: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        migrate_guardian_reviews(&mut c).unwrap();

        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            32
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='schema_epoch'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            "32"
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            store_id
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            restore_generation
        );
        let guardian_pin: bool = c
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('delegation_roots') WHERE name='guardian_routing_revision_id')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(guardian_pin);
        for table in [
            "review_operations",
            "review_human_approvals",
            "review_release_receipts",
        ] {
            let exists: bool = c
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exists, "missing {table}");
        }
    }

    #[test]
    fn v21_upgrades_to_v22_with_pipeline_tables_without_rewriting_store_identity() {
        let mut c = v19_connection();
        migrate_trusted_local_direct(&mut c).unwrap();
        migrate_multiplayer_drafts(&mut c).unwrap();
        let store_id: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let restore_generation: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        migrate_pipelines(&mut c).unwrap();

        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            22
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='schema_epoch'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            "22"
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            store_id
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            restore_generation
        );
        for table in [
            "pipelines",
            "pipeline_revisions",
            "pipeline_bindings",
            "pipeline_runs",
            "pipeline_step_attempts",
            "pipeline_approvals",
        ] {
            let exists: bool = c
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exists, "missing {table}");
        }
        for trigger in [
            "pipeline_revisions_immutable_update",
            "pipeline_revisions_immutable_delete",
        ] {
            let exists: bool = c
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='trigger' AND name=?1)",
                    [trigger],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exists, "missing {trigger}");
        }
    }
}

fn import_portable_session(
    store: &SessionStore,
    owner: &PrincipalId,
    document: aw_store_api::PortableSession,
    binding: SessionBinding,
) -> Result<(
    SessionInfo,
    BranchInfo,
    Arc<dyn aw_store_api::SessionCursor>,
)> {
    if document.version != 1
        || document.title.trim().is_empty()
        || document.title.len() > 160
        || !document.history.context_blocks.is_empty()
    {
        return Err(Error::Config(
            "portable session is unsupported/oversized or contains context artifacts that require aw-tree import".into(),
        ));
    }
    validate_binding(&binding)?;
    Conversation::from_history(binding.system_prompt.clone(), &document.history)?;
    for turn in &document.history.turns {
        types::validate_turn(turn)?;
    }

    let mut c = store.connection()?;
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let (session, mut branch) = create_in(&tx, owner, &document.title, &binding)?;
    let mut head = session.root_node.clone();

    if let Some(summary) = &document.history.summary {
        let checkpoint = id("n")?;
        insert_node(
            &tx,
            &session.id,
            &checkpoint,
            Some(&head),
            "checkpoint",
            owner,
            serde_json::json!({"summary":summary}),
        )?;
        head = checkpoint;
    }

    for turn in &document.history.turns {
        let actor = turn.actor.as_ref().unwrap_or(owner);
        let prompt = id("n")?;
        insert_node(
            &tx,
            &session.id,
            &prompt,
            Some(&head),
            "prompt",
            actor,
            serde_json::json!({
                "turn_id": turn.id,
                "prompt": turn.prompt,
                "imported_unverified": true
            }),
        )?;
        let result = id("n")?;
        insert_node(
            &tx,
            &session.id,
            &result,
            Some(&prompt),
            "turn",
            actor,
            serde_json::to_value(turn).map_err(|_| Error::Storage("invalid portable turn"))?,
        )?;
        head = result;
    }

    let revision = u64::try_from(document.history.turns.len())
        .map_err(|_| Error::Limit("portable session turn count"))?
        .saturating_add(u64::from(document.history.summary.is_some()));
    db(tx.execute(
        "UPDATE branches SET head=?1,revision=?2 WHERE id=?3 AND session_id=?4",
        params![head, revision, branch.id, session.id],
    ))?;
    db(tx.execute(
        "UPDATE sessions SET next_turn=?1,import_review_needed=1,updated_at=?2 WHERE id=?3",
        params![document.history.next_turn_id, now(), session.id],
    ))?;
    audit(
        &tx,
        &session.id,
        owner,
        "session.imported",
        serde_json::json!({
            "format":"portable",
            "source_backend": document.source.backend,
            "source_session": document.source.session_id,
            "source_branch": document.source.branch_id,
            "parent_session": document.source.parent_session
        }),
    )?;
    db(tx.commit())?;
    drop(c);

    let session = store.session(owner, &session.id)?;
    {
        let connection = store.connection()?;
        branch = branch_in(&connection, &session.id, &branch.id)?;
    }
    let (_, handle) = store.activate(owner, &session.id, &branch.id)?;
    Ok((session, branch, Arc::new(handle)))
}

impl aw_store_api::StoreBackend for SessionStore {
    fn descriptor(&self) -> aw_store_api::StoreDescriptor {
        use aw_store_api::{StoreCapabilities, StoreCapability as C, StoreDescriptor, StoreKind};
        StoreDescriptor {
            kind: StoreKind::Sqlite,
            name: "sqlite".into(),
            durable: true,
            capabilities: StoreCapabilities::new([
                C::Sessions,
                C::Branches,
                C::Forks,
                C::Journal,
                C::ArchiveImport,
                C::ArchiveExport,
                C::Checkpoints,
                C::ContextFlow,
                C::EffectRecovery,
                C::Coordination,
                C::MultiProcess,
                C::MultiUser,
                C::SharedProviderAccounts,
                C::Identity,
                C::DistributedResources,
                C::SemanticSync,
                C::BackupRestore,
                C::Migrations,
                C::PrivateStateAuthority,
            ]),
        }
    }
}

impl aw_store_api::SessionCursor for SessionHandle {
    fn session_id(&self) -> &str {
        SessionHandle::session_id(self)
    }

    fn branch(&self) -> Result<BranchInfo> {
        SessionHandle::branch(self)
    }

    fn view(&self) -> Result<SessionView> {
        SessionHandle::view(self)
    }

    fn journal(&self) -> Arc<dyn aw_agent_core::TurnJournal> {
        Arc::new(self.clone())
    }
}

impl aw_store_api::SessionBackend for SessionStore {
    fn create(
        &self,
        owner: &PrincipalId,
        title: &str,
        binding: SessionBinding,
    ) -> Result<(
        SessionInfo,
        BranchInfo,
        Arc<dyn aw_store_api::SessionCursor>,
    )> {
        let (session, branch) = SessionStore::create(self, owner, title, binding)?;
        let (_, handle) = SessionStore::activate(self, owner, &session.id, &branch.id)?;
        Ok((session, branch, Arc::new(handle)))
    }

    fn open(
        &self,
        owner: &PrincipalId,
        session: &str,
        branch: &str,
    ) -> Result<Arc<dyn aw_store_api::SessionCursor>> {
        let (_, handle) = SessionStore::activate(self, owner, session, branch)?;
        Ok(Arc::new(handle))
    }

    fn list(&self, owner: &PrincipalId) -> Result<Vec<SessionInfo>> {
        SessionStore::list(self, owner)
    }

    fn set_title(&self, owner: &PrincipalId, session: &str, title: &str) -> Result<SessionInfo> {
        SessionStore::set_title(self, owner, session, title)
    }

    fn branches(&self, owner: &PrincipalId, session: &str) -> Result<Vec<BranchInfo>> {
        SessionStore::branches(self, owner, session)
    }

    fn fork(
        &self,
        owner: &PrincipalId,
        session: &str,
        branch: &str,
        name: &str,
    ) -> Result<Arc<dyn aw_store_api::SessionCursor>> {
        let current = {
            let connection = self.connection()?;
            branch_in(&connection, session, branch)?
        };
        let next = SessionStore::fork(self, owner, session, name, &current.head)?;
        let (_, handle) = SessionStore::activate(self, owner, session, &next.id)?;
        Ok(Arc::new(handle))
    }

    fn export_document(
        &self,
        owner: &PrincipalId,
        session: &str,
        branch: &str,
    ) -> Result<aw_store_api::PortableSession> {
        let (view, _) = self.activate(owner, session, branch)?;
        Ok(aw_store_api::PortableSession {
            version: 1,
            title: view.session.title,
            cwd: Some(view.session.binding.working_directory.clone()),
            source: aw_store_api::PortableSessionSource {
                backend: aw_store_api::StoreKind::Sqlite,
                session_id: view.session.id,
                branch_id: view.branch.id,
                parent_session: None,
            },
            history: view.history,
        })
    }

    fn import_document(
        &self,
        owner: &PrincipalId,
        document: aw_store_api::PortableSession,
        binding: SessionBinding,
    ) -> Result<(
        SessionInfo,
        BranchInfo,
        Arc<dyn aw_store_api::SessionCursor>,
    )> {
        import_portable_session(self, owner, document, binding)
    }
}
