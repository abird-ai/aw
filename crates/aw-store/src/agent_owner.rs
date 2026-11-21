//! Authoritative durable DL-200 Agent envelope owner.
//!
//! Schema v38 seeds this owner once under the existing exclusive migration
//! transaction. After activation, legacy Session/audit state remains Turn/history
//! and compatibility evidence but never defines Agent revision/lifecycle/lineage.

use crate::{
    db, now, session_participation::SessionParticipationAction, user_for_principal, Error,
    PrincipalId, Result,
};
use rusqlite::{params, Connection, OptionalExtension};

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

fn canonical_ref(connection: &Connection, agent: AgentKey<'_>) -> Result<String> {
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
    }
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

pub(crate) fn authorize_access(
    connection: &Connection,
    actor: &PrincipalId,
    agent: AgentKey<'_>,
    action: SessionParticipationAction,
) -> Result<AgentAccessEvidence> {
    validate(agent)?;
    let user = user_for_principal(connection, actor)?
        .ok_or(Error::Denied("Agent principal is inactive or unknown"))?;
    let (owner, acl_revision): (String, u64) = db(connection.query_row(
        "SELECT owner_handle,acl_rev FROM agent_state_current
         WHERE session_id=?1 AND branch_id=?2",
        params![agent.session, agent.branch],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    if user == owner {
        let security_epoch: u64 = db(connection.query_row(
            "SELECT security_epoch FROM users WHERE user_id=?1 AND status='active'",
            [&user],
            |row| row.get(0),
        ))?;
        return Ok(AgentAccessEvidence {
            grant_revision: security_epoch,
            acl_revision,
            owner: true,
        });
    }
    let row: Option<(i64, i64, i64, i64, i64, u64)> = db(connection
        .query_row(
            "SELECT can_view,can_contribute,can_submit,can_control,can_manage,grant_rev
             FROM agent_acl_current
             WHERE session_id=?1 AND branch_id=?2
               AND principal_kind='user' AND principal_handle=?3",
            params![agent.session, agent.branch, user],
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
        )
        .optional())?;
    let Some((view, contribute, submit, control, manage, grant_revision)) = row else {
        return Err(Error::Denied("Agent access is not granted"));
    };
    let allowed = match action {
        SessionParticipationAction::View => view != 0,
        SessionParticipationAction::Contribute => contribute != 0,
        SessionParticipationAction::Submit => submit != 0,
        SessionParticipationAction::Control => control != 0,
        SessionParticipationAction::Manage => manage != 0,
    };
    if !allowed {
        return Err(Error::Denied("Agent access action is not granted"));
    }
    Ok(AgentAccessEvidence {
        grant_revision,
        acl_revision,
        owner: false,
    })
}

pub(crate) fn key_for_ref(connection: &Connection, reference: &str) -> Result<(String, String)> {
    let candidate: (String, String) = db(connection.query_row(
        "SELECT session_id,branch_id FROM job_exposure_current
         WHERE kind='agent' AND generation='' AND state='published' AND canonical_ref=?1",
        [reference],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    let actual = canonical_ref(
        connection,
        AgentKey::new(candidate.0.as_str(), candidate.1.as_str()),
    )?;
    if actual != reference {
        return Err(Error::Denied("Agent ref is unavailable or not authorized"));
    }
    Ok(candidate)
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

pub(crate) fn lifecycle(connection: &Connection, agent: AgentKey<'_>) -> Result<(String, String)> {
    validate(agent)?;
    db(connection.query_row(
        "SELECT lifecycle,admission FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
        params![agent.session, agent.branch],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))
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
