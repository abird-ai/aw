//! Read-only Git-relevant tree observation and private-index construction.
//! The same observation is used for planning, capture and final revalidation.
use super::{git_error, hash_part, head, is_oid, run, run_os};
use crate::{RepoPath, Repository, MAX_WIRE_BYTES};
use aw_agent_core::{Error, Result};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::{self, Metadata},
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    File,
    Executable,
    Symlink,
    Gitlink,
}
impl Mode {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "100644" => Ok(Self::File),
            "100755" => Ok(Self::Executable),
            "120000" => Ok(Self::Symlink),
            "160000" => Ok(Self::Gitlink),
            _ => Err(Error::Denied("unsupported Git tree mode")),
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::File => "100644",
            Self::Executable => "100755",
            Self::Symlink => "120000",
            Self::Gitlink => "160000",
        }
    }
    fn regular(self) -> bool {
        matches!(self, Self::File | Self::Executable)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    mode: Mode,
    oid: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Tree(BTreeMap<RepoPath, Entry>);

impl Tree {
    pub(super) fn fingerprint(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(b"aw-git-flat-tree-v1\0");
        for (path, entry) in &self.0 {
            hash_part(&mut hash, path.bytes());
            hash_part(&mut hash, entry.mode.as_str().as_bytes());
            hash_part(&mut hash, entry.oid.as_bytes());
        }
        format!("{:x}", hash.finalize())
    }

    fn insert(&mut self, path: RepoPath, mode: Mode, oid: String) -> Result<()> {
        if !is_oid(&oid) || self.0.insert(path, Entry { mode, oid }).is_some() {
            return Err(Error::Storage("invalid or duplicate Git tree entry"));
        }
        Ok(())
    }
}

fn records(bytes: &[u8]) -> Result<impl Iterator<Item = &[u8]>> {
    let (body, limit) = if bytes.is_empty() {
        (bytes, 0)
    } else {
        (
            bytes
                .strip_suffix(&[0])
                .ok_or(Error::Storage("unterminated Git paths"))?,
            usize::MAX,
        )
    };
    Ok(body.split(|&b| b == 0).take(limit))
}

fn entry_parts(record: &[u8]) -> Result<(&str, RepoPath)> {
    let tab = record
        .iter()
        .position(|&b| b == b'\t')
        .ok_or(Error::Storage("invalid Git tree record"))?;
    let metadata = std::str::from_utf8(&record[..tab])
        .map_err(|_| Error::Storage("invalid Git tree metadata"))?;
    let path = RepoPath::new(record[tab + 1..].to_vec()).map_err(git_error)?;
    Ok((metadata, path))
}

pub(super) fn committed_tree(repo: &Repository, revision: &str) -> Result<Tree> {
    let bytes = run(repo, &["ls-tree", "-r", "-z", revision])?;
    let mut tree = Tree::default();
    for record in records(&bytes)? {
        let (metadata, path) = entry_parts(record)?;
        let mut fields = metadata.split(' ');
        let mode = Mode::parse(fields.next().unwrap_or_default())?;
        let kind = if mode == Mode::Gitlink {
            "commit"
        } else {
            "blob"
        };
        if fields.next() != Some(kind) {
            return Err(Error::Storage("invalid Git tree object kind"));
        }
        let oid = fields.next().unwrap_or_default();
        if fields.next().is_some() {
            return Err(Error::Storage("invalid Git tree metadata"));
        }
        tree.insert(path, mode, oid.into())?;
    }
    Ok(tree)
}

fn real_index(repo: &Repository) -> Result<Tree> {
    let bytes = run(repo, &["ls-files", "-v", "--stage", "-z"])?;
    let mut tree = Tree::default();
    for record in records(&bytes)? {
        let (metadata, path) = entry_parts(record)?;
        let mut fields = metadata.split(' ');
        if fields.next() != Some("H") {
            return Err(Error::Denied("Git capture refuses hidden index entries"));
        }
        let mode = Mode::parse(fields.next().unwrap_or_default())?;
        let oid = fields.next().unwrap_or_default();
        if fields.next() != Some("0") || fields.next().is_some() {
            return Err(Error::Denied(
                "Git capture refuses conflicted index entries",
            ));
        }
        tree.insert(path, mode, oid.into())?;
    }
    Ok(tree)
}

fn config_bool(repo: &Repository, key: &str, default: bool) -> Result<bool> {
    match repo
        .run(&["config", "--bool", "--get", key], None, true)
        .map_err(git_error)?
        .as_slice()
    {
        b"" => Ok(default),
        b"true\n" => Ok(true),
        b"false\n" => Ok(false),
        _ => Err(Error::Storage("invalid Git boolean configuration")),
    }
}

// Never traverse a symlink/reparse parent, including when a tracked directory
// has become a symlink. A missing descendant is then a deletion.
fn path_metadata(root: &Path, relative: &RepoPath) -> Result<Option<(PathBuf, Metadata)>> {
    let mut path = root.to_owned();
    let mut parts = relative.bytes().split(|&b| b == b'/').peekable();
    while let Some(part) = parts.next() {
        let part = aw_platform::os_string_from_bytes(part.to_vec())?;
        let mut components = Path::new(&part).components();
        if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
            return Err(Error::Denied("unsafe Git worktree path component"));
        }
        path.push(part);
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if parts.peek().is_none() {
            return Ok(Some((path, meta)));
        }
        if !meta.is_dir() || link_like(&path, &meta)? {
            return Ok(None);
        }
    }
    Err(Error::Storage("empty Git worktree path"))
}

