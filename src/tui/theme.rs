use aw::config::ThemeMode;
use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub background: Color,
    #[cfg(feature = "workspace")]
    pub surface: Color,
    pub foreground: Color,
    pub muted: Color,
    pub faint: Color,
    pub accent: Color,
    pub accent_foreground: Color,
    pub user_prompt_foreground: Color,
    pub user_prompt_background: Color,
    #[cfg(feature = "workspace")]
    pub success: Color,
    #[cfg(feature = "workspace")]
    pub warning: Color,
    #[cfg(feature = "workspace")]
    pub danger: Color,
    pub selection_foreground: Color,
    pub selection_background: Color,
    pub tool_foreground: Color,
    pub tool_background: Color,
    pub tool_neutral_foreground: Color,
    pub tool_neutral_background: Color,
    pub tool_error_foreground: Color,
    pub tool_error_background: Color,
}

impl Palette {
    pub fn base_style(self) -> Style {
        Style::default().fg(self.foreground).bg(self.background)
    }

    pub fn text_style(self) -> Style {
        Style::default().fg(self.foreground)
    }

    pub fn user_marker_style(self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    pub fn user_prompt_style(self) -> Style {
        Style::default()
            .fg(self.user_prompt_foreground)
            .bg(self.user_prompt_background)
    }

    #[cfg(feature = "workspace")]
    pub fn tab_active_style(self) -> Style {
        self.user_prompt_style().add_modifier(Modifier::BOLD)
    }

    pub fn thinking_style(self) -> Style {
        Style::default()
            .fg(self.muted)
            .add_modifier(Modifier::ITALIC)
    }

    pub fn error_style(self) -> Style {
        Style::default()
            .fg(self.tool_error_foreground)
            .add_modifier(Modifier::BOLD)
    }

    pub fn tool_style(self, tone: aw::presentation::ToolTone) -> Style {
        match tone {
            aw::presentation::ToolTone::Success => Style::default()
                .fg(self.tool_foreground)
                .bg(self.tool_background),
            aw::presentation::ToolTone::Failure => Style::default()
                .fg(self.tool_error_foreground)
                .bg(self.tool_error_background),
            aw::presentation::ToolTone::Neutral => Style::default()
                .fg(self.tool_neutral_foreground)
                .bg(self.tool_neutral_background),
        }
    }

    pub fn menu_style(self) -> Style {
        Style::default()
            .fg(self.tool_neutral_foreground)
            .bg(self.tool_neutral_background)
    }

    pub fn menu_usage_style(self) -> Style {
        self.menu_style().add_modifier(Modifier::BOLD)
    }

    pub fn menu_description_style(self) -> Style {
        self.menu_style().fg(self.muted)
    }

    pub fn menu_selected_style(self) -> Style {
        Style::default()
            .fg(self.accent_foreground)
            .bg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    pub fn markdown_heading_style(self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    pub fn markdown_strong_style(self) -> Style {
        self.text_style().add_modifier(Modifier::BOLD)
    }

    pub fn markdown_emphasis_style(self) -> Style {
        self.text_style().add_modifier(Modifier::ITALIC)
    }

    pub fn markdown_code_style(self) -> Style {
        Style::default()
            .fg(self.tool_neutral_foreground)
            .bg(self.tool_neutral_background)
    }

    pub fn markdown_quote_style(self) -> Style {
        Style::default()
            .fg(self.muted)
            .add_modifier(Modifier::ITALIC)
    }

    pub fn markdown_link_style(self) -> Style {
        self.text_style().add_modifier(Modifier::UNDERLINED)
    }

    pub fn markdown_link_destination_style(self) -> Style {
        Style::default().fg(self.faint)
    }

    pub fn markdown_rule_style(self) -> Style {
        Style::default().fg(self.faint)
    }

    #[cfg(feature = "workspace")]
    pub fn scrollbar_track(self) -> Color {
        if self.background == Color::Reset {
            Color::DarkGray
        } else {
            self.faint
        }
    }

    #[cfg(feature = "workspace")]
    pub fn scrollbar_thumb(self) -> Color {
        self.accent
    }

    pub fn selected_style(self) -> Style {
        if self.background == Color::Reset {
            Style::default()
                .fg(self.foreground)
                .add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default()
                .fg(self.selection_foreground)
                .bg(self.selection_background)
                .add_modifier(Modifier::BOLD)
        }
    }
}

pub fn palette(mode: ThemeMode) -> Palette {
    match mode {
        ThemeMode::System => Palette {
            // The terminal owns the base palette. Semantic surfaces use named
            // ANSI colors so the terminal theme, not a COLORFGBG guess, decides
            // their actual RGB values.
            background: Color::Reset,
            #[cfg(feature = "workspace")]
            surface: Color::Reset,
            foreground: Color::Reset,
            muted: Color::DarkGray,
            faint: Color::Gray,
            accent: Color::Cyan,
            accent_foreground: Color::Black,
            user_prompt_foreground: Color::White,
            user_prompt_background: Color::Blue,
            #[cfg(feature = "workspace")]
            success: Color::Green,
            #[cfg(feature = "workspace")]
            warning: Color::Yellow,
            #[cfg(feature = "workspace")]
            danger: Color::Red,
            selection_foreground: Color::Reset,
            selection_background: Color::Reset,
            tool_foreground: Color::Green,
            tool_background: Color::Reset,
            tool_neutral_foreground: Color::DarkGray,
            tool_neutral_background: Color::Reset,
            tool_error_foreground: Color::Red,
            tool_error_background: Color::Reset,
        },
        ThemeMode::Light => Palette {
            background: Color::Rgb(252, 252, 251),
            #[cfg(feature = "workspace")]
            surface: Color::Rgb(244, 245, 242),
            foreground: Color::Rgb(36, 39, 37),
            muted: Color::Rgb(105, 113, 109),
            faint: Color::Rgb(144, 153, 147),
            accent: Color::Rgb(56, 104, 199),
            accent_foreground: Color::White,
            user_prompt_foreground: Color::Rgb(42, 55, 69),
            user_prompt_background: Color::Rgb(232, 240, 249),
            #[cfg(feature = "workspace")]
            success: Color::Rgb(60, 134, 97),
            #[cfg(feature = "workspace")]
            warning: Color::Rgb(138, 93, 36),
            #[cfg(feature = "workspace")]
            danger: Color::Rgb(170, 79, 72),
            selection_foreground: Color::Rgb(36, 39, 37),
            selection_background: Color::Rgb(223, 232, 226),
            tool_foreground: Color::Rgb(53, 105, 73),
            tool_background: Color::Rgb(231, 243, 235),
            tool_neutral_foreground: Color::Rgb(88, 96, 91),
            tool_neutral_background: Color::Rgb(239, 241, 239),
            tool_error_foreground: Color::Rgb(145, 65, 65),
            tool_error_background: Color::Rgb(250, 235, 235),
        },
        ThemeMode::Dark => Palette {
            background: Color::Rgb(24, 28, 26),
            #[cfg(feature = "workspace")]
            surface: Color::Rgb(31, 36, 33),
            foreground: Color::Rgb(237, 241, 238),
            muted: Color::Rgb(164, 173, 167),
            faint: Color::Rgb(120, 131, 124),
            accent: Color::Rgb(110, 168, 254),
            accent_foreground: Color::Rgb(18, 24, 21),
            user_prompt_foreground: Color::Rgb(224, 234, 244),
            user_prompt_background: Color::Rgb(31, 44, 58),
            #[cfg(feature = "workspace")]
            success: Color::Rgb(114, 201, 152),
            #[cfg(feature = "workspace")]
            warning: Color::Rgb(224, 179, 110),
            #[cfg(feature = "workspace")]
            danger: Color::Rgb(240, 138, 138),
            selection_foreground: Color::Rgb(237, 241, 238),
            selection_background: Color::Rgb(49, 64, 56),
            tool_foreground: Color::Rgb(194, 213, 199),
            tool_background: Color::Rgb(35, 48, 39),
            tool_neutral_foreground: Color::Rgb(188, 196, 191),
            tool_neutral_background: Color::Rgb(43, 48, 45),
            tool_error_foreground: Color::Rgb(244, 184, 188),
            tool_error_background: Color::Rgb(58, 38, 41),
        },
    }
}

/// Apply only already parsed/validated semantic accent metadata. A project
/// palette cannot inject terminal commands or modify status/color authority.
pub fn palette_for(ui: &aw::config::UiConfig) -> Palette {
    let mut resolved = palette(ui.theme);
    if let Some(hex) = &ui.theme_accent {
        // UiConfig::validate and the project theme parser enforce #RRGGBB.
        // On invalid runtime UI mutation, fail closed to the base scheme.
        if hex.len() == 7 && hex.starts_with('#') {
            if let Ok(rgb) = u32::from_str_radix(&hex[1..], 16) {
                resolved.accent = Color::Rgb(
                    ((rgb >> 16) & 255) as u8,
                    ((rgb >> 8) & 255) as u8,
                    (rgb & 255) as u8,
                );
            }
        }
    }
    resolved
}

/// Best-effort terminal-background hint used only for syntax theme selection.
/// System rendering itself leaves foreground/background as `Reset`, so the
/// terminal remains authoritative even when this hint is absent or wrong.
#[cfg(feature = "workspace")]
pub fn effective_syntax_theme(mode: ThemeMode) -> ThemeMode {
    match mode {
        ThemeMode::System => {
            if colorfgbg_prefers_light(std::env::var("COLORFGBG").ok().as_deref()) {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            }
        }
        other => other,
    }
}

#[cfg(feature = "workspace")]
fn colorfgbg_prefers_light(value: Option<&str>) -> bool {
    let Some(background) = value
        .and_then(|value| value.rsplit([';', ':']).next())
        .and_then(|value| value.trim().parse::<u8>().ok())
    else {
        return false;
    };
    matches!(background, 7 | 15)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_selected_named_semantic_accent_changes_one_palette_slot_only() {
        let mut ui = aw::config::UiConfig {
            theme: aw::config::ThemeMode::Light,
            ..Default::default()
        };
        let baseline = palette_for(&ui);
        ui.theme_accent = Some("#1199cc".into());
        ui.validate().unwrap();
        let themed = palette_for(&ui);
        assert_eq!(themed.accent, Color::Rgb(17, 153, 204));
        assert_eq!(themed.background, baseline.background);
        assert_eq!(themed.tool_background, baseline.tool_background);
        ui.theme_accent = Some("ESC[31m".into());
        assert!(ui.validate().is_err());
        assert_eq!(palette_for(&ui).accent, baseline.accent);
    }
    #[test]
    fn system_palette_keeps_terminal_foreground_and_background_authoritative() {
        let palette = palette(ThemeMode::System);
        assert_eq!(palette.background, Color::Reset);
        assert_eq!(palette.foreground, Color::Reset);
        assert_eq!(palette.user_prompt_background, Color::Blue);
        assert_eq!(palette.tool_background, Color::Reset);
        assert_eq!(palette.tool_neutral_background, Color::Reset);
        assert_eq!(palette.tool_error_background, Color::Reset);
        assert!(
            !matches!(palette.user_prompt_background, Color::Rgb(_, _, _)),
            "System semantic surfaces must not guess a fixed RGB terminal background"
        );
    }

    #[test]
    fn semantic_menu_and_tool_styles_are_distinct_in_explicit_themes() {
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let palette = palette(mode);
            let success = palette.tool_style(aw::presentation::ToolTone::Success);
            let failure = palette.tool_style(aw::presentation::ToolTone::Failure);
            let neutral = palette.tool_style(aw::presentation::ToolTone::Neutral);
            assert_ne!(success.bg, failure.bg);
            assert_ne!(success.bg, neutral.bg);
            assert_ne!(failure.bg, neutral.bg);
            let selected = palette.menu_selected_style();
            assert_eq!(selected.bg, Some(palette.accent));
            assert_eq!(selected.fg, Some(palette.accent_foreground));
            assert_eq!(
                palette.menu_style().bg,
                Some(palette.tool_neutral_background)
            );
            #[cfg(feature = "workspace")]
            {
                assert_ne!(palette.scrollbar_track(), palette.scrollbar_thumb());
                assert_eq!(palette.scrollbar_thumb(), palette.accent);
            }
        }
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn colorfgbg_hint_distinguishes_common_light_and_dark_terminal_backgrounds() {
        assert!(colorfgbg_prefers_light(Some("0;15")));
        assert!(colorfgbg_prefers_light(Some("15;7")));
        assert!(!colorfgbg_prefers_light(Some("15;0")));
        assert!(!colorfgbg_prefers_light(None));
    }
}
