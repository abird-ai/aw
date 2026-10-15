//! AW-P6-9D2B2A: a bounded model-requested Process **stdin-close** intent
//! over original SQLite v33 immutable Session audit, never another Process
//! runner, grant engine, Hook, Message kind, or payment/external-effect ACK.
//!
//! Prepare is durable BEFORE the original PipeProcess resource is touched.
//! Both prepare and terminal receipt require a concrete coordinated original
//! Process incarnation; an uncoordinated in-memory Store is never equivalent
//! to a verified external OS effect owner. After crash, Prepared requires
//! explicit inspection, never replay of the original paid/model Tool/OS effect.
use crate::{
    actor_admission, audit, branch_in, db, principal_is_session_owner, workspace_writer,
    SessionStore,
};
use aw_agent_core::{Error, PrincipalId, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// AW-P6-10C: canonical, domain-separated original Process Write identity.
/// Used by BOTH the trusted owning Agent Service and lower SQLite writer,
/// so no caller-supplied `mp_` alias may become a distinct effect intent.
/// Preserve the original 9D2B2A/B1 encoding for already-journaled Turns.
pub fn model_process_intent_id(session: &str, branch: &str, turn: u64) -> String {
    let mut digest = Sha256::new();
    digest.update(b"aw-model-process-one-io-intent-per-turn-v1\0");
    for value in [session.as_bytes(), branch.as_bytes(), &turn.to_be_bytes()] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value);
    }
    format!("mp_{:x}", digest.finalize())
}

