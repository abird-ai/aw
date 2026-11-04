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
fn canonical_semantic_value(
    tool: aw_tools::filesystem::schema::Tool,
    value: &serde_json::Value,
) -> serde_json::Value {
    let mut semantic = value.clone();
    if tool.action() == aw_tools::filesystem::access::Action::Read {
        if let Some(object) = semantic.as_object_mut() {
            object.remove("parallel");
        }
    }
    semantic
}

fn canonical_request(
    tool: aw_tools::filesystem::schema::Tool,
    value: &serde_json::Value,
    limits: LocalToolLimits,
) -> Result<aw_tools::filesystem::request::Request> {
    aw_tools::filesystem::request::parse(
        tool,
        &canonical_semantic_value(tool, value),
        limits.request_limits(),
    )
    .map_err(|_| Error::Denied("invalid canonical filesystem request"))
}

fn canonical_direct_limits(
    limits: LocalToolLimits,
    result_bytes: usize,
    request: &aw_tools::filesystem::request::Request,
) -> aw_tools::filesystem::direct::Limits {
    let read_data_bytes = match request.kind() {
        aw_tools::filesystem::request::RequestKind::Read(read) if read.numbered => {
            result_bytes.saturating_sub(256)
        }
        aw_tools::filesystem::request::RequestKind::Read(_) => result_bytes / 2,
        _ => result_bytes,
    };
    aw_tools::filesystem::direct::Limits {
        request: limits.request_limits(),
        result_bytes,
        read_data_bytes,
        binary_result_bytes: result_bytes,
        text_read_bytes: 1024 * 1024,
        text_write_bytes: 1024 * 1024,
        binary_file_bytes: aw_tools::MAX_READ_WINDOW_BYTES,
        binary_default_window: 24 * 1024,
        grep: aw_tools::search::GrepLimits::default(),
    }
}

fn media_resource_uri(path: &str) -> Result<String> {
    let mut uri = url::Url::parse("abird://file/")
        .map_err(|_| Error::Protocol("invalid binary resource URI base"))?;
    uri.path_segments_mut()
        .map_err(|_| Error::Protocol("invalid binary resource URI path"))?
        .pop_if_empty()
        .extend(path.split('/'));
    Ok(uri.into())
}

fn canonical_direct_output(
    output: aw_tools::filesystem::direct::Output,
    result_bytes: usize,
) -> Result<ToolOutput> {
    use aw_tools::filesystem::direct::Body;
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    let mut projected = match output.body {
        Body::Text(content) => ToolOutput {
            content,
            is_error: output.is_error,
            truncated: output.truncated,
        },
        Body::Read {
            mut data,
            status,
            next,
            numbered,
        } => {
            if let Some(status_line) = status {
                if numbered {
                    if !data.ends_with('\n') {
                        data.push('\n');
                    }
                    data.push_str(&status_line);
                    ToolOutput {
                        content: data,
                        is_error: output.is_error,
                        truncated: true,
                    }
                } else {
                    ToolOutput {
                        content: serde_json::json!({
                            "data":data,
                            "status":if output.is_error{"mixed"}else{"more"},
                            "next":next.map(|(line,byte)|serde_json::json!({
                                "line":line,"byte":byte
                            }))
                        })
                        .to_string(),
                        is_error: output.is_error,
                        truncated: true,
                    }
                }
            } else {
                ToolOutput {
                    content: data,
                    is_error: output.is_error,
                    truncated: output.truncated,
                }
            }
        }
        Body::Media { path, packet } => {
            let descriptor = packet.descriptor();
            let kind = descriptor.kind.as_str();
            ToolOutput::text(
                serde_json::json!({
                    "type":kind,
                    "mime_type":descriptor.mime,
                    "format":"mcp",
                    "path":path,
                    "data":BASE64.encode(packet.bytes()),
                    "uri":if kind=="resource"{Some(media_resource_uri(&path)?)}else{None},
                    "bytes":packet.bytes().len(),
                    "sha256":descriptor.sha256_hex(),
                    "mime_source":if descriptor.signature_identified{"signature"}else{"fallback"},
                    "transport":"encoded_json"
                })
                .to_string(),
            )
        }
    };
    if projected.content.len() > result_bytes {
        return Err(Error::Limit(
            "canonical direct result exceeds configured result ceiling",
        ));
    }
    projected.truncated |= output.truncated;
    Ok(projected)
}

