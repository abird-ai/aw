use crate::{
    authorize, branch_in, db, decode, durability, ensure_user_for_principal, id, json, label, now,
    principal_is_session_owner, resources, session_in, SessionBinding, SessionInfo, SessionStore,
};
#[cfg(feature = "routing")]
use aw_agent_core::RoutingResult;
use aw_agent_core::{Error, InvocationIdentity, InvocationRecord, PrincipalId, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentMode {
    Foreground,
    Background,
}
impl SubagentMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Foreground => "foreground",
            Self::Background => "background",
        }
    }
}
impl std::str::FromStr for SubagentMode {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "foreground" => Ok(Self::Foreground),
            "background" => Ok(Self::Background),
            _ => Err(Error::Storage("invalid stored subagent mode")),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubagentInfo {
    pub id: String,
    pub parent_session_id: String,
    pub parent_branch_id: String,
    pub parent_head: String,
    pub parent_branch_revision: u64,
    pub parent_turn_id: u64,
    pub child_session_id: String,
    pub title: String,
    pub mode: SubagentMode,
    pub state: String,
    pub task_sha256: String,
    pub identity: InvocationIdentity,
    pub tool_mode: String,
    pub delegation_root_id: Option<String>,
    pub delegation_allocation_id: Option<String>,
    pub depth: Option<u8>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub settled_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubagentExecutionBindingInfo {
    pub id: String,
    pub parent_session_id: String,
    pub parent_branch_id: String,
    pub parent_turn_id: u64,
    pub kind: String,
    pub tool_mode: String,
    pub parent_repo_root: PathBuf,
    pub execution_root: PathBuf,
    pub base_revision: String,
    pub state: String,
    pub child_session_id: Option<String>,
    pub created_at: i64,
    pub ready_at: Option<i64>,
    pub bound_at: Option<i64>,
    pub cleanup_started_at: Option<i64>,
    pub removed_at: Option<i64>,
    pub error: Option<String>,
    pub delegation_root_id: Option<String>,
    pub delegation_allocation_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationLimits {
    pub max_children_created: u32,
    pub max_active_children: u32,
    pub max_model_attempts: u32,
    pub max_depth: u8,
    pub deadline_seconds: u32,
}
impl DelegationLimits {
    pub fn validate(self) -> Result<Self> {
        if !(1..=32).contains(&self.max_children_created)
            || !(1..=4).contains(&self.max_active_children)
            || self.max_active_children > self.max_children_created
            || !(1..=256).contains(&self.max_model_attempts)
            || !(1..=4).contains(&self.max_depth)
            || !(1..=86_400).contains(&self.deadline_seconds)
        {
            return Err(Error::Config(
                "invalid bounded subagent delegation limits".into(),
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DelegationRootInfo {
    pub id: String,
    pub root_session_id: String,
    pub root_branch_id: String,
    pub root_turn_id: u64,
    pub state: String,
    pub limits: DelegationLimits,
    pub deadline_at: i64,
    pub children_created: u32,
    pub active_children: u32,
    pub model_attempts_started: u32,
    pub model_attempts_finished: u32,
    pub input_tokens_known: u64,
    pub output_tokens_known: u64,
    pub calls_without_complete_usage: u32,
    pub child_routing_revision_id: Option<String>,
    pub planner_routing_revision_id: Option<String>,
    pub guardian_routing_revision_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DelegationChildAllocationInfo {
    pub id: String,
    pub root_id: String,
    pub parent_session_id: String,
    pub parent_branch_id: String,
    pub parent_turn_id: u64,
    pub depth: u8,
    pub state: String,
    pub child_session_id: Option<String>,
    pub execution_binding_id: Option<String>,
    pub parent_allocation_id: Option<String>,
    pub routing_operation_id: Option<String>,
    pub routing_target_id: Option<String>,
    pub routing_selection_source: Option<String>,
    pub selected_identity: Option<InvocationIdentity>,
    pub routing_bound_at: Option<i64>,
    pub created_at: i64,
    pub settled_at: Option<i64>,
    pub error: Option<String>,
}

pub fn new_subagent_execution_binding_id() -> Result<String> {
    id("sxb")
}

fn path_bytes(path: &Path) -> Result<Vec<u8>> {
    if !path.is_absolute() {
        return Err(Error::Config(
            "subagent execution paths must be absolute".into(),
        ));
    }
    let bytes = aw_platform::os_str_bytes(path.as_os_str())
        .map_err(Error::Io)?
        .into_owned();
    if bytes.is_empty() || bytes.len() > 16 * 1024 || bytes.contains(&0) {
        return Err(Error::Config(
            "invalid bounded subagent execution path".into(),
        ));
    }
    Ok(bytes)
}

fn path_from_bytes(bytes: Vec<u8>) -> Result<PathBuf> {
    if bytes.is_empty() || bytes.len() > 16 * 1024 || bytes.contains(&0) {
        return Err(Error::Storage("invalid stored subagent execution path"));
    }
    Ok(PathBuf::from(
        aw_platform::os_string_from_bytes(bytes).map_err(Error::Io)?,
    ))
}

fn validate_revision(value: &str) -> Result<()> {
    if !matches!(value.len(), 40 | 64) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Config(
            "invalid exact Git object ID for subagent execution".into(),
        ));
    }
    Ok(())
}

fn bounded_execution_error(value: Option<&str>) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(Error::Config(
            "invalid bounded subagent execution error".into(),
        ));
    }
    Ok(Some(value.to_owned()))
}

#[cfg(feature = "pipelines")]
pub(crate) fn row_for_team(c: &rusqlite::Connection, id: &str) -> Result<SubagentInfo> {
    row(c, id)
}

fn validate_sha256(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Config(
            "subagent task digest must be 32-byte lowercase/uppercase hex".into(),
        ));
    }
    Ok(())
}

fn validate_tool_mode(value: &str) -> Result<()> {
    if matches!(value, "off" | "read-only" | "workspace" | "trusted") {
        Ok(())
    } else {
        Err(Error::Config("invalid subagent tool mode".into()))
    }
}

fn row(c: &rusqlite::Connection, id: &str) -> Result<SubagentInfo> {
    let value = db(c
        .query_row(
            "SELECT a.id,a.parent_session_id,a.parent_branch_id,a.parent_head,
                    a.parent_branch_revision,a.parent_turn_id,a.child_session_id,s.title,
                    a.mode,a.state,a.task_sha256,a.invocation_identity_json,a.tool_mode,
                    a.delegation_root_id,a.delegation_allocation_id,a.recursive_depth,
                    a.created_at,a.started_at,a.settled_at,a.error
             FROM subagent_sessions a
             JOIN sessions s ON s.id=a.child_session_id
             WHERE a.id=?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, u64>(4)?,
                    r.get::<_, u64>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, String>(9)?,
                    r.get::<_, String>(10)?,
                    r.get::<_, String>(11)?,
                    r.get::<_, String>(12)?,
                    r.get::<_, Option<String>>(13)?,
                    r.get::<_, Option<String>>(14)?,
                    r.get::<_, Option<u8>>(15)?,
                    r.get::<_, i64>(16)?,
                    r.get::<_, Option<i64>>(17)?,
                    r.get::<_, Option<i64>>(18)?,
                    r.get::<_, Option<String>>(19)?,
                ))
            },
        )
        .optional())?
    .ok_or(Error::Denied("subagent unavailable or not permitted"))?;
    Ok(SubagentInfo {
        id: value.0,
        parent_session_id: value.1,
        parent_branch_id: value.2,
        parent_head: value.3,
        parent_branch_revision: value.4,
        parent_turn_id: value.5,
        child_session_id: value.6,
        title: value.7,
        mode: value.8.parse()?,
        state: value.9,
        task_sha256: value.10,
        identity: decode(&value.11)?,
        tool_mode: value.12,
        delegation_root_id: value.13,
        delegation_allocation_id: value.14,
        depth: value.15,
        created_at: value.16,
        started_at: value.17,
        settled_at: value.18,
        error: value.19,
    })
}

