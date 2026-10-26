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
mod patch;
mod search;
mod text;

pub mod browse;
pub mod media_port;
pub mod process;
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

/// Translate a bounded, known user error to an ordinary tool result. An
/// interrupted/uncertain mutator remains a journal-visible execution failure.
fn tool_failure(outcome: Result<ToolOutput>, mutating: bool) -> Result<ToolOutput> {
    match outcome {
        Err(Error::Denied(reason)) | Err(Error::Limit(reason)) => Ok(ToolOutput::error(reason)),
        Err(Error::Io(_)) if mutating => Err(Error::Protocol(
            "filesystem mutation failed; inspect workspace before retrying",
        )),
        Err(Error::Io(_)) => Ok(ToolOutput::error("file unavailable under the granted root")),
        other => other,
    }
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
pub fn configure(
    mut builder: AgentBuilder,
    actor: PrincipalId,
    options: &LocalToolOptions,
) -> Result<AgentBuilder> {
    if options.mode == ToolMode::Off {
        if !options.selected_tools.is_empty() {
            return Err(Error::Denied("tool selection needs an enabled mode"));
        }
        return Ok(builder);
    }
    let root = Arc::new(platform_fs(aw_platform::scoped_fs::Root::open(
        &options.root,
    ))?);
    let selected = options
        .selected_tools
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut available = BTreeSet::new();
    let mut policy = ToolPolicy {
        execution_site: options.execution_site.clone(),
        ..ToolPolicy::default()
    };
    builder = offer(
        builder,
        text::Text::new(root.clone(), text::Operation::Read),
        &selected,
        &mut available,
        &mut policy,
        &actor,
    )?;
    for operation in [search::Operation::Ls, search::Operation::Grep] {
        builder = offer(
            builder,
            search::Search::new(root.clone(), operation),
            &selected,
            &mut available,
            &mut policy,
            &actor,
        )?;
    }
    builder = offer(
        builder,
        binary::Binary::new(root.clone(), binary::Operation::Read),
        &selected,
        &mut available,
        &mut policy,
        &actor,
    )?;
    if matches!(options.mode, ToolMode::Workspace | ToolMode::Trusted) {
        builder = offer(
            builder,
            text::Text::new(root.clone(), text::Operation::Write),
            &selected,
            &mut available,
            &mut policy,
            &actor,
        )?;
        builder = offer(
            builder,
            edit::Edit::new(root.clone()),
            &selected,
            &mut available,
            &mut policy,
            &actor,
        )?;
        builder = offer(
            builder,
            patch::Patch::new(root.clone()),
            &selected,
            &mut available,
            &mut policy,
            &actor,
        )?;
        for operation in [binary::Operation::Write, binary::Operation::Edit] {
            builder = offer(
                builder,
                binary::Binary::new(root.clone(), operation),
                &selected,
                &mut available,
                &mut policy,
                &actor,
            )?;
        }
    }
    if options.mode == ToolMode::Trusted {
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
                builder = offer(
                    builder,
                    shell::Bash::new(options.root.canonicalize()?, bash)?,
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
            if selected.is_empty() || selected.contains("bash") {
                return Err(Error::Config(
                    "trusted shell is not enabled on Windows; select read/file tools".into(),
                ));
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
