//! AW-P6-9B2B: authenticated, immutable-fact custody of observational Notify.
//!
//! A notification is never an Input/Control/Hook, provider or effect receipt.
//! Prepared is NOT delivery. Accepted records only a real in-memory Runtime
//! mailbox acceptance, not user observation; a crash before that ACK is
//! Unknown and NEVER re-enqueued by this store. Accepted private observations
//! may be read after restart under a CURRENT Session owner/writer without
//! reviving their original sender/Actor generation.
use crate::{
    actor_admission, actor_control, actor_queue, audit, branch_in, db, principal_is_session_owner,
    workspace_writer, QueueTarget, SecretStore, SessionStore,
};
use aw_agent_core::{Error, PrincipalId, Result};
use getrandom::fill;
use rusqlite::{params, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use zeroize::Zeroizing;

const ACTION: &str = "actor.notice";
const MAX_EVENTS: usize = 8192;
const MAX_LIVE: usize = 32;
const MAX_BODY: usize = 64 * 1024;

fn token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-:.@".contains(&b))
}
fn payload_digest(body: &[u8]) -> String {
    format!("{:x}", Sha256::digest(body))
}
fn new_blob_ref(session: &str, request: &str) -> Result<String> {
    let mut entropy = [0u8; 32];
    fill(&mut entropy).map_err(|_| Error::Storage("OS randomness unavailable for Notify"))?;
    let mut h = Sha256::new();
    h.update(b"aw-actor-notice-blob-v1\0");
    h.update(session.as_bytes());
    h.update([0]);
    h.update(request.as_bytes());
    h.update(entropy);
    Ok(format!("sec_{:x}", h.finalize()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorNoticeState {
    Prepared,
    Accepted,
    Read,
    Rejected,
    Unknown,
    Inspected,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorNoticeRequest {
    pub request_id: String,
    pub session_id: String,
    pub branch_id: String,
    pub recipient_site: String,
    pub sender: String,
    pub recipient: String,
    pub message_id: u64,
    pub writer_epoch: u64,
    pub turn: QueueTarget,
    pub priority: i8,
}
impl ActorNoticeRequest {
    pub fn validate(&self) -> Result<()> {
        if !token(&self.request_id)
            || self.request_id.starts_with("qi_")
            || !token(&self.session_id)
            || !token(&self.branch_id)
            || !token(&self.recipient_site)
            || !token(&self.sender)
            || !token(&self.recipient)
            || self.sender == self.recipient
            || self.message_id == 0
            || self.writer_epoch == 0
            || !(-20..=19).contains(&self.priority)
            || matches!(self.turn, QueueTarget::Exact(0))
        {
            return Err(Error::Denied("invalid bounded Notify identity"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorNoticeFact {
    pub request: ActorNoticeRequest,
    pub state: ActorNoticeState,
    pub revision: u64,
    pub payload_sha256: String,
    /// Write-once private encrypted custody, never a portable reading grant.
    pub payload_ref: String,
    pub owner_process_id: String,
    pub owner_store_generation: u64,
    pub note: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorNoticeView {
    pub request_id: String,
    pub state: ActorNoticeState,
    pub revision: u64,
    pub sender: String,
    pub turn: QueueTarget,
    pub note: Option<String>,
}
impl From<&ActorNoticeFact> for ActorNoticeView {
    fn from(f: &ActorNoticeFact) -> Self {
        Self {
            request_id: f.request.request_id.clone(),
            state: f.state,
            revision: f.revision,
            sender: f.request.sender.clone(),
            turn: f.request.turn,
            note: f.note.clone(),
        }
    }
}

pub(crate) fn events(c: &rusqlite::Connection, session: &str) -> Result<Vec<ActorNoticeFact>> {
    let mut query = db(c.prepare(
        "SELECT data_json FROM audit WHERE session_id=?1 AND action=?2 ORDER BY seq LIMIT 8193",
    ))?;
    let rows = db(query.query_map(params![session, ACTION], |r| r.get::<_, String>(0)))?;
    let encoded = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if encoded.len() > MAX_EVENTS {
        return Err(Error::Limit("Notify fact audit requires review"));
    }
    encoded
        .into_iter()
        .map(|s| crate::decode::<ActorNoticeFact>(&s))
        .collect()
}
fn latest(all: &[ActorNoticeFact]) -> BTreeMap<String, ActorNoticeFact> {
    let mut result = BTreeMap::new();
    for fact in all {
        result.insert(fact.request.request_id.clone(), fact.clone());
    }
    result
}
fn future_facts(all: &[ActorNoticeFact]) -> usize {
    latest(all)
        .values()
        .map(|f| match f.state {
            ActorNoticeState::Prepared => 2, // accepted then Read, or Unknown then Inspected
            ActorNoticeState::Accepted | ActorNoticeState::Unknown => 1,
            ActorNoticeState::Read | ActorNoticeState::Rejected | ActorNoticeState::Inspected => 0,
        })
        .sum()
}
fn writer(c: &rusqlite::Connection, owner: &PrincipalId, session: &str, epoch: u64) -> Result<()> {
    if !principal_is_session_owner(c, session, owner)? {
        return Err(Error::Denied("Notify belongs to the current Session owner"));
    }
    let status = workspace_writer::verify_writer_epoch_for_session(c, session, epoch)?;
    if status.state != "active" {
        return Err(Error::Denied("Notify writer is no longer active"));
    }
    Ok(())
}
fn append(c: &rusqlite::Connection, owner: &PrincipalId, fact: &ActorNoticeFact) -> Result<()> {
    audit(
        c,
        &fact.request.session_id,
        owner,
        ACTION,
        serde_json::to_value(fact)
            .map_err(|_| Error::Storage("cannot serialize immutable Notify fact"))?,
    )
}
fn source_sequence(c: &rusqlite::Connection, session: &str, sender: &str) -> Result<u64> {
    let mut max = 0;
    for f in actor_queue::events(c, session)? {
        if f.request.sender == sender {
            max = max.max(f.request.message_id);
        }
        if let Some(claim) = f.claim {
            if claim.recipient_actor == sender {
                max = max.max(claim.message_id);
            }
        }
    }
    for f in actor_admission::events(c, session)? {
        if f.admission.sender == sender {
            max = max.max(f.admission.message_id);
        }
    }
    for f in actor_control::events(c, session)? {
        if f.admission.sender == sender {
            max = max.max(f.admission.message_id);
        }
    }
    for f in events(c, session)? {
        if f.request.sender == sender {
            max = max.max(f.request.message_id);
        }
    }
    Ok(max)
}

impl SessionStore {
    /// Two durable actions precede Runtime delivery: encrypt private body,
    /// then append immutable Prepared intent. This MUST be followed by an
    /// independently verified current-generation Runtime `Delivery::Queued`
    /// before an `Accepted` may be recorded. No automatic retry is possible.
    pub fn actor_notice_prepare(
        &self,
        owner: &PrincipalId,
        request: ActorNoticeRequest,
        body: &str,
    ) -> Result<ActorNoticeFact> {
        request.validate()?;
        if body.is_empty() || body.len() > MAX_BODY || body.contains('\0') {
            return Err(Error::Limit("Notify body must contain 1..65536 safe bytes"));
        }
        self.actor_input_writer_fence(owner, &request.session_id, request.writer_epoch)?;
        let process = self.process_identity()?.ok_or(Error::Denied(
            "durable Notify requires a real owner process",
        ))?;
        let vault = self.local_secret_store()?;
        let blob = new_blob_ref(&request.session_id, &request.request_id)?;
        vault.put(&blob, body.as_bytes())?;
        let result = (|| {
            let mut c = self.connection()?;
            let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
            writer(&tx, owner, &request.session_id, request.writer_epoch)?;
            branch_in(&tx, &request.session_id, &request.branch_id)?;
            let all = events(&tx, &request.session_id)?;
            if all
                .iter()
                .any(|f| f.request.request_id == request.request_id)
                || actor_admission::events(&tx, &request.session_id)?
                    .iter()
                    .any(|f| f.admission.request_id == request.request_id)
                || actor_control::events(&tx, &request.session_id)?
                    .iter()
                    .any(|f| f.admission.request_id == request.request_id)
                || actor_queue::events(&tx, &request.session_id)?
                    .iter()
                    .any(|f| f.request.request_id == request.request_id)
            {
                return Err(Error::Conflict("Actor request ID already used"));
            }
            if all.len() + future_facts(&all) + 3 > MAX_EVENTS {
                return Err(Error::Limit("Notify audit reserves closure facts"));
            }
            if latest(&all)
                .values()
                .filter(|f| {
                    f.request.branch_id == request.branch_id
                        && f.request.recipient_site == request.recipient_site
                        && matches!(
                            f.state,
                            ActorNoticeState::Prepared
                                | ActorNoticeState::Accepted
                                | ActorNoticeState::Unknown
                        )
                })
                .count()
                >= MAX_LIVE
            {
                return Err(Error::Limit("unacknowledged Notify capacity reached"));
            }
            if request.message_id <= source_sequence(&tx, &request.session_id, &request.sender)? {
                return Err(Error::Denied("Notify sender sequence was reused or stale"));
            }
            let fact = ActorNoticeFact {
                request,
                state: ActorNoticeState::Prepared,
                revision: 1,
                payload_sha256: payload_digest(body.as_bytes()),
                payload_ref: blob.clone(),
                owner_process_id: process.process_id,
                owner_store_generation: process.store_generation,
                note: None,
            };
            append(&tx, owner, &fact)?;
            db(tx.commit())?;
            Ok(fact)
        })();
        if result.is_err() {
            let _ = vault.delete(&blob);
        }
        result
    }

    /// Same-process receipt of the actual Runtime outcome, not an effect
    /// receipt. If the process died after Prepared, only manual inspection is
    /// allowed. Rejected is an explicit `Consumed`; unknown includes deferred
    /// or failed transport and is never replayable.
    pub fn actor_notice_transition(
        &self,
        owner: &PrincipalId,
        expected: &ActorNoticeFact,
        next: ActorNoticeState,
    ) -> Result<ActorNoticeFact> {
        if expected.state != ActorNoticeState::Prepared
            || !matches!(
                next,
                ActorNoticeState::Accepted | ActorNoticeState::Rejected | ActorNoticeState::Unknown
            )
        {
            return Err(Error::Denied("Notify outcome requires Prepared fact"));
        }
        let process = self.process_identity()?.ok_or(Error::Denied(
            "Notify outcome requires current real owner process",
        ))?;
        if expected.owner_process_id != process.process_id
            || expected.owner_store_generation != process.store_generation
        {
            return Err(Error::Conflict(
                "restarted owner cannot fabricate Notify acceptance",
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        writer(
            &tx,
            owner,
            &expected.request.session_id,
            expected.request.writer_epoch,
        )?;
        if latest(&events(&tx, &expected.request.session_id)?).get(&expected.request.request_id)
            != Some(expected)
        {
            return Err(Error::Conflict("Notify outcome CAS changed"));
        }
        let fact = ActorNoticeFact {
            state: next,
            revision: expected.revision + 1,
            ..expected.clone()
        };
        append(&tx, owner, &fact)?;
        db(tx.commit())?;
        if next == ActorNoticeState::Rejected {
            self.local_secret_store()?.delete(&fact.payload_ref)?;
        }
        Ok(fact)
    }

    /// Metadata is read-only, bounded and Session-owner scoped. Neither this
    /// nor ActorOperation exports a plaintext body or its encrypted-item ref.
    pub fn actor_notice_list(
        &self,
        owner: &PrincipalId,
        session: &str,
    ) -> Result<Vec<ActorNoticeFact>> {
        let c = self.connection()?;
        if !principal_is_session_owner(&c, session, owner)? {
            return Err(Error::Denied("Notify metadata belongs to Session owner"));
        }
        Ok(latest(&events(&c, session)?).into_values().collect())
    }
    pub fn actor_notice_fact(
        &self,
        owner: &PrincipalId,
        session: &str,
        id: &str,
    ) -> Result<ActorNoticeFact> {
        self.actor_notice_list(owner, session)?
            .into_iter()
            .find(|f| f.request.request_id == id)
            .ok_or(Error::Denied("Notify fact not found"))
    }
    /// Explicit current-writer private observation. Read does NOT re-inject
    /// a notification, contact the Agent, or mark it user-observed.
    pub fn actor_notice_read(
        &self,
        owner: &PrincipalId,
        session: &str,
        id: &str,
        epoch: u64,
    ) -> Result<Zeroizing<String>> {
        let c = self.connection()?;
        writer(&c, owner, session, epoch)?;
        let fact = latest(&events(&c, session)?)
            .remove(id)
            .ok_or(Error::Denied("Notify not found"))?;
        if fact.state != ActorNoticeState::Accepted {
            return Err(Error::Conflict(
                "Notify body not accepted or already acknowledged",
            ));
        }
        let bytes = self.local_secret_store()?.get(&fact.payload_ref)?;
        if payload_digest(bytes.as_slice()) != fact.payload_sha256 {
            return Err(Error::Denied("Notify private body digest mismatch"));
        }
        Ok(Zeroizing::new(String::from_utf8(bytes.to_vec()).map_err(
            |_| Error::Denied("Notify private body is not UTF-8"),
        )?))
    }
    /// Acknowledgment is an authorized reader fact, NEVER an external effect
    /// success. A different current writer may read/ack an old Accepted fact
    /// after failover, but cannot retroactively fake Runtime acceptance.
    pub fn actor_notice_ack_read(
        &self,
        owner: &PrincipalId,
        session: &str,
        id: &str,
        epoch: u64,
        expected_revision: u64,
    ) -> Result<ActorNoticeFact> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        writer(&tx, owner, session, epoch)?;
        let old = latest(&events(&tx, session)?)
            .remove(id)
            .ok_or(Error::Denied("Notify not found"))?;
        if old.state != ActorNoticeState::Accepted || old.revision != expected_revision {
            return Err(Error::Conflict(
                "Notify read ACK requires exact Accepted revision",
            ));
        }
        let fact = ActorNoticeFact {
            state: ActorNoticeState::Read,
            revision: old.revision + 1,
            ..old
        };
        append(&tx, owner, &fact)?;
        db(tx.commit())?;
        self.local_secret_store()?.delete(&fact.payload_ref)?;
        Ok(fact)
    }
    /// Explicit disposition of ambiguous Prepared / Unknown; never turns it
    /// into Accepted, Read, or a new Runtime dispatch. Audit note is printable.
    pub fn actor_notice_inspect(
        &self,
        owner: &PrincipalId,
        session: &str,
        id: &str,
        epoch: u64,
        note: &str,
    ) -> Result<ActorNoticeFact> {
        if note.trim().is_empty() || note.len() > 4096 || note.chars().any(char::is_control) {
            return Err(Error::Denied(
                "Notify inspection note must be bounded printable text",
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        writer(&tx, owner, session, epoch)?;
        let old = latest(&events(&tx, session)?)
            .remove(id)
            .ok_or(Error::Denied("Notify not found"))?;
        if !matches!(
            old.state,
            ActorNoticeState::Prepared | ActorNoticeState::Unknown
        ) {
            return Err(Error::Conflict("only ambiguous Notify requires inspection"));
        }
        let fact = ActorNoticeFact {
            state: ActorNoticeState::Inspected,
            revision: old.revision + 1,
            note: Some(note.to_owned()),
            ..old
        };
        append(&tx, owner, &fact)?;
        db(tx.commit())?;
        self.local_secret_store()?.delete(&fact.payload_ref)?;
        Ok(fact)
    }
}
