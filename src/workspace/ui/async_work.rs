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
            Job::Read { path } => Request::Read(path.clone()),
            Job::Search { query, cancel, .. } => Request::Search {
                query: query.clone(),
                limit: 80,
                cancel: cancel.clone(),
            },
        };
        browser.request(self.ticket, request)?;
        self.jobs.insert(self.ticket, job);
        Ok(self.ticket)
    }
    pub(super) fn list_files(&mut self, browser: &Browser, pane: Id, path: String) {
        if self
            .jobs
            .values()
            .any(|job| matches!(job, Job::List { pane: p, path: q } if *p == pane && *q == path))
        {
            return;
        }
        match self.file_request(
            browser,
            Job::List {
                pane,
                path: path.clone(),
            },
        ) {
            Ok(ticket) => {
                let n = self.nav.entry((pane, Navigator::Files)).or_default();
                n.pending = Some(ticket);
                if n.display == NavDisplay::List {
                    n.directory = path;
                    n.entries.clear();
                    n.loaded = false;
                } else {
                    if path.is_empty() {
                        n.loaded = false;
                    } else {
                        n.expanded.insert(path);
                    }
                }
            }
            Err(e) => self.notice = e.to_string(),
        }
    }
    pub(super) fn read_file(&mut self, browser: &Browser, path: String) {
        if !aw_workspace::valid_relative(&path) {
            self.notice = "Use a clean relative file path under this workspace root.".into();
            return;
        }
        self.open_file_preview(path.clone());
        if !self.files.contains_key(&path)
            && !self
                .jobs
                .values()
                .any(|j| matches!(j,Job::Read{path:p} if *p==path))
        {
            if let Err(e) = self.file_request(browser, Job::Read { path }) {
                self.notice = e.to_string();
            }
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
        if self.palette_file_query == query
            && (self.palette_file_pending.is_some() || !self.palette_file_results.is_empty())
        {
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
        match self.file_request(
            browser,
            Job::Search {
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
        match self.file_request(
            browser,
            Job::Search {
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

    pub(super) fn next_highlight_generation(&mut self) -> u64 {
        self.highlight_generation = self.highlight_generation.wrapping_add(1).max(1);
        self.highlight_generation
    }

    pub(super) fn schedule_file_highlight(&mut self, path: &str) {
        let Some(text) = self.files.get(path).cloned() else { return };
        let generation = self.next_highlight_generation();
        let theme = self.ui_config.theme;
        let total = text.split('\n').take(8192).count();
        let cancel = Arc::new(AtomicBool::new(false));
        self.file_highlights.insert(
            path.to_owned(),
            HighlightCache {
                generation,
                theme,
                lines: vec![None; total],
                complete: false,
                cancel: cancel.clone(),
            },
        );
        let target = HighlightTarget::File(path.to_owned());
        let path = path.to_owned();
        let path_for_job = path.clone();
        let out = self.background_tx.clone();
        let panic_out = out.clone();
        let panic_target = target.clone();
        if let Err(error) = self.background.try_submit_with_panic("syntax", move || {
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
        }, move || {
            let _ = panic_out.blocking_send(UiBackgroundResult::Highlight {
                target: panic_target,
                generation,
                start: 0,
                lines: Vec::new(),
                done: true,
            });
        }) {
            if let Some(cache) = self.file_highlights.get_mut(&path) {
                cache.complete = true;
            }
            self.notice = error.to_string();
        }
    }

    pub(super) fn schedule_diff_highlight(&mut self, review_id: &str) {
        self.diff_highlights
            .retain(|id, _| self.git.reviews.contains_key(id));
        let Some((_, review)) = self.git.reviews.get(review_id).cloned() else { return };
        let generation = self.next_highlight_generation();
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
        if let Err(error) = self.background.try_submit_with_panic("syntax", move || {
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
        }, move || {
            let _ = panic_out.blocking_send(UiBackgroundResult::Highlight {
                target: panic_target,
                generation,
                start: 0,
                lines: Vec::new(),
                done: true,
            });
        }) {
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
        match job {
            Job::List { pane, path } => {
                if !self.layout.panes.contains_key(&pane) {
                    return;
                }
                let n = self.nav.entry((pane, Navigator::Files)).or_default();
                if n.pending == Some(response.ticket) {
                    n.pending = None;
                }
                match response.result {
                    Ok(Content::Directory(list)) => {
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
                    Err(e) => {
                        if n.display == NavDisplay::List && n.directory == path {
                            n.entries.clear();
                        } else {
                            n.expanded.remove(&path);
                        }
                        self.notice = e;
                    }
                    _ => {
                        self.notice = "Invalid directory preview reply".into();
                    }
                }
            }
            Job::Read { path } => {
                let text = match response.result {
                    Ok(Content::File {
                        path: resolved,
                        text,
                    }) if resolved == path => text,
                    Err(e) => format!("[file unavailable]\n{e}"),
                    _ => "[unexpected preview response]".into(),
                };
                // Bound cache independently from on-disk/source limits. Explicit
                // reopen/refresh reloads an evicted view, never an agent call.
                while self.files.len() >= 8 {
                    if let Some(key) = self.files.keys().find(|k| **k != path).cloned() {
                        self.files.remove(&key);
                        self.file_lines.remove(&key);
                        self.file_highlights.remove(&key);
                    } else {
                        break;
                    }
                }
                self.file_highlights.remove(&path);
                let lines = Arc::new(
                    text.lines()
                        .take(8192)
                        .map(aw_workspace::editor::display)
                        .collect::<Vec<_>>(),
                );
                self.file_lines.insert(path.clone(), lines);
                self.files.insert(path.clone(), text);
                self.file_revision = self.file_revision.wrapping_add(1);
                self.schedule_file_highlight(&path);
            }
            Job::Search { pane, query, .. } => match pane {
                None => {
                    if self.palette_file_pending == Some(response.ticket) {
                        self.palette_file_pending = None;
                    }
                    if query != self.palette_file_query {
                        return;
                    }
                    match response.result {
                        Ok(Content::Search {
                            query: resolved,
                            entries,
                        }) if resolved == query => {
                            self.palette_file_results = entries;
                        }
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
                    if state.search_pending == Some(response.ticket) {
                        state.search_pending = None;
                    }
                    if state.filter.trim() != query || state.search_query != query {
                        return;
                    }
                    match response.result {
                        Ok(Content::Search {
                            query: resolved,
                            entries,
                        }) if resolved == query => {
                            state.search_results = entries;
                        }
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
                        self.layout = layout;
                        self.layout_generation = self.layout_generation.wrapping_add(1);
                        self.terminal_return = None;
                        self.reconcile_layout_state();
                        if !matches!(
                            self.layout.active(self.layout.focused).map(|tab| &tab.target),
                            Some(Target::Conversation)
                        ) {
                            self.input = false;
                        }
                        self.notice = "Saved layout restored asynchronously; Agent/session state unchanged".into();
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
                        self.layout = layout;
                        self.layout_generation = self.layout_generation.wrapping_add(1);
                        self.terminal_return = None;
                        self.reconcile_layout_state();
                        if !matches!(
                            self.layout.active(self.layout.focused).map(|tab| &tab.target),
                            Some(Target::Conversation)
                        ) {
                            self.input = false;
                        }
                        self.notice = "Layout loaded; active session and drafts unchanged".into();
                    }
                    Ok(None) => {
                        self.notice = "Layout saved; no account secrets, transcript or draft stored".into();
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
                let Some(pending) = self.conversation_pending.as_ref() else { return };
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
                let selected = self
                    .find
                    .get(&tab)
                    .map_or(0, |state| state.selected.min(matches.len().saturating_sub(1)));
                self.find.insert(
                    tab,
                    FindState {
                        query,
                        matches: matches.clone(),
                        selected,
                        choosing: choosing && !matches.is_empty(),
                    },
                );
                if let Some(row) = matches.get(selected).copied() {
                    self.jump_to_row(tab, row);
                }
            }
        }
    }

    pub(super) fn visible_files(&mut self, browser: &Browser) {
        if let Some(Pane::Navigator {
            kind: Navigator::Git,
        }) = self.layout.panes.get(&self.layout.focused)
        {
            self.git.ensure(self.layout.focused);
        }
        let active = self.layout.active(self.group()).map(|t| t.target.clone());
        if let Some(Target::File { path }) = active {
            if !self.files.contains_key(&path)
                && !self
                    .jobs
                    .values()
                    .any(|j| matches!(j,Job::Read{path:p} if *p==path))
            {
                if let Err(e) = self.file_request(browser, Job::Read { path }) {
                    self.notice = e.to_string();
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
