//! Explicit human-initiated, read-only workspace browsing. This is NOT an
//! advertised model tool or a permission grant. Nothing is added to context.
use aw_agent_core::{Error, Result};
use std::{collections::VecDeque, path::Path};

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

/// Retains a platform-scoped local root. Unix uses descriptor-relative no-follow
/// traversal; Windows uses reparse-point-aware component checks.
pub struct WorkspaceFiles {
    root: aw_platform::scoped_fs::Root,
}

fn listing(value: aw_platform::scoped_fs::Listing) -> Listing {
    Listing {
        path: value.path,
        incomplete: value.incomplete,
        entries: value
            .entries
            .into_iter()
            .map(|entry| Entry {
                name: entry.name,
                path: entry.path,
                kind: match entry.kind {
                    aw_platform::scoped_fs::EntryKind::Directory => EntryKind::Directory,
                    aw_platform::scoped_fs::EntryKind::File => EntryKind::File,
                    aw_platform::scoped_fs::EntryKind::Unsupported => EntryKind::Unsupported,
                },
            })
            .collect(),
    }
}

impl WorkspaceFiles {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self {
            root: crate::platform_fs(aw_platform::scoped_fs::Root::open(path))?,
        })
    }

    pub fn read(&self, path: &str) -> Result<String> {
        let _guard = crate::platform_fs(self.root.lock())?;
        crate::platform_fs(self.root.read(path))
    }

    pub fn list(&self, path: &str) -> Result<Listing> {
        let _guard = crate::platform_fs(self.root.lock())?;
        crate::platform_fs(self.root.list(path)).map(listing)
    }

    /// Bounded filename/path search for the human workspace navigator. This is
    /// not model context and never follows unsupported/symlink/reparse entries.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Entry>> {
        self.search_with_cancel(query, limit, || false)
    }

    pub fn search_with_cancel(
        &self,
        query: &str,
        limit: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Vec<Entry>> {
        if query.trim().is_empty()
            || query.len() > 256
            || query.chars().any(char::is_control)
            || !(1..=200).contains(&limit)
        {
            return Err(Error::Config(
                "file search needs a 1..256 byte query and limit 1..200".into(),
            ));
        }
        let needle = query.to_lowercase();
        let mut queue = VecDeque::from([String::new()]);
        let mut matches = Vec::new();
        let mut directories = 0usize;
        let mut entries_seen = 0usize;

        while let Some(path) = queue.pop_front() {
            if cancelled() {
                return Ok(matches);
            }
            directories = directories.saturating_add(1);
            if directories > 2048 || entries_seen > 50_000 {
                break;
            }
            let listing = match self.list(&path) {
                Ok(listing) => listing,
                Err(_) if !path.is_empty() => continue,
                Err(error) => return Err(error),
            };
            for entry in listing.entries {
                if cancelled() {
                    return Ok(matches);
                }
                entries_seen = entries_seen.saturating_add(1);
                if entry.path.to_lowercase().contains(&needle)
                    || entry.name.to_lowercase().contains(&needle)
                {
                    matches.push(entry.clone());
                    if matches.len() >= limit {
                        return Ok(matches);
                    }
                }
                if entry.kind == EntryKind::Directory && !is_search_noise(&entry.path) {
                    queue.push_back(entry.path);
                }
                if entries_seen > 50_000 {
                    break;
                }
            }
        }
        Ok(matches)
    }
}

fn is_search_noise(path: &str) -> bool {
    path.split('/').any(|part| {
        matches!(
            part,
            ".git" | "node_modules" | "target" | ".direnv" | ".next" | ".cache" | "__pycache__"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn cancellable_search_stops_before_traversing_more_directories() {
        let root = std::env::temp_dir().join(format!("aw-browse-cancel-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        for index in 0..8 {
            let dir = root.join(format!("d{index}"));
            fs::create_dir(&dir).unwrap();
            fs::write(dir.join("needle.txt"), b"x").unwrap();
        }
        let files = WorkspaceFiles::open(&root).unwrap();
        let polls = AtomicUsize::new(0);
        let found = files
            .search_with_cancel("needle", 80, || polls.fetch_add(1, Ordering::SeqCst) >= 1)
            .unwrap();
        assert!(found.is_empty());
        assert!(polls.load(Ordering::SeqCst) <= 3);
        fs::remove_dir_all(root).unwrap();
    }
}
