//! Human Git navigation: bounded worker, explicit scope, no agent context injection.
use super::background::Executor;
use aw_git::{Branch, Change, ChangeGroup, Repository, Review, ReviewComment, Status, Worktree};
use aw_workspace::{GitContextId, Id};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    #[default]
    Changes,
    Worktrees,
    Branches,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Display {
    #[default]
    List,
    Tree,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Heading(String),
    Folder(String),
    Change(Change),
    Worktree(Worktree),
    Branch(Branch),
}
#[derive(Clone)]
pub struct Row {
    pub label: String,
    pub item: Item,
}
#[derive(Default)]
pub struct Nav {
    pub mode: Mode,
    pub display: Display,
    pub repo: Option<Repository>,
    pub status: Option<Status>,
    pub worktrees: Vec<Worktree>,
    pub branches: Vec<Branch>,
    pub selected: usize,
    pub filter: String,
    pub collapsed: BTreeSet<String>,
    pub remembered: BTreeMap<Mode, (usize, String)>,
    pub notice: String,
    pub pending: Option<u64>,
    pub loaded: bool,
    origin: Option<Origin>,
    context_id: Option<GitContextId>,
    access: Option<Access>,
}
impl Nav {
    pub fn set_display(&mut self, display: Display) {
        let selected = self.rows().get(self.selected).map(|r| r.item.clone());
        self.display = display;
        if let Some(item) = selected {
            if let Some(i) = self.rows().iter().position(|r| r.item == item) {
                self.selected = i;
            }
        }
    }
    fn select_mode(&mut self, mode: Mode) {
        if self.mode == mode {
            return;
        }
        self.remembered
            .insert(self.mode, (self.selected, self.filter.clone()));
        self.mode = mode;
        let (selected, filter) = self.remembered.get(&mode).cloned().unwrap_or_default();
        self.selected = selected;
        self.filter = filter;
    }

    pub fn rows(&self) -> Vec<Row> {
        let query = self.filter.to_lowercase();
        let mut out = vec![];
        match self.mode {
            Mode::Changes => {
                if let Some(status) = &self.status {
                    for group in [
                        ChangeGroup::Conflicts,
                        ChangeGroup::Staged,
                        ChangeGroup::Unstaged,
                        ChangeGroup::Untracked,
                    ] {
                        // aw-git parses status into deterministic (group,path) order on the
                        // background worker already. Preserve that order here instead of
                        // re-sorting up to 4096 rows on every paint/scroll frame.
                        let list = status
                            .changes
                            .iter()
                            .filter(|c| {
                                c.group == group && c.path.label().to_lowercase().contains(&query)
                            })
                            .collect::<Vec<_>>();
                        if list.is_empty() {
                            continue;
                        }
                        let group_key = format!("group:{:?}", group);
                        let group_collapsed = self.collapsed.contains(&group_key);
                        out.push(Row {
                            label: format!(
                                "{} {} ({})",
                                if group_collapsed { ">" } else { "v" },
                                group.label(),
                                list.len()
                            ),
                            item: Item::Heading(group_key),
                        });
                        if group_collapsed {
                            continue;
                        }
                        let mut emitted = BTreeSet::new();
                        for change in list {
                            let label = change.path.label();
                            let parts = label.split('/').collect::<Vec<_>>();
                            let mut hidden = false;
                            if self.display == Display::Tree {
                                let mut prefix = String::new();
                                for (level, part) in
                                    parts.iter().take(parts.len().saturating_sub(1)).enumerate()
                                {
                                    if !prefix.is_empty() {
                                        prefix.push('/');
                                    }
                                    prefix.push_str(part);
                                    let key = format!("{:?}/{prefix}", group);
                                    if emitted.insert(key.clone()) {
                                        out.push(Row {
                                            label: format!(
                                                "{}{} {part}/",
                                                "  ".repeat(level + 1),
                                                if self.collapsed.contains(&key) {
                                                    ">"
                                                } else {
                                                    "v"
                                                }
                                            ),
                                            item: Item::Folder(key.clone()),
                                        });
                                    }
                                    if self.collapsed.contains(&key) {
                                        hidden = true;
                                        break;
                                    }
                                }
                            }
                            if hidden {
                                continue;
                            }
                            let shown = if self.display == Display::Tree {
                                format!(
                                    "{}{} {}",
                                    "  ".repeat(parts.len()),
                                    change.code,
                                    parts.last().copied().unwrap_or("")
                                )
                            } else {
                                format!("  {} {}", change.code, label)
                            };
                            let shown = if let Some(old) = &change.old_path {
                                format!("{shown} [from {}]", old.label())
                            } else {
                                shown
                            };
                            out.push(Row {
                                label: shown,
                                item: Item::Change(change.clone()),
                            });
                        }
                    }
                }
            }
            Mode::Worktrees => {
                for worktree in &self.worktrees {
                    let root = aw_git::escape_bytes(worktree.path.as_os_str().as_encoded_bytes());
                    let label = format!(
                        "{} {} {}{}{}",
                        if self
                            .repo
                            .as_ref()
                            .is_some_and(|r| r.root() == worktree.path.as_path())
                        {
                            "*"
                        } else {
                            " "
                        },
                        worktree.branch.as_deref().unwrap_or("(detached/bare)"),
                        root,
                        if worktree.locked { " [locked]" } else { "" },
                        if worktree.prunable { " [prunable]" } else { "" }
                    );
                    if label.to_lowercase().contains(&query) {
                        out.push(Row {
                            label: aw_git::escape_bytes(label.as_bytes()),
                            item: Item::Worktree(worktree.clone()),
                        });
                    }
                }
            }
            Mode::Branches => {
                for branch in &self.branches {
                    let label = format!(
                        "{} {}  {}",
                        if branch.current { "*" } else { " " },
                        branch.reference,
                        branch.object.get(..12).unwrap_or(&branch.object)
                    );
                    if label.to_lowercase().contains(&query) {
                        out.push(Row {
                            label: aw_git::escape_bytes(label.as_bytes()),
                            item: Item::Branch(branch.clone()),
                        });
                    }
                }
            }
        }
        out
    }
}
/// A live UI origin. Serialized pane IDs alone never admit a completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Origin {
    pub container: Option<Id>,
    pub pane: Id,
    pub lifetime: u64,
}

