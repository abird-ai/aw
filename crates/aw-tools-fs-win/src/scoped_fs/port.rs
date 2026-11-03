//! Windows counterparts of AW's bounded file windows and staged writes.
//! Directory handles are retained across both staging and publication.
use super::{is_reparse, pin_directory, validate_path, wide, Parent, Result, Root, ScopedError};
use aw_tools::filesystem::private_artifact::{
    Kind as ArtifactKind, Lease as ArtifactLease, ObjectIdentity, Record,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::{
    fs::{self, File},
    io::{self, BufReader, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::{Arc, MutexGuard},
};
use windows_sys::Win32::{
    Security::{GetFileSecurityW, DACL_SECURITY_INFORMATION, SECURITY_ATTRIBUTES},
    Storage::FileSystem::{
        CreateFileW, MoveFileW, ReplaceFileW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL,
        FILE_GENERIC_WRITE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    },
};

const MAX_BATCH_FILE: usize = 64 * 1024 * 1024;
static TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
fn regular(file: &File) -> Result<u64> {
    let meta = file.metadata()?;
    if !meta.is_file() || is_reparse(&meta) {
        return Err(ScopedError::Denied("only regular files are permitted"));
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
fn digest_file(file: File) -> Result<(u64, [u8; 32])> {
    let size = regular(&file)?;
    if size > MAX_BATCH_FILE as u64 {
        return Err(ScopedError::Limit("binary source exceeds 64 MiB"));
    }
    let mut reader = file;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .ok_or(ScopedError::Limit("binary source size overflow"))?;
        if total > MAX_BATCH_FILE as u64 {
            return Err(ScopedError::Limit("binary source grew past 64 MiB"));
        }
        digest.update(&buffer[..n]);
    }
    Ok((total, digest.finalize().into()))
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
/// Match dotlink's Windows mutation name identity exactly: invariant
/// Windows UTF-16 uppercase mapping, then SHA-256 over little-endian
/// code units. Rust Unicode case-folding would choose different mutexes
/// for case-equivalent NTFS names.
fn fold_invariant_utf16(source: &[u16], label: &'static str) -> Result<Vec<u16>> {
    use std::ptr;
    use windows_sys::Win32::Globalization::{
        LCMapStringEx, LCMAP_UPPERCASE, LOCALE_NAME_INVARIANT,
    };
    let count = i32::try_from(source.len()).map_err(|_| ScopedError::Limit(label))?;
    // SAFETY: fixed UTF-16 input; first call sizes destination.
    let required = unsafe {
        LCMapStringEx(
            LOCALE_NAME_INVARIANT,
            LCMAP_UPPERCASE,
            source.as_ptr(),
            count,
            ptr::null_mut(),
            0,
            ptr::null(),
            ptr::null(),
            0,
        )
    };
    if required <= 0 {
        return Err(io::Error::last_os_error().into());
    }
    let mut mapped = vec![0u16; required as usize];
    // SAFETY: destination matches kernel-provided length.
    let written = unsafe {
        LCMapStringEx(
            LOCALE_NAME_INVARIANT,
            LCMAP_UPPERCASE,
            source.as_ptr(),
            count,
            mapped.as_mut_ptr(),
            required,
            ptr::null(),
            ptr::null(),
            0,
        )
    };
    if written != required {
        return Err(io::Error::last_os_error().into());
    }
    Ok(mapped)
}

/// Match dotlink's Windows mutation name identity exactly: invariant Windows
/// UTF-16 uppercase mapping encoded as little-endian bytes.
pub(super) fn folded_namespace_key(path: &std::path::Path) -> Result<Vec<u8>> {
    use std::os::windows::ffi::OsStrExt as _;
    let source = path.as_os_str().encode_wide().collect::<Vec<_>>();
    let mapped = fold_invariant_utf16(&source, "mutation path exceeds UTF-16 length")?;
    let mut key = Vec::with_capacity(mapped.len() * 2);
    for unit in mapped {
        key.extend_from_slice(&unit.to_le_bytes());
    }
    Ok(key)
}

pub(super) fn folded_component_units(value: &str) -> Result<Vec<u16>> {
    let source = value.encode_utf16().collect::<Vec<_>>();
    fold_invariant_utf16(&source, "path segment too long")
}

pub(super) fn folded_component_label(value: &str) -> Result<String> {
    let mapped = folded_component_units(value)?;
    let mut label = String::with_capacity(1 + mapped.len() * 4);
    label.push('~');
    for unit in mapped {
        use std::fmt::Write as _;
        write!(&mut label, "{unit:04x}").expect("String writes cannot fail");
    }
    Ok(label)
}

pub(super) fn folded_model_path(model: &str) -> Result<String> {
    super::validate_path(model)?;
    if model == "." {
        return Ok(".".into());
    }
    model
        .split('/')
        .map(folded_component_label)
        .collect::<Result<Vec<_>>>()
        .map(|parts| parts.join("/"))
}

fn named_lock_key(path: &std::path::Path, scope: &str) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(folded_namespace_key(path)?);
    let digest = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("Global\\abird-dotlink-{scope}-{digest}"))
}

fn directory_identity(file: &File) -> Result<(u32, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let mut info = unsafe { std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok((
        info.dwVolumeSerialNumber,
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
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
        parent: &Parent,
        parent_path: String,
        name: &str,
        object: ObjectIdentity,
        kind: ArtifactKind,
    ) -> Result<ArtifactLease> {
        let parent = self.parent_artifact_identity(parent)?;
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

struct PreparedParent {
    parent: Option<Parent>,
}
impl PreparedParent {
    fn path(&self) -> &std::path::Path {
        &self.parent.as_ref().expect("prepared parent").path
    }
    fn release(mut self) -> Parent {
        self.parent.take().expect("prepared parent")
    }
}
impl Root {
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
            let (parent, target) = match self.parent(&path) {
                Ok(value) => value,
                Err(ScopedError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                    self.private
                        .retire_record(&record)
                        .map_err(|_| ScopedError::Denied("cannot retire stale private artifact"))?;
                    continue;
                }
                Err(error) => return Err(error),
            };
            if self.parent_artifact_identity(&parent)? != record.parent {
                self.private
                    .retire_record(&record)
                    .map_err(|_| ScopedError::Denied("cannot retire stale private artifact"))?;
                continue;
            }
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
                FILE_SHARE_WRITE,
            };
            let opened = match fs::OpenOptions::new()
                .access_mode(0)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
                .open(&target)
            {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    self.private
                        .retire_record(&record)
                        .map_err(|_| ScopedError::Denied("cannot retire stale private artifact"))?;
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            let (volume, object) = super::super::file_identity(&opened)?;
            if (ObjectIdentity {
                volume: u64::from(volume),
                object,
            }) != record.object
            {
                self.private
                    .retire_record(&record)
                    .map_err(|_| ScopedError::Denied("cannot retire stale private artifact"))?;
                continue;
            }
            drop(opened);
            let mut work = aw_tools::filesystem::lifecycle::WorkBudget::default();
            lifecycle::cleanup_private(&target, 0, &mut work, &|| false)?;
            if target.exists() {
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
        // The newest dotlink protocol acquires lexical pathname admissions
        // before canonical mutation locks. Use separate namespaces so one
        // request can own both without self-deadlocking.
        let mut admissions = BTreeSet::new();
        let mut targets = BTreeSet::new();
        for path in paths {
            validate_path(path)?;
            let lexical = self.root.join(path);
            admissions.insert(named_lock_key(&lexical, "admission")?);
            targets.insert(named_lock_key(&lexical, "path")?);
        }
        let mut locks = Vec::with_capacity(admissions.len() + targets.len());
        // One absolute deadline applies to both tiers; cancellation is
        // checked during every kernel mutex wait.
        for key in admissions.into_iter().chain(targets) {
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
    /// No-follow metadata on a pinned Windows parent chain. Unavailable
    /// status-change timestamps stay None, never synthesized from birth time.
    pub fn entry_meta(&self, path: &str) -> Result<aw_tools::filesystem::EntryMeta> {
        self.ensure_public_path(path)?;
        use aw_tools::filesystem::{EntryMeta, Kind};
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            FILE_SHARE_WRITE,
        };
        let (_parent, target) = self.parent(path)?;
        let file = std::fs::OpenOptions::new()
            .access_mode(0)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(&target)?;
        let stat = file.metadata()?;
        let kind = if stat.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            if stat.file_type().is_symlink() {
                Kind::Symlink
            } else {
                Kind::Other
            }
        } else if stat.is_dir() {
            Kind::Dir
        } else if stat.is_file() {
            Kind::File
        } else {
            Kind::Other
        };
        Ok(EntryMeta {
            kind,
            size: if kind == Kind::File {
                Some(stat.len())
            } else {
                None
            },
            mtime: stat.modified().ok(),
            ctime: None,
            btime: stat.created().ok(),
            mode: None,
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
        self.ensure_public_path(path)?;
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
        self.ensure_public_path(path)?;
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
        self.ensure_public_path(path)?;
        let (_parent, target) = self.parent(path)?;
        let file = super::super::open_nofollow_read(&target)?;
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
        let (_parent, target) = self.parent(path)?;
        let file = super::super::open_nofollow_read(&target)?;
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
    pub fn open_regular(&self, path: &str) -> Result<File> {
        self.ensure_public_path(path)?;
        let (_parent, target) = self.parent(path)?;
        let file = super::super::open_nofollow_read(&target)?;
        regular(&file)?;
        Ok(file)
    }
    pub fn stage_stream_create(
        &self,
        path: &str,
        writer: impl FnOnce(&mut File) -> Result<()>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<StagedWrite> {
        self.stage_with(path, StageKind::Create, cancelled, writer)
    }
    pub fn stage_stream_update(
        &self,
        path: &str,
        expected: (u64, [u8; 32]),
        writer: impl FnOnce(&mut File) -> Result<()>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<StagedWrite> {
        self.stage_with(path, StageKind::UpdateDigest(expected), cancelled, writer)
    }
    fn stage(
        &self,
        path: &str,
        data: &[u8],
        kind: StageKind<'_>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<StagedWrite> {
        if data.len() > MAX_BATCH_FILE {
            return Err(ScopedError::Limit("staged file exceeds 64 MiB"));
        }
        self.stage_with(path, kind, cancelled, |out| {
            for chunk in data.chunks(64 * 1024) {
                if cancelled() {
                    return Err(ScopedError::Cancelled);
                }
                out.write_all(chunk)?;
            }
            Ok(())
        })
    }
    fn missing_parent(&self, path: &str) -> Result<Option<(Parent, Vec<String>, String)>> {
        validate_path(path)?;
        let mut parts = path.split('/').collect::<Vec<_>>();
        let leaf = parts.pop().ok_or(ScopedError::Denied("missing target"))?;
        let mut parent = Parent {
            path: self.root.clone(),
            directories: Vec::new(),
        };
        for (idx, part) in parts.iter().enumerate() {
            let next = parent.path.join(part);
            match pin_directory(&next) {
                Ok(opened) => {
                    parent.path = next;
                    parent.directories.push(opened);
                }
                Err(ScopedError::Io(ref err)) if err.kind() == io::ErrorKind::NotFound => {
                    let missing = parts[idx..]
                        .iter()
                        .map(|s| (*s).to_owned())
                        .collect::<Vec<_>>();
                    if missing.len() > aw_tools::filesystem::lifecycle::MAX_LIFECYCLE_DEPTH {
                        return Err(ScopedError::Limit("missing parent suffix too deep"));
                    }
                    return Ok(Some((parent, missing, leaf.to_owned())));
                }
                Err(err) => return Err(err),
            }
        }
        Ok(None)
    }
    fn stage_missing_suffix(
        &self,
        path: &str,
        parent: Parent,
        missing: Vec<String>,
        leaf: String,
        cancelled: &dyn Fn() -> bool,
        writer: impl FnOnce(&mut File) -> Result<()>,
    ) -> Result<StagedWrite> {
        let temp = parent.path.join(format!(
            ".aw-stage-{}-{:016x}.tmp",
            std::process::id(),
            TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&temp)?;
        let opened = pin_directory(&temp)?;
        let identity = directory_identity(&opened)?;
        drop(opened);
        let artifact = match self.reserve_private_artifact(
            &parent,
            existing_parent_model_path(path, missing.len()),
            temp.file_name()
                .and_then(|value| value.to_str())
                .ok_or(ScopedError::Denied("private stage name is not reusable"))?,
            ObjectIdentity {
                volume: u64::from(identity.0),
                object: identity.1,
            },
            ArtifactKind::Stage,
        ) {
            Ok(artifact) => artifact,
            Err(error) => {
                lifecycle::cleanup_stage_tree(&temp, identity);
                return Err(error);
            }
        };
        let result = (|| -> Result<()> {
            let mut target = temp.clone();
            let mut handles = Vec::with_capacity(missing.len());
            for part in missing.iter().skip(1) {
                if cancelled() {
                    return Err(ScopedError::Cancelled);
                }
                target.push(part);
                fs::create_dir(&target)?;
                handles.push(pin_directory(&target)?);
            }
            let filename = target.join(&leaf);
            let mut output = secure_staging_file(&filename, None)?;
            writer(&mut output)?;
            if output.metadata()?.len() > MAX_BATCH_FILE as u64 {
                return Err(ScopedError::Limit("staged file exceeds 64 MiB"));
            }
            if cancelled() {
                return Err(ScopedError::Cancelled);
            }
            output.sync_all()?;
            drop(output);
            drop(handles);
            Ok(())
        })();
        if let Err(error) = result {
            lifecycle::cleanup_stage_tree(&temp, identity);
            if !temp.exists() {
                let _ = artifact.retire();
            }
            return Err(error);
        }
        let published = parent.path.join(&missing[0]);
        let parent_identity = self.parent_artifact_identity(&parent)?;
        Ok(StagedWrite {
            parent: Some(parent),
            target: published,
            temp,
            expected: None,
            create: true,
            existed: false,
            committed: false,
            preserve_on_failure: false,
            temp_dir_identity: Some(identity),
            artifact: Some(artifact),
            private: self.private.clone(),
            parent_identity,
            parent_model_path: existing_parent_model_path(path, missing.len()),
        })
    }
    fn stage_missing_group(
        &self,
        parent: Parent,
        existing_model: String,
        publication: String,
        entries: Vec<(Vec<String>, String, &[u8])>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<StagedWrite> {
        let temp = parent.path.join(format!(
            ".aw-stage-{}-{:016x}.tmp",
            std::process::id(),
            TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&temp)?;
        let opened = pin_directory(&temp)?;
        let identity = directory_identity(&opened)?;
        drop(opened);
        let artifact = match self.reserve_private_artifact(
            &parent,
            existing_model.clone(),
            temp.file_name()
                .and_then(|value| value.to_str())
                .ok_or(ScopedError::Denied("private stage name is not reusable"))?,
            ObjectIdentity {
                volume: u64::from(identity.0),
                object: identity.1,
            },
            ArtifactKind::Stage,
        ) {
            Ok(artifact) => artifact,
            Err(error) => {
                lifecycle::cleanup_stage_tree(&temp, identity);
                return Err(error);
            }
        };
        let result = (|| -> Result<()> {
            let mut handles = vec![pin_directory(&temp)?];
            for (directories, leaf, bytes) in entries {
                if cancelled() {
                    return Err(ScopedError::Cancelled);
                }
                let mut target = temp.clone();
                for component in directories {
                    target.push(&component);
                    match fs::create_dir(&target) {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                        Err(error) => return Err(error.into()),
                    }
                    handles.push(pin_directory(&target)?);
                }
                let filename = target.join(&leaf);
                let mut output = secure_staging_file(&filename, None)?;
                for chunk in bytes.chunks(64 * 1024) {
                    if cancelled() {
                        return Err(ScopedError::Cancelled);
                    }
                    output.write_all(chunk)?;
                }
                if output.metadata()?.len() > MAX_BATCH_FILE as u64 {
                    return Err(ScopedError::Limit("staged output exceeds 64 MiB"));
                }
                output.sync_all()?;
                drop(output);
                handles.push(pin_directory(&target)?);
            }
            if cancelled() {
                return Err(ScopedError::Cancelled);
            }
            drop(handles);
            Ok(())
        })();
        if let Err(error) = result {
            lifecycle::cleanup_stage_tree(&temp, identity);
            if !temp.exists() {
                let _ = artifact.retire();
            }
            return Err(error);
        }
        let published = parent.path.join(&publication);
        let parent_identity = self.parent_artifact_identity(&parent)?;
        Ok(StagedWrite {
            parent: Some(parent),
            target: published,
            temp,
            expected: None,
            create: true,
            existed: false,
            committed: false,
            preserve_on_failure: false,
            temp_dir_identity: Some(identity),
            artifact: Some(artifact),
            private: self.private.clone(),
            parent_identity,
            parent_model_path: existing_model,
        })
    }

    pub fn stage_create_many(
        &self,
        files: &[aw_tools::filesystem::backend::CreateFile<'_>],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<aw_tools::filesystem::backend::CreateStage<StagedWrite>>> {
        if files.is_empty() {
            return Ok(Vec::new());
        }
        let mut total = 0usize;
        let mut components = Vec::with_capacity(files.len());
        for file in files {
            validate_path(file.path)?;
            self.ensure_public_path(file.path)?;
            total = total
                .checked_add(file.bytes.len())
                .ok_or(ScopedError::Limit("staged create batch size overflow"))?;
            if total > MAX_BATCH_FILE {
                return Err(ScopedError::Limit("staged create batch exceeds 64 MiB"));
            }
            components.push(
                file.path
                    .split('/')
                    .map(|component| folded_namespace_key(std::path::Path::new(component)))
                    .collect::<Result<Vec<_>>>()?,
            );
        }
        for left in 0..components.len() {
            for right in left + 1..components.len() {
                let a = &components[left];
                let b = &components[right];
                if (a.len() <= b.len() && a.iter().zip(b).all(|(x, y)| x == y))
                    || (b.len() <= a.len() && b.iter().zip(a).all(|(x, y)| x == y))
                {
                    return Err(ScopedError::Denied("create paths overlap"));
                }
            }
        }

        struct Group<'a> {
            first: usize,
            inputs: Vec<usize>,
            parent: Parent,
            existing_model: String,
            publication: String,
            entries: Vec<(Vec<String>, String, &'a [u8])>,
        }
        enum Unit<'a> {
            Single {
                first: usize,
                path: &'a str,
                bytes: &'a [u8],
            },
            Group(Group<'a>),
        }
        impl Unit<'_> {
            fn first(&self) -> usize {
                match self {
                    Self::Single { first, .. } | Self::Group(Group { first, .. }) => *first,
                }
            }
        }

        let mut units = Vec::<Unit<'_>>::new();
        let mut groups = BTreeMap::<Vec<u8>, usize>::new();
        for (index, file) in files.iter().enumerate() {
            if cancelled() {
                return Err(ScopedError::Cancelled);
            }
            match self.missing_parent(file.path)? {
                None => units.push(Unit::Single {
                    first: index,
                    path: file.path,
                    bytes: file.bytes,
                }),
                Some((parent, missing, leaf)) => {
                    let existing_model = existing_parent_model_path(file.path, missing.len());
                    let publication = missing[0].clone();
                    let key = folded_namespace_key(&parent.path.join(&publication))?;
                    let directories = missing.into_iter().skip(1).collect::<Vec<_>>();
                    if let Some(unit) = groups.get(&key).copied() {
                        let Unit::Group(group) = &mut units[unit] else {
                            unreachable!("group index points to group unit");
                        };
                        group.inputs.push(index);
                        group.entries.push((directories, leaf, file.bytes));
                    } else {
                        let unit = units.len();
                        units.push(Unit::Group(Group {
                            first: index,
                            inputs: vec![index],
                            parent,
                            existing_model,
                            publication,
                            entries: vec![(directories, leaf, file.bytes)],
                        }));
                        groups.insert(key, unit);
                    }
                }
            }
        }
        units.sort_by_key(Unit::first);
        let mut staged = Vec::with_capacity(units.len());
        for unit in units {
            if cancelled() {
                return Err(ScopedError::Cancelled);
            }
            match unit {
                Unit::Single { first, path, bytes } => {
                    staged.push(aw_tools::filesystem::backend::CreateStage {
                        first,
                        inputs: vec![first],
                        stage: self.stage_create(path, bytes, cancelled)?,
                    });
                }
                Unit::Group(group) => staged.push(aw_tools::filesystem::backend::CreateStage {
                    first: group.first,
                    inputs: group.inputs,
                    stage: self.stage_missing_group(
                        group.parent,
                        group.existing_model,
                        group.publication,
                        group.entries,
                        cancelled,
                    )?,
                }),
            }
        }
        Ok(staged)
    }

    fn stage_with(
        &self,
        path: &str,
        kind: StageKind<'_>,
        cancelled: &dyn Fn() -> bool,
        writer: impl FnOnce(&mut File) -> Result<()>,
    ) -> Result<StagedWrite> {
        self.ensure_public_path(path)?;
        if cancelled() {
            return Err(ScopedError::Cancelled);
        }
        if let Some((parent, missing, leaf)) = self.missing_parent(path)? {
            if matches!(kind, StageKind::Update(_) | StageKind::UpdateDigest(_)) {
                return Err(ScopedError::Denied("target changed before update"));
            }
            return self.stage_missing_suffix(path, parent, missing, leaf, cancelled, writer);
        }
        let (existing, target) = self.parent(path)?;
        let prepared = PreparedParent {
            parent: Some(existing),
        };
        let old = match if matches!(kind, StageKind::Replace) {
            super::super::open_nofollow_metadata(&target)
        } else {
            super::super::open_nofollow_read(&target)
        } {
            Ok(file) => {
                regular(&file)?;
                if matches!(kind, StageKind::Create) {
                    return Err(ScopedError::Denied("target already exists"));
                }
                Some(file)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        if old.is_none() && matches!(kind, StageKind::Update(_) | StageKind::UpdateDigest(_)) {
            return Err(ScopedError::Denied("target changed before update"));
        }
        if let Some(ref file) = old {
            match kind {
                StageKind::Update(bytes)
                    if stream_bytes(file.try_clone()?, MAX_BATCH_FILE)? != bytes =>
                {
                    return Err(ScopedError::Denied("target changed before update"))
                }
                StageKind::UpdateDigest(expected)
                    if digest_file(file.try_clone()?)? != expected =>
                {
                    return Err(ScopedError::Denied("binary source changed before staging"))
                }
                _ => {}
            }
        }
        let dacl = old.as_ref().map(|_| file_dacl(&target)).transpose()?;
        let temp = prepared.path().join(format!(
            ".aw-stage-{}-{:016x}.tmp",
            std::process::id(),
            TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let mut out = secure_staging_file(&temp, dacl.as_deref())?;
        let (stage_volume, stage_object) = super::super::file_identity(&out)?;
        let parent_ref = prepared.parent.as_ref().expect("prepared parent");
        let artifact = match self.reserve_private_artifact(
            parent_ref,
            parent_model_path(path).to_owned(),
            temp.file_name()
                .and_then(|value| value.to_str())
                .ok_or(ScopedError::Denied("private stage name is not reusable"))?,
            ObjectIdentity {
                volume: u64::from(stage_volume),
                object: stage_object,
            },
            ArtifactKind::Stage,
        ) {
            Ok(artifact) => artifact,
            Err(error) => {
                drop(out);
                let _ = fs::remove_file(&temp);
                return Err(error);
            }
        };
        let result = (|| -> Result<()> {
            writer(&mut out)?;
            if out.metadata()?.len() > MAX_BATCH_FILE as u64 {
                return Err(ScopedError::Limit("staged output exceeds 64 MiB"));
            }
            if cancelled() {
                return Err(ScopedError::Cancelled);
            }
            out.sync_all()?;
            Ok(())
        })();
        drop(out);
        if let Err(error) = result {
            let _ = fs::remove_file(&temp);
            if !temp.exists() {
                let _ = artifact.retire();
            }
            return Err(error);
        }
        let parent_identity =
            self.parent_artifact_identity(prepared.parent.as_ref().expect("prepared parent"))?;
        let parent = prepared.release();
        Ok(StagedWrite {
            parent: Some(parent),
            target,
            temp,
            expected: match kind {
                StageKind::Update(bytes) => Some(Expected::Bytes(bytes.to_vec())),
                StageKind::UpdateDigest(digest) => Some(Expected::Digest(digest)),
                _ => None,
            },
            create: matches!(kind, StageKind::Create),
            existed: old.is_some(),
            committed: false,
            preserve_on_failure: false,
            temp_dir_identity: None,
            artifact: Some(artifact),
            private: self.private.clone(),
            parent_identity,
            parent_model_path: parent_model_path(path).to_owned(),
        })
    }
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
/// Staging on Windows requires security-preserving installation; MoveFileEx
/// would inherit the parent directory ACL rather than the existing target.
pub struct StagedWrite {
    parent: Option<Parent>,
    target: PathBuf,
    temp: PathBuf,
    expected: Option<Expected>,
    create: bool,
    existed: bool,
    committed: bool,
    preserve_on_failure: bool,
    temp_dir_identity: Option<(u32, u64)>,
    artifact: Option<ArtifactLease>,
    private: Arc<aw_tools::filesystem::private_artifact::Registry>,
    parent_identity: ObjectIdentity,
    parent_model_path: String,
}
impl StagedWrite {
    pub fn commit(mut self) -> Result<()> {
        if let Some(expected) = self.temp_dir_identity {
            let opened = pin_directory(&self.temp)?;
            if directory_identity(&opened)? != expected {
                return Err(ScopedError::Denied("staged suffix directory changed"));
            }
            drop(opened);
        }
        if let Some(expected) = &self.expected {
            let file = super::super::open_nofollow_read(&self.target)?;
            let valid = match expected {
                Expected::Bytes(bytes) => stream_bytes(file, MAX_BATCH_FILE)? == *bytes,
                Expected::Digest(digest) => digest_file(file)? == *digest,
            };
            if !valid {
                return Err(ScopedError::Denied("target changed before commit"));
            }
        }
        // A normal write-only replacement needs metadata evidence,
        // not access to read the target's payload.
        let final_entry = if self.expected.is_none() {
            super::super::open_nofollow_metadata(&self.target)
        } else {
            super::super::open_nofollow_read(&self.target)
        };
        let mut original_identity = None;
        match final_entry {
            Ok(file) => {
                regular(&file)?;
                let (volume, object) = super::super::file_identity(&file)?;
                original_identity = Some(ObjectIdentity {
                    volume: u64::from(volume),
                    object,
                });
                if self.create || !self.existed {
                    return Err(ScopedError::Denied("target appeared before create"));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                if self.existed {
                    return Err(ScopedError::Denied("replacement target disappeared"));
                }
            }
            Err(e) => return Err(e.into()),
        }
        if !self.existed {
            // MoveFileW cannot overwrite an occupied name, unlike MoveFileExW.
            install_create(&self.temp, &self.target)?;
            self.committed = true;
            if !self.temp.exists() {
                if let Some(artifact) = self.artifact.take() {
                    let _ = artifact.retire();
                }
            }
            return Ok(());
        }
        let backup = self.target.with_file_name(format!(
            ".aw-backup-{}-{:016x}.tmp",
            std::process::id(),
            TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        let backup_name = backup
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or(ScopedError::Denied("private backup name is not reusable"))?;
        let backup_artifact = self
            .private
            .reserve(Record {
                kind: ArtifactKind::Backup,
                parent_path: self.parent_model_path.clone(),
                parent: self.parent_identity,
                name: backup_name.to_owned(),
                object: original_identity.ok_or(ScopedError::Denied(
                    "replacement target identity unavailable",
                ))?,
            })
            .map_err(|_| ScopedError::Denied("cannot durably reserve replacement backup"))?;
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
            if !backup.exists() {
                let _ = backup_artifact.retire();
            }
            return Err(problem.into());
        }
        self.committed = true;
        if !self.temp.exists() {
            if let Some(artifact) = self.artifact.take() {
                let _ = artifact.retire();
            }
        }
        match fs::remove_file(&backup) {
            Ok(()) => {
                let _ = backup_artifact.retire();
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let _ = backup_artifact.retire();
            }
            Err(_) => {
                // Keep exact recovery ownership for the surviving backup.
                drop(backup_artifact);
            }
        }
        Ok(())
    }
}
impl Drop for StagedWrite {
    fn drop(&mut self) {
        if !self.committed && !self.preserve_on_failure {
            if let Some(identity) = self.temp_dir_identity {
                lifecycle::cleanup_stage_tree(&self.temp, identity);
            } else {
                let _ = fs::remove_file(&self.temp);
            }
            if !self.temp.exists() {
                if let Some(artifact) = self.artifact.take() {
                    let _ = artifact.retire();
                }
            }
            drop(self.parent.take());
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
fn secure_staging_file(path: &std::path::Path, dacl: Option<&[u8]>) -> Result<File> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    let name = wide(path);
    // Windows accepts a self-relative security descriptor here. The kernel
    // assigns its DACL atomically with creation; no widening window exists.
    let mut attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: dacl
            .map_or(std::ptr::null_mut(), |data| data.as_ptr().cast_mut().cast()),
        bInheritHandle: 0,
    };
    let attributes = if dacl.is_some() {
        &mut attrs as *mut _
    } else {
        std::ptr::null_mut()
    };
    // SAFETY: NUL-terminated pathname, live SECURITY_ATTRIBUTES/descriptor,
    // CREATE_NEW refuses overwriting an existing path. Handle is owned below.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if handle.is_null() || handle as isize == -1 {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: a successful CreateFileW returns exactly one owned handle.
    Ok(File::from(unsafe { OwnedHandle::from_raw_handle(handle) }))
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

#[path = "port/lifecycle.rs"]
pub(super) mod lifecycle;

#[cfg(test)]
mod mutation_lock_key_tests {
    use super::*;
    #[test]
    fn windows_case_equivalent_paths_share_admission_identity() {
        let a = std::path::Path::new(r"C:\AW\Workspace\FILE.TXT");
        let b = std::path::Path::new(r"c:\aw\workspace\file.txt");
        assert_eq!(
            folded_namespace_key(a).unwrap(),
            folded_namespace_key(b).unwrap(),
            "mutation planning and lock admission must share Windows case identity"
        );
        assert_eq!(
            named_lock_key(a, "path").unwrap(),
            named_lock_key(b, "path").unwrap()
        );
        let parent_a = std::path::Path::new(r"C:\AW\Workspace\Dir");
        let parent_b = std::path::Path::new(r"c:\aw\workspace\dir");
        assert_eq!(
            named_lock_key(parent_a, "overlap").unwrap(),
            named_lock_key(parent_b, "overlap").unwrap()
        );
        assert_eq!(
            named_lock_key(a, "admission").unwrap(),
            named_lock_key(b, "admission").unwrap()
        );
        assert_ne!(
            named_lock_key(a, "admission").unwrap(),
            named_lock_key(a, "path").unwrap()
        );
    }
}
