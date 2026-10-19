//! Runs actual PTYs on the Rust validation host. These are not screenshot tests.
use aw_terminal::{encode_key, Key, Modifiers, Options, Status, Terminal};
use std::{
    path::PathBuf,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
fn bash() -> PathBuf {
    std::env::var_os("AW_BASH")
        .map(PathBuf::from)
        .or_else(|| option_env!("AW_DEFAULT_BASH").map(PathBuf::from))
        .unwrap_or_else(|| "/bin/bash".into())
}
fn options(script: &str) -> Options {
    let mut o = Options::bash(std::env::temp_dir(), bash(), "fixture");
    o.args = vec![
        "--noprofile".into(),
        "--norc".into(),
        "-c".into(),
        script.into(),
    ];
    o
}
fn text(t: &Terminal) -> String {
    let s = t.snapshot(0);
    s.cells
        .chunks(usize::from(s.cols))
        .map(|row| row.iter().map(|c| c.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}
fn wait(t: &Terminal, expected: &str) {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        if text(t).contains(expected) {
            return;
        }
        assert!(Instant::now() < end, "expected {expected}, got {}", text(t));
        thread::sleep(Duration::from_millis(20));
    }
}
#[test]
fn shell_output_drains_without_ui() {
    let t = Terminal::spawn(
        options("printf 'hello-from-pty'; sleep .1; printf ' later'"),
        || {},
    )
    .unwrap();
    thread::sleep(Duration::from_millis(250));
    wait(&t, "hello-from-pty later");
}
#[test]
fn controlling_terminal_exists() {
    let t = Terminal::spawn(options("test -t 0 && test -t 1 && printf 'TTY_OK'"), || {}).unwrap();
    wait(&t, "TTY_OK");
}
#[test]
fn resize_changes_stty_dimensions() {
    let t = Terminal::spawn(options("read -r x; stty size"), || {}).unwrap();
    t.resize(31, 91).unwrap();
    thread::sleep(Duration::from_millis(50));
    t.input(b"go\r".to_vec()).unwrap();
    wait(&t, "31 91");
}
#[test]
fn alternate_screen_state_and_sgr() {
    let t = Terminal::spawn(options("printf '\\033[?1049h\\033[31mRED'; sleep 2"), || {}).unwrap();
    wait(&t, "RED");
    assert!(t.snapshot(0).alternate_screen);
    assert!(t
        .snapshot(0)
        .cells
        .iter()
        .any(|c| c.fg == aw_terminal::Color::Indexed(1)));
    t.terminate();
}
#[test]
fn unsupported_osc_has_no_host_action() {
    let t = Terminal::spawn(options("printf '\\033]52;c;SECRET\\007visible'"), || {}).unwrap();
    wait(&t, "visible");
    assert!(!text(&t).contains("SECRET"));
}
#[test]
fn ctrl_c_interrupts_foreground_task() {
    let t = Terminal::spawn(
        options("trap 'printf INTERRUPTED; exit 0' INT; printf READY; while :; do sleep 1; done"),
        || {},
    )
    .unwrap();
    wait(&t, "READY");
    t.input(
        encode_key(
            Key::Char('c'),
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            false,
        )
        .unwrap(),
    )
    .unwrap();
    wait(&t, "INTERRUPTED");
}
#[test]
fn clone_view_does_not_spawn_second_process() {
    let t = Arc::new(Terminal::spawn(options("printf ONCE; sleep 2"), || {}).unwrap());
    let v = t.clone();
    assert_eq!(t.id, v.id);
    wait(&v, "ONCE");
    drop(v);
    assert!(matches!(t.status(), Status::Running));
    t.terminate();
}
#[test]
fn exit_status_is_recorded() {
    let t = Terminal::spawn(options("exit 7"), || {}).unwrap();
    let end = Instant::now() + Duration::from_secs(3);
    while t.status() == Status::Running && Instant::now() < end {
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(t.status(), Status::Exited(7));
}
#[test]
fn terminate_stops_waiting_shell() {
    let t = Terminal::spawn(options("sleep 30"), || {}).unwrap();
    t.terminate();
    let end = Instant::now() + Duration::from_secs(4);
    while matches!(t.status(), Status::Running | Status::Stopping) && Instant::now() < end {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(!matches!(t.status(), Status::Running | Status::Stopping));
}
#[test]
fn invalid_size_rejected_without_launch() {
    let mut o = options("echo SHOULD_NOT_LAUNCH");
    o.rows = 0;
    assert!(Terminal::spawn(o, || {}).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn reversible_reader_pause_blocks_source_actions_and_restores_same_shell() {
    let t = Terminal::spawn(
        options(
            "printf 'ORIGINAL_PID=%s\\nREADY\\n' \"$$\"; \
         sleep .4; printf 'HELD_OUTPUT\\n'; \
         read -r answer; printf 'SAME_PID=%s ANSWER=%s\\n' \"$$\" \"$answer\"; sleep .7",
        ),
        || {},
    )
    .unwrap();
    wait(&t, "READY");
    let before = t.snapshot(0);
    let cutover = t
        .quiesce(Duration::from_secs(2))
        .expect("original reader must pause");
    assert_eq!(cutover.snapshot().id, before.id);
    assert_eq!(cutover.snapshot().version, t.snapshot(0).version);
    assert_eq!(cutover.snapshot().pending_input, 0);
    assert!(t.input(b"SHOULD_NOT_ENTER\r".to_vec()).is_err());
    assert!(t.resize(30, 100).is_err());
    assert!(
        t.quiesce(Duration::from_millis(20)).is_err(),
        "a second controller must not establish another reader gate"
    );
    thread::sleep(Duration::from_millis(650));
    assert!(
        !text(&t).contains("HELD_OUTPUT"),
        "the paused reader must not consume output generated by the shell"
    );
    drop(cutover);
    wait(&t, "HELD_OUTPUT");
    t.input(b"accepted\r".to_vec()).unwrap();
    wait(&t, "ANSWER=accepted");
    let screen = text(&t);
    let original = screen
        .split("ORIGINAL_PID=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    let restored = screen
        .split("SAME_PID=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    assert_eq!(original, restored, "rollback must never start a new shell");
    t.terminate();
}

#[cfg(target_os = "linux")]
#[test]
fn reader_pause_timeout_and_repeated_guards_cannot_leave_source_fenced() {
    let t = Terminal::spawn(
        options("printf READY; while read -r command; do printf 'OK=%s\\n' \"$command\"; done"),
        || {},
    )
    .unwrap();
    wait(&t, "READY");
    assert!(
        t.quiesce(Duration::ZERO).is_err(),
        "an unconfirmed pause must be rolled back, not assumed successful"
    );
    for round in 0..24 {
        // Rapid cancellation and a new generation may overlap a delayed
        // pause acknowledgment. It must not consume the new receipt.
        if round % 2 == 0 {
            assert!(t.quiesce(Duration::ZERO).is_err());
        }
        let gated = t.quiesce(Duration::from_secs(2)).unwrap();
        assert_eq!(gated.snapshot().status, Status::Running);
        drop(gated);
    }
    t.input(b"back-in-control\r".to_vec()).unwrap();
    wait(&t, "OK=back-in-control");
    let gate = t.quiesce(Duration::from_secs(2)).unwrap();
    t.terminate();
    drop(gate);
    let deadline = Instant::now() + Duration::from_secs(4);
    while matches!(t.status(), Status::Running | Status::Stopping) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !matches!(t.status(), Status::Running | Status::Stopping),
        "termination during pause may not strand the PTY reader or child"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn reader_cutover_rollback_preserves_alternate_screen_and_input_modes() {
    let t = Terminal::spawn(
        options(
            "printf '\\033[?1049h\\033[?2004h\\033[1;31mALT_INIT'; \
         sleep .4; printf ' ALT_AFTER'; sleep .8",
        ),
        || {},
    )
    .unwrap();
    wait(&t, "ALT_INIT");
    assert!(t.snapshot(0).alternate_screen);
    let gate = t.quiesce(Duration::from_secs(2)).unwrap();
    let paused = gate.snapshot();
    assert!(paused.alternate_screen);
    assert!(paused.bracketed_paste);
    assert_eq!(paused.id, t.id);
    assert!(paused
        .cells
        .iter()
        .any(|cell| cell.fg == aw_terminal::Color::Indexed(1)));
    thread::sleep(Duration::from_millis(520));
    assert!(!text(&t).contains("ALT_AFTER"));
    drop(gate);
    wait(&t, "ALT_AFTER");
    let resumed = t.snapshot(0);
    assert!(resumed.alternate_screen);
    assert!(resumed.bracketed_paste);
    assert_eq!(resumed.id, paused.id);
    t.terminate();
}
