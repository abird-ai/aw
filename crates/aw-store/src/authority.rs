use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainAuthoritySetInfo {
    pub domain_id: String,
    pub set_revision: u64,
    pub quorum: u64,
    pub members_sha256: String,
    pub current_term: u64,
    pub members: Vec<String>,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainAuthorityGrantBundle {
    pub authority_domain_id: String,
    pub set_revision: u64,
    pub quorum: u64,
    pub members_sha256: String,
    pub members: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainAuthorityPeerGrantInfo {
    pub id: String,
    pub authority_domain_id: String,
    pub accepted_from_node_id: String,
    pub set_revision: u64,
    pub members_sha256: String,
    pub quorum: u64,
    pub members: Vec<String>,
    pub status: String,
    pub accepted_by_user_id: String,
    pub accepted_at: i64,
    pub revoked_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainAuthorityElectionInfo {
    pub id: String,
    pub domain_id: String,
    pub set_revision: u64,
    pub members_sha256: String,
    pub quorum: u64,
    pub term: u64,
    pub candidate_node_id: String,
    pub requested_ttl_seconds: u64,
    pub state: String,
    pub vote_count: u64,
    pub created_at: i64,
    pub activated_at: Option<i64>,
    pub expires_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainAuthorityVoteInfo {
    pub election_id: String,
    pub voter_node_id: String,
    pub expires_at: i64,
    pub signature: Option<String>,
    pub received_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainAuthorityPeerVoteInfo {
    pub authority_domain_id: String,
    pub set_revision: u64,
    pub members_sha256: String,
    pub term: u64,
    pub election_id: String,
    pub voter_node_id: String,
    pub candidate_node_id: String,
    pub ttl_seconds: u64,
    pub expires_at: i64,
    pub created_at: i64,
}

fn strict_majority(member_count: usize, quorum: u64) -> Result<()> {
    if member_count == 0 || member_count > 32 {
        return Err(Error::Limit(
            "Domain AuthoritySet must contain 1..=32 members",
        ));
    }
    if quorum == 0 || quorum > member_count as u64 || quorum <= (member_count as u64 / 2) {
        return Err(Error::Config(
            "Domain authority quorum must be a strict majority of the AuthoritySet".into(),
        ));
    }
    Ok(())
}

fn normalized_members(values: &[String]) -> Result<Vec<String>> {
    let mut members = values.to_vec();
    members.sort();
    members.dedup();
    for node in &members {
        let legacy_placeholder = node.len() == 37;
        let cryptographic = node.len() == 69;
        if !(legacy_placeholder || cryptographic)
            || !node.starts_with("node_")
            || !node[5..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::Config("invalid AuthoritySet AwNode ID".into()));
        }
    }
    Ok(members)
}

fn authority_transcript(label: &[u8], fields: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    out.extend_from_slice(label);
    for field in fields {
        let len = u32::try_from(field.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(field);
    }
    out
}

pub fn domain_authority_vote_transcript(vote: &DomainAuthorityPeerVoteInfo) -> Vec<u8> {
    authority_transcript(
        b"aw-domain-authority-vote-v1\0",
        &[
            vote.authority_domain_id.as_bytes(),
            &vote.set_revision.to_be_bytes(),
            vote.members_sha256.as_bytes(),
            &vote.term.to_be_bytes(),
            vote.election_id.as_bytes(),
            vote.voter_node_id.as_bytes(),
            vote.candidate_node_id.as_bytes(),
            &vote.ttl_seconds.to_be_bytes(),
            &vote.expires_at.to_be_bytes(),
        ],
    )
}

fn decode_authority_public_key(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Protocol(
            "Domain authority voter public key is invalid",
        ));
    }
    let mut out = [0u8; 32];
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let pair = std::str::from_utf8(pair)
            .map_err(|_| Error::Protocol("Domain authority voter public key is invalid"))?;
        out[index] = u8::from_str_radix(pair, 16)
            .map_err(|_| Error::Protocol("Domain authority voter public key is invalid"))?;
    }
    Ok(out)
}

pub(crate) fn verify_domain_authority_vote_signature(
    node_id: &str,
    public_key: &str,
    vote: &DomainAuthorityPeerVoteInfo,
    signature: &str,
) -> Result<()> {
    let public = decode_authority_public_key(public_key)?;
    if format!("node_{:x}", Sha256::digest(public)) != node_id {
        return Err(Error::Protocol(
            "Domain authority voter Node ID does not match its public key",
        ));
    }
    let verifying = VerifyingKey::from_bytes(&public)
        .map_err(|_| Error::Protocol("invalid Domain authority Ed25519 public key"))?;
    let bytes = STANDARD
        .decode(signature)
        .map_err(|_| Error::Protocol("invalid base64 Domain authority vote signature"))?;
    let bytes: [u8; 64] = bytes
        .try_into()
        .map_err(|_| Error::Protocol("Domain authority vote signature must be 64 bytes"))?;
    let signature = Signature::from_bytes(&bytes);
    verifying
        .verify_strict(&domain_authority_vote_transcript(vote), &signature)
        .map_err(|_| Error::Denied("Domain authority vote signature verification failed"))
}

pub(crate) fn authority_set_hash(
    domain_id: &str,
    revision: u64,
    quorum: u64,
    members: &[String],
) -> Result<String> {
    let members = normalized_members(members)?;
    Ok(format!(
        "{:x}",
        Sha256::digest(
            json(&serde_json::json!({
                "domain_id":domain_id,
                "set_revision":revision,
                "quorum":quorum,
                "members":members
            }))?
            .as_bytes()
        )
    ))
}

fn authority_set_row(c: &Connection, domain_id: &str) -> Result<DomainAuthoritySetInfo> {
    let row: (u64, u64, String, u64, i64) = db(c
        .query_row(
            "SELECT set_revision,quorum,members_sha256,current_term,updated_at FROM domain_authority_sets WHERE domain_id=?1",
            [domain_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional())?
        .ok_or(Error::Storage("Domain AuthoritySet is missing"))?;
    let mut q = db(c.prepare(
        "SELECT node_id FROM domain_authority_members WHERE domain_id=?1 AND state='active' ORDER BY ordinal,node_id",
    ))?;
    let members = db(db(q.query_map([domain_id], |r| r.get::<_, String>(0)))?
        .collect::<rusqlite::Result<Vec<_>>>())?;
    Ok(DomainAuthoritySetInfo {
        domain_id: domain_id.into(),
        set_revision: row.0,
        quorum: row.1,
        members_sha256: row.2,
        current_term: row.3,
        members,
        updated_at: row.4,
    })
}

fn election_row(c: &Connection, election_id: &str) -> Result<DomainAuthorityElectionInfo> {
    let row: (String, u64, String, u64, u64, String, u64, String, i64, Option<i64>, Option<i64>) = db(c
        .query_row(
            "SELECT domain_id,set_revision,members_sha256,quorum,term,candidate_node_id,requested_ttl_seconds,state,created_at,activated_at,expires_at FROM domain_authority_elections WHERE election_id=?1",
            [election_id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?)),
        )
        .optional())?
        .ok_or(Error::Denied("Domain authority election unavailable"))?;
    let vote_count: u64 = db(c.query_row(
        "SELECT count(*) FROM domain_authority_votes WHERE election_id=?1 AND expires_at>?2",
        params![election_id, now()],
        |r| r.get(0),
    ))?;
    Ok(DomainAuthorityElectionInfo {
        id: election_id.into(),
        domain_id: row.0,
        set_revision: row.1,
        members_sha256: row.2,
        quorum: row.3,
        term: row.4,
        candidate_node_id: row.5,
        requested_ttl_seconds: row.6,
        state: row.7,
        vote_count,
        created_at: row.8,
        activated_at: row.9,
        expires_at: row.10,
    })
}

fn peer_grant_row(c: &Connection, grant_id: &str) -> Result<DomainAuthorityPeerGrantInfo> {
    let row: (String,String,u64,String,u64,String,String,String,i64,Option<i64>) = db(c
        .query_row(
            "SELECT authority_domain_id,accepted_from_node_id,set_revision,members_sha256,quorum,members_json,status,accepted_by_user_id,accepted_at,revoked_at FROM domain_authority_peer_grants WHERE grant_id=?1",
            [grant_id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?)),
        )
        .optional())?
        .ok_or(Error::Denied("Domain authority peer grant unavailable"))?;
    Ok(DomainAuthorityPeerGrantInfo {
        id: grant_id.into(),
        authority_domain_id: row.0,
        accepted_from_node_id: row.1,
        set_revision: row.2,
        members_sha256: row.3,
        quorum: row.4,
        members: decode(&row.5)?,
        status: row.6,
        accepted_by_user_id: row.7,
        accepted_at: row.8,
        revoked_at: row.9,
    })
}

pub(crate) fn initialize_domain_authority_if_available(
    c: &Connection,
    domain_id: &str,
) -> Result<()> {
    let available: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='domain_authority_sets')",
        [],
        |r| r.get(0),
    ))?;
    if available {
        initialize_domain_authority(c, domain_id)?;
    }
    Ok(())
}

