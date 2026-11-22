//! Host composition for root/default Agent lifecycle.
//!
//! This owns no new lifecycle state. Durable Agent state remains in
//! `SessionStore`, current-Turn cancellation remains in `actor_owner`, finite
//! work remains in `JobRuntime`, and live routing remains in `agent_jobs`.

use crate::{actor_owner, agent_jobs, Error, PrincipalId, Result};
use aw_agent_core::{
    Agent, AgentExecutionBinding, AgentOptions, CancellationToken, InvocationScope,
    ProviderSelection, ToolEffect,
};
use aw_store::{AgentLifecycle, AgentSnapshot, QueueState, SessionStore};
use aw_store_api::SessionBackend;
use aw_tools::execution_ref::ExecutionRef;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};

#[derive(Clone)]
pub struct RootAgentClose {
    store: SessionStore,
    actor: PrincipalId,
    canonical_ref: String,
    owner: actor_owner::Service,
    jobs: Option<Arc<aw_tools_adapter::job_runtime::JobRuntime>>,
    router: Arc<agent_jobs::Router>,
}

impl RootAgentClose {
    pub fn new(
        store: SessionStore,
        actor: PrincipalId,
        canonical_ref: impl Into<String>,
        owner: actor_owner::Service,
        jobs: Option<Arc<aw_tools_adapter::job_runtime::JobRuntime>>,
        router: Arc<agent_jobs::Router>,
    ) -> Result<Self> {
        let canonical_ref = canonical_ref.into();
        let (bound_actor, session, branch, _, _) = owner.bound_peer_scope();
        if bound_actor != &actor {
            return Err(Error::Denied(
                "Agent close owner principal differs from durable Agent owner",
            ));
        }
        let binding = store.agent_binding(&actor, session, branch)?;
        let expected = format!(
            "/d:{}/i:{}/p:{}/a:{}",
            binding.domain_handle,
            binding.identity_handle,
            binding.project_handle,
            binding.agent_handle
        );
        if canonical_ref != expected {
            return Err(Error::Denied(
                "Agent close canonical ref differs from live owner binding",
            ));
        }
        Ok(Self {
            store,
            actor,
            canonical_ref,
            owner,
            jobs,
            router,
        })
    }

    pub fn canonical_ref(&self) -> &str {
        &self.canonical_ref
    }

    fn retire_pending_inputs(&self) -> Result<()> {
        let (_, session, _, _, _) = self.owner.bound_peer_scope();
        for fact in self.owner.queue_list()? {
            if fact.state != QueueState::Pending {
                continue;
            }
            let request_id = fact.request.request_id.clone();
            self.owner
                .operate(actor_owner::ActorOperation::DeleteQueue {
                    request_id: request_id.clone(),
                })?;
            if request_id.starts_with("jc_") {
                // Same conservative budget release used by stable-child close.
                // Queue custody is already gone, so failure can only leak
                // delegation capacity; it cannot resurrect runnable work.
                let _ =
                    self.store
                        .cancel_delegation_continuation(&self.actor, session, &request_id);
            }
        }
        Ok(())
    }

