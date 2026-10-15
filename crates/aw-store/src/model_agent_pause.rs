//! AW-P6-9D2B2B2: opt-in model-origin *future-admission hold* for an ORIGINAL
//! journaled Agent. This is not CancelTurn, FreezeActor, an OS signal, an
//! additional Agent runner, or approval from the model as a human user.
//!
//! The original currently executing Turn remains untouched and must settle
//! in its original immutable Journal. A same-writer SQLite v33 audit event
//! blocks ALL later original Actor Input admission and queue claims. Only
//! an independently authenticated original owner can append a later release
//! after confirming the originating Input factually Settled. A crashed or
//! Unknown Turn stays held for inspection, never auto-resumes or replays.
use crate::{
    actor_admission, audit, branch_in, db, principal_is_session_owner, workspace_writer,
    SessionStore,
};
use aw_agent_core::{Error, PrincipalId, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const ACTION: &str = "actor.model.future_pause";
const MAX_EVENTS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAgentPauseState {
    Armed,
    Released,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAgentPauseRequest {
    pub intent_id: String,
    pub input_request_id: String,
    pub session_id: String,
    pub branch_id: String,
    pub sender: String,
    pub writer_epoch: u64,
    pub turn_id: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAgentPauseFact {
    pub request: ModelAgentPauseRequest,
    pub state: ModelAgentPauseState,
    pub owner_note: Option<String>,
}
/// This is the SAME ID in both the original bound owner and the store;
/// a provider/ToolCall ID or prompt text cannot control another pause scope.
pub fn model_agent_pause_id(session: &str, branch: &str, turn: u64) -> String {
    let mut hash = Sha256::new();
    hash.update(b"aw-model-agent-future-admission-pause-v1\0");
    for part in [session.as_bytes(), branch.as_bytes(), &turn.to_be_bytes()] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    format!("ap_{:x}", hash.finalize())
}
impl ModelAgentPauseRequest {
    fn validate(&self) -> Result<()> {
        let valid = |s: &str| {
            !s.is_empty()
                && s.len() <= 128
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_:-.@".contains(&c))
        };
        if !valid(&self.input_request_id)
            || !valid(&self.session_id)
            || !valid(&self.branch_id)
            || !valid(&self.sender)
            || self.writer_epoch == 0
            || self.turn_id == 0
            || self.intent_id
                != model_agent_pause_id(&self.session_id, &self.branch_id, self.turn_id)
        {
            return Err(Error::Denied("invalid original model Agent pause identity"));
        }
        Ok(())
    }
}
fn records(conn: &rusqlite::Connection, session: &str) -> Result<Vec<ModelAgentPauseFact>> {
    let mut query = db(conn.prepare(
        "SELECT data_json FROM audit WHERE session_id=?1 AND action=?2 ORDER BY seq LIMIT 4097",
    ))?;
    let rows = db(query.query_map(params![session, ACTION], |row| row.get::<_, String>(0)))?;
    let serialized = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if serialized.len() > MAX_EVENTS {
        return Err(Error::Limit(
            "original model Agent pause audit requires explicit owner review",
        ));
    }
    serialized.iter().map(|json| crate::decode(json)).collect()
}
fn latest(
    conn: &rusqlite::Connection,
    session: &str,
) -> Result<BTreeMap<String, ModelAgentPauseFact>> {
    let mut found = BTreeMap::new();
    for fact in records(conn, session)? {
        fact.request.validate()?;
        found.insert(fact.request.intent_id.clone(), fact);
    }
    Ok(found)
}
fn writer(
    conn: &rusqlite::Connection,
    owner: &PrincipalId,
    session: &str,
    epoch: u64,
) -> Result<()> {
    if !principal_is_session_owner(conn, session, owner)? {
        return Err(Error::Denied(
            "Agent future pause belongs to the original Session owner",
        ));
    }
    let current = workspace_writer::verify_writer_epoch_for_session(conn, session, epoch)?;
    if current.state != "active" {
        return Err(Error::Denied(
            "Agent future pause requires the active original writer",
        ));
    }
    Ok(())
}
fn append(
    conn: &rusqlite::Connection,
    owner: &PrincipalId,
    fact: &ModelAgentPauseFact,
) -> Result<()> {
    audit(
        conn,
        &fact.request.session_id,
        owner,
        ACTION,
        serde_json::to_value(fact)
            .map_err(|_| Error::Storage("cannot encode original Agent future pause fact"))?,
    )
}
/// **Inside the existing original Input/Queue SQLite writer transaction**.
/// No new counter/clock/worker: one still-Armed model pause gates next
/// admission, even when the source Turn is Unknown or the Process restarted.
pub(crate) fn require_future_admission(
    conn: &rusqlite::Connection,
    session: &str,
    branch: &str,
) -> Result<()> {
    if latest(conn, session)?
        .values()
        .any(|fact| fact.request.branch_id == branch && fact.state == ModelAgentPauseState::Armed)
    {
        return Err(Error::Conflict(
            "original Agent future turn admission is held; exact current owner must explicitly release after Turn settlement",
        ));
    }
    Ok(())
}
impl SessionStore {
    /// Durable, non-interrupting request from a Tool in the ONE original
    /// currently journaled paid Turn. Strict active-run and Input evidence
    /// is checked inside the SAME SQLite writer transaction as Armed.
    pub fn model_agent_pause_after_turn(
        &self,
        owner: &PrincipalId,
        request: ModelAgentPauseRequest,
    ) -> Result<ModelAgentPauseFact> {
        request.validate()?;
        let mut conn = self.connection()?;
        let tx = db(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
        writer(&tx, owner, &request.session_id, request.writer_epoch)?;
        branch_in(&tx, &request.session_id, &request.branch_id)?;
        let active: Option<u64> = db(tx
            .query_row(
                "SELECT r.turn_id FROM runs r JOIN branches b ON b.active_run=r.id
              WHERE b.id=?1 AND r.session_id=?2 AND r.branch_id=?1
                AND r.status='running' AND r.produced_from_writer_epoch=?3",
                params![request.branch_id, request.session_id, request.writer_epoch],
                |r| r.get(0),
            )
            .optional())?;
        if active != Some(request.turn_id) {
            return Err(Error::Denied(
                "model Agent pause requires actual original running Turn",
            ));
        }
        let mut inputs = BTreeMap::new();
        for fact in actor_admission::events(&tx, &request.session_id)? {
            inputs.insert(fact.admission.request_id.clone(), fact);
        }
        let current = inputs.get(&request.input_request_id).ok_or(Error::Denied(
            "model Agent pause has no original admitted Input",
        ))?;
        if current.state != actor_admission::InputAdmissionState::Dispatching
            || current.admission.session_id != request.session_id
            || current.admission.branch_id != request.branch_id
            || current.admission.turn_id != request.turn_id
            || current.admission.writer_epoch != request.writer_epoch
            || current.admission.sender != request.sender
            || current.admission.target != request.sender
        {
            return Err(Error::Denied(
                "model Agent pause does not match original active Input",
            ));
        }
        let previous = latest(&tx, &request.session_id)?;
        if previous.len() >= MAX_EVENTS / 2
            || previous.contains_key(&request.intent_id)
            || previous.values().any(|fact| {
                fact.request.branch_id == request.branch_id
                    && fact.state == ModelAgentPauseState::Armed
            })
        {
            return Err(Error::Conflict(
                "model Agent pause already recorded or unresolved",
            ));
        }
        let fact = ModelAgentPauseFact {
            request,
            state: ModelAgentPauseState::Armed,
            owner_note: None,
        };
        append(&tx, owner, &fact)?;
        db(tx.commit())?;
        Ok(fact)
    }
    /// Read-only current-writer-bound factual pause view, NEVER a means
    /// of granting model Agent/Control or clearing an Unknown paid effect.
    pub fn model_agent_pause_current(
        &self,
        owner: &PrincipalId,
        session: &str,
        branch: &str,
        epoch: u64,
    ) -> Result<Option<ModelAgentPauseFact>> {
        let conn = self.connection()?;
        writer(&conn, owner, session, epoch)?;
        branch_in(&conn, session, branch)?;
        Ok(latest(&conn, session)?.into_values().find(|fact| {
            fact.request.branch_id == branch && fact.state == ModelAgentPauseState::Armed
        }))
    }
    /// Independently explicit owning host approval AFTER the original paid
    /// source Input is FACTUALLY Settled. The model cannot call this API.
    /// Unknown/Inspected/active source Turn remains held for human inspection.
    pub fn model_agent_pause_release(
        &self,
        owner: &PrincipalId,
        session: &str,
        branch: &str,
        epoch: u64,
        intent_id: &str,
        note: &str,
    ) -> Result<ModelAgentPauseFact> {
        if note.trim().is_empty() || note.len() > 256 || note.chars().any(char::is_control) {
            return Err(Error::Denied(
                "Agent pause release needs a bounded printable owner note",
            ));
        }
        let mut conn = self.connection()?;
        let tx = db(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
        writer(&tx, owner, session, epoch)?;
        branch_in(&tx, session, branch)?;
        let mut all = latest(&tx, session)?;
        let prior = all
            .remove(intent_id)
            .ok_or(Error::Denied("no exact model Agent pause to release"))?;
        if prior.request.branch_id != branch || prior.state != ModelAgentPauseState::Armed {
            return Err(Error::Conflict(
                "Agent future pause is not Armed on this branch",
            ));
        }
        let active = db(tx
            .query_row(
                "SELECT b.active_run FROM branches b WHERE b.id=?1 AND b.session_id=?2",
                params![branch, session],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional())?;
        if active.flatten().is_some() {
            return Err(Error::Conflict(
                "cannot release pause while original paid Turn is running",
            ));
        }
        let mut input_states = BTreeMap::new();
        for fact in actor_admission::events(&tx, session)? {
            input_states.insert(fact.admission.request_id.clone(), fact);
        }
        let original = input_states
            .get(&prior.request.input_request_id)
            .ok_or(Error::Denied("paused original Turn disappeared"))?;
        if original.state != actor_admission::InputAdmissionState::Settled
            || original.admission.turn_id != prior.request.turn_id
            || original.admission.sender != prior.request.sender
            || original.admission.branch_id != branch
            || original.admission.session_id != session
        {
            return Err(Error::Conflict(
                "cannot release model Agent future pause until its original Turn actually Settled",
            ));
        }
        if input_states
            .values()
            .any(|item| item.state.needs_inspection())
        {
            return Err(Error::Conflict(
                "unsettled separate Actor Input hazard requires inspection before pause release",
            ));
        }
        let result = ModelAgentPauseFact {
            state: ModelAgentPauseState::Released,
            owner_note: Some(note.to_owned()),
            ..prior
        };
        if records(&tx, session)?.len() >= MAX_EVENTS {
            return Err(Error::Limit("model Agent pause log full; require review"));
        }
        append(&tx, owner, &result)?;
        db(tx.commit())?;
        Ok(result)
    }
}