pub(crate) fn initialize_domain_authority(c: &Connection, domain_id: &str) -> Result<()> {
    let local_node: String = db(c.query_row(
        "SELECT value FROM store_meta WHERE key='local_node_id'",
        [],
        |r| r.get(0),
    ))?;
    let exists: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM domain_authority_sets WHERE domain_id=?1)",
        [domain_id],
        |r| r.get(0),
    ))?;
    if exists {
        return Ok(());
    }
    let members = vec![local_node.clone()];
    let hash = authority_set_hash(domain_id, 1, 1, &members)?;
    let when = now();
    db(c.execute(
        "INSERT INTO domain_authority_sets(domain_id,set_revision,quorum,members_sha256,current_term,updated_at) VALUES(?1,1,1,?2,0,?3)",
        params![domain_id,hash,when],
    ))?;
    db(c.execute(
        "INSERT INTO domain_authority_members(domain_id,node_id,ordinal,state,joined_at) VALUES(?1,?2,0,'active',?3)",
        params![domain_id,local_node,when],
    ))?;
    Ok(())
}

pub(crate) fn backfill_domain_authority(c: &Connection) -> Result<()> {
    let mut q = db(c.prepare("SELECT domain_id FROM domains ORDER BY domain_id"))?;
    let ids =
        db(db(q.query_map([], |r| r.get::<_, String>(0)))?.collect::<rusqlite::Result<Vec<_>>>())?;
    for domain in ids {
        initialize_domain_authority(c, &domain)?;
    }
    Ok(())
}

