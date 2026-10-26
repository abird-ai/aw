//! Bounded file windows, staged mutation and cooperative workspace writes.
//! Filesystem authority comes exclusively from the pinned `Root`.
use super::{at, cstring, validate_path, Result, Root, ScopedError};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    sync::MutexGuard,
    time::{Duration, Instant},
};

const MAX_BATCH_FILE: usize = 64 * 1024 * 1024;
const MAX_READ_SCAN_BYTES: usize = 32 * 1024 * 1024;
static NEXT_TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn regular(file: &File) -> Result<u64> {
    let meta = file.metadata()?;
    if !meta.is_file() || meta.nlink() != 1 {
        return Err(ScopedError::Denied(
            "only singly-linked regular files are permitted",
        ));
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

// Skip a line without buffering arbitrary bytes (including an overlong line).
/// Skipped lines must not allocate, but also cannot scan an unlimited file.
fn skip_line(
    reader: &mut BufReader<File>,
    scanned: &mut usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<bool> {
    let mut seen = false;
    loop {
        if cancelled() {
            return Err(ScopedError::Cancelled);
        }
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            return Ok(seen);
        }
        let end = buf.iter().position(|&byte| byte == b'\n');
        let take = end.map_or(buf.len(), |at| at + 1);
        if buf[..take]
            .iter()
            .any(|byte| matches!(byte,0..=8 | 11..=12 | 14..=31 | 127))
        {
            return Err(ScopedError::Denied("binary file; use bread"));
        }
        *scanned = scanned.saturating_add(take);
        if *scanned > MAX_READ_SCAN_BYTES {
            return Err(ScopedError::Limit(
                "read offset requires scanning more than 32 MiB",
            ));
        }
        reader.consume(take);
        if end.is_some() {
            return Ok(true);
        }
        seen = true;
    }
}
fn next_line(reader: &mut BufReader<File>, cap: usize) -> Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            return Ok(if line.is_empty() { None } else { Some(line) });
        }
        let newline = buf.iter().position(|&byte| byte == b'\n');
        let take = newline.map_or(buf.len(), |at| at + 1);
        if take > cap.saturating_sub(line.len()) {
            return Err(ScopedError::Limit("one line exceeds tool byte limit"));
        }
        line.extend_from_slice(&buf[..take]);
        reader.consume(take);
        if newline.is_some() {
            return Ok(Some(line));
        }
    }
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
        if kind != libc::S_IFDIR && (kind != libc::S_IFREG || info.st_nlink != 1) {
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
        if offset == 0 || limit == 0 || cap == 0 || cap > MAX_BATCH_FILE {
            return Err(ScopedError::Limit("invalid file window"));
        }
        let (dir, name) = self.parent(path)?;
        let file = at(&dir, &name, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
        regular(&file)?;
        let mut reader = BufReader::new(file);
        let mut scanned = 0usize;
        for _ in 1..offset {
            if !skip_line(&mut reader, &mut scanned, cancelled)? {
                return Err(ScopedError::Denied("read offset is past the end of file"));
            }
        }
        let mut output = Vec::new();
        let mut truncated = false;
        for row in 0..=limit {
            if cancelled() {
                return Err(ScopedError::Cancelled);
            }
            if row == limit {
                truncated = !reader.fill_buf()?.is_empty();
                break;
            }
            let Some(line) = next_line(&mut reader, cap)? else {
                if row == 0 && offset != 1 {
                    return Err(ScopedError::Denied("read offset is past the end of file"));
                }
                break;
            };
            let prefix = if numbered {
                format!("{}\t", offset + row).into_bytes()
            } else {
                Vec::new()
            };
            let body = if numbered {
                let mut end = line.len();
                if end > 0 && line[end - 1] == b'\n' {
                    end -= 1;
                }
                if end > 0 && line[end - 1] == b'\r' {
                    end -= 1;
                }
                let mut v = line[..end].to_vec();
                v.push(b'\n');
                v
            } else {
                line
            };
            if output
                .len()
                .saturating_add(prefix.len())
                .saturating_add(body.len())
                > cap
            {
                truncated = true;
                break;
            }
            output.extend(prefix);
            output.extend(body);
        }
        Ok((
            String::from_utf8(output).map_err(|_| ScopedError::Denied("file is not UTF-8 text"))?,
            truncated,
        ))
    }

    fn parent_for_write(&self, path: &str) -> Result<(File, String, CreatedParents)> {
        validate_path(path)?;
        let mut parts: Vec<&str> = path.split('/').collect();
        let leaf = parts
            .pop()
            .ok_or(ScopedError::Denied("missing file path"))?
            .to_owned();
        let mut dir = self.directory.try_clone()?;
        let mut created = CreatedParents::default();
        for part in parts {
            dir = match at(&dir, part, libc::O_RDONLY | libc::O_DIRECTORY, 0) {
                Ok(opened) => opened,
                Err(ScopedError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {
                    let part_c = cstring(part)?;
                    // SAFETY: the directory is pinned and part is one clean component.
                    if unsafe { libc::mkdirat(dir.as_raw_fd(), part_c.as_ptr(), 0o777) } != 0 {
                        let error = io::Error::last_os_error();
                        if error.kind() != io::ErrorKind::AlreadyExists {
                            return Err(error.into());
                        }
                    } else {
                        created.0.push((dir.try_clone()?, part.to_owned()));
                    }
                    at(&dir, part, libc::O_RDONLY | libc::O_DIRECTORY, 0)?
                }
                Err(e) => return Err(e),
            };
        }
        Ok((dir, leaf, created))
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
        if cancel() {
            return Err(ScopedError::Cancelled);
        }
        let (parent, target, created) = self.parent_for_write(path)?;
        let old = match at(&parent, &target, libc::O_RDONLY | libc::O_NONBLOCK, 0) {
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
        if old.is_none() && matches!(kind, StageKind::Update(_)) {
            return Err(ScopedError::Denied("target changed before update"));
        }
        if let (Some(existing), StageKind::Update(expected)) = (&old, kind) {
            let current = stream_bytes(existing.try_clone()?, MAX_BATCH_FILE)?;
            if current != expected {
                return Err(ScopedError::Denied("target changed before update"));
            }
        }
        let mode = old.as_ref().map_or(0o600, |file| {
            file.metadata().map(|m| m.mode() & 0o7777).unwrap_or(0o600)
        });
        let name = format!(
            ".aw-stage-{}-{:016x}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let mut out = at(
            &parent,
            &name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            mode,
        )?;
        let written = (|| -> Result<()> {
            for chunk in data.chunks(64 * 1024) {
                if cancel() {
                    return Err(ScopedError::Cancelled);
                }
                out.write_all(chunk)?;
            }
            // umask might have narrowed mode; reapply the original file mode.
            if old.is_some() {
                // SAFETY: this is the owned staging file descriptor.
                if unsafe { libc::fchmod(out.as_raw_fd(), mode as libc::mode_t) } != 0 {
                    return Err(io::Error::last_os_error().into());
                }
            }
            out.sync_all()?;
            Ok(())
        })();
        drop(out);
        if let Err(error) = written {
            unlink(&parent, &name);
            return Err(error);
        }
        Ok(StagedWrite {
            parent,
            created,
            name: target,
            temp: name,
            expected: match kind {
                StageKind::Update(expected) => Some(expected.to_vec()),
                _ => None,
            },
            create: matches!(kind, StageKind::Create),
            committed: false,
        })
    }
}

fn unlink(parent: &File, path: &str) {
    if let Ok(name) = cstring(path) {
        // SAFETY: a unique staging basename within this pinned parent.
        unsafe {
            libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0);
        }
    }
}
#[derive(Clone, Copy)]
enum StageKind<'a> {
    Create,
    Replace,
    Update(&'a [u8]),
}

/// Each directory created by this staging attempt. On uncommitted Drop,
/// remove only if empty, so unrelated user files are never removed.
#[derive(Default)]
struct CreatedParents(Vec<(File, String)>);
impl CreatedParents {
    fn disarm(&mut self) {
        self.0.clear();
    }
}
impl Drop for CreatedParents {
    fn drop(&mut self) {
        for (parent, name) in self.0.drain(..).rev() {
            if let Ok(name) = cstring(&name) {
                // SAFETY: a clean single component below the pinned parent.
                unsafe {
                    libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR);
                }
            }
        }
    }
}
/// A fully synced sibling file; publishing is a single rename/link operation.
pub struct StagedWrite {
    parent: File,
    created: CreatedParents,
    name: String,
    temp: String,
    expected: Option<Vec<u8>>,
    create: bool,
    committed: bool,
}
impl StagedWrite {
    pub fn commit(mut self) -> Result<()> {
        if let Some(expected) = &self.expected {
            let current = stream_bytes(
                at(
                    &self.parent,
                    &self.name,
                    libc::O_RDONLY | libc::O_NONBLOCK,
                    0,
                )?,
                MAX_BATCH_FILE,
            )?;
            if &current != expected {
                return Err(ScopedError::Denied("target changed before commit"));
            }
        }
        let src = cstring(&self.temp)?;
        let dst = cstring(&self.name)?;
        if self.create {
            // SAFETY: linkat installs only when the target name does not exist.
            if unsafe {
                libc::linkat(
                    self.parent.as_raw_fd(),
                    src.as_ptr(),
                    self.parent.as_raw_fd(),
                    dst.as_ptr(),
                    0,
                )
            } < 0
            {
                return Err(io::Error::last_os_error().into());
            }
            unlink(&self.parent, &self.temp);
        } else {
            // Reject symlink/non-regular targets, even if staged before a swap.
            match at(
                &self.parent,
                &self.name,
                libc::O_RDONLY | libc::O_NONBLOCK,
                0,
            ) {
                Ok(file) => {
                    regular(&file)?;
                }
                Err(ScopedError::Io(e))
                    if e.kind() == io::ErrorKind::NotFound && self.expected.is_none() => {}
                Err(e) => return Err(e),
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
        self.created.disarm();
        let _ = self.parent.sync_all(); // committed; never invite a retry after sync failure
        Ok(())
    }
}
impl Drop for StagedWrite {
    fn drop(&mut self) {
        if !self.committed {
            unlink(&self.parent, &self.temp);
        }
    }
}
