use super::*;
use serde::{Deserialize, Serialize};

// Semantic commit-log envelope version. This is intentionally independent of
// SQLite PRAGMA user_version and the migration coordination schema epoch.
const COMMIT_EVENT_SCHEMA_VERSION: i64 = 3;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetryMode {
    #[default]
    Interactive,
    Autonomous,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RetryPolicy {
    #[serde(default)]
    pub mode: RetryMode,
    pub max_attempts: u32,
    pub retry_unknown: bool,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
}
impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            mode: RetryMode::Interactive,
            max_attempts: 16,
            retry_unknown: false,
            initial_backoff_ms: 1_000,
            max_backoff_ms: 60_000,
        }
    }
}
impl RetryPolicy {
    pub fn validate(&self) -> Result<()> {
        if self.max_attempts == 0 || self.max_attempts > 16 {
            return Err(Error::Config("retry max_attempts must be 1..16".into()));
        }
        if self.initial_backoff_ms > 86_400_000
            || self.max_backoff_ms > 86_400_000
            || self.max_backoff_ms < self.initial_backoff_ms
        {
            return Err(Error::Config("invalid retry backoff bounds".into()));
        }
        if matches!(self.mode, RetryMode::Interactive)
            && self.max_attempts > 1
            && self.retry_unknown
        {
            return Err(Error::Config(
                "interactive retries require explicit user confirmation".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryHint {
    pub id: String,
    pub operation_id: String,
    pub session_id: String,
    pub branch_id: String,
    pub kind: String,
    pub severity: String,
    pub message: String,
    pub actions: serde_json::Value,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableOperation {
    pub id: String,
    pub session_id: String,
    pub branch_id: String,
    pub origin_head: String,
    pub origin_revision: u64,
    pub prompt: String,
    pub identity: aw_agent_core::InvocationIdentity,
    pub routing_operation_id: Option<String>,
    pub retry_policy: RetryPolicy,
    pub state: String,
    pub attempts: u32,
    pub unknown_attempts: u32,
    pub next_retry_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitEvent {
    pub seq: u64,
    pub event_id: String,
    pub kind: String,
    pub entity_type: String,
    pub entity_id: String,
    pub actor: Option<String>,
    pub payload: serde_json::Value,
    pub created_at: i64,
}

/// One bounded source-sequence page from the local semantic event stream.
///
/// `through_seq` is the last source sequence examined, not merely the last
/// event disclosed to `actor`. This distinction is required because local-only
/// or unauthorized events may be skipped without stalling a subscriber behind
/// the same invisible rows forever.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitEventPage {
    pub after_seq: u64,
    pub through_seq: u64,
    pub events: Vec<CommitEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticSyncBatch {
    pub source_store_id: String,
    pub after_seq: u64,
    pub through_seq: u64,
    pub events: Vec<CommitEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticSyncApply {
    pub federation_id: String,
    pub source_store_id: String,
    pub through_seq: u64,
    pub imported: usize,
    pub reused: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticMaterializationPolicy {
    pub id: String,
    pub federation_id: String,
    pub local_domain_id: String,
    pub source_store_id: String,
    pub event_kind: String,
    pub entity_type: String,
    pub entity_id: String,
    pub status: String,
    pub revision: u64,
    pub created_by_user_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub revoked_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticMaterializationPolicyResult {
    pub policy: SemanticMaterializationPolicy,
    pub reused: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticMaterializedFact {
    pub id: String,
    pub local_domain_id: String,
    pub federation_id: String,
    pub source_store_id: String,
    pub source_seq: u64,
    pub source_event_id: String,
    pub source_event_sha256: String,
    pub source_kind: String,
    pub source_entity_type: String,
    pub source_entity_id: String,
    pub source_actor: Option<String>,
    pub source_payload: serde_json::Value,
    pub source_created_at: i64,
    pub policy_id: String,
    pub policy_revision: u64,
    pub materialized_by_user_id: String,
    pub materialized_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticMaterializeResult {
    pub fact: SemanticMaterializedFact,
    pub reused: bool,
}

pub(crate) fn semantic_event_hash(event: &CommitEvent) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(json(event)?.as_bytes())))
}

pub(crate) fn semantic_materialization_policy_row(
    c: &Connection,
    policy_id: &str,
) -> Result<SemanticMaterializationPolicy> {
    let row = db(c
        .query_row(
            "SELECT policy_id,federation_id,local_domain_id,source_store_id,event_kind,entity_type,entity_id,status,revision,created_by_user_id,created_at,updated_at,revoked_at FROM semantic_materialization_policies WHERE policy_id=?1",
            [policy_id],
            |r| {
                Ok(SemanticMaterializationPolicy {
                    id: r.get(0)?,
                    federation_id: r.get(1)?,
                    local_domain_id: r.get(2)?,
                    source_store_id: r.get(3)?,
                    event_kind: r.get(4)?,
                    entity_type: r.get(5)?,
                    entity_id: r.get(6)?,
                    status: r.get(7)?,
                    revision: r.get(8)?,
                    created_by_user_id: r.get(9)?,
                    created_at: r.get(10)?,
                    updated_at: r.get(11)?,
                    revoked_at: r.get(12)?,
                })
            },
        )
        .optional())?
        .ok_or(Error::Denied("semantic materialization policy unavailable"))?;
    Ok(row)
}

pub(crate) fn semantic_materialized_fact_row(
    c: &Connection,
    materialization_id: &str,
) -> Result<SemanticMaterializedFact> {
    let row = db(c
        .query_row(
            "SELECT materialization_id,local_domain_id,federation_id,source_store_id,source_seq,source_event_id,source_event_sha256,source_kind,source_entity_type,source_entity_id,source_actor,source_payload_json,source_created_at,policy_id,policy_revision,materialized_by_user_id,materialized_at FROM semantic_materialized_facts WHERE materialization_id=?1",
            [materialization_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, u64>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, String>(9)?,
                    r.get::<_, Option<String>>(10)?,
                    r.get::<_, String>(11)?,
                    r.get::<_, i64>(12)?,
                    r.get::<_, String>(13)?,
                    r.get::<_, u64>(14)?,
                    r.get::<_, String>(15)?,
                    r.get::<_, i64>(16)?,
                ))
            },
        )
        .optional())?
        .ok_or(Error::Denied("semantic materialized fact unavailable"))?;
    Ok(SemanticMaterializedFact {
        id: row.0,
        local_domain_id: row.1,
        federation_id: row.2,
        source_store_id: row.3,
        source_seq: row.4,
        source_event_id: row.5,
        source_event_sha256: row.6,
        source_kind: row.7,
        source_entity_type: row.8,
        source_entity_id: row.9,
        source_actor: row.10,
        source_payload: decode(&row.11)?,
        source_created_at: row.12,
        policy_id: row.13,
        policy_revision: row.14,
        materialized_by_user_id: row.15,
        materialized_at: row.16,
    })
}

pub(crate) fn semantic_materialization_authority(
    c: &Connection,
    actor: &PrincipalId,
    federation_id: &str,
    require_admin: bool,
    require_active: bool,
) -> Result<(String, String)> {
    let federation: Option<(String, String, String)> = db(c
        .query_row(
            "SELECT local_domain_id,status,permissions_json FROM domain_federations WHERE federation_id=?1",
            [federation_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional())?;
    let Some((domain_id, federation_status, permissions_json)) = federation else {
        return Err(Error::Denied(
            "semantic materialization federation unavailable",
        ));
    };
    let domain_status: String = db(c.query_row(
        "SELECT status FROM domains WHERE domain_id=?1",
        [&domain_id],
        |r| r.get(0),
    ))?;
    if require_active && (federation_status != "active" || domain_status != "active") {
        return Err(Error::Denied(
            "semantic materialization requires an active Domain federation",
        ));
    }
    if require_active {
        let permissions: serde_json::Value = decode(&permissions_json)?;
        let accepts_inbound_sync = permissions
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item.as_str() == Some("sync.write")));
        if !accepts_inbound_sync {
            return Err(Error::Denied(
                "semantic materialization federation does not permit inbound sync evidence",
            ));
        }
    }
    let user = user_for_principal(c, actor)?.ok_or(Error::Denied(
        "semantic materialization requires current Domain membership",
    ))?;
    let role: Option<String> = db(c
        .query_row(
            "SELECT role FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active'",
            params![domain_id, user],
            |r| r.get(0),
        )
        .optional())?;
    let Some(role) = role else {
        return Err(Error::Denied(
            "semantic materialization requires current Domain membership",
        ));
    };
    if require_admin && !matches!(role.as_str(), "owner" | "admin") {
        return Err(Error::Denied(
            "semantic materialization requires Domain owner/admin authority",
        ));
    }
    Ok((domain_id, user))
}

pub(crate) fn tx_id() -> Result<String> {
    id("tx")
}
pub(crate) fn commit_event(
    c: &Connection,
    tx: &str,
    kind: &str,
    entity_type: &str,
    entity_id: &str,
    actor: Option<&PrincipalId>,
    payload: serde_json::Value,
) -> Result<()> {
    let store: String = db(c.query_row(
        "SELECT value FROM store_meta WHERE key='store_id'",
        [],
        |r| r.get(0),
    ))?;
    let event = id("ev")?;
    db(c.execute("INSERT INTO commit_log(event_id,store_id,tx_id,kind,entity_type,entity_id,actor,schema_version,payload_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![event,store,tx,kind,entity_type,entity_id,actor.map(PrincipalId::as_str),COMMIT_EVENT_SCHEMA_VERSION,json(&payload)?,now()]))?;
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn ref_event(
    c: &Connection,
    branch: &BranchInfo,
    new_head: &str,
    new_revision: u64,
    actor: &PrincipalId,
    cause_type: &str,
    cause_id: Option<&str>,
    txid: &str,
) -> Result<()> {
    let event = id("ref")?;
    let writer = crate::workspace_writer::workspace_writer_for_session(c, &branch.session_id)?;
    db(c.execute("INSERT INTO branch_ref_events(event_id,branch_id,old_head,old_revision,new_head,new_revision,actor,cause_type,cause_id,committed_at,writer_node_id,writer_epoch) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",params![event,branch.id,branch.head,branch.revision,new_head,new_revision,actor.as_str(),cause_type,cause_id,now(),writer.writer_node_id,writer.writer_epoch]))?;
    commit_event(
        c,
        txid,
        "branch.advanced",
        "branch",
        &branch.id,
        Some(actor),
        serde_json::json!({"old_head":branch.head,"old_revision":branch.revision,"new_head":new_head,"new_revision":new_revision,"cause_type":cause_type,"cause_id":cause_id,"writer_node_id":writer.writer_node_id,"writer_epoch":writer.writer_epoch}),
    )
}

impl SessionStore {
    pub fn store_id(&self) -> Result<String> {
        let c = self.connection()?;
        db(c.query_row(
            "SELECT value FROM store_meta WHERE key='store_id'",
            [],
            |r| r.get(0),
        ))
    }
    pub fn commit_event_page_after(
        &self,
        actor: &PrincipalId,
        after: u64,
        limit: usize,
    ) -> Result<CommitEventPage> {
        if limit == 0 || limit > 512 {
            return Err(Error::Limit(
                "commit event page must scan 1..512 source events",
            ));
        }
        let c = self.connection()?;
        let mut q=db(c.prepare("SELECT seq,event_id,kind,entity_type,entity_id,actor,payload_json,created_at FROM commit_log WHERE seq>?1 ORDER BY seq LIMIT ?2"))?;
        let rows = db(q.query_map(params![after, limit as i64], |r| {
            Ok((
                r.get::<_, u64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, i64>(7)?,
            ))
        }))?;
        let mut events = Vec::new();
        let mut through_seq = after;
        for row in rows {
            let (seq, event_id, kind, entity_type, entity_id, event_actor, payload, created_at) =
                db(row)?;
            through_seq = seq;
            // Events are filtered through their current authoritative scope.
            // Local-only presentation events are never semantic-sync payloads.
            // Session events use Session ACLs; Domain/Workspace/Fleet control-plane
            // events require current Domain membership. Unknown scopes fail closed.
            let p: serde_json::Value = decode(&payload)?;
            let local_only =
                p.get("sync_scope").and_then(serde_json::Value::as_str) == Some("local");
            let permitted = if local_only {
                false
            } else if let Some(session) = p.get("session_id").and_then(serde_json::Value::as_str) {
                authorize(&c, session, actor, false).is_ok()
            } else if let Some(domain) = p.get("domain_id").and_then(serde_json::Value::as_str) {
                if let Some(user) = user_for_principal(&c, actor)? {
                    db(c.query_row("SELECT EXISTS(SELECT 1 FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active')",params![domain,user],|r|r.get::<_,bool>(0)))?
                } else {
                    false
                }
            } else {
                false
            };
            if !permitted {
                continue;
            }
            events.push(CommitEvent {
                seq,
                event_id,
                kind,
                entity_type,
                entity_id,
                actor: event_actor,
                payload: p,
                created_at,
            });
        }
        Ok(CommitEventPage {
            after_seq: after,
            through_seq,
            events,
        })
    }

    /// Compatibility convenience for callers that only need disclosed events.
    /// Cursor-based consumers should use `commit_event_page_after` so they can
    /// advance across filtered source events without rescanning them.
    pub fn commit_events_after(
        &self,
        actor: &PrincipalId,
        after: u64,
        limit: usize,
    ) -> Result<Vec<CommitEvent>> {
        Ok(self.commit_event_page_after(actor, after, limit)?.events)
    }

    pub fn semantic_sync_export(
        &self,
        actor: &PrincipalId,
        lease_id: &str,
        peer_node_id: &str,
        after_seq: u64,
        limit: usize,
    ) -> Result<SemanticSyncBatch> {
        let lease = self.validate_federation_lease(lease_id, peer_node_id, "sync.read")?;
        let _ = self.domain_federation(actor, &lease.federation_id)?;
        let page = self.commit_event_page_after(actor, after_seq, limit)?;
        Ok(SemanticSyncBatch {
            source_store_id: self.store_id()?,
            after_seq,
            through_seq: page.through_seq,
            events: page.events,
        })
    }

    pub fn semantic_sync_apply(
        &self,
        lease_id: &str,
        peer_node_id: &str,
        batch: &SemanticSyncBatch,
    ) -> Result<SemanticSyncApply> {
        if batch.events.len() > 512 {
            return Err(Error::Limit("semantic sync batch exceeds 512 events"));
        }
        let lease = self.validate_federation_lease(lease_id, peer_node_id, "sync.write")?;
        let local_store = self.store_id()?;
        if batch.source_store_id == local_store {
            return Err(Error::Denied(
                "semantic sync cannot import this store's own commit stream",
            ));
        }
        if batch.through_seq < batch.after_seq {
            return Err(Error::Protocol(
                "semantic sync through_seq must not move backwards",
            ));
        }
        if batch.through_seq == batch.after_seq && !batch.events.is_empty() {
            return Err(Error::Protocol(
                "semantic sync events require cursor advancement",
            ));
        }
        let mut previous = batch.after_seq;
        for event in &batch.events {
            if event.seq <= previous || event.seq > batch.through_seq {
                return Err(Error::Protocol(
                    "semantic sync event sequence must be strictly increasing within the scanned cursor range",
                ));
            }
            if event.event_id.trim().is_empty()
                || event.kind.trim().is_empty()
                || event.entity_type.trim().is_empty()
                || event.entity_id.trim().is_empty()
            {
                return Err(Error::Protocol(
                    "semantic sync event identity fields must be nonempty",
                ));
            }
            if json(event)?.len() > 256 * 1024 {
                return Err(Error::Limit("semantic sync event exceeds 256 KiB"));
            }
            previous = event.seq;
        }

        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current: Option<u64> = db(tx
            .query_row(
                "SELECT last_source_seq FROM semantic_sync_cursors WHERE federation_id=?1 AND source_store_id=?2",
                params![lease.federation_id, batch.source_store_id],
                |r| r.get(0),
            )
            .optional())?;
        let current = current.unwrap_or(0);

        if batch.through_seq <= current {
            for event in &batch.events {
                let stored: Option<(String, String)> = db(tx
                    .query_row(
                        "SELECT source_event_id,event_sha256 FROM semantic_sync_inbox WHERE federation_id=?1 AND source_store_id=?2 AND source_seq=?3",
                        params![lease.federation_id, batch.source_store_id, event.seq],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional())?;
                let expected = semantic_event_hash(event)?;
                if stored.as_ref() != Some(&(event.event_id.clone(), expected)) {
                    return Err(Error::Conflict(
                        "semantic sync retry differs from already imported source facts",
                    ));
                }
            }
            return Ok(SemanticSyncApply {
                federation_id: lease.federation_id,
                source_store_id: batch.source_store_id.clone(),
                through_seq: current,
                imported: 0,
                reused: true,
            });
        }

        if batch.after_seq != current {
            return Err(Error::Conflict(
                "semantic sync cursor changed; request a batch from the current cursor",
            ));
        }

        let imported_at = now();
        for event in &batch.events {
            let hash = semantic_event_hash(event)?;
            let conflicting: Option<u64> = db(tx
                .query_row(
                    "SELECT source_seq FROM semantic_sync_inbox WHERE federation_id=?1 AND source_store_id=?2 AND source_event_id=?3",
                    params![lease.federation_id, batch.source_store_id, event.event_id],
                    |r| r.get(0),
                )
                .optional())?;
            if conflicting.is_some_and(|seq| seq != event.seq) {
                return Err(Error::Conflict(
                    "semantic sync source event ID was rebound to another sequence",
                ));
            }
            db(tx.execute(
                "INSERT INTO semantic_sync_inbox(federation_id,source_store_id,source_seq,source_event_id,event_sha256,kind,entity_type,entity_id,actor,payload_json,source_created_at,imported_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                params![
                    lease.federation_id,
                    batch.source_store_id,
                    event.seq,
                    event.event_id,
                    hash,
                    event.kind,
                    event.entity_type,
                    event.entity_id,
                    event.actor,
                    json(&event.payload)?,
                    event.created_at,
                    imported_at
                ],
            ))?;
        }
        let last_event_id = batch.events.last().map(|event| event.event_id.as_str());
        db(tx.execute(
            "INSERT INTO semantic_sync_cursors(federation_id,source_store_id,last_source_seq,last_event_id,updated_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(federation_id,source_store_id) DO UPDATE SET last_source_seq=excluded.last_source_seq,last_event_id=excluded.last_event_id,updated_at=excluded.updated_at",
            params![
                lease.federation_id,
                batch.source_store_id,
                batch.through_seq,
                last_event_id,
                imported_at
            ],
        ))?;
        let txid = tx_id()?;
        commit_event(
            &tx,
            &txid,
            "semantic_sync.applied",
            "semantic_sync_cursor",
            &format!("{}:{}", lease.federation_id, batch.source_store_id),
            None,
            serde_json::json!({
                "federation_id":lease.federation_id,
                "source_store_id":batch.source_store_id,
                "through_seq":batch.through_seq,
                "imported":batch.events.len()
            }),
        )?;
        db(tx.commit())?;
        Ok(SemanticSyncApply {
            federation_id: lease.federation_id,
            source_store_id: batch.source_store_id.clone(),
            through_seq: batch.through_seq,
            imported: batch.events.len(),
            reused: false,
        })
    }

    pub fn semantic_sync_inbox(
        &self,
        actor: &PrincipalId,
        federation_id: &str,
        source_store_id: &str,
        after_seq: u64,
        limit: usize,
    ) -> Result<Vec<CommitEvent>> {
        if limit == 0 || limit > 512 {
            return Err(Error::Limit("semantic sync inbox batch must be 1..512"));
        }
        let _ = self.domain_federation(actor, federation_id)?;
        let c = self.connection()?;
        let mut q = db(c.prepare(
            "SELECT source_seq,source_event_id,kind,entity_type,entity_id,actor,payload_json,source_created_at FROM semantic_sync_inbox WHERE federation_id=?1 AND source_store_id=?2 AND source_seq>?3 ORDER BY source_seq LIMIT ?4",
        ))?;
        let rows = db(q.query_map(
            params![federation_id, source_store_id, after_seq, limit as i64],
            |r| {
                Ok((
                    r.get::<_, u64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                ))
            },
        ))?;
        let mut out = Vec::new();
        for row in rows {
            let (seq, event_id, kind, entity_type, entity_id, event_actor, payload, created_at) =
                db(row)?;
            out.push(CommitEvent {
                seq,
                event_id,
                kind,
                entity_type,
                entity_id,
                actor: event_actor,
                payload: decode(&payload)?,
                created_at,
            });
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_semantic_materialization_policy(
        &self,
        actor: &PrincipalId,
        federation_id: &str,
        source_store_id: &str,
        event_kind: &str,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<SemanticMaterializationPolicyResult> {
        for value in [
            federation_id,
            source_store_id,
            event_kind,
            entity_type,
            entity_id,
        ] {
            label(value, 256)?;
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (domain_id, user) =
            semantic_materialization_authority(&tx, actor, federation_id, true, true)?;
        authority::require_domain_authority_write(&tx, &domain_id)?;
        let evidence_exists: bool = db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM semantic_sync_inbox WHERE federation_id=?1 AND source_store_id=?2 AND kind=?3 AND entity_type=?4 AND entity_id=?5)",
            params![federation_id, source_store_id, event_kind, entity_type, entity_id],
            |r| r.get(0),
        ))?;
        if !evidence_exists {
            return Err(Error::Denied(
                "semantic materialization policy must select already imported evidence",
            ));
        }
        let existing: Option<String> = db(tx
            .query_row(
                "SELECT policy_id FROM semantic_materialization_policies WHERE federation_id=?1 AND source_store_id=?2 AND event_kind=?3 AND entity_type=?4 AND entity_id=?5 AND status='active'",
                params![federation_id, source_store_id, event_kind, entity_type, entity_id],
                |r| r.get(0),
            )
            .optional())?;
        if let Some(policy_id) = existing {
            drop(tx);
            return Ok(SemanticMaterializationPolicyResult {
                policy: semantic_materialization_policy_row(&c, &policy_id)?,
                reused: true,
            });
        }
        let policy_id = id("smp")?;
        let created = now();
        db(tx.execute(
            "INSERT INTO semantic_materialization_policies(policy_id,federation_id,local_domain_id,source_store_id,event_kind,entity_type,entity_id,status,revision,created_by_user_id,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,'active',1,?8,?9,?9)",
            params![policy_id, federation_id, domain_id, source_store_id, event_kind, entity_type, entity_id, user, created],
        ))?;
        let txid = tx_id()?;
        commit_event(
            &tx,
            &txid,
            "semantic_materialization.policy_created",
            "semantic_materialization_policy",
            &policy_id,
            Some(actor),
            serde_json::json!({
                "domain_id":domain_id,
                "federation_id":federation_id,
                "source_store_id":source_store_id,
                "event_kind":event_kind,
                "entity_type":entity_type,
                "entity_id":entity_id,
                "policy_revision":1
            }),
        )?;
        db(tx.commit())?;
        Ok(SemanticMaterializationPolicyResult {
            policy: semantic_materialization_policy_row(&c, &policy_id)?,
            reused: false,
        })
    }

    pub fn semantic_materialization_policies(
        &self,
        actor: &PrincipalId,
        federation_id: &str,
    ) -> Result<Vec<SemanticMaterializationPolicy>> {
        let c = self.connection()?;
        let _ = semantic_materialization_authority(&c, actor, federation_id, false, false)?;
        let mut q = db(c.prepare(
            "SELECT policy_id FROM semantic_materialization_policies WHERE federation_id=?1 ORDER BY created_at,policy_id LIMIT 512",
        ))?;
        let ids = db(db(q.query_map([federation_id], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter()
            .map(|policy_id| semantic_materialization_policy_row(&c, policy_id))
            .collect()
    }

    pub fn revoke_semantic_materialization_policy(
        &self,
        actor: &PrincipalId,
        policy_id: &str,
    ) -> Result<SemanticMaterializationPolicy> {
        label(policy_id, 128)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let policy = semantic_materialization_policy_row(&tx, policy_id)?;
        let (domain_id, _) =
            semantic_materialization_authority(&tx, actor, &policy.federation_id, true, false)?;
        authority::require_domain_authority_write(&tx, &domain_id)?;
        if policy.status == "active" {
            let revoked = now();
            db(tx.execute(
                "UPDATE semantic_materialization_policies SET status='revoked',revision=revision+1,updated_at=?1,revoked_at=?1 WHERE policy_id=?2 AND status='active'",
                params![revoked, policy_id],
            ))?;
            let txid = tx_id()?;
            commit_event(
                &tx,
                &txid,
                "semantic_materialization.policy_revoked",
                "semantic_materialization_policy",
                policy_id,
                Some(actor),
                serde_json::json!({
                    "domain_id":domain_id,
                    "federation_id":policy.federation_id,
                    "previous_revision":policy.revision,
                    "new_revision":policy.revision.saturating_add(1)
                }),
            )?;
        }
        db(tx.commit())?;
        semantic_materialization_policy_row(&c, policy_id)
    }

    pub fn materialize_semantic_fact(
        &self,
        actor: &PrincipalId,
        policy_id: &str,
        source_seq: u64,
    ) -> Result<SemanticMaterializeResult> {
        if source_seq == 0 {
            return Err(Error::Config(
                "semantic materialization source sequence must be positive".into(),
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let policy = semantic_materialization_policy_row(&tx, policy_id)?;
        let (domain_id, user) =
            semantic_materialization_authority(&tx, actor, &policy.federation_id, true, true)?;
        authority::require_domain_authority_write(&tx, &domain_id)?;
        if policy.status != "active" {
            return Err(Error::Denied(
                "semantic materialization policy is not active",
            ));
        }
        let inbox: Option<(String, String, String, String, String, Option<String>, String, i64)> =
            db(tx
                .query_row(
                    "SELECT source_event_id,event_sha256,kind,entity_type,entity_id,actor,payload_json,source_created_at FROM semantic_sync_inbox WHERE federation_id=?1 AND source_store_id=?2 AND source_seq=?3",
                    params![policy.federation_id, policy.source_store_id, source_seq],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
                )
                .optional())?;
        let Some((
            source_event_id,
            source_event_sha256,
            source_kind,
            source_entity_type,
            source_entity_id,
            source_actor,
            source_payload_json,
            source_created_at,
        )) = inbox
        else {
            return Err(Error::Denied(
                "semantic materialization source event is not present in the immutable inbox",
            ));
        };
        if source_kind != policy.event_kind
            || source_entity_type != policy.entity_type
            || source_entity_id != policy.entity_id
        {
            return Err(Error::Denied(
                "semantic materialization policy does not select this source event",
            ));
        }
        let source_payload: serde_json::Value = decode(&source_payload_json)?;
        let source_event = CommitEvent {
            seq: source_seq,
            event_id: source_event_id.clone(),
            kind: source_kind.clone(),
            entity_type: source_entity_type.clone(),
            entity_id: source_entity_id.clone(),
            actor: source_actor.clone(),
            payload: source_payload.clone(),
            created_at: source_created_at,
        };
        if semantic_event_hash(&source_event)? != source_event_sha256 {
            return Err(Error::Storage(
                "semantic materialization source hash no longer matches immutable evidence",
            ));
        }
        let existing: Option<String> = db(tx
            .query_row(
                "SELECT materialization_id FROM semantic_materialized_facts WHERE federation_id=?1 AND source_store_id=?2 AND source_seq=?3",
                params![policy.federation_id, policy.source_store_id, source_seq],
                |r| r.get(0),
            )
            .optional())?;
        if let Some(materialization_id) = existing {
            let existing = semantic_materialized_fact_row(&tx, &materialization_id)?;
            if existing.source_event_id != source_event_id
                || existing.source_event_sha256 != source_event_sha256
            {
                return Err(Error::Conflict(
                    "semantic materialization source identity conflicts with an existing local fact",
                ));
            }
            drop(tx);
            return Ok(SemanticMaterializeResult {
                fact: semantic_materialized_fact_row(&c, &materialization_id)?,
                reused: true,
            });
        }
        let materialization_id = id("smf")?;
        let materialized_at = now();
        db(tx.execute(
            "INSERT INTO semantic_materialized_facts(materialization_id,local_domain_id,federation_id,source_store_id,source_seq,source_event_id,source_event_sha256,source_kind,source_entity_type,source_entity_id,source_actor,source_payload_json,source_created_at,policy_id,policy_revision,materialized_by_user_id,materialized_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
            params![materialization_id, domain_id, policy.federation_id, policy.source_store_id, source_seq, source_event_id, source_event_sha256, source_kind, source_entity_type, source_entity_id, source_actor, source_payload_json, source_created_at, policy.id, policy.revision, user, materialized_at],
        ))?;
        let txid = tx_id()?;
        commit_event(
            &tx,
            &txid,
            "semantic_materialization.fact_created",
            "semantic_materialized_fact",
            &materialization_id,
            Some(actor),
            serde_json::json!({
                "domain_id":domain_id,
                "federation_id":policy.federation_id,
                "source_store_id":policy.source_store_id,
                "source_seq":source_seq,
                "source_event_id":source_event_id,
                "source_event_sha256":source_event_sha256,
                "source_kind":source_kind,
                "source_entity_type":source_entity_type,
                "source_entity_id":source_entity_id,
                "policy_id":policy.id,
                "policy_revision":policy.revision
            }),
        )?;
        db(tx.commit())?;
        Ok(SemanticMaterializeResult {
            fact: semantic_materialized_fact_row(&c, &materialization_id)?,
            reused: false,
        })
    }

    pub fn semantic_materialized_facts(
        &self,
        actor: &PrincipalId,
        federation_id: &str,
        source_store_id: &str,
        after_seq: u64,
        limit: usize,
    ) -> Result<Vec<SemanticMaterializedFact>> {
        if limit == 0 || limit > 512 {
            return Err(Error::Limit(
                "semantic materialized fact batch must be 1..512",
            ));
        }
        let c = self.connection()?;
        let _ = semantic_materialization_authority(&c, actor, federation_id, false, false)?;
        let mut q = db(c.prepare(
            "SELECT materialization_id FROM semantic_materialized_facts WHERE federation_id=?1 AND source_store_id=?2 AND source_seq>?3 ORDER BY source_seq LIMIT ?4",
        ))?;
        let ids = db(db(q.query_map(
            params![federation_id, source_store_id, after_seq, limit as i64],
            |r| r.get::<_, String>(0),
        ))?
        .collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter()
            .map(|materialization_id| semantic_materialized_fact_row(&c, materialization_id))
            .collect()
    }

    pub fn recovery_hints(
        &self,
        actor: &PrincipalId,
        session: Option<&str>,
    ) -> Result<Vec<RecoveryHint>> {
        let c = self.connection()?;
        let sql = if session.is_some() {
            "SELECT id,operation_id,session_id,branch_id,kind,severity,message,action_json,created_at FROM recovery_hints WHERE state='open' AND session_id=?1 ORDER BY created_at,id LIMIT 128"
        } else {
            "SELECT id,operation_id,session_id,branch_id,kind,severity,message,action_json,created_at FROM recovery_hints WHERE state='open' ORDER BY created_at,id LIMIT 128"
        };
        let mut q = db(c.prepare(sql))?;
        let map = |r: &rusqlite::Row<'_>| {
            rusqlite::Result::Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, i64>(8)?,
            ))
        };
        let rows = if let Some(s) = session {
            db(q.query_map([s], map))?.collect::<rusqlite::Result<Vec<_>>>()
        } else {
            db(q.query_map([], map))?.collect::<rusqlite::Result<Vec<_>>>()
        };
        let mut out = Vec::new();
        for row in db(rows)? {
            let (
                id,
                operation_id,
                session_id,
                branch_id,
                kind,
                severity,
                message,
                action,
                created_at,
            ) = row;
            if authorize(&c, &session_id, actor, false).is_err() {
                continue;
            }
            out.push(RecoveryHint {
                id,
                operation_id,
                session_id,
                branch_id,
                kind,
                severity,
                message,
                actions: decode(&action)?,
                created_at,
            });
        }
        Ok(out)
    }
    pub fn durable_operation(
        &self,
        actor: &PrincipalId,
        operation: &str,
    ) -> Result<DurableOperation> {
        let c = self.connection()?;
        let row=db(c.query_row("SELECT o.session_id,o.origin_branch_id,o.origin_head,o.origin_revision,n.payload,o.identity_json,o.routing_operation_id,o.retry_policy_json,o.state,(SELECT count(*) FROM llm_attempts a WHERE a.operation_id=o.id),(SELECT count(*) FROM llm_attempts a WHERE a.operation_id=o.id AND a.dispatch_state='unknown'),(SELECT due_at FROM retry_schedules rs WHERE rs.operation_id=o.id AND rs.state='scheduled') FROM llm_operations o JOIN nodes n ON n.id=o.prompt_node WHERE o.id=?1",[operation],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,u64>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,Option<String>>(6)?,r.get::<_,String>(7)?,r.get::<_,String>(8)?,r.get::<_,u32>(9)?,r.get::<_,u32>(10)?,r.get::<_,Option<i64>>(11)?))).optional())?.ok_or(Error::Denied("operation unavailable or not permitted"))?;
        authorize(&c, &row.0, actor, false)?;
        let payload: serde_json::Value = decode(&row.4)?;
        Ok(DurableOperation {
            id: operation.into(),
            session_id: row.0,
            branch_id: row.1,
            origin_head: row.2,
            origin_revision: row.3,
            prompt: payload
                .get("prompt")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_owned(),
            identity: decode(&row.5)?,
            routing_operation_id: row.6,
            retry_policy: decode(&row.7)?,
            state: row.8,
            attempts: row.9,
            unknown_attempts: row.10,
            next_retry_at: row.11,
        })
    }
    /// Explicit interactive retry permission. This does not dispatch a provider.
    /// It only arms the exact next admitted run to join the existing operation.
    pub fn authorize_retry(
        &self,
        actor: &PrincipalId,
        operation: &str,
    ) -> Result<DurableOperation> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (session, state, policy): (String, String, String) = db(tx.query_row(
            "SELECT session_id,state,retry_policy_json FROM llm_operations WHERE id=?1",
            [operation],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ))?;
        authorize(&tx, &session, actor, true)?;
        let p: RetryPolicy = decode(&policy)?;
        if !matches!(p.mode, RetryMode::Interactive) {
            return Err(Error::Denied(
                "autonomous retry authority belongs to its persisted policy",
            ));
        }
        if !matches!(state.as_str(), "interrupted" | "failed" | "needs_attention") {
            return Err(Error::Conflict(
                "operation is not awaiting an interactive retry",
            ));
        }
        let attempts: i64 = db(tx.query_row(
            "SELECT count(*) FROM llm_attempts WHERE operation_id=?1",
            [operation],
            |r| r.get(0),
        ))?;
        let next = attempts + 1;
        if next > 16 {
            return Err(Error::Limit("retry attempt limit reached"));
        }
        db(tx.execute("INSERT INTO retry_schedules(operation_id,generation,state,next_attempt_no,updated_at) VALUES(?1,1,'waiting_user',?2,?3) ON CONFLICT(operation_id) DO UPDATE SET generation=generation+1,state='waiting_user',due_at=NULL,next_attempt_no=excluded.next_attempt_no,claimed_by_client=NULL,claim_generation=NULL,updated_at=excluded.updated_at",params![operation,next,now()]))?;
        let txid = tx_id()?;
        commit_event(
            &tx,
            &txid,
            "llm.retry_authorized",
            "llm_operation",
            operation,
            Some(actor),
            serde_json::json!({"session_id":session,"next_attempt":next}),
        )?;
        db(tx.commit())?;
        drop(c);
        self.durable_operation(actor, operation)
    }
    pub fn schedule_autonomous_retry(
        &self,
        actor: &PrincipalId,
        operation: &str,
        now_micros: i64,
    ) -> Result<DurableOperation> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (session, state, policy): (String, String, String) = db(tx.query_row(
            "SELECT session_id,state,retry_policy_json FROM llm_operations WHERE id=?1",
            [operation],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ))?;
        authorize(&tx, &session, actor, true)?;
        let p: RetryPolicy = decode(&policy)?;
        if !matches!(p.mode, RetryMode::Autonomous) {
            return Err(Error::Denied(
                "interactive operation requires user-confirmed retry",
            ));
        }
        if !p.retry_unknown
            || !matches!(state.as_str(), "interrupted" | "failed" | "needs_attention")
        {
            return Err(Error::Conflict(
                "autonomous retry policy does not permit this outcome",
            ));
        }
        let attempts: u32 = db(tx.query_row(
            "SELECT count(*) FROM llm_attempts WHERE operation_id=?1",
            [operation],
            |r| r.get(0),
        ))?;
        if attempts >= p.max_attempts {
            db(tx.execute(
                "UPDATE llm_operations SET state='exhausted',settled_at=?1 WHERE id=?2",
                params![now(), operation],
            ))?;
            return Err(Error::Conflict("autonomous retry policy exhausted"));
        }
        let shift = attempts.saturating_sub(1).min(20);
        let factor = 1u64 << shift;
        let delay = p
            .initial_backoff_ms
            .saturating_mul(factor)
            .min(p.max_backoff_ms);
        let due = now_micros.saturating_add((delay as i64).saturating_mul(1000));
        db(tx.execute("INSERT INTO retry_schedules(operation_id,generation,state,due_at,next_attempt_no,updated_at) VALUES(?1,1,'scheduled',?2,?3,?4) ON CONFLICT(operation_id) DO UPDATE SET generation=generation+1,state='scheduled',due_at=excluded.due_at,next_attempt_no=excluded.next_attempt_no,claimed_by_client=NULL,claim_generation=NULL,updated_at=excluded.updated_at",params![operation,due,attempts+1,now()]))?;
        db(tx.execute(
            "UPDATE llm_operations SET state='retry_scheduled' WHERE id=?1",
            [operation],
        ))?;
        let hint = id("hint")?;
        db(tx.execute("INSERT INTO recovery_hints(id,operation_id,session_id,branch_id,kind,severity,message,action_json,state,created_at) SELECT ?1,id,session_id,origin_branch_id,'retry_scheduled','info','Autonomous retry is durably scheduled; no provider call has been repeated yet.',json_object('operation_id',id,'due_at',?2),'open',?3 FROM llm_operations WHERE id=?4",params![hint,due,now(),operation]))?;
        let txid = tx_id()?;
        commit_event(
            &tx,
            &txid,
            "llm.retry_scheduled",
            "llm_operation",
            operation,
            Some(actor),
            serde_json::json!({"session_id":session,"due_at":due,"next_attempt":attempts+1}),
        )?;
        db(tx.commit())?;
        drop(c);
        self.durable_operation(actor, operation)
    }
    /// Claim one due autonomous retry using a generation-fenced durable claim.
    /// Claiming is not provider dispatch; the caller must bind the returned
    /// operation to the next admitted run before making any model call.
    pub fn claim_due_autonomous_retry(
        &self,
        client_id: &str,
        at: i64,
    ) -> Result<Option<DurableOperation>> {
        if client_id.is_empty() || client_id.len() > 128 {
            return Err(Error::Config("invalid retry client ID".into()));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let row:Option<(String,i64)>=db(tx.query_row("SELECT operation_id,generation FROM retry_schedules WHERE state='scheduled' AND due_at IS NOT NULL AND due_at<=?1 ORDER BY due_at,operation_id LIMIT 1",[at],|r|Ok((r.get(0)?,r.get(1)?))).optional())?;
        let Some((operation, generation)) = row else {
            return Ok(None);
        };
        let changed=db(tx.execute("UPDATE retry_schedules SET state='claimed',claimed_by_client=?1,claim_generation=generation,updated_at=?2 WHERE operation_id=?3 AND generation=?4 AND state='scheduled'",params![client_id,now(),operation,generation]))?;
        if changed != 1 {
            return Ok(None);
        }
        let (session, actor): (String, String) = db(tx.query_row(
            "SELECT session_id,actor FROM llm_operations WHERE id=?1",
            [&operation],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ))?;
        let principal = PrincipalId::new(actor)?;
        let txid = tx_id()?;
        commit_event(
            &tx,
            &txid,
            "llm.retry_claimed",
            "llm_operation",
            &operation,
            Some(&principal),
            serde_json::json!({"session_id":session,"client_id":client_id,"generation":generation}),
        )?;
        db(tx.commit())?;
        drop(c);
        Ok(Some(self.durable_operation(&principal, &operation)?))
    }
    pub fn abandon_retry_claim(
        &self,
        actor: &PrincipalId,
        operation: &str,
        client_id: &str,
    ) -> Result<()> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let session: String = db(tx.query_row(
            "SELECT session_id FROM llm_operations WHERE id=?1",
            [operation],
            |r| r.get(0),
        ))?;
        authorize(&tx, &session, actor, true)?;
        let changed=db(tx.execute("UPDATE retry_schedules SET state='scheduled',claimed_by_client=NULL,claim_generation=NULL,updated_at=?1 WHERE operation_id=?2 AND state='claimed' AND claimed_by_client=?3",params![now(),operation,client_id]))?;
        if changed != 1 {
            return Err(Error::Conflict(
                "retry claim is no longer owned by this client",
            ));
        }
        db(tx.commit())?;
        Ok(())
    }
}
