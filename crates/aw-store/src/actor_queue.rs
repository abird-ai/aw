//! AW-P6-9B2: encrypted, immutable-fact, writer-fenced pending Actor Input.
//!
//! This is a *derived service* over the existing Actor Message and SQLite v33
//! audit, not a second mailbox, transcript, effect journal or authority source.
//! A queued item may be admitted to the original Agent only once by the
//! current owner. A crash after its claim is an inspection obligation, never
//! authorization to repeat a possibly paid or effectful Turn.
use crate::{
    actor_admission, actor_control, audit, branch_in, db, principal_is_session_owner, session_in,
    workspace_writer, SecretStore, SessionStore,
};
use aw_agent_core::{
    Error, InvocationIdentity, JournalEntry, PrincipalId, Result, Role, ToolCall, ToolEffect,
    ToolRecord, ToolSourceEvidence, ToolStatus,
};
use getrandom::fill;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use zeroize::Zeroizing;

const ACTION: &str = "actor.queue";
const MAX_EVENTS: usize = 8192;
const MAX_PENDING_PER_RECIPIENT: usize = 32;
const MAX_BODY: usize = 64 * 1024;

/// AW-P6-9D2B1: one immutable disposition per original journaled Turn.
/// This is a derived audit action in existing SQLite v33, not a new queue.
const MODEL_DISPOSITION_ACTION: &str = "actor.model.queue_disposition";
const MAX_MODEL_DISPOSITIONS: usize = 4096;
const MAX_MODEL_EDIT: usize = 4096;