fn model_only_parent_run(
    c: &rusqlite::Connection,
    child_session: &str,
    root_key: &str,
    identity: Option<&InvocationIdentity>,
) -> Result<Option<String>> {
    let relation: Option<(String, String, u64, String, String, String, Option<String>)> = db(c
        .query_row(
            "SELECT parent_session_id,parent_branch_id,parent_turn_id,invocation_identity_json,
                    tool_mode,state,delegation_root_id
             FROM subagent_sessions WHERE child_session_id=?1",
            [child_session],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .optional())?;
    let Some((
        parent_session,
        parent_branch,
        parent_turn_id,
        stored_identity,
        tool_mode,
        state,
        root_id,
    )) = relation
    else {
        return Ok(None);
    };
    if tool_mode != "off" || state != "running" {
        return Ok(None);
    }
    let stored_identity: InvocationIdentity = decode(&stored_identity)?;
    if identity.is_some_and(|identity| identity != &stored_identity) {
        return Ok(None);
    }
    let Some(root_id) = root_id else {
        return Ok(None);
    };
    let root = delegation_root_row(c, &root_id)?;
    if root.state != "active" {
        return Ok(None);
    }

    // The direct parent must be either the root Agent itself or another running
    // model-only child in the same delegation root with the exact same identity.
    if parent_session == root.root_session_id {
        if parent_branch != root.root_branch_id || parent_turn_id != root.root_turn_id {
            return Ok(None);
        }
    } else {
        let parent_ok: bool = db(c.query_row(
            "SELECT EXISTS(
                SELECT 1
                FROM subagent_sessions p
                JOIN delegation_child_allocations a
                  ON a.id=p.delegation_allocation_id
                 AND a.root_id=p.delegation_root_id
                 AND a.child_session_id=p.child_session_id
                WHERE p.child_session_id=?1 AND p.delegation_root_id=?2
                  AND p.state='running' AND p.tool_mode='off'
                  AND a.state='running'
             )",
            params![parent_session, root_id],
            |r| r.get(0),
        ))?;
        if !parent_ok {
            return Ok(None);
        }
    }

    let direct_parent_run: Option<String> = db(c
        .query_row(
            "SELECT active_run FROM branches WHERE id=?1 AND session_id=?2",
            params![parent_branch, parent_session],
            |r| r.get(0),
        )
        .optional())?
    .flatten();
    let Some(direct_parent_run) = direct_parent_run else {
        return Ok(None);
    };
    let direct_parent_running: bool = db(c.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM runs
            WHERE id=?1 AND session_id=?2 AND branch_id=?3
              AND turn_id=?4 AND status='running'
         )",
        params![
            direct_parent_run,
            parent_session,
            parent_branch,
            parent_turn_id
        ],
        |r| r.get(0),
    ))?;
    if !direct_parent_running {
        return Ok(None);
    }

    // The original root Agent remains the only legitimate owner of the physical
    // root. Deeper model-only descendants have synthetic run keys. Any other
    // physical run or hazard still blocks recursive admission.
    let root_run: Option<String> = db(c
        .query_row(
            "SELECT active_run FROM branches WHERE id=?1 AND session_id=?2",
            params![root.root_branch_id, root.root_session_id],
            |r| r.get(0),
        )
        .optional())?
    .flatten();
    let Some(root_run) = root_run else {
        return Ok(None);
    };
    let root_running: bool = db(c.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM runs
            WHERE id=?1 AND session_id=?2 AND branch_id=?3 AND turn_id=?4
              AND root_key=?5 AND status='running'
         )",
        params![
            root_run,
            root.root_session_id,
            root.root_branch_id,
            root.root_turn_id,
            root_key
        ],
        |r| r.get(0),
    ))?;
    if !root_running {
        return Ok(None);
    }
    let foreign_hazards: i64 = db(c.query_row(
        "SELECT count(*) FROM hazards
         WHERE root_key=?1 AND state<>'settled' AND run_id<>?2",
        params![root_key, root_run],
        |r| r.get(0),
    ))?;
    let foreign_runs: i64 = db(c.query_row(
        "SELECT count(*) FROM runs
         WHERE root_key=?1 AND status='running' AND id<>?2",
        params![root_key, root_run],
        |r| r.get(0),
    ))?;
    if foreign_hazards != 0 || foreign_runs != 0 {
        return Ok(None);
    }
    Ok(Some(direct_parent_run))
}

pub(crate) fn model_only_child_can_ignore_parent_hazard(
    c: &rusqlite::Connection,
    child_session: &str,
    root_key: &str,
) -> Result<bool> {
    Ok(model_only_parent_run(c, child_session, root_key, None)?.is_some())
}

fn execution_row(c: &rusqlite::Connection, id: &str) -> Result<SubagentExecutionBindingInfo> {
    let value = db(c
        .query_row(
            "SELECT id,parent_session_id,parent_branch_id,parent_turn_id,kind,tool_mode,
                    parent_repo_root,execution_root,base_revision,state,child_session_id,
                    created_at,ready_at,bound_at,cleanup_started_at,removed_at,error,
                    delegation_root_id,delegation_allocation_id
             FROM subagent_execution_bindings WHERE id=?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, u64>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, Vec<u8>>(6)?,
                    r.get::<_, Vec<u8>>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, String>(9)?,
                    r.get::<_, Option<String>>(10)?,
                    r.get::<_, i64>(11)?,
                    r.get::<_, Option<i64>>(12)?,
                    r.get::<_, Option<i64>>(13)?,
                    r.get::<_, Option<i64>>(14)?,
                    r.get::<_, Option<i64>>(15)?,
                    r.get::<_, Option<String>>(16)?,
                    r.get::<_, Option<String>>(17)?,
                    r.get::<_, Option<String>>(18)?,
                ))
            },
        )
        .optional())?
    .ok_or(Error::Denied(
        "subagent execution binding unavailable or not permitted",
    ))?;
    Ok(SubagentExecutionBindingInfo {
        id: value.0,
        parent_session_id: value.1,
        parent_branch_id: value.2,
        parent_turn_id: value.3,
        kind: value.4,
        tool_mode: value.5,
        parent_repo_root: path_from_bytes(value.6)?,
        execution_root: path_from_bytes(value.7)?,
        base_revision: value.8,
        state: value.9,
        child_session_id: value.10,
        created_at: value.11,
        ready_at: value.12,
        bound_at: value.13,
        cleanup_started_at: value.14,
        removed_at: value.15,
        error: value.16,
        delegation_root_id: value.17,
        delegation_allocation_id: value.18,
    })
}

pub(crate) fn delegation_root_row(
    c: &rusqlite::Connection,
    id: &str,
) -> Result<DelegationRootInfo> {
    let value = db(c
        .query_row(
            "SELECT id,root_session_id,root_branch_id,root_turn_id,state,
                    max_children_created,max_active_children,max_model_attempts,recursive_max_depth,
                    deadline_seconds,deadline_at,children_created,active_children,
                    model_attempts_started,model_attempts_finished,input_tokens_known,
                    output_tokens_known,calls_without_complete_usage,child_routing_revision_id,planner_routing_revision_id,
                    guardian_routing_revision_id,created_at,updated_at
             FROM delegation_roots WHERE id=?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, u64>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, u32>(5)?,
                    r.get::<_, u32>(6)?,
                    r.get::<_, u32>(7)?,
                    r.get::<_, u8>(8)?,
                    r.get::<_, u32>(9)?,
                    r.get::<_, i64>(10)?,
                    r.get::<_, u32>(11)?,
                    r.get::<_, u32>(12)?,
                    r.get::<_, u32>(13)?,
                    r.get::<_, u32>(14)?,
                    r.get::<_, u64>(15)?,
                    r.get::<_, u64>(16)?,
                    r.get::<_, u32>(17)?,
                    r.get::<_, Option<String>>(18)?,
                    r.get::<_, Option<String>>(19)?,
                    r.get::<_, Option<String>>(20)?,
                    r.get::<_, i64>(21)?,
                    r.get::<_, i64>(22)?,
                ))
            },
        )
        .optional())?
    .ok_or(Error::Denied(
        "delegation root unavailable or not permitted",
    ))?;
    Ok(DelegationRootInfo {
        id: value.0,
        root_session_id: value.1,
        root_branch_id: value.2,
        root_turn_id: value.3,
        state: value.4,
        limits: DelegationLimits {
            max_children_created: value.5,
            max_active_children: value.6,
            max_model_attempts: value.7,
            max_depth: value.8,
            deadline_seconds: value.9,
        },
        deadline_at: value.10,
        children_created: value.11,
        active_children: value.12,
        model_attempts_started: value.13,
        model_attempts_finished: value.14,
        input_tokens_known: value.15,
        output_tokens_known: value.16,
        calls_without_complete_usage: value.17,
        child_routing_revision_id: value.18,
        planner_routing_revision_id: value.19,
        guardian_routing_revision_id: value.20,
        created_at: value.21,
        updated_at: value.22,
    })
}

pub(crate) fn delegation_allocation_row(
    c: &rusqlite::Connection,
    id: &str,
) -> Result<DelegationChildAllocationInfo> {
    let value = db(c
        .query_row(
            "SELECT id,root_id,parent_session_id,parent_branch_id,parent_turn_id,recursive_depth,state,
                    child_session_id,execution_binding_id,parent_allocation_id,routing_operation_id,
                    routing_target_id,routing_selection_source,selected_identity_json,routing_bound_at,
                    created_at,settled_at,error
             FROM delegation_child_allocations WHERE id=?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, u64>(4)?,
                    r.get::<_, u8>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, Option<String>>(7)?,
                    r.get::<_, Option<String>>(8)?,
                    r.get::<_, Option<String>>(9)?,
                    r.get::<_, Option<String>>(10)?,
                    r.get::<_, Option<String>>(11)?,
                    r.get::<_, Option<String>>(12)?,
                    r.get::<_, Option<String>>(13)?,
                    r.get::<_, Option<i64>>(14)?,
                    r.get::<_, i64>(15)?,
                    r.get::<_, Option<i64>>(16)?,
                    r.get::<_, Option<String>>(17)?,
                ))
            },
        )
        .optional())?
    .ok_or(Error::Denied(
        "delegation child allocation unavailable or not permitted",
    ))?;
    Ok(DelegationChildAllocationInfo {
        id: value.0,
        root_id: value.1,
        parent_session_id: value.2,
        parent_branch_id: value.3,
        parent_turn_id: value.4,
        depth: value.5,
        state: value.6,
        child_session_id: value.7,
        execution_binding_id: value.8,
        parent_allocation_id: value.9,
        routing_operation_id: value.10,
        routing_target_id: value.11,
        routing_selection_source: value.12,
        selected_identity: value.13.as_deref().map(decode).transpose()?,
        routing_bound_at: value.14,
        created_at: value.15,
        settled_at: value.16,
        error: value.17,
    })
}

pub(crate) fn require_active_parent_turn(
    c: &rusqlite::Connection,
    actor: &PrincipalId,
    parent_session: &str,
    parent_branch: &str,
    parent_turn_id: u64,
) -> Result<(crate::SessionInfo, crate::BranchInfo)> {
    if parent_turn_id == 0 || !principal_is_session_owner(c, parent_session, actor)? {
        return Err(Error::Denied(
            "subagent execution requires the native parent Session owner",
        ));
    }
    let parent = session_in(c, parent_session)?;
    let branch = branch_in(c, parent_session, parent_branch)?;
    let active_run = branch.active_run.as_deref().ok_or(Error::Conflict(
        "subagent execution admission requires the active parent Agent turn",
    ))?;
    let active_turn: Option<u64> = db(c
        .query_row(
            "SELECT turn_id FROM runs
             WHERE id=?1 AND session_id=?2 AND branch_id=?3 AND status='running'",
            params![active_run, parent_session, branch.id],
            |r| r.get(0),
        )
        .optional())?;
    if active_turn != Some(parent_turn_id) {
        return Err(Error::Conflict(
            "subagent execution admission does not match the active parent turn",
        ));
    }
    Ok((parent, branch))
}

