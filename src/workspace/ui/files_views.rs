//! Human Files capabilities and view lifetimes. Layout descriptors never grant roots.
use super::*;

impl Ui {
    pub(super) fn current_file_scope(&self) -> ScopeId {
        self.layout
            .active(self.layout.focused)
            .and_then(|tab| match &tab.target {
                Target::File { scope_id, .. } => Some(scope_id.clone()),
                _ => None,
            })
            .or_else(|| self.layout.files_scope_for(self.layout.focused).cloned())
            .unwrap_or_else(ScopeId::launch)
    }

    pub(super) fn file_scope_label(&self, scope_id: &ScopeId) -> String {
        self.file_scopes
            .label(scope_id)
            .map(|path| {
                let root = aw_workspace::editor::display(&path.display().to_string());
                let name = path
                    .file_name()
                    .map(|name| aw_workspace::editor::display(&name.to_string_lossy()));
                name.map_or_else(|| root.clone(), |name| format!("{name} · {root}"))
            })
            .unwrap_or_else(|| {
                self.layout
                    .scope_labels
                    .get(scope_id)
                    .map(|label| format!("{} [unavailable]", aw_workspace::editor::display(label)))
                    .unwrap_or_else(|| "unavailable folder".into())
            })
    }

    pub(super) fn file_token(
        &self,
        scope_id: &ScopeId,
        container: Option<Id>,
    ) -> Result<FileToken> {
        if self.file_epoch == u64::MAX {
            return Err(Error::Limit("Files request lifetime exhausted"));
        }
        let access = self.file_scopes.access(scope_id).ok_or_else(|| {
            Error::Config(
                "Folder unavailable. Focus its Files view and choose Reconnect Files folder…."
                    .into(),
            )
        })?;
        Ok(FileToken {
            access,
            epoch: self.file_epoch,
            container,
        })
    }

    fn file_job_current(&self, job: &Job) -> bool {
        let Some(token) = job.token() else {
            return false;
        };
        if token.epoch != self.file_epoch
            || !token.access.is_valid()
            || !self
                .file_scopes
                .access(token.access.scope_id())
                .is_some_and(|access| access.same_binding(&token.access))
        {
            return false;
        }
        match job {
            Job::List { pane, .. } | Job::Search { pane: Some(pane), .. } => {
                matches!(self.layout.panes.get(pane), Some(Pane::Navigator { kind: Navigator::Files }))
                    && self.layout.files_scope_for(*pane) == Some(token.access.scope_id())
                    && self.layout.files_container_for(*pane) == token.container
            }
            Job::Read { tab, path, .. } => {
                self.layout.tabs.get(tab).is_some_and(|tab| matches!(&tab.target,
                    Target::File { scope_id, path: current } if scope_id == token.access.scope_id() && current == path))
                    && self.layout.group_for(*tab).and_then(|group| self.layout.files_container_for(group)) == token.container
            }
            Job::Search { pane: None, .. } => self.palette.is_some()
                && &self.palette_file_scope == token.access.scope_id()
                && self.palette_file_container == token.container,
            Job::Authorize { .. } => false,
        }
    }

