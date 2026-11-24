//! DL-200 logical Sandbox owner.
//!
//! A Sandbox owns durable Project-scoped mutable execution identity, layout
//! and named SourceBindings. It does not own Process lifetime, Vault
//! credentials, mount/network/device policy, or Agent ACLs.

use crate::{
    db, durability, id, now, session_in, user_for_principal, workspace_writer, Error, PrincipalId,
    Result, SessionBinding, SessionStore,
};
use aw_agent_core::{SourceBinding, SourceDriverRegistry, SourceSetLimit};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const MAX_SANDBOX_LIST: usize = 256;

mod effects;
pub(crate) use effects::journal::EFFECT_PREFIX as JOURNALED_SOURCE_EFFECT_PREFIX;
pub use effects::{
    PreparedSandboxEffect, SandboxEffectAuthority, SandboxEffectLease, SandboxEffectOrigin,
    SandboxEffectOutcome, SandboxEffectPlan, SandboxEffectRequest, SandboxToolRecovery,
};
mod grants;
pub use grants::{
    EffectiveSourceBinding, SandboxGrantObservation, SandboxGrantPolicy, SandboxGrantStatus,
    SandboxPolicyEvidence, SandboxSourceGrant,
};
pub(crate) mod source_bindings;
pub use source_bindings::{SourceBindingCompatibility, SourceBindingSummary};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxLifecycle {
    Open,
    Closed,
}

impl SandboxLifecycle {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "open" => Ok(Self::Open),
            "closed" => Ok(Self::Closed),
            _ => Err(Error::Storage("invalid authoritative Sandbox lifecycle")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxSnapshot {
    pub canonical_ref: String,
    pub project_ref: String,
    pub rev: u64,
    pub lifecycle: SandboxLifecycle,
    pub parent_sandbox_ref: Option<String>,
    pub working_directory: PathBuf,
    pub tool_root: PathBuf,
    pub layout_digest: String,
    #[serde(default)]
    pub sources: Vec<SourceBindingSummary>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProjectKey {
    domain: String,
    identity: String,
    project: String,
}

impl ProjectKey {
    fn canonical(&self) -> String {
        format!("/d:{}/i:{}/p:{}", self.domain, self.identity, self.project)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SandboxKey {
    project: ProjectKey,
    sandbox: String,
}

impl SandboxKey {
    fn canonical(&self) -> String {
        format!("{}/s:{}", self.project.canonical(), self.sandbox)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Layout {
    working_directory: String,
    tool_root: String,
    digest: String,
}

fn parse_project_ref(value: &str) -> Result<ProjectKey> {
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
    Ok(ProjectKey {
        domain: domain.to_owned(),
        identity: identity.to_owned(),
        project: project.to_owned(),
    })
}

fn parse_sandbox_ref(value: &str) -> Result<SandboxKey> {
    let Some((project, sandbox)) = value.rsplit_once("/s:") else {
        return Err(Error::Denied("Sandbox ref must be canonical"));
    };
    let project = parse_project_ref(project)?;
    crate::validate_agent_handle_component(sandbox)?;
    Ok(SandboxKey {
        project,
        sandbox: sandbox.to_owned(),
    })
}

fn layout(binding: &SessionBinding) -> Result<Layout> {
    crate::validate_binding(binding)?;
    let working_directory = binding
        .working_directory
        .to_str()
        .ok_or(Error::Config(
            "Sandbox working directory must be UTF-8".into(),
        ))?
        .to_owned();
    let tool_root = binding
        .tool_root
        .to_str()
        .ok_or(Error::Config("Sandbox tool root must be UTF-8".into()))?
        .to_owned();
    let mut hash = Sha256::new();
    hash.update(b"aw-sandbox-layout-v1\0");
    for value in [working_directory.as_bytes(), tool_root.as_bytes()] {
        hash.update((value.len() as u64).to_be_bytes());
        hash.update(value);
    }
    Ok(Layout {
        working_directory,
        tool_root,
        digest: format!("{:x}", hash.finalize()),
    })
}

fn table_exists(connection: &Connection) -> Result<bool> {
    db(connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sqlite_master
             WHERE type='table' AND name='sandboxes_current'
         )",
        [],
        |row| row.get(0),
    ))
}

fn authorize_project(
    connection: &Connection,
    actor: &PrincipalId,
    project: &ProjectKey,
    write: bool,
) -> Result<()> {
    let user = user_for_principal(connection, actor)?
        .ok_or(Error::Denied("Sandbox Project authority unavailable"))?;
    if user != project.identity {
        return Err(Error::Denied(
            "Sandbox Project principal differs from authenticated user",
        ));
    }
    let authorized: bool = db(connection.query_row(
        "SELECT EXISTS(
             SELECT 1
             FROM workspaces w
             JOIN domain_memberships m
               ON m.domain_id=w.domain_id AND m.user_id=?2 AND m.status='active'
             JOIN domain_principals p
               ON p.domain_id=w.domain_id AND p.principal_handle=?2
              AND p.kind='user' AND p.user_id=?2 AND p.status='active'
             WHERE w.workspace_id=?1 AND w.domain_id=?3 AND w.archived_at IS NULL
         )",
        params![project.project, project.identity, project.domain],
        |row| row.get(0),
    ))?;
    if !authorized {
        return Err(Error::Denied(
            "Sandbox Project is unavailable or not permitted",
        ));
    }
    if write {
        workspace_writer::require_workspace_writer(connection, &project.project)?;
    }
    Ok(())
}

fn snapshot_row(connection: &Connection, canonical_ref: &str) -> Result<SandboxSnapshot> {
    let row: Option<(
        String,
        String,
        String,
        String,
        u64,
        String,
        Option<String>,
        String,
        String,
        String,
        i64,
        i64,
    )> = db(connection
        .query_row(
            "SELECT sandbox_id,domain_handle,identity_handle,project_handle,rev,lifecycle,
                    parent_sandbox_id,working_directory,tool_root,layout_digest,created_at,updated_at
             FROM sandboxes_current WHERE sandbox_id=?1",
            [canonical_ref],
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
                ))
            },
        )
        .optional())?;
    let row = row.ok_or(Error::Denied("Sandbox ref is unavailable or not permitted"))?;
    Ok(SandboxSnapshot {
        canonical_ref: row.0,
        project_ref: format!("/d:{}/i:{}/p:{}", row.1, row.2, row.3),
        rev: row.4,
        lifecycle: SandboxLifecycle::parse(&row.5)?,
        parent_sandbox_ref: row.6,
        working_directory: PathBuf::from(row.7),
        tool_root: PathBuf::from(row.8),
        layout_digest: row.9,
        sources: Vec::new(),
        created_at: row.10,
        updated_at: row.11,
    })
}

fn snapshot_with_sources(connection: &Connection, canonical_ref: &str) -> Result<SandboxSnapshot> {
    let mut snapshot = snapshot_row(connection, canonical_ref)?;
    if source_bindings::available(connection)? {
        snapshot.sources = source_bindings::summaries(connection, canonical_ref)?;
    }
    Ok(snapshot)
}

fn insert_sandbox(
    connection: &Connection,
    project: &ProjectKey,
    handle: &str,
    parent: Option<&str>,
    layout: &Layout,
    event_actor: Option<&PrincipalId>,
) -> Result<String> {
    crate::validate_agent_handle_component(handle)?;
    let key = SandboxKey {
        project: project.clone(),
        sandbox: handle.to_owned(),
    };
    let canonical = key.canonical();
    let created = now();
    db(connection.execute(
        "INSERT INTO sandboxes_current(
             sandbox_id,domain_handle,identity_handle,project_handle,sandbox_handle,
             parent_sandbox_id,working_directory,tool_root,layout_digest,
             rev,lifecycle,created_at,updated_at
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,1,'open',?10,?10)",
        params![
            canonical,
            project.domain,
            project.identity,
            project.project,
            handle,
            parent,
            layout.working_directory,
            layout.tool_root,
            layout.digest,
            created
        ],
    ))?;
    if let Some(actor) = event_actor {
        let tx = durability::tx_id()?;
        durability::commit_event(
            connection,
            &tx,
            "sandbox.created",
            "sandbox",
            &canonical,
            Some(actor),
            serde_json::json!({
                "sandbox_ref":canonical,
                "project_ref":project.canonical(),
                "parent_sandbox_ref":parent,
                "rev":1,
            }),
        )?;
    }
    Ok(canonical)
}

