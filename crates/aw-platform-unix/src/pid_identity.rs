//! Stable Linux process identity for future PTY ownership transfer.
//! No implicit signal, child adoption, or execution authority.
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

/// Read the PID bound to this *existing* kernel pidfd in our PID namespace.
/// Validation uses waitid first; procfs is metadata, never a PID reopen or
/// signaling fallback. A dead/reaped identity cannot pass live admission.
pub fn live_pid(pidfd: BorrowedFd<'_>) -> io::Result<u32> {
    if inspect_exit(pidfd)? != ExitObservation::Running {
        return Err(rejected());
    }
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", pidfd.as_raw_fd()))?;
    let pid = info
        .lines()
        .find_map(|line| line.strip_prefix("Pid:\t"))
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|pid| *pid > 1)
        .ok_or_else(rejected)?;
    if exited(pidfd)? {
        return Err(rejected());
    }
    Ok(pid)
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

/// Factual wait status, available only to the kernel-designated parent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildExit {
    Code(u8),
    Signal { number: i32, core_dumped: bool },
}

/// A nonchild's readable pidfd proves exit, never its exit code or reaping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitObservation {
    Running,
    Exited { status: Option<ChildExit> },
}

/// Nonblocking observation without consuming the original parent's wait
/// status. SCM_RIGHTS does not change parentage. ECHILD alone is not exit.
/// No procfs/PID lookup, subreaper installation, or signal is performed.
pub fn inspect_exit(pidfd: BorrowedFd<'_>) -> io::Result<ExitObservation> {
    match wait_child(pidfd, libc::WNOWAIT) {
        Ok(Some(status)) => Ok(ExitObservation::Exited {
            status: Some(status),
        }),
        Ok(None) => Ok(ExitObservation::Running),
        Err(error) if error.raw_os_error() == Some(libc::ECHILD) => {
            if exited(pidfd)? {
                Ok(ExitObservation::Exited { status: None })
            } else {
                Ok(ExitObservation::Running)
            }
        }
        Err(error) => Err(error),
    }
}

/// Explicit nonblocking reap of this exact pidfd *only if* it is our child.
/// Caller must exclusively own wait responsibility; do not race ChildGuard
/// or another waiter. Nonchildren and previously reaped children return
/// ECHILD, never a synthetic success/status. No fallback to waitpid by PID.
pub fn reap_child(pidfd: BorrowedFd<'_>) -> io::Result<Option<ChildExit>> {
    wait_child(pidfd, 0)
}

