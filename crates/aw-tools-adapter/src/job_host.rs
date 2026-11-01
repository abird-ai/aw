//! AW binding for the shared canonical live-session JobManager.
//! The existing outer AW ToolStarted journal intent authorizes the Graph before
//! this host runs. Node lifecycle/results stay in the shared live manager; AW
//! creates no second workflow database and never resurrects Jobs after restart.
use crate::{binary, edit, lifecycle, patch, process, search, shell, text, Error, Result};
use aw_agent_core::tool::{ToolBindingAuthority, ToolBindingSnapshot};
use aw_agent_core::{CancellationToken, PrincipalId, Tool as AgentTool, ToolContext, ToolOutput};
use aw_tools::{
    filesystem::{
        batch::{Graph, NodeTool, Status},
        footprint::{Access, Extent, Footprint, Scope},
        job::NodeState,
        job_manager::{
            plan_graph, semantic_hash, JobManager, Limits, NodePlan, NodePolicy, OutputAck,
            OutputOwner, OutputPage, OutputStream, ProcessSettlement, ReplayDecision,
            ReplayPersist, SemanticBudget, StartRequest, TaskAdmission,
        },
        schema::Tool as CoreTool,
        status::Primary,
        Kind,
    },
    task_fence::TaskIdentity,
    ToolEffect as CoreEffect,
};
use futures_util::{stream::FuturesUnordered, StreamExt};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};

const NODE_WORK_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const SHELL_RESULT_BYTES: usize = 4 * 1024;

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
        let has_shell = graph
            .steps()
            .iter()
            .any(|step| step.tool.native().is_none());
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdmissionDecision {
    New(String),
    Existing(String),
    Unknown,
}

pub struct JobHost {
    manager: Arc<JobManager>,
    authority: ToolBindingAuthority,
    root: Arc<aw_platform::scoped_fs::Root>,
    session: String,
    branch: String,
    root_binding: [u8; 32],
    transition_gate: Mutex<()>,
    shells: shell::ShellRegistry,
    native_tools: Arc<Vec<CoreTool>>,
    parallelism: Arc<GraphParallelism>,
    tool_limits: crate::LocalToolLimits,
}

