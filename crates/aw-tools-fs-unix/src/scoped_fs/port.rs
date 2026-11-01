//! Bounded file windows, staged mutation and cooperative workspace writes.
//! Filesystem authority comes exclusively from the pinned `Root`.
use super::{at, cstring, validate_path, Result, Root, ScopedError};
use aw_tools::filesystem::private_artifact::{
    Kind as ArtifactKind, Lease as ArtifactLease, ObjectIdentity, Record,
};
use sha2::{Digest as _, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom, Write},
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    sync::MutexGuard,
    time::{Duration, Instant},
};

const MAX_BATCH_FILE: usize = 64 * 1024 * 1024;
static NEXT_TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn regular(file: &File) -> Result<u64> {
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(ScopedError::Denied("only regular files are permitted"));
    }
    Ok(meta.len())
}
fn stream_bytes(file: File, cap: usize) -> Result<Vec<u8>> {
    let size = regular(&file)?;
    if size > cap as u64 {
        return Err(ScopedError::Limit("file exceeds tool byte limit"));
    }
    let mut data = Vec::new();
    file.take((cap as u64).saturating_add(1))
        .read_to_end(&mut data)?;
    if data.len() > cap {
        return Err(ScopedError::Limit("file grew beyond tool byte limit"));
    }
    Ok(data)
}

/// Hash a singly-linked regular file with fixed scratch memory.
fn digest_file(mut file: File) -> Result<(u64, [u8; 32])> {
    let size = regular(&file)?;
    if size > MAX_BATCH_FILE as u64 {
        return Err(ScopedError::Limit("binary source exceeds 64 MiB"));
    }
    let mut digest = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut count = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        count = count
            .checked_add(n as u64)
            .ok_or(ScopedError::Limit("binary source size overflow"))?;
        if count > MAX_BATCH_FILE as u64 {
            return Err(ScopedError::Limit("binary source grew past 64 MiB"));
        }
        digest.update(&buf[..n]);
    }
    Ok((count, digest.finalize().into()))
}

