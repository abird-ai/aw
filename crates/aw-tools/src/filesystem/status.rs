//! Canonical DL-150 outcome grammar. Protocol success/error flag authoritative.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Primary {
    Ok,
    Unchanged,
    Fail,
    Mixed,
    Unknown,
    Skip,
}
impl Primary {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Unchanged => "unchanged",
            Self::Fail => "fail",
            Self::Mixed => "mixed",
            Self::Unknown => "unknown",
            Self::Skip => "skip",
        }
    }
    pub fn is_error(self) -> bool {
        matches!(self, Self::Fail | Self::Mixed | Self::Unknown)
    }
}
/// One classification for a Job/Graph or any multi-object operation.
///// Structural skips do not change material outcome family.
pub fn aggregate(components: impl IntoIterator<Item = Primary>) -> Primary {
    let mut saw_success = false;
    let mut saw_changed = false;
    let mut saw_failure = false;
    let mut saw_unknown = false;
    let mut saw_mixed = false;
    for status in components {
        match status {
            Primary::Ok => {
                saw_success = true;
                saw_changed = true;
            }
            Primary::Unchanged => saw_success = true,
            Primary::Fail => saw_failure = true,
            Primary::Unknown => saw_unknown = true,
            Primary::Mixed => saw_mixed = true,
            Primary::Skip => {}
        }
    }
    let types = u8::from(saw_success) + u8::from(saw_failure) + u8::from(saw_unknown);
    if saw_mixed || types > 1 {
        Primary::Mixed
    } else if saw_unknown {
        Primary::Unknown
    } else if saw_failure {
        Primary::Fail
    } else if saw_success {
        if saw_changed {
            Primary::Ok
        } else {
            Primary::Unchanged
        }
    } else {
        Primary::Skip
    }
}
// Paging and incomplete coverage are independent modifiers.
pub fn query(next_offset: Option<usize>, incomplete: Option<&str>) -> Option<String> {
    match (next_offset, incomplete) {
        (None, None) => None,
        (Some(next), None) => Some(format!("more offset={next}\n")),
        (None, Some(reasons)) => Some(format!("incomplete {reasons}\n")),
        (Some(next), Some(reasons)) => Some(format!("more offset={next} incomplete={reasons}\n")),
    }
}
pub fn read(next: Option<(usize, usize)>, io_fault: bool) -> Option<String> {
    match (next, io_fault) {
        (None, false) => None,
        (Some((line, byte)), false) => Some(format!("more line={line} byte={byte}\n")),
        (Some((line, byte)), true) => Some(format!(
            "mixed ok=prefix fail=io more line={line} byte={byte}\n"
        )),
        (None, true) => Some("mixed ok=prefix fail=io\n".into()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paging_and_coverage_are_independent_modifiers() {
        assert_eq!(
            query(Some(200), Some("walk,gitignore")).unwrap(),
            "more offset=200 incomplete=walk,gitignore\n"
        );
        assert_eq!(query(None, Some("budget")).unwrap(), "incomplete budget\n");
        assert_eq!(query(Some(2), None).unwrap(), "more offset=2\n");
        assert_eq!(query(None, None), None);
    }
    #[test]
    fn heterogeneous_outcomes_never_discard_unknown_components() {
        use Primary as P;
        assert_eq!(aggregate([P::Skip]), P::Skip);
        assert_eq!(aggregate([P::Unchanged, P::Skip]), P::Unchanged);
        assert_eq!(aggregate([P::Ok, P::Unchanged]), P::Ok);
        assert_eq!(aggregate([P::Fail, P::Fail]), P::Fail);
        assert_eq!(aggregate([P::Unknown, P::Unknown]), P::Unknown);
        assert_eq!(aggregate([P::Fail, P::Unknown]), P::Mixed);
        assert_eq!(aggregate([P::Unknown, P::Ok]), P::Mixed);
        assert_eq!(aggregate([P::Mixed, P::Skip]), P::Mixed);
        assert_eq!(aggregate([P::Skip, P::Fail]), P::Fail);
    }
    #[test]
    fn read_cursors_and_error_primary_are_stable() {
        assert_eq!(
            read(Some((42, 1048)), false).unwrap(),
            "more line=42 byte=1048\n"
        );
        assert_eq!(
            read(Some((81, 0)), true).unwrap(),
            "mixed ok=prefix fail=io more line=81 byte=0\n"
        );
        assert!(Primary::Mixed.is_error());
        assert!(!Primary::Ok.is_error());
    }
}
