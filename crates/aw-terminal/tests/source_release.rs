//! Local source-custody primitive only; no production broker authentication.
#![cfg(target_os = "linux")]

use aw_terminal::{Options, SourceRelease, Status, Terminal};
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::{
        fd::{AsFd, AsRawFd, OwnedFd},
        unix::{
            fs::DirBuilderExt,
            net::{UnixListener, UnixStream},
        },
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const PROOF: [u8; 32] = [61; 32];
const SCRIPT: &str = "printf 'SOURCE_READY'; IFS= read -r word || exit 41; [ \"$word\" = accepted ] || exit 42; printf 'RECEIVED:%s PID:%s\\n' \"$word\" \"$$\"; IFS= read -r finish; exit 7";

fn options(script: &str) -> Options {
    let bash = std::env::var_os("AW_BASH")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/bin/bash".into());
    let mut options = Options::bash(std::env::temp_dir(), bash, "source-release-fixture");
    options.args = vec![
        "--noprofile".into(),
        "--norc".into(),
        "-c".into(),
        script.into(),
    ];
    options
}
fn until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "synthetic custody transition timed out"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn ready(terminal: &Terminal) {
    until(|| {
        terminal
            .snapshot(0)
            .cells
            .iter()
            .map(|cell| cell.text.as_str())
            .collect::<String>()
            .contains("SOURCE_READY")
    });
}
struct Escrow {
    master: File,
    pidfd: OwnedFd,
}
impl Escrow {
    fn receive(socket: &UnixStream) -> Self {
        let (master, pidfd) = aw_platform::fd_transfer::receive_pair(socket, &PROOF).unwrap();
        Self {
            master: File::from(master),
            pidfd,
        }
    }
    fn alive(&self) -> bool {
        !aw_platform::pid_identity::exited(self.pidfd.as_fd()).unwrap()
    }
    fn send(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).unwrap();
        self.master.flush().unwrap();
    }
    fn read_until(&mut self, needle: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        until(|| {
            if aw_platform::pty_poll::readable(self.master.as_raw_fd(), Duration::from_millis(20))
                .unwrap()
            {
                let mut chunk = [0; 4096];
                let n = self.master.read(&mut chunk).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
            }
            String::from_utf8_lossy(&bytes).contains(needle)
        });
        bytes
    }
}
impl Drop for Escrow {
    fn drop(&mut self) {
        // Synthetic fixture only; exact pidfd, no stale PID/group lookup.
        let _ = aw_platform::pid_identity::signal(
            self.pidfd.as_fd(),
            aw_platform::pid_identity::ProcessSignal::Kill,
        );
    }
}

#[test]
fn unsent_or_failed_escrow_cannot_disarm_original_guard() {
    for failed_send in [false, true] {
        let original = Terminal::spawn(options(SCRIPT), || {}).unwrap();
        ready(&original);
        let mut gate = original.quiesce(Duration::from_secs(2)).unwrap();
        let mut staged = gate.stage().unwrap();
        if failed_send {
            let (sender, receiver) = UnixStream::pair().unwrap();
            drop(receiver);
            assert!(staged.send_once(&sender, &PROOF).is_err());
        }
        assert!(staged.release_source().is_err());
        assert_eq!(original.source_release(), SourceRelease::Retained);
        drop(gate);
        original.input(b"accepted\n".to_vec()).unwrap();
        until(|| {
            original
                .snapshot(0)
                .cells
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .contains("RECEIVED:accepted")
        });
        original.terminate();
    }
}