/// Discovered only on the bounded executor following a human action.
pub struct Grant {
    repo: Repository,
}
impl fmt::Debug for Grant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Grant").finish_non_exhaustive()
    }
}
struct Binding {
    repo: Repository,
    valid: AtomicBool,
}
#[derive(Clone)]
pub struct Access {
    context_id: GitContextId,
    epoch: u64,
    binding: Arc<Binding>,
}
impl Access {
    pub fn context_id(&self) -> &GitContextId {
        &self.context_id
    }
    pub fn is_valid(&self) -> bool {
        self.binding.valid.load(Ordering::Acquire)
    }
    /// Blocking admission for an executor/owner job, never the UI event thread.
    /// This observes repository identity; concurrent external writers can race
    /// the operation after admission, as with the existing Git process API.
    pub fn validate_binding(&self) -> aw_git::Result<()> {
        if !self.is_valid() {
            return Err(aw_git::Error::Stale);
        }
        self.binding.repo.validate_binding()?;
        if !self.is_valid() {
            return Err(aw_git::Error::Stale);
        }
        Ok(())
    }
    pub fn same_binding(&self, other: &Self) -> bool {
        self.is_valid()
            && other.is_valid()
            && self.context_id == other.context_id
            && self.epoch == other.epoch
            && Arc::ptr_eq(&self.binding, &other.binding)
    }
    fn revoke(&self) {
        self.binding.valid.store(false, Ordering::Release);
    }
}
impl fmt::Debug for Access {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Access")
            .field("context_id", &self.context_id)
            .field("epoch", &self.epoch)
            .field("valid", &self.is_valid())
            .finish()
    }
}

/// Retained human repository capabilities. Saved IDs and labels supply no grant.
pub struct Contexts {
    launch_root: PathBuf,
    launch_identity: Option<Arc<aw_platform::DirectoryIdentity>>,
    launch_repo: Option<PathBuf>,
    bindings: BTreeMap<GitContextId, Access>,
    next_epoch: u64,
}
impl Contexts {
    pub fn new(root: PathBuf) -> Self {
        let launch_root = root.canonicalize().unwrap_or(root);
        let launch_identity = aw_platform::DirectoryIdentity::open(&launch_root)
            .ok()
            .map(Arc::new);
        Self {
            launch_root,
            launch_identity,
            launch_repo: None,
            bindings: BTreeMap::new(),
            next_epoch: 0,
        }
    }
    pub fn access(&self, id: &GitContextId) -> Option<Access> {
        self.bindings.get(id).filter(|a| a.is_valid()).cloned()
    }
    pub fn label(&self, id: &GitContextId) -> Option<&Path> {
        self.bindings
            .get(id)
            .filter(|a| a.is_valid())
            .map(|a| a.binding.repo.root())
    }
    pub fn install(&mut self, grant: Grant) -> std::result::Result<GitContextId, String> {
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).map_err(|_| "Git context randomness unavailable")?;
        let id = GitContextId(nonce.iter().map(|byte| format!("{byte:02x}")).collect());
        if self.bindings.contains_key(&id) {
            return Err("Git context ID collision; retry explicitly".into());
        }
        self.bind(id.clone(), grant)?;
        Ok(id)
    }
    pub fn rebind(&mut self, id: &GitContextId, grant: Grant) -> std::result::Result<(), String> {
        self.bind(id.clone(), grant)
    }
    fn bind(&mut self, id: GitContextId, grant: Grant) -> std::result::Result<(), String> {
        if !id.is_valid() {
            return Err("Invalid Git context ID".into());
        }
        if self.bindings.len() >= 8 && !self.bindings.contains_key(&id) {
            return Err("At most 8 Git contexts may be open".into());
        }
        if id == GitContextId::launch()
            && (self.launch_identity.is_none()
                || !self.launch_root.starts_with(grant.repo.root())
                || self
                    .launch_repo
                    .as_ref()
                    .is_some_and(|root| root != grant.repo.root()))
        {
            return Err("Launch Git context can only reconnect its original repository".into());
        }
        let epoch = self
            .next_epoch
            .checked_add(1)
            .ok_or("Git binding counter exhausted")?;
        self.next_epoch = epoch;
        if id == GitContextId::launch() {
            self.launch_repo = Some(grant.repo.root().to_owned());
        }
        let access = Access {
            context_id: id.clone(),
            epoch,
            binding: Arc::new(Binding {
                repo: grant.repo,
                valid: AtomicBool::new(true),
            }),
        };
        if let Some(old) = self.bindings.insert(id, access) {
            old.revoke();
        }
        Ok(())
    }
    pub fn close(&mut self, id: &GitContextId) -> bool {
        if *id == GitContextId::launch() {
            return false;
        }
        if let Some(access) = self.bindings.remove(id) {
            access.revoke();
            true
        } else {
            false
        }
    }
    pub fn retain(&mut self, referenced: &BTreeSet<GitContextId>) {
        let removed = self
            .bindings
            .keys()
            .filter(|id| **id != GitContextId::launch() && !referenced.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        for id in removed {
            self.close(&id);
        }
    }
}
impl Drop for Contexts {
    fn drop(&mut self) {
        for access in self.bindings.values() {
            access.revoke();
        }
    }
}

