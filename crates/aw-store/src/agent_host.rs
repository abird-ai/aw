//! Thin DL-200 host Agent state shell.
//!
//! This module owns no Agent/Session/Project/Sandbox state. It composes the
//! authoritative Agent/history/resource owners and keeps trusted host selection
//! separate from model-visible constructor arguments.

use crate::{
    agent_owner, audit, db, history_owner, principal_groups, resources, stable_user_principal,
    user_for_principal, AgentAccessSnapshot, AgentHistoryForkAccess, AgentPermission,
    AgentPermissions, Error, PrincipalId, Result, SessionBinding, SessionStore,
};
use aw_tools::execution_ref::{AgentRef, ExecutionRef, ExecutionSegment};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MAX_AGENT_LIST: usize = 256;
const MAX_INITIAL_ACCESS: usize = 512;

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProjectRef {
    domain: String,
    identity: String,
    project: String,
}

impl ProjectRef {
    fn canonical(&self) -> String {
        format!("/d:{}/i:{}/p:{}", self.domain, self.identity, self.project)
    }
}

fn parse_project_ref(value: &str) -> Result<ProjectRef> {
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

fn snapshot_in(
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

fn install_initial_custom_access(
    connection: &Connection,
    actor: &PrincipalId,
    session: &str,
    branch: &str,
    grants: &[AgentCreateAccessGrant],
) -> Result<()> {
    if grants.len() > MAX_INITIAL_ACCESS {
        return Err(Error::Limit("initial Agent access list exceeds bound"));
    }
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
    let mut seen = BTreeSet::new();
    for grant in grants {
        let permissions = grant.permissions.validate()?;
        if permissions.is_empty() {
            return Err(Error::Denied(
                "initial Agent access grants cannot be empty; omit the principal instead",
            ));
        }
        let (kind, handle) =
            principal_groups::resolve_principal_ref(connection, &domain, &grant.principal)?;
        if kind == owner_kind && handle == owner_handle {
            return Err(Error::Denied("Agent owner access is implicit"));
        }
        if !seen.insert((kind.clone(), handle.clone())) {
            return Err(Error::Denied("duplicate principal in initial Agent access"));
        }
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

    /// Phase-1 host constructor cut. It supports fresh or exact-history dormant
    /// Agent creation with trusted sandbox=auto binding, source/current Project
    /// defaulting and private/inherited/custom access. Initial task admission and
    /// explicit logical Sandbox refs remain fail-closed until their runtime owner
    /// is composed by the outer host shell.
    pub fn agent_create(
        &self,
        actor: &PrincipalId,
        context: &AgentCreateContext,
        request: AgentCreateRequest,
    ) -> Result<AgentSnapshot> {
        crate::validate_binding(&context.binding)?;
        let current_project = parse_project_ref(&context.current_project_ref)?;
        if request
            .task
            .as_deref()
            .is_some_and(|task| task.trim().is_empty() || task.contains('\0'))
        {
            return Err(Error::Denied("invalid initial Agent task"));
        }
        if request.task.is_some() {
            return Err(Error::Denied(
                "initial Agent task requires the live host runtime admission shell",
            ));
        }
        if let AgentSandboxSelection::Existing(_) = request.sandbox {
            return Err(Error::Denied(
                "explicit Sandbox refs require the logical Sandbox owner",
            ));
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

        if let Some(source_project) = &source_project {
            if &selected_project != source_project {
                return Err(Error::Denied(
                    "cross-Project history construction requires reviewed redirect authority",
                ));
            }
        } else if selected_project != current_project {
            return Err(Error::Denied(
                "fresh Agent destination must match the trusted current Project",
            ));
        }

        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        validate_project_exists(&tx, &selected_project)?;
        let (session, branch) = if let Some((canonical_source, source_is_agent, _)) = source {
            let fork_access = match &request.access {
                AgentCreateAccess::Private | AgentCreateAccess::Custom(_) => {
                    AgentHistoryForkAccess::Private
                }
                AgentCreateAccess::Inherit if source_is_agent => AgentHistoryForkAccess::Inherit,
                AgentCreateAccess::Inherit => {
                    return Err(Error::Denied(
                        "access=inherit requires an Agent history source, not a Turn",
                    ))
                }
            };
            history_owner::create_agent_from_history_ref_in(
                &tx,
                actor,
                &canonical_source,
                title,
                &context.binding,
                fork_access,
            )?
        } else {
            if matches!(request.access, AgentCreateAccess::Inherit) {
                return Err(Error::Denied(
                    "access=inherit requires an Agent history source",
                ));
            }
            let owner = stable_user_principal(&selected_project.identity)?;
            resources::create_in_workspace_tx(
                &tx,
                &owner,
                title,
                &context.binding,
                &selected_project.project,
                None,
            )?
        };

        if let AgentCreateAccess::Custom(grants) = &request.access {
            install_initial_custom_access(&tx, actor, &session.id, &branch.id, grants)?;
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
                "sandbox":"auto",
            }),
        )?;
        db(tx.commit())?;
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
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (session, branch) = key_for_ref_in(&tx, canonical_ref)?;
        let agent = agent_owner::AgentKey::new(&session, &branch);
        agent_owner::authorize_permission(&tx, actor, agent, AgentPermission::Manage)?;
        let (rev, changed) = agent_owner::set_admission(&tx, agent, expected_rev, state.as_str())?;
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
    use aw_store_api::SessionBackend;
    use aw_tools::job_event::{EventRule, EventRuleSet, JobEventKind, JobWakeMode};
    use std::path::PathBuf;

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
        let context = AgentCreateContext {
            current_project_ref: project_ref.clone(),
            current_agent_ref: Some(root_ref.clone()),
            binding: binding(),
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

        let access = store
            .agent_access_list_ref(&actor, &created.canonical_ref)
            .unwrap();
        assert_eq!(access.rev, held.rev);
        assert!(access.caller_permissions.manage);
        assert!(matches!(
            store.agent_finish_close(&actor, &created.canonical_ref),
            Err(Error::Conflict(_))
        ));

        let closing = store
            .agent_request_close(&actor, &created.canonical_ref, Some(held.rev))
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
}