#[test]
fn committed_source_release_injects_no_eof_and_old_handles_cannot_control() {
    let original = Terminal::spawn(options(SCRIPT), || {}).unwrap();
    ready(&original);
    let mut gate = original.quiesce(Duration::from_secs(2)).unwrap();
    let mut staged = gate.stage().unwrap();
    let epoch = staged.checkpoint().epoch();
    let (sender, receiver) = UnixStream::pair().unwrap();
    staged.send_once(&sender, &PROOF).unwrap();
    let mut escrow = Escrow::receive(&receiver);
    let receipt = staged
        .release_source()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap();
    assert_eq!(receipt.terminal_id(), original.id);
    assert_eq!(receipt.epoch(), epoch);
    drop(gate);
    assert_eq!(original.source_release(), SourceRelease::Released(epoch));
    assert_eq!(original.status(), Status::Released);
    assert!(original.input(b"WRONG\n".to_vec()).is_err());
    assert!(original.resize(18, 72).is_err());
    assert!(original.quiesce(Duration::from_millis(20)).is_err());
    original.terminate();
    drop(original);
    // portable-pty's old writer Drop would send a blank line/VEOF here,
    // causing this exact script to exit 41/42 before explicit acceptance.
    thread::sleep(Duration::from_millis(100));
    assert!(escrow.alive(), "source Drop/Stop must not send EOF or kill");
    escrow.send(b"accepted\n");
    escrow.read_until("RECEIVED:accepted PID:");
    escrow.send(b"finish\n");
    until(|| !escrow.alive());
    // Original source worker retains parent-only reap responsibility.
    until(|| {
        matches!(
            aw_platform::pid_identity::inspect_exit(escrow.pidfd.as_fd()).unwrap(),
            aw_platform::pid_identity::ExitObservation::Exited { status: None }
        )
    });
}

#[test]
fn lost_release_ack_and_guard_drop_never_resume_the_original_reader() {
    let original = Terminal::spawn(options(SCRIPT), || {}).unwrap();
    ready(&original);
    let mut gate = original.quiesce(Duration::from_secs(2)).unwrap();
    let mut staged = gate.stage().unwrap();
    let epoch = staged.checkpoint().epoch();
    let (sender, receiver) = UnixStream::pair().unwrap();
    staged.send_once(&sender, &PROOF).unwrap();
    let mut escrow = Escrow::receive(&receiver);
    let pending = staged.release_source().unwrap();
    drop(pending); // lose the ACK without cancelling or retrying the release
    drop(gate);
    original.terminate(); // also fenced while release is merely pending
    until(|| original.source_release() == SourceRelease::Released(epoch));
    assert!(original.input(b"MUST_NOT_REPLAY\n".to_vec()).is_err());
    let snapshot_version = original.snapshot(0).version;
    escrow.send(b"accepted\n");
    escrow.read_until("RECEIVED:accepted PID:");
    assert_eq!(
        original.snapshot(0).version,
        snapshot_version,
        "source reader must remain retired after lost ACK"
    );
    assert!(escrow.alive());
    escrow.send(b"finish\n");
    until(|| !escrow.alive());
}

#[test]
fn stop_before_release_preserves_original_termination_responsibility() {
    let original = Terminal::spawn(options(SCRIPT), || {}).unwrap();
    ready(&original);
    let mut gate = original.quiesce(Duration::from_secs(2)).unwrap();
    let mut staged = gate.stage().unwrap();
    let (sender, receiver) = UnixStream::pair().unwrap();
    staged.send_once(&sender, &PROOF).unwrap();
    let escrow = Escrow::receive(&receiver);
    original.terminate();
    assert!(staged.release_source().is_err());
    drop(gate);
    until(|| !escrow.alive());
    assert_eq!(original.source_release(), SourceRelease::Retained);
}

