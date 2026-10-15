use super::editor::{edit_key, is_plain_enter, EditMode};
use crate::tui::commands::{canonical_slash_name, cycle_selection};
use aw_editor::Editor;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Category {
    Actions,
    Windows,
    Files,
    Sessions,
    Terminals,
    Commands,
    Agents,
}

impl Category {
    pub(crate) const ALL: [Self; 7] = [
        Self::Actions,
        Self::Windows,
        Self::Files,
        Self::Sessions,
        Self::Terminals,
        Self::Commands,
        Self::Agents,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Actions => "Actions",
            Self::Windows => "Windows",
            Self::Files => "Files",
            Self::Sessions => "Sessions",
            Self::Terminals => "Terminals",
            Self::Commands => "Commands",
            Self::Agents => "Agents",
        }
    }

    pub(crate) const fn prefix(self) -> char {
        match self {
            Self::Actions => '>',
            Self::Windows => '!',
            Self::Files => '@',
            Self::Sessions => '#',
            Self::Terminals => '$',
            Self::Commands => '/',
            Self::Agents => '&',
        }
    }

    pub(crate) const fn from_prefix(value: char) -> Option<Self> {
        match value {
            '>' => Some(Self::Actions),
            '!' => Some(Self::Windows),
            '@' => Some(Self::Files),
            '#' => Some(Self::Sessions),
            '$' => Some(Self::Terminals),
            '/' => Some(Self::Commands),
            '&' => Some(Self::Agents),
            _ => None,
        }
    }

    pub(crate) const fn color(self) -> Color {
        match self {
            Self::Actions => Color::LightBlue,
            Self::Windows => Color::LightYellow,
            Self::Files => Color::LightGreen,
            Self::Sessions => Color::LightMagenta,
            Self::Terminals => Color::LightCyan,
            Self::Commands => Color::LightRed,
            Self::Agents => Color::LightBlue,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Query<'a> {
    pub(crate) category: Option<Category>,
    pub(crate) text: &'a str,
}

pub(crate) fn parse(input: &str) -> Query<'_> {
    let input = input.trim_start();
    let Some(first) = input.chars().next() else {
        return Query {
            category: None,
            text: "",
        };
    };
    if let Some(category) = Category::from_prefix(first) {
        let offset = first.len_utf8();
        return Query {
            category: Some(category),
            text: input[offset..].trim_start(),
        };
    }
    Query {
        category: None,
        text: input,
    }
}

pub(crate) fn with_category(input: &str, category: Option<Category>) -> String {
    let parsed = parse(input);
    match category {
        Some(category) if parsed.text.is_empty() => category.prefix().to_string(),
        Some(category) => format!("{}{}", category.prefix(), parsed.text),
        None => parsed.text.to_owned(),
    }
}

