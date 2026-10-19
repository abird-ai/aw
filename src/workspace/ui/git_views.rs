//! Git view origins bind human navigation to exact repositories, never Agent roots.
use super::*;

/// Captured on the UI thread, admitted on the executor immediately before spawn.
pub(super) struct TerminalRoot {
    pub path: PathBuf,
    access: Option<git::Access>,
}
impl TerminalRoot {
    pub(super) fn launch(
        self,
        pool: terminals::Pool,
        shell: PathBuf,
        owner: String,
    ) -> std::result::Result<String, String> {
        if let Some(access) = self.access {
            access.validate_binding().map_err(|error| format!("Git repository changed or unavailable; reconnect before starting a terminal: {error}"))?;
        }
        pool.launch(self.path, shell, owner)
            .map_err(|error| error.to_string())
    }
}

impl Ui {
    pub(super) fn owned_navigator(&self) -> Option<Id> {
        let container = self
            .layout
            .container_for(self.layout.focused)
            .and_then(|id| self.layout.container(id))?;
        let target = self
            .layout
            .active(self.layout.focused)
            .map(|tab| &tab.target);
        let preferred = match target {
            Some(Target::File { scope_id, .. }) => container
                .navigators
                .iter()
                .find(|pane| self.layout.files_scope_for(**pane) == Some(scope_id)),
            Some(Target::Diff { review }) => self.git.review_access(review).and_then(|access| {
                container
                    .navigators
                    .iter()
                    .find(|pane| self.layout.git_context_for(**pane) == Some(access.context_id()))
            }),
            _ => None,
        };
        preferred.or_else(|| container.navigators.first()).copied()
    }

    fn git_origin(&self, pane: Id) -> git::Origin {
        git::Origin {
            container: self.layout.container_for(pane),
            pane,
            lifetime: self.git_epoch,
        }
    }

    fn git_origin_current(&self, origin: git::Origin) -> bool {
        self.git_epoch != u64::MAX
            && origin == self.git_origin(origin.pane)
            && self.layout.panes.contains_key(&origin.pane)
    }

    pub(super) fn git_context_label(&self, context: &GitContextId) -> String {
        self.git
            .contexts
            .label(context)
            .map(|path| {
                let root = aw_workspace::editor::display(&path.display().to_string());
                path.file_name()
                    .map(|name| {
                        format!(
                            "{} · {root}",
                            aw_workspace::editor::display(&name.to_string_lossy())
                        )
                    })
                    .unwrap_or(root)
            })
            .unwrap_or_else(|| {
                self.layout
                    .git_labels
                    .get(context)
                    .map(|label| format!("{} [unavailable]", aw_workspace::editor::display(label)))
                    .unwrap_or_else(|| "unavailable repository".into())
            })
    }