/// Reject a known existing layout for an isolated construction. The
/// trusted driver/host still owns physical isolation (including aliases).
pub(crate) fn require_fresh_layout_in(
    connection: &Connection,
    project_ref: &str,
    binding: &SessionBinding,
) -> Result<()> {
    let project = parse_project_ref(project_ref)?;
    let layout = layout(binding)?;
    let exists: bool = db(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sandboxes_current
         WHERE domain_handle=?1 AND identity_handle=?2 AND project_handle=?3
           AND lifecycle='open' AND layout_digest=?4
           AND working_directory=?5 AND tool_root=?6)",
        params![
            project.domain,
            project.identity,
            project.project,
            layout.digest,
            layout.working_directory,
            layout.tool_root
        ],
        |row| row.get(0),
    ))?;
    if exists {
        return Err(Error::Conflict(
            "isolated construction requires a fresh Sandbox layout",
        ));
    }
    Ok(())
}

/// Constructor-owned fresh Sandbox identity. Only its publication owner can
/// mint this token; ordinary layout selection cannot attach source-bearing
/// state or reinterpret a fresh candidate as a shared Sandbox.
pub(crate) struct FreshSandbox {
    reference: String,
}

pub(crate) fn publish_fresh_in(
    tx: &Transaction<'_>,
    actor: &PrincipalId,
    canonical_ref: &str,
    binding: &SessionBinding,
    sources: &[SourceBinding],
    registry: &SourceDriverRegistry,
    limit: SourceSetLimit,
) -> Result<FreshSandbox> {
    source_bindings::validate_set(limit, registry, sources)?;
    let key = parse_sandbox_ref(canonical_ref)?;
    authorize_project(tx, actor, &key.project, true)?;
    let reference = insert_sandbox(
        tx,
        &key.project,
        &key.sandbox,
        None,
        &layout(binding)?,
        Some(actor),
    )?;
    source_bindings::insert(tx, &reference, sources)?;
    Ok(FreshSandbox { reference })
}

