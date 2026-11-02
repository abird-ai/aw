//! Reparse-point-aware scoped filesystem implementation for Windows.
//! This is a local single-user safety boundary, not a tenant sandbox against
//! administrators or same-user native writers.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_READ, FILE_SHARE_WRITE,
};

pub mod capability;
#[path = "scoped_fs/port.rs"]
mod port;
pub use port::{MutationGuard, StagedWrite};

const MAX_FILE: usize = 1024 * 1024;
#[cfg(test)]
static TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
#[cfg(test)]
use std::sync::atomic::Ordering;

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
    root: PathBuf,
    _directories: Vec<File>,
    serial: Mutex<()>,
    private: Arc<aw_tools::filesystem::private_artifact::Registry>,
}

/// Directory handles keep every traversed path component in place until the
/// actual filesystem operation finishes. A bare PathBuf is not a capability.
struct Parent {
    path: PathBuf,
    directories: Vec<File>,
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

fn pin_directory(path: &Path) -> Result<File> {
    // Metadata-only access does not require directory-list permission. Omit
    // FILE_SHARE_DELETE so Windows denies rename/delete while this handle is
    // alive; ordinary child-file reads/writes remain shared. Validate the opened
    // object, never a path check followed by an unprotected reopen.
    // https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew
    // https://doc.rust-lang.org/std/os/windows/fs/trait.OpenOptionsExt.html
    let directory = OpenOptions::new()
        .access_mode(0)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let meta = directory.metadata()?;
    if !meta.is_dir() || is_reparse(&meta) {
        return Err(ScopedError::Denied(
            "path traverses a non-directory or reparse point",
        ));
    }
    Ok(directory)
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
        // Bind preparation to the exact non-reparse directory object first.
        // The subsequent top-down ancestor pins protect the reusable namespace;
        // the final object identity comparison closes replacement races between
        // canonicalization and publication of the retained capability.
        let expected = pin_directory(&root)?;
        let expected_identity = super::file_identity(&expected)?;
        let mut ancestors = root.ancestors().collect::<Vec<_>>();
        ancestors.reverse();
        let mut directories = Vec::with_capacity(ancestors.len());
        for ancestor in ancestors {
            directories.push(pin_directory(ancestor)?);
        }
        let actual = directories
            .last()
            .ok_or(ScopedError::Denied("missing pinned root directory"))?;
        if super::file_identity(actual)? != expected_identity {
            return Err(ScopedError::Denied("granted root changed while opening"));
        }
        drop(expected);
        Ok(Self {
            root,
            _directories: directories,
            serial: Mutex::new(()),
            private,
        })
    }

