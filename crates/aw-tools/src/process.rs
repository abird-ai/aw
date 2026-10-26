//! Host-owned process I/O service: no model tool, sandbox or second Agent.
//! A trusted caller must authenticate grants and journal Process intent
//! before spawn. Pipes and optional PTYs share lifecycle ownership.
use std::{
    collections::{BTreeMap, VecDeque},
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
    fn start<R: AsyncRead + Unpin + Send + 'static>(input: R) -> (Self, JoinHandle<()>) {
        let state = Arc::new(Mutex::new(CaptureState::default()));
        let shared = state.clone();
        let task = tokio::spawn(async move {
            let mut input = input;
            let mut buffer = [0u8; 4096];
            let failed = loop {
                match input.read(&mut buffer).await {
                    Ok(0) => break false,
                    Ok(n) => shared
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .append(&buffer[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break true,
                }
            };
            let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
            state.closed = true;
            state.read_failed = failed;
        });
        (Self { state }, task)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessSnapshot {
    pub pid: u32,
    pub stdout: StreamSnapshot,
    pub stderr: StreamSnapshot,
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
}
impl ProcessReceipt {
    pub fn finished(&self) -> bool {
        matches!(self.outcome, Outcome::Exited(_) | Outcome::ExitedBySignal) && self.output_complete
    }
}
pub struct PipeProcess {
    child: Child,
    pid: u32,
    stdin: Option<ChildStdin>,
    stdout: Reader,
    stderr: Reader,
    readers: Vec<JoinHandle<()>>,
    /// Fenced original OS termination attempt. A second attempt on this
    /// live host-owned PipeProcess is always refused, even after an unknown
    /// Process exit or a cancelled caller; never silently resend a signal.
    termination_attempted: bool,
    #[cfg(windows)]
    job: aw_platform::ManagedProcessGuard,
}
impl PipeProcess {
    pub async fn spawn(spec: PipeSpec) -> crate::Result<Self> {
        spec.validate()?;
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
        let pid = child
            .id()
            .ok_or(crate::Error::Protocol("process has no PID"))?;
        #[cfg(windows)]
        let job = aw_platform::ManagedProcessGuard::attach(
            pid,
            child.raw_handle().ok_or(crate::Error::Protocol(
                "managed child process has no Windows handle",
            ))?,
        )
        .map_err(crate::Error::Io)?;
        let stdout = child
            .stdout
            .take()
            .ok_or(crate::Error::Protocol("missing stdout pipe"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or(crate::Error::Protocol("missing stderr pipe"))?;
        let stdin = child.stdin.take();
        let (out, out_task) = Reader::start(stdout);
        let (err, err_task) = Reader::start(stderr);
        Ok(Self {
            child,
            pid,
            stdin,
            stdout: out,
            stderr: err,
            readers: vec![out_task, err_task],
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
        let mut complete = true;
        for task in &mut self.readers {
            match timeout(READER_DRAIN_DEADLINE, &mut *task).await {
                Ok(Ok(())) => {}
                _ => {
                    task.abort();
                    complete = false;
                }
            }
        }
        let s = self.snapshot();
        complete
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
