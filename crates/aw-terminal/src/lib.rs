//! Real local PTYs with bounded screen/scrollback and asynchronous input.
//! Explicit trusted host capability; not a model tool, sandbox or remote server.
//! Spawn on a blocking worker. Hidden views do not stop output draining.
mod driver;
mod filter;
mod input;
pub use input::{encode_key, encode_paste, Key, Modifiers};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::{Deserialize, Serialize};
#[cfg(target_os = "linux")]
use std::sync::atomic::AtomicU64;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub type Result<T> = std::result::Result<T, Error>;
pub const MAX_INPUT: usize = 128 * 1024;
pub const MAX_PASTE: usize = 64 * 1024;
pub const SCROLLBACK: usize = 1000;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("terminal I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("terminal unavailable: {0}")]
    Unavailable(&'static str),
    #[error("terminal input queue full; input was not accepted")]
    Full,
    #[error("terminal request failed: {0}")]
    Backend(String),
}
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Running,
    Stopping,
    Exited(u32),
    Failed,
}
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Cell {
    pub text: String,
    pub wide: bool,
    pub continuation: bool,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Snapshot {
    pub id: String,
    pub root: PathBuf,
    pub owner: String,
    pub site: String,
    pub version: u64,
    pub rows: u16,
    pub cols: u16,
    pub cells: Vec<Cell>,
    pub cursor: (u16, u16),
    pub cursor_visible: bool,
    pub application_cursor: bool,
    pub bracketed_paste: bool,
    pub alternate_screen: bool,
    pub scrollback: usize,
    pub status: Status,
    pub notice: String,
    pub pending_input: usize,
}

/// Explicit process launch capability. Only trusted hosts may construct it.
/// Values are not serialized into layout state or model context.
pub struct Options {
    pub root: PathBuf,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub owner: String,
    pub site: String,
    pub rows: u16,
    pub cols: u16,
}
impl Options {
    pub fn bash(
        root: impl Into<PathBuf>,
        program: impl Into<PathBuf>,
        owner: impl Into<String>,
    ) -> Self {
        Self {
            root: root.into(),
            program: program.into(),
            args: vec!["--noprofile".into(), "--norc".into(), "-i".into()],
            owner: owner.into(),
            site: "local".into(),
            rows: 24,
            cols: 80,
        }
    }
}
type Notify = Arc<dyn Fn() + Send + Sync>;
#[derive(Default)]
pub(crate) struct Replies {
    pub bytes: Vec<u8>,
}
impl vt100::Callbacks for Replies {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        if self.bytes.len() > 1024 || i2.is_some() {
            return;
        }
        let first = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
        let reply = match (i1, c, first) {
            (None, 'n', 6) => {
                let (r, c) = screen.cursor_position();
                format!("\x1b[{};{}R", r + 1, c + 1)
            }
            (None, 'n', 5) => "\x1b[0n".into(),
            (None, 'c', 0) => "\x1b[?1;2c".into(),
            (Some(b'>'), 'c', 0) => "\x1b[>0;1;0c".into(),
            _ => return,
        };
        self.bytes.extend_from_slice(reply.as_bytes());
    }
}
pub(crate) struct State {
    pub parser: vt100::Parser<Replies>,
    pub version: u64,
    pub status: Status,
    pub notice: String,
    #[cfg(target_os = "linux")]
    pub quiescing: bool,
    #[cfg(target_os = "linux")]
    pub quiesce_epoch: u64,
}
pub(crate) struct Shared {
    pub state: Mutex<State>,
    pub stop: AtomicBool,
    pub pending: AtomicUsize,
    pub notify: Notify,
    #[cfg(target_os = "linux")]
    pub reader_pause_requested: AtomicBool,
    #[cfg(target_os = "linux")]
    pub reader_epoch: AtomicU64,
}
pub(crate) enum Message {
    Input(Vec<u8>),
    Resize(u16, u16),
    #[cfg(target_os = "linux")]
    Quiesce {
        epoch: u64,
        reply: mpsc::SyncSender<Result<()>>,
    },
    Stop,
}
/// One terminal resource. Multiple `Arc<Terminal>` views refer to the SAME PTY.
/// The UI host arbitrates which view may input/resize (one active local view).
pub struct Terminal {
    pub id: String,
    pub root: PathBuf,
    pub owner: String,
    pub site: String,
    shared: Arc<Shared>,
    tx: mpsc::SyncSender<Message>,
}
impl Terminal {
    pub fn spawn(options: Options, notify: impl Fn() + Send + Sync + 'static) -> Result<Self> {
        valid_size(options.rows, options.cols)?;
        let root = options.root.canonicalize()?;
        let program = options.program.canonicalize()?;
        if !root.is_dir() || !program.is_file() || !options.program.is_absolute() {
            return Err(Error::Unavailable(
                "an absolute trusted shell/program and real directory are required",
            ));
        }
        if options.owner.is_empty()
            || options.owner.len() > 128
            || options.site.len() > 128
            || options.args.len() > 32
            || options
                .args
                .iter()
                .any(|s| s.len() > 8192 || s.contains('\0'))
        {
            return Err(Error::Unavailable("invalid terminal metadata/arguments"));
        }
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: options.rows,
                cols: options.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(backend)?;
        let mut command = CommandBuilder::new(&program);
        command.args(&options.args);
        command.cwd(&root);
        command.env_clear();
        for key in ["PATH", "HOME", "LANG", "LC_CTYPE"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("SHELL", program.as_os_str());
        // No BASH_ENV/ENV/LD_*/API-token/agent-socket inheritance. The shell still
        // has the user's filesystem/network authority; this is NOT a sandbox.
        // portable-pty supplies the Unix PTY / Windows ConPTY reader/writer pair.
        #[cfg(target_os = "linux")]
        let monitor_fd = aw_platform::pty_poll::duplicate(
            pair.master
                .as_raw_fd()
                .ok_or(Error::Unavailable("native PTY descriptor missing"))?,
        )?;
        let reader = pair.master.try_clone_reader().map_err(backend)?;
        let writer = pair.master.take_writer().map_err(backend)?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                parser: vt100::Parser::new_with_callbacks(
                    options.rows,
                    options.cols,
                    SCROLLBACK,
                    Replies::default(),
                ),
                version: 0,
                status: Status::Running,
                notice: String::new(),
                #[cfg(target_os = "linux")]
                quiescing: false,
                #[cfg(target_os = "linux")]
                quiesce_epoch: 0,
            }),
            stop: AtomicBool::new(false),
            pending: AtomicUsize::new(0),
            notify: Arc::new(notify),
            #[cfg(target_os = "linux")]
            reader_pause_requested: AtomicBool::new(false),
            #[cfg(target_os = "linux")]
            reader_epoch: AtomicU64::new(0),
        });
        let child = driver::ChildGuard::new(pair.slave.spawn_command(command).map_err(backend)?);
        drop(pair.slave);
        let (tx, rx) = mpsc::sync_channel(32);
        let state = shared.clone();
        #[cfg(target_os = "linux")]
        let task = move || driver::run(pair.master, child, reader, writer, rx, state, monitor_fd);
        #[cfg(not(target_os = "linux"))]
        let task = move || driver::run(pair.master, child, reader, writer, rx, state);
        std::thread::Builder::new()
            .name("aw-terminal-pty".into())
            .spawn(task)?;
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_nanos();
        Ok(Self {
            id: format!(
                "pty_{}_{}_{}",
                std::process::id(),
                epoch,
                NEXT.fetch_add(1, Ordering::Relaxed)
            ),
            root,
            owner: options.owner,
            site: options.site,
            shared,
            tx,
        })
    }
    pub fn input_modes(&self) -> (bool, bool) {
        let s = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        (
            s.parser.screen().application_cursor(),
            s.parser.screen().bracketed_paste(),
        )
    }
    pub fn status(&self) -> Status {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status
    }
    pub fn input(&self, bytes: Vec<u8>) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let guard = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if guard.status != Status::Running {
            return Err(Error::Unavailable("terminal is not running"));
        }
        #[cfg(target_os = "linux")]
        if guard.quiescing {
            return Err(Error::Unavailable(
                "PTY owner is quiescing; input not accepted",
            ));
        }
        let len = bytes.len();
        if len > MAX_PASTE + 32 {
            return Err(Error::Full);
        }
        self.shared
            .pending
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(len).filter(|n| *n <= MAX_INPUT)
            })
            .map_err(|_| Error::Full)?;
        if self.tx.try_send(Message::Input(bytes)).is_err() {
            self.shared.pending.fetch_sub(len, Ordering::AcqRel);
            return Err(Error::Full);
        }
        drop(guard);
        Ok(())
    }
    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        valid_size(rows, cols)?;
        let state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.status != Status::Running {
            return Err(Error::Unavailable("terminal is not running"));
        }
        #[cfg(target_os = "linux")]
        if state.quiescing {
            return Err(Error::Unavailable(
                "PTY owner is quiescing; resize not accepted",
            ));
        }
        if state.parser.screen().size() == (rows, cols) {
            return Ok(());
        }
        self.tx
            .try_send(Message::Resize(rows, cols))
            .map_err(|_| Error::Full)?;
        Ok(())
    }

    /// Reversible Linux single-reader cutover barrier. Deny new input/resize,
    /// drain prior commands, then await the only PTY reader's pause receipt.
    /// This does NOT release child custody, clone a parser or authorize any
    /// broker to consume escrowed FDs.
    #[cfg(target_os = "linux")]
    pub fn quiesce(&self, timeout: Duration) -> Result<Quiesced<'_>> {
        let (tx, rx) = mpsc::sync_channel(1);
        let epoch;
        {
            let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.status != Status::Running || self.shared.stop.load(Ordering::Acquire) {
                return Err(Error::Unavailable("terminal no longer running"));
            }
            if state.quiescing {
                return Err(Error::Unavailable("PTY reader quiescence already pending"));
            }
            epoch = state
                .quiesce_epoch
                .checked_add(1)
                .ok_or(Error::Unavailable("PTY reader generation exhausted"))?;
            state.quiesce_epoch = epoch;
            state.quiescing = true;
            if self
                .tx
                .try_send(Message::Quiesce { epoch, reply: tx })
                .is_err()
            {
                state.quiescing = false;
                return Err(Error::Full);
            }
        }
        let response = rx.recv_timeout(timeout.min(Duration::from_secs(5)));
        match response {
            Ok(Ok(())) => Ok(Quiesced {
                terminal: self,
                epoch,
            }),
            _ => {
                self.release_quiescence(epoch);
                Err(Error::Unavailable(
                    "PTY quiescence unconfirmed; original reader resumed",
                ))
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn release_quiescence(&self, epoch: u64) {
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.quiescing && state.quiesce_epoch == epoch {
            state.quiescing = false;
            self.shared
                .reader_pause_requested
                .store(false, Ordering::Release);
        }
    }
    pub fn terminate(&self) {
        self.shared.stop.store(true, Ordering::Release);
        let _ = self.tx.try_send(Message::Stop);
    }
    pub fn snapshot(&self, scrollback: usize) -> Snapshot {
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        state
            .parser
            .screen_mut()
            .set_scrollback(scrollback.min(SCROLLBACK));
        let s = state.parser.screen();
        let (rows, cols) = s.size();
        let mut cells = Vec::with_capacity(usize::from(rows) * usize::from(cols));
        for row in 0..rows {
            for col in 0..cols {
                if let Some(cell) = s.cell(row, col) {
                    cells.push(Cell {
                        text: cell.contents().to_owned(),
                        wide: cell.is_wide(),
                        continuation: cell.is_wide_continuation(),
                        fg: color(cell.fgcolor()),
                        bg: color(cell.bgcolor()),
                        bold: cell.bold(),
                        dim: cell.dim(),
                        italic: cell.italic(),
                        underline: cell.underline(),
                        inverse: cell.inverse(),
                    });
                }
            }
        }
        let result = Snapshot {
            id: self.id.clone(),
            root: self.root.clone(),
            owner: self.owner.clone(),
            site: self.site.clone(),
            version: state.version,
            rows,
            cols,
            cells,
            cursor: s.cursor_position(),
            cursor_visible: !s.hide_cursor() && s.scrollback() == 0,
            application_cursor: s.application_cursor(),
            bracketed_paste: s.bracketed_paste(),
            alternate_screen: s.alternate_screen(),
            scrollback: s.scrollback(),
            status: state.status,
            notice: state.notice.clone(),
            pending_input: self.shared.pending.load(Ordering::Acquire),
        };
        state.parser.screen_mut().set_scrollback(0);
        result
    }
}
/// Exclusive reversible source-reader pause. No ownership is transferred.
/// RAII rollback resumes the original driver, including after an aborted
/// handoff. A recipient has no authority simply because this guard exists.
#[cfg(target_os = "linux")]
#[must_use = "dropping the reader gate resumes the original owner"]
pub struct Quiesced<'a> {
    terminal: &'a Terminal,
    epoch: u64,
}
#[cfg(target_os = "linux")]
impl Quiesced<'_> {
    pub fn snapshot(&self) -> Snapshot {
        self.terminal.snapshot(0)
    }
}
#[cfg(target_os = "linux")]
impl Drop for Quiesced<'_> {
    fn drop(&mut self) {
        self.terminal.release_quiescence(self.epoch);
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        self.terminate();
    }
}
pub fn valid_size(rows: u16, cols: u16) -> Result<()> {
    if !(2..=120).contains(&rows) || !(4..=300).contains(&cols) {
        Err(Error::Unavailable(
            "terminal size must be 4–300 columns by 2–120 rows",
        ))
    } else {
        Ok(())
    }
}
fn color(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Default,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn backend(e: impl std::fmt::Display) -> Error {
    Error::Backend(e.to_string())
}
pub(crate) fn changed(shared: &Shared, f: impl FnOnce(&mut State)) {
    {
        let mut s = shared.state.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut s);
        s.version = s.version.wrapping_add(1);
    }
    (shared.notify)();
}

