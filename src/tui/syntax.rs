//! Shared read-only syntax highlighting for native human-visible text.
//!
//! The caller already owns/has authorized the displayed text. This module has
//! no filesystem, model, provider, network, process or command authority.

use aw::config::ThemeMode;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::{path::Path, sync::OnceLock};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, ThemeSet},
    parsing::{SyntaxDefinition, SyntaxReference, SyntaxSet},
};

static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
static THEMES: OnceLock<ThemeSet> = OnceLock::new();

const MAX_LANGUAGE_TOKEN_BYTES: usize = 64;

const TOML_SYNTAX: &str = r#"
name: TOML
scope: source.toml
file_extensions: [toml]
contexts:
  main:
    - match: '#.*$'
      scope: comment.line.number-sign.toml
    - match: '^\s*\[\[?.*?\]?\]\s*$'
      scope: entity.name.section.toml
    - match: '"(?:\\.|[^"\\])*"'
      scope: string.quoted.double.toml
    - match: "'[^']*'"
      scope: string.quoted.single.toml
    - match: '\b(true|false)\b'
      scope: constant.language.boolean.toml
    - match: '[-+]?\b[0-9]+(?:\.[0-9]+)?\b'
      scope: constant.numeric.toml
    - match: '^[ \t]*[A-Za-z0-9_.-]+(?=\s*=)'
      scope: variable.other.key.toml
    - match: '='
      scope: keyword.operator.assignment.toml
"#;

const NIX_SYNTAX: &str = r#"
name: Nix
scope: source.nix
file_extensions: [nix]
contexts:
  main:
    - match: '#.*$'
      scope: comment.line.number-sign.nix
    - match: '"(?:\\.|[^"\\])*"'
      scope: string.quoted.double.nix
    - match: "''.*?''"
      scope: string.quoted.other.nix
    - match: '\b(let|in|with|rec|inherit|if|then|else|assert|or)\b'
      scope: keyword.control.nix
    - match: '\bbuiltins\b'
      scope: support.variable.nix
    - match: '\b(true|false|null)\b'
      scope: constant.language.nix
    - match: '[-+]?\b[0-9]+(?:\.[0-9]+)?\b'
      scope: constant.numeric.nix
    - match: '\$\{'
      scope: punctuation.section.interpolation.begin.nix
    - match: '[A-Za-z_][A-Za-z0-9_-]*(?=\s*=)'
      scope: variable.other.attribute.nix
    - match: '(?:\./|\.\./|/)[^\s;)}]+'
      scope: string.unquoted.path.nix
    - match: '='
      scope: keyword.operator.assignment.nix
"#;

fn syntax_set() -> &'static SyntaxSet {
    SYNTAXES.get_or_init(|| {
        let mut builder = SyntaxSet::load_defaults_nonewlines().into_builder();
        builder.add(
            SyntaxDefinition::load_from_str(TOML_SYNTAX, false, Some("TOML"))
                .expect("embedded TOML syntax is valid"),
        );
        builder.add(
            SyntaxDefinition::load_from_str(NIX_SYNTAX, false, Some("Nix"))
                .expect("embedded Nix syntax is valid"),
        );
        builder.build()
    })
}

fn theme_set() -> &'static ThemeSet {
    THEMES.get_or_init(ThemeSet::load_defaults)
}

fn theme(mode: ThemeMode) -> Option<&'static syntect::highlighting::Theme> {
    let themes = theme_set();
    let effective = super::theme::effective_syntax_theme(mode);
    let name = match effective {
        ThemeMode::Light => "InspiredGitHub",
        ThemeMode::Dark | ThemeMode::System => "base16-ocean.dark",
    };
    themes
        .themes
        .get(name)
        .or_else(|| themes.themes.values().next())
}

fn ratatui_style(style: syntect::highlighting::Style) -> Style {
    let mut out = Style::default().fg(Color::Rgb(
        style.foreground.r,
        style.foreground.g,
        style.foreground.b,
    ));
    if style.font_style.contains(FontStyle::BOLD) {
        out = out.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        out = out.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        out = out.add_modifier(Modifier::UNDERLINED);
    }
    out
}

