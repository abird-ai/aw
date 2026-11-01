//! Descriptor-relative scoped filesystem implementation for Unix.
//! This preserves aw's no-follow/single-link invariants but is not a tenant
//! sandbox against administrators, same-UID native code, mount changes, or
//! non-cooperating writers.

use std::{
    ffi::{CStr, CString, OsStr},
    fs::File,
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd, IntoRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

pub mod capability;
mod port;
pub use port::{MutationGuard, StagedWrite};

const MAX_FILE: usize = 1024 * 1024;

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
    Symlink,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
    /// Native-hidden attribute or dot-prefix, obtained during the no-follow walk.
    pub hidden: bool,
}

#[derive(Clone, Debug)]
pub struct Listing {
    pub path: String,
    pub entries: Vec<Entry>,
    pub incomplete: bool,
    /// A filename could not be represented as an exact JSON/UTF-8 path.
    pub path_encoding: bool,
}

pub struct Root {
    directory: File,
    serial: Mutex<()>,
    private: Arc<aw_tools::filesystem::private_artifact::Registry>,
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

#[cfg(target_os = "linux")]
#[repr(C)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}

fn at(dir: &File, name: &str, flags: i32, mode: u32) -> Result<File> {
    let name = cstring(name)?;
    // A single component below an already-pinned granted directory. Linux
    // additionally enforces no traversal through mountpoints with openat2.
    #[cfg(target_os = "linux")]
    let fd = {
        const RESOLVE_NO_XDEV: u64 = 0x01;
        const RESOLVE_NO_SYMLINKS: u64 = 0x04;
        const RESOLVE_BENEATH: u64 = 0x08;
        let how = OpenHow {
            flags: (flags | libc::O_CLOEXEC | libc::O_NOFOLLOW) as u64,
            mode: mode as u64,
            resolve: RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_XDEV,
        };
        // SAFETY: live descriptor, NUL-terminated name, fixed-layout input.
        // The kernel copies OpenHow; returned fd is checked before ownership.
        unsafe {
            libc::syscall(
                libc::SYS_openat2,
                dir.as_raw_fd(),
                name.as_ptr(),
                &how as *const OpenHow,
                std::mem::size_of::<OpenHow>(),
            ) as i32
        }
    };
    #[cfg(not(target_os = "linux"))]
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
    // SAFETY: successful syscall returned one owned descriptor.
    let opened = unsafe { File::from_raw_fd(fd) };
    // macOS and other Unix targets have no Linux openat2. Each component
    // remains no-follow; refuse crossing a mount boundary at this lowest
    // pinned-descriptor layer rather than following a path fallback.
    #[cfg(not(target_os = "linux"))]
    if opened.metadata()?.dev() != dir.metadata()?.dev() {
        return Err(io::Error::from_raw_os_error(libc::EXDEV).into());
    }
    Ok(opened)
}

fn open_root_component(dir: &File, name: &OsStr) -> Result<File> {
    let name =
        CString::new(name.as_bytes()).map_err(|_| ScopedError::Denied("invalid root path"))?;
    #[cfg(target_os = "linux")]
    let fd = {
        const RESOLVE_NO_SYMLINKS: u64 = 0x04;
        const RESOLVE_BENEATH: u64 = 0x08;
        let how = OpenHow {
            flags: (libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW) as u64,
            mode: 0,
            resolve: RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS,
        };
        unsafe {
            libc::syscall(
                libc::SYS_openat2,
                dir.as_raw_fd(),
                name.as_ptr(),
                &how as *const OpenHow,
                std::mem::size_of::<OpenHow>(),
            ) as i32
        }
    };
    #[cfg(not(target_os = "linux"))]
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn pin_root_directory(path: &Path) -> Result<File> {
    if !path.is_absolute() {
        return Err(ScopedError::Denied("capability root must be absolute"));
    }
    let mut directory = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(part) => {
                directory = open_root_component(&directory, part)?;
            }
            _ => return Err(ScopedError::Denied("capability root is not canonical")),
        }
    }
    Ok(directory)
}

#[cfg(target_os = "linux")]
fn descriptor_path(file: &File) -> Result<PathBuf> {
    std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd())).map_err(Into::into)
}

