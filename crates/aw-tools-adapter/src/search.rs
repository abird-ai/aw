//! Native descriptor-scoped discovery and grep. Globs/regex/gitignore match
//! only already-authorized names and buffers. Reads never take mutation locks.
use crate::{platform_fs, tool_failure};
use aw_agent_core::{
    CancellationToken, Error, Result, Tool, ToolContext, ToolFuture, ToolOutput, ToolSpec,
};
use aw_platform::scoped_fs::Root;
use aw_tools::filesystem::{
    Filter as FsFilter, Kind as FsKind, MetaKey, Selection as FsSelection, Sort as FsSort,
    TimeRange,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

const MAX_DEPTH: usize = 128;
fn walk_error(
    error: aw_tools::filesystem::walk::WalkError<aw_platform::scoped_fs::ScopedError>,
) -> Error {
    use aw_tools::filesystem::walk::WalkError;
    match error {
        WalkError::Source(error) => platform_fs::<()>(Err(error)).unwrap_err(),
        WalkError::Cancelled => Error::Cancelled,
        WalkError::IgnoreLimit => Error::Limit("gitignore search budget exhausted"),
        WalkError::InvalidIgnore => Error::Denied("invalid gitignore pattern"),
    }
}
fn default_limit() -> usize {
    200
}
fn ls_limit() -> usize {
    4096
}
fn dot() -> String {
    ".".into()
}
#[derive(Clone, Copy)]
pub(crate) enum Operation {
    Ls,
    Find,
    Grep,
}

pub(crate) struct Search {
    root: Arc<Root>,
    op: Operation,
    limits: crate::LocalToolLimits,
}
impl Search {
    #[cfg(test)]
    pub(crate) fn new(root: Arc<Root>, op: Operation) -> Self {
        Self::with_limits(root, op, crate::LocalToolLimits::default())
    }
    pub(crate) fn with_limits(
        root: Arc<Root>,
        op: Operation,
        limits: crate::LocalToolLimits,
    ) -> Self {
        Self { root, op, limits }
    }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct LsArgs {
    #[serde(default = "dot")]
    path: String,
    #[serde(default)]
    sort: FsSort,
    #[serde(default)]
    meta: Vec<MetaKey>,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    gitignore: bool,
    #[serde(default)]
    offset: usize,
    #[serde(default = "ls_limit")]
    limit: usize,
    #[serde(default)]
    parallel: bool,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FindArgs {
    #[serde(default = "dot")]
    path: String,
    glob: Option<String>,
    regex: Option<String>,
    depth: Option<usize>,
    #[serde(default)]
    kind: FsKind,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    gitignore: bool,
    mtime: Option<TimeRange>,
    ctime: Option<TimeRange>,
    btime: Option<TimeRange>,
    #[serde(default)]
    sort: FsSort,
    #[serde(default)]
    meta: Vec<MetaKey>,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    parallel: bool,
}
#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum OutputMode {
    #[default]
    Content,
    Files,
    Count,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrepArgs {
    pattern: String,
    #[serde(default = "dot")]
    path: String,
    glob: Option<String>,
    #[serde(default)]
    literal: bool,
    #[serde(default)]
    ignore_case: bool,
    #[serde(default, alias = "output_mode")]
    mode: OutputMode,
    #[serde(default)]
    context: Option<usize>,
    before: Option<usize>,
    after: Option<usize>,
    depth: Option<usize>,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    gitignore: bool,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    parallel: bool,
}
fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|_| Error::Denied("invalid/unknown search arguments"))
}
fn path(root: &str) -> Result<&str> {
    if root.is_empty() || root == "." {
        return Ok("");
    }
    crate::model_path(root)?;
    Ok(root)
}
fn require_meta(meta: &[MetaKey]) -> Result<()> {
    if meta.len() > 5 {
        return Err(Error::Limit("at most five metadata columns"));
    }
    for (idx, key) in meta.iter().enumerate() {
        if meta[..idx].contains(key) {
            return Err(Error::Denied("duplicate metadata column"));
        }
    }
    Ok(())
}
fn run_discovery(
    root: &Root,
    query: aw_tools::filesystem::discovery::Options<'_>,
    cancel: &CancellationToken,
) -> Result<ToolOutput> {
    use aw_tools::filesystem::discovery::{self, Error as E};
    let validated = path(query.path)?;
    let backend_path = if validated.is_empty() {
        None
    } else {
        Some(crate::directory_path(root, validated)?)
    };
    let options = discovery::Options {
        path: validated,
        backend_path: backend_path.as_deref(),
        ..query
    };
    let page =
        discovery::run(root, options, &|| cancel.is_cancelled()).map_err(|error| match error {
            E::InvalidOptions(reason) => Error::Denied(reason),
            E::Walk(error) => walk_error(error),
        })?;
    Ok(ToolOutput {
        content: page.content,
        is_error: false,
        truncated: page.truncated,
    })
}
fn list(
    root: &Root,
    args: &LsArgs,
    cancel: &CancellationToken,
    output_bytes: usize,
) -> Result<ToolOutput> {
    let filter =
        FsFilter::compile(None, None, FsKind::Any, None, None, None).map_err(Error::Denied)?;
    run_discovery(
        root,
        aw_tools::filesystem::discovery::Options {
            path: &args.path,
            backend_path: None,
            depth: 1,
            gitignore: args.gitignore,
            hidden: args.hidden,
            filter: &filter,
            sort: args.sort,
            meta: &args.meta,
            offset: args.offset,
            limit: args.limit,
            basenames: true,
            output_bytes,
        },
        cancel,
    )
}
fn find(
    root: &Root,
    args: &FindArgs,
    cancel: &CancellationToken,
    output_bytes: usize,
) -> Result<ToolOutput> {
    let filter = FsFilter::compile(
        args.glob.as_deref(),
        args.regex.as_deref(),
        args.kind,
        args.mtime,
        args.ctime,
        args.btime,
    )
    .map_err(Error::Denied)?;
    run_discovery(
        root,
        aw_tools::filesystem::discovery::Options {
            path: &args.path,
            backend_path: None,
            depth: args.depth.unwrap_or(MAX_DEPTH),
            gitignore: args.gitignore,
            hidden: args.hidden,
            filter: &filter,
            sort: args.sort,
            meta: &args.meta,
            offset: args.offset,
            limit: args.limit,
            basenames: false,
            output_bytes,
        },
        cancel,
    )
}
impl From<OutputMode> for aw_tools::search::GrepMode {
    fn from(value: OutputMode) -> Self {
        match value {
            OutputMode::Content => Self::Content,
            OutputMode::Files => Self::Files,
            OutputMode::Count => Self::Count,
        }
    }
}

fn grep_options<'a>(
    args: &'a GrepArgs,
    start: &'a str,
    backend_path: Option<&'a str>,
    output_bytes: usize,
) -> aw_tools::search::GrepOptions<'a> {
    let context = args.context.unwrap_or(0);
    aw_tools::search::GrepOptions {
        pattern: &args.pattern,
        path: start,
        backend_path,
        glob: args.glob.as_deref(),
        depth: args.depth.unwrap_or(MAX_DEPTH),
        literal: args.literal,
        ignore_case: args.ignore_case,
        mode: args.mode.into(),
        before: args.before.unwrap_or(context),
        after: args.after.unwrap_or(context),
        hidden: args.hidden,
        gitignore: args.gitignore,
        offset: args.offset,
        limit: args.limit,
        limits: aw_tools::search::GrepLimits {
            output_bytes,
            ..Default::default()
        },
    }
}

