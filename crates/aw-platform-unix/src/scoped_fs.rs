//! Descriptor-relative scoped filesystem implementation for Unix.
//! This preserves aw's no-follow/single-link invariants but is not a tenant
//! sandbox against administrators, same-UID native code, mount changes, or
//! non-cooperating writers.

use std::{
    ffi::{CStr, CString},
    fs::File,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, IntoRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Component, Path},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, MutexGuard,
    },
};

const MAX_FILE: usize = 1024 * 1024;
static TEMP: AtomicU64 = AtomicU64::new(1);

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "emscripten",
    target_os = "hurd",
    target_os = "redox",
    target_os = "dragonfly"
))]
fn errno_location() -> *mut libc::c_int {
    // SAFETY: libc exposes the thread-local errno pointer for these targets.
    unsafe { libc::__errno_location() }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos",
    target_os = "freebsd"
))]
fn errno_location() -> *mut libc::c_int {
    // SAFETY: libc exposes the thread-local errno pointer for Apple/FreeBSD.
    unsafe { libc::__error() }
}

#[cfg(any(target_os = "netbsd", target_os = "openbsd"))]
fn errno_location() -> *mut libc::c_int {
    // SAFETY: libc exposes the thread-local errno pointer for NetBSD/OpenBSD.
    unsafe { libc::__errno() }
}

#[cfg(any(target_os = "solaris", target_os = "illumos"))]
fn errno_location() -> *mut libc::c_int {
    // SAFETY: libc exposes the thread-local errno pointer for Solaris/illumos.
    unsafe { libc::___errno() }
}

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
    directory: File,
    serial: Mutex<()>,
}

pub fn validate_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.len() > 4096
        || path.contains('\0')
        || path.contains('\\')
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

fn cstring(value: &str) -> Result<CString> {
    CString::new(value).map_err(|_| ScopedError::Denied("invalid path"))
}

fn at(dir: &File, name: &str, flags: i32, mode: u32) -> Result<File> {
    let name = cstring(name)?;
    // Keep mode at C unsigned-int width for variadic argument promotion;
    // Darwin's narrower mode_t cannot be passed directly to openat.
    // SAFETY: dir is live and name is NUL-terminated. The result is checked and
    // owned exactly once below.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            mode,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: fd is newly owned by this function.
    Ok(unsafe { File::from_raw_fd(fd) })
}

impl Root {
    pub fn open(root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        let directory = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(root)?;
        Ok(Self {
            directory,
            serial: Mutex::new(()),
        })
    }

