//! One live Job session for AW's canonical Graph runtime.
//!
//! This is host coordination only: Job lifecycle/output stay in `JobManager`.
//! The runtime retains only runner cancellation tokens and replaces the whole
//! live Job namespace when the authenticated Agent Session scope changes.
use crate::{
    exposure::ExposureIndex,
    job_host::{GraphParallelism, JobHost, JobHostConfig},
    shell, Error, Result,
};
use aw_agent_core::tool::ToolBindingAuthority;
use aw_agent_core::{
    CancellationToken, InvocationScope, InvocationScopeLifecycle, ToolContext, ToolOutput,
};
use aw_tools::{
    filesystem::job_manager::{Limits, Snapshot},
    job_event::EventRuleSet,
};
use futures_util::FutureExt as _;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex, Weak},
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum LiveScope {
    Memory { session: String, branch: String },
    Journal(InvocationScope),
}
impl LiveScope {
    fn memory() -> Result<Self> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes)
            .map_err(|_| Error::Config("OS randomness unavailable for Job runtime".into()))?;
        let suffix = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Ok(Self::Memory {
            session: format!("memory-{suffix}"),
            branch: "main".into(),
        })
    }
    fn session(&self) -> &str {
        match self {
            Self::Memory { session, .. } => session,
            Self::Journal(scope) => &scope.session_id,
        }
    }
    fn branch(&self) -> &str {
        match self {
            Self::Memory { branch, .. } => branch,
            Self::Journal(scope) => &scope.branch_id,
        }
    }
    fn matches_context(&self, scope: Option<&InvocationScope>) -> bool {
        match (self, scope) {
            (Self::Memory { .. }, None) => true,
            (Self::Journal(bound), Some(scope)) => bound == scope,
            _ => false,
        }
    }
}

struct State {
    generation: u64,
    scope: LiveScope,
    host: Arc<JobHost>,
    runners: HashMap<String, CancellationToken>,
    control_replays: HashMap<[u8; 32], ControlReplayEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ControlReplayReceipt {
    pub canonical_ref: Option<String>,
    pub recipient_ref: Option<String>,
    pub recipient_authority_ref: Option<String>,
    pub output: ToolOutput,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ControlReplayState {
    Pending,
    Ready(ControlReplayReceipt),
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ControlReplayEntry {
    wire_fingerprint: [u8; 32],
    canonical_ref: Option<String>,
    state: ControlReplayState,
}

pub(crate) enum ControlReplayDecision {
    New([u8; 32]),
    Existing(ControlReplayReceipt),
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InitialWake {
    recipient: crate::agent_jobs::AgentWakeRecipient,
    rules: EventRuleSet,
}

pub struct StartedJob {
    pub id: String,
    pub host: Arc<JobHost>,
    pub task: tokio::task::JoinHandle<Result<Snapshot>>,
}
pub enum StartDecision {
    New(StartedJob),
    Existing { id: String, host: Arc<JobHost> },
    Unknown,
}

pub(crate) struct JobRuntimeOptions {
    pub(crate) agent_jobs: Arc<crate::agent_jobs::AgentJobRouterSlot>,
    pub(crate) graph_policy: Arc<crate::operation_router::GraphPolicyRouterSlot>,
    pub(crate) graph_host_tools: Arc<crate::operation_router::GraphHostToolSlot>,
    pub(crate) allow_agent_graph: bool,
    pub(crate) allow_wake: bool,
    pub(crate) max_graph_nodes: usize,
}

impl Default for JobRuntimeOptions {
    fn default() -> Self {
        Self {
            agent_jobs: Arc::new(crate::agent_jobs::AgentJobRouterSlot::default()),
            graph_policy: Arc::new(crate::operation_router::GraphPolicyRouterSlot::default()),
            graph_host_tools: Arc::new(crate::operation_router::GraphHostToolSlot::default()),
            allow_agent_graph: false,
            allow_wake: false,
            max_graph_nodes: aw_tools::filesystem::batch::MAX_OPS,
        }
    }
}

pub struct JobRuntime {
    root: Arc<aw_platform::scoped_fs::Root>,
    authority: ToolBindingAuthority,
    tool_limits: crate::LocalToolLimits,
    shells: shell::ShellRegistry,
    native_tools: Arc<Vec<aw_tools::filesystem::schema::Tool>>,
    parallelism: Arc<GraphParallelism>,
    exposure: Arc<ExposureIndex>,
    agent_jobs: Arc<crate::agent_jobs::AgentJobRouterSlot>,
    graph_policy: Arc<crate::operation_router::GraphPolicyRouterSlot>,
    graph_host_tools: Arc<crate::operation_router::GraphHostToolSlot>,
    allow_agent_graph: bool,
    allow_wake: bool,
    max_graph_nodes: usize,
    state: Mutex<State>,
}
impl JobRuntime {
    pub fn new(
        root: Arc<aw_platform::scoped_fs::Root>,
        authority: ToolBindingAuthority,
        limits: Limits,
    ) -> Result<Arc<Self>> {
        Self::new_with_native_tools(
            root,
            authority,
            crate::LocalToolLimits {
                jobs: limits,
                ..Default::default()
            },
            crate::LocalToolConcurrency::default(),
            aw_tools::filesystem::schema::Tool::GRAPH_NATIVE.to_vec(),
        )
    }

    pub(crate) fn new_with_native_tools(
        root: Arc<aw_platform::scoped_fs::Root>,
        authority: ToolBindingAuthority,
        tool_limits: crate::LocalToolLimits,
        concurrency: crate::LocalToolConcurrency,
        native_tools: Vec<aw_tools::filesystem::schema::Tool>,
    ) -> Result<Arc<Self>> {
        Self::new_with_native_tools_bounded(
            root,
            authority,
            tool_limits,
            concurrency,
            JobRuntimeOptions::default(),
            native_tools,
        )
    }

    pub(crate) fn new_with_native_tools_bounded(
        root: Arc<aw_platform::scoped_fs::Root>,
        authority: ToolBindingAuthority,
        tool_limits: crate::LocalToolLimits,
        concurrency: crate::LocalToolConcurrency,
        options: JobRuntimeOptions,
        native_tools: Vec<aw_tools::filesystem::schema::Tool>,
    ) -> Result<Arc<Self>> {
        let JobRuntimeOptions {
            agent_jobs,
            graph_policy,
            graph_host_tools,
            allow_agent_graph,
            allow_wake,
            max_graph_nodes,
        } = options;
        let scope = LiveScope::memory()?;
        let shells = shell::ShellRegistry::default();
        let native_tools = Arc::new(native_tools);
        let tool_limits = tool_limits.validate_for_core_tools(&native_tools, false, true)?;
        let mut concurrency = concurrency.validate()?;
        concurrency.graph_width = concurrency.graph_width.min(max_graph_nodes);
        tool_limits.validate_job_runtime_capacity(
            concurrency.graph_width,
            max_graph_nodes,
            !native_tools.is_empty(),
            false,
        )?;
        let parallelism = Arc::new(GraphParallelism::new(concurrency)?);
        let exposure = Arc::new(ExposureIndex::new(tool_limits.jobs.max_retained_jobs)?);
        let host = Arc::new(JobHost::new_with_config(
            root.clone(),
            authority.clone(),
            JobHostConfig {
                session: scope.session().to_owned(),
                branch: scope.branch().to_owned(),
                tool_limits,
                shells: shells.clone(),
                native_tools: native_tools.clone(),
                parallelism: parallelism.clone(),
                exposure: exposure.clone(),
                agent_jobs: agent_jobs.clone(),
                graph_policy: graph_policy.clone(),
                graph_host_tools: graph_host_tools.clone(),
                max_graph_nodes,
            },
        )?);
        Ok(Arc::new(Self {
            root,
            authority,
            tool_limits,
            shells,
            native_tools,
            parallelism,
            exposure,
            agent_jobs,
            graph_policy,
            graph_host_tools,
            allow_agent_graph,
            allow_wake,
            max_graph_nodes,
            state: Mutex::new(State {
                generation: 1,
                scope,
                host,
                runners: HashMap::new(),
                control_replays: HashMap::new(),
            }),
        }))
    }

    fn control_replay_key(generation: u64, ctx: &ToolContext, operation: &str) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"aw-job-control-invocation-v1\0");
        hash.update(generation.to_le_bytes());
        for part in [
            ctx.actor.as_str(),
            ctx.source.kind_label(),
            ctx.source.id.as_str(),
            ctx.execution_site.as_str(),
            ctx.call_id.as_str(),
            operation,
        ] {
            hash.update((part.len() as u64).to_le_bytes());
            hash.update(part.as_bytes());
        }
        hash.update(ctx.source.epoch.to_le_bytes());
        hash.update(ctx.turn_id.to_le_bytes());
        if let Some(scope) = &ctx.scope {
            for part in [scope.session_id.as_str(), scope.branch_id.as_str()] {
                hash.update((part.len() as u64).to_le_bytes());
                hash.update(part.as_bytes());
            }
        }
        hash.finalize().into()
    }

    pub(crate) fn claim_control_replay(
        &self,
        ctx: &ToolContext,
        operation: &str,
        fingerprint: [u8; 32],
    ) -> Result<ControlReplayDecision> {
        if fingerprint == [0; 32] {
            return Err(Error::Denied("invalid control replay fingerprint"));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
        if !state.scope.matches_context(ctx.scope.as_ref()) {
            return Err(Error::Denied(
                "Job runtime is bound to another live Session",
            ));
        }
        let key = Self::control_replay_key(state.generation, ctx, operation);
        if let Some(entry) = state.control_replays.get(&key) {
            if entry.wire_fingerprint != fingerprint {
                return Err(Error::Denied(
                    "control invocation identity reused with changed request",
                ));
            }
            return Ok(match &entry.state {
                ControlReplayState::Ready(receipt) => {
                    ControlReplayDecision::Existing(receipt.clone())
                }
                ControlReplayState::Pending | ControlReplayState::Unknown => {
                    ControlReplayDecision::Unknown
                }
            });
        }
        if state.control_replays.len() >= self.tool_limits.jobs.max_hidden_replays {
            return Err(Error::Limit("control replay capacity exhausted"));
        }
        state.control_replays.insert(
            key,
            ControlReplayEntry {
                wire_fingerprint: fingerprint,
                canonical_ref: None,
                state: ControlReplayState::Pending,
            },
        );
        Ok(ControlReplayDecision::New(key))
    }

    pub(crate) fn bind_control_replay_target(
        &self,
        key: [u8; 32],
        canonical_ref: &str,
    ) -> Result<()> {
        if canonical_ref.is_empty()
            || canonical_ref.len() > 2048
            || !canonical_ref.is_ascii()
            || canonical_ref.chars().any(char::is_control)
        {
            return Err(Error::Denied("invalid canonical control replay target"));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
        let entry = state
            .control_replays
            .get_mut(&key)
            .ok_or(Error::Protocol("control replay claim disappeared"))?;
        match entry.canonical_ref.as_deref() {
            None => {
                entry.canonical_ref = Some(canonical_ref.to_owned());
                Ok(())
            }
            Some(existing) if existing == canonical_ref => Ok(()),
            Some(_) => Err(Error::Protocol(
                "control replay target changed after original resolution",
            )),
        }
    }

    pub(crate) fn finish_control_replay(
        &self,
        key: [u8; 32],
        mut receipt: ControlReplayReceipt,
    ) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
        let entry = state
            .control_replays
            .get_mut(&key)
            .ok_or(Error::Protocol("control replay claim disappeared"))?;
        if receipt.canonical_ref.is_none() {
            receipt.canonical_ref = entry.canonical_ref.clone();
        }
        if entry.canonical_ref.is_none() {
            entry.canonical_ref = receipt.canonical_ref.clone();
        }
        if entry.canonical_ref != receipt.canonical_ref {
            return Err(Error::Protocol(
                "control replay receipt target differs from claimed target",
            ));
        }
        match &entry.state {
            ControlReplayState::Pending => {
                entry.state = ControlReplayState::Ready(receipt);
                Ok(())
            }
            ControlReplayState::Ready(existing) if existing == &receipt => Ok(()),
            _ => Err(Error::Protocol("control replay state changed unexpectedly")),
        }
    }

    pub(crate) fn mark_control_replay_unknown(&self, key: [u8; 32]) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(entry) = state.control_replays.get_mut(&key) {
                if entry.state == ControlReplayState::Pending {
                    entry.state = ControlReplayState::Unknown;
                }
            }
        }
    }

    fn build_host(&self, scope: &LiveScope) -> Result<Arc<JobHost>> {
        Ok(Arc::new(JobHost::new_with_config(
            self.root.clone(),
            self.authority.clone(),
            JobHostConfig {
                session: scope.session().to_owned(),
                branch: scope.branch().to_owned(),
                tool_limits: self.tool_limits,
                shells: self.shells.clone(),
                native_tools: self.native_tools.clone(),
                parallelism: self.parallelism.clone(),
                exposure: self.exposure.clone(),
                agent_jobs: self.agent_jobs.clone(),
                graph_policy: self.graph_policy.clone(),
                graph_host_tools: self.graph_host_tools.clone(),
                max_graph_nodes: self.max_graph_nodes,
            },
        )?))
    }

    pub(crate) fn register_shell(&self, shell: Arc<shell::NativeShell>) -> Result<()> {
        self.tool_limits
            .validate_for_core_tools(self.native_tools.as_ref(), true, true)?;
        self.tool_limits.validate_job_runtime_capacity(
            self.parallelism.max_width().min(self.max_graph_nodes),
            self.max_graph_nodes,
            !self.native_tools.is_empty(),
            true,
        )?;
        self.shells.register(shell)
    }

    pub(crate) fn agent_jobs_router(
        &self,
    ) -> Result<Option<Arc<dyn crate::agent_jobs::AgentJobRouter>>> {
        self.agent_jobs.current()
    }

    pub(crate) fn wake_capable(&self) -> bool {
        self.allow_wake
    }

    pub(crate) fn prepare_initial_wake(
        &self,
        ctx: &ToolContext,
        rules: EventRuleSet,
    ) -> Result<Option<InitialWake>> {
        if !self.allow_wake {
            return Err(Error::Denied(
                "constructor wake is unavailable for this Agent/host",
            ));
        }
        for rule in rules.as_slice() {
            match (rule.event, rule.mode) {
                (
                    aw_tools::job_event::JobEventKind::Result
                    | aw_tools::job_event::JobEventKind::Settled,
                    aw_tools::job_event::JobWakeMode::Auto
                    | aw_tools::job_event::JobWakeMode::Queue,
                ) => {}
                (aw_tools::job_event::JobEventKind::Attention, _) => {
                    return Err(Error::Denied(
                        "finite constructor wake does not expose attention",
                    ))
                }
                (_, _) => {
                    return Err(Error::Denied(
                        "finite constructor result/settled wake supports only auto or queue",
                    ))
                }
            }
        }
        if rules.is_empty() {
            return Ok(None);
        }
        let router = self
            .agent_jobs
            .current()?
            .ok_or(Error::Denied("constructor wake needs an Agent jobs router"))?;
        let recipient = router.wake_recipient(ctx)?.ok_or(Error::Denied(
            "current Agent has no bounded automatic-work wake authority",
        ))?;
        Ok(Some(InitialWake { recipient, rules }))
    }

    fn revalidate_initial_wake(&self, ctx: &ToolContext, wake: &InitialWake) -> Result<()> {
        if !self.allow_wake {
            return Err(Error::Denied("constructor wake capability was revoked"));
        }
        let router = self
            .agent_jobs
            .current()?
            .ok_or(Error::Denied("constructor wake router was removed"))?;
        router.revalidate_wake_recipient(ctx, &wake.recipient)
    }

    pub(crate) fn job_list_limit(&self) -> usize {
        self.tool_limits
            .jobs
            .max_exposed_jobs
            .min(aw_tools::filesystem::job_manager::MAX_JOB_LIST_PAGE)
    }
    pub(crate) fn result_limit(&self) -> usize {
        self.tool_limits.result_bytes
    }

    pub(crate) fn node_tools(&self) -> Vec<aw_tools::filesystem::batch::NodeTool> {
        let mut tools = self
            .native_tools
            .iter()
            .copied()
            .map(aw_tools::filesystem::batch::NodeTool::Native)
            .collect::<Vec<_>>();
        tools.extend(self.shells.node_tools());
        if self.allow_agent_graph {
            tools.push(aw_tools::filesystem::batch::NodeTool::Agent);
        }
        tools
    }

    pub(crate) fn validate_graph(&self, input: &Value) -> Result<()> {
        let host = self
            .state
            .lock()
            .map_err(|_| Error::Protocol("Job runtime state poisoned"))?
            .host
            .clone();
        host.validate_graph(input)
    }

    fn current_host(&self, ctx: &ToolContext) -> Result<(u64, Arc<JobHost>)> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
        if !state.scope.matches_context(ctx.scope.as_ref()) {
            return Err(Error::Denied(
                "Job runtime is bound to another live Session; finish scope replacement first",
            ));
        }
        state.host.bind_execution_owner(ctx)?;
        Ok((state.generation, state.host.clone()))
    }

