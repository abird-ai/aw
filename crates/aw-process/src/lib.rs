//! Agent-neutral owner for one asynchronous Process execution lifetime.
//!
//! Hosts prepare an authorized command and transfer it by value. This crate
//! finalizes containment/stdin/stdout/stderr, spawns exactly once, owns the
//! child + Process Scope, and settles wait/stop/reap/output into one
//! ProcessTerminal. Policy, durable output storage, product transport and
//! Agent/Job authority remain at the host edge.

use aw_tools::{
    execution_stop::{
        ExecutionEventSink, ExecutionLifecycleEvent, StopEvidence, StopReason, StopRecord,
        StopTracker,
    },
    filesystem::runner::{Occupancy, ProcessExit, ProcessOutputMeta, ProcessTerminal},
};
use std::{
    collections::VecDeque,
    future::Future,
    io,
    pin::Pin,
    process::{ExitStatus, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
use thiserror::Error;
#[cfg(target_os = "linux")]
mod linux_spawn;

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, Command},
    sync::{mpsc, oneshot, Notify},
    task::JoinHandle,
    time::{sleep_until, timeout, Instant},
};

pub const CAPTURE_HEAD: usize = 48 * 1024;
pub const CAPTURE_TAIL: usize = 64 * 1024;
pub const MAX_PROCESS_PAGE_BYTES: usize = 48 * 1024;
pub const MAX_STDIN_WRITE: usize = 64 * 1024;

const REAP_DEADLINE: Duration = Duration::from_secs(2);
const READER_DRAIN_DEADLINE: Duration = Duration::from_millis(600);
const GRACEFUL_TERM: Duration = Duration::from_millis(350);
#[cfg(windows)]
const WINDOWS_OCCUPANCY_SETTLE: Duration = Duration::from_secs(2);
#[cfg(windows)]
const WINDOWS_OCCUPANCY_POLL: Duration = Duration::from_millis(10);
const PROCESS_INPUT_QUEUE: usize = 16;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("process I/O: {0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    SetupUncertain(&'static str),
    #[error("{0}")]
    Limit(&'static str),
    #[error("{0}")]
    InputClosed(&'static str),
    #[error("{0}")]
    OwnerGone(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("required closed descendant containment is unavailable")]
    ContainmentUnavailable,
    #[error("process launch authority denied before spawn")]
    DeniedBeforeSpawn,
    #[error("process launch cancelled before spawn")]
    CancelledBeforeSpawn,
    #[error("process launch deadline expired before spawn")]
    TimedOutBeforeSpawn,
    #[error("process input deadline exceeded")]
    Timeout,
}

/// Host-owned retained-output adapter.
///
/// This deliberately models only the operations the pipe reader needs.
/// Implementations may retain, truncate or account bytes however they choose;
/// they must not block forever. A sink failure is preserved as incomplete
/// output while the runtime continues draining the OS pipe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SinkError;

pub trait OutputSink: Send + 'static {
    fn discard_if_full(&mut self, seen: u64) -> std::result::Result<bool, SinkError>;
    fn append(&mut self, bytes: &[u8]) -> std::result::Result<usize, SinkError>;
    fn observe_after_failure(&mut self, seen: u64) -> std::result::Result<(), SinkError>;
    fn mark_failed(&mut self);
    fn finish(&mut self) -> std::result::Result<(), SinkError>;
}

// Scope first: containment Drop precedes the child's kill-on-drop.
struct Spawned {
    platform: aw_platform::DirectScope,
    child: Child,
    pid: u32,
}

fn spawn_attached(command: &mut Command) -> Result<Spawned> {
    let mut child = command.spawn()?;
    let Some(pid) = child.id() else {
        let _ = child.start_kill();
        return Err(Error::SetupUncertain(
            "process launched but PID/setup outcome is uncertain",
        ));
    };
    #[cfg(unix)]
    let scope = aw_platform::DirectScope::attach(pid);
    #[cfg(windows)]
    let scope = {
        let Some(handle) = child.raw_handle() else {
            let _ = child.start_kill();
            return Err(Error::SetupUncertain(
                "process launched but Windows handle/setup outcome is uncertain",
            ));
        };
        aw_platform::DirectScope::attach(pid, handle)
    };
    let platform = match scope {
        Ok(scope) => scope,
        Err(_) => {
            let _ = child.start_kill();
            return Err(Error::SetupUncertain(
                "process launched but Process scope setup is uncertain",
            ));
        }
    };
    Ok(Spawned {
        platform,
        child,
        pid,
    })
}

type LaunchFence = Box<dyn FnOnce(SpawnOnce<'_>) + Send + 'static>;

/// Trusted synchronous authority boundary on the actual spawn lane.
/// Consuming this token is the only way to authorize the original spawn.
/// The runtime retains its result, including OS failure; returning without
/// consumption denies launch. No second executor or result can be supplied.
pub struct SpawnOnce<'a> {
    command: &'a mut Command,
    check: &'a mut dyn FnMut() -> Result<()>,
    outcome: &'a mut Option<Result<Spawned>>,
}

impl SpawnOnce<'_> {
    /// Insert a trusted provider-reserved environment value without
    /// overwriting an existing command or ambient value. This does not
    /// expose command, stdio, containment or a second spawn surface.
    pub fn insert_reserved_env(&mut self, name: &str, value: &str) -> Result<()> {
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            || value.contains('\0')
            || std::env::var_os(name).is_some()
            || self
                .command
                .as_std()
                .get_envs()
                .any(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case(name))
        {
            return Err(Error::DeniedBeforeSpawn);
        }
        self.command.env(name, value);
        Ok(())
    }

    pub fn spawn(self) {
        *self.outcome = Some((self.check)().and_then(|()| spawn_attached(self.command)));
    }
}

