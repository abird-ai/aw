use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

const AUTHORITY_SNAPSHOT_VERSION: u16 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainAuthorityReplicaUser {
    pub user_id: String,
    pub realm_id: String,
    pub status: String,
    pub display: serde_json::Value,
    pub security_epoch: u64,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainAuthorityReplicaMembership {
    pub user_id: String,
    pub role: String,
    pub status: String,
    pub policy_revision: u64,
    pub joined_at: i64,
    pub revoked_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainAuthorityControlSnapshot {
    pub version: u16,
    pub authority_domain_id: String,
    pub source_store_id: String,
    pub source_node_id: String,
    pub parent_domain_id: Option<String>,
    pub domain_name: String,
    pub domain_policy_revision: u64,
    pub domain_security_epoch: u64,
    pub domain_created_at: i64,
    pub authority_set: DomainAuthoritySetInfo,
    pub users: Vec<DomainAuthorityReplicaUser>,
    pub memberships: Vec<DomainAuthorityReplicaMembership>,
    pub through_commit_seq: u64,
    pub leader_election_id: Option<String>,
    pub leader_term: u64,
    pub leader_expires_at: Option<i64>,
    pub created_at: i64,
    pub snapshot_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainAuthorityReplicaSnapshotInfo {
    pub id: String,
    pub snapshot: DomainAuthorityControlSnapshot,
    pub source_signature: String,
    pub imported_by_grant_id: String,
    pub imported_at: i64,
    pub head_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainAuthorityReplicaElectionInfo {
    pub id: String,
    pub authority_domain_id: String,
    pub snapshot_id: String,
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
pub struct DomainAuthorityPromotionInfo {
    pub id: String,
    pub authority_domain_id: String,
    pub snapshot_id: String,
    pub election_id: String,
    pub previous_authority_store_id: String,
    pub promoted_authority_store_id: String,
    pub promoted_node_id: String,
    pub promoted_term: u64,
    pub leader_expires_at: i64,
    pub status: String,
    pub promoted_at: i64,
}

fn snapshot_hash(snapshot: &DomainAuthorityControlSnapshot) -> Result<String> {
    let mut value = serde_json::to_value(snapshot)
        .map_err(|_| Error::Storage("cannot encode Domain authority snapshot"))?;
    let object = value
        .as_object_mut()
        .ok_or(Error::Storage("invalid Domain authority snapshot shape"))?;
    object.insert(
        "snapshot_sha256".into(),
        serde_json::Value::String(String::new()),
    );
    Ok(format!("{:x}", Sha256::digest(json(&value)?.as_bytes())))
}

fn authority_snapshot_transcript_fields(snapshot: &DomainAuthorityControlSnapshot) -> Vec<Vec<u8>> {
    vec![
        snapshot.authority_domain_id.as_bytes().to_vec(),
        snapshot.source_store_id.as_bytes().to_vec(),
        snapshot.source_node_id.as_bytes().to_vec(),
        snapshot.authority_set.set_revision.to_be_bytes().to_vec(),
        snapshot.authority_set.members_sha256.as_bytes().to_vec(),
        snapshot.authority_set.quorum.to_be_bytes().to_vec(),
        snapshot.authority_set.current_term.to_be_bytes().to_vec(),
        snapshot.through_commit_seq.to_be_bytes().to_vec(),
        snapshot.leader_term.to_be_bytes().to_vec(),
        snapshot
            .leader_expires_at
            .unwrap_or_default()
            .to_be_bytes()
            .to_vec(),
        snapshot.snapshot_sha256.as_bytes().to_vec(),
    ]
}

pub fn domain_authority_snapshot_transcript(snapshot: &DomainAuthorityControlSnapshot) -> Vec<u8> {
    let fields = authority_snapshot_transcript_fields(snapshot);
    let mut out = Vec::with_capacity(512);
    out.extend_from_slice(b"aw-domain-authority-snapshot-v1\0");
    for field in fields {
        let len = u32::try_from(field.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&field);
    }
    out
}

fn decode_public_key(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Protocol(
            "invalid Domain authority snapshot public key",
        ));
    }
    let mut out = [0u8; 32];
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let pair = std::str::from_utf8(pair)
            .map_err(|_| Error::Protocol("invalid Domain authority snapshot public key"))?;
        out[index] = u8::from_str_radix(pair, 16)
            .map_err(|_| Error::Protocol("invalid Domain authority snapshot public key"))?;
    }
    Ok(out)
}

fn verify_snapshot_signature(
    node_id: &str,
    public_key: &str,
    snapshot: &DomainAuthorityControlSnapshot,
    signature: &str,
) -> Result<()> {
    let public = decode_public_key(public_key)?;
    if format!("node_{:x}", Sha256::digest(public)) != node_id {
        return Err(Error::Protocol(
            "Domain authority snapshot Node ID does not match its public key",
        ));
    }
    let verifying = VerifyingKey::from_bytes(&public)
        .map_err(|_| Error::Protocol("invalid Domain authority snapshot Ed25519 key"))?;
    let bytes = STANDARD
        .decode(signature)
        .map_err(|_| Error::Protocol("invalid Domain authority snapshot signature"))?;
    let bytes: [u8; 64] = bytes
        .try_into()
        .map_err(|_| Error::Protocol("Domain authority snapshot signature must be 64 bytes"))?;
    verifying
        .verify_strict(
            &domain_authority_snapshot_transcript(snapshot),
            &Signature::from_bytes(&bytes),
        )
        .map_err(|_| Error::Denied("Domain authority snapshot signature verification failed"))
}

fn replica_snapshot_row(
    c: &Connection,
    snapshot_id: &str,
) -> Result<DomainAuthorityReplicaSnapshotInfo> {
    let row: (String, String, String, String, i64, String) = db(c
        .query_row(
            "SELECT body_json,source_signature,imported_by_grant_id,authority_domain_id,imported_at,(SELECT status FROM domain_authority_replica_heads h WHERE h.authority_domain_id=s.authority_domain_id AND h.snapshot_id=s.snapshot_id) FROM domain_authority_replica_snapshots s WHERE snapshot_id=?1",
            [snapshot_id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get::<_,Option<String>>(5)?.unwrap_or_else(|| "historical".into()))),
        )
        .optional())?
        .ok_or(Error::Denied("Domain authority replica snapshot unavailable"))?;
    Ok(DomainAuthorityReplicaSnapshotInfo {
        id: snapshot_id.into(),
        snapshot: decode(&row.0)?,
        source_signature: row.1,
        imported_by_grant_id: row.2,
        imported_at: row.4,
        head_status: row.5,
    })
}

fn replica_head_row(c: &Connection, domain_id: &str) -> Result<DomainAuthorityReplicaSnapshotInfo> {
    let snapshot_id: String = db(c
        .query_row(
            "SELECT snapshot_id FROM domain_authority_replica_heads WHERE authority_domain_id=?1 AND status IN ('standby','promoted')",
            [domain_id],
            |r| r.get(0),
        )
        .optional())?
        .ok_or(Error::Denied("Domain authority standby replica unavailable"))?;
    replica_snapshot_row(c, &snapshot_id)
}

fn replica_election_row(
    c: &Connection,
    election_id: &str,
) -> Result<DomainAuthorityReplicaElectionInfo> {
    let row: (String,String,u64,String,u64,u64,String,u64,String,i64,Option<i64>,Option<i64>) = db(c
        .query_row(
            "SELECT authority_domain_id,snapshot_id,set_revision,members_sha256,quorum,term,candidate_node_id,requested_ttl_seconds,state,created_at,activated_at,expires_at FROM domain_authority_replica_elections WHERE election_id=?1",
            [election_id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?,r.get(11)?)),
        )
        .optional())?
        .ok_or(Error::Denied("Domain authority replica election unavailable"))?;
    let vote_count: u64 = db(c.query_row(
        "SELECT count(*) FROM domain_authority_replica_votes WHERE election_id=?1 AND expires_at>?2",
        params![election_id,now()],
        |r| r.get(0),
    ))?;
    Ok(DomainAuthorityReplicaElectionInfo {
        id: election_id.into(),
        authority_domain_id: row.0,
        snapshot_id: row.1,
        set_revision: row.2,
        members_sha256: row.3,
        quorum: row.4,
        term: row.5,
        candidate_node_id: row.6,
        requested_ttl_seconds: row.7,
        state: row.8,
        vote_count,
        created_at: row.9,
        activated_at: row.10,
        expires_at: row.11,
    })
}

