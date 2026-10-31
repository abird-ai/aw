//! Exact-path lifecycle planning. Host backends enforce descriptor and
//! platform mutation authority; no glob, shell, OS path or tool grant here.
use std::collections::BTreeSet;
pub const MAX_LIFECYCLE_ROOTS: usize = 64;
pub const MAX_LIFECYCLE_ENTRIES: usize = 4096;
pub const MAX_LIFECYCLE_DEPTH: usize = 128;
pub const MAX_LIFECYCLE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleError {
    InvalidPath,
    OverlappingPaths,
    DestinationInsideSource,
    TooManyRoots,
    TooManyEntries,
    TooDeep,
    TooManyBytes,
}
/// Use one canonical exact relative-path grammar across copy/move/delete.
pub use super::is_reserved_name;
pub fn exact_path(path: &str) -> Result<(), LifecycleError> {
    if path.split('/').any(is_reserved_name) {
        return Err(LifecycleError::InvalidPath);
    }
    if path.is_empty()
        || path.len() > 4096
        || path.starts_with('/')
        || path.contains(['\\', '\0'])
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(LifecycleError::InvalidPath);
    }
    Ok(())
}
pub fn ancestor(ancestor: &str, descendant: &str) -> bool {
    descendant
        .strip_prefix(ancestor)
        .is_some_and(|suffix| suffix.starts_with('/'))
}
pub fn pair(source: &str, destination: &str, directory: bool) -> Result<(), LifecycleError> {
    exact_path(source)?;
    exact_path(destination)?;
    if source == destination {
        return Err(LifecycleError::OverlappingPaths);
    }
    if directory && ancestor(source, destination) {
        return Err(LifecycleError::DestinationInsideSource);
    }
    Ok(())
}
pub fn removals<'a>(
    paths: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<&'a str>, LifecycleError> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for p in paths {
        exact_path(p)?;
        if result.len() >= MAX_LIFECYCLE_ROOTS {
            return Err(LifecycleError::TooManyRoots);
        }
        if !seen.insert(p) {
            return Err(LifecycleError::OverlappingPaths);
        }
        result.push(p);
    }
    if result.is_empty() {
        return Err(LifecycleError::InvalidPath);
    }
    for (idx, path) in result.iter().enumerate() {
        if result
            .iter()
            .enumerate()
            .any(|(i, other)| i != idx && (ancestor(other, path) || ancestor(path, other)))
        {
            return Err(LifecycleError::OverlappingPaths);
        }
    }
    Ok(result)
}
#[derive(Clone, Copy, Debug, Default)]
pub struct WorkBudget {
    entries: usize,
    bytes: u64,
}
impl WorkBudget {
    pub fn visit(&mut self, depth: usize) -> Result<(), LifecycleError> {
        if depth > MAX_LIFECYCLE_DEPTH {
            return Err(LifecycleError::TooDeep);
        }
        self.entries = self
            .entries
            .checked_add(1)
            .ok_or(LifecycleError::TooManyEntries)?;
        if self.entries > MAX_LIFECYCLE_ENTRIES {
            return Err(LifecycleError::TooManyEntries);
        }
        Ok(())
    }
    pub fn copied(&mut self, bytes: u64) -> Result<(), LifecycleError> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or(LifecycleError::TooManyBytes)?;
        if self.bytes > MAX_LIFECYCLE_BYTES {
            return Err(LifecycleError::TooManyBytes);
        }
        Ok(())
    }
    pub fn entries(self) -> usize {
        self.entries
    }
    pub fn bytes(self) -> u64 {
        self.bytes
    }
}
/// Exact bounded byte copy with a private caller-provided destination.
/// This owns the 64 KiB streaming/cancellation invariant for both OS backends.
#[derive(Debug)]
pub enum CopyStreamError {
    Io(std::io::Error),
    Cancelled,
    Changed,
    TooManyBytes,
}
impl From<std::io::Error> for CopyStreamError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
pub fn copy_stream(
    from: &mut impl std::io::Read,
    to: &mut impl std::io::Write,
    expected: u64,
    cancel: &dyn Fn() -> bool,
) -> Result<u64, CopyStreamError> {
    if expected > MAX_LIFECYCLE_BYTES {
        return Err(CopyStreamError::TooManyBytes);
    }
    let mut buf = [0u8; 64 * 1024];
    let mut written = 0u64;
    loop {
        if cancel() {
            return Err(CopyStreamError::Cancelled);
        }
        let count = match from.read(&mut buf) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        if count == 0 {
            break;
        }
        written = written
            .checked_add(count as u64)
            .ok_or(CopyStreamError::TooManyBytes)?;
        if written > expected || written > MAX_LIFECYCLE_BYTES {
            return Err(CopyStreamError::Changed);
        }
        to.write_all(&buf[..count])?;
    }
    if written != expected {
        return Err(CopyStreamError::Changed);
    }
    Ok(written)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle_paths_never_expand_or_infer_destinations() {
        assert!(pair("src/*.rs", "dest.txt", false).is_ok());
        assert_eq!(
            pair("src", "src/out", true),
            Err(LifecycleError::DestinationInsideSource)
        );
        assert_eq!(pair("a", "a", true), Err(LifecycleError::OverlappingPaths));
        assert_eq!(
            removals(["a", "a/b"]),
            Err(LifecycleError::OverlappingPaths)
        );
        assert_eq!(removals(["../a"]), Err(LifecycleError::InvalidPath));
        assert_eq!(removals(["a", "b"]).unwrap(), ["a", "b"]);
    }
    #[test]
    fn shared_copy_stops_on_short_growing_or_cancelled_sources() {
        let mut source = &b"correct"[..];
        let mut output = Vec::new();
        assert_eq!(
            copy_stream(&mut source, &mut output, 7, &|| false).unwrap(),
            7
        );
        assert_eq!(output, b"correct");
        let mut short = &b"short"[..];
        assert!(matches!(
            copy_stream(&mut short, &mut Vec::new(), 10, &|| false),
            Err(CopyStreamError::Changed)
        ));
        let mut growing = &b"growing"[..];
        assert!(matches!(
            copy_stream(&mut growing, &mut Vec::new(), 2, &|| false),
            Err(CopyStreamError::Changed)
        ));
        let mut cancelled = &b"cancelled"[..];
        assert!(matches!(
            copy_stream(&mut cancelled, &mut Vec::new(), 9, &|| true),
            Err(CopyStreamError::Cancelled)
        ));
    }
    #[test]
    fn removal_order_and_status_are_stable_for_partial_commits() {
        assert_eq!(removals(["z.txt", "a.txt"]).unwrap(), ["z.txt", "a.txt"]);
        let partial = RemoveOutcome::Partial {
            ok: 1,
            total: 3,
            at: 2,
            reason: "cancelled",
        };
        assert!(partial.error());
        assert_eq!(
            partial.compact(),
            "mixed ok=1 fail=1 skip=1\n2 fail cancelled"
        );
        let cleanup = RemoveOutcome::CleanupPending {
            ok: 1,
            total: 1,
            at: 1,
        };
        assert!(!matches!(cleanup, RemoveOutcome::Complete(_)));
        assert_eq!(cleanup.compact(), "ok; cleanup pending");
        let mixed = RemoveOutcome::CleanupPending {
            ok: 2,
            total: 3,
            at: 2,
        };
        assert_eq!(
            mixed.compact(),
            "mixed ok=1 cleanup=1 skip=1\n2 cleanup pending"
        );
        assert!(mixed.error());
        assert!(
            !cleanup.error(),
            "published single-root remove is success with warning"
        );
    }
    #[test]
    fn recursive_work_is_independently_bounded() {
        let mut work = WorkBudget::default();
        for _ in 0..MAX_LIFECYCLE_ENTRIES {
            work.visit(1).unwrap();
        }
        assert_eq!(work.visit(1), Err(LifecycleError::TooManyEntries));
        assert_eq!(
            work.copied(MAX_LIFECYCLE_BYTES + 1),
            Err(LifecycleError::TooManyBytes)
        );
    }
}