    pub async fn close(&self, expected_rev: Option<u64>) -> Result<AgentSnapshot> {
        let closing =
            self.store
                .agent_request_close(&self.actor, &self.canonical_ref, expected_rev)?;
        self.retire_pending_inputs()?;

        let turn_cancel = self
            .owner
            .cancel_active_turn_for_agent_close(closing.rev)
            .map(|_| ());
        let (_, session, branch, _, _) = self.owner.bound_peer_scope();
        let finite_close = match &self.jobs {
            Some(runtime) => {
                runtime
                    .close_owner(Some(&InvocationScope {
                        session_id: session.to_owned(),
                        branch_id: branch.to_owned(),
                    }))
                    .await
            }
            None => Ok(()),
        };

        // Request both independent cancellations before surfacing either
        // failure. A partial close stays durably `closing` and is retryable.
        turn_cancel?;
        finite_close?;
        self.owner.wait_turn_quiescent().await?;

        let closed = self
            .store
            .agent_finish_close(&self.actor, &self.canonical_ref)?;
        if closed.lifecycle != AgentLifecycle::Closed {
            return Err(Error::Conflict(
                "Agent terminal close returned a non-closed lifecycle",
            ));
        }
        self.router.clear_live_owner(&self.canonical_ref)?;
        Ok(closed)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum LiveAgentCommand {
    Wake,
    Close,
}

pub struct LiveAgentControl {
    closing: AtomicBool,
    close_queued: AtomicBool,
    active_cancel: Mutex<Option<CancellationToken>>,
}

impl LiveAgentControl {
    pub fn new() -> Self {
        Self {
            closing: AtomicBool::new(false),
            close_queued: AtomicBool::new(false),
            active_cancel: Mutex::new(None),
        }
    }

    pub fn is_closing(&self) -> bool {
        self.closing.load(Ordering::Acquire)
    }

    pub fn begin_turn(self: &Arc<Self>, token: CancellationToken) -> Result<LiveAgentTurnGuard> {
        let mut active = self
            .active_cancel
            .lock()
            .map_err(|_| Error::Storage("live Agent cancellation state poisoned"))?;
        if active.is_some() {
            return Err(Error::Conflict(
                "live Agent cannot run more than one Turn at once",
            ));
        }
        *active = Some(token.clone());
        if self.is_closing() {
            token.cancel();
        }
        Ok(LiveAgentTurnGuard {
            control: self.clone(),
        })
    }

    pub fn request_close(&self) -> Result<bool> {
        self.closing.store(true, Ordering::Release);
        if let Some(token) = self
            .active_cancel
            .lock()
            .map_err(|_| Error::Storage("live Agent cancellation state poisoned"))?
            .as_ref()
        {
            token.cancel();
        }
        Ok(!self.close_queued.swap(true, Ordering::AcqRel))
    }

    pub fn finish_close_attempt(&self) {
        self.close_queued.store(false, Ordering::Release);
    }
}

impl Default for LiveAgentControl {
    fn default() -> Self {
        Self::new()
    }
}

pub struct LiveAgentTurnGuard {
    control: Arc<LiveAgentControl>,
}

impl Drop for LiveAgentTurnGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = self.control.active_cancel.lock() {
            *active = None;
        }
    }
}

#[derive(Clone)]
pub struct LiveAgentWakeHandle {
    sender: mpsc::Sender<LiveAgentCommand>,
    control: Arc<LiveAgentControl>,
}

impl agent_jobs::LiveAgentWake for LiveAgentWakeHandle {
    fn wake(&self) -> Result<()> {
        if self.control.is_closing() {
            return Err(Error::Conflict(
                "Agent is closing and accepts no future input",
            ));
        }
        self.sender
            .send(LiveAgentCommand::Wake)
            .map_err(|_| Error::Conflict("live Agent worker is unavailable"))
    }

    fn close(&self) -> Result<()> {
        if self.control.request_close()? && self.sender.send(LiveAgentCommand::Close).is_err() {
            self.control.finish_close_attempt();
            return Err(Error::Conflict("live Agent worker is unavailable"));
        }
        Ok(())
    }

    fn is_closing(&self) -> bool {
        self.control.is_closing()
    }
}

pub fn live_agent_channel() -> (
    LiveAgentWakeHandle,
    mpsc::Receiver<LiveAgentCommand>,
    Arc<LiveAgentControl>,
) {
    let (sender, receiver) = mpsc::channel();
    let control = Arc::new(LiveAgentControl::new());
    (
        LiveAgentWakeHandle {
            sender,
            control: control.clone(),
        },
        receiver,
        control,
    )
}

#[derive(Clone)]
pub struct ActivatedHostAgent {
    canonical_ref: String,
    wake: LiveAgentWakeHandle,
}

impl ActivatedHostAgent {
    pub fn canonical_ref(&self) -> &str {
        &self.canonical_ref
    }

    pub fn close(&self) -> Result<()> {
        agent_jobs::LiveAgentWake::close(&self.wake)
    }
}

/// Reconstructible post-publication runtime activation for a host-created Agent.
///
/// This deliberately accepts only an already-published canonical Agent root.
/// It does not construct or publish Agents and therefore cannot bypass the
/// prepared-publication requirements for task-bearing agent.create.
#[derive(Clone)]
pub struct HostAgentActivation {
    store: SessionStore,
    actor: PrincipalId,
    selection: ProviderSelection,
    options: AgentOptions,
    tools: aw_tools_adapter::LocalToolOptions,
    router: Arc<agent_jobs::Router>,
}

impl HostAgentActivation {
    pub fn new(
        store: SessionStore,
        actor: PrincipalId,
        selection: ProviderSelection,
        options: AgentOptions,
        tools: aw_tools_adapter::LocalToolOptions,
        router: Arc<agent_jobs::Router>,
    ) -> Result<Self> {
        if options.profile != selection.identity().account_id
            || options.model != selection.identity().model
        {
            return Err(Error::Denied(
                "host Agent options must match the trusted provider selection",
            ));
        }
        Ok(Self {
            store,
            actor,
            selection,
            options,
            tools,
            router,
        })
    }

