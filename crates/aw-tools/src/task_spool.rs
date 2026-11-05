//! Bounded byte-exact spool over an already opened host-owned file.
//! This module has no path creation, task registry, subprocess, source
//! authority, or automatic background execution. Host opens/owns the file
//! and must durably journal the task before attaching this writer.

use crate::task::{
    SpoolWriterLease, SpoolWriters, StreamSummary, TaskLimit, MAX_SESSION_SPOOL,
    MAX_SPOOL_PER_STREAM,
};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    sync::{Arc, Mutex},
};
#[derive(Debug)]
pub enum SpoolError {
    Io(io::Error),
    NotEmptyOrRegular,
    NotReadableWritable,
    AlreadyClosed,
    NotQuiesced,
    Poisoned,
    Overflow,
    BadPage,
    Undelivered,
    WriteFailed,
    RecoveryRejected,
    QuotaExhausted,
}
impl From<io::Error> for SpoolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<TaskLimit> for SpoolError {
    fn from(error: TaskLimit) -> Self {
        match error {
            TaskLimit::BeyondDelivered => Self::Undelivered,
            _ => Self::Overflow,
        }
    }
}
/// Files are identified by opened-handle identity, not caller-supplied
/// pathnames. The Process host uses the same primitive on Windows and Unix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SpoolFileIdentity {
    volume: u64,
    file: u64,
}
pub fn spool_file_identity(file: &File) -> Result<SpoolFileIdentity, SpoolError> {
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(SpoolError::NotEmptyOrRegular);
    }
    #[cfg(unix)]
    {
        use std::os::unix::{fs::MetadataExt, io::AsRawFd};
        if meta.nlink() != 1 {
            return Err(SpoolError::NotEmptyOrRegular);
        }
        // SAFETY: F_GETFL reads the flags on an existing owned descriptor.
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
        if flags < 0 {
            return Err(SpoolError::Io(io::Error::last_os_error()));
        }
        if flags & libc::O_ACCMODE != libc::O_RDWR {
            return Err(SpoolError::NotReadableWritable);
        }
        Ok(SpoolFileIdentity {
            volume: meta.dev(),
            file: meta.ino(),
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        // SAFETY: open File handle and initialized output buffer.
        let mut info = unsafe { std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(SpoolError::Io(io::Error::last_os_error()));
        }
        if info.nNumberOfLinks != 1 {
            return Err(SpoolError::NotEmptyOrRegular);
        }
        // Read access plus ability to set the *current* length validates a
        // read/write handle without writing over any recovered bytes.
        let mut probe = file.try_clone()?;
        let cursor = probe.stream_position()?;
        let mut byte = [0u8; 1];
        let tested = probe
            .seek(SeekFrom::Start(0))
            .and_then(|_| probe.read(&mut byte).map(|_| ()))
            .and_then(|_| probe.set_len(meta.len()));
        let restored = probe.seek(SeekFrom::Start(cursor));
        if tested.is_err() || restored.is_err() {
            return Err(SpoolError::NotReadableWritable);
        }
        Ok(SpoolFileIdentity {
            volume: u64::from(info.dwVolumeSerialNumber),
            file: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        })
    }
}
/// RAII ownership of retained quota, independent of temporary append
/// reservations. Normal drop always releases the currently retained bytes;
/// a durable host must retain this owner for as long as the stored prefix is
/// alive, rather than silently detaching a raw file.
struct RetainedCharge {
    quota: Arc<Mutex<SessionQuota>>,
    held: u64,
}
impl RetainedCharge {
    fn new(quota: Arc<Mutex<SessionQuota>>) -> Self {
        Self { quota, held: 0 }
    }
    fn add(&mut self, n: u64) {
        self.held += n;
    }
    fn release(&mut self, n: u64) -> Result<(), SpoolError> {
        if n > self.held {
            return Err(SpoolError::Overflow);
        }
        self.quota
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .release(n)?;
        self.held -= n;
        Ok(())
    }
}
impl Drop for RetainedCharge {
    fn drop(&mut self) {
        if self.held > 0 {
            let _ = self
                .quota
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .release(self.held);
            self.held = 0;
        }
    }
}

/// Shared raw-page safety ceiling. Product hosts apply their tighter serialized
/// response budgets (AW advertises 64 KiB; dotlink may use larger transport-safe
/// pages). A raw page can never exceed one retained stream.
pub const MAX_SPOOL_PAGE_BYTES: usize = MAX_SPOOL_PER_STREAM as usize;

#[derive(Debug)]
pub struct SessionQuota {
    used: u64,
    limit: u64,
}
impl SessionQuota {
    pub fn new(limit: u64) -> Self {
        Self {
            used: 0,
            limit: limit.min(MAX_SESSION_SPOOL),
        }
    }
    pub fn used(&self) -> u64 {
        self.used
    }
    pub fn limit(&self) -> u64 {
        self.limit
    }
    fn reserve(&mut self, wanted: u64) -> u64 {
        let held = wanted.min(self.limit.saturating_sub(self.used));
        self.used += held;
        held
    }
    fn release(&mut self, bytes: u64) -> Result<(), SpoolError> {
        self.used = self.used.checked_sub(bytes).ok_or(SpoolError::Overflow)?;
        Ok(())
    }
}
/// A reservation owns every reserved byte until the spool's accounting is
/// successfully committed. On any early error, Drop returns all untransferred
/// bytes; no disk write can accidentally leak session capacity.
struct Reservation {
    session: Arc<Mutex<SessionQuota>>,
    reserved: u64,
    transferred: u64,
}
impl Reservation {
    fn new(session: &Arc<Mutex<SessionQuota>>, want: u64) -> Result<Self, SpoolError> {
        let reserved = session
            .lock()
            .map_err(|_| SpoolError::Poisoned)?
            .reserve(want);
        Ok(Self {
            session: session.clone(),
            reserved,
            transferred: 0,
        })
    }
    fn accepted(&self) -> u64 {
        self.reserved
    }
    fn transfer(&mut self, bytes: u64) {
        debug_assert!(bytes <= self.reserved - self.transferred);
        self.transferred += bytes;
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        let unused = self.reserved - self.transferred;
        let _ = self
            .session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .release(unused);
    }
}

/// Exact bounded in-memory retained output. Used for native Graph results so
/// cross-platform Job result custody does not depend on private temporary files.
/// Capacity is prepaid against the same SessionQuota before any effect starts.
pub struct BoundedMemorySpool {
    bytes: Vec<u8>,
    quota: Arc<Mutex<SessionQuota>>,
    limit: u64,
    retained: RetainedCharge,
    prepaid: Option<RetainedCharge>,
    prepaid_remaining: u64,
    gate: Arc<SpoolWriters>,
    lease: Option<SpoolWriterLease>,
    summary: StreamSummary,
    failed: bool,
    closed: bool,
}
impl BoundedMemorySpool {
    pub fn new(
        quota: Arc<Mutex<SessionQuota>>,
        gate: Arc<SpoolWriters>,
    ) -> Result<Self, SpoolError> {
        Self::new_with_limit(quota, gate, MAX_SPOOL_PER_STREAM)
    }

    pub fn new_with_limit(
        quota: Arc<Mutex<SessionQuota>>,
        gate: Arc<SpoolWriters>,
        limit: u64,
    ) -> Result<Self, SpoolError> {
        if limit == 0 || limit > MAX_SPOOL_PER_STREAM {
            return Err(SpoolError::QuotaExhausted);
        }
        let [lease] = gate.begin::<1>().ok_or(SpoolError::AlreadyClosed)?;
        Ok(Self {
            bytes: Vec::new(),
            retained: RetainedCharge::new(quota.clone()),
            prepaid: None,
            prepaid_remaining: 0,
            quota,
            limit,
            gate,
            lease: Some(lease),
            summary: StreamSummary::default(),
            failed: false,
            closed: false,
        })
    }

    pub fn new_reserved(
        quota: Arc<Mutex<SessionQuota>>,
        gate: Arc<SpoolWriters>,
        capacity: u64,
    ) -> Result<Self, SpoolError> {
        if capacity == 0 || capacity > MAX_SPOOL_PER_STREAM {
            return Err(SpoolError::QuotaExhausted);
        }
        let [lease] = gate.begin::<1>().ok_or(SpoolError::AlreadyClosed)?;
        let mut reservation = Reservation::new(&quota, capacity)?;
        if reservation.accepted() != capacity {
            return Err(SpoolError::QuotaExhausted);
        }
        let cap = usize::try_from(capacity).map_err(|_| SpoolError::Overflow)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(cap)
            .map_err(|_| SpoolError::QuotaExhausted)?;
        let mut prepaid = RetainedCharge::new(quota.clone());
        prepaid.add(capacity);
        reservation.transfer(capacity);
        Ok(Self {
            bytes,
            retained: RetainedCharge::new(quota.clone()),
            prepaid: Some(prepaid),
            prepaid_remaining: capacity,
            quota,
            limit: capacity,
            gate,
            lease: Some(lease),
            summary: StreamSummary::default(),
            failed: false,
            closed: false,
        })
    }

    pub fn discard_if_full(&mut self, seen: u64) -> Result<bool, SpoolError> {
        if self.closed || self.lease.is_none() || self.failed {
            return Err(SpoolError::AlreadyClosed);
        }
        if self.summary.truncated {
            self.summary.record_chunk(seen, 0)?;
            return Ok(true);
        }
        let per_stream_full = self.summary.retained_bytes >= self.limit;
        let session_full = if self.prepaid.is_some() {
            self.prepaid_remaining == 0
        } else {
            self.quota
                .try_lock()
                .is_ok_and(|quota| quota.used() >= quota.limit())
        };
        if !per_stream_full && !session_full {
            return Ok(false);
        }
        self.summary.record_chunk(seen, 0)?;
        Ok(true)
    }

    pub fn observe_after_failure(&mut self, seen: u64) -> Result<(), SpoolError> {
        if !self.failed || self.closed || self.lease.is_none() {
            return Err(SpoolError::AlreadyClosed);
        }
        self.summary.record_chunk(seen, 0)?;
        Ok(())
    }

    pub fn append(&mut self, chunk: &[u8]) -> Result<usize, SpoolError> {
        if self.closed || self.lease.is_none() {
            return Err(SpoolError::AlreadyClosed);
        }
        if self.failed {
            return Err(SpoolError::WriteFailed);
        }
        let incoming = chunk.len() as u64;
        if self.summary.truncated {
            self.summary.record_chunk(incoming, 0)?;
            return Ok(0);
        }
        if self.summary.total_bytes.checked_add(incoming).is_none() {
            self.failed = true;
            return Err(SpoolError::Overflow);
        }
        let available = self.limit.saturating_sub(self.summary.retained_bytes);
        let mut dynamic = None;
        let retained = if self.prepaid.is_some() {
            self.prepaid_remaining.min(incoming.min(available)) as usize
        } else {
            let reservation = Reservation::new(&self.quota, incoming.min(available))?;
            let retained = reservation.accepted() as usize;
            dynamic = Some(reservation);
            retained
        };
        if retained != 0 {
            if self.bytes.try_reserve(retained).is_err() {
                self.failed = true;
                self.summary.record_chunk(incoming, 0)?;
                return Err(SpoolError::QuotaExhausted);
            }
            self.bytes.extend_from_slice(&chunk[..retained]);
        }
        self.summary.record_chunk(incoming, retained as u64)?;
        if self.prepaid.is_some() {
            self.prepaid_remaining = self
                .prepaid_remaining
                .checked_sub(retained as u64)
                .ok_or(SpoolError::Overflow)?;
        } else if let Some(mut reservation) = dynamic {
            self.retained.add(retained as u64);
            reservation.transfer(retained as u64);
        }
        Ok(retained)
    }

    pub fn mark_writer_failed(&mut self) {
        self.failed = true;
    }

    pub fn finish_writer(&mut self) -> Result<(), SpoolError> {
        if self.closed {
            return Err(SpoolError::AlreadyClosed);
        }
        self.closed = true;
        drop(self.lease.take());
        if self.prepaid_remaining > 0 {
            let unused = self.prepaid_remaining;
            self.prepaid
                .as_mut()
                .ok_or(SpoolError::Overflow)?
                .release(unused)?;
            self.prepaid_remaining = 0;
        }
        if self.failed {
            return Err(SpoolError::WriteFailed);
        }
        Ok(())
    }

    pub fn summary(&self) -> StreamSummary {
        self.summary
    }

    pub fn retained_bytes(&self) -> u64 {
        self.summary.retained_bytes
    }

    pub fn read_retained_page(&mut self, offset: u64, limit: usize) -> Result<Vec<u8>, SpoolError> {
        if self.failed {
            return Err(SpoolError::WriteFailed);
        }
        if limit == 0 || limit > MAX_SPOOL_PAGE_BYTES || offset > self.summary.retained_bytes {
            return Err(SpoolError::BadPage);
        }
        let start = usize::try_from(offset).map_err(|_| SpoolError::BadPage)?;
        let count = (self.summary.retained_bytes - offset).min(limit as u64) as usize;
        let end = start.checked_add(count).ok_or(SpoolError::Overflow)?;
        Ok(self.bytes[start..end].to_vec())
    }

    pub fn compact_retained(&mut self, keep: u64) -> Result<u64, SpoolError> {
        if !self.closed || self.lease.is_some() || !self.gate.is_sealed() || !self.gate.is_idle() {
            return Err(SpoolError::NotQuiesced);
        }
        if self.failed || keep > self.summary.retained_bytes {
            return Err(if self.failed {
                SpoolError::WriteFailed
            } else {
                SpoolError::BadPage
            });
        }
        let released = self.summary.retained_bytes - keep;
        let keep = usize::try_from(keep).map_err(|_| SpoolError::Overflow)?;
        self.bytes.truncate(keep);
        self.summary.compact_retained(keep as u64)?;
        if let Some(prepaid) = self.prepaid.as_mut() {
            prepaid.release(released)?;
        } else {
            self.retained.release(released)?;
        }
        Ok(released)
    }
}

/// Host supplies an empty, readable/writable, singly linked regular-file
/// capability. The supplied file cannot be retargeted by path replacement.
pub struct BoundedFileSpool {
    file: File,
    quota: Arc<Mutex<SessionQuota>>,
    retained: RetainedCharge,
    prepaid: Option<RetainedCharge>,
    prepaid_remaining: u64,
    limit: u64,
    gate: Arc<SpoolWriters>,
    lease: Option<SpoolWriterLease>,
    summary: StreamSummary,
    failed: bool,
    closed: bool,
    delivery_prefix: u64,
}
impl BoundedFileSpool {
    pub fn new(
        file: File,
        quota: Arc<Mutex<SessionQuota>>,
        gate: Arc<SpoolWriters>,
    ) -> Result<Self, SpoolError> {
        Self::new_with_limit(file, quota, gate, MAX_SPOOL_PER_STREAM)
    }

    /// Create a streaming file-backed spool with a host-selected retained-prefix
    /// ceiling. The host may only narrow the shared per-stream safety maximum.
    pub fn new_with_limit(
        mut file: File,
        quota: Arc<Mutex<SessionQuota>>,
        gate: Arc<SpoolWriters>,
        limit: u64,
    ) -> Result<Self, SpoolError> {
        if limit == 0 || limit > MAX_SPOOL_PER_STREAM {
            return Err(SpoolError::QuotaExhausted);
        }
        let stat = file.metadata()?;
        if !stat.is_file() || stat.len() != 0 {
            return Err(SpoolError::NotEmptyOrRegular);
        }
        spool_file_identity(&file)?;
        // The host can supply an empty file whose inherited cursor is not
        // positioned at zero. Avoid sparse holes in the retained prefix.
        file.seek(SeekFrom::Start(0))?;
        let [lease] = gate.begin::<1>().ok_or(SpoolError::AlreadyClosed)?;
        Ok(Self {
            file,
            retained: RetainedCharge::new(quota.clone()),
            prepaid: None,
            prepaid_remaining: 0,
            limit,
            quota,
            gate,
            lease: Some(lease),
            summary: StreamSummary::default(),
            failed: false,
            closed: false,
            delivery_prefix: 0,
        })
    }

    /// Create a spool whose retained capacity is reserved in full before
    /// any producer bytes are accepted. This is for exact native results that
    /// must not become lossy after an effect starts. Streaming stdout/stderr
    /// should continue using `new` and may truncate under shared quota pressure.
    pub fn new_reserved(
        mut file: File,
        quota: Arc<Mutex<SessionQuota>>,
        gate: Arc<SpoolWriters>,
        capacity: u64,
    ) -> Result<Self, SpoolError> {
        if capacity == 0 || capacity > MAX_SPOOL_PER_STREAM {
            return Err(SpoolError::QuotaExhausted);
        }
        let stat = file.metadata()?;
        if !stat.is_file() || stat.len() != 0 {
            return Err(SpoolError::NotEmptyOrRegular);
        }
        spool_file_identity(&file)?;
        file.seek(SeekFrom::Start(0))?;
        let [lease] = gate.begin::<1>().ok_or(SpoolError::AlreadyClosed)?;
        let mut reservation = Reservation::new(&quota, capacity)?;
        if reservation.accepted() != capacity {
            return Err(SpoolError::QuotaExhausted);
        }
        let mut prepaid = RetainedCharge::new(quota.clone());
        prepaid.add(capacity);
        reservation.transfer(capacity);
        Ok(Self {
            file,
            quota,
            retained: RetainedCharge::new(prepaid.quota.clone()),
            prepaid: Some(prepaid),
            prepaid_remaining: capacity,
            limit: capacity,
            gate,
            lease: Some(lease),
            summary: StreamSummary::default(),
            failed: false,
            closed: false,
            delivery_prefix: 0,
        })
    }

    /// Restore a previously journaled, quiescent terminal spool from an
    /// already-opened host file. No path resolution, task ID or journal is
    /// created here; the host must prove exact identity and generation.
    pub fn restore_verified(
        mut file: File,
        quota: Arc<Mutex<SessionQuota>>,
        checkpoint: &crate::task_checkpoint::SpoolCheckpoint,
        task: &crate::task_fence::TaskIdentity,
        journal_sequence: u64,
        generation: u64,
    ) -> Result<Self, SpoolError> {
        if !checkpoint.disposition.terminal() {
            return Err(SpoolError::RecoveryRejected);
        }
        checkpoint
            .validate(&file, task, journal_sequence, generation)
            .map_err(|_| SpoolError::RecoveryRejected)?;
        let mut reservation = Reservation::new(&quota, checkpoint.retained_len)?;
        if reservation.accepted() != checkpoint.retained_len {
            return Err(SpoolError::QuotaExhausted);
        }
        // Revalidated, singly linked R/W file; no payload mutation.
        file.seek(SeekFrom::Start(0))?;
        let mut retained = RetainedCharge::new(quota.clone());
        retained.add(checkpoint.retained_len);
        reservation.transfer(checkpoint.retained_len);
        // There is no live writer after recovery. A new Process requires
        // an independent host journal intent, new generation and spool.
        let gate = Arc::new(SpoolWriters::default());
        gate.seal();
        Ok(Self {
            file,
            quota,
            retained,
            prepaid: None,
            prepaid_remaining: 0,
            limit: checkpoint
                .effective_retained_limit()
                .map_err(|_| SpoolError::RecoveryRejected)?,
            gate,
            lease: None,
            summary: checkpoint.stream,
            closed: true,
            failed: !checkpoint.output_ok,
            delivery_prefix: checkpoint.stream.delivered_high_water,
        })
    }
    /// Cheap no-I/O accounting once hard storage limits are exhausted.
    /// Async readers use try_lock and avoid dispatching a blocking worker
    /// for every later 4 KiB pipe chunk. The byte total remains exact.
    pub fn discard_if_full(&mut self, seen: u64) -> Result<bool, SpoolError> {
        if self.closed || self.lease.is_none() || self.failed {
            return Err(SpoolError::AlreadyClosed);
        }
        if self.summary.truncated {
            self.summary.record_chunk(seen, 0)?;
            return Ok(true);
        }
        let per_stream_full = self.summary.retained_bytes >= self.limit;
        let session_full = if self.prepaid.is_some() {
            self.prepaid_remaining == 0
        } else {
            self.quota
                .try_lock()
                .is_ok_and(|quota| quota.used() >= quota.limit())
        };
        if !per_stream_full && !session_full {
            return Ok(false);
        }
        self.summary.record_chunk(seen, 0)?;
        Ok(true)
    }

    /// Continue accounting bytes after a failed spool write while the
    /// upstream pipe is drained. A failed prefix can never resume writing.
    pub fn observe_after_failure(&mut self, seen: u64) -> Result<(), SpoolError> {
        if !self.failed || self.closed || self.lease.is_none() {
            return Err(SpoolError::AlreadyClosed);
        }
        self.summary.record_chunk(seen, 0)?;
        Ok(())
    }
    /// Count every source byte, even after a retention quota is hit.
    /// Producers must continue draining their pipe after this returns zero.
    pub fn append(&mut self, chunk: &[u8]) -> Result<usize, SpoolError> {
        if self.closed || self.lease.is_none() {
            return Err(SpoolError::AlreadyClosed);
        }
        if self.failed {
            return Err(SpoolError::WriteFailed);
        }
        let incoming = chunk.len() as u64;
        if self.summary.truncated {
            self.summary.record_chunk(incoming, 0)?;
            return Ok(0);
        }
        if self.summary.total_bytes.checked_add(incoming).is_none() {
            self.failed = true;
            return Err(SpoolError::Overflow);
        }
        let available = self.limit.saturating_sub(self.summary.retained_bytes);
        let prepaid_limit = self
            .prepaid
            .as_ref()
            .map(|_| self.prepaid_remaining.min(incoming.min(available)));
        let mut dynamic_reservation = None;
        let retained = if let Some(prepaid) = prepaid_limit {
            prepaid as usize
        } else {
            let reservation = match Reservation::new(&self.quota, incoming.min(available)) {
                Ok(reservation) => reservation,
                Err(error) => {
                    self.summary.record_chunk(incoming, 0)?;
                    self.failed = true;
                    return Err(error);
                }
            };
            let retained = reservation.accepted() as usize;
            dynamic_reservation = Some(reservation);
            retained
        };
        let mut written = 0usize;
        let mut failed = None;
        while written < retained {
            match self.file.write(&chunk[written..retained]) {
                Ok(0) => {
                    failed = Some(io::Error::from(io::ErrorKind::WriteZero));
                    break;
                }
                Ok(n) => {
                    written += n;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
        }
        if let Err(error) = self.summary.record_chunk(incoming, written as u64) {
            self.failed = true;
            return Err(error.into());
        }
        if self.prepaid.is_some() {
            self.prepaid_remaining = self
                .prepaid_remaining
                .checked_sub(written as u64)
                .ok_or(SpoolError::Overflow)?;
        } else if let Some(mut reservation) = dynamic_reservation {
            self.retained.add(written as u64);
            reservation.transfer(written as u64);
        }
        if let Some(error) = failed {
            self.failed = true;
            return Err(SpoolError::Io(error));
        }
        Ok(written)
    }
    /// Persisted output is unreliable; let Failed/Uncertain settle after
    /// the real writer has quiesced without claiming completed output.
    pub fn mark_writer_failed(&mut self) {
        self.failed = true;
    }
    /// Flush and release the writer lease before terminal publication.
    pub fn finish_writer(&mut self) -> Result<(), SpoolError> {
        if self.closed {
            return Err(SpoolError::AlreadyClosed);
        }
        self.closed = true;
        let synced = self.file.sync_data();
        drop(self.lease.take());
        if let Err(error) = synced {
            self.failed = true;
            return Err(SpoolError::Io(error));
        }
        if self.prepaid_remaining > 0 {
            let unused = self.prepaid_remaining;
            self.prepaid
                .as_mut()
                .ok_or(SpoolError::Overflow)?
                .release(unused)?;
            self.prepaid_remaining = 0;
        }
        Ok(())
    }
    fn ready(&self) -> Result<(), SpoolError> {
        if !self.writers_quiesced() {
            return Err(SpoolError::NotQuiesced);
        }
        if self.failed {
            return Err(SpoolError::WriteFailed);
        }
        Ok(())
    }
    pub fn summary(&self) -> StreamSummary {
        self.summary
    }
    pub fn retained_bytes(&self) -> u64 {
        self.summary.retained_bytes
    }
    pub(crate) fn retention_limit(&self) -> u64 {
        self.limit
    }
    /// Only the host snapshot/recovery module may inspect this pinned file;
    /// no pathname is reconstructed and no external mutation is inferred.
    pub(crate) fn file(&self) -> &File {
        &self.file
    }
    pub(crate) fn output_failed(&self) -> bool {
        self.failed
    }

    /// Terminal failure/uncertain settlement is independent of whether
    /// persisted output was successful. Completed requires both conditions.
    pub fn writers_quiesced(&self) -> bool {
        self.closed && self.lease.is_none() && self.gate.is_sealed() && self.gate.is_idle()
    }
    pub fn is_quiesced(&self) -> bool {
        self.ready().is_ok()
    }
    /// Pure raw-byte read of the currently retained prefix, including while a
    /// writer is live. The file cursor is restored before return so subsequent
    /// append calls cannot overwrite earlier bytes. This does not acknowledge or
    /// advance any delivery watermark.
    pub fn read_retained_page(&mut self, offset: u64, limit: usize) -> Result<Vec<u8>, SpoolError> {
        if self.failed {
            return Err(SpoolError::WriteFailed);
        }
        if limit == 0 || limit > MAX_SPOOL_PAGE_BYTES || offset > self.summary.retained_bytes {
            return Err(SpoolError::BadPage);
        }
        let count = (self.summary.retained_bytes - offset).min(limit as u64) as usize;
        let restore = self.file.stream_position()?;
        let mut page = vec![0u8; count];
        let read = self
            .file
            .seek(SeekFrom::Start(offset))
            .and_then(|_| self.file.read_exact(&mut page));
        let restored = self.file.seek(SeekFrom::Start(restore));
        if let Err(error) = read {
            self.failed = true;
            return Err(SpoolError::Io(error));
        }
        if let Err(error) = restored {
            self.failed = true;
            return Err(SpoolError::Io(error));
        }
        Ok(page)
    }

    /// Returns bytes only from a fully quiesced, immutable retained prefix.
    /// A verified page can advance only a contiguous delivered prefix.
    pub fn page(&mut self, offset: u64, limit: usize) -> Result<Vec<u8>, SpoolError> {
        self.ready()?;
        if limit == 0 || limit > MAX_SPOOL_PAGE_BYTES || offset > self.summary.retained_bytes {
            return Err(SpoolError::BadPage);
        }
        let page = self.read_retained_page(offset, limit)?;
        let count = page.len();
        if offset <= self.delivery_prefix {
            let delivered = self.delivery_prefix.max(offset + count as u64);
            // Historical ACK/delivery remains authoritative after a
            // compacted prefix becomes shorter; repeat pages still work.
            if delivered > self.summary.delivered_high_water {
                self.summary.delivered(delivered)?;
            }
            self.delivery_prefix = delivered;
        }
        Ok(page)
    }
    pub fn acknowledge(&mut self, through: u64) -> Result<bool, SpoolError> {
        self.ready()?;
        self.summary.acknowledge(through).map_err(Into::into)
    }
    /// Compact retained data only AFTER the writers are quiesced. ACK history
    /// remains factual even when the current readable prefix becomes shorter.
    pub fn compact_retained(&mut self, keep: u64) -> Result<u64, SpoolError> {
        self.ready()?;
        if keep > self.summary.retained_bytes {
            return Err(SpoolError::BadPage);
        }
        let released = self.summary.retained_bytes - keep;
        if let Err(error) = self.file.set_len(keep).and_then(|()| self.file.sync_data()) {
            // File may already be truncated when sync fails. Do not claim
            // that the old retained length is still readable or complete.
            self.failed = true;
            return Err(SpoolError::Io(error));
        }
        self.summary.compact_retained(keep)?;
        if let Some(prepaid) = self.prepaid.as_mut() {
            prepaid.release(released)?;
        } else {
            self.retained.release(released)?;
        }
        Ok(released)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::OpenOptions,
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };
    static NEXT: AtomicU64 = AtomicU64::new(1);
    fn file() -> (File, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "abird-spool-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        (file, path)
    }
    #[test]
    fn varied_chunks_and_quotas_preserve_exact_prefix_and_raii_invariants() {
        let mut seed = 0x8a5f_2730_984f_12bdu64;
        for trial in 0..128u64 {
            let next = |value: &mut u64| {
                *value = value.wrapping_mul(6364136223846793005).wrapping_add(1);
                *value
            };
            let limit = next(&mut seed) % 700;
            let quota = Arc::new(Mutex::new(SessionQuota::new(limit)));
            let gate = Arc::new(SpoolWriters::default());
            let (file, path) = file();
            let mut spool = BoundedFileSpool::new(file, quota.clone(), gate.clone()).unwrap();
            let mut expected = Vec::new();
            let mut total = 0u64;
            let mut truncated = false;
            for _ in 0..20 {
                let len = (next(&mut seed) % 100) as usize;
                let bytes = (0..len).map(|_| next(&mut seed) as u8).collect::<Vec<_>>();
                let kept = spool.append(&bytes).unwrap();
                let permitted = if truncated {
                    0
                } else {
                    (limit - expected.len() as u64).min(len as u64) as usize
                };
                assert_eq!(kept, permitted, "trial {trial}");
                expected.extend_from_slice(&bytes[..kept]);
                total += len as u64;
                truncated |= kept < len;
                let state = spool.summary();
                assert_eq!(state.total_bytes, total);
                assert_eq!(state.retained_bytes, expected.len() as u64);
                assert_eq!(state.truncated, truncated);
                assert_eq!(quota.lock().unwrap().used(), state.retained_bytes);
            }
            spool.finish_writer().unwrap();
            assert!(gate.seal_and_wait_idle(Duration::from_millis(100)));
            assert_eq!(spool.page(0, expected.len().max(1)).unwrap(), expected);
            drop(spool);
            assert_eq!(quota.lock().unwrap().used(), 0);
            let _ = std::fs::remove_file(path);
        }
    }
    #[test]
    fn memory_spool_dynamic_quota_truncates_but_keeps_draining_accounting() {
        let quota = Arc::new(Mutex::new(SessionQuota::new(5)));
        let gate = Arc::new(SpoolWriters::default());
        let mut spool = BoundedMemorySpool::new(quota.clone(), gate.clone()).unwrap();
        assert_eq!(spool.append(b"abc").unwrap(), 3);
        assert_eq!(spool.append(b"defg").unwrap(), 2);
        assert!(spool.summary().truncated);
        assert_eq!(spool.summary().total_bytes, 7);
        assert_eq!(spool.summary().retained_bytes, 5);
        assert_eq!(spool.append(b"hij").unwrap(), 0);
        assert_eq!(spool.summary().total_bytes, 10);
        spool.finish_writer().unwrap();
        gate.seal();
        assert_eq!(spool.read_retained_page(0, 5).unwrap(), b"abcde");
        assert_eq!(spool.compact_retained(0).unwrap(), 5);
        assert_eq!(quota.lock().unwrap().used(), 0);
    }

    #[test]
    fn memory_spool_prepaid_exact_capacity_pages_and_releases_without_files() {
        let quota = Arc::new(Mutex::new(SessionQuota::new(16)));
        let gate = Arc::new(SpoolWriters::default());
        let mut spool = BoundedMemorySpool::new_reserved(quota.clone(), gate.clone(), 10).unwrap();
        assert_eq!(quota.lock().unwrap().used(), 10);
        assert_eq!(spool.append(b"abcd").unwrap(), 4);
        assert_eq!(spool.read_retained_page(1, 2).unwrap(), b"bc");
        spool.finish_writer().unwrap();
        gate.seal();
        assert_eq!(quota.lock().unwrap().used(), 4);
        assert_eq!(spool.compact_retained(0).unwrap(), 4);
        assert_eq!(quota.lock().unwrap().used(), 0);
    }

    #[test]
    fn memory_spool_over_capacity_is_detectably_truncated() {
        let quota = Arc::new(Mutex::new(SessionQuota::new(4)));
        let gate = Arc::new(SpoolWriters::default());
        let mut spool = BoundedMemorySpool::new_reserved(quota.clone(), gate.clone(), 4).unwrap();
        assert_eq!(spool.append(b"abcdef").unwrap(), 4);
        assert!(spool.summary().truncated);
        assert_eq!(spool.summary().total_bytes, 6);
        assert_eq!(spool.summary().retained_bytes, 4);
    }

    #[test]
    fn prepaid_exact_capacity_is_reserved_before_write_and_unused_bytes_release_on_finish() {
        let quota = Arc::new(Mutex::new(SessionQuota::new(16)));
        let gate = Arc::new(SpoolWriters::default());
        let (file, path) = file();
        let mut spool =
            BoundedFileSpool::new_reserved(file, quota.clone(), gate.clone(), 10).unwrap();
        assert_eq!(quota.lock().unwrap().used(), 10);
        assert_eq!(spool.append(b"abcd").unwrap(), 4);
        assert_eq!(quota.lock().unwrap().used(), 10);
        spool.finish_writer().unwrap();
        gate.seal();
        assert!(gate.is_idle());
        assert_eq!(quota.lock().unwrap().used(), 4);
        assert_eq!(spool.page(0, 4).unwrap(), b"abcd");
        assert_eq!(spool.compact_retained(0).unwrap(), 4);
        assert_eq!(quota.lock().unwrap().used(), 0);
        drop(spool);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn prepaid_exact_capacity_fails_before_effect_when_full_reservation_is_unavailable() {
        let quota = Arc::new(Mutex::new(SessionQuota::new(5)));
        let gate = Arc::new(SpoolWriters::default());
        let (file, path) = file();
        assert!(matches!(
            BoundedFileSpool::new_reserved(file, quota.clone(), gate, 6),
            Err(SpoolError::QuotaExhausted)
        ));
        assert_eq!(quota.lock().unwrap().used(), 0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn live_retained_page_restores_append_cursor() {
        let quota = Arc::new(Mutex::new(SessionQuota::new(64)));
        let gate = Arc::new(SpoolWriters::default());
        let (file, path) = file();
        let mut spool = BoundedFileSpool::new(file, quota, gate.clone()).unwrap();
        spool.append(b"abcdef").unwrap();
        assert_eq!(spool.read_retained_page(1, 3).unwrap(), b"bcd");
        spool.append(b"XYZ").unwrap();
        spool.finish_writer().unwrap();
        assert!(gate.seal_and_wait_idle(Duration::from_millis(100)));
        assert_eq!(spool.page(0, 9).unwrap(), b"abcdefXYZ");
        drop(spool);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn uncommitted_reservation_releases_all_capacity_on_early_error() {
        let quota = Arc::new(Mutex::new(SessionQuota::new(20)));
        {
            let reserved = Reservation::new(&quota, 12).unwrap();
            assert_eq!(reserved.accepted(), 12);
            assert_eq!(quota.lock().unwrap().used(), 12);
            // No successful StreamSummary update => no transfer.
        }
        assert_eq!(quota.lock().unwrap().used(), 0);
        let mut retained = RetainedCharge::new(quota.clone());
        {
            let mut reserved = Reservation::new(&quota, 12).unwrap();
            retained.add(7);
            reserved.transfer(7);
        }
        assert_eq!(quota.lock().unwrap().used(), 7);
        drop(retained);
        assert_eq!(quota.lock().unwrap().used(), 0);
    }
    #[test]
    fn once_truncated_stream_never_reuses_freed_quota_for_later_bytes() {
        let quota = Arc::new(Mutex::new(SessionQuota::new(3)));
        let first_gate = Arc::new(SpoolWriters::default());
        let second_gate = Arc::new(SpoolWriters::default());
        let (fa, pa) = file();
        let (fb, pb) = file();
        let mut a = BoundedFileSpool::new(fa, quota.clone(), first_gate.clone()).unwrap();
        let mut b = BoundedFileSpool::new(fb, quota.clone(), second_gate.clone()).unwrap();
        assert_eq!(a.append(b"ABC").unwrap(), 3);
        assert_eq!(b.append(b"discarded").unwrap(), 0);
        a.finish_writer().unwrap();
        assert!(first_gate.seal_and_wait_idle(Duration::from_millis(100)));
        assert_eq!(a.compact_retained(0).unwrap(), 3);
        assert_eq!(quota.lock().unwrap().used(), 0);
        assert!(b.discard_if_full(6).unwrap());
        assert_eq!(b.append(b"XYZ").unwrap(), 0);
        assert_eq!(b.summary().retained_bytes, 0);
        assert_eq!(b.summary().total_bytes, 9 + 6 + 3);
        b.finish_writer().unwrap();
        assert!(second_gate.seal_and_wait_idle(Duration::from_millis(100)));
        assert_eq!(b.page(0, 10).unwrap(), b"");
        drop(a);
        drop(b);
        assert_eq!(quota.lock().unwrap().used(), 0);
        for path in [pa, pb] {
            let _ = std::fs::remove_file(path);
        }
    }
    #[test]
    fn drop_releases_retained_quota_once_even_when_not_explicitly_compacted() {
        let quota = Arc::new(Mutex::new(SessionQuota::new(5)));
        let gate = Arc::new(SpoolWriters::default());
        let (file, path) = file();
        let mut spool = BoundedFileSpool::new(file, quota.clone(), gate).unwrap();
        assert_eq!(spool.append(b"abcde").unwrap(), 5);
        assert_eq!(quota.lock().unwrap().used(), 5);
        drop(spool);
        assert_eq!(quota.lock().unwrap().used(), 0);
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn quota_poisoning_cannot_report_successful_durable_output() {
        let quota = Arc::new(Mutex::new(SessionQuota::new(32)));
        let poisoned = quota.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoned.lock().unwrap();
            panic!("test: poison accounting");
        })
        .join();
        let gate = Arc::new(SpoolWriters::default());
        let (file, path) = file();
        let mut spool = BoundedFileSpool::new(file, quota.clone(), gate.clone()).unwrap();
        assert!(matches!(spool.append(b"data"), Err(SpoolError::Poisoned)));
        assert_eq!(spool.summary().total_bytes, 4);
        assert_eq!(spool.summary().retained_bytes, 0);
        assert!(spool.summary().truncated);
        spool.observe_after_failure(7).unwrap();
        assert_eq!(spool.summary().total_bytes, 11);
        spool.finish_writer().unwrap();
        gate.seal();
        assert!(gate.is_idle());
        assert!(
            spool.writers_quiesced(),
            "failure must still permit settlement"
        );
        assert!(!spool.is_quiesced(), "poisoned quota cannot certify output");
        drop(spool);
        assert_eq!(quota.lock().unwrap_err().into_inner().used(), 0);
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn even_an_empty_file_with_a_nonzero_cursor_stages_at_offset_zero() {
        let quota = Arc::new(Mutex::new(SessionQuota::new(64)));
        let gate = Arc::new(SpoolWriters::default());
        let (mut file, path) = file();
        file.seek(SeekFrom::Start(256)).unwrap();
        let mut spool = BoundedFileSpool::new(file, quota, gate.clone()).unwrap();
        assert_eq!(spool.append(b"abc").unwrap(), 3);
        spool.finish_writer().unwrap();
        assert!(gate.seal_and_wait_idle(Duration::from_millis(100)));
        assert_eq!(spool.page(0, 8).unwrap(), b"abc");
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 3);
        drop(spool);
        let _ = std::fs::remove_file(path);
    }
    #[cfg(unix)]
    #[test]
    fn read_only_spool_is_refused_at_preflight_before_any_output() {
        let (file, path) = file();
        drop(file);
        let ro = std::fs::OpenOptions::new().read(true).open(&path).unwrap();
        let quota = Arc::new(Mutex::new(SessionQuota::new(10)));
        let gate = Arc::new(SpoolWriters::default());
        assert!(matches!(
            BoundedFileSpool::new(ro, quota, gate),
            Err(SpoolError::NotReadableWritable)
        ));
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn bounded_quota_and_byte_exact_pages_after_quiescence() {
        let quota = Arc::new(Mutex::new(SessionQuota::new(24)));
        let gate = Arc::new(SpoolWriters::default());
        let (f1, p1) = file();
        let (f2, p2) = file();
        let mut out = BoundedFileSpool::new(f1, quota.clone(), gate.clone()).unwrap();
        let mut err = BoundedFileSpool::new(f2, quota.clone(), gate.clone()).unwrap();
        assert_eq!(out.append(b"123456789012345").unwrap(), 15);
        assert_eq!(err.append(b"abcdefghijklmno").unwrap(), 9);
        assert_eq!(err.append(b"more").unwrap(), 0);
        assert_eq!(quota.lock().unwrap().used(), 24);
        assert!(out.page(0, 3).is_err());
        out.finish_writer().unwrap();
        err.finish_writer().unwrap();
        assert!(gate.seal_and_wait_idle(Duration::from_secs(1)));
        assert_eq!(out.page(0, 8).unwrap(), b"12345678");
        assert!(out.acknowledge(9).is_err());
        assert!(out.acknowledge(8).unwrap());
        assert_eq!(out.page(8, 20).unwrap(), b"9012345");
        assert!(out.acknowledge(15).unwrap());
        assert_eq!(err.page(0, 24).unwrap(), b"abcdefghi");
        assert!(err.summary().truncated);
        assert_eq!(err.summary().total_bytes, 19);
        assert_eq!(err.summary().retained_bytes, 9);
        assert_eq!(out.compact_retained(4).unwrap(), 11);
        assert_eq!(quota.lock().unwrap().used(), 13);
        assert!(out.page(5, 2).is_err());
        assert_eq!(out.summary().acknowledged, 15);
        assert_eq!(out.page(0, 4).unwrap(), b"1234");
        assert!(out.append(b"extra").is_err());
        let _ = std::fs::remove_file(p1);
        let _ = std::fs::remove_file(p2);
    }
    #[test]
    fn exhausted_quota_drains_without_new_file_writes() {
        let quota = Arc::new(Mutex::new(SessionQuota::new(2)));
        let gate = Arc::new(SpoolWriters::default());
        let (file, path) = file();
        let mut spool = BoundedFileSpool::new(file, quota.clone(), gate.clone()).unwrap();
        assert!(!spool.discard_if_full(5).unwrap());
        assert_eq!(spool.append(b"ABCDE").unwrap(), 2);
        assert!(spool.discard_if_full(10).unwrap());
        assert_eq!(spool.summary().total_bytes, 15);
        assert_eq!(spool.summary().retained_bytes, 2);
        assert!(spool.summary().truncated);
        spool.finish_writer().unwrap();
        assert!(gate.seal_and_wait_idle(Duration::from_secs(1)));
        assert_eq!(spool.page(0, 2).unwrap(), b"AB");
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn host_can_page_beyond_aw_product_default_within_shared_stream_bound() {
        let quota = Arc::new(Mutex::new(SessionQuota::new(256 * 1024)));
        let gate = Arc::new(SpoolWriters::default());
        let mut spool = BoundedMemorySpool::new(quota, gate).unwrap();
        let bytes = vec![b'x'; 128 * 1024];
        assert_eq!(spool.append(&bytes).unwrap(), bytes.len());
        spool.finish_writer().unwrap();
        let page = spool.read_retained_page(0, bytes.len()).unwrap();
        assert_eq!(page.len(), bytes.len());
        assert_eq!(page, bytes);
    }

    #[test]
    fn host_cannot_widen_the_absolute_session_spool_ceiling() {
        let quota = SessionQuota::new(u64::MAX);
        assert_eq!(quota.limit(), MAX_SESSION_SPOOL);
        assert!(quota.limit() > crate::task::DEFAULT_SESSION_SPOOL);
        assert_eq!(quota.used(), 0);
    }
    #[test]
    fn rejects_nonempty_existing_spool_and_sealed_registration() {
        let quota = Arc::new(Mutex::new(SessionQuota::new(1024)));
        let gate = Arc::new(SpoolWriters::default());
        let (mut f, path) = file();
        f.write_all(b"existing").unwrap();
        assert!(matches!(
            BoundedFileSpool::new(f, quota.clone(), gate.clone()),
            Err(SpoolError::NotEmptyOrRegular)
        ));
        let _ = std::fs::remove_file(path);
        let (f, path) = file();
        gate.seal();
        assert!(matches!(
            BoundedFileSpool::new(f, quota, gate),
            Err(SpoolError::AlreadyClosed)
        ));
        let _ = std::fs::remove_file(path);
    }
}
