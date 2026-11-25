//! Host-owned Process adapter: no model tool, sandbox or second Agent.
//! A trusted caller authenticates grants and journals Process intent before
//! spawn. Pipe lifetimes delegate to aw-process; PTY Terminal custody remains
//! a distinct aw-terminal resource.
pub use aw_process::{
    ContainmentRequirement, OutputPageError, OutputStream, ProcessOutputPage, ProcessSnapshot,
    StreamSnapshot, CAPTURE_HEAD, CAPTURE_TAIL, MAX_PROCESS_PAGE_BYTES,
};
use aw_tools::{
    execution_stop::{
        ExecutionEventSink, StopEvidence, StopReason as ProcessStopReason, StopRecord,
    },
    filesystem::{
        job_manager::OutputWriter,
        runner::{ProcessExit as RunnerExit, ProcessTerminal, StopProof},
    },
    task::{SpoolWriterLease, SpoolWriters, StreamSummary, TaskDisposition},
    task_fence::{FenceError, TaskFence, TaskIdentity, TaskTransition},
    task_ops::{HostTaskOperations, Stream as TaskStream, TaskInfo},
    task_spool::{BoundedFileSpool, SessionQuota, SpoolError},
};
use std::{
    collections::BTreeMap,
    fs::File,
    future::Future,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{process::Command, time::Instant};

#[cfg(test)]
use aw_tools::{execution_stop::ExecutionLifecycleEvent, filesystem::runner::Occupancy};

#[derive(Clone)]
pub(crate) struct ProcessInput(aw_process::ProcessInput);

impl ProcessInput {
    pub async fn write(&self, bytes: Vec<u8>) -> crate::Result<()> {
        self.0.write(bytes).await.map_err(map_process_error)
    }

    pub async fn close(&self) -> crate::Result<()> {
        self.0.close().await.map_err(map_process_error)
    }
}

pub(crate) fn process_input_channel() -> (ProcessInput, aw_process::ProcessInputReceiver) {
    let (input, receiver) = aw_process::process_input_channel();
    (ProcessInput(input), receiver)
}

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
            terminal: None,
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
            terminal: None,
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

enum RetainedSinkKind {
    Legacy(Arc<Mutex<BoundedFileSpool>>),
    Job(OutputWriter),
}

struct RetainedSink {
    inner: RetainedSinkKind,
    // The shared SpoolWriters gate remains an AW storage concern. Holding this
    // lease through the shared sink worker keeps legacy spool readiness tied to
    // the actual pipe-reader lifetime without teaching aw-process about spools.
    _reader_lease: Option<SpoolWriterLease>,
}

impl RetainedSink {
    fn legacy(spool: Arc<Mutex<BoundedFileSpool>>, lease: SpoolWriterLease) -> Self {
        Self {
            inner: RetainedSinkKind::Legacy(spool),
            _reader_lease: Some(lease),
        }
    }

    fn job(writer: OutputWriter) -> Self {
        Self {
            inner: RetainedSinkKind::Job(writer),
            _reader_lease: None,
        }
    }
}

impl aw_process::OutputSink for RetainedSink {
    fn discard_if_full(&mut self, seen: u64) -> Result<bool, aw_process::SinkError> {
        match &self.inner {
            RetainedSinkKind::Legacy(spool) => spool
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .discard_if_full(seen)
                .map_err(|_| aw_process::SinkError),
            RetainedSinkKind::Job(writer) => writer
                .discard_if_full(seen)
                .map_err(|_| aw_process::SinkError),
        }
    }

    fn append(&mut self, bytes: &[u8]) -> Result<usize, aw_process::SinkError> {
        match &self.inner {
            RetainedSinkKind::Legacy(spool) => spool
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .append(bytes)
                .map_err(|_| aw_process::SinkError),
            RetainedSinkKind::Job(writer) => {
                writer.append(bytes).map_err(|_| aw_process::SinkError)
            }
        }
    }

    fn observe_after_failure(&mut self, seen: u64) -> Result<(), aw_process::SinkError> {
        match &self.inner {
            RetainedSinkKind::Legacy(spool) => spool
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .observe_after_failure(seen)
                .map_err(|_| aw_process::SinkError),
            RetainedSinkKind::Job(writer) => writer
                .observe_after_failure(seen)
                .map_err(|_| aw_process::SinkError),
        }
    }

    fn mark_failed(&mut self) {
        match &self.inner {
            RetainedSinkKind::Legacy(spool) => spool
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .mark_writer_failed(),
            RetainedSinkKind::Job(writer) => {
                let _ = writer.mark_failed();
            }
        }
    }

    fn finish(&mut self) -> Result<(), aw_process::SinkError> {
        match &self.inner {
            RetainedSinkKind::Legacy(spool) => spool
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .finish_writer()
                .map_err(|_| aw_process::SinkError),
            RetainedSinkKind::Job(writer) => writer.finish().map_err(|_| aw_process::SinkError),
        }
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
    ExitedBySignal(i32),
    Cancelled,
    DeadlineExceeded,
    Unknown,
}
pub struct ProcessReceipt {
    /// One factual Process terminal truth. Outcome/status are projections.
    pub terminal: ProcessTerminal,
    pub output: ProcessSnapshot,
    /// Present only when the trusted host supplied durable output handles.
    /// Ownership is the original Process; no model-facing job ID exists.
    pub spools: Option<ProcessSpoolAccess>,
}
impl ProcessReceipt {
    fn from_runtime(result: aw_process::ProcessResult, spools: Option<ProcessSpoolAccess>) -> Self {
        Self {
            terminal: result.terminal,
            output: result.output,
            spools,
        }
    }

    pub fn outcome(&self) -> Outcome {
        if self
            .terminal
            .stop
            .is_some_and(|record| record.evidence == StopEvidence::Unknown)
        {
            return Outcome::Unknown;
        }
        match self.terminal.stop_reason() {
            Some(ProcessStopReason::Cancelled) => return Outcome::Cancelled,
            Some(ProcessStopReason::Timeout) => return Outcome::DeadlineExceeded,
            Some(ProcessStopReason::RuntimeError) => return Outcome::Unknown,
            None => {}
        }
        match self.terminal.exit {
            RunnerExit::Code(code) => Outcome::Exited(code),
            RunnerExit::Signal(signal) => Outcome::ExitedBySignal(signal),
            RunnerExit::Unknown => Outcome::Unknown,
        }
    }

    pub fn output_complete(&self) -> bool {
        self.terminal.output.complete()
    }

    pub fn finished(&self) -> bool {
        matches!(
            self.outcome(),
            Outcome::Exited(_) | Outcome::ExitedBySignal(_)
        ) && self.output_complete()
    }

    pub const fn stop_record(&self) -> Option<StopRecord> {
        self.terminal.stop
    }

    pub const fn runner_terminal(&self) -> ProcessTerminal {
        self.terminal
    }

    pub fn runner_status(&self) -> aw_tools::filesystem::batch::Status {
        self.terminal.status()
    }

    pub fn stop_proof(&self) -> StopProof {
        let terminal = self.terminal;
        if !terminal.output.complete()
            || terminal
                .stop
                .is_some_and(|record| record.evidence == StopEvidence::Unknown)
            || (terminal.exit == RunnerExit::Unknown && terminal.stop_reason().is_none())
        {
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

struct RuntimeOutput {
    spools: Option<ProcessSpoolAccess>,
    stdout: Option<Box<dyn aw_process::OutputSink>>,
    stderr: Option<Box<dyn aw_process::OutputSink>>,
}

fn map_process_error(error: aw_process::Error) -> crate::Error {
    match error {
        aw_process::Error::Io(error) => crate::Error::Io(error),
        aw_process::Error::SetupUncertain(message) | aw_process::Error::OwnerGone(message) => {
            crate::Error::Transport(message)
        }
        aw_process::Error::Limit(message) => crate::Error::Limit(message),
        aw_process::Error::InputClosed(message) => crate::Error::Denied(message),
        aw_process::Error::Conflict(message) => crate::Error::Conflict(message),
        aw_process::Error::ContainmentUnavailable => {
            crate::Error::Denied("required closed descendant containment is unavailable")
        }
        aw_process::Error::DeniedBeforeSpawn => {
            crate::Error::Denied("process launch authority denied")
        }
        aw_process::Error::CancelledBeforeSpawn => crate::Error::Cancelled,
        aw_process::Error::TimedOutBeforeSpawn | aw_process::Error::Timeout => {
            crate::Error::Timeout
        }
    }
}

pub use aw_process::SpawnOnce;

/// Trusted prepared command, minimum containment and launch pins. No
/// model arguments select this capability. The Process runtime owns
/// Scope selection, stdio, launch and terminal facts.
pub struct PreparedPipeLaunch<P = ()> {
    command: Command,
    stdin_open: bool,
    pins: P,
    containment: ContainmentRequirement,
}

impl<P> PreparedPipeLaunch<P> {
    pub fn from_trusted_command(command: std::process::Command, stdin_open: bool, pins: P) -> Self {
        Self {
            command: Command::from(command),
            stdin_open,
            pins,
            containment: ContainmentRequirement::BestAvailable,
        }
    }

    /// A trusted driver or policy owner may strengthen, never weaken,
    /// the already prepared Process containment requirement.
    pub fn requiring_containment(mut self, requirement: ContainmentRequirement) -> Self {
        self.containment = self.containment.intersect(requirement);
        self
    }

    /// Transfer additional live authority/capability custody with this launch.
    pub fn with_custody<A>(self, authority: A) -> PreparedPipeLaunch<(P, A)> {
        PreparedPipeLaunch {
            command: self.command,
            stdin_open: self.stdin_open,
            pins: (self.pins, authority),
            containment: self.containment,
        }
    }
}

/// Preserve the runtime's distinction between definite no-spawn and
/// uncertain setup. Neither case fabricates a ProcessTerminal.
#[derive(Debug)]
pub enum PipeLaunchFailure {
    NotStarted(crate::Error),
    Unknown(crate::Error),
}

impl PipeLaunchFailure {
    fn from_runtime(error: aw_process::Error) -> Self {
        let not_started = matches!(
            error,
            aw_process::Error::Io(_)
                | aw_process::Error::Limit(_)
                | aw_process::Error::ContainmentUnavailable
                | aw_process::Error::DeniedBeforeSpawn
                | aw_process::Error::CancelledBeforeSpawn
                | aw_process::Error::TimedOutBeforeSpawn
        );
        let error = map_process_error(error);
        if not_started {
            Self::NotStarted(error)
        } else {
            Self::Unknown(error)
        }
    }

    pub fn into_error(self) -> crate::Error {
        match self {
            Self::NotStarted(error) | Self::Unknown(error) => error,
        }
    }
}

pub struct PipeProcess {
    process: aw_process::Process,
    spools: Option<ProcessSpoolAccess>,
}

impl PipeProcess {
    pub async fn spawn(spec: PipeSpec) -> crate::Result<Self> {
        Self::spawn_inner(spec, SpawnOutputTarget::None, None, None).await
    }

    pub async fn spawn_until(
        spec: PipeSpec,
        deadline: Instant,
        cancelled: impl Fn() -> bool + Send + 'static,
        cancelled_signal: impl Future<Output = ()> + Send + 'static,
    ) -> crate::Result<Self> {
        Self::spawn_inner(
            spec,
            SpawnOutputTarget::None,
            None,
            Some(aw_process::SpawnControl::new(
                deadline,
                cancelled,
                cancelled_signal,
            )),
        )
        .await
    }

    pub async fn spawn_with_event_sink(
        spec: PipeSpec,
        event_sink: Arc<dyn ExecutionEventSink>,
    ) -> crate::Result<Self> {
        Self::spawn_inner(spec, SpawnOutputTarget::None, Some(event_sink), None).await
    }

    pub async fn spawn_with_host_spools(
        spec: PipeSpec,
        files: HostSpoolFiles,
    ) -> crate::Result<Self> {
        Self::spawn_inner(spec, SpawnOutputTarget::Legacy(files), None, None).await
    }

    pub async fn spawn_with_job_outputs(
        spec: PipeSpec,
        stdout: OutputWriter,
        stderr: OutputWriter,
    ) -> crate::Result<Self> {
        Self::spawn_inner(spec, SpawnOutputTarget::Job { stdout, stderr }, None, None).await
    }

    pub async fn spawn_with_job_outputs_until(
        spec: PipeSpec,
        stdout: OutputWriter,
        stderr: OutputWriter,
        deadline: Instant,
        cancelled: impl Fn() -> bool + Send + 'static,
        cancelled_signal: impl Future<Output = ()> + Send + 'static,
    ) -> crate::Result<Self> {
        Self::spawn_inner(
            spec,
            SpawnOutputTarget::Job { stdout, stderr },
            None,
            Some(aw_process::SpawnControl::new(
                deadline,
                cancelled,
                cancelled_signal,
            )),
        )
        .await
    }

    pub async fn spawn_with_job_outputs_and_event_sink(
        spec: PipeSpec,
        stdout: OutputWriter,
        stderr: OutputWriter,
        event_sink: Arc<dyn ExecutionEventSink>,
    ) -> crate::Result<Self> {
        Self::spawn_inner(
            spec,
            SpawnOutputTarget::Job { stdout, stderr },
            Some(event_sink),
            None,
        )
        .await
    }

    /// Consume a driver-prepared launch capability through the same Process
    /// owner as PipeSpec. Pins stay live until the async launch returns.
    /// No fallback to a layout-only command.
    pub async fn spawn_prepared_until<P: Send + 'static>(
        launch: PreparedPipeLaunch<P>,
        deadline: Instant,
        cancelled: impl Fn() -> bool + Send + 'static,
        cancelled_signal: impl Future<Output = ()> + Send + 'static,
    ) -> std::result::Result<Self, PipeLaunchFailure> {
        let PreparedPipeLaunch {
            command,
            stdin_open,
            pins,
            containment,
        } = launch;
        // The existing spawn control moves its level-triggered probe into
        // the Linux launch lane with the command. Capture custody there,
        // not merely in this awaiter: dropping the caller during launch
        // must not close/reuse the driver's pinned descriptors.
        let control = aw_process::SpawnControl::new(
            deadline,
            move || {
                let _custody = &pins;
                cancelled()
            },
            cancelled_signal,
        );
        Self::spawn_command_inner(
            command,
            stdin_open,
            SpawnOutputTarget::None,
            None,
            Some(control),
            containment,
        )
        .await
    }

    /// Opt-in final authority and projection at the original spawn lane.
    /// Custody moves into the fence, including cancellation before launch.
    pub async fn spawn_prepared_fenced_until<P: Send + 'static>(
        launch: PreparedPipeLaunch<P>,
        deadline: Instant,
        cancelled: impl Fn() -> bool + Send + 'static,
        cancelled_signal: impl Future<Output = ()> + Send + 'static,
        fence: impl FnOnce(SpawnOnce<'_>, &P) + Send + 'static,
    ) -> std::result::Result<Self, PipeLaunchFailure> {
        let PreparedPipeLaunch {
            command,
            stdin_open,
            pins,
            containment,
        } = launch;
        let control = aw_process::SpawnControl::new_fenced(
            deadline,
            cancelled,
            cancelled_signal,
            move |spawn| fence(spawn, &pins),
        );
        Self::spawn_command_inner(
            command,
            stdin_open,
            SpawnOutputTarget::None,
            None,
            Some(control),
            containment,
        )
        .await
    }

    async fn spawn_inner(
        spec: PipeSpec,
        target: SpawnOutputTarget,
        event_sink: Option<Arc<dyn ExecutionEventSink>>,
        control: Option<aw_process::SpawnControl>,
    ) -> crate::Result<Self> {
        spec.validate()?;
        let mut command = Command::new(spec.program);
        command.args(spec.args).current_dir(spec.root).env_clear();
        for (key, value) in spec.env {
            command.env(key, value);
        }
        Self::spawn_command_inner(
            command,
            spec.stdin_open,
            target,
            event_sink,
            control,
            ContainmentRequirement::BestAvailable,
        )
        .await
        .map_err(PipeLaunchFailure::into_error)
    }

    async fn spawn_command_inner(
        command: Command,
        stdin_open: bool,
        target: SpawnOutputTarget,
        event_sink: Option<Arc<dyn ExecutionEventSink>>,
        control: Option<aw_process::SpawnControl>,
        containment: ContainmentRequirement,
    ) -> std::result::Result<Self, PipeLaunchFailure> {
        let output = Self::prepare_outputs(target).map_err(PipeLaunchFailure::NotStarted)?;
        let process = aw_process::Process::spawn_with_containment(
            command,
            aw_process::SpawnOptions {
                stdin_open,
                stdout: output.stdout,
                stderr: output.stderr,
                event_sink,
                control,
            },
            containment,
        )
        .await
        .map_err(PipeLaunchFailure::from_runtime)?;
        Ok(Self {
            process,
            spools: output.spools,
        })
    }

    // Validate/reserve product-owned output capabilities before external
    // spawn. Only the tiny OutputSink adapter crosses into aw-process.
    fn prepare_outputs(target: SpawnOutputTarget) -> crate::Result<RuntimeOutput> {
        Ok(match target {
            SpawnOutputTarget::Legacy(files) => {
                {
                    let _quota_check = files.session_quota.lock().map_err(|_| {
                        crate::Error::Denied(
                            "host spool quota is poisoned; inspect before process launch",
                        )
                    })?;
                }
                let a = aw_tools::task_spool::spool_file_identity(&files.stdout)
                    .map_err(|_| crate::Error::Denied("invalid host stdout spool"))?;
                let b = aw_tools::task_spool::spool_file_identity(&files.stderr)
                    .map_err(|_| crate::Error::Denied("invalid host stderr spool"))?;
                if a == b {
                    return Err(crate::Error::Denied(
                        "stdout and stderr require distinct spool files",
                    ));
                }

                let spool_writers = Arc::new(SpoolWriters::default());
                let out = BoundedFileSpool::new(
                    files.stdout,
                    files.session_quota.clone(),
                    spool_writers.clone(),
                )
                .map_err(|_| crate::Error::Denied("invalid host stdout spool"))?;
                let err =
                    BoundedFileSpool::new(files.stderr, files.session_quota, spool_writers.clone())
                        .map_err(|_| crate::Error::Denied("invalid host stderr spool"))?;
                let Some([out_lease, err_lease]) = spool_writers.begin::<2>() else {
                    return Err(crate::Error::Denied(
                        "host spool writers are already sealed",
                    ));
                };
                // All legitimate writer/reader leases are now reserved. No
                // Process path may register a writer after external spawn.
                spool_writers.seal();

                let stdout = Arc::new(Mutex::new(out));
                let stderr = Arc::new(Mutex::new(err));
                let access = ProcessSpoolAccess {
                    stdout: stdout.clone(),
                    stderr: stderr.clone(),
                };
                RuntimeOutput {
                    spools: Some(access),
                    stdout: Some(Box::new(RetainedSink::legacy(stdout, out_lease))),
                    stderr: Some(Box::new(RetainedSink::legacy(stderr, err_lease))),
                }
            }
            SpawnOutputTarget::Job { stdout, stderr } => RuntimeOutput {
                spools: None,
                stdout: Some(Box::new(RetainedSink::job(stdout))),
                stderr: Some(Box::new(RetainedSink::job(stderr))),
            },
            SpawnOutputTarget::None => RuntimeOutput {
                spools: None,
                stdout: None,
                stderr: None,
            },
        })
    }

    pub fn mode(&self) -> ProcessMode {
        ProcessMode::Pipes
    }

    pub fn snapshot(&self) -> ProcessSnapshot {
        self.process.snapshot()
    }

    pub async fn write(&mut self, bytes: &[u8]) -> crate::Result<()> {
        self.process.write(bytes).await.map_err(map_process_error)
    }

    pub fn stdin_open(&self) -> bool {
        self.process.stdin_open()
    }

    pub fn close_stdin(&mut self) {
        self.process.close_stdin();
    }

    pub fn termination_available(&self) -> crate::Result<bool> {
        self.process
            .termination_available()
            .map_err(map_process_error)
    }

    pub async fn terminate(&mut self) -> crate::Result<ProcessReceipt> {
        let result = self.process.terminate().await.map_err(map_process_error)?;
        Ok(ProcessReceipt::from_runtime(result, self.spools.clone()))
    }

    pub async fn wait<C: core::future::Future<Output = ()>>(
        self,
        cancellation: C,
        deadline: Duration,
    ) -> ProcessReceipt {
        let Self { process, spools } = self;
        ProcessReceipt::from_runtime(process.wait(cancellation, deadline).await, spools)
    }

    pub(crate) async fn wait_interactive<C: core::future::Future<Output = ()>>(
        self,
        cancellation: C,
        deadline: Duration,
        input: aw_process::ProcessInputReceiver,
    ) -> ProcessReceipt {
        let Self { process, spools } = self;
        ProcessReceipt::from_runtime(
            process
                .wait_interactive(cancellation, deadline, input)
                .await,
            spools,
        )
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

    #[derive(Default)]
    struct RecordingEventSink(Mutex<Vec<ExecutionLifecycleEvent>>);

    impl ExecutionEventSink for RecordingEventSink {
        fn record(&self, event: ExecutionLifecycleEvent) {
            self.0.lock().expect("event sink").push(event);
        }
    }

    #[tokio::test]
    async fn lifecycle_sink_observes_connector_stop_but_not_external_exit() {
        let sink = Arc::new(RecordingEventSink::default());
        let process = PipeProcess::spawn_with_event_sink(spec("while :; do :; done"), sink.clone())
            .await
            .unwrap();
        let receipt = process
            .wait(
                async { tokio::time::sleep(Duration::from_millis(30)).await },
                Duration::from_secs(3),
            )
            .await;
        assert_eq!(
            receipt.runner_terminal().stop,
            Some(StopRecord::stopped(ProcessStopReason::Cancelled))
        );
        assert_eq!(
            receipt.stop_record(),
            Some(StopRecord::stopped(ProcessStopReason::Cancelled)),
            "one final factual stop record must be available independently of attempt events"
        );
        assert!(sink.0.lock().expect("events").iter().any(|event| matches!(
            event,
            ExecutionLifecycleEvent::ProcessStop {
                record: StopRecord {
                    reason: Some(ProcessStopReason::Cancelled),
                    evidence: StopEvidence::Stopped,
                },
            }
        )));

        let external = Arc::new(RecordingEventSink::default());
        let process = PipeProcess::spawn_with_event_sink(spec("kill -KILL $$"), external.clone())
            .await
            .unwrap();
        let receipt = process.wait(pending::<()>(), Duration::from_secs(3)).await;
        assert_eq!(receipt.runner_terminal().exit, RunnerExit::Signal(9));
        #[cfg(unix)]
        assert_eq!(receipt.runner_terminal().occupancy, Occupancy::Unknown);
        #[cfg(windows)]
        assert_eq!(receipt.runner_terminal().occupancy, Occupancy::Empty);
        assert_eq!(
            receipt
                .runner_terminal()
                .stop
                .and_then(|record| record.reason),
            None,
            "external process death may retain housekeeping cleanup but never a semantic stop"
        );
        assert!(
            receipt
                .stop_record()
                .is_none_or(|record| record.reason.is_none()),
            "external process death must not acquire a semantic connector stop record"
        );
        assert!(external
            .0
            .lock()
            .expect("events")
            .iter()
            .all(|event| matches!(
                event,
                ExecutionLifecycleEvent::ProcessStop {
                    record: StopRecord { reason: None, .. },
                }
            )));
    }

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
        assert_eq!(receipt.outcome(), Outcome::Cancelled);
        assert!(receipt.output_complete());
        assert!(!process.stdin_open());
        assert!(
            !process.termination_available().unwrap(),
            "one original OS Process incarnation permits only one termination attempt"
        );
        assert!(
            process.terminate().await.is_err(),
            "post-settlement original Process must never receive a second signal"
        );
        let terminal = receipt.runner_terminal();
        assert_eq!(
            terminal.stop,
            Some(StopRecord::stopped(ProcessStopReason::Cancelled))
        );
        #[cfg(unix)]
        match terminal.exit {
            RunnerExit::Signal(signal) => {
                assert_eq!(aw_platform::signal_name(signal), Some("SIGTERM"));
            }
            other => panic!("expected factual SIGTERM exit, got {other:?}"),
        }
        assert_eq!(
            receipt.runner_status().primary,
            aw_tools::filesystem::status::Primary::Fail
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn external_signal_identity_survives_into_shared_process_terminal() {
        async fn signalled(command: &str) -> (Outcome, RunnerExit) {
            let process = PipeProcess::spawn(spec(command)).await.unwrap();
            let receipt = process.wait(pending::<()>(), Duration::from_secs(5)).await;
            let terminal = receipt.runner_terminal();
            (receipt.outcome(), terminal.exit)
        }

        let killed = signalled("kill -KILL $$").await;
        let crashed = signalled("kill -SEGV $$").await;
        assert_eq!(killed, (Outcome::ExitedBySignal(9), RunnerExit::Signal(9)));
        assert_eq!(
            crashed,
            (Outcome::ExitedBySignal(11), RunnerExit::Signal(11))
        );
        assert_ne!(killed.1, crashed.1);
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
        assert!(!result.output_complete());
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
    #[cfg(unix)]
    #[tokio::test]
    async fn prepared_launches_preserve_required_containment_through_custody() {
        let launch = || {
            PreparedPipeLaunch::from_trusted_command(
                std::process::Command::new("/definitely/not/an/aw-process-command"),
                false,
                (),
            )
            .requiring_containment(ContainmentRequirement::ClosedDescendants)
            .with_custody(())
            .requiring_containment(ContainmentRequirement::BestAvailable)
        };
        let deadline = Instant::now() + Duration::from_secs(3);
        let result =
            PipeProcess::spawn_prepared_until(launch(), deadline, || false, pending()).await;
        assert!(matches!(
            result,
            Err(PipeLaunchFailure::NotStarted(crate::Error::Denied(
                "required closed descendant containment is unavailable"
            )))
        ));
        let result = PipeProcess::spawn_prepared_fenced_until(
            launch(),
            deadline,
            || false,
            pending(),
            |_, _| panic!("unsupported containment must not enter launch fence"),
        )
        .await;
        assert!(matches!(
            result,
            Err(PipeLaunchFailure::NotStarted(crate::Error::Denied(
                "required closed descendant containment is unavailable"
            )))
        ));
    }

    #[tokio::test]
    async fn process_spawns_drains_and_receipts_exit() {
        let p = PipeProcess::spawn(spec("printf hello; sleep 0.05; printf tail >&2"))
            .await
            .unwrap();
        let receipt = p.wait(pending::<()>(), Duration::from_secs(4)).await;
        assert_eq!(receipt.outcome(), Outcome::Exited(0));
        assert!(receipt.output_complete());
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
        assert_eq!(receipt.outcome(), Outcome::Exited(9));
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
            receipt.outcome(),
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
            receipt.outcome(),
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
        assert_eq!(receipt.outcome(), Outcome::Exited(0));
        assert!(receipt.output_complete());
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
                .outcome(),
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
        assert_eq!(receipt.outcome(), Outcome::Exited(0));
        assert_eq!(receipt.runner_terminal().occupancy, Occupancy::Live);
        assert!(receipt.output_complete());
        tokio::time::sleep(Duration::from_millis(1300)).await;
        assert!(
            !marker.exists(),
            "grandchild from ended Bash may not remain effectful"
        );
    }
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn inherited_output_pipe_is_closed_before_drain() {
        let resource = PipeProcess::spawn(spec("(sleep 3) & printf parent"))
            .await
            .unwrap();
        let receipt = resource.wait(pending::<()>(), Duration::from_secs(4)).await;
        assert_eq!(receipt.outcome(), Outcome::Exited(0));
        assert_eq!(receipt.runner_terminal().occupancy, Occupancy::Live);
        assert!(receipt.output_complete());
        assert!(receipt.finished());
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
