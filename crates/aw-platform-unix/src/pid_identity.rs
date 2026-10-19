//! Stable Linux process identity for future PTY ownership transfer.
//! No implicit signal, child adoption, wait/reap, or execution authority.
use std::{
    io,
    os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd},
};

fn rejected() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "invalid process identity")
}

/// Kernel identity is an owned, close-on-exec pidfd, not a reusable PID.
/// Caller must still verify that the target was its *own* running child at
/// acquisition; pidfd_open alone does not establish custody.
pub fn open(pid: u32) -> io::Result<OwnedFd> {
    if pid <= 1 || pid > i32::MAX as u32 {
        return Err(rejected());
    }
    // SAFETY: syscall takes a scalar process ID and zero flags. Kernel
    // returns a fresh FD or -1. No user pointers are passed.
    let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0_u32) };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: pidfd_open returned a unique descriptor owned by this process.
    Ok(unsafe { OwnedFd::from_raw_fd(raw as i32) })
}

/// Nonblocking process-exit observation; does not reap a process belonging
/// to another parent or infer exit based on /proc pathname/PID lookup.
pub fn exited(pidfd: BorrowedFd<'_>) -> io::Result<bool> {
    let mut pfd = libc::pollfd {
        fd: pidfd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        // SAFETY: one initialized pollfd and an exact element count.
        let n = unsafe { libc::poll(&mut pfd, 1, 0) };
        if n >= 0 {
            if n == 0 {
                return Ok(false);
            }
            if pfd.revents & libc::POLLNVAL != 0 {
                return Err(rejected());
            }
            return Ok(pfd.revents & (libc::POLLIN | libc::POLLHUP) != 0);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

/// Stable process-directed signals for a future adopted-PTY custodian.
/// This does NOT signal a process group, reap a child, or authorize custody.
/// Callers must independently verify the owner/Actor and migration commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessSignal {
    Terminate,
    Kill,
}

/// Signal the precise kernel process identity, never a PID resolved again by
/// number. No fallback to kill(2) is permitted on unsupported kernels.
pub fn signal(pidfd: BorrowedFd<'_>, signal: ProcessSignal) -> io::Result<()> {
    let signo = match signal {
        ProcessSignal::Terminate => libc::SIGTERM,
        ProcessSignal::Kill => libc::SIGKILL,
    };
    loop {
        // SAFETY: pidfd is an owned Linux descriptor borrowed for the call,
        // siginfo pointer is null, flags zero. No raw process IDs involved.
        let result = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                pidfd.as_raw_fd(),
                signo,
                std::ptr::null::<libc::siginfo_t>(),
                0_u32,
            )
        };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::fd::AsFd, process::Command, thread, time::Duration};

    #[test]
    fn pidfd_signals_only_the_exact_private_child_and_never_resolves_a_reused_pid() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exec sleep 30"])
            .spawn()
            .expect("spawn private signal target");
        let identity = open(child.id()).expect("pidfd for original child");
        assert!(!exited(identity.as_fd()).unwrap());
        signal(identity.as_fd(), ProcessSignal::Terminate).unwrap();
        let status = child.wait().unwrap();
        assert!(
            !status.success(),
            "SIGTERM must terminate the private target"
        );
        assert!(exited(identity.as_fd()).unwrap());
        let repeat = signal(identity.as_fd(), ProcessSignal::Kill);
        assert!(
            repeat.is_err(),
            "stale pidfd cannot signal a different process"
        );
    }

    #[test]
    fn pidfd_tracks_exact_synthetic_child_and_cannot_resolve_bad_ids() {
        assert!(open(0).is_err());
        assert!(open(1).is_err());
        assert!(open(u32::MAX).is_err());
        let mut child = Command::new("/bin/sh")
            .args(["-c", "sleep 0.1"])
            .spawn()
            .expect("private harmless child");
        let identity = open(child.id()).expect("kernel supports pidfd on Linux");
        assert!(
            !exited(identity.as_fd()).unwrap(),
            "live child is not exited"
        );
        assert!(child.wait().unwrap().success());
        for _ in 0..100 {
            if exited(identity.as_fd()).unwrap() {
                return;
            }
            thread::sleep(Duration::from_millis(3));
        }
        panic!("kernel pidfd did not report exact child exit");
    }
}