const ACTION: &str = "actor.model.process";
const MAX_FACTS: usize = 4096;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProcessAction {
    CloseStdin,
    /// Only a separately original-host-registered, per-incarnation Process.
    Terminate,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProcessState {
    Prepared,
    Applied,
    Rejected,
    Unknown,
    Inspected,
}
impl ModelProcessState {
    pub const fn needs_inspection(self) -> bool {
        matches!(self, Self::Prepared | Self::Unknown)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProcessRequest {
    pub intent_id: String,
    pub input_request_id: String,
    pub session_id: String,
    pub branch_id: String,
    pub recipient_site: String,
    pub sender: String,
    pub writer_epoch: u64,
    pub turn_id: u64,
    pub handle: String,
    pub action: ModelProcessAction,
    /// Random host-minted Process incarnation for termination. Historically
    /// absent/empty for CloseStdin; NEVER deserialized from model arguments.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub process_binding: String,
}
impl ModelProcessRequest {
    fn validate(&self) -> Result<()> {
        let token = |s: &str, max: usize| {
            !s.is_empty()
                && s.len() <= max
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-.@:".contains(&b))
        };
        if !token(&self.intent_id, 128)
            || !token(&self.input_request_id, 128)
            || !token(&self.session_id, 128)
            || !token(&self.branch_id, 128)
            || !token(&self.recipient_site, 128)
            || !token(&self.sender, 128)
            || !token(&self.handle, 64)
            || self.writer_epoch == 0
            || self.turn_id == 0
            || match self.action {
                ModelProcessAction::CloseStdin => !self.process_binding.is_empty(),
                ModelProcessAction::Terminate => {
                    self.process_binding.len() != 64
                        || !self.process_binding.bytes().all(|b| b.is_ascii_hexdigit())
                }
            }
            || self.intent_id
                != model_process_intent_id(&self.session_id, &self.branch_id, self.turn_id)
        {
            return Err(Error::Denied("invalid immutable model Process intent"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProcessFact {
    pub request: ModelProcessRequest,
    pub state: ModelProcessState,
    /// No PID, Process program, argv, root, env or output bytes in audit.
    pub owner_process_id: Option<String>,
    pub owner_store_generation: Option<u64>,
    pub note: Option<String>,
}
fn events(dbconn: &rusqlite::Connection, session: &str) -> Result<Vec<ModelProcessFact>> {
    let mut q = db(dbconn.prepare(
        "SELECT data_json FROM audit WHERE session_id=?1 AND action=?2 ORDER BY seq LIMIT 4097",
    ))?;
    let rows = db(q.query_map(params![session, ACTION], |r| r.get::<_, String>(0)))?;
    let stored = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if stored.len() > MAX_FACTS {
        return Err(Error::Limit(
            "model Process audit limit requires inspection",
        ));
    }
    stored
        .iter()
        .map(|encoded| crate::decode(encoded))
        .collect()
}
fn last(records: &[ModelProcessFact], id: &str) -> Option<ModelProcessFact> {
    records
        .iter()
        .rev()
        .find(|record| record.request.intent_id == id)
        .cloned()
}
fn original_writer(
    conn: &rusqlite::Connection,
    principal: &PrincipalId,
    request: &ModelProcessRequest,
) -> Result<()> {
    if !principal_is_session_owner(conn, &request.session_id, principal)? {
        return Err(Error::Denied(
            "model Process intent requires current Session owner",
        ));
    }
    let writer = workspace_writer::verify_writer_epoch_for_session(
        conn,
        &request.session_id,
        request.writer_epoch,
    )?;
    if writer.state != "active" {
        return Err(Error::Denied("model Process writer is no longer active"));
    }
    branch_in(conn, &request.session_id, &request.branch_id)?;
    Ok(())
}
fn append(conn: &rusqlite::Connection, owner: &PrincipalId, fact: &ModelProcessFact) -> Result<()> {
    audit(
        conn,
        &fact.request.session_id,
        owner,
        ACTION,
        serde_json::to_value(fact)
            .map_err(|_| Error::Storage("cannot encode model Process audit fact"))?,
    )
}
impl SessionStore {
    /// One immutable intent per original active journaled Agent Turn,
    /// regardless of handle or ToolCall ID. Source/current-run checks occur
    /// inside the same original SQLite BEGIN IMMEDIATE writer transaction.
    pub fn model_process_prepare(
        &self,
        owner: &PrincipalId,
        request: ModelProcessRequest,
    ) -> Result<ModelProcessFact> {
        request.validate()?;
        let incarnation = self.coordinated_process()?;
        let mut conn = self.connection()?;
        let tx = db(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
        original_writer(&tx, owner, &request)?;
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
                "model Process intent requires the currently running original Turn",
            ));
        }
        let mut current = BTreeMap::new();
        for fact in actor_admission::events(&tx, &request.session_id)? {
            current.insert(fact.admission.request_id.clone(), fact);
        }
        let origin = current
            .get(&request.input_request_id)
            .ok_or(Error::Denied("model Process intent has no original Input"))?;
        if origin.state != actor_admission::InputAdmissionState::Dispatching
            || origin.admission.session_id != request.session_id
            || origin.admission.branch_id != request.branch_id
            || origin.admission.sender != request.sender
            || origin.admission.target != request.sender
            || origin.admission.turn_id != request.turn_id
            || origin.admission.writer_epoch != request.writer_epoch
        {
            return Err(Error::Denied(
                "model Process intent differs from actual original Input",
            ));
        }
        let facts = events(&tx, &request.session_id)?;
        if facts.len() >= MAX_FACTS
            || facts.iter().any(|f| {
                f.request.intent_id == request.intent_id
                    || (f.request.branch_id == request.branch_id
                        && f.request.turn_id == request.turn_id)
            })
        {
            return Err(Error::Conflict(
                "original Turn already attempted model Process effect",
            ));
        }
        let fact = ModelProcessFact {
            request,
            state: ModelProcessState::Prepared,
            owner_process_id: Some(incarnation.process_id.clone()),
            owner_store_generation: Some(incarnation.store_generation),
            note: None,
        };
        append(&tx, owner, &fact)?;
        db(tx.commit())?;
        Ok(fact)
    }
    /// Commit Applied/Rejected/Unknown only from the SAME owner Process
    /// incarnation as Prepared. A restarted owner can INSPECT but cannot
    /// manufacture success or retry OS I/O after a crash.
    pub fn model_process_transition(
        &self,
        owner: &PrincipalId,
        expected: &ModelProcessFact,
        next: ModelProcessState,
    ) -> Result<ModelProcessFact> {
        if expected.state != ModelProcessState::Prepared
            || !matches!(
                next,
                ModelProcessState::Applied
                    | ModelProcessState::Rejected
                    | ModelProcessState::Unknown
            )
        {
            return Err(Error::Denied("invalid model Process terminal transition"));
        }
        expected.request.validate()?;
        let incarnation = self.coordinated_process()?;
        let mut conn = self.connection()?;
        let tx = db(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
        original_writer(&tx, owner, &expected.request)?;
        let old = last(
            &events(&tx, &expected.request.session_id)?,
            &expected.request.intent_id,
        )
        .ok_or(Error::Denied("missing prepared model Process intent"))?;
        if &old != expected {
            return Err(Error::Conflict(
                "model Process intent changed before terminal receipt",
            ));
        }
        if old.owner_process_id.as_deref() != Some(incarnation.process_id.as_str())
            || old.owner_store_generation != Some(incarnation.store_generation)
        {
            return Err(Error::Conflict(
                "a restarted Process owner may not fabricate terminal OS effect receipt",
            ));
        }
        let fact = ModelProcessFact { state: next, ..old };
        append(&tx, owner, &fact)?;
        db(tx.commit())?;
        Ok(fact)
    }
    pub fn model_process_fact(
        &self,
        owner: &PrincipalId,
        session: &str,
        id: &str,
    ) -> Result<ModelProcessFact> {
        let conn = self.connection()?;
        if !principal_is_session_owner(&conn, session, owner)? {
            return Err(Error::Denied("model Process fact belongs to Session owner"));
        }
        last(&events(&conn, session)?, id).ok_or(Error::Denied("model Process intent not found"))
    }
    pub fn model_process_inspect(
        &self,
        owner: &PrincipalId,
        session: &str,
        id: &str,
        epoch: u64,
        note: &str,
    ) -> Result<ModelProcessFact> {
        if note.trim().is_empty() || note.len() > 512 || note.chars().any(char::is_control) {
            return Err(Error::Denied(
                "model Process inspection requires printable bounded note",
            ));
        }
        let mut conn = self.connection()?;
        let tx = db(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
        if !principal_is_session_owner(&tx, session, owner)? {
            return Err(Error::Denied(
                "model Process inspection belongs to Session owner",
            ));
        }
        workspace_writer::verify_writer_epoch_for_session(&tx, session, epoch)?;
        let old = last(&events(&tx, session)?, id)
            .ok_or(Error::Denied("model Process intent not found"))?;
        if !old.state.needs_inspection() {
            return Err(Error::Conflict("model Process receipt already terminal"));
        }
        let fact = ModelProcessFact {
            state: ModelProcessState::Inspected,
            note: Some(note.into()),
            ..old
        };
        append(&tx, owner, &fact)?;
        db(tx.commit())?;
        Ok(fact)
    }
}