pub(crate) fn step(selection: usize, len: usize, forward: bool) -> usize {
    cycle_selection(selection, len, forward)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SlashResolution {
    pub(crate) command: String,
    pub(crate) has_args: bool,
}

pub(crate) fn resolve_slash(input: &str, selected: &str) -> SlashResolution {
    let base = selected.trim_end();
    let input = input.trim();
    let (typed_verb, typed_rest) = input.split_once(' ').unwrap_or((input, ""));
    let typed_canonical = canonical_slash_name(typed_verb).unwrap_or(typed_verb);
    let args = if typed_canonical == base {
        typed_rest.trim()
    } else {
        ""
    };
    SlashResolution {
        command: if args.is_empty() {
            base.to_owned()
        } else {
            format!("{base} {args}")
        },
        has_args: !args.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_are_one_to_one_and_commands_use_slash() {
        for category in Category::ALL {
            assert_eq!(Category::from_prefix(category.prefix()), Some(category));
        }
        assert_eq!(Category::Commands.prefix(), '/');
        assert_eq!(Category::Actions.prefix(), '>');
        assert_eq!(Category::Windows.prefix(), '!');
    }

    #[test]
    fn parse_and_rewrite_preserve_only_the_query_body() {
        let parsed = parse("/think high");
        assert_eq!(parsed.category, Some(Category::Commands));
        assert_eq!(parsed.text, "think high");
        assert_eq!(
            with_category("/think high", Some(Category::Actions)),
            ">think high"
        );
        assert_eq!(with_category("#session", None), "session");
    }

    #[test]
    fn slash_alias_resolution_preserves_inline_arguments() {
        assert_eq!(
            resolve_slash("side what changed?", "btw "),
            SlashResolution {
                command: "btw what changed?".into(),
                has_args: true,
            }
        );
        assert_eq!(
            resolve_slash("clone feature-x", "fork "),
            SlashResolution {
                command: "fork feature-x".into(),
                has_args: true,
            }
        );
        assert_eq!(
            resolve_slash("open", "resume"),
            SlashResolution {
                command: "resume".into(),
                has_args: false,
            }
        );
    }

    #[test]
    fn selection_wraps_both_directions() {
        assert_eq!(step(0, 6, false), 5);
        assert_eq!(step(5, 6, true), 0);
        assert_eq!(step(0, 0, true), 0);
    }
}

/// A palette owns its query, never the invoking composer or pane focus.
#[derive(Clone, Debug, Default)]
pub(crate) struct State {
    editor: Editor,
    selected: Option<usize>,
    selected_key: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Intent {
    Close,
    Complete(usize),
    Activate(usize),
    /// A newly typed nonempty query has no result. Adapters may pass explicit
    /// command text to an existing dispatcher; a disappeared selection never
    /// produces this intent.
    Unmatched,
    Changed,
    Handled,
}

pub(crate) fn is_open_key(key: &KeyEvent) -> bool {
    key.code == KeyCode::F(1)
        || (key
            .modifiers
            .contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT)
            && matches!(key.code, KeyCode::Char('P' | 'p')))
}

impl State {
    pub(crate) fn text(&self) -> &str {
        self.editor.text()
    }
    pub(crate) fn selected(&self, len: usize) -> Option<usize> {
        self.selected
            .filter(|_| len > 0)
            .map(|index| index.min(len - 1))
    }
    #[cfg(any(feature = "workspace", test))]
    pub(crate) fn select(&mut self, index: usize) {
        self.selected = Some(index);
        self.selected_key = None;
    }
    /// Bind to provider identity after a deliberate selection/query change;
    /// same-query publications relocate that identity or explicitly disarm it.
    pub(crate) fn reconcile(&mut self, keys: &[String]) {
        if let Some(key) = self.selected_key.as_ref() {
            self.selected = keys.iter().position(|candidate| candidate == key);
            if self.selected.is_none() {
                self.selected_key = None;
            }
        } else if let Some(index) = self.selected {
            self.selected_key = keys.get(index).cloned();
        }
    }
    pub(crate) fn set(&mut self, text: impl Into<String>) -> Result<(), aw_editor::EditorError> {
        self.editor.set(text)?;
        self.selected = (!parse(self.text()).text.trim().is_empty()).then_some(0);
        self.selected_key = None;
        Ok(())
    }
    pub(crate) fn paste(&mut self, text: &str) -> Result<(), aw_editor::EditorError> {
        // Bracketed paste cannot create a multiline command or mutate the composer.
        let text = text.replace(['\n', '\r', '\t'], " ");
        self.editor.insert(&text)?;
        self.selected = (!parse(self.text()).text.trim().is_empty()).then_some(0);
        self.selected_key = None;
        Ok(())
    }
    pub(crate) fn category(&mut self, category: Category) -> Result<(), aw_editor::EditorError> {
        let next = (parse(self.text()).category != Some(category)).then_some(category);
        self.set(with_category(self.text(), next))
    }
    pub(crate) fn key(
        &mut self,
        key: KeyEvent,
        len: usize,
    ) -> Result<Intent, aw_editor::EditorError> {
        if is_open_key(&key) {
            return Ok(Intent::Handled);
        }
        if key.modifiers.contains(KeyModifiers::ALT) {
            if let KeyCode::Char(digit @ '1'..='7') = key.code {
                self.category(Category::ALL[(digit as u8 - b'1') as usize])?;
                return Ok(Intent::Changed);
            }
        }
        match key.code {
            KeyCode::Esc => Ok(Intent::Close),
            KeyCode::Up | KeyCode::Down => {
                self.selected_key = None;
                self.selected = if len == 0 {
                    None
                } else {
                    Some(match self.selected(len) {
                        Some(index) => step(index, len, key.code == KeyCode::Down),
                        None if key.code == KeyCode::Up => len - 1,
                        None => 0,
                    })
                };
                Ok(Intent::Handled)
            }
            KeyCode::Tab => Ok(self
                .selected(len)
                .map(Intent::Complete)
                .unwrap_or(Intent::Handled)),
            _ if is_plain_enter(&key) => {
                Ok(self.selected(len).map(Intent::Activate).unwrap_or_else(|| {
                    if len == 0
                        && self.selected.is_some()
                        && !parse(self.text()).text.trim().is_empty()
                    {
                        Intent::Unmatched
                    } else {
                        Intent::Handled
                    }
                }))
            }
            _ => {
                let before = self.text().to_owned();
                edit_key(&mut self.editor, key, EditMode::SingleLine)?;
                if self.text() != before {
                    self.selected = (!parse(self.text()).text.trim().is_empty()).then_some(0);
                    self.selected_key = None;
                    Ok(Intent::Changed)
                } else {
                    Ok(Intent::Handled)
                }
            }
        }
    }
}

pub(crate) struct Row<'a> {
    pub(crate) category: Category,
    pub(crate) label: &'a str,
    pub(crate) description: &'a str,
    pub(crate) shortcut: &'a str,
}

#[derive(Default)]
pub(crate) struct Geometry {
    pub(crate) categories: Vec<(Rect, Category)>,
    pub(crate) rows: Vec<(Rect, usize)>,
    pub(crate) cursor: Option<(u16, u16)>,
}

/// Draw only inside the caller's owned viewport. Painted rectangles are also
/// the mouse targets; the adapter retains all command and navigation authority.
pub(crate) fn draw(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &State,
    rows: &[Row<'_>],
    ui: &aw::config::UiConfig,
    empty: &str,
) -> Geometry {
    let mut geometry = Geometry::default();
    let colors = super::theme::palette_for(ui);
    let width = area
        .width
        .saturating_sub(if area.width > 12 { 4 } else { 0 })
        .min(104);
    let height = area
        .height
        .saturating_sub(if area.height > 12 { 4 } else { 0 })
        .min(24);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .style(colors.menu_style())
        .border_style(Style::default().fg(colors.muted))
        .title(" Command Palette ");
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    if inner.width < 2 || inner.height < 2 {
        return geometry;
    }
    let input = state.editor.view(inner.width.saturating_sub(2), 1);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("› ", Style::default().fg(colors.accent)),
            Span::raw(input.lines.join("")),
        ])),
        Rect { height: 1, ..inner },
    );
    geometry.cursor = Some((
        inner
            .x
            .saturating_add(2)
            .saturating_add(input.cursor_x)
            .min(inner.right().saturating_sub(1)),
        inner.y,
    ));
    if let Some(cursor) = geometry.cursor {
        frame.set_cursor_position(cursor);
    }
    if inner.height < 5 {
        return geometry;
    }
    let full_width: usize = Category::ALL
        .iter()
        .map(|category| category.label().len() + 3)
        .sum();
    let full = full_width <= usize::from(inner.width);
    let active = parse(state.text()).category;
    let mut x = inner.x;
    for category in Category::ALL {
        let label = if full {
            format!("{} {} ", category.prefix(), category.label())
        } else {
            format!(" {} ", category.prefix())
        };
        let width = (label.len() as u16).min(inner.right().saturating_sub(x));
        if width == 0 {
            break;
        }
        let rect = Rect::new(x, inner.y + 2, width, 1);
        let style = if active == Some(category) {
            colors.menu_selected_style().add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(colors.muted)
        };
        frame.render_widget(Paragraph::new(Span::styled(label, style)), rect);
        geometry.categories.push((rect, category));
        x += width;
    }
    let list = Rect {
        y: inner.y + 4,
        height: inner.height.saturating_sub(5),
        ..inner
    };
    let capacity = usize::from(list.height);
    let selected = state.selected(rows.len());
    let start = selected.map_or(0, |index| (index + 1).saturating_sub(capacity));
    let end = start.saturating_add(capacity).min(rows.len());
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(empty).style(Style::default().fg(colors.muted)),
            list,
        );
    } else if capacity > 0 {
        let items = rows[start..end]
            .iter()
            .map(|row| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{} ", row.category.prefix()),
                        Style::default().fg(row.category.color()),
                    ),
                    Span::styled(
                        row.label,
                        Style::default()
                            .fg(colors.foreground)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("  {}", row.shortcut),
                        Style::default().fg(colors.muted),
                    ),
                    Span::styled(
                        format!("  — {}", row.description),
                        Style::default().fg(colors.faint),
                    ),
                ]))
            })
            .collect::<Vec<_>>();
        let mut selection = ListState::default().with_selected(selected.map(|index| index - start));
        frame.render_stateful_widget(
            List::new(items)
                .style(colors.menu_style())
                .highlight_style(colors.menu_selected_style()),
            list,
            &mut selection,
        );
        for (offset, index) in (start..end).enumerate() {
            geometry.rows.push((
                Rect::new(list.x, list.y + offset as u16, list.width, 1),
                index,
            ));
        }
    }
    frame.render_widget(
        Paragraph::new(format!(
            "{} results · ↑↓ choose · Enter open · Alt1–7 category · Esc close",
            rows.len()
        ))
        .style(Style::default().fg(colors.muted)),
        Rect {
            y: inner.bottom() - 1,
            height: 1,
            ..inner
        },
    );
    geometry
}

