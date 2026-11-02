//! Explicit native host tools. AW owns authorization, journal, execution and
//! model-directed scheduling. This crate supplies only bounded tool adapters.
use aw_agent_core::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};

mod binary;
mod edit;
pub mod job_host;
pub mod job_runtime;
pub mod job_tools;
mod lifecycle;
mod mutate;
mod patch;
mod private_artifacts;
mod search;
mod text;
mod text_class;

pub mod browse;
pub mod media_port;
pub mod process;
mod shell;

/// Transport-neutral canonical DL-150 schemas come from aw-tools;
/// AW maps their access requirement into the original Agent effect type.
fn core_spec(
    tool: aw_tools::filesystem::schema::Tool,
    caps: aw_tools::filesystem::schema::SchemaCaps,
) -> ToolSpec {
    use aw_tools::filesystem::access::Action;
    let core = aw_tools::filesystem::schema::spec(tool, caps);
    let effect = match core.action {
        Action::Read => ToolEffect::Read,
        Action::Shell => ToolEffect::Process,
        _ => ToolEffect::Write,
    };
    ToolSpec {
        name: core.name.into(),
        description: core.description,
        parameters: core.parameters,
        effect,
    }
}
/// Map shared-operation failures without creating a second tool-policy path.
fn file_operation_error(
    error: aw_tools::filesystem::operations::Error<aw_platform::scoped_fs::ScopedError>,
) -> Error {
    use aw_tools::filesystem::operations::Error as E;
    match error {
        E::InvalidPath(_) => Error::Denied("expected exact clean relative path"),
        E::TooLarge => Error::Limit("file exceeds bounded write size"),
        E::InvalidText => Error::Denied("binary or control-containing content; use binary tools"),
        E::Edit(_) => Error::Denied("invalid or ambiguous exact replacement"),
        E::BinaryPlan(_) => Error::Denied("invalid binary edit plan"),
        E::BinaryPrecondition => Error::Denied("binary precondition mismatch"),
        E::BinarySourceChanged => {
            Error::Denied("binary source changed during edit; re-read before retrying")
        }
        E::Patch(_) => Error::Denied("invalid canonical patch"),
        E::DestinationExists => Error::Denied("destination already exists"),
        E::Cancelled => Error::Cancelled,
        E::PartialCancelled { .. } => Error::Protocol(
            "filesystem mutation partially committed before cancellation; inspect workspace",
        ),
        E::PatchCommitUnknown { .. } => {
            Error::Protocol("patch publication outcome unknown; inspect workspace before retrying")
        }
        E::PartialCommit { .. } => Error::Protocol(
            "filesystem mutation partially committed; inspect workspace before retrying",
        ),
        E::Backend(error) => platform_fs::<()>(Err(error)).unwrap_err(),
        E::UnknownCommit(_) => {
            Error::Protocol("unknown filesystem commit; do not retry; inspect workspace")
        }
    }
}
/// One model-path admission rule for every filesystem adapter, including
/// read/binary/edit/patch (not only the exact lifecycle tools).
fn model_path(path: &str) -> Result<()> {
    platform_fs(aw_platform::scoped_fs::validate_path(path))
}

fn exact_model_path(root: &aw_platform::scoped_fs::Root, path: &str) -> Result<()> {
    model_path(path)?;
    if platform_fs(root.is_private_path(path))? {
        return Err(Error::Denied(
            "private mutation artifact is not model-addressable",
        ));
    }
    Ok(())
}
fn content_path(
    root: &aw_platform::scoped_fs::Root,
    path: &str,
    allow_missing: bool,
) -> Result<String> {
    exact_model_path(root, path)?;
    let resolved = platform_fs(root.resolve_content_path(path, allow_missing))?;
    exact_model_path(root, &resolved)?;
    Ok(resolved)
}
fn directory_path(root: &aw_platform::scoped_fs::Root, path: &str) -> Result<String> {
    exact_model_path(root, path)?;
    let resolved = platform_fs(root.resolve_directory_path(path))?;
    exact_model_path(root, &resolved)?;
    Ok(resolved)
}
fn platform_fs<T>(result: aw_platform::scoped_fs::Result<T>) -> Result<T> {
    result.map_err(|error| match error {
        aw_platform::scoped_fs::ScopedError::Io(error) => Error::Io(error),
        aw_platform::scoped_fs::ScopedError::Denied(reason) => Error::Denied(reason),
        aw_platform::scoped_fs::ScopedError::Limit(reason) => Error::Limit(reason),
        aw_platform::scoped_fs::ScopedError::Cancelled => Error::Cancelled,
    })
}

