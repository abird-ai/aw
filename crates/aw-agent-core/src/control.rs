//! Single hosted Agent control service over Runtime/Actor/Message/Envelope.
//! No independent model/tool loop, remote actor daemon or unjournaled steering.
//! The caller owns the one Agent; only the established TurnJournal can persist
//! an accepted/settled model or external effect.
use crate::{
    agent::{Agent, AgentEvent},
    journal::JournalFuture,
    runtime::{BoundaryDecision, Delivery, HostRuntime, PendingInputEdit},
    CancellationToken, Error, PrincipalId, Result, Turn,
};
use aw_agent_kernel::{actor::ActorKey, message::ActorMessage};
use std::{
    num::NonZeroU64,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

/// Called at the only safe boundaries: pre-model dispatch, and after a
/// provider result has been journaled but before a new tool batch intent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentGatePoint {
    BeforeModel,
    BeforeToolBatch,
}
pub trait AgentStepGate: Send + Sync {
    fn checkpoint<'a>(
        &'a self,
        turn_id: u64,
        round: u32,
        point: AgentGatePoint,
    ) -> JournalFuture<'a, ()>;
    /// Exact source/effect Envelope enforcement at EVERY model-visible tool
    /// advertisement, actual dispatch and post-effect disclosure.
    fn hook_continuation_port(&self) -> Option<&dyn crate::hooks::HookContinuationPort> {
        None
    }
    fn allow_tool(
        &self,
        name: &str,
        source: &crate::ToolSource,
        effect: &crate::ToolEffect,
    ) -> Result<()>;
}

/// Optional host-only live owner epoch fence. The implementation must query
/// the current authoritative *durable* Workspace writer state, not cached
/// ActorKey or an RPC-supplied generation. It is independent of hooks and
/// never disables mandatory ToolPolicy, Session journal or paid receipts.
pub trait WriterEpochFence: Send + Sync {
    fn verify(&self) -> Result<()>;
}
type ClaimedInputVerification<'a> = dyn Fn(&ActorMessage, u64) -> Result<()> + Send + Sync + 'a;

