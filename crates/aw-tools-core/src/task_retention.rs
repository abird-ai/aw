//! Host-owned task retention decisions; no automatic deletion or background
//! sweeper. A resource must be physically disposed before retained budget is
//! released. The host's journal owns time, CAS and cleanup evidence.

use crate::{
    task::{TaskDisposition, MAX_SPOOL_PER_STREAM},
    task_checkpoint::{SpoolCheckpoint, CHECKPOINT_VERSION},
    task_fence::TaskIdentity,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetentionError {
    InvalidDeadline,
    NotTerminal,
    WrongTask,
    StaleGeneration,
    WritersActive,
    NotExpired,
    CleanupUnverified,
    InvalidCheckpoint,
    AlreadyReleased,
}
/// Persistable host-authored policy metadata. Neither source identity nor a
/// model tool response may extend retention implicitly.
#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionLease {
    task_binding: [u8; 32],
    generation: u64,
    journal_sequence: u64,
    expires_utc_ms: i64,
    retained_bytes: u64,
    released: bool,
}
#[derive(Debug, PartialEq, Eq)]
pub struct CleanupAuthority<'a> {
    lease: &'a mut RetentionLease,
    retained_bytes: u64,
    generation: u64,
    journal_sequence: u64,
}
impl CleanupAuthority<'_> {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn journal_sequence(&self) -> u64 {
        self.journal_sequence
    }
    pub fn retained_bytes(&self) -> u64 {
        self.retained_bytes
    }
}
impl RetentionLease {
    pub fn for_checkpoint(
        checkpoint: &SpoolCheckpoint,
        identity: &TaskIdentity,
        expires_utc_ms: i64,
    ) -> Result<Self, RetentionError> {
        if expires_utc_ms <= 0 {
            return Err(RetentionError::InvalidDeadline);
        }
        if !checkpoint.disposition.terminal()
            || checkpoint.disposition == TaskDisposition::Uncertain
        {
            return Err(RetentionError::NotTerminal);
        }
        if checkpoint.version != CHECKPOINT_VERSION
            || checkpoint.intent_sequence == 0
            || checkpoint.generation == 0
            || checkpoint.retained_len > MAX_SPOOL_PER_STREAM
            || checkpoint.stream.retained_bytes != checkpoint.retained_len
            || checkpoint.stream.total_bytes < checkpoint.retained_len
            || checkpoint.stream.acknowledged > checkpoint.stream.delivered_high_water
            || checkpoint.stream.delivered_high_water > checkpoint.stream.total_bytes
            || (!checkpoint.stream.truncated
                && checkpoint.stream.total_bytes != checkpoint.retained_len)
        {
            return Err(RetentionError::InvalidCheckpoint);
        }
        if checkpoint.task_binding != identity.fingerprint() {
            return Err(RetentionError::WrongTask);
        }
        Ok(Self {
            task_binding: checkpoint.task_binding,
            generation: checkpoint.generation,
            journal_sequence: checkpoint.intent_sequence,
            expires_utc_ms,
            retained_bytes: checkpoint.retained_len,
            released: false,
        })
    }
    pub fn expiry_ms(&self) -> i64 {
        self.expires_utc_ms
    }
    pub fn eligible(
        &mut self,
        identity: &TaskIdentity,
        generation: u64,
        journal_sequence: u64,
        state: TaskDisposition,
        now_utc_ms: i64,
        writers_finished: bool,
    ) -> Result<CleanupAuthority<'_>, RetentionError> {
        if self.released {
            return Err(RetentionError::AlreadyReleased);
        }
        if self.task_binding != identity.fingerprint() || self.journal_sequence != journal_sequence
        {
            return Err(RetentionError::WrongTask);
        }
        if self.generation != generation {
            return Err(RetentionError::StaleGeneration);
        }
        if !state.terminal() || state == TaskDisposition::Uncertain {
            return Err(RetentionError::NotTerminal);
        }
        if !writers_finished {
            return Err(RetentionError::WritersActive);
        }
        if now_utc_ms < self.expires_utc_ms {
            return Err(RetentionError::NotExpired);
        }
        Ok(CleanupAuthority {
            retained_bytes: self.retained_bytes,
            generation: self.generation,
            journal_sequence: self.journal_sequence,
            lease: self,
        })
    }
}
impl CleanupAuthority<'_> {
    /// Convert an eligibility decision into a budget-release receipt only
    /// after the *existing* host actually disposes/quarantines the file.
    /// Consumes the authority so the host cannot reuse it accidentally.
    pub fn confirmed(self, files_disposed: bool) -> Result<u64, RetentionError> {
        if !files_disposed {
            return Err(RetentionError::CleanupUnverified);
        }
        self.lease.released = true;
        Ok(self.retained_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{task::StreamSummary, ToolEffect};
    fn input() -> (TaskIdentity, SpoolCheckpoint) {
        let identity = TaskIdentity {
            actor: "owner".into(),
            source_kind: "host".into(),
            source_id: "source".into(),
            source_epoch: 9,
            execution_site: "local".into(),
            session: "s".into(),
            branch: "main".into(),
            turn_id: 3,
            call_id: "c".into(),
            effect: ToolEffect::Process,
        };
        let checkpoint = SpoolCheckpoint {
            version: 1,
            task_binding: identity.fingerprint(),
            intent_sequence: 77,
            generation: 5,
            disposition: TaskDisposition::Completed,
            file_identity: serde_json::from_value(serde_json::json!({
                "volume":1,"file":2
            }))
            .unwrap(),
            retained_len: 20,
            sha256: [1; 32],
            stream: StreamSummary {
                total_bytes: 20,
                retained_bytes: 20,
                delivered_high_water: 0,
                acknowledged: 0,
                truncated: false,
            },
            output_ok: true,
        };
        (identity, checkpoint)
    }
    #[test]
    fn time_or_unsettled_process_cannot_expire_active_output() {
        let (identity, cp) = input();
        let mut lease = RetentionLease::for_checkpoint(&cp, &identity, 100).unwrap();
        for (now, state, writers, expected) in [
            (
                99,
                TaskDisposition::Completed,
                true,
                RetentionError::NotExpired,
            ),
            (
                100,
                TaskDisposition::Running,
                true,
                RetentionError::NotTerminal,
            ),
            (
                100,
                TaskDisposition::Completed,
                false,
                RetentionError::WritersActive,
            ),
        ] {
            assert_eq!(
                lease.eligible(&identity, 5, 77, state, now, writers),
                Err(expected)
            );
        }
        assert_eq!(
            lease.eligible(&identity, 4, 77, TaskDisposition::Completed, 100, true),
            Err(RetentionError::StaleGeneration)
        );
        let mut changed = identity.clone();
        changed.source_epoch += 1;
        assert_eq!(
            lease.eligible(&changed, 5, 77, TaskDisposition::Completed, 100, true),
            Err(RetentionError::WrongTask)
        );
    }
    #[test]
    fn cleanup_requires_verified_host_disposal() {
        let (identity, cp) = input();
        let mut lease = RetentionLease::for_checkpoint(&cp, &identity, 100).unwrap();
        let allowed = lease
            .eligible(&identity, 5, 77, TaskDisposition::Completed, 100, true)
            .unwrap();
        assert_eq!(
            allowed.confirmed(false),
            Err(RetentionError::CleanupUnverified)
        );
        let allowed = lease
            .eligible(&identity, 5, 77, TaskDisposition::Completed, 100, true)
            .unwrap();
        assert_eq!(allowed.confirmed(true), Ok(20));
        assert_eq!(
            lease.eligible(&identity, 5, 77, TaskDisposition::Completed, 101, true),
            Err(RetentionError::AlreadyReleased)
        );
        let restored: RetentionLease =
            serde_json::from_slice(&serde_json::to_vec(&lease).unwrap()).unwrap();
        assert!(restored.released);
    }
    #[test]
    fn uncertain_external_effect_is_not_retention_expiry_evidence() {
        let (identity, mut cp) = input();
        cp.disposition = TaskDisposition::Uncertain;
        assert_eq!(
            RetentionLease::for_checkpoint(&cp, &identity, 100).err(),
            Some(RetentionError::NotTerminal)
        );
        cp.disposition = TaskDisposition::Failed;
        let mut lease = RetentionLease::for_checkpoint(&cp, &identity, 100).unwrap();
        assert_eq!(
            lease.eligible(&identity, 5, 77, TaskDisposition::Uncertain, 200, true),
            Err(RetentionError::NotTerminal)
        );
    }
    #[test]
    fn invalid_checkpoint_cannot_mint_retention_release_authority() {
        let (identity, mut cp) = input();
        cp.version = 99;
        assert_eq!(
            RetentionLease::for_checkpoint(&cp, &identity, 100).err(),
            Some(RetentionError::InvalidCheckpoint)
        );
        cp.version = CHECKPOINT_VERSION;
        cp.retained_len = MAX_SPOOL_PER_STREAM + 1;
        assert_eq!(
            RetentionLease::for_checkpoint(&cp, &identity, 100).err(),
            Some(RetentionError::InvalidCheckpoint)
        );
        cp.retained_len = 20;
        cp.stream.acknowledged = 1;
        assert_eq!(
            RetentionLease::for_checkpoint(&cp, &identity, 100).err(),
            Some(RetentionError::InvalidCheckpoint)
        );
        cp.stream.acknowledged = 0;
        let retained = RetentionLease::for_checkpoint(&cp, &identity, 100).unwrap();
        let decoded: RetentionLease =
            serde_json::from_slice(&serde_json::to_vec(&retained).unwrap()).unwrap();
        assert_eq!(decoded.expiry_ms(), 100);
    }
    #[test]
    fn source_or_generation_mismatch_fails_before_cleanup() {
        let (identity, mut cp) = input();
        cp.disposition = TaskDisposition::Running;
        assert!(matches!(
            RetentionLease::for_checkpoint(&cp, &identity, 100),
            Err(RetentionError::NotTerminal)
        ));
        cp.disposition = TaskDisposition::Failed;
        cp.task_binding = [0; 32];
        assert!(matches!(
            RetentionLease::for_checkpoint(&cp, &identity, 100),
            Err(RetentionError::WrongTask)
        ));
        assert_eq!(
            RetentionLease::for_checkpoint(&cp, &identity, 0).err(),
            Some(RetentionError::InvalidDeadline)
        );
    }
}
