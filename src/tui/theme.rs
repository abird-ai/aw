use aw::config::ThemeMode;
use ratatui::style::{Color, Modifier, Style};
use std::io::IsTerminal;

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
            faint: Color::Rgb(102, 112, 104),
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
            faint: Color::Rgb(146, 156, 149),
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

/// Terminal color depth is a presentation capability, not an authority
/// claim. On a real 16/256-color TTY, approximate explicit RGB themes
/// instead of emitting unsupported 24-bit escape sequences. TestBackend and
/// noninteractive output retain exact RGB for reproducible visual fixtures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColorDepth { Ansi16, Ansi256, TrueColor }

fn terminal_depth() -> ColorDepth {
    if let Ok(override_depth) = std::env::var("AW_UI_COLOR_DEPTH") {
        match override_depth.as_str() {
            "16" => return ColorDepth::Ansi16,
            "256" => return ColorDepth::Ansi256,
            "truecolor" => return ColorDepth::TrueColor,
            _ => {} // Ignore unsupported values; never execute user text.
        }
    }
    if !std::io::stdout().is_terminal() {
        return ColorDepth::TrueColor;
    }
    let color_term = std::env::var("COLORTERM").unwrap_or_default().to_ascii_lowercase();
    let term = std::env::var("TERM").unwrap_or_default().to_ascii_lowercase();
    if matches!(color_term.as_str(), "truecolor" | "24bit")
        || term.contains("direct") || term.contains("truecolor") {
        ColorDepth::TrueColor
    } else if term.contains("256color") {
        ColorDepth::Ansi256
    } else {
        ColorDepth::Ansi16
    }
}

fn limited_color(value: Color, depth: ColorDepth) -> Color {
    let Color::Rgb(r, g, b) = value else { return value };
    match depth {
        ColorDepth::TrueColor => value,
        ColorDepth::Ansi256 => {
            let cube = |channel: u8| -> u8 {
                if channel < 48 { 0 } else if channel < 115 { 1 }
                else { ((u16::from(channel) - 35) / 40).min(5) as u8 }
            };
            Color::Indexed(16 + 36 * cube(r) + 6 * cube(g) + cube(b))
        }
        ColorDepth::Ansi16 => {
            const CANDIDATES: [(u8,u8,u8,Color);16] = [
                (0,0,0,Color::Black), (170,0,0,Color::Red),
                (0,170,0,Color::Green), (170,85,0,Color::Yellow),
                (0,0,170,Color::Blue), (170,0,170,Color::Magenta),
                (0,170,170,Color::Cyan), (170,170,170,Color::Gray),
                (85,85,85,Color::DarkGray), (255,85,85,Color::LightRed),
                (85,255,85,Color::LightGreen), (255,255,85,Color::LightYellow),
                (85,85,255,Color::LightBlue), (255,85,255,Color::LightMagenta),
                (85,255,255,Color::LightCyan), (255,255,255,Color::White),
            ];
            let distance = |v: (u8,u8,u8,Color)| {
                let dr = i32::from(r) - i32::from(v.0);
                let dg = i32::from(g) - i32::from(v.1);
                let db = i32::from(b) - i32::from(v.2);
                2*dr*dr + 4*dg*dg + 3*db*db
            };
            CANDIDATES.iter().min_by_key(|candidate| distance(**candidate))
                .expect("fixed palette has 16 colors").3
        }
    }
}

impl Palette {
    fn limited(self, depth: ColorDepth, mode: ThemeMode) -> Self {
        if depth == ColorDepth::TrueColor || mode == ThemeMode::System {
            return self;
        }
        let map = |color| limited_color(color, depth);
        let mut projected = Self {
            background: map(self.background),
            #[cfg(feature = "workspace")]
            surface: map(self.surface),
            foreground: map(self.foreground),
            muted: map(self.muted),
            faint: map(self.faint),
            accent: map(self.accent),
            accent_foreground: map(self.accent_foreground),
            user_prompt_foreground: map(self.user_prompt_foreground),
            user_prompt_background: map(self.user_prompt_background),
            #[cfg(feature = "workspace")]
            success: map(self.success),
            #[cfg(feature = "workspace")]
            warning: map(self.warning),
            #[cfg(feature = "workspace")]
            danger: map(self.danger),
            selection_foreground: map(self.selection_foreground),
            selection_background: map(self.selection_background),
            tool_foreground: map(self.tool_foreground),
            tool_background: map(self.tool_background),
            tool_neutral_foreground: map(self.tool_neutral_foreground),
            tool_neutral_background: map(self.tool_neutral_background),
            tool_error_foreground: map(self.tool_error_foreground),
            tool_error_background: map(self.tool_error_background),
        };
        if depth == ColorDepth::Ansi16 {
            // Fixed 16-color terminal palettes vary by emulator. Favor basic
            // legible foreground/background pairs over subtle, low-contrast
            // near-gray text shades in a limited-color environment.
            let dark = mode == ThemeMode::Dark;
            projected.foreground = if dark { Color::White } else { Color::Black };
            projected.background = if dark { Color::Black } else { Color::White };
            projected.muted = if dark { Color::Gray } else { Color::Black };
            projected.faint = projected.muted;
            // Never collapse success and failure into one nearest-gray ANSI
            // code. Factual status words remain the primary distinction.
            projected.tool_foreground = if dark { Color::LightGreen } else { Color::Green };
            projected.tool_error_foreground = if dark { Color::LightRed } else { Color::Red };
            projected.tool_neutral_foreground = projected.muted;
            #[cfg(feature = "workspace")]
            {
                projected.success = projected.tool_foreground;
                projected.danger = projected.tool_error_foreground;
                projected.warning = if dark { Color::LightYellow } else { Color::Yellow };
            }
        }
        projected
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
    resolved.limited(terminal_depth(), ui.theme)
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
    fn explicit_light_dark_and_system_have_bounded_16_256_truecolor_fallback() {
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let rgb = palette(mode);
            let full = rgb.limited(ColorDepth::TrueColor, mode);
            assert_eq!(full.foreground, rgb.foreground);
            assert!(matches!(full.foreground, Color::Rgb(_, _, _)));
            let indexed = rgb.limited(ColorDepth::Ansi256, mode);
            assert!(matches!(indexed.foreground, Color::Indexed(16..=231)));
            assert!(matches!(indexed.background, Color::Indexed(16..=231)));
            assert_ne!(indexed.foreground, indexed.background);
            let basic = rgb.limited(ColorDepth::Ansi16, mode);
            assert_eq!(basic.foreground, if mode == ThemeMode::Dark {
                Color::White
            } else {
                Color::Black
            });
            assert_eq!(basic.background, if mode == ThemeMode::Dark {
                Color::Black
            } else {
                Color::White
            });
            assert_ne!(basic.tool_foreground, basic.tool_error_foreground);
        }
        let system = palette(ThemeMode::System);
        for depth in [ColorDepth::Ansi16, ColorDepth::Ansi256, ColorDepth::TrueColor] {
            let selected = system.limited(depth, ThemeMode::System);
            assert_eq!(selected.foreground, Color::Reset);
            assert_eq!(selected.background, Color::Reset);
            assert_eq!(selected.user_prompt_background, Color::Blue);
        }
        assert_eq!(limited_color(Color::Reset, ColorDepth::Ansi16), Color::Reset);
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
