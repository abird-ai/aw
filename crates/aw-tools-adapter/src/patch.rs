//! Dotlink's Codex patch grammar atop AW's scoped staged filesystem.
use crate::{platform_fs, tool_failure};
use anyhow::{bail, Result};
use aw_agent_core::{self as agent, Error, Tool, ToolContext, ToolFuture, ToolOutput, ToolSpec};
use aw_platform::scoped_fs::Root;
use aw_tools::{
    mutation::{MutationReceipt, PlanBudget},
    patch::{apply_patch_hunks, parse_patch, PatchOp},
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeSet, sync::Arc};
const MAX_FILE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchArgs {
    #[serde(default = "dot")]
    dir: String,
    patch: String,
    #[serde(default)]
    best_effort: bool,
}
fn dot() -> String {
    ".".into()
}
pub(crate) struct Patch {
    root: Arc<Root>,
    limits: crate::LocalToolLimits,
}
impl Patch {
    #[cfg(test)]
    pub(crate) fn new(root: Arc<Root>) -> Self {
        Self::with_limits(root, crate::LocalToolLimits::default())
    }
    pub(crate) fn with_limits(root: Arc<Root>, limits: crate::LocalToolLimits) -> Self {
        Self { root, limits }
    }
}

/// Patch paths are relative to the patch's `dir`, not to the launch directory.
fn join_patch_path(dir: &str, path: &str) -> agent::Result<String> {
    aw_tools::filesystem::path::join(dir, path)
        .map_err(|_| Error::Denied("invalid canonical patch path"))
}

