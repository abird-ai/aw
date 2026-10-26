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
    path::Path,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

const MAX_ENTRY_RESULTS: usize = 16384;
const MAX_CANDIDATES: usize = 65536;
const MAX_DEPTH: usize = 128;
const MAX_OUTPUT: usize = 96 * 1024;
const MAX_GREP_HITS: usize = 200;
const MAX_GREP_CONTEXT: usize = 8;
const MAX_PATTERN: usize = 1024;
const MAX_TEXT_FILE: usize = 1024 * 1024;
const MAX_GITIGNORE_BYTES: usize = 64 * 1024;
const MAX_GITIGNORE_LINES: usize = 1024;
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
    name.starts_with(".aw-stage-") && name.ends_with(".tmp")
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
) -> Result<Arc<IgnoreLevel>> {
    let name = if dir.is_empty() {
        ".gitignore".to_owned()
    } else {
        format!("{dir}/.gitignore")
    };
    let rules = match root.read(&name) {
        Ok(content) => {
            if content.len() > MAX_GITIGNORE_BYTES || content.lines().count() > MAX_GITIGNORE_LINES
            {
                return Err(Error::Limit("gitignore exceeds configured search bound"));
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
        Err(_) => return Err(Error::Denied("cannot inspect requested gitignore rules")),
    };
    Ok(Arc::new(IgnoreLevel { rules, parent }))
}
fn ancestors(root: &Root, path: &str, enabled: bool) -> Result<Option<Arc<IgnoreLevel>>> {
    if !enabled {
        return Ok(None);
    }
    let mut current = None;
    let mut parent = String::new();
    for part in path.split('/').filter(|part| !part.is_empty()) {
        current = Some(read_ignore(root, &parent, current)?);
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
) -> Result<bool> {
    let mut stack = vec![Level {
        path: start.to_owned(),
        depth: 0,
        ignore: ancestors(root, start, gitignore)?,
    }];
    let mut seen = 0usize;
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut truncated = false;
    while let Some(level) = stack.pop() {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if Instant::now() >= deadline || seen >= MAX_CANDIDATES || level.depth >= MAX_DEPTH {
            return Ok(true);
        }
        let listing = match root.list_with_limit(&level.path, MAX_ENTRY_RESULTS) {
            Ok(listing) => listing,
            Err(err) if level.depth == 0 => {
                return Err(crate::platform_fs::<()>(Err(err)).unwrap_err())
            }
            Err(_) => {
                truncated = true;
                continue;
            }
        };
        let rules = if gitignore {
            Some(read_ignore(root, &level.path, level.ignore)?)
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
            if seen > MAX_CANDIDATES || Instant::now() >= deadline {
                return Ok(true);
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
                return Ok(true);
            }
            if entry.kind == EntryKind::Directory && level.depth + 1 < max_depth {
                children.push(entry.path);
            }
        }
        truncated |= listing.incomplete;
        for child in children.into_iter().rev() {
            stack.push(Level {
                path: child,
                depth: level.depth + 1,
                ignore: rules.clone(),
            });
        }
    }
    Ok(truncated)
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
    let filter = args.pattern.as_deref().map(glob).transpose()?;
    let mut result = Vec::<Listed>::new();
    let mut matched = 0usize;
    let depth = depth_for(args);
    let truncated = walk(
        root,
        start,
        depth,
        false,
        false,
        cancel,
        |path, name, kind| {
            if !args.kind.accepts(kind)
                || filter
                    .as_ref()
                    .is_some_and(|filter| !matches_path(filter, path, name, start))
            {
                return Ok(false);
            }
            matched += 1;
            if matched > args.limit {
                return Ok(true);
            }
            let (size, modified) = match root.entry_info(path) {
                Ok(info) => info,
                Err(_) => return Ok(false),
            };
            result.push(Listed {
                path: path.into(),
                kind: if *kind == EntryKind::File {
                    "file"
                } else {
                    "dir"
                },
                size,
                modified,
            });
            Ok(false)
        },
    )?;
    match args.sort {
        Sort::Path => result.sort_by(|a, b| a.path.cmp(&b.path)),
        Sort::Mtime => result.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.path.cmp(&b.path))),
    }
    let (body, truncated) = trim_json(
        result,
        truncated || matched > args.limit,
        |entries, truncated| json!({"entries":entries,"truncated":truncated}).to_string(),
    );
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
    let filter = args.glob.as_deref().map(glob).transpose()?;
    let mut candidates = Vec::<String>::new();
    let mut candidate_truncated = false;
    if !start.is_empty() && root.read_bytes(start, MAX_TEXT_FILE).is_ok() {
        candidates.push(start.into());
    } else {
        candidate_truncated = walk(
            root,
            start,
            usize::MAX,
            args.gitignore,
            args.skip_hidden,
            cancel,
            |path, name, kind| {
                if *kind != EntryKind::File
                    || filter
                        .as_ref()
                        .is_some_and(|f| !matches_path(f, path, name, start))
                {
                    return Ok(false);
                }
                candidates.push(path.to_owned());
                Ok(candidates.len() >= MAX_CANDIDATES)
            },
        )?;
    }
    candidates.sort();
    let mut seen = 0usize;
    let mut emitted = 0usize;
    let mut files = Vec::<String>::new();
    let mut counts = Vec::<Counted>::new();
    let mut content = String::new();
    let mut truncated = candidate_truncated;
    'file: for path in candidates {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let bytes = match root.read_bytes(&path, MAX_TEXT_FILE) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let text = match std::str::from_utf8(&bytes) {
            Ok(text) => text,
            Err(_) => continue,
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
                    files.push(path);
                } else {
                    counts.push(Counted {
                        path,
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
                        if included.contains(&row) {
                            continue;
                        }
                        // A match outside this page must not be emitted as context.
                        if row != i && re.is_match(line) {
                            continue;
                        }
                        block.push_str(&content_line(
                            &path,
                            row + 1,
                            line,
                            args.numbered,
                            row == i,
                        ));
                    }
                    if content.len().saturating_add(block.len()) + 128 > MAX_OUTPUT {
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
    match args.output_mode {
        OutputMode::Files => {
            let (body, truncated) = trim_json(files, truncated, |files, truncated| {
                json!({"files":files,"truncated":truncated}).to_string()
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
                json!({"counts":counts,"page_total":page_total,"truncated":truncated}).to_string()
            });
            Ok(ToolOutput {
                content: body,
                is_error: false,
                truncated,
            })
        }
        OutputMode::Content => {
            if truncated {
                content.push_str(&format!("… truncated at {emitted} hits\n"));
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
                if let Some(pattern) = a.pattern {
                    glob(&pattern)?;
                }
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
                if let Some(glob_pattern) = &a.glob {
                    glob(glob_pattern)?;
                }
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
