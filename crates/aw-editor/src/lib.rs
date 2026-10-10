//! A bounded plain-text composer with grapheme-aware cursor movement. There is
//! deliberately no network, provider call, persistent draft or collaboration.
use thiserror::Error;

pub type Result<T> = std::result::Result<T, EditorError>;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum EditorError {
    #[error("{0}")]
    Limit(&'static str),
}
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
pub const MAX_DRAFT_BYTES: usize = 128 * 1024;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Editor {
    text: String,
    cursor: usize,
    undo: Vec<(String, usize)>,
    kill_ring: Vec<String>,
    yank_index: usize,
    last_yank: Option<(usize, usize)>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorView {
    pub lines: Vec<String>,
    pub cursor_x: u16,
    pub cursor_y: u16,
}
impl Editor {
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
    pub fn set(&mut self, text: impl Into<String>) -> Result<()> {
        let text = text.into();
        if text.len() > MAX_DRAFT_BYTES {
            return Err(EditorError::Limit("prompt exceeds 128 KiB"));
        }
        self.cursor = text.len();
        self.text = text;
        self.undo.clear();
        self.last_yank = None;
        Ok(())
    }
    fn checkpoint(&mut self) {
        const MAX_UNDO: usize = 128;
        if self
            .undo
            .last()
            .is_some_and(|(text, cursor)| text == &self.text && *cursor == self.cursor)
        {
            return;
        }
        if self.undo.len() >= MAX_UNDO {
            self.undo.remove(0);
        }
        self.undo.push((self.text.clone(), self.cursor));
    }

    fn insert_raw(&mut self, text: &str) -> Result<(usize, usize)> {
        if self.text.len().saturating_add(text.len()) > MAX_DRAFT_BYTES {
            return Err(EditorError::Limit("prompt exceeds 128 KiB"));
        }
        let start = self.cursor;
        let end = self.cursor + text.len();
        self.text.insert_str(self.cursor, text);
        self.cursor = self.next_boundary_at_or_after(end);
        Ok((start, self.cursor))
    }

    fn invalidate_yank(&mut self) {
        self.last_yank = None;
        self.yank_index = 0;
    }

    fn push_kill(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        const MAX_KILLS: usize = 16;
        if self
            .kill_ring
            .first()
            .is_some_and(|existing| existing == &text)
        {
            self.yank_index = 0;
            return;
        }
        self.kill_ring.insert(0, text);
        self.kill_ring.truncate(MAX_KILLS);
        self.yank_index = 0;
    }

    /// Paste is data insertion. The UI must never route Event::Paste to Submit.
    pub fn insert(&mut self, text: &str) -> Result<()> {
        if self.text.len().saturating_add(text.len()) > MAX_DRAFT_BYTES {
            return Err(EditorError::Limit("prompt exceeds 128 KiB"));
        }
        self.checkpoint();
        self.invalidate_yank();
        self.insert_raw(text)?;
        Ok(())
    }
    fn next_boundary_at_or_after(&self, at: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|i| *i >= at)
            .unwrap_or(self.text.len())
    }
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.undo.clear();
        self.last_yank = None;
    }
    /// Delete from the cursor back to the start of the current logical line.
    ///
    /// This is the familiar readline Ctrl+U behavior. It is deliberately
    /// line-local so a multiline draft is never erased by a single editing
    /// shortcut.
    pub fn delete_to_line_start(&mut self) {
        let start = self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1);
        if start == self.cursor {
            return;
        }
        self.checkpoint();
        let killed = self.text[start..self.cursor].to_owned();
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
        self.invalidate_yank();
        self.push_kill(killed);
    }
    /// Delete from the cursor to the end of the current logical line.
    ///
    /// This is the familiar readline Ctrl+K behavior. Like Ctrl+U it is
    /// deliberately line-local for multiline drafts.
    pub fn delete_to_line_end(&mut self) {
        let end = self.text[self.cursor..]
            .find('\n')
            .map_or(self.text.len(), |i| self.cursor + i);
        if end == self.cursor {
            return;
        }
        self.checkpoint();
        let killed = self.text[self.cursor..end].to_owned();
        self.text.replace_range(self.cursor..end, "");
        self.normalize_cursor();
        self.invalidate_yank();
        self.push_kill(killed);
    }
    fn previous_grapheme_start(&self, at: usize) -> Option<usize> {
        self.text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .take_while(|i| *i < at)
            .last()
    }
    fn next_grapheme_start(&self, at: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|i| *i > at)
            .unwrap_or(self.text.len())
    }
    fn grapheme_at(&self, at: usize) -> Option<&str> {
        self.text[at..].graphemes(true).next()
    }
    fn grapheme_class(grapheme: &str) -> u8 {
        if grapheme.chars().all(char::is_whitespace) {
            0
        } else if grapheme.chars().any(|c| c.is_alphanumeric() || c == '_') {
            1
        } else {
            2
        }
    }
    /// Shell/readline-style backward movement: discard trailing whitespace,
    /// then cross exactly one word-like or punctuation run.
    fn previous_word_boundary(&self, at: usize) -> usize {
        let mut cursor = at.min(self.text.len());
        while let Some(previous) = self.previous_grapheme_start(cursor) {
            let Some(grapheme) = self.grapheme_at(previous) else {
                break;
            };
            if Self::grapheme_class(grapheme) != 0 {
                break;
            }
            cursor = previous;
        }
        let Some(previous) = self.previous_grapheme_start(cursor) else {
            return cursor;
        };
        let class = self
            .grapheme_at(previous)
            .map(Self::grapheme_class)
            .unwrap_or(0);
        while let Some(previous) = self.previous_grapheme_start(cursor) {
            let Some(grapheme) = self.grapheme_at(previous) else {
                break;
            };
            if Self::grapheme_class(grapheme) != class {
                break;
            }
            cursor = previous;
        }
        cursor
    }
    /// Shell/readline-style forward movement: discard leading whitespace,
    /// then cross exactly one word-like or punctuation run.
    fn next_word_boundary(&self, at: usize) -> usize {
        let mut cursor = at.min(self.text.len());
        while cursor < self.text.len() {
            let Some(grapheme) = self.grapheme_at(cursor) else {
                break;
            };
            if Self::grapheme_class(grapheme) != 0 {
                break;
            }
            cursor = self.next_grapheme_start(cursor);
        }
        let Some(grapheme) = self.grapheme_at(cursor) else {
            return cursor;
        };
        let class = Self::grapheme_class(grapheme);
        while cursor < self.text.len() {
            let Some(grapheme) = self.grapheme_at(cursor) else {
                break;
            };
            if Self::grapheme_class(grapheme) != class {
                break;
            }
            cursor = self.next_grapheme_start(cursor);
        }
        cursor
    }
    pub fn word_left(&mut self) {
        self.cursor = self.previous_word_boundary(self.cursor);
        self.invalidate_yank();
    }
    pub fn word_right(&mut self) {
        self.cursor = self.next_word_boundary(self.cursor);
        self.invalidate_yank();
    }
    pub fn delete_word_backward(&mut self) {
        let end = self.cursor;
        let start = self.previous_word_boundary(end);
        if start == end {
            return;
        }
        self.checkpoint();
        let killed = self.text[start..end].to_owned();
        self.text.replace_range(start..end, "");
        self.cursor = start;
        self.normalize_cursor();
        self.invalidate_yank();
        self.push_kill(killed);
    }
    pub fn delete_word_forward(&mut self) {
        let start = self.cursor;
        let end = self.next_word_boundary(start);
        if start == end {
            return;
        }
        self.checkpoint();
        let killed = self.text[start..end].to_owned();
        self.text.replace_range(start..end, "");
        self.cursor = start;
        self.normalize_cursor();
        self.invalidate_yank();
        self.push_kill(killed);
    }
    pub fn left(&mut self) {
        self.cursor = self
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .take_while(|i| *i < self.cursor)
            .last()
            .unwrap_or(0);
        self.invalidate_yank();
    }
    pub fn right(&mut self) {
        self.cursor = self.next_grapheme_start(self.cursor);
        self.invalidate_yank();
    }
    pub fn backspace(&mut self) {
        let end = self.cursor;
        let start = self.previous_grapheme_start(end).unwrap_or(0);
        if start == end {
            return;
        }
        self.checkpoint();
        self.text.replace_range(start..end, "");
        self.cursor = start;
        self.normalize_cursor();
        self.invalidate_yank();
    }
    pub fn delete(&mut self) {
        let start = self.cursor;
        let end = self.next_grapheme_start(start);
        if start == end {
            return;
        }
        self.checkpoint();
        self.text.replace_range(start..end, "");
        self.cursor = start;
        self.normalize_cursor();
        self.invalidate_yank();
    }
    fn normalize_cursor(&mut self) {
        if self.cursor == self.text.len() {
            return;
        }
        self.cursor = self
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .take_while(|i| *i <= self.cursor)
            .last()
            .unwrap_or(0);
    }
    pub fn home(&mut self) {
        self.cursor = self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1);
        self.invalidate_yank();
    }
    pub fn end(&mut self) {
        self.cursor = self.text[self.cursor..]
            .find('\n')
            .map_or(self.text.len(), |i| self.cursor + i);
        self.normalize_cursor();
        self.invalidate_yank();
    }
    pub fn vertical(&mut self, down: bool) {
        let start = self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1);
        let column = self.text[start..self.cursor].graphemes(true).count();
        let bounds = if down {
            self.text[self.cursor..].find('\n').map(|n| {
                let s = self.cursor + n + 1;
                let e = self.text[s..].find('\n').map_or(self.text.len(), |p| s + p);
                (s, e)
            })
        } else if start == 0 {
            None
        } else {
            let e = start - 1;
            let s = self.text[..e].rfind('\n').map_or(0, |p| p + 1);
            Some((s, e))
        };
        if let Some((s, e)) = bounds {
            self.cursor = self.text[s..e]
                .grapheme_indices(true)
                .nth(column)
                .map_or(e, |(n, _)| s + n);
            self.normalize_cursor();
        }
        self.invalidate_yank();
    }
    pub fn undo(&mut self) {
        if let Some((text, cursor)) = self.undo.pop() {
            self.text = text;
            self.cursor = cursor.min(self.text.len());
            self.normalize_cursor();
        }
        self.invalidate_yank();
    }

    pub fn yank(&mut self) -> Result<()> {
        let Some(text) = self.kill_ring.first().cloned() else {
            return Ok(());
        };
        if self.text.len().saturating_add(text.len()) > MAX_DRAFT_BYTES {
            return Err(EditorError::Limit("prompt exceeds 128 KiB"));
        }
        self.checkpoint();
        let range = self.insert_raw(&text)?;
        self.last_yank = Some(range);
        self.yank_index = 0;
        Ok(())
    }

    pub fn yank_pop(&mut self) -> Result<()> {
        let Some((start, end)) = self.last_yank else {
            return Ok(());
        };
        if self.kill_ring.len() < 2 {
            return Ok(());
        }
        let next_index = (self.yank_index + 1) % self.kill_ring.len();
        let text = self.kill_ring[next_index].clone();
        let next_len = self
            .text
            .len()
            .saturating_sub(end.saturating_sub(start))
            .saturating_add(text.len());
        if next_len > MAX_DRAFT_BYTES {
            return Err(EditorError::Limit("prompt exceeds 128 KiB"));
        }
        self.checkpoint();
        self.yank_index = next_index;
        self.text.replace_range(start..end, &text);
        self.cursor = start.saturating_add(text.len()).min(self.text.len());
        self.normalize_cursor();
        self.last_yank = Some((start, self.cursor));
        Ok(())
    }

    pub fn view(&self, width: u16, height: u16) -> EditorView {
        if width == 0 || height == 0 {
            return EditorView {
                lines: vec![],
                cursor_x: 0,
                cursor_y: 0,
            };
        }
        let mut lines = vec![String::new()];
        let (mut x, mut y) = (0usize, 0usize);
        let (mut cx, mut cy) = (0, 0);
        for (at, g) in self.text.grapheme_indices(true) {
            if at == self.cursor {
                cx = x;
                cy = y;
            }
            if matches!(g, "\n" | "\r\n") {
                lines.push(String::new());
                x = 0;
                y += 1;
                continue;
            }
            let shown = display(g);
            for atom in shown.graphemes(true) {
                let w = UnicodeWidthStr::width(atom).max(1);
                if x + w > usize::from(width) && x > 0 {
                    lines.push(String::new());
                    x = 0;
                    y += 1;
                }
                if at == self.cursor && x == 0 {
                    cx = x;
                    cy = y;
                }
                lines[y].push_str(atom);
                x += w;
                if x >= usize::from(width) {
                    lines.push(String::new());
                    x = 0;
                    y += 1;
                }
            }
        }
        if self.cursor == self.text.len() {
            cx = x;
            cy = y;
        }
        let top = cy.saturating_sub(usize::from(height) - 1);
        EditorView {
            lines: lines
                .into_iter()
                .skip(top)
                .take(usize::from(height))
                .collect(),
            cursor_x: cx.min(usize::from(width) - 1) as u16,
            cursor_y: (cy - top).min(usize::from(height) - 1) as u16,
        }
    }
}
/// Escape control sequences before terminal rendering. Preserve original data
/// in Editor; the display representation is never submitted instead of it.
pub fn display(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '\n' => out.push(c),
            '\t' => out.push_str("    "),
            c if c.is_control()
                || matches!(c,'\u{061c}'|'\u{200e}'|'\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}') =>
            {
                use std::fmt::Write;
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_motion_uses_whitespace_word_and_punctuation_runs() {
        let mut editor = Editor::default();
        editor.set("alpha  beta-gamma").unwrap();
        editor.home();
        editor.word_right();
        assert_eq!(editor.cursor(), "alpha".len());
        editor.word_right();
        assert_eq!(editor.cursor(), "alpha  beta".len());
        editor.word_right();
        assert_eq!(editor.cursor(), "alpha  beta-".len());
        editor.word_right();
        assert_eq!(editor.cursor(), editor.text().len());
        editor.word_left();
        assert_eq!(editor.cursor(), "alpha  beta-".len());
        editor.word_left();
        assert_eq!(editor.cursor(), "alpha  beta".len());
    }

    #[test]
    fn kill_ring_yank_pop_and_undo_are_bounded_editor_state() {
        let mut editor = Editor::default();
        editor.set("alpha beta gamma").unwrap();
        editor.delete_word_backward();
        assert_eq!(editor.text(), "alpha beta ");
        editor.delete_word_backward();
        assert_eq!(editor.text(), "alpha ");
        editor.yank().unwrap();
        assert_eq!(editor.text(), "alpha beta ");
        editor.yank_pop().unwrap();
        assert_eq!(editor.text(), "alpha gamma");
        editor.undo();
        assert_eq!(editor.text(), "alpha beta ");
    }

    #[test]
    fn ordinary_insert_can_be_undone_without_unbounded_history() {
        let mut editor = Editor::default();
        editor.insert("a").unwrap();
        editor.insert("b").unwrap();
        editor.undo();
        assert_eq!(editor.text(), "a");
        editor.undo();
        assert!(editor.is_empty());
    }
}
