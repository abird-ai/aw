//! On-demand bounded search tools. Enumeration AND final reads use the
//! existing no-follow scoped Root; ignore/glob/regex are pattern libraries,
//! never filesystem authority. No index, watcher, or external process.
use crate::platform_fs;
use aw_agent_core::{
    CancellationToken, Error, Result, Tool, ToolContext, ToolEffect, ToolFuture, ToolOutput,
    ToolSpec,
};
use aw_platform::scoped_fs::{EntryKind, Root};
use globset::{GlobBuilder, GlobMatcher};
use ignore::{gitignore::GitignoreBuilder, Match};
use regex::{Regex, RegexBuilder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

const MAX_DEPTH: usize = 64;
const MAX_ENTRIES: usize = 8000;
const MAX_OUTPUT: usize = 50_000;
const MAX_IGNORE_BYTES: usize = 64 * 1024;
const MAX_IGNORE_LINES: usize = 1024;
const MAX_PATTERN: usize = 1024;
const MAX_MATCHES: usize = 200;
const MAX_CONTEXT: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operation {
    Ls,
    Find,
    Grep,
}
impl Operation {
    fn name(self) -> &'static str {
        match self {
            Self::Ls => "ls",
            Self::Find => "find",
            Self::Grep => "grep",
        }
    }
}
pub(crate) struct Search {
    root: Arc<Root>,
    operation: Operation,
}
impl Search {
    pub(crate) fn new(root: Arc<Root>, operation: Operation) -> Self {
        Self { root, operation }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LsArgs {
    #[serde(default, rename = "parallel")]
    _parallel: bool,
    #[serde(default)]
    path: String,
    #[serde(default = "default_limit")]
    limit: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FindArgs {
    #[serde(default, rename = "parallel")]
    _parallel: bool,
    pattern: String,
    #[serde(default)]
    path: String,
    #[serde(default = "default_limit")]
    limit: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GrepArgs {
    #[serde(default, rename = "parallel")]
    _parallel: bool,
    pattern: String,
    #[serde(default)]
    path: String,
    glob: Option<String>,
    #[serde(default)]
    ignore_case: bool,
    #[serde(default)]
    literal: bool,
    #[serde(default)]
    context: usize,
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_limit() -> usize {
    100
}
fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|_| Error::Denied("invalid, unknown or missing search arguments"))
}
fn path(value: &str) -> Result<&str> {
    if value.is_empty() || value == "." {
        return Ok("");
    }
    platform_fs(aw_platform::scoped_fs::validate_path(value))?;
    Ok(value)
}
fn limit(value: usize, maximum: usize) -> Result<()> {
    if (1..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(Error::Denied("search limit is outside the allowed range"))
    }
}
fn pattern(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > MAX_PATTERN || value.chars().any(char::is_control) {
        Err(Error::Denied(
            "search pattern must be bounded printable text",
        ))
    } else {
        Ok(())
    }
}
fn glob(value: &str) -> Result<GlobMatcher> {
    pattern(value)?;
    Ok(GlobBuilder::new(value)
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
        .map_err(|_| Error::Denied("invalid or overly complex grep expression"))
}
fn validate(op: Operation, args: &Value) -> Result<()> {
    match op {
        Operation::Ls => {
            let a: LsArgs = parse(args)?;
            path(&a.path)?;
            limit(a.limit, 512)
        }
        Operation::Find => {
            let a: FindArgs = parse(args)?;
            path(&a.path)?;
            limit(a.limit, MAX_MATCHES)?;
            glob(&a.pattern).map(|_| ())
        }
        Operation::Grep => {
            let a: GrepArgs = parse(args)?;
            path(&a.path)?;
            limit(a.limit, MAX_MATCHES)?;
            if a.context > MAX_CONTEXT {
                return Err(Error::Denied("grep context exceeds eight lines"));
            }
            if let Some(g) = a.glob.as_deref() {
                glob(g)?;
            }
            regex(&a).map(|_| ())
        }
    }
}

struct BoundedOutput {
    body: String,
    truncated: bool,
}
impl BoundedOutput {
    fn new() -> Self {
        Self {
            body: String::new(),
            truncated: false,
        }
    }
    fn line(&mut self, text: &str) -> bool {
        if self.body.len().saturating_add(text.len()).saturating_add(1) > MAX_OUTPUT - 256 {
            self.truncated = true;
            return false;
        }
        self.body.push_str(text);
        self.body.push('\n');
        true
    }
    fn finish(mut self, label: &str) -> ToolOutput {
        if self.truncated {
            self.body.push_str(&format!("[{label} truncated]\n"));
        }
        if self.body.is_empty() {
            self.body.push_str("no matches\n");
        }
        ToolOutput {
            content: self.body,
            is_error: false,
            truncated: self.truncated,
        }
    }
}
fn ls(root: &Root, args: &LsArgs, cancel: &CancellationToken) -> Result<ToolOutput> {
    let dir = path(&args.path)?;
    let _guard = platform_fs(root.lock())?;
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let listing = platform_fs(root.list(dir))?;
    let mut entries = listing.entries;
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    let mut output = BoundedOutput::new();
    for (i, e) in entries.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if i >= args.limit {
            output.truncated = true;
            break;
        }
        let suffix = if e.kind == EntryKind::Directory {
            "/"
        } else {
            ""
        };
        if !output.line(&format!("{}{suffix}", e.name)) {
            break;
        }
    }
    output.truncated |= listing.incomplete;
    Ok(output.finish("ls"))
}

// Rules are always parsed from descriptor-rooted read bytes. Never call the
// ignore crate's filesystem walker or GitignoreBuilder::add on an absolute path.
struct IgnoreLevel {
    rules: Option<ignore::gitignore::Gitignore>,
    parent: Option<Arc<IgnoreLevel>>,
}
impl IgnoreLevel {
    fn ignored(&self, path: &str, is_dir: bool) -> bool {
        let mut current = Some(self);
        while let Some(level) = current {
            if let Some(matcher) = &level.rules {
                match matcher.matched(Path::new(path), is_dir) {
                    Match::Ignore(_) => return true,
                    Match::Whitelist(_) => return false,
                    Match::None => {}
                }
            }
            current = level.parent.as_deref();
        }
        false
    }
}
fn ignore_level(
    root: &Root,
    dir: &str,
    parent: Option<Arc<IgnoreLevel>>,
) -> Result<Arc<IgnoreLevel>> {
    let file = if dir.is_empty() {
        ".gitignore".into()
    } else {
        format!("{dir}/.gitignore")
    };
    let contents = match platform_fs(root.read(&file)) {
        Ok(contents) => Some(contents),
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => {
            // Fail closed if existing ignore rules cannot be inspected.
            let listing = platform_fs(root.list(dir))?;
            if listing.incomplete || listing.entries.iter().any(|item| item.name == ".gitignore") {
                return Err(Error::Denied("unreadable .gitignore in search root"));
            }
            None
        }
    };
    let rules = if let Some(content) = contents {
        if content.len() > MAX_IGNORE_BYTES || content.lines().count() > MAX_IGNORE_LINES {
            return Err(Error::Limit("search .gitignore exceeds permitted bounds"));
        }
        let mut builder = GitignoreBuilder::new(dir);
        for line in content.lines() {
            builder
                .add_line(None, line)
                .map_err(|_| Error::Denied("invalid .gitignore pattern"))?;
        }
        Some(
            builder
                .build()
                .map_err(|_| Error::Denied("invalid .gitignore matcher"))?,
        )
    } else {
        None
    };
    Ok(Arc::new(IgnoreLevel { rules, parent }))
}
struct Directory {
    path: String,
    depth: usize,
    ignore: Option<Arc<IgnoreLevel>>,
}
// A path-scoped search still respects .gitignore files in every ancestor.
// Load them through Root::read, not through the ambient OS path namespace.
fn parent_ignore_rules(root: &Root, start: &str) -> Result<Option<Arc<IgnoreLevel>>> {
    let mut chain = None;
    let mut parent = String::new();
    for component in start.split('/').filter(|component| !component.is_empty()) {
        chain = Some(ignore_level(root, &parent, chain)?);
        if !parent.is_empty() {
            parent.push('/');
        }
        parent.push_str(component);
    }
    Ok(chain)
}
// Returns true when truncated/terminated by the callback.
fn walk(
    root: &Root,
    start: &str,
    cancel: &CancellationToken,
    mut visit: impl FnMut(&str, &str, EntryKind) -> Result<bool>,
) -> Result<bool> {
    let mut stack = vec![Directory {
        path: start.to_owned(),
        depth: 0,
        ignore: parent_ignore_rules(root, start)?,
    }];
    let mut seen = 0usize;
    let deadline = Instant::now() + Duration::from_secs(10);
    while let Some(dir) = stack.pop() {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if Instant::now() >= deadline || dir.depth > MAX_DEPTH || seen >= MAX_ENTRIES {
            return Ok(true);
        }
        let rules = ignore_level(root, &dir.path, dir.ignore)?;
        let listing = platform_fs(root.list(&dir.path))?;
        let mut entries = listing.entries;
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        let mut child_dirs = Vec::new();
        for entry in entries {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            seen += 1;
            if seen > MAX_ENTRIES || Instant::now() >= deadline {
                return Ok(true);
            }
            if entry.name == ".git"
                || rules.ignored(&entry.path, entry.kind == EntryKind::Directory)
            {
                continue;
            }
            if visit(&entry.path, &entry.name, entry.kind.clone())? {
                return Ok(true);
            }
            if entry.kind == EntryKind::Directory {
                child_dirs.push(entry.path);
            }
        }
        if listing.incomplete {
            return Ok(true);
        }
        for child in child_dirs.into_iter().rev() {
            stack.push(Directory {
                path: child,
                depth: dir.depth + 1,
                ignore: Some(rules.clone()),
            });
        }
    }
    Ok(false)
}
fn target<'a>(full: &'a str, name: &'a str, start: &str, pattern: &str) -> &'a str {
    if !pattern.contains('/') {
        return name;
    }
    if start.is_empty() {
        full
    } else {
        full.strip_prefix(start)
            .and_then(|p| p.strip_prefix('/'))
            .unwrap_or(full)
    }
}
fn find(root: &Root, args: &FindArgs, cancel: &CancellationToken) -> Result<ToolOutput> {
    let start = path(&args.path)?;
    let glob = glob(&args.pattern)?;
    let mut output = BoundedOutput::new();
    let mut found = 0usize;
    let _guard = platform_fs(root.lock())?;
    if !start.is_empty() && root.list(start).is_err() {
        // Match an explicitly selected regular file without requiring its
        // contents to be UTF-8; all metadata comes from scoped parent listing.
        let (parent, name) = start.rsplit_once('/').unwrap_or(("", start));
        let listing = platform_fs(root.list(parent))?;
        if !listing
            .entries
            .iter()
            .any(|entry| entry.name == name && entry.kind == EntryKind::File)
        {
            return Err(Error::Denied(
                "find path is not a regular file or directory",
            ));
        }
        if glob.is_match(target(start, name, start, &args.pattern)) {
            output.line(start);
        }
        return Ok(output.finish("find"));
    }
    output.truncated = walk(root, start, cancel, |full, name, kind| {
        if kind == EntryKind::Unsupported {
            return Ok(false);
        }
        if glob.is_match(target(full, name, start, &args.pattern)) {
            if found >= args.limit {
                return Ok(true);
            }
            if !output.line(full) {
                return Ok(true);
            }
            found += 1;
        }
        Ok(false)
    })?;
    Ok(output.finish("find"))
}
fn grep_lines(
    output: &mut BoundedOutput,
    file: &str,
    text: &str,
    re: &Regex,
    context: usize,
    limit: usize,
    count: &mut usize,
) -> bool {
    let lines: Vec<_> = text.lines().collect();
    let mut included_to = 0usize;
    for (i, line) in lines.iter().enumerate() {
        if !re.is_match(line) {
            continue;
        }
        if *count >= limit {
            output.truncated = true;
            return true;
        }
        *count += 1;
        let begin = i.saturating_sub(context).max(included_to);
        let end = (i + context + 1).min(lines.len());
        for (offset, line) in lines[begin..end].iter().enumerate() {
            let display = if line.chars().count() > 500 {
                format!("{}...", line.chars().take(500).collect::<String>())
            } else {
                (*line).to_owned()
            };
            if !output.line(&format!("{file}:{}:{display}", begin + offset + 1)) {
                return true;
            }
        }
        included_to = end;
    }
    false
}
fn grep(root: &Root, args: &GrepArgs, cancel: &CancellationToken) -> Result<ToolOutput> {
    let start = path(&args.path)?;
    let re = regex(args)?;
    let filter = args.glob.as_deref().map(glob).transpose()?;
    let mut output = BoundedOutput::new();
    let mut matches = 0usize;
    let _guard = platform_fs(root.lock())?;
    // If a precise file was requested, inspect just that file. Otherwise the
    // exact path must be a real directory under the descriptor root.
    if !start.is_empty() {
        if let Ok(content) = platform_fs(root.read(start)) {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            grep_lines(
                &mut output,
                start,
                &content,
                &re,
                args.context,
                args.limit,
                &mut matches,
            );
            return Ok(output.finish("grep"));
        }
    }
    output.truncated = walk(root, start, cancel, |full, name, kind| {
        if kind != EntryKind::File {
            return Ok(false);
        }
        if let (Some(filter), Some(pattern)) = (&filter, args.glob.as_deref()) {
            if !filter.is_match(target(full, name, start, pattern)) {
                return Ok(false);
            }
        }
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        match platform_fs(root.read(full)) {
            Ok(content) => {
                if grep_lines(
                    &mut output,
                    full,
                    &content,
                    &re,
                    args.context,
                    args.limit,
                    &mut matches,
                ) {
                    return Ok(true);
                }
            }
            Err(Error::Io(_)) | Err(Error::Denied(_)) | Err(Error::Limit(_)) => {
                // Text-only, <=1MiB file. Unsupported entries never bypass Root.
            }
            Err(error) => return Err(error),
        }
        Ok(false)
    })?;
    Ok(output.finish("grep"))
}
impl Tool for Search {
    fn spec(&self) -> ToolSpec {
        let (description, parameters) = match self.operation {
            Operation::Ls => (
                "List one granted directory alphabetically; no-follow and bounded. Set parallel=true only when independent from adjacent tool calls.",
                json!({"type":"object","properties":{"parallel":{"type":"boolean","description":"Model scheduling choice: true allows overlap with adjacent independent reads; false/omit for ordered reads."},"path":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":512}},"additionalProperties":false}),
            ),
            Operation::Find => (
                "Find paths by glob using scoped no-index filesystem traversal. Set parallel=true when safe to overlap with adjacent reads.",
                json!({"type":"object","properties":{"parallel":{"type":"boolean","description":"Model scheduling choice: true allows overlap with adjacent independent reads; false/omit for ordered reads."},"pattern":{"type":"string"},"path":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":200}},"required":["pattern"],"additionalProperties":false}),
            ),
            Operation::Grep => (
                "Search UTF-8 files with bounded Rust regex, glob and context. Set parallel=true for independent reads only.",
                json!({"type":"object","properties":{"parallel":{"type":"boolean","description":"Model scheduling choice: true allows overlap with adjacent independent reads; false/omit for ordered reads."},"pattern":{"type":"string"},"path":{"type":"string"},"glob":{"type":"string"},"ignore_case":{"type":"boolean"},"literal":{"type":"boolean"},"context":{"type":"integer","minimum":0,"maximum":8},"limit":{"type":"integer","minimum":1,"maximum":200}},"required":["pattern"],"additionalProperties":false}),
            ),
        };
        ToolSpec {
            name: self.operation.name().into(),
            description: description.into(),
            parameters,
            effect: ToolEffect::Read,
        }
    }
    fn validate(&self, value: &Value) -> Result<()> {
        validate(self.operation, value)
    }
    fn execute<'a>(&'a self, ctx: ToolContext, args: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(args)?;
            let request = args.clone();
            let op = self.operation;
            let root = self.root.clone();
            let cancel = ctx.cancellation;
            tokio::task::spawn_blocking(move || {
                let outcome = match op {
                    Operation::Ls => ls(&root, &parse::<LsArgs>(&request)?, &cancel),
                    Operation::Find => find(&root, &parse::<FindArgs>(&request)?, &cancel),
                    Operation::Grep => grep(&root, &parse::<GrepArgs>(&request)?, &cancel),
                };
                match outcome {
                    Ok(value) => Ok(value),
                    Err(Error::Io(_)) | Err(Error::Denied(_)) => Ok(ToolOutput::error(
                        "Search path or ignore rules unavailable under granted root",
                    )),
                    Err(error) => Err(error),
                }
            })
            .await
            .map_err(|_| Error::Protocol("search worker failed"))?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_scoped_find_grep_ls_obey_ignore_and_no_follow() {
        use std::fs;
        let root = std::env::temp_dir().join(format!(
            "aw-r2-search-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join(".gitignore"), "ignored.txt\nsrc/skip.rs\n").unwrap();
        fs::write(root.join("src/main.rs"), "pub fn hello() {}\n// TOKEN\n").unwrap();
        fs::write(root.join("src/skip.rs"), "// TOKEN IGNORE\n").unwrap();
        fs::write(root.join("ignored.txt"), "TOKEN IGNORED\n").unwrap();
        fs::write(root.join("safe.txt"), "TOKEN VISIBLE\n").unwrap();
        fs::write(root.join("raw.bin"), [0xff, 0xfe, 0xfd]).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc", root.join("outside")).unwrap();
            fs::write(root.join("hard-target.txt"), "HARD SECRET\n").unwrap();
            fs::hard_link(root.join("hard-target.txt"), root.join("hardlink")).unwrap();
        }
        let scoped = Root::open(&root).unwrap();
        let token = CancellationToken::new();
        let listing = ls(
            &scoped,
            &LsArgs {
                _parallel: false,
                path: ".".into(),
                limit: 100,
            },
            &token,
        )
        .unwrap();
        assert!(listing.content.contains("src/"));
        assert!(listing.content.contains(".gitignore"));
        let results = find(
            &scoped,
            &FindArgs {
                _parallel: false,
                pattern: "**/*.rs".into(),
                path: "".into(),
                limit: 100,
            },
            &token,
        )
        .unwrap();
        assert!(
            results.content.contains("src/main.rs"),
            "{}",
            results.content
        );
        assert!(!results.content.contains("skip.rs"), "{}", results.content);
        let scoped_find = find(
            &scoped,
            &FindArgs {
                _parallel: false,
                pattern: "*.rs".into(),
                path: "src".into(),
                limit: 100,
            },
            &token,
        )
        .unwrap();
        assert!(scoped_find.content.contains("src/main.rs"));
        assert!(!scoped_find.content.contains("skip.rs"));
        let single_find = find(
            &scoped,
            &FindArgs {
                _parallel: false,
                pattern: "*.rs".into(),
                path: "src/main.rs".into(),
                limit: 100,
            },
            &token,
        )
        .unwrap();
        assert_eq!(single_find.content.trim(), "src/main.rs");
        let grep_results = grep(
            &scoped,
            &GrepArgs {
                _parallel: false,
                pattern: "token".into(),
                path: "".into(),
                glob: None,
                ignore_case: true,
                literal: true,
                context: 0,
                limit: 100,
            },
            &token,
        )
        .unwrap();
        assert!(grep_results.content.contains("src/main.rs:2:// TOKEN"));
        assert!(grep_results.content.contains("safe.txt:1:TOKEN VISIBLE"));
        assert!(!grep_results.content.contains("IGNORE"));
        assert!(!grep_results.content.contains("IGNORED"));
        assert!(!grep_results.content.contains("hardlink"));
        assert!(!grep_results.content.contains("outside"));
        let precise = grep(
            &scoped,
            &GrepArgs {
                _parallel: false,
                pattern: "hello".into(),
                path: "src/main.rs".into(),
                glob: None,
                ignore_case: false,
                literal: false,
                context: 0,
                limit: 100,
            },
            &token,
        )
        .unwrap();
        assert!(precise.content.contains("src/main.rs:1:pub fn hello() {}"));
        #[cfg(unix)]
        assert!(grep(
            &scoped,
            &GrepArgs {
                _parallel: false,
                pattern: "SECRET".into(),
                path: "outside".into(),
                glob: None,
                ignore_case: false,
                literal: true,
                context: 0,
                limit: 100,
            },
            &token
        )
        .is_err());
        fs::write(root.join("src/another.rs"), "pub fn another() {}\n").unwrap();
        let truncated = find(
            &scoped,
            &FindArgs {
                _parallel: false,
                pattern: "**/*.rs".into(),
                path: "".into(),
                limit: 1,
            },
            &token,
        )
        .unwrap();
        assert!(truncated.truncated, "{}", truncated.content);
        assert!(truncated.content.contains("[find truncated]"));
        token.cancel();
        assert!(matches!(
            grep(
                &scoped,
                &GrepArgs {
                    _parallel: false,
                    pattern: "TOKEN".into(),
                    path: "".into(),
                    glob: None,
                    ignore_case: false,
                    literal: true,
                    context: 0,
                    limit: 10,
                },
                &token
            ),
            Err(Error::Cancelled)
        ));
        drop(scoped);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bounded_schema() {
        assert!(validate(Operation::Ls, &json!({})).is_ok());
        assert!(validate(Operation::Ls, &json!({"path":"."})).is_ok());
        assert!(validate(Operation::Ls, &json!({"path":"../etc"})).is_err());
        assert!(validate(Operation::Find, &json!({"pattern":"**/*.rs"})).is_ok());
        assert!(validate(Operation::Find, &json!({"pattern":""})).is_err());
        assert!(validate(Operation::Find, &json!({"pattern":"*.rs","limit":0})).is_err());
        assert!(validate(Operation::Grep, &json!({"pattern":"f(o)+"})).is_ok());
        assert!(validate(Operation::Grep, &json!({"pattern":"(","literal":false})).is_err());
        assert!(validate(Operation::Grep, &json!({"pattern":"(","literal":true})).is_ok());
        assert!(validate(Operation::Grep, &json!({"pattern":"x","context":9})).is_err());
        assert!(validate(Operation::Grep, &json!({"pattern":"x","unexpected":1})).is_err());
    }
}
