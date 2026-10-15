//! Workspace adapters over the shared native syntax-highlighting service.
//!
//! File/diff text is already loaded by explicit human workspace operations.
//! This layer adds Git/file presentation semantics only.

use aw::config::ThemeMode;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

#[cfg(test)]
pub fn highlight(
    path: &str,
    text: &str,
    theme_mode: ThemeMode,
) -> Option<Vec<Line<'static>>> {
    crate::tui::syntax::highlight_path(path, text, theme_mode)
}

fn diff_semantics(
    line: &aw_git::ReviewLine,
    theme_mode: ThemeMode,
) -> (&'static str, Color, Option<Color>) {
    let palette = crate::tui::theme::palette(theme_mode);
    let effective = crate::tui::theme::effective_syntax_theme(theme_mode);
    let added_background = match theme_mode {
        ThemeMode::System => None,
        _ if effective == ThemeMode::Light => Some(Color::Rgb(231, 243, 235)),
        _ => Some(Color::Rgb(29, 52, 37)),
    };
    let removed_background = match theme_mode {
        ThemeMode::System => None,
        _ if effective == ThemeMode::Light => Some(Color::Rgb(250, 235, 235)),
        _ => Some(Color::Rgb(58, 37, 41)),
    };
    match line.kind {
        aw_git::LineKind::Added => ("+ ", palette.success, added_background),
        aw_git::LineKind::Removed => ("- ", palette.danger, removed_background),
        _ => ("  ", palette.muted, None),
    }
}

pub fn plain_diff_line(line: &aw_git::ReviewLine, theme_mode: ThemeMode) -> Line<'static> {
    if !matches!(
        line.kind,
        aw_git::LineKind::Added | aw_git::LineKind::Removed | aw_git::LineKind::Context
    ) {
        return Line::default();
    }
    let (marker, marker_color, background) = diff_semantics(line, theme_mode);
    let mut marker_style = Style::default()
        .fg(marker_color)
        .add_modifier(Modifier::BOLD);
    let mut code_style = Style::default();
    if let Some(background) = background {
        marker_style = marker_style.bg(background);
        code_style = code_style.bg(background);
    }
    Line::from(vec![
        Span::styled(marker, marker_style),
        Span::styled(
            aw_workspace::editor::display(line.text.get(1..).unwrap_or("")),
            code_style,
        ),
    ])
}