    fn runner_finished(&self, generation: u64, id: &str) {
        if let Ok(mut state) = self.state.lock() {
            if state.generation == generation {
                state.runners.remove(id);
            }
        }
    }

    fn spawn_runner(
        self: &Arc<Self>,
        generation: u64,
        host: Arc<JobHost>,
        ctx: &ToolContext,
        id: String,
        token: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<Result<Snapshot>>> {
        let ownership_scope = ctx.execution_scope.clone();
        let ownership_lease = ownership_scope
            .as_ref()
            .map(|scope| scope.acquire())
            .transpose()?;
        let mut runner_ctx = ctx.clone();
        runner_ctx.cancellation = token.clone();
        let host_for_task = host.clone();
        let runtime: Weak<Self> = Arc::downgrade(self);
        let runner_id = id.clone();
        Ok(tokio::spawn(async move {
            let _ownership_lease = ownership_lease;
            let run = AssertUnwindSafe(async {
                host_for_task
                    .before_graph_run(&runner_ctx, &runner_id)
                    .await?;
                host_for_task
                    .run_graph(&runner_ctx, &runner_id, token)
                    .await
            })
            .catch_unwind()
            .await;
            let result = match run {
                Ok(Ok(snapshot)) => Ok(snapshot),
                Ok(Err(error)) => {
                    let _ = host_for_task.settle_runner_abort(&runner_id);
                    Err(error)
                }
                Err(_) => {
                    let _ = host_for_task.settle_runner_abort(&runner_id);
                    Err(Error::Protocol(
                        "native Job runner panicked; effect state fenced",
                    ))
                }
            };
            host_for_task
                .deliver_finite_terminal_events(&runner_id)
                .await;
            if let Some(scope) = ownership_scope.as_ref() {
                match host_for_task.manager().runtime_snapshot(&runner_id) {
                    Ok(snapshot)
                        if snapshot.summary.is_some_and(|(primary, _, incomplete)| {
                            primary == aw_tools::filesystem::status::Primary::Unknown || incomplete
                        }) =>
                    {
                        scope.mark_unresolved();
                    }
                    Err(_) if result.is_err() => scope.mark_unresolved(),
                    _ => {}
                }
            }
            if let Some(runtime) = runtime.upgrade() {
                runtime.runner_finished(generation, &runner_id);
            }
            result
        }))
    }

    /// Admit and start one prepared Graph under the exact outer ToolContext.
    /// `expose=true` implements immediate durable/live handoff; optimistic batch
    /// starts hidden and may expose the same Job later.
    pub fn start_graph(
        self: &Arc<Self>,
        ctx: &ToolContext,
        ops: &Value,
        expose: bool,
    ) -> Result<StartedJob> {
        let (generation, host) = self.current_host(ctx)?;
        let prepared = host.prepare(ctx, ops)?;
        let token = CancellationToken::new();
        let id = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
            if state.generation != generation
                || !Arc::ptr_eq(&state.host, &host)
                || !state.scope.matches_context(ctx.scope.as_ref())
            {
                return Err(Error::Denied("Job Session changed during Graph preflight"));
            }
            let id = if expose {
                host.admit_exposed(prepared)?
            } else {
                host.admit(prepared)?
            };
            if state.runners.insert(id.clone(), token.clone()).is_some() {
                return Err(Error::Protocol("duplicate Job runner identity"));
            }
            id
        };

        let task = self.spawn_runner(generation, host.clone(), ctx, id.clone(), token)?;
        Ok(StartedJob { id, host, task })
    }