#[cfg(target_os = "macos")]
fn descriptor_path(file: &File) -> Result<PathBuf> {
    use std::{ffi::CStr, os::unix::ffi::OsStrExt};
    let mut bytes = vec![0u8; libc::PATH_MAX as usize];
    let result = unsafe {
        libc::fcntl(
            file.as_raw_fd(),
            libc::F_GETPATH,
            bytes.as_mut_ptr().cast::<libc::c_char>(),
        )
    };
    if result < 0 {
        return Err(io::Error::last_os_error().into());
    }
    let path = unsafe { CStr::from_ptr(bytes.as_ptr().cast::<libc::c_char>()) };
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(path.to_bytes())))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn descriptor_path(_file: &File) -> Result<PathBuf> {
    Err(ScopedError::Denied(
        "final symlink resolution is unsupported on this Unix platform",
    ))
}

fn stat_leaf(parent: &File, name: &str, nofollow: bool) -> Result<libc::stat> {
    let name = cstring(name)?;
    let mut info = std::mem::MaybeUninit::<libc::stat>::uninit();
    let flags = if nofollow {
        libc::AT_SYMLINK_NOFOLLOW
    } else {
        0
    };
    if unsafe { libc::fstatat(parent.as_raw_fd(), name.as_ptr(), info.as_mut_ptr(), flags) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(unsafe { info.assume_init() })
}

fn root_relative_utf8(root: &Path, target: &Path) -> Result<String> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| ScopedError::Denied("resolved content target escapes granted root"))?;
    if relative.as_os_str().is_empty() {
        return Err(ScopedError::Denied("content target is not a regular file"));
    }
    let mut result = String::new();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(ScopedError::Denied(
                "resolved content target is not reusable",
            ));
        };
        let part = part
            .to_str()
            .ok_or(ScopedError::Denied("resolved content target is not UTF-8"))?;
        if !result.is_empty() {
            result.push('/');
        }
        result.push_str(part);
    }
    Ok(result)
}

impl Root {
    pub fn open(root: &Path) -> Result<Self> {
        Self::open_with_private_artifacts(
            root,
            aw_tools::filesystem::private_artifact::Registry::memory(),
        )
    }

    pub fn open_with_private_artifacts(
        root: &Path,
        private: Arc<aw_tools::filesystem::private_artifact::Registry>,
    ) -> Result<Self> {
        let root = root.canonicalize()?;
        let expected = std::fs::metadata(&root)?;
        if !expected.is_dir() {
            return Err(ScopedError::Denied("granted root is not a directory"));
        }
        let directory = pin_root_directory(&root)?;
        let actual = directory.metadata()?;
        if (expected.dev(), expected.ino()) != (actual.dev(), actual.ino()) {
            return Err(ScopedError::Denied("granted root changed while opening"));
        }
        Ok(Self {
            directory,
            serial: Mutex::new(()),
            private,
        })
    }

