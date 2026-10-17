//! Workspace input and interaction reducer for the existing `Ui` state.
//! Agent composer editing still delegates to the shared `tui::composer::Interaction`.
use super::*;

impl Ui {
    pub(super) fn navigator_page(&mut self, down: bool) -> bool {
        let pane = self.layout.focused;
        let Some(Pane::Navigator { kind }) = self.layout.panes.get(&pane).cloned() else {
            return false;
        };
        let step = self
            .hit_regions
            .iter()
            .rev()
            .find(|region| matches!(region.hit, Hit::Pane(id) if id == pane))
            .map_or(10, |region| {
                usize::from(region.area.height).saturating_sub(4).max(1)
            });
        if kind == Navigator::Git {
            let state = self.git.panes.entry(pane).or_default();
            state.selected = if down {
                state.selected.saturating_add(step)
            } else {
                state.selected.saturating_sub(step)
            };
        } else {
            let state = self.nav.entry((pane, kind)).or_default();
            state.selected = if down {
                state.selected.saturating_add(step)
            } else {
                state.selected.saturating_sub(step)
            };
        }
        true
    }

    pub(super) fn start_search(&mut self) {
        let pane = self.layout.focused;
        let (target, original) = match self.layout.panes.get(&pane).cloned() {
            Some(Pane::Navigator { kind }) => {
                let current = if kind == Navigator::Git {
                    self.git.panes.entry(pane).or_default().filter.clone()
                } else {
                    self.nav.entry((pane, kind)).or_default().filter.clone()
                };
                (SearchTarget::Navigator { pane, kind }, current)
            }
            Some(Pane::Group { .. }) => {
                let Some(tab) = self.layout.active(pane).map(|tab| tab.id) else {
                    return;
                };
                let current = self
                    .find
                    .get(&tab)
                    .map(|state| state.query.clone())
                    .unwrap_or_default();
                (SearchTarget::Main { tab }, current)
            }
            None => return,
        };
        let mut editor = Editor::default();
        if let Err(error) = editor.set(original.clone()) {
            self.notice = error.to_string();
            return;
        }
        self.search = Some(SearchState {
            editor,
            target,
            original,
        });
    }

    pub(super) fn searchable_lines(&mut self, tab: Id) -> Vec<String> {
        let Some(target) = self.layout.tabs.get(&tab).map(|tab| tab.target.clone()) else {
            return Vec::new();
        };
        if let Target::File { scope_id, path } = &target {
            return self
                .file_view_lines(&FileKey::new(scope_id.clone(), path.clone()))
                .into_iter()
                .map(|line| {
                    line.spans
                        .into_iter()
                        .map(|span| span.content.into_owned())
                        .collect()
                })
                .collect();
        }
        let width = self
            .rendered
            .get(&tab)
            .map_or(120, |rendered| rendered.width);
        self.rendered_lines(tab, &target, width).as_ref().clone()
    }

    pub(super) fn schedule_find(
        &mut self,
        tab: Id,
        target: Target,
        query: String,
        choosing: bool,
    ) -> bool {
        if !matches!(
            target,
            Target::File { .. } | Target::Diff { .. } | Target::AgentView { .. }
        ) {
            return false;
        }
        if let Some(cancel) = self.find_cancel.remove(&tab) {
            cancel.store(true, Ordering::Release);
        }
        let active = self
            .layout
            .active(self.layout.focused)
            .is_some_and(|active| active.id == tab);
        let Some(generation) = self.find_request_generation.checked_add(1) else {
            self.find_generation.remove(&tab);
            self.find.insert(
                tab,
                FindState {
                    query,
                    ..FindState::default()
                },
            );
            if active {
                self.notice = "Find request IDs exhausted; restart the workspace.".into();
            }
            return true;
        };
        self.find_request_generation = generation;
        self.find_generation.insert(tab, generation);
        let cancel = Arc::new(AtomicBool::new(false));
        self.find_cancel.insert(tab, cancel.clone());
        self.find.insert(
            tab,
            FindState {
                query: query.clone(),
                matches: Vec::new(),
                selected: 0,
                choosing: false,
                committed: choosing,
            },
        );
        if query.is_empty() {
            self.find_cancel.remove(&tab);
            self.find_generation.remove(&tab);
            if choosing && active {
                self.notice = Self::find_notice(0);
            }
            return true;
        }
        let agent_lines =
            matches!(target, Target::AgentView { .. }).then(|| self.agent_view_plain(tab));
        let lowered = query.to_lowercase();
        let file_lines = match &target {
            Target::File { scope_id, path } => self
                .file_lines
                .get(&FileKey::new(scope_id.clone(), path.clone()))
                .cloned(),
            _ => None,
        };
        let file_path = match &target {
            Target::File { path, .. } => Some(path.clone()),
            _ => None,
        };
        let file_context = match &target {
            Target::File { scope_id, path } => {
                Some(self.file_key_context(&FileKey::new(scope_id.clone(), path.clone())))
            }
            _ => None,
        };
        let diff = match &target {
            Target::Diff { review } => self.git.reviews.get(review).cloned(),
            _ => None,
        };
        let comments = match &target {
            Target::Diff { review } => self.git.comments.get(review).cloned().unwrap_or_default(),
            _ => Vec::new(),
        };
        let reviewed = match &target {
            Target::Diff { review } => self.git.reviewed.contains(review),
            _ => false,
        };
        let out = self.background_tx.clone();
        let panic_out = out.clone();
        let panic_query = query.clone();
        if let Err(error) = self.background.try_submit_with_panic(
            "find",
            move || {
                let mut matches = Vec::new();
                if let Some(lines) = agent_lines {
                    for (index, line) in lines.iter().enumerate() {
                        if cancel.load(Ordering::Acquire) {
                            return;
                        }
                        if line.to_lowercase().contains(&lowered) {
                            matches.push(index);
                        }
                    }
                } else if let (Some(_path), Some(lines)) = (file_path, file_lines) {
                    let header = file_context.unwrap_or_default().to_lowercase();
                    if header.contains(&lowered) {
                        matches.push(0);
                    }
                    for (index, line) in lines.iter().enumerate() {
                        if cancel.load(Ordering::Acquire) {
                            return;
                        }
                        if line.to_lowercase().contains(&lowered) {
                            matches.push(index + 2);
                        }
                    }
                } else if let Some((_, review)) = diff {
                    let header = Self::diff_context(&review.path_label, &review.basis, reviewed)
                        .to_lowercase();
                    if header.contains(&lowered) {
                        matches.push(0);
                    }
                    if format!("Snapshot {}", review.id)
                        .to_lowercase()
                        .contains(&lowered)
                    {
                        matches.push(1);
                    }
                    for (index, line) in review.lines.iter().enumerate() {
                        if cancel.load(Ordering::Acquire) {
                            return;
                        }
                        let old = line.old.map(|v| v.to_string()).unwrap_or_default();
                        let new = line.new.map(|v| v.to_string()).unwrap_or_default();
                        if format!("{:>4} {:>6} {:>6} {}", index + 1, old, new, line.text)
                            .to_lowercase()
                            .contains(&lowered)
                        {
                            matches.push(index + 2);
                        }
                    }
                    let base = 2 + review.lines.len();
                    for (index, comment) in comments.iter().enumerate() {
                        if cancel.load(Ordering::Acquire) {
                            return;
                        }
                        if format!("COMMENT row {}: {}", comment.row + 1, comment.text)
                            .to_lowercase()
                            .contains(&lowered)
                        {
                            matches.push(base + index * 2 + 1);
                        }
                    }
                    if review.truncated_display {
                        let row = base + comments.len() * 2;
                        if "[display bounded; raw review patch retained separately]"
                            .contains(&lowered)
                        {
                            matches.push(row);
                        }
                    }
                }
                if !cancel.load(Ordering::Acquire) {
                    let _ = out.blocking_send(UiBackgroundResult::Find {
                        tab,
                        generation,
                        query,
                        matches,
                        choosing,
                        error: None,
                    });
                }
            },
            move || {
                let _ = panic_out.blocking_send(UiBackgroundResult::Find {
                    tab,
                    generation,
                    query: panic_query,
                    matches: Vec::new(),
                    choosing,
                    error: Some("Find failed; try the search again"),
                });
            },
        ) {
            if active {
                self.notice = error.to_string();
            }
            self.find_cancel.remove(&tab);
            self.find_generation.remove(&tab);
        } else if choosing && active {
            self.notice = "Searching…".into();
        }
        true
    }