fn spawn_fenced(
    command: &mut Command,
    fence: Option<LaunchFence>,
    mut check: impl FnMut() -> Result<()>,
) -> Result<Spawned> {
    match fence {
        None => spawn_attached(command),
        Some(fence) => {
            let mut outcome = None;
            fence(SpawnOnce {
                command,
                check: &mut check,
                outcome: &mut outcome,
            });
            outcome.unwrap_or(Err(Error::DeniedBeforeSpawn))
        }
    }
}

pub struct SpawnControl {
    deadline: Instant,
    cancelled: Box<dyn Fn() -> bool + Send + 'static>,
    cancelled_signal: Pin<Box<dyn Future<Output = ()> + Send + 'static>>,
    launch_fence: Option<LaunchFence>,
}

impl SpawnControl {
    pub fn new(
        deadline: Instant,
        cancelled: impl Fn() -> bool + Send + 'static,
        cancelled_signal: impl Future<Output = ()> + Send + 'static,
    ) -> Self {
        Self {
            deadline,
            cancelled: Box::new(cancelled),
            cancelled_signal: Box::pin(cancelled_signal),
            launch_fence: None,
        }
    }

    /// Hold synchronous authority across actual spawn, not across await.
    /// The bounded callback must consume the token only while authorized.
    /// A cancellation/deadline recheck runs immediately before OS spawn.
    pub fn new_fenced(
        deadline: Instant,
        cancelled: impl Fn() -> bool + Send + 'static,
        cancelled_signal: impl Future<Output = ()> + Send + 'static,
        fence: impl FnOnce(SpawnOnce<'_>) + Send + 'static,
    ) -> Self {
        let mut control = Self::new(deadline, cancelled, cancelled_signal);
        control.launch_fence = Some(Box::new(fence));
        control
    }

    #[cfg(not(target_os = "linux"))]
    fn launch(mut self, command: &mut Command) -> Result<Spawned> {
        self.check()?;
        let fence = self.launch_fence.take();
        spawn_fenced(command, fence, || self.check())
    }

    pub const fn deadline(&self) -> Instant {
        self.deadline
    }

    fn check(&self) -> Result<()> {
        if (self.cancelled)() {
            return Err(Error::CancelledBeforeSpawn);
        }
        if Instant::now() >= self.deadline {
            return Err(Error::TimedOutBeforeSpawn);
        }
        Ok(())
    }
}

/// Trusted minimum Process-scope guarantee, not a backend selector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContainmentRequirement {
    #[default]
    BestAvailable,
    ClosedDescendants,
}

impl ContainmentRequirement {
    /// Combine independently imposed minima without a downgrade.
    #[inline]
    pub const fn intersect(self, other: Self) -> Self {
        match (self, other) {
            (Self::BestAvailable, Self::BestAvailable) => Self::BestAvailable,
            _ => Self::ClosedDescendants,
        }
    }

