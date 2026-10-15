#[cfg(test)]
use super::editor::{edit_key, EditMode};
use super::{
    commands::{slash_matches, INLINE_KEYMAP_POPUP},
    composer::{Action as ComposerAction, Interaction as ComposerInteraction},
    extensions, palette as command_palette,
    settings::{SettingsInput, SettingsState},
    status::StatusContext,
    terminal_session::{Surface, TerminalSession},
};
use aw::{Error, Result};
use aw_editor::Editor;
use aw_ui_extension::Slot;
#[cfg(test)]
use crossterm::event::KeyEvent;
use crossterm::{
    cursor::MoveTo,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{size as terminal_size, Clear as TerminalClear, ClearType},
};

use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Widget, Wrap},
    Frame, Terminal, TerminalOptions, Viewport,
};
use std::{
    io::{self, IsTerminal, Write},
    sync::mpsc,
    time::{Duration, Instant},
};

pub enum ReadOutcome {
    PaletteCommand {
        command: String,
        draft: Editor,
        ui: aw::config::UiConfig,
    },
    Quit {
        ui: aw::config::UiConfig,
    },
    Submit {
        text: String,
        ui: aw::config::UiConfig,
    },
    UiChanged {
        draft: Editor,
        ui: aw::config::UiConfig,
    },
    ConfigureProvider {
        draft: Editor,
        ui: aw::config::UiConfig,
    },
}

/// Restore an invoking editor only when the host kept that exact draft. A
/// deliberate host draft replacement cannot inherit stale caret/undo state.
pub fn prompt_editor(text: String, preserved: Option<Editor>) -> Result<Editor> {
    if let Some(editor) = preserved.filter(|editor| editor.text() == text) {
        return Ok(editor);
    }
    let mut editor = Editor::default();
    editor
        .set(text)
        .map_err(|error| Error::Config(error.to_string()))?;
    Ok(editor)
}

struct DeferredFocus {
    settings: Option<SettingsState>,
    keymap: bool,
}

const DOUBLE_CTRL_C_WINDOW: Duration = Duration::from_secs(1);

pub struct OwnedPresenter {
    mode: aw::config::TuiMode,
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    _session: TerminalSession,
    notice: String,
    command_palette: Option<command_palette::State>,
    deferred_focus: Option<DeferredFocus>,
    inline_height: u16,
    inline_origin_y: u16,
    inline_bottom_saturated: bool,
    inline_reanchor: bool,
    /// Compatibility fallback for PTYs/terminals that cannot answer CPR.
    /// Real overflow-to-scrollback semantics require Ratatui Viewport::Inline.
    inline_fixed_fallback: bool,
    inline_context_key: Option<String>,
    inline_baseline_turns: usize,
    inline_display_start: usize,
    inline_header_visible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InlineEviction {
    Fits,
    HideHeader,
    EvictTurn { index: usize, commit: bool },
    Saturate,
}

fn inline_eviction(
    required: u16,
    physical_height: u16,
    header_visible: bool,
    display_start: usize,
    turns_len: usize,
    baseline_turns: usize,
) -> InlineEviction {
    if required <= physical_height {
        InlineEviction::Fits
    } else if header_visible {
        InlineEviction::HideHeader
    } else if display_start < turns_len {
        InlineEviction::EvictTurn {
            index: display_start,
            commit: display_start >= baseline_turns,
        }
    } else {
        InlineEviction::Saturate
    }
}

impl OwnedPresenter {
    pub fn enter(mode: aw::config::TuiMode) -> Result<Self> {
        if !matches!(
            mode,
            aw::config::TuiMode::Inline | aw::config::TuiMode::Fullscreen
        ) {
            return Err(Error::Config(
                "OwnedPresenter supports only inline or fullscreen Agent modes".into(),
            ));
        }
        let surface = match mode {
            aw::config::TuiMode::Inline => Surface::PrimaryOwned,
            aw::config::TuiMode::Fullscreen => Surface::Alternate { mouse: false },
            aw::config::TuiMode::Scrollback => unreachable!("validated above"),
        };
        let session = TerminalSession::enter(surface)?;
        let (mut terminal, inline_fixed_fallback) = if mode == aw::config::TuiMode::Inline {
            owned_inline_terminal(1)?
        } else {
            (Terminal::new(CrosstermBackend::new(io::stdout()))?, false)
        };
        let inline_origin_y = terminal.get_frame().area().y;
        if mode == aw::config::TuiMode::Inline {
            session.set_primary_exit_row(inline_origin_y);
        }
        Ok(Self {
            mode,
            terminal,
            _session: session,
            notice: String::new(),
            command_palette: None,
            deferred_focus: None,
            inline_height: 1,
            inline_origin_y,
            inline_bottom_saturated: false,
            inline_reanchor: false,
            inline_fixed_fallback,
            inline_context_key: None,
            inline_baseline_turns: 0,
            inline_display_start: 0,
            inline_header_visible: false,
        })
    }

    pub fn mode(&self) -> aw::config::TuiMode {
        self.mode
    }
}

pub struct ComposerContext<'a> {
    pub resources: super::agent_resources::LoadedResourceSummary,
    pub cwd: &'a str,
    pub profile: &'a str,
    pub provider: &'a str,
    pub effort: &'a str,
    pub model: &'a str,
    pub agent: &'a str,
    pub branch: &'a str,
    pub tools: &'a str,
    pub session: &'a str,
    pub state: &'a str,
    pub session_tps: Option<f64>,
}

#[derive(Default)]
struct LiveTurn {
    thinking: bool,
    answer: String,
    tools: Vec<aw_agent_core::ToolRecord>,
    first_delta: Option<Duration>,
    elapsed: Duration,
}

impl LiveTurn {
    fn event(&mut self, event: &aw::AgentEvent, started: Instant) {
        match event {
            aw::AgentEvent::Started { .. } => self.thinking = true,
            aw::AgentEvent::Delta { text, .. } => {
                self.thinking = false;
                if self.first_delta.is_none() {
                    self.first_delta = Some(started.elapsed());
                }
                if self.answer.len().saturating_add(text.len()) <= aw::config::MAX_OUTPUT_BYTES {
                    self.answer.push_str(text);
                }
            }
            aw::AgentEvent::ToolStarted { record, .. }
            | aw::AgentEvent::ToolFinished { record, .. } => {
                self.thinking = false;
                if let Some(existing) = self
                    .tools
                    .iter_mut()
                    .find(|item| item.call.id == record.call.id)
                {
                    *existing = record.clone();
                } else if self.tools.len() < 32 {
                    self.tools.push(record.clone());
                }
            }
            aw::AgentEvent::InvocationFinished { .. }
            | aw::AgentEvent::Finished { .. }
            | aw::AgentEvent::Rejected { .. } => self.thinking = false,
        }
    }
}

pub struct StreamOutcome {
    pub turn: Result<aw::conversation::Turn>,
    pub ui: aw::config::UiConfig,
    pub draft: Editor,
}

fn live_lines(live: &LiveTurn, ui: &aw::config::UiConfig, width: usize) -> Vec<Line<'static>> {
    let palette = crate::tui::theme::palette_for(ui);
    let mut lines = Vec::new();
    if !live.answer.trim().is_empty() {
        let safe = aw::presentation::safe_text(aw::presentation::tail_text(&live.answer, 4096));
        lines.extend(
            safe.lines()
                .rev()
                .take(5)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|line| Line::styled(line.to_owned(), palette.text_style())),
        );
    }
    for record in live.tools.iter().rev().take(2).rev() {
        let style = palette.tool_style(aw::presentation::tool_tone(Some(record)));
        let mut block = Vec::new();
        if ui.show_tool_calls {
            block.push(aw::presentation::tool_title(record));
            if let Some(output) = &record.output {
                let safe = crate::terminal::safe_text(&output.content);
                block.extend(
                    safe.lines()
                        .rev()
                        .take(2)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .map(str::to_owned),
                );
            }
        } else {
            block.push(format!(
                "tool {} · {} · hidden",
                record.call.name,
                format!("{:?}", record.status).to_ascii_lowercase()
            ));
        }
        lines.extend(super::agent_chrome::filled_block(block, style, width));
    }
    lines
}

const AUXILIARY_VIEW_COLOR: Color = Color::Rgb(255, 133, 71);
const COMPOSER_PALETTE: [Color; 6] = [
    Color::Rgb(194, 137, 255),
    Color::Rgb(96, 183, 134),
    Color::Rgb(94, 149, 255),
    Color::Rgb(232, 177, 95),
    Color::Rgb(223, 119, 169),
    Color::Rgb(75, 190, 180),
];

fn status_context(context: &ComposerContext<'_>) -> StatusContext {
    StatusContext {
        model: context.model.to_owned(),
        profile: context.profile.to_owned(),
        provider: context.provider.to_owned(),
        effort: context.effort.to_owned(),
        directory: context.cwd.to_owned(),
        agent: context.agent.to_owned(),
        branch: context.branch.to_owned(),
        tools: context.tools.to_owned(),
        session: context.session.to_owned(),
        state: context.state.to_owned(),
        session_tps: context.session_tps,
    }
}

fn composer_accent(context: &ComposerContext<'_>) -> Color {
    let mut hash = 0xcbf29ce484222325_u64;
    let mut mix = |value: &str| {
        for byte in value.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    };
    mix(context.session);
    if context.session == "memory" {
        mix(context.cwd);
        mix(context.profile);
        mix(&std::process::id().to_string());
    }
    COMPOSER_PALETTE[(hash as usize) % COMPOSER_PALETTE.len()]
}

fn extension_lines(
    ui: &aw::config::UiConfig,
    context: &ComposerContext<'_>,
    width: u16,
) -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    let status = status_context(context);
    (
        extensions::project_status(Slot::AboveComposer, ui, &status, None, width),
        extensions::project_status(Slot::BelowComposer, ui, &status, None, width),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FlowRequest {
    height: u16,
    transcript_rows: usize,
    above_rows: u16,
    notice_rows: u16,
    popup_rows: u16,
    composer_rows: u16,
    below_rows: u16,
    natural_flow: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FlowLayout {
    transcript: u16,
    above: u16,
    notice: u16,
    popup: u16,
    composer: u16,
    below: u16,
    trailing: u16,
}

fn visual_rows(lines: &[Line<'_>], width: usize) -> usize {
    let width = width.max(1);
    lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum()
}

fn flow_layout(request: FlowRequest) -> FlowLayout {
    let FlowRequest {
        height,
        transcript_rows,
        above_rows,
        notice_rows,
        popup_rows,
        composer_rows: requested_composer_rows,
        below_rows,
        natural_flow,
    } = request;
    let non_composer = above_rows
        .saturating_add(notice_rows)
        .saturating_add(popup_rows)
        .saturating_add(below_rows);
    let composer = requested_composer_rows
        .min(height.saturating_sub(non_composer))
        .max(u16::from(height > non_composer));
    let chrome = non_composer.saturating_add(composer);
    let available_transcript = height.saturating_sub(chrome);
    let transcript = if natural_flow {
        transcript_rows
            .min(usize::from(available_transcript))
            .min(usize::from(u16::MAX)) as u16
    } else {
        available_transcript
    };
    let used = chrome.saturating_add(transcript);
    FlowLayout {
        transcript,
        above: above_rows,
        notice: notice_rows,
        popup: popup_rows,
        composer,
        below: below_rows,
        trailing: height.saturating_sub(used),
    }
}

pub(super) fn inline_terminal(height: u16) -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    Terminal::with_options(
        CrosstermBackend::new(io::stdout()),
        TerminalOptions {
            viewport: Viewport::Inline(height),
        },
    )
    .map_err(Into::into)
}

fn reserve_primary_fallback() -> Result<()> {
    let (_, height) = terminal_size()?;
    let mut out = io::stdout();
    execute!(out, crossterm::cursor::MoveToColumn(0))?;
    for _ in 0..height.saturating_sub(1) {
        write!(out, "\r\n")?;
    }
    out.flush()?;
    Ok(())
}

fn fixed_bottom_terminal(height: u16) -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    let (width, physical_height) = terminal_size()?;
    let height = height.clamp(1, physical_height.max(1));
    let y = physical_height.saturating_sub(height);
    Terminal::with_options(
        CrosstermBackend::new(io::stdout()),
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, y, width.max(1), height)),
        },
    )
    .map_err(Into::into)
}

fn owned_inline_terminal(height: u16) -> Result<(Terminal<CrosstermBackend<io::Stdout>>, bool)> {
    match inline_terminal(height) {
        Ok(terminal) => Ok((terminal, false)),
        Err(_) => {
            reserve_primary_fallback()?;
            fixed_bottom_terminal(height).map(|terminal| (terminal, true))
        }
    }
}

