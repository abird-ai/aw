use super::theme::Palette;
use aw::config::ThemeMode;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone)]
struct Cell {
    text: String,
    style: Style,
}

fn cells(spans: Vec<Span<'static>>) -> Vec<Cell> {
    let mut out = Vec::new();
    for span in spans {
        let text = span.content.into_owned();
        for grapheme in UnicodeSegmentation::graphemes(text.as_str(), true) {
            if grapheme == "\t" {
                for _ in 0..4 {
                    out.push(Cell {
                        text: " ".into(),
                        style: span.style,
                    });
                }
            } else {
                out.push(Cell {
                    text: grapheme.to_owned(),
                    style: span.style,
                });
            }
        }
    }
    out
}

fn cell_width(cell: &Cell) -> usize {
    UnicodeWidthStr::width(cell.text.as_str())
}

fn cells_to_line(cells: &[Cell]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for cell in cells {
        if let Some(last) = spans.last_mut() {
            if last.style == cell.style {
                let mut text = last.content.clone().into_owned();
                text.push_str(&cell.text);
                last.content = text.into();
                continue;
            }
        }
        spans.push(Span::styled(cell.text.clone(), cell.style));
    }
    Line::from(spans)
}

fn hard_wrap_cells(cells: &[Cell], width: usize) -> Vec<Vec<Cell>> {
    let width = width.max(1);
    if cells.is_empty() {
        return vec![Vec::new()];
    }
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut used = 0usize;
    for cell in cells {
        let mut projected = cell.clone();
        let mut cw = cell_width(&projected);
        if cw > width {
            projected.text = "…".into();
            cw = 1;
        }
        if !row.is_empty() && used.saturating_add(cw) > width {
            rows.push(row);
            row = Vec::new();
            used = 0;
        }
        row.push(projected);
        used = used.saturating_add(cw);
    }
    if !row.is_empty() {
        rows.push(row);
    }
    rows
}

fn soft_wrap_cells(cells: &[Cell], width: usize) -> Vec<Vec<Cell>> {
    let width = width.max(1);
    if cells.is_empty() {
        return vec![Vec::new()];
    }
    let cells = cells
        .iter()
        .cloned()
        .map(|mut cell| {
            if cell_width(&cell) > width {
                cell.text = "…".into();
            }
            cell
        })
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    let mut start = 0usize;
    while start < cells.len() {
        while start < cells.len()
            && cells[start].text.chars().all(char::is_whitespace)
            && cells[start].text != "\u{00a0}"
        {
            start += 1;
        }
        if start >= cells.len() {
            break;
        }
        let mut end = start;
        let mut used = 0usize;
        let mut last_space = None;
        while end < cells.len() {
            let cw = cell_width(&cells[end]);
            if end > start && used.saturating_add(cw) > width {
                break;
            }
            used = used.saturating_add(cw);
            if cells[end].text.chars().all(char::is_whitespace) {
                last_space = Some(end);
            }
            end += 1;
            if used >= width {
                break;
            }
        }
        if end < cells.len() {
            if let Some(space) = last_space.filter(|space| *space > start) {
                end = space;
            }
        }
        if end == start {
            end += 1;
        }
        let mut row = cells[start..end].to_vec();
        while row.last().is_some_and(|cell| {
            cell.text.chars().all(char::is_whitespace) && cell.text != "\u{00a0}"
        }) {
            row.pop();
        }
        rows.push(row);
        start = end;
    }
    if rows.is_empty() {
        rows.push(Vec::new());
    }
    rows
}

fn wrap_with_prefix(
    prefix: Vec<Span<'static>>,
    content: Vec<Span<'static>>,
    width: usize,
    preserve_spaces: bool,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let prefix_cells = cells(prefix);
    let prefix_width = prefix_cells.iter().map(cell_width).sum::<usize>();
    let available = width.saturating_sub(prefix_width).max(1);
    let content_cells = cells(content);
    let wrapped = if preserve_spaces {
        hard_wrap_cells(&content_cells, available)
    } else {
        soft_wrap_cells(&content_cells, available)
    };
    let mut out = Vec::with_capacity(wrapped.len());
    for (index, row) in wrapped.into_iter().enumerate() {
        let mut all = if index == 0 {
            prefix_cells.clone()
        } else {
            prefix_cells
                .iter()
                .map(|cell| Cell {
                    text: " ".repeat(cell_width(cell)),
                    style: cell.style,
                })
                .collect()
        };
        all.extend(row);
        out.push(cells_to_line(&all));
    }
    out
}