pub(crate) fn model_only_child_run_key(
    c: &rusqlite::Connection,
    child_session: &str,
    root_key: &str,
    identity: &InvocationIdentity,
) -> Result<Option<String>> {
    Ok(
        model_only_parent_run(c, child_session, root_key, Some(identity))?
            .map(|_| format!("subagent:{root_key}:{child_session}")),
    )
}

pub(crate) fn validate_child_routing_revision(
    c: &rusqlite::Connection,
    owner_user_id: &str,
    revision_id: Option<&str>,
) -> Result<()> {
    let Some(revision_id) = revision_id else {
        return Ok(());
    };
    let valid: bool = db(c.query_row(
        "SELECT EXISTS(
            SELECT 1
            FROM routing_strategy_revisions r
            JOIN routing_strategies s ON s.id=r.strategy_id
            WHERE r.revision_id=?1
              AND r.created_by_user_id=?2
              AND s.owner_user_id=?2
              AND s.status='active'
              AND json_extract(r.definition_json,'$.role')='worker'
         )",
        params![revision_id, owner_user_id],
        |r| r.get(0),
    ))?;
    if !valid {
        return Err(Error::Denied(
            "child routing revision must be an active actor-owned worker strategy",
        ));
    }
    Ok(())
}

impl SessionStore {
    pub fn reserve_delegation_child(
        &self,
        actor: &PrincipalId,
        parent_session: &str,
        parent_branch: &str,
        parent_turn_id: u64,
        limits: DelegationLimits,
        child_routing_revision_id: Option<&str>,
        depth: u8,
    ) -> Result<(DelegationRootInfo, DelegationChildAllocationInfo)> {
        let limits = limits.validate()?;
        if depth != 1 {
            return Err(Error::Denied(
                "top-level subagent allocation depth must be one",
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (_parent, branch) =
            require_active_parent_turn(&tx, actor, parent_session, parent_branch, parent_turn_id)?;
        let user = ensure_user_for_principal(&tx, actor)?;
        validate_child_routing_revision(&tx, &user, child_routing_revision_id)?;
        let existing: Option<String> = db(tx
            .query_row(
                "SELECT id FROM delegation_roots
                 WHERE root_session_id=?1 AND root_branch_id=?2 AND root_turn_id=?3",
                params![parent_session, branch.id, parent_turn_id],
                |r| r.get(0),
            )
            .optional())?;
        let root_id = if let Some(root_id) = existing {
            let root = delegation_root_row(&tx, &root_id)?;
            if root.limits != limits
                || root.child_routing_revision_id.as_deref() != child_routing_revision_id
            {
                return Err(Error::Conflict(
                    "delegation limits or child routing revision changed during the active parent turn",
                ));
            }
            root_id
        } else {
            let root_id = id("dgr")?;
            let at = now();
            let deadline_at = at.saturating_add(i64::from(limits.deadline_seconds) * 1_000_000);
            db(tx.execute(
                "INSERT INTO delegation_roots(
                    id,root_session_id,root_branch_id,root_turn_id,created_by_user_id,state,
                    max_children_created,max_active_children,max_model_attempts,max_depth,
                    deadline_seconds,deadline_at,created_at,updated_at,recursive_max_depth,
                    child_routing_revision_id
                 ) VALUES(?1,?2,?3,?4,?5,'active',?6,?7,?8,1,?9,?10,?11,?11,?12,?13)",
                params![
                    root_id,
                    parent_session,
                    branch.id,
                    parent_turn_id,
                    user,
                    limits.max_children_created,
                    limits.max_active_children,
                    limits.max_model_attempts,
                    limits.deadline_seconds,
                    deadline_at,
                    at,
                    limits.max_depth,
                    child_routing_revision_id,
                ],
            ))?;
            root_id
        };
        let root = delegation_root_row(&tx, &root_id)?;
        if root.state != "active" || now() >= root.deadline_at {
            db(tx.execute(
                "UPDATE delegation_roots SET state='exhausted',updated_at=?1
                 WHERE id=?2 AND state='active'",
                params![now(), root_id],
            ))?;
            db(tx.commit())?;
            return Err(Error::Limit("delegation root deadline is exhausted"));
        }
        if root.children_created >= root.limits.max_children_created {
            return Err(Error::Limit("delegation child-created limit reached"));
        }
        let allocation_id = id("dga")?;
        let at = now();
        db(tx.execute(
            "INSERT INTO delegation_child_allocations(
                id,root_id,parent_session_id,parent_branch_id,parent_turn_id,depth,state,created_at,
                recursive_depth,parent_allocation_id
             ) VALUES(?1,?2,?3,?4,?5,1,'reserved',?6,1,NULL)",
            params![
                allocation_id,
                root_id,
                parent_session,
                branch.id,
                parent_turn_id,
                at
            ],
        ))?;
        let changed = db(tx.execute(
            "UPDATE delegation_roots
             SET children_created=children_created+1,updated_at=?1
             WHERE id=?2 AND children_created<max_children_created",
            params![at, root_id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict(
                "delegation child allocation raced its root limit",
            ));
        }
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "delegation.child_reserved",
            "delegation_child_allocation",
            &allocation_id,
            Some(actor),
            serde_json::json!({"root_id":root_id,"depth":depth}),
        )?;
        db(tx.commit())?;
        Ok((
            delegation_root_row(&c, &root_id)?,
            delegation_allocation_row(&c, &allocation_id)?,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn reserve_delegation_descendant(
        &self,
        actor: &PrincipalId,
        root_id: &str,
        parent_allocation_id: &str,
        parent_session: &str,
        parent_branch: &str,
        parent_turn_id: u64,
        limits: DelegationLimits,
        child_routing_revision_id: Option<&str>,
        identity: &InvocationIdentity,
    ) -> Result<(DelegationRootInfo, DelegationChildAllocationInfo)> {
        let limits = limits.validate()?;
        if identity.actor != *actor {
            return Err(Error::Denied("descendant delegation actor changed"));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (_parent, branch) =
            require_active_parent_turn(&tx, actor, parent_session, parent_branch, parent_turn_id)?;
        let root = delegation_root_row(&tx, root_id)?;
        if root.limits != limits
            || root.child_routing_revision_id.as_deref() != child_routing_revision_id
        {
            return Err(Error::Conflict(
                "delegation limits or child routing revision changed inside the recursive root",
            ));
        }
        if root.state != "active" || now() >= root.deadline_at {
            db(tx.execute(
                "UPDATE delegation_roots SET state='exhausted',updated_at=?1
                 WHERE id=?2 AND state='active'",
                params![now(), root_id],
            ))?;
            db(tx.commit())?;
            return Err(Error::Limit("delegation root deadline is exhausted"));
        }
        if root.children_created >= root.limits.max_children_created {
            return Err(Error::Limit("delegation child-created limit reached"));
        }
        let parent_allocation = delegation_allocation_row(&tx, parent_allocation_id)?;
        if parent_allocation.root_id != root_id
            || parent_allocation.child_session_id.as_deref() != Some(parent_session)
            || parent_allocation.state != "running"
        {
            return Err(Error::Denied(
                "recursive delegation requires the running direct-parent allocation",
            ));
        }
        let parent_relation: Option<(String, String, u8)> = db(tx
            .query_row(
                "SELECT invocation_identity_json,state,recursive_depth
                 FROM subagent_sessions
                 WHERE child_session_id=?1 AND delegation_root_id=?2
                   AND delegation_allocation_id=?3",
                params![parent_session, root_id, parent_allocation_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional())?;
        let Some((stored_identity_json, relation_state, relation_depth)) = parent_relation else {
            return Err(Error::Denied(
                "recursive delegation parent relation is unavailable",
            ));
        };
        let stored_identity: InvocationIdentity = decode(&stored_identity_json)?;
        if stored_identity != *identity
            || relation_state != "running"
            || relation_depth != parent_allocation.depth
        {
            return Err(Error::Denied(
                "recursive delegation cannot widen or change direct-parent authority",
            ));
        }
        let next_depth = parent_allocation
            .depth
            .checked_add(1)
            .ok_or(Error::Limit("recursive delegation depth overflow"))?;
        if next_depth > root.limits.max_depth {
            return Err(Error::Limit("delegation recursive depth limit reached"));
        }
        let allocation_id = id("dga")?;
        let at = now();
        db(tx.execute(
            "INSERT INTO delegation_child_allocations(
                id,root_id,parent_session_id,parent_branch_id,parent_turn_id,depth,state,created_at,
                recursive_depth,parent_allocation_id
             ) VALUES(?1,?2,?3,?4,?5,1,'reserved',?6,?7,?8)",
            params![
                allocation_id,
                root_id,
                parent_session,
                branch.id,
                parent_turn_id,
                at,
                next_depth,
                parent_allocation_id,
            ],
        ))?;
        let changed = db(tx.execute(
            "UPDATE delegation_roots
             SET children_created=children_created+1,updated_at=?1
             WHERE id=?2 AND state='active' AND children_created<max_children_created",
            params![at, root_id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict(
                "recursive delegation allocation raced its root limit",
            ));
        }
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "delegation.descendant_reserved",
            "delegation_child_allocation",
            &allocation_id,
            Some(actor),
            serde_json::json!({
                "root_id":root_id,
                "parent_allocation_id":parent_allocation_id,
                "depth":next_depth,
            }),
        )?;
        db(tx.commit())?;
        Ok((
            delegation_root_row(&c, root_id)?,
            delegation_allocation_row(&c, &allocation_id)?,
        ))
    }

    pub fn fail_delegation_child_allocation(
        &self,
        actor: &PrincipalId,
        allocation_id: &str,
        error: &str,
    ) -> Result<DelegationChildAllocationInfo> {
        let error = bounded_execution_error(Some(error))?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let allocation = delegation_allocation_row(&tx, allocation_id)?;
        if !principal_is_session_owner(&tx, &allocation.parent_session_id, actor)? {
            return Err(Error::Denied(
                "delegation allocation belongs to another owner",
            ));
        }
        let changed = db(tx.execute(
            "UPDATE delegation_child_allocations
             SET state='failed',settled_at=?1,error=?2
             WHERE id=?3 AND state='reserved'",
            params![now(), error, allocation_id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("delegation allocation is not reserved"));
        }
        db(tx.commit())?;
        delegation_allocation_row(&c, allocation_id)
    }

    pub fn delegation_child_allocation(
        &self,
        actor: &PrincipalId,
        allocation_id: &str,
    ) -> Result<DelegationChildAllocationInfo> {
        let c = self.connection()?;
        let allocation = delegation_allocation_row(&c, allocation_id)?;
        authorize(&c, &allocation.parent_session_id, actor, false)?;
        Ok(allocation)
    }

    pub fn delegation_root(
        &self,
        actor: &PrincipalId,
        root_id: &str,
    ) -> Result<DelegationRootInfo> {
        let c = self.connection()?;
        let root = delegation_root_row(&c, root_id)?;
        authorize(&c, &root.root_session_id, actor, false)?;
        Ok(root)
    }

    pub fn delegation_root_for_session(
        &self,
        actor: &PrincipalId,
        session_id: &str,
    ) -> Result<Option<DelegationRootInfo>> {
        let c = self.connection()?;
        authorize(&c, session_id, actor, false)?;
        let child_root: Option<String> = db(c
            .query_row(
                "SELECT delegation_root_id FROM subagent_sessions
                 WHERE child_session_id=?1 AND delegation_root_id IS NOT NULL LIMIT 1",
                [session_id],
                |r| r.get(0),
            )
            .optional())?;
        let root_id = if child_root.is_some() {
            child_root
        } else {
            db(c.query_row(
                "SELECT id FROM delegation_roots
                     WHERE root_session_id=?1 ORDER BY created_at DESC,id DESC LIMIT 1",
                [session_id],
                |r| r.get(0),
            )
            .optional())?
        };
        root_id.map(|id| delegation_root_row(&c, &id)).transpose()
    }

    pub fn visible_delegation_roots(&self, actor: &PrincipalId) -> Result<Vec<DelegationRootInfo>> {
        let c = self.connection()?;
        let actor_user = crate::user_for_principal(&c, actor)?;
        let mut query = db(c.prepare(
            "SELECT DISTINCT d.id
             FROM delegation_roots d
             JOIN sessions s ON s.id=d.root_session_id
             LEFT JOIN members m ON m.session_id=s.id
             WHERE (s.owner_user_id IS NULL AND s.owner=?1)
                OR (m.principal_user_id IS NULL AND m.principal=?1)
                OR (?2 IS NOT NULL AND (s.owner_user_id=?2 OR m.principal_user_id=?2))
             ORDER BY d.created_at DESC,d.id DESC LIMIT 512",
        ))?;
        let ids = db(query.query_map(params![actor.as_str(), actor_user], |r| {
            r.get::<_, String>(0)
        }))?;
        let ids = db(ids.collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter().map(|id| delegation_root_row(&c, id)).collect()
    }

    #[cfg(feature = "routing")]
    pub fn reserve_delegation_routing_attempt(
        &self,
        actor: &PrincipalId,
        root_id: &str,
        allocation_id: &str,
        routing_operation_id: &str,
        identity: &InvocationIdentity,
    ) -> Result<()> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let root = delegation_root_row(&tx, root_id)?;
        let user = ensure_user_for_principal(&tx, actor)?;
        if !principal_is_session_owner(&tx, &root.root_session_id, actor)?
            || identity.actor != *actor
        {
            return Err(Error::Denied(
                "delegation routing attempt belongs to another owner",
            ));
        }
        let allocation = delegation_allocation_row(&tx, allocation_id)?;
        if allocation.root_id != root_id || allocation.state != "reserved" {
            return Err(Error::Denied(
                "delegation routing attempt allocation is not reserved",
            ));
        }
        let operation: Option<(String, String, String, String, String, String)> = db(tx
            .query_row(
                "SELECT actor_user_id,session_id,branch_id,revision_id,role,state
                 FROM routing_operations WHERE id=?1",
                [routing_operation_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .optional())?;
        let Some((op_user, session, branch, revision, role, state)) = operation else {
            return Err(Error::Denied("routing operation unavailable"));
        };
        if op_user != user
            || session != allocation.parent_session_id
            || branch != allocation.parent_branch_id
            || role != "worker"
            || state != "admitted"
            || root.child_routing_revision_id.as_deref() != Some(revision.as_str())
        {
            return Err(Error::Denied(
                "routing operation does not match this child allocation/root",
            ));
        }
        let existing: Option<String> = db(tx
            .query_row(
                "SELECT classifier_identity_json FROM delegation_routing_attempts
                 WHERE allocation_id=?1",
                [allocation_id],
                |r| r.get(0),
            )
            .optional())?;
        if let Some(stored) = existing {
            let stored: InvocationIdentity = decode(&stored)?;
            if stored != *identity {
                return Err(Error::Conflict(
                    "delegation routing classifier identity changed",
                ));
            }
            return Ok(());
        }
        if root.state != "active" || now() >= root.deadline_at {
            db(tx.execute(
                "UPDATE delegation_roots SET state='exhausted',updated_at=?1
                 WHERE id=?2 AND state='active'",
                params![now(), root_id],
            ))?;
            db(tx.commit())?;
            return Err(Error::Limit("delegation root deadline is exhausted"));
        }
        if root.model_attempts_started >= root.limits.max_model_attempts {
            return Err(Error::Limit("delegation model-attempt limit reached"));
        }
        let at = now();
        db(tx.execute(
            "INSERT INTO delegation_routing_attempts(
                root_id,allocation_id,routing_operation_id,classifier_identity_json,state,started_at
             ) VALUES(?1,?2,?3,?4,'started',?5)",
            params![
                root_id,
                allocation_id,
                routing_operation_id,
                json(identity)?,
                at
            ],
        ))?;
        let changed = db(tx.execute(
            "UPDATE delegation_roots
             SET model_attempts_started=model_attempts_started+1,updated_at=?1
             WHERE id=?2 AND model_attempts_started<max_model_attempts",
            params![at, root_id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict(
                "delegation routing attempt raced its root limit",
            ));
        }
        db(tx.commit())?;
        Ok(())
    }

    #[cfg(feature = "routing")]
    pub fn mark_delegation_routing_attempt_unknown(
        &self,
        actor: &PrincipalId,
        allocation_id: &str,
        reason: &str,
    ) -> Result<()> {
        let reason = bounded_execution_error(Some(reason))?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let allocation = delegation_allocation_row(&tx, allocation_id)?;
        let root = delegation_root_row(&tx, &allocation.root_id)?;
        if !principal_is_session_owner(&tx, &root.root_session_id, actor)? {
            return Err(Error::Denied(
                "delegation routing attempt belongs to another owner",
            ));
        }
        let changed = db(tx.execute(
            "UPDATE delegation_routing_attempts
             SET state='unknown',outcome=?1,finished_at=?2
             WHERE allocation_id=?3 AND state='started'",
            params![reason, now(), allocation_id],
        ))?;
        if changed != 1 {
            let state: Option<String> = db(tx
                .query_row(
                    "SELECT state FROM delegation_routing_attempts WHERE allocation_id=?1",
                    [allocation_id],
                    |r| r.get(0),
                )
                .optional())?;
            if state.as_deref() != Some("unknown") {
                return Err(Error::Conflict(
                    "delegation routing attempt is not markable unknown",
                ));
            }
        }
        db(tx.commit())?;
        Ok(())
    }

    #[cfg(feature = "routing")]
    pub fn finish_delegation_routing_attempt(
        &self,
        actor: &PrincipalId,
        allocation_id: &str,
        identity: &InvocationIdentity,
        result: &RoutingResult,
    ) -> Result<()> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let allocation = delegation_allocation_row(&tx, allocation_id)?;
        let root = delegation_root_row(&tx, &allocation.root_id)?;
        if !principal_is_session_owner(&tx, &root.root_session_id, actor)? {
            return Err(Error::Denied(
                "delegation routing attempt belongs to another owner",
            ));
        }
        let row: Option<(String, String)> = db(tx
            .query_row(
                "SELECT state,classifier_identity_json FROM delegation_routing_attempts
                 WHERE allocation_id=?1",
                [allocation_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional())?;
        let Some((state, identity_json)) = row else {
            return Err(Error::Conflict(
                "delegation routing attempt was not reserved",
            ));
        };
        let stored: InvocationIdentity = decode(&identity_json)?;
        if stored != *identity {
            return Err(Error::Conflict(
                "delegation routing classifier identity changed",
            ));
        }
        if state == "finished" {
            return Ok(());
        }
        if state != "started" {
            return Err(Error::Conflict(
                "delegation routing attempt is not settleable",
            ));
        }
        let usage = result.meta.as_ref().map(|meta| &meta.usage);
        let input = usage.and_then(|usage| usage.input_tokens);
        let output = usage.and_then(|usage| usage.output_tokens);
        let complete = input.is_some() && output.is_some();
        let outcome = match result.outcome {
            aw_agent_core::Outcome::Completed => "completed",
            aw_agent_core::Outcome::Cancelled => "cancelled",
            aw_agent_core::Outcome::Failed => "failed",
            aw_agent_core::Outcome::Incomplete => "incomplete",
        };
        let at = now();
        db(tx.execute(
            "UPDATE delegation_routing_attempts
             SET state='finished',outcome=?1,metadata_json=?2,finished_at=?3
             WHERE allocation_id=?4 AND state='started'",
            params![
                outcome,
                result.meta.as_ref().map(json).transpose()?,
                at,
                allocation_id
            ],
        ))?;
        db(tx.execute(
            "UPDATE delegation_roots
             SET model_attempts_finished=model_attempts_finished+1,
                 input_tokens_known=input_tokens_known+?1,
                 output_tokens_known=output_tokens_known+?2,
                 calls_without_complete_usage=calls_without_complete_usage+?3,
                 updated_at=?4
             WHERE id=?5",
            params![
                input.unwrap_or(0),
                output.unwrap_or(0),
                i64::from(!complete),
                at,
                root.id
            ],
        ))?;
        db(tx.commit())?;
        Ok(())
    }

    pub fn reserve_delegation_model_attempt(
        &self,
        actor: &PrincipalId,
        root_id: &str,
        child_session_id: &str,
        turn_id: u64,
        ordinal: u32,
        identity: &InvocationIdentity,
    ) -> Result<()> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let root = delegation_root_row(&tx, root_id)?;
        if !principal_is_session_owner(&tx, &root.root_session_id, actor)?
            || identity.actor != *actor
        {
            return Err(Error::Denied(
                "delegation model attempt belongs to another owner",
            ));
        }
        let linked: bool = db(tx.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM subagent_sessions s
                JOIN delegation_child_allocations a ON a.id=s.delegation_allocation_id
                WHERE s.child_session_id=?1 AND s.delegation_root_id=?2
                  AND s.state='running' AND a.state='running'
             )",
            params![child_session_id, root_id],
            |r| r.get(0),
        ))?;
        if !linked {
            return Err(Error::Denied(
                "child is not a running member of this delegation root",
            ));
        }
        let existing: Option<String> = db(tx
            .query_row(
                "SELECT invocation_identity_json FROM delegation_model_attempts
                 WHERE root_id=?1 AND child_session_id=?2 AND turn_id=?3 AND ordinal=?4",
                params![root_id, child_session_id, turn_id, ordinal],
                |r| r.get(0),
            )
            .optional())?;
        if let Some(stored) = existing {
            let stored: InvocationIdentity = decode(&stored)?;
            if stored != *identity {
                return Err(Error::Conflict("delegation attempt identity changed"));
            }
            return Ok(());
        }
        if root.state != "active" || now() >= root.deadline_at {
            db(tx.execute(
                "UPDATE delegation_roots SET state='exhausted',updated_at=?1
                 WHERE id=?2 AND state='active'",
                params![now(), root_id],
            ))?;
            db(tx.commit())?;
            return Err(Error::Limit("delegation root deadline is exhausted"));
        }
        if root.model_attempts_started >= root.limits.max_model_attempts {
            return Err(Error::Limit("delegation model-attempt limit reached"));
        }
        let at = now();
        db(tx.execute(
            "INSERT INTO delegation_model_attempts(
                root_id,child_session_id,turn_id,ordinal,invocation_identity_json,state,started_at
             ) VALUES(?1,?2,?3,?4,?5,'started',?6)",
            params![
                root_id,
                child_session_id,
                turn_id,
                ordinal,
                json(identity)?,
                at
            ],
        ))?;
        let changed = db(tx.execute(
            "UPDATE delegation_roots
             SET model_attempts_started=model_attempts_started+1,updated_at=?1
             WHERE id=?2 AND model_attempts_started<max_model_attempts",
            params![at, root_id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict(
                "delegation model-attempt reservation raced its limit",
            ));
        }
        db(tx.commit())?;
        Ok(())
    }

    pub fn mark_delegation_model_attempt_unknown(
        &self,
        actor: &PrincipalId,
        root_id: &str,
        child_session_id: &str,
        turn_id: u64,
        ordinal: u32,
        reason: &str,
    ) -> Result<()> {
        let reason = bounded_execution_error(Some(reason))?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let root = delegation_root_row(&tx, root_id)?;
        if !principal_is_session_owner(&tx, &root.root_session_id, actor)? {
            return Err(Error::Denied(
                "delegation model attempt belongs to another owner",
            ));
        }
        let changed = db(tx.execute(
            "UPDATE delegation_model_attempts
             SET state='unknown',outcome=?1,finished_at=?2
             WHERE root_id=?3 AND child_session_id=?4 AND turn_id=?5 AND ordinal=?6
               AND state='started'",
            params![reason, now(), root_id, child_session_id, turn_id, ordinal],
        ))?;
        if changed != 1 {
            let state: Option<String> = db(tx
                .query_row(
                    "SELECT state FROM delegation_model_attempts
                     WHERE root_id=?1 AND child_session_id=?2 AND turn_id=?3 AND ordinal=?4",
                    params![root_id, child_session_id, turn_id, ordinal],
                    |r| r.get(0),
                )
                .optional())?;
            if state.as_deref() != Some("unknown") {
                return Err(Error::Conflict(
                    "delegation model attempt is not markable unknown",
                ));
            }
        }
        db(tx.commit())?;
        Ok(())
    }

    pub fn finish_delegation_model_attempt(
        &self,
        actor: &PrincipalId,
        root_id: &str,
        child_session_id: &str,
        turn_id: u64,
        record: &InvocationRecord,
    ) -> Result<()> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let root = delegation_root_row(&tx, root_id)?;
        if !principal_is_session_owner(&tx, &root.root_session_id, actor)? {
            return Err(Error::Denied(
                "delegation model attempt belongs to another owner",
            ));
        }
        let row: Option<(String, String)> = db(tx
            .query_row(
                "SELECT state,invocation_identity_json FROM delegation_model_attempts
                 WHERE root_id=?1 AND child_session_id=?2 AND turn_id=?3 AND ordinal=?4",
                params![root_id, child_session_id, turn_id, record.ordinal],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional())?;
        let Some((state, identity_json)) = row else {
            return Err(Error::Conflict("delegation model attempt was not reserved"));
        };
        let stored: InvocationIdentity = decode(&identity_json)?;
        if stored != record.identity {
            return Err(Error::Conflict("delegation model attempt identity changed"));
        }
        if state == "finished" {
            return Ok(());
        }
        if state != "started" {
            return Err(Error::Conflict(
                "delegation model attempt is not settleable",
            ));
        }
        let usage = record.meta.as_ref().map(|meta| &meta.usage);
        let input = usage.and_then(|usage| usage.input_tokens);
        let output = usage.and_then(|usage| usage.output_tokens);
        let complete = input.is_some() && output.is_some();
        let at = now();
        db(tx.execute(
            "UPDATE delegation_model_attempts
             SET state='finished',outcome=?1,input_tokens=?2,output_tokens=?3,
                 usage_complete=?4,finished_at=?5
             WHERE root_id=?6 AND child_session_id=?7 AND turn_id=?8 AND ordinal=?9 AND state='started'",
            params![
                record.outcome,
                input,
                output,
                i64::from(complete),
                at,
                root_id,
                child_session_id,
                turn_id,
                record.ordinal,
            ],
        ))?;
        db(tx.execute(
            "UPDATE delegation_roots
             SET model_attempts_finished=model_attempts_finished+1,
                 input_tokens_known=input_tokens_known+?1,
                 output_tokens_known=output_tokens_known+?2,
                 calls_without_complete_usage=calls_without_complete_usage+?3,
                 updated_at=?4
             WHERE id=?5",
            params![
                input.unwrap_or(0),
                output.unwrap_or(0),
                i64::from(!complete),
                at,
                root_id
            ],
        ))?;
        db(tx.commit())?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn plan_subagent_execution_binding(
        &self,
        actor: &PrincipalId,
        binding_id: &str,
        parent_session: &str,
        parent_branch: &str,
        parent_turn_id: u64,
        delegation_root_id: &str,
        delegation_allocation_id: &str,
        tool_mode: &str,
        parent_repo_root: &Path,
        execution_root: &Path,
        base_revision: &str,
    ) -> Result<SubagentExecutionBindingInfo> {
        label(binding_id, 160)?;
        if !matches!(tool_mode, "read-only" | "workspace") {
            return Err(Error::Denied(
                "Phase 42 managed children allow only read-only or workspace file tools",
            ));
        }
        validate_revision(base_revision)?;
        let parent_repo_bytes = path_bytes(parent_repo_root)?;
        let execution_bytes = path_bytes(execution_root)?;
        if parent_repo_root == execution_root || execution_root.starts_with(parent_repo_root) {
            return Err(Error::Denied(
                "managed child worktree must be outside the parent repository root",
            ));
        }

        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (parent, branch) =
            require_active_parent_turn(&tx, actor, parent_session, parent_branch, parent_turn_id)?;
        let allocation = delegation_allocation_row(&tx, delegation_allocation_id)?;
        if allocation.root_id != delegation_root_id
            || allocation.parent_session_id != parent_session
            || allocation.parent_branch_id != branch.id
            || allocation.parent_turn_id != parent_turn_id
            || allocation.state != "reserved"
            || allocation.child_session_id.is_some()
        {
            return Err(Error::Conflict(
                "managed worktree does not match the reserved delegation allocation",
            ));
        }
        for path in [&parent.binding.working_directory, &parent.binding.tool_root] {
            if !path.is_absolute() || path.strip_prefix(parent_repo_root).is_err() {
                return Err(Error::Denied(
                    "parent working/tool roots must be inside the managed Git repository",
                ));
            }
        }
        let user = ensure_user_for_principal(&tx, actor)?;
        let created = now();
        db(tx.execute(
            "INSERT INTO subagent_execution_bindings(
                id,parent_session_id,parent_branch_id,parent_turn_id,created_by_user_id,
                kind,tool_mode,parent_repo_root,execution_root,base_revision,state,created_at,
                delegation_root_id,delegation_allocation_id
             ) VALUES(?1,?2,?3,?4,?5,'managed_worktree',?6,?7,?8,?9,'planned',?10,?11,?12)",
            params![
                binding_id,
                parent_session,
                branch.id,
                parent_turn_id,
                user,
                tool_mode,
                parent_repo_bytes,
                execution_bytes,
                base_revision,
                created,
                delegation_root_id,
                delegation_allocation_id,
            ],
        ))?;
        let linked = db(tx.execute(
            "UPDATE delegation_child_allocations SET execution_binding_id=?1
             WHERE id=?2 AND root_id=?3 AND state='reserved' AND execution_binding_id IS NULL",
            params![binding_id, delegation_allocation_id, delegation_root_id],
        ))?;
        if linked != 1 {
            return Err(Error::Conflict(
                "delegation allocation could not be linked to managed worktree intent",
            ));
        }
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "subagent.execution_planned",
            "subagent_execution_binding",
            binding_id,
            Some(actor),
            serde_json::json!({
                "parent_session_id": parent_session,
                "parent_branch_id": branch.id,
                "parent_turn_id": parent_turn_id,
                "kind": "managed_worktree",
                "tool_mode": tool_mode,
                "base_revision": base_revision,
                "delegation_root_id": delegation_root_id,
                "delegation_allocation_id": delegation_allocation_id,
            }),
        )?;
        db(tx.commit())?;
        execution_row(&c, binding_id)
    }

    pub fn mark_subagent_execution_ready(
        &self,
        actor: &PrincipalId,
        binding_id: &str,
    ) -> Result<SubagentExecutionBindingInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let info = execution_row(&tx, binding_id)?;
        if !principal_is_session_owner(&tx, &info.parent_session_id, actor)? {
            return Err(Error::Denied(
                "subagent execution binding belongs to another owner",
            ));
        }
        let changed = db(tx.execute(
            "UPDATE subagent_execution_bindings SET state='ready',ready_at=?1,error=NULL
             WHERE id=?2 AND state='planned'",
            params![now(), binding_id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("subagent execution binding is not planned"));
        }
        db(tx.commit())?;
        execution_row(&c, binding_id)
    }

    pub fn mark_subagent_execution_unknown(
        &self,
        actor: &PrincipalId,
        binding_id: &str,
        error: &str,
    ) -> Result<SubagentExecutionBindingInfo> {
        let error = bounded_execution_error(Some(error))?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let info = execution_row(&tx, binding_id)?;
        if !principal_is_session_owner(&tx, &info.parent_session_id, actor)? {
            return Err(Error::Denied(
                "subagent execution binding belongs to another owner",
            ));
        }
        let changed = db(tx.execute(
            "UPDATE subagent_execution_bindings SET state='unknown',error=?1
             WHERE id=?2 AND state IN ('planned','ready','cleanup_planned')",
            params![error, binding_id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict(
                "subagent execution binding cannot become unknown from its current state",
            ));
        }
        db(tx.commit())?;
        execution_row(&c, binding_id)
    }

    pub fn subagent_execution_binding(
        &self,
        actor: &PrincipalId,
        binding_id: &str,
    ) -> Result<SubagentExecutionBindingInfo> {
        let c = self.connection()?;
        let info = execution_row(&c, binding_id)?;
        authorize(&c, &info.parent_session_id, actor, false)?;
        Ok(info)
    }

    pub fn subagent_execution_binding_for_subagent(
        &self,
        actor: &PrincipalId,
        subagent_id: &str,
    ) -> Result<Option<SubagentExecutionBindingInfo>> {
        let c = self.connection()?;
        let relation = row(&c, subagent_id)?;
        authorize(&c, &relation.parent_session_id, actor, false)?;
        let binding_id: Option<String> = db(c
            .query_row(
                "SELECT id FROM subagent_execution_bindings WHERE child_session_id=?1",
                [&relation.child_session_id],
                |r| r.get(0),
            )
            .optional())?;
        binding_id.map(|id| execution_row(&c, &id)).transpose()
    }

    pub fn subagent_execution_binding_for_child_session(
        &self,
        actor: &PrincipalId,
        child_session_id: &str,
    ) -> Result<Option<SubagentExecutionBindingInfo>> {
        let c = self.connection()?;
        authorize(&c, child_session_id, actor, false)?;
        let binding_id: Option<String> = db(c
            .query_row(
                "SELECT id FROM subagent_execution_bindings WHERE child_session_id=?1",
                [child_session_id],
                |r| r.get(0),
            )
            .optional())?;
        binding_id.map(|id| execution_row(&c, &id)).transpose()
    }

    pub fn visible_subagent_execution_bindings(
        &self,
        actor: &PrincipalId,
    ) -> Result<Vec<SubagentExecutionBindingInfo>> {
        let c = self.connection()?;
        let actor_user = crate::user_for_principal(&c, actor)?;
        let mut query = db(c.prepare(
            "SELECT DISTINCT e.id
             FROM subagent_execution_bindings e
             JOIN sessions s ON s.id=e.parent_session_id
             LEFT JOIN members m ON m.session_id=s.id
             WHERE (s.owner_user_id IS NULL AND s.owner=?1)
                OR (m.principal_user_id IS NULL AND m.principal=?1)
                OR (?2 IS NOT NULL AND (s.owner_user_id=?2 OR m.principal_user_id=?2))
             ORDER BY e.created_at,e.id LIMIT 512",
        ))?;
        let ids = db(query.query_map(params![actor.as_str(), actor_user], |r| {
            r.get::<_, String>(0)
        }))?;
        let ids = db(ids.collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter().map(|id| execution_row(&c, id)).collect()
    }

    pub fn begin_subagent_execution_cleanup(
        &self,
        actor: &PrincipalId,
        target_id: &str,
    ) -> Result<SubagentExecutionBindingInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let direct_binding: Option<String> = db(tx
            .query_row(
                "SELECT id FROM subagent_execution_bindings WHERE id=?1",
                [target_id],
                |r| r.get(0),
            )
            .optional())?;
        let binding_id = if let Some(binding_id) = direct_binding {
            binding_id
        } else {
            let relation = row(&tx, target_id)?;
            if matches!(relation.state.as_str(), "admitted" | "running") {
                return Err(Error::Conflict("cannot clean a running subagent worktree"));
            }
            db(tx
                .query_row(
                    "SELECT id FROM subagent_execution_bindings WHERE child_session_id=?1",
                    [&relation.child_session_id],
                    |r| r.get(0),
                )
                .optional())?
            .ok_or(Error::Denied("subagent has no managed execution binding"))?
        };
        let info = execution_row(&tx, &binding_id)?;
        if !principal_is_session_owner(&tx, &info.parent_session_id, actor)? {
            return Err(Error::Denied(
                "only the parent Session owner can clean subagent worktrees",
            ));
        }
        if let Some(child_session) = info.child_session_id.as_deref() {
            let relation_state: Option<String> = db(tx
                .query_row(
                    "SELECT state FROM subagent_sessions WHERE child_session_id=?1",
                    [child_session],
                    |r| r.get(0),
                )
                .optional())?;
            if relation_state
                .as_deref()
                .is_some_and(|state| matches!(state, "admitted" | "running"))
            {
                return Err(Error::Conflict("cannot clean a running subagent worktree"));
            }
        }
        let changed = db(tx.execute(
            "UPDATE subagent_execution_bindings
             SET state='cleanup_planned',cleanup_started_at=?1,error=NULL
             WHERE id=?2 AND state IN ('ready','bound')",
            params![now(), binding_id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict(
                "subagent execution binding is not cleanable",
            ));
        }
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "subagent.execution_cleanup_planned",
            "subagent_execution_binding",
            &binding_id,
            Some(actor),
            serde_json::json!({"target_id":target_id}),
        )?;
        db(tx.commit())?;
        execution_row(&c, &binding_id)
    }

    pub fn cancel_subagent_execution_cleanup(
        &self,
        actor: &PrincipalId,
        binding_id: &str,
    ) -> Result<SubagentExecutionBindingInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let info = execution_row(&tx, binding_id)?;
        if !principal_is_session_owner(&tx, &info.parent_session_id, actor)? {
            return Err(Error::Denied(
                "subagent execution binding belongs to another owner",
            ));
        }
        let next_state = if info.child_session_id.is_some() {
            "bound"
        } else {
            "ready"
        };
        let changed = db(tx.execute(
            "UPDATE subagent_execution_bindings
             SET state=?1,cleanup_started_at=NULL,error=NULL
             WHERE id=?2 AND state='cleanup_planned'",
            params![next_state, binding_id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("subagent cleanup intent is not active"));
        }
        db(tx.commit())?;
        execution_row(&c, binding_id)
    }

    pub fn finish_subagent_execution_cleanup(
        &self,
        actor: &PrincipalId,
        binding_id: &str,
    ) -> Result<SubagentExecutionBindingInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let info = execution_row(&tx, binding_id)?;
        if !principal_is_session_owner(&tx, &info.parent_session_id, actor)? {
            return Err(Error::Denied(
                "subagent execution binding belongs to another owner",
            ));
        }
        let changed = db(tx.execute(
            "UPDATE subagent_execution_bindings
             SET state='removed',removed_at=?1,error=NULL
             WHERE id=?2 AND state='cleanup_planned'",
            params![now(), binding_id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("subagent cleanup intent is not active"));
        }
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "subagent.execution_removed",
            "subagent_execution_binding",
            binding_id,
            Some(actor),
            serde_json::json!({}),
        )?;
        db(tx.commit())?;
        execution_row(&c, binding_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_subagent_session(
        &self,
        actor: &PrincipalId,
        parent_session: &str,
        parent_branch: &str,
        parent_turn_id: u64,
        title: &str,
        task_sha256: &str,
        identity: &InvocationIdentity,
        delegation_root_id: &str,
        delegation_allocation_id: &str,
        depth: u8,
        tool_mode: &str,
        execution_binding_id: Option<&str>,
        mode: SubagentMode,
    ) -> Result<(SubagentInfo, SessionInfo, crate::BranchInfo)> {
        label(title, 160)?;
        validate_sha256(task_sha256)?;
        validate_tool_mode(tool_mode)?;
        if tool_mode == "trusted" {
            return Err(Error::Denied(
                "Phase 42 does not grant trusted shell tools to child Agents",
            ));
        }
        if parent_turn_id == 0 || identity.actor != *actor {
            return Err(Error::Denied(
                "subagent admission must use the exact current actor/parent turn",
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (parent, branch) =
            require_active_parent_turn(&tx, actor, parent_session, parent_branch, parent_turn_id)?;
        let root = delegation_root_row(&tx, delegation_root_id)?;
        let allocation = delegation_allocation_row(&tx, delegation_allocation_id)?;
        if root.state != "active"
            || now() >= root.deadline_at
            || depth == 0
            || depth > root.limits.max_depth
            || allocation.root_id != delegation_root_id
            || allocation.parent_session_id != parent_session
            || allocation.parent_branch_id != branch.id
            || allocation.parent_turn_id != parent_turn_id
            || allocation.depth != depth
            || allocation.state != "reserved"
            || allocation.child_session_id.is_some()
        {
            return Err(Error::Conflict(
                "child Session does not match its reserved delegation allocation",
            ));
        }
        if depth == 1 {
            if allocation.parent_allocation_id.is_some()
                || root.root_session_id != parent_session
                || root.root_branch_id != branch.id
                || root.root_turn_id != parent_turn_id
            {
                return Err(Error::Conflict(
                    "root child allocation does not match the root parent turn",
                ));
            }
        } else {
            let parent_allocation_id =
                allocation
                    .parent_allocation_id
                    .as_deref()
                    .ok_or(Error::Conflict(
                        "recursive child allocation is missing parent lineage",
                    ))?;
            let parent_allocation = delegation_allocation_row(&tx, parent_allocation_id)?;
            if parent_allocation.root_id != delegation_root_id
                || parent_allocation.child_session_id.as_deref() != Some(parent_session)
                || parent_allocation.state != "running"
                || parent_allocation.depth.saturating_add(1) != depth
            {
                return Err(Error::Denied(
                    "recursive child direct-parent allocation is not running in the same root",
                ));
            }
            let parent_relation: Option<(String, String, u8)> = db(tx
                .query_row(
                    "SELECT invocation_identity_json,state,recursive_depth
                     FROM subagent_sessions
                     WHERE child_session_id=?1 AND delegation_root_id=?2
                       AND delegation_allocation_id=?3",
                    params![parent_session, delegation_root_id, parent_allocation_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional())?;
            let Some((stored_identity_json, parent_state, parent_depth)) = parent_relation else {
                return Err(Error::Denied(
                    "recursive child direct-parent relation is unavailable",
                ));
            };
            let stored_identity: InvocationIdentity = decode(&stored_identity_json)?;
            if parent_state != "running" || parent_depth + 1 != depth {
                return Err(Error::Denied(
                    "recursive child direct-parent relation is not running at the expected depth",
                ));
            }
            if root.child_routing_revision_id.is_none() && stored_identity != *identity {
                return Err(Error::Denied(
                    "inherit-mode recursive child cannot change direct-parent account/model authority",
                ));
            }
        }
        if root.child_routing_revision_id.is_some() {
            let selected = allocation
                .selected_identity
                .as_ref()
                .ok_or(Error::Conflict(
                    "routed child allocation has no selected working identity",
                ))?;
            if selected != identity
                || allocation.routing_operation_id.is_none()
                || allocation.routing_target_id.is_none()
                || !matches!(
                    allocation.routing_selection_source.as_deref(),
                    Some("fixed" | "affinity" | "classifier")
                )
            {
                return Err(Error::Denied(
                    "child Session identity differs from its consumed routing selection",
                ));
            }
        } else {
            if allocation.routing_operation_id.is_some()
                || allocation.routing_target_id.is_some()
                || allocation.selected_identity.is_some()
                || allocation.routing_selection_source.is_some()
            {
                return Err(Error::Conflict(
                    "inherit-mode child allocation unexpectedly contains routing selection",
                ));
            }
            if depth == 1 {
                let active_run = branch.active_run.as_deref().ok_or(Error::Conflict(
                    "inherit-mode child requires the active root parent run",
                ))?;
                let parent_identity_json: String = db(tx.query_row(
                    "SELECT identity_json FROM runs WHERE id=?1 AND status='running'",
                    [active_run],
                    |r| r.get(0),
                ))?;
                let parent_identity: InvocationIdentity = decode(&parent_identity_json)?;
                if parent_identity != *identity {
                    return Err(Error::Denied(
                        "inherit-mode child must keep the exact root parent account/model identity",
                    ));
                }
            }
            let changed = db(tx.execute(
                "UPDATE delegation_child_allocations
                 SET routing_selection_source='inherit',selected_identity_json=?1,routing_bound_at=?2
                 WHERE id=?3 AND state='reserved' AND selected_identity_json IS NULL",
                params![json(identity)?, now(), delegation_allocation_id],
            ))?;
            if changed != 1 {
                return Err(Error::Conflict(
                    "inherit-mode child identity was bound concurrently",
                ));
            }
        }
        let child_destination_fingerprint = if let Some(revision_id) =
            root.child_routing_revision_id.as_deref()
        {
            #[cfg(feature = "routing")]
            {
                let target_id = allocation
                    .routing_target_id
                    .as_deref()
                    .ok_or(Error::Conflict(
                        "routed child allocation has no selected target",
                    ))?;
                crate::routing::routing_target_destination_fingerprint(&tx, revision_id, target_id)?
            }
            #[cfg(not(feature = "routing"))]
            {
                let _ = revision_id;
                return Err(Error::Denied(
                    "this build cannot materialize a routed child Session",
                ));
            }
        } else {
            parent.binding.destination_fingerprint.clone()
        };
        let user = ensure_user_for_principal(&tx, actor)?;
        let target = resources::session_target_in(&tx, parent_session)?;
        let binding = if tool_mode == "off" {
            if execution_binding_id.is_some() {
                return Err(Error::Denied(
                    "model-only child cannot consume a managed execution binding",
                ));
            }
            SessionBinding {
                profile_hint: identity.account_id.clone(),
                model_hint: identity.model.clone(),
                destination_fingerprint: child_destination_fingerprint.clone(),
                ..parent.binding.clone()
            }
        } else {
            let binding_id = execution_binding_id.ok_or(Error::Denied(
                "tool-enabled child requires a ready managed execution binding",
            ))?;
            let execution = execution_row(&tx, binding_id)?;
            if execution.state != "ready"
                || execution.parent_session_id != parent_session
                || execution.parent_branch_id != branch.id
                || execution.parent_turn_id != parent_turn_id
                || execution.tool_mode != tool_mode
                || execution.child_session_id.is_some()
                || execution.delegation_root_id.as_deref() != Some(delegation_root_id)
                || execution.delegation_allocation_id.as_deref() != Some(delegation_allocation_id)
            {
                return Err(Error::Conflict(
                    "managed child execution binding does not match this exact parent turn",
                ));
            }
            let cwd_relative = parent
                .binding
                .working_directory
                .strip_prefix(&execution.parent_repo_root)
                .map_err(|_| Error::Denied("parent cwd moved outside managed repository"))?;
            let tool_relative = parent
                .binding
                .tool_root
                .strip_prefix(&execution.parent_repo_root)
                .map_err(|_| Error::Denied("parent tool root moved outside managed repository"))?;
            SessionBinding {
                working_directory: execution.execution_root.join(cwd_relative),
                tool_root: execution.execution_root.join(tool_relative),
                profile_hint: identity.account_id.clone(),
                model_hint: identity.model.clone(),
                system_prompt: parent.binding.system_prompt.clone(),
                destination_fingerprint: child_destination_fingerprint.clone(),
            }
        };
        let (child, child_branch) = resources::create_in_workspace_tx(
            &tx,
            actor,
            title,
            &binding,
            &parent.workspace_id,
            Some(target),
        )?;
        if let Some(binding_id) = execution_binding_id {
            let changed = db(tx.execute(
                "UPDATE subagent_execution_bindings
                 SET state='bound',child_session_id=?1,bound_at=?2,error=NULL
                 WHERE id=?3 AND state='ready' AND child_session_id IS NULL",
                params![child.id, now(), binding_id],
            ))?;
            if changed != 1 {
                return Err(Error::Conflict(
                    "managed child execution binding was consumed concurrently",
                ));
            }
        }
        let allocation_bound = db(tx.execute(
            "UPDATE delegation_child_allocations
             SET state='bound',child_session_id=?1,bound_at=?2
             WHERE id=?3 AND root_id=?4 AND state='reserved' AND child_session_id IS NULL",
            params![
                child.id,
                now(),
                delegation_allocation_id,
                delegation_root_id
            ],
        ))?;
        if allocation_bound != 1 {
            return Err(Error::Conflict(
                "delegation child allocation was consumed concurrently",
            ));
        }
        let subagent_id = id("sag")?;
        let created = now();
        db(tx.execute(
            "INSERT INTO subagent_sessions(
                id,parent_session_id,parent_branch_id,parent_head,parent_branch_revision,
                parent_turn_id,child_session_id,created_by_user_id,mode,state,task_sha256,
                invocation_identity_json,tool_mode,created_at,
                delegation_root_id,delegation_allocation_id,depth,recursive_depth
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'admitted',?10,?11,?12,?13,?14,?15,1,?16)",
            params![
                subagent_id,
                parent_session,
                branch.id,
                branch.head,
                branch.revision,
                parent_turn_id,
                child.id,
                user,
                mode.as_str(),
                task_sha256,
                json(identity)?,
                tool_mode,
                created,
                delegation_root_id,
                delegation_allocation_id,
                depth,
            ],
        ))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "subagent.admitted",
            "subagent",
            &subagent_id,
            Some(actor),
            serde_json::json!({
                "parent_session_id": parent_session,
                "parent_branch_id": branch.id,
                "parent_turn_id": parent_turn_id,
                "child_session_id": child.id,
                "mode": mode.as_str(),
                "account_id": identity.account_id,
                "model": identity.model,
                "tool_mode": tool_mode,
                "execution_binding_id": execution_binding_id,
                "delegation_root_id": delegation_root_id,
                "delegation_allocation_id": delegation_allocation_id,
                "depth": depth,
            }),
        )?;
        db(tx.commit())?;
        let info = row(&c, &subagent_id)?;
        Ok((info, child, child_branch))
    }

    pub fn subagent(&self, actor: &PrincipalId, id: &str) -> Result<SubagentInfo> {
        let c = self.connection()?;
        let info = row(&c, id)?;
        authorize(&c, &info.parent_session_id, actor, false)?;
        Ok(info)
    }

    pub fn subagents_for_parent(
        &self,
        actor: &PrincipalId,
        parent_session: &str,
    ) -> Result<Vec<SubagentInfo>> {
        let c = self.connection()?;
        authorize(&c, parent_session, actor, false)?;
        let mut query = db(c.prepare(
            "SELECT id FROM subagent_sessions
             WHERE parent_session_id=?1 ORDER BY created_at,id LIMIT 64",
        ))?;
        let ids = db(query.query_map([parent_session], |r| r.get::<_, String>(0)))?;
        let ids = db(ids.collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter().map(|id| row(&c, id)).collect()
    }

    pub fn visible_subagents(&self, actor: &PrincipalId) -> Result<Vec<SubagentInfo>> {
        let c = self.connection()?;
        let actor_user = crate::user_for_principal(&c, actor)?;
        let mut query = db(c.prepare(
            "SELECT DISTINCT a.id
             FROM subagent_sessions a
             JOIN sessions s ON s.id=a.parent_session_id
             LEFT JOIN members m ON m.session_id=s.id
             WHERE (s.owner_user_id IS NULL AND s.owner=?1)
                OR (m.principal_user_id IS NULL AND m.principal=?1)
                OR (?2 IS NOT NULL AND (s.owner_user_id=?2 OR m.principal_user_id=?2))
             ORDER BY a.created_at,a.id LIMIT 512",
        ))?;
        let ids = db(query.query_map(params![actor.as_str(), actor_user], |r| {
            r.get::<_, String>(0)
        }))?;
        let ids = db(ids.collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter().map(|id| row(&c, id)).collect()
    }

    pub fn mark_subagent_running(&self, actor: &PrincipalId, id: &str) -> Result<SubagentInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let info = row(&tx, id)?;
        if !principal_is_session_owner(&tx, &info.parent_session_id, actor)?
            || info.identity.actor != *actor
        {
            return Err(Error::Denied("subagent lifecycle belongs to another actor"));
        }
        let at = now();
        if let (Some(root_id), Some(allocation_id)) = (
            info.delegation_root_id.as_deref(),
            info.delegation_allocation_id.as_deref(),
        ) {
            let root = delegation_root_row(&tx, root_id)?;
            if root.state != "active" || at >= root.deadline_at {
                db(tx.execute(
                    "UPDATE delegation_roots SET state='exhausted',updated_at=?1
                     WHERE id=?2 AND state='active'",
                    params![at, root_id],
                ))?;
                db(tx.commit())?;
                return Err(Error::Limit("delegation root deadline is exhausted"));
            }
            if root.active_children >= root.limits.max_active_children {
                return Err(Error::Limit("delegation active-child limit reached"));
            }
            let allocation_changed = db(tx.execute(
                "UPDATE delegation_child_allocations
                 SET state='running',running_at=?1
                 WHERE id=?2 AND root_id=?3 AND child_session_id=?4 AND state='bound'",
                params![at, allocation_id, root_id, info.child_session_id],
            ))?;
            if allocation_changed != 1 {
                return Err(Error::Conflict("delegation allocation is not bound"));
            }
            let root_changed = db(tx.execute(
                "UPDATE delegation_roots
                 SET active_children=active_children+1,updated_at=?1
                 WHERE id=?2 AND state='active' AND active_children<max_active_children",
                params![at, root_id],
            ))?;
            if root_changed != 1 {
                return Err(Error::Conflict(
                    "delegation active-child reservation raced its limit",
                ));
            }
        }
        let changed = db(tx.execute(
            "UPDATE subagent_sessions SET state='running',started_at=?1
             WHERE id=?2 AND state='admitted'",
            params![at, id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("subagent is not in admitted state"));
        }
        db(tx.commit())?;
        row(&c, id)
    }

    pub fn fail_admitted_subagent(
        &self,
        actor: &PrincipalId,
        id: &str,
        error: &str,
    ) -> Result<SubagentInfo> {
        let error = bounded_execution_error(Some(error))?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let info = row(&tx, id)?;
        if !principal_is_session_owner(&tx, &info.parent_session_id, actor)?
            || info.identity.actor != *actor
        {
            return Err(Error::Denied("subagent lifecycle belongs to another actor"));
        }
        let at = now();
        let changed = db(tx.execute(
            "UPDATE subagent_sessions
             SET state='failed',started_at=COALESCE(started_at,?1),settled_at=?1,error=?2
             WHERE id=?3 AND state='admitted'",
            params![at, error, id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("subagent is not in admitted state"));
        }
        if let (Some(root_id), Some(allocation_id)) = (
            info.delegation_root_id.as_deref(),
            info.delegation_allocation_id.as_deref(),
        ) {
            let allocation_changed = db(tx.execute(
                "UPDATE delegation_child_allocations
                 SET state='failed',settled_at=?1,error=?2
                 WHERE id=?3 AND root_id=?4 AND state='bound'",
                params![at, error, allocation_id, root_id],
            ))?;
            if allocation_changed != 1 {
                return Err(Error::Conflict("delegation allocation is not bound"));
            }
        }
        db(tx.commit())?;
        row(&c, id)
    }

    pub fn settle_subagent(
        &self,
        actor: &PrincipalId,
        id: &str,
        state: &str,
        error: Option<&str>,
    ) -> Result<SubagentInfo> {
        if !matches!(state, "completed" | "failed" | "cancelled" | "interrupted") {
            return Err(Error::Config("invalid terminal subagent state".into()));
        }
        if error.is_some_and(|value| value.len() > 4096 || value.chars().any(char::is_control)) {
            return Err(Error::Config("invalid bounded subagent error".into()));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let info = row(&tx, id)?;
        if !principal_is_session_owner(&tx, &info.parent_session_id, actor)?
            || info.identity.actor != *actor
        {
            return Err(Error::Denied("subagent lifecycle belongs to another actor"));
        }
        let at = now();
        let changed = db(tx.execute(
            "UPDATE subagent_sessions
             SET state=?1,settled_at=?2,error=?3
             WHERE id=?4 AND state='running'",
            params![state, at, error, id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("subagent is not running"));
        }
        if let (Some(root_id), Some(allocation_id)) = (
            info.delegation_root_id.as_deref(),
            info.delegation_allocation_id.as_deref(),
        ) {
            let allocation_changed = db(tx.execute(
                "UPDATE delegation_child_allocations
                 SET state=?1,settled_at=?2,error=?3
                 WHERE id=?4 AND root_id=?5 AND state='running'",
                params![state, at, error, allocation_id, root_id],
            ))?;
            if allocation_changed != 1 {
                return Err(Error::Conflict("delegation allocation is not running"));
            }
            let root_changed = db(tx.execute(
                "UPDATE delegation_roots
                 SET active_children=active_children-1,updated_at=?1
                 WHERE id=?2 AND active_children>0",
                params![at, root_id],
            ))?;
            if root_changed != 1 {
                return Err(Error::Conflict(
                    "delegation active-child counter is inconsistent",
                ));
            }
        }
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "subagent.settled",
            "subagent",
            id,
            Some(actor),
            serde_json::json!({"state":state}),
        )?;
        db(tx.commit())?;
        row(&c, id)
    }

    pub(crate) fn recover_subagent_execution_bindings_after_owner_restart(
        &self,
        c: &rusqlite::Connection,
    ) -> Result<()> {
        if self.trusted_local_direct() {
            return Ok(());
        }
        db(c.execute(
            "UPDATE subagent_execution_bindings
             SET state='unknown',error=CASE
                 WHEN state='planned' THEN 'owner restarted during managed worktree creation; inspect Git worktrees before any retry'
                 ELSE 'owner restarted during managed worktree cleanup; inspect Git worktrees before any retry'
             END
             WHERE state IN ('planned','cleanup_planned')",
            [],
        ))?;
        Ok(())
    }

    pub(crate) fn recover_subagents_after_owner_restart(
        &self,
        c: &rusqlite::Connection,
    ) -> Result<()> {
        if self.trusted_local_direct() {
            return Ok(());
        }
        let at = now();
        db(c.execute(
            "UPDATE delegation_roots
             SET active_children=MAX(
                    0,
                    active_children-(
                        SELECT count(*) FROM delegation_child_allocations a
                        WHERE a.root_id=delegation_roots.id AND a.state='running'
                    )
                 ),
                 updated_at=?1
             WHERE EXISTS(
                SELECT 1 FROM delegation_child_allocations a
                WHERE a.root_id=delegation_roots.id AND a.state='running'
             )",
            [at],
        ))?;
        db(c.execute(
            "UPDATE delegation_child_allocations
             SET state='failed',settled_at=?1,
                 error='owner restarted before child Session admission; allocation remains consumed'
             WHERE state='reserved'",
            [at],
        ))?;
        db(c.execute(
            "UPDATE delegation_child_allocations
             SET state='interrupted',settled_at=?1,
                 error='owner restarted before child completion; allocation remains consumed'
             WHERE state IN ('bound','running')",
            [at],
        ))?;
        db(c.execute(
            "UPDATE delegation_model_attempts
             SET state='unknown',finished_at=?1,
                 outcome='owner_restart_unknown'
             WHERE state='started'",
            [at],
        ))?;
        db(c.execute(
            "UPDATE delegation_routing_attempts
             SET state='unknown',finished_at=?1,
                 outcome='owner_restart_unknown'
             WHERE state='started'",
            [at],
        ))?;
        db(c.execute(
            "UPDATE subagent_sessions
             SET state='interrupted',started_at=COALESCE(started_at,?1),settled_at=?1,
                 error='owner restarted before child completion; inspect child Session before retry'
             WHERE state IN ('admitted','running')",
            [at],
        ))?;
        Ok(())
    }
}
