use super::{
    editor::{edit_key, EditMode},
    inline::inline_terminal,
    terminal_session::{Surface, TerminalSession},
};
use aw::{
    config::{FileConfig, Profile, ProfileUpdate, ProviderKind, TokenField},
    Error, Result,
};
use aw_editor::Editor;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};
use std::io::{self, IsTerminal};

const SETUP_COLOR: Color = Color::Rgb(255, 133, 71);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProviderChoice {
    ChatGptPlan,
    ClaudeApi,
    GeminiApi,
    OllamaCloud,
    CustomChatCompletions,
    CustomResponses,
    Skip,
}

impl ProviderChoice {
    const ALL: [Self; 7] = [
        Self::ChatGptPlan,
        Self::ClaudeApi,
        Self::GeminiApi,
        Self::OllamaCloud,
        Self::CustomChatCompletions,
        Self::CustomResponses,
        Self::Skip,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::ChatGptPlan => "ChatGPT subscription",
            Self::ClaudeApi => "Claude API (Anthropic)",
            Self::GeminiApi => "Gemini API (Google)",
            Self::OllamaCloud => "Ollama Cloud",
            Self::CustomChatCompletions => "OpenAI-compatible Chat Completions",
            Self::CustomResponses => "OpenAI-compatible Responses API",
            Self::Skip => "Skip for now",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::ChatGptPlan => {
                "Continue with ChatGPT; tokens stay in SecretStore and inference uses Responses"
            }
            Self::ClaudeApi => {
                "Native Anthropic Messages API; use ANTHROPIC_API_KEY from your Claude Console"
            }
            Self::GeminiApi => {
                "Native Gemini generateContent API; use GEMINI_API_KEY from Google AI Studio"
            }
            Self::OllamaCloud => {
                "Native Ollama cloud endpoint; OLLAMA_API_KEY is read only when you later send"
            }
            Self::CustomChatCompletions => "Streaming OpenAI-compatible /chat/completions endpoint",
            Self::CustomResponses => "Streaming OpenAI-compatible /responses endpoint",
            Self::Skip => "Stay unconfigured; Settings and aw web remain available later",
        }
    }

    fn preset(self) -> Option<Preset> {
        match self {
            Self::ChatGptPlan => None,
            Self::ClaudeApi => Some(Preset {
                name: "anthropic",
                kind: ProviderKind::Anthropic,
                base_url: "https://api.anthropic.com/v1",
                api_key_env: Some("ANTHROPIC_API_KEY"),
                token_field: TokenField::MaxTokens,
                include_usage: true,
                custom_endpoint: false,
                custom_api_env: false,
            }),
            Self::GeminiApi => Some(Preset {
                name: "gemini",
                kind: ProviderKind::Gemini,
                base_url: "https://generativelanguage.googleapis.com/v1beta",
                api_key_env: Some("GEMINI_API_KEY"),
                token_field: TokenField::MaxTokens,
                include_usage: true,
                custom_endpoint: false,
                custom_api_env: false,
            }),
            Self::OllamaCloud => Some(Preset {
                name: "ollama-cloud",
                kind: ProviderKind::Ollama,
                base_url: "https://ollama.com",
                api_key_env: Some("OLLAMA_API_KEY"),
                token_field: TokenField::MaxTokens,
                include_usage: false,
                custom_endpoint: false,
                custom_api_env: false,
            }),
            Self::CustomChatCompletions => Some(Preset {
                name: "openai-compatible",
                kind: ProviderKind::Openai,
                base_url: "http://127.0.0.1:8080/v1",
                api_key_env: None,
                token_field: TokenField::MaxTokens,
                include_usage: false,
                custom_endpoint: true,
                custom_api_env: true,
            }),
            Self::CustomResponses => Some(Preset {
                name: "responses-compatible",
                kind: ProviderKind::OpenaiResponses,
                base_url: "https://api.openai.com/v1",
                api_key_env: None,
                token_field: TokenField::MaxTokens,
                include_usage: true,
                custom_endpoint: true,
                custom_api_env: true,
            }),
            Self::Skip => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Preset {
    name: &'static str,
    kind: ProviderKind,
    base_url: &'static str,
    api_key_env: Option<&'static str>,
    token_field: TokenField,
    include_usage: bool,
    custom_endpoint: bool,
    custom_api_env: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Choose,
    Endpoint,
    ApiEnv,
    Discovering,
    Models,
    Model,
    Confirm,
}

#[derive(Debug)]
struct Wizard {
    step: Step,
    selected: usize,
    choice: Option<ProviderChoice>,
    profile_name: String,
    endpoint: String,
    api_key_env: String,
    model: String,
    models: Vec<String>,
    model_selected: usize,
    editor: Editor,
    notice: String,
}

impl Default for Wizard {
    fn default() -> Self {
        Self {
            step: Step::Choose,
            selected: 0,
            choice: None,
            profile_name: String::new(),
            endpoint: String::new(),
            api_key_env: String::new(),
            model: String::new(),
            models: Vec::new(),
            model_selected: 0,
            editor: Editor::default(),
            notice: String::new(),
        }
    }
}

pub enum OnboardingOutcome {
    Save(ProfileUpdate),
    ChatGptPlan { profile_name: String },
    Skip,
}

fn unique_name(config: &FileConfig, base: &str) -> String {
    if !config.profiles.contains_key(base) {
        return base.to_owned();
    }
    for n in 2..=99 {
        let candidate = format!("{base}-{n}");
        if !config.profiles.contains_key(&candidate) {
            return candidate;
        }
    }
    format!("{base}-{}", std::process::id())
}

impl Wizard {
    fn select(&mut self, config: &FileConfig) -> Option<OnboardingOutcome> {
        let choice = ProviderChoice::ALL[self.selected];
        if choice == ProviderChoice::Skip {
            return Some(OnboardingOutcome::Skip);
        }
        if choice == ProviderChoice::ChatGptPlan {
            return Some(OnboardingOutcome::ChatGptPlan {
                profile_name: unique_name(config, "chatgpt"),
            });
        }

        let preset = choice.preset().expect("non-skip preset");
        self.choice = Some(choice);
        self.profile_name = unique_name(config, preset.name);
        self.endpoint = preset.base_url.to_owned();
        self.api_key_env = preset.api_key_env.unwrap_or("").to_owned();
        self.model.clear();
        self.models.clear();
        self.model_selected = 0;
        self.editor.clear();

        if preset.custom_endpoint {
            let _ = self.editor.set(self.endpoint.clone());
            self.step = Step::Endpoint;
        } else if preset.custom_api_env || preset.api_key_env.is_some() {
            let _ = self.editor.set(self.api_key_env.clone());
            self.step = Step::ApiEnv;
        } else {
            self.step = Step::Discovering;
        }
        None
    }

    fn input_prompt(&self) -> &'static str {
        match self.step {
            Step::Endpoint => "Provider base URL",
            Step::ApiEnv => "API key environment variable (optional)",
            Step::Model => "Model ID (automatic discovery fallback)",
            _ => "",
        }
    }

    fn submit_input(&mut self) -> Result<()> {
        let value = self.editor.text().trim().to_owned();
        match self.step {
            Step::Endpoint => {
                if value.is_empty() {
                    self.notice = "Endpoint is required.".into();
                    return Ok(());
                }
                self.endpoint = value;
                let preset = self
                    .choice
                    .and_then(ProviderChoice::preset)
                    .expect("preset");
                self.editor.clear();
                if preset.custom_api_env || preset.api_key_env.is_some() {
                    let _ = self.editor.set(self.api_key_env.clone());
                    self.step = Step::ApiEnv;
                } else {
                    self.step = Step::Discovering;
                }
            }
            Step::ApiEnv => {
                self.api_key_env = value;
                self.editor.clear();
                self.step = Step::Discovering;
            }
            Step::Model => {
                if value.is_empty() {
                    self.notice = "Enter a model ID, or go back and retry discovery.".into();
                    return Ok(());
                }
                aw::config::validate_model(&value)?;
                self.model = value;
                self.editor.clear();
                self.step = Step::Confirm;
            }
            _ => {}
        }
        self.notice.clear();
        Ok(())
    }

    fn discovery_profile(&self) -> Result<Profile> {
        let preset = self
            .choice
            .and_then(ProviderChoice::preset)
            .ok_or_else(|| Error::Config("provider choice missing".into()))?;
        let profile = Profile {
            kind: preset.kind,
            base_url: self.endpoint.clone(),
            model: None,
            models: Vec::new(),
            api_key_env: if self.api_key_env.trim().is_empty() {
                None
            } else {
                Some(self.api_key_env.trim().to_owned())
            },
            oauth_connector: None,
            oauth_host_id: None,
            allow_insecure_http: false,
            token_field: preset.token_field,
            max_tokens: None,
            temperature: None,
            reasoning_effort: None,
            think: None,
            ca_file: None,
            include_usage: preset.include_usage,
        };
        profile.validate()?;
        Ok(profile)
    }

    fn apply_discovery(&mut self, result: Result<Vec<String>>) {
        match result {
            Ok(models) if !models.is_empty() => {
                self.models = models;
                self.model_selected = 0;
                self.notice.clear();
                self.step = Step::Models;
            }
            Ok(_) => {
                self.models.clear();
                self.editor.clear();
                self.notice =
                    "Automatic model discovery returned no usable models. Enter a model ID manually."
                        .into();
                self.step = Step::Model;
            }
            Err(error) => {
                self.models.clear();
                self.editor.clear();
                self.notice = format!(
                    "Automatic model discovery unavailable: {}. Enter a model ID manually.",
                    crate::terminal::safe_text(&error.to_string())
                );
                self.step = Step::Model;
            }
        }
    }

    fn choose_discovered_model(&mut self) {
        if self.model_selected >= self.models.len() {
            self.editor.clear();
            self.notice.clear();
            self.step = Step::Model;
            return;
        }
        self.model = self.models[self.model_selected].clone();
        self.notice.clear();
        self.step = Step::Confirm;
    }

    fn update(&self) -> ProfileUpdate {
        let preset = self
            .choice
            .and_then(ProviderChoice::preset)
            .expect("chosen preset");
        ProfileUpdate {
            name: self.profile_name.clone(),
            kind: preset.kind,
            base_url: self.endpoint.clone(),
            default_model: Some(self.model.clone()),
            models: self.models.clone(),
            api_key_env: if self.api_key_env.trim().is_empty() {
                None
            } else {
                Some(self.api_key_env.trim().to_owned())
            },
            oauth_connector: None,
            oauth_host_id: None,
            allow_insecure_http: false,
            token_field: preset.token_field,
            include_usage: preset.include_usage,
            make_default: true,
        }
    }

    fn back_to_credentials(&mut self) {
        let preset = self
            .choice
            .and_then(ProviderChoice::preset)
            .expect("preset");
        self.editor.clear();
        if preset.custom_api_env || preset.api_key_env.is_some() {
            let _ = self.editor.set(self.api_key_env.clone());
            self.step = Step::ApiEnv;
        } else if preset.custom_endpoint {
            let _ = self.editor.set(self.endpoint.clone());
            self.step = Step::Endpoint;
        } else {
            self.step = Step::Choose;
        }
    }

    fn back(&mut self) {
        match self.step {
            Step::Choose => {}
            Step::Endpoint => self.step = Step::Choose,
            Step::ApiEnv => {
                let preset = self
                    .choice
                    .and_then(ProviderChoice::preset)
                    .expect("preset");
                if preset.custom_endpoint {
                    let _ = self.editor.set(self.endpoint.clone());
                    self.step = Step::Endpoint;
                } else {
                    self.step = Step::Choose;
                }
            }
            Step::Discovering => self.back_to_credentials(),
            Step::Models => self.back_to_credentials(),
            Step::Model => {
                if self.models.is_empty() {
                    self.back_to_credentials();
                } else {
                    self.model_selected = self
                        .models
                        .iter()
                        .position(|model| model == &self.model)
                        .unwrap_or(0);
                    self.step = Step::Models;
                }
            }
            Step::Confirm => {
                if let Some(index) = self.models.iter().position(|model| model == &self.model) {
                    self.model_selected = index;
                    self.step = Step::Models;
                } else {
                    let _ = self.editor.set(self.model.clone());
                    self.step = Step::Model;
                }
            }
        }
        self.notice.clear();
    }
}

fn draw(frame: &mut Frame<'_>, wizard: &mut Wizard, theme_mode: aw::config::ThemeMode) {
    let palette = crate::tui::theme::palette(theme_mode);
    let area = frame.area();
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(palette.base_style()), area);
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(SETUP_COLOR));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "First run",
                Style::default()
                    .fg(SETUP_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  Configure a provider/model now, or skip.",
                Style::default().fg(palette.faint),
            ),
        ])),
        chunks[0],
    );

    match wizard.step {
        Step::Choose => {
            let mut state = ListState::default();
            state.select(Some(wizard.selected));
            let rows = ProviderChoice::ALL
                .iter()
                .copied()
                .map(|choice| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{:<34}", choice.title()),
                            Style::default().fg(palette.foreground),
                        ),
                        Span::styled(choice.description(), Style::default().fg(palette.muted)),
                    ]))
                })
                .collect::<Vec<_>>();
            frame.render_stateful_widget(
                List::new(rows).highlight_style(
                    palette.selected_style(),
                ),
                chunks[1],
                &mut state,
            );
        }
        Step::Endpoint | Step::ApiEnv | Step::Model => {
            let prompt = wizard.input_prompt();
            let view = wizard.editor.view(chunks[1].width.saturating_sub(2), 3);
            let mut lines = vec![
                Line::styled(prompt, Style::default().fg(palette.faint)),
                Line::from(vec![
                    Span::styled("› ", Style::default().fg(SETUP_COLOR)),
                    Span::raw(view.lines.first().cloned().unwrap_or_default()),
                ]),
            ];
            if wizard.step == Step::ApiEnv {
                lines.push(Line::styled(
                    "Store only the environment-variable NAME; the secret value never enters config.",
                    Style::default().fg(palette.muted),
                ));
            }
            frame.render_widget(Paragraph::new(lines), chunks[1]);
            frame.set_cursor_position((
                chunks[1].x + 2 + view.cursor_x,
                chunks[1].y + 1 + view.cursor_y,
            ));
        }
        Step::Discovering => {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled(
                        "Discovering available models…",
                        Style::default().fg(SETUP_COLOR).add_modifier(Modifier::BOLD),
                    ),
                    Line::styled(
                        format!("{}  {}", wizard.endpoint, if wizard.api_key_env.is_empty() { "no credential" } else { "credential via environment" }),
                        Style::default().fg(palette.muted),
                    ),
                ]),
                chunks[1],
            );
        }
        Step::Models => {
            let total = wizard.models.len() + 1;
            let visible = usize::from(chunks[1].height.max(1));
            let start = wizard.model_selected.saturating_sub(visible.saturating_sub(1));
            let mut rows = wizard
                .models
                .iter()
                .enumerate()
                .skip(start)
                .take(visible)
                .map(|(index, model)| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            if index == wizard.model_selected { "● " } else { "  " },
                            Style::default().fg(SETUP_COLOR),
                        ),
                        Span::styled(model, Style::default().fg(palette.foreground)),
                    ]))
                })
                .collect::<Vec<_>>();
            if start + rows.len() <= wizard.models.len() && rows.len() < visible {
                rows.push(ListItem::new(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled("Enter model ID manually…", Style::default().fg(palette.faint)),
                ])));
            }
            let mut state = ListState::default();
            state.select(Some(wizard.model_selected.saturating_sub(start).min(rows.len().saturating_sub(1))));
            frame.render_stateful_widget(
                List::new(rows).highlight_style(
                    palette.selected_style(),
                ),
                chunks[1],
                &mut state,
            );
            let _ = total;
        }
        Step::Confirm => {
            let kind = wizard
                .choice
                .and_then(ProviderChoice::preset)
                .expect("preset")
                .kind;
            let lines = vec![
                Line::styled(
                    "Save this profile?",
                    Style::default()
                        .fg(SETUP_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Line::from(format!("profile   {}", wizard.profile_name)),
                Line::from(format!("provider  {kind:?}")),
                Line::from(format!("endpoint  {}", wizard.endpoint)),
                Line::from(format!("model     {}", wizard.model)),
                Line::from(format!(
                    "secret    {}",
                    if wizard.api_key_env.is_empty() {
                        "none".into()
                    } else {
                        format!("env:{}", wizard.api_key_env)
                    }
                )),
                Line::from(""),
                Line::styled(
                    "No provider request is made while saving.",
                    Style::default().fg(palette.muted),
                ),
            ];
            frame.render_widget(Paragraph::new(lines), chunks[1]);
        }
    }

    let footer = if wizard.notice.is_empty() {
        match wizard.step {
            Step::Choose => "↑↓ choose · Enter continue · Esc skip",
            Step::Discovering => "Discovering models…",
            Step::Models => "↑↓ choose · Enter use model · Esc back",
            Step::Confirm => "Enter save · Esc back · Ctrl+C skip",
            _ => "Enter continue · Esc back · Ctrl+C skip",
        }
    } else {
        &wizard.notice
    };
    frame.render_widget(
        Paragraph::new(Span::styled(footer, Style::default().fg(palette.muted))),
        chunks[2],
    );
}

