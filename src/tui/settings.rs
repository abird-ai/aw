use aw::config::{StatusItem, StatusLineConfig, StatusRegion, ThemeMode, TuiMode, UiConfig};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingKey {
    ProviderSetup,
    Theme,
    TuiMode,
    AgentEditorPadding,
    AgentOutputPadding,
    WorkspaceEditorPadding,
    WorkspaceOutputPadding,
    Autocomplete,
    Performance,
    Ttft,
    Waterfall,
    Thinking,
    ToolCalls,
    Status(StatusItem),
}

#[derive(Clone, Debug)]
struct Row {
    key: SettingKey,
    label: &'static str,
    value: String,
    description: &'static str,
}

#[derive(Default)]
pub struct SettingsState {
    selected: usize,
    filter: String,
}

pub enum SettingsInput {
    Handled,
    Close,
    ProviderSetup,
}

fn on_off(value: bool) -> String {
    if value { "on" } else { "off" }.into()
}

fn status_enabled(ui: &UiConfig, item: StatusItem) -> bool {
    ui.status_lines
        .iter()
        .any(|line| line.items.contains(&item) || line.right_items.contains(&item))
}

fn toggle_status(ui: &mut UiConfig, item: StatusItem) {
    if status_enabled(ui, item) {
        for line in &mut ui.status_lines {
            line.items.retain(|candidate| *candidate != item);
            line.right_items.retain(|candidate| *candidate != item);
        }
        ui.status_lines
            .retain(|line| !line.items.is_empty() || !line.right_items.is_empty());
        return;
    }

    let mut inserted_below = false;
    let mut inserted_fullscreen = false;
    for line in &mut ui.status_lines {
        match line.region {
            StatusRegion::BelowComposer if !inserted_below => {
                line.items.push(item);
                inserted_below = true;
            }
            StatusRegion::FullscreenStatus if !inserted_fullscreen => {
                line.items.push(item);
                inserted_fullscreen = true;
            }
            _ => {}
        }
    }
    if !inserted_below {
        ui.status_lines.push(StatusLineConfig {
            region: StatusRegion::BelowComposer,
            items: vec![item],
            right_items: Vec::new(),
        });
    }
    if !inserted_fullscreen {
        ui.status_lines.push(StatusLineConfig {
            region: StatusRegion::FullscreenStatus,
            items: vec![item],
            right_items: Vec::new(),
        });
    }
}

