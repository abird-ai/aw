use super::{
    background::Executor as BackgroundExecutor,
    files::{Access, Browser, Content, Request, Scopes},
    git,
    owner::{Command, Snapshot, Worker},
    palette::{self, Category as PaletteCategory},
    persistence,
    session_nav::{self, Scope as SessionScope},
    syntax, terminals,
    viewport::{Viewport, SCROLLOFF_ROWS, WHEEL_ROWS},
};
use crate::tui::{
    commands::{
        complete_slash, exact_slash_command, slash_matches, workspace_matches,
        WorkspaceCommandContext, WORKSPACE_KEYMAP,
    },
    composer::{Action as ComposerAction, Interaction as ComposerInteraction},
    editor::{edit_key, EditMode},
    extensions,
    settings::{SettingsInput, SettingsState},
    status::StatusContext,
};
use aw::{Error, Result};
use aw_tools::browse::{Entry, EntryKind};
use aw_ui_extension::Slot;
use aw_workspace::{
    Action, Axis, Editor, GitContextId, Id, Navigator, Pane, Rect as ModelRect, ScopeId, Side,
    Target, Workspace, MAX_SCOPE_LABEL_BYTES,
};
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use futures_util::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

mod agent_views;
mod async_work;
mod commands;
mod files_views;
mod git_views;
mod interaction;
mod render;

const KEY_CHORD_TIMEOUT: Duration = Duration::from_millis(1500);
const LIST_VIEW_ICON: &str = "≡";
const TREE_VIEW_ICON: &str = "┬";
const TAB_DIRTY_ICON: &str = "●";

fn relative_age(updated_at_micros: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let updated = u64::try_from(updated_at_micros.max(0)).unwrap_or_default() / 1_000_000;
    let age = now.saturating_sub(updated);
    match age {
        0..=59 => "now".into(),
        60..=3_599 => format!("{}m ago", age / 60),
        3_600..=86_399 => format!("{}h ago", age / 3_600),
        _ => format!("{}d ago", age / 86_400),
    }
}

const HELP: &str = "The workspace uses familiar VS Code/Helix-style navigation and the same slash-command registry as inline mode.

Type / in the Agent composer for agent/session commands. Use Ctrl+Shift+P or F1 for the workspace command palette. Ordinary keys stay with an embedded terminal when it has focus, except those explicit global UI chords.
";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Agent,
    ConfigureProvider,
    Quit,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum NavDisplay {
    List,
    #[default]
    Tree,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExplorerPlacement {
    Left,
    Right,
    Hidden,
}

#[derive(Clone, Debug)]
enum SearchTarget {
    Navigator { pane: Id, kind: Navigator },
    Main { tab: Id },
}

#[derive(Clone, Debug)]
struct SearchState {
    editor: Editor,
    target: SearchTarget,
    original: String,
}

#[derive(Clone, Debug, Default)]
struct FindState {
    query: String,
    matches: Vec<usize>,
    selected: usize,
    choosing: bool,
    // Enter's intent survives pending/missing results until a jump consumes it.
    committed: bool,
}

#[derive(Default)]
struct NavState {
    directory: String,
    entries: Vec<Entry>,
    selected: usize,
    filter: String,
    loaded: bool,
    pending: Option<u64>,
    incomplete: bool,
    display: NavDisplay,
    session_scope: SessionScope,
    search_query: String,
    search_results: Vec<Entry>,
    search_pending: Option<u64>,
    /// Directory inventories are loaded lazily as folders are expanded. Keys are
    /// clean relative directory paths; the empty key is the workspace root.
    directories: BTreeMap<String, Vec<Entry>>,
    expanded: BTreeSet<String>,
    collapsed: BTreeSet<String>,
    multi_select: bool,
    selected_sessions: BTreeSet<String>,
}

#[derive(Clone, Debug)]
enum Hit {
    Pane(Id),
    NavigatorKind { pane: Id, kind: Navigator },
    NavigatorDisplay { pane: Id, display: NavDisplay },
    SessionScope { pane: Id, scope: SessionScope },
    SessionMultiSelect { pane: Id },
    GitMode { pane: Id, mode: git::Mode },
    GitDisplay { pane: Id, display: git::Display },
    NavigatorRow { pane: Id, row: usize },
    GroupTab { group: Id, tab: Id },
    MaximizePane(Id),
    Composer(Id),
    SlashRow { group: Id, row: usize },
    Settings { group: Id },
    PaletteCategory(PaletteCategory),
    PaletteRow(usize),
    TextView { group: Id, tab: Id },
    AgentSession { session: String },
    Divider { first: Id, second: Id, axis: Axis },
}

#[derive(Clone, Debug)]
struct HitRegion {
    area: Rect,
    hit: Hit,
}

#[derive(Clone, Debug)]
enum PaletteTarget {
    Command(String),
    Slash(String),
    File(FileKey),
    Session { session: String, branch: String },
    Window { group: Id, tab: Id },
    Terminal(String),
}

#[derive(Clone, Debug)]
struct PaletteItem {
    category: PaletteCategory,
    label: String,
    description: String,
    shortcut: String,
    target: PaletteTarget,
    score: usize,
}

impl PaletteItem {
    fn key(&self) -> String {
        format!("{}:{:?}", self.category.prefix(), self.target)
    }
}

fn palette_fuzzy_score(candidate: &str, query: &str) -> Option<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Some(0);
    }
    let candidate = candidate.to_lowercase();
    if candidate.starts_with(&query) {
        return Some(0);
    }
    if let Some(at) = candidate.find(&query) {
        return Some(10 + at);
    }
    let mut source = candidate.chars();
    let mut gaps = 0usize;
    for wanted in query.chars() {
        let mut gap = 0usize;
        loop {
            let next = source.next()?;
            if next == wanted {
                gaps = gaps.saturating_add(gap);
                break;
            }
            gap = gap.saturating_add(1);
        }
    }
    Some(100 + gaps)
}

fn palette_resource_score(
    category: PaletteCategory,
    label: &str,
    description: &str,
    query: &str,
) -> Option<usize> {
    [
        palette_fuzzy_score(label, query),
        palette_fuzzy_score(description, query).map(|score| score.saturating_add(10)),
        palette_fuzzy_score(category.label(), query).map(|score| score.saturating_add(20)),
    ]
    .into_iter()
    .flatten()
    .min()
}

fn draw_segmented_control<T: Copy + PartialEq>(
    frame: &mut Frame<'_>,
    area: Rect,
    items: &[(T, &str)],
    selected: T,
    selected_color: Color,
    inactive_color: Color,
) -> (Vec<(Rect, T)>, u16) {
    if area.width == 0 || area.height == 0 || items.is_empty() {
        return (Vec::new(), 0);
    }
    const DIVIDER: &str = " · ";
    let divider_width = UnicodeWidthStr::width(DIVIDER) as u16;
    let needed = items
        .iter()
        .map(|(_, label)| UnicodeWidthStr::width(*label) as u16)
        .sum::<u16>()
        .saturating_add(divider_width.saturating_mul(items.len().saturating_sub(1) as u16));
    let mut hits = Vec::new();
    if needed <= area.width {
        let mut x = area.x;
        for (index, (value, label)) in items.iter().enumerate() {
            let width = (UnicodeWidthStr::width(*label) as u16)
                .min(area.x.saturating_add(area.width).saturating_sub(x));
            if width == 0 {
                break;
            }
            let rect = Rect {
                x,
                y: area.y,
                width,
                height: 1,
            };
            let style = if *value == selected {
                Style::default()
                    .fg(selected_color)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(inactive_color)
            };
            frame.render_widget(Paragraph::new(Span::styled(*label, style)), rect);
            hits.push((rect, *value));
            x = x.saturating_add(width);
            if index + 1 < items.len() && x < area.x.saturating_add(area.width) {
                let width = divider_width.min(area.x.saturating_add(area.width).saturating_sub(x));
                frame.render_widget(
                    Paragraph::new(Span::styled(DIVIDER, Style::default().fg(inactive_color))),
                    Rect {
                        x,
                        y: area.y,
                        width,
                        height: 1,
                    },
                );
                x = x.saturating_add(width);
            }
        }
        return (hits, 1);
    }

    let used = (items.len() as u16).min(area.height);
    for (index, (value, label)) in items.iter().take(usize::from(used)).enumerate() {
        let y = area.y.saturating_add(index as u16);
        let style = if *value == selected {
            Style::default()
                .fg(selected_color)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(inactive_color)
        };
        frame.render_widget(
            Paragraph::new(Span::styled(*label, style)),
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            },
        );
        hits.push((
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            },
            *value,
        ));
    }
    (hits, used)
}

fn draw_scroll_indicator(
    frame: &mut Frame<'_>,
    area: Rect,
    total: usize,
    viewport: usize,
    start: usize,
    palette: crate::tui::theme::Palette,
) {
    if area.width == 0 || area.height == 0 || viewport == 0 || total <= viewport {
        return;
    }
    let track_height = usize::from(area.height);
    if track_height == 0 {
        return;
    }
    let thumb_height = ((viewport.saturating_mul(track_height) + total.saturating_sub(1)) / total)
        .clamp(1, track_height);
    let max_scroll = total.saturating_sub(viewport);
    let max_top = track_height.saturating_sub(thumb_height);
    let top = start
        .min(max_scroll)
        .saturating_mul(max_top)
        .checked_div(max_scroll)
        .unwrap_or(0);
    let x = area.x.saturating_add(area.width.saturating_sub(1));
    let lines = (0..track_height)
        .map(|row| {
            let active = row >= top && row < top.saturating_add(thumb_height);
            Line::styled(
                if active { "┃" } else { "│" },
                Style::default().fg(if active {
                    palette.scrollbar_thumb()
                } else {
                    palette.scrollbar_track()
                }),
            )
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines),
        Rect {
            x,
            y: area.y,
            width: 1,
            height: area.height,
        },
    );
}

#[derive(Clone, Copy, Debug)]
struct ResizeDrag {
    first: Id,
    second: Id,
    axis: Axis,
    last: u16,
    span: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TextPoint {
    line: usize,
    col: usize,
}

#[derive(Clone, Debug)]
struct TextSelection {
    tab: Id,
    anchor: TextPoint,
    current: TextPoint,
}

#[derive(Clone, Debug)]
struct SelectableView {
    area: Rect,
    start_line: usize,
    horizontal: usize,
    lines: Vec<String>,
}

fn line_plain_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

fn cell_slice(text: &str, start: usize, end: usize) -> (usize, String) {
    if end <= start || text.is_empty() {
        return (start, String::new());
    }
    let mut cell = 0usize;
    let mut actual_start = None;
    let mut out = String::new();
    for ch in text.chars() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
        let next = cell.saturating_add(width);
        if next <= start {
            cell = next;
            continue;
        }
        if cell >= end {
            break;
        }
        actual_start.get_or_insert(cell);
        out.push(ch);
        cell = next;
    }
    (actual_start.unwrap_or(start), out)
}