fn detect_path<'a>(
    syntaxes: &'a SyntaxSet,
    path: &str,
    text: &str,
) -> Option<&'a SyntaxReference> {
    let path = Path::new(path);
    let filename = path.file_name().and_then(|value| value.to_str()).unwrap_or("");
    if filename.eq_ignore_ascii_case("Cargo.lock") {
        return syntaxes.find_syntax_by_name("TOML");
    }
    if filename.eq_ignore_ascii_case("flake.lock") {
        return syntaxes.find_syntax_by_name("JSON");
    }
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        if let Some(syntax) = syntaxes.find_syntax_by_extension(extension) {
            if syntax.name != "Plain Text" {
                return Some(syntax);
            }
        }
    }
    if serde_json::from_str::<serde_json::Value>(text).is_ok() {
        return syntaxes.find_syntax_by_name("JSON");
    }
    text.lines()
        .next()
        .and_then(|line| syntaxes.find_syntax_by_first_line(line))
        .filter(|syntax| syntax.name != "Plain Text")
}

fn normalized_language(language: &str) -> Option<&str> {
    let token = language.trim();
    if token.is_empty()
        || token.len() > MAX_LANGUAGE_TOKEN_BYTES
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"+#._-".contains(&byte))
    {
        return None;
    }
    Some(match token.to_ascii_lowercase().as_str() {
        "rs" | "rust" => "rs",
        "js" | "javascript" => "js",
        "ts" | "typescript" => "ts",
        "py" | "python" => "py",
        "sh" | "bash" | "shell" | "zsh" => "sh",
        "json" => "json",
        "toml" => "toml",
        "nix" => "nix",
        "yaml" | "yml" => "yaml",
        "md" | "markdown" => "md",
        _ => return Some(token),
    })
}

fn detect_language<'a>(
    syntaxes: &'a SyntaxSet,
    language: &str,
) -> Option<&'a SyntaxReference> {
    let token = normalized_language(language)?;
    syntaxes
        .find_syntax_by_token(token)
        .or_else(|| {
            if token.eq_ignore_ascii_case("ts") {
                syntaxes.find_syntax_by_token("js")
            } else {
                None
            }
        })
        .filter(|syntax| syntax.name != "Plain Text")
}

fn highlight_with(
    syntax: &SyntaxReference,
    text: &str,
    mode: ThemeMode,
) -> Option<Vec<Line<'static>>> {
    let mut lines = Vec::new();
    highlight_with_progressive(syntax, text, mode, 8192, |_, chunk| {
        lines.extend(chunk);
        true
    })?;
    Some(lines)
}

