#![cfg(target_os = "linux")]
//! Kernel proof, not an enabled Workspace PTY migration API. A live child
//! continues with the same PID after its original process exits, provided
//! the recipient receives the original PTY master FD before owner exit.
use aw_platform_unix::fd_transfer::{receive_one, receive_pair, send_one, send_pair};
use aw_platform_unix::pid_identity;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::{
    fs::{self, DirBuilder, File},
    io::{Read, Write},
    os::{
        fd::AsFd,
        unix::{
            fs::DirBuilderExt,
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const PROOF: [u8; 32] = [0x79; 32];

/// Test-only cleanup: kill only a shell whose current cwd still identifies
/// this exact randomly created fixture, never by an unverified stale PID.
struct PrivateFixture(PathBuf);
impl Drop for PrivateFixture {
    fn drop(&mut self) {
        if let Ok(pid) = fs::read_to_string(self.0.join("before.pid")) {
            if let Ok(pid) = pid.trim().parse::<i32>() {
                let cwd = PathBuf::from(format!("/proc/{pid}/cwd"));
                if fs::read_link(cwd).ok().as_deref() == Some(self.0.as_path()) {
                    // SAFETY: process-group identity is tied to the live
                    // fixture-owned cwd immediately before signaling.
                    unsafe {
                        libc::kill(-pid, libc::SIGTERM);
                    }
                }
            }
        }
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct OwnerGuard(std::process::Child);
impl Drop for OwnerGuard {
    fn drop(&mut self) {
        if self.0.try_wait().is_ok_and(|status| status.is_none()) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn fixture_dir() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "aw-pty-kernel-test-{}-{suffix}",
        std::process::id()
    ));
    DirBuilder::new().mode(0o700).create(&path).unwrap();
    path
}
fn wait_for_file(path: &Path, timeout: Duration) -> String {
    let until = Instant::now() + timeout;
    loop {
        if let Ok(contents) = fs::read_to_string(path) {
            if !contents.trim().is_empty() {
                return contents.trim().into();
            }
        }
        assert!(Instant::now() < until, "private Bash marker never arrived");
        thread::sleep(Duration::from_millis(25));
    }
}

/// Executed only in an isolated libtest subprocess. Exiting without running
/// destructors intentionally exercises the real kernel orphan transition.
#[test]
fn owner_process_fixture() {
    if std::env::var_os("AW_PTY_FD_HANDOFF_OWNER_TEST").is_none() {
        return;
    }
    let root = PathBuf::from(std::env::var_os("AW_PTY_FD_HANDOFF_ROOT_TEST").unwrap());
    let endpoint = PathBuf::from(std::env::var_os("AW_PTY_FD_HANDOFF_SOCKET_TEST").unwrap());
    let bash = PathBuf::from(std::env::var_os("AW_PTY_FD_HANDOFF_BASH_TEST").unwrap());

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(bash);
    command.cwd(&root);
    command.args(["--noprofile", "--norc", "-i"]);
    command.env_clear();
    command.env("HOME", &root);
    command.env("TERM", "xterm-256color");
    let child = pair.slave.spawn_command(command).unwrap();
    let pid = child.process_id().unwrap();
    assert!(pid > 1);
    drop(pair.slave);
    let mut writer = pair.master.take_writer().unwrap();
    writer
        .write_all(b"printf '%s' \"$$\" > before.pid\n")
        .unwrap();
    writer.flush().unwrap();
    assert_eq!(
        wait_for_file(&root.join("before.pid"), Duration::from_secs(5)),
        pid.to_string()
    );

    let mut stream = UnixStream::connect(endpoint).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let fd = pair
        .master
        .as_raw_fd()
        .expect("UnixMasterPty exposes its OS FD");
    // SAFETY: pair.master owns fd for at least the duration of send_one.
    let borrow = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
    if std::env::var_os("AW_PTY_FD_HANDOFF_PIDFD_TEST").is_some() {
        // The kernel transfers master + stable child identity as one
        // message. No partial receipt may activate either resource.
        let identity = pid_identity::open(pid).unwrap();
        send_pair(&stream, borrow, identity.as_fd(), &PROOF).unwrap();
    } else {
        send_one(&stream, borrow, &PROOF).unwrap();
    }
    let mut acknowledgement = [0_u8; 1];
    stream.read_exact(&mut acknowledgement).unwrap();
    if acknowledgement == *b"N" {
        // A denied handoff must leave original input and process control
        // usable; no recipient may secretly assume PTY ownership.
        writer
            .write_all(b"printf '%s' \"$$\" > declined.pid\n")
            .unwrap();
        writer.flush().unwrap();
        assert_eq!(
            wait_for_file(&root.join("declined.pid"), Duration::from_secs(4)),
            pid.to_string()
        );
        writer.write_all(b"exit\n").unwrap();
        writer.flush().unwrap();
        std::process::exit(0);
    }
    assert_eq!(acknowledgement, *b"A");
    // No further reads or writes, and no child/PTY destructor after receipt.
    // This subprocess is the former owner; the next process holds the master.
    std::process::exit(0);
}

fn probe_handoff(approve: bool, with_pidfd: bool) {
    let root = fixture_dir();
    let _cleanup = PrivateFixture(root.clone());
    let endpoint = root.join("owner.sock");
    let listener = UnixListener::bind(&endpoint).unwrap();
    listener.set_nonblocking(true).unwrap();
    let exe = std::env::current_exe().unwrap();
    let bash = std::env::var_os("AW_BASH")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("PATH").and_then(|path| {
                std::env::split_paths(&path)
                    .map(|directory| directory.join("bash"))
                    .find(|candidate| candidate.is_file())
            })
        })
        .expect("absolute Bash executable in AW_BASH or PATH");
    assert!(bash.is_absolute() && bash.is_file());
    let mut owner_command = Command::new(exe);
    owner_command
        .args(["--exact", "owner_process_fixture", "--nocapture"])
        .env("AW_PTY_FD_HANDOFF_OWNER_TEST", "1")
        .env("AW_PTY_FD_HANDOFF_ROOT_TEST", &root)
        .env("AW_PTY_FD_HANDOFF_SOCKET_TEST", &endpoint)
        .env("AW_PTY_FD_HANDOFF_BASH_TEST", bash)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if with_pidfd {
        owner_command.env("AW_PTY_FD_HANDOFF_PIDFD_TEST", "1");
    }
    let mut owner = OwnerGuard(owner_command.spawn().unwrap());
    let until = Instant::now() + Duration::from_secs(6);
    let (mut socket, _) = loop {
        match listener.accept() {
            Ok(peer) => break peer,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < until =>
            {
                thread::sleep(Duration::from_millis(15))
            }
            Err(error) => panic!("original owner never sent master: {error}"),
        }
    };
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let (owned_master, stable_process) = if with_pidfd {
        let (master, pidfd) = receive_pair(&socket, &PROOF).unwrap();
        (master, Some(pidfd))
    } else {
        (receive_one(&socket, &PROOF).unwrap(), None)
    };
    let pre = wait_for_file(&root.join("before.pid"), Duration::from_secs(3));
    socket.write_all(if approve { b"A" } else { b"N" }).unwrap();
    let exit = owner.0.wait().unwrap();
    assert!(exit.success(), "original owner failed to settle its PTY");
    if approve {
        // Original owner exited completely; adopted exact kernel master
        // still controls the *same* already-running Bash.
        if let Some(identity) = &stable_process {
            assert!(
                !pid_identity::exited(identity.as_fd()).unwrap(),
                "original shell must still be alive after the source exits"
            );
        }
        let mut master = File::from(owned_master);
        master
            .write_all(b"printf '%s' \"$$\" > after.pid\n")
            .unwrap();
        master.flush().unwrap();
        let post = wait_for_file(&root.join("after.pid"), Duration::from_secs(5));
        assert_eq!(pre, post, "FD transfer must not create another shell");
        master.write_all(b"exit\n").unwrap();
        master.flush().unwrap();
        if let Some(identity) = &stable_process {
            let until = Instant::now() + Duration::from_secs(5);
            while !pid_identity::exited(identity.as_fd()).unwrap() {
                assert!(
                    Instant::now() < until,
                    "pidfd did not observe original shell exit"
                );
                thread::sleep(Duration::from_millis(20));
            }
        }
    } else {
        // Denied admission: receiver closes the FD and the old controller
        // has proved it could still issue a command after the offer.
        drop(owned_master);
        let post = wait_for_file(&root.join("declined.pid"), Duration::from_secs(4));
        assert_eq!(pre, post, "denial must preserve source input ownership");
    }
    drop(socket);
    drop(listener);
    // Bash may exit slightly later than its former owner; close the private
    // fixture only after it has stopped changing marker files.
    for _ in 0..25 {
        if fs::remove_dir_all(&root).is_ok() {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    assert!(!root.exists(), "private test directory not cleaned");
}

#[test]
fn kernel_master_fd_survives_original_process_without_restarting_bash() {
    probe_handoff(true, false);
}

#[test]
fn rejected_transfer_preserves_original_shell_controller() {
    probe_handoff(false, false);
}

#[test]
fn transferred_pidfd_tracks_exact_running_bash_after_owner_exits() {
    probe_handoff(true, true);
}