struct DrawState<'a, 'b> {
    selected: usize,
    context: &'a ComposerContext<'b>,
    ui: &'a aw::config::UiConfig,
    live: Option<&'a LiveTurn>,
    settings: Option<&'a mut SettingsState>,
    show_keymap: bool,
    command_palette: Option<&'a command_palette::State>,
    notice: &'a str,
}

#[cfg(test)]
fn draw(frame: &mut Frame<'_>, editor: &Editor, state: DrawState<'_, '_>) {
    let _ = draw_with_cursor(frame, editor, state);
}

fn draw_with_cursor(
    frame: &mut Frame<'_>,
    editor: &Editor,
    state: DrawState<'_, '_>,
) -> Option<(u16, u16)> {
    let DrawState {
        selected,
        context,
        ui,
        live,
        settings,
        show_keymap,
        command_palette,
        notice,
    } = state;
    let palette = crate::tui::theme::palette_for(ui);
    let area = frame.area();
    let mut cursor = None;
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(palette.base_style()), area);
    let in_settings = settings.is_some();
    let embedded_view = in_settings || show_keymap || command_palette.is_some();
    let matches = if embedded_view {
        Vec::new()
    } else {
        slash_matches(editor.text(), false)
    };
    let popup_rows = matches.len().min(usize::from(ui.autocomplete_max_visible)) as u16;
    let live_lines = if embedded_view {
        Vec::new()
    } else {
        live.map(|live| live_lines(live, ui, usize::from(area.width).max(1)))
            .unwrap_or_default()
    };
    let live_rows = live_lines.len().min(8) as u16;
    let (above_lines, below_lines) = extension_lines(ui, context, area.width);
    let above_rows = above_lines.len().min(8) as u16;
    let below_rows = below_lines.len().min(8) as u16;
    let notice_rows = u16::from(!notice.is_empty());
    let edit_rows = editor.text().lines().count().clamp(1, 5) as u16;
    let requested_composer = if command_palette.is_some() {
        24
    } else if in_settings {
        17
    } else if show_keymap {
        7
    } else {
        edit_rows + 2
    };
    let geometry = flow_layout(FlowRequest {
        height: area.height,
        transcript_rows: usize::from(live_rows),
        above_rows,
        notice_rows,
        popup_rows,
        composer_rows: requested_composer,
        below_rows,
        natural_flow: true,
    });
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(geometry.transcript),
            Constraint::Length(geometry.above),
            Constraint::Length(geometry.notice),
            Constraint::Length(geometry.popup),
            Constraint::Length(geometry.composer),
            Constraint::Length(geometry.below),
            Constraint::Length(geometry.trailing),
        ])
        .split(area);

    if geometry.transcript > 0 {
        let start = live_lines
            .len()
            .saturating_sub(usize::from(geometry.transcript));
        frame.render_widget(
            Paragraph::new(live_lines.into_iter().skip(start).collect::<Vec<_>>())
                .wrap(Wrap { trim: false }),
            chunks[0],
        );
    }
    if geometry.above > 0 {
        frame.render_widget(
            Paragraph::new(
                above_lines
                    .into_iter()
                    .take(usize::from(geometry.above))
                    .collect::<Vec<_>>(),
            ),
            chunks[1],
        );
    }
    if geometry.notice > 0 {
        frame.render_widget(
            Paragraph::new(Span::styled(
                notice,
                Style::default().fg(AUXILIARY_VIEW_COLOR),
            )),
            chunks[2],
        );
    }
    if geometry.popup > 0 {
        let mut list_state = ListState::default();
        list_state.select(Some(selected.min(matches.len().saturating_sub(1))));
        let rows = matches
            .iter()
            .take(usize::from(geometry.popup))
            .map(|spec| {
                ListItem::new(Line::from(vec![
                    Span::styled(spec.usage, palette.menu_usage_style()),
                    Span::styled("  ", palette.menu_style()),
                    Span::styled(spec.description, palette.menu_description_style()),
                ]))
            })
            .collect::<Vec<_>>();
        frame.render_stateful_widget(
            List::new(rows)
                .style(palette.menu_style())
                .highlight_style(palette.menu_selected_style()),
            chunks[3],
            &mut list_state,
        );
    }

    let composer = chunks[4];
    let border_color = if embedded_view {
        AUXILIARY_VIEW_COLOR
    } else {
        composer_accent(context)
    };
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(border_color));
    let mut inner = block.inner(composer);
    frame.render_widget(block, composer);
    if let Some(live) = live {
        frame.render_widget(
            Paragraph::new(super::agent_chrome::activity_border_line(
                composer.width,
                live.elapsed,
                border_color,
                ui.theme,
            )),
            Rect {
                height: 1,
                ..composer
            },
        );
    }
    let padding = u16::from(ui.editor_padding_x).min(inner.width / 2);
    inner.x = inner.x.saturating_add(padding);
    inner.width = inner.width.saturating_sub(padding.saturating_mul(2));

    if let Some(settings) = settings {
        settings.draw(frame, inner, ui);
    } else if show_keymap {
        let lines = INLINE_KEYMAP_POPUP
            .lines()
            .take(usize::from(inner.height))
            .map(|line| Line::styled(line, Style::default().fg(AUXILIARY_VIEW_COLOR)))
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines), inner);
    } else {
        let view = editor.view(inner.width, inner.height);
        if editor.is_empty() {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    super::agent_chrome::composer_placeholder(inner.width, live.is_some(), true),
                    Style::default().fg(palette.muted),
                )),
                inner,
            );
        } else {
            frame.render_widget(
                Paragraph::new(
                    view.lines
                        .iter()
                        .map(|line| Line::from(line.clone()))
                        .collect::<Vec<_>>(),
                ),
                inner,
            );
        }
        if inner.width > 0 && inner.height > 0 {
            let position = (inner.x + view.cursor_x, inner.y + view.cursor_y);
            frame.set_cursor_position(position);
            cursor = Some(position);
        }
    }

    if geometry.below > 0 {
        frame.render_widget(
            Paragraph::new(
                below_lines
                    .into_iter()
                    .take(usize::from(geometry.below))
                    .collect::<Vec<_>>(),
            ),
            chunks[5],
        );
    }
    if let Some(state) = command_palette {
        cursor = command_palette::draw_minimal(frame, composer, state, context, ui).cursor;
    }
    cursor
}

struct ScratchRowsRequest<'a, 'b> {
    editor: &'a Editor,
    context: &'a ComposerContext<'b>,
    ui: &'a aw::config::UiConfig,
    live: Option<&'a LiveTurn>,
    in_settings: bool,
    show_keymap: bool,
    in_palette: bool,
    notice: &'a str,
    width: u16,
}

fn scratch_required_rows(request: ScratchRowsRequest<'_, '_>) -> u16 {
    let ScratchRowsRequest {
        editor,
        context,
        ui,
        live,
        in_settings,
        show_keymap,
        in_palette,
        notice,
        width,
    } = request;
    let embedded_view = in_settings || show_keymap || in_palette;
    let popup_rows = if embedded_view {
        0
    } else {
        slash_matches(editor.text(), false)
            .len()
            .min(usize::from(ui.autocomplete_max_visible)) as u16
    };
    let live_rows = if embedded_view {
        0
    } else {
        live.map(|live| live_lines(live, ui, usize::from(width).max(1)).len().min(8) as u16)
            .unwrap_or(0)
    };
    let (above, below) = extension_lines(ui, context, width);
    let above_rows = above.len().min(8) as u16;
    let below_rows = below.len().min(8) as u16;
    let notice_rows = u16::from(!notice.is_empty());
    let edit_rows = editor.text().lines().count().clamp(1, 5) as u16;
    let composer_rows = if in_palette {
        24
    } else if in_settings {
        17
    } else if show_keymap {
        7
    } else {
        edit_rows + 2
    };
    live_rows
        .saturating_add(above_rows)
        .saturating_add(notice_rows)
        .saturating_add(popup_rows)
        .saturating_add(composer_rows)
        .saturating_add(below_rows)
        .max(1)
}

fn final_turn_lines(
    turn: &aw::conversation::Turn,
    stats: &super::performance::PerformanceStats,
    ui: &aw::config::UiConfig,
    width: usize,
) -> Vec<Line<'static>> {
    let palette = crate::tui::theme::palette_for(ui);
    let mut lines = Vec::new();
    let thinking_elapsed = stats.first_token.unwrap_or(stats.elapsed);
    if !thinking_elapsed.is_zero() {
        lines.push(Line::styled(
            if ui.show_thinking {
                format!("Thinking {:.1}s", thinking_elapsed.as_secs_f64())
            } else {
                "Thinking · hidden".into()
            },
            palette.thinking_style(),
        ));
        lines.push(Line::default());
    }
    let record_for = |call_id: &str| {
        turn.tool_records
            .iter()
            .find(|record| record.call.id == call_id)
    };
    let trace = turn.context_messages.iter().skip(1).collect::<Vec<_>>();
    let answer_fallback = aw::presentation::settled_answer_fallback(turn);
    let mut rendered_assistant = false;
    for message in trace {
        match message.role {
            aw_agent_core::Role::Assistant => {
                if !message.content.trim().is_empty() {
                    rendered_assistant = true;
                    lines.extend(super::markdown::render(
                        &message.content,
                        palette,
                        ui.theme,
                        width,
                    ));
                }
                for call in &message.tool_calls {
                    let record = record_for(&call.id);
                    let style = palette.tool_style(aw::presentation::tool_tone(record));
                    lines.push(Line::default());
                    let title = if ui.show_tool_calls {
                        record.map_or_else(
                            || {
                                aw::presentation::safe_text(&format!(
                                    "{}  {}",
                                    call.name, call.arguments
                                ))
                            },
                            aw::presentation::tool_title,
                        )
                    } else {
                        let status = record
                            .map(|record| format!("{:?}", record.status).to_ascii_lowercase())
                            .unwrap_or_else(|| "running".into());
                        format!("tool {} · {status} · hidden", call.name)
                    };
                    lines.extend(super::agent_chrome::filled_block(vec![title], style, width));
                }
            }
            aw_agent_core::Role::Tool if ui.show_tool_calls => {
                let record = message.tool_call_id.as_deref().and_then(record_for);
                let style = palette.tool_style(aw::presentation::tool_tone(record));
                let safe = crate::terminal::safe_text(&message.content);
                let all = safe.lines().collect::<Vec<_>>();
                const MAX_VISIBLE: usize = 12;
                let mut block = Vec::new();
                if all.len() > MAX_VISIBLE {
                    block.push(format!(
                        "… ({} earlier lines omitted)",
                        all.len() - MAX_VISIBLE
                    ));
                }
                block.extend(
                    all.iter()
                        .skip(all.len().saturating_sub(MAX_VISIBLE))
                        .map(|line| (*line).to_owned()),
                );
                lines.extend(super::agent_chrome::filled_block(block, style, width));
            }
            _ => {}
        }
    }
    if let Some(fallback) = answer_fallback {
        if rendered_assistant {
            lines.push(Line::default());
        }
        lines.extend(super::markdown::render(&fallback, palette, ui.theme, width));
        rendered_assistant = true;
    }
    if turn.outcome != aw::conversation::Outcome::Completed {
        if rendered_assistant {
            lines.push(Line::default());
        }
        let detail = turn
            .error
            .as_deref()
            .unwrap_or("model turn did not complete with visible output");
        lines.push(Line::styled(
            format!("! {}", crate::terminal::safe_text(detail)),
            palette.error_style(),
        ));
    } else if !rendered_assistant {
        lines.push(Line::styled(
            "! completed turn contained no visible assistant output",
            palette.error_style(),
        ));
    }
    let performance =
        extensions::project(Slot::AfterTurn, ui, &StatusContext::default(), Some(stats));
    if !performance.is_empty() {
        lines.push(Line::default());
        lines.extend(performance);
    }
    lines.push(Line::default());
    lines
}

fn owned_transcript_lines(
    turns: &[aw::conversation::Turn],
    pending_prompt: Option<&str>,
    live: Option<&LiveTurn>,
    resources: &super::agent_resources::LoadedResourceSummary,
    ui: &aw::config::UiConfig,
    width: usize,
    header_mode: super::agent_chrome::HeaderMode,
) -> Vec<Line<'static>> {
    let live_projection = live.map(|live| super::agent_chrome::LiveProjection {
        answer: &live.answer,
        tools: &live.tools,
    });
    let header = super::agent_chrome::AgentHeader::from_resources(resources);
    super::agent_chrome::transcript_lines(super::agent_chrome::TranscriptProjection {
        header: &header,
        header_mode,
        turns,
        pending: pending_prompt,
        live: live_projection.as_ref(),
        ui,
        width,
        padding_x: ui.output_padding,
        last_performance: None,
        highlight_code: true,
    })
}

