//! Native descriptor-scoped discovery and grep. Globs/regex/gitignore match
//! only already-authorized names and buffers. Reads never take mutation locks.
use crate::{platform_fs, tool_failure};
use aw_agent_core::{
    CancellationToken, Error, Result, Tool, ToolContext, ToolFuture, ToolOutput, ToolSpec,
};
use aw_platform::scoped_fs::{EntryKind, Root};
use aw_tools_core::filesystem::{
    Filter as FsFilter, Kind as FsKind, MetaKey, Selection as FsSelection, Sort as FsSort,
    TimeRange,
};
use globset::{GlobBuilder, GlobMatcher};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BinaryHeap, sync::Arc, time::Duration};

// The file scan admits at most 4096 candidates, not 65536 retained paths.
const MAX_CANDIDATES: usize = 4096;
const MAX_GREP_SCAN_BYTES: usize = 32 * 1024 * 1024;
const MAX_GREP_SCAN_FILES: usize = 4096;
const MAX_DEPTH: usize = 128;
const MAX_OUTPUT: usize = 96 * 1024;
const MAX_GREP_HITS: usize = 200;
const MAX_GREP_CONTEXT: usize = 8;
const MAX_PATTERN: usize = 1024;
const MAX_TEXT_FILE: usize = 1024 * 1024;
type WalkProgress = aw_tools_core::filesystem::walk::Progress;
fn walk_error(
    error: aw_tools_core::filesystem::walk::WalkError<aw_platform::scoped_fs::ScopedError>,
) -> Error {
    use aw_tools_core::filesystem::walk::WalkError;
    match error {
        WalkError::Source(error) => platform_fs::<()>(Err(error)).unwrap_err(),
        WalkError::Cancelled => Error::Cancelled,
        WalkError::IgnoreLimit => Error::Limit("gitignore search budget exhausted"),
        WalkError::InvalidIgnore => Error::Denied("invalid gitignore pattern"),
    }
}
fn walk(
    root: &Root,
    start: &str,
    depth: usize,
    gitignore: bool,
    skip_hidden: bool,
    token: &CancellationToken,
    mut visit: impl FnMut(&str, &str, &EntryKind) -> Result<bool>,
) -> Result<WalkProgress> {
    let result = aw_tools_core::filesystem::walk::walk(
        root,
        start,
        depth,
        !skip_hidden,
        gitignore,
        &|| token.is_cancelled(),
        |entry| {
            let kind = match entry.kind {
                FsKind::File => EntryKind::File,
                FsKind::Dir => EntryKind::Directory,
                FsKind::Symlink => EntryKind::Symlink,
                _ => EntryKind::Unsupported,
            };
            visit(&entry.path, &entry.name, &kind).map_err(|error| {
                aw_tools_core::filesystem::walk::WalkError::Source(match error {
                    Error::Io(e) => aw_platform::scoped_fs::ScopedError::Io(e),
                    Error::Cancelled => aw_platform::scoped_fs::ScopedError::Cancelled,
                    Error::Denied(s) => aw_platform::scoped_fs::ScopedError::Denied(s),
                    Error::Limit(s) => aw_platform::scoped_fs::ScopedError::Limit(s),
                    _ => aw_platform::scoped_fs::ScopedError::Denied("filesystem visit failed"),
                })
            })
        },
    )
    .map_err(walk_error)?;
    Ok(result)
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
}
impl Search {
    pub(crate) fn new(root: Arc<Root>, op: Operation) -> Self {
        Self { root, op }
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
fn retain_best<T: Ord>(heap: &mut BinaryHeap<T>, value: T, limit: usize) -> bool {
    if heap.len() < limit {
        heap.push(value);
        false
    } else {
        if heap.peek().is_some_and(|worst| value < *worst) {
            heap.pop();
            heap.push(value);
        }
        true
    }
}
#[derive(Serialize)]
struct Counted {
    path: String,
    count: usize,
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
fn pattern(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > MAX_PATTERN || value.chars().any(char::is_control) {
        return Err(Error::Denied(
            "search pattern must be bounded printable text",
        ));
    }
    Ok(())
}
fn glob(pattern: &str) -> Result<GlobMatcher> {
    self::pattern(pattern)?;
    Ok(GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map_err(|_| Error::Denied("invalid search glob"))?
        .compile_matcher())
}
fn optional_glob(value: Option<&str>) -> Result<Option<GlobMatcher>> {
    value
        .filter(|value| !value.is_empty())
        .map(glob)
        .transpose()
}
fn regex(args: &GrepArgs) -> Result<Regex> {
    pattern(&args.pattern)?;
    let expression = if args.literal {
        regex::escape(&args.pattern)
    } else {
        args.pattern.clone()
    };
    RegexBuilder::new(&expression)
        .case_insensitive(args.ignore_case)
        .size_limit(1 << 20)
        .dfa_size_limit(1 << 20)
        .build()
        .map_err(|_| Error::Denied("invalid or overly complex grep regex"))
}
/// Globs are relative to the directory requested by the caller. A bare
/// basename glob also works at any depth (dotlink's single glob grammar).
fn matches_path(glob: &GlobMatcher, entry: &str, name: &str, start: &str) -> bool {
    let relative = if start.is_empty() {
        entry
    } else {
        entry
            .strip_prefix(start)
            .and_then(|rest| rest.strip_prefix('/'))
            .unwrap_or(entry)
    };
    glob.is_match(relative) || glob.is_match(name)
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
    query: aw_tools_core::filesystem::discovery::Options<'_>,
    cancel: &CancellationToken,
) -> Result<ToolOutput> {
    use aw_tools_core::filesystem::discovery::{self, Error as E};
    let validated = path(query.path)?;
    let options = discovery::Options {
        path: validated,
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
fn list(root: &Root, args: &LsArgs, cancel: &CancellationToken) -> Result<ToolOutput> {
    let filter =
        FsFilter::compile(None, None, FsKind::Any, None, None, None).map_err(Error::Denied)?;
    run_discovery(
        root,
        aw_tools_core::filesystem::discovery::Options {
            path: &args.path,
            depth: 1,
            gitignore: args.gitignore,
            hidden: args.hidden,
            filter: &filter,
            sort: args.sort,
            meta: &args.meta,
            offset: args.offset,
            limit: args.limit,
            basenames: true,
            output_bytes: MAX_OUTPUT,
        },
        cancel,
    )
}
fn find(root: &Root, args: &FindArgs, cancel: &CancellationToken) -> Result<ToolOutput> {
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
        aw_tools_core::filesystem::discovery::Options {
            path: &args.path,
            depth: args.depth.unwrap_or(MAX_DEPTH),
            gitignore: args.gitignore,
            hidden: args.hidden,
            filter: &filter,
            sort: args.sort,
            meta: &args.meta,
            offset: args.offset,
            limit: args.limit,
            basenames: false,
            output_bytes: MAX_OUTPUT,
        },
        cancel,
    )
}
fn content_line(path: &str, line: usize, text: &str, numbered: bool, hit: bool) -> String {
    let label = aw_tools_core::search::content_path_label(path);
    if numbered {
        format!("{label}:{}:{}{}\n", line, if hit { "" } else { "-" }, text)
    } else {
        format!("{label}:{}{}\n", if hit { "" } else { "-" }, text)
    }
}
fn grep(root: &Root, args: &GrepArgs, cancel: &CancellationToken) -> Result<ToolOutput> {
    let start = path(&args.path)?;
    let re = regex(args)?;
    let filter = optional_glob(args.glob.as_deref())?;
    let mut candidates = BinaryHeap::<String>::new();
    let mut candidate_capped = false;
    let mut progress = WalkProgress::default();
    let mut unreadable_files = false;
    let mut oversize_files = false;
    let mut explicit_ignore_unavailable = false;

    // AW additionally permits an exact-file grep. It must honor the same
    // glob, hidden and ignore rules as a directory-scoped search.
    let explicit_file = if start.is_empty() {
        false
    } else {
        root.list(start).is_err() && root.entry_info(start).is_ok()
    };
    if explicit_file {
        let basename = start.rsplit('/').next().unwrap_or(start);
        if platform_fs(root.entry_info(start))?.0 > MAX_TEXT_FILE as u64 {
            return Err(Error::Limit("grep file exceeds 1 MiB; use read or bread"));
        }
        let hidden = !args.hidden && start.split('/').any(|name| name.starts_with('.'));
        let ignored = if args.gitignore {
            let (ignored, unavailable) =
                aw_tools_core::filesystem::walk::explicitly_ignored(root, start, false)
                    .map_err(walk_error)?;
            explicit_ignore_unavailable = unavailable;
            ignored
        } else {
            false
        };
        if !hidden
            && !ignored
            && filter
                .as_ref()
                .is_none_or(|f| matches_path(f, start, basename, ""))
        {
            candidates.push(start.into());
        }
    } else {
        progress = walk(
            root,
            start,
            usize::MAX,
            args.gitignore,
            !args.hidden,
            cancel,
            |full, name, kind| {
                if *kind != EntryKind::File
                    || filter
                        .as_ref()
                        .is_some_and(|f| !matches_path(f, full, name, start))
                {
                    return Ok(false);
                }
                candidate_capped |= retain_best(&mut candidates, full.to_owned(), MAX_CANDIDATES);
                Ok(false)
            },
        )?;
    }
    let mut candidates = candidates.into_sorted_vec();
    candidates.dedup();

    let mut seen = 0usize;
    let mut emitted = 0usize;
    let mut files = Vec::<String>::new();
    let mut counts = Vec::<Counted>::new();
    let mut content = String::new();
    let mut page_truncated = false;
    let mut paging_more = false;
    let mut scan_incomplete = progress.incomplete() || explicit_ignore_unavailable;
    let mut truncated = candidate_capped || scan_incomplete;
    let mut stop_reason = progress.reason().or(if explicit_ignore_unavailable {
        Some("ignore_rules")
    } else {
        None
    });
    let mut work = aw_tools_core::search::WorkBudget::new(
        Duration::from_secs(15),
        MAX_GREP_SCAN_BYTES,
        MAX_GREP_SCAN_FILES,
    );
    'file: for file in candidates {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if work.remaining_bytes() == 0 || work.remaining_files() == 0 {
            scan_incomplete = true;
            stop_reason = Some("content_budget");
            truncated = true;
            break;
        }
        let size = match root.entry_info(&file) {
            Ok((size, _)) => size,
            Err(_) => {
                unreadable_files = true;
                continue;
            }
        };
        if size > MAX_TEXT_FILE as u64 {
            oversize_files = true;
            continue;
        }
        if !work.begin_file(size as usize) {
            scan_incomplete = true;
            stop_reason = Some("content_budget");
            truncated = true;
            break;
        }
        let bytes = match root.read_bytes(&file, MAX_TEXT_FILE.min(work.remaining_bytes())) {
            Ok(bytes) => bytes,
            Err(aw_platform::scoped_fs::ScopedError::Limit(_)) => {
                scan_incomplete = true;
                stop_reason = Some("content_budget");
                truncated = true;
                break;
            }
            Err(_) => {
                unreadable_files = true;
                continue;
            }
        };
        if !work.charge(bytes.len()) {
            scan_incomplete = true;
            stop_reason = Some("content_budget");
            truncated = true;
            break;
        }
        let text = match crate::text_class::text_bytes(&bytes) {
            Some(text) => text,
            None => continue,
        };
        let lines: Vec<_> = text.lines().collect();
        let hits = lines.iter().filter(|line| re.is_match(line)).count();
        if hits == 0 {
            continue;
        }
        match args.mode {
            OutputMode::Files | OutputMode::Count => {
                if seen < args.offset {
                    seen += 1;
                    continue;
                }
                if emitted >= args.limit {
                    paging_more = true;
                    truncated = true;
                    break;
                }
                if matches!(args.mode, OutputMode::Files) {
                    files.push(file);
                } else {
                    counts.push(Counted {
                        path: file,
                        count: hits,
                    });
                }
                emitted += 1;
            }
            OutputMode::Content => {
                let mut included = std::collections::BTreeSet::<usize>::new();
                for i in lines
                    .iter()
                    .enumerate()
                    .filter_map(|(i, line)| re.is_match(line).then_some(i))
                {
                    if seen < args.offset {
                        seen += 1;
                        continue;
                    }
                    if emitted >= args.limit {
                        page_truncated = true;
                        truncated = true;
                        break 'file;
                    }
                    let from = i.saturating_sub(args.before.or(args.context).unwrap_or(0));
                    let through =
                        (i + args.after.or(args.context).unwrap_or(0) + 1).min(lines.len());
                    let mut block = String::new();
                    for (row, line) in lines.iter().enumerate().take(through).skip(from) {
                        if included.contains(&row) || (row != i && re.is_match(line)) {
                            continue;
                        }
                        block.push_str(&content_line(&file, row + 1, line, true, row == i));
                    }
                    // A primary hit and its requested/coalesced context is
                    // one indivisible semantic result. Do not omit context
                    // or replace a long match with a misleading placeholder.
                    let available = MAX_OUTPUT.saturating_sub(content.len()).saturating_sub(256);
                    if block.len() > available {
                        if content.is_empty() {
                            return Err(Error::Limit(
                                "grep hit/context exceeds result ceiling; use context=0, read, or narrow path"
                            ));
                        }
                        page_truncated = true;
                        truncated = true;
                        break 'file;
                    }
                    for (row, line) in lines.iter().enumerate().take(through).skip(from) {
                        if row == i || !re.is_match(line) {
                            included.insert(row);
                        }
                    }
                    content.push_str(&block);
                    emitted += 1;
                }
            }
        }
    }
    if candidate_capped && stop_reason.is_none() {
        scan_incomplete = true;
        stop_reason = Some("candidate_budget");
    }
    if unreadable_files {
        scan_incomplete = true;
        if stop_reason.is_none() {
            stop_reason = Some("unreadable_files");
        }
    }
    if oversize_files {
        scan_incomplete = true;
        if stop_reason.is_none() {
            stop_reason = Some("oversize_files");
        }
    }
    truncated |= scan_incomplete;
    let flags = aw_tools_core::search::Incomplete {
        content_budget: stop_reason == Some("content_budget"),
        candidate_budget: candidate_capped,
        directory_budget: progress.directory_budget,
        traversal_error: progress.traversal_error,
        ignore_rules: progress.ignore_rules || explicit_ignore_unavailable,
        path_encoding: progress.path_encoding,
        unreadable_files,
        oversize_files,
    };
    match args.mode {
        OutputMode::Files | OutputMode::Count => {
            let mut out = String::new();
            let mut shown = 0usize;
            let mut overflow = false;
            match args.mode {
                OutputMode::Files => {
                    for file in &files {
                        let line = format!("{}\n", aw_tools_core::filesystem::safe_label(file));
                        if out.len() + line.len() + 128 > MAX_OUTPUT {
                            overflow = true;
                            break;
                        }
                        out.push_str(&line);
                        shown += 1;
                    }
                }
                OutputMode::Count => {
                    for item in &counts {
                        let line = format!(
                            "{}  {}\n",
                            item.count,
                            aw_tools_core::filesystem::safe_label(&item.path)
                        );
                        if out.len() + line.len() + 128 > MAX_OUTPUT {
                            overflow = true;
                            break;
                        }
                        out.push_str(&line);
                        shown += 1;
                    }
                }
                OutputMode::Content => unreachable!(),
            }
            let causes = aw_tools_core::search::compact_incomplete(flags);
            let normalized = causes
                .as_deref()
                .map(|s| s.trim().trim_start_matches("… incomplete:"));
            let next = (paging_more || overflow).then(|| args.offset + shown);
            if let Some(status) = aw_tools_core::filesystem::status::query(next, normalized) {
                if out.len() + status.len() <= MAX_OUTPUT {
                    out.push_str(&status);
                }
            }
            Ok(ToolOutput {
                content: out,
                is_error: false,
                truncated: next.is_some() || scan_incomplete,
            })
        }
        OutputMode::Content => {
            let mut reasons = Vec::new();
            if flags.content_budget
                || flags.directory_budget
                || flags.candidate_budget
                || flags.oversize_files
            {
                reasons.push("budget");
            }
            if flags.traversal_error || flags.unreadable_files {
                reasons.push("walk");
            }
            if flags.ignore_rules {
                reasons.push("gitignore");
            }
            if flags.path_encoding {
                reasons.push("path_encoding");
            }
            let incomplete = (!reasons.is_empty()).then(|| reasons.join(","));
            let next = page_truncated.then(|| args.offset + emitted);
            if let Some(status) =
                aw_tools_core::filesystem::status::query(next, incomplete.as_deref())
            {
                if content.len() + status.len() <= MAX_OUTPUT {
                    content.push_str(&status);
                }
            }
            Ok(ToolOutput {
                content,
                is_error: false,
                truncated,
            })
        }
    }
}
impl Tool for Search {
    fn spec(&self) -> ToolSpec {
        use aw_tools_core::filesystem::schema::{SchemaCaps, Tool as C};
        let tool = match self.op {
            Operation::Ls => C::Ls,
            Operation::Find => C::Find,
            Operation::Grep => C::Grep,
        };
        crate::core_spec(
            tool,
            SchemaCaps {
                parallel_reads: true,
                ..Default::default()
            },
        )
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
                path(&a.path)?;
                if !(1..=MAX_GREP_HITS).contains(&a.limit)
                    || a.before.or(a.context).is_some_and(|v| v > MAX_GREP_CONTEXT)
                    || a.after.or(a.context).is_some_and(|v| v > MAX_GREP_CONTEXT)
                    || (a.context.is_some() && (a.before.is_some() || a.after.is_some()))
                    || a.depth.is_some_and(|v| !(1..=MAX_DEPTH).contains(&v))
                {
                    return Err(Error::Denied("invalid grep limit/depth/context"));
                }
                regex(&a)?;
                optional_glob(a.glob.as_deref())?;
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
            let answer = aw_tools_core::asynchronous::run_blocking(move || match op {
                Operation::Ls => list(&root, &parse::<LsArgs>(&request)?, &token),
                Operation::Find => find(&root, &parse::<FindArgs>(&request)?, &token),
                Operation::Grep => grep(&root, &parse::<GrepArgs>(&request)?, &token),
            })
            .await
            .map_err(|_| Error::Protocol("search worker failed"))?;
            tool_failure(answer, false)
        })
    }
}
