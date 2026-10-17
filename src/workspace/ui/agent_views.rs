//! Read-only, exact-resource views. The existing owner remains the only executor.
use super::super::branch_view;
use super::*;

const REFRESH: Duration = Duration::from_secs(1);
const FRESH_FOR: Duration = Duration::from_secs(3);
const MAX_CACHE_BYTES: usize = 8 * 1024 * 1024;
const MAX_VIEW_BYTES: usize = 2 * 1024 * 1024;
const MAX_RENDER_INPUT: usize = 64 * 1024;
const MAX_ROWS: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReadMode {
    Snapshot,
    Live,
}

pub(super) struct ViewCache {
    pub title: String,
    pub lines: Arc<Vec<Line<'static>>>,
    pub plain: Arc<Vec<String>>,
    bytes: usize,
}

pub(super) struct Pending {
    generation: u64,
    mode: ReadMode,
    cancel: Arc<AtomicBool>,
    started: Instant,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

pub(super) struct View {
    pub(super) target: branch_view::Target,
    generation: u64,
    pending: Option<Pending>,
    pub(super) cache: Option<ViewCache>,
    checked: Option<Instant>,
    attempted: Option<Instant>,
    live: bool,
    owner_generation: u64,
    width: u16,
    config: aw::config::UiConfig,
    error: String,
}

pub(super) struct Reply {
    pub tab: Id,
    pub target: branch_view::Target,
    pub generation: u64,
    pub mode: ReadMode,
    pub owner_generation: u64,
    pub result: std::result::Result<ViewCache, String>,
}

fn resource(target: &Target) -> Option<branch_view::Target> {
    match target {
        Target::AgentView {
            store_id,
            session_id,
            branch_id,
        } => Some(branch_view::Target {
            store_id: store_id.clone(),
            session_id: session_id.clone(),
            branch_id: branch_id.clone(),
        }),
        _ => None,
    }
}

fn bounded_turns(turns: &[aw::conversation::Turn]) -> (Vec<aw::conversation::Turn>, bool) {
    let mut out = Vec::new();
    let mut used = 0;
    let mut limited = false;
    for turn in turns.iter().rev().take(16) {
        let size = serde_json::to_vec(turn).map_or(MAX_RENDER_INPUT + 1, |bytes| bytes.len());
        if used + size <= MAX_RENDER_INPUT {
            used += size;
            out.push(turn.clone());
        } else if out.is_empty() {
            // Retain the authoritative answer when a single turn exceeds the
            // presentation budget. Partial traces are never presented as full.
            let mut tail = turn.clone();
            tail.prompt = aw::presentation::tail_text(&tail.prompt, 8 * 1024).to_owned();
            tail.answer = aw::presentation::tail_text(&tail.answer, 16 * 1024).to_owned();
            tail.model = aw::presentation::tail_text(&tail.model, 256).to_owned();
            tail.profile = aw::presentation::tail_text(&tail.profile, 256).to_owned();
            tail.error = tail
                .error
                .map(|error| aw::presentation::tail_text(&error, 256).to_owned());
            tail.context_messages.clear();
            tail.tool_records.clear();
            tail.invocations.clear();
            out.push(tail);
            limited = true;
            break;
        } else {
            limited = true;
            break;
        }
    }
    limited |= out.len() < turns.len();
    out.reverse();
    (out, limited)
}

fn lines_bytes(lines: &Vec<Line<'static>>, plain: &Vec<String>, title: &String) -> usize {
    std::mem::size_of::<ViewCache>()
        + title.capacity()
        + lines.capacity() * std::mem::size_of::<Line<'static>>()
        + lines
            .iter()
            .map(|line| {
                line.spans.capacity() * std::mem::size_of::<Span<'static>>()
                    + line
                        .spans
                        .iter()
                        .map(|span| match &span.content {
                            std::borrow::Cow::Owned(text) => text.capacity(),
                            _ => 0,
                        })
                        .sum::<usize>()
            })
            .sum::<usize>()
        + plain.capacity() * std::mem::size_of::<String>()
        + plain.iter().map(String::capacity).sum::<usize>()
}

fn cache(title: String, mut lines: Vec<Line<'static>>) -> ViewCache {
    // Keep a bounded recent display, with both styled and searchable owned
    // allocations included. The immutable Store history remains authoritative.
    let mut bytes = 0usize;
    let mut keep = 0;
    for line in lines.iter().rev().take(MAX_ROWS - 1) {
        let cost = std::mem::size_of::<Line<'static>>()
            + line.spans.capacity() * std::mem::size_of::<Span<'static>>()
            + line
                .spans
                .iter()
                .map(|span| span.content.len() * 2)
                .sum::<usize>()
            + std::mem::size_of::<String>();
        if bytes.saturating_add(cost) > MAX_VIEW_BYTES - 2048 {
            break;
        }
        bytes += cost;
        keep += 1;
    }
    if keep < lines.len() {
        lines.drain(..lines.len() - keep);
        lines.insert(
            0,
            Line::raw(
                "[Read-only recent display bounded; use history/export for complete records.]",
            ),
        );
    }
    lines.shrink_to_fit();
    lines.shrink_to_fit();
    let mut plain = crate::tui::markdown::plain_lines(&lines);
    while lines_bytes(&lines, &plain, &title) > MAX_VIEW_BYTES && lines.len() > 2 {
        lines.remove(1);
        plain.remove(1);
    }
    let bytes = lines_bytes(&lines, &plain, &title);
    ViewCache {
        title,
        lines: Arc::new(lines),
        plain: Arc::new(plain),
        bytes,
    }
}

fn render_snapshot(
    snapshot: branch_view::Snapshot,
    ui: &aw::config::UiConfig,
    width: u16,
) -> std::result::Result<ViewCache, String> {
    let (turns, limited) = bounded_turns(&snapshot.turns);
    let resources = crate::tui::agent_resources::LoadedResourceSummary::default()
        .with_context_blocks(&snapshot.context_blocks);
    let mut lines = vec![Line::raw(format!(
        "Read only · {} · {} · revision {}{}",
        snapshot.title,
        snapshot.branch_name,
        snapshot.revision,
        if snapshot.needs_inspection {
            " · inspection required"
        } else {
            ""
        }
    ))];
    lines.push(Line::raw(format!(
        "Session {} · branch {} · head {}",
        snapshot.target.session_id, snapshot.target.branch_id, snapshot.head
    )));
    if limited || snapshot.retained_bytes > MAX_RENDER_INPUT {
        lines.push(Line::raw(
            "[Recent transcript display; earlier/oversized content remains in history/export.]",
        ));
    }
    lines.extend(super::super::agent_view::settled_lines(
        &turns, &resources, ui, None, width, false,
    ));
    Ok(cache(
        format!("{} · {}", snapshot.title, snapshot.branch_name),
        lines,
    ))
}

impl Ui {
    fn current_resource(&self, target: &branch_view::Target) -> bool {
        self.snapshot.store_id.as_deref() == Some(target.store_id.as_str())
            && self.snapshot.session.as_deref() == Some(target.session_id.as_str())
            && self.snapshot.branch_id.as_deref() == Some(target.branch_id.as_str())
    }