struct OwnedDrawState<'a, 'b> {
    selected: usize,
    context: &'a ComposerContext<'b>,
    ui: &'a aw::config::UiConfig,
    turns: &'a [aw::conversation::Turn],
    header_mode: super::agent_chrome::HeaderMode,
    pending_prompt: Option<&'a str>,
    live: Option<&'a LiveTurn>,
    settings: Option<&'a mut SettingsState>,
    show_keymap: bool,
    command_palette: Option<&'a command_palette::State>,
    notice: &'a str,
}

struct OwnedRowsRequest<'a, 'b> {
    editor: &'a Editor,
    context: &'a ComposerContext<'b>,
    ui: &'a aw::config::UiConfig,
    turns: &'a [aw::conversation::Turn],
    header_mode: super::agent_chrome::HeaderMode,
    pending_prompt: Option<&'a str>,
    live: Option<&'a LiveTurn>,
    in_settings: bool,
    show_keymap: bool,
    in_palette: bool,
    notice: &'a str,
    width: u16,
}

fn owned_required_rows(request: OwnedRowsRequest<'_, '_>) -> u16 {
    let OwnedRowsRequest {
        editor,
        context,
        ui,
        turns,
        header_mode,
        pending_prompt,
        live,
        in_settings,
        show_keymap,
        in_palette,
        notice,
        width,
    } = request;
    let embedded_view = in_settings || show_keymap || in_palette;
    let popup_rows = if embedded_view {
        0
    } else {
        slash_matches(editor.text(), false)
            .len()
            .min(usize::from(ui.autocomplete_max_visible)) as u16
    };
    let (above, below) = extension_lines(ui, context, width);
    let above_rows = above.len().min(8) as u16;
    let below_rows = below.len().min(8) as u16;
    let notice_rows = u16::from(!notice.is_empty());
    let editor_rows = editor.text().lines().count().clamp(1, 5) as u16;
    let composer_rows = if in_palette {
        24
    } else if in_settings {
        17
    } else if show_keymap {
        7
    } else {
        editor_rows.saturating_add(2)
    };
    let transcript = owned_transcript_lines(
        turns,
        pending_prompt,
        live,
        &context.resources,
        ui,
        usize::from(width).max(1),
        header_mode,
    );
    let transcript_rows =
        visual_rows(&transcript, usize::from(width).max(1)).min(usize::from(u16::MAX)) as u16;
    transcript_rows
        .saturating_add(above_rows)
        .saturating_add(notice_rows)
        .saturating_add(popup_rows)
        .saturating_add(composer_rows)
        .saturating_add(below_rows)
        .max(1)
}

fn draw_owned(
    frame: &mut Frame<'_>,
    editor: &Editor,
    state: OwnedDrawState<'_, '_>,
    natural_flow: bool,
) -> u16 {
    let OwnedDrawState {
        selected,
        context,
        ui,
        turns,
        header_mode,
        pending_prompt,
        live,
        settings,
        show_keymap,
        command_palette,
        notice,
    } = state;
    let palette = crate::tui::theme::palette_for(ui);
    let area = frame.area();
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(palette.base_style()), area);

    let in_settings = settings.is_some();
    let embedded_view = in_settings || show_keymap || command_palette.is_some();
    let matches = if embedded_view {
        Vec::new()
    } else {
        slash_matches(editor.text(), false)
    };
    let popup_rows = matches.len().min(usize::from(ui.autocomplete_max_visible)) as u16;
    let (above_lines, below_lines) = extension_lines(ui, context, area.width);
    let above_rows = above_lines.len().min(8) as u16;
    let below_rows = below_lines.len().min(8) as u16;
    let notice_rows = u16::from(!notice.is_empty());
    let edit_rows = editor.text().lines().count().clamp(1, 5) as u16;
    let requested_composer = if command_palette.is_some() {
        24
    } else if in_settings {
        17
    } else if show_keymap {
        7
    } else {
        edit_rows + 2
    };
    let width = usize::from(area.width).max(1);
    let transcript_lines = owned_transcript_lines(
        turns,
        pending_prompt,
        live,
        &context.resources,
        ui,
        width,
        header_mode,
    );
    let transcript_visual_rows = visual_rows(&transcript_lines, width);
    let geometry = flow_layout(FlowRequest {
        height: area.height,
        transcript_rows: transcript_visual_rows,
        above_rows,
        notice_rows,
        popup_rows,
        composer_rows: requested_composer,
        below_rows,
        natural_flow,
    });

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(geometry.transcript),
            Constraint::Length(geometry.above),
            Constraint::Length(geometry.notice),
            Constraint::Length(geometry.popup),
            Constraint::Length(geometry.composer),
            Constraint::Length(geometry.below),
            Constraint::Length(geometry.trailing),
        ])
        .split(area);

    if geometry.transcript > 0 {
        let visible = usize::from(geometry.transcript);
        let start = transcript_visual_rows.saturating_sub(visible);
        frame.render_widget(
            Paragraph::new(transcript_lines)
                .wrap(Wrap { trim: false })
                .scroll((start.min(usize::from(u16::MAX)) as u16, 0)),
            chunks[0],
        );
    }
    if geometry.above > 0 {
        frame.render_widget(
            Paragraph::new(
                above_lines
                    .into_iter()
                    .take(usize::from(geometry.above))
                    .collect::<Vec<_>>(),
            ),
            chunks[1],
        );
    }
    if geometry.notice > 0 {
        frame.render_widget(
            Paragraph::new(Span::styled(
                notice,
                Style::default().fg(AUXILIARY_VIEW_COLOR),
            )),
            chunks[2],
        );
    }
    if geometry.popup > 0 {
        let mut list_state = ListState::default();
        list_state.select(Some(selected.min(matches.len().saturating_sub(1))));
        let rows = matches
            .iter()
            .take(usize::from(geometry.popup))
            .map(|spec| {
                ListItem::new(Line::from(vec![
                    Span::styled(spec.usage, palette.menu_usage_style()),
                    Span::styled("  ", palette.menu_style()),
                    Span::styled(spec.description, palette.menu_description_style()),
                ]))
            })
            .collect::<Vec<_>>();
        frame.render_stateful_widget(
            List::new(rows)
                .style(palette.menu_style())
                .highlight_style(palette.menu_selected_style()),
            chunks[3],
            &mut list_state,
        );
    }

    let composer = chunks[4];
    let border_color = if embedded_view {
        AUXILIARY_VIEW_COLOR
    } else {
        composer_accent(context)
    };
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(border_color));
    let mut inner = block.inner(composer);
    frame.render_widget(block, composer);
    if let Some(live) = live {
        frame.render_widget(
            Paragraph::new(super::agent_chrome::activity_border_line(
                composer.width,
                live.elapsed,
                border_color,
                ui.theme,
            )),
            Rect {
                height: 1,
                ..composer
            },
        );
    }
    let padding = u16::from(ui.editor_padding_x).min(inner.width / 2);
    inner.x = inner.x.saturating_add(padding);
    inner.width = inner.width.saturating_sub(padding.saturating_mul(2));

    if let Some(settings) = settings {
        settings.draw(frame, inner, ui);
    } else if show_keymap {
        let lines = INLINE_KEYMAP_POPUP
            .lines()
            .take(usize::from(inner.height))
            .map(|line| Line::styled(line, Style::default().fg(AUXILIARY_VIEW_COLOR)))
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines), inner);
    } else {
        let view = editor.view(inner.width, inner.height);
        if editor.is_empty() {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    super::agent_chrome::composer_placeholder(inner.width, live.is_some(), true),
                    Style::default().fg(palette.muted),
                )),
                inner,
            );
        } else {
            frame.render_widget(
                Paragraph::new(
                    view.lines
                        .iter()
                        .map(|line| Line::from(line.clone()))
                        .collect::<Vec<_>>(),
                ),
                inner,
            );
        }
        if inner.width > 0 && inner.height > 0 {
            frame.set_cursor_position((inner.x + view.cursor_x, inner.y + view.cursor_y));
        }
    }
    if geometry.below > 0 {
        frame.render_widget(
            Paragraph::new(
                below_lines
                    .into_iter()
                    .take(usize::from(geometry.below))
                    .collect::<Vec<_>>(),
            ),
            chunks[5],
        );
    }
    if let Some(state) = command_palette {
        command_palette::draw_minimal(frame, composer, state, context, ui);
    }
    // First transient row after the settled transcript. Owned-primary exit
    // clears from here down so the invoking shell resumes immediately after
    // durable aw output rather than at the physical bottom of the terminal.
    chunks[1].y
}

