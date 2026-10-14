//! Explicit host tools. Files use descriptor-relative no-follow traversal on
//! Linux. Trusted `bash` is deliberately NOT a security sandbox. This crate is
//! separate from the SDK so embedded consumers need not link subprocess/I/O code.
use aw_agent_core::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
/// Host-owned OS process I/O and optional PTY resource service.
/// No process starts from module initialization or an untrusted source.
pub mod media_port;
pub mod process;
mod search;
#[cfg(unix)]
mod shell;

fn platform_fs<T>(result: aw_platform::scoped_fs::Result<T>) -> Result<T> {
    result.map_err(|error| match error {
        aw_platform::scoped_fs::ScopedError::Io(error) => Error::Io(error),
        aw_platform::scoped_fs::ScopedError::Denied(reason) => Error::Denied(reason),
        aw_platform::scoped_fs::ScopedError::Limit(reason) => Error::Limit(reason),
        aw_platform::scoped_fs::ScopedError::Cancelled => Error::Cancelled,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ToolMode {
    #[default]
    Off,
    ReadOnly,
    Workspace,
    Trusted,
}
impl std::str::FromStr for ToolMode {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s {
            "off" => Ok(Self::Off),
            "read-only" => Ok(Self::ReadOnly),
            "workspace" => Ok(Self::Workspace),
            "trusted" => Ok(Self::Trusted),
            _ => Err("use off, read-only, workspace or trusted".into()),
        }
    }
}
impl std::fmt::Display for ToolMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Off => "off",
            Self::ReadOnly => "read-only",
            Self::Workspace => "workspace",
            Self::Trusted => "trusted",
        })
    }
}
#[derive(Clone, Debug)]
pub struct LocalToolOptions {
    pub root: PathBuf,
    pub mode: ToolMode,
    pub execution_site: String,
    pub bash: Option<PathBuf>,
    pub selected_tools: Vec<String>,
}
impl LocalToolOptions {
    pub fn new(root: impl Into<PathBuf>, mode: ToolMode) -> Self {
        Self {
            root: root.into(),
            mode,
            execution_site: "local".into(),
            bash: None,
            selected_tools: Vec::new(),
        }
    }
}
pub fn configure(
    mut builder: AgentBuilder,
    actor: PrincipalId,
    options: &LocalToolOptions,
) -> Result<AgentBuilder> {
    if options.mode == ToolMode::Off {
        if !options.selected_tools.is_empty() {
            return Err(Error::Denied("tool selection needs an enabled tool mode"));
        }
        return Ok(builder);
    }
    let root = Arc::new(platform_fs(aw_platform::scoped_fs::Root::open(
        &options.root,
    ))?);
    let mut names = vec!["read".to_string()];
    builder = builder.tool(Files {
        root: root.clone(),
        operation: Operation::Read,
    });
    for operation in [
        search::Operation::Ls,
        search::Operation::Find,
        search::Operation::Grep,
    ] {
        names.push(
            match operation {
                search::Operation::Ls => "ls",
                search::Operation::Find => "find",
                search::Operation::Grep => "grep",
            }
            .into(),
        );
        builder = builder.tool(search::Search::new(root.clone(), operation));
    }
    if matches!(options.mode, ToolMode::Workspace | ToolMode::Trusted) {
        for operation in [Operation::Write, Operation::Edit] {
            names.push(operation.name().into());
            builder = builder.tool(Files {
                root: root.clone(),
                operation,
            });
        }
    }
    // A trusted ceiling is not permission to initialize an excluded shell.
    // A selected read-only subset must run without requiring any bash path.
    if options.mode == ToolMode::Trusted
        && (options.selected_tools.is_empty()
            || options.selected_tools.iter().any(|name| name == "bash"))
    {
        #[cfg(unix)]
        {
            let bash=options.bash.clone().ok_or_else(||Error::Config("trusted mode needs an absolute --bash path (Nix builds supply a store path; otherwise use --bash or AW_BASH)".into()))?;
            if !bash.is_absolute() || !bash.is_file() {
                return Err(Error::Config(
                    "bash must name an existing absolute executable path".into(),
                ));
            }
            names.push("bash".into());
            builder = builder.tool(shell::Bash::new(options.root.canonicalize()?, bash)?);
        }
        #[cfg(windows)]
        {
            return Err(Error::Config(
                "trusted shell mode is not enabled on Windows yet; use read-only/workspace tools"
                    .into(),
            ));
        }
    }
    if !options.selected_tools.is_empty() {
        let selected = options
            .selected_tools
            .iter()
            .collect::<std::collections::BTreeSet<_>>();
        if selected.iter().any(|name| !names.contains(name)) {
            return Err(Error::Denied(
                "selected tool is unavailable under the current tool mode",
            ));
        }
        names.retain(|name| selected.contains(name));
    }
    let mut policy = ToolPolicy {
        execution_site: options.execution_site.clone(),
        ..ToolPolicy::default()
    };
    // A host-selected tool still needs an exact source/actor/site/effect grant.
    // The registry caches the same trusted-host ToolSpec at registration; tool
    // modes choose the subset and effect ceiling, never authorization by name.
    for name in names {
        let effect = match name.as_str() {
            "read" | "ls" | "find" | "grep" => ToolEffect::Read,
            "write" | "edit" => ToolEffect::Write,
            "bash" => ToolEffect::Process,
            _ => return Err(Error::Denied("unrecognized trusted Host tool")),
        };
        policy.permit_host(actor.clone(), &name, &effect)?;
    }
    Ok(builder.tool_policy(policy))
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    Read,
    Write,
    Edit,
}
impl Operation {
    fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Edit => "edit",
        }
    }
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    path: String,
    /// Scheduling only: true requests overlap with adjacent parallel reads.
    #[serde(default, rename = "parallel")]
    _parallel: bool,
    #[serde(default = "one")]
    start_line: usize,
    #[serde(default = "two_hundred")]
    max_lines: usize,
}
fn one() -> usize {
    1
}
fn two_hundred() -> usize {
    200
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteArgs {
    path: String,
    content: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    path: String,
    old_text: String,
    new_text: String,
}
fn parse<T: serde::de::DeserializeOwned>(v: &Value) -> Result<T> {
    serde_json::from_value(v.clone())
        .map_err(|_| Error::Denied("tool arguments have unknown/missing/wrongly typed fields"))
}
struct Files {
    root: Arc<aw_platform::scoped_fs::Root>,
    operation: Operation,
}
impl Tool for Files {
    fn spec(&self) -> ToolSpec {
        let (description,parameters,effect)=match self.operation {
            Operation::Read=>("Read a bounded UTF-8 regular file below the granted root; lines are 1-indexed. Symlinks, traversal and multiply-linked files are refused. Use parallel=true when this read is independent and can overlap with adjacent opted-in reads.",json!({"type":"object","properties":{"path":{"type":"string"},"start_line":{"type":"integer","minimum":1},"max_lines":{"type":"integer","minimum":1,"maximum":1000},"parallel":{"type":"boolean","description":"Model scheduling choice: true allows overlap with adjacent independent reads; omit/false for ordered reads."}},"required":["path"],"additionalProperties":false}),ToolEffect::Read),
            Operation::Write=>("Create a NEW UTF-8 file below the root. Refuses overwriting; use edit with an exact old_text for existing files. Parent directories must already exist.",json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false}),ToolEffect::Write),
            Operation::Edit=>("Replace one unique exact nonempty old_text in an existing UTF-8 regular file. Fails for zero/multiple matches or observed concurrent changes. No fuzzy patching.",json!({"type":"object","properties":{"path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"}},"required":["path","old_text","new_text"],"additionalProperties":false}),ToolEffect::Write),
        };
        ToolSpec {
            name: self.operation.name().into(),
            description: description.into(),
            parameters,
            effect,
        }
    }
    fn validate(&self, v: &Value) -> Result<()> {
        match self.operation {
            Operation::Read => {
                let a: ReadArgs = parse(v)?;
                platform_fs(aw_platform::scoped_fs::validate_path(&a.path))?;
                if a.start_line == 0 || a.max_lines == 0 || a.max_lines > 1000 {
                    return Err(Error::Denied("invalid read line range"));
                }
            }
            Operation::Write => {
                let a: WriteArgs = parse(v)?;
                platform_fs(aw_platform::scoped_fs::validate_path(&a.path))?;
                if a.content.len() > 65536 {
                    return Err(Error::Limit("write content exceeds 64 KiB"));
                }
            }
            Operation::Edit => {
                let a: EditArgs = parse(v)?;
                platform_fs(aw_platform::scoped_fs::validate_path(&a.path))?;
                if a.old_text.is_empty() || a.old_text.len() + a.new_text.len() > 65536 {
                    return Err(Error::Denied(
                        "edit old_text must be nonempty and total replacement data bounded",
                    ));
                }
            }
        };
        Ok(())
    }
    fn execute<'a>(&'a self, ctx: ToolContext, v: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(v)?;
            let args = v.clone();
            let root = self.root.clone();
            let operation = self.operation;
            // Bounded file work on a blocking pool. Drop cannot preempt a kernel
            // write already committed: cancellation is checked before publication,
            // and core records uncertain effects instead of replaying them.
            let result = tokio::task::spawn_blocking(move || -> Result<ToolOutput> {
                let _serial = platform_fs(root.lock())?;
                if ctx.cancellation.is_cancelled() {
                    return Err(Error::Cancelled);
                }
                match operation {
                    Operation::Read => {
                        let a: ReadArgs = parse(&args)?;
                        let text = platform_fs(root.read(&a.path))?;
                        let mut output = String::new();
                        let mut truncated = false;
                        for (seen, (i, line)) in
                            text.lines().enumerate().skip(a.start_line - 1).enumerate()
                        {
                            if seen >= a.max_lines {
                                truncated = true;
                                break;
                            }
                            let row = format!("{}: {}\n", i + 1, line);
                            if output.len() + row.len() > 64 * 1024 {
                                truncated = true;
                                break;
                            }
                            output.push_str(&row);
                        }
                        Ok(ToolOutput {
                            content: output,
                            is_error: false,
                            truncated,
                        })
                    }
                    Operation::Write => {
                        let a: WriteArgs = parse(&args)?;
                        platform_fs(
                            root.create(&a.path, &a.content, &|| ctx.cancellation.is_cancelled()),
                        )?;
                        Ok(ToolOutput::text(format!(
                            "Created {} ({} bytes).",
                            a.path,
                            a.content.len()
                        )))
                    }
                    Operation::Edit => {
                        let a: EditArgs = parse(&args)?;
                        platform_fs(root.edit(&a.path, &a.old_text, &a.new_text, &|| {
                            ctx.cancellation.is_cancelled()
                        }))?;
                        Ok(ToolOutput::text(format!(
                            "Replaced one exact match in {}.",
                            a.path
                        )))
                    }
                }
            })
            .await
            .map_err(|_| Error::Protocol("file worker failed; inspect before retrying writes"))?;
            match result {
                Err(Error::Cancelled)=>Err(Error::Cancelled),
                Err(Error::Io(_)) if operation!=Operation::Read=>Err(Error::Protocol("filesystem mutation failed during I/O; partial effects are possible, inspect before continuing")),
                Err(Error::Io(_))=>Ok(ToolOutput::error("File read failed; inspect path/permissions. Raw host error is withheld.")),
                Err(Error::Denied(reason))|Err(Error::Limit(reason))=>Ok(ToolOutput::error(reason)),
                other=>other,
            }
        })
    }
}
// Keep Path used on all target branches without a blanket dead-code allowance.
pub fn validate_root(path: &Path) -> Result<PathBuf> {
    let path = path.canonicalize()?;
    if !path.is_dir() {
        return Err(Error::Config("tool root must be a directory".into()));
    }
    Ok(path)
}

/// Explicit human read-only browsing; never added to model context implicitly.
pub mod browse;