    pub(super) fn reconcile_file_views(&mut self) {
        let mut bindings = BTreeMap::new();
        for (pane, kind) in &self.layout.panes {
            if matches!(
                kind,
                Pane::Navigator {
                    kind: Navigator::Files
                }
            ) {
                if let Some(scope) = self.layout.files_scope_for(*pane) {
                    bindings.insert(
                        *pane,
                        (scope.clone(), self.layout.files_container_for(*pane), None),
                    );
                }
            }
        }
        for (tab, descriptor) in &self.layout.tabs {
            if let Target::File { scope_id, path } = &descriptor.target {
                let container = self
                    .layout
                    .group_for(*tab)
                    .and_then(|group| self.layout.files_container_for(group));
                bindings.insert(*tab, (scope_id.clone(), container, Some(path.clone())));
            }
        }
        if self.file_bindings != bindings {
            for (pane, binding) in &self.file_bindings {
                if binding.2.is_none() && bindings.get(pane) != Some(binding) {
                    self.nav.remove(&(*pane, Navigator::Files));
                }
            }
            let changed = bindings
                .iter()
                .filter_map(|(tab, binding)| {
                    binding.2.as_ref()?;
                    let old = self.file_bindings.get(tab);
                    (old.is_some_and(|old| old.0 != binding.0 || old.2 != binding.2))
                        .then_some(*tab)
                })
                .collect::<Vec<_>>();
            self.invalidate_file_work();
            for tab in changed {
                self.discard_file_derived(tab);
            }
            self.file_bindings = bindings;
        }
        let live = self
            .layout
            .tabs
            .values()
            .filter_map(|tab| match &tab.target {
                Target::File { scope_id, path } => {
                    Some(FileKey::new(scope_id.clone(), path.clone()))
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let mut retained = live
            .iter()
            .map(|key| key.scope_id.clone())
            .collect::<BTreeSet<_>>();
        retained.extend(
            self.layout
                .containers
                .values()
                .map(|container| container.scope_id.clone()),
        );
        self.file_scopes.retain(&retained);
        for scope in &retained {
            if self.file_scopes.access(scope).is_none() {
                continue;
            }
            let mut label = self.file_scope_label(scope);
            if label.len() > MAX_SCOPE_LABEL_BYTES {
                let mut end = MAX_SCOPE_LABEL_BYTES - '…'.len_utf8();
                while !label.is_char_boundary(end) {
                    end -= 1;
                }
                label.truncate(end);
                label.push('…');
            }
            if self.layout.scope_labels.get(scope) != Some(&label) {
                match self.layout.apply(Action::SetScopeLabel {
                    scope_id: scope.clone(),
                    label,
                }) {
                    Ok(()) => self.layout_generation = self.layout_generation.wrapping_add(1),
                    Err(error) => self.notice = error.to_string(),
                }
            }
        }
        self.files.retain(|key, _| {
            live.contains(key) && self.file_scopes.access(&key.scope_id).is_some()
        });
        self.file_lines
            .retain(|key, _| self.files.contains_key(key));
        self.file_highlights
            .retain(|key, _| self.files.contains_key(key));
        let obsolete = self
            .jobs
            .iter()
            .filter_map(|(ticket, job)| {
                if job.token().is_some() && !self.file_job_current(job) {
                    Some(*ticket)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for ticket in obsolete {
            if let Some(Job::Search { cancel, .. }) = self.jobs.remove(&ticket) {
                cancel.store(true, Ordering::Release);
            }
        }
    }

    /// One conservative UI lifetime fences the small, bounded Files work graph.
    /// Focus, tab selection, scrolling and geometry do not replace this lifetime.
    pub(super) fn invalidate_file_work(&mut self) {
        self.file_epoch = self.file_epoch.checked_add(1).unwrap_or(u64::MAX);
        for job in self.jobs.values() {
            if let Job::Search { cancel, .. } = job {
                cancel.store(true, Ordering::Release);
            }
        }
        self.jobs.clear();
        for ((_, kind), nav) in &mut self.nav {
            if *kind == Navigator::Files {
                nav.pending = None;
                nav.search_pending = None;
            }
        }
        self.palette_file_pending = None;
        self.palette_file_query.clear();
        self.palette_file_results.clear();
        self.file_highlights.clear();
        self.file_find_dirty
            .retain(|tab| self.layout.tabs.contains_key(tab));
        let tabs = self
            .layout
            .tabs
            .values()
            .filter_map(|tab| matches!(tab.target, Target::File { .. }).then_some(tab.id))
            .collect::<Vec<_>>();
        for tab in tabs {
            if let Some(cancel) = self.find_cancel.remove(&tab) {
                cancel.store(true, Ordering::Release);
                self.file_find_dirty.insert(tab);
            }
            self.find_generation.remove(&tab);
        }
        self.rendered.clear();
    }

    fn discard_file_derived(&mut self, tab: Id) {
        self.file_find_dirty.insert(tab);
        if let Some(find) = self.find.get_mut(&tab) {
            find.matches.clear();
            find.choosing = false;
        }
        self.selectable_views.remove(&tab);
        self.saved_selections.remove(&tab);
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

    pub(super) fn reset_file_lifetime(&mut self) {
        self.invalidate_file_work();
        let tabs = self
            .layout
            .tabs
            .values()
            .filter_map(|tab| matches!(tab.target, Target::File { .. }).then_some(tab.id))
            .collect::<Vec<_>>();
        for tab in tabs {
            self.discard_file_derived(tab);
        }
    }

    pub(super) fn file_reply_current(&self, job: &Job, access: Option<&Access>) -> bool {
        self.file_job_current(job)
            && access
                .zip(job.token())
                .is_some_and(|(access, token)| access.same_binding(&token.access))
    }

    pub(super) fn reconnect_file_scope(&self, pane: Id) -> Option<ScopeId> {
        self.layout
            .active(pane)
            .and_then(|tab| match &tab.target {
                Target::File { scope_id, .. } => Some(scope_id.clone()),
                _ => None,
            })
            .or_else(|| self.layout.files_scope_for(pane).cloned())
    }

    pub(super) fn authorize_files(
        &mut self,
        browser: &Browser,
        path: &str,
        reconnect: bool,
    ) -> bool {
        if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
            self.notice = if reconnect {
                "files-reconnect FOLDER_PATH"
            } else {
                "files-open FOLDER_PATH"
            }
            .into();
            return false;
        }
        let beside = self.layout.focused;
        let scope = if reconnect {
            let Some(scope) = self.reconnect_file_scope(beside) else {
                self.notice =
                    "Focus a Files navigator or file before reconnecting its folder.".into();
                return false;
            };
            Some(scope)
        } else {
            None
        };
        if self
            .jobs
            .values()
            .any(|job| matches!(job, Job::Authorize { .. }))
        {
            self.notice = "A Files folder is already opening.".into();
            return false;
        }
        if self.jobs.len() >= 16 {
            self.notice = "Too many Files previews pending; retry shortly.".into();
            return false;
        }
        let Some(ticket) = self.ticket.checked_add(1) else {
            self.notice = "Files request IDs exhausted.".into();
            return false;
        };
        self.ticket = ticket;
        let path = PathBuf::from(path);
        let path = if path.is_absolute() {
            path
        } else {
            self.root.join(path)
        };
        match browser.authorize(ticket, path) {
            Ok(()) => {
                let origin_tab = reconnect
                    .then(|| self.layout.active(beside).map(|tab| tab.id))
                    .flatten();
                self.jobs.insert(
                    ticket,
                    Job::Authorize {
                        beside,
                        reconnect: scope,
                        origin_tab,
                        epoch: self.file_epoch,
                    },
                );
                self.notice = "Opening read-only Files folder…".into();
                true
            }
            Err(error) => {
                self.notice = error.to_string();
                false
            }
        }
    }

    pub(super) fn refresh_file(&mut self, browser: &Browser, key: FileKey) {
        self.invalidate_file_work();
        self.files.remove(&key);
        self.file_lines.remove(&key);
        self.file_highlights.remove(&key);
        let tabs = self
            .layout
            .tabs
            .values()
            .filter(|tab| tab.target == key.target())
            .map(|tab| tab.id)
            .collect::<Vec<_>>();
        for tab in tabs {
            self.discard_file_derived(tab);
        }
        self.file_revision = self.file_revision.wrapping_add(1);
        self.read_file_key(browser, key);
    }

    pub(super) fn refresh_files_navigator(&mut self, browser: &Browser, pane: Id, path: String) {
        self.invalidate_file_work();
        self.list_files(browser, pane, path);
    }

    pub(super) fn refresh_file_find(&mut self, key: &FileKey) {
        let tabs = self
            .layout
            .tabs
            .values()
            .filter(|tab| tab.target == key.target())
            .map(|tab| tab.id)
            .collect::<Vec<_>>();
        for tab in tabs {
            self.file_find_dirty.remove(&tab);
            if let Some(find) = self.find.get(&tab) {
                if !find.query.is_empty() {
                    self.schedule_find(tab, key.target(), find.query.clone(), find.committed);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use std::sync::atomic::AtomicU64;

    struct Roots {
        base: PathBuf,
        a: PathBuf,
        b: PathBuf,
    }
    impl Roots {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(1);
            let base = std::env::temp_dir().join(format!(
                "aw-scoped-files-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&base).unwrap();
            let a = base.join("Alpha雪");
            let b = base.join("Betaλ");
            for (root, prefix) in [(&a, "ALPHA"), (&b, "BETA")] {
                std::fs::create_dir_all(root.join("src")).unwrap();
                std::fs::write(
                    root.join("src/lib.rs"),
                    (0..120)
                        .map(|row| format!("// {prefix} row {row}\n"))
                        .collect::<String>(),
                )
                .unwrap();
            }
            Self { base, a, b }
        }
    }
    impl Drop for Roots {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    fn fixture(roots: &Roots) -> (Ui, Browser) {
        let mut snapshot = super::super::render_tests::projection();
        snapshot.root = roots.a.display().to_string();
        snapshot.agent_root = Some(roots.a.clone());
        let mut ui = Ui::new(
            Workspace::default(),
            "draft 界 keep caret".into(),
            None,
            roots.a.clone(),
            snapshot,
            aw::config::UiConfig::default(),
        )
        .unwrap();
        ui.draft.left();
        ui.set_navigator(Navigator::Files);
        let browser = Browser::new(ui.background.clone());
        (ui, browser)
    }

    fn draw(ui: &mut Ui) -> String {
        let mut terminal = Terminal::new(TestBackend::new(220, 48)).unwrap();
        terminal.draw(|frame| ui.draw(frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    fn pump(ui: &mut Ui, browser: &mut Browser, mut ready: impl FnMut(&Ui) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            ui.visible_files(browser);
            while let Ok(reply) = browser.replies.try_recv() {
                ui.response(reply);
            }
            while let Ok(reply) = ui.background_rx.try_recv() {
                ui.apply_background_result(reply);
            }
            if ready(ui) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "Files background work did not settle: {}",
                ui.notice
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn next_reply(
        ticket: u64,
        ui: &mut Ui,
        browser: &mut Browser,
    ) -> super::super::super::files::Response {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Ok(reply) = browser.replies.try_recv() {
                if reply.ticket == ticket {
                    return reply;
                }
                ui.response(reply);
            }
            assert!(Instant::now() < deadline, "Files reply timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn open_second(
        ui: &mut Ui,
        browser: &mut Browser,
        root: &std::path::Path,
    ) -> (ScopeId, Id, Id) {
        assert!(ui.authorize_files(browser, root.to_str().unwrap(), false));
        let reply = next_reply(ui.ticket, ui, browser);
        ui.response(reply);
        let nav = ui.layout.focused;
        let scope = ui.layout.files_scope_for(nav).unwrap().clone();
        let group = ui.layout.files_group_for(nav).unwrap();
        (scope, nav, group)
    }

    #[test]
    fn scoped_files_real_browser_keeps_two_roots_state_focus_and_agent_independent() {
        let roots = Roots::new();
        let (mut ui, mut browser) = fixture(&roots);
        let draft = (ui.draft.text().to_owned(), ui.draft.cursor());
        let agent = (
            ui.snapshot.agent_root.clone(),
            ui.snapshot.session.clone(),
            ui.snapshot.branch_id.clone(),
            ui.snapshot.generation,
            ui.snapshot.turns.clone(),
        );
        let nav_a = ui.layout.focused;
        let group_a = ui.group();
        ui.read_file(&browser, "src/lib.rs".into());
        assert_eq!(ui.layout.focused, nav_a);
        let tab_a = ui.layout.active(group_a).unwrap().id;
        let a = FileKey::new(ScopeId::launch(), "src/lib.rs".into());
        pump(&mut ui, &mut browser, |ui| ui.files.contains_key(&a));
        let (scope_b, nav_b, group_b) = open_second(&mut ui, &mut browser, &roots.b);
        ui.read_file(&browser, "src/lib.rs".into());
        let tab_b = ui.layout.active(group_b).unwrap().id;
        assert_eq!(ui.layout.focused, nav_b);
        let b = FileKey::new(scope_b.clone(), "src/lib.rs".into());
        pump(&mut ui, &mut browser, |ui| {
            ui.files.contains_key(&a) && ui.files.contains_key(&b)
        });
        assert!(ui.files[&a].contains("ALPHA") && !ui.files[&a].contains("BETA"));
        assert!(ui.files[&b].contains("BETA") && !ui.files[&b].contains("ALPHA"));
        let frame = draw(&mut ui);
        assert!(frame.contains("Alpha雪") && frame.contains("Betaλ"));
        ui.scroll.get_mut(&tab_a).unwrap().scroll(17);
        assert_ne!(ui.scroll[&tab_a].offset(), ui.scroll[&tab_b].offset());
        ui.update_find(tab_a, "ALPHA row 70".into(), false);
        ui.update_find(tab_b, "BETA row 50".into(), false);
        pump(&mut ui, &mut browser, |ui| {
            ui.find
                .get(&tab_a)
                .is_some_and(|find| find.matches == vec![72])
                && ui
                    .find
                    .get(&tab_b)
                    .is_some_and(|find| find.matches == vec![52])
        });
        assert_eq!(ui.find[&tab_a].query, "ALPHA row 70");
        assert_eq!(ui.find[&tab_b].query, "BETA row 50");
        ui.apply(Action::Focus(group_b));
        ui.focus_explorer();
        assert_eq!(ui.layout.focused, nav_b);
        ui.focus_main();
        assert_eq!(ui.layout.focused, group_b);
        ui.apply(Action::SplitGroup {
            pane: group_b,
            side: Side::Below,
        });
        let split = ui.layout.focused;
        assert_eq!(ui.layout.files_scope_for(split), Some(&scope_b));
        ui.apply(Action::Pin {
            tab: tab_a,
            pinned: true,
        });
        ui.apply(Action::MoveTab {
            tab: tab_a,
            group: split,
        });
        assert_eq!(ui.layout.tabs[&tab_a].target, a.target());
        assert_eq!(ui.find[&tab_a].query, "ALPHA row 70");
        ui.apply(Action::Focus(nav_b));
        ui.open_file_preview_key(b.clone());
        assert_eq!(ui.layout.focused, nav_b);
        assert_eq!(ui.layout.active(split).unwrap().target, b.target());
        assert_eq!((ui.draft.text().to_owned(), ui.draft.cursor()), draft);
        assert_eq!(
            (
                ui.snapshot.agent_root.clone(),
                ui.snapshot.session.clone(),
                ui.snapshot.branch_id.clone(),
                ui.snapshot.generation
            ),
            (agent.0, agent.1, agent.2, agent.3)
        );
        assert!(Arc::ptr_eq(&ui.snapshot.turns, &agent.4));
        assert_eq!(ui.root, roots.a.canonicalize().unwrap());
        assert_eq!(ui.snapshot.pending_prompt, "");
        assert!(ui
            .layout
            .tabs
            .values()
            .any(|tab| matches!(tab.target, Target::Conversation)));
    }

    #[test]
    fn scoped_files_navigator_remembers_its_last_group_across_other_folders() {
        let roots = Roots::new();
        let (mut ui, mut browser) = fixture(&roots);
        let nav_a = ui.layout.focused;
        let group_a1 = ui.group();
        let key_a = FileKey::new(ScopeId::launch(), "src/lib.rs".into());
        ui.read_file(&browser, key_a.path.clone());
        let tab_a1 = ui.layout.active(group_a1).unwrap().id;
        pump(&mut ui, &mut browser, |ui| ui.files.contains_key(&key_a));
        ui.apply(Action::SplitGroup {
            pane: group_a1,
            side: Side::Below,
        });
        let group_a2 = ui.layout.focused;
        ui.read_file(&browser, key_a.path.clone());
        let tab_a2 = ui.layout.active(group_a2).unwrap().id;
        draw(&mut ui);
        ui.update_find(tab_a2, "ALPHA row 70".into(), false);
        pump(&mut ui, &mut browser, |ui| {
            ui.find
                .get(&tab_a2)
                .is_some_and(|find| find.matches == vec![72])
        });
        ui.scroll.get_mut(&tab_a1).unwrap().scroll(7);
        ui.scroll.get_mut(&tab_a2).unwrap().scroll(17);
        let positions = (ui.scroll[&tab_a1].offset(), ui.scroll[&tab_a2].offset());
        let (_, _, group_b) = open_second(&mut ui, &mut browser, &roots.b);
        ui.read_file(&browser, "src/lib.rs".into());
        let key_b = match &ui.layout.active(group_b).unwrap().target {
            Target::File { scope_id, path } => FileKey::new(scope_id.clone(), path.clone()),
            _ => unreachable!(),
        };
        pump(&mut ui, &mut browser, |ui| {
            ui.files
                .get(&key_b)
                .is_some_and(|text| text.contains("BETA"))
        });
        let ids = ui.layout.tabs.keys().copied().collect::<Vec<_>>();
        let draft = (ui.draft.text().to_owned(), ui.draft.cursor());
        ui.apply(Action::Focus(group_a2));
        ui.apply(Action::Focus(group_b));
        ui.apply(Action::Focus(nav_a));
        ui.read_file(&browser, key_a.path.clone());
        assert_eq!(
            ui.layout.focused, nav_a,
            "preview preserves the invoking Explorer focus"
        );
        assert_eq!(
            ui.group(),
            group_a2,
            "each folder retains its last-used document group"
        );
        assert_eq!(ui.layout.active(group_a1).unwrap().id, tab_a1);
        assert_eq!(ui.layout.active(group_a2).unwrap().id, tab_a2);
        assert_eq!(ui.layout.tabs.keys().copied().collect::<Vec<_>>(), ids);
        assert_eq!(
            (ui.scroll[&tab_a1].offset(), ui.scroll[&tab_a2].offset()),
            positions
        );
        assert_eq!(ui.find[&tab_a2].query, "ALPHA row 70");
        assert_eq!(ui.find[&tab_a2].matches, vec![72]);
        assert_eq!((ui.draft.text().to_owned(), ui.draft.cursor()), draft);
        let saved = ui.layout.clone();
        ui.apply_background_result(UiBackgroundResult::Layout {
            base_generation: ui.layout_generation,
            result: Ok(Some(saved)),
        });
        assert_eq!(
            ui.group(),
            group_a1,
            "restored layouts start with their own placement, without an old remembered group"
        );
        ui.apply(Action::Focus(group_a2));
        ui.apply(Action::Focus(nav_a));
        assert_eq!(ui.group(), group_a2);
        ui.apply(Action::ClosePane(group_a2));
        ui.apply(Action::Focus(nav_a));
        assert_eq!(ui.group(), group_a1, "closed document groups are pruned");
    }

    #[test]
    fn scoped_files_close_restore_rejects_actual_read_and_search_replies() {
        let roots = Roots::new();
        let (mut ui, mut browser) = fixture(&roots);
        let nav = ui.layout.focused;
        let group = ui.group();
        ui.read_file(&browser, "src/lib.rs".into());
        let tab = ui.layout.active(group).unwrap().id;
        let old_read = next_reply(ui.ticket, &mut ui, &mut browser);
        let saved = ui.layout.clone();
        ui.apply(Action::CloseTab { group, tab });
        ui.apply_background_result(UiBackgroundResult::Layout {
            base_generation: ui.layout_generation,
            result: Ok(Some(saved)),
        });
        let epoch = ui.file_epoch;
        ui.response(old_read);
        assert!(
            ui.files.is_empty(),
            "a restored ID cannot admit the prior view's read"
        );
        ui.apply(Action::Focus(nav));
        ui.nav.entry((nav, Navigator::Files)).or_default().filter = "lib".into();
        ui.search_file_filter(&browser, nav, "lib");
        let old_search = next_reply(ui.ticket, &mut ui, &mut browser);
        let saved = ui.layout.clone();
        ui.apply(Action::SetNavigator {
            pane: nav,
            kind: Navigator::Sessions,
        });
        ui.apply_background_result(UiBackgroundResult::Layout {
            base_generation: ui.layout_generation,
            result: Ok(Some(saved)),
        });
        assert!(ui.file_epoch > epoch);
        ui.response(old_search);
        assert!(ui
            .nav
            .get(&(nav, Navigator::Files))
            .is_none_or(|nav| nav.search_results.is_empty()));
        let key = FileKey::new(ScopeId::launch(), "src/lib.rs".into());
        pump(&mut ui, &mut browser, |ui| {
            ui.files
                .get(&key)
                .is_some_and(|text| text.contains("ALPHA"))
        });
    }

    #[test]
    fn scoped_files_explicit_reconnect_rejects_actual_old_read_search_and_highlight() {
        let roots = Roots::new();
        let (mut ui, mut browser) = fixture(&roots);
        let (scope, nav, group) = open_second(&mut ui, &mut browser, &roots.b);
        ui.read_file(&browser, "src/lib.rs".into());
        let key = FileKey::new(scope.clone(), "src/lib.rs".into());
        let tab = ui.layout.active(group).unwrap().id;
        pump(&mut ui, &mut browser, |ui| ui.files.contains_key(&key));
        ui.schedule_file_highlight(&key);
        let generation = ui.file_highlights[&key].generation;
        let deadline = Instant::now() + Duration::from_secs(8);
        let old_highlight = loop {
            if let Ok(reply) = ui.background_rx.try_recv() {
                if matches!(&reply, UiBackgroundResult::Highlight { target: HighlightTarget::File(current), generation: g, .. } if current == &key && *g == generation)
                {
                    break reply;
                }
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        ui.files.remove(&key);
        ui.file_lines.remove(&key);
        ui.read_file(&browser, "src/lib.rs".into());
        let old_read = next_reply(ui.ticket, &mut ui, &mut browser);
        ui.nav.entry((nav, Navigator::Files)).or_default().filter = "lib".into();
        ui.search_file_filter(&browser, nav, "lib");
        let old_search = next_reply(ui.ticket, &mut ui, &mut browser);
        let old_access = ui.file_scopes.access(&scope).unwrap();
        ui.apply(Action::Focus(nav));
        assert!(ui.authorize_files(&browser, roots.a.to_str().unwrap(), true));
        let grant = next_reply(ui.ticket, &mut ui, &mut browser);
        ui.response(grant);
        assert!(!old_access.is_valid());
        ui.response(old_read);
        ui.response(old_search);
        ui.apply_background_result(old_highlight);
        assert!(!ui.files.contains_key(&key));
        assert!(!ui.file_highlights.contains_key(&key));
        assert!(ui
            .nav
            .get(&(nav, Navigator::Files))
            .is_none_or(|nav| nav.search_results.is_empty()));
        pump(&mut ui, &mut browser, |ui| {
            ui.files
                .get(&key)
                .is_some_and(|text| text.contains("ALPHA"))
        });
        assert!(!ui.files[&key].contains("BETA"));
        assert_eq!(ui.layout.tabs[&tab].target, key.target());
        assert_eq!(ui.snapshot.agent_root, Some(roots.a.clone()));
    }

    #[test]
    fn scoped_files_root_open_is_fenced_and_does_not_steal_changed_focus() {
        let roots = Roots::new();
        let (mut ui, mut browser) = fixture(&roots);
        let origin = ui.layout.focused;
        let group = ui.group();
        assert!(ui.authorize_files(&browser, roots.b.to_str().unwrap(), false));
        let reply = next_reply(ui.ticket, &mut ui, &mut browser);
        ui.apply(Action::Focus(group));
        ui.input = true;
        let draft = (ui.draft.text().to_owned(), ui.draft.cursor());
        ui.response(reply);
        assert_eq!(ui.layout.focused, group);
        assert!(
            ui.input,
            "a completed folder open cannot take the invoking composer input"
        );
        assert_eq!((ui.draft.text().to_owned(), ui.draft.cursor()), draft);
        ui.apply(Action::Focus(origin));
        assert!(ui.authorize_files(&browser, roots.b.to_str().unwrap(), false));
        let late = next_reply(ui.ticket, &mut ui, &mut browser);
        let saved = ui.layout.clone();
        let containers = saved.containers.len();
        ui.apply(Action::ClosePane(origin));
        ui.apply_background_result(UiBackgroundResult::Layout {
            base_generation: ui.layout_generation,
            result: Ok(Some(saved)),
        });
        ui.response(late);
        assert_eq!(ui.layout.containers.len(), containers);
    }

    #[test]
    fn scoped_files_saved_unknown_root_requires_explicit_reconnect() {
        let roots = Roots::new();
        let (mut ui, mut browser) = fixture(&roots);
        let unknown = ScopeId("saved-external-root".into());
        ui.apply(Action::AddFiles {
            beside: ui.group(),
            side: Side::Right,
            scope_id: unknown.clone(),
        });
        let nav = ui.layout.focused;
        let group = ui.group();
        ui.read_file(&browser, "src/lib.rs".into());
        let key = FileKey::new(unknown.clone(), "src/lib.rs".into());
        assert!(!ui.files.contains_key(&key));
        assert!(ui.jobs.is_empty());
        assert!(draw(&mut ui).contains("Folder unavailable"));
        assert!(ui.authorize_files(&browser, roots.b.to_str().unwrap(), true));
        let grant = next_reply(ui.ticket, &mut ui, &mut browser);
        ui.response(grant);
        pump(&mut ui, &mut browser, |ui| {
            ui.files.get(&key).is_some_and(|text| text.contains("BETA"))
        });
        assert_eq!(ui.layout.focused, nav);
        assert_eq!(ui.layout.active(group).unwrap().target, key.target());
        assert_eq!(ui.snapshot.agent_root, Some(roots.a.clone()));
    }

    #[test]
    fn scoped_files_saved_label_and_moved_file_reconnect_use_the_files_own_scope() {
        let roots = Roots::new();
        let (mut ui, mut browser) = fixture(&roots);
        let group_a = ui.group();
        let (scope, nav, group_b) = open_second(&mut ui, &mut browser, &roots.b);
        ui.read_file(&browser, "src/lib.rs".into());
        let tab = ui.layout.active(group_b).unwrap().id;
        ui.apply(Action::Pin { tab, pinned: true });
        ui.apply(Action::MoveTab {
            tab,
            group: group_a,
        });
        ui.apply(Action::ClosePane(nav));
        ui.apply(Action::ClosePane(group_b));
        assert_eq!(ui.layout.files_scope_for(group_a), Some(&ScopeId::launch()));
        assert!(ui.layout.scope_labels[&scope].contains("Betaλ"));
        let mut restored = Ui::new(
            ui.layout.clone(),
            ui.draft.text().to_owned(),
            None,
            roots.a.clone(),
            ui.snapshot.clone(),
            aw::config::UiConfig::default(),
        )
        .unwrap();
        restored.apply(Action::Focus(group_a));
        assert!(restored.file_scopes.access(&scope).is_none());
        assert!(restored.file_scope_label(&scope).contains("Betaλ"));
        assert!(restored.file_scope_label(&scope).contains("unavailable"));
        let mut browser = Browser::new(restored.background.clone());
        assert!(restored.authorize_files(&browser, roots.b.to_str().unwrap(), true));
        let reply = next_reply(restored.ticket, &mut restored, &mut browser);
        restored.response(reply);
        assert!(restored.file_scopes.access(&scope).is_some());
        assert_eq!(
            restored.file_scopes.label(&ScopeId::launch()),
            Some(roots.a.as_path())
        );
        let key = FileKey::new(scope, "src/lib.rs".into());
        pump(&mut restored, &mut browser, |ui| {
            ui.files.get(&key).is_some_and(|text| text.contains("BETA"))
        });
        assert_eq!(restored.layout.tabs[&tab].target, key.target());
        assert_eq!(restored.snapshot.agent_root, Some(roots.a.clone()));
    }

    #[test]
    fn scoped_files_explicit_refresh_rejects_an_actual_older_read() {
        let roots = Roots::new();
        let (mut ui, mut browser) = fixture(&roots);
        let key = FileKey::new(ScopeId::launch(), "src/lib.rs".into());
        ui.read_file(&browser, key.path.clone());
        let old = next_reply(ui.ticket, &mut ui, &mut browser);
        let group = ui.group();
        ui.apply(Action::Focus(group));
        std::fs::write(roots.a.join(&key.path), "// FRESH refresh only\n").unwrap();
        ui.refresh_file(&browser, key.clone());
        let fresh_ticket = ui.ticket;
        ui.response(old);
        assert!(
            !ui.files.contains_key(&key),
            "explicit refresh must fence the earlier read"
        );
        let fresh = next_reply(fresh_ticket, &mut ui, &mut browser);
        ui.response(fresh);
        assert!(ui.files[&key].contains("FRESH refresh only"));
        assert!(!ui.files[&key].contains("ALPHA"));
    }

    #[test]
    fn scoped_files_explicit_refresh_rejects_an_actual_older_directory_listing() {
        let roots = Roots::new();
        let (mut ui, mut browser) = fixture(&roots);
        let nav = ui.layout.focused;
        ui.list_files(&browser, nav, String::new());
        let old = next_reply(ui.ticket, &mut ui, &mut browser);
        std::fs::write(roots.a.join("new-after-refresh.rs"), "new bytes").unwrap();
        ui.refresh_files_navigator(&browser, nav, String::new());
        let fresh_ticket = ui.ticket;
        ui.response(old);
        assert_eq!(ui.nav[&(nav, Navigator::Files)].pending, Some(fresh_ticket));
        let fresh = next_reply(fresh_ticket, &mut ui, &mut browser);
        ui.response(fresh);
        assert!(ui.nav[&(nav, Navigator::Files)].directories[""]
            .iter()
            .any(|entry| entry.path == "new-after-refresh.rs"));
    }

    #[test]
    fn scoped_files_reconnect_invalidates_palette_results_and_reissues_the_same_query() {
        let roots = Roots::new();
        let (mut ui, mut browser) = fixture(&roots);
        std::fs::write(roots.a.join("old-only.rs"), "old bytes").unwrap();
        std::fs::write(roots.b.join("new-only.rs"), "new bytes").unwrap();
        let (scope, _, _) = open_second(&mut ui, &mut browser, &roots.a);
        assert!(ui.authorize_files(&browser, roots.b.to_str().unwrap(), true));
        let grant = next_reply(ui.ticket, &mut ui, &mut browser);
        ui.open_palette();
        ui.palette.as_mut().unwrap().set("@only").unwrap();
        ui.sync_palette_file_search(&browser, "@only");
        let old_search = next_reply(ui.ticket, &mut ui, &mut browser);
        ui.response(old_search);
        assert!(ui
            .palette_file_results
            .iter()
            .any(|entry| entry.path == "old-only.rs"));
        ui.response(grant);
        assert!(
            ui.palette_file_results.is_empty(),
            "old-root names cannot survive reconnect"
        );
        assert_eq!(ui.palette_file_scope, scope);
        pump(&mut ui, &mut browser, |ui| {
            ui.palette_file_results
                .iter()
                .any(|entry| entry.path == "new-only.rs")
        });
        assert!(!ui
            .palette_file_results
            .iter()
            .any(|entry| entry.path == "old-only.rs"));
        assert_eq!(ui.palette.as_ref().unwrap().text(), "@only");
    }

    #[test]
    fn send_review_uses_actual_agent_root_after_resume_and_preserves_draft_on_denial() {
        let roots = Roots::new();
        let (mut ui, _) = fixture(&roots);
        let git = std::env::var_os("AW_GIT")
            .map(PathBuf::from)
            .expect("pinned shell AW_GIT");
        let status = std::process::Command::new(&git)
            .args(["init", "--quiet"])
            .arg(&roots.a)
            .status()
            .unwrap();
        assert!(status.success());
        let repo = aw_git::Repository::discover(&git, &roots.a).unwrap();
        let change = repo
            .status()
            .unwrap()
            .changes
            .into_iter()
            .find(|change| change.path.label() == "src/lib.rs")
            .unwrap();
        let review = repo.review(&change).unwrap();
        let row = review
            .lines
            .iter()
            .position(|line| line.text.contains("ALPHA"))
            .unwrap();
        let comment = review.comment(row, "Review this exact root").unwrap();
        let id = review.id.clone();
        ui.git.reviews.insert(id.clone(), (repo, review));
        ui.git.comments.insert(id.clone(), vec![comment]);
        ui.open(Target::Diff { review: id.clone() });
        ui.snapshot.agent_root = Some(roots.b.clone()); // Resume changed the Agent; human launch root remains Alpha.
        let draft = (ui.draft.text().to_owned(), ui.draft.cursor());
        assert!(!ui.send_review_feedback());
        assert!(ui.notice.contains("another checkout"));
        assert_eq!((ui.draft.text().to_owned(), ui.draft.cursor()), draft);
        ui.root = roots.b.clone();
        ui.snapshot.agent_root = Some(roots.a.clone()); // inverse proves no launch-root denial.
        assert!(ui.send_review_feedback());
        assert!(ui.draft.text().contains("Review this exact root"));
        assert!(ui.snapshot.pending_prompt.is_empty());
    }
}