    fn start_graph_with_initial_wake(
        self: &Arc<Self>,
        ctx: &ToolContext,
        ops: &Value,
        expose: bool,
        wake: InitialWake,
    ) -> Result<StartedJob> {
        self.revalidate_initial_wake(ctx, &wake)?;
        let (generation, host) = self.current_host(ctx)?;
        let prepared = host.prepare(ctx, ops)?;
        let reservation = host.reserve_graph_admission(ctx, expose)?;
        let token = CancellationToken::new();
        let id = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
            if state.generation != generation
                || !Arc::ptr_eq(&state.host, &host)
                || !state.scope.matches_context(ctx.scope.as_ref())
            {
                drop(state);
                let _ = host.release_graph_admission(&reservation);
                return Err(Error::Denied(
                    "Job Session changed during wake-enabled Graph admission",
                ));
            }
            self.revalidate_initial_wake(ctx, &wake)?;
            let id = host.admit_graph_reservation_with_wake(
                reservation,
                prepared,
                Some(&ctx.actor),
                Some((
                    &wake.recipient.canonical_ref,
                    &wake.recipient.authority_ref,
                    wake.rules.clone(),
                )),
            )?;
            if state.runners.insert(id.clone(), token.clone()).is_some() {
                let _ = host.revoke(&id);
                return Err(Error::Protocol("duplicate Job runner identity"));
            }
            id
        };
        let task = self.spawn_runner(generation, host.clone(), ctx, id.clone(), token)?;
        Ok(StartedJob { id, host, task })
    }

    /// Replay-safe model-facing Graph constructor. The live replay claim is
    /// committed before Job admission and before any root node can start.
    pub fn start_replay(
        self: &Arc<Self>,
        ctx: &ToolContext,
        ops: &Value,
        expose: bool,
    ) -> Result<StartDecision> {
        // A journaled AW invocation already has a durable outer ToolStarted
        // replay/fencing identity. Do not create a redundant per-Job replay
        // ledger entry for it; the same outer journal prevents redispatch after
        // process replacement or response loss. Memory-only callers have no
        // such authority and use the bounded live JobManager replay ledger.
        if ctx.scope.is_some() {
            return self.start_graph(ctx, ops, expose).map(StartDecision::New);
        }
        let (generation, host) = self.current_host(ctx)?;
        let prepared = host.prepare(ctx, ops)?;
        let namespace = format!(
            "job-create:{}:{}",
            ctx.scope
                .as_ref()
                .map_or_else(|| "memory".to_owned(), |scope| scope.session_id.clone()),
            ctx.scope
                .as_ref()
                .map_or_else(|| "main".to_owned(), |scope| scope.branch_id.clone())
        );
        let decision = {
            let state = self
                .state
                .lock()
                .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
            if state.generation != generation
                || !Arc::ptr_eq(&state.host, &host)
                || !state.scope.matches_context(ctx.scope.as_ref())
            {
                return Err(Error::Denied("Job Session changed during Graph preflight"));
            }
            if expose {
                host.admit_replay_exposed(prepared, &namespace, &ctx.call_id)?
            } else {
                host.admit_replay(prepared, &namespace, &ctx.call_id)?
            }
        };
        match decision {
            crate::job_host::AdmissionDecision::Unknown => Ok(StartDecision::Unknown),
            crate::job_host::AdmissionDecision::Existing(id) => {
                Ok(StartDecision::Existing { id, host })
            }
            crate::job_host::AdmissionDecision::New(id) => {
                let token = CancellationToken::new();
                {
                    let mut state = self
                        .state
                        .lock()
                        .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
                    if state.generation != generation || !Arc::ptr_eq(&state.host, &host) {
                        let _ = host.revoke(&id);
                        return Err(Error::Denied("Job Session changed during replay admission"));
                    }
                    if state.runners.insert(id.clone(), token.clone()).is_some() {
                        return Err(Error::Protocol("duplicate Job runner identity"));
                    }
                }
                let task = self.spawn_runner(generation, host.clone(), ctx, id.clone(), token)?;
                Ok(StartDecision::New(StartedJob { id, host, task }))
            }
        }
    }

    pub(crate) fn start_replay_with_initial_wake(
        self: &Arc<Self>,
        ctx: &ToolContext,
        ops: &Value,
        expose: bool,
        wake: InitialWake,
    ) -> Result<StartDecision> {
        self.revalidate_initial_wake(ctx, &wake)?;
        if ctx.scope.is_some() {
            return self
                .start_graph_with_initial_wake(ctx, ops, expose, wake)
                .map(StartDecision::New);
        }

        let (generation, host) = self.current_host(ctx)?;
        let prepared = host.prepare(ctx, ops)?;
        let namespace = format!(
            "job-create:{}:{}",
            ctx.scope
                .as_ref()
                .map_or_else(|| "memory".to_owned(), |scope| scope.session_id.clone()),
            ctx.scope
                .as_ref()
                .map_or_else(|| "main".to_owned(), |scope| scope.branch_id.clone())
        );
        self.revalidate_initial_wake(ctx, &wake)?;
        let decision =
            host.admit_replay_with_initial_wake(crate::job_host::InitialWakeAdmissionRequest {
                prepared,
                namespace: &namespace,
                request_id: &ctx.call_id,
                reserve_exposed: expose,
                actor: &ctx.actor,
                recipient_ref: &wake.recipient.canonical_ref,
                recipient_authority_ref: &wake.recipient.authority_ref,
                rules: wake.rules.clone(),
            })?;
        match decision {
            crate::job_host::AdmissionDecision::Unknown => Ok(StartDecision::Unknown),
            crate::job_host::AdmissionDecision::Existing(id) => {
                Ok(StartDecision::Existing { id, host })
            }
            crate::job_host::AdmissionDecision::New(id) => {
                let token = CancellationToken::new();
                {
                    let mut state = self
                        .state
                        .lock()
                        .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
                    if state.generation != generation
                        || !Arc::ptr_eq(&state.host, &host)
                        || !state.scope.matches_context(ctx.scope.as_ref())
                    {
                        let _ = host.revoke(&id);
                        return Err(Error::Denied(
                            "Job Session changed during wake replay admission",
                        ));
                    }
                    if state.runners.insert(id.clone(), token.clone()).is_some() {
                        return Err(Error::Protocol("duplicate Job runner identity"));
                    }
                }
                let task = self.spawn_runner(generation, host.clone(), ctx, id.clone(), token)?;
                Ok(StartDecision::New(StartedJob { id, host, task }))
            }
        }
    }

    /// Batch-only advanced admission. Durable AW policy may revise arguments
    /// before Graph sealing while the final Job ID/capacity/exposure position is
    /// already reserved. Local/no-router and memory-only callers preserve the
    /// existing start_replay path exactly.
    pub async fn start_replay_presealed(
        self: &Arc<Self>,
        ctx: &ToolContext,
        ops: &Value,
        expose: bool,
    ) -> Result<StartDecision> {
        self.start_replay_presealed_with_initial_wake(ctx, ops, expose, None)
            .await
    }

    pub(crate) async fn start_replay_presealed_with_initial_wake(
        self: &Arc<Self>,
        ctx: &ToolContext,
        ops: &Value,
        expose: bool,
        initial_wake: Option<InitialWake>,
    ) -> Result<StartDecision> {
        if let Some(wake) = initial_wake.as_ref() {
            self.revalidate_initial_wake(ctx, wake)?;
        }
        let Some(router) = self.graph_policy.current()? else {
            return match initial_wake {
                Some(wake) => self.start_replay_with_initial_wake(ctx, ops, expose, wake),
                None => self.start_replay(ctx, ops, expose),
            };
        };
        if ctx.scope.is_none() {
            return match initial_wake {
                Some(wake) => self.start_replay_with_initial_wake(ctx, ops, expose, wake),
                None => self.start_replay(ctx, ops, expose),
            };
        }

        let (generation, host) = self.current_host(ctx)?;
        let draft = host.prepare(ctx, ops)?;
        let reservation = host.reserve_graph_admission(ctx, expose)?;
        let request = match host.graph_preseal_request(reservation.id(), ops, &draft) {
            Ok(request) => request,
            Err(error) => {
                let _ = host.release_graph_admission(&reservation);
                return Err(error);
            }
        };
        let decision = match router.before_seal(ctx, request).await {
            Ok(decision) => decision,
            Err(error) => {
                let _ = host.release_graph_admission(&reservation);
                return Err(error);
            }
        };
        if let Err(error) =
            crate::operation_router::validate_preseal_graph_revision(ops, &decision.graph)
        {
            let _ = router.abandon_preseal(reservation.id());
            let _ = host.release_graph_admission(&reservation);
            return Err(error);
        }

        let mut overrides = BTreeMap::new();
        let node_count = draft.node_count();
        for override_footprint in decision.footprint_overrides {
            if override_footprint.coordinate == 0
                || override_footprint.coordinate as usize > node_count
                || overrides
                    .insert(override_footprint.coordinate, override_footprint.footprint)
                    .is_some()
            {
                let _ = router.abandon_preseal(reservation.id());
                let _ = host.release_graph_admission(&reservation);
                return Err(Error::Denied(
                    "pre-seal Graph footprint overrides must be unique valid coordinates",
                ));
            }
        }

        let prepared = match host.prepare_with_footprints(ctx, &decision.graph, &overrides) {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = router.abandon_preseal(reservation.id());
                let _ = host.release_graph_admission(&reservation);
                return Err(error);
            }
        };
        let token = CancellationToken::new();
        let id = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
            if state.generation != generation
                || !Arc::ptr_eq(&state.host, &host)
                || !state.scope.matches_context(ctx.scope.as_ref())
            {
                drop(state);
                let _ = router.abandon_preseal(reservation.id());
                let _ = host.release_graph_admission(&reservation);
                return Err(Error::Denied(
                    "Job Session changed during pre-seal Graph admission",
                ));
            }
            let reservation_id = reservation.id().to_owned();
            if let Some(wake) = initial_wake.as_ref() {
                if let Err(error) = self.revalidate_initial_wake(ctx, wake) {
                    drop(state);
                    let _ = router.abandon_preseal(&reservation_id);
                    let _ = host.release_graph_admission(&reservation);
                    return Err(error);
                }
            }
            let admitted = match initial_wake.as_ref() {
                Some(wake) => host.admit_graph_reservation_with_wake(
                    reservation,
                    prepared,
                    Some(&ctx.actor),
                    Some((
                        &wake.recipient.canonical_ref,
                        &wake.recipient.authority_ref,
                        wake.rules.clone(),
                    )),
                ),
                None => host.admit_graph_reservation(reservation, prepared),
            };
            let id = match admitted {
                Ok(id) => id,
                Err(error) => {
                    let _ = router.abandon_preseal(&reservation_id);
                    return Err(error);
                }
            };
            if state.runners.insert(id.clone(), token.clone()).is_some() {
                let _ = router.abandon_preseal(&id);
                let _ = host.revoke(&id);
                return Err(Error::Protocol("duplicate Job runner identity"));
            }
            id
        };
        let task = self.spawn_runner(generation, host.clone(), ctx, id.clone(), token)?;
        Ok(StartDecision::New(StartedJob { id, host, task }))
    }

    pub fn cancel(&self, ctx: &ToolContext, id: &str) -> Result<(bool, Snapshot)> {
        let (_, host) = self.current_host(ctx)?;
        let before = host
            .manager()
            .get(id)
            .map_err(|_| Error::Denied("Job is not exposed or missing"))?;
        host.cancel(id)?;
        let token = self
            .state
            .lock()
            .map_err(|_| Error::Protocol("Job runtime state poisoned"))?
            .runners
            .get(id)
            .cloned();
        if let Some(token) = token {
            token.cancel();
        }
        let after = host
            .manager()
            .get(id)
            .map_err(|_| Error::Protocol("Job disappeared after cancellation"))?;
        Ok((after.sequence != before.sequence, after))
    }

    pub async fn cancel_checked(
        &self,
        ctx: &ToolContext,
        id: &str,
        expected_revision: Option<u64>,
    ) -> Result<(bool, Snapshot)> {
        let (_, host) = self.current_host(ctx)?;
        let (changed, after) = host.cancel_control(id, expected_revision).await?;
        if changed {
            let token = self
                .state
                .lock()
                .map_err(|_| Error::Protocol("Job runtime state poisoned"))?
                .runners
                .get(id)
                .cloned();
            if let Some(token) = token {
                token.cancel();
            }
        }
        Ok((changed, after))
    }

    pub fn restart_graph(
        self: &Arc<Self>,
        ctx: &ToolContext,
        source_id: &str,
    ) -> Result<(crate::job_host::AdmissionDecision, Option<StartedJob>)> {
        let (generation, host) = self.current_host(ctx)?;
        let decision = host.restart(ctx, source_id)?;
        let id = match &decision {
            crate::job_host::AdmissionDecision::New(id) => id.clone(),
            _ => return Ok((decision, None)),
        };
        let token = CancellationToken::new();
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
            if state.generation != generation || !Arc::ptr_eq(&state.host, &host) {
                let _ = host.revoke(&id);
                return Err(Error::Denied(
                    "Job Session changed during restart admission",
                ));
            }
            if state.runners.insert(id.clone(), token.clone()).is_some() {
                return Err(Error::Protocol("duplicate restarted Job runner identity"));
            }
        }
        let task = self.spawn_runner(generation, host.clone(), ctx, id.clone(), token)?;
        Ok((
            decision,
            Some(StartedJob {
                id: id.clone(),
                host,
                task,
            }),
        ))
    }

    pub fn host_for(&self, ctx: &ToolContext) -> Result<Arc<JobHost>> {
        self.current_host(ctx).map(|(_, host)| host)
    }

    #[cfg(test)]
    pub(crate) fn test_scope_identity(&self) -> (String, String) {
        let state = self.state.lock().expect("Job runtime state poisoned");
        (
            state.scope.session().to_owned(),
            state.scope.branch().to_owned(),
        )
    }
}