    fn require_supported(self) -> Result<()> {
        // Current DirectScope: Unix process groups cannot exclude setsid;
        // Windows attaches a private Job before resuming the suspended child.
        // Selection/setup failure is never a reason to fall back.
        if self == Self::ClosedDescendants && !cfg!(windows) {
            return Err(Error::ContainmentUnavailable);
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct SpawnOptions {
    pub stdin_open: bool,
    pub stdout: Option<Box<dyn OutputSink>>,
    pub stderr: Option<Box<dyn OutputSink>>,
    pub event_sink: Option<Arc<dyn ExecutionEventSink>>,
    pub control: Option<SpawnControl>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamSnapshot {
    pub head: Vec<u8>,
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

#[derive(Clone)]
struct Reader {
    state: Arc<Mutex<CaptureState>>,
    stop: Arc<Notify>,
}

impl Reader {
    fn snapshot(&self) -> StreamSnapshot {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .snapshot()
    }

    fn stop(&self) {
        self.stop.notify_one();
    }

    fn start<R: AsyncRead + Unpin + Send + 'static>(
        input: R,
        sink: Option<Box<dyn OutputSink>>,
    ) -> (Self, JoinHandle<()>) {
        const PIPE_BUFFER: usize = 16 * 1024;
        const SINK_BATCH: usize = 64 * 1024;
        const SINK_QUEUE: usize = 4;

        let state = Arc::new(Mutex::new(CaptureState::default()));
        let shared = state.clone();
        let stop = Arc::new(Notify::new());
        let reader_stop = stop.clone();
        let task = tokio::spawn(async move {
            let mut input = input;
            let (mut sender, writer) = if let Some(mut sink) = sink {
                // One fixed blocking writer per stream and bounded in-flight
                // storage. The no-sink foreground path allocates no batches.
                let (tx, mut rx) = mpsc::channel::<Vec<u8>>(SINK_QUEUE);
                let worker = tokio::task::spawn_blocking(move || -> bool {
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut failed = false;
                        while let Some(chunk) = rx.blocking_recv() {
                            let result = if failed {
                                sink.observe_after_failure(chunk.len() as u64).map(|_| 0)
                            } else {
                                match sink.discard_if_full(chunk.len() as u64) {
                                    Ok(true) => Ok(0),
                                    Ok(false) => sink.append(&chunk),
                                    Err(error) => Err(error),
                                }
                            };
                            if result.is_err() {
                                failed = true;
                            }
                        }
                        failed
                    }));
                    let failed = outcome.unwrap_or(true);
                    if failed {
                        sink.mark_failed();
                    }
                    // Finish is mandatory even after a write/accounting
                    // failure: it releases the host sink's writer custody.
                    // Do not short-circuit this call on the failure flag.
                    let finish_failed = sink.finish().is_err();
                    failed || finish_failed
                });
                (Some(tx), Some(worker))
            } else {
                (None, None)
            };

            let mut batch = if sender.is_some() {
                Vec::with_capacity(SINK_BATCH)
            } else {
                Vec::new()
            };
            let mut buffer = [0u8; PIPE_BUFFER];
            let mut failed = loop {
                tokio::select! {
                    biased;
                    _ = reader_stop.notified() => break true,
                    read = input.read(&mut buffer) => match read {
                        Ok(0) => break false,
                        Ok(n) => {
                            shared
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .append(&buffer[..n]);
                            if let Some(ref tx) = sender {
                                batch.extend_from_slice(&buffer[..n]);
                                if batch.len() >= SINK_BATCH {
                                    let full = std::mem::replace(
                                        &mut batch,
                                        Vec::with_capacity(SINK_BATCH),
                                    );
                                    if tx.send(full).await.is_err() {
                                        sender = None;
                                        break true;
                                    }
                                }
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        Err(_) => break true,
                    }
                }
            };

            if let Some(tx) = sender {
                if !batch.is_empty() && tx.send(batch).await.is_err() {
                    failed = true;
                }
            }
            if let Some(writer) = writer {
                if !matches!(writer.await, Ok(false)) {
                    failed = true;
                }
            }

            let mut state = shared.lock().unwrap_or_else(|error| error.into_inner());
            state.closed = true;
            state.read_failed = failed;
        });

        (Self { state, stop }, task)
    }
}

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
        (head < start).then_some((head, start))
    }

    pub fn page(
        &self,
        offset: u64,
        limit: usize,
    ) -> std::result::Result<ProcessOutputPage, OutputPageError> {
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

        let max_end = offset.saturating_add(limit as u64).min(self.total_bytes);
        let end = match gap {
            Some((start, _)) if offset < start => max_end.min(start),
            _ => max_end,
        };
        let mut data = Vec::with_capacity((end - offset) as usize);

        let head_len = self.head.len() as u64;
        if offset < head_len {
            let head_end = end.min(head_len);
            data.extend_from_slice(&self.head[offset as usize..head_end as usize]);
        }
        if end > head_len {
            let tail_start = self.total_bytes.saturating_sub(self.tail.len() as u64);
            if offset < tail_start {
                if let Some((start, stop)) = gap {
                    return Err(OutputPageError::Gap {
                        start: start.max(offset),
                        end: stop.min(end),
                    });
                }
            }
            let begin = offset.max(tail_start).saturating_sub(tail_start) as usize;
            let len = end.saturating_sub(offset.max(tail_start)) as usize;
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
    ) -> std::result::Result<ProcessOutputPage, OutputPageError> {
        self.output(stream).page(offset, limit)
    }
}

#[derive(Debug)]
enum InputCommand {
    Write {
        bytes: Vec<u8>,
        reply: oneshot::Sender<Result<()>>,
    },
    Close {
        reply: oneshot::Sender<Result<()>>,
    },
}

#[derive(Clone)]
pub struct ProcessInput {
    tx: mpsc::Sender<InputCommand>,
}

impl ProcessInput {
    pub async fn write(&self, bytes: Vec<u8>) -> Result<()> {
        if bytes.len() > MAX_STDIN_WRITE {
            return Err(Error::Limit("process input exceeds 64KiB"));
        }
        let (reply, result) = oneshot::channel();
        self.tx
            .send(InputCommand::Write { bytes, reply })
            .await
            .map_err(|_| Error::InputClosed("process input is closed"))?;
        result
            .await
            .map_err(|_| Error::OwnerGone("process input owner disappeared"))?
    }

    pub async fn close(&self) -> Result<()> {
        let (reply, result) = oneshot::channel();
        self.tx
            .send(InputCommand::Close { reply })
            .await
            .map_err(|_| Error::InputClosed("process input is closed"))?;
        result
            .await
            .map_err(|_| Error::OwnerGone("process input owner disappeared"))?
    }
}

pub struct ProcessInputReceiver {
    rx: mpsc::Receiver<InputCommand>,
}

pub fn process_input_channel() -> (ProcessInput, ProcessInputReceiver) {
    let (tx, rx) = mpsc::channel(PROCESS_INPUT_QUEUE);
    (ProcessInput { tx }, ProcessInputReceiver { rx })
}

fn stop_evidence(result: io::Result<bool>) -> StopEvidence {
    match result {
        Ok(true) => StopEvidence::Stopped,
        Ok(false) => StopEvidence::NoTarget,
        Err(_) => StopEvidence::Unknown,
    }
}

fn occupancy(result: io::Result<bool>) -> Occupancy {
    match result {
        Ok(true) => Occupancy::Live,
        #[cfg(windows)]
        Ok(false) => Occupancy::Empty,
        // Direct Unix observes only the owned process group. A negative group
        // scan cannot prove the semantic Process scope empty because a child
        // may have escaped with setsid(2).
        #[cfg(unix)]
        Ok(false) => Occupancy::Unknown,
        Err(_) => Occupancy::Unknown,
    }
}

struct Scope {
    platform: aw_platform::DirectScope,
    child: Child,
    pid: u32,
}

impl Scope {
    fn new(platform: aw_platform::DirectScope, child: Child, pid: u32) -> Self {
        Self {
            platform,
            child,
            pid,
        }
    }

    fn pid(&self) -> u32 {
        self.pid
    }

    fn ready(&self) -> io::Result<bool> {
        self.platform.ready()
    }

    #[cfg(unix)]
    async fn wait_ready(&self) -> io::Result<()> {
        use tokio::signal::unix::{signal, SignalKind};

        let mut changes = signal(SignalKind::child())?;
        while !self.platform.ready()? {
            changes.recv().await.ok_or_else(|| {
                io::Error::new(io::ErrorKind::BrokenPipe, "SIGCHLD stream closed")
            })?;
        }
        Ok(())
    }

    fn observe(&self) -> Occupancy {
        occupancy(self.platform.observe())
    }

    fn graceful(&self) -> StopEvidence {
        stop_evidence(self.platform.graceful())
    }

    fn terminate(&mut self) -> StopEvidence {
        stop_evidence(self.platform.terminate())
    }

    fn kill_main(&mut self) -> StopEvidence {
        #[cfg(unix)]
        let result = self.child.start_kill();
        #[cfg(windows)]
        let result = self.platform.kill_leader();
        match result {
            Ok(()) => StopEvidence::Stopped,
            Err(_) => StopEvidence::Unknown,
        }
    }

    async fn reap(&mut self) -> io::Result<ExitStatus> {
        self.child.wait().await
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        let _ = self.terminate();
        self.kill_main();
    }
}

pub struct ProcessResult {
    pub terminal: ProcessTerminal,
    pub output: ProcessSnapshot,
}

/// Sole asynchronous owner of one Process/Job execution lifetime.
pub struct Process {
    scope: Scope,
    stdin: Option<ChildStdin>,
    stdout: Reader,
    stderr: Reader,
    readers: [JoinHandle<()>; 2],
    execution_deadline: Option<Instant>,
    known_exit: Option<ProcessExit>,
    known_occupancy: Option<Occupancy>,
    termination_attempted: bool,
    stop_state: StopTracker,
    event_sink: Option<Arc<dyn ExecutionEventSink>>,
}

impl Process {
    /// Finalize and spawn one already-authorized command.
    ///
    /// The caller must not preconfigure process containment or stdio ownership;
    /// those are lifetime-sensitive and are owned here.
    pub async fn spawn(command: Command, options: SpawnOptions) -> Result<Self> {
        Self::spawn_with_containment(command, options, ContainmentRequirement::BestAvailable).await
    }

    /// Finalize only a Scope satisfying the trusted minimum. Reject an
    /// unsupported requirement before entering any external launch fence,
    /// spawn lane, or command execution. No weaker retry.
    pub async fn spawn_with_containment(
        mut command: Command,
        options: SpawnOptions,
        requirement: ContainmentRequirement,
    ) -> Result<Self> {
        requirement.require_supported()?;
        let SpawnOptions {
            stdin_open,
            stdout,
            stderr,
            event_sink,
            control,
        } = options;
        let execution_deadline = control.as_ref().map(SpawnControl::deadline);

        command
            .stdin(if stdin_open {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        #[cfg(unix)]
        aw_platform::configure_child_group(command.as_std_mut());
        #[cfg(windows)]
        aw_platform::configure_managed_process(command.as_std_mut());

        #[cfg(target_os = "linux")]
        let spawned = linux_spawn::spawn(command, control).await?;
        #[cfg(not(target_os = "linux"))]
        let spawned = match control {
            Some(control) => control.launch(&mut command)?,
            None => spawn_attached(&mut command)?,
        };
        let Spawned {
            mut platform,
            mut child,
            pid,
        } = spawned;

        let Some(stdout_pipe) = child.stdout.take() else {
            let _ = platform.terminate();
            let _ = child.start_kill();
            return Err(Error::SetupUncertain(
                "process launched but stdout setup outcome is uncertain",
            ));
        };
        let Some(stderr_pipe) = child.stderr.take() else {
            let _ = platform.terminate();
            let _ = child.start_kill();
            return Err(Error::SetupUncertain(
                "process launched but stderr setup outcome is uncertain",
            ));
        };
        let stdin = child.stdin.take();

        let (stdout_reader, stdout_task) = Reader::start(stdout_pipe, stdout);
        let (stderr_reader, stderr_task) = Reader::start(stderr_pipe, stderr);

        Ok(Self {
            scope: Scope::new(platform, child, pid),
            stdin,
            stdout: stdout_reader,
            stderr: stderr_reader,
            readers: [stdout_task, stderr_task],
            execution_deadline,
            known_exit: None,
            known_occupancy: None,
            termination_attempted: false,
            stop_state: StopTracker::default(),
            event_sink,
        })
    }

    pub fn pid(&self) -> u32 {
        self.scope.pid()
    }

    pub fn snapshot(&self) -> ProcessSnapshot {
        ProcessSnapshot {
            pid: self.pid(),
            stdout: self.stdout.snapshot(),
            stderr: self.stderr.snapshot(),
        }
    }

    fn wait_deadline(&self, fallback: Duration) -> Instant {
        self.execution_deadline.unwrap_or_else(|| {
            Instant::now()
                .checked_add(fallback)
                .unwrap_or_else(Instant::now)
        })
    }

    pub async fn write(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        if bytes.len() > MAX_STDIN_WRITE {
            return Err(Error::Limit("process input too large"));
        }
        let stdin = self
            .stdin
            .as_mut()
            .ok_or(Error::InputClosed("stdin not open"))?;
        timeout(Duration::from_secs(2), async {
            stdin.write_all(bytes).await?;
            stdin.flush().await
        })
        .await
        .map_err(|_| Error::Timeout)??;
        Ok(())
    }

    pub fn stdin_open(&self) -> bool {
        self.stdin.is_some()
    }

    pub fn close_stdin(&mut self) {
        self.stdin.take();
    }

    pub fn termination_available(&self) -> Result<bool> {
        if self.termination_attempted {
            return Ok(false);
        }
        self.scope.ready().map(|ready| !ready).map_err(Error::Io)
    }

    pub async fn terminate(&mut self) -> Result<ProcessResult> {
        self.stop_with(StopReason::Cancelled).await
    }

    /// Host-requested semantic stop before wait ownership is consumed.
    /// The shared runtime remains the only owner that signals/reaps the Process.
    pub async fn stop_with(&mut self, reason: StopReason) -> Result<ProcessResult> {
        if !self.termination_available()? {
            return Err(Error::Conflict(
                "Process already exited or had a termination attempt; inspect original receipt",
            ));
        }
        let (stop, exit, occupancy) = self.stop(reason).await;
        Ok(self.finish(exit, Some(stop), occupancy).await)
    }

    pub async fn wait<C: core::future::Future<Output = ()>>(
        mut self,
        cancellation: C,
        deadline: Duration,
    ) -> ProcessResult {
        let absolute = self.wait_deadline(deadline);
        let timer = sleep_until(absolute);
        tokio::pin!(timer);
        let cancellation = cancellation;
        tokio::pin!(cancellation);
        let (exit, stop, occupancy) = tokio::select! {
            biased;
            _ = &mut cancellation => {
                let (record, exit, occupancy) = self.stop(StopReason::Cancelled).await;
                (exit, Some(record), occupancy)
            },
            _ = &mut timer => {
                let (record, exit, occupancy) = self.stop(StopReason::Timeout).await;
                (exit, Some(record), occupancy)
            },
            result = self.settle_natural_exit() => match result {
                Ok(settled) => settled,
                Err(_) => {
                    let (record, exit, occupancy) = self.stop(StopReason::RuntimeError).await;
                    (exit, Some(record), occupancy)
                }
            },
        };
        self.finish(exit, stop, occupancy).await
    }

    pub async fn wait_interactive<C: core::future::Future<Output = ()>>(
        mut self,
        cancellation: C,
        deadline: Duration,
        mut input: ProcessInputReceiver,
    ) -> ProcessResult {
        let cancellation = cancellation;
        tokio::pin!(cancellation);
        let deadline = sleep_until(self.wait_deadline(deadline));
        tokio::pin!(deadline);

        let (exit, stop, occupancy) = loop {
            tokio::select! {
                biased;
                _ = &mut cancellation => {
                    let (record, exit, occupancy) = self.stop(StopReason::Cancelled).await;
                    break (exit, Some(record), occupancy);
                }
                _ = &mut deadline => {
                    let (record, exit, occupancy) = self.stop(StopReason::Timeout).await;
                    break (exit, Some(record), occupancy);
                }
                command = input.rx.recv() => {
                    let Some(command) = command else {
                        self.close_stdin();
                        continue;
                    };
                    match command {
                        InputCommand::Write { bytes, reply } => {
                            enum WriteOutcome {
                                Complete(Result<()>),
                                Cancelled,
                                Timeout,
                            }
                            let outcome = {
                                let write = self.write(&bytes);
                                tokio::pin!(write);
                                tokio::select! {
                                    biased;
                                    _ = &mut cancellation => WriteOutcome::Cancelled,
                                    _ = &mut deadline => WriteOutcome::Timeout,
                                    result = &mut write => WriteOutcome::Complete(result),
                                }
                            };
                            match outcome {
                                WriteOutcome::Complete(result) => {
                                    let _ = reply.send(result);
                                }
                                WriteOutcome::Cancelled => {
                                    let _ = reply.send(Err(Error::InputClosed("process input is closed")));
                                    let (record, exit, occupancy) =
                                        self.stop(StopReason::Cancelled).await;
                                    break (exit, Some(record), occupancy);
                                }
                                WriteOutcome::Timeout => {
                                    let _ = reply.send(Err(Error::Timeout));
                                    let (record, exit, occupancy) =
                                        self.stop(StopReason::Timeout).await;
                                    break (exit, Some(record), occupancy);
                                }
                            }
                        }
                        InputCommand::Close { reply } => {
                            if self.stdin_open() {
                                self.close_stdin();
                                let _ = reply.send(Ok(()));
                            } else {
                                let _ = reply.send(Err(Error::InputClosed("process input is closed")));
                            }
                        }
                    }
                }
                result = self.settle_natural_exit() => {
                    break match result {
                        Ok(settled) => settled,
                        Err(_) => {
                            let (record, exit, occupancy) =
                                self.stop(StopReason::RuntimeError).await;
                            (exit, Some(record), occupancy)
                        }
                    };
                }
            }
        };

        input.rx.close();
        while let Ok(command) = input.rx.try_recv() {
            match command {
                InputCommand::Write { reply, .. } | InputCommand::Close { reply } => {
                    let _ = reply.send(Err(Error::InputClosed("process input is closed")));
                }
            }
        }
        self.close_stdin();
        self.finish(exit, stop, occupancy).await
    }

    async fn finish(
        &mut self,
        exit: ProcessExit,
        stop: Option<StopRecord>,
        occupancy: Occupancy,
    ) -> ProcessResult {
        let (stdout_complete, stderr_complete) = self.drain_output().await;
        let output = self.snapshot();
        ProcessResult {
            terminal: ProcessTerminal {
                exit,
                stop,
                occupancy,
                output: ProcessOutputMeta {
                    stdout_bytes: output.stdout.total_bytes,
                    stderr_bytes: output.stderr.total_bytes,
                    stdout_truncated: output.stdout.truncated,
                    stderr_truncated: output.stderr.truncated,
                    stdout_complete,
                    stderr_complete,
                },
            },
            output,
        }
    }

    async fn drain_output(&mut self) -> (bool, bool) {
        // One Process drain deadline covers both streams together. Sequential
        // per-stream deadlines would let two inherited FDs double the bounded
        // settlement window even though they belong to one Process lifetime.
        let drained = timeout(READER_DRAIN_DEADLINE, async {
            for task in &mut self.readers {
                if !task.is_finished() {
                    let _ = task.await;
                }
            }
        })
        .await
        .is_ok();
        if !drained {
            // The pipe is still open (commonly an inherited descendant FD).
            // Ask both readers to stop at the same Process boundary so each can
            // flush any bytes already captured in its partial sink batch.
            self.stdout.stop();
            self.stderr.stop();
            let quiesced = timeout(READER_DRAIN_DEADLINE, async {
                for task in &mut self.readers {
                    if !task.is_finished() {
                        let _ = task.await;
                    }
                }
            })
            .await
            .is_ok();
            if !quiesced {
                for task in &self.readers {
                    if !task.is_finished() {
                        task.abort();
                    }
                }
                // Sink failure must never hold containment cleanup forever.
                let _ = timeout(READER_DRAIN_DEADLINE, async {
                    for task in &mut self.readers {
                        if !task.is_finished() {
                            let _ = task.await;
                        }
                    }
                })
                .await;
            }
        }
        let snapshot = self.snapshot();
        (
            snapshot.stdout.closed && !snapshot.stdout.read_failed,
            snapshot.stderr.closed && !snapshot.stderr.read_failed,
        )
    }

    fn record_stop(&mut self, reason: Option<StopReason>, evidence: StopEvidence) -> StopRecord {
        let record = self.stop_state.record(reason, evidence);
        if let Some(sink) = &self.event_sink {
            sink.record(ExecutionLifecycleEvent::ProcessStop { record });
        }
        record
    }

    fn stop_scope(&mut self, reason: Option<StopReason>) -> StopRecord {
        let evidence = self.scope.terminate();
        self.record_stop(reason, evidence)
    }

    fn remember_exit(&mut self, status: ExitStatus) -> ProcessExit {
        let exit = exit_fact(status);
        self.known_exit = Some(exit);
        exit
    }

    #[cfg(unix)]
    async fn settle_natural_exit(
        &mut self,
    ) -> Result<(ProcessExit, Option<StopRecord>, Occupancy)> {
        self.scope.wait_ready().await?;
        let occupancy = self.scope.observe();
        self.known_occupancy = Some(occupancy);
        let cleanup = self.stop_scope(None);
        let status = self.scope.reap().await?;
        let exit = self.remember_exit(status);
        Ok((exit, Some(cleanup), occupancy))
    }

    #[cfg(windows)]
    async fn settle_windows_occupancy(&mut self) -> Occupancy {
        let until = Instant::now() + WINDOWS_OCCUPANCY_SETTLE;
        loop {
            match self.scope.observe() {
                Occupancy::Empty => {
                    self.known_occupancy = Some(Occupancy::Empty);
                    return Occupancy::Empty;
                }
                Occupancy::Unknown => {
                    self.known_occupancy = Some(Occupancy::Unknown);
                    return Occupancy::Unknown;
                }
                Occupancy::Live if Instant::now() >= until => {
                    self.known_occupancy = Some(Occupancy::Live);
                    return Occupancy::Live;
                }
                Occupancy::Live => {
                    let next = (Instant::now() + WINDOWS_OCCUPANCY_POLL).min(until);
                    sleep_until(next).await;
                }
            }
        }
    }

    #[cfg(windows)]
    async fn settle_natural_exit(
        &mut self,
    ) -> Result<(ProcessExit, Option<StopRecord>, Occupancy)> {
        let status = self.scope.reap().await?;
        let exit = self.remember_exit(status);
        let occupancy = self.settle_windows_occupancy().await;
        let cleanup = self.stop_scope(None);
        Ok((exit, Some(cleanup), occupancy))
    }

    async fn settle_preexisting_exit(&mut self) -> (StopRecord, ProcessExit, Occupancy) {
        #[cfg(unix)]
        let occupancy = self.known_occupancy.unwrap_or_else(|| self.scope.observe());

        #[cfg(windows)]
        let occupancy = self
            .known_occupancy
            .unwrap_or_else(|| match self.scope.observe() {
                Occupancy::Empty => Occupancy::Empty,
                Occupancy::Live | Occupancy::Unknown => Occupancy::Unknown,
            });

        self.known_occupancy = Some(occupancy);
        let cleanup = self.stop_scope(None);
        let exit = if let Some(exit) = self.known_exit {
            exit
        } else {
            match timeout(REAP_DEADLINE, self.scope.reap()).await {
                Ok(Ok(status)) => self.remember_exit(status),
                _ => ProcessExit::Unknown,
            }
        };
        (cleanup, exit, occupancy)
    }

    async fn stop(&mut self, reason: StopReason) -> (StopRecord, ProcessExit, Occupancy) {
        self.termination_attempted = true;
        self.stdin.take();

        if self.known_exit.is_some() || matches!(self.scope.ready(), Ok(true)) {
            return self.settle_preexisting_exit().await;
        }

        let graceful = self.scope.graceful();
        let graceful_record = self.record_stop(Some(reason), graceful);

        #[cfg(unix)]
        if graceful != StopEvidence::Unknown
            && matches!(
                timeout(GRACEFUL_TERM, self.scope.wait_ready()).await,
                Ok(Ok(()))
            )
        {
            let occupancy = self.scope.observe();
            self.known_occupancy = Some(occupancy);
            let cleanup = self.stop_scope(None);
            let status = match timeout(REAP_DEADLINE, self.scope.reap()).await {
                Ok(Ok(status)) => Some(status),
                _ => None,
            };
            let exit = status
                .as_ref()
                .map_or(ProcessExit::Unknown, |status| exit_fact(*status));
            self.known_exit = Some(exit);
            let record = if cleanup.evidence == StopEvidence::Unknown {
                StopRecord::new(reason, StopEvidence::Unknown)
            } else {
                reconcile_stop(graceful_record, status.as_ref(), false)
            };
            return (record, exit, occupancy);
        }

        #[cfg(windows)]
        let _ = graceful;

        // The leader may have become terminal while graceful settlement was
        // pending. Physical status established before the force stop wins.
        if matches!(self.scope.ready(), Ok(true)) {
            return self.settle_preexisting_exit().await;
        }

        let mut record = self.stop_scope(Some(reason));
        if record.evidence != StopEvidence::Stopped {
            let fallback = self.scope.kill_main();
            record = self.record_stop(Some(reason), fallback);
        }
        let status = match timeout(REAP_DEADLINE, self.scope.reap()).await {
            Ok(Ok(status)) => Some(status),
            _ => None,
        };
        let exit = status
            .as_ref()
            .map_or(ProcessExit::Unknown, |status| exit_fact(*status));
        self.known_exit = Some(exit);
        record = reconcile_stop(record, status.as_ref(), true);
        (record, exit, Occupancy::Unknown)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        for reader in &self.readers {
            reader.abort();
        }
        // Scope drops after this body and performs the final fail-safe
        // containment stop + main-child kill if settlement did not finish.
    }
}

fn exit_fact(status: ExitStatus) -> ProcessExit {
    if let Some(code) = status.code() {
        return ProcessExit::Code(code);
    }
    if let Some(signal) = aw_platform::exit_signal(&status) {
        return ProcessExit::Signal(signal);
    }
    ProcessExit::Unknown
}

fn reconcile_stop(mut record: StopRecord, status: Option<&ExitStatus>, force: bool) -> StopRecord {
    if record.reason.is_some()
        && record.evidence == StopEvidence::Stopped
        && status.is_some_and(|status| !aw_platform::stop_exit_matches(status, force))
    {
        // The stop operation reached live containment, but this physical main
        // exit is incompatible with our signal/marker. Retain operation evidence
        // without falsely attributing the command ending to timeout/cancel.
        record.reason = None;
    }
    record
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn weak_direct_unix_negative_is_unknown() {
        assert_eq!(occupancy(Ok(false)), Occupancy::Unknown);
        assert_eq!(occupancy(Ok(true)), Occupancy::Live);
        assert_eq!(
            occupancy(Err(io::Error::other("unavailable"))),
            Occupancy::Unknown
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn required_closed_scope_refuses_direct_unix_before_launch() {
        // Even an invalid program must be rejected by containment,
        // before the OS spawn or a provider launch fence can run.
        let command = Command::new("/definitely/not/an/aw-process-command");
        let control = SpawnControl::new_fenced(
            Instant::now() + Duration::from_secs(3),
            || false,
            std::future::pending(),
            |_| panic!("unsupported containment must not enter launch fence"),
        );
        let requirement = ContainmentRequirement::ClosedDescendants
            .intersect(ContainmentRequirement::BestAvailable);
        assert!(matches!(
            Process::spawn_with_containment(
                command,
                SpawnOptions {
                    control: Some(control),
                    ..Default::default()
                },
                requirement,
            )
            .await,
            Err(Error::ContainmentUnavailable)
        ));
        assert!(matches!(
            Process::spawn(
                Command::new("/definitely/not/an/aw-process-command"),
                SpawnOptions::default(),
            )
            .await,
            Err(Error::Io(_))
        ));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn spawn_lane_rechecks_cancellation_before_exec() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let probes = Arc::new(AtomicUsize::new(0));
        let cancellation = probes.clone();
        let control = SpawnControl::new(
            Instant::now() + Duration::from_secs(3),
            move || cancellation.fetch_add(1, Ordering::SeqCst) != 0,
            std::future::pending(),
        );
        let command = Command::new("/definitely/not/an/aw-process-command");
        let error = match Process::spawn(
            command,
            SpawnOptions {
                control: Some(control),
                ..Default::default()
            },
        )
        .await
        {
            Ok(_) => panic!("lane-side recheck must suppress exec"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::CancelledBeforeSpawn));
        assert!(probes.load(Ordering::SeqCst) >= 2);

        // Once the synchronous authority fence is entered, cancellation
        // still wins before the real OS call (an invalid program would
        // otherwise give Io). No authority callback fabricates a result.
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let probe = cancelled.clone();
        let control = SpawnControl::new_fenced(
            Instant::now() + Duration::from_secs(3),
            move || probe.load(Ordering::Acquire),
            std::future::pending(),
            move |spawn| {
                cancelled.store(true, Ordering::Release);
                spawn.spawn();
            },
        );
        let error = match Process::spawn(
            Command::new("/definitely/not/an/aw-process-command"),
            SpawnOptions {
                control: Some(control),
                ..Default::default()
            },
        )
        .await
        {
            Ok(_) => panic!("fenced cancellation must suppress exec"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::CancelledBeforeSpawn));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn controlled_spawn_reuses_one_absolute_deadline() {
        let deadline = Instant::now() + Duration::from_millis(120);
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 5"]);
        let process = Process::spawn(
            command,
            SpawnOptions {
                control: Some(SpawnControl::new(
                    deadline,
                    || false,
                    std::future::pending(),
                )),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let started = Instant::now();
        let result = process
            .wait(std::future::pending::<()>(), Duration::from_secs(5))
            .await;
        assert_eq!(result.terminal.stop_reason(), Some(StopReason::Timeout));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "wait restarted the full fallback timeout"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn process_lifetime_spawn_parent_survives_submitting_thread_exit() {
        use std::{
            os::unix::process::CommandExt as _,
            path::PathBuf,
            sync::mpsc,
            thread,
            time::{SystemTime, UNIX_EPOCH},
        };

        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let marker: PathBuf =
            std::env::temp_dir().join(format!("aw-process-parent-{}-{suffix}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let script = format!(
            "sleep 0.15; printf survived > '{}'; sleep 5",
            marker.display()
        );
        let (tx, rx) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let process = runtime.block_on(async move {
                let mut command = Command::new("/bin/sh");
                command.args(["-c", &script]);
                // SAFETY: prctl is async-signal-safe and this callback performs
                // no allocation or lock acquisition in the child.
                unsafe {
                    command.as_std_mut().pre_exec(|| {
                        if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                            return Err(io::Error::last_os_error());
                        }
                        Ok(())
                    });
                }
                Process::spawn(command, SpawnOptions::default())
                    .await
                    .unwrap()
            });
            tx.send(process).unwrap();
            // Dropping this runtime and ending this OS thread would kill the
            // child if the request thread itself were still the fork parent.
        })
        .join()
        .unwrap();

        let process = rx.recv().unwrap();
        thread::sleep(Duration::from_millis(350));
        assert_eq!(std::fs::read(&marker).unwrap(), b"survived");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let mut process = process;
            let _ = process.terminate().await;
        });
        let _ = std::fs::remove_file(marker);
    }

    #[cfg(unix)]
    #[test]
    fn incompatible_exit_does_not_acquire_stop_cause() {
        use std::os::unix::process::ExitStatusExt as _;

        let status = ExitStatus::from_raw(0);
        let record = reconcile_stop(
            StopRecord::stopped(StopReason::Cancelled),
            Some(&status),
            true,
        );
        assert_eq!(record.reason, None);
        assert_eq!(record.evidence, StopEvidence::Stopped);
    }

    #[derive(Clone)]
    struct RecordingSink(Arc<Mutex<Vec<u8>>>);

    impl OutputSink for RecordingSink {
        fn discard_if_full(&mut self, _seen: u64) -> std::result::Result<bool, SinkError> {
            Ok(false)
        }

        fn append(&mut self, bytes: &[u8]) -> std::result::Result<usize, SinkError> {
            self.0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn observe_after_failure(&mut self, _seen: u64) -> std::result::Result<(), SinkError> {
            Ok(())
        }

        fn mark_failed(&mut self) {}

        fn finish(&mut self) -> std::result::Result<(), SinkError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn reader_stop_flushes_partial_sink_batch_before_incomplete() {
        let retained = Arc::new(Mutex::new(Vec::new()));
        let (mut producer, input) = tokio::io::duplex(64);
        let (reader, task) = Reader::start(input, Some(Box::new(RecordingSink(retained.clone()))));

        producer.write_all(b"done").await.unwrap();
        for _ in 0..32 {
            if reader.snapshot().total_bytes == 4 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(reader.snapshot().total_bytes, 4);

        // Keep the producer open: this models an inherited descendant FD that
        // prevents EOF after the main command has already exited.
        reader.stop();
        task.await.unwrap();

        assert_eq!(
            retained
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            b"done"
        );
        let snapshot = reader.snapshot();
        assert!(snapshot.closed);
        assert!(snapshot.read_failed, "forced bounded drain is incomplete");
    }

    #[test]
    fn capture_page_never_crosses_missing_middle() {
        let mut capture = CaptureState::default();
        capture.append(&vec![b'a'; CAPTURE_HEAD + CAPTURE_TAIL + 1024]);
        capture.closed = true;
        let snapshot = capture.snapshot();
        let gap = snapshot.retained_gap().expect("capture should have a gap");
        assert!(matches!(
            snapshot.page(gap.0, 1),
            Err(OutputPageError::Gap { .. })
        ));
        assert!(snapshot.page(0, 1024).is_ok());
        assert!(snapshot.page(gap.1, 1024).is_ok());
    }
}
