//! DL-200 Domain principal registry and simple user-set groups.
//!
//! Users remain the authentication/security owner. This module owns only the
//! Domain-scoped public principal namespace and reusable group membership.

use crate::{
    db, durability, ensure_user_for_principal, id, label, now, user_for_principal, Error,
    PrincipalId, Result, SessionStore,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MAX_GROUP_MEMBERS: usize = 512;
const MAX_GROUP_LIST: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainPrincipalInfo {
    pub reference: String,
    pub domain_handle: String,
    pub principal_handle: String,
    pub kind: String,
    pub status: String,
    pub display_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrincipalGroupInfo {
    pub reference: String,
    pub domain_handle: String,
    pub principal_handle: String,
    pub name: String,
    pub owner_user_handle: String,
    pub rev: u64,
    pub status: String,
    pub members: Vec<String>,
}

fn canonical_ref(domain: &str, handle: &str) -> String {
    format!("/d:{domain}/i:{handle}")
}

fn group_row(c: &Connection, domain: &str, handle: &str) -> Result<PrincipalGroupInfo> {
    let row: Option<(String, String, u64, String)> = db(c
        .query_row(
            "SELECT json_extract(p.display_json,'$.name'),g.owner_user_id,g.rev,p.status
             FROM domain_principals p
             JOIN principal_groups g
               ON g.domain_id=p.domain_id AND g.principal_handle=p.principal_handle
             WHERE p.domain_id=?1 AND p.principal_handle=?2 AND p.kind='group'",
            params![domain, handle],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional())?;
    let Some((name, owner_user_handle, rev, status)) = row else {
        return Err(Error::Denied("group principal unavailable"));
    };
    let mut query = db(c.prepare(
        "SELECT member_handle FROM principal_group_members
         WHERE domain_id=?1 AND group_handle=?2 AND state='active'
         ORDER BY member_handle LIMIT ?3",
    ))?;
    let rows = db(query.query_map(
        params![domain, handle, (MAX_GROUP_MEMBERS + 1) as i64],
        |row| row.get::<_, String>(0),
    ))?;
    let members = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if members.len() > MAX_GROUP_MEMBERS {
        return Err(Error::Limit("group membership exceeds supported bound"));
    }
    Ok(PrincipalGroupInfo {
        reference: canonical_ref(domain, handle),
        domain_handle: domain.to_owned(),
        principal_handle: handle.to_owned(),
        name,
        owner_user_handle,
        rev,
        status,
        members,
    })
}

fn actor_user(c: &Connection, actor: &PrincipalId) -> Result<String> {
    user_for_principal(c, actor)?.ok_or(Error::Denied(
        "group operation requires an active known user",
    ))
}

fn domain_admin(c: &Connection, actor: &PrincipalId, domain: &str) -> Result<String> {
    let user = actor_user(c, actor)?;
    let role: Option<String> = db(c
        .query_row(
            "SELECT role FROM domain_memberships
             WHERE domain_id=?1 AND user_id=?2 AND status='active'",
            params![domain, user],
            |row| row.get(0),
        )
        .optional())?;
    if !matches!(role.as_deref(), Some("owner" | "admin")) {
        return Err(Error::Denied(
            "group operation requires Domain owner/admin authority",
        ));
    }
    Ok(user)
}

fn can_manage_group(
    c: &Connection,
    actor: &PrincipalId,
    domain: &str,
    handle: &str,
) -> Result<String> {
    let user = actor_user(c, actor)?;
    let owner: String = db(c.query_row(
        "SELECT owner_user_id FROM principal_groups
         WHERE domain_id=?1 AND principal_handle=?2 AND deleted_at IS NULL",
        params![domain, handle],
        |row| row.get(0),
    ))?;
    if user == owner {
        return Ok(user);
    }
    let role: Option<String> = db(c
        .query_row(
            "SELECT role FROM domain_memberships
             WHERE domain_id=?1 AND user_id=?2 AND status='active'",
            params![domain, user],
            |row| row.get(0),
        )
        .optional())?;
    if matches!(role.as_deref(), Some("owner" | "admin")) {
        Ok(user)
    } else {
        Err(Error::Denied(
            "group mutation requires group owner or Domain owner/admin",
        ))
    }
}

pub(crate) fn resolve_principal_ref(
    c: &Connection,
    expected_domain: &str,
    reference: &str,
) -> Result<(String, String)> {
    let body = reference
        .strip_prefix("/d:")
        .ok_or(Error::Denied("invalid canonical principal ref"))?;
    let (domain, handle) = body
        .split_once("/i:")
        .ok_or(Error::Denied("invalid canonical principal ref"))?;
    if domain != expected_domain || domain.is_empty() || handle.is_empty() || handle.contains('/') {
        return Err(Error::Denied(
            "principal ref is outside the Agent Domain or malformed",
        ));
    }
    crate::validate_agent_handle_component(domain)?;
    crate::validate_agent_handle_component(handle)?;
    let mut kind: Option<String> = db(c
        .query_row(
            "SELECT kind FROM domain_principals
             WHERE domain_id=?1 AND principal_handle=?2 AND status='active'",
            params![domain, handle],
            |row| row.get(0),
        )
        .optional())?;
    if kind.is_none() {
        let active_user: bool = db(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE user_id=?1 AND status='active')",
            [handle],
            |row| row.get(0),
        ))?;
        if active_user {
            ensure_user_principal(c, domain, handle)?;
            kind = Some("user".to_owned());
        }
    }
    let kind = kind.ok_or(Error::Denied("principal unavailable"))?;
    if !matches!(kind.as_str(), "user" | "group") {
        return Err(Error::Storage("unknown stored principal kind"));
    }
    Ok((kind, handle.to_owned()))
}

pub(crate) fn ensure_user_principal(c: &Connection, domain: &str, user: &str) -> Result<()> {
    crate::validate_agent_handle_component(user)?;
    let exists: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM users WHERE user_id=?1)",
        [user],
        |row| row.get(0),
    ))?;
    if !exists {
        return Err(Error::Denied("user principal is unknown"));
    }
    let at = now();
    db(c.execute(
        "INSERT INTO domain_principals(
             domain_id,principal_handle,kind,user_id,status,display_json,created_at,updated_at
         ) VALUES(?1,?2,'user',?2,'active','{}',?3,?3)
         ON CONFLICT(domain_id,principal_handle) DO NOTHING",
        params![domain, user, at],
    ))?;
    let valid: bool = db(c.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM domain_principals
             WHERE domain_id=?1 AND principal_handle=?2
               AND kind='user' AND user_id=?2 AND status='active'
         )",
        params![domain, user],
        |row| row.get(0),
    ))?;
    if !valid {
        return Err(Error::Conflict(
            "Domain principal handle already belongs to another principal",
        ));
    }
    Ok(())
}

