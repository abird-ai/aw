//! Workspace human-UI background coordination.
//! Owns bounded file/search/highlight request and result plumbing for `Ui`.
use super::*;

impl Ui {
    pub(super) fn file_request(&mut self, browser: &Browser, job: Job) -> Result<u64> {
        if self.jobs.len() >= 16 {
            return Err(Error::Limit("too many file previews pending"));
        }
        self.ticket = self
            .ticket
            .checked_add(1)
            .ok_or(Error::Limit("preview ID exhausted"))?;
        let request = match &job {
            Job::List { path, .. } => Request::List(path.clone()),
            Job::Read { path, .. } => Request::Read(path.clone()),
            Job::Search { query, cancel, .. } => Request::Search {
                query: query.clone(),
                limit: 80,
                cancel: cancel.clone(),
            },
            Job::Authorize { .. } => {
                return Err(Error::Config(
                    "root authorization requires explicit Files action".into(),
                ))
            }
        };
        let token = job.token().expect("file request token");
        browser.request(self.ticket, &token.access, request)?;
        self.jobs.insert(self.ticket, job);
        Ok(self.ticket)
    }

    pub(super) fn list_files(&mut self, browser: &Browser, pane: Id, path: String) {
        let Some(scope) = self.layout.files_scope_for(pane).cloned() else {
            return;
        };
        let container = self.layout.files_container_for(pane);
        let token = match self.file_token(&scope, container) {
            Ok(token) => token,
            Err(error) => {
                self.notice = error.to_string();
                return;
            }
        };
        if self.jobs.values().any(|job| matches!(job, Job::List { pane: p, path: q, token: current } if *p == pane && *q == path && current.access.same_binding(&token.access))) { return; }
        match self.file_request(
            browser,
            Job::List {
                pane,
                path: path.clone(),
                token,
            },
        ) {
            Ok(ticket) => {
                let n = self.nav.entry((pane, Navigator::Files)).or_default();
                n.pending = Some(ticket);
                if n.display == NavDisplay::List {
                    n.directory = path;
                    n.entries.clear();
                    n.loaded = false;
                } else if path.is_empty() {
                    n.loaded = false;
                } else {
                    n.expanded.insert(path);
                }
            }
            Err(error) => self.notice = error.to_string(),
        }
    }

    pub(super) fn read_file(&mut self, browser: &Browser, path: String) {
        self.read_file_key(browser, FileKey::new(self.current_file_scope(), path));
    }

    pub(super) fn read_file_key(&mut self, browser: &Browser, key: FileKey) {
        if !aw_workspace::valid_relative(&key.path) {
            self.notice = "Use a clean relative file path under this Files root.".into();
            return;
        }
        let Some(tab) = self.open_file_preview_key(key.clone()) else {
            return;
        };
        self.request_file_view(browser, tab, key);
    }

    fn request_file_view(&mut self, browser: &Browser, tab: Id, key: FileKey) {
        if self.files.contains_key(&key) || self.jobs.values().any(|job| matches!(job, Job::Read { path, token, .. } if path == &key.path && token.access.scope_id() == &key.scope_id)) { return; }
        let container = self
            .layout
            .group_for(tab)
            .and_then(|group| self.layout.container_for(group));
        let token = match self.file_token(&key.scope_id, container) {
            Ok(token) => token,
            Err(error) => {
                self.notice = error.to_string();
                return;
            }
        };
        if let Err(error) = self.file_request(
            browser,
            Job::Read {
                tab,
                path: key.path,
                token,
            },
        ) {
            self.notice = error.to_string();
        }
    }

    pub(super) fn cancel_file_searches(&mut self, pane: Option<Id>) {
        let mut cancelled = Vec::new();
        for (ticket, job) in &self.jobs {
            if let Job::Search {
                pane: active_pane,
                cancel,
                ..
            } = job
            {
                if *active_pane == pane {
                    cancel.store(true, Ordering::Release);
                    cancelled.push(*ticket);
                }
            }
        }
        for ticket in cancelled {
            self.jobs.remove(&ticket);
        }
        match pane {
            None => self.palette_file_pending = None,
            Some(pane) => {
                if let Some(state) = self.nav.get_mut(&(pane, Navigator::Files)) {
                    state.search_pending = None;
                }
            }
        }
    }

