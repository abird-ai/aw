//! Descriptor-backed traversal, hidden/ignore semantics, and bounded work.
//! Consumers provide opened-capability access; the walker never opens paths
//! through the ambient OS namespace.
use crate::filesystem::Kind;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

pub const MAX_ENTRY_RESULTS: usize = 16_384;
pub const MAX_IGNORE_BYTES: usize = 64 * 1024;
pub const MAX_IGNORE_LINES: usize = 1024;
const MAX_IGNORE_TOTAL_BYTES: usize = 4 * 1024 * 1024;
const MAX_IGNORE_FILES: usize = 256;
#[derive(Clone, Debug)]
pub struct WalkEntry {
    pub name: String,
    pub path: String,
    pub kind: Kind,
    pub hidden: bool,
}
pub struct Listing {
    pub entries: Vec<WalkEntry>,
    pub incomplete: bool,
    pub path_encoding: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendErrorKind {
    Missing,
    Unavailable,
    Limit,
    Other,
}
pub trait Backend {
    type Error;
    fn list(&self, path: &str, max: usize) -> Result<Listing, Self::Error>;
    fn read_bytes(&self, path: &str, max: usize) -> Result<Vec<u8>, Self::Error>;
    fn classify(&self, error: &Self::Error) -> BackendErrorKind;
}
#[derive(Debug)]
pub enum WalkError<E> {
    Source(E),
    Cancelled,
    IgnoreLimit,
    InvalidIgnore,
}
#[derive(Default, Clone, Copy, Debug)]
pub struct Progress {
    pub directory_budget: bool,
    pub traversal_error: bool,
    pub ignore_rules: bool,
    pub path_encoding: bool,
}
impl Progress {
    pub fn incomplete(self) -> bool {
        self.directory_budget || self.traversal_error || self.ignore_rules || self.path_encoding
    }
    pub fn reason(self) -> Option<&'static str> {
        if self.directory_budget {
            Some("directory_budget")
        } else if self.traversal_error {
            Some("traversal_error")
        } else if self.ignore_rules {
            Some("ignore_rules")
        } else {
            None
        }
    }
}
#[derive(Default)]
struct IgnoreWork {
    bytes: usize,
    files: usize,
    unavailable: bool,
}
impl IgnoreWork {
    fn charge(&mut self, bytes: usize) -> Result<(), WalkError<()>> {
        self.files += 1;
        self.bytes = self.bytes.saturating_add(bytes);
        if self.files > MAX_IGNORE_FILES || self.bytes > MAX_IGNORE_TOTAL_BYTES {
            return Err(WalkError::IgnoreLimit);
        }
        Ok(())
    }
}
struct IgnoreLevel {
    rules: Option<Gitignore>,
    parent: Option<Arc<IgnoreLevel>>,
}
impl IgnoreLevel {
    fn ignored(&self, path: &str, is_dir: bool) -> bool {
        let mut level = Some(self);
        while let Some(value) = level {
            if let Some(rules) = &value.rules {
                match rules.matched(Path::new(path), is_dir) {
                    ignore::Match::Ignore(_) => return true,
                    ignore::Match::Whitelist(_) => return false,
                    ignore::Match::None => {}
                }
            }
            level = value.parent.as_deref();
        }
        false
    }
}
fn ignore_file<B: Backend>(
    backend: &B,
    dir: &str,
    parent: Option<Arc<IgnoreLevel>>,
    work: &mut IgnoreWork,
) -> Result<Arc<IgnoreLevel>, WalkError<B::Error>> {
    let path = if dir.is_empty() {
        ".gitignore".into()
    } else {
        format!("{dir}/.gitignore")
    };
    // An optional ignore filter is not authorization. Malformed/truncated/
    // unreadable/over-budget inputs cannot apply a partial rule set.
    let rules = if work.files >= MAX_IGNORE_FILES || work.bytes >= MAX_IGNORE_TOTAL_BYTES {
        work.unavailable = true;
        None
    } else {
        match backend.read_bytes(&path, MAX_IGNORE_BYTES + 1) {
            Ok(bytes) => {
                if bytes.len() > MAX_IGNORE_BYTES || work.charge(bytes.len()).is_err() {
                    work.unavailable = true;
                    None
                } else if let Ok(contents) = std::str::from_utf8(&bytes) {
                    if contents.lines().count() > MAX_IGNORE_LINES {
                        work.unavailable = true;
                        None
                    } else {
                        let mut builder = GitignoreBuilder::new(dir);
                        let parsed = contents
                            .lines()
                            .all(|line| builder.add_line(None, line).is_ok());
                        if parsed {
                            match builder.build() {
                                Ok(rules) => Some(rules),
                                Err(_) => {
                                    work.unavailable = true;
                                    None
                                }
                            }
                        } else {
                            work.unavailable = true;
                            None
                        }
                    }
                } else {
                    work.unavailable = true;
                    None
                }
            }
            Err(error) => match backend.classify(&error) {
                BackendErrorKind::Missing => None,
                BackendErrorKind::Unavailable | BackendErrorKind::Limit => {
                    work.unavailable = true;
                    None
                }
                BackendErrorKind::Other => return Err(WalkError::Source(error)),
            },
        }
    };
    Ok(Arc::new(IgnoreLevel { rules, parent }))
}
fn ancestors<B: Backend>(
    backend: &B,
    path: &str,
    work: &mut IgnoreWork,
) -> Result<Option<Arc<IgnoreLevel>>, WalkError<B::Error>> {
    let mut state = None;
    let mut prefix = String::new();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        state = Some(ignore_file(backend, &prefix, state, work)?);
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(part);
    }
    Ok(state)
}
struct Level {
    path: String,
    depth: usize,
    rules: Option<Arc<IgnoreLevel>>,
}
pub fn walk<B: Backend>(
    backend: &B,
    start: &str,
    depth: usize,
    hidden: bool,
    gitignore: bool,
    cancelled: &dyn Fn() -> bool,
    mut visit: impl FnMut(&WalkEntry) -> Result<bool, WalkError<B::Error>>,
) -> Result<Progress, WalkError<B::Error>> {
    let mut work = IgnoreWork::default();
    // The named directory is the traversal root, not its ancestors.
    // No parent or global ignore config is imported into this search.
    let state = None;
    let mut stack = vec![Level {
        path: start.into(),
        depth: 0,
        rules: state,
    }];
    let mut progress = Progress::default();
    let mut visited = 0usize;
    let deadline = Instant::now() + Duration::from_secs(15);
    while let Some(level) = stack.pop() {
        if cancelled() {
            return Err(WalkError::Cancelled);
        }
        if Instant::now() >= deadline
            || visited >= super::MAX_WALK_VISITS
            || level.depth >= super::MAX_WALK_DEPTH
        {
            progress.directory_budget = true;
            break;
        }
        let listing = match backend.list(&level.path, MAX_ENTRY_RESULTS) {
            Ok(listing) => listing,
            Err(e) if level.depth == 0 => return Err(WalkError::Source(e)),
            Err(_) => {
                progress.traversal_error = true;
                continue;
            }
        };
        let rules = if gitignore {
            Some(ignore_file(backend, &level.path, level.rules, &mut work)?)
        } else {
            None
        };
        let mut entries = listing.entries;
        entries.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        let mut children = Vec::new();
        for entry in entries {
            if cancelled() {
                return Err(WalkError::Cancelled);
            }
            visited += 1;
            if visited > super::MAX_WALK_VISITS || Instant::now() >= deadline {
                progress.directory_budget = true;
                progress.ignore_rules = work.unavailable;
                return Ok(progress);
            }
            if super::is_reserved_name(&entry.name)
                || (!hidden && entry.hidden)
                || rules
                    .as_ref()
                    .is_some_and(|r| r.ignored(&entry.path, entry.kind == Kind::Dir))
            {
                continue;
            }
            if visit(&entry)? {
                progress.ignore_rules = work.unavailable;
                return Ok(progress);
            }
            if entry.kind == Kind::Dir && level.depth + 1 < depth {
                children.push(entry.path);
            }
        }
        progress.directory_budget |= listing.incomplete;
        progress.path_encoding |= listing.path_encoding;
        for path in children.into_iter().rev() {
            stack.push(Level {
                path,
                depth: level.depth + 1,
                rules: rules.clone(),
            });
        }
    }
    progress.ignore_rules = work.unavailable;
    Ok(progress)
}
pub fn explicitly_ignored<B: Backend>(
    backend: &B,
    exact_path: &str,
    is_dir: bool,
) -> Result<(bool, bool), WalkError<B::Error>> {
    let mut work = IgnoreWork::default();
    let parent = exact_path.rsplit_once('/').map_or("", |(p, _)| p);
    let state = ancestors(backend, parent, &mut work)?;
    let level = ignore_file(backend, parent, state, &mut work)?;
    Ok((level.ignored(exact_path, is_dir), work.unavailable))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    struct Fake {
        dirs: BTreeMap<String, Vec<WalkEntry>>,
        ignore: BTreeMap<String, Vec<u8>>,
    }
    impl Backend for Fake {
        type Error = &'static str;
        fn list(&self, path: &str, _: usize) -> Result<Listing, Self::Error> {
            Ok(Listing {
                entries: self.dirs.get(path).cloned().ok_or("missing")?,
                incomplete: false,
                path_encoding: false,
            })
        }
        fn read_bytes(&self, path: &str, _: usize) -> Result<Vec<u8>, Self::Error> {
            self.ignore.get(path).cloned().ok_or("missing")
        }
        fn classify(&self, error: &&'static str) -> BackendErrorKind {
            if *error == "missing" {
                BackendErrorKind::Missing
            } else {
                BackendErrorKind::Other
            }
        }
    }
    #[test]
    fn requested_root_does_not_read_parent_ignore_files() {
        let fake = Fake {
            dirs: BTreeMap::from([(
                "src".into(),
                vec![WalkEntry {
                    name: "visible.rs".into(),
                    path: "src/visible.rs".into(),
                    kind: Kind::File,
                    hidden: false,
                }],
            )]),
            ignore: BTreeMap::from([(".gitignore".into(), b"src/visible.rs\n".to_vec())]),
        };
        let mut output = Vec::new();
        let progress = walk(&fake, "src", 1, false, true, &|| false, |entry| {
            output.push(entry.path.clone());
            Ok(false)
        })
        .unwrap();
        assert_eq!(output, ["src/visible.rs"]);
        assert!(!progress.ignore_rules);
    }
    #[test]
    fn invalid_or_oversized_ignore_is_incomplete_not_a_hard_error() {
        for bad in [vec![0xff], vec![b'x'; MAX_IGNORE_BYTES + 1]] {
            let fake = Fake {
                dirs: BTreeMap::from([(
                    "".into(),
                    vec![WalkEntry {
                        name: "safe.rs".into(),
                        path: "safe.rs".into(),
                        kind: Kind::File,
                        hidden: false,
                    }],
                )]),
                ignore: BTreeMap::from([(".gitignore".into(), bad)]),
            };
            let mut output = Vec::new();
            let progress = walk(&fake, "", 1, false, true, &|| false, |entry| {
                output.push(entry.path.clone());
                Ok(false)
            })
            .unwrap();
            assert_eq!(output, ["safe.rs"]);
            assert!(progress.ignore_rules);
        }
    }
    #[test]
    fn depth_and_hidden_are_shared_across_consumers() {
        let entries = vec![
            WalkEntry {
                name: ".secret".into(),
                path: ".secret".into(),
                kind: Kind::File,
                hidden: true,
            },
            WalkEntry {
                name: "src".into(),
                path: "src".into(),
                kind: Kind::Dir,
                hidden: false,
            },
        ];
        let fake = Fake {
            dirs: BTreeMap::from([
                ("".into(), entries),
                (
                    "src".into(),
                    vec![WalkEntry {
                        name: "hello.rs".into(),
                        path: "src/hello.rs".into(),
                        kind: Kind::File,
                        hidden: false,
                    }],
                ),
            ]),
            ignore: BTreeMap::new(),
        };
        let mut seen = Vec::new();
        walk(&fake, "", 1, false, false, &|| false, |e| {
            seen.push(e.path.clone());
            Ok(false)
        })
        .unwrap();
        assert_eq!(seen, ["src"]);
        seen.clear();
        walk(&fake, "", 2, true, false, &|| false, |e| {
            seen.push(e.path.clone());
            Ok(false)
        })
        .unwrap();
        assert_eq!(seen, [".secret", "src", "src/hello.rs"]);
    }
}