    pub(super) fn find_notice(count: usize) -> String {
        if count == 0 {
            "No matches".into()
        } else {
            let label = if count == 1 { "match" } else { "matches" };
            format!("{count} {label} · press a flash label or n/N")
        }
    }

    pub(super) fn update_find(&mut self, tab: Id, query: String, choosing: bool) {
        let Some(target) = self.layout.tabs.get(&tab).map(|tab| tab.target.clone()) else {
            return;
        };
        if self.schedule_find(tab, target, query.clone(), choosing) {
            return;
        }
        let lowered = query.to_lowercase();
        let matches = if lowered.is_empty() {
            Vec::new()
        } else {
            self.searchable_lines(tab)
                .iter()
                .enumerate()
                .filter_map(|(index, line)| line.to_lowercase().contains(&lowered).then_some(index))
                .collect::<Vec<_>>()
        };
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
        if let Some(row) = matches.get(selected) {
            self.jump_to_row(tab, *row);
        }
        if choosing {
            self.notice = Self::find_notice(matches.len());
        }
    }

    pub(super) fn apply_search_editor(&mut self) {
        let Some(search) = self.search.clone() else {
            return;
        };
        let query = search.editor.text().to_owned();
        match search.target {
            SearchTarget::Navigator { pane, kind } => {
                if kind == Navigator::Git {
                    let state = self.git.panes.entry(pane).or_default();
                    state.filter = query;
                    state.selected = 0;
                } else {
                    let state = self.nav.entry((pane, kind)).or_default();
                    state.filter = query;
                    state.selected = 0;
                    if kind == Navigator::Sessions {
                        state.selected_sessions.clear();
                    }
                }
            }
            SearchTarget::Main { tab } => self.update_find(tab, query, false),
        }
    }

    pub(super) fn search_key(&mut self, key: KeyEvent) -> bool {
        let Some(mut search) = self.search.take() else {
            return false;
        };
        match key.code {
            KeyCode::Esc => {
                match search.target {
                    SearchTarget::Navigator { pane, kind } => {
                        if kind == Navigator::Git {
                            self.git.panes.entry(pane).or_default().filter = search.original;
                        } else {
                            let state = self.nav.entry((pane, kind)).or_default();
                            state.filter = search.original;
                            if kind == Navigator::Sessions {
                                state.selected_sessions.clear();
                            }
                        }
                    }
                    SearchTarget::Main { tab } => self.update_find(tab, search.original, false),
                }
                self.notice.clear();
                true
            }
            KeyCode::Enter => {
                let query = search.editor.text().to_owned();
                match search.target {
                    SearchTarget::Navigator { .. } => {
                        self.search = Some(search);
                        self.apply_search_editor();
                        self.search = None;
                    }
                    SearchTarget::Main { tab } => {
                        self.update_find(tab, query, true);
                    }
                }
                true
            }
            _ => {
                if let Err(error) = edit_key(&mut search.editor, key, EditMode::SingleLine) {
                    self.notice = error.to_string();
                }
                self.search = Some(search);
                self.apply_search_editor();
                true
            }
        }
    }

    pub(super) fn cycle_find(&mut self, forward: bool) -> bool {
        let Some(tab) = self.layout.active(self.layout.focused).map(|tab| tab.id) else {
            return false;
        };
        let Some(state) = self.find.get_mut(&tab) else {
            return false;
        };
        if state.matches.is_empty() {
            return false;
        }
        state.selected = if forward {
            (state.selected + 1) % state.matches.len()
        } else if state.selected == 0 {
            state.matches.len() - 1
        } else {
            state.selected - 1
        };
        state.choosing = false;
        state.committed = false;
        let row = state.matches.get(state.selected).copied();
        let _ = state;
        if let Some(row) = row {
            self.jump_to_row(tab, row);
        }
        true
    }

    pub(super) fn flash_jump(&mut self, key: char) -> bool {
        const LABELS: &str = "asdfghjklqwertyuiopzxcvbnm1234567890";
        let Some(tab) = self.layout.active(self.layout.focused).map(|tab| tab.id) else {
            return false;
        };
        let Some(state) = self.find.get_mut(&tab) else {
            return false;
        };
        if !state.choosing {
            return false;
        }
        let Some(index) = LABELS.chars().position(|candidate| candidate == key) else {
            return false;
        };
        let Some(row) = state.matches.get(index).copied() else {
            return false;
        };
        state.selected = index;
        state.choosing = false;
        state.committed = false;
        self.jump_to_row(tab, row);
        true
    }

    pub(super) fn settings_input(&mut self, action: SettingsInput) {
        match action {
            SettingsInput::Close => {
                self.settings = None;
                self.notice.clear();
                if let Some(group) = self.layout.groups().into_iter().find(|group| {
                    matches!(
                        self.layout.active(*group).map(|tab| &tab.target),
                        Some(Target::Settings)
                    )
                }) {
                    if let Some(tab) = self.layout.active(group).map(|tab| tab.id) {
                        self.apply(Action::CloseTab { group, tab });
                    }
                }
            }
            SettingsInput::ProviderSetup => {
                self.settings = None;
                self.exit = Some(Exit::ConfigureProvider);
            }
            SettingsInput::Handled => {}
        }
    }