    pub(super) fn search_palette_files(&mut self, browser: &Browser, query: &str) {
        let query = query.trim();
        if query.len() < 2 || query.len() > 256 || query.chars().any(char::is_control) {
            self.cancel_file_searches(None);
            self.palette_file_query.clear();
            self.palette_file_results.clear();
            return;
        }
        // This includes completed empty results; paint must not resubmit a miss.
        if self.palette_file_query == query {
            return;
        }
        self.cancel_file_searches(None);
        self.palette_file_query = query.to_owned();
        self.palette_file_results.clear();
        if self.jobs.values().any(
            |job| matches!(job, Job::Search { pane: None, query: active, .. } if active == query),
        ) {
            return;
        }
        let token = match self.file_token(&self.palette_file_scope, self.palette_file_container) {
            Ok(token) => token,
            Err(error) => {
                self.notice = error.to_string();
                return;
            }
        };
        match self.file_request(
            browser,
            Job::Search {
                token,
                pane: None,
                query: query.to_owned(),
                cancel: Arc::new(AtomicBool::new(false)),
            },
        ) {
            Ok(ticket) => self.palette_file_pending = Some(ticket),
            Err(error) => self.notice = error.to_string(),
        }
    }

    pub(super) fn search_file_filter(&mut self, browser: &Browser, pane: Id, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            self.cancel_file_searches(Some(pane));
            let state = self.nav.entry((pane, Navigator::Files)).or_default();
            state.search_query.clear();
            state.search_results.clear();
            return;
        }
        if query.len() > 256 || query.chars().any(char::is_control) {
            self.cancel_file_searches(Some(pane));
            self.nav
                .entry((pane, Navigator::Files))
                .or_default()
                .search_results
                .clear();
            return;
        }
        let state = self.nav.entry((pane, Navigator::Files)).or_default();
        if state.search_query == query
            && (state.search_pending.is_some() || !state.search_results.is_empty())
        {
            return;
        }
        self.cancel_file_searches(Some(pane));
        let state = self.nav.entry((pane, Navigator::Files)).or_default();
        state.search_query = query.to_owned();
        state.search_results.clear();
        if self.jobs.values().any(|job| {
            matches!(
                job,
                Job::Search {
                    pane: Some(active_pane),
                    query: active_query,
                    ..
                } if *active_pane == pane && active_query == query
            )
        }) {
            return;
        }
        let Some(scope) = self.layout.files_scope_for(pane).cloned() else {
            return;
        };
        let token = match self.file_token(&scope, self.layout.files_container_for(pane)) {
            Ok(token) => token,
            Err(error) => {
                self.notice = error.to_string();
                return;
            }
        };
        match self.file_request(
            browser,
            Job::Search {
                token,
                pane: Some(pane),
                query: query.to_owned(),
                cancel: Arc::new(AtomicBool::new(false)),
            },
        ) {
            Ok(ticket) => {
                self.nav
                    .entry((pane, Navigator::Files))
                    .or_default()
                    .search_pending = Some(ticket);
            }
            Err(error) => self.notice = error.to_string(),
        }
    }

    pub(super) fn next_highlight_generation(&mut self) -> Option<u64> {
        self.highlight_generation = self.highlight_generation.checked_add(1)?;
        Some(self.highlight_generation)
    }

    pub(super) fn schedule_file_highlight(&mut self, key: &FileKey) {
        let Some(text) = self.files.get(key).cloned() else {
            return;
        };
        let Some(generation) = self.next_highlight_generation() else {
            self.notice = "Syntax request IDs exhausted.".into();
            return;
        };
        let theme = self.ui_config.theme;
        let total = text.split('\n').take(8192).count();
        let cancel = Arc::new(AtomicBool::new(false));
        self.file_highlights.insert(
            key.clone(),
            HighlightCache {
                generation,
                theme,
                lines: vec![None; total],
                complete: false,
                cancel: cancel.clone(),
            },
        );
        let target = HighlightTarget::File(key.clone());
        let key = key.clone();
        let path = key.path.clone();
        let path_for_job = path.clone();
        let out = self.background_tx.clone();
        let panic_out = out.clone();
        let panic_target = target.clone();
        if let Err(error) = self.background.try_submit_with_panic(
            "syntax",
            move || {
                let target_for_chunks = target.clone();
                let mut final_lines = Vec::with_capacity(total);
                let _ = crate::tui::syntax::highlight_path_progressive(
                    &path_for_job,
                    &text,
                    theme,
                    128,
                    |start, lines| {
                        if cancel.load(Ordering::Acquire) {
                            return false;
                        }
                        final_lines.extend(lines.iter().cloned());
                        try_send_ui_progress(
                            &out,
                            UiBackgroundResult::Highlight {
                                target: target_for_chunks.clone(),
                                generation,
                                start,
                                lines,
                                done: false,
                            },
                        )
                    },
                );
                if !cancel.load(Ordering::Acquire) {
                    let _ = out.blocking_send(UiBackgroundResult::Highlight {
                        target,
                        generation,
                        start: 0,
                        lines: final_lines,
                        done: true,
                    });
                }
            },
            move || {
                let _ = panic_out.blocking_send(UiBackgroundResult::Highlight {
                    target: panic_target,
                    generation,
                    start: 0,
                    lines: Vec::new(),
                    done: true,
                });
            },
        ) {
            if let Some(cache) = self.file_highlights.get_mut(&key) {
                cache.complete = true;
            }
            self.notice = error.to_string();
        }
    }

    pub(super) fn schedule_diff_highlight(&mut self, review_id: &str) {
        self.diff_highlights
            .retain(|id, _| self.git.reviews.contains_key(id));
        let Some((_, review)) = self.git.reviews.get(review_id).cloned() else {
            return;
        };
        let Some(generation) = self.next_highlight_generation() else {
            self.notice = "Syntax request IDs exhausted.".into();
            return;
        };
        let theme = self.ui_config.theme;
        let cancel = Arc::new(AtomicBool::new(false));
        self.diff_highlights.insert(
            review_id.to_owned(),
            HighlightCache {
                generation,
                theme,
                lines: vec![None; review.lines.len()],
                complete: false,
                cancel: cancel.clone(),
            },
        );
        let target = HighlightTarget::Diff(review_id.to_owned());
        let out = self.background_tx.clone();
        let panic_out = out.clone();
        let panic_target = target.clone();
        if let Err(error) = self.background.try_submit_with_panic(
            "syntax",
            move || {
                let target_for_chunks = target.clone();
                let mut final_lines = Vec::with_capacity(review.lines.len());
                let _ = syntax::highlight_diff_progressive(
                    &review.path_label,
                    &review.lines,
                    theme,
                    128,
                    |start, lines| {
                        if cancel.load(Ordering::Acquire) {
                            return false;
                        }
                        final_lines.extend(lines.iter().cloned());
                        try_send_ui_progress(
                            &out,
                            UiBackgroundResult::Highlight {
                                target: target_for_chunks.clone(),
                                generation,
                                start,
                                lines,
                                done: false,
                            },
                        )
                    },
                );
                if !cancel.load(Ordering::Acquire) {
                    let _ = out.blocking_send(UiBackgroundResult::Highlight {
                        target,
                        generation,
                        start: 0,
                        lines: final_lines,
                        done: true,
                    });
                }
            },
            move || {
                let _ = panic_out.blocking_send(UiBackgroundResult::Highlight {
                    target: panic_target,
                    generation,
                    start: 0,
                    lines: Vec::new(),
                    done: true,
                });
            },
        ) {
            if let Some(cache) = self.diff_highlights.get_mut(review_id) {
                cache.complete = true;
            }
            self.notice = error.to_string();
        }
    }

    pub(super) fn refresh_file_highlights_for_theme(&mut self) {
        let files = self.files.keys().cloned().collect::<Vec<_>>();
        let diffs = self.git.reviews.keys().cloned().collect::<Vec<_>>();
        for path in files {
            self.schedule_file_highlight(&path);
        }
        for review in diffs {
            self.schedule_diff_highlight(&review);
        }
        self.rendered.clear();
    }

    pub(super) fn response(&mut self, response: super::super::files::Response) {
        let Some(job) = self.jobs.remove(&response.ticket) else {
            return;
        };
        if let Job::Authorize {
            beside,
            reconnect,
            origin_tab,
            epoch,
        } = job
        {
            if epoch != self.file_epoch
                || !self.layout.panes.contains_key(&beside)
                || response.access.is_some()
            {
                return;
            }
            let grant = match response.result {
                Ok(Content::Authorized(grant)) => grant,
                Err(error) => {
                    self.notice = error;
                    return;
                }
                _ => {
                    self.notice = "Invalid Files root authorization reply".into();
                    return;
                }
            };
            if let Some(scope) = reconnect {
                if self.reconnect_file_scope(beside).as_ref() != Some(&scope)
                    || origin_tab.is_some_and(|tab| {
                        self.layout
                            .active(beside)
                            .is_none_or(|active| active.id != tab)
                    })
                {
                    return;
                }
                match self.file_scopes.rebind(&scope, grant) {
                    Ok(()) => {
                        self.reset_file_lifetime();
                        self.files.retain(|key, _| key.scope_id != scope);
                        self.file_lines.retain(|key, _| key.scope_id != scope);
                        let navigators = self
                            .layout
                            .panes
                            .keys()
                            .filter(|pane| self.layout.files_scope_for(**pane) == Some(&scope))
                            .copied()
                            .collect::<Vec<_>>();
                        for pane in navigators {
                            self.nav.remove(&(pane, Navigator::Files));
                        }
                        self.reconcile_layout_state();
                        self.notice = "Files folder reconnected; Agent unchanged.".into();
                    }
                    Err(error) => self.notice = error.to_string(),
                }
            } else {
                match self.file_scopes.install(grant) {
                    Ok(scope) => {
                        let focused = self.layout.focused;
                        let input = self.input;
                        match self.layout.apply(Action::AddFiles {
                            beside,
                            side: Side::Right,
                            scope_id: scope.clone(),
                        }) {
                            Ok(()) => {
                                self.layout_generation = self.layout_generation.wrapping_add(1);
                                self.reconcile_layout_state();
                                if focused != beside && self.layout.panes.contains_key(&focused) {
                                    self.apply(Action::Focus(focused));
                                    self.input = input;
                                } else {
                                    self.input = false;
                                }
                                self.notice =
                                    "Read-only Files folder opened; Agent unchanged.".into();
                            }
                            Err(error) => {
                                self.file_scopes.close(&scope);
                                self.notice = error.to_string();
                            }
                        }
                    }
                    Err(error) => self.notice = error.to_string(),
                }
            }
            return;
        }
        if !self.file_reply_current(&job, response.access.as_ref()) {
            return;
        }
        match job {
            Job::List { pane, path, .. } => {
                let n = self.nav.entry((pane, Navigator::Files)).or_default();
                if n.pending == Some(response.ticket) {
                    n.pending = None;
                }
                match response.result {
                    Ok(Content::Directory(list)) if list.path == path => {
                        n.loaded = true;
                        n.incomplete |= list.incomplete;
                        if n.display == NavDisplay::Tree {
                            n.directories.insert(path, list.entries);
                        } else if n.directory == path {
                            n.entries = list.entries;
                            n.incomplete = list.incomplete;
                            n.selected = 0;
                        }
                    }
                    Err(error) => {
                        if n.display == NavDisplay::List && n.directory == path {
                            n.entries.clear();
                        } else {
                            n.expanded.remove(&path);
                        }
                        self.notice = error;
                    }
                    _ => self.notice = "Invalid directory preview reply".into(),
                }
            }
            Job::Read { path, token, .. } => {
                let key = FileKey::new(token.access.scope_id().clone(), path.clone());
                let text = match response.result {
                    Ok(Content::File {
                        path: resolved,
                        text,
                    }) if resolved == path => text,
                    Err(error) => format!("[file unavailable]\n{error}"),
                    _ => "[unexpected preview response]".into(),
                };
                while self.files.len() >= 8 {
                    if let Some(evicted) = self.files.keys().find(|other| **other != key).cloned() {
                        self.files.remove(&evicted);
                        self.file_lines.remove(&evicted);
                        self.file_highlights.remove(&evicted);
                    } else {
                        break;
                    }
                }
                self.file_highlights.remove(&key);
                self.file_lines.insert(
                    key.clone(),
                    Arc::new(
                        text.lines()
                            .take(8192)
                            .map(aw_workspace::editor::display)
                            .collect(),
                    ),
                );
                self.files.insert(key.clone(), text);
                self.file_revision = self.file_revision.wrapping_add(1);
                self.schedule_file_highlight(&key);
                self.refresh_file_find(&key);
            }
            Job::Search { pane, query, .. } => match pane {
                None => {
                    if self.palette_file_pending != Some(response.ticket)
                        || query != self.palette_file_query
                    {
                        return;
                    }
                    self.palette_file_pending = None;
                    match response.result {
                        Ok(Content::Search {
                            query: resolved,
                            entries,
                        }) if resolved == query => self.palette_file_results = entries,
                        Err(error) => {
                            self.palette_file_results.clear();
                            self.notice = error;
                        }
                        _ => {
                            self.palette_file_results.clear();
                            self.notice = "Invalid file-search reply".into();
                        }
                    }
                }
                Some(pane) => {
                    let state = self.nav.entry((pane, Navigator::Files)).or_default();
                    if state.search_pending != Some(response.ticket)
                        || state.filter.trim() != query
                        || state.search_query != query
                    {
                        return;
                    }
                    state.search_pending = None;
                    match response.result {
                        Ok(Content::Search {
                            query: resolved,
                            entries,
                        }) if resolved == query => state.search_results = entries,
                        Err(error) => {
                            state.search_results.clear();
                            self.notice = error;
                        }
                        _ => {
                            state.search_results.clear();
                            self.notice = "Invalid Files filter reply".into();
                        }
                    }
                }
            },
            Job::Authorize { .. } => unreachable!(),
        }
    }
    pub(super) fn apply_background_result(&mut self, result: UiBackgroundResult) {
        match result {
            UiBackgroundResult::AgentView(reply) => self.apply_agent_view_reply(reply),
            UiBackgroundResult::Terminal(result) => {
                self.terminal_pending = false;
                match result {
                    Ok(terminal) => {
                        self.open(Target::Terminal { terminal });
                        self.notice = "Trusted local shell ready. Ctrl+Shift+P/F1 opens workspace commands; child owns ordinary keys.".into();
                    }
                    Err(error) => self.notice = error,
                }
            }
            UiBackgroundResult::InitialLayout {
                base_generation,
                result,
            } => {
                self.layout_pending = false;
                if self.layout_generation != base_generation {
                    return;
                }
                match result {
                    Ok(Some(layout)) => {
                        self.reset_file_lifetime();
                        self.reset_git_lifetime();
                        self.container_last_group.clear();
                        self.layout = layout;
                        self.layout_generation = self.layout_generation.wrapping_add(1);
                        self.terminal_return = None;
                        self.reconcile_layout_state();
                        if !matches!(
                            self.layout
                                .active(self.layout.focused)
                                .map(|tab| &tab.target),
                            Some(Target::Conversation)
                        ) {
                            self.input = false;
                        }
                        self.notice =
                            "Saved layout restored asynchronously; Agent/session state unchanged"
                                .into();
                    }
                    Ok(None) => {}
                    Err(error) => {
                        self.notice = format!("Saved layout unavailable; using default: {error}");
                    }
                }
            }
            UiBackgroundResult::Layout {
                base_generation,
                result,
            } => {
                self.layout_pending = false;
                match result {
                    Ok(Some(layout)) => {
                        if self.layout_generation != base_generation {
                            self.notice = "Loaded layout ignored because the workspace changed while the load was running".into();
                            return;
                        }
                        self.reset_file_lifetime();
                        self.reset_git_lifetime();
                        self.container_last_group.clear();
                        self.layout = layout;
                        self.layout_generation = self.layout_generation.wrapping_add(1);
                        self.terminal_return = None;
                        self.reconcile_layout_state();
                        if !matches!(
                            self.layout
                                .active(self.layout.focused)
                                .map(|tab| &tab.target),
                            Some(Target::Conversation)
                        ) {
                            self.input = false;
                        }
                        self.notice = "Layout loaded; active session and drafts unchanged".into();
                    }
                    Ok(None) => {
                        self.notice =
                            "Layout saved; no account secrets, transcript or draft stored".into();
                    }
                    Err(error) => self.notice = error,
                }
            }
            UiBackgroundResult::Highlight {
                target,
                generation,
                start,
                lines,
                done,
            } => {
                let cache = match &target {
                    HighlightTarget::File(path) => self.file_highlights.get_mut(path),
                    HighlightTarget::Diff(review) => self.diff_highlights.get_mut(review),
                };
                let Some(cache) = cache else { return };
                if cache.generation != generation || cache.theme != self.ui_config.theme {
                    return;
                }
                for (offset, line) in lines.into_iter().enumerate() {
                    if let Some(slot) = cache.lines.get_mut(start + offset) {
                        *slot = Some(line);
                    }
                }
                cache.complete |= done;
                match target {
                    HighlightTarget::File(_) => {
                        self.file_revision = self.file_revision.wrapping_add(1);
                    }
                    HighlightTarget::Diff(_) => {
                        self.git.revision = self.git.revision.wrapping_add(1);
                    }
                }
                self.rendered.clear();
            }
            UiBackgroundResult::Conversation {
                generation,
                key,
                result,
            } => {
                let Some(pending) = self.conversation_pending.as_ref() else {
                    return;
                };
                if pending.generation != generation || !pending.key.matches(&key) {
                    return;
                }
                self.conversation_pending = None;
                let current = self.conversation_render_key(key.width);
                if !current.matches(&key) {
                    return;
                }
                match result {
                    Ok(lines) => {
                        self.conversation_cache = Some(ConversationCache {
                            key,
                            lines: Arc::new(lines),
                        });
                        self.rendered.clear();
                    }
                    Err(error) => self.notice = error,
                }
            }
            UiBackgroundResult::Find {
                tab,
                generation,
                query,
                matches,
                choosing,
                error,
            } => {
                if self
                    .find_cancel
                    .get(&tab)
                    .is_none_or(|cancel| cancel.load(Ordering::Acquire))
                {
                    return;
                }
                let current_query = self.find.get(&tab).map(|state| state.query.as_str());
                if current_query != Some(query.as_str())
                    || self.find_generation.get(&tab).copied() != Some(generation)
                {
                    return;
                }
                self.find_cancel.remove(&tab);
                self.find_generation.remove(&tab);
                if choosing {
                    let active = self.layout.active(self.layout.focused).map(|tab| tab.id);
                    if active == Some(tab) {
                        self.notice =
                            error.map_or_else(|| Self::find_notice(matches.len()), str::to_owned);
                    } else if self.notice == "Searching…"
                        && active.is_none_or(|active| !self.find_cancel.contains_key(&active))
                    {
                        self.notice.clear();
                    }
                }
                let selected = self.find.get(&tab).map_or(0, |state| {
                    state.selected.min(matches.len().saturating_sub(1))
                });
                self.find.insert(
                    tab,
                    FindState {
                        query,
                        matches: matches.clone(),
                        selected,
                        choosing: choosing && !matches.is_empty(),
                        committed: choosing,
                    },
                );
                if let Some(row) = matches.get(selected).copied() {
                    self.jump_to_row(tab, row);
                }
            }
        }
    }

    pub(super) fn visible_files(&mut self, browser: &Browser) {
        if let Some(palette) = &self.palette {
            let query = palette.text().to_owned();
            self.sync_palette_file_search(browser, &query);
        }

        if let Some(Pane::Navigator {
            kind: Navigator::Git,
        }) = self.layout.panes.get(&self.layout.focused)
        {
            self.git.ensure(self.layout.focused);
        }
        let files = self
            .layout
            .groups()
            .into_iter()
            .filter_map(|group| {
                self.layout.active(group).and_then(|tab| match &tab.target {
                    Target::File { scope_id, path } => {
                        Some((tab.id, FileKey::new(scope_id.clone(), path.clone())))
                    }
                    _ => None,
                })
            })
            .collect::<Vec<_>>();
        for (tab, key) in files {
            self.request_file_view(browser, tab, key.clone());
            if self.files.contains_key(&key) {
                if !self.file_highlights.contains_key(&key) {
                    self.schedule_file_highlight(&key);
                }
                if self.file_find_dirty.remove(&tab) {
                    self.refresh_file_find(&key);
                }
            }
        }
        if let Some(Pane::Navigator {
            kind: Navigator::Files,
        }) = self.layout.panes.get(&self.layout.focused)
        {
            let pane = self.layout.focused;
            let n = self.nav.entry((pane, Navigator::Files)).or_default();
            if !n.loaded && n.pending.is_none() {
                let path = n.directory.clone();
                self.list_files(browser, pane, path);
            }
        }
    }
}