fn find_closing(input: &str, from: usize, marker: &str) -> Option<usize> {
    input[from..].find(marker).map(|offset| from + offset)
}

fn push_plain(buf: &mut String, spans: &mut Vec<Span<'static>>, style: Style) {
    if !buf.is_empty() {
        spans.push(Span::styled(std::mem::take(buf), style));
    }
}

fn inline_spans(input: &str, palette: Palette, base: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut i = 0usize;
    while i < input.len() {
        let rest = &input[i..];
        if rest.starts_with('\\') {
            let mut chars = rest.chars();
            let _ = chars.next();
            if let Some(next) = chars.next() {
                if r#"\`*_{}[]()#+-.!>~"#.contains(next) {
                    plain.push(next);
                    i += 1 + next.len_utf8();
                    continue;
                }
            }
        }
        if rest.starts_with("**") || rest.starts_with("__") {
            let marker = &rest[..2];
            if let Some(close) = find_closing(input, i + 2, marker) {
                if close > i + 2 {
                    push_plain(&mut plain, &mut spans, base);
                    spans.push(Span::styled(
                        input[i + 2..close].to_owned(),
                        palette.markdown_strong_style(),
                    ));
                    i = close + 2;
                    continue;
                }
            }
        }
        if rest.starts_with('`') {
            if let Some(close) = find_closing(input, i + 1, "`") {
                if close > i + 1 {
                    push_plain(&mut plain, &mut spans, base);
                    spans.push(Span::styled(
                        input[i + 1..close].to_owned(),
                        palette.markdown_code_style(),
                    ));
                    i = close + 1;
                    continue;
                }
            }
        }
        if rest.starts_with("~~") {
            if let Some(close) = find_closing(input, i + 2, "~~") {
                if close > i + 2 {
                    push_plain(&mut plain, &mut spans, base);
                    spans.push(Span::styled(
                        input[i + 2..close].to_owned(),
                        base.add_modifier(Modifier::CROSSED_OUT),
                    ));
                    i = close + 2;
                    continue;
                }
            }
        }
        if rest.starts_with("![") {
            if let Some(label_end_rel) = rest.find("](") {
                let label_end = i + label_end_rel;
                let destination_start = label_end + 2;
                if let Some(destination_end_rel) = input[destination_start..].find(')') {
                    let destination_end = destination_start + destination_end_rel;
                    if destination_end > destination_start {
                        push_plain(&mut plain, &mut spans, base);
                        let alt = &input[i + 2..label_end];
                        spans.push(Span::styled(
                            format!("[image: {}]", if alt.is_empty() { "media" } else { alt }),
                            palette.markdown_link_style(),
                        ));
                        spans.push(Span::styled(
                            format!(" ({})", &input[destination_start..destination_end]),
                            palette.markdown_link_destination_style(),
                        ));
                        i = destination_end + 1;
                        continue;
                    }
                }
            }
        }
        if rest.starts_with('[') {
            if let Some(label_end_rel) = rest.find("](") {
                let label_end = i + label_end_rel;
                let destination_start = label_end + 2;
                if let Some(destination_end_rel) = input[destination_start..].find(')') {
                    let destination_end = destination_start + destination_end_rel;
                    if label_end > i + 1 && destination_end > destination_start {
                        push_plain(&mut plain, &mut spans, base);
                        spans.push(Span::styled(
                            input[i + 1..label_end].to_owned(),
                            palette.markdown_link_style(),
                        ));
                        spans.push(Span::styled(
                            format!(" ({})", &input[destination_start..destination_end]),
                            palette.markdown_link_destination_style(),
                        ));
                        i = destination_end + 1;
                        continue;
                    }
                }
            }
        }
        if rest.starts_with('*') || rest.starts_with('_') {
            let marker = &rest[..1];
            if let Some(close) = find_closing(input, i + 1, marker) {
                if close > i + 1 {
                    push_plain(&mut plain, &mut spans, base);
                    spans.push(Span::styled(
                        input[i + 1..close].to_owned(),
                        palette.markdown_emphasis_style(),
                    ));
                    i = close + 1;
                    continue;
                }
            }
        }
        let ch = rest.chars().next().expect("nonempty rest");
        plain.push(ch);
        i += ch.len_utf8();
    }
    push_plain(&mut plain, &mut spans, base);
    spans
}

