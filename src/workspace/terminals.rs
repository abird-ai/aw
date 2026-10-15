//! Explicit human PTY resources retained independently of tabs/presentation.
use aw_terminal::{Options, Status, Terminal};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Paragraph,
    Frame,
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Default)]
struct Inner {
    terminals: BTreeMap<String, Arc<Terminal>>,
    starting: usize,
}
#[derive(Clone)]
pub struct Pool {
    inner: Arc<Mutex<Inner>>,
    tick: tokio::sync::watch::Sender<u64>,
}
impl Pool {
    pub fn new() -> Self {
        let (tick, _) = tokio::sync::watch::channel(0);
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
            tick,
        }
    }
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.tick.subscribe()
    }
    pub fn launch(&self, root: PathBuf, shell: PathBuf, owner: String) -> aw::Result<String> {
        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if inner.terminals.len() + inner.starting >= 4 {
                return Err(aw::Error::Limit(
                    "at most four retained terminals; forget an exited terminal first",
                ));
            }
            inner.starting += 1;
        }
        let notify = self.tick.clone();
        let result = Terminal::spawn(Options::bash(root, shell, owner), move || {
            notify.send_modify(|n| *n = n.wrapping_add(1));
        });
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.starting = inner.starting.saturating_sub(1);
        let terminal = result.map_err(|e| aw::Error::Config(e.to_string()))?;
        let id = terminal.id.clone();
        inner.terminals.insert(id.clone(), Arc::new(terminal));
        self.tick.send_modify(|n| *n = n.wrapping_add(1));
        Ok(id)
    }
    pub fn get(&self, id: &str) -> Option<Arc<Terminal>> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .terminals
            .get(id)
            .cloned()
    }
    pub fn all(&self) -> Vec<Arc<Terminal>> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .terminals
            .values()
            .cloned()
            .collect()
    }
    pub fn running(&self) -> bool {
        self.all()
            .iter()
            .any(|t| matches!(t.status(), Status::Running | Status::Stopping))
    }
    pub fn forget(&self, id: &str) -> aw::Result<()> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let terminal = inner
            .terminals
            .get(id)
            .ok_or(aw::Error::Denied("unknown terminal"))?;
        if matches!(terminal.status(), Status::Running | Status::Stopping) {
            return Err(aw::Error::Denied("stop the terminal before forgetting it"));
        }
        inner.terminals.remove(id);
        Ok(())
    }
    pub fn shutdown(&self) {
        let list = self.all();
        for terminal in &list {
            terminal.terminate();
        }
        let until = Instant::now() + Duration::from_millis(2500);
        while Instant::now() < until
            && list
                .iter()
                .any(|t| matches!(t.status(), Status::Running | Status::Stopping))
        {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

pub fn key(key: KeyEvent, application: bool) -> Option<Vec<u8>> {
    use aw_terminal::{Key, Modifiers};
    let m = Modifiers {
        ctrl: key.modifiers.contains(KeyModifiers::CONTROL),
        alt: key.modifiers.contains(KeyModifiers::ALT),
        shift: key.modifiers.contains(KeyModifiers::SHIFT),
    };
    let key = match key.code {
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Enter => Key::Enter,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::BackTab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Esc => Key::Escape,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::Delete => Key::Delete,
        KeyCode::Insert => Key::Insert,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::F(n) => Key::Function(n),
        _ => return None,
    };
    aw_terminal::encode_key(key, m, application).ok()
}
fn color(c: aw_terminal::Color) -> Color {
    match c {
        aw_terminal::Color::Default => Color::Reset,
        aw_terminal::Color::Indexed(i) => Color::Indexed(i),
        aw_terminal::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}
pub fn draw(
    frame: &mut Frame<'_>,
    area: Rect,
    terminal: &Terminal,
    scrollback: usize,
    focused: bool,
) {
    if area.width < 4 || area.height < 3 {
        frame.render_widget(
            Paragraph::new("Enlarge terminal view; existing PTY size retained"),
            area,
        );
        return;
    }
    let snapshot = terminal.snapshot(scrollback);
    let body = Rect {
        height: area.height - 1,
        ..area
    };
    for row in 0..snapshot.rows.min(body.height) {
        for col in 0..snapshot.cols.min(body.width) {
            let Some(cell) = snapshot
                .cells
                .get(usize::from(row) * usize::from(snapshot.cols) + usize::from(col))
            else {
                continue;
            };
            if cell.continuation {
                continue;
            }
            let mut style = Style::default().fg(color(cell.fg)).bg(color(cell.bg));
            for (yes, flag) in [
                (cell.bold, Modifier::BOLD),
                (cell.dim, Modifier::DIM),
                (cell.italic, Modifier::ITALIC),
                (cell.underline, Modifier::UNDERLINED),
                (cell.inverse, Modifier::REVERSED),
            ] {
                if yes {
                    style = style.add_modifier(flag);
                }
            }
            let text = if cell.text.is_empty() {
                " "
            } else if cell.text.chars().any(|c| {
                c.is_control() || matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')
            }) {
                "�"
            } else {
                &cell.text
            };
            let width = if cell.wide { 2 } else { 1 };
            if col + width <= body.width {
                frame.buffer_mut().set_stringn(
                    body.x + col,
                    body.y + row,
                    text,
                    usize::from(width),
                    style,
                );
            }
        }
    }
    let label = format!(
        "{:?} · {} · UNSANDBOXED · P p commands{}",
        snapshot.status,
        aw_workspace::editor::display(&snapshot.root.display().to_string()),
        if snapshot.scrollback > 0 {
            " · scrollback"
        } else {
            ""
        }
    );
    frame.render_widget(
        Paragraph::new(label),
        Rect {
            y: area.y + area.height - 1,
            height: 1,
            ..area
        },
    );
    if focused
        && snapshot.cursor_visible
        && snapshot.cursor.0 < body.height
        && snapshot.cursor.1 < body.width
    {
        frame.set_cursor_position((body.x + snapshot.cursor.1, body.y + snapshot.cursor.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn control_c_belongs_to_child() {
        assert_eq!(
            key(
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                false
            ),
            Some(vec![3])
        );
    }
    #[test]
    fn tab_and_escape_are_not_workspace_navigation() {
        assert_eq!(
            key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), false),
            Some(vec![9])
        );
        assert_eq!(
            key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), false),
            Some(vec![27])
        );
    }
    #[test]
    fn empty_pool_has_no_processes() {
        let p = Pool::new();
        assert!(!p.running());
        assert!(p.all().is_empty());
    }
}