struct Plan {
    path: String,
    expected: Option<Vec<u8>>,
    replacement: Vec<u8>,
    hunks: usize,
    added: usize,
    removed: usize,
}
fn validate_patch_path(value: &str) -> agent::Result<()> {
    crate::model_path(value)
}
impl Tool for Patch {
    fn spec(&self) -> ToolSpec {
        use aw_tools::filesystem::schema::Tool as C;
        crate::core_spec(C::Patch, self.limits.schema)
    }
    fn validate(&self, value: &Value) -> agent::Result<()> {
        let args: PatchArgs = serde_json::from_value(value.clone())
            .map_err(|_| Error::Denied("patch expects dir?, patch and best_effort?"))?;
        if args.patch.len() > self.limits.schema.patch_bytes {
            return Err(Error::Limit("patch text exceeds configured tool ceiling"));
        }
        crate::text_class::require_text(args.patch.as_bytes())?;
        if args.dir != "." {
            validate_patch_path(&args.dir)?;
        }
        Ok(())
    }
    fn execute<'a>(&'a self, ctx: ToolContext, value: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(value)?;
            let args: PatchArgs = serde_json::from_value(value.clone())
                .map_err(|_| Error::Denied("invalid patch args"))?;
            let result_bytes = ctx.result_budget(self.limits.result_bytes)?;
            if args.dir == "." && !args.best_effort {
                let root = self.root.clone();
                let patch = args.patch.clone();
                let answer =
                    aw_tools::asynchronous::run_blocking(move || -> agent::Result<ToolOutput> {
                        use aw_tools::filesystem::operations::{self, Error as OperationError};
                        match operations::patch_text(
                            root.as_ref(),
                            &patch,
                            MAX_FILE_BYTES,
                            MAX_FILE_BYTES,
                            &|| ctx.cancellation.is_cancelled(),
                        ) {
                            Ok(primary) => Ok(ToolOutput::text(primary.as_str())),
                            Err(OperationError::Patch(message)) => Ok(ToolOutput::error(message)),
                            Err(error) => Err(crate::file_operation_error(error)),
                        }
                    })
                    .await
                    .map_err(|_| Error::Protocol("patch worker failed; inspect workspace"))?;
                return tool_failure(answer, true);
            }
            let root = self.root.clone();
            let answer = aw_tools::asynchronous::run_blocking(move || -> agent::Result<ToolOutput> {
                let ops = match parse_patch(&args.patch) {
                    Ok(ops) => ops,
                    Err(e) => return Ok(ToolOutput::error(e.to_string())),
                };
                let mut paths = Vec::new();
                let mut eligible = Vec::new();
                let mut claimed = BTreeSet::new();
                let mut failures = Vec::<Value>::new();
                let total_hunks = ops
                    .iter()
                    .map(|op| match op {
                        PatchOp::Update { hunks, .. } => hunks.len(),
                        PatchOp::Add { .. } => 0,
                    })
                    .sum::<usize>();
                let mut next_hunk = 0usize;
                for op in ops {
                    let relative = match &op {
                        PatchOp::Update { path, .. } | PatchOp::Add { path, .. } => path,
                    };
                    let first_hunk = next_hunk;
                    if let PatchOp::Update { hunks, .. } = &op {
                        next_hunk += hunks.len();
                    }
                    let name = match join_patch_path(&args.dir, relative) {
                        Ok(name) => name,
                        Err(error) if args.best_effort => {
                            failures.push(json!({"path":relative,"error":format!("{error}")}));
                            continue;
                        }
                        Err(error) => {
                            return Ok(ToolOutput::error(format!("patch path refused: {error}")));
                        }
                    };
                    if let Err(error) = validate_patch_path(&name) {
                        if !args.best_effort {
                            return Ok(ToolOutput::error(format!("patch path refused: {error}")));
                        }
                        failures.push(json!({"path":name,"error":format!("{error}")}));
                        continue;
                    }
                    let effective = match &op {
                        PatchOp::Update { .. } => match crate::content_path(&root, &name, false) {
                            Ok(path) => path,
                            Err(error) if args.best_effort => {
                                failures.push(json!({"path":name,"error":format!("{error}")}));
                                continue;
                            }
                            Err(error) => {
                                return Ok(ToolOutput::error(format!("patch path refused: {error}")));
                            }
                        },
                        PatchOp::Add { .. } => name.clone(),
                    };
                    let claim=match platform_fs(
                        aw_tools::filesystem::backend::SecuredFilesystem::mutation_path_key(
                            root.as_ref(),&effective
                        )
                    ) {
                        Ok(claim)=>claim,
                        Err(error) if args.best_effort => {
                            failures.push(json!({"path":name,"error":format!("{error}")}));
                            continue;
                        }
                        Err(error)=>return Err(error),
                    };
                    if !claimed.insert(claim) {
                        if !args.best_effort {
                            return Ok(ToolOutput::error(format!(
                                "patch path appears in more than one section: {name}"
                            )));
                        }
                        failures.push(json!({"path":name,"error":"duplicate patch path"}));
                        continue;
                    }
                    paths.push(effective.clone());
                    eligible.push((op, effective, first_hunk));
                }
                let _guard = platform_fs(
                    root.lock_mutations_with_cancel(&paths, &|| ctx.cancellation.is_cancelled()),
                )?;
                let mut plans = Vec::<Plan>::new();
                let mut work=PlanBudget::default();
                for (op, path, first_hunk) in eligible {
                    if ctx.cancellation.is_cancelled() {
                        return Err(Error::Cancelled);
                    }
                    let res = (|| -> Result<Plan> {
                        match op {
                            PatchOp::Update { hunks, .. } => {
                                let cap=work.copy_read_limit(MAX_FILE_BYTES)?;
                                let original=root.read_bytes(&path,cap).map_err(|error|
                                    anyhow::anyhow!("cannot read update target within aggregate plan budget: {error:?}")
                                )?;
                                let text =
                                    crate::text_class::text_bytes(&original).ok_or_else(|| {
                                        anyhow::anyhow!("update target is binary; use bedit")
                                    })?;
                                let (changed, added, removed) = apply_patch_hunks(
                                    text,
                                    &hunks,
                                    &path,
                                    first_hunk,
                                    total_hunks,
                                )?;
                                if changed.len() > MAX_FILE_BYTES {
                                    bail!("patch replacement exceeds 1 MiB");
                                }
                                crate::text_class::require_text(changed.as_bytes())
                                    .map_err(|_| anyhow::anyhow!("patch replacement is binary"))?;
                                work.add(original.len(),changed.len())?;
                                Ok(Plan {
                                    path: path.clone(),
                                    expected: Some(original),
                                    replacement: changed.into_bytes(),
                                    hunks: hunks.len(),
                                    added,
                                    removed,
                                })
                            }
                            PatchOp::Add { lines, .. } => {
                                let bytes = aw_tools::patch::add_file_bytes(&lines);
                                if bytes.len() > MAX_FILE_BYTES {
                                    bail!("patch addition exceeds 1 MiB");
                                }
                                crate::text_class::require_text(&bytes)
                                    .map_err(|_| anyhow::anyhow!("patch addition is binary"))?;
                                work.add(0,bytes.len())?;
                                Ok(Plan {
                                    path: path.clone(),
                                    expected: None,
                                    replacement: bytes,
                                    hunks: 0,
                                    added: lines.len(),
                                    removed: 0,
                                })
                            }
                        }
                    })();
                    if let Err(e) = &res {
                        if !args.best_effort {
                            return Ok(ToolOutput::error(format!("patch: {}: {e}", path)));
                        }
                        failures.push(json!({"path":path,"error":e.to_string()}));
                    } else {
                        plans.push(res.expect("checked"));
                    }
                }
                // Preflight the smallest truthful receipt before any effect.
                // AW's Agent result budget may be smaller, but never reports
                // malformed post-commit JSON.
                let Value::Object(meta)=json!({
                    "mode":if args.best_effort{"best_effort"}else{"strict"},
                    "changed_files":plans.len(),
                }) else {unreachable!()};
                let receipt_budget=MutationReceipt::new(result_bytes,meta)
                    .map_err(|_|Error::Limit("patch receipt metadata exceeds tool budget"))?;
                // Every strict-mode plan must stage successfully before the
                // first commit. StagedWrite::Drop cleans any uncommitted temp.
                let mut staged = Vec::new();
                let mut receipts = Vec::new();
                for plan in plans {
                    if ctx.cancellation.is_cancelled() {
                        return Err(Error::Cancelled);
                    }
                    let action = match &plan.expected {
                        Some(original) if original == &plan.replacement => None,
                        Some(original) => Some(root.stage_update(
                            &plan.path,
                            original,
                            &plan.replacement,
                            &|| ctx.cancellation.is_cancelled(),
                        )),
                        None => Some(root.stage_create(&plan.path, &plan.replacement, &|| {
                            ctx.cancellation.is_cancelled()
                        })),
                    };
                    let receipt = json!({"path":plan.path,"hunks":plan.hunks,
                        "added":plan.added,"removed":plan.removed});
                    match action {
                        Some(Ok(file)) => staged.push((plan.path, file, receipt)),
                        Some(Err(error)) if args.best_effort => {
                            failures.push(json!({"path":plan.path,"error":format!("{error:?}")}))
                        }
                        Some(Err(error)) => {
                            while let Some((_, pending, _)) = staged.pop() {
                                drop(pending);
                            }
                            return Err(crate::platform_fs::<()>(Err(error)).unwrap_err());
                        }
                        None => {} // unchanged file; no receipt or metadata churn
                    }
                }
                let mut committed = 0usize;
                for (path, file, receipt) in staged {
                    if ctx.cancellation.is_cancelled() {
                        return Err(Error::Protocol(
                            "patch interrupted during multi-file commit; inspect workspace",
                        ));
                    }
                    if let Err(error) = platform_fs(file.commit()) {
                        if !args.best_effort {
                            return if committed == 0 {
                                Err(error)
                            } else {
                                Err(Error::Protocol(
                                    "patch partially committed; inspect workspace",
                                ))
                            };
                        }
                        failures
                            .push(json!({"path":path,"error":"commit failed; inspect this file"}));
                        continue;
                    }
                    receipts.push(receipt);
                    committed += 1;
                }
                // The bounded full receipt is validated internally before
                // publication, while the model sees only actionable outcome.
                let _=receipt_budget.finish_with_count(&receipts,&failures);
                if failures.is_empty() {
                    Ok(ToolOutput::text(
                        aw_tools::mutation::compact_success(committed,None)
                    ))
                }else{
                    let mut notice=format!("mixed ok={committed} fail={}",failures.len());
                    for failure in failures.iter().take(8){
                        if let (Some(path),Some(reason))=
                            (failure["path"].as_str(),failure["error"].as_str()){
                            let line=format!("\n{}: {}",path,reason);
                            if notice.len()+line.len()>result_bytes{break;}
                            notice.push_str(&line);
                        }
                    }
                    Ok(ToolOutput::error(notice))
                }
            })
            .await
            .map_err(|_| Error::Protocol("patch worker failed; inspect workspace"))?;
            tool_failure(answer, true)
        })
    }
}
