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
    sync::{Mutex, MutexGuard},
};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_READ, FILE_SHARE_WRITE,
};

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
    _directories: Vec<File>,
    serial: Mutex<()>,
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

impl Root {
    pub fn open(root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        let mut ancestors = root.ancestors().collect::<Vec<_>>();
        ancestors.reverse();
        let mut directories = Vec::with_capacity(ancestors.len());
        for ancestor in ancestors {
            // Pin drive/share root first. Each later open is protected by all
            // preceding handles, including ancestors outside the granted root.
            directories.push(pin_directory(ancestor)?);
        }
        Ok(Self {
            root,
            _directories: directories,
            serial: Mutex::new(()),
        })
    }

    pub fn lock(&self) -> Result<MutexGuard<'_, ()>> {
        self.serial
            .lock()
            .map_err(|_| ScopedError::Denied("file root is poisoned; stop and inspect"))
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
        let (_parent, path) = self.parent(path)?;
        let mut file = super::open_nofollow_read(&path)?;
        Self::contents(&mut file)
    }

    pub fn list(&self, path: &str) -> Result<Listing> {
        self.list_with_limit(path, 512)
    }

    pub fn list_with_limit(&self, path: &str, max_entries: usize) -> Result<Listing> {
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
            if entries.len() >= max_entries {
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
