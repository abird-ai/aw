use std::{
    borrow::Cow,
    ffi::{CString, OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        process::CommandExt,
    },
    path::Path,
    process::{Child, Command, ExitStatus},
    sync::atomic::{AtomicU64, Ordering},
};

#[cfg(target_os = "linux")]
use std::os::fd::{AsFd, OwnedFd};

#[cfg(target_os = "linux")]
pub mod fd_transfer;
#[cfg(target_os = "linux")]
pub mod pid_identity;
#[cfg(target_os = "linux")]
mod process_observation;
#[cfg(target_os = "linux")]
mod process_projection;
#[cfg(target_os = "linux")]
pub use process_projection::{install_child_read_capability, ReadCapabilityKind};
pub mod pty_poll;
pub use aw_tools_fs_unix::scoped_fs;

pub const FAMILY: &str = "unix";

/// Signal that physically ended a waited process, when the platform reports one.
///
/// This is exit evidence only: it says nothing about who sent the signal or why.
pub fn exit_signal(status: &ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt as _;
    status.signal()
}

/// Stable platform name for a known signal number.
///
/// Process state retains only the number; names are presentation helpers.
pub const fn signal_name(number: i32) -> Option<&'static str> {
    match number {
        libc::SIGHUP => Some("SIGHUP"),
        libc::SIGINT => Some("SIGINT"),
        libc::SIGQUIT => Some("SIGQUIT"),
        libc::SIGILL => Some("SIGILL"),
        libc::SIGTRAP => Some("SIGTRAP"),
        libc::SIGABRT => Some("SIGABRT"),
        libc::SIGBUS => Some("SIGBUS"),
        libc::SIGFPE => Some("SIGFPE"),
        libc::SIGKILL => Some("SIGKILL"),
        libc::SIGSEGV => Some("SIGSEGV"),
        libc::SIGPIPE => Some("SIGPIPE"),
        libc::SIGALRM => Some("SIGALRM"),
        libc::SIGTERM => Some("SIGTERM"),
        libc::SIGUSR1 => Some("SIGUSR1"),
        libc::SIGUSR2 => Some("SIGUSR2"),
        libc::SIGXCPU => Some("SIGXCPU"),
        libc::SIGSYS => Some("SIGSYS"),
        _ => None,
    }
}

/// An observed directory identity, retained without locking its pathname.
/// Callers must still account for external writers racing a later path operation.
#[derive(Debug)]
pub struct DirectoryIdentity(File);
impl DirectoryIdentity {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        Ok(Self(file))
    }

    /// Local filesystem identity, not a cross-machine resource identifier.
    pub fn identity(&self) -> io::Result<(u64, u64)> {
        let meta = self.0.metadata()?;
        Ok((meta.dev(), meta.ino()))
    }

    pub fn is_current(&self, path: &Path) -> io::Result<bool> {
        let current = Self::open(path)?;
        Ok(self.identity()? == current.identity()?)
    }
}

/// An observed regular-file identity, retained without locking its pathname.
/// Used to bind configured executables/resources to the object observed during
/// trusted host preparation; callers still revalidate immediately before use.
#[derive(Debug)]
pub struct FileIdentity(File);
impl FileIdentity {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "file identity requires a regular file",
            ));
        }
        Ok(Self(file))
    }

    pub fn identity(&self) -> io::Result<(u64, u64)> {
        let meta = self.0.metadata()?;
        Ok((meta.dev(), meta.ino()))
    }

    pub fn is_current(&self, path: &Path) -> io::Result<bool> {
        let current = Self::open(path)?;
        Ok(self.identity()? == current.identity()?)
    }
}

pub fn local_principal() -> io::Result<String> {
    // SAFETY: geteuid has no arguments or memory-safety preconditions.
    Ok(format!("unix:{}", unsafe { libc::geteuid() }))
}

pub fn effective_uid() -> Option<u32> {
    // SAFETY: geteuid has no arguments or memory-safety preconditions.
    Some(unsafe { libc::geteuid() })
}

pub fn null_device() -> &'static str {
    "/dev/null"
}

pub fn os_string_from_bytes(bytes: Vec<u8>) -> io::Result<OsString> {
    if bytes.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains NUL",
        ));
    }
    Ok(OsString::from_vec(bytes))
}

pub fn os_str_bytes(value: &OsStr) -> io::Result<Cow<'_, [u8]>> {
    Ok(Cow::Borrowed(value.as_bytes()))
}