    pub(super) fn open_selected_agent_view(&mut self) {
        let pane = self.layout.focused;
        if !matches!(
            self.layout.panes.get(&pane),
            Some(Pane::Navigator {
                kind: Navigator::Sessions,
                ..
            })
        ) {
            self.notice = "Select an exact branch in the Sessions navigator, then Open read-only Agent view (o).".into();
            return;
        }
        self.nav.entry((pane, Navigator::Sessions)).or_default();
        let selected = self
            .nav
            .get(&(pane, Navigator::Sessions))
            .map_or(0, |state| state.selected);
        let Some(NavigatorRow {
            item: NavigatorItem::SessionBranch { session, branch },
            ..
        }) = self.session_rows(pane).get(selected).cloned()
        else {
            self.notice = "Select a saved branch to open its read-only view; Enter still activates the Agent.".into();
            return;
        };
        let Some(store_id) = self.snapshot.store_id.clone() else {
            self.notice =
                "Read-only Agent views require an existing local SQLite State reader.".into();
            return;
        };
        let target = Target::AgentView {
            store_id,
            session_id: session,
            branch_id: branch,
        };
        self.open(target.clone());
        if self
            .layout
            .active(self.layout.focused)
            .is_some_and(|tab| tab.target == target)
        {
            self.input = false;
            self.notice = "Read-only Agent view opened; active Agent and draft unchanged.".into();
        }
    }