pub fn highlight_diff_progressive(
    path: &str,
    lines: &[aw_git::ReviewLine],
    theme_mode: ThemeMode,
    chunk_lines: usize,
    mut emit: impl FnMut(usize, Vec<Line<'static>>) -> bool,
) -> bool {
    let code = lines
        .iter()
        .map(|line| match line.kind {
            aw_git::LineKind::Added | aw_git::LineKind::Removed | aw_git::LineKind::Context => {
                line.text.get(1..).unwrap_or("")
            }
            _ => "",
        })
        .collect::<Vec<_>>()
        .join("\n");
    crate::tui::syntax::highlight_path_progressive(
        path,
        &code,
        theme_mode,
        chunk_lines,
        |start, highlighted| {
            let mut out = Vec::with_capacity(highlighted.len());
            for (offset, code_line) in highlighted.into_iter().enumerate() {
                let index = start + offset;
                let Some(review_line) = lines.get(index) else { break };
                if !matches!(
                    review_line.kind,
                    aw_git::LineKind::Added | aw_git::LineKind::Removed | aw_git::LineKind::Context
                ) {
                    out.push(Line::default());
                    continue;
                }
                let (marker, marker_color, background) = diff_semantics(review_line, theme_mode);
                let mut spans = vec![Span::styled(
                    marker,
                    Style::default()
                        .fg(marker_color)
                        .add_modifier(Modifier::BOLD),
                )];
                for mut span in code_line.spans {
                    if let Some(background) = background {
                        span.style = span.style.bg(background);
                    }
                    spans.push(span);
                }
                if let Some(background) = background {
                    spans[0].style = spans[0].style.bg(background);
                }
                out.push(Line::from(spans));
            }
            emit(start, out)
        },
    )
}

/// Syntax-highlight diff code while retaining semantic Git add/remove styling.
/// One output line corresponds to one input review line; headers/hunks remain
/// empty here so the caller can render their own metadata columns/colors.
#[cfg(test)]
pub fn highlight_diff(
    path: &str,
    lines: &[aw_git::ReviewLine],
    theme_mode: ThemeMode,
) -> Vec<Line<'static>> {
    let code = lines
        .iter()
        .map(|line| match line.kind {
            aw_git::LineKind::Added | aw_git::LineKind::Removed | aw_git::LineKind::Context => {
                line.text.get(1..).unwrap_or("")
            }
            _ => "",
        })
        .collect::<Vec<_>>()
        .join("\n");
    let highlighted = highlight(path, &code, theme_mode);
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            if !matches!(
                line.kind,
                aw_git::LineKind::Added | aw_git::LineKind::Removed | aw_git::LineKind::Context
            ) {
                return Line::default();
            }
            let marker = match line.kind {
                aw_git::LineKind::Added => "+ ",
                aw_git::LineKind::Removed => "- ",
                _ => "  ",
            };
            let palette = crate::tui::theme::palette(theme_mode);
            let effective = crate::tui::theme::effective_syntax_theme(theme_mode);
            let added_background = match theme_mode {
                ThemeMode::System => None,
                _ if effective == ThemeMode::Light => Some(Color::Rgb(231, 243, 235)),
                _ => Some(Color::Rgb(29, 52, 37)),
            };
            let removed_background = match theme_mode {
                ThemeMode::System => None,
                _ if effective == ThemeMode::Light => Some(Color::Rgb(250, 235, 235)),
                _ => Some(Color::Rgb(58, 37, 41)),
            };
            let (marker_color, background) = match line.kind {
                aw_git::LineKind::Added => (palette.success, added_background),
                aw_git::LineKind::Removed => (palette.danger, removed_background),
                _ => (palette.muted, None),
            };
            let mut spans = vec![Span::styled(
                marker,
                Style::default()
                    .fg(marker_color)
                    .add_modifier(Modifier::BOLD),
            )];
            if let Some(highlighted) = highlighted.as_ref().and_then(|rows| rows.get(index)) {
                for mut span in highlighted.spans.clone() {
                    if let Some(background) = background {
                        span.style = span.style.bg(background);
                    }
                    spans.push(span);
                }
            } else {
                let code = line.text.get(1..).unwrap_or("");
                let style = background.map_or(Style::default(), |bg| Style::default().bg(bg));
                spans.push(Span::styled(aw_workspace::editor::display(code), style));
            }
            Line::from(spans)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_file_gets_highlighted_without_ansi_or_background_escape_sequences() {
        let lines = highlight("src/lib.rs", "pub fn bird() -> usize { 1 }", ThemeMode::Dark).unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].spans.len() > 1);
        assert!(lines[0]
            .spans
            .iter()
            .all(|span| !span.content.contains('\u{1b}')));
    }

    #[test]
    fn explicit_light_and_dark_use_distinct_syntect_palettes() {
        let light = highlight("src/lib.rs", "pub fn bird() -> usize { 1 }", ThemeMode::Light)
            .unwrap();
        let dark = highlight("src/lib.rs", "pub fn bird() -> usize { 1 }", ThemeMode::Dark)
            .unwrap();
        assert_ne!(light[0].spans[0].style.fg, dark[0].spans[0].style.fg);
    }

    #[test]
    fn progressive_diff_highlighting_emits_multiple_bounded_chunks() {
        let lines = (0..300)
            .map(|index| aw_git::ReviewLine {
                kind: aw_git::LineKind::Added,
                old: None,
                new: Some(index + 1),
                text: format!("+pub fn bird_{index}() -> usize {{ {index} }}"),
            })
            .collect::<Vec<_>>();
        let mut starts = Vec::new();
        let mut rows = 0usize;
        assert!(highlight_diff_progressive(
            "src/lib.rs",
            &lines,
            ThemeMode::Dark,
            64,
            |start, chunk| {
                starts.push(start);
                rows += chunk.len();
                assert!(chunk.iter().all(|line| line.width() > 2));
                true
            },
        ));
        assert!(starts.len() >= 4);
        assert_eq!(rows, 300);
        assert!(starts.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn diff_highlight_keeps_syntax_spans_and_git_backgrounds() {
        let lines = vec![
            aw_git::ReviewLine {
                kind: aw_git::LineKind::Removed,
                old: Some(1),
                new: None,
                text: "-let bird = 1;".into(),
            },
            aw_git::ReviewLine {
                kind: aw_git::LineKind::Added,
                old: None,
                new: Some(1),
                text: "+let bird = 2;".into(),
            },
        ];
        let highlighted = highlight_diff("src/lib.rs", &lines, ThemeMode::Dark);
        assert_eq!(highlighted.len(), 2);
        assert!(highlighted[0].spans.len() > 2);
        assert!(highlighted[1].spans.len() > 2);
        assert!(highlighted[0].spans.iter().any(|span| span.style.bg.is_some()));
        assert!(highlighted[1].spans.iter().any(|span| span.style.bg.is_some()));
    }

    #[test]
    fn toml_nix_and_json_lockfiles_keep_shared_detection() {
        assert!(highlight(
            "Cargo.lock",
            "version = 4\n[[package]]\nname = \"aw\"",
            ThemeMode::Dark
        )
        .is_some());
        assert!(highlight(
            "flake.nix",
            "{ pkgs, ... }: { environment.systemPackages = [ pkgs.git ]; }",
            ThemeMode::Dark
        )
        .is_some());
        assert!(highlight(
            "flake.lock",
            "{\"nodes\":{\"root\":{}}}",
            ThemeMode::Dark
        )
        .is_some());
    }

    #[test]
    fn unknown_plain_file_falls_back_to_caller() {
        assert!(highlight("notes.unknown-aw-extension", "hello", ThemeMode::System).is_none());
    }
}