fn canonical_direct_error(
    error: aw_tools::filesystem::direct::Error<aw_platform::scoped_fs::ScopedError>,
) -> Error {
    use aw_tools::filesystem::direct::Error as E;
    match error {
        E::Invalid(_) => Error::Denied("invalid canonical filesystem operation"),
        E::Limit(reason) => Error::Limit(reason),
        E::Cancelled => Error::Cancelled,
        E::Backend(error) => platform_fs::<()>(Err(error)).unwrap_err(),
        E::UnknownCommit(_) => {
            Error::Protocol("unknown filesystem commit; do not retry; inspect workspace")
        }
        E::PartialCommit(_) => Error::Protocol(
            "filesystem mutation partially/uncertainly committed; inspect workspace before retrying",
        ),
    }
}

fn legacy_filesystem_request(
    tool: aw_tools::filesystem::schema::Tool,
    value: &serde_json::Value,
) -> bool {
    use aw_tools::filesystem::schema::Tool as T;
    let has = |key: &str| value.get(key).is_some();
    match tool {
        T::Read => has("offset") || has("limit"),
        T::Write | T::Bwrite => has("create_only") || has("expected_sha256"),
        T::Edit => {
            let canonical = value
                .get("edits")
                .and_then(serde_json::Value::as_array)
                .and_then(|edits| edits.first())
                .is_some_and(serde_json::Value::is_array);
            !canonical
                && (has("edits")
                    || has("old")
                    || has("new")
                    || has("all")
                    || has("old_text")
                    || has("new_text")
                    || has("replace_all"))
        }
        T::Patch => has("dir") || has("best_effort"),
        T::Grep => has("before") || has("after") || has("output_mode"),
        T::Copy => has("recursive"),
        T::Ls | T::Find | T::Move | T::Remove | T::Bread | T::Bedit => false,
    }
}

fn compatibility_filesystem_tool(
    root: Arc<aw_platform::scoped_fs::Root>,
    tool: aw_tools::filesystem::schema::Tool,
    limits: LocalToolLimits,
) -> Arc<dyn Tool> {
    use aw_tools::filesystem::schema::Tool as T;
    match tool {
        T::Ls => Arc::new(search::Search::with_limits(
            root,
            search::Operation::Ls,
            limits,
        )),
        T::Find => Arc::new(search::Search::with_limits(
            root,
            search::Operation::Find,
            limits,
        )),
        T::Grep => Arc::new(search::Search::with_limits(
            root,
            search::Operation::Grep,
            limits,
        )),
        T::Read => Arc::new(text::Text::with_limits(root, text::Operation::Read, limits)),
        T::Write => Arc::new(text::Text::with_limits(
            root,
            text::Operation::Write,
            limits,
        )),
        T::Edit => Arc::new(edit::Edit::with_limits(root, limits)),
        T::Patch => Arc::new(patch::Patch::with_limits(root, limits)),
        T::Copy => Arc::new(lifecycle::Lifecycle::with_limits(
            root,
            lifecycle::Operation::Copy,
            limits,
        )),
        T::Move => Arc::new(lifecycle::Lifecycle::with_limits(
            root,
            lifecycle::Operation::Move,
            limits,
        )),
        T::Remove => Arc::new(lifecycle::Lifecycle::with_limits(
            root,
            lifecycle::Operation::Remove,
            limits,
        )),
        T::Bread => Arc::new(binary::Binary::with_limits(
            root,
            binary::Operation::Read,
            limits,
        )),
        T::Bwrite => Arc::new(binary::Binary::with_limits(
            root,
            binary::Operation::Write,
            limits,
        )),
        T::Bedit => Arc::new(binary::Binary::with_limits(
            root,
            binary::Operation::Edit,
            limits,
        )),
    }
}

struct CanonicalFilesystemTool {
    root: Arc<aw_platform::scoped_fs::Root>,
    tool: aw_tools::filesystem::schema::Tool,
    limits: LocalToolLimits,
    compatibility: Arc<dyn Tool>,
}
impl CanonicalFilesystemTool {
    fn new(
        root: Arc<aw_platform::scoped_fs::Root>,
        tool: aw_tools::filesystem::schema::Tool,
        limits: LocalToolLimits,
    ) -> Self {
        let compatibility = compatibility_filesystem_tool(root.clone(), tool, limits);
        Self {
            root,
            tool,
            limits,
            compatibility,
        }
    }
}
impl Tool for CanonicalFilesystemTool {
    fn spec(&self) -> ToolSpec {
        core_spec(self.tool, self.limits.schema)
    }

    fn validate(&self, arguments: &serde_json::Value) -> Result<()> {
        if legacy_filesystem_request(self.tool, arguments) {
            self.compatibility.validate(arguments)
        } else {
            canonical_request(self.tool, arguments, self.limits).map(|_| ())
        }
    }

