//! Versioned host-journal spool checkpoint. A digest/identity is evidence of
//! exact bytes, not a grant to read the file or permission to start a process.
//! The authoritative Session/TurnJournal must durably own this record.

use crate::{
    task::{StreamSummary, TaskDisposition, MAX_SPOOL_PER_STREAM},
    task_fence::{TaskFence, TaskIdentity},
    task_spool::{spool_file_identity, BoundedFileSpool, SpoolFileIdentity},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    fs::File,
    io::{self},
};

pub const CHECKPOINT_VERSION: u16 = 2;
const LEGACY_CHECKPOINT_VERSION: u16 = 1;
const CHUNK: usize = 64 * 1024;

pub(crate) fn supported_checkpoint_version(version: u16) -> bool {
    matches!(version, LEGACY_CHECKPOINT_VERSION | CHECKPOINT_VERSION)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpoolCheckpoint {
    pub version: u16,
    pub task_binding: [u8; 32],
    pub intent_sequence: u64,
    pub generation: u64,
    pub disposition: TaskDisposition,
    pub file_identity: SpoolFileIdentity,
    pub retained_len: u64,
    /// Exact retained-prefix ceiling used by this stream. Version 1 omitted
    /// this field because the shared maximum was the only ceiling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_limit: Option<u64>,
    pub sha256: [u8; 32],
    pub stream: StreamSummary,
    pub output_ok: bool,
}
#[derive(Debug)]
pub enum CheckpointError {
    NotQuiesced,
    WrongVersion,
    WrongTask,
    StaleGeneration,
    InvalidSummary,
    ChangedFile,
    UnsettledTask,
    Io(io::Error),
}
impl From<io::Error> for CheckpointError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl SpoolCheckpoint {
    /// Construct only after writer sealing and flush. Snapshot the exact file
    /// identity, length and digest; fail closed if file contents can change.
    pub fn from_spool(
        spool: &BoundedFileSpool,
        fence: &TaskFence,
    ) -> Result<Self, CheckpointError> {
        if !spool.writers_quiesced() {
            return Err(CheckpointError::NotQuiesced);
        }
        if !fence.state().terminal() {
            return Err(CheckpointError::UnsettledTask);
        }
        if fence.state() == TaskDisposition::Completed && spool.output_failed() {
            return Err(CheckpointError::InvalidSummary);
        }
        let file = spool.file();
        let summary = spool.summary();
        let (length, sha256) = digest_file(file)?;
        if length != summary.retained_bytes {
            return Err(CheckpointError::ChangedFile);
        }
        Ok(Self {
            version: CHECKPOINT_VERSION,
            task_binding: fence.binding().fingerprint(),
            intent_sequence: fence.journal_intent_sequence(),
            generation: fence.generation(),
            disposition: fence.state(),
            file_identity: spool_file_identity(file).map_err(|_| CheckpointError::ChangedFile)?,
            retained_len: length,
            retained_limit: Some(spool.retention_limit()),
            sha256,
            stream: summary,
            output_ok: !spool.output_failed(),
        })
    }
    pub(crate) fn effective_retained_limit(&self) -> Result<u64, CheckpointError> {
        match (self.version, self.retained_limit) {
            (LEGACY_CHECKPOINT_VERSION, None) => Ok(MAX_SPOOL_PER_STREAM),
            (CHECKPOINT_VERSION, Some(limit)) if (1..=MAX_SPOOL_PER_STREAM).contains(&limit) => {
                Ok(limit)
            }
            _ => Err(CheckpointError::InvalidSummary),
        }
    }

    pub fn validate(
        &self,
        file: &File,
        identity: &TaskIdentity,
        journal_sequence: u64,
        expected_generation: u64,
    ) -> Result<(), CheckpointError> {
        if !supported_checkpoint_version(self.version) {
            return Err(CheckpointError::WrongVersion);
        }
        if self.task_binding != identity.fingerprint() || self.intent_sequence != journal_sequence {
            return Err(CheckpointError::WrongTask);
        }
        if self.generation != expected_generation {
            return Err(CheckpointError::StaleGeneration);
        }
        if !self.disposition.terminal() {
            return Err(CheckpointError::UnsettledTask);
        }
        let retained_limit = self.effective_retained_limit()?;
        if (self.disposition == TaskDisposition::Completed && !self.output_ok)
            || self.stream.retained_bytes != self.retained_len
            || self.retained_len > retained_limit
            || self.stream.total_bytes < self.stream.retained_bytes
            || self.stream.acknowledged > self.stream.delivered_high_water
            || self.stream.delivered_high_water > self.stream.total_bytes
            || (!self.stream.truncated && self.stream.total_bytes != self.stream.retained_bytes)
        {
            return Err(CheckpointError::InvalidSummary);
        }
        if spool_file_identity(file).map_err(|_| CheckpointError::ChangedFile)?
            != self.file_identity
        {
            return Err(CheckpointError::ChangedFile);
        }
        let (len, digest) = digest_file(file)?;
        if len != self.retained_len || digest != self.sha256 {
            return Err(CheckpointError::ChangedFile);
        }
        Ok(())
    }
}

/// Read-at avoids the duplicated-handle shared seek cursor. Checkpoints
/// must not alter the position of a simultaneous host-owned output reader.
#[cfg(unix)]
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;
    file.read_at(buf, offset)
}
#[cfg(windows)]
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_read(buf, offset)
}
fn digest_file(file: &File) -> Result<(u64, [u8; 32]), CheckpointError> {
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > MAX_SPOOL_PER_STREAM {
        return Err(CheckpointError::InvalidSummary);
    }
    let len = meta.len();
    let mut digest = Sha256::new();
    let mut buf = [0u8; CHUNK];
    let mut offset = 0u64;
    while offset < len {
        let take = usize::try_from((len - offset).min(CHUNK as u64))
            .map_err(|_| CheckpointError::InvalidSummary)?;
        let read = read_at(file, &mut buf[..take], offset)?;
        if read == 0 {
            return Err(CheckpointError::ChangedFile);
        }
        digest.update(&buf[..read]);
        offset = offset
            .checked_add(read as u64)
            .ok_or(CheckpointError::InvalidSummary)?;
    }
    let after = file.metadata()?;
    if after.len() != len {
        return Err(CheckpointError::ChangedFile);
    }
    Ok((len, digest.finalize().into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{task::SpoolWriters, task_spool::SessionQuota, ToolEffect};
    use std::{
        fs::{self, OpenOptions},
        io::{Seek, SeekFrom},
        sync::{Arc, Mutex},
        time::Duration,
    };
    fn fixture() -> (std::path::PathBuf, File, TaskIdentity, TaskFence) {
        let path = std::env::temp_dir().join(format!(
            "aw-spool-checkpoint-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let identity = TaskIdentity {
            actor: "actor".into(),
            source_kind: "host".into(),
            source_id: "trusted".into(),
            source_epoch: 9,
            execution_site: "local".into(),
            session: "s".into(),
            branch: "b".into(),
            turn_id: 5,
            call_id: "call".into(),
            effect: ToolEffect::Process,
        };
        let mut fence = TaskFence::after_journal(identity.clone(), 3).unwrap();
        fence
            .transition_journaled(&identity, 1, true, TaskDisposition::Running, false, |_| {
                Ok(())
            })
            .unwrap();
        fence
            .transition_journaled(
                &identity,
                2,
                true,
                TaskDisposition::Completed,
                false,
                |_| Ok(()),
            )
            .unwrap();
        (path, file, identity, fence)
    }
    #[test]
    fn snapshot_matches_pinned_bytes_and_exact_task_identity() {
        let (path, file, identity, fence) = fixture();
        let handle = file.try_clone().unwrap();
        let gate = Arc::new(SpoolWriters::default());
        let quota = Arc::new(Mutex::new(SessionQuota::new(1024)));
        let mut spool = BoundedFileSpool::new(file, quota, gate.clone()).unwrap();
        assert_eq!(spool.append(b"hello world").unwrap(), 11);
        spool.finish_writer().unwrap();
        assert!(gate.seal_and_wait_idle(Duration::from_secs(1)));
        let checkpoint = SpoolCheckpoint::from_spool(&spool, &fence).unwrap();
        let serialized = serde_json::to_vec(&checkpoint).unwrap();
        let recovered: SpoolCheckpoint = serde_json::from_slice(&serialized).unwrap();
        recovered.validate(&handle, &identity, 3, 3).unwrap();
        let mut wrong = identity.clone();
        wrong.source_epoch += 1;
        assert!(matches!(
            recovered.validate(&handle, &wrong, 3, 3),
            Err(CheckpointError::WrongTask)
        ));
        assert!(matches!(
            recovered.validate(&handle, &identity, 4, 3),
            Err(CheckpointError::WrongTask)
        ));
        assert!(matches!(
            recovered.validate(&handle, &identity, 3, 2),
            Err(CheckpointError::StaleGeneration)
        ));
        drop(spool);
        drop(handle);
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn tampered_or_truncated_disk_and_metadata_cannot_be_accepted() {
        let (path, file, identity, fence) = fixture();
        let mut handle = file.try_clone().unwrap();
        let gate = Arc::new(SpoolWriters::default());
        let quota = Arc::new(Mutex::new(SessionQuota::new(1024)));
        let mut spool = BoundedFileSpool::new(file, quota, gate.clone()).unwrap();
        assert_eq!(spool.append(b"correct").unwrap(), 7);
        spool.finish_writer().unwrap();
        gate.seal();
        let checkpoint = SpoolCheckpoint::from_spool(&spool, &fence).unwrap();
        let mut changed = checkpoint.clone();
        changed.version = 99;
        assert!(matches!(
            changed.validate(&handle, &identity, 3, 3),
            Err(CheckpointError::WrongVersion)
        ));
        changed = checkpoint.clone();
        changed.output_ok = false;
        assert!(matches!(
            changed.validate(&handle, &identity, 3, 3),
            Err(CheckpointError::InvalidSummary)
        ));
        changed = checkpoint.clone();
        changed.retained_limit = Some(0);
        assert!(matches!(
            changed.validate(&handle, &identity, 3, 3),
            Err(CheckpointError::InvalidSummary)
        ));
        changed = checkpoint.clone();
        changed.retained_limit = Some(changed.retained_len.saturating_sub(1));
        assert!(matches!(
            changed.validate(&handle, &identity, 3, 3),
            Err(CheckpointError::InvalidSummary)
        ));
        changed = checkpoint.clone();
        changed.stream.acknowledged = 999;
        assert!(matches!(
            changed.validate(&handle, &identity, 3, 3),
            Err(CheckpointError::InvalidSummary)
        ));
        use std::io::Write;
        handle.seek(SeekFrom::Start(0)).unwrap();
        handle.write_all(b"CORRUPT").unwrap();
        assert!(matches!(
            checkpoint.validate(&handle, &identity, 3, 3),
            Err(CheckpointError::ChangedFile)
        ));
        drop(spool);
        drop(handle);
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn digest_does_not_move_shared_file_cursor() {
        use std::io::{Seek, SeekFrom};
        let (path, mut file, _, _) = fixture();
        use std::io::Write;
        file.write_all(b"unchanged data").unwrap();
        file.seek(SeekFrom::Start(7)).unwrap();
        let other = file.try_clone().unwrap();
        let (_, hash) = digest_file(&other).unwrap();
        assert_eq!(hash, <[u8; 32]>::from(Sha256::digest(b"unchanged data")));
        assert_eq!(file.stream_position().unwrap(), 7);
        drop(other);
        drop(file);
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn cannot_checkpoint_active_writers() {
        let (path, file, _, fence) = fixture();
        let quota = Arc::new(Mutex::new(SessionQuota::new(1024)));
        let gate = Arc::new(SpoolWriters::default());
        let spool = BoundedFileSpool::new(file, quota, gate).unwrap();
        assert!(matches!(
            SpoolCheckpoint::from_spool(&spool, &fence),
            Err(CheckpointError::NotQuiesced)
        ));
        drop(spool);
        fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod restore_tests {
    use super::*;
    use crate::{
        task::{SpoolWriters, TaskDisposition},
        task_spool::SessionQuota,
        ToolEffect,
    };
    use std::{
        fs::{self, OpenOptions},
        sync::{Arc, Mutex},
        time::Duration,
    };
    fn setup() -> (std::path::PathBuf, File, TaskIdentity, TaskFence) {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "aw-checkpoint-restore-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let id = TaskIdentity {
            actor: "owner".into(),
            source_kind: "host".into(),
            source_id: "native".into(),
            source_epoch: 1,
            execution_site: "local".into(),
            session: "s".into(),
            branch: "main".into(),
            turn_id: 2,
            call_id: "call".into(),
            effect: ToolEffect::Process,
        };
        let mut fence = TaskFence::after_journal(id.clone(), 18).unwrap();
        fence
            .transition_journaled(&id, 1, true, TaskDisposition::Running, false, |_| Ok(()))
            .unwrap();
        fence
            .transition_journaled(&id, 2, true, TaskDisposition::Completed, false, |_| Ok(()))
            .unwrap();
        (path, file, id, fence)
    }
    #[test]
    fn recovery_recharges_quota_and_preserves_exact_ack_history() {
        let (path, file, id, fence) = setup();
        let quota = Arc::new(Mutex::new(SessionQuota::new(20)));
        let gate = Arc::new(SpoolWriters::default());
        let mut spool = BoundedFileSpool::new(file, quota.clone(), gate.clone()).unwrap();
        spool.append(b"abcdefghij").unwrap();
        spool.finish_writer().unwrap();
        assert!(gate.seal_and_wait_idle(Duration::from_secs(1)));
        assert_eq!(spool.page(0, 7).unwrap(), b"abcdefg");
        assert!(spool.acknowledge(7).unwrap());
        let checkpoint = SpoolCheckpoint::from_spool(&spool, &fence).unwrap();
        assert_eq!(quota.lock().unwrap().used(), 10);
        drop(spool);
        assert_eq!(quota.lock().unwrap().used(), 0);
        let handle = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let mut recovered =
            BoundedFileSpool::restore_verified(handle, quota.clone(), &checkpoint, &id, 18, 3)
                .unwrap();
        assert_eq!(quota.lock().unwrap().used(), 10);
        assert_eq!(recovered.page(7, 8).unwrap(), b"hij");
        assert!(recovered.acknowledge(10).unwrap());
        assert_eq!(recovered.summary().acknowledged, 10);
        assert_eq!(recovered.compact_retained(4).unwrap(), 6);
        assert_eq!(quota.lock().unwrap().used(), 4);
        drop(recovered);
        assert_eq!(quota.lock().unwrap().used(), 0);
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn recovery_preserves_narrow_retained_prefix_limit_and_old_v1_defaults() {
        let (path, file, id, fence) = setup();
        let quota = Arc::new(Mutex::new(SessionQuota::new(32)));
        let gate = Arc::new(SpoolWriters::default());
        let mut spool =
            BoundedFileSpool::new_with_limit(file, quota.clone(), gate.clone(), 4).unwrap();
        assert_eq!(spool.append(b"abcdefgh").unwrap(), 4);
        spool.finish_writer().unwrap();
        assert!(gate.seal_and_wait_idle(Duration::from_secs(1)));
        let checkpoint = SpoolCheckpoint::from_spool(&spool, &fence).unwrap();
        assert_eq!(checkpoint.retained_limit, Some(4));
        assert_eq!(checkpoint.retained_len, 4);
        assert!(checkpoint.stream.truncated);
        let wire = serde_json::to_vec(&checkpoint).unwrap();
        let decoded: SpoolCheckpoint = serde_json::from_slice(&wire).unwrap();
        assert_eq!(decoded.retained_limit, Some(4));
        drop(spool);
        let handle = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let recovered =
            BoundedFileSpool::restore_verified(handle, quota.clone(), &decoded, &id, 18, 3)
                .unwrap();
        assert_eq!(recovered.retention_limit(), 4);
        drop(recovered);

        let mut old = serde_json::to_value(&decoded).unwrap();
        old["version"] = serde_json::json!(LEGACY_CHECKPOINT_VERSION);
        old.as_object_mut().unwrap().remove("retained_limit");
        let old: SpoolCheckpoint = serde_json::from_value(old).unwrap();
        assert_eq!(old.retained_limit, None);
        assert_eq!(
            old.effective_retained_limit().unwrap(),
            MAX_SPOOL_PER_STREAM
        );
        old.validate(
            &OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .unwrap(),
            &id,
            18,
            3,
        )
        .unwrap();
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn recovery_refuses_unsettled_or_corrupt_and_insufficient_quota() {
        let (path, file, id, fence) = setup();
        let quota = Arc::new(Mutex::new(SessionQuota::new(15)));
        let gate = Arc::new(SpoolWriters::default());
        let mut spool = BoundedFileSpool::new(file, quota.clone(), gate.clone()).unwrap();
        spool.append(b"123456789").unwrap();
        spool.finish_writer().unwrap();
        gate.seal();
        let checkpoint = SpoolCheckpoint::from_spool(&spool, &fence).unwrap();
        drop(spool);
        let handle = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let tiny = Arc::new(Mutex::new(SessionQuota::new(5)));
        assert!(matches!(
            BoundedFileSpool::restore_verified(handle, tiny.clone(), &checkpoint, &id, 18, 3),
            Err(crate::task_spool::SpoolError::QuotaExhausted)
        ));
        assert_eq!(tiny.lock().unwrap().used(), 0);
        let mut queued = checkpoint.clone();
        queued.disposition = TaskDisposition::Running;
        let handle = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        assert!(matches!(
            BoundedFileSpool::restore_verified(handle, quota.clone(), &queued, &id, 18, 3),
            Err(crate::task_spool::SpoolError::RecoveryRejected)
        ));
        let mut changed = checkpoint.clone();
        changed.sha256[0] ^= 1;
        let handle = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        assert!(matches!(
            BoundedFileSpool::restore_verified(handle, quota.clone(), &changed, &id, 18, 3),
            Err(crate::task_spool::SpoolError::RecoveryRejected)
        ));
        assert_eq!(quota.lock().unwrap().used(), 0);
        fs::remove_file(path).unwrap();
    }
}