pub(crate) fn replace_local_authority_node(
    c: &Connection,
    old_node: &str,
    new_node: &str,
) -> Result<()> {
    let mut q = db(c.prepare(
        "SELECT domain_id FROM domain_authority_members WHERE node_id=?1 AND state='active' ORDER BY domain_id",
    ))?;
    let domains = db(db(q.query_map([old_node], |r| r.get::<_, String>(0)))?
        .collect::<rusqlite::Result<Vec<_>>>())?;
    for domain_id in domains {
        db(c.execute(
            "UPDATE domain_authority_members SET node_id=?1 WHERE domain_id=?2 AND node_id=?3 AND state='active'",
            params![new_node,domain_id,old_node],
        ))?;
        let set = authority_set_row(c, &domain_id)?;
        let hash = authority_set_hash(&domain_id, set.set_revision, set.quorum, &set.members)?;
        db(c.execute(
            "UPDATE domain_authority_sets SET members_sha256=?1,updated_at=?2 WHERE domain_id=?3",
            params![hash, now(), domain_id],
        ))?;
    }
    Ok(())
}

pub(crate) fn require_domain_authority_write(c: &Connection, domain_id: &str) -> Result<()> {
    let set = authority_set_row(c, domain_id)?;
    let local_node: String = db(c.query_row(
        "SELECT value FROM store_meta WHERE key='local_node_id'",
        [],
        |r| r.get(0),
    ))?;
    if set.quorum == 1 && set.members.len() == 1 && set.members[0] == local_node {
        return Ok(());
    }
    let allowed: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM domain_authority_elections WHERE domain_id=?1 AND set_revision=?2 AND members_sha256=?3 AND term=?4 AND candidate_node_id=?5 AND state='active' AND expires_at>?6)",
        params![domain_id,set.set_revision,set.members_sha256,set.current_term,local_node,now()],
        |r| r.get(0),
    ))?;
    if allowed {
        Ok(())
    } else {
        Err(Error::Denied(
            "Domain authority quorum lease unavailable; authority-changing writes fail closed",
        ))
    }
}

fn require_domain_admin(c: &Connection, actor: &PrincipalId, domain_id: &str) -> Result<String> {
    let user = user_for_principal(c, actor)?.ok_or(Error::Denied(
        "Domain authority operation requires an active known user",
    ))?;
    let role: Option<String> = db(c.query_row(
        "SELECT role FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active'",
        params![domain_id,user], |r| r.get(0),
    ).optional())?;
    if !matches!(role.as_deref(), Some("owner" | "admin")) {
        return Err(Error::Denied(
            "Domain authority operation requires owner/admin authority",
        ));
    }
    Ok(user)
}