    pub fn lock(&self) -> Result<MutexGuard<'_, ()>> {
        self.serial
            .lock()
            .map_err(|_| ScopedError::Denied("file root is poisoned; stop and inspect"))
    }

    fn artifact_identity(
        metadata: &std::fs::Metadata,
    ) -> aw_tools::filesystem::private_artifact::ObjectIdentity {
        aw_tools::filesystem::private_artifact::ObjectIdentity {
            volume: metadata.dev(),
            object: metadata.ino(),
        }
    }

    pub fn private_artifacts(&self) -> &Arc<aw_tools::filesystem::private_artifact::Registry> {
        &self.private
    }

    pub fn is_private_path(&self, path: &str) -> Result<bool> {
        validate_path(path)?;
        let (parent, name) = match self.parent(path) {
            Ok(value) => value,
            Err(ScopedError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(false)
            }
            Err(error) => return Err(error),
        };
        let parent_id = Self::artifact_identity(&parent.metadata()?);
        let stat = match stat_leaf(&parent, &name, true) {
            Ok(stat) => stat,
            Err(ScopedError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(false)
            }
            Err(error) => return Err(error),
        };
        let object = aw_tools::filesystem::private_artifact::ObjectIdentity {
            volume: stat.st_dev as u64,
            object: stat.st_ino as u64,
        };
        self.private
            .owns(parent_id, &name, object)
            .map_err(|_| ScopedError::Denied("private mutation artifact registry unavailable"))
    }

    pub(crate) fn ensure_public_path(&self, path: &str) -> Result<()> {
        if self.is_private_path(path)? {
            return Err(ScopedError::Denied(
                "private mutation artifact is not externally addressable",
            ));
        }
        Ok(())
    }

    fn resolve_final_path(
        &self,
        path: &str,
        allow_missing: bool,
        expected_kind: libc::mode_t,
        label: &'static str,
    ) -> Result<String> {
        validate_path(path)?;
        let (parent, name) = match self.parent(path) {
            Ok(value) => value,
            Err(ScopedError::Io(error))
                if allow_missing && error.kind() == io::ErrorKind::NotFound =>
            {
                return Ok(path.to_owned());
            }
            Err(error) => return Err(error),
        };
        let entry = match stat_leaf(&parent, &name, true) {
            Ok(entry) => entry,
            Err(ScopedError::Io(error))
                if allow_missing && error.kind() == io::ErrorKind::NotFound =>
            {
                return Ok(path.to_owned());
            }
            Err(error) => return Err(error),
        };
        let entry_kind = entry.st_mode & libc::S_IFMT;
        if entry_kind == expected_kind {
            return Ok(path.to_owned());
        }
        if entry_kind != libc::S_IFLNK {
            return Err(ScopedError::Denied(label));
        }

        // Follow only after proving that the caller's intermediate namespace was
        // traversed without symlinks. fstatat is metadata-only and therefore
        // does not turn write-only authority into a content read.
        let followed = match stat_leaf(&parent, &name, false) {
            Ok(value) => value,
            Err(ScopedError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                return Err(ScopedError::Denied(
                    "dangling final symlink is not a missing target",
                ));
            }
            Err(error) => return Err(error),
        };
        if followed.st_mode & libc::S_IFMT != expected_kind {
            return Err(ScopedError::Denied(label));
        }
        let root_meta = self.directory.metadata()?;
        if followed.st_dev as u64 != root_meta.dev() {
            return Err(ScopedError::Denied(
                "final symlink target crosses filesystem boundary",
            ));
        }

        // Resolve the current pathname of the already-pinned parent and root,
        // then verify the canonical spelling identifies the same inode observed
        // through the pinned parent. A rename/swap race becomes a mismatch, not
        // authorization of a different object.
        let parent_path = descriptor_path(&parent)?;
        let root_path = descriptor_path(&self.directory)?;
        let canonical = std::fs::canonicalize(parent_path.join(&name)).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                ScopedError::Denied("dangling final symlink is not a missing target")
            } else {
                ScopedError::Io(error)
            }
        })?;
        let resolved = root_relative_utf8(&root_path, &canonical)?;
        let (resolved_parent, resolved_name) = self.parent(&resolved)?;
        let verified = stat_leaf(&resolved_parent, &resolved_name, true)?;
        if verified.st_mode & libc::S_IFMT != expected_kind
            || verified.st_dev != followed.st_dev
            || verified.st_ino != followed.st_ino
        {
            return Err(ScopedError::Denied(
                "final symlink target changed during resolution",
            ));
        }
        Ok(resolved)
    }

    /// Resolve an explicitly named final content symlink without weakening the
    /// ordinary no-follow open path. Intermediate components are pinned first.
    pub fn resolve_content_path(&self, path: &str, allow_missing: bool) -> Result<String> {
        self.resolve_final_path(
            path,
            allow_missing,
            libc::S_IFREG,
            "final symlink target is not a regular file",
        )
    }

    pub fn resolve_directory_path(&self, path: &str) -> Result<String> {
        self.resolve_final_path(
            path,
            false,
            libc::S_IFDIR,
            "discovery root is not a directory",
        )
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
        if !meta.is_file() {
            return Err(ScopedError::Denied("only regular files are permitted"));
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
        self.ensure_public_path(path)?;
        let (dir, name) = self.parent(path)?;
        let mut file = at(&dir, &name, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
        Self::contents(&mut file)
    }

    pub fn list(&self, path: &str) -> Result<Listing> {
        self.list_with_limit(path, 512)
    }

    pub fn list_with_limit(&self, path: &str, max_entries: usize) -> Result<Listing> {
        if !path.is_empty() {
            self.ensure_public_path(path)?;
        }
        if !(1..=16384).contains(&max_entries) {
            return Err(ScopedError::Limit("listing limit outside 1..=16384"));
        }
        let dir = if path.is_empty() {
            // A fresh open file description has an independent readdir cursor.
            // `try_clone()` shares the directory offset and races with
            // parallel root listings after dropping the old global read lock.
            at(&self.directory, ".", libc::O_RDONLY | libc::O_DIRECTORY, 0)?
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
        let mut path_encoding = false;

        for scanned in 0..65536 {
            if scanned == 65535 {
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
                    path_encoding = true;
                    continue;
                }
            };
            if name == "." || name == ".." {
                continue;
            }
            if validate_path(name).is_err() {
                path_encoding = true;
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
                libc::S_IFREG => EntryKind::File,
                libc::S_IFLNK => EntryKind::Symlink,
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
                hidden: name.starts_with('.'),
            });
            if entries.len() >= max_entries || scanned == 65535 {
                incomplete = true;
                break;
            }
        }

        entries.sort_by(|a, b| {
            let rank = |kind: &EntryKind| match kind {
                EntryKind::Directory => 0,
                EntryKind::File => 1,
                EntryKind::Symlink => 2,
                EntryKind::Unsupported => 3,
            };
            rank(&a.kind).cmp(&rank(&b.kind)).then(a.name.cmp(&b.name))
        });
        Ok(Listing {
            path: path.into(),
            entries,
            incomplete,
            path_encoding,
        })
    }

    /// Retained scoped API: create is still create-only but publication now
    /// uses the same atomic stage/commit path as the model-facing tools.
    pub fn create(&self, path: &str, content: &str, cancelled: &dyn Fn() -> bool) -> Result<()> {
        let _mutation = self.lock_mutations(&[path.to_owned()])?;
        self.stage_create(path, content.as_bytes(), cancelled)?
            .commit()
    }

    /// Retained single-span API, implemented through the shared snapshot and
    /// atomic mutation path; no second truncate/rename implementation.
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
        let _mutation = self.lock_mutations(&[path.to_owned()])?;
        let original = self.read_bytes(path, MAX_FILE)?;
        let text = std::str::from_utf8(&original)
            .map_err(|_| ScopedError::Denied("file is not UTF-8 text"))?;
        if text.match_indices(old).count() != 1 {
            return Err(ScopedError::Denied("edit needs exactly one match"));
        }
        let replacement = text.replacen(old, new, 1);
        if replacement.len() > MAX_FILE {
            return Err(ScopedError::Limit("edited file would exceed 1 MiB"));
        }
        if replacement.as_bytes() == original {
            return Ok(());
        }
        self.stage_update(path, &original, replacement.as_bytes(), cancelled)?
            .commit()
    }
}

