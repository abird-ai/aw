//! Exact-path file lifecycle tools. AW owns journal/ToolPolicy; the platform
//! root owns pinned authority, mutation locks and no-replace publication.
use crate::{platform_fs, tool_failure};
use aw_agent_core::{Error, Result, Tool, ToolContext, ToolFuture, ToolOutput, ToolSpec};
use aw_platform::scoped_fs::Root;
use aw_tools_core::filesystem::lifecycle as life;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(crate) enum Operation {
    Copy,
    Move,
    Remove,
}

pub(crate) struct Lifecycle {
    root: Arc<Root>,
    op: Operation,
}
impl Lifecycle {
    pub(crate) fn new(root: Arc<Root>, op: Operation) -> Self {
        Self { root, op }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cp {
    from: String,
    to: String,
    #[serde(default)]
    recursive: Option<bool>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mv {
    from: String,
    to: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rm {
    paths: Vec<String>,
    #[serde(default)]
    recursive: bool,
}
fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|_| Error::Denied("invalid/unknown lifecycle arguments"))
}
fn valid(path: &str) -> Result<()> {
    life::exact_path(path).map_err(|_| Error::Denied("expected exact clean relative path"))?;
    platform_fs(aw_platform::scoped_fs::validate_path(path))
}
impl Tool for Lifecycle {
    fn spec(&self) -> ToolSpec {
        use aw_tools_core::filesystem::schema::{SchemaCaps, Tool as C};
        let tool = match self.op {
            Operation::Copy => C::Copy,
            Operation::Move => C::Move,
            Operation::Remove => C::Remove,
        };
        crate::core_spec(tool, SchemaCaps::default())
    }
    fn validate(&self, value: &Value) -> Result<()> {
        match self.op {
            Operation::Copy => {
                let a: Cp = parse(value)?;
                valid(&a.from)?;
                valid(&a.to)?;
                life::pair(&a.from, &a.to, true)
                    .map_err(|_| Error::Denied("invalid overlapping copy paths"))?;
            }
            Operation::Move => {
                let a: Mv = parse(value)?;
                valid(&a.from)?;
                valid(&a.to)?;
                if a.from != a.to {
                    life::pair(&a.from, &a.to, true)
                        .map_err(|_| Error::Denied("move destination within source"))?;
                }
            }
            Operation::Remove => {
                let a: Rm = parse(value)?;
                for path in &a.paths {
                    valid(path)?;
                }
                life::removals(a.paths.iter().map(String::as_str))
                    .map_err(|_| Error::Denied("invalid/overlapping removal paths"))?;
            }
        }
        Ok(())
    }
    fn execute<'a>(&'a self, ctx: ToolContext, input: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(input)?;
            let value = input.clone();
            let root = self.root.clone();
            let op = self.op;
            let token = ctx.cancellation.clone();
            let answer =
                aw_tools_core::asynchronous::run_blocking(move || -> Result<ToolOutput> {
                    if token.is_cancelled() {
                        return Err(Error::Cancelled);
                    }
                    let cancelled = || token.is_cancelled();
                    use aw_tools_core::filesystem::operations as shared;
                    match op {
                        Operation::Copy => {
                            let args: Cp = parse(&value)?;
                            let status = shared::copy_path(
                                root.as_ref(),
                                &args.from,
                                &args.to,
                                args.recursive.unwrap_or(true),
                                &cancelled,
                            )
                            .map_err(crate::file_operation_error)?;
                            Ok(ToolOutput::text(status.as_str()))
                        }
                        Operation::Move => {
                            let args: Mv = parse(&value)?;
                            let status =
                                shared::move_path(root.as_ref(), &args.from, &args.to, &cancelled)
                                    .map_err(crate::file_operation_error)?;
                            Ok(ToolOutput::text(status.as_str()))
                        }
                        Operation::Remove => {
                            let args: Rm = parse(&value)?;
                            let outcome = shared::remove_paths(
                                root.as_ref(),
                                &args.paths,
                                args.recursive,
                                &cancelled,
                            )
                            .map_err(crate::file_operation_error)?;
                            Ok(ToolOutput {
                                content: outcome.compact(),
                                is_error: outcome.error(),
                                truncated: false,
                            })
                        }
                    }
                })
                .await
                .map_err(|_| {
                    Error::Protocol("filesystem lifecycle worker failed; inspect workspace")
                })?;
            tool_failure(answer, true)
        })
    }
}