/// AW-P6-10C: canonical, stable original Queue Update/Delete identity.
/// The same Session/branch/Turn hash is used by BOTH the owning Agent and
/// existing SQLite writer; `md_` spelling alone cannot authorize aliases.
/// This preserves the 9D2B1 original hash domain and stored IDs unchanged.
pub fn model_queue_disposition_id(session: &str, branch: &str, turn: u64) -> String {
    let mut hash = Sha256::new();
    hash.update(b"aw-model-queue-disposition-per-turn-v1\0");
    for part in [session.as_bytes(), branch.as_bytes(), &turn.to_be_bytes()] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    format!("md_{:x}", hash.finalize())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelQueueDispositionKind {
    Update,
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelQueueDispositionRequest {
    pub intent_id: String,
    pub originating_input_request_id: String,
    pub originating_turn_id: u64,
    pub session_id: String,
    pub branch_id: String,
    pub recipient_site: String,
    pub sender: String,
    pub writer_epoch: u64,
    pub target_request_id: String,
    pub expected_revision: u64,
    pub kind: ModelQueueDispositionKind,
}
impl ModelQueueDispositionRequest {
    fn validate(&self) -> Result<()> {
        if !token(&self.intent_id, 128)
            || !token(&self.originating_input_request_id, 128)
            || !token(&self.session_id, 128)
            || !token(&self.branch_id, 128)
            || !token(&self.recipient_site, 128)
            || !token(&self.sender, 128)
            || !token(&self.target_request_id, 128)
            || !self.target_request_id.starts_with("mq_")
            || self.originating_turn_id == 0
            || self.expected_revision == 0
            || self.writer_epoch == 0
            || self.intent_id
                != model_queue_disposition_id(
                    &self.session_id,
                    &self.branch_id,
                    self.originating_turn_id,
                )
        {
            return Err(Error::Denied(
                "invalid current model queue disposition authority",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelQueueDispositionFact {
    request: ModelQueueDispositionRequest,
    resulting_revision: u64,
    resulting_state: QueueState,
}
fn model_disposition_events(
    conn: &rusqlite::Connection,
    session: &str,
) -> Result<Vec<ModelQueueDispositionFact>> {
    let mut query = db(conn.prepare(
        "SELECT data_json FROM audit WHERE session_id=?1 AND action=?2 ORDER BY seq LIMIT 4097",
    ))?;
    let rows = db(
        query.query_map(params![session, MODEL_DISPOSITION_ACTION], |row| {
            row.get::<_, String>(0)
        }),
    )?;
    let encoded = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if encoded.len() > MAX_MODEL_DISPOSITIONS {
        return Err(Error::Limit(
            "model queue disposition ledger requires owner inspection",
        ));
    }
    encoded.iter().map(|value| crate::decode(value)).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueTarget {
    Auto,
    Exact(u64),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueState {
    Pending,
    Claimed,
    Accepted,
    Settled,
    Deleted,
    Inspected,
}
impl QueueState {
    pub const fn needs_inspection(self) -> bool {
        matches!(self, Self::Claimed)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorQueueRequest {
    pub request_id: String,
    pub session_id: String,
    pub branch_id: String,
    /// A logical execution site, not an ActorKey or a grant. Authenticated
    /// host binding is checked independently on every delivery attempt.
    pub recipient_site: String,
    pub sender: String,
    pub message_id: u64,
    pub writer_epoch: u64,
    pub target: QueueTarget,
    pub priority: i8,
}
fn token(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-:.@".contains(&b))
}
impl ActorQueueRequest {
    pub fn validate(&self) -> Result<()> {
        if !token(&self.request_id, 128)
            || self.request_id.starts_with("qi_")
            || !token(&self.session_id, 128)
            || !token(&self.branch_id, 128)
            || !token(&self.recipient_site, 128)
            || !token(&self.sender, 128)
            || self.message_id == 0
            || self.writer_epoch == 0
            || !(-20..=19).contains(&self.priority)
            || matches!(self.target, QueueTarget::Exact(0))
        {
            return Err(Error::Denied("invalid bounded Actor queue request"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueClaim {
    /// New authenticated receiver binding, not the original enqueue actor.
    pub recipient_actor: String,
    pub message_id: u64,
    pub turn_id: u64,
    pub writer_epoch: u64,
    pub owner_process_id: Option<String>,
    pub owner_store_generation: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorQueueFact {
    pub request: ActorQueueRequest,
    pub state: QueueState,
    pub revision: u64,
    pub payload_sha256: String,
    /// Opaque encrypted write-once SecretStore item; never plaintext in audit.
    pub payload_ref: String,
    pub claim: Option<QueueClaim>,
    pub note: Option<String>,
}
/// Deliberately reduced data-only projection for an authenticated host
/// query/transport. Never reveal encrypted-item lookup refs, plaintext,
/// unkeyed payload hashes or process-incarnation markers over ActorOperation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorQueueView {
    pub request_id: String,
    pub state: QueueState,
    pub revision: u64,
    pub priority: i8,
    pub target: QueueTarget,
    pub turn_id: Option<u64>,
    pub note: Option<String>,
}
impl From<&ActorQueueFact> for ActorQueueView {
    fn from(f: &ActorQueueFact) -> Self {
        Self {
            request_id: f.request.request_id.clone(),
            state: f.state,
            revision: f.revision,
            priority: f.request.priority,
            target: f.request.target,
            turn_id: f.claim.as_ref().map(|claim| claim.turn_id),
            note: f.note.clone(),
        }
    }
}
/// A private, guarded payload for precisely one claimed receipt; deliberately
/// not Serialize/Debug so a query/CLI cannot accidentally export its body.
pub struct ClaimedQueueInput {
    pub fact: ActorQueueFact,
    pub payload: Zeroizing<String>,
}
/// Same domain-separated deterministic D2A model enqueue identity used by
/// the original owning Service and by D2B provenance checks. A model-created
/// `mq_` prefix WITHOUT this origin Turn proof is never enough to grant an
/// effectful update/delete over the original user's queue.
pub fn model_queue_request_id(session: &str, branch: &str, turn: u64) -> String {
    let mut sha = Sha256::new();
    sha.update(b"aw-model-queue-one-intent-per-turn-v1\0");
    for part in [session.as_bytes(), branch.as_bytes(), &turn.to_be_bytes()] {
        sha.update((part.len() as u64).to_be_bytes());
        sha.update(part);
    }
    format!("mq_{:x}", sha.finalize())
}

/// A stable unique 9A Input correlation for a queue claim. This is *not* an
/// effect/attempt ID and does not authorize retrying an uncertain attempt.
pub fn queued_input_request_id(session: &str, request_id: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"aw-actor-queue-input-id-v1\0");
    hash.update(session.as_bytes());
    hash.update([0]);
    hash.update(request_id.as_bytes());
    format!("qi_{:x}", hash.finalize())
}
fn random_secret_ref(session: &str, request_id: &str) -> Result<String> {
    let mut entropy = [0u8; 32];
    fill(&mut entropy)
        .map_err(|_| Error::Storage("OS randomness unavailable for queue custody"))?;
    let mut hash = Sha256::new();
    hash.update(b"aw-actor-queue-blob-v1\0");
    hash.update(session.as_bytes());
    hash.update([0]);
    hash.update(request_id.as_bytes());
    hash.update(entropy);
    Ok(format!("sec_{:x}", hash.finalize()))
}
fn hash_payload(payload: &[u8]) -> String {
    format!("{:x}", Sha256::digest(payload))
}
pub(crate) fn events(c: &rusqlite::Connection, session: &str) -> Result<Vec<ActorQueueFact>> {
    let mut query = db(c.prepare(
        "SELECT data_json FROM audit WHERE session_id=?1 AND action=?2 ORDER BY seq LIMIT 8193",
    ))?;
    let items = db(query.query_map(params![session, ACTION], |row| row.get::<_, String>(0)))?;
    let encoded = db(items.collect::<rusqlite::Result<Vec<_>>>())?;
    if encoded.len() > MAX_EVENTS {
        return Err(Error::Limit("Actor queue audit requires review"));
    }
    encoded
        .into_iter()
        .map(|value| crate::decode::<ActorQueueFact>(&value))
        .collect()
}
fn latest(items: &[ActorQueueFact]) -> BTreeMap<String, ActorQueueFact> {
    let mut state = BTreeMap::new();
    for item in items {
        state.insert(item.request.request_id.clone(), item.clone());
    }
    state
}
/// Reserve the remaining immutable facts for every already-accepted item.
/// A future claim, native acceptance and factual closure must never fail
/// solely because subsequent new queue requests filled an audit limit.
fn future_facts(all: &[ActorQueueFact]) -> usize {
    latest(all)
        .values()
        .map(|fact| match fact.state {
            QueueState::Pending => 3,  // claimed / ACKed / settled
            QueueState::Claimed => 2,  // ACKed / settled (or inspection)
            QueueState::Accepted => 1, // original Turn factual closure
            QueueState::Settled | QueueState::Deleted | QueueState::Inspected => 0,
        })
        .sum()
}
fn reserve_audit(all: &[ActorQueueFact], next_count: usize, next_reserve: usize) -> Result<()> {
    let total = all
        .len()
        .checked_add(next_count)
        .and_then(|sum| sum.checked_add(next_reserve))
        .ok_or(Error::Limit(
            "Actor queue immutable audit capacity overflow",
        ))?;
    if total > MAX_EVENTS {
        return Err(Error::Limit(
            "Actor queue immutable audit capacity reserved for existing Inputs",
        ));
    }
    Ok(())
}
fn writer(c: &rusqlite::Connection, owner: &PrincipalId, session: &str, epoch: u64) -> Result<()> {
    if !principal_is_session_owner(c, session, owner)? {
        return Err(Error::Denied("queue requires the durable Session owner"));
    }
    let status = workspace_writer::verify_writer_epoch_for_session(c, session, epoch)?;
    if status.state != "active" {
        return Err(Error::Denied("queue writer is no longer active"));
    }
    Ok(())
}
fn audit_fact(c: &rusqlite::Connection, owner: &PrincipalId, fact: &ActorQueueFact) -> Result<()> {
    audit(
        c,
        &fact.request.session_id,
        owner,
        ACTION,
        serde_json::to_value(fact)
            .map_err(|_| Error::Storage("cannot serialize Actor queue fact"))?,
    )
}
fn collision(c: &rusqlite::Connection, session: &str, request_id: &str) -> Result<bool> {
    Ok(crate::actor_notice::events(c, session)?
        .iter()
        .any(|x| x.request.request_id == request_id)
        || actor_admission::events(c, session)?
            .iter()
            .any(|x| x.admission.request_id == request_id)
        || actor_control::events(c, session)?
            .iter()
            .any(|x| x.admission.request_id == request_id))
}
fn max_sequence(
    c: &rusqlite::Connection,
    session: &str,
    sender: &str,
    qs: &[ActorQueueFact],
) -> Result<u64> {
    let mut max = qs
        .iter()
        .filter(|x| x.request.sender == sender)
        .map(|x| x.request.message_id)
        .max()
        .unwrap_or(0);
    for x in qs.iter().filter_map(|x| x.claim.as_ref()) {
        if x.recipient_actor == sender {
            max = max.max(x.message_id);
        }
    }
    for x in actor_admission::events(c, session)? {
        if x.admission.sender == sender {
            max = max.max(x.admission.message_id);
        }
    }
    for x in actor_control::events(c, session)? {
        if x.admission.sender == sender {
            max = max.max(x.admission.message_id);
        }
    }
    for x in crate::actor_notice::events(c, session)? {
        if x.request.sender == sender {
            max = max.max(x.request.message_id);
        }
    }
    Ok(max)
}
fn same_recipient(fact: &ActorQueueFact, branch: &str, site: &str) -> bool {
    fact.request.branch_id == branch && fact.request.recipient_site == site
}
fn safe_to_claim(c: &rusqlite::Connection, session: &str, branch: &str) -> Result<u64> {
    let current = session_in(c, session)?;
    let current_branch = branch_in(c, session, branch)?;
    if current_branch.active_run.is_some() || current.next_turn == 0 {
        return Err(Error::Conflict("queue receiver has an active Turn"));
    }
    let hazardous: Option<i64> = db(c.query_row(
        "SELECT 1 FROM hazards WHERE session_id=?1 AND state IN ('running','needs_inspection') LIMIT 1",
        params![session], |r| r.get(0),
    ).optional())?;
    if hazardous.is_some() {
        return Err(Error::Conflict(
            "queue receiver has unresolved effect hazards",
        ));
    }
    let mut previous = BTreeMap::new();
    for fact in actor_admission::events(c, session)? {
        previous.insert(fact.admission.request_id.clone(), fact.state);
    }
    if previous.values().any(|state| state.needs_inspection()) {
        return Err(Error::Conflict(
            "unsettled Actor Input forbids queue delivery",
        ));
    }
    let mut controls = BTreeMap::new();
    for fact in actor_control::events(c, session)? {
        controls.insert(fact.admission.request_id.clone(), fact.state);
    }
    if controls.values().any(|state| state.needs_inspection()) {
        return Err(Error::Conflict(
            "uncertain Actor Control forbids queue delivery",
        ));
    }
    Ok(current.next_turn)
}
impl SessionStore {
    /// Persist encrypted custody BEFORE an immutable Pending fact. Only an
    /// existing on-disk private owner store is supported; minimal/stateless
    /// and in-memory stores cannot claim restart durability.
    pub fn actor_queue_enqueue(
        &self,
        owner: &PrincipalId,
        request: ActorQueueRequest,
        payload: &str,
    ) -> Result<ActorQueueFact> {
        self.actor_queue_enqueue_inner(owner, request, payload, false)
    }

    /// P6-9D2A: original journaled Agent's explicitly Host/Write-granted
    /// model-origin Input admission. Only the *current SQLite writer* mints
    /// source_sequence within the SAME BEGIN IMMEDIATE collision transaction,
    /// after checking every historical original Input/Control/Queue/Notify
    /// sender sequence. The caller supplies message_id=0 (not authority),
    /// and a deterministic once-per-originating-Turn request ID; no model
    /// supplied number or actor generation can reserve a future sequence.
    pub fn actor_queue_enqueue_minted(
        &self,
        owner: &PrincipalId,
        request: ActorQueueRequest,
        payload: &str,
    ) -> Result<ActorQueueFact> {
        if request.message_id != 0 {
            return Err(Error::Denied(
                "host-minted queued Input rejects caller-supplied message sequence",
            ));
        }
        self.actor_queue_enqueue_inner(owner, request, payload, true)
    }

    fn actor_queue_enqueue_inner(
        &self,
        owner: &PrincipalId,
        mut request: ActorQueueRequest,
        payload: &str,
        mint_sequence: bool,
    ) -> Result<ActorQueueFact> {
        if mint_sequence {
            request.message_id = 1;
        }
        request.validate()?;
        if payload.is_empty() || payload.len() > MAX_BODY || payload.contains('\0') {
            return Err(Error::Limit(
                "Actor queued text must contain 1..65536 safe bytes",
            ));
        }
        self.actor_input_writer_fence(owner, &request.session_id, request.writer_epoch)?;
        let vault = self.local_secret_store()?;
        let blob_ref = random_secret_ref(&request.session_id, &request.request_id)?;
        vault.put(&blob_ref, payload.as_bytes())?;
        let result = (|| {
            let mut conn = self.connection()?;
            let tx = db(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
            writer(&tx, owner, &request.session_id, request.writer_epoch)?;
            branch_in(&tx, &request.session_id, &request.branch_id)?;
            let all = events(&tx, &request.session_id)?;
            if all
                .iter()
                .any(|item| item.request.request_id == request.request_id)
                || collision(&tx, &request.session_id, &request.request_id)?
            {
                return Err(Error::Conflict("Actor request ID already used"));
            }
            let current = latest(&all);
            if current
                .values()
                .filter(|f| {
                    same_recipient(f, &request.branch_id, &request.recipient_site)
                        && matches!(f.state, QueueState::Pending | QueueState::Claimed)
                })
                .count()
                >= MAX_PENDING_PER_RECIPIENT
            {
                return Err(Error::Limit("Actor pending queue capacity reached"));
            }
            reserve_audit(&all, 1, future_facts(&all) + 3)?;
            let last_sequence = max_sequence(&tx, &request.session_id, &request.sender, &all)?;
            if mint_sequence {
                request.message_id = last_sequence
                    .checked_add(1)
                    .ok_or(Error::Limit("Actor model queued sender sequence exhausted"))?;
            } else if request.message_id <= last_sequence {
                return Err(Error::Denied(
                    "Actor queued source sequence is stale or duplicated",
                ));
            }
            let fact = ActorQueueFact {
                request,
                state: QueueState::Pending,
                revision: 1,
                payload_sha256: hash_payload(payload.as_bytes()),
                payload_ref: blob_ref.clone(),
                claim: None,
                note: None,
            };
            audit_fact(&tx, owner, &fact)?;
            db(tx.commit())?;
            Ok(fact)
        })();
        if result.is_err() {
            // Unique random custody ref, never delete an older accepted blob.
            let _ = vault.delete(&blob_ref);
        }
        result
    }

    /// Pending edit is a NEW encrypted write-once blob and an immutable
    /// revision with SQLite writer CAS. It cannot retarget a recipient,
    /// change the message identity/Turn or rewrite a claimed Input. Stale
    /// revision and duplicate edit attempts are rejected without changing
    /// the existing accepted fact or payload.
    pub fn actor_queue_update(
        &self,
        owner: &PrincipalId,
        session: &str,
        request_id: &str,
        expected_epoch: u64,
        expected_revision: u64,
        payload: &str,
        priority: i8,
    ) -> Result<ActorQueueFact> {
        if payload.is_empty()
            || payload.len() > MAX_BODY
            || payload.contains('\0')
            || !(-20..=19).contains(&priority)
        {
            return Err(Error::Limit(
                "Actor queued edit must be bounded, safe text and priority",
            ));
        }
        self.actor_input_writer_fence(owner, session, expected_epoch)?;
        let vault = self.local_secret_store()?;
        let new_ref = random_secret_ref(session, request_id)?;
        vault.put(&new_ref, payload.as_bytes())?;
        let result = (|| {
            let mut conn = self.connection()?;
            let tx = db(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
            writer(&tx, owner, session, expected_epoch)?;
            let all = events(&tx, session)?;
            let old = latest(&all)
                .remove(request_id)
                .ok_or(Error::Denied("queue item does not exist"))?;
            if old.state != QueueState::Pending || old.revision != expected_revision {
                return Err(Error::Conflict(
                    "queue edit requires exact pending revision",
                ));
            }
            reserve_audit(&all, 1, future_facts(&all))?;
            let mut next = old.clone();
            next.revision += 1;
            next.payload_ref = new_ref.clone();
            next.payload_sha256 = hash_payload(payload.as_bytes());
            next.request.priority = priority;
            audit_fact(&tx, owner, &next)?;
            db(tx.commit())?;
            Ok((next, old.payload_ref))
        })();
        match result {
            Ok((updated, old_ref)) => {
                vault.delete(&old_ref)?;
                Ok(updated)
            }
            Err(error) => {
                let _ = vault.delete(&new_ref);
                Err(error)
            }
        }
    }

    /// AW-P6-10D: a matching `mq_` hash plus a Settled Input is NOT proof
    /// that the original model actually requested and received a successful
    /// `queue_enqueue` Tool receipt. Inspect the ONE existing immutable
    /// TurnJournal in the SAME SQLite writer transaction as disposition.
    /// Never copy provider-private data, model arguments or queue ciphertext
    /// into the disposition/queue audit. No second journal/provenance table.
    fn original_model_enqueue_receipt(
        conn: &rusqlite::Connection,
        owner: &PrincipalId,
        origin: &actor_admission::ActorInputFact,
        initial: &ActorQueueFact,
    ) -> Result<bool> {
        let session = &origin.admission.session_id;
        let branch = &origin.admission.branch_id;
        let mut runs = db(conn.prepare(
            "SELECT id,identity_json FROM runs
             WHERE session_id=?1 AND branch_id=?2 AND turn_id=?3
               AND produced_from_writer_epoch=?4
               AND status='completed' AND attach_status='attached'
             ORDER BY id LIMIT 2",
        ))?;
        let matches = db(runs.query_map(
            params![
                session,
                branch,
                origin.admission.turn_id,
                origin.admission.writer_epoch
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ))?;
        let matches = db(matches.collect::<rusqlite::Result<Vec<_>>>())?;
        if matches.len() != 1 {
            return Ok(false);
        }
        let (run_id, identity) = &matches[0];
        let identity: InvocationIdentity = crate::decode(identity)?;
        if identity.actor != *owner {
            return Ok(false);
        }
        let mut query =
            db(conn
                .prepare("SELECT payload FROM run_events WHERE run_id=?1 ORDER BY seq LIMIT 513"))?;
        let entries = db(query.query_map([run_id], |row| row.get::<_, String>(0)))?;
        let mut invocation_started = BTreeMap::<u32, InvocationIdentity>::new();
        let mut model_reply_received = false;
        let mut proposals = BTreeMap::<String, ToolCall>::new();
        let mut revisions = BTreeMap::<String, ToolCall>::new();
        let mut started = BTreeMap::<String, ToolRecord>::new();
        let mut matched_receipts = 0u32;
        let mut count = 0;
        for encoded in entries {
            count += 1;
            if count > 512 {
                return Err(Error::Limit(
                    "original model ToolJournal requires owner inspection",
                ));
            }
            let entry: JournalEntry = crate::decode(&db(encoded)?)?;
            match entry {
                JournalEntry::InvocationStarted { ordinal, identity }
                    if identity.actor == *owner =>
                {
                    invocation_started.insert(ordinal, identity);
                }
                JournalEntry::InvocationFinished { record } => {
                    model_reply_received = record.outcome == "response_received"
                        && record.meta.is_some()
                        && invocation_started.get(&record.ordinal) == Some(&record.identity);
                }
                JournalEntry::Assistant { message } => {
                    if message.role == Role::Assistant && model_reply_received {
                        for call in message.tool_calls {
                            if call.name == "queue_enqueue" {
                                proposals.insert(call.id.clone(), call);
                            }
                        }
                    }
                    model_reply_received = false;
                }
                JournalEntry::ToolRevised {
                    original,
                    revised,
                    source_id,
                    source_kind,
                    source_epoch,
                    effect,
                } if proposals.get(&original.id) == Some(&original)
                    && revised.name == "queue_enqueue"
                    && revised.id == original.id
                    && effect == ToolEffect::Write
                    && source_kind == "host"
                    && source_id == "trusted-host"
                    && source_epoch == 1 =>
                {
                    revisions.insert(original.id, revised);
                }
                JournalEntry::ToolStarted { record, source } => {
                    // Exact original Host/trusted-host/epoch1 provenance is
                    // recorded by the SAME trusted Agent/ToolRegistry before
                    // effect dispatch. Legacy/unattributed or foreign intents
                    // remain readable but cannot claim model-owned Queue.
                    if source.as_ref() != Some(&ToolSourceEvidence::trusted_host()) {
                        continue;
                    }
                    if record.call.name != "queue_enqueue"
                        || record.effect != ToolEffect::Write
                        || record.status != ToolStatus::Running
                        || record.execution_site != initial.request.recipient_site
                    {
                        continue;
                    }
                    let proposed = proposals.get(&record.call.id) == Some(&record.call)
                        || revisions.get(&record.call.id) == Some(&record.call);
                    let args = record.call.arguments.as_object();
                    let text = args.and_then(|args| {
                        (args.len() == 2
                            && args.get("op").and_then(|v| v.as_str()) == Some("enqueue"))
                        .then(|| args.get("text").and_then(|v| v.as_str()))
                        .flatten()
                    });
                    if proposed
                        && text
                            .is_some_and(|t| hash_payload(t.as_bytes()) == initial.payload_sha256)
                    {
                        started.insert(record.call.id.clone(), record);
                    }
                }
                JournalEntry::ToolFinished { record } => {
                    let started_matches = started.get(&record.call.id).is_some_and(|intent| {
                        intent.call == record.call
                            && intent.effect == record.effect
                            && intent.execution_site == record.execution_site
                    });
                    if !started_matches || record.status != ToolStatus::Succeeded {
                        continue;
                    }
                    let Some(output) = &record.output else {
                        continue;
                    };
                    if output.is_error || output.truncated {
                        continue;
                    }
                    let Ok(output) = serde_json::from_str::<serde_json::Value>(&output.content)
                    else {
                        continue;
                    };
                    if output.get("kind").and_then(|v| v.as_str()) == Some("queued_future_input")
                        && output.get("request_id").and_then(|v| v.as_str())
                            == Some(&initial.request.request_id)
                        && output.get("state").and_then(|v| v.as_str()) == Some("pending")
                        && output.get("revision").and_then(|v| v.as_u64()) == Some(1)
                        && output.get("target").and_then(|v| v.as_str()) == Some("auto")
                        && output.get("priority").and_then(|v| v.as_i64()) == Some(0)
                        && output.get("execution").and_then(|v| v.as_str())
                            == Some("NOT_DISPATCHED")
                        && output.get("retry").and_then(|v| v.as_str()) == Some("NEVER_AUTOMATIC")
                    {
                        matched_receipts += 1;
                    }
                }
                _ => {}
            }
        }
        Ok(matched_receipts == 1)
    }

    /// Original actor queue UPDATE/DELETE under exactly one original
    /// active journaled model Turn, without a model-controlled sender/target.
    /// BOTH the queue fact and the stable per-Turn disposition audit marker
    /// commit atomically in the SAME original SQLite v33 BEGIN IMMEDIATE.
    /// The model may only edit an *unclaimed* prior model-created `mq_` item
    /// from this same original sender/branch/site, at an exact revision.
    /// No standalone call may retry after an uncertain postcommit unlink.
    pub fn actor_queue_model_disposition(
        &self,
        owner: &PrincipalId,
        request: ModelQueueDispositionRequest,
        replacement: Option<&str>,
    ) -> Result<ActorQueueView> {
        request.validate()?;
        match (request.kind, replacement) {
            (ModelQueueDispositionKind::Update, Some(text))
                if !text.is_empty()
                    && text.len() <= MAX_MODEL_EDIT
                    && !text.contains('\0')
                    && !text
                        .chars()
                        .any(|c| c.is_control() && c != '\n' && c != '\t') => {}
            (ModelQueueDispositionKind::Delete, None) => {}
            _ => {
                return Err(Error::Denied(
                    "model queue disposition requires exact update text or deletion-only intent",
                ))
            }
        }
        self.actor_input_writer_fence(owner, &request.session_id, request.writer_epoch)?;
        // The original SecretStore encrypts before SQLite acceptance, as in
        // the existing queue UPDATE. Failed precommit attempts best-effort
        // unlink only their unique newly created blob.
        let vault = self.local_secret_store()?;
        let new_ref = if replacement.is_some() {
            let reference = random_secret_ref(&request.session_id, &request.intent_id)?;
            vault.put(&reference, replacement.unwrap_or_default().as_bytes())?;
            Some(reference)
        } else {
            None
        };
        let outcome = (|| -> Result<(ActorQueueView, String)> {
            let mut conn = self.connection()?;
            let tx = db(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
            writer(&tx, owner, &request.session_id, request.writer_epoch)?;
            branch_in(&tx, &request.session_id, &request.branch_id)?;
            // No caller-supplied ToolContext can substitute for the latest
            // ORIGINAL immutable Input Dispatching fact during this CAS.
            let origin = actor_admission::events(&tx, &request.session_id)?
                .into_iter()
                .rev()
                .find(|fact| fact.admission.request_id == request.originating_input_request_id)
                .ok_or(Error::Denied(
                    "model disposition has no original Input admission",
                ))?;
            if origin.state != actor_admission::InputAdmissionState::Dispatching
                || origin.admission.session_id != request.session_id
                || origin.admission.branch_id != request.branch_id
                || origin.admission.writer_epoch != request.writer_epoch
                || origin.admission.turn_id != request.originating_turn_id
                || origin.admission.sender != request.sender
                || origin.admission.target != request.sender
            {
                return Err(Error::Denied(
                    "model disposition must match original running Input",
                ));
            }
            // A string beginning `mq_` is NOT provenance. The target must
            // literally be the D2A deterministic receipt of an EARLIER,
            // factually settled original owning Agent Turn in this Session,
            // branch and exact same Actor generation. A forged queued item
            // with a model-looking ID cannot be modified as model-owned.
            let mut historical = BTreeMap::new();
            for fact in actor_admission::events(&tx, &request.session_id)? {
                historical.insert(fact.admission.request_id.clone(), fact);
            }
            let original_origin = historical
                .values()
                .find(|fact| {
                    fact.state == actor_admission::InputAdmissionState::Settled
                        && fact.admission.turn_id < request.originating_turn_id
                        && fact.admission.session_id == request.session_id
                        && fact.admission.branch_id == request.branch_id
                        && fact.admission.sender == request.sender
                        && fact.admission.target == request.sender
                        && model_queue_request_id(
                            &request.session_id,
                            &request.branch_id,
                            fact.admission.turn_id,
                        ) == request.target_request_id
                })
                .ok_or(Error::Denied(
                    "model queue target lacks settled original model-enqueue Turn provenance",
                ))?;
            let dispositions = model_disposition_events(&tx, &request.session_id)?;
            if dispositions.len() >= MAX_MODEL_DISPOSITIONS
                || dispositions.iter().any(|fact| {
                    fact.request.intent_id == request.intent_id
                        || (fact.request.branch_id == request.branch_id
                            && fact.request.originating_turn_id == request.originating_turn_id)
                })
            {
                return Err(Error::Conflict(
                    "original Turn already committed one model queue disposition",
                ));
            }
            let all = events(&tx, &request.session_id)?;
            let old = latest(&all)
                .remove(&request.target_request_id)
                .ok_or(Error::Denied("model queue target does not exist"))?;
            if old.request.branch_id != request.branch_id
                || old.request.recipient_site != request.recipient_site
                || old.request.sender != request.sender
                || old.state != QueueState::Pending
                || old.revision != request.expected_revision
            {
                return Err(Error::Conflict(
                    "model queue target must be same owner, site, and exact Pending revision",
                ));
            }
            // The FIRST immutable queue fact is the Tool's original encrypted
            // Pending payload, even if a later owner edited its ciphertext.
            // Verify actual original journal evidence atomically with the new
            // model disposition; the old mq_ name alone proves nothing.
            let initial = all
                .iter()
                .find(|fact| fact.request.request_id == old.request.request_id)
                .ok_or(Error::Denied("original model queue creation fact missing"))?;
            if initial.state != QueueState::Pending
                || initial.revision != 1
                || initial.request.target != QueueTarget::Auto
                || initial.request.priority != 0
                || initial.request.branch_id != old.request.branch_id
                || initial.request.recipient_site != old.request.recipient_site
                || initial.request.sender != old.request.sender
                || initial.request.message_id != old.request.message_id
                || !Self::original_model_enqueue_receipt(&tx, owner, original_origin, initial)?
            {
                return Err(Error::Denied(
                    "model queue disposition lacks original successful queue_enqueue Tool receipt",
                ));
            }
            let revision = old
                .revision
                .checked_add(1)
                .ok_or(Error::Limit("model queue target revision exhausted"))?;
            // Queue service still reserves all claims/acceptance/closure facts.
            // One separately immutable disposition audit must also fit.
            reserve_audit(&all, 1, future_facts(&all))?;
            let mut next = old.clone();
            next.revision = revision;
            match request.kind {
                ModelQueueDispositionKind::Update => {
                    let body = replacement.ok_or(Error::Denied("missing model update text"))?;
                    next.payload_ref = new_ref
                        .as_ref()
                        .ok_or(Error::Storage("missing new encrypted disposition ref"))?
                        .clone();
                    next.payload_sha256 = hash_payload(body.as_bytes());
                    // No model-supplied priority/recipient/Target/sequence.
                }
                ModelQueueDispositionKind::Delete => next.state = QueueState::Deleted,
            }
            audit_fact(&tx, owner, &next)?;
            audit(
                &tx,
                &request.session_id,
                owner,
                MODEL_DISPOSITION_ACTION,
                serde_json::to_value(ModelQueueDispositionFact {
                    request: request.clone(),
                    resulting_revision: revision,
                    resulting_state: next.state,
                })
                .map_err(|_| {
                    Error::Storage("cannot serialize bounded model disposition receipt")
                })?,
            )?;
            db(tx.commit())?;
            Ok((ActorQueueView::from(&next), old.payload_ref))
        })();
        match outcome {
            Ok((view, retired)) => {
                // Postcommit unlink errors represent UNCERTAINTY, not a
                // failed no-effect disposition. The original Tool journal
                // must require inspection and MUST NOT repeat the request.
                vault.delete(&retired)?;
                Ok(view)
            }
            Err(error) => {
                if let Some(reference) = new_ref {
                    let _ = vault.delete(&reference);
                }
                Err(error)
            }
        }
    }

    /// An owner-only metadata snapshot never returns plaintext. Even the
    /// encrypted custody reference is not an authorization to read it.
    pub fn actor_queue_list(
        &self,
        owner: &PrincipalId,
        session: &str,
    ) -> Result<Vec<ActorQueueFact>> {
        let conn = self.connection()?;
        if !principal_is_session_owner(&conn, session, owner)? {
            return Err(Error::Denied(
                "Actor queue metadata belongs to Session owner",
            ));
        }
        Ok(latest(&events(&conn, session)?).into_values().collect())
    }
    pub fn actor_queue_fact(
        &self,
        owner: &PrincipalId,
        session: &str,
        id: &str,
    ) -> Result<ActorQueueFact> {
        self.actor_queue_list(owner, session)?
            .into_iter()
            .find(|f| f.request.request_id == id)
            .ok_or(Error::Denied("Actor queue request not found"))
    }

    /// Select under BEGIN IMMEDIATE, original-mailbox priority and stable
    /// acceptance ordering, with a four-urgent fairness cap. Only an unclaimed
    /// Pending item is recoverable across restarts; a Claimed item is never
    /// replayable even if its corresponding Input receipt was lost.
    pub fn actor_queue_claim_next(
        &self,
        owner: &PrincipalId,
        session: &str,
        branch: &str,
        site: &str,
        recipient_actor: &str,
        writer_epoch: u64,
    ) -> Result<Option<ClaimedQueueInput>> {
        if !token(site, 128) || !token(recipient_actor, 128) {
            return Err(Error::Denied("invalid bound queue recipient"));
        }
        let process = self
            .process_identity()?
            .ok_or(Error::Denied("durable queue requires a real owner process"))?;
        let vault = self.local_secret_store()?;
        let mut conn = self.connection()?;
        let tx = db(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
        writer(&tx, owner, session, writer_epoch)?;
        // Model-origin future Agent pause is checked inside the ORIGINAL
        // queue Claim transaction, before custody/ref decryption or claim
        // append. A rejected attempt must leave Pending intact, including
        // after process restart or an uncertain original paid Turn.
        crate::model_agent_pause::require_future_admission(&tx, session, branch)?;
        let turn = safe_to_claim(&tx, session, branch)?;
        let all = events(&tx, session)?;
        let state = latest(&all);
        if state
            .values()
            .any(|f| same_recipient(f, branch, site) && f.state.needs_inspection())
        {
            return Err(Error::Conflict(
                "claimed queue item requires inspection, not replay",
            ));
        }
        let mut pending: Vec<&ActorQueueFact> = state
            .values()
            .filter(|f| same_recipient(f, branch, site) && f.state == QueueState::Pending)
            .collect();
        if pending.is_empty() {
            return Ok(None);
        }
        // Find original enqueue sequence from immutable audit, never from
        // lexical client ID or a mutable timestamps field.
        let order = |f: &ActorQueueFact| {
            all.iter()
                .position(|event| event.request.request_id == f.request.request_id)
                .unwrap_or(usize::MAX)
        };
        if pending
            .iter()
            .any(|f| matches!(f.request.target,QueueTarget::Exact(t) if t < turn))
        {
            return Err(Error::Conflict(
                "stale explicit Turn needs pending queue deletion",
            ));
        }
        pending.retain(|f| {
            matches!(f.request.target, QueueTarget::Auto)
                || matches!(f.request.target, QueueTarget::Exact(t) if t==turn)
        });
        if pending.is_empty() {
            return Ok(None);
        }
        let last_claims: Vec<_> = all
            .iter()
            .rev()
            .filter(|f| f.state == QueueState::Claimed && same_recipient(f, branch, site))
            .take(4)
            .collect();
        let favor_normal = last_claims.len() == 4
            && last_claims.iter().all(|f| f.request.priority < 0)
            && pending.iter().any(|f| f.request.priority >= 0);
        if favor_normal {
            pending.retain(|f| f.request.priority >= 0);
        }
        pending.sort_by_key(|f| (f.request.priority, order(f)));
        let selected = (*pending[0]).clone();
        // A fresh Runtime incarnation may rebind a pending Input only when
        // the SAME current writer epoch still holds. Failover/revocation
        // requires an explicit new owner request, not an implicit transfer.
        if selected.request.writer_epoch != writer_epoch {
            return Err(Error::Conflict(
                "queued Input belonged to a different writer epoch; inspect before reauthorization",
            ));
        }
        let bytes = vault.get(&selected.payload_ref)?;
        if hash_payload(bytes.as_slice()) != selected.payload_sha256 {
            return Err(Error::Denied("queued private payload digest mismatch"));
        }
        let payload = Zeroizing::new(
            String::from_utf8(bytes.to_vec())
                .map_err(|_| Error::Denied("queued private payload is not UTF-8"))?,
        );
        let next_id = max_sequence(&tx, session, recipient_actor, &all)?
            .checked_add(1)
            .ok_or(Error::Limit("Actor sender sequence exhausted"))?;
        let claim = QueueClaim {
            recipient_actor: recipient_actor.to_owned(),
            message_id: next_id,
            turn_id: turn,
            writer_epoch,
            owner_process_id: Some(process.process_id),
            owner_store_generation: Some(process.store_generation),
        };
        let fact = ActorQueueFact {
            state: QueueState::Claimed,
            revision: selected.revision + 1,
            claim: Some(claim),
            ..selected
        };
        audit_fact(&tx, owner, &fact)?;
        db(tx.commit())?;
        Ok(Some(ClaimedQueueInput { fact, payload }))
    }

    /// Confirm only a real 9A Accepted original Actor Input at the same
    /// recipient, exact Turn, SHA, source sequence and writer epoch. This is
    /// a queue-delivery ACK, NEVER proof of provider/tool/effect settlement.
    pub fn actor_queue_ack(
        &self,
        owner: &PrincipalId,
        expected: &ActorQueueFact,
    ) -> Result<ActorQueueFact> {
        if expected.state != QueueState::Claimed {
            return Err(Error::Denied("queue ACK needs exact claim"));
        }
        let claim = expected
            .claim
            .as_ref()
            .ok_or(Error::Denied("queue claim missing"))?;
        let process = self
            .process_identity()?
            .ok_or(Error::Denied("queue ACK needs current owner process"))?;
        if claim.owner_process_id.as_deref() != Some(process.process_id.as_str())
            || claim.owner_store_generation != Some(process.store_generation)
        {
            return Err(Error::Conflict(
                "restarted queue owner cannot ACK previous delivery",
            ));
        }
        let mut conn = self.connection()?;
        let tx = db(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
        writer(&tx, owner, &expected.request.session_id, claim.writer_epoch)?;
        let all = events(&tx, &expected.request.session_id)?;
        if latest(&all).get(&expected.request.request_id) != Some(expected) {
            return Err(Error::Conflict("queue ACK claim CAS changed"));
        }
        let input_id =
            queued_input_request_id(&expected.request.session_id, &expected.request.request_id);
        let accepted = actor_admission::events(&tx, &expected.request.session_id)?
            .into_iter()
            .rev()
            .find(|x| x.admission.request_id == input_id)
            .ok_or(Error::Denied("queue has no original accepted Actor Input"))?;
        if accepted.state != actor_admission::InputAdmissionState::Accepted
            || accepted.admission.session_id != expected.request.session_id
            || accepted.admission.branch_id != expected.request.branch_id
            || accepted.admission.writer_epoch != claim.writer_epoch
            || accepted.admission.turn_id != claim.turn_id
            || accepted.admission.message_id != claim.message_id
            || accepted.admission.sender != claim.recipient_actor
            || accepted.admission.target != claim.recipient_actor
            || accepted.admission.payload_sha256 != expected.payload_sha256
            || accepted.admission.priority != expected.request.priority
        {
            return Err(Error::Denied(
                "queue ACK does not match actual accepted Actor Input",
            ));
        }
        let fact = ActorQueueFact {
            state: QueueState::Accepted,
            revision: expected.revision + 1,
            ..expected.clone()
        };
        audit_fact(&tx, owner, &fact)?;
        db(tx.commit())?;
        Ok(fact)
    }

    /// After the ORIGINAL 9A admission has a terminal factual Turn receipt,
    /// retire the opaque queue ACK and release private encrypted payload
    /// custody. `Settled` here says only that the existing Turn journal was
    /// checked, not that every external effect was successful or reversed.
    pub fn actor_queue_finalize(
        &self,
        owner: &PrincipalId,
        input: &crate::ActorInputAdmission,
    ) -> Result<Option<ActorQueueFact>> {
        let mut conn = self.connection()?;
        let tx = db(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
        writer(&tx, owner, &input.session_id, input.writer_epoch)?;
        let all = events(&tx, &input.session_id)?;
        let Some(old) = latest(&all).into_values().find(|f| {
            queued_input_request_id(&input.session_id, &f.request.request_id) == input.request_id
        }) else {
            return Ok(None);
        };
        let process = self
            .process_identity()?
            .ok_or(Error::Denied("queue finalize needs owner process"))?;
        let claim = old
            .claim
            .as_ref()
            .ok_or(Error::Denied("final queue fact lost claim"))?;
        if old.state != QueueState::Accepted
            || claim.owner_process_id.as_deref() != Some(process.process_id.as_str())
            || claim.owner_store_generation != Some(process.store_generation)
            || claim.writer_epoch != input.writer_epoch
            || claim.turn_id != input.turn_id
            || claim.message_id != input.message_id
            || claim.recipient_actor != input.target
            || claim.recipient_actor != input.sender
            || old.payload_sha256 != input.payload_sha256
            || old.request.priority != input.priority
            || old.request.branch_id != input.branch_id
        {
            return Err(Error::Conflict(
                "queue finalization does not match original claim",
            ));
        }
        let input_fact = actor_admission::events(&tx, &input.session_id)?
            .into_iter()
            .rev()
            .find(|x| x.admission.request_id == input.request_id)
            .ok_or(Error::Denied("queue lacks original settled Input fact"))?;
        if input_fact.admission != *input
            || input_fact.state != actor_admission::InputAdmissionState::Settled
        {
            return Err(Error::Denied(
                "queue cannot finalize before factual settled Input",
            ));
        }
        let fact = ActorQueueFact {
            state: QueueState::Settled,
            revision: old.revision + 1,
            ..old
        };
        audit_fact(&tx, owner, &fact)?;
        db(tx.commit())?;
        self.local_secret_store()?.delete(&fact.payload_ref)?;
        Ok(Some(fact))
    }

    /// Pending deletion is a no-effect disposition: it is impossible after
    /// claim, and never erases separate accepted Input / paid receipts.
    pub fn actor_queue_delete(
        &self,
        owner: &PrincipalId,
        session: &str,
        id: &str,
        writer_epoch: u64,
    ) -> Result<ActorQueueFact> {
        let mut conn = self.connection()?;
        let tx = db(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
        writer(&tx, owner, session, writer_epoch)?;
        let old = latest(&events(&tx, session)?)
            .remove(id)
            .ok_or(Error::Denied("queued request does not exist"))?;
        if old.state != QueueState::Pending {
            return Err(Error::Conflict("only pending queue can be deleted"));
        }
        let fact = ActorQueueFact {
            state: QueueState::Deleted,
            revision: old.revision + 1,
            ..old
        };
        audit_fact(&tx, owner, &fact)?;
        db(tx.commit())?;
        // Unlink the custody item after the immutable deletion; a failed
        // unlink is reported, never interpreted as successful erasure.
        self.local_secret_store()?.delete(&fact.payload_ref)?;
        Ok(fact)
    }
    pub fn actor_queue_inspect(
        &self,
        owner: &PrincipalId,
        session: &str,
        id: &str,
        writer_epoch: u64,
        note: &str,
    ) -> Result<ActorQueueFact> {
        if note.trim().is_empty() || note.len() > 4096 || note.chars().any(char::is_control) {
            return Err(Error::Denied(
                "queue inspection note must be bounded and printable",
            ));
        }
        let mut conn = self.connection()?;
        let tx = db(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
        writer(&tx, owner, session, writer_epoch)?;
        let old = latest(&events(&tx, session)?)
            .remove(id)
            .ok_or(Error::Denied("queue claim not found"))?;
        if !matches!(old.state, QueueState::Claimed | QueueState::Accepted) {
            return Err(Error::Conflict(
                "only uncertain queue delivery needs inspection",
            ));
        }
        if old.state == QueueState::Accepted {
            let input_id = queued_input_request_id(session, &old.request.request_id);
            let independent = actor_admission::events(&tx, session)?
                .into_iter()
                .rev()
                .find(|item| item.admission.request_id == input_id)
                .ok_or(Error::Denied("accepted queue lost original Input fact"))?;
            if independent.state != actor_admission::InputAdmissionState::Inspected {
                return Err(Error::Conflict(
                    "inspect the independently uncertain original Agent Input before retiring its queue receipt",
                ));
            }
        }
        let fact = ActorQueueFact {
            state: QueueState::Inspected,
            revision: old.revision + 1,
            note: Some(note.to_owned()),
            ..old
        };
        audit_fact(&tx, owner, &fact)?;
        db(tx.commit())?;
        // An explicit inspection retires the unexecutable private queue
        // payload, but never clears independent 9A or paid/tool hazards.
        self.local_secret_store()?.delete(&fact.payload_ref)?;
        Ok(fact)
    }
}
