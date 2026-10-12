//! Human-invoked Git services. No model calls, credentials, checkout, network
//! fetch, recursive watchers, or implicit index updates. Blocking API: use a worker.
//! View selection never supplies an agent's tool grant or provider account.
mod parse;
mod process;
mod review;

pub use parse::{parse_branches, parse_status, parse_worktrees};
pub use review::{parse_diff_lines, LineKind, ReviewComment, ReviewLine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

pub type Result<T> = std::result::Result<T, Error>;
pub const MAX_WIRE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PATCH_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_ROWS: usize = 4096;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Git I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid Git data: {0}")]
    Invalid(&'static str),
    #[error("Git limit: {0}")]
    Limit(&'static str),
    #[error("Git operation failed (exit {0}); inspect the repository with trusted Git if needed")]
    Exit(i32),
    #[error("Git command timed out; no automatic retry")]
    Timeout,
    #[error("review is stale; refresh and inspect the new diff before changing the index")]
    Stale,
    #[error("Git operation unavailable: {0}")]
    Unsupported(&'static str),
    #[error("index update outcome is uncertain; inspect current Git status before a new action")]
    MutationUnknown,
}
/// Exact Git path bytes, separate from an escaped human label. Never a pathspec.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepoPath(Vec<u8>);
impl RepoPath {
    pub fn new(bytes: Vec<u8>) -> Result<Self> {
        if bytes.is_empty()
            || bytes.len() > 16 * 1024
            || bytes.contains(&0)
            || bytes.starts_with(b"/")
            || bytes
                .split(|b| *b == b'/')
                .any(|p| p.is_empty() || p == b"." || p == b".." || p.eq_ignore_ascii_case(b".git"))
        {
            return Err(Error::Invalid("unsafe repository-relative path"));
        }
        Ok(Self(bytes))
    }
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
    pub fn os(&self) -> Result<OsString> {
        aw_platform::os_string_from_bytes(self.0.clone()).map_err(Error::Io)
    }
    pub fn label(&self) -> String {
        match std::str::from_utf8(&self.0) {
            Ok(s) => s
                .chars()
                .map(|c| {
                    if c == '\\' {
                        "\\\\".into()
                    } else {
                        escape_bytes(c.to_string().as_bytes())
                    }
                })
                .collect(),
            Err(_) => escape_bytes(&self.0),
        }
    }
}
pub fn escape_bytes(bytes: &[u8]) -> String {
    let mut out = String::new();
    // Invalid UTF-8 stays distinguishable (not a replacement-character identity).
    match std::str::from_utf8(bytes) {
        Ok(s) => {
            for c in s.chars() {
                if c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
                {
                    out.extend(c.escape_default());
                } else {
                    out.push(c);
                }
            }
        }
        Err(_) => {
            for &b in bytes {
                if (b' '..=b'~').contains(&b) && b != b'\\' {
                    out.push(char::from(b));
                } else {
                    out.push_str(&format!("\\x{b:02x}"));
                }
            }
        }
    }
    out
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeGroup {
    Conflicts,
    Staged,
    Unstaged,
    Untracked,
}
impl ChangeGroup {
    pub fn label(self) -> &'static str {
        match self {
            Self::Conflicts => "Conflicts",
            Self::Staged => "Staged",
            Self::Unstaged => "Unstaged",
            Self::Untracked => "Untracked",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub group: ChangeGroup,
    pub path: RepoPath,
    pub old_path: Option<RepoPath>,
    pub code: char,
    pub submodule: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Status {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub changes: Vec<Change>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub locked: bool,
    pub prunable: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    pub reference: String,
    pub object: String,
    pub current: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexAction {
    Stage,
    Unstage,
}
#[derive(Clone, Debug)]
pub struct Repository {
    git: PathBuf,
    root: PathBuf,
    common: PathBuf,
    timeout: Duration,
}
#[derive(Clone, Debug)]
pub struct Review {
    pub id: String,
    pub root: PathBuf,
    pub basis: String,
    pub path_label: String,
    pub lines: Vec<ReviewLine>,
    pub truncated_display: bool,
    pub binary: bool,
    pub change: Option<Change>,
    patch: Vec<u8>,
    head: Option<String>,
}
impl Review {
    pub fn patch(&self) -> &[u8] {
        &self.patch
    }
    pub fn can_apply(&self, action: IndexAction) -> bool {
        self.change.as_ref().is_some_and(|c| {
            !c.submodule
                && match action {
                    IndexAction::Stage => {
                        matches!(c.group, ChangeGroup::Unstaged | ChangeGroup::Untracked)
                    }
                    IndexAction::Unstage => c.group == ChangeGroup::Staged,
                }
        }) && !self.patch.is_empty()
    }
    pub fn comment(&self, row: usize, text: &str) -> Result<ReviewComment> {
        if text.trim().is_empty() || text.len() > 8192 {
            return Err(Error::Limit("comment must be 1–8192 bytes"));
        }
        let line = self.lines.get(row).ok_or(Error::Invalid("diff row"))?;
        Ok(ReviewComment {
            review_id: self.id.clone(),
            row,
            old_line: line.old,
            new_line: line.new,
            quote: line.text.chars().take(1024).collect(),
            text: text.to_owned(),
        })
    }
}
impl Repository {
    pub fn discover(git: impl AsRef<Path>, directory: impl AsRef<Path>) -> Result<Self> {
        if !git.as_ref().is_absolute() {
            return Err(Error::Invalid("absolute trusted Git executable required"));
        }
        let git = git.as_ref().canonicalize()?;
        if !git.is_file() || !git.is_absolute() {
            return Err(Error::Invalid("absolute trusted Git executable required"));
        }
        let mut repo = Self {
            git,
            root: directory.as_ref().canonicalize()?,
            common: PathBuf::new(),
            timeout: Duration::from_secs(10),
        };
        let top = repo.run(
            &["rev-parse", "--path-format=absolute", "--show-toplevel"],
            None,
            false,
        )?;
        repo.root = output_path(&top)?.canonicalize()?;
        let common = repo.run(
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            None,
            false,
        )?;
        repo.common = output_path(&common)?.canonicalize()?;
        Ok(repo)
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn common_dir(&self) -> &Path {
        &self.common
    }
    pub fn git(&self) -> &Path {
        &self.git
    }
    pub fn status(&self) -> Result<Status> {
        self.guard_local_content()?;
        parse_status(&self.run(
            &[
                "status",
                "--porcelain=v2",
                "-z",
                "--branch",
                "--untracked-files=all",
                "--ignore-submodules=all",
            ],
            None,
            false,
        )?)
    }
    pub fn worktrees(&self) -> Result<Vec<Worktree>> {
        parse_worktrees(&self.run(&["worktree", "list", "--porcelain", "-z"], None, false)?)
    }

    /// Resolve the exact committed HEAD that is safe to use as the base of a
    /// host-managed child worktree. Phase-42 callers deliberately refuse dirty
    /// parents rather than silently omitting staged/unstaged/untracked state.
    pub fn clean_head_for_managed_worktree(&self) -> Result<String> {
        self.guard_managed_checkout()?;
        let status = self.status()?;
        if !status.changes.is_empty() {
            return Err(Error::Unsupported(
                "managed child worktree requires a clean parent checkout",
            ));
        }
        status
            .head
            .filter(|head| valid_object_id(head))
            .ok_or(Error::Unsupported(
                "managed child worktree requires a committed HEAD",
            ))
    }

    /// Create exactly one detached worktree at an already-reviewed object ID.
    /// The target path is supplied by the trusted host, never by model input.
    /// Any failure after Git process entry is treated by the caller as an
    /// uncertain external effect; this function never retries.
    pub fn create_managed_worktree(&self, path: &Path, expected_head: &str) -> Result<Worktree> {
        if !path.is_absolute() || path.exists() || !valid_object_id(expected_head) {
            return Err(Error::Invalid("invalid managed worktree target or object"));
        }
        let parent = path
            .parent()
            .ok_or(Error::Invalid("managed worktree target has no parent"))?;
        let parent = parent.canonicalize()?;
        if !parent.is_dir() {
            return Err(Error::Invalid("managed worktree parent is not a directory"));
        }
        let current = self.clean_head_for_managed_worktree()?;
        if current != expected_head {
            return Err(Error::Stale);
        }
        let args = vec![
            OsString::from("worktree"),
            OsString::from("add"),
            OsString::from("--detach"),
            path.as_os_str().to_owned(),
            OsString::from(expected_head),
        ];
        if self.run_os(&args, None, false).is_err() {
            return Err(Error::MutationUnknown);
        }
        let created = path.canonicalize().map_err(|_| Error::MutationUnknown)?;
        if created.parent() != Some(parent.as_path()) {
            return Err(Error::MutationUnknown);
        }
        let child = Self::discover(&self.git, &created).map_err(|_| Error::MutationUnknown)?;
        if child.common != self.common {
            return Err(Error::MutationUnknown);
        }
        let child_status = child.status().map_err(|_| Error::MutationUnknown)?;
        if child_status.head.as_deref() != Some(expected_head) || !child_status.changes.is_empty() {
            return Err(Error::MutationUnknown);
        }
        self.worktrees()?
            .into_iter()
            .find(|worktree| {
                !worktree.bare
                    && !worktree.prunable
                    && worktree.path.canonicalize().ok().as_ref() == Some(&created)
                    && worktree.head.as_deref() == Some(expected_head)
            })
            .ok_or(Error::MutationUnknown)
    }

    /// Explicit host cleanup of one discovered managed worktree. Without
    /// `discard`, dirty content is retained for human inspection.
    pub fn remove_managed_worktree(&self, path: &Path, discard: bool) -> Result<()> {
        let target = path.canonicalize()?;
        let child = self.select_worktree(&target)?;
        let status = child.status()?;
        if !discard && !status.changes.is_empty() {
            return Err(Error::Stale);
        }
        let mut args = vec![OsString::from("worktree"), OsString::from("remove")];
        if discard {
            args.push(OsString::from("--force"));
        }
        args.push(target.as_os_str().to_owned());
        if self.run_os(&args, None, false).is_err() {
            return Err(Error::MutationUnknown);
        }
        let remains = self
            .worktrees()?
            .into_iter()
            .any(|worktree| worktree.path.canonicalize().ok().as_ref() == Some(&target));
        if remains || target.exists() {
            return Err(Error::MutationUnknown);
        }
        Ok(())
    }
    /// Select only an existing discovered checkout in the same repository.
    /// This changes no files, branch refs, model bindings, or process-global cwd.
    pub fn select_worktree(&self, path: &Path) -> Result<Self> {
        let target = path.canonicalize()?;
        let found = self.worktrees()?.into_iter().any(|w| {
            !w.bare && !w.prunable && w.path.canonicalize().ok().as_ref() == Some(&target)
        });
        if !found {
            return Err(Error::Invalid(
                "checkout was not discovered in this repository",
            ));
        }
        let next = Self::discover(&self.git, &target)?;
        if next.common != self.common {
            return Err(Error::Invalid("repository changed during selection"));
        }
        Ok(next)
    }
    pub fn branches(&self) -> Result<Vec<Branch>> {
        parse_branches(&self.run(
            &[
                "for-each-ref",
                "--count=1025",
                "--format=%(refname)%00%(objectname)%00%(HEAD)%00",
                "refs/heads",
                "refs/remotes",
            ],
            None,
            false,
        )?)
    }
    /// Review a current exact status entry. External diff/textconv and clean
    /// filters cannot execute; unsupported filter/partial-clone repos fail closed.
    pub fn review(&self, change: &Change) -> Result<Review> {
        self.guard_local_content()?;
        let status = self.status()?;
        if !status.changes.contains(change) {
            return Err(Error::Stale);
        }
        self.guard_attributes(&change.path, change.group == ChangeGroup::Untracked)?;
        if let Some(old) = &change.old_path {
            self.guard_attributes(old, false)?;
        }
        let mut args: Vec<OsString> = [
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--binary",
            "--full-index",
            "--no-color",
            "--no-renames",
            "--src-prefix=a/",
            "--dst-prefix=b/",
        ]
        .iter()
        .map(OsString::from)
        .collect();
        let basis = match change.group {
            ChangeGroup::Staged => {
                args.push("--cached".into());
                "HEAD → index"
            }
            ChangeGroup::Unstaged => "index → working tree",
            ChangeGroup::Conflicts => {
                args.push("--cc".into());
                "unmerged / combined working diff (read only)"
            }
            ChangeGroup::Untracked => {
                args.push("--no-index".into());
                "new untracked file → index (raw bytes)"
            }
        };
        args.push("--".into());
        if change.group == ChangeGroup::Untracked {
            // Prevent no-index from traversing directories or following links.
            self.guard_untracked(&change.path)?;
            args.push(aw_platform::null_device().into());
        }
        args.push(change.path.os()?);
        if change.group != ChangeGroup::Untracked {
            if let Some(old) = &change.old_path {
                args.push(old.os()?);
            }
        }
        let patch = self.run_os(&args, None, change.group == ChangeGroup::Untracked)?;
        self.finish_review(
            basis,
            change.path.label(),
            Some(change.clone()),
            patch,
            status.head,
        )
    }
    /// Branch selection is a read-only comparison, never an implicit checkout.
    pub fn compare(&self, branch: &Branch) -> Result<Review> {
        self.guard_local_content()?;
        if !self.branches()?.contains(branch) {
            return Err(Error::Stale);
        }
        let status = self.status()?;
        let head = status
            .head
            .clone()
            .ok_or(Error::Unsupported("no commit yet for branch comparison"))?;
        let patch = self.run(
            &[
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--binary",
                "--full-index",
                "--no-color",
                &head,
                &branch.object,
                "--",
            ],
            None,
            false,
        )?;
        self.finish_review(
            &format!("{head} → {} (read only)", branch.object),
            branch.reference.clone(),
            None,
            patch,
            Some(head),
        )
    }
    fn finish_review(
        &self,
        basis: &str,
        path_label: String,
        change: Option<Change>,
        patch: Vec<u8>,
        head: Option<String>,
    ) -> Result<Review> {
        if patch.len() > MAX_PATCH_BYTES {
            return Err(Error::Limit("patch exceeds 2 MiB; use external review"));
        }
        let mut hash = Sha256::new();
        let root_bytes = aw_platform::os_str_bytes(self.root.as_os_str()).map_err(Error::Io)?;
        for part in [
            root_bytes.as_ref(),
            basis.as_bytes(),
            path_label.as_bytes(),
            head.as_deref().unwrap_or("").as_bytes(),
            &patch,
        ] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part);
        }
        let id = format!("{:x}", hash.finalize());
        let binary = std::str::from_utf8(&patch).is_err()
            || patch.windows(16).any(|w| w == b"GIT binary patch");
        let (lines, truncated_display) = parse_diff_lines(&patch, MAX_ROWS);
        Ok(Review {
            id,
            root: self.root.clone(),
            basis: basis.into(),
            path_label,
            lines,
            truncated_display,
            binary,
            change,
            patch,
            head,
        })
    }
    /// Whole-file snapshot staging/unstaging only. Check a freshly obtained
    /// review, then apply precisely those reviewed bytes to the index, never the
    /// worktree. External writers can race; Git's cached preimage check is final.
    pub fn apply_index(&self, review: &Review, action: IndexAction) -> Result<()> {
        if review.root != self.root || !review.can_apply(action) {
            return Err(Error::Unsupported("this review cannot be staged/unstaged"));
        }
        let change = review
            .change
            .as_ref()
            .ok_or(Error::Invalid("missing change"))?;
        let fresh = self.review(change)?;
        if fresh.id != review.id || fresh.head != review.head {
            return Err(Error::Stale);
        }
        let mut args = vec!["apply", "--cached", "--binary", "--whitespace=nowarn"];
        if action == IndexAction::Unstage {
            args.push("--reverse");
        }
        let mut check = args.clone();
        check.push("--check");
        check.push("-");
        self.run(&check, Some(&review.patch), false)?;
        args.push("-");
        match self.run(&args, Some(&review.patch), false) {
            Ok(_) => Ok(()),
            Err(Error::Exit(code)) => Err(Error::Exit(code)),
            Err(_) => Err(Error::MutationUnknown),
        }
    }
    fn guard_local_content(&self) -> Result<()> {
        // Git diff may invoke clean/process filters even with --no-textconv.
        // Refuse rather than changing the repository's conversion semantics.
        let filters = self.run(
            &[
                "config",
                "--null",
                "--get-regexp",
                "^filter\\..*\\.(clean|process)$",
            ],
            None,
            true,
        )?;
        if !filters.is_empty() {
            return Err(Error::Unsupported(
                "configured clean/process filters; use trusted external Git",
            ));
        }
        let partial = self.run(
            &[
                "config",
                "--null",
                "--get-regexp",
                "^(extensions\\.partialclone|remote\\..*\\.promisor)$",
            ],
            None,
            true,
        )?;
        if !partial.is_empty() {
            return Err(Error::Unsupported(
                "partial-clone/promisor repositories require explicit external handling",
            ));
        }
        Ok(())
    }
    fn guard_managed_checkout(&self) -> Result<()> {
        self.guard_local_content()?;
        let filters = self.run(
            &[
                "config",
                "--null",
                "--get-regexp",
                "^filter\\..*\\.(clean|smudge|process)$",
            ],
            None,
            true,
        )?;
        if !filters.is_empty() {
            return Err(Error::Unsupported(
                "managed worktrees refuse executable Git clean/smudge/process filters",
            ));
        }
        Ok(())
    }

    fn guard_attributes(&self, path: &RepoPath, untracked: bool) -> Result<()> {
        let mut args = vec![OsString::from("check-attr"), "-z".into(), "filter".into()];
        if untracked {
            args.extend(["text".into(), "eol".into(), "working-tree-encoding".into()]);
        }
        args.push("--".into());
        args.push(path.os()?);
        let result = self.run_os(&args, None, false)?;
        let fields = result.split(|b| *b == 0).collect::<Vec<_>>();
        if fields.is_empty()
            || (fields.len() - 1) % 3 != 0
            || !fields.last().is_some_and(|value| value.is_empty())
        {
            return Err(Error::Invalid("attribute reply"));
        }
        for triple in fields[..fields.len() - 1].as_chunks::<3>().0 {
            if ![b"unspecified".as_slice(), b"unset".as_slice()].contains(&triple[2]) {
                return Err(Error::Unsupported(
                    "active filter or untracked conversion attribute; use trusted external Git",
                ));
            }
        }
        if untracked {
            let crlf = self.run(&["config", "--get", "core.autocrlf"], None, true)?;
            if !crlf.is_empty() && crlf != b"false\n" {
                return Err(Error::Unsupported(
                    "untracked autocrlf conversion; use trusted external Git",
                ));
            }
        }
        Ok(())
    }
    fn guard_untracked(&self, path: &RepoPath) -> Result<()> {
        let mut current = self.root.clone();
        for part in path.bytes().split(|b| *b == b'/') {
            current.push(aw_platform::os_string_from_bytes(part.to_vec()).map_err(Error::Io)?);
            if aw_platform::path_is_link_like(&current).map_err(Error::Io)? {
                return Err(Error::Unsupported("untracked symlink/reparse preview"));
            }
        }
        let file = aw_platform::open_nofollow_read(&current).map_err(Error::Io)?;
        let meta = file.metadata()?;
        if !meta.is_file()
            || aw_platform::file_link_count(&file).map_err(Error::Io)? != 1
            || meta.len() > MAX_PATCH_BYTES as u64
        {
            return Err(Error::Unsupported("untracked file type, links or size"));
        }
        // Git subsequently reads the name. This is a trusted local repo workflow,
        // not confinement against racing same-user writers; cached apply stages
        // only the exact resulting reviewed snapshot.
        Ok(())
    }
    fn run(&self, args: &[&str], input: Option<&[u8]>, allow_one: bool) -> Result<Vec<u8>> {
        self.run_os(
            &args.iter().map(OsString::from).collect::<Vec<_>>(),
            input,
            allow_one,
        )
    }
    fn run_os(&self, args: &[OsString], input: Option<&[u8]>, allow_one: bool) -> Result<Vec<u8>> {
        process::run(&self.git, &self.root, args, input, self.timeout, allow_one)
    }
}
fn valid_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn output_path(bytes: &[u8]) -> Result<PathBuf> {
    let path = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    if path.is_empty() || path.contains(&0) {
        return Err(Error::Invalid("Git path output"));
    }
    Ok(PathBuf::from(
        aw_platform::os_string_from_bytes(path.to_vec()).map_err(Error::Io)?,
    ))
}