fn fence(line: &str) -> Option<(char, usize, &str)> {
    let trimmed = line.trim_start();
    let first = trimmed.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|ch| *ch == first).count();
    if count < 3 {
        return None;
    }
    let offset = first.len_utf8() * count;
    Some((first, count, trimmed[offset..].trim()))
}

fn heading(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let count = trimmed.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&count) {
        return None;
    }
    trimmed[count..].strip_prefix(' ')
}

fn list_indent(line: &str) -> usize {
    line.chars().take_while(|ch| ch.is_whitespace()).count().div_euclid(2).min(4)
}

fn unordered(line: &str) -> Option<(String, &str)> {
    let depth = list_indent(line);
    let trimmed = line.trim_start();
    for marker in ["- ", "* ", "+ "] {
        if let Some(content) = trimmed.strip_prefix(marker) {
            let (glyph, content) = if let Some(rest) = content.strip_prefix("[ ] ") {
                ("☐", rest)
            } else if let Some(rest) = content.strip_prefix("[x] ").or_else(|| content.strip_prefix("[X] ")) {
                ("☑", rest)
            } else {
                ("•", content)
            };
            return Some((format!("{}{} ", "  ".repeat(depth), glyph), content));
        }
    }
    None
}

fn ordered(line: &str) -> Option<(String, &str)> {
    let depth = list_indent(line);
    let trimmed = line.trim_start();
    let digits = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let content = trimmed[digits..].strip_prefix(". ")?;
    Some((format!("{}{}. ", "  ".repeat(depth), &trimmed[..digits]), content))
}

fn table_cells(line: &str) -> Option<Vec<&str>> {
    if !line.contains('|') { return None; }
    let trimmed = line.trim().trim_matches('|');
    let cells = trimmed.split('|').map(str::trim).collect::<Vec<_>>();
    (cells.len() >= 2).then_some(cells)
}

fn table_separator(line: &str) -> bool {
    let Some(cells) = table_cells(line) else { return false; };
    cells.iter().all(|cell| {
        let core = cell.trim_matches(':');
        core.len() >= 3 && core.chars().all(|ch| ch == '-')
    })
}

fn render_table_row(cells: &[&str], palette: Palette, width: usize, header: bool) -> Vec<Line<'static>> {
    let style = if header { palette.markdown_heading_style() } else { palette.text_style() };
    let joined = cells.join(" │ ");
    wrap_with_prefix(
        vec![Span::styled("│ ", palette.user_marker_style())],
        inline_spans(&joined, palette, style),
        width,
        false,
    )
}

fn horizontal_rule(line: &str) -> bool {
    let compact = line.chars().filter(|ch| !ch.is_whitespace()).collect::<String>();
    compact.len() >= 3
        && (compact.chars().all(|ch| ch == '-')
            || compact.chars().all(|ch| ch == '*')
            || compact.chars().all(|ch| ch == '_'))
}

fn fill_line(mut line: Line<'static>, style: Style, width: usize) -> Line<'static> {
    let used = line.width();
    if used < width {
        line.spans
            .push(Span::styled(" ".repeat(width - used), style));
    }
    line
}

fn bounded_language(language: &str) -> Option<&str> {
    let token = language.trim();
    if token.is_empty()
        || token.len() > 64
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"+#._-".contains(&byte))
    {
        return None;
    }
    Some(token)
}

fn code_source_spans(
    language: Option<&str>,
    code: &str,
    theme_mode: ThemeMode,
) -> Option<Vec<Line<'static>>> {
    #[cfg(feature = "workspace")]
    {
        language.and_then(|language| {
            crate::tui::syntax::highlight_language(language, code, theme_mode)
        })
    }
    #[cfg(not(feature = "workspace"))]
    {
        let _ = (language, code, theme_mode);
        None
    }
}

