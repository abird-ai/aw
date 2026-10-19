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
    assert!(
        gate.checkpoint().is_err(),
        "terminate while paused may not produce a transferable checkpoint"
    );
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

#[cfg(target_os = "linux")]
#[test]
fn quiesced_raw_replay_keeps_live_original_bash_modes_and_rolls_back() {
    let t = Terminal::spawn(
        options(
            "printf 'MAIN_MARK\\n\\033[?1049h\\033[?2004h\\033[1;31mALT_MARK'; \
             read -r answer; printf '\\033[0m END=%s' \"$answer\"; sleep 2",
        ),
        || {},
    )
    .unwrap();
    wait(&t, "ALT_MARK");
    let paused = t
        .quiesce(Duration::from_secs(2))
        .expect("single original owner must pause");
    let original = paused.snapshot();
    assert!(original.alternate_screen);
    assert!(original.bracketed_paste);
    assert!(original
        .cells
        .iter()
        .any(|cell| cell.fg == aw_terminal::Color::Indexed(1)));
    let checkpoint = paused
        .checkpoint()
        .expect("bounded original PTY output replayable");
    assert_eq!(checkpoint.terminal_id(), original.id);
    assert_eq!(checkpoint.original_root(), &t.root);
    assert_eq!(checkpoint.original_owner(), &t.owner);
    assert_eq!(checkpoint.original_site(), t.site);
    assert!(checkpoint.retained_bytes() > 0);
    let receiver = checkpoint.replay().expect("replay must restore VT states");
    assert!(receiver.screen().alternate_screen());
    assert!(receiver.screen().bracketed_paste());
    assert_eq!(receiver.screen().contents(), "ALT_MARK");
    assert_eq!(receiver.screen().size(), (original.rows, original.cols));
    for (index, expected) in original.cells.iter().enumerate() {
        let row = (index / usize::from(original.cols)) as u16;
        let col = (index % usize::from(original.cols)) as u16;
        let current = receiver.screen().cell(row, col).unwrap();
        assert_eq!(current.contents(), expected.text);
        assert_eq!(current.bold(), expected.bold);
        assert_eq!(current.is_wide(), expected.wide);
        assert_eq!(current.is_wide_continuation(), expected.continuation);
        if expected.fg == aw_terminal::Color::Indexed(1) {
            assert_eq!(current.fgcolor(), vt100::Color::Idx(1));
        }
    }
    assert!(t.input(b"NOT_ALLOWED\r".to_vec()).is_err());
    drop(paused); // rollback must resume the one original shell and reader
    t.input(b"accepted\r".to_vec()).unwrap();
    wait(&t, "END=accepted");
    assert_eq!(t.id, original.id);
    t.terminate();
}

#[cfg(target_os = "linux")]
#[test]
fn quiesced_checkpoint_replays_a_live_pty_resize_before_cutover() {
    let t = Terminal::spawn(
        options("printf 'BEFORE_RESIZE'; read -r answer; stty size; sleep 2"),
        || {},
    )
    .unwrap();
    wait(&t, "BEFORE_RESIZE");
    t.resize(19, 47).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while t.snapshot(0).rows != 19 || t.snapshot(0).cols != 47 {
        assert!(
            Instant::now() < deadline,
            "original PTY resize did not apply"
        );
        thread::sleep(Duration::from_millis(15));
    }
    let gated = t.quiesce(Duration::from_secs(2)).unwrap();
    let original = gated.snapshot();
    let replay = gated.checkpoint().unwrap().replay().unwrap();
    assert_eq!(replay.screen().size(), (19, 47));
    let content = original
        .cells
        .chunks(usize::from(original.cols))
        .map(|line| line.iter().map(|c| c.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(replay.screen().contents().trim_end(), content.trim_end());
    drop(gated);
    t.input(b"continue\r".to_vec()).unwrap();
    wait(&t, "19 47");
    t.terminate();
}

#[cfg(target_os = "linux")]
#[test]
fn checkpoint_receiver_child_process() {
    if std::env::var_os("AW_TEST_CHECKPOINT_CHILD").is_none() {
        return; // ordinary integration-test invocation
    }
    use std::io::Read;
    let mut encoded = Vec::new();
    std::io::stdin()
        .take((aw_terminal::MAX_CHECKPOINT_WIRE_BYTES + 1) as u64)
        .read_to_end(&mut encoded)
        .unwrap();
    let checkpoint = aw_terminal::Checkpoint::from_wire(&encoded)
        .expect("independent process must reject unsupported checkpoint data");
    assert!(checkpoint.terminal_id().starts_with("pty_"));
    assert_eq!(checkpoint.original_owner(), "fixture");
    let mut restored = checkpoint.replay().unwrap();
    assert!(restored.screen().alternate_screen());
    assert!(restored.screen().bracketed_paste());
    assert!(restored.screen().contents().contains("ALT_CHILD"));
    restored.process(b"\x1b[?1049l");
    assert!(!restored.screen().alternate_screen());
    assert!(restored.screen().contents().contains("MAIN_CHILD"));
}

#[cfg(target_os = "linux")]
#[test]
fn real_quiesced_shell_checkpoint_replays_in_independent_process_without_release() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let t = Terminal::spawn(
        options(
            "printf 'MAIN_CHILD\\r\\n\\033[?1049h\\033[?2004hALT_CHILD'; \
             read -r command; printf '\\033[?1049lRESUMED=%s' \"$command\"; sleep 2",
        ),
        || {},
    )
    .unwrap();
    wait(&t, "ALT_CHILD");
    let original = t.snapshot(0);
    let gate = t.quiesce(Duration::from_secs(2)).unwrap();
    let bytes = gate.checkpoint().unwrap().to_wire().unwrap();
    let executable = std::env::current_exe().unwrap();
    let mut child = Command::new(executable)
        .args([
            "--exact",
            "checkpoint_receiver_child_process",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("AW_TEST_CHECKPOINT_CHILD", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&bytes).unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "independent checkpoint replay failed: {} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(t.snapshot(0).id, original.id);
    assert_eq!(t.snapshot(0).version, original.version);
    assert!(t.input(b"ILLEGAL\r".to_vec()).is_err());
    drop(gate); // cross-process replay was never an ownership transfer
    t.input(b"accepted\r".to_vec()).unwrap();
    wait(&t, "RESUMED=accepted");
    assert_eq!(t.id, original.id);
    t.terminate();
}