/// Shared traversal on the pinned no-follow filesystem authority.
impl aw_tools::filesystem::walk::Backend for Root {
    type Error = ScopedError;
    fn list(
        &self,
        path: &str,
        max: usize,
    ) -> std::result::Result<aw_tools::filesystem::walk::Listing, Self::Error> {
        use aw_tools::filesystem::{walk, Kind};
        let listing = self.list_with_limit(path, max)?;
        Ok(walk::Listing {
            incomplete: listing.incomplete,
            path_encoding: listing.path_encoding,
            entries: listing
                .entries
                .into_iter()
                .map(|item| {
                    let kind = match item.kind {
                        EntryKind::File => Kind::File,
                        EntryKind::Directory => Kind::Dir,
                        EntryKind::Symlink => Kind::Symlink,
                        EntryKind::Unsupported => Kind::Other,
                    };
                    walk::WalkEntry {
                        name: item.name,
                        path: item.path,
                        kind,
                        hidden: item.hidden,
                    }
                })
                .collect(),
        })
    }
    fn read_bytes(&self, path: &str, max: usize) -> std::result::Result<Vec<u8>, Self::Error> {
        Root::read_bytes(self, path, max)
    }
    fn classify(&self, error: &Self::Error) -> aw_tools::filesystem::walk::BackendErrorKind {
        use aw_tools::filesystem::walk::BackendErrorKind as K;
        match error {
            ScopedError::Io(e) if e.kind() == io::ErrorKind::NotFound => K::Missing,
            ScopedError::Io(_) | ScopedError::Denied(_) => K::Unavailable,
            ScopedError::Limit(_) => K::Limit,
            ScopedError::Cancelled => K::Other,
        }
    }
    fn private_entry(&self, path: &str) -> Result<bool> {
        self.is_private_path(path)
    }
}