/// Translate a bounded, known user error to an ordinary tool result. An
/// interrupted/uncertain mutator remains a journal-visible execution failure.
fn tool_failure(outcome: Result<ToolOutput>, mutating: bool) -> Result<ToolOutput> {
    match outcome {
        Err(Error::Denied(reason)) | Err(Error::Limit(reason)) => {
            Ok(ToolOutput::error(format!("fail {reason}")))
        }
        Err(Error::Io(_)) if mutating => Err(Error::Protocol(
            "unknown commit; do not retry; inspect workspace",
        )),
        Err(Error::Io(_)) => Ok(ToolOutput::error(
            "fail file unavailable under the granted root",
        )),
        other => other,
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ToolMode {
    #[default]
    Off,
    ReadOnly,
    #[serde(rename = "rw", alias = "workspace")]
    Workspace,
    #[serde(rename = "full", alias = "trusted")]
    Trusted,
}
impl ToolMode {
    /// Existing constrained SQLite child-mode representation.
    pub fn storage_label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::ReadOnly => "read-only",
            Self::Workspace => "workspace",
            Self::Trusted => "trusted",
        }
    }
    fn access(self) -> aw_tools::filesystem::access::Access {
        use aw_tools::filesystem::access::Access;
        match self {
            Self::Off => Access::Off,
            Self::ReadOnly => Access::ReadOnly,
            Self::Workspace => Access::ReadWrite,
            Self::Trusted => Access::Full,
        }
    }
}
impl std::str::FromStr for ToolMode {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s {
            "off" => Ok(Self::Off),
            "read-only" => Ok(Self::ReadOnly),
            "rw" | "workspace" => Ok(Self::Workspace),
            "full" | "trusted" => Ok(Self::Trusted),
            _ => Err("use off, read-only, rw or full".into()),
        }
    }
}
impl std::fmt::Display for ToolMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Off => "off",
            Self::ReadOnly => "read-only",
            Self::Workspace => "rw",
            Self::Trusted => "full",
        })
    }
}
const MIN_LOCAL_RESULT_BYTES: usize =
    aw_tools::filesystem::job_manager::MAX_COMPACT_JOB_STATUS_BYTES;
