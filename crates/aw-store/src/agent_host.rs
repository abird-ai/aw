//! Thin DL-200 host Agent state shell.
//!
//! This module owns no Agent/Session/Project/Sandbox state. It composes the
//! authoritative Agent/history/resource owners and keeps trusted host selection
//! separate from model-visible constructor arguments.

use crate::{
    actor_queue, agent_owner, audit, db, history_owner, principal_groups, resources,
    stable_user_principal, user_for_principal, workspace_writer, AgentAccessSnapshot,
    AgentHistoryForkAccess, AgentPermission, AgentPermissions, Error, PrincipalId, Result,
    SessionBinding, SessionStore,
};
use aw_tools::execution_ref::{AgentRef, ExecutionRef, ExecutionSegment};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;

const MAX_AGENT_LIST: usize = 256;
const MAX_INITIAL_ACCESS: usize = 512;
use crate::construction_receipt::{
    self, ConstructionCandidate, ConstructionOperation,
    ConstructionPreparation as AgentCreatePreparation, InitialTask,
    PreparationState as AgentCreatePreparationState,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLifecycle {
    Open,
    Closing,
    Closed,
}

impl AgentLifecycle {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "open" => Ok(Self::Open),
            "closing" => Ok(Self::Closing),
            "closed" => Ok(Self::Closed),
            _ => Err(Error::Storage("invalid authoritative Agent lifecycle")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAdmission {
    Open,
    Held,
}

impl AgentAdmission {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "open" => Ok(Self::Open),
            "held" => Ok(Self::Held),
            _ => Err(Error::Storage(
                "invalid authoritative Agent admission state",
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Held => "held",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSnapshot {
    pub canonical_ref: String,
    pub project_ref: String,
    pub owner_ref: String,
    pub title: String,
    pub rev: u64,
    pub caller_permissions: AgentPermissions,
    pub access_generation: [u8; 32],
    pub lifecycle: AgentLifecycle,
    pub admission: AgentAdmission,
    pub active_turn: Option<u64>,
    pub parent_agent_ref: Option<String>,
    pub sandbox_ref: Option<String>,
    pub history_head: String,
    pub history_content_digest: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentCreateAccessGrant {
    pub principal: String,
    pub permissions: AgentPermissions,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode", content = "grants")]
pub enum AgentCreateAccess {
    #[default]
    Private,
    Inherit,
    Custom(Vec<AgentCreateAccessGrant>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode", content = "reference")]
pub enum AgentSandboxSelection {
    #[default]
    Auto,
    Existing(String),
}

/// Caller-visible agent.create arguments. Trusted current Project / execution
/// binding live in AgentCreateContext, never in this model-visible shape.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentCreateRequest {
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub access: AgentCreateAccess,
    #[serde(default)]
    pub sandbox: AgentSandboxSelection,
}

/// Trusted host-only construction context. binding is the already-resolved
/// sandbox=auto execution binding in this Phase-1 cut; it is not derived from
/// history ancestry and is never accepted from model arguments.
#[derive(Clone, Debug)]
pub struct AgentCreateContext {
    pub current_project_ref: String,
    pub current_agent_ref: Option<String>,
    pub binding: SessionBinding,
    /// Trusted current-host Sandbox policy for sandbox=auto. This is an
    /// immutable current Agent binding, never history-derived or model-chosen.
    pub auto_sandbox_ref: Option<String>,
    /// Stable authenticated host/RPC request identity. Required only for
    /// task-bearing construction because it owns prepared external custody.
    pub request_id: Option<String>,
    /// Trusted logical execution site selected by the host. Never accepted
    /// from model-visible Agent constructor arguments.
    pub execution_site: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectRef {
    pub(crate) domain: String,
    pub(crate) identity: String,
    pub(crate) project: String,
}

impl ProjectRef {
    pub(crate) fn canonical(&self) -> String {
        format!("/d:{}/i:{}/p:{}", self.domain, self.identity, self.project)
    }
}

pub(crate) fn parse_project_ref(value: &str) -> Result<ProjectRef> {
    let Some(body) = value.strip_prefix('/') else {
        return Err(Error::Denied("Project ref must be canonical"));
    };
    let mut parts = body.split('/');
    let domain = parts
        .next()
        .and_then(|value| value.strip_prefix("d:"))
        .ok_or(Error::Denied("invalid canonical Project ref"))?;
    let identity = parts
        .next()
        .and_then(|value| value.strip_prefix("i:"))
        .ok_or(Error::Denied("invalid canonical Project ref"))?;
    let project = parts
        .next()
        .and_then(|value| value.strip_prefix("p:"))
        .ok_or(Error::Denied("invalid canonical Project ref"))?;
    if parts.next().is_some() {
        return Err(Error::Denied("invalid canonical Project ref"));
    }
    for value in [domain, identity, project] {
        crate::validate_agent_handle_component(value)?;
    }
    Ok(ProjectRef {
        domain: domain.to_owned(),
        identity: identity.to_owned(),
        project: project.to_owned(),
    })
}

fn canonical_agent_ref(domain: &str, identity: &str, project: &str, agent: &str) -> String {
    format!("/d:{domain}/i:{identity}/p:{project}/a:{agent}")
}

fn key_for_ref_in(connection: &Connection, canonical_ref: &str) -> Result<(String, String)> {
    let row = db(connection
        .query_row(
            "SELECT session_id,branch_id
             FROM agent_state_current
             WHERE '/d:' || domain_handle || '/i:' || identity_handle ||
                   '/p:' || project_handle || '/a:' || agent_handle = ?1",
            [canonical_ref],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional())?;
    row.ok_or(Error::Denied("Agent ref is unavailable or not authorized"))
}

fn canonical_current_agent(context: &AgentCreateContext) -> Result<Option<AgentRef>> {
    let Some(reference) = context.current_agent_ref.as_deref() else {
        return Ok(None);
    };
    let parsed = ExecutionRef::parse(reference)
        .map_err(|_| Error::Denied("trusted current Agent ref is invalid"))?;
    if !parsed.is_canonical() {
        return Err(Error::Denied("trusted current Agent ref must be canonical"));
    }
    let resolved = parsed
        .resolve(None, None)
        .map_err(|_| Error::Denied("trusted current Agent ref is invalid"))?;
    if !resolved.suffix().is_empty() {
        return Err(Error::Denied(
            "trusted current Agent ref must be an Agent root",
        ));
    }
    Ok(Some(resolved.agent_ref().clone()))
}

fn canonical_history_source(
    context: &AgentCreateContext,
    reference: &str,
) -> Result<(String, bool, ProjectRef)> {
    let parsed =
        ExecutionRef::parse(reference).map_err(|_| Error::Denied("invalid Agent history ref"))?;
    let current = canonical_current_agent(context)?;
    let resolved = if parsed.is_canonical() {
        parsed.resolve(None, None)
    } else {
        parsed.resolve(current.as_ref(), None)
    }
    .map_err(|_| Error::Denied("Agent history ref cannot be resolved in this host context"))?;
    let is_agent = match resolved.suffix() {
        [] => true,
        [ExecutionSegment::Turn(_)] => false,
        _ => {
            return Err(Error::Denied(
                "Agent history source must be an Agent or settled Turn ref",
            ))
        }
    };
    let agent = resolved.agent_ref();
    Ok((
        resolved.canonical(),
        is_agent,
        ProjectRef {
            domain: agent.domain.as_str().to_owned(),
            identity: agent.identity.as_str().to_owned(),
            project: agent.project.as_str().to_owned(),
        },
    ))
}

fn validate_host_request_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-:".contains(&byte))
    {
        return Err(Error::Denied("invalid host Agent create request identity"));
    }
    Ok(())
}

pub(crate) fn validate_execution_site(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value.is_ascii()
        || value.chars().any(char::is_control)
    {
        return Err(Error::Denied("invalid trusted Agent execution site"));
    }
    Ok(())
}

fn agent_create_fingerprint(
    context: &AgentCreateContext,
    request: &AgentCreateRequest,
    canonical_source: Option<&str>,
    project: &ProjectRef,
    title: &str,
) -> Result<String> {
    let task_sha256 = request
        .task
        .as_deref()
        .map(|task| format!("{:x}", Sha256::digest(task.as_bytes())));
    let semantic = serde_json::json!({
        "version":1,
        "current_agent":context.current_agent_ref,
        "from":canonical_source,
        "project":project.canonical(),
        "title":title,
        "task_sha256":task_sha256,
        "access":request.access,
        "sandbox":request.sandbox,
        "auto_sandbox":context.auto_sandbox_ref,
        "binding":context.binding,
        "execution_site":context.execution_site,
    });
    let encoded = serde_json::to_vec(&semantic)
        .map_err(|_| Error::Storage("cannot encode Agent create fingerprint"))?;
    let mut hash = Sha256::new();
    hash.update(b"aw-host-agent-create-v1\0");
    hash.update(encoded);
    Ok(format!("{:x}", hash.finalize()))
}

fn agent_create_preparation_in(
    connection: &Connection,
    parent_session: &str,
    parent_branch: &str,
    request_id: &str,
) -> Result<Option<AgentCreatePreparation>> {
    let operation = ConstructionOperation::AgentCreate;
    let scope = operation.scope(
        "",
        agent_owner::AgentKey::new(parent_session, parent_branch),
    );
    construction_receipt::lookup_in(connection, operation, &scope, request_id)
}

fn transition_agent_create_preparation(
    tx: &rusqlite::Transaction<'_>,
    actor: &PrincipalId,
    parent_session: &str,
    parent_branch: &str,
    expected: &AgentCreatePreparation,
    state: AgentCreatePreparationState,
    result_ref: Option<String>,
    note: Option<String>,
) -> Result<AgentCreatePreparation> {
    if expected.parent_session_id != parent_session || expected.parent_branch_id != parent_branch {
        return Err(Error::Conflict("Agent create receipt origin changed"));
    }
    construction_receipt::transition_in(tx, actor, expected, state, result_ref, note)
}

fn persist_agent_create_preparation_transition(
    store: &SessionStore,
    actor: &PrincipalId,
    parent_session: &str,
    parent_branch: &str,
    expected: &AgentCreatePreparation,
    state: AgentCreatePreparationState,
    note: Option<String>,
) -> Result<AgentCreatePreparation> {
    let mut connection = store.connection()?;
    let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let next = transition_agent_create_preparation(
        &tx,
        actor,
        parent_session,
        parent_branch,
        expected,
        state,
        None,
        note,
    )?;
    db(tx.commit())?;
    Ok(next)
}

fn reject_prepared_agent_create(
    store: &SessionStore,
    actor: &PrincipalId,
    parent_session: &str,
    parent_branch: &str,
    preparation: &AgentCreatePreparation,
    custody: &actor_queue::PreparedQueueCustody,
    error: Error,
) -> Error {
    let note = error.to_string().chars().take(2048).collect::<String>();
    if store.discard_prepared_queue_custody(custody).is_ok()
        && persist_agent_create_preparation_transition(
            store,
            actor,
            parent_session,
            parent_branch,
            preparation,
            AgentCreatePreparationState::Rejected,
            Some(note.clone()),
        )
        .is_ok()
    {
        return error;
    }
    let _ = persist_agent_create_preparation_transition(
        store,
        actor,
        parent_session,
        parent_branch,
        preparation,
        AgentCreatePreparationState::Unknown,
        Some(note),
    );
    Error::Conflict("Agent create preparation outcome requires inspection")
}

fn mark_agent_create_preparation_unknown(
    store: &SessionStore,
    actor: &PrincipalId,
    parent_session: &str,
    parent_branch: &str,
    preparation: &AgentCreatePreparation,
    error: &Error,
) -> Error {
    let note = error.to_string().chars().take(2048).collect::<String>();
    let _ = persist_agent_create_preparation_transition(
        store,
        actor,
        parent_session,
        parent_branch,
        preparation,
        AgentCreatePreparationState::Unknown,
        Some(note),
    );
    Error::Conflict("Agent create publication outcome requires inspection")
}

fn validate_project_exists(connection: &Connection, project: &ProjectRef) -> Result<()> {
    let actual_domain: Option<String> = db(connection
        .query_row(
            "SELECT domain_id FROM workspaces
             WHERE workspace_id=?1 AND archived_at IS NULL",
            [&project.project],
            |row| row.get(0),
        )
        .optional())?;
    if actual_domain.as_deref() != Some(project.domain.as_str()) {
        return Err(Error::Denied("Project is unavailable"));
    }
    let principal_exists: bool = db(connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM domain_principals
             WHERE domain_id=?1 AND kind='user' AND principal_handle=?2
               AND user_id=?2 AND status='active'
         )",
        params![project.domain, project.identity],
        |row| row.get(0),
    ))?;
    if !principal_exists {
        return Err(Error::Denied(
            "Phase-1 Agent creation requires an active user-owned Project principal",
        ));
    }
    Ok(())
}

pub(crate) fn authorize_project_create(
    connection: &Connection,
    actor: &PrincipalId,
    project: &ProjectRef,
) -> Result<()> {
    validate_project_exists(connection, project)?;
    let actor_user = crate::user_for_principal(connection, actor)?.ok_or(Error::Denied(
        "Agent destination Project authority unavailable",
    ))?;
    if actor_user != project.identity {
        return Err(Error::Denied(
            "Agent destination Project principal must be the authenticated stable user",
        ));
    }
    let member: bool = db(connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM domain_memberships
             WHERE domain_id=?1 AND user_id=?2 AND status='active'
         )",
        params![project.domain, actor_user],
        |row| row.get(0),
    ))?;
    if !member {
        return Err(Error::Denied(
            "Agent destination Project requires active Domain membership",
        ));
    }
    workspace_writer::require_workspace_writer(connection, &project.project)?;
    Ok(())
}

fn parent_if_visible(
    connection: &Connection,
    actor: &PrincipalId,
    parent: Option<String>,
) -> Result<Option<String>> {
    let Some(parent) = parent else {
        return Ok(None);
    };
    let (session, branch) = key_for_ref_in(connection, &parent)?;
    match agent_owner::authorize_permission(
        connection,
        actor,
        agent_owner::AgentKey::new(&session, &branch),
        AgentPermission::View,
    ) {
        Ok(_) => Ok(Some(parent)),
        Err(Error::Denied(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn snapshot_in(
    connection: &Connection,
    actor: &PrincipalId,
    canonical_ref: &str,
) -> Result<AgentSnapshot> {
    let (session, branch) = key_for_ref_in(connection, canonical_ref)?;
    let agent = agent_owner::AgentKey::new(&session, &branch);
    let access =
        agent_owner::authorize_permission(connection, actor, agent, AgentPermission::View)?;
    let row: (
        String,
        String,
        String,
        String,
        String,
        String,
        u64,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        i64,
        i64,
        Option<u64>,
    ) = db(connection.query_row(
        "SELECT a.domain_handle,a.identity_handle,a.project_handle,
                a.owner_kind,a.owner_handle,a.title,a.rev,a.lifecycle,a.admission,
                a.parent_agent_ref,a.sandbox_id,b.head,h.history_content_digest,
                a.created_at,a.updated_at,r.turn_id
         FROM agent_state_current a
         JOIN branches b ON b.session_id=a.session_id AND b.id=a.branch_id
         LEFT JOIN history_bases h ON h.base_id=a.history_base_id
         LEFT JOIN runs r ON r.id=b.active_run AND r.session_id=a.session_id
                          AND r.branch_id=a.branch_id AND r.status='running'
         WHERE a.session_id=?1 AND a.branch_id=?2",
        params![session, branch],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
                row.get(12)?,
                row.get(13)?,
                row.get(14)?,
                row.get(15)?,
            ))
        },
    ))?;
    let owner_ref = format!("/d:{}/i:{}", row.0, row.4);
    let project_ref = format!("/d:{}/i:{}/p:{}", row.0, row.1, row.2);
    let parent = parent_if_visible(connection, actor, row.9)?;
    let sandbox_ref = match row.10 {
        Some(reference) if reference.starts_with("/d:") => Some(reference),
        Some(_) => {
            return Err(Error::Storage(
                "Agent Sandbox binding is not a canonical Sandbox ref",
            ))
        }
        None => None,
    };
    Ok(AgentSnapshot {
        canonical_ref: canonical_ref.to_owned(),
        project_ref,
        owner_ref,
        title: row.5,
        rev: row.6,
        caller_permissions: access.effective_permissions,
        access_generation: access.access_generation,
        lifecycle: AgentLifecycle::parse(&row.7)?,
        admission: AgentAdmission::parse(&row.8)?,
        active_turn: row.15,
        parent_agent_ref: parent,
        sandbox_ref,
        history_head: row.11,
        history_content_digest: row.12,
        created_at: row.13,
        updated_at: row.14,
    })
}

pub(crate) fn validate_initial_custom_access(grants: &[AgentCreateAccessGrant]) -> Result<()> {
    if grants.len() > MAX_INITIAL_ACCESS {
        return Err(Error::Limit("initial Agent access list exceeds bound"));
    }
    let mut principals = BTreeSet::new();
    for grant in grants {
        if grant.permissions.validate()?.is_empty() {
            return Err(Error::Denied(
                "initial Agent access grants cannot be empty; omit the principal instead",
            ));
        }
        if !principals.insert(grant.principal.as_str()) {
            return Err(Error::Denied("duplicate principal in initial Agent access"));
        }
    }
    Ok(())
}

pub(crate) type InitialAcl = Vec<(String, String, AgentPermissions)>;

/// Resolve the exact initial ACL against the destination Domain. Both
/// ordinary construction and history construction use this owner.
pub(crate) fn resolve_initial_custom_access(
    connection: &Connection,
    domain: &str,
    owner_kind: &str,
    owner_handle: &str,
    grants: &[AgentCreateAccessGrant],
) -> Result<InitialAcl> {
    validate_initial_custom_access(grants)?;
    let mut seen = BTreeSet::new();
    let mut resolved = Vec::with_capacity(grants.len());
    for grant in grants {
        let (kind, handle) =
            principal_groups::resolve_principal_ref(connection, domain, &grant.principal)?;
        if kind == owner_kind && handle == owner_handle {
            return Err(Error::Denied("Agent owner access is implicit"));
        }
        if !seen.insert((kind.clone(), handle.clone())) {
            return Err(Error::Denied("duplicate principal in initial Agent access"));
        }
        resolved.push((kind, handle, grant.permissions));
    }
    Ok(resolved)
}

fn install_initial_custom_access(
    connection: &Connection,
    actor: &PrincipalId,
    session: &str,
    branch: &str,
    grants: &[AgentCreateAccessGrant],
) -> Result<()> {
    let agent = agent_owner::AgentKey::new(session, branch);
    agent_owner::authorize_permission(connection, actor, agent, AgentPermission::Manage)?;
    let (domain, owner_kind, owner_handle): (String, String, String) = db(connection.query_row(
        "SELECT domain_handle,owner_kind,owner_handle
         FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
        params![session, branch],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ))?;
    let manager = user_for_principal(connection, actor)?
        .ok_or(Error::Denied("Agent manager identity is inactive"))?;
    for (kind, handle, permissions) in
        resolve_initial_custom_access(connection, &domain, &owner_kind, &owner_handle, grants)?
    {
        if permissions.view {
            history_owner::prepare_disclosure_expansion(connection, actor, agent, &kind, &handle)?;
        }
        agent_owner::initialize_acl(connection, agent, &kind, &handle, permissions, &manager)?;
    }
    Ok(())
}

impl SessionStore {
    /// Pure authoritative Agent read by canonical ref.
    pub fn agent_get(&self, actor: &PrincipalId, canonical_ref: &str) -> Result<AgentSnapshot> {
        let connection = self.connection()?;
        snapshot_in(&connection, actor, canonical_ref)
    }

    /// Exact structural binding for trusted host composition, not an
    /// execution grant. Resolve through the original Agent owner.
    pub fn agent_execution_binding(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
    ) -> Result<aw_agent_core::AgentExecutionBinding> {
        let connection = self.connection()?;
        let (session, branch) = agent_owner::key_for_ref(&connection, canonical_ref)?;
        let key = agent_owner::AgentKey::new(&session, &branch);
        agent_owner::authorize_permission(&connection, actor, key, AgentPermission::View)?;
        let reference = agent_owner::canonical_ref(&connection, key)?;
        if reference != canonical_ref {
            return Err(Error::Denied(
                "host binding requires an exact canonical Agent",
            ));
        }
        aw_agent_core::AgentExecutionBinding::from_trusted_host(reference, session, branch)
    }

    /// Bounded project-scoped Agent enumeration. after is the canonical ref
    /// returned by the previous page; rows are ordered by immutable creation key.
    pub fn agent_list(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<AgentSnapshot>> {
        if limit == 0 || limit > MAX_AGENT_LIST {
            return Err(Error::Limit("invalid Agent list limit"));
        }
        let project = parse_project_ref(project_ref)?;
        let connection = self.connection()?;
        let mut cursor = if let Some(reference) = after {
            let row: Option<(i64, String, String, String, String)> = db(connection
                .query_row(
                    "SELECT created_at,agent_handle,domain_handle,identity_handle,project_handle
                     FROM agent_state_current
                     WHERE '/d:' || domain_handle || '/i:' || identity_handle ||
                           '/p:' || project_handle || '/a:' || agent_handle = ?1",
                    [reference],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    },
                )
                .optional())?;
            let Some((created, agent, domain, identity, project_handle)) = row else {
                return Err(Error::Denied("Agent list cursor is unavailable"));
            };
            if domain != project.domain
                || identity != project.identity
                || project_handle != project.project
            {
                return Err(Error::Denied(
                    "Agent list cursor belongs to another Project",
                ));
            }
            Some((created, agent))
        } else {
            None
        };

        let mut out = Vec::with_capacity(limit);
        while out.len() < limit {
            let batch = (limit.saturating_sub(out.len()).max(16) * 2).min(512);
            let (created, handle) = cursor.clone().unwrap_or((i64::MIN, String::new()));
            let mut query = db(connection.prepare(
                "SELECT created_at,agent_handle
                 FROM agent_state_current
                 WHERE domain_handle=?1 AND identity_handle=?2 AND project_handle=?3
                   AND (created_at>?4 OR (created_at=?4 AND agent_handle>?5))
                 ORDER BY created_at,agent_handle
                 LIMIT ?6",
            ))?;
            let rows = db(query.query_map(
                params![
                    project.domain,
                    project.identity,
                    project.project,
                    created,
                    handle,
                    batch as u64
                ],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            ))?;
            let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
            if rows.is_empty() {
                break;
            }
            let fetched = rows.len();
            for (created, handle) in rows {
                cursor = Some((created, handle.clone()));
                let canonical = canonical_agent_ref(
                    &project.domain,
                    &project.identity,
                    &project.project,
                    &handle,
                );
                match snapshot_in(&connection, actor, &canonical) {
                    Ok(snapshot) => out.push(snapshot),
                    Err(Error::Denied(_)) => {}
                    Err(error) => return Err(error),
                }
                if out.len() == limit {
                    break;
                }
            }
            if fetched < batch {
                break;
            }
        }
        Ok(out)
    }

    /// Phase-1 host constructor cut. It supports fresh or exact-history Agent
    /// creation with source/current Project defaulting, reviewed same-user
    /// cross-Project redirect, private/inherited/custom access, and immutable
    /// Sandbox selection. Optional initial task custody is prepared before
    /// publication and the ordinary Pending queue fact commits atomically with
    /// the Agent. Cross-Project redirect requires an explicit destination
    /// Sandbox until Phase-3 materialization can implement truthful auto.
    pub fn agent_create(
        &self,
        actor: &PrincipalId,
        context: &AgentCreateContext,
        request: AgentCreateRequest,
    ) -> Result<AgentSnapshot> {
        crate::validate_binding(&context.binding)?;
        validate_execution_site(&context.execution_site)?;
        let current_project = parse_project_ref(&context.current_project_ref)?;
        if request.task.as_deref().is_some_and(|task| {
            task.trim().is_empty() || task.len() > 64 * 1024 || task.contains('\0')
        }) {
            return Err(Error::Denied("invalid initial Agent task"));
        }
        let title = request.title.as_deref().unwrap_or("Agent");
        crate::label(title, 160)?;

        let source = request
            .from
            .as_deref()
            .map(|reference| canonical_history_source(context, reference))
            .transpose()?;
        let source_project = source.as_ref().map(|(_, _, project)| project.clone());
        let selected_project = if let Some(requested) = request.project.as_deref() {
            parse_project_ref(requested)?
        } else {
            source_project
                .clone()
                .unwrap_or_else(|| current_project.clone())
        };

        let redirected_from_source = source_project
            .as_ref()
            .is_some_and(|source_project| source_project != &selected_project);
        let redirected_from_current = selected_project != current_project;
        if redirected_from_current && matches!(request.sandbox, AgentSandboxSelection::Auto) {
            return Err(Error::Conflict(
                "cross-Project Agent creation requires an explicit destination Sandbox until Sandbox materialization is available",
            ));
        }
        let requested_sandbox = match &request.sandbox {
            AgentSandboxSelection::Auto => context.auto_sandbox_ref.as_deref(),
            AgentSandboxSelection::Existing(reference) => Some(reference.as_str()),
        };
        if matches!(request.access, AgentCreateAccess::Inherit)
            && !source
                .as_ref()
                .is_some_and(|(_, source_is_agent, _)| *source_is_agent)
        {
            return Err(Error::Denied(
                "access=inherit requires an Agent history source",
            ));
        }
        if let AgentCreateAccess::Custom(grants) = &request.access {
            validate_initial_custom_access(grants)?;
        }

        let prepared_task = if let Some(task) = request.task.as_deref() {
            if self.directory().is_none() {
                return Err(Error::Denied(
                    "task-bearing Agent creation requires persistent SecretStore custody",
                ));
            }
            let request_id = context.request_id.as_deref().ok_or(Error::Denied(
                "task-bearing Agent creation requires a stable host request identity",
            ))?;
            validate_host_request_id(request_id)?;
            let current_agent = canonical_current_agent(context)?.ok_or(Error::Denied(
                "task-bearing Agent creation requires a bound current Agent",
            ))?;
            let parent_ref = current_agent.canonical();
            let fingerprint = agent_create_fingerprint(
                context,
                &request,
                source.as_ref().map(|(canonical, _, _)| canonical.as_str()),
                &selected_project,
                title,
            )?;

            let (parent_session, parent_branch, creator) = {
                let connection = self.connection()?;
                authorize_project_create(&connection, actor, &selected_project)?;
                if let Some((canonical_source, _, _)) = source.as_ref() {
                    history_owner::preflight_canonical_history_source_in(
                        &connection,
                        actor,
                        canonical_source,
                    )?;
                    if redirected_from_source {
                        history_owner::authorize_canonical_history_share_in(
                            &connection,
                            actor,
                            canonical_source,
                        )?;
                    }
                }
                crate::sandbox_owner::preflight_initial_selection(
                    &connection,
                    actor,
                    &selected_project.domain,
                    &selected_project.identity,
                    &selected_project.project,
                    &context.binding,
                    requested_sandbox,
                )?;
                let (session, branch) = key_for_ref_in(&connection, &parent_ref)?;
                agent_owner::authorize_permission(
                    &connection,
                    actor,
                    agent_owner::AgentKey::new(&session, &branch),
                    AgentPermission::View,
                )?;
                if let Some(existing) =
                    agent_create_preparation_in(&connection, &session, &branch, request_id)?
                {
                    existing.require_actor(&connection, actor)?;
                    if existing.fingerprint != fingerprint {
                        return Err(Error::Denied(
                            "Agent create request identity was reused with changed semantics",
                        ));
                    }
                    return match existing.state {
                        AgentCreatePreparationState::Applied => {
                            let canonical = existing.result_ref.ok_or(Error::Storage(
                                "applied Agent create receipt lost its result ref",
                            ))?;
                            snapshot_in(&connection, actor, &canonical)
                        }
                        AgentCreatePreparationState::Prepared
                        | AgentCreatePreparationState::Unknown
                        | AgentCreatePreparationState::Rejected => Err(Error::Conflict(
                            "Agent create request is non-retryable; inspect its prepared receipt",
                        )),
                    };
                }
                let creator = user_for_principal(&connection, actor)?
                    .ok_or(Error::Denied("Agent create caller unavailable"))?;
                (session, branch, creator)
            };

            let ids = resources::mint_session_candidate_ids()?;
            let initial_task =
                InitialTask::mint(request_id, &ids.session, task, &context.execution_site)?;
            let custody = initial_task.custody();
            let operation = ConstructionOperation::AgentCreate;
            let preparation = AgentCreatePreparation {
                operation,
                scope: operation.scope(
                    &creator,
                    agent_owner::AgentKey::new(&parent_session, &parent_branch),
                ),
                creator,
                parent_session_id: parent_session.clone(),
                parent_branch_id: parent_branch.clone(),
                request_id: request_id.to_owned(),
                fingerprint,
                state: AgentCreatePreparationState::Prepared,
                candidate: ConstructionCandidate::Agent {
                    session: ids.session,
                    root: ids.root,
                    branch: ids.branch,
                    sandbox: None,
                },
                task: Some(initial_task),
                sources: None,
                capture_boundary: None,
                restore_boundary: None,
                sandbox_boundary: None,
                result_ref: None,
                note: None,
            };

            {
                let mut connection = self.connection()?;
                let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
                authorize_project_create(&tx, actor, &selected_project)?;
                if let Some((canonical_source, _, _)) = source.as_ref() {
                    history_owner::preflight_canonical_history_source_in(
                        &tx,
                        actor,
                        canonical_source,
                    )?;
                    if redirected_from_source {
                        history_owner::authorize_canonical_history_share_in(
                            &tx,
                            actor,
                            canonical_source,
                        )?;
                    }
                }
                crate::sandbox_owner::preflight_initial_selection(
                    &tx,
                    actor,
                    &selected_project.domain,
                    &selected_project.identity,
                    &selected_project.project,
                    &context.binding,
                    requested_sandbox,
                )?;
                let (session, branch) = key_for_ref_in(&tx, &parent_ref)?;
                if session != parent_session || branch != parent_branch {
                    return Err(Error::Conflict(
                        "current Agent identity changed before create preparation",
                    ));
                }
                agent_owner::authorize_permission(
                    &tx,
                    actor,
                    agent_owner::AgentKey::new(&session, &branch),
                    AgentPermission::View,
                )?;
                construction_receipt::claim_in(
                    &tx,
                    actor,
                    &self.inner.source_drivers,
                    self.inner.source_limit,
                    &preparation,
                )?;
                db(tx.commit())?;
            }

            if let Err(error) = self.write_prepared_queue_custody(&custody, task) {
                let note = error.to_string().chars().take(2048).collect::<String>();
                if self.discard_prepared_queue_custody(&custody).is_ok() {
                    let _ = persist_agent_create_preparation_transition(
                        self,
                        actor,
                        &parent_session,
                        &parent_branch,
                        &preparation,
                        AgentCreatePreparationState::Rejected,
                        Some(note),
                    );
                    return Err(error);
                }
                let _ = persist_agent_create_preparation_transition(
                    self,
                    actor,
                    &parent_session,
                    &parent_branch,
                    &preparation,
                    AgentCreatePreparationState::Unknown,
                    Some(note),
                );
                return Err(Error::Conflict(
                    "Agent create task custody outcome requires inspection",
                ));
            }
            if let Err(error) = self.verify_prepared_queue_custody(&custody) {
                let note = error.to_string().chars().take(2048).collect::<String>();
                let state = if self.discard_prepared_queue_custody(&custody).is_ok() {
                    AgentCreatePreparationState::Rejected
                } else {
                    AgentCreatePreparationState::Unknown
                };
                let _ = persist_agent_create_preparation_transition(
                    self,
                    actor,
                    &parent_session,
                    &parent_branch,
                    &preparation,
                    state,
                    Some(note),
                );
                return Err(error);
            }
            Some((parent_session, parent_branch, preparation, custody))
        } else {
            None
        };

        let reject_precommit = |error: Error| -> Error {
            if let Some((parent_session, parent_branch, preparation, custody)) = &prepared_task {
                reject_prepared_agent_create(
                    self,
                    actor,
                    parent_session,
                    parent_branch,
                    preparation,
                    custody,
                    error,
                )
            } else {
                error
            }
        };

        let mut connection = self.connection().map_err(&reject_precommit)?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))
            .map_err(&reject_precommit)?;
        let publication = (|| -> Result<String> {
            // Do not turn a consumed effectful request into a store-only
            // constructor by removing its task on retry.
            if request.task.is_none() {
                if let (Some(request_id), Some(origin)) = (
                    context.request_id.as_deref(),
                    context.current_agent_ref.as_deref(),
                ) {
                    validate_host_request_id(request_id)?;
                    let (session, branch) = key_for_ref_in(&tx, origin)?;
                    if agent_create_preparation_in(&tx, &session, &branch, request_id)?.is_some() {
                        return Err(Error::Conflict(
                            "Agent create request is non-retryable; inspect its prepared receipt",
                        ));
                    }
                }
            }

            authorize_project_create(&tx, actor, &selected_project)?;
            let candidate_ids = prepared_task
                .as_ref()
                .map(|(_, _, preparation, _)| preparation.candidate.agent_ids())
                .transpose()?;
            let (session, branch) =
                if let Some((canonical_source, source_is_agent, _)) = source.as_ref() {
                    let fork_access = match &request.access {
                        AgentCreateAccess::Private => AgentHistoryForkAccess::Private,
                        AgentCreateAccess::Custom(grants) => {
                            AgentHistoryForkAccess::Custom(grants.clone())
                        }
                        AgentCreateAccess::Inherit if *source_is_agent => {
                            AgentHistoryForkAccess::Inherit
                        }
                        AgentCreateAccess::Inherit => {
                            return Err(Error::Denied(
                                "access=inherit requires an Agent history source, not a Turn",
                            ))
                        }
                    };
                    let destination_owner = stable_user_principal(&selected_project.identity)?;
                    if let Some(ids) = candidate_ids {
                        history_owner::create_agent_from_history_ref_with_ids_in_project(
                            &tx,
                            actor,
                            canonical_source,
                            title,
                            &context.binding,
                            fork_access,
                            &destination_owner,
                            &selected_project.project,
                            ids,
                        )?
                    } else {
                        history_owner::create_agent_from_history_ref_in_project(
                            &tx,
                            actor,
                            canonical_source,
                            title,
                            &context.binding,
                            fork_access,
                            &destination_owner,
                            &selected_project.project,
                        )?
                    }
                } else {
                    let owner = stable_user_principal(&selected_project.identity)?;
                    if let Some(ids) = candidate_ids {
                        resources::create_in_workspace_with_ids_tx(
                            &tx,
                            &owner,
                            title,
                            &context.binding,
                            &selected_project.project,
                            None,
                            ids,
                        )?
                    } else {
                        resources::create_in_workspace_tx(
                            &tx,
                            &owner,
                            title,
                            &context.binding,
                            &selected_project.project,
                            None,
                        )?
                    }
                };

            let selected_sandbox = crate::sandbox_owner::select_initial_agent_sandbox(
                &tx,
                actor,
                &session.id,
                &branch.id,
                requested_sandbox,
            )?;

            // Exact-history construction already installed the complete
            // custom ACL and its initial HistoryGrant together.
            if source.is_none() {
                if let AgentCreateAccess::Custom(grants) = &request.access {
                    install_initial_custom_access(&tx, actor, &session.id, &branch.id, grants)?;
                }
            }

            if let Some((_, _, preparation, _)) = &prepared_task {
                let ids = preparation.candidate.agent_ids()?;
                if session.id != ids.session || branch.id != ids.branch {
                    return Err(Error::Conflict(
                        "published Agent identity differs from prepared candidate",
                    ));
                }
                let owner = stable_user_principal(&selected_project.identity)?;
                preparation
                    .initial_task()?
                    .publish_in(&tx, &owner, &session.id, &branch.id)?;
            }

            let canonical = db(tx.query_row(
                "SELECT '/d:' || domain_handle || '/i:' || identity_handle ||
                        '/p:' || project_handle || '/a:' || agent_handle
                 FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
                params![session.id, branch.id],
                |row| row.get::<_, String>(0),
            ))?;
            audit(
                &tx,
                &session.id,
                actor,
                "agent.created",
                serde_json::json!({
                    "branch_id":branch.id,
                    "ref":canonical,
                    "from":request.from,
                    "project":selected_project.canonical(),
                    "access":request.access,
                    "sandbox":selected_sandbox,
                    "initial_task":prepared_task.as_ref().and_then(|(_,_,preparation,_)| preparation.task.as_ref().map(|task| task.queue_request_id.as_str())),
                }),
            )?;
            if let Some((parent_session, parent_branch, preparation, _)) = &prepared_task {
                transition_agent_create_preparation(
                    &tx,
                    actor,
                    parent_session,
                    parent_branch,
                    preparation,
                    AgentCreatePreparationState::Applied,
                    Some(canonical.clone()),
                    None,
                )?;
            }
            Ok(canonical)
        })();

        let canonical = match publication {
            Ok(canonical) => canonical,
            Err(error) => {
                drop(tx);
                return Err(reject_precommit(error));
            }
        };
        if let Err(error) = db(tx.commit()) {
            if let Some((parent_session, parent_branch, preparation, _)) = &prepared_task {
                return Err(mark_agent_create_preparation_unknown(
                    self,
                    actor,
                    parent_session,
                    parent_branch,
                    preparation,
                    &error,
                ));
            }
            return Err(error);
        }
        snapshot_in(&connection, actor, &canonical)
    }

    pub fn agent_edit(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
        expected_rev: u64,
        title: &str,
    ) -> Result<AgentSnapshot> {
        let (session, branch) = {
            let connection = self.connection()?;
            key_for_ref_in(&connection, canonical_ref)?
        };
        self.set_agent_title(actor, &session, &branch, expected_rev, title)?;
        self.agent_get(actor, canonical_ref)
    }

    pub fn agent_admission(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
        state: AgentAdmission,
        expected_rev: Option<u64>,
    ) -> Result<AgentSnapshot> {
        self.agent_admission_in(actor, canonical_ref, state, expected_rev, None)
    }

    /// Explicit Manage operation under the actual live source execution
    /// decision. A configured route or persisted grant alone cannot reopen.
    pub fn agent_open_with_source(
        &self,
        manager: &PrincipalId,
        expected_rev: u64,
        source: &crate::SandboxExecution,
    ) -> Result<AgentSnapshot> {
        self.agent_admission_in(
            manager,
            source.agent().canonical_ref(),
            AgentAdmission::Open,
            Some(expected_rev),
            Some(source),
        )
    }

    fn agent_admission_in(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
        state: AgentAdmission,
        expected_rev: Option<u64>,
        source: Option<&crate::SandboxExecution>,
    ) -> Result<AgentSnapshot> {
        let guard = crate::sandbox_owner::execution::read(source, self)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (session, branch) = key_for_ref_in(&tx, canonical_ref)?;
        let agent = agent_owner::AgentKey::new(&session, &branch);
        agent_owner::authorize_permission(&tx, actor, agent, AgentPermission::Manage)?;
        let (rev, changed) = agent_owner::set_admission(
            &tx,
            agent,
            expected_rev,
            state.as_str(),
            guard.as_ref(),
            source.map_or(actor, |source| source.actor()),
        )?;
        if changed {
            audit(
                &tx,
                &session,
                actor,
                "agent.admission.changed",
                serde_json::json!({
                    "branch_id":branch,
                    "state":state,
                    "rev":rev,
                }),
            )?;
        }
        db(tx.commit())?;
        snapshot_in(&connection, actor, canonical_ref)
    }

    /// Read-only host close preflight. This prevents live-runtime topology from
    /// becoming observable to a caller that lacks Agent control, while the
    /// later request transaction still revalidates the same authority/revision
    /// at the actual lifecycle linearization point.
    pub fn agent_close_preflight(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
        expected_rev: Option<u64>,
    ) -> Result<AgentSnapshot> {
        let connection = self.connection()?;
        let (session, branch) = key_for_ref_in(&connection, canonical_ref)?;
        let agent = agent_owner::AgentKey::new(&session, &branch);
        agent_owner::authorize_permission(&connection, actor, agent, AgentPermission::Control)?;
        let revision = agent_owner::revision(&connection, agent)?;
        if expected_rev.is_some_and(|expected| expected != revision) {
            return Err(Error::Conflict("stale Agent revision"));
        }
        snapshot_in(&connection, actor, canonical_ref)
    }

    /// Lower close lifecycle seam for the live host. This atomically fences
    /// future admission and records the close request, but deliberately does not
    /// fabricate runtime cancellation/settlement; the outer live host must pair
    /// it with the existing factual control owners before advertising full
    /// agent.close completion.
    pub fn agent_request_close(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
        expected_rev: Option<u64>,
    ) -> Result<AgentSnapshot> {
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (session, branch) = key_for_ref_in(&tx, canonical_ref)?;
        let agent = agent_owner::AgentKey::new(&session, &branch);
        agent_owner::authorize_permission(&tx, actor, agent, AgentPermission::Control)?;
        let before = agent_owner::revision(&tx, agent)?;
        if expected_rev.is_some_and(|expected| expected != before) {
            return Err(Error::Conflict("stale Agent revision"));
        }
        let (lifecycle, _) = agent_owner::lifecycle(&tx, agent)?;
        agent_owner::request_close(&tx, agent)?;
        crate::agent_job_events::retire_recipient_in(&tx, canonical_ref)?;
        crate::event_subscription::disable_recipient_in(&tx, canonical_ref)?;
        if lifecycle == "open" {
            audit(
                &tx,
                &session,
                actor,
                "agent.close_requested",
                serde_json::json!({
                    "branch_id":branch,
                    "rev":before.saturating_add(1),
                }),
            )?;
        }
        db(tx.commit())?;
        snapshot_in(&connection, actor, canonical_ref)
    }

    /// Lower terminal-close seam for the live host.
    ///
    /// The caller must already have driven every non-Store factual owner
    /// (finite Jobs, Processes, etc.) to terminal/quiescent state. This
    /// transaction owns only the authoritative Agent/Turn boundary: it refuses
    /// to publish `closed` while a journaled Turn is still running.
    pub fn agent_finish_close(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
    ) -> Result<AgentSnapshot> {
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (session, branch) = key_for_ref_in(&tx, canonical_ref)?;
        let agent = agent_owner::AgentKey::new(&session, &branch);
        agent_owner::authorize_permission(&tx, actor, agent, AgentPermission::Control)?;
        let (lifecycle, _) = agent_owner::lifecycle(&tx, agent)?;
        if lifecycle == "closed" {
            db(tx.commit())?;
            return snapshot_in(&connection, actor, canonical_ref);
        }
        if lifecycle != "closing" {
            return Err(Error::Conflict(
                "Agent terminal close requires an existing close request",
            ));
        }
        let active: bool = db(tx.query_row(
            "SELECT EXISTS(
                SELECT 1
                FROM branches b
                JOIN runs r ON r.id=b.active_run
                WHERE b.session_id=?1 AND b.id=?2
                  AND r.session_id=?1 AND r.branch_id=?2
                  AND r.status='running'
             )",
            params![session, branch],
            |row| row.get(0),
        ))?;
        if active {
            return Err(Error::Conflict("Agent still has an active journaled Turn"));
        }
        let before = agent_owner::revision(&tx, agent)?;
        if !agent_owner::close(&tx, agent)? {
            return Err(Error::Conflict("Agent terminal close did not apply"));
        }
        audit(
            &tx,
            &session,
            actor,
            "agent.closed",
            serde_json::json!({
                "branch_id":branch,
                "rev":before.saturating_add(1),
            }),
        )?;
        db(tx.commit())?;
        snapshot_in(&connection, actor, canonical_ref)
    }

    pub fn agent_access_list_ref(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
    ) -> Result<AgentAccessSnapshot> {
        let connection = self.connection()?;
        let (session, branch) = key_for_ref_in(&connection, canonical_ref)?;
        agent_owner::access_snapshot(
            &connection,
            actor,
            agent_owner::AgentKey::new(&session, &branch),
        )
    }

    pub fn agent_access_set_ref(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
        principal_ref: &str,
        permissions: Option<AgentPermissions>,
        expected_rev: u64,
    ) -> Result<AgentSnapshot> {
        let (session, branch) = {
            let connection = self.connection()?;
            key_for_ref_in(&connection, canonical_ref)?
        };
        self.agent_access_set(
            actor,
            &session,
            &branch,
            principal_ref,
            permissions,
            expected_rev,
        )?;
        self.agent_get(actor, canonical_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SecretStore;
    use aw_store_api::SessionBackend;
    use aw_tools::job_event::{EventRule, EventRuleSet, JobEventKind, JobWakeMode};
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    struct PrivateTemp(PathBuf);

    impl PrivateTemp {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "aw-agent-create-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            aw_platform::ensure_private_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for PrivateTemp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn binding() -> SessionBinding {
        SessionBinding {
            working_directory: PathBuf::from("/tmp"),
            tool_root: PathBuf::from("/tmp"),
            system_prompt: "system".into(),
            profile_hint: "fixture".into(),
            model_hint: "model".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    #[test]
    fn host_agent_state_shell_creates_reads_edits_and_fences_future_admission() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("internal:fixture:host-agent").unwrap();
        let (root, branch) = store.create(&actor, "root", binding()).unwrap();
        let root_binding = store.agent_binding(&actor, &root.id, &branch.id).unwrap();
        let root_ref = canonical_agent_ref(
            &root_binding.domain_handle,
            &root_binding.identity_handle,
            &root_binding.project_handle,
            &root_binding.agent_handle,
        );
        let project_ref = format!(
            "/d:{}/i:{}/p:{}",
            root_binding.domain_handle, root_binding.identity_handle, root_binding.project_handle
        );
        let root_sandbox = store
            .agent_get(&actor, &root_ref)
            .unwrap()
            .sandbox_ref
            .expect("v41 root Agent must have a Sandbox");
        let context = AgentCreateContext {
            current_project_ref: project_ref.clone(),
            current_agent_ref: Some(root_ref.clone()),
            binding: binding(),
            auto_sandbox_ref: Some(root_sandbox.clone()),
            request_id: None,
            execution_site: "local".into(),
        };

        let created = store
            .agent_create(
                &actor,
                &context,
                AgentCreateRequest {
                    title: Some("host child".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(created.title, "host child");
        assert_eq!(created.lifecycle, AgentLifecycle::Open);
        assert_eq!(created.admission, AgentAdmission::Open);
        assert_ne!(created.canonical_ref, root_ref);
        assert_eq!(created.sandbox_ref.as_deref(), Some(root_sandbox.as_str()));

        let explicit = store
            .sandbox_create_fresh(&actor, &project_ref, &binding())
            .unwrap();
        let explicit_child = store
            .agent_create(
                &actor,
                &context,
                AgentCreateRequest {
                    title: Some("explicit sandbox child".into()),
                    sandbox: AgentSandboxSelection::Existing(explicit.canonical_ref.clone()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(
            explicit_child.sandbox_ref.as_deref(),
            Some(explicit.canonical_ref.as_str())
        );

        let mut other_layout = binding();
        other_layout.working_directory = PathBuf::from("/var/tmp");
        other_layout.tool_root = PathBuf::from("/var/tmp");
        let incompatible = store
            .sandbox_create_fresh(&actor, &project_ref, &other_layout)
            .unwrap();
        assert!(matches!(
            store.agent_create(
                &actor,
                &context,
                AgentCreateRequest {
                    title: Some("wrong sandbox layout".into()),
                    sandbox: AgentSandboxSelection::Existing(incompatible.canonical_ref),
                    ..Default::default()
                },
            ),
            Err(Error::Conflict(_))
        ));

        let redirected_workspace = store
            .create_workspace(&actor, Some(&root_binding.domain_handle), "redirected")
            .unwrap();
        let redirected_project = format!(
            "/d:{}/i:{}/p:{}",
            root_binding.domain_handle, root_binding.identity_handle, redirected_workspace.id
        );
        assert!(matches!(
            store.agent_create(
                &actor,
                &context,
                AgentCreateRequest {
                    project: Some(redirected_project.clone()),
                    title: Some("redirect auto unavailable".into()),
                    ..Default::default()
                },
            ),
            Err(Error::Conflict(_))
        ));
        let redirected_sandbox = store
            .sandbox_create_fresh(&actor, &redirected_project, &binding())
            .unwrap();
        let redirected = store
            .agent_create(
                &actor,
                &context,
                AgentCreateRequest {
                    project: Some(redirected_project.clone()),
                    title: Some("redirected fresh".into()),
                    sandbox: AgentSandboxSelection::Existing(
                        redirected_sandbox.canonical_ref.clone(),
                    ),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(redirected.project_ref, redirected_project);
        assert_eq!(
            redirected.sandbox_ref.as_deref(),
            Some(redirected_sandbox.canonical_ref.as_str())
        );
        let redirected_history = store
            .agent_create(
                &actor,
                &context,
                AgentCreateRequest {
                    from: Some(root_ref.clone()),
                    project: Some(redirected_project.clone()),
                    title: Some("redirected history".into()),
                    sandbox: AgentSandboxSelection::Existing(
                        redirected_sandbox.canonical_ref.clone(),
                    ),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(redirected_history.project_ref, redirected_project);
        assert_eq!(
            redirected_history.sandbox_ref.as_deref(),
            Some(redirected_sandbox.canonical_ref.as_str())
        );

        let (_, subscription) = store
            .replace_agent_job_subscription(
                &actor,
                &root.id,
                &branch.id,
                None,
                None,
                &created.canonical_ref,
                "jwa_host_close_fixture",
                EventRuleSet::new([EventRule::new(JobEventKind::Settled, JobWakeMode::Queue)])
                    .unwrap(),
                8,
            )
            .unwrap();
        assert!(subscription.active);
        let source_rev_before_close = store
            .agent_public_revision(&actor, &root.id, &branch.id)
            .unwrap();

        let listed = store.agent_list(&actor, &project_ref, None, 16).unwrap();
        assert!(listed
            .iter()
            .any(|candidate| candidate.canonical_ref == created.canonical_ref));

        let edited = store
            .agent_edit(
                &actor,
                &created.canonical_ref,
                created.rev,
                "host child edited",
            )
            .unwrap();
        assert_eq!(edited.title, "host child edited");
        assert!(edited.rev > created.rev);

        let held = store
            .agent_admission(
                &actor,
                &created.canonical_ref,
                AgentAdmission::Held,
                Some(edited.rev),
            )
            .unwrap();
        assert_eq!(held.admission, AgentAdmission::Held);
        assert!(held.rev > edited.rev);

        let collaborator = PrincipalId::new("internal:fixture:host-agent-collaborator").unwrap();
        let group = store
            .create_principal_group(
                &actor,
                Some(&root_binding.domain_handle),
                "Host reviewers",
                std::slice::from_ref(&collaborator),
            )
            .unwrap();
        let granted = store
            .agent_access_set_ref(
                &actor,
                &created.canonical_ref,
                &group.reference,
                Some(AgentPermissions::VIEW_ONLY),
                held.rev,
            )
            .unwrap();
        assert!(granted.rev > held.rev);
        let access = store
            .agent_access_list_ref(&actor, &created.canonical_ref)
            .unwrap();
        assert_eq!(access.rev, granted.rev);
        assert!(access.caller_permissions.manage);
        assert!(access.entries.iter().any(|entry| {
            entry.principal_ref == group.reference
                && entry.principal_kind == "group"
                && entry.permissions == AgentPermissions::VIEW_ONLY
        }));
        assert!(matches!(
            store.agent_finish_close(&actor, &created.canonical_ref),
            Err(Error::Conflict(_))
        ));

        let closing = store
            .agent_request_close(&actor, &created.canonical_ref, Some(granted.rev))
            .unwrap();
        assert_eq!(closing.lifecycle, AgentLifecycle::Closing);
        assert_eq!(closing.admission, AgentAdmission::Held);
        assert!(closing.rev > held.rev);
        let (_, retired) = store
            .replace_agent_job_subscription(
                &actor,
                &root.id,
                &branch.id,
                None,
                None,
                &created.canonical_ref,
                "jwa_host_close_fixture",
                EventRuleSet::default(),
                8,
            )
            .unwrap();
        assert!(!retired.changed);
        assert_eq!(
            store
                .agent_public_revision(&actor, &root.id, &branch.id)
                .unwrap(),
            source_rev_before_close + 1,
            "recipient close retires the inbound source rule exactly once"
        );
        assert!(matches!(
            store.agent_admission(
                &actor,
                &created.canonical_ref,
                AgentAdmission::Open,
                Some(closing.rev),
            ),
            Err(Error::Conflict(_))
        ));
        let closed = store
            .agent_finish_close(&actor, &created.canonical_ref)
            .unwrap();
        assert_eq!(closed.lifecycle, AgentLifecycle::Closed);
        assert_eq!(closed.admission, AgentAdmission::Held);
        assert!(closed.rev > closing.rev);
        let replay = store
            .agent_finish_close(&actor, &created.canonical_ref)
            .unwrap();
        assert_eq!(replay.rev, closed.rev);
        assert_eq!(replay.lifecycle, AgentLifecycle::Closed);

        let failed = store.agent_create(
            &actor,
            &context,
            AgentCreateRequest {
                title: Some("must not publish".into()),
                task: Some("run now".into()),
                ..Default::default()
            },
        );
        assert!(matches!(failed, Err(Error::Denied(_))));
        let after = store.agent_list(&actor, &project_ref, None, 16).unwrap();
        assert!(!after
            .iter()
            .any(|candidate| candidate.title == "must not publish"));
    }

    #[test]
    fn task_bearing_create_publishes_pending_input_atomically_and_replays_exactly() {
        let temp = PrivateTemp::new();
        let store = SessionStore::open(&temp.0).unwrap();
        let actor = PrincipalId::new("internal:fixture:host-task-create").unwrap();
        let (root, branch) = store.create(&actor, "root", binding()).unwrap();
        let root_binding = store.agent_binding(&actor, &root.id, &branch.id).unwrap();
        let root_ref = canonical_agent_ref(
            &root_binding.domain_handle,
            &root_binding.identity_handle,
            &root_binding.project_handle,
            &root_binding.agent_handle,
        );
        let project_ref = format!(
            "/d:{}/i:{}/p:{}",
            root_binding.domain_handle, root_binding.identity_handle, root_binding.project_handle
        );
        let context = AgentCreateContext {
            current_project_ref: project_ref.clone(),
            current_agent_ref: Some(root_ref.clone()),
            binding: binding(),
            auto_sandbox_ref: None,
            request_id: Some("host-task-create-1".into()),
            execution_site: "local".into(),
        };
        let request = AgentCreateRequest {
            title: Some("task child".into()),
            task: Some("perform the initial task".into()),
            ..Default::default()
        };

        let created = store
            .agent_create(&actor, &context, request.clone())
            .unwrap();
        let connection = store.connection().unwrap();
        let (child_session, child_branch) =
            key_for_ref_in(&connection, &created.canonical_ref).unwrap();
        let queue = actor_queue::events(&connection, &child_session).unwrap();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].state, crate::QueueState::Pending);
        assert_eq!(queue[0].request.branch_id, child_branch);
        assert_eq!(queue[0].request.recipient_site, "local");
        assert!(queue[0].request.request_id.starts_with("hq_"));
        let encoded = serde_json::to_string(&queue[0]).unwrap();
        assert!(!encoded.contains("perform the initial task"));
        let secret = store
            .local_secret_store()
            .unwrap()
            .get(&queue[0].payload_ref)
            .unwrap();
        assert_eq!(secret.as_slice(), b"perform the initial task");

        let preparation =
            agent_create_preparation_in(&connection, &root.id, &branch.id, "host-task-create-1")
                .unwrap()
                .unwrap();
        assert_eq!(preparation.state, AgentCreatePreparationState::Applied);
        assert_eq!(
            preparation.result_ref.as_deref(),
            Some(created.canonical_ref.as_str())
        );
        drop(connection);

        let replay = store
            .agent_create(&actor, &context, request.clone())
            .unwrap();
        assert_eq!(replay.canonical_ref, created.canonical_ref);
        assert_eq!(replay.rev, created.rev);
        let listed = store.agent_list(&actor, &project_ref, None, 16).unwrap();
        assert_eq!(
            listed
                .iter()
                .filter(|candidate| candidate.title == "task child")
                .count(),
            1
        );

        let mut changed = request;
        changed.task = Some("changed task".into());
        assert!(matches!(
            store.agent_create(&actor, &context, changed),
            Err(Error::Denied(_))
        ));
    }
}
