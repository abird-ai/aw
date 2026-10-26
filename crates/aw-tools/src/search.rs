//! Native descriptor-scoped discovery and grep. Globs/regex/gitignore match
//! only already-authorized names and buffers. Reads never take mutation locks.
use crate::{platform_fs, tool_failure};
use aw_agent_core::{
    CancellationToken, Error, Result, Tool, ToolContext, ToolEffect, ToolFuture, ToolOutput,
    ToolSpec,
};
use aw_platform::scoped_fs::{EntryKind, Root};
use globset::{GlobBuilder, GlobMatcher};
use ignore::{gitignore::GitignoreBuilder, Match};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BinaryHeap,
    path::Path,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

const MAX_ENTRY_RESULTS: usize = 16384;
const MAX_CANDIDATES: usize = 65536;
const MAX_WALK_VISITS: usize = 100_000;
const MAX_GREP_SCAN_BYTES: usize = 32 * 1024 * 1024;
const MAX_GREP_SCAN_FILES: usize = 4096;
const MAX_DEPTH: usize = 128;
const MAX_OUTPUT: usize = 96 * 1024;
const MAX_GREP_HITS: usize = 200;
const MAX_GREP_CONTEXT: usize = 8;
const MAX_PATTERN: usize = 1024;
const MAX_TEXT_FILE: usize = 1024 * 1024;
const MAX_GITIGNORE_BYTES: usize = 64 * 1024;
const MAX_GITIGNORE_LINES: usize = 1024;
const MAX_IGNORE_SCAN_BYTES: usize = 4 * 1024 * 1024;
const MAX_IGNORE_SCAN_FILES: usize = 256;
#[derive(Default)]
struct IgnoreBudget {
    bytes: usize,
    files: usize,
    unavailable: bool,
}
impl IgnoreBudget {
    fn consume(&mut self, bytes: usize) -> Result<()> {
        if self.files >= MAX_IGNORE_SCAN_FILES
            || self.bytes.saturating_add(bytes) > MAX_IGNORE_SCAN_BYTES
        {
            return Err(Error::Limit(
                "gitignore search budget exhausted; narrow path or disable gitignore",
            ));
        }
        self.files += 1;
        self.bytes += bytes;
        Ok(())
    }
}
#[derive(Default, Clone, Copy)]
struct WalkProgress {
    budget_exhausted: bool,
    traversal_error: bool,
    ignore_unavailable: bool,
}
impl WalkProgress {
    fn incomplete(self) -> bool {
        self.budget_exhausted || self.traversal_error || self.ignore_unavailable
    }
    fn reason(self) -> Option<&'static str> {
        if self.budget_exhausted {
            Some("directory_budget")
        } else if self.traversal_error {
            Some("traversal_error")
        } else if self.ignore_unavailable {
            Some("ignore_rules")
        } else {
            None
        }
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
fn yes() -> bool {
    true
}

#[derive(Clone, Copy)]
pub(crate) enum Operation {
    Ls,
    Grep,
}
impl Operation {
    fn name(self) -> &'static str {
        match self {
            Self::Ls => "ls",
            Self::Grep => "grep",
        }
    }
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
#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum Kind {
    #[default]
    Any,
    File,
    Dir,
}
impl Kind {
    fn accepts(self, kind: &EntryKind) -> bool {
        match self {
            Self::Any => true,
            Self::File => kind == &EntryKind::File,
            Self::Dir => kind == &EntryKind::Directory,
        }
    }
}
#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum Sort {
    #[default]
    Path,
    Mtime,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct LsArgs {
    #[serde(default = "dot")]
    path: String,
    pattern: Option<String>,
    depth: Option<usize>,
    #[serde(default)]
    kind: Kind,
    #[serde(default)]
    sort: Sort,
    #[serde(default = "ls_limit")]
    limit: usize,
    #[serde(default)]
    gitignore: bool,
    #[serde(default)]
    skip_hidden: bool,
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
    #[serde(default)]
    output_mode: OutputMode,
    #[serde(default)]
    before: usize,
    #[serde(default)]
    after: usize,
    #[serde(default = "yes")]
    numbered: bool,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    gitignore: bool,
    #[serde(default)]
    skip_hidden: bool,
    #[serde(default)]
    parallel: bool,
}
#[derive(Serialize)]
struct Listed {
    path: String,
    kind: &'static str,
    size: u64,
    #[serde(skip)]
    modified: Option<SystemTime>,
}
/// Worst ranked entry sits at the heap root, independent of walk order.
struct Ranked {
    entry: Listed,
    sort: Sort,
}
impl PartialEq for Ranked {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}
impl Eq for Ranked {}
impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Ranked {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.sort {
            Sort::Path => self.entry.path.cmp(&other.entry.path),
            Sort::Mtime => other
                .entry
                .modified
                .cmp(&self.entry.modified)
                .then_with(|| self.entry.path.cmp(&other.entry.path)),
        }
    }
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
    platform_fs(aw_platform::scoped_fs::validate_path(root))?;
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
fn depth_for(args: &LsArgs) -> usize {
    args.depth.unwrap_or_else(|| match args.pattern.as_deref() {
        Some(pattern) if pattern.contains("**") => usize::MAX,
        Some(pattern) => pattern.split('/').count(),
        None => 1,
    })
}
fn hidden(name: &str) -> bool {
    name.starts_with('.')
}
fn staged_temp(name: &str) -> bool {
    (name.starts_with(".aw-stage-") || name.starts_with(".aw-backup-")) && name.ends_with(".tmp")
}
struct IgnoreLevel {
    rules: Option<ignore::gitignore::Gitignore>,
    parent: Option<Arc<IgnoreLevel>>,
}
impl IgnoreLevel {
    fn ignored(&self, path: &str, is_dir: bool) -> bool {
        let mut level = Some(self);
        while let Some(current) = level {
            if let Some(matcher) = &current.rules {
                match matcher.matched(Path::new(path), is_dir) {
                    Match::Ignore(_) => return true,
                    Match::Whitelist(_) => return false,
                    Match::None => {}
                }
            }
            level = current.parent.as_deref();
        }
        false
    }
}
fn read_ignore(
    root: &Root,
    dir: &str,
    parent: Option<Arc<IgnoreLevel>>,
    work: &mut IgnoreBudget,
) -> Result<Arc<IgnoreLevel>> {
    let name = if dir.is_empty() {
        ".gitignore".to_owned()
    } else {
        format!("{dir}/.gitignore")
    };
    let rules = match root.read_bytes(&name, MAX_GITIGNORE_BYTES + 1) {
        Ok(contents) => {
            if contents.len() > MAX_GITIGNORE_BYTES {
                return Err(Error::Limit("gitignore exceeds 64 KiB byte limit"));
            }
            work.consume(contents.len())?;
            let content = std::str::from_utf8(&contents)
                .map_err(|_| Error::Denied("gitignore must be UTF-8"))?;
            if content.lines().count() > MAX_GITIGNORE_LINES {
                return Err(Error::Limit("gitignore exceeds 1024-line limit"));
            }
            let mut builder = GitignoreBuilder::new(dir);
            for line in content.lines() {
                builder
                    .add_line(None, line)
                    .map_err(|_| Error::Denied("invalid gitignore pattern"))?;
            }
            Some(
                builder
                    .build()
                    .map_err(|_| Error::Denied("invalid gitignore"))?,
            )
        }
        Err(aw_platform::scoped_fs::ScopedError::Io(e))
            if e.kind() == std::io::ErrorKind::NotFound =>
        {
            None
        }
        // An inaccessible or unsafe ignore file cannot be treated as absent.
        // Continue with independently authorized entries but explicitly mark
        // the entire search incomplete. Invalid/oversized ignore files above
        // remain hard errors rather than silently dropping their rules.
        Err(aw_platform::scoped_fs::ScopedError::Io(_))
        | Err(aw_platform::scoped_fs::ScopedError::Denied(_)) => {
            work.unavailable = true;
            None
        }
        Err(aw_platform::scoped_fs::ScopedError::Limit(_)) => {
            return Err(Error::Limit("gitignore exceeds 64 KiB; narrow search"));
        }
        Err(aw_platform::scoped_fs::ScopedError::Cancelled) => return Err(Error::Cancelled),
    };
    Ok(Arc::new(IgnoreLevel { rules, parent }))
}
fn ancestors(
    root: &Root,
    path: &str,
    enabled: bool,
    work: &mut IgnoreBudget,
) -> Result<Option<Arc<IgnoreLevel>>> {
    if !enabled {
        return Ok(None);
    }
    let mut current = None;
    let mut parent = String::new();
    for part in path.split('/').filter(|part| !part.is_empty()) {
        current = Some(read_ignore(root, &parent, current, work)?);
        if !parent.is_empty() {
            parent.push('/');
        }
        parent.push_str(part);
    }
    Ok(current)
}
struct Level {
    path: String,
    depth: usize,
    ignore: Option<Arc<IgnoreLevel>>,
}
fn walk(
    root: &Root,
    start: &str,
    max_depth: usize,
    gitignore: bool,
    skip_hidden: bool,
    cancel: &CancellationToken,
    mut visit: impl FnMut(&str, &str, &EntryKind) -> Result<bool>,
) -> Result<WalkProgress> {
    let mut work = IgnoreBudget::default();
    let mut stack = vec![Level {
        path: start.to_owned(),
        depth: 0,
        ignore: ancestors(root, start, gitignore, &mut work)?,
    }];
    let mut seen = 0usize;
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut progress = WalkProgress::default();
    while let Some(level) = stack.pop() {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if Instant::now() >= deadline || seen >= MAX_WALK_VISITS || level.depth >= MAX_DEPTH {
            progress.budget_exhausted = true;
            progress.ignore_unavailable = work.unavailable;
            return Ok(progress);
        }
        let listing = match root.list_with_limit(&level.path, MAX_ENTRY_RESULTS) {
            Ok(listing) => listing,
            Err(err) if level.depth == 0 => {
                return Err(crate::platform_fs::<()>(Err(err)).unwrap_err())
            }
            Err(_) => {
                progress.traversal_error = true;
                continue;
            }
        };
        let rules = if gitignore {
            Some(read_ignore(root, &level.path, level.ignore, &mut work)?)
        } else {
            None
        };
        let mut entries = listing.entries;
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        let mut children = Vec::new();
        for entry in entries {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            seen += 1;
            if seen > MAX_WALK_VISITS || Instant::now() >= deadline {
                progress.budget_exhausted = true;
                progress.ignore_unavailable = work.unavailable;
                return Ok(progress);
            }
            if staged_temp(&entry.name)
                || (skip_hidden && hidden(&entry.name))
                || rules.as_ref().is_some_and(|rules| {
                    rules.ignored(&entry.path, entry.kind == EntryKind::Directory)
                })
            {
                continue;
            }
            if entry.kind == EntryKind::Unsupported {
                continue;
            }
            if visit(&entry.path, &entry.name, &entry.kind)? {
                progress.ignore_unavailable = work.unavailable;
                return Ok(progress);
            }
            if entry.kind == EntryKind::Directory && level.depth + 1 < max_depth {
                children.push(entry.path);
            }
        }
        progress.budget_exhausted |= listing.incomplete;
        for child in children.into_iter().rev() {
            stack.push(Level {
                path: child,
                depth: level.depth + 1,
                ignore: rules.clone(),
            });
        }
    }
    progress.ignore_unavailable = work.unavailable;
    Ok(progress)
}
fn trim_json<T>(
    mut data: Vec<T>,
    truncated: bool,
    render: impl Fn(&[T], bool) -> String,
) -> (String, bool) {
    let body = render(&data, truncated);
    if body.len() <= MAX_OUTPUT {
        return (body, truncated);
    }
    let (mut low, mut high) = (0usize, data.len());
    while low < high {
        let mid = low + (high - low).div_ceil(2);
        if render(&data[..mid], true).len() <= MAX_OUTPUT {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    data.truncate(low);
    (render(&data, true), true)
}
fn list(root: &Root, args: &LsArgs, cancel: &CancellationToken) -> Result<ToolOutput> {
    let start = path(&args.path)?;
    let filter = optional_glob(args.pattern.as_deref())?;
    let mut selected = BinaryHeap::<Ranked>::with_capacity(args.limit);
    let mut exceeded = false;
    let mut unreadable = false;
    let depth = depth_for(args);
    let progress = walk(
        root,
        start,
        depth,
        args.gitignore,
        args.skip_hidden,
        cancel,
        |full, name, kind| {
            if !args.kind.accepts(kind)
                || filter
                    .as_ref()
                    .is_some_and(|matcher| !matches_path(matcher, full, name, start))
            {
                return Ok(false);
            }
            let (size, modified) = match root.entry_info(full) {
                Ok(info) => info,
                Err(_) => {
                    unreadable = true;
                    return Ok(false);
                }
            };
            exceeded |= retain_best(
                &mut selected,
                Ranked {
                    entry: Listed {
                        path: full.to_owned(),
                        kind: if *kind == EntryKind::File {
                            "file"
                        } else {
                            "dir"
                        },
                        size,
                        modified,
                    },
                    sort: args.sort,
                },
                args.limit,
            );
            Ok(false)
        },
    )?;
    let mut entries: Vec<Listed> = selected.into_iter().map(|item| item.entry).collect();
    match args.sort {
        Sort::Path => entries.sort_by(|a, b| a.path.cmp(&b.path)),
        Sort::Mtime => entries.sort_by(|a, b| {
            b.modified
                .cmp(&a.modified)
                .then_with(|| a.path.cmp(&b.path))
        }),
    }
    let incomplete = progress.incomplete() || unreadable;
    let reason = progress.reason().or(if unreadable {
        Some("traversal_error")
    } else {
        None
    });
    let (body, truncated) = trim_json(entries, exceeded || incomplete, |entries, truncated| {
        json!({"entries":entries, "truncated":truncated,
            "scan_incomplete":incomplete, "incomplete_reason":reason})
        .to_string()
    });
    Ok(ToolOutput {
        content: body,
        is_error: false,
        truncated,
    })
}
fn content_line(path: &str, line: usize, text: &str, numbered: bool, hit: bool) -> String {
    if numbered {
        format!("{path}:{}:{}{}\n", line, if hit { "" } else { "-" }, text)
    } else {
        format!("{path}:{}{}\n", if hit { "" } else { "-" }, text)
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
        let hidden = args.skip_hidden && start.split('/').any(|name| name.starts_with('.'));
        let ignored = if args.gitignore {
            let parent = start.rsplit_once('/').map_or("", |(parent, _)| parent);
            let mut work = IgnoreBudget::default();
            let parents = ancestors(root, parent, true, &mut work)?;
            let ignored = read_ignore(root, parent, parents, &mut work)?.ignored(start, false);
            explicit_ignore_unavailable = work.unavailable;
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
            args.skip_hidden,
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
    let mut scan_incomplete = progress.incomplete() || explicit_ignore_unavailable;
    let mut truncated = candidate_capped || scan_incomplete;
    let mut stop_reason = progress.reason().or(if explicit_ignore_unavailable {
        Some("ignore_rules")
    } else {
        None
    });
    let mut bytes_left = MAX_GREP_SCAN_BYTES;
    let mut files_left = MAX_GREP_SCAN_FILES;
    'file: for file in candidates {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if bytes_left == 0 || files_left == 0 {
            scan_incomplete = true;
            stop_reason = Some("content_budget");
            truncated = true;
            break;
        }
        files_left -= 1;
        let size = match root.entry_info(&file) {
            Ok((size, _)) => size,
            Err(_) => {
                unreadable_files = true;
                continue;
            }
        };
        if size > MAX_TEXT_FILE as u64 {
            continue;
        }
        if size > bytes_left as u64 {
            scan_incomplete = true;
            stop_reason = Some("content_budget");
            truncated = true;
            break;
        }
        let bytes = match root.read_bytes(&file, MAX_TEXT_FILE) {
            Ok(bytes) => bytes,
            Err(_) => {
                unreadable_files = true;
                continue;
            }
        };
        bytes_left = bytes_left.saturating_sub(bytes.len());
        let text = match crate::text_class::text_bytes(&bytes) {
            Some(text) => text,
            None => continue,
        };
        let lines: Vec<_> = text.lines().collect();
        let matches = lines
            .iter()
            .enumerate()
            .filter_map(|(i, line)| re.is_match(line).then_some(i))
            .collect::<Vec<_>>();
        if matches.is_empty() {
            continue;
        }
        match args.output_mode {
            OutputMode::Files | OutputMode::Count => {
                if seen < args.offset {
                    seen += 1;
                    continue;
                }
                if emitted >= args.limit {
                    truncated = true;
                    break;
                }
                if matches!(args.output_mode, OutputMode::Files) {
                    files.push(file);
                } else {
                    counts.push(Counted {
                        path: file,
                        count: matches.len(),
                    });
                }
                emitted += 1;
            }
            OutputMode::Content => {
                let mut per_file = 0usize;
                let mut included = std::collections::BTreeSet::<usize>::new();
                for i in matches {
                    if seen < args.offset {
                        seen += 1;
                        continue;
                    }
                    if emitted >= args.limit || per_file >= 50 {
                        truncated = true;
                        break 'file;
                    }
                    let from = i.saturating_sub(args.before);
                    let through = (i + args.after + 1).min(lines.len());
                    let mut block = String::new();
                    for (row, line) in lines.iter().enumerate().take(through).skip(from) {
                        if included.contains(&row) || (row != i && re.is_match(line)) {
                            continue;
                        }
                        block.push_str(&content_line(
                            &file,
                            row + 1,
                            line,
                            args.numbered,
                            row == i,
                        ));
                    }
                    // Always reserve enough bytes for one complete suffix.
                    if content
                        .len()
                        .saturating_add(block.len())
                        .saturating_add(256)
                        > MAX_OUTPUT
                    {
                        truncated = true;
                        break 'file;
                    }
                    for (row, line) in lines.iter().enumerate().take(through).skip(from) {
                        if row == i || !re.is_match(line) {
                            included.insert(row);
                        }
                    }
                    content.push_str(&block);
                    per_file += 1;
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
    truncated |= scan_incomplete;
    let incomplete_reason = stop_reason;
    match args.output_mode {
        OutputMode::Files => {
            let (body, truncated) = trim_json(files, truncated, |files, truncated| {
                json!({"files":files,"truncated":truncated,"scan_incomplete":scan_incomplete,
                    "incomplete_reason":incomplete_reason})
                .to_string()
            });
            Ok(ToolOutput {
                content: body,
                is_error: false,
                truncated,
            })
        }
        OutputMode::Count => {
            let (body, truncated) = trim_json(counts, truncated, |counts, truncated| {
                let page_total = counts.iter().map(|count| count.count).sum::<usize>();
                json!({"counts":counts,"page_total":page_total,"truncated":truncated,
                    "scan_incomplete":scan_incomplete,"incomplete_reason":incomplete_reason})
                .to_string()
            });
            Ok(ToolOutput {
                content: body,
                is_error: false,
                truncated,
            })
        }
        OutputMode::Content => {
            if let Some(reason) = incomplete_reason {
                let suffix = format!("… search scan incomplete ({reason}); narrow path or glob\n");
                if content.len().saturating_add(suffix.len()) <= MAX_OUTPUT {
                    content.push_str(&suffix);
                }
            }
            if progress.ignore_unavailable || explicit_ignore_unavailable {
                const SUFFIX: &str =
                    "… ignore rules unavailable; results may include ignored files\n";
                if content.len().saturating_add(SUFFIX.len()) <= MAX_OUTPUT {
                    content.push_str(SUFFIX);
                }
            }
            if progress.traversal_error || unreadable_files {
                const SUFFIX: &str = "… unreadable entries; search results incomplete\n";
                if content.len().saturating_add(SUFFIX.len()) <= MAX_OUTPUT {
                    content.push_str(SUFFIX);
                }
            }
            if incomplete_reason.is_none() && truncated {
                let suffix = format!("… truncated at {emitted} hits\n");
                if content.len().saturating_add(suffix.len()) <= MAX_OUTPUT {
                    content.push_str(&suffix);
                }
            } else if emitted == 0 && args.offset > 0 {
                content.push_str(&format!("… no hits at offset {}\n", args.offset));
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
        let (description,parameters)=match self.op{
            Operation::Ls=>("List granted files/directories recursively by a single glob grammar: pattern, depth, kind=file/dir/any, sort=path/mtime and limit. Depth derives from the pattern segments; ** descends recursively. Symlinks and staging files are never traversed. Set parallel=true for independent reads.",
                json!({"type":"object","properties":{"path":{"type":"string"},
                "pattern":{"type":"string"},"depth":{"type":"integer","minimum":1},
                "kind":{"type":"string","enum":["any","file","dir"]},
                "sort":{"type":"string","enum":["path","mtime"]},
                "limit":{"type":"integer","minimum":1,"maximum":MAX_ENTRY_RESULTS},
                "gitignore":{"type":"boolean"},"skip_hidden":{"type":"boolean"},
                "parallel":{"type":"boolean"}},"additionalProperties":false})),
            Operation::Grep=>("Search granted UTF-8 files using bounded regex/literal text and a glob filter. output_mode is content/files/count, before/after control context, offset and limit page matches/files. gitignore and skip_hidden opt in; all outputs are byte-bounded. Set parallel=true only for independent reads.",
                json!({"type":"object","properties":{"pattern":{"type":"string"},
                "path":{"type":"string"},"glob":{"type":"string"},
                "literal":{"type":"boolean"},"ignore_case":{"type":"boolean"},
                "output_mode":{"type":"string","enum":["content","files","count"]},
                "before":{"type":"integer","minimum":0,"maximum":MAX_GREP_CONTEXT},
                "after":{"type":"integer","minimum":0,"maximum":MAX_GREP_CONTEXT},
                "numbered":{"type":"boolean"},"offset":{"type":"integer","minimum":0},
                "limit":{"type":"integer","minimum":1,"maximum":MAX_GREP_HITS},
                "gitignore":{"type":"boolean"},"skip_hidden":{"type":"boolean"},
                "parallel":{"type":"boolean"}},"required":["pattern"],"additionalProperties":false})),
        };
        ToolSpec {
            name: self.op.name().into(),
            description: description.into(),
            parameters,
            effect: ToolEffect::Read,
        }
    }
    fn validate(&self, value: &Value) -> Result<()> {
        match self.op {
            Operation::Ls => {
                let a: LsArgs = parse(value)?;
                path(&a.path)?;
                if !(1..=MAX_ENTRY_RESULTS).contains(&a.limit) || a.depth == Some(0) {
                    return Err(Error::Denied("invalid ls depth or limit"));
                }
                optional_glob(a.pattern.as_deref())?;
                let _ = a.parallel;
            }
            Operation::Grep => {
                let a: GrepArgs = parse(value)?;
                path(&a.path)?;
                if !(1..=MAX_GREP_HITS).contains(&a.limit)
                    || a.before > MAX_GREP_CONTEXT
                    || a.after > MAX_GREP_CONTEXT
                {
                    return Err(Error::Denied("invalid grep limit or context"));
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
            let answer = tokio::task::spawn_blocking(move || match op {
                Operation::Ls => list(&root, &parse::<LsArgs>(&request)?, &token),
                Operation::Grep => grep(&root, &parse::<GrepArgs>(&request)?, &token),
            })
            .await
            .map_err(|_| Error::Protocol("search worker failed"))?;
            tool_failure(answer, false)
        })
    }
}