#[derive(Clone, Debug)]
enum MinimalTarget {
    Settings,
    Keymap,
    Thinking,
    Tools,
    Current,
    Slash(String),
}
struct MinimalItem {
    category: Category,
    label: String,
    description: String,
    shortcut: &'static str,
    target: MinimalTarget,
}

fn minimal_items(context: &super::inline::ComposerContext<'_>, query: &str) -> Vec<MinimalItem> {
    let parsed = parse(query);
    let include = |category| parsed.category.is_none_or(|active| active == category);
    let mut items = Vec::new();
    if include(Category::Actions) {
        for (label, description, shortcut, target) in [
            (
                "Settings",
                "Presentation preferences",
                "",
                MinimalTarget::Settings,
            ),
            (
                "Keyboard shortcuts",
                "Show the existing shortcut summary",
                "",
                MinimalTarget::Keymap,
            ),
            (
                "Toggle thinking",
                "Expand or collapse thinking activity",
                "F6",
                MinimalTarget::Thinking,
            ),
            (
                "Toggle tool details",
                "Expand or collapse tool details",
                "F7",
                MinimalTarget::Tools,
            ),
            (
                "Focus current Agent",
                "Return to this Agent's composer",
                "",
                MinimalTarget::Current,
            ),
        ] {
            if label.to_lowercase().contains(&parsed.text.to_lowercase()) {
                items.push(MinimalItem {
                    category: Category::Actions,
                    label: label.into(),
                    description: description.into(),
                    shortcut,
                    target,
                });
            }
        }
        if cfg!(feature = "workspace") && "open workspace".contains(&parsed.text.to_lowercase()) {
            items.push(MinimalItem {
                category: Category::Actions,
                label: "Open Workspace".into(),
                description: "Open the existing fullscreen Workspace".into(),
                shortcut: "",
                target: MinimalTarget::Slash("workspace".into()),
            });
        }
    }
    if include(Category::Commands) {
        for spec in super::commands::slash_matches(&format!("/{}", parsed.text), false) {
            items.push(MinimalItem {
                category: Category::Commands,
                label: format!("/{}", spec.name),
                description: spec.description.into(),
                shortcut: "",
                target: MinimalTarget::Slash(if spec.supports_inline_args {
                    format!("{} ", spec.name)
                } else {
                    spec.name.into()
                }),
            });
        }
    }
    for (category, label, description) in [
        (
            Category::Agents,
            format!("{} · {}", context.agent, context.session),
            format!("Current Agent · branch {}", context.branch),
        ),
        (
            Category::Sessions,
            context.session.to_owned(),
            format!("Current session · branch {}", context.branch),
        ),
    ] {
        if include(category)
            && format!("{label} {description}")
                .to_lowercase()
                .contains(&parsed.text.to_lowercase())
        {
            items.push(MinimalItem {
                category,
                label,
                description,
                shortcut: "",
                target: MinimalTarget::Current,
            });
        }
    }
    items
}

