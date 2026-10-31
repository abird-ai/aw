//! Transport-independent search work/reply accounting.
//! Filesystem walking and regex engines remain host adapters until both
//! repositories agree on a single capability-rooted file reader.

use std::time::{Duration, Instant};

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
pub fn compact_incomplete(causes: Incomplete) -> Option<String> {
    let mut labels = Vec::with_capacity(3);
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
    if labels.is_empty() {
        None
    } else {
        Some(format!("… incomplete:{}\n", labels.join(",")))
    }
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
    #[test]
    fn escape_only_ambiguous_path_labels() {
        assert_eq!(content_path_label("src/file.rs"), "src/file.rs");
        assert_eq!(content_path_label("src/a:b.rs"), r#""src/a:b.rs""#);
    }
}