    pub fn lock(&self) -> Result<MutexGuard<'_, ()>> {
        self.serial
            .lock()
            .map_err(|_| ScopedError::Denied("file root is poisoned; stop and inspect"))
    }

    fn parent_artifact_identity(
        &self,
        parent: &Parent,
    ) -> Result<aw_tools::filesystem::private_artifact::ObjectIdentity> {
        let handle = parent
            .directories
            .last()
            .or_else(|| self._directories.last())
            .ok_or(ScopedError::Denied("missing pinned root directory"))?;
        let (volume, object) = super::file_identity(handle)?;
        Ok(aw_tools::filesystem::private_artifact::ObjectIdentity {
            volume: u64::from(volume),
            object,
        })
    }

    pub fn private_artifacts(&self) -> &Arc<aw_tools::filesystem::private_artifact::Registry> {
        &self.private
    }

    pub fn is_private_path(&self, path: &str) -> Result<bool> {
        validate_path(path)?;
        let (parent, target) = match self.parent(path) {
            Ok(value) => value,
            Err(ScopedError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(false);
            }
            Err(error) => return Err(error),
        };
        let name = target
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or(ScopedError::Denied("private artifact name is not reusable"))?;
        let parent_id = self.parent_artifact_identity(&parent)?;
        let file = match OpenOptions::new()
            .access_mode(0)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&target)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        let (volume, object) = super::file_identity(&file)?;
        self.private
            .owns(
                parent_id,
                name,
                aw_tools::filesystem::private_artifact::ObjectIdentity {
                    volume: u64::from(volume),
                    object,
                },
            )
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
        directory: bool,
        label: &'static str,
    ) -> Result<String> {
        validate_path(path)?;
        let (_parent, target) = match self.parent(path) {
            Ok(value) => value,
            Err(ScopedError::Io(error))
                if allow_missing && error.kind() == io::ErrorKind::NotFound =>
            {
                return Ok(path.to_owned());
            }
            Err(error) => return Err(error),
        };
        let entry = match fs::symlink_metadata(&target) {
            Ok(entry) => entry,
            Err(error) if allow_missing && error.kind() == io::ErrorKind::NotFound => {
                return Ok(path.to_owned());
            }
            Err(error) => return Err(error.into()),
        };
        if !is_reparse(&entry) {
            if entry.is_dir() == directory && (entry.is_dir() || entry.is_file()) {
                return Ok(path.to_owned());
            }
            return Err(ScopedError::Denied(label));
        }
        if !entry.file_type().is_symlink() {
            return Err(ScopedError::Denied("unsupported final reparse point"));
        }

        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE,
        };
        let followed = OpenOptions::new()
            .access_mode(0)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(&target)
            .map_err(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    ScopedError::Denied("dangling final symlink is not a missing target")
                } else {
                    ScopedError::Io(error)
                }
            })?;
        let followed_meta = followed.metadata()?;
        if followed_meta.is_dir() != directory
            || (!followed_meta.is_dir() && !followed_meta.is_file())
        {
            return Err(ScopedError::Denied(label));
        }
        let followed_id = super::file_identity(&followed)?;
        let canonical = target.canonicalize().map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                ScopedError::Denied("dangling final symlink is not a missing target")
            } else {
                ScopedError::Io(error)
            }
        })?;
        let resolved = root_relative_utf8(&self.root, &canonical)?;
        let (_verified_parent, verified_path) = self.parent(&resolved)?;
        let verified = if directory {
            pin_directory(&verified_path)?
        } else {
            super::open_nofollow_metadata(&verified_path)?
        };
        let verified_meta = verified.metadata()?;
        if verified_meta.is_dir() != directory
            || (!verified_meta.is_dir() && !verified_meta.is_file())
            || super::file_identity(&verified)? != followed_id
        {
            return Err(ScopedError::Denied(
                "final symlink target changed during resolution",
            ));
        }
        Ok(resolved)
    }

    /// Metadata-only final content-link resolution. Parent handles omit
    /// FILE_SHARE_DELETE, so the traversed namespace remains pinned while the
    /// final target spelling/identity is revalidated.
    pub fn resolve_content_path(&self, path: &str, allow_missing: bool) -> Result<String> {
        self.resolve_final_path(
            path,
            allow_missing,
            false,
            "final symlink target is not a regular file",
        )
    }

    pub fn resolve_directory_path(&self, path: &str) -> Result<String> {
        self.resolve_final_path(path, false, true, "discovery root is not a directory")
    }

    fn parent(&self, path: &str) -> Result<(Parent, PathBuf)> {
        validate_path(path)?;
        let mut parts = path.split('/').collect::<Vec<_>>();
        let leaf = parts.pop().ok_or(ScopedError::Denied("path missing"))?;
        let mut dir = self.root.clone();
        let mut directories = Vec::with_capacity(parts.len());
        for part in parts {
            dir.push(part);
            directories.push(pin_directory(&dir)?);
        }
        let target = dir.join(leaf);
        Ok((
            Parent {
                path: dir,
                directories,
            },
            target,
        ))
    }

    fn contents(file: &mut File) -> Result<String> {
        let meta = file.metadata()?;
        if !meta.is_file() || is_reparse(&meta) {
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
        let (_parent, path) = self.parent(path)?;
        let mut file = super::open_nofollow_read(&path)?;
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
            Parent {
                path: self.root.clone(),
                directories: Vec::new(),
            }
        } else {
            let (mut parent, target) = self.parent(path)?;
            parent.directories.push(pin_directory(&target)?);
            parent.path = target;
            parent
        };

        let mut entries = Vec::new();
        let mut incomplete = false;
        let mut path_encoding = false;
        for (scanned, item) in fs::read_dir(&dir.path)?.enumerate() {
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
                path_encoding = true;
                continue;
            };
            if validate_path(&name).is_err() {
                path_encoding = true;
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
                if meta.file_type().is_symlink() {
                    EntryKind::Symlink
                } else {
                    EntryKind::Unsupported
                }
            } else if meta.is_dir() {
                EntryKind::Directory
            } else if meta.is_file() {
                EntryKind::File
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
            let hidden = name.starts_with('.')
                || meta.file_attributes()
                    & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_HIDDEN
                    != 0;
            entries.push(Entry {
                name,
                path: joined,
                kind,
                hidden,
            });
            if entries.len() >= max_entries {
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

    /// Existing create-only platform API, now sharing the staged writer.
    pub fn create(&self, path: &str, content: &str, cancelled: &dyn Fn() -> bool) -> Result<()> {
        let _mutation = self.lock_mutations(&[path.to_owned()])?;
        self.stage_create(path, content.as_bytes(), cancelled)?
            .commit()
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

#[cfg(test)]
mod tests {
    use super::*;

    struct Tree(PathBuf);
    impl Tree {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "aw-win-scoped-pins-{}-{}",
                std::process::id(),
                TEMP.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir_all(path.join("granted/src/deep")).unwrap();
            fs::write(path.join("granted/src/deep/file.txt"), "Alpha 雪").unwrap();
            Self(path)
        }
        fn granted(&self) -> PathBuf {
            self.0.join("granted")
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn retained_root_and_ancestors_cannot_be_replaced_until_drop() {
        let tree = Tree::new();
        let root = Root::open(&tree.granted()).unwrap();
        let moved_root = tree.0.join("moved-root");
        let moved_ancestor = tree.0.with_extension("moved");
        assert!(fs::rename(tree.granted(), &moved_root).is_err());
        assert!(fs::rename(&tree.0, &moved_ancestor).is_err());
        assert_eq!(root.read("src/deep/file.txt").unwrap(), "Alpha 雪");
        assert_eq!(root.list("").unwrap().entries[0].name, "src");
        assert_eq!(root.list("src/deep").unwrap().entries[0].name, "file.txt");
        drop(root);
        fs::rename(tree.granted(), &moved_root).unwrap();
        fs::rename(&moved_root, tree.granted()).unwrap();
        fs::rename(&tree.0, &moved_ancestor).unwrap();
        fs::rename(&moved_ancestor, &tree.0).unwrap();
    }

    #[test]
    fn descendant_guards_cover_the_operation_then_release_directory_renames() {
        let tree = Tree::new();
        let root = Root::open(&tree.granted()).unwrap();
        let (parents, target) = root.parent("src/deep/file.txt").unwrap();
        let source = tree.granted().join("src");
        let moved = tree.granted().join("moved-src");
        assert!(fs::rename(&source, &moved).is_err());
        assert!(fs::rename(source.join("deep"), source.join("moved-deep")).is_err());
        let mut file = super::super::open_nofollow_read(&target).unwrap();
        assert_eq!(Root::contents(&mut file).unwrap(), "Alpha 雪");
        drop(file);
        drop(parents);
        fs::rename(&source, &moved).unwrap();
        fs::rename(&moved, &source).unwrap();

        let listed = pin_directory(&source).unwrap();
        assert!(fs::rename(&source, &moved).is_err());
        drop(listed);
        fs::rename(&source, &moved).unwrap();
        fs::rename(&moved, &source).unwrap();
    }

    #[test]
    fn directory_pins_preserve_leaf_edits_cancellation_and_read_policy() {
        let tree = Tree::new();
        let root = Root::open(&tree.granted()).unwrap();
        root.create("src/new.txt", "before", &|| false).unwrap();
        assert!(matches!(
            root.edit("src/new.txt", "before", "after", &|| true),
            Err(ScopedError::Cancelled)
        ));
        assert_eq!(root.read("src/new.txt").unwrap(), "before");
        root.edit("src/new.txt", "before", "after", &|| false)
            .unwrap();
        assert_eq!(root.read("src/new.txt").unwrap(), "after");
        assert!(root.create("src/new.txt", "overwrite", &|| false).is_err());
        assert!(root.read("../outside").is_err());
        fs::hard_link(
            tree.granted().join("src/new.txt"),
            tree.granted().join("hardlink.txt"),
        )
        .unwrap();
        assert!(root.read("src/new.txt").is_err());
        fs::write(tree.granted().join("binary"), [0xff]).unwrap();
        assert!(root.read("binary").is_err());
        fs::write(tree.granted().join("oversized"), vec![b'x'; MAX_FILE + 1]).unwrap();
        assert!(root.read("oversized").is_err());
        assert!(pin_directory(&tree.granted().join("binary")).is_err());
    }

    #[test]
    fn opened_directory_reparse_points_are_rejected_before_traversal() {
        let tree = Tree::new();
        let outside = tree.0.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("secret"), "outside").unwrap();
        let alias = tree.granted().join("alias");
        std::os::windows::fs::symlink_dir(&outside, &alias)
            .expect("Windows reparse regression requires symlink privilege or Developer Mode");
        let root = Root::open(&tree.granted()).unwrap();
        assert!(pin_directory(&alias).is_err());
        assert!(root.read("alias/secret").is_err());
        assert!(root.list("alias").is_err());
    }
}

/// Shared traversal with pinned Windows ancestor handles; no reparse follow.
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
            ScopedError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => K::Missing,
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
        port::folded_namespace_key(&self.root.join(path))
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
    fn stage_create_many(
        &self,
        files: &[aw_tools::filesystem::backend::CreateFile<'_>],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Vec<aw_tools::filesystem::backend::CreateStage<Self::Stage>>> {
        Root::stage_create_many(self, files, cancel)
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
    fn open_regular_for_update(&self, path: &str) -> Result<std::fs::File> {
        Root::open_regular(self, path)
    }
    fn stage_stream_update(
        &self,
        path: &str,
        expected: (u64, [u8; 32]),
        writer: &mut dyn FnMut(
            &mut std::fs::File,
        ) -> std::result::Result<
            (),
            aw_tools::filesystem::backend::StreamWriteError,
        >,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self::Stage> {
        Root::stage_stream_update(
            self,
            path,
            expected,
            |output| {
                writer(output).map_err(|error| match error {
                    aw_tools::filesystem::backend::StreamWriteError::Io(error) => {
                        ScopedError::Io(error)
                    }
                    aw_tools::filesystem::backend::StreamWriteError::Cancelled => {
                        ScopedError::Cancelled
                    }
                    aw_tools::filesystem::backend::StreamWriteError::Refused(reason) => {
                        ScopedError::Denied(reason)
                    }
                })
            },
            cancel,
        )
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