/// Explicitly authenticated local owner binding; caller must still own the
/// existing Agent and a current Session writer lease. Other hosts/clients
/// cannot deserialize or invent this controller from a display label.
#[derive(Clone)]
pub struct AgentSessionController {
    runtime: Arc<Mutex<HostRuntime>>,
    principal: PrincipalId,
    actor: ActorKey,
    writer_epoch: Option<NonZeroU64>,
    writer_fence: Option<Arc<dyn WriterEpochFence>>,
    wake: watch::Sender<u64>,
}
impl AgentSessionController {
    pub fn new(
        runtime: Arc<Mutex<HostRuntime>>,
        principal: PrincipalId,
        actor: ActorKey,
        writer_epoch: Option<NonZeroU64>,
    ) -> Result<Self> {
        let guard = runtime
            .lock()
            .map_err(|_| Error::Storage("Actor runtime lock poisoned"))?;
        guard.actor(&principal, actor, writer_epoch)?;
        drop(guard);
        let (wake, _) = watch::channel(0);
        Ok(Self {
            runtime,
            principal,
            actor,
            writer_epoch,
            writer_fence: None,
            wake,
        })
    }
    /// Attach an authenticated durable writer epoch fence to an otherwise
    /// Session-bound Actor. Never derived from the tool/model/client request.
    pub fn with_writer_fence(mut self, fence: Arc<dyn WriterEpochFence>) -> Result<Self> {
        fence.verify()?;
        if self.writer_epoch.is_none() {
            return Err(Error::Denied(
                "durable writer fence needs a nonzero Actor writer epoch",
            ));
        }
        self.writer_fence = Some(fence);
        Ok(self)
    }
    fn verify_writer_fence(&self) -> Result<()> {
        if let Some(fence) = &self.writer_fence {
            fence.verify()?;
        }
        Ok(())
    }
    fn with_runtime<T>(&self, f: impl FnOnce(&mut HostRuntime) -> Result<T>) -> Result<T> {
        let mut guard = self
            .runtime
            .lock()
            .map_err(|_| Error::Storage("Actor runtime lock poisoned"))?;
        f(&mut guard)
    }
    fn changed(&self) {
        self.wake.send_modify(|n| *n = n.wrapping_add(1));
    }
    /// Current ActorKey generation/Envelope and Session writer remain valid.
    /// Host-only; no permission is derived from a deserialized string.
    pub fn verify_current_binding(&self) -> Result<()> {
        self.verify_writer_fence()?;
        self.with_runtime(|runtime| {
            let actor = runtime.actor(&self.principal, self.actor, self.writer_epoch)?;
            if !matches!(
                actor.state,
                aw_agent_kernel::actor::ActorState::Running
                    | aw_agent_kernel::actor::ActorState::Waiting
            ) {
                return Err(Error::Denied("RPC Actor generation is not an active owner"));
            }
            runtime.envelope(&self.principal, self.actor, self.writer_epoch)?;
            runtime.session_binding(&self.principal, self.actor, self.writer_epoch)?;
            Ok(())
        })
    }
    /// Host-only pre-admission validation. This checks current exact Actor
    /// Envelope and real Agent journal; it never mints a Session/tool grant.
    pub fn validate_agent_binding(&self, agent: &Agent) -> Result<()> {
        self.check_agent_binding(agent)
    }
    fn check_agent_binding(&self, agent: &Agent) -> Result<()> {
        self.verify_writer_fence()?;
        let identity = agent.default_identity()?;
        if identity.actor != self.principal {
            return Err(Error::Denied(
                "Agent provider account owner differs from controller",
            ));
        }
        let env = self.with_runtime(|r| {
            Ok(r.envelope(&self.principal, self.actor, self.writer_epoch)?
                .clone())
        })?;
        // A site transfer creates a new Actor with a new, explicit Envelope;
        // the Runtime's root site is not this child's execution-site grant.
        if agent.tool_execution_site() != env.site() {
            return Err(Error::Denied(
                "Agent tool site differs from child's authenticated Envelope",
            ));
        }
        agent.validate_runtime_envelope(&env)?;
        let journal = agent.journal_scope()?;
        let session = self
            .with_runtime(|r| r.session_binding(&self.principal, self.actor, self.writer_epoch))?;
        if agent.is_journaled() {
            match journal {
                Some(ref scope) if scope.session_id == session.session_id() => {}
                _ => {
                    return Err(Error::Denied(
                        "journaled Agent must match exact bound Session",
                    ))
                }
            }
        } else if self.writer_epoch.is_some() {
            return Err(Error::Denied(
                "durable writer-bound Actor requires a real Agent journal",
            ));
        }
        Ok(())
    }
    pub fn send(&self, message: ActorMessage, current_turn: u64) -> Result<Delivery> {
        // A stale SQLite Workspace owner may retain a valid in-memory
        // ActorKey. It must not be able to send even a synchronous Control.
        self.verify_writer_fence()?;
        let result = self
            .with_runtime(|r| r.send(&self.principal, self.writer_epoch, message, current_turn))?;
        self.changed();
        Ok(result)
    }
    pub fn pending(&self) -> Result<usize> {
        self.with_runtime(|r| r.pending(&self.principal, self.actor, self.writer_epoch))
    }
    pub fn reject_stale_senders(&self) -> Result<Vec<ActorMessage>> {
        let refused = self.with_runtime(|r| {
            r.reject_stale_senders(&self.principal, self.actor, self.writer_epoch)
        })?;
        if !refused.is_empty() {
            self.changed();
        }
        Ok(refused)
    }
    /// Explicitly reject expired exact-Turn Inputs and return their complete
    /// source-labeled text to the host for a visible rejection notice. A
    /// cancelled/missed message is NEVER shifted to a later Turn or executed.
    pub fn reject_expired(&self, current_turn: u64) -> Result<Vec<ActorMessage>> {
        let rejected = self.with_runtime(|r| {
            r.reject_settled_inputs(&self.principal, self.actor, self.writer_epoch, current_turn)
        })?;
        if !rejected.is_empty() {
            self.changed();
        }
        Ok(rejected)
    }
    pub fn update_pending(
        &self,
        sender: ActorKey,
        id: u64,
        payload: String,
        priority: i8,
        current_turn: u64,
    ) -> Result<()> {
        self.with_runtime(|r| {
            r.update_pending_input(
                &self.principal,
                self.actor,
                self.writer_epoch,
                PendingInputEdit {
                    sender,
                    id,
                    payload,
                    priority,
                    current_turn,
                },
            )
        })?;
        self.changed();
        Ok(())
    }
    pub fn delete_pending(&self, sender: ActorKey, id: u64) -> Result<ActorMessage> {
        let message = self.with_runtime(|r| {
            r.delete_pending_input(&self.principal, self.actor, self.writer_epoch, sender, id)
        })?;
        self.changed();
        Ok(message)
    }
    pub fn active(&self) -> Result<Option<ActorMessage>> {
        self.with_runtime(|r| r.active_input(&self.principal, self.actor, self.writer_epoch))
    }
    pub fn poll_notification(&self, current_turn: u64) -> Result<Option<ActorMessage>> {
        self.with_runtime(|r| {
            r.poll_notification(&self.principal, self.actor, self.writer_epoch, current_turn)
        })
    }
    /// A single queued Input becomes precisely one new Agent Turn. After an
    /// uncertain host/Agent failure, the claim stays marked Unknown and blocks
    /// further automatic dispatch; no automatic retry or reuse of input ID.
    pub async fn run_once<F>(&self, agent: &mut Agent, on_event: F) -> Result<Option<Turn>>
    where
        F: FnMut(&AgentEvent) -> Result<()> + Send,
    {
        self.run_once_inner(agent, None, None, None, on_event).await
    }
    /// Use the exact previously selected parent-approved child route, not an
    /// arbitrary default/payer/credential. One normal journaled Agent Turn.
    pub async fn run_once_selected<F>(
        &self,
        agent: &mut Agent,
        selection: &crate::ProviderSelection,
        on_event: F,
    ) -> Result<Option<Turn>>
    where
        F: FnMut(&AgentEvent) -> Result<()> + Send,
    {
        self.run_once_inner(agent, Some(selection), None, None, on_event)
            .await
    }
    /// A parent's existing CancellationToken is a *cooperative* Turn cancel
    /// wired into this SAME Agent attempt, not a dropped future/retry.
    pub async fn run_once_selected_with_cancel<F>(
        &self,
        agent: &mut Agent,
        selected: &crate::ProviderSelection,
        parent_cancel: CancellationToken,
        on_event: F,
    ) -> Result<Option<Turn>>
    where
        F: FnMut(&AgentEvent) -> Result<()> + Send,
    {
        self.run_once_inner(agent, Some(selected), Some(parent_cancel), None, on_event)
            .await
    }
    /// The only accepted-input hook for a host with an existing durable
    /// queue receipt. The verifier sees the *actual claimed message* after
    /// priority selection and before ANY model dispatch/effect intent.
    /// A failed verification retains an UNKNOWN claim, never auto-requeued.
    pub async fn run_once_verified<F, V>(
        &self,
        agent: &mut Agent,
        verify: &V,
        on_event: F,
    ) -> Result<Option<Turn>>
    where
        F: FnMut(&AgentEvent) -> Result<()> + Send,
        V: Fn(&ActorMessage, u64) -> Result<()> + Send + Sync,
    {
        self.run_once_inner(agent, None, None, Some(verify), on_event)
            .await
    }
    async fn run_once_inner<F>(
        &self,
        agent: &mut Agent,
        selected: Option<&crate::ProviderSelection>,
        parent_cancel: Option<CancellationToken>,
        verification: Option<&ClaimedInputVerification<'_>>,
        on_event: F,
    ) -> Result<Option<Turn>>
    where
        F: FnMut(&AgentEvent) -> Result<()> + Send,
    {
        self.check_agent_binding(agent)?;
        if let Some(selected) = selected {
            selected.revalidate()?;
            let binding = agent.default_identity()?;
            if selected.identity() != &binding {
                return Err(Error::Denied(
                    "child selected route differs from bound Agent identity",
                ));
            }
        }
        if agent.needs_reconciliation() {
            return Err(Error::Denied(
                "Agent needs factual effect inspection before new Turn",
            ));
        }
        let turn_id = agent.history_state().next_turn_id;
        let claimed = self.with_runtime(|r| {
            r.claim_next_input(&self.principal, self.actor, self.writer_epoch, turn_id)
        })?;
        let Some(claimed) = claimed else {
            return Ok(None);
        };
        let mut guard = ClaimGuard {
            controller: self.clone(),
            turn_id,
            armed: true,
        };
        if let Some(verify) = verification {
            verify(&claimed.message, turn_id)?;
        }
        self.verify_writer_fence()?;
        // No Runtime mutex is held during provider, tools, hooks or journal
        // awaits. Concurrent authenticated send/cancel/freeze/resume can run.
        let combined_cancel = async {
            if let Some(parent) = parent_cancel {
                tokio::select! {
                    _=claimed.cancellation.cancelled()=>{},
                    _=parent.cancelled()=>{},
                }
            } else {
                claimed.cancellation.cancelled().await;
            }
        };
        let result = if let Some(selected) = selected {
            agent
                .prompt_with_step_gate_as(
                    selected,
                    claimed.message.payload,
                    combined_cancel,
                    on_event,
                    self,
                )
                .await
        } else {
            agent
                .prompt_with_step_gate(claimed.message.payload, combined_cancel, on_event, self)
                .await
        };
        match result {
            Ok(turn) if turn.id == turn_id => {
                self.verify_writer_fence()?;
                self.with_runtime(|r| {
                    r.finish_claim(
                        &self.principal,
                        self.actor,
                        self.writer_epoch,
                        turn_id,
                        true,
                    )
                })?;
                guard.armed = false;
                self.changed();
                Ok(Some(turn))
            }
            Ok(_) => Err(Error::Conflict(
                "Agent settled a different Turn than the claimed Input",
            )),
            Err(error) => Err(error),
        }
    }
    /// Host-authenticated completion only; no model/tool source receives
    /// a raw Runtime handle or can create/approve its own continuation.
    pub fn resolve_hook(
        &self,
        token: aw_agent_kernel::hook_contract::Continuation,
        flow: aw_agent_kernel::hook_contract::HookFlow<crate::HookEvent>,
    ) -> Result<()> {
        self.with_runtime(|r| {
            r.resolve_agent_hook(&self.principal, self.actor, self.writer_epoch, token, flow)
        })
    }
    /// Human-inspected disposition of an UNKNOWN in-memory claim. It NEVER
    /// runs the previous message again, reuses a paid attempt or clears
    /// independent journal hazards without their own explicit acknowledgement.
    pub async fn discard_after_inspection(&self, agent: &mut Agent, note: &str) -> Result<()> {
        self.check_agent_binding(agent)?;
        if note.trim().is_empty() || note.len() > 4096 {
            return Err(Error::Config(
                "inspection reason must contain 1..4096 bytes".into(),
            ));
        }
        if agent.needs_reconciliation() {
            agent.acknowledge_reconciliation(note).await?;
        }
        self.with_runtime(|r| r.discard_uncertain(&self.principal, self.actor, self.writer_epoch))?;
        self.changed();
        Ok(())
    }
}
impl crate::hooks::HookContinuationPort for AgentSessionController {
    fn reserve(
        &self,
        turn_id: u64,
        stage: aw_agent_kernel::hook_contract::Stage,
    ) -> Result<(
        aw_agent_kernel::hook_contract::Continuation,
        tokio::sync::oneshot::Receiver<aw_agent_kernel::hook_contract::HookFlow<crate::HookEvent>>,
    )> {
        self.verify_writer_fence()?;
        self.with_runtime(|r| {
            r.reserve_agent_hook(
                &self.principal,
                self.actor,
                self.writer_epoch,
                turn_id,
                stage,
            )
        })
    }
    fn abandon(&self, token: aw_agent_kernel::hook_contract::Continuation) {
        let _ = self.with_runtime(|r| {
            r.abandon_agent_hook(token);
            Ok(())
        });
    }
    fn verify(&self, token: aw_agent_kernel::hook_contract::Continuation) -> Result<()> {
        self.verify_writer_fence()?;
        self.with_runtime(|r| {
            r.verify_agent_hook(&self.principal, self.actor, self.writer_epoch, token)
        })
    }
}
impl AgentStepGate for AgentSessionController {
    fn hook_continuation_port(&self) -> Option<&dyn crate::hooks::HookContinuationPort> {
        Some(self)
    }
    fn allow_tool(
        &self,
        name: &str,
        source: &crate::ToolSource,
        effect: &crate::ToolEffect,
    ) -> Result<()> {
        self.verify_writer_fence()?;
        self.with_runtime(|r| {
            let env = r.envelope(&self.principal, self.actor, self.writer_epoch)?;
            if crate::tool::envelope_permits_tool(env, name, source, effect) {
                Ok(())
            } else {
                Err(Error::Denied(
                    "Actor Envelope revoked or lacks exact tool authority",
                ))
            }
        })
    }
    fn checkpoint<'a>(
        &'a self,
        turn_id: u64,
        round: u32,
        point: AgentGatePoint,
    ) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            let mut updates = self.wake.subscribe();
            loop {
                // Mark observed version before checking state. A sender waking
                // between the check and changed() cannot be missed.
                let _ = *updates.borrow_and_update();
                self.verify_writer_fence()?;
                let state = self.with_runtime(|r| {
                    r.boundary_decision(
                        &self.principal,
                        self.actor,
                        self.writer_epoch,
                        turn_id,
                        round,
                        point == AgentGatePoint::BeforeToolBatch,
                    )
                })?;
                match state {
                    BoundaryDecision::Proceed => return Ok(()),
                    BoundaryDecision::Interrupt => return Err(Error::Cancelled),
                    BoundaryDecision::Paused => {
                        updates.changed().await.map_err(|_| {
                            Error::Conflict("Actor controller disconnected while frozen")
                        })?;
                    }
                }
            }
        })
    }
}
struct ClaimGuard {
    controller: AgentSessionController,
    turn_id: u64,
    armed: bool,
}
impl Drop for ClaimGuard {
    fn drop(&mut self) {
        if self.armed {
            // The future may have been dropped during an in-flight paid call.
            // No receipt can be invented in Drop; retain Unknown for inspection.
            let _ = self.controller.with_runtime(|r| {
                r.finish_claim(
                    &self.controller.principal,
                    self.controller.actor,
                    self.controller.writer_epoch,
                    self.turn_id,
                    false,
                )
            });
            self.controller.changed();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CompletionFuture, CompletionMeta, DeltaSink, HistoryState, InvocationScope, JournalEntry,
        JournalFuture, ModelFuture, ModelReply, Outcome, Provider, Tool, ToolCall, ToolContext,
        ToolEffect, ToolFuture, ToolOutput, ToolSpec, TurnJournal, TurnStart, Usage,
    };
    use aw_agent_kernel::{
        actor::RuntimeKey,
        envelope::{Access, Budget, EnvelopeSpec, Network},
        message::{ControlKind, Kind, Scope, TurnTarget},
    };
    use serde_json::{json, Value};
    use std::{
        future::pending,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use tokio::{
        sync::Notify,
        time::{sleep, timeout, Duration},
    };
    fn nz(n: u64) -> NonZeroU64 {
        NonZeroU64::new(n).unwrap()
    }
    fn principal() -> PrincipalId {
        PrincipalId::new("local").unwrap()
    }
    fn runtime_with_scope(persisted: bool) -> (AgentSessionController, ActorKey) {
        let key = RuntimeKey {
            id: nz(77),
            generation: nz(1),
        };
        let mut runtime = HostRuntime::new(principal(), "local", key, 5, 3).unwrap();
        let spec = EnvelopeSpec {
            actor: "local".into(),
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
                max_output_bytes: 524288,
            },
            // Explicit root ceiling does not automatically register/admit any
            // Tool. The existing Agent ToolPolicy is a second narrowing bound.
            grants: vec![
                aw_agent_kernel::envelope::ExactGrant {
                    name: "count".into(),
                    kind: aw_agent_kernel::capability::SourceKind::Host,
                    source: "trusted-host".into(),
                    source_epoch: 1,
                    effect: aw_agent_kernel::capability::Effect::Read,
                },
                aw_agent_kernel::envelope::ExactGrant {
                    name: "slow_write".into(),
                    kind: aw_agent_kernel::capability::SourceKind::Host,
                    source: "trusted-host".into(),
                    source_epoch: 1,
                    effect: aw_agent_kernel::capability::Effect::Write,
                },
            ],
        };
        let epoch = persisted.then_some(nz(9));
        let binding =
            crate::runtime::SessionBinding::from_trusted_host("test-session", epoch).unwrap();
        let actor = runtime
            .bind_agent(&principal(), binding, None, spec, "root")
            .unwrap();
        let shared = Arc::new(Mutex::new(runtime));
        (
            AgentSessionController::new(shared, principal(), actor, epoch).unwrap(),
            actor,
        )
    }
    fn input(actor: ActorKey, id: u64, priority: i8, text: &str) -> ActorMessage {
        ActorMessage {
            id,
            from: actor,
            to: actor,
            kind: Kind::Input,
            priority,
            turn: TurnTarget::Auto,
            source_sequence: id,
            payload: text.into(),
            content_type: aw_agent_kernel::message::ContentType::Text,
        }
    }
    fn control(actor: ActorKey, id: u64, kind: ControlKind) -> ActorMessage {
        ActorMessage {
            id,
            from: actor,
            to: actor,
            kind: Kind::Control(kind),
            priority: 0,
            turn: TurnTarget::Auto,
            source_sequence: id,
            payload: String::new(),
            content_type: aw_agent_kernel::message::ContentType::Text,
        }
    }
    struct EchoProvider(Arc<AtomicUsize>);
    impl Provider for EchoProvider {
        fn stream<'a>(
            &'a self,
            messages: &'a [crate::Message],
            _model: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                self.0.fetch_add(1, Ordering::SeqCst);
                sink(&messages.last().expect("prompt").content)?;
                Ok(CompletionMeta {
                    finish_reason: "stop".into(),
                    usage: Usage {
                        input_tokens: Some(3),
                        output_tokens: Some(1),
                    },
                })
            })
        }
    }
    #[tokio::test]
    async fn one_hosted_agent_runs_priority_queue_as_distinct_turns() {
        let (ctrl, actor) = runtime_with_scope(false);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut agent = Agent::builder(
            Arc::new(EchoProvider(calls.clone())),
            crate::AgentOptions::new("p", "m"),
        )
        .build()
        .unwrap();
        assert!(matches!(
            ctrl.send(input(actor, 1, 0, "ordinary"), 1).unwrap(),
            Delivery::Queued(_)
        ));
        ctrl.send(input(actor, 2, -2, "urgent"), 1).unwrap();
        assert_eq!(ctrl.pending().unwrap(), 2);
        let first = ctrl
            .run_once(&mut agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        let second = ctrl
            .run_once(&mut agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.answer, "urgent");
        assert_eq!(second.answer, "ordinary");
        assert_eq!(first.id, 1);
        assert_eq!(second.id, 2);
        assert_eq!(agent.history_state().turns.len(), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(ctrl
            .run_once(&mut agent, |_| Ok(()))
            .await
            .unwrap()
            .is_none());
        assert_eq!(ctrl.pending().unwrap(), 0);
    }
    #[tokio::test]
    async fn paused_queue_and_pending_edit_delete_never_replay_claimed_input() {
        let (ctrl, actor) = runtime_with_scope(false);
        let mut agent = Agent::builder(
            Arc::new(EchoProvider(Arc::new(AtomicUsize::new(0)))),
            crate::AgentOptions::new("p", "m"),
        )
        .build()
        .unwrap();
        assert!(matches!(
            ctrl.send(control(actor, 1, ControlKind::Freeze(Scope::Actor)), 1)
                .unwrap(),
            Delivery::Applied(_)
        ));
        ctrl.send(input(actor, 2, 0, "delete-me"), 1).unwrap();
        ctrl.send(input(actor, 3, 0, "pre-edit"), 1).unwrap();
        assert!(ctrl
            .run_once(&mut agent, |_| Ok(()))
            .await
            .unwrap()
            .is_none());
        ctrl.update_pending(actor, 3, "edited".into(), -2, 1)
            .unwrap();
        assert_eq!(ctrl.delete_pending(actor, 2).unwrap().payload, "delete-me");
        assert!(ctrl.delete_pending(actor, 2).is_err());
        assert!(ctrl.send(input(actor, 2, 0, "replay"), 1).is_err());
        assert!(matches!(
            ctrl.send(control(actor, 4, ControlKind::Resume(Scope::Actor)), 1)
                .unwrap(),
            Delivery::Applied(_)
        ));
        let turn = ctrl
            .run_once(&mut agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(turn.answer, "edited");
        assert!(ctrl.update_pending(actor, 3, "late".into(), -1, 2).is_err());
    }
    struct SlowProvider {
        entered: Arc<Notify>,
        calls: Arc<AtomicUsize>,
    }
    impl Provider for SlowProvider {
        fn stream<'a>(
            &'a self,
            _: &'a [crate::Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.entered.notify_one();
                pending::<crate::Result<CompletionMeta>>().await
            })
        }
    }
    #[tokio::test]
    async fn cancel_in_flight_provider_records_cancelled_paid_attempt_once() {
        let (ctrl, actor) = runtime_with_scope(false);
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(SlowProvider {
            entered: Arc::new(Notify::new()),
            calls: calls.clone(),
        });
        let entered = provider.entered.clone();
        let mut agent = Agent::builder(provider, crate::AgentOptions::new("p", "m"))
            .build()
            .unwrap();
        ctrl.send(input(actor, 1, 0, "long"), 1).unwrap();
        let issuing = ctrl.clone();
        let sender = async move {
            entered.notified().await;
            assert!(matches!(
                issuing.send(control(actor, 2, ControlKind::Cancel(Scope::Turn)), 1),
                Ok(Delivery::Applied(_))
            ));
        };
        let (turn, ()) = timeout(Duration::from_secs(3), async {
            tokio::join!(ctrl.run_once(&mut agent, |_| Ok(())), sender)
        })
        .await
        .unwrap();
        let turn = turn.unwrap().unwrap();
        assert_eq!(turn.outcome, Outcome::Cancelled);
        assert_eq!(turn.invocations.len(), 1);
        assert_eq!(turn.invocations[0].outcome, "failed_or_cancelled");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(ctrl
            .run_once(&mut agent, |_| Ok(()))
            .await
            .unwrap()
            .is_none());
    }
    struct CallProvider {
        ctrl: AgentSessionController,
        actor: ActorKey,
        triggered: Arc<AtomicBool>,
        kind: CallAction,
    }
    #[derive(Clone, Copy)]
    enum CallAction {
        Interrupt,
        Freeze,
    }
    impl Provider for CallProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [crate::Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("test requires typed tool invocation")) })
        }
        fn stream_tools<'a>(
            &'a self,
            history: &'a [crate::Message],
            _: &'a str,
            _: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                if !self.triggered.swap(true, Ordering::SeqCst) {
                    match self.kind {
                        CallAction::Interrupt => {
                            self.ctrl.send(input(self.actor, 2, -5, "replace"), 1)?;
                        }
                        CallAction::Freeze => {
                            self.ctrl.send(
                                control(self.actor, 2, ControlKind::Freeze(Scope::Turn)),
                                1,
                            )?;
                        }
                    }
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            usage: Usage {
                                input_tokens: Some(2),
                                output_tokens: Some(2),
                            },
                        },
                        tool_calls: vec![ToolCall {
                            id: "tool-call-1".into(),
                            name: "count".into(),
                            arguments: json!({}),
                        }],
                        ..Default::default()
                    })
                } else {
                    sink(&history.last().expect("history").content)?;
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(2),
                                output_tokens: Some(1),
                            },
                        },
                        ..Default::default()
                    })
                }
            })
        }
    }
    struct CountTool(Arc<AtomicUsize>);
    impl Tool for CountTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "count".into(),
                description: "fake read".into(),
                parameters: json!({"type":"object","additionalProperties":false}),
                effect: ToolEffect::Read,
            }
        }
        fn validate(&self, v: &Value) -> Result<()> {
            if v.as_object().is_some_and(|o| o.is_empty()) {
                Ok(())
            } else {
                Err(Error::Denied("expected empty"))
            }
        }
        fn execute<'a>(&'a self, _: ToolContext, _: &'a Value) -> ToolFuture<'a> {
            Box::pin(async move {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(ToolOutput::text("effect-complete"))
            })
        }
    }
    #[tokio::test]
    async fn urgent_steer_prevents_tool_intent_then_becomes_distinct_turn() {
        let (ctrl, actor) = runtime_with_scope(false);
        let tool_calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(CallProvider {
            ctrl: ctrl.clone(),
            actor,
            triggered: Arc::new(AtomicBool::new(false)),
            kind: CallAction::Interrupt,
        });
        let mut agent = Agent::builder(provider, crate::AgentOptions::new("p", "m"))
            .build()
            .unwrap();
        agent
            .register_host_tool(Arc::new(CountTool(tool_calls.clone())))
            .unwrap();
        ctrl.send(input(actor, 1, 0, "first"), 1).unwrap();
        let interrupted = ctrl
            .run_once(&mut agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(interrupted.outcome, Outcome::Cancelled);
        assert_eq!(interrupted.invocations.len(), 1);
        assert!(
            interrupted.tool_records.is_empty(),
            "no tool intent was admitted"
        );
        assert_eq!(tool_calls.load(Ordering::SeqCst), 0);
        assert_eq!(ctrl.pending().unwrap(), 1);
        let next = ctrl
            .run_once(&mut agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(next.id, 2);
        assert_eq!(next.answer, "replace");
        assert_eq!(next.outcome, Outcome::Completed);
        assert_eq!(tool_calls.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn safe_point_freeze_does_not_execute_tools_until_explicit_resume() {
        let (ctrl, actor) = runtime_with_scope(false);
        let tool_calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(CallProvider {
            ctrl: ctrl.clone(),
            actor,
            triggered: Arc::new(AtomicBool::new(false)),
            kind: CallAction::Freeze,
        });
        let mut agent = Agent::builder(provider, crate::AgentOptions::new("p", "m"))
            .build()
            .unwrap();
        agent
            .register_host_tool(Arc::new(CountTool(tool_calls.clone())))
            .unwrap();
        ctrl.send(input(actor, 1, 0, "first"), 1).unwrap();
        let sender = ctrl.clone();
        let free = tool_calls.clone();
        let resume = async move {
            sleep(Duration::from_millis(45)).await;
            assert_eq!(
                free.load(Ordering::SeqCst),
                0,
                "tool cannot enter before the frozen safe point resumes"
            );
            assert!(matches!(
                sender.send(control(actor, 3, ControlKind::Resume(Scope::Turn)), 1),
                Ok(Delivery::Applied(_))
            ));
        };
        let (turn, ()) = timeout(Duration::from_secs(3), async {
            tokio::join!(ctrl.run_once(&mut agent, |_| Ok(())), resume)
        })
        .await
        .unwrap();
        let turn = turn.unwrap().unwrap();
        assert_eq!(turn.outcome, Outcome::Completed);
        assert_eq!(turn.tool_records.len(), 1);
        assert_eq!(tool_calls.load(Ordering::SeqCst), 1);
    }
    #[derive(Clone)]
    struct Recorder {
        events: Arc<Mutex<Vec<String>>>,
    }
    impl TurnJournal for Recorder {
        fn invocation_scope(&self) -> Result<Option<InvocationScope>> {
            Ok(Some(InvocationScope {
                session_id: "test-session".into(),
                branch_id: "main".into(),
            }))
        }
        fn begin<'a>(&'a self, start: TurnStart) -> JournalFuture<'a, ()> {
            Box::pin(async move {
                self.events
                    .lock()
                    .unwrap()
                    .push(format!("begin:{}", start.turn_id));
                Ok(())
            })
        }
        fn append<'a>(&'a self, id: u64, entry: JournalEntry) -> JournalFuture<'a, ()> {
            Box::pin(async move {
                let kind = match entry {
                    JournalEntry::InvocationStarted { .. } => "intent",
                    JournalEntry::InvocationFinished { .. } => "result",
                    JournalEntry::Assistant { .. } => "assistant",
                    JournalEntry::ToolStarted { .. } => "tool-intent",
                    JournalEntry::ToolFinished { .. } => "tool-result",
                    JournalEntry::ToolDenied { .. } => "tool-denied",
                    JournalEntry::ToolRevised { .. } => "tool-revised",
                };
                self.events.lock().unwrap().push(format!("{kind}:{id}"));
                Ok(())
            })
        }
        fn finish<'a>(&'a self, turn: Turn) -> JournalFuture<'a, ()> {
            Box::pin(async move {
                self.events
                    .lock()
                    .unwrap()
                    .push(format!("settle:{}", turn.id));
                Ok(())
            })
        }
        fn acknowledge<'a>(&'a self, _: String) -> JournalFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }
    }
    #[tokio::test]
    async fn journaled_queue_uses_real_begin_and_receipts_one_time() {
        let (ctrl, actor) = runtime_with_scope(true);
        let events = Arc::new(Mutex::new(Vec::new()));
        let journal = Arc::new(Recorder {
            events: events.clone(),
        });
        let mut agent = Agent::builder(
            Arc::new(EchoProvider(Arc::new(AtomicUsize::new(0)))),
            crate::AgentOptions::new("p", "m"),
        )
        .build()
        .unwrap();
        agent
            .load_history(HistoryState::default(), Some(journal))
            .unwrap();
        ctrl.send(input(actor, 1, 0, "recorded"), 1).unwrap();
        assert_eq!(
            ctrl.run_once(&mut agent, |_| Ok(()))
                .await
                .unwrap()
                .unwrap()
                .answer,
            "recorded"
        );
        assert_eq!(
            &*events.lock().unwrap(),
            &["begin:1", "intent:1", "result:1", "assistant:1", "settle:1"]
        );
        assert!(ctrl
            .run_once(&mut agent, |_| Ok(()))
            .await
            .unwrap()
            .is_none());
        assert_eq!(events.lock().unwrap().len(), 5);
    }
    #[tokio::test]
    async fn dropped_runner_marks_claim_unknown_and_never_replays() {
        let (ctrl, actor) = runtime_with_scope(false);
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(SlowProvider {
            entered: Arc::new(Notify::new()),
            calls: calls.clone(),
        });
        let entered = provider.entered.clone();
        ctrl.send(input(actor, 1, 0, "paid"), 1).unwrap();
        let worker = ctrl.clone();
        let task = tokio::spawn(async move {
            let mut agent = Agent::builder(provider, crate::AgentOptions::new("p", "m"))
                .build()
                .unwrap();
            let _ = worker.run_once(&mut agent, |_| Ok(())).await;
        });
        timeout(Duration::from_secs(2), entered.notified())
            .await
            .unwrap();
        task.abort();
        let _ = task.await;
        let mut fresh = Agent::builder(
            Arc::new(EchoProvider(calls.clone())),
            crate::AgentOptions::new("p", "m"),
        )
        .build()
        .unwrap();
        assert!(ctrl.run_once(&mut fresh, |_| Ok(())).await.is_err());
        assert!(ctrl.active().unwrap().is_some());
        assert_eq!(ctrl.pending().unwrap(), 0);
        ctrl.discard_after_inspection(&mut fresh, "inspected uncertain paid operation")
            .await
            .unwrap();
        assert_eq!(ctrl.pending().unwrap(), 0);
        assert!(ctrl.active().unwrap().is_none());
        assert!(ctrl
            .run_once(&mut fresh, |_| Ok(()))
            .await
            .unwrap()
            .is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn expired_exact_turn_is_returned_for_reporting_before_next_admission() {
        let (ctrl, actor) = runtime_with_scope(false);
        let mut agent = Agent::builder(
            Arc::new(EchoProvider(Arc::new(AtomicUsize::new(0)))),
            crate::AgentOptions::new("p", "m"),
        )
        .build()
        .unwrap();
        ctrl.send(input(actor, 1, 0, "first"), 1).unwrap();
        let mut exact = input(actor, 2, 0, "missed");
        exact.turn = TurnTarget::Exact(1);
        ctrl.send(exact, 1).unwrap();
        assert!(ctrl
            .run_once(&mut agent, |_| Ok(()))
            .await
            .unwrap()
            .is_some());
        // A missed exact Turn is never silently moved to Turn 2.
        assert!(ctrl.run_once(&mut agent, |_| Ok(())).await.is_err());
        assert_eq!(ctrl.pending().unwrap(), 1);
        let rejected = ctrl.reject_expired(2).unwrap();
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].payload, "missed");
        assert!(ctrl
            .run_once(&mut agent, |_| Ok(()))
            .await
            .unwrap()
            .is_none());
    }
    #[tokio::test]
    async fn full_input_queue_does_not_block_authenticated_turn_cancel() {
        let (ctrl, actor) = runtime_with_scope(false);
        let provider = Arc::new(SlowProvider {
            entered: Arc::new(Notify::new()),
            calls: Arc::new(AtomicUsize::new(0)),
        });
        let started = provider.entered.clone();
        let mut agent = Agent::builder(provider, crate::AgentOptions::new("p", "m"))
            .build()
            .unwrap();
        ctrl.send(input(actor, 1, 0, "start"), 1).unwrap();
        let sender = ctrl.clone();
        let filling = async move {
            started.notified().await;
            for id in 2..=4 {
                sender.send(input(actor, id, 0, "queued"), 1).unwrap();
            }
            assert_eq!(sender.pending().unwrap(), 3);
            assert!(sender.send(input(actor, 5, 0, "excess"), 1).is_err());
            assert!(matches!(
                sender.send(control(actor, 5, ControlKind::Cancel(Scope::Turn)), 1),
                Ok(Delivery::Applied(_))
            ));
            assert_eq!(sender.pending().unwrap(), 3);
        };
        let (turn, ()) = timeout(Duration::from_secs(3), async {
            tokio::join!(ctrl.run_once(&mut agent, |_| Ok(())), filling)
        })
        .await
        .unwrap();
        assert_eq!(turn.unwrap().unwrap().outcome, Outcome::Cancelled);
        assert_eq!(ctrl.pending().unwrap(), 3);
    }
    struct ToolProposal;
    impl Provider for ToolProposal {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [crate::Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("tool proposal required")) })
        }
        fn stream_tools<'a>(
            &'a self,
            history: &'a [crate::Message],
            _: &'a str,
            _: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                if history.last().is_some_and(|m| m.role == crate::Role::Tool) {
                    sink("done")?;
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(2),
                                output_tokens: Some(1),
                            },
                        },
                        ..Default::default()
                    })
                } else {
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            usage: Usage {
                                input_tokens: Some(2),
                                output_tokens: Some(1),
                            },
                        },
                        tool_calls: vec![ToolCall {
                            id: "write-1".into(),
                            name: "slow_write".into(),
                            arguments: json!({}),
                        }],
                        ..Default::default()
                    })
                }
            })
        }
    }
    struct SlowWrite {
        started: Arc<Notify>,
        entered: Arc<AtomicUsize>,
    }
    impl Tool for SlowWrite {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "slow_write".into(),
                description: "blocking external write".into(),
                parameters: json!({"type":"object","additionalProperties":false}),
                effect: ToolEffect::Write,
            }
        }
        fn validate(&self, v: &Value) -> Result<()> {
            if v.as_object().is_some_and(|o| o.is_empty()) {
                Ok(())
            } else {
                Err(Error::Denied("unexpected args"))
            }
        }
        fn execute<'a>(&'a self, _: ToolContext, _: &'a Value) -> ToolFuture<'a> {
            Box::pin(async move {
                self.entered.fetch_add(1, Ordering::SeqCst);
                self.started.notify_one();
                pending::<Result<ToolOutput>>().await
            })
        }
    }
    #[tokio::test]
    async fn cancel_during_write_retains_hazard_and_blocks_next_queued_turn() {
        let (ctrl, actor) = runtime_with_scope(false);
        let tool = Arc::new(SlowWrite {
            started: Arc::new(Notify::new()),
            entered: Arc::new(AtomicUsize::new(0)),
        });
        let started = tool.started.clone();
        let entered = tool.entered.clone();
        let mut agent = Agent::builder(Arc::new(ToolProposal), crate::AgentOptions::new("p", "m"))
            .build()
            .unwrap();
        agent.register_host_tool(tool).unwrap();
        ctrl.send(input(actor, 1, 0, "effectful"), 1).unwrap();
        let sender = ctrl.clone();
        let cancelling = async move {
            started.notified().await;
            sender.send(input(actor, 2, 0, "followup"), 1).unwrap();
            assert!(matches!(
                sender.send(control(actor, 3, ControlKind::Cancel(Scope::Turn)), 1),
                Ok(Delivery::Applied(_))
            ));
        };
        let (turn, ()) = timeout(Duration::from_secs(3), async {
            tokio::join!(ctrl.run_once(&mut agent, |_| Ok(())), cancelling)
        })
        .await
        .unwrap();
        let turn = turn.unwrap().unwrap();
        assert_eq!(turn.outcome, Outcome::Cancelled);
        assert_eq!(entered.load(Ordering::SeqCst), 1);
        assert!(turn.needs_reconciliation);
        assert!(agent.needs_reconciliation());
        assert_eq!(ctrl.pending().unwrap(), 1);
        assert!(ctrl.run_once(&mut agent, |_| Ok(())).await.is_err());
        assert_eq!(ctrl.pending().unwrap(), 1);
    }
    struct RogueProcessTool;
    impl Tool for RogueProcessTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "ungranted_process".into(),
                description: "must never run".into(),
                parameters: json!({"type":"object"}),
                effect: ToolEffect::Process,
            }
        }
        fn validate(&self, _: &Value) -> Result<()> {
            Ok(())
        }
        fn execute<'a>(&'a self, _: ToolContext, _: &'a Value) -> ToolFuture<'a> {
            Box::pin(async { panic!("controller let an ungranted Process tool execute") })
        }
    }
    #[tokio::test]
    async fn actor_envelope_is_an_additional_narrow_only_tool_ceiling() {
        let (ctrl, actor) = runtime_with_scope(false);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut agent = Agent::builder(
            Arc::new(EchoProvider(calls.clone())),
            crate::AgentOptions::new("p", "m"),
        )
        .build()
        .unwrap();
        agent
            .register_host_tool(Arc::new(RogueProcessTool))
            .unwrap();
        ctrl.send(input(actor, 1, 0, "never billed"), 1).unwrap();
        assert!(ctrl.run_once(&mut agent, |_| Ok(())).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            ctrl.pending().unwrap(),
            1,
            "denial happened before admission claim"
        );
        assert!(ctrl.active().unwrap().is_none());
    }
    struct ControlledFence(Arc<AtomicBool>);
    impl WriterEpochFence for ControlledFence {
        fn verify(&self) -> Result<()> {
            if self.0.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(Error::Conflict("live Workspace writer epoch revoked"))
            }
        }
    }
    struct FlippingProvider {
        active: Arc<AtomicBool>,
        calls: Arc<AtomicUsize>,
    }
    impl Provider for FlippingProvider {
        fn stream<'a>(
            &'a self,
            _: &'a [crate::Message],
            _: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                sink("paid response")?;
                self.active.store(false, Ordering::SeqCst);
                Ok(CompletionMeta {
                    finish_reason: "stop".into(),
                    usage: Usage {
                        input_tokens: Some(1),
                        output_tokens: Some(1),
                    },
                })
            })
        }
    }
    #[test]
    fn stale_writer_cannot_accept_new_input_or_synchronous_control() {
        let (ctrl, actor) = runtime_with_scope(true);
        let allowed = Arc::new(AtomicBool::new(true));
        let ctrl = ctrl
            .with_writer_fence(Arc::new(ControlledFence(allowed.clone())))
            .unwrap();
        assert!(matches!(
            ctrl.send(input(actor, 1, 0, "initial"), 1),
            Ok(Delivery::Queued(_))
        ));
        assert_eq!(ctrl.pending().unwrap(), 1);
        allowed.store(false, Ordering::SeqCst);
        assert!(ctrl.send(input(actor, 2, 0, "later"), 1).is_err());
        assert!(ctrl
            .send(control(actor, 2, ControlKind::Freeze(Scope::Actor)), 1)
            .is_err());
        assert_eq!(ctrl.pending().unwrap(), 1);
        allowed.store(true, Ordering::SeqCst);
        assert!(matches!(
            ctrl.send(input(actor, 2, 0, "new"), 1),
            Ok(Delivery::Queued(_))
        ));
        assert_eq!(ctrl.pending().unwrap(), 2);
    }
    #[tokio::test]
    async fn writer_epoch_fence_rechecks_after_paid_provider_and_never_replays() {
        let (ctrl, actor) = runtime_with_scope(true);
        let allowed = Arc::new(AtomicBool::new(true));
        let ctrl = ctrl
            .with_writer_fence(Arc::new(ControlledFence(allowed.clone())))
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut agent = Agent::builder(
            Arc::new(FlippingProvider {
                active: allowed.clone(),
                calls: calls.clone(),
            }),
            crate::AgentOptions::new("p", "m"),
        )
        .build()
        .unwrap();
        agent
            .load_history(
                HistoryState::default(),
                Some(Arc::new(Recorder {
                    events: Arc::new(Mutex::new(Vec::new())),
                })),
            )
            .unwrap();
        ctrl.send(input(actor, 1, 0, "paid intent"), 1).unwrap();
        assert!(ctrl.run_once(&mut agent, |_| Ok(())).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(
            ctrl.active().unwrap().is_some(),
            "stale owner may not settle a claimed Turn"
        );
        assert!(ctrl.run_once(&mut agent, |_| Ok(())).await.is_err());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "paid attempt must never auto-replay after lost writer epoch"
        );
        assert!(
            AgentSessionController::new(ctrl.runtime.clone(), principal(), actor, Some(nz(9)))
                .unwrap()
                .with_writer_fence(Arc::new(ControlledFence(allowed)))
                .is_err()
        );
    }
    #[tokio::test]
    async fn mismatched_writer_epoch_and_session_are_denied_before_effect() {
        let (ctrl, actor) = runtime_with_scope(true);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut agent = Agent::builder(
            Arc::new(EchoProvider(calls.clone())),
            crate::AgentOptions::new("p", "m"),
        )
        .build()
        .unwrap();
        // Durable owner must supply a journal with the exact Session.
        ctrl.send(input(actor, 1, 0, "not authorized"), 1).unwrap();
        assert!(ctrl.run_once(&mut agent, |_| Ok(())).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(ctrl.pending().unwrap(), 1);
        let old =
            AgentSessionController::new(ctrl.runtime.clone(), principal(), actor, Some(nz(8)));
        assert!(old.is_err());
    }
    #[test]
    fn child_notifications_share_the_inbox_without_becoming_steering() {
        let (ctrl, root) = runtime_with_scope(false);
        let child = ctrl
            .with_runtime(|r| {
                let parent = r.envelope(&principal(), root, None)?.clone();
                let mut spec = parent.spec().clone();
                spec.revision = 2;
                let binding = crate::runtime::SessionBinding::from_trusted_host("child", None)?;
                r.bind_agent(&principal(), binding, Some(root), spec, "child")
            })
            .unwrap();
        let message = ActorMessage {
            id: 1,
            from: child,
            to: root,
            kind: Kind::Notify,
            turn: TurnTarget::Auto,
            priority: 0,
            source_sequence: 1,
            payload: "done".into(),
            content_type: aw_agent_kernel::message::ContentType::Text,
        };
        ctrl.send(message, 1).unwrap();
        let observed = ctrl.poll_notification(1).unwrap().unwrap();
        assert_eq!(observed.payload, "done");
        assert_eq!(observed.from, child);
        assert!(ctrl.poll_notification(1).unwrap().is_none());
        ctrl.with_runtime(|r| {
            r.record_state(
                &principal(),
                child,
                None,
                aw_agent_kernel::actor::ActorState::Running,
            )?;
            r.record_state(
                &principal(),
                child,
                None,
                aw_agent_kernel::actor::ActorState::Settled,
            )
        })
        .unwrap();
        // A child that has factually settled may still send its final notice
        // to its parent, but cannot steer or reopen a terminal Actor.
        ctrl.send(
            ActorMessage {
                id: 2,
                from: child,
                to: root,
                kind: Kind::Notify,
                turn: TurnTarget::Auto,
                priority: 0,
                source_sequence: 2,
                payload: "finished".into(),
                content_type: aw_agent_kernel::message::ContentType::Text,
            },
            1,
        )
        .unwrap();
        assert_eq!(
            ctrl.poll_notification(1).unwrap().unwrap().payload,
            "finished"
        );
        assert!(ctrl
            .send(
                ActorMessage {
                    id: 3,
                    from: child,
                    to: root,
                    kind: Kind::Input,
                    turn: TurnTarget::Auto,
                    priority: 0,
                    source_sequence: 3,
                    payload: "steer parent".into(),
                    content_type: aw_agent_kernel::message::ContentType::Text,
                },
                1
            )
            .is_err());
    }
    struct LateTool {
        live: Arc<AtomicBool>,
        executed: Arc<AtomicUsize>,
    }
    impl Tool for LateTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "late_tool".into(),
                description: "revived after initial admission".into(),
                parameters: json!({"type":"object","additionalProperties":false}),
                effect: ToolEffect::Read,
            }
        }
        fn is_available(&self) -> bool {
            self.live.load(Ordering::SeqCst)
        }
        fn validate(&self, _: &Value) -> Result<()> {
            Ok(())
        }
        fn execute<'a>(&'a self, _: ToolContext, _: &'a Value) -> ToolFuture<'a> {
            Box::pin(async move {
                self.executed.fetch_add(1, Ordering::SeqCst);
                Ok(ToolOutput::text("should never execute"))
            })
        }
    }
    struct RevivingProvider(Arc<AtomicBool>, Arc<AtomicUsize>);
    impl Provider for RevivingProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [crate::Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("tool proposal required")) })
        }
        fn stream_tools<'a>(
            &'a self,
            messages: &'a [crate::Message],
            _: &'a str,
            _: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                self.1.fetch_add(1, Ordering::SeqCst);
                if messages.last().is_some_and(|m| m.role == crate::Role::Tool) {
                    sink("tool was denied; acknowledged")?;
                    return Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(2),
                                output_tokens: Some(1),
                            },
                        },
                        ..Default::default()
                    });
                }
                // Source was invisible to the initial envelope check, but
                // revives after an async provider dispatch. ToolPolicy alone
                // is insufficient: the Actor Envelope lacks this exact grant.
                self.0.store(true, Ordering::SeqCst);
                Ok(ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "tool_calls".into(),
                        usage: Usage {
                            input_tokens: Some(2),
                            output_tokens: Some(1),
                        },
                    },
                    tool_calls: vec![ToolCall {
                        id: "late-1".into(),
                        name: "late_tool".into(),
                        arguments: json!({}),
                    }],
                    ..Default::default()
                })
            })
        }
    }
    #[tokio::test]
    async fn revived_external_source_cannot_outrun_actor_envelope_at_dispatch() {
        let (ctrl, actor) = runtime_with_scope(false);
        let live = Arc::new(AtomicBool::new(false));
        let executed = Arc::new(AtomicUsize::new(0));
        let model_calls = Arc::new(AtomicUsize::new(0));
        let source = crate::ToolSource::new(crate::SourceKind::Extension, "revivable", 8).unwrap();
        let mut agent = Agent::builder(
            Arc::new(RevivingProvider(live.clone(), model_calls.clone())),
            crate::AgentOptions::new("p", "m"),
        )
        .build()
        .unwrap();
        agent
            .register_source_tool(
                Arc::new(LateTool {
                    live: live.clone(),
                    executed: executed.clone(),
                }),
                source.clone(),
            )
            .unwrap();
        // Current ToolPolicy explicitly grants this source, but the Actor's
        // independent Envelope is narrow and intentionally has NO late_tool.
        agent
            .permit_registered_source_tool(principal(), "late_tool", &source)
            .unwrap();
        ctrl.send(input(actor, 1, 0, "attempt"), 1).unwrap();
        let turn = ctrl
            .run_once(&mut agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(turn.outcome, Outcome::Completed);
        assert_eq!(turn.invocations.len(), 2);
        assert_eq!(turn.tool_records.len(), 1);
        assert_eq!(turn.tool_records[0].status, crate::ToolStatus::Denied);
        assert_eq!(turn.tool_records[0].call.id, "late-1");
        assert_eq!(model_calls.load(Ordering::SeqCst), 2);
        assert_eq!(executed.load(Ordering::SeqCst), 0);
        assert!(live.load(Ordering::SeqCst));
    }
    #[test]
    fn retired_child_messages_must_be_rejected_before_parent_visibility() {
        let (ctrl, root) = runtime_with_scope(false);
        let child = ctrl
            .with_runtime(|r| {
                let mut spec = r.envelope(&principal(), root, None)?.spec().clone();
                spec.revision = 2;
                let session = crate::runtime::SessionBinding::from_trusted_host("child-2", None)?;
                r.bind_agent(&principal(), session, Some(root), spec, "child")
            })
            .unwrap();
        ctrl.send(
            ActorMessage {
                id: 1,
                from: child,
                to: root,
                kind: Kind::Notify,
                turn: TurnTarget::Auto,
                priority: 0,
                source_sequence: 1,
                payload: "old-generation".into(),
                content_type: aw_agent_kernel::message::ContentType::Text,
            },
            1,
        )
        .unwrap();
        ctrl.with_runtime(|r| {
            r.record_state(
                &principal(),
                child,
                None,
                aw_agent_kernel::actor::ActorState::Running,
            )?;
            r.record_state(
                &principal(),
                child,
                None,
                aw_agent_kernel::actor::ActorState::Settled,
            )?;
            r.release(&principal(), child, None)
        })
        .unwrap();
        // The notice is still in the parent's bounded mailbox but is NOT
        // returned as an active child event after its sender was retired.
        assert!(ctrl.poll_notification(1).is_err());
        let refused = ctrl.reject_stale_senders().unwrap();
        assert_eq!(refused.len(), 1);
        assert_eq!(refused[0].payload, "old-generation");
        assert!(ctrl.poll_notification(1).unwrap().is_none());
    }
    struct DeferredPrompt {
        sender: tokio::sync::mpsc::UnboundedSender<(
            aw_agent_kernel::hook_contract::Continuation,
            crate::HookEvent,
        )>,
    }
    impl crate::Extension for DeferredPrompt {
        fn name(&self) -> &str {
            "runtime-owned-defer"
        }
        fn wants_stage(&self, stage: aw_agent_kernel::hook_contract::Stage) -> bool {
            stage == aw_agent_kernel::hook_contract::Stage::Prompt
        }
        fn on_hook<'a>(
            &'a self,
            ctx: crate::HookContext<'a>,
            event: crate::HookEvent,
        ) -> crate::HookFuture<'a> {
            Box::pin(async move {
                let token = ctx.continuation.ok_or(Error::Denied(
                    "Agent without owning Runtime cannot issue a defer",
                ))?;
                self.sender
                    .send((token, event))
                    .map_err(|_| Error::Denied("hook callback receiver dropped"))?;
                Ok(aw_agent_kernel::hook_contract::HookFlow::Defer(token))
            })
        }
    }
    #[tokio::test]
    async fn runtime_owned_defer_resolves_once_and_cannot_be_forged_or_replayed() {
        use aw_agent_kernel::hook_contract::HookFlow;
        let (ctrl, actor) = runtime_with_scope(false);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut agent = Agent::builder(
            Arc::new(EchoProvider(calls.clone())),
            crate::AgentOptions::new("p", "m"),
        )
        .extension(DeferredPrompt { sender: tx })
        .build()
        .unwrap();
        ctrl.send(input(actor, 1, 0, "deferred-prompt"), 1).unwrap();
        let resolving = ctrl.clone();
        let action = async move {
            let (token, event) = rx.recv().await.expect("hook must defer");
            assert_eq!(token.owner, actor);
            let mut forged = token;
            forged.id = std::num::NonZeroU64::new(token.id.get() + 1).unwrap();
            assert!(resolving
                .resolve_hook(forged, HookFlow::Next(Some(event.clone())))
                .is_err());
            assert!(resolving
                .resolve_hook(token, HookFlow::Next(Some(event.clone())))
                .is_ok());
            assert!(resolving
                .resolve_hook(token, HookFlow::Next(Some(event)))
                .is_err());
        };
        let (settled, ()) = timeout(Duration::from_secs(3), async {
            tokio::join!(ctrl.run_once(&mut agent, |_| Ok(())), action)
        })
        .await
        .unwrap();
        assert_eq!(settled.unwrap().unwrap().answer, "deferred-prompt");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            ctrl.with_runtime(|r| Ok(r.pending_agent_hook_count()))
                .unwrap(),
            0
        );
    }
    #[tokio::test]
    async fn cancelling_deferred_prompt_releases_token_and_never_calls_provider() {
        let (ctrl, actor) = runtime_with_scope(false);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut agent = Agent::builder(
            Arc::new(EchoProvider(calls.clone())),
            crate::AgentOptions::new("p", "m"),
        )
        .extension(DeferredPrompt { sender: tx })
        .build()
        .unwrap();
        ctrl.send(input(actor, 1, 0, "cancel-defer"), 1).unwrap();
        let ending = ctrl.clone();
        let action = async move {
            let (token, event) = rx.recv().await.unwrap();
            assert!(matches!(
                ending.send(control(actor, 2, ControlKind::Cancel(Scope::Turn)), 1),
                Ok(Delivery::Applied(_))
            ));
            tokio::task::yield_now().await;
            assert!(ending
                .resolve_hook(
                    token,
                    aw_agent_kernel::hook_contract::HookFlow::Next(Some(event))
                )
                .is_err());
        };
        let (settled, ()) = timeout(Duration::from_secs(3), async {
            tokio::join!(ctrl.run_once(&mut agent, |_| Ok(())), action)
        })
        .await
        .unwrap();
        // Cancellation before a journal TurnStart is not a completed Turn.
        // The claimed in-memory Input remains uncertain until host inspection.
        assert!(matches!(settled, Err(Error::Cancelled)));
        assert!(ctrl.active().unwrap().is_some());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            ctrl.with_runtime(|r| Ok(r.pending_agent_hook_count()))
                .unwrap(),
            0
        );
    }
}
