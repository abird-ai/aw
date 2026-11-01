//! Transport-independent cumulative DL-150 progress. Pure snapshots;
//! the final result, not a progress notification, owns completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    Known(u64),
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidTotal,
    NotMonotonic,
    OverTotal,
    InvalidPhase,
}
#[derive(Debug, Clone)]
pub struct Progress {
    end: End,
    completed: u64,
}
impl Progress {
    pub fn new(end: End) -> Result<Self, Error> {
        if end == End::Known(0) {
            return Err(Error::InvalidTotal);
        }
        Ok(Self { end, completed: 0 })
    }
    /// The numeric unit stays fixed for the lifetime of this progress owner;
    /// status/phase may change while the counter stays unchanged.
    pub fn snapshot(&mut self, cumulative: u64, phase: &str) -> Result<String, Error> {
        if cumulative < self.completed {
            return Err(Error::NotMonotonic);
        }
        if let End::Known(total) = self.end {
            if cumulative > total {
                return Err(Error::OverTotal);
            }
        }
        if phase.len() > 64 || phase.chars().any(char::is_control) {
            return Err(Error::InvalidPhase);
        }
        self.completed = cumulative;
        let prefix = match self.end {
            End::Known(total) => format!("{cumulative}/{total}"),
            End::Unknown => cumulative.to_string(),
        };
        if phase.is_empty() {
            Ok(prefix)
        } else {
            Ok(format!("{prefix} {phase}"))
        }
    }
    pub fn phase_only(phase: &str) -> Result<String, Error> {
        if phase.is_empty() || phase.len() > 64 || phase.chars().any(char::is_control) {
            return Err(Error::InvalidPhase);
        }
        Ok(phase.into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cumulative_fixed_and_unknown_cannot_change_total_or_regress() {
        let mut fixed = Progress::new(End::Known(10)).unwrap();
        assert_eq!(fixed.snapshot(0, "scan").unwrap(), "0/10 scan");
        assert_eq!(fixed.snapshot(4, "commit").unwrap(), "4/10 commit");
        assert_eq!(fixed.snapshot(4, "cleanup").unwrap(), "4/10 cleanup");
        assert_eq!(fixed.snapshot(3, ""), Err(Error::NotMonotonic));
        assert_eq!(fixed.snapshot(11, ""), Err(Error::OverTotal));
        assert_eq!(fixed.snapshot(10, "").unwrap(), "10/10");
        let mut dynamic = Progress::new(End::Unknown).unwrap();
        assert_eq!(dynamic.snapshot(918, "scan").unwrap(), "918 scan");
        assert_eq!(Progress::phase_only("commit").unwrap(), "commit");
        assert_eq!(
            Progress::new(End::Known(0)).err(),
            Some(Error::InvalidTotal)
        );
    }
}