#[test]
fn observer_panic_after_release_cannot_kill_shell_or_strand_parent_reaping() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let panic_on_release = Arc::new(AtomicBool::new(false));
    let observer = panic_on_release.clone();
    let original = Terminal::spawn(options(SCRIPT), move || {
        assert!(
            !observer.load(Ordering::Acquire),
            "synthetic observer fault"
        );
    })
    .unwrap();
    ready(&original);
    let mut gate = original.quiesce(Duration::from_secs(2)).unwrap();
    let mut staged = gate.stage().unwrap();
    let (sender, receiver) = UnixStream::pair().unwrap();
    staged.send_once(&sender, &PROOF).unwrap();
    let mut escrow = Escrow::receive(&receiver);
    panic_on_release.store(true, Ordering::Release);
    staged
        .release_source()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap();
    drop(gate);
    drop(original);
    escrow.send(b"accepted\n");
    escrow.read_until("RECEIVED:accepted PID:");
    escrow.send(b"finish\n");
    until(|| {
        matches!(
            aw_platform::pid_identity::inspect_exit(escrow.pidfd.as_fd()).unwrap(),
            aw_platform::pid_identity::ExitObservation::Exited { status: None }
        )
    });
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "aw-source-release-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct SourceProcess(Child);
impl Drop for SourceProcess {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[test]
fn independent_source_release_fixture() {
    let Some(root) = std::env::var_os("AW_TEST_SOURCE_RELEASE_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let script = format!("printf '%s' \"$$\" > shell.pid; printf '\\033[?1049h\\033[1;31m\\033[?2004hSOURCE_READY_界'; {SCRIPT}");
    let mut spec = options(&script);
    spec.root = root.clone();
    let original = Terminal::spawn(spec, || {}).unwrap();
    ready(&original);
    let mut socket = UnixStream::connect(root.join("source.sock")).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut gate = original.quiesce(Duration::from_secs(2)).unwrap();
    let mut staged = gate.stage().unwrap();
    staged.send_once(&socket, &PROOF).unwrap();
    let wire = staged.checkpoint().to_wire().unwrap();
    socket
        .write_all(&(wire.len() as u32).to_be_bytes())
        .unwrap();
    socket.write_all(&wire).unwrap();
    let mut ready = [0];
    socket.read_exact(&mut ready).unwrap();
    assert_eq!(&ready, b"R"); // trusted synthetic owner's independent readiness
    let receipt = staged
        .release_source()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap();
    assert_eq!(receipt.terminal_id(), original.id);
    drop(gate);
    drop(original); // exercises the actual Terminal Drop after disarm
    socket.write_all(b"RELEASED").unwrap();
    // Actual process exit also destroys the passive parent waiter thread.
    std::process::exit(0);
}

#[test]
fn original_shell_survives_actual_source_exit_after_guarded_release() {
    let fixture = Fixture::new();
    let listener = UnixListener::bind(fixture.0.join("source.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut source = SourceProcess(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "independent_source_release_fixture",
                "--nocapture",
            ])
            .env("AW_TEST_SOURCE_RELEASE_ROOT", &fixture.0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut connected = None;
    until(|| match listener.accept() {
        Ok((socket, _)) => {
            connected = Some(socket);
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
        Err(error) => panic!("source socket: {error}"),
    });
    let mut socket = connected.unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut escrow = Escrow::receive(&socket);
    let mut length = [0; 4];
    socket.read_exact(&mut length).unwrap();
    let length = u32::from_be_bytes(length) as usize;
    assert!(length <= aw_terminal::MAX_CHECKPOINT_WIRE_BYTES);
    let mut wire = vec![0; length];
    socket.read_exact(&mut wire).unwrap();
    let checkpoint = aw_terminal::Checkpoint::from_wire(&wire).unwrap();
    let mut replayed = checkpoint.replay().unwrap();
    assert!(replayed.screen().alternate_screen());
    assert!(replayed.screen().bracketed_paste());
    assert!(replayed.screen().contents().contains("SOURCE_READY_界"));
    let shell_pid = fs::read_to_string(fixture.0.join("shell.pid")).unwrap();
    socket.write_all(b"R").unwrap();
    let mut receipt = [0; 8];
    socket.read_exact(&mut receipt).unwrap();
    assert_eq!(&receipt, b"RELEASED");
    until(|| source.0.try_wait().unwrap().is_some());
    assert!(source.0.wait().unwrap().success());
    assert!(escrow.alive());
    escrow.send(b"accepted\n");
    let bytes = escrow.read_until(&format!("RECEIVED:accepted PID:{shell_pid}"));
    // This recipient starts reading only after the source release receipt.
    let replies = replayed.process_live(&bytes);
    assert!(replies.is_empty());
    assert!(replayed.screen().alternate_screen());
    assert!(replayed.screen().bracketed_paste());
    assert!(replayed.screen().contents().contains("SOURCE_READY_界"));
    escrow.send(b"finish\n");
    until(|| !escrow.alive());
    assert_eq!(
        aw_platform::pid_identity::inspect_exit(escrow.pidfd.as_fd()).unwrap(),
        aw_platform::pid_identity::ExitObservation::Exited { status: None }
    );
}
