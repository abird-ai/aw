//! Linux anonymous, child-only read capabilities. No filesystem name,
//! listener, helper Process, or second lifetime owner.
use std::{
    fs::File,
    io::{self, Seek, SeekFrom, Write},
    net::Shutdown,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{net::UnixStream, process::CommandExt},
    },
    process::Command,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadCapabilityKind {
    SealedFile,
    OneShotSocket,
}

fn sealed_file(bytes: &[u8]) -> io::Result<OwnedFd> {
    // SAFETY: static NUL-terminated name; new owned descriptor on success.
    // Non-executable file mode, sealed against executable mode changes;
    // no filesystem pathname.
    let fd = unsafe {
        libc::memfd_create(
            c"aw-process-input".as_ptr(),
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING | libc::MFD_NOEXEC_SEAL,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful memfd_create returned a fresh owned fd.
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(bytes)?;
    file.seek(SeekFrom::Start(0))?;
    let seals = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
    // SAFETY: valid exclusively owned file. No mappings were created.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, seals) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(file.into())
}

fn one_shot_socket(bytes: &[u8]) -> io::Result<OwnedFd> {
    let (mut producer, consumer) = UnixStream::pair()?;
    // Never block the shared spawn lane waiting for an unlaunched reader.
    producer.set_nonblocking(true)?;
    consumer.shutdown(Shutdown::Write)?;
    producer.write_all(bytes)?;
    producer.shutdown(Shutdown::Write)?;
    // No live host endpoint survives preparation. Read queued bytes then
    // EOF; this is not a general request/response signing service.
    drop(producer);
    Ok(consumer.into())
}

/// Install one anonymous capability in the actual child at exec. The
/// returned locator is either /proc/self/fd/N (file) or decimal N (socket),
/// not an arbitrary mount or filesystem path. The caller must use a
/// reviewed consumer, provide the required procfs view for files, exclude
/// unauthorized readers, and prove permanent descendant quiescence.
/// The Command owns its parent descriptor until it is dropped; the parent
/// descriptor always remains close-on-exec, including concurrent spawns.
/// Only the exact child copy becomes inheritable in pre_exec.
pub fn install_child_read_capability(
    command: &mut Command,
    kind: ReadCapabilityKind,
    bytes: &[u8],
) -> io::Result<String> {
    if bytes.is_empty() || bytes.len() > 64 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid Process capability size",
        ));
    }
    let original = match kind {
        ReadCapabilityKind::SealedFile => sealed_file(bytes)?,
        ReadCapabilityKind::OneShotSocket => one_shot_socket(bytes)?,
    };
    let fd = install_child_fd(command, original)?;
    Ok(match kind {
        ReadCapabilityKind::SealedFile => format!("/proc/self/fd/{fd}"),
        ReadCapabilityKind::OneShotSocket => fd.to_string(),
    })
}

/// Connected anonymous request/reply socket. Only the exact child gets
/// the peer descriptor; the host endpoint is nonblocking and CLOEXEC.
pub fn install_child_request_socket(command: &mut Command) -> io::Result<(String, UnixStream)> {
    let (host, child) = UnixStream::pair()?;
    host.set_nonblocking(true)?;
    let fd = install_child_fd(command, child.into())?;
    Ok((fd.to_string(), host))
}

fn install_child_fd(command: &mut Command, original: OwnedFd) -> io::Result<i32> {
    // Ensure it cannot collide with stdin/out/err even in a host with
    // closed standard descriptors. The kernel selects an unoccupied fd.
    // SAFETY: original is a live owned descriptor, dup creates a new one.
    let fd = unsafe { libc::fcntl(original.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful dup returned a fresh owned descriptor.
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    drop(original);
    // SAFETY: capture the actual owned descriptor for the command's
    // lifetime. Only async-signal-safe fcntl calls in the forked child;
    // no allocation, locks, environment mutation, or parent FD mutation.
    unsafe {
        command.pre_exec(move || {
            let fd = owned.as_raw_fd();
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(fd)
}