/// Attach only as part of the original Agent creation. Source-bearing
/// execution remains held until the grant/realization owner can admit it.
pub(crate) fn bind_fresh_initial_in(
    connection: &Connection,
    session: &str,
    branch: &str,
    fresh: &FreshSandbox,
) -> Result<()> {
    if connection.is_autocommit() {
        return Err(Error::Storage(
            "fresh Sandbox binding requires the construction transaction",
        ));
    }
    let (project, expected, current, rev, lifecycle) =
        agent_project_and_layout(connection, session, branch)?;
    let sandbox = snapshot_row(connection, &fresh.reference)?;
    let used: bool = db(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_state_current WHERE sandbox_id=?1)",
        [&fresh.reference],
        |row| row.get(0),
    ))?;
    if current.is_some()
        || rev != 1
        || lifecycle != "open"
        || used
        || sandbox.rev != 1
        || sandbox.lifecycle != SandboxLifecycle::Open
        || sandbox.project_ref != project.canonical()
        || sandbox.layout_digest != expected.digest
        || sandbox.working_directory.to_str() != Some(expected.working_directory.as_str())
        || sandbox.tool_root.to_str() != Some(expected.tool_root.as_str())
    {
        return Err(Error::Conflict(
            "fresh Sandbox differs from initial Agent layout",
        ));
    }
    let has_sources = source_bindings::has_sources(connection, &fresh.reference)?;
    let changed = db(connection.execute(
        "UPDATE agent_state_current SET sandbox_id=?1,admission=?2
         WHERE session_id=?3 AND branch_id=?4 AND sandbox_id IS NULL AND rev=1",
        params![
            fresh.reference,
            if has_sources { "held" } else { "open" },
            session,
            branch
        ],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict("fresh Sandbox initial binding changed"));
    }
    Ok(())
}

/// The backing Session must not bypass source-aware execution admission via
/// a legacy activation/branch/tool path. Until realization and grants are
/// installed, any source-bearing Agent in that Session is effect-inert.
pub(crate) fn require_execution_in(connection: &Connection, session: &str) -> Result<()> {
    let blocked: bool = db(connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM agent_state_current a
             JOIN sandbox_source_bindings_current b ON b.sandbox_id=a.sandbox_id
             WHERE a.session_id=?1
         )",
        [session],
        |row| row.get(0),
    ))?;
    if blocked {
        return Err(Error::Denied(
            "source-bearing Agent Sandbox realization is unavailable",
        ));
    }
    Ok(())
}

/// Legacy runs/hazards still fence a source layout until every writer
/// participates in source custody. Include the whole backing Session of
/// every sharing Agent, as well as the exact legacy tool-root key.
pub(crate) fn require_layout_quiescent_in(
    connection: &Connection,
    sandbox: &str,
    root_key: &str,
) -> Result<()> {
    let blocked: bool = db(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM runs WHERE root_key=?1 AND status='running')
         OR EXISTS(SELECT 1 FROM hazards WHERE root_key=?1 AND state<>'settled')
         OR EXISTS(
            SELECT 1 FROM agent_state_current a JOIN branches b ON b.session_id=a.session_id
            WHERE a.sandbox_id=?2 AND b.active_run IS NOT NULL
         )
         OR EXISTS(
            SELECT 1 FROM agent_state_current a JOIN runs r ON r.session_id=a.session_id
            WHERE a.sandbox_id=?2 AND r.status='running'
         )
         OR EXISTS(
            SELECT 1 FROM agent_state_current a JOIN hazards h ON h.session_id=a.session_id
            WHERE a.sandbox_id=?2 AND h.state<>'settled'
         )",
        params![root_key, sandbox],
        |row| row.get(0),
    ))?;
    if blocked {
        return Err(Error::Conflict(
            "Sandbox layout has active work or unresolved execution hazards",
        ));
    }
    Ok(())
}

fn ensure_layout(
    connection: &Connection,
    project: &ProjectKey,
    layout: &Layout,
    event_actor: Option<&PrincipalId>,
) -> Result<String> {
    if source_bindings::available(connection)? {
        let source_layout: bool = db(connection.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sandboxes_current s
                 JOIN sandbox_source_bindings_current b ON b.sandbox_id=s.sandbox_id
                 WHERE s.domain_handle=?1 AND s.identity_handle=?2 AND s.project_handle=?3
                   AND s.lifecycle='open' AND s.layout_digest=?4
                   AND s.working_directory=?5 AND s.tool_root=?6
             )",
            params![
                project.domain,
                project.identity,
                project.project,
                layout.digest,
                layout.working_directory,
                layout.tool_root
            ],
            |row| row.get(0),
        ))?;
        if source_layout {
            return Err(Error::Denied(
                "source-bearing Agent Sandbox realization is unavailable",
            ));
        }
    }
    let existing: Option<String> = db(connection
        .query_row(
            "SELECT sandbox_id FROM sandboxes_current
             WHERE domain_handle=?1 AND identity_handle=?2 AND project_handle=?3
               AND lifecycle='open' AND layout_digest=?4
               AND working_directory=?5 AND tool_root=?6
             ORDER BY CASE WHEN sandbox_handle='0' THEN 0 ELSE 1 END,created_at,sandbox_handle
             LIMIT 1",
            params![
                project.domain,
                project.identity,
                project.project,
                layout.digest,
                layout.working_directory,
                layout.tool_root
            ],
            |row| row.get(0),
        )
        .optional())?;
    if let Some(existing) = existing {
        // Do not fall back to an empty Sandbox for a source-bearing layout.
        if source_bindings::has_sources(connection, &existing)? {
            return Err(Error::Denied(
                "source-bearing Agent Sandbox realization is unavailable",
            ));
        }
        return Ok(existing);
    }
    let any: bool = db(connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sandboxes_current
             WHERE domain_handle=?1 AND identity_handle=?2 AND project_handle=?3
         )",
        params![project.domain, project.identity, project.project],
        |row| row.get(0),
    ))?;
    let handle = if any { id("sbx")? } else { "0".to_owned() };
    insert_sandbox(connection, project, &handle, None, layout, event_actor)
}

