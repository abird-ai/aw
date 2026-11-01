//! Transport-neutral Codex patch grammar and content planning. Native
//! staging and authorization remain with the host. Source algorithms are
//! adapted from the dotlink code owned by this project.

use crate::matching::scan_sequence;
use anyhow::{bail, Result};
const MAX_PATCH_FILES: usize = 32;
const MAX_PATCH_HUNKS: usize = 64;
const MAX_AMBIGUOUS_LINES: usize = 10;
#[derive(Debug)]
pub enum PatchOp {
    Update {
        path: String,
        hunks: Vec<PatchHunk>,
    },
    Add {
        path: String,
        lines: Vec<String>,
        crlf: bool,
    },
}

#[derive(Debug)]
pub struct PatchHunk {
    anchor: Option<String>,
    lines: Vec<PatchLine>,
    /// The Codex `*** End of File` marker: this hunk must end at the last line.
    at_end_of_file: bool,
    patch_crlf: bool,
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

pub fn parse_patch(text: &str) -> Result<Vec<PatchOp>> {
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
                    crlf: line.ends_with('\r'),
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
                patch_crlf: line.ends_with('\r'),
            });
            continue;
        }

        match current {
            PatchOp::Update { hunks: list, .. } => {
                let Some(hunk) = list.last_mut() else {
                    bail!("patch: line {number}: text outside a hunk");
                };
                let body = if hunk.patch_crlf {
                    line.strip_suffix('\r').unwrap_or(line)
                } else {
                    line
                };
                match body.as_bytes().first() {
                    Some(b' ') => hunk.lines.push(PatchLine::Context(body[1..].to_owned())),
                    Some(b'-') => hunk.lines.push(PatchLine::Removed(body[1..].to_owned())),
                    Some(b'+') => hunk.lines.push(PatchLine::Added(body[1..].to_owned())),
                    _ if body.trim().is_empty() => {
                        hunk.lines.push(PatchLine::Context(String::new()))
                    }
                    _ => bail!("patch: line {number}: unexpected text '{body}'"),
                }
            }
            PatchOp::Add { lines, crlf, .. } => {
                let body = if *crlf {
                    line.strip_suffix('\r').unwrap_or(line)
                } else {
                    line
                };
                match body.as_bytes().first() {
                    Some(b'+') => lines.push(body[1..].to_owned()),
                    _ if body.trim().is_empty() => lines.push(String::new()),
                    _ => bail!("patch: line {number}: unexpected text '{body}'"),
                }
            }
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
pub fn apply_patch_hunks(
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

        // An explicit EOF-constrained addition-only hunk is a valid append;
        // without that anchor, an insertion with no context is ambiguous.
        let eof = lines.len() - usize::from(lines.last().is_some_and(String::is_empty));
        let start = if block.is_empty() {
            if !hunk.at_end_of_file || from_line > eof {
                bail!("hunk {position}/{total_hunks} ({display}): insertion requires EOF or context near line {near}");
            }
            eof
        } else {
            // Normalize only a CR that belongs to a line separator. A CR on
            // the final unterminated source line is literal content.
            let normalized = lines
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    if i + 1 < lines.len() {
                        line.strip_suffix('\r').unwrap_or(line)
                    } else {
                        line.as_str()
                    }
                })
                .collect::<Vec<_>>();
            let mut exact_count = 0usize;
            let mut approx_count = 0usize;
            let mut exact_starts = Vec::with_capacity(MAX_AMBIGUOUS_LINES);
            let mut approx_starts = Vec::with_capacity(MAX_AMBIGUOUS_LINES);
            let eligible = |at: usize| !hunk.at_end_of_file || at + block.len() == eof;
            scan_sequence(
                &block,
                &normalized,
                search_start,
                |a, b| a == b,
                |item, pattern| item == pattern,
                |at| {
                    if eligible(at) {
                        exact_count += 1;
                        if exact_starts.len() < MAX_AMBIGUOUS_LINES {
                            exact_starts.push(at);
                        }
                    }
                },
            );
            if exact_count == 0 {
                scan_sequence(
                    &block,
                    &normalized,
                    search_start,
                    |a, b| a.trim_end() == b.trim_end(),
                    |item, pattern| item.trim_end() == pattern.trim_end(),
                    |at| {
                        if eligible(at) {
                            approx_count += 1;
                            if approx_starts.len() < MAX_AMBIGUOUS_LINES {
                                approx_starts.push(at);
                            }
                        }
                    },
                );
            }
            let (count, positions) = if exact_count > 0 {
                (exact_count, &exact_starts)
            } else {
                (approx_count, &approx_starts)
            };
            let Some(&start) = positions.first() else {
                bail!(
                    "hunk {position}/{total_hunks} ({display}): context not found near line {near}"
                );
            };
            if count > 1 {
                let lines_listed = positions
                    .iter()
                    .map(|line| (line + 1).to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                bail!("hunk {position}/{total_hunks} ({display}): context matches {count} locations (lines {lines_listed})");
            }
            start
        };

        let mut replacement = Vec::with_capacity(hunk.lines.len());
        let mut consumed_at = 0usize;
        let mut prior_removed_crlf = None::<bool>;
        for (index, line) in hunk.lines.iter().enumerate() {
            match line {
                PatchLine::Context(_) => {
                    replacement.push(lines[start + consumed_at].clone());
                    consumed_at += 1;
                    prior_removed_crlf = None;
                }
                PatchLine::Removed(_) => {
                    let at = start + consumed_at;
                    prior_removed_crlf = Some(at + 1 < lines.len() && lines[at].ends_with('\r'));
                    removed += 1;
                    consumed_at += 1;
                }
                PatchLine::Added(text) => {
                    added += 1;
                    let nearby = lines
                        .get(start + consumed_at)
                        .map(|line| line.ends_with('\r') && start + consumed_at + 1 < lines.len())
                        .or_else(|| {
                            start
                                .checked_add(consumed_at)
                                .and_then(|n| n.checked_sub(1))
                                .and_then(|at| {
                                    lines
                                        .get(at)
                                        .map(|line| line.ends_with('\r') && at + 1 < lines.len())
                                })
                        })
                        .unwrap_or(false);
                    // Replacements inherit the actual removed line's
                    // delimiter, rather than an unrelated leading context.
                    let next_is_remove = hunk.lines[index + 1..]
                        .iter()
                        .take_while(|item| !matches!(item, PatchLine::Context(_)))
                        .any(|item| matches!(item, PatchLine::Removed(_)));
                    let crlf = if next_is_remove {
                        nearby
                    } else {
                        prior_removed_crlf.unwrap_or(nearby)
                    };
                    let mut added_text = text.clone();
                    if crlf {
                        added_text.push('\r');
                    }
                    replacement.push(added_text);
                }
            }
        }
        let replacement_len = replacement.len();
        lines.splice(start..start + block.len(), replacement);
        from_line = start + replacement_len;
    }

    Ok((lines.join("\n"), added, removed))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn update(source: &str, hunk: &str) -> Result<String> {
        let patch = format!("*** Begin Patch\n*** Update File: file.txt\n{hunk}\n*** End Patch\n");
        let ops = parse_patch(&patch)?;
        let PatchOp::Update { hunks, .. } = &ops[0] else {
            unreachable!()
        };
        Ok(apply_patch_hunks(source, hunks, "file.txt", 0, hunks.len())?.0)
    }
    #[test]
    fn crlf_source_with_lf_patch_and_leading_context() {
        let result = update("head\r\nold\r\ntail\r\n", "@@\n head\n-old\n+new\n tail").unwrap();
        assert_eq!(result, "head\r\nnew\r\ntail\r\n");
    }
    #[test]
    fn mixed_newlines_use_removed_line_delimiter() {
        let result = update("head\r\nold\ntail\r\n", "@@\n head\n-old\n+new\n tail").unwrap();
        assert_eq!(result, "head\r\nnew\ntail\r\n");
    }
    #[test]
    fn trailing_literal_cr_in_lf_patch_is_not_silently_stripped() {
        let result = update("old\r", "@@\n-old\r\n+new\r").unwrap();
        assert_eq!(result, "new\r");
    }
    #[test]
    fn eof_markers_filter_ambiguous_repeated_context_before_counting() {
        let result = update("same\nsame\n", "@@\n-same\n+last\n*** End of File").unwrap();
        assert_eq!(result, "same\nlast\n");
    }
    #[test]
    fn eof_only_append_requires_explicit_marker() {
        let result = update("old\n", "@@\n+new\n*** End of File").unwrap();
        assert_eq!(result, "old\nnew\n");
        assert!(update("old\n", "@@\n+new").is_err());
    }
    #[test]
    fn crlf_patch_syntax_decodes_separators() {
        let patch =
            "*** Begin Patch\r\n*** Add File: f.txt\r\n+one\r\n+\r\n+two\r\n*** End Patch\r\n";
        let result = parse_patch(patch).unwrap();
        let PatchOp::Add { lines, crlf, .. } = &result[0] else {
            unreachable!()
        };
        assert!(*crlf);
        assert_eq!(lines, &["one", "", "two"]);
        assert_eq!(format!("{}\r\n", lines.join("\r\n")), "one\r\n\r\ntwo\r\n");
    }
    #[test]
    fn malformed_duplicate_hunks_are_not_quadratic() {
        let source = (0..12_000).map(|_| "same\n").collect::<String>();
        let result = update(&source, "@@\n-same\n+different");
        assert!(result.is_err());
    }
}