pub fn configure_child_group(command: &mut Command) {
    command.process_group(0);
}

/// Whether one waited Unix exit is physically compatible with our direct
/// Process stop signal. A different signal or any exit code is independent
/// evidence and must not acquire timeout/cancel attribution.
pub fn stop_exit_matches(status: &ExitStatus, force: bool) -> bool {
    use std::os::unix::process::ExitStatusExt as _;
    status.signal() == Some(if force { libc::SIGKILL } else { libc::SIGTERM })
}

/// Launch an explicit, foreground-independent local owner in a new OS
/// session. The caller must close inherited stdio, bound its lifetime, and
/// supply a private rendezvous; this never implicitly starts a service.
pub fn spawn_detached_session(command: &mut Command) -> io::Result<Child> {
    // SAFETY: the child pre-exec callback calls only async-signal-safe
    // setsid(2); no allocation, locks, environment mutation, or Rust unwind.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn()
}

fn checked_process_group(pid: u32) -> io::Result<libc::pid_t> {
    let pid = libc::pid_t::try_from(pid)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid process group"))?;
    if pid <= 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe process group",
        ));
    }
    Ok(pid)
}

/// Signal one explicitly spawned per-child process group.
///
/// `Ok(true)` proves the signal was delivered to a live group; `Ok(false)`
/// proves the group was absent; an error leaves the result unknown.
fn signal_process_group(pid: libc::pid_t, signal: libc::c_int) -> io::Result<bool> {
    // SAFETY: negative pid targets only the group established on child spawn.
    if unsafe { libc::kill(-pid, signal) } == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(false)
    } else {
        Err(error)
    }
}

/// Forced stop of the explicitly spawned per-child process group.
///
/// The bool preserves live-target vs already-absent evidence for the caller.
pub fn kill_process_group(pid: u32) -> io::Result<bool> {
    signal_process_group(checked_process_group(pid)?, libc::SIGKILL)
}

fn child_exit_ready(pid: libc::pid_t) -> io::Result<bool> {
    loop {
        // SAFETY: zeroed siginfo_t is valid writable storage for waitid.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: this queries our exact child PID and WNOWAIT preserves status.
        if unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        } == 0
        {
            // SAFETY: waitid initialized the SIGCHLD union fields or left pid zero.
            return Ok(unsafe { info.si_pid() } == pid);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

/// Low-level direct execution scope for one child invocation.
///
/// It owns stable leader identity plus the containment identity. Higher layers
/// should treat this as a private Process backend, not as the Process model.
pub struct DirectScope {
    group: libc::pid_t,
    #[cfg(target_os = "linux")]
    leader: Option<OwnedFd>,
    active: bool,
}
///
/// This direct fallback contains one process group only; an unsandboxed
/// descendant may escape with `setsid`. The strong Linux PID-namespace backend
/// removes that limitation without changing Process semantics.
impl DirectScope {
    pub fn attach(pid: u32) -> io::Result<Self> {
        let group = checked_process_group(pid)?;
        #[cfg(target_os = "linux")]
        let leader = pid_identity::open(pid).ok();
        Ok(Self {
            group,
            #[cfg(target_os = "linux")]
            leader,
            active: true,
        })
    }

    /// Has the exact main command become terminal without consuming its wait status?
    pub fn ready(&self) -> io::Result<bool> {
        #[cfg(target_os = "linux")]
        if let Some(leader) = &self.leader {
            return Ok(matches!(
                pid_identity::inspect_child_exit(leader.as_fd())?,
                pid_identity::ExitObservation::Exited { .. }
            ));
        }
        child_exit_ready(self.group)
    }

    /// Observe whether live work remains after the waited group leader is terminal.
    ///
    /// The terminal leader itself is excluded. `Ok(true)` proves at least one
    /// live remaining member, `Ok(false)` proves no live remaining member, and
    /// `Err` leaves the result unknown.
    pub fn observe(&self) -> io::Result<bool> {
        if !self.active {
            return Ok(false);
        }
        #[cfg(target_os = "linux")]
        {
            process_observation::group_has_live_member(self.group)
        }
        #[cfg(target_os = "macos")]
        {
            mac_group_has_live_member(self.group)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "process-group observation unsupported on this Unix target",
            ))
        }
    }

    /// Request graceful stop of the owned direct process group.
    pub fn graceful(&self) -> io::Result<bool> {
        if !self.active {
            return Ok(false);
        }
        signal_process_group(self.group, libc::SIGTERM)
    }

    fn force_once(&self) -> io::Result<bool> {
        match signal_process_group(self.group, libc::SIGKILL) {
            #[cfg(target_os = "macos")]
            Err(error)
                if error.raw_os_error() == Some(libc::EPERM)
                    && matches!(self.ready(), Ok(true))
                    && matches!(self.observe(), Ok(false)) =>
            {
                // Darwin killpg can report EPERM when only the terminal leader/
                // unsignalable zombie remains. Proven-ready leader + empty scope
                // is positive NoTarget, not an unbounded cleanup failure.
                Ok(false)
            }
            result => result,
        }
    }

    /// Final bounded force-stop of this direct execution scope.
    ///
    /// An unconfirmed first attempt is retried once immediately while leader
    /// identity is still stable. After that final result the scope is inert;
    /// callers retain `Err` as Unknown rather than retrying after leader reap.
    pub fn terminate(&mut self) -> io::Result<bool> {
        if !self.active {
            return Ok(false);
        }
        let first = self.force_once();
        let result = match first {
            Err(_) => self.force_once(),
            other => other,
        };
        self.active = false;
        result
    }
}

