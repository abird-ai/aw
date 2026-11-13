//! Durable custody for same-Turn Agent steering boundaries.
//!
//! This is an immutable audit + encrypted SecretStore receipt, not another
//! mailbox or Agent journal. The authoritative TurnJournal records the actual
//! AuthoritySegment application; these facts only prove custody/admission and
//! prevent replay after ambiguous delivery.

use crate::{
    audit, branch_in, context, db, principal_is_session_owner, session_participation,
    workspace_writer, Error, ParticipantInputAdmission, PrincipalId, Result, SecretStore,
    SessionParticipationAction, SessionStore,
};
use getrandom::fill;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

const ACTION: &str = "actor.authority_steer";
const MAX_EVENTS: usize = 4096;
const MAX_BODY: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthoritySteerState {
    Prepared,
    Accepted,
    Applied,
    Rejected,
    Unknown,
    Inspected,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoritySteerRequest {
    pub request_id: String,
    pub session_id: String,
    pub branch_id: String,
    pub writer_epoch: u64,
    pub turn_id: u64,
    pub canonical_target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub participant: Option<ParticipantInputAdmission>,
}

impl AuthoritySteerRequest {
    fn validate(&self) -> Result<()> {
        let valid = |value: &str, max: usize| {
            !value.is_empty()
                && value.len() <= max
                && value.is_ascii()
                && !value.chars().any(char::is_control)
        };
        if !valid(&self.request_id, 128)
            || !valid(&self.session_id, 128)
            || !valid(&self.branch_id, 128)
            || !valid(&self.canonical_target, 1024)
            || self.writer_epoch == 0
            || self.turn_id == 0
        {
            return Err(Error::Denied("invalid AuthoritySteer request"));
        }
        if let Some(participant) = &self.participant {
            participant.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoritySteerFact {
    pub request: AuthoritySteerRequest,
    pub state: AuthoritySteerState,
    pub revision: u64,
    pub payload_sha256: String,
    /// Opaque encrypted write-once SecretStore item; never plaintext in audit.
    pub payload_ref: String,
    pub segment_ordinal: Option<u32>,
    pub segment_fingerprint: Option<[u8; 32]>,
    pub note: Option<String>,
}

pub struct ClaimedAuthoritySteer {
    pub fact: AuthoritySteerFact,
    pub payload: Zeroizing<String>,
}

fn hash_payload(payload: &[u8]) -> String {
    format!("{:x}", Sha256::digest(payload))
}

fn random_secret_ref(session: &str, request_id: &str) -> Result<String> {
    let mut entropy = [0u8; 32];
    fill(&mut entropy)
        .map_err(|_| Error::Storage("OS randomness unavailable for steer custody"))?;
    let mut hash = Sha256::new();
    hash.update(b"aw-authority-steer-secret-v1\0");
    hash.update(session.as_bytes());
    hash.update([0]);
    hash.update(request_id.as_bytes());
    hash.update(entropy);
    Ok(format!("sec_{:x}", hash.finalize()))
}

pub(crate) fn events(c: &rusqlite::Connection, session: &str) -> Result<Vec<AuthoritySteerFact>> {
    let mut query = db(c.prepare(
        "SELECT data_json FROM audit WHERE session_id=?1 AND action=?2 ORDER BY seq LIMIT 4097",
    ))?;
    let rows = db(query.query_map(params![session, ACTION], |row| row.get::<_, String>(0)))?;
    let encoded = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if encoded.len() > MAX_EVENTS {
        return Err(Error::Limit("AuthoritySteer audit requires inspection"));
    }
    encoded
        .into_iter()
        .map(|value| crate::decode::<AuthoritySteerFact>(&value))
        .collect()
}

fn latest<'a>(
    events: &'a [AuthoritySteerFact],
    request_id: &str,
) -> Option<&'a AuthoritySteerFact> {
    events
        .iter()
        .rev()
        .find(|fact| fact.request.request_id == request_id)
}

fn writer(c: &rusqlite::Connection, owner: &PrincipalId, session: &str, epoch: u64) -> Result<()> {
    if !principal_is_session_owner(c, session, owner)? {
        return Err(Error::Denied(
            "AuthoritySteer requires the durable Session owner",
        ));
    }
    let status = workspace_writer::verify_writer_epoch_for_session(c, session, epoch)?;
    if status.state != "active" {
        return Err(Error::Denied("AuthoritySteer writer is no longer active"));
    }
    Ok(())
}

fn audit_fact(
    c: &rusqlite::Connection,
    owner: &PrincipalId,
    fact: &AuthoritySteerFact,
) -> Result<()> {
    audit(
        c,
        &fact.request.session_id,
        owner,
        ACTION,
        serde_json::to_value(fact)
            .map_err(|_| Error::Storage("cannot serialize AuthoritySteer fact"))?,
    )
}

impl SessionStore {
    pub fn authority_steer_prepare(
        &self,
        owner: &PrincipalId,
        request: AuthoritySteerRequest,
        payload: &str,
    ) -> Result<AuthoritySteerFact> {
        request.validate()?;
        if payload.is_empty() || payload.len() > MAX_BODY || payload.contains('\0') {
            return Err(Error::Limit(
                "AuthoritySteer text must contain 1..65536 safe bytes",
            ));
        }
        self.actor_input_writer_fence(owner, &request.session_id, request.writer_epoch)?;
        let vault = self.local_secret_store()?;
        let payload_ref = random_secret_ref(&request.session_id, &request.request_id)?;
        vault.put(&payload_ref, payload.as_bytes())?;
        let result = (|| {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            writer(&tx, owner, &request.session_id, request.writer_epoch)?;
            let branch = branch_in(&tx, &request.session_id, &request.branch_id)?;
            let active_run = branch.active_run.as_deref().ok_or(Error::Conflict(
                "AuthoritySteer requires an active durable Turn",
            ))?;
            let active_turn: u64 = db(tx.query_row(
                "SELECT turn_id FROM runs
                 WHERE id=?1 AND session_id=?2 AND branch_id=?3 AND status='running'",
                params![active_run, request.session_id, request.branch_id],
                |row| row.get(0),
            ))?;
            if active_turn != request.turn_id {
                return Err(Error::Conflict(
                    "AuthoritySteer targets a different active Turn",
                ));
            }
            if let Some(participant) = &request.participant {
                if participant.actor == *owner {
                    return Err(Error::Denied(
                        "Session owner Control uses the owner authority path",
                    ));
                }
                let current = session_participation::authorize_in(
                    &tx,
                    &participant.actor,
                    &request.session_id,
                    SessionParticipationAction::Control,
                )?;
                if current.owner
                    || current.membership_epoch != participant.membership_epoch
                    || current.audience_epoch != participant.audience_epoch
                {
                    return Err(Error::Denied(
                        "participant Control/audience authority changed before steer admission",
                    ));
                }
                context::authorize_node(
                    &tx,
                    &request.session_id,
                    &branch.head,
                    &participant.actor,
                )?;
                context::authorize_model_context_for_participation(
                    &tx,
                    &request.session_id,
                    &branch,
                    &participant.identity,
                    SessionParticipationAction::Control,
                )?;
            }
            let all = events(&tx, &request.session_id)?;
            if latest(&all, &request.request_id).is_some() {
                return Err(Error::Conflict("AuthoritySteer request ID already used"));
            }
            if all.len() >= MAX_EVENTS.saturating_sub(2) {
                return Err(Error::Limit(
                    "AuthoritySteer audit reserves closure capacity",
                ));
            }
            let fact = AuthoritySteerFact {
                request,
                state: AuthoritySteerState::Prepared,
                revision: 1,
                payload_sha256: hash_payload(payload.as_bytes()),
                payload_ref: payload_ref.clone(),
                segment_ordinal: None,
                segment_fingerprint: None,
                note: None,
            };
            audit_fact(&tx, owner, &fact)?;
            db(tx.commit())?;
            Ok(fact)
        })();
        if result.is_err() {
            let _ = vault.delete(&payload_ref);
        }
        result
    }

    pub fn authority_steer_fact(
        &self,
        owner: &PrincipalId,
        session: &str,
        request_id: &str,
    ) -> Result<AuthoritySteerFact> {
        let connection = self.connection()?;
        if !principal_is_session_owner(&connection, session, owner)? {
            return Err(Error::Denied(
                "AuthoritySteer fact belongs to Session owner",
            ));
        }
        let all = events(&connection, session)?;
        latest(&all, request_id)
            .cloned()
            .ok_or(Error::Denied("AuthoritySteer fact unavailable"))
    }

    pub fn authority_steer_accept(
        &self,
        owner: &PrincipalId,
        prepared: &AuthoritySteerFact,
        segment: &aw_agent_core::AuthoritySegment,
    ) -> Result<AuthoritySteerFact> {
        segment.validate()?;
        if prepared.state != AuthoritySteerState::Prepared
            || prepared.segment_ordinal.is_some()
            || prepared.segment_fingerprint.is_some()
        {
            return Err(Error::Conflict(
                "AuthoritySteer accept requires exact Prepared fact",
            ));
        }
        if let Some(participant) = &prepared.request.participant {
            let expected = participant.authority_segment_evidence()?;
            if segment.evidence.as_ref() != Some(&expected) {
                return Err(Error::Denied(
                    "staged AuthoritySegment differs from admitted participant Control evidence",
                ));
            }
        }
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        writer(
            &tx,
            owner,
            &prepared.request.session_id,
            prepared.request.writer_epoch,
        )?;
        let all = events(&tx, &prepared.request.session_id)?;
        let current = latest(&all, &prepared.request.request_id)
            .ok_or(Error::Denied("AuthoritySteer fact disappeared"))?;
        if current != prepared {
            return Err(Error::Conflict("AuthoritySteer fact changed before accept"));
        }
        let mut next = prepared.clone();
        next.state = AuthoritySteerState::Accepted;
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or(Error::Limit("AuthoritySteer revision exhausted"))?;
        next.segment_ordinal = Some(segment.ordinal);
        next.segment_fingerprint = Some(segment.fingerprint);
        audit_fact(&tx, owner, &next)?;
        db(tx.commit())?;
        Ok(next)
    }

    pub fn authority_steer_reject_prepared(
        &self,
        owner: &PrincipalId,
        prepared: &AuthoritySteerFact,
        note: &str,
    ) -> Result<AuthoritySteerFact> {
        if prepared.state != AuthoritySteerState::Prepared
            || note.trim().is_empty()
            || note.len() > 1024
        {
            return Err(Error::Conflict(
                "AuthoritySteer rejection requires exact Prepared fact and note",
            ));
        }
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        writer(
            &tx,
            owner,
            &prepared.request.session_id,
            prepared.request.writer_epoch,
        )?;
        let all = events(&tx, &prepared.request.session_id)?;
        let current = latest(&all, &prepared.request.request_id)
            .ok_or(Error::Denied("AuthoritySteer fact disappeared"))?;
        if current != prepared {
            return Err(Error::Conflict(
                "AuthoritySteer fact changed before rejection",
            ));
        }
        let mut next = prepared.clone();
        next.state = AuthoritySteerState::Rejected;
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or(Error::Limit("AuthoritySteer revision exhausted"))?;
        next.note = Some(note.to_owned());
        audit_fact(&tx, owner, &next)?;
        db(tx.commit())?;
        let _ = self.local_secret_store()?.delete(&prepared.payload_ref);
        Ok(next)
    }

    pub fn authority_steer_mark_unknown(
        &self,
        owner: &PrincipalId,
        fact: &AuthoritySteerFact,
        note: &str,
    ) -> Result<AuthoritySteerFact> {
        if note.trim().is_empty() || note.len() > 1024 {
            return Err(Error::Denied("AuthoritySteer unknown note is invalid"));
        }
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        writer(
            &tx,
            owner,
            &fact.request.session_id,
            fact.request.writer_epoch,
        )?;
        let all = events(&tx, &fact.request.session_id)?;
        let current = latest(&all, &fact.request.request_id)
            .ok_or(Error::Denied("AuthoritySteer fact disappeared"))?;
        if current != fact
            || !matches!(
                fact.state,
                AuthoritySteerState::Prepared | AuthoritySteerState::Accepted
            )
        {
            return Err(Error::Conflict(
                "AuthoritySteer unknown requires exact unsettled fact",
            ));
        }
        let mut next = fact.clone();
        next.state = AuthoritySteerState::Unknown;
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or(Error::Limit("AuthoritySteer revision exhausted"))?;
        next.note = Some(note.to_owned());
        audit_fact(&tx, owner, &next)?;
        db(tx.commit())?;
        Ok(next)
    }

    /// Close an accepted steer only if the original immutable TurnJournal
    /// actually contains the matching AuthoritySegment. This never replays the
    /// encrypted payload or infers application from Runtime memory.
    pub fn authority_steer_finalize_turn(
        &self,
        owner: &PrincipalId,
        session: &str,
        branch: &str,
        writer_epoch: u64,
        turn_id: u64,
    ) -> Result<()> {
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        writer(&tx, owner, session, writer_epoch)?;
        let all = events(&tx, session)?;
        let accepted = all
            .iter()
            .filter(|fact| {
                fact.request.branch_id == branch
                    && fact.request.turn_id == turn_id
                    && fact.state == AuthoritySteerState::Accepted
            })
            .cloned()
            .collect::<Vec<_>>();
        if accepted.is_empty() {
            db(tx.commit())?;
            return Ok(());
        }
        let run: Option<String> = db(tx
            .query_row(
                "SELECT id FROM runs WHERE session_id=?1 AND branch_id=?2 AND turn_id=?3
                 ORDER BY started_at DESC LIMIT 1",
                params![session, branch, turn_id],
                |row| row.get(0),
            )
            .optional())?;
        let run = run.ok_or(Error::Conflict(
            "AuthoritySteer finalize cannot find original Turn journal",
        ))?;
        let encoded = {
            let mut query = db(
                tx.prepare("SELECT payload FROM run_events WHERE run_id=?1 ORDER BY seq LIMIT 513")
            )?;
            let rows = db(query.query_map([&run], |row| row.get::<_, String>(0)))?;
            db(rows.collect::<rusqlite::Result<Vec<_>>>())?
        };
        if encoded.len() > 512 {
            return Err(Error::Limit(
                "AuthoritySteer journal inspection exceeds bound",
            ));
        }
        let journal = encoded
            .into_iter()
            .map(|value| crate::decode::<aw_agent_core::JournalEntry>(&value))
            .collect::<Result<Vec<_>>>()?;
        for fact in accepted {
            let applied = journal.iter().any(|entry| {
                matches!(
                    entry,
                    aw_agent_core::JournalEntry::AuthoritySegmentStarted { segment }
                        if Some(segment.ordinal) == fact.segment_ordinal
                            && Some(segment.fingerprint) == fact.segment_fingerprint
                            && segment.boundary_id == fact.request.request_id
                )
            });
            if !applied {
                return Err(Error::Conflict(
                    "accepted AuthoritySteer lacks matching TurnJournal segment",
                ));
            }
            let mut next = fact.clone();
            next.state = AuthoritySteerState::Applied;
            next.revision = next
                .revision
                .checked_add(1)
                .ok_or(Error::Limit("AuthoritySteer revision exhausted"))?;
            audit_fact(&tx, owner, &next)?;
        }
        db(tx.commit())?;
        Ok(())
    }

    pub fn authority_steer_payload(
        &self,
        owner: &PrincipalId,
        fact: &AuthoritySteerFact,
    ) -> Result<ClaimedAuthoritySteer> {
        if fact.state != AuthoritySteerState::Accepted {
            return Err(Error::Denied(
                "AuthoritySteer payload requires exact Accepted custody",
            ));
        }
        self.actor_input_writer_fence(owner, &fact.request.session_id, fact.request.writer_epoch)?;
        let bytes = self.local_secret_store()?.get(&fact.payload_ref)?;
        if hash_payload(&bytes) != fact.payload_sha256 {
            return Err(Error::Denied(
                "AuthoritySteer encrypted payload hash changed",
            ));
        }
        let text = String::from_utf8(bytes.to_vec())
            .map_err(|_| Error::Denied("AuthoritySteer payload is not UTF-8"))?;
        Ok(ClaimedAuthoritySteer {
            fact: fact.clone(),
            payload: Zeroizing::new(text),
        })
    }
}
