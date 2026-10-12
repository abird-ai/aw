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