fn link_like(path: &Path, meta: &Metadata) -> Result<bool> {
    #[cfg(unix)]
    {
        let _ = path;
        Ok(meta.file_type().is_symlink())
    }
    #[cfg(not(unix))]
    {
        Ok(meta.file_type().is_symlink() || aw_platform::path_is_link_like(path)?)
    }
}

fn executable(meta: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o100 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

pub(super) fn object_id(bytes: &[u8]) -> Result<String> {
    let value = std::str::from_utf8(bytes)
        .ok()
        .and_then(|s| s.strip_suffix('\n'))
        .filter(|s| is_oid(s))
        .ok_or(Error::Storage("invalid Git object identity"))?;
    Ok(value.to_owned())
}

#[cfg(unix)]
fn hash_bytes(repo: &Repository, bytes: &[u8], write: bool) -> Result<String> {
    let mut args = vec![
        OsString::from("hash-object"),
        "--no-filters".into(),
        "--stdin".into(),
    ];
    if write {
        args.push("-w".into());
    }
    object_id(&run_os(repo, &args, Some(bytes), None)?)
}

// Batch regular path hashing; no per-file Git process or file-content buffer.
// Without -w Git computes the normalized blob OIDs without writing objects.
fn hash_regular(
    repo: &Repository,
    files: Vec<(RepoPath, Mode)>,
    write: bool,
    raw: bool,
    tree: &mut Tree,
) -> Result<()> {
    let mut files = files.into_iter().peekable();
    while files.peek().is_some() {
        let mut args = vec![OsString::from("hash-object")];
        if write {
            args.push("-w".into());
        }
        if raw {
            args.push("--no-filters".into());
        }
        args.push("--".into());
        let mut batch = Vec::new();
        let mut size = 0;
        while let Some((path, _)) = files.peek() {
            let next = path.bytes().len() + 4;
            if !batch.is_empty() && size + next > 16 * 1024 {
                break;
            }
            size += next;
            let (path, mode) = files.next().expect("peeked path");
            args.push(path.os().map_err(git_error)?);
            batch.push((path, mode));
        }
        let output = run_os(repo, &args, None, None)?;
        let mut lines = std::str::from_utf8(&output)
            .map_err(|_| Error::Storage("invalid Git hash response"))?
            .lines();
        for (path, mode) in batch {
            tree.insert(
                path,
                mode,
                lines
                    .next()
                    .ok_or(Error::Storage("missing Git blob identity"))?
                    .into(),
            )?;
        }
        if lines.next().is_some() {
            return Err(Error::Storage("extra Git blob identity"));
        }
    }
    Ok(())
}

pub(super) struct Snapshot {
    pub(super) base: String,
    base_tree: Tree,
    pub(super) tree: Tree,
    pub(super) status: String,
    clean_index: bool,
}
impl Snapshot {
    pub(super) fn read(repo: &Repository, write: bool) -> Result<Self> {
        Self::observe(repo, write, 0)
    }

    fn observe(repo: &Repository, write: bool, depth: usize) -> Result<Self> {
        if depth > 16 {
            return Err(Error::Limit("nested Git source depth"));
        }
        repo.guard_managed_checkout().map_err(git_error)?;
        let base = head(repo)?;
        let base_tree = committed_tree(repo, &base)?;
        let index = real_index(repo)?;
        let untracked = run(repo, &["ls-files", "--others", "--exclude-standard", "-z"])?;
        let clean_index = index == base_tree && untracked.is_empty();
        let mut status = Sha256::new();
        status.update(b"aw-git-capture-status-v1\0");
        hash_part(&mut status, index.fingerprint().as_bytes());
        hash_part(&mut status, &untracked);
        let status = format!("{:x}", status.finalize());

        let tree = observe_tree(repo, &base_tree, &index, &untracked, write, depth, false)?;
        if head(repo)? != base {
            return Err(Error::Conflict("Git HEAD changed during capture"));
        }
        repo.validate_binding().map_err(git_error)?;
        Ok(Self {
            base,
            base_tree,
            tree,
            status,
            clean_index,
        })
    }

    pub(super) fn clean(&self) -> bool {
        self.tree == self.base_tree
    }

    // Seed from committed HEAD and apply the exact final tree, not the user's
    // staged contents. Remove paths first to handle directory/file transitions.
    pub(super) fn write_tree(&self, repo: &Repository, index: &Path) -> Result<String> {
        let args = ["read-tree", "--no-sparse-checkout", self.base.as_str()].map(OsString::from);
        run_os(repo, &args, None, Some(index))?;
        let mut input = Vec::new();
        let zero = "0".repeat(self.base.len());
        for path in self.base_tree.0.keys() {
            if !self.tree.0.contains_key(path) {
                index_line(&mut input, "0", &zero, path);
            }
        }
        for (path, entry) in &self.tree.0 {
            if self.base_tree.0.get(path) != Some(entry) {
                index_line(&mut input, entry.mode.as_str(), &entry.oid, path);
            }
        }
        if input.len() > 3 * MAX_WIRE_BYTES {
            return Err(Error::Limit("Git private index input"));
        }
        if !input.is_empty() {
            let args = ["update-index", "-z", "--index-info"].map(OsString::from);
            run_os(repo, &args, Some(&input), Some(index))?;
        }
        let tree = object_id(&run_os(repo, &["write-tree".into()], None, Some(index))?)?;
        if committed_tree(repo, &tree)? != self.tree {
            return Err(Error::Conflict(
                "Git private index differs from observed tree",
            ));
        }
        Ok(tree)
    }
}

// Restore supplies the captured tree as the tracked-path context, not as the
// actual index. This verifies even captured symlinks or ignored files which
// become untracked relative to the original base.
fn observe_tree(
    repo: &Repository,
    base_tree: &Tree,
    index: &Tree,
    untracked: &[u8],
    write: bool,
    depth: usize,
    strict: bool,
) -> Result<Tree> {
    let mut paths: BTreeSet<RepoPath> = base_tree.0.keys().chain(index.0.keys()).cloned().collect();
    for record in records(untracked)? {
        let nested = record.ends_with(b"/");
        let path = RepoPath::new(record.strip_suffix(b"/").unwrap_or(record).to_vec())
            .map_err(git_error)?;
        if nested
            && !base_tree
                .0
                .get(&path)
                .into_iter()
                .chain(index.0.get(&path))
                .any(|entry| entry.mode == Mode::Gitlink)
        {
            return Err(Error::Denied(
                "untracked nested Git repository is not a regular-file source",
            ));
        }
        paths.insert(path);
    }

    let filemode = config_bool(repo, "core.filemode", true)?;
    let symlinks = config_bool(repo, "core.symlinks", true)?;
    let mut tree = Tree::default();
    let mut regular = Vec::new();
    let mut raw = Vec::new();
    for path in paths {
        let original = index.0.get(&path).or_else(|| base_tree.0.get(&path));
        let tracked = original.is_some();
        let Some((full, meta)) = path_metadata(repo.root(), &path)? else {
            continue; // tracked deletion
        };
        if link_like(&full, &meta)? {
            if !tracked {
                if strict {
                    return Err(Error::Conflict("unexpected link in restored Git worktree"));
                }
                continue; // untracked links are never captured or followed
            }
            if !meta.file_type().is_symlink() {
                return Err(Error::Denied("tracked reparse point is not a Git symlink"));
            }
            #[cfg(not(unix))]
            return Err(Error::Denied(
                "native Git symlink capture is unavailable on this platform",
            ));
            #[cfg(unix)]
            {
                let target = fs::read_link(&full)?;
                tree.insert(
                    path,
                    Mode::Symlink,
                    hash_bytes(repo, target.as_os_str().as_encoded_bytes(), write)?,
                )?;
            }
        } else if meta.is_file() {
            let original_mode = original.map(|entry| entry.mode);
            if !symlinks && original_mode == Some(Mode::Symlink) {
                raw.push((path, Mode::Symlink));
            } else {
                let mode = if filemode {
                    if executable(&meta) {
                        Mode::Executable
                    } else {
                        Mode::File
                    }
                } else {
                    original_mode
                        .filter(|mode| mode.regular())
                        .unwrap_or(Mode::File)
                };
                regular.push((path, mode));
            }
        } else if meta.is_dir() {
            let known_link = base_tree
                .0
                .get(&path)
                .into_iter()
                .chain(index.0.get(&path))
                .any(|entry| entry.mode == Mode::Gitlink);
            if known_link {
                if let Some(oid) = gitlink(repo, &full, index.0.get(&path), depth)? {
                    if original.is_some_and(|entry| entry.oid.len() != oid.len()) {
                        return Err(Error::Denied(
                            "Gitlink object format differs from its superproject",
                        ));
                    }
                    tree.insert(path, Mode::Gitlink, oid)?;
                }
            }
            // Ordinary directories are represented by their file entries.
        } else if tracked || strict {
            return Err(Error::Denied("unsupported Git worktree file type"));
        }
    }
    hash_regular(repo, regular, write, false, &mut tree)?;
    hash_regular(repo, raw, write, true, &mut tree)?;
    Ok(tree)
}

/// A restore preserves the base index. Therefore a new or changed gitlink
/// cannot be materialized as an empty directory: doing so would silently use
/// the old OID. Such links require separately authorized nested material.
pub(super) fn check_restore_gitlinks(base: &Tree, captured: &Tree) -> Result<()> {
    for (path, entry) in &captured.0 {
        if entry.mode == Mode::Gitlink && base.0.get(path) != Some(entry) {
            return Err(Error::Denied(
                "Git restore requires separate materialization for changed or added gitlinks",
            ));
        }
    }
    // Replacing a base gitlink with a directory of ordinary files is also
    // ambiguous to Git's base-index untracked enumeration.
    for (path, entry) in &base.0 {
        if entry.mode != Mode::Gitlink || captured.0.contains_key(path) {
            continue;
        }
        let mut prefix = path.bytes().to_vec();
        prefix.push(b'/');
        if captured
            .0
            .range(path.clone()..)
            .take_while(|(candidate, _)| candidate.bytes().starts_with(path.bytes()))
            .any(|(candidate, _)| candidate.bytes().starts_with(&prefix))
        {
            return Err(Error::Denied(
                "Git restore cannot replace a base gitlink with a directory",
            ));
        }
    }
    Ok(())
}

pub(super) fn verify_restored(repo: &Repository, base: &Tree, captured: &Tree) -> Result<()> {
    repo.guard_managed_checkout().map_err(git_error)?;
    if &real_index(repo)? != base {
        return Err(Error::Conflict("restored Git index differs from base"));
    }
    check_restore_gitlinks(base, captured)?;
    // Check gitlinks directly: the base index hides their contents from
    // ls-files --others. Never adopt an externally initialized submodule.
    for (path, entry) in &base.0 {
        if entry.mode != Mode::Gitlink {
            continue;
        }
        match (captured.0.get(path), path_metadata(repo.root(), path)?) {
            (Some(expected), Some((full, meta))) if expected.mode == Mode::Gitlink => {
                if !meta.is_dir()
                    || link_like(&full, &meta)?
                    || fs::read_dir(full)?.next().transpose()?.is_some()
                {
                    return Err(Error::Conflict(
                        "restored Gitlink is not an empty uninitialized directory",
                    ));
                }
            }
            (Some(expected), _) if expected.mode != Mode::Gitlink => {}
            (None, None) => {}
            _ => {
                return Err(Error::Conflict(
                    "restored Gitlink differs from captured tree",
                ))
            }
        }
    }
    // No ignore rules here: an unpublished restore must not adopt additional
    // ignored files, links or special files under the new destination.
    let untracked = run(repo, &["ls-files", "--others", "-z"])?;
    let actual = observe_tree(repo, base, captured, &untracked, false, 0, true)?;
    if &actual != captured {
        return Err(Error::Conflict(
            "restored Git worktree differs from captured tree",
        ));
    }
    repo.validate_binding().map_err(git_error)?;
    Ok(())
}

fn index_line(input: &mut Vec<u8>, mode: &str, oid: &str, path: &RepoPath) {
    input.extend_from_slice(mode.as_bytes());
    input.push(b' ');
    input.extend_from_slice(oid.as_bytes());
    input.push(b'\t');
    input.extend_from_slice(path.bytes());
    input.push(0);
}

fn gitlink(
    repo: &Repository,
    path: &Path,
    indexed: Option<&Entry>,
    depth: usize,
) -> Result<Option<String>> {
    match fs::symlink_metadata(path.join(".git")) {
        Ok(meta) => {
            if link_like(&path.join(".git"), &meta)? {
                return Err(Error::Denied("Gitlink administration cannot be a symlink"));
            }
            let nested = Repository::discover(repo.git(), path).map_err(git_error)?;
            if nested.root() != path.canonicalize()? {
                return Err(Error::Denied(
                    "Gitlink does not identify its own repository",
                ));
            }
            let snapshot = Snapshot::observe(&nested, false, depth + 1)?;
            if !snapshot.clean() || !snapshot.clean_index {
                return Err(Error::Conflict("initialized Git submodule is dirty"));
            }
            Ok(Some(snapshot.base))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // An uninitialized empty worktree retains the indexed gitlink.
            // A missing path (handled above) or removed index entry is deletion.
            if let Some(entry) = indexed.filter(|entry| entry.mode == Mode::Gitlink) {
                if fs::read_dir(path)?.next().transpose()?.is_some() {
                    return Err(Error::Denied(
                        "uninitialized Gitlink contains unexpected files",
                    ));
                }
                Ok(Some(entry.oid.clone()))
            } else {
                Ok(None)
            }
        }
        Err(error) => Err(error.into()),
    }
}

pub(super) struct PrivateIndexes {
    path: PathBuf,
    identity: aw_platform::DirectoryIdentity,
}
impl PrivateIndexes {
    fn path(repo: &Repository, prepared_identity: &str) -> PathBuf {
        repo.git_dir.join(format!("aw-capture-{prepared_identity}"))
    }

    // An interrupted scratch directory has no independently retained
    // physical identity. Never delete it merely from its derived name.
    pub(super) fn require_absent(repo: &Repository, prepared_identity: &str) -> Result<()> {
        match fs::symlink_metadata(Self::path(repo, prepared_identity)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
            Ok(_) => Err(Error::Conflict(
                "original Git private indexes require reconciliation",
            )),
        }
    }

    pub(super) fn create(repo: &Repository, prepared_identity: &str) -> Result<Self> {
        let path = Self::path(repo, prepared_identity);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new().mode(0o700).create(&path)?;
        }
        #[cfg(not(unix))]
        fs::create_dir(&path)?;
        let identity = aw_platform::DirectoryIdentity::open(&path)?;
        Ok(Self { path, identity })
    }
    pub(super) fn index(&self, pass: usize) -> PathBuf {
        self.path.join(format!("index-{pass}"))
    }
}
impl Drop for PrivateIndexes {
    fn drop(&mut self) {
        if self.identity.is_current(&self.path).unwrap_or(false) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