fn agent_project_and_layout(
    connection: &Connection,
    session: &str,
    branch: &str,
) -> Result<(ProjectKey, Layout, Option<String>, u64, String)> {
    let row: (String, String, String, Option<String>, u64, String) = db(connection.query_row(
        "SELECT domain_handle,identity_handle,project_handle,sandbox_id,rev,lifecycle
         FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
        params![session, branch],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        },
    ))?;
    let binding = session_in(connection, session)?.binding;
    Ok((
        ProjectKey {
            domain: row.0,
            identity: row.1,
            project: row.2,
        },
        layout(&binding)?,
        row.3,
        row.4,
        row.5,
    ))
}

fn bind_auto(
    connection: &Connection,
    session: &str,
    branch: &str,
    event_actor: Option<&PrincipalId>,
) -> Result<Option<String>> {
    if !table_exists(connection)? {
        return Ok(None);
    }
    let (project, layout, current, _, lifecycle) =
        agent_project_and_layout(connection, session, branch)?;
    if let Some(current) = current {
        let sandbox = snapshot_row(connection, &current)?;
        if source_bindings::has_sources(connection, &current)? {
            return Err(Error::Denied(
                "source-bearing Agent Sandbox realization is unavailable",
            ));
        }
        if sandbox.project_ref != project.canonical()
            || sandbox.layout_digest != layout.digest
            || sandbox.working_directory.to_str() != Some(layout.working_directory.as_str())
            || sandbox.tool_root.to_str() != Some(layout.tool_root.as_str())
            || (lifecycle == "open" && sandbox.lifecycle != SandboxLifecycle::Open)
        {
            return Err(Error::Conflict(
                "Agent Sandbox binding differs from its immutable execution layout",
            ));
        }
        return Ok(Some(current));
    }
    if source_bindings::available(connection)? {
        require_execution_in(connection, session)?;
    }
    let sandbox = ensure_layout(connection, &project, &layout, event_actor)?;
    let changed = db(connection.execute(
        "UPDATE agent_state_current SET sandbox_id=?1
         WHERE session_id=?2 AND branch_id=?3 AND sandbox_id IS NULL",
        params![sandbox, session, branch],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict(
            "Agent initial Sandbox binding changed concurrently",
        ));
    }
    Ok(Some(sandbox))
}

pub(crate) fn bind_auto_if_available(
    connection: &Connection,
    session: &str,
    branch: &str,
) -> Result<Option<String>> {
    if !table_exists(connection)? {
        return Ok(None);
    }
    let (project, _, _, _, _) = agent_project_and_layout(connection, session, branch)?;
    let actor = crate::stable_user_principal(&project.identity)?;
    bind_auto(connection, session, branch, Some(&actor))
}

pub(crate) fn seed_existing_agents(connection: &Connection) -> Result<usize> {
    let agents = {
        let mut query = db(connection.prepare(
            "SELECT session_id,branch_id
             FROM agent_state_current
             ORDER BY domain_handle,identity_handle,project_handle,created_at,agent_handle",
        ))?;
        let rows = db(query.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        }))?;
        db(rows.collect::<rusqlite::Result<Vec<_>>>())?
    };
    for (session, branch) in &agents {
        bind_auto(connection, session, branch, None)?;
    }
    Ok(agents.len())
}

pub(crate) fn preflight_initial_selection(
    connection: &Connection,
    actor: &PrincipalId,
    domain: &str,
    identity: &str,
    project: &str,
    binding: &SessionBinding,
    requested: Option<&str>,
) -> Result<()> {
    let Some(requested) = requested else {
        return Ok(());
    };
    let project = ProjectKey {
        domain: domain.to_owned(),
        identity: identity.to_owned(),
        project: project.to_owned(),
    };
    authorize_project(connection, actor, &project, false)?;
    let key = parse_sandbox_ref(requested)?;
    if key.project != project {
        return Err(Error::Denied(
            "Agent Sandbox must belong to the exact destination Project",
        ));
    }
    let sandbox = snapshot_row(connection, &key.canonical())?;
    if source_bindings::has_sources(connection, &sandbox.canonical_ref)? {
        return Err(Error::Denied(
            "source-bearing Agent Sandbox realization is unavailable",
        ));
    }
    let expected = layout(binding)?;
    if sandbox.lifecycle != SandboxLifecycle::Open
        || sandbox.layout_digest != expected.digest
        || sandbox.working_directory.to_str() != Some(expected.working_directory.as_str())
        || sandbox.tool_root.to_str() != Some(expected.tool_root.as_str())
    {
        return Err(Error::Conflict(
            "selected Sandbox is closed or has a different execution layout",
        ));
    }
    Ok(())
}