#[cfg(test)]
mod handoff_checkpoint_tests {
    #[test]
    fn formatted_screen_replays_visible_cells_but_not_alternate_screen_ownership() {
        let mut origin = vt100::Parser::new(8, 40, 100);
        origin.process(b"\x1b[1;31mcolored text\x1b[0m\x1b[?2004h");
        let formatted = origin.screen().state_formatted();
        let mut receiver = vt100::Parser::new(8, 40, 100);
        receiver.process(&formatted);
        assert_eq!(receiver.screen().contents(), origin.screen().contents());
        assert_eq!(
            receiver.screen().bracketed_paste(),
            origin.screen().bracketed_paste()
        );

        // state_formatted() is a *visible display* reconstruction, not a
        // guaranteed parser checkpoint. The inactive screen, scrollback and
        // any partially received escape sequence need a separate transfer
        // design before migrating an arbitrary running application.
        origin.process(b"\x1b[?1049h\x1b[1;32malternate app\x1b[0m");
        assert!(origin.screen().alternate_screen());
        let mut fresh = vt100::Parser::new(8, 40, 100);
        fresh.process(&origin.screen().state_formatted());
        assert_eq!(fresh.screen().contents(), origin.screen().contents());
        assert!(!fresh.screen().alternate_screen());
    }
}