    fn clear_agent_view_content(&mut self, tab: Id) {
        if let Some(view) = self.agent_views.get_mut(&tab) {
            view.cache = None;
            view.checked = None;
            view.live = false;
        }
        self.rendered.remove(&tab);
        self.selectable_views.remove(&tab);
        if let Some(cancel) = self.find_cancel.remove(&tab) {
            cancel.store(true, Ordering::Release);
        }
        self.find_generation.remove(&tab);
        if let Some(find) = self.find.get_mut(&tab) {
            find.matches.clear();
            find.selected = 0;
            find.choosing = false;
        }
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

    pub(super) fn reconcile_agent_views(&mut self) {
        let removed = self
            .agent_views
            .iter()
            .filter_map(|(id, view)| {
                (self
                    .layout
                    .tabs
                    .get(id)
                    .and_then(|tab| resource(&tab.target))
                    .as_ref()
                    != Some(&view.target))
                .then_some(*id)
            })
            .collect::<Vec<_>>();
        for id in removed {
            self.clear_agent_view_content(id);
            self.agent_views.remove(&id);
        }
        self.saved_selections
            .retain(|id, _| self.layout.tabs.contains_key(id));
        if let Some(selection) = self.text_selection.take() {
            if self.layout.tabs.contains_key(&selection.tab) {
                if self
                    .layout
                    .active(self.layout.focused)
                    .is_none_or(|tab| tab.id != selection.tab)
                {
                    self.selection_dragging = false;
                    self.selection_moved = false;
                }
                self.saved_selections.insert(selection.tab, selection);
            } else {
                self.selection_dragging = false;
                self.selection_moved = false;
            }
        }
        if let Some(tab) = self.layout.active(self.layout.focused) {
            self.text_selection = self.saved_selections.get(&tab.id).cloned();
        }
    }

    pub(super) fn expire_agent_views(&mut self) {
        let stale = self
            .agent_views
            .iter()
            .filter_map(|(id, view)| {
                ((!self.published_source_matches(&view.target))
                    || (view.live && !self.current_resource(&view.target))
                    || view.checked.is_some_and(|at| at.elapsed() >= FRESH_FOR))
                .then_some(*id)
            })
            .collect::<Vec<_>>();
        for id in stale {
            self.clear_agent_view_content(id);
            if let Some(view) = self.agent_views.get_mut(&id) {
                view.pending = None;
                view.attempted = None;
                view.error = "Revalidating read-only view…".into();
            }
        }
    }

    pub(super) fn schedule_agent_views(&mut self) {
        self.expire_agent_views();
        let visible = self
            .selectable_views
            .iter()
            .filter_map(|(id, view)| {
                self.layout
                    .tabs
                    .get(id)
                    .and_then(|tab| resource(&tab.target))
                    .map(|target| (*id, target, view.area.width))
            })
            .collect::<Vec<_>>();
        for (tab, target, width) in visible {
            if self
                .agent_views
                .values()
                .filter(|view| view.pending.is_some())
                .count()
                >= 4
            {
                break;
            }
            let live = self.current_resource(&target);
            let owner_generation = self.snapshot.generation;
            let state = self.agent_views.entry(tab).or_insert_with(|| View {
                target: target.clone(),
                generation: 0,
                pending: None,
                cache: None,
                checked: None,
                attempted: None,
                live: false,
                owner_generation: 0,
                width,
                config: self.ui_config.clone(),
                error: "Loading authorized read-only view…".into(),
            });
            if state.pending.is_some() {
                continue;
            }
            let geometry_changed = state.width != width || state.config != self.ui_config;
            if !geometry_changed
                && state.attempted.is_some_and(|at| at.elapsed() < REFRESH)
                && !(live && state.owner_generation != owner_generation)
            {
                continue;
            }
            let Some(reader) = self.branch_reader.clone() else {
                state.error =
                    "Read-only Agent views unavailable: no existing local SQLite reader.".into();
                continue;
            };
            if geometry_changed {
                self.clear_agent_view_content(tab);
            }
            let Some(generation) = self.agent_view_generation.checked_add(1) else {
                self.clear_agent_view_content(tab);
                self.agent_views.get_mut(&tab).expect("view inserted").error =
                    "Read-only view request IDs exhausted; restart the workspace.".into();
                continue;
            };
            self.agent_view_generation = generation;
            let state = self.agent_views.get_mut(&tab).expect("view inserted");
            state.generation = generation;
            state.width = width;
            state.config = self.ui_config.clone();
            state.attempted = Some(Instant::now());
            let generation = state.generation;
            let mode = if live {
                ReadMode::Live
            } else {
                ReadMode::Snapshot
            };
            let cancel = Arc::new(AtomicBool::new(false));
            state.pending = Some(Pending {
                generation,
                mode,
                cancel: cancel.clone(),
                started: Instant::now(),
            });
            let ui = self.ui_config.clone();
            // Arc transcript clones are bounded owner projections; only these
            // presentation fields cross the read executor, never owner commands.
            let live_data = live.then(|| {
                (
                    self.snapshot.turns.clone(),
                    self.snapshot.loaded_resources.clone(),
                    self.snapshot.pending_prompt.clone(),
                    self.snapshot.live.clone(),
                    self.snapshot.live_tools.clone(),
                    self.snapshot
                        .session_title
                        .clone()
                        .unwrap_or_else(|| "Agent".into()),
                    self.snapshot.branch.clone(),
                )
            });
            let out = self.background_tx.clone();
            let panic_out = out.clone();
            let panic_target = target.clone();
            if let Err(error) = self.background.try_submit_with_panic("agent-view", move || {
                if cancel.load(Ordering::Acquire) { return; }
                let result = if let Some((turns, resources, pending, answer, tools, title, branch)) = live_data {
                    reader.authorize(&target).map_err(|error| error.to_string()).and_then(|()| {
                        let (turns, limited) = bounded_turns(&turns);
                        let pending = aw::presentation::tail_text(&pending, 8 * 1024);
                        let answer = aw::presentation::tail_text(&answer, 16 * 1024);
                        let tools_size = serde_json::to_vec(&tools).map_or(MAX_RENDER_INPUT + 1, |bytes| bytes.len());
                        let tools = if tools_size <= MAX_RENDER_INPUT { &tools[..tools.len().min(8)] } else { &[] };
                        let mut lines = vec![Line::raw(format!("Read only · {title} · {branch} · exact current Agent branch")),
                            Line::raw(format!("Session {} · branch {}", target.session_id, target.branch_id))];
                        if limited { lines.push(Line::raw("[Recent transcript display; full history/export is unchanged.]")); }
                        lines.extend(super::super::agent_view::settled_lines(&turns, &resources, &ui, None, width, false));
                        if !pending.is_empty() {
                            let inner = usize::from(width).saturating_sub(usize::from(ui.workspace_output_padding) * 2).max(1);
                            lines.extend(crate::tui::agent_chrome::prompt_lines(pending, &ui, inner));
                            lines.extend(crate::tui::agent_chrome::live_lines(&crate::tui::agent_chrome::LiveProjection { answer, tools }, &ui, inner));
                        }
                        if tools_size > MAX_RENDER_INPUT { lines.push(Line::raw("[Live tool details bounded; inspect the current Agent for complete details.]")); }
                        Ok(cache(format!("{title} · {branch}"), lines))
                    })
                } else {
                    reader.read(&target).map_err(|error| error.to_string()).and_then(|snapshot| render_snapshot(snapshot, &ui, width))
                };
                if !cancel.load(Ordering::Acquire) { let _ = out.blocking_send(UiBackgroundResult::AgentView(Reply { tab, target, generation, mode, owner_generation, result })); }
            }, move || {
                let _ = panic_out.blocking_send(UiBackgroundResult::AgentView(Reply { tab, target: panic_target, generation, mode, owner_generation, result: Err("Read-only view reader failed; no Agent action was taken.".into()) }));
            }) {
                if let Some(state) = self.agent_views.get_mut(&tab) { state.pending = None; state.error = error.to_string(); }
            }
        }
    }

    pub(super) fn apply_agent_view_reply(&mut self, reply: Reply) {
        let Some(view) = self.agent_views.get(&reply.tab) else {
            return;
        };
        if view.target != reply.target
            || self
                .layout
                .tabs
                .get(&reply.tab)
                .and_then(|tab| resource(&tab.target))
                .as_ref()
                != Some(&reply.target)
            || view.pending.as_ref().is_none_or(|pending| {
                pending.generation != reply.generation
                    || pending.mode != reply.mode
                    || pending.cancel.load(Ordering::Acquire)
            })
        {
            return;
        }
        if !self.published_source_matches(&reply.target) {
            self.clear_agent_view_content(reply.tab);
            let view = self
                .agent_views
                .get_mut(&reply.tab)
                .expect("validated view");
            view.pending = None;
            view.attempted = None;
            view.error = "Read-only view unavailable: the State reader changed.".into();
            return;
        }
        if reply.mode == ReadMode::Live
            && (!self.current_resource(&reply.target)
                || self.snapshot.generation < reply.owner_generation)
        {
            self.clear_agent_view_content(reply.tab);
            let view = self
                .agent_views
                .get_mut(&reply.tab)
                .expect("validated view");
            view.pending = None;
            view.attempted = None;
            return;
        }
        let started = view.pending.as_ref().expect("pending checked").started;
        if started.elapsed() >= FRESH_FOR {
            self.clear_agent_view_content(reply.tab);
            let view = self
                .agent_views
                .get_mut(&reply.tab)
                .expect("validated view");
            view.pending = None;
            view.attempted = None;
            view.error = "Read-only view reply expired; revalidating…".into();
            return;
        }
        let old_plain = view.cache.as_ref().map(|cache| cache.plain.clone());
        self.agent_views
            .get_mut(&reply.tab)
            .expect("validated view")
            .pending = None;
        let other_bytes: usize = self
            .agent_views
            .iter()
            .filter(|(id, _)| **id != reply.tab)
            .filter_map(|(_, view)| view.cache.as_ref())
            .map(|cache| cache.bytes)
            .sum();
        let result = reply.result.and_then(|cache| {
            if other_bytes + cache.bytes <= MAX_CACHE_BYTES {
                Ok(cache)
            } else {
                Err("Read-only view cache is full (8 MiB); close another view to refresh.".into())
            }
        });
        match result {
            Ok(cache) => {
                let changed = old_plain
                    .as_ref()
                    .is_none_or(|plain| **plain != *cache.plain);
                if changed {
                    self.clear_agent_view_content(reply.tab);
                }
                let view = self
                    .agent_views
                    .get_mut(&reply.tab)
                    .expect("validated view");
                view.cache = Some(cache);
                view.checked = Some(started);
                view.live = reply.mode == ReadMode::Live;
                view.owner_generation = reply.owner_generation;
                view.error.clear();
                if changed {
                    if let Some(find) = self
                        .find
                        .get(&reply.tab)
                        .filter(|find| !find.query.is_empty())
                    {
                        let query = find.query.clone();
                        let committed = find.committed;
                        let target = self.layout.tabs[&reply.tab].target.clone();
                        self.schedule_find(reply.tab, target, query, committed);
                    }
                }
            }
            Err(error) => {
                self.clear_agent_view_content(reply.tab);
                self.agent_views
                    .get_mut(&reply.tab)
                    .expect("validated view")
                    .error = format!("Read-only view unavailable: {error}");
            }
        }
    }

    fn published_source_matches(&self, target: &branch_view::Target) -> bool {
        self.branch_reader
            .as_ref()
            .is_some_and(|reader| reader.store_id() == target.store_id)
    }

    pub(super) fn agent_view_fresh(&self, tab: Id) -> bool {
        self.agent_views.get(&tab).is_some_and(|view| {
            self.layout
                .tabs
                .get(&tab)
                .and_then(|tab| resource(&tab.target))
                .as_ref()
                == Some(&view.target)
                && self.published_source_matches(&view.target)
                && view.cache.is_some()
                && view.checked.is_some_and(|at| at.elapsed() < FRESH_FOR)
                && (!view.live || self.current_resource(&view.target))
        })
    }

    pub(super) fn agent_view_lines(&self, tab: Id) -> Arc<Vec<Line<'static>>> {
        match self.agent_views.get(&tab) {
            Some(view) if self.agent_view_fresh(tab) => view
                .cache
                .as_ref()
                .expect("fresh cache checked")
                .lines
                .clone(),
            Some(view) => Arc::new(vec![Line::raw(
                if !self.published_source_matches(&view.target) {
                    "Read-only view unavailable: the State reader changed.".into()
                } else if view.error.is_empty() {
                    "Revalidating read-only view…".into()
                } else {
                    view.error.clone()
                },
            )]),
            None => Arc::new(vec![Line::raw("Loading authorized read-only view…")]),
        }
    }