#[cfg(feature = "oidc")]
pub fn choose_chatgpt_model(
    models: &[aw::oidc_flow::ChatGptPlanModel],
    theme_mode: aw::config::ThemeMode,
) -> Result<Option<String>> {
    if models.is_empty() {
        return Err(Error::Config(
            "ChatGPT account returned no selectable models".into(),
        ));
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::Config(
            "ChatGPT model selection requires an interactive terminal".into(),
        ));
    }
    let _session = TerminalSession::enter(Surface::Scrollback)?;
    let mut terminal = inline_terminal(15)?;
    let mut selected = 0usize;
    let palette = crate::tui::theme::palette(theme_mode);
    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            frame.render_widget(Clear, area);
            frame.render_widget(Block::default().style(palette.base_style()), area);
            let block = Block::default()
                .borders(Borders::TOP | Borders::BOTTOM)
                .border_style(Style::default().fg(palette.muted));
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
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        "ChatGPT",
                        Style::default()
                            .fg(palette.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "  Choose a model available to this signed-in account.",
                        Style::default().fg(palette.faint),
                    ),
                ])),
                chunks[0],
            );
            let visible = usize::from(chunks[1].height.max(1));
            let start = selected.saturating_sub(visible.saturating_sub(1));
            let rows = models
                .iter()
                .skip(start)
                .take(visible)
                .map(|model| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{:<28}", model.display_name),
                            Style::default().fg(palette.foreground),
                        ),
                        Span::styled(&model.slug, Style::default().fg(palette.muted)),
                    ]))
                })
                .collect::<Vec<_>>();
            let mut state = ListState::default();
            state.select(Some(selected.saturating_sub(start)));
            frame.render_stateful_widget(
                List::new(rows).highlight_style(
                    palette.selected_style(),
                ),
                chunks[1],
                &mut state,
            );
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "↑↓ choose · Enter use model · Esc cancel sign-in setup",
                    Style::default().fg(palette.muted),
                )),
                chunks[2],
            );
        })?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            continue;
        }
        match key.code {
            KeyCode::Up => selected = selected.saturating_sub(1),
            KeyCode::Down => selected = (selected + 1).min(models.len() - 1),
            KeyCode::PageUp => selected = selected.saturating_sub(8),
            KeyCode::PageDown => selected = (selected + 8).min(models.len() - 1),
            KeyCode::Home => selected = 0,
            KeyCode::End => selected = models.len() - 1,
            KeyCode::Enter => {
                terminal.clear()?;
                return Ok(Some(models[selected].slug.clone()));
            }
            KeyCode::Esc => {
                terminal.clear()?;
                return Ok(None);
            }
            _ => {}
        }
    }
}