impl OwnedPresenter {
    fn inline_context_key(context: &ComposerContext<'_>) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{:016x}",
            context.session,
            context.branch,
            context.resources.fingerprint()
        )
    }

    fn observe_inline_viewport(&mut self) -> Result<Rect> {
        let area = self.terminal.get_frame().area();
        self.inline_origin_y = area.y;
        let (_, physical_height) = terminal_size()?;
        self.inline_bottom_saturated |= area.bottom() >= physical_height;
        Ok(area)
    }

    fn clear_inline_viewport_from(&self, origin_y: u16) -> Result<()> {
        let (_, physical_height) = terminal_size()?;
        let y = origin_y.min(physical_height.saturating_sub(1));
        let mut out = io::stdout();
        execute!(out, MoveTo(0, y), TerminalClear(ClearType::FromCursorDown))?;
        out.flush()?;
        Ok(())
    }

    fn ensure_inline_height(&mut self, requested: u16) -> Result<()> {
        if self.mode != aw::config::TuiMode::Inline {
            return Ok(());
        }
        let (_, physical_height) = terminal_size()?;
        let desired = requested.clamp(1, physical_height.max(1));
        self.observe_inline_viewport()?;
        if desired == self.inline_height && !self.inline_reanchor {
            return Ok(());
        }

        self.clear_inline_viewport_from(self.inline_origin_y)?;
        let target_y = if self.inline_bottom_saturated || self.inline_fixed_fallback {
            physical_height.saturating_sub(desired)
        } else {
            self.inline_origin_y.min(physical_height.saturating_sub(1))
        };
        if self.inline_fixed_fallback {
            let (width, _) = terminal_size()?;
            self.terminal
                .resize(Rect::new(0, target_y, width.max(1), desired))?;
        } else {
            self.terminal.set_cursor_position((0, target_y))?;
            match inline_terminal(desired) {
                Ok(terminal) => self.terminal = terminal,
                Err(_) => {
                    self.inline_fixed_fallback = true;
                    reserve_primary_fallback()?;
                    self.terminal = fixed_bottom_terminal(desired)?;
                }
            }
        }
        self.inline_height = desired;
        self.inline_reanchor = false;
        self.observe_inline_viewport()?;
        Ok(())
    }

    fn commit_inline_turn(
        &mut self,
        turn: &aw::conversation::Turn,
        ui: &aw::config::UiConfig,
        width: u16,
    ) -> Result<()> {
        let header = super::agent_chrome::AgentHeader::default();
        let lines =
            super::agent_chrome::transcript_lines(super::agent_chrome::TranscriptProjection {
                header: &header,
                header_mode: crate::tui::agent_chrome::HeaderMode::Hidden,
                turns: std::slice::from_ref(turn),
                pending: None,
                live: None,
                ui,
                width: usize::from(width).max(1),
                padding_x: ui.output_padding,
                last_performance: None,
                highlight_code: true,
            });
        let height =
            visual_rows(&lines, usize::from(width).max(1)).clamp(1, usize::from(u16::MAX)) as u16;
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        self.terminal
            .insert_before(height, move |buffer| paragraph.render(buffer.area, buffer))?;
        self.observe_inline_viewport()?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn reconcile_inline_history(
        &mut self,
        editor: &Editor,
        context: &ComposerContext<'_>,
        turns: &[aw::conversation::Turn],
        pending_prompt: Option<&str>,
        live: Option<&LiveTurn>,
        ui: &aw::config::UiConfig,
        in_settings: bool,
        show_keymap: bool,
        in_palette: bool,
    ) -> Result<()> {
        if self.mode != aw::config::TuiMode::Inline {
            self.terminal.autoresize()?;
            return Ok(());
        }

        let context_key = Self::inline_context_key(context);
        if self.inline_context_key.as_deref() != Some(context_key.as_str())
            || turns.len() < self.inline_display_start
            || turns.len() < self.inline_baseline_turns
        {
            self.inline_context_key = Some(context_key);
            self.inline_baseline_turns = turns.len();
            self.inline_display_start = 0;
            self.inline_header_visible = false;
        }

        let (width, physical_height) = terminal_size()?;
        let physical_height = physical_height.max(1);
        loop {
            let start = self.inline_display_start.min(turns.len());
            let header_mode = if self.inline_header_visible {
                crate::tui::agent_chrome::HeaderMode::Full
            } else {
                crate::tui::agent_chrome::HeaderMode::Hidden
            };
            let required = owned_required_rows(OwnedRowsRequest {
                editor,
                context,
                ui,
                turns: &turns[start..],
                header_mode,
                pending_prompt,
                live,
                in_settings,
                show_keymap,
                in_palette,
                notice: &self.notice,
                width,
            });
            match inline_eviction(
                required,
                physical_height,
                self.inline_header_visible,
                self.inline_display_start,
                turns.len(),
                self.inline_baseline_turns,
            ) {
                InlineEviction::Fits => {
                    self.ensure_inline_height(required)?;
                    return Ok(());
                }
                InlineEviction::HideHeader => {
                    self.inline_header_visible = false;
                }
                InlineEviction::EvictTurn { index, commit } => {
                    if commit && self.inline_fixed_fallback {
                        // Fixed fallback cannot insert into terminal scrollback.
                        // Keep authoritative runtime turns in the active viewport
                        // and degrade to bounded clipping rather than pretending
                        // an external history commit occurred.
                        self.ensure_inline_height(physical_height)?;
                        return Ok(());
                    }
                    if commit {
                        self.commit_inline_turn(&turns[index], ui, width)?;
                    }
                    self.inline_display_start = self.inline_display_start.saturating_add(1);
                }
                InlineEviction::Saturate => {
                    self.ensure_inline_height(physical_height)?;
                    return Ok(());
                }
            }
        }
    }

    fn visible_inline_turns<'a>(
        &self,
        turns: &'a [aw::conversation::Turn],
    ) -> (
        &'a [aw::conversation::Turn],
        super::agent_chrome::HeaderMode,
    ) {
        if self.mode != aw::config::TuiMode::Inline {
            return (turns, crate::tui::agent_chrome::HeaderMode::Hidden);
        }
        let start = self.inline_display_start.min(turns.len());
        (
            &turns[start..],
            if self.inline_header_visible {
                crate::tui::agent_chrome::HeaderMode::Full
            } else {
                crate::tui::agent_chrome::HeaderMode::Hidden
            },
        )
    }

    pub fn set_notice(&mut self, notice: impl Into<String>) {
        self.notice = notice.into();
    }

    pub fn read_prompt(
        &mut self,
        initial: Editor,
        history: &[String],
        context: ComposerContext<'_>,
        turns: &[aw::conversation::Turn],
        mut ui: aw::config::UiConfig,
    ) -> Result<ReadOutcome> {
        let initial_ui = ui.clone();
        let mut editor = initial;
        let mut interaction = ComposerInteraction::default();
        let focus = self.deferred_focus.take();
        let mut show_keymap = focus.as_ref().is_some_and(|focus| focus.keymap);
        let mut menu = self.command_palette.take();
        let mut settings = focus.and_then(|focus| focus.settings);
        let mut quit_armed_until: Option<Instant> = None;

        loop {
            self.reconcile_inline_history(
                &editor,
                &context,
                turns,
                None,
                None,
                &ui,
                settings.is_some(),
                show_keymap,
                menu.is_some(),
            )?;
            let (visible_turns, header_mode) = self.visible_inline_turns(turns);
            let mut exit_row = 0u16;
            self.terminal.draw(|frame| {
                exit_row = draw_owned(
                    frame,
                    &editor,
                    OwnedDrawState {
                        selected: interaction.selected,
                        context: &context,
                        ui: &ui,
                        turns: visible_turns,
                        header_mode,
                        pending_prompt: None,
                        live: None,
                        settings: settings.as_mut(),
                        show_keymap,
                        command_palette: menu.as_ref(),
                        notice: &self.notice,
                    },
                    self.mode == aw::config::TuiMode::Inline,
                );
            })?;
            self._session.set_primary_exit_row(exit_row);
            let event = event::read()?;
            match event {
                Event::Paste(text) => {
                    if let Some(state) = menu.as_mut() {
                        state
                            .paste(&text)
                            .map_err(|error| Error::Config(error.to_string()))?;
                        continue;
                    }
                    quit_armed_until = None;
                    editor
                        .insert(&text)
                        .map_err(|e| Error::Config(e.to_string()))?;
                    interaction.reset_after_external_edit();
                    self.notice.clear();
                }
                Event::Resize(_, _) if self.mode == aw::config::TuiMode::Inline => {
                    self.inline_reanchor = true;
                }
                Event::Key(key)
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                {
                    match command_palette::minimal_key(
                        &mut menu,
                        key,
                        &context,
                        false,
                        &mut ui,
                        &mut settings,
                        &mut show_keymap,
                        &mut self.notice,
                    )? {
                        command_palette::MinimalInput::Pass => {}
                        command_palette::MinimalInput::Handled => continue,
                        command_palette::MinimalInput::Command(command) => {
                            return Ok(ReadOutcome::PaletteCommand {
                                command,
                                draft: editor,
                                ui,
                            })
                        }
                    }
                    if let Some(active) = settings.as_mut() {
                        match active.handle(key, &mut ui) {
                            SettingsInput::Close => {
                                if ui != initial_ui {
                                    let draft = editor;
                                    return Ok(ReadOutcome::UiChanged { draft, ui });
                                }
                                settings = None;
                                self.notice.clear();
                            }
                            SettingsInput::ProviderSetup => {
                                let draft = editor;
                                return Ok(ReadOutcome::ConfigureProvider { draft, ui });
                            }
                            SettingsInput::Handled => {}
                        }
                        continue;
                    }
                    if key.code == KeyCode::F(6) {
                        ui.show_thinking = !ui.show_thinking;
                        self.notice = format!(
                            "Thinking activity {}",
                            if ui.show_thinking {
                                "expanded"
                            } else {
                                "collapsed"
                            }
                        );
                        continue;
                    }
                    if key.code == KeyCode::F(7) {
                        ui.show_tool_calls = !ui.show_tool_calls;
                        self.notice = format!(
                            "Tool details {}",
                            if ui.show_tool_calls {
                                "expanded"
                            } else {
                                "collapsed"
                            }
                        );
                        continue;
                    }
                    let idle_ctrl_c = key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL);
                    if !idle_ctrl_c {
                        quit_armed_until = None;
                    }
                    if idle_ctrl_c {
                        let now = Instant::now();
                        if editor.is_empty()
                            && quit_armed_until.is_some_and(|deadline| now <= deadline)
                        {
                            return Ok(ReadOutcome::Quit { ui });
                        }
                        editor.clear();
                        quit_armed_until = Some(now + DOUBLE_CTRL_C_WINDOW);
                        self.notice = "Input cleared · Ctrl+C again to quit".into();
                        continue;
                    }
                    if show_keymap {
                        if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                            show_keymap = false;
                        }
                        continue;
                    }

                    match interaction.handle(&mut editor, key, history, false)? {
                        ComposerAction::Handled => {
                            self.notice.clear();
                            continue;
                        }
                        ComposerAction::OpenSettings => {
                            settings = Some(SettingsState::default());
                            show_keymap = false;
                            self.notice.clear();
                            continue;
                        }
                        ComposerAction::Submit(value) => {
                            return Ok(ReadOutcome::Submit { text: value, ui });
                        }
                        ComposerAction::SubmitWhileBusy => {
                            unreachable!("idle composer reducer cannot return SubmitWhileBusy")
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub fn stream_turn<F>(
        &mut self,
        initial_draft: String,
        context: ComposerContext<'_>,
        turns: &[aw::conversation::Turn],
        prompt: &str,
        mut ui: aw::config::UiConfig,
        worker: F,
    ) -> Result<StreamOutcome>
    where
        F: FnOnce(
                mpsc::Sender<aw::AgentEvent>,
                aw::CancellationToken,
            ) -> Result<aw::conversation::Turn>
            + Send,
    {
        let started = Instant::now();
        let cancel = aw::CancellationToken::new();
        let worker_cancel = cancel.clone();
        let (tx, rx) = mpsc::channel::<aw::AgentEvent>();
        let mut live = LiveTurn::default();
        let mut editor = Editor::default();
        editor
            .set(initial_draft)
            .map_err(|e| Error::Config(e.to_string()))?;
        let mut interaction = ComposerInteraction::default();
        let focus = self.deferred_focus.take();
        let mut show_keymap = focus.as_ref().is_some_and(|focus| focus.keymap);
        let mut menu = self.command_palette.take();
        let mut settings = focus.and_then(|focus| focus.settings);
        self.notice.clear();

        std::thread::scope(|scope| -> Result<StreamOutcome> {
            let handle = scope.spawn(move || worker(tx, worker_cancel));
            while !handle.is_finished() {
                live.elapsed = started.elapsed();
                while let Ok(event) = rx.try_recv() {
                    live.event(&event, started);
                }
                self.reconcile_inline_history(
                    &editor,
                    &context,
                    turns,
                    Some(prompt),
                    Some(&live),
                    &ui,
                    settings.is_some(),
                    show_keymap,
                    menu.is_some(),
                )?;
                let (visible_turns, header_mode) = self.visible_inline_turns(turns);
                let mut exit_row = 0u16;
                self.terminal.draw(|frame| {
                    exit_row = draw_owned(
                        frame,
                        &editor,
                        OwnedDrawState {
                            selected: interaction.selected,
                            context: &context,
                            ui: &ui,
                            turns: visible_turns,
                            header_mode,
                            pending_prompt: Some(prompt),
                            live: Some(&live),
                            settings: settings.as_mut(),
                            show_keymap,
                            command_palette: menu.as_ref(),
                            notice: &self.notice,
                        },
                        self.mode == aw::config::TuiMode::Inline,
                    );
                })?;
                self._session.set_primary_exit_row(exit_row);

                if !event::poll(Duration::from_millis(25))? {
                    continue;
                }
                match event::read()? {
                    Event::Paste(text) => {
                        if let Some(state) = menu.as_mut() {
                            state
                                .paste(&text)
                                .map_err(|error| Error::Config(error.to_string()))?;
                            continue;
                        }
                        editor
                            .insert(&text)
                            .map_err(|e| Error::Config(e.to_string()))?;
                        interaction.reset_after_external_edit();
                    }
                    Event::Resize(_, _) if self.mode == aw::config::TuiMode::Inline => {
                        self.inline_reanchor = true;
                    }
                    Event::Key(key)
                        if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                    {
                        if key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            cancel.cancel();
                            self.notice =
                                "Cancellation requested · waiting for native settlement".into();
                            continue;
                        }
                        match command_palette::minimal_key(
                            &mut menu,
                            key,
                            &context,
                            true,
                            &mut ui,
                            &mut settings,
                            &mut show_keymap,
                            &mut self.notice,
                        )? {
                            command_palette::MinimalInput::Pass => {}
                            command_palette::MinimalInput::Handled => continue,
                            command_palette::MinimalInput::Command(_) => {
                                unreachable!("busy palette cannot dispatch host commands")
                            }
                        }
                        if let Some(active) = settings.as_mut() {
                            match active.handle(key, &mut ui) {
                                SettingsInput::Close => {
                                    settings = None;
                                    self.notice.clear();
                                }
                                SettingsInput::ProviderSetup => {
                                    self.notice =
                                        "Provider/model setup is available after the current turn settles"
                                            .into();
                                }
                                SettingsInput::Handled => {}
                            }
                            continue;
                        }
                        match key.code {
                            KeyCode::F(6) => {
                                ui.show_thinking = !ui.show_thinking;
                                self.notice = format!(
                                    "Thinking activity {}",
                                    if ui.show_thinking {
                                        "expanded"
                                    } else {
                                        "collapsed"
                                    }
                                );
                                continue;
                            }
                            KeyCode::F(7) => {
                                ui.show_tool_calls = !ui.show_tool_calls;
                                self.notice = format!(
                                    "Tool details {}",
                                    if ui.show_tool_calls {
                                        "expanded"
                                    } else {
                                        "collapsed"
                                    }
                                );
                                continue;
                            }
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                cancel.cancel();
                                self.notice =
                                    "Cancellation requested · waiting for native settlement".into();
                                continue;
                            }
                            _ => {}
                        }
                        if show_keymap {
                            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                                show_keymap = false;
                            }
                            continue;
                        }
                        match interaction.handle_live(&mut editor, key, false)? {
                            ComposerAction::Handled => {
                                self.notice.clear();
                                continue;
                            }
                            ComposerAction::OpenSettings => {
                                settings = Some(SettingsState::default());
                                show_keymap = false;
                                self.notice.clear();
                                continue;
                            }
                            ComposerAction::SubmitWhileBusy | ComposerAction::Submit(_) => {
                                self.notice =
                                    "Current turn is still running; draft kept for the next turn"
                                        .into();
                                continue;
                            }
                        }
                    }
                    _ => {}
                }
            }

            while let Ok(event) = rx.try_recv() {
                live.event(&event, started);
            }
            let turn = handle
                .join()
                .map_err(|_| Error::Storage("owned Agent model worker panicked"))?;
            match &turn {
                Ok(_) => self.notice.clear(),
                Err(error) => {
                    self.notice = format!("aw: {}", crate::terminal::safe_text(&error.to_string()))
                }
            }
            if menu.is_some() {
                self.deferred_focus = Some(DeferredFocus {
                    settings,
                    keymap: show_keymap,
                });
            }
            self.command_palette = menu;
            let draft = editor;
            Ok(StreamOutcome { turn, ui, draft })
        })
    }
}