fn promotion_row(c: &Connection, promotion_id: &str) -> Result<DomainAuthorityPromotionInfo> {
    db(c.query_row(
        "SELECT promotion_id,authority_domain_id,snapshot_id,election_id,previous_authority_store_id,promoted_authority_store_id,promoted_node_id,promoted_term,leader_expires_at,status,promoted_at FROM domain_authority_promotions WHERE promotion_id=?1",
        [promotion_id],
        |r| Ok(DomainAuthorityPromotionInfo{id:r.get(0)?,authority_domain_id:r.get(1)?,snapshot_id:r.get(2)?,election_id:r.get(3)?,previous_authority_store_id:r.get(4)?,promoted_authority_store_id:r.get(5)?,promoted_node_id:r.get(6)?,promoted_term:r.get(7)?,leader_expires_at:r.get(8)?,status:r.get(9)?,promoted_at:r.get(10)?}),
    ))
}

fn accepted_replica_grant(
    c: &Connection,
    actor: &PrincipalId,
    domain_id: &str,
    set_revision: u64,
    members_sha256: &str,
    quorum: u64,
) -> Result<(String, String)> {
    let user = user_for_principal(c, actor)?.ok_or(Error::Denied(
        "current local user has not accepted this Domain AuthoritySet standby grant",
    ))?;
    let grant: Option<String> = db(c
        .query_row(
            "SELECT grant_id FROM domain_authority_peer_grants WHERE authority_domain_id=?1 AND set_revision=?2 AND members_sha256=?3 AND quorum=?4 AND status='active' AND accepted_by_user_id=?5 ORDER BY accepted_at DESC LIMIT 1",
            params![domain_id,set_revision,members_sha256,quorum,user],
            |r| r.get(0),
        )
        .optional())?;
    grant.map(|id| (id, user)).ok_or(Error::Denied(
        "current local user has not accepted this Domain AuthoritySet standby grant",
    ))
}