pub(crate) fn ensure_user_principal_for_agent(
    c: &Connection,
    session: &str,
    branch: &str,
    user: &str,
) -> Result<()> {
    let domain: String = db(c.query_row(
        "SELECT domain_handle FROM agent_state_current
         WHERE session_id=?1 AND branch_id=?2",
        params![session, branch],
        |row| row.get(0),
    ))?;
    ensure_user_principal(c, &domain, user)
}

pub(crate) fn backfill_user_principals(c: &Connection) -> Result<usize> {
    let rows = {
        let mut query = db(c.prepare(
            "SELECT domain_id,user_id FROM domain_memberships
             UNION
             SELECT domain_handle,owner_handle FROM agent_state_current WHERE owner_kind='user'
             UNION
             SELECT a.domain_handle,x.principal_handle
             FROM agent_acl_current x
             JOIN agent_state_current a
               ON a.session_id=x.session_id AND a.branch_id=x.branch_id
             WHERE x.principal_kind='user'
             ORDER BY 1,2",
        ))?;
        let rows = db(query.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        }))?;
        db(rows.collect::<rusqlite::Result<Vec<_>>>())?
    };
    for (domain, user) in &rows {
        ensure_user_principal(c, domain, user)?;
    }
    Ok(rows.len())
}

fn normalized_members(
    c: &Connection,
    domain: &str,
    members: &[PrincipalId],
) -> Result<BTreeSet<String>> {
    if members.len() > MAX_GROUP_MEMBERS {
        return Err(Error::Limit("group member bound exceeded"));
    }
    let mut out = BTreeSet::new();
    for member in members {
        let user = ensure_user_for_principal(c, member)?;
        ensure_user_principal(c, domain, &user)?;
        out.insert(user);
    }
    Ok(out)
}

