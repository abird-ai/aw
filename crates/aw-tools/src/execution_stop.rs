//! Host-neutral process stop evidence.
//!
//! These values describe why a runtime attempted to stop a process and what
//! evidence that attempt produced. They carry no authority, clock, logging or
//! OS handles. Physical process exit remains separate in `ProcessExit`.

/// Who originated a cooperative/forced cancellation request.
///
/// Keep this factual and portable. Product-specific spellings such as
/// dotlink's `shutdown` are projections of these values, not shared semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CancellationOrigin {
    /// Explicit control request from the current caller/owner.
    Caller,
    /// A parent execution cancelled a child/subagent execution.
    Parent,
    /// Runtime shutdown, restart, owner teardown or replacement.
    Runtime,
    /// Source/grant/policy authority was revoked or replaced.
    Authority,
}

/// Why the runtime can claim it stopped the waited command.
///
/// This is terminal attribution, not Job lifecycle and not physical exit shape.
/// It exists only when a claimable stop attempt was actually delivered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProcessStopCause {
    Timeout,
    Cancelled(CancellationOrigin),
    Failed,
}

/// Why the runtime attempted to signal/terminate a process tree.
///
/// `Cleanup` and `Dropped` are forensic attempt reasons only. They never justify
/// a terminal `ProcessStopCause` by themselves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StopAttemptReason {
    Timeout,
    Cancelled(CancellationOrigin),
    Failed,
    Cleanup,
    Dropped,
}

impl StopAttemptReason {
    /// Terminal claim this reason could support if termination was delivered.
    pub const fn terminal_cause(self) -> Option<ProcessStopCause> {
        match self {
            Self::Timeout => Some(ProcessStopCause::Timeout),
            Self::Cancelled(origin) => Some(ProcessStopCause::Cancelled(origin)),
            Self::Failed => Some(ProcessStopCause::Failed),
            Self::Cleanup | Self::Dropped => None,
        }
    }
}

/// What the platform termination operation actually proved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum StopOutcome {
    /// The platform proved the termination operation was delivered to a live
    /// containment object/target.
    Delivered,
    /// The target/containment object was already absent. Desired state, but not
    /// evidence that this attempt stopped the waited command.
    AlreadyGone,
    /// Delivery could not be proved; the target may still be running.
    Failed,
}

/// One factual process-tree stop attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StopAttempt {
    pub reason: StopAttemptReason,
    pub outcome: StopOutcome,
}

impl StopAttempt {
    pub const fn new(reason: StopAttemptReason, outcome: StopOutcome) -> Self {
        Self { reason, outcome }
    }

    /// Convert platform evidence into a terminal attribution.
    ///
    /// Only a delivered, claimable reason may become a `ProcessStopCause`.
    /// Cleanup/Drop attempts and gone/failed delivery remain forensic evidence
    /// only.
    pub const fn terminal_cause(self) -> Option<ProcessStopCause> {
        match self.outcome {
            StopOutcome::Delivered => self.reason.terminal_cause(),
            StopOutcome::AlreadyGone | StopOutcome::Failed => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cancellation_origin_survives_a_delivered_claim() {
        for origin in [
            CancellationOrigin::Caller,
            CancellationOrigin::Parent,
            CancellationOrigin::Runtime,
            CancellationOrigin::Authority,
        ] {
            assert_eq!(
                StopAttempt::new(StopAttemptReason::Cancelled(origin), StopOutcome::Delivered,)
                    .terminal_cause(),
                Some(ProcessStopCause::Cancelled(origin))
            );
        }
    }

    #[test]
    fn only_delivered_claimable_attempts_become_terminal_causes() {
        for outcome in [StopOutcome::AlreadyGone, StopOutcome::Failed] {
            for reason in [
                StopAttemptReason::Timeout,
                StopAttemptReason::Cancelled(CancellationOrigin::Caller),
                StopAttemptReason::Cancelled(CancellationOrigin::Parent),
                StopAttemptReason::Cancelled(CancellationOrigin::Runtime),
                StopAttemptReason::Cancelled(CancellationOrigin::Authority),
                StopAttemptReason::Failed,
                StopAttemptReason::Cleanup,
                StopAttemptReason::Dropped,
            ] {
                assert_eq!(
                    StopAttempt::new(reason, outcome).terminal_cause(),
                    None,
                    "{reason:?} with {outcome:?} must not claim a terminal stop"
                );
            }
        }

        assert_eq!(
            StopAttempt::new(StopAttemptReason::Timeout, StopOutcome::Delivered).terminal_cause(),
            Some(ProcessStopCause::Timeout)
        );
        assert_eq!(
            StopAttempt::new(StopAttemptReason::Failed, StopOutcome::Delivered).terminal_cause(),
            Some(ProcessStopCause::Failed)
        );
    }

    #[test]
    fn cleanup_and_drop_are_forensic_even_when_delivery_succeeds() {
        for reason in [StopAttemptReason::Cleanup, StopAttemptReason::Dropped] {
            let attempt = StopAttempt::new(reason, StopOutcome::Delivered);
            assert_eq!(attempt.terminal_cause(), None);
        }
    }

    #[test]
    fn value_types_are_fixed_small_copy_evidence() {
        fn copy<T: Copy>(value: T) -> T {
            value
        }

        let origin = copy(CancellationOrigin::Authority);
        let cause = copy(ProcessStopCause::Cancelled(origin));
        let attempt = copy(StopAttempt::new(
            StopAttemptReason::Cancelled(origin),
            StopOutcome::Delivered,
        ));
        assert_eq!(attempt.terminal_cause(), Some(cause));
        assert!(std::mem::size_of::<CancellationOrigin>() <= 1);
        assert!(std::mem::size_of::<StopOutcome>() <= 1);
        assert!(std::mem::size_of::<StopAttempt>() <= 4);
    }
}
