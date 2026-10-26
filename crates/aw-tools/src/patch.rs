//! Dotlink's Codex patch grammar atop AW's scoped staged filesystem.
use crate::{platform_fs, tool_failure};
use anyhow::{bail, Result};
use aw_agent_core::{
    self as agent, Error, Tool, ToolContext, ToolEffect, ToolFuture, ToolOutput, ToolSpec,
};
use aw_platform::scoped_fs::Root;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::Path, sync::Arc};
const MAX_PATCH_FILES: usize = 32;
const MAX_PATCH_HUNKS: usize = 64;
const MAX_AMBIGUOUS_LINES: usize = 10;
const MAX_PATCH_BYTES: usize = 48 * 1024;
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
}
impl Patch {
    pub(crate) fn new(root: Arc<Root>) -> Self {
        Self { root }
    }
}

#[derive(Debug)]
enum PatchOp {
    Update { path: String, hunks: Vec<PatchHunk> },
    Add { path: String, lines: Vec<String> },
}

#[derive(Debug)]
struct PatchHunk {
    anchor: Option<String>,
    lines: Vec<PatchLine>,
    /// The Codex `*** End of File` marker: this hunk must end at the last line.
    at_end_of_file: bool,
}

#[derive(Debug)]
enum PatchLine {
    Context(String),
    Removed(String),
    Added(String),
}

/// Patch lines, split on `\n` only: a CRLF patch keeps its `\r`, and
/// `apply_patch_hunks` normalizes added lines to the target file's terminator.
fn patch_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split_inclusive('\n')
        .map(|line| line.strip_suffix('\n').unwrap_or(line))
}