fn base64_bytes(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        out.push(TABLE[(a >> 2) as usize] as char);
        out.push(TABLE[(((a & 0x03) << 4) | (b >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b & 0x0f) << 2) | (c >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(c & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn copy_osc52(text: &str) -> io::Result<()> {
    const MAX_CLIPBOARD_BYTES: usize = 256 * 1024;
    if text.is_empty() || text.len() > MAX_CLIPBOARD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "clipboard selection is empty or too large",
        ));
    }
    let encoded = base64_bytes(text.as_bytes());
    let mut stdout = io::stdout();
    write!(stdout, "\x1b]52;c;{encoded}\x07")?;
    stdout.flush()
}

#[derive(Clone)]
struct Rendered {
    target: Target,
    width: u16,
    revision: u64,
    lines: Arc<Vec<String>>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FileKey {
    scope_id: ScopeId,
    path: String,
}

impl FileKey {
    fn new(scope_id: ScopeId, path: String) -> Self {
        Self { scope_id, path }
    }
    fn target(&self) -> Target {
        Target::File {
            scope_id: self.scope_id.clone(),
            path: self.path.clone(),
        }
    }
}

#[cfg(test)]
impl From<String> for FileKey {
    fn from(path: String) -> Self {
        Self::new(ScopeId::launch(), path)
    }
}
#[cfg(test)]
impl From<&str> for FileKey {
    fn from(path: &str) -> Self {
        Self::from(path.to_owned())
    }
}

#[derive(Clone)]
struct FileToken {
    access: Access,
    epoch: u64,
    container: Option<Id>,
}

#[derive(Clone)]
enum Job {
    List {
        pane: Id,
        path: String,
        token: FileToken,
    },
    Read {
        tab: Id,
        path: String,
        token: FileToken,
    },
    Search {
        pane: Option<Id>,
        query: String,
        cancel: Arc<AtomicBool>,
        token: FileToken,
    },
    Authorize {
        beside: Id,
        reconnect: Option<ScopeId>,
        origin_tab: Option<Id>,
        epoch: u64,
    },
}

impl Job {
    fn token(&self) -> Option<&FileToken> {
        match self {
            Self::List { token, .. } | Self::Read { token, .. } | Self::Search { token, .. } => {
                Some(token)
            }
            Self::Authorize { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum HighlightTarget {
    File(FileKey),
    Diff(String),
}

struct HighlightCache {
    generation: u64,
    theme: aw::config::ThemeMode,
    lines: Vec<Option<Line<'static>>>,
    complete: bool,
    cancel: Arc<AtomicBool>,
}

impl Drop for HighlightCache {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

#[derive(Clone)]
struct ConversationRenderKey {
    turns: Arc<Vec<aw::conversation::Turn>>,
    resources: crate::tui::agent_resources::LoadedResourceSummary,
    ui: aw::config::UiConfig,
    width: u16,
}

impl ConversationRenderKey {
    fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.turns, &other.turns)
            && self.resources == other.resources
            && self.ui == other.ui
            && self.width == other.width
    }
}

struct ConversationCache {
    key: ConversationRenderKey,
    lines: Arc<Vec<Line<'static>>>,
}

struct ConversationPending {
    generation: u64,
    key: ConversationRenderKey,
}

enum UiBackgroundResult {
    AgentView(agent_views::Reply),
    Terminal(std::result::Result<String, String>),
    InitialLayout {
        base_generation: u64,
        result: std::result::Result<Option<Workspace>, String>,
    },
    Layout {
        base_generation: u64,
        result: std::result::Result<Option<Workspace>, String>,
    },
    Highlight {
        target: HighlightTarget,
        generation: u64,
        start: usize,
        lines: Vec<Line<'static>>,
        done: bool,
    },
    Conversation {
        generation: u64,
        key: ConversationRenderKey,
        result: std::result::Result<Vec<Line<'static>>, String>,
    },
    Find {
        tab: Id,
        generation: u64,
        query: String,
        matches: Vec<usize>,
        choosing: bool,
        error: Option<&'static str>,
    },
}

fn try_send_ui_progress(
    out: &tokio::sync::mpsc::Sender<UiBackgroundResult>,
    result: UiBackgroundResult,
) -> bool {
    match out.try_send(result) {
        Ok(()) | Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => true,
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => false,
    }
}

#[derive(Clone)]
enum NavigatorItem {
    Parent,
    File(Entry),
    UnsavedSession,
    SessionRoot { session: String },
    SessionBranch { session: String, branch: String },
}

#[derive(Clone)]
struct NavigatorRow {
    label: String,
    item: NavigatorItem,
}

fn navigator_session_id(item: &NavigatorItem) -> Option<&str> {
    match item {
        NavigatorItem::SessionRoot { session } | NavigatorItem::SessionBranch { session, .. } => {
            Some(session)
        }
        NavigatorItem::Parent | NavigatorItem::File(_) | NavigatorItem::UnsavedSession => None,
    }
}

fn navigator_branch(item: &NavigatorItem) -> Option<&str> {
    match item {
        NavigatorItem::SessionBranch { branch, .. } => Some(branch),
        _ => None,
    }
}

pub struct Ui {
    pub layout: Workspace,
    pub draft: Editor,
    pub layout_file: Option<PathBuf>,
    pub root: PathBuf,
    pub file_scopes: Scopes,
    file_epoch: u64,
    file_bindings: BTreeMap<Id, (ScopeId, Option<Id>, Option<String>)>,
    palette_file_scope: ScopeId,
    palette_file_container: Option<Id>,
    file_find_dirty: BTreeSet<Id>,
    layout_pending: bool,
    layout_generation: u64,
    pub(super) git: git::Views,
    git_epoch: u64,
    git_bindings: Vec<(Id, Id, GitContextId)>,
    git_tab_bindings: BTreeMap<Id, (String, Option<Id>)>,
    terminal_pool: Option<terminals::Pool>,
    shell: PathBuf,
    terminal_pending: bool,
    terminal_scroll: BTreeMap<Id, usize>,
    rendered: BTreeMap<Id, Rendered>,
    file_revision: u64,
    snapshot: Snapshot,
    branch_reader: Option<super::branch_view::Reader>,
    agent_views: BTreeMap<Id, agent_views::View>,
    agent_view_generation: u64,
    saved_selections: BTreeMap<Id, TextSelection>,
    nav: BTreeMap<(Id, Navigator), NavState>,
    files: BTreeMap<FileKey, String>,
    file_lines: BTreeMap<FileKey, Arc<Vec<String>>>,
    file_highlights: BTreeMap<FileKey, HighlightCache>,
    diff_highlights: BTreeMap<String, HighlightCache>,
    highlight_generation: u64,
    conversation_generation: u64,
    conversation_cache: Option<ConversationCache>,
    conversation_pending: Option<ConversationPending>,
    palette_file_results: Vec<Entry>,
    palette_file_query: String,
    palette_file_pending: Option<u64>,
    jobs: BTreeMap<u64, Job>,
    ticket: u64,
    scroll: BTreeMap<Id, Viewport>,
    horizontal: BTreeMap<Id, u16>,
    line_counts: BTreeMap<Id, usize>,
    last_group: Id,
    container_last_group: BTreeMap<Id, Id>,
    input: bool,
    palette: Option<palette::State>,
    composer: ComposerInteraction,
    chord: Option<Instant>,
    terminal_return: Option<(Id, Option<Id>)>,
    notice: String,
    maximized: bool,
    explorer_placement: ExplorerPlacement,
    search: Option<SearchState>,
    find: BTreeMap<Id, FindState>,
    find_generation: BTreeMap<Id, u64>,
    find_request_generation: u64,
    find_cancel: BTreeMap<Id, Arc<AtomicBool>>,
    last_tab_click: Option<(Id, Id, Instant)>,
    exit: Option<Exit>,
    recovery_seen: u64,
    recovery: String,
    can_submit: bool,
    pub(crate) ui_config: aw::config::UiConfig,
    background: BackgroundExecutor,
    background_tx: tokio::sync::mpsc::Sender<UiBackgroundResult>,
    background_rx: tokio::sync::mpsc::Receiver<UiBackgroundResult>,
    settings: Option<SettingsState>,
    hit_regions: Vec<HitRegion>,
    resize_drag: Option<ResizeDrag>,
    selectable_views: BTreeMap<Id, SelectableView>,
    text_selection: Option<TextSelection>,
    selection_dragging: bool,
    selection_moved: bool,
}
impl Ui {
    #[cfg(test)]
    pub fn new(
        layout: Workspace,
        draft: String,
        layout_file: Option<PathBuf>,
        root: PathBuf,
        snapshot: Snapshot,
        ui_config: aw::config::UiConfig,
    ) -> Result<Self> {
        let background = BackgroundExecutor::new()?;
        let scopes = Scopes::new(root.clone())?;
        let contexts = git::Contexts::new(root.clone());
        Self::new_with_background(
            layout,
            draft,
            layout_file,
            root,
            snapshot,
            ui_config,
            background,
            scopes,
            contexts,
        )
    }

    pub fn new_with_background(
        layout: Workspace,
        draft: String,
        layout_file: Option<PathBuf>,
        root: PathBuf,
        snapshot: Snapshot,
        ui_config: aw::config::UiConfig,
        background: BackgroundExecutor,
        file_scopes: Scopes,
        git_contexts: git::Contexts,
    ) -> Result<Self> {
        layout
            .validate()
            .map_err(|e| Error::Config(e.to_string()))?;
        let last_group = layout.groups()[0];
        let input = matches!(
            layout.active(layout.focused).map(|tab| &tab.target),
            Some(Target::Conversation)
        );
        let mut editor = Editor::default();
        editor
            .set(draft)
            .map_err(|e| Error::Config(e.to_string()))?;
        let order = layout.ordered_panes();
        let nav_at = order
            .iter()
            .position(|id| matches!(layout.panes.get(id), Some(Pane::Navigator { .. })));
        let group_at = order
            .iter()
            .position(|id| matches!(layout.panes.get(id), Some(Pane::Group { .. })));
        let explorer_placement = match (nav_at, group_at) {
            (Some(n), Some(g)) if n > g => ExplorerPlacement::Right,
            (Some(_), Some(_)) => ExplorerPlacement::Left,
            _ => ExplorerPlacement::Hidden,
        };
        let (background_tx, background_rx) = tokio::sync::mpsc::channel(64);
        let mut ui = Self {
            layout,
            draft: editor,
            layout_file,
            git: git::Views::new_with_contexts(git_contexts, background.clone()),
            git_epoch: 1,
            git_bindings: Vec::new(),
            git_tab_bindings: BTreeMap::new(),
            terminal_pool: None,
            shell: PathBuf::from("/bin/bash"),
            terminal_pending: false,
            terminal_scroll: BTreeMap::new(),
            root,
            file_scopes,
            file_epoch: 1,
            file_bindings: BTreeMap::new(),
            palette_file_scope: ScopeId::launch(),
            palette_file_container: None,
            file_find_dirty: BTreeSet::new(),
            layout_pending: false,
            layout_generation: 0,
            rendered: BTreeMap::new(),
            file_revision: 0,
            snapshot,
            branch_reader: None,
            agent_views: BTreeMap::new(),
            agent_view_generation: 0,
            saved_selections: BTreeMap::new(),
            nav: BTreeMap::new(),
            files: BTreeMap::new(),
            file_lines: BTreeMap::new(),
            file_highlights: BTreeMap::new(),
            diff_highlights: BTreeMap::new(),
            highlight_generation: 0,
            conversation_generation: 0,
            conversation_cache: None,
            conversation_pending: None,
            palette_file_results: Vec::new(),
            palette_file_query: String::new(),
            palette_file_pending: None,
            jobs: BTreeMap::new(),
            ticket: 0,
            scroll: BTreeMap::new(),
            horizontal: BTreeMap::new(),
            line_counts: BTreeMap::new(),
            last_group,
            container_last_group: BTreeMap::new(),
            input,
            palette: None,
            composer: ComposerInteraction::default(),
            chord: None,
            terminal_return: None,
            notice: String::new(),
            maximized: false,
            explorer_placement,
            search: None,
            find: BTreeMap::new(),
            find_generation: BTreeMap::new(),
            find_request_generation: 0,
            find_cancel: BTreeMap::new(),
            last_tab_click: None,
            exit: None,
            recovery_seen: 0,
            recovery: String::new(),
            can_submit: false,
            ui_config,
            background,
            background_tx,
            background_rx,
            settings: None,
            hit_regions: Vec::new(),
            resize_drag: None,
            selectable_views: BTreeMap::new(),
            text_selection: None,
            selection_dragging: false,
            selection_moved: false,
        };
        ui.reconcile_layout_state();
        Ok(ui)
    }
    pub fn attach_terminals(&mut self, pool: terminals::Pool, shell: PathBuf) {
        self.terminal_pool = Some(pool);
        self.shell = shell;
    }

    pub fn schedule_initial_layout_load(&mut self) {
        if self.layout_pending {
            return;
        }
        let Some(path) = self.layout_file.clone() else {
            return;
        };
        let root = self.root.clone();
        let launch_scope = ScopeId::launch();
        let base_generation = self.layout_generation;
        let out = self.background_tx.clone();
        let panic_out = out.clone();
        match self.background.try_submit_with_panic(
            "layout",
            move || {
                let result =
                    persistence::load(&path, &root, &launch_scope).map_err(|e| e.to_string());
                let _ = out.blocking_send(UiBackgroundResult::InitialLayout {
                    base_generation,
                    result,
                });
            },
            move || {
                let _ = panic_out.blocking_send(UiBackgroundResult::InitialLayout {
                    base_generation,
                    result: Err(
                        "initial layout background job panicked; default layout retained".into(),
                    ),
                });
            },
        ) {
            Ok(()) => self.layout_pending = true,
            Err(error) => self.notice = error.to_string(),
        }
    }

    fn navigator_order() -> [(Navigator, &'static str); 3] {
        [
            (Navigator::Sessions, "Sessions"),
            (Navigator::Files, "Files"),
            (Navigator::Git, "Git"),
        ]
    }

    fn focus_explorer(&mut self) {
        let owned_navigator = self.owned_navigator();
        if let Some(pane) = owned_navigator.or_else(|| {
            self.layout
                .ordered_panes()
                .into_iter()
                .find(|id| matches!(self.layout.panes.get(id), Some(Pane::Navigator { .. })))
        }) {
            self.apply(Action::Focus(pane));
            self.input = false;
        } else {
            self.apply(Action::AddNavigator {
                beside: self.group(),
                side: Side::Left,
                kind: Navigator::Sessions,
            });
            self.input = false;
        }
    }

    fn focus_main(&mut self) {
        let group = self.group();
        self.apply(Action::Focus(group));
        self.input = matches!(
            self.layout.active(group).map(|tab| &tab.target),
            Some(Target::Conversation)
        );
    }

    fn activate_number(&mut self, number: usize, worker: &Worker) {
        if number == 0 {
            return;
        }
        match self.layout.panes.get(&self.layout.focused).cloned() {
            Some(Pane::Navigator { .. }) => {
                if let Some((kind, _)) = Self::navigator_order().get(number - 1).copied() {
                    self.set_navigator(kind);
                    if kind == Navigator::Sessions {
                        self.owner(worker, Command::Refresh, false);
                    }
                }
            }
            Some(Pane::Group { tabs, .. }) => {
                if let Some(tab) = tabs.get(number - 1).copied() {
                    self.apply(Action::Select {
                        group: self.layout.focused,
                        tab,
                    });
                    self.input = matches!(
                        self.layout
                            .active(self.layout.focused)
                            .map(|tab| &tab.target),
                        Some(Target::Conversation)
                    );
                }
            }
            None => {}
        }
    }

    fn hit(&mut self, area: Rect, hit: Hit) {
        if area.width > 0 && area.height > 0 {
            self.hit_regions.push(HitRegion { area, hit });
        }
    }

    fn toggle_maximize(&mut self, pane: Id) {
        if self.maximized {
            self.maximized = false;
        } else if self.layout.panes.contains_key(&pane) {
            if self.layout.focused != pane {
                self.input = false;
            }
            self.apply(Action::Focus(pane));
            self.maximized = true;
        }
    }

    fn pane_content_area(area: Rect, visible: &[aw_workspace::Placement]) -> Rect {
        let right = area.right();
        let bottom = area.bottom();
        let split_right = visible.iter().any(|item| {
            item.area.x == right
                && item.area.y < bottom
                && item.area.y.saturating_add(item.area.height) > area.y
        });
        let split_below = visible.iter().any(|item| {
            item.area.y == bottom
                && item.area.x < right
                && item.area.x.saturating_add(item.area.width) > area.x
        });
        Rect {
            width: area.width.saturating_sub(u16::from(split_right)),
            height: area.height.saturating_sub(u16::from(split_below)),
            ..area
        }
    }

    fn record_dividers(&mut self, visible: &[aw_workspace::Placement]) {
        for (i, a) in visible.iter().enumerate() {
            for b in visible.iter().skip(i + 1) {
                let a_right = a.area.x.saturating_add(a.area.width);
                let b_right = b.area.x.saturating_add(b.area.width);
                let a_bottom = a.area.y.saturating_add(a.area.height);
                let b_bottom = b.area.y.saturating_add(b.area.height);
                let y0 = a.area.y.max(b.area.y);
                let y1 = a_bottom.min(b_bottom);
                if y1 > y0 && (a_right == b.area.x || b_right == a.area.x) {
                    let (first, second, x) = if a_right == b.area.x {
                        (a.pane, b.pane, a_right.saturating_sub(1))
                    } else {
                        (b.pane, a.pane, b_right.saturating_sub(1))
                    };
                    self.hit(
                        Rect {
                            x,
                            y: y0,
                            width: 1,
                            height: y1 - y0,
                        },
                        Hit::Divider {
                            first,
                            second,
                            axis: Axis::Columns,
                        },
                    );
                }
                let x0 = a.area.x.max(b.area.x);
                let x1 = a_right.min(b_right);
                if x1 > x0 && (a_bottom == b.area.y || b_bottom == a.area.y) {
                    let (first, second, y) = if a_bottom == b.area.y {
                        (a.pane, b.pane, a_bottom.saturating_sub(1))
                    } else {
                        (b.pane, a.pane, b_bottom.saturating_sub(1))
                    };
                    self.hit(
                        Rect {
                            x: x0,
                            y,
                            width: x1 - x0,
                            height: 1,
                        },
                        Hit::Divider {
                            first,
                            second,
                            axis: Axis::Rows,
                        },
                    );
                }
            }
        }
    }
    fn terminal(&self) -> Option<Arc<aw_terminal::Terminal>> {
        match self.layout.active(self.layout.focused).map(|t| &t.target) {
            Some(Target::Terminal { terminal }) => self.terminal_pool.as_ref()?.get(terminal),
            _ => None,
        }
    }
    fn review_id(&self) -> Option<String> {
        match self.layout.active(self.layout.focused).map(|t| &t.target) {
            Some(Target::Diff { review }) => Some(review.clone()),
            _ => None,
        }
    }
    fn terminal_paste(&mut self, text: &str) -> bool {
        if let Some(terminal) = self.terminal() {
            let result = aw_terminal::encode_paste(text, terminal.input_modes().1)
                .and_then(|bytes| terminal.input(bytes));
            if let Err(e) = result {
                self.notice = e.to_string();
            }
            return true;
        }
        false
    }
    fn toggle_terminal(&mut self) {
        let focused = self.layout.focused;
        let focused_terminal = matches!(
            self.layout.active(focused).map(|tab| &tab.target),
            Some(Target::Terminal { .. })
        );
        if focused_terminal {
            if let Some((pane, tab)) = self.terminal_return.take() {
                if self.layout.panes.contains_key(&pane) {
                    if let Some(tab) = tab {
                        if self.layout.group_for(tab) == Some(pane) {
                            self.apply(Action::Select { group: pane, tab });
                        } else {
                            self.apply(Action::Focus(pane));
                        }
                    } else {
                        self.apply(Action::Focus(pane));
                    }
                    self.input = matches!(
                        self.layout.active(self.layout.focused).map(|t| &t.target),
                        Some(Target::Conversation)
                    );
                    return;
                }
            }
            // Fallback after layout changes: prefer any non-terminal content,
            // then a navigator. Never "toggle" back onto the same terminal.
            let content = self.layout.tabs.iter().find_map(|(id, tab)| {
                (!matches!(tab.target, Target::Terminal { .. })).then_some(*id)
            });
            if let Some(tab) = content {
                if let Some(group) = self.layout.group_for(tab) {
                    self.apply(Action::Select { group, tab });
                    self.input = matches!(
                        self.layout.active(group).map(|t| &t.target),
                        Some(Target::Conversation)
                    );
                    return;
                }
            }
            if let Some(pane) = self
                .layout
                .ordered_panes()
                .into_iter()
                .find(|id| matches!(self.layout.panes.get(id), Some(Pane::Navigator { .. })))
            {
                self.apply(Action::Focus(pane));
                self.input = false;
                return;
            }
            self.notice = "No non-terminal workspace surface is available.".into();
            return;
        }

        let target = self
            .layout
            .tabs
            .iter()
            .find_map(|(id, tab)| matches!(tab.target, Target::Terminal { .. }).then_some(*id));
        if let Some(tab) = target {
            self.terminal_return =
                Some((focused, self.layout.active(focused).map(|active| active.id)));
            if let Some(group) = self.layout.group_for(tab) {
                self.apply(Action::Select { group, tab });
                self.input = false;
                return;
            }
        }
        self.notice =
            "No live terminal view. Use Ctrl+Shift+P → New Terminal to launch one explicitly."
                .into();
    }

    fn palette_context(&self) -> WorkspaceCommandContext {
        WorkspaceCommandContext {
            terminal_focused: self.terminal().is_some(),
            diff_focused: self.review_id().is_some(),
            navigator_focused: matches!(
                self.layout.panes.get(&self.layout.focused),
                Some(Pane::Navigator { .. })
            ),
            has_recovery: !self.recovery.is_empty(),
            has_unsent_text: !self.draft.is_empty() || !self.recovery.is_empty(),
            has_live_terminals: self
                .terminal_pool
                .as_ref()
                .is_some_and(|pool| pool.running()),
            has_layout_file: self.layout_file.is_some(),
            agent_busy: self.snapshot.busy || self.snapshot.state_pending,
            needs_inspection: self.snapshot.needs_inspection,
        }
    }

    fn palette_choices(&mut self) -> Vec<PaletteItem> {
        let Some(state) = self.palette.as_ref() else {
            return Vec::new();
        };
        let choices = self.palette_items(state.text());
        self.palette
            .as_mut()
            .expect("open palette")
            .reconcile(&choices.iter().map(PaletteItem::key).collect::<Vec<_>>());
        choices
    }

    fn palette_items(&self, raw_query: &str) -> Vec<PaletteItem> {
        let context = self.palette_context();
        let parsed = palette::parse(raw_query);
        let query = parsed.text;
        let include =
            |category: PaletteCategory| parsed.category.is_none_or(|active| active == category);

        let actions = if include(PaletteCategory::Actions) {
            workspace_matches(query, context)
        } else {
            Vec::new()
        };
        if actions.len() == 1
            && actions[0].command.ends_with(' ')
            && query
                .to_ascii_lowercase()
                .starts_with(&actions[0].command.to_ascii_lowercase())
        {
            let action = actions[0];
            return vec![PaletteItem {
                category: PaletteCategory::Actions,
                label: action.label.to_owned(),
                description: action.description.to_owned(),
                shortcut: action.shortcut.to_owned(),
                target: PaletteTarget::Command(action.command.to_owned()),
                score: 0,
            }];
        }

        let mut items = Vec::new();
        if include(PaletteCategory::Actions) {
            let action_limit = if query.is_empty() { 8 } else { 20 };
            for (index, action) in actions.into_iter().take(action_limit).enumerate() {
                items.push(PaletteItem {
                    category: PaletteCategory::Actions,
                    label: action.label.to_owned(),
                    description: action.description.to_owned(),
                    shortcut: action.shortcut.to_owned(),
                    target: PaletteTarget::Command(action.command.to_owned()),
                    score: index,
                });
            }
        }

        if include(PaletteCategory::Commands) {
            let command_limit = if query.is_empty() { 10 } else { 24 };
            let slash_query = format!("/{query}");
            for (index, spec) in slash_matches(&slash_query, true)
                .into_iter()
                .take(command_limit)
                .enumerate()
            {
                let aliases = crate::tui::commands::slash_aliases(spec.name);
                let alias_text = if aliases.is_empty() {
                    String::new()
                } else {
                    format!(
                        " · {}",
                        aliases
                            .iter()
                            .map(|alias| format!("/{alias}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                items.push(PaletteItem {
                    category: PaletteCategory::Commands,
                    label: format!("/{}", spec.name),
                    description: format!("{}{}", spec.description, alias_text),
                    shortcut: String::new(),
                    target: PaletteTarget::Slash(
                        if spec.supports_inline_args && spec.name != "resume" {
                            format!("{} ", spec.name)
                        } else {
                            spec.name.to_owned()
                        },
                    ),
                    score: index,
                });
            }
        }

        if include(PaletteCategory::Files) {
            let mut files = BTreeMap::<FileKey, String>::new();
            for ((pane, kind), state) in &self.nav {
                if *kind != Navigator::Files {
                    continue;
                }
                let Some(scope) = self.layout.files_scope_for(*pane) else {
                    continue;
                };
                for entries in state.directories.values() {
                    for entry in entries {
                        if entry.kind == EntryKind::File {
                            files
                                .entry(FileKey::new(scope.clone(), entry.path.clone()))
                                .or_insert_with(|| entry.name.clone());
                        }
                    }
                }
            }
            for key in self.files.keys() {
                files
                    .entry(key.clone())
                    .or_insert_with(|| key.path.rsplit('/').next().unwrap_or(&key.path).to_owned());
            }
            for entry in &self.palette_file_results {
                if entry.kind == EntryKind::File {
                    files
                        .entry(FileKey::new(
                            self.palette_file_scope.clone(),
                            entry.path.clone(),
                        ))
                        .or_insert_with(|| entry.name.clone());
                }
            }
            let file_limit = if query.is_empty() { 8 } else { 24 };
            let mut file_items = files
                .into_iter()
                .filter_map(|(key, name)| {
                    let description =
                        format!("{} · {}", key.path, self.file_scope_label(&key.scope_id));
                    palette_resource_score(PaletteCategory::Files, &name, &description, query).map(
                        |score| PaletteItem {
                            category: PaletteCategory::Files,
                            label: name,
                            description,
                            shortcut: String::new(),
                            target: PaletteTarget::File(key),
                            score,
                        },
                    )
                })
                .collect::<Vec<_>>();
            file_items.sort_by(|a, b| (a.score, &a.label).cmp(&(b.score, &b.label)));
            items.extend(file_items.into_iter().take(file_limit));
        }

        if include(PaletteCategory::Sessions) {
            let session_limit = if query.is_empty() { 8 } else { 24 };
            let mut session_items = Vec::new();
            for session in &self.snapshot.sessions {
                for branch in &session.branches {
                    let label = session.title.clone();
                    let description = format!("{} · {}", branch.name, session.directory);
                    if let Some(score) = palette_resource_score(
                        PaletteCategory::Sessions,
                        &label,
                        &description,
                        query,
                    ) {
                        session_items.push(PaletteItem {
                            category: PaletteCategory::Sessions,
                            label,
                            description,
                            shortcut: String::new(),
                            target: PaletteTarget::Session {
                                session: session.id.clone(),
                                branch: branch.id.clone(),
                            },
                            score,
                        });
                    }
                }
            }
            session_items.sort_by(|a, b| (a.score, &a.label).cmp(&(b.score, &b.label)));
            items.extend(session_items.into_iter().take(session_limit));
        }

        if include(PaletteCategory::Agents) {
            // These are actual session relationships from the owner snapshot,
            // not a parallel runtime registry or an inference-on-browse path.
            let mut agents = BTreeMap::<String, String>::new();
            if let Some(current) = &self.snapshot.session {
                agents.insert(
                    current.clone(),
                    if self.snapshot.busy {
                        "current · working"
                    } else {
                        "current · idle"
                    }
                    .into(),
                );
                for child in &self.snapshot.subagents {
                    if child.parent_session == *current {
                        agents.insert(
                            child.child_session.clone(),
                            format!("child · {} · {}", child.state, child.mode),
                        );
                    } else if child.child_session == *current {
                        agents.insert(child.parent_session.clone(), "parent".into());
                    }
                }
            }
            let mut agent_items = Vec::new();
            for (session_id, state) in agents {
                let label = self.session_title(&session_id);
                let description = format!("{state} · {session_id}");
                let branch = if self.snapshot.session.as_ref() == Some(&session_id) {
                    self.snapshot.branch_id.clone()
                } else {
                    self.snapshot
                        .sessions
                        .iter()
                        .find(|session| session.id == session_id)
                        .and_then(|session| {
                            session
                                .branches
                                .iter()
                                .find(|branch| branch.name == "main")
                                .or_else(|| session.branches.first())
                        })
                        .map(|branch| branch.id.clone())
                };
                if let (Some(score), Some(branch)) = (
                    palette_resource_score(PaletteCategory::Agents, &label, &description, query),
                    branch,
                ) {
                    agent_items.push(PaletteItem {
                        category: PaletteCategory::Agents,
                        label,
                        description,
                        shortcut: String::new(),
                        target: PaletteTarget::Session {
                            session: session_id,
                            branch,
                        },
                        score,
                    });
                }
            }
            agent_items.sort_by(|a, b| (a.score, &a.label).cmp(&(b.score, &b.label)));
            items.extend(agent_items.into_iter().take(24));
        }

        if include(PaletteCategory::Windows) {
            let window_limit = if query.is_empty() { 8 } else { 24 };
            let mut window_items = Vec::new();
            for (tab_id, tab) in &self.layout.tabs {
                let Some(group) = self.layout.group_for(*tab_id) else {
                    continue;
                };
                let label = self.tab_label(&tab.target, tab.pinned);
                let description = match &tab.target {
                    Target::Conversation => "Agent".into(),
                    Target::SessionTree => "Durable session tree".into(),
                    Target::ContextFlow => "Context and provenance".into(),
                    Target::Activity => "Agent activity".into(),
                    Target::File { scope_id, path } => {
                        format!("File · {path} · {}", self.file_scope_label(scope_id))
                    }
                    Target::Diff { review } => self
                        .git
                        .reviews
                        .get(review)
                        .map(|(_, review)| format!("Diff · {}", review.path_label))
                        .unwrap_or_else(|| "Review diff".into()),
                    Target::Terminal { .. } => "Open terminal tab".into(),
                    Target::Settings => "Fullscreen settings".into(),
                    Target::Help => "Keyboard shortcuts and help".into(),
                    Target::AgentView {
                        session_id,
                        branch_id,
                        ..
                    } => format!("Read only · {session_id} · {branch_id}"),
                };
                if let Some(score) =
                    palette_resource_score(PaletteCategory::Windows, &label, &description, query)
                {
                    window_items.push(PaletteItem {
                        category: PaletteCategory::Windows,
                        label,
                        description,
                        shortcut: String::new(),
                        target: PaletteTarget::Window {
                            group,
                            tab: *tab_id,
                        },
                        score,
                    });
                }
            }
            window_items.sort_by(|a, b| (a.score, &a.label).cmp(&(b.score, &b.label)));
            items.extend(window_items.into_iter().take(window_limit));
        }

        if include(PaletteCategory::Terminals) {
            if let Some(pool) = &self.terminal_pool {
                let terminal_limit = if query.is_empty() { 4 } else { 16 };
                let mut terminal_items = pool
                    .all()
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, terminal)| {
                        let label = format!("Terminal {}", index + 1);
                        let description = format!(
                            "{:?} · {}",
                            terminal.status(),
                            aw_workspace::editor::display(&terminal.root.display().to_string())
                        );
                        palette_resource_score(
                            PaletteCategory::Terminals,
                            &label,
                            &description,
                            query,
                        )
                        .map(|score| PaletteItem {
                            category: PaletteCategory::Terminals,
                            label,
                            description,
                            shortcut: "Ctrl+`".into(),
                            target: PaletteTarget::Terminal(terminal.id.clone()),
                            score,
                        })
                    })
                    .collect::<Vec<_>>();
                terminal_items.sort_by(|a, b| (a.score, &a.label).cmp(&(b.score, &b.label)));
                items.extend(terminal_items.into_iter().take(terminal_limit));
            }
        }

        if query.is_empty() && parsed.category.is_none() {
            items.sort_by(|a, b| {
                (a.category, a.score, &a.label).cmp(&(b.category, b.score, &b.label))
            });
        } else {
            items.sort_by(|a, b| {
                (a.score, a.category, &a.label).cmp(&(b.score, b.category, &b.label))
            });
        }
        items
    }

    fn reconcile_layout_state(&mut self) {
        self.reconcile_file_views();
        self.reconcile_agent_views();
        let groups = self.layout.groups();
        self.container_last_group.retain(|container, group| {
            self.layout
                .container(*container)
                .is_some_and(|owner| owner.groups.contains(group))
        });
        if matches!(
            self.layout.panes.get(&self.layout.focused),
            Some(Pane::Group { .. })
        ) {
            self.last_group = self.layout.focused;
            if let Some(container) = self.layout.container_for(self.layout.focused) {
                self.container_last_group
                    .insert(container, self.layout.focused);
            }
        } else if !groups.contains(&self.last_group) {
            self.last_group = groups[0];
        }
        self.scroll
            .retain(|id, _| self.layout.tabs.contains_key(id));
        self.horizontal
            .retain(|id, _| self.layout.tabs.contains_key(id));
        self.line_counts
            .retain(|id, _| self.layout.tabs.contains_key(id));
        self.rendered
            .retain(|id, _| self.layout.tabs.contains_key(id));
        self.find.retain(|id, _| self.layout.tabs.contains_key(id));
        self.find_generation
            .retain(|id, _| self.layout.tabs.contains_key(id));
        self.find_cancel.retain(|id, cancel| {
            let keep = self.layout.tabs.contains_key(id);
            if !keep {
                cancel.store(true, Ordering::Release);
            }
            keep
        });
        if self
            .text_selection
            .as_ref()
            .is_some_and(|selection| !self.layout.tabs.contains_key(&selection.tab))
        {
            self.text_selection = None;
            self.selection_dragging = false;
            self.selection_moved = false;
        }
        self.nav
            .retain(|(id, _), _| self.layout.panes.contains_key(id));
        self.git
            .panes
            .retain(|id, _| self.layout.panes.contains_key(id));
        self.reconcile_git_views();
        self.terminal_scroll
            .retain(|id, _| self.layout.tabs.contains_key(id));
        if self.terminal_return.is_some_and(|(pane, tab)| {
            !self.layout.panes.contains_key(&pane)
                || tab.is_some_and(|tab| self.layout.group_for(tab) != Some(pane))
        }) {
            self.terminal_return = None;
        }
    }

    fn apply(&mut self, a: Action) {
        match self.layout.apply(a) {
            Ok(()) => self.layout_generation = self.layout_generation.wrapping_add(1),
            Err(e) => self.notice = e.to_string(),
        }
        self.reconcile_layout_state();
    }

    fn group(&self) -> Id {
        if let Some(container_id) = self.layout.container_for(self.layout.focused) {
            if let Some(container) = self.layout.container(container_id) {
                if let Some(group) = self
                    .container_last_group
                    .get(&container_id)
                    .filter(|group| container.groups.contains(group))
                {
                    return *group;
                }
                if let Some(group) = container.groups.first() {
                    return *group;
                }
            }
        }
        if self.layout.groups().contains(&self.last_group) {
            self.last_group
        } else {
            self.layout.groups()[0]
        }
    }

    fn nav_display_icon(display: NavDisplay) -> &'static str {
        match display {
            NavDisplay::List => LIST_VIEW_ICON,
            NavDisplay::Tree => TREE_VIEW_ICON,
        }
    }

    fn git_display_icon(display: git::Display) -> &'static str {
        match display {
            git::Display::List => LIST_VIEW_ICON,
            git::Display::Tree => TREE_VIEW_ICON,
        }
    }

    fn navigator_color(&self, kind: Navigator) -> Color {
        let palette = crate::tui::theme::palette_for(&self.ui_config);
        match kind {
            Navigator::Sessions => palette.accent,
            Navigator::Files => palette.success,
            Navigator::Git => palette.warning,
        }
    }

    fn parent_directory(path: &str) -> String {
        path.rsplit_once('/')
            .map_or(String::new(), |(parent, _)| parent.to_owned())
    }

    fn list_directory_on_switch(state: &NavState) -> String {
        if state.display == NavDisplay::List {
            return state.directory.clone();
        }
        match Self::file_rows(state)
            .get(state.selected)
            .map(|row| &row.item)
        {
            Some(NavigatorItem::File(entry)) if entry.kind == EntryKind::Directory => {
                entry.path.clone()
            }
            Some(NavigatorItem::File(entry)) => Self::parent_directory(&entry.path),
            _ => String::new(),
        }
    }

    fn set_navigator_display(
        &mut self,
        browser: &Browser,
        pane: Id,
        kind: Navigator,
        display: NavDisplay,
    ) {
        let list_directory = (kind == Navigator::Files && display == NavDisplay::List).then(|| {
            self.nav
                .get(&(pane, Navigator::Files))
                .map(Self::list_directory_on_switch)
                .unwrap_or_default()
        });
        let mut request = None;
        {
            let state = self.nav.entry((pane, kind)).or_default();
            if state.display == display {
                return;
            }
            state.display = display;
            state.selected = 0;
            if kind == Navigator::Files {
                match display {
                    NavDisplay::List => {
                        let directory = list_directory.unwrap_or_default();
                        state.directory = directory.clone();
                        state.pending = None;
                        if let Some(entries) = state.directories.get(&directory).cloned() {
                            state.entries = entries;
                            state.loaded = true;
                        } else {
                            state.entries.clear();
                            state.loaded = false;
                            request = Some(directory);
                        }
                    }
                    NavDisplay::Tree => {
                        if !state.directories.contains_key("") {
                            state.loaded = false;
                            state.pending = None;
                            request = Some(String::new());
                        }
                    }
                }
            }
        }
        if let Some(path) = request {
            self.list_files(browser, pane, path);
        }
    }

    fn open_file_preview_key(&mut self, key: FileKey) -> Option<Id> {
        let focused = self.layout.focused;
        let group = self.group();
        self.apply(Action::Open {
            group,
            target: key.target(),
            pinned: false,
        });
        let tab = self
            .layout
            .active(group)
            .filter(|tab| tab.target == key.target())
            .map(|tab| tab.id);
        if self.layout.panes.contains_key(&focused) {
            self.apply(Action::Focus(focused));
        }
        self.input = false;
        tab
    }

    #[cfg(test)]
    fn open_file_preview(&mut self, path: String) {
        self.open_file_preview_key(FileKey::new(self.current_file_scope(), path));
    }

    fn focus_palette_file(&mut self, group: Id, key: &FileKey) {
        if self
            .layout
            .active(group)
            .is_some_and(|tab| tab.target == key.target())
        {
            self.apply(Action::Focus(group));
            self.input = false;
        }
    }

    fn open(&mut self, target: Target) {
        let input = matches!(target, Target::Conversation);
        self.apply(Action::Open {
            group: self.group(),
            target,
            pinned: true,
        });
        self.input = input;
    }

    fn focus_current_agent(&mut self, session: &str, branch: &str) -> bool {
        if self.snapshot.session.as_deref() != Some(session)
            || self.snapshot.branch_id.as_deref() != Some(branch)
        {
            return false;
        }
        let existing = self.layout.tabs.values().find_map(|tab| {
            matches!(tab.target, Target::Conversation)
                .then(|| self.layout.group_for(tab.id).map(|group| (group, tab.id)))
                .flatten()
        });
        if let Some((group, tab)) = existing {
            self.apply(Action::Select { group, tab });
            self.input = true;
        } else {
            self.open(Target::Conversation);
        }
        true
    }
    fn explorer_pane(&self) -> Option<Id> {
        self.layout
            .ordered_panes()
            .into_iter()
            .find(|id| matches!(self.layout.panes.get(id), Some(Pane::Navigator { .. })))
    }

    fn set_explorer_placement(&mut self, placement: ExplorerPlacement) {
        let group = self.group();
        let previous_focus = self.layout.focused;
        match placement {
            ExplorerPlacement::Hidden => {
                self.explorer_placement = ExplorerPlacement::Hidden;
                if self.explorer_pane() == Some(self.layout.focused) {
                    self.apply(Action::Focus(group));
                }
            }
            ExplorerPlacement::Left | ExplorerPlacement::Right => {
                let side = if placement == ExplorerPlacement::Left {
                    Side::Left
                } else {
                    Side::Right
                };
                if let Some(pane) = self.explorer_pane() {
                    if pane != group {
                        self.apply(Action::MovePane {
                            pane,
                            beside: group,
                            side,
                        });
                    }
                } else {
                    self.apply(Action::AddNavigator {
                        beside: group,
                        side,
                        kind: Navigator::Sessions,
                    });
                }
                self.explorer_placement = placement;
                if self.layout.panes.contains_key(&previous_focus)
                    && !matches!(
                        self.layout.panes.get(&previous_focus),
                        Some(Pane::Navigator { .. })
                    )
                {
                    self.apply(Action::Focus(previous_focus));
                }
            }
        }
        self.input = matches!(
            self.layout
                .active(self.layout.focused)
                .map(|tab| &tab.target),
            Some(Target::Conversation)
        );
    }

    fn set_navigator(&mut self, kind: Navigator) {
        if self.explorer_placement == ExplorerPlacement::Hidden {
            self.set_explorer_placement(ExplorerPlacement::Left);
        }
        let pane = if matches!(
            self.layout.panes.get(&self.layout.focused),
            Some(Pane::Navigator { .. })
        ) {
            Some(self.layout.focused)
        } else {
            self.owned_navigator().or_else(|| {
                self.layout
                    .panes
                    .iter()
                    .find_map(|(id, p)| matches!(p, Pane::Navigator { .. }).then_some(*id))
            })
        };
        if let Some(pane) = pane {
            let leaving_files = matches!(
                self.layout.panes.get(&pane),
                Some(Pane::Navigator {
                    kind: Navigator::Files
                })
            ) && kind != Navigator::Files;
            if leaving_files {
                self.cancel_file_searches(Some(pane));
            }
            self.apply(Action::SetNavigator { pane, kind });
            self.apply(Action::Focus(pane));
        } else {
            self.apply(Action::AddNavigator {
                beside: self.group(),
                side: Side::Left,
                kind,
            });
        }
        self.input = false;
        if kind == Navigator::Git {
            self.git.ensure(self.layout.focused);
            self.notice = "Git views are explicit human reads; model/root remain unchanged".into();
        }
    }
    fn owner(&mut self, worker: &Worker, command: Command, changes_target: bool) -> bool {
        if changes_target && (!self.draft.is_empty() || !self.recovery.is_empty()) {
            self.notice="Draft/recovery preserved: submit or explicitly clear it before changing the session/branch.".into();
            return false;
        }
        if let Err(e) = worker.send(command) {
            self.notice = e.to_string();
            false
        } else {
            self.notice.clear();
            self.snapshot = worker.projection.snapshot();
            true
        }
    }
    fn file_rows(n: &NavState) -> Vec<NavigatorRow> {
        let query = n.filter.trim().to_lowercase();
        if !query.is_empty()
            && n.search_pending.is_none()
            && n.search_query.eq_ignore_ascii_case(n.filter.trim())
        {
            return n
                .search_results
                .iter()
                .cloned()
                .map(|entry| {
                    let marker = match entry.kind {
                        EntryKind::Directory => ">",
                        EntryKind::File => " ",
                        EntryKind::Unsupported => "!",
                    };
                    NavigatorRow {
                        label: format!(
                            "{marker} {}{}",
                            entry.path,
                            if entry.kind == EntryKind::Directory {
                                "/"
                            } else {
                                ""
                            }
                        ),
                        item: NavigatorItem::File(entry),
                    }
                })
                .collect();
        }
        if n.display == NavDisplay::List {
            let mut rows = Vec::new();
            if !n.directory.is_empty() {
                rows.push(NavigatorRow {
                    label: "../".into(),
                    item: NavigatorItem::Parent,
                });
            }
            rows.extend(
                n.entries
                    .iter()
                    .filter(|entry| entry.name.to_lowercase().contains(&query))
                    .cloned()
                    .map(|entry| {
                        let marker = match entry.kind {
                            EntryKind::Directory => ">",
                            EntryKind::File => " ",
                            EntryKind::Unsupported => "!",
                        };
                        NavigatorRow {
                            label: format!(
                                "{marker} {}{}",
                                entry.name,
                                if entry.kind == EntryKind::Directory {
                                    "/"
                                } else {
                                    ""
                                }
                            ),
                            item: NavigatorItem::File(entry),
                        }
                    }),
            );
            return rows;
        }

        fn walk(path: &str, depth: usize, n: &NavState, query: &str, rows: &mut Vec<NavigatorRow>) {
            let Some(entries) = n.directories.get(path) else {
                return;
            };
            for entry in entries {
                let is_dir = entry.kind == EntryKind::Directory;
                let expanded = is_dir && n.expanded.contains(&entry.path);
                let matches = query.is_empty()
                    || entry.name.to_lowercase().contains(query)
                    || entry.path.to_lowercase().contains(query);
                if matches {
                    rows.push(NavigatorRow {
                        label: format!(
                            "{}{} {}{}",
                            "  ".repeat(depth),
                            match entry.kind {
                                EntryKind::Directory if expanded => "v",
                                EntryKind::Directory => ">",
                                EntryKind::File => " ",
                                EntryKind::Unsupported => "!",
                            },
                            entry.name,
                            if is_dir { "/" } else { "" }
                        ),
                        item: NavigatorItem::File(entry.clone()),
                    });
                }
                if expanded {
                    walk(&entry.path, depth + 1, n, query, rows);
                }
            }
        }

        let mut rows = Vec::new();
        walk("", 0, n, &query, &mut rows);
        rows
    }

    fn session_rows(&self, pane: Id) -> Vec<NavigatorRow> {
        let state = self
            .nav
            .get(&(pane, Navigator::Sessions))
            .expect("session navigator state exists while rendering");
        let include_unsaved = self.snapshot.session.is_none()
            && self
                .layout
                .tabs
                .values()
                .any(|tab| matches!(tab.target, Target::Conversation));
        session_nav::rows(session_nav::Projection {
            sessions: &self.snapshot.sessions,
            subagents: &self.snapshot.subagents,
            active_session: self.snapshot.session.as_deref(),
            active_branch: self.snapshot.branch_id.as_deref().unwrap_or(""),
            scope: state.session_scope,
            list: state.display == NavDisplay::List,
            query: &state.filter,
            collapsed: &state.collapsed,
            include_unsaved,
        })
        .into_iter()
        .map(|row| NavigatorRow {
            label: row.label,
            item: match row.target {
                session_nav::Target::Unsaved => NavigatorItem::UnsavedSession,
                session_nav::Target::Root { session } => NavigatorItem::SessionRoot { session },
                session_nav::Target::Branch { session, branch } => {
                    NavigatorItem::SessionBranch { session, branch }
                }
            },
        })
        .collect()
    }

    fn selected_session_id(&self, pane: Id) -> Option<String> {
        let selected = self
            .nav
            .get(&(pane, Navigator::Sessions))
            .map_or(0, |state| state.selected);
        self.session_rows(pane)
            .get(selected)
            .and_then(|row| navigator_session_id(&row.item))
            .map(str::to_owned)
    }

    fn toggle_selected_session(&mut self, pane: Id) {
        let Some(session) = self.selected_session_id(pane) else {
            self.notice = "Select a saved Session first".into();
            return;
        };
        let state = self.nav.entry((pane, Navigator::Sessions)).or_default();
        state.multi_select = true;
        if !state.selected_sessions.insert(session.clone()) {
            state.selected_sessions.remove(&session);
        }
        self.notice = if state.selected_sessions.is_empty() {
            "Multi-select active".into()
        } else {
            format!("{} Session(s) selected", state.selected_sessions.len())
        };
    }

    fn close_help_on_escape(&mut self) -> bool {
        let Some(tab) = self.layout.active(self.layout.focused).cloned() else {
            return false;
        };
        if !matches!(tab.target, Target::Help) {
            return false;
        }
        self.apply(Action::CloseTab {
            group: self.layout.focused,
            tab: tab.id,
        });
        true
    }

    fn selected_session_action_targets(&self, pane: Id, archived: bool) -> Vec<String> {
        let visible = self
            .session_rows(pane)
            .iter()
            .filter_map(|row| navigator_session_id(&row.item))
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        let mut sessions = self
            .nav
            .get(&(pane, Navigator::Sessions))
            .map(|state| {
                state
                    .selected_sessions
                    .intersection(&visible)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if sessions.is_empty() {
            if let Some(session) = self.selected_session_id(pane) {
                sessions.push(session);
            }
        }
        sessions.sort();
        sessions.dedup();
        if archived {
            if let Some(active) = self.snapshot.session.as_deref() {
                sessions.retain(|session| session != active);
            }
        }
        sessions
    }

    fn set_selected_sessions_archived(&mut self, pane: Id, worker: &Worker, archived: bool) {
        let sessions = self.selected_session_action_targets(pane, archived);
        if sessions.is_empty() {
            self.notice = if archived && self.snapshot.session.is_some() {
                "The active Agent stays visible; switch to another Agent before archiving it."
                    .into()
            } else {
                "Select a saved Session first.".into()
            };
            return;
        }
        if self.owner(
            worker,
            Command::SetSessionsArchived { sessions, archived },
            false,
        ) {
            let state = self.nav.entry((pane, Navigator::Sessions)).or_default();
            state.selected_sessions.clear();
            state.selected = state.selected.saturating_sub(1);
        }
    }
}
pub async fn event_loop(
    mut ui: Ui,
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    worker: &Worker,
    browser: &mut Browser,
    initial: Option<String>,
) -> Result<(Ui, Exit)> {
    ui.branch_reader = worker.branch_reader();
    let mut view_refresh = tokio::time::interval(Duration::from_millis(250));
    view_refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut input = EventStream::new();
    let mut watch = worker.projection.subscribe();
    let mut dirty = true;
    let mut terminal_watch = ui.terminal_pool.as_ref().map(|pool| pool.subscribe());
    let mut last = tokio::time::Instant::now() - Duration::from_millis(20);
    let mut last_subagent_refresh = tokio::time::Instant::now();
    let mut last_animation = tokio::time::Instant::now();
    if let Some(prompt) = initial {
        ui.open(Target::Conversation);
        ui.draft
            .set(prompt)
            .map_err(|e| Error::Config(e.to_string()))?;
        terminal.draw(|f| ui.draw(f))?;
        ui.send_draft(worker, browser);
    } else {
        ui.owner(worker, Command::Refresh, false);
    }
    loop {
        if ui.snapshot.fatal {
            return Err(Error::Storage(
                "workspace owner panicked; terminal will restore, recover store before continuing",
            ));
        }
        if let Some(exit) = ui.exit {
            if exit == Exit::Quit {
                worker.projection.cancel();
            }
            if !ui.snapshot.busy
                && !ui.snapshot.state_pending
                && !ui.layout_pending
                && !ui.terminal_pending
            {
                return Ok((ui, exit));
            }
        }
        ui.visible_files(browser);
        let chord_deadline = ui
            .chord
            .map(|started| tokio::time::Instant::from_std(started + KEY_CHORD_TIMEOUT));
        let poll_subagents = !ui.snapshot.busy
            && !ui.snapshot.state_pending
            && ui
                .snapshot
                .subagents
                .iter()
                .any(|item| matches!(item.state.as_str(), "admitted" | "running"));
        let subagent_refresh_deadline = last_subagent_refresh + Duration::from_millis(500);
        let animate_progress = ui.snapshot.busy
            || ui
                .snapshot
                .subagents
                .iter()
                .any(|item| matches!(item.state.as_str(), "admitted" | "running"));
        let animation_deadline = last_animation + Duration::from_millis(260);
        tokio::select! {
            _=view_refresh.tick(),if ui.layout.tabs.values().any(|tab|matches!(tab.target,Target::AgentView{..}))=>{ui.branch_reader=worker.branch_reader();ui.schedule_agent_views();dirty=true;},
            event=input.next()=>{match event{Some(Ok(Event::Key(key)))=>ui.key(key,worker,browser),Some(Ok(Event::Paste(text)))=>{
                if let Some(p)=&mut ui.palette{if let Err(e)=p.paste(&text){ui.notice=e.to_string();}else{let query=p.text().to_owned();ui.sync_palette_file_search(browser,&query);}}
                else if ui.terminal_paste(&text){}
                else if ui.input&&matches!(ui.layout.active(ui.layout.focused).map(|t|&t.target),Some(Target::Conversation)){if let Err(e)=ui.draft.insert(&text){ui.notice=e.to_string();}else{ui.composer.reset_after_external_edit();}}
            },Some(Ok(Event::Mouse(mouse)))=>ui.mouse(mouse,worker,browser),Some(Ok(Event::Resize(..)))=>{ui.can_submit=false;},Some(Ok(_))=>{},Some(Err(e))=>return Err(e.into()),None=>return Err(Error::Io(std::io::Error::new(std::io::ErrorKind::UnexpectedEof,"terminal event stream closed")))}dirty=true;},
            changed=watch.changed()=>{if changed.is_err(){return Err(Error::Storage("workspace owner disconnected"));}ui.snapshot=worker.projection.snapshot();ui.expire_agent_views();
                if ui.snapshot.rejected_revision!=ui.recovery_seen{
                    ui.recovery_seen=ui.snapshot.rejected_revision;
                    if ui.draft.is_empty(){let _=ui.draft.set(ui.snapshot.rejected_prompt.clone());ui.composer.reset_after_external_edit();ui.notice="Preflight failed; rejected input returned to your composer. No automatic retry.".into();}
                    else{ui.recovery=ui.snapshot.rejected_prompt.clone();ui.notice="Preflight failed. Newer draft retained; recover-draft can restore the older input explicitly.".into();}
                }
                dirty=true;},
            Some(reply)=browser.replies.recv()=>{ui.response(reply);dirty=true;},
            Some(reply)=ui.git.next()=>{ui.git_response(reply);dirty=true;},
            _=async{if let Some(w)=&mut terminal_watch{let _=w.changed().await;}else{std::future::pending::<()>().await;}}=>{dirty=true;},
            Some(result)=ui.background_rx.recv()=>{
                ui.apply_background_result(result);
                dirty=true;
            },
            _=tokio::time::sleep_until(subagent_refresh_deadline),if poll_subagents=>{
                last_subagent_refresh=tokio::time::Instant::now();
                if worker.send(Command::Refresh).is_ok(){ui.snapshot=worker.projection.snapshot();}
                dirty=true;
            },
            _=tokio::time::sleep_until(animation_deadline),if animate_progress=>{
                last_animation=tokio::time::Instant::now();
                dirty=true;
            },
            _=async {
                if let Some(deadline)=chord_deadline {
                    tokio::time::sleep_until(deadline).await;
                } else {
                    std::future::pending::<()>().await;
                }
            },if chord_deadline.is_some()=>{
                if ui.expire_chord(){dirty=true;}
            },
            _=tokio::time::sleep_until(last+Duration::from_millis(16)),if dirty=>{terminal.draw(|f|ui.draw(f))?;ui.schedule_visible_background_work();last=tokio::time::Instant::now();dirty=false;},
        }
    }
}
fn chord_is_live(started: Instant) -> bool {
    started.elapsed() <= KEY_CHORD_TIMEOUT
}

fn tab_cycle_direction(key: &KeyEvent) -> Option<bool> {
    if !key.modifiers.contains(KeyModifiers::CONTROL)
        || !matches!(key.code, KeyCode::Tab | KeyCode::BackTab)
    {
        return None;
    }
    Some(key.code != KeyCode::BackTab && !key.modifiers.contains(KeyModifiers::SHIFT))
}

fn is_ctrl_char(key: &KeyEvent, expected: char) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char(c) if c.eq_ignore_ascii_case(&expected))
}

fn is_ctrl_shift_char(key: &KeyEvent, expected: char) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL)
        && key.modifiers.contains(KeyModifiers::SHIFT)
        && matches!(key.code, KeyCode::Char(c) if c.eq_ignore_ascii_case(&expected))
}

fn is_command_palette_key(key: &KeyEvent, allow_collapsed_ctrl_p: bool) -> bool {
    if !key.modifiers.contains(KeyModifiers::CONTROL)
        || !matches!(key.code, KeyCode::Char('P' | 'p'))
    {
        return false;
    }
    key.modifiers.contains(KeyModifiers::SHIFT) || allow_collapsed_ctrl_p
}

fn rect_contains(area: Rect, point: (u16, u16)) -> bool {
    point.0 >= area.x
        && point.0 < area.x.saturating_add(area.width)
        && point.1 >= area.y
        && point.1 < area.y.saturating_add(area.height)
}

fn truncate_cells(text: &str, max: usize) -> String {
    if UnicodeWidthStr::width(text) <= max {
        return text.to_owned();
    }
    let mut out = String::new();
    let limit = max.saturating_sub(1);
    let mut width = 0usize;
    for grapheme in text.graphemes(true) {
        let next = UnicodeWidthStr::width(grapheme);
        if width.saturating_add(next) > limit {
            break;
        }
        out.push_str(grapheme);
        width = width.saturating_add(next);
    }
    out.push('…');
    out
}
fn wrap(text: &str, width: u16) -> Vec<String> {
    let text = aw_workspace::editor::display(text);
    let mut out = vec![String::new()];
    let mut column = 0;
    for atom in text.graphemes(true) {
        if atom == "\n" {
            out.push(String::new());
            column = 0;
            continue;
        }
        let w = UnicodeWidthStr::width(atom);
        if column + w > usize::from(width) && column > 0 {
            out.push(String::new());
            column = 0;
        }
        if let Some(row) = out.last_mut() {
            row.push_str(atom);
        }
        column += w;
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markup_is_plain_and_escapes_are_not_emitted() {
        let lines = wrap("<script>hello</script>\x1b]52;c;data\x07", 80);
        assert!(!lines.join("").contains('\x1b'));
        assert!(lines.join("").contains("<script>"));
    }
    #[test]
    fn narrow_wrap_does_not_drop_words() {
        assert_eq!(wrap("abcdefgh", 4), vec!["abcd", "efgh"]);
    }
    #[test]
    fn ctrl_k_chord_expires_instead_of_stealing_a_later_key() {
        assert!(chord_is_live(Instant::now()));
        assert!(!chord_is_live(
            Instant::now() - KEY_CHORD_TIMEOUT - Duration::from_millis(1)
        ));
    }

    #[test]
    fn tab_cycle_accepts_crossterm_backtab_without_stealing_plain_shift_tab() {
        assert_eq!(
            tab_cycle_direction(&KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL)),
            Some(true)
        );
        assert_eq!(
            tab_cycle_direction(&KeyEvent::new(
                KeyCode::Tab,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )),
            Some(false)
        );
        assert_eq!(
            tab_cycle_direction(&KeyEvent::new(
                KeyCode::BackTab,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )),
            Some(false)
        );
        assert_eq!(
            tab_cycle_direction(&KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)),
            None
        );
    }

    #[test]
    fn ctrl_tab_from_navigator_targets_last_content_group() {
        let mut ui = Ui::new(
            Workspace::default(),
            String::new(),
            None,
            PathBuf::from("/tmp"),
            Snapshot {
                generation: 0,
                busy: false,
                state_pending: false,
                fatal: false,
                store_id: None,
                branch_id: None,
                session: None,
                session_title: None,
                branch: "main".into(),
                model: "fixture".into(),
                profile: "fixture".into(),
                provider: "fixture".into(),
                effort: String::new(),
                actor: "unix:fixture".into(),
                root: "/tmp".into(),
                agent_root: Some(PathBuf::from("/tmp")),
                tools: "off".into(),
                needs_inspection: false,
                turns: Arc::new(vec![]),
                pending_prompt: String::new(),
                live: String::new(),
                thinking: false,
                turn_started: None,
                live_tools: vec![],
                rejected_prompt: String::new(),
                rejected_revision: 0,
                activity: std::collections::VecDeque::new(),
                event_rail: None,
                sessions: vec![],
                subagents: vec![],
                delegation_budget: None,
                tree: String::new(),
                context_output: String::new(),
                context_blocks: vec![],
                loaded_resources: Default::default(),
                status: "fixture".into(),
                auxiliary_output: None,
                performance: None,
                session_tps: None,
            },
            aw::config::UiConfig::default(),
        )
        .unwrap();
        let group = ui.group();
        ui.apply(Action::Open {
            group,
            target: Target::Activity,
            pinned: true,
        });
        ui.apply(Action::Select {
            group,
            tab: ui
                .layout
                .panes
                .get(&group)
                .and_then(|pane| match pane {
                    Pane::Group { tabs, .. } => tabs.first().copied(),
                    _ => None,
                })
                .unwrap(),
        });
        let before = ui.layout.active(group).unwrap().id;
        ui.apply(Action::Focus(1));
        ui.apply(Action::CycleTab {
            group: ui.group(),
            forward: true,
        });
        assert_ne!(ui.layout.active(group).unwrap().id, before);
        assert_eq!(ui.layout.focused, group);
    }

    #[test]
    fn familiar_global_shortcuts_replace_the_legacy_prefix() {
        assert!(is_command_palette_key(
            &KeyEvent::new(
                KeyCode::Char('P'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            ),
            false,
        ));
        assert!(!is_command_palette_key(
            &KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
            false,
        ));
        assert!(is_command_palette_key(
            &KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
            true,
        ));
        assert!(!is_command_palette_key(
            &KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
            true,
        ));
        assert!(is_ctrl_char(
            &KeyEvent::new(KeyCode::Char('\x60'), KeyModifiers::CONTROL),
            '\x60'
        ));
        assert!(!is_ctrl_char(
            &KeyEvent::new(KeyCode::Char(']'), KeyModifiers::CONTROL),
            'k'
        ));
        assert!(is_ctrl_shift_char(
            &KeyEvent::new(
                KeyCode::Char('E'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            ),
            'e'
        ));
        assert!(!is_ctrl_shift_char(
            &KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
            'e'
        ));
    }
    #[test]
    fn palette_and_slash_selection_wrap_in_both_directions() {
        assert_eq!(palette::step(4, 5, false), 3);
        assert_eq!(palette::step(999, 5, false), 3);
        assert_eq!(palette::step(4, 5, true), 0);
        assert_eq!(palette::step(0, 5, false), 4);
        assert_eq!(palette::step(0, 0, false), 0);
    }

    #[test]
    fn key_insertion_and_newline() {
        let mut e = Editor::default();
        edit_key(
            &mut e,
            KeyEvent::new(KeyCode::Char('é'), KeyModifiers::NONE),
            EditMode::Composer,
        )
        .unwrap();
        edit_key(
            &mut e,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
            EditMode::Composer,
        )
        .unwrap();
        assert_eq!(e.text(), "é\n");
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use std::collections::VecDeque;
    include!("ui/ux_visual_tests.rs");
    pub(super) fn projection() -> Snapshot {
        Snapshot {
            generation: 0,
            busy: false,
            state_pending: false,
            fatal: false,
            store_id: None,
            branch_id: None,
            session: None,
            session_title: None,
            branch: "main".into(),
            model: "fixture".into(),
            profile: "fixture-account".into(),
            provider: "fixture".into(),
            effort: String::new(),
            actor: "unix:fixture".into(),
            root: "/tmp".into(),
            agent_root: Some(PathBuf::from("/tmp")),
            tools: "off".into(),
            needs_inspection: false,
            turns: Arc::new(vec![]),
            pending_prompt: String::new(),
            live: String::new(),
            thinking: false,
            turn_started: None,
            live_tools: vec![],
            rejected_prompt: String::new(),
            rejected_revision: 0,
            activity: VecDeque::new(),
            event_rail: None,
            sessions: vec![],
            subagents: vec![],
            delegation_budget: None,
            tree: String::new(),
            context_output: String::new(),
            context_blocks: Vec::new(),
            loaded_resources: Default::default(),
            status: "Fixture".into(),
            auxiliary_output: None,
            performance: None,
            session_tps: None,
        }
    }
    pub(super) fn model() -> Ui {
        Ui::new(
            Workspace::default(),
            "draft remains local".into(),
            None,
            PathBuf::from("/tmp"),
            projection(),
            aw::config::UiConfig::default(),
        )
        .unwrap()
    }

    #[test]
    fn aw_p6_8c_workspace_event_rail_is_opt_in_activity_data_not_assistant_transcript() {
        use aw_ui_extension::status_events::{EventContext, EventKind, EventRail, STATUS_PROTOCOL};
        let mut ui = model();
        let mut enabled = aw::config::UiConfig::default();
        assert!(!enabled.show_event_notices);
        enabled.show_event_notices = true;
        let registry = crate::tui::extensions::event_registry(&enabled);
        let mut rail = EventRail::new("s_1").unwrap();
        rail.publish(
            &registry,
            &EventContext {
                protocol_version: STATUS_PROTOCOL,
                kind: EventKind::TurnEnd,
                session: "s_1".into(),
                turn_id: 2,
                sequence: 1,
                context: aw_ui_extension::Context {
                    session: "s_1".into(),
                    state: "ready".into(),
                    ..Default::default()
                },
            },
        )
        .unwrap();
        ui.snapshot.event_rail = Some(rail);
        let activity = ui.content_lines(&Target::Activity, 80).join("\n");
        assert!(activity.contains("Turn settled · turn 2"));
        assert!(
            !ui.conversation_lines(80)
                .join("\n")
                .contains("Turn settled · turn 2"),
            "factual UI notices cannot become assistant turns"
        );
        ui.snapshot.event_rail = None;
        assert!(
            !ui.content_lines(&Target::Activity, 80)
                .join("\n")
                .contains("Turn settled · turn 2"),
            "clearing a transient notice does not rewrite Session history"
        );
    }

    pub(super) fn turn_with_trace(
        prompt: &str,
        answer: &str,
        outcome: aw::conversation::Outcome,
        error: Option<&str>,
        terminal_trace_answer: Option<&str>,
    ) -> aw::conversation::Turn {
        let mut context_messages = vec![aw_agent_core::Message::text(
            1024,
            aw_agent_core::Role::User,
            prompt,
        )];
        if let Some(content) = terminal_trace_answer {
            context_messages.push(aw_agent_core::Message::text(
                1025,
                aw_agent_core::Role::Assistant,
                content,
            ));
        }
        aw::conversation::Turn {
            id: 1,
            prompt: prompt.into(),
            answer: answer.into(),
            model: "fixture".into(),
            profile: "fixture-account".into(),
            outcome,
            meta: None,
            error: error.map(str::to_owned),
            actor: None,
            tool_records: Vec::new(),
            invocations: Vec::new(),
            context_messages,
            needs_reconciliation: false,
        }
    }

    #[test]
    fn workspace_agent_uses_shared_prompt_history_reducer() {
        let mut ui = model();
        ui.snapshot.turns = Arc::new(vec![
            turn_with_trace(
                "first prompt",
                "first answer",
                aw::conversation::Outcome::Completed,
                None,
                Some("first answer"),
            ),
            turn_with_trace(
                "second prompt",
                "second answer",
                aw::conversation::Outcome::Completed,
                None,
                Some("second answer"),
            ),
        ]);
        ui.draft.set("scratch").unwrap();
        let history = ui.prompt_history();

        let previous = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert!(matches!(
            ui.composer
                .handle(&mut ui.draft, previous, &history, true)
                .unwrap(),
            ComposerAction::Handled
        ));
        assert_eq!(ui.draft.text(), "second prompt");

        let next = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL);
        ui.composer
            .handle(&mut ui.draft, next, &history, true)
            .unwrap();
        assert_eq!(ui.draft.text(), "scratch");
    }

    fn tool_record(
        status: aw_agent_core::ToolStatus,
        output_error: bool,
    ) -> aw_agent_core::ToolRecord {
        aw_agent_core::ToolRecord {
            call: aw_agent_core::ToolCall {
                id: "tool-1".into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command":"echo hello"}),
            },
            effect: aw_agent_core::ToolEffect::Process,
            execution_site: "local".into(),
            status,
            output: Some(if output_error {
                aw_agent_core::ToolOutput::error("boom")
            } else {
                aw_agent_core::ToolOutput::text("hello")
            }),
        }
    }

    #[test]
    fn conversation_falls_back_to_authoritative_answer_when_terminal_trace_is_empty() {
        let mut snapshot = projection();
        snapshot.turns = Arc::new(vec![turn_with_trace(
            "hello",
            "VISIBLE ANSWER",
            aw::conversation::Outcome::Completed,
            None,
            Some(""),
        )]);
        let ui = Ui::new(
            Workspace::default(),
            String::new(),
            None,
            PathBuf::from("/tmp"),
            snapshot,
            aw::config::UiConfig::default(),
        )
        .unwrap();
        let text = ui.conversation_lines(100).join("\n");
        assert!(text.contains("hello"));
        assert!(!text.contains("› hello"));
        assert!(text.contains("VISIBLE ANSWER"));
        assert!(!text.contains("completed turn contained no visible assistant output"));
    }

    #[test]
    fn conversation_restores_authoritative_answer_when_trace_is_stale_or_partial() {
        for (trace, answer) in [
            ("stale trace", "AUTHORITATIVE ANSWER"),
            ("world", "hello world"),
        ] {
            let mut snapshot = projection();
            snapshot.turns = Arc::new(vec![turn_with_trace(
                "hello",
                answer,
                aw::conversation::Outcome::Completed,
                None,
                Some(trace),
            )]);
            let ui = Ui::new(
                Workspace::default(),
                String::new(),
                None,
                PathBuf::from("/tmp"),
                snapshot,
                aw::config::UiConfig::default(),
            )
            .unwrap();
            let text = ui.conversation_lines(100).join("\n");
            assert!(
                text.contains(answer),
                "missing authoritative answer for trace {trace:?}"
            );
        }
    }

    #[test]
    fn conversation_shows_safe_error_for_incomplete_blank_turn() {
        let mut snapshot = projection();
        snapshot.turns = Arc::new(vec![turn_with_trace(
            "hello",
            "",
            aw::conversation::Outcome::Incomplete,
            Some("provider did not complete a text answer"),
            None,
        )]);
        snapshot.performance = Some(crate::tui::performance::PerformanceStats {
            elapsed: Duration::from_millis(300),
            first_token: None,
            input_tokens: None,
            output_tokens: None,
            throughput_eligible: false,
        });
        let ui = Ui::new(
            Workspace::default(),
            String::new(),
            None,
            PathBuf::from("/tmp"),
            snapshot,
            aw::config::UiConfig::default(),
        )
        .unwrap();
        let text = ui.conversation_lines(100).join("\n");
        assert!(text.contains("Thinking 0.3s"));
        assert!(text.contains("provider did not complete a text answer"));
    }

    #[test]
    fn fullscreen_markdown_rows_match_search_copy_projection_without_private_markers() {
        let markdown = "# Result\n\n- **bold** and *emphasis* with §code§\n> quoted\n\n§§§rust\nfn bird() {}\n§§§\n[docs](https://example.test)";
        let markdown = markdown.replace('§', &char::from(96).to_string());
        let mut snapshot = projection();
        snapshot.turns = Arc::new(vec![turn_with_trace(
            "show markdown",
            &markdown,
            aw::conversation::Outcome::Completed,
            None,
            Some(&markdown),
        )]);
        let mut ui = Ui::new(
            Workspace::default(),
            String::new(),
            None,
            PathBuf::from("/tmp"),
            snapshot,
            aw::config::UiConfig {
                theme: aw::config::ThemeMode::Dark,
                ..aw::config::UiConfig::default()
            },
        )
        .unwrap();
        let group = ui.group();
        let tab = ui.layout.active(group).unwrap().id;
        let styled = ui.conversation_view_lines(72);
        let visible = crate::tui::markdown::plain_lines(&styled);
        let cached = ui.rendered_lines(tab, &Target::Conversation, 72);
        assert_eq!(visible, *cached);
        let joined = visible.join("\n");
        assert!(joined.contains("Result"));
        assert!(joined.contains("• bold and emphasis with code"));
        assert!(joined.contains("│ quoted"));
        assert!(joined.contains("fn bird() {}"));
        assert!(joined.contains("docs (https://example.test)"));
        assert!(!joined.contains("**"));
        assert!(!joined.contains("§§§".replace('§', &char::from(96).to_string()).as_str()));
    }

    #[test]
    fn fullscreen_tool_strip_uses_shared_tone_across_the_agent_width() {
        let mut snapshot = projection();
        snapshot.pending_prompt = "run it".into();
        snapshot.live_tools = vec![tool_record(aw_agent_core::ToolStatus::Succeeded, false)];
        let mut ui = Ui::new(
            Workspace::default(),
            String::new(),
            None,
            PathBuf::from("/tmp"),
            snapshot,
            aw::config::UiConfig {
                theme: aw::config::ThemeMode::Dark,
                ..aw::config::UiConfig::default()
            },
        )
        .unwrap();
        let group = ui.group();
        let tab = ui.layout.active(group).unwrap().id;
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|frame| ui.draw(frame)).unwrap();
        let area = ui
            .selectable_views
            .get(&tab)
            .expect("conversation text view")
            .area;
        let palette = crate::tui::theme::palette_for(&ui.ui_config);
        let buffer = terminal.backend().buffer();
        let tool_y = (area.y..area.y.saturating_add(area.height))
            .find(|y| {
                (area.x..area.x.saturating_add(area.width))
                    .map(|x| buffer[(x, *y)].symbol())
                    .collect::<String>()
                    .contains("$ echo hello")
            })
            .expect("tool row");
        let probe_x = area.x.saturating_add(area.width / 2);
        assert_eq!(buffer[(probe_x, tool_y)].bg, palette.tool_background);
    }

    #[test]
    fn user_prompt_uses_pi_like_muted_filled_background_without_marker() {
        let mut snapshot = projection();
        snapshot.turns = Arc::new(vec![turn_with_trace(
            "hello",
            "answer",
            aw::conversation::Outcome::Completed,
            None,
            Some("answer"),
        )]);
        let mut ui = Ui::new(
            Workspace::default(),
            String::new(),
            None,
            PathBuf::from("/tmp"),
            snapshot,
            aw::config::UiConfig {
                theme: aw::config::ThemeMode::Dark,
                ..aw::config::UiConfig::default()
            },
        )
        .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(100, 28)).unwrap();
        terminal.draw(|frame| ui.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let palette = crate::tui::theme::palette_for(&ui.ui_config);
        let mut saw_prompt = false;
        for y in 0..buffer.area.height {
            let row = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>();
            if row.contains(" hello") {
                assert!(!row.contains("› hello"));
                saw_prompt = true;
                let prompt_x = (0..buffer.area.width)
                    .find(|x| buffer[(*x, y)].symbol() == "h")
                    .expect("prompt text cell");
                assert_eq!(buffer[(prompt_x, y)].bg, palette.user_prompt_background);
                let filled_x = prompt_x.saturating_add(12).min(buffer.area.width - 1);
                assert_eq!(buffer[(filled_x, y)].bg, palette.user_prompt_background);
                break;
            }
        }
        assert!(saw_prompt);
    }
    #[test]
    fn file_preview_open_keeps_explorer_focus() {
        let mut ui = model();
        let explorer = 1;
        ui.apply(Action::SetNavigator {
            pane: explorer,
            kind: Navigator::Files,
        });
        ui.apply(Action::Focus(explorer));
        let group = ui.group();

        ui.open_file_preview("src/lib.rs".into());

        assert_eq!(ui.layout.focused, explorer);
        assert!(matches!(
            ui.layout.active(group).map(|tab| &tab.target),
            Some(Target::File { path, .. }) if path == "src/lib.rs"
        ));
    }

    #[test]
    fn file_list_mode_enters_selected_tree_directory_and_exposes_parent_row() {
        let mut state = NavState::default();
        state.directories.insert(
            String::new(),
            vec![Entry {
                name: "src".into(),
                path: "src".into(),
                kind: EntryKind::Directory,
            }],
        );
        state.selected = 0;
        assert_eq!(Ui::list_directory_on_switch(&state), "src");

        state.display = NavDisplay::List;
        state.directory = "src/nested".into();
        state.entries = vec![
            Entry {
                name: "child".into(),
                path: "src/nested/child".into(),
                kind: EntryKind::Directory,
            },
            Entry {
                name: "main.rs".into(),
                path: "src/nested/main.rs".into(),
                kind: EntryKind::File,
            },
        ];
        let rows = Ui::file_rows(&state);
        assert!(matches!(
            rows.first().map(|row| &row.item),
            Some(NavigatorItem::Parent)
        ));
        assert_eq!(rows[0].label, "../");
        assert_eq!(rows[1].label, "> child/");
        assert_eq!(rows[2].label, "  main.rs");
    }

    #[test]
    fn git_view_toggle_is_present_only_for_changes() {
        let mut ui = model();
        let explorer = 1;
        ui.apply(Action::SetNavigator {
            pane: explorer,
            kind: Navigator::Git,
        });
        ui.apply(Action::Focus(explorer));
        ui.git.panes.entry(explorer).or_default().mode = git::Mode::Changes;
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|f| ui.draw(f)).unwrap();
        assert!(ui
            .hit_regions
            .iter()
            .any(|region| matches!(region.hit, Hit::GitDisplay { pane, .. } if pane == explorer)));

        ui.git.panes.entry(explorer).or_default().mode = git::Mode::Worktrees;
        terminal.draw(|f| ui.draw(f)).unwrap();
        assert!(!ui
            .hit_regions
            .iter()
            .any(|region| matches!(region.hit, Hit::GitDisplay { pane, .. } if pane == explorer)));

        ui.git.panes.entry(explorer).or_default().mode = git::Mode::Branches;
        terminal.draw(|f| ui.draw(f)).unwrap();
        assert!(!ui
            .hit_regions
            .iter()
            .any(|region| matches!(region.hit, Hit::GitDisplay { pane, .. } if pane == explorer)));
    }

    #[test]
    fn workspace_chrome_is_minimal_and_agent_tab_uses_embedded_shared_header() {
        let mut ui = model();
        ui.snapshot.loaded_resources =
            crate::tui::agent_resources::LoadedResourceSummary::with_system_prompt(
                crate::tui::agent_resources::SystemPromptOrigin::Config,
                128,
            );
        ui.snapshot
            .loaded_resources
            .add_instruction_file("./AGENTS.md", 42);
        ui.set_navigator(Navigator::Files);
        ui.open(Target::Activity);
        ui.open(Target::Settings);
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|f| ui.draw(f)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(!text.contains(" Explorer "));
        for numbered in [
            "[1] Sessions",
            "[2] Files",
            "[3] Git",
            "[1] Agent",
            "[2] Activity",
            "[3] Settings",
        ] {
            assert!(
                !text.contains(numbered),
                "visible navigator/tab numbering must be removed: {numbered}"
            );
        }

        ui.open(Target::Conversation);
        terminal.draw(|f| ui.draw(f)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("[Context]"));
        assert!(text.contains("system prompt · config · 128 B"));
        assert!(text.contains("instructions · ./AGENTS.md · 42 B"));
        // Embedded Agent chrome hides empty optional sections, but must keep
        // admitted instructions and built-in/loaded Context provenance.
        assert!(!text.contains("[Skills]"));
        assert!(!text.contains("none loaded"));
        assert!(!text.contains("aw v0.15.0"));
        assert!(!text.contains("/ commands"));
    }

    #[test]
    fn conversation_render_key_changes_with_loaded_resource_provenance() {
        let mut ui = model();
        let before = ui.conversation_render_key(100);
        ui.snapshot.loaded_resources =
            crate::tui::agent_resources::LoadedResourceSummary::with_system_prompt(
                crate::tui::agent_resources::SystemPromptOrigin::BuiltIn,
                64,
            );
        ui.snapshot
            .loaded_resources
            .add_instruction_file("./runtime-instructions.md", 17);
        let after = ui.conversation_render_key(100);
        assert!(!before.matches(&after));
    }

    #[test]
    fn main_tab_hit_regions_are_the_exact_rectangles_that_paint_labels() {
        let mut ui = model();
        ui.open(Target::Activity);
        ui.open(Target::Help);
        let group = ui.group();
        let help = ui
            .layout
            .panes
            .get(&group)
            .and_then(|pane| match pane {
                Pane::Group { tabs, .. } => tabs.iter().find_map(|id| {
                    ui.layout
                        .tabs
                        .get(id)
                        .is_some_and(|tab| matches!(tab.target, Target::Help))
                        .then_some(*id)
                }),
                _ => None,
            })
            .expect("Help tab");

        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|frame| ui.draw(frame)).unwrap();
        let hit = ui
            .hit_regions
            .iter()
            .find_map(|region| {
                matches!(
                    region.hit,
                    Hit::GroupTab {
                        group: candidate_group,
                        tab: candidate_tab,
                    } if candidate_group == group && candidate_tab == help
                )
                .then_some(region.area)
            })
            .expect("Help tab hit region");
        let buffer = terminal.backend().buffer();
        let label_cells = (0..buffer.area.width)
            .find(|x| {
                let row = (0..4)
                    .filter_map(|offset| {
                        x.checked_add(offset)
                            .filter(|column| *column < buffer.area.width)
                            .map(|column| buffer[(column, hit.y)].symbol())
                    })
                    .collect::<String>();
                row == "Help"
            })
            .expect("painted Help label");
        assert!(rect_contains(hit, (label_cells, hit.y)));
        assert!(rect_contains(hit, (label_cells + 3, hit.y)));
    }

    #[test]
    fn escape_closes_the_active_help_view() {
        let mut ui = model();
        ui.open(Target::Help);
        let group = ui.group();
        let help = ui.layout.active(group).expect("Help active").id;
        assert!(matches!(
            ui.layout.active(group).map(|tab| &tab.target),
            Some(Target::Help)
        ));
        assert!(ui.close_help_on_escape());
        assert!(!ui.layout.tabs.contains_key(&help));
        assert!(!matches!(
            ui.layout.active(group).map(|tab| &tab.target),
            Some(Target::Help)
        ));
    }

    #[test]
    fn session_list_uses_two_line_rows_with_multi_select_and_status_markers() {
        let mut ui = model();
        let explorer = ui.explorer_pane().expect("Explorer");
        ui.apply(Action::SetNavigator {
            pane: explorer,
            kind: Navigator::Sessions,
        });
        let state = ui.nav.entry((explorer, Navigator::Sessions)).or_default();
        state.display = NavDisplay::List;
        ui.snapshot.session = Some("running-session".into());
        ui.snapshot.branch = "main".into();
        ui.snapshot.branch_id = Some("main".into());
        ui.snapshot.busy = true;
        ui.snapshot.turn_started = Some(Instant::now() - Duration::from_secs(2));
        ui.snapshot.sessions = vec![
            crate::workspace::owner::SavedSession {
                id: "completed-session".into(),
                title: "Completed agent".into(),
                directory: "/tmp/project".into(),
                autonomous: false,
                archived: false,
                updated_at: 0,
                turn_count: 17,
                branches: vec![crate::workspace::owner::SavedBranch {
                    id: "main".into(),
                    name: "main".into(),
                    parent: None,
                }],
            },
            crate::workspace::owner::SavedSession {
                id: "running-session".into(),
                title: "Running agent".into(),
                directory: "/tmp/project".into(),
                autonomous: false,
                archived: false,
                updated_at: 0,
                turn_count: 4,
                branches: vec![crate::workspace::owner::SavedBranch {
                    id: "main".into(),
                    name: "main".into(),
                    parent: None,
                }],
            },
        ];
        ui.snapshot.subagents = vec![crate::workspace::owner::SavedSubagent {
            parent_session: "other".into(),
            child_session: "completed-session".into(),
            state: "completed".into(),
            mode: "background".into(),
            tool_mode: "off".into(),
            depth: 1,
        }];

        let mut terminal = Terminal::new(TestBackend::new(180, 36)).unwrap();
        terminal.draw(|frame| ui.draw(frame)).unwrap();
        let mut rows = ui
            .hit_regions
            .iter()
            .filter_map(|region| {
                matches!(
                    region.hit,
                    Hit::NavigatorRow { pane, .. } if pane == explorer
                )
                .then_some(region.area)
            })
            .collect::<Vec<_>>();
        rows.sort_by_key(|area| area.y);
        assert!(rows.len() >= 2);
        assert!(rows.iter().take(2).all(|area| area.height == 2));
        assert!(ui.hit_regions.iter().any(|region| {
            matches!(region.hit, Hit::SessionMultiSelect { pane } if pane == explorer)
        }));

        let buffer = terminal.backend().buffer();
        let first = (rows[0].y..rows[0].bottom())
            .map(|y| {
                (rows[0].x..rows[0].right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(first[0].contains("Completed agent"));
        assert!(first[0].contains('●'));
        assert!(first[1].contains("17 turns"));
        assert!(first[1].contains("main"));

        let second = (rows[1].x..rows[1].right())
            .map(|x| buffer[(x, rows[1].y)].symbol())
            .collect::<String>();
        assert!(second.contains("Running agent"));
        assert!(second.contains('▪') || second.contains('▫'));
    }

    #[test]
    fn tabs_use_dirty_dot_and_double_click_close_without_close_glyph() {
        let mut ui = model();
        ui.draft.set("unsent").unwrap();
        let group = ui.group();
        let tab = ui.layout.active(group).unwrap().id;
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|f| ui.draw(f)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains(TAB_DIRTY_ICON));
        assert!(!text.contains('×'));
        let at = Instant::now();
        ui.select_or_close_tab(group, tab, at);
        assert!(ui.layout.tabs.contains_key(&tab));
        ui.select_or_close_tab(group, tab, at + Duration::from_millis(100));
        assert!(!ui.layout.tabs.contains_key(&tab));
    }

    #[test]
    fn restored_non_conversation_focus_starts_in_navigation_mode() {
        let mut layout = Workspace::default();
        layout.apply(Action::Focus(1)).unwrap();
        let ui = Ui::new(
            layout,
            "draft remains local".into(),
            None,
            PathBuf::from("/tmp"),
            projection(),
            aw::config::UiConfig::default(),
        )
        .unwrap();
        assert!(!ui.input);
        assert_eq!(ui.draft.text(), "draft remains local");
    }

    #[test]
    fn minimal_returns_to_agent_without_mutating_configured_tui_mode() {
        let mut ui = model();
        ui.ui_config.tui_mode = aw::config::TuiMode::Fullscreen;
        assert!(ui.return_to_agent());
        assert_eq!(ui.ui_config.tui_mode, aw::config::TuiMode::Fullscreen);
        assert_eq!(ui.exit, Some(Exit::Agent));

        let mut blocked = model();
        blocked.ui_config.tui_mode = aw::config::TuiMode::Inline;
        blocked.recovery = "rejected input".into();
        assert!(!blocked.return_to_agent());
        assert_eq!(blocked.ui_config.tui_mode, aw::config::TuiMode::Inline);
        assert_eq!(blocked.exit, None);
    }

    #[test]
    fn fullscreen_slash_menu_uses_semantic_accent_selection() {
        let mut ui = model();
        ui.draft.set("/").unwrap();
        ui.input = true;
        ui.ui_config.theme = aw::config::ThemeMode::Dark;
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|frame| ui.draw(frame)).unwrap();

        let selected = ui
            .hit_regions
            .iter()
            .find_map(|region| {
                matches!(region.hit, Hit::SlashRow { row: 0, .. }).then_some(region.area)
            })
            .expect("slash popup row");
        let palette = crate::tui::theme::palette_for(&ui.ui_config);
        let buffer = terminal.backend().buffer();
        let cells = (selected.x..selected.x.saturating_add(selected.width))
            .map(|x| &buffer[(x, selected.y)])
            .collect::<Vec<_>>();
        assert!(cells.iter().any(|cell| cell.bg == palette.accent));
        assert!(cells
            .iter()
            .filter(|cell| cell.bg == palette.accent)
            .all(|cell| cell.fg == palette.accent_foreground));
    }

    #[test]
    fn normal_frame_renders_without_services() {
        let mut ui = model();
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|f| ui.draw(f)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(text.contains("Agent"));
        assert!(text.contains("draft remains local"));
        assert!(ui.can_submit);
    }
    #[test]
    fn empty_agent_uses_single_line_placeholder_without_transcript_hint() {
        let mut ui = Ui::new(
            Workspace::default(),
            String::new(),
            None,
            PathBuf::from("/tmp"),
            projection(),
            aw::config::UiConfig::default(),
        )
        .unwrap();
        let group = ui.group();
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|f| ui.draw(f)).unwrap();
        let composer = ui
            .hit_regions
            .iter()
            .find_map(|region| {
                matches!(region.hit, Hit::Composer(candidate) if candidate == group)
                    .then_some(region.area)
            })
            .unwrap();
        assert_eq!(composer.height, 2);
        let buffer = terminal.backend().buffer();
        let row = (composer.x..composer.x.saturating_add(composer.width))
            .map(|x| buffer[(x, composer.y + 1)].symbol())
            .collect::<String>();
        assert!(row.contains("Ask anything…"));
        assert!(row.contains("/ for commands"));
        let first_placeholder = (composer.x..composer.x.saturating_add(composer.width))
            .find(|x| buffer[(*x, composer.y + 1)].symbol() == "A")
            .unwrap();
        let placeholder_cell = &buffer[(first_placeholder, composer.y + 1)];
        let palette = crate::tui::theme::palette_for(&ui.ui_config);
        assert_eq!(placeholder_cell.fg, palette.muted);
        assert!(!placeholder_cell.modifier.contains(Modifier::DIM));
        let all = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(!all.contains("Ask anything. Type / for commands."));

        let mut narrow = Ui::new(
            Workspace::default(),
            String::new(),
            None,
            PathBuf::from("/tmp"),
            projection(),
            aw::config::UiConfig::default(),
        )
        .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(54, 24)).unwrap();
        terminal.draw(|f| narrow.draw(f)).unwrap();
        let composer = narrow
            .hit_regions
            .iter()
            .find_map(|region| matches!(region.hit, Hit::Composer(_)).then_some(region.area))
            .unwrap();
        assert_eq!(composer.height, 2);
        let buffer = terminal.backend().buffer();
        let row = (composer.x..composer.x.saturating_add(composer.width))
            .map(|x| buffer[(x, composer.y + 1)].symbol())
            .collect::<String>();
        assert!(row.contains("Ask anything…"));
    }

    #[test]
    fn agent_tab_uses_current_session_title_without_changing_layout_target() {
        let mut snapshot = projection();
        snapshot.session = Some("s_fixture".into());
        snapshot.session_title = Some("Bird research".into());
        let mut ui = Ui::new(
            Workspace::default(),
            String::new(),
            None,
            PathBuf::from("/tmp"),
            snapshot,
            aw::config::UiConfig::default(),
        )
        .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|f| ui.draw(f)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("Agent: Bird research"));
        assert!(matches!(
            ui.layout.active(ui.group()).map(|tab| &tab.target),
            Some(Target::Conversation)
        ));
    }

    #[test]
    fn narrow_frame_keeps_desired_layout() {
        let mut ui = model();
        let before = ui.layout.clone();
        for (w, h) in [(1, 1), (30, 8), (80, 24), (160, 48)] {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| ui.draw(f)).unwrap();
            assert_eq!(ui.layout, before);
        }
        assert_eq!(ui.draft.text(), "draft remains local");
    }
    #[test]
    fn hidden_composer_cannot_send() {
        let mut ui = model();
        ui.open(Target::Help);
        let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
        t.draw(|f| ui.draw(f)).unwrap();
        assert!(!ui.can_submit);
    }
    #[test]
    fn repaint_reuses_unchanged_file_rows() {
        let mut ui = model();
        ui.files.insert("a.txt".into(), "source".into());
        let target = Target::File {
            scope_id: ScopeId::launch(),
            path: "a.txt".into(),
        };
        let first = ui.rendered_lines(9, &target, 80);
        let second = ui.rendered_lines(9, &target, 80);
        assert!(Arc::ptr_eq(&first, &second));
        ui.file_revision += 1;
        assert!(!Arc::ptr_eq(&first, &ui.rendered_lines(9, &target, 80)));
    }

    #[test]
    fn composer_slash_popup_uses_the_shared_command_registry() {
        let mut ui = model();
        ui.draft.set("/mo").unwrap();
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|f| ui.draw(f)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(text.contains("/model [MODEL]"));
        assert!(!text.contains("Ctrl+]"));
        let buffer = terminal.backend().buffer();
        let row = |y: u16| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };
        let popup_y = (0..buffer.area.height)
            .find(|y| row(*y).contains("/model [MODEL]"))
            .unwrap();
        let composer = ui
            .hit_regions
            .iter()
            .find_map(|region| matches!(region.hit, Hit::Composer(_)).then_some(region.area))
            .unwrap();
        let composer_y = composer.y.saturating_add(1);
        assert!(popup_y < composer_y);
        assert!(
            composer_y.saturating_sub(popup_y)
                <= u16::from(ui.ui_config.autocomplete_max_visible).saturating_add(2),
            "slash suggestions must stay attached immediately above the composer"
        );
    }

    #[test]
    fn terminal_toggle_returns_to_non_terminal_workspace_surface() {
        let mut ui = model();
        let group = ui.group();
        let conversation = ui.layout.active(group).unwrap().id;
        ui.apply(Action::Open {
            group,
            target: Target::Terminal {
                terminal: "pty_fixture".into(),
            },
            pinned: true,
        });
        assert!(matches!(
            ui.layout.active(group).map(|tab| &tab.target),
            Some(Target::Terminal { .. })
        ));

        // No remembered return point yet: fallback must still leave terminal.
        ui.toggle_terminal();
        assert!(!matches!(
            ui.layout.active(ui.layout.focused).map(|tab| &tab.target),
            Some(Target::Terminal { .. })
        ));

        ui.apply(Action::Select {
            group,
            tab: conversation,
        });
        ui.toggle_terminal();
        assert!(matches!(
            ui.layout.active(ui.layout.focused).map(|tab| &tab.target),
            Some(Target::Terminal { .. })
        ));
        ui.toggle_terminal();
        assert_eq!(
            ui.layout.active(group).map(|tab| tab.id),
            Some(conversation)
        );
    }

    #[test]
    fn fullscreen_visibility_collapse_keeps_labels_and_hides_live_tool_output() {
        let mut ui = model();
        ui.ui_config.show_thinking = false;
        ui.ui_config.show_tool_calls = false;
        ui.snapshot.pending_prompt = "work".into();
        ui.snapshot.thinking = true;
        ui.snapshot.live_tools = vec![aw_agent_core::ToolRecord {
            call: aw_agent_core::ToolCall {
                id: "call-1".into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command":"echo secret-detail"}),
            },
            effect: aw_agent_core::ToolEffect::Process,
            execution_site: "local".into(),
            status: aw_agent_core::ToolStatus::Succeeded,
            output: Some(aw_agent_core::ToolOutput::text("secret-detail")),
        }];
        let text = ui.conversation_lines(100).join(
            "
",
        );
        assert!(
            !text.contains("Thinking"),
            "live thinking/progress belongs in the composer border, not a transcript row"
        );
        assert!(text.contains("tool bash"));
        assert!(text.contains("hidden"));
        assert!(!text.contains("secret-detail"));
    }

    #[test]
    fn navigator_display_controls_are_icon_only() {
        assert_eq!(Ui::nav_display_icon(NavDisplay::List), LIST_VIEW_ICON);
        assert_eq!(Ui::nav_display_icon(NavDisplay::Tree), TREE_VIEW_ICON);
        assert_eq!(Ui::git_display_icon(git::Display::List), LIST_VIEW_ICON);
        assert_eq!(Ui::git_display_icon(git::Display::Tree), TREE_VIEW_ICON);
        assert_ne!(LIST_VIEW_ICON, "List");
        assert_ne!(TREE_VIEW_ICON, "Tree");
    }

    #[test]
    fn large_file_paint_projects_only_requested_visible_slice() {
        let mut ui = model();
        let path = "src/huge.rs".to_owned();
        let lines = (0..8192)
            .map(|index| format!("pub fn bird_{index}() {{}}"))
            .collect::<Vec<_>>();
        ui.file_lines
            .insert(path.clone().into(), Arc::new(lines.clone()));
        ui.files.insert(path.clone().into(), lines.join("\n"));
        let slice = ui.file_view_slice(&path.clone().into(), 7000, 31);
        assert_eq!(slice.len(), 31);
        assert!(line_plain_text(&slice[0]).contains("bird_6998"));
        assert!(line_plain_text(slice.last().unwrap()).contains("bird_7028"));
        assert_eq!(ui.file_line_count(&path.clone().into()), 8194);
    }

    #[test]
    fn palette_file_selection_focuses_the_actual_view_from_explorer_or_editor() {
        for from_explorer in [true, false] {
            let mut ui = model();
            let group = ui.group();
            let explorer = ui.explorer_pane().unwrap();
            ui.apply(Action::Focus(if from_explorer { explorer } else { group }));
            let editor = ui.draft.clone();
            let turns = ui.snapshot.turns.clone();
            ui.open_file_preview("chosen.txt".into());
            if from_explorer {
                assert_eq!(
                    ui.layout.focused, explorer,
                    "ordinary Explorer preview preserves focus"
                );
            }
            ui.focus_palette_file(group, &"unopened.txt".into());
            assert_eq!(
                ui.layout.focused,
                if from_explorer { explorer } else { group }
            );
            ui.focus_palette_file(group, &"chosen.txt".into());
            assert_eq!(ui.layout.focused, group);
            assert!(!ui.input);
            assert!(
                matches!(ui.layout.active(group).map(|tab|&tab.target),Some(Target::File {path,..}) if path=="chosen.txt")
            );
            assert_eq!(ui.draft, editor);
            assert!(Arc::ptr_eq(&ui.snapshot.turns, &turns));
            assert!(ui.jobs.is_empty());
        }
    }

    #[test]
    fn palette_keeps_exact_session_and_window_across_delayed_file_insertions() {
        let mut ui = model();
        ui.snapshot.sessions = vec![crate::workspace::owner::SavedSession {
            id: "chosen-session".into(),
            title: "Chosen".into(),
            directory: "/tmp".into(),
            autonomous: false,
            archived: false,
            updated_at: 0,
            turn_count: 0,
            branches: vec![crate::workspace::owner::SavedBranch {
                id: "work".into(),
                name: "work".into(),
                parent: None,
            }],
        }];
        ui.open_palette();
        let before = ui.palette_choices();
        let original_index=before.iter().position(|item|matches!(&item.target,PaletteTarget::Session {session,branch} if session=="chosen-session" && branch=="work")).unwrap();
        let key = before[original_index].key();
        ui.palette.as_mut().unwrap().select(original_index);
        ui.palette_choices();
        ui.palette_file_results.push(Entry {
            name: "delayed.txt".into(),
            path: "delayed.txt".into(),
            kind: EntryKind::File,
        });
        let after = ui.palette_choices();
        let selected = ui.palette.as_ref().unwrap().selected(after.len()).unwrap();
        assert!(selected > original_index);
        assert_eq!(after[selected].key(), key);
        assert_eq!(
            ui.palette
                .as_mut()
                .unwrap()
                .key(
                    KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                    after.len()
                )
                .unwrap(),
            palette::Intent::Activate(selected)
        );
        ui.snapshot.sessions.clear();
        let gone = ui.palette_choices();
        assert_eq!(ui.palette.as_ref().unwrap().selected(gone.len()), None);
        assert_eq!(
            ui.palette
                .as_mut()
                .unwrap()
                .key(
                    KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                    gone.len()
                )
                .unwrap(),
            palette::Intent::Handled
        );

        ui.open(Target::Help);
        ui.palette.as_mut().unwrap().set("shortcuts").unwrap();
        let before = ui.palette_choices();
        let original_index = before
            .iter()
            .position(|item| matches!(&item.target, PaletteTarget::Window { .. }))
            .unwrap();
        let key = before[original_index].key();
        ui.palette.as_mut().unwrap().select(original_index);
        ui.palette_choices();
        ui.palette_file_results.push(Entry {
            name: "shortcuts".into(),
            path: "shortcuts".into(),
            kind: EntryKind::File,
        });
        let after = ui.palette_choices();
        let selected = ui.palette.as_ref().unwrap().selected(after.len()).unwrap();
        assert!(selected > original_index);
        assert_eq!(after[selected].key(), key);
        assert!(ui.jobs.is_empty());
    }

    #[test]
    fn palette_disarms_when_current_agent_title_stops_matching() {
        let mut ui = model();
        ui.snapshot.session = Some("current".into());
        ui.snapshot.sessions = vec![crate::workspace::owner::SavedSession {
            id: "current".into(),
            title: "Bird work".into(),
            directory: "/tmp".into(),
            autonomous: false,
            archived: false,
            updated_at: 0,
            turn_count: 0,
            branches: vec![crate::workspace::owner::SavedBranch {
                id: "work".into(),
                name: "work".into(),
                parent: None,
            }],
        }];
        ui.snapshot.branch = "work".into();
        ui.snapshot.branch_id = Some("work".into());
        ui.open_palette();
        ui.palette.as_mut().unwrap().set("&Bird").unwrap();
        let before = ui.palette_choices();
        assert_eq!(before.len(), 1);
        assert_eq!(ui.palette.as_ref().unwrap().selected(1), Some(0));
        ui.snapshot.sessions[0].title = "Renamed".into();
        let after = ui.palette_choices();
        assert!(after.is_empty());
        ui.snapshot.sessions[0].title = "Bird replacement".into();
        let returning = ui.palette_choices();
        assert_eq!(returning.len(), 1);
        assert_eq!(ui.palette.as_ref().unwrap().selected(returning.len()), None);
        assert_eq!(
            ui.palette
                .as_mut()
                .unwrap()
                .key(
                    KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                    returning.len()
                )
                .unwrap(),
            palette::Intent::Handled
        );
        assert_eq!(ui.snapshot.session.as_deref(), Some("current"));
        assert_eq!(ui.snapshot.branch, "work");
        assert!(ui.jobs.is_empty());
    }

    #[test]
    fn unified_palette_cancel_and_resize_preserve_invoking_focus_and_editor() {
        let mut ui = model();
        ui.open(Target::Settings);
        ui.draft.left();
        ui.draft.left();
        let original_editor = ui.draft.clone();
        let original_layout = ui.layout.clone();
        let original_input = ui.input;
        ui.open_palette();
        ui.palette.as_mut().unwrap().set("/theme dark").unwrap();
        ui.open_palette();
        assert_eq!(ui.palette.as_ref().unwrap().text(), "/theme dark");
        for (width, height) in [(1, 1), (30, 8), (80, 24), (160, 48)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| ui.draw(frame)).unwrap();
            assert_eq!(ui.layout, original_layout);
            assert_eq!(ui.draft, original_editor);
        }
        ui.close_palette();
        assert_eq!(ui.layout, original_layout);
        assert_eq!(ui.draft, original_editor);
        assert_eq!(ui.input, original_input);
        assert!(ui.settings.is_some());
        ui.open_palette();
        assert_eq!(ui.palette.as_ref().unwrap().text(), "");
        assert_eq!(ui.palette.as_ref().unwrap().selected(10), None);
    }

    #[test]
    fn busy_palette_admission_allows_local_views_and_exact_current_owner_only() {
        let mut ui = model();
        ui.snapshot.busy = true;
        ui.snapshot.session = Some("current".into());
        ui.snapshot.branch = "work".into();
        ui.snapshot.branch_id = Some("work".into());
        let item = |target| PaletteItem {
            category: PaletteCategory::Actions,
            label: String::new(),
            description: String::new(),
            shortcut: String::new(),
            target,
            score: 0,
        };
        for command in [
            "new",
            "stage",
            "terminal-start",
            "quit",
            "model other",
            "minimal",
            "title-generate",
        ] {
            assert!(
                !ui.palette_item_allowed(&item(PaletteTarget::Command(command.into()))),
                "{command}"
            );
        }
        for command in [
            "settings",
            "conversation",
            "files",
            "theme light",
            "maximize",
            "terminal-live",
            "cancel-agent",
        ] {
            assert!(
                ui.palette_item_allowed(&item(PaletteTarget::Command(command.into()))),
                "{command}"
            );
        }
        assert!(ui.palette_item_allowed(&item(PaletteTarget::Session {
            session: "current".into(),
            branch: "work".into()
        })));
        assert!(!ui.palette_item_allowed(&item(PaletteTarget::Session {
            session: "current".into(),
            branch: "main".into()
        })));
        assert!(!ui.palette_item_allowed(&item(PaletteTarget::Session {
            session: "other".into(),
            branch: "work".into()
        })));
        assert!(ui.jobs.is_empty());
    }

    #[test]
    fn closing_palette_cancels_its_inflight_file_search() {
        let mut ui = model();
        ui.palette = Some(palette::State::default());
        let cancel = Arc::new(AtomicBool::new(false));
        ui.jobs.insert(
            40,
            Job::Search {
                token: ui.file_token(&ScopeId::launch(), None).unwrap(),
                pane: None,
                query: "old".into(),
                cancel: cancel.clone(),
            },
        );
        ui.palette_file_pending = Some(40);
        ui.palette_file_query = "old".into();
        ui.palette_file_results.push(Entry {
            name: "old.rs".into(),
            path: "old.rs".into(),
            kind: EntryKind::File,
        });
        ui.palette.as_mut().unwrap().set("old").unwrap();
        ui.palette
            .as_mut()
            .unwrap()
            .key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE), 0)
            .unwrap();
        let invoking = ui.palette.take().unwrap();
        ui.finish_palette_dispatch(invoking, false);
        assert_eq!(ui.palette.as_ref().unwrap().text(), "old");
        assert!(!cancel.load(Ordering::Acquire));
        assert!(ui.jobs.contains_key(&40));
        assert_eq!(ui.palette_file_pending, Some(40));
        assert_eq!(ui.palette_file_query, "old");
        assert_eq!(ui.palette_file_results[0].path, "old.rs");
        ui.palette
            .as_mut()
            .unwrap()
            .key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE), 0)
            .unwrap();
        assert_eq!(
            ui.palette.as_ref().unwrap().text(),
            "olXd",
            "declined dispatch preserves query caret"
        );
        ui.close_palette();
        assert!(cancel.load(Ordering::Acquire));
        assert!(!ui.jobs.contains_key(&40));
        assert!(ui.palette.is_none());
        assert!(ui.palette_file_pending.is_none());
        assert!(ui.palette_file_query.is_empty());
        assert!(ui.palette_file_results.is_empty());
        ui.open_palette();
        ui.palette.as_mut().unwrap().set("pending").unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        ui.jobs.insert(
            41,
            Job::Search {
                token: ui.file_token(&ScopeId::launch(), None).unwrap(),
                pane: None,
                query: "pending".into(),
                cancel: cancel.clone(),
            },
        );
        ui.palette_file_pending = Some(41);
        ui.palette_file_query = "pending".into();
        let invoking = ui.palette.take().unwrap();
        ui.finish_palette_dispatch(invoking, false);
        ui.response(crate::workspace::files::Response {
            access: ui.file_scopes.access(&ScopeId::launch()),
            ticket: 41,
            result: Ok(Content::Search {
                query: "pending".into(),
                entries: vec![Entry {
                    name: "pending.rs".into(),
                    path: "pending.rs".into(),
                    kind: EntryKind::File,
                }],
            }),
        });
        assert!(!cancel.load(Ordering::Acquire));
        assert_eq!(ui.palette_file_pending, None);
        assert_eq!(
            ui.palette_file_results[0].path, "pending.rs",
            "declined command retains eventual original search result"
        );
        assert_eq!(ui.palette.as_ref().unwrap().text(), "pending");
    }

    #[test]
    fn replacing_file_search_cancels_and_releases_stale_job_budget_immediately() {
        let mut ui = model();
        let first_cancel = Arc::new(AtomicBool::new(false));
        ui.jobs.insert(
            41,
            Job::Search {
                token: ui.file_token(&ScopeId::launch(), None).unwrap(),
                pane: None,
                query: "old".into(),
                cancel: first_cancel.clone(),
            },
        );
        ui.palette_file_pending = Some(41);
        ui.cancel_file_searches(None);
        assert!(first_cancel.load(Ordering::Acquire));
        assert!(!ui.jobs.contains_key(&41));
        assert!(ui.palette_file_pending.is_none());

        let pane = ui.layout.focused;
        let second_cancel = Arc::new(AtomicBool::new(false));
        ui.jobs.insert(
            42,
            Job::Search {
                token: ui.file_token(&ScopeId::launch(), None).unwrap(),
                pane: Some(pane),
                query: "older".into(),
                cancel: second_cancel.clone(),
            },
        );
        ui.nav
            .entry((pane, Navigator::Files))
            .or_default()
            .search_pending = Some(42);
        ui.cancel_file_searches(Some(pane));
        assert!(second_cancel.load(Ordering::Acquire));
        assert!(!ui.jobs.contains_key(&42));
        assert!(ui.nav[&(pane, Navigator::Files)].search_pending.is_none());
    }

    #[test]
    fn large_file_find_runs_in_background_and_latest_query_wins() {
        let mut ui = model();
        let group = ui.group();
        let path = "src/find.rs".to_owned();
        ui.open(Target::File {
            scope_id: ScopeId::launch(),
            path: path.clone(),
        });
        let tab = ui.layout.active(group).unwrap().id;
        let lines = (0..8192)
            .map(|index| format!("fn bird_{index}() {{}}"))
            .collect::<Vec<_>>();
        ui.file_lines
            .insert(path.clone().into(), Arc::new(lines.clone()));
        ui.files.insert(path.into(), lines.join("\n"));

        let started = Instant::now();
        ui.update_find(tab, "bird_7000".into(), false);
        ui.update_find(tab, "bird_7999".into(), false);
        assert!(started.elapsed() < Duration::from_millis(50));

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            while let Ok(result) = ui.background_rx.try_recv() {
                ui.apply_background_result(result);
            }
            if ui
                .find
                .get(&tab)
                .is_some_and(|state| state.query == "bird_7999" && state.matches == vec![8001])
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let state = ui.find.get(&tab).unwrap();
        assert_eq!(state.query, "bird_7999");
        assert_eq!(state.matches, vec![8001]);
        let offset = ui.scroll.get(&tab).map(Viewport::offset).unwrap();
        assert!(offset < 8001);
        assert!((2..=4).contains(&(8001 - offset)));
    }

    #[test]
    fn progressive_result_publication_never_blocks_when_channel_is_full() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        tx.try_send(UiBackgroundResult::Terminal(Ok("occupied".into())))
            .unwrap();
        let started = Instant::now();
        assert!(try_send_ui_progress(
            &tx,
            UiBackgroundResult::Terminal(Ok("progress".into())),
        ));
        assert!(started.elapsed() < Duration::from_millis(50));
        assert!(matches!(
            rx.blocking_recv(),
            Some(UiBackgroundResult::Terminal(_))
        ));
        drop(rx);
        assert!(!try_send_ui_progress(
            &tx,
            UiBackgroundResult::Terminal(Ok("closed".into())),
        ));
    }

    #[test]
    fn local_scroll_remains_immediate_while_background_worker_is_blocked() {
        let mut ui = model();
        let group = ui.group();
        ui.open(Target::File {
            scope_id: ScopeId::launch(),
            path: "src/slow.rs".into(),
        });
        let tab = ui.layout.active(group).unwrap().id;
        ui.line_counts.insert(tab, 500);
        ui.selectable_views.insert(
            tab,
            SelectableView {
                area: Rect::new(0, 0, 80, 30),
                start_line: 0,
                horizontal: 0,
                lines: Vec::new(),
            },
        );
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel::<()>(0);
        ui.background
            .try_submit("test", move || {
                let _ = release_rx.recv();
            })
            .unwrap();
        let started = Instant::now();
        for _ in 0..20 {
            ui.scroll_by(WHEEL_ROWS as isize);
        }
        assert!(started.elapsed() < Duration::from_millis(50));
        assert_eq!(ui.scroll.get(&tab).map(Viewport::offset), Some(120));
        drop(release_tx);
    }

    #[test]
    fn initial_layout_load_has_one_real_pending_lifecycle() {
        let mut ui = model();
        ui.layout_file = Some(std::env::temp_dir().join("aw-review-missing-layout.json"));
        assert!(!ui.layout_pending);
        let submitted_before = {
            let stats = ui.background.stats();
            stats.queued + stats.running + stats.completed as usize
        };
        let base_generation = ui.layout_generation;
        ui.schedule_initial_layout_load();
        assert!(ui.layout_pending);
        let submitted_once = {
            let stats = ui.background.stats();
            stats.queued + stats.running + stats.completed as usize
        };
        assert_eq!(submitted_once, submitted_before + 1);

        ui.schedule_initial_layout_load();
        let submitted_twice = {
            let stats = ui.background.stats();
            stats.queued + stats.running + stats.completed as usize
        };
        assert_eq!(
            submitted_twice, submitted_once,
            "pending guard must prevent duplicate startup layout jobs"
        );

        let group = ui.group();
        ui.apply(Action::SplitGroup {
            pane: group,
            side: Side::Right,
        });
        assert!(
            ui.layout_generation > base_generation,
            "local layout intent must remain responsive while startup load is pending"
        );
        let groups_after_local_change = ui.layout.groups().len();
        ui.apply_background_result(UiBackgroundResult::InitialLayout {
            base_generation,
            result: Ok(Some(Workspace::default())),
        });
        assert!(!ui.layout_pending);
        assert_eq!(ui.layout.groups().len(), groups_after_local_change);
    }

    #[test]
    fn late_initial_layout_result_cannot_overwrite_newer_local_layout_intent() {
        let mut ui = model();
        let original_generation = ui.layout_generation;
        let group = ui.group();
        ui.apply(Action::SplitGroup {
            pane: group,
            side: Side::Right,
        });
        assert!(ui.layout_generation > original_generation);
        let groups_after_local_change = ui.layout.groups().len();
        ui.apply_background_result(UiBackgroundResult::InitialLayout {
            base_generation: original_generation,
            result: Ok(Some(Workspace::default())),
        });
        assert_eq!(ui.layout.groups().len(), groups_after_local_change);
    }

    #[test]
    fn late_manual_layout_load_cannot_overwrite_newer_local_layout_intent() {
        let mut ui = model();
        let base_generation = ui.layout_generation;
        let mut loaded = Workspace::default();
        let loaded_group = loaded.groups()[0];
        loaded
            .apply(Action::SplitGroup {
                pane: loaded_group,
                side: Side::Right,
            })
            .unwrap();

        ui.layout_pending = true;
        let group = ui.group();
        ui.apply(Action::SplitGroup {
            pane: group,
            side: Side::Left,
        });
        let current_groups = ui.layout.groups().len();
        assert!(
            ui.layout_generation > base_generation,
            "local layout intent must remain responsive while manual load is pending"
        );

        ui.apply_background_result(UiBackgroundResult::Layout {
            base_generation,
            result: Ok(Some(loaded)),
        });
        assert!(!ui.layout_pending);
        assert_eq!(ui.layout.groups().len(), current_groups);
        assert!(ui.notice.contains("ignored because the workspace changed"));
    }

    #[test]
    fn theme_rehighlight_is_deferred_and_replaces_cache_with_plain_fallback_first() {
        let mut ui = model();
        let path = "src/lib.rs".to_owned();
        ui.files
            .insert(path.clone().into(), "pub fn bird() {}".into());
        ui.file_lines.insert(
            path.clone().into(),
            Arc::new(vec!["pub fn bird() {}".into()]),
        );
        ui.ui_config.theme = aw::config::ThemeMode::Dark;
        ui.schedule_file_highlight(&path.clone().into());
        let first_generation = ui.file_highlights[&path.clone().into()].generation;
        ui.ui_config.theme = aw::config::ThemeMode::Light;
        ui.refresh_file_highlights_for_theme();
        let cache = &ui.file_highlights[&path.clone().into()];
        assert!(cache.generation > first_generation);
        assert_eq!(cache.theme, aw::config::ThemeMode::Light);
        assert!(cache.lines.iter().all(Option::is_none));
        let plain = ui.file_view_slice(&path.clone().into(), 2, 1);
        assert_eq!(line_plain_text(&plain[0]), "pub fn bird() {}");
    }

    #[test]
    fn agent_transcript_background_projection_is_single_flight_across_key_changes() {
        let mut ui = model();
        let tab = ui.layout.active(ui.layout.focused).unwrap().id;
        ui.selectable_views.insert(
            tab,
            SelectableView {
                area: Rect::new(0, 0, 90, 20),
                start_line: 0,
                horizontal: 0,
                lines: Vec::new(),
            },
        );
        ui.schedule_visible_background_work();
        let first_generation = ui.conversation_pending.as_ref().unwrap().generation;

        ui.selectable_views.get_mut(&tab).unwrap().area.width = 120;
        ui.schedule_visible_background_work();
        assert_eq!(
            ui.conversation_pending.as_ref().unwrap().generation,
            first_generation,
            "width/theme churn must not enqueue a second rich transcript while one is in flight"
        );
    }

    #[test]
    fn agent_transcript_uses_plain_paint_then_background_syntax_without_geometry_change() {
        let markdown = "```rust\npub fn bird() -> usize { 1 }\n```";
        let mut snapshot = projection();
        snapshot.turns = Arc::new(vec![turn_with_trace(
            "show code",
            markdown,
            aw::conversation::Outcome::Completed,
            None,
            Some(markdown),
        )]);
        let mut ui = Ui::new(
            Workspace::default(),
            String::new(),
            None,
            PathBuf::from("/tmp"),
            snapshot,
            aw::config::UiConfig {
                theme: aw::config::ThemeMode::Dark,
                ..aw::config::UiConfig::default()
            },
        )
        .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| ui.draw(frame)).unwrap();
        let tab = ui.layout.active(ui.layout.focused).unwrap().id;
        let width = ui.selectable_views[&tab].area.width;
        let plain = ui.conversation_view_lines(width);
        let plain_text = crate::tui::markdown::plain_lines(&plain);
        let plain_spans = plain.iter().map(|line| line.spans.len()).sum::<usize>();

        ui.schedule_visible_background_work();
        let result = ui
            .background_rx
            .blocking_recv()
            .expect("conversation background projection");
        assert!(matches!(result, UiBackgroundResult::Conversation { .. }));
        ui.apply_background_result(result);
        let rich = ui.conversation_view_lines(width);
        assert_eq!(plain_text, crate::tui::markdown::plain_lines(&rich));
        assert!(rich.iter().map(|line| line.spans.len()).sum::<usize>() > plain_spans);
        assert!(ui.conversation_cache.is_some());
    }

    #[test]
    fn file_view_renders_plain_text_before_progressive_highlight_then_upgrades_rows() {
        let mut ui = model();
        let path = "src/lib.rs".to_owned();
        let text = "pub fn bird() -> usize { 1 }".to_owned();
        ui.files.insert(path.clone().into(), text.clone());
        ui.file_lines
            .insert(path.clone().into(), Arc::new(vec![text.clone()]));
        let generation = 7;
        ui.file_highlights.insert(
            path.clone().into(),
            HighlightCache {
                generation,
                theme: ui.ui_config.theme,
                lines: vec![None],
                complete: false,
                cancel: Arc::new(AtomicBool::new(false)),
            },
        );
        let plain = ui.file_view_lines(&path.clone().into());
        assert_eq!(plain[2].spans.len(), 1);
        assert_eq!(line_plain_text(&plain[2]), text);

        let highlighted = Line::from(vec![
            Span::styled("pub ", Style::default().fg(Color::LightMagenta)),
            Span::styled(
                "fn bird() -> usize { 1 }",
                Style::default().fg(Color::LightCyan),
            ),
        ]);
        ui.apply_background_result(UiBackgroundResult::Highlight {
            target: HighlightTarget::File(path.clone().into()),
            generation,
            start: 0,
            lines: vec![highlighted],
            done: false,
        });
        let styled = ui.file_view_lines(&path.clone().into());
        assert!(styled[2].spans.len() >= 2);
        assert_eq!(line_plain_text(&styled[2]), text);
    }

    #[test]
    fn stale_or_wrong_theme_highlight_chunks_cannot_replace_current_cache() {
        let mut ui = model();
        let path = "src/lib.rs".to_owned();
        ui.files.insert(path.clone().into(), "fn bird() {}".into());
        ui.file_highlights.insert(
            path.clone().into(),
            HighlightCache {
                generation: 22,
                theme: ui.ui_config.theme,
                lines: vec![None],
                complete: false,
                cancel: Arc::new(AtomicBool::new(false)),
            },
        );
        ui.apply_background_result(UiBackgroundResult::Highlight {
            target: HighlightTarget::File(path.clone().into()),
            generation: 21,
            start: 0,
            lines: vec![Line::styled("stale", Style::default().fg(Color::Red))],
            done: false,
        });
        assert!(ui.file_highlights[&path.clone().into()].lines[0].is_none());

        let old_theme = ui.ui_config.theme;
        ui.ui_config.theme = match old_theme {
            aw::config::ThemeMode::Light => aw::config::ThemeMode::Dark,
            _ => aw::config::ThemeMode::Light,
        };
        ui.apply_background_result(UiBackgroundResult::Highlight {
            target: HighlightTarget::File(path.clone().into()),
            generation: 22,
            start: 0,
            lines: vec![Line::styled("wrong-theme", Style::default().fg(Color::Red))],
            done: false,
        });
        assert!(ui.file_highlights[&path.clone().into()].lines[0].is_none());
    }

    #[test]
    fn replacing_highlight_generation_cancels_previous_worker_token() {
        let mut ui = model();
        let path = "src/lib.rs".to_owned();
        ui.files.insert(path.clone().into(), "fn bird() {}".into());
        let old_cancel = Arc::new(AtomicBool::new(false));
        ui.file_highlights.insert(
            path.clone().into(),
            HighlightCache {
                generation: 1,
                theme: ui.ui_config.theme,
                lines: vec![None],
                complete: false,
                cancel: old_cancel.clone(),
            },
        );
        ui.file_highlights.insert(
            path.into(),
            HighlightCache {
                generation: 2,
                theme: ui.ui_config.theme,
                lines: vec![None],
                complete: false,
                cancel: Arc::new(AtomicBool::new(false)),
            },
        );
        assert!(old_cancel.load(Ordering::Acquire));
    }

    #[test]
    fn mouse_wheel_scroll_is_fast_local_and_does_not_enqueue_background_work() {
        let mut ui = model();
        let group = ui.group();
        let tab = ui.layout.active(group).unwrap().id;
        ui.line_counts.insert(tab, 100);
        ui.selectable_views.insert(
            tab,
            SelectableView {
                area: Rect::new(0, 0, 80, 20),
                start_line: 0,
                horizontal: 0,
                lines: Vec::new(),
            },
        );
        ui.scroll.insert(tab, Viewport::default());
        let before = ui.background.stats();
        ui.scroll_hit(Some(Hit::Pane(group)), true);
        assert_eq!(ui.scroll.get(&tab).map(Viewport::offset), Some(WHEEL_ROWS));
        let after = ui.background.stats();
        assert_eq!(before.queued, after.queued);
        assert_eq!(before.running, after.running);
        assert_eq!(before.completed, after.completed);
        ui.scroll_hit(Some(Hit::Pane(group)), false);
        assert_eq!(ui.scroll.get(&tab).map(Viewport::offset), Some(0));
    }

    #[test]
    fn file_tail_overscroll_adds_only_renderer_blank_space_not_fake_source_rows() {
        let mut ui = model();
        let path = "src/overscroll.rs".to_owned();
        let source = (0..20).map(|i| format!("line-{i}")).collect::<Vec<_>>();
        ui.files.insert(path.clone().into(), source.join("\n"));
        ui.file_lines.insert(path.clone().into(), Arc::new(source));
        let total = ui.file_line_count(&path.clone().into());
        let mut viewport = Viewport::default();
        viewport.configure(total, 8, true);
        viewport.end();
        viewport.scroll(4);
        assert!(viewport.offset() > total.saturating_sub(8));
        assert_eq!(viewport.source_scrollbar_offset(), total.saturating_sub(8));
        let slice = ui.file_view_slice(&path.clone().into(), viewport.offset(), 8);
        assert!(
            slice.len() < 8,
            "overscroll tail must remain renderer blank space"
        );
        let visible = crate::tui::markdown::plain_lines(&slice).join("\n");
        assert!(visible.contains("line-19"));
        assert!(!visible.contains("overscroll"));
    }

    #[test]
    fn page_scroll_uses_visible_viewport_height() {
        let mut ui = model();
        let group = ui.group();
        let tab = ui.layout.active(group).unwrap().id;
        ui.line_counts.insert(tab, 200);
        ui.selectable_views.insert(
            tab,
            SelectableView {
                area: Rect::new(0, 0, 80, 30),
                start_line: 0,
                horizontal: 0,
                lines: Vec::new(),
            },
        );
        ui.scroll.insert(tab, Viewport::default());
        ui.page_scroll(true);
        assert_eq!(ui.scroll.get(&tab).map(Viewport::offset), Some(28));
    }

    #[test]
    fn settings_is_a_real_tab_without_conversation_composer() {
        let mut ui = model();
        ui.open_settings();
        let group = ui.group();
        assert!(matches!(
            ui.layout.active(group).map(|tab| &tab.target),
            Some(Target::Settings)
        ));
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|f| ui.draw(f)).unwrap();
        assert!(ui.hit_regions.iter().any(|region| {
            matches!(region.hit, Hit::Settings { group: candidate } if candidate == group)
        }));
        assert!(!ui.hit_regions.iter().any(|region| {
            matches!(region.hit, Hit::Composer(candidate) if candidate == group)
        }));
    }

    #[test]
    fn explorer_moves_right_and_hidden_projection_preserves_main_groups() {
        let mut ui = model();
        let first_group = ui.group();
        ui.apply(Action::SplitGroup {
            pane: first_group,
            side: Side::Right,
        });
        let second_group = ui.layout.focused;
        ui.apply(Action::Open {
            group: second_group,
            target: Target::Help,
            pinned: true,
        });
        ui.set_explorer_placement(ExplorerPlacement::Right);
        let order = ui.layout.ordered_panes();
        let explorer = ui.explorer_pane().unwrap();
        assert_eq!(order.last().copied(), Some(explorer));

        ui.set_explorer_placement(ExplorerPlacement::Hidden);
        let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
        terminal.draw(|f| ui.draw(f)).unwrap();
        assert!(!ui
            .hit_regions
            .iter()
            .any(|region| { matches!(region.hit, Hit::Pane(candidate) if candidate == explorer) }));
        assert!(ui.hit_regions.iter().any(|region| {
            matches!(region.hit, Hit::Pane(candidate) if candidate == first_group)
        }));
        assert!(ui.hit_regions.iter().any(|region| {
            matches!(region.hit, Hit::Pane(candidate) if candidate == second_group)
        }));
    }

    #[test]
    fn files_filter_uses_workspace_search_results_not_only_expanded_tree() {
        let mut ui = model();
        let explorer = ui.explorer_pane().unwrap();
        ui.set_navigator(Navigator::Files);
        {
            let state = ui.nav.entry((explorer, Navigator::Files)).or_default();
            state.filter = "needle".into();
            state.search_query = "needle".into();
            state.search_pending = Some(91);
        }
        ui.jobs.insert(
            91,
            Job::Search {
                token: ui
                    .file_token(&ScopeId::launch(), ui.layout.files_container_for(explorer))
                    .unwrap(),
                pane: Some(explorer),
                query: "needle".into(),
                cancel: Arc::new(AtomicBool::new(false)),
            },
        );
        ui.response(crate::workspace::files::Response {
            access: ui.file_scopes.access(&ScopeId::launch()),
            ticket: 91,
            result: Ok(Content::Search {
                query: "needle".into(),
                entries: vec![Entry {
                    name: "needle.rs".into(),
                    path: "deep/collapsed/needle.rs".into(),
                    kind: EntryKind::File,
                }],
            }),
        });
        let rows = Ui::file_rows(ui.nav.get(&(explorer, Navigator::Files)).unwrap());
        assert_eq!(rows.len(), 1);
        assert!(rows[0].label.contains("deep/collapsed/needle.rs"));
    }

    #[test]
    fn unsaved_chat_appears_as_muted_new_session_without_persisting() {
        let mut ui = model();
        ui.snapshot.session = None;
        let explorer = ui.explorer_pane().unwrap();
        ui.nav
            .insert((explorer, Navigator::Sessions), NavState::default());
        let rows = ui.session_rows(explorer);
        assert!(rows.iter().any(|row| {
            row.label == "New ○" && matches!(row.item, NavigatorItem::UnsavedSession)
        }));
        ui.nav
            .entry((explorer, Navigator::Sessions))
            .or_default()
            .session_scope = SessionScope::Autonomous;
        assert!(!ui
            .session_rows(explorer)
            .iter()
            .any(|row| matches!(row.item, NavigatorItem::UnsavedSession)));
    }

    #[test]
    fn sessions_subtitle_switches_user_autonomous_and_tree_groups_forks() {
        let mut ui = model();
        ui.snapshot.sessions = vec![
            crate::workspace::owner::SavedSession {
                id: "user-session".into(),
                title: "User work".into(),
                directory: "/tmp/user".into(),
                autonomous: false,
                archived: false,
                updated_at: 0,
                turn_count: 0,
                branches: vec![
                    crate::workspace::owner::SavedBranch {
                        id: "main".into(),
                        name: "main".into(),
                        parent: None,
                    },
                    crate::workspace::owner::SavedBranch {
                        id: "feature".into(),
                        name: "feature".into(),
                        parent: Some("main".into()),
                    },
                ],
            },
            crate::workspace::owner::SavedSession {
                id: "auto-session".into(),
                title: "Autonomous run".into(),
                directory: "/tmp/auto".into(),
                autonomous: true,
                archived: false,
                updated_at: 0,
                turn_count: 0,
                branches: vec![crate::workspace::owner::SavedBranch {
                    id: "main".into(),
                    name: "main".into(),
                    parent: None,
                }],
            },
        ];
        let explorer = ui.explorer_pane().unwrap();
        ui.nav
            .insert((explorer, Navigator::Sessions), NavState::default());
        let user_rows = ui.session_rows(explorer);
        assert!(user_rows.iter().any(|row| row.label.contains("User work")));
        assert!(user_rows.iter().any(|row| row.label.contains("`- feature")));
        assert!(!user_rows
            .iter()
            .any(|row| row.label.contains("Autonomous run")));
        ui.nav
            .entry((explorer, Navigator::Sessions))
            .or_default()
            .session_scope = SessionScope::Autonomous;
        let auto_rows = ui.session_rows(explorer);
        assert!(auto_rows
            .iter()
            .any(|row| row.label.contains("Autonomous run")));
        assert!(!auto_rows.iter().any(|row| row.label.contains("User work")));

        ui.nav
            .entry((explorer, Navigator::Sessions))
            .or_default()
            .session_scope = SessionScope::User;
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|f| ui.draw(f)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("User"));
        assert!(text.contains("Autonomous"));
        assert!(text.contains("/tmp"));
        assert!(!text.contains("cwd /tmp"));
    }

    #[test]
    fn archived_parent_does_not_hide_visible_child_and_archived_scope_is_explicit() {
        let mut ui = model();
        ui.snapshot.sessions = vec![
            crate::workspace::owner::SavedSession {
                id: "parent".into(),
                title: "Archived parent".into(),
                directory: "/tmp/parent".into(),
                autonomous: false,
                archived: true,
                updated_at: 0,
                turn_count: 1,
                branches: vec![crate::workspace::owner::SavedBranch {
                    id: "main".into(),
                    name: "main".into(),
                    parent: None,
                }],
            },
            crate::workspace::owner::SavedSession {
                id: "child".into(),
                title: "Visible child".into(),
                directory: "/tmp/child".into(),
                autonomous: false,
                archived: false,
                updated_at: 0,
                turn_count: 1,
                branches: vec![crate::workspace::owner::SavedBranch {
                    id: "main".into(),
                    name: "main".into(),
                    parent: None,
                }],
            },
        ];
        ui.snapshot.subagents = vec![crate::workspace::owner::SavedSubagent {
            parent_session: "parent".into(),
            child_session: "child".into(),
            state: "completed".into(),
            mode: "background".into(),
            tool_mode: "off".into(),
            depth: 1,
        }];
        let explorer = ui.explorer_pane().unwrap();
        ui.nav
            .insert((explorer, Navigator::Sessions), NavState::default());

        let visible = ui.session_rows(explorer);
        assert!(visible.iter().any(|row| {
            matches!(&row.item, NavigatorItem::SessionRoot { session } if session == "child")
        }));
        assert!(!visible
            .iter()
            .any(|row| row.label.contains("Archived parent")));

        ui.nav
            .get_mut(&(explorer, Navigator::Sessions))
            .unwrap()
            .session_scope = SessionScope::Archived;
        let archived = ui.session_rows(explorer);
        assert!(archived
            .iter()
            .any(|row| row.label.contains("Archived parent")));
        assert!(!archived
            .iter()
            .any(|row| row.label.contains("Visible child")));
    }

    #[test]
    fn session_list_branch_filter_opens_the_branch_that_matched() {
        let mut ui = model();
        ui.snapshot.sessions = vec![crate::workspace::owner::SavedSession {
            id: "session".into(),
            title: "Work".into(),
            directory: "/tmp/work".into(),
            autonomous: false,
            archived: false,
            updated_at: 0,
            turn_count: 2,
            branches: vec![
                crate::workspace::owner::SavedBranch {
                    id: "main".into(),
                    name: "main".into(),
                    parent: None,
                },
                crate::workspace::owner::SavedBranch {
                    id: "feature-x".into(),
                    name: "feature-x".into(),
                    parent: Some("main".into()),
                },
            ],
        }];
        let explorer = ui.explorer_pane().unwrap();
        let state = ui.nav.entry((explorer, Navigator::Sessions)).or_default();
        state.display = NavDisplay::List;
        state.filter = "feature-x".into();
        let rows = ui.session_rows(explorer);
        assert_eq!(rows.len(), 1);
        assert!(matches!(
            &rows[0].item,
            NavigatorItem::SessionBranch { session, branch }
                if session == "session" && branch == "feature-x"
        ));
    }

    #[test]
    fn session_actions_use_only_visible_selection_and_never_archive_active_agent() {
        let mut ui = model();
        ui.snapshot.session = Some("a".into());
        ui.snapshot.sessions = ["a", "b"]
            .into_iter()
            .map(|id| crate::workspace::owner::SavedSession {
                id: id.into(),
                title: id.to_uppercase(),
                directory: format!("/tmp/{id}"),
                autonomous: false,
                archived: false,
                updated_at: 0,
                turn_count: 1,
                branches: vec![crate::workspace::owner::SavedBranch {
                    id: "main".into(),
                    name: "main".into(),
                    parent: None,
                }],
            })
            .collect();
        let explorer = ui.explorer_pane().unwrap();
        let state = ui.nav.entry((explorer, Navigator::Sessions)).or_default();
        state.display = NavDisplay::List;
        state.multi_select = true;
        state.selected_sessions = BTreeSet::from(["a".into(), "b".into()]);
        state.filter = "b".into();

        assert_eq!(
            ui.selected_session_action_targets(explorer, true),
            vec!["b".to_owned()],
            "hidden filtered selections must not participate in archive actions"
        );

        ui.nav
            .get_mut(&(explorer, Navigator::Sessions))
            .unwrap()
            .filter
            .clear();
        assert_eq!(
            ui.selected_session_action_targets(explorer, true),
            vec!["b".to_owned()],
            "the active Agent must never be archived from its own active view"
        );
    }

    #[test]
    fn archived_scope_delete_targets_restore_not_rearchive() {
        let mut ui = model();
        ui.snapshot.sessions = vec![crate::workspace::owner::SavedSession {
            id: "archived".into(),
            title: "Archived".into(),
            directory: "/tmp/archived".into(),
            autonomous: false,
            archived: true,
            updated_at: 0,
            turn_count: 1,
            branches: vec![crate::workspace::owner::SavedBranch {
                id: "main".into(),
                name: "main".into(),
                parent: None,
            }],
        }];
        let explorer = ui.explorer_pane().unwrap();
        let state = ui.nav.entry((explorer, Navigator::Sessions)).or_default();
        state.display = NavDisplay::List;
        state.session_scope = SessionScope::Archived;
        assert_eq!(
            ui.selected_session_action_targets(explorer, false),
            vec!["archived".to_owned()]
        );
    }

    #[test]
    fn subagents_nest_under_parent_session_and_render_agent_sidecar() {
        let mut ui = model();
        ui.snapshot.session = Some("parent".into());
        ui.snapshot.session_title = Some("Parent agent".into());
        ui.snapshot.sessions = vec![
            crate::workspace::owner::SavedSession {
                id: "parent".into(),
                title: "Parent agent".into(),
                directory: "/tmp".into(),
                autonomous: false,
                archived: false,
                updated_at: 0,
                turn_count: 0,
                branches: vec![crate::workspace::owner::SavedBranch {
                    id: "main".into(),
                    name: "main".into(),
                    parent: None,
                }],
            },
            crate::workspace::owner::SavedSession {
                id: "child".into(),
                title: "Research child".into(),
                directory: "/tmp".into(),
                autonomous: false,
                archived: false,
                updated_at: 0,
                turn_count: 0,
                branches: vec![crate::workspace::owner::SavedBranch {
                    id: "main".into(),
                    name: "main".into(),
                    parent: None,
                }],
            },
            crate::workspace::owner::SavedSession {
                id: "grandchild".into(),
                title: "Leaf child".into(),
                directory: "/tmp".into(),
                autonomous: false,
                archived: false,
                updated_at: 0,
                turn_count: 0,
                branches: vec![crate::workspace::owner::SavedBranch {
                    id: "main".into(),
                    name: "main".into(),
                    parent: None,
                }],
            },
        ];
        ui.snapshot.subagents = vec![
            crate::workspace::owner::SavedSubagent {
                parent_session: "parent".into(),
                child_session: "child".into(),
                state: "running".into(),
                mode: "background".into(),
                tool_mode: "workspace".into(),
                depth: 1,
            },
            crate::workspace::owner::SavedSubagent {
                parent_session: "child".into(),
                child_session: "grandchild".into(),
                state: "running".into(),
                mode: "foreground".into(),
                tool_mode: "workspace".into(),
                depth: 2,
            },
        ];
        ui.snapshot.delegation_budget = Some(crate::workspace::owner::SavedDelegationBudget {
            state: "active".into(),
            children_created: 2,
            max_children_created: 32,
            active_children: 2,
            max_active_children: 4,
            model_attempts_started: 3,
            max_model_attempts: 64,
        });
        let explorer = ui.explorer_pane().unwrap();
        ui.nav
            .insert((explorer, Navigator::Sessions), NavState::default());
        let rows = ui.session_rows(explorer);
        let parent_at = rows
            .iter()
            .position(|row| row.label.contains("Parent agent"))
            .unwrap();
        let child_at = rows
            .iter()
            .position(|row| row.label.contains("Research child"))
            .unwrap();
        assert!(child_at > parent_at);
        assert!(rows[child_at].label.contains("●"));
        assert!(rows[child_at].label.contains("workspace · isolated"));
        assert!(rows[child_at].label.contains("d1"));
        let grandchild_at = rows
            .iter()
            .position(|row| row.label.contains("Leaf child"))
            .unwrap();
        assert!(grandchild_at > child_at);
        assert!(rows[grandchild_at].label.contains("d2"));
        assert!(rows[grandchild_at].label.contains("workspace · isolated"));
        assert_eq!(
            rows.iter()
                .filter(|row| row.label.trim_start().starts_with("v Research child"))
                .count(),
            0,
            "child must not also render as an unrelated tree root"
        );

        let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
        terminal.draw(|frame| ui.draw(frame)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("Subagents active 2/32"));
        assert!(text.contains("active 2/4"));
        assert!(text.contains("calls 3/64"));
        assert!(text.contains("Research child"));
        assert!(text.contains("workspace · isolated"));
        assert!(ui.hit_regions.iter().any(|region| {
            matches!(
                &region.hit,
                Hit::AgentSession { session } if session == "child"
            )
        }));
        assert!(!ui.hit_regions.iter().any(|region| {
            matches!(
                &region.hit,
                Hit::AgentSession { session } if session == "grandchild"
            )
        }));

        ui.snapshot.session = Some("child".into());
        ui.snapshot.session_title = Some("Research child".into());
        terminal.draw(|frame| ui.draw(frame)).unwrap();
        assert!(ui.hit_regions.iter().any(|region| {
            matches!(
                &region.hit,
                Hit::AgentSession { session } if session == "parent"
            )
        }));
        assert!(ui.hit_regions.iter().any(|region| {
            matches!(
                &region.hit,
                Hit::AgentSession { session } if session == "grandchild"
            )
        }));

        ui.snapshot.session = Some("grandchild".into());
        ui.snapshot.session_title = Some("Leaf child".into());
        terminal.draw(|frame| ui.draw(frame)).unwrap();
        assert!(ui.hit_regions.iter().any(|region| {
            matches!(
                &region.hit,
                Hit::AgentSession { session } if session == "child"
            )
        }));
        assert!(!ui.hit_regions.iter().any(|region| {
            matches!(
                &region.hit,
                Hit::AgentSession { session } if session == "parent"
            )
        }));
    }

    #[test]
    fn segmented_controls_hit_every_visible_label_and_stack_when_narrow() {
        let sessions = [
            (SessionScope::User, "User"),
            (SessionScope::Autonomous, "Autonomous"),
        ];
        let mut wide_hits = Vec::new();
        let mut wide_used = 0;
        let mut terminal = Terminal::new(TestBackend::new(24, 4)).unwrap();
        terminal
            .draw(|frame| {
                (wide_hits, wide_used) = draw_segmented_control(
                    frame,
                    Rect::new(0, 0, 24, 4),
                    &sessions,
                    SessionScope::User,
                    Color::LightMagenta,
                    Color::Gray,
                );
            })
            .unwrap();
        assert_eq!(wide_used, 1);
        let autonomous = wide_hits
            .iter()
            .find(|(_, scope)| *scope == SessionScope::Autonomous)
            .unwrap()
            .0;
        assert_eq!(autonomous.width, "Autonomous".len() as u16);
        assert_eq!(
            autonomous.x,
            (UnicodeWidthStr::width("User") + UnicodeWidthStr::width(" · ")) as u16
        );

        let git = [
            (git::Mode::Changes, "Changes"),
            (git::Mode::Worktrees, "Worktrees"),
            (git::Mode::Branches, "Branches"),
        ];
        let mut narrow_hits = Vec::new();
        let mut narrow_used = 0;
        let mut terminal = Terminal::new(TestBackend::new(12, 5)).unwrap();
        terminal
            .draw(|frame| {
                (narrow_hits, narrow_used) = draw_segmented_control(
                    frame,
                    Rect::new(0, 0, 12, 5),
                    &git,
                    git::Mode::Changes,
                    Color::LightGreen,
                    Color::Gray,
                );
            })
            .unwrap();
        assert_eq!(narrow_used, 3);
        assert_eq!(
            narrow_hits
                .iter()
                .map(|(area, _)| area.y)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert!(narrow_hits.iter().all(|(area, _)| area.width == 12));
    }

    #[test]
    fn session_scope_rendered_label_is_inside_its_mouse_hit_region() {
        let mut ui = model();
        let explorer = ui.explorer_pane().unwrap();
        ui.nav
            .insert((explorer, Navigator::Sessions), NavState::default());
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|frame| ui.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let mut found = None;
        for y in 0..buffer.area.height {
            let line = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>();
            if let Some(byte_x) = line.find("Autonomous") {
                let cell_x = UnicodeWidthStr::width(&line[..byte_x]) as u16;
                found = Some((cell_x, y));
                break;
            }
        }
        let (x, y) = found.expect("Autonomous label rendered");
        let hit = ui
            .hit_regions
            .iter()
            .find(|region| {
                matches!(
                    region.hit,
                    Hit::SessionScope {
                        scope: SessionScope::Autonomous,
                        ..
                    }
                )
            })
            .expect("Autonomous hit region");
        assert!(hit.area.x <= x);
        assert!(hit.area.x.saturating_add(hit.area.width) >= x + "Autonomous".len() as u16);
        assert_eq!(hit.area.y, y);
    }

    #[test]
    fn composer_uses_outer_pane_bottom_as_its_bottom_boundary() {
        let mut ui = model();
        let group = ui.group();
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|f| ui.draw(f)).unwrap();
        let pane = ui
            .hit_regions
            .iter()
            .find_map(|region| {
                matches!(region.hit, Hit::Pane(candidate) if candidate == group)
                    .then_some(region.area)
            })
            .unwrap();
        let composer = ui
            .hit_regions
            .iter()
            .find_map(|region| {
                matches!(region.hit, Hit::Composer(candidate) if candidate == group)
                    .then_some(region.area)
            })
            .unwrap();
        assert_eq!(composer.bottom(), pane.bottom());
        let buffer = terminal.backend().buffer();
        let top = (composer.x..composer.x.saturating_add(composer.width))
            .map(|x| buffer[(x, composer.y)].symbol())
            .collect::<String>();
        assert!(!top.contains("Message"));
        assert!(
            top.chars().filter(|c| *c == '─').count()
                >= usize::from(composer.width.saturating_sub(1))
        );
        assert_eq!(
            composer.height, 2,
            "fullscreen Agent composer is one editable row plus its top rule"
        );
        let edit = (composer.x..composer.x.saturating_add(composer.width))
            .map(|x| buffer[(x, composer.y + 1)].symbol())
            .collect::<String>();
        assert!(edit.contains("draft remains local"));
        assert!(!edit.contains("Message ·"));
    }

    #[test]
    fn main_find_renders_flash_labels_and_jumps_by_label() {
        let mut ui = model();
        let group = ui.group();
        ui.apply(Action::Open {
            group,
            target: Target::Help,
            pinned: true,
        });
        let tab = ui.layout.active(group).unwrap().id;
        ui.update_find(tab, "shortcuts".into(), true);
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|f| ui.draw(f)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("[a]"));
        assert!(ui.flash_jump('a'));
        assert!(!ui.find.get(&tab).unwrap().choosing);
    }

    #[test]
    fn command_palette_is_searchable_and_not_debug_chrome() {
        let mut ui = model();
        ui.open_palette();
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|f| ui.draw(f)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(text.contains("Command Palette"));
        assert!(text.contains("Focus Agent"));
        assert!(text.contains("> Actions"));
        assert!(text.contains("@ Files"));
        assert!(text.contains("# Sessions"));
        assert!(text.contains("! Windows"));
        assert!(text.contains("$ Terminals"));
        assert!(text.contains("/ Commands"));
        assert!(!text.contains("ACTIONS"));
        assert!(!text.contains("· Content"));
        assert_ne!(
            PaletteCategory::Actions.color(),
            PaletteCategory::Files.color()
        );
        assert_ne!(
            PaletteCategory::Sessions.color(),
            PaletteCategory::Windows.color()
        );
    }

    #[test]
    fn palette_prefixes_select_exact_categories_without_changing_search_body() {
        for (raw, category, text) in [
            (">set model", Some(PaletteCategory::Actions), "set model"),
            ("@Cargo.toml", Some(PaletteCategory::Files), "Cargo.toml"),
            ("#hello", Some(PaletteCategory::Sessions), "hello"),
            ("!settings", Some(PaletteCategory::Windows), "settings"),
            ("$term", Some(PaletteCategory::Terminals), "term"),
            ("/think", Some(PaletteCategory::Commands), "think"),
            ("&worker", Some(PaletteCategory::Agents), "worker"),
            ("plain query", None, "plain query"),
        ] {
            let parsed = palette::parse(raw);
            assert_eq!(parsed.category, category);
            assert_eq!(parsed.text, text);
        }
        assert_eq!(
            palette::with_category("hello", Some(PaletteCategory::Files)),
            "@hello"
        );
        assert_eq!(palette::with_category("@hello", None), "hello");
    }

    #[test]
    fn palette_categories_have_click_regions_and_prefix_filtering() {
        let mut ui = model();
        ui.open_palette();
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|f| ui.draw(f)).unwrap();
        let categories = ui
            .hit_regions
            .iter()
            .filter_map(|region| match region.hit {
                Hit::PaletteCategory(category) => Some((category, region.area)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(categories.len(), 7);
        assert!(categories.iter().all(|(_, area)| area.width > 0));

        let actions = ui.palette_items(">settings");
        assert!(!actions.is_empty());
        assert!(actions
            .iter()
            .all(|item| item.category == PaletteCategory::Actions));
        let commands = ui.palette_items("/think");
        assert!(commands
            .iter()
            .any(|item| { item.category == PaletteCategory::Commands && item.label == "/think" }));
        assert!(commands
            .iter()
            .all(|item| item.category == PaletteCategory::Commands));
        let all = ui.palette_items("settings");
        assert!(all.len() >= actions.len());
    }

    #[test]
    fn explorer_category_hits_share_one_row_when_narrow() {
        let mut ui = model();
        let explorer = ui.explorer_pane().unwrap();
        let mut terminal = Terminal::new(TestBackend::new(80, 28)).unwrap();
        terminal.draw(|f| ui.draw(f)).unwrap();
        let mut hits = ui
            .hit_regions
            .iter()
            .filter_map(|region| match region.hit {
                Hit::NavigatorKind { pane, kind } if pane == explorer => Some((kind, region.area)),
                _ => None,
            })
            .collect::<Vec<_>>();
        hits.sort_by_key(|(_, area)| area.y);
        assert!(!hits.is_empty());
        assert_eq!(
            hits.iter()
                .map(|(_, area)| area.y)
                .collect::<BTreeSet<_>>()
                .len(),
            1
        );
        for (index, (_, area)) in hits.iter().enumerate() {
            for (_, other) in hits.iter().skip(index + 1) {
                assert!(!rect_contains(*other, (area.x, area.y)));
                assert!(!rect_contains(*area, (other.x, other.y)));
            }
        }
    }

    #[test]
    fn main_tabs_keep_active_visible_in_one_row_when_narrow() {
        let mut ui = model();
        ui.set_explorer_placement(ExplorerPlacement::Hidden);
        ui.open(Target::Help);
        ui.open(Target::Settings);
        ui.open(Target::File {
            scope_id: ScopeId::launch(),
            path: "a/very/long/path/that/forces/the/tab/strip/to/fold.rs".into(),
        });
        let group = ui.group();
        let mut terminal = Terminal::new(TestBackend::new(28, 24)).unwrap();
        terminal.draw(|f| ui.draw(f)).unwrap();
        let rows = ui
            .hit_regions
            .iter()
            .filter_map(|region| match region.hit {
                Hit::GroupTab {
                    group: candidate, ..
                } if candidate == group => Some(region.area.y),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(rows.len(), 1, "narrow tabs retain one header row");
        let active = ui.layout.active(group).unwrap().id;
        assert!(ui.hit_regions.iter().any(|region| matches!(region.hit,
            Hit::GroupTab { group: candidate, tab } if candidate == group && tab == active)));
    }

    #[test]
    fn maximize_roundtrip_preserves_layout_draft_and_scroll() {
        let mut ui = model();
        let group = ui.group();
        ui.open(Target::File {
            scope_id: ScopeId::launch(),
            path: "src/lib.rs".into(),
        });
        let tab = ui.layout.active(group).unwrap().id;
        let mut scroll = Viewport::default();
        scroll.configure(200, 10, false);
        scroll.scroll(37);
        ui.scroll.insert(tab, scroll);
        ui.files.insert(
            "src/lib.rs".into(),
            (0..200).map(|row| format!("line {row}\n")).collect(),
        );
        ui.file_lines.insert(
            "src/lib.rs".into(),
            Arc::new((0..200).map(|row| format!("line {row}")).collect()),
        );
        ui.draft.set("unsent draft").unwrap();
        ui.apply(Action::Focus(group));
        let layout = ui.layout.clone();
        ui.toggle_maximize(group);
        assert!(ui.maximized);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| ui.draw(frame)).unwrap();
        let control = ui
            .hit_regions
            .iter()
            .find(|region| matches!(region.hit, Hit::MaximizePane(id) if id == group))
            .expect("restore control");
        assert_eq!(control.area.height, 1);
        assert!(control.area.width <= 2);
        ui.toggle_maximize(group);
        assert!(!ui.maximized);
        assert_eq!(ui.layout, layout);
        assert_eq!(ui.draft.text(), "unsent draft");
        assert_eq!(ui.scroll.get(&tab).unwrap().offset(), 37);
    }

    #[test]
    fn agents_palette_uses_actual_owner_relationships_and_exact_branches() {
        let mut ui = model();
        assert!(ui.palette_items("&").is_empty());
        ui.snapshot.session = Some("root-agent".into());
        ui.snapshot.branch = "work".into();
        ui.snapshot.branch_id = Some("work".into());
        ui.snapshot.subagents = vec![crate::workspace::owner::SavedSubagent {
            parent_session: "root-agent".into(),
            child_session: "child-agent".into(),
            state: "completed".into(),
            mode: "readonly".into(),
            tool_mode: "none".into(),
            depth: 1,
        }];
        ui.snapshot.sessions = vec![crate::workspace::owner::SavedSession {
            id: "child-agent".into(),
            title: "Review child".into(),
            directory: "/tmp/project".into(),
            autonomous: false,
            archived: false,
            updated_at: 0,
            turn_count: 2,
            branches: vec![crate::workspace::owner::SavedBranch {
                id: "review".into(),
                name: "review".into(),
                parent: None,
            }],
        }];
        let items = ui.palette_items("&");
        assert_eq!(items.len(), 2);
        assert!(items
            .iter()
            .all(|item| item.category == PaletteCategory::Agents));
        assert!(items.iter().any(|item| matches!(&item.target, PaletteTarget::Session { session, branch } if session == "root-agent" && branch == "work")));
        assert!(items.iter().any(|item| matches!(&item.target, PaletteTarget::Session { session, branch } if session == "child-agent" && branch == "review") && item.description.contains("completed")));
        assert_eq!(ui.palette_items("&Review").len(), 1);
    }

    #[test]
    fn maximizing_another_pane_leaves_the_composer_without_clearing_its_draft() {
        let mut ui = model();
        ui.open(Target::Conversation);
        ui.draft.set("keep this draft").unwrap();
        assert!(ui.input);
        let navigator = ui.explorer_pane().unwrap();
        ui.toggle_maximize(navigator);
        assert_eq!(ui.layout.focused, navigator);
        assert!(
            !ui.input,
            "navigator focus cannot retain composer Ctrl+C semantics"
        );
        assert_eq!(ui.draft.text(), "keep this draft");
        ui.toggle_maximize(navigator);
        assert!(!ui.input);
        assert_eq!(ui.draft.text(), "keep this draft");
    }

    #[test]
    fn focusing_the_current_agent_preserves_busy_state_and_unsent_input() {
        let mut ui = model();
        ui.snapshot.session = Some("root-agent".into());
        ui.snapshot.branch = "work".into();
        ui.snapshot.branch_id = Some("work".into());
        ui.snapshot.busy = true;
        ui.draft.set("keep this draft").unwrap();
        ui.recovery = "rejected input".into();
        let original_group = ui.group();
        ui.apply(Action::SplitGroup {
            pane: original_group,
            side: Side::Right,
        });
        ui.open(Target::File {
            scope_id: ScopeId::launch(),
            path: "src/lib.rs".into(),
        });
        let layout = ui.layout.clone();
        assert!(!ui.focus_current_agent("root-agent", "main"));
        assert!(!ui.focus_current_agent("another-agent", "work"));
        assert_eq!(ui.layout, layout);
        let turns = ui.snapshot.turns.clone();
        assert!(ui.focus_current_agent("root-agent", "work"));
        assert_eq!(ui.layout.focused, original_group);
        assert_eq!(
            ui.layout
                .tabs
                .values()
                .filter(|tab| matches!(tab.target, Target::Conversation))
                .count(),
            1
        );
        assert!(matches!(
            ui.layout.active(ui.layout.focused).map(|tab| &tab.target),
            Some(Target::Conversation)
        ));
        assert_eq!(ui.draft.text(), "keep this draft");
        assert_eq!(ui.recovery, "rejected input");
        assert!(ui.snapshot.busy);
        assert!(Arc::ptr_eq(&ui.snapshot.turns, &turns));
        assert!(ui.jobs.is_empty());
    }

    #[test]
    fn minimal_scroll_indicator_shows_track_and_thumb_only_when_needed() {
        let mut terminal = Terminal::new(TestBackend::new(4, 8)).unwrap();
        terminal
            .draw(|frame| {
                draw_scroll_indicator(
                    frame,
                    Rect::new(0, 0, 4, 8),
                    100,
                    10,
                    45,
                    crate::tui::theme::palette(aw::config::ThemeMode::Dark),
                );
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains('┃'));
        assert!(text.contains('│'));
        let palette = crate::tui::theme::palette(aw::config::ThemeMode::Dark);
        let buffer = terminal.backend().buffer();
        let thumb = (0..8)
            .map(|y| &buffer[(3, y)])
            .find(|cell| cell.symbol() == "┃")
            .expect("active scrollbar thumb");
        let track = (0..8)
            .map(|y| &buffer[(3, y)])
            .find(|cell| cell.symbol() == "│")
            .expect("inactive scrollbar track");
        assert_eq!(thumb.fg, palette.scrollbar_thumb());
        assert_eq!(track.fg, palette.scrollbar_track());
        assert_eq!(thumb.fg, palette.accent);
        assert_ne!(thumb.fg, track.fg);
    }

    #[test]
    fn closing_tab_prunes_find_cancel_and_selection_state() {
        let mut ui = model();
        let group = ui.group();
        ui.open(Target::Help);
        let tab = ui.layout.active(group).unwrap().id;
        ui.find.insert(
            tab,
            FindState {
                query: "bird".into(),
                matches: vec![1],
                selected: 0,
                choosing: false,
                committed: false,
            },
        );
        ui.find_generation.insert(tab, 7);
        let cancel = Arc::new(AtomicBool::new(false));
        ui.find_cancel.insert(tab, cancel.clone());
        ui.text_selection = Some(TextSelection {
            tab,
            anchor: TextPoint { line: 0, col: 0 },
            current: TextPoint { line: 0, col: 1 },
        });
        ui.selection_dragging = true;
        ui.selection_moved = true;

        ui.apply(Action::CloseTab { group, tab });
        assert!(!ui.find.contains_key(&tab));
        assert!(!ui.find_generation.contains_key(&tab));
        assert!(!ui.find_cancel.contains_key(&tab));
        assert!(cancel.load(Ordering::Acquire));
        assert!(ui.text_selection.is_none());
        assert!(!ui.selection_dragging);
        assert!(!ui.selection_moved);
    }

    #[test]
    fn switching_away_from_files_cancels_its_recursive_search_job() {
        let mut ui = model();
        ui.set_navigator(Navigator::Files);
        let pane = ui.explorer_pane().unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        ui.jobs.insert(
            98,
            Job::Search {
                token: ui.file_token(&ScopeId::launch(), None).unwrap(),
                pane: Some(pane),
                query: "obsolete".into(),
                cancel: cancel.clone(),
            },
        );
        ui.nav
            .entry((pane, Navigator::Files))
            .or_default()
            .search_pending = Some(98);
        ui.set_navigator(Navigator::Git);
        assert!(cancel.load(Ordering::Acquire));
        assert!(!ui.jobs.contains_key(&98));
    }

    #[test]
    fn closing_navigator_pane_cancels_its_stale_recursive_search_job() {
        let mut ui = model();
        let pane = ui.explorer_pane().unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        ui.jobs.insert(
            99,
            Job::Search {
                token: ui.file_token(&ScopeId::launch(), None).unwrap(),
                pane: Some(pane),
                query: "obsolete".into(),
                cancel: cancel.clone(),
            },
        );
        ui.apply(Action::ClosePane(pane));
        assert!(cancel.load(Ordering::Acquire));
        assert!(!ui.jobs.contains_key(&99));
    }

    #[test]
    fn diff_highlight_cache_prunes_reviews_evicted_by_git_cache() {
        let mut ui = model();
        let cancel = Arc::new(AtomicBool::new(false));
        ui.diff_highlights.insert(
            "expired".into(),
            HighlightCache {
                generation: 1,
                theme: ui.ui_config.theme,
                lines: Vec::new(),
                complete: false,
                cancel: cancel.clone(),
            },
        );
        ui.schedule_diff_highlight("missing");
        assert!(ui.diff_highlights.is_empty());
        assert!(cancel.load(Ordering::Acquire));
    }

    #[test]
    fn read_only_text_selection_extracts_unicode_aware_multiline_text() {
        let mut ui = model();
        let tab = ui.layout.active(ui.group()).unwrap().id;
        ui.selectable_views.insert(
            tab,
            SelectableView {
                area: Rect::new(0, 0, 40, 2),
                start_line: 3,
                horizontal: 0,
                lines: vec!["hello world".into(), "second line".into()],
            },
        );
        ui.text_selection = Some(TextSelection {
            tab,
            anchor: TextPoint { line: 3, col: 1 },
            current: TextPoint { line: 4, col: 5 },
        });
        assert_eq!(ui.selection_text().as_deref(), Some("ello world\nsecond"));
        assert_eq!(base64_bytes(b"hello"), "aGVsbG8=");
    }

    #[test]
    fn palette_file_search_reply_becomes_a_typed_file_result() {
        let mut ui = model();
        ui.open_palette();
        ui.palette_file_query = "needle".into();
        ui.palette_file_pending = Some(7);
        ui.jobs.insert(
            7,
            Job::Search {
                token: ui.file_token(&ScopeId::launch(), None).unwrap(),
                pane: None,
                query: "needle".into(),
                cancel: Arc::new(AtomicBool::new(false)),
            },
        );
        ui.response(crate::workspace::files::Response {
            access: ui.file_scopes.access(&ScopeId::launch()),
            ticket: 7,
            result: Ok(Content::Search {
                query: "needle".into(),
                entries: vec![Entry {
                    name: "needle.rs".into(),
                    path: "src/needle.rs".into(),
                    kind: EntryKind::File,
                }],
            }),
        });
        assert!(ui.palette_file_pending.is_none());
        assert!(ui.palette_items("needle").iter().any(|item| {
            item.category == PaletteCategory::Files
                && matches!(&item.target, PaletteTarget::File(key) if key.path == "src/needle.rs")
        }));
    }

    #[test]
    fn command_palette_searches_runtime_files_sessions_and_windows() {
        let mut ui = model();
        ui.set_navigator(Navigator::Files);

        let mut files = NavState::default();
        files.directories.insert(
            String::new(),
            vec![Entry {
                name: "main.rs".into(),
                path: "src/main.rs".into(),
                kind: EntryKind::File,
            }],
        );
        ui.nav.insert((1, Navigator::Files), files);

        ui.snapshot
            .sessions
            .push(crate::workspace::owner::SavedSession {
                id: "session-fixture".into(),
                title: "Release work".into(),
                directory: "/tmp/project".into(),
                autonomous: false,
                archived: false,
                updated_at: 0,
                turn_count: 0,
                branches: vec![crate::workspace::owner::SavedBranch {
                    id: "feature-x".into(),
                    name: "feature-x".into(),
                    parent: None,
                }],
            });

        let group = ui.group();
        ui.apply(Action::Open {
            group,
            target: Target::Help,
            pinned: true,
        });

        assert!(ui.palette_items("main.rs").iter().any(|item| {
            item.category == PaletteCategory::Files
                && matches!(&item.target, PaletteTarget::File(key) if key.path == "src/main.rs")
        }));
        assert!(ui.palette_items("feature-x").iter().any(|item| {
            item.category == PaletteCategory::Sessions
                && matches!(
                    &item.target,
                    PaletteTarget::Session { session, branch }
                        if session == "session-fixture" && branch == "feature-x"
                )
        }));
        assert!(ui.palette_items("shortcuts").iter().any(|item| {
            item.category == PaletteCategory::Windows
                && matches!(&item.target, PaletteTarget::Window { .. })
                && item.label == "Help"
        }));
    }
}
