//! Human Git navigation: bounded worker, explicit scope, no agent context injection.
use aw_git::{Branch, Change, ChangeGroup, Repository, Review, ReviewComment, Status, Worktree};
use aw_workspace::Id;
use super::background::Executor;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
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
#[derive(Clone)]
enum Job {
    Refresh,
    Review(Change),
    Select(Worktree),
    Compare(Branch),
}
struct Request {
    ticket: u64,
    pane: Id,
    git: PathBuf,
    root: PathBuf,
    repo: Option<Repository>,
    mode: Mode,
    job: Job,
}
pub struct Reply {
    ticket: u64,
    pane: Id,
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
        repo: Repository,
        review: Review,
    },
}
fn execute(request: &Request) -> aw_git::Result<Data> {
    let repo = match &request.repo {
        Some(r) => r.clone(),
        None => Repository::discover(&request.git, &request.root)?,
    };
    match &request.job {
        Job::Review(change) => {
            let review = repo.review(change)?;
            Ok(Data::Review { repo, review })
        }
        Job::Compare(branch) => {
            let review = repo.compare(branch)?;
            Ok(Data::Review { repo, review })
        }
        Job::Select(worktree) => {
            let repo = repo.select_worktree(&worktree.path)?;
            let status = repo.status()?;
            Ok(Data::Inventory {
                repo,
                status: Some(status),
                worktrees: vec![],
                branches: vec![],
            })
        }
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
    }
}
pub struct Views {
    root: PathBuf,
    git: PathBuf,
    background: Executor,
    reply_tx: tokio::sync::mpsc::Sender<Reply>,
    reply_rx: tokio::sync::mpsc::Receiver<Reply>,
    pub panes: BTreeMap<Id, Nav>,
    pub reviews: BTreeMap<String, (Repository, Review)>,
    pub comments: BTreeMap<String, Vec<ReviewComment>>,
    pub reviewed: BTreeSet<String>,
    ticket: u64,
    pub selected_root: Option<PathBuf>,
    pub revision: u64,
}
impl Views {
    pub fn new(root: PathBuf, background: Executor) -> Self {
        let git = std::env::var_os("AW_GIT")
            .map(PathBuf::from)
            .or_else(|| option_env!("AW_DEFAULT_GIT").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/usr/bin/git"));
        let (reply_tx, reply_rx) = tokio::sync::mpsc::channel(32);
        Self {
            root,
            git,
            background,
            reply_tx,
            reply_rx,
            panes: BTreeMap::new(),
            reviews: BTreeMap::new(),
            comments: BTreeMap::new(),
            reviewed: BTreeSet::new(),
            ticket: 0,
            selected_root: None,
            revision: 0,
        }
    }
    fn request(&mut self, pane: Id, job: Job) -> std::result::Result<(), String> {
        if !self.git.is_absolute() {
            return Err("AW_GIT must be an explicit absolute trusted Git path".into());
        }
        self.ticket = self
            .ticket
            .checked_add(1)
            .ok_or("Git request counter exhausted")?;
        let nav = self.panes.entry(pane).or_default();
        let request = Request {
            ticket: self.ticket,
            pane,
            git: self.git.clone(),
            root: self.root.clone(),
            repo: nav.repo.clone(),
            mode: nav.mode,
            job,
        };
        let out = self.reply_tx.clone();
        let panic_out = out.clone();
        let ticket = request.ticket;
        let reply_pane = request.pane;
        self.background
            .try_submit_with_panic("git", move || {
                let result = execute(&request).map_err(|e| e.to_string());
                let _ = out.blocking_send(Reply {
                    ticket,
                    pane: reply_pane,
                    result,
                });
            }, move || {
                let _ = panic_out.blocking_send(Reply {
                    ticket,
                    pane: reply_pane,
                    result: Err("Git background job panicked; retry explicitly".into()),
                });
            })
            .map_err(|e| e.to_string())?;
        nav.pending = Some(self.ticket);
        nav.notice.clear();
        Ok(())
    }
    pub fn refresh(&mut self, pane: Id) -> std::result::Result<(), String> {
        self.request(pane, Job::Refresh)
    }
    pub fn ensure(&mut self, pane: Id) {
        let nav = self.panes.entry(pane).or_default();
        if !nav.loaded && nav.pending.is_none() {
            if let Err(e) = self.refresh(pane) {
                let n = self.panes.entry(pane).or_default();
                n.loaded = true;
                n.notice = e;
            }
        }
    }
    pub fn set_mode(&mut self, pane: Id, mode: Mode) -> std::result::Result<(), String> {
        let n = self.panes.entry(pane).or_default();
        n.select_mode(mode);
        n.loaded = false;
        self.refresh(pane)
    }
    pub fn open(&mut self, pane: Id) -> std::result::Result<(), String> {
        let n = self.panes.entry(pane).or_default();
        if n.pending.is_some() {
            return Err("Git request still pending".into());
        }
        let item = n.rows().get(n.selected).map(|r| r.item.clone());
        match item {
            Some(Item::Change(c)) => self.request(pane, Job::Review(c)),
            Some(Item::Worktree(w)) => self.request(pane, Job::Select(w)),
            Some(Item::Branch(b)) => self.request(pane, Job::Compare(b)),
            Some(Item::Folder(key)) | Some(Item::Heading(key)) => {
                if !n.collapsed.remove(&key) {
                    n.collapsed.insert(key);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    pub async fn next(&mut self) -> Option<Reply> {
        self.reply_rx.recv().await
    }
    pub fn response(&mut self, reply: Reply) -> Option<String> {
        let n = self.panes.get_mut(&reply.pane)?;
        if n.pending != Some(reply.ticket) {
            return None;
        }
        let selected = n.rows().get(n.selected).map(|r| r.item.clone());
        n.pending = None;
        n.loaded = true;
        self.revision = self.revision.wrapping_add(1);
        match reply.result {
            Err(e) => {
                n.notice = e;
                None
            }
            Ok(Data::Inventory {
                repo,
                status,
                worktrees,
                branches,
            }) => {
                if status.is_some() {
                    n.select_mode(Mode::Changes);
                    n.status = status;
                }
                n.repo = Some(repo.clone());
                n.worktrees = worktrees;
                n.branches = branches;
                if let Some(item) = selected {
                    n.selected = n.rows().iter().position(|r| r.item == item).unwrap_or(0);
                }
                n.notice =
                    "Enter opens a diff / chooses existing worktree / compares a branch".into();
                self.selected_root = Some(repo.root().to_owned());
                None
            }
            Ok(Data::Review { repo, review }) => {
                let id = review.id.clone();
                while self.reviews.len() >= 8 && !self.reviews.contains_key(&id) {
                    if let Some(old) = self.reviews.keys().next().cloned() {
                        self.reviews.remove(&old);
                        self.comments.remove(&old);
                        self.reviewed.remove(&old);
                    } else {
                        break;
                    }
                }
                self.reviews.insert(id.clone(), (repo, review));
                Some(id)
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