fn render_code_block(
    language: &str,
    source: &[&str],
    palette: Palette,
    theme_mode: ThemeMode,
    width: usize,
    syntax_enabled: bool,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let code_style = palette.markdown_code_style();
    let language = bounded_language(language);
    let mut out = Vec::new();

    if let Some(language) = language {
        for line in wrap_with_prefix(
            vec![Span::styled("  ", code_style)],
            vec![Span::styled(
                language.to_owned(),
                code_style.add_modifier(Modifier::ITALIC),
            )],
            width,
            true,
        ) {
            out.push(fill_line(line, code_style, width));
        }
    }

    let code = source.join("\n");
    let highlighted = syntax_enabled
        .then(|| code_source_spans(language, &code, theme_mode))
        .flatten();
    for (index, raw) in source.iter().enumerate() {
        let spans = highlighted
            .as_ref()
            .and_then(|rows| rows.get(index))
            .map(|line| {
                line.spans
                    .iter()
                    .cloned()
                    .map(|mut span| {
                        span.style = span.style.bg(palette.tool_neutral_background);
                        span
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![Span::styled((*raw).to_owned(), code_style)]);
        for line in wrap_with_prefix(
            vec![Span::styled("  ", code_style)],
            spans,
            width,
            true,
        ) {
            out.push(fill_line(line, code_style, width));
        }
    }

    if source.is_empty() {
        out.push(fill_line(Line::styled("  ", code_style), code_style, width));
    }
    out
}

fn render_impl(
    text: &str,
    palette: Palette,
    theme_mode: ThemeMode,
    width: usize,
    syntax_enabled: bool,
) -> Vec<Line<'static>> {
    let safe = aw::presentation::safe_text(text);
    let width = width.max(1);
    let mut out = Vec::new();
    let source = safe.split('\n').collect::<Vec<_>>();
    let mut index = 0usize;

    while index < source.len() {
        let line = source[index];

        if let Some((marker, count, language)) = fence(line) {
            index += 1;
            let mut code = Vec::new();
            while index < source.len() {
                if let Some((candidate, candidate_count, _)) = fence(source[index]) {
                    if candidate == marker && candidate_count >= count {
                        index += 1;
                        break;
                    }
                }
                code.push(source[index]);
                index += 1;
            }
            out.extend(render_code_block(
                language,
                &code,
                palette,
                theme_mode,
                width,
                syntax_enabled,
            ));
            continue;
        }

        if index + 1 < source.len() && table_cells(line).is_some() && table_separator(source[index + 1]) {
            let header = table_cells(line).expect("checked table header");
            out.extend(render_table_row(&header, palette, width, true));
            out.push(Line::styled("─".repeat(width.min(72)), palette.markdown_rule_style()));
            index += 2;
            while index < source.len() {
                let Some(row) = table_cells(source[index]) else { break; };
                out.extend(render_table_row(&row, palette, width, false));
                index += 1;
            }
            continue;
        }

        if line.trim().is_empty() {
            out.push(Line::default());
            index += 1;
            continue;
        }
        if horizontal_rule(line) {
            out.push(Line::styled(
                "─".repeat(width.min(72)),
                palette.markdown_rule_style(),
            ));
            index += 1;
            continue;
        }
        if let Some(content) = heading(line) {
            out.extend(wrap_with_prefix(
                Vec::new(),
                inline_spans(content, palette, palette.markdown_heading_style()),
                width,
                false,
            ));
            index += 1;
            continue;
        }
        let trimmed = line.trim_start();
        if let Some(content) = trimmed.strip_prefix("> ") {
            out.extend(wrap_with_prefix(
                vec![Span::styled("│ ", palette.user_marker_style())],
                inline_spans(content, palette, palette.markdown_quote_style()),
                width,
                false,
            ));
            index += 1;
            continue;
        }
        if let Some((marker, content)) = unordered(line) {
            out.extend(wrap_with_prefix(
                vec![Span::styled(marker, palette.user_marker_style())],
                inline_spans(content, palette, palette.text_style()),
                width,
                false,
            ));
            index += 1;
            continue;
        }
        if let Some((marker, content)) = ordered(line) {
            out.extend(wrap_with_prefix(
                vec![Span::styled(marker, palette.user_marker_style())],
                inline_spans(content, palette, palette.text_style()),
                width,
                false,
            ));
            index += 1;
            continue;
        }
        out.extend(wrap_with_prefix(
            Vec::new(),
            inline_spans(line, palette, palette.text_style()),
            width,
            false,
        ));
        index += 1;
    }

    if out.is_empty() && !safe.is_empty() {
        out.push(Line::styled(safe, palette.text_style()));
    }
    out
}

pub fn render(
    text: &str,
    palette: Palette,
    theme_mode: ThemeMode,
    width: usize,
) -> Vec<Line<'static>> {
    render_impl(text, palette, theme_mode, width, true)
}

pub fn render_plain(
    text: &str,
    palette: Palette,
    theme_mode: ThemeMode,
    width: usize,
) -> Vec<Line<'static>> {
    render_impl(text, palette, theme_mode, width, false)
}