    fn execute<'a>(&'a self, ctx: ToolContext, arguments: &'a serde_json::Value) -> ToolFuture<'a> {
        if legacy_filesystem_request(self.tool, arguments) {
            return self.compatibility.execute(ctx, arguments);
        }
        Box::pin(async move {
            let request = canonical_request(self.tool, arguments, self.limits)?;
            let result_bytes = ctx.result_budget(self.limits.result_bytes)?;
            let direct_limits = canonical_direct_limits(self.limits, result_bytes, &request);
            let root = self.root.clone();
            let cancellation = ctx.cancellation.clone();
            let result = aw_tools::asynchronous::run_blocking(move || {
                aw_tools::filesystem::direct::execute(
                    root.as_ref(),
                    request,
                    direct_limits,
                    &|| cancellation.is_cancelled(),
                )
            })
            .await
            .map_err(|_| {
                Error::Protocol("canonical filesystem worker failed; inspect workspace")
            })?;
            let projected = match result {
                Ok(output) => canonical_direct_output(output, result_bytes),
                Err(error) => Err(canonical_direct_error(error)),
            };
            tool_failure(
                projected,
                self.tool.action() != aw_tools::filesystem::access::Action::Read,
            )
        })
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
    pub(crate) fn validate_job_runtime_capacity(
        self,
        graph_width: usize,
        native_nodes: bool,
        shell_nodes: bool,
    ) -> Result<()> {
        if graph_width == 0 || graph_width > self.jobs.max_active_nodes {
            return Err(Error::Config(
                "Graph parallel ceiling exceeds configured Job active-node ceiling".into(),
            ));
        }
        let native_reservation = if native_nodes {
            self.result_bytes as u64
        } else {
            0
        };
        let shell_reservation = if shell_nodes {
            (job_host::SHELL_RESULT_BYTES as u64)
                .checked_add(2 * self.shell_stream_bytes as u64)
                .ok_or_else(|| Error::Config("ShellNode spool reservation overflow".into()))?
        } else {
            0
        };
        let per_node = native_reservation.max(shell_reservation);
        let minimum_graph_spool = per_node
            .checked_mul(aw_tools::filesystem::batch::MAX_OPS as u64)
            .ok_or_else(|| Error::Config("Job Graph spool reservation overflow".into()))?;
        if minimum_graph_spool > 0 && self.jobs.max_spool_bytes < minimum_graph_spool {
            return Err(Error::Config(
                "Job spool ceiling cannot retain one maximum-size Graph's exact node results"
                    .into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn request_limits(self) -> aw_tools::filesystem::request::Limits {
        aw_tools::filesystem::request::Limits {
            schema: self.schema,
            edit_text_bytes: self.edit_text_bytes,
        }
    }

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
struct ParallelismCeiling {
    ceiling: aw_agent_core::ToolParallelism,
}
impl ToolPolicyLifecycle for ParallelismCeiling {
    fn validate_parallelism(&self, next: aw_agent_core::ToolParallelism) -> Result<()> {
        let ceiling = self.ceiling;
        if next.max_parallel_reads > ceiling.max_parallel_reads
            || next.max_parallel_shell > ceiling.max_parallel_shell
            || (next.allow_parallel_shell && !ceiling.allow_parallel_shell)
        {
            return Err(Error::Config(
                "tool parallel policy may only narrow the configured local-tool ceiling".into(),
            ));
        }
        Ok(())
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
    builder = builder.tool_policy_lifecycle(Arc::new(ParallelismCeiling {
        ceiling: effective_concurrency.parallelism,
    }));
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
    // effect starts and remain retained until ACK/expiry. Every constructor uses
    // the same capacity preflight so valid Graphs never degrade into quota-driven
    // skips merely because the host chose an internally inconsistent limit set.
    if wants_jobs {
        limits.validate_job_runtime_capacity(
            concurrency.graph_width,
            !graph_native_tools.is_empty(),
            graph_shell_available,
        )?;
    }
    let job_runtime = if wants_jobs {
        let runtime = job_runtime::JobRuntime::new_with_native_tools(
            root.clone(),
            builder.tool_binding_authority(),
            limits,
            effective_concurrency,
            graph_native_tools,
        )?;
        builder = builder
            .scope_lifecycle(runtime.clone())
            .tool_policy_lifecycle(runtime.clone());
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
    for core in aw_tools::filesystem::schema::Tool::ALL {
        if !options.mode.access().permits(core.action()) {
            continue;
        }
        if core == aw_tools::filesystem::schema::Tool::Remove && !durable_remove {
            continue;
        }
        builder = offer_shared(
            builder,
            Arc::new(CanonicalFilesystemTool::new(root.clone(), core, limits)),
            &selected,
            &mut available,
            &mut policy,
            &actor,
        )?;
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
