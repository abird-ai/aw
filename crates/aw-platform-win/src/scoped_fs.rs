//! Reparse-point-aware scoped filesystem implementation for Windows.
//! This is a local single-user safety boundary, not a tenant sandbox against
//! administrators or same-user native writers.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::windows::{ffi::OsStrExt, fs::MetadataExt, io::AsRawHandle},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, MutexGuard,
    },
};

use windows_sys::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{
        GetFileInformationByHandle, MoveFileExW, BY_HANDLE_FILE_INFORMATION,
        FILE_ATTRIBUTE_REPARSE_POINT, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    },
};

const MAX_FILE: usize = 1024 * 1024;
static TEMP: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub enum ScopedError {
    Io(io::Error),
    Denied(&'static str),
    Limit(&'static str),
    Cancelled,
}

impl From<io::Error> for ScopedError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub type Result<T> = std::result::Result<T, ScopedError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    File,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
}

#[derive(Clone, Debug)]
pub struct Listing {
    pub path: String,
    pub entries: Vec<Entry>,
    pub incomplete: bool,
}

pub struct Root {
    root: PathBuf,
    serial: Mutex<()>,
}

pub fn validate_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.len() > 4096
        || path.contains('\0')
        || path.contains('\\')
        || path.contains(':')
        || path.starts_with('/')
        || path
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
        || path.chars().any(char::is_control)
        || Path::new(path)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ScopedError::Denied(
            "path must be a clean relative path under the granted root",
        ));
    }
    Ok(())
}

fn is_reparse(meta: &fs::Metadata) -> bool {
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn file_identity(file: &File) -> io::Result<(u32, u64)> {
    unsafe {
        let mut info = std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>();
        if GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut info) == 0 {
            return Err(io::Error::last_os_error());
        }
        let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
        Ok((info.dwVolumeSerialNumber, index))
    }
}

impl Root {
    pub fn open(root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        let meta = fs::symlink_metadata(&root)?;
        if !meta.is_dir() || is_reparse(&meta) {
            return Err(ScopedError::Denied(
                "tool root must be a real directory, not a reparse point",
            ));
        }
        Ok(Self {
            root,
            serial: Mutex::new(()),
        })
    }