pub(crate) fn draw_minimal(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &State,
    context: &super::inline::ComposerContext<'_>,
    ui: &aw::config::UiConfig,
) -> Geometry {
    let items = minimal_items(context, state.text());
    let rows = items
        .iter()
        .map(|item| Row {
            category: item.category,
            label: &item.label,
            description: &item.description,
            shortcut: item.shortcut,
        })
        .collect::<Vec<_>>();
    let empty = match parse(state.text()).category {
        Some(Category::Files) => "File navigation is available in Workspace",
        Some(Category::Windows) => "Window navigation is available in Workspace",
        Some(Category::Terminals) => "Terminal navigation is available in Workspace",
        _ => "No matching commands or current Agent",
    };
    draw(frame, area, state, &rows, ui, empty)
}

pub(crate) enum MinimalInput {
    Pass,
    Handled,
    Command(String),
}

/// Local presentation actions stay here; host commands leave through the
/// existing ReadOutcome dispatch boundary, never a second command dispatcher.
#[allow(clippy::too_many_arguments)]
pub(crate) fn minimal_key(
    menu: &mut Option<State>,
    key: KeyEvent,
    context: &super::inline::ComposerContext<'_>,
    busy: bool,
    ui: &mut aw::config::UiConfig,
    settings: &mut Option<super::settings::SettingsState>,
    keymap: &mut bool,
    notice: &mut String,
) -> aw::Result<MinimalInput> {
    if is_open_key(&key) {
        menu.get_or_insert_with(State::default);
        notice.clear();
        return Ok(MinimalInput::Handled);
    }
    let Some(state) = menu.as_mut() else {
        return Ok(MinimalInput::Pass);
    };
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        if busy {
            return Ok(MinimalInput::Pass);
        }
        *menu = None;
        return Ok(MinimalInput::Handled);
    }
    let items = minimal_items(context, state.text());
    state.reconcile(
        &items
            .iter()
            .map(|item| format!("{}:{:?}", item.category.prefix(), item.target))
            .collect::<Vec<_>>(),
    );
    let intent = state
        .key(key, items.len())
        .map_err(|error| aw::Error::Config(error.to_string()))?;
    match intent {
        Intent::Close => *menu = None,
        Intent::Activate(index) | Intent::Complete(index) => {
            let item = &items[index];
            if let MinimalTarget::Slash(command) = &item.target {
                let resolved = resolve_slash(parse(state.text()).text, command);
                if matches!(intent, Intent::Complete(_))
                    || (command.ends_with(' ') && !resolved.has_args)
                {
                    state
                        .set(format!(
                            "/{}",
                            if resolved.has_args {
                                resolved.command
                            } else {
                                command.clone()
                            }
                        ))
                        .map_err(|error| aw::Error::Config(error.to_string()))?;
                    return Ok(MinimalInput::Handled);
                }
                if busy && resolved.command != "settings" {
                    *notice = "This command is available after the current turn settles; no command queued".into();
                    return Ok(MinimalInput::Handled);
                }
                match resolved.command.as_str() {
                    "settings" => {
                        *settings = Some(super::settings::SettingsState::default());
                        *keymap = false;
                    }
                    _ => {
                        *menu = None;
                        return Ok(MinimalInput::Command(format!("/{}", resolved.command)));
                    }
                }
            } else if matches!(intent, Intent::Activate(_)) {
                match item.target {
                    MinimalTarget::Settings => {
                        *settings = Some(super::settings::SettingsState::default());
                        *keymap = false;
                    }
                    MinimalTarget::Keymap => {
                        *settings = None;
                        *keymap = true;
                    }
                    MinimalTarget::Thinking => ui.show_thinking = !ui.show_thinking,
                    MinimalTarget::Tools => ui.show_tool_calls = !ui.show_tool_calls,
                    MinimalTarget::Current => {
                        *settings = None;
                        *keymap = false;
                    }
                    MinimalTarget::Slash(_) => unreachable!(),
                }
            } else {
                return Ok(MinimalInput::Handled);
            }
            *menu = None;
        }
        _ => {}
    }
    Ok(MinimalInput::Handled)
}

