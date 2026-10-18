use std::{
    borrow::Cow,
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        process::CommandExt,
    },
    path::Path,
    process::{Child, Command},
    sync::atomic::{AtomicU64, Ordering},
};

#[cfg(target_os = "linux")]
pub mod fd_transfer;
pub mod scoped_fs;

pub const FAMILY: &str = "unix";

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

    pub fn is_current(&self, path: &Path) -> io::Result<bool> {
        let current = Self::open(path)?;
        let original = self.0.metadata()?;
        let current = current.0.metadata()?;
        Ok(original.dev() == current.dev() && original.ino() == current.ino())
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

/// Graceful stop of the explicitly spawned per-child process group.
/// A missing/already-exited group is harmless; a failed signal is never
/// interpreted as proof of process termination.
pub fn terminate_process_group(pid: u32) -> io::Result<()> {
    let pid = i32::try_from(pid)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid process group"))?;
    if pid <= 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe process group",
        ));
    }
    // SAFETY: negative pid targets only the group established on child spawn.
    let result = unsafe { libc::kill(-pid, libc::SIGTERM) };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

pub fn kill_process_group(pid: u32) {
    if let Ok(pid) = i32::try_from(pid) {
        if pid > 1 {
            // SAFETY: negative pid addresses the process group created by aw.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
}

pub fn terminate_child_tree(child: &mut Child) -> io::Result<()> {
    kill_process_group(child.id());
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
        let magic = unsafe { stats.assume_init() }.f_type as u64;
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
