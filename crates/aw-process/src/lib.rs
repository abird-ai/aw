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
    io,
    process::{ExitStatus, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, Command},
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::{sleep, timeout},
};

pub const CAPTURE_HEAD: usize = 48 * 1024;
pub const CAPTURE_TAIL: usize = 64 * 1024;
pub const MAX_PROCESS_PAGE_BYTES: usize = 48 * 1024;
pub const MAX_STDIN_WRITE: usize = 64 * 1024;

const REAP_DEADLINE: Duration = Duration::from_secs(2);
const READER_DRAIN_DEADLINE: Duration = Duration::from_millis(600);
const GRACEFUL_TERM: Duration = Duration::from_millis(350);
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

#[derive(Default)]
pub struct SpawnOptions {
    pub stdin_open: bool,
    pub stdout: Option<Box<dyn OutputSink>>,
    pub stderr: Option<Box<dyn OutputSink>>,
    pub event_sink: Option<Arc<dyn ExecutionEventSink>>,
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
}

impl Reader {
    fn snapshot(&self) -> StreamSnapshot {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .snapshot()
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
                match input.read(&mut buffer).await {
                    Ok(0) => break false,
                    Ok(n) => {
                        shared
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .append(&buffer[..n]);
                        if let Some(ref tx) = sender {
                            batch.extend_from_slice(&buffer[..n]);
                            if batch.len() >= SINK_BATCH {
                                let full =
                                    std::mem::replace(&mut batch, Vec::with_capacity(SINK_BATCH));
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

        (Self { state }, task)
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

    fn kill_main(&mut self) {
        let _ = self.child.start_kill();
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
    termination_attempted: bool,
    stop_state: StopTracker,
    event_sink: Option<Arc<dyn ExecutionEventSink>>,
}

impl Process {
    /// Finalize and spawn one already-authorized command.
    ///
    /// The caller must not preconfigure process containment or stdio ownership;
    /// those are lifetime-sensitive and are owned here.
    pub fn spawn(mut command: Command, options: SpawnOptions) -> Result<Self> {
        command
            .stdin(if options.stdin_open {
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

        let mut child = command.spawn()?;
        let Some(pid) = child.id() else {
            let _ = child.start_kill();
            return Err(Error::SetupUncertain(
                "process launched but PID/setup outcome is uncertain",
            ));
        };

        #[cfg(unix)]
        let mut platform = match aw_platform::DirectScope::attach(pid) {
            Ok(scope) => scope,
            Err(_) => {
                let _ = child.start_kill();
                return Err(Error::SetupUncertain(
                    "process launched but Process scope setup is uncertain",
                ));
            }
        };

        #[cfg(windows)]
        let mut platform = {
            let Some(handle) = child.raw_handle() else {
                let _ = child.start_kill();
                return Err(Error::SetupUncertain(
                    "process launched but Windows handle/setup outcome is uncertain",
                ));
            };
            match aw_platform::DirectScope::attach(pid, handle) {
                Ok(scope) => scope,
                Err(_) => {
                    let _ = child.start_kill();
                    return Err(Error::SetupUncertain(
                        "process launched but Windows Process scope setup is uncertain",
                    ));
                }
            }
        };

        let Some(stdout) = child.stdout.take() else {
            let _ = platform.terminate();
            let _ = child.start_kill();
            return Err(Error::SetupUncertain(
                "process launched but stdout setup outcome is uncertain",
            ));
        };
        let Some(stderr) = child.stderr.take() else {
            let _ = platform.terminate();
            let _ = child.start_kill();
            return Err(Error::SetupUncertain(
                "process launched but stderr setup outcome is uncertain",
            ));
        };
        let stdin = child.stdin.take();

        let (stdout_reader, stdout_task) = Reader::start(stdout, options.stdout);
        let (stderr_reader, stderr_task) = Reader::start(stderr, options.stderr);

        Ok(Self {
            scope: Scope::new(platform, child, pid),
            stdin,
            stdout: stdout_reader,
            stderr: stderr_reader,
            readers: [stdout_task, stderr_task],
            termination_attempted: false,
            stop_state: StopTracker::default(),
            event_sink: options.event_sink,
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
        if !self.termination_available()? {
            return Err(Error::Conflict(
                "Process already exited or had a termination attempt; inspect original receipt",
            ));
        }
        let (stop, exit, occupancy) = self.stop(StopReason::Cancelled).await;
        Ok(self.finish(exit, Some(stop), occupancy).await)
    }

    pub async fn wait<C: core::future::Future<Output = ()>>(
        mut self,
        cancellation: C,
        deadline: Duration,
    ) -> ProcessResult {
        let (exit, stop, occupancy) = tokio::select! {
            biased;
            _ = cancellation => {
                let (record, exit, occupancy) = self.stop(StopReason::Cancelled).await;
                (exit, Some(record), occupancy)
            },
            result = timeout(deadline, self.settle_natural_exit()) => match result {
                Ok(Ok(settled)) => settled,
                Ok(Err(_)) => {
                    let (record, exit, occupancy) = self.stop(StopReason::RuntimeError).await;
                    (exit, Some(record), occupancy)
                }
                Err(_) => {
                    let (record, exit, occupancy) = self.stop(StopReason::Timeout).await;
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
        let deadline = sleep(deadline);
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
                            let _ = reply.send(self.write(&bytes).await);
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
        for task in &mut self.readers {
            match timeout(READER_DRAIN_DEADLINE, &mut *task).await {
                Ok(Ok(())) => {}
                _ => {
                    task.abort();
                    let _ = timeout(READER_DRAIN_DEADLINE, &mut *task).await;
                }
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

    #[cfg(unix)]
    async fn settle_natural_exit(
        &mut self,
    ) -> Result<(ProcessExit, Option<StopRecord>, Occupancy)> {
        self.scope.wait_ready().await?;
        let occupancy = self.scope.observe();
        let cleanup = self.stop_scope(None);
        let status = self.scope.reap().await?;
        Ok((exit_fact(status), Some(cleanup), occupancy))
    }

    #[cfg(windows)]
    async fn settle_natural_exit(
        &mut self,
    ) -> Result<(ProcessExit, Option<StopRecord>, Occupancy)> {
        let status = self.scope.reap().await?;
        let occupancy = self.scope.observe();
        let cleanup = self.stop_scope(None);
        Ok((exit_fact(status), Some(cleanup), occupancy))
    }

    async fn stop(&mut self, reason: StopReason) -> (StopRecord, ProcessExit, Occupancy) {
        self.termination_attempted = true;
        self.stdin.take();

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
            let cleanup = self.stop_scope(None);
            let exit = match timeout(REAP_DEADLINE, self.scope.reap()).await {
                Ok(Ok(status)) => exit_fact(status),
                _ => ProcessExit::Unknown,
            };
            let record = if cleanup.evidence == StopEvidence::Unknown {
                StopRecord::new(reason, StopEvidence::Unknown)
            } else {
                graceful_record
            };
            return (record, exit, occupancy);
        }

        #[cfg(windows)]
        let _ = graceful;

        let record = self.stop_scope(Some(reason));
        self.scope.kill_main();
        let exit = match timeout(REAP_DEADLINE, self.scope.reap()).await {
            Ok(Ok(status)) => exit_fact(status),
            _ => ProcessExit::Unknown,
        };
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
