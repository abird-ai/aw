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
use aw_store::{
    AgentCreateContext, AgentCreateRequest, AgentLifecycle, AgentSnapshot, QueueState,
    SessionBinding, SessionStore,
};
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

#[derive(Clone)]
pub struct HostAgentClose {
    store: SessionStore,
    actor: PrincipalId,
    root: Arc<RootAgentClose>,
    router: Arc<agent_jobs::Router>,
}

impl HostAgentClose {
    pub fn new(
        store: SessionStore,
        actor: PrincipalId,
        root: Arc<RootAgentClose>,
        router: Arc<agent_jobs::Router>,
    ) -> Result<Self> {
        if root.actor != actor {
            return Err(Error::Denied(
                "host Agent close principal differs from root lifecycle owner",
            ));
        }
        Ok(Self {
            store,
            actor,
            root,
            router,
        })
    }

    pub fn root_ref(&self) -> &str {
        self.root.canonical_ref()
    }

    fn canonical_ref(&self, reference: &str) -> Result<String> {
        if reference == "." {
            return Ok(self.root.canonical_ref().to_owned());
        }
        let parsed =
            ExecutionRef::parse(reference).map_err(|_| Error::Denied("invalid Agent close ref"))?;
        if !parsed.is_canonical() {
            return Err(Error::Denied(
                "non-current Agent close requires a canonical Agent ref",
            ));
        }
        let resolved = parsed
            .resolve(None, None)
            .map_err(|_| Error::Denied("invalid Agent close ref"))?;
        if !resolved.suffix().is_empty() {
            return Err(Error::Denied("Agent close requires an Agent root ref"));
        }
        Ok(resolved.agent_ref().canonical())
    }

