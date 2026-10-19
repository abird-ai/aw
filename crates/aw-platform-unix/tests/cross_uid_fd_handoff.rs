#![cfg(target_os = "linux")]
//! Explicit namespace gate: no host credentials or privilege changes.
use aw_platform_unix::{fd_transfer, pid_identity};
use portable_pty::{native_pty_system, PtySize};
use std::{
    io::{self, Read, Write},
    os::{
        fd::{AsFd, BorrowedFd},
        linux::net::SocketAddrExt,
        unix::net::{SocketAddr, UnixListener, UnixStream},
    },
    process::{Child, Command},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const PROOF: [u8; 32] = [0x43; 32];
const CHILD: &str = "AW_CROSS_UID_PROBE";

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().is_ok_and(|status| status.is_none()) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn denied<T: std::fmt::Debug>(result: io::Result<T>) {
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
}

fn reject_all(socket: &UnixStream) {
    let pty = native_pty_system().openpty(PtySize::default()).unwrap();
    let pidfd = pid_identity::open(std::process::id()).unwrap();
    // SAFETY: the master remains owned by pty throughout every call.
    let master = unsafe { BorrowedFd::borrow_raw(pty.master.as_raw_fd().unwrap()) };
    denied(fd_transfer::peer_pid(socket));
    denied(fd_transfer::send_one(socket, master, &PROOF));
    denied(fd_transfer::send_pair(
        socket,
        master,
        pidfd.as_fd(),
        &PROOF,
    ));
    // A denied receive must not consume application bytes, and denied sends
    // must not emit a descriptor-bearing message ahead of this byte.
    let mut stream = socket;
    stream.write_all(b"X").unwrap();
    denied(fd_transfer::receive_one(socket, &PROOF));
    denied(fd_transfer::receive_pair(socket, &PROOF));
    let mut marker = [0];
    stream.read_exact(&mut marker).unwrap();
    assert_eq!(marker, *b"X");
    assert_eq!(
        pid_identity::live_pid(pidfd.as_fd()).unwrap(),
        std::process::id()
    );
    assert!(pty.master.get_size().is_ok());
}

#[test]
#[ignore = "run explicitly inside a user namespace mapping UID 0 and 1"]
fn different_uid_is_rejected_before_descriptor_io() {
    // Require mapped identities, never initial-namespace root. The test's
    // only credential change happens in its disposable child process.
    let map = std::fs::read_to_string("/proc/self/uid_map").unwrap();
    let ranges: Vec<Vec<u64>> = map
        .lines()
        .map(|line| {
            line.split_whitespace()
                .map(|n| n.parse().unwrap())
                .collect()
        })
        .collect();
    assert!(ranges.iter().any(|r| r[0] == 0 && r[1] != 0));
    assert!(ranges.iter().any(|r| r[0] <= 1 && 1 < r[0] + r[2]));
    // SAFETY: geteuid has no arguments or memory effects.
    assert_eq!(unsafe { libc::geteuid() }, 0);

    if let Some(name) = std::env::var_os(CHILD) {
        // SAFETY: isolated test subprocess, mapped UID 1; no shared host
        // credential, configuration or persistent account is changed.
        assert_eq!(unsafe { libc::setresuid(1, 1, 1) }, 0);
        let address = SocketAddr::from_abstract_name(name.as_encoded_bytes()).unwrap();
        let socket = UnixStream::connect_addr(&address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        reject_all(&socket);
        return;
    }

    let name = format!(
        "aw-cross-uid-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let address = SocketAddr::from_abstract_name(name.as_bytes()).unwrap();
    let listener = UnixListener::bind_addr(&address).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut child = ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "different_uid_is_rejected_before_descriptor_io",
                "--nocapture",
            ])
            .env(CHILD, &name)
            .spawn()
            .unwrap(),
    );
    let until = std::time::Instant::now() + Duration::from_secs(5);
    let socket = loop {
        match listener.accept() {
            Ok((socket, _)) => break socket,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                assert!(
                    child.0.try_wait().unwrap().is_none(),
                    "UID child exited before connecting"
                );
                assert!(
                    std::time::Instant::now() < until,
                    "UID child did not connect"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("accept: {error}"),
        }
    };
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    reject_all(&socket);
    assert!(child.0.wait().unwrap().success());
}
