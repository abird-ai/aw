//! AI-native filesystem contracts shared by AW and future dotlink adapters.
//! All selectors operate on verified, descriptor-rooted host metadata.
//! This crate never obtains an ambient filesystem path or model permission.
pub mod access;
#[cfg(feature = "read")]
pub mod backend;
#[cfg(feature = "batch")]
pub mod batch;
#[cfg(feature = "read")]
pub mod discovery;
#[cfg(feature = "write")]
pub mod exact_edit;
#[cfg(all(feature = "batch", feature = "durable"))]
pub mod job;
#[cfg(feature = "lifecycle")]
pub mod lifecycle;
#[cfg(feature = "rw")]
pub mod operations;
pub mod progress;
pub mod schema;
pub mod status;
#[cfg(feature = "read")]
pub mod walk;
pub mod write_hint;
use chrono::{DateTime, SecondsFormat, Utc};
use globset::{GlobBuilder, GlobMatcher};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::BinaryHeap, time::SystemTime};

/// Private mutation stages and tombstones are not model-selectable names,
/// but similar ordinary user names remain visible.
/// Deny model-visible selection of private in-flight stages/tombstones,
/// even when the caller bypasses discovery and supplies an exact path.
pub fn is_protected_path(path: &str) -> bool {
    path.split('/').any(is_reserved_name)
}
pub fn is_reserved_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".tmp") else {
        return false;
    };
    let Some(tail) = [".aw-stage-", ".aw-trash-", ".aw-backup-", ".dotlink-"]
        .iter()
        .find_map(|prefix| stem.strip_prefix(prefix))
    else {
        return false;
    };
    let Some((pid, nonce)) = tail.split_once('-') else {
        return false;
    };
    !pid.is_empty()
        && pid.bytes().all(|b| b.is_ascii_digit())
        && nonce.len() == 16
        && nonce.bytes().all(|b| b.is_ascii_hexdigit())
}
pub const MAX_WALK_VISITS: usize = 100_000;
pub const MAX_WALK_DEPTH: usize = 128;
pub const MAX_DISCOVERY_LIMIT: usize = 4096;
pub const MAX_RENDER_BYTES: usize = 96 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Any,
    File,
    Dir,
    Symlink,
    Other,
}
impl Kind {
    pub fn accepts(self, actual: Self) -> bool {
        self == Self::Any || self == actual
    }
    pub fn marker(self) -> char {
        match self {
            Self::File => 'f',
            Self::Dir => 'd',
            Self::Symlink => 'l',
            _ => 'o',
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    Path,
    Size,
    Mtime,
    Ctime,
    Btime,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetaKey {
    Size,
    Mtime,
    Ctime,
    Btime,
    Mode,
}
#[derive(Clone, Debug, Default)]
pub struct EntryMeta {
    pub kind: Kind,
    pub size: Option<u64>,
    pub mtime: Option<SystemTime>,
    pub ctime: Option<SystemTime>,
    pub btime: Option<SystemTime>,
    pub mode: Option<u32>,
}
#[derive(Clone, Debug)]
pub struct Entry {
    /// Fully qualified root-relative model path, already verified by host.
    pub path: String,
    pub name: String,
    pub meta: EntryMeta,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Progress {
    pub directory_budget: bool,
    pub traversal_error: bool,
    pub ignore_rules: bool,
    pub unreadable_entries: bool,
    pub path_encoding: bool,
}
impl Progress {
    pub fn incomplete(self) -> bool {
        self.directory_budget
            || self.traversal_error
            || self.ignore_rules
            || self.unreadable_entries
            || self.path_encoding
    }
    pub fn reasons(self) -> Option<String> {
        let mut causes = Vec::with_capacity(3);
        if self.directory_budget {
            causes.push("budget");
        }
        if self.traversal_error || self.unreadable_entries {
            causes.push("walk");
        }
        if self.ignore_rules {
            causes.push("gitignore");
        }
        if self.path_encoding {
            causes.push("path_encoding");
        }
        (!causes.is_empty()).then(|| causes.join(","))
    }
}
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeRange {
    pub after: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
}
impl TimeRange {
    pub fn matches(self, value: Option<SystemTime>) -> bool {
        let Some(value) = value else {
            return false;
        };
        let value = DateTime::<Utc>::from(value);
        self.after.is_none_or(|min| value > min) && self.before.is_none_or(|max| value < max)
    }
    pub fn valid(self) -> bool {
        self.after.zip(self.before).is_none_or(|(a, b)| a < b)
    }
}
#[derive(Clone, Debug)]
pub struct Filter {
    glob: Option<GlobMatcher>,
    glob_has_slash: bool,
    regex: Option<Regex>,
    pub kind: Kind,
    pub mtime: Option<TimeRange>,
    pub ctime: Option<TimeRange>,
    pub btime: Option<TimeRange>,
}
impl Filter {
    pub fn compile(
        glob: Option<&str>,
        regex: Option<&str>,
        kind: Kind,
        mtime: Option<TimeRange>,
        ctime: Option<TimeRange>,
        btime: Option<TimeRange>,
    ) -> Result<Self, &'static str> {
        let cap = 1024;
        for term in [glob, regex].into_iter().flatten() {
            if term.len() > cap || term.is_empty() || term.chars().any(char::is_control) {
                return Err("path filter must be 1..1024 printable bytes");
            }
        }
        if [mtime, ctime, btime]
            .into_iter()
            .flatten()
            .any(|r| !r.valid())
        {
            return Err("time range after must precede before");
        }
        let glob_has_slash = glob.is_some_and(|pattern| pattern.contains('/'));
        let glob = glob
            .map(|pattern| {
                GlobBuilder::new(pattern)
                    .literal_separator(true)
                    .build()
                    .map(|glob| glob.compile_matcher())
                    .map_err(|_| "invalid glob filter")
            })
            .transpose()?;
        let regex = regex
            .map(|expression| {
                RegexBuilder::new(expression)
                    .size_limit(1 << 20)
                    .dfa_size_limit(1 << 20)
                    .build()
                    .map_err(|_| "invalid pathname regex")
            })
            .transpose()?;
        Ok(Self {
            glob,
            glob_has_slash,
            regex,
            kind,
            mtime,
            ctime,
            btime,
        })
    }
    pub fn matches(&self, entry: &Entry, relative: &str) -> bool {
        if !self.kind.accepts(entry.meta.kind) {
            return false;
        }
        if self.glob.as_ref().is_some_and(|g| {
            // Slash-containing patterns match paths; simple globs match
            // basenames at any depth, without shell expansion.
            !g.is_match(if self.glob_has_slash {
                relative
            } else {
                &entry.name
            })
        }) {
            return false;
        }
        if self.regex.as_ref().is_some_and(|r| !r.is_match(relative)) {
            return false;
        }
        for (range, value) in [
            (self.mtime, entry.meta.mtime),
            (self.ctime, entry.meta.ctime),
            (self.btime, entry.meta.btime),
        ] {
            if range.is_some_and(|range| !range.matches(value)) {
                return false;
            }
        }
        true
    }
}
/// Global bounded selection. Never choose an arbitrary traversal-order page.
struct Ranked {
    entry: Entry,
    sort: Sort,
}
impl PartialEq for Ranked {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Ranked {}
impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Ranked {
    fn cmp(&self, other: &Self) -> Ordering {
        let a = &self.entry;
        let b = &other.entry;
        match self.sort {
            Sort::Path => a.path.cmp(&b.path),
            Sort::Size => size_order(a.meta.size, b.meta.size).then_with(|| a.path.cmp(&b.path)),
            Sort::Mtime => time_order(a.meta.mtime, b.meta.mtime).then_with(|| a.path.cmp(&b.path)),
            Sort::Ctime => time_order(a.meta.ctime, b.meta.ctime).then_with(|| a.path.cmp(&b.path)),
            Sort::Btime => time_order(a.meta.btime, b.meta.btime).then_with(|| a.path.cmp(&b.path)),
        }
    }
}
fn size_order(a: Option<u64>, b: Option<u64>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => b.cmp(&a),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}
fn time_order(a: Option<SystemTime>, b: Option<SystemTime>) -> Ordering {
    // Newest first; None sorts last.
    match (a, b) {
        (Some(a), Some(b)) => b.cmp(&a),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}
pub struct Selection {
    heap: BinaryHeap<Ranked>,
    sort: Sort,
    offset: usize,
    limit: usize,
    pub more: bool,
}
impl Selection {
    pub fn new(sort: Sort, offset: usize, limit: usize) -> Result<Self, &'static str> {
        if limit == 0
            || limit > MAX_DISCOVERY_LIMIT
            || offset.checked_add(limit).is_none()
            || offset + limit > 16_384
        {
            return Err("discovery offset/limit exceeds bounded work");
        }
        Ok(Self {
            heap: BinaryHeap::new(),
            sort,
            offset,
            limit,
            more: false,
        })
    }
    pub fn add(&mut self, entry: Entry) {
        let ranked = Ranked {
            entry,
            sort: self.sort,
        };
        let cap = self.offset + self.limit;
        if self.heap.len() < cap {
            self.heap.push(ranked);
        } else {
            self.more = true;
            if self.heap.peek().is_some_and(|last| ranked < *last) {
                self.heap.pop();
                self.heap.push(ranked);
            }
        }
    }
    pub fn finish(self) -> (Vec<Entry>, bool) {
        let mut all = self.heap.into_sorted_vec();
        let more = self.more || all.len() > self.offset + self.limit;
        let start = self.offset.min(all.len());
        let take = (all.len() - start).min(self.limit);
        let result = all.drain(start..start + take).map(|v| v.entry).collect();
        (result, more)
    }
}
pub fn safe_label(path: &str) -> String {
    if path.chars().any(char::is_control) || path.contains([':', '\\', '"']) {
        serde_json::to_string(path).unwrap_or_else(|_| "\"<invalid-path>\"".into())
    } else {
        path.into()
    }
}
/// Exact POSIX seconds/nanoseconds, including dates before 1970.
pub fn system_time_from_unix(secs: i64, nanos: u32) -> Option<SystemTime> {
    if nanos >= 1_000_000_000 {
        return None;
    }
    let abs = secs.unsigned_abs();
    if secs >= 0 {
        std::time::UNIX_EPOCH.checked_add(std::time::Duration::new(abs, nanos))
    } else {
        let delta = if nanos == 0 {
            std::time::Duration::from_secs(abs)
        } else {
            std::time::Duration::new(abs - 1, 1_000_000_000 - nanos)
        };
        std::time::UNIX_EPOCH.checked_sub(delta)
    }
}
pub fn format_time(value: Option<SystemTime>) -> String {
    value
        .map(|v| DateTime::<Utc>::from(v).to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_else(|| "-".into())
}
pub fn render_line(entry: &Entry, display: &str, meta: &[MetaKey]) -> String {
    let mut line = String::new();
    line.push(entry.meta.kind.marker());
    for key in meta {
        line.push_str("  ");
        let rendered = match key {
            MetaKey::Size => entry
                .meta
                .size
                .map(|n| n.to_string())
                .unwrap_or_else(|| "-".into()),
            MetaKey::Mtime => format_time(entry.meta.mtime),
            MetaKey::Ctime => format_time(entry.meta.ctime),
            MetaKey::Btime => format_time(entry.meta.btime),
            MetaKey::Mode => entry
                .meta
                .mode
                .map(|m| format!("{m:04o}"))
                .unwrap_or_else(|| "-".into()),
        };
        line.push_str(&rendered);
    }
    line.push_str("  ");
    line.push_str(&safe_label(display));
    line.push('\n');
    line
}
pub fn render(
    entries: &[Entry],
    display_path: impl Fn(&Entry) -> &str,
    meta: &[MetaKey],
    offset: usize,
    more: bool,
    progress: Progress,
    max_bytes: usize,
) -> (String, bool) {
    let mut result = String::new();
    let ceiling = max_bytes.min(MAX_RENDER_BYTES);
    let mut emitted = 0usize;
    let reserve = 128usize.min(ceiling);
    for entry in entries {
        let line = render_line(entry, display_path(entry), meta);
        if result
            .len()
            .saturating_add(line.len())
            .saturating_add(reserve)
            > ceiling
        {
            break;
        }
        result.push_str(&line);
        emitted += 1;
    }
    let paged = more || emitted < entries.len();
    let next = paged.then(|| offset + emitted);
    let reasons = progress.reasons();
    if let Some(status) = status::query(next, reasons.as_deref()) {
        if result.len() + status.len() <= ceiling {
            result.push_str(&status);
        }
    }
    (result, paged || progress.incomplete())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn entry(path: &str, size: u64) -> Entry {
        Entry {
            path: path.into(),
            name: path.rsplit('/').next().unwrap().into(),
            meta: EntryMeta {
                kind: Kind::File,
                size: Some(size),
                ..Default::default()
            },
        }
    }
    #[test]
    fn globally_sorted_page_after_traversal_order() {
        let mut selection = Selection::new(Sort::Size, 1, 2).unwrap();
        for (path, size) in [("z", 2), ("a", 4), ("b", 1), ("c", 7), ("d", 9)] {
            selection.add(entry(path, size));
        }
        let (entries, more) = selection.finish();
        assert!(more);
        assert_eq!(
            entries.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            ["c", "a"]
        );
    }
    #[test]
    fn compact_labels_and_ordered_metadata() {
        let e = entry("hello\nevil", 19);
        assert_eq!(
            render_line(&e, &e.path, &[MetaKey::Size]),
            "f  19  \"hello\\nevil\"\n"
        );
        let good = entry("src/main.rs", 10);
        assert_eq!(render_line(&good, &good.path, &[]), "f  src/main.rs\n");
    }
    #[test]
    fn missing_directory_and_other_sizes_are_unavailable_not_zero() {
        let mut directory = entry("d", 0);
        directory.meta.kind = Kind::Dir;
        directory.meta.size = None;
        assert_eq!(render_line(&directory, "d", &[MetaKey::Size]), "d  -  d\n");
        let mut special = entry("fifo", 0);
        special.meta.kind = Kind::Other;
        special.meta.size = None;
        assert_eq!(
            render_line(&special, "fifo", &[MetaKey::Size]),
            "o  -  fifo\n"
        );
        let mut sort = Selection::new(Sort::Size, 0, 3).unwrap();
        sort.add(directory);
        sort.add(entry("file", 0));
        sort.add(special);
        assert_eq!(
            sort.finish()
                .0
                .iter()
                .map(|entry| entry.path.as_str())
                .collect::<Vec<_>>(),
            ["file", "d", "fifo"]
        );
    }
    #[test]
    fn time_ranges_have_exclusive_exact_rfc3339_boundaries() {
        use chrono::DateTime;
        let at = DateTime::parse_from_rfc3339("2026-10-31T06:00:00.123456789Z")
            .unwrap()
            .to_utc();
        let time = SystemTime::from(at);
        let exact = TimeRange {
            after: Some(at),
            before: None,
        };
        assert!(!exact.matches(Some(time)));
        let before = TimeRange {
            after: None,
            before: Some(at),
        };
        assert!(!before.matches(Some(time)));
        assert!(!TimeRange {
            after: Some(at),
            before: Some(at)
        }
        .valid());
        let below = TimeRange {
            after: Some(at - chrono::Duration::nanoseconds(1)),
            before: Some(at + chrono::Duration::nanoseconds(1)),
        };
        assert!(below.valid());
        assert!(below.matches(Some(time)));
        assert!(!below.matches(None));
    }
    #[test]
    fn timestamp_sort_unavailable_last_and_paths_stable() {
        let before = system_time_from_unix(-1, 500_000_000).unwrap();
        let after = system_time_from_unix(1_750_000_000, 0).unwrap();
        assert!(before < std::time::UNIX_EPOCH);
        assert!(after > std::time::UNIX_EPOCH);
        let mut selected = Selection::new(Sort::Mtime, 0, 5).unwrap();
        let mut old = entry("old", 1);
        old.meta.mtime = Some(before);
        let mut fresh = entry("new", 1);
        fresh.meta.mtime = Some(after);
        selected.add(old);
        selected.add(entry("unknown", 1));
        selected.add(fresh);
        let (ordered, more) = selected.finish();
        assert!(!more);
        assert_eq!(
            ordered.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            ["new", "old", "unknown"]
        );
        assert_eq!(system_time_from_unix(0, 1_000_000_000), None);
    }
    #[test]
    fn glob_regex_and_time_filters_intersect() {
        let filter = Filter::compile(
            Some("src/**/*.rs"),
            Some("src/.+\\.rs$"),
            Kind::File,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(filter.matches(&entry("src/nested/lib.rs", 1), "src/nested/lib.rs"));
        assert!(!filter.matches(&entry("src/nested/lib.md", 1), "src/nested/lib.md"));
        assert!(!filter.matches(&entry("other/lib.rs", 1), "other/lib.rs"));
        assert!(Filter::compile(
            None,
            None,
            Kind::Any,
            None,
            Some(TimeRange {
                after: Some(
                    DateTime::parse_from_rfc3339("2026-12-31T23:59:59Z")
                        .unwrap()
                        .to_utc()
                ),
                before: Some(
                    DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                        .unwrap()
                        .to_utc()
                )
            }),
            None
        )
        .is_err());
    }
    #[test]
    fn glob_path_separator_is_decided_from_original_pattern() {
        let nested = Entry {
            path: "src/deep/code.rs".into(),
            name: "code.rs".into(),
            meta: EntryMeta {
                kind: Kind::File,
                ..Default::default()
            },
        };
        let basename = Filter::compile(Some("*.rs"), None, Kind::File, None, None, None).unwrap();
        assert!(
            basename.matches(&nested, "deep/code.rs"),
            "basename glob applies at every depth"
        );
        let by_path =
            Filter::compile(Some("deep/*.rs"), None, Kind::File, None, None, None).unwrap();
        assert!(by_path.matches(&nested, "deep/code.rs"));
        assert!(!by_path.matches(&nested, "code.rs"));
    }
    #[test]
    fn timestamp_filter_requires_actual_metadata() {
        let range = TimeRange {
            after: Some(
                DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
                    .unwrap()
                    .to_utc(),
            ),
            before: None,
        };
        let filter = Filter::compile(None, None, Kind::Any, Some(range), None, None).unwrap();
        assert!(!filter.matches(&entry("foo", 1), "foo"));
    }
}
