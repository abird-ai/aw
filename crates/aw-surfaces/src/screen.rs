//! Safe cell projection rather than raw PTY bytes in the browser. OSC/ANSI
//! controls have already been consumed by aw-terminal and are never HTML.
use serde::Serialize;
#[derive(Serialize, PartialEq, Eq, Clone, Default)]
struct Style {
    fg: Option<String>,
    bg: Option<String>,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
}
#[derive(Serialize)]
struct Run {
    text: String,
    style: Style,
}
#[derive(Serialize)]
pub(crate) struct Screen {
    pub id: String,
    version: u64,
    rows: u16,
    cols: u16,
    lines: Vec<Vec<Run>>,
    cursor: (u16, u16),
    cursor_visible: bool,
    status: String,
    notice: String,
    root: String,
    scrollback: usize,
    pending_input: usize,
}
fn color(c: aw_terminal::Color) -> Option<String> {
    match c {
        aw_terminal::Color::Default => None,
        aw_terminal::Color::Rgb(r, g, b) => Some(format!("#{r:02x}{g:02x}{b:02x}")),
        aw_terminal::Color::Indexed(i) => Some(format!("ansi-{i}")),
    }
}
impl From<aw_terminal::Snapshot> for Screen {
    fn from(s: aw_terminal::Snapshot) -> Self {
        let mut lines = Vec::new();
        for row in 0..s.rows {
            let mut runs: Vec<Run> = vec![];
            for col in 0..s.cols {
                let Some(c) = s
                    .cells
                    .get(usize::from(row) * usize::from(s.cols) + usize::from(col))
                else {
                    continue;
                };
                if c.continuation {
                    continue;
                }
                let text = if c.text.is_empty() {
                    " ".into()
                } else {
                    c.text
                        .chars()
                        .map(|ch| {
                            if ch.is_control()
                                || matches!(ch,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')
                            {
                                '\u{fffd}'
                            } else {
                                ch
                            }
                        })
                        .collect::<String>()
                };
                let style = Style {
                    fg: color(c.fg),
                    bg: color(c.bg),
                    bold: c.bold,
                    dim: c.dim,
                    italic: c.italic,
                    underline: c.underline,
                    inverse: c.inverse,
                };
                if let Some(last) = runs.last_mut().filter(|r| r.style == style) {
                    last.text.push_str(&text);
                } else {
                    runs.push(Run { text, style });
                }
            }
            lines.push(runs);
        }
        Self {
            id: s.id,
            version: s.version,
            rows: s.rows,
            cols: s.cols,
            lines,
            cursor: s.cursor,
            cursor_visible: s.cursor_visible,
            status: format!("{:?}", s.status),
            notice: s.notice,
            root: s.root.display().to_string(),
            scrollback: s.scrollback,
            pending_input: s.pending_input,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexed_colors_are_tokens() {
        assert_eq!(
            color(aw_terminal::Color::Indexed(255)).as_deref(),
            Some("ansi-255")
        );
    }
    #[test]
    fn rgb_is_fixed_hex() {
        assert_eq!(
            color(aw_terminal::Color::Rgb(255, 0, 16)).as_deref(),
            Some("#ff0010")
        );
    }
}
