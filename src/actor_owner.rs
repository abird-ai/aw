//! AW-P6-9A: host-only, SQLite-writer-fenced admission of one real Actor Input.
//!
//! One owner, one original Agent, one original durable Session/TurnJournal.
//! The SQLite audit is immutable *admission evidence*, not a replayable queue,
//! provider receipt, new model loop, authority grant or an RPC endpoint.
//! Future multi-message queues and external RPC need a separately tested CAS.
use aw_agent_core::{
    control::{AgentSessionController, WriterEpochFence},
    runtime::{Delivery, HostRuntime},
    Agent, AgentEvent, Error, PrincipalId, Result, Turn,
};
use aw_agent_kernel::{
    actor::ActorKey,
    message::{ActorMessage, ControlKind, Kind, Scope, TurnTarget},
};
use aw_store::{
    model_agent_pause_id, model_process_intent_id, model_queue_disposition_id,
    model_queue_request_id, queued_input_request_id, ActorControlAction, ActorControlAdmission,
    ActorControlFact, ActorControlState, ActorInputAdmission, ActorInputFact, ActorNoticeFact,
    ActorNoticeRequest, ActorNoticeState, ActorNoticeView, ActorQueueFact, ActorQueueRequest,
    ActorQueueView, CustodySweep, InputAdmissionState, ModelAgentPauseFact, ModelAgentPauseRequest,
    ModelProcessAction, ModelProcessFact, ModelProcessRequest, ModelProcessState,
    ModelQueueDispositionKind, ModelQueueDispositionRequest, QueueTarget, SessionStore,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    num::NonZeroU64,
    sync::{Arc, Mutex},
};

