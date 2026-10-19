//! Linux-only, single FD transfer between locally authenticated processes.
//! The caller must independently fence the original PTY reader and child
//! ownership. Passing an FD never authorizes command replay or termination.
use std::{
    io,
    mem::{size_of, MaybeUninit},
    os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd},
    os::unix::net::UnixStream,
};

use libc::{self, c_void};

const WIRE_VERSION: u8 = 1;
const WIRE_LEN: usize = 33;
const MAX_RIGHTS: usize = 4;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "local FD handoff proof or principal mismatch",
    )
}

/// Unix peer identity must be validated on both sending and receiving ends.
/// A pathname, process ID, or inherited environment variable is not authority.
pub fn peer_pid(socket: &UnixStream) -> io::Result<u32> {
    let mut creds = MaybeUninit::<libc::ucred>::zeroed();
    let mut size = size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: initialized socklen_t; kernel writes ucred into valid storage.
    let rc = unsafe {
        libc::getsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            creds.as_mut_ptr().cast::<c_void>(),
            &mut size,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    if size as usize != size_of::<libc::ucred>() {
        return Err(denied());
    }
    // SAFETY: getsockopt succeeded and confirmed exact output size.
    let creds = unsafe { creds.assume_init() };
    // SAFETY: geteuid takes no arguments.
    if creds.uid != unsafe { libc::geteuid() } {
        return Err(denied());
    }
    u32::try_from(creds.pid).map_err(|_| denied())
}

fn validate_peer(socket: &UnixStream) -> io::Result<()> {
    peer_pid(socket).map(|_| ())
}

/// Send one FD with an explicit 256-bit one-use correlation proof.
/// Does not consume the source FD; caller must await owner-approved receipt
/// before relinquishing its original reader/process lifecycle.
pub fn send_one(socket: &UnixStream, fd: BorrowedFd<'_>, proof: &[u8; 32]) -> io::Result<()> {
    send_exact(socket, &[fd], proof)
}

/// Atomic master + process-identity escrow. Both descriptors are carried by
/// one authenticated SCM_RIGHTS message, never two partially admitted sends.
/// The recipient may *not* activate either without the owner's final receipt.
pub fn send_pair(
    socket: &UnixStream,
    master: BorrowedFd<'_>,
    pidfd: BorrowedFd<'_>,
    proof: &[u8; 32],
) -> io::Result<()> {
    send_exact(socket, &[master, pidfd], proof)
}

fn send_exact(socket: &UnixStream, fds: &[BorrowedFd<'_>], proof: &[u8; 32]) -> io::Result<()> {
    validate_peer(socket)?;
    if !matches!(fds.len(), 1 | 2) {
        return Err(invalid("unsupported FD transfer count"));
    }
    let mut payload = [0_u8; WIRE_LEN];
    payload[0] = WIRE_VERSION + (fds.len() as u8 - 1);
    payload[1..].copy_from_slice(proof);
    let mut iov = libc::iovec {
        iov_base: payload.as_mut_ptr().cast::<c_void>(),
        iov_len: payload.len(),
    };
    let bytes = (fds.len() * size_of::<RawFd>()) as u32;
    let ctrl_size = unsafe { libc::CMSG_SPACE(bytes) as usize };
    let mut control = vec![0_usize; ctrl_size.div_ceil(size_of::<usize>())];
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast::<c_void>();
    // Bounded sizes fit musl socklen_t and glibc size_t.
    msg.msg_controllen = ctrl_size as _;
    // SAFETY: aligned control allocation and valid msghdr/iovec; exactly
    // one or two owned source FDs copied as SCM_RIGHTS, not dereferenced.
    unsafe {
        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        if cmsg.is_null() {
            return Err(invalid("missing SCM_RIGHTS control space"));
        }
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = libc::CMSG_LEN(bytes) as _;
        for (index, fd) in fds.iter().enumerate() {
            (libc::CMSG_DATA(cmsg) as *mut RawFd)
                .add(index)
                .write_unaligned(fd.as_raw_fd());
        }
    }
    loop {
        // SAFETY: valid fd and msghdr. MSG_NOSIGNAL avoids SIGPIPE; a
        // short/unknown send never constitutes authorization to release.
        let n = unsafe { libc::sendmsg(socket.as_raw_fd(), &msg, libc::MSG_NOSIGNAL) };
        if n < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if n as usize != payload.len() {
            return Err(invalid("incomplete descriptor transfer; receipt required"));
        }
        return Ok(());
    }
}

/// Receive exactly one owned descriptor. On malformed messages, wrong proofs,
/// ancillary truncation, or multiple FDs, close **every** received descriptor.
pub fn receive_one(socket: &UnixStream, proof: &[u8; 32]) -> io::Result<OwnedFd> {
    Ok(receive_exact(socket, proof, 1)?
        .pop()
        .expect("exactly one checked descriptor"))
}

/// Receive master and stable child pidfd as one kernel-atomic message.
/// Each returned handle is close-on-exec and closes on validation failure.
pub fn receive_pair(socket: &UnixStream, proof: &[u8; 32]) -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = receive_exact(socket, proof, 2)?;
    let identity = fds.pop().expect("checked pidfd");
    let master = fds.pop().expect("checked master FD");
    Ok((master, identity))
}

fn receive_exact(
    socket: &UnixStream,
    proof: &[u8; 32],
    expected: usize,
) -> io::Result<Vec<OwnedFd>> {
    validate_peer(socket)?;
    if !matches!(expected, 1 | 2) {
        return Err(invalid("unsupported FD transfer count"));
    }
    let mut payload = [0_u8; WIRE_LEN];
    let mut iov = libc::iovec {
        iov_base: payload.as_mut_ptr().cast::<c_void>(),
        iov_len: payload.len(),
    };
    let ctrl_size = unsafe { libc::CMSG_SPACE((MAX_RIGHTS * size_of::<RawFd>()) as u32) as usize };
    let mut control = vec![0_usize; ctrl_size.div_ceil(size_of::<usize>())];
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast::<c_void>();
    msg.msg_controllen = ctrl_size as _;
    // SAFETY: socket, iovec and ancillary storage are valid for recvmsg;
    // MSG_CMSG_CLOEXEC protects both descriptors from exec inheritance.
    let n = unsafe {
        libc::recvmsg(
            socket.as_raw_fd(),
            &mut msg,
            libc::MSG_WAITALL | libc::MSG_CMSG_CLOEXEC,
        )
    };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut received = Vec::<OwnedFd>::new();
    // SAFETY: traverse kernel-validated msghdr ancillary offsets. Every FD
    // is immediately wrapped for RAII close even if checks later fail.
    unsafe {
        let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
        while !cmsg.is_null() {
            if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                let header_len = libc::CMSG_LEN(0) as usize;
                // musl uses socklen_t here; glibc uses size_t.
                let cmsg_len: usize = (*cmsg).cmsg_len as _;
                if cmsg_len < header_len {
                    return Err(invalid("malformed ancillary descriptor header"));
                }
                let bytes = cmsg_len - header_len;
                if !bytes.is_multiple_of(size_of::<RawFd>()) {
                    return Err(invalid("misaligned ancillary descriptors"));
                }
                for offset in 0..(bytes / size_of::<RawFd>()) {
                    let fd = (libc::CMSG_DATA(cmsg) as *const RawFd)
                        .add(offset)
                        .read_unaligned();
                    if fd >= 0 {
                        received.push(OwnedFd::from_raw_fd(fd));
                    }
                }
            }
            cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
        }
    }
    if n as usize != WIRE_LEN
        || msg.msg_flags & (libc::MSG_CTRUNC | libc::MSG_TRUNC) != 0
        || payload[0] != WIRE_VERSION + (expected as u8 - 1)
        || !constant_time_equal(&payload[1..], proof)
        || received.len() != expected
    {
        return Err(denied());
    }
    Ok(received)
}
fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |diff, (&a, &b)| diff | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::File,
        io::Read,
        os::fd::{AsFd, AsRawFd},
    };

    #[test]
    fn one_fd_is_owned_at_destination_and_invalid_proof_closes_received_fd() {
        let (sender, receiver) = UnixStream::pair().unwrap();
        let source = File::open("/dev/zero").unwrap();
        send_one(&sender, source.as_fd(), &[17; 32]).unwrap();
        let mut received = File::from(receive_one(&receiver, &[17; 32]).unwrap());
        // Received PTY masters must never be inherited by an unrelated
        // subprocess through a forgotten close-on-exec flag.
        let flags = unsafe { libc::fcntl(received.as_raw_fd(), libc::F_GETFD) };
        assert!(flags >= 0 && flags & libc::FD_CLOEXEC != 0);
        let mut bytes = [1_u8; 16];
        received.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, [0; 16]);

        send_one(&sender, source.as_fd(), &[19; 32]).unwrap();
        assert_eq!(
            receive_one(&receiver, &[88; 32]).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        // A failed admission may not close the original sender handle.
        let mut original = source;
        original.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, [0; 16]);
    }

    #[test]
    fn atomic_two_fd_escrow_is_exactly_typed_and_close_on_exec() {
        let (sender, receiver) = UnixStream::pair().unwrap();
        let mut master = File::open("/dev/zero").unwrap();
        let identity = File::open("/dev/null").unwrap();
        send_pair(&sender, master.as_fd(), identity.as_fd(), &[23; 32]).unwrap();
        let (fd_a, fd_b) = receive_pair(&receiver, &[23; 32]).unwrap();
        for fd in [&fd_a, &fd_b] {
            // SAFETY: the received descriptors are valid for F_GETFD.
            let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
            assert!(flags >= 0 && flags & libc::FD_CLOEXEC != 0);
        }
        let mut restored = File::from(fd_a);
        let mut bytes = [1_u8; 16];
        restored.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, [0; 16]);
        let mut marker = File::from(fd_b);
        assert_eq!(marker.read(&mut bytes).unwrap(), 0);

        // Message cardinality is encoded in the protocol version: accepting
        // one half of a two-FD escrow must close everything and fail.
        send_pair(&sender, master.as_fd(), identity.as_fd(), &[33; 32]).unwrap();
        assert_eq!(
            receive_one(&receiver, &[33; 32]).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied,
        );
        send_one(&sender, master.as_fd(), &[35; 32]).unwrap();
        assert_eq!(
            receive_pair(&receiver, &[35; 32]).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied,
        );
        send_pair(&sender, master.as_fd(), identity.as_fd(), &[37; 32]).unwrap();
        assert_eq!(
            receive_pair(&receiver, &[99; 32]).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied,
        );
        // No failed destination admission consumes the original handles.
        master.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, [0; 16]);
    }

    #[test]
    fn short_or_missing_ancillary_message_cannot_authorize_transfer() {
        let (mut sender, receiver) = UnixStream::pair().unwrap();
        use std::io::Write;
        sender.write_all(&[WIRE_VERSION; WIRE_LEN]).unwrap();
        assert!(receive_one(&receiver, &[WIRE_VERSION; 32]).is_err());
        assert!(!constant_time_equal(&[1_u8; 32], &[2_u8; 32]));
        assert!(constant_time_equal(&[1_u8; 32], &[1_u8; 32]));
    }
}
