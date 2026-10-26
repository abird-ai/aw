//! Windows counterparts of AW's bounded file windows and staged writes.
//! Directory handles are retained across both staging and publication.
use super::{is_reparse, pin_directory, validate_path, wide, Parent, Result, Root, ScopedError};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::MutexGuard,
};
use windows_sys::Win32::{
    Security::{
        GetFileSecurityW, SetFileSecurityW, DACL_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION,
    },
    Storage::FileSystem::{MoveFileW, ReplaceFileW},
};

const MAX_BATCH_FILE: usize = 64 * 1024 * 1024;
const MAX_READ_SCAN_BYTES: usize = 32 * 1024 * 1024;
static TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
fn regular(file: &File) -> Result<u64> {
    let meta = file.metadata()?;
    if !meta.is_file() || is_reparse(&meta) || super::super::file_link_count(file)? != 1 {
        return Err(ScopedError::Denied(
            "only singly-linked regular files are permitted",
        ));
    }
    Ok(meta.len())
}
fn stream_bytes(file: File, cap: usize) -> Result<Vec<u8>> {
    if regular(&file)? > cap as u64 {
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
/// Cross-process, thread-affine Windows named mutex. Same canonical path
/// hashing and kernel namespace as dotlink, so the tools coordinate.
struct NamedMutationLock(windows_sys::Win32::Foundation::HANDLE);
impl Drop for NamedMutationLock {
    fn drop(&mut self) {
        use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::ReleaseMutex};
        // SAFETY: Drop runs on the blocking worker that acquired the mutex.
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}
pub struct MutationGuard<'a> {
    _local: MutexGuard<'a, ()>,
    _names: Vec<NamedMutationLock>,
}
fn named_lock_key(path: &std::path::Path) -> String {
    let hash = Sha256::digest(path.to_string_lossy().to_lowercase().as_bytes());
    let digest = hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("Global\\abird-dotlink-path-{digest}")
}
/// Own newly created parents until the staged write is safely published.
struct PreparedParent {
    parent: Option<Parent>,
    created: Vec<PathBuf>,
}
impl PreparedParent {
    fn path(&self) -> &std::path::Path {
        &self.parent.as_ref().expect("prepared parent").path
    }
    fn release(mut self) -> (Parent, Vec<PathBuf>) {
        (
            self.parent.take().expect("prepared parent"),
            std::mem::take(&mut self.created),
        )
    }
}
impl Drop for PreparedParent {
    fn drop(&mut self) {
        // Windows directory handles deny delete sharing; release before rmdir.
        drop(self.parent.take());
        for path in self.created.iter().rev() {
            let _ = fs::remove_dir(path); // only empty directories
        }
    }
}
impl Root {
    pub fn lock_mutations<'a>(&'a self, paths: &[String]) -> Result<MutationGuard<'a>> {
        self.lock_mutations_with_cancel(paths, &|| false)
    }
    pub fn lock_mutations_with_cancel<'a>(
        &'a self,
        paths: &[String],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<MutationGuard<'a>> {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT},
            System::Threading::{CreateMutexW, WaitForSingleObject},
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let local = loop {
            if cancelled() {
                return Err(ScopedError::Cancelled);
            }
            match self.serial.try_lock() {
                Ok(lock) => break lock,
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return Err(ScopedError::Denied("file root is poisoned; inspect"))
                }
                Err(std::sync::TryLockError::WouldBlock)
                    if std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(2))
                }
                Err(std::sync::TryLockError::WouldBlock) => {
                    return Err(ScopedError::Denied("local mutation lock timed out"))
                }
            }
        };
        let mut keys = BTreeSet::new();
        for path in paths {
            validate_path(path)?;
            keys.insert(named_lock_key(&self.root.join(path)));
        }
        let mut locks = Vec::new();
        for key in keys {
            let wide = key.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            // SAFETY: NUL-terminated name and an initially unowned mutex.
            let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error().into());
            }
            loop {
                if cancelled() {
                    // SAFETY: an unacquired live mutex handle.
                    unsafe { CloseHandle(handle) };
                    return Err(ScopedError::Cancelled);
                }
                // SAFETY: handle was returned by CreateMutexW, 10ms wait
                // keeps cancellation responsive without async thread migration.
                let code = unsafe { WaitForSingleObject(handle, 10) };
                if code == WAIT_OBJECT_0 || code == WAIT_ABANDONED {
                    locks.push(NamedMutationLock(handle));
                    break;
                }
                if code != WAIT_TIMEOUT || std::time::Instant::now() >= deadline {
                    let error = io::Error::last_os_error();
                    unsafe { CloseHandle(handle) };
                    return if code == WAIT_TIMEOUT {
                        Err(ScopedError::Denied("Windows mutation lock timed out"))
                    } else {
                        Err(error.into())
                    };
                }
            }
        }
        Ok(MutationGuard {
            _local: local,
            _names: locks,
        })
    }
    /// Metadata through a validated parent chain; neither directory nor
    /// regular-file handles permit following reparse points.
    pub fn entry_info(&self, path: &str) -> Result<(u64, Option<std::time::SystemTime>)> {
        let (_parent, target) = self.parent(path)?;
        let file = pin_directory(&target).or_else(|_| super::super::open_nofollow_read(&target))?;
        let metadata = file.metadata()?;
        if is_reparse(&metadata) || (!metadata.is_file() && !metadata.is_dir()) {
            return Err(ScopedError::Denied("unsupported filesystem entry"));
        }
        if metadata.is_file() && super::super::file_link_count(&file)? != 1 {
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

    pub fn read_bytes(&self, path: &str, cap: usize) -> Result<Vec<u8>> {
        if cap > MAX_BATCH_FILE {
            return Err(ScopedError::Limit("binary read cap exceeds 64 MiB"));
        }
        let (_parent, target) = self.parent(path)?;
        stream_bytes(super::super::open_nofollow_read(&target)?, cap)
    }
    /// Bounded binary byte window; no file-sized buffer.
    pub fn read_binary_window(
        &self,
        path: &str,
        offset: u64,
        limit: usize,
    ) -> Result<(Vec<u8>, u64, bool)> {
        if limit == 0 || limit > MAX_BATCH_FILE {
            return Err(ScopedError::Limit("invalid binary window limit"));
        }
        let (_parent, target) = self.parent(path)?;
        let mut file = super::super::open_nofollow_read(&target)?;
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
        let (_parent, target) = self.parent(path)?;
        let file = super::super::open_nofollow_read(&target)?;
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
    fn parent_for_write(&self, path: &str) -> Result<(PreparedParent, PathBuf)> {
        validate_path(path)?;
        let mut parts: Vec<&str> = path.split('/').collect();
        parts.pop();
        let mut prepared = PreparedParent {
            parent: Some(Parent {
                path: self.root.clone(),
                directories: Vec::new(),
            }),
            created: Vec::new(),
        };
        for part in parts {
            let parent = prepared.parent.as_mut().expect("prepared parent");
            parent.path.push(part);
            let next = match pin_directory(&parent.path) {
                Ok(opened) => opened,
                Err(ScopedError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {
                    fs::create_dir(&parent.path)?;
                    prepared.created.push(parent.path.clone());
                    pin_directory(&parent.path)?
                }
                Err(e) => return Err(e),
            };
            parent.directories.push(next);
        }
        let target = self.root.join(path);
        Ok((prepared, target))
    }
    pub fn stage_create(
        &self,
        path: &str,
        data: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<StagedWrite> {
        self.stage(path, data, StageKind::Create, cancel)
    }
    pub fn stage_replace(
        &self,
        path: &str,
        data: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<StagedWrite> {
        self.stage(path, data, StageKind::Replace, cancel)
    }
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
        let (prepared, target) = self.parent_for_write(path)?;
        let old = match super::super::open_nofollow_read(&target) {
            Ok(file) => {
                regular(&file)?;
                if matches!(kind, StageKind::Create) {
                    return Err(ScopedError::Denied("target already exists"));
                }
                Some(file)
            }
            Err(ScopedError::Io(e)) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        if old.is_none() && matches!(kind, StageKind::Update(_)) {
            return Err(ScopedError::Denied("target changed before update"));
        }
        if let (Some(file), StageKind::Update(expected)) = (&old, kind) {
            if stream_bytes(file.try_clone()?, MAX_BATCH_FILE)? != expected {
                return Err(ScopedError::Denied("target changed before update"));
            }
        }
        // Secure the still-empty staging file with the existing target's
        // restrictive DACL before writing *any* user bytes. A default file
        // created under a more permissive parent must not leak a private file.
        let dacl = old.as_ref().map(|_| file_dacl(&target)).transpose()?;
        let temp = prepared.path().join(format!(
            ".aw-stage-{}-{:016x}.tmp",
            std::process::id(),
            TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        let result = (|| -> Result<()> {
            if let Some(ref dacl) = dacl {
                protect_temporary(&temp, dacl)?;
            }
            for chunk in data.chunks(65536) {
                if cancel() {
                    return Err(ScopedError::Cancelled);
                }
                out.write_all(chunk)?;
            }
            out.sync_all()?;
            Ok(())
        })();
        drop(out);
        if let Err(error) = result {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        let (parent, created) = prepared.release();
        Ok(StagedWrite {
            parent: Some(parent),
            created,
            target,
            temp,
            expected: match kind {
                StageKind::Update(bytes) => Some(bytes.to_vec()),
                _ => None,
            },
            create: matches!(kind, StageKind::Create),
            existed: old.is_some(),
            committed: false,
            preserve_on_failure: false,
        })
    }
}
#[derive(Clone, Copy)]
enum StageKind<'a> {
    Create,
    Replace,
    Update(&'a [u8]),
}
/// Staging on Windows requires security-preserving installation; MoveFileEx
/// would inherit the parent directory ACL rather than the existing target.
pub struct StagedWrite {
    parent: Option<Parent>,
    created: Vec<PathBuf>,
    target: PathBuf,
    temp: PathBuf,
    expected: Option<Vec<u8>>,
    create: bool,
    existed: bool,
    committed: bool,
    preserve_on_failure: bool,
}
impl StagedWrite {
    pub fn commit(mut self) -> Result<()> {
        if let Some(expected) = &self.expected {
            if stream_bytes(
                super::super::open_nofollow_read(&self.target)?,
                MAX_BATCH_FILE,
            )? != *expected
            {
                return Err(ScopedError::Denied("target changed before commit"));
            }
        }
        match super::super::open_nofollow_read(&self.target) {
            Ok(file) => {
                regular(&file)?;
                if self.create || !self.existed {
                    return Err(ScopedError::Denied("target appeared before create"));
                }
            }
            Err(ScopedError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {
                if self.existed {
                    return Err(ScopedError::Denied("replacement target disappeared"));
                }
            }
            Err(e) => return Err(e),
        }
        if !self.existed {
            // MoveFileW cannot overwrite an occupied name, unlike MoveFileExW.
            install_create(&self.temp, &self.target)?;
            self.committed = true;
            return Ok(());
        }
        let backup = self.target.with_file_name(format!(
            ".aw-backup-{}-{:016x}.tmp",
            std::process::id(),
            TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        let dst = wide(&self.target);
        let src = wide(&self.temp);
        let bak = wide(&backup);
        // SAFETY: paths are NUL-terminated, live, and the zero flag requires
        // Windows to preserve ACL and file metadata or fail instead.
        if unsafe {
            ReplaceFileW(
                dst.as_ptr(),
                src.as_ptr(),
                bak.as_ptr(),
                0,
                std::ptr::null(),
                std::ptr::null(),
            )
        } == 0
        {
            let problem = io::Error::last_os_error();
            // ReplaceFileW can move the original to the backup even on failure.
            // Never delete uncertain state.
            self.preserve_on_failure = !recover_backup(&self.target, &backup);
            return Err(problem.into());
        }
        self.committed = true;
        let _ = fs::remove_file(&backup); // after commit; best effort
        Ok(())
    }
}
impl Drop for StagedWrite {
    fn drop(&mut self) {
        if !self.committed && !self.preserve_on_failure {
            let _ = fs::remove_file(&self.temp);
            drop(self.parent.take());
            for path in self.created.iter().rev() {
                let _ = fs::remove_dir(path); // never remove a nonempty parent
            }
        }
    }
}

/// The two-call size probe is bounded; no file data is written until this
/// descriptor is successfully applied to the staging name.
fn file_dacl(path: &std::path::Path) -> Result<Vec<u8>> {
    let path = wide(path);
    let mut required = 0_u32;
    // SAFETY: wide path is terminated; querying the buffer length is allowed.
    unsafe {
        GetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            0,
            &mut required,
        );
    }
    if required == 0 || required > 1024 * 1024 {
        return Err(ScopedError::Denied("cannot read existing Windows ACL"));
    }
    let mut descriptor = vec![0_u8; required as usize];
    // SAFETY: descriptor is writable and contains the kernel-reported length.
    if unsafe {
        GetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION,
            descriptor.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err(io::Error::last_os_error().into());
    }
    Ok(descriptor)
}
fn protect_temporary(path: &std::path::Path, dacl: &[u8]) -> Result<()> {
    let path = wide(path);
    // SAFETY: this is the complete self-relative security descriptor from
    // GetFileSecurityW. The staging file is empty until the ACL is set.
    if unsafe {
        SetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            dacl.as_ptr().cast_mut().cast(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}
fn install_create(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    let src = wide(src);
    let dst = wide(dst);
    // SAFETY: both paths are live, NUL-terminated and validated by the caller.
    if unsafe { MoveFileW(src.as_ptr(), dst.as_ptr()) } == 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}
fn recover_backup(target: &std::path::Path, backup: &std::path::Path) -> bool {
    let exists = |path: &std::path::Path| match fs::symlink_metadata(path) {
        Ok(_) => Some(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Some(false),
        Err(_) => None,
    };
    match (exists(target), exists(backup)) {
        (Some(true), Some(false)) => true,
        (Some(false), Some(true)) => install_create(backup, target).is_ok(),
        _ => false,
    }
}