/// Canonical typed *host-side* operation vocabulary for an eventual R12
/// authenticated transport. Deserializing one of these records grants NOTHING:
/// the bound Service must independently verify owner, epoch, Session, active
/// Turn and actual Runtime source before any Control effect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActorOperation {
    QueryInput {
        request_id: String,
    },
    QueryControl {
        request_id: String,
    },
    ListControls,
    Control {
        request_id: String,
        message_id: u64,
        action: ActorControlAction,
    },
    InspectInput {
        request_id: String,
        note: String,
    },
    InspectControl {
        request_id: String,
        note: String,
    },
    QueryQueue {
        request_id: String,
    },
    ListQueue,
    DeleteQueue {
        request_id: String,
    },
    InspectQueue {
        request_id: String,
        note: String,
    },
    QueryNotice {
        request_id: String,
    },
    ListNotices,
    AckNotice {
        request_id: String,
        revision: u64,
    },
    InspectNotice {
        request_id: String,
        note: String,
    },
}
impl ActorOperation {
    /// Strictly parse untrusted R12 operations including unit enum variants:
    /// serde's derived internally tagged enum does not reject extra fields
    /// for unit cases even with deny_unknown_fields. A client may NEVER pass
    /// an ActorKey, owner, source, writer epoch, site or arbitrary grant.
    pub fn from_rpc_value(value: serde_json::Value) -> Result<Self> {
        let object = value.as_object().ok_or(Error::Protocol(
            "Actor RPC operation must be a typed object",
        ))?;
        let action = object
            .get("op")
            .and_then(serde_json::Value::as_str)
            .ok_or(Error::Protocol("Actor RPC operation type missing"))?;
        let fields: &[&str] = match action {
            "query_input" | "query_control" | "query_queue" | "delete_queue" | "query_notice" => {
                &["op", "request_id"]
            }
            "list_controls" | "list_queue" | "list_notices" => &["op"],
            "inspect_input" | "inspect_control" | "inspect_queue" | "inspect_notice" => {
                &["op", "request_id", "note"]
            }
            "ack_notice" => &["op", "request_id", "revision"],
            "control" => &["op", "request_id", "message_id", "action"],
            _ => return Err(Error::Protocol("unknown Actor RPC operation")),
        };
        if object.len() != fields.len() || fields.iter().any(|field| !object.contains_key(*field)) {
            return Err(Error::Protocol(
                "Actor RPC operation contains unknown or missing fields",
            ));
        }
        serde_json::from_value(value)
            .map_err(|_| Error::Protocol("invalid typed Actor RPC operation"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActorOperationResult {
    Input { fact: ActorInputFact },
    Control { fact: ActorControlFact },
    Controls { facts: Vec<ActorControlFact> },
    Queue { fact: ActorQueueView },
    Queued { facts: Vec<ActorQueueView> },
    Notice { fact: ActorNoticeView },
    Notices { facts: Vec<ActorNoticeView> },
}
/// Explicit bounded white-listed public Actor facts; never serialize raw
/// Input digests, ciphertext refs, process identity or issuer credentials.
pub fn public_operation(result: ActorOperationResult) -> serde_json::Value {
    match result {
        ActorOperationResult::Input { fact } => serde_json::json!({
            "kind":"input","request_id":fact.admission.request_id,
            "state":fact.state,"turn_id":fact.admission.turn_id,"note":fact.note,
        }),
        ActorOperationResult::Control { fact } => serde_json::json!({
            "kind":"control","request_id":fact.admission.request_id,
            "state":fact.state,"action":fact.admission.action,
            "turn_id":fact.admission.turn_id,"note":fact.note,
        }),
        ActorOperationResult::Controls { facts } => serde_json::json!({
            "kind":"controls","facts":facts.iter().map(|fact|serde_json::json!({
                "request_id":fact.admission.request_id,"state":fact.state,
                "action":fact.admission.action,"turn_id":fact.admission.turn_id,
                "note":fact.note,
            })).collect::<Vec<_>>(),
        }),
        ActorOperationResult::Queue { fact } => serde_json::json!({"kind":"queue","fact":fact}),
        ActorOperationResult::Queued { facts } => {
            serde_json::json!({"kind":"queued","facts":facts})
        }
        ActorOperationResult::Notice { fact } => serde_json::json!({"kind":"notice","fact":fact}),
        ActorOperationResult::Notices { facts } => {
            serde_json::json!({"kind":"notices","facts":facts})
        }
    }
}

/// Existing host-only current-Turn `Service::control` is the only mutable
/// signal path. R12's independent reader may invoke it on an *already*
/// journal-dispatching Actor while the owning Agent is awaiting a provider.
impl crate::headless::LiveControlPort for Service {
    fn apply(&self, operation: serde_json::Value) -> Result<serde_json::Value> {
        let (id, message_id, action) = match ActorOperation::from_rpc_value(operation)? {
            ActorOperation::Control {
                request_id,
                message_id,
                action,
            } => (request_id, message_id, action),
            _ => {
                return Err(Error::Denied(
                    "live Actor port accepts only exact typed Control",
                ))
            }
        };
        self.controller.verify_current_binding()?;
        let fact = self.control(id, action, message_id)?;
        Ok(public_operation(ActorOperationResult::Control { fact }))
    }
}

struct DurableFence {
    store: SessionStore,
    owner: PrincipalId,
    session: String,
    epoch: u64,
}
impl WriterEpochFence for DurableFence {
    fn verify(&self) -> Result<()> {
        self.store
            .actor_input_writer_fence(&self.owner, &self.session, self.epoch)
    }
}
#[derive(Default)]
struct State {
    input: Option<ActorInputAdmission>,
    busy: bool,
    poisoned: bool,
    claiming: bool,
}
/// Host-owned first-party durable admission adapter. Construct only from a
/// real current SQLite Session writer and a Session-bound Runtime Actor.
#[derive(Clone)]
pub struct Service {
    store: SessionStore,
    owner: PrincipalId,
    actor: ActorKey,
    epoch: NonZeroU64,
    session: String,
    branch: String,
    site: String,
    controller: AgentSessionController,
    state: Arc<Mutex<State>>,
}
struct RunGuard {
    state: Arc<Mutex<State>>,
    finished: bool,
}
impl RunGuard {
    fn done(&mut self) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Storage("Actor owner state lock poisoned"))?;
        state.busy = false;
        state.input = None;
        self.finished = true;
        Ok(())
    }
}
impl Drop for RunGuard {
    fn drop(&mut self) {
        if !self.finished {
            if let Ok(mut state) = self.state.lock() {
                state.poisoned = true;
                state.busy = false;
            }
        }
    }
}
fn label(key: ActorKey) -> String {
    format!(
        "r{}g{}a{}g{}",
        key.runtime.id, key.runtime.generation, key.id, key.generation
    )
}
impl Service {
    pub fn new(
        store: SessionStore,
        runtime: Arc<Mutex<HostRuntime>>,
        owner: PrincipalId,
        actor: ActorKey,
        epoch: NonZeroU64,
        session: impl Into<String>,
        branch: impl Into<String>,
    ) -> Result<Self> {
        let session = session.into();
        let branch = branch.into();
        store.actor_input_writer_fence(&owner, &session, epoch.get())?;
        let stored = store.session(&owner, &session)?;
        if stored.owner != owner {
            return Err(Error::Denied(
                "durable Actor owner must own the actual Session",
            ));
        }
        let branch_row = store
            .branches(&owner, &session)?
            .into_iter()
            .find(|row| row.id == branch)
            .ok_or(Error::Denied("Actor owner branch unavailable"))?;
        if branch_row.active_run.is_some() {
            return Err(Error::Conflict("Actor owner cannot attach to busy branch"));
        }
        let site = {
            let runtime = runtime
                .lock()
                .map_err(|_| Error::Storage("Actor Runtime lock poisoned"))?;
            let binding = runtime.session_binding(&owner, actor, Some(epoch))?;
            if binding.session_id() != session {
                return Err(Error::Denied(
                    "Actor Runtime binding differs from durable Session",
                ));
            }
            if runtime.pending(&owner, actor, Some(epoch))? != 0 {
                return Err(Error::Conflict(
                    "Actor owner attaches only to empty in-memory mailbox",
                ));
            }
            runtime
                .envelope(&owner, actor, Some(epoch))?
                .site()
                .to_owned()
        };
        let fence = Arc::new(DurableFence {
            store: store.clone(),
            owner: owner.clone(),
            session: session.clone(),
            epoch: epoch.get(),
        });
        let controller = AgentSessionController::new(runtime, owner.clone(), actor, Some(epoch))?
            .with_writer_fence(fence)?;
        Ok(Self {
            store,
            owner,
            actor,
            epoch,
            session,
            branch,
            site,
            controller,
            state: Arc::new(Mutex::new(State::default())),
        })
    }
    /// One accepted Input per current branch view. We never persist prompt
    /// text in audit; the original Agent's TurnJournal records user text ONLY
    /// after a guarded claim and factual TurnStart. A repeated request ID
    /// cannot re-enter even after process restart.
    pub fn submit(
        &self,
        agent: &Agent,
        request_id: impl Into<String>,
        message: ActorMessage,
    ) -> Result<ActorInputFact> {
        self.submit_inner(agent, request_id, message, false)
    }
    fn submit_inner(
        &self,
        agent: &Agent,
        request_id: impl Into<String>,
        message: ActorMessage,
        from_queue: bool,
    ) -> Result<ActorInputFact> {
        let request_id = request_id.into();
        if !from_queue && request_id.starts_with("qi_") {
            return Err(Error::Denied(
                "queue-derived Input IDs are reserved for owning Runtime",
            ));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Storage("Actor owner state lock poisoned"))?;
        if state.poisoned || state.busy || state.input.is_some() || (state.claiming && !from_queue)
        {
            return Err(Error::Conflict("Actor owner has unsettled accepted Input"));
        }
        self.controller.validate_agent_binding(agent)?;
        let scope = agent.journal_scope()?.ok_or(Error::Denied(
            "durable Actor owner needs an actual Session/branch journal",
        ))?;
        if scope.session_id != self.session || scope.branch_id != self.branch {
            return Err(Error::Denied(
                "Actor input Agent journal branch differs from owner binding",
            ));
        }
        if !agent.is_journaled()
            || agent.default_identity()?.actor != self.owner
            || message.kind != Kind::Input
            || message.to != self.actor
            || message.id != message.source_sequence
            || !matches!(message.turn,TurnTarget::Exact(id)
                if id==agent.history_state().next_turn_id)
        {
            return Err(Error::Denied(
                "Actor owner requires journaled exact-Turn Input",
            ));
        }
        self.store
            .actor_input_writer_fence(&self.owner, &self.session, self.epoch.get())?;
        let session = self.store.session(&self.owner, &self.session)?;
        let branch = self
            .store
            .branches(&self.owner, &self.session)?
            .into_iter()
            .find(|row| row.id == self.branch)
            .ok_or(Error::Denied("Actor owner branch disappeared"))?;
        if branch.active_run.is_some() || session.next_turn != agent.history_state().next_turn_id {
            return Err(Error::Conflict(
                "Actor Input branch already advanced or occupied",
            ));
        }
        let admission = ActorInputAdmission {
            request_id,
            session_id: self.session.clone(),
            branch_id: self.branch.clone(),
            branch_head: branch.head,
            branch_revision: branch.revision,
            writer_epoch: self.epoch.get(),
            turn_id: session.next_turn,
            sender: label(message.from),
            target: label(message.to),
            message_id: message.id,
            payload_sha256: format!("{:x}", Sha256::digest(message.payload.as_bytes())),
            priority: message.priority,
        };
        self.store
            .actor_input_prepare(&self.owner, admission.clone())?;
        let delivered = self.controller.send(message, admission.turn_id);
        match delivered {
            Ok(Delivery::Queued(_)) => {
                // The in-memory queue now contains the message. Until this
                // immutable acceptance ACK commits, it MUST NOT dispatch.
                state.input = Some(admission.clone());
                match self.store.actor_input_transition(
                    &self.owner,
                    &admission,
                    InputAdmissionState::Prepared,
                    InputAdmissionState::Accepted,
                ) {
                    Ok(fact) => Ok(fact),
                    Err(error) => {
                        state.poisoned = true;
                        Err(error)
                    }
                }
            }
            Ok(Delivery::Consumed) | Err(_) => {
                if self
                    .store
                    .actor_input_transition(
                        &self.owner,
                        &admission,
                        InputAdmissionState::Prepared,
                        InputAdmissionState::Rejected,
                    )
                    .is_err()
                {
                    state.poisoned = true;
                }
                Err(Error::Denied(
                    "Actor Input was rejected/consumed before durable acceptance",
                ))
            }
            Ok(Delivery::Deferred(_)) | Ok(Delivery::Applied(_)) => {
                // A deferred admission lacks a durable payload/continuation;
                // an Accepted record would falsely imply a dispatchable Input.
                state.poisoned = true;
                let _ = self.store.actor_input_transition(
                    &self.owner,
                    &admission,
                    InputAdmissionState::Prepared,
                    InputAdmissionState::Unknown,
                );
                Err(Error::Denied(
                    "deferred Actor Input requires inspection; never replay",
                ))
            }
        }
    }
    /// One canonical typed owner API; no new listener, request dispatcher,
    /// provider loop or user-visible CLI parser. The same current SQLite
    /// writer fence and original session grant apply at each operation.
    pub fn operate(&self, op: ActorOperation) -> Result<ActorOperationResult> {
        match op {
            ActorOperation::QueryInput { request_id } => Ok(ActorOperationResult::Input {
                fact: self.fact(&request_id)?,
            }),
            ActorOperation::QueryControl { request_id } => Ok(ActorOperationResult::Control {
                fact: self.control_fact(&request_id)?,
            }),
            ActorOperation::ListControls => Ok(ActorOperationResult::Controls {
                facts: self.controls()?,
            }),
            ActorOperation::Control {
                request_id,
                message_id,
                action,
            } => Ok(ActorOperationResult::Control {
                fact: self.control(request_id, action, message_id)?,
            }),
            ActorOperation::InspectInput { request_id, note } => Ok(ActorOperationResult::Input {
                fact: self.store.actor_input_inspect(
                    &self.owner,
                    &self.session,
                    &request_id,
                    self.epoch.get(),
                    &note,
                )?,
            }),
            ActorOperation::InspectControl { request_id, note } => {
                Ok(ActorOperationResult::Control {
                    fact: self.inspect_control(&request_id, &note)?,
                })
            }
            ActorOperation::QueryQueue { request_id } => Ok(ActorOperationResult::Queue {
                fact: (&self.queue_fact(&request_id)?).into(),
            }),
            ActorOperation::ListQueue => Ok(ActorOperationResult::Queued {
                facts: self
                    .queue_list()?
                    .iter()
                    .map(ActorQueueView::from)
                    .collect(),
            }),
            ActorOperation::DeleteQueue { request_id } => Ok(ActorOperationResult::Queue {
                fact: (&self.store.actor_queue_delete(
                    &self.owner,
                    &self.session,
                    &request_id,
                    self.epoch.get(),
                )?)
                    .into(),
            }),
            ActorOperation::InspectQueue { request_id, note } => Ok(ActorOperationResult::Queue {
                fact: (&self.store.actor_queue_inspect(
                    &self.owner,
                    &self.session,
                    &request_id,
                    self.epoch.get(),
                    &note,
                )?)
                    .into(),
            }),
            ActorOperation::QueryNotice { request_id } => Ok(ActorOperationResult::Notice {
                fact: (&self.notice_fact(&request_id)?).into(),
            }),
            ActorOperation::ListNotices => Ok(ActorOperationResult::Notices {
                facts: self.notices()?.iter().map(ActorNoticeView::from).collect(),
            }),
            ActorOperation::AckNotice {
                request_id,
                revision,
            } => Ok(ActorOperationResult::Notice {
                fact: (&self.store.actor_notice_ack_read(
                    &self.owner,
                    &self.session,
                    &request_id,
                    self.epoch.get(),
                    revision,
                )?)
                    .into(),
            }),
            ActorOperation::InspectNotice { request_id, note } => {
                Ok(ActorOperationResult::Notice {
                    fact: (&self.store.actor_notice_inspect(
                        &self.owner,
                        &self.session,
                        &request_id,
                        self.epoch.get(),
                        &note,
                    )?)
                        .into(),
                })
            }
        }
    }
    /// Preserve a child/Process's observational Notify before sending it
    /// through the SAME original Runtime mailbox. The original Actor check
    /// proves current generation, owner, child-parent relation and site;
    /// store Prepared alone never implies Runtime delivery.
    pub fn record_notice(
        &self,
        request_id: impl Into<String>,
        message: ActorMessage,
    ) -> Result<ActorNoticeFact> {
        if message.kind != Kind::Notify
            || message.from == self.actor
            || message.to != self.actor
            || message.id == 0
            || message.id != message.source_sequence
        {
            return Err(Error::Denied(
                "Notify requires a distinct child source and this bound parent",
            ));
        }
        let active_turn = {
            let state = self
                .state
                .lock()
                .map_err(|_| Error::Storage("Actor owner lock poisoned"))?;
            if state.poisoned || state.claiming {
                return Err(Error::Conflict("Notify owner is uncertain"));
            }
            state.input.as_ref().map(|input| input.turn_id)
        };
        self.store
            .actor_input_writer_fence(&self.owner, &self.session, self.epoch.get())?;
        let current_turn =
            active_turn.unwrap_or(self.store.session(&self.owner, &self.session)?.next_turn);
        if matches!(message.turn, TurnTarget::Exact(t) if t < current_turn || t == 0) {
            return Err(Error::Denied("Notify exact target is already settled"));
        }
        let fact = self.store.actor_notice_prepare(
            &self.owner,
            ActorNoticeRequest {
                request_id: request_id.into(),
                session_id: self.session.clone(),
                branch_id: self.branch.clone(),
                recipient_site: self.site.clone(),
                sender: label(message.from),
                recipient: label(message.to),
                message_id: message.id,
                writer_epoch: self.epoch.get(),
                turn: match message.turn {
                    TurnTarget::Auto => QueueTarget::Auto,
                    TurnTarget::Exact(t) => QueueTarget::Exact(t),
                },
                priority: message.priority,
            },
            &message.payload,
        )?;
        let outcome = match self.controller.send(message, current_turn) {
            Ok(Delivery::Queued(_)) => ActorNoticeState::Accepted,
            Ok(Delivery::Consumed) => ActorNoticeState::Rejected,
            Ok(Delivery::Deferred(_) | Delivery::Applied(_)) | Err(_) => ActorNoticeState::Unknown,
        };
        let result = self
            .store
            .actor_notice_transition(&self.owner, &fact, outcome);
        if outcome == ActorNoticeState::Unknown || result.is_err() {
            if let Ok(mut state) = self.state.lock() {
                state.poisoned = true;
            }
        }
        let fact = result?;
        if outcome == ActorNoticeState::Unknown {
            return Err(Error::Conflict(
                "Notify delivery uncertain; inspect and never resend",
            ));
        }
        Ok(fact)
    }

    pub fn notices(&self) -> Result<Vec<ActorNoticeFact>> {
        self.store.actor_notice_list(&self.owner, &self.session)
    }
    pub fn notice_fact(&self, request_id: &str) -> Result<ActorNoticeFact> {
        self.store
            .actor_notice_fact(&self.owner, &self.session, request_id)
    }
    /// Current authenticated host may read the private observation AFTER
    /// actual mailbox acceptance, including following a Runtime restart.
    /// This neither polls nor re-injects the old Actor Message.
    pub fn notice_private(&self, id: &str) -> Result<zeroize::Zeroizing<String>> {
        self.controller.verify_current_binding()?;
        let fact = self.notice_fact(id)?;
        // A fresh Actor generation may view the current owner's old
        // observation without reminting the old child Actor or mailbox.
        if fact.request.branch_id != self.branch || fact.request.recipient_site != self.site {
            return Err(Error::Denied(
                "Notify private read is for a different recipient",
            ));
        }
        self.store
            .actor_notice_read(&self.owner, &self.session, id, self.epoch.get())
    }

    /// Consume one *original* volatile Runtime Notify without conflating it
    /// with durable user acknowledgment. Each consumed message must match a
    /// previously Accepted encrypted receipt exactly (including generation
    /// labels, sequence, priority and SHA), or the owner is poisoned and the
    /// caller must inspect. Private text remains available for the current
    /// owner even if this in-memory poll was interrupted or Runtime restarted.
    pub fn poll_notice_inbox(&self) -> Result<Option<ActorNoticeView>> {
        self.store
            .actor_input_writer_fence(&self.owner, &self.session, self.epoch.get())?;
        let active = {
            let state = self
                .state
                .lock()
                .map_err(|_| Error::Storage("Actor owner lock poisoned"))?;
            if state.poisoned || state.claiming {
                return Err(Error::Conflict("Notify owner needs inspection"));
            }
            state.input.as_ref().map(|admission| admission.turn_id)
        };
        let turn = active.unwrap_or(self.store.session(&self.owner, &self.session)?.next_turn);
        let Some(message) = self.controller.poll_notification(turn)? else {
            return Ok(None);
        };
        let notices = self.notices()?;
        let found = notices.iter().find(|notice| {
            notice.state == ActorNoticeState::Accepted
                && notice.request.sender == label(message.from)
                && notice.request.recipient == label(message.to)
                && notice.request.recipient_site == self.site
                && notice.request.branch_id == self.branch
                && notice.request.message_id == message.id
                && message.id == message.source_sequence
                && notice.request.turn
                    == match message.turn {
                        TurnTarget::Auto => QueueTarget::Auto,
                        TurnTarget::Exact(t) => QueueTarget::Exact(t),
                    }
                && notice.request.priority == message.priority
                && notice.payload_sha256
                    == format!("{:x}", Sha256::digest(message.payload.as_bytes()))
        });
        match found {
            Some(fact) => Ok(Some(fact.into())),
            None => {
                if let Ok(mut state) = self.state.lock() {
                    state.poisoned = true;
                }
                Err(Error::Denied(
                    "Runtime Notify lacks exact immutable acceptance; inspect",
                ))
            }
        }
    }

    /// Explicit owner-only maintenance; cannot sweep unknown/unlogged
    /// ciphertext, credential refs, live Input, or unacknowledged notices.
    pub fn reclaim_retired_custody(&self, limit: usize) -> Result<CustodySweep> {
        self.store.actor_reclaim_retired_custody(
            &self.owner,
            &self.session,
            self.epoch.get(),
            limit,
        )
    }

    /// AW-P6-9C owning-host operation gate. Unlike the internal same-owner
    /// metadata SDK, a transport may see only the currently *selected* branch
    /// and site. Recheck original Actor generation/Envelope and SQLite writer
    /// for EVERY frame; caller-supplied ActorKey/site/epoch never authorizes.
    pub fn operate_bound(&self, op: ActorOperation) -> Result<ActorOperationResult> {
        self.store
            .actor_input_writer_fence(&self.owner, &self.session, self.epoch.get())?;
        self.controller.verify_current_binding()?;
        let target = match &op {
            ActorOperation::QueryInput { request_id }
            | ActorOperation::InspectInput { request_id, .. } => {
                let fact = self.fact(request_id)?;
                (fact.admission.branch_id == self.branch).then_some(())
            }
            ActorOperation::QueryControl { request_id }
            | ActorOperation::InspectControl { request_id, .. } => {
                let fact = self.control_fact(request_id)?;
                (fact.admission.branch_id == self.branch).then_some(())
            }
            ActorOperation::QueryQueue { request_id }
            | ActorOperation::DeleteQueue { request_id }
            | ActorOperation::InspectQueue { request_id, .. } => {
                let fact = self.queue_fact(request_id)?;
                (fact.request.branch_id == self.branch && fact.request.recipient_site == self.site)
                    .then_some(())
            }
            ActorOperation::QueryNotice { request_id }
            | ActorOperation::AckNotice { request_id, .. }
            | ActorOperation::InspectNotice { request_id, .. } => {
                let fact = self.notice_fact(request_id)?;
                (fact.request.branch_id == self.branch && fact.request.recipient_site == self.site)
                    .then_some(())
            }
            ActorOperation::ListControls
            | ActorOperation::ListQueue
            | ActorOperation::ListNotices => Some(()),
            // stdio serial processing cannot Control a concurrently active
            // Agent Turn. Do not pretend that replayable/future Controls exist.
            ActorOperation::Control { .. } => return Err(Error::Denied(
                "external Actor Control requires an independently authenticated live-turn driver",
            )),
        };
        if target.is_none() {
            return Err(Error::Denied(
                "Actor operation targets another branch or site",
            ));
        }
        // Do not emit unbounded historical metadata on a shared transport.
        // Never silently truncate or pretend to provide exhaustive results;
        // callers can use exact request-ID queries until paged scans are
        // explicitly implemented with stable writer/audit cursors.
        const MAX_RPC_FACTS: usize = 128;
        match self.operate(op)? {
            ActorOperationResult::Controls { facts } => {
                let facts: Vec<_> = facts
                    .into_iter()
                    .filter(|f| f.admission.branch_id == self.branch)
                    .collect();
                if facts.len() > MAX_RPC_FACTS {
                    return Err(Error::Limit(
                        "Actor Control RPC list exceeds 128 entries; use exact query",
                    ));
                }
                Ok(ActorOperationResult::Controls { facts })
            }
            ActorOperationResult::Queued { .. } => {
                let facts: Vec<_> = self
                    .queue_list()?
                    .iter()
                    .filter(|f| {
                        f.request.branch_id == self.branch && f.request.recipient_site == self.site
                    })
                    .map(ActorQueueView::from)
                    .collect();
                if facts.len() > MAX_RPC_FACTS {
                    return Err(Error::Limit(
                        "Actor queue RPC list exceeds 128 entries; use exact query",
                    ));
                }
                Ok(ActorOperationResult::Queued { facts })
            }
            ActorOperationResult::Notices { .. } => {
                let facts: Vec<_> = self
                    .notices()?
                    .iter()
                    .filter(|f| {
                        f.request.branch_id == self.branch && f.request.recipient_site == self.site
                    })
                    .map(ActorNoticeView::from)
                    .collect();
                if facts.len() > MAX_RPC_FACTS {
                    return Err(Error::Limit(
                        "Actor Notice RPC list exceeds 128 entries; use exact query",
                    ));
                }
                Ok(ActorOperationResult::Notices { facts })
            }
            result => Ok(result),
        }
    }

    /// Metadata-only binding for signed-MachineLink peer guard. Remote peers
    /// never choose or mutate these values. Actual Agent authority remains
    /// the one host-created Runtime Actor and the current SQLite writer.
    pub fn verify_rpc_state(&self) -> Result<()> {
        self.store
            .actor_input_writer_fence(&self.owner, &self.session, self.epoch.get())?;
        self.controller.verify_current_binding()
    }

    /// AW-P6-9D1: an already installed trusted, READ-ONLY model management
    /// tool may inspect ONLY its exact currently journal-dispatching original
    /// Agent Turn. Source grants are checked again independently by the
    /// original ToolPolicy and AgentStepGate at advertisement / execution /
    /// post-output. This is not a prompt-originated capability grant.
    pub fn verify_model_observation(&self, ctx: &aw_agent_core::ToolContext) -> Result<()> {
        self.verify_rpc_state()?;
        if ctx.cancellation.is_cancelled()
            || ctx.turn_id == 0
            || ctx.actor != self.owner
            || ctx.identity.actor != self.owner
            || ctx.execution_site != self.site
            || ctx.scope.as_ref().is_none_or(|scope| {
                scope.session_id != self.session || scope.branch_id != self.branch
            })
        {
            return Err(Error::Denied(
                "model management source, principal, site or Session differs from bound owner",
            ));
        }
        let accepted = {
            let state = self
                .state
                .lock()
                .map_err(|_| Error::Storage("model management owner state poisoned"))?;
            if state.poisoned || state.claiming || !state.busy {
                return Err(Error::Denied(
                    "model management requires the active original Agent Turn",
                ));
            }
            state
                .input
                .as_ref()
                .filter(|input| input.turn_id == ctx.turn_id)
                .cloned()
                .ok_or(Error::Denied(
                    "model management has no matching admitted input",
                ))?
        };
        let running = self.controller.active()?.ok_or(Error::Denied(
            "model management original Runtime has no active Input",
        ))?;
        if running.id != accepted.message_id
            || running.to != self.actor
            || !matches!(running.turn, TurnTarget::Exact(t) if t==ctx.turn_id)
        {
            return Err(Error::Denied(
                "model management Runtime claim differs from admitted Agent Turn",
            ));
        }
        let fact = self
            .store
            .actor_input_fact(&self.owner, &self.session, &accepted.request_id)?;
        if fact.admission != accepted || fact.state != InputAdmissionState::Dispatching {
            return Err(Error::Denied(
                "model management Input is not durably Dispatching",
            ));
        }
        self.verify_rpc_state()
    }

    /// An opt-in model-visible read tool needs the *existing* exact Host
    /// source/epoch/name/Read Envelope grant IN ADDITION TO ToolPolicy. The
    /// grant must have been explicitly minted before Actor binding; neither
    /// a model tool name nor a project manifest can widen the Envelope.
    pub fn require_model_observer_grant(&self, name: &str) -> Result<()> {
        self.require_model_tool_grant(name, &aw_agent_core::ToolEffect::Read)
    }
    /// The read observer never implies a write permission. A model-origin
    /// effect needs its OWN exact original Host name + effect=Write grant,
    /// present in the Actor Envelope BEFORE the owning Agent was bound.
    pub fn require_model_tool_grant(
        &self,
        name: &str,
        effect: &aw_agent_core::ToolEffect,
    ) -> Result<()> {
        use aw_agent_core::control::AgentStepGate;
        self.verify_rpc_state()?;
        self.controller
            .allow_tool(name, &aw_agent_core::ToolSource::trusted_host(), effect)
    }

    /// AW-P6-9D2A: exactly one *new future* queue Input per original
    /// journaled originating Turn. `ctx` is the original Agent's private
    /// ToolContext; the external model cannot supply any authority, sender,
    /// target Actor, site, writer epoch, priority or message/source sequence.
    /// The stable owner-minted id is derived from original Session/branch/Turn
    /// and permanently collides if any second/retried tool proposal tries to
    /// enqueue again from the same already-paid Turn. This is a Pending
    /// durable encrypted *intent*, never an Agent/model execution ACK.
    /// AW-P6-9D2B1: model queue mutation is deliberately NOT the general
    /// host ActorOperation path. It only acts on a previous `mq_` item from
    /// the same original actor/site/branch, under exact Pending revision CAS.
    /// No model-origin delete/update may target an ordinary user Input,
    /// claimed/accepted paid Turn, cross-site recipient or other Actor.
    /// One stable disposition ID spans both update AND delete for this
    /// originating Session+branch+Turn, so duplicate or contradictory model
    /// tool proposals cannot disposition two different items.
    pub fn dispose_model_queue(
        &self,
        ctx: &aw_agent_core::ToolContext,
        kind: ModelQueueDispositionKind,
        target: &str,
        expected_revision: u64,
        replacement: Option<&str>,
    ) -> Result<ActorQueueView> {
        self.verify_model_observation(ctx)?;
        let tool = match kind {
            ModelQueueDispositionKind::Update => "queue_update",
            ModelQueueDispositionKind::Delete => "queue_delete",
        };
        self.require_model_tool_grant(tool, &aw_agent_core::ToolEffect::Write)?;
        if !target.starts_with("mq_")
            || target.len() != 67
            || !target[3..].bytes().all(|b| b.is_ascii_hexdigit())
            || expected_revision == 0
        {
            return Err(Error::Denied(
                "model queue disposition needs exact prior model-owned queue ID and revision",
            ));
        }
        let original_input = {
            let state = self.state.lock().map_err(|_| {
                Error::Storage("model queue disposition original owner state poisoned")
            })?;
            state
                .input
                .as_ref()
                .filter(|input| input.turn_id == ctx.turn_id)
                .ok_or(Error::Denied(
                    "model queue disposition has no running original Input",
                ))?
                .request_id
                .clone()
        };
        let intent = model_queue_disposition_id(&self.session, &self.branch, ctx.turn_id);
        let view = self.store.actor_queue_model_disposition(
            &self.owner,
            ModelQueueDispositionRequest {
                intent_id: intent,
                originating_input_request_id: original_input,
                originating_turn_id: ctx.turn_id,
                session_id: self.session.clone(),
                branch_id: self.branch.clone(),
                recipient_site: self.site.clone(),
                sender: label(self.actor),
                writer_epoch: self.epoch.get(),
                target_request_id: target.into(),
                expected_revision,
                kind,
            },
            replacement,
        )?;
        self.verify_model_observation(ctx)?;
        Ok(view)
    }

    pub fn enqueue_model_future_text(
        &self,
        ctx: &aw_agent_core::ToolContext,
        text: &str,
    ) -> Result<ActorQueueView> {
        self.verify_model_observation(ctx)?;
        self.require_model_tool_grant("queue_enqueue", &aw_agent_core::ToolEffect::Write)?;
        if text.is_empty()
            || text.len() > 4096
            || text.contains('\0')
            || text
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(Error::Denied(
                "model queued Input must be bounded 1..4096 safe text",
            ));
        }
        let request_id = model_queue_request_id(&self.session, &self.branch, ctx.turn_id);
        let fact = self.store.actor_queue_enqueue_minted(
            &self.owner,
            ActorQueueRequest {
                request_id,
                session_id: self.session.clone(),
                branch_id: self.branch.clone(),
                recipient_site: self.site.clone(),
                sender: label(self.actor),
                message_id: 0,
                writer_epoch: self.epoch.get(),
                target: QueueTarget::Auto,
                priority: 0,
            },
            text,
        )?;
        // A revoked/retired owner immediately after irreversible SQLite
        // acceptance is UNCERTAIN for this Tool call: original Agent journal
        // must preserve hazard; NEVER automatically enqueue another job.
        self.verify_model_observation(ctx)?;
        Ok((&fact).into())
    }

    /// AW-P6-9D2B2A: this prepares an immutable owner-only Process I/O
    /// intent for the *same* current original journaled Agent Tool Turn.
    /// Actual Process custody is independently checked by the trusted host
    /// against its explicitly registered original Process handle. Neither
    /// model JSON nor an external Process ID can select a different process.
    pub fn prepare_model_process_close(
        &self,
        ctx: &aw_agent_core::ToolContext,
        handle: &str,
    ) -> Result<ModelProcessFact> {
        self.prepare_model_process_effect(ctx, handle, ModelProcessAction::CloseStdin, "")
    }
    pub fn prepare_model_process_terminate(
        &self,
        ctx: &aw_agent_core::ToolContext,
        handle: &str,
        binding: &str,
    ) -> Result<ModelProcessFact> {
        self.prepare_model_process_effect(ctx, handle, ModelProcessAction::Terminate, binding)
    }
    fn prepare_model_process_effect(
        &self,
        ctx: &aw_agent_core::ToolContext,
        handle: &str,
        action: ModelProcessAction,
        process_binding: &str,
    ) -> Result<ModelProcessFact> {
        self.verify_model_observation(ctx)?;
        let tool = match action {
            ModelProcessAction::CloseStdin => "process_close_stdin",
            ModelProcessAction::Terminate => "process_terminate",
        };
        self.require_model_tool_grant(tool, &aw_agent_core::ToolEffect::Write)?;
        if handle.is_empty()
            || handle.len() > 64
            || !handle
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        {
            return Err(Error::Denied(
                "model Process requires host-registered opaque handle",
            ));
        }
        let input = {
            let state = self
                .state
                .lock()
                .map_err(|_| Error::Storage("model Process owner state poisoned"))?;
            state
                .input
                .as_ref()
                .filter(|i| i.turn_id == ctx.turn_id)
                .ok_or(Error::Denied("model Process has no original active Input"))?
                .request_id
                .clone()
        };
        let fact = self.store.model_process_prepare(
            &self.owner,
            ModelProcessRequest {
                intent_id: model_process_intent_id(&self.session, &self.branch, ctx.turn_id),
                input_request_id: input,
                session_id: self.session.clone(),
                branch_id: self.branch.clone(),
                recipient_site: self.site.clone(),
                sender: label(self.actor),
                writer_epoch: self.epoch.get(),
                turn_id: ctx.turn_id,
                handle: handle.to_owned(),
                action,
                process_binding: process_binding.to_owned(),
            },
        )?;
        // If the host lost authority *after* the immutable prepare, leave
        // Prepared: require inspection; NEVER erase/replay intent ID.
        self.verify_model_observation(ctx)?;
        Ok(fact)
    }
    /// A same-incarnation synchronous terminal fact: Applied means ONLY the
    /// existing Process stdin ownership was dropped, NOT child exit, payment
    /// or delivery of bytes to a process. The Tool may not claim completion
    /// if the original writer/Runtime has become stale in the meantime.
    pub fn finish_model_process_close(
        &self,
        ctx: &aw_agent_core::ToolContext,
        expected: &ModelProcessFact,
        state: ModelProcessState,
    ) -> Result<ModelProcessFact> {
        self.finish_model_process_effect(ctx, expected, ModelProcessAction::CloseStdin, state)
    }
    pub fn finish_model_process_terminate(
        &self,
        ctx: &aw_agent_core::ToolContext,
        expected: &ModelProcessFact,
        state: ModelProcessState,
    ) -> Result<ModelProcessFact> {
        self.finish_model_process_effect(ctx, expected, ModelProcessAction::Terminate, state)
    }
    fn finish_model_process_effect(
        &self,
        ctx: &aw_agent_core::ToolContext,
        expected: &ModelProcessFact,
        action: ModelProcessAction,
        state: ModelProcessState,
    ) -> Result<ModelProcessFact> {
        if expected.request.action != action
            || expected.request.session_id != self.session
            || expected.request.branch_id != self.branch
            || expected.request.recipient_site != self.site
            || expected.request.sender != label(self.actor)
            || expected.request.turn_id != ctx.turn_id
            || expected.request.writer_epoch != self.epoch.get()
        {
            return Err(Error::Denied(
                "original model Process receipt changed owner scope",
            ));
        }
        self.verify_model_observation(ctx)?;
        let tool = match action {
            ModelProcessAction::CloseStdin => "process_close_stdin",
            ModelProcessAction::Terminate => "process_terminate",
        };
        self.require_model_tool_grant(tool, &aw_agent_core::ToolEffect::Write)?;
        self.store
            .model_process_transition(&self.owner, expected, state)
    }
    /// Inspect only: cannot resend Process I/O or fabricate Applied. Original
    /// paid Tool/Turn hazard remains a separate human review obligation.
    pub fn inspect_model_process(&self, id: &str, note: &str) -> Result<ModelProcessFact> {
        self.verify_rpc_state()?;
        self.store
            .model_process_inspect(&self.owner, &self.session, id, self.epoch.get(), note)
    }
    pub fn model_process_fact(&self, id: &str) -> Result<ModelProcessFact> {
        self.store
            .model_process_fact(&self.owner, &self.session, id)
    }

    /// AW-P6-9D2B2B2: unlike in-flight Cancel/Freeze Control, this model
    /// Tool can only request a durable pause of FUTURE original Agent Turn
    /// admissions. The presently journaled Tool/Turn is left uninterrupted
    /// so it can produce factual Tool and paid model receipts. No second
    /// model runner, Hook, Actor Control signal or new durable store.
    pub fn request_model_pause_after_turn(
        &self,
        ctx: &aw_agent_core::ToolContext,
    ) -> Result<ModelAgentPauseFact> {
        self.verify_model_observation(ctx)?;
        self.require_model_tool_grant("agent_pause_after_turn", &aw_agent_core::ToolEffect::Write)?;
        let original = {
            let state = self
                .state
                .lock()
                .map_err(|_| Error::Storage("model Agent pause owner state poisoned"))?;
            state
                .input
                .as_ref()
                .filter(|i| i.turn_id == ctx.turn_id)
                .ok_or(Error::Denied(
                    "Agent future pause has no current original Input",
                ))?
                .request_id
                .clone()
        };
        let stored = self.store.model_agent_pause_after_turn(
            &self.owner,
            ModelAgentPauseRequest {
                intent_id: model_agent_pause_id(&self.session, &self.branch, ctx.turn_id),
                input_request_id: original,
                session_id: self.session.clone(),
                branch_id: self.branch.clone(),
                sender: label(self.actor),
                writer_epoch: self.epoch.get(),
                turn_id: ctx.turn_id,
            },
        )?;
        // If original actor/source disappears post-commit, the hold remains
        // durable and the paid Tool result is uncertain, NEVER auto-retried.
        self.verify_model_observation(ctx)?;
        Ok(stored)
    }
    /// Redacted factual owner-only view; a model never receives permission
    /// to release this guard or inspect private paid/model continuation.
    pub fn current_future_pause(&self) -> Result<Option<ModelAgentPauseFact>> {
        self.verify_rpc_state()?;
        self.store.model_agent_pause_current(
            &self.owner,
            &self.session,
            &self.branch,
            self.epoch.get(),
        )
    }
    /// An explicitly authenticated original owning HOST, NEVER a model
    /// Tool, may release an Armed future-admission pause only after the
    /// original source Input FACTUALLY Settled and there is no outstanding
    /// Actor Input hazard. A resumed next Turn still obeys all original
    /// queue, ToolPolicy, WriterEpoch and paid receipt gates.
    pub fn release_future_pause(
        &self,
        agent: &Agent,
        intent_id: &str,
        note: &str,
    ) -> Result<ModelAgentPauseFact> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::Storage("Agent future pause state poisoned"))?;
        if state.poisoned || state.claiming || state.busy || state.input.is_some() {
            return Err(Error::Conflict(
                "Agent future pause release requires original owner to be completely idle and reconciled"));
        }
        self.verify_rpc_state()?;
        self.verify_rpc_agent(agent)?;
        let fact = self.store.model_agent_pause_release(
            &self.owner,
            &self.session,
            &self.branch,
            self.epoch.get(),
            intent_id,
            note,
        )?;
        self.verify_rpc_state()?;
        Ok(fact)
    }

    /// Read-only bound identity for derived first-party host services. The
    /// raw key is NEVER authority without current owner/Envelope/writer checks.
    pub(crate) fn bound_actor_key(&self) -> ActorKey {
        self.actor
    }

    pub fn bound_peer_scope(&self) -> (&PrincipalId, &str, &str, &str, u64) {
        (
            &self.owner,
            &self.session,
            &self.branch,
            &self.site,
            self.epoch.get(),
        )
    }

    /// Select the same already-journaled Agent and narrowed Envelope at the
    /// external transport entry; cannot create or select another provider.
    pub fn verify_rpc_agent(&self, agent: &Agent) -> Result<()> {
        self.controller.validate_agent_binding(agent)
    }

    /// Mint exactly one current owned Input from an RPC-supplied bounded
    /// request correlation and text. No caller may supply Runtime/Actor key,
    /// sender/site, Envelope, process incarnation or writer epoch.
    pub fn enqueue_bound_text(
        &self,
        agent: &Agent,
        request_id: &str,
        source_sequence: u64,
        target: QueueTarget,
        priority: i8,
        text: &str,
    ) -> Result<ActorQueueView> {
        self.store
            .actor_input_writer_fence(&self.owner, &self.session, self.epoch.get())?;
        self.controller.verify_current_binding()?;
        let message = ActorMessage {
            id: source_sequence,
            source_sequence,
            from: self.actor,
            to: self.actor,
            kind: Kind::Input,
            turn: match target {
                QueueTarget::Auto => TurnTarget::Auto,
                QueueTarget::Exact(t) => TurnTarget::Exact(t),
            },
            priority,
            payload: text.to_owned(),
            content_type: aw_agent_kernel::message::ContentType::Text,
        };
        Ok((&self.enqueue(agent, request_id, message)?).into())
    }

    /// Queue *more than one* bounded private Input without forwarding a
    /// provider request or consuming Runtime inbox capacity. A queue item is
    /// journaled via immutable SQLite metadata; its body is encrypted only
    /// in the owner's private SecretStore. Auto may rebind to the same
    /// logical Session+site after a safe restart; Exact never silently moves.
    pub fn enqueue(
        &self,
        agent: &Agent,
        request_id: impl Into<String>,
        message: ActorMessage,
    ) -> Result<ActorQueueFact> {
        {
            let state = self
                .state
                .lock()
                .map_err(|_| Error::Storage("Actor owner lock poisoned"))?;
            if state.poisoned || state.claiming {
                return Err(Error::Conflict(
                    "Actor owner cannot enqueue while uncertain",
                ));
            }
        }
        self.controller.validate_agent_binding(agent)?;
        let scope = agent
            .journal_scope()?
            .ok_or(Error::Denied("queue requires real Agent journal"))?;
        if scope.session_id != self.session
            || scope.branch_id != self.branch
            || agent.default_identity()?.actor != self.owner
            || !agent.is_journaled()
            || message.kind != Kind::Input
            || message.from != self.actor
            || message.to != self.actor
            || message.id != message.source_sequence
        {
            return Err(Error::Denied(
                "queue Input requires exact owned Actor/Agent/Session",
            ));
        }
        self.store
            .actor_input_writer_fence(&self.owner, &self.session, self.epoch.get())?;
        let current = self.store.session(&self.owner, &self.session)?;
        let target = match message.turn {
            TurnTarget::Auto => QueueTarget::Auto,
            TurnTarget::Exact(t)
                if t >= current.next_turn && t >= agent.history_state().next_turn_id =>
            {
                QueueTarget::Exact(t)
            }
            _ => return Err(Error::Denied("queued exact target Turn is already settled")),
        };
        self.store.actor_queue_enqueue(
            &self.owner,
            ActorQueueRequest {
                request_id: request_id.into(),
                session_id: self.session.clone(),
                branch_id: self.branch.clone(),
                recipient_site: self.site.clone(),
                sender: label(message.from),
                message_id: message.id,
                writer_epoch: self.epoch.get(),
                target,
                priority: message.priority,
            },
            &message.payload,
        )
    }

    pub fn queue_list(&self) -> Result<Vec<ActorQueueFact>> {
        self.store.actor_queue_list(&self.owner, &self.session)
    }
    pub fn queue_fact(&self, id: &str) -> Result<ActorQueueFact> {
        self.store.actor_queue_fact(&self.owner, &self.session, id)
    }

    /// Pending-only immutable revision edit; it never mutates the Runtime
    /// mailbox or an already claimed provider/Agent Turn.
    pub fn update_queued(
        &self,
        id: &str,
        revision: u64,
        payload: &str,
        priority: i8,
    ) -> Result<ActorQueueFact> {
        {
            let state = self
                .state
                .lock()
                .map_err(|_| Error::Storage("Actor owner lock poisoned"))?;
            if state.poisoned || state.claiming {
                return Err(Error::Conflict("Actor queue owner is uncertain"));
            }
        }
        let current = self.queue_fact(id)?;
        if current.request.branch_id != self.branch || current.request.recipient_site != self.site {
            return Err(Error::Denied(
                "queued message is bound to a different recipient",
            ));
        }
        self.store.actor_queue_update(
            &self.owner,
            &self.session,
            id,
            self.epoch.get(),
            revision,
            payload,
            priority,
        )
    }

    /// One current writer claims and decrypts the next safely pending item,
    /// then uses precisely the existing 9A Input admission + original Runtime
    /// mailbox. SQLite ACK is written only after actual Runtime Queued. A
    /// crash anywhere after the claim leaves it non-replayable, even if the
    /// original Agent's paid attempt has not yet written a completion receipt.
    pub fn admit_next_queued(&self, agent: &Agent) -> Result<Option<ActorQueueFact>> {
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| Error::Storage("Actor owner lock poisoned"))?;
            if state.poisoned || state.busy || state.input.is_some() || state.claiming {
                return Err(Error::Conflict(
                    "another accepted Input or uncertain queue claim exists",
                ));
            }
            state.claiming = true;
        }
        let result = (|| {
            self.controller.validate_agent_binding(agent)?;
            let scope = agent
                .journal_scope()?
                .ok_or(Error::Denied("queue requires journal"))?;
            if scope.session_id != self.session
                || scope.branch_id != self.branch
                || agent.history_state().next_turn_id
                    != self.store.session(&self.owner, &self.session)?.next_turn
                || self.controller.pending()? != 0
            {
                return Err(Error::Conflict(
                    "queue receiver branch/Turn/mailbox is not ready",
                ));
            }
            self.store.actor_queue_claim_next(
                &self.owner,
                &self.session,
                &self.branch,
                &self.site,
                &label(self.actor),
                self.epoch.get(),
            )
        })();
        let claimed = match result {
            Ok(Some(claim)) => claim,
            Ok(None) => {
                self.state
                    .lock()
                    .map_err(|_| Error::Storage("Actor owner lock poisoned"))?
                    .claiming = false;
                return Ok(None);
            }
            Err(error) => {
                self.state
                    .lock()
                    .map_err(|_| Error::Storage("Actor owner lock poisoned"))?
                    .claiming = false;
                return Err(error);
            }
        };
        let fact = &claimed.fact;
        let receipt = fact
            .claim
            .as_ref()
            .ok_or(Error::Storage("queue lost claim identity"));
        let accepted = receipt.and_then(|receipt| {
            self.submit_inner(
                agent,
                queued_input_request_id(&self.session, &fact.request.request_id),
                ActorMessage {
                    id: receipt.message_id,
                    source_sequence: receipt.message_id,
                    from: self.actor,
                    to: self.actor,
                    kind: Kind::Input,
                    turn: TurnTarget::Exact(receipt.turn_id),
                    priority: fact.request.priority,
                    payload: claimed.payload.to_string(),
                    content_type: aw_agent_kernel::message::ContentType::Text,
                },
                true,
            )
        });
        let result = accepted.and_then(|_| self.store.actor_queue_ack(&self.owner, fact));
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Storage("Actor owner lock poisoned"))?;
        state.claiming = false;
        if result.is_err() {
            state.poisoned = true;
        }
        result.map(Some)
    }

    /// Typed, strictly owner-originated synchronous Control on an already
    /// active exact Turn. Control facts are immutable audit evidence only.
    /// Prepared is durable BEFORE the Runtime mutation, Applied only after
    /// the actual bounded mailbox/Control receipt is returned. No queued
    /// payload, replay token, OS signal or background Control worker exists.
    pub fn control(
        &self,
        request_id: impl Into<String>,
        action: ActorControlAction,
        message_id: u64,
    ) -> Result<ActorControlFact> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Storage("Actor owner state lock poisoned"))?;
        if state.poisoned || !state.busy {
            return Err(Error::Conflict("Control requires this owner's active Turn"));
        }
        let input = state
            .input
            .as_ref()
            .ok_or(Error::Conflict("Control has no current admitted Input"))?
            .clone();
        let active = self
            .controller
            .active()?
            .ok_or(Error::Conflict("Control has no actual active Actor Turn"))?;
        if active.id != input.message_id
            || active.to != self.actor
            || !matches!(active.turn,TurnTarget::Exact(t) if t==input.turn_id)
            || message_id <= input.message_id
        {
            return Err(Error::Denied(
                "Control could not prove current exact Actor claim",
            ));
        }
        let admission = ActorControlAdmission {
            request_id: request_id.into(),
            input_request_id: input.request_id,
            session_id: self.session.clone(),
            branch_id: self.branch.clone(),
            writer_epoch: self.epoch.get(),
            turn_id: input.turn_id,
            sender: label(self.actor),
            target: label(self.actor),
            message_id,
            action,
        };
        self.store
            .actor_control_prepare(&self.owner, admission.clone())?;
        let kind = match action {
            ActorControlAction::CancelTurn => ControlKind::Cancel(Scope::Turn),
            ActorControlAction::FreezeTurn => ControlKind::Freeze(Scope::Turn),
            ActorControlAction::ResumeTurn => ControlKind::Resume(Scope::Turn),
            ActorControlAction::FreezeActor => ControlKind::Freeze(Scope::Actor),
            ActorControlAction::ResumeActor => ControlKind::Resume(Scope::Actor),
        };
        let delivered = self.controller.send(
            ActorMessage {
                from: self.actor,
                to: self.actor,
                id: message_id,
                source_sequence: message_id,
                kind: Kind::Control(kind),
                turn: TurnTarget::Exact(input.turn_id),
                priority: 0,
                payload: String::new(),
                content_type: aw_agent_kernel::message::ContentType::Text,
            },
            input.turn_id,
        );
        let transition = match delivered {
            Ok(Delivery::Applied(_)) => ActorControlState::Applied,
            Ok(Delivery::Consumed) => ActorControlState::Rejected,
            // A deferred/failed send is potentially ambiguous: never
            // invent an Applied acknowledgement or retry a Control ID.
            Ok(Delivery::Deferred(_) | Delivery::Queued(_)) | Err(_) => ActorControlState::Unknown,
        };
        let outcome = self.store.actor_control_transition(
            &self.owner,
            &admission,
            ActorControlState::Prepared,
            transition,
        );
        if outcome.is_err() || transition == ActorControlState::Unknown {
            state.poisoned = true;
        }
        let fact = outcome?;
        if transition == ActorControlState::Unknown {
            return Err(Error::Conflict(
                "Actor Control outcome uncertain; inspect, do not replay",
            ));
        }
        Ok(fact)
    }
    pub fn control_fact(&self, request_id: &str) -> Result<ActorControlFact> {
        self.store
            .actor_control_fact(&self.owner, &self.session, request_id)
    }
    pub fn controls(&self) -> Result<Vec<ActorControlFact>> {
        self.store.actor_control_list(&self.owner, &self.session)
    }
    /// Inspect after crash/restart; never replays a control or changes a
    /// separate paid/tool hazard. Only the current SQLite writer can inspect.
    pub fn inspect_control(&self, request_id: &str, note: &str) -> Result<ActorControlFact> {
        self.store.actor_control_inspect(
            &self.owner,
            &self.session,
            request_id,
            self.epoch.get(),
            note,
        )
    }
    pub fn fact(&self, request_id: &str) -> Result<ActorInputFact> {
        self.store
            .actor_input_fact(&self.owner, &self.session, request_id)
    }
    /// The original Agent journal is still the sole factual provider/effect
    /// source. This uses the exact claimed Input as a second proof; a different
    /// same-Actor message or stale branch cannot be laundered into dispatch.
    pub async fn run_once<F>(&self, agent: &mut Agent, on_event: F) -> Result<Option<Turn>>
    where
        F: FnMut(&AgentEvent) -> Result<()> + Send,
    {
        let admission = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| Error::Storage("Actor owner state lock poisoned"))?;
            if state.poisoned || state.busy || state.claiming {
                return Err(Error::Conflict(
                    "Actor owner needs inspection or is already running",
                ));
            }
            let Some(admission) = state.input.clone() else {
                return Ok(None);
            };
            state.busy = true;
            admission
        };
        let mut guard = RunGuard {
            state: self.state.clone(),
            finished: false,
        };
        let proof = |message: &ActorMessage, turn: u64| -> Result<()> {
            if message.kind != Kind::Input
                || message.to != self.actor
                || message.id != admission.message_id
                || label(message.to) != admission.target
                || label(message.from) != admission.sender
                || message.priority != admission.priority
                || !matches!(message.turn,TurnTarget::Exact(id) if id==admission.turn_id)
                || turn != admission.turn_id
                || format!("{:x}", Sha256::digest(message.payload.as_bytes()))
                    != admission.payload_sha256
            {
                return Err(Error::Denied(
                    "claimed Actor Input differs from durable acceptance",
                ));
            }
            let current = self.store.actor_input_fact(
                &self.owner,
                &admission.session_id,
                &admission.request_id,
            )?;
            if current.admission != admission || current.state != InputAdmissionState::Accepted {
                return Err(Error::Denied(
                    "Actor Input has no exact SQLite acceptance receipt",
                ));
            }
            self.store.actor_input_transition(
                &self.owner,
                &admission,
                InputAdmissionState::Accepted,
                InputAdmissionState::Dispatching,
            )?;
            Ok(())
        };
        let result = self
            .controller
            .run_once_verified(agent, &proof, on_event)
            .await;
        let turn = match result {
            Ok(Some(turn)) if turn.id == admission.turn_id => turn,
            Ok(_) => {
                return Err(Error::Conflict(
                    "exact Actor claim never produced one settled Turn",
                ))
            }
            Err(error) => {
                let _ = self.store.actor_input_transition(
                    &self.owner,
                    &admission,
                    InputAdmissionState::Dispatching,
                    InputAdmissionState::Unknown,
                );
                return Err(error);
            }
        };
        self.store.actor_input_transition(
            &self.owner,
            &admission,
            InputAdmissionState::Dispatching,
            InputAdmissionState::Settled,
        )?;
        // Close the custody receipt only after the SAME journaled Turn
        // has factually settled; never synthesize an external effect ACK.
        self.store.actor_queue_finalize(&self.owner, &admission)?;
        guard.done()?;
        Ok(Some(turn))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::{
        AgentOptions, CompletionFuture, CompletionMeta, DeltaSink, Provider, Role, Usage,
    };
    use aw_agent_kernel::{
        actor::{ActorState, RuntimeKey},
        envelope::{Access, Budget, EnvelopeSpec, Network},
    };
    use aw_store::{SecretStore, SessionBinding as StoredSessionBinding};
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };
    struct EchoProvider {
        calls: Arc<AtomicUsize>,
        wait: Option<Arc<tokio::sync::Notify>>,
    }
    impl Provider for EchoProvider {
        fn stream<'a>(
            &'a self,
            messages: &'a [aw_agent_core::Message],
            _: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                assert!(messages.iter().any(|m| m.role == Role::User));
                self.calls.fetch_add(1, Ordering::SeqCst);
                if let Some(notify) = &self.wait {
                    notify.notify_one();
                    std::future::pending::<()>().await;
                }
                sink("exact Agent reply")?;
                Ok(CompletionMeta {
                    finish_reason: "stop".into(),
                    usage: Usage {
                        input_tokens: Some(3),
                        output_tokens: Some(2),
                    },
                })
            })
        }
    }
    struct Fixture {
        store: SessionStore,
        owner: Service,
        agent: Agent,
        calls: Arc<AtomicUsize>,
        _runtime: Arc<Mutex<HostRuntime>>,
    }
    fn setup(wait: Option<Arc<tokio::sync::Notify>>) -> Fixture {
        setup_with_store(wait, SessionStore::in_memory().unwrap())
    }
    fn setup_with_store(wait: Option<Arc<tokio::sync::Notify>>, store: SessionStore) -> Fixture {
        let principal = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(
                &principal,
                "Durable Actor",
                StoredSessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        setup_bound(wait, store, session, branch, 2)
    }
    fn setup_bound(
        wait: Option<Arc<tokio::sync::Notify>>,
        store: SessionStore,
        session: aw_store::SessionInfo,
        branch: aw_store::BranchInfo,
        runtime_generation: u64,
    ) -> Fixture {
        setup_bound_with_tools(
            wait,
            store,
            session,
            branch,
            runtime_generation,
            Vec::new(),
            None,
        )
    }
    /// Same original Session/Agent, optionally seeded with exactly reviewed
    /// P6-9D1 Host-Read Envelope grants before the current Actor binds. It
    /// does NOT mint grants in ToolPolicy or register any tools implicitly.
    fn setup_bound_with_tools(
        wait: Option<Arc<tokio::sync::Notify>>,
        store: SessionStore,
        session: aw_store::SessionInfo,
        branch: aw_store::BranchInfo,
        runtime_generation: u64,
        grants: Vec<aw_agent_kernel::envelope::ExactGrant>,
        selected_provider: Option<Arc<dyn Provider>>,
    ) -> Fixture {
        let principal = PrincipalId::new("local").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = selected_provider.unwrap_or_else(|| {
            Arc::new(EchoProvider {
                calls: calls.clone(),
                wait,
            }) as Arc<dyn Provider>
        });
        let mut agent = Agent::builder(provider, AgentOptions::new("fixture", "model"))
            .build()
            .unwrap();
        assert_eq!(agent.default_identity().unwrap().actor, principal);
        let (view, journal) = store.activate(&principal, &session.id, &branch.id).unwrap();
        agent
            .load_history(view.history, Some(Arc::new(journal)))
            .unwrap();
        let epoch = NonZeroU64::new(
            store
                .workspace_writer(&principal, None, &session.workspace_id)
                .unwrap()
                .writer_epoch,
        )
        .unwrap();
        let mut runtime = HostRuntime::new(
            principal.clone(),
            "local",
            RuntimeKey {
                id: NonZeroU64::new(9).unwrap(),
                generation: NonZeroU64::new(runtime_generation).unwrap(),
            },
            8,
            4,
        )
        .unwrap();
        let actor = runtime
            .bind_agent(
                &principal,
                aw_agent_core::runtime::SessionBinding::from_trusted_host(
                    session.id.clone(),
                    Some(epoch),
                )
                .unwrap(),
                None,
                EnvelopeSpec {
                    actor: principal.as_str().into(),
                    site: "local".into(),
                    revision: 1,
                    access: Access::ReadWrite,
                    network: Network::Deny,
                    process: false,
                    tty: false,
                    microphone: false,
                    speaker: false,
                    budget: Budget {
                        max_model_rounds: 8,
                        max_tool_calls: 128,
                        max_children: 2,
                        max_depth: 2,
                        max_output_bytes: 1024 * 1024,
                    },
                    grants,
                },
                "root",
            )
            .unwrap();
        runtime
            .record_state(&principal, actor, Some(epoch), ActorState::Running)
            .unwrap();
        let runtime = Arc::new(Mutex::new(runtime));
        let owner = Service::new(
            store.clone(),
            runtime.clone(),
            principal,
            actor,
            epoch,
            session.id,
            branch.id,
        )
        .unwrap();
        Fixture {
            store,
            owner,
            agent,
            calls,
            _runtime: runtime,
        }
    }
    fn input(owner: &Service, id: u64, turn: u64, payload: &str) -> ActorMessage {
        ActorMessage {
            id,
            source_sequence: id,
            from: owner.actor,
            to: owner.actor,
            kind: Kind::Input,
            turn: TurnTarget::Exact(turn),
            priority: 0,
            payload: payload.into(),
            content_type: aw_agent_kernel::message::ContentType::Text,
        }
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn aw_p6_9b2b_child_notify_uses_original_runtime_and_persistent_private_receipts() {
        let temp = PrivateTemp::new();
        let f = setup_with_store(None, SessionStore::open(&temp.0).unwrap());
        let (child_session, _) = f
            .store
            .create(
                &f.owner.owner,
                "real child Session",
                StoredSessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let child = {
            let mut rt = f._runtime.lock().unwrap();
            let key = rt
                .bind_child_agent(
                    &f.owner.owner,
                    f.owner.actor,
                    Some(f.owner.epoch),
                    aw_agent_core::runtime::SessionBinding::from_trusted_host(
                        child_session.id,
                        Some(f.owner.epoch),
                    )
                    .unwrap(),
                    EnvelopeSpec {
                        actor: "local".into(),
                        site: "local".into(),
                        revision: 2,
                        access: Access::ReadOnly,
                        network: Network::Deny,
                        process: false,
                        tty: false,
                        microphone: false,
                        speaker: false,
                        budget: Budget {
                            max_model_rounds: 4,
                            max_tool_calls: 8,
                            max_children: 0,
                            max_depth: 1,
                            max_output_bytes: 32768,
                        },
                        grants: vec![],
                    },
                    "notice-child",
                )
                .unwrap();
            rt.record_state(
                &f.owner.owner,
                key,
                Some(f.owner.epoch),
                ActorState::Running,
            )
            .unwrap();
            key
        };
        // A terminal child can still emit exactly its data-only final notice.
        f._runtime
            .lock()
            .unwrap()
            .record_state(
                &f.owner.owner,
                child,
                Some(f.owner.epoch),
                ActorState::Settled,
            )
            .unwrap();
        let msg = ActorMessage {
            from: child,
            to: f.owner.actor,
            id: 1,
            source_sequence: 1,
            kind: Kind::Notify,
            turn: TurnTarget::Auto,
            priority: 0,
            payload: "terminal child data-only secret".into(),
            content_type: aw_agent_kernel::message::ContentType::Text,
        };
        let notice = f
            .owner
            .record_notice("child_notice_1", msg.clone())
            .unwrap();
        assert_eq!(notice.state, ActorNoticeState::Accepted);
        assert_eq!(
            f.calls.load(Ordering::SeqCst),
            0,
            "Notify does not execute Agent/model"
        );
        assert!(
            matches!(f.owner.operate(ActorOperation::ListNotices).unwrap(),
            ActorOperationResult::Notices { facts } if facts.len()==1 && facts[0].state==ActorNoticeState::Accepted)
        );
        let public = serde_json::to_string(
            &f.owner
                .operate(ActorOperation::QueryNotice {
                    request_id: "child_notice_1".into(),
                })
                .unwrap(),
        )
        .unwrap();
        for secret in [
            "terminal child",
            "sec_",
            "payload_sha256",
            "payload_ref",
            "process_id",
            "store_generation",
        ] {
            assert!(!public.contains(secret), "Notify view leaked {secret}");
        }
        assert_eq!(
            f.owner.poll_notice_inbox().unwrap().unwrap().request_id,
            "child_notice_1"
        );
        assert!(f.owner.poll_notice_inbox().unwrap().is_none());
        assert_eq!(
            f.owner.notice_private("child_notice_1").unwrap().as_str(),
            msg.payload
        );
        assert!(f
            .owner
            .record_notice("child_notice_1", msg.clone())
            .is_err());
        assert!(f
            .owner
            .record_notice(
                "self_notice",
                ActorMessage {
                    from: f.owner.actor,
                    ..msg.clone()
                }
            )
            .is_err());
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
        let ack = f
            .owner
            .operate(ActorOperation::AckNotice {
                request_id: "child_notice_1".into(),
                revision: notice.revision,
            })
            .unwrap();
        assert!(
            matches!(ack, ActorOperationResult::Notice { fact } if fact.state==ActorNoticeState::Read)
        );
        assert!(f.owner.notice_private("child_notice_1").is_err());
        assert_eq!(f.owner.reclaim_retired_custody(16).unwrap().unlinked, 0);
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn aw_p6_9b2b_wrong_sender_cannot_forge_accepted_notify_and_requires_inspection() {
        let temp = PrivateTemp::new();
        let f = setup_with_store(None, SessionStore::open(&temp.0).unwrap());
        let forged = ActorMessage {
            id: 1,
            source_sequence: 1,
            from: ActorKey {
                runtime: f.owner.actor.runtime,
                id: NonZeroU64::new(99).unwrap(),
                generation: NonZeroU64::new(1).unwrap(),
            },
            to: f.owner.actor,
            kind: Kind::Notify,
            turn: TurnTarget::Auto,
            priority: 0,
            payload: "forged final child event".into(),
            content_type: aw_agent_kernel::message::ContentType::Text,
        };
        assert!(f.owner.record_notice("forged_notice", forged).is_err());
        let stored = f.owner.notice_fact("forged_notice").unwrap();
        assert_eq!(stored.state, ActorNoticeState::Unknown);
        assert!(f.owner.notice_private("forged_notice").is_err());
        assert!(f.owner.poll_notice_inbox().is_err());
        assert!(f
            .owner
            .record_notice(
                "later_notice",
                ActorMessage {
                    id: 2,
                    source_sequence: 2,
                    from: f.owner.actor,
                    to: f.owner.actor,
                    kind: Kind::Notify,
                    turn: TurnTarget::Auto,
                    priority: 0,
                    payload: "not child".into(),
                    content_type: aw_agent_kernel::message::ContentType::Text,
                }
            )
            .is_err());
        assert!(matches!(f.owner.operate(ActorOperation::InspectNotice {
            request_id: "forged_notice".into(), note: "stale nonexistent Actor refused by Runtime".into(),
        }).unwrap(), ActorOperationResult::Notice { fact } if fact.state==ActorNoticeState::Inspected));
        assert!(f.owner.notice_private("forged_notice").is_err());
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn exact_sqlite_accepted_input_drives_one_original_agent_turn_and_receipt() {
        let mut f = setup(None);
        let accepted = f
            .owner
            .submit(&f.agent, "req_1", input(&f.owner, 1, 1, "hello"))
            .unwrap();
        assert_eq!(accepted.state, InputAdmissionState::Accepted);
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            f.owner.fact("req_1").unwrap().state,
            InputAdmissionState::Accepted
        );
        assert!(f
            .owner
            .submit(&f.agent, "req_1", input(&f.owner, 1, 1, "hello"))
            .is_err());
        let turn = f
            .owner
            .run_once(&mut f.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(turn.answer, "exact Agent reply");
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            f.owner.fact("req_1").unwrap().state,
            InputAdmissionState::Settled
        );
        assert!(!f.owner.fact("req_1").unwrap().state.needs_inspection());
        assert!(f
            .owner
            .submit(&f.agent, "req_1", input(&f.owner, 2, 2, "second"))
            .is_err());
        let accepted = f
            .owner
            .submit(&f.agent, "req_2", input(&f.owner, 2, 2, "second"))
            .unwrap();
        assert_eq!(accepted.state, InputAdmissionState::Accepted);
        let second = f
            .owner
            .run_once(&mut f.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(second.id, 2);
        assert_eq!(f.calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            f.owner.fact("req_2").unwrap().state,
            InputAdmissionState::Settled
        );
    }
    #[tokio::test]
    async fn cancelled_owner_future_retains_dispatching_fact_and_never_replays() {
        let notify = Arc::new(tokio::sync::Notify::new());
        let mut f = setup(Some(notify.clone()));
        f.owner
            .submit(&f.agent, "paid_1", input(&f.owner, 1, 1, "billable"))
            .unwrap();
        let owner = &f.owner;
        let running = owner.run_once(&mut f.agent, |_| Ok(()));
        let result = tokio::time::timeout(Duration::from_millis(70), async {
            tokio::pin!(running);
            tokio::select! {
                r=&mut running=>Some(r),
                _=notify.notified()=>{
                    tokio::time::sleep(Duration::from_millis(15)).await;
                    None
                },
            }
        })
        .await
        .unwrap();
        assert!(result.is_none());
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            f.owner.fact("paid_1").unwrap().state,
            InputAdmissionState::Dispatching
        );
        assert!(f
            .owner
            .submit(&f.agent, "paid_2", input(&f.owner, 2, 1, "retry"))
            .is_err());
        assert!(f.owner.run_once(&mut f.agent, |_| Ok(())).await.is_err());
        // Inspection is an explicit durable *disposition*, not a replayed Turn,
        // and does not erase independent incomplete provider/OS hazard.
        let fact = f
            .store
            .actor_input_inspect(
                &f.owner.owner,
                &f.owner.session,
                "paid_1",
                f.owner.epoch.get(),
                "manual review: do not replay",
            )
            .unwrap();
        assert_eq!(fact.state, InputAdmissionState::Inspected);
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
        assert!(f.owner.run_once(&mut f.agent, |_| Ok(())).await.is_err());
    }
    #[tokio::test]
    async fn pending_actor_payload_tampering_is_caught_before_model_and_poisoned() {
        let mut f = setup(None);
        f.owner
            .submit(
                &f.agent,
                "tamper_1",
                input(&f.owner, 1, 1, "immutable original text"),
            )
            .unwrap();
        // A separate privileged control can edit a volatile *pending*
        // Message, but cannot rewrite the already durable SHA/identity.
        f.owner
            .controller
            .update_pending(f.owner.actor, 1, "mutated pending text".into(), 0, 1)
            .unwrap();
        assert!(f.owner.run_once(&mut f.agent, |_| Ok(())).await.is_err());
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            f.owner.fact("tamper_1").unwrap().state,
            InputAdmissionState::Accepted
        );
        assert!(f
            .owner
            .submit(
                &f.agent,
                "tamper_2",
                input(&f.owner, 2, 1, "attempt replay")
            )
            .is_err());
        f.store
            .actor_input_inspect(
                &f.owner.owner,
                &f.owner.session,
                "tamper_1",
                f.owner.epoch.get(),
                "verified: volatile payload changed; nothing may replay",
            )
            .unwrap();
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn immutable_controls_cancel_only_current_turn_without_replaying_provider() {
        let entered = Arc::new(tokio::sync::Notify::new());
        let mut f = setup(Some(entered.clone()));
        f.owner
            .submit(
                &f.agent,
                "paid_ctrl_1",
                input(&f.owner, 1, 1, "long-running attempt"),
            )
            .unwrap();
        let owner = &f.owner;
        let action = async {
            entered.notified().await;
            let frozen = owner
                .control("freeze_2", ActorControlAction::FreezeTurn, 2)
                .unwrap();
            assert_eq!(frozen.state, ActorControlState::Applied);
            assert_eq!(frozen.admission.turn_id, 1);
            assert!(owner
                .control("duplicate", ActorControlAction::FreezeTurn, 2)
                .is_err());
            assert!(owner
                .control("too-old", ActorControlAction::ResumeTurn, 1)
                .is_err());
            let resumed = owner
                .control("resume_3", ActorControlAction::ResumeTurn, 3)
                .unwrap();
            assert_eq!(resumed.state, ActorControlState::Applied);
            let cancelled = owner
                .control("cancel_4", ActorControlAction::CancelTurn, 4)
                .unwrap();
            assert_eq!(cancelled.state, ActorControlState::Applied);
            assert!(owner
                .control("cancel_4", ActorControlAction::CancelTurn, 5)
                .is_err());
            let rows = owner.controls().unwrap();
            assert_eq!(rows.len(), 3);
            assert_eq!(rows[0].admission.request_id, "cancel_4");
            assert_eq!(rows[1].admission.request_id, "freeze_2");
            assert_eq!(rows[2].admission.request_id, "resume_3");
            assert!(rows
                .iter()
                .all(|fact| fact.state == ActorControlState::Applied));
            assert_eq!(
                owner.control_fact("freeze_2").unwrap().state,
                ActorControlState::Applied
            );
            assert!(owner
                .inspect_control("freeze_2", "should never overwrite Applied")
                .is_err());
        };
        let (completed, ()) = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(owner.run_once(&mut f.agent, |_| Ok(())), action)
        })
        .await
        .unwrap();
        let turn = completed.unwrap().unwrap();
        assert_eq!(turn.outcome, aw_agent_core::Outcome::Cancelled);
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            f.owner.fact("paid_ctrl_1").unwrap().state,
            InputAdmissionState::Settled
        );
        assert!(f
            .owner
            .control("frozen_5", ActorControlAction::FreezeActor, 5)
            .is_err());
        assert!(
            f.owner
                .run_once(&mut f.agent, |_| Ok(()))
                .await
                .unwrap()
                .is_none(),
            "completed cancellation never restarts a paid provider"
        );
        // Request IDs are unique across Input and Control classes, not
        // merely within their own tables/stages.
        let current = f.store.session(&f.owner.owner, &f.owner.session).unwrap();
        let branch = f
            .store
            .branches(&f.owner.owner, &f.owner.session)
            .unwrap()
            .into_iter()
            .find(|b| b.id == f.owner.branch)
            .unwrap();
        let forged = ActorInputAdmission {
            request_id: "cancel_4".into(),
            session_id: current.id,
            branch_id: branch.id,
            branch_head: branch.head,
            branch_revision: branch.revision,
            writer_epoch: f.owner.epoch.get(),
            turn_id: current.next_turn,
            sender: label(f.owner.actor),
            target: label(f.owner.actor),
            message_id: 5,
            payload_sha256: "f".repeat(64),
            priority: 0,
        };
        assert!(matches!(
            f.store.actor_input_prepare(&f.owner.owner,forged),
            Err(Error::Conflict(text)) if text.contains("request ID already used")
        ));
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn strict_typed_operations_never_turn_query_into_control_or_replay() {
        assert!(serde_json::from_value::<ActorOperation>(serde_json::json!({
            "op":"control","request_id":"ctrl2","message_id":2,
            "action":"cancel_turn","admin":true
        }))
        .is_err());
        assert!(serde_json::from_value::<ActorOperation>(serde_json::json!({
            "op":"process_write","pid":1,"shell":"bash"
        }))
        .is_err());
        assert!(serde_json::from_value::<ActorOperation>(serde_json::json!({
            "op":"control","request_id":"ctrl2","message_id":2,
            "action":"cancel_subtree"
        }))
        .is_err());
        let mut f = setup(None);
        assert!(f
            .owner
            .operate(ActorOperation::QueryControl {
                request_id: "missing".into()
            })
            .is_err());
        f.owner
            .submit(&f.agent, "op1", input(&f.owner, 1, 1, "once"))
            .unwrap();
        let query = f
            .owner
            .operate(ActorOperation::QueryInput {
                request_id: "op1".into(),
            })
            .unwrap();
        assert!(matches!(query,ActorOperationResult::Input{ref fact}
            if fact.state==InputAdmissionState::Accepted));
        assert!(
            matches!(f.owner.operate(ActorOperation::ListControls).unwrap(),
            ActorOperationResult::Controls{facts} if facts.is_empty())
        );
        assert!(
            f.owner
                .operate(ActorOperation::Control {
                    request_id: "early_ctrl".into(),
                    message_id: 2,
                    action: ActorControlAction::CancelTurn
                })
                .is_err(),
            "cannot send an Actor Control before a real active Turn"
        );
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
        assert!(f
            .owner
            .run_once(&mut f.agent, |_| Ok(()))
            .await
            .unwrap()
            .is_some());
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
        let query = f
            .owner
            .operate(ActorOperation::QueryInput {
                request_id: "op1".into(),
            })
            .unwrap();
        assert!(matches!(query,ActorOperationResult::Input{ref fact}
            if fact.state==InputAdmissionState::Settled));
        assert!(f
            .owner
            .operate(ActorOperation::Control {
                request_id: "late_ctrl".into(),
                message_id: 2,
                action: ActorControlAction::CancelTurn
            })
            .is_err());
        let encoded = serde_json::to_string(&query).unwrap();
        assert!(encoded.contains("settled"));
        assert!(
            !encoded.contains("once"),
            "bounded typed result must never export raw private prompt"
        );
    }

    #[cfg(unix)]
    struct PrivateTemp(std::path::PathBuf);
    #[cfg(unix)]
    impl PrivateTemp {
        fn new() -> Self {
            use std::os::unix::fs::DirBuilderExt;
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "aw-queue-e2e-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&path)
                .unwrap();
            Self(path)
        }
    }
    #[cfg(unix)]
    impl Drop for PrivateTemp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn queued_private_messages_use_same_original_agent_and_cas_receipts_in_priority_order() {
        let temp = PrivateTemp::new();
        let mut f = setup_with_store(None, SessionStore::open(&temp.0).unwrap());
        for (id, seq, text, priority) in [
            ("queue_normal", 1, "queued normal private text", 0),
            ("queue_urgent", 2, "queued urgent private text", -3),
            ("queue_later", 3, "queued later private text", -1),
        ] {
            let mut message = input(&f.owner, seq, 1, text);
            message.turn = TurnTarget::Auto;
            message.priority = priority;
            let fact = f.owner.enqueue(&f.agent, id, message).unwrap();
            assert_eq!(fact.state, aw_store::QueueState::Pending);
            assert_eq!(f.calls.load(Ordering::SeqCst), 0);
            assert!(!serde_json::to_string(&fact).unwrap().contains(text));
        }
        assert!(
            matches!(f.owner.operate(ActorOperation::ListQueue).unwrap(),
            ActorOperationResult::Queued{facts} if facts.len()==3)
        );
        let wrong = PrincipalId::new("other").unwrap();
        assert!(f.store.actor_queue_list(&wrong, &f.owner.session).is_err());
        let public =
            serde_json::to_string(&f.owner.operate(ActorOperation::ListQueue).unwrap()).unwrap();
        for forbidden in [
            "sec_",
            "payload_sha256",
            "payload_ref",
            "owner_process_id",
            "queued normal private",
            "queued urgent private",
        ] {
            assert!(
                !public.contains(forbidden),
                "ActorOperation leaked {forbidden}"
            );
        }
        for (expected, turn_id, private_prompt) in [
            ("queue_urgent", 1, "queued urgent private text"),
            ("queue_later", 2, "queued later private text"),
            ("queue_normal", 3, "queued normal private text"),
        ] {
            let queue_ack = f.owner.admit_next_queued(&f.agent).unwrap().unwrap();
            assert_eq!(queue_ack.state, aw_store::QueueState::Accepted);
            assert_eq!(queue_ack.request.request_id, expected);
            assert_eq!(queue_ack.claim.as_ref().unwrap().turn_id, turn_id);
            assert_eq!(f.calls.load(Ordering::SeqCst), turn_id as usize - 1);
            let input_req = queued_input_request_id(&f.owner.session, expected);
            assert_eq!(
                f.owner.fact(&input_req).unwrap().state,
                InputAdmissionState::Accepted
            );
            let turn = f
                .owner
                .run_once(&mut f.agent, |_| Ok(()))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(turn.id, turn_id);
            assert_eq!(turn.prompt, private_prompt);
            assert_eq!(f.calls.load(Ordering::SeqCst), turn_id as usize);
            assert_eq!(
                f.owner.fact(&input_req).unwrap().state,
                InputAdmissionState::Settled
            );
        }
        assert!(f.owner.admit_next_queued(&f.agent).unwrap().is_none());
        let from_audit = f.owner.queue_list().unwrap();
        assert!(from_audit
            .iter()
            .all(|f| f.state == aw_store::QueueState::Settled));
        assert_eq!(from_audit.len(), 3);
        assert!(
            f.owner
                .operate(ActorOperation::DeleteQueue {
                    request_id: "queue_urgent".into()
                })
                .is_err(),
            "claimed queue item cannot be erased retroactively"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pending_queue_edits_are_cas_fenced_and_private_delete_unlinks_payload() {
        let temp = PrivateTemp::new();
        let f = setup_with_store(None, SessionStore::open(&temp.0).unwrap());
        let mut one = input(&f.owner, 1, 1, "superseded private body");
        one.turn = TurnTarget::Auto;
        let pending = f.owner.enqueue(&f.agent, "editable_one", one).unwrap();
        let previous_ref = pending.payload_ref.clone();
        assert!(f
            .owner
            .update_queued("editable_one", pending.revision, "bad\0body", 0)
            .is_err());
        assert!(f
            .owner
            .update_queued("editable_one", pending.revision + 1, "forged edit", -3)
            .is_err());
        let updated = f
            .owner
            .update_queued("editable_one", pending.revision, "the new private body", -1)
            .unwrap();
        assert_eq!(updated.revision, pending.revision + 1);
        assert_eq!(updated.request.message_id, pending.request.message_id);
        assert_eq!(updated.request.priority, -1);
        assert_ne!(updated.payload_ref, previous_ref);
        assert!(!f
            .store
            .local_secret_store()
            .unwrap()
            .exists(&previous_ref)
            .unwrap());
        assert!(!f
            .owner
            .queue_list()
            .unwrap()
            .iter()
            .any(|q| serde_json::to_string(q).unwrap().contains("private body")));
        assert!(f
            .owner
            .update_queued("editable_one", pending.revision, "stale", 2)
            .is_err());
        let claimed = f.owner.admit_next_queued(&f.agent).unwrap().unwrap();
        assert_eq!(
            claimed.payload_sha256,
            format!("{:x}", Sha256::digest(b"the new private body"))
        );
        assert!(f
            .owner
            .update_queued("editable_one", updated.revision, "too late", 0)
            .is_err());
        assert_eq!(
            f.owner
                .fact(&queued_input_request_id(&f.owner.session, "editable_one"))
                .unwrap()
                .state,
            InputAdmissionState::Accepted
        );
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
        let mut other = input(&f.owner, 3, 1, "private pending deletion");
        other.turn = TurnTarget::Auto;
        let pending = f.owner.enqueue(&f.agent, "to_delete", other).unwrap();
        let deleted = f
            .owner
            .operate(ActorOperation::DeleteQueue {
                request_id: "to_delete".into(),
            })
            .unwrap();
        assert!(matches!(deleted,ActorOperationResult::Queue{fact}
            if fact.state==aw_store::QueueState::Deleted));
        assert!(!f
            .store
            .local_secret_store()
            .unwrap()
            .exists(&pending.payload_ref)
            .unwrap());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unclaimed_private_queue_rehydrates_into_new_owner_runtime_after_restart() {
        let temp = PrivateTemp::new();
        let (session_id, branch_id, old_label) = {
            let f = setup_with_store(None, SessionStore::open(&temp.0).unwrap());
            for (req, seq) in [("restart_a", 1), ("restart_b", 2)] {
                let mut m = input(&f.owner, seq, 1, req);
                m.turn = TurnTarget::Auto;
                f.owner.enqueue(&f.agent, req, m).unwrap();
            }
            assert_eq!(f.calls.load(Ordering::SeqCst), 0);
            (
                f.owner.session.clone(),
                f.owner.branch.clone(),
                label(f.owner.actor),
            )
        };
        let reopened = SessionStore::open(&temp.0).unwrap();
        let principal = PrincipalId::new("local").unwrap();
        let session = reopened.session(&principal, &session_id).unwrap();
        let branch = reopened
            .branches(&principal, &session_id)
            .unwrap()
            .into_iter()
            .find(|b| b.id == branch_id)
            .unwrap();
        let mut f = setup_bound(None, reopened, session, branch, 9);
        assert_ne!(label(f.owner.actor), old_label);
        for (expected, turn) in [("restart_a", 1), ("restart_b", 2)] {
            let ack = f.owner.admit_next_queued(&f.agent).unwrap().unwrap();
            assert_eq!(ack.request.request_id, expected);
            let result = f
                .owner
                .run_once(&mut f.agent, |_| Ok(()))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(result.id, turn);
            assert_eq!(result.prompt, expected);
            assert_eq!(
                f.owner.queue_fact(expected).unwrap().state,
                aw_store::QueueState::Settled
            );
            assert!(!f
                .store
                .local_secret_store()
                .unwrap()
                .exists(&ack.payload_ref)
                .unwrap());
        }
        assert_eq!(f.calls.load(Ordering::SeqCst), 2);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn queue_priority_has_durable_four_urgent_fairness_bound() {
        let temp = PrivateTemp::new();
        let mut f = setup_with_store(None, SessionStore::open(&temp.0).unwrap());
        for i in 1..=6 {
            let text = format!("fair_{i}");
            let mut m = input(&f.owner, i, 1, &text);
            m.turn = TurnTarget::Auto;
            m.priority = if i == 6 { 0 } else { -3 };
            f.owner.enqueue(&f.agent, &text, m).unwrap();
        }
        for (turn, expected) in [
            (1, "fair_1"),
            (2, "fair_2"),
            (3, "fair_3"),
            (4, "fair_4"),
            (5, "fair_6"),
            (6, "fair_5"),
        ] {
            let a = f.owner.admit_next_queued(&f.agent).unwrap().unwrap();
            assert_eq!(a.request.request_id, expected);
            let result = f
                .owner
                .run_once(&mut f.agent, |_| Ok(()))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(result.prompt, expected);
            assert_eq!(result.id, turn);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn queue_cannot_reissue_a_paid_attempt_after_a_dropped_agent_future() {
        let temp = PrivateTemp::new();
        let entered = Arc::new(tokio::sync::Notify::new());
        let mut f = setup_with_store(Some(entered.clone()), SessionStore::open(&temp.0).unwrap());
        for (id, seq) in [("uncertain_a", 1), ("safe_pending_b", 2)] {
            let mut m = input(&f.owner, seq, 1, id);
            m.turn = TurnTarget::Auto;
            f.owner.enqueue(&f.agent, id, m).unwrap();
        }
        f.owner.admit_next_queued(&f.agent).unwrap().unwrap();
        {
            let running = f.owner.run_once(&mut f.agent, |_| Ok(()));
            tokio::pin!(running);
            tokio::time::timeout(Duration::from_secs(3), async {
                tokio::select! {
                    _ = entered.notified() => (),
                    result = &mut running => panic!("paid provider unexpectedly finished: {result:?}"),
                }
            }).await.unwrap();
            // Dropping a paid/in-flight provider is not proof of settlement.
        }
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
        assert!(f.owner.admit_next_queued(&f.agent).is_err());
        assert_eq!(
            f.owner.queue_fact("uncertain_a").unwrap().state,
            aw_store::QueueState::Accepted
        );
        assert_eq!(
            f.owner.queue_fact("safe_pending_b").unwrap().state,
            aw_store::QueueState::Pending
        );
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn aw_p6_9c_bound_operations_reject_revoked_actor_and_transport_control() {
        let f = setup(None);
        assert!(matches!(f.owner.operate_bound(ActorOperation::ListQueue),
            Ok(ActorOperationResult::Queued { facts }) if facts.is_empty()));
        assert!(f
            .owner
            .operate_bound(ActorOperation::Control {
                request_id: "forged_rpc_control".into(),
                message_id: 1,
                action: ActorControlAction::CancelTurn,
            })
            .is_err());
        assert!(ActorOperation::from_rpc_value(serde_json::json!({
            "op":"list_queue","actor_key":"r1g1a1g1","writer_epoch":100
        }))
        .is_err());
        f._runtime
            .lock()
            .unwrap()
            .record_state(
                &f.owner.owner,
                f.owner.actor,
                Some(f.owner.epoch),
                ActorState::Settled,
            )
            .unwrap();
        assert!(
            f.owner.operate_bound(ActorOperation::ListQueue).is_err(),
            "terminal Actor cannot retain transport authority while writer stays active"
        );
        assert!(f
            .owner
            .enqueue_bound_text(
                &f.agent,
                "after_retire",
                1,
                QueueTarget::Auto,
                0,
                "never executed"
            )
            .is_err());
        f._runtime
            .lock()
            .unwrap()
            .release(&f.owner.owner, f.owner.actor, Some(f.owner.epoch))
            .unwrap();
        assert!(
            f.owner.operate_bound(ActorOperation::ListQueue).is_err(),
            "retired Actor generation cannot be used by RPC even with same Session key"
        );
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn forged_owner_epoch_and_unjournaled_context_fail_before_any_actor_admission() {
        let f = setup(None);
        let wrong_epoch = NonZeroU64::new(f.owner.epoch.get() + 1).unwrap();
        assert!(Service::new(
            f.store.clone(),
            f._runtime.clone(),
            f.owner.owner.clone(),
            f.owner.actor,
            wrong_epoch,
            f.owner.session.clone(),
            f.owner.branch.clone()
        )
        .is_err());
        let wrong = PrincipalId::new("local:other").unwrap();
        assert!(Service::new(
            f.store.clone(),
            f._runtime.clone(),
            wrong,
            f.owner.actor,
            f.owner.epoch,
            f.owner.session.clone(),
            f.owner.branch.clone()
        )
        .is_err());
        assert!(f
            .owner
            .submit(&f.agent, "bad_id", input(&f.owner, 1, 2, "wrong turn"))
            .is_err());
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    }
    #[cfg(all(feature = "pipelines", feature = "routing"))]
    #[test]
    fn aw_p6_8c_guardian_approval_ui_rejects_forged_or_terminal_original_actor() {
        let f = setup(None);
        assert!(crate::approval_ui::pending(&f.store, &f.owner)
            .unwrap()
            .is_empty());
        assert!(crate::approval_ui::inspect(&f.store, &f.owner, "forged_review_id").is_err());
        assert!(crate::approval_ui::decide(
            &f.store,
            &f.owner,
            "forged_review_id",
            &"a".repeat(64),
            true
        )
        .is_err());
        f._runtime
            .lock()
            .unwrap()
            .record_state(
                &f.owner.owner,
                f.owner.actor,
                Some(f.owner.epoch),
                ActorState::Settled,
            )
            .unwrap();
        assert!(
            crate::approval_ui::pending(&f.store, &f.owner).is_err(),
            "a terminal original Actor may not impersonate a future human reviewer"
        );
    }

    struct ModelObserverFixtureProvider {
        calls: Arc<AtomicUsize>,
        seen: Arc<Mutex<Vec<String>>>,
    }
    impl Provider for ModelObserverFixtureProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async {
                Err(Error::Protocol(
                    "fixture expected typed original tool provider",
                ))
            })
        }
        fn stream_tools<'a>(
            &'a self,
            messages: &'a [aw_agent_core::Message],
            _: &'a str,
            specs: &'a [aw_agent_core::ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> aw_agent_core::provider::ModelFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.seen.lock().unwrap().push(format!("{}", specs.len()));
                if messages.last().is_some_and(|m| m.role == Role::Tool) {
                    sink("observed and done")?;
                    Ok(aw_agent_core::provider::ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(3),
                                output_tokens: Some(2),
                            },
                        },
                        ..Default::default()
                    })
                } else {
                    Ok(aw_agent_core::provider::ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            ..Default::default()
                        },
                        tool_calls: vec![
                            aw_agent_core::ToolCall {
                                id: "agent1".into(),
                                name: "agent".into(),
                                arguments: serde_json::json!({"op":"status"}),
                            },
                            aw_agent_core::ToolCall {
                                id: "queue1".into(),
                                name: "queue".into(),
                                arguments: serde_json::json!({"op":"list"}),
                            },
                            aw_agent_core::ToolCall {
                                id: "process1".into(),
                                name: "process".into(),
                                arguments: serde_json::json!({"op":"status","handle":"owned_proc"}),
                            },
                        ],
                        provider_context: None,
                    })
                }
            })
        }
    }
    struct ModelQueuedFutureProvider {
        model_calls: Arc<AtomicUsize>,
        duplicate_proposals: bool,
    }
    impl Provider for ModelQueuedFutureProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async {
                Err(Error::Protocol(
                    "model queue fixture requires original tool-aware invoke",
                ))
            })
        }
        fn stream_tools<'a>(
            &'a self,
            messages: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a [aw_agent_core::ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> aw_agent_core::provider::ModelFuture<'a> {
            Box::pin(async move {
                self.model_calls.fetch_add(1, Ordering::SeqCst);
                let last = messages.last();
                if last.is_some_and(|m| m.role == Role::Tool)
                    || last.is_some_and(|m| {
                        m.role == Role::User && m.content == "MODEL_FUTURE_PRIVATE_PAYLOAD"
                    })
                {
                    sink("existing Agent completed")?;
                    return Ok(aw_agent_core::provider::ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(3),
                                output_tokens: Some(2),
                            },
                        },
                        ..Default::default()
                    });
                }
                let mut tools = vec![aw_agent_core::ToolCall {
                    id: "write_once_1".into(),
                    name: "queue_enqueue".into(),
                    arguments: serde_json::json!({
                        "op":"enqueue","text":"MODEL_FUTURE_PRIVATE_PAYLOAD",
                    }),
                }];
                if self.duplicate_proposals {
                    tools.push(aw_agent_core::ToolCall {
                        id: "write_once_2".into(),
                        name: "queue_enqueue".into(),
                        arguments: serde_json::json!({
                            "op":"enqueue","text":"MALICIOUS_SECOND_SAME_TURN_BODY",
                        }),
                    });
                }
                Ok(aw_agent_core::provider::ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "tool_calls".into(),
                        ..Default::default()
                    },
                    tool_calls: tools,
                    provider_context: None,
                })
            })
        }
    }
    #[derive(Clone, Copy, Debug)]
    enum ModelDispositionFixtureMode {
        Update,
        Delete,
        Conflict,
    }
    struct ModelDispositionFixtureProvider {
        target: String,
        mode: ModelDispositionFixtureMode,
        calls: Arc<AtomicUsize>,
    }
    impl Provider for ModelDispositionFixtureProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async {
                Err(Error::Protocol(
                    "disposition fixture requires original tool-aware Agent",
                ))
            })
        }
        fn stream_tools<'a>(
            &'a self,
            messages: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a [aw_agent_core::ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> aw_agent_core::provider::ModelFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                if messages.last().is_some_and(|last| {
                    last.role == Role::Tool
                        || (last.role == Role::User
                            && matches!(
                                last.content.as_str(),
                                "EDITED_PRIVATE_MODEL_BODY" | "ORIGINAL_MODEL_PRIVATE_BODY"
                            ))
                }) {
                    sink("original Agent finished factual Tool observation")?;
                    return Ok(aw_agent_core::provider::ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(3),
                                output_tokens: Some(3),
                            },
                        },
                        ..Default::default()
                    });
                }
                let latest = messages.last().ok_or(Error::Protocol(
                    "missing factual original Agent user prompt",
                ))?;
                let proposal = match latest.content.as_str() {
                    "seed previous model-owned pending" => vec![aw_agent_core::ToolCall {
                        id: "model_seed_1".into(),
                        name: "queue_enqueue".into(),
                        arguments: serde_json::json!({"op":"enqueue","text":"ORIGINAL_MODEL_PRIVATE_BODY"}),
                    }],
                    "mutate previous model pending" => {
                        let update = aw_agent_core::ToolCall {
                            id: "model_update_2".into(),
                            name: "queue_update".into(),
                            arguments: serde_json::json!({"op":"update","request_id":self.target,
                                "expected_revision":1,"text":"EDITED_PRIVATE_MODEL_BODY"}),
                        };
                        let deletion = aw_agent_core::ToolCall {
                            id: "model_delete_2".into(),
                            name: "queue_delete".into(),
                            arguments: serde_json::json!({"op":"delete","request_id":self.target,
                                "expected_revision":1}),
                        };
                        match self.mode {
                            ModelDispositionFixtureMode::Update => vec![update],
                            ModelDispositionFixtureMode::Delete => vec![deletion],
                            ModelDispositionFixtureMode::Conflict => vec![update, deletion],
                        }
                    }
                    _ => {
                        return Err(Error::Protocol(
                            "model disposition fixture cannot create unknown paid attempt",
                        ))
                    }
                };
                Ok(aw_agent_core::provider::ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "tool_calls".into(),
                        ..Default::default()
                    },
                    tool_calls: proposal,
                    provider_context: None,
                })
            })
        }
    }
    fn disposition_grant(name: &str) -> aw_agent_kernel::envelope::ExactGrant {
        aw_agent_kernel::envelope::ExactGrant {
            name: name.into(),
            kind: aw_agent_kernel::capability::SourceKind::Host,
            source: "trusted-host".into(),
            source_epoch: 1,
            effect: aw_agent_kernel::capability::Effect::Write,
        }
    }
    #[test]
    fn aw_p6_9d2b_disposition_requires_its_own_exact_host_write_source() {
        use crate::model_effects::{self, DispositionTool};
        let mut plain = setup(None);
        let both = [DispositionTool::Update, DispositionTool::Delete]
            .into_iter()
            .collect();
        assert!(
            model_effects::install_queue_dispositions(&plain.owner, &mut plain.agent, &both)
                .is_err()
        );
        assert!(plain.agent.tool_specs().is_empty());
        for unauthorized in [
            model_host_grants(&["queue_update"])[0].clone(),
            aw_agent_kernel::envelope::ExactGrant {
                name: "queue_update".into(),
                kind: aw_agent_kernel::capability::SourceKind::Host,
                source: "trusted-host".into(),
                source_epoch: 7,
                effect: aw_agent_kernel::capability::Effect::Write,
            },
            aw_agent_kernel::envelope::ExactGrant {
                name: "queue_update".into(),
                kind: aw_agent_kernel::capability::SourceKind::Extension,
                source: "remote-mcp".into(),
                source_epoch: 1,
                effect: aw_agent_kernel::capability::Effect::Write,
            },
            disposition_grant("queue_enqueue"),
        ] {
            let store = SessionStore::in_memory().unwrap();
            let principal = PrincipalId::new("local").unwrap();
            let (session, branch) = store
                .create(
                    &principal,
                    "D2B exact source preflight",
                    StoredSessionBinding {
                        working_directory: PathBuf::from("/tmp"),
                        tool_root: PathBuf::from("/tmp"),
                        system_prompt: "system".into(),
                        profile_hint: "fixture".into(),
                        model_hint: "model".into(),
                        destination_fingerprint: "0".repeat(64),
                    },
                )
                .unwrap();
            let mut f =
                setup_bound_with_tools(None, store, session, branch, 2, vec![unauthorized], None);
            assert!(
                model_effects::install_queue_dispositions(
                    &f.owner,
                    &mut f.agent,
                    &[DispositionTool::Update].into_iter().collect()
                )
                .is_err(),
                "D1 Read or wrong Host source/epoch/name never authorizes model Queue Write"
            );
            assert!(f.agent.tool_specs().is_empty());
        }
        let store = SessionStore::in_memory().unwrap();
        let principal = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(
                &principal,
                "D2B all-or-nothing allowlist",
                StoredSessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let mut f = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            2,
            vec![disposition_grant("queue_update")],
            None,
        );
        assert!(model_effects::install_queue_dispositions(&f.owner,&mut f.agent,&both).is_err(),
            "missing exact Delete grant must deny BOTH before installing any original ToolPolicy entry");
        assert!(f.agent.tool_specs().is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn aw_p6_9d2b_original_agent_model_update_reencrypts_prior_proven_future() {
        use crate::model_effects::{self, DispositionTool};
        let temp = PrivateTemp::new();
        let store = SessionStore::open(&temp.0).unwrap();
        let principal = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(
                &principal,
                "real model disposition Update",
                StoredSessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let original_request = aw_store::model_queue_request_id(&session.id, &branch.id, 1);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut f = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            2,
            vec![model_write_grant(), disposition_grant("queue_update")],
            Some(Arc::new(ModelDispositionFixtureProvider {
                target: original_request.clone(),
                mode: ModelDispositionFixtureMode::Update,
                calls: calls.clone(),
            })),
        );
        assert!(f.agent.tool_specs().is_empty());
        model_effects::install_queue_enqueue(&f.owner, &mut f.agent).unwrap();
        model_effects::install_queue_dispositions(
            &f.owner,
            &mut f.agent,
            &[DispositionTool::Update].into_iter().collect(),
        )
        .unwrap();
        assert_eq!(f.agent.tool_specs().len(), 2);
        assert!(f
            .agent
            .tool_specs()
            .iter()
            .all(|spec| spec.effect == aw_agent_core::ToolEffect::Write));
        f.owner
            .submit(
                &f.agent,
                "original_d2a_seed",
                input(&f.owner, 1, 1, "seed previous model-owned pending"),
            )
            .unwrap();
        let first = f
            .owner
            .run_once(&mut f.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.id, 1);
        assert_eq!(first.tool_records.len(), 1);
        let earlier = f.owner.queue_fact(&original_request).unwrap();
        assert_eq!(earlier.state, aw_store::QueueState::Pending);
        assert_eq!(earlier.revision, 1);
        assert!(earlier.request.message_id >= 2);
        let old_ref = earlier.payload_ref.clone();
        f.owner
            .submit(
                &f.agent,
                "original_d2b_update",
                input(&f.owner, 3, 2, "mutate previous model pending"),
            )
            .unwrap();
        let second = f
            .owner
            .run_once(&mut f.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(second.id, 2);
        assert_eq!(second.tool_records.len(), 1);
        assert_eq!(second.tool_records[0].call.name, "queue_update");
        assert_eq!(
            second.tool_records[0].status,
            aw_agent_core::ToolStatus::Succeeded
        );
        assert_eq!(
            second.tool_records[0].effect,
            aw_agent_core::ToolEffect::Write
        );
        let output = &second.tool_records[0].output.as_ref().unwrap().content;
        assert!(output.contains("NOT_DISPATCHED"));
        assert!(output.contains("updated"));
        assert!(!output.contains("EDITED_PRIVATE_MODEL_BODY"));
        let changed = f.owner.queue_fact(&original_request).unwrap();
        assert_eq!(changed.revision, 2);
        assert_eq!(changed.state, aw_store::QueueState::Pending);
        assert_eq!(changed.request.message_id, earlier.request.message_id);
        assert_eq!(changed.request.priority, earlier.request.priority);
        assert_eq!(
            changed.request.recipient_site,
            earlier.request.recipient_site
        );
        assert_ne!(changed.payload_ref, old_ref);
        assert!(!f
            .store
            .local_secret_store()
            .unwrap()
            .exists(&old_ref)
            .unwrap());
        assert_eq!(
            f.store
                .local_secret_store()
                .unwrap()
                .get(&changed.payload_ref)
                .unwrap()
                .as_slice(),
            b"EDITED_PRIVATE_MODEL_BODY"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        assert!(
            f.owner.admit_next_queued(&f.agent).unwrap().is_some(),
            "only explicit owning host claim can invoke next model Turn"
        );
        let third = f
            .owner
            .run_once(&mut f.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(third.id, 3);
        assert_eq!(third.prompt, "EDITED_PRIVATE_MODEL_BODY");
        assert_eq!(
            third.outcome,
            aw_agent_core::conversation::Outcome::Completed
        );
        assert!(third.tool_records.is_empty());
        assert_eq!(
            f.owner.queue_fact(&original_request).unwrap().state,
            aw_store::QueueState::Settled
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn aw_p6_9d2b_original_agent_model_delete_never_executes_future() {
        use crate::model_effects::{self, DispositionTool};
        let temp = PrivateTemp::new();
        let store = SessionStore::open(&temp.0).unwrap();
        let principal = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(
                &principal,
                "real model disposition Delete",
                StoredSessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let original_request = aw_store::model_queue_request_id(&session.id, &branch.id, 1);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut f = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            2,
            vec![model_write_grant(), disposition_grant("queue_delete")],
            Some(Arc::new(ModelDispositionFixtureProvider {
                target: original_request.clone(),
                mode: ModelDispositionFixtureMode::Delete,
                calls: calls.clone(),
            })),
        );
        model_effects::install_queue_enqueue(&f.owner, &mut f.agent).unwrap();
        model_effects::install_queue_dispositions(
            &f.owner,
            &mut f.agent,
            &[DispositionTool::Delete].into_iter().collect(),
        )
        .unwrap();
        f.owner
            .submit(
                &f.agent,
                "original_d2a_seed",
                input(&f.owner, 1, 1, "seed previous model-owned pending"),
            )
            .unwrap();
        f.owner
            .run_once(&mut f.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        let earlier = f.owner.queue_fact(&original_request).unwrap();
        f.owner
            .submit(
                &f.agent,
                "original_d2b_delete",
                input(&f.owner, 3, 2, "mutate previous model pending"),
            )
            .unwrap();
        let result = f
            .owner
            .run_once(&mut f.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.tool_records.len(), 1);
        assert_eq!(result.tool_records[0].call.name, "queue_delete");
        assert_eq!(
            result.tool_records[0].status,
            aw_agent_core::ToolStatus::Succeeded
        );
        assert_eq!(
            f.owner.queue_fact(&original_request).unwrap().state,
            aw_store::QueueState::Deleted
        );
        assert!(!f
            .store
            .local_secret_store()
            .unwrap()
            .exists(&earlier.payload_ref)
            .unwrap());
        assert!(
            f.owner.admit_next_queued(&f.agent).unwrap().is_none(),
            "deleting only Pending prevents next paid Agent execution forever"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            4,
            "two already journaled original Agent Turns, no future third paid Turn"
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn aw_p6_9d2b_conflicting_model_update_delete_is_unknown_not_replayed() {
        use crate::model_effects::{self, DispositionTool};
        let temp = PrivateTemp::new();
        let store = SessionStore::open(&temp.0).unwrap();
        let principal = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(
                &principal,
                "conflicting model disposition",
                StoredSessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let original_request = aw_store::model_queue_request_id(&session.id, &branch.id, 1);
        let mut f = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            2,
            vec![
                model_write_grant(),
                disposition_grant("queue_update"),
                disposition_grant("queue_delete"),
            ],
            Some(Arc::new(ModelDispositionFixtureProvider {
                target: original_request.clone(),
                mode: ModelDispositionFixtureMode::Conflict,
                calls: Arc::new(AtomicUsize::new(0)),
            })),
        );
        model_effects::install_queue_enqueue(&f.owner, &mut f.agent).unwrap();
        model_effects::install_queue_dispositions(
            &f.owner,
            &mut f.agent,
            &[DispositionTool::Update, DispositionTool::Delete]
                .into_iter()
                .collect(),
        )
        .unwrap();
        f.owner
            .submit(
                &f.agent,
                "original_d2a_seed",
                input(&f.owner, 1, 1, "seed previous model-owned pending"),
            )
            .unwrap();
        f.owner
            .run_once(&mut f.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        f.owner
            .submit(
                &f.agent,
                "original_d2b_conflict",
                input(&f.owner, 3, 2, "mutate previous model pending"),
            )
            .unwrap();
        assert!(
            f.owner.run_once(&mut f.agent, |_| Ok(())).await.is_err(),
            "contradictory model Write ToolCalls must retain original hazard, not false success"
        );
        let latest = f.owner.queue_fact(&original_request).unwrap();
        assert_eq!(latest.revision, 2, "only ONE conflicting Write may commit");
        assert!(matches!(
            latest.state,
            aw_store::QueueState::Pending | aw_store::QueueState::Deleted
        ));
        assert!(
            f.owner.admit_next_queued(&f.agent).is_err(),
            "original unresolved Tool effect requires inspection before another Input"
        );
        assert!(
            f.owner
                .queue_list()
                .unwrap()
                .iter()
                .filter(|fact| fact.request.request_id == original_request)
                .count()
                == 1,
            "original queue projects exactly one current fact despite competing Write proposals"
        );
    }

    struct ModelProcessCloseProvider {
        calls: Arc<AtomicUsize>,
        duplicate: bool,
        terminate: bool,
    }
    impl Provider for ModelProcessCloseProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async {
                Err(Error::Protocol(
                    "model Process requires existing original tool provider",
                ))
            })
        }
        fn stream_tools<'a>(
            &'a self,
            messages: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a [aw_agent_core::ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> aw_agent_core::provider::ModelFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                if messages.last().is_some_and(|m| m.role == Role::Tool) {
                    sink("original Process intent recorded; Process effects are separately journaled")?;
                    return Ok(aw_agent_core::provider::ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(2),
                                output_tokens: Some(2),
                            },
                        },
                        ..Default::default()
                    });
                }
                let mut requests = vec![aw_agent_core::ToolCall {
                    id: "io_first".into(),
                    name: if self.terminate {
                        "process_terminate"
                    } else {
                        "process_close_stdin"
                    }
                    .into(),
                    arguments: serde_json::json!({"op":if self.terminate {"terminate"} else {"close_stdin"},"handle":"owned_cat"}),
                }];
                if self.duplicate {
                    requests.push(aw_agent_core::ToolCall {
                        id: "io_duplicate".into(),
                        name: if self.terminate {"process_terminate"} else {"process_close_stdin"}.into(),
                        arguments: serde_json::json!({"op":if self.terminate {"terminate"} else {"close_stdin"},"handle":"owned_cat"}),
                    });
                }
                Ok(aw_agent_core::provider::ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "tool_calls".into(),
                        ..Default::default()
                    },
                    tool_calls: requests,
                    provider_context: None,
                })
            })
        }
    }
    fn process_close_grant() -> aw_agent_kernel::envelope::ExactGrant {
        aw_agent_kernel::envelope::ExactGrant {
            name: "process_close_stdin".into(),
            kind: aw_agent_kernel::capability::SourceKind::Host,
            source: "trusted-host".into(),
            source_epoch: 1,
            effect: aw_agent_kernel::capability::Effect::Write,
        }
    }
    fn process_terminate_grant() -> aw_agent_kernel::envelope::ExactGrant {
        aw_agent_kernel::envelope::ExactGrant {
            name: "process_terminate".into(),
            kind: aw_agent_kernel::capability::SourceKind::Host,
            source: "trusted-host".into(),
            source_epoch: 1,
            effect: aw_agent_kernel::capability::Effect::Write,
        }
    }
    #[cfg(unix)]
    fn native_cat_path() -> PathBuf {
        std::env::var_os("PATH")
            .into_iter()
            .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
            .map(|part| part.join("cat"))
            .find(|p| p.is_file())
            .expect("test environment requires native cat fixture")
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn aw_p6_10b_rebound_actor_cannot_inherit_host_registered_process_handles() {
        use crate::model_management::ProcessRegistry;
        use aw_tools::process::{PipeProcess, PipeSpec};
        use std::collections::BTreeMap;

        let temp = PrivateTemp::new();
        let store = SessionStore::open(&temp.0).unwrap();
        let principal = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(
                &principal,
                "model Process Actor-generation fence",
                StoredSessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let old = setup_bound(None, store.clone(), session.clone(), branch.clone(), 2);
        let rebound = setup_bound(None, store, session, branch, 3);
        assert_ne!(old.owner.actor, rebound.owner.actor);
        assert_eq!(
            old.owner.bound_peer_scope(),
            rebound.owner.bound_peer_scope()
        );
        old.owner.verify_rpc_state().unwrap();
        rebound.owner.verify_rpc_state().unwrap();

        let process = Arc::new(tokio::sync::Mutex::new(
            PipeProcess::spawn(PipeSpec {
                program: native_cat_path(),
                root: std::env::temp_dir(),
                args: Vec::new(),
                env: BTreeMap::new(),
                stdin_open: true,
            })
            .await
            .unwrap(),
        ));
        let registry = ProcessRegistry::default();
        registry
            .register_terminable_owned(
                &old.owner,
                &old.agent,
                "prior_actor_process",
                process.clone(),
            )
            .unwrap();
        assert!(registry.resolve(&old.owner, "prior_actor_process").is_ok());
        assert!(registry
            .resolve_terminable(&old.owner, "prior_actor_process")
            .is_ok());
        assert!(
            registry
                .resolve(&rebound.owner, "prior_actor_process")
                .is_err(),
            "new Actor generation cannot inherit prior Actor Process observation"
        );
        assert!(
            registry
                .resolve_terminable(&rebound.owner, "prior_actor_process")
                .is_err(),
            "new Actor generation cannot inherit destructive termination grant"
        );
        assert!(
            registry
                .unregister_owned(&rebound.owner, "prior_actor_process")
                .is_err(),
            "new Actor generation cannot steal old Process registry custody"
        );
        registry
            .unregister_owned(&old.owner, "prior_actor_process")
            .unwrap();
        // Even the correct generation loses the resource on retirement.
        // An opaque handle is NEVER durable or a permission after revocation.
        let second_registry = ProcessRegistry::default();
        second_registry
            .register_terminable_owned(
                &rebound.owner,
                &rebound.agent,
                "rebound_owned_process",
                process.clone(),
            )
            .unwrap();
        assert!(second_registry
            .resolve_terminable(&rebound.owner, "rebound_owned_process")
            .is_ok());
        rebound
            ._runtime
            .lock()
            .unwrap()
            .record_state(
                &rebound.owner.owner,
                rebound.owner.actor,
                Some(rebound.owner.epoch),
                ActorState::Settled,
            )
            .unwrap();
        assert!(
            second_registry
                .resolve(&rebound.owner, "rebound_owned_process")
                .is_err(),
            "retired Process owner can no longer observe its own handle"
        );
        assert!(
            second_registry
                .resolve_terminable(&rebound.owner, "rebound_owned_process")
                .is_err(),
            "retired Process owner can no longer terminate its old child"
        );
        drop(second_registry);
        let mut process = Arc::try_unwrap(process)
            .unwrap_or_else(|_| panic!("test Process registry leaked original resource"))
            .into_inner();
        process.close_stdin();
        assert!(process
            .wait(std::future::pending(), Duration::from_secs(3))
            .await
            .finished());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn aw_p6_9d2b2b1_real_agent_terminates_only_separately_approved_original_process() {
        use crate::model_management::ProcessRegistry;
        use aw_tools::process::{PipeProcess, PipeSpec};
        use std::collections::BTreeMap;
        let tmp = PrivateTemp::new();
        let store = SessionStore::open(&tmp.0).unwrap();
        let principal = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(
                &principal,
                "model original Process terminate",
                StoredSessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let mut f = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            2,
            vec![process_terminate_grant()],
            Some(Arc::new(ModelProcessCloseProvider {
                calls: count.clone(),
                duplicate: false,
                terminate: true,
            })),
        );
        assert!(
            f.agent.tool_specs().is_empty(),
            "Process TERMINATE must never be installed by default"
        );
        let p = Arc::new(tokio::sync::Mutex::new(
            PipeProcess::spawn(PipeSpec {
                program: native_cat_path(),
                root: std::env::temp_dir(),
                args: Vec::new(),
                env: BTreeMap::new(),
                stdin_open: true,
            })
            .await
            .unwrap(),
        ));
        let registry = ProcessRegistry::default();
        registry
            .register_owned(&f.owner, &f.agent, "ordinary_obs", p.clone())
            .unwrap();
        assert!(
            registry
                .resolve_terminable(&f.owner, "ordinary_obs")
                .is_err(),
            "ordinary original Process observation handle MUST NOT imply OS termination authority"
        );
        registry
            .register_terminable_owned(&f.owner, &f.agent, "owned_cat", p.clone())
            .unwrap();
        assert!(registry
            .register_terminable_owned(&f.owner, &f.agent, "owned_cat", p.clone())
            .is_err());
        crate::model_process_control::install_terminate(&f.owner, &mut f.agent, registry.clone())
            .unwrap();
        assert_eq!(f.agent.tool_specs().len(), 1);
        assert_eq!(
            f.agent.tool_specs()[0].effect,
            aw_agent_core::ToolEffect::Write
        );
        assert_eq!(f.agent.tool_specs()[0].name, "process_terminate");
        f.owner
            .submit(
                &f.agent,
                "original_terminate_input",
                input(
                    &f.owner,
                    1,
                    1,
                    "terminate this separately host-approved original Process",
                ),
            )
            .unwrap();
        let finished = f
            .owner
            .run_once(&mut f.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(finished.id, 1);
        assert_eq!(finished.tool_records.len(), 1);
        assert_eq!(finished.tool_records[0].call.name, "process_terminate");
        assert_eq!(
            finished.tool_records[0].effect,
            aw_agent_core::ToolEffect::Write
        );
        assert_eq!(
            finished.tool_records[0].status,
            aw_agent_core::ToolStatus::Succeeded
        );
        let raw = &finished.tool_records[0].output.as_ref().unwrap().content;
        assert!(raw.contains("original_process_termination"));
        assert!(raw.contains("direct_child_reaped"));
        assert!(raw.contains("BEST_EFFORT"));
        assert!(raw.contains("NEVER_AUTOMATIC"));
        for forbidden in [
            "pid", "stdin", "stdout", "stderr", "program", "env", "shell", "command", "argv",
        ] {
            assert!(
                !raw.contains(&format!(r#""{forbidden}""#)),
                "Process termination tool exposed {forbidden}"
            );
        }
        let response: serde_json::Value = serde_json::from_str(raw).unwrap();
        let id = response["fact"].as_str().unwrap();
        let saved = f.owner.model_process_fact(id).unwrap();
        assert_eq!(saved.state, aw_store::ModelProcessState::Applied);
        assert_eq!(
            saved.request.action,
            aw_store::ModelProcessAction::Terminate
        );
        assert_eq!(
            saved.request.process_binding.len(),
            64,
            "OS Process incarnation must be pinned in durable intent"
        );
        assert_eq!(saved.request.handle, "owned_cat");
        assert!(
            !p.lock().await.termination_available().unwrap(),
            "original Process cannot be terminated again after true OS wait"
        );
        assert!(
            p.lock().await.terminate().await.is_err(),
            "original Process OS group may never receive a second termination attempt"
        );
        assert_eq!(
            count.load(Ordering::SeqCst),
            2,
            "one original paid tool proposal and one completion, not another model executor"
        );
        assert!(crate::model_process_control::install_terminate(
            &f.owner,
            &mut f.agent,
            registry.clone()
        )
        .is_err());
        registry.unregister_owned(&f.owner, "owned_cat").unwrap();
        registry.unregister_owned(&f.owner, "ordinary_obs").unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn aw_p6_9d2b2b1_duplicate_termination_proposals_preserve_original_unknown_hazard() {
        use crate::model_management::ProcessRegistry;
        use aw_tools::process::{PipeProcess, PipeSpec};
        use std::collections::BTreeMap;
        let tmp = PrivateTemp::new();
        let store = SessionStore::open(&tmp.0).unwrap();
        let principal = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(
                &principal,
                "two model original Process terminate calls",
                StoredSessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let mut f = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            2,
            vec![process_terminate_grant()],
            Some(Arc::new(ModelProcessCloseProvider {
                calls: Arc::new(AtomicUsize::new(0)),
                duplicate: true,
                terminate: true,
            })),
        );
        let p = Arc::new(tokio::sync::Mutex::new(
            PipeProcess::spawn(PipeSpec {
                program: native_cat_path(),
                root: std::env::temp_dir(),
                args: Vec::new(),
                env: BTreeMap::new(),
                stdin_open: true,
            })
            .await
            .unwrap(),
        ));
        let registry = ProcessRegistry::default();
        registry
            .register_terminable_owned(&f.owner, &f.agent, "owned_cat", p.clone())
            .unwrap();
        crate::model_process_control::install_terminate(&f.owner, &mut f.agent, registry.clone())
            .unwrap();
        f.owner
            .submit(
                &f.agent,
                "original_duplicate_terminate",
                input(&f.owner, 1, 1, "duplicate Process termination proposals"),
            )
            .unwrap();
        assert!(
            f.owner.run_once(&mut f.agent, |_| Ok(())).await.is_err(),
            "second conflicting original Write proposal must not settle a paid Turn"
        );
        assert!(!p.lock().await.termination_available().unwrap());
        assert!(
            p.lock().await.terminate().await.is_err(),
            "original OS terminate is once-only"
        );
        assert!(
            f.owner.admit_next_queued(&f.agent).is_err(),
            "original paid Tool hazard must block future queued work until inspected"
        );
        registry.unregister_owned(&f.owner, "owned_cat").unwrap();
    }
    #[test]
    fn aw_p6_9d2b2b1_process_terminate_requires_new_exact_write_grant() {
        let registry = crate::model_management::ProcessRegistry::default();
        let mut ordinary = setup(None);
        assert!(crate::model_process_control::install_terminate(
            &ordinary.owner,
            &mut ordinary.agent,
            registry.clone()
        )
        .is_err());
        assert!(ordinary.agent.tool_specs().is_empty());
        for grant in [
            model_host_grants(&["process_terminate"])[0].clone(),
            process_close_grant(),
            aw_agent_kernel::envelope::ExactGrant {
                name: "process_terminate".into(),
                kind: aw_agent_kernel::capability::SourceKind::Host,
                source: "trusted-host".into(),
                source_epoch: 20,
                effect: aw_agent_kernel::capability::Effect::Write,
            },
            aw_agent_kernel::envelope::ExactGrant {
                name: "process_terminate".into(),
                kind: aw_agent_kernel::capability::SourceKind::Extension,
                source: "remote_mcp".into(),
                source_epoch: 1,
                effect: aw_agent_kernel::capability::Effect::Write,
            },
        ] {
            let store = SessionStore::in_memory().unwrap();
            let principal = PrincipalId::new("local").unwrap();
            let (session, branch) = store
                .create(
                    &principal,
                    "Terminate exact source pregrant",
                    StoredSessionBinding {
                        working_directory: PathBuf::from("/tmp"),
                        tool_root: PathBuf::from("/tmp"),
                        system_prompt: "system".into(),
                        profile_hint: "fixture".into(),
                        model_hint: "model".into(),
                        destination_fingerprint: "0".repeat(64),
                    },
                )
                .unwrap();
            let mut f = setup_bound_with_tools(None, store, session, branch, 2, vec![grant], None);
            assert!(crate::model_process_control::install_terminate(
                &f.owner,&mut f.agent,registry.clone()).is_err(),
                "close-stdin, Read, extension source and wrong epoch never imply Process termination");
            assert!(f.agent.tool_specs().is_empty());
        }
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn aw_p6_9d2b2a_original_agent_closes_only_host_owned_process_stdin_with_receipts() {
        use crate::model_management::ProcessRegistry;
        use aw_tools::process::{PipeProcess, PipeSpec};
        use std::collections::BTreeMap;
        let tmp = PrivateTemp::new();
        let store = SessionStore::open(&tmp.0).unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(
                &actor,
                "Process model control exact origin",
                StoredSessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let provider_calls = Arc::new(AtomicUsize::new(0));
        let mut fixture = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            2,
            vec![process_close_grant()],
            Some(Arc::new(ModelProcessCloseProvider {
                calls: provider_calls.clone(),
                duplicate: false,
                terminate: false,
            })),
        );
        assert!(
            fixture.agent.tool_specs().is_empty(),
            "pre-minted original Actor Envelope does NOT automatically enable model Process I/O"
        );
        let process = Arc::new(tokio::sync::Mutex::new(
            PipeProcess::spawn(PipeSpec {
                program: native_cat_path(),
                root: std::env::temp_dir(),
                args: Vec::new(),
                env: BTreeMap::new(),
                stdin_open: true,
            })
            .await
            .unwrap(),
        ));
        let registry = ProcessRegistry::default();
        registry
            .register_owned(&fixture.owner, &fixture.agent, "owned_cat", process.clone())
            .unwrap();
        crate::model_process_control::install_close_stdin(
            &fixture.owner,
            &mut fixture.agent,
            registry.clone(),
        )
        .unwrap();
        assert_eq!(fixture.agent.tool_specs().len(), 1);
        assert_eq!(
            fixture.agent.tool_specs()[0].effect,
            aw_agent_core::ToolEffect::Write
        );
        fixture
            .owner
            .submit(
                &fixture.agent,
                "process_original",
                input(
                    &fixture.owner,
                    1,
                    1,
                    "close only already authorized Process stdin",
                ),
            )
            .unwrap();
        let turn = fixture
            .owner
            .run_once(&mut fixture.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(turn.id, 1);
        assert_eq!(turn.tool_records.len(), 1);
        assert_eq!(turn.tool_records[0].call.name, "process_close_stdin");
        assert_eq!(
            turn.tool_records[0].status,
            aw_agent_core::ToolStatus::Succeeded
        );
        assert_eq!(
            turn.tool_records[0].effect,
            aw_agent_core::ToolEffect::Write
        );
        let receipt = &turn.tool_records[0].output.as_ref().unwrap().content;
        assert!(receipt.contains("original_process_stdin_close"));
        assert!(receipt.contains("NOT_PROVEN"));
        assert!(receipt.contains("NEVER_AUTOMATIC"));
        for key in ["pid", "program", "stdout", "stderr", "env", "command"] {
            assert!(
                !receipt.contains(&format!(r#""{key}""#)),
                "model received private Process metadata {key}"
            );
        }
        assert!(!process.lock().await.stdin_open());
        let encoded: serde_json::Value = serde_json::from_str(receipt).unwrap();
        let id = encoded["fact"].as_str().unwrap();
        let fact = fixture.owner.model_process_fact(id).unwrap();
        assert_eq!(fact.state, aw_store::ModelProcessState::Applied);
        assert_eq!(fact.request.handle, "owned_cat");
        assert_eq!(fact.request.turn_id, 1);
        assert_eq!(provider_calls.load(Ordering::SeqCst),2,
            "only one original paid provider proposal plus original completion, no model/process executor");
        assert!(crate::model_process_control::install_close_stdin(
            &fixture.owner,
            &mut fixture.agent,
            registry.clone()
        )
        .is_err());
        registry
            .unregister_owned(&fixture.owner, "owned_cat")
            .unwrap();
        drop(registry);
        let process = Arc::try_unwrap(process)
            .unwrap_or_else(|_| {
                panic!(
            "original Process registry must not retain closed handle after explicit unregister")
            })
            .into_inner();
        assert!(
            process
                .wait(std::future::pending(), Duration::from_secs(3))
                .await
                .finished(),
            "an actual /cat Process exits after the sole host-owned stdin was closed"
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn aw_p6_10f_retired_original_actor_after_tool_intent_never_touches_real_process() {
        use crate::model_management::ProcessRegistry;
        use aw_tools::process::{PipeProcess, PipeSpec};
        use std::collections::BTreeMap;
        let tmp = PrivateTemp::new();
        let store = SessionStore::open(&tmp.0).unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(
                &actor,
                "Actor retirement between durable ToolStarted and OS effect",
                StoredSessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let paid_calls = Arc::new(AtomicUsize::new(0));
        let mut fixture = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            2,
            vec![process_close_grant()],
            Some(Arc::new(ModelProcessCloseProvider {
                calls: paid_calls.clone(),
                duplicate: false,
                terminate: false,
            })),
        );
        let process = Arc::new(tokio::sync::Mutex::new(
            PipeProcess::spawn(PipeSpec {
                program: native_cat_path(),
                root: std::env::temp_dir(),
                args: Vec::new(),
                env: BTreeMap::new(),
                stdin_open: true,
            })
            .await
            .unwrap(),
        ));
        let registry = ProcessRegistry::default();
        registry
            .register_owned(
                &fixture.owner,
                &fixture.agent,
                "retire_owned_cat",
                process.clone(),
            )
            .unwrap();
        crate::model_process_control::install_close_stdin(
            &fixture.owner,
            &mut fixture.agent,
            registry,
        )
        .unwrap();
        fixture
            .owner
            .submit(
                &fixture.agent,
                "retire_actor_before_effect",
                input(
                    &fixture.owner,
                    1,
                    1,
                    "close approved Process after actor retired",
                ),
            )
            .unwrap();
        let runtime = fixture._runtime.clone();
        let owner = fixture.owner.clone();
        let mut intent_witnessed = false;
        let result = fixture
            .owner
            .run_once(&mut fixture.agent, |event| {
                if let aw_agent_core::AgentEvent::ToolStarted { record, .. } = event {
                    if record.call.name == "process_close_stdin" {
                        // Host revokes the ORIGINAL Actor generation just
                        // AFTER immutable started receipt but BEFORE Tool IO.
                        runtime.lock().unwrap().record_state(
                            &owner.owner,
                            owner.actor,
                            Some(owner.epoch),
                            ActorState::Settled,
                        )?;
                        intent_witnessed = true;
                    }
                }
                Ok(())
            })
            .await;
        assert!(
            intent_witnessed,
            "the original durable Tool intent must have been written"
        );
        assert!(
            result.is_err()
                || result.as_ref().is_ok_and(|turn| {
                    turn.as_ref()
                        .is_some_and(|t| t.outcome != aw_agent_core::Outcome::Completed)
                }),
            "a retired Actor must not complete a paid Tool Turn as success"
        );
        assert!(
            process.lock().await.stdin_open(),
            "retiring Actor before Tool entry MUST preserve physical stdin"
        );
        let forbidden =
            aw_store::model_process_intent_id(&fixture.owner.session, &fixture.owner.branch, 1);
        assert!(
            fixture.owner.model_process_fact(&forbidden).is_err(),
            "Actor generation retirement before Tool entry must not mint an OS Process intent"
        );
        assert_eq!(
            paid_calls.load(Ordering::SeqCst),
            1,
            "paid model proposal not replayed"
        );
        assert!(
            fixture.owner.verify_rpc_state().is_err(),
            "retired Actor generation is no longer authority"
        );
        drop(fixture.agent);
        let mut original = Arc::try_unwrap(process)
            .unwrap_or_else(|_| panic!("original Actor fixture leaked Process after tool release"))
            .into_inner();
        original.close_stdin();
        assert!(original
            .wait(std::future::pending(), Duration::from_secs(3))
            .await
            .finished());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn aw_p6_9d2b2a_double_process_close_tool_proposal_is_unsettled_not_repeated() {
        use crate::model_management::ProcessRegistry;
        use aw_tools::process::{PipeProcess, PipeSpec};
        use std::collections::BTreeMap;
        let tmp = PrivateTemp::new();
        let store = SessionStore::open(&tmp.0).unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(
                &actor,
                "Process model conflicting effect",
                StoredSessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let mut f = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            2,
            vec![process_close_grant()],
            Some(Arc::new(ModelProcessCloseProvider {
                calls: Arc::new(AtomicUsize::new(0)),
                duplicate: true,
                terminate: false,
            })),
        );
        let process = Arc::new(tokio::sync::Mutex::new(
            PipeProcess::spawn(PipeSpec {
                program: native_cat_path(),
                root: std::env::temp_dir(),
                args: Vec::new(),
                env: BTreeMap::new(),
                stdin_open: true,
            })
            .await
            .unwrap(),
        ));
        let registry = ProcessRegistry::default();
        registry
            .register_owned(&f.owner, &f.agent, "owned_cat", process.clone())
            .unwrap();
        crate::model_process_control::install_close_stdin(&f.owner, &mut f.agent, registry.clone())
            .unwrap();
        f.owner
            .submit(
                &f.agent,
                "process_twice",
                input(&f.owner, 1, 1, "two conflicting Process effects"),
            )
            .unwrap();
        assert!(
            f.owner.run_once(&mut f.agent, |_| Ok(())).await.is_err(),
            "original Agent must not falsely settle two conflicting Write tools"
        );
        assert!(!process.lock().await.stdin_open());
        assert!(
            f.owner.admit_next_queued(&f.agent).is_err(),
            "unresolved paid Write effect cannot trigger another model Turn"
        );
        assert!(f
            .owner
            .model_process_fact(&{
                let mut d = Sha256::new();
                d.update(b"aw-model-process-one-io-intent-per-turn-v1\0");
                for v in [
                    f.owner.session.as_bytes(),
                    f.owner.branch.as_bytes(),
                    &1u64.to_be_bytes(),
                ] {
                    d.update((v.len() as u64).to_be_bytes());
                    d.update(v);
                }
                format!("mp_{:x}", d.finalize())
            })
            .is_ok());
        registry.unregister_owned(&f.owner, "owned_cat").unwrap();
    }
    #[test]
    fn aw_p6_9d2b2a_exact_process_close_grant_required_for_original_agent() {
        let mut ordinary = setup(None);
        let registry = crate::model_management::ProcessRegistry::default();
        assert!(crate::model_process_control::install_close_stdin(
            &ordinary.owner,
            &mut ordinary.agent,
            registry.clone()
        )
        .is_err());
        assert!(ordinary.agent.tool_specs().is_empty());
        for grant in [
            model_host_grants(&["process_close_stdin"])[0].clone(),
            aw_agent_kernel::envelope::ExactGrant {
                name: "process_close_stdin".into(),
                kind: aw_agent_kernel::capability::SourceKind::Host,
                source: "trusted-host".into(),
                source_epoch: 7,
                effect: aw_agent_kernel::capability::Effect::Write,
            },
            aw_agent_kernel::envelope::ExactGrant {
                name: "process_close_stdin".into(),
                kind: aw_agent_kernel::capability::SourceKind::Extension,
                source: "forged".into(),
                source_epoch: 1,
                effect: aw_agent_kernel::capability::Effect::Write,
            },
        ] {
            let store = SessionStore::in_memory().unwrap();
            let actor = PrincipalId::new("local").unwrap();
            let (session, branch) = store
                .create(
                    &actor,
                    "Process exact source rejection",
                    StoredSessionBinding {
                        working_directory: PathBuf::from("/tmp"),
                        tool_root: PathBuf::from("/tmp"),
                        system_prompt: "system".into(),
                        profile_hint: "fixture".into(),
                        model_hint: "model".into(),
                        destination_fingerprint: "0".repeat(64),
                    },
                )
                .unwrap();
            let mut f = setup_bound_with_tools(None, store, session, branch, 2, vec![grant], None);
            assert!(
                crate::model_process_control::install_close_stdin(
                    &f.owner,
                    &mut f.agent,
                    registry.clone()
                )
                .is_err(),
                "Read, foreign SourceKind or wrong epoch may not mint Process Write authority"
            );
            assert!(f.agent.tool_specs().is_empty());
        }
    }

    fn model_write_grant() -> aw_agent_kernel::envelope::ExactGrant {
        aw_agent_kernel::envelope::ExactGrant {
            name: "queue_enqueue".into(),
            kind: aw_agent_kernel::capability::SourceKind::Host,
            source: "trusted-host".into(),
            source_epoch: 1,
            effect: aw_agent_kernel::capability::Effect::Write,
        }
    }
    #[test]
    fn aw_p6_9d2a_model_enqueue_requires_exact_write_not_read_or_source_name() {
        let mut plain = setup(None);
        assert!(
            crate::model_effects::install_queue_enqueue(&plain.owner, &mut plain.agent).is_err()
        );
        assert!(
            plain.agent.tool_specs().is_empty(),
            "original default Actor Envelope can never gain model queue Write from its name"
        );
        let owner = PrincipalId::new("local").unwrap();
        for grant in [
            model_host_grants(&["queue_enqueue"])[0].clone(),
            aw_agent_kernel::envelope::ExactGrant {
                name: "queue_enqueue".into(),
                kind: aw_agent_kernel::capability::SourceKind::Host,
                source: "trusted-host".into(),
                source_epoch: 7,
                effect: aw_agent_kernel::capability::Effect::Write,
            },
            aw_agent_kernel::envelope::ExactGrant {
                name: "queue_enqueue".into(),
                kind: aw_agent_kernel::capability::SourceKind::Extension,
                source: "untrusted-proxy".into(),
                source_epoch: 1,
                effect: aw_agent_kernel::capability::Effect::Write,
            },
        ] {
            let store = SessionStore::in_memory().unwrap();
            let (session, branch) = store
                .create(
                    &owner,
                    "wrong model effect grant",
                    StoredSessionBinding {
                        working_directory: PathBuf::from("/tmp"),
                        tool_root: PathBuf::from("/tmp"),
                        system_prompt: "system".into(),
                        profile_hint: "fixture".into(),
                        model_hint: "model".into(),
                        destination_fingerprint: "0".repeat(64),
                    },
                )
                .unwrap();
            let mut f = setup_bound_with_tools(None, store, session, branch, 2, vec![grant], None);
            assert!(
                crate::model_effects::install_queue_enqueue(&f.owner, &mut f.agent).is_err(),
                "Read effect/foreign source/wrong source epoch cannot mint Host Write authority"
            );
            assert!(
                f.agent.tool_specs().is_empty(),
                "failed preflight must leave no ToolPolicy-granted model management Tool"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn aw_p6_9d2a_one_original_turn_enqueues_one_encrypted_future_without_auto_running() {
        let private = PrivateTemp::new();
        let store = SessionStore::open(&private.0).unwrap();
        let principal = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(
                &principal,
                "model queue exact custody",
                StoredSessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut f = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            2,
            vec![model_write_grant()],
            Some(Arc::new(ModelQueuedFutureProvider {
                model_calls: calls.clone(),
                duplicate_proposals: false,
            })),
        );
        assert!(
            f.agent.tool_specs().is_empty(),
            "Envelope does not install/grant model-write tool"
        );
        crate::model_effects::install_queue_enqueue(&f.owner, &mut f.agent).unwrap();
        assert_eq!(f.agent.tool_specs().len(), 1);
        assert_eq!(
            f.agent.tool_specs()[0].effect,
            aw_agent_core::ToolEffect::Write
        );
        assert!(crate::model_effects::install_queue_enqueue(&f.owner, &mut f.agent).is_err());
        f.owner
            .submit(
                &f.agent,
                "original_queued_user",
                input(&f.owner, 1, 1, "queue one future"),
            )
            .unwrap();
        let first = f
            .owner
            .run_once(&mut f.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.id, 1);
        assert_eq!(first.invocations.len(), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(first.tool_records.len(), 1);
        assert_eq!(first.tool_records[0].call.name, "queue_enqueue");
        assert_eq!(
            first.tool_records[0].effect,
            aw_agent_core::ToolEffect::Write
        );
        assert_eq!(
            first.tool_records[0].status,
            aw_agent_core::ToolStatus::Succeeded
        );
        let content = &first.tool_records[0].output.as_ref().unwrap().content;
        assert!(content.contains("NOT_DISPATCHED"));
        assert!(!content.contains("MODEL_FUTURE_PRIVATE_PAYLOAD"));
        assert!(!content.contains("sec_"));
        let entries = f.owner.queue_list().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].state, aw_store::QueueState::Pending);
        assert!(entries[0].request.request_id.starts_with("mq_"));
        assert!(
            entries[0].request.message_id > 1,
            "writer minted next sequence after original admitted Input"
        );
        let audit_projection = serde_json::to_string(&f.owner.queue_list().unwrap()).unwrap();
        assert!(!audit_projection.contains("MODEL_FUTURE_PRIVATE_PAYLOAD"));
        assert!(!audit_projection.contains("MALICIOUS_SECOND_SAME_TURN_BODY"));
        let original_blob = f.owner.queue_list().unwrap()[0].payload_ref.clone();
        let ciphertext = std::fs::read(
            private
                .0
                .join("secrets/items")
                .join(format!("{original_blob}.bin")),
        )
        .unwrap();
        assert!(
            !ciphertext
                .windows(b"MODEL_FUTURE_PRIVATE_PAYLOAD".len())
                .any(|w| w == b"MODEL_FUTURE_PRIVATE_PAYLOAD"),
            "queue-specific immutable metadata and physical ciphertext cannot contain plaintext"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "encrypted Pending is not a paid provider dispatch"
        );
        assert!(
            f.owner.admit_next_queued(&f.agent).unwrap().is_some(),
            "only EXPLICIT host owner claim can run a subsequent future Input"
        );
        let second = f
            .owner
            .run_once(&mut f.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(second.id, 2);
        assert_eq!(
            second.outcome,
            aw_agent_core::conversation::Outcome::Completed
        );
        assert!(
            second.tool_records.is_empty(),
            "future Turn need not enqueue again"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(
            f.owner
                .queue_fact(&entries[0].request.request_id)
                .unwrap()
                .state,
            aw_store::QueueState::Settled
        );
        assert_eq!(
            f.owner.admit_next_queued(&f.agent).unwrap(),
            None,
            "settled model-origin queued Input may never be automatically replayed"
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn aw_p6_9d2a_duplicate_model_tool_proposals_have_only_one_original_pending_fact() {
        let private = PrivateTemp::new();
        let store = SessionStore::open(&private.0).unwrap();
        let principal = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(
                &principal,
                "model duplicate denial",
                StoredSessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut f = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            2,
            vec![model_write_grant()],
            Some(Arc::new(ModelQueuedFutureProvider {
                model_calls: calls.clone(),
                duplicate_proposals: true,
            })),
        );
        crate::model_effects::install_queue_enqueue(&f.owner, &mut f.agent).unwrap();
        f.owner
            .submit(
                &f.agent,
                "original_1",
                input(&f.owner, 1, 1, "attempt duplicate write"),
            )
            .unwrap();
        let ambiguous = f.owner.run_once(&mut f.agent, |_| Ok(())).await;
        assert!(ambiguous.is_err(),
            "original journal correctly forbids settling a Turn with an unresolved conflicting Write hazard");
        let facts = f.owner.queue_list().unwrap();
        assert_eq!(
            facts.len(),
            1,
            "the stable originating Turn ID permanently collides"
        );
        assert_eq!(facts[0].state, aw_store::QueueState::Pending);
        assert!(facts[0].request.request_id.starts_with("mq_"));
        assert!(f.owner.admit_next_queued(&f.agent).is_err(),
            "original unresolved paid/Write hazard prevents automatic or explicit unsafe queue claim");
        let audit_projection = serde_json::to_string(&f.owner.queue_list().unwrap()).unwrap();
        assert!(!audit_projection.contains("MODEL_FUTURE_PRIVATE_PAYLOAD"));
        assert!(!audit_projection.contains("MALICIOUS_SECOND_SAME_TURN_BODY"));
        let original_blob = f.owner.queue_list().unwrap()[0].payload_ref.clone();
        let ciphertext = std::fs::read(
            private
                .0
                .join("secrets/items")
                .join(format!("{original_blob}.bin")),
        )
        .unwrap();
        assert!(
            !ciphertext
                .windows(b"MODEL_FUTURE_PRIVATE_PAYLOAD".len())
                .any(|w| w == b"MODEL_FUTURE_PRIVATE_PAYLOAD"),
            "queue-specific immutable metadata and physical ciphertext cannot contain plaintext"
        );
        assert!(
            (1..=2).contains(&calls.load(Ordering::SeqCst)),
            "conflicting model write cannot spawn an independent provider"
        );
    }
    fn model_host_grants(names: &[&str]) -> Vec<aw_agent_kernel::envelope::ExactGrant> {
        names
            .iter()
            .map(|name| aw_agent_kernel::envelope::ExactGrant {
                name: (*name).into(),
                kind: aw_agent_kernel::capability::SourceKind::Host,
                source: "trusted-host".into(),
                source_epoch: 1,
                effect: aw_agent_kernel::capability::Effect::Read,
            })
            .collect()
    }
    #[test]
    fn aw_p6_9d1_model_management_fails_closed_without_exact_envelope_or_owner_scope() {
        use crate::model_management::{self, Observer, ProcessRegistry};
        let mut f = setup(None);
        assert!(model_management::install(
            &f.owner,
            &mut f.agent,
            &[Observer::Agent, Observer::Queue].into_iter().collect(),
            None
        )
        .is_err());
        assert!(
            f.agent.tool_specs().is_empty(),
            "absence of Envelope permission installs NOTHING"
        );
        assert!(
            model_management::install(
                &f.owner,
                &mut f.agent,
                &[Observer::Process].into_iter().collect(),
                None
            )
            .is_err(),
            "a model Process status tool cannot appear without original Process registry"
        );
        let registry = ProcessRegistry::default();
        assert!(registry.unregister_owned(&f.owner, "forged_pid").is_err());
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
        // Preflight every name BEFORE any Agent ToolRegistry/ToolPolicy mutation.
        // Agent has one exact Host Read grant but Queue does not; installing
        // both must not leave a partially advertised `agent` tool behind.
        let store = SessionStore::in_memory().unwrap();
        let (session, branch) = store
            .create(
                &PrincipalId::new("local").unwrap(),
                "partial grant denial",
                StoredSessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let mut partially_granted = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            3,
            model_host_grants(&["agent"]),
            None,
        );
        assert!(model_management::install(
            &partially_granted.owner,
            &mut partially_granted.agent,
            &[Observer::Agent, Observer::Queue].into_iter().collect(),
            None
        )
        .is_err());
        assert!(
            partially_granted.agent.tool_specs().is_empty(),
            "all-or-nothing Host/Read Envelope preflight must leave no visible Tool"
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn aw_p6_9d1_model_management_uses_original_journal_and_read_only_tool_grants() {
        use crate::model_management::{self, Observer, ProcessRegistry};
        use aw_tools::process::PipeSpec;
        use std::collections::{BTreeMap, BTreeSet};
        let principal = PrincipalId::new("local").unwrap();
        let temp = PrivateTemp::new();
        let store = SessionStore::open(&temp.0).unwrap();
        let (session, branch) = store
            .create(
                &principal,
                "real model management",
                StoredSessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let counts = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(ModelObserverFixtureProvider {
            calls: counts.clone(),
            seen: seen.clone(),
        });
        let mut f = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            2,
            model_host_grants(&["agent", "queue", "process"]),
            Some(provider),
        );
        assert!(
            f.agent.tool_specs().is_empty(),
            "Envelope is a ceiling, not a model tool install"
        );
        let process_name = std::env::var_os("PATH")
            .into_iter()
            .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
            .map(|path| path.join("cat"))
            .find(|path| path.is_file())
            .expect("test environment needs a cat fixture in PATH");
        let process = Arc::new(tokio::sync::Mutex::new(
            aw_tools::process::PipeProcess::spawn(PipeSpec {
                program: process_name,
                root: std::env::temp_dir(),
                args: vec![],
                env: BTreeMap::new(),
                stdin_open: true,
            })
            .await
            .unwrap(),
        ));
        let registry = ProcessRegistry::default();
        registry
            .register_owned(&f.owner, &f.agent, "owned_proc", process.clone())
            .unwrap();
        assert!(registry
            .register_owned(&f.owner, &f.agent, "owned_proc", process.clone())
            .is_err());
        assert!(registry
            .register_owned(&f.owner, &f.agent, "../../proc", process.clone())
            .is_err());
        let requested = BTreeSet::from([Observer::Agent, Observer::Queue, Observer::Process]);
        model_management::install(&f.owner, &mut f.agent, &requested, Some(registry.clone()))
            .unwrap();
        let tools = f.agent.tool_specs();
        assert_eq!(tools.len(), 3);
        assert!(tools
            .iter()
            .all(|t| t.effect == aw_agent_core::ToolEffect::Read));
        assert!(tools
            .iter()
            .all(|t| matches!(t.name.as_str(), "agent" | "queue" | "process")));
        let accepted = f
            .owner
            .submit(
                &f.agent,
                "owned_user_1",
                input(&f.owner, 1, 1, "model inspect status"),
            )
            .unwrap();
        assert_eq!(accepted.state, InputAdmissionState::Accepted);
        f.owner
            .enqueue(
                &f.agent,
                "future_private",
                ActorMessage {
                    id: 2,
                    source_sequence: 2,
                    from: f.owner.actor,
                    to: f.owner.actor,
                    kind: Kind::Input,
                    turn: TurnTarget::Auto,
                    priority: 0,
                    payload: "PRIVATE_UNEXECUTED_QUEUE_BODY".into(),
                    content_type: aw_agent_kernel::message::ContentType::Text,
                },
            )
            .unwrap();
        let executed = f
            .owner
            .run_once(&mut f.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(executed.id, 1);
        assert_eq!(
            counts.load(Ordering::SeqCst),
            2,
            "one original Agent tool round and one original completion"
        );
        assert_eq!(executed.tool_records.len(), 3);
        assert!(executed
            .tool_records
            .iter()
            .all(|record| record.status == aw_agent_core::ToolStatus::Succeeded));
        assert_eq!(executed.invocations.len(), 2);
        assert_eq!(seen.lock().unwrap().as_slice(), ["3", "3"]);
        let agent_reply = executed
            .tool_records
            .iter()
            .find(|r| r.call.name == "agent")
            .unwrap()
            .output
            .as_ref()
            .unwrap()
            .content
            .clone();
        assert!(agent_reply.contains("active_turn"));
        assert!(agent_reply.contains(r#""turn_id":1"#));
        let queue_reply = executed
            .tool_records
            .iter()
            .find(|r| r.call.name == "queue")
            .unwrap()
            .output
            .as_ref()
            .unwrap()
            .content
            .clone();
        assert!(queue_reply.contains("future_private"));
        assert!(!queue_reply.contains("PRIVATE_UNEXECUTED_QUEUE_BODY"));
        assert!(!queue_reply.contains("sec_"));
        assert!(!queue_reply.contains("payload_ref"));
        let process_reply = executed
            .tool_records
            .iter()
            .find(|r| r.call.name == "process")
            .unwrap()
            .output
            .as_ref()
            .unwrap()
            .content
            .clone();
        assert!(process_reply.contains("captured_bytes_only"));
        for forbidden in ["pid", "head", "tail", "command", "program", "shell"] {
            assert!(
                !process_reply.contains(&format!(r#""{forbidden}""#)),
                "Process projection may never disclose {forbidden}"
            );
        }
        assert_eq!(
            f.owner.queue_fact("future_private").unwrap().state,
            aw_store::QueueState::Pending,
            "observation does NOT dispatch or delete a queued next Turn"
        );
        assert!(
            model_management::install(&f.owner, &mut f.agent, &requested, Some(registry.clone()))
                .is_err(),
            "tools cannot be registered twice"
        );
        registry.unregister_owned(&f.owner, "owned_proc").unwrap();
        let mut original = Arc::try_unwrap(process)
            .unwrap_or_else(|_| {
                panic!("Process registry must not retain a handle after explicit unregister")
            })
            .into_inner();
        original.close_stdin();
        assert!(original
            .wait(std::future::pending(), Duration::from_secs(3))
            .await
            .finished());
        let context = aw_agent_core::ToolContext {
            actor: f.owner.owner.clone(),
            turn_id: 1,
            execution_site: "local".into(),
            cancellation: aw_agent_core::CancellationToken::new(),
            identity: f.agent.default_identity().unwrap(),
            scope: f.agent.journal_scope().unwrap(),
        };
        assert!(
            f.owner.verify_model_observation(&context).is_err(),
            "already settled Turn cannot inspect via cached ToolContext"
        );
        f._runtime
            .lock()
            .unwrap()
            .record_state(
                &f.owner.owner,
                f.owner.actor,
                Some(f.owner.epoch),
                ActorState::Settled,
            )
            .unwrap();
        assert!(
            f.agent.tool_specs().is_empty(),
            "retired original Actor generation cannot advertise cached management Tools"
        );
    }

    #[tokio::test]
    async fn aw_p6_9d1_exact_original_running_turn_fences_forged_model_context() {
        let started = Arc::new(tokio::sync::Notify::new());
        let fixture = setup(Some(started.clone()));
        fixture
            .owner
            .submit(
                &fixture.agent,
                "auth_current",
                input(&fixture.owner, 1, 1, "current"),
            )
            .unwrap();
        let owner = fixture.owner.clone();
        let ctx = aw_agent_core::ToolContext {
            actor: owner.owner.clone(),
            turn_id: 1,
            execution_site: "local".into(),
            cancellation: aw_agent_core::CancellationToken::new(),
            identity: fixture.agent.default_identity().unwrap(),
            scope: fixture.agent.journal_scope().unwrap(),
        };
        assert!(
            owner.verify_model_observation(&ctx).is_err(),
            "Accepted but undispatched Input is not an active model Tool context"
        );
        let mut agent = fixture.agent;
        let active = owner.clone();
        let task = tokio::spawn(async move { active.run_once(&mut agent, |_| Ok(())).await });
        tokio::time::timeout(Duration::from_secs(3), started.notified())
            .await
            .unwrap();
        assert!(
            owner.verify_model_observation(&ctx).is_ok(),
            "original immutable Dispatching Input and actual active Runtime must match"
        );
        let mut forged = ctx.clone();
        forged.turn_id = 2;
        assert!(owner.verify_model_observation(&forged).is_err());
        forged = ctx.clone();
        forged.execution_site = "untrusted:other".into();
        assert!(owner.verify_model_observation(&forged).is_err());
        forged = ctx.clone();
        forged.scope = Some(aw_agent_core::InvocationScope {
            session_id: "s_foreign".into(),
            branch_id: owner.branch.clone(),
        });
        assert!(owner.verify_model_observation(&forged).is_err());
        forged = ctx.clone();
        forged.actor = PrincipalId::new("forged").unwrap();
        assert!(owner.verify_model_observation(&forged).is_err());
        forged = ctx.clone();
        forged.identity.actor = PrincipalId::new("forged").unwrap();
        assert!(owner.verify_model_observation(&forged).is_err());
        forged = ctx.clone();
        forged.cancellation.cancel();
        assert!(owner.verify_model_observation(&forged).is_err());
        task.abort();
        let _ = task.await;
        assert!(
            owner.verify_model_observation(&ctx).is_err(),
            "abandoned paid attempt cannot be observed after original Unknown"
        );
    }

    struct ModelFuturePauseProvider {
        calls: Arc<AtomicUsize>,
        duplicate: bool,
    }
    impl Provider for ModelFuturePauseProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async {
                Err(Error::Protocol(
                    "fixture requires original Tool-aware Agent",
                ))
            })
        }
        fn stream_tools<'a>(
            &'a self,
            messages: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a [aw_agent_core::ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> aw_agent_core::provider::ModelFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                match messages.last() {
                    Some(m) if m.role == Role::Tool => {
                        sink("original paid Turn completed after future-admission hold")?;
                        Ok(aw_agent_core::provider::ModelReply {
                            meta: CompletionMeta {
                                finish_reason: "stop".into(),
                                usage: Usage {
                                    input_tokens: Some(2),
                                    output_tokens: Some(3),
                                },
                            },
                            ..Default::default()
                        })
                    }
                    Some(m)
                        if m.role == Role::User && m.content == "PENDING_PRIVATE_AFTER_PAUSE" =>
                    {
                        sink("owner-resumed second journaled Turn completed")?;
                        Ok(aw_agent_core::provider::ModelReply {
                            meta: CompletionMeta {
                                finish_reason: "stop".into(),
                                usage: Usage {
                                    input_tokens: Some(3),
                                    output_tokens: Some(2),
                                },
                            },
                            ..Default::default()
                        })
                    }
                    Some(m)
                        if m.role == Role::User
                            && m.content == "request future pause without interrupting current" =>
                    {
                        let mut proposals = vec![aw_agent_core::ToolCall {
                            id: "pause_original".into(),
                            name: "agent_pause_after_turn".into(),
                            arguments: serde_json::json!({"op":"pause_after_turn"}),
                        }];
                        if self.duplicate {
                            proposals.push(aw_agent_core::ToolCall {
                                id: "pause_duplicate".into(),
                                name: "agent_pause_after_turn".into(),
                                arguments: serde_json::json!({"op":"pause_after_turn"}),
                            });
                        }
                        Ok(aw_agent_core::provider::ModelReply {
                            meta: CompletionMeta {
                                finish_reason: "tool_calls".into(),
                                ..Default::default()
                            },
                            tool_calls: proposals,
                            provider_context: None,
                        })
                    }
                    _ => Err(Error::Protocol(
                        "unexpected original provider state in pause fixture",
                    )),
                }
            })
        }
    }
    fn model_future_pause_grant() -> aw_agent_kernel::envelope::ExactGrant {
        aw_agent_kernel::envelope::ExactGrant {
            name: "agent_pause_after_turn".into(),
            kind: aw_agent_kernel::capability::SourceKind::Host,
            source: "trusted-host".into(),
            source_epoch: 1,
            effect: aw_agent_kernel::capability::Effect::Write,
        }
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn aw_p6_9d2b2b2_original_agent_pauses_only_future_and_owner_resumes_after_settlement() {
        let temp = PrivateTemp::new();
        let store = SessionStore::open(&temp.0).unwrap();
        let principal = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(
                &principal,
                "pause future original Agent",
                StoredSessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut f = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            2,
            vec![model_future_pause_grant()],
            Some(Arc::new(ModelFuturePauseProvider {
                calls: calls.clone(),
                duplicate: false,
            })),
        );
        assert!(
            f.agent.tool_specs().is_empty(),
            "original Envelope alone never enables model control"
        );
        crate::model_agent_control::install_pause_after_turn(&f.owner, &mut f.agent).unwrap();
        assert_eq!(f.agent.tool_specs().len(), 1);
        assert_eq!(
            f.agent.tool_specs()[0].effect,
            aw_agent_core::ToolEffect::Write
        );
        f.owner
            .submit(
                &f.agent,
                "owner_current_1",
                input(
                    &f.owner,
                    1,
                    1,
                    "request future pause without interrupting current",
                ),
            )
            .unwrap();
        f.owner
            .enqueue(
                &f.agent,
                "host_queued_future",
                ActorMessage {
                    id: 2,
                    source_sequence: 2,
                    from: f.owner.actor,
                    to: f.owner.actor,
                    kind: Kind::Input,
                    turn: TurnTarget::Auto,
                    priority: 0,
                    payload: "PENDING_PRIVATE_AFTER_PAUSE".into(),
                    content_type: aw_agent_kernel::message::ContentType::Text,
                },
            )
            .unwrap();
        let first = f
            .owner
            .run_once(&mut f.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.id, 1);
        assert_eq!(
            first.outcome,
            aw_agent_core::conversation::Outcome::Completed
        );
        assert_eq!(first.tool_records.len(), 1);
        assert_eq!(first.tool_records[0].call.name, "agent_pause_after_turn");
        assert_eq!(
            first.tool_records[0].status,
            aw_agent_core::ToolStatus::Succeeded
        );
        let result = &first.tool_records[0].output.as_ref().unwrap().content;
        assert!(result.contains("AFTER_CURRENT_ORIGINAL_TURN"));
        assert!(result.contains("EXPLICIT_AUTHENTICATED_OWNER_ONLY"));
        assert!(!result.contains("PENDING_PRIVATE_AFTER_PAUSE"));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "current original provider and Tool settlement must finish normally"
        );
        let held = f.owner.current_future_pause().unwrap().unwrap();
        assert_eq!(held.state, aw_store::ModelAgentPauseState::Armed);
        assert_eq!(held.request.turn_id, 1);
        let pending = f.owner.queue_fact("host_queued_future").unwrap();
        assert_eq!(pending.state, aw_store::QueueState::Pending);
        assert!(
            f.owner.admit_next_queued(&f.agent).is_err(),
            "future Agent hold must deny original queue claim without consuming it"
        );
        assert_eq!(f.owner.queue_fact("host_queued_future").unwrap(), pending);
        assert!(f
            .owner
            .submit(
                &f.agent,
                "unapproved_followup",
                input(&f.owner, 3, 2, "attempt unpaid future Turn while paused")
            )
            .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let released = f
            .owner
            .release_future_pause(
                &f.agent,
                &held.request.intent_id,
                "original authenticated owner reviewed settled source Turn",
            )
            .unwrap();
        assert_eq!(released.state, aw_store::ModelAgentPauseState::Released);
        assert!(f.owner.current_future_pause().unwrap().is_none());
        assert!(f
            .owner
            .release_future_pause(&f.agent, &held.request.intent_id, "duplicate release")
            .is_err());
        let claimed = f.owner.admit_next_queued(&f.agent).unwrap().unwrap();
        assert_eq!(claimed.request.request_id, "host_queued_future");
        let second = f
            .owner
            .run_once(&mut f.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(second.id, 2);
        assert_eq!(second.prompt, "PENDING_PRIVATE_AFTER_PAUSE");
        assert_eq!(
            second.outcome,
            aw_agent_core::conversation::Outcome::Completed
        );
        assert!(
            second.tool_records.is_empty(),
            "the model pause Tool never replays after owner release"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(
            f.owner.queue_fact("host_queued_future").unwrap().state,
            aw_store::QueueState::Settled
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn aw_p6_9d2b2b2_duplicate_model_pause_is_held_until_unknown_paid_tool_reconciled() {
        let temp = PrivateTemp::new();
        let store = SessionStore::open(&temp.0).unwrap();
        let principal = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(
                &principal,
                "conflicting model pause",
                StoredSessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let mut f = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            2,
            vec![model_future_pause_grant()],
            Some(Arc::new(ModelFuturePauseProvider {
                calls: Arc::new(AtomicUsize::new(0)),
                duplicate: true,
            })),
        );
        crate::model_agent_control::install_pause_after_turn(&f.owner, &mut f.agent).unwrap();
        f.owner
            .submit(
                &f.agent,
                "original_duplicate_pause",
                input(
                    &f.owner,
                    1,
                    1,
                    "request future pause without interrupting current",
                ),
            )
            .unwrap();
        assert!(
            f.owner.run_once(&mut f.agent, |_| Ok(())).await.is_err(),
            "original Tool/Turn with a conflicting Write proposal cannot falsely settle"
        );
        let paused = f.owner.current_future_pause().unwrap().unwrap();
        assert_eq!(paused.state, aw_store::ModelAgentPauseState::Armed);
        assert!(f
            .owner
            .release_future_pause(
                &f.agent,
                &paused.request.intent_id,
                "cannot ignore unknown paid Tool state"
            )
            .is_err());
        assert!(f.owner.admit_next_queued(&f.agent).is_err());
        assert_eq!(f.owner.current_future_pause().unwrap(), Some(paused));
    }
    #[test]
    fn aw_p6_9d2b2b2_future_pause_needs_its_own_exact_host_write_not_read_or_process_grant() {
        let mut normal = setup(None);
        assert!(crate::model_agent_control::install_pause_after_turn(
            &normal.owner,
            &mut normal.agent
        )
        .is_err());
        assert!(normal.agent.tool_specs().is_empty());
        for grant in [
            model_host_grants(&["agent_pause_after_turn"])[0].clone(),
            process_terminate_grant(),
            aw_agent_kernel::envelope::ExactGrant {
                name: "agent_pause_after_turn".into(),
                kind: aw_agent_kernel::capability::SourceKind::Host,
                source: "trusted-host".into(),
                source_epoch: 17,
                effect: aw_agent_kernel::capability::Effect::Write,
            },
            aw_agent_kernel::envelope::ExactGrant {
                name: "agent_pause_after_turn".into(),
                kind: aw_agent_kernel::capability::SourceKind::Extension,
                source: "external-model".into(),
                source_epoch: 1,
                effect: aw_agent_kernel::capability::Effect::Write,
            },
        ] {
            let store = SessionStore::in_memory().unwrap();
            let owner = PrincipalId::new("local").unwrap();
            let (session, branch) = store
                .create(
                    &owner,
                    "invalid pause source",
                    StoredSessionBinding {
                        working_directory: PathBuf::from("/tmp"),
                        tool_root: PathBuf::from("/tmp"),
                        system_prompt: "system".into(),
                        profile_hint: "fixture".into(),
                        model_hint: "model".into(),
                        destination_fingerprint: "0".repeat(64),
                    },
                )
                .unwrap();
            let mut f = setup_bound_with_tools(None, store, session, branch, 2, vec![grant], None);
            assert!(crate::model_agent_control::install_pause_after_turn(
                &f.owner,&mut f.agent).is_err(),
                "Agent observer Read, Process Write, foreign source or wrong epoch may not mint model pause Write");
            assert!(f.agent.tool_specs().is_empty());
        }
    }
    #[cfg(unix)]
    #[test]
    fn aw_p6_8b_host_secret_service_requires_live_generation_and_trusted_manifest() {
        use aw_ui_extension::settings::{
            SettingDefinition, SettingKind, SettingValue, SettingsRegistry,
        };
        use std::os::unix::fs::DirBuilderExt;
        let root = std::env::temp_dir().join(format!(
            "aw-p6-8b-host-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        let fixture = setup_with_store(None, SessionStore::open(&root).unwrap());
        let registry = SettingsRegistry::new([
            SettingDefinition {
                extension: "trusted.vault".into(),
                name: "credential".into(),
                kind: SettingKind::Secret { max_bytes: 64 },
                default: None,
                description: String::new(),
            },
            SettingDefinition {
                extension: "trusted.vault".into(),
                name: "display".into(),
                kind: SettingKind::Boolean,
                default: Some(SettingValue::Boolean(false)),
                description: String::new(),
            },
        ])
        .unwrap();
        assert!(crate::extension_secrets::set(
            &fixture.store,
            &fixture.owner,
            &registry,
            "trusted.vault",
            "display",
            None,
            b"not-a-secret"
        )
        .is_err());
        assert!(crate::extension_secrets::set(
            &fixture.store,
            &fixture.owner,
            &registry,
            "forged",
            "credential",
            None,
            b"unregistered"
        )
        .is_err());
        let placed = crate::extension_secrets::set(
            &fixture.store,
            &fixture.owner,
            &registry,
            "trusted.vault",
            "credential",
            None,
            b"bound_owner_private",
        )
        .unwrap();
        assert_eq!(placed.revision, 1);
        assert!(placed.configured);
        assert_eq!(
            crate::extension_secrets::read(
                &fixture.store,
                &fixture.owner,
                &registry,
                "trusted.vault",
                "credential"
            )
            .unwrap()
            .as_slice(),
            b"bound_owner_private"
        );
        assert_eq!(
            crate::extension_secrets::list(&fixture.store, &fixture.owner, &registry)
                .unwrap()
                .len(),
            1
        );
        assert!(!serde_json::to_string(&placed)
            .unwrap()
            .contains("bound_owner_private"));
        assert!(!serde_json::to_string(&placed).unwrap().contains("sec_"));
        fixture
            ._runtime
            .lock()
            .unwrap()
            .record_state(
                &fixture.owner.owner,
                fixture.owner.actor,
                Some(fixture.owner.epoch),
                ActorState::Settled,
            )
            .unwrap();
        assert!(
            crate::extension_secrets::read(
                &fixture.store,
                &fixture.owner,
                &registry,
                "trusted.vault",
                "credential"
            )
            .is_err(),
            "terminal Actor cannot retain secret access"
        );
        assert!(crate::extension_secrets::set(
            &fixture.store,
            &fixture.owner,
            &registry,
            "trusted.vault",
            "credential",
            Some(1),
            b"forbidden"
        )
        .is_err());
        drop(fixture);
        std::fs::remove_dir_all(root).unwrap();
    }
}
