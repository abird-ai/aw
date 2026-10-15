//! AW-P6-9A. Immutable, writer-CAS-fenced Input admissions over existing
//! SQLite v33 audit facts. Nothing here persists replayable message text or
//! claims an acknowledged input was a provider/tool/OS effect receipt.
//! This is a host-only fact API; authentication is the existing SessionStore.
use crate::{
    audit, branch_in, db, principal_is_session_owner, session_in, workspace_writer, SessionStore,
};
use aw_agent_core::{Error, PrincipalId, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

const ACTION: &str = "actor.input";
const MAX_ADMISSIONS_PER_SESSION: i64 = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputAdmissionState {
    Prepared,
    Accepted,
    Dispatching,
    Settled,
    Rejected,
    Unknown,
    Inspected,
}
impl InputAdmissionState {
    pub fn needs_inspection(self) -> bool {
        matches!(
            self,
            Self::Prepared | Self::Accepted | Self::Dispatching | Self::Unknown
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorInputAdmission {
    pub request_id: String,
    pub session_id: String,
    pub branch_id: String,
    pub branch_head: String,
    pub branch_revision: u64,
    pub writer_epoch: u64,
    pub turn_id: u64,
    pub sender: String,
    pub target: String,
    pub message_id: u64,
    pub payload_sha256: String,
    pub priority: i8,
}
impl ActorInputAdmission {
    pub fn validate(&self) -> Result<()> {
        let simple = |s: &str, max: usize| {
            !s.is_empty()
                && s.len() <= max
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_:-.@".contains(&c))
        };
        if !simple(&self.request_id, 128)
            || !simple(&self.session_id, 128)
            || !simple(&self.branch_id, 128)
            || !simple(&self.branch_head, 128)
            || !simple(&self.sender, 128)
            || !simple(&self.target, 128)
            || self.writer_epoch == 0
            || self.turn_id == 0
            || self.message_id == 0
            || !(-20..=19).contains(&self.priority)
            || self.payload_sha256.len() != 64
            || !self.payload_sha256.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(Error::Denied("invalid immutable Actor Input admission"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorInputFact {
    pub admission: ActorInputAdmission,
    pub state: InputAdmissionState,
    /// Deliberately not the tool-result state. Only factual Run journal
    /// completion can cause Settled; Unknown requires explicit inspection.
    pub note: Option<String>,
}
impl ActorInputFact {
    fn new(
        admission: ActorInputAdmission,
        state: InputAdmissionState,
        note: Option<String>,
    ) -> Self {
        Self {
            admission,
            state,
            note,
        }
    }
}

pub(crate) fn events(c: &rusqlite::Connection, session: &str) -> Result<Vec<ActorInputFact>> {
    let mut query = db(c.prepare(
        "SELECT data_json FROM audit WHERE session_id=?1 AND action=?2
         ORDER BY seq ASC LIMIT 32769",
    ))?;
    let list = db(query.query_map(params![session, ACTION], |row| row.get::<_, String>(0)))?;
    let encoded = db(list.collect::<rusqlite::Result<Vec<_>>>())?;
    if encoded.len() > 32768 {
        return Err(Error::Limit("Actor Input admission ledger requires review"));
    }
    encoded
        .into_iter()
        .map(|s| crate::decode::<ActorInputFact>(&s))
        .collect()
}
fn current(events: &[ActorInputFact], request_id: &str) -> Option<ActorInputFact> {
    events
        .iter()
        .rev()
        .find(|event| event.admission.request_id == request_id)
        .cloned()
}
fn writer_guard(
    c: &rusqlite::Connection,
    actor: &PrincipalId,
    admission: &ActorInputAdmission,
    require_branch_anchor: bool,
) -> Result<()> {
    if !principal_is_session_owner(c, &admission.session_id, actor)? {
        return Err(Error::Denied(
            "only the durable Session owner may admit Actor Input",
        ));
    }
    let writer = workspace_writer::verify_writer_epoch_for_session(
        c,
        &admission.session_id,
        admission.writer_epoch,
    )?;
    if writer.state != "active" {
        return Err(Error::Denied("Workspace owner writer no longer active"));
    }
    let session = session_in(c, &admission.session_id)?;
    let branch = branch_in(c, &admission.session_id, &admission.branch_id)?;
    if branch.id != admission.branch_id || session.id != admission.session_id {
        return Err(Error::Denied("Actor Input branch/Session identity differs"));
    }
    if require_branch_anchor
        && (branch.revision != admission.branch_revision
            || branch.head != admission.branch_head
            || branch.active_run.is_some()
            || session.next_turn != admission.turn_id)
    {
        return Err(Error::Conflict(
            "Actor Input Session/branch/Turn CAS changed",
        ));
    }
    Ok(())
}
fn append(c: &rusqlite::Connection, actor: &PrincipalId, fact: &ActorInputFact) -> Result<()> {
    audit(
        c,
        &fact.admission.session_id,
        actor,
        ACTION,
        serde_json::to_value(fact)
            .map_err(|_| Error::Storage("cannot serialize Actor Input fact"))?,
    )
}
impl SessionStore {
    /// Existing SQLite owner-and-epoch test exposed as a host-only fence:
    /// every model/tool boundary must consult THIS current Workspace writer.
    pub fn actor_input_writer_fence(
        &self,
        actor: &PrincipalId,
        session: &str,
        expected_epoch: u64,
    ) -> Result<()> {
        let c = self.connection()?;
        if !principal_is_session_owner(&c, session, actor)? {
            return Err(Error::Denied("Actor controller is not the Session owner"));
        }
        workspace_writer::verify_writer_epoch_for_session(&c, session, expected_epoch)?;
        Ok(())
    }
    /// Durable *proposal*, not accepted queue state. The sole owner validates
    /// the exact existing SQLite writer epoch + branch head/revision + Turn.
    /// Duplicate IDs cannot revive even after a crash or process restart.
    pub fn actor_input_prepare(
        &self,
        actor: &PrincipalId,
        admission: ActorInputAdmission,
    ) -> Result<ActorInputFact> {
        admission.validate()?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        writer_guard(&tx, actor, &admission, true)?;
        // P6-9D2B2B2: check the one durable original Agent future pause
        // UNDER THE SAME SQLite writer transaction before allocating an
        // Input Prepared fact. A model cannot interrupt its own paid Turn;
        // this only blocks another Turn after the current one.
        crate::model_agent_pause::require_future_admission(
            &tx,
            &admission.session_id,
            &admission.branch_id,
        )?;
        let prior = events(&tx, &admission.session_id)?;
        if prior
            .iter()
            .any(|event| event.admission.request_id == admission.request_id)
            || crate::actor_control::events(&tx, &admission.session_id)?
                .iter()
                .any(|event| event.admission.request_id == admission.request_id)
            || crate::actor_queue::events(&tx, &admission.session_id)?
                .iter()
                .any(|event| event.request.request_id == admission.request_id)
            || crate::actor_notice::events(&tx, &admission.session_id)?
                .iter()
                .any(|event| event.request.request_id == admission.request_id)
        {
            return Err(Error::Conflict(
                "Actor request ID already used by Input or Control",
            ));
        }
        let mut latest = std::collections::BTreeMap::new();
        for event in &prior {
            latest.insert(&event.admission.request_id, event.state);
        }
        if latest.values().any(|state| state.needs_inspection()) {
            return Err(Error::Conflict(
                "an unsettled Actor Input blocks automatic replay",
            ));
        }
        // A pending enqueue reserves its original source sequence. One
        // queue claim may *consume its own* newly minted Runtime sequence
        // exactly once; other claims, earlier sequences and forged IDs may
        // not borrow that exemption.
        if crate::actor_notice::events(&tx, &admission.session_id)?
            .iter()
            .any(|event| {
                event.request.sender == admission.sender
                    && event.request.message_id >= admission.message_id
            })
        {
            return Err(Error::Denied(
                "Actor Input source sequence already used by Notify",
            ));
        }
        let queue = crate::actor_queue::events(&tx, &admission.session_id)?;
        let mut current_queue = std::collections::BTreeMap::new();
        for fact in queue {
            current_queue.insert(fact.request.request_id.clone(), fact);
        }
        if admission.request_id.starts_with("qi_")
            && !current_queue.values().any(|event| {
                event.state == crate::QueueState::Claimed
                    && event.claim.as_ref().is_some_and(|claim| {
                        admission.request_id
                            == crate::queued_input_request_id(
                                &admission.session_id,
                                &event.request.request_id,
                            )
                            && claim.recipient_actor == admission.sender
                            && claim.recipient_actor == admission.target
                            && claim.message_id == admission.message_id
                            && claim.turn_id == admission.turn_id
                            && claim.writer_epoch == admission.writer_epoch
                            && event.request.branch_id == admission.branch_id
                            && event.payload_sha256 == admission.payload_sha256
                            && event.request.priority == admission.priority
                    })
            })
        {
            return Err(Error::Denied(
                "reserved queue Input ID requires its exact current claim",
            ));
        }
        if current_queue.values().any(|event| {
            (event.request.sender == admission.sender
                && event.request.message_id >= admission.message_id)
                || event.claim.as_ref().is_some_and(|claim| {
                    if claim.recipient_actor != admission.sender
                        || claim.message_id < admission.message_id
                    {
                        return false;
                    }
                    !(event.state == crate::QueueState::Claimed
                        && claim.message_id == admission.message_id
                        && claim.turn_id == admission.turn_id
                        && claim.writer_epoch == admission.writer_epoch
                        && claim.recipient_actor == admission.target
                        && event.request.branch_id == admission.branch_id
                        && event.payload_sha256 == admission.payload_sha256
                        && event.request.priority == admission.priority
                        && admission.request_id
                            == crate::queued_input_request_id(
                                &admission.session_id,
                                &event.request.request_id,
                            ))
                })
        }) {
            return Err(Error::Denied(
                "Actor Input sender sequence was already reserved by the queue",
            ));
        }
        let count: i64 = db(tx.query_row(
            "SELECT count(*) FROM audit WHERE session_id=?1 AND action=?2",
            params![admission.session_id, ACTION],
            |row| row.get(0),
        ))?;
        if count >= MAX_ADMISSIONS_PER_SESSION {
            return Err(Error::Limit("Actor Input audit capacity reached"));
        }
        let fact = ActorInputFact::new(admission, InputAdmissionState::Prepared, None);
        append(&tx, actor, &fact)?;
        db(tx.commit())?;
        Ok(fact)
    }
    /// Monotonic immutable transition. An Accepted is an inbox-acceptance FACT
    /// only; Dispatching is a guarded claim, NOT evidence of a model call.
    pub fn actor_input_transition(
        &self,
        actor: &PrincipalId,
        admission: &ActorInputAdmission,
        expect: InputAdmissionState,
        next: InputAdmissionState,
    ) -> Result<ActorInputFact> {
        admission.validate()?;
        let legal = matches!(
            (expect, next),
            (InputAdmissionState::Prepared, InputAdmissionState::Accepted)
                | (InputAdmissionState::Prepared, InputAdmissionState::Rejected)
                | (InputAdmissionState::Prepared, InputAdmissionState::Unknown)
                | (
                    InputAdmissionState::Accepted,
                    InputAdmissionState::Dispatching
                )
                | (InputAdmissionState::Accepted, InputAdmissionState::Unknown)
                | (
                    InputAdmissionState::Dispatching,
                    InputAdmissionState::Unknown
                )
                | (
                    InputAdmissionState::Dispatching,
                    InputAdmissionState::Settled
                )
        );
        if !legal {
            return Err(Error::Denied("invalid Actor Input fact transition"));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        writer_guard(
            &tx,
            actor,
            admission,
            next != InputAdmissionState::Settled
                && next != InputAdmissionState::Unknown
                && next != InputAdmissionState::Rejected,
        )?;
        let events = events(&tx, &admission.session_id)?;
        let old = current(&events, &admission.request_id)
            .ok_or(Error::Denied("no prepared Actor Input exists"))?;
        if old.admission != *admission || old.state != expect {
            return Err(Error::Conflict(
                "Actor Input changed or CAS already advanced",
            ));
        }
        if next == InputAdmissionState::Settled {
            let result: Option<(String, Option<String>)> = db(tx
                .query_row(
                    "SELECT r.status,h.state FROM runs r
                 LEFT JOIN hazards h ON h.run_id=r.id
                 WHERE r.session_id=?1 AND r.branch_id=?2 AND r.turn_id=?3
                 ORDER BY r.started_at DESC LIMIT 1",
                    params![admission.session_id, admission.branch_id, admission.turn_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional())?;
            if !matches!(result.as_ref(),
                Some((status,hazard))
                if matches!(status.as_str(),"completed"|"failed"|"cancelled"|"incomplete")
                    && !matches!(hazard.as_deref(),Some("running"|"needs_inspection"))
            ) {
                return Err(Error::Denied(
                    "Actor Input cannot settle without final durable Turn and resolved external hazards"
                ));
            }
        }
        let fact = ActorInputFact::new(admission.clone(), next, None);
        append(&tx, actor, &fact)?;
        db(tx.commit())?;
        Ok(fact)
    }
    /// Read-only inspection: an unfinished admission is NOT a replay token.
    pub fn actor_input_fact(
        &self,
        actor: &PrincipalId,
        session: &str,
        request_id: &str,
    ) -> Result<ActorInputFact> {
        let c = self.connection()?;
        if !principal_is_session_owner(&c, session, actor)? {
            return Err(Error::Denied("Actor queue facts belong to Session owner"));
        }
        current(&events(&c, session)?, request_id)
            .ok_or(Error::Denied("Actor Input admission not found"))
    }
    /// Human-reviewed disposition of a prepared, accepted, dispatched or
    /// Unknown admission. Never starts/resumes any model/tool/process effect
    /// and cannot acknowledge the independent TurnJournal's hazards.
    pub fn actor_input_inspect(
        &self,
        actor: &PrincipalId,
        session: &str,
        request_id: &str,
        expected_writer_epoch: u64,
        note: &str,
    ) -> Result<ActorInputFact> {
        if note.trim().is_empty() || note.len() > 4096 || note.chars().any(char::is_control) {
            return Err(Error::Config(
                "inspection note must be bounded and printable".into(),
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        if !principal_is_session_owner(&tx, session, actor)? {
            return Err(Error::Denied("only Session owner can inspect Actor queue"));
        }
        workspace_writer::verify_writer_epoch_for_session(&tx, session, expected_writer_epoch)?;
        let facts = events(&tx, session)?;
        let old =
            current(&facts, request_id).ok_or(Error::Denied("Actor Input admission not found"))?;
        if !old.state.needs_inspection() {
            return Err(Error::Conflict("Actor Input admission already terminal"));
        }
        // The note does not remove the original immutable fact or make the
        // same request_id eligible for retry; a new explicit user request is
        // a *new* operation with its own id and current branch revision.
        let result = ActorInputFact::new(
            old.admission,
            InputAdmissionState::Inspected,
            Some(note.to_owned()),
        );
        append(&tx, actor, &result)?;
        db(tx.commit())?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionBinding;
    use std::path::PathBuf;
    fn owner() -> PrincipalId {
        PrincipalId::new("local").unwrap()
    }
    fn store() -> (SessionStore, ActorInputAdmission) {
        let store = SessionStore::in_memory().unwrap();
        let user = owner();
        let (session, branch) = store
            .create(
                &user,
                "Actor queue",
                SessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "policy".into(),
                    profile_hint: "p".into(),
                    model_hint: "m".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let writer = store
            .workspace_writer(&user, None, &session.workspace_id)
            .unwrap();
        let admission = ActorInputAdmission {
            request_id: "req1".into(),
            session_id: session.id,
            branch_id: branch.id,
            branch_head: branch.head,
            branch_revision: branch.revision,
            writer_epoch: writer.writer_epoch,
            turn_id: session.next_turn,
            sender: "actor:1:1:1".into(),
            target: "actor:1:2:1".into(),
            message_id: 1,
            payload_sha256: "e".repeat(64),
            priority: 0,
        };
        (store, admission)
    }
    #[test]
    fn sqlite_prepare_accept_dispatch_is_cas_and_never_a_receipt() {
        let (store, request) = store();
        let principal = owner();
        let prepared = store
            .actor_input_prepare(&principal, request.clone())
            .unwrap();
        assert_eq!(prepared.state, InputAdmissionState::Prepared);
        assert!(prepared.state.needs_inspection());
        assert!(store
            .actor_input_prepare(&principal, request.clone())
            .is_err());
        let mut bad = request.clone();
        bad.writer_epoch += 1;
        assert!(store
            .actor_input_transition(
                &principal,
                &bad,
                InputAdmissionState::Prepared,
                InputAdmissionState::Accepted
            )
            .is_err());
        bad = request.clone();
        bad.payload_sha256 = "f".repeat(64);
        assert!(store
            .actor_input_transition(
                &principal,
                &bad,
                InputAdmissionState::Prepared,
                InputAdmissionState::Accepted
            )
            .is_err());
        assert!(store
            .actor_input_transition(
                &principal,
                &request,
                InputAdmissionState::Prepared,
                InputAdmissionState::Dispatching
            )
            .is_err());
        store
            .actor_input_transition(
                &principal,
                &request,
                InputAdmissionState::Prepared,
                InputAdmissionState::Accepted,
            )
            .unwrap();
        assert!(store
            .actor_input_prepare(
                &principal,
                ActorInputAdmission {
                    request_id: "another".into(),
                    ..request.clone()
                }
            )
            .is_err());
        assert!(store
            .actor_input_transition(
                &principal,
                &request,
                InputAdmissionState::Dispatching,
                InputAdmissionState::Settled
            )
            .is_err());
        store
            .actor_input_transition(
                &principal,
                &request,
                InputAdmissionState::Accepted,
                InputAdmissionState::Dispatching,
            )
            .unwrap();
        assert_eq!(
            store
                .actor_input_fact(&principal, &request.session_id, "req1")
                .unwrap()
                .state,
            InputAdmissionState::Dispatching
        );
        assert!(store
            .actor_input_prepare(
                &PrincipalId::new("another").unwrap(),
                ActorInputAdmission {
                    request_id: "other".into(),
                    ..request.clone()
                }
            )
            .is_err());
        store
            .actor_input_transition(
                &principal,
                &request,
                InputAdmissionState::Dispatching,
                InputAdmissionState::Unknown,
            )
            .unwrap();
        assert!(store
            .actor_input_prepare(
                &principal,
                ActorInputAdmission {
                    request_id: "another".into(),
                    ..request.clone()
                }
            )
            .is_err());
        store
            .actor_input_inspect(
                &principal,
                &request.session_id,
                "req1",
                request.writer_epoch,
                "owner confirmed no automatic replay",
            )
            .unwrap();
        let next = ActorInputAdmission {
            request_id: "new-explicit-user-request".into(),
            ..request.clone()
        };
        store.actor_input_prepare(&principal, next).unwrap();
        assert_eq!(
            store
                .actor_input_fact(&principal, &request.session_id, "req1")
                .unwrap()
                .state,
            InputAdmissionState::Inspected
        );
        assert!(store.actor_input_prepare(&principal, request).is_err());
    }
    #[test]
    fn queued_input_rejects_stale_writer_branch_and_unbounded_metadata() {
        let (store, request) = store();
        let user = owner();
        for modified in [
            ActorInputAdmission {
                writer_epoch: request.writer_epoch + 1,
                ..request.clone()
            },
            ActorInputAdmission {
                branch_revision: request.branch_revision + 1,
                ..request.clone()
            },
            ActorInputAdmission {
                branch_head: "not_current".into(),
                ..request.clone()
            },
            ActorInputAdmission {
                turn_id: request.turn_id + 1,
                ..request.clone()
            },
            ActorInputAdmission {
                request_id: "".into(),
                ..request.clone()
            },
            ActorInputAdmission {
                priority: 20,
                ..request.clone()
            },
            ActorInputAdmission {
                payload_sha256: "aaa".into(),
                ..request.clone()
            },
            ActorInputAdmission {
                sender: "bad source with spaces".into(),
                ..request.clone()
            },
        ] {
            assert!(store.actor_input_prepare(&user, modified).is_err());
        }
        let current = store.actor_input_prepare(&user, request.clone()).unwrap();
        assert_eq!(current.state, InputAdmissionState::Prepared);
        store
            .actor_input_transition(
                &user,
                &request,
                InputAdmissionState::Prepared,
                InputAdmissionState::Rejected,
            )
            .unwrap();
        assert_eq!(
            store
                .actor_input_fact(&user, &request.session_id, &request.request_id)
                .unwrap()
                .state,
            InputAdmissionState::Rejected
        );
        assert!(store
            .actor_input_inspect(
                &user,
                &request.session_id,
                &request.request_id,
                request.writer_epoch,
                "cannot inspect a rejected request"
            )
            .is_err());
    }
}