fn rows(ui: &UiConfig) -> Vec<Row> {
    vec![
        Row {
            key: SettingKey::ProviderSetup,
            label: "Provider & model…",
            value: "open".into(),
            description: "Add or change the persisted provider/model profile. Secrets stay in env/SecretStore.",
        },
        Row {
            key: SettingKey::Theme,
            label: "Theme",
            value: match ui.theme {
                ThemeMode::System => "system",
                ThemeMode::Light => "light",
                ThemeMode::Dark => "dark",
            }
            .into(),
            description: "System follows terminal/browser appearance by default; Light/Dark force an aw palette.",
        },
        Row {
            key: SettingKey::TuiMode,
            label: "TUI mode",
            value: match ui.tui_mode {
                TuiMode::Scrollback => "scrollback",
                TuiMode::Inline => "inline",
                TuiMode::Fullscreen => "fullscreen",
            }
            .into(),
            description:
                "Agent presentation: terminal scrollback, owned primary screen, or owned alternate screen.",
        },
        Row {
            key: SettingKey::AgentEditorPadding,
            label: "Agent editor padding",
            value: ui.editor_padding_x.to_string(),
            description: "Horizontal prompt padding (0–3) in standalone Agent modes.",
        },
        Row {
            key: SettingKey::AgentOutputPadding,
            label: "Agent output padding",
            value: ui.output_padding.to_string(),
            description: "Horizontal transcript padding (0–3) in standalone Agent modes.",
        },
        Row {
            key: SettingKey::WorkspaceEditorPadding,
            label: "Workspace editor padding",
            value: ui.workspace_editor_padding_x.to_string(),
            description: "Horizontal Agent-composer padding (0–3) inside Workspace.",
        },
        Row {
            key: SettingKey::WorkspaceOutputPadding,
            label: "Workspace output padding",
            value: ui.workspace_output_padding.to_string(),
            description: "Horizontal content/transcript padding (0–3) inside Workspace.",
        },
        Row {
            key: SettingKey::Autocomplete,
            label: "Autocomplete max items",
            value: ui.autocomplete_max_visible.to_string(),
            description: "Maximum visible slash-command suggestions (3–20).",
        },
        Row {
            key: SettingKey::Performance,
            label: "Performance stats",
            value: on_off(ui.show_performance),
            description: "Show built-in monochrome throughput/token timing after completed turns.",
        },
        Row {
            key: SettingKey::Ttft,
            label: "Show TTFT",
            value: on_off(ui.show_ttft),
            description: "Show first-token latency when it was measured for the live invocation.",
        },
        Row {
            key: SettingKey::Waterfall,
            label: "Per-turn waterfall",
            value: on_off(ui.show_waterfall),
            description: "Detailed per-invocation timing bar. Off by default.",
        },
        Row {
            key: SettingKey::Thinking,
            label: "Thinking activity",
            value: on_off(ui.show_thinking),
            description: "Show compact thinking activity/timing labels. Provider-private reasoning text remains private.",
        },
        Row {
            key: SettingKey::ToolCalls,
            label: "Tool call details",
            value: on_off(ui.show_tool_calls),
            description: "Expand typed tool calls/results. Off collapses current and future activity to compact labels.",
        },
        Row {
            key: SettingKey::Status(StatusItem::Model),
            label: "Status · model",
            value: on_off(status_enabled(ui, StatusItem::Model)),
            description: "Show the selected model in the footer.",
        },
        Row {
            key: SettingKey::Status(StatusItem::Directory),
            label: "Status · directory",
            value: on_off(status_enabled(ui, StatusItem::Directory)),
            description: "Show the current execution directory.",
        },
        Row {
            key: SettingKey::Status(StatusItem::Branch),
            label: "Status · branch",
            value: on_off(status_enabled(ui, StatusItem::Branch)),
            description: "Show the current durable branch.",
        },
        Row {
            key: SettingKey::Status(StatusItem::Agent),
            label: "Status · agent",
            value: on_off(status_enabled(ui, StatusItem::Agent)),
            description: "Show the current Agent title/identity.",
        },
        Row {
            key: SettingKey::Status(StatusItem::Provider),
            label: "Status · provider",
            value: on_off(status_enabled(ui, StatusItem::Provider)),
            description: "Show the provider kind (OpenAI, Responses, Ollama).",
        },
        Row {
            key: SettingKey::Status(StatusItem::Profile),
            label: "Status · profile",
            value: on_off(status_enabled(ui, StatusItem::Profile)),
            description: "Show the active provider profile/account label.",
        },
        Row {
            key: SettingKey::Status(StatusItem::Effort),
            label: "Status · thinking effort",
            value: on_off(status_enabled(ui, StatusItem::Effort)),
            description: "Show the selected reasoning effort when configured.",
        },
        Row {
            key: SettingKey::Status(StatusItem::SessionTps),
            label: "Status · run average TPS",
            value: on_off(status_enabled(ui, StatusItem::SessionTps)),
            description: "Show measured current-process model generation throughput when trustworthy.",
        },
        Row {
            key: SettingKey::Status(StatusItem::Tools),
            label: "Status · tools",
            value: on_off(status_enabled(ui, StatusItem::Tools)),
            description: "Show the current local tool mode.",
        },
        Row {
            key: SettingKey::Status(StatusItem::Session),
            label: "Status · session",
            value: on_off(status_enabled(ui, StatusItem::Session)),
            description: "Show the durable session identifier.",
        },
        Row {
            key: SettingKey::Status(StatusItem::State),
            label: "Status · state",
            value: on_off(status_enabled(ui, StatusItem::State)),
            description: "Show ready/running/inspection state.",
        },
    ]
}

impl SettingsState {
    fn filtered(&self, ui: &UiConfig) -> Vec<Row> {
        let query = self.filter.to_ascii_lowercase();
        rows(ui)
            .into_iter()
            .filter(|row| {
                query.is_empty()
                    || row.label.to_ascii_lowercase().contains(&query)
                    || row.description.to_ascii_lowercase().contains(&query)
                    || row.value.to_ascii_lowercase().contains(&query)
            })
            .collect()
    }

    fn clamp(&mut self, len: usize) {
        self.selected = self.selected.min(len.saturating_sub(1));
    }