fn parse_patch(text: &str) -> Result<Vec<PatchOp>> {
    let mut ops: Vec<PatchOp> = Vec::new();
    let mut saw_begin = false;
    let mut saw_end = false;
    let mut hunks = 0_usize;

    for (index, line) in patch_lines(text).enumerate() {
        let number = index + 1;
        if !saw_begin {
            if line.trim().is_empty() {
                continue;
            }
            if line.trim_end() != "*** Begin Patch" {
                bail!("patch: missing '*** Begin Patch' header");
            }
            saw_begin = true;
            continue;
        }
        if saw_end {
            if !line.trim().is_empty() {
                bail!("patch: line {number}: unexpected text after '*** End Patch'");
            }
            continue;
        }

        if let Some(directive) = line.strip_prefix("*** ") {
            let directive = directive.trim_end();
            if directive == "End Patch" {
                saw_end = true;
                continue;
            }
            if directive == "End of File" {
                match ops.last_mut() {
                    Some(PatchOp::Update { hunks, .. }) => match hunks.last_mut() {
                        Some(hunk) => hunk.at_end_of_file = true,
                        None => bail!("patch: line {number}: '*** End of File' needs a hunk"),
                    },
                    _ => {
                        bail!("patch: line {number}: '*** End of File' is only valid in an update")
                    }
                }
                continue;
            }
            if let Some(path) = directive.strip_prefix("Update File:") {
                let path = path.trim().to_owned();
                if path.is_empty() {
                    bail!("patch: line {number}: '*** Update File' needs a path");
                }
                ops.push(PatchOp::Update {
                    path,
                    hunks: Vec::new(),
                });
                continue;
            }
            if let Some(path) = directive.strip_prefix("Add File:") {
                let path = path.trim().to_owned();
                if path.is_empty() {
                    bail!("patch: line {number}: '*** Add File' needs a path");
                }
                ops.push(PatchOp::Add {
                    path,
                    lines: Vec::new(),
                });
                continue;
            }
            let name = directive.split(':').next().unwrap_or(directive);
            bail!("patch: line {number}: unsupported directive '*** {name}'");
        }

        let Some(current) = ops.last_mut() else {
            bail!("patch: line {number}: text outside a file section");
        };

        if let Some(rest) = line.strip_prefix("@@") {
            let PatchOp::Update { hunks: list, .. } = current else {
                bail!("patch: line {number}: '*** Add File' does not take hunks");
            };
            if list.last().is_some_and(|hunk| hunk.at_end_of_file) {
                bail!(
                    "patch: line {number}: no hunk may follow '*** End of File' in the same file"
                );
            }
            hunks += 1;
            if hunks > MAX_PATCH_HUNKS {
                bail!("patch: {hunks} hunks, above the {MAX_PATCH_HUNKS} hunk limit");
            }
            list.push(PatchHunk {
                anchor: parse_patch_anchor(rest),
                lines: Vec::new(),
                at_end_of_file: false,
            });
            continue;
        }

        match current {
            PatchOp::Update { hunks: list, .. } => {
                let Some(hunk) = list.last_mut() else {
                    bail!("patch: line {number}: text outside a hunk");
                };
                match line.as_bytes().first() {
                    Some(b' ') => hunk.lines.push(PatchLine::Context(line[1..].to_owned())),
                    Some(b'-') => hunk.lines.push(PatchLine::Removed(line[1..].to_owned())),
                    Some(b'+') => hunk.lines.push(PatchLine::Added(line[1..].to_owned())),
                    _ if line.trim().is_empty() => {
                        hunk.lines.push(PatchLine::Context(String::new()))
                    }
                    _ => {
                        bail!("patch: line {number}: unexpected text '{line}'");
                    }
                }
            }
            PatchOp::Add { lines, .. } => match line.as_bytes().first() {
                Some(b'+') => lines.push(line[1..].to_owned()),
                _ if line.trim().is_empty() => lines.push(String::new()),
                _ => bail!("patch: line {number}: unexpected text '{line}'"),
            },
        }
    }

    if !saw_begin {
        bail!("patch: missing '*** Begin Patch' header");
    }
    if !saw_end {
        bail!("patch: missing '*** End Patch' terminator");
    }
    if ops.len() > MAX_PATCH_FILES {
        bail!(
            "patch: {} files, above the {MAX_PATCH_FILES} file limit",
            ops.len()
        );
    }
    for op in &ops {
        if let PatchOp::Update { path, hunks } = op {
            if hunks.is_empty() {
                bail!("patch: '*** Update File' for {path} has no hunks");
            }
        }
    }
    Ok(ops)
}

/// `@@` may carry anchor text or unified-diff line numbers. Numbers are
/// accepted and ignored; anchor text narrows where the search starts.
/// `@@` content is either anchor text or unified-diff line numbers. Both may
/// appear (`@@ -5,1 +5,1 @@ fn b()`), so leading `-`/`+` number tokens are
/// dropped and whatever follows them narrows the search.
fn parse_patch_anchor(rest: &str) -> Option<String> {
    let mut rest = rest.trim();
    loop {
        // `@@ -3,1 +3,1 @@ fn b()`: drop the range tokens, then the closing `@@`.
        let stripped = rest
            .strip_prefix('-')
            .or_else(|| rest.strip_prefix('+'))
            .map(str::trim_start)
            .filter(|stripped| stripped.starts_with(|c: char| c.is_ascii_digit()))
            .map(|stripped| {
                stripped
                    .trim_start_matches(|c: char| c.is_ascii_digit() || c == ',')
                    .trim_start()
            })
            .or_else(|| rest.strip_prefix("@@").map(str::trim_start));
        match stripped {
            Some(after) if after.len() < rest.len() => rest = after,
            _ => break,
        }
    }
    match rest.is_empty() {
        true => None,
        false => Some(rest.to_owned()),
    }
}

