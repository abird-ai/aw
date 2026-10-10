//! Transport-neutral bounded pipeline definitions and deterministic step logic.
//!
//! This crate has deliberately no provider, tool, store, filesystem, network or
//! authentication authority. Hosts own all external effects and must persist
//! intent before invoking them.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use thiserror::Error;

pub const MAX_PIPELINE_STEPS: usize = 32;
pub const MAX_PIPELINE_TEXT_BYTES: usize = 128 * 1024;
pub const MAX_STEP_LABEL_BYTES: usize = 64;
pub const MAX_APPROVAL_MESSAGE_BYTES: usize = 1024;
pub const MIN_TEAM_MEMBERS: usize = 2;
pub const MAX_TEAM_MEMBERS: usize = 4;
pub const MAX_TEAM_INSTRUCTION_BYTES: usize = 4096;
pub const MAX_TEAM_JOIN_BYTES: usize = 96 * 1024;
pub const MAX_PLANNER_TASKS: usize = 6;
pub const MAX_PLANNER_TASK_BYTES: usize = 8 * 1024;
pub const MAX_PLANNER_OUTPUT_BYTES: usize = 64 * 1024;
pub const MAX_PLANNER_JOIN_BYTES: usize = 96 * 1024;
pub const MAX_TEAM_MEMBER_IMPORT_BYTES: usize = 20 * 1024;
pub const MAX_PLANNER_TASK_IMPORT_BYTES: usize = 12 * 1024;
pub const MAX_REVIEW_POLICY_BYTES: usize = 8 * 1024;
pub const MAX_REVIEW_OUTPUT_BYTES: usize = 16 * 1024;
pub const MAX_REVIEW_SUMMARY_BYTES: usize = 4 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineDefinition {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub limits: PipelineLimits,
    pub steps: Vec<PipelineStep>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamDefinition {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub members: Vec<TeamMember>,
    #[serde(default)]
    pub join: TeamJoinPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamMember {
    pub id: String,
    pub role: String,
    pub instruction: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TeamJoinPolicy {
    #[default]
    AllRequired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamJoinMember {
    pub id: String,
    pub role: String,
    pub answer: String,
    pub answer_truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamJoinReceipt {
    pub policy: TeamJoinPolicy,
    pub members: Vec<TeamJoinMember>,
    pub joined_text: String,
    pub joined_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannerTask {
    pub id: String,
    pub role: String,
    pub task: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannerProposal {
    pub tasks: Vec<PlannerTask>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannerEvidence {
    pub task: String,
    pub allowed_roles: Vec<String>,
    pub max_tasks: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannerTaskResult {
    pub id: String,
    pub role: String,
    pub answer: String,
    pub answer_truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannerJoinReceipt {
    pub tasks: Vec<PlannerTaskResult>,
    pub joined_text: String,
    pub joined_sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewSourceKind {
    Team,
    Planner,
}

impl ReviewSourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Team => "team",
            Self::Planner => "planner",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewEvidence {
    pub source_kind: ReviewSourceKind,
    pub objective: String,
    pub policy: String,
    pub evidence: String,
    pub source_sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    Pass,
    Deny,
    NeedsHuman,
    RequireEvidence,
}

impl ReviewDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Deny => "deny",
            Self::NeedsHuman => "needs_human",
            Self::RequireEvidence => "require_evidence",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewVerdict {
    pub decision: ReviewDecision,
    pub reason_code: String,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineLimits {
    pub max_steps: u16,
    pub max_agent_steps: u16,
    pub max_wall_seconds: u32,
    pub max_input_bytes: u32,
    pub max_output_bytes: u32,
}

impl Default for PipelineLimits {
    fn default() -> Self {
        Self {
            max_steps: 16,
            max_agent_steps: 4,
            max_wall_seconds: 900,
            max_input_bytes: MAX_PIPELINE_TEXT_BYTES as u32,
            max_output_bytes: MAX_PIPELINE_TEXT_BYTES as u32,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PipelineStep {
    Check {
        id: String,
        check: CheckSpec,
    },
    Transform {
        id: String,
        transform: TransformSpec,
    },
    Approval {
        id: String,
        message: String,
    },
    Agent {
        id: String,
        role: String,
    },
    Emit {
        id: String,
    },
}

impl PipelineStep {
    pub fn id(&self) -> &str {
        match self {
            Self::Check { id, .. }
            | Self::Transform { id, .. }
            | Self::Approval { id, .. }
            | Self::Agent { id, .. }
            | Self::Emit { id } => id,
        }
    }

    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Check { .. } => "check",
            Self::Transform { .. } => "transform",
            Self::Approval { .. } => "approval",
            Self::Agent { .. } => "agent",
            Self::Emit { .. } => "emit",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "builtin", rename_all = "snake_case", deny_unknown_fields)]
pub enum CheckSpec {
    NonEmpty,
    MaxBytes { max: u32 },
    Contains { needle: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "builtin", rename_all = "snake_case", deny_unknown_fields)]
pub enum TransformSpec {
    Trim,
    Prefix { text: String },
    Suffix { text: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepDisposition {
    Deterministic,
    Approval,
    ExternalAgent,
    Emit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeterministicReceipt {
    pub step_id: String,
    pub kind: String,
    pub input_sha256: String,
    pub output_sha256: String,
    pub changed: bool,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PipelineError {
    #[error("pipeline definition is invalid: {0}")]
    Definition(&'static str),
    #[error("pipeline text exceeds configured bound")]
    TextLimit,
    #[error("deterministic check failed")]
    CheckFailed,
}

pub type Result<T> = std::result::Result<T, PipelineError>;

fn planner_label(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn validate_review_evidence(evidence: &ReviewEvidence) -> Result<()> {
    if evidence.objective.trim().is_empty()
        || evidence.objective.len() > MAX_PIPELINE_TEXT_BYTES
        || evidence.objective.chars().any(|ch| ch == '\0')
        || evidence.policy.trim().is_empty()
        || evidence.policy.len() > MAX_REVIEW_POLICY_BYTES
        || evidence.policy.chars().any(|ch| ch == '\0')
        || evidence.evidence.trim().is_empty()
        || evidence.evidence.len() > MAX_PIPELINE_TEXT_BYTES
        || evidence.evidence.chars().any(|ch| ch == '\0')
        || !valid_sha256(&evidence.source_sha256)
        || text_sha256(&evidence.evidence) != evidence.source_sha256
    {
        return Err(PipelineError::Definition(
            "guardian review evidence is invalid",
        ));
    }
    Ok(())
}

pub fn review_evidence_sha256(evidence: &ReviewEvidence) -> Result<String> {
    validate_review_evidence(evidence)?;
    let encoded = serde_json::to_vec(evidence)
        .map_err(|_| PipelineError::Definition("guardian review evidence encoding failed"))?;
    Ok(sha256_bytes(&encoded))
}

pub fn validate_review_verdict(verdict: &ReviewVerdict) -> Result<()> {
    if !planner_label(&verdict.reason_code, MAX_STEP_LABEL_BYTES)
        || verdict.summary.trim().is_empty()
        || verdict.summary.len() > MAX_REVIEW_SUMMARY_BYTES
        || verdict.summary.chars().any(|ch| ch == '\0')
    {
        return Err(PipelineError::Definition(
            "guardian review verdict is invalid",
        ));
    }
    Ok(())
}

pub fn review_verdict_sha256(verdict: &ReviewVerdict) -> Result<String> {
    validate_review_verdict(verdict)?;
    let encoded = serde_json::to_vec(verdict)
        .map_err(|_| PipelineError::Definition("guardian review verdict encoding failed"))?;
    Ok(sha256_bytes(&encoded))
}

pub fn parse_review_verdict(raw: &str) -> Result<ReviewVerdict> {
    if raw.is_empty() || raw.len() > MAX_REVIEW_OUTPUT_BYTES {
        return Err(PipelineError::TextLimit);
    }
    let verdict: ReviewVerdict = serde_json::from_str(raw)
        .map_err(|_| PipelineError::Definition("guardian output is not strict verdict JSON"))?;
    validate_review_verdict(&verdict)?;
    Ok(verdict)
}

pub fn validate_planner_evidence(evidence: &PlannerEvidence) -> Result<()> {
    if evidence.task.trim().is_empty()
        || evidence.task.len() > MAX_PIPELINE_TEXT_BYTES
        || evidence.task.chars().any(|ch| ch == '\0')
        || evidence.allowed_roles.is_empty()
        || evidence.allowed_roles.len() > MAX_PLANNER_TASKS
        || evidence.max_tasks == 0
        || usize::from(evidence.max_tasks) > MAX_PLANNER_TASKS
    {
        return Err(PipelineError::Definition("planner evidence is invalid"));
    }
    let mut roles = BTreeSet::new();
    for role in &evidence.allowed_roles {
        if !planner_label(role, MAX_STEP_LABEL_BYTES) || !roles.insert(role.as_str()) {
            return Err(PipelineError::Definition(
                "planner allowed roles must be unique bounded labels",
            ));
        }
    }
    Ok(())
}

pub fn planner_evidence_sha256(evidence: &PlannerEvidence) -> Result<String> {
    validate_planner_evidence(evidence)?;
    let encoded = serde_json::to_vec(evidence)
        .map_err(|_| PipelineError::Definition("planner evidence encoding failed"))?;
    Ok(sha256_bytes(&encoded))
}

pub fn validate_planner_proposal(
    evidence: &PlannerEvidence,
    proposal: &PlannerProposal,
) -> Result<()> {
    validate_planner_evidence(evidence)?;
    if proposal.tasks.is_empty()
        || proposal.tasks.len() > usize::from(evidence.max_tasks)
        || proposal.tasks.len() > MAX_PLANNER_TASKS
    {
        return Err(PipelineError::Definition(
            "planner task count exceeds configured bound",
        ));
    }
    let allowed = evidence
        .allowed_roles
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut ids = BTreeSet::new();
    for task in &proposal.tasks {
        if !planner_label(&task.id, MAX_STEP_LABEL_BYTES) || !ids.insert(task.id.as_str()) {
            return Err(PipelineError::Definition(
                "planner task IDs must be unique bounded labels",
            ));
        }
        if !allowed.contains(task.role.as_str()) {
            return Err(PipelineError::Definition(
                "planner task role is outside the host allowlist",
            ));
        }
        if task.task.trim().is_empty()
            || task.task.len() > MAX_PLANNER_TASK_BYTES
            || task.task.chars().any(|ch| ch == '\0')
            || task.depends_on.len() > MAX_PLANNER_TASKS
        {
            return Err(PipelineError::Definition(
                "planner task body/dependencies are invalid",
            ));
        }
    }
    for task in &proposal.tasks {
        let mut deps = BTreeSet::new();
        for dependency in &task.depends_on {
            if dependency == &task.id
                || !ids.contains(dependency.as_str())
                || !deps.insert(dependency.as_str())
            {
                return Err(PipelineError::Definition(
                    "planner dependency is unknown, duplicate or self-referential",
                ));
            }
        }
    }

    fn visit<'a>(
        id: &'a str,
        tasks: &'a std::collections::BTreeMap<&'a str, &'a PlannerTask>,
        visiting: &mut BTreeSet<&'a str>,
        done: &mut BTreeSet<&'a str>,
    ) -> Result<()> {
        if done.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id) {
            return Err(PipelineError::Definition(
                "planner task graph contains a cycle",
            ));
        }
        let task = tasks
            .get(id)
            .copied()
            .ok_or(PipelineError::Definition("planner dependency disappeared"))?;
        for dependency in &task.depends_on {
            visit(dependency, tasks, visiting, done)?;
        }
        visiting.remove(id);
        done.insert(id);
        Ok(())
    }

    let tasks = proposal
        .tasks
        .iter()
        .map(|task| (task.id.as_str(), task))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut visiting = BTreeSet::new();
    let mut done = BTreeSet::new();
    for id in tasks.keys().copied() {
        visit(id, &tasks, &mut visiting, &mut done)?;
    }
    Ok(())
}

pub fn planner_topological_order(
    evidence: &PlannerEvidence,
    proposal: &PlannerProposal,
) -> Result<Vec<String>> {
    validate_planner_proposal(evidence, proposal)?;
    let mut completed = BTreeSet::new();
    let mut order = Vec::with_capacity(proposal.tasks.len());
    while order.len() < proposal.tasks.len() {
        let before = order.len();
        for task in &proposal.tasks {
            if completed.contains(task.id.as_str()) {
                continue;
            }
            if task
                .depends_on
                .iter()
                .all(|dependency| completed.contains(dependency.as_str()))
            {
                completed.insert(task.id.as_str());
                order.push(task.id.clone());
            }
        }
        if order.len() == before {
            return Err(PipelineError::Definition(
                "planner task graph contains a cycle",
            ));
        }
    }
    Ok(order)
}

pub fn join_planner_results(
    evidence: &PlannerEvidence,
    proposal: &PlannerProposal,
    mut results: Vec<PlannerTaskResult>,
) -> Result<PlannerJoinReceipt> {
    let order = planner_topological_order(evidence, proposal)?;
    if results.len() != proposal.tasks.len() {
        return Err(PipelineError::Definition(
            "planner join is missing task results",
        ));
    }
    let tasks = proposal
        .tasks
        .iter()
        .map(|task| (task.id.as_str(), task))
        .collect::<std::collections::BTreeMap<_, _>>();
    results.sort_by_key(|result| {
        order
            .iter()
            .position(|id| id == &result.id)
            .unwrap_or(usize::MAX)
    });
    let mut seen = BTreeSet::new();
    let mut joined = String::new();
    for result in &results {
        let task = tasks
            .get(result.id.as_str())
            .copied()
            .ok_or(PipelineError::Definition(
                "planner join contains unknown task",
            ))?;
        if task.role != result.role || !seen.insert(result.id.as_str()) {
            return Err(PipelineError::Definition(
                "planner join task identity/role mismatch",
            ));
        }
        if !joined.is_empty() {
            joined.push_str("\n\n");
        }
        joined.push_str("### ");
        joined.push_str(&result.role);
        joined.push_str(" [");
        joined.push_str(&result.id);
        joined.push_str("]\n");
        joined.push_str(&result.answer);
        if result.answer_truncated {
            joined.push_str("\n[bounded import truncated; full answer remains in child Session]");
        }
        if joined.len() > MAX_PLANNER_JOIN_BYTES {
            return Err(PipelineError::TextLimit);
        }
    }
    Ok(PlannerJoinReceipt {
        joined_sha256: text_sha256(&joined),
        joined_text: joined,
        tasks: results,
    })
}

pub fn parse_planner_proposal(evidence: &PlannerEvidence, raw: &str) -> Result<PlannerProposal> {
    if raw.is_empty() || raw.len() > MAX_PLANNER_OUTPUT_BYTES {
        return Err(PipelineError::TextLimit);
    }
    let proposal: PlannerProposal = serde_json::from_str(raw)
        .map_err(|_| PipelineError::Definition("planner output is not strict proposal JSON"))?;
    validate_planner_proposal(evidence, &proposal)?;
    Ok(proposal)
}

pub fn validate_team_definition(definition: &TeamDefinition) -> Result<()> {
    if definition.name.trim().is_empty() || definition.name.len() > 128 {
        return Err(PipelineError::Definition(
            "team name requires 1..128 UTF-8 bytes",
        ));
    }
    if definition.description.len() > 4096 {
        return Err(PipelineError::Definition(
            "team description exceeds 4096 bytes",
        ));
    }
    if !(MIN_TEAM_MEMBERS..=MAX_TEAM_MEMBERS).contains(&definition.members.len()) {
        return Err(PipelineError::Definition(
            "team requires between two and four members",
        ));
    }
    let mut ids = BTreeSet::new();
    for member in &definition.members {
        if member.id.is_empty()
            || member.id.len() > MAX_STEP_LABEL_BYTES
            || !member
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
            || !ids.insert(member.id.clone())
        {
            return Err(PipelineError::Definition(
                "team member IDs must be unique bounded tokens",
            ));
        }
        if member.role.is_empty()
            || member.role.len() > MAX_STEP_LABEL_BYTES
            || !member
                .role
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(PipelineError::Definition("team member role is invalid"));
        }
        if member.instruction.trim().is_empty()
            || member.instruction.len() > MAX_TEAM_INSTRUCTION_BYTES
            || member.instruction.chars().any(|ch| ch == '\0')
        {
            return Err(PipelineError::Definition(
                "team member instruction is invalid",
            ));
        }
    }
    Ok(())
}

pub fn team_definition_sha256(definition: &TeamDefinition) -> Result<String> {
    validate_team_definition(definition)?;
    let encoded = serde_json::to_vec(definition)
        .map_err(|_| PipelineError::Definition("team definition encoding failed"))?;
    Ok(sha256_bytes(&encoded))
}

pub fn join_team_members(
    definition: &TeamDefinition,
    mut members: Vec<TeamJoinMember>,
) -> Result<TeamJoinReceipt> {
    validate_team_definition(definition)?;
    if members.len() != definition.members.len() {
        return Err(PipelineError::Definition(
            "all-required team join is missing member results",
        ));
    }
    members.sort_by_key(|member| {
        definition
            .members
            .iter()
            .position(|expected| expected.id == member.id)
            .unwrap_or(usize::MAX)
    });
    for (expected, actual) in definition.members.iter().zip(members.iter()) {
        if expected.id != actual.id || expected.role != actual.role {
            return Err(PipelineError::Definition(
                "team join member identity/role does not match template",
            ));
        }
    }
    let mut joined = String::new();
    for member in &members {
        if !joined.is_empty() {
            joined.push_str("\n\n");
        }
        joined.push_str("### ");
        joined.push_str(&member.role);
        joined.push_str(" [");
        joined.push_str(&member.id);
        joined.push_str("]\n");
        joined.push_str(&member.answer);
        if member.answer_truncated {
            joined.push_str("\n[bounded import truncated; full answer remains in child Session]");
        }
        if joined.len() > MAX_TEAM_JOIN_BYTES {
            return Err(PipelineError::TextLimit);
        }
    }
    Ok(TeamJoinReceipt {
        policy: definition.join.clone(),
        joined_sha256: text_sha256(&joined),
        joined_text: joined,
        members,
    })
}

pub fn validate_definition(definition: &PipelineDefinition) -> Result<()> {
    if definition.name.trim().is_empty() || definition.name.len() > 128 {
        return Err(PipelineError::Definition(
            "name requires 1..128 UTF-8 bytes",
        ));
    }
    if definition.description.len() > 4096 {
        return Err(PipelineError::Definition("description exceeds 4096 bytes"));
    }
    let limits = &definition.limits;
    if limits.max_steps == 0
        || limits.max_steps as usize > MAX_PIPELINE_STEPS
        || limits.max_agent_steps > limits.max_steps
        || limits.max_wall_seconds == 0
        || limits.max_wall_seconds > 86_400
        || limits.max_input_bytes == 0
        || limits.max_input_bytes as usize > MAX_PIPELINE_TEXT_BYTES
        || limits.max_output_bytes == 0
        || limits.max_output_bytes as usize > MAX_PIPELINE_TEXT_BYTES
    {
        return Err(PipelineError::Definition("invalid pipeline limits"));
    }
    if definition.steps.is_empty()
        || definition.steps.len() > limits.max_steps as usize
        || definition.steps.len() > MAX_PIPELINE_STEPS
    {
        return Err(PipelineError::Definition(
            "step count exceeds configured bound",
        ));
    }
    let mut ids = BTreeSet::new();
    let mut agent_steps = 0u16;
    let mut emit_count = 0usize;
    for (index, step) in definition.steps.iter().enumerate() {
        let id = step.id();
        if id.is_empty()
            || id.len() > MAX_STEP_LABEL_BYTES
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
            || !ids.insert(id.to_owned())
        {
            return Err(PipelineError::Definition(
                "step IDs must be unique bounded tokens",
            ));
        }
        match step {
            PipelineStep::Check { check, .. } => validate_check(check)?,
            PipelineStep::Transform { transform, .. } => validate_transform(transform)?,
            PipelineStep::Approval { message, .. } => {
                if message.trim().is_empty() || message.len() > MAX_APPROVAL_MESSAGE_BYTES {
                    return Err(PipelineError::Definition("approval message is invalid"));
                }
            }
            PipelineStep::Agent { role, .. } => {
                agent_steps = agent_steps.saturating_add(1);
                if role.trim().is_empty()
                    || role.len() > MAX_STEP_LABEL_BYTES
                    || !role
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
                {
                    return Err(PipelineError::Definition("agent role is invalid"));
                }
            }
            PipelineStep::Emit { .. } => {
                emit_count += 1;
                if index + 1 != definition.steps.len() {
                    return Err(PipelineError::Definition("emit must be the final step"));
                }
            }
        }
    }
    if agent_steps > limits.max_agent_steps {
        return Err(PipelineError::Definition(
            "agent step count exceeds configured bound",
        ));
    }
    if emit_count > 1 {
        return Err(PipelineError::Definition(
            "only one final emit step is permitted",
        ));
    }
    Ok(())
}

fn validate_check(check: &CheckSpec) -> Result<()> {
    match check {
        CheckSpec::NonEmpty => Ok(()),
        CheckSpec::MaxBytes { max } => {
            if *max == 0 || *max as usize > MAX_PIPELINE_TEXT_BYTES {
                Err(PipelineError::Definition("check max_bytes is invalid"))
            } else {
                Ok(())
            }
        }
        CheckSpec::Contains { needle } => {
            if needle.is_empty() || needle.len() > 4096 {
                Err(PipelineError::Definition("check needle is invalid"))
            } else {
                Ok(())
            }
        }
    }
}

fn validate_transform(transform: &TransformSpec) -> Result<()> {
    match transform {
        TransformSpec::Trim => Ok(()),
        TransformSpec::Prefix { text } | TransformSpec::Suffix { text } => {
            if text.len() > 16 * 1024 {
                Err(PipelineError::Definition("transform literal is too large"))
            } else {
                Ok(())
            }
        }
    }
}

pub fn definition_sha256(definition: &PipelineDefinition) -> Result<String> {
    validate_definition(definition)?;
    let encoded = serde_json::to_vec(definition)
        .map_err(|_| PipelineError::Definition("definition encoding failed"))?;
    Ok(sha256_bytes(&encoded))
}

pub fn text_sha256(value: &str) -> String {
    sha256_bytes(value.as_bytes())
}

fn sha256_bytes(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

pub fn disposition(step: &PipelineStep) -> StepDisposition {
    match step {
        PipelineStep::Check { .. } | PipelineStep::Transform { .. } => {
            StepDisposition::Deterministic
        }
        PipelineStep::Approval { .. } => StepDisposition::Approval,
        PipelineStep::Agent { .. } => StepDisposition::ExternalAgent,
        PipelineStep::Emit { .. } => StepDisposition::Emit,
    }
}

pub fn apply_deterministic(
    step: &PipelineStep,
    input: &str,
    output_limit: usize,
) -> Result<(String, DeterministicReceipt)> {
    if input.len() > MAX_PIPELINE_TEXT_BYTES
        || output_limit == 0
        || output_limit > MAX_PIPELINE_TEXT_BYTES
    {
        return Err(PipelineError::TextLimit);
    }
    let input_digest = text_sha256(input);
    let output = match step {
        PipelineStep::Check { check, .. } => {
            let allowed = match check {
                CheckSpec::NonEmpty => !input.trim().is_empty(),
                CheckSpec::MaxBytes { max } => input.len() <= *max as usize,
                CheckSpec::Contains { needle } => input.contains(needle),
            };
            if !allowed {
                return Err(PipelineError::CheckFailed);
            }
            input.to_owned()
        }
        PipelineStep::Transform { transform, .. } => match transform {
            TransformSpec::Trim => input.trim().to_owned(),
            TransformSpec::Prefix { text } => format!("{text}{input}"),
            TransformSpec::Suffix { text } => format!("{input}{text}"),
        },
        _ => return Err(PipelineError::Definition("step is not deterministic")),
    };
    if output.len() > output_limit || output.len() > MAX_PIPELINE_TEXT_BYTES {
        return Err(PipelineError::TextLimit);
    }
    let receipt = DeterministicReceipt {
        step_id: step.id().to_owned(),
        kind: step.kind_name().to_owned(),
        input_sha256: input_digest,
        output_sha256: text_sha256(&output),
        changed: output != input,
    };
    Ok((output, receipt))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition() -> PipelineDefinition {
        PipelineDefinition {
            name: "review".into(),
            description: String::new(),
            limits: PipelineLimits::default(),
            steps: vec![
                PipelineStep::Check {
                    id: "nonempty".into(),
                    check: CheckSpec::NonEmpty,
                },
                PipelineStep::Transform {
                    id: "trim".into(),
                    transform: TransformSpec::Trim,
                },
                PipelineStep::Approval {
                    id: "approve".into(),
                    message: "Continue?".into(),
                },
                PipelineStep::Agent {
                    id: "agent".into(),
                    role: "main".into(),
                },
                PipelineStep::Emit { id: "emit".into() },
            ],
        }
    }

    #[test]
    fn bounded_linear_definition_is_valid() {
        validate_definition(&definition()).unwrap();
    }

    #[test]
    fn duplicate_step_ids_fail_closed() {
        let mut def = definition();
        def.steps[1] = PipelineStep::Transform {
            id: "nonempty".into(),
            transform: TransformSpec::Trim,
        };
        assert!(matches!(
            validate_definition(&def),
            Err(PipelineError::Definition(_))
        ));
    }

    #[test]
    fn emit_must_be_last() {
        let mut def = definition();
        def.steps.swap(3, 4);
        assert!(matches!(
            validate_definition(&def),
            Err(PipelineError::Definition(_))
        ));
    }

    #[test]
    fn deterministic_transform_has_exact_digests() {
        let step = PipelineStep::Transform {
            id: "trim".into(),
            transform: TransformSpec::Trim,
        };
        let (out, receipt) = apply_deterministic(&step, "  hi 🙂  ", 1024).unwrap();
        assert_eq!(out, "hi 🙂");
        assert_eq!(receipt.input_sha256, text_sha256("  hi 🙂  "));
        assert_eq!(receipt.output_sha256, text_sha256("hi 🙂"));
        assert!(receipt.changed);
    }

    #[test]
    fn failed_check_does_not_transform_or_emit() {
        let step = PipelineStep::Check {
            id: "contains".into(),
            check: CheckSpec::Contains {
                needle: "ok".into(),
            },
        };
        assert_eq!(
            apply_deterministic(&step, "no", 1024),
            Err(PipelineError::CheckFailed)
        );
    }

    #[test]
    fn definition_digest_changes_with_step_semantics() {
        let a = definition();
        let mut b = a.clone();
        if let PipelineStep::Approval { message, .. } = &mut b.steps[2] {
            *message = "Different".into();
        }
        assert_ne!(
            definition_sha256(&a).unwrap(),
            definition_sha256(&b).unwrap()
        );
    }

    fn planner_evidence() -> PlannerEvidence {
        PlannerEvidence {
            task: "Investigate and implement the requested change.".into(),
            allowed_roles: vec!["worker.analysis".into(), "worker.impl".into()],
            max_tasks: 4,
        }
    }

    #[test]
    fn planner_proposal_is_bounded_acyclic_and_topologically_ordered() {
        let evidence = planner_evidence();
        let proposal = PlannerProposal {
            tasks: vec![
                PlannerTask {
                    id: "inspect".into(),
                    role: "worker.analysis".into(),
                    task: "Inspect the relevant code.".into(),
                    depends_on: vec![],
                },
                PlannerTask {
                    id: "implement".into(),
                    role: "worker.impl".into(),
                    task: "Implement using the inspection evidence.".into(),
                    depends_on: vec!["inspect".into()],
                },
            ],
        };
        validate_planner_proposal(&evidence, &proposal).unwrap();
        assert_eq!(
            planner_topological_order(&evidence, &proposal).unwrap(),
            vec!["inspect", "implement"]
        );
        let raw = serde_json::to_string(&proposal).unwrap();
        assert_eq!(parse_planner_proposal(&evidence, &raw).unwrap(), proposal);
        let joined = join_planner_results(
            &evidence,
            &proposal,
            vec![
                PlannerTaskResult {
                    id: "implement".into(),
                    role: "worker.impl".into(),
                    answer: "implemented".into(),
                    answer_truncated: false,
                },
                PlannerTaskResult {
                    id: "inspect".into(),
                    role: "worker.analysis".into(),
                    answer: "inspected".into(),
                    answer_truncated: false,
                },
            ],
        )
        .unwrap();
        assert!(joined
            .joined_text
            .starts_with("### worker.analysis [inspect]"));
        assert!(joined.joined_text.contains("### worker.impl [implement]"));
    }

    #[test]
    fn planner_rejects_unknown_roles_dependencies_and_cycles() {
        let evidence = planner_evidence();
        let mut proposal = PlannerProposal {
            tasks: vec![PlannerTask {
                id: "a".into(),
                role: "worker.unknown".into(),
                task: "x".into(),
                depends_on: vec![],
            }],
        };
        assert!(validate_planner_proposal(&evidence, &proposal).is_err());
        proposal.tasks[0].role = "worker.analysis".into();
        proposal.tasks[0].depends_on = vec!["missing".into()];
        assert!(validate_planner_proposal(&evidence, &proposal).is_err());
        proposal.tasks = vec![
            PlannerTask {
                id: "a".into(),
                role: "worker.analysis".into(),
                task: "a".into(),
                depends_on: vec!["b".into()],
            },
            PlannerTask {
                id: "b".into(),
                role: "worker.impl".into(),
                task: "b".into(),
                depends_on: vec!["a".into()],
            },
        ];
        assert!(validate_planner_proposal(&evidence, &proposal).is_err());
    }

    #[test]
    fn guardian_review_evidence_and_verdict_are_strict_and_digest_bound() {
        let joined = "### worker.analysis [inspect]\\ninspected";
        let evidence = ReviewEvidence {
            source_kind: ReviewSourceKind::Planner,
            objective: "Review the implementation evidence.".into(),
            policy: "Pass only when the evidence directly supports the claimed result.".into(),
            evidence: joined.into(),
            source_sha256: text_sha256(joined),
        };
        validate_review_evidence(&evidence).unwrap();
        let digest = review_evidence_sha256(&evidence).unwrap();
        assert_eq!(digest.len(), 64);

        let verdict = parse_review_verdict(
            r#"{"decision":"pass","reason_code":"evidence_ok","summary":"The bounded evidence supports the claim."}"#,
        )
        .unwrap();
        assert_eq!(verdict.decision, ReviewDecision::Pass);
        assert_eq!(review_verdict_sha256(&verdict).unwrap().len(), 64);

        assert!(parse_review_verdict(
            r#"{"decision":"pass","reason_code":"evidence_ok","summary":"ok","route":"worker"}"#
        )
        .is_err());
        assert!(parse_review_verdict(
            r#"{"decision":"grant_tools","reason_code":"unsafe","summary":"no"}"#
        )
        .is_err());

        let mut forged = evidence.clone();
        forged.evidence.push_str(" tampered");
        assert!(validate_review_evidence(&forged).is_err());
    }

    #[test]
    fn guardian_review_supports_only_bounded_policy_decisions() {
        for decision in ["pass", "deny", "needs_human", "require_evidence"] {
            let raw = format!(
                "{{\"decision\":\"{decision}\",\"reason_code\":\"policy\",\"summary\":\"bounded explanation\"}}"
            );
            parse_review_verdict(&raw).unwrap();
        }
        let oversized = ReviewVerdict {
            decision: ReviewDecision::Deny,
            reason_code: "policy".into(),
            summary: "x".repeat(MAX_REVIEW_SUMMARY_BYTES + 1),
        };
        assert!(validate_review_verdict(&oversized).is_err());
    }

    fn team_definition() -> TeamDefinition {
        TeamDefinition {
            name: "candidate-team".into(),
            description: String::new(),
            members: vec![
                TeamMember {
                    id: "analysis".into(),
                    role: "worker.analysis".into(),
                    instruction: "Analyze the task independently.".into(),
                },
                TeamMember {
                    id: "critic".into(),
                    role: "worker.critic".into(),
                    instruction: "Look for flaws and alternatives.".into(),
                },
            ],
            join: TeamJoinPolicy::AllRequired,
        }
    }

    #[test]
    fn bounded_team_template_and_join_are_deterministic() {
        let team = team_definition();
        validate_team_definition(&team).unwrap();
        let a = team_definition_sha256(&team).unwrap();
        let b = team_definition_sha256(&team).unwrap();
        assert_eq!(a, b);
        let receipt = join_team_members(
            &team,
            vec![
                TeamJoinMember {
                    id: "critic".into(),
                    role: "worker.critic".into(),
                    answer: "second".into(),
                    answer_truncated: false,
                },
                TeamJoinMember {
                    id: "analysis".into(),
                    role: "worker.analysis".into(),
                    answer: "first".into(),
                    answer_truncated: false,
                },
            ],
        )
        .unwrap();
        assert!(receipt
            .joined_text
            .starts_with("### worker.analysis [analysis]"));
        assert!(receipt.joined_text.contains("### worker.critic [critic]"));
        assert_eq!(receipt.joined_sha256, text_sha256(&receipt.joined_text));
    }

    #[test]
    fn team_template_rejects_unbounded_or_mismatched_members() {
        let mut team = team_definition();
        team.members.truncate(1);
        assert!(validate_team_definition(&team).is_err());
        let team = team_definition();
        let bad = join_team_members(
            &team,
            vec![TeamJoinMember {
                id: "analysis".into(),
                role: "worker.analysis".into(),
                answer: "only one".into(),
                answer_truncated: false,
            }],
        );
        assert!(bad.is_err());
    }
}