// Terminal emulators may reflow full-width scratch rows on resize. The cached
// viewport y coordinate then no longer identifies the first owned physical row.
// Record only row extents before the known cursor, never transcript contents.
struct ScratchAnchor {
    width: u16,
    rows_before_cursor: Vec<u16>,
    cursor_column: u16,
}

impl ScratchAnchor {
    fn from_buffer(buffer: &ratatui::buffer::Buffer, cursor: Option<(u16, u16)>) -> Self {
        let area = buffer.area;
        let (column, row) = cursor.unwrap_or((area.x, area.y));
        let rows_before_cursor = (area.y..row.min(area.bottom()))
            .map(|y| {
                (area.x..area.right())
                    .map(|x| {
                        let cell = &buffer[(x, y)];
                        if cell.symbol() == " " && cell.bg == Color::Reset {
                            0
                        } else {
                            let cells = unicode_width::UnicodeWidthStr::width(cell.symbol()).max(1);
                            usize::from(x - area.x)
                                .saturating_add(cells)
                                .min(usize::from(area.width)) as u16
                        }
                    })
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        Self {
            width: area.width,
            rows_before_cursor,
            cursor_column: column.saturating_sub(area.x),
        }
    }

    fn resized_origin(&self, cursor_row: u16, width: u16) -> u16 {
        let width = width.max(1);
        let preceding = self
            .rows_before_cursor
            .iter()
            .map(|cells| (*cells).max(1).div_ceil(width))
            .fold(0u16, u16::saturating_add)
            .saturating_add(self.cursor_column / width);
        cursor_row.saturating_sub(preceding)
    }
}

pub struct ScrollbackPresenter {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    _session: TerminalSession,
    notice: String,
    command_palette: Option<command_palette::State>,
    deferred_focus: Option<DeferredFocus>,
    viewport_origin_y: u16,
    scratch_height: u16,
    bottom_saturated: bool,
    released: bool,
    scratch_anchor: Option<ScratchAnchor>,
}

impl ScrollbackPresenter {
    pub fn enter() -> Result<Self> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(Error::Config(
                "scrollback Agent TUI requires a real terminal; use --output-print for redirected output"
                    .into(),
            ));
        }
        let session = TerminalSession::enter(Surface::Scrollback)?;
        let (_, physical_height) = terminal_size()?;
        let height = physical_height.saturating_sub(1).clamp(1, 4);
        let terminal = inline_terminal(height)?;
        Ok(Self {
            terminal,
            _session: session,
            notice: String::new(),
            command_palette: None,
            deferred_focus: None,
            viewport_origin_y: 0,
            scratch_height: height,
            bottom_saturated: false,
            released: false,
            scratch_anchor: None,
        })
    }

    fn insert_prompt(&mut self, prompt: &str, ui: &aw::config::UiConfig) -> Result<()> {
        let (width, _) = terminal_size()?;
        let padding = usize::from(ui.output_padding).min(usize::from(width) / 2);
        let content_width = usize::from(width)
            .saturating_sub(padding.saturating_mul(2))
            .max(1);
        let mut lines = super::agent_chrome::prompt_lines(prompt, ui, content_width);
        super::agent_chrome::inset_lines(&mut lines, padding as u8);
        lines.push(Line::default());
        let height = lines.len().clamp(1, usize::from(u16::MAX)) as u16;
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        self.terminal
            .insert_before(height, move |buffer| paragraph.render(buffer.area, buffer))?;
        Ok(())
    }

    fn clear_physical_scratch_from(&self, origin_y: u16) -> Result<()> {
        let (_, physical_height) = terminal_size()?;
        let y = origin_y.min(physical_height.saturating_sub(1));
        let mut out = io::stdout();
        execute!(out, MoveTo(0, y), TerminalClear(ClearType::FromCursorDown))?;
        out.flush()?;
        Ok(())
    }

    fn release_scratch(&mut self) -> Result<()> {
        if self.released {
            return Ok(());
        }
        let completed = self.terminal.draw(|frame| {
            frame.render_widget(Clear, frame.area());
        })?;
        self.viewport_origin_y = completed.area.y;
        self.clear_physical_scratch_from(self.viewport_origin_y)?;
        self.terminal
            .set_cursor_position((0, self.viewport_origin_y))?;
        self.released = true;
        Ok(())
    }

    fn commit_quit_and_release(&mut self, prompt: &str, ui: &aw::config::UiConfig) -> Result<()> {
        self.insert_prompt(prompt, ui)?;
        self.release_scratch()
    }

    pub fn set_notice(&mut self, notice: impl Into<String>) {
        self.notice = notice.into();
    }

    fn observe_current_viewport(&mut self) -> Result<Rect> {
        let area = {
            let frame = self.terminal.get_frame();
            frame.area()
        };
        self.viewport_origin_y = area.y;
        let (_, physical_height) = terminal_size()?;
        self.bottom_saturated |= area.bottom() >= physical_height;
        Ok(area)
    }

    fn ensure_scratch_height(&mut self, requested: u16, force_reanchor: bool) -> Result<()> {
        let (width, physical_height) = terminal_size()?;
        let max_height = physical_height.saturating_sub(1).max(1);
        let desired = requested.clamp(1, max_height);
        self.observe_current_viewport()?;
        let width_changed = self
            .scratch_anchor
            .as_ref()
            .is_some_and(|anchor| anchor.width != width);
        if desired == self.scratch_height && !force_reanchor && !width_changed {
            return Ok(());
        }

        let origin_y = if force_reanchor || width_changed {
            if let Some(anchor) = &self.scratch_anchor {
                let cursor_row = self.terminal.get_cursor_position()?.y;
                anchor.resized_origin(cursor_row, width)
            } else {
                self.viewport_origin_y
            }
        } else {
            self.viewport_origin_y
        };
        self.clear_physical_scratch_from(origin_y)?;
        let target_y = if self.bottom_saturated {
            physical_height.saturating_sub(desired)
        } else {
            origin_y.min(physical_height.saturating_sub(1))
        };
        self.terminal.set_cursor_position((0, target_y))?;
        self.terminal = inline_terminal(desired)?;
        self.scratch_height = desired;
        self.scratch_anchor = None;
        self.observe_current_viewport()?;
        Ok(())
    }