    pub fn lock(&self) -> Result<MutexGuard<'_, ()>> {
        self.serial
            .lock()
            .map_err(|_| ScopedError::Denied("file root is poisoned; stop and inspect"))
    }

    fn parent(&self, path: &str) -> Result<(PathBuf, PathBuf)> {
        validate_path(path)?;
        let mut parts = path.split('/').collect::<Vec<_>>();
        let leaf = parts.pop().ok_or(ScopedError::Denied("path missing"))?;
        let mut dir = self.root.clone();
        for part in parts {
            dir.push(part);
            let meta = fs::symlink_metadata(&dir)?;
            if !meta.is_dir() || is_reparse(&meta) {
                return Err(ScopedError::Denied(
                    "path traverses a non-directory or reparse point",
                ));
            }
        }
        Ok((dir.clone(), dir.join(leaf)))
    }

    fn contents(file: &mut File) -> Result<String> {
        let meta = file.metadata()?;
        if !meta.is_file() || super::file_link_count(file)? != 1 || is_reparse(&meta) {
            return Err(ScopedError::Denied(
                "only singly-linked regular files are permitted",
            ));
        }
        if meta.len() > MAX_FILE as u64 {
            return Err(ScopedError::Limit(
                "file exceeds 1 MiB; use a smaller source",
            ));
        }
        let mut bytes = Vec::new();
        file.take((MAX_FILE + 1) as u64).read_to_end(&mut bytes)?;
        if bytes.len() > MAX_FILE {
            return Err(ScopedError::Limit("file grew beyond 1 MiB"));
        }
        String::from_utf8(bytes).map_err(|_| ScopedError::Denied("file is not UTF-8 text"))
    }

    pub fn read(&self, path: &str) -> Result<String> {
        let (_, path) = self.parent(path)?;
        let mut file = super::open_nofollow_read(&path)?;
        Self::contents(&mut file)
    }

    pub fn list(&self, path: &str) -> Result<Listing> {
        let dir = if path.is_empty() {
            self.root.clone()
        } else {
            let (_, target) = self.parent(path)?;
            let meta = fs::symlink_metadata(&target)?;
            if !meta.is_dir() || is_reparse(&meta) {
                return Err(ScopedError::Denied(
                    "directory path is not a real directory",
                ));
            }
            target
        };

        let mut entries = Vec::new();
        let mut incomplete = false;
        for (scanned, item) in fs::read_dir(dir)?.enumerate() {
            if scanned >= 4096 {
                incomplete = true;
                break;
            }
            let item = match item {
                Ok(item) => item,
                Err(_) => {
                    incomplete = true;
                    continue;
                }
            };
            let Some(name) = item.file_name().to_str().map(str::to_owned) else {
                incomplete = true;
                continue;
            };
            if validate_path(&name).is_err() {
                incomplete = true;
                continue;
            }
            let meta = match fs::symlink_metadata(item.path()) {
                Ok(meta) => meta,
                Err(_) => {
                    incomplete = true;
                    continue;
                }
            };
            let kind = if is_reparse(&meta) {
                EntryKind::Unsupported
            } else if meta.is_dir() {
                EntryKind::Directory
            } else if meta.is_file() {
                let file = match super::open_nofollow_read(&item.path()) {
                    Ok(file) => file,
                    Err(_) => {
                        incomplete = true;
                        continue;
                    }
                };
                if super::file_link_count(&file).unwrap_or(2) == 1 {
                    EntryKind::File
                } else {
                    EntryKind::Unsupported
                }
            } else {
                EntryKind::Unsupported
            };
            let joined = if path.is_empty() {
                name.clone()
            } else {
                format!("{path}/{name}")
            };
            if joined.len() > 4096 {
                incomplete = true;
                continue;
            }
            entries.push(Entry {
                name,
                path: joined,
                kind,
            });
            if entries.len() >= 512 {
                incomplete = true;
                break;
            }
        }

        entries.sort_by(|a, b| {
            let rank = |kind: &EntryKind| match kind {
                EntryKind::Directory => 0,
                EntryKind::File => 1,
                EntryKind::Unsupported => 2,
            };
            rank(&a.kind).cmp(&rank(&b.kind)).then(a.name.cmp(&b.name))
        });
        Ok(Listing {
            path: path.into(),
            entries,
            incomplete,
        })
    }

    pub fn create(&self, path: &str, content: &str, cancelled: &dyn Fn() -> bool) -> Result<()> {
        let (_, target) = self.parent(path)?;
        if cancelled() {
            return Err(ScopedError::Cancelled);
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        Ok(())
    }

    pub fn edit(
        &self,
        path: &str,
        old: &str,
        new: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<()> {
        if old.is_empty() {
            return Err(ScopedError::Denied("empty edit match"));
        }
        let (dir, target) = self.parent(path)?;
        let mut source = super::open_nofollow_read(&target)?;
        let before_identity = file_identity(&source)?;
        let text = Self::contents(&mut source)?;
        if text.match_indices(old).count() != 1 {
            return Err(ScopedError::Denied(
                "edit needs exactly one match; no fuzzy replacement performed",
            ));
        }
        let replacement = text.replacen(old, new, 1);
        if replacement.len() > MAX_FILE {
            return Err(ScopedError::Limit("edited file would exceed 1 MiB"));
        }

        let temp = dir.join(format!(
            ".aw-edit-{}-{}",
            std::process::id(),
            TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        let result = (|| {
            out.write_all(replacement.as_bytes())?;
            out.sync_all()?;

            let mut current = super::open_nofollow_read(&target)?;
            if file_identity(&current)? != before_identity || Self::contents(&mut current)? != text
            {
                return Err(ScopedError::Denied(
                    "file changed concurrently; edit not committed",
                ));
            }
            if cancelled() {
                return Err(ScopedError::Cancelled);
            }

            let target_w = wide(&target);
            let temp_w = wide(&temp);
            // SAFETY: both paths are valid NUL-terminated UTF-16 buffers and
            // remain alive for the duration of the call.
            let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;
            if unsafe { MoveFileExW(temp_w.as_ptr(), target_w.as_ptr(), flags) } == 0 {
                return Err(io::Error::last_os_error().into());
            }
            Ok(())
        })();

        let _ = fs::remove_file(&temp);
        result
    }
}