/// Constructor-only initial selection. This may replace the automatic layout
/// choice while the Agent row is still rev=1 inside its uncommitted create
/// transaction. There is deliberately no public rebind operation.
pub(crate) fn select_initial_agent_sandbox(
    connection: &Connection,
    actor: &PrincipalId,
    session: &str,
    branch: &str,
    requested: Option<&str>,
) -> Result<String> {
    let (project, expected, current, rev, lifecycle) =
        agent_project_and_layout(connection, session, branch)?;
    if rev != 1 || lifecycle != "open" {
        return Err(Error::Conflict(
            "initial Sandbox selection requires a newly created open Agent",
        ));
    }
    let target = if let Some(requested) = requested {
        preflight_initial_selection(
            connection,
            actor,
            &project.domain,
            &project.identity,
            &project.project,
            &session_in(connection, session)?.binding,
            Some(requested),
        )?;
        parse_sandbox_ref(requested)?.canonical()
    } else if let Some(current) = current {
        current
    } else {
        ensure_layout(connection, &project, &expected, Some(actor))?
    };
    let changed = db(connection.execute(
        "UPDATE agent_state_current SET sandbox_id=?1
         WHERE session_id=?2 AND branch_id=?3 AND rev=1 AND lifecycle='open'",
        params![target, session, branch],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict(
            "Agent initial Sandbox binding changed during creation",
        ));
    }
    Ok(target)
}

impl SessionStore {
    pub fn sandbox_get(&self, actor: &PrincipalId, canonical_ref: &str) -> Result<SandboxSnapshot> {
        let key = parse_sandbox_ref(canonical_ref)?;
        let connection = self.connection()?;
        authorize_project(&connection, actor, &key.project, false)?;
        snapshot_with_sources(&connection, &key.canonical())
    }

    pub fn sandbox_list(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SandboxSnapshot>> {
        if limit == 0 || limit > MAX_SANDBOX_LIST {
            return Err(Error::Limit("invalid Sandbox list limit"));
        }
        let project = parse_project_ref(project_ref)?;
        let connection = self.connection()?;
        authorize_project(&connection, actor, &project, false)?;
        let after_handle = if let Some(after) = after {
            let key = parse_sandbox_ref(after)?;
            if key.project != project {
                return Err(Error::Denied(
                    "Sandbox list cursor belongs to another Project",
                ));
            }
            Some(key.sandbox)
        } else {
            None
        };
        let mut query = db(connection.prepare(
            "SELECT sandbox_id FROM sandboxes_current
             WHERE domain_handle=?1 AND identity_handle=?2 AND project_handle=?3
               AND (?4 IS NULL OR sandbox_handle>?4)
             ORDER BY sandbox_handle LIMIT ?5",
        ))?;
        let rows = db(query.query_map(
            params![
                project.domain,
                project.identity,
                project.project,
                after_handle,
                limit as u64
            ],
            |row| row.get::<_, String>(0),
        ))?;
        let references = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        references
            .iter()
            .map(|reference| snapshot_with_sources(&connection, reference))
            .collect()
    }

    /// Create one fresh logical Sandbox for a trusted host-selected execution
    /// layout. Derivation/source copying is intentionally left to the later
    /// SourceDriver/checkpoint owner.
    pub fn sandbox_create_fresh(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        binding: &SessionBinding,
    ) -> Result<SandboxSnapshot> {
        self.sandbox_create_fresh_with_sources(actor, project_ref, binding, &[])
    }

    pub fn source_set_limit(&self) -> SourceSetLimit {
        self.inner.source_limit
    }

    /// Trusted host lookup in this Store's actual registered driver leaf.
    /// Resolving an implementation is not source/disclosure authority.
    pub fn source_driver<T: Send + Sync + 'static>(
        &self,
        handle: &aw_agent_core::SourceDriverHandle<T>,
    ) -> Result<std::sync::Arc<T>> {
        self.inner.source_drivers.reader()?.get(handle)
    }

    /// Trusted host publication of a fresh logical Sandbox with an already
    /// materialized SourceSet. The trusted caller supplies authorized,
    /// non-secret references. No driver effect, grant, capture or restore:
    /// effectful constructors must use the future durable Prepared path.
    pub fn sandbox_create_fresh_with_sources(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        binding: &SessionBinding,
        sources: &[SourceBinding],
    ) -> Result<SandboxSnapshot> {
        let mutation_set = crate::SourceMutationSet::from_bindings(sources)?;
        let project = parse_project_ref(project_ref)?;
        let canonical = format!("{}/s:{}", project.canonical(), id("sbx")?);
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        mutation_set.require_unleased_in(&tx)?;
        let fresh = publish_fresh_in(
            &tx,
            actor,
            &canonical,
            binding,
            sources,
            &self.inner.source_drivers,
            self.source_set_limit(),
        )?;
        let snapshot = snapshot_with_sources(&tx, &fresh.reference)?;
        db(tx.commit())?;
        Ok(snapshot)
    }