    pub(super) fn select_or_close_tab(&mut self, group: Id, tab: Id, now: Instant) {
        let double = self.last_tab_click.is_some_and(|(g, t, at)| {
            g == group && t == tab && now.duration_since(at) <= Duration::from_millis(450)
        });
        let closable = self
            .layout
            .tabs
            .get(&tab)
            .is_some_and(|tab| !matches!(tab.target, Target::Terminal { .. }));
        if double && closable {
            let was_settings = self
                .layout
                .tabs
                .get(&tab)
                .is_some_and(|tab| matches!(tab.target, Target::Settings));
            self.apply(Action::CloseTab { group, tab });
            if was_settings {
                self.settings = None;
            }
            self.last_tab_click = None;
        } else {
            self.apply(Action::Select { group, tab });
            self.last_tab_click = Some((group, tab, now));
        }
        self.input = matches!(
            self.layout.active(group).map(|tab| &tab.target),
            Some(Target::Conversation)
        );
    }

    pub(super) fn scroll_hit(&mut self, hit: Option<Hit>, down: bool) {
        match hit {
            Some(Hit::Pane(pane)) => {
                self.apply(Action::Focus(pane));
                match self.layout.panes.get(&pane).cloned() {
                    Some(Pane::Navigator { kind }) => {
                        if kind == Navigator::Git {
                            let state = self.git.panes.entry(pane).or_default();
                            state.selected = if down {
                                state.selected.saturating_add(WHEEL_ROWS)
                            } else {
                                state.selected.saturating_sub(WHEEL_ROWS)
                            };
                        } else {
                            let state = self.nav.entry((pane, kind)).or_default();
                            state.selected = if down {
                                state.selected.saturating_add(WHEEL_ROWS)
                            } else {
                                state.selected.saturating_sub(WHEEL_ROWS)
                            };
                        }
                    }
                    Some(Pane::Group { .. }) => self.scroll_by(if down {
                        WHEEL_ROWS as isize
                    } else {
                        -(WHEEL_ROWS as isize)
                    }),
                    None => {}
                }
            }
            Some(Hit::NavigatorRow { pane, .. })
            | Some(Hit::NavigatorKind { pane, .. })
            | Some(Hit::NavigatorDisplay { pane, .. })
            | Some(Hit::SessionScope { pane, .. })
            | Some(Hit::SessionMultiSelect { pane })
            | Some(Hit::GitMode { pane, .. })
            | Some(Hit::GitDisplay { pane, .. }) => {
                self.apply(Action::Focus(pane));
                if let Some(Pane::Navigator { kind }) = self.layout.panes.get(&pane).cloned() {
                    if kind == Navigator::Git {
                        let state = self.git.panes.entry(pane).or_default();
                        state.selected = if down {
                            state.selected.saturating_add(WHEEL_ROWS)
                        } else {
                            state.selected.saturating_sub(WHEEL_ROWS)
                        };
                    } else {
                        let state = self.nav.entry((pane, kind)).or_default();
                        state.selected = if down {
                            state.selected.saturating_add(WHEEL_ROWS)
                        } else {
                            state.selected.saturating_sub(WHEEL_ROWS)
                        };
                    }
                }
            }
            Some(Hit::Settings { group }) => {
                self.apply(Action::Focus(group));
                if let Some(settings) = self.settings.as_mut() {
                    let key = KeyEvent::new(
                        if down { KeyCode::Down } else { KeyCode::Up },
                        KeyModifiers::NONE,
                    );
                    let _ = settings.handle(key, &mut self.ui_config);
                }
            }
            Some(Hit::GroupTab { group, .. })
            | Some(Hit::Composer(group))
            | Some(Hit::SlashRow { group, .. }) => {
                self.apply(Action::Focus(group));
                self.scroll_by(if down {
                    WHEEL_ROWS as isize
                } else {
                    -(WHEEL_ROWS as isize)
                });
            }
            _ => self.scroll_by(if down {
                WHEEL_ROWS as isize
            } else {
                -(WHEEL_ROWS as isize)
            }),
        }
    }

    pub(super) fn mouse(&mut self, event: MouseEvent, worker: &Worker, browser: &Browser) {
        let point = (event.column, event.row);
        if self.palette.is_some()
            && !matches!(
                event.kind,
                MouseEventKind::ScrollUp
                    | MouseEventKind::ScrollDown
                    | MouseEventKind::Down(MouseButton::Left)
            )
        {
            return;
        }
        match event.kind {
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(mut drag) = self.resize_drag {
                    let current = match drag.axis {
                        Axis::Columns => event.column,
                        Axis::Rows => event.row,
                    };
                    let movement = i32::from(current) - i32::from(drag.last);
                    if movement != 0 {
                        let step = (10_000i32 / i32::from(drag.span.max(20))).max(40);
                        let delta = movement
                            .saturating_mul(step)
                            .clamp(i32::from(i16::MIN), i32::from(i16::MAX))
                            as i16;
                        self.apply(Action::ResizeBetween {
                            first: drag.first,
                            second: drag.second,
                            delta,
                        });
                        drag.last = current;
                        self.resize_drag = Some(drag);
                    }
                    return;
                }
                if self.selection_dragging {
                    let tab = self.text_selection.as_ref().map(|selection| selection.tab);
                    if let Some(tab) = tab {
                        if let Some(point) = self.text_point_for(tab, event.column, event.row) {
                            if let Some(selection) = self.text_selection.as_mut() {
                                if selection.current != point {
                                    selection.current = point;
                                    self.selection_moved = true;
                                }
                            }
                        }
                    }
                    return;
                }
                return;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.resize_drag.take().is_some() {
                    return;
                }
                if self.selection_dragging {
                    let tab = self.text_selection.as_ref().map(|selection| selection.tab);
                    if let Some(tab) = tab {
                        if let Some(point) = self.text_point_for(tab, event.column, event.row) {
                            if let Some(selection) = self.text_selection.as_mut() {
                                if selection.current != point {
                                    selection.current = point;
                                    self.selection_moved = true;
                                }
                            }
                        }
                    }
                    self.selection_dragging = false;
                    if self.selection_moved {
                        let _ = self.copy_selection();
                    } else {
                        if let Some(selection) = self.text_selection.take() {
                            self.saved_selections.remove(&selection.tab);
                        }
                    }
                    return;
                }
                return;
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let down = matches!(event.kind, MouseEventKind::ScrollDown);
                if self.palette.is_some() {
                    let choices = self.palette_choices();
                    let len = choices.len();
                    if len > 0 {
                        let selected = self
                            .palette
                            .as_ref()
                            .expect("open palette")
                            .selected(len)
                            .unwrap_or(0);
                        let next = if down {
                            selected.saturating_add(WHEEL_ROWS).min(len - 1)
                        } else {
                            selected.saturating_sub(WHEEL_ROWS)
                        };
                        if let Some(state) = self.palette.as_mut() {
                            state.select(next);
                            state.reconcile(
                                &choices.iter().map(PaletteItem::key).collect::<Vec<_>>(),
                            );
                        }
                    }
                    return;
                }
                let hit = self
                    .hit_regions
                    .iter()
                    .rev()
                    .find(|region| rect_contains(region.area, point))
                    .map(|region| region.hit.clone());
                self.scroll_hit(hit, down);
                return;
            }
            MouseEventKind::Down(MouseButton::Left) => {}
            _ => return,
        }