impl Drop for DirectScope {
    fn drop(&mut self) {
        if self.active {
            let _ = self.terminate();
        }
    }
}

#[cfg(target_os = "macos")]
fn mac_group_has_live_member(group: libc::pid_t) -> io::Result<bool> {
    let mut pids: Vec<libc::pid_t> = vec![0; 32];
    loop {
        let bytes = std::mem::size_of_val(&pids[..]);
        // SAFETY: writable PID array of exactly `bytes` bytes.
        let used =
            unsafe { libc::proc_listpgrppids(group, pids.as_mut_ptr().cast(), bytes as i32) };
        if used <= 0 {
            return Err(io::Error::other("cannot list command process group"));
        }
        let listed = &pids[..pids.len().min(used as usize)];
        for &pid in listed {
            // Settlement keeps the leader waitable; it is already terminal.
            if pid <= 0 || pid == group {
                continue;
            }
            let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
            let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
            // SAFETY: output storage matches the requested flavor and size.
            let got = unsafe {
                libc::proc_pidinfo(
                    pid,
                    libc::PROC_PIDTBSDINFO,
                    0,
                    info.as_mut_ptr().cast(),
                    size,
                )
            };
            if got != size {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ESRCH) {
                    continue;
                }
                return Err(error);
            }
            // SAFETY: proc_pidinfo returned the complete structure.
            let info = unsafe { info.assume_init() };
            if info.pbi_pgid == group as u32 && info.pbi_status != libc::SZOMB {
                return Ok(true);
            }
        }
        if listed.len() < pids.len() {
            return Ok(false);
        }
        if pids.len() >= 65_536 {
            return Err(io::Error::other("incomplete process group listing"));
        }
        pids.resize(pids.len() * 2, 0);
    }
}

pub fn terminate_child_tree(child: &mut Child) -> io::Result<()> {
    let _ = kill_process_group(child.id());
    child.kill()
}

pub fn validate_private_dir(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    // SAFETY: geteuid has no arguments.
    let uid = unsafe { libc::geteuid() };
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || meta.uid() != uid
        || meta.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "directory is not a private owned real directory",
        ));
    }
    Ok(())
}

pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    if !path.exists() {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(path)?;
    }
    validate_private_dir(path)
}

pub fn open_private_new(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    validate_private_file(&file)?;
    Ok(file)
}

pub fn open_private_existing(path: &Path, write: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(write);
    let file = options
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    validate_private_file(&file)?;
    Ok(file)
}

pub fn open_private_rw_create(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    validate_private_file(&file)?;
    Ok(file)
}

pub fn validate_private_file(file: &File) -> io::Result<()> {
    let meta = file.metadata()?;
    // SAFETY: geteuid has no arguments.
    let uid = unsafe { libc::geteuid() };
    if !meta.is_file() || meta.uid() != uid || meta.nlink() != 1 || meta.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "file is not an owned private single-link regular file",
        ));
    }
    Ok(())
}

pub fn sync_parent(path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent"))?;
    File::open(parent)?.sync_all()
}