/// Hunk location is content-based: exact match first, then a per-line
/// `trim_end` comparison. Ambiguity is an error, and hunks within a file apply
/// top-down without re-scanning above the previous match.
fn apply_patch_hunks(
    text: &str,
    hunks: &[PatchHunk],
    display: &str,
    first_hunk: usize,
    total_hunks: usize,
) -> Result<(String, usize, usize)> {
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    let mut from_line = 0_usize;
    let mut added = 0_usize;
    let mut removed = 0_usize;

    for (offset, hunk) in hunks.iter().enumerate() {
        let position = first_hunk + offset + 1;
        let block = hunk
            .lines
            .iter()
            .filter_map(|line| match line {
                PatchLine::Context(text) => Some(text.as_str()),
                PatchLine::Removed(text) => Some(text.as_str()),
                PatchLine::Added(_) => None,
            })
            .collect::<Vec<_>>();
        let mut search_start = from_line;
        if let Some(anchor) = hunk.anchor.as_deref() {
            if let Some(at) = lines
                .iter()
                .enumerate()
                .skip(search_start)
                .find(|(_, line)| line.contains(anchor))
                .map(|(index, _)| index)
            {
                search_start = at;
            }
        }
        let near = search_start + 1;

        if block.is_empty() {
            bail!("hunk {position}/{total_hunks} ({display}): context not found near line {near}");
        }

        let mut exact = Vec::new();
        let mut lenient = Vec::new();
        for start in search_start..lines.len() {
            if start + block.len() > lines.len() {
                break;
            }
            if lines[start..start + block.len()]
                .iter()
                .zip(&block)
                .all(|(file, patch)| file == patch)
            {
                exact.push(start);
                continue;
            }
            if lines[start..start + block.len()]
                .iter()
                .zip(&block)
                .all(|(file, patch)| file.trim_end() == patch.trim_end())
            {
                lenient.push(start);
            }
        }
        let matches = if exact.is_empty() { lenient } else { exact };
        let Some(&start) = matches.first() else {
            bail!("hunk {position}/{total_hunks} ({display}): context not found near line {near}");
        };
        if matches.len() > 1 {
            let lines_listed = matches
                .iter()
                .take(MAX_AMBIGUOUS_LINES)
                .map(|line| (line + 1).to_string())
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "hunk {position}/{total_hunks} ({display}): context matches {} locations (lines {lines_listed})",
                matches.len()
            );
        }

        if hunk.at_end_of_file {
            // `text.split('\n')` yields one empty tail for a trailing newline;
            // it is not a line the marker can refer to.
            let file_lines = lines.len() - usize::from(lines.last().is_some_and(String::is_empty));
            if start + block.len() != file_lines {
                bail!(
                    "hunk {position}/{total_hunks} ({display}): '*** End of File' requires the hunk to end at the end of the file (line {file_lines})"
                );
            }
        }

        let mut replacement = Vec::with_capacity(hunk.lines.len());
        // Added lines inherit the terminator of the file they land in. The
        // patch itself often carries LF because numbered `read` strips `\r`,
        // so preserving only the patch's bytes would produce mixed endings in
        // a CRLF file.
        let file_crlf = lines.get(start).is_some_and(|line| line.ends_with('\r'));
        // Cursor into the consumed block: only context/removed lines consume
        // existing file lines.
        let mut consumed_at = 0_usize;
        for line in &hunk.lines {
            match line {
                // Keep the file's own bytes for context lines, so a lenient
                // match never rewrites trailing whitespace it did not touch.
                PatchLine::Context(_) => {
                    replacement.push(lines[start + consumed_at].clone());
                    consumed_at += 1;
                }
                PatchLine::Removed(_) => {
                    removed += 1;
                    consumed_at += 1;
                }
                PatchLine::Added(text) => {
                    added += 1;
                    let mut line = text.clone();
                    match file_crlf {
                        true if !line.ends_with('\r') => line.push('\r'),
                        false if line.ends_with('\r') => {
                            line.pop();
                        }
                        _ => {}
                    }
                    replacement.push(line);
                }
            }
        }
        let replacement_len = replacement.len();
        lines.splice(start..start + block.len(), replacement);
        from_line = start + replacement_len;
    }

    Ok((lines.join("\n"), added, removed))
}

