//! AW-P6-9A: host-only, SQLite-writer-fenced admission of one real Actor Input.
//!
//! One owner, one original Agent, one original durable Session/TurnJournal.
//! The SQLite audit is immutable *admission evidence*, not a replayable queue,
//! provider receipt, new model loop, authority grant or an RPC endpoint.
//! Future multi-message queues and external RPC need a separately tested CAS.
use aw_agent_core::{
    control::{AgentSessionController, WriterEpochFence},
    runtime::{Delivery, HostRuntime},
    tool::ToolBindingAuthority,
    Agent, AgentEvent, AuthoritySegmentEvidence, Error, PrincipalId, ProviderAccounts,
    ProviderSelection, Result, Turn,
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
    ActorQueueView, AuthoritySteerFact, AuthoritySteerRequest, AuthoritySteerState, CustodySweep,
    InputAdmissionState, ModelAgentPauseFact, ModelAgentPauseRequest, ModelProcessAction,
    ModelProcessFact, ModelProcessRequest, ModelProcessState, ModelQueueDispositionKind,
    ModelQueueDispositionRequest, ParticipantInputAdmission, QueueTarget,
    SessionParticipationAction, SessionStore,
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
    Input { fact: Box<ActorInputFact> },
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
#[derive(Clone)]
pub struct ParticipantControlAuthority {
    provider_accounts: ProviderAccounts,
    tool_authority: ToolBindingAuthority,
    session_id: String,
    branch_id: String,
    agent_ref: String,
}

struct ParticipantAuthorityAdmission<'a> {
    provider_accounts: &'a ProviderAccounts,
    tool_authority: &'a ToolBindingAuthority,
    participant: &'a PrincipalId,
    selected: &'a ProviderSelection,
    participation: &'a aw_store::SessionParticipationEvidence,
    action: SessionParticipationAction,
    request_id: &'a str,
    branch: &'a aw_store::BranchInfo,
    turn_id: u64,
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

    /// Attach the existing durable Agent to one first-party Actor owner using
    /// only current SessionStore writer evidence and the Agent's exact current
    /// tool envelope. No caller supplies an Actor key, grant, site or epoch.
    pub fn attach_for_agent(
        store: SessionStore,
        session: &str,
        branch: &str,
        agent: &Agent,
    ) -> Result<Self> {
        use aw_agent_core::runtime::SessionBinding;
        use aw_agent_kernel::{
            actor::{ActorState, RuntimeKey},
            capability::Effect,
            envelope::{Access, Budget, EnvelopeSpec, Network},
        };

        let owner = agent.actor().clone();
        let binding = agent.execution_binding().ok_or(Error::Denied(
            "Actor owner attachment requires a Session-derived Agent identity",
        ))?;
        if binding.session_id() != session || binding.branch_id() != branch {
            return Err(Error::Denied(
                "Actor owner attachment Session differs from Agent identity",
            ));
        }
        let stored = store.session(&owner, session)?;
        let writer = store.workspace_writer(&owner, None, &stored.workspace_id)?;
        let epoch = NonZeroU64::new(writer.writer_epoch)
            .ok_or(Error::Denied("Agent writer epoch is unavailable"))?;
        if writer.state != "active" {
            return Err(Error::Denied("Agent Workspace writer is not active"));
        }
        store.actor_input_writer_fence(&owner, session, epoch.get())?;

        let grants = agent.requested_tool_envelope_grants()?;
        let access = if grants
            .iter()
            .any(|grant| matches!(grant.effect, Effect::Write | Effect::Unknown))
        {
            Access::ReadWrite
        } else {
            Access::ReadOnly
        };
        let process = grants
            .iter()
            .any(|grant| matches!(grant.effect, Effect::Process | Effect::Unknown));
        let site = agent.tool_execution_site().to_owned();

        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce)
            .map_err(|_| Error::Storage("Actor Runtime nonce unavailable"))?;
        let runtime_key = RuntimeKey {
            id: NonZeroU64::new(u64::from_le_bytes(
                nonce[..8]
                    .try_into()
                    .map_err(|_| Error::Storage("Actor Runtime nonce malformed"))?,
            ))
            .ok_or(Error::Storage("Actor Runtime ID was zero"))?,
            generation: NonZeroU64::new(u64::from_le_bytes(
                nonce[8..]
                    .try_into()
                    .map_err(|_| Error::Storage("Actor Runtime generation malformed"))?,
            ))
            .ok_or(Error::Storage("Actor Runtime generation was zero"))?,
        };
        let mut runtime = HostRuntime::new(owner.clone(), site.clone(), runtime_key, 64, 32)?;
        let actor = runtime.bind_agent(
            &owner,
            SessionBinding::from_trusted_host(session, Some(epoch))?,
            None,
            EnvelopeSpec {
                actor: owner.as_str().to_owned(),
                site,
                revision: 1,
                access,
                network: Network::Deny,
                process,
                tty: false,
                microphone: false,
                speaker: false,
                budget: Budget {
                    max_model_rounds: 32,
                    max_tool_calls: 128,
                    max_children: 64,
                    max_depth: 8,
                    max_output_bytes: agent.options().max_output_bytes.min(1024 * 1024),
                },
                grants,
            },
            "agent-session",
        )?;
        runtime.record_state(&owner, actor, Some(epoch), ActorState::Running)?;
        let service = Self::new(
            store,
            Arc::new(Mutex::new(runtime)),
            owner,
            actor,
            epoch,
            session,
            branch,
        )?;
        service.verify_rpc_agent(agent)?;
        Ok(service)
    }

    /// One accepted Input per current branch view. We never persist prompt
    /// text in audit; the original Agent's TurnJournal records user text ONLY
    /// after a guarded claim and factual TurnStart. A repeated request ID
    /// cannot re-enter even after process restart.
    fn participant_tool_envelope_ref(
        authority: &ToolBindingAuthority,
        actor: &PrincipalId,
    ) -> Result<String> {
        let bindings = authority.permitted_bindings(actor)?;
        let mut hash = Sha256::new();
        hash.update(b"aw-shared-session-tool-envelope-v1\0");
        hash.update((actor.as_str().len() as u64).to_be_bytes());
        hash.update(actor.as_str().as_bytes());
        for binding in bindings {
            for part in [
                binding.name.as_str(),
                binding.source.kind_label(),
                binding.source.id.as_str(),
                binding.execution_site.as_str(),
            ] {
                hash.update((part.len() as u64).to_be_bytes());
                hash.update(part.as_bytes());
            }
            hash.update(binding.registry_revision.to_be_bytes());
            hash.update(binding.authority_revision.to_be_bytes());
            hash.update(binding.source.epoch.to_be_bytes());
            hash.update([match binding.effect {
                aw_agent_core::ToolEffect::Read => 0,
                aw_agent_core::ToolEffect::Write => 1,
                aw_agent_core::ToolEffect::Process => 2,
                aw_agent_core::ToolEffect::Control => 3,
                aw_agent_core::ToolEffect::Unknown => 4,
            }]);
        }
        Ok(format!("tools:{:x}", hash.finalize()))
    }

    fn participant_authority_admission(
        &self,
        request: ParticipantAuthorityAdmission<'_>,
    ) -> Result<ParticipantInputAdmission> {
        let ParticipantAuthorityAdmission {
            provider_accounts,
            tool_authority,
            participant,
            selected,
            participation,
            action,
            request_id,
            branch,
            turn_id,
        } = request;
        selected.revalidate()?;
        if selected.identity().actor != *participant
            || participation.actor != *participant
            || participation.session_id != self.session
            || participation.owner
            || !participation.permissions.allows(action)
        {
            return Err(Error::Denied(
                "participant action requires exact current collaboration/provider authority",
            ));
        }
        let current = provider_accounts.select(
            participant,
            &selected.identity().account_id,
            &selected.identity().model,
        )?;
        if current.identity() != selected.identity() {
            return Err(Error::Denied(
                "participant provider/account/model authority changed before admission",
            ));
        }
        self.store.authorize_participant_model_context(
            participant,
            &self.session,
            &branch.id,
            selected.identity(),
            action,
        )?;

        let payer_ref = AuthoritySegmentEvidence::payer_ref(selected.identity())?;
        let provider_model_ceiling_ref =
            AuthoritySegmentEvidence::provider_model_ceiling_ref(selected.identity())?;
        let tool_envelope_ref = Self::participant_tool_envelope_ref(tool_authority, participant)?;
        let disclosure_projection_ref = format!(
            "full-session:{}:audience:{}",
            self.session, participation.audience_epoch
        );

        let identity_json = serde_json::to_vec(selected.identity())
            .map_err(|_| Error::Protocol("cannot fingerprint participant provider identity"))?;
        let mut context = Sha256::new();
        context.update(b"aw-shared-session-context-v1\0");
        for part in [
            self.session.as_bytes(),
            self.branch.as_bytes(),
            branch.head.as_bytes(),
            request_id.as_bytes(),
            participant.as_str().as_bytes(),
            payer_ref.as_bytes(),
            provider_model_ceiling_ref.as_bytes(),
            tool_envelope_ref.as_bytes(),
            disclosure_projection_ref.as_bytes(),
            identity_json.as_slice(),
        ] {
            context.update((part.len() as u64).to_be_bytes());
            context.update(part);
        }
        context.update(branch.revision.to_be_bytes());
        context.update(turn_id.to_be_bytes());
        context.update(participation.membership_epoch.to_be_bytes());
        context.update(participation.audience_epoch.to_be_bytes());
        let context_fingerprint: [u8; 32] = context.finalize().into();

        let mut causal = Sha256::new();
        causal.update(match action {
            SessionParticipationAction::Submit => {
                b"aw-shared-session-input-causal-root-v1\0".as_slice()
            }
            SessionParticipationAction::Control => {
                b"aw-shared-session-control-causal-root-v1\0".as_slice()
            }
            SessionParticipationAction::View
            | SessionParticipationAction::Contribute
            | SessionParticipationAction::Manage => {
                b"aw-shared-session-other-causal-root-v1\0".as_slice()
            }
        });
        for part in [
            self.session.as_bytes(),
            request_id.as_bytes(),
            participant.as_str().as_bytes(),
        ] {
            causal.update((part.len() as u64).to_be_bytes());
            causal.update(part);
        }
        let causal_root = format!(
            "{}:{:x}",
            if action == SessionParticipationAction::Submit {
                "input"
            } else {
                "control"
            },
            causal.finalize()
        );

        let admission = ParticipantInputAdmission {
            actor: participant.clone(),
            identity: selected.identity().clone(),
            membership_epoch: participation.membership_epoch,
            audience_epoch: participation.audience_epoch,
            causal_root,
            payer_ref,
            provider_model_ceiling_ref,
            tool_envelope_ref,
            disclosure_projection_ref,
            context_fingerprint,
        };
        admission.validate()?;
        Ok(admission)
    }

    /// Submit one shared participant Input into this same Agent/Session queue.
    /// The participant supplies no ActorKey, writer, tool grant or payer token:
    /// all authority is resolved from current host stores before admission.
    pub fn submit_participant_text(
        &self,
        agent: &Agent,
        participant: &PrincipalId,
        selected: &ProviderSelection,
        text: &str,
    ) -> Result<ActorInputFact> {
        if text.is_empty() || text.len() > crate::config::MAX_PROMPT_BYTES || text.contains('\0') {
            return Err(Error::Denied(
                "participant Agent Input text is invalid or too large",
            ));
        }
        self.verify_rpc_agent(agent)?;
        let participation = self.store.authorize_session_participation(
            participant,
            &self.session,
            SessionParticipationAction::Submit,
        )?;
        let turn = agent.history_state().next_turn_id;
        let branch = self
            .store
            .branches(&self.owner, &self.session)?
            .into_iter()
            .find(|row| row.id == self.branch)
            .ok_or(Error::Denied("Actor owner branch disappeared"))?;

        let mut request_hash = Sha256::new();
        request_hash.update(b"aw-shared-session-input-v1\0");
        for part in [
            self.session.as_bytes(),
            self.branch.as_bytes(),
            participant.as_str().as_bytes(),
            &turn.to_be_bytes(),
            text.as_bytes(),
        ] {
            request_hash.update((part.len() as u64).to_be_bytes());
            request_hash.update(part);
        }
        let request_id = format!("pi_{:x}", request_hash.finalize());
        if let Ok(existing) = self.fact(&request_id) {
            return Err(match existing.state {
                InputAdmissionState::Settled => {
                    Error::Conflict("participant Agent Input already settled; do not replay")
                }
                _ => Error::Conflict(
                    "participant Agent Input already exists or is uncertain; inspect before retry",
                ),
            });
        }
        let provider_accounts = agent.provider_accounts();
        let tool_authority = agent.tool_binding_authority();
        let participant_admission =
            self.participant_authority_admission(ParticipantAuthorityAdmission {
                provider_accounts: &provider_accounts,
                tool_authority: &tool_authority,
                participant,
                selected,
                participation: &participation,
                action: SessionParticipationAction::Submit,
                request_id: &request_id,
                branch: &branch,
                turn_id: turn,
            })?;
        self.submit_inner_with_participant(
            agent,
            request_id,
            ActorMessage {
                id: turn,
                source_sequence: turn,
                from: self.actor,
                to: self.actor,
                kind: Kind::Input,
                turn: TurnTarget::Exact(turn),
                priority: 0,
                payload: text.to_owned(),
                content_type: aw_agent_kernel::message::ContentType::Text,
            },
            false,
            Some(participant_admission),
        )
    }

    pub fn participant_control_authority(
        &self,
        agent: &Agent,
    ) -> Result<ParticipantControlAuthority> {
        self.verify_rpc_agent(agent)?;
        let scope = agent.journal_scope()?.ok_or(Error::Denied(
            "participant Control requires a journaled Agent",
        ))?;
        let execution = agent.execution_binding().ok_or(Error::Denied(
            "participant Control requires a bound Agent identity",
        ))?;
        if scope.session_id != self.session
            || scope.branch_id != self.branch
            || execution.session_id() != self.session
            || execution.branch_id() != self.branch
        {
            return Err(Error::Denied(
                "participant Control authority differs from owning Session/branch",
            ));
        }
        Ok(ParticipantControlAuthority {
            provider_accounts: agent.provider_accounts(),
            tool_authority: agent.tool_binding_authority(),
            session_id: self.session.clone(),
            branch_id: self.branch.clone(),
            agent_ref: execution.canonical_ref().to_owned(),
        })
    }

    pub fn prepare_participant_control(
        &self,
        agent: &Agent,
        participant: &PrincipalId,
        selected: &ProviderSelection,
        turn_id: u64,
        request_id: &str,
    ) -> Result<ParticipantInputAdmission> {
        let authority = self.participant_control_authority(agent)?;
        self.prepare_participant_control_live(
            &authority,
            participant,
            selected,
            turn_id,
            request_id,
        )
    }

    pub fn prepare_participant_control_live(
        &self,
        authority: &ParticipantControlAuthority,
        participant: &PrincipalId,
        selected: &ProviderSelection,
        turn_id: u64,
        request_id: &str,
    ) -> Result<ParticipantInputAdmission> {
        if authority.session_id != self.session
            || authority.branch_id != self.branch
            || authority.agent_ref.is_empty()
        {
            return Err(Error::Denied(
                "participant Control coordinator snapshot is stale or foreign",
            ));
        }
        if turn_id == 0
            || request_id.is_empty()
            || request_id.len() > 128
            || !request_id.is_ascii()
            || request_id.chars().any(char::is_control)
        {
            return Err(Error::Denied("participant Control identity is invalid"));
        }
        self.verify_rpc_state()?;
        let participation = self.store.authorize_session_participation(
            participant,
            &self.session,
            SessionParticipationAction::Control,
        )?;
        let branch = self
            .store
            .branches(&self.owner, &self.session)?
            .into_iter()
            .find(|row| row.id == self.branch)
            .ok_or(Error::Denied("Actor owner branch disappeared"))?;
        self.participant_authority_admission(ParticipantAuthorityAdmission {
            provider_accounts: &authority.provider_accounts,
            tool_authority: &authority.tool_authority,
            participant,
            selected,
            participation: &participation,
            action: SessionParticipationAction::Control,
            request_id,
            branch: &branch,
            turn_id,
        })
    }

    pub fn submit_participant_control(
        &self,
        participant: ParticipantInputAdmission,
        boundary: aw_agent_core::AuthorityBoundaryKind,
        turn_id: u64,
        request_id: &str,
        canonical_target: &str,
        text: &str,
    ) -> Result<AuthoritySteerFact> {
        if !matches!(
            boundary,
            aw_agent_core::AuthorityBoundaryKind::Steer
                | aw_agent_core::AuthorityBoundaryKind::Interrupt
        ) {
            return Err(Error::Denied(
                "participant Control must be steer or interrupt",
            ));
        }
        participant.validate()?;
        if participant.actor == self.owner
            || turn_id == 0
            || request_id.is_empty()
            || request_id.len() > 128
            || !request_id.is_ascii()
            || request_id.chars().any(char::is_control)
            || canonical_target.is_empty()
            || canonical_target.len() > 1024
            || !canonical_target.is_ascii()
            || canonical_target.chars().any(char::is_control)
            || text.is_empty()
            || text.len() > 64 * 1024
            || text.contains('\0')
        {
            return Err(Error::Denied("participant Control request is invalid"));
        }
        self.verify_rpc_state()?;
        let payload_sha256 = format!("{:x}", Sha256::digest(text.as_bytes()));
        if let Ok(existing) =
            self.store
                .authority_steer_fact(&self.owner, &self.session, request_id)
        {
            if existing.request.branch_id != self.branch
                || existing.request.turn_id != turn_id
                || existing.request.canonical_target != canonical_target
                || existing.request.participant.as_ref() != Some(&participant)
                || existing.payload_sha256 != payload_sha256
            {
                return Err(Error::Denied(
                    "participant Control replay identity reused with changed semantics",
                ));
            }
            return match existing.state {
                AuthoritySteerState::Accepted | AuthoritySteerState::Applied => Ok(existing),
                AuthoritySteerState::Prepared | AuthoritySteerState::Unknown => {
                    Err(Error::Conflict(
                        "participant Control outcome is unresolved; inspect, do not replay",
                    ))
                }
                AuthoritySteerState::Rejected | AuthoritySteerState::Inspected => Err(
                    Error::Conflict("participant Control was dispositioned and cannot be replayed"),
                ),
            };
        }

        let prepared = self.store.authority_steer_prepare(
            &self.owner,
            AuthoritySteerRequest {
                request_id: request_id.to_owned(),
                session_id: self.session.clone(),
                branch_id: self.branch.clone(),
                writer_epoch: self.epoch.get(),
                turn_id,
                canonical_target: canonical_target.to_owned(),
                participant: Some(participant.clone()),
            },
            text,
        )?;
        let evidence = participant.authority_segment_evidence()?;
        let staged = match boundary {
            aw_agent_core::AuthorityBoundaryKind::Steer => self
                .controller
                .admit_authority_steer_with_evidence(turn_id, request_id, text, evidence),
            aw_agent_core::AuthorityBoundaryKind::Interrupt => self
                .controller
                .admit_authority_interrupt_with_evidence(turn_id, request_id, text, evidence),
            aw_agent_core::AuthorityBoundaryKind::Input => unreachable!("checked above"),
        };
        let segment = match staged {
            Ok(segment) => segment,
            Err(error) => {
                let _ = self.store.authority_steer_reject_prepared(
                    &self.owner,
                    &prepared,
                    "live Runtime rejected participant Control before admission",
                );
                return Err(error);
            }
        };
        match self
            .store
            .authority_steer_accept(&self.owner, &prepared, &segment)
        {
            Ok(accepted) => Ok(accepted),
            Err(error) => {
                let withdrawn = self.controller.discard_authority_steer(
                    turn_id,
                    request_id,
                    segment.fingerprint,
                );
                if withdrawn.is_ok() {
                    let _ = self.store.authority_steer_reject_prepared(
                        &self.owner,
                        &prepared,
                        "durable participant Control accept failed; Runtime update withdrawn",
                    );
                } else {
                    if let Ok(mut state) = self.state.lock() {
                        state.poisoned = true;
                    }
                    let _ = self.store.authority_steer_mark_unknown(
                        &self.owner,
                        &prepared,
                        "durable participant Control accept failed and withdrawal was uncertain",
                    );
                }
                Err(error)
            }
        }
    }

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
        self.submit_inner_with_participant(agent, request_id, message, from_queue, None)
    }

    fn submit_inner_with_participant(
        &self,
        agent: &Agent,
        request_id: impl Into<String>,
        message: ActorMessage,
        from_queue: bool,
        participant: Option<ParticipantInputAdmission>,
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
            participant,
        };
        self.store
            .actor_input_prepare(&self.owner, admission.clone())?;
        let delivered = if let Some(participant) = &admission.participant {
            self.controller.send_with_authority(
                message,
                admission.turn_id,
                participant.authority_segment_evidence()?,
            )
        } else {
            self.controller.send(message, admission.turn_id)
        };
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
                fact: Box::new(self.fact(&request_id)?),
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
                fact: Box::new(self.store.actor_input_inspect(
                    &self.owner,
                    &self.session,
                    &request_id,
                    self.epoch.get(),
                    &note,
                )?),
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

    /// Stable identity-only gate for replay lookup of an already committed
    /// jobs_* control receipt. It deliberately does not require an active Turn:
    /// new effects call verify_model_observation afterwards, while exact replay
    /// can recover a factual receipt after settlement without re-execution.
    fn verify_jobs_context_identity(&self, ctx: &aw_agent_core::ToolContext) -> Result<()> {
        self.verify_rpc_state()?;
        let binding = ctx
            .execution_owner
            .as_ref()
            .ok_or(Error::Denied("jobs control lacks bound Agent identity"))?;
        if ctx.turn_id == 0
            || ctx.call_id.is_empty()
            || ctx.call_id.len() > 256
            || ctx.actor != self.owner
            || ctx.identity.actor != self.owner
            || ctx.execution_site != self.site
            || ctx.source != aw_agent_core::ToolSource::trusted_host()
            || ctx.effect != aw_agent_core::ToolEffect::Control
            || ctx.scope.as_ref().is_none_or(|scope| {
                scope.session_id != self.session || scope.branch_id != self.branch
            })
            || binding.session_id() != self.session
            || binding.branch_id() != self.branch
        {
            return Err(Error::Denied(
                "jobs control identity/source/site/Session differs from bound owner",
            ));
        }
        Ok(())
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

    /// Unified jobs.send same-Turn steer. The text is encrypted and an
    /// immutable Prepared fact commits before Runtime staging. Accepted commits
    /// only after the exact pending AuthoritySegment exists in the live owner.
    pub fn jobs_steer_text(
        &self,
        ctx: &aw_agent_core::ToolContext,
        canonical_target: &str,
        expected_revision: Option<u64>,
        text: &str,
    ) -> Result<AuthoritySteerFact> {
        self.jobs_authority_update_text(
            ctx,
            canonical_target,
            expected_revision,
            text,
            aw_agent_core::AuthorityBoundaryKind::Steer,
        )
    }

    /// Interrupt redirects at the earliest *factual* safe boundary. It never
    /// drops an in-flight paid/provider or external effect merely to appear
    /// immediate; already-started work must receipt before the new segment.
    pub fn jobs_interrupt_text(
        &self,
        ctx: &aw_agent_core::ToolContext,
        canonical_target: &str,
        expected_revision: Option<u64>,
        text: &str,
    ) -> Result<AuthoritySteerFact> {
        self.jobs_authority_update_text(
            ctx,
            canonical_target,
            expected_revision,
            text,
            aw_agent_core::AuthorityBoundaryKind::Interrupt,
        )
    }

    fn jobs_authority_update_text(
        &self,
        ctx: &aw_agent_core::ToolContext,
        canonical_target: &str,
        expected_revision: Option<u64>,
        text: &str,
        boundary: aw_agent_core::AuthorityBoundaryKind,
    ) -> Result<AuthoritySteerFact> {
        if !matches!(
            boundary,
            aw_agent_core::AuthorityBoundaryKind::Steer
                | aw_agent_core::AuthorityBoundaryKind::Interrupt
        ) {
            return Err(Error::Denied("invalid same-Turn authority update kind"));
        }
        self.verify_jobs_context_identity(ctx)?;
        if text.is_empty()
            || text.len() > 64 * 1024
            || text.contains('\0')
            || text
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(Error::Denied(
                "same-Turn jobs_send update must be bounded safe text",
            ));
        }
        if canonical_target.is_empty()
            || canonical_target.len() > 1024
            || !canonical_target.is_ascii()
            || canonical_target.chars().any(char::is_control)
        {
            return Err(Error::Denied("invalid same-Turn jobs_send target"));
        }

        let mut id_hash = Sha256::new();
        id_hash.update(match boundary {
            aw_agent_core::AuthorityBoundaryKind::Steer => {
                b"aw-jobs-send-agent-steer-v1\0".as_slice()
            }
            aw_agent_core::AuthorityBoundaryKind::Interrupt => {
                b"aw-jobs-send-agent-interrupt-v1\0".as_slice()
            }
            aw_agent_core::AuthorityBoundaryKind::Input => unreachable!("checked above"),
        });
        for part in [
            self.session.as_bytes(),
            self.branch.as_bytes(),
            ctx.actor.as_str().as_bytes(),
            &ctx.turn_id.to_be_bytes(),
            ctx.call_id.as_bytes(),
            canonical_target.as_bytes(),
        ] {
            id_hash.update((part.len() as u64).to_be_bytes());
            id_hash.update(part);
        }
        let request_id = format!(
            "{}_{:x}",
            if boundary == aw_agent_core::AuthorityBoundaryKind::Steer {
                "js"
            } else {
                "ji"
            },
            id_hash.finalize()
        );
        let payload_sha256 = format!("{:x}", Sha256::digest(text.as_bytes()));

        if let Ok(existing) =
            self.store
                .authority_steer_fact(&self.owner, &self.session, &request_id)
        {
            if existing.request.branch_id != self.branch
                || existing.request.turn_id != ctx.turn_id
                || existing.request.canonical_target != canonical_target
                || existing.payload_sha256 != payload_sha256
            {
                return Err(Error::Denied(
                    "jobs_send authority-update replay identity reused with changed semantics",
                ));
            }
            return match existing.state {
                AuthoritySteerState::Accepted | AuthoritySteerState::Applied => Ok(existing),
                AuthoritySteerState::Prepared | AuthoritySteerState::Unknown => {
                    Err(Error::Conflict(
                        "jobs_send authority-update outcome is unresolved; inspect, do not replay",
                    ))
                }
                AuthoritySteerState::Rejected | AuthoritySteerState::Inspected => {
                    Err(Error::Conflict(
                        "jobs_send authority update was dispositioned and cannot be replayed",
                    ))
                }
            };
        }

        self.verify_model_observation(ctx)?;
        self.require_model_tool_grant("jobs_send", &aw_agent_core::ToolEffect::Control)?;
        let current_revision =
            self.store
                .agent_public_revision(&self.owner, &self.session, &self.branch)?;
        if expected_revision.is_some_and(|revision| revision != current_revision) {
            return Err(Error::Conflict("stale Agent revision"));
        }
        let prepared = self.store.authority_steer_prepare(
            &self.owner,
            AuthoritySteerRequest {
                request_id: request_id.clone(),
                session_id: self.session.clone(),
                branch_id: self.branch.clone(),
                writer_epoch: self.epoch.get(),
                turn_id: ctx.turn_id,
                canonical_target: canonical_target.to_owned(),
                participant: None,
            },
            text,
        )?;

        let staged = match boundary {
            aw_agent_core::AuthorityBoundaryKind::Steer => {
                self.controller
                    .admit_authority_steer(ctx.turn_id, &request_id, text)
            }
            aw_agent_core::AuthorityBoundaryKind::Interrupt => self
                .controller
                .admit_authority_interrupt(ctx.turn_id, &request_id, text),
            aw_agent_core::AuthorityBoundaryKind::Input => unreachable!("checked above"),
        };
        let segment = match staged {
            Ok(segment) => segment,
            Err(error) => {
                let _ = self.store.authority_steer_reject_prepared(
                    &self.owner,
                    &prepared,
                    "live Runtime rejected authority update before admission",
                );
                return Err(error);
            }
        };

        let accepted = match self
            .store
            .authority_steer_accept(&self.owner, &prepared, &segment)
        {
            Ok(accepted) => accepted,
            Err(error) => {
                let withdrawn = self.controller.discard_authority_steer(
                    ctx.turn_id,
                    &request_id,
                    segment.fingerprint,
                );
                if withdrawn.is_ok() {
                    let _ = self.store.authority_steer_reject_prepared(
                        &self.owner,
                        &prepared,
                        "durable accept failed; exact Runtime authority update withdrawn",
                    );
                } else {
                    if let Ok(mut state) = self.state.lock() {
                        state.poisoned = true;
                    }
                    let _ = self.store.authority_steer_mark_unknown(
                        &self.owner,
                        &prepared,
                        "durable accept failed and Runtime authority-update withdrawal was uncertain",
                    );
                }
                return Err(error);
            }
        };
        // Post-commit revocation makes this Tool attempt uncertain, but the
        // accepted durable steer remains the sole custody fact and is never
        // automatically staged under another request identity.
        self.verify_model_observation(ctx)?;
        Ok(accepted)
    }

    /// Parent/child Agent continuation uses a separate gate from self-control:
    /// the caller must be the exact active direct-parent Session/branch/Turn
    /// recorded on this child relation. The target queue remains owned by this
    /// child Service and continuation capacity is reserved before custody.
    pub fn jobs_queue_text_from_parent(
        &self,
        ctx: &aw_agent_core::ToolContext,
        canonical_target: &str,
        expected_revision: Option<u64>,
        text: &str,
    ) -> Result<ActorQueueView> {
        self.verify_rpc_state()?;
        let scope = ctx.scope.as_ref().ok_or(Error::Denied(
            "cross-Agent jobs_send requires a durable caller scope",
        ))?;
        let caller = ctx.execution_owner.as_ref().ok_or(Error::Denied(
            "cross-Agent jobs_send lacks caller Agent identity",
        ))?;
        if ctx.cancellation.is_cancelled()
            || ctx.turn_id == 0
            || ctx.call_id.is_empty()
            || ctx.call_id.len() > 256
            || ctx.actor != self.owner
            || ctx.identity.actor != self.owner
            || ctx.source != aw_agent_core::ToolSource::trusted_host()
            || ctx.effect != aw_agent_core::ToolEffect::Control
            || caller.session_id() != scope.session_id
            || caller.branch_id() != scope.branch_id
        {
            return Err(Error::Denied(
                "cross-Agent jobs_send caller identity/source/scope is invalid",
            ));
        }
        let relation = self
            .store
            .subagent_for_child_session(&self.owner, &self.session)?
            .ok_or(Error::Denied(
                "cross-Agent jobs_send target is not a durable delegated Agent",
            ))?;
        if relation.state != "running"
            || relation.parent_session_id != scope.session_id
            || relation.parent_branch_id != scope.branch_id
            || self.store.active_turn_for_branch(
                &self.owner,
                &scope.session_id,
                &scope.branch_id,
            )? != Some(ctx.turn_id)
        {
            return Err(Error::Denied(
                "cross-Agent jobs_send caller is not the active direct parent",
            ));
        }
        if text.is_empty()
            || text.len() > 64 * 1024
            || text.contains('\0')
            || text
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(Error::Denied(
                "Agent jobs_send text must be bounded 1..65536 safe bytes",
            ));
        }
        if canonical_target.is_empty()
            || canonical_target.len() > 1024
            || !canonical_target.is_ascii()
            || canonical_target.chars().any(char::is_control)
        {
            return Err(Error::Denied("invalid cross-Agent jobs_send target"));
        }

        let mut id_hash = Sha256::new();
        id_hash.update(b"aw-jobs-send-related-agent-queue-v1\0");
        for part in [
            scope.session_id.as_bytes(),
            scope.branch_id.as_bytes(),
            &ctx.turn_id.to_be_bytes(),
            ctx.call_id.as_bytes(),
            self.session.as_bytes(),
            self.branch.as_bytes(),
            canonical_target.as_bytes(),
        ] {
            id_hash.update((part.len() as u64).to_be_bytes());
            id_hash.update(part);
        }
        let request_id = format!("jc_{:x}", id_hash.finalize());
        let payload_sha256 = format!("{:x}", Sha256::digest(text.as_bytes()));

        if let Ok(existing) = self
            .store
            .actor_queue_fact(&self.owner, &self.session, &request_id)
        {
            if existing.request.branch_id != self.branch
                || existing.payload_sha256 != payload_sha256
            {
                return Err(Error::Denied(
                    "cross-Agent jobs_send replay identity reused with changed semantics",
                ));
            }
            return Ok((&existing).into());
        }

        let current_revision =
            self.store
                .agent_public_revision(&self.owner, &self.session, &self.branch)?;
        if expected_revision.is_some_and(|revision| revision != current_revision) {
            return Err(Error::Conflict("stale Agent revision"));
        }
        self.store
            .reserve_delegation_continuation(&self.owner, &self.session, &request_id)?;
        let sender_hash = format!(
            "agent:{:x}",
            Sha256::digest(caller.canonical_ref().as_bytes())
        );
        let fact = match self.store.actor_queue_enqueue_minted(
            &self.owner,
            ActorQueueRequest {
                request_id: request_id.clone(),
                session_id: self.session.clone(),
                branch_id: self.branch.clone(),
                recipient_site: self.site.clone(),
                sender: sender_hash,
                message_id: 0,
                writer_epoch: self.epoch.get(),
                target: QueueTarget::Auto,
                priority: 0,
            },
            text,
        ) {
            Ok(fact) => fact,
            Err(error) => {
                let _ = self.store.cancel_delegation_continuation(
                    &self.owner,
                    &self.session,
                    &request_id,
                );
                return Err(error);
            }
        };
        if self
            .store
            .active_turn_for_branch(&self.owner, &scope.session_id, &scope.branch_id)?
            != Some(ctx.turn_id)
        {
            return Err(Error::Conflict(
                "cross-Agent jobs_send caller changed after factual queue custody; inspect receipt",
            ));
        }
        Ok((&fact).into())
    }

    pub fn admit_job_event(
        &self,
        delivery: &aw_tools::job_event::JobEventDelivery,
    ) -> Result<(ActorQueueView, bool)> {
        delivery
            .validate()
            .map_err(|_| Error::Denied("invalid Job wake delivery identity"))?;
        let mode = match delivery.mode {
            aw_tools::job_event::JobWakeMode::Auto | aw_tools::job_event::JobWakeMode::Queue => {
                aw_tools::job_event::JobWakeMode::Queue
            }
            aw_tools::job_event::JobWakeMode::Steer
            | aw_tools::job_event::JobWakeMode::Interrupt => {
                return Err(Error::Denied(
                    "result/settled wake currently admits only queue delivery",
                ))
            }
        };
        self.verify_rpc_state()?;
        if delivery.source_ref == delivery.recipient_ref {
            return Err(Error::Denied("Job event source cannot wake itself"));
        }

        let mut hash = Sha256::new();
        hash.update(b"aw-job-event-continuation-v1\0");
        for part in [
            delivery.source_ref.as_bytes(),
            &delivery.event_rev.to_be_bytes(),
            delivery.recipient_ref.as_bytes(),
            &delivery.subscription_generation.to_be_bytes(),
            mode.as_str().as_bytes(),
        ] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part);
        }
        // Delegation continuation storage already owns the bounded causal-work
        // reservation contract and requires this exact jc_<sha256> grammar.
        let request_id = format!("jc_{:x}", hash.finalize());
        let payload = format!(
            "job event source={} rev={}",
            delivery.source_ref, delivery.event_rev
        );
        let payload_sha256 = format!("{:x}", Sha256::digest(payload.as_bytes()));

        if let Ok(existing) = self
            .store
            .actor_queue_fact(&self.owner, &self.session, &request_id)
        {
            if existing.request.branch_id != self.branch
                || existing.payload_sha256 != payload_sha256
            {
                return Err(Error::Denied(
                    "Job event continuation identity reused with changed semantics",
                ));
            }
            return Ok(((&existing).into(), false));
        }

        self.store
            .reserve_delegation_continuation(&self.owner, &self.session, &request_id)?;
        let sender = format!(
            "job-event:{:x}",
            Sha256::digest(delivery.source_ref.as_bytes())
        );
        let fact = match self.store.actor_queue_enqueue_minted(
            &self.owner,
            ActorQueueRequest {
                request_id: request_id.clone(),
                session_id: self.session.clone(),
                branch_id: self.branch.clone(),
                recipient_site: self.site.clone(),
                sender,
                message_id: 0,
                writer_epoch: self.epoch.get(),
                target: QueueTarget::Auto,
                priority: 0,
            },
            &payload,
        ) {
            Ok(fact) => fact,
            Err(error) => {
                let _ = self.store.cancel_delegation_continuation(
                    &self.owner,
                    &self.session,
                    &request_id,
                );
                return Err(error);
            }
        };
        Ok(((&fact).into(), true))
    }

    /// Unified jobs.send Agent-root queue path. This reuses the same encrypted
    /// Actor queue and current writer/Runtime owner. The stable request identity
    /// is derived from the original ToolCall, so duplicate delivery returns the
    /// original durable custody fact without enqueuing or paying again.
    pub fn jobs_queue_text(
        &self,
        ctx: &aw_agent_core::ToolContext,
        canonical_target: &str,
        expected_revision: Option<u64>,
        text: &str,
    ) -> Result<ActorQueueView> {
        self.verify_jobs_context_identity(ctx)?;
        if text.is_empty()
            || text.len() > 64 * 1024
            || text.contains('\0')
            || text
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(Error::Denied(
                "Agent jobs_send text must be bounded 1..65536 safe bytes",
            ));
        }
        if canonical_target.is_empty()
            || canonical_target.len() > 1024
            || !canonical_target.is_ascii()
            || canonical_target.chars().any(char::is_control)
        {
            return Err(Error::Denied("invalid Agent jobs_send target"));
        }

        let mut id_hash = Sha256::new();
        id_hash.update(b"aw-jobs-send-agent-queue-v1\0");
        for part in [
            self.session.as_bytes(),
            self.branch.as_bytes(),
            ctx.actor.as_str().as_bytes(),
            &ctx.turn_id.to_be_bytes(),
            ctx.call_id.as_bytes(),
            canonical_target.as_bytes(),
        ] {
            id_hash.update((part.len() as u64).to_be_bytes());
            id_hash.update(part);
        }
        let request_id = format!("jq_{:x}", id_hash.finalize());

        let payload_sha256 = format!("{:x}", Sha256::digest(text.as_bytes()));
        if let Ok(existing) = self
            .store
            .actor_queue_fact(&self.owner, &self.session, &request_id)
        {
            if existing.request.branch_id != self.branch
                || existing.payload_sha256 != payload_sha256
            {
                return Err(Error::Denied(
                    "jobs_send replay identity reused with changed queue semantics",
                ));
            }
            return Ok((&existing).into());
        }

        self.verify_model_observation(ctx)?;
        self.require_model_tool_grant("jobs_send", &aw_agent_core::ToolEffect::Control)?;
        let current_revision =
            self.store
                .agent_public_revision(&self.owner, &self.session, &self.branch)?;
        if expected_revision.is_some_and(|revision| revision != current_revision) {
            return Err(Error::Conflict("stale Agent revision"));
        }

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
        // The queue commit is already factual at this point. If the owner is
        // revoked now, the Tool attempt is uncertain and must never enqueue a
        // second item under another identity.
        self.verify_model_observation(ctx)?;
        Ok((&fact).into())
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
    /// Same-user fast-path admission for one new original Agent Turn. It
    /// creates the exact Actor Input message from the already bound owner and
    /// current Agent next-Turn identity; no queue hop or caller-supplied Actor
    /// metadata is involved.
    pub fn submit_bound_text(&self, agent: &Agent, text: &str) -> Result<ActorInputFact> {
        if text.is_empty() || text.len() > crate::config::MAX_PROMPT_BYTES || text.contains('\0') {
            return Err(Error::Denied("Agent Input text is invalid or too large"));
        }
        self.verify_rpc_agent(agent)?;
        let turn = agent.history_state().next_turn_id;
        let mut hash = Sha256::new();
        hash.update(b"aw-agent-input-fast-path-v1\0");
        for part in [
            self.session.as_bytes(),
            self.branch.as_bytes(),
            &turn.to_be_bytes(),
            text.as_bytes(),
        ] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part);
        }
        let request_id = format!("ui_{:x}", hash.finalize());
        if let Ok(existing) = self.fact(&request_id) {
            return Err(match existing.state {
                InputAdmissionState::Settled => {
                    Error::Conflict("Agent Input already settled; do not replay")
                }
                _ => Error::Conflict(
                    "Agent Input already exists or is uncertain; inspect before retry",
                ),
            });
        }
        self.submit(
            agent,
            request_id,
            ActorMessage {
                id: turn,
                source_sequence: turn,
                from: self.actor,
                to: self.actor,
                kind: Kind::Input,
                turn: TurnTarget::Exact(turn),
                priority: 0,
                payload: text.to_owned(),
                content_type: aw_agent_kernel::message::ContentType::Text,
            },
        )
    }

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
            if fact.request.request_id.starts_with("jc_") {
                self.store.claim_delegation_continuation(
                    &self.owner,
                    &self.session,
                    &fact.request.request_id,
                    receipt.turn_id,
                )?;
            }
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

    /// Unified jobs.cancel for the exact currently active Turn. The request
    /// identity is stable per original ToolCall; duplicate delivery returns the
    /// immutable Control fact and never emits a second Runtime cancellation.
    pub fn jobs_cancel_turn(
        &self,
        ctx: &aw_agent_core::ToolContext,
        canonical_target: &str,
        target_turn: u64,
        expected_revision: Option<u64>,
    ) -> Result<ActorControlFact> {
        self.verify_jobs_context_identity(ctx)?;
        if target_turn != ctx.turn_id
            || canonical_target.is_empty()
            || canonical_target.len() > 1024
            || !canonical_target.is_ascii()
            || canonical_target.chars().any(char::is_control)
        {
            return Err(Error::Denied(
                "jobs_cancel requires this owner's exact active Turn",
            ));
        }

        let mut id_hash = Sha256::new();
        id_hash.update(b"aw-jobs-cancel-turn-v1\0");
        for part in [
            self.session.as_bytes(),
            self.branch.as_bytes(),
            ctx.actor.as_str().as_bytes(),
            &ctx.turn_id.to_be_bytes(),
            ctx.call_id.as_bytes(),
        ] {
            id_hash.update((part.len() as u64).to_be_bytes());
            id_hash.update(part);
        }
        let request_id = format!("jc_{:x}", id_hash.finalize());

        if let Ok(existing) = self.control_fact(&request_id) {
            if existing.admission.action != ActorControlAction::CancelTurn
                || existing.admission.turn_id != target_turn
                || existing.admission.session_id != self.session
                || existing.admission.branch_id != self.branch
            {
                return Err(Error::Denied(
                    "jobs_cancel replay identity reused with changed semantics",
                ));
            }
            return match existing.state {
                ActorControlState::Applied | ActorControlState::Rejected => Ok(existing),
                ActorControlState::Prepared | ActorControlState::Unknown => Err(Error::Conflict(
                    "jobs_cancel outcome is unresolved; inspect, do not replay",
                )),
                ActorControlState::Inspected => Err(Error::Conflict(
                    "jobs_cancel was inspected after uncertainty and cannot be replayed",
                )),
            };
        }

        self.verify_model_observation(ctx)?;
        self.require_model_tool_grant("jobs_cancel", &aw_agent_core::ToolEffect::Control)?;
        let current_revision =
            self.store
                .agent_public_revision(&self.owner, &self.session, &self.branch)?;
        if expected_revision.is_some_and(|revision| revision != current_revision) {
            return Err(Error::Conflict("stale Agent revision"));
        }
        let sender = label(self.actor);
        let message_id = self.store.actor_control_next_message_id(
            &self.owner,
            &self.session,
            self.epoch.get(),
            &sender,
        )?;
        self.control(request_id, ActorControlAction::CancelTurn, message_id)
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
        self.run_once_inner(agent, None, None, on_event).await
    }

    pub async fn run_once_selected_with_cancel<F>(
        &self,
        agent: &mut Agent,
        selected: &aw_agent_core::ProviderSelection,
        parent_cancel: aw_agent_core::CancellationToken,
        on_event: F,
    ) -> Result<Option<Turn>>
    where
        F: FnMut(&AgentEvent) -> Result<()> + Send,
    {
        self.run_once_inner(agent, Some(selected), Some(parent_cancel), on_event)
            .await
    }

    async fn run_once_inner<F>(
        &self,
        agent: &mut Agent,
        selected: Option<&aw_agent_core::ProviderSelection>,
        parent_cancel: Option<aw_agent_core::CancellationToken>,
        on_event: F,
    ) -> Result<Option<Turn>>
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
        let participant_selected = (|| -> Result<Option<aw_agent_core::ProviderSelection>> {
            if let Some(participant) = &admission.participant {
                if selected.is_some() || parent_cancel.is_some() {
                    return Err(Error::Denied(
                        "participant Input provider is fixed by its durable admission",
                    ));
                }
                let current_participation = self.store.authorize_session_participation(
                    &participant.actor,
                    &self.session,
                    SessionParticipationAction::Submit,
                )?;
                if current_participation.owner
                    || current_participation.membership_epoch != participant.membership_epoch
                    || current_participation.audience_epoch != participant.audience_epoch
                {
                    return Err(Error::Denied(
                        "participant Submit/audience authority changed before Turn claim",
                    ));
                }
                let current = agent.provider_accounts().select(
                    &participant.actor,
                    &participant.identity.account_id,
                    &participant.identity.model,
                )?;
                current.revalidate()?;
                if current.identity() != &participant.identity {
                    return Err(Error::Denied(
                        "participant provider/account/model authority changed before dispatch",
                    ));
                }
                Ok(Some(current))
            } else {
                Ok(None)
            }
        })();
        let participant_selected = match participant_selected {
            Ok(selection) => selection,
            Err(error) => {
                self.state
                    .lock()
                    .map_err(|_| Error::Storage("Actor owner state lock poisoned"))?
                    .busy = false;
                return Err(error);
            }
        };
        let effective_selected = participant_selected.as_ref().or(selected);

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
        let result = match (effective_selected, parent_cancel) {
            (Some(selected), Some(parent_cancel)) => {
                self.controller
                    .run_once_verified_selected_with_cancel(
                        agent,
                        selected,
                        parent_cancel,
                        &proof,
                        on_event,
                    )
                    .await
            }
            (Some(selected), None) => {
                self.controller
                    .run_once_verified_selected(agent, selected, &proof, on_event)
                    .await
            }
            (None, None) => {
                self.controller
                    .run_once_verified(agent, &proof, on_event)
                    .await
            }
            (None, Some(_)) => Err(Error::Config(
                "durable Agent cancellation requires an exact selected provider".into(),
            )),
        };
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
        if let Err(error) = self.store.authority_steer_finalize_turn(
            &self.owner,
            &self.session,
            &self.branch,
            self.epoch.get(),
            admission.turn_id,
        ) {
            let _ = self.store.actor_input_transition(
                &self.owner,
                &admission,
                InputAdmissionState::Dispatching,
                InputAdmissionState::Unknown,
            );
            return Err(error);
        }
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
        AccountDescriptor, AgentOptions, CompletionFuture, CompletionMeta, DeltaSink, ModelFuture,
        ModelReply, Provider, Role, Tool, ToolCall, ToolContext, ToolEffect, ToolFuture,
        ToolOutput, ToolSpec, Usage,
    };
    use aw_agent_kernel::{
        actor::{ActorState, RuntimeKey},
        envelope::{Access, Budget, EnvelopeSpec, Network},
    };
    use aw_store::{SecretStore, SessionBinding as StoredSessionBinding};
    use aw_store_api::SessionBackend;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };
    struct SharedParticipantProvider {
        calls: Arc<AtomicUsize>,
        saw_owner_history: Arc<std::sync::atomic::AtomicBool>,
    }
    impl Provider for SharedParticipantProvider {
        fn stream<'a>(
            &'a self,
            messages: &'a [aw_agent_core::Message],
            model: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                assert_eq!(model, "model");
                assert!(messages
                    .iter()
                    .any(|message| message.role == Role::User
                        && message.content == "bob shared turn"));
                let saw_owner = messages.iter().any(|message| {
                    message.role == Role::User && message.content == "alice public turn"
                }) && messages.iter().any(|message| {
                    message.role == Role::Assistant && message.content == "exact Agent reply"
                });
                self.saw_owner_history.store(saw_owner, Ordering::SeqCst);
                self.calls.fetch_add(1, Ordering::SeqCst);
                sink("bob shared answer")?;
                Ok(CompletionMeta {
                    finish_reason: "stop".into(),
                    usage: Usage {
                        input_tokens: Some(5),
                        output_tokens: Some(3),
                    },
                })
            })
        }
    }

    #[tokio::test]
    async fn revoked_participant_after_acceptance_cannot_claim_or_pay_and_receipt_remains_factual()
    {
        let store = SessionStore::in_memory().unwrap();
        let alice = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(
                &alice,
                "Revoked shared input",
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
        let session_id = session.id.clone();
        let mut f = setup_bound(None, store, session, branch, 2);
        let bob = PrincipalId::new("internal:fixture:revoked-bob").unwrap();
        f.store
            .set_session_participation(
                &alice,
                &session_id,
                &bob,
                aw_store::SessionParticipationPermissions {
                    view: true,
                    submit: true,
                    ..Default::default()
                },
            )
            .unwrap();

        let bob_calls = Arc::new(AtomicUsize::new(0));
        f.agent
            .register_provider_account(
                AccountDescriptor::personal(
                    "revoked-bob-account",
                    bob.clone(),
                    "local",
                    ["model".into()],
                ),
                Arc::new(EchoProvider {
                    calls: bob_calls.clone(),
                    wait: None,
                }),
            )
            .unwrap();
        let selected = f
            .agent
            .provider_accounts()
            .select(&bob, "revoked-bob-account", "model")
            .unwrap();

        let accepted = f
            .owner
            .submit_participant_text(&f.agent, &bob, &selected, "accepted then revoked")
            .unwrap();
        assert_eq!(accepted.state, InputAdmissionState::Accepted);
        let request_id = accepted.admission.request_id.clone();

        f.store
            .revoke_session_participation(&alice, &session_id, &bob)
            .unwrap();

        assert!(f.owner.run_once(&mut f.agent, |_| Ok(())).await.is_err());
        assert_eq!(
            bob_calls.load(Ordering::SeqCst),
            0,
            "revocation after inbox acceptance must fence before paid provider dispatch"
        );
        let factual = f.owner.fact(&request_id).unwrap();
        assert_eq!(
            factual.state,
            InputAdmissionState::Accepted,
            "revocation must not rewrite the original factual acceptance receipt"
        );
        {
            let state = f.owner.state.lock().unwrap();
            assert!(!state.busy);
            assert!(
                !state.poisoned,
                "expected current-authority denial before Runtime claim is not an uncertain effect"
            );
        }

        // Re-granting creates a new membership/audience epoch. The old accepted
        // receipt still cannot be claimed under that later authority.
        f.store
            .set_session_participation(
                &alice,
                &session_id,
                &bob,
                aw_store::SessionParticipationPermissions {
                    view: true,
                    submit: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(f.owner.run_once(&mut f.agent, |_| Ok(())).await.is_err());
        assert_eq!(bob_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            f.owner.fact(&request_id).unwrap().state,
            InputAdmissionState::Accepted
        );
    }

    struct OwnerOnlyTool;
    impl Tool for OwnerOnlyTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "owner_only".into(),
                description: "owner-only fixture".into(),
                parameters: serde_json::json!({
                    "type":"object",
                    "additionalProperties":false
                }),
                effect: ToolEffect::Read,
            }
        }

        fn validate(&self, arguments: &serde_json::Value) -> Result<()> {
            if arguments.as_object().is_some_and(serde_json::Map::is_empty) {
                Ok(())
            } else {
                Err(Error::Denied("owner-only fixture args"))
            }
        }

        fn execute<'a>(
            &'a self,
            _ctx: ToolContext,
            _arguments: &'a serde_json::Value,
        ) -> ToolFuture<'a> {
            Box::pin(async { Ok(ToolOutput::text("owner-only")) })
        }
    }

    struct ParticipantPhaseTool {
        calls: Arc<AtomicUsize>,
        actors: Arc<Mutex<Vec<PrincipalId>>>,
    }
    impl Tool for ParticipantPhaseTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "phase_tool".into(),
                description: "shared-control phase fixture".into(),
                parameters: serde_json::json!({
                    "type":"object",
                    "additionalProperties":false
                }),
                effect: ToolEffect::Read,
            }
        }
        fn validate(&self, arguments: &serde_json::Value) -> Result<()> {
            if arguments.as_object().is_some_and(serde_json::Map::is_empty) {
                Ok(())
            } else {
                Err(Error::Denied("phase fixture requires empty object"))
            }
        }
        fn execute<'a>(
            &'a self,
            ctx: ToolContext,
            _arguments: &'a serde_json::Value,
        ) -> ToolFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.actors.lock().unwrap().push(ctx.actor);
            Box::pin(async { Ok(ToolOutput::text("phase tool factual receipt")) })
        }
    }

    struct AliceControlProvider {
        calls: Arc<AtomicUsize>,
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }
    impl Provider for AliceControlProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("Alice control fixture requires tool path")) })
        }
        fn stream_tools<'a>(
            &'a self,
            history: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a [ToolSpec],
            _: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                let call = self.calls.fetch_add(1, Ordering::SeqCst);
                if call != 0
                    || history
                        .iter()
                        .rev()
                        .find(|message| message.role == Role::User)
                        .is_none_or(|message| message.content != "alice start")
                {
                    return Err(Error::Protocol("unexpected Alice shared-control round"));
                }
                self.entered.notify_one();
                self.release.notified().await;
                Ok(ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "tool_calls".into(),
                        usage: Usage {
                            input_tokens: Some(2),
                            output_tokens: Some(1),
                        },
                    },
                    tool_calls: vec![ToolCall {
                        id: "alice-superseded-tool".into(),
                        name: "phase_tool".into(),
                        arguments: serde_json::json!({}),
                    }],
                    ..Default::default()
                })
            })
        }
    }

    struct BobControlProvider {
        calls: Arc<AtomicUsize>,
        second_entered: Arc<tokio::sync::Notify>,
        second_release: Arc<tokio::sync::Notify>,
    }
    impl Provider for BobControlProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("Bob control fixture requires tool path")) })
        }
        fn stream_tools<'a>(
            &'a self,
            history: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a [ToolSpec],
            _: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                let call = self.calls.fetch_add(1, Ordering::SeqCst);
                let latest_user = history
                    .iter()
                    .rev()
                    .find(|message| message.role == Role::User)
                    .ok_or(Error::Protocol("Bob control fixture user message missing"))?;
                if latest_user.content != "bob steer" {
                    return Err(Error::Protocol("Bob control segment text changed"));
                }
                match call {
                    0 => Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            usage: Usage {
                                input_tokens: Some(3),
                                output_tokens: Some(1),
                            },
                        },
                        tool_calls: vec![ToolCall {
                            id: "bob-factual-tool".into(),
                            name: "phase_tool".into(),
                            arguments: serde_json::json!({}),
                        }],
                        ..Default::default()
                    }),
                    1 => {
                        if !history.iter().any(|message| {
                            message.role == Role::Tool
                                && message.tool_call_id.as_deref() == Some("bob-factual-tool")
                        }) {
                            return Err(Error::Protocol(
                                "Bob second Step must causally follow his factual tool receipt",
                            ));
                        }
                        self.second_entered.notify_one();
                        self.second_release.notified().await;
                        Ok(ModelReply {
                            meta: CompletionMeta {
                                finish_reason: "tool_calls".into(),
                                usage: Usage {
                                    input_tokens: Some(4),
                                    output_tokens: Some(1),
                                },
                            },
                            tool_calls: vec![ToolCall {
                                id: "bob-superseded-tool".into(),
                                name: "phase_tool".into(),
                                arguments: serde_json::json!({}),
                            }],
                            ..Default::default()
                        })
                    }
                    _ => Err(Error::Protocol("unexpected extra Bob shared-control round")),
                }
            })
        }
    }

    struct CarolControlProvider {
        calls: Arc<AtomicUsize>,
    }
    impl Provider for CarolControlProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("Carol control fixture requires tool path")) })
        }
        fn stream_tools<'a>(
            &'a self,
            history: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                let call = self.calls.fetch_add(1, Ordering::SeqCst);
                if call != 0
                    || history
                        .iter()
                        .rev()
                        .find(|message| message.role == Role::User)
                        .is_none_or(|message| message.content != "carol steer")
                {
                    return Err(Error::Protocol("unexpected Carol shared-control round"));
                }
                sink("carol final")?;
                Ok(ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "stop".into(),
                        usage: Usage {
                            input_tokens: Some(3),
                            output_tokens: Some(2),
                        },
                    },
                    ..Default::default()
                })
            })
        }
    }

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
        let execution = store
            .agent_binding(&principal, &session.id, &branch.id)
            .unwrap();
        agent
            .bind_execution_owner(
                aw_agent_core::AgentExecutionBinding::from_trusted_host(
                    format!(
                        "/d:{}/i:{}/p:{}/a:{}",
                        execution.domain_handle,
                        execution.identity_handle,
                        execution.project_handle,
                        execution.agent_handle,
                    ),
                    execution.session_id,
                    execution.branch_id,
                )
                .unwrap(),
            )
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

    #[tokio::test]
    async fn participant_control_rejects_after_private_source_release_is_revoked_before_provider() {
        let store = SessionStore::in_memory().unwrap();
        let alice = PrincipalId::new("local").unwrap();
        let bob = PrincipalId::new("internal:fixture:disclosure-bob").unwrap();

        let (source_session, source_branch) = store
            .create(
                &alice,
                "Private source",
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
        let mut source = setup_bound(
            None,
            store.clone(),
            source_session.clone(),
            source_branch,
            21,
        );
        source
            .owner
            .submit(
                &source.agent,
                "private-source-input",
                input(&source.owner, 1, 1, "private source fact"),
            )
            .unwrap();
        source
            .owner
            .run_once(&mut source.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        let source_branch = store
            .branches(&alice, &source_session.id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();

        let (target_session, target_branch) = store
            .create(
                &alice,
                "Shared target",
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

        let plan = store
            .context_prepare(
                &alice,
                aw_store::PrepareContext {
                    target: aw_store::TargetHead::from(&target_branch),
                    sources: vec![aw_store::SourceSelection {
                        session_id: source_session.id.clone(),
                        branch: source_branch.id.clone(),
                        head: None,
                        selection: aw_store::Selection::Standalone,
                    }],
                    kind: aw_store::PlanKind::Import,
                    focus: "vetted private source".into(),
                    max_input_bytes: 512 * 1024,
                    max_output_bytes: 32 * 1024,
                    timeout_seconds: 10,
                },
            )
            .unwrap();
        let artifact = store
            .context_manual(
                &alice,
                &plan.id,
                "private-source-artifact",
                "authorized source observation",
                None,
            )
            .unwrap();
        let applied = store
            .context_apply(
                &alice,
                aw_store::ApplyContext {
                    artifact_id: artifact.id,
                    target: aw_store::TargetHead::from(&target_branch),
                    mode: aw_store::ApplyMode::Append,
                    request_id: "private-source-apply".into(),
                    confirmed_disclosure: true,
                },
            )
            .unwrap();

        store
            .context_grant_read(
                &alice,
                &source_session.id,
                &bob,
                true,
                vec!["bob-disclosure-account".into()],
            )
            .unwrap();
        store
            .set_session_participation(
                &alice,
                &target_session.id,
                &bob,
                aw_store::SessionParticipationPermissions {
                    view: true,
                    control: true,
                    submit: false,
                    ..Default::default()
                },
            )
            .unwrap();

        let target = setup_bound(
            None,
            store.clone(),
            target_session.clone(),
            applied.branch,
            22,
        );
        let bob_calls = Arc::new(AtomicUsize::new(0));
        target
            .agent
            .register_provider_account(
                AccountDescriptor::personal(
                    "bob-disclosure-account",
                    bob.clone(),
                    "local",
                    ["model".into()],
                ),
                Arc::new(EchoProvider {
                    calls: bob_calls.clone(),
                    wait: None,
                }),
            )
            .unwrap();
        let selected = target
            .agent
            .provider_accounts()
            .select(&bob, "bob-disclosure-account", "model")
            .unwrap();

        store
            .context_grant_read(&alice, &source_session.id, &bob, false, vec![])
            .unwrap();

        assert!(
            target
                .owner
                .prepare_participant_control(
                    &target.agent,
                    &bob,
                    &selected,
                    1,
                    "unsafe-bob-control",
                )
                .is_err(),
            "cross-user Control must stop before provider dispatch when required source context is no longer disclosable"
        );
        assert_eq!(
            bob_calls.load(Ordering::SeqCst),
            0,
            "unsafe disclosure must reject before any Bob provider call"
        );
        assert!(store
            .authority_steer_fact(&alice, &target_session.id, "unsafe-bob-control")
            .is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn participant_control_segments_persist_across_steps_and_change_actor_on_later_steer() {
        let temp = PrivateTemp::new();
        let store = SessionStore::open(&temp.0).unwrap();
        let alice = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(
                &alice,
                "Shared Control Agent",
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
        let session_id = session.id.clone();

        let alice_calls = Arc::new(AtomicUsize::new(0));
        let alice_entered = Arc::new(tokio::sync::Notify::new());
        let alice_release = Arc::new(tokio::sync::Notify::new());
        let bob_calls = Arc::new(AtomicUsize::new(0));
        let bob_second_entered = Arc::new(tokio::sync::Notify::new());
        let bob_second_release = Arc::new(tokio::sync::Notify::new());
        let carol_calls = Arc::new(AtomicUsize::new(0));

        let mut f = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            12,
            vec![aw_agent_kernel::envelope::ExactGrant {
                name: "phase_tool".into(),
                kind: aw_agent_kernel::capability::SourceKind::Host,
                source: "trusted-host".into(),
                source_epoch: 1,
                effect: aw_agent_kernel::capability::Effect::Read,
            }],
            Some(Arc::new(AliceControlProvider {
                calls: alice_calls.clone(),
                entered: alice_entered.clone(),
                release: alice_release.clone(),
            })),
        );

        let bob = PrincipalId::new("internal:fixture:control-bob").unwrap();
        let carol = PrincipalId::new("internal:fixture:control-carol").unwrap();
        for participant in [&bob, &carol] {
            let granted = f
                .store
                .set_session_participation(
                    &alice,
                    &session_id,
                    participant,
                    aw_store::SessionParticipationPermissions {
                        view: true,
                        control: true,
                        submit: false,
                        ..Default::default()
                    },
                )
                .unwrap();
            assert!(granted.permissions.control);
            assert!(
                !granted.permissions.submit,
                "Control must remain independent from Submit"
            );
        }

        let phase_calls = Arc::new(AtomicUsize::new(0));
        let phase_actors = Arc::new(Mutex::new(Vec::new()));
        f.agent
            .register_host_tool(Arc::new(ParticipantPhaseTool {
                calls: phase_calls.clone(),
                actors: phase_actors.clone(),
            }))
            .unwrap();
        f.agent
            .permit_registered_host_tool(bob.clone(), "phase_tool")
            .unwrap();
        f.agent
            .permit_registered_host_tool(carol.clone(), "phase_tool")
            .unwrap();

        f.agent
            .register_provider_account(
                AccountDescriptor::personal(
                    "control-bob-account",
                    bob.clone(),
                    "local",
                    ["model".into()],
                ),
                Arc::new(BobControlProvider {
                    calls: bob_calls.clone(),
                    second_entered: bob_second_entered.clone(),
                    second_release: bob_second_release.clone(),
                }),
            )
            .unwrap();
        f.agent
            .register_provider_account(
                AccountDescriptor::personal(
                    "control-carol-account",
                    carol.clone(),
                    "local",
                    ["model".into()],
                ),
                Arc::new(CarolControlProvider {
                    calls: carol_calls.clone(),
                }),
            )
            .unwrap();

        let accounts = f.agent.provider_accounts();
        let bob_selected = accounts
            .select(&bob, "control-bob-account", "model")
            .unwrap();
        let carol_selected = accounts
            .select(&carol, "control-carol-account", "model")
            .unwrap();
        let live_control = f.owner.participant_control_authority(&f.agent).unwrap();
        let canonical_turn = format!(
            "{}/t:1",
            f.agent
                .execution_binding()
                .expect("shared Control fixture Agent binding")
                .canonical_ref()
        );

        f.owner
            .submit(
                &f.agent,
                "shared-control-owner-input",
                input(&f.owner, 1, 1, "alice start"),
            )
            .unwrap();

        let store = f.store.clone();
        let owner = f.owner.clone();
        let running_owner = owner.clone();
        let mut agent = f.agent;
        let task =
            tokio::spawn(async move { running_owner.run_once(&mut agent, |_| Ok(())).await });

        tokio::time::timeout(Duration::from_secs(3), alice_entered.notified())
            .await
            .unwrap();
        let bob_evidence = owner
            .prepare_participant_control_live(
                &live_control,
                &bob,
                &bob_selected,
                1,
                "bob-control-1",
            )
            .unwrap();
        let bob_accepted = owner
            .submit_participant_control(
                bob_evidence,
                aw_agent_core::AuthorityBoundaryKind::Steer,
                1,
                "bob-control-1",
                &canonical_turn,
                "bob steer",
            )
            .unwrap();
        assert_eq!(bob_accepted.state, AuthoritySteerState::Accepted);
        assert_eq!(bob_accepted.segment_ordinal, Some(2));
        assert_eq!(
            bob_accepted
                .request
                .participant
                .as_ref()
                .expect("Bob Control participant evidence")
                .actor,
            bob
        );
        alice_release.notify_waiters();

        tokio::time::timeout(Duration::from_secs(3), bob_second_entered.notified())
            .await
            .unwrap();
        assert_eq!(
            phase_calls.load(Ordering::SeqCst),
            1,
            "Bob's first tool effect must factually settle before Carol redirects"
        );
        assert_eq!(
            phase_actors.lock().unwrap().as_slice(),
            std::slice::from_ref(&bob)
        );

        let carol_evidence = owner
            .prepare_participant_control_live(
                &live_control,
                &carol,
                &carol_selected,
                1,
                "carol-control-1",
            )
            .unwrap();
        let carol_accepted = owner
            .submit_participant_control(
                carol_evidence,
                aw_agent_core::AuthorityBoundaryKind::Steer,
                1,
                "carol-control-1",
                &canonical_turn,
                "carol steer",
            )
            .unwrap();
        assert_eq!(carol_accepted.state, AuthoritySteerState::Accepted);
        assert_eq!(carol_accepted.segment_ordinal, Some(3));
        assert_eq!(
            carol_accepted
                .request
                .participant
                .as_ref()
                .expect("Carol Control participant evidence")
                .actor,
            carol
        );
        bob_second_release.notify_waiters();

        let turn = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(turn.outcome, aw_agent_core::Outcome::Completed);
        assert_eq!(turn.answer, "carol final");
        assert_eq!(alice_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            bob_calls.load(Ordering::SeqCst),
            2,
            "Bob AuthoritySegment must persist across tool receipt into the next model Step"
        );
        assert_eq!(carol_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            phase_calls.load(Ordering::SeqCst),
            1,
            "Alice and Bob's superseded unstarted proposals must never execute"
        );

        let actors = turn
            .invocations
            .iter()
            .map(|invocation| invocation.identity.actor.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            actors,
            vec![alice.clone(), bob.clone(), bob.clone(), carol.clone()]
        );
        assert!(turn.tool_records.iter().any(|record| {
            record.call.id == "bob-factual-tool"
                && record.status == aw_agent_core::ToolStatus::Succeeded
        }));
        assert!(turn.tool_records.iter().any(|record| {
            record.call.id == "alice-superseded-tool"
                && record.status == aw_agent_core::ToolStatus::Denied
        }));
        assert!(turn.tool_records.iter().any(|record| {
            record.call.id == "bob-superseded-tool"
                && record.status == aw_agent_core::ToolStatus::Denied
        }));

        let bob_applied = store
            .authority_steer_fact(&alice, &session_id, "bob-control-1")
            .unwrap();
        let carol_applied = store
            .authority_steer_fact(&alice, &session_id, "carol-control-1")
            .unwrap();
        assert_eq!(bob_applied.state, AuthoritySteerState::Applied);
        assert_eq!(carol_applied.state, AuthoritySteerState::Applied);
        assert_eq!(bob_applied.segment_ordinal, Some(2));
        assert_eq!(carol_applied.segment_ordinal, Some(3));
    }

    #[tokio::test]
    async fn shared_session_submit_keeps_one_agent_branch_and_uses_participant_provider_authority()
    {
        let store = SessionStore::in_memory().unwrap();
        let alice = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(
                &alice,
                "Shared Agent",
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
        let session_id = session.id.clone();
        let branch_id = branch.id.clone();
        let mut f = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            2,
            vec![aw_agent_kernel::envelope::ExactGrant {
                name: "owner_only".into(),
                kind: aw_agent_kernel::capability::SourceKind::Host,
                source: "trusted-host".into(),
                source_epoch: 1,
                effect: aw_agent_kernel::capability::Effect::Read,
            }],
            None,
        );
        let canonical_before = f
            .agent
            .execution_binding()
            .unwrap()
            .canonical_ref()
            .to_owned();

        f.owner
            .submit_bound_text(&f.agent, "alice public turn")
            .unwrap();
        let alice_turn = f
            .owner
            .run_once(&mut f.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(alice_turn.id, 1);
        assert_eq!(alice_turn.answer, "exact Agent reply");
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);

        f.agent.register_host_tool(Arc::new(OwnerOnlyTool)).unwrap();
        assert!(f.agent.permitted_tool_binding(&alice, "owner_only").is_ok());

        let bob = PrincipalId::new("internal:fixture:shared-bob").unwrap();
        let grant = f
            .store
            .set_session_participation(
                &alice,
                &session_id,
                &bob,
                aw_store::SessionParticipationPermissions {
                    view: true,
                    submit: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(!grant.owner);
        assert!(grant.permissions.view && grant.permissions.submit);
        assert!(!grant.permissions.control);

        let shared = f.store.session(&bob, &session_id).unwrap();
        assert_eq!(
            shared.owner, alice,
            "sharing must not change the Session resource owner"
        );
        let bob_binding = f
            .store
            .agent_binding(&bob, &session_id, &branch_id)
            .unwrap();
        assert_eq!(
            format!(
                "/d:{}/i:{}/p:{}/a:{}",
                bob_binding.domain_handle,
                bob_binding.identity_handle,
                bob_binding.project_handle,
                bob_binding.agent_handle
            ),
            canonical_before,
            "participant sees the same resource-owned Agent identity"
        );

        let bob_calls = Arc::new(AtomicUsize::new(0));
        let saw_owner_history = Arc::new(std::sync::atomic::AtomicBool::new(false));
        f.agent
            .register_provider_account(
                AccountDescriptor::personal("bob-account", bob.clone(), "local", ["model".into()]),
                Arc::new(SharedParticipantProvider {
                    calls: bob_calls.clone(),
                    saw_owner_history: saw_owner_history.clone(),
                }),
            )
            .unwrap();
        let selected = f
            .agent
            .provider_accounts()
            .select(&bob, "bob-account", "model")
            .unwrap();
        assert!(f.agent.permitted_tool_binding(&bob, "owner_only").is_err());

        let accepted = f
            .owner
            .submit_participant_text(&f.agent, &bob, &selected, "bob shared turn")
            .unwrap();
        assert_eq!(accepted.state, InputAdmissionState::Accepted);
        let participant = accepted
            .admission
            .participant
            .as_ref()
            .expect("shared Input must persist participant authority");
        assert_eq!(participant.actor, bob);
        assert_eq!(participant.identity, *selected.identity());
        assert_eq!(participant.membership_epoch, grant.membership_epoch);
        assert_eq!(participant.audience_epoch, grant.audience_epoch);
        assert!(participant
            .disclosure_projection_ref
            .starts_with("full-session:"));

        let bob_turn = f
            .owner
            .run_once(&mut f.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(bob_turn.id, 2);
        assert_eq!(bob_turn.prompt, "bob shared turn");
        assert_eq!(
            bob_turn.outcome,
            crate::conversation::Outcome::Completed,
            "participant Turn error={:?} owner_calls={} bob_calls={}",
            bob_turn.error,
            f.calls.load(Ordering::SeqCst),
            bob_calls.load(Ordering::SeqCst),
        );
        assert_eq!(
            bob_turn.answer,
            "bob shared answer",
            "participant Turn error={:?} owner_calls={} bob_calls={}",
            bob_turn.error,
            f.calls.load(Ordering::SeqCst),
            bob_calls.load(Ordering::SeqCst),
        );
        assert_eq!(
            f.calls.load(Ordering::SeqCst),
            1,
            "Bob's Turn must not fall back to Alice's provider/payer"
        );
        assert_eq!(bob_calls.load(Ordering::SeqCst), 1);
        assert!(saw_owner_history.load(Ordering::SeqCst));

        assert_eq!(
            f.agent.execution_binding().unwrap().canonical_ref(),
            canonical_before,
            "participant Submit must not rebind the Agent"
        );
        let branches = f.store.branches(&bob, &session_id).unwrap();
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].id, branch_id);
        assert!(branches[0].active_run.is_none());
        let history = f
            .store
            .open(&bob, &session_id, &branch_id)
            .unwrap()
            .view()
            .unwrap()
            .history;
        assert_eq!(history.turns.len(), 2);
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
        let result = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::pin!(running);
            tokio::select! {
                r=&mut running=>Some(r),
                _=notify.notified()=>None,
            }
        })
        .await
        .expect("fixture provider must enter the paid dispatch");
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
            participant: None,
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
        use aw_tools_adapter::process::{PipeProcess, PipeSpec};
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
        use aw_tools_adapter::process::{PipeProcess, PipeSpec};
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
        use aw_tools_adapter::process::{PipeProcess, PipeSpec};
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
        use aw_tools_adapter::process::{PipeProcess, PipeSpec};
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
        use aw_tools_adapter::process::{PipeProcess, PipeSpec};
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
        use aw_tools_adapter::process::{PipeProcess, PipeSpec};
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

    fn steer_request_control_grant() -> aw_agent_kernel::envelope::ExactGrant {
        aw_agent_kernel::envelope::ExactGrant {
            name: "steer_request".into(),
            kind: aw_agent_kernel::capability::SourceKind::Host,
            source: "trusted-host".into(),
            source_epoch: 1,
            effect: aw_agent_kernel::capability::Effect::Control,
        }
    }

    fn interrupt_request_control_grant() -> aw_agent_kernel::envelope::ExactGrant {
        aw_agent_kernel::envelope::ExactGrant {
            name: "interrupt_request".into(),
            kind: aw_agent_kernel::capability::SourceKind::Host,
            source: "trusted-host".into(),
            source_epoch: 1,
            effect: aw_agent_kernel::capability::Effect::Control,
        }
    }

    fn jobs_cancel_control_grant() -> aw_agent_kernel::envelope::ExactGrant {
        aw_agent_kernel::envelope::ExactGrant {
            name: "jobs_cancel".into(),
            kind: aw_agent_kernel::capability::SourceKind::Host,
            source: "trusted-host".into(),
            source_epoch: 1,
            effect: aw_agent_kernel::capability::Effect::Control,
        }
    }

    fn jobs_send_control_grant() -> aw_agent_kernel::envelope::ExactGrant {
        aw_agent_kernel::envelope::ExactGrant {
            name: "jobs_send".into(),
            kind: aw_agent_kernel::capability::SourceKind::Host,
            source: "trusted-host".into(),
            source_epoch: 1,
            effect: aw_agent_kernel::capability::Effect::Control,
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
        use aw_tools_adapter::process::PipeSpec;
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
            aw_tools_adapter::process::PipeProcess::spawn(PipeSpec {
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
            call_id: "host-direct".into(),
            source: aw_agent_core::ToolSource::trusted_host(),
            effect: aw_agent_core::ToolEffect::Unknown,
            execution_site: "local".into(),
            cancellation: aw_agent_core::CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: f.agent.default_identity().unwrap(),
            scope: f.agent.journal_scope().unwrap(),
            execution_owner: None,
            execution_scope: None,
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

    struct DurableSteerProvider {
        calls: Arc<AtomicUsize>,
        request_tool: &'static str,
    }
    impl Provider for DurableSteerProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("durable steer fixture requires tool path")) })
        }
        fn stream_tools<'a>(
            &'a self,
            history: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                let call = self.calls.fetch_add(1, Ordering::SeqCst);
                if call == 0 {
                    return Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            usage: Usage {
                                input_tokens: Some(3),
                                output_tokens: Some(2),
                            },
                        },
                        tool_calls: vec![ToolCall {
                            id: "authority-update-call-1".into(),
                            name: self.request_tool.into(),
                            arguments: serde_json::json!({"text":"replace"}),
                        }],
                        ..Default::default()
                    });
                }
                let steering = history
                    .iter()
                    .rev()
                    .find(|message| message.role == Role::User)
                    .ok_or(Error::Protocol("same-Turn steer user message missing"))?;
                if steering.content != "replace" {
                    return Err(Error::Protocol("same-Turn steer text changed"));
                }
                sink(&steering.content)?;
                Ok(ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "stop".into(),
                        usage: Usage {
                            input_tokens: Some(4),
                            output_tokens: Some(1),
                        },
                    },
                    ..Default::default()
                })
            })
        }
    }

    struct AuthorityUpdateRequestTool {
        owner: Service,
        accepted: Arc<Mutex<Option<AuthoritySteerFact>>>,
        boundary: aw_agent_core::AuthorityBoundaryKind,
    }
    impl AuthorityUpdateRequestTool {
        fn name(&self) -> &'static str {
            match self.boundary {
                aw_agent_core::AuthorityBoundaryKind::Steer => "steer_request",
                aw_agent_core::AuthorityBoundaryKind::Interrupt => "interrupt_request",
                aw_agent_core::AuthorityBoundaryKind::Input => "invalid_input_request",
            }
        }
    }
    impl Tool for AuthorityUpdateRequestTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: self.name().into(),
                description: "test-only same-Turn authority update request".into(),
                parameters: serde_json::json!({
                    "type":"object",
                    "properties":{"text":{"type":"string"}},
                    "required":["text"],
                    "additionalProperties":false
                }),
                effect: ToolEffect::Control,
            }
        }
        fn validate(&self, arguments: &serde_json::Value) -> Result<()> {
            if self.boundary == aw_agent_core::AuthorityBoundaryKind::Input {
                return Err(Error::Denied("Input is not a same-Turn update"));
            }
            let object = arguments
                .as_object()
                .ok_or(Error::Denied("authority-update fixture needs object"))?;
            if object.len() != 1
                || object
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .is_none_or(|text| text != "replace")
            {
                return Err(Error::Denied("invalid authority-update fixture text"));
            }
            Ok(())
        }
        fn execute<'a>(
            &'a self,
            ctx: ToolContext,
            arguments: &'a serde_json::Value,
        ) -> ToolFuture<'a> {
            Box::pin(async move {
                let text = arguments
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .ok_or(Error::Denied("authority-update fixture text missing"))?;
                let owner = ctx.execution_owner.as_ref().ok_or(Error::Denied(
                    "authority-update fixture Agent owner missing",
                ))?;
                let target = format!("{}/t:{}", owner.canonical_ref(), ctx.turn_id);
                let fact = match self.boundary {
                    aw_agent_core::AuthorityBoundaryKind::Steer => {
                        self.owner.jobs_steer_text(&ctx, &target, None, text)?
                    }
                    aw_agent_core::AuthorityBoundaryKind::Interrupt => {
                        self.owner.jobs_interrupt_text(&ctx, &target, None, text)?
                    }
                    aw_agent_core::AuthorityBoundaryKind::Input => {
                        return Err(Error::Denied("Input is not a same-Turn update"))
                    }
                };
                *self.accepted.lock().map_err(|_| {
                    Error::Storage("authority-update fixture receipt lock poisoned")
                })? = Some(fact.clone());
                Ok(ToolOutput::text(format!(
                    "accepted {}={} segment={}",
                    self.name(),
                    fact.request.request_id,
                    fact.segment_ordinal.unwrap_or(0)
                )))
            })
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn jobs_send_agent_queue_is_replay_safe_and_revision_fenced() {
        let temp = PrivateTemp::new();
        let started = Arc::new(tokio::sync::Notify::new());
        let principal = PrincipalId::new("local").unwrap();
        let store = SessionStore::open(&temp.0).unwrap();
        let (session, branch) = store
            .create(
                &principal,
                "jobs send queue fixture",
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
        let fixture = setup_bound_with_tools(
            Some(started.clone()),
            store,
            session,
            branch,
            7,
            vec![jobs_send_control_grant()],
            None,
        );
        let owner = fixture.owner.clone();
        let store = fixture.store.clone();
        let canonical = fixture
            .agent
            .execution_binding()
            .unwrap()
            .canonical_ref()
            .to_owned();
        owner
            .submit(
                &fixture.agent,
                "jobs_send_source",
                input(&owner, 1, 1, "run while queueing continuation"),
            )
            .unwrap();

        let mut context = aw_agent_core::ToolContext {
            actor: owner.owner.clone(),
            turn_id: 1,
            call_id: "jobs-send-1".into(),
            source: aw_agent_core::ToolSource::trusted_host(),
            effect: aw_agent_core::ToolEffect::Control,
            execution_site: "local".into(),
            cancellation: aw_agent_core::CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: fixture.agent.default_identity().unwrap(),
            scope: fixture.agent.journal_scope().unwrap(),
            execution_owner: fixture.agent.execution_binding().cloned(),
            execution_scope: None,
        };

        let mut agent = fixture.agent;
        let running = owner.clone();
        let task = tokio::spawn(async move { running.run_once(&mut agent, |_| Ok(())).await });
        tokio::time::timeout(Duration::from_secs(3), started.notified())
            .await
            .unwrap();
        let before = store
            .agent_public_revision(&owner.owner, &owner.session, &owner.branch)
            .unwrap();

        let first = owner
            .jobs_queue_text(&context, &canonical, Some(before), "continue once")
            .unwrap();
        assert!(first.request_id.starts_with("jq_"));
        assert_eq!(first.state, aw_store::QueueState::Pending);
        let duplicate = owner
            .jobs_queue_text(&context, &canonical, Some(before), "continue once")
            .unwrap();
        assert_eq!(duplicate.request_id, first.request_id);
        assert_eq!(owner.queue_list().unwrap().len(), 1);

        assert!(owner
            .jobs_queue_text(&context, &canonical, Some(before), "changed replay body")
            .is_err());
        context.call_id = "jobs-send-2".into();
        assert!(matches!(
            owner.jobs_queue_text(&context, &canonical, Some(before), "second request"),
            Err(Error::Conflict("stale Agent revision"))
        ));
        assert_eq!(owner.queue_list().unwrap().len(), 1);
        let public = serde_json::to_string(&owner.queue_list().unwrap()).unwrap();
        assert!(!public.contains("continue once"));

        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn jobs_cancel_turn_is_revision_fenced_and_replay_recovers_after_settlement() {
        let entered = Arc::new(tokio::sync::Notify::new());
        let principal = PrincipalId::new("local").unwrap();
        let store = SessionStore::in_memory().unwrap();
        let (session, branch) = store
            .create(
                &principal,
                "jobs cancel fixture",
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
        let fixture = setup_bound_with_tools(
            Some(entered.clone()),
            store,
            session,
            branch,
            9,
            vec![jobs_cancel_control_grant()],
            None,
        );
        fixture
            .owner
            .submit(
                &fixture.agent,
                "jobs_cancel_source",
                input(&fixture.owner, 1, 1, "cancel this exact turn"),
            )
            .unwrap();

        let owner = fixture.owner.clone();
        let mut agent = fixture.agent;
        let identity = agent.default_identity().unwrap();
        let scope = agent.journal_scope().unwrap();
        let execution_owner = agent.execution_binding().cloned();
        let canonical = format!("{}/t:1", execution_owner.as_ref().unwrap().canonical_ref());
        let context = aw_agent_core::ToolContext {
            actor: owner.owner.clone(),
            turn_id: 1,
            call_id: "jobs-cancel-call-1".into(),
            source: aw_agent_core::ToolSource::trusted_host(),
            effect: aw_agent_core::ToolEffect::Control,
            execution_site: "local".into(),
            cancellation: aw_agent_core::CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity,
            scope,
            execution_owner,
            execution_scope: None,
        };

        let running = owner.clone();
        let task = tokio::spawn(async move { running.run_once(&mut agent, |_| Ok(())).await });
        tokio::time::timeout(Duration::from_secs(3), entered.notified())
            .await
            .unwrap();
        let revision = owner
            .store
            .agent_public_revision(&owner.owner, &owner.session, &owner.branch)
            .unwrap();

        let cancelled = owner
            .jobs_cancel_turn(&context, &canonical, 1, Some(revision))
            .unwrap();
        assert_eq!(cancelled.state, ActorControlState::Applied);
        assert_eq!(cancelled.admission.action, ActorControlAction::CancelTurn);
        assert_eq!(cancelled.admission.turn_id, 1);

        let router =
            crate::agent_jobs::Router::new(owner.store.clone(), owner.owner.clone(), 8).unwrap();
        let mut observe = context.clone();
        observe.effect = aw_agent_core::ToolEffect::Read;
        let projected = aw_tools_adapter::agent_jobs::AgentJobRouter::get(
            &router,
            &observe,
            aw_tools_adapter::agent_jobs::AgentJobGet {
                reference: canonical.clone(),
                revision: None,
                wait_ms: 0,
            },
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            projected.content.contains(" turn cancelling "),
            "applied cancel must project public Turn cancelling before settlement: {}",
            projected.content
        );
        assert!(!projected.content.contains("blocked"));

        let turn = tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(turn.id, 1);
        assert_eq!(turn.outcome, aw_agent_core::Outcome::Cancelled);

        let duplicate = owner
            .jobs_cancel_turn(&context, &canonical, 1, Some(revision))
            .unwrap();
        assert_eq!(duplicate, cancelled);
        assert_eq!(owner.controls().unwrap().len(), 1);

        let mut changed = context.clone();
        changed.call_id = "jobs-cancel-call-2".into();
        assert!(owner
            .jobs_cancel_turn(&changed, &canonical, 1, Some(revision))
            .is_err());
        assert_eq!(owner.controls().unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn jobs_send_same_turn_steer_is_custodied_segmented_and_applied_once() {
        let temp = PrivateTemp::new();
        let principal = PrincipalId::new("local").unwrap();
        let store = SessionStore::open(&temp.0).unwrap();
        let (session, branch) = store
            .create(
                &principal,
                "jobs send steer fixture",
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
        let provider = Arc::new(DurableSteerProvider {
            calls: provider_calls.clone(),
            request_tool: "steer_request",
        });
        let mut fixture = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            8,
            vec![jobs_send_control_grant(), steer_request_control_grant()],
            Some(provider),
        );
        let accepted = Arc::new(Mutex::new(None));
        fixture
            .agent
            .register_host_tool(Arc::new(AuthorityUpdateRequestTool {
                owner: fixture.owner.clone(),
                accepted: accepted.clone(),
                boundary: aw_agent_core::AuthorityBoundaryKind::Steer,
            }))
            .unwrap();
        fixture
            .owner
            .submit(
                &fixture.agent,
                "steer_source",
                input(&fixture.owner, 1, 1, "start then steer"),
            )
            .unwrap();

        let turn = fixture
            .owner
            .run_once(&mut fixture.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(turn.id, 1);
        assert_eq!(turn.outcome, aw_agent_core::Outcome::Completed);
        assert_eq!(turn.answer, "replace");
        assert_eq!(turn.invocations.len(), 2);
        assert_eq!(provider_calls.load(Ordering::SeqCst), 2);
        assert_eq!(turn.tool_records.len(), 1);
        assert_eq!(turn.tool_records[0].call.name, "steer_request");
        assert_eq!(
            turn.tool_records[0].status,
            aw_agent_core::ToolStatus::Succeeded
        );
        assert!(turn
            .context_messages
            .iter()
            .any(|message| { message.role == Role::User && message.content == "replace" }));

        let accepted = accepted
            .lock()
            .unwrap()
            .clone()
            .expect("steer Tool must publish Accepted custody fact");
        assert_eq!(accepted.state, AuthoritySteerState::Accepted);
        assert_eq!(accepted.segment_ordinal, Some(2));
        let applied = fixture
            .store
            .authority_steer_fact(
                &fixture.owner.owner,
                &fixture.owner.session,
                &accepted.request.request_id,
            )
            .unwrap();
        assert_eq!(applied.state, AuthoritySteerState::Applied);
        assert_eq!(applied.segment_ordinal, Some(2));
        assert_eq!(applied.segment_fingerprint, accepted.segment_fingerprint);
        assert!(!serde_json::to_string(&accepted)
            .unwrap()
            .contains("replace"));
        assert!(!serde_json::to_string(&applied).unwrap().contains("replace"));

        let view = fixture
            .store
            .open(
                &fixture.owner.owner,
                &fixture.owner.session,
                &fixture.owner.branch,
            )
            .unwrap()
            .view()
            .unwrap();
        assert_eq!(view.history.turns.len(), 1);
        assert_eq!(view.history.turns[0].id, 1);
        assert_eq!(view.history.turns[0].answer, "replace");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn jobs_send_same_turn_interrupt_is_custodied_segmented_and_applied_once() {
        let temp = PrivateTemp::new();
        let principal = PrincipalId::new("local").unwrap();
        let store = SessionStore::open(&temp.0).unwrap();
        let (session, branch) = store
            .create(
                &principal,
                "jobs send interrupt fixture",
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
        let provider = Arc::new(DurableSteerProvider {
            calls: provider_calls.clone(),
            request_tool: "interrupt_request",
        });
        let mut fixture = setup_bound_with_tools(
            None,
            store,
            session,
            branch,
            9,
            vec![jobs_send_control_grant(), interrupt_request_control_grant()],
            Some(provider),
        );
        let accepted = Arc::new(Mutex::new(None));
        fixture
            .agent
            .register_host_tool(Arc::new(AuthorityUpdateRequestTool {
                owner: fixture.owner.clone(),
                accepted: accepted.clone(),
                boundary: aw_agent_core::AuthorityBoundaryKind::Interrupt,
            }))
            .unwrap();
        fixture
            .owner
            .submit(
                &fixture.agent,
                "interrupt_source",
                input(&fixture.owner, 1, 1, "start then interrupt"),
            )
            .unwrap();

        let turn = fixture
            .owner
            .run_once(&mut fixture.agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(turn.id, 1);
        assert_eq!(turn.outcome, aw_agent_core::Outcome::Completed);
        assert_eq!(turn.answer, "replace");
        assert_eq!(turn.invocations.len(), 2);
        assert_eq!(provider_calls.load(Ordering::SeqCst), 2);
        assert_eq!(turn.tool_records.len(), 1);
        assert_eq!(turn.tool_records[0].call.name, "interrupt_request");
        assert_eq!(
            turn.tool_records[0].status,
            aw_agent_core::ToolStatus::Succeeded
        );
        assert!(turn
            .context_messages
            .iter()
            .any(|message| { message.role == Role::User && message.content == "replace" }));

        let accepted = accepted
            .lock()
            .unwrap()
            .clone()
            .expect("interrupt Tool must publish Accepted custody fact");
        assert!(accepted.request.request_id.starts_with("ji_"));
        assert_eq!(accepted.state, AuthoritySteerState::Accepted);
        assert_eq!(accepted.segment_ordinal, Some(2));
        let applied = fixture
            .store
            .authority_steer_fact(
                &fixture.owner.owner,
                &fixture.owner.session,
                &accepted.request.request_id,
            )
            .unwrap();
        assert_eq!(applied.state, AuthoritySteerState::Applied);
        assert_eq!(applied.segment_ordinal, Some(2));
        assert_eq!(applied.segment_fingerprint, accepted.segment_fingerprint);
        assert!(!serde_json::to_string(&accepted)
            .unwrap()
            .contains("replace"));
        assert!(!serde_json::to_string(&applied).unwrap().contains("replace"));

        let duplicate = fixture
            .store
            .authority_steer_fact(
                &fixture.owner.owner,
                &fixture.owner.session,
                &accepted.request.request_id,
            )
            .unwrap();
        assert_eq!(duplicate, applied);
        let view = fixture
            .store
            .open(
                &fixture.owner.owner,
                &fixture.owner.session,
                &fixture.owner.branch,
            )
            .unwrap()
            .view()
            .unwrap();
        assert_eq!(view.history.turns.len(), 1);
        assert_eq!(view.history.turns[0].id, 1);
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
            call_id: "host-direct".into(),
            source: aw_agent_core::ToolSource::trusted_host(),
            effect: aw_agent_core::ToolEffect::Unknown,
            execution_site: "local".into(),
            cancellation: aw_agent_core::CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: fixture.agent.default_identity().unwrap(),
            scope: fixture.agent.journal_scope().unwrap(),
            execution_owner: None,
            execution_scope: None,
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
