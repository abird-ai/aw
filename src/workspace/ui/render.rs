//! Workspace presentation and projection for the existing `Ui` state.
//! Owns visible rows, rendering, hit geometry and demand-driven presentation work.
use super::*;

impl Ui {
    pub(super) fn conversation_lines(&self, width: u16) -> Vec<String> {
        crate::tui::markdown::plain_lines(&self.conversation_view_lines(width))
    }

    // The tab owns the document title. View metadata retains location and
    // authority context, and is shared with background find projection.
    pub(super) fn file_context(path: &str) -> String {
        let directory = path
            .rsplit_once('/')
            .map_or(".", |(directory, _)| directory);
        format!(
            "{} · read only; not model context",
            aw_workspace::editor::display(directory)
        )
    }

    pub(super) fn file_key_context(&self, key: &FileKey) -> String {
        format!(
            "{} · {}",
            self.file_scope_label(&key.scope_id),
            Self::file_context(&key.path)
        )
    }

    pub(super) fn diff_context(path: &str, basis: &str, reviewed: bool) -> String {
        let directory = path
            .rsplit_once('/')
            .map_or(".", |(directory, _)| directory);
        format!(
            "{} · {basis}{}",
            aw_workspace::editor::display(directory),
            if reviewed { "  [REVIEWED]" } else { "" }
        )
    }

    pub(super) fn file_line_count(&self, key: &FileKey) -> usize {
        2 + self.file_lines.get(key).map_or(1, |lines| lines.len())
    }

