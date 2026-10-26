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
fn skip_line(reader: &mut BufReader<File>) -> io::Result<bool> {
    let mut seen = false;
    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            return Ok(seen);
        }
        if let Some(at) = buf.iter().position(|&byte| byte == b'\n') {
            reader.consume(at + 1);
            return Ok(true);
        }
        let n = buf.len();
        reader.consume(n);
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
fn lock_parent(root: &Root, path: &str) -> Result<File> {
    validate_path(path)?;
    let mut dir = root.directory.try_clone()?;
    for part in path
        .split('/')
        .collect::<Vec<_>>()
        .split_last()
        .map_or(&[][..], |(_, parents)| parents)
    {
        match at(&dir, part, libc::O_RDONLY | libc::O_DIRECTORY, 0) {
            Ok(next) => dir = next,
            Err(ScopedError::Io(error)) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        }
    }
    Ok(dir)
}
impl Root {
    pub fn lock_mutations<'a>(&'a self, paths: &[String]) -> Result<MutationGuard<'a>> {
        let local = self.lock()?;
        let mut directories = BTreeMap::<(u64, u64), File>::new();
        // Every writer of the same granted root takes one stable OS lock,
        // including when a write creates missing parent directories. Locking
        // only the nearest parent lets a new child directory change lock
        // identity between competing processes.
        let root_lock = at(&self.directory, ".", libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        let root_meta = root_lock.metadata()?;
        directories.insert((root_meta.dev(), root_meta.ino()), root_lock);
        for path in paths {
            let dir = lock_parent(self, path)?;
            let m = dir.metadata()?;
            directories.entry((m.dev(), m.ino())).or_insert(dir);
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut held = Vec::new();
        for (_, dir) in directories {
            loop {
                // SAFETY: this is a live directory fd; flock is advisory and
                // never requires filesystem lockfiles or a separate state path.
                if unsafe { libc::flock(dir.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                    held.push(dir);
                    break;
                }
                let err = io::Error::last_os_error();
                match err.kind() {
                    io::ErrorKind::Interrupted => continue,
                    io::ErrorKind::WouldBlock if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    io::ErrorKind::WouldBlock => {
                        return Err(ScopedError::Denied("workspace mutation lock timed out"))
                    }
                    _ => return Err(err.into()),
                }
            }
        }
        Ok(MutationGuard {
            _local: local,
            _directories: held,
        })
    }

    /// Metadata is resolved below the pinned root; never by an ambient path.
    pub fn entry_info(&self, path: &str) -> Result<(u64, Option<std::time::SystemTime>)> {
        let (dir, name) = self.parent(path)?;
        let file = at(&dir, &name, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() && !metadata.is_dir() {
            return Err(ScopedError::Denied("unsupported filesystem entry"));
        }
        if metadata.is_file() && metadata.nlink() != 1 {
            return Err(ScopedError::Denied("multiply linked file is not granted"));
        }
        Ok((
            if metadata.is_file() {
                metadata.len()
            } else {
                0
            },
            metadata.modified().ok(),
        ))
    }

    /// Bounded bytes from a pinned regular file. Unlike `read`, the caller
    /// chooses its own hard limit; binary tools can opt into larger files.
    pub fn read_bytes(&self, path: &str, cap: usize) -> Result<Vec<u8>> {
        if cap > MAX_BATCH_FILE {
            return Err(ScopedError::Limit("binary read cap exceeds 64 MiB"));
        }
        let (dir, name) = self.parent(path)?;
        stream_bytes(at(&dir, &name, libc::O_RDONLY | libc::O_NONBLOCK, 0)?, cap)
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
        if offset == 0 || limit == 0 || cap == 0 || cap > MAX_BATCH_FILE {
            return Err(ScopedError::Limit("invalid file window"));
        }
        let (dir, name) = self.parent(path)?;
        let file = at(&dir, &name, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
        regular(&file)?;
        let mut reader = BufReader::new(file);
        for _ in 1..offset {
            if !skip_line(&mut reader)? {
                return Err(ScopedError::Denied("read offset is past the end of file"));
            }
        }
        let mut output = Vec::new();
        let mut truncated = false;
        for row in 0..=limit {
            if row == limit {
                truncated = skip_line(&mut reader)?;
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

    fn parent_for_write(&self, path: &str) -> Result<(File, String)> {
        validate_path(path)?;
        let mut parts: Vec<&str> = path.split('/').collect();
        let leaf = parts
            .pop()
            .ok_or(ScopedError::Denied("missing file path"))?
            .to_owned();
        let mut dir = self.directory.try_clone()?;
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
                    }
                    at(&dir, part, libc::O_RDONLY | libc::O_DIRECTORY, 0)?
                }
                Err(e) => return Err(e),
            };
        }
        Ok((dir, leaf))
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
        let (parent, target) = self.parent_for_write(path)?;
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

/// A fully synced sibling file; publishing is a single rename/link operation.
pub struct StagedWrite {
    parent: File,
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
