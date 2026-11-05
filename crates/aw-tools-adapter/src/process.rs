//! Host-owned process I/O service: no model tool, sandbox or second Agent.
//! A trusted caller must authenticate grants and journal Process intent
//! before spawn. Pipes and optional PTYs share lifecycle ownership.
use aw_tools::{
    filesystem::{
        job_manager::OutputWriter,
        runner::{ProcessExit as RunnerExit, ProcessOutputMeta, ProcessTerminal, StopProof},
    },
    task::{SpoolWriterLease, SpoolWriters, StreamSummary, TaskDisposition},
    task_fence::{FenceError, TaskFence, TaskIdentity, TaskTransition},
    task_ops::{HostTaskOperations, Stream as TaskStream, TaskInfo},
    task_spool::{BoundedFileSpool, SessionQuota, SpoolError},
};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::File,
    path::PathBuf,
    process::{ExitStatus, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, Command},
    task::JoinHandle,
    time::timeout,
};

const CAPTURE_HEAD: usize = 48 * 1024;
const CAPTURE_TAIL: usize = 64 * 1024;
const MAX_STDIN_WRITE: usize = 64 * 1024;
const REAP_DEADLINE: Duration = Duration::from_secs(2);
const READER_DRAIN_DEADLINE: Duration = Duration::from_millis(600);
const GRACEFUL_TERM: Duration = Duration::from_millis(350);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessMode {
    Pipes,
    #[cfg(feature = "pty")]
    Pty,
}
#[derive(Clone, Debug)]
pub struct PipeSpec {
    /// Host-approved absolute executable, never selected from model args.
    pub program: PathBuf,
    /// Host-approved directory; NOT a security sandbox.
    pub root: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub stdin_open: bool,
}
impl PipeSpec {
    /// A trusted host/Agent Tool owns this, not a deserializable RPC payload.
    pub fn validate(&self) -> crate::Result<()> {
        if !self.program.is_absolute()
            || !self.program.is_file()
            || !self.root.is_dir()
            || self.args.len() > 32
        {
            return Err(crate::Error::Denied(
                "process requires approved absolute program, root and bounded arguments",
            ));
        }
        if self
            .args
            .iter()
            .any(|s| s.len() > 16_384 || s.contains('\0'))
        {
            return Err(crate::Error::Limit("process arguments too large"));
        }
        if self.env.len() > 16
            || self.env.iter().any(|(k, v)| {
                k.is_empty()
                    || k.len() > 128
                    || v.len() > 8192
                    || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    || v.contains('\0')
            })
        {
            return Err(crate::Error::Denied(
                "process environment must be host-authored and bounded",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamSnapshot {
    /// First 48 KiB retained for coding-tool results.
    pub head: Vec<u8>,
    /// Last 64 KiB retained independently for background inspection.
    pub tail: Vec<u8>,
    pub total_bytes: u64,
    pub truncated: bool,
    pub closed: bool,
    pub read_failed: bool,
}
#[derive(Default)]
struct CaptureState {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    total: u64,
    closed: bool,
    read_failed: bool,
}
impl CaptureState {
    fn append(&mut self, bytes: &[u8]) {
        self.total = self.total.saturating_add(bytes.len() as u64);
        let take = bytes
            .len()
            .min(CAPTURE_HEAD.saturating_sub(self.head.len()));
        self.head.extend_from_slice(&bytes[..take]);
        if bytes.len() >= CAPTURE_TAIL {
            self.tail.clear();
            self.tail
                .extend(bytes[bytes.len() - CAPTURE_TAIL..].iter().copied());
        } else {
            let excess = self
                .tail
                .len()
                .saturating_add(bytes.len())
                .saturating_sub(CAPTURE_TAIL);
            self.tail.drain(..excess);
            self.tail.extend(bytes.iter().copied());
        }
    }
    fn snapshot(&self) -> StreamSnapshot {
        StreamSnapshot {
            head: self.head.clone(),
            tail: self.tail.iter().copied().collect(),
            total_bytes: self.total,
            truncated: self.total > self.head.len() as u64,
            closed: self.closed,
            read_failed: self.read_failed,
        }
    }
}
/// The host opens uniquely-owned spool file capabilities before calling
/// spawn. Model arguments cannot select these paths or create task IDs.
pub struct HostSpoolFiles {
    pub stdout: File,
    pub stderr: File,
    pub session_quota: Arc<Mutex<SessionQuota>>,
}
#[derive(Clone)]
pub struct ProcessSpoolAccess {
    stdout: Arc<Mutex<BoundedFileSpool>>,
    stderr: Arc<Mutex<BoundedFileSpool>>,
}
impl ProcessSpoolAccess {
    fn stream(&self, kind: OutputStream) -> &Arc<Mutex<BoundedFileSpool>> {
        match kind {
            OutputStream::Stdout => &self.stdout,
            OutputStream::Stderr => &self.stderr,
        }
    }
    fn writers_finished(&self) -> Result<bool, SpoolError> {
        // Poisoned output state is a failure, not a reason to strand Task
        // Failed/Uncertain settlement. Only terminal status may recover it.
        let out = self.stdout.lock().unwrap_or_else(|e| e.into_inner());
        let err = self.stderr.lock().unwrap_or_else(|e| e.into_inner());
        Ok(out.writers_quiesced() && err.writers_quiesced())
    }
    fn output_ready(&self) -> Result<bool, SpoolError> {
        let out = self.stdout.lock().unwrap_or_else(|e| e.into_inner());
        let err = self.stderr.lock().unwrap_or_else(|e| e.into_inner());
        Ok(!self.stdout.is_poisoned()
            && !self.stderr.is_poisoned()
            && out.is_quiesced()
            && err.is_quiesced())
    }
    pub(crate) fn summary(&self, kind: OutputStream) -> Result<StreamSummary, SpoolError> {
        self.stream(kind)
            .lock()
            .map_err(|_| SpoolError::Poisoned)
            .map(|spool| spool.summary())
    }
    pub(crate) fn page(
        &self,
        kind: OutputStream,
        offset: u64,
        limit: usize,
    ) -> Result<Vec<u8>, SpoolError> {
        self.stream(kind)
            .lock()
            .map_err(|_| SpoolError::Poisoned)?
            .page(offset, limit)
    }
    pub fn acknowledge(&self, kind: OutputStream, through: u64) -> Result<bool, SpoolError> {
        self.stream(kind)
            .lock()
            .map_err(|_| SpoolError::Poisoned)?
            .acknowledge(through)
    }
}
/// Exact-source, generation-fenced host projection over one durable
/// Process receipt. It contains no task DB or model-generated task grants.
pub struct FencedProcessSpools {
    access: ProcessSpoolAccess,
    fence: Mutex<TaskFence>,
}
#[derive(Debug)]
pub enum FencedProcessError {
    Fence(FenceError),
    Spool(SpoolError),
    Poisoned,
    NotSettled,
    CancelUnavailable,
    SourceCheckPanicked,
    CancelOutcomeUnknown,
}
impl From<FenceError> for FencedProcessError {
    fn from(error: FenceError) -> Self {
        Self::Fence(error)
    }
}
impl From<SpoolError> for FencedProcessError {
    fn from(error: SpoolError) -> Self {
        Self::Spool(error)
    }
}
impl FencedProcessSpools {
    /// The host calls this only after its immutable ToolStarted journal
    /// entry is committed, with its own authenticated TaskIdentity.
    pub fn after_journal(
        identity: TaskIdentity,
        intent_sequence: u64,
        access: ProcessSpoolAccess,
    ) -> Result<Self, FenceError> {
        if identity.effect != aw_tools::ToolEffect::Process {
            return Err(FenceError::InvalidIdentity);
        }
        Ok(Self {
            access,
            fence: Mutex::new(TaskFence::after_journal(identity, intent_sequence)?),
        })
    }
    pub fn summary(
        &self,
        live: &TaskIdentity,
        expected: u64,
        source_live: bool,
        stream: OutputStream,
    ) -> Result<StreamSummary, FencedProcessError> {
        let guard = self
            .fence
            .lock()
            .map_err(|_| FencedProcessError::Poisoned)?;
        guard.verify(live, expected, source_live)?;
        self.access.summary(stream).map_err(Into::into)
    }
    pub fn generation(&self) -> Result<u64, FencedProcessError> {
        Ok(self
            .fence
            .lock()
            .map_err(|_| FencedProcessError::Poisoned)?
            .generation())
    }
    /// Persist transition through the original Session/TurnJournal CAS
    /// before changing the local generation. Never starts a second journal.
    pub fn transition_journaled(
        &self,
        live: &TaskIdentity,
        expected: u64,
        source_live: bool,
        state: TaskDisposition,
        recovery_proven: bool,
        persist: impl FnOnce(TaskTransition) -> Result<(), FenceError>,
    ) -> Result<u64, FencedProcessError> {
        let mut guard = self
            .fence
            .lock()
            .map_err(|_| FencedProcessError::Poisoned)?;
        guard.verify(live, expected, source_live)?;
        if state.terminal()
            && (!self.access.writers_finished()?
                || (state == TaskDisposition::Completed && !self.access.output_ready()?))
        {
            return Err(FencedProcessError::NotSettled);
        }
        guard
            .transition_journaled(live, expected, source_live, state, recovery_proven, persist)
            .map_err(Into::into)
    }
    pub fn page(
        &self,
        live: &TaskIdentity,
        expected: u64,
        source_live: bool,
        stream: OutputStream,
        offset: u64,
        limit: usize,
    ) -> Result<Vec<u8>, FencedProcessError> {
        let guard = self
            .fence
            .lock()
            .map_err(|_| FencedProcessError::Poisoned)?;
        guard.verify(live, expected, source_live)?;
        self.access.page(stream, offset, limit).map_err(Into::into)
    }
    pub fn acknowledge(
        &self,
        live: &TaskIdentity,
        expected: u64,
        source_live: bool,
        stream: OutputStream,
        through: u64,
    ) -> Result<bool, FencedProcessError> {
        let guard = self
            .fence
            .lock()
            .map_err(|_| FencedProcessError::Poisoned)?;
        guard.verify(live, expected, source_live)?;
        self.access.acknowledge(stream, through).map_err(Into::into)
    }
}

/// One host-verified Process projection over TaskOps. A model-visible task
/// ID never authenticates itself: the original AW host supplies live source
/// liveness on *each* get/page/ack/cancel operation.
pub struct BoundProcessTaskHost<'a> {
    process: &'a FencedProcessSpools,
    verify_source: &'a dyn Fn(&TaskIdentity) -> bool,
    cancel: &'a dyn Fn(&TaskInfo) -> Result<bool, FencedProcessError>,
}
impl<'a> BoundProcessTaskHost<'a> {
    fn live(&self, id: &TaskIdentity) -> Result<bool, FencedProcessError> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.verify_source)(id)))
            .map_err(|_| FencedProcessError::SourceCheckPanicked)
    }
    pub fn new(
        process: &'a FencedProcessSpools,
        verify_source: &'a dyn Fn(&TaskIdentity) -> bool,
        cancel: &'a dyn Fn(&TaskInfo) -> Result<bool, FencedProcessError>,
    ) -> Self {
        Self {
            process,
            verify_source,
            cancel,
        }
    }
}
fn output_stream(kind: TaskStream) -> OutputStream {
    match kind {
        TaskStream::Stdout => OutputStream::Stdout,
        TaskStream::Stderr => OutputStream::Stderr,
    }
}
impl HostTaskOperations for BoundProcessTaskHost<'_> {
    type Error = FencedProcessError;
    fn inspect(&self, binding: &TaskIdentity) -> Result<TaskInfo, Self::Error> {
        let live = self.live(binding)?;
        let fence = self
            .process
            .fence
            .lock()
            .map_err(|_| FencedProcessError::Poisoned)?;
        fence.verify(binding, fence.generation(), live)?;
        Ok(TaskInfo {
            binding: fence.binding().clone(),
            generation: fence.generation(),
            journal_sequence: fence.journal_intent_sequence(),
            state: fence.state(),
            source_live: live,
        })
    }
    fn list(&self, scope: &TaskIdentity, limit: usize) -> Result<Vec<TaskInfo>, Self::Error> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let binding = self
            .process
            .fence
            .lock()
            .map_err(|_| FencedProcessError::Poisoned)?
            .binding()
            .clone();
        let live = self.live(&binding)?;
        let fence = self
            .process
            .fence
            .lock()
            .map_err(|_| FencedProcessError::Poisoned)?;
        if fence.binding() != &binding {
            return Err(FencedProcessError::Fence(FenceError::WrongSourceOrScope));
        }
        if binding.actor != scope.actor
            || binding.source_kind != scope.source_kind
            || binding.source_id != scope.source_id
            || binding.source_epoch != scope.source_epoch
            || binding.execution_site != scope.execution_site
            || binding.session != scope.session
            || binding.branch != scope.branch
            || !live
        {
            return Ok(Vec::new());
        }
        Ok(vec![TaskInfo {
            binding: binding.clone(),
            generation: fence.generation(),
            journal_sequence: fence.journal_intent_sequence(),
            state: fence.state(),
            source_live: true,
        }])
    }
    fn output(
        &self,
        task: &TaskInfo,
        stream: TaskStream,
        offset: u64,
        limit: usize,
    ) -> Result<Vec<u8>, Self::Error> {
        self.process.page(
            &task.binding,
            task.generation,
            self.live(&task.binding)?,
            output_stream(stream),
            offset,
            limit,
        )
    }
    fn acknowledge(
        &self,
        task: &TaskInfo,
        stream: TaskStream,
        through: u64,
    ) -> Result<bool, Self::Error> {
        self.process.acknowledge(
            &task.binding,
            task.generation,
            self.live(&task.binding)?,
            output_stream(stream),
            through,
        )
    }
    fn request_cancel(&self, task: &TaskInfo) -> Result<bool, Self::Error> {
        {
            let live = self.live(&task.binding)?;
            let fence = self
                .process
                .fence
                .lock()
                .map_err(|_| FencedProcessError::Poisoned)?;
            fence.verify(&task.binding, task.generation, live)?;
            if fence.state().terminal() {
                return Err(FencedProcessError::CancelUnavailable);
            }
        }
        // Cancellation may journal a transition through the same Process
        // fence. Never call an external host callback with that lock held.
        // The host must recheck the exact generation/grant before OS effects.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.cancel)(task)))
            .unwrap_or(Err(FencedProcessError::CancelOutcomeUnknown))
    }
}