/// Atomically move one already-created private file to a new name without ever
/// replacing an existing destination. Both paths must name entries in the same
/// filesystem; callers own file and parent durability barriers around this step.
pub fn publish_private_new(from: &Path, to: &Path) -> io::Result<()> {
    let from = CString::new(from.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "source path contains NUL"))?;
    let to = CString::new(to.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "destination path contains NUL")
    })?;

    #[cfg(target_os = "linux")]
    {
        // SAFETY: both C strings are NUL-terminated, AT_FDCWD selects the
        // supplied absolute/relative pathnames, and RENAME_NOREPLACE prevents
        // replacement of any pre-existing destination entry.
        let status = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                from.as_ptr(),
                libc::AT_FDCWD,
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if status == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if matches!(error.raw_os_error(), Some(libc::ENOSYS | libc::EINVAL)) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "atomic no-replace rename unavailable on this filesystem",
            ));
        }
        Err(error)
    }

    #[cfg(target_os = "macos")]
    {
        // SAFETY: both C strings are NUL-terminated and RENAME_EXCL requests
        // no-replace publication in the same filesystem namespace.
        let status = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
        if status == 0 {
            return Ok(());
        }
        Err(io::Error::last_os_error())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (from, to);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "atomic no-replace private publication unavailable on this Unix target",
        ))
    }
}

/// Open/create a stable advisory-lock file for non-secret user metadata.
/// The lock inode is never replaced while held.
pub fn open_user_metadata_lock(path: &Path) -> io::Result<File> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent"))?;
    fs::create_dir_all(parent)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    validate_private_file(&file)?;
    Ok(file)
}

/// Atomically publish non-secret user metadata in the target directory.
/// The temporary and final files are owner-only (0600). The parent directory
/// itself may be a conventional user config directory such as ~/.config.
pub fn atomic_write_user_metadata(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static NONCE: AtomicU64 = AtomicU64::new(1);
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent"))?;
    fs::create_dir_all(parent)?;

    let mut opened = None;
    for _ in 0..16 {
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let tmp = path.with_extension(format!("tmp-{}-{nonce}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&tmp)
        {
            Ok(file) => {
                opened = Some((tmp, file));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    let (tmp, mut file) = opened.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "cannot allocate temporary metadata file",
        )
    })?;

    let result = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub fn path_is_link_like(path: &Path) -> io::Result<bool> {
    Ok(fs::symlink_metadata(path)?.file_type().is_symlink())
}

pub fn open_nofollow_read(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
}

pub fn file_link_count(file: &File) -> io::Result<u64> {
    Ok(file.metadata()?.nlink())
}

pub fn trusted_direct_filesystem_supported(path: &Path) -> io::Result<bool> {
    #[cfg(target_os = "linux")]
    {
        use std::{ffi::CString, mem::MaybeUninit};
        let bytes = path.as_os_str().as_bytes();
        let path = CString::new(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
        let mut stats = MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: path is a live NUL-terminated string and stats is writable.
        if unsafe { libc::statfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // f_type signedness varies by libc.
        let magic: u64 = unsafe { stats.assume_init() }.f_type as _;
        Ok(matches!(
            magic,
            0xEF53
                | 0x9123_683E
                | 0x5846_5342
                | 0xF2F5_2010
                | 0xCA45_1A4E
                | 0x0102_1994
                | 0x8584_58F6
                | 0x794C_7630
                | 0x0000_F15F
                | 0x5265_4973
                | 0x0000_3434
                | 0x2FC1_2FC1
        ))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Ok(false)
    }
}

pub fn secure_private_files_supported() -> bool {
    true
}

#[cfg(all(target_os = "linux", feature = "local-ipc"))]
pub fn local_ipc_private_parent(path: &Path, uid: u32) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    if !path.is_absolute() || path.as_os_str().as_bytes().len() > 100 || path.file_name().is_none()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "socket needs an absolute pathname of at most 100 bytes",
        ));
    }
    if path
        .as_os_str()
        .as_bytes()
        .iter()
        .any(|b| *b < 32 || *b == 127)
        || path
            .as_os_str()
            .as_bytes()
            .split(|b| *b == b'/')
            .skip(1)
            .any(|part| part.is_empty() || part == b"." || part == b"..")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "socket path contains empty, dot or control components",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "socket parent missing"))?;
    for ancestor in parent.ancestors() {
        let meta = fs::symlink_metadata(ancestor)?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "socket ancestors must be real directories",
            ));
        }
    }
    let meta = fs::symlink_metadata(parent)?;
    if meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "socket parent must be owned and private",
        ));
    }
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "local-ipc"))]
pub fn local_ipc_socket_identity(path: &Path, uid: u32) -> io::Result<(u64, u64)> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    local_ipc_private_parent(path, uid)?;
    let meta = fs::symlink_metadata(path)?;
    if !meta.file_type().is_socket() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "endpoint must be your private filesystem socket",
        ));
    }
    Ok((meta.dev(), meta.ino()))
}