    pub fn lock(&self) -> Result<MutexGuard<'_, ()>> {
        self.serial
            .lock()
            .map_err(|_| ScopedError::Denied("file root is poisoned; stop and inspect"))
    }

    fn parent(&self, path: &str) -> Result<(File, String)> {
        validate_path(path)?;
        let mut parts = path.split('/').collect::<Vec<_>>();
        let leaf = parts
            .pop()
            .ok_or(ScopedError::Denied("path missing"))?
            .to_owned();
        let mut dir = self.directory.try_clone()?;
        for part in parts {
            dir = at(&dir, part, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        }
        Ok((dir, leaf))
    }

    fn contents(file: &mut File) -> Result<String> {
        let meta = file.metadata()?;
        if !meta.is_file() || meta.nlink() != 1 {
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
        let (dir, name) = self.parent(path)?;
        let mut file = at(&dir, &name, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
        Self::contents(&mut file)
    }

    pub fn list(&self, path: &str) -> Result<Listing> {
        let dir = if path.is_empty() {
            self.directory.try_clone()?
        } else {
            let (dir, name) = self.parent(path)?;
            at(&dir, &name, libc::O_RDONLY | libc::O_DIRECTORY, 0)?
        };
        let fd = dir.into_raw_fd();
        // SAFETY: fd is a newly-owned directory descriptor. fdopendir takes
        // ownership on success.
        let stream = unsafe { libc::fdopendir(fd) };
        if stream.is_null() {
            let error = io::Error::last_os_error();
            // SAFETY: fdopendir failed and did not take ownership.
            unsafe { libc::close(fd) };
            return Err(error.into());
        }

        struct Dir(*mut libc::DIR);
        impl Drop for Dir {
            fn drop(&mut self) {
                // SAFETY: this wrapper uniquely owns the DIR pointer.
                unsafe { libc::closedir(self.0) };
            }
        }

        let stream = Dir(stream);
        // SAFETY: stream is live.
        unsafe { libc::rewinddir(stream.0) };
        let mut entries = Vec::new();
        let mut incomplete = false;

        for scanned in 0..4096 {
            if scanned == 4095 {
                incomplete = true;
            }
            unsafe {
                *errno_location() = 0;
            }
            // SAFETY: stream remains owned by Dir for this loop.
            let item = unsafe { libc::readdir(stream.0) };
            if item.is_null() {
                let errno = unsafe { *errno_location() };
                if errno != 0 {
                    return Err(io::Error::from_raw_os_error(errno).into());
                }
                break;
            }

            // SAFETY: readdir returned a live dirent until the next call.
            let raw = unsafe { CStr::from_ptr((*item).d_name.as_ptr()) };
            let name = match raw.to_str() {
                Ok(name) => name,
                Err(_) => {
                    incomplete = true;
                    continue;
                }
            };
            if name == "." || name == ".." {
                continue;
            }
            if validate_path(name).is_err() {
                incomplete = true;
                continue;
            }

            let mut info = std::mem::MaybeUninit::<libc::stat>::uninit();
            // SAFETY: descriptor/name are live; fstatat initializes on success.
            if unsafe {
                libc::fstatat(
                    fd,
                    raw.as_ptr(),
                    info.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } < 0
            {
                incomplete = true;
                continue;
            }
            let info = unsafe { info.assume_init() };
            let kind = match info.st_mode & libc::S_IFMT {
                libc::S_IFDIR => EntryKind::Directory,
                libc::S_IFREG if info.st_nlink == 1 => EntryKind::File,
                _ => EntryKind::Unsupported,
            };
            let joined = if path.is_empty() {
                name.to_owned()
            } else {
                format!("{path}/{name}")
            };
            if joined.len() > 4096 {
                incomplete = true;
                continue;
            }
            entries.push(Entry {
                name: name.into(),
                path: joined,
                kind,
            });
            if entries.len() >= 512 || scanned == 4095 {
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
        let (dir, name) = self.parent(path)?;
        if cancelled() {
            return Err(ScopedError::Cancelled);
        }
        let mut file = at(
            &dir,
            &name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        dir.sync_all()?;
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
        let (dir, name) = self.parent(path)?;
        let mut source = at(&dir, &name, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
        let before = source.metadata()?;
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

        let temp = format!(
            ".aw-edit-{}-{}",
            std::process::id(),
            TEMP.fetch_add(1, Ordering::Relaxed)
        );
        let mut out = at(
            &dir,
            &temp,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            before.mode() & 0o777,
        )?;
        let result = (|| {
            out.write_all(replacement.as_bytes())?;
            out.sync_all()?;
            let mut current = at(&dir, &name, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
            let now = current.metadata()?;
            if before.ino() != now.ino()
                || before.dev() != now.dev()
                || Self::contents(&mut current)? != text
            {
                return Err(ScopedError::Denied(
                    "file changed concurrently; edit not committed",
                ));
            }
            if cancelled() {
                return Err(ScopedError::Cancelled);
            }
            let from = cstring(&temp)?;
            let to = cstring(&name)?;
            // SAFETY: both names and the held directory descriptor are live.
            if unsafe {
                libc::renameat(dir.as_raw_fd(), from.as_ptr(), dir.as_raw_fd(), to.as_ptr())
            } < 0
            {
                return Err(io::Error::last_os_error().into());
            }
            dir.sync_all()?;
            Ok(())
        })();

        let temp = cstring(&temp)?;
        // SAFETY: cleanup addresses only our unique temporary basename.
        unsafe {
            libc::unlinkat(dir.as_raw_fd(), temp.as_ptr(), 0);
        }
        result
    }
}
