//! Bounded readiness observation for the original Unix PTY master.
//! The caller retains and owns the FD; polling never reads, duplicates,
//! closes or grants authority to the underlying process.
use std::{
    io,
    os::fd::{FromRawFd, OwnedFd, RawFd},
    time::{Duration, Instant},
};

/// Keep the readiness descriptor live as long as the reader thread exists.
/// An owning duplicate prevents `poll` from observing a reused original FD.
pub fn duplicate(fd: RawFd) -> io::Result<OwnedFd> {
    if fd < 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid PTY descriptor",
        ));
    }
    // SAFETY: `fd` is an existing owner-held descriptor. F_DUPFD_CLOEXEC
    // returns an independently owned descriptor or -1.
    let copied = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if copied < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fcntl returned a new descriptor owned uniquely by this call.
    Ok(unsafe { OwnedFd::from_raw_fd(copied) })
}

pub fn readable(fd: RawFd, timeout: Duration) -> io::Result<bool> {
    if fd < 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid PTY descriptor",
        ));
    }
    let until = Instant::now() + timeout.min(Duration::from_secs(1));
    loop {
        let remaining = until.saturating_duration_since(Instant::now());
        let milliseconds = remaining.as_millis().min(i32::MAX as u128) as i32;
        let mut event = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd struct, exactly one element, bounded wait.
        let outcome = unsafe { libc::poll(&mut event, 1, milliseconds) };
        if outcome > 0 {
            if event.revents & libc::POLLNVAL != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "closed PTY descriptor",
                ));
            }
            return Ok(event.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0);
        }
        if outcome == 0 {
            return Ok(false);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
        if Instant::now() >= until {
            return Ok(false);
        }
    }
}

/// Validate a Linux PTY master without consuming output or changing size.
#[cfg(target_os = "linux")]
pub fn number(fd: std::os::fd::BorrowedFd<'_>) -> io::Result<u32> {
    use std::os::fd::AsRawFd;
    let mut number: libc::c_uint = 0;
    // SAFETY: TIOCGPTN writes one unsigned integer into valid storage.
    if unsafe { libc::ioctl(fd.as_raw_fd(), libc::TIOCGPTN, &mut number) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(number)
}

/// Change only the size of an already authorized Linux PTY master.
#[cfg(target_os = "linux")]
pub fn resize(fd: std::os::fd::BorrowedFd<'_>, rows: u16, cols: u16) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: TIOCSWINSZ reads one initialized winsize; caller owns the FD.
    if unsafe { libc::ioctl(fd.as_raw_fd(), libc::TIOCSWINSZ, &size) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    #[test]
    fn duplicated_poll_descriptor_survives_original_close_without_leak() {
        use std::os::fd::AsFd;
        let (original, mut writer) = UnixStream::pair().unwrap();
        let retained = duplicate(original.as_fd().as_raw_fd()).unwrap();
        drop(original);
        writer.write_all(b"owned").unwrap();
        assert!(readable(retained.as_raw_fd(), Duration::from_millis(15)).unwrap());
        let mut reader = UnixStream::from(retained);
        let mut text = [0_u8; 5];
        use std::io::Read;
        reader.read_exact(&mut text).unwrap();
        assert_eq!(&text, b"owned");
    }

    #[test]
    fn poll_never_consumes_bytes_and_flags_closed_descriptor() {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        assert!(!readable(reader.as_raw_fd(), Duration::from_millis(1)).unwrap());
        writer.write_all(b"hello").unwrap();
        assert!(readable(reader.as_raw_fd(), Duration::from_millis(5)).unwrap());
        let mut received = [0; 5];
        use std::io::Read;
        reader.read_exact(&mut received).unwrap();
        assert_eq!(&received, b"hello");
        assert!(readable(-1, Duration::ZERO).is_err());
    }
}