pub fn run_with_discovery(
    config: &FileConfig,
    discover: &mut dyn FnMut(&Profile) -> Result<Vec<String>>,
) -> Result<OnboardingOutcome> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::Config(
            "first-run onboarding requires a terminal; use explicit CLI/config options instead"
                .into(),
        ));
    }
    let _session = TerminalSession::enter(Surface::Scrollback)?;
    let mut terminal = inline_terminal(15)?;
    let mut wizard = Wizard::default();

    loop {
        terminal.draw(|frame| draw(frame, &mut wizard, config.ui.theme))?;

        if wizard.step == Step::Discovering {
            let result = wizard
                .discovery_profile()
                .and_then(|profile| discover(&profile));
            wizard.apply_discovery(result);
            continue;
        }

        let event = event::read()?;
        let Event::Key(key) = event else {
            continue;
        };
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            continue;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            terminal.clear()?;
            return Ok(OnboardingOutcome::Skip);
        }

        match wizard.step {
            Step::Choose => match key.code {
                KeyCode::Esc => {
                    terminal.clear()?;
                    return Ok(OnboardingOutcome::Skip);
                }
                KeyCode::Up => wizard.selected = wizard.selected.saturating_sub(1),
                KeyCode::Down => {
                    wizard.selected =
                        (wizard.selected + 1).min(ProviderChoice::ALL.len().saturating_sub(1))
                }
                KeyCode::Enter => {
                    if let Some(outcome) = wizard.select(config) {
                        terminal.clear()?;
                        return Ok(outcome);
                    }
                }
                _ => {}
            },
            Step::Endpoint | Step::ApiEnv | Step::Model => match key.code {
                KeyCode::Esc => wizard.back(),
                KeyCode::Enter => wizard.submit_input()?,
                _ => {
                    edit_key(&mut wizard.editor, key, EditMode::SingleLine)
                        .map_err(|error| Error::Config(error.to_string()))?;
                    wizard.notice.clear();
                }
            },
            Step::Models => match key.code {
                KeyCode::Esc => wizard.back(),
                KeyCode::Up => {
                    wizard.model_selected = wizard.model_selected.saturating_sub(1);
                }
                KeyCode::Down => {
                    wizard.model_selected =
                        (wizard.model_selected + 1).min(wizard.models.len());
                }
                KeyCode::PageUp => {
                    wizard.model_selected = wizard.model_selected.saturating_sub(8);
                }
                KeyCode::PageDown => {
                    wizard.model_selected =
                        (wizard.model_selected + 8).min(wizard.models.len());
                }
                KeyCode::Home => wizard.model_selected = 0,
                KeyCode::End => wizard.model_selected = wizard.models.len(),
                KeyCode::Enter => wizard.choose_discovered_model(),
                _ => {}
            },
            Step::Discovering => unreachable!("discovery is handled before reading input"),
            Step::Confirm => match key.code {
                KeyCode::Esc => wizard.back(),
                KeyCode::Enter => {
                    let update = wizard.update();
                    match aw::config::apply_profile_update(config, update.clone()) {
                        Ok(_) => {
                            terminal.clear()?;
                            return Ok(OnboardingOutcome::Save(update));
                        }
                        Err(error) => {
                            wizard.notice = error.to_string();
                        }
                    }
                }
                _ => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_names_do_not_overwrite_existing_profile() {
        let mut cfg = FileConfig::default();
        cfg.profiles.insert(
            "ollama".into(),
            aw::config::builtins().remove("ollama").unwrap(),
        );
        assert_eq!(unique_name(&cfg, "ollama"), "ollama-2");
    }

    #[test]
    fn native_anthropic_setup_uses_api_credential_reference_not_consumer_oauth() {
        let preset = ProviderChoice::ClaudeApi.preset().unwrap();
        assert_eq!(preset.kind, ProviderKind::Anthropic);
        assert_eq!(preset.base_url, "https://api.anthropic.com/v1");
        assert_eq!(preset.api_key_env, Some("ANTHROPIC_API_KEY"));
        assert!(!preset.custom_endpoint);
    }

    #[test]
    fn native_gemini_setup_uses_google_ai_studio_credential_reference() {
        let preset=ProviderChoice::GeminiApi.preset().unwrap();
        assert_eq!(preset.kind,ProviderKind::Gemini);
        assert_eq!(preset.base_url,"https://generativelanguage.googleapis.com/v1beta");
        assert_eq!(preset.api_key_env,Some("GEMINI_API_KEY"));
        assert!(!preset.custom_endpoint);
    }

    #[test]
    fn cloud_preset_uses_env_reference_not_secret() {
        let preset = ProviderChoice::OllamaCloud.preset().unwrap();
        assert_eq!(preset.base_url, "https://ollama.com");
        assert_eq!(preset.api_key_env, Some("OLLAMA_API_KEY"));
        assert_eq!(preset.kind, ProviderKind::Ollama);
    }

    #[test]
    fn custom_openai_wire_choices_are_explicit() {
        let chat = ProviderChoice::CustomChatCompletions.preset().unwrap();
        assert_eq!(chat.kind, ProviderKind::Openai);
        let responses = ProviderChoice::CustomResponses.preset().unwrap();
        assert_eq!(responses.kind, ProviderKind::OpenaiResponses);
    }

    #[test]
    fn chatgpt_plan_uses_the_dedicated_oauth_path() {
        assert_eq!(ProviderChoice::ChatGptPlan.title(), "ChatGPT subscription");
        assert!(ProviderChoice::ChatGptPlan.preset().is_none());
        let mut wizard = Wizard::default();
        assert!(matches!(
            wizard.select(&FileConfig::default()),
            Some(OnboardingOutcome::ChatGptPlan { .. })
        ));
    }
}