#[cfg(all(target_os = "linux", feature = "local-ipc"))]
pub struct LocalBoundSocket {
    pub listener: tokio::net::UnixListener,
    path: std::path::PathBuf,
    identity: (u64, u64),
    uid: u32,
}

#[cfg(all(target_os = "linux", feature = "local-ipc"))]
impl LocalBoundSocket {
    pub fn bind(path: &Path) -> io::Result<Self> {
        use std::os::unix::fs::PermissionsExt;
        let uid = effective_uid().ok_or_else(|| {
            io::Error::new(io::ErrorKind::Unsupported, "effective UID unavailable")
        })?;
        local_ipc_private_parent(path, uid)?;
        match fs::symlink_metadata(path) {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "socket path already exists",
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let listener = tokio::net::UnixListener::bind(path)?;
        let result = (|| {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
            local_ipc_socket_identity(path, uid)
        })();
        let identity = match result {
            Ok(identity) => identity,
            Err(error) => {
                drop(listener);
                let _ = fs::remove_file(path);
                return Err(error);
            }
        };
        Ok(Self {
            listener,
            path: path.to_owned(),
            identity,
            uid,
        })
    }
}

#[cfg(all(target_os = "linux", feature = "local-ipc"))]
impl Drop for LocalBoundSocket {
    fn drop(&mut self) {
        if local_ipc_socket_identity(&self.path, self.uid).ok() == Some(self.identity) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod direct_scope_tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt as _;

    fn grouped_shell(command: &str) -> Child {
        let mut child = Command::new("/bin/sh");
        configure_child_group(&mut child);
        child.arg("-c").arg(command).spawn().expect("grouped child")
    }

    #[test]
    fn direct_scope_distinguishes_stopped_and_no_target() {
        let mut child = grouped_shell("sleep 30");
        let mut scope = DirectScope::attach(child.id()).expect("direct scope");
        assert!(scope.terminate().expect("force stop"));
        assert!(!scope.terminate().expect("scope already finalized"));
        let status = child.wait().expect("reap");
        assert_eq!(exit_signal(&status), Some(libc::SIGKILL));
        assert_eq!(status.signal(), Some(libc::SIGKILL));

        let mut exited = grouped_shell("exit 0");
        let mut absent = DirectScope::attach(exited.id()).expect("owned scope");
        let status = exited.wait().expect("reap exited");
        assert!(status.success());
        assert_eq!(exit_signal(&status), None);
        assert!(!absent.terminate().expect("absent group"));
        assert!(!absent.terminate().expect("scope already finalized"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn direct_scope_observes_remaining_group_members_not_the_leader() {
        let mut leader_only = grouped_shell("sleep 30");
        let mut scope = DirectScope::attach(leader_only.id()).expect("direct scope");
        assert!(!scope.observe().expect("leader is excluded"));
        assert!(scope.terminate().expect("cleanup leader"));
        let _ = leader_only.wait();

        let mut with_child = grouped_shell("sleep 30 & wait");
        let mut scope = DirectScope::attach(with_child.id()).expect("direct scope");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while !scope.observe().expect("complete procfs observation") {
            assert!(
                std::time::Instant::now() < deadline,
                "background member did not appear"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(scope.terminate().expect("cleanup group"));
        let _ = with_child.wait();
    }

    #[test]
    fn signal_names_are_platform_facts_not_process_state() {
        assert_eq!(signal_name(libc::SIGKILL), Some("SIGKILL"));
        assert_eq!(signal_name(libc::SIGSEGV), Some("SIGSEGV"));
        assert_eq!(signal_name(i32::MAX), None);
    }

    #[test]
    fn direct_scope_drop_is_final_fail_safe() {
        let mut child = grouped_shell("sleep 30");
        let scope = DirectScope::attach(child.id()).expect("direct scope");
        drop(scope);
        assert_eq!(child.wait().expect("reap").signal(), Some(libc::SIGKILL));
    }
}