/// Patch paths are relative to the patch's `dir`, not to the launch directory.
fn join_patch_path(dir: &str, path: &str) -> String {
    if Path::new(path).is_absolute() || dir == "." {
        return path.to_owned();
    }
    Path::new(dir).join(path).to_string_lossy().into_owned()
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
    platform_fs(aw_platform::scoped_fs::validate_path(value))
}
impl Tool for Patch {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "patch".into(),
            description: "Apply a Codex *** Begin Patch / *** End Patch text patch to UTF-8 files. Supports Update File hunks and Add File, not Delete or Move. Optional dir and best_effort; strict preflight plans every file before commit. Hunk matching is content-based with ambiguity errors and CRLF preservation. At most 32 files and 64 hunks, bounded by AW's tool argument limit. Existing updates use snapshots and atomic per-file installs; cross-file publication is not a transaction.".into(),
            parameters: json!({"type":"object","properties":{
                "dir":{"type":"string"},
                "patch":{"type":"string","maxLength":MAX_PATCH_BYTES},
                "best_effort":{"type":"boolean"}},"required":["patch"],"additionalProperties":false}),
            effect: ToolEffect::Write,
        }
    }
    fn validate(&self, value: &Value) -> agent::Result<()> {
        let args: PatchArgs = serde_json::from_value(value.clone())
            .map_err(|_| Error::Denied("patch expects dir?, patch and best_effort?"))?;
        if args.patch.len() > MAX_PATCH_BYTES {
            return Err(Error::Limit("patch text exceeds 48 KiB"));
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
            let root = self.root.clone();
            let answer = tokio::task::spawn_blocking(move || -> agent::Result<ToolOutput> {
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
                    let name = join_patch_path(&args.dir, relative);
                    if let Err(error) = validate_patch_path(&name) {
                        if !args.best_effort {
                            return Ok(ToolOutput::error(format!("patch path refused: {error}")));
                        }
                        failures.push(json!({"path":name,"error":format!("{error}")}));
                        continue;
                    }
                    let claim = if cfg!(windows) {
                        name.to_lowercase()
                    } else {
                        name.clone()
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
                    paths.push(name.clone());
                    eligible.push((op, name, first_hunk));
                }
                let _guard = platform_fs(
                    root.lock_mutations_with_cancel(&paths, &|| ctx.cancellation.is_cancelled()),
                )?;
                let mut plans = Vec::<Plan>::new();
                for (op, path, first_hunk) in eligible {
                    if ctx.cancellation.is_cancelled() {
                        return Err(Error::Cancelled);
                    }
                    let res = (|| -> Result<Plan> {
                        match op {
                            PatchOp::Update { hunks, .. } => {
                                let original =
                                    root.read_bytes(&path, MAX_FILE_BYTES).map_err(|_| {
                                        anyhow::anyhow!("cannot read existing update target")
                                    })?;
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
                                let text = if lines.is_empty() {
                                    String::new()
                                } else {
                                    format!("{}\n", lines.join("\n"))
                                };
                                if text.len() > MAX_FILE_BYTES {
                                    bail!("patch addition exceeds 1 MiB");
                                }
                                crate::text_class::require_text(text.as_bytes())
                                    .map_err(|_| anyhow::anyhow!("patch addition is binary"))?;
                                Ok(Plan {
                                    path: path.clone(),
                                    expected: None,
                                    replacement: text.into_bytes(),
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
                Ok(ToolOutput::text(
                    json!({
                        "files":receipts,
                        "mode":if args.best_effort{"best_effort"}else{"strict"},
                        "changed_files":committed,
                        "failures":failures,
                    })
                    .to_string(),
                ))
            })
            .await
            .map_err(|_| Error::Protocol("patch worker failed; inspect workspace"))?;
            tool_failure(answer, true)
        })
    }
}
