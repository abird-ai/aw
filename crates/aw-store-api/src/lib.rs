//! Backend-neutral persistence contract.
//!
//! This crate deliberately contains no SQLite, filesystem, PostgreSQL, LevelDB,
//! network or platform-specific code. Backends declare capabilities explicitly;
//! callers must not infer authority/durability from a backend name.

use aw_agent_core::{
    CompactionPlan, HistoryState, PrincipalId, Result, SummaryJournal, TurnJournal,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreCapability {
    Sessions,
    Branches,
    Forks,
    Journal,
    ArchiveImport,
    ArchiveExport,
    Checkpoints,
    ContextFlow,
    EffectRecovery,
    Coordination,
    MultiProcess,
    MultiUser,
    SharedProviderAccounts,
    Identity,
    DistributedResources,
    SemanticSync,
    BackupRestore,
    Migrations,
    /// Backend can safely hold authority-bearing private state/credentials,
    /// not merely user-owned conversation transcripts.
    PrivateStateAuthority,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StoreCapabilities(BTreeSet<StoreCapability>);

impl StoreCapabilities {
    pub fn new(values: impl IntoIterator<Item = StoreCapability>) -> Self {
        Self(values.into_iter().collect())
    }
    pub fn contains(&self, capability: StoreCapability) -> bool {
        self.0.contains(&capability)
    }
    pub fn require(&self, capability: StoreCapability) -> Result<()> {
        if self.contains(capability) {
            Ok(())
        } else {
            Err(aw_agent_core::Error::Denied(
                "selected store backend does not provide this capability",
            ))
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = StoreCapability> + '_ {
        self.0.iter().copied()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreKind {
    Memory,
    Jsonl,
    Sqlite,
    Postgres,
    LevelDb,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreDescriptor {
    pub kind: StoreKind,
    pub name: String,
    pub durable: bool,
    pub capabilities: StoreCapabilities,
}

pub trait StoreBackend: Send + Sync {
    fn descriptor(&self) -> StoreDescriptor;
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionBinding {
    pub working_directory: PathBuf,
    pub tool_root: PathBuf,
    pub system_prompt: String,
    pub profile_hint: String,
    pub model_hint: String,
    /// Digest of endpoint/auth-reference configuration, never a secret.
    pub destination_fingerprint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionInfo {
    pub id: String,
    pub owner: PrincipalId,
    pub title: String,
    #[serde(default)]
    pub imported: bool,
    #[serde(default)]
    pub workspace_id: String,
    pub binding: SessionBinding,
    pub root_node: String,
    pub next_turn: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BranchInfo {
    pub id: String,
    pub session_id: String,
    pub name: String,
    pub head: String,
    pub revision: u64,
    pub active_run: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionView {
    pub session: SessionInfo,
    pub branch: BranchInfo,
    pub history: HistoryState,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableSession {
    pub version: u32,
    pub title: String,
    /// Informational source working directory. Import chooses a fresh trusted
    /// execution binding; this path is never authority by itself.
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    pub source: PortableSessionSource,
    pub history: HistoryState,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableSessionSource {
    pub backend: StoreKind,
    pub session_id: String,
    pub branch_id: String,
    #[serde(default)]
    pub parent_session: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextInfo {
    pub active_turns: usize,
    pub completed_turns: usize,
    pub context_bytes: usize,
    pub history_bytes: usize,
    pub checkpoint: bool,
    pub budget_bytes: usize,
}

/// Backend-selected active branch. The journal is the durability boundary used
/// by the agent; implementations may be transactional or append-only.
pub trait SessionCursor: Send + Sync {
    fn session_id(&self) -> &str;
    fn branch(&self) -> Result<BranchInfo>;
    fn view(&self) -> Result<SessionView>;
    fn journal(&self) -> Arc<dyn TurnJournal>;
    /// Optional backend-owned CAS and paid summary journal for bounded active
    /// projection advances. The default fails closed; full SQLite continues to
    /// use its existing authorized ContextFlow service instead of duplicating it.
    fn compaction_journal(&self, _plan: CompactionPlan) -> Result<Arc<dyn SummaryJournal>> {
        Err(aw_agent_core::Error::Denied(
            "backend does not expose compacted-tail journaling",
        ))
    }
    /// Apply ONLY an already settled successful summary, never regenerate it.
    /// Returns the new active tail; immutable raw history stays in the backend.
    fn apply_compaction(&self, _request_id: &str) -> Result<HistoryState> {
        Err(aw_agent_core::Error::Denied(
            "backend does not expose compacted-tail CAS",
        ))
    }
    /// Inspect a settled-but-unapplied successful summary after process crash
    /// WITHOUT making a new provider request. None means no ready CAS.
    fn compaction_ready(&self) -> Result<Option<String>> {
        Ok(None)
    }
}

/// Common single-user session operations. Richer backends expose extra service
/// APIs separately, guarded by StoreCapabilities.
pub trait SessionBackend: StoreBackend {
    fn create(
        &self,
        owner: &PrincipalId,
        title: &str,
        binding: SessionBinding,
    ) -> Result<(SessionInfo, BranchInfo, Arc<dyn SessionCursor>)>;

    fn open(
        &self,
        owner: &PrincipalId,
        session: &str,
        branch: &str,
    ) -> Result<Arc<dyn SessionCursor>>;

    fn list(&self, owner: &PrincipalId) -> Result<Vec<SessionInfo>>;
    fn set_title(&self, owner: &PrincipalId, session: &str, title: &str) -> Result<SessionInfo>;
    fn branches(&self, owner: &PrincipalId, session: &str) -> Result<Vec<BranchInfo>>;

    fn fork(
        &self,
        owner: &PrincipalId,
        session: &str,
        branch: &str,
        name: &str,
    ) -> Result<Arc<dyn SessionCursor>>;

    fn export_document(
        &self,
        owner: &PrincipalId,
        session: &str,
        branch: &str,
    ) -> Result<PortableSession>;

    fn import_document(
        &self,
        owner: &PrincipalId,
        document: PortableSession,
        binding: SessionBinding,
    ) -> Result<(SessionInfo, BranchInfo, Arc<dyn SessionCursor>)>;
}