/// Owner-thread admission for a particular displayed immutable review.
/// Closing/restoring/reconnecting invalidates queued requests. An already
/// admitted Git effect remains an external effect in progress, not cancellable.
#[derive(Debug)]
pub struct IndexRequest {
    access: Access,
    admission: Arc<AtomicBool>,
    review: Review,
    action: aw_git::IndexAction,
}
struct Admission(Arc<AtomicBool>);
impl std::ops::Deref for Admission {
    type Target = AtomicBool;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Drop for Admission {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl IndexRequest {
    pub fn apply(self) -> aw_git::Result<(Review, aw_git::IndexAction)> {
        if !self.admission.load(Ordering::Acquire) || !self.access.is_valid() {
            return Err(aw_git::Error::Stale);
        }
        self.access.validate_binding()?;
        if !self.admission.load(Ordering::Acquire) || !self.access.is_valid() {
            return Err(aw_git::Error::Stale);
        }
        self.access
            .binding
            .repo
            .apply_index(&self.review, self.action)?;
        Ok((self.review, self.action))
    }
}

#[derive(Clone)]
enum Job {
    Refresh,
    Review(Change),
    Select(Worktree),
    Compare(Branch),
}
struct Request {
    access: Option<Access>,
    launch_identity: Option<Arc<aw_platform::DirectoryIdentity>>,
    git: PathBuf,
    root: PathBuf,
    mode: Mode,
    job: Job,
}
#[derive(Clone)]
enum ReplyKind {
    Navigation {
        context_id: GitContextId,
        access: Option<Access>,
    },
    Root {
        reconnect: Option<GitContextId>,
    },
}
pub struct Reply {
    ticket: u64,
    origin: Origin,
    kind: ReplyKind,
    result: std::result::Result<Data, String>,
}
enum Data {
    Inventory {
        repo: Repository,
        status: Option<Status>,
        worktrees: Vec<Worktree>,
        branches: Vec<Branch>,
    },
    Review {
        review: Review,
    },
    Grant(Grant),
}
pub enum Event {
    Opened {
        origin: Origin,
        result: std::result::Result<Grant, String>,
    },
    Reconnected {
        origin: Origin,
        context_id: GitContextId,
        result: std::result::Result<Grant, String>,
    },
    Selected {
        origin: Origin,
        result: std::result::Result<Grant, String>,
    },
    ReviewReady {
        origin: Origin,
        context_id: GitContextId,
        access: Access,
        review: String,
    },
}
fn execute(request: &Request) -> aw_git::Result<Data> {
    let check_launch = || -> aw_git::Result<()> {
        if let Some(identity) = &request.launch_identity {
            if !identity.is_current(&request.root)? {
                return Err(aw_git::Error::Invalid("original launch directory changed"));
            }
        }
        Ok(())
    };
    check_launch()?;
    let repo = match &request.access {
        Some(access) if access.is_valid() => access.binding.repo.clone(),
        Some(_) => return Err(aw_git::Error::Stale),
        None => Repository::discover(&request.git, &request.root)?,
    };
    let data: aw_git::Result<Data> = match &request.job {
        Job::Review(change) => Ok(Data::Review {
            review: repo.review(change)?,
        }),
        Job::Compare(branch) => Ok(Data::Review {
            review: repo.compare(branch)?,
        }),
        Job::Select(worktree) => Ok(Data::Grant(Grant {
            repo: repo.select_worktree(&worktree.path)?,
        })),
        Job::Refresh => {
            let (status, worktrees, branches) = match request.mode {
                Mode::Changes => (Some(repo.status()?), vec![], vec![]),
                Mode::Worktrees => (None, repo.worktrees()?, vec![]),
                Mode::Branches => (None, vec![], repo.branches()?),
            };
            Ok(Data::Inventory {
                repo,
                status,
                worktrees,
                branches,
            })
        }
    };
    let data = data?;
    check_launch()?;
    Ok(data)
}
pub struct Views {
    pub contexts: Contexts,
    git: PathBuf,
    background: Executor,
    reply_tx: tokio::sync::mpsc::Sender<Reply>,
    reply_rx: tokio::sync::mpsc::Receiver<Reply>,
    pub panes: BTreeMap<Id, Nav>,
    pub reviews: BTreeMap<String, (Repository, Review)>,
    pub comments: BTreeMap<String, Vec<ReviewComment>>,
    pub reviewed: BTreeSet<String>,
    review_access: BTreeMap<String, Access>,
    admissions: BTreeMap<String, Admission>,
    ticket: u64,
    lifetime: Option<u64>,
    root_pending: Option<(u64, Origin)>,
    pub revision: u64,
}
impl Views {
    #[cfg(test)]
    pub fn new(root: PathBuf, background: Executor) -> Self {
        Self::new_with_contexts(Contexts::new(root), background)
    }
    pub fn new_with_contexts(contexts: Contexts, background: Executor) -> Self {
        let git = std::env::var_os("AW_GIT")
            .map(PathBuf::from)
            .or_else(|| option_env!("AW_DEFAULT_GIT").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/usr/bin/git"));
        let (reply_tx, reply_rx) = tokio::sync::mpsc::channel(32);
        Self {
            contexts,
            git,
            background,
            reply_tx,
            reply_rx,
            panes: BTreeMap::new(),
            reviews: BTreeMap::new(),
            comments: BTreeMap::new(),
            reviewed: BTreeSet::new(),
            review_access: BTreeMap::new(),
            admissions: BTreeMap::new(),
            ticket: 0,
            lifetime: None,
            root_pending: None,
            revision: 0,
        }
    }
    /// Active navigators admit completions. Dormant serialized bindings retain
    /// grants, while displayed immutable Diff references retain their exact repo.
    pub fn sync(
        &mut self,
        lifetime: u64,
        bindings: &[(Id, Id, GitContextId)],
        live_reviews: &BTreeSet<String>,
        referenced_contexts: &BTreeSet<GitContextId>,
    ) {
        let restored = self.lifetime != Some(lifetime);
        self.lifetime = Some(lifetime);
        if restored {
            self.root_pending = None;
        }
        let active = bindings
            .iter()
            .map(|(container, pane, context)| {
                (
                    *pane,
                    (
                        Origin {
                            container: Some(*container),
                            pane: *pane,
                            lifetime,
                        },
                        context.clone(),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        for (pane, nav) in &mut self.panes {
            let next = active.get(pane);
            let changed = restored
                || next.map(|(origin, id)| (Some(*origin), Some(id)))
                    != Some((nav.origin, nav.context_id.as_ref()))
                || match (
                    nav.access.as_ref(),
                    nav.context_id
                        .as_ref()
                        .and_then(|id| self.contexts.access(id)),
                ) {
                    (Some(old), Some(current)) => !old.same_binding(&current),
                    (None, None) => false,
                    _ => true,
                };
            if changed {
                nav.pending = None;
                nav.loaded = false;
                nav.repo = None;
                nav.status = None;
                nav.worktrees.clear();
                nav.branches.clear();
                nav.access = None;
            }
            nav.origin = next.map(|(origin, _)| *origin);
            if let Some((_, id)) = next {
                nav.context_id = Some(id.clone());
            }
        }
        for (pane, (origin, context_id)) in active {
            let nav = self.panes.entry(pane).or_default();
            nav.origin = Some(origin);
            nav.context_id = Some(context_id.clone());
            if nav.access.is_none() {
                nav.access = self.contexts.access(&context_id);
                nav.repo = nav.access.as_ref().map(|a| a.binding.repo.clone());
            }
        }
        let invalid = self
            .review_access
            .iter()
            .filter(|(_, access)| !access.is_valid())
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in invalid {
            self.remove_review(&id);
        }
        let old = self
            .admissions
            .keys()
            .filter(|id| restored || !live_reviews.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        for id in old {
            if let Some(admission) = self.admissions.remove(&id) {
                admission.store(false, Ordering::Release);
            }
        }
        let mut retained = referenced_contexts.clone();
        for id in live_reviews {
            if let Some(access) = self
                .review_access
                .get(id)
                .filter(|access| access.is_valid())
            {
                retained.insert(access.context_id().clone());
                self.admissions
                    .entry(id.clone())
                    .or_insert_with(|| Admission(Arc::new(AtomicBool::new(true))));
            }
        }
        self.contexts.retain(&retained);
        let invalid = self
            .review_access
            .iter()
            .filter(|(_, access)| !access.is_valid())
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in invalid {
            self.remove_review(&id);
        }
    }
    fn remove_review(&mut self, id: &str) {
        self.revision = self.revision.wrapping_add(1);
        self.reviews.remove(id);
        self.review_access.remove(id);
        self.comments.remove(id);
        self.reviewed.remove(id);
        if let Some(admission) = self.admissions.remove(id) {
            admission.store(false, Ordering::Release);
        }
    }
    pub fn review_access(&self, id: &str) -> Option<Access> {
        self.review_access
            .get(id)
            .filter(|access| access.is_valid())
            .cloned()
    }
    pub fn index_request(
        &self,
        id: &str,
        action: aw_git::IndexAction,
    ) -> std::result::Result<IndexRequest, String> {
        let access = self
            .review_access(id)
            .ok_or("Review context expired; reopen it from Git")?;
        let admission = self
            .admissions
            .get(id)
            .filter(|token| token.load(Ordering::Acquire))
            .map(|token| token.0.clone())
            .ok_or("Review is no longer displayed; reopen it from Git")?;
        let review = self
            .reviews
            .get(id)
            .ok_or("Review expired; reopen it from Git")?
            .1
            .clone();
        Ok(IndexRequest {
            access,
            admission,
            review,
            action,
        })
    }
    fn next_ticket(&mut self) -> std::result::Result<u64, String> {
        self.ticket = self
            .ticket
            .checked_add(1)
            .ok_or("Git request counter exhausted")?;
        Ok(self.ticket)
    }
    pub fn open_root(
        &mut self,
        origin: Origin,
        path: PathBuf,
        reconnect: Option<GitContextId>,
    ) -> std::result::Result<(), String> {
        if self.root_pending.is_some() {
            return Err("Git folder request still pending".into());
        }
        let ticket = self.next_ticket()?;
        let launch_check = if reconnect.as_ref() == Some(&GitContextId::launch()) {
            Some((
                self.contexts.launch_root.clone(),
                self.contexts
                    .launch_identity
                    .clone()
                    .ok_or("Original launch directory unavailable")?,
                self.contexts.launch_repo.clone(),
            ))
        } else {
            None
        };
        let git = self.git.clone();
        let out = self.reply_tx.clone();
        let panic_out = out.clone();
        let kind = ReplyKind::Root { reconnect };
        let panic_kind = kind.clone();
        self.background
            .try_submit_with_panic(
                "git",
                move || {
                    let result = (|| -> aw_git::Result<Repository> {
                        let check = || -> aw_git::Result<()> {
                            if let Some((root, identity, _)) = &launch_check {
                                if !identity.is_current(root)? {
                                    return Err(aw_git::Error::Invalid(
                                        "original launch directory changed",
                                    ));
                                }
                            }
                            Ok(())
                        };
                        check()?;
                        let repo = Repository::discover(git, path)?;
                        check()?;
                        if let Some((root, _, expected_repo)) = &launch_check {
                            if !root.starts_with(repo.root())
                                || expected_repo
                                    .as_ref()
                                    .is_some_and(|expected| expected != repo.root())
                            {
                                return Err(aw_git::Error::Invalid("launch repository changed"));
                            }
                        }
                        Ok(repo)
                    })()
                    .map(|repo| Data::Grant(Grant { repo }))
                    .map_err(|e| e.to_string());
                    let _ = out.blocking_send(Reply {
                        ticket,
                        origin,
                        kind,
                        result,
                    });
                },
                move || {
                    let _ = panic_out.blocking_send(Reply {
                        ticket,
                        origin,
                        kind: panic_kind,
                        result: Err("Git background job panicked; retry explicitly".into()),
                    });
                },
            )
            .map_err(|e| e.to_string())?;
        self.root_pending = Some((ticket, origin));
        Ok(())
    }
    fn request(&mut self, pane: Id, job: Job) -> std::result::Result<(), String> {
        let ticket = self.next_ticket()?;
        let nav = self
            .panes
            .get(&pane)
            .ok_or("Git navigator is unavailable")?;
        let origin = nav.origin.ok_or("Git navigator is not active")?;
        let context_id = nav.context_id.clone().ok_or("Git context is unavailable")?;
        let access = self.contexts.access(&context_id);
        if access.is_none()
            && (context_id != GitContextId::launch() || self.contexts.launch_identity.is_none())
        {
            return Err("Git folder unavailable; explicitly reconnect this navigator".into());
        }
        let request = Request {
            access: access.clone(),
            launch_identity: if access.is_none() {
                self.contexts.launch_identity.clone()
            } else {
                None
            },
            git: self.git.clone(),
            root: self.contexts.launch_root.clone(),
            mode: nav.mode,
            job,
        };
        let kind = ReplyKind::Navigation { context_id, access };
        let panic_kind = kind.clone();
        let out = self.reply_tx.clone();
        let panic_out = out.clone();
        self.background
            .try_submit_with_panic(
                "git",
                move || {
                    let result = execute(&request).map_err(|e| e.to_string());
                    let _ = out.blocking_send(Reply {
                        ticket,
                        origin,
                        kind,
                        result,
                    });
                },
                move || {
                    let _ = panic_out.blocking_send(Reply {
                        ticket,
                        origin,
                        kind: panic_kind,
                        result: Err("Git background job panicked; retry explicitly".into()),
                    });
                },
            )
            .map_err(|e| e.to_string())?;
        let nav = self.panes.get_mut(&pane).ok_or("Git navigator closed")?;
        nav.pending = Some(ticket);
        nav.notice.clear();
        Ok(())
    }
    pub fn refresh(&mut self, pane: Id) -> std::result::Result<(), String> {
        self.request(pane, Job::Refresh)
    }
    pub fn ensure(&mut self, pane: Id) {
        let nav = self.panes.entry(pane).or_default();
        if !nav.loaded && nav.pending.is_none() {
            if let Err(error) = self.refresh(pane) {
                let nav = self.panes.entry(pane).or_default();
                nav.loaded = true;
                nav.notice = error;
            }
        }
    }
    pub fn set_mode(&mut self, pane: Id, mode: Mode) -> std::result::Result<(), String> {
        let nav = self.panes.entry(pane).or_default();
        nav.select_mode(mode);
        nav.loaded = false;
        self.refresh(pane)
    }
    pub fn open(&mut self, pane: Id) -> std::result::Result<(), String> {
        let nav = self.panes.entry(pane).or_default();
        if nav.pending.is_some() {
            return Err("Git request still pending".into());
        }
        match nav.rows().get(nav.selected).map(|row| row.item.clone()) {
            Some(Item::Change(change)) => self.request(pane, Job::Review(change)),
            Some(Item::Worktree(worktree)) => self.request(pane, Job::Select(worktree)),
            Some(Item::Branch(branch)) => self.request(pane, Job::Compare(branch)),
            Some(Item::Folder(key)) | Some(Item::Heading(key)) => {
                if !nav.collapsed.remove(&key) {
                    nav.collapsed.insert(key);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    pub async fn next(&mut self) -> Option<Reply> {
        self.reply_rx.recv().await
    }
    pub fn response(&mut self, reply: Reply) -> Option<Event> {
        let Reply {
            ticket,
            origin,
            kind,
            result,
        } = reply;
        if let ReplyKind::Root { reconnect } = kind {
            if self.root_pending != Some((ticket, origin)) || self.lifetime != Some(origin.lifetime)
            {
                return None;
            }
            self.root_pending = None;
            let result = result.and_then(|data| match data {
                Data::Grant(grant) => Ok(grant),
                _ => Err("Invalid Git folder response".into()),
            });
            return Some(match reconnect {
                Some(context_id) => Event::Reconnected {
                    origin,
                    context_id,
                    result,
                },
                None => Event::Opened { origin, result },
            });
        }
        let ReplyKind::Navigation {
            context_id,
            access: requested_access,
        } = kind
        else {
            return None;
        };
        let nav = self.panes.get(&origin.pane)?;
        if nav.pending != Some(ticket)
            || nav.origin != Some(origin)
            || nav.context_id.as_ref() != Some(&context_id)
            || self.lifetime != Some(origin.lifetime)
        {
            return None;
        }
        if let Some(requested) = &requested_access {
            if self
                .contexts
                .access(&context_id)
                .is_none_or(|current| !current.same_binding(requested))
            {
                return None;
            }
        } else if context_id != GitContextId::launch()
            || self.contexts.launch_identity.is_none()
            || self.contexts.access(&context_id).is_some()
        {
            let nav = self.panes.get_mut(&origin.pane)?;
            nav.pending = None;
            nav.loaded = false;
            return None;
        }
        let nav = self.panes.get_mut(&origin.pane)?;
        nav.pending = None;
        nav.loaded = true;
        self.revision = self.revision.wrapping_add(1);
        let data = match result {
            Ok(data) => data,
            Err(error) => {
                nav.notice = error;
                return None;
            }
        };
        match data {
            Data::Grant(grant) => Some(Event::Selected {
                origin,
                result: Ok(grant),
            }),
            Data::Inventory {
                repo,
                status,
                worktrees,
                branches,
            } => {
                if self.contexts.access(&context_id).is_none() {
                    if let Err(error) = self.contexts.bind(context_id.clone(), Grant { repo }) {
                        self.panes.get_mut(&origin.pane)?.notice = error;
                        return None;
                    }
                }
                let access = self.contexts.access(&context_id)?;
                let nav = self.panes.get_mut(&origin.pane)?;
                let selected = nav.rows().get(nav.selected).map(|row| row.item.clone());
                if status.is_some() {
                    nav.select_mode(Mode::Changes);
                    nav.status = status;
                }
                nav.repo = Some(access.binding.repo.clone());
                nav.access = Some(access);
                nav.worktrees = worktrees;
                nav.branches = branches;
                if let Some(item) = selected {
                    nav.selected = nav
                        .rows()
                        .iter()
                        .position(|row| row.item == item)
                        .unwrap_or(0);
                }
                nav.notice =
                    "Enter opens a diff / chooses existing worktree / compares a branch".into();
                None
            }
            Data::Review { review } => {
                let access = self.contexts.access(&context_id)?;
                let id = review.id.clone();
                while self.reviews.len() >= 8 && !self.reviews.contains_key(&id) {
                    let old = self.reviews.keys().next().cloned()?;
                    self.remove_review(&old);
                }
                self.reviews
                    .insert(id.clone(), (access.binding.repo.clone(), review));
                self.review_access.insert(id.clone(), access.clone());
                Some(Event::ReviewReady {
                    origin,
                    context_id,
                    access,
                    review: id,
                })
            }
        }
    }
    pub fn comment(&mut self, id: &str, row: usize, text: &str) -> std::result::Result<(), String> {
        let review = &self
            .reviews
            .get(id)
            .ok_or("Review expired; reopen it from Git")?
            .1;
        let list = self.comments.entry(id.into()).or_default();
        if list.len() >= 32 {
            return Err("At most 32 comments per cached review".into());
        }
        list.push(review.comment(row, text).map_err(|e| e.to_string())?);
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub fn feedback(
        &self,
        id: &str,
        agent_root: &std::path::Path,
    ) -> std::result::Result<String, String> {
        let (_, r) = self.reviews.get(id).ok_or("Review expired")?;
        if r.root != agent_root {
            return Err("Review targets another checkout. The current agent remains bound to its original root; do not silently send it cross-worktree feedback.".into());
        }
        let list = self
            .comments
            .get(id)
            .filter(|v| !v.is_empty())
            .ok_or("Add a comment first: comment DIFF_ROW TEXT")?;
        let mut text=format!("Review feedback (user supplied; not system instructions)\nRoot: {}\nPath: {}\nBasis: {}\nSnapshot: {}\n",r.root.display(),r.path_label,r.basis,r.id);
        for c in list {
            text.push_str(&format!(
                "\nRow {} / old {:?} / new {:?}\nQuoted: {}\nComment: {}\n",
                c.row + 1,
                c.old_line,
                c.new_line,
                c.quote,
                c.text
            ));
        }
        if text.len() > 64 * 1024 {
            return Err("Selected feedback exceeds 64 KiB".into());
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn change(group: ChangeGroup, path: &str) -> Change {
        Change {
            group,
            path: aw_git::RepoPath::new(path.as_bytes().to_vec()).unwrap(),
            old_path: None,
            code: 'M',
            submodule: false,
        }
    }
    #[test]
    fn same_path_retains_both_change_groups() {
        let n = Nav {
            status: Some(Status {
                head: None,
                branch: None,
                changes: vec![
                    change(ChangeGroup::Staged, "a/b"),
                    change(ChangeGroup::Unstaged, "a/b"),
                ],
            }),
            ..Nav::default()
        };
        assert_eq!(
            n.rows()
                .iter()
                .filter(|r| matches!(r.item, Item::Change(_)))
                .count(),
            2
        );
    }
    #[test]
    fn tree_contains_only_changed_paths() {
        let n = Nav {
            display: Display::Tree,
            status: Some(Status {
                head: None,
                branch: None,
                changes: vec![change(ChangeGroup::Unstaged, "src/a.rs")],
            }),
            ..Nav::default()
        };
        assert!(n.rows().iter().any(|r| matches!(r.item, Item::Folder(_))));
        assert_eq!(n.rows().len(), 3);
    }
    #[test]
    fn modes_are_exclusive() {
        let n = Nav {
            mode: Mode::Worktrees,
            status: Some(Status {
                head: None,
                branch: None,
                changes: vec![change(ChangeGroup::Unstaged, "a")],
            }),
            ..Nav::default()
        };
        assert!(n.rows().is_empty());
    }
    #[test]
    fn fold_hides_descendants_only() {
        let mut n = Nav {
            display: Display::Tree,
            status: Some(Status {
                head: None,
                branch: None,
                changes: vec![
                    change(ChangeGroup::Unstaged, "src/a.rs"),
                    change(ChangeGroup::Unstaged, "readme"),
                ],
            }),
            ..Nav::default()
        };
        n.collapsed.insert("Unstaged/src".into());
        assert_eq!(
            n.rows()
                .iter()
                .filter(|r| matches!(r.item, Item::Change(_)))
                .count(),
            1
        );
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    #[test]
    fn list_tree_preserves_exact_selected_change() {
        let c = Change {
            group: ChangeGroup::Unstaged,
            path: aw_git::RepoPath::new(b"src/a".to_vec()).unwrap(),
            old_path: None,
            code: 'M',
            submodule: false,
        };
        let mut n = Nav {
            status: Some(Status {
                branch: None,
                head: None,
                changes: vec![c.clone()],
            }),
            selected: 1,
            ..Nav::default()
        };
        n.set_display(Display::Tree);
        assert_eq!(n.rows()[n.selected].item, Item::Change(c));
    }
    #[test]
    fn subviews_restore_filter_and_selection() {
        let mut n = Nav {
            filter: "auth".into(),
            selected: 3,
            ..Nav::default()
        };
        n.select_mode(Mode::Worktrees);
        assert!(n.filter.is_empty());
        n.filter = "other".into();
        n.select_mode(Mode::Changes);
        assert_eq!(n.filter, "auth");
        assert_eq!(n.selected, 3);
    }
}

#[cfg(test)]
mod capability_tests {
    use super::*;
    use std::{
        fs,
        process::Command,
        sync::Barrier,
        time::{Duration, Instant},
    };

    struct Fixture {
        parent: PathBuf,
        root: PathBuf,
        other: PathBuf,
        git: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let parent = crate::test_support::temp_dir("git-contexts");
            let root = parent.join("main");
            let other = parent.join("other");
            fs::create_dir(&root).unwrap();
            let git = std::env::var_os("AW_GIT")
                .map(PathBuf::from)
                .or_else(|| option_env!("AW_DEFAULT_GIT").map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("/usr/bin/git"));
            let fixture = Self {
                parent,
                root,
                other,
                git,
            };
            fixture.run(&fixture.root, &["init", "--initial-branch=main"]);
            fixture.run(&fixture.root, &["config", "user.name", "Fixture"]);
            fixture.run(
                &fixture.root,
                &["config", "user.email", "fixture@example.invalid"],
            );
            fs::write(fixture.root.join("same.txt"), "original\n").unwrap();
            fixture.run(&fixture.root, &["add", "same.txt"]);
            fixture.run(&fixture.root, &["commit", "--no-gpg-sign", "-m", "fixture"]);
            fixture.run(
                &fixture.root,
                &[
                    "worktree",
                    "add",
                    "-b",
                    "other",
                    fixture.other.to_str().unwrap(),
                ],
            );
            fs::write(fixture.root.join("same.txt"), "Alpha 雪\n").unwrap();
            fs::write(fixture.other.join("same.txt"), "Beta λ\n").unwrap();
            fixture
        }
        fn run(&self, root: &Path, args: &[&str]) -> Vec<u8> {
            let output = Command::new(&self.git)
                .current_dir(root)
                .env_clear()
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", aw_platform::null_device())
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "Git fixture {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        }
        fn grant(&self, root: &Path) -> Grant {
            Grant {
                repo: Repository::discover(&self.git, root).unwrap(),
            }
        }
        fn views(&self) -> (Views, GitContextId) {
            let mut contexts = Contexts::new(self.root.clone());
            let other = contexts.install(self.grant(&self.other)).unwrap();
            (
                Views::new_with_contexts(contexts, Executor::new().unwrap()),
                other,
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.parent);
        }
    }
    fn bindings(other: &GitContextId) -> Vec<(Id, Id, GitContextId)> {
        vec![(10, 11, GitContextId::launch()), (20, 21, other.clone())]
    }
    fn sync(views: &mut Views, other: &GitContextId, lifetime: u64, reviews: &[String]) {
        views.sync(
            lifetime,
            &bindings(other),
            &reviews.iter().cloned().collect(),
            &[GitContextId::launch(), other.clone()]
                .into_iter()
                .collect(),
        );
    }
    fn reply(views: &mut Views) -> Reply {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            match views.reply_rx.try_recv() {
                Ok(reply) => return reply,
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                _ => panic!("Git executor did not return a reply"),
            }
        }
    }
    fn load(views: &mut Views, pane: Id) {
        views.refresh(pane).unwrap();
        let response = reply(views);
        assert!(response.result.is_ok());
        assert!(views.response(response).is_none());
        assert!(views.panes[&pane].status.is_some());
    }
    fn request_review(views: &mut Views, pane: Id) {
        let nav = views.panes.get_mut(&pane).unwrap();
        nav.selected = nav
            .rows()
            .iter()
            .position(|row| matches!(row.item, Item::Change(_)))
            .unwrap();
        views.open(pane).unwrap();
    }
    fn review(views: &mut Views, pane: Id) -> String {
        request_review(views, pane);
        let response = reply(views);
        match views.response(response) {
            Some(Event::ReviewReady { origin, review, .. }) => {
                assert_eq!(origin.pane, pane);
                review
            }
            _ => panic!("Expected an exact-origin immutable review"),
        }
    }

    #[test]
    fn two_worktrees_keep_exact_reviews_and_only_selected_index_changes() {
        let fixture = Fixture::new();
        let (mut views, other) = fixture.views();
        sync(&mut views, &other, 1, &[]);
        load(&mut views, 11);
        load(&mut views, 21);
        let main = review(&mut views, 11);
        let secondary = review(&mut views, 21);
        assert_ne!(main, secondary);
        assert_eq!(views.reviews[&main].1.root, fixture.root);
        assert_eq!(views.reviews[&secondary].1.root, fixture.other);
        assert!(views.reviews[&main]
            .1
            .lines
            .iter()
            .any(|line| line.text.contains("Alpha 雪")));
        assert!(views.reviews[&secondary]
            .1
            .lines
            .iter()
            .any(|line| line.text.contains("Beta λ")));
        sync(&mut views, &other, 1, &[main, secondary.clone()]);
        let main_index = fixture.run(&fixture.root, &["ls-files", "--stage"]);
        let main_head = fixture.run(&fixture.root, &["rev-parse", "HEAD"]);
        let other_head = fixture.run(&fixture.other, &["rev-parse", "HEAD"]);
        // A moved immutable Diff intentionally keeps its original context after
        // its navigator/container closes. It never follows a new selection.
        views.panes.clear();
        views.sync(
            1,
            &[],
            &[secondary.clone()].into_iter().collect(),
            &BTreeSet::new(),
        );
        assert!(views.contexts.access(&other).is_some());
        views
            .index_request(&secondary, aw_git::IndexAction::Stage)
            .unwrap()
            .apply()
            .unwrap();
        assert_eq!(
            fixture.run(&fixture.root, &["ls-files", "--stage"]),
            main_index
        );
        assert_eq!(
            fixture.run(&fixture.root, &["rev-parse", "HEAD"]),
            main_head
        );
        assert_eq!(
            fixture.run(&fixture.other, &["rev-parse", "HEAD"]),
            other_head
        );
        assert!(views.reviews[&secondary]
            .0
            .status()
            .unwrap()
            .changes
            .iter()
            .any(|change| change.group == ChangeGroup::Staged));
        assert_eq!(
            fs::read_to_string(fixture.root.join("same.txt")).unwrap(),
            "Alpha 雪\n"
        );
        assert_eq!(
            fs::read_to_string(fixture.other.join("same.txt")).unwrap(),
            "Beta λ\n"
        );
    }

    #[test]
    fn queued_index_requests_expire_on_close_restore_rebind_and_view_drop() {
        let fixture = Fixture::new();
        let (mut views, other) = fixture.views();
        sync(&mut views, &other, 1, &[]);
        load(&mut views, 21);
        let id = review(&mut views, 21);
        sync(&mut views, &other, 1, &[id.clone()]);
        let closed = views
            .index_request(&id, aw_git::IndexAction::Stage)
            .unwrap();
        sync(&mut views, &other, 1, &[]);
        assert!(matches!(closed.apply(), Err(aw_git::Error::Stale)));
        sync(&mut views, &other, 1, &[id.clone()]);
        let restored = views
            .index_request(&id, aw_git::IndexAction::Stage)
            .unwrap();
        sync(&mut views, &other, 2, &[id.clone()]);
        assert!(matches!(restored.apply(), Err(aw_git::Error::Stale)));
        let rebound = views
            .index_request(&id, aw_git::IndexAction::Stage)
            .unwrap();
        views
            .contexts
            .rebind(&other, fixture.grant(&fixture.other))
            .unwrap();
        assert!(matches!(rebound.apply(), Err(aw_git::Error::Stale)));
        sync(&mut views, &other, 2, &[]);
        load(&mut views, 21);
        let current = review(&mut views, 21);
        sync(&mut views, &other, 2, &[current.clone()]);
        let pending = views
            .index_request(&current, aw_git::IndexAction::Stage)
            .unwrap();
        // Host retains Contexts while the old Views (and its admission owners)
        // are destroyed during a presentation change.
        fn leave(views: Views, request: IndexRequest) -> (Contexts, IndexRequest) {
            (views.contexts, request)
        }
        let (contexts, pending) = leave(views, pending);
        assert!(contexts.access(&other).unwrap().is_valid());
        assert!(matches!(pending.apply(), Err(aw_git::Error::Stale)));
        assert!(fixture
            .grant(&fixture.other)
            .repo
            .status()
            .unwrap()
            .changes
            .iter()
            .all(|change| change.group != ChangeGroup::Staged));
    }

    #[test]
    fn held_real_review_replies_reject_closed_restored_and_rebound_origins() {
        let fixture = Fixture::new();
        let (mut views, other) = fixture.views();
        sync(&mut views, &other, 1, &[]);
        load(&mut views, 21);
        request_review(&mut views, 21);
        let restored = reply(&mut views);
        sync(&mut views, &other, 2, &[]);
        assert!(views.response(restored).is_none());
        assert!(views.reviews.is_empty());
        load(&mut views, 21);
        request_review(&mut views, 21);
        let rebound = reply(&mut views);
        views
            .contexts
            .rebind(&other, fixture.grant(&fixture.root))
            .unwrap();
        sync(&mut views, &other, 2, &[]);
        assert!(views.response(rebound).is_none());
        assert!(views.reviews.is_empty());
        load(&mut views, 21);
        request_review(&mut views, 21);
        let closed = reply(&mut views);
        views.panes.remove(&21);
        views.sync(2, &[], &BTreeSet::new(), &BTreeSet::new());
        sync(&mut views, &other, 2, &[]);
        assert!(views.response(closed).is_none());
        assert!(views.reviews.is_empty());
        assert!(views.contexts.access(&other).is_none());
    }

    #[test]
    fn registry_bounds_prune_and_saved_ids_never_authorize_roots() {
        let fixture = Fixture::new();
        let mut contexts = Contexts::new(fixture.root.clone());
        contexts
            .bind(GitContextId::launch(), fixture.grant(&fixture.root))
            .unwrap();
        let saved = GitContextId("saved-external".into());
        assert!(contexts.access(&saved).is_none());
        contexts
            .rebind(&saved, fixture.grant(&fixture.other))
            .unwrap();
        let old = contexts.access(&saved).unwrap();
        for _ in 2..8 {
            contexts.install(fixture.grant(&fixture.root)).unwrap();
        }
        assert!(contexts.install(fixture.grant(&fixture.root)).is_err());
        contexts.retain(&BTreeSet::new());
        assert!(!old.is_valid());
        assert_eq!(contexts.bindings.len(), 1);
        assert!(!contexts.close(&GitContextId::launch()));
        assert!(contexts
            .rebind(&GitContextId::launch(), fixture.grant(&fixture.other))
            .is_err());
        contexts
            .rebind(&saved, fixture.grant(&fixture.other))
            .unwrap();
        let fresh = contexts.access(&saved).unwrap();
        assert!(!fresh.same_binding(&old));
        contexts.next_epoch = u64::MAX;
        assert!(contexts
            .rebind(&saved, fixture.grant(&fixture.root))
            .is_err());
        assert!(contexts.access(&saved).unwrap().same_binding(&fresh));
        drop(contexts);
        assert!(!fresh.is_valid());
        let (mut views, _) = fixture.views();
        views.sync(1, &[(10, 11, saved)], &BTreeSet::new(), &BTreeSet::new());
        views.ensure(11);
        assert!(views.panes[&11].notice.contains("reconnect"));
        assert!(views.reply_rx.try_recv().is_err());
    }

    #[test]
    fn held_initial_discovery_cannot_overwrite_a_later_explicit_launch_binding() {
        let fixture = Fixture::new();
        let mut views = Views::new(fixture.root.clone(), Executor::new().unwrap());
        let bindings = [(10, 11, GitContextId::launch())];
        views.sync(1, &bindings, &BTreeSet::new(), &BTreeSet::new());
        views.refresh(11).unwrap();
        let held = reply(&mut views);
        assert!(held.result.is_ok());
        views
            .contexts
            .rebind(&GitContextId::launch(), fixture.grant(&fixture.root))
            .unwrap();
        let explicit = views.contexts.access(&GitContextId::launch()).unwrap();
        views.sync(1, &bindings, &BTreeSet::new(), &BTreeSet::new());
        assert!(views.response(held).is_none());
        assert!(views.panes[&11].status.is_none());
        assert!(views
            .contexts
            .access(&GitContextId::launch())
            .unwrap()
            .same_binding(&explicit));
        load(&mut views, 11);
        assert!(views.panes[&11]
            .access
            .as_ref()
            .unwrap()
            .same_binding(&explicit));
    }

    #[cfg(unix)]
    #[test]
    fn worker_access_admission_rejects_revocation_and_checkout_replacement() {
        let fixture = Fixture::new();
        let mut contexts = Contexts::new(fixture.root.clone());
        let context = contexts.install(fixture.grant(&fixture.root)).unwrap();
        let access = contexts.access(&context).unwrap();
        access.validate_binding().unwrap();
        assert!(contexts.close(&context));
        assert!(matches!(
            access.validate_binding(),
            Err(aw_git::Error::Stale)
        ));
        let context = contexts.install(fixture.grant(&fixture.root)).unwrap();
        let access = contexts.access(&context).unwrap();
        fs::rename(&fixture.root, fixture.parent.join("retained-root")).unwrap();
        fs::create_dir(&fixture.root).unwrap();
        fs::write(fixture.root.join("same.txt"), "replacement\n").unwrap();
        assert!(access.is_valid());
        assert!(access.validate_binding().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn initial_launch_discovery_on_executor_rejects_replaced_directory() {
        let fixture = Fixture::new();
        let executor = Executor::new().unwrap();
        let mut views = Views::new(fixture.root.clone(), executor.clone());
        views.sync(
            1,
            &[(10, 11, GitContextId::launch())],
            &BTreeSet::new(),
            &BTreeSet::new(),
        );
        let workers = std::thread::available_parallelism()
            .map_or(2, usize::from)
            .clamp(2, 4);
        let release = Arc::new(Barrier::new(workers + 1));
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        for _ in 0..workers {
            let release = release.clone();
            let started_tx = started_tx.clone();
            executor
                .try_submit("git", move || {
                    started_tx.send(()).unwrap();
                    release.wait();
                })
                .unwrap();
        }
        for _ in 0..workers {
            started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        views.refresh(11).unwrap();
        fs::rename(&fixture.root, fixture.parent.join("original")).unwrap();
        fs::create_dir(&fixture.root).unwrap();
        fs::write(fixture.root.join("same.txt"), "replacement\n").unwrap();
        release.wait();
        let result = reply(&mut views);
        assert!(result
            .result
            .as_ref()
            .err()
            .unwrap()
            .contains("launch directory changed"));
        assert!(views.response(result).is_none());
        assert!(views.contexts.access(&GitContextId::launch()).is_none());
        assert!(views.panes[&11].repo.is_none());
    }
}