    pub(super) fn reconcile_git_views(&mut self) {
        let bindings = self
            .layout
            .panes
            .keys()
            .copied()
            .filter_map(|pane| {
                Some((
                    self.layout.container_for(pane)?,
                    pane,
                    self.layout.git_context_for(pane)?.clone(),
                ))
            })
            .collect::<Vec<_>>();
        if bindings != self.git_bindings {
            self.git_epoch = self.git_epoch.saturating_add(1);
            self.git_bindings = bindings;
        }
        let reviews = self
            .layout
            .tabs
            .values()
            .filter_map(|tab| match &tab.target {
                Target::Diff { review } => Some(review.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let retained = self
            .layout
            .containers
            .values()
            .flat_map(|container| container.git_contexts.values().cloned())
            .collect::<BTreeSet<_>>();
        if self.git_epoch == u64::MAX {
            self.git
                .sync(self.git_epoch, &[], &BTreeSet::new(), &retained);
            self.notice = "Git request lifetime exhausted; restart the workspace.".into();
        } else {
            self.git
                .sync(self.git_epoch, &self.git_bindings, &reviews, &retained);
        }
        let bindings = self
            .layout
            .tabs
            .iter()
            .filter_map(|(id, tab)| match &tab.target {
                Target::Diff { review } => Some((
                    *id,
                    (
                        review.clone(),
                        self.layout
                            .group_for(*id)
                            .and_then(|group| self.layout.container_for(group)),
                    ),
                )),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        let changed = bindings
            .iter()
            .filter_map(|(tab, binding)| {
                let old = self.git_tab_bindings.get(tab);
                let resource_changed = old.is_some_and(|old| old.0 != binding.0)
                    || self.git.review_access(&binding.0).is_none();
                let owner_changed = old.is_some_and(|old| old.1 != binding.1);
                (resource_changed || owner_changed).then_some((
                    *tab,
                    binding.0.clone(),
                    resource_changed,
                ))
            })
            .collect::<Vec<_>>();
        for (tab, review, resource_changed) in changed {
            if resource_changed {
                self.discard_git_derived(tab);
            } else if self.find_cancel.contains_key(&tab) {
                if let Some(find) = self.find.get(&tab).cloned() {
                    self.schedule_find(tab, Target::Diff { review }, find.query, find.committed);
                }
            }
        }
        self.git_tab_bindings = bindings;
        self.diff_highlights
            .retain(|id, _| self.git.review_access(id).is_some());
        for context in retained {
            if self.git.contexts.access(&context).is_none() {
                continue;
            }
            let mut label = self.git_context_label(&context);
            if label.len() > aw_workspace::MAX_GIT_LABEL_BYTES {
                let mut end = aw_workspace::MAX_GIT_LABEL_BYTES - '…'.len_utf8();
                while !label.is_char_boundary(end) {
                    end -= 1;
                }
                label.truncate(end);
                label.push('…');
            }
            if self.layout.git_labels.get(&context) != Some(&label) {
                match self.layout.apply(Action::SetGitLabel {
                    context_id: context,
                    label,
                }) {
                    Ok(()) => self.layout_generation = self.layout_generation.wrapping_add(1),
                    Err(error) => self.notice = error.to_string(),
                }
            }
        }
    }

    fn discard_git_derived(&mut self, tab: Id) {
        if let Some(cancel) = self.find_cancel.remove(&tab) {
            cancel.store(true, Ordering::Release);
        }
        self.find_generation.remove(&tab);
        if let Some(find) = self.find.get_mut(&tab) {
            find.matches.clear();
            find.selected = 0;
        }
        self.saved_selections.remove(&tab);
        self.selectable_views.remove(&tab);
        self.rendered.remove(&tab);
        if self
            .text_selection
            .as_ref()
            .is_some_and(|selection| selection.tab == tab)
        {
            self.text_selection = None;
            self.selection_dragging = false;
            self.selection_moved = false;
        }
    }

    pub(super) fn reset_git_lifetime(&mut self) {
        self.git_epoch = self.git_epoch.saturating_add(1);
        self.diff_highlights.clear();
        let tabs = self
            .layout
            .tabs
            .values()
            .filter_map(|tab| matches!(tab.target, Target::Diff { .. }).then_some(tab.id))
            .collect::<Vec<_>>();
        for tab in tabs {
            self.discard_git_derived(tab);
        }
    }

    pub(super) fn terminal_git_root(&self) -> std::result::Result<TerminalRoot, String> {
        if let Some(context) = self.layout.git_context_for(self.layout.focused) {
            let access = self.git.contexts.access(context).ok_or_else(|| {
                "Git repository unavailable. Reconnect it before starting a terminal.".to_string()
            })?;
            let path = self
                .git
                .contexts
                .label(context)
                .ok_or_else(|| {
                    "Git repository unavailable. Reconnect it before starting a terminal."
                        .to_string()
                })?
                .to_owned();
            return Ok(TerminalRoot {
                path,
                access: Some(access),
            });
        }
        if let Some(Target::Diff { review }) = self
            .layout
            .active(self.layout.focused)
            .map(|tab| &tab.target)
        {
            let access = self.git.review_access(review).ok_or_else(|| {
                "Git review unavailable. Reopen it before starting a terminal.".to_string()
            })?;
            let path = self
                .git
                .reviews
                .get(review)
                .ok_or_else(|| {
                    "Git review unavailable. Reopen it before starting a terminal.".to_string()
                })?
                .0
                .root()
                .to_owned();
            return Ok(TerminalRoot {
                path,
                access: Some(access),
            });
        }
        Ok(TerminalRoot {
            path: self.root.clone(),
            access: None,
        })
    }

    pub(super) fn authorize_git(&mut self, path: &str, reconnect: bool) -> bool {
        if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
            self.notice = if reconnect {
                "git-reconnect REPOSITORY_PATH"
            } else {
                "git-open REPOSITORY_PATH"
            }
            .into();
            return false;
        }
        if self.git_epoch == u64::MAX {
            self.notice = "Git request lifetime exhausted.".into();
            return false;
        }
        let origin = self.git_origin(self.layout.focused);
        let context = if reconnect {
            let Some(context) = self.layout.git_context_for(origin.pane).cloned() else {
                self.notice = "Focus a Git navigator before reconnecting its repository.".into();
                return false;
            };
            Some(context)
        } else {
            None
        };
        let path = PathBuf::from(path);
        let path = if path.is_absolute() {
            path
        } else {
            self.root.join(path)
        };
        match self.git.open_root(origin, path, context) {
            Ok(()) => {
                self.notice = "Opening Git repository…".into();
                true
            }
            Err(error) => {
                self.notice = error;
                false
            }
        }
    }

    pub(super) fn git_response(&mut self, reply: git::Reply) {
        let Some(event) = self.git.response(reply) else {
            // Lazy launch discovery updates inventory without opening a document.
            self.reconcile_git_views();
            return;
        };
        match event {
            git::Event::Opened { origin, result } => {
                if !self.git_origin_current(origin) {
                    return;
                }
                let grant = match result {
                    Ok(grant) => grant,
                    Err(error) => {
                        self.notice = error;
                        return;
                    }
                };
                let context = match self.git.contexts.install(grant) {
                    Ok(context) => context,
                    Err(error) => {
                        self.notice = error;
                        return;
                    }
                };
                let focused = self.layout.focused;
                let input = self.input;
                match self.layout.apply(Action::AddGit {
                    beside: origin.pane,
                    side: Side::Right,
                    context_id: context.clone(),
                }) {
                    Ok(()) => {
                        self.layout_generation = self.layout_generation.wrapping_add(1);
                        self.reconcile_layout_state();
                        if focused != origin.pane && self.layout.panes.contains_key(&focused) {
                            self.apply(Action::Focus(focused));
                            self.input = input;
                        } else {
                            self.input = false;
                        }
                        self.notice =
                            "Git repository opened; Agent and Files folders unchanged.".into();
                    }
                    Err(error) => {
                        self.git.contexts.close(&context);
                        self.notice = error.to_string();
                    }
                }
            }
            git::Event::Reconnected {
                origin,
                context_id,
                result,
            } => {
                if !self.git_origin_current(origin)
                    || self.layout.git_context_for(origin.pane) != Some(&context_id)
                {
                    return;
                }
                match result.and_then(|grant| self.git.contexts.rebind(&context_id, grant)) {
                    Ok(()) => {
                        self.reset_git_lifetime();
                        self.reconcile_layout_state();
                        self.notice =
                            "Git repository reconnected; Agent and Files folders unchanged.".into();
                    }
                    Err(error) => self.notice = error,
                }
            }
            git::Event::Selected { origin, result } => {
                if !self.git_origin_current(origin)
                    || self.layout.git_context_for(origin.pane).is_none()
                {
                    return;
                }
                let grant = match result {
                    Ok(grant) => grant,
                    Err(error) => {
                        self.notice = error;
                        return;
                    }
                };
                let context = match self.git.contexts.install(grant) {
                    Ok(context) => context,
                    Err(error) => {
                        self.notice = error;
                        return;
                    }
                };
                match self.layout.apply(Action::SetGitContext {
                    navigator: origin.pane,
                    context_id: context.clone(),
                }) {
                    Ok(()) => {
                        self.layout_generation = self.layout_generation.wrapping_add(1);
                        self.reconcile_layout_state();
                        self.notice = "Git worktree selected; existing diffs, Agent and Files folders unchanged.".into();
                    }
                    Err(error) => {
                        self.git.contexts.close(&context);
                        self.notice = error.to_string();
                    }
                }
            }
            git::Event::ReviewReady {
                origin,
                context_id,
                access,
                review,
            } => {
                if !self.git_origin_current(origin)
                    || self.layout.git_context_for(origin.pane) != Some(&context_id)
                    || !self
                        .git
                        .contexts
                        .access(&context_id)
                        .is_some_and(|current| current.same_binding(&access))
                {
                    return;
                }
                let Some(container) = origin
                    .container
                    .and_then(|id| self.layout.container(id).map(|container| (id, container)))
                else {
                    return;
                };
                let group = self
                    .container_last_group
                    .get(&container.0)
                    .filter(|group| container.1.groups.contains(group))
                    .copied()
                    .or_else(|| container.1.groups.first().copied());
                let Some(group) = group else {
                    return;
                };
                let focused = self.layout.focused;
                let input = self.input;
                self.apply(Action::Open {
                    group,
                    target: Target::Diff {
                        review: review.clone(),
                    },
                    pinned: false,
                });
                self.apply(Action::Focus(focused));
                self.input = input;
                self.schedule_diff_highlight(&review);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use std::process::Command as Process;

    struct Repo {
        base: PathBuf,
        a: PathBuf,
        b: PathBuf,
        git: PathBuf,
    }
    impl Repo {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!(
                "aw-native-git-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&base).unwrap();
            let repo = Self {
                a: base.join("checkout alpha雪"),
                b: base.join("checkout betaλ"),
                base,
                git: std::env::var_os("AW_GIT")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| "/usr/bin/git".into()),
            };
            std::fs::create_dir(&repo.a).unwrap();
            repo.run(&repo.a, &["init", "-b", "main"]);
            std::fs::create_dir(repo.a.join("src")).unwrap();
            std::fs::write(
                repo.a.join("src/lib.rs"),
                (0..120)
                    .map(|row| format!("// BASE {row}\n"))
                    .collect::<String>(),
            )
            .unwrap();
            repo.run(&repo.a, &["add", "src/lib.rs"]);
            repo.run(&repo.a, &["commit", "-m", "fixture"]);
            repo.run(
                &repo.a,
                &["worktree", "add", "-b", "other", repo.b.to_str().unwrap()],
            );
            for (root, prefix) in [(&repo.a, "ALPHA雪"), (&repo.b, "BETAλ")] {
                std::fs::write(
                    root.join("src/lib.rs"),
                    (0..120)
                        .map(|row| format!("// {prefix} {row}\n"))
                        .collect::<String>(),
                )
                .unwrap();
            }
            repo
        }
        fn run(&self, root: &std::path::Path, args: &[&str]) -> String {
            let output = Process::new(&self.git)
                .current_dir(root)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .env_remove("GIT_COMMON_DIR")
                .args([
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "core.hooksPath=/dev/null",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "Git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap()
        }
    }
    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    fn fixture(repo: &Repo) -> Ui {
        let mut snapshot = super::super::render_tests::projection();
        snapshot.root = repo.a.display().to_string();
        snapshot.agent_root = Some(repo.a.clone());
        let mut ui = Ui::new(
            Workspace::default(),
            "draft 界 keep caret".into(),
            None,
            repo.a.clone(),
            snapshot,
            aw::config::UiConfig::default(),
        )
        .unwrap();
        ui.draft.left();
        ui.set_navigator(Navigator::Git);
        settle(&mut ui);
        ui
    }
    fn next(ui: &mut Ui) -> git::Reply {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(8), ui.git.next())
                    .await
                    .unwrap()
                    .unwrap()
            })
    }
    fn settle(ui: &mut Ui) {
        while ui.git.panes.values().any(|nav| nav.pending.is_some()) {
            let reply = next(ui);
            ui.git_response(reply);
        }
    }
    fn open_other(ui: &mut Ui, repo: &Repo) -> Id {
        assert!(ui.authorize_git(repo.b.to_str().unwrap(), false));
        let reply = next(ui);
        ui.git_response(reply);
        let pane = ui.layout.focused;
        ui.git.ensure(pane);
        settle(ui);
        pane
    }
    fn request_review(ui: &mut Ui, pane: Id) {
        if !ui.git.panes[&pane].loaded {
            ui.git.ensure(pane);
            while ui.git.panes[&pane].pending.is_some() {
                let reply = next(ui);
                ui.git_response(reply);
            }
        }
        let nav = ui.git.panes.get_mut(&pane).unwrap();
        nav.selected = nav
            .rows()
            .iter()
            .position(|row| matches!(row.item, git::Item::Change(_)))
            .unwrap();
        ui.git.open(pane).unwrap();
    }
    fn review_tab(ui: &Ui, group: Id) -> (Id, String) {
        let tab = ui.layout.active(group).unwrap();
        let Target::Diff { review } = &tab.target else {
            panic!("expected Diff")
        };
        (tab.id, review.clone())
    }
    fn draw(ui: &mut Ui) -> String {
        let mut terminal = Terminal::new(TestBackend::new(240, 48)).unwrap();
        terminal.draw(|frame| ui.draw(frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn two_git_containers_route_delayed_reviews_to_their_own_recent_groups() {
        let repo = Repo::new();
        let mut ui = fixture(&repo);
        let a = ui.layout.focused;
        let a_container = ui.layout.container_for(a).unwrap();
        let a1 = ui.group();
        ui.apply(Action::SplitGroup {
            pane: a1,
            side: Side::Below,
        });
        let a2 = ui.layout.focused;
        assert_ne!(a1, a2);
        let b = open_other(&mut ui, &repo);
        let b_group = ui.group();
        let b_context = ui.layout.git_context_for(b).unwrap().clone();
        let draft = (ui.draft.text().to_owned(), ui.draft.cursor());
        let agent = (ui.snapshot.actor.clone(), ui.snapshot.agent_root.clone());
        let files_scope = ui.current_file_scope();
        ui.apply(Action::Focus(a));
        request_review(&mut ui, a);
        let a_reply = next(&mut ui);
        ui.apply(Action::Focus(b));
        request_review(&mut ui, b);
        let b_reply = next(&mut ui);
        ui.apply(Action::Focus(a));
        ui.git_response(b_reply);
        ui.git_response(a_reply);
        assert_eq!(ui.layout.focused, a, "preview preserves Explorer focus");
        assert_eq!(ui.container_last_group.get(&a_container), Some(&a2));
        let (a_tab, a_review) = review_tab(&ui, a2);
        let (b_tab, b_review) = review_tab(&ui, b_group);
        assert_ne!(a_review, b_review);
        assert_eq!(ui.git.reviews[&a_review].0.root(), repo.a);
        assert_eq!(ui.git.reviews[&b_review].0.root(), repo.b);
        let mut a_viewport = Viewport::default();
        a_viewport.configure(260, 20, false);
        a_viewport.jump_to(31, 0);
        ui.scroll.insert(a_tab, a_viewport);
        let mut b_viewport = Viewport::default();
        b_viewport.configure(260, 20, false);
        b_viewport.jump_to(7, 0);
        ui.scroll.insert(b_tab, b_viewport);
        ui.schedule_find(
            a_tab,
            Target::Diff {
                review: a_review.clone(),
            },
            "ALPHA雪".into(),
            false,
        );
        ui.schedule_find(
            b_tab,
            Target::Diff {
                review: b_review.clone(),
            },
            "BETAλ".into(),
            false,
        );
        let deadline = Instant::now() + Duration::from_secs(8);
        while ui.find_cancel.contains_key(&a_tab) || ui.find_cancel.contains_key(&b_tab) {
            while let Ok(result) = ui.background_rx.try_recv() {
                ui.apply_background_result(result);
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(ui.find[&a_tab].matches.len(), 120);
        assert_eq!(ui.find[&b_tab].matches.len(), 120);
        ui.git.panes.get_mut(&a).unwrap().filter = "lib".into();
        ui.git.panes.get_mut(&b).unwrap().filter = "src".into();
        let text = draw(&mut ui);
        assert!(text.contains("checkout alpha雪") && text.contains("checkout betaλ"));
        assert!(text.contains("ALPHA雪") && text.contains("BETAλ"));
        assert_eq!(ui.find[&a_tab].query, "ALPHA雪");
        assert_eq!(ui.find[&b_tab].query, "BETAλ");
        assert_eq!(ui.layout.git_context_for(b), Some(&b_context));
        assert_eq!((ui.draft.text().to_owned(), ui.draft.cursor()), draft);
        assert_eq!(
            (ui.snapshot.actor.clone(), ui.snapshot.agent_root.clone()),
            agent
        );
        assert_eq!(ui.current_file_scope(), files_scope);
        ui.layout.validate().unwrap();
    }

    #[test]
    fn explicit_open_preserves_new_focus_and_rejects_closed_or_restored_origins() {
        let repo = Repo::new();
        let mut ui = fixture(&repo);
        let a = ui.layout.focused;
        let group = ui.group();
        assert!(ui.authorize_git(repo.b.to_str().unwrap(), false));
        let reply = next(&mut ui);
        ui.apply(Action::Focus(group));
        ui.input = true;
        let draft = (ui.draft.text().to_owned(), ui.draft.cursor());
        ui.git_response(reply);
        assert_eq!(ui.layout.focused, group);
        assert!(ui.input);
        assert_eq!((ui.draft.text().to_owned(), ui.draft.cursor()), draft);
        assert_eq!(ui.layout.containers.len(), 2);
        ui.apply(Action::Focus(a));
        assert!(ui.authorize_git(repo.b.to_str().unwrap(), false));
        let reply = next(&mut ui);
        ui.apply(Action::ClosePane(a));
        let before = ui.layout.clone();
        ui.git_response(reply);
        assert_eq!(ui.layout, before);
        assert!(ui.authorize_git(repo.a.to_str().unwrap(), false));
        let reply = next(&mut ui);
        ui.reset_git_lifetime();
        ui.layout = before.clone();
        ui.reconcile_layout_state();
        ui.git_response(reply);
        assert_eq!(ui.layout, before);
    }

    #[test]
    fn worktree_selection_keeps_old_diff_exact_and_stages_only_its_index() {
        let repo = Repo::new();
        let mut ui = fixture(&repo);
        let a = ui.layout.focused;
        let group = ui.group();
        request_review(&mut ui, a);
        let reply = next(&mut ui);
        ui.git_response(reply);
        let (old_tab, old_review) = review_tab(&ui, group);
        ui.apply(Action::Pin {
            tab: old_tab,
            pinned: true,
        });
        ui.git.set_mode(a, git::Mode::Worktrees).unwrap();
        settle(&mut ui);
        let nav = ui.git.panes.get_mut(&a).unwrap();
        nav.selected = nav
            .rows()
            .iter()
            .position(
                |row| matches!(&row.item, git::Item::Worktree(worktree) if worktree.path == repo.b),
            )
            .unwrap();
        ui.git.open(a).unwrap();
        let reply = next(&mut ui);
        ui.git_response(reply);
        ui.git.set_mode(a, git::Mode::Changes).unwrap();
        settle(&mut ui);
        assert_eq!(ui.terminal_git_root().unwrap().path, repo.b);
        request_review(&mut ui, a);
        let reply = next(&mut ui);
        ui.git_response(reply);
        let (_, new_review) = review_tab(&ui, group);
        assert_ne!(old_review, new_review);
        assert_eq!(ui.git.reviews[&old_review].0.root(), repo.a);
        assert_eq!(ui.git.reviews[&new_review].0.root(), repo.b);
        let head_a = repo.run(&repo.a, &["rev-parse", "HEAD"]);
        let head_b = repo.run(&repo.b, &["rev-parse", "HEAD"]);
        let file_a = std::fs::read(repo.a.join("src/lib.rs")).unwrap();
        let file_b = std::fs::read(repo.b.join("src/lib.rs")).unwrap();
        ui.git
            .index_request(&old_review, aw_git::IndexAction::Stage)
            .unwrap()
            .apply()
            .unwrap();
        assert!(repo
            .run(&repo.a, &["diff", "--cached", "--", "src/lib.rs"])
            .contains("ALPHA雪"));
        assert!(repo.run(&repo.b, &["diff", "--cached"]).is_empty());
        ui.git
            .index_request(&new_review, aw_git::IndexAction::Stage)
            .unwrap()
            .apply()
            .unwrap();
        assert!(repo
            .run(&repo.b, &["diff", "--cached", "--", "src/lib.rs"])
            .contains("BETAλ"));
        assert_eq!(repo.run(&repo.a, &["rev-parse", "HEAD"]), head_a);
        assert_eq!(repo.run(&repo.b, &["rev-parse", "HEAD"]), head_b);
        assert_eq!(std::fs::read(repo.a.join("src/lib.rs")).unwrap(), file_a);
        assert_eq!(std::fs::read(repo.b.join("src/lib.rs")).unwrap(), file_b);
        ui.apply(Action::Focus(group));
        ui.apply(Action::Select {
            group,
            tab: old_tab,
        });
        assert_eq!(ui.terminal_git_root().unwrap().path, repo.a);
    }

    #[test]
    fn reconnect_and_restore_reject_held_reviews_and_queued_index_requests() {
        let repo = Repo::new();
        let mut ui = fixture(&repo);
        let a = ui.layout.focused;
        let group = ui.group();
        request_review(&mut ui, a);
        let old_reply = next(&mut ui);
        assert!(ui.authorize_git(repo.a.to_str().unwrap(), true));
        let grant = next(&mut ui);
        ui.git_response(grant);
        ui.git_response(old_reply);
        assert!(ui
            .layout
            .active(group)
            .is_none_or(|tab| !matches!(tab.target, Target::Diff { .. })));
        ui.git.ensure(a);
        settle(&mut ui);
        request_review(&mut ui, a);
        let reply = next(&mut ui);
        ui.git_response(reply);
        let (tab, review) = review_tab(&ui, group);
        let queued = ui
            .git
            .index_request(&review, aw_git::IndexAction::Stage)
            .unwrap();
        ui.find.insert(
            tab,
            FindState {
                query: "ALPHA雪".into(),
                matches: vec![4],
                ..Default::default()
            },
        );
        let layout = ui.layout.clone();
        ui.reset_git_lifetime();
        ui.layout = layout;
        ui.reconcile_layout_state();
        assert!(queued.apply().is_err());
        assert!(ui.find[&tab].matches.is_empty());
        assert!(repo.run(&repo.a, &["diff", "--cached"]).is_empty());
        ui.git.ensure(a);
        settle(&mut ui);
        request_review(&mut ui, a);
        let held = next(&mut ui);
        ui.apply(Action::ClosePane(a));
        ui.git_response(held);
        assert_eq!(review_tab(&ui, group).1, review);
    }

    #[test]
    fn moving_immutable_diff_preserves_completed_find_selection_and_viewport() {
        let repo = Repo::new();
        let mut ui = fixture(&repo);
        let a = ui.layout.focused;
        let a_group = ui.group();
        let b = open_other(&mut ui, &repo);
        let b_group = ui.group();
        ui.apply(Action::Focus(a));
        request_review(&mut ui, a);
        let reply = next(&mut ui);
        ui.git_response(reply);
        let (tab, review) = review_tab(&ui, a_group);
        ui.apply(Action::Pin { tab, pinned: true });
        let target = Target::Diff {
            review: review.clone(),
        };
        ui.schedule_find(tab, target.clone(), "ALPHA雪".into(), false);
        let deadline = Instant::now() + Duration::from_secs(8);
        while ui.find_cancel.contains_key(&tab) {
            while let Ok(result) = ui.background_rx.try_recv() {
                ui.apply_background_result(result);
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        ui.find.get_mut(&tab).unwrap().selected = 12;
        let find = ui.find[&tab].clone();
        let mut viewport = Viewport::default();
        viewport.configure(260, 20, false);
        viewport.jump_to(160, 0);
        ui.scroll.insert(tab, viewport);
        ui.horizontal.insert(tab, 3);
        let selection = TextSelection {
            tab,
            anchor: TextPoint { line: 150, col: 2 },
            current: TextPoint { line: 151, col: 8 },
        };
        ui.saved_selections.insert(tab, selection.clone());
        ui.apply(Action::MoveTab {
            tab,
            group: b_group,
        });
        assert_eq!(ui.find[&tab].matches, find.matches);
        assert_eq!(ui.find[&tab].selected, 12);
        assert_eq!(ui.find[&tab].query, "ALPHA雪");
        assert_eq!(ui.scroll[&tab], viewport);
        assert_eq!(ui.horizontal[&tab], 3);
        assert_eq!(ui.saved_selections[&tab].anchor, selection.anchor);
        assert_eq!(ui.saved_selections[&tab].current, selection.current);
        assert_eq!(ui.git.reviews[&review].0.root(), repo.a);
        assert!(ui
            .git
            .index_request(&review, aw_git::IndexAction::Stage)
            .is_ok());
        assert!(draw(&mut ui).contains("ALPHA雪"));
        ui.schedule_find(tab, target, "ALPHA雪 11".into(), false);
        let held = loop {
            if let Ok(result) = ui.background_rx.try_recv() {
                if matches!(result, UiBackgroundResult::Find { .. }) {
                    break result;
                }
                ui.apply_background_result(result);
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        ui.apply(Action::MoveTab {
            tab,
            group: a_group,
        });
        ui.apply_background_result(held);
        assert!(
            ui.find[&tab].matches.is_empty(),
            "held old-owner Find cannot attach"
        );
        while ui.find_cancel.contains_key(&tab) {
            while let Ok(result) = ui.background_rx.try_recv() {
                ui.apply_background_result(result);
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(ui.find[&tab].matches.len(), 11);
        ui.apply(Action::MoveTab {
            tab,
            group: b_group,
        });
        ui.apply(Action::ClosePane(a));
        assert_eq!(ui.layout.focused, b_group);
        assert_eq!(ui.layout.group_for(tab), Some(b_group));
        assert!(ui
            .git
            .index_request(&review, aw_git::IndexAction::Stage)
            .is_ok());
        assert!(ui.layout.git_context_for(b).is_some());
    }

    #[test]
    fn file_in_git_only_group_keeps_its_files_scope_and_derived_view_state() {
        let repo = Repo::new();
        let mut ui = fixture(&repo);
        let b = open_other(&mut ui, &repo);
        let group = ui.group();
        let container = ui.layout.container_for(group).unwrap();
        assert!(ui
            .layout
            .container(container)
            .unwrap()
            .files_scope
            .is_none());
        let key = FileKey::new(ScopeId::launch(), "src/lib.rs".into());
        let mut browser = Browser::new(ui.background.clone());
        ui.read_file_key(&browser, key.clone());
        let tab = ui.layout.active(group).unwrap().id;
        let reply = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(8), browser.replies.recv())
                    .await
                    .unwrap()
                    .unwrap()
            });
        ui.response(reply);
        assert!(ui.files[&key].contains("ALPHA雪"));
        assert!(!ui.files[&key].contains("BETAλ"));
        assert_eq!(ui.layout.focused, b);
        assert_eq!(ui.file_bindings[&tab].1, Some(container));
        assert!(ui.schedule_find(tab, key.target(), "// ALPHA雪 ".into(), false));
        let deadline = Instant::now() + Duration::from_secs(8);
        while ui.find_cancel.contains_key(&tab) || !ui.file_highlights[&key].complete {
            while let Ok(result) = ui.background_rx.try_recv() {
                ui.apply_background_result(result);
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(ui.find[&tab].matches.len(), 120);
        assert!(ui.file_highlights[&key].lines.iter().any(Option::is_some));
        assert!(draw(&mut ui).contains("ALPHA雪"));
        // Reconnecting the independent Files scope cannot cancel a Git read.
        request_review(&mut ui, b);
        let held_git = next(&mut ui);
        let epoch = ui.git_epoch;
        ui.apply(Action::Focus(group));
        assert!(ui.authorize_files(&browser, repo.a.to_str().unwrap(), true));
        let grant = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(8), browser.replies.recv())
                    .await
                    .unwrap()
                    .unwrap()
            });
        ui.response(grant);
        assert_eq!(ui.git_epoch, epoch);
        ui.git_response(held_git);
        assert_eq!(ui.git.reviews[&review_tab(&ui, group).1].0.root(), repo.b);

        assert_eq!(
            ui.layout
                .git_context_for(b)
                .and_then(|id| ui.git.contexts.label(id)),
            Some(repo.b.as_path())
        );
    }

    #[test]
    fn terminal_executor_denies_replaced_or_revoked_git_roots_before_spawn() {
        for replace in [true, false] {
            let repo = Repo::new();
            let mut ui = fixture(&repo);
            let b = open_other(&mut ui, &repo);
            let context = ui.layout.git_context_for(b).unwrap().clone();
            let root = ui.terminal_git_root().unwrap();
            let pool = terminals::Pool::new();
            let worker_pool = pool.clone();
            let shell = std::env::var_os("AW_BASH")
                .map(PathBuf::from)
                .unwrap_or_else(|| "/bin/bash".into());
            let (started_tx, started_rx) = std::sync::mpsc::channel();
            let (resume_tx, resume_rx) = std::sync::mpsc::channel();
            let (result_tx, result_rx) = std::sync::mpsc::channel();
            ui.background
                .try_submit("terminal", move || {
                    started_tx.send(()).unwrap();
                    resume_rx.recv_timeout(Duration::from_secs(8)).unwrap();
                    let result = root.launch(worker_pool, shell, "fixture-owner".into());
                    result_tx.send(result).unwrap();
                })
                .unwrap();
            started_rx.recv_timeout(Duration::from_secs(8)).unwrap();
            if replace {
                std::fs::rename(&repo.b, repo.base.join("retained old beta")).unwrap();
                std::fs::create_dir(&repo.b).unwrap();
                repo.run(&repo.b, &["init", "-b", "replacement"]);
                assert!(
                    ui.git.contexts.access(&context).is_some(),
                    "physical replacement leaves the captured Access live until worker validation"
                );
            } else {
                ui.apply(Action::ClosePane(b));
                assert!(ui.git.contexts.access(&context).is_none());
            }
            assert!(pool.all().is_empty());
            resume_tx.send(()).unwrap();
            let result = result_rx.recv_timeout(Duration::from_secs(8)).unwrap();
            assert!(result
                .unwrap_err()
                .contains("Git repository changed or unavailable"));
            assert!(
                pool.all().is_empty(),
                "denial must precede retained terminal creation"
            );
        }
    }

    #[test]
    fn dormant_bindings_keep_authority_and_terminal_roots_never_cross_containers() {
        let repo = Repo::new();
        let mut ui = fixture(&repo);
        let a = ui.layout.focused;
        let a_context = ui.layout.git_context_for(a).unwrap().clone();
        let b = open_other(&mut ui, &repo);
        let b_context = ui.layout.git_context_for(b).unwrap().clone();
        assert_eq!(ui.terminal_git_root().unwrap().path, repo.b);
        ui.git.panes.get_mut(&b).unwrap().filter = "lib".into();
        ui.apply(Action::SetNavigator {
            pane: b,
            kind: Navigator::Sessions,
        });
        assert!(ui.git.contexts.access(&b_context).is_some());
        assert_eq!(ui.terminal_git_root().unwrap().path, repo.a);
        ui.apply(Action::SetNavigator {
            pane: b,
            kind: Navigator::Git,
        });
        ui.git.ensure(b);
        settle(&mut ui);
        assert_eq!(ui.git.panes[&b].filter, "lib");
        assert_eq!(ui.terminal_git_root().unwrap().path, repo.b);
        ui.apply(Action::Focus(a));
        assert_eq!(ui.terminal_git_root().unwrap().path, repo.a);
        assert_eq!(ui.layout.git_context_for(a), Some(&a_context));
        let unavailable = GitContextId("saved-external-context".into());
        ui.apply(Action::SetGitContext {
            navigator: b,
            context_id: unavailable.clone(),
        });
        ui.apply(Action::SetGitLabel {
            context_id: unavailable,
            label: "checkout betaλ · saved label".into(),
        });
        ui.apply(Action::Focus(b));
        ui.git.ensure(b);
        assert!(ui.git.panes[&b].notice.contains("unavailable"));
        assert!(ui.terminal_git_root().is_err());
        assert_eq!(
            ui.git_context_label(ui.layout.git_context_for(b).unwrap()),
            "checkout betaλ · saved label [unavailable]"
        );
        assert!(draw(&mut ui).contains("checkout betaλ"));
        assert!(ui.authorize_git(repo.b.to_str().unwrap(), true));
        let grant = next(&mut ui);
        ui.git_response(grant);
        ui.git.ensure(b);
        settle(&mut ui);
        assert_eq!(ui.terminal_git_root().unwrap().path, repo.b);
        assert_eq!(ui.layout.git_context_for(a), Some(&a_context));
    }
}
