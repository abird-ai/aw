//! One idempotent native-terminal ownership/restore seam.
//!
//! Agent Scrollback/Inline/Fullscreen and Workspace all acquire this guard.
//! Normal return, `?` errors, unwinding panics and catchable Unix shutdown
//! signals converge on the same best-effort restore routine. SIGKILL, power
//! loss and unrecoverable process/kernel failure are outside an in-process
//! program's ability to repair.

use aw::{Error, Result};
use crossterm::{
    cursor::{Hide, MoveTo, MoveToColumn, Show},
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    style::{Attribute, ResetColor, SetAttribute},
    terminal::{
        disable_raw_mode, enable_raw_mode, size as terminal_size, supports_keyboard_enhancement,
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use std::{
    cell::Cell,
    io::{self, IsTerminal, Write},
    sync::{
        atomic::{AtomicU32, AtomicU64, Ordering},
        Mutex, Once, OnceLock,
    },
    thread::ThreadId,
};

const RAW: u32 = 1 << 0;
const PASTE: u32 = 1 << 1;
const MOUSE: u32 = 1 << 2;
const CURSOR_HIDDEN: u32 = 1 << 3;
const ALTERNATE: u32 = 1 << 4;
const PRIMARY_OWNED: u32 = 1 << 5;
const KEYBOARD_ENHANCED: u32 = 1 << 6;
const RESTORING_OWNER: u64 = u64::MAX;

static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
static ACTIVE_OWNER: AtomicU64 = AtomicU64::new(0);
static ACTIVE_FLAGS: AtomicU32 = AtomicU32::new(0);
/// One-based physical row where an owned-primary presenter wants the shell to
/// resume. Zero means no explicit row has been published yet.
static PRIMARY_EXIT_ROW: AtomicU32 = AtomicU32::new(0);
static ACTIVE_THREAD: Mutex<Option<ThreadId>> = Mutex::new(None);
static HOOKS: Once = Once::new();
static KEYBOARD_ENHANCEMENT_SUPPORTED: OnceLock<bool> = OnceLock::new();

thread_local! {
    static ISOLATED_PANIC: Cell<bool> = const { Cell::new(false) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Surface {
    Scrollback,
    PrimaryOwned,
    Alternate { mouse: bool },
}

#[derive(Debug)]
pub(crate) struct TerminalSession {
    owner: u64,
    active: bool,
}

impl TerminalSession {
    pub(crate) fn enter(surface: Surface) -> Result<Self> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(Error::Config(
                "native terminal presentation requires a real terminal; use --output-print for redirected output"
                    .into(),
            ));
        }

        let owner = NEXT_OWNER.fetch_add(1, Ordering::Relaxed).max(1);
        ACTIVE_OWNER
            .compare_exchange(0, owner, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::Storage("another native terminal presenter is already active"))?;
        ACTIVE_FLAGS.store(0, Ordering::Release);
        PRIMARY_EXIT_ROW.store(0, Ordering::Release);
        *ACTIVE_THREAD.lock().unwrap_or_else(|error| error.into_inner()) =
            Some(std::thread::current().id());
        let mut guard = Self {
            owner,
            active: true,
        };

        let setup = (|| -> Result<()> {
            if surface == Surface::PrimaryOwned {
                guard.mark(PRIMARY_OWNED);
            }
            enable_raw_mode()?;
            guard.mark(RAW);

            let known_direct_keyboard_protocol = [
                "KITTY_WINDOW_ID",
                "WEZTERM_PANE",
                "GHOSTTY_RESOURCES_DIR",
                "FOOT_CLIENT_PID",
            ]
            .iter()
            .any(|name| std::env::var_os(name).is_some());
            let in_tmux = std::env::var_os("TMUX").is_some();
            let tmux_supports_keyboard_protocol = in_tmux
                && *KEYBOARD_ENHANCEMENT_SUPPORTED
                    .get_or_init(|| supports_keyboard_enhancement().unwrap_or(false));
            let keyboard_enhanced = keyboard_enhancement_allowed(
                in_tmux,
                known_direct_keyboard_protocol,
                tmux_supports_keyboard_protocol,
            );
            if keyboard_enhanced {
                execute!(
                    io::stdout(),
                    PushKeyboardEnhancementFlags(
                        KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                            | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                            | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                            | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                    )
                )?;
                guard.mark(KEYBOARD_ENHANCED);
            }

            if matches!(surface, Surface::Alternate { .. }) {
                execute!(io::stdout(), EnterAlternateScreen)?;
                guard.mark(ALTERNATE);
            }

            execute!(io::stdout(), EnableBracketedPaste)?;
            guard.mark(PASTE);

            if matches!(surface, Surface::Alternate { mouse: true }) {
                execute!(io::stdout(), EnableMouseCapture)?;
                guard.mark(MOUSE);
            }

            execute!(io::stdout(), Hide)?;
            guard.mark(CURSOR_HIDDEN);
            Ok(())
        })();

        if let Err(error) = setup {
            guard.restore();
            return Err(error);
        }
        Ok(guard)
    }

    fn mark(&mut self, bit: u32) {
        if ACTIVE_OWNER.load(Ordering::Acquire) == self.owner {
            ACTIVE_FLAGS.fetch_or(bit, Ordering::AcqRel);
        }
    }

    pub(crate) fn set_primary_exit_row(&self, row: u16) {
        if self.active
            && ACTIVE_OWNER.load(Ordering::Acquire) == self.owner
            && ACTIVE_FLAGS.load(Ordering::Acquire) & PRIMARY_OWNED != 0
        {
            PRIMARY_EXIT_ROW.store(u32::from(row).saturating_add(1), Ordering::Release);
        }
    }

    pub(crate) fn restore(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        restore_owner(self.owner);
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        self.restore();
    }
}

fn keyboard_enhancement_allowed(
    in_tmux: bool,
    known_direct_keyboard_protocol: bool,
    tmux_reports_support: bool,
) -> bool {
    if in_tmux {
        // Outer-terminal environment variables are commonly inherited through
        // tmux. Once tmux is the immediate peer, only tmux's own positive
        // capability report is authoritative for enhanced-key transport.
        tmux_reports_support
    } else {
        known_direct_keyboard_protocol
    }
}

fn restore_owner(owner: u64) {
    if ACTIVE_OWNER
        .compare_exchange(
            owner,
            RESTORING_OWNER,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        return;
    }
    let flags = ACTIVE_FLAGS.swap(0, Ordering::AcqRel);
    *ACTIVE_THREAD.lock().unwrap_or_else(|error| error.into_inner()) = None;
    restore_flags(flags);
    ACTIVE_OWNER.store(0, Ordering::Release);
}

fn restore_flags(flags: u32) {
    if flags == 0 {
        return;
    }

    let mut out = io::stdout();
    if flags & MOUSE != 0 {
        let _ = execute!(out, DisableMouseCapture);
    }
    if flags & PASTE != 0 {
        let _ = execute!(out, DisableBracketedPaste);
    }
    if flags & KEYBOARD_ENHANCED != 0 {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
    let _ = execute!(out, SetAttribute(Attribute::Reset), ResetColor);
    if flags & CURSOR_HIDDEN != 0 {
        let _ = execute!(out, Show);
    }
    if flags & ALTERNATE != 0 {
        let _ = execute!(out, LeaveAlternateScreen);
    } else if flags & PRIMARY_OWNED != 0 {
        // Keep the settled transcript above the shell, but erase all transient
        // owned-primary chrome from the exact row published by the presenter.
        // Never force the shell to the physical bottom: that creates a large
        // blank gap after short sessions or terminal resizes.
        let (_, height) = terminal_size().unwrap_or((1, 1));
        let row = PRIMARY_EXIT_ROW
            .swap(0, Ordering::AcqRel)
            .checked_sub(1)
            .and_then(|value| u16::try_from(value).ok())
            .unwrap_or_else(|| height.saturating_sub(1))
            .min(height.saturating_sub(1));
        let _ = execute!(
            out,
            MoveTo(0, row),
            Clear(ClearType::FromCursorDown),
            MoveToColumn(0)
        );
    }
    let _ = out.flush();
    if flags & RAW != 0 {
        let _ = disable_raw_mode();
    }
}

pub(crate) fn emergency_restore() {
    loop {
        let owner = ACTIVE_OWNER.load(Ordering::Acquire);
        if owner == 0 {
            return;
        }
        if owner == RESTORING_OWNER {
            std::thread::yield_now();
            continue;
        }
        if ACTIVE_OWNER
            .compare_exchange(
                owner,
                RESTORING_OWNER,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            break;
        }
    }
    let flags = ACTIVE_FLAGS.swap(0, Ordering::AcqRel);
    *ACTIVE_THREAD.lock().unwrap_or_else(|error| error.into_inner()) = None;
    restore_flags(flags);
    ACTIVE_OWNER.store(0, Ordering::Release);
}

fn restore_for_owner_thread_panic() {
    let current = std::thread::current().id();
    let owner = *ACTIVE_THREAD.lock().unwrap_or_else(|error| error.into_inner());
    if owner == Some(current) {
        emergency_restore();
    }
}

fn panic_is_isolated() -> bool {
    ISOLATED_PANIC.get()
}

#[cfg(any(feature = "workspace", test))]
pub(crate) fn catch_isolated_panic<T>(f: impl FnOnce() -> T) -> std::thread::Result<T> {
    ISOLATED_PANIC.with(|flag| {
        let previous = flag.replace(true);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        flag.set(previous);
        result
    })
}

pub(crate) fn maybe_test_panic_after_terminal_enter() {
    if std::env::var_os("AW_TEST_PANIC_AFTER_TUI").is_some() {
        std::env::remove_var("AW_TEST_PANIC_AFTER_TUI");
        panic!("AW_TEST_PANIC_AFTER_TUI");
    }
}

pub(crate) fn install_emergency_restore_hooks() {
    HOOKS.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // UI-background jobs are explicitly wrapped in catch_unwind and
            // converted to typed failures. Their panic must not dismantle the
            // terminal owned by the main/UI thread or print a misleading
            // process-level panic diagnostic.
            if panic_is_isolated() {
                return;
            }
            restore_for_owner_thread_panic();
            previous(info);
        }));

        #[cfg(unix)]
        {
            use signal_hook::{
                consts::signal::{SIGHUP, SIGQUIT, SIGTERM},
                iterator::Signals,
            };
            if let Ok(mut signals) = Signals::new([SIGHUP, SIGTERM, SIGQUIT]) {
                let _ = std::thread::Builder::new()
                    .name("aw-terminal-restore".into())
                    .spawn(move || {
                        if let Some(signal) = signals.forever().next() {
                            emergency_restore();
                            std::process::exit(128 + signal);
                        }
                    });
            }
        }
    });
}