#[derive(Clone)]
enum RetainedSink {
    Legacy(Arc<Mutex<BoundedFileSpool>>),
    Job(OutputWriter),
}
impl RetainedSink {
    fn discard_if_full(&self, seen: u64) -> Result<bool, ()> {
        match self {
            Self::Legacy(spool) => spool
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .discard_if_full(seen)
                .map_err(|_| ()),
            Self::Job(writer) => writer.discard_if_full(seen).map_err(|_| ()),
        }
    }

    fn append(&self, bytes: &[u8]) -> Result<usize, ()> {
        match self {
            Self::Legacy(spool) => spool
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .append(bytes)
                .map_err(|_| ()),
            Self::Job(writer) => writer.append(bytes).map_err(|_| ()),
        }
    }

    fn observe_after_failure(&self, seen: u64) -> Result<(), ()> {
        match self {
            Self::Legacy(spool) => spool
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .observe_after_failure(seen)
                .map_err(|_| ()),
            Self::Job(writer) => writer.observe_after_failure(seen).map_err(|_| ()),
        }
    }

    fn mark_failed(&self) {
        match self {
            Self::Legacy(spool) => spool
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .mark_writer_failed(),
            Self::Job(writer) => {
                let _ = writer.mark_failed();
            }
        }
    }

    fn finish(&self) -> Result<(), ()> {
        match self {
            Self::Legacy(spool) => spool
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .finish_writer()
                .map_err(|_| ()),
            Self::Job(writer) => writer.finish().map_err(|_| ()),
        }
    }
}

