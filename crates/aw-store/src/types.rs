use aw_agent_core::{HistoryState, PrincipalId, Turn};
use serde::{Deserialize, Serialize};

pub use aw_store_api::{BranchInfo, ContextInfo, SessionBinding, SessionInfo, SessionView};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub parent_id: Option<String>,
    pub kind: String,
    pub actor: PrincipalId,
    pub payload: serde_json::Value,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebReceipt {
    pub run_id: u64,
    pub revision: u64,
    pub state: String,
    pub session_id: String,
    pub branch_id: String,
    pub native_run_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[allow(clippy::large_enum_variant)]
pub enum ExportLine {
    Header {
        format: String,
        version: u32,
        session: SessionInfo,
    },
    Artifact {
        artifact: crate::SummaryArtifact,
    },
    PrivateRelease {
        sources: Vec<aw_agent_core::EvidenceRef>,
    },
    PromptContext {
        prompt_node: String,
        artifact_ids: Vec<String>,
    },
    Node {
        node: Node,
    },
    Branch {
        branch: BranchInfo,
    },
    Footer {
        sha256: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredUsage {
    pub actor: PrincipalId,
    pub model_calls: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub calls_without_complete_usage: u64,
}

pub(crate) fn validate_turn(turn: &Turn) -> aw_agent_core::Result<()> {
    let state = HistoryState {
        version: 1,
        context_blocks: Vec::new(),
        turns: vec![turn.clone()],
        next_turn_id: turn
            .id
            .checked_add(1)
            .ok_or(aw_agent_core::Error::Limit("turn ID overflow"))?,
        summary: None,
        compacted_through: 0,
        summary_provenance_sha256: None,
        needs_reconciliation: turn.needs_reconciliation,
    };
    aw_agent_core::Conversation::from_history(String::new(), &state).map(|_| ())
}