fn highlight_with_progressive(
    syntax: &SyntaxReference,
    text: &str,
    mode: ThemeMode,
    chunk_lines: usize,
    mut emit: impl FnMut(usize, Vec<Line<'static>>) -> bool,
) -> Option<()> {
    let syntaxes = syntax_set();
    let theme = theme(mode)?;
    let mut highlighter = HighlightLines::new(syntax, theme);
    let chunk_lines = chunk_lines.clamp(1, 512);
    let mut start = 0usize;
    let mut chunk = Vec::with_capacity(chunk_lines);
    for (index, line) in text.split('\n').take(8192).enumerate() {
        let spans = match highlighter.highlight_line(line, syntaxes) {
            Ok(regions) => regions
                .into_iter()
                .map(|(style, token)| {
                    Span::styled(
                        aw_workspace::editor::display(token),
                        ratatui_style(style),
                    )
                })
                .collect::<Vec<_>>(),
            Err(_) => vec![Span::raw(aw_workspace::editor::display(line))],
        };
        if chunk.is_empty() {
            start = index;
        }
        chunk.push(Line::from(spans));
        if chunk.len() >= chunk_lines {
            if !emit(start, std::mem::take(&mut chunk)) {
                return Some(());
            }
            chunk = Vec::with_capacity(chunk_lines);
        }
    }
    if !chunk.is_empty() {
        let _ = emit(start, chunk);
    }
    Some(())
}

pub fn highlight_path_progressive(
    path: &str,
    text: &str,
    mode: ThemeMode,
    chunk_lines: usize,
    emit: impl FnMut(usize, Vec<Line<'static>>) -> bool,
) -> bool {
    let Some(syntax) = detect_path(syntax_set(), path, text) else {
        return false;
    };
    highlight_with_progressive(syntax, text, mode, chunk_lines, emit).is_some()
}

#[cfg(test)]
pub fn highlight_path(
    path: &str,
    text: &str,
    mode: ThemeMode,
) -> Option<Vec<Line<'static>>> {
    let syntax = detect_path(syntax_set(), path, text)?;
    highlight_with(syntax, text, mode)
}

pub fn highlight_language(
    language: &str,
    text: &str,
    mode: ThemeMode,
) -> Option<Vec<Line<'static>>> {
    let syntax = detect_language(syntax_set(), language)?;
    highlight_with(syntax, text, mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_aliases_are_bounded_and_local() {
        let syntaxes = syntax_set();
        assert_eq!(detect_language(syntaxes, "rust").unwrap().name, "Rust");
        assert_eq!(detect_language(syntaxes, "rs").unwrap().name, "Rust");
        assert_eq!(detect_language(syntaxes, "python").unwrap().name, "Python");
        assert_eq!(detect_language(syntaxes, "py").unwrap().name, "Python");
        assert_eq!(detect_language(syntaxes, "javascript").unwrap().name, "JavaScript");
        assert_eq!(detect_language(syntaxes, "js").unwrap().name, "JavaScript");
        assert!(detect_language(syntaxes, "typescript").is_some());
        assert!(detect_language(syntaxes, "ts").is_some());
        assert!(detect_language(syntaxes, "bash").is_some());
        assert!(detect_language(syntaxes, "shell").is_some());
        assert_eq!(detect_language(syntaxes, "json").unwrap().name, "JSON");
        assert_eq!(detect_language(syntaxes, "toml").unwrap().name, "TOML");
        assert_eq!(detect_language(syntaxes, "nix").unwrap().name, "Nix");
        assert!(detect_language(syntaxes, "yaml").is_some());
        assert!(detect_language(syntaxes, "yml").is_some());
        assert!(detect_language(syntaxes, "markdown").is_some());
        assert!(detect_language(syntaxes, "md").is_some());
        assert!(detect_language(syntaxes, "unknown-aw-language").is_none());
        assert!(detect_language(syntaxes, &"x".repeat(65)).is_none());
        assert!(detect_language(syntaxes, "../../rust").is_none());
    }

    #[test]
    fn progressive_path_highlighting_emits_multiple_ordered_chunks() {
        let text = (0..300)
            .map(|index| format!("pub fn bird_{index}() -> usize {{ {index} }}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut starts = Vec::new();
        let mut rows = 0usize;
        assert!(highlight_path_progressive(
            "src/lib.rs",
            &text,
            ThemeMode::Dark,
            64,
            |start, chunk| {
                starts.push(start);
                rows += chunk.len();
                true
            },
        ));
        assert!(starts.len() >= 4, "large fixture should highlight progressively");
        assert_eq!(starts[0], 0);
        assert!(starts.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(rows, 300);
    }

    #[test]
    fn rust_fence_highlighting_is_theme_aware_and_terminal_safe() {
        let code = "pub fn bird() -> usize { 1 }";
        let dark = highlight_language("rust", code, ThemeMode::Dark).unwrap();
        let light = highlight_language("rust", code, ThemeMode::Light).unwrap();
        assert!(dark[0].spans.len() > 1);
        assert!(light[0].spans.len() > 1);
        assert_ne!(dark[0].spans[0].style.fg, light[0].spans[0].style.fg);
        assert!(dark[0]
            .spans
            .iter()
            .all(|span| !span.content.contains('\u{1b}')));
    }

    #[test]
    fn path_detection_preserves_toml_nix_and_json_lock_contract() {
        let syntaxes = syntax_set();
        assert_eq!(
            detect_path(
                syntaxes,
                "Cargo.lock",
                "version = 4\n[[package]]\nname = \"aw\""
            )
            .unwrap()
            .name,
            "TOML"
        );
        assert_eq!(
            detect_path(
                syntaxes,
                "flake.nix",
                "{ pkgs, ... }: { environment.systemPackages = [ pkgs.git ]; }"
            )
            .unwrap()
            .name,
            "Nix"
        );
        assert_eq!(
            detect_path(syntaxes, "flake.lock", "{\"nodes\":{\"root\":{}}}")
                .unwrap()
                .name,
            "JSON"
        );
    }
}
