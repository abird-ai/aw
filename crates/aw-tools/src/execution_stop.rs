//! Host-neutral process stop evidence.
//!
//! These values retain only facts that affect shared process semantics:
//! why a process was meant to stop, when known, and what the platform proved.
//! Authority, caller provenance, cleanup paths and logging remain host context.
//! Physical process exit remains separate in `ProcessExit`.

/// Semantic reason the runtime intended a process to stop.
///
/// Every variant is a valid terminal attribution when paired with
/// `StopEvidence::Stopped`. Housekeeping has no semantic reason and is
/// represented by `StopRecord::reason == None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum StopReason {
    Timeout,
    Cancelled,
    RuntimeError,
}

/// What the platform stop operation actually proved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum StopEvidence {
    /// The platform proved the stop operation reached a live containment target.
    Stopped,
    /// Positive knowledge that no live containment target remains.
    NoTarget,
    /// The result cannot be established; the target may still be alive.
    Unknown,
}

/// One factual process-tree stop record.
///
/// `reason == None` means housekeeping rather than a semantic terminal cause.
/// How the attempt happened (explicit cleanup, Drop fallback, authority
/// withdrawal, parent cancellation, etc.) belongs in higher-layer context.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StopRecord {
    pub reason: Option<StopReason>,
    pub evidence: StopEvidence,
}

/// Host-neutral execution evidence, separate from Job wake events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExecutionLifecycleEvent {
    ProcessStop { record: StopRecord },
}

/// Optional observation hook for execution evidence.
///
/// Implementations may capture richer host/product context externally; the
/// shared event remains bounded and allocation-free.
pub trait ExecutionEventSink: Send + Sync {
    fn record(&self, event: ExecutionLifecycleEvent);
}

impl StopRecord {
    /// Record a stop attempt with a semantic reason.
    pub const fn new(reason: StopReason, evidence: StopEvidence) -> Self {
        Self {
            reason: Some(reason),
            evidence,
        }
    }

    pub const fn stopped(reason: StopReason) -> Self {
        Self::new(reason, StopEvidence::Stopped)
    }

    pub const fn no_target(reason: StopReason) -> Self {
        Self::new(reason, StopEvidence::NoTarget)
    }

    pub const fn unknown(reason: StopReason) -> Self {
        Self::new(reason, StopEvidence::Unknown)
    }

    /// Record housekeeping that has no semantic terminal reason.
    pub const fn housekeeping(evidence: StopEvidence) -> Self {
        Self {
            reason: None,
            evidence,
        }
    }

    /// Terminal attribution supported by this record.
    ///
    /// Every semantic reason is claimable; only `Stopped` evidence proves that
    /// this runtime actually stopped the live target.
    pub const fn terminal_reason(self) -> Option<StopReason> {
        match self.evidence {
            StopEvidence::Stopped => self.reason,
            StopEvidence::NoTarget | StopEvidence::Unknown => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REASONS: [StopReason; 3] = [
        StopReason::Timeout,
        StopReason::Cancelled,
        StopReason::RuntimeError,
    ];

    #[test]
    fn every_semantic_reason_is_terminal_when_stopped() {
        for reason in REASONS {
            assert_eq!(StopRecord::stopped(reason).terminal_reason(), Some(reason));
        }
    }

    #[test]
    fn no_target_and_unknown_never_claim_a_terminal_stop() {
        for reason in REASONS {
            assert_eq!(StopRecord::no_target(reason).terminal_reason(), None);
            assert_eq!(StopRecord::unknown(reason).terminal_reason(), None);
        }
    }

    #[test]
    fn housekeeping_never_claims_a_terminal_reason() {
        for evidence in [
            StopEvidence::Stopped,
            StopEvidence::NoTarget,
            StopEvidence::Unknown,
        ] {
            let record = StopRecord::housekeeping(evidence);
            assert_eq!(record.reason, None);
            assert_eq!(record.terminal_reason(), None);
        }
    }

    #[test]
    fn no_target_and_unknown_remain_distinct_evidence() {
        let reason = StopReason::RuntimeError;
        assert_eq!(
            StopRecord::no_target(reason).evidence,
            StopEvidence::NoTarget
        );
        assert_eq!(StopRecord::unknown(reason).evidence, StopEvidence::Unknown);
        assert_ne!(StopEvidence::NoTarget, StopEvidence::Unknown);
    }

    #[test]
    fn value_types_are_fixed_small_copy_evidence() {
        fn copy<T: Copy>(value: T) -> T {
            value
        }

        let reason = copy(StopReason::Cancelled);
        let evidence = copy(StopEvidence::Stopped);
        let record = copy(StopRecord::new(reason, evidence));
        assert_eq!(record.terminal_reason(), Some(reason));
        assert!(core::mem::size_of::<StopReason>() <= 1);
        assert!(core::mem::size_of::<StopEvidence>() <= 1);
        assert!(core::mem::size_of::<StopRecord>() <= 4);
    }
}