    pub fn read_prompt(
        &mut self,
        initial: Editor,
        history: &[String],
        context: ComposerContext<'_>,
        mut ui: aw::config::UiConfig,
    ) -> Result<ReadOutcome> {
        let initial_ui = ui.clone();
        let mut editor = initial;
        let mut interaction = ComposerInteraction::default();
        let focus = self.deferred_focus.take();
        let mut show_keymap = focus.as_ref().is_some_and(|focus| focus.keymap);
        let mut menu = self.command_palette.take();
        let mut settings = focus.and_then(|focus| focus.settings);
        let mut quit_armed_until: Option<Instant> = None;

        loop {
            let (width, _) = terminal_size()?;
            let required = scratch_required_rows(ScratchRowsRequest {
                editor: &editor,
                context: &context,
                ui: &ui,
                live: None,
                in_settings: settings.is_some(),
                show_keymap,
                in_palette: menu.is_some(),
                notice: &self.notice,
                width,
            });
            self.ensure_scratch_height(required, false)?;
            let mut cursor = None;
            let completed = self.terminal.draw(|frame| {
                cursor = draw_with_cursor(
                    frame,
                    &editor,
                    DrawState {
                        selected: interaction.selected,
                        context: &context,
                        ui: &ui,
                        live: None,
                        settings: settings.as_mut(),
                        show_keymap,
                        command_palette: menu.as_ref(),
                        notice: &self.notice,
                    },
                )
            })?;
            self.viewport_origin_y = completed.area.y;
            self.scratch_anchor = Some(ScratchAnchor::from_buffer(completed.buffer, cursor));
            if cursor.is_none() {
                self.terminal
                    .set_cursor_position((0, self.viewport_origin_y))?;
            }
            match event::read()? {
                Event::Paste(text) => {
                    if let Some(state) = menu.as_mut() {
                        state
                            .paste(&text)
                            .map_err(|error| Error::Config(error.to_string()))?;
                        continue;
                    }
                    quit_armed_until = None;
                    editor
                        .insert(&text)
                        .map_err(|e| Error::Config(e.to_string()))?;
                    interaction.reset_after_external_edit();
                    self.notice.clear();
                }
                Event::Resize(_, _) => {
                    // Ratatui Inline has no mutable viewport-height/anchor API.
                    // Reconstruct the scratch viewport from the current physical
                    // terminal instead of resizing the old viewport in place;
                    // this clears the old footprint and avoids resize artifacts
                    // or accidental history insertion.
                    self.ensure_scratch_height(self.scratch_height, true)?;
                }
                Event::Key(key)
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                {
                    match command_palette::minimal_key(
                        &mut menu,
                        key,
                        &context,
                        false,
                        &mut ui,
                        &mut settings,
                        &mut show_keymap,
                        &mut self.notice,
                    )? {
                        command_palette::MinimalInput::Pass => {}
                        command_palette::MinimalInput::Handled => continue,
                        command_palette::MinimalInput::Command(command) => {
                            return Ok(ReadOutcome::PaletteCommand {
                                command,
                                draft: editor,
                                ui,
                            })
                        }
                    }
                    if let Some(active) = settings.as_mut() {
                        match active.handle(key, &mut ui) {
                            SettingsInput::Close => {
                                if ui != initial_ui {
                                    let draft = editor;
                                    return Ok(ReadOutcome::UiChanged { draft, ui });
                                }
                                settings = None;
                                self.notice.clear();
                            }
                            SettingsInput::ProviderSetup => {
                                let draft = editor;
                                return Ok(ReadOutcome::ConfigureProvider { draft, ui });
                            }
                            SettingsInput::Handled => {}
                        }
                        continue;
                    }
                    if key.code == KeyCode::F(6) {
                        ui.show_thinking = !ui.show_thinking;
                        self.notice = format!(
                            "Thinking activity {}",
                            if ui.show_thinking {
                                "expanded"
                            } else {
                                "collapsed"
                            }
                        );
                        continue;
                    }
                    if key.code == KeyCode::F(7) {
                        ui.show_tool_calls = !ui.show_tool_calls;
                        self.notice = format!(
                            "Tool details {}",
                            if ui.show_tool_calls {
                                "expanded"
                            } else {
                                "collapsed"
                            }
                        );
                        continue;
                    }
                    let idle_ctrl_c = key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL);
                    if !idle_ctrl_c {
                        quit_armed_until = None;
                    }
                    if idle_ctrl_c {
                        let now = Instant::now();
                        if editor.is_empty()
                            && quit_armed_until.is_some_and(|deadline| now <= deadline)
                        {
                            self.release_scratch()?;
                            return Ok(ReadOutcome::Quit { ui });
                        }
                        editor.clear();
                        quit_armed_until = Some(now + DOUBLE_CTRL_C_WINDOW);
                        self.notice = "Input cleared · Ctrl+C again to quit".into();
                        continue;
                    }
                    if show_keymap {
                        if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                            show_keymap = false;
                        }
                        continue;
                    }

                    match interaction.handle(&mut editor, key, history, false)? {
                        ComposerAction::Handled => {
                            self.notice.clear();
                            continue;
                        }
                        ComposerAction::OpenSettings => {
                            settings = Some(SettingsState::default());
                            show_keymap = false;
                            self.notice.clear();
                            continue;
                        }
                        ComposerAction::Submit(value) => {
                            if value.trim() == "/quit" {
                                self.commit_quit_and_release(&value, &ui)?;
                                return Ok(ReadOutcome::Quit { ui });
                            }
                            return Ok(ReadOutcome::Submit { text: value, ui });
                        }
                        ComposerAction::SubmitWhileBusy => {
                            unreachable!("idle composer reducer cannot return SubmitWhileBusy")
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub fn stream_turn<F>(
        &mut self,
        initial_draft: String,
        context: ComposerContext<'_>,
        prompt: &str,
        mut ui: aw::config::UiConfig,
        worker: F,
    ) -> Result<StreamOutcome>
    where
        F: FnOnce(
                mpsc::Sender<aw::AgentEvent>,
                aw::CancellationToken,
            ) -> Result<aw::conversation::Turn>
            + Send,
    {
        self.insert_prompt(prompt, &ui)?;
        let started = Instant::now();
        let cancel = aw::CancellationToken::new();
        let worker_cancel = cancel.clone();
        let (tx, rx) = mpsc::channel::<aw::AgentEvent>();
        let mut live = LiveTurn::default();
        let mut editor = Editor::default();
        editor
            .set(initial_draft)
            .map_err(|e| Error::Config(e.to_string()))?;
        let mut interaction = ComposerInteraction::default();
        let focus = self.deferred_focus.take();
        let mut show_keymap = focus.as_ref().is_some_and(|focus| focus.keymap);
        let mut menu = self.command_palette.take();
        let mut settings = focus.and_then(|focus| focus.settings);
        self.notice.clear();

        std::thread::scope(|scope| -> Result<StreamOutcome> {
            let handle = scope.spawn(move || worker(tx, worker_cancel));
            while !handle.is_finished() {
                live.elapsed = started.elapsed();
                while let Ok(event) = rx.try_recv() {
                    live.event(&event, started);
                }
                let (width, _) = terminal_size()?;
                let required = scratch_required_rows(ScratchRowsRequest {
                    editor: &editor,
                    context: &context,
                    ui: &ui,
                    live: Some(&live),
                    in_settings: settings.is_some(),
                    show_keymap,
                    in_palette: menu.is_some(),
                    notice: &self.notice,
                    width,
                });
                self.ensure_scratch_height(required, false)?;
                let mut cursor = None;
                let completed = self.terminal.draw(|frame| {
                    cursor = draw_with_cursor(
                        frame,
                        &editor,
                        DrawState {
                            selected: interaction.selected,
                            context: &context,
                            ui: &ui,
                            live: Some(&live),
                            settings: settings.as_mut(),
                            show_keymap,
                            command_palette: menu.as_ref(),
                            notice: &self.notice,
                        },
                    )
                })?;
                self.viewport_origin_y = completed.area.y;
                self.scratch_anchor = Some(ScratchAnchor::from_buffer(completed.buffer, cursor));
                if cursor.is_none() {
                    self.terminal
                        .set_cursor_position((0, self.viewport_origin_y))?;
                }

                if !event::poll(Duration::from_millis(25))? {
                    continue;
                }
                match event::read()? {
                    Event::Paste(text) => {
                        if let Some(state) = menu.as_mut() {
                            state
                                .paste(&text)
                                .map_err(|error| Error::Config(error.to_string()))?;
                            continue;
                        }
                        editor
                            .insert(&text)
                            .map_err(|e| Error::Config(e.to_string()))?;
                        interaction.reset_after_external_edit();
                    }
                    Event::Resize(_, _) => {
                        self.ensure_scratch_height(self.scratch_height, true)?;
                    }
                    Event::Key(key)
                        if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                    {
                        if key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            cancel.cancel();
                            self.notice =
                                "Cancellation requested · waiting for native settlement".into();
                            continue;
                        }
                        match command_palette::minimal_key(
                            &mut menu,
                            key,
                            &context,
                            true,
                            &mut ui,
                            &mut settings,
                            &mut show_keymap,
                            &mut self.notice,
                        )? {
                            command_palette::MinimalInput::Pass => {}
                            command_palette::MinimalInput::Handled => continue,
                            command_palette::MinimalInput::Command(_) => {
                                unreachable!("busy palette cannot dispatch host commands")
                            }
                        }
                        if let Some(active) = settings.as_mut() {
                            match active.handle(key, &mut ui) {
                                SettingsInput::Close => {
                                    settings = None;
                                    self.notice.clear();
                                }
                                SettingsInput::ProviderSetup => {
                                    self.notice =
                                        "Provider/model setup is available after the current turn settles"
                                            .into();
                                }
                                SettingsInput::Handled => {}
                            }
                            continue;
                        }
                        match key.code {
                            KeyCode::F(6) => {
                                ui.show_thinking = !ui.show_thinking;
                                self.notice = format!(
                                    "Thinking activity {}",
                                    if ui.show_thinking {
                                        "expanded"
                                    } else {
                                        "collapsed"
                                    }
                                );
                                continue;
                            }
                            KeyCode::F(7) => {
                                ui.show_tool_calls = !ui.show_tool_calls;
                                self.notice = format!(
                                    "Tool details {}",
                                    if ui.show_tool_calls {
                                        "expanded"
                                    } else {
                                        "collapsed"
                                    }
                                );
                                continue;
                            }
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                cancel.cancel();
                                self.notice =
                                    "Cancellation requested · waiting for native settlement".into();
                                continue;
                            }
                            _ => {}
                        }
                        if show_keymap {
                            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                                show_keymap = false;
                            }
                            continue;
                        }
                        match interaction.handle_live(&mut editor, key, false)? {
                            ComposerAction::Handled => {
                                self.notice.clear();
                                continue;
                            }
                            ComposerAction::OpenSettings => {
                                settings = Some(SettingsState::default());
                                show_keymap = false;
                                self.notice.clear();
                                continue;
                            }
                            ComposerAction::SubmitWhileBusy | ComposerAction::Submit(_) => {
                                self.notice =
                                    "Current turn is still running; draft kept for the next turn"
                                        .into();
                                continue;
                            }
                        }
                    }
                    _ => {}
                }
            }

            while let Ok(event) = rx.try_recv() {
                live.event(&event, started);
            }
            let turn = handle
                .join()
                .map_err(|_| Error::Storage("scrollback Agent model worker panicked"))?;
            if let Ok(completed_turn) = &turn {
                let stats = super::performance::PerformanceStats::from_turn(
                    completed_turn,
                    started.elapsed(),
                    live.first_delta,
                );
                insert_final_turn(&mut self.terminal, completed_turn, &stats, &ui)?;
                self.notice.clear();
            } else if let Err(error) = &turn {
                self.notice = format!("aw: {}", crate::terminal::safe_text(&error.to_string()));
            }
            if menu.is_some() {
                self.deferred_focus = Some(DeferredFocus {
                    settings,
                    keymap: show_keymap,
                });
            }
            self.command_palette = menu;
            let draft = editor;
            Ok(StreamOutcome { turn, ui, draft })
        })
    }
}

impl Drop for ScrollbackPresenter {
    fn drop(&mut self) {
        let _ = self.release_scratch();
    }
}

pub enum AgentPresenter {
    Scrollback(ScrollbackPresenter),
    Owned(OwnedPresenter),
}

impl AgentPresenter {
    pub fn enter(mode: aw::config::TuiMode) -> Result<Self> {
        match mode {
            aw::config::TuiMode::Scrollback => ScrollbackPresenter::enter().map(Self::Scrollback),
            aw::config::TuiMode::Inline | aw::config::TuiMode::Fullscreen => {
                OwnedPresenter::enter(mode).map(Self::Owned)
            }
        }
    }

    pub fn mode(&self) -> aw::config::TuiMode {
        match self {
            Self::Scrollback(_) => aw::config::TuiMode::Scrollback,
            Self::Owned(owned) => owned.mode(),
        }
    }

    pub fn set_notice(&mut self, notice: impl Into<String>) {
        let notice = notice.into();
        match self {
            Self::Scrollback(scrollback) => scrollback.set_notice(notice),
            Self::Owned(owned) => owned.set_notice(notice),
        }
    }

    pub fn read_prompt(
        &mut self,
        initial: Editor,
        history: &[String],
        context: ComposerContext<'_>,
        turns: &[aw::conversation::Turn],
        ui: aw::config::UiConfig,
    ) -> Result<ReadOutcome> {
        match self {
            Self::Scrollback(scrollback) => scrollback.read_prompt(initial, history, context, ui),
            Self::Owned(owned) => owned.read_prompt(initial, history, context, turns, ui),
        }
    }