#[cfg(test)]
mod behavior_tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn context() -> super::super::inline::ComposerContext<'static> {
        super::super::inline::ComposerContext {
            resources: Default::default(),
            cwd: "/fixture",
            profile: "fixture",
            provider: "fixture",
            effort: "",
            model: "fixture",
            agent: "work",
            branch: "work",
            tools: "off",
            session: "session-17",
            state: "running",
            session_tps: None,
        }
    }
    #[derive(Default)]
    struct Harness {
        menu: Option<State>,
        ui: aw::config::UiConfig,
        settings: Option<super::super::settings::SettingsState>,
        keymap: bool,
        notice: String,
        busy: bool,
    }
    impl Harness {
        fn key(&mut self, key: KeyEvent) -> MinimalInput {
            minimal_key(
                &mut self.menu,
                key,
                &context(),
                self.busy,
                &mut self.ui,
                &mut self.settings,
                &mut self.keymap,
                &mut self.notice,
            )
            .unwrap()
        }
        fn query(&mut self, text: &str) {
            self.menu.as_mut().unwrap().set(text).unwrap();
        }
    }

    #[test]
    fn empty_palette_requires_deliberate_selection_and_wraps() {
        let mut state = State::default();
        assert_eq!(state.key(key(KeyCode::Enter), 3).unwrap(), Intent::Handled);
        assert_eq!(state.key(key(KeyCode::Tab), 3).unwrap(), Intent::Handled);
        state.key(key(KeyCode::Up), 3).unwrap();
        assert_eq!(state.selected(3), Some(2));
        state.key(key(KeyCode::Down), 3).unwrap();
        assert_eq!(state.selected(3), Some(0));
        assert_eq!(
            state.key(key(KeyCode::Enter), 3).unwrap(),
            Intent::Activate(0)
        );
        state.set("/").unwrap();
        assert_eq!(state.selected(3), None);
        state.set("/status").unwrap();
        assert_eq!(state.selected(3), Some(0));
        state.key(key(KeyCode::Down), 0).unwrap();
        assert_eq!(state.selected(0), None);
        state.select(99);
        assert_eq!(state.selected(2), Some(1));
        state.set("quit-discard").unwrap();
        assert_eq!(
            state.key(key(KeyCode::Enter), 0).unwrap(),
            Intent::Unmatched
        );
        state.reconcile(&["actual-destination".into()]);
        state.reconcile(&[]);
        assert_eq!(state.key(key(KeyCode::Enter), 0).unwrap(), Intent::Handled);
        assert_eq!(
            state
                .key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT), 2)
                .unwrap(),
            Intent::Handled
        );
    }

    #[test]
    fn category_rewrite_and_repeated_chord_preserve_query_and_caret() {
        let mut state = State::default();
        state.set("/think high").unwrap();
        state
            .key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::ALT), 1)
            .unwrap();
        assert_eq!(state.text(), ">think high");
        state
            .key(KeyEvent::new(KeyCode::Char('7'), KeyModifiers::ALT), 1)
            .unwrap();
        assert_eq!(state.text(), "&think high");
        state.key(key(KeyCode::Left), 1).unwrap();
        let before = state.editor.clone();
        state.key(key(KeyCode::F(1)), 1).unwrap();
        state
            .key(
                KeyEvent::new(
                    KeyCode::Char('P'),
                    KeyModifiers::CONTROL | KeyModifiers::SHIFT,
                ),
                1,
            )
            .unwrap();
        assert_eq!(state.editor, before);
        state.category(Category::Agents).unwrap();
        assert_eq!(state.text(), "think high");
        assert!(!is_open_key(&KeyEvent::new(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL
        )));
        assert!(!is_open_key(&KeyEvent::new(
            KeyCode::Char('n'),
            KeyModifiers::CONTROL
        )));
    }

    #[test]
    fn minimal_cancel_restores_nested_view_and_exact_draft_and_caret() {
        let mut draft = Editor::default();
        draft.set("unsent 🐦 draft").unwrap();
        draft.left();
        draft.left();
        let original = draft.clone();
        let mut h = Harness {
            settings: Some(Default::default()),
            ..Default::default()
        };
        h.key(key(KeyCode::F(1)));
        h.query("/model");
        h.menu.as_mut().unwrap().paste("\n界\tfile").unwrap();
        h.key(key(KeyCode::Esc));
        assert!(h.menu.is_none());
        assert!(h.settings.is_some());
        assert!(!h.keymap);
        assert_eq!(draft, original);
        let restored =
            super::super::inline::prompt_editor(draft.text().into(), Some(draft)).unwrap();
        assert_eq!(restored, original);
        let replacement =
            super::super::inline::prompt_editor("replacement".into(), Some(restored)).unwrap();
        assert_eq!(replacement.text(), "replacement");
        assert_eq!(replacement.cursor(), "replacement".len());
        h.key(key(KeyCode::F(1)));
        assert_eq!(h.menu.as_ref().unwrap().text(), "");
        assert_eq!(h.menu.as_ref().unwrap().selected(5), None);
    }

    #[test]
    fn minimal_busy_commands_never_queue_and_current_identity_only_focuses() {
        let mut h = Harness {
            busy: true,
            ..Default::default()
        };
        h.key(key(KeyCode::F(1)));
        h.query("/new");
        assert!(matches!(h.key(key(KeyCode::Enter)), MinimalInput::Handled));
        assert!(h.notice.contains("no command queued"));
        assert_eq!(h.menu.as_ref().unwrap().text(), "/new");
        assert!(matches!(
            h.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            MinimalInput::Pass
        ));
        h.query(">Toggle thinking");
        let before = h.ui.show_thinking;
        assert!(matches!(h.key(key(KeyCode::Enter)), MinimalInput::Handled));
        assert_ne!(h.ui.show_thinking, before);
        h.settings = Some(Default::default());
        h.keymap = true;
        h.key(key(KeyCode::F(1)));
        h.query("&session-17");
        assert!(matches!(h.key(key(KeyCode::Enter)), MinimalInput::Handled));
        assert!(h.menu.is_none());
        assert!(h.settings.is_none());
        assert!(!h.keymap);
        h.busy = false;
        h.key(key(KeyCode::F(1)));
        h.query("/status");
        assert!(
            matches!(h.key(key(KeyCode::Enter)),MinimalInput::Command(command) if command=="/status")
        );
    }

    #[test]
    fn unavailable_categories_have_no_executable_placeholder() {
        for category in [Category::Files, Category::Windows, Category::Terminals] {
            let mut h = Harness::default();
            h.key(key(KeyCode::F(1)));
            h.query(&category.prefix().to_string());
            assert!(minimal_items(&context(), h.menu.as_ref().unwrap().text()).is_empty());
            h.key(key(KeyCode::Down));
            assert!(matches!(h.key(key(KeyCode::Enter)), MinimalInput::Handled));
            assert!(h.menu.is_some());
        }
    }

    #[test]
    fn palette_paint_and_hits_stay_inside_owned_viewport_at_all_sizes() {
        let mut state = State::default();
        state.set("界e\u{301}👨‍👩‍👧‍👦").unwrap();
        let rows = (0..30)
            .map(|_| Row {
                category: Category::Files,
                label: "界e\u{301}👨‍👩‍👧‍👦 file",
                description: "bounded",
                shortcut: "",
            })
            .collect::<Vec<_>>();
        state.select(29);
        for (width, height) in [(1, 1), (2, 2), (7, 8), (21, 12), (80, 28), (120, 36)] {
            let mut terminal = Terminal::new(TestBackend::new(width + 4, height + 4)).unwrap();
            let owned = Rect::new(2, 2, width, height);
            let mut geometry = Geometry::default();
            terminal
                .draw(|frame| {
                    geometry = draw(
                        frame,
                        owned,
                        &state,
                        &rows,
                        &Default::default(),
                        "Unavailable",
                    )
                })
                .unwrap();
            for (rect, _) in &geometry.rows {
                assert!(owned.contains((rect.x, rect.y).into()));
                assert!(rect.right() <= owned.right());
                assert!(rect.bottom() <= owned.bottom());
            }
            for (rect, category) in &geometry.categories {
                assert!(owned.contains((rect.x, rect.y).into()));
                let text = (rect.x..rect.right())
                    .map(|x| terminal.backend().buffer()[(x, rect.y)].symbol())
                    .collect::<String>();
                assert!(text.contains(category.prefix()));
            }
            if width >= 80 {
                assert_eq!(geometry.categories.len(), 7);
                assert_eq!(geometry.rows.last().map(|(_, index)| *index), Some(29));
            }
            if let Some((x, y)) = geometry.cursor {
                assert!(owned.contains((x, y).into()));
            }
            for x in 0..width + 4 {
                assert_eq!(terminal.backend().buffer()[(x, 0)].symbol(), " ");
            }
        }
    }
}