#[cfg(test)]
pub(crate) fn active_for_test() -> bool {
    ACTIVE_OWNER.load(Ordering::Acquire) != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TERMINAL_GLOBALS: Mutex<()> = Mutex::new(());

    fn reset() {
        ACTIVE_OWNER.store(0, Ordering::Release);
        ACTIVE_FLAGS.store(0, Ordering::Release);
        PRIMARY_EXIT_ROW.store(0, Ordering::Release);
        *ACTIVE_THREAD.lock().unwrap_or_else(|error| error.into_inner()) = None;
    }

    #[test]
    fn keyboard_enhancement_respects_the_immediate_tmux_boundary() {
        assert!(keyboard_enhancement_allowed(false, true, false));
        assert!(!keyboard_enhancement_allowed(false, false, false));
        assert!(
            !keyboard_enhancement_allowed(true, true, false),
            "inherited Kitty/WezTerm markers must not bypass unsupported tmux"
        );
        assert!(keyboard_enhancement_allowed(true, true, true));
        assert!(keyboard_enhancement_allowed(true, false, true));
    }

    #[test]
    fn emergency_restore_is_idempotent_without_active_owner() {
        let _guard = TERMINAL_GLOBALS.lock().unwrap();
        reset();
        emergency_restore();
        emergency_restore();
        assert!(!active_for_test());
    }

    #[test]
    fn panic_restore_is_scoped_to_terminal_owner_thread() {
        let _guard = TERMINAL_GLOBALS.lock().unwrap();
        reset();
        ACTIVE_OWNER.store(41, Ordering::Release);
        *ACTIVE_THREAD.lock().unwrap_or_else(|error| error.into_inner()) =
            Some(std::thread::current().id());

        std::thread::spawn(restore_for_owner_thread_panic)
            .join()
            .unwrap();
        assert_eq!(ACTIVE_OWNER.load(Ordering::Acquire), 41);

        restore_for_owner_thread_panic();
        assert_eq!(ACTIVE_OWNER.load(Ordering::Acquire), 0);
        reset();
    }

    #[test]
    fn isolated_panic_on_terminal_owner_thread_does_not_restore_terminal() {
        let _guard = TERMINAL_GLOBALS.lock().unwrap();
        reset();
        ACTIVE_OWNER.store(41, Ordering::Release);
        *ACTIVE_THREAD.lock().unwrap_or_else(|error| error.into_inner()) =
            Some(std::thread::current().id());

        let result = catch_isolated_panic(|| panic!("caught background-style panic"));
        assert!(result.is_err());
        assert_eq!(ACTIVE_OWNER.load(Ordering::Acquire), 41);
        assert_eq!(
            *ACTIVE_THREAD.lock().unwrap_or_else(|error| error.into_inner()),
            Some(std::thread::current().id())
        );
        reset();
    }

    #[test]
    fn isolated_panic_marker_is_scoped_to_wrapped_closure() {
        assert!(!panic_is_isolated());
        let result = catch_isolated_panic(|| {
            assert!(panic_is_isolated());
            panic!("isolated fixture");
        });
        assert!(result.is_err());
        assert!(!panic_is_isolated());
    }

    #[test]
    fn emergency_restore_waits_for_an_in_progress_restore_generation() {
        let _guard = TERMINAL_GLOBALS.lock().unwrap();
        reset();
        ACTIVE_OWNER.store(RESTORING_OWNER, Ordering::Release);
        let releaser = std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            ACTIVE_OWNER.store(0, Ordering::Release);
        });
        let started = std::time::Instant::now();
        emergency_restore();
        releaser.join().unwrap();
        assert!(started.elapsed() >= std::time::Duration::from_millis(5));
        assert_eq!(ACTIVE_OWNER.load(Ordering::Acquire), 0);
        reset();
    }

    #[test]
    fn restoring_generation_is_not_available_to_a_new_presenter() {
        let _guard = TERMINAL_GLOBALS.lock().unwrap();
        reset();
        ACTIVE_OWNER.store(RESTORING_OWNER, Ordering::Release);
        assert!(ACTIVE_OWNER
            .compare_exchange(0, 77, Ordering::AcqRel, Ordering::Acquire)
            .is_err());
        reset();
    }

    #[test]
    fn stale_owner_cannot_restore_a_newer_owner() {
        let _guard = TERMINAL_GLOBALS.lock().unwrap();
        reset();
        // Pure owner-generation invariant; no physical terminal mutation.
        ACTIVE_OWNER.store(41, Ordering::Release);
        ACTIVE_FLAGS.store(PASTE, Ordering::Release);
        restore_owner(40);
        assert_eq!(ACTIVE_OWNER.load(Ordering::Acquire), 41);
        assert_eq!(ACTIVE_FLAGS.load(Ordering::Acquire), PASTE);
        reset();
    }
}