fn replace_members(
    c: &Connection,
    domain: &str,
    handle: &str,
    members: &BTreeSet<String>,
    revision: u64,
    at: i64,
) -> Result<()> {
    db(c.execute(
        "UPDATE principal_group_members
         SET state='removed',revision=?1,updated_at=?2
         WHERE domain_id=?3 AND group_handle=?4 AND state='active'",
        params![revision, at, domain, handle],
    ))?;
    for member in members {
        let kind: Option<String> = db(c
            .query_row(
                "SELECT kind FROM domain_principals
                 WHERE domain_id=?1 AND principal_handle=?2 AND status='active'",
                params![domain, member],
                |row| row.get(0),
            )
            .optional())?;
        if kind.as_deref() != Some("user") {
            return Err(Error::Denied(
                "group members must resolve to user-kind principals",
            ));
        }
        db(c.execute(
            "INSERT INTO principal_group_members(
                 domain_id,group_handle,member_handle,state,revision,updated_at
             ) VALUES(?1,?2,?3,'active',?4,?5)
             ON CONFLICT(domain_id,group_handle,member_handle) DO UPDATE SET
                 state='active',revision=excluded.revision,updated_at=excluded.updated_at",
            params![domain, handle, member, revision, at],
        ))?;
    }
    Ok(())
}

fn current_reference_count(c: &Connection, domain: &str, handle: &str) -> Result<u64> {
    db(c.query_row(
        "SELECT
             (SELECT count(*) FROM agent_state_current
              WHERE domain_handle=?1 AND owner_kind='group' AND owner_handle=?2)
           + (SELECT count(*) FROM agent_acl_current x
              JOIN agent_state_current a
                ON a.session_id=x.session_id AND a.branch_id=x.branch_id
              WHERE a.domain_handle=?1 AND x.principal_kind='group'
                AND x.principal_handle=?2)",
        params![domain, handle],
        |row| row.get(0),
    ))
}

