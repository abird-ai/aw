//! Authoritative durable DL-200 Agent envelope owner.
//!
//! Schema v38 seeds this owner once under the existing exclusive migration
//! transaction. After activation, legacy Session/audit state remains Turn/history
//! and compatibility evidence but never defines Agent revision/lifecycle/lineage.

use crate::{
    db, now,
    session_participation::{SessionParticipationAction, SessionParticipationPermissions},
    user_for_principal, Error, PrincipalId, Result,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const MAX_ACCESS_DEPENDENCIES: usize = 512;
const VIEW: u8 = 1 << 0;
const CONTRIBUTE: u8 = 1 << 1;
const SUBMIT: u8 = 1 << 2;
const CONTROL: u8 = 1 << 3;
const DERIVE: u8 = 1 << 4;
const SHARE: u8 = 1 << 5;
const MANAGE: u8 = 1 << 6;
const ALL_PERMISSIONS: u8 = VIEW | CONTRIBUTE | SUBMIT | CONTROL | DERIVE | SHARE | MANAGE;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentPermission {
    View,
    Contribute,
    Submit,
    Control,
    Derive,
    Share,
    Manage,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentPermissions {
    pub view: bool,
    pub contribute: bool,
    pub submit: bool,
    pub control: bool,
    pub derive: bool,
    pub share: bool,
    pub manage: bool,
}

impl AgentPermissions {
    pub const ALL: Self = Self {
        view: true,
        contribute: true,
        submit: true,
        control: true,
        derive: true,
        share: true,
        manage: true,
    };

    pub const VIEW_ONLY: Self = Self {
        view: true,
        contribute: false,
        submit: false,
        control: false,
        derive: false,
        share: false,
        manage: false,
    };

    #[inline]
    pub const fn allows(self, permission: AgentPermission) -> bool {
        match permission {
            AgentPermission::View => self.view,
            AgentPermission::Contribute => self.contribute,
            AgentPermission::Submit => self.submit,
            AgentPermission::Control => self.control,
            AgentPermission::Derive => self.derive,
            AgentPermission::Share => self.share,
            AgentPermission::Manage => self.manage,
        }
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        !self.view
            && !self.contribute
            && !self.submit
            && !self.control
            && !self.derive
            && !self.share
            && !self.manage
    }

    pub(crate) fn validate(self) -> Result<Self> {
        if !self.view
            && (self.contribute
                || self.submit
                || self.control
                || self.derive
                || self.share
                || self.manage)
        {
            return Err(Error::Denied(
                "Agent permissions require explicit view authority",
            ));
        }
        Ok(self)
    }

    #[inline]
    const fn from_mask(mask: u8) -> Self {
        Self {
            view: mask & VIEW != 0,
            contribute: mask & CONTRIBUTE != 0,
            submit: mask & SUBMIT != 0,
            control: mask & CONTROL != 0,
            derive: mask & DERIVE != 0,
            share: mask & SHARE != 0,
            manage: mask & MANAGE != 0,
        }
    }
}

impl From<SessionParticipationPermissions> for AgentPermissions {
    fn from(value: SessionParticipationPermissions) -> Self {
        Self {
            view: value.view,
            contribute: value.contribute,
            submit: value.submit,
            control: value.control,
            derive: false,
            share: false,
            manage: value.manage,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentAccessEntry {
    pub principal_ref: String,
    pub principal_kind: String,
    pub permissions: AgentPermissions,
    pub grant_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentAccessSnapshot {
    pub owner_ref: String,
    pub entries: Vec<AgentAccessEntry>,
    pub rev: u64,
    pub acl_rev: u64,
    pub caller_permissions: AgentPermissions,
    pub access_generation: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct AgentKey<'a> {
    pub(crate) session: &'a str,
    pub(crate) branch: &'a str,
}

impl<'a> AgentKey<'a> {
    #[inline]
    pub(crate) const fn new(session: &'a str, branch: &'a str) -> Self {
        Self { session, branch }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentAccessEvidence {
    pub grant_revision: u64,
    pub acl_revision: u64,
    pub owner: bool,
    pub effective_permissions: AgentPermissions,
    pub access_generation: [u8; 32],
}

#[inline]
fn permission_bit(permission: AgentPermission) -> u8 {
    match permission {
        AgentPermission::View => VIEW,
        AgentPermission::Contribute => CONTRIBUTE,
        AgentPermission::Submit => SUBMIT,
        AgentPermission::Control => CONTROL,
        AgentPermission::Derive => DERIVE,
        AgentPermission::Share => SHARE,
        AgentPermission::Manage => MANAGE,
    }
}

#[inline]
fn hash_part(hash: &mut Sha256, value: &[u8]) {
    hash.update((value.len() as u64).to_be_bytes());
    hash.update(value);
}

fn validate(agent: AgentKey<'_>) -> Result<()> {
    if agent.session.is_empty() || agent.branch.is_empty() {
        return Err(Error::Denied("invalid exact Agent owner key"));
    }
    Ok(())
}

fn agent_handle(canonical_ref: &str) -> Result<&str> {
    let (_, handle) = canonical_ref.rsplit_once("/a:").ok_or(Error::Storage(
        "Agent exposure lacks canonical Agent handle",
    ))?;
    crate::validate_agent_handle_component(handle)?;
    Ok(handle)
}

pub(crate) fn canonical_ref(connection: &Connection, agent: AgentKey<'_>) -> Result<String> {
    validate(agent)?;
    db(connection.query_row(
        "SELECT '/d:' || domain_handle || '/i:' || identity_handle ||
                '/p:' || project_handle || '/a:' || agent_handle
         FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
        params![agent.session, agent.branch],
        |row| row.get(0),
    ))
}

fn seed_acl_for_agent(connection: &Connection, agent: AgentKey<'_>) -> Result<()> {
    db(connection.execute(
        "INSERT INTO agent_acl_current(
             session_id,branch_id,principal_kind,principal_handle,
             can_view,can_contribute,can_submit,can_control,can_derive,can_share,can_manage,
             grant_rev,granted_by_handle,updated_at
         )
         SELECT ?1,?2,'user',g.user_id,
                g.can_view,g.can_contribute,g.can_submit,g.can_control,0,0,g.can_manage,
                g.revision,g.granted_by_user_id,g.updated_at
         FROM session_participation_grants g
         JOIN agent_state_current a ON a.session_id=?1 AND a.branch_id=?2
         WHERE g.session_id=?1 AND g.status='active' AND g.user_id<>a.owner_handle
         ON CONFLICT(session_id,branch_id,principal_kind,principal_handle) DO NOTHING",
        params![agent.session, agent.branch],
    ))?;
    Ok(())
}

/// Seed all existing public Agents and their conservative direct ACL snapshots.
/// Called only from the exclusive v38 migration before owner activation.
pub(crate) fn seed_from_legacy(connection: &Connection) -> Result<usize> {
    let rows = {
        let mut query = db(connection.prepare(
            "SELECT e.canonical_ref,e.domain_handle,e.identity_handle,e.project_handle,
                    e.session_id,e.branch_id,s.owner_user_id,s.title,b.created_at,
                    COALESCE((SELECT MAX(a.seq)+1 FROM audit a WHERE a.session_id=e.session_id),1)
             FROM job_exposure_current e
             JOIN sessions s ON s.id=e.session_id
             JOIN branches b ON b.id=e.branch_id AND b.session_id=e.session_id
             WHERE e.kind='agent' AND e.generation='' AND e.state='published'
             ORDER BY b.created_at,e.session_id,e.branch_id",
        ))?;
        let rows = db(query.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, u64>(9)?,
            ))
        }))?;
        db(rows.collect::<rusqlite::Result<Vec<_>>>())?
    };

    for (canonical, domain, identity, project, session, branch, owner, title, created, rev) in &rows
    {
        let owner = owner.as_deref().ok_or(Error::Storage(
            "Agent migration requires stable Session owner identity",
        ))?;
        let handle = agent_handle(canonical)?;
        if owner != identity {
            return Err(Error::Storage(
                "Agent migration owner differs from frozen canonical identity",
            ));
        }
        db(connection.execute(
            "INSERT INTO agent_state_current(
                 domain_handle,identity_handle,project_handle,agent_handle,
                 session_id,branch_id,owner_kind,owner_handle,title,rev,acl_rev,
                 lifecycle,admission,created_at,updated_at
             ) VALUES(?1,?2,?3,?4,?5,?6,'user',?7,?8,?9,1,'open','open',?10,?10)",
            params![domain, identity, project, handle, session, branch, owner, title, rev, created],
        ))?;
        seed_acl_for_agent(connection, AgentKey::new(session, branch))?;
    }

    // Freeze trusted delegation lineage only onto the original child Agent.
    let relations = {
        let mut query = db(connection.prepare(
            "SELECT child_session_id,parent_session_id,parent_branch_id,state,
                    EXISTS(SELECT 1 FROM audit a
                           WHERE a.session_id=s.child_session_id
                             AND a.action='agent.close_requested')
             FROM subagent_sessions s ORDER BY created_at,id",
        ))?;
        let rows = db(query.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, bool>(4)?,
            ))
        }))?;
        db(rows.collect::<rusqlite::Result<Vec<_>>>())?
    };
    for (child_session, parent_session, parent_branch, state, close_requested) in relations {
        let child_branch: Option<String> = db(connection
            .query_row(
                "SELECT branch_id FROM agent_state_current
                 WHERE session_id=?1 ORDER BY created_at,branch_id LIMIT 1",
                [&child_session],
                |row| row.get(0),
            )
            .optional())?;
        let Some(child_branch) = child_branch else {
            return Err(Error::Storage(
                "delegated child migration lacks authoritative Agent row",
            ));
        };
        let parent = canonical_ref(connection, AgentKey::new(&parent_session, &parent_branch))?;
        let terminal = matches!(
            state.as_str(),
            "completed" | "failed" | "cancelled" | "interrupted"
        );
        let (lifecycle, admission) = if terminal {
            ("closed", "held")
        } else if close_requested {
            ("closing", "held")
        } else {
            ("open", "open")
        };
        let changed = db(connection.execute(
            "UPDATE agent_state_current
             SET parent_agent_ref=?1,lifecycle=?2,admission=?3
             WHERE session_id=?4 AND branch_id=?5 AND parent_agent_ref IS NULL",
            params![parent, lifecycle, admission, child_session, child_branch],
        ))?;
        if changed != 1 {
            return Err(Error::Storage(
                "delegated child migration lineage is ambiguous",
            ));
        }
    }
    Ok(rows.len())
}

