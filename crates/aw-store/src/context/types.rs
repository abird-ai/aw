use super::super::{BranchInfo, SessionInfo};
use aw_agent_core::{CompletionMeta, EvidenceRef, InvocationIdentity, PrincipalId, SummaryInput};
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TargetHead {
    pub session_id: String,
    pub branch_id: String,
    pub head: String,
    pub revision: u64,
}
impl From<&BranchInfo> for TargetHead {
    fn from(b: &BranchInfo) -> Self {
        Self {
            session_id: b.session_id.clone(),
            branch_id: b.id.clone(),
            head: b.head.clone(),
            revision: b.revision,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Selection {
    Standalone,
    Divergent,
    Range { start_exclusive: String },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSelection {
    pub session_id: String,
    pub branch: String,
    pub head: Option<String>,
    pub selection: Selection,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanKind {
    Import,
    Compact {
        keep_recent_turns: usize,
    },
    /// Deterministic local projection of already materialized semantic facts.
    /// It is never eligible for summary/model generation.
    MaterializedFacts,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareContext {
    pub target: TargetHead,
    pub sources: Vec<SourceSelection>,
    pub kind: PlanKind,
    pub focus: String,
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
    pub timeout_seconds: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedSource {
    pub session_id: String,
    pub branch_id: String,
    pub branch_name: String,
    pub head: String,
    pub revision: u64,
    pub selection: Selection,
    pub common_ancestor: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub source: EvidenceRef,
    pub kind: String,
    pub actor: PrincipalId,
    pub execution_context: String,
    pub content: serde_json::Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextPlan {
    pub version: u32,
    pub id: String,
    pub actor: PrincipalId,
    pub target: TargetHead,
    pub kind: PlanKind,
    pub sources: Vec<PinnedSource>,
    pub evidence: Vec<Evidence>,
    pub dependencies: Vec<EvidenceRef>,
    pub release_versions: std::collections::BTreeMap<String, String>,
    pub duplicate_nodes_omitted: usize,
    pub keep_from: Option<String>,
    pub focus: String,
    pub max_output_bytes: usize,
    pub timeout_seconds: u64,
    pub created_at: i64,
}
impl ContextPlan {
    pub fn input(&self) -> aw_agent_core::Result<SummaryInput> {
        if matches!(self.kind, PlanKind::MaterializedFacts) {
            return Err(aw_agent_core::Error::Denied(
                "materialized-fact context is deterministic local projection; model generation is not permitted",
            ));
        }
        Ok(SummaryInput { plan_id:self.id.clone(), focus:self.focus.clone(),
            evidence_json:serde_json::to_string(&serde_json::json!({"entries":self.evidence,"dependencies":self.dependencies,"snapshots":self.sources,"kind":self.kind,"release_versions":self.release_versions,"coverage":"complete_selected_scope"})).map_err(|_|aw_agent_core::Error::Storage("invalid evidence"))?,
            sources:self.dependencies.clone(), max_output_bytes:self.max_output_bytes, timeout_seconds:self.timeout_seconds })
    }
    pub fn digest(&self) -> aw_agent_core::Result<String> {
        if matches!(self.kind, PlanKind::MaterializedFacts) {
            let encoded = serde_json::to_vec(self)
                .map_err(|_| aw_agent_core::Error::Storage("invalid materialized context plan"))?;
            return Ok(format!("{:x}", sha2::Sha256::digest(encoded)));
        }
        self.input()?.digest()
    }
    pub fn validate_stored(&self) -> aw_agent_core::Result<()> {
        if matches!(self.kind, PlanKind::MaterializedFacts) {
            if !self.sources.is_empty()
                || !self.evidence.is_empty()
                || !self.dependencies.is_empty()
                || !self.release_versions.is_empty()
                || self.duplicate_nodes_omitted != 0
                || self.keep_from.is_some()
                || self.focus.is_empty()
                || self.focus.len() > 4096
                || self.max_output_bytes == 0
                || self.max_output_bytes > aw_agent_core::MAX_SUMMARY_OUTPUT
                || !(1..=600).contains(&self.timeout_seconds)
            {
                return Err(aw_agent_core::Error::Protocol(
                    "invalid materialized-fact context plan",
                ));
            }
            return Ok(());
        }
        self.input()?.validate()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SummaryArtifact {
    pub version: u32,
    pub id: String,
    pub owner: PrincipalId,
    pub method: String,
    pub text: String,
    pub text_sha256: String,
    pub plan: ContextPlan,
    pub generated_from: Option<String>,
    pub identity: Option<InvocationIdentity>,
    pub meta: Option<CompletionMeta>,
    pub imported_unverified: bool,
    pub created_at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextNode {
    pub artifact_id: String,
    pub keep_from: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ApplyMode {
    Append,
    Fork { name: String },
    NewSession { title: String },
    NextPrompt,
    Compact,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyContext {
    pub artifact_id: String,
    pub target: TargetHead,
    pub mode: ApplyMode,
    pub request_id: String,
    pub confirmed_disclosure: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextApplied {
    pub session: SessionInfo,
    pub branch: BranchInfo,
    pub node_id: Option<String>,
    pub pending_id: Option<String>,
    pub reused: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SummaryOperation {
    pub id: String,
    pub request_id: String,
    pub plan_id: String,
    pub state: String,
    pub artifact_id: Option<String>,
    pub identity: Option<InvocationIdentity>,
    pub meta: Option<CompletionMeta>,
    pub error: Option<String>,
}