impl SessionStore {
    pub fn export_domain_authority_snapshot(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
    ) -> Result<DomainAuthorityControlSnapshot> {
        let domain = match domain_selector {
            Some(selector) => self.domain(actor, selector)?,
            None => self.default_domain(actor)?,
        };
        let set = self.domain_authority_set(actor, Some(&domain.id))?;
        let c = self.connection()?;
        let user = user_for_principal(&c, actor)?.ok_or(Error::Denied(
            "Domain authority snapshot export requires an active known user",
        ))?;
        let role: Option<String> = db(c
            .query_row(
                "SELECT role FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active'",
                params![domain.id,user],
                |r| r.get(0),
            )
            .optional())?;
        if !matches!(role.as_deref(), Some("owner" | "admin")) {
            return Err(Error::Denied(
                "Domain authority snapshot export requires owner/admin authority",
            ));
        }
        authority::require_domain_authority_write(&c, &domain.id)?;
        let local_node: String = db(c.query_row(
            "SELECT value FROM store_meta WHERE key='local_node_id'",
            [],
            |r| r.get(0),
        ))?;
        let source_store_id: String = db(c.query_row(
            "SELECT value FROM store_meta WHERE key='store_id'",
            [],
            |r| r.get(0),
        ))?;
        let leader: Option<(String, u64, i64)> = if set.quorum > 1 {
            db(c.query_row(
                "SELECT election_id,term,expires_at FROM domain_authority_elections WHERE domain_id=?1 AND set_revision=?2 AND members_sha256=?3 AND candidate_node_id=?4 AND state='active' AND term=?5 AND expires_at>?6 ORDER BY term DESC LIMIT 1",
                params![domain.id,set.set_revision,set.members_sha256,local_node,set.current_term,now()],
                |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
            ).optional())?
        } else {
            None
        };
        if set.quorum > 1 && leader.is_none() {
            return Err(Error::Denied(
                "HA Domain snapshot export requires the current live quorum leader",
            ));
        }
        let mut q = db(c.prepare(
            "SELECT u.user_id,u.realm_id,u.status,u.display_json,u.security_epoch,u.created_at,m.role,m.status,m.policy_revision,m.joined_at,m.revoked_at FROM domain_memberships m JOIN users u ON u.user_id=m.user_id WHERE m.domain_id=?1 ORDER BY u.user_id",
        ))?;
        let rows = db(q.query_map([&domain.id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, u64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, u64>(8)?,
                r.get::<_, i64>(9)?,
                r.get::<_, Option<i64>>(10)?,
            ))
        }))?;
        let mut users = Vec::new();
        let mut memberships = Vec::new();
        for row in rows {
            let row = db(row)?;
            users.push(DomainAuthorityReplicaUser {
                user_id: row.0.clone(),
                realm_id: row.1,
                status: row.2,
                display: decode(&row.3)?,
                security_epoch: row.4,
                created_at: row.5,
            });
            memberships.push(DomainAuthorityReplicaMembership {
                user_id: row.0,
                role: row.6,
                status: row.7,
                policy_revision: row.8,
                joined_at: row.9,
                revoked_at: row.10,
            });
        }
        let through_commit_seq: u64 = db(c.query_row(
            "SELECT coalesce(max(seq),0) FROM commit_log",
            [],
            |r| r.get(0),
        ))?;
        let created_at = now();
        let mut snapshot = DomainAuthorityControlSnapshot {
            version: AUTHORITY_SNAPSHOT_VERSION,
            authority_domain_id: domain.id,
            source_store_id,
            source_node_id: local_node,
            parent_domain_id: domain.parent_domain_id,
            domain_name: domain.name,
            domain_policy_revision: domain.policy_revision,
            domain_security_epoch: domain.security_epoch,
            domain_created_at: domain.created_at,
            authority_set: set.clone(),
            users,
            memberships,
            through_commit_seq,
            leader_election_id: leader.as_ref().map(|v| v.0.clone()),
            leader_term: leader.as_ref().map(|v| v.1).unwrap_or(set.current_term),
            leader_expires_at: leader.as_ref().map(|v| v.2),
            created_at,
            snapshot_sha256: String::new(),
        };
        snapshot.snapshot_sha256 = snapshot_hash(&snapshot)?;
        Ok(snapshot)
    }

    pub fn import_domain_authority_replica_snapshot(
        &self,
        actor: &PrincipalId,
        authenticated_source_node: &str,
        snapshot: &DomainAuthorityControlSnapshot,
        source_signature: &str,
    ) -> Result<DomainAuthorityReplicaSnapshotInfo> {
        if snapshot.version != AUTHORITY_SNAPSHOT_VERSION
            || snapshot.source_node_id != authenticated_source_node
            || snapshot.authority_set.domain_id != snapshot.authority_domain_id
            || snapshot.authority_set.set_revision == 0
            || snapshot.authority_set.quorum == 0
        {
            return Err(Error::Protocol(
                "invalid Domain authority snapshot envelope",
            ));
        }
        if snapshot_hash(snapshot)? != snapshot.snapshot_sha256 {
            return Err(Error::Conflict("Domain authority snapshot hash mismatch"));
        }
        if authority::authority_set_hash(
            &snapshot.authority_domain_id,
            snapshot.authority_set.set_revision,
            snapshot.authority_set.quorum,
            &snapshot.authority_set.members,
        )? != snapshot.authority_set.members_sha256
        {
            return Err(Error::Conflict(
                "Domain authority snapshot AuthoritySet hash mismatch",
            ));
        }
        if !snapshot
            .authority_set
            .members
            .contains(&snapshot.source_node_id)
        {
            return Err(Error::Denied("snapshot source is outside the AuthoritySet"));
        }
        let local = self.local_machine()?;
        if !snapshot.authority_set.members.contains(&local.node_id) {
            return Err(Error::Denied(
                "local AwNode is outside the replicated AuthoritySet",
            ));
        }
        if snapshot.authority_set.quorum > 1
            && (snapshot.leader_election_id.is_none()
                || snapshot.leader_term != snapshot.authority_set.current_term
                || snapshot
                    .leader_expires_at
                    .is_none_or(|expiry| expiry <= now()))
        {
            return Err(Error::Denied(
                "HA Domain snapshot is not backed by a live source leader lease",
            ));
        }
        let peer = self.awnet_peer_by_node_id(authenticated_source_node)?;
        verify_snapshot_signature(&peer.node_id, &peer.public_key, snapshot, source_signature)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (grant_id, _user) = accepted_replica_grant(
            &tx,
            actor,
            &snapshot.authority_domain_id,
            snapshot.authority_set.set_revision,
            &snapshot.authority_set.members_sha256,
            snapshot.authority_set.quorum,
        )?;
        let existing_hash:Option<String>=db(tx.query_row(
            "SELECT snapshot_id FROM domain_authority_replica_snapshots WHERE authority_domain_id=?1 AND snapshot_sha256=?2",
            params![snapshot.authority_domain_id,snapshot.snapshot_sha256],|r|r.get(0)).optional())?;
        if let Some(id) = existing_hash {
            drop(tx);
            return replica_snapshot_row(&c, &id);
        }
        let head:Option<(String,u64,u64,String,String)>=db(tx.query_row(
            "SELECT source_store_id,set_revision,through_commit_seq,members_sha256,snapshot_id FROM domain_authority_replica_heads WHERE authority_domain_id=?1",
            [&snapshot.authority_domain_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional())?;
        if let Some((old_store, old_revision, old_seq, old_hash, old_snapshot_id)) = head {
            if snapshot.authority_set.set_revision < old_revision {
                return Err(Error::Denied("stale Domain AuthoritySet replica snapshot"));
            }
            if snapshot.authority_set.set_revision == old_revision {
                if snapshot.authority_set.members_sha256 != old_hash {
                    return Err(Error::Conflict(
                        "same AuthoritySet revision was rebound to another member hash",
                    ));
                }
                if snapshot.source_store_id == old_store && snapshot.through_commit_seq <= old_seq {
                    return Err(Error::Conflict(
                        "Domain authority replica snapshot does not advance its source cursor",
                    ));
                }
                if snapshot.source_store_id != old_store {
                    let previous = replica_snapshot_row(&tx, &old_snapshot_id)?;
                    if snapshot.leader_term <= previous.snapshot.leader_term {
                        return Err(Error::Conflict(
                            "authority replica source changed without a newer authority term",
                        ));
                    }
                }
            }
        }
        let snapshot_id = id("arepl")?;
        let imported_at = now();
        db(tx.execute(
            "INSERT INTO domain_authority_replica_snapshots(snapshot_id,authority_domain_id,source_store_id,source_node_id,set_revision,members_sha256,quorum,authority_term,through_commit_seq,snapshot_sha256,body_json,source_signature,imported_by_grant_id,imported_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![snapshot_id,snapshot.authority_domain_id,snapshot.source_store_id,snapshot.source_node_id,snapshot.authority_set.set_revision,snapshot.authority_set.members_sha256,snapshot.authority_set.quorum,snapshot.authority_set.current_term,snapshot.through_commit_seq,snapshot.snapshot_sha256,json(snapshot)?,source_signature,grant_id,imported_at],
        ))?;
        db(tx.execute(
            "INSERT INTO domain_authority_replica_heads(authority_domain_id,snapshot_id,source_store_id,set_revision,members_sha256,through_commit_seq,status,revision,updated_at) VALUES(?1,?2,?3,?4,?5,?6,'standby',1,?7) ON CONFLICT(authority_domain_id) DO UPDATE SET snapshot_id=excluded.snapshot_id,source_store_id=excluded.source_store_id,set_revision=excluded.set_revision,members_sha256=excluded.members_sha256,through_commit_seq=excluded.through_commit_seq,status='standby',revision=domain_authority_replica_heads.revision+1,updated_at=excluded.updated_at",
            params![snapshot.authority_domain_id,snapshot_id,snapshot.source_store_id,snapshot.authority_set.set_revision,snapshot.authority_set.members_sha256,snapshot.through_commit_seq,imported_at],
        ))?;
        db(tx.commit())?;
        replica_snapshot_row(&c, &snapshot_id)
    }

    pub fn domain_authority_replica(
        &self,
        actor: &PrincipalId,
        authority_domain_id: &str,
    ) -> Result<DomainAuthorityReplicaSnapshotInfo> {
        let c = self.connection()?;
        let head = replica_head_row(&c, authority_domain_id)?;
        let _ = accepted_replica_grant(
            &c,
            actor,
            authority_domain_id,
            head.snapshot.authority_set.set_revision,
            &head.snapshot.authority_set.members_sha256,
            head.snapshot.authority_set.quorum,
        )?;
        Ok(head)
    }

    pub fn begin_domain_authority_replica_election(
        &self,
        actor: &PrincipalId,
        authority_domain_id: &str,
        ttl_seconds: u64,
    ) -> Result<DomainAuthorityReplicaElectionInfo> {
        if !(5..=300).contains(&ttl_seconds) {
            return Err(Error::Config(
                "Domain authority promotion lease TTL must be 5..=300 seconds".into(),
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let head = replica_head_row(&tx, authority_domain_id)?;
        if head.head_status != "standby" {
            return Err(Error::Conflict(
                "Domain authority replica is not in standby state",
            ));
        }
        let (_grant, user) = accepted_replica_grant(
            &tx,
            actor,
            authority_domain_id,
            head.snapshot.authority_set.set_revision,
            &head.snapshot.authority_set.members_sha256,
            head.snapshot.authority_set.quorum,
        )?;
        let local: String = db(tx.query_row(
            "SELECT value FROM store_meta WHERE key='local_node_id'",
            [],
            |r| r.get(0),
        ))?;
        if !head.snapshot.authority_set.members.contains(&local) {
            return Err(Error::Denied("local AwNode is not an AuthoritySet member"));
        }
        let active_other:bool=db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM domain_authority_replica_elections WHERE authority_domain_id=?1 AND state='active' AND expires_at>?2 AND candidate_node_id<>?3)",params![authority_domain_id,now(),local],|r|r.get(0)))?;
        if active_other {
            return Err(Error::Denied(
                "another standby promotion leader lease remains active",
            ));
        }
        let max_term:u64=db(tx.query_row("SELECT coalesce(max(term),0) FROM domain_authority_replica_elections WHERE authority_domain_id=?1",[authority_domain_id],|r|r.get(0)))?;
        let term = max_term
            .max(head.snapshot.authority_set.current_term)
            .max(head.snapshot.leader_term)
            .checked_add(1)
            .ok_or(Error::Limit("Domain authority term exhausted"))?;
        let election = id("relect")?;
        let created = now();
        let ttl_micros = i64::try_from(
            ttl_seconds
                .checked_mul(1_000_000)
                .ok_or(Error::Limit("authority promotion TTL overflow"))?,
        )
        .map_err(|_| Error::Limit("authority promotion TTL overflow"))?;
        let expires = created
            .checked_add(ttl_micros)
            .ok_or(Error::Limit("authority promotion expiry overflow"))?;
        db(tx.execute(
            "INSERT INTO domain_authority_replica_elections(election_id,authority_domain_id,snapshot_id,set_revision,members_sha256,quorum,term,candidate_node_id,requested_ttl_seconds,state,created_by_user_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'collecting',?10,?11)",
            params![election,authority_domain_id,head.id,head.snapshot.authority_set.set_revision,head.snapshot.authority_set.members_sha256,head.snapshot.authority_set.quorum,term,local,ttl_seconds,user,created],
        ))?;
        db(tx.execute("INSERT INTO domain_authority_replica_votes(election_id,voter_node_id,expires_at,signature,received_at) VALUES(?1,?2,?3,NULL,?4)",params![election,local,expires,created]))?;
        db(tx.commit())?;
        replica_election_row(&c, &election)
    }

    pub fn domain_authority_replica_election(
        &self,
        actor: &PrincipalId,
        election_id: &str,
    ) -> Result<DomainAuthorityReplicaElectionInfo> {
        let c = self.connection()?;
        let election = replica_election_row(&c, election_id)?;
        let head = replica_snapshot_row(&c, &election.snapshot_id)?;
        let _ = accepted_replica_grant(
            &c,
            actor,
            &election.authority_domain_id,
            election.set_revision,
            &election.members_sha256,
            election.quorum,
        )?;
        if head.snapshot.authority_domain_id != election.authority_domain_id {
            return Err(Error::Conflict("replica election snapshot mismatch"));
        }
        Ok(election)
    }

    pub fn record_domain_authority_replica_vote(
        &self,
        actor: &PrincipalId,
        election_id: &str,
        voter_node_id: &str,
        expires_at: i64,
        signature: &str,
    ) -> Result<DomainAuthorityReplicaElectionInfo> {
        label(signature, 256)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let election = replica_election_row(&tx, election_id)?;
        let head = replica_snapshot_row(&tx, &election.snapshot_id)?;
        let _ = accepted_replica_grant(
            &tx,
            actor,
            &election.authority_domain_id,
            election.set_revision,
            &election.members_sha256,
            election.quorum,
        )?;
        if election.state != "collecting" {
            return Err(Error::Conflict(
                "Domain authority replica election is not collecting votes",
            ));
        }
        if !head
            .snapshot
            .authority_set
            .members
            .iter()
            .any(|n| n == voter_node_id)
        {
            return Err(Error::Denied(
                "replica authority vote is outside the AuthoritySet",
            ));
        }
        let local: String = db(tx.query_row(
            "SELECT value FROM store_meta WHERE key='local_node_id'",
            [],
            |r| r.get(0),
        ))?;
        if voter_node_id == local {
            return Err(Error::Conflict(
                "local replica vote is created with the election",
            ));
        }
        if expires_at <= now() {
            return Err(Error::Denied("replica authority vote is already expired"));
        }
        let public: Option<String> = db(tx
            .query_row(
                "SELECT public_key FROM awnet_peers WHERE node_id=?1 AND trust_state='trusted'",
                [voter_node_id],
                |r| r.get(0),
            )
            .optional())?;
        let Some(public) = public else {
            return Err(Error::Denied("replica authority voter peer is not trusted"));
        };
        let vote = DomainAuthorityPeerVoteInfo {
            authority_domain_id: election.authority_domain_id.clone(),
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
        authority::verify_domain_authority_vote_signature(
            voter_node_id,
            &public,
            &vote,
            signature,
        )?;
        let existing:Option<(i64,Option<String>)>=db(tx.query_row("SELECT expires_at,signature FROM domain_authority_replica_votes WHERE election_id=?1 AND voter_node_id=?2",params![election_id,voter_node_id],|r|Ok((r.get(0)?,r.get(1)?))).optional())?;
        if let Some((old_exp, old_sig)) = existing {
            if old_exp == expires_at && old_sig.as_deref() == Some(signature) {
                return Ok(election);
            }
            return Err(Error::Conflict(
                "replica authority voter attempted to rebind an election vote",
            ));
        }
        db(tx.execute("INSERT INTO domain_authority_replica_votes(election_id,voter_node_id,expires_at,signature,received_at) VALUES(?1,?2,?3,?4,?5)",params![election_id,voter_node_id,expires_at,signature,now()]))?;
        db(tx.commit())?;
        replica_election_row(&c, election_id)
    }

    pub fn activate_domain_authority_replica_election(
        &self,
        actor: &PrincipalId,
        election_id: &str,
    ) -> Result<DomainAuthorityReplicaElectionInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let election = replica_election_row(&tx, election_id)?;
        let head = replica_snapshot_row(&tx, &election.snapshot_id)?;
        let _ = accepted_replica_grant(
            &tx,
            actor,
            &election.authority_domain_id,
            election.set_revision,
            &election.members_sha256,
            election.quorum,
        )?;
        if election.state == "active" {
            drop(tx);
            return replica_election_row(&c, election_id);
        }
        if election.state != "collecting" {
            return Err(Error::Conflict(
                "Domain authority replica election is not activatable",
            ));
        }
        if head.head_status != "standby"
            || head.snapshot.authority_set.set_revision != election.set_revision
            || head.snapshot.authority_set.members_sha256 != election.members_sha256
            || head.snapshot.authority_set.quorum != election.quorum
        {
            return Err(Error::Conflict(
                "standby AuthoritySet changed while promotion election was collecting",
            ));
        }
        let now_value = now();
        if head.snapshot.authority_set.quorum > 1
            && head
                .snapshot
                .leader_expires_at
                .is_some_and(|expiry| expiry > now_value)
        {
            return Err(Error::Denied(
                "standby promotion is fenced until the replicated source leader lease expires",
            ));
        }
        let votes = {
            let mut q = db(tx.prepare(
                "SELECT voter_node_id,expires_at FROM domain_authority_replica_votes WHERE election_id=?1 AND expires_at>?2 ORDER BY voter_node_id",
            ))?;
            let rows = db(q.query_map(params![election_id, now_value], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            }))?;
            db(rows.collect::<rusqlite::Result<Vec<_>>>())?
        };
        let valid: Vec<_> = votes
            .into_iter()
            .filter(|(node, _)| head.snapshot.authority_set.members.contains(node))
            .collect();
        if valid.len() < election.quorum as usize {
            return Err(Error::Denied(
                "Domain authority replica promotion lacks quorum",
            ));
        }
        let expires = valid
            .iter()
            .map(|(_, e)| *e)
            .min()
            .ok_or(Error::Denied("replica promotion quorum has no live votes"))?;
        db(tx.execute("UPDATE domain_authority_replica_elections SET state='superseded' WHERE authority_domain_id=?1 AND state='active'",[&election.authority_domain_id]))?;
        db(tx.execute("UPDATE domain_authority_replica_elections SET state='active',activated_at=?1,expires_at=?2 WHERE election_id=?3 AND state='collecting'",params![now_value,expires,election_id]))?;
        db(tx.commit())?;
        replica_election_row(&c, election_id)
    }

    pub fn promote_domain_authority_replica(
        &self,
        actor: &PrincipalId,
        election_id: &str,
    ) -> Result<DomainAuthorityPromotionInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let election = replica_election_row(&tx, election_id)?;
        let head = replica_snapshot_row(&tx, &election.snapshot_id)?;
        let (_grant, user) = accepted_replica_grant(
            &tx,
            actor,
            &election.authority_domain_id,
            election.set_revision,
            &election.members_sha256,
            election.quorum,
        )?;
        if election.state != "active" || election.expires_at.is_none_or(|v| v <= now()) {
            return Err(Error::Denied(
                "Domain authority replica promotion requires a live activated quorum lease",
            ));
        }
        if head.head_status != "standby" {
            return Err(Error::Conflict(
                "Domain authority replica is no longer standby",
            ));
        }
        let local_node: String = db(tx.query_row(
            "SELECT value FROM store_meta WHERE key='local_node_id'",
            [],
            |r| r.get(0),
        ))?;
        if election.candidate_node_id != local_node {
            return Err(Error::Denied(
                "this AwNode is not the promoted quorum candidate",
            ));
        }
        let exists: bool = db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM domains WHERE domain_id=?1)",
            [&election.authority_domain_id],
            |r| r.get(0),
        ))?;
        if exists {
            return Err(Error::Conflict(
                "Domain already exists locally; generic authority merge is refused",
            ));
        }
        if let Some(parent) = head.snapshot.parent_domain_id.as_deref() {
            let present: bool = db(tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM domains WHERE domain_id=?1 AND status='active')",
                [parent],
                |r| r.get(0),
            ))?;
            if !present {
                return Err(Error::Denied(
                    "promoted child Domain requires its parent Domain to exist locally first",
                ));
            }
        }
        for replica_user in &head.snapshot.users {
            let existing:Option<(String,String,String,u64,i64)>=db(tx.query_row("SELECT realm_id,status,display_json,security_epoch,created_at FROM users WHERE user_id=?1",[&replica_user.user_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional())?;
            let display = json(&replica_user.display)?;
            if let Some(old) = existing {
                if old
                    != (
                        replica_user.realm_id.clone(),
                        replica_user.status.clone(),
                        display.clone(),
                        replica_user.security_epoch,
                        replica_user.created_at,
                    )
                {
                    return Err(Error::Conflict(
                        "stable User ID collides with different local safe metadata",
                    ));
                }
            } else {
                db(tx.execute("INSERT INTO users(user_id,realm_id,status,display_json,security_epoch,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![replica_user.user_id,replica_user.realm_id,replica_user.status,display,replica_user.security_epoch,replica_user.created_at]))?;
            }
        }
        let local_store: String = db(tx.query_row(
            "SELECT value FROM store_meta WHERE key='store_id'",
            [],
            |r| r.get(0),
        ))?;
        let promoted_at = now();
        let policy_revision =
            head.snapshot
                .domain_policy_revision
                .checked_add(1)
                .ok_or(Error::Limit(
                    "Domain policy revision exhausted during failover",
                ))?;
        let security_epoch =
            head.snapshot
                .domain_security_epoch
                .checked_add(1)
                .ok_or(Error::Limit(
                    "Domain security epoch exhausted during failover",
                ))?;
        db(tx.execute("INSERT INTO domains(domain_id,authority_store_id,parent_domain_id,name,status,policy_revision,security_epoch,created_at,updated_at) VALUES(?1,?2,?3,?4,'active',?5,?6,?7,?8)",params![head.snapshot.authority_domain_id,local_store,head.snapshot.parent_domain_id,head.snapshot.domain_name,policy_revision,security_epoch,head.snapshot.domain_created_at,promoted_at]))?;
        for membership in &head.snapshot.memberships {
            db(tx.execute("INSERT INTO domain_memberships(domain_id,user_id,role,status,policy_revision,joined_at,revoked_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![head.snapshot.authority_domain_id,membership.user_id,membership.role,membership.status,membership.policy_revision,membership.joined_at,membership.revoked_at]))?;
        }
        db(tx.execute("INSERT INTO domain_authority_sets(domain_id,set_revision,quorum,members_sha256,current_term,updated_at) VALUES(?1,?2,?3,?4,?5,?6)",params![head.snapshot.authority_domain_id,election.set_revision,election.quorum,election.members_sha256,election.term,promoted_at]))?;
        for (ordinal, node) in head.snapshot.authority_set.members.iter().enumerate() {
            db(tx.execute("INSERT INTO domain_authority_members(domain_id,node_id,ordinal,state,joined_at) VALUES(?1,?2,?3,'active',?4)",params![head.snapshot.authority_domain_id,node,ordinal as u64,promoted_at]))?;
        }
        db(tx.execute("INSERT INTO domain_authority_elections(election_id,domain_id,set_revision,members_sha256,quorum,term,candidate_node_id,requested_ttl_seconds,state,created_by_user_id,created_at,activated_at,expires_at) SELECT election_id,authority_domain_id,set_revision,members_sha256,quorum,term,candidate_node_id,requested_ttl_seconds,'active',?1,created_at,activated_at,expires_at FROM domain_authority_replica_elections WHERE election_id=?2",params![user,election_id]))?;
        let votes = {
            let mut q=db(tx.prepare("SELECT voter_node_id,expires_at,signature,received_at FROM domain_authority_replica_votes WHERE election_id=?1 ORDER BY voter_node_id"))?;
            let rows = db(q.query_map([election_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            }))?;
            db(rows.collect::<rusqlite::Result<Vec<_>>>())?
        };
        for vote in votes {
            db(tx.execute("INSERT INTO domain_authority_votes(election_id,voter_node_id,expires_at,signature,received_at) VALUES(?1,?2,?3,?4,?5)",params![election_id,vote.0,vote.1,vote.2,vote.3]))?;
        }
        db(tx.execute(
            "UPDATE domain_authority_replica_elections SET state='promoted' WHERE election_id=?1",
            [election_id],
        ))?;
        db(tx.execute("UPDATE domain_authority_replica_heads SET status='promoted',revision=revision+1,updated_at=?1 WHERE authority_domain_id=?2 AND snapshot_id=?3",params![promoted_at,election.authority_domain_id,election.snapshot_id]))?;
        db(tx.execute("UPDATE domain_authority_promotions SET status='superseded' WHERE authority_domain_id=?1 AND status='active'",[&election.authority_domain_id]))?;
        let promotion = id("promote")?;
        let expiry = election
            .expires_at
            .ok_or(Error::Storage("activated replica election expiry missing"))?;
        db(tx.execute("INSERT INTO domain_authority_promotions(promotion_id,authority_domain_id,snapshot_id,election_id,previous_authority_store_id,promoted_authority_store_id,promoted_node_id,promoted_term,leader_expires_at,status,promoted_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'active',?10)",params![promotion,election.authority_domain_id,election.snapshot_id,election.id,head.snapshot.source_store_id,local_store,local_node,election.term,expiry,promoted_at]))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "domain.authority_replica_promoted",
            "domain",
            &election.authority_domain_id,
            Some(actor),
            serde_json::json!({"domain_id":election.authority_domain_id,"snapshot_id":election.snapshot_id,"election_id":election.id,"term":election.term,"previous_authority_store_id":head.snapshot.source_store_id,"promoted_authority_store_id":local_store,"policy_revision":policy_revision,"security_epoch":security_epoch,"leader_expires_at":expiry}),
        )?;
        db(tx.commit())?;
        promotion_row(&c, &promotion)
    }
}
