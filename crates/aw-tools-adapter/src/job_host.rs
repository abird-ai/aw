//! AW binding for the shared canonical live-session JobManager.
//! The existing outer AW ToolStarted journal intent authorizes the Graph before
//! this host runs. Node lifecycle/results stay in the shared live manager; AW
//! creates no second workflow database and never resurrects Jobs after restart.
#[cfg(all(test, feature = "conformance"))]
use crate::{binary, edit, lifecycle, patch, search, text};
use crate::{
    exposure::{ExposureCursor, ExposureIndex, ExposureOwner},
    process, shell, Error, Result,
};
use aw_agent_core::tool::{ToolBindingAuthority, ToolBindingSnapshot};
use aw_agent_core::{
    CancellationToken, ExecutionOwnershipScope, PrincipalId, Tool as AgentTool, ToolContext,
    ToolOutput,
};
use aw_tools::{
    execution_ref::{
        AgentRef, ExecutionRef, ExecutionSegment, Handle, NodeCoordinate, ResolvedExecutionRef,
    },
    filesystem::{
        batch::{Graph, NodeTool, Status},
        job::NodeState,
        job_manager::{
            plan_graph, semantic_hash, Error as JobManagerError, JobManager, Limits, NodePlan,
            NodePolicy, OutputAck, OutputOwner, OutputPage, OutputStream, ProcessSettlement,
            ReplayDecision, ReplayPersist, ReservedExposedTaskAdmission, SemanticBudget, Snapshot,
            StartRequest, TaskAdmission,
        },
        schema::Tool as CoreTool,
        status::Primary,
    },
    job_event::{
        EventRuleSet, JobEventDelivery, JobEventKind, JobWakeMode, SourceEventBook,
        SubscriptionUpdate,
    },
    task_fence::TaskIdentity,
    ToolEffect as CoreEffect,
};
use base64::Engine as _;
use futures_util::{stream::FuturesUnordered, StreamExt};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

const NODE_WORK_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const SHELL_RESULT_BYTES: usize = 4 * 1024;

fn job_admission_error(error: JobManagerError) -> Error {
    match error {
        JobManagerError::Capacity => Error::Limit("live Job capacity exhausted"),
        JobManagerError::Busy => Error::Conflict("live Job admission is busy"),
        _ => Error::Protocol("live Job admission failed"),
    }
}

fn replay_state_error(error: JobManagerError) -> Error {
    match error {
        JobManagerError::Capacity => Error::Limit("live replay identity capacity exhausted"),
        JobManagerError::Busy => Error::Conflict("live replay identity is being claimed"),
        JobManagerError::ReplayMismatch => {
            Error::Denied("replay identity reused with different semantics")
        }
        JobManagerError::ReplayRejected => {
            Error::Conflict("replay identity rejected before execution")
        }
        JobManagerError::ReplayUnknown => {
            Error::Protocol("replay identity outcome unknown; do not retry automatically")
        }
        _ => Error::Protocol("live replay state failed"),
    }
}

pub(crate) struct GraphParallelism {
    max_width: usize,
    max_reads: AtomicUsize,
    allow_shell: AtomicBool,
    max_shell: AtomicUsize,
}
impl GraphParallelism {
    pub(crate) fn new(value: crate::LocalToolConcurrency) -> Result<Self> {
        let value = value.validate()?;
        Ok(Self {
            max_width: value.graph_width,
            max_reads: AtomicUsize::new(value.parallelism.max_parallel_reads),
            allow_shell: AtomicBool::new(value.parallelism.allow_parallel_shell),
            max_shell: AtomicUsize::new(value.parallelism.max_parallel_shell),
        })
    }

    pub(crate) fn max_width(&self) -> usize {
        self.max_width
    }

    pub(crate) fn apply(&self, value: aw_agent_core::ToolParallelism) {
        self.max_reads
            .store(value.max_parallel_reads, Ordering::Release);
        self.max_shell
            .store(value.max_parallel_shell, Ordering::Release);
        self.allow_shell
            .store(value.allow_parallel_shell, Ordering::Release);
    }

    fn width(&self, graph: &Graph) -> usize {
        let has_read = graph.steps().iter().any(|step| {
            step.tool
                .native()
                .is_some_and(|tool| tool.action() == aw_tools::filesystem::access::Action::Read)
        });
        let has_shell = graph.steps().iter().any(|step| step.tool.is_shell());
        let mut width = self.max_width;
        if has_read {
            width = width.min(self.max_reads.load(Ordering::Acquire));
        }
        if has_shell {
            width = width.min(if self.allow_shell.load(Ordering::Acquire) {
                self.max_shell.load(Ordering::Acquire)
            } else {
                1
            });
        }
        width.max(1)
    }
}

#[derive(Clone, Debug)]
pub struct PreparedJob {
    graph: Graph,
    plans: Vec<NodePlan>,
    task: TaskIdentity,
    semantic_hash: [u8; 32],
    width: usize,
}
impl PreparedJob {
    pub fn semantic_hash(&self) -> [u8; 32] {
        self.semantic_hash
    }

    fn replay_semantic_hash(&self, initial_wake: Option<(&str, &str, &EventRuleSet)>) -> [u8; 32] {
        let Some((recipient, authority_ref, rules)) = initial_wake else {
            return self.semantic_hash;
        };
        let mut hash = Sha256::new();
        hash.update(b"aw-job-constructor-semantic-v2\0");
        hash.update(self.semantic_hash);
        hash.update((recipient.len() as u64).to_le_bytes());
        hash.update(recipient.as_bytes());
        hash.update((authority_ref.len() as u64).to_le_bytes());
        hash.update(authority_ref.as_bytes());
        for rule in rules.as_slice() {
            let event = rule.event.as_str();
            let mode = rule.mode.as_str();
            hash.update((event.len() as u64).to_le_bytes());
            hash.update(event.as_bytes());
            hash.update((mode.len() as u64).to_le_bytes());
            hash.update(mode.as_bytes());
        }
        hash.finalize().into()
    }

    pub(crate) fn node_count(&self) -> usize {
        self.graph.steps().len()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GraphAdmissionReservation {
    id: String,
    task: TaskIdentity,
    exposure_owner: crate::agent_jobs::FiniteExposureOwner,
    exposure_ref: String,
    exposed: bool,
}

impl GraphAdmissionReservation {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdmissionDecision {
    New(String),
    Existing(String),
    Unknown,
}

#[derive(Default)]
struct JobEventState {
    sources: HashMap<String, SourceEventBook>,
    terminal_committed: HashSet<String>,
}

pub struct JobHost {
    manager: Arc<JobManager>,
    exposure: Arc<ExposureIndex>,
    agent_jobs: Arc<crate::agent_jobs::AgentJobRouterSlot>,
    graph_policy: Arc<crate::operation_router::GraphPolicyRouterSlot>,
    graph_host_tools: Arc<crate::operation_router::GraphHostToolSlot>,
    authority: ToolBindingAuthority,
    root: Arc<aw_platform::scoped_fs::Root>,
    session: String,
    branch: String,
    root_binding: [u8; 32],
    agent_ref: Mutex<Option<AgentRef>>,
    transition_gate: Mutex<()>,
    control_gate: tokio::sync::Mutex<()>,
    events: Mutex<JobEventState>,
    process_inputs: Mutex<HashMap<(String, usize), process::ProcessInput>>,
    shells: shell::ShellRegistry,
    native_tools: Arc<Vec<CoreTool>>,
    parallelism: Arc<GraphParallelism>,
    tool_limits: crate::LocalToolLimits,
    max_graph_nodes: usize,
}

struct PreparedGraphNodeExecutor {
    host: Arc<JobHost>,
    ctx: ToolContext,
    id: String,
    coordinate: usize,
    cancellation: CancellationToken,
}

impl crate::operation_router::GraphNodeOriginalExecutor for PreparedGraphNodeExecutor {
    fn execute<'a>(&'a self) -> crate::operation_router::GraphNodeExecutionFuture<'a> {
        Box::pin(async move {
            self.host
                .execute_node_local(
                    &self.ctx,
                    &self.id,
                    self.coordinate,
                    self.cancellation.clone(),
                )
                .await
        })
    }
}

fn core_effect(effect: &aw_agent_core::ToolEffect) -> CoreEffect {
    match effect {
        aw_agent_core::ToolEffect::Read => CoreEffect::Read,
        aw_agent_core::ToolEffect::Write => CoreEffect::Write,
        aw_agent_core::ToolEffect::Process => CoreEffect::Process,
        aw_agent_core::ToolEffect::Control => CoreEffect::Control,
        aw_agent_core::ToolEffect::Unknown => CoreEffect::Unknown,
    }
}

fn binding_hash(actor: &PrincipalId, binding: &ToolBindingSnapshot) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"aw-tools-adapter-job-authority-v1\0");
    for part in [
        actor.as_str(),
        binding.name.as_str(),
        binding.source.kind_label(),
        binding.source.id.as_str(),
        binding.execution_site.as_str(),
    ] {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    hash.update(binding.registry_revision.to_le_bytes());
    hash.update(binding.authority_revision.to_le_bytes());
    hash.update(binding.source.epoch.to_le_bytes());
    hash.update([match binding.effect {
        aw_agent_core::ToolEffect::Read => 0,
        aw_agent_core::ToolEffect::Write => 1,
        aw_agent_core::ToolEffect::Process => 2,
        aw_agent_core::ToolEffect::Control => 3,
        aw_agent_core::ToolEffect::Unknown => 4,
    }]);
    hash.finalize().into()
}

