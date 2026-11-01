//! Host-owned durable task interface and budget accounting.
//!
//! Dotlink can opt into this with its JobManager. AW binds it to the existing
//! TurnJournal/Session/Process owner. No second database, detached worker,
//! replay namespace or implicit model grant is created by this library.

use serde::{Deserialize, Serialize};
use std::{
    fmt,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

pub const MAX_ACTIVE_TASKS: usize = 32;
pub const MAX_RETAINED_TASKS: usize = 256;
pub const MAX_HIDDEN_REQUEST_REPLAYS: usize = 4096;
pub const MAX_SPOOL_PER_STREAM: u64 = 64 * 1024 * 1024;
pub const DEFAULT_SESSION_SPOOL: u64 = 512 * 1024 * 1024;
pub const MAX_SESSION_SPOOL: u64 = 8 * 1024 * 1024 * 1024;
pub const MAX_TASK_ERROR_BYTES: usize = 16 * 1024;

/// A host-owned output writer lease. Terminal publication/seal waits until
/// registered writer I/O has completed. No file, job or Tokio task is created
/// by the shared core itself.
const WRITERS_SEALED: usize = 1usize << (usize::BITS - 1);
const WRITER_COUNT_MASK: usize = !WRITERS_SEALED;

#[derive(Debug, Default)]
pub struct SpoolWriters {
    state: AtomicUsize,
    wait_mutex: Mutex<()>,
    idle: Condvar,
}
#[derive(Debug)]
pub struct SpoolWriterLease {
    owner: Arc<SpoolWriters>,
}
impl SpoolWriters {
    /// Reserve both stdout/stderr leases together or neither. The caller
    /// owns the underlying spools and cancellation policy.
    pub fn begin<const N: usize>(self: &Arc<Self>) -> Option<[SpoolWriterLease; N]> {
        if N == 0 {
            return Some(std::array::from_fn(|_| unreachable!()));
        }
        let mut current = self.state.load(Ordering::Acquire);
        loop {
            if current & WRITERS_SEALED != 0 {
                return None;
            }
            let count = current & WRITER_COUNT_MASK;
            if count > WRITER_COUNT_MASK.saturating_sub(N) {
                return None;
            }
            match self.state.compare_exchange_weak(
                current,
                current + N,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(std::array::from_fn(|_| SpoolWriterLease {
                        owner: self.clone(),
                    }))
                }
                Err(actual) => current = actual,
            }
        }
    }
    pub fn seal(&self) {
        self.state.fetch_or(WRITERS_SEALED, Ordering::AcqRel);
        // Writers have their own lease lifetime. A sealed gate can no longer
        // silently restart work after the final terminal receipt.
    }
    pub fn is_idle(&self) -> bool {
        self.state.load(Ordering::Acquire) & WRITER_COUNT_MASK == 0
    }
    pub fn is_sealed(&self) -> bool {
        self.state.load(Ordering::Acquire) & WRITERS_SEALED != 0
    }
    /// Synchronous wait with a hard upper bound. Call through host's blocking
    /// executor when the host itself is async; never block a Tokio reactor.
    pub fn seal_and_wait_idle(&self, limit: Duration) -> bool {
        self.seal();
        let start = Instant::now();
        let Ok(mut held) = self.wait_mutex.lock() else {
            return false;
        };
        loop {
            if self.is_idle() {
                return true;
            }
            let Some(remaining) = limit.checked_sub(start.elapsed()) else {
                return false;
            };
            match self.idle.wait_timeout(held, remaining) {
                Ok((lock, _)) => held = lock,
                Err(_) => return false,
            }
        }
    }
}
impl Drop for SpoolWriterLease {
    fn drop(&mut self) {
        let prior = self.owner.state.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(
            prior & WRITER_COUNT_MASK != 0,
            "spool writer count underflow"
        );
        if prior & WRITER_COUNT_MASK == 1 {
            // Synchronize with the check-and-sleep critical section. A plain
            // notify_all after a lock-free decrement can miss the final drop.
            let _held = self.owner.wait_mutex.lock();
            self.owner.idle.notify_all();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskDisposition {
    Queued,
    Running,
    Completed,
    TimedOut,
    Failed,
    Cancelled,
    Uncertain,
}
impl TaskDisposition {
    pub fn terminal(self) -> bool {
        !matches!(self, Self::Queued | Self::Running)
    }
    pub fn can_transition(self, next: Self) -> bool {
        self == next
            || matches!(
                (self, next),
                (Self::Queued, Self::Running | Self::Failed | Self::Cancelled | Self::Uncertain)
                    | (
                        Self::Running,
                        Self::Completed
                            | Self::TimedOut
                            | Self::Failed
                            | Self::Cancelled
                            | Self::Uncertain
                    )
                    // The host must prove final disposition before resolving
                    // an uncertain execution. The core never provides proof.
                    | (Self::Uncertain, Self::Completed | Self::Failed | Self::Cancelled)
            )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamSummary {
    pub total_bytes: u64,
    pub retained_bytes: u64,
    pub delivered_high_water: u64,
    pub acknowledged: u64,
    pub truncated: bool,
}
impl StreamSummary {
    /// One irreversible prefix-state transition for every source chunk.
    /// The physical writer reports the exact bytes it retained. If any
    /// source bytes were lost, later chunks can never fill that missing gap.
    pub fn record_chunk(&mut self, observed: u64, retained: u64) -> Result<(), TaskLimit> {
        if retained > observed
            || (self.truncated && retained != 0)
            || retained > MAX_SPOOL_PER_STREAM.saturating_sub(self.retained_bytes)
        {
            return Err(TaskLimit::InvalidStreamAccounting);
        }
        let total = self
            .total_bytes
            .checked_add(observed)
            .ok_or(TaskLimit::Overflow)?;
        let held = self
            .retained_bytes
            .checked_add(retained)
            .ok_or(TaskLimit::Overflow)?;
        self.total_bytes = total;
        self.retained_bytes = held;
        self.truncated |= retained < observed;
        Ok(())
    }
    /// Pure size-budget helper for synchronous callers. Once truncated,
    /// compaction or free quota cannot restart this source prefix.
    pub fn observe(&mut self, size: u64) -> Result<u64, TaskLimit> {
        let keep = if self.truncated {
            0
        } else {
            size.min(MAX_SPOOL_PER_STREAM.saturating_sub(self.retained_bytes))
        };
        self.record_chunk(size, keep)?;
        Ok(keep)
    }
    pub fn delivered(&mut self, through: u64) -> Result<(), TaskLimit> {
        if through > self.retained_bytes {
            return Err(TaskLimit::BeyondRetained);
        }
        self.delivered_high_water = self.delivered_high_water.max(through);
        Ok(())
    }
    /// Retention compaction changes future readable bytes, never historical
    /// delivery/acknowledgement evidence. A terminal spool is immutable;
    /// the host must first seal writers and await their quiescence.
    pub fn compact_retained(&mut self, keep: u64) -> Result<u64, TaskLimit> {
        if keep > self.retained_bytes {
            return Err(TaskLimit::BeyondRetained);
        }
        let released = self.retained_bytes - keep;
        self.retained_bytes = keep;
        self.truncated |= released != 0;
        Ok(released)
    }
    pub fn released(&self) -> bool {
        self.retained_bytes == 0 && self.total_bytes != 0
    }
    /// Acknowledge only bytes known delivered; never ack an unknown future
    /// offset or decrease a previously acknowledged safe high-water mark.
    pub fn acknowledge(&mut self, through: u64) -> Result<bool, TaskLimit> {
        if through > self.delivered_high_water {
            return Err(TaskLimit::BeyondDelivered);
        }
        if through <= self.acknowledged {
            return Ok(false);
        }
        self.acknowledged = through;
        Ok(true)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskLimit {
    TooManyActive,
    TooManyRetained,
    TooManyHidden,
    SpoolQuota,
    Overflow,
    BeyondRetained,
    BeyondDelivered,
    InvalidTransition,
    InvalidStreamAccounting,
}
impl fmt::Display for TaskLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "task budget or transition denied: {self:?}")
    }
}
impl std::error::Error for TaskLimit {}

#[derive(Clone, Copy, Debug)]
pub struct TaskLimits {
    pub active: usize,
    pub retained: usize,
    pub hidden: usize,
    pub spool: u64,
}
impl Default for TaskLimits {
    fn default() -> Self {
        Self {
            active: MAX_ACTIVE_TASKS,
            retained: MAX_RETAINED_TASKS,
            hidden: MAX_HIDDEN_REQUEST_REPLAYS,
            spool: DEFAULT_SESSION_SPOOL,
        }
    }
}
/// Admission policy only: the host owns the actual task and output ledger.
/// Keep hidden replay bookkeeping separate from user-visible retention slots.
#[derive(Clone, Copy, Debug, Default)]
pub struct TaskUsage {
    pub active: usize,
    pub retained: usize,
    pub hidden: usize,
    pub spool: u64,
}
impl TaskUsage {
    pub fn admit_task(self, limits: TaskLimits) -> Result<Self, TaskLimit> {
        if self.active >= limits.active {
            return Err(TaskLimit::TooManyActive);
        }
        if self.retained >= limits.retained {
            return Err(TaskLimit::TooManyRetained);
        }
        Ok(Self {
            active: self.active.checked_add(1).ok_or(TaskLimit::Overflow)?,
            retained: self.retained.checked_add(1).ok_or(TaskLimit::Overflow)?,
            ..self
        })
    }
    /// Terminal transitions free a running slot, but do not silently erase
    /// retained terminal output or a hidden request replay identity.
    pub fn settle_active(self) -> Result<Self, TaskLimit> {
        let active = self
            .active
            .checked_sub(1)
            .ok_or(TaskLimit::InvalidTransition)?;
        Ok(Self { active, ..self })
    }
    /// Retention is released only after the host confirms cleanup or expiry.
    pub fn release_retained(self) -> Result<Self, TaskLimit> {
        let retained = self
            .retained
            .checked_sub(1)
            .ok_or(TaskLimit::InvalidTransition)?;
        Ok(Self { retained, ..self })
    }
    pub fn reserve_spool(self, size: u64, limits: TaskLimits) -> Result<Self, TaskLimit> {
        let total = self.spool.checked_add(size).ok_or(TaskLimit::Overflow)?;
        if total > limits.spool {
            return Err(TaskLimit::SpoolQuota);
        }
        Ok(Self {
            spool: total,
            ..self
        })
    }
    pub fn reserve_hidden(self, limits: TaskLimits) -> Result<Self, TaskLimit> {
        if self.hidden >= limits.hidden {
            return Err(TaskLimit::TooManyHidden);
        }
        Ok(Self {
            hidden: self.hidden.checked_add(1).ok_or(TaskLimit::Overflow)?,
            ..self
        })
    }
    pub fn release_hidden(self) -> Result<Self, TaskLimit> {
        let hidden = self
            .hidden
            .checked_sub(1)
            .ok_or(TaskLimit::InvalidTransition)?;
        Ok(Self { hidden, ..self })
    }
    pub fn release_spool(self, size: u64) -> Result<Self, TaskLimit> {
        let spool = self
            .spool
            .checked_sub(size)
            .ok_or(TaskLimit::InvalidTransition)?;
        Ok(Self { spool, ..self })
    }
}

pub fn bounded_task_error(mut error: String) -> String {
    if error.len() <= MAX_TASK_ERROR_BYTES {
        return error;
    }
    let mut end = MAX_TASK_ERROR_BYTES.saturating_sub('…'.len_utf8());
    while !error.is_char_boundary(end) {
        end -= 1;
    }
    error.truncate(end);
    error.push('…');
    error
}

/// AW's durable journal or dotlink's job manager implements this. The
/// returned identity is exact-host generated, not a model-selected shell ID.
pub trait DurableTaskHost {
    type Id;
    type Error;
    fn authorize(&self, effect: super::ToolEffect) -> Result<(), Self::Error>;
    fn journal_intent(&self, key: &str) -> Result<Self::Id, Self::Error>;
    fn settle(&self, task: &Self::Id, state: TaskDisposition) -> Result<(), Self::Error>;
    fn inspect(&self, task: &Self::Id) -> Result<TaskDisposition, Self::Error>;
    fn output_page(
        &self,
        task: &Self::Id,
        stream: &str,
        offset: u64,
        limit: usize,
    ) -> Result<Vec<u8>, Self::Error>;
    fn acknowledge(&self, task: &Self::Id, stream: &str, through: u64) -> Result<(), Self::Error>;
    fn request_cancel(&self, task: &Self::Id) -> Result<(), Self::Error>;
}
#[derive(Clone, Copy, Debug)]
pub struct ExecutionDeadline {
    pub runtime: Duration,
    pub delivery_window: Duration,
}
impl ExecutionDeadline {
    /// The transport response window must not silently shorten execution.
    pub fn delivery_before_execution(self) -> bool {
        self.delivery_window < self.runtime
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sealed_spool_rejects_late_writers_and_waits_for_pair() {
        let owner = Arc::new(SpoolWriters::default());
        let [stdout, stderr] = owner.begin::<2>().unwrap();
        let waiting = owner.clone();
        let (ready, seen) = std::sync::mpsc::channel();
        let waiter = std::thread::spawn(move || {
            waiting.seal();
            ready.send(()).unwrap();
            waiting.seal_and_wait_idle(Duration::from_secs(3))
        });
        seen.recv_timeout(Duration::from_secs(3))
            .expect("sealing thread did not signal readiness");
        assert!(owner.is_sealed());
        assert!(owner.begin::<1>().is_none());
        drop(stdout);
        assert!(!owner.is_idle());
        drop(stderr);
        assert!(waiter.join().unwrap());
        assert!(owner.is_idle());
    }
    #[test]
    fn waiting_for_a_live_writer_times_out_without_hanging() {
        let owner = Arc::new(SpoolWriters::default());
        let [lease] = owner.begin::<1>().unwrap();
        assert!(!owner.seal_and_wait_idle(Duration::from_millis(1)));
        drop(lease);
        assert!(owner.seal_and_wait_idle(Duration::from_millis(10)));
        assert!(owner.begin::<1>().is_none());
    }
    #[test]
    fn writer_drop_notification_does_not_miss_the_final_transition() {
        for _ in 0..256 {
            let owner = Arc::new(SpoolWriters::default());
            let [lease] = owner.begin::<1>().unwrap();
            let wait = owner.clone();
            let waiter =
                std::thread::spawn(move || wait.seal_and_wait_idle(Duration::from_secs(1)));
            drop(lease);
            assert!(waiter.join().unwrap());
        }
    }

    #[test]
    fn admission_bounds_and_hidden_slots_are_separate() {
        let policy = TaskLimits::default();
        let usage = TaskUsage {
            active: 32,
            retained: 0,
            hidden: 4095,
            spool: 0,
        };
        assert_eq!(
            usage.admit_task(policy).unwrap_err(),
            TaskLimit::TooManyActive
        );
        assert_eq!(usage.reserve_hidden(policy).unwrap().hidden, 4096);
        assert_eq!(
            usage
                .reserve_hidden(policy)
                .unwrap()
                .reserve_hidden(policy)
                .unwrap_err(),
            TaskLimit::TooManyHidden
        );
        assert_eq!(
            TaskUsage {
                spool: policy.spool,
                ..Default::default()
            }
            .reserve_spool(1, policy)
            .unwrap_err(),
            TaskLimit::SpoolQuota
        );
    }
    #[test]
    fn irreversible_prefix_even_after_retention_compaction() {
        let mut summary = StreamSummary::default();
        summary.record_chunk(5, 2).unwrap();
        assert_eq!(summary.retained_bytes, 2);
        assert!(summary.truncated);
        assert_eq!(summary.observe(3).unwrap(), 0);
        assert_eq!(summary.total_bytes, 8);
        summary.compact_retained(0).unwrap();
        assert_eq!(summary.observe(3).unwrap(), 0);
        assert_eq!(summary.total_bytes, 11);
        assert_eq!(summary.retained_bytes, 0);
        assert_eq!(
            summary.record_chunk(1, 1),
            Err(TaskLimit::InvalidStreamAccounting)
        );
        assert_eq!(summary.total_bytes, 11);
    }
    #[test]
    fn output_ack_is_capped_by_delivered_not_written() {
        let mut summary = StreamSummary::default();
        assert_eq!(summary.observe(200).unwrap(), 200);
        assert_eq!(
            summary.acknowledge(1).unwrap_err(),
            TaskLimit::BeyondDelivered
        );
        summary.delivered(100).unwrap();
        assert!(summary.acknowledge(99).unwrap());
        assert!(!summary.acknowledge(50).unwrap());
        assert_eq!(summary.acknowledged, 99);
        assert_eq!(
            summary.acknowledge(150).unwrap_err(),
            TaskLimit::BeyondDelivered
        );
    }
    #[test]
    fn stale_task_lifecycle_cannot_return_to_running() {
        assert!(TaskDisposition::Queued.can_transition(TaskDisposition::Running));
        assert!(TaskDisposition::Running.can_transition(TaskDisposition::Uncertain));
        assert!(TaskDisposition::Queued.can_transition(TaskDisposition::Uncertain));
        assert!(TaskDisposition::Uncertain.can_transition(TaskDisposition::Completed));
        assert!(!TaskDisposition::Completed.can_transition(TaskDisposition::Running));
        assert!(TaskDisposition::Completed.terminal());
    }
    #[test]
    fn task_retention_is_separate_from_live_and_hidden_capacity() {
        let limits = TaskLimits::default();
        let start = TaskUsage::default();
        let started = start
            .admit_task(limits)
            .unwrap()
            .reserve_hidden(limits)
            .unwrap()
            .reserve_spool(4096, limits)
            .unwrap();
        let settled = started.settle_active().unwrap();
        assert_eq!(settled.active, 0);
        assert_eq!(settled.retained, 1);
        assert_eq!(settled.hidden, 1);
        assert_eq!(settled.spool, 4096);
        let after = settled
            .release_spool(4096)
            .unwrap()
            .release_retained()
            .unwrap()
            .release_hidden()
            .unwrap();
        assert_eq!(after.active, 0);
        assert_eq!(after.retained, 0);
        assert_eq!(after.hidden, 0);
        assert_eq!(after.spool, 0);
        assert_eq!(
            after.settle_active().unwrap_err(),
            TaskLimit::InvalidTransition
        );
        assert_eq!(
            after.release_spool(1).unwrap_err(),
            TaskLimit::InvalidTransition
        );
        assert_eq!(
            after.release_retained().unwrap_err(),
            TaskLimit::InvalidTransition
        );
    }
    #[test]
    fn compacting_retained_prefix_keeps_prior_delivery_history() {
        let mut stream = StreamSummary::default();
        stream.observe(12).unwrap();
        stream.delivered(10).unwrap();
        stream.acknowledge(9).unwrap();
        assert_eq!(stream.compact_retained(4).unwrap(), 8);
        assert_eq!(stream.retained_bytes, 4);
        assert_eq!(stream.delivered_high_water, 10);
        assert_eq!(stream.acknowledged, 9);
        assert!(stream.truncated);
        assert_eq!(stream.delivered(10).unwrap_err(), TaskLimit::BeyondRetained);
        assert_eq!(
            stream.compact_retained(5).unwrap_err(),
            TaskLimit::BeyondRetained
        );
        assert_eq!(stream.compact_retained(0).unwrap(), 4);
        assert!(stream.released());
        assert_eq!(stream.acknowledged, 9);
    }

    #[test]
    fn diagnostics_are_bounded_on_utf8_char_boundary() {
        let text = "雪".repeat(10000);
        let reduced = bounded_task_error(text);
        assert!(reduced.len() <= MAX_TASK_ERROR_BYTES);
        assert!(reduced.ends_with('…'));
    }
}
