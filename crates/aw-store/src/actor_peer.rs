//! AW-P6-9C2: revocable, per-branch/per-site AwNet peer authorization for an
//! existing authenticated Actor owner. SQLite v33 append-only audit only.
//! A signed MachineLink proves a peer Node ID; THIS distinct owner's grant
//! binds that peer to a concrete Session/branch/site, writer epoch, security
//! epoch and narrow operation rights. Neither is sufficient alone.
use crate::{audit, branch_in, db, principal_is_session_owner, workspace_writer, SessionStore};
use aw_agent_core::{Error, PrincipalId, Result};
use rusqlite::{params, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    time::{SystemTime, UNIX_EPOCH},
};

const ACTION: &str = "actor.peer_grant";
const MAX_EVENTS: usize = 4096;
const MAX_ACTIVE: usize = 32;
const MAX_TTL: u64 = 3600;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorPeerPermission {
    Query,
    Inspect,
    Enqueue,
    Step,
    Control,
    NoticeRead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorPeerGrantState {
    Active,
    Revoked,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorPeerGrant {
    pub grant_id: String,
    pub session_id: String,
    pub branch_id: String,
    pub recipient_site: String,
    pub peer_node_id: String,
    pub peer_security_epoch: u64,
    pub peer_public_key: String,
    pub writer_epoch: u64,
    pub permissions: BTreeSet<ActorPeerPermission>,
    pub expires_at: u64,
    pub state: ActorPeerGrantState,
}
fn clock() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Storage("invalid clock for peer grant"))?
        .as_secs())
}
fn events(c: &rusqlite::Connection, session: &str) -> Result<Vec<ActorPeerGrant>> {
    let mut query = db(c.prepare(
        "SELECT data_json FROM audit WHERE session_id=?1 AND action=?2 ORDER BY seq LIMIT 4097",
    ))?;
    let rows = db(query.query_map(params![session, ACTION], |r| r.get::<_, String>(0)))?;
    let encoded = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if encoded.len() > MAX_EVENTS {
        return Err(Error::Limit("Actor peer grant history needs review"));
    }
    encoded
        .iter()
        .map(|json| crate::decode::<ActorPeerGrant>(json))
        .collect()
}
fn latest(all: &[ActorPeerGrant]) -> std::collections::BTreeMap<String, ActorPeerGrant> {
    let mut current = std::collections::BTreeMap::new();
    for entry in all {
        current.insert(entry.grant_id.clone(), entry.clone());
    }
    current
}
fn verify_writer(
    c: &rusqlite::Connection,
    who: &PrincipalId,
    session: &str,
    epoch: u64,
) -> Result<()> {
    if !principal_is_session_owner(c, session, who)? {
        return Err(Error::Denied(
            "only exact current Session owner may grant peer Actor access",
        ));
    }
    workspace_writer::verify_writer_epoch_for_session(c, session, epoch)?;
    Ok(())
}
fn trusted_peer(c: &rusqlite::Connection, node: &str) -> Result<(u64, String)> {
    let (epoch, public, trusted): (u64, String, String) = db(c.query_row(
        "SELECT security_epoch,public_key,trust_state FROM awnet_peers WHERE node_id=?1",
        [node],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ))
    .map_err(|_| Error::Denied("Actor peer not paired/trusted"))?;
    if trusted != "trusted" || public.is_empty() {
        return Err(Error::Denied("Actor peer trust revoked"));
    }
    Ok((epoch, public))
}
fn append(c: &rusqlite::Connection, owner: &PrincipalId, grant: &ActorPeerGrant) -> Result<()> {
    audit(
        c,
        &grant.session_id,
        owner,
        ACTION,
        serde_json::to_value(grant)
            .map_err(|_| Error::Storage("cannot serialize Actor peer grant"))?,
    )
}
fn label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-:.@".contains(&b))
}
impl SessionStore {
    /// Explicit owner-granted revocable lease. This DOES NOT trust arbitrary
    /// remote Session provenance, a QUIC self-report or model text; signed
    /// paired Node identity must be separately proved by MachineLink.
    pub fn grant_actor_peer(
        &self,
        owner: &PrincipalId,
        session: &str,
        branch: &str,
        site: &str,
        node: &str,
        writer_epoch: u64,
        permissions: BTreeSet<ActorPeerPermission>,
        ttl: u64,
    ) -> Result<ActorPeerGrant> {
        if !label(session)
            || !label(branch)
            || !label(site)
            || !label(node)
            || writer_epoch == 0
            || !(5..=MAX_TTL).contains(&ttl)
            || permissions.is_empty()
            || permissions.len() > 6
        {
            return Err(Error::Denied("invalid bounded Actor peer grant"));
        }
        let expires = clock()?
            .checked_add(ttl)
            .ok_or(Error::Limit("grant expiry overflow"))?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        verify_writer(&tx, owner, session, writer_epoch)?;
        branch_in(&tx, session, branch)?;
        let (peer_epoch, peer_public) = trusted_peer(&tx, node)?;
        let old = events(&tx, session)?;
        let current = latest(&old);
        if current
            .values()
            .filter(|g| {
                g.state == ActorPeerGrantState::Active && g.expires_at > clock().unwrap_or(u64::MAX)
            })
            .count()
            >= MAX_ACTIVE
        {
            return Err(Error::Limit("Actor peer grant count reached 32"));
        }
        if old.len() + MAX_ACTIVE + 2 > MAX_EVENTS {
            return Err(Error::Limit(
                "Actor peer grant ledger has no closure budget",
            ));
        }
        let grant = ActorPeerGrant {
            grant_id: crate::id("apg")?,
            session_id: session.into(),
            branch_id: branch.into(),
            recipient_site: site.into(),
            peer_node_id: node.into(),
            peer_security_epoch: peer_epoch,
            peer_public_key: peer_public,
            writer_epoch,
            permissions,
            expires_at: expires,
            state: ActorPeerGrantState::Active,
        };
        append(&tx, owner, &grant)?;
        db(tx.commit())?;
        Ok(grant)
    }
    pub fn revoke_actor_peer(
        &self,
        owner: &PrincipalId,
        session: &str,
        grant_id: &str,
        current_epoch: u64,
    ) -> Result<ActorPeerGrant> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        verify_writer(&tx, owner, session, current_epoch)?;
        let facts = events(&tx, session)?;
        let mut grant = latest(&facts)
            .remove(grant_id)
            .ok_or(Error::Denied("Actor peer grant not found"))?;
        if grant.state != ActorPeerGrantState::Active {
            return Err(Error::Conflict("Actor peer grant already revoked"));
        }
        if facts.len() >= MAX_EVENTS {
            return Err(Error::Limit("Actor peer grant audit full"));
        }
        grant.state = ActorPeerGrantState::Revoked;
        append(&tx, owner, &grant)?;
        db(tx.commit())?;
        Ok(grant)
    }
    pub fn list_actor_peers(
        &self,
        owner: &PrincipalId,
        session: &str,
    ) -> Result<Vec<ActorPeerGrant>> {
        let c = self.connection()?;
        if !principal_is_session_owner(&c, session, owner)? {
            return Err(Error::Denied(
                "only current Session owner may view Actor peer grants",
            ));
        }
        let facts = latest(&events(&c, session)?)
            .into_values()
            .collect::<Vec<_>>();
        if facts.len() > 128 {
            return Err(Error::Limit("Actor peer grants need paginated inspection"));
        }
        Ok(facts)
    }
    /// Authorization must be called for EVERY peer-originated operation and
    /// periodically while a billed provider might remain active. A handshake
    /// lease is NEVER an authorization cache: revoke, writer failover, peer
    /// security epoch change or expiry immediately denies later work.
    pub fn authorize_actor_peer(
        &self,
        owner: &PrincipalId,
        session: &str,
        branch: &str,
        site: &str,
        signed_peer_node: &str,
        writer_epoch: u64,
        peer_security_epoch: u64,
        permission: ActorPeerPermission,
    ) -> Result<ActorPeerGrant> {
        let c = self.connection()?;
        verify_writer(&c, owner, session, writer_epoch)?;
        branch_in(&c, session, branch)?;
        let (epoch, public) = trusted_peer(&c, signed_peer_node)?;
        if epoch != peer_security_epoch {
            return Err(Error::Denied(
                "Actor peer security epoch changed since handshake",
            ));
        }
        let now = clock()?;
        latest(&events(&c, session)?)
            .into_values()
            .find(|grant| {
                grant.state == ActorPeerGrantState::Active
                    && grant.session_id == session
                    && grant.branch_id == branch
                    && grant.recipient_site == site
                    && grant.peer_node_id == signed_peer_node
                    && grant.peer_security_epoch == epoch
                    && grant.peer_public_key == public
                    && grant.writer_epoch == writer_epoch
                    && grant.expires_at > now
                    && grant.permissions.contains(&permission)
            })
            .ok_or(Error::Denied(
                "Actor peer does not hold a current explicit scoped permission",
            ))
    }
}