fn new_job_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| Error::Config("OS randomness unavailable".into()))?;
    Ok(format!(
        "j{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

#[cfg(all(test, feature = "conformance"))]
fn canonical_tool(
    root: Arc<aw_platform::scoped_fs::Root>,
    tool: CoreTool,
    limits: crate::LocalToolLimits,
) -> Arc<dyn AgentTool> {
    match tool {
        CoreTool::Ls => Arc::new(search::Search::with_limits(
            root,
            search::Operation::Ls,
            limits,
        )),
        CoreTool::Find => Arc::new(search::Search::with_limits(
            root,
            search::Operation::Find,
            limits,
        )),
        CoreTool::Grep => Arc::new(search::Search::with_limits(
            root,
            search::Operation::Grep,
            limits,
        )),
        CoreTool::Read => Arc::new(text::Text::with_limits(root, text::Operation::Read, limits)),
        CoreTool::Write => Arc::new(text::Text::with_limits(
            root,
            text::Operation::Write,
            limits,
        )),
        CoreTool::Edit => Arc::new(edit::Edit::with_limits(root, limits)),
        CoreTool::Patch => Arc::new(patch::Patch::with_limits(root, limits)),
        CoreTool::Bread => Arc::new(binary::Binary::with_limits(
            root,
            binary::Operation::Read,
            limits,
        )),
        CoreTool::Bwrite => Arc::new(binary::Binary::with_limits(
            root,
            binary::Operation::Write,
            limits,
        )),
        CoreTool::Bedit => Arc::new(binary::Binary::with_limits(
            root,
            binary::Operation::Edit,
            limits,
        )),
        CoreTool::Copy => Arc::new(lifecycle::Lifecycle::with_limits(
            root,
            lifecycle::Operation::Copy,
            limits,
        )),
        CoreTool::Move => Arc::new(lifecycle::Lifecycle::with_limits(
            root,
            lifecycle::Operation::Move,
            limits,
        )),
        CoreTool::Remove => Arc::new(lifecycle::Lifecycle::with_limits(
            root,
            lifecycle::Operation::Remove,
            limits,
        )),
    }
}

fn canonical_request(
    native: CoreTool,
    arguments: &Value,
    limits: crate::LocalToolLimits,
) -> Option<aw_tools::filesystem::request::Request> {
    // `parallel` is AW scheduler metadata on read tools, not canonical
    // filesystem semantics. Strip only that explicitly advertised adapter
    // addition; every other field must pass the shared canonical parser.
    let mut semantic = arguments.clone();
    if native.action() == aw_tools::filesystem::access::Action::Read {
        if let Some(object) = semantic.as_object_mut() {
            object.remove("parallel");
        }
    }
    aw_tools::filesystem::request::parse(native, &semantic, limits.request_limits()).ok()
}

fn canonical_arguments(
    native: CoreTool,
    arguments: &Value,
    limits: crate::LocalToolLimits,
) -> bool {
    canonical_request(native, arguments, limits).is_some()
}

fn graph_arguments_are_semantic(arguments: &Value) -> bool {
    // `parallel` is an AW top-level scheduler hint. Graph dependency syntax
    // already owns concurrency, so accepting it here would create ignored
    // input that changes replay fingerprints without changing execution.
    arguments.get("parallel").is_none()
}

fn error_result(tool: CoreTool, error: Error) -> (ToolOutput, Status) {
    let detail = aw_tools::task::bounded_task_error(error.to_string());
    let ambiguous = tool.action() != aw_tools::filesystem::access::Action::Read
        && matches!(
            error,
            Error::Io(_) | Error::Protocol(_) | Error::Cancelled | Error::Timeout
        );
    let primary = if ambiguous {
        Primary::Unknown
    } else {
        Primary::Fail
    };
    (
        ToolOutput::error(format!("{} {detail}", primary.as_str())),
        Status {
            primary,
            more: false,
            incomplete: false,
        },
    )
}

fn direct_error_result(
    error: aw_tools::filesystem::direct::Error<aw_platform::scoped_fs::ScopedError>,
) -> (ToolOutput, Status) {
    let status = aw_tools::filesystem::direct::graph_error_status(&error);
    let detail =
        aw_tools::task::bounded_task_error(crate::canonical_direct_error(error).to_string());
    (
        ToolOutput::error(format!("{} {detail}", status.primary.as_str())),
        status,
    )
}

pub(crate) struct JobHostConfig {
    pub session: String,
    pub branch: String,
    pub tool_limits: crate::LocalToolLimits,
    pub shells: shell::ShellRegistry,
    pub native_tools: Arc<Vec<CoreTool>>,
    pub parallelism: Arc<GraphParallelism>,
    pub exposure: Arc<ExposureIndex>,
    pub agent_jobs: Arc<crate::agent_jobs::AgentJobRouterSlot>,
    pub graph_policy: Arc<crate::operation_router::GraphPolicyRouterSlot>,
    pub graph_host_tools: Arc<crate::operation_router::GraphHostToolSlot>,
    pub max_graph_nodes: usize,
}

impl JobHost {
    pub fn new(
        root: Arc<aw_platform::scoped_fs::Root>,
        authority: ToolBindingAuthority,
        session: impl Into<String>,
        branch: impl Into<String>,
        limits: Limits,
    ) -> Result<Self> {
        Self::new_with_config(
            root,
            authority,
            JobHostConfig {
                session: session.into(),
                branch: branch.into(),
                tool_limits: crate::LocalToolLimits {
                    jobs: limits,
                    ..Default::default()
                },
                shells: shell::ShellRegistry::default(),
                native_tools: Arc::new(CoreTool::GRAPH_NATIVE.to_vec()),
                parallelism: Arc::new(GraphParallelism::new(
                    crate::LocalToolConcurrency::default(),
                )?),
                exposure: Arc::new(ExposureIndex::new(limits.max_retained_jobs)?),
                agent_jobs: Arc::new(crate::agent_jobs::AgentJobRouterSlot::default()),
                graph_policy: Arc::new(crate::operation_router::GraphPolicyRouterSlot::default()),
                graph_host_tools: Arc::new(crate::operation_router::GraphHostToolSlot::default()),
                max_graph_nodes: aw_tools::filesystem::batch::MAX_OPS,
            },
        )
    }

    pub(crate) fn new_with_config(
        root: Arc<aw_platform::scoped_fs::Root>,
        authority: ToolBindingAuthority,
        config: JobHostConfig,
    ) -> Result<Self> {
        let JobHostConfig {
            session,
            branch,
            tool_limits,
            shells,
            native_tools,
            parallelism,
            exposure,
            agent_jobs,
            graph_policy,
            graph_host_tools,
            max_graph_nodes,
        } = config;
        for value in [&session, &branch] {
            if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                return Err(Error::Config("invalid Job Session/branch identity".into()));
            }
        }
        if !(1..=aw_tools::filesystem::batch::MAX_OPS).contains(&max_graph_nodes) {
            return Err(Error::Config(
                "Job Graph node ceiling exceeds the shared safety maximum".into(),
            ));
        }
        if native_tools.iter().any(|tool| !tool.graph_eligible()) {
            return Err(Error::Config(
                "Job native registry contains a direct-only filesystem tool".into(),
            ));
        }
        let mut hash = Sha256::new();
        hash.update(b"aw-tools-adapter-native-runtime-v1\0");
        hash.update((session.len() as u64).to_le_bytes());
        hash.update(session.as_bytes());
        hash.update((branch.len() as u64).to_le_bytes());
        hash.update(branch.as_bytes());
        let root_binding = hash.finalize().into();
        Ok(Self {
            manager: Arc::new(
                JobManager::new(tool_limits.jobs)
                    .map_err(|_| Error::Config("invalid shared JobManager limits".into()))?,
            ),
            exposure,
            agent_jobs,
            graph_policy,
            graph_host_tools,
            authority,
            root,
            session,
            branch,
            root_binding,
            agent_ref: Mutex::new(None),
            transition_gate: Mutex::new(()),
            control_gate: tokio::sync::Mutex::new(()),
            events: Mutex::new(JobEventState::default()),
            process_inputs: Mutex::new(HashMap::new()),
            shells,
            native_tools,
            parallelism,
            tool_limits,
            max_graph_nodes,
        })
    }

    pub fn manager(&self) -> &Arc<JobManager> {
        &self.manager
    }

    fn finite_terminal_mode(mode: JobWakeMode) -> Result<JobWakeMode> {
        match mode {
            JobWakeMode::Auto | JobWakeMode::Queue => Ok(JobWakeMode::Queue),
            JobWakeMode::Steer | JobWakeMode::Interrupt => Err(Error::Denied(
                "finite result/settled wake supports only auto or queue until attention-capable control is captured",
            )),
        }
    }

    pub(crate) fn subscribe_finite(
        &self,
        actor: &PrincipalId,
        id: &str,
        node: Option<usize>,
        expected_revision: Option<u64>,
        recipient_ref: &str,
        recipient_authority_ref: &str,
        rules: EventRuleSet,
    ) -> Result<(String, SubscriptionUpdate)> {
        let _transition = self
            .transition_gate
            .lock()
            .map_err(|_| Error::Protocol("Job transition gate poisoned"))?;
        if node.is_some() {
            return Err(Error::Denied(
                "initial finite jobs_subscribe targets a Process/Graph Job root",
            ));
        }
        if !self.payload_authorized(actor, id) {
            return Err(Error::Denied("Job subscription source is not authorized"));
        }
        let snapshot = self
            .manager
            .runtime_snapshot(id)
            .map_err(|_| Error::Denied("Job subscription source is unavailable"))?;
        if expected_revision.is_some_and(|revision| revision != snapshot.sequence) {
            return Err(Error::Conflict("stale Job revision"));
        }
        for rule in rules.as_slice() {
            match rule.event {
                JobEventKind::Result | JobEventKind::Settled => {
                    Self::finite_terminal_mode(rule.mode)?;
                }
                JobEventKind::Attention => {
                    return Err(Error::Denied(
                        "finite Job source does not expose attention events",
                    ))
                }
            }
        }
        if let (Some(result), Some(settled)) = (
            rules.get(JobEventKind::Result),
            rules.get(JobEventKind::Settled),
        ) {
            if Self::finite_terminal_mode(result)? != Self::finite_terminal_mode(settled)? {
                return Err(Error::Denied(
                    "same terminal checkpoint cannot request incompatible wake modes",
                ));
            }
        }
        let canonical = self.execution_ref(id, None)?.canonical();
        let mut events = self
            .events
            .lock()
            .map_err(|_| Error::Protocol("Job event source state poisoned"))?;
        let update = events
            .sources
            .entry(canonical.clone())
            .or_default()
            .replace(recipient_ref, recipient_authority_ref, rules)
            .map_err(Error::Denied)?;
        Ok((canonical, update))
    }

    pub(crate) fn commit_finite_terminal_events(&self, id: &str) -> Result<Vec<JobEventDelivery>> {
        let _transition = self
            .transition_gate
            .lock()
            .map_err(|_| Error::Protocol("Job transition gate poisoned"))?;
        let snapshot = self
            .manager
            .runtime_snapshot(id)
            .map_err(|_| Error::Denied("Job event source is unavailable"))?;
        if snapshot.lifecycle != aw_tools::filesystem::job::Lifecycle::Terminal {
            return Ok(Vec::new());
        }
        let canonical = self.execution_ref(id, None)?.canonical();
        let mut events = self
            .events
            .lock()
            .map_err(|_| Error::Protocol("Job event source state poisoned"))?;
        if !events.terminal_committed.insert(canonical.clone()) {
            return Ok(Vec::new());
        }
        let deliveries = events
            .sources
            .entry(canonical.clone())
            .or_default()
            .commit(&canonical, &[JobEventKind::Result, JobEventKind::Settled])
            .map_err(Error::Denied)?;
        events.sources.remove(&canonical);
        Ok(deliveries)
    }

    pub(crate) async fn deliver_finite_terminal_events(&self, id: &str) {
        let deliveries = match self.commit_finite_terminal_events(id) {
            Ok(deliveries) => deliveries,
            Err(_) => return,
        };
        if deliveries.is_empty() {
            return;
        }
        let Ok(Some(router)) = self.agent_jobs.current() else {
            return;
        };
        for delivery in deliveries {
            // Source commitment is already factual. Receiver revocation or
            // admission failure suppresses future work without rewriting it.
            let _ = router.admit_event(delivery).await;
        }
    }

    pub(crate) fn bind_execution_owner(&self, ctx: &ToolContext) -> Result<AgentRef> {
        let binding = ctx
            .execution_owner
            .as_ref()
            .ok_or(Error::Denied("Job execution requires a bound Agent owner"))?;
        if ctx.scope.is_some()
            && (binding.session_id() != self.session || binding.branch_id() != self.branch)
        {
            return Err(Error::Denied(
                "Job execution owner differs from the bound Session/branch",
            ));
        }
        let parsed = ExecutionRef::parse(binding.canonical_ref())
            .map_err(|_| Error::Denied("invalid canonical Agent execution owner"))?;
        let resolved = parsed
            .resolve(None, None)
            .map_err(|_| Error::Denied("invalid canonical Agent execution owner"))?;
        if !resolved.suffix().is_empty() {
            return Err(Error::Denied("Job execution owner must be an Agent root"));
        }
        let owner = resolved.agent_ref().clone();
        let mut current = self
            .agent_ref
            .lock()
            .map_err(|_| Error::Protocol("Job Agent owner binding poisoned"))?;
        match current.as_ref() {
            None => *current = Some(owner.clone()),
            Some(bound) if bound == &owner => {}
            Some(_) => {
                return Err(Error::Conflict(
                    "live Job namespace cannot be rebound to another Agent",
                ))
            }
        }
        Ok(owner)
    }

    fn bound_agent_ref(&self) -> Result<AgentRef> {
        self.agent_ref
            .lock()
            .map_err(|_| Error::Protocol("Job Agent owner binding poisoned"))?
            .clone()
            .ok_or(Error::Denied("Job namespace has no bound Agent owner"))
    }

    fn finite_exposure_identity(
        &self,
        id: &str,
        task: &TaskIdentity,
    ) -> Result<(crate::agent_jobs::FiniteExposureOwner, String)> {
        let agent = self.bound_agent_ref()?;
        if task.session != self.session || task.branch != self.branch {
            return Err(Error::Denied(
                "finite Job exposure task differs from bound Session",
            ));
        }
        let owner = crate::agent_jobs::FiniteExposureOwner {
            canonical_agent_ref: agent.canonical(),
            actor: task.actor.clone(),
            session_id: task.session.clone(),
            branch_id: task.branch.clone(),
        };
        let reference = ResolvedExecutionRef::new(
            agent,
            vec![
                ExecutionSegment::Turn(
                    Handle::new(task.turn_id.to_string())
                        .map_err(|_| Error::Protocol("invalid Job Turn handle"))?,
                ),
                ExecutionSegment::Job(
                    Handle::new(id.to_owned())
                        .map_err(|_| Error::Protocol("invalid Job handle"))?,
                ),
            ],
        )
        .map_err(|_| Error::Protocol("cannot construct finite Job exposure ref"))?
        .canonical();
        Ok((owner, reference))
    }

    fn finite_exposure_for_id(
        &self,
        id: &str,
    ) -> Result<(crate::agent_jobs::FiniteExposureOwner, String)> {
        let task = self
            .manager
            .task_binding(id)
            .map_err(|_| Error::Denied("Job routing identity is unavailable"))?;
        self.finite_exposure_identity(id, &task)
    }

    fn reserve_finite_exposure(
        &self,
        id: &str,
        task: &TaskIdentity,
    ) -> Result<(crate::agent_jobs::FiniteExposureOwner, String)> {
        self.exposure.reserve_finite(id)?;
        let identity = self.finite_exposure_identity(id, task)?;
        if let Some(router) = self.agent_jobs.current()? {
            if let Err(error) = router.reserve_finite(
                &identity.0,
                &identity.1,
                self.tool_limits.jobs.max_retained_jobs,
            ) {
                let _ = self.exposure.release_unpublished_finite(id);
                return Err(error);
            }
        }
        Ok(identity)
    }

    fn publish_finite_exposure(&self, id: &str) -> Result<()> {
        let (owner, reference) = self.finite_exposure_for_id(id)?;
        if let Some(router) = self.agent_jobs.current()? {
            router.publish_finite(&owner, &reference)?;
        }
        self.exposure.publish_finite(id)?;
        Ok(())
    }

    fn release_unpublished_finite_exposure(
        &self,
        id: &str,
        owner: &crate::agent_jobs::FiniteExposureOwner,
        reference: &str,
    ) -> Result<()> {
        self.exposure.release_unpublished_finite(id)?;
        if let Some(router) = self.agent_jobs.current()? {
            router.release_unpublished_finite(owner, reference)?;
        }
        Ok(())
    }

    fn discard_unstarted_with_exposure(&self, id: &str) -> Result<()> {
        let (owner, reference) = self.finite_exposure_for_id(id)?;
        if let Ok(mut events) = self.events.lock() {
            events.sources.remove(&reference);
            events.terminal_committed.remove(&reference);
        }
        self.manager
            .discard_unstarted(id)
            .map_err(|_| Error::Protocol("provisional Job cleanup failed"))?;
        self.release_unpublished_finite_exposure(id, &owner, &reference)
    }

    pub fn jobs_list_authority_fingerprint(&self, actor: &PrincipalId) -> Result<[u8; 32]> {
        let binding = self
            .authority
            .permitted_binding(actor, "jobs_list")
            .map_err(|_| Error::Denied("jobs_list authority is not live"))?;
        Ok(binding_hash(actor, &binding))
    }

    pub fn execution_ref(&self, id: &str, node: Option<usize>) -> Result<ResolvedExecutionRef> {
        let task = self
            .manager
            .task_binding(id)
            .map_err(|_| Error::Denied("Job routing identity is unavailable"))?;
        let mut suffix = vec![
            ExecutionSegment::Turn(
                Handle::new(task.turn_id.to_string())
                    .map_err(|_| Error::Protocol("invalid Job Turn handle"))?,
            ),
            ExecutionSegment::Job(
                Handle::new(id.to_owned()).map_err(|_| Error::Protocol("invalid Job handle"))?,
            ),
        ];
        if let Some(node) = node {
            let node = u32::try_from(node)
                .ok()
                .and_then(|value| NodeCoordinate::new(value).ok())
                .ok_or(Error::Denied("Job node coordinate is invalid"))?;
            suffix.push(ExecutionSegment::Node(node));
        }
        ResolvedExecutionRef::new(self.bound_agent_ref()?, suffix)
            .map_err(|_| Error::Protocol("cannot construct canonical Job ref"))
    }

    pub fn resolve_job_ref(&self, requested: &str) -> Result<(String, Option<usize>)> {
        let parsed =
            ExecutionRef::parse(requested).map_err(|_| Error::Denied("invalid execution ref"))?;
        let owner = self.bound_agent_ref()?;
        let resolved = parsed
            .resolve(Some(&owner), None)
            .map_err(|_| Error::Denied("execution ref needs unavailable context"))?;
        if resolved.agent_ref() != &owner {
            return Err(Error::Denied("Job ref is outside the current Agent"));
        }
        let suffix = resolved.suffix();
        let (turn, job, node) = match suffix {
            [ExecutionSegment::Turn(turn), ExecutionSegment::Job(job)] => (turn, job, None),
            [ExecutionSegment::Turn(turn), ExecutionSegment::Job(job), ExecutionSegment::Node(node)] => {
                (turn, job, Some(node.get() as usize))
            }
            _ => {
                return Err(Error::Denied(
                    "finite Job ref must be t:<turn>/j:<job>[/n:<coordinate>]",
                ))
            }
        };
        let id = job.as_str().to_owned();
        let task = self
            .manager
            .task_binding(&id)
            .map_err(|_| Error::Denied("Job is not exposed or missing"))?;
        if turn.as_str() != task.turn_id.to_string() {
            return Err(Error::Denied("Job ref Turn does not match retained Job"));
        }
        self.manager
            .get(&id)
            .map_err(|_| Error::Denied("Job is not exposed or missing"))?;
        Ok((id, node))
    }

    fn cursor_mac(&self, actor: &PrincipalId, payload: &[u8]) -> Result<[u8; 32]> {
        let binding = self
            .authority
            .permitted_binding(actor, "jobs_list")
            .map_err(|_| Error::Denied("jobs_list authority is not live"))?;
        let mut hash = Sha256::new();
        hash.update(b"aw-jobs-list-cursor-v2\0");
        hash.update(self.root_binding);
        hash.update(binding_hash(actor, &binding));
        hash.update(payload);
        Ok(hash.finalize().into())
    }

    fn encode_list_cursor(&self, actor: &PrincipalId, cursor: &ExposureCursor) -> Result<String> {
        let key = cursor.owner_key.as_bytes();
        let mut payload = Vec::with_capacity(10 + key.len() + 32);
        payload.extend_from_slice(&cursor.position.to_le_bytes());
        payload.extend_from_slice(&(key.len() as u16).to_le_bytes());
        payload.extend_from_slice(key);
        let mac = self.cursor_mac(actor, &payload)?;
        payload.extend_from_slice(&mac);
        Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload))
    }

    fn decode_list_cursor(&self, actor: &PrincipalId, encoded: &str) -> Result<ExposureCursor> {
        if encoded.is_empty() || encoded.len() > 512 {
            return Err(Error::Denied("invalid jobs_list cursor"));
        }
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| Error::Denied("invalid jobs_list cursor"))?;
        if bytes.len() < 42 {
            return Err(Error::Denied("invalid jobs_list cursor"));
        }
        let payload_len = bytes.len() - 32;
        let (payload, supplied_mac) = bytes.split_at(payload_len);
        if self.cursor_mac(actor, payload)?.as_slice() != supplied_mac {
            return Err(Error::Denied("jobs_list cursor is stale or unauthorized"));
        }
        let position = u64::from_le_bytes(
            payload[0..8]
                .try_into()
                .map_err(|_| Error::Denied("invalid jobs_list cursor"))?,
        );
        let key_len = u16::from_le_bytes(
            payload[8..10]
                .try_into()
                .map_err(|_| Error::Denied("invalid jobs_list cursor"))?,
        ) as usize;
        if position == 0 || payload.len() != 10 + key_len {
            return Err(Error::Denied("invalid jobs_list cursor"));
        }
        let owner_key = std::str::from_utf8(&payload[10..])
            .map_err(|_| Error::Denied("invalid jobs_list cursor"))?
            .to_owned();
        if owner_key.is_empty() || owner_key.len() > 2048 || owner_key.chars().any(char::is_control)
        {
            return Err(Error::Denied("invalid jobs_list cursor"));
        }
        Ok(ExposureCursor {
            position,
            owner_key,
        })
    }

    pub fn list_jobs(
        &self,
        actor: &PrincipalId,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<(Vec<Snapshot>, Option<String>)> {
        let cursor = cursor
            .map(|value| self.decode_list_cursor(actor, value))
            .transpose()?;
        let (items, next_cursor) = self.exposure.list(cursor.as_ref(), limit)?;
        let mut jobs = Vec::with_capacity(items.len());
        for item in items {
            match item.owner {
                ExposureOwner::Finite(id) => {
                    let snapshot = self.manager.get(&id).map_err(|_| {
                        Error::Protocol("published finite Job owner is unavailable")
                    })?;
                    jobs.push(snapshot);
                }
                ExposureOwner::Agent(_) => {
                    return Err(Error::Protocol(
                        "Agent exposure reached finite Job projection before Agent routing was installed",
                    ));
                }
            }
        }
        let next = next_cursor
            .as_ref()
            .map(|cursor| self.encode_list_cursor(actor, cursor))
            .transpose()?;
        Ok((jobs, next))
    }

    pub async fn send_process_input(
        &self,
        actor: &PrincipalId,
        id: &str,
        coordinate: usize,
        expected_revision: Option<u64>,
        bytes: Option<Vec<u8>>,
        close: bool,
    ) -> Result<()> {
        if bytes.is_some() == close {
            return Err(Error::Denied(
                "process input requires exactly one of bytes or close",
            ));
        }
        let _control = self.control_gate.lock().await;
        let snapshot = self
            .manager
            .get(id)
            .map_err(|_| Error::Denied("Process Job is not exposed or missing"))?;
        if snapshot.lifecycle != aw_tools::filesystem::job::Lifecycle::Running {
            return Err(Error::Denied("Process Job is not running"));
        }
        // jobs_send's own Control grant is checked by Agent dispatch. Process
        // input is a separate non-model host capability and remains revocable
        // independently from the original shell launch binding.
        self.authority
            .permits_capability(
                actor,
                "process.input",
                &aw_agent_core::ToolSource::trusted_host(),
                &aw_agent_core::ToolEffect::Control,
            )
            .map_err(|_| Error::Denied("process.input authority is not live"))?;
        if !self.node_live(actor, id, coordinate) {
            return Err(Error::Denied(
                "Process input authority is not live for this shell node",
            ));
        }
        let input = self
            .process_inputs
            .lock()
            .map_err(|_| Error::Protocol("Process input registry poisoned"))?
            .get(&(id.to_owned(), coordinate))
            .cloned()
            .ok_or(Error::Denied("Process stdin is not open"))?;
        // Authorization/target existence failures above are no-effect and do
        // not advance public state. Compare-and-act linearizes immediately
        // before bytes/EOF are handed to the sole Process owner.
        self.manager
            .compare_and_bump_revision(id, expected_revision)
            .map_err(|_| Error::Conflict("stale Job revision"))?;
        if let Some(bytes) = bytes {
            input.write(bytes).await
        } else {
            input.close().await
        }
    }

    fn task_identity(&self, ctx: &ToolContext) -> Result<TaskIdentity> {
        self.bind_execution_owner(ctx)?;
        let binding = ctx
            .execution_owner
            .as_ref()
            .ok_or(Error::Denied("Job execution requires a bound Agent owner"))?;
        let (session, branch) = if let Some(scope) = ctx.scope.as_ref() {
            if scope.session_id != self.session || scope.branch_id != self.branch {
                return Err(Error::Denied(
                    "Job invocation scope differs from bound Session",
                ));
            }
            (scope.session_id.clone(), scope.branch_id.clone())
        } else {
            // Contextless SDK/host calls must explicitly select an existing
            // authorized Agent owner; the parser/runtime never creates a:0.
            (
                binding.session_id().to_owned(),
                binding.branch_id().to_owned(),
            )
        };
        let task = TaskIdentity {
            actor: ctx.actor.as_str().to_owned(),
            source_kind: ctx.source.kind_label().into(),
            source_id: ctx.source.id.clone(),
            source_epoch: ctx.source.epoch,
            execution_site: ctx.execution_site.clone(),
            session,
            branch,
            turn_id: ctx.turn_id,
            call_id: ctx.call_id.clone(),
            effect: core_effect(&ctx.effect),
        };
        task.validate()
            .map_err(|_| Error::Denied("invalid durable Job invocation identity"))?;
        Ok(task)
    }

    fn resolve_node(&self, name: &str) -> Option<NodeTool> {
        let operation = aw_tools::OperationId::from_model_name(name)?;
        if operation == aw_tools::OperationId::Agent {
            return self
                .graph_host_tools
                .binding(operation)
                .ok()
                .flatten()
                .map(|_| NodeTool::Agent);
        }
        let node = NodeTool::from_operation(operation)?;
        if let Some(native) = node.native() {
            return self
                .native_tools
                .contains(&native)
                .then_some(NodeTool::Native(native));
        }
        self.shells.resolve(operation.model_name())
    }

    fn validate_node(&self, node: NodeTool, arguments: &Value) -> bool {
        if !graph_arguments_are_semantic(arguments) {
            return false;
        }
        if let Some(native) = node.native() {
            return canonical_arguments(native, arguments, self.tool_limits);
        }
        if node.is_shell() {
            return self
                .shells
                .get(node)
                .is_some_and(|shell| shell.validate(arguments).is_ok());
        }
        if node.is_agent() {
            return self
                .graph_host_tools
                .binding(aw_tools::OperationId::Agent)
                .ok()
                .flatten()
                .is_some_and(|binding| {
                    let tool = binding.tool();
                    tool.is_available() && tool.validate(arguments).is_ok()
                });
        }
        false
    }

    fn plans_for_graph(&self, ctx: &ToolContext, graph: &Graph) -> Result<Vec<NodePlan>> {
        self.plans_for_graph_with_overrides(ctx, graph, &BTreeMap::new())
    }

    fn plans_for_graph_with_overrides(
        &self,
        ctx: &ToolContext,
        graph: &Graph,
        overrides: &BTreeMap<u32, aw_tools::filesystem::footprint::Footprint>,
    ) -> Result<Vec<NodePlan>> {
        let one = graph.steps().len() == 1;
        plan_graph(graph, |coordinate, step| {
            if !graph_arguments_are_semantic(&step.arguments) {
                return Err(());
            }
            let binding = self
                .authority
                .permitted_binding(&ctx.actor, step.tool.name())
                .map_err(|_| ())?;
            let (footprint, runtime_binding, task_result_eligible, result_bytes) =
                if let Some(native) = step.tool.native() {
                    let request =
                        canonical_request(native, &step.arguments, self.tool_limits).ok_or(())?;
                    (
                        request.footprint(self.root.as_ref()).map_err(|_| ())?,
                        self.root_binding,
                        one,
                        self.tool_limits.result_bytes,
                    )
                } else if step.tool.is_shell() {
                    let shell = self.shells.get(step.tool).ok_or(())?;
                    let prepared = shell.prepare(&step.arguments).map_err(|_| ())?;
                    (
                        shell.footprint(&prepared),
                        shell.runtime_binding().map_err(|_| ())?,
                        false,
                        SHELL_RESULT_BYTES,
                    )
                } else if step.tool.is_agent() {
                    let host = self
                        .graph_host_tools
                        .binding(aw_tools::OperationId::Agent)
                        .map_err(|_| ())?
                        .ok_or(())?;
                    let tool = host.tool();
                    if !tool.is_available() || tool.validate(&step.arguments).is_err() {
                        return Err(());
                    }
                    (
                        aw_tools::filesystem::footprint::Footprint::unknown(true, true),
                        host.runtime_binding(),
                        one,
                        self.tool_limits.result_bytes,
                    )
                } else {
                    return Err(());
                };
            let footprint = if let Some(override_footprint) = overrides.get(&(coordinate as u32)) {
                footprint.union(override_footprint).map_err(|_| ())?
            } else {
                footprint
            };
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: NODE_WORK_BYTES,
                    result_bytes,
                },
                footprint,
                registry_generation: binding.registry_revision,
                authority_binding: binding_hash(&ctx.actor, &binding),
                runtime_binding,
                task_result_eligible,
            })
        })
        .map_err(|_| Error::Denied("Job node preparation failed"))
    }

    fn compile_graph(&self, graph_input: &Value) -> Result<Graph> {
        let graph = Graph::compile_with(
            graph_input,
            |name| self.resolve_node(name),
            |node, arguments| self.validate_node(node, arguments),
        )
        .map_err(|_| Error::Denied("invalid canonical Job graph"))?;
        if graph.steps().len() > self.max_graph_nodes {
            return Err(Error::Limit(
                "Job Graph exceeds the configured node ceiling",
            ));
        }
        Ok(graph)
    }

    pub fn validate_graph(&self, graph_input: &Value) -> Result<()> {
        self.compile_graph(graph_input).map(|_| ())
    }

    pub fn prepare(&self, ctx: &ToolContext, graph_input: &Value) -> Result<PreparedJob> {
        self.prepare_with_footprints(ctx, graph_input, &BTreeMap::new())
    }

    pub(crate) fn prepare_with_footprints(
        &self,
        ctx: &ToolContext,
        graph_input: &Value,
        overrides: &BTreeMap<u32, aw_tools::filesystem::footprint::Footprint>,
    ) -> Result<PreparedJob> {
        let task = self.task_identity(ctx)?;
        let graph = self.compile_graph(graph_input)?;
        let plans = self.plans_for_graph_with_overrides(ctx, &graph, overrides)?;
        let semantic_hash = semantic_hash(&graph, &plans)
            .map_err(|_| Error::Denied("Job semantic identity failed"))?;
        let width = self.parallelism.width(&graph);
        Ok(PreparedJob {
            graph,
            plans,
            task,
            semantic_hash,
            width,
        })
    }

    fn admit_with_id(&self, id: &str, prepared: PreparedJob) -> Result<()> {
        self.manager
            .admit_task(
                prepared.graph,
                prepared.plans,
                TaskAdmission {
                    id,
                    task: &prepared.task,
                    admission_sequence: 1,
                    width: prepared.width,
                    exposed: false,
                },
            )
            .map_err(job_admission_error)
    }

    fn reserve_exposed_with_id(&self, id: &str, prepared: PreparedJob) -> Result<()> {
        self.manager
            .admit_reserved_exposed_task(
                prepared.graph,
                prepared.plans,
                ReservedExposedTaskAdmission {
                    id,
                    task: &prepared.task,
                    admission_sequence: 1,
                    width: prepared.width,
                },
            )
            .map_err(job_admission_error)
    }

    pub(crate) fn reserve_graph_admission(
        &self,
        ctx: &ToolContext,
        exposed: bool,
    ) -> Result<GraphAdmissionReservation> {
        let task = self.task_identity(ctx)?;
        let id = new_job_id()?;
        self.manager
            .reserve_task_admission(&id, exposed)
            .map_err(job_admission_error)?;
        let (exposure_owner, exposure_ref) = match self.reserve_finite_exposure(&id, &task) {
            Ok(value) => value,
            Err(error) => {
                let _ = self.manager.release_task_admission(&id);
                return Err(error);
            }
        };
        Ok(GraphAdmissionReservation {
            id,
            task,
            exposure_owner,
            exposure_ref,
            exposed,
        })
    }

    pub(crate) fn release_graph_admission(
        &self,
        reservation: &GraphAdmissionReservation,
    ) -> Result<()> {
        let manager = self.manager.release_task_admission(&reservation.id);
        let exposure = self.release_unpublished_finite_exposure(
            &reservation.id,
            &reservation.exposure_owner,
            &reservation.exposure_ref,
        );
        manager.map_err(job_admission_error)?;
        exposure
    }

    pub(crate) fn admit_graph_reservation(
        &self,
        reservation: GraphAdmissionReservation,
        prepared: PreparedJob,
    ) -> Result<String> {
        self.admit_graph_reservation_with_wake(reservation, prepared, None, None)
    }

    pub(crate) fn admit_graph_reservation_with_wake(
        &self,
        reservation: GraphAdmissionReservation,
        prepared: PreparedJob,
        actor: Option<&PrincipalId>,
        initial_wake: Option<(&str, &str, EventRuleSet)>,
    ) -> Result<String> {
        if prepared.task != reservation.task {
            let _ = self.release_graph_admission(&reservation);
            return Err(Error::Denied(
                "pre-seal Graph policy changed the owning Task identity",
            ));
        }
        let admission = self.manager.admit_pre_reserved_task(
            prepared.graph,
            prepared.plans,
            TaskAdmission {
                id: &reservation.id,
                task: &reservation.task,
                admission_sequence: 1,
                width: prepared.width,
                exposed: reservation.exposed,
            },
        );
        if let Err(error) = admission {
            let _ = self.manager.release_task_admission(&reservation.id);
            let _ = self.release_unpublished_finite_exposure(
                &reservation.id,
                &reservation.exposure_owner,
                &reservation.exposure_ref,
            );
            return Err(job_admission_error(error));
        }
        if let Some((recipient, authority_ref, rules)) = initial_wake {
            let actor = actor.ok_or(Error::Protocol(
                "initial Graph wake registration lacks authenticated actor",
            ))?;
            if let Err(error) = self.subscribe_finite(
                actor,
                &reservation.id,
                None,
                None,
                recipient,
                authority_ref,
                rules,
            ) {
                let _ = self.discard_unstarted_with_exposure(&reservation.id);
                return Err(error);
            }
        }
        if reservation.exposed {
            if let Err(error) = self.publish_finite_exposure(&reservation.id) {
                let _ = self.discard_unstarted_with_exposure(&reservation.id);
                return Err(error);
            }
            self.manager
                .expose(&reservation.id)
                .map_err(|_| Error::Protocol("reserved Graph exposure invariant failed"))?;
        }
        Ok(reservation.id)
    }

    pub fn admit(&self, prepared: PreparedJob) -> Result<String> {
        let id = new_job_id()?;
        let (owner, reference) = self.reserve_finite_exposure(&id, &prepared.task)?;
        if let Err(error) = self.admit_with_id(&id, prepared) {
            let _ = self.release_unpublished_finite_exposure(&id, &owner, &reference);
            return Err(error);
        }
        Ok(id)
    }

    pub fn admit_exposed(&self, prepared: PreparedJob) -> Result<String> {
        let id = new_job_id()?;
        let (owner, reference) = self.reserve_finite_exposure(&id, &prepared.task)?;
        if let Err(error) = self.reserve_exposed_with_id(&id, prepared) {
            let _ = self.release_unpublished_finite_exposure(&id, &owner, &reference);
            return Err(error);
        }
        if let Err(error) = self.publish_finite_exposure(&id) {
            let _ = self.discard_unstarted_with_exposure(&id);
            return Err(error);
        }
        self.manager
            .expose(&id)
            .map_err(|_| Error::Protocol("reserved Job exposure invariant failed"))?;
        Ok(id)
    }

    /// Live-session replay dedupe. Cross-process safety is provided by AW's
    /// existing unresolved outer ToolStarted intent; AW never redispatches that
    /// effect after restart, so no persistent Graph/tombstone database is added.
    pub fn admit_replay(
        &self,
        prepared: PreparedJob,
        namespace: &str,
        request_id: &str,
    ) -> Result<AdmissionDecision> {
        self.admit_replay_with_visibility(prepared, namespace, request_id, false, None, None)
    }

    pub fn admit_replay_exposed(
        &self,
        prepared: PreparedJob,
        namespace: &str,
        request_id: &str,
    ) -> Result<AdmissionDecision> {
        self.admit_replay_with_visibility(prepared, namespace, request_id, true, None, None)
    }

    pub(crate) fn admit_replay_with_initial_wake(
        &self,
        prepared: PreparedJob,
        namespace: &str,
        request_id: &str,
        reserve_exposed: bool,
        actor: &PrincipalId,
        recipient_ref: &str,
        recipient_authority_ref: &str,
        rules: EventRuleSet,
    ) -> Result<AdmissionDecision> {
        self.admit_replay_with_visibility(
            prepared,
            namespace,
            request_id,
            reserve_exposed,
            Some(actor),
            Some((recipient_ref, recipient_authority_ref, rules)),
        )
    }

    fn admit_replay_with_visibility(
        &self,
        prepared: PreparedJob,
        namespace: &str,
        request_id: &str,
        reserve_exposed: bool,
        actor: Option<&PrincipalId>,
        initial_wake: Option<(&str, &str, EventRuleSet)>,
    ) -> Result<AdmissionDecision> {
        let semantics = prepared.replay_semantic_hash(
            initial_wake
                .as_ref()
                .map(|(recipient, authority_ref, rules)| (*recipient, *authority_ref, rules)),
        );
        if let Some(existing) = self
            .manager
            .lookup_replay(namespace, request_id, semantics)
            .map_err(replay_state_error)?
        {
            return Ok(match existing {
                ReplayDecision::Existing { job_id }
                    if self.manager.runtime_snapshot(&job_id).is_ok() =>
                {
                    if reserve_exposed {
                        self.expose(&job_id)?;
                    }
                    AdmissionDecision::Existing(job_id)
                }
                ReplayDecision::Existing { .. } | ReplayDecision::Unknown => {
                    AdmissionDecision::Unknown
                }
                ReplayDecision::New { .. } => unreachable!("lookup never creates replay work"),
            });
        }

        let proposed = new_job_id()?;
        // Reserve both finite-owner metadata and the host/project exposure slot
        // before committing replay identity or starting any root.
        let (exposure_owner, exposure_ref) =
            self.reserve_finite_exposure(&proposed, &prepared.task)?;
        let admission = if reserve_exposed {
            self.reserve_exposed_with_id(&proposed, prepared)
        } else {
            self.admit_with_id(&proposed, prepared)
        };
        if let Err(error) = admission {
            let _ =
                self.release_unpublished_finite_exposure(&proposed, &exposure_owner, &exposure_ref);
            return Err(error);
        }
        let decision =
            match self
                .manager
                .claim_replay(namespace, request_id, semantics, &proposed, |_| {
                    Ok(ReplayPersist::Created)
                }) {
                Ok(decision) => decision,
                Err(error) => {
                    self.discard_unstarted_with_exposure(&proposed)?;
                    return Err(replay_state_error(error));
                }
            };
        match decision {
            ReplayDecision::Existing { job_id } => {
                self.discard_unstarted_with_exposure(&proposed)?;
                if self.manager.runtime_snapshot(&job_id).is_ok() {
                    if reserve_exposed {
                        self.expose(&job_id)?;
                    }
                    Ok(AdmissionDecision::Existing(job_id))
                } else {
                    Ok(AdmissionDecision::Unknown)
                }
            }
            ReplayDecision::Unknown => {
                self.discard_unstarted_with_exposure(&proposed)?;
                Ok(AdmissionDecision::Unknown)
            }
            ReplayDecision::New { key, job_id } => {
                if job_id != proposed {
                    self.discard_unstarted_with_exposure(&proposed)?;
                    return Err(Error::Protocol(
                        "replay claim returned a different proposed Job",
                    ));
                }
                if let Some((recipient, authority_ref, rules)) = initial_wake.as_ref() {
                    let actor = actor.ok_or(Error::Protocol(
                        "initial replay wake registration lacks authenticated actor",
                    ))?;
                    if let Err(error) = self.subscribe_finite(
                        actor,
                        &job_id,
                        None,
                        None,
                        recipient,
                        authority_ref,
                        rules.clone(),
                    ) {
                        self.discard_unstarted_with_exposure(&job_id)?;
                        return Err(error);
                    }
                }
                if let Err(error) = self.manager.mark_replay_ready(key, &job_id, |_| Ok(())) {
                    self.discard_unstarted_with_exposure(&job_id)?;
                    return Err(replay_state_error(error));
                }
                if reserve_exposed {
                    self.expose(&job_id)?;
                }
                Ok(AdmissionDecision::New(job_id))
            }
        }
    }

    pub fn expose(&self, id: &str) -> Result<()> {
        self.manager
            .reserve_exposed(id)
            .map_err(job_admission_error)?;
        self.publish_finite_exposure(id)?;
        self.manager
            .expose(id)
            .map_err(|_| Error::Protocol("reserved Job exposure invariant failed"))?;
        Ok(())
    }

    pub fn discard_unexposed_terminal(&self, id: &str) -> Result<()> {
        let (owner, reference) = self.finite_exposure_for_id(id)?;
        if let Ok(mut events) = self.events.lock() {
            events.sources.remove(&reference);
            events.terminal_committed.remove(&reference);
        }
        self.manager
            .discard_unexposed_terminal(id)
            .map_err(|_| Error::Protocol("cannot retire completed hidden Job"))?;
        self.release_unpublished_finite_exposure(id, &owner, &reference)
    }

    /// Revalidate and create a fresh execution from one retained terminal Graph.
    /// The source Job is preserved. Duplicate delivery of this exact restart
    /// call returns the same new Job through the live replay ledger.
    pub fn restart(&self, ctx: &ToolContext, source_id: &str) -> Result<AdmissionDecision> {
        let source = self
            .manager
            .get(source_id)
            .map_err(|_| Error::Denied("restart source Job is not exposed or missing"))?;
        if source.lifecycle != aw_tools::filesystem::job::Lifecycle::Terminal {
            return Err(Error::Denied("running Job cannot restart; cancel first"));
        }
        let graph = self
            .manager
            .graph(source_id)
            .map_err(|_| Error::Protocol("restart source Graph unavailable"))?;
        if graph.steps().len() > self.max_graph_nodes {
            return Err(Error::Limit(
                "restart source Graph exceeds the configured node ceiling",
            ));
        }
        let plans = self.plans_for_graph(ctx, &graph)?;
        let task = self.task_identity(ctx)?;
        let semantics = semantic_hash(&graph, &plans)
            .map_err(|_| Error::Denied("restart semantic identity failed"))?;
        let width = self.parallelism.width(&graph);
        let namespace = format!("restart:{}:{source_id}", self.session);
        if let Some(existing) = self
            .manager
            .lookup_replay(&namespace, &ctx.call_id, semantics)
            .map_err(replay_state_error)?
        {
            return Ok(match existing {
                ReplayDecision::Existing { job_id }
                    if self.manager.runtime_snapshot(&job_id).is_ok() =>
                {
                    AdmissionDecision::Existing(job_id)
                }
                ReplayDecision::Existing { .. } | ReplayDecision::Unknown => {
                    AdmissionDecision::Unknown
                }
                ReplayDecision::New { .. } => unreachable!("lookup never creates replay work"),
            });
        }

        let proposed = new_job_id()?;
        self.manager
            .restart_reserved_exposed_task(
                source_id,
                graph,
                plans,
                ReservedExposedTaskAdmission {
                    id: &proposed,
                    task: &task,
                    admission_sequence: 1,
                    width,
                },
            )
            .map_err(job_admission_error)?;
        let decision =
            match self
                .manager
                .claim_replay(&namespace, &ctx.call_id, semantics, &proposed, |_| {
                    Ok(ReplayPersist::Created)
                }) {
                Ok(decision) => decision,
                Err(error) => {
                    self.manager
                        .discard_unstarted(&proposed)
                        .map_err(|_| Error::Protocol("restart provisional cleanup failed"))?;
                    return Err(replay_state_error(error));
                }
            };
        match decision {
            ReplayDecision::Existing { job_id } => {
                self.manager
                    .discard_unstarted(&proposed)
                    .map_err(|_| Error::Protocol("duplicate restart provisional cleanup failed"))?;
                if self.manager.runtime_snapshot(&job_id).is_ok() {
                    Ok(AdmissionDecision::Existing(job_id))
                } else {
                    Ok(AdmissionDecision::Unknown)
                }
            }
            ReplayDecision::Unknown => {
                self.manager
                    .discard_unstarted(&proposed)
                    .map_err(|_| Error::Protocol("unknown restart provisional cleanup failed"))?;
                Ok(AdmissionDecision::Unknown)
            }
            ReplayDecision::New { key, job_id } => {
                if job_id != proposed {
                    self.manager
                        .discard_unstarted(&proposed)
                        .map_err(|_| Error::Protocol("mismatched restart cleanup failed"))?;
                    return Err(Error::Protocol(
                        "restart replay claim changed proposed Job ID",
                    ));
                }
                if let Err(error) = self.manager.mark_replay_ready(key, &job_id, |_| Ok(())) {
                    self.manager.discard_unstarted(&job_id).map_err(|_| {
                        Error::Protocol("unready restart provisional cleanup failed")
                    })?;
                    return Err(replay_state_error(error));
                }
                self.manager
                    .expose(&job_id)
                    .map_err(|_| Error::Protocol("reserved restart exposure invariant failed"))?;
                Ok(AdmissionDecision::New(job_id))
            }
        }
    }

    fn current_binding(&self, actor: &PrincipalId, plan: &NodePlan) -> Option<ToolBindingSnapshot> {
        let binding = self
            .authority
            .permitted_binding(actor, plan.tool().name())
            .ok()?;
        let runtime_live = if plan.tool().native().is_some() {
            plan.runtime_binding() == self.root_binding
        } else if plan.tool().is_shell() {
            self.shells.get(plan.tool()).is_some_and(|shell| {
                shell
                    .runtime_binding()
                    .is_ok_and(|runtime| runtime == plan.runtime_binding())
                    && shell.revalidate().is_ok()
            })
        } else if plan.tool().is_agent() {
            self.graph_host_tools
                .binding(aw_tools::OperationId::Agent)
                .ok()
                .flatten()
                .is_some_and(|host| {
                    host.runtime_binding() == plan.runtime_binding() && host.tool().is_available()
                })
        } else {
            false
        };
        (binding.registry_revision == plan.registry_generation()
            && binding_hash(actor, &binding) == plan.authority_binding()
            && runtime_live)
            .then_some(binding)
    }

    fn node_live(&self, actor: &PrincipalId, id: &str, coordinate: usize) -> bool {
        self.manager
            .plans(id)
            .ok()
            .and_then(|plans| plans.get(coordinate.saturating_sub(1)).cloned())
            .and_then(|plan| self.current_binding(actor, &plan))
            .is_some()
    }

    /// Revalidate a node after Claim but before Start. Any authority/runtime
    /// loss in this narrow window is still a proven no-launch outcome, so close
    /// the Claim instead of returning with a live permit stranded.
    fn claimed_binding(
        &self,
        actor: &PrincipalId,
        id: &str,
        coordinate: usize,
        claimed_sequence: u64,
        plan: &NodePlan,
    ) -> Result<ToolBindingSnapshot> {
        if let Some(binding) = self.current_binding(actor, plan) {
            return Ok(binding);
        }
        self.settle_unlaunched(id, coordinate, claimed_sequence)?;
        Err(Error::Denied(
            "Job node binding changed or was revoked before execution",
        ))
    }

    /// Payload authority is stricter than status/control authority: every
    /// retained node plan must still match the current host-minted authority
    /// generation. Revocation/regrant never reopens old result bytes.
    pub fn payload_authorized(&self, actor: &PrincipalId, id: &str) -> bool {
        let Ok(plans) = self.manager.plans(id) else {
            return false;
        };
        plans
            .iter()
            .all(|plan| self.current_binding(actor, plan).is_some())
    }

    fn transition_sequence(&self, id: &str) -> Result<u64> {
        self.manager
            .current_sequence(id)
            .map_err(|_| Error::Denied("Job is missing or unavailable"))?
            .checked_add(1)
            .ok_or(Error::Limit("Job transition sequence exhausted"))
    }

    fn claim_exact(
        &self,
        actor: &PrincipalId,
        id: &str,
        coordinate: usize,
    ) -> Result<([u8; 32], u64)> {
        let _gate = self
            .transition_gate
            .lock()
            .map_err(|_| Error::Protocol("Job transition gate poisoned"))?;
        let sequence = self.transition_sequence(id)?;
        let live = self.node_live(actor, id, coordinate);
        let key = self
            .manager
            .claim(id, coordinate, live, sequence, |_, _| Ok(()))
            .map_err(|_| Error::Denied("Job node claim refused"))?;
        Ok((key, sequence))
    }

    pub fn claim(&self, actor: &PrincipalId, id: &str, coordinate: usize) -> Result<[u8; 32]> {
        self.claim_exact(actor, id, coordinate).map(|(key, _)| key)
    }

    fn start_exact(
        &self,
        actor: &PrincipalId,
        id: &str,
        coordinate: usize,
        claimed_sequence: u64,
        launch_authorized: bool,
    ) -> Result<u64> {
        let _gate = self
            .transition_gate
            .lock()
            .map_err(|_| Error::Protocol("Job transition gate poisoned"))?;
        let sequence = self.transition_sequence(id)?;
        let live = self.node_live(actor, id, coordinate);
        self.manager
            .start(
                id,
                StartRequest {
                    coordinate,
                    claimed_sequence,
                    source_live: live,
                    launch_authorized,
                    sequence,
                },
                |_, _| Ok(()),
            )
            .map_err(|_| Error::Denied("Job node start refused"))?;
        Ok(sequence)
    }

    pub fn start(
        &self,
        actor: &PrincipalId,
        id: &str,
        coordinate: usize,
        claimed_sequence: u64,
        launch_authorized: bool,
    ) -> Result<()> {
        self.start_exact(actor, id, coordinate, claimed_sequence, launch_authorized)
            .map(|_| ())
    }

    pub fn settle(
        &self,
        id: &str,
        coordinate: usize,
        expected: NodeState,
        result: Status,
    ) -> Result<()> {
        let _gate = self
            .transition_gate
            .lock()
            .map_err(|_| Error::Protocol("Job transition gate poisoned"))?;
        let sequence = self.transition_sequence(id)?;
        self.manager
            .settle(id, coordinate, expected, result, sequence, |_, _| Ok(()))
            .map_err(|_| Error::Protocol("Job node settlement failed"))
    }

    fn settle_unlaunched(&self, id: &str, coordinate: usize, claimed_sequence: u64) -> Result<()> {
        let _gate = self
            .transition_gate
            .lock()
            .map_err(|_| Error::Protocol("Job transition gate poisoned"))?;
        let sequence = self.transition_sequence(id)?;
        self.manager
            .settle_unlaunched(
                id,
                coordinate,
                claimed_sequence,
                sequence,
                |_node, observed_claim| {
                    if observed_claim == claimed_sequence {
                        Ok(())
                    } else {
                        Err(aw_tools::filesystem::job::Error::StaleGeneration)
                    }
                },
                |_, _| Ok(()),
            )
            .map_err(|_| Error::Protocol("claimed Job no-launch settlement failed"))
    }

    fn attach_exact_result(&self, id: &str, coordinate: usize, capacity: usize) -> Result<()> {
        let owners = [self
            .manager
            .job_output_owner(id)
            .map_err(|_| Error::Protocol("Job output owner unavailable"))?];
        self.manager
            .attach_output_reserved(
                id,
                coordinate,
                OutputStream::Result,
                owners,
                capacity as u64,
            )
            .map_err(|error| match error {
                aw_tools::filesystem::job_manager::Error::Capacity => {
                    Error::Limit("Job retained-result quota exhausted")
                }
                _ => Error::Protocol("Job retained-result attachment failed"),
            })
    }

    pub fn result_page(
        &self,
        id: &str,
        coordinate: usize,
        offset: u64,
        limit: usize,
    ) -> Result<OutputPage> {
        self.manager
            .output_page(id, coordinate, OutputStream::Result, offset, limit)
            .map_err(|_| Error::Denied("Job result output is unavailable"))
    }

    pub fn job_result_owner(&self, id: &str) -> Result<OutputOwner> {
        self.manager
            .job_output_owner(id)
            .map_err(|_| Error::Denied("Job result owner is unavailable"))
    }

    pub fn task_result_owner(&self, id: &str, coordinate: usize) -> Result<OutputOwner> {
        self.manager
            .task_result_owner(id, coordinate)
            .map_err(|_| Error::Denied("Task result owner is unavailable"))
    }

    pub fn acknowledge_result(
        &self,
        id: &str,
        coordinate: usize,
        owner: OutputOwner,
        through: u64,
    ) -> Result<OutputAck> {
        self.manager
            .acknowledge_output(id, coordinate, OutputStream::Result, owner, through)
            .map_err(|_| Error::Denied("Job result acknowledgement refused"))
    }

    fn finish_shell_streams(&self, id: &str, coordinate: usize) {
        let _ = self
            .manager
            .finish_output(id, coordinate, OutputStream::Stdout);
        let _ = self
            .manager
            .finish_output(id, coordinate, OutputStream::Stderr);
    }

    fn settle_shell_status(
        &self,
        id: &str,
        coordinate: usize,
        running_sequence: u64,
        terminal: Option<aw_tools::filesystem::runner::ProcessTerminal>,
        status: Status,
    ) -> Result<Status> {
        if let Some(terminal) = terminal {
            let _gate = self
                .transition_gate
                .lock()
                .map_err(|_| Error::Protocol("Job transition gate poisoned"))?;
            let sequence = self.transition_sequence(id)?;
            self.manager
                .settle_process_with_status(
                    id,
                    ProcessSettlement {
                        coordinate,
                        expected: NodeState::Running {
                            sequence: running_sequence,
                        },
                        terminal,
                        status,
                        sequence,
                    },
                    |_, _| Ok(()),
                )
                .map_err(|_| Error::Protocol("ShellNode terminal settlement failed"))
        } else {
            self.settle(
                id,
                coordinate,
                NodeState::Running {
                    sequence: running_sequence,
                },
                status,
            )?;
            Ok(status)
        }
    }

    fn settle_shell_result(
        &self,
        id: &str,
        coordinate: usize,
        running_sequence: u64,
        content: String,
        terminal: Option<aw_tools::filesystem::runner::ProcessTerminal>,
        status: Status,
    ) -> Result<Status> {
        if content.len() > SHELL_RESULT_BYTES {
            let unknown = Status {
                primary: Primary::Unknown,
                more: false,
                incomplete: false,
            };
            self.settle_shell_status(id, coordinate, running_sequence, terminal, unknown)?;
            return Err(Error::Limit(
                "ShellNode terminal metadata exceeds compact result ceiling",
            ));
        }
        let retained = self
            .manager
            .append_output(id, coordinate, OutputStream::Result, content.as_bytes())
            .map_err(|_| Error::Protocol("ShellNode result retention failed"))?;
        if retained != content.len()
            || self
                .manager
                .finish_output(id, coordinate, OutputStream::Result)
                .is_err()
        {
            let unknown = Status {
                primary: Primary::Unknown,
                more: false,
                incomplete: false,
            };
            self.settle_shell_status(id, coordinate, running_sequence, terminal, unknown)?;
            return Err(Error::Protocol(
                "ShellNode compact result could not be retained exactly",
            ));
        }
        self.settle_shell_status(id, coordinate, running_sequence, terminal, status)
    }

    fn retained_process_terminal(
        &self,
        id: &str,
        coordinate: usize,
        exit: aw_tools::filesystem::runner::ProcessExit,
        process_output_complete: bool,
    ) -> Result<aw_tools::filesystem::runner::ProcessTerminal> {
        use aw_tools::filesystem::{
            job_manager::OutputStream,
            runner::{ProcessOutputMeta, ProcessTerminal},
        };
        let (stdout, stdout_finished) = self
            .manager
            .output_summary(id, coordinate, OutputStream::Stdout)
            .map_err(|_| Error::Protocol("ShellNode stdout summary unavailable"))?;
        let (stderr, stderr_finished) = self
            .manager
            .output_summary(id, coordinate, OutputStream::Stderr)
            .map_err(|_| Error::Protocol("ShellNode stderr summary unavailable"))?;
        Ok(ProcessTerminal {
            exit,
            output: ProcessOutputMeta {
                stdout_bytes: stdout.total_bytes,
                stderr_bytes: stderr.total_bytes,
                stdout_truncated: stdout.truncated,
                stderr_truncated: stderr.truncated,
                complete: process_output_complete && stdout_finished && stderr_finished,
            },
        })
    }

    async fn execute_shell_node(
        &self,
        ctx: &ToolContext,
        id: &str,
        coordinate: usize,
        cancellation: CancellationToken,
    ) -> Result<Status> {
        use aw_tools::filesystem::runner::{ProcessExit, ProcessTerminal};
        let plan = self
            .manager
            .plans(id)
            .map_err(|_| Error::Denied("Job plan is unavailable"))?
            .get(coordinate.saturating_sub(1))
            .cloned()
            .ok_or(Error::Denied("Job coordinate is invalid"))?;
        if !plan.tool().is_shell() {
            return Err(Error::Denied("Job node is not a ShellNode"));
        }
        let shell = self
            .shells
            .get(plan.tool())
            .ok_or(Error::Denied("prepared shell interpreter is unavailable"))?;
        let (_node_key, claimed_sequence) = self.claim_exact(&ctx.actor, id, coordinate)?;
        if cancellation.is_cancelled() {
            self.settle_unlaunched(id, coordinate, claimed_sequence)?;
            return Err(Error::Cancelled);
        }
        let binding = self.claimed_binding(&ctx.actor, id, coordinate, claimed_sequence, &plan)?;
        // Finish every fallible, side-effect-free launch preparation while the
        // node is still only Claimed. `Start` is the final canonical checkpoint
        // immediately before handing the prepared spec to the OS process host.
        let prepared = match shell.prepare(plan.arguments()) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.settle_unlaunched(id, coordinate, claimed_sequence)?;
                return Err(error);
            }
        };
        if let Err(error) = prepared.spec.validate() {
            self.settle_unlaunched(id, coordinate, claimed_sequence)?;
            return Err(error);
        }
        let stream_capacity = prepared.max_output_bytes as u64;
        let owner = match self.manager.job_output_owner(id) {
            Ok(owner) => owner,
            Err(_) => {
                self.settle_unlaunched(id, coordinate, claimed_sequence)?;
                return Err(Error::Protocol("ShellNode output owner unavailable"));
            }
        };
        let (stdout, stderr) = match self.manager.attach_shell_outputs(
            id,
            coordinate,
            owner,
            SHELL_RESULT_BYTES as u64,
            stream_capacity,
        ) {
            Ok(writers) => writers,
            Err(aw_tools::filesystem::job_manager::Error::Capacity) => {
                self.settle_unlaunched(id, coordinate, claimed_sequence)?;
                return Err(Error::Limit("ShellNode retained-output quota exhausted"));
            }
            Err(_) => {
                self.settle_unlaunched(id, coordinate, claimed_sequence)?;
                return Err(Error::Protocol("ShellNode output attachment failed"));
            }
        };
        if cancellation.is_cancelled() {
            self.settle_unlaunched(id, coordinate, claimed_sequence)?;
            return Err(Error::Cancelled);
        }
        let running_sequence =
            match self.start_exact(&ctx.actor, id, coordinate, claimed_sequence, true) {
                Ok(sequence) => sequence,
                Err(error) => {
                    self.settle_unlaunched(id, coordinate, claimed_sequence)?;
                    return Err(error);
                }
            };

        let mut process =
            match process::PipeProcess::spawn_with_job_outputs(prepared.spec, stdout, stderr).await
            {
                Ok(process) => process,
                Err(error) => {
                    self.finish_shell_streams(id, coordinate);
                    let detail = aw_tools::task::bounded_task_error(error.to_string());
                    let primary = if matches!(error, Error::Transport(_)) {
                        Primary::Unknown
                    } else {
                        Primary::Fail
                    };
                    return self.settle_shell_result(
                        id,
                        coordinate,
                        running_sequence,
                        format!("{} launch={detail}", primary.as_str()),
                        None,
                        Status {
                            primary,
                            more: false,
                            incomplete: false,
                        },
                    );
                }
            };
        if let Some(stdin) = &prepared.stdin {
            if let Err(write_error) = process.write(stdin).await {
                let stop = process.terminate().await;
                let (terminal, primary) = match stop {
                    Ok(receipt) => {
                        let observed = receipt.runner_terminal();
                        let terminal = self.retained_process_terminal(
                            id,
                            coordinate,
                            observed.exit,
                            observed.output.complete,
                        )?;
                        let primary =
                            if terminal.exit == ProcessExit::Unknown || !terminal.output.complete {
                                Primary::Unknown
                            } else {
                                Primary::Fail
                            };
                        (terminal, primary)
                    }
                    Err(_) => (
                        self.retained_process_terminal(id, coordinate, ProcessExit::Unknown, false)
                            .unwrap_or(ProcessTerminal {
                                exit: ProcessExit::Unknown,
                                output: Default::default(),
                            }),
                        Primary::Unknown,
                    ),
                };
                let detail = aw_tools::task::bounded_task_error(write_error.to_string());
                return self.settle_shell_result(
                    id,
                    coordinate,
                    running_sequence,
                    format!(
                        "{} stdin={} {}",
                        primary.as_str(),
                        detail,
                        terminal.compact_result()
                    ),
                    Some(terminal),
                    Status {
                        primary,
                        more: false,
                        incomplete: false,
                    },
                );
            }
        }
        if !prepared.interactive {
            process.close_stdin();
        }
        let authority = self.authority.clone();
        let actor = ctx.actor.clone();
        let binding_watch = binding.clone();
        let revoke_on_authority_loss = async {
            authority.wait_until_not_live(&actor, &binding_watch).await;
            let _ = self.revoke(id);
        };
        let stop_signal = async {
            tokio::select! {
                _ = cancellation.cancelled() => {},
                _ = revoke_on_authority_loss => {},
            }
        };
        let receipt = if prepared.interactive {
            let (input, receiver) = process::process_input_channel();
            let key = (id.to_owned(), coordinate);
            let replaced = self
                .process_inputs
                .lock()
                .map_err(|_| Error::Protocol("Process input registry poisoned"))?
                .insert(key.clone(), input);
            if replaced.is_some() {
                return Err(Error::Protocol("duplicate Process input owner"));
            }
            let receipt = process
                .wait_interactive(stop_signal, prepared.timeout, receiver)
                .await;
            self.process_inputs
                .lock()
                .map_err(|_| Error::Protocol("Process input registry poisoned"))?
                .remove(&key);
            receipt
        } else {
            process.wait(stop_signal, prepared.timeout).await
        };
        let observed = receipt.runner_terminal();
        let terminal = self.retained_process_terminal(
            id,
            coordinate,
            observed.exit,
            observed.output.complete,
        )?;
        let status = terminal.status();
        self.settle_shell_result(
            id,
            coordinate,
            running_sequence,
            terminal.compact_result(),
            Some(terminal),
            status,
        )
    }

    pub(crate) fn graph_preseal_request(
        &self,
        id: &str,
        original_graph: &Value,
        prepared: &PreparedJob,
    ) -> Result<crate::operation_router::GraphPresealRequest> {
        if prepared.plans.len() != prepared.graph.steps().len() {
            return Err(Error::Protocol(
                "Graph pre-seal plan/topology length changed",
            ));
        }
        let mut nodes = Vec::with_capacity(prepared.plans.len());
        for (step, plan) in prepared.graph.steps().iter().zip(prepared.plans.iter()) {
            let coordinate = u32::try_from(plan.coordinate())
                .map_err(|_| Error::Protocol("Graph pre-seal coordinate overflow"))?;
            let dependencies = step
                .when
                .dependencies()
                .into_iter()
                .map(|dependency| {
                    u32::try_from(dependency)
                        .map_err(|_| Error::Protocol("Graph dependency coordinate overflow"))
                })
                .collect::<Result<Vec<_>>>()?;
            nodes.push(crate::operation_router::GraphPresealNode {
                coordinate,
                operation: plan.tool().operation(),
                arguments: step.arguments.clone(),
                replay_identity: plan.replay_identity(),
                footprint: plan.footprint().clone(),
                dependencies,
            });
        }
        Ok(crate::operation_router::GraphPresealRequest {
            job_id: id.to_owned(),
            original_graph: original_graph.clone(),
            graph_fingerprint: prepared.graph.fingerprint(),
            nodes,
        })
    }

    fn graph_policy_request(
        &self,
        id: &str,
    ) -> Result<crate::operation_router::GraphPolicyRequest> {
        let graph = self
            .manager
            .graph(id)
            .map_err(|_| Error::Denied("Graph policy cannot read sealed Graph"))?;
        let plans = self
            .manager
            .plans(id)
            .map_err(|_| Error::Denied("Graph policy cannot read sealed NodePlans"))?;
        if plans.len() != graph.steps().len() {
            return Err(Error::Protocol("Graph policy plan/topology length changed"));
        }
        let mut nodes = Vec::with_capacity(plans.len());
        for (step, plan) in graph.steps().iter().zip(plans.iter()) {
            let coordinate = u32::try_from(plan.coordinate())
                .map_err(|_| Error::Protocol("Graph policy coordinate overflow"))?;
            let dependencies = step
                .when
                .dependencies()
                .into_iter()
                .map(|dependency| {
                    u32::try_from(dependency)
                        .map_err(|_| Error::Protocol("Graph dependency coordinate overflow"))
                })
                .collect::<Result<Vec<_>>>()?;
            nodes.push(crate::operation_router::GraphPolicyNode {
                coordinate,
                operation: plan.tool().operation(),
                replay_identity: plan.replay_identity(),
                footprint_fingerprint: plan.footprint().fingerprint(),
                dependencies,
            });
        }
        Ok(crate::operation_router::GraphPolicyRequest {
            job_id: id.to_owned(),
            graph_fingerprint: graph.fingerprint(),
            nodes,
        })
    }

    pub(crate) async fn before_graph_run(&self, ctx: &ToolContext, id: &str) -> Result<()> {
        let Some(router) = self.graph_policy.current()? else {
            return Ok(());
        };
        router.before_run(ctx, self.graph_policy_request(id)?).await
    }

    fn graph_node_policy_request(
        &self,
        id: &str,
        coordinate: usize,
    ) -> Result<crate::operation_router::GraphNodePolicyRequest> {
        let graph = self
            .manager
            .graph(id)
            .map_err(|_| Error::Denied("Graph activation cannot read sealed Graph"))?;
        let plans = self
            .manager
            .plans(id)
            .map_err(|_| Error::Denied("Graph activation cannot read NodePlans"))?;
        let step = graph
            .steps()
            .get(coordinate.saturating_sub(1))
            .ok_or(Error::Denied("Graph activation coordinate is invalid"))?;
        let plan = plans
            .get(coordinate.saturating_sub(1))
            .ok_or(Error::Denied("Graph activation NodePlan is unavailable"))?;
        let node_coordinate = NodeCoordinate::new(
            u32::try_from(coordinate)
                .map_err(|_| Error::Protocol("Graph activation coordinate overflow"))?,
        )
        .map_err(|_| Error::Protocol("Graph activation coordinate is invalid"))?;
        let graph_fingerprint = graph.fingerprint();
        let mut dependency_receipts = Vec::new();
        let mut activation_receipts = Vec::new();
        for dependency in step.when.dependencies() {
            let status = self
                .manager
                .node_state(id, dependency)
                .map_err(|_| Error::Protocol("Graph dependency state is unavailable"))?
                .result()
                .ok_or(Error::Protocol(
                    "Graph policy activation requires settled declared dependencies",
                ))?;
            let coordinate = NodeCoordinate::new(
                u32::try_from(dependency)
                    .map_err(|_| Error::Protocol("Graph dependency coordinate overflow"))?,
            )
            .map_err(|_| Error::Protocol("Graph dependency coordinate is invalid"))?;
            dependency_receipts.push(crate::operation_router::GraphDependencyReceipt {
                coordinate: coordinate.get(),
                status,
            });
            activation_receipts.push(aw_tools::coordination::ActivationReceipt {
                coordinate,
                outcome: status.primary,
                public_result: None,
            });
        }
        aw_tools::coordination::GraphActivationContext::new(
            &graph_fingerprint,
            node_coordinate,
            &activation_receipts,
        )
        .map_err(|_| Error::Protocol("Graph activation context is not dependency-closed"))?;
        Ok(crate::operation_router::GraphNodePolicyRequest {
            job_id: id.to_owned(),
            graph_fingerprint,
            coordinate: node_coordinate.get(),
            operation: plan.tool().operation(),
            replay_identity: plan.replay_identity(),
            dependency_receipts,
        })
    }

    async fn execute_node(
        self: &Arc<Self>,
        ctx: &ToolContext,
        id: &str,
        coordinate: usize,
        cancellation: CancellationToken,
    ) -> Result<Status> {
        let Some(router) = self.graph_policy.current()? else {
            return self
                .execute_node_local(ctx, id, coordinate, cancellation)
                .await;
        };
        let request = self.graph_node_policy_request(id, coordinate)?;
        let original: Arc<dyn crate::operation_router::GraphNodeOriginalExecutor> =
            Arc::new(PreparedGraphNodeExecutor {
                host: self.clone(),
                ctx: ctx.clone(),
                id: id.to_owned(),
                coordinate,
                cancellation,
            });
        router.execute_node(ctx, request, original).await
    }

    async fn execute_node_local(
        &self,
        ctx: &ToolContext,
        id: &str,
        coordinate: usize,
        cancellation: CancellationToken,
    ) -> Result<Status> {
        let tool = self
            .manager
            .plans(id)
            .map_err(|_| Error::Denied("Job plan is unavailable"))?
            .get(coordinate.saturating_sub(1))
            .map(NodePlan::tool)
            .ok_or(Error::Denied("Job coordinate is invalid"))?;
        if tool.is_shell() {
            self.execute_shell_node(ctx, id, coordinate, cancellation)
                .await
        } else if tool.is_agent() {
            self.execute_host_tool_node(ctx, id, coordinate, cancellation)
                .await
        } else {
            self.execute_native_node(ctx, id, coordinate, cancellation)
                .await
        }
    }

    async fn execute_host_tool_node(
        &self,
        ctx: &ToolContext,
        id: &str,
        coordinate: usize,
        cancellation: CancellationToken,
    ) -> Result<Status> {
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let plan = self
            .manager
            .plans(id)
            .map_err(|_| Error::Denied("Job plan is unavailable"))?
            .get(coordinate.saturating_sub(1))
            .cloned()
            .ok_or(Error::Denied("Job coordinate is invalid"))?;
        if !plan.tool().is_agent() {
            return Err(Error::Denied("Job node is not a host Agent node"));
        }

        let (_node_key, claimed_sequence) = self.claim_exact(&ctx.actor, id, coordinate)?;
        if cancellation.is_cancelled() {
            self.settle_unlaunched(id, coordinate, claimed_sequence)?;
            return Err(Error::Cancelled);
        }
        let binding = self.claimed_binding(&ctx.actor, id, coordinate, claimed_sequence, &plan)?;
        let host = self
            .graph_host_tools
            .binding(aw_tools::OperationId::Agent)?
            .ok_or(Error::Denied(
                "prepared Agent Graph executor is unavailable",
            ))?;
        if host.runtime_binding() != plan.runtime_binding() {
            self.settle_unlaunched(id, coordinate, claimed_sequence)?;
            return Err(Error::Denied(
                "prepared Agent Graph executor generation changed before execution",
            ));
        }
        let tool = host.tool();
        if !tool.is_available() || tool.validate(plan.arguments()).is_err() {
            self.settle_unlaunched(id, coordinate, claimed_sequence)?;
            return Err(Error::Denied(
                "prepared Agent Graph arguments/executor are no longer valid",
            ));
        }
        if let Err(error) = self.attach_exact_result(id, coordinate, plan.budget().result_bytes) {
            self.settle_unlaunched(id, coordinate, claimed_sequence)?;
            return Err(error);
        }

        let running_sequence =
            match self.start_exact(&ctx.actor, id, coordinate, claimed_sequence, true) {
                Ok(sequence) => sequence,
                Err(error) => {
                    self.settle_unlaunched(id, coordinate, claimed_sequence)?;
                    return Err(error);
                }
            };

        let mut hash = Sha256::new();
        hash.update(b"aw-graph-host-node-call-v1\0");
        for part in [ctx.call_id.as_bytes(), id.as_bytes()] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part);
        }
        hash.update((coordinate as u64).to_be_bytes());
        hash.update(plan.replay_identity());
        let call_id = format!("gn_{:x}", hash.finalize());
        let ownership = ExecutionOwnershipScope::from_trusted_host(
            self.execution_ref(id, Some(coordinate))?.canonical(),
            cancellation.clone(),
        )?;
        let node_ctx = ToolContext {
            actor: ctx.actor.clone(),
            turn_id: ctx.turn_id,
            call_id,
            source: binding.source.clone(),
            effect: binding.effect.clone(),
            execution_site: binding.execution_site.clone(),
            cancellation: cancellation.clone(),
            result_budget_bytes: plan.budget().result_bytes,
            identity: ctx.identity.clone(),
            scope: ctx.scope.clone(),
            execution_owner: ctx.execution_owner.clone(),
            execution_scope: Some(ownership.clone()),
        };

        let output = tool.execute(node_ctx, plan.arguments()).await;
        ownership.wait_quiescent().await;
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                let _ = self
                    .manager
                    .finish_output(id, coordinate, OutputStream::Result);
                let unknown = Status {
                    primary: Primary::Unknown,
                    more: false,
                    incomplete: ownership.is_unresolved(),
                };
                self.settle(
                    id,
                    coordinate,
                    NodeState::Running {
                        sequence: running_sequence,
                    },
                    unknown,
                )?;
                return Err(error);
            }
        };
        output.validate()?;
        if output.content.len() > plan.budget().result_bytes {
            let _ = self
                .manager
                .finish_output(id, coordinate, OutputStream::Result);
            let unknown = Status {
                primary: Primary::Unknown,
                more: false,
                incomplete: false,
            };
            self.settle(
                id,
                coordinate,
                NodeState::Running {
                    sequence: running_sequence,
                },
                unknown,
            )?;
            return Err(Error::Limit(
                "Agent Graph node result exceeds captured semantic ceiling",
            ));
        }
        let retained = self
            .manager
            .append_output(
                id,
                coordinate,
                OutputStream::Result,
                output.content.as_bytes(),
            )
            .map_err(|_| Error::Protocol("Agent Graph node result retention failed"))?;
        if retained != output.content.len()
            || self
                .manager
                .finish_output(id, coordinate, OutputStream::Result)
                .is_err()
        {
            let unknown = Status {
                primary: Primary::Unknown,
                more: false,
                incomplete: false,
            };
            self.settle(
                id,
                coordinate,
                NodeState::Running {
                    sequence: running_sequence,
                },
                unknown,
            )?;
            return Err(Error::Protocol(
                "Agent Graph node result could not be retained exactly",
            ));
        }
        let status = if ownership.is_unresolved() {
            Status {
                primary: Primary::Unknown,
                more: false,
                incomplete: true,
            }
        } else {
            Status {
                primary: if output.is_error {
                    Primary::Fail
                } else {
                    Primary::Ok
                },
                more: false,
                incomplete: false,
            }
        };
        self.settle(
            id,
            coordinate,
            NodeState::Running {
                sequence: running_sequence,
            },
            status,
        )?;
        Ok(status)
    }

    /// Execute one V1 native node. Result capacity is reserved while Claimed,
    /// before durable-live Start releases the node into a filesystem effect.
    pub async fn execute_native_node(
        &self,
        ctx: &ToolContext,
        id: &str,
        coordinate: usize,
        cancellation: CancellationToken,
    ) -> Result<Status> {
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let plan = self
            .manager
            .plans(id)
            .map_err(|_| Error::Denied("Job plan is unavailable"))?
            .get(coordinate.saturating_sub(1))
            .cloned()
            .ok_or(Error::Denied("Job coordinate is invalid"))?;
        let native = plan
            .tool()
            .native()
            .ok_or(Error::Denied("Job node is not a native filesystem node"))?;
        let (_node_key, claimed_sequence) = self.claim_exact(&ctx.actor, id, coordinate)?;
        if cancellation.is_cancelled() {
            self.settle_unlaunched(id, coordinate, claimed_sequence)?;
            return Err(Error::Cancelled);
        }
        let _binding = self.claimed_binding(&ctx.actor, id, coordinate, claimed_sequence, &plan)?;
        if let Err(error) = self.attach_exact_result(id, coordinate, plan.budget().result_bytes) {
            self.settle_unlaunched(id, coordinate, claimed_sequence)?;
            return Err(error);
        }
        // Construct every fallible semantic input while the node is still only
        // Claimed. Once Start commits, execution goes through the canonical
        // shared direct engine; AW contributes only authority and projection.
        if self.manager.task_binding(id).is_err() {
            self.settle_unlaunched(id, coordinate, claimed_sequence)?;
            return Err(Error::Protocol("Job Task binding unavailable"));
        }
        let request = match crate::canonical_request(native, plan.arguments(), self.tool_limits) {
            Ok(request) => request,
            Err(error) => {
                self.settle_unlaunched(id, coordinate, claimed_sequence)?;
                return Err(error);
            }
        };
        let current_footprint = match request.footprint(self.root.as_ref()) {
            Ok(footprint) => footprint,
            Err(_) => {
                self.settle_unlaunched(id, coordinate, claimed_sequence)?;
                return Err(Error::Denied(
                    "filesystem topology changed after Job preflight; restart the Graph",
                ));
            }
        };
        let current_within_sealed = plan
            .footprint()
            .union(&current_footprint)
            .is_ok_and(|union| union == *plan.footprint());
        if !current_within_sealed {
            self.settle_unlaunched(id, coordinate, claimed_sequence)?;
            return Err(Error::Denied(
                "filesystem topology changed after Job preflight; restart the Graph",
            ));
        }
        let direct_limits =
            crate::canonical_direct_limits(self.tool_limits, plan.budget().result_bytes, &request);
        let running_sequence =
            match self.start_exact(&ctx.actor, id, coordinate, claimed_sequence, true) {
                Ok(sequence) => sequence,
                Err(error) => {
                    self.settle_unlaunched(id, coordinate, claimed_sequence)?;
                    return Err(error);
                }
            };
        let root = self.root.clone();
        let token = cancellation.clone();
        let executed = aw_tools::asynchronous::run_blocking(move || {
            aw_tools::filesystem::direct::execute(root.as_ref(), request, direct_limits, &|| {
                token.is_cancelled()
            })
        })
        .await;
        let (output, status) = match executed {
            Ok(Ok(output)) => {
                let status = aw_tools::filesystem::direct::graph_status(native, &output);
                let content = aw_tools::filesystem::direct::graph_text(&output).ok_or(
                    Error::Protocol("direct-only media result reached native Job execution"),
                );
                match content {
                    Ok(content) => (
                        ToolOutput {
                            content,
                            is_error: output.is_error,
                            truncated: output.truncated,
                        },
                        status,
                    ),
                    Err(error) => error_result(native, error),
                }
            }
            Ok(Err(error)) => direct_error_result(error),
            Err(_) => error_result(
                native,
                Error::Protocol(
                    "canonical filesystem worker failed; inspect workspace before retrying",
                ),
            ),
        };
        if output.content.len() > plan.budget().result_bytes {
            let _ = self
                .manager
                .finish_output(id, coordinate, OutputStream::Result);
            let unknown = Status {
                primary: Primary::Unknown,
                more: false,
                incomplete: false,
            };
            self.settle(
                id,
                coordinate,
                NodeState::Running {
                    sequence: running_sequence,
                },
                unknown,
            )?;
            return Err(Error::Limit("Job result exceeds captured semantic ceiling"));
        }
        let retained = self
            .manager
            .append_output(
                id,
                coordinate,
                OutputStream::Result,
                output.content.as_bytes(),
            )
            .map_err(|_| Error::Protocol("Job result spool write failed"))?;
        if retained != output.content.len()
            || self
                .manager
                .finish_output(id, coordinate, OutputStream::Result)
                .is_err()
        {
            let unknown = Status {
                primary: Primary::Unknown,
                more: false,
                incomplete: false,
            };
            self.settle(
                id,
                coordinate,
                NodeState::Running {
                    sequence: running_sequence,
                },
                unknown,
            )?;
            return Err(Error::Protocol(
                "Job result retention failed after execution",
            ));
        }
        self.settle(
            id,
            coordinate,
            NodeState::Running {
                sequence: running_sequence,
            },
            status,
        )?;
        Ok(status)
    }

    fn skip(&self, id: &str, coordinate: usize) -> Result<()> {
        let _gate = self
            .transition_gate
            .lock()
            .map_err(|_| Error::Protocol("Job transition gate poisoned"))?;
        let sequence = self.transition_sequence(id)?;
        self.manager
            .skip(id, coordinate, sequence, |_, _| Ok(()))
            .map_err(|_| Error::Protocol("Job conditional/cancellation skip failed"))
    }

    /// Graph-runner panic/fatal-host fallback. No node effect is
    /// re-executed: pending nodes skip, Claimed nodes are proven unlaunched,
    /// and Running nodes become unknown because their effect may have happened.
    pub fn settle_runner_abort(&self, id: &str) -> Result<()> {
        let snapshot = self
            .manager
            .runtime_snapshot(id)
            .map_err(|_| Error::Denied("Job is unavailable"))?;
        if snapshot.lifecycle == aw_tools::filesystem::job::Lifecycle::Terminal {
            return Ok(());
        }
        let _ = self.revoke(id);
        loop {
            let mut progressed = false;
            for coordinate in self
                .manager
                .skippable(id)
                .map_err(|_| Error::Protocol("Job abort skip projection failed"))?
            {
                self.skip(id, coordinate)?;
                progressed = true;
            }
            for (coordinate, state) in self
                .manager
                .pending_reattach(id)
                .map_err(|_| Error::Protocol("Job abort in-flight projection failed"))?
            {
                match state {
                    NodeState::Claimed { sequence } => {
                        self.settle_unlaunched(id, coordinate, sequence)?;
                        progressed = true;
                    }
                    NodeState::Running { sequence } => {
                        self.settle(
                            id,
                            coordinate,
                            NodeState::Running { sequence },
                            Status {
                                primary: Primary::Unknown,
                                more: false,
                                incomplete: false,
                            },
                        )?;
                        progressed = true;
                    }
                    NodeState::Pending | NodeState::Terminal { .. } => {}
                }
            }
            if self
                .manager
                .runtime_snapshot(id)
                .map_err(|_| Error::Protocol("Job disappeared during abort settlement"))?
                .lifecycle
                == aw_tools::filesystem::job::Lifecycle::Terminal
            {
                return Ok(());
            }
            if !progressed {
                return Err(Error::Protocol(
                    "native Job runner aborted without a safely settleable state",
                ));
            }
        }
    }

    /// Drive one prepared Graph to terminal state. Runnable waves are selected
    /// by the shared manager (conditions, width, global permits and conflict
    /// order); effects inside one wave run concurrently without detached tasks.
    pub async fn run_graph(
        self: &Arc<Self>,
        ctx: &ToolContext,
        id: &str,
        cancellation: CancellationToken,
    ) -> Result<aw_tools::filesystem::job_manager::Snapshot> {
        loop {
            let snapshot = self
                .manager
                .runtime_snapshot(id)
                .map_err(|_| Error::Denied("Job is unavailable"))?;
            if snapshot.lifecycle == aw_tools::filesystem::job::Lifecycle::Terminal {
                return Ok(snapshot);
            }
            if cancellation.is_cancelled()
                && snapshot.lifecycle == aw_tools::filesystem::job::Lifecycle::Running
            {
                self.cancel(id)?;
            }

            for coordinate in self
                .manager
                .skippable(id)
                .map_err(|_| Error::Protocol("Job skip projection failed"))?
            {
                self.skip(id, coordinate)?;
            }
            let snapshot = self
                .manager
                .runtime_snapshot(id)
                .map_err(|_| Error::Denied("Job is unavailable"))?;
            if snapshot.lifecycle == aw_tools::filesystem::job::Lifecycle::Terminal {
                return Ok(snapshot);
            }

            let runnable = self
                .manager
                .runnable(id)
                .map_err(|_| Error::Protocol("Job runnable projection failed"))?;
            if runnable.is_empty() {
                // Another Job may have consumed a global permit between the
                // projection and this scheduler turn. Yield rather than invent
                // a failure; a structurally dead Graph cannot pass compilation.
                tokio::task::yield_now().await;
                continue;
            }

            let mut wave = FuturesUnordered::new();
            for coordinate in runnable {
                let token = cancellation.clone();
                wave.push(async move {
                    (
                        coordinate,
                        self.execute_node(ctx, id, coordinate, token).await,
                    )
                });
            }
            let mut fatal = None;
            while let Some((coordinate, outcome)) = wave.next().await {
                if let Err(error) = outcome {
                    let state = self
                        .manager
                        .node_state(id, coordinate)
                        .map_err(|_| Error::Protocol("Job node disappeared during execution"))?;
                    if matches!(state, NodeState::Terminal { .. }) {
                        continue;
                    }
                    if cancellation.is_cancelled() {
                        continue;
                    }
                    if matches!(state, NodeState::Pending)
                        && self.node_live(&ctx.actor, id, coordinate)
                    {
                        // Transient global admission contention: retry on the
                        // next scheduler turn without duplicating an effect.
                        tokio::task::yield_now().await;
                        continue;
                    }
                    // Internal runner failure is not an ordinary Graph result.
                    // Stop admitting/effecting new work, but keep polling every
                    // already-started sibling until its process/output custody
                    // has quiesced before authoritative abort settlement.
                    fatal = Some(error);
                    cancellation.cancel();
                    let _ = self.revoke(id);
                }
            }
            if let Some(error) = fatal {
                self.settle_runner_abort(id)?;
                return Err(error);
            }
        }
    }

    pub fn cancel(&self, id: &str) -> Result<()> {
        let _gate = self
            .transition_gate
            .lock()
            .map_err(|_| Error::Protocol("Job transition gate poisoned"))?;
        let sequence = self.transition_sequence(id)?;
        self.manager
            .cancel(id, sequence, |_, _| Ok(()))
            .map_err(|_| Error::Protocol("Job cancellation intent failed"))
    }

    /// Model/SDK control path: compare public revision and reserve a fresh
    /// revision before cancellation intent can leave this live Job owner.
    pub async fn cancel_control(
        &self,
        id: &str,
        expected_revision: Option<u64>,
    ) -> Result<(bool, Snapshot)> {
        let _control = self.control_gate.lock().await;
        let before = self
            .manager
            .get(id)
            .map_err(|_| Error::Denied("Job is not exposed or missing"))?;
        if expected_revision.is_some_and(|expected| expected != before.sequence) {
            return Err(Error::Conflict("stale Job revision"));
        }
        if before.lifecycle != aw_tools::filesystem::job::Lifecycle::Running {
            return Ok((false, before));
        }
        self.manager
            .compare_and_bump_revision(id, expected_revision)
            .map_err(|_| Error::Conflict("stale Job revision"))?;
        self.cancel(id)?;
        let after = self
            .manager
            .get(id)
            .map_err(|_| Error::Protocol("Job disappeared after cancellation"))?;
        Ok((true, after))
    }

    pub fn revoke(&self, id: &str) -> Result<()> {
        let _gate = self
            .transition_gate
            .lock()
            .map_err(|_| Error::Protocol("Job transition gate poisoned"))?;
        let sequence = self.transition_sequence(id)?;
        self.manager
            .revoke(id, sequence, |_, _| Ok(()))
            .map_err(|_| Error::Protocol("Job revocation intent failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{configure, LocalToolOptions, ToolMode};
    use aw_agent_core::{
        Agent, AgentOptions, CompletionFuture, DeltaSink, Message, Provider, ToolEffect, ToolSource,
    };
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };

    struct ExistingTool;
    impl AgentTool for ExistingTool {
        fn spec(&self) -> aw_agent_core::ToolSpec {
            aw_agent_core::ToolSpec {
                name: "existing".into(),
                description: "fixture".into(),
                parameters: serde_json::json!({"type":"object","additionalProperties":false}),
                effect: ToolEffect::Read,
            }
        }
        fn validate(&self, value: &Value) -> aw_agent_core::Result<()> {
            if value.as_object().is_some_and(serde_json::Map::is_empty) {
                Ok(())
            } else {
                Err(aw_agent_core::Error::Denied("fixture args"))
            }
        }
        fn execute<'a>(
            &'a self,
            _ctx: ToolContext,
            _arguments: &'a Value,
        ) -> aw_agent_core::ToolFuture<'a> {
            Box::pin(async { Ok(ToolOutput::text("ok")) })
        }
    }

    struct FakeAgentTool {
        calls: Arc<AtomicUsize>,
        receipt: &'static str,
    }
    impl AgentTool for FakeAgentTool {
        fn spec(&self) -> aw_agent_core::ToolSpec {
            aw_agent_core::ToolSpec {
                name: "agent".into(),
                description: "fixture Agent constructor".into(),
                parameters: serde_json::json!({
                    "type":"object",
                    "properties":{"task":{"type":"string"}},
                    "required":["task"],
                    "additionalProperties":false
                }),
                effect: ToolEffect::Process,
            }
        }
        fn validate(&self, value: &Value) -> aw_agent_core::Result<()> {
            value
                .get("task")
                .and_then(Value::as_str)
                .filter(|task| !task.is_empty())
                .map(|_| ())
                .ok_or(aw_agent_core::Error::Denied("fixture Agent needs task"))
        }
        fn execute<'a>(
            &'a self,
            _ctx: ToolContext,
            _arguments: &'a Value,
        ) -> aw_agent_core::ToolFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let receipt = self.receipt;
            Box::pin(async move { Ok(ToolOutput::text(receipt)) })
        }
    }

    struct LeaseHoldingAgentTool {
        calls: Arc<AtomicUsize>,
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        owner_ref: Arc<std::sync::Mutex<Option<String>>>,
    }

    impl AgentTool for LeaseHoldingAgentTool {
        fn spec(&self) -> aw_agent_core::ToolSpec {
            aw_agent_core::ToolSpec {
                name: "agent".into(),
                description: "fixture Agent constructor with owned descendant".into(),
                parameters: serde_json::json!({
                    "type":"object",
                    "properties":{"task":{"type":"string"}},
                    "required":["task"],
                    "additionalProperties":false
                }),
                effect: ToolEffect::Process,
            }
        }

        fn validate(&self, value: &Value) -> aw_agent_core::Result<()> {
            value
                .get("task")
                .and_then(Value::as_str)
                .filter(|task| !task.is_empty())
                .map(|_| ())
                .ok_or(aw_agent_core::Error::Denied("fixture Agent needs task"))
        }

        fn execute<'a>(
            &'a self,
            ctx: ToolContext,
            _arguments: &'a Value,
        ) -> aw_agent_core::ToolFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let entered = self.entered.clone();
            let release = self.release.clone();
            let owner_ref = self.owner_ref.clone();
            Box::pin(async move {
                let scope = ctx.execution_scope.ok_or(aw_agent_core::Error::Protocol(
                    "Graph Agent node lost execution ownership scope",
                ))?;
                *owner_ref.lock().unwrap() = Some(scope.owner_ref().to_owned());
                let lease = scope.acquire()?;
                tokio::spawn(async move {
                    release.notified().await;
                    drop(lease);
                });
                entered.notify_one();
                Ok(ToolOutput::text(
                    "/d:0/i:0/p:0/a:child agent idle rev=7 turn=t:1 outcome=ok",
                ))
            })
        }
    }

    struct UnusedProvider;
    impl Provider for UnusedProvider {
        fn stream<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            _sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async {
                Err(aw_agent_core::Error::Protocol(
                    "provider must not be called by JobHost fixture",
                ))
            })
        }
    }

    struct Temp {
        root: PathBuf,
        workspace: PathBuf,
    }
    impl Temp {
        fn new() -> Self {
            let mut random = [0u8; 8];
            getrandom::fill(&mut random).unwrap();
            let root = std::env::temp_dir().join(format!(
                "aw-job-host-{}-{}",
                std::process::id(),
                random
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            ));
            let workspace = root.join("workspace");
            fs::create_dir_all(&workspace).unwrap();
            Self { root, workspace }
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn agent(root: &Path, actor: PrincipalId, mode: ToolMode) -> Agent {
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        );
        let tools = LocalToolOptions {
            root: root.to_owned(),
            mode,
            execution_site: "local".into(),
            bash: None,
            process_input: false,
            agent_jobs: Default::default(),
            operation_router: Default::default(),
            graph_policy_router: Default::default(),
            graph_host_tools: Default::default(),
            allow_agent_graph: false,
            allow_wake: false,
            private_state_dir: None,
            selected_tools: Vec::new(),
            concurrency: crate::LocalToolConcurrency::default(),
            limits: crate::LocalToolLimits::default(),
        };
        configure(builder, actor, &tools).unwrap().build().unwrap()
    }

    fn context(agent: &Agent, actor: PrincipalId, call_id: &str) -> ToolContext {
        ToolContext {
            actor,
            turn_id: 1,
            call_id: call_id.into(),
            source: ToolSource::trusted_host(),
            effect: ToolEffect::Unknown,
            execution_site: "local".into(),
            cancellation: aw_agent_core::CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: agent.default_identity().unwrap(),
            scope: Some(aw_agent_core::InvocationScope {
                session_id: "session-a".into(),
                branch_id: "main".into(),
            }),
            execution_owner: Some(
                aw_agent_core::AgentExecutionBinding::from_trusted_host(
                    "/d:0/i:0/p:0/a:0",
                    "session-a",
                    "main",
                )
                .unwrap(),
            ),
            execution_scope: None,
        }
    }

    fn host(temp: &Temp, agent: &Agent, limits: Limits) -> Arc<JobHost> {
        Arc::new(
            JobHost::new(
                Arc::new(aw_platform::scoped_fs::Root::open(&temp.workspace).unwrap()),
                agent.tool_binding_authority(),
                "session-a",
                "main",
                limits,
            )
            .unwrap(),
        )
    }

    fn host_with_graph_agent(
        temp: &Temp,
        agent: &Agent,
        graph_host_tools: Arc<crate::operation_router::GraphHostToolSlot>,
    ) -> Arc<JobHost> {
        Arc::new(
            JobHost::new_with_config(
                Arc::new(aw_platform::scoped_fs::Root::open(&temp.workspace).unwrap()),
                agent.tool_binding_authority(),
                JobHostConfig {
                    session: "session-a".into(),
                    branch: "main".into(),
                    tool_limits: crate::LocalToolLimits::default(),
                    shells: shell::ShellRegistry::default(),
                    native_tools: Arc::new(Vec::new()),
                    parallelism: Arc::new(
                        GraphParallelism::new(crate::LocalToolConcurrency::default()).unwrap(),
                    ),
                    exposure: Arc::new(
                        ExposureIndex::new(
                            crate::LocalToolLimits::default().jobs.max_retained_jobs,
                        )
                        .unwrap(),
                    ),
                    agent_jobs: Default::default(),
                    graph_policy: Default::default(),
                    graph_host_tools,
                    max_graph_nodes: aw_tools::filesystem::batch::MAX_OPS,
                },
            )
            .unwrap(),
        )
    }

    #[tokio::test]
    async fn host_qualified_agent_graph_node_uses_same_job_and_retains_factual_receipt() {
        let temp = Temp::new();
        let actor = PrincipalId::new("local").unwrap();
        let mut agent = agent(&temp.workspace, actor.clone(), ToolMode::Off);
        let calls = Arc::new(AtomicUsize::new(0));
        let tool: Arc<dyn AgentTool> = Arc::new(FakeAgentTool {
            calls: calls.clone(),
            receipt: "/d:0/i:0/p:0/a:child agent idle rev=7 turn=t:1 outcome=ok",
        });
        agent.register_host_tool(tool.clone()).unwrap();
        let graph_tools = Arc::new(crate::operation_router::GraphHostToolSlot::default());
        graph_tools
            .install(aw_tools::OperationId::Agent, tool)
            .unwrap();
        let host = host_with_graph_agent(&temp, &agent, graph_tools);
        let ctx = context(&agent, actor, "agent-graph-success");
        let prepared = host
            .prepare(
                &ctx,
                &serde_json::json!({"ops":[["agent",{"task":"child task"}]]}),
            )
            .unwrap();
        let id = host.admit(prepared).unwrap();
        let snapshot = host
            .run_graph(&ctx, &id, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(snapshot.finished, 1);
        assert_eq!(snapshot.summary.unwrap().0, Primary::Ok);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let page = host.result_page(&id, 1, 0, 4096).unwrap();
        assert!(page.writer_finished);
        assert_eq!(
            String::from_utf8(page.bytes).unwrap(),
            "/d:0/i:0/p:0/a:child agent idle rev=7 turn=t:1 outcome=ok"
        );
    }

    #[tokio::test]
    async fn agent_graph_node_waits_for_owned_descendant_lease_before_settlement() {
        let temp = Temp::new();
        let actor = PrincipalId::new("local").unwrap();
        let mut agent = agent(&temp.workspace, actor.clone(), ToolMode::Off);
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let owner_ref = Arc::new(std::sync::Mutex::new(None));
        let tool: Arc<dyn AgentTool> = Arc::new(LeaseHoldingAgentTool {
            calls: calls.clone(),
            entered: entered.clone(),
            release: release.clone(),
            owner_ref: owner_ref.clone(),
        });
        agent.register_host_tool(tool.clone()).unwrap();
        let graph_tools = Arc::new(crate::operation_router::GraphHostToolSlot::default());
        graph_tools
            .install(aw_tools::OperationId::Agent, tool)
            .unwrap();
        let host = host_with_graph_agent(&temp, &agent, graph_tools);
        let ctx = context(&agent, actor, "agent-graph-owned-descendant");
        let prepared = host
            .prepare(
                &ctx,
                &serde_json::json!({"ops":[["agent",{"task":"child task"}]]}),
            )
            .unwrap();
        let id = host.admit(prepared).unwrap();

        let run_host = host.clone();
        let run_ctx = ctx.clone();
        let run_id = id.clone();
        let run = tokio::spawn(async move {
            run_host
                .run_graph(&run_ctx, &run_id, CancellationToken::new())
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
            .await
            .unwrap();
        tokio::task::yield_now().await;
        assert!(
            !run.is_finished(),
            "Agent node must remain running while an owned descendant lease exists"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let expected = host.execution_ref(&id, Some(1)).unwrap().canonical();
        assert_eq!(
            owner_ref.lock().unwrap().as_deref(),
            Some(expected.as_str())
        );

        release.notify_one();
        let snapshot = tokio::time::timeout(std::time::Duration::from_secs(2), run)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.finished, 1);
        assert_eq!(snapshot.summary.unwrap().0, Primary::Ok);
    }

    #[tokio::test]
    async fn prepared_agent_graph_node_rejects_removed_host_generation_before_constructor() {
        let temp = Temp::new();
        let actor = PrincipalId::new("local").unwrap();
        let mut agent = agent(&temp.workspace, actor.clone(), ToolMode::Off);
        let calls = Arc::new(AtomicUsize::new(0));
        let tool: Arc<dyn AgentTool> = Arc::new(FakeAgentTool {
            calls: calls.clone(),
            receipt: "must-not-run",
        });
        agent.register_host_tool(tool.clone()).unwrap();
        let graph_tools = Arc::new(crate::operation_router::GraphHostToolSlot::default());
        graph_tools
            .install(aw_tools::OperationId::Agent, tool)
            .unwrap();
        let host = host_with_graph_agent(&temp, &agent, graph_tools.clone());
        let ctx = context(&agent, actor, "agent-graph-revoked");
        let prepared = host
            .prepare(
                &ctx,
                &serde_json::json!({"ops":[["agent",{"task":"child task"}]]}),
            )
            .unwrap();
        let id = host.admit(prepared).unwrap();
        graph_tools.remove(aw_tools::OperationId::Agent).unwrap();

        assert!(host
            .execute_node_local(&ctx, &id, 1, CancellationToken::new())
            .await
            .is_err());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "host Agent adapter revocation must fence before constructor entry"
        );
        assert!(matches!(
            host.manager().node_state(&id, 1).unwrap(),
            NodeState::Pending
        ));
    }

    #[test]
    fn contextless_job_constructor_requires_explicit_existing_agent_owner_before_admission() {
        let temp = Temp::new();
        fs::write(temp.workspace.join("a.txt"), "hello\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let agent = agent(&temp.workspace, actor.clone(), ToolMode::ReadOnly);
        let host = host(&temp, &agent, Limits::default());
        let arguments = serde_json::json!({"ops":[["read",{"path":"a.txt","numbered":false}]]});

        let mut missing = context(&agent, actor.clone(), "contextless-missing-owner");
        missing.scope = None;
        missing.execution_owner = None;
        assert!(matches!(
            host.prepare(&missing, &arguments),
            Err(Error::Denied("Job execution requires a bound Agent owner"))
        ));
        assert!(
            host.manager().list(0, 8).unwrap().jobs.is_empty(),
            "missing owner must deny before creating any finite Job"
        );
        assert!(
            host.exposure.list(None, 8).unwrap().0.is_empty(),
            "missing owner must deny before reserving any exposure position"
        );

        let mut explicit = context(&agent, actor, "contextless-explicit-owner");
        explicit.scope = None;
        let prepared = host.prepare(&explicit, &arguments).unwrap();
        let id = host.admit_exposed(prepared).unwrap();
        assert_eq!(host.manager().list(0, 8).unwrap().jobs.len(), 1);
        assert_eq!(
            host.execution_ref(&id, None)
                .unwrap()
                .agent_ref()
                .canonical(),
            "/d:0/i:0/p:0/a:0",
            "contextless success uses only the explicitly supplied existing owner"
        );
    }

    #[test]
    fn adapter_composition_preserves_existing_tool_policy_and_site_grants() {
        let temp = Temp::new();
        let actor = PrincipalId::new("local").unwrap();
        let mut policy = aw_agent_core::ToolPolicy {
            max_model_rounds: 3,
            max_parallel_reads: 1,
            allow_parallel_bash: false,
            max_parallel_bash: 2,
            ..Default::default()
        };
        policy
            .permit_host(actor.clone(), "existing", &ToolEffect::Read)
            .unwrap();
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        )
        .tool(ExistingTool)
        .tool_policy(policy.clone());
        let options = LocalToolOptions::new(&temp.workspace, ToolMode::Off);
        let agent = configure(builder, actor.clone(), &options)
            .unwrap()
            .build()
            .unwrap();
        assert!(agent
            .tool_specs()
            .iter()
            .any(|spec| spec.name == "existing"));
        assert_eq!(agent.tool_execution_site(), "local");
        assert_eq!(
            agent.tool_parallelism(),
            aw_agent_core::ToolParallelism {
                max_parallel_reads: 1,
                allow_parallel_shell: false,
                max_parallel_shell: 2,
            },
            "LocalToolOptions defaults must not widen pre-existing host scheduler ceilings"
        );

        let deliberate_site = aw_agent_core::ToolPolicy {
            execution_site: "other-site".into(),
            max_model_rounds: 3,
            ..Default::default()
        };
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        )
        .tool_policy(deliberate_site.clone());
        let preserved = configure(builder, actor.clone(), &options)
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(preserved.tool_execution_site(), "other-site");
        assert!(preserved.tool_specs().is_empty());

        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        )
        .tool_policy(deliberate_site);
        let enabled = LocalToolOptions::new(&temp.workspace, ToolMode::ReadOnly);
        assert!(
            configure(builder, actor.clone(), &enabled).is_err(),
            "enabled local tools must not retarget a deliberate non-default host site"
        );

        let mut moved = aw_agent_core::ToolPolicy {
            execution_site: "other-site".into(),
            max_model_rounds: 3,
            ..Default::default()
        };
        // Mint the grant at the changed site so the policy is internally valid;
        // composing it with LocalToolOptions(local) must still refuse a site move.
        moved
            .permit_host(actor.clone(), "existing", &ToolEffect::Read)
            .unwrap();
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        )
        .tool(ExistingTool)
        .tool_policy(moved.clone());
        let preserved = configure(builder, actor.clone(), &options)
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(preserved.tool_execution_site(), "other-site");
        assert!(preserved
            .tool_specs()
            .iter()
            .any(|spec| spec.name == "existing"));

        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        )
        .tool(ExistingTool)
        .tool_policy(moved);
        assert!(configure(builder, actor, &enabled).is_err());
    }

    #[cfg(feature = "conformance")]
    #[test]
    fn aw_direct_adapter_accepts_every_shared_canonical_request_vector() {
        let temp = Temp::new();
        let root = Arc::new(aw_platform::scoped_fs::Root::open(&temp.workspace).unwrap());
        let limits = crate::LocalToolLimits::default();
        aw_tools::conformance::validate_with_limits(limits.request_limits()).unwrap();
        for vector in aw_tools::conformance::requests() {
            let native = CoreTool::from_name(&vector.tool)
                .unwrap_or_else(|| panic!("unknown shared tool {}", vector.tool));
            assert!(
                canonical_arguments(native, &vector.arguments, limits),
                "shared parser rejected {}",
                vector.id
            );
            let adapter = canonical_tool(root.clone(), native, limits);
            assert!(
                adapter.validate(&vector.arguments).is_ok(),
                "AW adapter rejected canonical request {} ({})",
                vector.id,
                vector.tool
            );
        }
    }

    #[test]
    fn graph_rejects_top_level_parallel_scheduler_hint() {
        let temp = Temp::new();
        fs::write(temp.workspace.join("a.txt"), "hello\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let agent = agent(&temp.workspace, actor.clone(), ToolMode::ReadOnly);
        let host = host(&temp, &agent, Limits::default());
        let ctx = context(&agent, actor, "graph-parallel-hint");
        assert!(host
            .prepare(
                &ctx,
                &serde_json::json!({"ops":[["read",{
                    "path":"a.txt","numbered":false,"parallel":true
                }]]}),
            )
            .is_err());
        assert!(host
            .prepare(
                &ctx,
                &serde_json::json!({"ops":[["read",{
                    "path":"a.txt","numbered":false
                }]]}),
            )
            .is_ok());
    }

    #[test]
    fn graph_width_never_bypasses_host_read_or_shell_parallel_policy() {
        let graph = |ops: serde_json::Value| {
            Graph::compile_with(
                &serde_json::json!({"ops": ops}),
                |name| match name {
                    "read" => Some(NodeTool::Native(CoreTool::Read)),
                    "write" => Some(NodeTool::Native(CoreTool::Write)),
                    "bash" => Some(NodeTool::Bash),
                    _ => None,
                },
                |_, arguments| arguments.is_object(),
            )
            .unwrap()
        };
        let policy = GraphParallelism::new(crate::LocalToolConcurrency {
            graph_width: 8,
            parallelism: aw_agent_core::ToolParallelism {
                max_parallel_reads: 2,
                allow_parallel_shell: true,
                max_parallel_shell: 3,
            },
        })
        .unwrap();
        let reads = graph(serde_json::json!([
            ["read", {"path":"a"}, "*"],
            ["read", {"path":"b"}, "*"]
        ]));
        let shell = graph(serde_json::json!([
            ["bash", {"command":"true"}, "*"],
            ["bash", {"command":"true"}, "*"]
        ]));
        let writes = graph(serde_json::json!([
            ["write", {"path":"a","content":"a"}, "*"],
            ["write", {"path":"b","content":"b"}, "*"]
        ]));
        assert_eq!(policy.width(&reads), 2);
        assert_eq!(policy.width(&shell), 3);
        assert_eq!(policy.width(&writes), 8);

        policy.apply(aw_agent_core::ToolParallelism {
            max_parallel_reads: 1,
            allow_parallel_shell: false,
            max_parallel_shell: 3,
        });
        assert_eq!(policy.width(&reads), 1);
        assert_eq!(policy.width(&shell), 1);
        assert_eq!(policy.width(&writes), 8);
    }

    #[test]
    fn bounded_job_host_rejects_graph_above_exposed_node_ceiling() {
        let temp = Temp::new();
        let actor = PrincipalId::new("local").unwrap();
        let agent = agent(&temp.workspace, actor.clone(), ToolMode::ReadOnly);
        let host = JobHost::new_with_config(
            Arc::new(aw_platform::scoped_fs::Root::open(&temp.workspace).unwrap()),
            agent.tool_binding_authority(),
            JobHostConfig {
                session: "session-a".into(),
                branch: "main".into(),
                tool_limits: crate::LocalToolLimits::default(),
                shells: shell::ShellRegistry::default(),
                native_tools: Arc::new(vec![CoreTool::Read]),
                parallelism: Arc::new(
                    GraphParallelism::new(crate::LocalToolConcurrency::default()).unwrap(),
                ),
                exposure: Arc::new(
                    ExposureIndex::new(crate::LocalToolLimits::default().jobs.max_retained_jobs)
                        .unwrap(),
                ),
                agent_jobs: Default::default(),
                graph_policy: Default::default(),
                graph_host_tools: Default::default(),
                max_graph_nodes: 1,
            },
        )
        .unwrap();
        let ctx = context(&agent, actor, "bounded-graph");
        assert!(host
            .prepare(
                &ctx,
                &serde_json::json!({"ops":[
                    ["read",{"path":"a.txt"},"*"],
                    ["read",{"path":"b.txt"},"*"]
                ]}),
            )
            .is_err());
    }

    #[test]
    fn job_host_rejects_direct_only_native_registry_entries() {
        let temp = Temp::new();
        let actor = PrincipalId::new("local").unwrap();
        let agent = agent(&temp.workspace, actor, ToolMode::ReadOnly);
        let result = JobHost::new_with_config(
            Arc::new(aw_platform::scoped_fs::Root::open(&temp.workspace).unwrap()),
            agent.tool_binding_authority(),
            JobHostConfig {
                session: "session-a".into(),
                branch: "main".into(),
                tool_limits: crate::LocalToolLimits::default(),
                shells: shell::ShellRegistry::default(),
                native_tools: Arc::new(vec![CoreTool::Bread]),
                parallelism: Arc::new(
                    GraphParallelism::new(crate::LocalToolConcurrency::default()).unwrap(),
                ),
                exposure: Arc::new(
                    ExposureIndex::new(crate::LocalToolLimits::default().jobs.max_retained_jobs)
                        .unwrap(),
                ),
                agent_jobs: Default::default(),
                graph_policy: Default::default(),
                graph_host_tools: Default::default(),
                max_graph_nodes: aw_tools::filesystem::batch::MAX_OPS,
            },
        );
        assert!(matches!(result, Err(Error::Config(message)) if message.contains("direct-only")));
    }

    #[test]
    fn graph_validation_uses_same_narrowed_native_tool_limits_as_direct_calls() {
        let temp = Temp::new();
        let actor = PrincipalId::new("local").unwrap();
        let agent = agent(&temp.workspace, actor.clone(), ToolMode::Workspace);
        let schema = aw_tools::filesystem::schema::SchemaCaps {
            text_write_bytes: 4,
            ..crate::LocalToolLimits::default().schema
        };
        let limits = crate::LocalToolLimits {
            schema,
            result_bytes: 8192,
            ..Default::default()
        };
        let host = JobHost::new_with_config(
            Arc::new(aw_platform::scoped_fs::Root::open(&temp.workspace).unwrap()),
            agent.tool_binding_authority(),
            JobHostConfig {
                session: "session-a".into(),
                branch: "main".into(),
                tool_limits: limits.validate().unwrap(),
                shells: shell::ShellRegistry::default(),
                native_tools: Arc::new(CoreTool::GRAPH_NATIVE.to_vec()),
                parallelism: Arc::new(
                    GraphParallelism::new(crate::LocalToolConcurrency::default()).unwrap(),
                ),
                exposure: Arc::new(
                    ExposureIndex::new(crate::LocalToolLimits::default().jobs.max_retained_jobs)
                        .unwrap(),
                ),
                agent_jobs: Default::default(),
                graph_policy: Default::default(),
                graph_host_tools: Default::default(),
                max_graph_nodes: aw_tools::filesystem::batch::MAX_OPS,
            },
        )
        .unwrap();
        let ctx = context(&agent, actor, "narrow-write");
        assert!(host
            .prepare(
                &ctx,
                &serde_json::json!({"ops":[["write",{"path":"a.txt","content":"12345"}]]}),
            )
            .is_err());
        assert!(host
            .prepare(
                &ctx,
                &serde_json::json!({"ops":[["write",{"path":"a.txt","content":"1234"}]]}),
            )
            .is_ok());
        fs::write(
            temp.workspace.join("a.txt"),
            "fixture
",
        )
        .unwrap();
        let read = host
            .prepare(
                &ctx,
                &serde_json::json!({"ops":[["read",{"path":"a.txt","numbered":false}]]}),
            )
            .unwrap();
        assert_eq!(
            read.plans[0].budget().result_bytes,
            aw_tools::filesystem::job_manager::MAX_COMPACT_JOB_STATUS_BYTES
        );
    }

    #[test]
    fn direct_graph_error_classification_preserves_mixed_vs_unknown() {
        use aw_tools::filesystem::direct::Error as DirectError;
        use aw_tools::filesystem::status::Primary;
        let (_, partial) = direct_error_result(DirectError::PartialCommit(None));
        let (_, unknown) = direct_error_result(DirectError::UnknownCommit(
            aw_platform::scoped_fs::ScopedError::Denied("fixture"),
        ));
        assert_eq!(partial.primary, Primary::Mixed);
        assert_eq!(unknown.primary, Primary::Unknown);
    }

    #[test]
    fn generated_job_ids_are_opaque_and_unique() {
        let a = new_job_id().unwrap();
        let b = new_job_id().unwrap();
        assert!(a.starts_with('j') && a.len() == 33);
        assert_ne!(a, b);
    }

    #[test]
    fn exposed_replay_capacity_failure_happens_before_identity_claim() {
        let temp = Temp::new();
        fs::write(temp.workspace.join("a.txt"), "hello\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let agent = agent(&temp.workspace, actor.clone(), ToolMode::ReadOnly);
        let host = host(
            &temp,
            &agent,
            Limits {
                max_retained_jobs: 2,
                max_exposed_jobs: 1,
                max_hidden_jobs: 1,
                max_active_nodes: 2,
                max_hidden_replays: 2,
                max_spool_bytes: 1024 * 1024,
            },
        );
        let occupied_ctx = context(&agent, actor.clone(), "occupied-visible");
        let occupied = host
            .prepare(
                &occupied_ctx,
                &serde_json::json!({"ops":[["read",{"path":"a.txt","numbered":false}]]}),
            )
            .unwrap();
        let occupied_id = host.admit_exposed(occupied).unwrap();
        assert!(host.manager().get(&occupied_id).is_ok());

        let ctx = context(&agent, actor, "capacity-visible-replay");
        let prepared = host
            .prepare(
                &ctx,
                &serde_json::json!({"ops":[["read",{"path":"a.txt","numbered":false}]]}),
            )
            .unwrap();
        let semantics = prepared.semantic_hash();
        assert!(matches!(
            host.admit_replay_exposed(prepared, "session-a", &ctx.call_id),
            Err(Error::Limit("live Job capacity exhausted"))
        ));
        assert_eq!(
            host.manager()
                .lookup_replay("session-a", &ctx.call_id, semantics)
                .unwrap(),
            None,
            "visible-capacity failure must happen before replay identity"
        );
        assert_eq!(host.manager().list(0, 1).unwrap().jobs.len(), 1);
    }

    #[test]
    fn replay_capacity_failure_happens_before_identity_claim() {
        let temp = Temp::new();
        fs::write(temp.workspace.join("a.txt"), "hello\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let agent = agent(&temp.workspace, actor.clone(), ToolMode::ReadOnly);
        let host = host(
            &temp,
            &agent,
            Limits {
                max_retained_jobs: 1,
                max_exposed_jobs: 1,
                max_hidden_jobs: 1,
                max_active_nodes: 1,
                max_hidden_replays: 2,
                max_spool_bytes: 1024 * 1024,
            },
        );
        let occupied_ctx = context(&agent, actor.clone(), "occupied");
        let occupied = host
            .prepare(
                &occupied_ctx,
                &serde_json::json!({"ops":[["read",{"path":"a.txt","numbered":false}]]}),
            )
            .unwrap();
        let _occupied_id = host.admit(occupied).unwrap();

        let ctx = context(&agent, actor, "capacity-replay");
        let prepared = host
            .prepare(
                &ctx,
                &serde_json::json!({"ops":[["read",{"path":"a.txt","numbered":false}]]}),
            )
            .unwrap();
        let semantics = prepared.semantic_hash();
        assert!(matches!(
            host.admit_replay(prepared, "session-a", &ctx.call_id),
            Err(Error::Limit(
                "Job exposure recoverability capacity exhausted"
            ))
        ));
        assert_eq!(
            host.manager()
                .lookup_replay("session-a", &ctx.call_id, semantics)
                .unwrap(),
            None,
            "known pre-effect capacity failure must not consume replay identity"
        );
    }

    #[test]
    fn exposed_duplicate_promotes_existing_hidden_replay_job() {
        let temp = Temp::new();
        fs::write(temp.workspace.join("a.txt"), "hello\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let agent = agent(&temp.workspace, actor.clone(), ToolMode::ReadOnly);
        let host = host(&temp, &agent, Limits::default());
        let ctx = context(&agent, actor, "shared-replay");
        let graph = serde_json::json!({"ops":[["read",{"path":"a.txt","numbered":false}]]});
        let first = host.prepare(&ctx, &graph).unwrap();
        let id = match host.admit_replay(first, "session-a", &ctx.call_id).unwrap() {
            AdmissionDecision::New(id) => id,
            other => panic!("unexpected initial hidden replay: {other:?}"),
        };
        assert!(matches!(
            host.manager().get(&id),
            Err(aw_tools::filesystem::job_manager::Error::NotExposed)
        ));
        let duplicate = host.prepare(&ctx, &graph).unwrap();
        assert_eq!(
            host.admit_replay_exposed(duplicate, "session-a", &ctx.call_id)
                .unwrap(),
            AdmissionDecision::Existing(id.clone())
        );
        assert_eq!(host.manager().get(&id).unwrap().id, id);
    }

    #[test]
    fn duplicate_replay_resolves_existing_even_when_job_capacity_is_full() {
        let temp = Temp::new();
        fs::write(temp.workspace.join("a.txt"), "hello\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let agent = agent(&temp.workspace, actor.clone(), ToolMode::ReadOnly);
        let host = host(
            &temp,
            &agent,
            Limits {
                max_retained_jobs: 1,
                max_exposed_jobs: 1,
                max_hidden_jobs: 1,
                max_active_nodes: 1,
                max_hidden_replays: 2,
                max_spool_bytes: 1024 * 1024,
            },
        );
        let ctx = context(&agent, actor, "duplicate-replay");
        let graph = serde_json::json!({"ops":[["read",{"path":"a.txt","numbered":false}]]});
        let first = match host
            .admit_replay(
                host.prepare(&ctx, &graph).unwrap(),
                "session-a",
                &ctx.call_id,
            )
            .unwrap()
        {
            AdmissionDecision::New(id) => id,
            other => panic!("unexpected initial replay admission: {other:?}"),
        };
        let duplicate = host
            .admit_replay(
                host.prepare(&ctx, &graph).unwrap(),
                "session-a",
                &ctx.call_id,
            )
            .unwrap();
        assert_eq!(duplicate, AdmissionDecision::Existing(first.clone()));
        assert!(host.manager().runtime_snapshot(&first).is_ok());
    }

    #[tokio::test]
    async fn restart_capacity_failure_preserves_source_and_does_not_claim_replay() {
        let temp = Temp::new();
        fs::write(temp.workspace.join("a.txt"), "hello\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let agent = agent(&temp.workspace, actor.clone(), ToolMode::ReadOnly);
        let host = host(
            &temp,
            &agent,
            Limits {
                max_retained_jobs: 1,
                max_exposed_jobs: 1,
                max_hidden_jobs: 1,
                max_active_nodes: 1,
                max_hidden_replays: 2,
                max_spool_bytes: 1024 * 1024,
            },
        );
        let source_ctx = context(&agent, actor.clone(), "source-call");
        let source = host
            .prepare(
                &source_ctx,
                &serde_json::json!({"ops":[["read",{"path":"a.txt","numbered":false}]]}),
            )
            .unwrap();
        let source_id = host.admit(source).unwrap();
        host.expose(&source_id).unwrap();
        host.execute_native_node(
            &source_ctx,
            &source_id,
            1,
            aw_agent_core::CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(
            host.manager().get(&source_id).unwrap().lifecycle,
            aw_tools::filesystem::job::Lifecycle::Terminal
        );

        let restart_ctx = context(&agent, actor, "restart-capacity");
        let graph = host.manager().graph(&source_id).unwrap();
        let plans = host.plans_for_graph(&restart_ctx, &graph).unwrap();
        let semantics = semantic_hash(&graph, &plans).unwrap();
        let namespace = format!("restart:session-a:{source_id}");
        assert!(matches!(
            host.restart(&restart_ctx, &source_id),
            Err(Error::Limit("live Job capacity exhausted"))
        ));
        assert!(host.manager().get(&source_id).is_ok());
        assert_eq!(
            host.manager()
                .lookup_replay(&namespace, &restart_ctx.call_id, semantics)
                .unwrap(),
            None
        );
    }

    #[test]
    fn live_replay_returns_same_job_and_mismatch_fails_closed() {
        let temp = Temp::new();
        fs::write(temp.workspace.join("a.txt"), "hello\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let agent = agent(&temp.workspace, actor.clone(), ToolMode::ReadOnly);
        let host = host(&temp, &agent, Limits::default());
        let ctx = context(&agent, actor, "batch-call");
        let graph = serde_json::json!({"ops":[["read",{"path":"a.txt"}]]});
        let prepared = host.prepare(&ctx, &graph).unwrap();
        let semantics = prepared.semantic_hash();
        let id = match host
            .admit_replay(prepared, "session-a", &ctx.call_id)
            .unwrap()
        {
            AdmissionDecision::New(id) => id,
            other => panic!("unexpected initial admission: {other:?}"),
        };
        assert!(matches!(
            host.manager().get(&id),
            Err(aw_tools::filesystem::job_manager::Error::NotExposed)
        ));
        host.expose(&id).unwrap();
        let replayed = host
            .manager()
            .claim_replay("session-a", &ctx.call_id, semantics, "ignored", |_| {
                panic!("live replay must not call persistence callback")
            })
            .unwrap();
        assert_eq!(replayed, ReplayDecision::Existing { job_id: id });
        let mut changed = semantics;
        changed[0] ^= 1;
        assert!(host
            .manager()
            .claim_replay("session-a", &ctx.call_id, changed, "other", |_| {
                Ok(ReplayPersist::Created)
            })
            .is_err());
    }

    #[tokio::test]
    async fn restart_visible_capacity_failure_precedes_replay_claim_and_leaves_no_job() {
        let temp = Temp::new();
        fs::write(temp.workspace.join("a.txt"), "hello\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let agent = agent(&temp.workspace, actor.clone(), ToolMode::ReadOnly);
        let host = host(
            &temp,
            &agent,
            Limits {
                max_retained_jobs: 2,
                max_exposed_jobs: 1,
                max_hidden_jobs: 1,
                max_active_nodes: 2,
                max_hidden_replays: 2,
                max_spool_bytes: 1024 * 1024,
            },
        );
        let source_ctx = context(&agent, actor.clone(), "source");
        let prepared = host
            .prepare(
                &source_ctx,
                &serde_json::json!({"ops":[["read",{"path":"a.txt","numbered":false}]]}),
            )
            .unwrap();
        let source = host.admit_exposed(prepared).unwrap();
        host.run_graph(
            &source_ctx,
            &source,
            aw_agent_core::CancellationToken::new(),
        )
        .await
        .unwrap();

        let restart_ctx = context(&agent, actor, "restart-capacity");
        let graph = host.manager().graph(&source).unwrap();
        let plans = host.plans_for_graph(&restart_ctx, &graph).unwrap();
        let semantics = semantic_hash(&graph, &plans).unwrap();
        assert!(matches!(
            host.restart(&restart_ctx, &source),
            Err(Error::Limit("live Job capacity exhausted"))
        ));
        let namespace = format!("restart:session-a:{source}");
        assert_eq!(
            host.manager()
                .lookup_replay(&namespace, &restart_ctx.call_id, semantics)
                .unwrap(),
            None,
            "restart capacity failure must not consume replay identity"
        );
        assert_eq!(host.manager().list(0, 1).unwrap().jobs[0].id, source);
        assert!(!host.manager().has_nonterminal_jobs().unwrap());
    }

    #[tokio::test]
    async fn native_read_result_uses_shared_output_and_multi_owner_ack() {
        let temp = Temp::new();
        fs::write(temp.workspace.join("a.txt"), "hello\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let agent = agent(&temp.workspace, actor.clone(), ToolMode::ReadOnly);
        let host = host(&temp, &agent, Limits::default());
        let ctx = context(&agent, actor.clone(), "read-job");
        let prepared = host
            .prepare(
                &ctx,
                &serde_json::json!({"ops":[["read",{"path":"a.txt","numbered":false}]]}),
            )
            .unwrap();
        let id = match host
            .admit_replay(prepared, "session-a", &ctx.call_id)
            .unwrap()
        {
            AdmissionDecision::New(id) => id,
            other => panic!("unexpected native admission: {other:?}"),
        };
        host.expose(&id).unwrap();
        let status = host
            .execute_native_node(&ctx, &id, 1, aw_agent_core::CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(status.primary, Primary::Ok);
        assert_eq!(
            host.manager().get(&id).unwrap().summary,
            Some((Primary::Ok, false, false))
        );
        let page = host.result_page(&id, 1, 0, 64).unwrap();
        assert_eq!(page.bytes, b"hello\n");
        assert!(page.writer_finished);
        assert!(!page.truncated);
        assert_eq!(host.manager().spool_usage().unwrap().0, 6);
        let job_owner = host.job_result_owner(&id).unwrap();
        assert!(
            host.acknowledge_result(&id, 1, job_owner, 6)
                .unwrap()
                .released
        );
        assert_eq!(host.manager().spool_usage().unwrap().0, 0);
    }

    #[test]
    fn exact_transition_receipts_survive_sibling_sequence_advances() {
        let temp = Temp::new();
        fs::write(temp.workspace.join("a.txt"), "a\n").unwrap();
        fs::write(temp.workspace.join("b.txt"), "b\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let agent = agent(&temp.workspace, actor.clone(), ToolMode::ReadOnly);
        let host = host(&temp, &agent, Limits::default());
        let ctx = context(&agent, actor.clone(), "exact-transition-receipts");
        let prepared = host
            .prepare(
                &ctx,
                &serde_json::json!({"ops":[
                    ["read",{"path":"a.txt","numbered":false},"*"],
                    ["read",{"path":"b.txt","numbered":false},"*"]
                ]}),
            )
            .unwrap();
        let id = host.admit_exposed(prepared).unwrap();

        let (_, claimed_one) = host.claim_exact(&actor, &id, 1).unwrap();
        let (_, claimed_two) = host.claim_exact(&actor, &id, 2).unwrap();
        assert!(claimed_two > claimed_one);

        let running_one = host.start_exact(&actor, &id, 1, claimed_one, true).unwrap();
        let running_two = host.start_exact(&actor, &id, 2, claimed_two, true).unwrap();
        assert!(running_two > running_one);

        let ok = Status {
            primary: Primary::Ok,
            more: false,
            incomplete: false,
        };
        host.settle(
            &id,
            1,
            NodeState::Running {
                sequence: running_one,
            },
            ok,
        )
        .unwrap();
        host.settle(
            &id,
            2,
            NodeState::Running {
                sequence: running_two,
            },
            ok,
        )
        .unwrap();
        assert!(matches!(
            host.manager().runtime_snapshot(&id).unwrap().lifecycle,
            aw_tools::filesystem::job::Lifecycle::Terminal
        ));
    }

    #[tokio::test]
    async fn authority_loss_after_claim_settles_proven_unlaunched_node() {
        let temp = Temp::new();
        fs::write(temp.workspace.join("a.txt"), "hello\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let mut agent = agent(&temp.workspace, actor.clone(), ToolMode::ReadOnly);
        let host = host(&temp, &agent, Limits::default());
        let ctx = context(&agent, actor.clone(), "claim-revocation");
        let prepared = host
            .prepare(
                &ctx,
                &serde_json::json!({"ops":[["read",{"path":"a.txt","numbered":false}]]}),
            )
            .unwrap();
        let id = host.admit_exposed(prepared).unwrap();
        let plan = host.manager().plans(&id).unwrap()[0].clone();

        let (_, claimed_sequence) = host.claim_exact(&actor, &id, 1).unwrap();
        assert_eq!(host.manager().runner_load(&id).unwrap().job_active, 1);

        agent
            .revoke_registered_source_tool(&actor, "read", &ToolSource::trusted_host())
            .unwrap();

        assert!(host
            .claimed_binding(&actor, &id, 1, claimed_sequence, &plan)
            .is_err());
        assert!(matches!(
            host.manager().node_state(&id, 1).unwrap(),
            NodeState::Terminal {
                status: Status {
                    primary: Primary::Skip,
                    ..
                },
                ..
            }
        ));
        let load = host.manager().runner_load(&id).unwrap();
        assert_eq!(load.job_active, 0);
        assert_eq!(load.global_active, 0);
        assert!(!host.manager().has_nonterminal_jobs().unwrap());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn native_graph_rejects_final_symlink_retarget_before_effect() {
        use std::os::unix::fs::symlink;

        let temp = Temp::new();
        let first = temp.workspace.join("first.txt");
        let second = temp.workspace.join("second.txt");
        let alias = temp.workspace.join("alias.txt");
        fs::write(&first, "first").unwrap();
        fs::write(&second, "second").unwrap();
        symlink("first.txt", &alias).unwrap();

        let actor = PrincipalId::new("local").unwrap();
        let agent = agent(&temp.workspace, actor.clone(), ToolMode::Workspace);
        let host = host(&temp, &agent, Limits::default());
        let ctx = context(&agent, actor, "retarget-before-effect");
        let prepared = host
            .prepare(
                &ctx,
                &serde_json::json!({"ops":[["write",{
                    "path":"alias.txt","content":"changed"
                }]]}),
            )
            .unwrap();
        let id = host.admit(prepared).unwrap();
        host.expose(&id).unwrap();

        fs::remove_file(&alias).unwrap();
        symlink("second.txt", &alias).unwrap();

        assert!(host
            .execute_native_node(&ctx, &id, 1, aw_agent_core::CancellationToken::new(),)
            .await
            .is_err());
        assert_eq!(fs::read_to_string(&first).unwrap(), "first");
        assert_eq!(fs::read_to_string(&second).unwrap(), "second");
        assert!(matches!(
            host.manager().node_state(&id, 1).unwrap(),
            NodeState::Terminal {
                status: Status {
                    primary: Primary::Skip,
                    ..
                },
                ..
            }
        ));
    }

    #[tokio::test]
    async fn result_capacity_failure_is_proven_unlaunched_before_write_effect() {
        let temp = Temp::new();
        fs::write(temp.workspace.join("a.txt"), "old").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let agent = agent(&temp.workspace, actor.clone(), ToolMode::Workspace);
        let host = host(
            &temp,
            &agent,
            Limits {
                max_retained_jobs: 4,
                max_exposed_jobs: 4,
                max_hidden_jobs: 4,
                max_active_nodes: 2,
                max_hidden_replays: 4,
                max_spool_bytes: 4,
            },
        );
        let ctx = context(&agent, actor.clone(), "write-job");
        let prepared = host
            .prepare(
                &ctx,
                &serde_json::json!({"ops":[["write",{"path":"a.txt","content":"new"}]]}),
            )
            .unwrap();
        let id = host.admit(prepared).unwrap();
        host.expose(&id).unwrap();
        assert!(host
            .execute_native_node(&ctx, &id, 1, aw_agent_core::CancellationToken::new(),)
            .await
            .is_err());
        assert_eq!(
            fs::read_to_string(temp.workspace.join("a.txt")).unwrap(),
            "old"
        );
        assert_eq!(
            host.manager().get(&id).unwrap().summary,
            Some((Primary::Skip, false, false))
        );
        assert_eq!(host.manager().spool_usage().unwrap().0, 0);
    }

    #[tokio::test]
    async fn graph_scheduler_settles_dependency_before_successor_and_skips_false_branch() {
        let temp = Temp::new();
        fs::write(temp.workspace.join("a.txt"), "a\n").unwrap();
        fs::write(temp.workspace.join("b.txt"), "b\n").unwrap();
        fs::write(temp.workspace.join("c.txt"), "c\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let agent = agent(&temp.workspace, actor.clone(), ToolMode::ReadOnly);
        let host = host(&temp, &agent, Limits::default());
        let ctx = context(&agent, actor.clone(), "graph-conditions");
        let prepared = host
            .prepare(
                &ctx,
                &serde_json::json!({"ops":[
                    ["read",{"path":"a.txt","numbered":false}],
                    ["read",{"path":"b.txt","numbered":false},"1.ok"],
                    ["read",{"path":"c.txt","numbered":false},"1.fail"]
                ]}),
            )
            .unwrap();
        let id = host.admit(prepared).unwrap();
        host.expose(&id).unwrap();
        let terminal = host
            .run_graph(&ctx, &id, aw_agent_core::CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(terminal.finished, 3);
        assert_eq!(terminal.total, 3);
        assert_eq!(terminal.summary, Some((Primary::Ok, false, false)));
        let first = host.manager().node_state(&id, 1).unwrap();
        let second = host.manager().node_state(&id, 2).unwrap();
        let third = host.manager().node_state(&id, 3).unwrap();
        let (first_sequence, second_sequence) = match (first, second) {
            (
                NodeState::Terminal {
                    sequence: first,
                    status: first_status,
                },
                NodeState::Terminal {
                    sequence: second,
                    status: second_status,
                },
            ) => {
                assert_eq!(first_status.primary, Primary::Ok);
                assert_eq!(second_status.primary, Primary::Ok);
                (first, second)
            }
            other => panic!("unexpected dependency states: {other:?}"),
        };
        assert!(
            second_sequence > first_sequence,
            "dependent node must checkpoint terminal only after predecessor"
        );
        assert!(matches!(
            third,
            NodeState::Terminal {
                status: Status {
                    primary: Primary::Skip,
                    ..
                },
                ..
            }
        ));
        assert_eq!(host.result_page(&id, 1, 0, 8).unwrap().bytes, b"a\n");
        assert_eq!(host.result_page(&id, 2, 0, 8).unwrap().bytes, b"b\n");
    }

    #[tokio::test]
    async fn graph_scheduler_cancellation_freezes_all_pending_nodes_before_effect() {
        let temp = Temp::new();
        fs::write(temp.workspace.join("a.txt"), "old-a").unwrap();
        fs::write(temp.workspace.join("b.txt"), "old-b").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let agent = agent(&temp.workspace, actor.clone(), ToolMode::Workspace);
        let host = host(&temp, &agent, Limits::default());
        let ctx = context(&agent, actor.clone(), "graph-cancel");
        let prepared = host
            .prepare(
                &ctx,
                &serde_json::json!({"ops":[
                    ["write",{"path":"a.txt","content":"new-a"},"*"],
                    ["write",{"path":"b.txt","content":"new-b"},"*"]
                ]}),
            )
            .unwrap();
        let id = host.admit(prepared).unwrap();
        host.expose(&id).unwrap();
        let token = aw_agent_core::CancellationToken::new();
        token.cancel();
        let terminal = host.run_graph(&ctx, &id, token).await.unwrap();
        assert_eq!(terminal.finished, 2);
        assert_eq!(terminal.summary, Some((Primary::Skip, false, false)));
        assert_eq!(
            fs::read_to_string(temp.workspace.join("a.txt")).unwrap(),
            "old-a"
        );
        assert_eq!(
            fs::read_to_string(temp.workspace.join("b.txt")).unwrap(),
            "old-b"
        );
        assert_eq!(host.manager().spool_usage().unwrap().0, 0);
    }
}