        let hit = self
            .hit_regions
            .iter()
            .rev()
            .find(|region| {
                rect_contains(region.area, point)
                    && (self.palette.is_none()
                        || matches!(region.hit, Hit::PaletteCategory(_) | Hit::PaletteRow(_)))
            })
            .map(|region| (region.hit.clone(), region.area));
        let Some((hit, area)) = hit else {
            if self.palette.is_some() {
                return;
            }
            if let Some(selection) = self.text_selection.take() {
                self.saved_selections.remove(&selection.tab);
            }
            self.selection_dragging = false;
            self.selection_moved = false;
            return;
        };
        if self.palette.is_none() && !matches!(hit, Hit::TextView { .. }) {
            if let Some(selection) = self.text_selection.take() {
                self.saved_selections.insert(selection.tab, selection);
            }
            self.selection_dragging = false;
            self.selection_moved = false;
        }
        match hit {
            Hit::Divider {
                first,
                second,
                axis,
            } => {
                let pane_areas = self
                    .hit_regions
                    .iter()
                    .filter_map(|region| match region.hit {
                        Hit::Pane(id) if id == first || id == second => Some(region.area),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let span = if pane_areas.len() == 2 {
                    match axis {
                        Axis::Columns => pane_areas
                            .iter()
                            .map(|rect| rect.width)
                            .fold(0u16, u16::saturating_add),
                        Axis::Rows => pane_areas
                            .iter()
                            .map(|rect| rect.height)
                            .fold(0u16, u16::saturating_add),
                    }
                } else {
                    match axis {
                        Axis::Columns => area.width.max(40),
                        Axis::Rows => area.height.max(20),
                    }
                };
                self.resize_drag = Some(ResizeDrag {
                    first,
                    second,
                    axis,
                    last: match axis {
                        Axis::Columns => event.column,
                        Axis::Rows => event.row,
                    },
                    span,
                });
            }
            Hit::MaximizePane(pane) => {
                self.toggle_maximize(pane);
                self.notice.clear();
            }
            Hit::Pane(pane) => {
                self.apply(Action::Focus(pane));
                self.input = false;
            }
            Hit::NavigatorKind { pane, kind } => {
                self.apply(Action::Focus(pane));
                self.set_navigator(kind);
                if kind == Navigator::Sessions {
                    self.owner(worker, Command::Refresh, false);
                }
            }
            Hit::NavigatorDisplay { pane, display } => {
                self.apply(Action::Focus(pane));
                if let Some(Pane::Navigator { kind }) = self.layout.panes.get(&pane).cloned() {
                    self.set_navigator_display(browser, pane, kind, display);
                }
            }
            Hit::SessionScope { pane, scope } => {
                self.apply(Action::Focus(pane));
                let state = self.nav.entry((pane, Navigator::Sessions)).or_default();
                state.session_scope = scope;
                state.selected = 0;
                state.selected_sessions.clear();
            }
            Hit::SessionMultiSelect { pane } => {
                self.apply(Action::Focus(pane));
                let state = self.nav.entry((pane, Navigator::Sessions)).or_default();
                state.multi_select = !state.multi_select;
                if !state.multi_select {
                    state.selected_sessions.clear();
                }
            }
            Hit::GitMode { pane, mode } => {
                self.apply(Action::Focus(pane));
                if let Err(error) = self.git.set_mode(pane, mode) {
                    self.notice = error;
                }
            }
            Hit::GitDisplay { pane, display } => {
                self.apply(Action::Focus(pane));
                let nav = self.git.panes.entry(pane).or_default();
                if nav.mode == git::Mode::Changes {
                    nav.set_display(display);
                }
            }
            Hit::NavigatorRow { pane, row } => {
                self.apply(Action::Focus(pane));
                if let Some(Pane::Navigator { kind }) = self.layout.panes.get(&pane).cloned() {
                    if kind == Navigator::Git {
                        self.git.panes.entry(pane).or_default().selected = row;
                    } else {
                        self.nav.entry((pane, kind)).or_default().selected = row;
                    }
                    if kind == Navigator::Sessions
                        && self
                            .nav
                            .get(&(pane, Navigator::Sessions))
                            .is_some_and(|state| state.multi_select)
                    {
                        let session = self
                            .session_rows(pane)
                            .get(row)
                            .and_then(|row| navigator_session_id(&row.item))
                            .map(str::to_owned);
                        if let Some(session) = session {
                            let state = self.nav.entry((pane, Navigator::Sessions)).or_default();
                            if !state.selected_sessions.insert(session.clone()) {
                                state.selected_sessions.remove(&session);
                            }
                        }
                    } else {
                        self.navigator_open(worker, browser);
                    }
                }
            }
            Hit::AgentSession { session } => {
                if self.owner(
                    worker,
                    Command::Resume {
                        session,
                        branch: "main".into(),
                    },
                    true,
                ) {
                    self.open(Target::Conversation);
                    self.input = false;
                }
            }
            Hit::TextView { group, tab } => {
                self.apply(Action::Focus(group));
                self.input = false;
                if let Some(point) = self.text_point_for(tab, event.column, event.row) {
                    self.text_selection = Some(TextSelection {
                        tab,
                        anchor: point,
                        current: point,
                    });
                    self.selection_dragging = true;
                    self.selection_moved = false;
                }
            }
            Hit::GroupTab { group, tab } => {
                self.select_or_close_tab(group, tab, Instant::now());
            }
            Hit::Composer(group) => {
                self.apply(Action::Focus(group));
                self.input = true;
            }
            Hit::SlashRow { group, row } => {
                self.apply(Action::Focus(group));
                self.input = true;
                let choices = slash_matches(self.draft.text(), true);
                if let Some(spec) = choices.get(row.min(choices.len().saturating_sub(1))) {
                    let completed = complete_slash(self.draft.text(), spec);
                    let _ = self.draft.set(completed);
                    self.composer.reset_after_external_edit();
                    self.composer.selected = row;
                }
            }
            Hit::Settings { group } => {
                self.apply(Action::Focus(group));
                let previous_ui = self.ui_config.clone();
                let previous_theme = self.ui_config.theme;
                let action = self
                    .settings
                    .as_mut()
                    .map(|settings| settings.click(area, event.row, &mut self.ui_config));
                if self.ui_config.theme != previous_theme {
                    self.refresh_file_highlights_for_theme();
                }
                if self.ui_config != previous_ui {
                    if let Err(error) = worker.persist_ui(self.ui_config.clone()) {
                        self.notice = error.to_string();
                    }
                }
                if let Some(action) = action {
                    self.settings_input(action);
                }
            }
            Hit::PaletteCategory(category) => {
                self.toggle_palette_category(category, browser);
            }
            Hit::PaletteRow(index) => {
                if let Some(state) = self.palette.as_mut() {
                    state.select(index);
                }
                let selected = self
                    .palette
                    .as_ref()
                    .and_then(|editor| self.palette_items(editor.text()).get(index).cloned());
                if let Some(item) = selected {
                    let completion = match &item.target {
                        PaletteTarget::Command(command) if command.ends_with(' ') => {
                            Some(command.clone())
                        }
                        PaletteTarget::Slash(command) if command.ends_with(' ') => {
                            Some(format!("/{command}"))
                        }
                        _ => None,
                    };
                    if let Some(completion) = completion {
                        if let Some(editor) = self.palette.as_mut() {
                            let _ = editor.set(&completion);
                        }
                        return;
                    }
                    self.close_palette();
                    self.execute_palette_item(item, worker, browser);
                }
            }
        }
    }

    pub(super) fn key(&mut self, key: KeyEvent, worker: &Worker, browser: &Browser) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        if palette::is_open_key(&key) {
            self.open_palette();
            return;
        }
        if self.palette_key(key, worker, browser) {
            return;
        }
        if self.search_key(key) {
            if matches!(
                self.layout.panes.get(&self.layout.focused),
                Some(Pane::Navigator {
                    kind: Navigator::Files
                })
            ) {
                let pane = self.layout.focused;
                let query = self
                    .nav
                    .get(&(pane, Navigator::Files))
                    .map(|state| state.filter.clone())
                    .unwrap_or_default();
                self.search_file_filter(browser, pane, &query);
            }
            return;
        }
        let settings_active = matches!(
            self.layout
                .active(self.layout.focused)
                .map(|tab| &tab.target),
            Some(Target::Settings)
        );
        if settings_active {
            let previous_ui = self.ui_config.clone();
            let previous_theme = self.ui_config.theme;
            let settings = self.settings.get_or_insert_with(SettingsState::default);
            let action = settings.handle(key, &mut self.ui_config);
            if self.ui_config.theme != previous_theme {
                self.refresh_file_highlights_for_theme();
            }
            if self.ui_config != previous_ui {
                if let Err(error) = worker.persist_ui(self.ui_config.clone()) {
                    self.notice = error.to_string();
                }
            }
            self.settings_input(action);
            return;
        }
        // Metadata-rich Ctrl+Shift+P and F1 are always global. A terminal
        // that collapses Ctrl+Shift+P to Ctrl+P may use that fallback only in
        // navigation mode; a composer or child PTY owns ordinary Ctrl+P.
        let collapsed_palette_fallback = !self.input && self.terminal().is_none();
        if is_command_palette_key(&key, collapsed_palette_fallback) || key.code == KeyCode::F(1) {
            self.open_palette();
            return;
        }
        self.expire_chord();
        if let Some(started) = self.chord.take() {
            if chord_is_live(started) {
                match key.code {
                    KeyCode::Char('z') | KeyCode::Char('Z') => {
                        self.toggle_maximize(self.layout.focused);
                        self.notice.clear();
                    }
                    KeyCode::Char('s') | KeyCode::Char('S') => {
                        self.command("layout-save", worker, browser);
                    }
                    KeyCode::Esc => self.notice.clear(),
                    _ => {
                        self.notice = "Unknown Ctrl+K chord. F1 opens the command palette.".into();
                    }
                }
                return;
            }
        }
        if is_ctrl_char(&key, '`') {
            self.toggle_terminal();
            return;
        }
        if let Some(forward) = tab_cycle_direction(&key) {
            let group = if matches!(
                self.layout.panes.get(&self.layout.focused),
                Some(Pane::Group { .. })
            ) {
                self.layout.focused
            } else {
                self.group()
            };
            self.apply(Action::CycleTab { group, forward });
            self.input = false;
            return;
        }
        if key.modifiers.contains(KeyModifiers::ALT) {
            match key.code {
                KeyCode::Char('e' | 'E') => {
                    self.focus_explorer();
                    return;
                }
                KeyCode::Char('w' | 'W') => {
                    self.focus_main();
                    return;
                }
                KeyCode::Char(c @ '1'..='9') => {
                    self.activate_number((c as u8 - b'0') as usize, worker);
                    return;
                }
                _ => {}
            }
        }

        // A focused embedded terminal owns ordinary Ctrl/Alt/Esc/Tab/editor
        // input. We reserve only the explicit global workspace chords above.
        if let Some(t) = self.terminal() {
            if let Some(bytes) = terminals::key(key, t.input_modes().0) {
                if let Err(e) = t.input(bytes) {
                    self.notice = e.to_string();
                }
            }
            return;
        }

        if matches!(
            self.layout
                .active(self.layout.focused)
                .map(|tab| &tab.target),
            Some(Target::Conversation)
        ) {
            match key.code {
                KeyCode::F(6) => {
                    self.ui_config.show_thinking = !self.ui_config.show_thinking;
                    self.notice = format!(
                        "Thinking activity {}",
                        if self.ui_config.show_thinking {
                            "expanded"
                        } else {
                            "collapsed"
                        }
                    );
                    return;
                }
                KeyCode::F(7) => {
                    self.ui_config.show_tool_calls = !self.ui_config.show_tool_calls;
                    self.notice = format!(
                        "Tool details {}",
                        if self.ui_config.show_tool_calls {
                            "expanded"
                        } else {
                            "collapsed"
                        }
                    );
                    return;
                }
                _ => {}
            }
        }

        if is_ctrl_shift_char(&key, 'e') {
            self.set_navigator(Navigator::Files);
            return;
        }
        if is_ctrl_shift_char(&key, 'g') {
            self.set_navigator(Navigator::Git);
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            if self.text_selection.is_some() && !self.input && self.copy_selection() {
                return;
            }
            if self.snapshot.busy || self.snapshot.state_pending {
                worker.projection.cancel();
                self.notice = "Cancellation requested; remote billing/effects may remain.".into();
            } else if self.input {
                self.draft.clear();
                self.composer.reset_after_external_edit();
                self.notice = "Draft cleared".into();
            }
            return;
        }

        if is_ctrl_char(&key, 'k') {
            self.chord = Some(Instant::now());
            self.notice = "Ctrl+K…  Z maximize · S save layout".into();
            return;
        }

        if is_ctrl_char(&key, 'r') {
            self.command("refresh", worker, browser);
            return;
        }
        if is_ctrl_char(&key, '\\') {
            self.command("split right", worker, browser);
            return;
        }
        if is_ctrl_char(&key, 'w') {
            if matches!(
                self.layout.panes.get(&self.layout.focused),
                Some(Pane::Group { .. })
            ) {
                self.command("close-tab", worker, browser);
            }
            return;
        }

        if !self.input {
            if key.code == KeyCode::Char(':')
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            {
                self.open_palette();
                return;
            }
            if key.code == KeyCode::Char('?')
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            {
                self.open(Target::Help);
                return;
            }
        }

        if !self.input && key.modifiers.is_empty() {
            if let KeyCode::Char('/') = key.code {
                self.start_search();
                return;
            }
            if let KeyCode::Char('n') = key.code {
                if self.cycle_find(true) {
                    return;
                }
            }
            if let KeyCode::Char('N') = key.code {
                if self.cycle_find(false) {
                    return;
                }
            }
            if let KeyCode::Char(label) = key.code {
                if self.flash_jump(label) {
                    return;
                }
            }
        }

        match key.code {
            KeyCode::PageUp => {
                if !self.navigator_page(false) {
                    self.page_scroll(false);
                }
                return;
            }
            KeyCode::PageDown => {
                if !self.navigator_page(true) {
                    self.page_scroll(true);
                }
                return;
            }
            KeyCode::Esc => {
                self.input = false;
                self.composer.reset_after_external_edit();
                self.close_help_on_escape();
                return;
            }
            KeyCode::Tab if !self.input => {
                if let Err(e) = self.layout.focus_next() {
                    self.notice = e.to_string();
                }
                return;
            }
            _ => {}
        }

        if let Some(Pane::Navigator { kind }) = self.layout.panes.get(&self.layout.focused).cloned()
        {
            let pane = self.layout.focused;
            if kind == Navigator::Git {
                let n = self.git.panes.entry(pane).or_default();
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => n.selected = n.selected.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => n.selected = n.selected.saturating_add(1),
                    KeyCode::Char('t') if n.mode == git::Mode::Changes => {
                        n.set_display(if n.display == git::Display::Tree {
                            git::Display::List
                        } else {
                            git::Display::Tree
                        });
                    }
                    KeyCode::Enter | KeyCode::Char('l') => {
                        if let Err(e) = self.git.open(pane) {
                            self.notice = e;
                        }
                    }
                    _ => {}
                }
                return;
            }
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    let n = self.nav.entry((pane, kind)).or_default();
                    n.selected = n.selected.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let n = self.nav.entry((pane, kind)).or_default();
                    n.selected = n.selected.saturating_add(1);
                }
                KeyCode::Char('o') if kind == Navigator::Sessions => {
                    self.open_selected_agent_view()
                }
                KeyCode::Delete if kind == Navigator::Sessions => {
                    let archived = self
                        .nav
                        .get(&(pane, Navigator::Sessions))
                        .is_none_or(|state| state.session_scope != SessionScope::Archived);
                    self.set_selected_sessions_archived(pane, worker, archived);
                }
                KeyCode::Char(' ') | KeyCode::Enter
                    if kind == Navigator::Sessions
                        && self
                            .nav
                            .get(&(pane, Navigator::Sessions))
                            .is_some_and(|state| state.multi_select) =>
                {
                    self.toggle_selected_session(pane);
                }
                KeyCode::Char('t') => {
                    let display = self
                        .nav
                        .get(&(pane, kind))
                        .map_or(NavDisplay::Tree, |state| state.display);
                    let next = if display == NavDisplay::Tree {
                        NavDisplay::List
                    } else {
                        NavDisplay::Tree
                    };
                    self.set_navigator_display(browser, pane, kind, next);
                }
                KeyCode::Enter | KeyCode::Char('l') => self.navigator_open(worker, browser),
                KeyCode::Backspace | KeyCode::Char('h') if kind == Navigator::Files => {
                    let n = self.nav.entry((pane, kind)).or_default();
                    let parent = n
                        .directory
                        .rsplit_once('/')
                        .map_or(String::new(), |(p, _)| p.into());
                    self.list_files(browser, pane, parent);
                }
                _ => {}
            }
            return;
        }

        let conversation = matches!(
            self.layout.active(self.layout.focused).map(|t| &t.target),
            Some(Target::Conversation)
        );
        if conversation && self.input {
            let history = self.prompt_history();
            match self.composer.handle(&mut self.draft, key, &history, true) {
                Ok(ComposerAction::Handled) => {}
                Ok(ComposerAction::OpenSettings) => self.open_settings(),
                Ok(ComposerAction::Submit(_)) => self.send_draft(worker, browser),
                Ok(ComposerAction::SubmitWhileBusy) => {
                    self.notice = "Current Agent input is busy; draft preserved".into();
                }
                Err(error) => self.notice = error.to_string(),
            }
        } else {
            match key.code {
                KeyCode::Left | KeyCode::Char('h') => {
                    if let Some(t) = self.layout.active(self.layout.focused) {
                        let n = self.horizontal.entry(t.id).or_default();
                        *n = n.saturating_sub(4);
                    }
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    if let Some(t) = self.layout.active(self.layout.focused) {
                        let n = self.horizontal.entry(t.id).or_default();
                        *n = n.saturating_add(4);
                    }
                }
                KeyCode::Char('i') if conversation => {
                    self.input = true;
                    if let Some(t) = self.layout.active(self.layout.focused) {
                        self.scroll.entry(t.id).or_default().end();
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => self.scroll_by(-1),
                KeyCode::Down | KeyCode::Char('j') => self.scroll_by(1),
                KeyCode::End => self.scroll_end(),
                KeyCode::Home => self.scroll_home(),
                _ => {}
            }
        }
    }
    pub(super) fn open_palette(&mut self) {
        if self.palette.is_some() {
            return;
        }
        self.palette_file_scope = self.current_file_scope();
        self.palette_file_container = self.layout.container_for(self.layout.focused);
        self.palette = Some(palette::State::default());
        self.palette_file_query.clear();
        self.palette_file_results.clear();
        self.palette_file_pending = None;
        self.chord = None;
    }

    pub(super) fn finish_palette_dispatch(&mut self, invoking: palette::State, accepted: bool) {
        if !accepted {
            if self.palette.is_none() {
                self.palette = Some(invoking);
            }
        } else if self.palette.is_none() {
            self.close_palette();
        } else {
            // A successful command may itself open a fresh palette (resume).
            // Cancel the old query without closing that new invoking surface.
            self.cancel_file_searches(None);
        }
    }

    pub(super) fn close_palette(&mut self) {
        self.cancel_file_searches(None);
        self.palette = None;
        self.palette_file_query.clear();
        self.palette_file_results.clear();
        self.chord = None;
    }

    pub(super) fn sync_palette_file_search(&mut self, browser: &Browser, raw_query: &str) {
        let parsed = palette::parse(raw_query);
        if parsed
            .category
            .is_none_or(|category| category == PaletteCategory::Files)
        {
            self.search_palette_files(browser, parsed.text);
        } else {
            self.search_palette_files(browser, "");
        }
    }

    pub(super) fn toggle_palette_category(&mut self, category: PaletteCategory, browser: &Browser) {
        if let Some(state) = self.palette.as_mut() {
            if let Err(error) = state.category(category) {
                self.notice = error.to_string();
                return;
            }
            let query = state.text().to_owned();
            self.sync_palette_file_search(browser, &query);
        }
    }

    pub(super) fn expire_chord(&mut self) -> bool {
        if self
            .chord
            .is_some_and(|started| started.elapsed() >= KEY_CHORD_TIMEOUT)
        {
            self.chord = None;
            if self.notice.starts_with("Ctrl+K…") {
                self.notice.clear();
            }
            return true;
        }
        false
    }

    pub(super) fn palette_key(
        &mut self,
        key: KeyEvent,
        worker: &Worker,
        browser: &Browser,
    ) -> bool {
        if self.palette.is_none() {
            return false;
        }
        if is_ctrl_char(&key, 'c') {
            if self.snapshot.busy || self.snapshot.state_pending {
                worker.projection.cancel();
                self.notice = "Cancellation requested; remote billing/effects may remain.".into();
            } else {
                self.close_palette();
            }
            return true;
        }
        let typed = self
            .palette
            .as_ref()
            .map(|editor| editor.text().to_owned())
            .unwrap_or_default();
        let parsed = palette::parse(&typed);
        let choices = self.palette_choices();
        let intent = match self
            .palette
            .as_mut()
            .expect("open palette")
            .key(key, choices.len())
        {
            Ok(intent) => intent,
            Err(error) => {
                self.notice = error.to_string();
                return true;
            }
        };
        match intent {
            palette::Intent::Close => self.close_palette(),
            palette::Intent::Handled => {}
            palette::Intent::Unmatched => {
                // Keep the existing ':' typed-command contract even when an
                // action is contextually absent from search (e.g. quit-discard
                // with no draft). Missing selected resources remain disarmed.
                let body = parsed.text.trim();
                if parsed.category.is_none_or(|category| {
                    matches!(
                        category,
                        PaletteCategory::Actions | PaletteCategory::Commands
                    )
                }) {
                    let item = PaletteItem {
                        category: PaletteCategory::Actions,
                        label: body.into(),
                        description: String::new(),
                        shortcut: String::new(),
                        target: PaletteTarget::Command(body.into()),
                        score: 0,
                    };
                    if !self.palette_item_allowed(&item) {
                        self.notice="This command is available after the current turn settles; no command queued".into();
                        return true;
                    }
                    let invoking = self.palette.take().expect("open palette");
                    let accepted = self.command(body, worker, browser);
                    self.finish_palette_dispatch(invoking, accepted);
                }
            }
            palette::Intent::Changed => {
                let query = self
                    .palette
                    .as_ref()
                    .expect("open palette")
                    .text()
                    .to_owned();
                self.sync_palette_file_search(browser, &query);
            }
            palette::Intent::Complete(index) => {
                if let Some(item) = choices.get(index) {
                    let completion = match &item.target {
                        PaletteTarget::Command(command) => {
                            if parsed.category == Some(PaletteCategory::Actions) {
                                Some(palette::with_category(
                                    command,
                                    Some(PaletteCategory::Actions),
                                ))
                            } else {
                                Some(command.clone())
                            }
                        }
                        PaletteTarget::Slash(command) => {
                            let resolved = palette::resolve_slash(parsed.text, command);
                            Some(if resolved.has_args {
                                format!("/{}", resolved.command)
                            } else {
                                format!("/{command}")
                            })
                        }
                        _ => None,
                    };
                    if let (Some(editor), Some(completion)) = (self.palette.as_mut(), completion) {
                        let _ = editor.set(&completion);
                    }
                }
            }
            palette::Intent::Activate(index) => {
                let typed_body = parsed.text.trim().to_owned();
                let selected = choices.get(index).cloned();
                if let Some(mut item) = selected {
                    match &mut item.target {
                        PaletteTarget::Command(command) => {
                            let base = command.trim_end().to_owned();
                            let has_inline_args =
                                typed_body.strip_prefix(&base).is_some_and(|rest| {
                                    rest.starts_with(' ') && !rest.trim().is_empty()
                                });
                            if command.ends_with(' ') && !has_inline_args {
                                if let Some(editor) = self.palette.as_mut() {
                                    let completion =
                                        if parsed.category == Some(PaletteCategory::Actions) {
                                            palette::with_category(
                                                command,
                                                Some(PaletteCategory::Actions),
                                            )
                                        } else {
                                            command.clone()
                                        };
                                    let _ = editor.set(&completion);
                                }
                                return true;
                            }
                            *command = if has_inline_args {
                                typed_body.clone()
                            } else {
                                base
                            };
                        }
                        PaletteTarget::Slash(command) => {
                            let resolved = palette::resolve_slash(&typed_body, command);
                            if command.ends_with(' ') && !resolved.has_args {
                                if let Some(editor) = self.palette.as_mut() {
                                    let _ = editor.set(format!("/{command}"));
                                }
                                return true;
                            }
                            *command = resolved.command;
                        }
                        _ => {}
                    }
                    self.close_palette();
                    self.execute_palette_item(item, worker, browser);
                }
            }
        }
        self.palette_choices();
        true
    }

    pub(super) fn prompt_history(&self) -> Vec<String> {
        let turns = self.snapshot.turns.as_ref();
        let start = turns.len().saturating_sub(128);
        let mut history = Vec::with_capacity(turns.len().saturating_sub(start));
        for turn in &turns[start..] {
            if history.last().is_some_and(|last| last == &turn.prompt) {
                continue;
            }
            history.push(turn.prompt.clone());
        }
        history
    }

    pub(super) fn configure_active_viewport(&mut self, tab: Id, target: &Target) -> &mut Viewport {
        let total = self.line_counts.get(&tab).copied().unwrap_or(0);
        let height = self
            .selectable_views
            .get(&tab)
            .map_or(5, |view| usize::from(view.area.height).max(1));
        let follows_end = matches!(target, Target::Conversation | Target::Activity);
        let allow_overscroll = matches!(target, Target::File { .. } | Target::Diff { .. });
        let viewport = self.scroll.entry(tab).or_insert_with(|| {
            if follows_end {
                Viewport::following_end()
            } else {
                Viewport::default()
            }
        });
        viewport.configure(total, height, allow_overscroll);
        viewport
    }

    pub(super) fn scroll_by(&mut self, delta: isize) {
        let Some(tab) = self.layout.active(self.layout.focused).cloned() else {
            return;
        };
        if matches!(tab.target, Target::Terminal { .. }) {
            let offset = self.terminal_scroll.entry(tab.id).or_default();
            *offset = offset
                .saturating_add_signed(delta)
                .min(aw_terminal::SCROLLBACK);
            self.input = false;
            return;
        }
        self.configure_active_viewport(tab.id, &tab.target)
            .scroll(delta);
        self.input = false;
    }

    pub(super) fn page_scroll(&mut self, down: bool) {
        let Some(tab) = self.layout.active(self.layout.focused).cloned() else {
            return;
        };
        if matches!(tab.target, Target::Terminal { .. }) {
            let step = self.selectable_views.get(&tab.id).map_or(20, |view| {
                usize::from(view.area.height).saturating_sub(2).max(1)
            });
            self.scroll_by(if down {
                step as isize
            } else {
                -(step as isize)
            });
            return;
        }
        self.configure_active_viewport(tab.id, &tab.target)
            .page(down);
        self.input = false;
    }

    pub(super) fn jump_to_row(&mut self, tab: Id, row: usize) {
        let Some(target) = self.layout.tabs.get(&tab).map(|tab| tab.target.clone()) else {
            return;
        };
        let total = self
            .line_counts
            .get(&tab)
            .copied()
            .unwrap_or(0)
            .max(row.saturating_add(1));
        let height = self
            .selectable_views
            .get(&tab)
            .map_or(5, |view| usize::from(view.area.height).max(1));
        let follows_end = matches!(target, Target::Conversation | Target::Activity);
        let allow_overscroll = matches!(target, Target::File { .. } | Target::Diff { .. });
        let viewport = self.scroll.entry(tab).or_insert_with(|| {
            if follows_end {
                Viewport::following_end()
            } else {
                Viewport::default()
            }
        });
        viewport.configure(total, height, allow_overscroll);
        viewport.jump_to(row, SCROLLOFF_ROWS);
    }

    pub(super) fn scroll_home(&mut self) {
        let Some(tab) = self.layout.active(self.layout.focused).cloned() else {
            return;
        };
        self.configure_active_viewport(tab.id, &tab.target).home();
    }

    pub(super) fn scroll_end(&mut self) {
        let Some(tab) = self.layout.active(self.layout.focused).cloned() else {
            return;
        };
        self.configure_active_viewport(tab.id, &tab.target).end();
    }

    pub(super) fn navigator_open(&mut self, worker: &Worker, browser: &Browser) {
        let pane = self.layout.focused;
        let Some(Pane::Navigator { kind }) = self.layout.panes.get(&pane).cloned() else {
            return;
        };
        if kind == Navigator::Git {
            if let Err(e) = self.git.open(pane) {
                self.notice = e;
            }
            return;
        }

        self.nav.entry((pane, kind)).or_default();
        let selected = match kind {
            Navigator::Files => {
                let n = self
                    .nav
                    .get(&(pane, Navigator::Files))
                    .expect("files navigator state exists");
                Self::file_rows(n).get(n.selected).cloned()
            }
            Navigator::Sessions => {
                let selected = self
                    .nav
                    .get(&(pane, Navigator::Sessions))
                    .map_or(0, |state| state.selected);
                self.session_rows(pane).get(selected).cloned()
            }
            Navigator::Git => None,
        };
        let Some(selected) = selected else {
            return;
        };

        match selected.item {
            NavigatorItem::Parent => {
                let directory = self
                    .nav
                    .get(&(pane, Navigator::Files))
                    .map(|state| state.directory.clone())
                    .unwrap_or_default();
                let parent = directory
                    .rsplit_once('/')
                    .map_or(String::new(), |(path, _)| path.to_owned());
                self.list_files(browser, pane, parent);
            }
            NavigatorItem::File(entry) => match entry.kind {
                EntryKind::Directory => {
                    let tree = self
                        .nav
                        .get(&(pane, Navigator::Files))
                        .is_some_and(|state| state.display == NavDisplay::Tree);
                    if !tree {
                        self.list_files(browser, pane, entry.path);
                        return;
                    }
                    let cached = {
                        let state = self.nav.entry((pane, Navigator::Files)).or_default();
                        if state.expanded.remove(&entry.path) {
                            return;
                        }
                        state.expanded.insert(entry.path.clone());
                        state.directories.contains_key(&entry.path)
                    };
                    if !cached {
                        self.list_files(browser, pane, entry.path);
                    }
                }
                EntryKind::File => self.read_file(browser, entry.path),
                EntryKind::Unsupported => {
                    self.notice = "Unsupported/symlink/hardlinked entry was not followed.".into()
                }
            },
            NavigatorItem::UnsavedSession => {
                self.open(Target::Conversation);
                self.input = true;
            }
            NavigatorItem::SessionRoot { session } => {
                let state = self.nav.entry((pane, Navigator::Sessions)).or_default();
                if !state.collapsed.remove(&session) {
                    state.collapsed.insert(session);
                }
            }
            NavigatorItem::SessionBranch { session, branch } => {
                if self.owner(worker, Command::Resume { session, branch }, true) {
                    self.open(Target::Conversation);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_async_find_reports_only_its_validated_completion() {
        fn commit(ui: &mut Ui, tab: Id, query: &str) {
            let mut editor = Editor::default();
            editor.set(query).unwrap();
            ui.search = Some(SearchState {
                editor,
                target: SearchTarget::Main { tab },
                original: String::new(),
            });
            assert!(ui.search_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
            assert_eq!(ui.notice, "Searching…");
        }

        fn complete(ui: &mut Ui, tab: Id) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while ui.find_cancel.contains_key(&tab) && Instant::now() < deadline {
                while let Ok(result) = ui.background_rx.try_recv() {
                    ui.apply_background_result(result);
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(!ui.find_cancel.contains_key(&tab));
        }

        let mut ui = super::super::render_tests::model();
        let path = "src/find-status.txt".to_owned();
        ui.open(Target::File {
            scope_id: ScopeId::launch(),
            path: path.clone(),
        });
        let tab = ui.layout.active(ui.group()).unwrap().id;
        ui.file_lines.insert(
            path.into(),
            Arc::new(vec!["before".into(), "needle".into()]),
        );
        commit(&mut ui, tab, "needle");
        let generation = ui.find_generation[&tab];
        complete(&mut ui, tab);
        assert_eq!(ui.find[&tab].matches, vec![3]);
        assert_eq!(ui.notice, "1 match · press a flash label or n/N");

        // Even a same-generation duplicate cannot overwrite a completed search.
        ui.apply_background_result(UiBackgroundResult::Find {
            tab,
            generation,
            query: "needle".into(),
            matches: vec![],
            choosing: true,
            error: Some("stale failure"),
        });
        assert_eq!(ui.notice, "1 match · press a flash label or n/N");
        commit(&mut ui, tab, "absent");
        ui.apply_background_result(UiBackgroundResult::Find {
            tab,
            generation,
            query: "needle".into(),
            matches: vec![99],
            choosing: true,
            error: None,
        });
        assert_eq!(ui.notice, "Searching…");
        assert_eq!(ui.find[&tab].query, "absent");
        complete(&mut ui, tab);
        assert!(ui.find[&tab].matches.is_empty());
        assert_eq!(ui.notice, "No matches");

        // The executor's panic completion is a failure, never a zero-match claim.
        commit(&mut ui, tab, "needle");
        ui.apply_background_result(UiBackgroundResult::Find {
            tab,
            generation: ui.find_generation[&tab],
            query: "needle".into(),
            matches: vec![],
            choosing: true,
            error: Some("Find failed; try the search again"),
        });
        assert_eq!(ui.notice, "Find failed; try the search again");
        assert!(!ui.find_cancel.contains_key(&tab));
        assert!(ui.find[&tab].matches.is_empty());

        commit(&mut ui, tab, "needle");
        ui.open(Target::Help);
        complete(&mut ui, tab);
        assert!(
            ui.notice.is_empty(),
            "an inactive tab cannot publish a count"
        );
    }
}