impl SessionStore {
    pub fn domain_principal(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
        handle: &str,
    ) -> Result<DomainPrincipalInfo> {
        crate::validate_agent_handle_component(handle)?;
        let domain = match domain_selector {
            Some(selector) => self.domain(actor, selector)?,
            None => self.default_domain(actor)?,
        };
        let connection = self.connection()?;
        let row: Option<(String, String, String)> = db(connection
            .query_row(
                "SELECT kind,status,CASE kind
                    WHEN 'group' THEN COALESCE(json_extract(display_json,'$.name'),principal_handle)
                    ELSE principal_handle END
                 FROM domain_principals
                 WHERE domain_id=?1 AND principal_handle=?2",
                params![domain.id, handle],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional())?;
        let Some((kind, status, display_name)) = row else {
            return Err(Error::Denied("principal unavailable"));
        };
        Ok(DomainPrincipalInfo {
            reference: canonical_ref(&domain.id, handle),
            domain_handle: domain.id,
            principal_handle: handle.to_owned(),
            kind,
            status,
            display_name,
        })
    }

    pub fn domain_principals(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
    ) -> Result<Vec<DomainPrincipalInfo>> {
        let domain = match domain_selector {
            Some(selector) => self.domain(actor, selector)?,
            None => self.default_domain(actor)?,
        };
        let connection = self.connection()?;
        let mut query = db(connection.prepare(
            "SELECT principal_handle,kind,status,CASE kind
                WHEN 'group' THEN COALESCE(json_extract(display_json,'$.name'),principal_handle)
                ELSE principal_handle END
             FROM domain_principals WHERE domain_id=?1 AND status='active'
             ORDER BY kind,principal_handle LIMIT ?2",
        ))?;
        let rows = db(
            query.query_map(params![domain.id, (MAX_GROUP_LIST + 1) as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            }),
        )?;
        let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        if rows.len() > MAX_GROUP_LIST {
            return Err(Error::Limit("principal list exceeds supported bound"));
        }
        Ok(rows
            .into_iter()
            .map(|(handle, kind, status, display_name)| DomainPrincipalInfo {
                reference: canonical_ref(&domain.id, &handle),
                domain_handle: domain.id.clone(),
                principal_handle: handle,
                kind,
                status,
                display_name,
            })
            .collect())
    }

    pub fn create_principal_group(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
        name: &str,
        members: &[PrincipalId],
    ) -> Result<PrincipalGroupInfo> {
        label(name, 128)?;
        let domain = match domain_selector {
            Some(selector) => self.domain(actor, selector)?,
            None => self.default_domain(actor)?,
        };
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let owner = domain_admin(&tx, actor, &domain.id)?;
        ensure_user_principal(&tx, &domain.id, &owner)?;
        let members = normalized_members(&tx, &domain.id, members)?;
        let handle = id("grp")?;
        crate::validate_agent_handle_component(&handle)?;
        let at = now();
        db(tx.execute(
            "INSERT INTO domain_principals(
                 domain_id,principal_handle,kind,user_id,status,display_json,created_at,updated_at
             ) VALUES(?1,?2,'group',NULL,'active',json_object('name',?3),?4,?4)",
            params![domain.id, handle, name, at],
        ))?;
        db(tx.execute(
            "INSERT INTO principal_groups(domain_id,principal_handle,owner_user_id,rev)
             VALUES(?1,?2,?3,1)",
            params![domain.id, handle, owner],
        ))?;
        replace_members(&tx, &domain.id, &handle, &members, 1, at)?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "principal.group.created",
            "principal",
            &canonical_ref(&domain.id, &handle),
            Some(actor),
            serde_json::json!({"domain":domain.id,"principal":handle,"rev":1}),
        )?;
        db(tx.commit())?;
        group_row(&connection, &domain.id, &handle)
    }

    pub fn principal_group(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
        handle: &str,
    ) -> Result<PrincipalGroupInfo> {
        let domain = match domain_selector {
            Some(selector) => self.domain(actor, selector)?,
            None => self.default_domain(actor)?,
        };
        crate::validate_agent_handle_component(handle)?;
        let connection = self.connection()?;
        can_manage_group(&connection, actor, &domain.id, handle)?;
        group_row(&connection, &domain.id, handle)
    }

    pub fn principal_groups(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
    ) -> Result<Vec<PrincipalGroupInfo>> {
        let domain = match domain_selector {
            Some(selector) => self.domain(actor, selector)?,
            None => self.default_domain(actor)?,
        };
        let connection = self.connection()?;
        let user = actor_user(&connection, actor)?;
        let admin: bool = db(connection.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM domain_memberships
                 WHERE domain_id=?1 AND user_id=?2 AND status='active'
                   AND role IN ('owner','admin')
             )",
            params![domain.id, user],
            |row| row.get(0),
        ))?;
        let mut query = db(connection.prepare(
            "SELECT g.principal_handle
             FROM principal_groups g
             JOIN domain_principals p
               ON p.domain_id=g.domain_id AND p.principal_handle=g.principal_handle
             WHERE g.domain_id=?1 AND p.status='active'
               AND (?2 OR g.owner_user_id=?3)
             ORDER BY json_extract(p.display_json,'$.name'),g.principal_handle
             LIMIT ?4",
        ))?;
        let rows = db(query.query_map(
            params![domain.id, admin, user, (MAX_GROUP_LIST + 1) as i64],
            |row| row.get::<_, String>(0),
        ))?;
        let handles = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        if handles.len() > MAX_GROUP_LIST {
            return Err(Error::Limit("group list exceeds supported bound"));
        }
        handles
            .iter()
            .map(|handle| group_row(&connection, &domain.id, handle))
            .collect()
    }

    pub fn edit_principal_group(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
        handle: &str,
        name: Option<&str>,
        members: Option<&[PrincipalId]>,
        expected_rev: u64,
    ) -> Result<PrincipalGroupInfo> {
        if let Some(name) = name {
            label(name, 128)?;
        }
        crate::validate_agent_handle_component(handle)?;
        let domain = match domain_selector {
            Some(selector) => self.domain(actor, selector)?,
            None => self.default_domain(actor)?,
        };
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        can_manage_group(&tx, actor, &domain.id, handle)?;
        let current = group_row(&tx, &domain.id, handle)?;
        if current.status != "active" || current.rev != expected_rev {
            return Err(Error::Conflict("stale or deleted group revision"));
        }
        let members = members
            .map(|members| normalized_members(&tx, &domain.id, members))
            .transpose()?;
        let same_name = name.is_none_or(|name| name == current.name);
        let same_members = members
            .as_ref()
            .is_none_or(|members| members.iter().eq(current.members.iter()));
        if same_name && same_members {
            db(tx.commit())?;
            return Ok(current);
        }
        let next = current
            .rev
            .checked_add(1)
            .ok_or(Error::Limit("group revision exhausted"))?;
        let at = now();
        if let Some(name) = name {
            db(tx.execute(
                "UPDATE domain_principals
                 SET display_json=json_object('name',?1),updated_at=?2
                 WHERE domain_id=?3 AND principal_handle=?4 AND kind='group' AND status='active'",
                params![name, at, domain.id, handle],
            ))?;
        }
        if let Some(members) = &members {
            replace_members(&tx, &domain.id, handle, members, next, at)?;
        }
        let changed = db(tx.execute(
            "UPDATE principal_groups SET rev=?1
             WHERE domain_id=?2 AND principal_handle=?3 AND rev=?4 AND deleted_at IS NULL",
            params![next, domain.id, handle, expected_rev],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("stale group revision"));
        }
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "principal.group.changed",
            "principal",
            &canonical_ref(&domain.id, handle),
            Some(actor),
            serde_json::json!({
                "domain":domain.id,"principal":handle,"rev":next,
                "name_changed":name.is_some(),"members_changed":members.is_some()
            }),
        )?;
        db(tx.commit())?;
        group_row(&connection, &domain.id, handle)
    }

    /// Compatibility adapter for the five legacy collaboration permissions.
    /// Canonical ACL mutation/disclosure semantics live in `agent_access_set`.
    pub fn set_agent_group_collaboration_access(
        &self,
        requestor: &PrincipalId,
        session: &str,
        branch: &str,
        group_handle: &str,
        permissions: crate::SessionParticipationPermissions,
        expected_rev: u64,
    ) -> Result<u64> {
        let permissions = permissions.validate()?;
        crate::validate_agent_handle_component(group_handle)?;
        let domain: String = {
            let connection = self.connection()?;
            let branch = crate::branch_in(&connection, session, branch)?;
            db(connection.query_row(
                "SELECT domain_handle FROM agent_state_current
                 WHERE session_id=?1 AND branch_id=?2",
                params![session, branch.id],
                |row| row.get(0),
            ))?
        };
        let principal_ref = canonical_ref(&domain, group_handle);
        self.agent_access_set(
            requestor,
            session,
            branch,
            &principal_ref,
            Some(permissions.into()),
            expected_rev,
        )
    }

    pub fn delete_principal_group(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
        handle: &str,
        expected_rev: u64,
    ) -> Result<u64> {
        crate::validate_agent_handle_component(handle)?;
        let domain = match domain_selector {
            Some(selector) => self.domain(actor, selector)?,
            None => self.default_domain(actor)?,
        };
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        can_manage_group(&tx, actor, &domain.id, handle)?;
        let current = group_row(&tx, &domain.id, handle)?;
        if current.status != "active" || current.rev != expected_rev {
            return Err(Error::Conflict("stale or deleted group revision"));
        }
        if current_reference_count(&tx, &domain.id, handle)? != 0 {
            return Err(Error::Conflict(
                "group principal still has current Agent ownership/access references",
            ));
        }
        let next = expected_rev
            .checked_add(1)
            .ok_or(Error::Limit("group revision exhausted"))?;
        let at = now();
        db(tx.execute(
            "UPDATE principal_group_members
             SET state='removed',revision=?1,updated_at=?2
             WHERE domain_id=?3 AND group_handle=?4 AND state='active'",
            params![next, at, domain.id, handle],
        ))?;
        let changed = db(tx.execute(
            "UPDATE principal_groups SET rev=?1,deleted_at=?2
             WHERE domain_id=?3 AND principal_handle=?4 AND rev=?5 AND deleted_at IS NULL",
            params![next, at, domain.id, handle, expected_rev],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("stale group revision"));
        }
        db(tx.execute(
            "UPDATE domain_principals SET status='deleted',updated_at=?1
             WHERE domain_id=?2 AND principal_handle=?3 AND kind='group' AND status='active'",
            params![at, domain.id, handle],
        ))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "principal.group.deleted",
            "principal",
            &canonical_ref(&domain.id, handle),
            Some(actor),
            serde_json::json!({"domain":domain.id,"principal":handle,"rev":next}),
        )?;
        db(tx.commit())?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionBinding;
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
    fn group_membership_is_revisioned_and_does_not_touch_agent_rev() {
        let store = SessionStore::in_memory().unwrap();
        let owner = PrincipalId::new("internal:fixture:group-owner").unwrap();
        let bob = PrincipalId::new("internal:fixture:group-bob").unwrap();
        let carol = PrincipalId::new("internal:fixture:group-carol").unwrap();
        let (session, branch) = store.create(&owner, "Agent", binding()).unwrap();
        let domain = store.default_domain(&owner).unwrap();
        let agent_rev = store
            .agent_public_revision(&owner, &session.id, &branch.id)
            .unwrap();

        let group = store
            .create_principal_group(
                &owner,
                Some(&domain.id),
                "Reviewers",
                std::slice::from_ref(&bob),
            )
            .unwrap();
        assert_eq!(group.rev, 1);
        assert_eq!(group.members.len(), 1);
        let principals = store.domain_principals(&owner, Some(&domain.id)).unwrap();
        assert!(principals.iter().any(|principal| {
            principal.principal_handle == group.principal_handle
                && principal.kind == "group"
                && principal.display_name == "Reviewers"
        }));
        assert!(principals.iter().any(|principal| {
            principal.kind == "user" && group.members.contains(&principal.principal_handle)
        }));

        let edited = store
            .edit_principal_group(
                &owner,
                Some(&domain.id),
                &group.principal_handle,
                Some("Runtime reviewers"),
                Some(&[bob.clone(), carol.clone()]),
                group.rev,
            )
            .unwrap();
        assert_eq!(edited.rev, 2);
        assert_eq!(edited.members.len(), 2);
        assert_eq!(
            store
                .agent_public_revision(&owner, &session.id, &branch.id)
                .unwrap(),
            agent_rev,
            "group membership cannot fan out Agent revision"
        );

        let deleted = store
            .delete_principal_group(
                &owner,
                Some(&domain.id),
                &group.principal_handle,
                edited.rev,
            )
            .unwrap();
        assert_eq!(deleted, 3);
        assert!(store
            .principal_group(&owner, Some(&domain.id), &group.principal_handle)
            .is_err());
    }

    #[test]
    fn v38_to_v39_backfills_domain_user_principals_without_changing_handles() {
        let store = SessionStore::in_memory().unwrap();
        let owner = PrincipalId::new("internal:fixture:v39-owner").unwrap();
        let member = PrincipalId::new("internal:fixture:v39-member").unwrap();
        let (session, branch) = store.create(&owner, "Agent", binding()).unwrap();
        let domain = store.default_domain(&owner).unwrap();
        let rev = store
            .agent_public_revision(&owner, &session.id, &branch.id)
            .unwrap();
        store
            .set_agent_collaboration_access(
                &owner,
                &session.id,
                &branch.id,
                &member,
                crate::SessionParticipationPermissions::VIEW_ONLY,
                rev,
            )
            .unwrap();
        let mut connection = store.connection().unwrap();
        let owner_user = user_for_principal(&connection, &owner).unwrap().unwrap();
        let member_user = user_for_principal(&connection, &member).unwrap().unwrap();
        connection
            .execute_batch(
                "DROP TRIGGER domain_membership_principal_collision;
                 DROP TRIGGER domain_membership_principal_register;
                 DROP TABLE principal_group_members;
                 DROP TABLE principal_groups;
                 DROP TABLE domain_principals;
                 UPDATE store_meta SET value='38' WHERE key='schema_epoch';
                 UPDATE schema_meta
                    SET schema_epoch=38,activation_epoch=38,migration_phase='ready'
                    WHERE singleton=1;
                 UPDATE migration_lease
                    SET holder_client_id=NULL,current_epoch=38,target_epoch=NULL,lease_expires_at=NULL
                    WHERE singleton=1;
                 PRAGMA user_version=38;",
            )
            .unwrap();

        crate::migrate_domain_principals(&mut connection).unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            39
        );
        for handle in [owner_user, member_user] {
            let kind: String = connection
                .query_row(
                    "SELECT kind FROM domain_principals
                     WHERE domain_id=?1 AND principal_handle=?2",
                    params![domain.id, handle],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(kind, "user");
        }
    }
}