    pub(super) fn agent_view_plain(&self, tab: Id) -> Arc<Vec<String>> {
        if self.agent_view_fresh(tab) {
            return self.agent_views[&tab]
                .cache
                .as_ref()
                .expect("fresh cache checked")
                .plain
                .clone();
        }
        Arc::new(crate::tui::markdown::plain_lines(
            &self.agent_view_lines(tab),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::{HistoryState, PrincipalId};
    use aw_store::{SessionBinding, SessionStore};
    use aw_store_api::{PortableSession, PortableSessionSource, SessionBackend, StoreKind};
    use ratatui::backend::TestBackend;

    fn seed(store: &SessionStore, owner: &PrincipalId, member: &PrincipalId, word: &str) -> Target {
        let answer = format!("answer {word}\n{}", "readable row\n".repeat(40));
        let turn = super::super::render_tests::turn_with_trace(
            &format!("question {word}"),
            &answer,
            aw::conversation::Outcome::Completed,
            None,
            Some(&answer),
        );
        let document = PortableSession {
            version: 1,
            title: "Same title".into(),
            cwd: None,
            source: PortableSessionSource {
                backend: StoreKind::Sqlite,
                session_id: "seed".into(),
                branch_id: "seed".into(),
                parent_session: None,
            },
            history: HistoryState {
                turns: vec![turn],
                next_turn_id: 2,
                ..HistoryState::default()
            },
        };
        let (session, branch, _) = store
            .import_document(
                owner,
                document,
                SessionBinding {
                    working_directory: PathBuf::from("/fixture"),
                    tool_root: PathBuf::from("/fixture"),
                    system_prompt: String::new(),
                    profile_hint: "fixture".into(),
                    model_hint: "fixture".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        store
            .grant(owner, &session.id, member, Some(false))
            .unwrap();
        Target::AgentView {
            store_id: store.store_id().unwrap(),
            session_id: session.id,
            branch_id: branch.id,
        }
    }

    fn fixture() -> (Ui, SessionStore, PrincipalId, PrincipalId, Target, Target) {
        let store = SessionStore::in_memory().unwrap();
        let owner = PrincipalId::new("fixture-owner").unwrap();
        let member = PrincipalId::new("fixture-reader").unwrap();
        let a = seed(&store, &owner, &member, "ALPHA");
        let b = seed(&store, &owner, &member, "BETA");
        let mut ui = super::super::render_tests::model();
        ui.branch_reader = Some(branch_view::Reader::new(store.clone(), member.clone()).unwrap());
        ui.snapshot.store_id = Some(store.store_id().unwrap());
        ui.snapshot.session = Some("paused-current-owner".into());
        ui.snapshot.branch_id = Some("paused-current-branch".into());
        ui.snapshot.busy = true;
        ui.snapshot.state_pending = true;
        ui.snapshot.pending_prompt = "Owner's pending turn stays local".into();
        ui.draft.left();
        (ui, store, owner, member, a, b)
    }

    fn draw(ui: &mut Ui) {
        let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
        terminal.draw(|frame| ui.draw(frame)).unwrap();
    }

    fn replies(ui: &mut Ui, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = 0;
        while seen < count && Instant::now() < deadline {
            if let Ok(reply) = ui.background_rx.try_recv() {
                if matches!(reply, UiBackgroundResult::AgentView(_)) {
                    seen += 1;
                }
                ui.apply_background_result(reply);
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        assert_eq!(seen, count, "actual bounded reader jobs completed");
    }

    fn read_two(ui: &mut Ui, a: Target, b: Target) -> (Id, Id) {
        ui.open(a);
        let group_a = ui.layout.focused;
        let tab_a = ui.layout.active(group_a).unwrap().id;
        ui.apply(Action::SplitGroup {
            pane: group_a,
            side: Side::Right,
        });
        ui.open(b);
        let tab_b = ui.layout.active(ui.layout.focused).unwrap().id;
        draw(ui);
        ui.schedule_agent_views();
        replies(ui, 2);
        draw(ui);
        (tab_a, tab_b)
    }

    #[test]
    fn readonly_agent_view_refresh_preserves_committed_find_until_consumed() {
        fn finish_find(ui: &mut Ui, tab: Id) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while ui.find_cancel.contains_key(&tab) && Instant::now() < deadline {
                if let Ok(reply) = ui.background_rx.try_recv() {
                    ui.apply_background_result(reply);
                } else {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            assert!(!ui.find_cancel.contains_key(&tab));
        }

        let (mut ui, _, _, _, a, _) = fixture();
        let target = resource(&a).unwrap();
        ui.snapshot.session = Some(target.session_id);
        ui.snapshot.branch_id = Some(target.branch_id);
        ui.snapshot.live = "ALPHA initial".into();
        ui.open(a);
        let tab = ui.layout.active(ui.layout.focused).unwrap().id;
        let view_group = ui.layout.focused;
        ui.apply(Action::SplitGroup {
            pane: view_group,
            side: Side::Right,
        });
        ui.open(Target::Help);
        let other_group = ui.layout.focused;
        ui.apply(Action::Focus(view_group));
        draw(&mut ui);
        ui.schedule_agent_views();
        replies(&mut ui, 1);
        draw(&mut ui);

        // A genuine changed, authorized live frame can already be waiting when
        // Enter commits Find. Deliver that frame before either Find completion.
        ui.snapshot.live = "ALPHA changed".into();
        ui.snapshot.generation += 1;
        ui.schedule_agent_views();
        let deadline = Instant::now() + Duration::from_secs(5);
        let frame = loop {
            if let Ok(UiBackgroundResult::AgentView(reply)) = ui.background_rx.try_recv() {
                break reply;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        ui.update_find(tab, "ALPHA".into(), true);
        assert_eq!(ui.notice, "Searching…");
        let cancelled = ui.find_cancel[&tab].clone();
        let stale_generation = ui.find_generation[&tab];
        ui.apply_agent_view_reply(frame);
        assert!(cancelled.load(Ordering::Acquire));
        assert_ne!(ui.find_generation[&tab], stale_generation);
        finish_find(&mut ui, tab);
        assert_eq!(ui.find[&tab].matches.len(), 1);
        assert_eq!(ui.notice, "1 match · press a flash label or n/N");
        assert!(ui.find[&tab].choosing);

        // A committed miss retains its intent too: later live content may add
        // a match, even though there was no active flash selection beforehand.
        ui.update_find(tab, "GAMMA".into(), true);
        finish_find(&mut ui, tab);
        assert_eq!(ui.notice, "No matches");
        assert!(!ui.find[&tab].choosing);
        draw(&mut ui);
        ui.apply(Action::Focus(other_group));
        ui.notice = "Other pane retains its status".into();
        ui.snapshot.live = "GAMMA newly arrived".into();
        ui.snapshot.generation += 1;
        ui.schedule_agent_views();
        replies(&mut ui, 1);
        assert_eq!(ui.notice, "Other pane retains its status");
        finish_find(&mut ui, tab);
        assert_eq!(ui.find[&tab].matches.len(), 1);
        assert_eq!(ui.layout.focused, other_group);
        assert_eq!(ui.notice, "Other pane retains its status");
        assert!(ui.find[&tab].choosing);
        ui.apply(Action::Focus(view_group));

        // Once n/N or a flash label consumes that mode, another live refresh
        // keeps the query but must not capture the user's ordinary keys again.
        assert!(ui.cycle_find(true));
        draw(&mut ui);
        ui.snapshot.live = "GAMMA another refresh".into();
        ui.snapshot.generation += 1;
        ui.schedule_agent_views();
        replies(&mut ui, 1);
        finish_find(&mut ui, tab);
        assert_eq!(ui.find[&tab].matches.len(), 1);
        assert!(!ui.find[&tab].choosing);
    }

    #[test]
    fn readonly_agent_view_restored_tab_rejects_find_from_previous_lifetime() {
        let (mut ui, _, _, _, a, b) = fixture();
        ui.open(a);
        let tab = ui.layout.active(ui.layout.focused).unwrap().id;
        let saved = ui.layout.clone();
        draw(&mut ui);
        ui.schedule_agent_views();
        replies(&mut ui, 1);
        draw(&mut ui);
        ui.update_find(tab, "ALPHA".into(), true);
        let deadline = Instant::now() + Duration::from_secs(5);
        let old = loop {
            if let Ok(reply @ UiBackgroundResult::Find { .. }) = ui.background_rx.try_recv() {
                break reply;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        assert!(matches!(&old, UiBackgroundResult::Find { matches, .. } if !matches.is_empty()));

        ui.apply(Action::CloseTab {
            group: ui.layout.group_for(tab).unwrap(),
            tab,
        });
        ui.layout = saved;
        ui.layout.tabs.get_mut(&tab).unwrap().target = b;
        ui.reconcile_layout_state();
        draw(&mut ui);
        ui.schedule_agent_views();
        replies(&mut ui, 1);
        draw(&mut ui);
        ui.update_find(tab, "ALPHA".into(), true);
        ui.apply_background_result(old);
        assert!(
            ui.find[&tab].matches.is_empty(),
            "old resource's match rows stay inert"
        );
        assert!(
            ui.find_cancel.contains_key(&tab),
            "old lifetime cannot settle the replacement search"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while ui.find_cancel.contains_key(&tab) && Instant::now() < deadline {
            if let Ok(reply) = ui.background_rx.try_recv() {
                ui.apply_background_result(reply);
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        assert!(!ui.find_cancel.contains_key(&tab));
        assert!(ui.find[&tab].matches.is_empty());
        assert_eq!(ui.notice, "No matches");
    }

    #[test]
    fn readonly_agent_views_two_real_targets_keep_owner_draft_and_independent_state() {
        let (mut ui, store, owner, _, a, b) = fixture();
        let draft = (ui.draft.text().to_owned(), ui.draft.cursor());
        let agent = (
            ui.snapshot.session.clone(),
            ui.snapshot.branch_id.clone(),
            ui.snapshot.pending_prompt.clone(),
            ui.snapshot.generation,
        );
        let sessions = store.list(&owner).unwrap();
        let heads = sessions
            .iter()
            .map(|session| store.branches(&owner, &session.id).unwrap())
            .collect::<Vec<_>>();
        let (ta, tb) = read_two(&mut ui, a, b);
        assert!(ui.agent_view_plain(ta).join("\n").contains("ALPHA"));
        assert!(!ui.agent_view_plain(ta).join("\n").contains("BETA"));
        assert!(ui.agent_view_plain(tb).join("\n").contains("BETA"));
        assert!(!ui.can_submit && !ui.input, "pinned views have no composer");
        ui.scroll.get_mut(&ta).unwrap().scroll(5);
        assert_ne!(ui.scroll[&ta].offset(), ui.scroll[&tb].offset());
        assert!(ui.schedule_find(
            ta,
            ui.layout.tabs[&ta].target.clone(),
            "ALPHA".into(),
            false
        ));
        assert!(ui.schedule_find(tb, ui.layout.tabs[&tb].target.clone(), "BETA".into(), false));
        let deadline = Instant::now() + Duration::from_secs(5);
        while (ui.find[&ta].matches.is_empty() || ui.find[&tb].matches.is_empty())
            && Instant::now() < deadline
        {
            if let Ok(reply) = ui.background_rx.try_recv() {
                ui.apply_background_result(reply);
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        assert!(!ui.find[&ta].matches.is_empty() && !ui.find[&tb].matches.is_empty());
        let ga = ui.layout.group_for(ta).unwrap();
        let gb = ui.layout.group_for(tb).unwrap();
        ui.apply(Action::Focus(ga));
        ui.text_selection = Some(TextSelection {
            tab: ta,
            anchor: TextPoint { line: 0, col: 0 },
            current: TextPoint { line: 0, col: 3 },
        });
        ui.apply(Action::Focus(gb));
        ui.text_selection = Some(TextSelection {
            tab: tb,
            anchor: TextPoint { line: 1, col: 0 },
            current: TextPoint { line: 1, col: 5 },
        });
        ui.apply(Action::Focus(ga));
        assert_eq!(ui.text_selection.as_ref().unwrap().tab, ta);
        assert_eq!(ui.saved_selections[&tb].anchor.line, 1);
        assert_eq!((ui.draft.text().to_owned(), ui.draft.cursor()), draft);
        assert_eq!(
            (
                ui.snapshot.session.clone(),
                ui.snapshot.branch_id.clone(),
                ui.snapshot.pending_prompt.clone(),
                ui.snapshot.generation
            ),
            agent
        );
        for (session, before) in sessions.iter().zip(heads) {
            let after = store.branches(&owner, &session.id).unwrap();
            assert_eq!(after[0].head, before[0].head);
            assert_eq!(after[0].revision, before[0].revision);
            assert!(after[0].active_run.is_none());
            assert_eq!(store.usage(&owner, &session.id).unwrap().model_calls, 0);
        }
    }

    #[test]
    fn readonly_agent_views_revocation_without_head_change_erases_derived_state() {
        let (mut ui, store, owner, member, a, b) = fixture();
        let target = resource(&a).unwrap();
        let head = store.branches(&owner, &target.session_id).unwrap()[0]
            .head
            .clone();
        let (ta, tb) = read_two(&mut ui, a, b);
        ui.find.insert(
            ta,
            FindState {
                query: "ALPHA".into(),
                matches: vec![1],
                selected: 0,
                choosing: false,
                committed: false,
            },
        );
        ui.saved_selections.insert(
            ta,
            TextSelection {
                tab: ta,
                anchor: TextPoint { line: 0, col: 0 },
                current: TextPoint { line: 0, col: 2 },
            },
        );
        store
            .grant(&owner, &target.session_id, &member, None)
            .unwrap();
        ui.agent_views.get_mut(&ta).unwrap().attempted = None;
        ui.schedule_agent_views();
        replies(&mut ui, 1);
        assert!(ui.agent_view_plain(ta).join("\n").contains("unavailable"));
        assert!(!ui.agent_view_plain(ta).join("\n").contains("ALPHA"));
        assert!(ui.find[&ta].matches.is_empty());
        assert!(!ui.saved_selections.contains_key(&ta));
        assert!(!ui.selectable_views.contains_key(&ta));
        assert!(ui.agent_view_plain(tb).join("\n").contains("BETA"));
        assert_eq!(
            store.branches(&owner, &target.session_id).unwrap()[0].head,
            head
        );
    }

    #[test]
    fn readonly_agent_views_closed_delayed_and_wrong_resource_replies_stay_inert() {
        let (mut ui, _, _, _, a, _) = fixture();
        ui.open(a);
        let tab = ui.layout.active(ui.layout.focused).unwrap().id;
        draw(&mut ui);
        ui.schedule_agent_views();
        let deadline = Instant::now() + Duration::from_secs(5);
        let reply = loop {
            if let Ok(UiBackgroundResult::AgentView(reply)) = ui.background_rx.try_recv() {
                break reply;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        ui.agent_views
            .get_mut(&tab)
            .unwrap()
            .pending
            .as_mut()
            .unwrap()
            .started = Instant::now() - FRESH_FOR;
        ui.apply_agent_view_reply(reply);
        assert!(
            ui.agent_views[&tab].cache.is_none(),
            "delayed initial read never renews stale authorization"
        );
        draw(&mut ui);
        ui.schedule_agent_views();
        let pending = ui.agent_views[&tab].pending.as_ref().unwrap();
        let mut wrong = ui.agent_views[&tab].target.clone();
        wrong.branch_id = "wrong-branch".into();
        let generation = pending.generation;
        ui.apply_agent_view_reply(Reply {
            tab,
            target: wrong,
            generation,
            mode: ReadMode::Snapshot,
            owner_generation: ui.snapshot.generation,
            result: Ok(cache("wrong".into(), vec![Line::raw("WRONG CONTENT")])),
        });
        assert!(ui.agent_views[&tab].cache.is_none());
        let target = ui.agent_views[&tab].target.clone();
        let focus = ui.layout.focused;
        ui.apply(Action::CloseTab {
            group: ui.layout.group_for(tab).unwrap(),
            tab,
        });
        ui.apply_agent_view_reply(Reply {
            tab,
            target,
            generation,
            mode: ReadMode::Snapshot,
            owner_generation: 0,
            result: Ok(cache("closed".into(), vec![Line::raw("CLOSED CONTENT")])),
        });
        assert!(!ui.agent_views.contains_key(&tab));
        assert_eq!(ui.layout.focused, focus);
        assert!(!ui.selectable_views.contains_key(&tab));
    }

    #[test]
    fn readonly_agent_views_current_live_frame_survives_token_generation_and_drops_on_identity_change(
    ) {
        let (mut ui, _, _, _, a, _) = fixture();
        let target = resource(&a).unwrap();
        ui.snapshot.session = Some(target.session_id.clone());
        ui.snapshot.branch_id = Some(target.branch_id.clone());
        ui.snapshot.live = "Authorized live answer".into();
        ui.open(a);
        let tab = ui.layout.active(ui.layout.focused).unwrap().id;
        draw(&mut ui);
        ui.schedule_agent_views();
        ui.snapshot.generation += 100; // ordinary token updates do not change authority/resource
        replies(&mut ui, 1);
        assert!(ui
            .agent_view_plain(tab)
            .join("\n")
            .contains("Authorized live answer"));
        ui.snapshot.branch_id = Some("different-owner-branch".into());
        ui.expire_agent_views();
        assert!(ui.agent_views[&tab].cache.is_none());
        assert!(!ui
            .agent_view_plain(tab)
            .join("\n")
            .contains("Authorized live answer"));
        draw(&mut ui);
        ui.schedule_agent_views();
        // A repeated scheduler pass must not cancel the replacement strict read
        // because the previous cache used to follow the current owner.
        ui.schedule_agent_views();
        replies(&mut ui, 1);
        assert!(ui.agent_view_plain(tab).join("\n").contains("ALPHA"));
        assert!(!ui.agent_views[&tab].live);
    }

    #[test]
    fn readonly_agent_views_large_unicode_history_keeps_bounded_recent_answer() {
        let mut turn = super::super::render_tests::turn_with_trace(
            "large prompt",
            &"界🙂".repeat(100_000),
            aw::conversation::Outcome::Completed,
            None,
            None,
        );
        turn.answer.push_str("LATEST ANSWER");
        let (turns, limited) = bounded_turns(&[turn]);
        assert!(limited);
        assert!(turns[0].answer.ends_with("LATEST ANSWER"));
        assert!(turns[0].context_messages.is_empty());
        for width in [0, 1, 80] {
            let lines = super::super::super::agent_view::settled_lines(
                &turns,
                &Default::default(),
                &Default::default(),
                None,
                width,
                false,
            );
            let cache = cache("bounded".into(), lines);
            assert!(cache.bytes <= MAX_VIEW_BYTES);
            assert!(cache.lines.len() <= MAX_ROWS);
            let letters = cache
                .plain
                .concat()
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>();
            assert!(
                letters.contains("LATESTANSWER"),
                "latest authoritative answer survives width {width}: {:?}",
                cache.plain.iter().rev().take(20).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn readonly_agent_views_layout_reopen_aba_and_retarget_cancel_auxiliary_state() {
        let (mut ui, _, _, _, a, b) = fixture();
        ui.open(a);
        let tab = ui.layout.active(ui.layout.focused).unwrap().id;
        let saved = ui.layout.clone();
        draw(&mut ui);
        ui.schedule_agent_views();
        let deadline = Instant::now() + Duration::from_secs(5);
        let old = loop {
            if let Ok(UiBackgroundResult::AgentView(reply)) = ui.background_rx.try_recv() {
                break reply;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        ui.apply(Action::CloseTab {
            group: ui.layout.group_for(tab).unwrap(),
            tab,
        });
        ui.layout = saved; // saved layout legitimately reuses the exact tab/resource identity
        ui.reconcile_layout_state();
        draw(&mut ui);
        ui.schedule_agent_views();
        assert_ne!(
            ui.agent_views[&tab].pending.as_ref().unwrap().generation,
            old.generation
        );
        ui.apply_agent_view_reply(old);
        assert!(
            ui.agent_views[&tab].cache.is_none(),
            "old lifetime reply cannot match restored tab"
        );
        replies(&mut ui, 1);
        draw(&mut ui);
        ui.find.insert(
            tab,
            FindState {
                query: "ALPHA".into(),
                matches: vec![1],
                selected: 0,
                choosing: false,
                committed: false,
            },
        );
        let cancel = Arc::new(AtomicBool::new(false));
        ui.find_cancel.insert(tab, cancel.clone());
        ui.find_generation.insert(tab, 10);
        ui.text_selection = Some(TextSelection {
            tab,
            anchor: TextPoint { line: 0, col: 0 },
            current: TextPoint { line: 0, col: 4 },
        });
        ui.layout.tabs.get_mut(&tab).unwrap().target = b;
        ui.reconcile_layout_state();
        assert!(cancel.load(Ordering::Acquire));
        assert!(ui.text_selection.is_none());
        assert!(!ui.selectable_views.contains_key(&tab));
        assert!(!ui.agent_views.contains_key(&tab));
        ui.apply_background_result(UiBackgroundResult::Find {
            tab,
            generation: 10,
            query: "ALPHA".into(),
            matches: vec![2],
            choosing: true,
            error: None,
        });
        assert!(
            ui.find[&tab].matches.is_empty(),
            "old target's find reply stays inert"
        );
    }

    #[test]
    fn readonly_agent_views_expired_last_frame_cannot_be_copied_before_timer() {
        let (mut ui, _, _, _, a, b) = fixture();
        let (tab, _) = read_two(&mut ui, a, b);
        ui.text_selection = Some(TextSelection {
            tab,
            anchor: TextPoint { line: 0, col: 0 },
            current: TextPoint { line: 0, col: 4 },
        });
        assert!(ui.selection_text().is_some());
        ui.agent_views.get_mut(&tab).unwrap().checked = Some(Instant::now() - FRESH_FOR);
        assert!(
            ui.selection_text().is_none(),
            "copy uses freshness rather than only last painted geometry"
        );
    }

    #[test]
    fn readonly_agent_views_open_action_uses_ids_and_preserves_limit_error() {
        let (mut ui, store, owner, _, a, _) = fixture();
        let target = resource(&a).unwrap();
        let info = store.session(&owner, &target.session_id).unwrap();
        ui.snapshot.sessions = vec![super::super::super::owner::SavedSession {
            id: target.session_id.clone(),
            title: info.title,
            directory: "/fixture".into(),
            autonomous: false,
            archived: false,
            updated_at: 0,
            turn_count: 1,
            branches: vec![super::super::super::owner::SavedBranch {
                id: target.branch_id.clone(),
                name: "display name, not ID".into(),
                parent: None,
            }],
        }];
        let nav = ui.explorer_pane().unwrap();
        ui.apply(Action::SetNavigator {
            pane: nav,
            kind: Navigator::Sessions,
        });
        ui.nav
            .entry((nav, Navigator::Sessions))
            .or_default()
            .display = NavDisplay::List;
        let draft = (ui.draft.text().to_owned(), ui.draft.cursor());
        ui.apply(Action::Focus(nav));
        ui.open_selected_agent_view();
        assert_eq!(ui.layout.active(ui.layout.focused).unwrap().target, a);
        assert_eq!((ui.draft.text().to_owned(), ui.draft.cursor()), draft);
        // A different exact target cannot replace an existing view at the global limit.
        while ui.layout.tabs.len() < 32 {
            ui.open(Target::File {
                scope_id: ScopeId::launch(),
                path: format!("fixture-{}.txt", ui.layout.tabs.len()),
            });
        }
        ui.snapshot.sessions[0].branches[0].id = "different-branch".into();
        ui.apply(Action::Focus(nav));
        ui.open_selected_agent_view();
        assert!(!ui.notice.contains("view opened"));
        assert!(
            ui.notice.contains("limit") || ui.notice.contains("32"),
            "{}",
            ui.notice
        );
    }

    #[test]
    fn readonly_agent_views_published_reader_loss_or_store_change_hides_immediately() {
        let (mut ui, _, _, member, a, b) = fixture();
        let (tab, _) = read_two(&mut ui, a, b);
        ui.text_selection = Some(TextSelection {
            tab,
            anchor: TextPoint { line: 0, col: 0 },
            current: TextPoint { line: 0, col: 4 },
        });
        let original = ui.branch_reader.take().unwrap();
        assert!(!ui.agent_view_plain(tab).join("\n").contains("ALPHA"));
        assert!(ui.selection_text().is_none());
        ui.branch_reader =
            Some(branch_view::Reader::new(SessionStore::in_memory().unwrap(), member).unwrap());
        assert!(!ui.agent_view_fresh(tab));
        ui.expire_agent_views();
        assert!(ui.agent_views[&tab].cache.is_none());
        ui.branch_reader = Some(original);
        assert!(
            !ui.agent_view_fresh(tab),
            "restoring a reader does not resurrect the old grant"
        );
    }

    #[test]
    fn readonly_agent_views_current_live_authorization_revokes_without_new_head() {
        let (mut ui, store, owner, member, a, _) = fixture();
        let target = resource(&a).unwrap();
        ui.snapshot.session = Some(target.session_id.clone());
        ui.snapshot.branch_id = Some(target.branch_id.clone());
        ui.snapshot.live = "LIVE PRIVATE ANSWER".into();
        ui.open(a);
        let tab = ui.layout.active(ui.layout.focused).unwrap().id;
        draw(&mut ui);
        ui.schedule_agent_views();
        replies(&mut ui, 1);
        assert!(ui
            .agent_view_plain(tab)
            .join("\n")
            .contains("LIVE PRIVATE ANSWER"));
        let before = store.branches(&owner, &target.session_id).unwrap()[0].clone();
        store
            .grant(&owner, &target.session_id, &member, None)
            .unwrap();
        draw(&mut ui);
        ui.agent_views.get_mut(&tab).unwrap().attempted = None;
        ui.schedule_agent_views();
        replies(&mut ui, 1);
        assert!(!ui
            .agent_view_plain(tab)
            .join("\n")
            .contains("LIVE PRIVATE ANSWER"));
        assert!(ui.agent_view_plain(tab).join("\n").contains("unavailable"));
        let after = &store.branches(&owner, &target.session_id).unwrap()[0];
        assert_eq!(
            (&after.head, after.revision),
            (&before.head, before.revision)
        );
    }
}