const DEFAULT_LOCAL_RESULT_BYTES: usize = 96 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalToolLimits {
    pub schema: aw_tools::filesystem::schema::SchemaCaps,
    /// Semantic native-tool result ceiling. Direct paged tools and retained
    /// native Job results share this bound; outer Agent presentation may be tighter.
    pub result_bytes: usize,
    /// Per-stream retained/truncated shell output ceiling. This is independent
    /// from compact ShellNode terminal metadata and may only narrow the default.
    pub shell_stream_bytes: usize,
    /// Aggregate old+new text admitted by one edit request. Per-file and total
    /// mutation work remain fixed shared safety bounds below this adapter.
    pub edit_text_bytes: usize,
    pub jobs: aw_tools::filesystem::job_manager::Limits,
}
impl Default for LocalToolLimits {
    fn default() -> Self {
        Self {
            schema: aw_tools::filesystem::schema::SchemaCaps {
                parallel_reads: true,
                ..Default::default()
            },
            result_bytes: DEFAULT_LOCAL_RESULT_BYTES,
            shell_stream_bytes: 48 * 1024,
            edit_text_bytes: 48 * 1024,
            jobs: aw_tools::filesystem::job_manager::Limits::default(),
        }
    }
}
impl LocalToolLimits {
    pub fn validate(self) -> Result<Self> {
        self.schema
            .validate()
            .map_err(|message| Error::Config(message.into()))?;
        self.jobs
            .validate()
            .map_err(|_| Error::Config("invalid shared JobManager limits".into()))?;
        let defaults = Self::default();
        if self.schema.text_write_bytes > defaults.schema.text_write_bytes
            || self.schema.patch_bytes > defaults.schema.patch_bytes
            || self.schema.binary_data_bytes > defaults.schema.binary_data_bytes
            || self.schema.binary_edit_ranges > defaults.schema.binary_edit_ranges
            || self.schema.binary_read_bytes > defaults.schema.binary_read_bytes
            || self.schema.binary_encoded_read_bytes > defaults.schema.binary_encoded_read_bytes
            || self.schema.read_line_limit > defaults.schema.read_line_limit
            || self.schema.typed_media
            || !(MIN_LOCAL_RESULT_BYTES..=defaults.result_bytes).contains(&self.result_bytes)
            || !(1..=defaults.shell_stream_bytes).contains(&self.shell_stream_bytes)
            || !(1..=defaults.edit_text_bytes).contains(&self.edit_text_bytes)
            || self.jobs.max_retained_jobs > defaults.jobs.max_retained_jobs
            || self.jobs.max_active_nodes > defaults.jobs.max_active_nodes
            || self.jobs.max_hidden_replays > defaults.jobs.max_hidden_replays
            || self.jobs.max_spool_bytes > defaults.jobs.max_spool_bytes
        {
            return Err(Error::Config(
                "local AW tool limits may only narrow built-in numeric defaults; typed-media capability is host-owned".into(),
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalToolConcurrency {
    /// Graph-internal global ceiling. The default preserves the historical
    /// shared Job width; hosts may narrow it for a configured Agent.
    pub graph_width: usize,
    /// The same host-owned read/shell ceilings used by the Agent scheduler.
    pub parallelism: aw_agent_core::ToolParallelism,
}
impl Default for LocalToolConcurrency {
    fn default() -> Self {
        Self {
            graph_width: 8,
            parallelism: aw_agent_core::ToolParallelism::default(),
        }
    }
}
impl LocalToolConcurrency {
    pub fn validate(self) -> Result<Self> {
        if !(1..=8).contains(&self.graph_width) {
            return Err(Error::Config("Graph parallel ceiling must be 1..=8".into()));
        }
        self.parallelism.validate()?;
        Ok(self)
    }
}

#[derive(Clone, Debug)]
pub struct LocalToolOptions {
    pub root: PathBuf,
    pub mode: ToolMode,
    pub execution_site: String,
    pub bash: Option<PathBuf>,
    /// Existing AW private state root used only for crash-safe internal tool
    /// records. None preserves explicit memory-only/direct SDK semantics.
    pub private_state_dir: Option<PathBuf>,
    pub selected_tools: Vec<String>,
    pub concurrency: LocalToolConcurrency,
    pub limits: LocalToolLimits,
}
impl LocalToolOptions {
    pub fn new(root: impl Into<PathBuf>, mode: ToolMode) -> Self {
        Self {
            root: root.into(),
            mode,
            execution_site: "local".into(),
            bash: None,
            private_state_dir: None,
            selected_tools: Vec::new(),
            concurrency: LocalToolConcurrency::default(),
            limits: LocalToolLimits::default(),
        }
    }
}
fn offer<T: Tool + 'static>(
    builder: AgentBuilder,
    tool: T,
    selected: &BTreeSet<&str>,
    available: &mut BTreeSet<String>,
    policy: &mut ToolPolicy,
    actor: &PrincipalId,
) -> Result<AgentBuilder> {
    let spec = tool.spec();
    available.insert(spec.name.clone());
    if selected.is_empty() || selected.contains(spec.name.as_str()) {
        policy.permit_host(actor.clone(), &spec.name, &spec.effect)?;
        Ok(builder.tool(tool))
    } else {
        Ok(builder)
    }
}
fn offer_shared(
    builder: AgentBuilder,
    tool: Arc<dyn Tool>,
    selected: &BTreeSet<&str>,
    available: &mut BTreeSet<String>,
    policy: &mut ToolPolicy,
    actor: &PrincipalId,
) -> Result<AgentBuilder> {
    let spec = tool.spec();
    available.insert(spec.name.clone());
    if selected.is_empty() || selected.contains(spec.name.as_str()) {
        policy.permit_host(actor.clone(), &spec.name, &spec.effect)?;
        Ok(builder.shared_tool(tool))
    } else {
        Ok(builder)
    }
}
fn offer_required<T: Tool + 'static>(
    builder: AgentBuilder,
    tool: T,
    selected: &BTreeSet<&str>,
    required: bool,
    available: &mut BTreeSet<String>,
    policy: &mut ToolPolicy,
    actor: &PrincipalId,
) -> Result<AgentBuilder> {
    let spec = tool.spec();
    available.insert(spec.name.clone());
    if required || selected.is_empty() || selected.contains(spec.name.as_str()) {
        policy.permit_host(actor.clone(), &spec.name, &spec.effect)?;
        Ok(builder.tool(tool))
    } else {
        Ok(builder)
    }
}
pub fn configure(
    mut builder: AgentBuilder,
    actor: PrincipalId,
    options: &LocalToolOptions,
) -> Result<AgentBuilder> {
    let concurrency = options.concurrency.validate()?;
    let limits = options.limits.validate()?;
    if concurrency.graph_width > limits.jobs.max_active_nodes {
        return Err(Error::Config(
            "Graph parallel ceiling exceeds configured Job active-node ceiling".into(),
        ));
    }
    let mut policy = builder.tool_policy_snapshot();
    if policy.execution_site != options.execution_site {
        let default_site = ToolPolicy::default().execution_site;
        if policy.execution_site == default_site && !policy.has_grants() {
            policy.execution_site = options.execution_site.clone();
        } else {
            return Err(Error::Config(
                "local tools cannot retarget an existing host tool execution site".into(),
            ));
        }
    }
    // Adapter composition is narrow-only for host resource policy. Existing
    // builder ceilings are authoritative; LocalToolOptions may tighten them but
    // must never silently widen an embedding host's scheduler policy.
    policy.max_parallel_reads = policy
        .max_parallel_reads
        .min(concurrency.parallelism.max_parallel_reads);
    policy.allow_parallel_bash &= concurrency.parallelism.allow_parallel_shell;
    policy.max_parallel_bash = policy
        .max_parallel_bash
        .min(concurrency.parallelism.max_parallel_shell);
    let effective_concurrency = LocalToolConcurrency {
        graph_width: concurrency.graph_width,
        parallelism: policy.parallelism(),
    };
    if options.mode == ToolMode::Off {
        if !options.selected_tools.is_empty() {
            return Err(Error::Denied("tool selection needs an enabled mode"));
        }
        return Ok(builder.tool_policy(policy));
    }
    let root = Arc::new(private_artifacts::open_root(
        &options.root,
        options.private_state_dir.as_deref(),
    )?);
    let durable_remove = root.private_artifacts().is_durable();
    let selected = options
        .selected_tools
        .iter()
        // Input-only legacy selection alias. The registered model tool
        // advertises the sealed DL-150 pwsh identity on Windows.
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut available = BTreeSet::new();
    // One live Job runtime is shared by batch and every job_* projection.
    // All Job projections share this one live runtime. V2 ShellNodes are now
    // prepared through the same direct-shell interpreter objects, so job_start
    // can advertise the final Graph constructor without an intermediate schema.
    const JOB_TOOL_NAMES: &[&str] = &[
        "batch",
        "job_start",
        "job_list",
        "job_get",
        "job_output",
        "job_ack",
        "job_cancel",
        "job_restart",
    ];
    let constructor_selected = selected.iter().any(|name| {
        matches!(
            *name,
            "batch" | "job_start" | "bash" | "pwsh" | "powershell"
        )
    });
    let control_selected = selected.iter().any(|name| {
        matches!(
            *name,
            "job_list" | "job_get" | "job_output" | "job_ack" | "job_cancel" | "job_restart"
        )
    });
    let wants_jobs = selected.is_empty() || constructor_selected || control_selected;
    let graph_native_tools = aw_tools::filesystem::schema::Tool::GRAPH_NATIVE
        .into_iter()
        .filter(|tool| options.mode.access().permits(tool.action()))
        .filter(|tool| *tool != aw_tools::filesystem::schema::Tool::Remove || durable_remove)
        .filter(|tool| selected.is_empty() || selected.contains(tool.name()))
        .collect::<Vec<_>>();
    let graph_shell_available = options
        .mode
        .access()
        .permits(aw_tools::filesystem::access::Action::Shell)
        && (selected.is_empty()
            || selected.iter().any(|name| {
                #[cfg(unix)]
                {
                    *name == "bash"
                }
                #[cfg(windows)]
                {
                    matches!(*name, "pwsh" | "powershell")
                }
            }));
    if !selected.is_empty() && control_selected && !constructor_selected {
        return Err(Error::Denied(
            "selected Job controls require an explicitly selected batch, job_start or shell constructor",
        ));
    }
    if !selected.is_empty()
        && constructor_selected
        && graph_native_tools.is_empty()
        && !graph_shell_available
    {
        return Err(Error::Denied(
            "selected Job constructors require at least one explicitly selected, mode-permitted native or shell node",
        ));
    }
    // Exact node results reserve their captured semantic capacity before an
    // effect starts and remain retained until ACK/expiry. With an otherwise
    // empty live Job namespace, one valid max-size Graph must therefore be able
    // to retain every node's exact result reservation; a narrower session spool
    // would turn valid later nodes into quota-driven `skip` outcomes.
    if wants_jobs {
        let native_reservation = if graph_native_tools.is_empty() {
            0
        } else {
            limits.result_bytes as u64
        };
        let shell_reservation = if graph_shell_available {
            (job_host::SHELL_RESULT_BYTES as u64)
                .checked_add(2 * limits.shell_stream_bytes as u64)
                .ok_or_else(|| Error::Config("ShellNode spool reservation overflow".into()))?
        } else {
            0
        };
        let per_node_reservation = native_reservation.max(shell_reservation);
        let minimum_graph_spool = per_node_reservation
            .checked_mul(aw_tools::filesystem::batch::MAX_OPS as u64)
            .ok_or_else(|| Error::Config("Job Graph spool reservation overflow".into()))?;
        if minimum_graph_spool > 0 && limits.jobs.max_spool_bytes < minimum_graph_spool {
            return Err(Error::Config(
                "Job spool ceiling cannot retain one maximum-size Graph's exact node results"
                    .into(),
            ));
        }
    }
    let job_runtime = if wants_jobs {
        let runtime = job_runtime::JobRuntime::new_with_native_tools(
            root.clone(),
            builder.tool_binding_authority(),
            limits,
            effective_concurrency,
            graph_native_tools,
        )?;
        builder = builder.scope_lifecycle(runtime.clone());
        builder = offer(
            builder,
            job_tools::Batch::new(runtime.clone()),
            &selected,
            &mut available,
            &mut policy,
            &actor,
        )?;
        builder = offer(
            builder,
            job_tools::JobControl::new(runtime.clone(), job_tools::JobToolKind::Start),
            &selected,
            &mut available,
            &mut policy,
            &actor,
        )?;
        for kind in [
            job_tools::JobToolKind::List,
            job_tools::JobToolKind::Get,
            job_tools::JobToolKind::Output,
            job_tools::JobToolKind::Ack,
            job_tools::JobToolKind::Cancel,
            job_tools::JobToolKind::Restart,
        ] {
            builder = offer_required(
                builder,
                job_tools::JobControl::new(runtime.clone(), kind),
                &selected,
                constructor_selected,
                &mut available,
                &mut policy,
                &actor,
            )?;
        }
        Some(runtime)
    } else {
        available.extend(JOB_TOOL_NAMES.iter().map(|name| (*name).to_owned()));
        None
    };
    builder = offer(
        builder,
        text::Text::with_limits(root.clone(), text::Operation::Read, limits),
        &selected,
        &mut available,
        &mut policy,
        &actor,
    )?;
    for operation in [
        search::Operation::Ls,
        search::Operation::Find,
        search::Operation::Grep,
    ] {
        builder = offer(
            builder,
            search::Search::with_limits(root.clone(), operation, limits),
            &selected,
            &mut available,
            &mut policy,
            &actor,
        )?;
    }
    builder = offer(
        builder,
        binary::Binary::with_limits(root.clone(), binary::Operation::Read, limits),
        &selected,
        &mut available,
        &mut policy,
        &actor,
    )?;
    if options
        .mode
        .access()
        .permits(aw_tools::filesystem::access::Action::Write)
    {
        builder = offer(
            builder,
            text::Text::with_limits(root.clone(), text::Operation::Write, limits),
            &selected,
            &mut available,
            &mut policy,
            &actor,
        )?;
        builder = offer(
            builder,
            edit::Edit::with_limits(root.clone(), limits),
            &selected,
            &mut available,
            &mut policy,
            &actor,
        )?;
        builder = offer(
            builder,
            patch::Patch::with_limits(root.clone(), limits),
            &selected,
            &mut available,
            &mut policy,
            &actor,
        )?;
        for operation in [lifecycle::Operation::Copy, lifecycle::Operation::Move] {
            builder = offer(
                builder,
                lifecycle::Lifecycle::with_limits(root.clone(), operation, limits),
                &selected,
                &mut available,
                &mut policy,
                &actor,
            )?;
        }
        if durable_remove {
            builder = offer(
                builder,
                lifecycle::Lifecycle::with_limits(
                    root.clone(),
                    lifecycle::Operation::Remove,
                    limits,
                ),
                &selected,
                &mut available,
                &mut policy,
                &actor,
            )?;
        }
        for operation in [binary::Operation::Write, binary::Operation::Edit] {
            builder = offer(
                builder,
                binary::Binary::with_limits(root.clone(), operation, limits),
                &selected,
                &mut available,
                &mut policy,
                &actor,
            )?;
        }
    }
    if options
        .mode
        .access()
        .permits(aw_tools::filesystem::access::Action::Shell)
    {
        #[cfg(unix)]
        {
            // The selected subset must not force an excluded shell to start.
            if selected.is_empty() || selected.contains("bash") {
                let bash = options.bash.clone().ok_or_else(|| {
                    Error::Config("trusted mode needs an absolute --tools-bash executable".into())
                })?;
                if !bash.is_absolute() || !bash.is_file() {
                    return Err(Error::Config(
                        "bash must be an absolute executable file".into(),
                    ));
                }
                let shell = Arc::new(shell::Bash::with_output_limit(
                    options.root.canonicalize()?,
                    bash,
                    limits.shell_stream_bytes,
                )?);
                if let Some(runtime) = &job_runtime {
                    runtime.register_shell(shell.clone())?;
                }
                let direct: Arc<dyn Tool> = if let Some(runtime) = &job_runtime {
                    Arc::new(job_tools::JobShell::new(runtime.clone(), shell.clone()))
                } else {
                    shell
                };
                builder = offer_shared(
                    builder,
                    direct,
                    &selected,
                    &mut available,
                    &mut policy,
                    &actor,
                )?;
            } else {
                available.insert("bash".into());
            }
        }
        #[cfg(windows)]
        {
            if selected.is_empty() || selected.contains("pwsh") || selected.contains("powershell") {
                let executable = options.bash.clone().ok_or_else(|| {
                    Error::Config(
                        "trusted mode needs an absolute --tools-powershell executable".into(),
                    )
                })?;
                if !executable.is_absolute() || !executable.is_file() {
                    return Err(Error::Config(
                        "powershell executable must be absolute".into(),
                    ));
                }
                let shell = Arc::new(shell::PowerShell::with_output_limit(
                    options.root.canonicalize()?,
                    executable,
                    limits.shell_stream_bytes,
                )?);
                if let Some(runtime) = &job_runtime {
                    runtime.register_shell(shell.clone())?;
                }
                // Exact prepared interpreter identity. A selected pwsh is
                // never silently substituted with Windows PowerShell.
                let direct: Arc<dyn Tool> = if let Some(runtime) = &job_runtime {
                    Arc::new(job_tools::JobShell::new(runtime.clone(), shell.clone()))
                } else {
                    shell
                };
                builder = offer_shared(
                    builder,
                    direct,
                    &selected,
                    &mut available,
                    &mut policy,
                    &actor,
                )?;
            } else {
                // No shell requested in this selected-tool subset.
            }
        }
    }
    if selected.iter().any(|name| !available.contains(*name)) {
        return Err(Error::Denied(
            "selected tool is unavailable under the current tool mode",
        ));
    }
    Ok(builder.tool_policy(policy))
}
pub fn validate_root(path: &Path) -> Result<PathBuf> {
    let path = path.canonicalize()?;
    if !path.is_dir() {
        return Err(Error::Config("tool root must be a directory".into()));
    }
    Ok(path)
}

#[cfg(test)]
mod tool_tests;
