//! Transport-independent search work/reply accounting.
//! Filesystem walking and regex engines remain host adapters until both
//! repositories agree on a single capability-rooted file reader.

use crate::filesystem::{
    self,
    backend::MetadataBackend,
    walk::{self, BackendErrorKind},
    Kind,
};
use globset::{GlobBuilder, GlobMatcher};
use regex::{Regex, RegexBuilder};
use std::{
    collections::{BTreeSet, BinaryHeap},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct Incomplete {
    pub directory_budget: bool,
    pub content_budget: bool,
    pub candidate_budget: bool,
    pub traversal_error: bool,
    pub ignore_rules: bool,
    pub path_encoding: bool,
    pub unreadable_files: bool,
    pub oversize_files: bool,
}
impl Incomplete {
    pub fn any(self) -> bool {
        self.directory_budget
            || self.content_budget
            || self.candidate_budget
            || self.traversal_error
            || self.ignore_rules
            || self.path_encoding
            || self.unreadable_files
            || self.oversize_files
    }
    /// Stable cause priority, retaining every simultaneous cause.
    pub fn reasons(self) -> Vec<&'static str> {
        let mut causes = Vec::new();
        if self.content_budget {
            causes.push("content_budget");
        }
        if self.directory_budget {
            causes.push("directory_budget");
        }
        if self.candidate_budget {
            causes.push("candidate_budget");
        }
        if self.traversal_error {
            causes.push("traversal_error");
        }
        if self.ignore_rules {
            causes.push("ignore_rules");
        }
        if self.path_encoding {
            causes.push("path_encoding");
        }
        if self.unreadable_files {
            causes.push("unreadable_files");
        }
        if self.oversize_files {
            causes.push("oversize_files");
        }
        causes
    }
    pub fn first(self) -> Option<&'static str> {
        self.reasons().first().copied()
    }
}
/// DL-150 compact public reasons. Internal causes remain precise for
/// debugging and structured host logs, but models see a stable small set.
pub fn compact_reasons(causes: Incomplete) -> Option<String> {
    let mut labels = Vec::with_capacity(4);
    if causes.content_budget
        || causes.directory_budget
        || causes.candidate_budget
        || causes.oversize_files
    {
        labels.push("budget");
    }
    if causes.traversal_error || causes.unreadable_files {
        labels.push("walk");
    }
    if causes.ignore_rules {
        labels.push("gitignore");
    }
    if causes.path_encoding {
        labels.push("path_encoding");
    }
    (!labels.is_empty()).then(|| labels.join(","))
}

pub fn compact_incomplete(causes: Incomplete) -> Option<String> {
    compact_reasons(causes).map(|reasons| format!("… incomplete:{reasons}\n"))
}
pub struct WorkBudget {
    deadline: Instant,
    remaining_bytes: usize,
    remaining_files: usize,
}
impl WorkBudget {
    pub fn new(duration: Duration, bytes: usize, files: usize) -> Self {
        Self {
            deadline: Instant::now()
                .checked_add(duration)
                .unwrap_or_else(Instant::now),
            remaining_bytes: bytes,
            remaining_files: files,
        }
    }
    /// Admission and actual byte accounting are distinct: files can grow
    /// after metadata inspection, but may never exceed this request's budget.
    pub fn begin_file(&mut self, declared_size: usize) -> bool {
        if Instant::now() >= self.deadline
            || self.remaining_files == 0
            || declared_size > self.remaining_bytes
        {
            return false;
        }
        self.remaining_files -= 1;
        true
    }
    pub fn charge(&mut self, actual_bytes: usize) -> bool {
        if actual_bytes > self.remaining_bytes {
            return false;
        }
        self.remaining_bytes -= actual_bytes;
        true
    }
    /// An explicitly skipped path still consumes its source budget when
    /// inspected, but does not carry bytes into future pages.
    pub fn admit(&mut self, file_size: usize) -> bool {
        self.begin_file(file_size) && self.charge(file_size)
    }
    pub fn remaining_files(&self) -> usize {
        self.remaining_files
    }
    pub fn remaining_bytes(&self) -> usize {
        self.remaining_bytes
    }
}
/// Ordinary paths remain unchanged; ambiguous grammar characters are quoted.
pub fn content_path_label(path: &str) -> String {
    if path.contains(':')
        || path.contains('\n')
        || path.contains('\r')
        || path.contains('\\')
        || path.contains('"')
    {
        serde_json::to_string(path).unwrap_or_else(|_| "<invalid-path>".into())
    } else {
        path.into()
    }
}

