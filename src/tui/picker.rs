use super::{
    commands::cycle_selection,
    editor::{edit_key, EditMode},
    inline::inline_terminal,
    terminal_session::{Surface, TerminalSession},
};
use aw::{Error, Result};
use aw_editor::Editor;
use crossterm::{
    cursor::{MoveTo, MoveToColumn},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{size as terminal_size, Clear as TerminalClear, ClearType},
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};
use std::io::{self, Write};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub detail: String,
}

pub fn choose(title: &str, choices: &[Choice], theme: aw::config::ThemeMode) -> Result<Option<String>> {
    if choices.is_empty() {
        return Ok(None);
    }
    let _session = TerminalSession::enter(Surface::Scrollback)?;
    let (_, physical_height) = terminal_size()?;
    let height = (choices.len() as u16)
        .saturating_add(6)
        .clamp(7, 17)
        .min(physical_height.saturating_sub(1).max(1));
    let mut terminal = inline_terminal(height)?;
    let mut query = Editor::default();
    let mut selected = 0usize;
    let result = (|| -> Result<Option<String>> {
        loop {
            let filtered = filtered_indices(choices, query.text());
            if filtered.is_empty() {
                selected = 0;
            } else {
                selected = selected.min(filtered.len() - 1);
            }
            terminal.draw(|frame| {
                draw(
                    frame,
                    title,
                    choices,
                    &filtered,
                    selected,
                    query.text(),
                    theme,
                )
            })?;
            match event::read()? {
                Event::Key(key)
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                {
                    match key.code {
                        KeyCode::Esc => return Ok(None),
                        KeyCode::Enter => {
                            if let Some(index) = filtered.get(selected).copied() {
                                return Ok(Some(choices[index].id.clone()));
                            }
                        }
                        KeyCode::Up => {
                            selected = cycle_selection(selected, filtered.len(), false);
                        }
                        KeyCode::Down => {
                            selected = cycle_selection(selected, filtered.len(), true);
                        }
                        KeyCode::Char('k')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            selected = cycle_selection(selected, filtered.len(), false);
                        }
                        KeyCode::Char('j')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            selected = cycle_selection(selected, filtered.len(), true);
                        }
                        _ => {
                            edit_key(&mut query, key, EditMode::SingleLine)
                                .map_err(|error| Error::Config(error.to_string()))?;
                            selected = 0;
                        }
                    }
                }
                Event::Paste(text) => {
                    query
                        .insert(&text)
                        .map_err(|error| Error::Config(error.to_string()))?;
                    selected = 0;
                }
                Event::Resize(width, physical_height) => {
                    let area = terminal.get_frame().area();
                    let next = height.min(physical_height.saturating_sub(1).max(1));
                    terminal.resize(Rect::new(0, area.y, width.max(1), next))?;
                }
                _ => {}
            }
        }
    })();

    let origin = terminal.get_frame().area().y;
    let _ = terminal.draw(|frame| frame.render_widget(Clear, frame.area()));
    let mut stdout = io::stdout();
    let _ = execute!(
        stdout,
        MoveTo(0, origin),
        TerminalClear(ClearType::FromCursorDown),
        MoveToColumn(0)
    );
    let _ = stdout.flush();
    result
}

fn filtered_indices(choices: &[Choice], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    choices
        .iter()
        .enumerate()
        .filter_map(|(index, choice)| {
            (query.is_empty()
                || choice.label.to_lowercase().contains(&query)
                || choice.detail.to_lowercase().contains(&query)
                || choice.id.to_lowercase().contains(&query))
            .then_some(index)
        })
        .collect()
}

fn draw(
    frame: &mut Frame<'_>,
    title: &str,
    choices: &[Choice],
    filtered: &[usize],
    selected: usize,
    query: &str,
    theme: aw::config::ThemeMode,
) {
    let palette = super::theme::palette(theme);
    let area = frame.area();
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(palette.accent));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    title.to_owned(),
                    Style::default()
                        .fg(palette.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  {} saved", choices.len()),
                    Style::default().fg(palette.muted),
                ),
            ]),
            Line::from(vec![
                Span::styled("› ", Style::default().fg(palette.accent)),
                Span::raw(query.to_owned()),
            ]),
        ]),
        chunks[0],
    );

    if filtered.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "No matching Sessions",
                Style::default().fg(palette.muted),
            )),
            chunks[1],
        );
    } else {
        let rows = filtered
            .iter()
            .map(|index| {
                let choice = &choices[*index];
                ListItem::new(Line::from(vec![
                    Span::styled(
                        choice.label.clone(),
                        Style::default().fg(palette.foreground),
                    ),
                    Span::styled(
                        format!("  {}", choice.detail),
                        Style::default().fg(palette.muted),
                    ),
                ]))
            })
            .collect::<Vec<_>>();
        let mut state = ListState::default();
        state.select(Some(selected.min(filtered.len() - 1)));
        frame.render_stateful_widget(
            List::new(rows).highlight_style(palette.selected_style()),
            chunks[1],
            &mut state,
        );
    }
    frame.render_widget(
        Paragraph::new(Line::styled(
            "type to filter · ↑↓ select · Enter resume · Esc cancel",
            Style::default().fg(palette.faint),
        )),
        chunks[2],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_matches_title_detail_and_id() {
        let choices = vec![
            Choice {
                id: "s_1".into(),
                label: "Alpha".into(),
                detail: "/repo/main".into(),
            },
            Choice {
                id: "s_2".into(),
                label: "Beta".into(),
                detail: "/other".into(),
            },
        ];
        assert_eq!(filtered_indices(&choices, "alpha"), vec![0]);
        assert_eq!(filtered_indices(&choices, "repo"), vec![0]);
        assert_eq!(filtered_indices(&choices, "s_2"), vec![1]);
        assert_eq!(filtered_indices(&choices, ""), vec![0, 1]);
    }
}