impl aw_agent_core::ToolPolicyLifecycle for JobRuntime {
    fn validate_parallelism(
        &self,
        next: aw_agent_core::ToolParallelism,
    ) -> aw_agent_core::Result<()> {
        next.validate()?;
        let state = self
            .state
            .lock()
            .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
        if !state.runners.is_empty()
            || state
                .host
                .manager()
                .has_nonterminal_jobs()
                .map_err(|_| Error::Protocol("cannot inspect live Job session"))?
        {
            return Err(Error::Conflict(
                "tool parallel policy cannot change while Jobs are live",
            ));
        }
        Ok(())
    }

    fn apply_parallelism(&self, next: aw_agent_core::ToolParallelism) {
        self.parallelism.apply(next);
    }
}

impl InvocationScopeLifecycle for JobRuntime {
    fn replace_scope(
        &self,
        previous: Option<&InvocationScope>,
        next: Option<&InvocationScope>,
    ) -> aw_agent_core::Result<()> {
        if previous == next {
            return Ok(());
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
        for (id, token) in &state.runners {
            let _ = state.host.revoke(id);
            token.cancel();
        }
        if !state.runners.is_empty()
            || state
                .host
                .manager()
                .has_nonterminal_jobs()
                .map_err(|_| Error::Protocol("cannot inspect live Job session"))?
        {
            return Err(Error::Conflict(
                "live Jobs are cancelling for Session replacement; retry after they settle",
            ));
        }
        let scope = match next {
            Some(scope) => LiveScope::Journal(scope.clone()),
            None => LiveScope::memory()?,
        };
        let host = self.build_host(&scope)?;
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or(Error::Limit("Job runtime scope generations exhausted"))?;
        state.scope = scope;
        state.host = host;
        state.runners.clear();
        state.control_replays.clear();
        Ok(())
    }
}

impl Drop for JobRuntime {
    fn drop(&mut self) {
        if let Ok(state) = self.state.get_mut() {
            for (id, token) in &state.runners {
                let _ = state.host.revoke(id);
                token.cancel();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{configure, LocalToolOptions, ToolMode};
    use aw_agent_core::{
        Agent, AgentOptions, CompletionFuture, DeltaSink, Message, PrincipalId, Provider,
        ToolEffect, ToolSource,
    };
    use std::fs;
    #[cfg(unix)]
    use std::path::PathBuf;

    struct UnusedProvider;
    impl Provider for UnusedProvider {
        fn stream<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            _sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("provider unused")) })
        }
    }

    #[derive(Default)]
    struct ReplayRouter {
        parent: std::sync::Mutex<String>,
        seen_refs: std::sync::Mutex<Vec<String>>,
        subscribe_calls: std::sync::atomic::AtomicUsize,
        deliveries: std::sync::Mutex<Vec<aw_tools::job_event::JobEventDelivery>>,
        recipient: String,
    }

    impl crate::agent_jobs::AgentJobRouter for ReplayRouter {
        fn resolve_control_ref(
            &self,
            _ctx: &ToolContext,
            reference: &str,
        ) -> Result<Option<String>> {
            self.seen_refs.lock().unwrap().push(reference.to_owned());
            if reference == "^" {
                return Ok(Some(self.parent.lock().unwrap().clone()));
            }
            if reference.starts_with("/d:") {
                return Ok(Some(reference.to_owned()));
            }
            Ok(None)
        }

        fn wake_recipient(
            &self,
            _ctx: &ToolContext,
        ) -> Result<Option<crate::agent_jobs::AgentWakeRecipient>> {
            Ok(Some(crate::agent_jobs::AgentWakeRecipient {
                canonical_ref: self.recipient.clone(),
                authority_ref: "jwa_fixture".into(),
            }))
        }

        fn revalidate_wake_recipient(
            &self,
            _ctx: &ToolContext,
            recipient: &crate::agent_jobs::AgentWakeRecipient,
        ) -> Result<()> {
            if recipient.canonical_ref != self.recipient || recipient.authority_ref != "jwa_fixture"
            {
                return Err(Error::Denied("fixture wake admission changed"));
            }
            Ok(())
        }