fn grep_query_error(error: aw_tools::search::QueryError) -> Error {
    use aw_tools::search::QueryError as E;
    match error {
        E::InvalidPattern => Error::Denied("invalid or overly complex grep pattern"),
        E::InvalidGlob => Error::Denied("invalid grep glob"),
        E::InvalidDepth | E::InvalidContext | E::InvalidPage | E::InvalidLimits => {
            Error::Denied("invalid grep limit/depth/context")
        }
    }
}

fn grep_error(error: aw_tools::search::GrepError<aw_platform::scoped_fs::ScopedError>) -> Error {
    use aw_tools::search::GrepError as E;
    match error {
        E::Query(error) => grep_query_error(error),
        E::Source(error) => platform_fs::<()>(Err(error)).unwrap_err(),
        E::Walk(error) => walk_error(error),
        E::Cancelled => Error::Cancelled,
        E::ResultLimit(reason) => Error::Limit(reason),
    }
}

fn search_backend_path(root: &Root, start: &str) -> Result<String> {
    if start.is_empty() {
        return Ok(String::new());
    }
    let kind = platform_fs(root.entry_meta(start))?.kind;
    match kind {
        FsKind::File => crate::content_path(root, start, false),
        FsKind::Dir => crate::directory_path(root, start),
        FsKind::Symlink => {
            crate::directory_path(root, start).or_else(|_| crate::content_path(root, start, false))
        }
        _ => Err(Error::Denied("grep path must be a file or directory")),
    }
}

