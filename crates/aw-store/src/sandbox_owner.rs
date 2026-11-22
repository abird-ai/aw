//! DL-200 Phase-1 logical Sandbox owner.
//!
//! A Sandbox owns only durable Project-scoped mutable execution *identity and
//! layout* here. It does not own Process lifetime, SourceBindings, Vault
//! credentials, mount/network/device policy, or Agent ACLs.

use crate::{
    db, durability, id, now, session_in, user_for_principal, workspace_writer, Error, PrincipalId,
    Result, SessionBinding, SessionStore,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const MAX_SANDBOX_LIST: usize = 256;

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
        created_at: row.10,
        updated_at: row.11,
    })
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

fn ensure_layout(
    connection: &Connection,
    project: &ProjectKey,
    layout: &Layout,
    event_actor: Option<&PrincipalId>,
) -> Result<String> {
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
        snapshot_row(&connection, &key.canonical())
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
            .map(|reference| snapshot_row(&connection, reference))
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
        let project = parse_project_ref(project_ref)?;
        let layout = layout(binding)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        authorize_project(&tx, actor, &project, true)?;
        let handle = id("sbx")?;
        let canonical = insert_sandbox(&tx, &project, &handle, None, &layout, Some(actor))?;
        db(tx.commit())?;
        snapshot_row(&connection, &canonical)
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
            return snapshot_row(&connection, &key.canonical());
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
        snapshot_row(&connection, &key.canonical())
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