impl SessionStore {
    pub fn domain_authority_set(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
    ) -> Result<DomainAuthoritySetInfo> {
        let domain = match domain_selector {
            Some(s) => self.domain(actor, s)?,
            None => self.default_domain(actor)?,
        };
        let c = self.connection()?;
        let user = user_for_principal(&c, actor)?
            .ok_or(Error::Denied("Domain unavailable or not permitted"))?;
        let member: bool = db(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active')",
            params![domain.id,user], |r| r.get(0),
        ))?;
        if !member {
            return Err(Error::Denied("Domain unavailable or not permitted"));
        }
        authority_set_row(&c, &domain.id)
    }

    pub fn configure_domain_authority(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
        members: &[String],
        quorum: u64,
    ) -> Result<DomainAuthoritySetInfo> {
        let domain = match domain_selector {
            Some(s) => self.domain(actor, s)?,
            None => self.default_domain(actor)?,
        };
        let members = normalized_members(members)?;
        strict_majority(members.len(), quorum)?;
        if members.iter().any(|node| node.len() != 69) {
            return Err(Error::Denied(
                "AuthoritySet configuration requires cryptographic AwNode identities",
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let _user = require_domain_admin(&tx, actor, &domain.id)?;
        require_domain_authority_write(&tx, &domain.id)?;
        let current = authority_set_row(&tx, &domain.id)?;
        if current.members == members && current.quorum == quorum {
            return Ok(current);
        }
        let local_node: String = db(tx.query_row(
            "SELECT value FROM store_meta WHERE key='local_node_id'",
            [],
            |r| r.get(0),
        ))?;
        if !members.contains(&local_node) {
            return Err(Error::Denied(
                "current authority store's AwNode must remain in the AuthoritySet in Phase 26",
            ));
        }
        for node in &members {
            if node == &local_node {
                continue;
            }
            let valid: bool = db(tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM machines m JOIN awnet_peers p ON p.node_id=m.node_id WHERE m.node_id=?1 AND m.owning_domain_id=?2 AND m.kind='remote' AND m.status='active' AND p.trust_state='trusted')",
                params![node,domain.id], |r| r.get(0),
            ))?;
            if !valid {
                return Err(Error::Denied("AuthoritySet remote members must be trusted active remote Machines in the Domain"));
            }
        }
        let revision = current
            .set_revision
            .checked_add(1)
            .ok_or(Error::Limit("AuthoritySet revision exhausted"))?;
        let hash = authority_set_hash(&domain.id, revision, quorum, &members)?;
        let when = now();
        db(tx.execute("UPDATE domain_authority_members SET state='removed',removed_at=?1 WHERE domain_id=?2 AND state='active'",params![when,domain.id]))?;
        for (ordinal, node) in members.iter().enumerate() {
            db(tx.execute(
                "INSERT INTO domain_authority_members(domain_id,node_id,ordinal,state,joined_at,removed_at) VALUES(?1,?2,?3,'active',?4,NULL) ON CONFLICT(domain_id,node_id) DO UPDATE SET ordinal=excluded.ordinal,state='active',joined_at=excluded.joined_at,removed_at=NULL",
                params![domain.id,node,ordinal as u64,when],
            ))?;
        }
        db(tx.execute(
            "UPDATE domain_authority_sets SET set_revision=?1,quorum=?2,members_sha256=?3,current_term=0,updated_at=?4 WHERE domain_id=?5",
            params![revision,quorum,hash,when,domain.id],
        ))?;
        db(tx.execute("UPDATE domain_authority_elections SET state='superseded' WHERE domain_id=?1 AND state IN ('collecting','active')",[&domain.id]))?;
        db(tx.execute("UPDATE domains SET policy_revision=policy_revision+1,security_epoch=security_epoch+1,updated_at=?1 WHERE domain_id=?2",params![when,domain.id]))?;
        db(tx.execute("UPDATE federation_capability_leases SET status='revoked',revoked_at=?1 WHERE federation_id IN (SELECT federation_id FROM domain_federations WHERE local_domain_id=?2) AND status='active'",params![when,domain.id]))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "domain.authority_set_changed",
            "domain",
            &domain.id,
            Some(actor),
            serde_json::json!({
                "domain_id":domain.id,"set_revision":revision,"quorum":quorum,"members_sha256":hash,"members":members
            }),
        )?;
        db(tx.commit())?;
        authority_set_row(&c, &domain.id)
    }

    pub fn domain_authority_grant_bundle(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
    ) -> Result<DomainAuthorityGrantBundle> {
        let set = self.domain_authority_set(actor, domain_selector)?;
        let c = self.connection()?;
        let _ = require_domain_admin(&c, actor, &set.domain_id)?;
        Ok(DomainAuthorityGrantBundle {
            authority_domain_id: set.domain_id,
            set_revision: set.set_revision,
            quorum: set.quorum,
            members_sha256: set.members_sha256,
            members: set.members,
        })
    }

    pub fn accept_domain_authority_peer_grant(
        &self,
        actor: &PrincipalId,
        accepted_from_node_id: &str,
        bundle: &DomainAuthorityGrantBundle,
    ) -> Result<DomainAuthorityPeerGrantInfo> {
        let members = normalized_members(&bundle.members)?;
        strict_majority(members.len(), bundle.quorum)?;
        if authority_set_hash(
            &bundle.authority_domain_id,
            bundle.set_revision,
            bundle.quorum,
            &members,
        )? != bundle.members_sha256
        {
            return Err(Error::Conflict(
                "Domain authority grant bundle hash does not match its members",
            ));
        }
        if !members.iter().any(|n| n == accepted_from_node_id) {
            return Err(Error::Denied(
                "authority grant source is not a member of the AuthoritySet",
            ));
        }
        let peer = self.awnet_peer_by_node_id(accepted_from_node_id)?;
        if peer.trust_state != "trusted" {
            return Err(Error::Denied("authority grant source peer is not trusted"));
        }
        let local = self.local_machine()?;
        if !members.contains(&local.node_id) {
            return Err(Error::Denied(
                "this AwNode is not a member of the offered AuthoritySet",
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = ensure_user_for_principal(&tx, actor)?;
        let existing: Option<String> = db(tx.query_row(
            "SELECT grant_id FROM domain_authority_peer_grants WHERE authority_domain_id=?1 AND status='active' ORDER BY accepted_at DESC LIMIT 1",
            [&bundle.authority_domain_id], |r|r.get(0),
        ).optional())?;
        if let Some(id) = existing {
            let grant = peer_grant_row(&tx, &id)?;
            if grant.accepted_from_node_id == accepted_from_node_id
                && grant.set_revision == bundle.set_revision
                && grant.members_sha256 == bundle.members_sha256
            {
                return Ok(grant);
            }
            return Err(Error::Conflict("a different active authority grant already exists for this foreign Domain; revoke it explicitly first"));
        }
        let grant = id("agrant")?;
        let when = now();
        db(tx.execute(
            "INSERT INTO domain_authority_peer_grants(grant_id,authority_domain_id,accepted_from_node_id,set_revision,members_sha256,quorum,members_json,status,accepted_by_user_id,accepted_at) VALUES(?1,?2,?3,?4,?5,?6,?7,'active',?8,?9)",
            params![grant,bundle.authority_domain_id,accepted_from_node_id,bundle.set_revision,bundle.members_sha256,bundle.quorum,json(&members)?,user,when],
        ))?;
        db(tx.commit())?;
        peer_grant_row(&c, &grant)
    }

    pub fn domain_authority_peer_grants(
        &self,
        actor: &PrincipalId,
    ) -> Result<Vec<DomainAuthorityPeerGrantInfo>> {
        let c = self.connection()?;
        let user = user_for_principal(&c, actor)?.ok_or(Error::Denied(
            "authority grants unavailable or not permitted",
        ))?;
        let mut q=db(c.prepare("SELECT grant_id FROM domain_authority_peer_grants WHERE accepted_by_user_id=?1 ORDER BY accepted_at,grant_id LIMIT 128"))?;
        let ids = db(db(q.query_map([user], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter().map(|id| peer_grant_row(&c, id)).collect()
    }

    pub fn revoke_domain_authority_peer_grant(
        &self,
        actor: &PrincipalId,
        grant_id: &str,
    ) -> Result<DomainAuthorityPeerGrantInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let grant = peer_grant_row(&tx, grant_id)?;
        let user = ensure_user_for_principal(&tx, actor)?;
        if grant.accepted_by_user_id != user {
            return Err(Error::Denied(
                "only the local user that accepted an authority grant may revoke it",
            ));
        }
        if grant.status == "active" {
            db(tx.execute("UPDATE domain_authority_peer_grants SET status='revoked',revoked_at=?1 WHERE grant_id=?2 AND status='active'",params![now(),grant_id]))?;
        }
        db(tx.commit())?;
        peer_grant_row(&c, grant_id)
    }

    pub fn begin_domain_authority_election(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
        ttl_seconds: u64,
    ) -> Result<DomainAuthorityElectionInfo> {
        if !(5..=300).contains(&ttl_seconds) {
            return Err(Error::Config(
                "Domain authority lease TTL must be 5..=300 seconds".into(),
            ));
        }
        let domain = match domain_selector {
            Some(s) => self.domain(actor, s)?,
            None => self.default_domain(actor)?,
        };
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = require_domain_admin(&tx, actor, &domain.id)?;
        let set = authority_set_row(&tx, &domain.id)?;
        let local: String = db(tx.query_row(
            "SELECT value FROM store_meta WHERE key='local_node_id'",
            [],
            |r| r.get(0),
        ))?;
        if !set.members.contains(&local) {
            return Err(Error::Denied(
                "local AwNode is not an active Domain authority member",
            ));
        }
        let active_other:bool=db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM domain_authority_elections WHERE domain_id=?1 AND state='active' AND expires_at>?2 AND candidate_node_id<>?3)",params![domain.id,now(),local],|r|r.get(0)))?;
        if active_other {
            return Err(Error::Denied(
                "another Domain authority leader lease remains active",
            ));
        }
        let max_term: u64 = db(tx.query_row(
            "SELECT coalesce(max(term),0) FROM domain_authority_elections WHERE domain_id=?1",
            [&domain.id],
            |r| r.get(0),
        ))?;
        let term = max_term
            .max(set.current_term)
            .checked_add(1)
            .ok_or(Error::Limit("Domain authority term exhausted"))?;
        let election = id("aelect")?;
        let created = now();
        let ttl_micros = i64::try_from(
            ttl_seconds
                .checked_mul(1_000_000)
                .ok_or(Error::Limit("authority TTL overflow"))?,
        )
        .map_err(|_| Error::Limit("authority TTL overflow"))?;
        let expires = created
            .checked_add(ttl_micros)
            .ok_or(Error::Limit("authority expiry overflow"))?;
        db(tx.execute(
            "INSERT INTO domain_authority_elections(election_id,domain_id,set_revision,members_sha256,quorum,term,candidate_node_id,requested_ttl_seconds,state,created_by_user_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'collecting',?9,?10)",
            params![election,domain.id,set.set_revision,set.members_sha256,set.quorum,term,local,ttl_seconds,user,created],
        ))?;
        db(tx.execute("INSERT INTO domain_authority_votes(election_id,voter_node_id,expires_at,signature,received_at) VALUES(?1,?2,?3,NULL,?4)",params![election,local,expires,created]))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "domain.authority_election_started",
            "domain",
            &domain.id,
            Some(actor),
            serde_json::json!({"election_id":election,"term":term,"set_revision":set.set_revision,"candidate_node_id":local,"requested_ttl_seconds":ttl_seconds}),
        )?;
        db(tx.commit())?;
        election_row(&c, &election)
    }

    pub fn domain_authority_election(
        &self,
        actor: &PrincipalId,
        election_id: &str,
    ) -> Result<DomainAuthorityElectionInfo> {
        let c = self.connection()?;
        let election = election_row(&c, election_id)?;
        let _ = require_domain_admin(&c, actor, &election.domain_id)?;
        Ok(election)
    }

    pub fn record_domain_authority_vote(
        &self,
        actor: &PrincipalId,
        election_id: &str,
        voter_node_id: &str,
        expires_at: i64,
        signature: &str,
    ) -> Result<DomainAuthorityElectionInfo> {
        label(signature, 256)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let election = election_row(&tx, election_id)?;
        let _ = require_domain_admin(&tx, actor, &election.domain_id)?;
        if election.state != "collecting" {
            return Err(Error::Conflict(
                "Domain authority election is not collecting votes",
            ));
        }
        let set = authority_set_row(&tx, &election.domain_id)?;
        if set.set_revision != election.set_revision
            || set.members_sha256 != election.members_sha256
        {
            return Err(Error::Conflict(
                "AuthoritySet changed while election was collecting",
            ));
        }
        if !set.members.iter().any(|n| n == voter_node_id) {
            return Err(Error::Denied(
                "authority vote is from a node outside the active AuthoritySet",
            ));
        }
        let local: String = db(tx.query_row(
            "SELECT value FROM store_meta WHERE key='local_node_id'",
            [],
            |r| r.get(0),
        ))?;
        if voter_node_id == local {
            return Err(Error::Conflict(
                "local authority vote is created with the election",
            ));
        }
        if expires_at <= now() {
            return Err(Error::Denied("authority vote is already expired"));
        }
        let peer_public_key: Option<String> = db(tx
            .query_row(
                "SELECT public_key FROM awnet_peers WHERE node_id=?1 AND trust_state='trusted'",
                [voter_node_id],
                |r| r.get(0),
            )
            .optional())?;
        let Some(peer_public_key) = peer_public_key else {
            return Err(Error::Denied("authority voter peer is not trusted"));
        };
        let signed_vote = DomainAuthorityPeerVoteInfo {
            authority_domain_id: election.domain_id.clone(),
            set_revision: election.set_revision,
            members_sha256: election.members_sha256.clone(),
            term: election.term,
            election_id: election.id.clone(),
            voter_node_id: voter_node_id.into(),
            candidate_node_id: election.candidate_node_id.clone(),
            ttl_seconds: election.requested_ttl_seconds,
            expires_at,
            created_at: 0,
        };
        verify_domain_authority_vote_signature(
            voter_node_id,
            &peer_public_key,
            &signed_vote,
            signature,
        )?;
        let existing:Option<(i64,Option<String>)>=db(tx.query_row("SELECT expires_at,signature FROM domain_authority_votes WHERE election_id=?1 AND voter_node_id=?2",params![election_id,voter_node_id],|r|Ok((r.get(0)?,r.get(1)?))).optional())?;
        if let Some((old_exp, old_sig)) = existing {
            if old_exp == expires_at && old_sig.as_deref() == Some(signature) {
                return Ok(election);
            }
            return Err(Error::Conflict(
                "authority voter attempted to rebind an existing election vote",
            ));
        }
        db(tx.execute("INSERT INTO domain_authority_votes(election_id,voter_node_id,expires_at,signature,received_at) VALUES(?1,?2,?3,?4,?5)",params![election_id,voter_node_id,expires_at,signature,now()]))?;
        db(tx.commit())?;
        election_row(&c, election_id)
    }

    pub fn activate_domain_authority_election(
        &self,
        actor: &PrincipalId,
        election_id: &str,
    ) -> Result<DomainAuthorityElectionInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let election = election_row(&tx, election_id)?;
        let _ = require_domain_admin(&tx, actor, &election.domain_id)?;
        if election.state == "active" {
            drop(tx);
            return election_row(&c, election_id);
        }
        if election.state != "collecting" {
            return Err(Error::Conflict(
                "Domain authority election is not activatable",
            ));
        }
        let set = authority_set_row(&tx, &election.domain_id)?;
        if set.set_revision != election.set_revision
            || set.members_sha256 != election.members_sha256
            || set.quorum != election.quorum
        {
            return Err(Error::Conflict(
                "AuthoritySet changed while election was collecting",
            ));
        }
        let now_value = now();
        let votes = {
            let mut q=db(tx.prepare("SELECT voter_node_id,expires_at FROM domain_authority_votes WHERE election_id=?1 AND expires_at>?2 ORDER BY voter_node_id"))?;
            let rows = db(q.query_map(params![election_id, now_value], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            }))?;
            db(rows.collect::<rusqlite::Result<Vec<_>>>())?
        };
        let valid: Vec<_> = votes
            .into_iter()
            .filter(|(node, _)| set.members.contains(node))
            .collect();
        if valid.len() < set.quorum as usize {
            return Err(Error::Denied(
                "Domain authority quorum has not approved this leader lease",
            ));
        }
        let expires = valid
            .iter()
            .map(|(_, e)| *e)
            .min()
            .ok_or(Error::Denied("authority quorum has no live votes"))?;
        db(tx.execute("UPDATE domain_authority_elections SET state='superseded' WHERE domain_id=?1 AND state='active'",[&election.domain_id]))?;
        db(tx.execute("UPDATE domain_authority_elections SET state='active',activated_at=?1,expires_at=?2 WHERE election_id=?3 AND state='collecting'",params![now_value,expires,election_id]))?;
        db(tx.execute(
            "UPDATE domain_authority_sets SET current_term=?1,updated_at=?2 WHERE domain_id=?3",
            params![election.term, now_value, election.domain_id],
        ))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "domain.authority_lease_activated",
            "domain",
            &election.domain_id,
            Some(actor),
            serde_json::json!({"election_id":election_id,"term":election.term,"candidate_node_id":election.candidate_node_id,"expires_at":expires,"quorum":set.quorum}),
        )?;
        db(tx.commit())?;
        election_row(&c, election_id)
    }

    pub fn prepare_domain_authority_peer_vote(
        &self,
        authenticated_candidate_node: &str,
        authority_domain_id: &str,
        set_revision: u64,
        members_sha256: &str,
        quorum: u64,
        term: u64,
        election_id: &str,
        candidate_node_id: &str,
        ttl_seconds: u64,
    ) -> Result<DomainAuthorityPeerVoteInfo> {
        if authenticated_candidate_node != candidate_node_id {
            return Err(Error::Denied(
                "authority vote candidate is not the authenticated MachineLink peer",
            ));
        }
        if term == 0 || !(5..=300).contains(&ttl_seconds) {
            return Err(Error::Config(
                "invalid Domain authority vote term/TTL".into(),
            ));
        }
        label(election_id, 128)?;
        let peer = self.awnet_peer_by_node_id(authenticated_candidate_node)?;
        if peer.trust_state != "trusted" {
            return Err(Error::Denied(
                "authority vote candidate peer is not trusted",
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let local: String = db(tx.query_row(
            "SELECT value FROM store_meta WHERE key='local_node_id'",
            [],
            |r| r.get(0),
        ))?;
        let local_set: Option<DomainAuthoritySetInfo> =
            match authority_set_row(&tx, authority_domain_id) {
                Ok(set)
                    if set.set_revision == set_revision
                        && set.members_sha256 == members_sha256
                        && set.quorum == quorum =>
                {
                    Some(set)
                }
                Ok(_) | Err(_) => None,
            };
        let members = if let Some(set) = local_set {
            set.members
        } else {
            let grant_id:Option<String>=db(tx.query_row(
                "SELECT grant_id FROM domain_authority_peer_grants WHERE authority_domain_id=?1 AND set_revision=?2 AND members_sha256=?3 AND quorum=?4 AND status='active' ORDER BY accepted_at DESC LIMIT 1",
                params![authority_domain_id,set_revision,members_sha256,quorum],|r|r.get(0)).optional())?;
            let Some(grant_id) = grant_id else {
                return Err(Error::Denied(
                    "this AwNode has neither the canonical Domain AuthoritySet nor an explicitly accepted foreign AuthoritySet grant",
                ));
            };
            peer_grant_row(&tx, &grant_id)?.members
        };
        if !members.contains(&candidate_node_id.to_string()) {
            return Err(Error::Denied(
                "candidate is not in the accepted AuthoritySet",
            ));
        }
        if !members.contains(&local) {
            return Err(Error::Denied(
                "local voter is no longer in the accepted AuthoritySet",
            ));
        }
        let existing_same:Option<(String,String,u64,i64,i64)>=db(tx.query_row(
            "SELECT election_id,candidate_node_id,ttl_seconds,expires_at,created_at FROM domain_authority_peer_votes WHERE authority_domain_id=?1 AND set_revision=?2 AND term=?3 AND voter_node_id=?4",
            params![authority_domain_id,set_revision,term,local],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional())?;
        if let Some((old_election, old_candidate, old_ttl, old_exp, old_created)) = existing_same {
            if old_election == election_id
                && old_candidate == candidate_node_id
                && old_ttl == ttl_seconds
            {
                return Ok(DomainAuthorityPeerVoteInfo {
                    authority_domain_id: authority_domain_id.into(),
                    set_revision,
                    members_sha256: members_sha256.into(),
                    term,
                    election_id: election_id.into(),
                    voter_node_id: local,
                    candidate_node_id: candidate_node_id.into(),
                    ttl_seconds,
                    expires_at: old_exp,
                    created_at: old_created,
                });
            }
            return Err(Error::Conflict(
                "this authority voter already cast a different vote for the term",
            ));
        }
        let newer:u64=db(tx.query_row("SELECT coalesce(max(term),0) FROM domain_authority_peer_votes WHERE authority_domain_id=?1 AND set_revision=?2 AND voter_node_id=?3",params![authority_domain_id,set_revision,local],|r|r.get(0)))?;
        if term <= newer {
            return Err(Error::Denied("stale Domain authority election term"));
        }
        let conflicting:bool=db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM domain_authority_peer_votes WHERE authority_domain_id=?1 AND voter_node_id=?2 AND expires_at>?3 AND candidate_node_id<>?4)",params![authority_domain_id,local,now(),candidate_node_id],|r|r.get(0)))?;
        if conflicting {
            return Err(Error::Denied(
                "authority voter is fenced by an unexpired vote for another candidate",
            ));
        }
        let created = now();
        let ttl_micros = i64::try_from(ttl_seconds * 1_000_000)
            .map_err(|_| Error::Limit("authority vote TTL overflow"))?;
        let expires = created
            .checked_add(ttl_micros)
            .ok_or(Error::Limit("authority vote expiry overflow"))?;
        db(tx.execute(
            "INSERT INTO domain_authority_peer_votes(authority_domain_id,set_revision,members_sha256,term,election_id,voter_node_id,candidate_node_id,ttl_seconds,expires_at,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![authority_domain_id,set_revision,members_sha256,term,election_id,local,candidate_node_id,ttl_seconds,expires,created],
        ))?;
        db(tx.commit())?;
        Ok(DomainAuthorityPeerVoteInfo {
            authority_domain_id: authority_domain_id.into(),
            set_revision,
            members_sha256: members_sha256.into(),
            term,
            election_id: election_id.into(),
            voter_node_id: local,
            candidate_node_id: candidate_node_id.into(),
            ttl_seconds,
            expires_at: expires,
            created_at: created,
        })
    }
}