fn grep(
    root: &Root,
    args: &GrepArgs,
    cancel: &CancellationToken,
    output_bytes: usize,
) -> Result<ToolOutput> {
    let start = path(&args.path)?;
    let backend_path = search_backend_path(root, start)?;
    let page = aw_tools::search::grep(
        root,
        grep_options(args, start, Some(&backend_path), output_bytes),
        &|| cancel.is_cancelled(),
    )
    .map_err(grep_error)?;
    Ok(ToolOutput {
        content: page.content,
        is_error: false,
        truncated: page.truncated,
    })
}

impl Tool for Search {
    fn spec(&self) -> ToolSpec {
        use aw_tools::filesystem::schema::Tool as C;
        let tool = match self.op {
            Operation::Ls => C::Ls,
            Operation::Find => C::Find,
            Operation::Grep => C::Grep,
        };
        crate::core_spec(tool, self.limits.schema)
    }
    fn validate(&self, value: &Value) -> Result<()> {
        match self.op {
            Operation::Ls => {
                let a: LsArgs = parse(value)?;
                path(&a.path)?;
                require_meta(&a.meta)?;
                FsSelection::new(a.sort, a.offset, a.limit).map_err(Error::Limit)?;
                let _ = a.parallel;
            }
            Operation::Find => {
                let a: FindArgs = parse(value)?;
                path(&a.path)?;
                require_meta(&a.meta)?;
                if a.depth.is_some_and(|v| !(1..=MAX_DEPTH).contains(&v)) {
                    return Err(Error::Denied("invalid discovery depth"));
                }
                FsSelection::new(a.sort, a.offset, a.limit).map_err(Error::Limit)?;
                FsFilter::compile(
                    a.glob.as_deref(),
                    a.regex.as_deref(),
                    a.kind,
                    a.mtime,
                    a.ctime,
                    a.btime,
                )
                .map_err(Error::Denied)?;
                let _ = a.parallel;
            }
            Operation::Grep => {
                let a: GrepArgs = parse(value)?;
                let start = path(&a.path)?;
                if a.context.is_some() && (a.before.is_some() || a.after.is_some()) {
                    return Err(Error::Denied("invalid grep limit/depth/context"));
                }
                aw_tools::search::validate_grep(&grep_options(
                    &a,
                    start,
                    None,
                    self.limits.result_bytes,
                ))
                .map_err(grep_query_error)?;
                let _ = a.parallel;
            }
        }
        Ok(())
    }
    fn execute<'a>(&'a self, ctx: ToolContext, value: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(value)?;
            let request = value.clone();
            let root = self.root.clone();
            let op = self.op;
            let token = ctx.cancellation.clone();
            let output_bytes = ctx.result_budget(self.limits.result_bytes)?;
            let answer = aw_tools::asynchronous::run_blocking(move || match op {
                Operation::Ls => list(&root, &parse::<LsArgs>(&request)?, &token, output_bytes),
                Operation::Find => find(&root, &parse::<FindArgs>(&request)?, &token, output_bytes),
                Operation::Grep => grep(&root, &parse::<GrepArgs>(&request)?, &token, output_bytes),
            })
            .await
            .map_err(|_| Error::Protocol("search worker failed"))?;
            tool_failure(answer, false)
        })
    }
}
