//! AW-P6-9B: bounded, immutable, writer-fenced control-admission facts over
//! SQLite v33. These are authenticated host-side intent/outcome observations,
//! never replay instructions, external-effect receipts or new tool grants.
use crate::{
    actor_admission, audit, branch_in, db, principal_is_session_owner, workspace_writer,
    SessionStore,
};
use aw_agent_core::{Error, PrincipalId, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const ACTION: &str = "actor.control";
const MAX_EVENTS: usize = 8192;
const MAX_LABEL: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorControlAction {
    CancelTurn,
    FreezeTurn,
    ResumeTurn,
    FreezeActor,
    ResumeActor,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorControlState {
    Prepared,
    Applied,
    Rejected,
    Unknown,
    Inspected,
}
impl ActorControlState {
    pub const fn needs_inspection(self) -> bool {
        matches!(self, Self::Prepared | Self::Unknown)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorControlAdmission {
    pub request_id: String,
    pub input_request_id: String,
    pub session_id: String,
    pub branch_id: String,
    pub writer_epoch: u64,
    pub turn_id: u64,
    pub sender: String,
    pub target: String,
    pub message_id: u64,
    pub action: ActorControlAction,
}
impl ActorControlAdmission {
    pub fn validate(&self) -> Result<()> {
        let token = |s: &str| {
            !s.is_empty()
                && s.len() <= MAX_LABEL
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-:.@".contains(&b))
        };
        if !token(&self.request_id)
            || self.request_id.starts_with("qi_")
            || !token(&self.input_request_id)
            || !token(&self.session_id)
            || !token(&self.branch_id)
            || !token(&self.sender)
            || !token(&self.target)
            || self.request_id == self.input_request_id
            || self.writer_epoch == 0
            || self.turn_id == 0
            || self.message_id == 0
        {
            return Err(Error::Denied("invalid bounded Actor Control admission"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorControlFact {
    pub admission: ActorControlAdmission,
    pub state: ActorControlState,
    pub note: Option<String>,
    /// Exact owner-process incarnation that prepared this action; a new
    /// process after restart may inspect but cannot fabricate Applied.
    pub owner_process_id: Option<String>,
    pub owner_store_generation: Option<u64>,
}
pub(crate) fn events(c: &rusqlite::Connection, session: &str) -> Result<Vec<ActorControlFact>> {
    let mut q = db(c.prepare(
        "SELECT data_json FROM audit WHERE session_id=?1 AND action=?2
         ORDER BY seq LIMIT 8193",
    ))?;
    let rows = db(q.query_map(params![session, ACTION], |r| r.get::<_, String>(0)))?;
    let payloads = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if payloads.len() > MAX_EVENTS {
        return Err(Error::Limit(
            "Actor control fact audit needs explicit rotation/review",
        ));
    }
    payloads
        .iter()
        .map(|s| crate::decode::<ActorControlFact>(s))
        .collect()
}
fn latest(events: &[ActorControlFact], id: &str) -> Option<ActorControlFact> {
    events
        .iter()
        .rev()
        .find(|event| event.admission.request_id == id)
        .cloned()
}
fn verify_writer(
    c: &rusqlite::Connection,
    who: &PrincipalId,
    session: &str,
    epoch: u64,
) -> Result<()> {
    if !principal_is_session_owner(c, session, who)? {
        return Err(Error::Denied("Control requires current Session owner"));
    }
    workspace_writer::verify_writer_epoch_for_session(c, session, epoch)?;
    Ok(())
}
fn append(c: &rusqlite::Connection, actor: &PrincipalId, fact: &ActorControlFact) -> Result<()> {
    audit(
        c,
        &fact.admission.session_id,
        actor,
        ACTION,
        serde_json::to_value(fact)
            .map_err(|_| Error::Storage("cannot serialize control admission"))?,
    )
}
impl SessionStore {
    /// Durable *intent* for an ALREADY-ACTIVE exact Actor Turn. This can only
    /// be submitted by the original owner with an active branch/model run and
    /// matching immutable Actor Input Dispatching evidence. RPC must not call
    /// this based on a client-displayed ActorKey or untrusted JSON alone.
    pub fn actor_control_prepare(
        &self,
        actor: &PrincipalId,
        admission: ActorControlAdmission,
    ) -> Result<ActorControlFact> {
        admission.validate()?;
        let owner_process = self.process_identity()?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        verify_writer(&tx, actor, &admission.session_id, admission.writer_epoch)?;
        let branch = branch_in(&tx, &admission.session_id, &admission.branch_id)?;
        if branch.id != admission.branch_id {
            return Err(Error::Denied("Control branch identity mismatch"));
        }
        let active: Option<u64> = db(tx
            .query_row(
                "SELECT r.turn_id FROM runs r JOIN branches b ON b.active_run=r.id
             WHERE b.id=?1 AND r.session_id=?2 AND r.branch_id=?1
               AND r.status='running' AND r.produced_from_writer_epoch=?3",
                params![
                    admission.branch_id,
                    admission.session_id,
                    admission.writer_epoch
                ],
                |r| r.get(0),
            )
            .optional())?;
        if active != Some(admission.turn_id) {
            return Err(Error::Conflict(
                "Control requires exact active journaled Turn",
            ));
        }
        let inputs = actor_admission::events(&tx, &admission.session_id)?;
        let current = inputs
            .iter()
            .rev()
            .find(|item| item.admission.request_id == admission.input_request_id)
            .ok_or(Error::Denied("Control has no original Actor Input fact"))?;
        if current.state != actor_admission::InputAdmissionState::Dispatching
            || current.admission.turn_id != admission.turn_id
            || current.admission.writer_epoch != admission.writer_epoch
            || current.admission.target != admission.target
            || current.admission.sender != admission.sender
            || current.admission.session_id != admission.session_id
            || current.admission.branch_id != admission.branch_id
            || (current.admission.sender == admission.sender
                && admission.message_id <= current.admission.message_id)
        {
            return Err(Error::Denied(
                "Control cannot target stale/different Actor Input",
            ));
        }
        let prior = events(&tx, &admission.session_id)?;
        if prior
            .iter()
            .any(|old| old.admission.request_id == admission.request_id)
            || inputs
                .iter()
                .any(|old| old.admission.request_id == admission.request_id)
            || crate::actor_queue::events(&tx, &admission.session_id)?
                .iter()
                .any(|old| old.request.request_id == admission.request_id)
        {
            return Err(Error::Conflict(
                "Control ID already used by Input or Control fact",
            ));
        }
        let mut latest = BTreeMap::new();
        for event in &prior {
            latest.insert(&event.admission.request_id, event.state);
        }
        if latest.values().any(|state| state.needs_inspection()) {
            return Err(Error::Conflict(
                "unsettled Control fact requires inspection",
            ));
        }
        let notices = crate::actor_notice::events(&tx, &admission.session_id)?;
        if notices.iter().any(|event| {
            event.request.request_id == admission.request_id
                || (event.request.sender == admission.sender
                    && event.request.message_id >= admission.message_id)
        }) {
            return Err(Error::Denied(
                "Actor Control ID/sequence was already used by Notify",
            ));
        }
        let queue = crate::actor_queue::events(&tx, &admission.session_id)?;
        if prior.iter().any(|event| {
            event.admission.sender == admission.sender
                && event.admission.message_id >= admission.message_id
        }) || queue.iter().any(|event| {
            (event.request.sender == admission.sender
                && event.request.message_id >= admission.message_id)
                || event.claim.as_ref().is_some_and(|claim| {
                    claim.recipient_actor == admission.sender
                        && claim.message_id >= admission.message_id
                })
        }) {
            return Err(Error::Denied(
                "Actor Control source sequence was reused/out of order",
            ));
        }
        if prior.len() >= MAX_EVENTS {
            return Err(Error::Limit("Actor Control audit capacity exhausted"));
        }
        let fact = ActorControlFact {
            admission,
            state: ActorControlState::Prepared,
            note: None,
            owner_process_id: owner_process.as_ref().map(|p| p.process_id.clone()),
            owner_store_generation: owner_process.as_ref().map(|p| p.store_generation),
        };
        append(&tx, actor, &fact)?;
        db(tx.commit())?;
        Ok(fact)
    }
    /// Terminal fact for one synchronous authenticated Runtime Control
    /// admission. "Applied" DOES NOT mean provider, tool or OS effect done.
    pub fn actor_control_transition(
        &self,
        actor: &PrincipalId,
        admission: &ActorControlAdmission,
        from: ActorControlState,
        to: ActorControlState,
    ) -> Result<ActorControlFact> {
        admission.validate()?;
        if from != ActorControlState::Prepared
            || !matches!(
                to,
                ActorControlState::Applied
                    | ActorControlState::Rejected
                    | ActorControlState::Unknown
            )
        {
            return Err(Error::Denied("invalid Control fact transition"));
        }
        let owner_process = self.process_identity()?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        verify_writer(&tx, actor, &admission.session_id, admission.writer_epoch)?;
        let previous = latest(&events(&tx, &admission.session_id)?, &admission.request_id)
            .ok_or(Error::Denied("Control has no prepared fact"))?;
        if previous.admission != *admission || previous.state != from {
            return Err(Error::Conflict(
                "Control changed or outcome already recorded",
            ));
        }
        if previous.owner_process_id.as_deref()
            != owner_process.as_ref().map(|p| p.process_id.as_str())
            || previous.owner_store_generation != owner_process.as_ref().map(|p| p.store_generation)
        {
            return Err(Error::Conflict(
                "a restarted owner may inspect Control but cannot claim it was applied",
            ));
        }
        let fact = ActorControlFact {
            admission: admission.clone(),
            state: to,
            note: None,
            owner_process_id: previous.owner_process_id,
            owner_store_generation: previous.owner_store_generation,
        };
        append(&tx, actor, &fact)?;
        db(tx.commit())?;
        Ok(fact)
    }
    pub fn actor_control_fact(
        &self,
        actor: &PrincipalId,
        session: &str,
        request_id: &str,
    ) -> Result<ActorControlFact> {
        let c = self.connection()?;
        if !principal_is_session_owner(&c, session, actor)? {
            return Err(Error::Denied("Control facts belong to Session owner"));
        }
        latest(&events(&c, session)?, request_id)
            .ok_or(Error::Denied("Actor Control fact not found"))
    }
    /// Bounded, current-state query; does not recover, resend, or grant
    /// control authority. Includes terminal facts for explicit inspection.
    pub fn actor_control_list(
        &self,
        actor: &PrincipalId,
        session: &str,
    ) -> Result<Vec<ActorControlFact>> {
        let c = self.connection()?;
        if !principal_is_session_owner(&c, session, actor)? {
            return Err(Error::Denied("Control audit belongs to Session owner"));
        }
        let mut by_id = BTreeMap::new();
        for fact in events(&c, session)? {
            by_id.insert(fact.admission.request_id.clone(), fact);
        }
        Ok(by_id.into_values().collect())
    }
    /// A crashed/stale control never resumes its effect. Explicit inspection
    /// records a new fact, while *independent* paid/effect hazards remain.
    pub fn actor_control_inspect(
        &self,
        actor: &PrincipalId,
        session: &str,
        request_id: &str,
        expected_epoch: u64,
        note: &str,
    ) -> Result<ActorControlFact> {
        if note.trim().is_empty() || note.len() > 4096 || note.chars().any(char::is_control) {
            return Err(Error::Denied(
                "Control inspection requires a printable note",
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        verify_writer(&tx, actor, session, expected_epoch)?;
        let old = latest(&events(&tx, session)?, request_id)
            .ok_or(Error::Denied("no Control fact to inspect"))?;
        if !old.state.needs_inspection() {
            return Err(Error::Conflict("Control fact already terminal"));
        }
        let fact = ActorControlFact {
            admission: old.admission,
            state: ActorControlState::Inspected,
            note: Some(note.to_owned()),
            owner_process_id: old.owner_process_id,
            owner_store_generation: old.owner_store_generation,
        };
        append(&tx, actor, &fact)?;
        db(tx.commit())?;
        Ok(fact)
    }
}