    fn change(ui: &mut UiConfig, key: SettingKey) {
        match key {
            SettingKey::ProviderSetup => {}
            SettingKey::Theme => {
                ui.theme = match ui.theme {
                    ThemeMode::System => ThemeMode::Light,
                    ThemeMode::Light => ThemeMode::Dark,
                    ThemeMode::Dark => ThemeMode::System,
                }
            }
            SettingKey::TuiMode => {
                ui.tui_mode = match ui.tui_mode {
                    TuiMode::Scrollback => TuiMode::Inline,
                    TuiMode::Inline => TuiMode::Fullscreen,
                    TuiMode::Fullscreen => TuiMode::Scrollback,
                }
            }
            SettingKey::AgentEditorPadding => {
                ui.editor_padding_x = (ui.editor_padding_x + 1) % 4
            }
            SettingKey::AgentOutputPadding => ui.output_padding = (ui.output_padding + 1) % 4,
            SettingKey::WorkspaceEditorPadding => {
                ui.workspace_editor_padding_x = (ui.workspace_editor_padding_x + 1) % 4
            }
            SettingKey::WorkspaceOutputPadding => {
                ui.workspace_output_padding = (ui.workspace_output_padding + 1) % 4
            }
            SettingKey::Autocomplete => {
                const VALUES: [u8; 6] = [3, 5, 7, 10, 15, 20];
                let next = VALUES
                    .iter()
                    .position(|value| *value == ui.autocomplete_max_visible)
                    .map_or(1, |index| (index + 1) % VALUES.len());
                ui.autocomplete_max_visible = VALUES[next];
            }
            SettingKey::Performance => ui.show_performance = !ui.show_performance,
            SettingKey::Ttft => ui.show_ttft = !ui.show_ttft,
            SettingKey::Waterfall => ui.show_waterfall = !ui.show_waterfall,
            SettingKey::Thinking => ui.show_thinking = !ui.show_thinking,
            SettingKey::ToolCalls => ui.show_tool_calls = !ui.show_tool_calls,
            SettingKey::Status(item) => toggle_status(ui, item),
        }
    }

    #[cfg(feature = "workspace")]
    pub fn click(&mut self, area: Rect, y: u16, ui: &mut UiConfig) -> SettingsInput {
        let visible = self.filtered(ui);
        self.clamp(visible.len());
        let max_rows = area.height.saturating_sub(7) as usize;
        if max_rows == 0 || y < area.y.saturating_add(2) {
            return SettingsInput::Handled;
        }
        let offset = usize::from(y.saturating_sub(area.y.saturating_add(2)));
        if offset >= max_rows {
            return SettingsInput::Handled;
        }
        let start = if self.selected >= max_rows {
            self.selected + 1 - max_rows
        } else {
            0
        };
        let index = start.saturating_add(offset);
        let Some(row) = visible.get(index) else {
            return SettingsInput::Handled;
        };
        self.selected = index;
        if row.key == SettingKey::ProviderSetup {
            return SettingsInput::ProviderSetup;
        }
        Self::change(ui, row.key);
        SettingsInput::Handled
    }