fn wait_child(pidfd: BorrowedFd<'_>, flags: i32) -> io::Result<Option<ChildExit>> {
    loop {
        // SAFETY: all-zero siginfo_t is valid storage; waitid initializes it.
        // Zero si_pid distinguishes WNOHANG with no waitable child.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: valid writable siginfo_t, borrowed FD, no PID resolution.
        let result = unsafe {
            libc::waitid(
                libc::P_PIDFD,
                pidfd.as_raw_fd() as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | flags,
            )
        };
        if result != 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        // SAFETY: waitid wrote the SIGCHLD union fields (or left zero si_pid).
        let (pid, status) = unsafe { (info.si_pid(), info.si_status()) };
        if pid == 0 {
            return Ok(None);
        }
        if info.si_signo != libc::SIGCHLD {
            return Err(rejected());
        }
        return match info.si_code {
            libc::CLD_EXITED => u8::try_from(status)
                .map(ChildExit::Code)
                .map(Some)
                .map_err(|_| rejected()),
            libc::CLD_KILLED | libc::CLD_DUMPED if status > 0 => Ok(Some(ChildExit::Signal {
                number: status,
                core_dumped: info.si_code == libc::CLD_DUMPED,
            })),
            _ => Err(rejected()),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        os::fd::AsFd,
        process::{Child, Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    struct PrivateChild(Child);
    impl Drop for PrivateChild {
        fn drop(&mut self) {
            if matches!(self.0.try_wait(), Ok(None)) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }
    fn waiting_child() -> PrivateChild {
        PrivateChild(
            Command::new("/bin/sh")
                .args(["-c", "read ignored; exit 37"])
                .stdin(Stdio::piped())
                .spawn()
                .unwrap(),
        )
    }
    fn await_exit(identity: BorrowedFd<'_>) -> ExitObservation {
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            let observed = inspect_exit(identity).unwrap();
            if observed != ExitObservation::Running {
                return observed;
            }
            assert!(Instant::now() < until, "private child did not exit");
            thread::sleep(Duration::from_millis(3));
        }
    }

    #[test]
    fn inspection_preserves_parent_wait_status_and_reap_is_exact_and_once() {
        let mut target = waiting_child();
        let sentinel = waiting_child();
        let identity = open(target.0.id()).unwrap();
        let sentinel_identity = open(sentinel.0.id()).unwrap();
        assert_eq!(
            inspect_exit(identity.as_fd()).unwrap(),
            ExitObservation::Running
        );
        assert_eq!(reap_child(identity.as_fd()).unwrap(), None);
        target.0.stdin.take().unwrap().write_all(b"done\n").unwrap();
        let expected = ExitObservation::Exited {
            status: Some(ChildExit::Code(37)),
        };
        assert_eq!(await_exit(identity.as_fd()), expected);
        assert_eq!(inspect_exit(identity.as_fd()).unwrap(), expected);
        assert_eq!(
            reap_child(identity.as_fd()).unwrap(),
            Some(ChildExit::Code(37))
        );
        assert_eq!(
            reap_child(identity.as_fd()).unwrap_err().raw_os_error(),
            Some(libc::ECHILD)
        );
        assert_eq!(
            inspect_exit(identity.as_fd()).unwrap(),
            ExitObservation::Exited { status: None }
        );
        assert_eq!(
            inspect_exit(sentinel_identity.as_fd()).unwrap(),
            ExitObservation::Running
        );
    }

    #[test]
    fn signalled_child_keeps_factual_signal_and_inspection_does_not_reap() {
        let mut target = waiting_child();
        let identity = open(target.0.id()).unwrap();
        signal(identity.as_fd(), ProcessSignal::Terminate).unwrap();
        assert_eq!(
            await_exit(identity.as_fd()),
            ExitObservation::Exited {
                status: Some(ChildExit::Signal {
                    number: libc::SIGTERM,
                    core_dumped: false
                }),
            }
        );
        // The ordinary original parent still receives its own wait receipt.
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(target.0.wait().unwrap().signal(), Some(libc::SIGTERM));
        assert_eq!(
            inspect_exit(identity.as_fd()).unwrap(),
            ExitObservation::Exited { status: None }
        );
    }

    #[test]
    fn ordinary_file_cannot_be_used_as_a_process_lifecycle_identity() {
        let file = std::fs::File::open("/dev/null").unwrap();
        assert!(inspect_exit(file.as_fd()).is_err());
        assert!(live_pid(file.as_fd()).is_err());
        assert!(reap_child(file.as_fd()).is_err());
    }

    #[test]
    fn pidfd_signals_only_the_exact_private_child_and_never_resolves_a_reused_pid() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exec sleep 30"])
            .spawn()
            .expect("spawn private signal target");
        let identity = open(child.id()).expect("pidfd for original child");
        assert!(!exited(identity.as_fd()).unwrap());
        assert_eq!(live_pid(identity.as_fd()).unwrap(), child.id());
        signal(identity.as_fd(), ProcessSignal::Terminate).unwrap();
        let status = child.wait().unwrap();
        assert!(
            !status.success(),
            "SIGTERM must terminate the private target"
        );
        assert!(exited(identity.as_fd()).unwrap());
        assert!(live_pid(identity.as_fd()).is_err());
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