    pub(super) fn file_view_slice(
        &self,
        key: &FileKey,
        start: usize,
        height: usize,
    ) -> Vec<Line<'static>> {
        let palette = crate::tui::theme::palette_for(&self.ui_config);
        let count = self.file_line_count(key);
        let end = start.saturating_add(height).min(count);
        let highlighted = self.file_highlights.get(key);
        let source = self.file_lines.get(key);
        (start..end)
            .map(|row| match row {
                0 => Line::styled(
                    self.file_key_context(key),
                    Style::default().fg(palette.muted),
                ),
                1 => Line::default(),
                source_row => {
                    let index = source_row - 2;
                    highlighted
                        .and_then(|cache| cache.lines.get(index))
                        .and_then(|line| line.clone())
                        .or_else(|| {
                            source
                                .and_then(|lines| lines.get(index))
                                .map(|line| Line::raw(line.clone()))
                        })
                        .unwrap_or_else(|| {
                            Line::raw(if source.is_some() {
                                String::new()
                            } else {
                                if self.file_scopes.access(&key.scope_id).is_some() {
                                    "Loading explicit local file preview…".into()
                                } else {
                                    "Folder unavailable. Choose Reconnect Files folder….".into()
                                }
                            })
                        })
                }
            })
            .collect()
    }

    pub(super) fn file_view_lines(&self, key: &FileKey) -> Vec<Line<'static>> {
        self.file_view_slice(key, 0, self.file_line_count(key))
    }

    pub(super) fn diff_line_count(&self, review_id: &str) -> usize {
        let Some((_, review)) = self.git.reviews.get(review_id) else {
            return 1;
        };
        let comments = self
            .git
            .comments
            .get(review_id)
            .map_or(0, |items| items.len() * 2);
        2 + review.lines.len() + comments + usize::from(review.truncated_display)
    }

    pub(super) fn diff_view_row(&self, review_id: &str, row: usize) -> Line<'static> {
        let palette = crate::tui::theme::palette_for(&self.ui_config);
        let Some((_, review)) = self.git.reviews.get(review_id) else {
            return Line::styled(
                "Review no longer cached. Reopen from Git; no stale patch is applied automatically.",
                Style::default().fg(palette.muted),
            );
        };
        if row == 0 {
            return Line::styled(
                Self::diff_context(
                    &review.path_label,
                    &review.basis,
                    self.git.reviewed.contains(review_id),
                ),
                Style::default().fg(palette.muted),
            );
        }
        if row == 1 {
            return Line::styled(
                format!("Snapshot {}", review.id),
                Style::default().fg(palette.muted),
            );
        }
        let source_index = row - 2;
        if let Some(line) = review.lines.get(source_index) {
            let old = line.old.map(|value| value.to_string()).unwrap_or_default();
            let new = line.new.map(|value| value.to_string()).unwrap_or_default();
            let prefix = format!("{:>4} {:>6} {:>6} ", source_index + 1, old, new);
            return match line.kind {
                aw_git::LineKind::Added | aw_git::LineKind::Removed | aw_git::LineKind::Context => {
                    let mut spans = vec![Span::styled(prefix, Style::default().fg(palette.muted))];
                    let code_line = self
                        .diff_highlights
                        .get(review_id)
                        .and_then(|cache| cache.lines.get(source_index))
                        .and_then(|line| line.clone())
                        .unwrap_or_else(|| syntax::plain_diff_line(line, self.ui_config.theme));
                    spans.extend(code_line.spans);
                    Line::from(spans)
                }
                aw_git::LineKind::Hunk => Line::from(vec![
                    Span::styled(prefix, Style::default().fg(palette.muted)),
                    Span::styled(
                        aw_workspace::editor::display(&line.text),
                        Style::default()
                            .fg(palette.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                aw_git::LineKind::Header => Line::from(vec![
                    Span::styled(prefix, Style::default().fg(palette.muted)),
                    Span::styled(
                        aw_workspace::editor::display(&line.text),
                        Style::default().fg(palette.accent),
                    ),
                ]),
                aw_git::LineKind::Notice => Line::from(vec![
                    Span::styled(prefix, Style::default().fg(palette.muted)),
                    Span::styled(
                        aw_workspace::editor::display(&line.text),
                        Style::default()
                            .fg(palette.muted)
                            .add_modifier(Modifier::ITALIC),
                    ),
                ]),
            };
        }
        let mut tail = source_index - review.lines.len();
        if let Some(comments) = self.git.comments.get(review_id) {
            let comment_rows = comments.len() * 2;
            if tail < comment_rows {
                if tail.is_multiple_of(2) {
                    return Line::default();
                }
                let comment = &comments[tail / 2];
                return Line::styled(
                    format!("COMMENT row {}: {}", comment.row + 1, comment.text),
                    Style::default()
                        .fg(palette.warning)
                        .add_modifier(Modifier::BOLD),
                );
            }
            tail -= comment_rows;
        }
        if review.truncated_display && tail == 0 {
            return Line::styled(
                "[display bounded; raw review patch retained separately]",
                Style::default().fg(palette.muted),
            );
        }
        Line::default()
    }

    pub(super) fn diff_view_slice(
        &self,
        review_id: &str,
        start: usize,
        height: usize,
    ) -> Vec<Line<'static>> {
        let end = start
            .saturating_add(height)
            .min(self.diff_line_count(review_id));
        (start..end)
            .map(|row| self.diff_view_row(review_id, row))
            .collect()
    }

    pub(super) fn decorate_find_line(
        &self,
        tab: Id,
        row: usize,
        mut line: Line<'static>,
    ) -> Line<'static> {
        const LABELS: &str = "asdfghjklqwertyuiopzxcvbnm1234567890";
        let palette = crate::tui::theme::palette_for(&self.ui_config);
        let Some(state) = self.find.get(&tab) else {
            return line;
        };
        let Some(match_index) = state.matches.iter().position(|candidate| *candidate == row) else {
            return line;
        };
        let selected = match_index == state.selected;
        let background = if selected {
            palette.selection_background
        } else {
            palette.surface
        };
        for span in &mut line.spans {
            span.style = span.style.bg(background);
        }
        if state.choosing {
            if let Some(label) = LABELS.chars().nth(match_index) {
                line.spans.insert(
                    0,
                    Span::styled(format!("[{label}] "), palette.selected_style()),
                );
            }
        }
        line
    }

    pub(super) fn conversation_render_key(&self, width: u16) -> ConversationRenderKey {
        ConversationRenderKey {
            turns: self.snapshot.turns.clone(),
            resources: self.snapshot.loaded_resources.clone(),
            ui: self.ui_config.clone(),
            width,
        }
    }

    pub(in crate::workspace) fn schedule_visible_background_work(&mut self) {
        self.schedule_agent_views();
        let Some(tab) = self.layout.active(self.layout.focused).cloned() else {
            return;
        };
        if !matches!(tab.target, Target::Conversation) {
            return;
        }
        let Some(view) = self.selectable_views.get(&tab.id) else {
            return;
        };
        let width = view.area.width;
        let key = self.conversation_render_key(width);
        if self
            .conversation_cache
            .as_ref()
            .is_some_and(|cache| cache.key.matches(&key))
            || self.conversation_pending.is_some()
        {
            // Keep at most one expensive rich-transcript projection in flight.
            // A stale completion is ignored below; the next dirty frame then
            // schedules the newest key while plain shared-Agent geometry stays visible.
            return;
        }
        self.conversation_generation = self.conversation_generation.wrapping_add(1).max(1);
        let generation = self.conversation_generation;
        let turns = key.turns.clone();
        let resources = key.resources.clone();
        let ui = key.ui.clone();
        let result_key = key.clone();
        let performance = self.snapshot.performance.clone();
        let out = self.background_tx.clone();
        let panic_out = out.clone();
        let panic_key = key.clone();
        self.conversation_pending = Some(ConversationPending { generation, key });
        if let Err(error) = self.background.try_submit_with_panic(
            "syntax",
            move || {
                let lines = super::super::agent_view::settled_lines(
                    &turns,
                    &resources,
                    &ui,
                    performance.as_ref(),
                    result_key.width,
                    true,
                );
                let _ = out.blocking_send(UiBackgroundResult::Conversation {
                    generation,
                    key: result_key,
                    result: Ok(lines),
                });
            },
            move || {
                let _ = panic_out.blocking_send(UiBackgroundResult::Conversation {
                    generation,
                    key: panic_key,
                    result: Err("Agent transcript background projection panicked; plain projection retained".into()),
                });
            },
        ) {
            self.conversation_pending = None;
            self.notice = error.to_string();
        }
    }

    pub(super) fn conversation_view_lines(&self, width: u16) -> Vec<Line<'static>> {
        let key = self.conversation_render_key(width);
        if self
            .conversation_cache
            .as_ref()
            .is_some_and(|cache| cache.key.matches(&key))
            && self.snapshot.auxiliary_output.is_none()
            && self.snapshot.pending_prompt.is_empty()
        {
            return self
                .conversation_cache
                .as_ref()
                .expect("matching conversation cache checked above")
                .lines
                .as_ref()
                .clone();
        }

        // Live/pending/BTW projection has its own typed Agent-view seam; the
        // background cache owns only settled rich transcript projection.
        let mut lines =
            super::super::agent_view::lines(&self.snapshot, &self.ui_config, width, false);
        if let Some(cache) = self
            .conversation_cache
            .as_ref()
            .filter(|cache| cache.key.matches(&key))
        {
            let settled_len = super::super::agent_view::settled_lines(
                &self.snapshot.turns,
                &self.snapshot.loaded_resources,
                &self.ui_config,
                self.snapshot.performance.as_ref(),
                width,
                false,
            )
            .len();
            if lines.len() >= settled_len {
                let tail = lines.split_off(settled_len);
                lines = cache.lines.as_ref().clone();
                lines.extend(tail);
            }
        }
        lines
    }

    pub(super) fn content_lines(&self, target: &Target, width: u16) -> Vec<String> {
        let text=match target{
            Target::Conversation => return self.conversation_lines(width),
            Target::AgentView { .. } => return vec!["Read-only view; use its exact tab projection".into()],
            Target::ContextFlow=>if self.snapshot.context_output.is_empty(){"Use context-action {JSON} in the palette. Start with {\"op\":\"sources\"} or {\"op\":\"target\"}. Prepare/generate/preview/apply are separate. See .agents/docs/CONTEXT-FLOW-IMPLEMENTATION.md. No context action is inferred from navigation.".into()}else{self.snapshot.context_output.clone()},
            Target::SessionTree=>if self.snapshot.tree.is_empty(){"No tree loaded. Use /tree or the command palette. Only safe completed boundaries can fork.\nThe session tree is not a Git tree; inspecting it never executes recorded tools.".into()}else{self.snapshot.tree.clone()},
            Target::Activity=>{
                let mut rows=self.snapshot.activity.iter().cloned().collect::<Vec<_>>();
                if let Some(rail)=&self.snapshot.event_rail {
                    if !rail.notices().is_empty() {
                        rows.push("".into());
                        for notice in rail.notices() {
                            let text=notice.lines.iter().flat_map(|row|&row.spans)
                                .map(|span|aw::presentation::safe_text(&span.text))
                                .collect::<Vec<_>>().join("");
                            if !text.is_empty() {rows.push(format!("• {text}"));}
                        }
                    }
                }
                if rows.is_empty(){"No live activity yet. This view observes native events; it does not create a run.".into()}
                else{rows.join("\n")}
            },
            Target::File{scope_id,path}=>format!("File: {} — read only; not model context\n\n{}",path,self.files.get(&FileKey::new(scope_id.clone(),path.clone())).map(String::as_str).unwrap_or("Loading explicit local file preview…")),
            Target::Help=>format!("{}\n{}\nExplicit human browsing root: {}",WORKSPACE_KEYMAP,HELP,self.root.display()),
            Target::Diff{review}=>match self.git.reviews.get(review){Some((_,r))=>{let mut text=format!("{} · {} · {}{}\nSnapshot {}\n",r.root.display(),r.path_label,r.basis,if self.git.reviewed.contains(review){" [REVIEWED]"}else{""},r.id);for (i,line) in r.lines.iter().enumerate(){text.push_str(&format!("{:>4} {:>6} {:>6} {}\n",i+1,line.old.map(|n|n.to_string()).unwrap_or_default(),line.new.map(|n|n.to_string()).unwrap_or_default(),line.text));}if let Some(comments)=self.git.comments.get(review){for c in comments{text.push_str(&format!("\nCOMMENT row {}: {}\n",c.row+1,c.text));}}
                if r.truncated_display{text.push_str("[display bounded; raw review patch retained separately]\n");}text},None=>"Review no longer cached. Reopen from Git; no stale patch is applied automatically.".into()},
            Target::Terminal{..}=>"Terminal renderer unavailable; use terminal-start for an explicit new process".into(),
            Target::Settings=>"Settings are rendered as a native interactive tab.".into(),
        };
        // The full original transcript remains available through history/export.
        // Bound work per paint; explicit tail clipping is visible to the reader.
        let text = if matches!(target, Target::Conversation) && text.len() > 64 * 1024 {
            format!(
                "[display tail; use /history or export for full records]\n{}",
                aw::presentation::tail_text(&text, 64 * 1024)
            )
        } else {
            text
        };
        if matches!(
            target,
            Target::File { .. } | Target::SessionTree | Target::Diff { .. }
        ) {
            text.lines()
                .take(8193)
                .map(aw_workspace::editor::display)
                .collect()
        } else {
            wrap(&text, width.max(1))
        }
    }
    pub(super) fn rendered_lines(
        &mut self,
        id: Id,
        target: &Target,
        width: u16,
    ) -> Arc<Vec<String>> {
        if matches!(target, Target::AgentView { .. }) {
            return self.agent_view_plain(id);
        }
        let revision = match target {
            Target::File { .. } => self.file_revision,
            Target::Diff { .. } => self.git.revision,
            Target::Settings | Target::Help => 0,
            _ => self.snapshot.generation,
        };
        if let Some(c) = self.rendered.get(&id) {
            if c.target == *target && c.width == width && c.revision == revision {
                return c.lines.clone();
            }
        }
        let mut lines = if matches!(target, Target::Conversation) {
            crate::tui::markdown::plain_lines(&self.conversation_view_lines(width))
        } else {
            self.content_lines(target, width)
        };
        if lines.len() > 8192 {
            lines.truncate(8192);
            lines.push("[View limited to 8192 rows; source/history is not modified.]".into());
        }
        while self.rendered.len() >= 8 {
            if let Some(old) = self.rendered.keys().find(|key| **key != id).copied() {
                self.rendered.remove(&old);
            } else {
                break;
            }
        }
        let lines = Arc::new(lines);
        self.rendered.insert(
            id,
            Rendered {
                target: target.clone(),
                width,
                revision,
                lines: lines.clone(),
            },
        );
        lines
    }
    pub(super) fn tab_label(&self, target: &Target, _pinned: bool) -> String {
        let raw = match target {
            Target::Conversation => self
                .snapshot
                .session_title
                .as_deref()
                .map(|title| format!("Agent: {title}"))
                .unwrap_or_else(|| "Agent".into()),
            Target::AgentView {
                store_id,
                session_id,
                branch_id,
            } => self
                .agent_views
                .values()
                .find(|view| {
                    view.target.store_id == *store_id
                        && view.target.session_id == *session_id
                        && view.target.branch_id == *branch_id
                })
                .and_then(|view| view.cache.as_ref())
                .map(|cache| format!("View: {}", cache.title))
                .unwrap_or_else(|| format!("View: {session_id} · {branch_id}")),
            Target::SessionTree => "Session tree".into(),
            Target::ContextFlow => "Context".into(),
            Target::Activity => "Activity".into(),
            Target::File { path, .. } => path.rsplit('/').next().unwrap_or(path).to_owned(),
            Target::Diff { review } => {
                let path = self
                    .git
                    .reviews
                    .get(review)
                    .map(|(_, review)| review.path_label.as_str())
                    .unwrap_or(review);
                format!("Diff: {}", path.rsplit('/').next().unwrap_or(path))
            }
            Target::Terminal { terminal } => format!(
                "Terminal {}",
                terminal
                    .chars()
                    .rev()
                    .take(5)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            ),
            Target::Settings => "Settings".into(),
            Target::Help => "Help".into(),
        };
        truncate_cells(&raw, 24)
    }

    pub(super) fn tab_color(&self, target: &Target) -> Color {
        let palette = crate::tui::theme::palette_for(&self.ui_config);
        match target {
            Target::Conversation => palette.success,
            Target::AgentView { .. } | Target::File { .. }
            | Target::Activity | Target::Settings => palette.accent,
            Target::Diff { .. } | Target::Terminal { .. }
            | Target::ContextFlow => palette.warning,
            Target::SessionTree | Target::Help => palette.muted,
        }
    }

    pub(super) fn tab_dirty(&self, target: &Target) -> bool {
        match target {
            Target::Conversation => !self.draft.is_empty() || !self.recovery.is_empty(),
            Target::Diff { review } => self
                .git
                .comments
                .get(review)
                .is_some_and(|comments| !comments.is_empty()),
            Target::Activity => self.snapshot.busy || !self.snapshot.live_tools.is_empty(),
            _ => false,
        }
    }

    pub(super) fn agent_family(
        &self,
    ) -> Option<(Option<String>, Vec<&crate::workspace::owner::SavedSubagent>)> {
        let current = self.snapshot.session.as_deref()?;
        let parent = self
            .snapshot
            .subagents
            .iter()
            .find(|item| item.child_session == current)
            .map(|item| item.parent_session.clone());
        let children = self
            .snapshot
            .subagents
            .iter()
            .filter(|item| item.parent_session == current)
            .collect::<Vec<_>>();
        (parent.is_some() || !children.is_empty()).then_some((parent, children))
    }

    pub(super) fn session_title(&self, session: &str) -> String {
        self.snapshot
            .sessions
            .iter()
            .find(|item| item.id == session)
            .map(|item| item.title.clone())
            .unwrap_or_else(|| session.to_owned())
    }

    pub(super) fn draw_agent_sidecar(&mut self, frame: &mut Frame<'_>, area: Rect) {
        if area.width < 36 || area.height < 4 {
            return;
        }
        let Some((parent, children)) = self.agent_family() else {
            return;
        };
        let current = self.snapshot.session.as_deref();
        let palette = crate::tui::theme::palette_for(&self.ui_config);
        let mut rows = Vec::<(String, Line<'static>)>::new();

        if let Some(parent) = parent {
            let title = self.session_title(&parent);
            rows.push((
                parent.clone(),
                Line::from(vec![
                    Span::styled("↑ ", Style::default().fg(palette.muted)),
                    Span::styled(
                        truncate_cells(&title, 28),
                        Style::default().fg(palette.foreground),
                    ),
                ]),
            ));
        }

        for child in children.into_iter().take(6) {
            let title = self.session_title(&child.child_session);
            let (icon, color) = match child.state.as_str() {
                "running" => ("●", palette.accent),
                "completed" => ("✓", palette.success),
                "failed" | "interrupted" => ("!", palette.danger),
                "cancelled" => ("×", palette.muted),
                _ => ("○", palette.muted),
            };
            let current_mark = if current == Some(child.child_session.as_str()) {
                "› "
            } else {
                "  "
            };
            rows.push((
                child.child_session.clone(),
                Line::from(vec![
                    Span::styled(format!("{current_mark}{icon} "), Style::default().fg(color)),
                    Span::styled(
                        truncate_cells(
                            &if child.tool_mode == "off" {
                                title
                            } else {
                                format!("{} · isolated · {title}", child.tool_mode)
                            },
                            38,
                        ),
                        Style::default().fg(palette.foreground),
                    ),
                ]),
            ));
        }

        if rows.is_empty() {
            return;
        }
        let max_width = if self.snapshot.delegation_budget.is_some() {
            58
        } else {
            42
        };
        let width = area.width.saturating_sub(2).min(max_width);
        if width < 28 {
            return;
        }
        let height = (rows.len() as u16).saturating_add(2).min(area.height);
        let rect = Rect {
            x: area.x.saturating_add(area.width.saturating_sub(width)),
            y: area.y,
            width,
            height,
        };
        frame.render_widget(Clear, rect);
        let budget_title = self
            .snapshot
            .delegation_budget
            .as_ref()
            .map(|budget| {
                format!(
                    " Subagents {} {}/{} · active {}/{} · calls {}/{} ",
                    budget.state,
                    budget.children_created,
                    budget.max_children_created,
                    budget.active_children,
                    budget.max_active_children,
                    budget.model_attempts_started,
                    budget.max_model_attempts,
                )
            })
            .unwrap_or_else(|| " Subagents ".into());
        let block = Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(
                truncate_cells(&budget_title, usize::from(width.saturating_sub(2))),
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::BOLD),
            ))
            .style(palette.base_style())
            .border_style(Style::default().fg(palette.muted));
        let inner = block.inner(rect);
        frame.render_widget(block, rect);
        frame.render_widget(
            Paragraph::new(
                rows.iter()
                    .take(usize::from(inner.height))
                    .map(|(_, line)| line.clone())
                    .collect::<Vec<_>>(),
            ),
            inner,
        );
        for (index, (session, _)) in rows.iter().take(usize::from(inner.height)).enumerate() {
            self.hit(
                Rect {
                    x: inner.x,
                    y: inner.y.saturating_add(index as u16),
                    width: inner.width,
                    height: 1,
                },
                Hit::AgentSession {
                    session: session.clone(),
                },
            );
        }
    }

    pub(super) fn open_settings(&mut self) {
        self.open(Target::Settings);
        self.settings.get_or_insert_with(SettingsState::default);
        self.input = false;
    }

    pub(super) fn text_point_for(&self, tab: Id, column: u16, row: u16) -> Option<TextPoint> {
        let view = self.selectable_views.get(&tab)?;
        if view.area.width == 0 || view.area.height == 0 || view.lines.is_empty() {
            return None;
        }
        let local_row = row
            .saturating_sub(view.area.y)
            .min(view.area.height.saturating_sub(1));
        let local_col = column
            .saturating_sub(view.area.x)
            .min(view.area.width.saturating_sub(1));
        let line_index = usize::from(local_row).min(view.lines.len().saturating_sub(1));
        let line = &view.lines[line_index];
        let width = UnicodeWidthStr::width(line.as_str());
        let col = view
            .horizontal
            .saturating_add(usize::from(local_col))
            .min(width);
        Some(TextPoint {
            line: view.start_line.saturating_add(line_index),
            col,
        })
    }

    pub(super) fn selection_text(&self) -> Option<String> {
        let selection = self.text_selection.as_ref()?;
        if self
            .layout
            .tabs
            .get(&selection.tab)
            .is_some_and(|tab| matches!(tab.target, Target::AgentView { .. }))
            && !self.agent_view_fresh(selection.tab)
        {
            return None;
        }
        let view = self.selectable_views.get(&selection.tab)?;
        let (first, last) = if selection.anchor <= selection.current {
            (selection.anchor, selection.current)
        } else {
            (selection.current, selection.anchor)
        };
        if first == last {
            return None;
        }
        if first.line < view.start_line
            || last.line >= view.start_line.saturating_add(view.lines.len())
        {
            return None;
        }
        let mut out = String::new();
        for line_no in first.line..=last.line {
            let line = &view.lines[line_no - view.start_line];
            let width = UnicodeWidthStr::width(line.as_str());
            let start = if line_no == first.line { first.col } else { 0 };
            let end = if line_no == last.line {
                last.col.saturating_add(1).min(width)
            } else {
                width
            };
            let (_, piece) = cell_slice(line, start.min(width), end.min(width));
            if line_no > first.line {
                out.push('\n');
            }
            out.push_str(&piece);
        }
        (!out.is_empty()).then_some(out)
    }

    pub(super) fn copy_selection(&mut self) -> bool {
        let Some(text) = self.selection_text() else {
            return false;
        };
        match copy_osc52(&text) {
            Ok(()) => {
                self.notice = format!("Copied {} characters", text.chars().count());
                true
            }
            Err(error) => {
                self.notice = format!("Clipboard copy failed: {error}");
                false
            }
        }
    }

    pub(super) fn draw_text_selection(&self, frame: &mut Frame<'_>, tab: Id, style: Style) {
        let Some(selection) = self
            .text_selection
            .as_ref()
            .filter(|selection| selection.tab == tab)
            .or_else(|| self.saved_selections.get(&tab))
        else {
            return;
        };
        let Some(view) = self.selectable_views.get(&tab) else {
            return;
        };
        let (first, last) = if selection.anchor <= selection.current {
            (selection.anchor, selection.current)
        } else {
            (selection.current, selection.anchor)
        };
        if first == last {
            return;
        }
        let visible_left = view.horizontal;
        let visible_right = visible_left.saturating_add(usize::from(view.area.width));
        for (offset, line) in view.lines.iter().enumerate() {
            let line_no = view.start_line.saturating_add(offset);
            if line_no < first.line || line_no > last.line {
                continue;
            }
            let width = UnicodeWidthStr::width(line.as_str());
            let selected_start = if line_no == first.line { first.col } else { 0 };
            let selected_end = if line_no == last.line {
                last.col.saturating_add(1).min(width)
            } else {
                width
            };
            let clipped_start = selected_start.max(visible_left).min(visible_right);
            let clipped_end = selected_end.max(visible_left).min(visible_right);
            if clipped_end <= clipped_start {
                continue;
            }
            let (actual_start, piece) = cell_slice(line, clipped_start, clipped_end);
            if piece.is_empty() {
                continue;
            }
            let x = view
                .area
                .x
                .saturating_add((actual_start.saturating_sub(visible_left)) as u16);
            let max_width = view
                .area
                .x
                .saturating_add(view.area.width)
                .saturating_sub(x);
            let width = (UnicodeWidthStr::width(piece.as_str()) as u16).min(max_width);
            if width == 0 {
                continue;
            }
            frame.render_widget(
                Paragraph::new(Span::styled(piece, style)),
                Rect {
                    x,
                    y: view.area.y.saturating_add(offset as u16),
                    width,
                    height: 1,
                },
            );
        }
    }

    pub(in crate::workspace) fn draw(&mut self, frame: &mut Frame<'_>) {
        self.can_submit = false;
        self.hit_regions.clear();
        self.selectable_views.clear();
        let palette = crate::tui::theme::palette_for(&self.ui_config);
        let area = frame.area();
        frame.render_widget(Block::default().style(palette.base_style()), area);
        if area.height < 4 || area.width < 8 {
            return;
        }
        let state = if self.snapshot.needs_inspection {
            "inspect"
        } else if self.snapshot.busy {
            "running"
        } else if self.snapshot.state_pending {
            "saving"
        } else {
            "ready"
        };
        let context = StatusContext {
            model: self.snapshot.model.clone(),
            profile: self.snapshot.profile.clone(),
            provider: self.snapshot.provider.clone(),
            effort: self.snapshot.effort.clone(),
            directory: self.snapshot.root.clone(),
            agent: self
                .snapshot
                .session_title
                .clone()
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| self.snapshot.branch.clone()),
            branch: self.snapshot.branch.clone(),
            tools: self.snapshot.tools.clone(),
            session: self
                .snapshot
                .session
                .clone()
                .unwrap_or_else(|| "memory".into()),
            state: state.into(),
            session_tps: self.snapshot.session_tps,
        };
        let status_lines = extensions::project_status(
            Slot::FullscreenStatus,
            &self.ui_config,
            &context,
            self.snapshot.performance.as_ref(),
            area.width,
        );
        // P6-8B: v2 regions are opt-in. No extra chrome, header, footer or
        // pane offset is introduced for the original default UiConfig.
        let below_status = extensions::project_status(
            Slot::BelowStatus,
            &self.ui_config,
            &context,
            None,
            area.width,
        );
        let below_rows = below_status.len().min(2) as u16;
        let status_rows = status_lines.len().min(6) as u16;
        let footer_rows = status_rows.saturating_add(below_rows).saturating_add(1);
        if area.height <= footer_rows.saturating_add(1) {
            return;
        }
        // At narrow widths preserve the existing workspace pane minimums.
        // Side regions render only if explicitly configured and wide enough;
        // they are not new invisible panes or a second toolbar.
        let side_width = if area.width >= 80 {
            (area.width / 5).clamp(16, 24)
        } else {
            0
        };
        let left_status = extensions::project_status(
            Slot::LeftStatus,
            &self.ui_config,
            &context,
            None,
            side_width,
        );
        let right_status = extensions::project_status(
            Slot::RightStatus,
            &self.ui_config,
            &context,
            None,
            side_width,
        );
        let left_width = if !left_status.is_empty() {
            side_width
        } else {
            0
        };
        let right_width = if !right_status.is_empty() {
            side_width
        } else {
            0
        };
        let body_height = area.height - footer_rows;
        let body = ModelRect {
            x: area.x + left_width,
            y: area.y,
            width: area.width - left_width - right_width,
            height: body_height,
        };
        let hide_explorer = self.explorer_placement == ExplorerPlacement::Hidden;
        let mut projected_layout = None;
        if hide_explorer {
            let mut layout = self.layout.clone();
            if let Some(navigator) = self.explorer_pane() {
                let _ = layout.apply(Action::ClosePane(navigator));
            }
            if layout.panes.contains_key(&self.layout.focused) {
                let _ = layout.apply(Action::Focus(self.layout.focused));
            } else if let Some(group) = layout.groups().first().copied() {
                let _ = layout.apply(Action::Focus(group));
            }
            projected_layout = Some(layout);
        }
        let geometry_layout = projected_layout.as_ref().unwrap_or(&self.layout);
        if let Ok(geometry) = geometry_layout.geometry(body, self.maximized) {
            let mut sizes = BTreeMap::<String, (bool, u16, u16)>::new();
            for item in &geometry.visible {
                if let Some(Target::Terminal { terminal }) =
                    self.layout.active(item.pane).map(|t| &t.target)
                {
                    let pane_area = Self::pane_content_area(
                        Rect {
                            x: item.area.x,
                            y: item.area.y,
                            width: item.area.width,
                            height: item.area.height,
                        },
                        &geometry.visible,
                    );
                    let cols = pane_area.width.min(300);
                    // One tab row and the terminal's own factual status row.
                    let rows = pane_area.height.saturating_sub(2).min(120);
                    if cols >= 4 && rows >= 2 {
                        let focused = item.pane == self.layout.focused;
                        if !sizes.contains_key(terminal) || focused {
                            sizes.insert(terminal.clone(), (focused, rows, cols));
                        }
                    }
                }
            }
            if let Some(pool) = &self.terminal_pool {
                for (id, (_, rows, cols)) in sizes {
                    if let Some(terminal) = pool.get(&id) {
                        let _ = pool.resize_local(&terminal.id, rows, cols);
                    }
                }
            }
            for item in &geometry.visible {
                let rect = Rect {
                    x: item.area.x,
                    y: item.area.y,
                    width: item.area.width,
                    height: item.area.height,
                };
                self.draw_pane(
                    frame,
                    item.pane,
                    Self::pane_content_area(rect, &geometry.visible),
                );
            }
            self.record_dividers(&geometry.visible);
            // The separator is exactly the hit-tested resize cell, never a
            // second perimeter stealing a neighboring content column/row.
            for region in &self.hit_regions {
                let Hit::Divider { axis, .. } = region.hit else {
                    continue;
                };
                let symbol = match axis {
                    Axis::Columns => "│",
                    Axis::Rows => "─",
                };
                for y in region.area.y..region.area.bottom() {
                    for x in region.area.x..region.area.right() {
                        frame.buffer_mut()[(x, y)]
                            .set_symbol(symbol)
                            .set_style(Style::default().fg(palette.faint));
                    }
                }
            }
            if left_width > 0 {
                frame.render_widget(
                    Paragraph::new(left_status.clone()).style(Style::default().fg(palette.muted)),
                    Rect {
                        x: area.x,
                        y: area.y,
                        width: left_width,
                        height: body_height,
                    },
                );
            }
            if right_width > 0 {
                frame.render_widget(
                    Paragraph::new(right_status.clone()).style(Style::default().fg(palette.muted)),
                    Rect {
                        x: area.x + area.width - right_width,
                        y: area.y,
                        width: right_width,
                        height: body_height,
                    },
                );
            }
            if below_rows > 0 {
                frame.render_widget(
                    Paragraph::new(
                        below_status
                            .iter()
                            .take(usize::from(below_rows))
                            .cloned()
                            .collect::<Vec<_>>(),
                    )
                    .style(Style::default().fg(palette.muted)),
                    Rect {
                        x: area.x,
                        y: area.y + body_height,
                        width: area.width,
                        height: below_rows,
                    },
                );
            }

            frame.render_widget(
                Paragraph::new(
                    status_lines
                        .iter()
                        .take(usize::from(status_rows))
                        .cloned()
                        .collect::<Vec<_>>(),
                ),
                Rect {
                    x: area.x,
                    y: area.y + area.height - footer_rows + below_rows,
                    width: area.width,
                    height: status_rows,
                },
            );

            let hint = if let Some(search) = &self.search {
                let prefix = match search.target {
                    SearchTarget::Navigator { .. } => "Filter",
                    SearchTarget::Main { .. } => "Find",
                };
                format!("{prefix} › {}", search.editor.text())
            } else if !self.notice.is_empty() {
                self.notice.clone()
            } else if self.snapshot.needs_inspection {
                "Inspection required before more model work · F1 for actions".into()
            } else if !self.snapshot.status.is_empty()
                && self.snapshot.status != "Workspace opened without a new model request."
            {
                self.snapshot.status.clone()
            } else {
                let focused = self
                    .layout
                    .active(self.layout.focused)
                    .map(|tab| &tab.target);
                match focused {
                    Some(Target::Conversation) => {
                        "? shortcuts · / commands · /minimal return to Agent".into()
                    }
                    Some(Target::Terminal { .. }) => {
                        "Ctrl+` return to workspace · F1 commands".into()
                    }
                    _ => "? shortcuts · F1 commands".into(),
                }
            };
            frame.render_widget(
                Paragraph::new(aw_workspace::editor::display(&hint))
                    .style(Style::default().fg(palette.muted)),
                Rect {
                    x: area.x,
                    y: area.y + area.height - 1,
                    width: area.width,
                    height: 1,
                },
            );
        }
        if self.palette.is_some() {
            self.draw_palette(frame, area);
        }
    }
    pub(super) fn draw_pane(&mut self, frame: &mut Frame<'_>, id: Id, area: Rect) {
        let Some(pane) = self.layout.panes.get(&id).cloned() else {
            return;
        };
        let focused = self.layout.focused == id;
        let palette = crate::tui::theme::palette_for(&self.ui_config);
        let inner = area;
        let surface = if matches!(pane, Pane::Navigator { .. }) {
            palette.base_style().bg(palette.surface)
        } else {
            palette.base_style()
        };
        frame.render_widget(Block::default().style(surface), area);
        self.hit(area, Hit::Pane(id));
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let control_width = inner.width.min(2);
        let header_area = Rect {
            width: inner.width - control_width,
            height: 1,
            ..inner
        };
        let control_area = Rect {
            x: header_area.right(),
            width: control_width,
            height: 1,
            ..inner
        };
        let control_style = Style::default().fg(if focused {
            palette.accent
        } else {
            palette.muted
        });
        frame.render_widget(
            Paragraph::new(if self.maximized { "❐ " } else { "⛶ " }).style(control_style),
            control_area,
        );
        self.hit(control_area, Hit::MaximizePane(id));
        match pane {
            Pane::Navigator { kind } => {
                let categories = Self::navigator_order();
                let labels = categories
                    .iter()
                    .enumerate()
                    .map(|(index, (candidate, label))| {
                        (
                            index as Id,
                            (*label).to_owned(),
                            self.navigator_color(*candidate),
                        )
                    })
                    .collect::<Vec<_>>();
                let active = categories
                    .iter()
                    .position(|(candidate, _)| *candidate == kind)
                    .map(|index| index as Id);
                let (nav_hits, _) =
                    super::super::tab_strip::draw(frame, header_area, &labels, active, palette);
                for (area, candidate) in nav_hits {
                    if let Some((kind, _)) = categories.get(candidate as usize) {
                        self.hit(
                            area,
                            Hit::NavigatorKind {
                                pane: id,
                                kind: *kind,
                            },
                        );
                    }
                }
                let nav_rows = 1;
                let mut rows = Rect {
                    y: inner.y.saturating_add(nav_rows),
                    height: inner.height.saturating_sub(nav_rows),
                    ..inner
                };
                if kind == Navigator::Git {
                    if rows.height > 0 {
                        let label = self
                            .layout
                            .git_context_for(id)
                            .map(|context| self.git_context_label(context))
                            .unwrap_or_else(|| "unavailable repository".into());
                        frame.render_widget(
                            Paragraph::new(label).style(Style::default().fg(palette.muted)),
                            Rect { height: 1, ..rows },
                        );
                        rows.y += 1;
                        rows.height = rows.height.saturating_sub(1);
                    }
                    let (mode, display, info, items, selected) = {
                        let nav = self.git.panes.entry(id).or_default();
                        let info = if nav.pending.is_some() {
                            "Working…".to_owned()
                        } else if !nav.notice.is_empty() {
                            nav.notice.clone()
                        } else if let Some(repo) = &nav.repo {
                            aw_git::escape_bytes(repo.root().as_os_str().as_encoded_bytes())
                        } else {
                            "Git repository not discovered yet".into()
                        };
                        (nav.mode, nav.display, info, nav.rows(), nav.selected)
                    };
                    if rows.height > 0 {
                        let mode_area = rows;
                        let display_toggle = (mode == git::Mode::Changes && mode_area.width >= 4)
                            .then(|| Rect {
                                x: mode_area.x + mode_area.width - 2,
                                y: mode_area.y,
                                width: 2,
                                height: 1,
                            });
                        let modes_area = Rect {
                            width: mode_area
                                .width
                                .saturating_sub(display_toggle.map_or(0, |area| area.width)),
                            ..mode_area
                        };
                        let modes = [
                            (git::Mode::Changes, "Changes"),
                            (git::Mode::Worktrees, "Worktrees"),
                            (git::Mode::Branches, "Branches"),
                        ];
                        let (mode_hits, consumed) = draw_segmented_control(
                            frame,
                            modes_area,
                            &modes,
                            mode,
                            palette.success,
                            palette.muted,
                        );
                        for (area, candidate) in mode_hits {
                            self.hit(
                                area,
                                Hit::GitMode {
                                    pane: id,
                                    mode: candidate,
                                },
                            );
                        }
                        if let Some(toggle) = display_toggle {
                            let next = if display == git::Display::Tree {
                                git::Display::List
                            } else {
                                git::Display::Tree
                            };
                            frame.render_widget(
                                Paragraph::new(format!(" {}", Self::git_display_icon(display)))
                                    .style(Style::default().fg(palette.muted)),
                                toggle,
                            );
                            self.hit(
                                toggle,
                                Hit::GitDisplay {
                                    pane: id,
                                    display: next,
                                },
                            );
                        }
                        rows.y = rows.y.saturating_add(consumed);
                        rows.height = rows.height.saturating_sub(consumed);
                    }
                    if rows.height > 0 {
                        frame.render_widget(
                            Paragraph::new(aw_workspace::editor::display(&info))
                                .style(Style::default().fg(palette.muted)),
                            Rect { height: 1, ..rows },
                        );
                        rows.y += 1;
                        rows.height = rows.height.saturating_sub(1);
                    }
                    let git_filter = self
                        .git
                        .panes
                        .get(&id)
                        .map(|nav| nav.filter.clone())
                        .unwrap_or_default();
                    if !git_filter.is_empty() && rows.height > 0 {
                        frame.render_widget(
                            Paragraph::new(format!("filter: {git_filter}"))
                                .style(Style::default().fg(palette.muted)),
                            Rect { height: 1, ..rows },
                        );
                        rows.y += 1;
                        rows.height = rows.height.saturating_sub(1);
                    }
                    let mut state = ListState::default();
                    if !items.is_empty() {
                        let selected = selected.min(items.len() - 1);
                        self.git.panes.entry(id).or_default().selected = selected;
                        state.select(Some(selected));
                    }
                    frame.render_stateful_widget(
                        List::new(
                            items
                                .iter()
                                .map(|row| {
                                    let style = match &row.item {
                                        git::Item::Heading(key) => {
                                            // Git state uses semantic palette roles in
                                            // System/Light/Dark, never fixed bright ANSI
                                            // colors that vanish on light terminals.
                                            let color = if key.contains("Conflicts") {
                                                palette.danger
                                            } else if key.contains("Staged") {
                                                palette.accent
                                            } else if key.contains("Untracked") {
                                                palette.success
                                            } else {
                                                palette.warning
                                            };
                                            Style::default().fg(color).add_modifier(Modifier::BOLD)
                                        }
                                        git::Item::Folder(_) => Style::default().fg(palette.accent),
                                        git::Item::Change(change) => {
                                            let color = match change.code {
                                                'A' | '?' => palette.success,
                                                'D' => palette.danger,
                                                'M' => palette.warning,
                                                'R' | 'C' => palette.accent,
                                                _ => palette.foreground,
                                            };
                                            Style::default().fg(color)
                                        }
                                        git::Item::Worktree(_) => {
                                            Style::default().fg(palette.accent)
                                        }
                                        git::Item::Branch(branch) => {
                                            if branch.current {
                                                Style::default()
                                                    .fg(palette.success)
                                                    .add_modifier(Modifier::BOLD)
                                            } else {
                                                Style::default().fg(palette.muted)
                                            }
                                        }
                                    };
                                    ListItem::new(row.label.clone()).style(style)
                                })
                                .collect::<Vec<_>>(),
                        )
                        .highlight_style(palette.selected_style()),
                        rows,
                        &mut state,
                    );
                    let visible_start = state.offset();
                    draw_scroll_indicator(
                        frame,
                        rows,
                        items.len(),
                        usize::from(rows.height),
                        visible_start,
                        palette,
                    );
                    for (offset, _) in items
                        .iter()
                        .skip(visible_start)
                        .take(usize::from(rows.height))
                        .enumerate()
                    {
                        self.hit(
                            Rect {
                                x: rows.x,
                                y: rows.y + offset as u16,
                                width: rows.width,
                                height: 1,
                            },
                            Hit::NavigatorRow {
                                pane: id,
                                row: visible_start + offset,
                            },
                        );
                    }
                    return;
                }

                self.nav.entry((id, kind)).or_default();

                if kind == Navigator::Files && rows.height > 0 {
                    let (display, directory, filter) = {
                        let state = self.nav.entry((id, Navigator::Files)).or_default();
                        (state.display, state.directory.clone(), state.filter.clone())
                    };
                    let toggle = Rect {
                        x: rows.x + rows.width.saturating_sub(2),
                        y: rows.y,
                        width: 2.min(rows.width),
                        height: 1,
                    };
                    let root = self
                        .layout
                        .files_scope_for(id)
                        .map(|scope| self.file_scope_label(scope))
                        .unwrap_or_else(|| "unavailable folder".into());
                    let cwd = if directory.is_empty() {
                        root
                    } else {
                        format!("{}/{}", root.trim_end_matches('/'), directory)
                    };
                    frame.render_widget(
                        Paragraph::new(cwd).style(Style::default().fg(palette.muted)),
                        Rect {
                            width: rows.width.saturating_sub(toggle.width),
                            height: 1,
                            ..rows
                        },
                    );
                    let next = if display == NavDisplay::Tree {
                        NavDisplay::List
                    } else {
                        NavDisplay::Tree
                    };
                    frame.render_widget(
                        Paragraph::new(format!(" {}", Self::nav_display_icon(display)))
                            .style(Style::default().fg(palette.muted)),
                        toggle,
                    );
                    self.hit(
                        toggle,
                        Hit::NavigatorDisplay {
                            pane: id,
                            display: next,
                        },
                    );
                    rows.y += 1;
                    rows.height = rows.height.saturating_sub(1);
                    if !filter.is_empty() && rows.height > 0 {
                        frame.render_widget(
                            Paragraph::new(format!("filter: {filter}"))
                                .style(Style::default().fg(palette.muted)),
                            Rect { height: 1, ..rows },
                        );
                        rows.y += 1;
                        rows.height = rows.height.saturating_sub(1);
                    }
                } else if kind == Navigator::Sessions && rows.height > 0 {
                    let (display, scope, filter, multi_select) = {
                        let state = self.nav.entry((id, Navigator::Sessions)).or_default();
                        (
                            state.display,
                            state.session_scope,
                            state.filter.clone(),
                            state.multi_select,
                        )
                    };
                    let toggle = Rect {
                        x: rows.x + rows.width.saturating_sub(2),
                        y: rows.y,
                        width: 2.min(rows.width),
                        height: 1,
                    };
                    let multi = (rows.width >= 4).then_some(Rect {
                        x: rows.x + rows.width.saturating_sub(4),
                        y: rows.y,
                        width: 2,
                        height: 1,
                    });
                    let scopes_area = Rect {
                        width: rows
                            .width
                            .saturating_sub(toggle.width)
                            .saturating_sub(multi.map_or(0, |area| area.width)),
                        ..rows
                    };
                    let scopes = [
                        (SessionScope::User, "User"),
                        (SessionScope::Autonomous, "Autonomous"),
                        (SessionScope::Archived, "Archived"),
                    ];
                    let (scope_hits, consumed) = draw_segmented_control(
                        frame,
                        scopes_area,
                        &scopes,
                        scope,
                        palette.accent,
                        palette.muted,
                    );
                    for (area, candidate) in scope_hits {
                        self.hit(
                            area,
                            Hit::SessionScope {
                                pane: id,
                                scope: candidate,
                            },
                        );
                    }
                    if let Some(multi) = multi {
                        frame.render_widget(
                            Paragraph::new(if multi_select { " ▣" } else { " □" }).style(
                                Style::default().fg(if multi_select {
                                    palette.accent
                                } else {
                                    palette.muted
                                }),
                            ),
                            multi,
                        );
                        self.hit(multi, Hit::SessionMultiSelect { pane: id });
                    }
                    let next = if display == NavDisplay::Tree {
                        NavDisplay::List
                    } else {
                        NavDisplay::Tree
                    };
                    frame.render_widget(
                        Paragraph::new(format!(" {}", Self::nav_display_icon(display)))
                            .style(Style::default().fg(palette.muted)),
                        toggle,
                    );
                    self.hit(
                        toggle,
                        Hit::NavigatorDisplay {
                            pane: id,
                            display: next,
                        },
                    );
                    rows.y = rows.y.saturating_add(consumed);
                    rows.height = rows.height.saturating_sub(consumed);
                    if rows.height > 0 {
                        frame.render_widget(
                            Paragraph::new(self.snapshot.root.clone())
                                .style(Style::default().fg(palette.muted)),
                            Rect { height: 1, ..rows },
                        );
                        rows.y += 1;
                        rows.height = rows.height.saturating_sub(1);
                    }
                    if !filter.is_empty() && rows.height > 0 {
                        frame.render_widget(
                            Paragraph::new(format!("filter: {filter}"))
                                .style(Style::default().fg(palette.muted)),
                            Rect { height: 1, ..rows },
                        );
                        rows.y += 1;
                        rows.height = rows.height.saturating_sub(1);
                    }
                }

                let items = if kind == Navigator::Files {
                    let state = self
                        .nav
                        .get(&(id, Navigator::Files))
                        .expect("files navigator state exists");
                    Self::file_rows(state)
                } else {
                    self.session_rows(id)
                };
                let selected = {
                    let state = self.nav.entry((id, kind)).or_default();
                    if items.is_empty() {
                        state.selected = 0;
                        None
                    } else {
                        state.selected = state.selected.min(items.len() - 1);
                        Some(state.selected)
                    }
                };
                let session_list = kind == Navigator::Sessions
                    && self
                        .nav
                        .get(&(id, Navigator::Sessions))
                        .is_some_and(|state| state.display == NavDisplay::List);
                if session_list {
                    let (multi_select, selected_sessions) = self
                        .nav
                        .get(&(id, Navigator::Sessions))
                        .map(|state| (state.multi_select, state.selected_sessions.clone()))
                        .unwrap_or_default();
                    let capacity = usize::from(rows.height / 2).max(1);
                    let visible_start = selected
                        .map(|selected| selected.saturating_sub(capacity.saturating_sub(1)))
                        .unwrap_or(0)
                        .min(items.len().saturating_sub(capacity));
                    let active_session = self.snapshot.session.clone();
                    let active_branch = self.snapshot.branch.clone();
                    let session_meta = self
                        .snapshot
                        .sessions
                        .iter()
                        .map(|session| {
                            (
                                session.id.clone(),
                                (
                                    session.updated_at,
                                    session.turn_count,
                                    session.title.clone(),
                                ),
                            )
                        })
                        .collect::<BTreeMap<_, _>>();
                    let completed = self
                        .snapshot
                        .subagents
                        .iter()
                        .filter(|relation| relation.state == "completed")
                        .map(|relation| relation.child_session.clone())
                        .collect::<BTreeSet<_>>();
                    let in_progress = self
                        .snapshot
                        .subagents
                        .iter()
                        .filter(|relation| {
                            matches!(relation.state.as_str(), "admitted" | "running")
                        })
                        .map(|relation| relation.child_session.clone())
                        .collect::<BTreeSet<_>>();

                    for (offset, row) in items.iter().skip(visible_start).take(capacity).enumerate()
                    {
                        let y = rows.y.saturating_add((offset as u16).saturating_mul(2));
                        if y >= rows.bottom() {
                            break;
                        }
                        let item_height = 2.min(rows.bottom().saturating_sub(y));
                        let rect = Rect {
                            x: rows.x,
                            y,
                            width: rows.width,
                            height: item_height,
                        };
                        let session_id = navigator_session_id(&row.item);
                        let checked =
                            session_id.is_some_and(|session| selected_sessions.contains(session));
                        let active = session_id.is_some_and(|session| {
                            active_session.as_deref() == Some(session)
                                && navigator_branch(&row.item)
                                    .is_none_or(|branch| branch == active_branch)
                        });
                        let prefix = if multi_select {
                            if checked {
                                "☑ "
                            } else {
                                "☐ "
                            }
                        } else if active {
                            "› "
                        } else {
                            "  "
                        };
                        let notification =
                            session_id.is_some_and(|session| completed.contains(session));
                        let running = session_id.is_some_and(|session| {
                            in_progress.contains(session)
                                || (active_session.as_deref() == Some(session)
                                    && self.snapshot.busy)
                        });
                        let marker_width = if running {
                            3
                        } else if notification {
                            1
                        } else {
                            0
                        };
                        let title_budget = usize::from(
                            rect.width
                                .saturating_sub(marker_width)
                                .saturating_sub(u16::from(marker_width > 0)),
                        );
                        let title = truncate_cells(
                            &format!("{prefix}{}", aw_workspace::editor::display(&row.label)),
                            title_budget,
                        );
                        let title_width = UnicodeWidthStr::width(title.as_str()) as u16;
                        let gap = rect
                            .width
                            .saturating_sub(title_width)
                            .saturating_sub(marker_width);
                        let selected_style = selected == Some(visible_start + offset);
                        let title_style = if selected_style {
                            palette.selected_style()
                        } else {
                            Style::default()
                                .fg(if matches!(row.item, NavigatorItem::UnsavedSession) {
                                    palette.muted
                                } else {
                                    palette.accent
                                })
                                .add_modifier(Modifier::BOLD)
                        };
                        let notify_style = if selected_style {
                            palette.selected_style()
                        } else {
                            Style::default().fg(palette.success)
                        };
                        let progress_elapsed = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default();
                        let mut first_spans = vec![
                            Span::styled(title, title_style),
                            Span::styled(" ".repeat(usize::from(gap)), title_style),
                        ];
                        if running {
                            let (a, b, c) =
                                crate::tui::agent_chrome::progress_indicator(progress_elapsed);
                            let active_style = if selected_style {
                                palette.selected_style()
                            } else {
                                Style::default().fg(palette.accent)
                            };
                            let inactive_style = if selected_style {
                                palette.selected_style()
                            } else {
                                Style::default().fg(palette.faint)
                            };
                            for glyph in [a, b, c] {
                                first_spans.push(Span::styled(
                                    glyph,
                                    if glyph == "▪" {
                                        active_style
                                    } else {
                                        inactive_style
                                    },
                                ));
                            }
                        } else if notification {
                            first_spans.push(Span::styled("●", notify_style));
                        }
                        let first = Line::from(first_spans);

                        let detail = match session_id.and_then(|session| session_meta.get(session))
                        {
                            Some((updated_at, turn_count, _)) => {
                                let branch = navigator_branch(&row.item).unwrap_or("main");
                                format!(
                                    "  {} · {} turn{} · {}",
                                    relative_age(*updated_at),
                                    turn_count,
                                    if *turn_count == 1 { "" } else { "s" },
                                    branch
                                )
                            }
                            None => "  unsaved Agent".into(),
                        };
                        let detail = truncate_cells(&detail, usize::from(rect.width));
                        let detail_style = if selected_style {
                            palette.selected_style()
                        } else {
                            Style::default().fg(palette.muted)
                        };
                        let lines = if item_height > 1 {
                            vec![first, Line::styled(detail, detail_style)]
                        } else {
                            vec![first]
                        };
                        frame.render_widget(Paragraph::new(lines), rect);
                        self.hit(
                            rect,
                            Hit::NavigatorRow {
                                pane: id,
                                row: visible_start + offset,
                            },
                        );
                    }
                    draw_scroll_indicator(
                        frame,
                        rows,
                        items.len().saturating_mul(2),
                        usize::from(rows.height),
                        visible_start.saturating_mul(2),
                        palette,
                    );
                } else {
                    let mut list_state = ListState::default();
                    list_state.select(selected);
                    frame.render_stateful_widget(
                        List::new(
                            items
                                .iter()
                                .map(|row| {
                                    let style = match &row.item {
                                        NavigatorItem::Parent => Style::default().fg(palette.muted),
                                        NavigatorItem::File(entry) => match entry.kind {
                                            EntryKind::Directory => Style::default()
                                                .fg(palette.accent)
                                                .add_modifier(Modifier::BOLD),
                                            EntryKind::File => {
                                                Style::default().fg(palette.foreground)
                                            }
                                            EntryKind::Unsupported => {
                                                Style::default().fg(palette.danger)
                                            }
                                        },
                                        NavigatorItem::UnsavedSession => {
                                            Style::default().fg(palette.muted)
                                        }
                                        NavigatorItem::SessionRoot { .. } => Style::default()
                                            .fg(palette.accent)
                                            .add_modifier(Modifier::BOLD),
                                        NavigatorItem::SessionBranch { .. } => {
                                            Style::default().fg(palette.accent)
                                        }
                                    };
                                    ListItem::new(aw_workspace::editor::display(&row.label))
                                        .style(style)
                                })
                                .collect::<Vec<_>>(),
                        )
                        .highlight_style(palette.selected_style()),
                        rows,
                        &mut list_state,
                    );
                    let visible_start = list_state.offset();
                    draw_scroll_indicator(
                        frame,
                        rows,
                        items.len(),
                        usize::from(rows.height),
                        visible_start,
                        palette,
                    );
                    for (offset, _) in items
                        .iter()
                        .skip(visible_start)
                        .take(usize::from(rows.height))
                        .enumerate()
                    {
                        self.hit(
                            Rect {
                                x: rows.x,
                                y: rows.y + offset as u16,
                                width: rows.width,
                                height: 1,
                            },
                            Hit::NavigatorRow {
                                pane: id,
                                row: visible_start + offset,
                            },
                        );
                    }
                }
            }
            Pane::Group { tabs, active } => {
                let labels = tabs
                    .iter()
                    .filter_map(|tab_id| {
                        self.layout.tabs.get(tab_id).map(|tab| {
                            let mut label = self.tab_label(&tab.target, tab.pinned);
                            if self.tab_dirty(&tab.target) {
                                label.push(' ');
                                label.push_str(TAB_DIRTY_ICON);
                            }
                            (*tab_id, label, self.tab_color(&tab.target))
                        })
                    })
                    .collect::<Vec<_>>();
                let (tab_hits, _) =
                    super::super::tab_strip::draw(frame, header_area, &labels, active, palette);
                let tab_rows = 1;
                for (area, tab) in tab_hits {
                    self.hit(area, Hit::GroupTab { group: id, tab });
                }
                let content_area = Rect {
                    y: inner.y.saturating_add(tab_rows),
                    height: inner.height.saturating_sub(tab_rows),
                    ..inner
                };
                let Some(tab) = active.and_then(|id| self.layout.tabs.get(&id)).cloned() else {
                    frame.render_widget(
                        Paragraph::new(
                            "Empty tab group. Ctrl+Shift+P opens commands; choose Agent or Activity.",
                        ),
                        content_area,
                    );
                    return;
                };
                if let Target::Terminal { terminal } = &tab.target {
                    let area = content_area;
                    if let Some(t) = self.terminal_pool.as_ref().and_then(|p| p.get(terminal)) {
                        terminals::draw(
                            frame,
                            area,
                            &t,
                            *self.terminal_scroll.get(&tab.id).unwrap_or(&0),
                            focused && self.palette.is_none()
                                && matches!(self.terminal_pool.as_ref()
                                    .and_then(|pool| pool.dock_mode(&t.id)),
                                    Some(super::super::terminal_control::Mode::Local)
                                        | Some(super::super::terminal_control::Mode::Offered)),
                            self.terminal_pool.as_ref().and_then(|pool| pool.dock_mode(&t.id)),
                        );
                    } else {
                        frame.render_widget(Paragraph::new("Terminal not live in this owner. Restoring a layout never launches a shell. Use terminal-start explicitly."),area);
                    }
                    return;
                }
                if matches!(tab.target, Target::Settings) {
                    let area = content_area;
                    let settings = self.settings.get_or_insert_with(SettingsState::default);
                    settings.draw(frame, area, &self.ui_config);
                    self.hit(area, Hit::Settings { group: id });
                    return;
                }
                let composer =
                    matches!(tab.target, Target::Conversation) && content_area.height >= 5;
                let slash = if composer && focused && self.input {
                    slash_matches(self.draft.text(), true)
                } else {
                    Vec::new()
                };
                let suggestion_rows = slash
                    .len()
                    .min(usize::from(self.ui_config.autocomplete_max_visible))
                    as u16;
                let suggestion_height = suggestion_rows;
                let ch = if composer { 2 + suggestion_height } else { 0 };
                let text_area = Rect {
                    height: content_area.height.saturating_sub(ch),
                    ..content_area
                };
                let conversation_content = if matches!(tab.target, Target::Conversation) {
                    Some(self.conversation_view_lines(text_area.width))
                } else {
                    None
                };
                let plain_lines = if matches!(
                    tab.target,
                    Target::File { .. } | Target::Diff { .. } | Target::Conversation
                ) {
                    None
                } else {
                    Some(self.rendered_lines(tab.id, &tab.target, text_area.width))
                };
                let count = match &tab.target {
                    Target::Conversation => conversation_content.as_ref().map_or(0, Vec::len),
                    Target::File { scope_id, path } => {
                        self.file_line_count(&FileKey::new(scope_id.clone(), path.clone()))
                    }
                    Target::Diff { review } => self.diff_line_count(review),
                    _ => plain_lines.as_ref().map_or(0, |lines| lines.len()),
                };
                self.line_counts.insert(tab.id, count);
                let follows_end = matches!(tab.target, Target::Conversation | Target::Activity);
                let allow_overscroll =
                    matches!(tab.target, Target::File { .. } | Target::Diff { .. });
                let (start, scrollbar_start) = {
                    let viewport = self.scroll.entry(tab.id).or_insert_with(|| {
                        if follows_end {
                            Viewport::following_end()
                        } else {
                            Viewport::default()
                        }
                    });
                    viewport.configure(count, usize::from(text_area.height), allow_overscroll);
                    (viewport.offset(), viewport.source_scrollbar_offset())
                };
                let visible_height = usize::from(text_area.height);
                let padding = " ".repeat(usize::from(self.ui_config.workspace_output_padding));
                let styled_slice = match &tab.target {
                    Target::Conversation => conversation_content.map(|lines| {
                        lines
                            .into_iter()
                            .skip(start)
                            .take(visible_height)
                            .collect::<Vec<_>>()
                    }),
                    Target::AgentView { .. } => Some(
                        self.agent_view_lines(tab.id)
                            .iter()
                            .skip(start)
                            .take(visible_height)
                            .cloned()
                            .collect::<Vec<_>>(),
                    ),
                    Target::File { scope_id, path } => Some(self.file_view_slice(
                        &FileKey::new(scope_id.clone(), path.clone()),
                        start,
                        visible_height,
                    )),
                    Target::Diff { review } => {
                        Some(self.diff_view_slice(review, start, visible_height))
                    }
                    _ => None,
                };
                let visible = if let Some(lines) = styled_slice {
                    lines
                        .into_iter()
                        .enumerate()
                        .map(|(offset, mut line)| {
                            if !matches!(
                                tab.target,
                                Target::Conversation | Target::AgentView { .. }
                            ) && !padding.is_empty()
                            {
                                line.spans.insert(0, Span::raw(padding.clone()));
                            }
                            self.decorate_find_line(tab.id, start + offset, line)
                        })
                        .collect::<Vec<_>>()
                } else {
                    plain_lines
                        .expect("non-styled view has rendered text")
                        .iter()
                        .skip(start)
                        .take(visible_height)
                        .enumerate()
                        .map(|(offset, line)| {
                            let rendered = Line::styled(
                                format!("{padding}{line}"),
                                Style::default().fg(palette.foreground),
                            );
                            self.decorate_find_line(tab.id, start + offset, rendered)
                        })
                        .collect::<Vec<_>>()
                };
                let horizontal = usize::from(*self.horizontal.get(&tab.id).unwrap_or(&0));
                let selectable_lines = visible.iter().map(line_plain_text).collect::<Vec<_>>();
                self.selectable_views.insert(
                    tab.id,
                    SelectableView {
                        area: text_area,
                        start_line: start,
                        horizontal,
                        lines: selectable_lines,
                    },
                );
                self.hit(
                    text_area,
                    Hit::TextView {
                        group: id,
                        tab: tab.id,
                    },
                );
                frame.render_widget(
                    Paragraph::new(visible).scroll((0, horizontal as u16)),
                    text_area,
                );
                draw_scroll_indicator(
                    frame,
                    text_area,
                    count,
                    usize::from(text_area.height),
                    scrollbar_start,
                    palette,
                );
                self.draw_text_selection(frame, tab.id, palette.selected_style());
                if matches!(tab.target, Target::Conversation) {
                    self.draw_agent_sidecar(frame, text_area);
                }
                if composer {
                    let rect = Rect {
                        y: inner.y + inner.height - ch,
                        height: ch,
                        ..inner
                    };
                    let mut composer_y = rect.y;
                    if suggestion_rows > 0 {
                        let popup = Rect {
                            height: suggestion_height,
                            ..rect
                        };
                        let mut state = ListState::default();
                        state.select(Some(
                            self.composer.selected.min(slash.len().saturating_sub(1)),
                        ));
                        let rows = slash
                            .iter()
                            .take(usize::from(suggestion_rows))
                            .map(|spec| {
                                ListItem::new(Line::from(vec![
                                    ratatui::text::Span::styled(
                                        spec.usage,
                                        palette.menu_usage_style(),
                                    ),
                                    ratatui::text::Span::styled("  ", palette.menu_style()),
                                    ratatui::text::Span::styled(
                                        spec.description,
                                        palette.menu_description_style(),
                                    ),
                                ]))
                            })
                            .collect::<Vec<_>>();
                        frame.render_stateful_widget(
                            List::new(rows)
                                .style(palette.menu_style())
                                .highlight_style(palette.menu_selected_style()),
                            popup,
                            &mut state,
                        );
                        for index in 0..usize::from(suggestion_rows) {
                            self.hit(
                                Rect {
                                    x: popup.x,
                                    y: popup.y.saturating_add(index as u16),
                                    width: popup.width,
                                    height: 1,
                                },
                                Hit::SlashRow {
                                    group: id,
                                    row: index,
                                },
                            );
                        }
                        composer_y = composer_y.saturating_add(suggestion_height);
                    }
                    let composer_rect = Rect {
                        y: composer_y,
                        height: rect.height.saturating_sub(suggestion_height),
                        ..rect
                    };
                    let block = Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(palette.muted));
                    let mut text_rect = block.inner(composer_rect);
                    frame.render_widget(block, composer_rect);
                    if self.snapshot.busy {
                        let elapsed = self
                            .snapshot
                            .turn_started
                            .map_or(Duration::ZERO, |started| started.elapsed());
                        frame.render_widget(
                            Paragraph::new(crate::tui::agent_chrome::activity_border_line(
                                composer_rect.width,
                                elapsed,
                                palette.muted,
                                self.ui_config.theme,
                            )),
                            Rect {
                                height: 1,
                                ..composer_rect
                            },
                        );
                    }
                    self.hit(composer_rect, Hit::Composer(id));
                    let padding = u16::from(self.ui_config.workspace_editor_padding_x)
                        .min(text_rect.width / 2);
                    text_rect.x = text_rect.x.saturating_add(padding);
                    text_rect.width = text_rect.width.saturating_sub(padding.saturating_mul(2));
                    text_rect.height = text_rect.height.min(1);
                    let edit = self.draft.view(text_rect.width, text_rect.height);
                    if self.draft.is_empty() {
                        let placeholder = crate::tui::agent_chrome::composer_placeholder(
                            text_rect.width,
                            self.snapshot.busy,
                            self.input,
                        );
                        frame.render_widget(
                            Paragraph::new(placeholder)
                                .wrap(ratatui::widgets::Wrap { trim: false })
                                .style(Style::default().fg(palette.muted)),
                            text_rect,
                        );
                    } else {
                        frame.render_widget(Paragraph::new(edit.lines.join("\n")), text_rect);
                    }
                    if focused && self.input && self.palette.is_none() {
                        self.can_submit = text_rect.width > 0 && text_rect.height > 0;
                        frame.set_cursor_position((
                            text_rect.x + edit.cursor_x,
                            text_rect.y + edit.cursor_y,
                        ));
                    }
                }
            }
        }
    }
    pub(super) fn draw_palette(&mut self, frame: &mut Frame<'_>, area: Rect) {
        if self.palette.is_none() {
            return;
        }
        let choices = self.palette_choices();
        let state = self.palette.as_ref().expect("open palette");
        let rows = choices
            .iter()
            .map(|choice| palette::Row {
                category: choice.category,
                label: &choice.label,
                description: &choice.description,
                shortcut: &choice.shortcut,
            })
            .collect::<Vec<_>>();
        let geometry = palette::draw(
            frame,
            area,
            state,
            &rows,
            &self.ui_config,
            "No matching resources or commands",
        );
        for (area, category) in geometry.categories {
            self.hit(area, Hit::PaletteCategory(category));
        }
        for (area, index) in geometry.rows {
            self.hit(area, Hit::PaletteRow(index));
        }
    }
}
