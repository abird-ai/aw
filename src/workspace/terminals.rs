//! Explicit human PTY resources retained independently of tabs/presentation.
use super::terminal_control::{Control, Invitation, Lease, Mode};
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
    control: Control,
    tick: tokio::sync::watch::Sender<u64>,
}
impl Pool {
    pub fn new() -> Self {
        let (tick, _) = tokio::sync::watch::channel(0);
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
            control: Control::default(),
            tick,
        }
    }
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.tick.subscribe()
    }
    pub fn launch(&self, root: PathBuf, shell: PathBuf, owner: String) -> aw::Result<String> {
        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if self.control.is_shutdown() {
                return Err(aw::Error::Denied("terminal pool is shutting down"));
            }
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
        if let Err(error) = self.control.register(&id) {
            // A concurrent owner shutdown after spawn cannot orphan a child
            // outside the pool's retained lifecycle.
            terminal.terminate();
            return Err(error);
        }
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
    /// A human-approved invitation transfers only this exact terminal's
    /// input/size controller. Snapshot reads stay tied to the original Pool.
    pub fn prepare_dock(&self, id: &str) -> aw::Result<Invitation> {
        let terminal = self.get(id).ok_or(aw::Error::Denied("unknown retained PTY"))?;
        let invite = self.control.invite(id, &terminal)?;
        self.tick.send_modify(|revision| *revision = revision.wrapping_add(1));
        Ok(invite)
    }
    pub fn cancel_dock(&self, id: &str) -> aw::Result<()> {
        self.control.cancel(id)?;
        self.tick.send_modify(|revision| *revision = revision.wrapping_add(1));
        Ok(())
    }
    pub fn accept_dock(&self, id: &str, ticket: &str) -> aw::Result<Lease> {
        let terminal = self.get(id).ok_or(aw::Error::Denied("unknown retained PTY"))?;
        let mut accepted = self.control.accept(id, ticket, &terminal)?;
        let tick = self.tick.clone();
        accepted.on_drop(move || tick.send_modify(|revision| *revision = revision.wrapping_add(1)));
        self.tick.send_modify(|revision| *revision = revision.wrapping_add(1));
        Ok(accepted)
    }
    pub fn is_shutting_down(&self) -> bool { self.control.is_shutdown() }
    pub fn has_invitation(&self, id: &str, ticket: &str) -> bool {
        self.control.has_offer(id, ticket)
    }
    pub fn dock_mode(&self, id: &str) -> Option<Mode> {
        let terminal = self.get(id)?;
        self.control.mode(id, &terminal)
    }
    pub fn input_local(&self, id: &str, bytes: Vec<u8>) -> aw::Result<()> {
        let terminal = self.get(id).ok_or(aw::Error::Denied("unknown retained PTY"))?;
        self.control.local(id, &terminal, || terminal.input(bytes))
    }
    pub fn resize_local(&self, id: &str, rows: u16, cols: u16) -> aw::Result<()> {
        let terminal = self.get(id).ok_or(aw::Error::Denied("unknown retained PTY"))?;
        self.control.local(id, &terminal, || terminal.resize(rows, cols))
    }
    pub fn terminate_local(&self, id: &str) -> aw::Result<()> {
        let terminal = self.get(id).ok_or(aw::Error::Denied("unknown retained PTY"))?;
        self.control.local(id, &terminal, || {
            terminal.terminate();
            Ok(())
        })
    }
    pub fn input_docked(&self, lease: &Lease, bytes: Vec<u8>) -> aw::Result<()> {
        let terminal = self.get(lease.terminal_id())
            .ok_or(aw::Error::Denied("unknown retained PTY"))?;
        self.control.docked(lease, || terminal.input(bytes))
    }
    pub fn resize_docked(&self, lease: &Lease, rows: u16, cols: u16) -> aw::Result<()> {
        let terminal = self.get(lease.terminal_id())
            .ok_or(aw::Error::Denied("unknown retained PTY"))?;
        self.control.docked(lease, || terminal.resize(rows, cols))
    }
    pub fn snapshot_docked(&self, lease: &Lease, scrollback: usize) -> aw::Result<aw_terminal::Snapshot> {
        let terminal = self.get(lease.terminal_id())
            .ok_or(aw::Error::Denied("unknown retained PTY"))?;
        self.control.docked(lease, || Ok(terminal.snapshot(scrollback)))
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
        if !matches!(self.control.mode(id, terminal), Some(Mode::Local | Mode::Offered)) {
            return Err(aw::Error::Denied(
                "detach the other native terminal before forgetting this PTY",
            ));
        }
        inner.terminals.remove(id);
        self.control.forget(id);
        Ok(())
    }
    pub fn shutdown(&self) {
        self.control.shutdown();
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
    controller: Option<Mode>,
) {
    if area.width < 4 || area.height < 3 {
        frame.render_widget(Paragraph::new("Enlarge terminal view; existing PTY size retained"), area);
        return;
    }
    let snapshot = terminal.snapshot(scrollback);
    render_snapshot(frame, area, &snapshot, focused, controller, false);
}

/// Exact same VT100 cell projection in the independently attached native
/// terminal. This renderer never holds or creates a child shell.
pub fn draw_snapshot(frame: &mut Frame<'_>, area: Rect, snapshot: &aw_terminal::Snapshot, focused: bool) {
    render_snapshot(frame, area, snapshot, focused, Some(Mode::Docked), true);
}
fn render_snapshot(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &aw_terminal::Snapshot,
    focused: bool,
    controller: Option<Mode>,
    remote: bool,
) {
    if area.width < 4 || area.height < 3 {
        frame.render_widget(Paragraph::new("Enlarge the receiving terminal; PTY size retained"), area);
        return;
    }
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
    let path = aw_workspace::editor::display(&snapshot.root.display().to_string());
    let label = if remote {
        // F12 is the recipient's ONLY local control shortcut, so keep it
        // first and visible even at 65x20. P/p commands belong exclusively
        // to the source Workspace, not this independent viewer.
        format!("F12 detach · {:?} · UNSANDBOXED · {path}", snapshot.status)
    } else {
        let controller_label = match controller {
            Some(Mode::Local) => "",
            Some(Mode::Offered) => " · invite pending",
            Some(Mode::Docked) => " · docked · read only",
            Some(Mode::Settling) => " · remote input settling · read only",
            None => " · control unavailable",
        };
        format!(
            "{:?} · {path} · UNSANDBOXED · P p commands{}{}",
            snapshot.status,
            if snapshot.scrollback > 0 { " · scrollback" } else { "" },
            controller_label,
        )
    };
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

    #[cfg(unix)]
    fn fixture() -> (Pool, String, std::path::PathBuf) {
        let bash = std::env::var_os("AW_BASH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/bin/bash"));
        let mut suffix = [0_u8; 12];
        getrandom::fill(&mut suffix).expect("synthetic terminal identity entropy");
        let root = std::env::temp_dir().join(format!(
            "aw-native-pty-test-{}",
            suffix.iter().map(|b| format!("{b:02x}")).collect::<String>()
        ));
        std::fs::create_dir(&root).unwrap();
        let pool = Pool::new();
        let id = pool.launch(root.clone(), bash, "synthetic_actor".into()).unwrap();
        (pool, id, root)
    }
    #[cfg(unix)]
    pub(super) fn wait_local(pool: &Pool, id: &str) {
        for _ in 0..120 {
            if pool.dock_mode(id) == Some(Mode::Local) { return; }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("original terminal controller did not settle");
    }

    #[cfg(unix)]
    #[test]
    fn one_retained_child_is_exclusively_controlled_and_returns_on_detach() {
        let (pool, id, root) = fixture();
        let original = pool.get(&id).unwrap();
        let initial = original.snapshot(0);
        assert_eq!(initial.id, id);
        assert_eq!(pool.dock_mode(&id), Some(Mode::Local));
        let invitation = pool.prepare_dock(&id).unwrap();
        assert_eq!(invitation.terminal_id, id);
        assert_eq!(invitation.ticket.len(), 64);
        assert_eq!(pool.dock_mode(&id), Some(Mode::Offered));
        assert!(pool.input_local(&id, vec![3]).is_ok(),
            "invitation alone does not move PTY ownership");
        assert!(pool.accept_dock(&id, "f".repeat(64).as_str()).is_err());
        for _ in 0..120 {
            if original.snapshot(0).pending_input == 0 { break; }
            std::thread::sleep(Duration::from_millis(15));
        }
        let lease = pool.accept_dock(&id, &invitation.ticket).unwrap();
        assert_eq!(lease.terminal_id(), id);
        assert_eq!(pool.dock_mode(&id), Some(Mode::Docked));
        assert!(pool.input_local(&id, b"echo ILLEGAL_SOURCE\n".to_vec()).is_err());
        assert!(pool.resize_local(&id, 40, 100).is_err());
        assert!(pool.terminate_local(&id).is_err());
        assert!(pool.accept_dock(&id, &invitation.ticket).is_err());
        assert!(pool.prepare_dock(&id).is_err());
        assert_eq!(pool.snapshot_docked(&lease, 0).unwrap().id, initial.id);
        assert!(std::sync::Arc::ptr_eq(&original, &pool.get(&id).unwrap()));
        pool.input_docked(&lease, b"echo ORIGINAL_PTY_DOCKED\n".to_vec()).unwrap();
        for _ in 0..120 {
            if original.snapshot(0).pending_input == 0 { break; }
            std::thread::sleep(Duration::from_millis(15));
        }
        drop(lease);
        wait_local(&pool, &id);
        assert!(pool.input_local(&id, b"echo ORIGINAL_PTY_RESTORED\n".to_vec()).is_ok());
        assert_eq!(pool.get(&id).unwrap().id, initial.id, "no new shell created");
        pool.shutdown();
        assert!(pool.input_local(&id, b"echo NOT_AUTHORIZED\n".to_vec()).is_err());
        drop(pool);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_rejects_new_shell_without_spawning_unowned_child() {
        let pool = Pool::new();
        pool.shutdown();
        let path = std::env::temp_dir();
        let shell = std::env::var_os("AW_BASH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/bin/bash"));
        assert!(pool.launch(path, shell, "synthetic_actor".into()).is_err());
        assert!(!pool.running());
        assert!(pool.all().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn expired_invitation_is_not_an_authority_and_reissues_are_distinct() {
        let (pool, id, root) = fixture();
        let expired = pool.prepare_dock(&id).unwrap();
        pool.control.expire_offer_for_test(&id);
        assert!(!pool.has_invitation(&id, &expired.ticket));
        assert_eq!(pool.dock_mode(&id), Some(Mode::Local));
        assert!(pool.accept_dock(&id, &expired.ticket).is_err());
        let next = pool.prepare_dock(&id).unwrap();
        assert_ne!(expired.ticket, next.ticket);
        let lease = pool.accept_dock(&id, &next.ticket).unwrap();
        assert!(pool.accept_dock(&id, &next.ticket).is_err());
        drop(lease);
        wait_local(&pool, &id);
        pool.shutdown();
        drop(pool);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn competing_recipient_admission_is_one_lease_even_during_a_race() {
        let (pool, id, root) = fixture();
        let invite = pool.prepare_dock(&id).unwrap();
        let contenders = std::thread::scope(|scope| {
            let left = scope.spawn(|| pool.accept_dock(&id, &invite.ticket));
            let right = scope.spawn(|| pool.accept_dock(&id, &invite.ticket));
            [left.join().unwrap(), right.join().unwrap()]
        });
        let accepted = contenders.iter().filter(|attempt| attempt.is_ok()).count();
        assert_eq!(accepted, 1, "exactly one owner-fenced recipient may admit");
        assert_eq!(pool.dock_mode(&id), Some(Mode::Docked));
        assert!(pool.input_local(&id, vec![3]).is_err());
        for lease in contenders.into_iter().flatten() {
            drop(lease);
        }
        wait_local(&pool, &id);
        pool.shutdown();
        drop(pool);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn revoked_old_native_invitation_cannot_replace_new_generation() {
        let (pool, id, root) = fixture();
        let original = pool.prepare_dock(&id).unwrap();
        assert!(pool.prepare_dock(&id).is_err(), "one pending invitation per PTY");
        pool.cancel_dock(&id).unwrap();
        assert!(pool.accept_dock(&id, &original.ticket).is_err());
        let fresh = pool.prepare_dock(&id).unwrap();
        assert_ne!(fresh.ticket, original.ticket);
        assert!(pool.accept_dock(&id, &original.ticket).is_err());
        let lease = pool.accept_dock(&id, &fresh.ticket).unwrap();
        assert!(pool.input_local(&id, b"no".to_vec()).is_err());
        drop(lease);
        wait_local(&pool, &id);
        let second = pool.prepare_dock(&id).unwrap();
        assert!(pool.accept_dock(&id, &fresh.ticket).is_err());
        assert_ne!(second.ticket, fresh.ticket);
        let new_controller = pool.accept_dock(&id, &second.ticket).unwrap();
        assert!(pool.input_local(&id, b"no".to_vec()).is_err());
        drop(new_controller);
        wait_local(&pool, &id);
        pool.shutdown();
        drop(pool);
        std::fs::remove_dir_all(root).unwrap();
    }
}