        fn get<'a>(
            &'a self,
            _ctx: &'a ToolContext,
            _request: crate::agent_jobs::AgentJobGet,
        ) -> crate::agent_jobs::AgentJobFuture<'a> {
            Box::pin(async { Ok(None) })
        }
        fn read<'a>(
            &'a self,
            _ctx: &'a ToolContext,
            _request: crate::agent_jobs::AgentJobRead,
        ) -> crate::agent_jobs::AgentJobFuture<'a> {
            Box::pin(async { Ok(None) })
        }
        fn send<'a>(
            &'a self,
            _ctx: &'a ToolContext,
            _request: crate::agent_jobs::AgentJobSend,
        ) -> crate::agent_jobs::AgentJobFuture<'a> {
            Box::pin(async { Ok(None) })
        }
        fn cancel<'a>(
            &'a self,
            _ctx: &'a ToolContext,
            _request: crate::agent_jobs::AgentJobCancel,
        ) -> crate::agent_jobs::AgentJobFuture<'a> {
            Box::pin(async { Ok(None) })
        }
        fn admit_event<'a>(
            &'a self,
            delivery: aw_tools::job_event::JobEventDelivery,
        ) -> crate::agent_jobs::AgentEventFuture<'a> {
            self.deliveries.lock().unwrap().push(delivery);
            Box::pin(async { Ok(()) })
        }

        fn subscribe<'a>(
            &'a self,
            _ctx: &'a ToolContext,
            request: crate::agent_jobs::AgentJobSubscribe,
        ) -> crate::agent_jobs::AgentJobFuture<'a> {
            self.subscribe_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async move {
                Ok(Some(aw_agent_core::ToolOutput::text(format!(
                    "{} subscription=active generation=7",
                    request.reference
                ))))
            })
        }
    }

    #[tokio::test]
    async fn jobs_subscribe_replay_claim_precedes_parent_ref_reresolution() {
        use aw_agent_core::Tool as _;
        let root =
            std::env::temp_dir().join(format!("aw-jobs-subscribe-replay-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let agent = configure(
            Agent::builder(
                Arc::new(UnusedProvider),
                AgentOptions::new("fixture", "model"),
            ),
            actor.clone(),
            &LocalToolOptions::new(&root, ToolMode::ReadOnly),
        )
        .unwrap()
        .build()
        .unwrap();

        let router = Arc::new(ReplayRouter {
            parent: std::sync::Mutex::new("/d:0/i:0/p:0/a:parent-a".into()),
            seen_refs: std::sync::Mutex::new(Vec::new()),
            subscribe_calls: std::sync::atomic::AtomicUsize::new(0),
            deliveries: std::sync::Mutex::new(Vec::new()),
            recipient: "/d:0/i:0/p:0/a:child".into(),
        });
        let router_slot = Arc::new(crate::agent_jobs::AgentJobRouterSlot::default());
        router_slot.replace(router.clone()).unwrap();
        let runtime = JobRuntime::new_with_native_tools_bounded(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            crate::LocalToolLimits::default(),
            crate::LocalToolConcurrency::default(),
            JobRuntimeOptions {
                agent_jobs: router_slot,
                ..Default::default()
            },
            vec![aw_tools::filesystem::schema::Tool::Read],
        )
        .unwrap();
        let ctx = ToolContext {
            actor: actor.clone(),
            turn_id: 7,
            call_id: "subscribe-replay-call".into(),
            source: ToolSource::trusted_host(),
            effect: ToolEffect::Control,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: agent.default_identity().unwrap(),
            scope: None,
            execution_owner: Some(
                aw_agent_core::AgentExecutionBinding::from_trusted_host(
                    "/d:0/i:0/p:0/a:child",
                    "fixture-child",
                    "main",
                )
                .unwrap(),
            ),
            execution_scope: None,
        };
        let tool =
            crate::jobs_tools::JobsTool::new(runtime, crate::jobs_tools::JobsKind::Subscribe);
        let arguments = serde_json::json!({
            "ref":"^",
            "events":[["result","queue"]]
        });
        let first = tool.execute(ctx.clone(), &arguments).await.unwrap();
        assert!(first.content.contains("parent-a"));
        *router.parent.lock().unwrap() = "/d:0/i:0/p:0/a:parent-b".into();

        let replay = tool.execute(ctx, &arguments).await.unwrap();
        assert_eq!(replay, first);
        assert_eq!(
            router
                .subscribe_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(
            router.seen_refs.lock().unwrap().as_slice(),
            ["^".to_owned(), "/d:0/i:0/p:0/a:parent-a".to_owned()],
            "replay may reauthorize only the stored canonical target, never re-resolve ^"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn finite_terminal_checkpoint_commits_result_and_settled_before_delivery() {
        let root =
            std::env::temp_dir().join(format!("aw-job-event-terminal-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.txt"), "terminal\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let agent = configure(
            Agent::builder(
                Arc::new(UnusedProvider),
                AgentOptions::new("fixture", "model"),
            ),
            actor.clone(),
            &LocalToolOptions::new(&root, ToolMode::ReadOnly),
        )
        .unwrap()
        .build()
        .unwrap();
        let router = Arc::new(ReplayRouter {
            parent: std::sync::Mutex::new("/d:0/i:0/p:0/a:parent".into()),
            seen_refs: std::sync::Mutex::new(Vec::new()),
            subscribe_calls: std::sync::atomic::AtomicUsize::new(0),
            deliveries: std::sync::Mutex::new(Vec::new()),
            recipient: "/d:0/i:0/p:0/a:child".into(),
        });
        let router_slot = Arc::new(crate::agent_jobs::AgentJobRouterSlot::default());
        router_slot.replace(router.clone()).unwrap();
        let runtime = JobRuntime::new_with_native_tools_bounded(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            crate::LocalToolLimits::default(),
            crate::LocalToolConcurrency::default(),
            JobRuntimeOptions {
                agent_jobs: router_slot,
                ..Default::default()
            },
            vec![aw_tools::filesystem::schema::Tool::Read],
        )
        .unwrap();
        let scope = InvocationScope {
            session_id: "fixture-source".into(),
            branch_id: "main".into(),
        };
        runtime.replace_scope(None, Some(&scope)).unwrap();
        let ctx = ToolContext {
            actor: actor.clone(),
            turn_id: 1,
            call_id: "event-source".into(),
            source: ToolSource::trusted_host(),
            effect: ToolEffect::Read,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: agent.default_identity().unwrap(),
            scope: Some(scope),
            execution_owner: Some(
                aw_agent_core::AgentExecutionBinding::from_trusted_host(
                    "/d:0/i:0/p:0/a:0",
                    "fixture-source",
                    "main",
                )
                .unwrap(),
            ),
            execution_scope: None,
        };
        let host = runtime.host_for(&ctx).unwrap();
        let graph = serde_json::json!({
            "ops":[["read",{"path":"a.txt","numbered":false}]]
        });
        let prepared = host.prepare(&ctx, &graph).unwrap();
        let id = host.admit_exposed(prepared).unwrap();
        let rules = aw_tools::job_event::EventRuleSet::new([
            aw_tools::job_event::EventRule::new(
                aw_tools::job_event::JobEventKind::Result,
                aw_tools::job_event::JobWakeMode::Auto,
            ),
            aw_tools::job_event::EventRule::new(
                aw_tools::job_event::JobEventKind::Settled,
                aw_tools::job_event::JobWakeMode::Queue,
            ),
        ])
        .unwrap();
        let (_, update) = host
            .subscribe_finite(crate::job_host::FiniteSubscriptionRequest {
                actor: &actor,
                id: &id,
                node: None,
                expected_revision: None,
                recipient_ref: &router.recipient,
                recipient_authority_ref: "jwa_fixture",
                rules,
            })
            .unwrap();
        assert!(update.active);
        let snapshot = host
            .run_graph(&ctx, &id, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            snapshot.lifecycle,
            aw_tools::filesystem::job::Lifecycle::Terminal
        );
        host.deliver_finite_terminal_events(&id).await;
        let deliveries = router.deliveries.lock().unwrap().clone();
        assert_eq!(deliveries.len(), 2);
        assert_eq!(deliveries[0].event_rev, deliveries[1].event_rev);
        assert_eq!(deliveries[0].subscription_generation, update.generation);
        assert_eq!(deliveries[1].subscription_generation, update.generation);
        assert!(
            host.commit_finite_terminal_events(&id).unwrap().is_empty(),
            "terminal source checkpoint must commit only once"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn constructor_wake_precedes_fast_roots_and_is_replay_semantics() {
        let root = std::env::temp_dir().join(format!("aw-constructor-wake-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("fast.txt"), "fast\n").unwrap();

        let actor = PrincipalId::new("local").unwrap();
        let agent = configure(
            Agent::builder(
                Arc::new(UnusedProvider),
                AgentOptions::new("fixture", "model"),
            ),
            actor.clone(),
            &LocalToolOptions::new(&root, ToolMode::ReadOnly),
        )
        .unwrap()
        .build()
        .unwrap();
        let router = Arc::new(ReplayRouter {
            parent: std::sync::Mutex::new("/d:0/i:0/p:0/a:parent".into()),
            seen_refs: std::sync::Mutex::new(Vec::new()),
            subscribe_calls: std::sync::atomic::AtomicUsize::new(0),
            deliveries: std::sync::Mutex::new(Vec::new()),
            recipient: "/d:0/i:0/p:0/a:recipient".into(),
        });
        let slot = Arc::new(crate::agent_jobs::AgentJobRouterSlot::default());
        slot.replace(router.clone()).unwrap();
        let runtime = JobRuntime::new_with_native_tools_bounded(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            crate::LocalToolLimits::default(),
            crate::LocalToolConcurrency::default(),
            JobRuntimeOptions {
                agent_jobs: slot,
                allow_wake: true,
                ..Default::default()
            },
            vec![aw_tools::filesystem::schema::Tool::Read],
        )
        .unwrap();
        let (runtime_session, runtime_branch) = {
            let state = runtime.state.lock().unwrap();
            (
                state.scope.session().to_owned(),
                state.scope.branch().to_owned(),
            )
        };
        let ctx = ToolContext {
            actor,
            turn_id: 1,
            call_id: "constructor-wake".into(),
            source: ToolSource::trusted_host(),
            effect: ToolEffect::Control,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: agent.default_identity().unwrap(),
            scope: None,
            execution_owner: Some(
                aw_agent_core::AgentExecutionBinding::from_trusted_host(
                    "/d:0/i:0/p:0/a:0",
                    runtime_session,
                    runtime_branch,
                )
                .unwrap(),
            ),
            execution_scope: None,
        };
        let graph = serde_json::json!({
            "ops":[["read",{"path":"fast.txt","numbered":false}]]
        });
        let rules = aw_tools::job_event::EventRuleSet::new([
            aw_tools::job_event::EventRule::new(
                aw_tools::job_event::JobEventKind::Result,
                aw_tools::job_event::JobWakeMode::Queue,
            ),
            aw_tools::job_event::EventRule::new(
                aw_tools::job_event::JobEventKind::Settled,
                aw_tools::job_event::JobWakeMode::Auto,
            ),
        ])
        .unwrap();
        let wake = runtime
            .prepare_initial_wake(&ctx, rules.clone())
            .unwrap()
            .unwrap();
        let first = runtime
            .start_replay_with_initial_wake(&ctx, &graph, false, wake)
            .unwrap();
        let StartDecision::New(started) = first else {
            panic!("first constructor wake must create one Job")
        };
        let first_id = started.id.clone();
        let snapshot = started.task.await.unwrap().unwrap();
        assert_eq!(
            snapshot.lifecycle,
            aw_tools::filesystem::job::Lifecycle::Terminal
        );
        let deliveries = router.deliveries.lock().unwrap().clone();
        assert_eq!(deliveries.len(), 2);
        assert_eq!(deliveries[0].event_rev, deliveries[1].event_rev);
        assert!(deliveries
            .iter()
            .all(|delivery| delivery.recipient_ref == router.recipient));

        let same = runtime.prepare_initial_wake(&ctx, rules).unwrap().unwrap();
        match runtime
            .start_replay_with_initial_wake(&ctx, &graph, false, same)
            .unwrap()
        {
            StartDecision::Existing { id, .. } => assert_eq!(id, first_id),
            _ => panic!("matching constructor replay must return the same Job"),
        }
        assert_eq!(
            router.deliveries.lock().unwrap().len(),
            2,
            "matching replay must not fire terminal events again"
        );

        let changed_rules =
            aw_tools::job_event::EventRuleSet::new([aw_tools::job_event::EventRule::new(
                aw_tools::job_event::JobEventKind::Result,
                aw_tools::job_event::JobWakeMode::Queue,
            )])
            .unwrap();
        let changed = runtime
            .prepare_initial_wake(&ctx, changed_rules)
            .unwrap()
            .unwrap();
        assert!(
            runtime
                .start_replay_with_initial_wake(&ctx, &graph, false, changed)
                .is_err(),
            "wake rules are constructor replay semantics"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn failed_immediate_exposure_does_not_leave_hidden_nonterminal_job() {
        let root = std::env::temp_dir().join(format!(
            "aw-job-runtime-exposure-capacity-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.txt"), "a\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        );
        let tools = LocalToolOptions::new(&root, ToolMode::ReadOnly);
        let agent = configure(builder, actor.clone(), &tools)
            .unwrap()
            .build()
            .unwrap();
        let runtime = JobRuntime::new(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            Limits {
                max_retained_jobs: 3,
                max_exposed_jobs: 1,
                max_hidden_jobs: 2,
                ..Limits::default()
            },
        )
        .unwrap();
        let (runtime_session, runtime_branch) = runtime.test_scope_identity();
        let ctx = ToolContext {
            actor,
            turn_id: 1,
            call_id: "exposure-capacity".into(),
            source: ToolSource::trusted_host(),
            effect: ToolEffect::Read,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            identity: agent.default_identity().unwrap(),
            scope: None,
            execution_owner: Some(
                aw_agent_core::AgentExecutionBinding::from_trusted_host(
                    "/d:0/i:0/p:0/a:0",
                    runtime_session,
                    runtime_branch,
                )
                .unwrap(),
            ),
            result_budget_bytes: crate::LocalToolLimits::default().result_bytes,
            execution_scope: None,
        };
        let graph = serde_json::json!({"ops":[["read",{"path":"a.txt","numbered":false}]]});
        let host = runtime.host_for(&ctx).unwrap();
        let first = host.prepare(&ctx, &graph).unwrap();
        let first_id = host.admit(first).unwrap();
        host.expose(&first_id).unwrap();

        assert!(runtime.start_graph(&ctx, &graph, true).is_err());
        host.run_graph(&ctx, &first_id, CancellationToken::new())
            .await
            .unwrap();
        assert!(
            !host.manager().has_nonterminal_jobs().unwrap(),
            "failed exposed admission must not leave an unreachable hidden Job"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn memory_runtime_runs_hidden_job_and_scope_replacement_discards_terminal_namespace() {
        let root = std::env::temp_dir().join(format!("aw-job-runtime-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.txt"), "a\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        );
        let tools = LocalToolOptions::new(&root, ToolMode::ReadOnly);
        let agent = configure(builder, actor.clone(), &tools)
            .unwrap()
            .build()
            .unwrap();
        let runtime = JobRuntime::new(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            Limits::default(),
        )
        .unwrap();
        let (runtime_session, runtime_branch) = runtime.test_scope_identity();
        let ctx = ToolContext {
            actor: actor.clone(),
            turn_id: 1,
            call_id: "batch".into(),
            source: ToolSource::trusted_host(),
            effect: ToolEffect::Unknown,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: agent.default_identity().unwrap(),
            scope: None,
            execution_owner: Some(
                aw_agent_core::AgentExecutionBinding::from_trusted_host(
                    "/d:0/i:0/p:0/a:0",
                    runtime_session,
                    runtime_branch,
                )
                .unwrap(),
            ),
            execution_scope: None,
        };
        let started = runtime
            .start_graph(
                &ctx,
                &serde_json::json!({"ops":[["read",{"path":"a.txt","numbered":false}]]}),
                false,
            )
            .unwrap();
        let snapshot = started.task.await.unwrap().unwrap();
        assert_eq!(snapshot.finished, 1);
        assert!(started.host.manager().get(&started.id).is_err());
        runtime
            .replace_scope(
                None,
                Some(&InvocationScope {
                    session_id: "s".into(),
                    branch_id: "main".into(),
                }),
            )
            .unwrap();
        assert!(runtime.host_for(&ctx).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn scope_replacement_cancels_live_runner_tokens_and_requires_retry() {
        let root =
            std::env::temp_dir().join(format!("aw-job-runtime-switch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        );
        let tools = LocalToolOptions::new(&root, ToolMode::ReadOnly);
        let agent = configure(builder, actor, &tools).unwrap().build().unwrap();
        let runtime = JobRuntime::new(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            Limits::default(),
        )
        .unwrap();
        let token = CancellationToken::new();
        runtime
            .state
            .lock()
            .unwrap()
            .runners
            .insert("fixture".into(), token.clone());
        let next = InvocationScope {
            session_id: "s".into(),
            branch_id: "main".into(),
        };
        assert!(runtime.replace_scope(None, Some(&next)).is_err());
        assert!(token.is_cancelled());
        {
            let mut state = runtime.state.lock().unwrap();
            assert!(matches!(state.scope, LiveScope::Memory { .. }));
            state.runners.remove("fixture");
        }
        runtime.replace_scope(None, Some(&next)).unwrap();
        assert!(matches!(
            runtime.state.lock().unwrap().scope,
            LiveScope::Journal(ref scope) if scope == &next
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    fn bash_executable() -> PathBuf {
        std::env::var_os("AW_BASH")
            .map(PathBuf::from)
            .or_else(|| {
                ["/bin/bash", "/usr/bin/bash"]
                    .iter()
                    .map(PathBuf::from)
                    .find(|path| path.is_file())
            })
            .expect("shell runtime tests need AW_BASH")
    }

    #[cfg(unix)]
    fn shell_runtime_fixture() -> (PathBuf, Agent, Arc<JobRuntime>, ToolContext) {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let suffix = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let root = std::env::temp_dir().join(format!(
            "aw-shell-job-runtime-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.txt"), "native\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        );
        let mut tools = LocalToolOptions::new(&root, ToolMode::Trusted);
        tools.bash = Some(bash_executable());
        let agent = configure(builder, actor.clone(), &tools)
            .unwrap()
            .build()
            .unwrap();
        let runtime = JobRuntime::new(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            Limits::default(),
        )
        .unwrap();
        runtime
            .register_shell(Arc::new(
                shell::NativeShell::new(root.clone(), bash_executable()).unwrap(),
            ))
            .unwrap();
        let (runtime_session, runtime_branch) = runtime.test_scope_identity();
        let ctx = ToolContext {
            actor: actor.clone(),
            turn_id: 1,
            call_id: "shell-job".into(),
            source: ToolSource::trusted_host(),
            effect: ToolEffect::Unknown,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: agent.default_identity().unwrap(),
            scope: None,
            execution_owner: Some(
                aw_agent_core::AgentExecutionBinding::from_trusted_host(
                    "/d:0/i:0/p:0/a:0",
                    runtime_session,
                    runtime_branch,
                )
                .unwrap(),
            ),
            execution_scope: None,
        };
        (root, agent, runtime, ctx)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn mixed_native_and_shell_graph_uses_one_job_and_separate_shell_streams() {
        use aw_tools::filesystem::job_manager::OutputStream;
        let (root, _agent, runtime, ctx) = shell_runtime_fixture();
        let started = runtime
            .start_graph(
                &ctx,
                &serde_json::json!({"ops":[
                    ["read",{"path":"a.txt","numbered":false}],
                    ["bash",{"command":"printf shell-out; printf shell-err >&2"},"1"]
                ]}),
                true,
            )
            .unwrap();
        let id = started.id.clone();
        let host = started.host.clone();
        let snapshot = started.task.await.unwrap().unwrap();
        assert_eq!(
            snapshot.summary,
            Some((aw_tools::filesystem::status::Primary::Ok, false, false))
        );
        assert_eq!(
            host.manager()
                .output_page(&id, 2, OutputStream::Stdout, 0, 64)
                .unwrap()
                .bytes,
            b"shell-out"
        );
        assert_eq!(
            host.manager()
                .output_page(&id, 2, OutputStream::Stderr, 0, 64)
                .unwrap()
                .bytes,
            b"shell-err"
        );
        let result = host
            .manager()
            .output_page(&id, 2, OutputStream::Result, 0, 512)
            .unwrap();
        let result = String::from_utf8(result.bytes).unwrap();
        assert!(result.starts_with("ok exit=0"), "{result}");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shell_node_nonzero_exit_is_graph_fail_not_protocol_retry() {
        let (root, _agent, runtime, ctx) = shell_runtime_fixture();
        use aw_tools::filesystem::job_manager::OutputStream;
        let started = runtime
            .start_graph(
                &ctx,
                &serde_json::json!({"ops":[["bash",{"command":"printf nope; exit 9"}]]}),
                true,
            )
            .unwrap();
        let id = started.id.clone();
        let host = started.host.clone();
        let snapshot = started.task.await.unwrap().unwrap();
        assert_eq!(
            snapshot.summary,
            Some((aw_tools::filesystem::status::Primary::Fail, false, false))
        );
        for stream in [
            OutputStream::Result,
            OutputStream::Stdout,
            OutputStream::Stderr,
        ] {
            let page = host.manager().output_page(&id, 1, stream, 0, 512).unwrap();
            assert!(page.writer_finished, "{stream:?} writer must be finished");
        }
        assert_eq!(
            host.manager()
                .output_page(&id, 1, OutputStream::Stdout, 0, 512)
                .unwrap()
                .bytes,
            b"nope"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shell_node_cancel_uses_process_stop_proof_and_settles_job() {
        let (root, _agent, runtime, mut ctx) = shell_runtime_fixture();
        ctx.call_id = "shell-cancel".into();
        let started = runtime
            .start_graph(
                &ctx,
                &serde_json::json!({"ops":[["bash",{"command":"while :; do :; done","timeout_secs":10}]]}),
                true,
            )
            .unwrap();
        let id = started.id.clone();
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        runtime.cancel(&ctx, &id).unwrap();
        let snapshot = tokio::time::timeout(std::time::Duration::from_secs(3), started.task)
            .await
            .expect("cancelled shell runner must settle")
            .unwrap()
            .unwrap();
        assert_eq!(
            snapshot.stop_reason,
            Some(aw_tools::filesystem::job::StopReason::Cancelled)
        );
        assert_eq!(
            snapshot.lifecycle,
            aw_tools::filesystem::job::Lifecycle::Terminal
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shell_stream_truncation_is_observability_not_graph_incomplete() {
        use aw_tools::filesystem::job_manager::OutputStream;
        let (root, _agent, runtime, ctx) = shell_runtime_fixture();
        let started = runtime
            .start_graph(
                &ctx,
                &serde_json::json!({"ops":[["bash",{
                    "command":"printf 123456789abcdef",
                    "max_output_bytes":8
                }]]}),
                true,
            )
            .unwrap();
        let id = started.id.clone();
        let host = started.host.clone();
        let snapshot = started.task.await.unwrap().unwrap();
        assert_eq!(
            snapshot.summary,
            Some((aw_tools::filesystem::status::Primary::Ok, false, false))
        );
        let stdout = host
            .manager()
            .output_page(&id, 1, OutputStream::Stdout, 0, 8)
            .unwrap();
        assert_eq!(stdout.bytes, b"12345678");
        assert!(stdout.truncated);
        assert_eq!(stdout.total_bytes, 15);
        assert_eq!(stdout.retained_bytes, 8);
        let result = host
            .manager()
            .output_page(&id, 1, OutputStream::Result, 0, 512)
            .unwrap();
        let result = String::from_utf8(result.bytes).unwrap();
        assert!(result.contains("stdout_truncated=true"), "{result}");
        assert!(result.contains("stdout=15"), "{result}");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shell_write_hint_conflict_orders_lower_shell_before_native_read() {
        use aw_tools::filesystem::job_manager::OutputStream;
        let (root, _agent, runtime, ctx) = shell_runtime_fixture();
        fs::write(root.join("x.txt"), "old").unwrap();
        let started = runtime
            .start_graph(
                &ctx,
                &serde_json::json!({"ops":[
                    ["bash",{
                        "command":"sleep 0.03; printf new > x.txt",
                        "writes":["x.txt"]
                    },"*"],
                    ["read",{"path":"x.txt","numbered":false},"*"]
                ]}),
                true,
            )
            .unwrap();
        let id = started.id.clone();
        let host = started.host.clone();
        let snapshot = started.task.await.unwrap().unwrap();
        assert_eq!(
            snapshot.summary,
            Some((aw_tools::filesystem::status::Primary::Ok, false, false))
        );
        assert_eq!(
            host.manager()
                .output_page(&id, 2, OutputStream::Result, 0, 16)
                .unwrap()
                .bytes,
            b"new"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn shell_registration_rechecks_max_graph_spool_capacity_before_publication() {
        let root = std::env::temp_dir().join(format!(
            "aw-job-runtime-shell-capacity-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        );
        let tools = LocalToolOptions::new(&root, ToolMode::ReadOnly);
        let agent = configure(builder, actor, &tools).unwrap().build().unwrap();
        // Native results need 6,291,456 bytes at the default 96 KiB/node;
        // adding 48 KiB stdout+stderr shell retention raises the max-Graph
        // requirement to 6,553,600 bytes.
        let tool_limits = crate::LocalToolLimits {
            jobs: Limits {
                max_spool_bytes: 6_400_000,
                ..Limits::default()
            },
            ..Default::default()
        };
        let runtime = JobRuntime::new_with_native_tools(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            tool_limits,
            crate::LocalToolConcurrency::default(),
            aw_tools::filesystem::schema::Tool::GRAPH_NATIVE.to_vec(),
        )
        .unwrap();
        let shell = Arc::new(shell::NativeShell::new(root.clone(), bash_executable()).unwrap());
        assert!(matches!(
            runtime.register_shell(shell),
            Err(Error::Config(_))
        ));
        assert!(!runtime
            .node_tools()
            .contains(&aw_tools::filesystem::batch::NodeTool::Bash));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn public_runtime_constructor_rejects_spool_too_small_for_one_max_graph() {
        let root =
            std::env::temp_dir().join(format!("aw-job-runtime-small-spool-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        );
        let tools = LocalToolOptions::new(&root, ToolMode::ReadOnly);
        let agent = configure(builder, actor, &tools).unwrap().build().unwrap();
        let result = JobRuntime::new(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            Limits {
                max_spool_bytes: 1024,
                ..Limits::default()
            },
        );
        assert!(matches!(result, Err(Error::Config(_))));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_job_runtime_refuses_parallel_policy_change_until_quiescent() {
        let root =
            std::env::temp_dir().join(format!("aw-job-runtime-policy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        );
        let tools = LocalToolOptions::new(&root, ToolMode::ReadOnly);
        let agent = configure(builder, actor, &tools).unwrap().build().unwrap();
        let runtime = JobRuntime::new(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            Limits::default(),
        )
        .unwrap();
        let token = CancellationToken::new();
        runtime
            .state
            .lock()
            .unwrap()
            .runners
            .insert("live".into(), token);
        let next = aw_agent_core::ToolParallelism {
            max_parallel_reads: 1,
            allow_parallel_shell: false,
            max_parallel_shell: 1,
        };
        assert!(aw_agent_core::ToolPolicyLifecycle::validate_parallelism(&*runtime, next).is_err());
        runtime.state.lock().unwrap().runners.remove("live");
        assert!(aw_agent_core::ToolPolicyLifecycle::validate_parallelism(&*runtime, next).is_ok());
        aw_agent_core::ToolPolicyLifecycle::apply_parallelism(&*runtime, next);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dropping_runtime_cancels_all_live_runner_tokens() {
        let root = std::env::temp_dir().join(format!("aw-job-runtime-drop-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        );
        let tools = LocalToolOptions::new(&root, ToolMode::ReadOnly);
        let agent = configure(builder, actor, &tools).unwrap().build().unwrap();
        let runtime = JobRuntime::new(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            Limits::default(),
        )
        .unwrap();
        let token = CancellationToken::new();
        runtime
            .state
            .lock()
            .unwrap()
            .runners
            .insert("live".into(), token.clone());
        drop(runtime);
        assert!(token.is_cancelled());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn exact_shell_grant_revocation_stops_running_process_and_marks_job_revoked() {
        let (root, mut agent, runtime, ctx) = shell_runtime_fixture();
        let started = runtime
            .start_graph(
                &ctx,
                &serde_json::json!({"ops":[["bash",{
                    "command":"while :; do :; done",
                    "timeout_secs":10
                }]]}),
                true,
            )
            .unwrap();
        let id = started.id.clone();
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        agent
            .revoke_registered_source_tool(
                &ctx.actor,
                "bash",
                &aw_agent_core::ToolSource::trusted_host(),
            )
            .unwrap();
        let snapshot = tokio::time::timeout(std::time::Duration::from_secs(3), started.task)
            .await
            .expect("revoked shell runner must terminate")
            .unwrap()
            .unwrap();
        assert_eq!(
            snapshot.stop_reason,
            Some(aw_tools::filesystem::job::StopReason::Revoked)
        );
        assert_eq!(
            snapshot.lifecycle,
            aw_tools::filesystem::job::Lifecycle::Terminal
        );
        assert!(runtime
            .host_for(&ctx)
            .unwrap()
            .manager()
            .render_job_status(&id)
            .unwrap()
            .starts_with("revoked bash exit=cancelled"));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn immediate_shell_job_cancel_before_runner_poll_never_launches_process() {
        let (root, _agent, runtime, ctx) = shell_runtime_fixture();
        let marker = root.join("launched.txt");
        let started = runtime
            .start_graph(
                &ctx,
                &serde_json::json!({"ops":[["bash",{
                    "command":"printf launched > launched.txt; sleep 1",
                    "writes":["launched.txt"]
                }]]}),
                true,
            )
            .unwrap();
        runtime.cancel(&ctx, &started.id).unwrap();
        let snapshot = started.task.await.unwrap().unwrap();
        assert_eq!(
            snapshot.stop_reason,
            Some(aw_tools::filesystem::job::StopReason::Cancelled)
        );
        assert!(
            !marker.exists(),
            "cancelled-before-runner shell must never spawn"
        );
        let _ = fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod graph_policy_tests {
    use super::*;
    use crate::{
        configure,
        operation_router::{
            GraphNodePolicyRequest, GraphPolicyFuture, GraphPolicyRequest, GraphPolicyRouter,
            GraphPolicyRouterSlot,
        },
        LocalToolConcurrency, LocalToolLimits, LocalToolOptions, ToolMode,
    };
    use aw_agent_core::{
        Agent, AgentOptions, CompletionFuture, DeltaSink, Message, PrincipalId, Provider,
        ToolEffect, ToolSource,
    };
    use aw_tools::filesystem::{job::NodeState, schema::Tool as FsTool};
    use std::{
        fs,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Mutex as StdMutex,
        },
    };
    use tokio::sync::Notify;

    struct UnusedProvider;
    impl Provider for UnusedProvider {
        fn stream<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            _sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("provider unused")) })
        }
    }

    struct BlockingPolicy {
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }
    impl GraphPolicyRouter for BlockingPolicy {
        fn before_run<'a>(
            &'a self,
            _ctx: &'a ToolContext,
            _request: GraphPolicyRequest,
        ) -> GraphPolicyFuture<'a> {
            Box::pin(async move {
                self.entered.notify_one();
                self.release.notified().await;
                Ok(())
            })
        }
    }

    struct BlockingNodePolicy {
        coordinate: u32,
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }
    impl GraphPolicyRouter for BlockingNodePolicy {
        fn before_node<'a>(
            &'a self,
            _ctx: &'a ToolContext,
            request: GraphNodePolicyRequest,
        ) -> GraphPolicyFuture<'a> {
            Box::pin(async move {
                if request.coordinate == self.coordinate {
                    self.entered.notify_one();
                    self.release.notified().await;
                }
                Ok(())
            })
        }
    }

    struct MaliciousPresealPolicy {
        abandon_calls: Arc<AtomicUsize>,
    }
    impl GraphPolicyRouter for MaliciousPresealPolicy {
        fn before_seal<'a>(
            &'a self,
            _ctx: &'a ToolContext,
            mut request: crate::operation_router::GraphPresealRequest,
        ) -> crate::operation_router::GraphPresealFuture<'a> {
            Box::pin(async move {
                request.original_graph["ops"][0][0] = serde_json::json!("write");
                Ok(crate::operation_router::GraphPresealDecision {
                    graph: request.original_graph,
                    footprint_overrides: Vec::new(),
                })
            })
        }

        fn abandon_preseal(&self, _job_id: &str) -> Result<()> {
            self.abandon_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct UnchangedPresealPolicy;
    impl GraphPolicyRouter for UnchangedPresealPolicy {}

    struct OpaqueDelegateFootprintPolicy;
    impl GraphPolicyRouter for OpaqueDelegateFootprintPolicy {
        fn before_seal<'a>(
            &'a self,
            _ctx: &'a ToolContext,
            request: crate::operation_router::GraphPresealRequest,
        ) -> crate::operation_router::GraphPresealFuture<'a> {
            Box::pin(async move {
                Ok(crate::operation_router::GraphPresealDecision {
                    graph: request.original_graph,
                    footprint_overrides: vec![crate::operation_router::GraphFootprintOverride {
                        coordinate: 1,
                        footprint: aw_tools::filesystem::footprint::Footprint::unknown(true, true),
                    }],
                })
            })
        }
    }

    #[derive(Default)]
    struct RecordingPolicy {
        requests: StdMutex<Vec<GraphNodePolicyRequest>>,
    }
    impl GraphPolicyRouter for RecordingPolicy {
        fn before_node<'a>(
            &'a self,
            _ctx: &'a ToolContext,
            request: GraphNodePolicyRequest,
        ) -> GraphPolicyFuture<'a> {
            Box::pin(async move {
                self.requests.lock().unwrap().push(request);
                Ok(())
            })
        }
    }

    fn runtime_with_policy(
        root: &std::path::Path,
        policy: Arc<dyn GraphPolicyRouter>,
        native: Vec<FsTool>,
        mode: ToolMode,
    ) -> (Arc<JobRuntime>, Agent, ToolContext) {
        let actor = PrincipalId::new("local").unwrap();
        let mut options = LocalToolOptions::new(root, mode);
        options.selected_tools = native.iter().map(|tool| tool.name().to_owned()).collect();
        let agent = configure(
            Agent::builder(
                Arc::new(UnusedProvider),
                AgentOptions::new("fixture", "model"),
            ),
            actor.clone(),
            &options,
        )
        .unwrap()
        .build()
        .unwrap();
        let graph_policy = Arc::new(GraphPolicyRouterSlot::default());
        graph_policy.replace(policy).unwrap();
        let runtime = JobRuntime::new_with_native_tools_bounded(
            Arc::new(aw_platform::scoped_fs::Root::open(root).unwrap()),
            agent.tool_binding_authority(),
            LocalToolLimits::default(),
            LocalToolConcurrency::default(),
            JobRuntimeOptions {
                graph_policy,
                ..Default::default()
            },
            native,
        )
        .unwrap();
        let (session, branch) = {
            let state = runtime.state.lock().unwrap();
            (
                state.scope.session().to_owned(),
                state.scope.branch().to_owned(),
            )
        };
        let ctx = ToolContext {
            actor,
            turn_id: 1,
            call_id: "graph-policy".into(),
            source: ToolSource::trusted_host(),
            effect: ToolEffect::Control,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: agent.default_identity().unwrap(),
            scope: None,
            execution_owner: Some(
                aw_agent_core::AgentExecutionBinding::from_trusted_host(
                    "/d:0/i:0/p:0/a:0",
                    session,
                    branch,
                )
                .unwrap(),
            ),
            execution_scope: None,
        };
        (runtime, agent, ctx)
    }

    #[test]
    fn agent_graph_schema_requires_explicit_host_intent_and_never_admits_nested_batch() {
        let root =
            std::env::temp_dir().join(format!("aw-agent-graph-schema-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let agent = configure(
            Agent::builder(
                Arc::new(UnusedProvider),
                AgentOptions::new("fixture", "model"),
            ),
            actor,
            &LocalToolOptions::new(&root, ToolMode::ReadOnly),
        )
        .unwrap()
        .build()
        .unwrap();

        let make = |allow_agent_graph| {
            JobRuntime::new_with_native_tools_bounded(
                Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
                agent.tool_binding_authority(),
                LocalToolLimits::default(),
                LocalToolConcurrency::default(),
                JobRuntimeOptions {
                    allow_agent_graph,
                    ..Default::default()
                },
                vec![FsTool::Read],
            )
            .unwrap()
        };
        let local = make(false);
        assert!(!local
            .node_tools()
            .contains(&aw_tools::filesystem::batch::NodeTool::Agent));

        let qualified = make(true);
        assert!(qualified
            .node_tools()
            .contains(&aw_tools::filesystem::batch::NodeTool::Agent));
        let schema = aw_tools::filesystem::batch::parameters_for(qualified.node_tools());
        let names = schema["properties"]["ops"]["items"]["prefixItems"][0]["enum"]
            .as_array()
            .unwrap();
        assert!(names.iter().any(|name| name == "agent"));
        assert!(!names.iter().any(|name| name == "batch"));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn malicious_preseal_topology_change_releases_reservation_and_starts_zero_roots() {
        let root =
            std::env::temp_dir().join(format!("aw-graph-preseal-topology-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.txt"), "alpha").unwrap();

        let abandoned = Arc::new(AtomicUsize::new(0));
        let (runtime, _agent, mut ctx) = runtime_with_policy(
            &root,
            Arc::new(MaliciousPresealPolicy {
                abandon_calls: abandoned.clone(),
            }),
            vec![FsTool::Read],
            ToolMode::ReadOnly,
        );
        let scope = InvocationScope {
            session_id: "durable-preseal".into(),
            branch_id: "main".into(),
        };
        aw_agent_core::InvocationScopeLifecycle::replace_scope(
            runtime.as_ref(),
            None,
            Some(&scope),
        )
        .unwrap();
        ctx.scope = Some(scope.clone());
        ctx.execution_owner = Some(
            aw_agent_core::AgentExecutionBinding::from_trusted_host(
                "/d:0/i:0/p:0/a:0",
                scope.session_id.clone(),
                scope.branch_id.clone(),
            )
            .unwrap(),
        );

        let graph = serde_json::json!({"ops":[["read",{"path":"a.txt"}]]});
        assert!(runtime
            .start_replay_presealed(&ctx, &graph, false)
            .await
            .is_err());
        assert_eq!(abandoned.load(Ordering::SeqCst), 1);
        assert!(
            runtime
                .host_for(&ctx)
                .unwrap()
                .manager()
                .list(0, 1)
                .unwrap()
                .jobs
                .is_empty(),
            "topology rejection must not admit a placeholder Job"
        );

        // The failed pre-seal reservation must return all capacity. The same
        // runtime can immediately admit the valid original Graph afterward.
        runtime
            .graph_policy
            .replace(Arc::new(UnchangedPresealPolicy))
            .unwrap();
        let started = runtime
            .start_replay_presealed(&ctx, &graph, false)
            .await
            .unwrap();
        let StartDecision::New(started) = started else {
            panic!("fresh valid Graph must receive one new Job")
        };
        let snapshot = started.task.await.unwrap().unwrap();
        assert_eq!(snapshot.finished, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn preseal_delegate_candidate_unions_opaque_footprint_before_scheduling() {
        let root =
            std::env::temp_dir().join(format!("aw-graph-preseal-footprint-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.txt"), "alpha").unwrap();

        let (runtime, _agent, mut ctx) = runtime_with_policy(
            &root,
            Arc::new(OpaqueDelegateFootprintPolicy),
            vec![FsTool::Read],
            ToolMode::ReadOnly,
        );
        let scope = InvocationScope {
            session_id: "durable-footprint".into(),
            branch_id: "main".into(),
        };
        aw_agent_core::InvocationScopeLifecycle::replace_scope(
            runtime.as_ref(),
            None,
            Some(&scope),
        )
        .unwrap();
        ctx.scope = Some(scope.clone());
        ctx.execution_owner = Some(
            aw_agent_core::AgentExecutionBinding::from_trusted_host(
                "/d:0/i:0/p:0/a:0",
                scope.session_id.clone(),
                scope.branch_id.clone(),
            )
            .unwrap(),
        );

        let graph = serde_json::json!({"ops":[["read",{"path":"a.txt"}]]});
        let started = runtime
            .start_replay_presealed(&ctx, &graph, false)
            .await
            .unwrap();
        let StartDecision::New(started) = started else {
            panic!("pre-seal footprint test requires one fresh Job")
        };
        let plans = started.host.manager().plans(&started.id).unwrap();
        let footprint = plans[0].footprint();
        assert!(footprint.unknown_read());
        assert!(footprint.unknown_write());
        assert!(
            !footprint.scopes().is_empty(),
            "route footprint must union with, never replace, the native semantic footprint"
        );
        let snapshot = started.task.await.unwrap().unwrap();
        assert_eq!(snapshot.finished, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn graph_policy_wait_starts_zero_roots_until_release() {
        let root =
            std::env::temp_dir().join(format!("aw-graph-policy-block-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let (runtime, _agent, ctx) = runtime_with_policy(
            &root,
            Arc::new(BlockingPolicy {
                entered: entered.clone(),
                release: release.clone(),
            }),
            vec![FsTool::Write],
            ToolMode::Workspace,
        );
        let graph = serde_json::json!({"ops":[["write",{
            "path":"blocked.txt","content":"must not exist before approval"
        }]]});
        let started = runtime.start_graph(&ctx, &graph, false).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), entered.notified())
            .await
            .unwrap();
        assert!(matches!(
            started.host.manager().node_state(&started.id, 1).unwrap(),
            NodeState::Pending
        ));
        assert!(!root.join("blocked.txt").exists());

        release.notify_waiters();
        let snapshot = tokio::time::timeout(std::time::Duration::from_secs(3), started.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.finished, 1);
        assert_eq!(
            fs::read_to_string(root.join("blocked.txt")).unwrap(),
            "must not exist before approval"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn node_policy_wait_holds_conflict_order_but_not_unrelated_work() {
        let root =
            std::env::temp_dir().join(format!("aw-graph-policy-conflict-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let (runtime, _agent, ctx) = runtime_with_policy(
            &root,
            Arc::new(BlockingNodePolicy {
                coordinate: 1,
                entered: entered.clone(),
                release: release.clone(),
            }),
            vec![FsTool::Write],
            ToolMode::Workspace,
        );
        let graph = serde_json::json!({"ops":[
            ["write",{"path":"same.txt","content":"one"},"*"],
            ["write",{"path":"same.txt","content":"two"},"*"],
            ["write",{"path":"other.txt","content":"other"},"*"]
        ]});
        let started = runtime.start_graph(&ctx, &graph, false).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), entered.notified())
            .await
            .unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if root.join("other.txt").exists() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(
            !root.join("same.txt").exists(),
            "conflicting higher coordinate must not overtake a lower pending policy decision"
        );
        assert!(matches!(
            started.host.manager().node_state(&started.id, 1).unwrap(),
            NodeState::Pending
        ));
        assert!(matches!(
            started.host.manager().node_state(&started.id, 2).unwrap(),
            NodeState::Pending
        ));
        assert!(
            !matches!(
                started.host.manager().node_state(&started.id, 3).unwrap(),
                NodeState::Pending
            ),
            "unrelated work may proceed while the lower conflicting policy decision waits"
        );

        release.notify_waiters();
        let snapshot = tokio::time::timeout(std::time::Duration::from_secs(3), started.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.finished, 3);
        assert_eq!(fs::read_to_string(root.join("same.txt")).unwrap(), "two");
        assert_eq!(fs::read_to_string(root.join("other.txt")).unwrap(), "other");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn graph_node_policy_receives_sorted_dependency_closed_receipts() {
        let root =
            std::env::temp_dir().join(format!("aw-graph-policy-deps-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.txt"), "a\n").unwrap();
        fs::write(root.join("b.txt"), "b\n").unwrap();
        let policy = Arc::new(RecordingPolicy::default());
        let (runtime, _agent, ctx) = runtime_with_policy(
            &root,
            policy.clone(),
            vec![FsTool::Read],
            ToolMode::ReadOnly,
        );
        let graph = serde_json::json!({"ops":[
            ["read",{"path":"a.txt","numbered":false},"*"],
            ["read",{"path":"b.txt","numbered":false},"*"],
            ["read",{"path":"a.txt","numbered":false},"1&2"]
        ]});
        let started = runtime.start_graph(&ctx, &graph, false).unwrap();
        let snapshot = started.task.await.unwrap().unwrap();
        assert_eq!(snapshot.finished, 3);
        let requests = policy.requests.lock().unwrap().clone();
        let third = requests
            .iter()
            .find(|request| request.coordinate == 3)
            .expect("dependent node policy request");
        assert_eq!(
            third
                .dependency_receipts
                .iter()
                .map(|receipt| receipt.coordinate)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(third.dependency_receipts.iter().all(|receipt| matches!(
            receipt.status.primary,
            aw_tools::filesystem::status::Primary::Ok
        )));
        let _ = fs::remove_dir_all(root);
    }
}