    pub fn stream_turn<F>(
        &mut self,
        initial_draft: String,
        context: ComposerContext<'_>,
        turns: &[aw::conversation::Turn],
        prompt: &str,
        ui: aw::config::UiConfig,
        worker: F,
    ) -> Result<StreamOutcome>
    where
        F: FnOnce(
                mpsc::Sender<aw::AgentEvent>,
                aw::CancellationToken,
            ) -> Result<aw::conversation::Turn>
            + Send,
    {
        match self {
            Self::Scrollback(scrollback) => {
                scrollback.stream_turn(initial_draft, context, prompt, ui, worker)
            }
            Self::Owned(owned) => {
                owned.stream_turn(initial_draft, context, turns, prompt, ui, worker)
            }
        }
    }
}

fn insert_final_turn(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    turn: &aw::conversation::Turn,
    stats: &super::performance::PerformanceStats,
    ui: &aw::config::UiConfig,
) -> Result<()> {
    let width = crossterm::terminal::size()
        .map_or(80, |(width, _)| width)
        .max(1);
    let padding = usize::from(ui.output_padding).min(usize::from(width) / 2);
    let content_width = usize::from(width)
        .saturating_sub(padding.saturating_mul(2))
        .max(1);
    let mut lines = final_turn_lines(turn, stats, ui, content_width);
    super::agent_chrome::inset_lines(&mut lines, padding as u8);
    let wrap_width = usize::from(width).max(1);
    let height = lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(wrap_width))
        .sum::<usize>()
        .clamp(1, usize::from(u16::MAX)) as u16;
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    terminal.insert_before(height, move |buffer| paragraph.render(buffer.area, buffer))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn palette_resize_and_live_updates_keep_authoritative_turns_and_editor() {
        let context = ComposerContext {
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
        };
        let turns = vec![inline_turn(
            "ACCEPTED_OUTPUT",
            aw::conversation::Outcome::Completed,
            None,
            Some("ACCEPTED_OUTPUT"),
        )];
        let original = serde_json::to_value(&turns).unwrap();
        let mut editor = Editor::default();
        editor.set("unsent 🐦 draft").unwrap();
        editor.left();
        let original_editor = editor.clone();
        let mut menu = command_palette::State::default();
        let mut live = LiveTurn::default();
        let started = Instant::now();
        let ui = aw::config::UiConfig::default();
        for (width, height, category) in [
            (80, 30, command_palette::Category::Commands),
            (35, 12, command_palette::Category::Files),
            (120, 44, command_palette::Category::Agents),
        ] {
            menu.set("/status").unwrap();
            menu.category(category).unwrap();
            live.event(
                &aw::AgentEvent::Delta {
                    turn_id: 1,
                    text: "FRESH_LIVE_OUTPUT ".into(),
                },
                started,
            );
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    draw_owned(
                        frame,
                        &editor,
                        OwnedDrawState {
                            selected: 0,
                            context: &context,
                            ui: &ui,
                            turns: &turns,
                            header_mode: super::super::agent_chrome::HeaderMode::Hidden,
                            pending_prompt: Some("RUNNING_PROMPT"),
                            live: Some(&live),
                            settings: None,
                            show_keymap: false,
                            command_palette: Some(&menu),
                            notice: "",
                        },
                        true,
                    );
                })
                .unwrap();
            assert_eq!(serde_json::to_value(&turns).unwrap(), original);
            assert_eq!(editor, original_editor);
        }
        let mut terminal = Terminal::new(TestBackend::new(120, 44)).unwrap();
        terminal
            .draw(|frame| {
                draw_owned(
                    frame,
                    &editor,
                    OwnedDrawState {
                        selected: 0,
                        context: &context,
                        ui: &ui,
                        turns: &turns,
                        header_mode: super::super::agent_chrome::HeaderMode::Hidden,
                        pending_prompt: Some("RUNNING_PROMPT"),
                        live: Some(&live),
                        settings: None,
                        show_keymap: false,
                        command_palette: None,
                        notice: "",
                    },
                    true,
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
        assert!(text.contains("ACCEPTED_OUTPUT"));
        assert_eq!(text.matches("FRESH_LIVE_OUTPUT").count(), 3);
        assert!(!text.contains("Command Palette"));
        assert_eq!(editor, original_editor);
    }

    #[test]
    fn scrollback_resize_anchor_accounts_for_reflow_without_clearing_history() {
        let mut buffer = ratatui::buffer::Buffer::empty(Rect::new(0, 10, 110, 5));
        buffer.set_string(0, 10, "─".repeat(110), Style::default());
        buffer.set_string(0, 11, "Ask anything", Style::default());
        let anchor = ScratchAnchor::from_buffer(&buffer, Some((0, 11)));
        assert_eq!(anchor.resized_origin(9, 82), 7);
        assert_eq!(anchor.resized_origin(11, 128), 10);
        assert_eq!(anchor.resized_origin(2, 82), 0);
        assert_eq!(anchor.resized_origin(9, 0), 0);
        let mut narrowed = ratatui::buffer::Buffer::empty(Rect::new(0, 7, 82, 5));
        narrowed.set_string(0, 7, "─".repeat(82), Style::default());
        narrowed.set_string(0, 8, "Ask anything", Style::default());
        let narrowed_anchor = ScratchAnchor::from_buffer(&narrowed, Some((0, 8)));
        assert_eq!(narrowed_anchor.resized_origin(11, 128), 10);
        let hidden_cursor = ScratchAnchor::from_buffer(&buffer, None);
        assert_eq!(hidden_cursor.resized_origin(7, 82), 7);
    }

    #[test]
    fn editor_key_contract_does_not_steal_plain_j() {
        let mut editor = Editor::default();
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
            EditMode::Composer,
        )
        .unwrap();
        assert_eq!(editor.text(), "j");
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
            EditMode::Composer,
        )
        .unwrap();
        assert_eq!(editor.text(), "j\n");
    }

    #[test]
    fn inline_slash_popup_is_ratatui_and_uses_shared_registry() {
        let mut editor = Editor::default();
        editor.set("/mo").unwrap();
        let context = ComposerContext {
            resources: Default::default(),
            cwd: "/tmp/project",
            profile: "local",
            provider: "fixture",
            effort: "",
            model: "fixture",
            agent: "main",
            branch: "main",
            tools: "off",
            session: "memory",
            state: "ready",
            session_tps: None,
        };
        let ui = aw::config::UiConfig::default();
        let mut terminal = Terminal::new(TestBackend::new(100, 18)).unwrap();
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &editor,
                    DrawState {
                        selected: 0,
                        context: &context,
                        ui: &ui,
                        live: None,
                        settings: None,
                        show_keymap: false,
                        command_palette: None,
                        notice: "",
                    },
                )
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("/model [MODEL]"));
        assert!(!text.contains("/scrollback"));
        assert!(super::slash_matches("/tu", false)
            .iter()
            .any(|command| command.name == "tui"));
        #[cfg(feature = "workspace")]
        assert!(super::slash_matches("/work", false)
            .iter()
            .any(|command| command.name == "workspace"));
        assert!(!text.contains("Ctrl+]"));
        let buffer = terminal.backend().buffer();
        let row = |y: u16| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };
        let composer_y = (0..buffer.area.height)
            .find(|y| {
                let text = row(*y);
                text.contains("/mo") && !text.contains("/model")
            })
            .unwrap();
        let popup_y = (0..buffer.area.height)
            .find(|y| row(*y).contains("/model [MODEL]"))
            .unwrap();
        let popup_x = row(popup_y).find("/model [MODEL]").unwrap() as u16;
        let palette = crate::tui::theme::palette_for(&ui);
        assert_eq!(buffer[(popup_x, popup_y)].bg, palette.accent);
        assert_eq!(buffer[(popup_x, popup_y)].fg, palette.accent_foreground);
        let upper_rule = (0..composer_y)
            .rev()
            .find(|y| row(*y).contains('─'))
            .unwrap();
        assert!(
            popup_y < upper_rule,
            "slash completion belongs above the composer"
        );

        let mut plain_editor = Editor::default();
        plain_editor.set("hello").unwrap();
        let mut plain_terminal = Terminal::new(TestBackend::new(100, 18)).unwrap();
        plain_terminal
            .draw(|frame| {
                draw(
                    frame,
                    &plain_editor,
                    DrawState {
                        selected: 0,
                        context: &context,
                        ui: &ui,
                        live: None,
                        settings: None,
                        show_keymap: false,
                        command_palette: None,
                        notice: "",
                    },
                )
            })
            .unwrap();
        let plain = plain_terminal.backend().buffer();
        let plain_row = |y: u16| {
            (0..plain.area.width)
                .map(|x| plain[(x, y)].symbol())
                .collect::<String>()
        };
        let plain_composer_y = (0..plain.area.height)
            .find(|y| plain_row(*y).contains("hello"))
            .unwrap();
        let expected_popup_rows = slash_matches("/mo", false)
            .len()
            .min(usize::from(ui.autocomplete_max_visible)) as u16;
        assert_eq!(
            composer_y,
            plain_composer_y + expected_popup_rows,
            "natural flow should move the composer only by the popup's actual visible rows"
        );
        assert!(
            composer_y + 2 < plain.area.height,
            "short popup flow must not pin the composer to the physical bottom"
        );
    }
    #[test]
    fn scrollback_scratch_height_tracks_only_active_ui_rows() {
        let context = ComposerContext {
            resources: Default::default(),
            cwd: "/tmp/project",
            profile: "local",
            provider: "fixture",
            effort: "",
            model: "fixture",
            agent: "main",
            branch: "main",
            tools: "off",
            session: "memory",
            state: "ready",
            session_tps: None,
        };
        let ui = aw::config::UiConfig::default();
        let editor = Editor::default();
        let idle = scratch_required_rows(ScratchRowsRequest {
            editor: &editor,
            context: &context,
            ui: &ui,
            live: None,
            in_settings: false,
            show_keymap: false,
            in_palette: false,
            notice: "",
            width: 100,
        });
        assert_eq!(
            idle, 5,
            "two default status rows plus compact composer should reserve five rows"
        );

        let mut slash = Editor::default();
        slash.set("/").unwrap();
        let popup = scratch_required_rows(ScratchRowsRequest {
            editor: &slash,
            context: &context,
            ui: &ui,
            live: None,
            in_settings: false,
            show_keymap: false,
            in_palette: false,
            notice: "",
            width: 100,
        });
        assert!(popup > idle);
        assert_eq!(popup - idle, u16::from(ui.autocomplete_max_visible));

        let live = LiveTurn {
            thinking: true,
            answer: "one\ntwo".into(),
            tools: Vec::new(),
            first_delta: None,
            elapsed: Duration::ZERO,
        };
        let live_rows = scratch_required_rows(ScratchRowsRequest {
            editor: &editor,
            context: &context,
            ui: &ui,
            live: Some(&live),
            in_settings: false,
            show_keymap: false,
            in_palette: false,
            notice: "Working",
            width: 100,
        });
        assert!(live_rows > idle);

        let settings = scratch_required_rows(ScratchRowsRequest {
            editor: &editor,
            context: &context,
            ui: &ui,
            live: None,
            in_settings: true,
            show_keymap: false,
            in_palette: false,
            notice: "",
            width: 100,
        });
        assert!(
            settings > popup,
            "settings may temporarily expand the scratch viewport"
        );
    }

    #[test]
    fn live_answer_preview_is_utf8_safe_and_terminal_sanitized() {
        let live = LiveTurn {
            thinking: false,
            answer: format!("{}\u{1b}]52;c;secret\u{7}", "🐦".repeat(1100)),
            tools: Vec::new(),
            first_delta: None,
            elapsed: Duration::ZERO,
        };
        let text = live_lines(&live, &aw::config::UiConfig::default(), 100)
            .into_iter()
            .flat_map(|line| line.spans.into_iter())
            .map(|span| span.content.into_owned())
            .collect::<String>();
        assert!(text.contains('🐦'));
        assert!(!text.contains('\u{1b}'));
        assert!(!text.contains('\u{7}'));
        assert!(text.contains("\\u{1b}"));
    }

    #[test]
    fn live_visibility_collapse_keeps_labels_and_hides_tool_output() {
        let record = aw_agent_core::ToolRecord {
            call: aw_agent_core::ToolCall {
                id: "call-1".into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command":"echo secret-detail"}),
            },
            effect: aw_agent_core::ToolEffect::Process,
            execution_site: "local".into(),
            status: aw_agent_core::ToolStatus::Succeeded,
            output: Some(aw_agent_core::ToolOutput::text("secret-detail")),
        };
        let live = LiveTurn {
            thinking: true,
            answer: String::new(),
            tools: vec![record],
            first_delta: None,
            elapsed: Duration::ZERO,
        };
        let ui = aw::config::UiConfig {
            show_thinking: false,
            show_tool_calls: false,
            ..aw::config::UiConfig::default()
        };
        let text = live_lines(&live, &ui, 100)
            .into_iter()
            .flat_map(|line| line.spans.into_iter())
            .map(|span| span.content.into_owned())
            .collect::<String>();
        assert!(
            !text.contains("Thinking"),
            "live thinking/progress belongs in the composer border, not a separate row"
        );
        assert!(text.contains("tool bash"));
        assert!(text.contains("hidden"));
        assert!(!text.contains("secret-detail"));

        let expanded = aw::config::UiConfig {
            show_thinking: false,
            show_tool_calls: true,
            ..aw::config::UiConfig::default()
        };
        let rows = live_lines(&live, &expanded, 100);
        let tool_rows = rows
            .iter()
            .filter(|line| {
                line.spans.iter().any(|span| {
                    span.content.contains("$ echo secret-detail")
                        || span.content.contains("secret-detail")
                })
            })
            .collect::<Vec<_>>();
        assert!(!tool_rows.is_empty());
        assert!(tool_rows.iter().all(|line| line.width() == 100));
    }

    fn inline_turn(
        answer: &str,
        outcome: aw::conversation::Outcome,
        error: Option<&str>,
        terminal_trace_answer: Option<&str>,
    ) -> aw::conversation::Turn {
        let mut context_messages = vec![aw_agent_core::Message::text(
            1024,
            aw_agent_core::Role::User,
            "hello",
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
            prompt: "hello".into(),
            answer: answer.into(),
            model: "fixture".into(),
            profile: "fixture".into(),
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
    fn final_turn_falls_back_to_authoritative_answer_and_surfaces_blank_errors() {
        let ui = aw::config::UiConfig::default();
        let stats = super::super::performance::PerformanceStats {
            elapsed: Duration::from_millis(300),
            first_token: None,
            input_tokens: None,
            output_tokens: None,
            throughput_eligible: false,
        };
        let completed = inline_turn(
            "VISIBLE ANSWER",
            aw::conversation::Outcome::Completed,
            None,
            Some(""),
        );
        let completed_text = final_turn_lines(&completed, &stats, &ui, 100)
            .into_iter()
            .flat_map(|line| line.spans.into_iter())
            .map(|span| span.content.into_owned())
            .collect::<String>();
        assert!(completed_text.contains("VISIBLE ANSWER"));

        let stale = inline_turn(
            "AUTHORITATIVE ANSWER",
            aw::conversation::Outcome::Completed,
            None,
            Some("stale trace"),
        );
        let stale_text = final_turn_lines(&stale, &stats, &ui, 100)
            .into_iter()
            .flat_map(|line| line.spans.into_iter())
            .map(|span| span.content.into_owned())
            .collect::<String>();
        assert!(stale_text.contains("stale trace"));
        assert!(stale_text.contains("AUTHORITATIVE ANSWER"));

        let partial_suffix = inline_turn(
            "hello world",
            aw::conversation::Outcome::Completed,
            None,
            Some("world"),
        );
        let partial_text = final_turn_lines(&partial_suffix, &stats, &ui, 100)
            .into_iter()
            .flat_map(|line| line.spans.into_iter())
            .map(|span| span.content.into_owned())
            .collect::<String>();
        assert!(partial_text.contains("hello world"));

        let incomplete = inline_turn(
            "",
            aw::conversation::Outcome::Incomplete,
            Some("provider did not complete a text answer"),
            None,
        );
        let incomplete_text = final_turn_lines(&incomplete, &stats, &ui, 100)
            .into_iter()
            .flat_map(|line| line.spans.into_iter())
            .map(|span| span.content.into_owned())
            .collect::<String>();
        assert!(incomplete_text.contains("Thinking 0.3s"));
        assert!(incomplete_text.contains("provider did not complete a text answer"));
    }

    #[test]
    fn inline_context_key_changes_when_loaded_resources_change() {
        let mut context = ComposerContext {
            resources: super::super::agent_resources::LoadedResourceSummary::with_system_prompt(
                super::super::agent_resources::SystemPromptOrigin::BuiltIn,
                64,
            ),
            cwd: "/tmp/project",
            profile: "fixture",
            provider: "fixture",
            effort: "",
            model: "fixture-model",
            agent: "main",
            branch: "main",
            tools: "off",
            session: "session-resources",
            state: "ready",
            session_tps: None,
        };
        let before = OwnedPresenter::inline_context_key(&context);
        context.resources.add_instruction_file("./AGENTS.md", 42);
        let after = OwnedPresenter::inline_context_key(&context);
        assert_ne!(before, after);
    }

    #[test]
    fn inline_eviction_commits_only_runtime_turns_after_loaded_history() {
        assert_eq!(inline_eviction(8, 10, true, 0, 4, 3), InlineEviction::Fits);
        assert_eq!(
            inline_eviction(12, 10, true, 0, 4, 3),
            InlineEviction::HideHeader
        );
        assert_eq!(
            inline_eviction(12, 10, false, 0, 4, 3),
            InlineEviction::EvictTurn {
                index: 0,
                commit: false,
            },
            "loaded historical turns may leave the live viewport but must never be re-emitted"
        );
        assert_eq!(
            inline_eviction(12, 10, false, 3, 4, 3),
            InlineEviction::EvictTurn {
                index: 3,
                commit: true,
            },
            "only turns settled during this Inline presenter may spill into shell history"
        );
        assert_eq!(
            inline_eviction(12, 10, false, 4, 4, 3),
            InlineEviction::Saturate
        );
    }

    #[test]
    fn inline_short_content_flows_naturally_with_two_status_rows_below_composer() {
        let ui = aw::config::UiConfig::default();
        let context = ComposerContext {
            resources: super::super::agent_resources::LoadedResourceSummary::with_system_prompt(
                super::super::agent_resources::SystemPromptOrigin::Config,
                128,
            ),
            cwd: "/tmp/project",
            profile: "fixture",
            provider: "fixture",
            effort: "",
            model: "fixture-model",
            agent: "main",
            branch: "main",
            tools: "off",
            session: "session-flow",
            state: "ready",
            session_tps: None,
        };
        let width = 100;
        let height = 32;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let editor = Editor::default();
        terminal
            .draw(|frame| {
                draw_owned(
                    frame,
                    &editor,
                    OwnedDrawState {
                        selected: 0,
                        context: &context,
                        ui: &ui,
                        turns: &[],
                        header_mode: crate::tui::agent_chrome::HeaderMode::Hidden,
                        pending_prompt: None,
                        live: None,
                        settings: None,
                        show_keymap: false,
                        command_palette: None,
                        notice: "",
                    },
                    true,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row = |y: u16| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };
        let rendered = (0..height).map(row).collect::<Vec<_>>().join("\n");
        for header in ["[Context]", "[Skills]", "aw v"] {
            assert!(
                !rendered.contains(header),
                "Minimal must be headerless: {header}"
            );
        }
        let status_y = (0..height)
            .find(|y| row(*y).contains("fixture-model"))
            .expect("default status row");
        let composer_y = (0..height)
            .find(|y| row(*y).contains("Ask anything"))
            .expect("composer row");
        assert!(
            status_y > composer_y,
            "default status rows belong below the composer"
        );
        assert!(
            composer_y < height / 2,
            "short content should flow naturally near transcript, not pin to terminal bottom"
        );
        assert!(
            (status_y + 1..height).all(|y| row(y).trim().is_empty()),
            "unused rows belong below the status stack"
        );
    }

    #[test]
    fn inline_long_content_saturates_and_keeps_composer_at_bottom() {
        let ui = aw::config::UiConfig::default();
        let context = ComposerContext {
            resources: Default::default(),
            cwd: "/tmp/project",
            profile: "fixture",
            provider: "fixture",
            effort: "",
            model: "fixture-model",
            agent: "main",
            branch: "main",
            tools: "off",
            session: "session-bottom",
            state: "ready",
            session_tps: None,
        };
        let turns = (0..20)
            .map(|index| {
                inline_turn(
                    &format!("answer-{index}"),
                    aw::conversation::Outcome::Completed,
                    None,
                    Some(&format!("answer-{index}")),
                )
            })
            .collect::<Vec<_>>();
        let width = 80;
        let height = 18;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let editor = Editor::default();
        terminal
            .draw(|frame| {
                draw_owned(
                    frame,
                    &editor,
                    OwnedDrawState {
                        selected: 0,
                        context: &context,
                        ui: &ui,
                        turns: &turns,
                        header_mode: crate::tui::agent_chrome::HeaderMode::Full,
                        pending_prompt: None,
                        live: None,
                        settings: None,
                        show_keymap: false,
                        command_palette: None,
                        notice: "",
                    },
                    true,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row = |y: u16| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };
        let composer_y = (0..height)
            .find(|y| row(*y).contains("Ask anything"))
            .expect("composer row");
        assert_eq!(
            composer_y + 4,
            height,
            "after natural saturation the composer plus two status rows should reach the physical bottom"
        );
    }

    #[test]
    fn owned_transcript_stays_bottom_anchored_when_wrapped_prompt_geometry_changes() {
        let mut turn = inline_turn(
            "TAIL_MARKER",
            aw::conversation::Outcome::Completed,
            None,
            Some("TAIL_MARKER"),
        );
        turn.prompt = "very-long-user-prompt-".repeat(16);
        let ui = aw::config::UiConfig {
            show_performance: false,
            ..aw::config::UiConfig::default()
        };
        let context = ComposerContext {
            resources: Default::default(),
            cwd: "/tmp/project",
            profile: "fixture",
            provider: "fixture",
            effort: "",
            model: "fixture",
            agent: "main",
            branch: "main",
            tools: "off",
            session: "session-1",
            state: "ready",
            session_tps: None,
        };
        for (width, height) in [(34, 14), (96, 20)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let editor = Editor::default();
            terminal
                .draw(|frame| {
                    draw_owned(
                        frame,
                        &editor,
                        OwnedDrawState {
                            selected: 0,
                            context: &context,
                            ui: &ui,
                            turns: std::slice::from_ref(&turn),
                            header_mode: crate::tui::agent_chrome::HeaderMode::Full,
                            pending_prompt: None,
                            live: None,
                            settings: None,
                            show_keymap: false,
                            command_palette: None,
                            notice: "",
                        },
                        true,
                    );
                })
                .unwrap();
            let rendered = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(
                rendered.contains("TAIL_MARKER"),
                "latest answer must remain visible at {width}x{height}"
            );
            assert!(
                rendered.contains("Ask anything"),
                "composer must remain visible at {width}x{height}"
            );
        }
    }

    #[test]
    fn settled_answers_render_markdown_but_live_partial_stream_stays_plain() {
        let markdown =
            "# Result\n\n- **bold** with §code§".replace('§', &char::from(96).to_string());
        let turn = inline_turn(
            &markdown,
            aw::conversation::Outcome::Completed,
            None,
            Some(&markdown),
        );
        let ui = aw::config::UiConfig {
            theme: aw::config::ThemeMode::Dark,
            ..aw::config::UiConfig::default()
        };
        let stats = super::super::performance::PerformanceStats::default();
        let settled = final_turn_lines(&turn, &stats, &ui, 80);
        let visible = crate::tui::markdown::plain_lines(&settled).join("\n");
        assert!(visible.contains("Result"));
        assert!(visible.contains("• bold with code"));
        assert!(!visible.contains("**"));
        assert!(!visible.contains("§code§".replace('§', &char::from(96).to_string()).as_str()));

        let live = LiveTurn {
            thinking: false,
            answer: markdown.clone(),
            tools: Vec::new(),
            first_delta: None,
            elapsed: Duration::ZERO,
        };
        let live_visible = live_lines(&live, &ui, 80)
            .into_iter()
            .flat_map(|line| line.spans.into_iter())
            .map(|span| span.content.into_owned())
            .collect::<String>();
        assert!(live_visible.contains("# Result"));
        assert!(live_visible.contains("**bold**"));
        assert!(live_visible.contains("§code§".replace('§', &char::from(96).to_string()).as_str()));
    }

    #[test]
    fn tool_tones_distinguish_success_failure_and_running() {
        let record = |status, output| aw_agent_core::ToolRecord {
            call: aw_agent_core::ToolCall {
                id: "call-1".into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command":"true"}),
            },
            effect: aw_agent_core::ToolEffect::Process,
            execution_site: "local".into(),
            status,
            output,
        };
        assert_eq!(
            aw::presentation::tool_tone(Some(&record(aw_agent_core::ToolStatus::Running, None))),
            aw::presentation::ToolTone::Neutral
        );
        assert_eq!(
            aw::presentation::tool_tone(Some(&record(
                aw_agent_core::ToolStatus::Succeeded,
                Some(aw_agent_core::ToolOutput::text("ok")),
            ))),
            aw::presentation::ToolTone::Success
        );
        assert_eq!(
            aw::presentation::tool_tone(Some(&record(
                aw_agent_core::ToolStatus::Failed,
                Some(aw_agent_core::ToolOutput::error("boom")),
            ))),
            aw::presentation::ToolTone::Failure
        );
        assert_eq!(
            aw::presentation::tool_tone(None),
            aw::presentation::ToolTone::Neutral
        );

        let palette = crate::tui::theme::palette(aw::config::ThemeMode::Dark);
        assert_eq!(
            palette.tool_style(aw::presentation::ToolTone::Success).bg,
            Some(palette.tool_background)
        );
        assert_eq!(
            palette.tool_style(aw::presentation::ToolTone::Failure).bg,
            Some(palette.tool_error_background)
        );
        assert_eq!(
            palette.tool_style(aw::presentation::ToolTone::Neutral).bg,
            Some(palette.tool_neutral_background)
        );
    }

    #[test]
    fn inline_settings_live_between_prompt_rules() {
        let editor = Editor::default();
        let context = ComposerContext {
            resources: Default::default(),
            cwd: "/tmp/project",
            profile: "local",
            provider: "fixture",
            effort: "",
            model: "fixture",
            agent: "main",
            branch: "main",
            tools: "off",
            session: "memory",
            state: "ready",
            session_tps: None,
        };
        let ui = aw::config::UiConfig::default();
        let mut settings = SettingsState::default();
        let mut terminal = Terminal::new(TestBackend::new(100, 18)).unwrap();
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &editor,
                    DrawState {
                        selected: 0,
                        context: &context,
                        ui: &ui,
                        live: None,
                        settings: Some(&mut settings),
                        show_keymap: false,
                        command_palette: None,
                        notice: "",
                    },
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row = |y: u16| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };
        let settings_y = (0..buffer.area.height)
            .find(|y| row(*y).contains("Settings"))
            .unwrap();
        assert!(settings_y > 0);
        assert!(row(settings_y - 1).contains('─'));
        let lower_rule = (settings_y + 1..buffer.area.height)
            .find(|y| row(*y).contains('─'))
            .unwrap();
        assert!(lower_rule > settings_y);
    }

    #[test]
    fn composer_and_auxiliary_views_use_distinct_stable_accents() {
        assert!(!COMPOSER_PALETTE.contains(&AUXILIARY_VIEW_COLOR));
        let context = ComposerContext {
            resources: Default::default(),
            cwd: "/tmp/project",
            profile: "local",
            provider: "fixture",
            effort: "",
            model: "fixture",
            agent: "main",
            branch: "main",
            tools: "off",
            session: "session-123",
            state: "ready",
            session_tps: None,
        };
        let first = composer_accent(&context);
        let second = composer_accent(&context);
        assert_eq!(first, second);
        assert!(COMPOSER_PALETTE.contains(&first));
        assert_ne!(first, AUXILIARY_VIEW_COLOR);

        fn rendered_rule_colors(
            context: &ComposerContext<'_>,
            settings: Option<&mut SettingsState>,
        ) -> Vec<Color> {
            let editor = Editor::default();
            let ui = aw::config::UiConfig {
                status_lines: Vec::new(),
                ..aw::config::UiConfig::default()
            };
            let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
            terminal
                .draw(|frame| {
                    draw(
                        frame,
                        &editor,
                        DrawState {
                            selected: 0,
                            context,
                            ui: &ui,
                            live: None,
                            settings,
                            show_keymap: false,
                            command_palette: None,
                            notice: "",
                        },
                    )
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            let mut colors = Vec::new();
            for y in 0..buffer.area.height {
                let mut saw_rule = false;
                let mut color = None;
                for x in 0..buffer.area.width {
                    let cell = &buffer[(x, y)];
                    if cell.symbol() == "─" {
                        saw_rule = true;
                        color.get_or_insert(cell.fg);
                    }
                }
                if saw_rule {
                    colors.push(color.expect("rule cell has foreground color"));
                }
            }
            colors
        }

        let composer_rules = rendered_rule_colors(&context, None);
        assert_eq!(composer_rules.len(), 2);
        assert!(composer_rules.iter().all(|color| *color == first));

        let mut settings = SettingsState::default();
        let settings_rules = rendered_rule_colors(&context, Some(&mut settings));
        assert_eq!(settings_rules.len(), 2);
        assert!(settings_rules
            .iter()
            .all(|color| *color == AUXILIARY_VIEW_COLOR));
    }
}