#[derive(Clone)]
struct Reader {
    state: Arc<Mutex<CaptureState>>,
}
impl Reader {
    fn snapshot(&self) -> StreamSnapshot {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .snapshot()
    }
    fn start<R: AsyncRead + Unpin + Send + 'static>(
        input: R,
        writer_lease: SpoolWriterLease,
        spool: Option<RetainedSink>,
    ) -> (Self, JoinHandle<()>) {
        const PIPE_BUFFER: usize = 16 * 1024;
        const SPOOL_BATCH: usize = 64 * 1024;
        const SPOOL_QUEUE: usize = 4;
        let state = Arc::new(Mutex::new(CaptureState::default()));
        let shared = state.clone();
        let task = tokio::spawn(async move {
            let _lease = writer_lease;
            let mut input = input;
            let (mut sender, writer) = if let Some(spool) = spool {
                // One fixed blocking writer per stream, bounded in-flight
                // buffering; no per-chunk worker spawning.
                let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(SPOOL_QUEUE);
                let worker = tokio::task::spawn_blocking(move || -> Result<(), SpoolError> {
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut failed = false;
                        while let Some(chunk) = rx.blocking_recv() {
                            let result = if failed {
                                spool.observe_after_failure(chunk.len() as u64).map(|_| 0)
                            } else {
                                match spool.discard_if_full(chunk.len() as u64) {
                                    Ok(true) => Ok(0),
                                    Ok(false) => spool.append(&chunk),
                                    Err(()) => Err(()),
                                }
                            };
                            if result.is_err() {
                                failed = true;
                            }
                        }
                        failed
                    }));
                    // A panicking writer cannot leave its output writer open.
                    // Mark failure and finish regardless of producer outcome.
                    let failed = outcome.unwrap_or(true);
                    if failed {
                        spool.mark_failed();
                    }
                    let sync = spool.finish();
                    if failed {
                        return Err(SpoolError::WriteFailed);
                    }
                    sync.map_err(|_| SpoolError::WriteFailed)
                });
                (Some(tx), Some(worker))
            } else {
                (None, None)
            };
            // Foreground Process without durable spooling pays no extra
            // batch allocation; the queue exists only in the opt-in path.
            let mut batch = if sender.is_some() {
                Vec::with_capacity(SPOOL_BATCH)
            } else {
                Vec::new()
            };
            let mut buffer = [0u8; PIPE_BUFFER];
            let mut failed = loop {
                match input.read(&mut buffer).await {
                    Ok(0) => break false,
                    Ok(n) => {
                        shared
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .append(&buffer[..n]);
                        if let Some(ref tx) = sender {
                            batch.extend_from_slice(&buffer[..n]);
                            if batch.len() >= SPOOL_BATCH {
                                let full =
                                    std::mem::replace(&mut batch, Vec::with_capacity(SPOOL_BATCH));
                                if tx.send(full).await.is_err() {
                                    sender = None;
                                    break true;
                                }
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break true,
                }
            };
            if let Some(tx) = sender {
                if !batch.is_empty() && tx.send(batch).await.is_err() {
                    failed = true;
                }
            }
            if let Some(worker) = writer {
                if !matches!(worker.await, Ok(Ok(()))) {
                    failed = true;
                }
            }
            let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
            state.closed = true;
            state.read_failed = failed;
        });
        (Self { state }, task)
    }
}
/// A bounded, exact byte-range read from the existing host-owned capture.
/// Unlike a durable disk spool, the head/tail ring may contain an unavailable
/// middle range. No page is invented or silently rewound across that gap.
pub const MAX_PROCESS_PAGE_BYTES: usize = 48 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessOutputPage {
    pub offset: u64,
    pub next_offset: u64,
    pub total_bytes: u64,
    pub data: Vec<u8>,
    /// Unrecoverable captured-output gap, if the head and tail don't meet.
    pub missing_range: Option<(u64, u64)>,
    pub stream_closed: bool,
    pub read_failed: bool,
    pub at_eof: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputPageError {
    InvalidLimit,
    PastEnd,
    Gap { start: u64, end: u64 },
}
impl StreamSnapshot {
    fn retained_gap(&self) -> Option<(u64, u64)> {
        let head = self.head.len() as u64;
        let start = self.total_bytes.saturating_sub(self.tail.len() as u64);
        if head < start {
            Some((head, start))
        } else {
            None
        }
    }
    /// Returns only original bytes; reads never cross a missing middle.
    pub fn page(&self, offset: u64, limit: usize) -> Result<ProcessOutputPage, OutputPageError> {
        if limit == 0 || limit > MAX_PROCESS_PAGE_BYTES {
            return Err(OutputPageError::InvalidLimit);
        }
        if offset > self.total_bytes {
            return Err(OutputPageError::PastEnd);
        }
        let gap = self.retained_gap();
        if let Some((start, end)) = gap {
            if (start..end).contains(&offset) {
                return Err(OutputPageError::Gap { start, end });
            }
        }
        let target_end = self.total_bytes.min(offset.saturating_add(limit as u64));
        let end = gap.map_or(target_end, |(start, _)| {
            if offset < start {
                target_end.min(start)
            } else {
                target_end
            }
        });
        let mut data = Vec::with_capacity((end - offset) as usize);
        let head_end = self.head.len() as u64;
        if offset < head_end {
            let next = end.min(head_end);
            data.extend_from_slice(&self.head[offset as usize..next as usize]);
        }
        let suffix_start = self.total_bytes.saturating_sub(self.tail.len() as u64);
        let tail_from = offset.max(head_end).max(suffix_start);
        if tail_from < end {
            let begin = (tail_from - suffix_start) as usize;
            let len = (end - tail_from) as usize;
            data.extend_from_slice(&self.tail[begin..begin + len]);
        }
        debug_assert_eq!(data.len(), (end - offset) as usize);
        Ok(ProcessOutputPage {
            offset,
            next_offset: end,
            total_bytes: self.total_bytes,
            data,
            missing_range: gap,
            stream_closed: self.closed,
            read_failed: self.read_failed,
            at_eof: end == self.total_bytes,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessSnapshot {
    pub pid: u32,
    pub stdout: StreamSnapshot,
    pub stderr: StreamSnapshot,
}
impl ProcessSnapshot {
    pub fn output(&self, stream: OutputStream) -> &StreamSnapshot {
        match stream {
            OutputStream::Stdout => &self.stdout,
            OutputStream::Stderr => &self.stderr,
        }
    }
    pub fn page(
        &self,
        stream: OutputStream,
        offset: u64,
        limit: usize,
    ) -> Result<ProcessOutputPage, OutputPageError> {
        self.output(stream).page(offset, limit)
    }
}

/// Host-only cursored delivery/ACK projection. The acknowledged range is a
/// contiguous byte prefix actually returned by this cursor, never a guessed
/// high-water mark over an unavailable head/tail gap.
pub struct ProcessOutputCursor {
    stream: StreamSnapshot,
    delivered_prefix: u64,
    acknowledged: u64,
}
impl ProcessOutputCursor {
    pub fn new(snapshot: &ProcessSnapshot, stream: OutputStream) -> Self {
        Self {
            stream: snapshot.output(stream).clone(),
            delivered_prefix: 0,
            acknowledged: 0,
        }
    }
    pub fn page(
        &mut self,
        offset: u64,
        limit: usize,
    ) -> Result<ProcessOutputPage, OutputPageError> {
        let page = self.stream.page(offset, limit)?;
        if offset <= self.delivered_prefix {
            self.delivered_prefix = self.delivered_prefix.max(page.next_offset);
        }
        Ok(page)
    }
    pub fn acknowledge(&mut self, through: u64) -> Result<bool, OutputPageError> {
        if through > self.delivered_prefix {
            return Err(OutputPageError::Gap {
                start: self.delivered_prefix,
                end: through,
            });
        }
        if through <= self.acknowledged {
            return Ok(false);
        }
        self.acknowledged = through;
        Ok(true)
    }
    pub fn delivered_prefix(&self) -> u64 {
        self.delivered_prefix
    }
    pub fn acknowledged(&self) -> u64 {
        self.acknowledged
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Exited(i32),
    ExitedBySignal,
    Cancelled,
    DeadlineExceeded,
    Unknown,
}
pub struct ProcessReceipt {
    pub outcome: Outcome,
    pub output: ProcessSnapshot,
    pub output_complete: bool,
    /// Present only when the trusted host supplied durable output handles.
    /// Ownership is the original Process; no model-facing job ID exists.
    pub spools: Option<ProcessSpoolAccess>,
}
impl ProcessReceipt {
    pub fn finished(&self) -> bool {
        matches!(self.outcome, Outcome::Exited(_) | Outcome::ExitedBySignal) && self.output_complete
    }

    pub fn runner_terminal(&self) -> ProcessTerminal {
        let exit = match self.outcome {
            Outcome::Exited(code) => RunnerExit::Code(code),
            Outcome::ExitedBySignal => RunnerExit::Signal,
            Outcome::Cancelled => RunnerExit::Cancelled,
            Outcome::DeadlineExceeded => RunnerExit::DeadlineExceeded,
            Outcome::Unknown => RunnerExit::Unknown,
        };
        ProcessTerminal {
            exit,
            output: ProcessOutputMeta {
                stdout_bytes: self.output.stdout.total_bytes,
                stderr_bytes: self.output.stderr.total_bytes,
                stdout_truncated: self.output.stdout.truncated,
                stderr_truncated: self.output.stderr.truncated,
                complete: self.output_complete,
            },
        }
    }

    pub fn stop_proof(&self) -> StopProof {
        let terminal = self.runner_terminal();
        if terminal.exit == RunnerExit::Unknown || !terminal.output.complete {
            StopProof::Unknown(terminal.output)
        } else {
            StopProof::Confirmed(terminal)
        }
    }
}

enum SpawnOutputTarget {
    None,
    Legacy(HostSpoolFiles),
    Job {
        stdout: OutputWriter,
        stderr: OutputWriter,
    },
}

pub struct PipeProcess {
    child: Child,
    pid: u32,
    stdin: Option<ChildStdin>,
    stdout: Reader,
    stderr: Reader,
    readers: Vec<JoinHandle<()>>,
    /// Host-owned output lifetime; no independent job or task ledger.
    spool_writers: Arc<SpoolWriters>,
    spools: Option<ProcessSpoolAccess>,
    /// Fenced original OS termination attempt. A second attempt on this
    /// live host-owned PipeProcess is always refused, even after an unknown
    /// Process exit or a cancelled caller; never silently resend a signal.
    termination_attempted: bool,
    #[cfg(windows)]
    job: aw_platform::ManagedProcessGuard,
}
impl PipeProcess {
    pub async fn spawn(spec: PipeSpec) -> crate::Result<Self> {
        Self::spawn_inner(spec, SpawnOutputTarget::None).await
    }
    /// Opt-in host-authorized durable spooling. Host constructs handles and
    /// journals Process intent; no new background task or model grant.
    pub async fn spawn_with_host_spools(
        spec: PipeSpec,
        files: HostSpoolFiles,
    ) -> crate::Result<Self> {
        Self::spawn_inner(spec, SpawnOutputTarget::Legacy(files)).await
    }

    /// V2 Job/ShellNode path: stdout/stderr writers are already admitted by
    /// the shared JobManager before process launch. The same process reader
    /// loop feeds them; paging/ACK ownership stays in JobManager.
    pub async fn spawn_with_job_outputs(
        spec: PipeSpec,
        stdout: OutputWriter,
        stderr: OutputWriter,
    ) -> crate::Result<Self> {
        Self::spawn_inner(spec, SpawnOutputTarget::Job { stdout, stderr }).await
    }

    async fn spawn_inner(spec: PipeSpec, target: SpawnOutputTarget) -> crate::Result<Self> {
        spec.validate()?;
        let spool_writers = Arc::new(SpoolWriters::default());
        // Validate/reserve output capabilities before external spawn.
        let (spools, stdout_sink, stderr_sink) = match target {
            SpawnOutputTarget::Legacy(files) => {
                // Do not launch a child with an already-poisoned accounting
                // owner; a later poison becomes a durable output failure.
                {
                    let _quota_check = files.session_quota.lock().map_err(|_| {
                        crate::Error::Denied(
                            "host spool quota is poisoned; inspect before process launch",
                        )
                    })?;
                }
                // Shared descriptor identity is stable on Unix and Windows.
                // Verify link count and R/W access before spawning a child.
                let a = aw_tools::task_spool::spool_file_identity(&files.stdout)
                    .map_err(|_| crate::Error::Denied("invalid host stdout spool"))?;
                let b = aw_tools::task_spool::spool_file_identity(&files.stderr)
                    .map_err(|_| crate::Error::Denied("invalid host stderr spool"))?;
                if a == b {
                    return Err(crate::Error::Denied(
                        "stdout and stderr require distinct spool files",
                    ));
                }
                let out = BoundedFileSpool::new(
                    files.stdout,
                    files.session_quota.clone(),
                    spool_writers.clone(),
                )
                .map_err(|_| crate::Error::Denied("invalid host stdout spool"))?;
                let err =
                    BoundedFileSpool::new(files.stderr, files.session_quota, spool_writers.clone())
                        .map_err(|_| crate::Error::Denied("invalid host stderr spool"))?;
                let stdout = Arc::new(Mutex::new(out));
                let stderr = Arc::new(Mutex::new(err));
                let access = ProcessSpoolAccess {
                    stdout: stdout.clone(),
                    stderr: stderr.clone(),
                };
                (
                    Some(access),
                    Some(RetainedSink::Legacy(stdout)),
                    Some(RetainedSink::Legacy(stderr)),
                )
            }
            SpawnOutputTarget::Job { stdout, stderr } => (
                None,
                Some(RetainedSink::Job(stdout)),
                Some(RetainedSink::Job(stderr)),
            ),
            SpawnOutputTarget::None => (None, None, None),
        };
        // Only trusted host config sets these values. The existing Agent
        // journals and revalidates Process grants before calling this port.
        let mut cmd = Command::new(spec.program);
        cmd.args(spec.args)
            .current_dir(spec.root)
            .env_clear()
            .stdin(if spec.stdin_open {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (key, value) in spec.env {
            cmd.env(key, value);
        }
        #[cfg(unix)]
        aw_platform::configure_child_group(cmd.as_std_mut());
        #[cfg(windows)]
        aw_platform::configure_managed_process(cmd.as_std_mut());
        let mut child = cmd.spawn().map_err(crate::Error::Io)?;
        // From this point onward a real external process may already have run.
        // Any host setup failure is therefore ambiguous for arbitrary side
        // effects: kill best-effort, but never report an ordinary launch fail.
        let Some(pid) = child.id() else {
            let _ = child.start_kill();
            return Err(crate::Error::Transport(
                "process launched but PID/setup outcome is uncertain",
            ));
        };
        #[cfg(windows)]
        let mut job = {
            let Some(handle) = child.raw_handle() else {
                let _ = child.start_kill();
                return Err(crate::Error::Transport(
                    "process launched but Windows handle/setup outcome is uncertain",
                ));
            };
            match aw_platform::ManagedProcessGuard::attach(pid, handle) {
                Ok(job) => job,
                Err(_) => {
                    let _ = child.start_kill();
                    return Err(crate::Error::Transport(
                        "process launched but Windows Job attachment outcome is uncertain",
                    ));
                }
            }
        };
        let Some(stdout) = child.stdout.take() else {
            #[cfg(unix)]
            aw_platform::kill_process_group(pid);
            #[cfg(windows)]
            job.terminate();
            let _ = child.start_kill();
            return Err(crate::Error::Transport(
                "process launched but stdout setup outcome is uncertain",
            ));
        };
        let Some(stderr) = child.stderr.take() else {
            #[cfg(unix)]
            aw_platform::kill_process_group(pid);
            #[cfg(windows)]
            job.terminate();
            let _ = child.start_kill();
            return Err(crate::Error::Transport(
                "process launched but stderr setup outcome is uncertain",
            ));
        };
        let stdin = child.stdin.take();
        let Some([out_lease, err_lease]) = spool_writers.begin::<2>() else {
            #[cfg(unix)]
            aw_platform::kill_process_group(pid);
            #[cfg(windows)]
            job.terminate();
            let _ = child.start_kill();
            return Err(crate::Error::Transport(
                "process launched but output-reader setup outcome is uncertain",
            ));
        };
        let (out, out_task) = Reader::start(stdout, out_lease, stdout_sink);
        let (err, err_task) = Reader::start(stderr, err_lease, stderr_sink);
        Ok(Self {
            child,
            pid,
            stdin,
            stdout: out,
            stderr: err,
            readers: vec![out_task, err_task],
            spool_writers,
            spools,
            termination_attempted: false,
            #[cfg(windows)]
            job,
        })
    }
    pub fn mode(&self) -> ProcessMode {
        ProcessMode::Pipes
    }
    pub fn snapshot(&self) -> ProcessSnapshot {
        ProcessSnapshot {
            pid: self.pid,
            stdout: self.stdout.snapshot(),
            stderr: self.stderr.snapshot(),
        }
    }
    /// Stdin write is itself an external effect. Only the current owner,
    /// having journaled this separate operation, may call it; no replay.
    pub async fn write(&mut self, bytes: &[u8]) -> crate::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        if bytes.len() > MAX_STDIN_WRITE {
            return Err(crate::Error::Limit("process input too large"));
        }
        let stdin = self
            .stdin
            .as_mut()
            .ok_or(crate::Error::Denied("stdin not open"))?;
        timeout(Duration::from_secs(2), async {
            stdin.write_all(bytes).await?;
            stdin.flush().await
        })
        .await
        .map_err(|_| crate::Error::Timeout)??;
        Ok(())
    }
    /// Inert snapshot of the existing original PipeProcess input state.
    /// It is not a subprocess capability or a guarantee of peer acceptance.
    pub fn stdin_open(&self) -> bool {
        self.stdin.is_some()
    }
    pub fn close_stdin(&mut self) {
        self.stdin.take();
    }
    async fn drain_output(&mut self) -> bool {
        // No new reader may enter once terminal output settlement begins.
        // Await actual reader completion: cancellation of a future alone is
        // not evidence that its in-flight read/append stopped.
        self.spool_writers.seal();
        let mut complete = true;
        for task in &mut self.readers {
            match timeout(READER_DRAIN_DEADLINE, &mut *task).await {
                Ok(Ok(())) => {}
                _ => {
                    task.abort();
                    // Poll the abort to completion; if even cancellation
                    // cannot quiesce the reader, preserve an incomplete result.
                    let _ = timeout(READER_DRAIN_DEADLINE, &mut *task).await;
                    complete = false;
                }
            }
        }
        let s = self.snapshot();
        complete
            && self.spool_writers.is_idle()
            && s.stdout.closed
            && s.stderr.closed
            && !s.stdout.read_failed
            && !s.stderr.read_failed
    }
    fn kill_group(&mut self) {
        #[cfg(unix)]
        aw_platform::kill_process_group(self.pid);
        #[cfg(windows)]
        self.job.terminate();
    }
    async fn stop(&mut self) -> Outcome {
        self.termination_attempted = true;
        self.stdin.take();
        // Graceful signal only if actually supported. On unsupported Windows
        // backend, force stop without claiming graceful OS group termination.
        if aw_platform::terminate_process_group(self.pid).is_ok() {
            if let Ok(Ok(_)) = timeout(GRACEFUL_TERM, self.child.wait()).await {
                return Outcome::Cancelled;
            }
        }
        self.kill_group();
        let _ = self.child.start_kill();
        match timeout(REAP_DEADLINE, self.child.wait()).await {
            Ok(Ok(_)) => Outcome::Cancelled,
            _ => Outcome::Unknown,
        }
    }
    /// AW-P6-9D2B2B1: Only the already-authorized owning HOST may expose
    /// this to an individually grant-fenced and journaled model Tool. The
    /// original PipeProcess performs the same bounded stop/group cleanup as
    /// the original timeout/cancellation path. A subsequent attempt is denied
    /// even if a previous OS signal or output drain returned Unknown.
    /// This does NOT allow a model to choose a PID/program/process group.
    pub fn termination_available(&mut self) -> crate::Result<bool> {
        if self.termination_attempted {
            return Ok(false);
        }
        Ok(self.child.try_wait().map_err(crate::Error::Io)?.is_none())
    }
    pub async fn terminate(&mut self) -> crate::Result<ProcessReceipt> {
        if !self.termination_available()? {
            return Err(crate::Error::Conflict(
                "Process already exited or had a termination attempt; inspect original receipt",
            ));
        }
        // stop() fences the attempt before the first possible OS signal.
        let outcome = self.stop().await;
        let output_complete = self.drain_output().await;
        if !output_complete {
            // Same original descendant cleanup as wait(); never assert that
            // a single exited direct child proves its group has stopped.
            self.kill_group();
        }
        Ok(ProcessReceipt {
            outcome,
            output: self.snapshot(),
            output_complete,
            spools: self.spools.clone(),
        })
    }
    /// Caller retains the resource across model steps if needed. Output drains
    /// while no one polls the process. Unknown effects never auto-replay.
    pub async fn wait<C: core::future::Future<Output = ()>>(
        mut self,
        cancellation: C,
        deadline: Duration,
    ) -> ProcessReceipt {
        let outcome = tokio::select! {
            biased;
            _=cancellation=>self.stop().await,
            result=timeout(deadline,self.child.wait())=>match result {
                Ok(Ok(status))=>{Self::status_outcome(status)}
                Ok(Err(_))=>{let _=self.stop().await;Outcome::Unknown}
                Err(_)=>{if self.stop().await==Outcome::Unknown {Outcome::Unknown}
                        else {Outcome::DeadlineExceeded}}
            },
        };
        let output_complete = self.drain_output().await;
        if !output_complete {
            // A descendant may still hold an inherited pipe after its parent
            // has exited. Do not detach that group's possible external effects.
            self.kill_group();
        }
        let output = self.snapshot();
        ProcessReceipt {
            outcome,
            output,
            output_complete,
            spools: self.spools.clone(),
        }
    }
    fn status_outcome(status: ExitStatus) -> Outcome {
        status
            .code()
            .map_or(Outcome::ExitedBySignal, Outcome::Exited)
    }
}
impl Drop for PipeProcess {
    fn drop(&mut self) {
        self.spool_writers.seal();
        for task in &self.readers {
            task.abort();
        }
        // Even after the direct child exited, inherited descendants may
        // remain in its process group with stdout/stderr redirected. Preserve
        // the old Bash guard's unconditional group teardown. Escaped children
        // and same-UID code remain outside this best-effort boundary.
        self.kill_group();
    }
}
#[cfg(feature = "pty")]
pub struct PtyProcess {
    terminal: aw_terminal::Terminal,
}
#[cfg(feature = "pty")]
impl PtyProcess {
    pub fn spawn(
        options: aw_terminal::Options,
        notify: impl Fn() + Send + Sync + 'static,
    ) -> crate::Result<Self> {
        Ok(Self {
            terminal: aw_terminal::Terminal::spawn(options, notify)
                .map_err(|_| crate::Error::Transport("approved PTY spawn failed"))?,
        })
    }
    pub fn mode(&self) -> ProcessMode {
        ProcessMode::Pty
    }
    pub fn status(&self) -> aw_terminal::Status {
        self.terminal.status()
    }
    pub fn snapshot(&self, scrollback: usize) -> aw_terminal::Snapshot {
        self.terminal.snapshot(scrollback)
    }
    pub fn input(&self, bytes: Vec<u8>) -> crate::Result<()> {
        self.terminal
            .input(bytes)
            .map_err(|_| crate::Error::Denied("PTY input unavailable"))
    }
    pub fn resize(&self, rows: u16, cols: u16) -> crate::Result<()> {
        self.terminal
            .resize(rows, cols)
            .map_err(|_| crate::Error::Denied("PTY resize unavailable"))
    }
    pub fn terminate(&self) {
        self.terminal.terminate()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::future::pending;
    fn bash() -> PathBuf {
        std::env::var_os("AW_BASH")
            .map(PathBuf::from)
            .or_else(|| {
                ["/bin/bash", "/usr/bin/bash"]
                    .iter()
                    .map(PathBuf::from)
                    .find(|p| p.is_file())
            })
            .expect("set AW_BASH to absolute Bash for process tests")
    }
    fn spec(command: &str) -> PipeSpec {
        PipeSpec {
            program: bash(),
            root: std::env::temp_dir(),
            args: vec![
                "--noprofile".into(),
                "--norc".into(),
                "-c".into(),
                command.into(),
            ],
            env: BTreeMap::from([
                ("LANG".into(), "C.UTF-8".into()),
                (
                    "PATH".into(),
                    std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
                ),
            ]),
            stdin_open: false,
        }
    }
    #[tokio::test]
    async fn aw_p6_9d2b2b1_terminate_original_host_process_once_no_second_signal() {
        let mut process = PipeProcess::spawn(PipeSpec {
            stdin_open: true,
            ..spec("cat >/dev/null")
        })
        .await
        .unwrap();
        assert!(process.termination_available().unwrap());
        let receipt = process.terminate().await.unwrap();
        assert_eq!(receipt.outcome, Outcome::Cancelled);
        assert!(receipt.output_complete);
        assert!(!process.stdin_open());
        assert!(
            !process.termination_available().unwrap(),
            "one original OS Process incarnation permits only one termination attempt"
        );
        assert!(
            process.terminate().await.is_err(),
            "post-settlement original Process must never receive a second signal"
        );
    }
    fn host_spool_file(label: &str) -> (File, std::path::PathBuf) {
        use std::{
            fs::OpenOptions,
            time::{SystemTime, UNIX_EPOCH},
        };
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "aw-host-spool-{}-{unique}-{label}",
            std::process::id()
        ));
        let handle = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        (handle, path)
    }
    #[tokio::test]
    async fn shared_job_outputs_use_same_pipe_reader_and_process_classifier() {
        use aw_tools::filesystem::{
            batch::{Graph, Status},
            footprint::Footprint,
            job::NodeState,
            job_manager::{
                plan_graph, JobManager, Limits, NodePolicy, OutputStream as JobStream,
                RuntimeAdmission, SemanticBudget,
            },
            status::Primary,
        };
        let graph = Graph::compile(
            &serde_json::json!({"ops":[["read",{"path":"a"}]]}),
            |_, _| true,
        )
        .unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1024,
                    result_bytes: 8192,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        let manager = JobManager::new(Limits {
            max_retained_jobs: 4,
            max_exposed_jobs: 4,
            max_hidden_jobs: 4,
            max_active_nodes: 2,
            max_hidden_replays: 4,
            max_spool_bytes: 64 * 1024,
        })
        .unwrap();
        manager
            .admit_runtime(
                graph,
                plans,
                RuntimeAdmission {
                    id: "process-job",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager
            .claim("process-job", 1, true, 2, |_, _| Ok(()))
            .unwrap();
        let owner = manager.job_output_owner("process-job").unwrap();
        let (stdout_file, stdout_path) = host_spool_file("job-out");
        let (stderr_file, stderr_path) = host_spool_file("job-err");
        manager
            .attach_output("process-job", 1, JobStream::Stdout, stdout_file, [owner])
            .unwrap();
        manager
            .attach_output("process-job", 1, JobStream::Stderr, stderr_file, [owner])
            .unwrap();
        let stdout = manager
            .output_writer("process-job", 1, JobStream::Stdout)
            .unwrap();
        let stderr = manager
            .output_writer("process-job", 1, JobStream::Stderr)
            .unwrap();
        manager
            .start(
                "process-job",
                aw_tools::filesystem::job_manager::StartRequest {
                    coordinate: 1,
                    claimed_sequence: 2,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 3,
                },
                |_, _| Ok(()),
            )
            .unwrap();
        let process = PipeProcess::spawn_with_job_outputs(
            spec("printf job-out; printf job-err >&2"),
            stdout,
            stderr,
        )
        .await
        .unwrap();
        let receipt = process.wait(pending::<()>(), Duration::from_secs(5)).await;
        let terminal = receipt.runner_terminal();
        assert_eq!(terminal.status().primary, Primary::Ok);
        let status = manager
            .settle_process(
                "process-job",
                1,
                NodeState::Running { sequence: 3 },
                terminal,
                4,
                |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(
            status,
            Status {
                primary: Primary::Ok,
                more: false,
                incomplete: false,
            }
        );
        assert_eq!(
            manager
                .output_page("process-job", 1, JobStream::Stdout, 0, 64)
                .unwrap()
                .bytes,
            b"job-out"
        );
        assert_eq!(
            manager
                .output_page("process-job", 1, JobStream::Stderr, 0, 64)
                .unwrap()
                .bytes,
            b"job-err"
        );
        let _ = std::fs::remove_file(stdout_path);
        let _ = std::fs::remove_file(stderr_path);
    }

    #[tokio::test]
    async fn host_opened_process_spools_retained_prefix_and_ack_are_exact() {
        let (stdout, out_path) = host_spool_file("out");
        let (stderr, err_path) = host_spool_file("err");
        let quota = Arc::new(Mutex::new(SessionQuota::new(65536)));
        let process = PipeProcess::spawn_with_host_spools(
            spec("head -c 90000 /dev/zero | tr '\\0' Z"),
            HostSpoolFiles {
                stdout,
                stderr,
                session_quota: quota.clone(),
            },
        )
        .await
        .unwrap();
        let receipt = process.wait(pending::<()>(), Duration::from_secs(5)).await;
        assert!(receipt.finished());
        let spools = receipt.spools.as_ref().expect("explicit host spool");
        let stdout = spools.summary(OutputStream::Stdout).unwrap();
        assert_eq!(stdout.total_bytes, 90_000);
        assert_eq!(stdout.retained_bytes, 65_536);
        assert!(stdout.truncated);
        assert_eq!(quota.lock().unwrap().used(), 65_536);
        let first = spools.page(OutputStream::Stdout, 0, 16_384).unwrap();
        assert_eq!(first, b"Z".repeat(16_384));
        assert!(spools.acknowledge(OutputStream::Stdout, 16_385).is_err());
        assert!(spools.acknowledge(OutputStream::Stdout, 16_384).unwrap());
        let mut next = 16_384u64;
        while next < 65_536 {
            let page = spools.page(OutputStream::Stdout, next, 16_384).unwrap();
            assert!(page.iter().all(|&b| b == b'Z'));
            next += page.len() as u64;
        }
        assert_eq!(next, 65_536);
        assert!(spools.acknowledge(OutputStream::Stdout, next).unwrap());
        assert_eq!(spools.page(OutputStream::Stdout, next, 100).unwrap(), b"");
        assert!(spools.page(OutputStream::Stdout, next + 1, 100).is_err());
        assert_eq!(spools.summary(OutputStream::Stderr).unwrap().total_bytes, 0);
        let _ = std::fs::remove_file(out_path);
        let _ = std::fs::remove_file(err_path);
    }
    fn task_binding() -> TaskIdentity {
        TaskIdentity {
            actor: "owner".into(),
            source_kind: "host".into(),
            source_id: "trusted".into(),
            source_epoch: 1,
            execution_site: "local".into(),
            session: "session-one".into(),
            branch: "main".into(),
            turn_id: 5,
            call_id: "call-a".into(),
            effect: aw_tools::ToolEffect::Process,
        }
    }
    #[tokio::test]
    async fn terminal_status_waits_for_real_spool_quiescence() {
        let (stdout, out_path) = host_spool_file("pending-out");
        let (stderr, err_path) = host_spool_file("pending-err");
        let process = PipeProcess::spawn_with_host_spools(
            spec("printf available; sleep 0.02"),
            HostSpoolFiles {
                stdout,
                stderr,
                session_quota: Arc::new(Mutex::new(SessionQuota::new(4096))),
            },
        )
        .await
        .unwrap();
        let access = process.spools.clone().unwrap();
        let fence = FencedProcessSpools::after_journal(task_binding(), 1, access).unwrap();
        assert_eq!(
            fence
                .transition_journaled(
                    &task_binding(),
                    1,
                    true,
                    TaskDisposition::Running,
                    false,
                    |_| Ok(())
                )
                .unwrap(),
            2
        );
        for state in [
            TaskDisposition::Completed,
            TaskDisposition::Failed,
            TaskDisposition::Uncertain,
        ] {
            assert!(matches!(
                fence.transition_journaled(&task_binding(), 2, true, state, false, |_| Ok(())),
                Err(FencedProcessError::NotSettled)
            ));
        }
        let receipt = process.wait(pending::<()>(), Duration::from_secs(3)).await;
        assert!(receipt.finished());
        assert_eq!(
            fence
                .transition_journaled(
                    &task_binding(),
                    2,
                    true,
                    TaskDisposition::Completed,
                    false,
                    |_| Ok(())
                )
                .unwrap(),
            3
        );
        let _ = std::fs::remove_file(out_path);
        let _ = std::fs::remove_file(err_path);
    }
    #[tokio::test]
    async fn panicking_host_callbacks_are_fail_closed_and_cancel_is_unknown() {
        use aw_tools::task_ops::{run, TaskAction, TaskInfo};
        let (stdout, out_path) = host_spool_file("panic-out");
        let (stderr, err_path) = host_spool_file("panic-err");
        let process = PipeProcess::spawn_with_host_spools(
            spec("printf ok"),
            HostSpoolFiles {
                stdout,
                stderr,
                session_quota: Arc::new(Mutex::new(SessionQuota::new(32))),
            },
        )
        .await
        .unwrap();
        let receipt = process.wait(pending::<()>(), Duration::from_secs(3)).await;
        assert!(receipt.finished());
        let fence = FencedProcessSpools::after_journal(task_binding(), 14, receipt.spools.unwrap())
            .unwrap();
        let panic_live = |_: &TaskIdentity| -> bool { panic!("host source check failed") };
        let none = |_: &TaskInfo| -> Result<bool, FencedProcessError> {
            panic!("must not issue cancellation without a live source")
        };
        let host = BoundProcessTaskHost::new(&fence, &panic_live, &none);
        assert!(matches!(
            run(&host, &task_binding(), 1, TaskAction::Inspect),
            Err(aw_tools::task_ops::TaskOpsError::Host(
                FencedProcessError::SourceCheckPanicked
            ))
        ));
        let live = |_: &TaskIdentity| true;
        let cancel = |_: &TaskInfo| -> Result<bool, FencedProcessError> {
            panic!("OS signal may already have been sent")
        };
        let host = BoundProcessTaskHost::new(&fence, &live, &cancel);
        assert!(matches!(
            run(&host, &task_binding(), 1, TaskAction::RequestCancel),
            Err(aw_tools::task_ops::TaskOpsError::Host(
                FencedProcessError::CancelOutcomeUnknown
            ))
        ));
        assert_eq!(fence.generation().unwrap(), 1);
        let _ = std::fs::remove_file(out_path);
        let _ = std::fs::remove_file(err_path);
    }
    #[tokio::test]
    async fn task_cancel_callback_may_reenter_its_fence_without_deadlock() {
        use aw_tools::task_ops::{run, TaskAction, TaskInfo};
        let (stdout, out_path) = host_spool_file("reenter-out");
        let (stderr, err_path) = host_spool_file("reenter-err");
        let process = PipeProcess::spawn_with_host_spools(
            spec("printf READY"),
            HostSpoolFiles {
                stdout,
                stderr,
                session_quota: Arc::new(Mutex::new(SessionQuota::new(64))),
            },
        )
        .await
        .unwrap();
        let receipt = process.wait(pending::<()>(), Duration::from_secs(3)).await;
        assert!(receipt.finished());
        let fence = FencedProcessSpools::after_journal(task_binding(), 13, receipt.spools.unwrap())
            .unwrap();
        let live = |_: &TaskIdentity| {
            assert_eq!(
                fence.generation().unwrap(),
                1,
                "source check must not run under Process fence"
            );
            true
        };
        let cancel = |_: &TaskInfo| {
            assert_eq!(fence.generation().unwrap(), 1);
            // The original Process host remains the only OS terminator.
            Err(FencedProcessError::CancelUnavailable)
        };
        let host = BoundProcessTaskHost::new(&fence, &live, &cancel);
        assert!(matches!(
            run(&host, &task_binding(), 1, TaskAction::Inspect),
            Ok(aw_tools::task_ops::TaskReply::Info(_))
        ));
        assert_eq!(
            aw_tools::task_ops::list_bound(&host, &task_binding(), 2)
                .unwrap()
                .len(),
            1
        );
        assert!(run(&host, &task_binding(), 1, TaskAction::RequestCancel).is_err());
        let _ = std::fs::remove_file(out_path);
        let _ = std::fs::remove_file(err_path);
    }
    #[tokio::test]
    async fn task_ops_bridge_rechecks_source_and_journals_before_generation() {
        use aw_tools::task_ops::{list_bound, run, Stream, TaskAction, TaskReply};
        let (stdout, out_path) = host_spool_file("ops-out");
        let (stderr, err_path) = host_spool_file("ops-err");
        let process = PipeProcess::spawn_with_host_spools(
            spec("printf hello"),
            HostSpoolFiles {
                stdout,
                stderr,
                session_quota: Arc::new(Mutex::new(SessionQuota::new(4096))),
            },
        )
        .await
        .unwrap();
        let receipt = process.wait(pending::<()>(), Duration::from_secs(3)).await;
        assert!(receipt.finished());
        let fenced =
            FencedProcessSpools::after_journal(task_binding(), 42, receipt.spools.unwrap())
                .unwrap();
        let live = std::sync::atomic::AtomicBool::new(true);
        let allowed = |_: &TaskIdentity| live.load(std::sync::atomic::Ordering::Acquire);
        let denied_cancel =
            |_: &aw_tools::task_ops::TaskInfo| Err(FencedProcessError::CancelUnavailable);
        let host = BoundProcessTaskHost::new(&fenced, &allowed, &denied_cancel);
        let id = task_binding();
        let reply = run(
            &host,
            &id,
            1,
            TaskAction::Output {
                stream: Stream::Stdout,
                offset: 0,
                limit: 16,
            },
        )
        .unwrap();
        assert!(matches!(reply,TaskReply::Bytes(bytes) if bytes==b"hello"));
        assert!(matches!(
            run(
                &host,
                &id,
                1,
                TaskAction::Acknowledge {
                    stream: Stream::Stdout,
                    through: 5
                }
            )
            .unwrap(),
            TaskReply::Acknowledged(true)
        ));
        assert_eq!(list_bound(&host, &id, 10).unwrap().len(), 1);
        assert!(run(&host, &id, 1, TaskAction::RequestCancel).is_err());
        assert!(matches!(
            fenced.transition_journaled(&id, 1, true, TaskDisposition::Running, false, |_| Err(
                FenceError::JournalRejectedNoEffect
            )),
            Err(FencedProcessError::Fence(
                FenceError::JournalRejectedNoEffect
            ))
        ));
        assert_eq!(fenced.generation().unwrap(), 1);
        assert_eq!(
            fenced
                .transition_journaled(&id, 1, true, TaskDisposition::Running, false, |proposal| {
                    assert_eq!(proposal.journal_intent_sequence, 42);
                    Ok(())
                })
                .unwrap(),
            2
        );
        assert!(run(&host, &id, 1, TaskAction::Inspect).is_err());
        live.store(false, std::sync::atomic::Ordering::Release);
        assert!(run(&host, &id, 2, TaskAction::Inspect).is_err());
        let _ = std::fs::remove_file(out_path);
        let _ = std::fs::remove_file(err_path);
    }
    #[tokio::test]
    async fn durable_spool_access_rechecks_exact_binding_and_generation() {
        let (stdout, out_path) = host_spool_file("fenced-out");
        let (stderr, err_path) = host_spool_file("fenced-err");
        let process = PipeProcess::spawn_with_host_spools(
            spec("printf READY"),
            HostSpoolFiles {
                stdout,
                stderr,
                session_quota: Arc::new(Mutex::new(SessionQuota::new(65536))),
            },
        )
        .await
        .unwrap();
        let receipt = process.wait(pending::<()>(), Duration::from_secs(4)).await;
        assert!(receipt.finished());
        let fenced =
            FencedProcessSpools::after_journal(task_binding(), 11, receipt.spools.unwrap())
                .unwrap();
        let mut wrong = task_binding();
        wrong.source_epoch = 2;
        assert!(matches!(
            fenced.page(&wrong, 1, true, OutputStream::Stdout, 0, 5),
            Err(FencedProcessError::Fence(FenceError::WrongSourceOrScope))
        ));
        assert!(matches!(
            fenced.page(&task_binding(), 1, false, OutputStream::Stdout, 0, 5),
            Err(FencedProcessError::Fence(FenceError::Revoked))
        ));
        assert_eq!(
            fenced
                .page(&task_binding(), 1, true, OutputStream::Stdout, 0, 5)
                .unwrap(),
            b"READY"
        );
        assert!(fenced
            .acknowledge(&task_binding(), 1, true, OutputStream::Stdout, 6)
            .is_err());
        assert!(fenced
            .acknowledge(&task_binding(), 1, true, OutputStream::Stdout, 5)
            .unwrap());
        assert_eq!(
            fenced
                .transition_journaled(
                    &task_binding(),
                    1,
                    true,
                    TaskDisposition::Running,
                    false,
                    |_| Ok(())
                )
                .unwrap(),
            2
        );
        assert!(matches!(
            fenced.page(&task_binding(), 1, true, OutputStream::Stdout, 0, 1),
            Err(FencedProcessError::Fence(FenceError::StaleGeneration))
        ));
        assert_eq!(
            fenced
                .transition_journaled(
                    &task_binding(),
                    2,
                    true,
                    TaskDisposition::Completed,
                    false,
                    |_| Ok(())
                )
                .unwrap(),
            3
        );
        assert_eq!(fenced.generation().unwrap(), 3);
        let _ = std::fs::remove_file(out_path);
        let _ = std::fs::remove_file(err_path);
    }
    #[tokio::test]
    async fn poisoned_spool_mutex_does_not_strand_failure_settlement() {
        let (stdout, out_path) = host_spool_file("poisoned-out");
        let (stderr, err_path) = host_spool_file("poisoned-err");
        let process = PipeProcess::spawn_with_host_spools(
            spec("printf data"),
            HostSpoolFiles {
                stdout,
                stderr,
                session_quota: Arc::new(Mutex::new(SessionQuota::new(4096))),
            },
        )
        .await
        .unwrap();
        let receipt = process.wait(pending::<()>(), Duration::from_secs(3)).await;
        assert!(receipt.finished());
        let access = receipt.spools.unwrap();
        let state = access.stdout.clone();
        let _ = std::thread::spawn(move || {
            let mut owner = state.lock().unwrap();
            owner.mark_writer_failed();
            panic!("test only: poison spool mutex");
        })
        .join();
        let fence = FencedProcessSpools::after_journal(task_binding(), 1, access).unwrap();
        assert_eq!(
            fence
                .transition_journaled(
                    &task_binding(),
                    1,
                    true,
                    TaskDisposition::Running,
                    false,
                    |_| Ok(())
                )
                .unwrap(),
            2
        );
        assert!(matches!(
            fence.transition_journaled(
                &task_binding(),
                2,
                true,
                TaskDisposition::Completed,
                false,
                |_| Ok(())
            ),
            Err(FencedProcessError::NotSettled)
        ));
        assert_eq!(
            fence
                .transition_journaled(
                    &task_binding(),
                    2,
                    true,
                    TaskDisposition::Failed,
                    false,
                    |_| Ok(())
                )
                .unwrap(),
            3
        );
        let _ = std::fs::remove_file(out_path);
        let _ = std::fs::remove_file(err_path);
    }
    #[tokio::test]
    async fn runtime_spool_failure_allows_failed_not_completed_task_settlement() {
        let (stdout, out_path) = host_spool_file("failure-out");
        let (stderr, err_path) = host_spool_file("failure-err");
        let quota = Arc::new(Mutex::new(SessionQuota::new(1024)));
        let process = PipeProcess::spawn_with_host_spools(
            spec("sleep 0.05; printf output"),
            HostSpoolFiles {
                stdout,
                stderr,
                session_quota: quota.clone(),
            },
        )
        .await
        .unwrap();
        let snapshot = process.spools.clone().unwrap();
        let after_spawn = quota.clone();
        let _ = std::thread::spawn(move || {
            let _guard = after_spawn.lock().unwrap();
            panic!("test only: corrupt quota accounting after spawn");
        })
        .join();
        let result = process.wait(pending::<()>(), Duration::from_secs(3)).await;
        assert!(!result.finished(), "failed disk accounting is incomplete");
        assert!(!result.output_complete);
        let fenced = FencedProcessSpools::after_journal(task_binding(), 11, snapshot).unwrap();
        assert_eq!(
            fenced
                .transition_journaled(
                    &task_binding(),
                    1,
                    true,
                    TaskDisposition::Running,
                    false,
                    |_| Ok(())
                )
                .unwrap(),
            2
        );
        assert!(matches!(
            fenced.transition_journaled(
                &task_binding(),
                2,
                true,
                TaskDisposition::Completed,
                false,
                |_| Ok(())
            ),
            Err(FencedProcessError::NotSettled)
        ));
        assert_eq!(
            fenced
                .transition_journaled(
                    &task_binding(),
                    2,
                    true,
                    TaskDisposition::Failed,
                    false,
                    |_| Ok(())
                )
                .unwrap(),
            3
        );
        let _ = std::fs::remove_file(out_path);
        let _ = std::fs::remove_file(err_path);
    }
    #[tokio::test]
    async fn separate_handles_to_the_same_spool_are_refused_before_spawn() {
        let (stdout, path) = host_spool_file("alias");
        let stderr = stdout.try_clone().unwrap();
        let outcome = PipeProcess::spawn_with_host_spools(
            spec("printf should-not-start"),
            HostSpoolFiles {
                stdout,
                stderr,
                session_quota: Arc::new(Mutex::new(SessionQuota::new(16))),
            },
        )
        .await;
        assert!(
            outcome.is_err(),
            "distinct streams require distinct file identities"
        );
        let _ = std::fs::remove_file(path);
    }
    #[tokio::test]
    async fn bounded_stream_pipeline_preserves_multi_megabyte_source_totals() {
        let (stdout, out_path) = host_spool_file("batched-out");
        let (stderr, err_path) = host_spool_file("batched-err");
        let process = PipeProcess::spawn_with_host_spools(
            spec("head -c 2097152 /dev/zero | tr '\\0' Z"),
            HostSpoolFiles {
                stdout,
                stderr,
                session_quota: Arc::new(Mutex::new(SessionQuota::new(65536))),
            },
        )
        .await
        .unwrap();
        let result = process.wait(pending::<()>(), Duration::from_secs(5)).await;
        assert!(
            result.finished(),
            "bounded queue must drain before terminal"
        );
        let summary = result
            .spools
            .as_ref()
            .unwrap()
            .summary(OutputStream::Stdout)
            .unwrap();
        assert_eq!(summary.total_bytes, 2097152);
        assert_eq!(summary.retained_bytes, 65536);
        assert!(summary.truncated);
        let out = result
            .spools
            .unwrap()
            .page(OutputStream::Stdout, 0, 4096)
            .unwrap();
        assert_eq!(out, b"Z".repeat(4096));
        let _ = std::fs::remove_file(out_path);
        let _ = std::fs::remove_file(err_path);
    }
    #[tokio::test]
    async fn invalid_host_spool_fails_before_launching_the_process() {
        use std::io::Write;
        let (mut stdout, out_path) = host_spool_file("invalid");
        stdout.write_all(b"not empty").unwrap();
        let (stderr, err_path) = host_spool_file("err");
        let marker =
            std::env::temp_dir().join(format!("aw-spool-preflight-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let mut command = spec("true");
        command.args[3] = format!("printf launched > {}", marker.display());
        let denied = PipeProcess::spawn_with_host_spools(
            command,
            HostSpoolFiles {
                stdout,
                stderr,
                session_quota: Arc::new(Mutex::new(SessionQuota::new(2048))),
            },
        )
        .await;
        assert!(denied.is_err());
        assert!(
            !marker.exists(),
            "an invalid host spool cannot launch a child"
        );
        let _ = std::fs::remove_file(out_path);
        let _ = std::fs::remove_file(err_path);
    }
    #[tokio::test]
    async fn process_writer_leases_seal_and_quiesce_before_terminal_receipt() {
        let process = PipeProcess::spawn(spec("printf first; sleep 0.02; printf second >&2"))
            .await
            .unwrap();
        let writer_gate = process.spool_writers.clone();
        assert!(!writer_gate.is_sealed());
        assert!(!writer_gate.is_idle());
        let receipt = process.wait(pending::<()>(), Duration::from_secs(3)).await;
        assert!(receipt.finished());
        assert!(writer_gate.is_sealed());
        assert!(writer_gate.is_idle());
        assert!(
            writer_gate.begin::<1>().is_none(),
            "late writer cannot publish into a terminal Process snapshot"
        );
    }
    #[tokio::test]
    async fn dropped_process_seals_registration_without_claiming_terminal_output() {
        let process = PipeProcess::spawn(spec("sleep 2")).await.unwrap();
        let writer_gate = process.spool_writers.clone();
        drop(process);
        assert!(writer_gate.is_sealed());
        assert!(writer_gate.begin::<1>().is_none());
    }
    #[tokio::test]
    async fn process_spawns_drains_and_receipts_exit() {
        let p = PipeProcess::spawn(spec("printf hello; sleep 0.05; printf tail >&2"))
            .await
            .unwrap();
        let receipt = p.wait(pending::<()>(), Duration::from_secs(4)).await;
        assert_eq!(receipt.outcome, Outcome::Exited(0));
        assert!(receipt.output_complete);
        assert_eq!(&receipt.output.stdout.head, b"hello");
        assert_eq!(&receipt.output.stderr.head, b"tail");
        assert!(receipt.finished());
    }
    #[tokio::test]
    async fn background_output_drains_without_waiting() {
        let p = PipeProcess::spawn(spec("printf first; sleep 0.05; printf second"))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(170)).await;
        assert_eq!(&p.snapshot().stdout.head, b"firstsecond");
        let receipt = p.wait(pending::<()>(), Duration::from_secs(3)).await;
        assert_eq!(&receipt.output.stdout.tail, b"firstsecond");
    }
    #[tokio::test]
    async fn nonzero_exit_is_factual() {
        let p = PipeProcess::spawn(spec("exit 9")).await.unwrap();
        let receipt = p.wait(pending::<()>(), Duration::from_secs(3)).await;
        assert_eq!(receipt.outcome, Outcome::Exited(9));
        assert!(receipt.finished());
    }
    #[tokio::test]
    async fn cancel_marks_interrupted_not_rolled_back() {
        let p = PipeProcess::spawn(spec("while :; do :; done"))
            .await
            .unwrap();
        let receipt = p
            .wait(
                async { tokio::time::sleep(Duration::from_millis(30)).await },
                Duration::from_secs(3),
            )
            .await;
        assert!(matches!(
            receipt.outcome,
            Outcome::Cancelled | Outcome::Unknown
        ));
        assert!(!receipt.finished());
    }
    #[tokio::test]
    async fn deadline_does_not_report_success_or_replay() {
        let resource = PipeProcess::spawn(spec("while :; do :; done"))
            .await
            .unwrap();
        let receipt = resource
            .wait(pending::<()>(), Duration::from_millis(35))
            .await;
        assert!(matches!(
            receipt.outcome,
            Outcome::DeadlineExceeded | Outcome::Unknown
        ));
        assert!(!receipt.finished());
    }
    #[tokio::test]
    async fn existing_process_capture_pages_exact_overlapping_head_and_tail() {
        let p = PipeProcess::spawn(spec("head -c 90000 /dev/zero | tr '\\0' Z"))
            .await
            .unwrap();
        let receipt = p.wait(pending::<()>(), Duration::from_secs(5)).await;
        assert!(receipt.finished());
        let mut cursor = ProcessOutputCursor::new(&receipt.output, OutputStream::Stdout);
        let mut pos = 0u64;
        let mut total = 0;
        while pos < 90_000 {
            let page = cursor.page(pos, MAX_PROCESS_PAGE_BYTES).unwrap();
            assert!(page.data.iter().all(|&b| b == b'Z'));
            assert_eq!(page.offset, pos);
            pos = page.next_offset;
            total += page.data.len();
        }
        assert_eq!(total, 90_000);
        assert!(cursor.acknowledge(90_000).unwrap());
        assert!(!cursor.acknowledge(89_000).unwrap());
        assert!(cursor.page(90_000, 10).unwrap().at_eof);
    }
    #[tokio::test]
    async fn capture_gap_is_not_fabricated_and_ack_cannot_skip_it() {
        let p = PipeProcess::spawn(spec("head -c 300000 /dev/zero | tr '\\0' Q"))
            .await
            .unwrap();
        let receipt = p.wait(pending::<()>(), Duration::from_secs(5)).await;
        assert!(receipt.finished());
        let mut cursor = ProcessOutputCursor::new(&receipt.output, OutputStream::Stdout);
        let first = cursor.page(0, MAX_PROCESS_PAGE_BYTES).unwrap();
        assert_eq!(first.next_offset, CAPTURE_HEAD as u64);
        let missing_start = CAPTURE_HEAD as u64;
        let missing_end = 300_000 - CAPTURE_TAIL as u64;
        assert_eq!(first.missing_range, Some((missing_start, missing_end)));
        assert_eq!(
            cursor.page(missing_start, 10).unwrap_err(),
            OutputPageError::Gap {
                start: missing_start,
                end: missing_end
            }
        );
        let tail = cursor.page(missing_end, 32).unwrap();
        assert_eq!(tail.data, b"Q".repeat(32));
        assert_eq!(
            cursor.delivered_prefix(),
            missing_start,
            "a suffix page cannot certify unobserved middle bytes"
        );
        assert!(cursor.acknowledge(missing_start).unwrap());
        assert!(cursor.acknowledge(missing_end).is_err());
        assert!(cursor.page(0, MAX_PROCESS_PAGE_BYTES + 1).is_err());
        assert!(cursor.page(300_001, 1).is_err());
    }
    #[tokio::test]
    async fn bounded_head_and_tail_preserve_truncation() {
        let p = PipeProcess::spawn(spec("head -c 90000 /dev/zero | tr '\\0' Z"))
            .await
            .unwrap();
        let receipt = p.wait(pending::<()>(), Duration::from_secs(5)).await;
        assert_eq!(receipt.outcome, Outcome::Exited(0));
        assert!(receipt.output_complete);
        let s = receipt.output.stdout;
        assert_eq!(s.head.len(), CAPTURE_HEAD);
        assert_eq!(s.tail.len(), CAPTURE_TAIL);
        assert_eq!(s.total_bytes, 90000);
        assert!(s.truncated);
        assert_eq!(s.head[0], b'Z');
        assert_eq!(s.tail[CAPTURE_TAIL - 1], b'Z');
    }
    #[tokio::test]
    async fn stdin_is_explicit_bounded_and_closed_by_default() {
        let mut p = PipeProcess::spawn(spec("cat")).await.unwrap();
        assert!(p.write(b"not allowed").await.is_err());
        assert_eq!(
            p.wait(pending::<()>(), Duration::from_secs(3))
                .await
                .outcome,
            Outcome::Exited(0)
        );
        let mut s = spec("read -r line; printf '%s' \"$line\"");
        s.stdin_open = true;
        let mut p = PipeProcess::spawn(s).await.unwrap();
        p.write(b"hello\n").await.unwrap();
        p.close_stdin();
        let receipt = p.wait(pending::<()>(), Duration::from_secs(3)).await;
        assert_eq!(&receipt.output.stdout.head, b"hello");
        assert!(receipt.finished());
    }
    #[test]
    fn unsafe_model_program_root_and_environment_are_rejected() {
        let mut s = spec("true");
        s.program = PathBuf::from("relative/false");
        assert!(s.validate().is_err());
        s.program = bash();
        s.args = vec!["x".repeat(17000)];
        assert!(s.validate().is_err());
        s.args.clear();
        s.env.insert("WITH=BAD".into(), "value".into());
        assert!(s.validate().is_err());
    }
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn direct_parent_exit_still_cleans_group_descendants() {
        // A child may redirect both pipes and outlive Bash. Preserve the
        // previous unconditional GroupGuard behavior; no background orphan.
        let marker = std::env::temp_dir().join(format!(
            "aw-p6-orphan-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut launch =
            spec("(sleep 1; printf leaked > \"$MARKER\") >/dev/null 2>&1 & printf parent");
        launch
            .env
            .insert("MARKER".into(), marker.to_string_lossy().into_owned());
        let resource = PipeProcess::spawn(launch).await.unwrap();
        let receipt = resource.wait(pending::<()>(), Duration::from_secs(4)).await;
        assert_eq!(receipt.outcome, Outcome::Exited(0));
        assert!(receipt.output_complete);
        tokio::time::sleep(Duration::from_millis(1300)).await;
        assert!(
            !marker.exists(),
            "grandchild from ended Bash may not remain effectful"
        );
    }
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn open_inherited_output_pipe_is_explicitly_incomplete() {
        let resource = PipeProcess::spawn(spec("(sleep 3) & printf parent"))
            .await
            .unwrap();
        let receipt = resource.wait(pending::<()>(), Duration::from_secs(4)).await;
        assert_eq!(receipt.outcome, Outcome::Exited(0));
        assert!(!receipt.output_complete);
        assert!(!receipt.finished());
    }
    #[cfg(feature = "pty")]
    #[test]
    fn terminal_adapter_uses_existing_pty_driver() {
        let mut opts = aw_terminal::Options::bash(std::env::temp_dir(), bash(), "owner");
        opts.args = vec![
            "--noprofile".into(),
            "--norc".into(),
            "-c".into(),
            "printf AW-P6-PTY".into(),
        ];
        let p = PtyProcess::spawn(opts, || {}).unwrap();
        assert_eq!(p.mode(), ProcessMode::Pty);
        assert!(!p.snapshot(0).id.is_empty());
        p.terminate();
    }
}