fn parent_model_path(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

fn existing_parent_model_path(path: &str, missing_parents: usize) -> String {
    let parts = path.split('/').collect::<Vec<_>>();
    let keep = parts.len().saturating_sub(missing_parents + 1);
    parts[..keep].join("/")
}

impl Root {
    fn reserve_private_artifact(
        &self,
        parent: &File,
        parent_path: String,
        name: &str,
        object: ObjectIdentity,
        kind: ArtifactKind,
    ) -> Result<ArtifactLease> {
        let parent = Self::artifact_identity(&parent.metadata()?);
        self.private
            .reserve(Record {
                kind,
                parent_path,
                parent,
                name: name.to_owned(),
                object,
            })
            .map_err(|_| ScopedError::Denied("cannot durably reserve private mutation artifact"))
    }
}

// Skip a line without buffering arbitrary bytes (including an overlong line).
/// Skipped lines must not allocate, but also cannot scan an unlimited file.
/// Existing-file metadata is part of the replacement precondition. A
/// replacement must not silently erase ownership, ACLs or extended attributes.
fn preserve_metadata(old: &File, staged: &File, cancelled: &dyn Fn() -> bool) -> Result<()> {
    let metadata = old.metadata()?;
    // Never manufacture privilege-bearing executable bytes from a model
    // replacement. Elevated file capabilities are refused below as well.
    if metadata.mode() & 0o6000 != 0 {
        return Err(ScopedError::Denied(
            "setuid/setgid file replacement requires an explicit privileged host operation",
        ));
    }
    // Newly created staging files initially belong to the effective user.
    // Only change ownership when necessary; unprivileged fchown to the same
    // owner/group is not guaranteed to succeed on every filesystem.
    let stage_meta = staged.metadata()?;
    if stage_meta.uid() != metadata.uid() || stage_meta.gid() != metadata.gid() {
        // SAFETY: owned, open staging descriptor and validated source metadata.
        if unsafe { libc::fchown(staged.as_raw_fd(), metadata.uid(), metadata.gid()) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
    }
    // Mode is applied after all bytes have been written, avoiding broader
    // temporary permissions while the file is incomplete.
    if unsafe { libc::fchmod(staged.as_raw_fd(), metadata.mode() & 0o7777) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    #[cfg(target_os = "linux")]
    preserve_linux_xattrs(old, staged, cancelled)?;
    #[cfg(target_os = "macos")]
    {
        if cancelled() {
            return Err(ScopedError::Cancelled);
        }
        // Darwin's descriptor-based copyfile supports ACLs and resource-fork
        // xattrs without reopening an ambient pathname or copying file data.
        // SAFETY: both live descriptors are regular files; no mutable state.
        if unsafe {
            libc::fcopyfile(
                old.as_raw_fd(),
                staged.as_raw_fd(),
                std::ptr::null_mut(),
                libc::COPYFILE_XATTR | libc::COPYFILE_ACL,
            )
        } != 0
        {
            return Err(io::Error::last_os_error().into());
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn preserve_linux_xattrs(old: &File, staged: &File, cancelled: &dyn Fn() -> bool) -> Result<()> {
    use std::ffi::CString;
    const MAX_NAMES: usize = 64 * 1024;
    const MAX_TOTAL: usize = 1024 * 1024;
    // SAFETY: a null buffer asks the kernel for the required list size.
    let needed = unsafe { libc::flistxattr(old.as_raw_fd(), std::ptr::null_mut(), 0) };
    if needed < 0 {
        let error = io::Error::last_os_error();
        if matches!(error.raw_os_error(), Some(libc::ENOTSUP)) {
            return Ok(());
        }
        return Err(error.into());
    }
    if needed as usize > MAX_NAMES {
        return Err(ScopedError::Limit("too many extended-attribute names"));
    }
    let mut names = vec![0u8; needed as usize];
    if !names.is_empty() {
        // SAFETY: capacity came from the kernel and buffer is writable.
        let read =
            unsafe { libc::flistxattr(old.as_raw_fd(), names.as_mut_ptr().cast(), names.len()) };
        if read < 0 {
            return Err(io::Error::last_os_error().into());
        }
        if read as usize > names.len() {
            return Err(ScopedError::Limit("xattr names changed during staging"));
        }
        names.truncate(read as usize);
    }
    let mut total = 0usize;
    for raw in names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        if cancelled() {
            return Err(ScopedError::Cancelled);
        }
        if raw == b"security.capability" {
            // Never reattach file capabilities to model-generated executable
            // bytes. Editing privileged executable content requires an
            // explicit separate host operation.
            return Err(ScopedError::Denied(
                "file has security.capability; refuse replacement",
            ));
        }
        let name = CString::new(raw).map_err(|_| ScopedError::Denied("malformed xattr name"))?;
        // SAFETY: query an owned source descriptor with a valid xattr name.
        let needed =
            unsafe { libc::fgetxattr(old.as_raw_fd(), name.as_ptr(), std::ptr::null_mut(), 0) };
        if needed < 0 {
            return Err(io::Error::last_os_error().into());
        }
        total = total
            .saturating_add(needed as usize)
            .saturating_add(raw.len());
        if total > MAX_TOTAL {
            return Err(ScopedError::Limit("file metadata exceeds 1 MiB"));
        }
        let mut data = vec![0u8; needed as usize];
        let got = unsafe {
            libc::fgetxattr(
                old.as_raw_fd(),
                name.as_ptr(),
                data.as_mut_ptr().cast(),
                data.len(),
            )
        };
        if got < 0 {
            return Err(io::Error::last_os_error().into());
        }
        if got as usize != data.len() {
            return Err(ScopedError::Denied("source xattr changed while staging"));
        }
        // SAFETY: staging fd is privately owned, byte slice remains live.
        if unsafe {
            libc::fsetxattr(
                staged.as_raw_fd(),
                name.as_ptr(),
                data.as_ptr().cast(),
                data.len(),
                0,
            )
        } != 0
        {
            return Err(io::Error::last_os_error().into());
        }
    }
    Ok(())
}

/// An operation-level mutation guard. Held through planning, staging and commit;
/// local writers share Root's mutex, cooperating processes flock the same
/// stable parent inode even when the target is atomically renamed.
pub struct MutationGuard<'a> {
    _local: MutexGuard<'a, ()>,
    _directories: Vec<File>,
}
impl Drop for MutationGuard<'_> {
    fn drop(&mut self) {
        for dir in &self._directories {
            // SAFETY: descriptor remains open until the guard drops.
            unsafe {
                libc::flock(dir.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum LockMode {
    Intent,
    Exclusive,
}
impl Root {
    /// Reconcile durable private-artifact records once before this registry is
    /// shared with model-facing roots. Exact identity is required for cleanup;
    /// absent/replaced namespace entries only retire their stale record.
    pub fn reconcile_private_artifacts(&self) -> Result<()> {
        for record in self
            .private
            .records()
            .map_err(|_| ScopedError::Denied("private mutation artifact registry unavailable"))?
        {
            let path = if record.parent_path.is_empty() {
                record.name.clone()
            } else {
                format!("{}/{}", record.parent_path, record.name)
            };
            let (parent, name) = match self.parent(&path) {
                Ok(value) => value,
                Err(ScopedError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                    self.private
                        .retire_record(&record)
                        .map_err(|_| ScopedError::Denied("cannot retire stale private artifact"))?;
                    continue;
                }
                Err(error) => return Err(error),
            };
            let parent_id = Self::artifact_identity(&parent.metadata()?);
            if parent_id != record.parent {
                self.private
                    .retire_record(&record)
                    .map_err(|_| ScopedError::Denied("cannot retire stale private artifact"))?;
                continue;
            }
            let stat = match super::stat_leaf(&parent, &name, true) {
                Ok(stat) => stat,
                Err(ScopedError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                    self.private
                        .retire_record(&record)
                        .map_err(|_| ScopedError::Denied("cannot retire stale private artifact"))?;
                    continue;
                }
                Err(error) => return Err(error),
            };
            let object = ObjectIdentity {
                volume: stat.st_dev as u64,
                object: stat.st_ino as u64,
            };
            if object != record.object {
                self.private
                    .retire_record(&record)
                    .map_err(|_| ScopedError::Denied("cannot retire stale private artifact"))?;
                continue;
            }
            match stat.st_mode & libc::S_IFMT {
                libc::S_IFDIR => lifecycle::cleanup_stage_tree(
                    &parent,
                    &name,
                    Some((stat.st_dev as u64, stat.st_ino as u64)),
                ),
                libc::S_IFREG | libc::S_IFLNK => {
                    unlink(&parent, &name);
                }
                _ => {
                    return Err(ScopedError::Denied(
                        "unsupported owned private mutation artifact",
                    ));
                }
            }
            if !stat_leaf_missing(&parent, &name) {
                return Err(ScopedError::Denied(
                    "private mutation artifact cleanup incomplete",
                ));
            }
            self.private
                .retire_record(&record)
                .map_err(|_| ScopedError::Denied("cannot retire cleaned private artifact"))?;
        }
        Ok(())
    }

    /// Compatible with dotlink: lock existing ancestor directories SH and
    /// the nearest existing parent EX; acquire in root-to-leaf path order.
    /// A missing parent inherits the EX lock on its existing ancestor.
    pub fn lock_mutations<'a>(&'a self, paths: &[String]) -> Result<MutationGuard<'a>> {
        self.lock_mutations_with_cancel(paths, &|| false)
    }
    pub fn lock_mutations_with_cancel<'a>(
        &'a self,
        paths: &[String],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<MutationGuard<'a>> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let local = loop {
            if cancelled() {
                return Err(ScopedError::Cancelled);
            }
            match self.serial.try_lock() {
                Ok(guard) => break guard,
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return Err(ScopedError::Denied("file root is poisoned; inspect"))
                }
                Err(std::sync::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(std::sync::TryLockError::WouldBlock) => {
                    return Err(ScopedError::Denied("local mutation lock timed out"))
                }
            }
        };
        let mut plan = BTreeMap::<String, (File, LockMode)>::new();
        for path in paths {
            validate_path(path)?;
            let components = path.split('/').collect::<Vec<_>>();
            let mut current_path = String::new();
            let mut parent = at(&self.directory, ".", libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
            if !plan.contains_key("") {
                plan.insert(String::new(), (parent.try_clone()?, LockMode::Intent));
            }
            for part in components.iter().take(components.len().saturating_sub(1)) {
                let child = match at(&parent, part, libc::O_RDONLY | libc::O_DIRECTORY, 0) {
                    Ok(dir) => dir,
                    Err(ScopedError::Io(error)) if error.kind() == io::ErrorKind::NotFound => break,
                    Err(error) => return Err(error),
                };
                if !current_path.is_empty() {
                    current_path.push('/');
                }
                current_path.push_str(part);
                if !plan.contains_key(&current_path) {
                    plan.insert(current_path.clone(), (child.try_clone()?, LockMode::Intent));
                }
                parent = child;
            }
            // Even a missing parent is protected: no other cooperating writer
            // can create it before this invocation finishes.
            if let Some((_, mode)) = plan.get_mut(&current_path) {
                *mode = LockMode::Exclusive;
            }
        }
        let mut held = Vec::with_capacity(plan.len());
        for (_, (dir, mode)) in plan {
            let operation = match mode {
                LockMode::Intent => libc::LOCK_SH,
                LockMode::Exclusive => libc::LOCK_EX,
            };
            loop {
                if cancelled() {
                    return Err(ScopedError::Cancelled);
                }
                // SAFETY: a pinned directory fd; protocol is advisory.
                if unsafe { libc::flock(dir.as_raw_fd(), operation | libc::LOCK_NB) } == 0 {
                    held.push(dir);
                    break;
                }
                let error = io::Error::last_os_error();
                match error.kind() {
                    io::ErrorKind::Interrupted => continue,
                    io::ErrorKind::WouldBlock if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    io::ErrorKind::WouldBlock => {
                        return Err(ScopedError::Denied("workspace mutation lock timed out"))
                    }
                    _ => return Err(error.into()),
                }
            }
        }
        Ok(MutationGuard {
            _local: local,
            _directories: held,
        })
    }

    /// Descriptor-rooted, no-follow metadata. Linux statx gathers change
    /// time and birth time in one syscall where the filesystem supplies it.
    pub fn entry_meta(&self, path: &str) -> Result<aw_tools::filesystem::EntryMeta> {
        self.ensure_public_path(path)?;
        use aw_tools::filesystem::{EntryMeta, Kind};
        let (dir, name) = self.parent(path)?;
        let name = cstring(&name)?;
        #[cfg(target_os = "linux")]
        {
            let mut stat = std::mem::MaybeUninit::<libc::statx>::zeroed();
            // SAFETY: pinned parent + valid leaf; statx never follows link.
            let result = unsafe {
                libc::statx(
                    dir.as_raw_fd(),
                    name.as_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                    libc::STATX_BASIC_STATS | libc::STATX_BTIME,
                    stat.as_mut_ptr(),
                )
            };
            if result != 0 {
                return Err(io::Error::last_os_error().into());
            }
            let stat = unsafe { stat.assume_init() };
            let kind = match stat.stx_mode as u32 & libc::S_IFMT {
                libc::S_IFDIR => Kind::Dir,
                libc::S_IFREG => Kind::File,
                libc::S_IFLNK => Kind::Symlink,
                _ => Kind::Other,
            };
            let time = |part: libc::statx_timestamp| {
                aw_tools::filesystem::system_time_from_unix(part.tv_sec, part.tv_nsec)
            };
            Ok(EntryMeta {
                kind,
                size: if matches!(kind, Kind::File | Kind::Symlink) {
                    Some(stat.stx_size)
                } else {
                    None
                },
                mtime: if stat.stx_mask & libc::STATX_MTIME != 0 {
                    time(stat.stx_mtime)
                } else {
                    None
                },
                ctime: if stat.stx_mask & libc::STATX_CTIME != 0 {
                    time(stat.stx_ctime)
                } else {
                    None
                },
                btime: if stat.stx_mask & libc::STATX_BTIME != 0 {
                    time(stat.stx_btime)
                } else {
                    None
                },
                mode: Some(stat.stx_mode as u32 & 0o7777),
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
            if unsafe {
                libc::fstatat(
                    dir.as_raw_fd(),
                    name.as_ptr(),
                    stat.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } != 0
            {
                return Err(io::Error::last_os_error().into());
            }
            let stat = unsafe { stat.assume_init() };
            let kind = match stat.st_mode & libc::S_IFMT {
                libc::S_IFDIR => Kind::Dir,
                libc::S_IFREG => Kind::File,
                libc::S_IFLNK => Kind::Symlink,
                _ => Kind::Other,
            };
            let time = |secs: i64| aw_tools::filesystem::system_time_from_unix(secs, 0);
            Ok(EntryMeta {
                kind,
                size: if matches!(kind, Kind::File | Kind::Symlink) {
                    Some(stat.st_size.max(0) as u64)
                } else {
                    None
                },
                mtime: time(stat.st_mtime),
                ctime: time(stat.st_ctime),
                btime: None,
                mode: Some(stat.st_mode as u32 & 0o7777),
            })
        }
    }

    /// Metadata is resolved below the pinned parent and does not require
    /// read permission for a file merely to list its name/size.
    pub fn entry_info(&self, path: &str) -> Result<(u64, Option<std::time::SystemTime>)> {
        let (dir, name) = self.parent(path)?;
        let name = cstring(&name)?;
        let mut info = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: owned parent descriptor and a NUL-terminated leaf name.
        if unsafe {
            libc::fstatat(
                dir.as_raw_fd(),
                name.as_ptr(),
                info.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(io::Error::last_os_error().into());
        }
        let info = unsafe { info.assume_init() };
        let kind = info.st_mode & libc::S_IFMT;
        if kind != libc::S_IFDIR && kind != libc::S_IFREG {
            return Err(ScopedError::Denied("unsupported filesystem entry"));
        }
        #[cfg(target_os = "linux")]
        let nanoseconds = info.st_mtime_nsec.max(0) as u32;
        #[cfg(not(target_os = "linux"))]
        let nanoseconds = 0;
        let timestamp = if info.st_mtime >= 0 {
            std::time::UNIX_EPOCH
                .checked_add(std::time::Duration::new(info.st_mtime as u64, nanoseconds))
        } else {
            None
        };
        Ok((
            if kind == libc::S_IFREG {
                info.st_size.max(0) as u64
            } else {
                0
            },
            timestamp,
        ))
    }

    /// Bounded bytes from a pinned regular file. Unlike `read`, the caller
    /// chooses its own hard limit; binary tools can opt into larger files.
    pub fn read_bytes(&self, path: &str, cap: usize) -> Result<Vec<u8>> {
        self.ensure_public_path(path)?;
        if cap > MAX_BATCH_FILE {
            return Err(ScopedError::Limit("binary read cap exceeds 64 MiB"));
        }
        let (dir, name) = self.parent(path)?;
        let file =
            at(&dir, &name, libc::O_RDONLY | libc::O_NONBLOCK, 0).map_err(|error| match error {
                ScopedError::Io(ref e) if e.raw_os_error() == Some(libc::ELOOP) => {
                    ScopedError::Denied("refusing symlink target")
                }
                other => other,
            })?;
        stream_bytes(file, cap)
    }

    /// Read a bounded byte window without loading the prefix or the whole file.
    pub fn read_binary_window(
        &self,
        path: &str,
        offset: u64,
        limit: usize,
    ) -> Result<(Vec<u8>, u64, bool)> {
        self.ensure_public_path(path)?;
        if limit == 0 || limit > MAX_BATCH_FILE {
            return Err(ScopedError::Limit("invalid binary window limit"));
        }
        let (dir, name) = self.parent(path)?;
        let mut file = at(&dir, &name, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
        let total = regular(&file)?;
        if offset > total {
            return Err(ScopedError::Denied("binary offset past EOF"));
        }
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::with_capacity(limit.min(65536));
        file.take(limit as u64).read_to_end(&mut bytes)?;
        let truncated = offset.saturating_add(bytes.len() as u64) < total;
        Ok((bytes, total, truncated))
    }

    /// Stream a line window without loading or decoding skipped lines. The
    /// returned window is byte-exact when `numbered` is false.
    pub fn read_window(
        &self,
        path: &str,
        offset: usize,
        limit: usize,
        cap: usize,
        numbered: bool,
    ) -> Result<(String, bool)> {
        self.read_window_with_cancel(path, offset, limit, cap, numbered, &|| false)
    }
    pub fn read_window_with_cancel(
        &self,
        path: &str,
        offset: usize,
        limit: usize,
        cap: usize,
        numbered: bool,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(String, bool)> {
        self.ensure_public_path(path)?;
        let (dir, name) = self.parent(path)?;
        let file = at(&dir, &name, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
        regular(&file)?;
        let mut reader = BufReader::new(file);
        aw_tools::read::window(&mut reader, offset, limit, cap, numbered, cancelled).map_err(
            |error| match error {
                aw_tools::read::ReadError::Io(e) => ScopedError::Io(e),
                aw_tools::read::ReadError::Denied(message) => ScopedError::Denied(message),
                aw_tools::read::ReadError::Limit(message) => ScopedError::Limit(message),
                aw_tools::read::ReadError::Cancelled => ScopedError::Cancelled,
            },
        )
    }
    /// DL-150 resumable line/byte window, entirely on a previously
    /// authorized regular file descriptor. Exact UTF-8 boundaries are
    /// determined by aw-tools, independently of the host platform.
    pub fn read_page_with_cancel(
        &self,
        path: &str,
        request: aw_tools::read::PageRequest,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<aw_tools::read::Page> {
        self.ensure_public_path(path)?;
        let (dir, name) = self.parent(path)?;
        let file = at(&dir, &name, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
        regular(&file)?;
        let mut reader = BufReader::new(file);
        aw_tools::read::page(
            &mut reader,
            request.line,
            request.lines,
            request.byte,
            request.cap,
            request.numbered,
            cancelled,
        )
        .map_err(|error| match error {
            aw_tools::read::ReadError::Io(e) => ScopedError::Io(e),
            aw_tools::read::ReadError::Denied(reason) => ScopedError::Denied(reason),
            aw_tools::read::ReadError::Limit(reason) => ScopedError::Limit(reason),
            aw_tools::read::ReadError::Cancelled => ScopedError::Cancelled,
        })
    }
    /// Only call this while holding `lock_mutations` for the target.
    pub fn stage_create(
        &self,
        path: &str,
        data: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<StagedWrite> {
        self.stage(path, data, StageKind::Create, cancel)
    }
    /// Only call this while holding `lock_mutations` for the target.
    pub fn stage_replace(
        &self,
        path: &str,
        data: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<StagedWrite> {
        self.stage(path, data, StageKind::Replace, cancel)
    }
    /// Only call this while holding `lock_mutations` for the target.
    pub fn stage_update(
        &self,
        path: &str,
        expected: &[u8],
        data: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<StagedWrite> {
        self.stage(path, data, StageKind::Update(expected), cancel)
    }
    /// Open one safe pinned regular file for zero-copy streamed edits.
    pub fn open_regular(&self, path: &str) -> Result<File> {
        self.ensure_public_path(path)?;
        let (dir, name) = self.parent(path)?;
        let file = at(&dir, &name, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
        regular(&file)?;
        Ok(file)
    }

    /// Stage a binary edit without allocating the original or replacement.
    /// The caller holds lock_mutations and owns the source digest/cancellation.
    pub fn stage_stream_create(
        &self,
        path: &str,
        writer: impl FnOnce(&mut File) -> Result<()>,
        cancel: &dyn Fn() -> bool,
    ) -> Result<StagedWrite> {
        self.stage_with(path, StageKind::Create, cancel, writer)
    }
    pub fn stage_stream_update(
        &self,
        path: &str,
        expected: (u64, [u8; 32]),
        writer: impl FnOnce(&mut File) -> Result<()>,
        cancel: &dyn Fn() -> bool,
    ) -> Result<StagedWrite> {
        self.stage_with(path, StageKind::UpdateDigest(expected), cancel, writer)
    }
    fn stage(
        &self,
        path: &str,
        data: &[u8],
        kind: StageKind<'_>,
        cancel: &dyn Fn() -> bool,
    ) -> Result<StagedWrite> {
        if data.len() > MAX_BATCH_FILE {
            return Err(ScopedError::Limit("staged file exceeds 64 MiB"));
        }
        self.stage_with(path, kind, cancel, |out| {
            for chunk in data.chunks(64 * 1024) {
                if cancel() {
                    return Err(ScopedError::Cancelled);
                }
                out.write_all(chunk)?;
            }
            Ok(())
        })
    }
    /// The first missing component is the only public rename target.
    /// Never materialize any missing parent directly in the public tree.
    fn missing_parent(&self, path: &str) -> Result<Option<(File, Vec<String>, String)>> {
        validate_path(path)?;
        let mut parts = path.split('/').collect::<Vec<_>>();
        let leaf = parts.pop().ok_or(ScopedError::Denied("missing target"))?;
        let mut parent = self.directory.try_clone()?;
        for (i, part) in parts.iter().enumerate() {
            match at(&parent, part, libc::O_RDONLY | libc::O_DIRECTORY, 0) {
                Ok(next) => parent = next,
                Err(ScopedError::Io(ref error)) if error.kind() == io::ErrorKind::NotFound => {
                    let missing = parts[i..]
                        .iter()
                        .map(|s| (*s).to_owned())
                        .collect::<Vec<_>>();
                    if missing.len() > aw_tools::filesystem::lifecycle::MAX_LIFECYCLE_DEPTH {
                        return Err(ScopedError::Limit("missing parent suffix is too deep"));
                    }
                    return Ok(Some((parent, missing, leaf.to_owned())));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }
    fn stage_missing_suffix(
        &self,
        path: &str,
        existing: File,
        missing: Vec<String>,
        leaf: String,
        cancel: &dyn Fn() -> bool,
        writer: impl FnOnce(&mut File) -> Result<()>,
    ) -> Result<StagedWrite> {
        let temp = format!(
            ".aw-stage-{}-{:016x}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let cname = cstring(&temp)?;
        if unsafe { libc::mkdirat(existing.as_raw_fd(), cname.as_ptr(), 0o700) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        let identity = (|| -> Result<(u64, u64)> {
            let stage = at(&existing, &temp, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
            let meta = stage.metadata()?;
            Ok((meta.dev(), meta.ino()))
        })()
        .inspect_err(|_| {
            lifecycle::remove_empty_stage(&existing, &temp);
        })?;
        let artifact = match self.reserve_private_artifact(
            &existing,
            existing_parent_model_path(path, missing.len()),
            &temp,
            ObjectIdentity {
                volume: identity.0,
                object: identity.1,
            },
            ArtifactKind::Stage,
        ) {
            Ok(artifact) => artifact,
            Err(error) => {
                lifecycle::cleanup_stage_tree(&existing, &temp, Some(identity));
                return Err(error);
            }
        };
        let result = (|| -> Result<()> {
            let mut current = at(&existing, &temp, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
            let mut directories = vec![current.try_clone()?];
            for component in missing.iter().skip(1) {
                if cancel() {
                    return Err(ScopedError::Cancelled);
                }
                let name = cstring(component)?;
                if unsafe { libc::mkdirat(current.as_raw_fd(), name.as_ptr(), 0o755) } != 0 {
                    return Err(io::Error::last_os_error().into());
                }
                current = at(&current, component, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
                directories.push(current.try_clone()?);
            }
            let mut file = at(
                &current,
                &leaf,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                0o600,
            )?;
            writer(&mut file)?;
            if file.metadata()?.len() > MAX_BATCH_FILE as u64 {
                return Err(ScopedError::Limit("staged output exceeds 64 MiB"));
            }
            if cancel() {
                return Err(ScopedError::Cancelled);
            }
            file.sync_all()?;
            drop(file);
            for dir in directories.into_iter().rev() {
                dir.sync_all()?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            lifecycle::cleanup_stage_tree(&existing, &temp, Some(identity));
            if stat_leaf_missing(&existing, &temp) {
                let _ = artifact.retire();
            }
            return Err(error);
        }
        Ok(StagedWrite {
            parent: existing,
            name: missing[0].clone(),
            temp,
            expected: None,
            create: true,
            committed: false,
            temp_directory: true,
            temp_identity: Some(identity),
            artifact: Some(artifact),
        })
    }
    fn stage_with(
        &self,
        path: &str,
        kind: StageKind<'_>,
        cancel: &dyn Fn() -> bool,
        writer: impl FnOnce(&mut File) -> Result<()>,
    ) -> Result<StagedWrite> {
        self.ensure_public_path(path)?;
        if cancel() {
            return Err(ScopedError::Cancelled);
        }
        if let Some((existing, missing, leaf)) = self.missing_parent(path)? {
            if matches!(kind, StageKind::Update(_) | StageKind::UpdateDigest(_)) {
                return Err(ScopedError::Denied("target changed before update"));
            }
            return self.stage_missing_suffix(path, existing, missing, leaf, cancel, writer);
        }
        // The ordinary branch accepts only an already-pinned parent;
        // concurrent removals must not trigger visible mkdir fallbacks.
        let (parent, target) = self.parent(path)?;
        // Plain canonical replacement never opens the target for reading
        // its contents. The old FD is solely metadata/xattr evidence.
        let existing_handle = if matches!(kind, StageKind::Replace) {
            match at(&parent, &target, libc::O_WRONLY | libc::O_NONBLOCK, 0) {
                Err(ScopedError::Io(ref err)) if err.kind() == io::ErrorKind::PermissionDenied => {
                    at(&parent, &target, libc::O_RDONLY | libc::O_NONBLOCK, 0)
                }
                other => other,
            }
        } else {
            at(&parent, &target, libc::O_RDONLY | libc::O_NONBLOCK, 0)
        };
        let old = match existing_handle {
            Ok(file) => {
                regular(&file)?;
                if matches!(kind, StageKind::Create) {
                    return Err(ScopedError::Denied("target already exists"));
                }
                Some(file)
            }
            Err(ScopedError::Io(e)) if e.kind() == io::ErrorKind::NotFound => None,
            Err(ScopedError::Io(e)) if e.raw_os_error() == Some(libc::ELOOP) => {
                return Err(ScopedError::Denied("refusing symlink target"))
            }
            Err(e) => return Err(e),
        };
        if old.is_none() && matches!(kind, StageKind::Update(_) | StageKind::UpdateDigest(_)) {
            return Err(ScopedError::Denied("target changed before update"));
        }
        if let Some(ref existing) = old {
            match kind {
                StageKind::Update(expected)
                    if stream_bytes(existing.try_clone()?, MAX_BATCH_FILE)? != expected =>
                {
                    return Err(ScopedError::Denied("target changed before update"));
                }
                StageKind::UpdateDigest((len, digest))
                    if digest_file(existing.try_clone()?)? != (len, digest) =>
                {
                    return Err(ScopedError::Denied("binary source changed before staging"));
                }
                _ => {}
            }
        }
        let name = format!(
            ".aw-stage-{}-{:016x}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        // Stage privately: source metadata is restored only after all bytes.
        let mut out = at(
            &parent,
            &name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )?;
        let stage_meta = out.metadata()?;
        let artifact = match self.reserve_private_artifact(
            &parent,
            parent_model_path(path).to_owned(),
            &name,
            ObjectIdentity {
                volume: stage_meta.dev(),
                object: stage_meta.ino(),
            },
            ArtifactKind::Stage,
        ) {
            Ok(artifact) => artifact,
            Err(error) => {
                drop(out);
                unlink(&parent, &name);
                return Err(error);
            }
        };
        let written = (|| -> Result<()> {
            if let Some(ref source) = old {
                let metadata = source.metadata()?;
                if metadata.mode() & 0o6000 != 0 {
                    return Err(ScopedError::Denied(
                        "setuid/setgid file replacement requires privileged host operation",
                    ));
                }
                // Keep an in-flight stage no broader than old mode.
                // File descriptor opened O_WRONLY remains writable after
                // narrowing its mode, until publication restores old mode.
                let mode = metadata.mode() & 0o600;
                if unsafe { libc::fchmod(out.as_raw_fd(), mode) } != 0 {
                    return Err(io::Error::last_os_error().into());
                }
            }
            writer(&mut out)?;
            if out.metadata()?.len() > MAX_BATCH_FILE as u64 {
                return Err(ScopedError::Limit("staged output exceeds 64 MiB"));
            }
            if let Some(ref source) = old {
                preserve_metadata(source, &out, cancel)?;
            }
            if cancel() {
                return Err(ScopedError::Cancelled);
            }
            out.sync_all()?;
            Ok(())
        })();
        drop(out);
        if let Err(error) = written {
            unlink(&parent, &name);
            if stat_leaf_missing(&parent, &name) {
                let _ = artifact.retire();
            }
            return Err(error);
        }
        Ok(StagedWrite {
            parent,
            name: target,
            temp: name,
            expected: match kind {
                StageKind::Update(expected) => Some(Expected::Bytes(expected.to_vec())),
                StageKind::UpdateDigest(hash) => Some(Expected::Digest(hash)),
                _ => None,
            },
            create: matches!(kind, StageKind::Create),
            committed: false,
            temp_directory: false,
            temp_identity: None,
            artifact: Some(artifact),
        })
    }
}

fn stat_leaf_missing(parent: &File, path: &str) -> bool {
    matches!(
        super::stat_leaf(parent, path, true),
        Err(ScopedError::Io(error)) if error.kind() == io::ErrorKind::NotFound
    )
}

fn unlink(parent: &File, path: &str) -> bool {
    let Ok(name) = cstring(path) else {
        return false;
    };
    // SAFETY: caller supplies an exact basename beneath the pinned parent.
    let result = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) };
    result == 0 || io::Error::last_os_error().kind() == io::ErrorKind::NotFound
}
#[derive(Clone, Copy)]
enum StageKind<'a> {
    Create,
    Replace,
    Update(&'a [u8]),
    UpdateDigest((u64, [u8; 32])),
}
enum Expected {
    Bytes(Vec<u8>),
    Digest((u64, [u8; 32])),
}

/// A fully synced sibling file; publishing is a single rename/link operation.
pub struct StagedWrite {
    parent: File,
    name: String,
    temp: String,
    expected: Option<Expected>,
    create: bool,
    committed: bool,
    temp_directory: bool,
    temp_identity: Option<(u64, u64)>,
    artifact: Option<ArtifactLease>,
}
impl StagedWrite {
    pub fn commit(mut self) -> Result<()> {
        if let Some((dev, ino)) = self.temp_identity {
            let opened = at(
                &self.parent,
                &self.temp,
                libc::O_RDONLY | libc::O_DIRECTORY,
                0,
            )?;
            let identity = opened.metadata()?;
            if identity.dev() != dev || identity.ino() != ino {
                return Err(ScopedError::Denied("staging directory identity changed"));
            }
        }
        if let Some(expected) = &self.expected {
            let current = at(
                &self.parent,
                &self.name,
                libc::O_RDONLY | libc::O_NONBLOCK,
                0,
            )?;
            let valid = match expected {
                Expected::Bytes(bytes) => stream_bytes(current, MAX_BATCH_FILE)? == *bytes,
                Expected::Digest(digest) => digest_file(current)? == *digest,
            };
            if !valid {
                return Err(ScopedError::Denied("target changed before commit"));
            }
        }
        let src = cstring(&self.temp)?;
        let dst = cstring(&self.name)?;
        if self.create {
            #[cfg(target_os = "linux")]
            {
                // Atomic install with no replacement and no second staging
                // hardlink. An unlink failure can no longer strand nlink=2.
                // SAFETY: pinned parent and live C strings; openat2 requires
                // Linux 5.6+, which also supports renameat2.
                let code = unsafe {
                    libc::syscall(
                        libc::SYS_renameat2,
                        self.parent.as_raw_fd(),
                        src.as_ptr(),
                        self.parent.as_raw_fd(),
                        dst.as_ptr(),
                        libc::RENAME_NOREPLACE,
                    )
                };
                if code != 0 {
                    return Err(io::Error::last_os_error().into());
                }
            }
            #[cfg(not(target_os = "linux"))]
            if self.temp_directory {
                lifecycle::rename_exact(&self.parent, &self.temp, &self.parent, &self.name)?;
            } else {
                // Other Unix kernels may not have rename-no-replace. Do not
                // pretend a failed cleanup leaves a singly linked file.
                let code = unsafe {
                    libc::linkat(
                        self.parent.as_raw_fd(),
                        src.as_ptr(),
                        self.parent.as_raw_fd(),
                        dst.as_ptr(),
                        0,
                    )
                };
                if code != 0 {
                    return Err(io::Error::last_os_error().into());
                }
                let removed = unsafe { libc::unlinkat(self.parent.as_raw_fd(), src.as_ptr(), 0) };
                if removed != 0 {
                    self.committed = true;
                    return Err(ScopedError::Io(io::Error::other(
                        "file published but temporary hardlink cleanup failed; inspect workspace",
                    )));
                }
            }
        } else {
            // Reject symlink/non-regular targets, even if staged before a swap.
            if self.expected.is_none() {
                // A write-only caller may not read the replaced file;
                // fstatat checks final-component type/identity without
                // opening its payload or following symlinks.
                let name = cstring(&self.name)?;
                let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
                let result = unsafe {
                    libc::fstatat(
                        self.parent.as_raw_fd(),
                        name.as_ptr(),
                        stat.as_mut_ptr(),
                        libc::AT_SYMLINK_NOFOLLOW,
                    )
                };
                if result == 0 {
                    let stat = unsafe { stat.assume_init() };
                    if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
                        return Err(ScopedError::Denied("replacement requires regular target"));
                    }
                } else {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::NotFound {
                        return Err(error.into());
                    }
                }
            } else {
                let file = at(
                    &self.parent,
                    &self.name,
                    libc::O_RDONLY | libc::O_NONBLOCK,
                    0,
                )?;
                regular(&file)?;
            }
            // SAFETY: paths are clean basenames beneath one pinned directory.
            if unsafe {
                libc::renameat(
                    self.parent.as_raw_fd(),
                    src.as_ptr(),
                    self.parent.as_raw_fd(),
                    dst.as_ptr(),
                )
            } < 0
            {
                return Err(io::Error::last_os_error().into());
            }
        }
        self.committed = true;
        let _ = self.parent.sync_all(); // committed; never invite a retry after sync failure
        if stat_leaf_missing(&self.parent, &self.temp) {
            if let Some(artifact) = self.artifact.take() {
                let _ = artifact.retire();
            }
        }
        Ok(())
    }
}
impl Drop for StagedWrite {
    fn drop(&mut self) {
        if !self.committed {
            if self.temp_directory {
                lifecycle::cleanup_stage_tree(&self.parent, &self.temp, self.temp_identity);
            } else {
                unlink(&self.parent, &self.temp);
            }
            if stat_leaf_missing(&self.parent, &self.temp) {
                if let Some(artifact) = self.artifact.take() {
                    let _ = artifact.retire();
                }
            }
        }
    }
}

#[path = "port/lifecycle.rs"]
pub(super) mod lifecycle;

#[cfg(test)]
mod publication_tests {
    use super::*;
    use std::{fs, path::PathBuf};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "aw-scoped-publish-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn root(&self) -> Root {
            Root::open(&self.0).unwrap()
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn private_stage_registry_is_exact_and_retires_after_publication() {
        let temp = Temp::new();
        fs::write(temp.0.join("user.txt"), b"old").unwrap();
        fs::write(
            temp.0.join(".aw-stage-123-0123456789abcdef.tmp"),
            b"ordinary user file",
        )
        .unwrap();
        let root = temp.root();
        let _guard = root.lock_mutations(&["user.txt".into()]).unwrap();
        let staged = root.stage_replace("user.txt", b"new", &|| false).unwrap();
        let records = root.private_artifacts().records().unwrap();
        assert_eq!(records.len(), 1);
        let private = &records[0];
        let private_path = private.name.clone();
        assert!(root.is_private_path(&private_path).unwrap());
        assert!(
            !root
                .is_private_path(".aw-stage-123-0123456789abcdef.tmp")
                .unwrap(),
            "well-formed lookalike is not private without exact identity record"
        );
        staged.commit().unwrap();
        assert!(root.private_artifacts().records().unwrap().is_empty());
        assert_eq!(fs::read(temp.0.join("user.txt")).unwrap(), b"new");
    }

    #[test]
    fn missing_parent_suffix_remains_invisible_until_single_commit() {
        let temp = Temp::new();
        let root = temp.root();
        let _guard = root.lock_mutations(&["new/deep/x.txt".into()]).unwrap();
        let staged = root
            .stage_create("new/deep/x.txt", b"contents", &|| false)
            .unwrap();
        assert!(!temp.0.join("new").exists());
        staged.commit().unwrap();
        assert_eq!(
            fs::read(temp.0.join("new/deep/x.txt")).unwrap(),
            b"contents"
        );
    }
    #[test]
    fn failed_writer_or_dropped_stage_leaves_no_visible_new_parent() {
        let temp = Temp::new();
        let root = temp.root();
        let _guard = root.lock_mutations(&["new/deep/x.txt".into()]).unwrap();
        let failed = root.stage_stream_create(
            "new/deep/x.txt",
            |out| {
                use std::io::Write;
                out.write_all(b"some bytes")?;
                Err(ScopedError::Denied("intentional staging fault"))
            },
            &|| false,
        );
        assert!(failed.is_err());
        assert!(!temp.0.join("new").exists());
        let staged = root
            .stage_create("new/deep/x.txt", b"contents", &|| false)
            .unwrap();
        assert!(!temp.0.join("new").exists());
        drop(staged);
        assert!(!temp.0.join("new").exists());
        assert!(!fs::read_dir(&temp.0).unwrap().flatten().any(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with(".aw-stage-")));
    }
    #[test]
    fn atomic_create_installs_one_link_without_leaking_a_temporary() {
        let temp = Temp::new();
        let root = temp.root();
        let _lock = root.lock_mutations(&["created.txt".to_owned()]).unwrap();
        root.stage_create("created.txt", b"content", &|| false)
            .unwrap()
            .commit()
            .unwrap();
        assert_eq!(fs::metadata(temp.0.join("created.txt")).unwrap().nlink(), 1);
        assert_eq!(root.read_bytes("created.txt", 128).unwrap(), b"content");
        assert!(!fs::read_dir(&temp.0).unwrap().flatten().any(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with(".aw-stage-")));
    }

    #[test]
    fn atomic_create_refuses_concurrent_existing_target() {
        let temp = Temp::new();
        let root = temp.root();
        let _lock = root.lock_mutations(&["created.txt".to_owned()]).unwrap();
        let stage = root
            .stage_create("created.txt", b"ours", &|| false)
            .unwrap();
        fs::write(temp.0.join("created.txt"), "theirs").unwrap();
        assert!(stage.commit().is_err());
        assert_eq!(fs::read(temp.0.join("created.txt")).unwrap(), b"theirs");
    }

    #[test]
    fn aborted_stage_never_deletes_a_different_directory_identity() {
        let temp = Temp::new();
        let root = temp.root();
        let _lock = root.lock_mutations(&["fresh/file.txt".to_owned()]).unwrap();
        let stage = root
            .stage_create("fresh/file.txt", b"payload", &|| false)
            .unwrap();
        assert!(!temp.0.join("fresh").exists());
        let private = fs::read_dir(&temp.0)
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".aw-stage-")
            })
            .unwrap();
        fs::rename(&private, temp.0.join("renamed")).unwrap();
        fs::create_dir(&private).unwrap();
        drop(stage);
        assert!(
            private.is_dir(),
            "another actor's private replacement survives"
        );
        assert!(temp.0.join("renamed").is_dir());
        assert!(!temp.0.join("fresh").exists());
    }

    #[test]
    fn streaming_update_hash_rechecks_preimage_before_publish() {
        let temp = Temp::new();
        let path = temp.0.join("blob.bin");
        fs::write(&path, b"abcdefghij").unwrap();
        let original = b"abcdefghij";
        let expected = (
            original.len() as u64,
            <[u8; 32]>::from(Sha256::digest(original)),
        );
        let root = temp.root();
        let _lock = root.lock_mutations(&["blob.bin".to_owned()]).unwrap();
        let stage = root
            .stage_stream_update(
                "blob.bin",
                expected,
                |out| {
                    out.write_all(b"abcXYZghij")?;
                    Ok(())
                },
                &|| false,
            )
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), original);
        stage.commit().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"abcXYZghij");
        assert_eq!(fs::metadata(&path).unwrap().nlink(), 1);

        let expected = (10, <[u8; 32]>::from(Sha256::digest(b"abcXYZghij")));
        let stage = root
            .stage_stream_update(
                "blob.bin",
                expected,
                |out| {
                    out.write_all(b"post change")?;
                    Ok(())
                },
                &|| false,
            )
            .unwrap();
        fs::write(&path, b"external!!").unwrap();
        assert!(stage.commit().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"external!!");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn replacement_keeps_owner_mode_and_extended_attributes() {
        use std::{
            ffi::CString,
            os::unix::{ffi::OsStrExt, fs::PermissionsExt},
        };
        let temp = Temp::new();
        let path = temp.0.join("sensitive.txt");
        fs::write(&path, b"before").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        let name = CString::new("user.aw_test_metadata").unwrap();
        let path_c = CString::new(path.as_os_str().as_bytes()).unwrap();
        let data = b"must survive";
        // SAFETY: pathname and metadata buffer are valid during the call.
        let set = unsafe {
            libc::setxattr(
                path_c.as_ptr(),
                name.as_ptr(),
                data.as_ptr().cast(),
                data.len(),
                0,
            )
        };
        if set != 0 {
            let e = io::Error::last_os_error();
            if matches!(e.raw_os_error(), Some(libc::ENOTSUP)) {
                return;
            }
            panic!("cannot create test xattr: {e}");
        }
        let original = fs::metadata(&path).unwrap();
        let root = temp.root();
        let _lock = root.lock_mutations(&["sensitive.txt".to_owned()]).unwrap();
        root.stage_update("sensitive.txt", b"before", b"after", &|| false)
            .unwrap()
            .commit()
            .unwrap();
        let replaced = fs::metadata(&path).unwrap();
        assert_eq!(replaced.mode() & 0o777, 0o700);
        assert_eq!(
            (replaced.uid(), replaced.gid()),
            (original.uid(), original.gid())
        );
        let mut stored = [0u8; 64];
        // SAFETY: valid, live pathname/name and writable output buffer.
        let n = unsafe {
            libc::getxattr(
                path_c.as_ptr(),
                name.as_ptr(),
                stored.as_mut_ptr().cast(),
                stored.len(),
            )
        };
        assert_eq!(n as usize, data.len(), "existing xattr must not be dropped");
        assert_eq!(&stored[..data.len()], data);
    }
}