    pub fn sandbox_close(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
        expected_rev: Option<u64>,
    ) -> Result<SandboxSnapshot> {
        let key = parse_sandbox_ref(canonical_ref)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        authorize_project(&tx, actor, &key.project, true)?;
        let current = snapshot_row(&tx, &key.canonical())?;
        if expected_rev.is_some_and(|rev| rev != current.rev) {
            return Err(Error::Conflict("Sandbox revision changed"));
        }
        if current.lifecycle == SandboxLifecycle::Closed {
            db(tx.commit())?;
            return snapshot_with_sources(&connection, &key.canonical());
        }
        // Retain live dependencies until the source lifecycle/cleanup contract
        // is implemented; closing must not drop a required driver dependency.
        if source_bindings::has_sources(&tx, &current.canonical_ref)? {
            return Err(Error::Conflict(
                "source-bearing Sandbox close requires SourceDriver lifecycle support",
            ));
        }
        let live_agents: u64 = db(tx.query_row(
            "SELECT count(*) FROM agent_state_current
             WHERE sandbox_id=?1 AND lifecycle IN ('open','closing')",
            [key.canonical()],
            |row| row.get(0),
        ))?;
        if live_agents != 0 {
            return Err(Error::Conflict(
                "Sandbox close requires every bound Agent to be closed",
            ));
        }
        let changed = db(tx.execute(
            "UPDATE sandboxes_current
             SET lifecycle='closed',rev=rev+1,updated_at=?1
             WHERE sandbox_id=?2 AND lifecycle='open' AND rev<9223372036854775807",
            params![now(), key.canonical()],
        ))?;
        if changed != 1 {
            return Err(Error::Limit("Sandbox revision unavailable or exhausted"));
        }
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "sandbox.closed",
            "sandbox",
            &key.canonical(),
            Some(actor),
            serde_json::json!({"sandbox_ref":key.canonical(),"rev":current.rev + 1}),
        )?;
        db(tx.commit())?;
        snapshot_with_sources(&connection, &key.canonical())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_store_api::SessionBackend;
    use std::path::PathBuf;

