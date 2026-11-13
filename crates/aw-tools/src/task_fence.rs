//! Durable task fencing, independent of journal storage and MCP transport.
//! The host supplies the authoritative actor/source/site/scope on *every*
//! operation. A client-provided ID alone is never a task capability.

use crate::{task::TaskDisposition, ToolEffect};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskIdentity {
    pub actor: String,
    pub source_kind: String,
    pub source_id: String,
    pub source_epoch: u64,
    pub execution_site: String,
    pub session: String,
    pub branch: String,
    pub turn_id: u64,
    pub call_id: String,
    pub effect: ToolEffect,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FenceError {
    InvalidIdentity,
    MissingJournalIntent,
    WrongSourceOrScope,
    Revoked,
    StaleGeneration,
    InvalidTransition,
    RecoveryEvidenceRequired,
    GenerationExhausted,
    JournalOutcomeUnknown,
    /// Host proved the write was rejected before any durable mutation.
    JournalRejectedNoEffect,
}
impl TaskIdentity {
    /// Stable, length-framed binding fingerprint for host-owned checkpoints.
    /// This prevents accidentally attaching a spool to another tool call,
    /// not malicious alteration of a host's journal.
    pub fn fingerprint(&self) -> [u8; 32] {
        use sha2::{Digest as _, Sha256};
        let mut h = Sha256::new();
        h.update(b"aw-tools-core-task-identity-v1");
        for part in [
            &self.actor,
            &self.source_kind,
            &self.source_id,
            &self.execution_site,
            &self.session,
            &self.branch,
            &self.call_id,
        ] {
            h.update((part.len() as u64).to_le_bytes());
            h.update(part.as_bytes());
        }
        h.update(self.source_epoch.to_le_bytes());
        h.update(self.turn_id.to_le_bytes());
        h.update([match self.effect {
            ToolEffect::Read => 0,
            ToolEffect::Write => 1,
            ToolEffect::Process => 2,
            ToolEffect::Control => 3,
            ToolEffect::Unknown => 4,
        }]);
        h.finalize().into()
    }
    pub fn validate(&self) -> Result<(), FenceError> {
        for field in [
            &self.actor,
            &self.source_kind,
            &self.source_id,
            &self.execution_site,
            &self.session,
            &self.branch,
            &self.call_id,
        ] {
            if field.is_empty() || field.len() > 256 || field.chars().any(char::is_control) {
                return Err(FenceError::InvalidIdentity);
            }
        }
        if self.turn_id == 0
            || self.source_epoch == 0
            || !matches!(self.source_kind.as_str(), "host" | "mcp" | "extension")
        {
            return Err(FenceError::InvalidIdentity);
        }
        Ok(())
    }
}
/// Proposed CAS transition, not evidence that a durable effect settled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskTransition {
    pub expected_generation: u64,
    pub next_generation: u64,
    pub next: TaskDisposition,
    pub journal_intent_sequence: u64,
    pub task_binding: [u8; 32],
}
#[derive(Debug, Clone)]
pub struct TaskFence {
    identity: TaskIdentity,
    generation: u64,
    state: TaskDisposition,
    journal_intent_sequence: u64,
    journal_unknown: bool,
}
impl TaskFence {
    /// Call only AFTER the exact immutable TurnJournal ToolStarted record is
    /// durably appended by the existing host owner. An unjournaled task
    /// cannot be represented as an executable fence.
    pub fn after_journal(identity: TaskIdentity, intent_sequence: u64) -> Result<Self, FenceError> {
        identity.validate()?;
        if intent_sequence == 0 {
            return Err(FenceError::MissingJournalIntent);
        }
        Ok(Self {
            identity,
            generation: 1,
            state: TaskDisposition::Queued,
            journal_intent_sequence: intent_sequence,
            journal_unknown: false,
        })
    }
    pub fn binding(&self) -> &TaskIdentity {
        &self.identity
    }
    pub fn state(&self) -> TaskDisposition {
        self.state
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn journal_intent_sequence(&self) -> u64 {
        self.journal_intent_sequence
    }
    /// Exact binding and current host liveness must be checked for get,
    /// page, acknowledgement, cancellation and every effect transition.
    pub fn verify(
        &self,
        now: &TaskIdentity,
        expected_generation: u64,
        source_live: bool,
    ) -> Result<(), FenceError> {
        if self.journal_unknown {
            return Err(FenceError::JournalOutcomeUnknown);
        }
        if &self.identity != now {
            return Err(FenceError::WrongSourceOrScope);
        }
        if !source_live {
            return Err(FenceError::Revoked);
        }
        if expected_generation != self.generation {
            return Err(FenceError::StaleGeneration);
        }
        Ok(())
    }
    /// A pure transition proposal: cannot change in-memory state. The host
    /// must persist this against its existing authoritative journal/CAS
    /// before any local generation is advanced.
    pub fn propose_transition(
        &self,
        now: &TaskIdentity,
        expected_generation: u64,
        source_live: bool,
        next: TaskDisposition,
        recovery_proven: bool,
    ) -> Result<TaskTransition, FenceError> {
        self.verify(now, expected_generation, source_live)?;
        if !self.state.can_transition(next) {
            return Err(FenceError::InvalidTransition);
        }
        if self.state == TaskDisposition::Uncertain
            && next.terminal()
            && next != self.state
            && !recovery_proven
        {
            return Err(FenceError::RecoveryEvidenceRequired);
        }
        let next_generation = if self.state == next {
            self.generation
        } else {
            self.generation
                .checked_add(1)
                .ok_or(FenceError::GenerationExhausted)?
        };
        Ok(TaskTransition {
            expected_generation: self.generation,
            next_generation,
            next,
            journal_intent_sequence: self.journal_intent_sequence,
            task_binding: self.identity.fingerprint(),
        })
    }

    /// Journal first, then change the local projection. The supplied closure
    /// is a host-owned durable CAS, never a model-selected "journal" callback.
    /// Only a PROVEN no-effect rejection preserves local usability;
    /// all other failures are unknown and require journal reconstruction.
    pub fn transition_journaled(
        &mut self,
        now: &TaskIdentity,
        expected_generation: u64,
        source_live: bool,
        next: TaskDisposition,
        recovery_proven: bool,
        persist: impl FnOnce(TaskTransition) -> Result<(), FenceError>,
    ) -> Result<u64, FenceError> {
        let proposal =
            self.propose_transition(now, expected_generation, source_live, next, recovery_proven)?;
        if self.state == next {
            return Ok(self.generation);
        }
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| persist(proposal))) {
            Ok(Ok(())) => {}
            Ok(Err(FenceError::JournalRejectedNoEffect)) => {
                return Err(FenceError::JournalRejectedNoEffect);
            }
            Ok(Err(_)) | Err(_) => {
                // Even an I/O error after a successful CAS could mean the
                // journal mutated. Treat all ambiguous failures as unknown.
                self.journal_unknown = true;
                return Err(FenceError::JournalOutcomeUnknown);
            }
        }
        // The caller holds the task owner's exclusive lock throughout the
        // durable transaction, so this is the unique local projection.
        self.state = proposal.next;
        self.generation = proposal.next_generation;
        Ok(self.generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn identity() -> TaskIdentity {
        TaskIdentity {
            actor: "owner".into(),
            source_kind: "host".into(),
            source_id: "native".into(),
            source_epoch: 12,
            execution_site: "local".into(),
            session: "session-1".into(),
            branch: "main".into(),
            turn_id: 7,
            call_id: "call-1".into(),
            effect: ToolEffect::Process,
        }
    }
    #[test]
    fn durable_tasks_must_be_exactly_scoped_and_journaled() {
        assert_eq!(
            TaskFence::after_journal(identity(), 0).unwrap_err(),
            FenceError::MissingJournalIntent
        );
        let mut malformed = identity();
        malformed.branch.clear();
        assert_eq!(
            TaskFence::after_journal(malformed, 1).unwrap_err(),
            FenceError::InvalidIdentity
        );
        let fence = TaskFence::after_journal(identity(), 5).unwrap();
        assert_eq!(fence.journal_intent_sequence(), 5);
        let mut wrong = identity();
        wrong.source_epoch += 1;
        assert_eq!(
            fence.verify(&wrong, 1, true),
            Err(FenceError::WrongSourceOrScope)
        );
        let mut wrong = identity();
        wrong.session = "different".into();
        assert_eq!(
            fence.verify(&wrong, 1, true),
            Err(FenceError::WrongSourceOrScope)
        );
        assert_eq!(
            fence.verify(&identity(), 1, false),
            Err(FenceError::Revoked)
        );
    }
    #[test]
    fn generation_and_terminal_recovery_are_fenced() {
        let mut task = TaskFence::after_journal(identity(), 99).unwrap();
        assert_eq!(
            task.transition_journaled(
                &identity(),
                1,
                true,
                TaskDisposition::Running,
                false,
                |_| Ok(())
            )
            .unwrap(),
            2
        );
        assert_eq!(
            task.transition_journaled(
                &identity(),
                1,
                true,
                TaskDisposition::Completed,
                false,
                |_| Ok(())
            ),
            Err(FenceError::StaleGeneration)
        );
        assert_eq!(
            task.transition_journaled(
                &identity(),
                2,
                true,
                TaskDisposition::Uncertain,
                false,
                |_| Ok(())
            )
            .unwrap(),
            3
        );
        assert_eq!(
            task.transition_journaled(
                &identity(),
                3,
                true,
                TaskDisposition::Completed,
                false,
                |_| Ok(())
            ),
            Err(FenceError::RecoveryEvidenceRequired)
        );
        assert_eq!(
            task.transition_journaled(
                &identity(),
                3,
                true,
                TaskDisposition::Completed,
                true,
                |_| Ok(())
            )
            .unwrap(),
            4
        );
        assert_eq!(
            task.transition_journaled(
                &identity(),
                4,
                true,
                TaskDisposition::Running,
                true,
                |_| Ok(())
            ),
            Err(FenceError::InvalidTransition)
        );
        assert_eq!(
            task.transition_journaled(
                &identity(),
                4,
                true,
                TaskDisposition::Completed,
                true,
                |_| Ok(())
            )
            .unwrap(),
            4
        );
    }
    #[test]
    fn panicking_or_uncertain_journal_fences_all_future_operations() {
        let mut fence = TaskFence::after_journal(identity(), 31).unwrap();
        let outcome = fence.transition_journaled(
            &identity(),
            1,
            true,
            TaskDisposition::Running,
            false,
            |_| panic!("host persistence may already have committed"),
        );
        assert_eq!(outcome, Err(FenceError::JournalOutcomeUnknown));
        assert_eq!(fence.generation(), 1);
        assert_eq!(
            fence.verify(&identity(), 1, true),
            Err(FenceError::JournalOutcomeUnknown)
        );
        assert_eq!(
            fence.transition_journaled(
                &identity(),
                1,
                true,
                TaskDisposition::Running,
                false,
                |_| Ok(())
            ),
            Err(FenceError::JournalOutcomeUnknown)
        );
        // Only a fresh host-owned projection from the original journal can
        // resume reads or transitions; this poisoned fence is never reused.
        let reconstructed = TaskFence::after_journal(identity(), 32).unwrap();
        assert!(reconstructed.verify(&identity(), 1, true).is_ok());
    }
    #[test]
    fn task_bindings_require_canonical_source_kind_and_nonzero_epoch() {
        let mut id = identity();
        id.source_epoch = 0;
        assert_eq!(id.validate(), Err(FenceError::InvalidIdentity));
        id = identity();
        id.source_kind = "arbitrary".into();
        assert_eq!(id.validate(), Err(FenceError::InvalidIdentity));
        for kind in ["host", "mcp", "extension"] {
            id.source_kind = kind.into();
            assert!(id.validate().is_ok());
        }
    }
    #[test]
    fn rejected_or_failed_journal_cannot_advance_local_generation() {
        let mut fence = TaskFence::after_journal(identity(), 27).unwrap();
        let request = fence
            .propose_transition(&identity(), 1, true, TaskDisposition::Running, false)
            .unwrap();
        assert_eq!(request.expected_generation, 1);
        assert_eq!(request.next_generation, 2);
        assert_eq!(request.journal_intent_sequence, 27);
        assert_eq!(request.task_binding, identity().fingerprint());
        let err = fence.transition_journaled(
            &identity(),
            1,
            true,
            TaskDisposition::Running,
            false,
            |_| Err(FenceError::JournalRejectedNoEffect),
        );
        assert_eq!(err, Err(FenceError::JournalRejectedNoEffect));
        assert_eq!(fence.generation(), 1);
        assert_eq!(fence.state(), TaskDisposition::Queued);
        let mut called = 0;
        assert_eq!(
            fence.transition_journaled(
                &identity(),
                1,
                true,
                TaskDisposition::Running,
                false,
                |proposal| {
                    assert_eq!(proposal.next_generation, 2);
                    called += 1;
                    Ok(())
                }
            ),
            Ok(2)
        );
        assert_eq!(called, 1);
        assert_eq!(
            fence.transition_journaled(
                &identity(),
                2,
                true,
                TaskDisposition::Running,
                false,
                |_| panic!("already durable; no rewrite")
            ),
            Ok(2)
        );
        assert_eq!(
            fence.transition_journaled(
                &identity(),
                1,
                true,
                TaskDisposition::Completed,
                false,
                |_| panic!("stale CAS must not persist")
            ),
            Err(FenceError::StaleGeneration)
        );
    }
    #[test]
    fn any_ambiguous_host_error_fences_the_task_until_journal_recovery() {
        let mut fence = TaskFence::after_journal(identity(), 23).unwrap();
        assert_eq!(
            fence.transition_journaled(
                &identity(),
                1,
                true,
                TaskDisposition::Running,
                false,
                |_| Err(FenceError::MissingJournalIntent)
            ),
            Err(FenceError::JournalOutcomeUnknown)
        );
        assert_eq!(fence.generation(), 1);
        assert_eq!(
            fence.verify(&identity(), 1, true),
            Err(FenceError::JournalOutcomeUnknown)
        );
        let mut rejected = TaskFence::after_journal(identity(), 24).unwrap();
        assert_eq!(
            rejected.transition_journaled(
                &identity(),
                1,
                true,
                TaskDisposition::Running,
                false,
                |_| Err(FenceError::JournalRejectedNoEffect)
            ),
            Err(FenceError::JournalRejectedNoEffect)
        );
        assert!(rejected.verify(&identity(), 1, true).is_ok());
    }
    #[test]
    fn source_effect_turn_and_call_are_not_model_interchangeable() {
        let task = TaskFence::after_journal(identity(), 1).unwrap();
        for variant in 0..4 {
            let mut wrong = identity();
            match variant {
                0 => wrong.actor = "other".into(),
                1 => wrong.execution_site = "remote".into(),
                2 => wrong.effect = ToolEffect::Read,
                _ => wrong.call_id = "another-call".into(),
            }
            assert_eq!(
                task.verify(&wrong, 1, true),
                Err(FenceError::WrongSourceOrScope)
            );
        }
    }
}