/// Physical cleanup is deliberately separate from namespace publication.
/// A committed tombstone is never mistaken for a safe-to-repeat deletion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveOutcome {
    Complete(usize),
    Partial {
        ok: usize,
        total: usize,
        at: usize,
        reason: &'static str,
    },
    CleanupPending {
        ok: usize,
        total: usize,
        at: usize,
    },
}
impl RemoveOutcome {
    pub fn error(&self) -> bool {
        match self {
            Self::Complete(_) => false,
            Self::CleanupPending {
                ok: 1, total: 1, ..
            } => false,
            Self::Partial { .. } | Self::CleanupPending { .. } => true,
        }
    }
    pub fn compact(&self) -> String {
        match self {
            Self::Complete(1) => "ok".into(),
            Self::Complete(count) => format!("ok {count}"),
            Self::Partial {
                ok,
                total,
                at,
                reason,
            } => format!(
                "mixed ok={ok} fail=1 skip={}\n{at} fail {reason}",
                total - ok - 1
            ),
            Self::CleanupPending { ok, total, at } if *total == 1 && *ok == 1 => {
                "ok; cleanup pending".into()
            }
            // ok includes the published-but-not-yet-cleaned tombstone.
            // Do not double-count it as both a clean success and cleanup.
            Self::CleanupPending { ok, total, at } => format!(
                "mixed ok={} cleanup=1 skip={}\n{at} cleanup pending",
                ok.saturating_sub(1),
                total.saturating_sub(*ok)
            ),
        }
    }
}