    pub fn handle(&mut self, key: KeyEvent, ui: &mut UiConfig) -> SettingsInput {
        let visible = self.filtered(ui);
        self.clamp(visible.len());
        match key.code {
            KeyCode::Esc => return SettingsInput::Close,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return SettingsInput::Close
            }
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(visible.len().saturating_sub(1))
            }
            KeyCode::Enter | KeyCode::Char(' ')
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if let Some(row) = visible.get(self.selected) {
                    if row.key == SettingKey::ProviderSetup {
                        return SettingsInput::ProviderSetup;
                    }
                    Self::change(ui, row.key);
                }
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.selected = 0;
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter.clear();
                self.selected = 0;
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.filter.push(c);
                self.selected = 0;
            }
            _ => {}
        }
        SettingsInput::Handled
    }

    pub fn draw(&mut self, frame: &mut Frame<'_>, area: Rect, ui: &UiConfig) {
        let palette = crate::tui::theme::palette_for(ui);
        let visible = self.filtered(ui);
        self.clamp(visible.len());
        let max_rows = area.height.saturating_sub(7) as usize;
        let start = if self.selected >= max_rows && max_rows > 0 {
            self.selected + 1 - max_rows
        } else {
            0
        };
        let shown = visible
            .iter()
            .skip(start)
            .take(max_rows)
            .collect::<Vec<_>>();
        let mut lines = Vec::new();
        let heading = if self.filter.is_empty() {
            "Settings".to_owned()
        } else {
            format!("Settings · search: {}", self.filter)
        };
        lines.push(Line::from(Span::styled(
            heading,
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        for (offset, row) in shown.iter().enumerate() {
            let index = start + offset;
            let selected = index == self.selected;
            lines.push(Line::from(vec![
                Span::styled(
                    if selected { "→ " } else { "  " },
                    Style::default().fg(if selected {
                        palette.accent
                    } else {
                        palette.muted
                    }),
                ),
                Span::styled(
                    format!("{:<30}", row.label),
                    Style::default().fg(if selected {
                        palette.accent
                    } else {
                        palette.foreground
                    }),
                ),
                Span::styled(
                    format!("[{}]", row.value),
                    Style::default().fg(if selected {
                        palette.accent
                    } else {
                        palette.faint
                    }),
                ),
            ]));
        }
        lines.push(Line::from(format!(
            "({}/{})",
            self.selected.saturating_add(1).min(visible.len()),
            visible.len()
        )));
        if let Some(row) = visible.get(self.selected) {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                row.description,
                Style::default().fg(palette.muted),
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Type to search · ↑↓ navigate · Enter/Space change · Esc close",
            Style::default().fg(palette.muted),
        )));
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_defaults_match_minimal_pi_like_contract() {
        let ui = UiConfig::default();
        assert_eq!(ui.theme, ThemeMode::System);
        assert_eq!(ui.editor_padding_x, 0);
        assert_eq!(ui.output_padding, 0);
        assert_eq!(ui.workspace_editor_padding_x, 1);
        assert_eq!(ui.workspace_output_padding, 1);
        assert_eq!(ui.autocomplete_max_visible, 5);
        assert!(ui.show_performance);
        assert!(!ui.show_ttft);
        assert!(!ui.show_waterfall);
        assert!(ui.show_thinking);
        assert!(ui.show_tool_calls);
    }

    #[test]
    fn tui_mode_cycles_scrollback_inline_fullscreen_without_workspace_coupling() {
        let mut ui = UiConfig::default();
        assert_eq!(ui.tui_mode, TuiMode::Inline);
        SettingsState::change(&mut ui, SettingKey::TuiMode);
        assert_eq!(ui.tui_mode, TuiMode::Fullscreen);
        SettingsState::change(&mut ui, SettingKey::TuiMode);
        assert_eq!(ui.tui_mode, TuiMode::Scrollback);
        SettingsState::change(&mut ui, SettingKey::TuiMode);
        assert_eq!(ui.tui_mode, TuiMode::Inline);
    }

    #[test]
    fn theme_cycles_system_light_dark_without_touching_execution_settings() {
        let mut ui = UiConfig::default();
        SettingsState::change(&mut ui, SettingKey::Theme);
        assert_eq!(ui.theme, ThemeMode::Light);
        SettingsState::change(&mut ui, SettingKey::Theme);
        assert_eq!(ui.theme, ThemeMode::Dark);
        SettingsState::change(&mut ui, SettingKey::Theme);
        assert_eq!(ui.theme, ThemeMode::System);
    }

    #[test]
    fn provider_setup_is_a_typed_action_not_a_ui_toggle() {
        let mut ui = UiConfig::default();
        let before = ui.clone();
        let mut settings = SettingsState::default();
        assert!(matches!(
            settings.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut ui,),
            SettingsInput::ProviderSetup
        ));
        assert_eq!(ui, before);
    }

    #[test]
    fn changing_status_item_preserves_rows_and_order_of_remaining_items() {
        let mut ui = UiConfig::default();
        let before = ui.status_lines.clone();
        SettingsState::change(&mut ui, SettingKey::Status(StatusItem::Agent));
        assert!(!status_enabled(&ui, StatusItem::Agent));

        let expected = before
            .into_iter()
            .filter_map(|mut line| {
                line.items.retain(|item| *item != StatusItem::Agent);
                line.right_items.retain(|item| *item != StatusItem::Agent);
                (!line.items.is_empty() || !line.right_items.is_empty()).then_some(line)
            })
            .collect::<Vec<_>>();
        assert_eq!(ui.status_lines, expected);
    }
}