/// Create one exact Agent owner row inside the caller's existing transaction.
/// Repeated creation is allowed only to attach immutable delegation lineage in
/// that same constructor transaction.
pub(crate) fn created(
    connection: &Connection,
    agent: AgentKey<'_>,
    parent: Option<AgentKey<'_>>,
) -> Result<()> {
    validate(agent)?;
    let parent_ref = parent
        .map(|key| canonical_ref(connection, key))
        .transpose()?;
    let row: (String, String, String, String, String, String, i64) = db(connection.query_row(
        "SELECT e.canonical_ref,e.domain_handle,e.identity_handle,e.project_handle,
                s.owner_user_id,s.title,b.created_at
         FROM job_exposure_current e
         JOIN sessions s ON s.id=e.session_id
         JOIN branches b ON b.id=e.branch_id AND b.session_id=e.session_id
         WHERE e.kind='agent' AND e.generation='' AND e.state='published'
           AND e.session_id=?1 AND e.branch_id=?2",
        params![agent.session, agent.branch],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        },
    ))?;
    let handle = agent_handle(&row.0)?;
    if row.2 != row.4 {
        return Err(Error::Storage(
            "new Agent owner differs from canonical identity",
        ));
    }
    let inserted = db(connection.execute(
        "INSERT INTO agent_state_current(
             domain_handle,identity_handle,project_handle,agent_handle,
             session_id,branch_id,owner_kind,owner_handle,title,rev,acl_rev,
             lifecycle,admission,parent_agent_ref,created_at,updated_at
         ) VALUES(?1,?2,?3,?4,?5,?6,'user',?7,?8,1,1,'open','open',?9,?10,?10)
         ON CONFLICT(session_id,branch_id) DO NOTHING",
        params![
            row.1,
            row.2,
            row.3,
            handle,
            agent.session,
            agent.branch,
            row.4,
            row.5,
            parent_ref,
            row.6
        ],
    ))?;
    if inserted == 1 {
        seed_acl_for_agent(connection, agent)?;
        crate::sandbox_owner::bind_auto_if_available(connection, agent.session, agent.branch)?;
        crate::agent_input_plan::ensure_for_agent_if_available(
            connection,
            agent.session,
            agent.branch,
        )?;
        return Ok(());
    }

    let existing: (String, String, String, String, String, Option<String>) =
        db(connection.query_row(
            "SELECT domain_handle,identity_handle,project_handle,agent_handle,owner_handle,parent_agent_ref
             FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
            params![agent.session, agent.branch],
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
    if existing.0 != row.1
        || existing.1 != row.2
        || existing.2 != row.3
        || existing.3 != handle
        || existing.4 != row.4
    {
        return Err(Error::Conflict(
            "Agent owner already exists with different canonical identity",
        ));
    }
    match (existing.5.as_deref(), parent_ref.as_deref()) {
        (None, None) => Ok(()),
        (Some(existing), Some(expected)) if existing == expected => Ok(()),
        (None, Some(parent_ref)) => {
            let changed = db(connection.execute(
                "UPDATE agent_state_current SET parent_agent_ref=?1
                 WHERE session_id=?2 AND branch_id=?3 AND parent_agent_ref IS NULL AND rev=1",
                params![parent_ref, agent.session, agent.branch],
            ))?;
            if changed == 1 {
                Ok(())
            } else {
                Err(Error::Conflict(
                    "Agent owner lineage changed during creation",
                ))
            }
        }
        _ => Err(Error::Conflict(
            "Agent owner already exists with different creation lineage",
        )),
    }?;
    crate::sandbox_owner::bind_auto_if_available(connection, agent.session, agent.branch)?;
    crate::agent_input_plan::ensure_for_agent_if_available(
        connection,
        agent.session,
        agent.branch,
    )?;
    Ok(())
}

/// Advance one exact Agent-owned revision in the caller's transaction.
pub(crate) fn changed(connection: &Connection, agent: AgentKey<'_>) -> Result<()> {
    validate(agent)?;
    let changed = db(connection.execute(
        "UPDATE agent_state_current SET rev=rev+1,updated_at=?1
         WHERE session_id=?2 AND branch_id=?3 AND rev<9223372036854775807",
        params![now(), agent.session, agent.branch],
    ))?;
    if changed != 1 {
        return Err(Error::Limit("Agent revision unavailable or exhausted"));
    }
    Ok(())
}

pub(crate) fn require_future_admission(connection: &Connection, agent: AgentKey<'_>) -> Result<()> {
    validate(agent)?;
    let (lifecycle, admission): (String, String) = db(connection.query_row(
        "SELECT lifecycle,admission FROM agent_state_current
         WHERE session_id=?1 AND branch_id=?2",
        params![agent.session, agent.branch],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    if lifecycle != "open" {
        return Err(Error::Denied("Agent is closing or closed"));
    }
    if admission != "open" {
        return Err(Error::Denied("Agent admission is held"));
    }
    Ok(())
}

pub(crate) fn request_close(connection: &Connection, agent: AgentKey<'_>) -> Result<bool> {
    validate(agent)?;
    let changed = db(connection.execute(
        "UPDATE agent_state_current
         SET lifecycle='closing',admission='held',rev=rev+1,updated_at=?1
         WHERE session_id=?2 AND branch_id=?3 AND lifecycle='open' AND rev<9223372036854775807",
        params![now(), agent.session, agent.branch],
    ))?;
    if changed == 1 {
        return Ok(true);
    }
    let lifecycle: String = db(connection.query_row(
        "SELECT lifecycle FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
        params![agent.session, agent.branch],
        |row| row.get(0),
    ))?;
    Ok(matches!(lifecycle.as_str(), "closing" | "closed"))
}

pub(crate) fn close(connection: &Connection, agent: AgentKey<'_>) -> Result<bool> {
    validate(agent)?;
    let changed = db(connection.execute(
        "UPDATE agent_state_current
         SET lifecycle='closed',admission='held',rev=rev+1,updated_at=?1
         WHERE session_id=?2 AND branch_id=?3 AND lifecycle<>'closed'
           AND rev<9223372036854775807",
        params![now(), agent.session, agent.branch],
    ))?;
    if changed == 1 {
        return Ok(true);
    }
    let lifecycle: String = db(connection.query_row(
        "SELECT lifecycle FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
        params![agent.session, agent.branch],
        |row| row.get(0),
    ))?;
    Ok(lifecycle == "closed")
}

pub(crate) fn revision(connection: &Connection, agent: AgentKey<'_>) -> Result<u64> {
    validate(agent)?;
    db(connection.query_row(
        "SELECT rev FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
        params![agent.session, agent.branch],
        |row| row.get(0),
    ))
}

pub(crate) fn authorize_permission(
    connection: &Connection,
    actor: &PrincipalId,
    agent: AgentKey<'_>,
    permission: AgentPermission,
) -> Result<AgentAccessEvidence> {
    validate(agent)?;
    let user = user_for_principal(connection, actor)?
        .ok_or(Error::Denied("Agent principal is inactive or unknown"))?;
    let user_security_epoch: u64 = db(connection.query_row(
        "SELECT security_epoch FROM users WHERE user_id=?1 AND status='active'",
        [&user],
        |row| row.get(0),
    ))?;
    let (domain, owner_kind, owner_handle, acl_revision): (String, String, String, u64) =
        db(connection.query_row(
            "SELECT domain_handle,owner_kind,owner_handle,acl_rev
             FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
            params![agent.session, agent.branch],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ))?;

    let mut hash = Sha256::new();
    hash.update(b"aw-agent-access-generation-v2\0");
    for part in [
        domain.as_bytes(),
        agent.session.as_bytes(),
        agent.branch.as_bytes(),
        user.as_bytes(),
        owner_kind.as_bytes(),
        owner_handle.as_bytes(),
    ] {
        hash_part(&mut hash, part);
    }
    hash.update(user_security_epoch.to_be_bytes());
    hash.update(acl_revision.to_be_bytes());

    if owner_kind == "user" && owner_handle == user {
        hash.update(b"owner:user\0");
        hash.update([ALL_PERMISSIONS]);
        return Ok(AgentAccessEvidence {
            grant_revision: user_security_epoch,
            acl_revision,
            owner: true,
            effective_permissions: AgentPermissions::ALL,
            access_generation: hash.finalize().into(),
        });
    }

    if owner_kind == "group" {
        let group_revision: Option<u64> = db(connection
            .query_row(
                "SELECT g.rev
                 FROM principal_groups g
                 JOIN domain_principals p
                   ON p.domain_id=g.domain_id AND p.principal_handle=g.principal_handle
                 JOIN principal_group_members m
                   ON m.domain_id=g.domain_id AND m.group_handle=g.principal_handle
                 WHERE g.domain_id=?1 AND g.principal_handle=?2
                   AND g.deleted_at IS NULL AND p.kind='group' AND p.status='active'
                   AND m.member_handle=?3 AND m.state='active'",
                params![domain, owner_handle, user],
                |row| row.get(0),
            )
            .optional())?;
        if let Some(group_revision) = group_revision {
            hash.update(b"owner:group\0");
            hash_part(&mut hash, owner_handle.as_bytes());
            hash.update(group_revision.to_be_bytes());
            hash.update([ALL_PERMISSIONS]);
            return Ok(AgentAccessEvidence {
                grant_revision: user_security_epoch.max(group_revision),
                acl_revision,
                owner: true,
                effective_permissions: AgentPermissions::ALL,
                access_generation: hash.finalize().into(),
            });
        }
    }

    let mut query = db(connection.prepare(
        "SELECT x.principal_kind,x.principal_handle,
                x.can_view,x.can_contribute,x.can_submit,x.can_control,x.can_derive,x.can_share,
                x.can_manage,x.grant_rev,
                CASE WHEN x.principal_kind='group' THEN g.rev ELSE NULL END
         FROM agent_acl_current x
         JOIN domain_principals p
           ON p.domain_id=?1 AND p.principal_handle=x.principal_handle AND p.status='active'
         LEFT JOIN principal_group_members m
           ON x.principal_kind='group' AND m.domain_id=?1
          AND m.group_handle=x.principal_handle AND m.member_handle=?2 AND m.state='active'
         LEFT JOIN principal_groups g
           ON g.domain_id=m.domain_id AND g.principal_handle=m.group_handle
          AND g.deleted_at IS NULL
         WHERE x.session_id=?3 AND x.branch_id=?4
           AND (
             (x.principal_kind='user' AND p.kind='user' AND p.user_id=?2
              AND x.principal_handle=?2)
             OR
             (x.principal_kind='group' AND p.kind='group'
              AND m.member_handle IS NOT NULL AND g.rev IS NOT NULL)
           )
         ORDER BY x.principal_kind,x.principal_handle
         LIMIT ?5",
    ))?;
    let mut rows = db(query.query(params![
        domain,
        user,
        agent.session,
        agent.branch,
        (MAX_ACCESS_DEPENDENCIES + 1) as i64
    ]))?;
    let mut permissions = 0u8;
    let mut dependency_revision = user_security_epoch;
    let mut dependencies = 0usize;
    while let Some(row) = db(rows.next())? {
        dependencies += 1;
        if dependencies > MAX_ACCESS_DEPENDENCIES {
            return Err(Error::Limit("Agent access dependency bound exceeded"));
        }
        let kind: String = row
            .get(0)
            .map_err(|_| Error::Storage("invalid Agent ACL kind"))?;
        let handle: String = row
            .get(1)
            .map_err(|_| Error::Storage("invalid Agent ACL principal"))?;
        let permission = |column, bit| -> Result<u8> {
            Ok(
                if row
                    .get::<_, i64>(column)
                    .map_err(|_| Error::Storage("invalid Agent ACL"))?
                    != 0
                {
                    bit
                } else {
                    0
                },
            )
        };
        let mask = permission(2, VIEW)?
            | permission(3, CONTRIBUTE)?
            | permission(4, SUBMIT)?
            | permission(5, CONTROL)?
            | permission(6, DERIVE)?
            | permission(7, SHARE)?
            | permission(8, MANAGE)?;
        let grant_revision: u64 = row
            .get(9)
            .map_err(|_| Error::Storage("invalid Agent ACL revision"))?;
        let group_revision: Option<u64> = row
            .get(10)
            .map_err(|_| Error::Storage("invalid group revision"))?;
        permissions |= mask;
        dependency_revision = dependency_revision.max(grant_revision);
        if let Some(group_revision) = group_revision {
            dependency_revision = dependency_revision.max(group_revision);
        }
        hash_part(&mut hash, kind.as_bytes());
        hash_part(&mut hash, handle.as_bytes());
        hash.update([mask]);
        hash.update(grant_revision.to_be_bytes());
        hash.update(group_revision.unwrap_or(0).to_be_bytes());
    }
    if permissions & permission_bit(permission) == 0 {
        return Err(Error::Denied("Agent permission is not granted"));
    }
    hash.update([permissions]);
    Ok(AgentAccessEvidence {
        grant_revision: dependency_revision,
        acl_revision,
        owner: false,
        effective_permissions: AgentPermissions::from_mask(permissions),
        access_generation: hash.finalize().into(),
    })
}

pub(crate) fn authorize_access(
    connection: &Connection,
    actor: &PrincipalId,
    agent: AgentKey<'_>,
    action: SessionParticipationAction,
) -> Result<AgentAccessEvidence> {
    let permission = match action {
        SessionParticipationAction::View => AgentPermission::View,
        SessionParticipationAction::Contribute => AgentPermission::Contribute,
        SessionParticipationAction::Submit => AgentPermission::Submit,
        SessionParticipationAction::Control => AgentPermission::Control,
        SessionParticipationAction::Manage => AgentPermission::Manage,
    };
    authorize_permission(connection, actor, agent, permission)
}

pub(crate) fn key_for_ref_optional(
    connection: &Connection,
    reference: &str,
) -> Result<Option<(String, String)>> {
    let candidate: Option<(String, String)> = db(connection
        .query_row(
            "SELECT session_id,branch_id FROM job_exposure_current
             WHERE kind='agent' AND generation='' AND state='published' AND canonical_ref=?1",
            [reference],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional())?;
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    let actual = canonical_ref(
        connection,
        AgentKey::new(candidate.0.as_str(), candidate.1.as_str()),
    )?;
    if actual != reference {
        return Err(Error::Storage(
            "Agent exposure canonical ref disagrees with authoritative owner state",
        ));
    }
    Ok(Some(candidate))
}

pub(crate) fn key_for_ref(connection: &Connection, reference: &str) -> Result<(String, String)> {
    key_for_ref_optional(connection, reference)?
        .ok_or(Error::Denied("Agent ref is unavailable or not authorized"))
}

pub(crate) fn changed_acl(
    connection: &Connection,
    agent: AgentKey<'_>,
    expected_rev: u64,
) -> Result<u64> {
    validate(agent)?;
    let changed = db(connection.execute(
        "UPDATE agent_state_current
         SET acl_rev=acl_rev+1,rev=rev+1,updated_at=?1
         WHERE session_id=?2 AND branch_id=?3 AND rev=?4
           AND rev<9223372036854775807 AND acl_rev<9223372036854775807",
        params![now(), agent.session, agent.branch, expected_rev],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict("stale Agent revision"));
    }
    expected_rev
        .checked_add(1)
        .ok_or(Error::Limit("Agent revision exhausted"))
}

pub(crate) fn acl_permissions(
    connection: &Connection,
    agent: AgentKey<'_>,
    principal_kind: &str,
    principal_handle: &str,
) -> Result<(AgentPermissions, Option<u64>)> {
    validate(agent)?;
    if !matches!(principal_kind, "user" | "group") {
        return Err(Error::Denied("invalid Agent ACL principal kind"));
    }
    let current: Option<(i64, i64, i64, i64, i64, i64, i64, u64)> = db(connection
        .query_row(
            "SELECT can_view,can_contribute,can_submit,can_control,can_derive,can_share,
                    can_manage,grant_rev
             FROM agent_acl_current
             WHERE session_id=?1 AND branch_id=?2
               AND principal_kind=?3 AND principal_handle=?4",
            params![
                agent.session,
                agent.branch,
                principal_kind,
                principal_handle
            ],
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
                ))
            },
        )
        .optional())?;
    Ok(current.map_or((AgentPermissions::default(), None), |row| {
        (
            AgentPermissions {
                view: row.0 != 0,
                contribute: row.1 != 0,
                submit: row.2 != 0,
                control: row.3 != 0,
                derive: row.4 != 0,
                share: row.5 != 0,
                manage: row.6 != 0,
            },
            Some(row.7),
        )
    }))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn initialize_acl(
    connection: &Connection,
    agent: AgentKey<'_>,
    principal_kind: &str,
    principal_handle: &str,
    permissions: AgentPermissions,
    granted_by_handle: &str,
) -> Result<()> {
    validate(agent)?;
    let permissions = permissions.validate()?;
    if permissions.is_empty() {
        return Ok(());
    }
    if !matches!(principal_kind, "user" | "group") {
        return Err(Error::Denied("invalid Agent ACL principal kind"));
    }
    let (rev, acl_rev): (u64, u64) = db(connection.query_row(
        "SELECT rev,acl_rev FROM agent_state_current
         WHERE session_id=?1 AND branch_id=?2",
        params![agent.session, agent.branch],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    if rev != 1 || acl_rev != 1 {
        return Err(Error::Conflict(
            "initial Agent access can only be installed during construction",
        ));
    }
    db(connection.execute(
        "INSERT INTO agent_acl_current(
             session_id,branch_id,principal_kind,principal_handle,
             can_view,can_contribute,can_submit,can_control,can_derive,can_share,can_manage,
             grant_rev,granted_by_handle,updated_at
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,1,?12,?13)",
        params![
            agent.session,
            agent.branch,
            principal_kind,
            principal_handle,
            i64::from(permissions.view),
            i64::from(permissions.contribute),
            i64::from(permissions.submit),
            i64::from(permissions.control),
            i64::from(permissions.derive),
            i64::from(permissions.share),
            i64::from(permissions.manage),
            granted_by_handle,
            now(),
        ],
    ))?;
    Ok(())
}

pub(crate) fn set_acl(
    connection: &Connection,
    agent: AgentKey<'_>,
    principal_kind: &str,
    principal_handle: &str,
    permissions: AgentPermissions,
    granted_by_handle: &str,
    expected_rev: u64,
) -> Result<(u64, u64, bool)> {
    let permissions = permissions.validate()?;
    if revision(connection, agent)? != expected_rev {
        return Err(Error::Conflict("stale Agent revision"));
    }
    let (before, current_grant) =
        acl_permissions(connection, agent, principal_kind, principal_handle)?;
    if before == permissions {
        return Ok((expected_rev, current_grant.unwrap_or(1), false));
    }
    let next_grant = current_grant
        .map_or(Some(1), |revision| revision.checked_add(1))
        .ok_or(Error::Limit("Agent access grant revision exhausted"))?;
    if permissions.is_empty() {
        db(connection.execute(
            "DELETE FROM agent_acl_current
             WHERE session_id=?1 AND branch_id=?2
               AND principal_kind=?3 AND principal_handle=?4",
            params![
                agent.session,
                agent.branch,
                principal_kind,
                principal_handle
            ],
        ))?;
    } else {
        db(connection.execute(
            "INSERT INTO agent_acl_current(
                 session_id,branch_id,principal_kind,principal_handle,
                 can_view,can_contribute,can_submit,can_control,can_derive,can_share,can_manage,
                 grant_rev,granted_by_handle,updated_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
             ON CONFLICT(session_id,branch_id,principal_kind,principal_handle) DO UPDATE SET
                 can_view=excluded.can_view,can_contribute=excluded.can_contribute,
                 can_submit=excluded.can_submit,can_control=excluded.can_control,
                 can_derive=excluded.can_derive,can_share=excluded.can_share,
                 can_manage=excluded.can_manage,grant_rev=excluded.grant_rev,
                 granted_by_handle=excluded.granted_by_handle,updated_at=excluded.updated_at",
            params![
                agent.session,
                agent.branch,
                principal_kind,
                principal_handle,
                i64::from(permissions.view),
                i64::from(permissions.contribute),
                i64::from(permissions.submit),
                i64::from(permissions.control),
                i64::from(permissions.derive),
                i64::from(permissions.share),
                i64::from(permissions.manage),
                next_grant,
                granted_by_handle,
                now(),
            ],
        ))?;
    }
    let next = changed_acl(connection, agent, expected_rev)?;
    Ok((next, next_grant, true))
}

pub(crate) fn access_snapshot(
    connection: &Connection,
    actor: &PrincipalId,
    agent: AgentKey<'_>,
) -> Result<AgentAccessSnapshot> {
    let caller = authorize_permission(connection, actor, agent, AgentPermission::Manage)?;
    let (domain, owner_handle, rev, acl_rev): (String, String, u64, u64) = db(connection
        .query_row(
            "SELECT domain_handle,owner_handle,rev,acl_rev FROM agent_state_current
             WHERE session_id=?1 AND branch_id=?2",
            params![agent.session, agent.branch],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ))?;
    let owner_ref = format!("/d:{domain}/i:{owner_handle}");
    let mut query = db(connection.prepare(
        "SELECT principal_kind,principal_handle,
                can_view,can_contribute,can_submit,can_control,can_derive,can_share,can_manage,
                grant_rev
         FROM agent_acl_current
         WHERE session_id=?1 AND branch_id=?2
         ORDER BY principal_kind,principal_handle LIMIT ?3",
    ))?;
    let rows = db(query.query_map(
        params![
            agent.session,
            agent.branch,
            (MAX_ACCESS_DEPENDENCIES + 1) as i64
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, u64>(9)?,
            ))
        },
    ))?;
    let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if rows.len() > MAX_ACCESS_DEPENDENCIES {
        return Err(Error::Limit("Agent ACL list bound exceeded"));
    }
    let entries = rows
        .into_iter()
        .map(|row| {
            let permissions = AgentPermissions {
                view: row.2 != 0,
                contribute: row.3 != 0,
                submit: row.4 != 0,
                control: row.5 != 0,
                derive: row.6 != 0,
                share: row.7 != 0,
                manage: row.8 != 0,
            }
            .validate()?;
            Ok(AgentAccessEntry {
                principal_ref: format!("/d:{domain}/i:{}", row.1),
                principal_kind: row.0,
                permissions,
                grant_revision: row.9,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(AgentAccessSnapshot {
        owner_ref,
        entries,
        rev,
        acl_rev,
        caller_permissions: caller.effective_permissions,
        access_generation: caller.access_generation,
    })
}

pub(crate) fn lifecycle(connection: &Connection, agent: AgentKey<'_>) -> Result<(String, String)> {
    validate(agent)?;
    db(connection.query_row(
        "SELECT lifecycle,admission FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
        params![agent.session, agent.branch],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))
}

pub(crate) fn set_admission(
    connection: &Connection,
    agent: AgentKey<'_>,
    expected_rev: Option<u64>,
    state: &str,
) -> Result<(u64, bool)> {
    validate(agent)?;
    if !matches!(state, "open" | "held") {
        return Err(Error::Denied("invalid Agent admission state"));
    }
    let (lifecycle, admission, rev): (String, String, u64) = db(connection.query_row(
        "SELECT lifecycle,admission,rev FROM agent_state_current
         WHERE session_id=?1 AND branch_id=?2",
        params![agent.session, agent.branch],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ))?;
    if expected_rev.is_some_and(|expected| expected != rev) {
        return Err(Error::Conflict("stale Agent revision"));
    }
    if lifecycle != "open" {
        return Err(Error::Conflict(
            "Agent admission cannot change after close begins",
        ));
    }
    if admission == state {
        return Ok((rev, false));
    }
    let changed = db(connection.execute(
        "UPDATE agent_state_current
         SET admission=?1,rev=rev+1,updated_at=?2
         WHERE session_id=?3 AND branch_id=?4 AND lifecycle='open' AND rev=?5
           AND rev<9223372036854775807",
        params![state, now(), agent.session, agent.branch, rev],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict("Agent admission changed concurrently"));
    }
    let next = rev
        .checked_add(1)
        .ok_or(Error::Limit("Agent revision exhausted"))?;
    Ok((next, true))
}

pub(crate) fn delegated_child_branch(
    connection: &Connection,
    child_session: &str,
    parent: AgentKey<'_>,
) -> Result<String> {
    let parent_ref = canonical_ref(connection, parent)?;
    let mut query = db(connection.prepare(
        "SELECT branch_id FROM agent_state_current
         WHERE session_id=?1 AND parent_agent_ref=?2
         ORDER BY created_at,branch_id LIMIT 2",
    ))?;
    let rows = db(query.query_map(params![child_session, parent_ref], |row| {
        row.get::<_, String>(0)
    }))?;
    let branches = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    match branches.as_slice() {
        [branch] => Ok(branch.clone()),
        [] => Err(Error::Storage(
            "delegated child lacks authoritative Agent lineage",
        )),
        _ => Err(Error::Storage(
            "delegated child has ambiguous authoritative Agent lineage",
        )),
    }
}

pub(crate) fn set_title(
    connection: &Connection,
    agent: AgentKey<'_>,
    expected_rev: u64,
    title: &str,
) -> Result<u64> {
    validate(agent)?;
    let changed = db(connection.execute(
        "UPDATE agent_state_current SET title=?1,rev=rev+1,updated_at=?2
         WHERE session_id=?3 AND branch_id=?4 AND rev=?5 AND rev<9223372036854775807",
        params![title, now(), agent.session, agent.branch, expected_rev],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict("stale Agent revision"));
    }
    expected_rev
        .checked_add(1)
        .ok_or(Error::Limit("Agent revision exhausted"))
}