pub const MAX_GREP_HITS: usize = 200;
pub const MAX_GREP_CONTEXT: usize = 8;
pub const MAX_GREP_PATTERN: usize = 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GrepMode {
    #[default]
    Content,
    Files,
    Count,
}

#[derive(Clone, Copy, Debug)]
pub struct GrepLimits {
    pub candidates: usize,
    pub scan_bytes: usize,
    pub scan_files: usize,
    pub text_file_bytes: usize,
    pub output_bytes: usize,
    pub duration: Duration,
}
impl Default for GrepLimits {
    fn default() -> Self {
        Self {
            candidates: 4096,
            scan_bytes: 32 * 1024 * 1024,
            scan_files: 4096,
            text_file_bytes: 1024 * 1024,
            output_bytes: 96 * 1024,
            duration: Duration::from_secs(15),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GrepOptions<'a> {
    pub pattern: &'a str,
    /// Model-facing requested root/path.
    pub path: &'a str,
    /// Resolved authorized backend root/path. None means identical to `path`.
    pub backend_path: Option<&'a str>,
    pub glob: Option<&'a str>,
    pub depth: usize,
    pub literal: bool,
    pub ignore_case: bool,
    pub mode: GrepMode,
    /// Internal compatibility can be asymmetric; the canonical schema passes
    /// the same bounded `context` value for both fields.
    pub before: usize,
    pub after: usize,
    pub hidden: bool,
    pub gitignore: bool,
    pub offset: usize,
    pub limit: usize,
    pub limits: GrepLimits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryError {
    InvalidPattern,
    InvalidGlob,
    InvalidDepth,
    InvalidContext,
    InvalidPage,
    InvalidLimits,
}

#[derive(Debug)]
pub enum GrepError<E> {
    Query(QueryError),
    Source(E),
    Walk(walk::WalkError<E>),
    Cancelled,
    ResultLimit(&'static str),
}

#[derive(Debug)]
pub struct GrepPage {
    pub content: String,
    pub truncated: bool,
    pub incomplete: Incomplete,
}

struct CompiledQuery {
    regex: Regex,
    glob: Option<(GlobMatcher, bool)>,
}

fn compile_query(options: &GrepOptions<'_>) -> Result<CompiledQuery, QueryError> {
    if options.pattern.is_empty()
        || options.pattern.len() > MAX_GREP_PATTERN
        || options.pattern.chars().any(char::is_control)
    {
        return Err(QueryError::InvalidPattern);
    }
    if options.depth == 0 || options.depth > filesystem::MAX_WALK_DEPTH {
        return Err(QueryError::InvalidDepth);
    }
    if options.before > MAX_GREP_CONTEXT || options.after > MAX_GREP_CONTEXT {
        return Err(QueryError::InvalidContext);
    }
    if !(1..=MAX_GREP_HITS).contains(&options.limit)
        || options.offset.checked_add(options.limit).is_none()
    {
        return Err(QueryError::InvalidPage);
    }
    let limits = options.limits;
    if limits.candidates == 0
        || limits.scan_bytes == 0
        || limits.scan_files == 0
        || limits.text_file_bytes == 0
        || limits.output_bytes == 0
        || limits.duration.is_zero()
    {
        return Err(QueryError::InvalidLimits);
    }

    let expression = if options.literal {
        regex::escape(options.pattern)
    } else {
        options.pattern.to_owned()
    };
    let regex = RegexBuilder::new(&expression)
        .case_insensitive(options.ignore_case)
        .size_limit(1 << 20)
        .dfa_size_limit(1 << 20)
        .build()
        .map_err(|_| QueryError::InvalidPattern)?;
    let glob = options
        .glob
        .map(|pattern| {
            if pattern.is_empty()
                || pattern.len() > MAX_GREP_PATTERN
                || pattern.chars().any(char::is_control)
            {
                return Err(QueryError::InvalidGlob);
            }
            GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
                .map(|glob| (glob.compile_matcher(), pattern.contains('/')))
                .map_err(|_| QueryError::InvalidGlob)
        })
        .transpose()?;
    Ok(CompiledQuery { regex, glob })
}

pub fn validate_grep(options: &GrepOptions<'_>) -> Result<(), QueryError> {
    compile_query(options).map(|_| ())
}

fn matches_path(glob: &(GlobMatcher, bool), entry: &str, name: &str, start: &str) -> bool {
    let relative = if start.is_empty() {
        entry
    } else {
        entry
            .strip_prefix(start)
            .and_then(|rest| rest.strip_prefix('/'))
            .unwrap_or(entry)
    };
    glob.0.is_match(if glob.1 { relative } else { name })
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

fn hidden_path(path: &str) -> bool {
    path.split('/').any(|part| part.starts_with('.'))
}

fn line(path: &str, number: usize, text: &str, primary: bool) -> String {
    let label = content_path_label(path);
    if primary {
        format!("{label}:{number}:{text}\n")
    } else {
        format!("{label}-{number}-{text}\n")
    }
}

struct ContextGroup {
    text: String,
    primaries: usize,
}

fn context_groups(
    path: &str,
    lines: &[&str],
    all_hits: &[usize],
    selected_hits: &[usize],
    before: usize,
    after: usize,
) -> Vec<ContextGroup> {
    if selected_hits.is_empty() {
        return Vec::new();
    }
    let selected = selected_hits.iter().copied().collect::<BTreeSet<_>>();
    let all = all_hits.iter().copied().collect::<BTreeSet<_>>();
    let mut intervals = Vec::<(usize, usize)>::new();
    for &hit in selected_hits {
        let start = hit.saturating_sub(before);
        let end = (hit + after + 1).min(lines.len());
        if let Some(last) = intervals.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        intervals.push((start, end));
    }

    let context_enabled = before != 0 || after != 0;
    intervals
        .into_iter()
        .map(|(start, end)| {
            let mut text = String::new();
            let mut previous_visible = None;
            let mut primaries = 0usize;
            for (row, source) in lines.iter().enumerate().take(end).skip(start) {
                // A matching line outside this logical page is not allowed to
                // leak in as free context. Its omission creates a real gap.
                if all.contains(&row) && !selected.contains(&row) {
                    previous_visible = None;
                    continue;
                }
                if context_enabled
                    && previous_visible.is_some_and(|previous| row != previous + 1)
                    && !text.is_empty()
                {
                    text.push_str("--\n");
                }
                let primary = selected.contains(&row);
                primaries += usize::from(primary);
                text.push_str(&line(path, row + 1, source, primary));
                previous_visible = Some(row);
            }
            ContextGroup { text, primaries }
        })
        .collect()
}

fn progress_flags(progress: walk::Progress) -> Incomplete {
    Incomplete {
        directory_budget: progress.directory_budget,
        traversal_error: progress.traversal_error,
        ignore_rules: progress.ignore_rules,
        path_encoding: progress.path_encoding,
        ..Incomplete::default()
    }
}

fn model_path_for(backend_path: &str, backend_root: &str, model_root: &str) -> String {
    if backend_path == backend_root {
        return model_root.to_owned();
    }
    let relative = if backend_root.is_empty() {
        backend_path
    } else {
        backend_path
            .strip_prefix(backend_root)
            .and_then(|rest| rest.strip_prefix('/'))
            .unwrap_or(backend_path)
    };
    if model_root.is_empty() {
        relative.to_owned()
    } else if relative.is_empty() {
        model_root.to_owned()
    } else {
        format!("{model_root}/{relative}")
    }
}

pub fn grep<B: MetadataBackend>(
    backend: &B,
    options: GrepOptions<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<GrepPage, GrepError<B::Error>> {
    let query = compile_query(&options).map_err(GrepError::Query)?;
    let backend_root = options.backend_path.unwrap_or(options.path);
    let mut candidates = BinaryHeap::<String>::new();
    let mut candidate_capped = false;
    let mut flags = Incomplete::default();

    let root_kind = if backend_root.is_empty() {
        Kind::Dir
    } else {
        backend
            .entry_meta(backend_root)
            .map_err(GrepError::Source)?
            .kind
    };

    match root_kind {
        Kind::File => {
            if !options.hidden && hidden_path(options.path) {
                // Explicit hidden compatibility path is filtered like a walk.
            } else {
                let ignored = if options.gitignore {
                    let (ignored, unavailable) =
                        walk::explicitly_ignored(backend, options.path, false)
                            .map_err(GrepError::Walk)?;
                    flags.ignore_rules |= unavailable;
                    ignored
                } else {
                    false
                };
                let name = options.path.rsplit('/').next().unwrap_or(options.path);
                if !ignored
                    && query
                        .glob
                        .as_ref()
                        .is_none_or(|glob| matches_path(glob, options.path, name, ""))
                {
                    candidates.push(backend_root.to_owned());
                }
            }
        }
        Kind::Dir => {
            let progress = walk::walk(
                backend,
                backend_root,
                options.depth,
                options.hidden,
                options.gitignore,
                cancelled,
                |entry| {
                    if entry.kind == Kind::File
                        && query.glob.as_ref().is_none_or(|glob| {
                            matches_path(glob, &entry.path, &entry.name, backend_root)
                        })
                    {
                        candidate_capped |= retain_best(
                            &mut candidates,
                            entry.path.clone(),
                            options.limits.candidates,
                        );
                    }
                    Ok(false)
                },
            )
            .map_err(GrepError::Walk)?;
            flags = progress_flags(progress);
        }
        _ => return Err(GrepError::Query(QueryError::InvalidDepth)),
    }

    flags.candidate_budget |= candidate_capped;
    let mut candidates = candidates.into_sorted_vec();
    candidates.dedup();
    let mut work = WorkBudget::new(
        options.limits.duration,
        options.limits.scan_bytes,
        options.limits.scan_files,
    );
    let mut logical_seen = 0usize;
    let mut selected_total = 0usize;
    let mut emitted = 0usize;
    let mut more = false;
    let mut out = String::new();
    let mut file_rows = Vec::<(String, usize)>::new();
    let reserve = 128usize.min(options.limits.output_bytes);

    'files: for file in candidates {
        if cancelled() {
            return Err(GrepError::Cancelled);
        }
        if work.remaining_bytes() == 0 || work.remaining_files() == 0 {
            flags.content_budget = true;
            break;
        }
        let meta = match backend.entry_meta(&file) {
            Ok(meta) if meta.kind == Kind::File => meta,
            Ok(_) => {
                flags.unreadable_files = true;
                continue;
            }
            Err(_) => {
                flags.unreadable_files = true;
                continue;
            }
        };
        let Some(size) = meta.size.and_then(|size| usize::try_from(size).ok()) else {
            flags.unreadable_files = true;
            continue;
        };
        if size > options.limits.text_file_bytes {
            if root_kind == Kind::File {
                return Err(GrepError::ResultLimit(
                    "grep file exceeds text-file ceiling; use read or bread",
                ));
            }
            flags.oversize_files = true;
            continue;
        }
        if !work.begin_file(size) {
            flags.content_budget = true;
            break;
        }
        let bytes = match backend.read_bytes(
            &file,
            options.limits.text_file_bytes.min(work.remaining_bytes()),
        ) {
            Ok(bytes) => bytes,
            Err(error) if backend.classify(&error) == BackendErrorKind::Limit => {
                flags.content_budget = true;
                break;
            }
            Err(_) => {
                flags.unreadable_files = true;
                continue;
            }
        };
        if !work.charge(bytes.len()) {
            flags.content_budget = true;
            break;
        }
        let Some(text) = crate::text::text_bytes(&bytes) else {
            // Binary/control/invalid-UTF8 files are outside grep's text domain,
            // not incomplete coverage.
            continue;
        };
        let lines = text.lines().collect::<Vec<_>>();
        let hits = lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| query.regex.is_match(line).then_some(index))
            .collect::<Vec<_>>();
        if hits.is_empty() {
            continue;
        }
        let display_file = model_path_for(&file, backend_root, options.path);

        match options.mode {
            GrepMode::Files | GrepMode::Count => {
                if logical_seen < options.offset {
                    logical_seen += 1;
                    continue;
                }
                if selected_total >= options.limit {
                    more = true;
                    break;
                }
                logical_seen += 1;
                selected_total += 1;
                file_rows.push((display_file, hits.len()));
            }
            GrepMode::Content => {
                let mut selected = Vec::new();
                for hit in hits.iter().copied() {
                    if logical_seen < options.offset {
                        logical_seen += 1;
                        continue;
                    }
                    if selected_total >= options.limit {
                        more = true;
                        break;
                    }
                    logical_seen += 1;
                    selected_total += 1;
                    selected.push(hit);
                }
                for group in context_groups(
                    &display_file,
                    &lines,
                    &hits,
                    &selected,
                    options.before,
                    options.after,
                ) {
                    let separator = (options.before != 0 || options.after != 0) && !out.is_empty();
                    let needed = group.text.len() + if separator { 3 } else { 0 };
                    if out.len().saturating_add(needed).saturating_add(reserve)
                        > options.limits.output_bytes
                    {
                        if out.is_empty() {
                            return Err(GrepError::ResultLimit(
                                "grep hit/context exceeds result ceiling; use context=0, read, or narrow path",
                            ));
                        }
                        more = true;
                        break 'files;
                    }
                    if separator {
                        out.push_str("--\n");
                    }
                    out.push_str(&group.text);
                    emitted += group.primaries;
                }
                if more {
                    break;
                }
            }
        }
    }

    if matches!(options.mode, GrepMode::Files | GrepMode::Count) {
        for (path, count) in file_rows {
            let row = match options.mode {
                GrepMode::Files => format!("{}\n", filesystem::safe_label(&path)),
                GrepMode::Count => format!("{count}  {}\n", filesystem::safe_label(&path)),
                GrepMode::Content => unreachable!(),
            };
            if out.len().saturating_add(row.len()).saturating_add(reserve)
                > options.limits.output_bytes
            {
                if out.is_empty() {
                    return Err(GrepError::ResultLimit(
                        "first grep result exceeds result ceiling; narrow path",
                    ));
                }
                more = true;
                break;
            }
            out.push_str(&row);
            emitted += 1;
        }
    }

    let next = more.then(|| options.offset + emitted);
    let reasons = compact_reasons(flags);
    if let Some(status) = filesystem::status::query(next, reasons.as_deref()) {
        if out.len() + status.len() > options.limits.output_bytes {
            return Err(GrepError::ResultLimit(
                "grep status exceeds result ceiling; narrow query",
            ));
        }
        out.push_str(&status);
    }
    Ok(GrepPage {
        content: out,
        truncated: more || flags.any(),
        incomplete: flags,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_concurrent_stop_reason_survives() {
        let i = Incomplete {
            directory_budget: true,
            ignore_rules: true,
            unreadable_files: true,
            ..Incomplete::default()
        };
        assert_eq!(
            i.reasons(),
            ["directory_budget", "ignore_rules", "unreadable_files"]
        );
        assert_eq!(i.first(), Some("directory_budget"));
    }
    #[test]
    fn work_limits_do_not_skip_an_unaccounted_file() {
        let mut w = WorkBudget::new(Duration::from_secs(10), 100, 2);
        assert!(w.admit(60));
        assert!(!w.admit(41));
        assert_eq!(w.remaining_bytes(), 40);
        assert!(w.admit(40));
        assert!(!w.admit(0));
    }
    #[derive(Default)]
    struct FakeFs {
        dirs: std::collections::BTreeMap<String, Vec<walk::WalkEntry>>,
        files: std::collections::BTreeMap<String, Vec<u8>>,
    }
    impl walk::Backend for FakeFs {
        type Error = &'static str;
        fn list(&self, path: &str, _: usize) -> Result<walk::Listing, Self::Error> {
            self.dirs
                .get(path)
                .cloned()
                .map(|entries| walk::Listing {
                    entries,
                    incomplete: false,
                    path_encoding: false,
                })
                .ok_or("missing")
        }
        fn read_bytes(&self, path: &str, max: usize) -> Result<Vec<u8>, Self::Error> {
            let bytes = self.files.get(path).ok_or("missing")?;
            if bytes.len() > max {
                Err("limit")
            } else {
                Ok(bytes.clone())
            }
        }
        fn classify(&self, error: &&'static str) -> BackendErrorKind {
            match *error {
                "missing" => BackendErrorKind::Missing,
                "limit" => BackendErrorKind::Limit,
                _ => BackendErrorKind::Other,
            }
        }
    }
    impl MetadataBackend for FakeFs {
        fn entry_meta(&self, path: &str) -> Result<filesystem::EntryMeta, Self::Error> {
            if self.dirs.contains_key(path) || (path.is_empty() && self.dirs.contains_key("")) {
                return Ok(filesystem::EntryMeta {
                    kind: Kind::Dir,
                    ..Default::default()
                });
            }
            self.files
                .get(path)
                .map(|bytes| filesystem::EntryMeta {
                    kind: Kind::File,
                    size: Some(bytes.len() as u64),
                    ..Default::default()
                })
                .ok_or("missing")
        }
    }
    fn file(name: &str, path: &str) -> walk::WalkEntry {
        walk::WalkEntry {
            name: name.into(),
            path: path.into(),
            kind: Kind::File,
            hidden: false,
        }
    }
    fn dir(name: &str, path: &str) -> walk::WalkEntry {
        walk::WalkEntry {
            name: name.into(),
            path: path.into(),
            kind: Kind::Dir,
            hidden: false,
        }
    }
    fn options<'a>(pattern: &'a str) -> GrepOptions<'a> {
        GrepOptions {
            pattern,
            path: "",
            backend_path: None,
            glob: None,
            depth: 128,
            literal: true,
            ignore_case: false,
            mode: GrepMode::Content,
            before: 0,
            after: 0,
            hidden: false,
            gitignore: false,
            offset: 0,
            limit: 200,
            limits: GrepLimits {
                output_bytes: 4096,
                ..GrepLimits::default()
            },
        }
    }
    #[test]
    fn canonical_grep_respects_depth_and_coalesces_context() {
        let fs = FakeFs {
            dirs: std::collections::BTreeMap::from([
                (
                    "".into(),
                    vec![file("top.txt", "top.txt"), dir("sub", "sub")],
                ),
                ("sub".into(), vec![file("nested.txt", "sub/nested.txt")]),
            ]),
            files: std::collections::BTreeMap::from([
                ("top.txt".into(), b"a\nhit\nctx\nhit\nz\n".to_vec()),
                ("sub/nested.txt".into(), b"hit\n".to_vec()),
            ]),
        };
        let mut query = options("hit");
        query.depth = 1;
        query.before = 1;
        query.after = 1;
        let page = grep(&fs, query, &|| false).unwrap();
        assert_eq!(
            page.content,
            "top.txt-1-a\ntop.txt:2:hit\ntop.txt-3-ctx\ntop.txt:4:hit\ntop.txt-5-z\n"
        );
        assert!(!page.content.contains("nested"));
    }
    #[test]
    fn grep_page_clips_unselected_matching_context_and_advances_by_primary() {
        let fs = FakeFs {
            dirs: std::collections::BTreeMap::from([("".into(), vec![file("f", "f")])]),
            files: std::collections::BTreeMap::from([("f".into(), b"hit\nhit\ntail\n".to_vec())]),
        };
        let mut query = options("hit");
        query.before = 1;
        query.after = 1;
        query.offset = 1;
        query.limit = 1;
        let page = grep(&fs, query, &|| false).unwrap();
        assert_eq!(page.content, "f:2:hit\nf-3-tail\n");
        assert!(!page.truncated);
    }
    #[test]
    fn grep_limit_and_output_units_never_return_nonadvancing_page() {
        let fs = FakeFs {
            dirs: std::collections::BTreeMap::from([("".into(), vec![file("f", "f")])]),
            files: std::collections::BTreeMap::from([(
                "f".into(),
                format!("{}hit\n", "x".repeat(600)).into_bytes(),
            )]),
        };
        let mut query = options("hit");
        query.limits.output_bytes = 256;
        assert!(matches!(
            grep(&fs, query, &|| false),
            Err(GrepError::ResultLimit(_))
        ));
    }

    #[test]
    fn escape_only_ambiguous_path_labels() {
        assert_eq!(content_path_label("src/file.rs"), "src/file.rs");
        assert_eq!(content_path_label("src/a:b.rs"), r#""src/a:b.rs""#);
    }
}