    fn binding(path: &str) -> SessionBinding {
        SessionBinding {
            working_directory: PathBuf::from(path),
            tool_root: PathBuf::from(path),
            system_prompt: "system".into(),
            profile_hint: "fixture".into(),
            model_hint: "model".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    #[test]
    fn source_bindings_survive_migration_and_incompatible_restart() {
        use aw_agent_core::{
            CheckpointPolicy, DriverKey, SchemaSupport, SourceAccess, SourceDriverRegistry,
            SourceDriverSchemas, SourceName,
        };
        use SourceBindingCompatibility::{Compatible, RecoveryRequired};

        struct PrivateTemp(PathBuf);
        impl Drop for PrivateTemp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let temp = PrivateTemp(std::env::temp_dir().join(id("aw-source-test").unwrap()));
        let actor = PrincipalId::new("internal:fixture:source-binding").unwrap();
        let store = SessionStore::open(&temp.0).unwrap();
        let (root, branch) = store.create(&actor, "root", binding("/tmp")).unwrap();
        let root = store.agent_binding(&actor, &root.id, &branch.id).unwrap();
        let project = format!(
            "/d:{}/i:{}/p:{}",
            root.domain_handle, root.identity_handle, root.project_handle
        );
        let root_ref = format!("{project}/a:{}", root.agent_handle);
        let root_sandbox = store
            .agent_get(&actor, &root_ref)
            .unwrap()
            .sandbox_ref
            .unwrap();
        drop(store);

        // Exercise a real v54 disk restart, not merely the migration SQL in isolation.
        {
            let connection = Connection::open(temp.0.join("sessions.sqlite3")).unwrap();
            connection
                .execute_batch(
                    "DROP TABLE agent_sandbox_grants_current;
                 DROP INDEX construction_legacy_actor;
                 DROP INDEX construction_source_prepare_id;
                 DROP INDEX construction_receipt_replay;
                 DROP TABLE checkpoint_sources;
                 DROP TABLE checkpoints;
                 DROP TABLE sandbox_source_bindings_current;
                 UPDATE store_meta SET value='54' WHERE key='schema_epoch';
                 UPDATE schema_meta SET schema_epoch=54,activation_epoch=54 WHERE singleton=1;
                 UPDATE migration_lease SET current_epoch=54 WHERE singleton=1;
                 PRAGMA user_version=54;",
                )
                .unwrap();
        }
        let git = DriverKey::builtin("fixture-git").unwrap();
        let data = DriverKey::extension("fixture", "data").unwrap();
        let schemas = |from, current| SourceDriverSchemas {
            binding: SchemaSupport::new(from, current).unwrap(),
            anchor: SchemaSupport::new(1, 1).unwrap(),
            recovery: SchemaSupport::new(1, 1).unwrap(),
        };
        let registry = SourceDriverRegistry::new();
        registry.register(git.clone(), schemas(1, 1), ()).unwrap();
        registry.register(data.clone(), schemas(1, 1), ()).unwrap();
        let limit = SourceSetLimit::new(2).unwrap();
        let store = SessionStore::open_with_sources(&temp.0, registry, limit).unwrap();
        assert!(store
            .sandbox_get(&actor, &root_sandbox)
            .unwrap()
            .sources
            .is_empty());
        assert_eq!(
            store
                .connection()
                .unwrap()
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            58
        );

        let sources = [
            SourceBinding {
                name: SourceName::new("code").unwrap(),
                driver: git.clone(),
                binding_schema: 1,
                source_ref: "repo-1".into(),
                materialization: "/private/materialization-1".into(),
                access: SourceAccess::ReadWrite,
                checkpoint_policy: CheckpointPolicy::Required,
            },
            SourceBinding {
                name: SourceName::new("cache").unwrap(),
                driver: data.clone(),
                binding_schema: 1,
                source_ref: "dataset-1".into(),
                materialization: "object-1".into(),
                access: SourceAccess::ReadOnly,
                checkpoint_policy: CheckpointPolicy::Ephemeral,
            },
        ];
        let path = binding("/var/tmp");
        let create = |sources: &[SourceBinding]| {
            store.sandbox_create_fresh_with_sources(&actor, &project, &path, sources)
        };
        assert!(matches!(
            create(&[sources[0].clone(), sources[1].clone(), sources[0].clone()]),
            Err(Error::Limit(_))
        ));
        assert!(matches!(
            create(&[sources[0].clone(), sources[0].clone()]),
            Err(Error::Conflict(_))
        ));
        let mut invalid = sources[0].clone();
        invalid.name = serde_json::from_str("\"Invalid\"").unwrap();
        assert!(create(&[invalid]).is_err());
        let mut invalid = sources[0].clone();
        invalid.binding_schema = 2;
        assert!(matches!(create(&[invalid]), Err(Error::Denied(_))));
        assert_eq!(
            store
                .sandbox_list(&actor, &project, None, 16)
                .unwrap()
                .len(),
            1
        );

        let created = create(&sources).unwrap();
        assert_eq!(created.sources.len(), 2);
        assert!(created
            .sources
            .iter()
            .all(|source| source.compatibility == Compatible && source.binding_rev == 1));
        assert_eq!(created.sources[0].name.as_str(), "cache");
        assert_eq!(created.sources[0].access, SourceAccess::ReadOnly);
        assert_eq!(
            created.sources[0].checkpoint_policy,
            CheckpointPolicy::Ephemeral
        );
        assert!(!serde_json::to_string(&created)
            .unwrap()
            .contains("private/materialization"));
        assert!(matches!(
            preflight_initial_selection(
                &store.connection().unwrap(),
                &actor,
                &root.domain_handle,
                &root.identity_handle,
                &root.project_handle,
                &path,
                Some(&created.canonical_ref)
            ),
            Err(Error::Denied(_))
        ));
        assert!(matches!(
            store.create(&actor, "same layout", path.clone()),
            Err(Error::Denied(_))
        ));
        assert_eq!(
            store
                .sandbox_list(&actor, &project, None, 16)
                .unwrap()
                .len(),
            2
        );
        drop(store);

        // A present but incompatible driver and a missing driver are both
        // recovery-required. Their pins, source identities, revisions and layout stay intact.
        let registry = SourceDriverRegistry::new();
        registry.register(git.clone(), schemas(2, 2), ()).unwrap();
        let store = SessionStore::open_with_sources(&temp.0, registry, limit).unwrap();
        let degraded = store.sandbox_get(&actor, &created.canonical_ref).unwrap();
        let mut expected = created.clone();
        for source in &mut expected.sources {
            source.compatibility = RecoveryRequired;
        }
        assert_eq!(degraded, expected);
        {
            let connection = store.connection().unwrap();
            let persisted: (String, String, u32, String) = connection
                .query_row(
                    "SELECT source_ref,materialization,binding_schema,compatibility
                 FROM sandbox_source_bindings_current WHERE sandbox_id=?1 AND source_name='code'",
                    [&created.canonical_ref],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap();
            assert_eq!(
                persisted,
                (
                    sources[0].source_ref.clone(),
                    sources[0].materialization.clone(),
                    1,
                    "recovery_required".into()
                )
            );
        }
        assert!(matches!(
            store.sandbox_close(&actor, &created.canonical_ref, None),
            Err(Error::Conflict(_))
        ));
        assert!(matches!(
            preflight_initial_selection(
                &store.connection().unwrap(),
                &actor,
                &root.domain_handle,
                &root.identity_handle,
                &root.project_handle,
                &path,
                Some(&created.canonical_ref)
            ),
            Err(Error::Denied(_))
        ));
        assert_eq!(
            store.agent_get(&actor, &root_ref).unwrap().sandbox_ref,
            Some(root_sandbox)
        );
        let backup = temp.0.join("degraded.sqlite3");
        store.backup(&backup).unwrap();
        drop(store);

        // Different registration order/revisions, explicitly compatible binding schema.
        let registry = SourceDriverRegistry::new();
        registry.register(data, schemas(1, 1), ()).unwrap();
        registry.register(git, schemas(1, 2), ()).unwrap();
        let store = SessionStore::open_with_sources(
            &temp.0,
            registry.clone(),
            SourceSetLimit::new(1).unwrap(),
        )
        .unwrap();
        assert_eq!(
            store.sandbox_get(&actor, &created.canonical_ref).unwrap(),
            created
        );
        assert!(matches!(
            store.sandbox_create_fresh_with_sources(&actor, &project, &path, &sources),
            Err(Error::Limit(_))
        ));

        // Restoring a disk backup must also revalidate against this host's
        // registry before the restored database is published.
        store.restore_backup(&backup).unwrap();
        assert_eq!(
            store.sandbox_get(&actor, &created.canonical_ref).unwrap(),
            created
        );

        #[cfg(target_os = "linux")]
        let store = {
            drop(store);
            let direct =
                SessionStore::open_trusted_local_direct(&temp.0, Default::default()).unwrap();
            assert_eq!(
                direct.sandbox_get(&actor, &created.canonical_ref).unwrap(),
                expected
            );
            drop(direct);
            let owner = SessionStore::open_with_sources(&temp.0, registry.clone(), limit).unwrap();
            assert_eq!(
                owner.sandbox_get(&actor, &created.canonical_ref).unwrap(),
                created
            );
            owner
        };

        // The hard bound is checked before decoding more than 1024 rows,
        // even for externally damaged/overfilled durable state.
        {
            let connection = store.connection().unwrap();
            connection
                .execute(
                    "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<1023)
                 INSERT INTO sandbox_source_bindings_current
                 SELECT sandbox_id,'extra-'||x,driver_key,binding_schema,source_ref,materialization,
                        access,checkpoint_policy,binding_rev,compatibility
                 FROM sandbox_source_bindings_current,n WHERE sandbox_id=?1 AND source_name='code'",
                    [&created.canonical_ref],
                )
                .unwrap();
            assert!(matches!(
                snapshot_with_sources(&connection, &created.canonical_ref),
                Err(Error::Limit(_))
            ));
        }
        drop(store);
        assert!(matches!(
            SessionStore::open_with_sources(&temp.0, registry, limit),
            Err(Error::Limit(_))
        ));
    }

    #[test]
    fn v40_migration_preserves_layout_identity_and_sandbox_close_is_quiescent() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("internal:fixture:sandbox-v41").unwrap();
        let (first, first_branch) = store.create(&actor, "first", binding("/tmp")).unwrap();
        let (second, second_branch) = store.create(&actor, "second", binding("/tmp")).unwrap();
        let (third, third_branch) = store.create(&actor, "third", binding("/var/tmp")).unwrap();

        {
            let mut connection = store.connection().unwrap();
            connection
                .execute_batch(
                    "DROP TABLE sandboxes_current;
                     UPDATE agent_state_current SET sandbox_id=NULL;
                     UPDATE store_meta SET value='40' WHERE key='schema_epoch';
                     UPDATE schema_meta
                        SET schema_epoch=40,activation_epoch=40,migration_phase='ready'
                        WHERE singleton=1;
                     UPDATE migration_lease
                        SET holder_client_id=NULL,current_epoch=40,target_epoch=NULL,
                            lease_expires_at=NULL
                        WHERE singleton=1;
                     PRAGMA user_version=40;",
                )
                .unwrap();
            crate::migrate_sandbox_owner(&mut connection).unwrap();
            assert_eq!(
                connection
                    .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                    .unwrap(),
                41
            );
        }

        let refs = [
            (&first, &first_branch),
            (&second, &second_branch),
            (&third, &third_branch),
        ]
        .into_iter()
        .map(|(session, branch)| {
            let binding = store
                .agent_binding(&actor, &session.id, &branch.id)
                .unwrap();
            let canonical = format!(
                "/d:{}/i:{}/p:{}/a:{}",
                binding.domain_handle,
                binding.identity_handle,
                binding.project_handle,
                binding.agent_handle
            );
            store
                .agent_get(&actor, &canonical)
                .unwrap()
                .sandbox_ref
                .unwrap()
        })
        .collect::<Vec<_>>();
        assert_eq!(refs[0], refs[1], "same Project+layout shares one Sandbox");
        assert_ne!(
            refs[0], refs[2],
            "different mutable execution layouts must not collapse"
        );
        assert!(refs.iter().any(|reference| reference.ends_with("/s:0")));

        assert!(matches!(
            store.sandbox_close(&actor, &refs[0], None),
            Err(Error::Conflict(_))
        ));

        let first_binding = store
            .agent_binding(&actor, &first.id, &first_branch.id)
            .unwrap();
        let project_ref = format!(
            "/d:{}/i:{}/p:{}",
            first_binding.domain_handle,
            first_binding.identity_handle,
            first_binding.project_handle
        );
        let fresh = store
            .sandbox_create_fresh(&actor, &project_ref, &binding("/tmp"))
            .unwrap();
        let closed = store
            .sandbox_close(&actor, &fresh.canonical_ref, Some(fresh.rev))
            .unwrap();
        assert_eq!(closed.lifecycle, SandboxLifecycle::Closed);
        assert!(closed.rev > fresh.rev);
        let replay = store
            .sandbox_close(&actor, &fresh.canonical_ref, Some(closed.rev))
            .unwrap();
        assert_eq!(replay.rev, closed.rev);
    }
}