fn core_effect(effect: &aw_agent_core::ToolEffect) -> CoreEffect {
    match effect {
        aw_agent_core::ToolEffect::Read => CoreEffect::Read,
        aw_agent_core::ToolEffect::Write => CoreEffect::Write,
        aw_agent_core::ToolEffect::Process => CoreEffect::Process,
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
        aw_agent_core::ToolEffect::Unknown => 3,
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

fn canonical_arguments(tool: &dyn AgentTool, arguments: &Value) -> bool {
    if tool.validate(arguments).is_err() {
        return false;
    }
    let spec = tool.spec();
    let Some(properties) = spec.parameters.get("properties").and_then(Value::as_object) else {
        return false;
    };
    arguments
        .as_object()
        .is_some_and(|object| object.keys().all(|key| properties.contains_key(key)))
}

fn arg_path(arguments: &Value, key: &str) -> Result<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(Error::Denied("canonical Job node path is missing"))
}

fn search_path(arguments: &Value) -> String {
    arguments
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or(".")
        .to_owned()
}

fn model_scope(path: &str) -> String {
    if path.is_empty() {
        ".".into()
    } else {
        path.into()
    }
}

fn mutation_tool(tool: CoreTool) -> bool {
    matches!(
        tool,
        CoreTool::Write
            | CoreTool::Edit
            | CoreTool::Patch
            | CoreTool::Copy
            | CoreTool::Move
            | CoreTool::Remove
            | CoreTool::Bwrite
            | CoreTool::Bedit
    )
}

fn receipt_primary(content: &str) -> Option<Primary> {
    match content.split_whitespace().next()? {
        "ok" => Some(Primary::Ok),
        "unchanged" => Some(Primary::Unchanged),
        "fail" => Some(Primary::Fail),
        "mixed" => Some(Primary::Mixed),
        "unknown" => Some(Primary::Unknown),
        "skip" => Some(Primary::Skip),
        _ => None,
    }
}

fn canonical_tail_status(tool: CoreTool, output: &ToolOutput) -> Option<String> {
    if !output.truncated {
        return None;
    }
    if tool == CoreTool::Read {
        if let Ok(value) = serde_json::from_str::<Value>(&output.content) {
            if let Some(status) = value.get("status").and_then(Value::as_str) {
                return Some(status.to_owned());
            }
        }
    }
    output.content.lines().last().and_then(|line| {
        let line = line.trim();
        (line.starts_with("more ")
            || line.starts_with("incomplete ")
            || line.starts_with("mixed ")
            || line.starts_with("… incomplete:"))
        .then(|| line.to_owned())
    })
}

fn status_from_output(tool: CoreTool, output: &ToolOutput) -> Status {
    let status_line = canonical_tail_status(tool, output);
    let mut primary = if mutation_tool(tool) {
        receipt_primary(&output.content).unwrap_or(if output.is_error {
            Primary::Fail
        } else {
            Primary::Ok
        })
    } else if output.is_error {
        status_line
            .as_deref()
            .and_then(receipt_primary)
            .unwrap_or(Primary::Fail)
    } else {
        Primary::Ok
    };
    if output.is_error && primary == Primary::Ok {
        primary = Primary::Fail;
    }
    let status = status_line.as_deref().unwrap_or_default();
    let more = status == "more" || status.starts_with("more ") || status.contains(" more ");
    let incomplete = status.starts_with("incomplete ")
        || status.contains("incomplete=")
        || status.contains("incomplete:");
    Status {
        primary,
        more,
        incomplete,
    }
}

fn error_result(tool: CoreTool, error: Error) -> (ToolOutput, Status) {
    let detail = aw_tools::task::bounded_task_error(error.to_string());
    let ambiguous = mutation_tool(tool)
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

pub(crate) struct JobHostConfig {
    pub session: String,
    pub branch: String,
    pub tool_limits: crate::LocalToolLimits,
    pub shells: shell::ShellRegistry,
    pub native_tools: Arc<Vec<CoreTool>>,
    pub parallelism: Arc<GraphParallelism>,
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
        } = config;
        for value in [&session, &branch] {
            if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                return Err(Error::Config("invalid Job Session/branch identity".into()));
            }
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
            authority,
            root,
            session,
            branch,
            root_binding,
            transition_gate: Mutex::new(()),
            shells,
            native_tools,
            parallelism,
            tool_limits,
        })
    }

    pub fn manager(&self) -> &Arc<JobManager> {
        &self.manager
    }

    fn task_identity(&self, ctx: &ToolContext) -> Result<TaskIdentity> {
        let (session, branch) = if let Some(scope) = ctx.scope.as_ref() {
            if scope.session_id != self.session || scope.branch_id != self.branch {
                return Err(Error::Denied(
                    "Job invocation scope differs from bound Session",
                ));
            }
            (scope.session_id.clone(), scope.branch_id.clone())
        } else {
            // Explicit memory-only Agent execution has no durable journal scope.
            // The JobRuntime supplies one host-minted live-process namespace;
            // model arguments never choose these values.
            (self.session.clone(), self.branch.clone())
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

    fn footprint(&self, tool: CoreTool, arguments: &Value) -> Result<Footprint> {
        let exact = |path: String, access| {
            Footprint::new(
                vec![Scope {
                    path: model_scope(&path),
                    access,
                    extent: Extent::Exact,
                }],
                false,
                false,
            )
            .map_err(|_| Error::Denied("invalid Job footprint"))
        };
        match tool {
            CoreTool::Read | CoreTool::Bread => {
                let path = crate::content_path(&self.root, &arg_path(arguments, "path")?, false)?;
                exact(path, Access::Read)
            }
            CoreTool::Write | CoreTool::Bwrite => {
                let path = crate::content_path(&self.root, &arg_path(arguments, "path")?, true)?;
                exact(path, Access::Write)
            }
            CoreTool::Edit | CoreTool::Bedit => {
                let path = crate::content_path(&self.root, &arg_path(arguments, "path")?, false)?;
                exact(path, Access::Write)
            }
            CoreTool::Ls | CoreTool::Find => {
                let lexical = search_path(arguments);
                let backend = if lexical == "." {
                    String::new()
                } else {
                    crate::directory_path(&self.root, &lexical)?
                };
                Footprint::new(
                    vec![Scope {
                        path: model_scope(&backend),
                        access: Access::Read,
                        extent: Extent::Subtree,
                    }],
                    false,
                    false,
                )
                .map_err(|_| Error::Denied("invalid Job footprint"))
            }
            CoreTool::Grep => {
                let lexical = search_path(arguments);
                if lexical == "." {
                    return Footprint::new(
                        vec![Scope {
                            path: ".".into(),
                            access: Access::Read,
                            extent: Extent::Subtree,
                        }],
                        false,
                        false,
                    )
                    .map_err(|_| Error::Denied("invalid Job footprint"));
                }
                let backend = match crate::platform_fs(self.root.entry_meta(&lexical))?.kind {
                    Kind::File => (
                        crate::content_path(&self.root, &lexical, false)?,
                        Extent::Exact,
                    ),
                    Kind::Dir => (
                        crate::directory_path(&self.root, &lexical)?,
                        Extent::Subtree,
                    ),
                    Kind::Symlink => crate::directory_path(&self.root, &lexical)
                        .map(|path| (path, Extent::Subtree))
                        .or_else(|_| {
                            crate::content_path(&self.root, &lexical, false)
                                .map(|path| (path, Extent::Exact))
                        })?,
                    _ => return Err(Error::Denied("grep Job path is not text-searchable")),
                };
                Footprint::new(
                    vec![Scope {
                        path: model_scope(&backend.0),
                        access: Access::Read,
                        extent: backend.1,
                    }],
                    false,
                    false,
                )
                .map_err(|_| Error::Denied("invalid Job footprint"))
            }
            CoreTool::Copy | CoreTool::Move => {
                let from = arg_path(arguments, "from")?;
                let to = arg_path(arguments, "to")?;
                // Lifecycle node planning must not require an extra metadata
                // read merely to improve scheduler precision. A subtree scope
                // is conservative for files and exact for directory trees.
                let mut scopes = vec![Scope {
                    path: model_scope(&from),
                    access: if tool == CoreTool::Copy {
                        Access::Read
                    } else {
                        Access::Write
                    },
                    extent: Extent::Subtree,
                }];
                scopes.push(Scope {
                    path: model_scope(&to),
                    access: Access::Write,
                    extent: Extent::Subtree,
                });
                Footprint::new(scopes, false, false)
                    .map_err(|_| Error::Denied("invalid Job footprint"))
            }
            CoreTool::Remove => {
                let paths = arguments
                    .get("paths")
                    .and_then(Value::as_array)
                    .ok_or(Error::Denied("rm Job paths are missing"))?;
                let mut scopes = Vec::with_capacity(paths.len());
                for path in paths {
                    let path = path
                        .as_str()
                        .ok_or(Error::Denied("rm Job path is invalid"))?;
                    scopes.push(Scope {
                        path: model_scope(path),
                        access: Access::Write,
                        extent: Extent::Subtree,
                    });
                }
                Footprint::new(scopes, false, false)
                    .map_err(|_| Error::Denied("invalid Job footprint"))
            }
            CoreTool::Patch => Ok(Footprint::unknown(true, true)),
        }
    }

    fn resolve_node(&self, name: &str) -> Option<NodeTool> {
        if let Some(native) = self
            .native_tools
            .iter()
            .copied()
            .find(|tool| tool.name() == name)
        {
            return Some(NodeTool::Native(native));
        }
        self.shells.resolve(name)
    }

    fn validate_node(&self, node: NodeTool, arguments: &Value) -> bool {
        if let Some(native) = node.native() {
            let adapter = canonical_tool(self.root.clone(), native, self.tool_limits);
            return canonical_arguments(adapter.as_ref(), arguments);
        }
        self.shells
            .get(node)
            .is_some_and(|shell| shell.validate(arguments).is_ok())
    }

    fn plans_for_graph(&self, ctx: &ToolContext, graph: &Graph) -> Result<Vec<NodePlan>> {
        let one = graph.steps().len() == 1;
        plan_graph(graph, |_, step| {
            let binding = self
                .authority
                .permitted_binding(&ctx.actor, step.tool.name())
                .map_err(|_| ())?;
            let (footprint, runtime_binding, task_result_eligible, result_bytes) =
                if let Some(native) = step.tool.native() {
                    let adapter = canonical_tool(self.root.clone(), native, self.tool_limits);
                    if !canonical_arguments(adapter.as_ref(), &step.arguments) {
                        return Err(());
                    }
                    (
                        self.footprint(native, &step.arguments).map_err(|_| ())?,
                        self.root_binding,
                        one,
                        self.tool_limits.result_bytes,
                    )
                } else {
                    let shell = self.shells.get(step.tool).ok_or(())?;
                    let prepared = shell.prepare(&step.arguments).map_err(|_| ())?;
                    (
                        shell.footprint(&prepared),
                        shell.runtime_binding().map_err(|_| ())?,
                        false,
                        SHELL_RESULT_BYTES,
                    )
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

    pub fn validate_graph(&self, graph_input: &Value) -> Result<()> {
        Graph::compile_with(
            graph_input,
            |name| self.resolve_node(name),
            |node, arguments| self.validate_node(node, arguments),
        )
        .map(|_| ())
        .map_err(|_| Error::Denied("invalid canonical Job graph"))
    }

    pub fn prepare(&self, ctx: &ToolContext, graph_input: &Value) -> Result<PreparedJob> {
        let task = self.task_identity(ctx)?;
        let graph = Graph::compile_with(
            graph_input,
            |name| self.resolve_node(name),
            |node, arguments| self.validate_node(node, arguments),
        )
        .map_err(|_| Error::Denied("invalid canonical Job graph"))?;
        let plans = self.plans_for_graph(ctx, &graph)?;
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
            .map_err(|_| Error::Protocol("live Job admission failed"))
    }

    pub fn admit(&self, prepared: PreparedJob) -> Result<String> {
        let id = new_job_id()?;
        self.admit_with_id(&id, prepared)?;
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
        let proposed = new_job_id()?;
        let semantics = prepared.semantic_hash;
        let decision = self
            .manager
            .claim_replay(namespace, request_id, semantics, &proposed, |_| {
                Ok(ReplayPersist::Created)
            })
            .map_err(|_| Error::Denied("Job replay claim refused"))?;
        match decision {
            ReplayDecision::Existing { job_id } => {
                if self.manager.runtime_snapshot(&job_id).is_ok() {
                    Ok(AdmissionDecision::Existing(job_id))
                } else {
                    Ok(AdmissionDecision::Unknown)
                }
            }
            ReplayDecision::Unknown => Ok(AdmissionDecision::Unknown),
            ReplayDecision::New { key, job_id } => {
                if job_id != proposed {
                    return Err(Error::Protocol(
                        "replay claim returned a different proposed Job",
                    ));
                }
                if self.admit_with_id(&job_id, prepared).is_err() {
                    // The live replay entry intentionally remains Claimed. A
                    // duplicate becomes Unknown rather than re-running work.
                    return Err(Error::Protocol("Job admission failed after replay claim"));
                }
                self.manager
                    .mark_replay_ready(key, &job_id, |_| Ok(()))
                    .map_err(|_| Error::Protocol("Job replay ready transition failed"))?;
                Ok(AdmissionDecision::New(job_id))
            }
        }
    }

    pub fn expose(&self, id: &str) -> Result<()> {
        self.manager
            .expose(id)
            .map_err(|_| Error::Protocol("Job exposure failed"))
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
        let plans = self.plans_for_graph(ctx, &graph)?;
        let task = self.task_identity(ctx)?;
        let semantics = semantic_hash(&graph, &plans)
            .map_err(|_| Error::Denied("restart semantic identity failed"))?;
        let width = self.parallelism.width(&graph);
        let proposed = new_job_id()?;
        let namespace = format!("restart:{}:{source_id}", self.session);
        let decision = self
            .manager
            .claim_replay(&namespace, &ctx.call_id, semantics, &proposed, |_| {
                Ok(ReplayPersist::Created)
            })
            .map_err(|_| Error::Denied("Job restart replay claim refused"))?;
        match decision {
            ReplayDecision::Existing { job_id } => {
                if self.manager.runtime_snapshot(&job_id).is_ok() {
                    Ok(AdmissionDecision::Existing(job_id))
                } else {
                    Ok(AdmissionDecision::Unknown)
                }
            }
            ReplayDecision::Unknown => Ok(AdmissionDecision::Unknown),
            ReplayDecision::New { key, job_id } => {
                if job_id != proposed {
                    return Err(Error::Protocol(
                        "restart replay claim changed proposed Job ID",
                    ));
                }
                if self
                    .manager
                    .restart_task(
                        source_id,
                        graph,
                        plans,
                        TaskAdmission {
                            id: &job_id,
                            task: &task,
                            admission_sequence: 1,
                            width,
                            exposed: true,
                        },
                    )
                    .is_err()
                {
                    return Err(Error::Protocol(
                        "restart admission failed after replay claim",
                    ));
                }
                self.manager
                    .mark_replay_ready(key, &job_id, |_| Ok(()))
                    .map_err(|_| Error::Protocol("restart replay ready transition failed"))?;
                Ok(AdmissionDecision::New(job_id))
            }
        }
    }

    fn node_live(&self, actor: &PrincipalId, id: &str, coordinate: usize) -> bool {
        let Ok(plans) = self.manager.plans(id) else {
            return false;
        };
        let Some(plan) = plans.get(coordinate.saturating_sub(1)) else {
            return false;
        };
        let Ok(binding) = self.authority.permitted_binding(actor, plan.tool().name()) else {
            return false;
        };
        let runtime_live = if plan.tool().native().is_some() {
            plan.runtime_binding() == self.root_binding
        } else {
            self.shells.get(plan.tool()).is_some_and(|shell| {
                shell
                    .runtime_binding()
                    .is_ok_and(|binding| binding == plan.runtime_binding())
                    && shell.revalidate().is_ok()
            })
        };
        binding.registry_revision == plan.registry_generation()
            && binding_hash(actor, &binding) == plan.authority_binding()
            && runtime_live
    }

    /// Payload authority is stricter than status/control authority: every
    /// retained node plan must still match the current host-minted authority
    /// generation. Revocation/regrant never reopens old result bytes.
    pub fn payload_authorized(&self, actor: &PrincipalId, id: &str) -> bool {
        let Ok(plans) = self.manager.plans(id) else {
            return false;
        };
        plans.iter().all(|plan| {
            self.authority
                .permitted_binding(actor, plan.tool().name())
                .is_ok_and(|binding| {
                    let runtime_live = if plan.tool().native().is_some() {
                        plan.runtime_binding() == self.root_binding
                    } else {
                        self.shells.get(plan.tool()).is_some_and(|shell| {
                            shell
                                .runtime_binding()
                                .is_ok_and(|runtime| runtime == plan.runtime_binding())
                                && shell.revalidate().is_ok()
                        })
                    };
                    binding.registry_revision == plan.registry_generation()
                        && binding_hash(actor, &binding) == plan.authority_binding()
                        && runtime_live
                })
        })
    }

    fn transition_sequence(&self, id: &str) -> Result<u64> {
        self.manager
            .current_sequence(id)
            .map_err(|_| Error::Denied("Job is missing or unavailable"))?
            .checked_add(1)
            .ok_or(Error::Limit("Job transition sequence exhausted"))
    }

    pub fn claim(&self, actor: &PrincipalId, id: &str, coordinate: usize) -> Result<[u8; 32]> {
        let _gate = self
            .transition_gate
            .lock()
            .map_err(|_| Error::Protocol("Job transition gate poisoned"))?;
        let sequence = self.transition_sequence(id)?;
        let live = self.node_live(actor, id, coordinate);
        self.manager
            .claim(id, coordinate, live, sequence, |_, _| Ok(()))
            .map_err(|_| Error::Denied("Job node claim refused"))
    }

    pub fn start(
        &self,
        actor: &PrincipalId,
        id: &str,
        coordinate: usize,
        claimed_sequence: u64,
        launch_authorized: bool,
    ) -> Result<()> {
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
            .map_err(|_| Error::Denied("Job node start refused"))
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
        let _node_key = self.claim(&ctx.actor, id, coordinate)?;
        let claimed_sequence = self
            .manager
            .current_sequence(id)
            .map_err(|_| Error::Protocol("claimed ShellNode sequence unavailable"))?;
        if cancellation.is_cancelled() {
            self.settle_unlaunched(id, coordinate, claimed_sequence)?;
            return Err(Error::Cancelled);
        }
        let binding = self
            .authority
            .permitted_binding(&ctx.actor, plan.tool().name())
            .map_err(|_| Error::Denied("ShellNode binding was revoked before execution"))?;
        if binding.registry_revision != plan.registry_generation()
            || binding_hash(&ctx.actor, &binding) != plan.authority_binding()
            || shell.runtime_binding()? != plan.runtime_binding()
        {
            self.settle_unlaunched(id, coordinate, claimed_sequence)?;
            return Err(Error::Denied("ShellNode binding changed before execution"));
        }
        shell.revalidate()?;
        let stream_capacity = match shell.output_capacity(plan.arguments()) {
            Ok(capacity) => capacity as u64,
            Err(error) => {
                self.settle_unlaunched(id, coordinate, claimed_sequence)?;
                return Err(error);
            }
        };
        let owner = self
            .manager
            .job_output_owner(id)
            .map_err(|_| Error::Protocol("ShellNode output owner unavailable"))?;
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
        if let Err(error) = self.start(&ctx.actor, id, coordinate, claimed_sequence, true) {
            let _ = stdout.finish();
            let _ = stderr.finish();
            self.settle_unlaunched(id, coordinate, claimed_sequence)?;
            return Err(error);
        }
        let running_sequence = self
            .manager
            .current_sequence(id)
            .map_err(|_| Error::Protocol("running ShellNode sequence unavailable"))?;

        let prepared = match shell.prepare(plan.arguments()) {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = stdout.finish();
                let _ = stderr.finish();
                let detail = aw_tools::task::bounded_task_error(error.to_string());
                return self.settle_shell_result(
                    id,
                    coordinate,
                    running_sequence,
                    format!("fail prepare={detail}"),
                    None,
                    Status {
                        primary: Primary::Fail,
                        more: false,
                        incomplete: false,
                    },
                );
            }
        };
        if cancellation.is_cancelled() {
            let _ = stdout.finish();
            let _ = stderr.finish();
            return self.settle_shell_result(
                id,
                coordinate,
                running_sequence,
                "fail cancelled_before_launch".into(),
                None,
                Status {
                    primary: Primary::Fail,
                    more: false,
                    incomplete: false,
                },
            );
        }
        if self.authority.revalidate(&ctx.actor, &binding).is_err()
            || shell.revalidate().is_err()
            || shell.runtime_binding()? != plan.runtime_binding()
        {
            let _ = self.revoke(id);
            let _ = stdout.finish();
            let _ = stderr.finish();
            return self.settle_shell_result(
                id,
                coordinate,
                running_sequence,
                "fail revoked_before_launch".into(),
                None,
                Status {
                    primary: Primary::Fail,
                    more: false,
                    incomplete: false,
                },
            );
        }

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
        let receipt = process.wait(stop_signal, prepared.timeout).await;
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

    async fn execute_node(
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
        } else {
            self.execute_native_node(ctx, id, coordinate, cancellation)
                .await
        }
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
        let _node_key = self.claim(&ctx.actor, id, coordinate)?;
        let claimed_sequence = self
            .manager
            .current_sequence(id)
            .map_err(|_| Error::Protocol("claimed Job sequence unavailable"))?;
        if cancellation.is_cancelled() {
            self.settle_unlaunched(id, coordinate, claimed_sequence)?;
            return Err(Error::Cancelled);
        }
        let binding = self
            .authority
            .permitted_binding(&ctx.actor, plan.tool().name())
            .map_err(|_| Error::Denied("Job tool binding was revoked before execution"))?;
        if binding.registry_revision != plan.registry_generation()
            || binding_hash(&ctx.actor, &binding) != plan.authority_binding()
            || plan.runtime_binding() != self.root_binding
        {
            self.settle_unlaunched(id, coordinate, claimed_sequence)?;
            return Err(Error::Denied("Job tool binding changed before execution"));
        }
        if let Err(error) = self.attach_exact_result(id, coordinate, plan.budget().result_bytes) {
            self.settle_unlaunched(id, coordinate, claimed_sequence)?;
            return Err(error);
        }
        // Construct every fallible execution input while the node is still only
        // Claimed. Once Start commits, this runner must either enter the native
        // tool or settle the node; no adapter lookup may strand Running state.
        let task = match self.manager.task_binding(id) {
            Ok(task) => task,
            Err(_) => {
                self.settle_unlaunched(id, coordinate, claimed_sequence)?;
                return Err(Error::Protocol("Job Task binding unavailable"));
            }
        };
        let identity = ctx.identity.clone();
        let nested = ToolContext {
            actor: ctx.actor.clone(),
            turn_id: task.turn_id,
            call_id: format!("job:{id}:{coordinate}"),
            source: binding.source.clone(),
            effect: binding.effect.clone(),
            execution_site: binding.execution_site.clone(),
            cancellation: cancellation.clone(),
            result_budget_bytes: plan.budget().result_bytes,
            identity,
            scope: Some(aw_agent_core::InvocationScope {
                session_id: self.session.clone(),
                branch_id: self.branch.clone(),
            }),
        };
        let tool = canonical_tool(self.root.clone(), native, self.tool_limits);
        if let Err(error) = self.start(&ctx.actor, id, coordinate, claimed_sequence, true) {
            self.settle_unlaunched(id, coordinate, claimed_sequence)?;
            return Err(error);
        }
        let running_sequence = self
            .manager
            .current_sequence(id)
            .map_err(|_| Error::Protocol("running Job sequence unavailable"))?;
        let executed = tool.execute(nested, plan.arguments()).await;
        let (output, status) = match executed {
            Ok(output) => {
                let status = status_from_output(native, &output);
                (output, status)
            }
            Err(error) => error_result(native, error),
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
        &self,
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
                    // Authority disappeared or a host invariant failed before
                    // this node reached a terminal checkpoint. Freeze all new
                    // work and preserve already-terminal sibling outcomes.
                    self.revoke(id)?;
                    let _ = error;
                }
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
        sync::Arc,
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
        }
    }

    fn host(temp: &Temp, agent: &Agent, limits: Limits) -> JobHost {
        JobHost::new(
            Arc::new(aw_platform::scoped_fs::Root::open(&temp.workspace).unwrap()),
            agent.tool_binding_authority(),
            "session-a",
            "main",
            limits,
        )
        .unwrap()
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
        .tool_policy(deliberate_site);
        assert!(
            configure(builder, actor.clone(), &options).is_err(),
            "adapter must not retarget a deliberate non-default host site even before grants"
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
        .tool_policy(moved);
        assert!(configure(builder, actor, &options).is_err());
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
    fn generated_job_ids_are_opaque_and_unique() {
        let a = new_job_id().unwrap();
        let b = new_job_id().unwrap();
        assert!(a.starts_with('j') && a.len() == 33);
        assert_ne!(a, b);
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