    pub async fn close(&self, reference: &str, expected_rev: Option<u64>) -> Result<AgentSnapshot> {
        let canonical = self.canonical_ref(reference)?;
        if canonical == self.root.canonical_ref() {
            return self.root.close(expected_rev).await;
        }

        let current = self
            .store
            .agent_close_preflight(&self.actor, &canonical, expected_rev)?;
        if current.lifecycle == AgentLifecycle::Closed {
            return Ok(current);
        }
        if !self.router.has_live_close_controller(&canonical)? {
            return Err(Error::Conflict(
                "non-current Agent has no live close controller",
            ));
        }

        let closing = self
            .store
            .agent_request_close(&self.actor, &canonical, expected_rev)?;
        match self.router.request_live_close(&canonical) {
            Ok(()) => Ok(closing),
            Err(error) => {
                let observed = self.store.agent_get(&self.actor, &canonical)?;
                if observed.lifecycle == AgentLifecycle::Closed {
                    Ok(observed)
                } else {
                    Err(match error {
                        Error::Conflict(_) | Error::Denied(_) => Error::Conflict(
                            "Agent close was requested, but its live close controller became unavailable",
                        ),
                        error => error,
                    })
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct HostAgentInputs {
    store: SessionStore,
    actor: PrincipalId,
    current_agent_ref: String,
    router: Arc<agent_jobs::Router>,
}

impl HostAgentInputs {
    pub fn new(
        store: SessionStore,
        actor: PrincipalId,
        current_agent_ref: impl Into<String>,
        router: Arc<agent_jobs::Router>,
    ) -> Result<Self> {
        let current_agent_ref = current_agent_ref.into();
        let current = store.agent_get(&actor, &current_agent_ref)?;
        if current.canonical_ref != current_agent_ref {
            return Err(Error::Denied(
                "host Agent input owner requires the exact current Agent ref",
            ));
        }
        Ok(Self {
            store,
            actor,
            current_agent_ref,
            router,
        })
    }

    fn canonical_ref(&self, reference: &str) -> Result<String> {
        if reference == "." {
            return Ok(self.current_agent_ref.clone());
        }
        let parsed =
            ExecutionRef::parse(reference).map_err(|_| Error::Denied("invalid Agent input ref"))?;
        if !parsed.is_canonical() {
            return Err(Error::Denied(
                "non-current Agent input operations require a canonical Agent ref",
            ));
        }
        let resolved = parsed
            .resolve(None, None)
            .map_err(|_| Error::Denied("invalid Agent input ref"))?;
        if !resolved.suffix().is_empty() {
            return Err(Error::Denied(
                "Agent input operations require an Agent root ref",
            ));
        }
        Ok(resolved.agent_ref().canonical())
    }

    pub fn list(&self, reference: &str) -> Result<aw_store::AgentInputPlanSnapshot> {
        let canonical = self.canonical_ref(reference)?;
        self.store.agent_inputs_list(&self.actor, &canonical)
    }

    pub fn add(
        &self,
        request_id: &str,
        reference: &str,
        items: &[aw_store::AgentInputAdd],
        expected_rev: Option<u64>,
    ) -> Result<aw_store::AgentInputPlanSnapshot> {
        let canonical = self.canonical_ref(reference)?;
        self.store
            .agent_inputs_add_host(&self.actor, &canonical, request_id, items, expected_rev)
    }

    pub fn edit(
        &self,
        request_id: &str,
        reference: &str,
        id: &str,
        edit: aw_store::AgentInputEdit,
        expected_rev: u64,
    ) -> Result<aw_store::AgentInputPlanSnapshot> {
        let canonical = self.canonical_ref(reference)?;
        self.store.agent_inputs_edit_host(
            &self.actor,
            &canonical,
            request_id,
            id,
            edit,
            expected_rev,
        )
    }

    pub fn delete(
        &self,
        request_id: &str,
        reference: &str,
        ids: &[String],
        expected_rev: u64,
    ) -> Result<aw_store::AgentInputPlanSnapshot> {
        let canonical = self.canonical_ref(reference)?;
        self.store
            .agent_inputs_delete_host(&self.actor, &canonical, request_id, ids, expected_rev)
    }

    pub fn send(
        &self,
        request_id: &str,
        reference: &str,
        ids: Option<&[String]>,
        expected_rev: u64,
    ) -> Result<aw_store::AgentInputPlanSnapshot> {
        let canonical = self.canonical_ref(reference)?;
        let prepared = match self.store.agent_inputs_prepare_send(
            &self.actor,
            &canonical,
            request_id,
            ids,
            expected_rev,
        )? {
            aw_store::AgentInputSendStart::Replay(snapshot) => return Ok(snapshot),
            aw_store::AgentInputSendStart::Prepared(prepared) => *prepared,
        };

        // Store authorization above intentionally precedes live-topology lookup.
        let live = self.router.host_live_owner(&canonical)?;
        let (owner, _session, _branch, site, epoch) = live.bound_peer_scope();
        let owner = owner.clone();
        let site = site.to_owned();
        let bound = self.store.agent_inputs_bind_send_runtime(
            &self.actor,
            prepared,
            owner,
            &site,
            epoch,
        )?;

        let segment = if let Some(control) = bound.control() {
            match live.stage_host_authority_update(
                control.turn,
                &control.request_id,
                control.boundary,
                control.text(),
            ) {
                Ok(segment) => Some(segment),
                Err(error) => {
                    let _ = self.store.agent_inputs_abort_send(
                        &bound,
                        false,
                        &format!("live Runtime rejected staged Send control: {error}"),
                    );
                    return Err(error);
                }
            }
        } else {
            None
        };

        match self
            .store
            .agent_inputs_commit_send(&self.actor, &bound, segment.as_ref())
        {
            Ok(snapshot) => {
                let _ = self.router.wake_live_if_supported(&canonical);
                Ok(snapshot)
            }
            Err(error) => {
                if let (Some(control), Some(segment)) = (bound.control(), segment.as_ref()) {
                    let withdrawn = live.withdraw_host_authority_update(
                        control.turn,
                        &control.request_id,
                        segment.fingerprint,
                    );
                    let uncertain = withdrawn.is_err();
                    let _ = self.store.agent_inputs_abort_send(
                        &bound,
                        uncertain,
                        if uncertain {
                            "Send publication failed and live control withdrawal was uncertain"
                        } else {
                            "Send publication failed; live control was withdrawn"
                        },
                    );
                    if uncertain {
                        return Err(Error::Conflict(
                            "Agent input Send outcome is unknown; inspect before retry",
                        ));
                    }
                }
                Err(error)
            }
        }
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
    #[cfg(feature = "workspace")]
    source_host: Option<aw_host::source_git::GitNativeSourceHost>,
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
            #[cfg(feature = "workspace")]
            source_host: None,
        })
    }

    /// Explicit host-approved native source route for this Agent only.
    /// No persisted grant is promoted into live physical authority.
    #[cfg(feature = "workspace")]
    pub fn with_git_native_source_host(
        mut self,
        host: aw_host::source_git::GitNativeSourceHost,
    ) -> Self {
        self.source_host = Some(host);
        self
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

    fn configure_legacy(
        &self,
        builder: aw_agent_core::AgentBuilder,
        mut tools: aw_tools_adapter::LocalToolOptions,
    ) -> Result<aw_tools_adapter::ConfiguredLocalTools> {
        tools.agent_jobs.replace(self.router.clone())?;
        let event_slot = Arc::new(aw_tools_adapter::events::EventRouterSlot::default());
        event_slot.replace(self.router.clone())?;
        tools.events = event_slot;
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
        if self.selection.supports_tools()
            && (tools.mode == aw_tools_adapter::ToolMode::Off || !tools.selected_tools.is_empty())
        {
            for name in [
                aw_tools_adapter::events::EVENTS_PUBLISH,
                aw_tools_adapter::events::EVENTS_SUBSCRIBE,
            ] {
                if !tools.selected_tools.iter().any(|selected| selected == name) {
                    tools.selected_tools.push(name.into());
                }
            }
        }

        aw_tools_adapter::configure_with_runtime(builder, self.actor.clone(), &tools)
    }

    /// Returns Some only when this call installed the live runtime. Exact
    /// create replay returns None when the canonical Agent is already live.
    pub fn activate(&self, canonical_ref: &str) -> Result<Option<ActivatedHostAgent>> {
        self.activate_in(canonical_ref, None)
    }

    /// Explicit Manage-authorized opening of an existing source-bearing
    /// Agent, only after building its original native execution route.
    #[cfg(feature = "workspace")]
    pub fn open_and_activate_source(
        &self,
        canonical_ref: &str,
        manager: &PrincipalId,
        expected_rev: u64,
    ) -> Result<Option<ActivatedHostAgent>> {
        self.activate_in(canonical_ref, Some((manager, expected_rev)))
    }

    fn activate_in(
        &self,
        canonical_ref: &str,
        source_open: Option<(&PrincipalId, u64)>,
    ) -> Result<Option<ActivatedHostAgent>> {
        let binding = self.binding_for(canonical_ref)?;
        if self.router.has_live_owner(canonical_ref)? {
            if source_open.is_some() {
                return Err(Error::Conflict("source Agent already has a live owner"));
            }
            return Ok(None);
        }
        let execution = AgentExecutionBinding::from_trusted_host(
            canonical_ref.to_owned(),
            binding.session_id.clone(),
            binding.branch_id.clone(),
        )?;
        // Ordinary activation must still pass its source-free Store
        // gate before ambient tools are configured. The explicit native
        // route instead builds from read-only immutable Session metadata,
        // then activates its exact journal under the actual live policy.
        let native_source = {
            #[cfg(feature = "workspace")]
            {
                self.source_host.is_some()
            }
            #[cfg(not(feature = "workspace"))]
            {
                false
            }
        };
        if !native_source && source_open.is_some() {
            return Err(Error::Denied(
                "explicit source opening requires a native host",
            ));
        }
        let legacy = if native_source {
            None
        } else {
            Some(
                self.store
                    .activate(&self.actor, &binding.session_id, &binding.branch_id)?,
            )
        };
        let session = match &legacy {
            Some((view, _)) => view.session.clone(),
            None => self.store.session(&self.actor, &binding.session_id)?,
        };
        let mut options = self.options.clone();
        options.system_prompt = session.binding.system_prompt.clone();
        let mut tools = self.tools.clone();
        tools.root = session.binding.tool_root.clone();
        let builder = Agent::builder_from_selection(self.selection.clone(), options);
        #[cfg(feature = "workspace")]
        let (mut agent, job_runtime, source) = match &self.source_host {
            Some(host) => {
                let (agent, source) = host.build_execution(
                    &self.store,
                    self.actor.clone(),
                    execution.clone(),
                    builder,
                    &tools,
                )?;
                if let Some((manager, rev)) = source_open {
                    self.store.agent_open_with_source(manager, rev, &source)?;
                }
                (agent, None, Some(source))
            }
            None => {
                let configured = self.configure_legacy(builder, tools)?;
                (configured.builder.build()?, configured.job_runtime, None)
            }
        };
        #[cfg(not(feature = "workspace"))]
        let (mut agent, job_runtime, source): (_, _, Option<aw_store::SandboxExecution>) = {
            let configured = self.configure_legacy(builder, tools)?;
            (configured.builder.build()?, configured.job_runtime, None)
        };
        let (view, journal) = match source.as_ref() {
            Some(source) => self.store.activate_with_source(source)?,
            None => legacy.ok_or(Error::Storage("legacy Agent activation missing"))?,
        };
        agent.load_history(view.history, Some(Arc::new(journal)))?;
        agent.bind_execution_owner(execution)?;
        if !native_source {
            agent.permit_host_capability(
                self.actor.clone(),
                "jobs_subscribe",
                &ToolEffect::Control,
            )?;
        }

        let owner = match source {
            Some(source) => actor_owner::Service::attach_for_agent_with_source(
                self.store.clone(),
                &agent,
                source,
            )?,
            None => actor_owner::Service::attach_for_agent(
                self.store.clone(),
                &binding.session_id,
                &binding.branch_id,
                &agent,
            )?,
        };
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
        if !self.router.register_live_owner_with_wake_if_absent(
            canonical_ref,
            owner.clone(),
            Some(registered_wake),
            agent.tool_binding_authority(),
        )? {
            return Ok(None);
        }

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
                Ok(Some(ActivatedHostAgent {
                    canonical_ref: canonical_ref.to_owned(),
                    wake,
                }))
            }
            Ok(Err(detail)) => Err(Error::StorageDetail(detail)),
            Err(_) => Err(Error::Storage(
                "host Agent worker ended before runtime readiness",
            )),
        }
    }
}

#[derive(Clone)]
pub struct HostAgentCreator {
    store: SessionStore,
    caller: PrincipalId,
    current_agent_ref: String,
    current_project_ref: String,
    auto_sandbox_ref: String,
    binding: SessionBinding,
    execution_site: String,
    activation: HostAgentActivation,
}

impl HostAgentCreator {
    pub fn new(
        store: SessionStore,
        caller: PrincipalId,
        current_agent_ref: impl Into<String>,
        binding: SessionBinding,
        execution_site: impl Into<String>,
        activation: HostAgentActivation,
    ) -> Result<Self> {
        let current_agent_ref = current_agent_ref.into();
        let current = store.agent_get(&caller, &current_agent_ref)?;
        let auto_sandbox_ref = current.sandbox_ref.clone().ok_or(Error::Storage(
            "current Agent is missing its authoritative Sandbox binding",
        ))?;
        let stable_user = store.current_user(&caller)?;
        let canonical_owner = aw_store::stable_user_principal(&stable_user.user_id)?;
        if activation.actor != canonical_owner {
            return Err(Error::Denied(
                "host Agent activation principal is not the caller's canonical stable user",
            ));
        }
        let execution_site = execution_site.into();
        if execution_site.is_empty()
            || execution_site.len() > 128
            || !execution_site.is_ascii()
            || execution_site.chars().any(char::is_control)
        {
            return Err(Error::Denied("invalid trusted Agent execution site"));
        }
        Ok(Self {
            store,
            caller,
            current_agent_ref,
            current_project_ref: current.project_ref,
            auto_sandbox_ref,
            binding,
            execution_site,
            activation,
        })
    }

    pub fn create(&self, request_id: &str, request: AgentCreateRequest) -> Result<AgentSnapshot> {
        let snapshot = self.store.agent_create(
            &self.caller,
            &AgentCreateContext {
                current_project_ref: self.current_project_ref.clone(),
                current_agent_ref: Some(self.current_agent_ref.clone()),
                binding: self.binding.clone(),
                auto_sandbox_ref: Some(self.auto_sandbox_ref.clone()),
                request_id: Some(request_id.to_owned()),
                execution_site: self.execution_site.clone(),
            },
            request,
        )?;
        let _ = self.activation.activate(&snapshot.canonical_ref)?;
        Ok(snapshot)
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
        AgentCreateRequest, AgentInputAdd, AgentInputIntent, SessionBinding as StoredSessionBinding,
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

    #[cfg(all(feature = "workspace", unix))]
    #[tokio::test]
    async fn native_source_actor_admission_revalidates_original_live_owner() {
        use aw_agent_core::{
            journal::{TurnJournal, TurnStart},
            CheckpointPolicy, DriverKey, Role, SandboxDriverRegistry, SourceAccess,
            SourceDriverRegistry, SourceName, ToolCall,
        };
        use aw_git::source_driver::{GitSourceDriver, GitSourceLocation};
        use aw_host::{
            source_filesystem::NativeFilesystemDriver,
            source_git::{GitNativeSourceHost, GitNativeSourceResolver},
        };
        use aw_store::{AgentAdmission, AgentHistoryForkAccess, CheckpointRestoreRequest};
        use aw_tools::filesystem::private_artifact::{Kind, ObjectIdentity};
        use serde_json::json;
        use std::{
            fs,
            io::Write,
            os::unix::fs::{MetadataExt, OpenOptionsExt},
            path::Path,
            process::Command,
            time::Duration,
        };

        // Independent fixture approval of the exact original checkout and
        // artifact namespace. All fixture writers/old Store are gone.
        struct RecoveryFence {
            locations: Vec<GitSourceLocation>,
            _pins: Vec<aw_platform::DirectoryIdentity>,
        }
        impl aw_host::source_checkpoint::GitSourceFence for RecoveryFence {
            fn locations(&self) -> &[GitSourceLocation] {
                &self.locations
            }
        }

        const PROCESS_COMMAND: &str = "printf process-owned > process-created; printf process-stdout; printf process-stderr >&2";
        const RECOVERY_COMMAND: &str = "printf recovered-process > process-recovery; printf process-stdout; printf process-stderr >&2";
        const MIXED_COMMAND: &str =
            "printf mixed-process > mixed-process-created; printf mixed-stdout";

        // A reviewed fixed builtin-only fixture, NOT a general shell
        // containment driver. It never interprets arbitrary model text.
        struct FixedProcessDriver {
            approval: aw_host::source_approval::NativeSourceApproval,
            bash: PathBuf,
        }
        impl aw_host::source_process::SourceProcessDriver for FixedProcessDriver {
            type Pins = aw_platform::DirectoryIdentity;
            type Witness = aw_host::source_approval::NativeSourceApprovalGuard;
            type Fence = Self::Witness;
            fn operation(&self) -> aw_tools::OperationId {
                aw_tools::OperationId::Bash
            }
            fn prepare(
                &self,
                plan: &aw_store::SandboxEffectPlan,
                arguments: &serde_json::Value,
            ) -> Result<(
                aw_tools_adapter::process::PreparedPipeLaunch<Self::Pins>,
                Self::Witness,
            )> {
                let command_text = if arguments == &json!({"command":PROCESS_COMMAND}) {
                    PROCESS_COMMAND
                } else if arguments == &json!({"command":RECOVERY_COMMAND}) {
                    RECOVERY_COMMAND
                } else if arguments == &json!({"command":MIXED_COMMAND}) {
                    MIXED_COMMAND
                } else {
                    return Err(Error::Denied(
                        "fixture driver admits only reviewed fixed commands",
                    ));
                };
                let guard = self.approval.read()?;
                assert_eq!(plan.grant.sources.len(), 1);
                assert_eq!(guard.roots().len(), 1);
                assert_eq!(plan.grant.sources[0], guard.roots()[0].source);
                assert_eq!(plan.sandbox.tool_root, guard.base());
                let pin = aw_platform::DirectoryIdentity::open(guard.base())?;
                assert_eq!(
                    pin.identity()?,
                    (
                        guard.roots()[0].physical.volume,
                        guard.roots()[0].physical.object
                    )
                );
                let mut command = Command::new(&self.bash);
                command.env_clear().current_dir(guard.base()).args([
                    "--noprofile",
                    "--norc",
                    "-c",
                    command_text,
                ]);
                Ok((
                    aw_tools_adapter::process::PreparedPipeLaunch::from_trusted_command(
                        command, false, pin,
                    ),
                    guard,
                ))
            }
            fn prove_quiescent(
                &self,
                witness: Self::Witness,
                terminal: &aw_tools::filesystem::runner::ProcessTerminal,
            ) -> Result<Self::Fence> {
                // This fixed builtin-only command has no external or
                // descendant execution. The original Process has been reaped.
                assert_eq!(
                    terminal.exit,
                    aw_tools::filesystem::runner::ProcessExit::Code(0)
                );
                Ok(witness)
            }
        }

        // Fault only the actual original node terminal persist callback,
        // after the real native worker and physical proof completed.
        // No manufactured ToolStarted/ToolFinished or source proof.
        struct LostNodeSettlement<R>(Arc<R>);
        impl<R: aw_tools_adapter::operation_router::OperationRouter>
            aw_tools_adapter::operation_router::OperationRouter for LostNodeSettlement<R>
        {
            fn claim_route(
                &self,
                operation: aw_tools::OperationId,
            ) -> Option<aw_agent_core::tool::ToolClaimRoute> {
                self.0.claim_route(operation)
            }
            fn execute<'a>(
                &'a self,
                operation: aw_tools::OperationId,
                ctx: aw_agent_core::ToolContext,
                arguments: &'a serde_json::Value,
                original: Arc<dyn aw_agent_core::Tool>,
            ) -> aw_agent_core::tool::ToolFuture<'a> {
                self.0.execute(operation, ctx, arguments, original)
            }
        }
        impl<R: aw_tools_adapter::native_filesystem::SourceGraphExecutor>
            aw_tools_adapter::native_filesystem::SourceGraphExecutor for LostNodeSettlement<R>
        {
            fn execute_node<'a>(
                &'a self,
                request: aw_tools_adapter::native_filesystem::SourceGraphNodeRequest<'a>,
            ) -> aw_tools_adapter::native_filesystem::SourceGraphNodeFuture<'a> {
                self.0.execute_node(request)
            }
            fn settle_node(
                &self,
                ctx: &aw_agent_core::ToolContext,
                node: &aw_tools::filesystem::job_manager::RunningNode,
                transition: aw_tools::filesystem::job::Transition,
            ) -> Result<()> {
                if node.binding().coordinate == 2 {
                    let mut foreign = transition;
                    foreign.node = Some(1);
                    assert!(self.0.settle_node(ctx, node, foreign).is_err());
                    return Err(Error::Protocol("fixture lost original node settlement"));
                }
                self.0.settle_node(ctx, node, transition)
            }
        }