impl aw_tools::filesystem::backend::MetadataBackend for Root {
    fn entry_meta(&self, path: &str) -> Result<aw_tools::filesystem::EntryMeta> {
        Root::entry_meta(self, path)
    }
}
impl aw_tools::filesystem::backend::ExplicitPathBackend for Root {
    fn resolve_content_path(&self, path: &str, allow_missing: bool) -> Result<String> {
        Root::resolve_content_path(self, path, allow_missing)
    }
    fn resolve_directory_path(&self, path: &str) -> Result<String> {
        Root::resolve_directory_path(self, path)
    }
}
impl aw_tools::filesystem::backend::StagedPublication for port::StagedWrite {
    type Error = ScopedError;
    fn commit(self) -> Result<()> {
        port::StagedWrite::commit(self)
    }
}
impl aw_tools::filesystem::backend::SecuredFilesystem for Root {
    type Guard<'a> = port::MutationGuard<'a>;
    type Stage = port::StagedWrite;
    fn mutation_path_key(&self, path: &str) -> Result<Vec<u8>> {
        validate_path(path)?;
        let parts = path.split('/').collect::<Vec<_>>();
        let mut parent = self.directory.try_clone()?;
        let mut suffix_at = parts.len().saturating_sub(1);
        for (index, part) in parts.iter().take(parts.len().saturating_sub(1)).enumerate() {
            match at(&parent, part, libc::O_RDONLY | libc::O_DIRECTORY, 0) {
                Ok(next) => parent = next,
                Err(ScopedError::Io(ref error)) if error.kind() == io::ErrorKind::NotFound => {
                    suffix_at = index;
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        let suffix = parts[suffix_at..].join("/");
        let meta = parent.metadata()?;
        let mut key = Vec::with_capacity(16 + suffix.len());
        key.extend_from_slice(&meta.dev().to_le_bytes());
        key.extend_from_slice(&meta.ino().to_le_bytes());
        key.extend_from_slice(suffix.as_bytes());
        Ok(key)
    }
    fn lock<'a>(&'a self, paths: &[String], cancel: &dyn Fn() -> bool) -> Result<Self::Guard<'a>> {
        self.lock_mutations_with_cancel(paths, cancel)
    }
    fn stage_create(
        &self,
        path: &str,
        bytes: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self::Stage> {
        Root::stage_create(self, path, bytes, cancel)
    }
    fn stage_replace(
        &self,
        path: &str,
        bytes: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self::Stage> {
        Root::stage_replace(self, path, bytes, cancel)
    }
    fn stage_update(
        &self,
        path: &str,
        expected: &[u8],
        bytes: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self::Stage> {
        Root::stage_update(self, path, expected, bytes, cancel)
    }
    fn classify_publication_failure(
        &self,
        error: &Self::Error,
    ) -> aw_tools::filesystem::backend::CommitFailure {
        use aw_tools::filesystem::backend::CommitFailure;
        match error {
            ScopedError::Denied(_) | ScopedError::Limit(_) | ScopedError::Cancelled => {
                CommitFailure::NoPublication
            }
            ScopedError::Io(_) => CommitFailure::Unknown,
        }
    }
    fn copy_exact(&self, source: &str, destination: &str, cancel: &dyn Fn() -> bool) -> Result<()> {
        Root::copy_path(self, source, destination, true, cancel)
    }
    fn move_exact(&self, source: &str, destination: &str, cancel: &dyn Fn() -> bool) -> Result<()> {
        Root::move_path(self, source, destination, cancel)
    }
    fn remove_exact(
        &self,
        paths: &[String],
        recursive: bool,
        cancel: &dyn Fn() -> bool,
    ) -> Result<aw_tools::filesystem::lifecycle::RemoveOutcome> {
        Root::remove_paths(self, paths, recursive, cancel)
    }
    fn read_page(
        &self,
        path: &str,
        request: aw_tools::read::PageRequest,
        cancel: &dyn Fn() -> bool,
    ) -> Result<aw_tools::read::Page> {
        Root::read_page_with_cancel(self, path, request, cancel)
    }
}

#[cfg(test)]
mod namespace_identity_tests {
    use super::*;
    use aw_tools::filesystem::backend::SecuredFilesystem as _;
    use std::fs;

    #[test]
    fn mutation_path_key_tracks_namespace_entry_across_overlapping_roots() {
        let base = std::env::temp_dir().join(format!(
            "aw-namespace-key-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sub = base.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("a.txt"), b"a").unwrap();
        fs::hard_link(sub.join("a.txt"), sub.join("hard.txt")).unwrap();
        let outer = Root::open(&base).unwrap();
        let inner = Root::open(&sub).unwrap();
        assert_eq!(
            outer.mutation_path_key("sub/a.txt").unwrap(),
            inner.mutation_path_key("a.txt").unwrap()
        );
        assert_eq!(
            outer.mutation_path_key("sub/new/deep.txt").unwrap(),
            inner.mutation_path_key("new/deep.txt").unwrap(),
            "missing suffix keys use the same nearest existing parent identity"
        );
        assert_ne!(
            inner.mutation_path_key("a.txt").unwrap(),
            inner.mutation_path_key("hard.txt").unwrap(),
            "distinct hard-link names are distinct publication entries"
        );
        let _ = fs::remove_dir_all(base);
    }
}