    fn binding_for(&self, canonical_ref: &str) -> Result<aw_store_api::AgentBindingInfo> {
        let parsed = ExecutionRef::parse(canonical_ref)
            .map_err(|_| Error::Denied("invalid canonical Agent ref"))?;
        let resolved = parsed
            .resolve(None, None)
            .map_err(|_| Error::Denied("invalid canonical Agent ref"))?;
        if !resolved.suffix().is_empty() {
            return Err(Error::Denied(
                "host Agent activation requires an Agent root ref",
            ));
        }
        let agent = resolved.agent_ref();
        self.store.agent_binding_by_handles(
            &self.actor,
            agent.domain.as_str(),
            agent.identity.as_str(),
            agent.project.as_str(),
            agent.agent.as_str(),
        )
    }

    pub fn activate(&self, canonical_ref: &str) -> Result<ActivatedHostAgent> {
        let binding = self.binding_for(canonical_ref)?;
        let session = self.store.session(&self.actor, &binding.session_id)?;

        let mut options = self.options.clone();
        options.system_prompt = session.binding.system_prompt.clone();
        let mut tools = self.tools.clone();
        tools.root = session.binding.tool_root.clone();
        tools.agent_jobs.replace(self.router.clone())?;
        tools.allow_wake = true;
        if self.selection.supports_tools()
            && !tools
                .selected_tools
                .iter()
                .any(|name| name == "jobs_subscribe")
            && (tools.mode == aw_tools_adapter::ToolMode::Off || !tools.selected_tools.is_empty())
        {
            tools.selected_tools.push("jobs_subscribe".into());
        }

        let builder = Agent::builder_from_selection(self.selection.clone(), options);
        let configured =
            aw_tools_adapter::configure_with_runtime(builder, self.actor.clone(), &tools)?;
        let job_runtime = configured.job_runtime;
        let mut agent = configured.builder.build()?;
        let (view, journal) =
            self.store
                .activate(&self.actor, &binding.session_id, &binding.branch_id)?;
        agent.load_history(view.history, Some(Arc::new(journal)))?;
        agent.bind_execution_owner(AgentExecutionBinding::from_trusted_host(
            canonical_ref.to_owned(),
            binding.session_id.clone(),
            binding.branch_id.clone(),
        )?)?;
        agent.permit_host_capability(self.actor.clone(), "jobs_subscribe", &ToolEffect::Control)?;

        let owner = actor_owner::Service::attach_for_agent(
            self.store.clone(),
            &binding.session_id,
            &binding.branch_id,
            &agent,
        )?;
        let close = Arc::new(RootAgentClose::new(
            self.store.clone(),
            self.actor.clone(),
            canonical_ref.to_owned(),
            owner.clone(),
            job_runtime,
            self.router.clone(),
        )?);
        let (wake, receiver, control) = live_agent_channel();
        let registered_wake: Arc<dyn agent_jobs::LiveAgentWake> = Arc::new(wake.clone());
        self.router.register_live_owner_with_wake(
            canonical_ref,
            owner.clone(),
            Some(registered_wake),
            agent.tool_binding_authority(),
        )?;

        let canonical = canonical_ref.to_owned();
        let router = self.router.clone();
        let selection = self.selection.clone();
        let agent_handle = binding.agent_handle;
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let spawn = std::thread::Builder::new()
            .name(format!(
                "aw-host-agent-{}",
                agent_handle.chars().rev().take(8).collect::<String>()
            ))
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .max_blocking_threads(4)
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.to_string()));
                        let _ = router.clear_live_owner(&canonical);
                        return;
                    }
                };
                if ready_tx.send(Ok(())).is_err() {
                    let _ = router.clear_live_owner(&canonical);
                    return;
                }
                loop {
                    match receiver.recv() {
                        Ok(LiveAgentCommand::Wake) => {
                            if control.is_closing() {
                                continue;
                            }
                            if !matches!(owner.admit_next_queued(&agent), Ok(Some(_))) {
                                continue;
                            }
                            let token = CancellationToken::new();
                            let guard = match control.begin_turn(token.clone()) {
                                Ok(guard) => guard,
                                Err(_) => continue,
                            };
                            let _ = runtime.block_on(owner.run_once_selected_with_cancel(
                                &mut agent,
                                &selection,
                                token,
                                |_| Ok(()),
                            ));
                            drop(guard);
                            let _ = runtime.block_on(
                                aw_tools_adapter::agent_jobs::AgentJobRouter::deliver_source_events(
                                    router.as_ref(),
                                    &canonical,
                                ),
                            );
                        }
                        Ok(LiveAgentCommand::Close) => {
                            let result = runtime.block_on(close.close(None));
                            if result.is_ok() {
                                break;
                            }
                            control.finish_close_attempt();
                        }
                        Err(_) => break,
                    }
                }
            });
        if let Err(error) = spawn {
            let _ = self.router.clear_live_owner(canonical_ref);
            return Err(Error::Io(error));
        }
        match ready_rx.recv() {
            Ok(Ok(())) => {
                agent_jobs::LiveAgentWake::wake(&wake)?;
                Ok(ActivatedHostAgent {
                    canonical_ref: canonical_ref.to_owned(),
                    wake,
                })
            }
            Ok(Err(detail)) => Err(Error::StorageDetail(detail)),
            Err(_) => Err(Error::Storage(
                "host Agent worker ended before runtime readiness",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccountDescriptor, Agent, AgentExecutionBinding, AgentOptions};
    use aw_agent_core::{
        provider::{CompletionFuture, DeltaSink, Provider},
        CompletionMeta, Message, Usage,
    };
    use aw_store::{
        AgentCreateContext, AgentCreateRequest, SessionBinding as StoredSessionBinding,
    };
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    struct BlockingProvider {
        entered: Arc<tokio::sync::Notify>,
    }

    struct CompletingProvider {
        calls: Arc<AtomicUsize>,
    }

    impl Provider for CompletingProvider {
        fn stream<'a>(
            &'a self,
            messages: &'a [Message],
            _model: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                assert!(messages
                    .iter()
                    .any(|message| message.content == "perform the initial task"));
                self.calls.fetch_add(1, Ordering::SeqCst);
                sink("initial task complete")?;
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

    impl Provider for BlockingProvider {
        fn supports_tools(&self) -> bool {
            true
        }

        fn stream_tools<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            _tools: &'a [aw_agent_core::ToolSpec],
            _sink: &'a mut DeltaSink<'_>,
        ) -> aw_agent_core::provider::ModelFuture<'a> {
            Box::pin(async move {
                self.entered.notify_one();
                std::future::pending::<()>().await;
                unreachable!()
            })
        }

        fn stream<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            _sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                self.entered.notify_one();
                std::future::pending::<()>().await;
                unreachable!()
            })
        }
    }

    #[tokio::test]
    async fn root_close_composes_turn_finite_work_and_terminal_store_once() {
        let actor = PrincipalId::new("local").unwrap();
        let store = SessionStore::in_memory().unwrap();
        let root = std::env::temp_dir();
        let (session, branch) = store
            .create(
                &actor,
                "root close",
                StoredSessionBinding {
                    working_directory: root.clone(),
                    tool_root: root.clone(),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();

        let entered = Arc::new(tokio::sync::Notify::new());
        let builder = Agent::builder(
            Arc::new(BlockingProvider {
                entered: entered.clone(),
            }),
            AgentOptions::new("fixture", "model"),
        );
        let mut tools = aw_tools_adapter::LocalToolOptions::new(
            PathBuf::from(&root),
            aw_tools_adapter::ToolMode::ReadOnly,
        );
        tools.selected_tools = vec!["read".into(), "batch".into()];
        let configured =
            aw_tools_adapter::configure_with_runtime(builder, actor.clone(), &tools).unwrap();
        let jobs = configured.job_runtime.clone().unwrap();
        let mut agent = configured.builder.build().unwrap();
        let (view, journal) = store.activate(&actor, &session.id, &branch.id).unwrap();
        agent
            .load_history(view.history, Some(Arc::new(journal)))
            .unwrap();
        let binding = store
            .agent_binding(&actor, &session.id, &branch.id)
            .unwrap();
        let canonical = format!(
            "/d:{}/i:{}/p:{}/a:{}",
            binding.domain_handle,
            binding.identity_handle,
            binding.project_handle,
            binding.agent_handle
        );
        agent
            .bind_execution_owner(
                AgentExecutionBinding::from_trusted_host(
                    canonical.clone(),
                    binding.session_id,
                    binding.branch_id,
                )
                .unwrap(),
            )
            .unwrap();

        let owner =
            actor_owner::Service::attach_for_agent(store.clone(), &session.id, &branch.id, &agent)
                .unwrap();
        let router = Arc::new(agent_jobs::Router::new(store.clone(), actor.clone(), 8).unwrap());
        router
            .register_live_owner(&canonical, owner.clone(), agent.tool_binding_authority())
            .unwrap();
        let close = RootAgentClose::new(
            store.clone(),
            actor.clone(),
            canonical.clone(),
            owner.clone(),
            Some(jobs),
            router,
        )
        .unwrap();

        owner.submit_bound_text(&agent, "close me").unwrap();
        let run_owner = owner.clone();
        let task = tokio::spawn(async move {
            run_owner
                .run_once(&mut agent, |_| Ok(()))
                .await
                .unwrap()
                .unwrap()
        });
        tokio::time::timeout(std::time::Duration::from_secs(3), entered.notified())
            .await
            .expect("provider must enter before root close");

        let closed = tokio::time::timeout(std::time::Duration::from_secs(3), close.close(None))
            .await
            .expect("root close must reach factual terminal state")
            .unwrap();
        assert_eq!(closed.lifecycle, AgentLifecycle::Closed);
        assert_eq!(closed.admission, aw_store::AgentAdmission::Held);
        let turn = task.await.unwrap();
        assert_eq!(turn.outcome, aw_agent_core::Outcome::Cancelled);

        let replay = close.close(None).await.unwrap();
        assert_eq!(replay.lifecycle, AgentLifecycle::Closed);
        assert_eq!(replay.rev, closed.rev);
    }

    #[tokio::test]
    async fn task_bearing_host_create_activates_and_dispatches_committed_initial_input_once() {
        let actor = PrincipalId::new("internal:fixture:host-activation").unwrap();
        let state_dir = crate::test_support::temp_dir("host-task-activation");
        aw_platform::ensure_private_dir(&state_dir).unwrap();
        let store = SessionStore::open(&state_dir).unwrap();
        let binding = StoredSessionBinding {
            working_directory: state_dir.clone(),
            tool_root: state_dir.clone(),
            system_prompt: "system".into(),
            profile_hint: "fixture".into(),
            model_hint: "model".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (root, root_branch) = store.create(&actor, "root", binding.clone()).unwrap();
        let root_binding = store
            .agent_binding(&actor, &root.id, &root_branch.id)
            .unwrap();
        let root_ref = format!(
            "/d:{}/i:{}/p:{}/a:{}",
            root_binding.domain_handle,
            root_binding.identity_handle,
            root_binding.project_handle,
            root_binding.agent_handle
        );
        let project_ref = format!(
            "/d:{}/i:{}/p:{}",
            root_binding.domain_handle, root_binding.identity_handle, root_binding.project_handle
        );
        let child_owner =
            aw_store::stable_user_principal(&store.current_user(&actor).unwrap().user_id).unwrap();
        let created = store
            .agent_create(
                &actor,
                &AgentCreateContext {
                    current_project_ref: project_ref,
                    current_agent_ref: Some(root_ref),
                    binding,
                    request_id: Some("host-activation-task-1".into()),
                    execution_site: "local".into(),
                },
                AgentCreateRequest {
                    title: Some("activated child".into()),
                    task: Some("perform the initial task".into()),
                    ..Default::default()
                },
            )
            .unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let seed = Agent::builder(
            Arc::new(CompletingProvider {
                calls: calls.clone(),
            }),
            AgentOptions::new("fixture", "model"),
        )
        .account(
            AccountDescriptor::personal("fixture", child_owner.clone(), "local", ["model".into()]),
            child_owner.clone(),
            "local",
        )
        .build()
        .unwrap();
        let selection = seed
            .provider_accounts()
            .select(&child_owner, "fixture", "model")
            .unwrap();
        let router =
            Arc::new(agent_jobs::Router::new(store.clone(), child_owner.clone(), 8).unwrap());
        let activation = HostAgentActivation::new(
            store.clone(),
            child_owner.clone(),
            selection,
            AgentOptions::new("fixture", "model"),
            aw_tools_adapter::LocalToolOptions::new(
                state_dir.clone(),
                aw_tools_adapter::ToolMode::Off,
            ),
            router,
        )
        .unwrap();
        let live = activation.activate(&created.canonical_ref).unwrap();
        assert_eq!(live.canonical_ref(), created.canonical_ref);

        let settled = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let snapshot = store.agent_get(&actor, &created.canonical_ref).unwrap();
                if snapshot.history_head != created.history_head && snapshot.active_turn.is_none() {
                    break snapshot;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("activated host Agent must dispatch its committed initial task");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(settled.lifecycle, AgentLifecycle::Open);

        live.close().unwrap();

        let closed = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let snapshot = store.agent_get(&actor, &created.canonical_ref).unwrap();
                if snapshot.lifecycle == AgentLifecycle::Closed {
                    break snapshot;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("activated host Agent close must reach authoritative terminal state");
        assert_eq!(closed.admission, aw_store::AgentAdmission::Held);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(state_dir);
    }
}