        struct NativeProvider {
            calls: Arc<AtomicUsize>,
        }
        impl Provider for NativeProvider {
            fn supports_tools(&self) -> bool {
                true
            }
            fn stream_tools<'a>(
                &'a self,
                messages: &'a [Message],
                _model: &'a str,
                tools: &'a [aw_agent_core::ToolSpec],
                sink: &'a mut DeltaSink<'_>,
            ) -> aw_agent_core::provider::ModelFuture<'a> {
                Box::pin(async move {
                    self.calls.fetch_add(1, Ordering::SeqCst);
                    let user = messages.iter().rposition(|m| m.role == Role::User).unwrap();
                    let results: Vec<_> = messages[user + 1..]
                        .iter()
                        .filter(|m| m.role == Role::Tool)
                        .collect();
                    let prompt = messages[user].content.as_str();
                    if matches!(
                        prompt,
                        "perform source Process"
                            | "reject uncontained Process"
                            | "interrupt-process-before-claim"
                            | "interrupt-process-before-release"
                    ) {
                        assert_eq!(tools.len(), 1);
                        assert_eq!(tools[0].name, "bash");
                        assert!(tools[0].parameters["properties"]
                            .get("background")
                            .is_none());
                    } else if prompt.starts_with("interrupt-graph-") {
                        assert_eq!(
                            tools
                                .iter()
                                .map(|tool| tool.name.as_str())
                                .collect::<std::collections::BTreeSet<_>>(),
                            std::collections::BTreeSet::from(["batch", "read", "write"])
                        );
                        let batch = tools.iter().find(|tool| tool.name == "batch").unwrap();
                        assert!(batch.parameters["properties"].get("background").is_none());
                    } else if prompt == "interrupt-mixed-before-claim" {
                        assert_eq!(
                            tools
                                .iter()
                                .map(|tool| tool.name.as_str())
                                .collect::<std::collections::BTreeSet<_>>(),
                            std::collections::BTreeSet::from(["bash", "read", "write"])
                        );
                    } else {
                        assert_eq!(tools.len(), 2);
                    }
                    let call = match (prompt, results.len()) {
                        (
                            "interrupt-graph-before-claim"
                            | "interrupt-graph-after-settlement"
                            | "interrupt-graph-lost-node"
                            | "interrupt-graph-needs-inspection",
                            0,
                        ) => {
                            let path = match prompt {
                                "interrupt-graph-before-claim" => "graph-not-started",
                                "interrupt-graph-after-settlement" => "graph-created",
                                "interrupt-graph-lost-node" => "graph-unreleased",
                                _ => "graph-inspected",
                            };
                            Some(ToolCall {
                                id: "source-graph-fixture".into(),
                                name: "batch".into(),
                                arguments: json!({"ops":[
                                    ["read", {"path":"tracked"}],
                                    ["write", {"path":path,"content":"graph node"}],
                                    ["read", {"path":path}],
                                    ["write", {"path":"graph-skipped","content":"must not exist"}, "1.fail"]
                                ]}),
                            })
                        }
                        ("interrupt-mixed-before-claim", 0) => Some(ToolCall {
                            id: "mixed-read".into(),
                            name: "read".into(),
                            arguments: json!({"path":"tracked"}),
                        }),
                        ("interrupt-mixed-before-claim", 1) => {
                            assert!(results[0].content.contains("fixture data"));
                            Some(ToolCall {
                                id: "mixed-process".into(),
                                name: "bash".into(),
                                arguments: json!({"command":MIXED_COMMAND}),
                            })
                        }
                        ("interrupt-mixed-before-claim", 2) => {
                            assert!(
                                results[1].content.contains("mixed-stdout"),
                                "{:?}",
                                results[1]
                            );
                            Some(ToolCall {
                                id: "mixed-unclaimed".into(),
                                name: "write".into(),
                                arguments: json!({"path":"mixed-not-started","content":"must not exist"}),
                            })
                        }
                        ("perform source Process" | "reject uncontained Process", 0) => {
                            Some(ToolCall {
                                id: "source-process-fixture".into(),
                                name: "bash".into(),
                                arguments: json!({"command":PROCESS_COMMAND}),
                            })
                        }
                        (
                            "interrupt-process-before-claim" | "interrupt-process-before-release",
                            0,
                        ) => Some(ToolCall {
                            id: "source-process-recovery".into(),
                            name: "bash".into(),
                            arguments: json!({"command":RECOVERY_COMMAND}),
                        }),
                        ("reject uncontained Process", 1) => {
                            assert!(
                                results[0].content.contains(
                                    "required closed descendant containment is unavailable"
                                ),
                                "{:?}",
                                results[0]
                            );
                            sink("containment refused")?;
                            None
                        }
                        ("perform source Process", 1) => {
                            assert!(
                                results[0].content.contains("process-stdout"),
                                "{:?}",
                                results[0]
                            );
                            assert!(
                                results[0].content.contains("process-stderr"),
                                "{:?}",
                                results[0]
                            );
                            sink("process done")?;
                            None
                        }
                        ("interrupt-read-before-claim", 0) => Some(ToolCall {
                            id: "no-start-read".into(),
                            name: "read".into(),
                            arguments: json!({"path":"tracked"}),
                        }),
                        ("interrupt-write-before-claim", 0) => Some(ToolCall {
                            id: "no-start-write".into(),
                            name: "write".into(),
                            arguments: json!({"path":"not-started","content":"must not exist"}),
                        }),
                        (_, 0) => Some(ToolCall {
                            id: "read-fixture".into(),
                            name: "read".into(),
                            arguments: json!({"path":"tracked"}),
                        }),
                        (_, 1) => {
                            assert!(
                                results[0].content.contains("fixture data"),
                                "{:?}",
                                results[0]
                            );
                            Some(ToolCall {
                                id: "write-fixture".into(),
                                name: "write".into(),
                                arguments: json!({"path":"created","content":"native actor"}),
                            })
                        }
                        (_, 2) => {
                            sink("done")?;
                            None
                        }
                        _ => panic!("unexpected native tool round"),
                    };
                    Ok(aw_agent_core::provider::ModelReply {
                        meta: CompletionMeta {
                            finish_reason: if call.is_some() { "tool_calls" } else { "stop" }
                                .into(),
                            usage: Usage {
                                input_tokens: Some(3),
                                output_tokens: Some(2),
                            },
                        },
                        tool_calls: call.into_iter().collect(),
                        provider_context: None,
                    })
                })
            }
            fn stream<'a>(
                &'a self,
                _messages: &'a [Message],
                _model: &'a str,
                _sink: &'a mut DeltaSink<'_>,
            ) -> CompletionFuture<'a> {
                Box::pin(async { Err(Error::Denied("fixture requires native tools")) })
            }
        }

        struct Temp(PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let temp = Temp(crate::test_support::temp_dir("native-source-actor"));
        aw_platform::ensure_private_dir(&temp.0).unwrap();
        let state = temp.0.join("state");
        let repo = temp.0.join("repo");
        let initial = temp.0.join("initial");
        let approvals = temp.0.join("approvals");
        for path in [&state, &repo, &initial, &approvals] {
            aw_platform::ensure_private_dir(path).unwrap();
        }
        let executable = std::env::var_os("AW_GIT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/bin/git"));
        let git = |args: &[&str]| {
            let out = Command::new(&executable)
                .current_dir(&repo)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{:?}: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-q"]);
        git(&["config", "user.name", "Fixture"]);
        git(&["config", "user.email", "fixture@example.invalid"]);
        fs::write(repo.join("tracked"), b"fixture data").unwrap();
        git(&["add", "tracked"]);
        git(&["commit", "--no-gpg-sign", "-qm", "fixture"]);
        let index = fs::read(repo.join(".git/index")).unwrap();
        let registry = SourceDriverRegistry::new();
        let handle = GitSourceDriver::new(&executable)
            .unwrap()
            .register(&registry)
            .unwrap();
        let manager = PrincipalId::new(aw_platform::local_principal().unwrap()).unwrap();
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        let layout = |path: &Path| StoredSessionBinding {
            working_directory: path.to_owned(),
            tool_root: path.to_owned(),
            system_prompt: "fixture".into(),
            profile_hint: "fixture".into(),
            model_hint: "model".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (parent, branch) = store.create(&manager, "fixture", layout(&initial)).unwrap();
        let info = store
            .agent_binding(&manager, &parent.id, &branch.id)
            .unwrap();
        let parent_ref = format!(
            "/d:{}/i:{}/p:{}/a:{}",
            info.domain_handle, info.identity_handle, info.project_handle, info.agent_handle
        );
        let history = store
            .checkpoint_create_history_only(&manager, &parent_ref, None, "fixture:history")
            .unwrap();
        let name = SourceName::new("code").unwrap();
        let binding = store
            .source_driver(&handle)
            .unwrap()
            .binding(
                &repo,
                name.clone(),
                SourceAccess::ReadWrite,
                CheckpointPolicy::Required,
            )
            .unwrap();
        let restored = store
            .checkpoint_restore_baseline(
                &manager,
                &CheckpointRestoreRequest {
                    checkpoint: history.id,
                    project: None,
                    title: None,
                    access: AgentHistoryForkAccess::Private,
                    task: None,
                },
                &layout(&repo),
                &[binding],
                "fixture:baseline",
                "local",
            )
            .unwrap();
        let reference = restored.agent.canonical_ref;
        let execution = store.agent_execution_binding(&manager, &reference).unwrap();
        let session = execution.session_id().to_owned();
        let branch = execution.branch_id().to_owned();
        let owner = store.session(&manager, &session).unwrap().owner;
        let sandbox = store
            .sandbox_get(&manager, restored.agent.sandbox_ref.as_deref().unwrap())
            .unwrap();
        let mut config = json!({
            "version":1, "store_directory":state,
            "store_generation":store.local_process_info().unwrap().store_generation,
            "manager":manager, "actor":owner, "agent_ref":reference,
            "sandbox_ref":sandbox.canonical_ref, "sandbox_rev":sandbox.rev,
            "policy_ref":"host:fixture-native-actor", "base":repo,
            "directories":[{"name":name, "repository":repo,
                "repository_identity":aw_platform::DirectoryIdentity::open(&repo).unwrap().identity().unwrap(),
                "binding_rev":sandbox.sources[0].binding_rev,"access":"read_write"}]
        });
        aw_platform::open_private_new(&approvals.join("git-native.json"))
            .unwrap()
            .write_all(&serde_json::to_vec(&config).unwrap())
            .unwrap();
        let provision = |store: &SessionStore| {
            let resolver = GitNativeSourceResolver::new(store.clone(), handle.clone()).unwrap();
            let approval = resolver
                .provision_configured_approval(&manager, &approvals)
                .unwrap();
            let drivers = SandboxDriverRegistry::new();
            let native = drivers
                .register(
                    DriverKey::builtin("native-filesystem").unwrap(),
                    NativeFilesystemDriver,
                )
                .unwrap();
            GitNativeSourceHost::new(approval, handle.clone(), drivers, native).unwrap()
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let seed = Agent::builder(
            Arc::new(NativeProvider {
                calls: calls.clone(),
            }),
            AgentOptions::new("fixture", "model"),
        )
        .account(
            AccountDescriptor::personal("fixture", owner.clone(), "local", ["model".into()]),
            owner.clone(),
            "local",
        )
        .build()
        .unwrap();
        let selection = seed
            .provider_accounts()
            .select(&owner, "fixture", "model")
            .unwrap();
        let mut tools = aw_tools_adapter::LocalToolOptions::new(
            repo.clone(),
            aw_tools_adapter::ToolMode::Workspace,
        );
        tools.selected_tools = vec!["read".into(), "write".into()];
        let host = provision(&store);
        let build = |store: &SessionStore, host: &GitNativeSourceHost| {
            host.build_execution(
                store,
                owner.clone(),
                execution.clone(),
                Agent::builder_from_selection(
                    selection.clone(),
                    AgentOptions::new("fixture", "model"),
                ),
                &tools,
            )
        };
        assert!(build(&store, &host).is_err());
        let grant = host.publish_grant(&manager, 0).unwrap();
        let (mut agent, source) = build(&store, &host).unwrap();
        let held = store.agent_get(&manager, &reference).unwrap();
        assert_eq!(held.admission, AgentAdmission::Held);
        assert!(store.activate_with_source(&source).is_err());
        assert!(store
            .agent_admission(&manager, &reference, AgentAdmission::Open, Some(held.rev))
            .is_err());
        store
            .agent_open_with_source(&manager, held.rev, &source)
            .unwrap();
        assert!(store.activate(&owner, &session, &branch).is_err());
        let (view, journal) = store.activate_with_source(&source).unwrap();
        assert!(journal
            .acknowledge("blanket inspection is not source proof".into())
            .await
            .is_err());
        assert!(journal
            .begin(TurnStart {
                turn_id: view.history.next_turn_id,
                prompt: "unowned".into(),
                identity: selection.identity().clone()
            })
            .await
            .is_err());
        agent
            .load_history(view.history, Some(Arc::new(journal)))
            .unwrap();
        let actor = actor_owner::Service::attach_for_agent_with_source(
            store.clone(),
            &agent,
            source.clone(),
        )
        .unwrap();
        actor
            .submit_bound_text(&agent, "perform source task")
            .unwrap();
        let turn = actor
            .run_once(&mut agent, |_| Ok(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            turn.outcome,
            aw_agent_core::Outcome::Completed,
            "{:?}",
            turn
        );
        assert_eq!(turn.tool_records.len(), 2);
        assert!(store
            .reconcile_native_turn(&manager, &reference, turn.id)
            .is_err());
        assert_eq!(fs::read(repo.join("created")).unwrap(), b"native actor");
        assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        actor
            .enqueue_bound_text(
                &agent,
                "fixture:next",
                2,
                aw_store::QueueTarget::Auto,
                1,
                "second source task",
            )
            .unwrap();
        let held = store
            .agent_admission(&manager, &reference, AgentAdmission::Held, None)
            .unwrap();
        assert!(actor.admit_next_queued(&agent).is_err());
        store
            .agent_open_with_source(&manager, held.rev, &source)
            .unwrap();
        assert!(host.revoke().await);
        assert!(actor.admit_next_queued(&agent).is_err());
        assert!(store.activate_with_source(&source).is_err());
        assert_eq!(
            store.sandbox_grant_status(&manager, &reference).unwrap(),
            grant
        );
        assert!(store
            .actor_queue_list(&owner, &session)
            .unwrap()
            .iter()
            .all(|fact| fact.state == aw_store::QueueState::Pending));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        drop(actor);
        drop(agent);
        drop(source);
        drop(host);
        drop(store);

        // The fixture is the independent approving host. Every restart
        // uses a fresh decision over the exact committed Sandbox.
        let mut refresh = |store: &SessionStore| {
            let current = store.sandbox_get(&manager, &sandbox.canonical_ref).unwrap();
            assert!(current.rev > sandbox.rev);
            assert_eq!(current.sources, sandbox.sources);
            config["sandbox_rev"] = json!(current.rev);
            fs::write(
                approvals.join("git-native.json"),
                serde_json::to_vec(&config).unwrap(),
            )
            .unwrap();
            provision(store)
        };
        let mut grant_rev = grant.revision;
        for (tool, sequence) in [("read", 3), ("write", 4)] {
            // Same durable open intent and pending ciphertext, no live grant.
            let store =
                SessionStore::open_with_sources(&state, registry.clone(), Default::default())
                    .unwrap();
            assert!(store.activate(&owner, &session, &branch).is_err());
            let host = refresh(&store);
            assert!(build(&store, &host).is_err());
            grant_rev = host.publish_grant(&manager, grant_rev).unwrap().revision;
            let (mut agent, source) = build(&store, &host).unwrap();
            let (view, journal) = store.activate_with_source(&source).unwrap();
            let turn_id = view.history.next_turn_id;
            agent
                .load_history(view.history, Some(Arc::new(journal)))
                .unwrap();
            let actor = actor_owner::Service::attach_for_agent_with_source(
                store.clone(),
                &agent,
                source.clone(),
            )
            .unwrap();
            let request_id = format!("fixture:no-start-{tool}");
            actor
                .enqueue_bound_text(
                    &agent,
                    &request_id,
                    sequence,
                    aw_store::QueueTarget::Auto,
                    -1,
                    &format!("interrupt-{tool}-before-claim"),
                )
                .unwrap();
            assert_eq!(
                actor
                    .admit_next_queued(&agent)
                    .unwrap()
                    .unwrap()
                    .request
                    .request_id,
                request_id,
            );
            let before = store.sandbox_get(&manager, &sandbox.canonical_ref).unwrap();
            // Lose the actual Actor immediately after the real journal
            // acknowledgement, before the fixed native route is polled.
            // No fake journal row, driver receipt, or production fault hook.
            let task = tokio::spawn(async move {
                actor
                    .run_once(&mut agent, |event| {
                        if let aw_agent_core::AgentEvent::ToolStarted { record, .. } = event {
                            assert_eq!(record.call.name, tool);
                            panic!("fixture lost original Actor after intent");
                        }
                        Ok(())
                    })
                    .await
            });
            let panic = task
                .await
                .expect_err("fixture must lose Actor")
                .into_panic();
            assert_eq!(
                panic.downcast_ref::<&str>(),
                Some(&"fixture lost original Actor after intent")
            );
            assert_eq!(
                store
                    .actor_queue_fact(&owner, &session, &request_id)
                    .unwrap()
                    .state,
                aw_store::QueueState::Accepted,
            );
            assert_eq!(
                store.sandbox_get(&manager, &sandbox.canonical_ref).unwrap(),
                before,
            );
            drop(source);
            drop(host);
            drop(store);

            // Actual exclusive Store restart reconstructs the original
            // interrupted intent without requiring a live source approval.
            let store =
                SessionStore::open_with_sources(&state, registry.clone(), Default::default())
                    .unwrap();
            let interrupted = store
                .read_agent_branch_view(&owner, &session, &branch)
                .unwrap();
            let original = interrupted.history.turns.last().unwrap();
            assert_eq!(original.id, turn_id);
            assert_eq!(original.outcome, aw_agent_core::Outcome::Failed);
            assert_eq!(original.tool_records.len(), 1);
            assert_eq!(
                original.tool_records[0].status,
                aw_agent_core::ToolStatus::Interrupted
            );
            assert_eq!(original.invocations.len(), 1);
            assert_eq!(interrupted.history.needs_reconciliation, tool == "write");
            assert!(store
                .sandbox_recover_tool_source_effect(
                    &manager,
                    &reference,
                    turn_id,
                    &original.tool_records[0].call.id,
                    |_| panic!("unclaimed intent has no physical recovery custody"),
                )
                .is_err());
            let result = store
                .reconcile_native_turn(&manager, &reference, turn_id)
                .unwrap();
            assert_eq!(result.source_effects, 0);
            assert_eq!(result.unclaimed_no_start, 1);
            assert_eq!(result.hazard_cleared, tool == "write");
            assert_eq!(result.input, aw_store::InputAdmissionState::Inspected);
            assert_eq!(result.queue, Some(aw_store::QueueState::Inspected));
            assert!(result.changed);
            let replay = store
                .reconcile_native_turn(&manager, &reference, turn_id)
                .unwrap();
            assert_eq!(replay.unclaimed_no_start, 1);
            assert!(!replay.changed);
            assert!(!replay.hazard_cleared);
            let settled = store
                .read_agent_branch_view(&owner, &session, &branch)
                .unwrap();
            assert_eq!(
                serde_json::to_value(settled.history.turns.last().unwrap()).unwrap(),
                serde_json::to_value(original).unwrap()
            );
            assert!(!settled.history.needs_reconciliation);
            assert_eq!(
                store
                    .actor_queue_fact(&owner, &session, "fixture:next")
                    .unwrap()
                    .state,
                aw_store::QueueState::Pending,
            );
            assert_eq!(
                store.sandbox_get(&manager, &sandbox.canonical_ref).unwrap(),
                before
            );
            assert!(!repo.join("not-started").exists());
            assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 5);
        for (strict, message_id, prompt) in [
            (true, 5, "reject uncontained Process"),
            (false, 6, "perform source Process"),
            (false, 7, "interrupt-process-before-claim"),
            (false, 8, "interrupt-process-before-release"),
            (false, 9, "interrupt-mixed-before-claim"),
        ] {
            let mixed = prompt == "interrupt-mixed-before-claim";
            let store =
                SessionStore::open_with_sources(&state, registry.clone(), Default::default())
                    .unwrap();
            let current = store.sandbox_get(&manager, &sandbox.canonical_ref).unwrap();
            // Fresh independent approval, not the prior durable grant.
            // Use the already configured original source layout.
            // The refresh closure updates the private config using current rev.
            drop(refresh(&store));
            let resolver = GitNativeSourceResolver::new(store.clone(), handle.clone()).unwrap();
            let approval = resolver
                .provision_configured_approval(&manager, &approvals)
                .unwrap();
            drop(resolver);
            let drivers = SandboxDriverRegistry::new();
            let bash = std::env::var_os("AW_BASH")
                .map(PathBuf::from)
                .or_else(|| option_env!("AW_DEFAULT_BASH").map(PathBuf::from))
                .expect("trusted fixture Bash");
            let driver = drivers
                .register(
                    DriverKey::builtin("fixture-source-process").unwrap(),
                    FixedProcessDriver {
                        approval: approval.clone(),
                        bash,
                    },
                )
                .unwrap();
            let mixed_host = if mixed {
                let native = drivers
                    .register(
                        DriverKey::builtin("native-filesystem").unwrap(),
                        NativeFilesystemDriver,
                    )
                    .unwrap();
                Some(
                    GitNativeSourceHost::new(
                        approval.clone(),
                        handle.clone(),
                        drivers.clone(),
                        native,
                    )
                    .unwrap(),
                )
            } else {
                None
            };
            let independent = approval.clone();
            let host = aw_host::source_process::SourceProcessHost::new(
                drivers,
                driver.clone(),
                move || independent.read(),
            )
            .unwrap();
            let mut options = aw_tools_adapter::LocalToolOptions::new(
                repo.clone(),
                aw_tools_adapter::ToolMode::Trusted,
            );
            options.selected_tools = if mixed {
                vec!["read".into(), "bash".into(), "write".into()]
            } else {
                vec!["bash".into()]
            };
            options.bash = None;
            let build_process = |options: &aw_tools_adapter::LocalToolOptions| {
                let builder = Agent::builder_from_selection(
                    selection.clone(),
                    AgentOptions::new("fixture", "model"),
                );
                if let Some(native) = &mixed_host {
                    native.build_execution_with_process(
                        &store,
                        owner.clone(),
                        execution.clone(),
                        builder,
                        options,
                        driver.clone(),
                    )
                } else {
                    host.build_execution(&store, owner.clone(), execution.clone(), builder, options)
                }
            };
            assert!(build_process(&options).is_err());
            grant_rev = approval
                .publish_grant(&manager, grant_rev)
                .unwrap()
                .revision;
            options.local_process_containment = if strict {
                aw_tools_adapter::process::ContainmentRequirement::ClosedDescendants
            } else {
                aw_tools_adapter::process::ContainmentRequirement::BestAvailable
            };
            if mixed {
                let mut unsupported = options.clone();
                unsupported.selected_tools.push("batch".into());
                assert!(build_process(&unsupported).is_err());
                unsupported.selected_tools.pop();
                unsupported.selected_tools.push("pwsh".into());
                assert!(build_process(&unsupported).is_err());
            }
            let (mut agent, source, jobs) = build_process(&options).unwrap();
            let jobs_weak = Arc::downgrade(&jobs);
            assert_eq!(
                agent.registered_tool_names(),
                if mixed {
                    vec!["bash", "read", "write"]
                } else {
                    vec!["bash"]
                }
            );
            let request_id = match prompt {
                "reject uncontained Process" => "fixture:process-strict",
                "perform source Process" => "fixture:process",
                "interrupt-process-before-claim" => "fixture:process-no-start",
                "interrupt-process-before-release" => "fixture:process-unreleased",
                "interrupt-mixed-before-claim" => "fixture:mixed-no-start",
                _ => unreachable!(),
            };
            let (view, journal) = store.activate_with_source(&source).unwrap();
            let turn_id = view.history.next_turn_id;
            agent
                .load_history(view.history, Some(Arc::new(journal)))
                .unwrap();
            let actor = actor_owner::Service::attach_for_agent_with_source(
                store.clone(),
                &agent,
                source.clone(),
            )
            .unwrap();
            actor
                .enqueue_bound_text(
                    &agent,
                    request_id,
                    message_id,
                    aw_store::QueueTarget::Auto,
                    0,
                    prompt,
                )
                .unwrap();
            assert_eq!(
                actor
                    .admit_next_queued(&agent)
                    .unwrap()
                    .unwrap()
                    .request
                    .request_id,
                request_id
            );
            if prompt.starts_with("interrupt-process-") || mixed {
                let no_start = prompt == "interrupt-process-before-claim" || mixed;
                // The actual journal ACK precedes each event. In the
                // second case Process wait/reap and physical proof have
                // committed, but the post-journal release hook has not run.
                let task = tokio::spawn(async move {
                    actor
                        .run_once(&mut agent, |event| {
                            let record = match event {
                                aw_agent_core::AgentEvent::ToolStarted { record, .. }
                                    if no_start && (!mixed || record.call.name == "write") =>
                                {
                                    record
                                }
                                aw_agent_core::AgentEvent::ToolFinished { record, .. }
                                    if !no_start =>
                                {
                                    assert_eq!(record.status, aw_agent_core::ToolStatus::Succeeded);
                                    record
                                }
                                _ => return Ok(()),
                            };
                            assert_eq!(record.call.name, if mixed { "write" } else { "bash" });
                            panic!("fixture lost original Process Actor");
                        })
                        .await
                });
                let panic = task
                    .await
                    .expect_err("fixture must lose Process Actor")
                    .into_panic();
                assert_eq!(
                    panic.downcast_ref::<&str>(),
                    Some(&"fixture lost original Process Actor")
                );
                let mut expected = current.clone();
                expected.rev += u64::from(mixed);
                let before_restart = store.sandbox_get(&manager, &sandbox.canonical_ref).unwrap();
                assert_eq!(before_restart.rev, expected.rev);
                assert_eq!(before_restart.sources, expected.sources);
                assert_eq!(
                    store
                        .actor_queue_fact(&owner, &session, request_id)
                        .unwrap()
                        .state,
                    aw_store::QueueState::Accepted
                );
                drop(source);
                drop(jobs);
                assert!(jobs_weak.upgrade().is_none());
                drop(host);
                drop(mixed_host);
                drop(approval);
                drop(store);

                // No current Process driver or approval. The original
                // source registry is needed only for the existing
                // per-effect release owner, not logical no-start.
                let store = if no_start {
                    SessionStore::open(&state).unwrap()
                } else {
                    SessionStore::open_with_sources(&state, registry.clone(), Default::default())
                        .unwrap()
                };
                let interrupted = store
                    .read_agent_branch_view(&owner, &session, &branch)
                    .unwrap();
                assert!(interrupted.history.needs_reconciliation);
                let original = interrupted.history.turns.last().unwrap();
                assert_eq!(original.id, turn_id);
                assert_eq!(original.outcome, aw_agent_core::Outcome::Failed);
                assert_eq!(original.tool_records.len(), if mixed { 3 } else { 1 });
                assert_eq!(
                    original.tool_records.last().unwrap().status,
                    if no_start {
                        aw_agent_core::ToolStatus::Interrupted
                    } else {
                        aw_agent_core::ToolStatus::Succeeded
                    }
                );
                if mixed {
                    assert!(original.tool_records[..2]
                        .iter()
                        .all(|record| record.status == aw_agent_core::ToolStatus::Succeeded));
                }
                assert!(store
                    .reconcile_native_turn(&manager, &reference, turn_id)
                    .is_err());
                let target = || aw_host::source_process::SourceToolRecoveryTarget {
                    agent_ref: &reference,
                    turn_id,
                    call_id: "source-process-recovery",
                };
                let unavailable = SandboxDriverRegistry::new();
                if mixed {
                    assert_eq!(
                        fs::read(repo.join("mixed-process-created")).unwrap(),
                        b"mixed-process"
                    );
                    assert!(!repo.join("mixed-not-started").exists());
                    assert_eq!(
                        store
                            .sandbox_recover_tool_source_effect(
                                &manager,
                                &reference,
                                turn_id,
                                "mixed-read",
                                |_| panic!("released read must not inspect")
                            )
                            .unwrap(),
                        aw_store::SandboxEffectOutcome::Settled
                    );
                    assert_eq!(
                        aw_host::source_process::recover_source_process(
                            &store,
                            &manager,
                            aw_host::source_process::SourceToolRecoveryTarget {
                                agent_ref: &reference,
                                turn_id,
                                call_id: "mixed-process",
                            },
                            &sandbox.canonical_ref,
                            &unavailable,
                            &driver,
                            |_, _| -> Result<()> { panic!("released Process must not inspect") }
                        )
                        .unwrap(),
                        aw_store::SandboxEffectOutcome::Settled
                    );
                    assert!(store
                        .sandbox_recover_tool_source_effect(
                            &manager,
                            &reference,
                            turn_id,
                            "mixed-unclaimed",
                            |_| panic!("no-start write must not inspect")
                        )
                        .is_err());
                } else if no_start {
                    assert!(!repo.join("process-recovery").exists());
                    assert!(aw_host::source_process::recover_source_process(
                        &store,
                        &manager,
                        target(),
                        &sandbox.canonical_ref,
                        &unavailable,
                        &driver,
                        |_, _| -> Result<()> {
                            panic!("unclaimed Process has no physical custody")
                        },
                    )
                    .is_err());
                } else {
                    assert_eq!(
                        fs::read(repo.join("process-recovery")).unwrap(),
                        b"recovered-process"
                    );
                    // Recorded quiescence alone is not the original
                    // source release. Retain the Run and queue until it
                    // is consumed by the existing per-effect owner.
                    assert!(store
                        .reconcile_source_turn(&manager, &reference, turn_id)
                        .is_err());
                    assert_eq!(
                        store
                            .actor_queue_fact(&owner, &session, request_id)
                            .unwrap()
                            .state,
                        aw_store::QueueState::Accepted
                    );
                    assert_eq!(
                        store.sandbox_get(&manager, &sandbox.canonical_ref).unwrap(),
                        current
                    );
                    assert_eq!(
                        aw_host::source_process::recover_source_process(
                            &store,
                            &manager,
                            target(),
                            &sandbox.canonical_ref,
                            &unavailable,
                            &driver,
                            |_, _| -> Result<()> {
                                panic!("recorded quiescence must not need a live Process driver")
                            },
                        )
                        .unwrap(),
                        aw_store::SandboxEffectOutcome::Settled
                    );
                }
                let result = store
                    .reconcile_source_turn(&manager, &reference, turn_id)
                    .unwrap();
                assert_eq!(
                    result.source_effects,
                    if mixed { 2 } else { usize::from(!no_start) }
                );
                assert_eq!(result.unclaimed_no_start, usize::from(no_start));
                assert!(result.hazard_cleared && result.changed);
                assert_eq!(result.input, aw_store::InputAdmissionState::Inspected);
                assert_eq!(result.queue, Some(aw_store::QueueState::Inspected));
                let replay = store
                    .reconcile_source_turn(&manager, &reference, turn_id)
                    .unwrap();
                assert_eq!(replay.source_effects, result.source_effects);
                assert_eq!(replay.unclaimed_no_start, result.unclaimed_no_start);
                assert!(!replay.hazard_cleared && !replay.changed);
                let after = store
                    .read_agent_branch_view(&owner, &session, &branch)
                    .unwrap();
                assert_eq!(json!(after.history.turns.last().unwrap()), json!(original));
                assert!(!after.history.needs_reconciliation);
                assert_eq!(
                    store
                        .actor_queue_fact(&owner, &session, "fixture:next")
                        .unwrap()
                        .state,
                    aw_store::QueueState::Pending
                );
                assert_eq!(
                    store
                        .sandbox_get(&manager, &sandbox.canonical_ref)
                        .unwrap()
                        .rev,
                    current.rev + u64::from(!no_start || mixed)
                );
                assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
                continue;
            }
            let turn = actor
                .run_once(&mut agent, |_| Ok(()))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(turn.id, turn_id);
            assert_eq!(turn.outcome, aw_agent_core::Outcome::Completed, "{turn:?}");
            assert_eq!(turn.tool_records.len(), 1);
            assert_eq!(
                turn.tool_records[0].status,
                if strict {
                    aw_agent_core::ToolStatus::Failed
                } else {
                    aw_agent_core::ToolStatus::Succeeded
                }
            );
            if strict {
                assert!(!repo.join("process-created").exists());
            } else {
                assert_eq!(
                    fs::read(repo.join("process-created")).unwrap(),
                    b"process-owned"
                );
            }
            assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
            assert_eq!(
                store
                    .sandbox_get(&manager, &sandbox.canonical_ref)
                    .unwrap()
                    .rev,
                current.rev + u64::from(!strict)
            );
            assert_eq!(
                store
                    .actor_queue_fact(&owner, &session, request_id)
                    .unwrap()
                    .state,
                aw_store::QueueState::Settled,
            );
            assert_eq!(
                store
                    .actor_queue_fact(&owner, &session, "fixture:next")
                    .unwrap()
                    .state,
                aw_store::QueueState::Pending
            );
            // The original journal, not the Job result alone, authorizes
            // the physical release; exact terminal replay needs no proof.
            assert_eq!(
                store
                    .sandbox_recover_tool_source_effect(
                        &manager,
                        &reference,
                        turn_id,
                        "source-process-fixture",
                        |_| panic!("released original Process must not inspect")
                    )
                    .unwrap(),
                if strict {
                    aw_store::SandboxEffectOutcome::NotStarted
                } else {
                    aw_store::SandboxEffectOutcome::Settled
                },
            );
            drop(actor);
            drop(agent);
            drop(source);
            drop(jobs);
            assert!(
                jobs_weak.upgrade().is_none(),
                "source Job runtime retained its owner"
            );
            drop(host);
            drop(mixed_host);
            drop(approval);
            drop(store);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 14);
        for (message_id, prompt, path) in [
            (10, "interrupt-graph-before-claim", "graph-not-started"),
            (11, "interrupt-graph-after-settlement", "graph-created"),
            (12, "interrupt-graph-lost-node", "graph-unreleased"),
            (13, "interrupt-graph-needs-inspection", "graph-inspected"),
        ] {
            let no_start = message_id == 10;
            let lost_node = message_id == 12;
            let needs_inspection = message_id == 13;
            let unresolved = lost_node || needs_inspection;
            let stage_name = ".aw-stage-graph-recovery-fixture.tmp";
            let stage = repo.join(stage_name);
            let lookalike = repo.join(".aw-stage-graph-unowned-fixture.tmp");
            let checkout_identity = aw_platform::DirectoryIdentity::open(&repo)
                .unwrap()
                .identity()
                .unwrap();
            let artifact_identity = aw_platform::DirectoryIdentity::open(&approvals)
                .unwrap()
                .identity()
                .unwrap();
            if needs_inspection {
                // Fault fixture at the real durable Stage boundary, exactly
                // as in the executable native recovery test. The later
                // original Graph write cannot assert quiescence while this
                // original artifact namespace has unresolved custody.
                let object_identity = |path: &Path| {
                    let meta = fs::symlink_metadata(path).unwrap();
                    ObjectIdentity {
                        volume: meta.dev(),
                        object: meta.ino(),
                    }
                };
                let artifacts =
                    aw_tools_adapter::native_filesystem::reopen_source_artifacts(&repo, &approvals)
                        .unwrap();
                let mut reservation = artifacts
                    .reserve_name(
                        Kind::Stage,
                        String::new(),
                        object_identity(&repo),
                        stage_name.into(),
                    )
                    .unwrap();
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&stage)
                    .unwrap();
                let lease = reservation.finalize(object_identity(&stage)).unwrap();
                file.write_all(b"original Graph artifact").unwrap();
                file.sync_all().unwrap();
                drop(lease);
                fs::write(&lookalike, b"unowned").unwrap();
            }
            let store =
                SessionStore::open_with_sources(&state, registry.clone(), Default::default())
                    .unwrap();
            let before = store.sandbox_get(&manager, &sandbox.canonical_ref).unwrap();
            drop(refresh(&store));
            let resolver = GitNativeSourceResolver::new(store.clone(), handle.clone()).unwrap();
            let approval = resolver
                .provision_configured_approval(&manager, &approvals)
                .unwrap();
            drop(resolver);
            let drivers = SandboxDriverRegistry::new();
            let native = drivers
                .register(
                    DriverKey::builtin("native-filesystem").unwrap(),
                    NativeFilesystemDriver,
                )
                .unwrap();
            let host = GitNativeSourceHost::new(
                approval.clone(),
                handle.clone(),
                drivers.clone(),
                native.clone(),
            )
            .unwrap();
            let mut options = tools.clone();
            options.selected_tools.push("batch".into());
            let builder = || {
                Agent::builder_from_selection(
                    selection.clone(),
                    AgentOptions::new("fixture", "model"),
                )
            };
            assert!(host
                .build_execution_with_graph(
                    &store,
                    owner.clone(),
                    execution.clone(),
                    builder(),
                    &options
                )
                .is_err());
            grant_rev = approval
                .publish_grant(&manager, grant_rev)
                .unwrap()
                .revision;
            let (mut agent, source, jobs) = if lost_node {
                let builder = builder();
                let route_host = host.clone();
                let route = Arc::new(
                    aw_host::source_filesystem::SourceFilesystemRoute::new(
                        store.clone(),
                        execution.clone(),
                        drivers.clone(),
                        native,
                        builder.tool_binding_authority(),
                        options.limits,
                        move |ctx, tool| route_host.resolve(ctx, tool),
                    )
                    .unwrap(),
                );
                let configured = aw_tools_adapter::configure_source_graph(
                    builder,
                    owner.clone(),
                    &options,
                    Arc::new(LostNodeSettlement(route.clone())),
                )
                .unwrap();
                let jobs = configured.job_runtime.unwrap();
                let mut agent = configured.builder.shared_extension(route).build().unwrap();
                agent.bind_execution_owner(execution.clone()).unwrap();
                let independent = approval.clone();
                let source = aw_store::SandboxExecution::from_trusted_host(
                    &store,
                    owner.clone(),
                    execution.clone(),
                    repo.clone(),
                    &agent.tool_binding_authority(),
                    move || independent.read(),
                )
                .unwrap();
                (agent, source, jobs)
            } else {
                host.build_execution_with_graph(
                    &store,
                    owner.clone(),
                    execution.clone(),
                    builder(),
                    &options,
                )
                .unwrap()
            };
            let jobs_weak = Arc::downgrade(&jobs);
            assert_eq!(agent.registered_tool_names(), ["batch", "read", "write"]);
            let (view, journal) = store.activate_with_source(&source).unwrap();
            let turn_id = view.history.next_turn_id;
            agent
                .load_history(view.history, Some(Arc::new(journal)))
                .unwrap();
            let actor = actor_owner::Service::attach_for_agent_with_source(
                store.clone(),
                &agent,
                source.clone(),
            )
            .unwrap();
            let request_id = format!("fixture:graph-{message_id}");
            actor
                .enqueue_bound_text(
                    &agent,
                    &request_id,
                    message_id,
                    aw_store::QueueTarget::Auto,
                    0,
                    prompt,
                )
                .unwrap();
            assert_eq!(
                actor
                    .admit_next_queued(&agent)
                    .unwrap()
                    .unwrap()
                    .request
                    .request_id,
                request_id
            );
            let task = tokio::spawn(async move {
                actor
                    .run_once(&mut agent, |event| {
                        let record = match event {
                            aw_agent_core::AgentEvent::ToolStarted { record, .. } if no_start => {
                                record
                            }
                            aw_agent_core::AgentEvent::ToolFinished { record, .. } if !no_start => {
                                assert_eq!(
                                    record.status,
                                    if unresolved {
                                        aw_agent_core::ToolStatus::Interrupted
                                    } else {
                                        aw_agent_core::ToolStatus::Succeeded
                                    }
                                );
                                if !unresolved {
                                    let output = record.output.as_ref().unwrap();
                                    assert!(output.content.contains("fixture data"), "{output:?}");
                                    assert!(output.content.contains("graph node"), "{output:?}");
                                }
                                record
                            }
                            _ => return Ok(()),
                        };
                        assert_eq!(record.call.name, "batch");
                        panic!("fixture lost original Graph Actor");
                    })
                    .await
            });
            let panic = task
                .await
                .expect_err("fixture must lose original Graph Actor")
                .into_panic();
            assert_eq!(
                panic.downcast_ref::<&str>(),
                Some(&"fixture lost original Graph Actor")
            );
            let after = store.sandbox_get(&manager, &sandbox.canonical_ref).unwrap();
            assert_eq!(after.rev, before.rev + u64::from(!no_start && !unresolved));
            assert_eq!(after.sources, before.sources);
            if no_start {
                assert!(!repo.join(path).exists());
            } else {
                assert_eq!(fs::read(repo.join(path)).unwrap(), b"graph node");
            }
            assert!(!repo.join("graph-skipped").exists());
            assert_eq!(
                store
                    .actor_queue_fact(&owner, &session, &request_id)
                    .unwrap()
                    .state,
                aw_store::QueueState::Accepted
            );
            drop(source);
            drop(jobs);
            assert!(jobs_weak.upgrade().is_none());
            drop(host);
            drop(drivers);
            drop(approval);
            drop(store);
            let store = if unresolved {
                SessionStore::open_with_sources(&state, registry.clone(), Default::default())
                    .unwrap()
            } else {
                SessionStore::open(&state).unwrap()
            };
            let view = store
                .read_agent_branch_view(&owner, &session, &branch)
                .unwrap();
            assert!(view.history.needs_reconciliation);
            let original = view.history.turns.last().unwrap();
            assert_eq!(original.id, turn_id);
            assert_eq!(original.outcome, aw_agent_core::Outcome::Failed);
            assert_eq!(original.tool_records.len(), 1);
            assert!(store
                .reconcile_native_turn(&manager, &reference, turn_id)
                .is_err());
            assert!(store
                .sandbox_recover_tool_source_effect(
                    &manager,
                    &reference,
                    turn_id,
                    "source-graph-fixture",
                    |_| panic!("Graph constructor is not node custody")
                )
                .is_err());
            let recover = |coordinate| {
                store.sandbox_recover_recorded_graph_node(
                    &manager,
                    &reference,
                    turn_id,
                    "source-graph-fixture",
                    coordinate,
                )
            };
            assert!(recover(4).is_err());
            assert!(recover(5).is_err());
            if no_start {
                assert!(recover(1).is_err());
            } else {
                assert_eq!(recover(1).unwrap(), aw_store::SandboxEffectOutcome::Settled);
                if unresolved {
                    assert!(store
                        .reconcile_source_turn(&manager, &reference, turn_id)
                        .is_err());
                    assert_eq!(
                        store
                            .actor_queue_fact(&owner, &session, &request_id)
                            .unwrap()
                            .state,
                        aw_store::QueueState::Accepted
                    );
                    assert!(recover(3).is_err());
                } else {
                    assert_eq!(recover(3).unwrap(), aw_store::SandboxEffectOutcome::Settled);
                }
                if needs_inspection {
                    use aw_host::source_git::NativeSourceRecoveryTarget;
                    let target = || NativeSourceRecoveryTarget {
                        agent_ref: &reference,
                        turn_id,
                        call_id: "source-graph-fixture",
                    };
                    let native_key = DriverKey::builtin("native-filesystem").unwrap();
                    let source = aw_agent_core::ToolSource::trusted_host();
                    let selector = |effects| aw_store::SandboxToolRecoverySelection {
                        driver: &native_key,
                        sandbox_ref: Some(&sandbox.canonical_ref),
                        effects,
                        source: &source,
                        execution_site: Some("local"),
                    };
                    // Reject a mismatched effect even on an already released
                    // sibling. An exact coordinate is not a capability.
                    assert!(store
                        .sandbox_recover_graph_node_exact(
                            &manager,
                            &reference,
                            turn_id,
                            "source-graph-fixture",
                            1,
                            selector(&[aw_agent_core::ToolEffect::Write]),
                            |_| panic!("wrong effect must refuse before release replay"),
                        )
                        .is_err());
                    assert!(stage.is_file());
                    assert!(recover(2).is_err(), "absence of proof cannot settle node");
                    // A claimed effect's fresh physical recovery cannot
                    // invent a pre-launch outcome.
                    assert!(store
                        .sandbox_recover_graph_node_exact(
                            &manager,
                            &reference,
                            turn_id,
                            "source-graph-fixture",
                            2,
                            selector(&[aw_agent_core::ToolEffect::Write]),
                            |_| Ok(aw_store::SandboxEffectOutcome::NotStarted),
                        )
                        .is_err());
                    let inspect = |identity| {
                        GitNativeSourceResolver::recover_configured_graph_node(
                            &store,
                            &manager,
                            target(),
                            2,
                            &sandbox.canonical_ref,
                            |recovery| {
                                let node = recovery.node.as_ref().expect("original Graph node");
                                assert_eq!(node.binding.coordinate, 2);
                                assert_eq!(node.tool.name(), "write");
                                assert_eq!(
                                    node.arguments,
                                    json!({"path":path,"content":"graph node"})
                                );
                                assert_eq!(recovery.original.call.name, "batch");
                                assert_eq!(
                                    recovery.original.effect,
                                    aw_agent_core::ToolEffect::Control
                                );
                                assert_eq!(
                                    recovery.terminal.status,
                                    aw_agent_core::ToolStatus::Interrupted
                                );
                                assert_eq!(recovery.effect(), aw_agent_core::ToolEffect::Write);
                                assert_eq!(recovery.sources.len(), 1);
                                assert_eq!(
                                    recovery.sources[0].binding.binding_rev,
                                    before.sources[0].binding_rev
                                );
                                assert_eq!(
                                    recovery.sources[0].binding.binding.name,
                                    before.sources[0].name
                                );
                                assert_eq!(
                                    recovery.sources[0].binding.binding.driver,
                                    before.sources[0].driver
                                );
                                assert_eq!(recovery.sources[0].access, SourceAccess::ReadWrite);
                                let root = aw_platform::DirectoryIdentity::open(&repo)?;
                                let artifacts = aw_platform::DirectoryIdentity::open(&approvals)?;
                                assert_eq!(artifacts.identity()?, artifact_identity);
                                assert_eq!(root.identity()?, checkout_identity);
                                Ok((
                                    RecoveryFence {
                                        locations: vec![GitSourceLocation {
                                            name: name.clone(),
                                            repository: repo.clone(),
                                            repository_identity: identity,
                                        }],
                                        _pins: vec![root, artifacts],
                                    },
                                    approvals.clone(),
                                ))
                            },
                        )
                    };
                    // Wrong independently approved checkout identity must
                    // leave the exact artifact and original source hazard.
                    assert!(inspect((0, 0)).is_err());
                    assert!(stage.is_file());
                    assert_eq!(
                        store
                            .sandbox_get(&manager, &sandbox.canonical_ref)
                            .unwrap()
                            .rev,
                        before.rev
                    );
                    assert!(store
                        .reconcile_source_turn(&manager, &reference, turn_id)
                        .is_err());
                    assert_eq!(
                        inspect(checkout_identity).unwrap(),
                        aw_store::SandboxEffectOutcome::Settled
                    );
                    assert!(!stage.exists());
                    assert_eq!(fs::read(&lookalike).unwrap(), b"unowned");
                    assert_eq!(fs::read(repo.join(path)).unwrap(), b"graph node");
                    // Existing release replay must not re-probe the driver
                    // or repeat physical cleanup.
                    assert_eq!(
                        GitNativeSourceResolver::recover_configured_graph_node::<RecoveryFence>(
                            &store,
                            &manager,
                            target(),
                            2,
                            &sandbox.canonical_ref,
                            |_| panic!("released Graph node must not inspect"),
                        )
                        .unwrap(),
                        aw_store::SandboxEffectOutcome::Settled
                    );
                } else {
                    assert_eq!(recover(2).unwrap(), aw_store::SandboxEffectOutcome::Settled);
                }
                assert_eq!(recover(2).unwrap(), aw_store::SandboxEffectOutcome::Settled);
            }
            let result = store
                .reconcile_source_turn(&manager, &reference, turn_id)
                .unwrap();
            let effects = if no_start {
                0
            } else if unresolved {
                2
            } else {
                3
            };
            assert_eq!(result.source_effects, effects);
            assert_eq!(result.unclaimed_no_start, 4 - effects);
            assert!(result.hazard_cleared && result.changed);
            assert_eq!(result.input, aw_store::InputAdmissionState::Inspected);
            assert_eq!(result.queue, Some(aw_store::QueueState::Inspected));
            let replay = store
                .reconcile_source_turn(&manager, &reference, turn_id)
                .unwrap();
            assert_eq!(replay.source_effects, effects);
            assert!(!replay.changed && !replay.hazard_cleared);
            let after = store
                .read_agent_branch_view(&owner, &session, &branch)
                .unwrap();
            assert_eq!(json!(after.history.turns.last().unwrap()), json!(original));
            assert!(!after.history.needs_reconciliation);
            assert_eq!(
                store
                    .actor_queue_fact(&owner, &session, "fixture:next")
                    .unwrap()
                    .state,
                aw_store::QueueState::Pending
            );
            assert_eq!(
                store
                    .sandbox_get(&manager, &sandbox.canonical_ref)
                    .unwrap()
                    .rev,
                before.rev + u64::from(!no_start)
            );
            assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 18);
        let store = SessionStore::open_with_sources(&state, registry, Default::default()).unwrap();
        let host = refresh(&store);
        assert!(build(&store, &host).is_err());
        host.publish_grant(&manager, grant_rev).unwrap();
        let router = Arc::new(agent_jobs::Router::new(store.clone(), owner.clone(), 8).unwrap());
        let activation = HostAgentActivation::new(
            store.clone(),
            owner.clone(),
            selection.clone(),
            AgentOptions::new("fixture", "model"),
            tools.clone(),
            router,
        )
        .unwrap()
        .with_git_native_source_host(host);
        let activated = activation.activate(&reference).unwrap().unwrap();
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                if store
                    .read_agent_branch_view(&owner, &session, &branch)
                    .is_ok_and(|v| {
                        v.history.turns.len() == 13
                            && v.history.turns[12].outcome == aw_agent_core::Outcome::Completed
                    })
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("original queued source input did not settle");
        assert_eq!(calls.load(Ordering::SeqCst), 21);
        assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
        activated.close().unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if store.agent_get(&manager, &reference).unwrap().lifecycle
                    == AgentLifecycle::Closed
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
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
        let redirected_workspace = store
            .create_workspace(
                &actor,
                Some(&root_binding.domain_handle),
                "activation-redirected",
            )
            .unwrap();
        let redirected_project = format!(
            "/d:{}/i:{}/p:{}",
            root_binding.domain_handle, root_binding.identity_handle, redirected_workspace.id
        );
        let redirected_sandbox = store
            .sandbox_create_fresh(&actor, &redirected_project, &binding)
            .unwrap();
        let child_owner =
            aw_store::stable_user_principal(&store.current_user(&actor).unwrap().user_id).unwrap();

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
        let router = Arc::new(agent_jobs::Router::new(store.clone(), actor.clone(), 8).unwrap());
        let child_router = Arc::new(router.for_actor(child_owner.clone()));
        let activation = HostAgentActivation::new(
            store.clone(),
            child_owner.clone(),
            selection,
            AgentOptions::new("fixture", "model"),
            aw_tools_adapter::LocalToolOptions::new(
                state_dir.clone(),
                aw_tools_adapter::ToolMode::Off,
            ),
            child_router,
        )
        .unwrap();
        let creator = HostAgentCreator::new(
            store.clone(),
            actor.clone(),
            root_ref.clone(),
            binding,
            "local",
            activation,
        )
        .unwrap();
        let request = AgentCreateRequest {
            project: Some(redirected_project.clone()),
            title: Some("activated child".into()),
            task: Some("perform the initial task".into()),
            sandbox: aw_store::AgentSandboxSelection::Existing(
                redirected_sandbox.canonical_ref.clone(),
            ),
            ..Default::default()
        };
        let created = creator
            .create("host-activation-task-1", request.clone())
            .unwrap();

        assert_eq!(created.project_ref, redirected_project);
        assert_eq!(
            created.sandbox_ref.as_deref(),
            Some(redirected_sandbox.canonical_ref.as_str())
        );
        assert!(router.has_live_owner(&created.canonical_ref).unwrap());

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

        let replay = creator.create("host-activation-task-1", request).unwrap();
        assert_eq!(replay.canonical_ref, created.canonical_ref);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let staged = store
            .agent_inputs_add(
                &actor,
                &created.canonical_ref,
                &[AgentInputAdd {
                    text: "perform the follow-up".into(),
                    intent: AgentInputIntent::Followup { after: Some(1) },
                    before: None,
                    after: None,
                }],
                None,
            )
            .unwrap();
        let agent_rev_before_send = store.agent_get(&actor, &created.canonical_ref).unwrap().rev;
        let host_inputs = HostAgentInputs::new(
            store.clone(),
            actor.clone(),
            root_ref.clone(),
            router.clone(),
        )
        .unwrap();
        let sent = host_inputs
            .send(
                "host-input-send-1",
                &created.canonical_ref,
                None,
                staged.inputs_rev,
            )
            .unwrap();
        assert!(sent.items.is_empty());
        assert_eq!(sent.inputs_rev, staged.inputs_rev + 1);

        let followup_settled = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let snapshot = store.agent_get(&actor, &created.canonical_ref).unwrap();
                if calls.load(Ordering::SeqCst) == 2 && snapshot.active_turn.is_none() {
                    break snapshot;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("sent staged follow-up must enter the existing queue and execute once");
        assert!(followup_settled.rev > agent_rev_before_send);
        let send_replay = host_inputs
            .send(
                "host-input-send-1",
                &created.canonical_ref,
                None,
                staged.inputs_rev,
            )
            .unwrap();
        assert_eq!(send_replay.inputs_rev, sent.inputs_rev);
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        let root_provider = Arc::new(CompletingProvider {
            calls: Arc::new(AtomicUsize::new(0)),
        });
        let mut root_agent =
            Agent::builder(root_provider, AgentOptions::new("root-fixture", "model"))
                .account(
                    AccountDescriptor::personal(
                        "root-fixture",
                        actor.clone(),
                        "local",
                        ["model".into()],
                    ),
                    actor.clone(),
                    "local",
                )
                .build()
                .unwrap();
        let (root_view, root_journal) = store.activate(&actor, &root.id, &root_branch.id).unwrap();
        root_agent
            .load_history(root_view.history, Some(Arc::new(root_journal)))
            .unwrap();
        root_agent
            .bind_execution_owner(
                AgentExecutionBinding::from_trusted_host(
                    root_ref.clone(),
                    root.id.clone(),
                    root_branch.id.clone(),
                )
                .unwrap(),
            )
            .unwrap();
        let root_owner = actor_owner::Service::attach_for_agent(
            store.clone(),
            &root.id,
            &root_branch.id,
            &root_agent,
        )
        .unwrap();
        router
            .register_live_owner(
                &root_ref,
                root_owner.clone(),
                root_agent.tool_binding_authority(),
            )
            .unwrap();
        let root_close = Arc::new(
            RootAgentClose::new(
                store.clone(),
                actor.clone(),
                root_ref,
                root_owner,
                None,
                router.clone(),
            )
            .unwrap(),
        );
        let host_close =
            HostAgentClose::new(store.clone(), actor.clone(), root_close, router.clone()).unwrap();
        let closing = host_close
            .close(&created.canonical_ref, Some(followup_settled.rev))
            .await
            .unwrap();
        assert_eq!(closing.lifecycle, AgentLifecycle::Closing);
        assert_eq!(closing.admission, aw_store::AgentAdmission::Held);

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
        let close_replay = host_close
            .close(&created.canonical_ref, None)
            .await
            .unwrap();
        assert_eq!(close_replay.lifecycle, AgentLifecycle::Closed);
        assert_eq!(close_replay.rev, closed.rev);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let _ = std::fs::remove_dir_all(state_dir);
    }
}