#[cfg(any(feature = "workspace", test))]
pub fn plain_lines(lines: &[Line<'_>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .trim_end_matches(' ')
                .to_owned()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line<'_>]) -> String {
        plain_lines(lines).join("\n")
    }

    #[test]
    fn native_markdown_projects_core_pi_codex_shapes() {
        let mode = aw::config::ThemeMode::Dark;
        let palette = super::super::theme::palette(mode);
        let input = "# Heading\n\n- **bold** and *emphasis* with `code`\n1. ordered\n> quote\n\n```rust\nfn bird() { println!(\"ok\"); }\n```\n\n---\n[OpenAI](https://openai.com)";
        let lines = render(input, palette, mode, 80);
        let visible = text(&lines);
        assert!(visible.contains("Heading"));
        assert!(visible.contains("• bold and emphasis with code"));
        assert!(visible.contains("1. ordered"));
        assert!(visible.contains("│ quote"));
        assert!(visible.contains("rust"));
        assert!(visible.contains("fn bird()"));
        assert!(visible.contains("OpenAI (https://openai.com)"));
        assert!(!visible.contains("```"));
        assert!(!visible.contains("**"));
        assert!(!visible.contains("\u{1b}"));
    }

    #[test]
    fn rich_markdown_tables_tasks_strike_nested_lists_and_media_are_safe() {
        let mode = aw::config::ThemeMode::Dark;
        let palette = super::super::theme::palette(mode);
        let input = "| Name | State |\n| --- | --- |\n| bird | ready |\n- [ ] todo\n  - [x] nested done\n~~old~~ new\n![diagram](https://example.test/diagram.png)";
        let lines = render(input, palette, mode, 44);
        let visible = text(&lines);
        assert!(visible.contains("│ Name │ State"));
        assert!(visible.contains("│ bird │ ready"));
        assert!(visible.contains("☐ todo"));
        assert!(visible.contains("  ☑ nested done"));
        assert!(visible.contains("old new"));
        assert!(visible.contains("[image: diagram]"));
        assert!(visible.contains("https://example.test/diagram.png"));
        assert!(lines.iter().all(|line| line.width() <= 44));
        assert!(!visible.contains('\u{1b}'));
    }

    #[test]
    fn malformed_markup_and_html_stay_visible_literal_text() {
        let mode = aw::config::ThemeMode::Light;
        let palette = super::super::theme::palette(mode);
        let input = "<script>alert(1)</script> **unclosed [bad](url";
        let visible = text(&render(input, palette, mode, 80));
        assert!(visible.contains("<script>alert(1)</script>"));
        assert!(visible.contains("**unclosed"));
        assert!(visible.contains("[bad](url"));
    }

    #[test]
    fn controls_are_escaped_and_links_never_emit_terminal_hyperlinks() {
        let mode = aw::config::ThemeMode::Dark;
        let palette = super::super::theme::palette(mode);
        let input = "[link](https://example.test/\u{1b}]8;;bad\u{7})";
        let visible = text(&render(input, palette, mode, 80));
        assert!(!visible.contains('\u{1b}'));
        assert!(!visible.contains('\u{7}'));
        assert!(visible.contains("\\u{1b}"));
        assert!(!visible.contains("\u{1b}]8;;"));
    }

    #[test]
    fn sanitizer_escape_text_is_not_consumed_as_markdown_escape() {
        let mode = aw::config::ThemeMode::Dark;
        let palette = super::super::theme::palette(mode);
        let visible = text(&render(
            "before \u{1b}]52;c;secret\u{7} after",
            palette,
            mode,
            80,
        ));
        assert!(visible.contains("\\u{1b}"));
        assert!(visible.contains("\\u{7}"));
        assert!(!visible.contains('\u{1b}'));
        assert!(!visible.contains('\u{7}'));
    }

    #[test]
    fn tiny_width_never_emits_a_wider_indivisible_grapheme() {
        let mode = aw::config::ThemeMode::Dark;
        let palette = super::super::theme::palette(mode);
        let lines = render("🐦", palette, mode, 1);
        assert!(lines.iter().all(|line| line.width() <= 1));
        assert_eq!(text(&lines), "…");
    }

    #[test]
    fn tabs_expand_to_bounded_visible_cells() {
        let mode = aw::config::ThemeMode::Dark;
        let palette = super::super::theme::palette(mode);
        let lines = render("```\n\tindented\n```", palette, mode, 12);
        assert!(lines.iter().all(|line| line.width() <= 12));
        assert!(text(&lines).contains("    indent"));
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn plain_and_highlighted_markdown_keep_identical_visible_rows() {
        let mode = aw::config::ThemeMode::Dark;
        let palette = super::super::theme::palette(mode);
        let source = "```rust\npub fn bird() -> usize { 1 }\n```";
        let plain = render_plain(source, palette, mode, 48);
        let rich = render(source, palette, mode, 48);
        assert_eq!(plain_lines(&plain), plain_lines(&rich));
        assert!(rich.iter().map(|line| line.spans.len()).sum::<usize>()
            > plain.iter().map(|line| line.spans.len()).sum::<usize>());
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn known_fence_uses_shared_syntax_foregrounds_on_neutral_background() {
        let mode = aw::config::ThemeMode::Dark;
        let palette = super::super::theme::palette(mode);
        let lines = render(
            "```rust\npub fn bird() -> usize { 1 }\n```",
            palette,
            mode,
            80,
        );
        let code = lines
            .iter()
            .find(|line| {
                plain_lines(std::slice::from_ref(line))[0].contains("pub fn bird")
            })
            .expect("highlighted Rust code row");
        assert!(code.spans.len() > 2);
        assert!(code
            .spans
            .iter()
            .all(|span| span.style.bg == Some(palette.tool_neutral_background)));
        let foregrounds = code
            .spans
            .iter()
            .filter_map(|span| span.style.fg)
            .collect::<Vec<_>>();
        assert!(foregrounds
            .iter()
            .enumerate()
            .any(|(i, color)| foregrounds.iter().skip(i + 1).any(|other| other != color)));
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn fence_highlighting_tracks_explicit_light_and_dark_themes() {
        let source = "```json\n{\"bird\": true, \"count\": 2}\n```";
        let dark_mode = aw::config::ThemeMode::Dark;
        let light_mode = aw::config::ThemeMode::Light;
        let dark = render(
            source,
            super::super::theme::palette(dark_mode),
            dark_mode,
            80,
        );
        let light = render(
            source,
            super::super::theme::palette(light_mode),
            light_mode,
            80,
        );
        let dark_row = dark
            .iter()
            .find(|line| plain_lines(std::slice::from_ref(line))[0].contains("bird"))
            .unwrap();
        let light_row = light
            .iter()
            .find(|line| plain_lines(std::slice::from_ref(line))[0].contains("bird"))
            .unwrap();
        let dark_fg = dark_row.spans.iter().filter_map(|span| span.style.fg).collect::<Vec<_>>();
        let light_fg = light_row.spans.iter().filter_map(|span| span.style.fg).collect::<Vec<_>>();
        assert_ne!(dark_fg, light_fg);
    }

    #[test]
    fn unknown_or_malformed_fence_language_falls_back_without_losing_code() {
        let mode = aw::config::ThemeMode::Dark;
        let palette = super::super::theme::palette(mode);
        let oversized = "x".repeat(65);
        for language in ["unknown-aw-language", "../../rust", oversized.as_str()] {
            let source = format!("```{language}\nlet bird = 1;\n```");
            let lines = render(&source, palette, mode, 40);
            let visible = text(&lines);
            assert!(visible.contains("let bird = 1;"));
            assert!(lines.iter().all(|line| line.width() <= 40));
        }
    }

    #[cfg(not(feature = "workspace"))]
    #[test]
    fn no_workspace_build_keeps_neutral_fence_fallback() {
        let mode = aw::config::ThemeMode::Dark;
        let palette = super::super::theme::palette(mode);
        let lines = render("```rust\npub fn bird() {}\n```", palette, mode, 40);
        let code = lines
            .iter()
            .find(|line| {
                plain_lines(std::slice::from_ref(line))[0].contains("pub fn bird")
            })
            .unwrap();
        assert!(code
            .spans
            .iter()
            .all(|span| span.style.bg == Some(palette.tool_neutral_background)));
    }

    #[test]
    fn code_and_wrapped_blocks_stay_width_bounded() {
        let mode = aw::config::ThemeMode::Dark;
        let palette = super::super::theme::palette(mode);
        let input = "```text\n0123456789abcdef\n```\n- a long list item that wraps cleanly across several rows\n> a quoted line that also wraps";
        let lines = render(input, palette, mode, 12);
        assert!(lines.iter().all(|line| line.width() <= 12));
        let code = lines
            .iter()
            .find(|line| {
                plain_lines(std::slice::from_ref(line))[0].contains("0123456789")
            })
            .expect("code line");
        assert_eq!(code.width(), 12);
        assert!(code
            .spans
            .iter()
            .all(|span| span.style.bg == Some(palette.tool_neutral_background)));
    }
}
