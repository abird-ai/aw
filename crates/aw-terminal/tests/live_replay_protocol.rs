//! Reader replay alone is passive. Only an independently authorized,
//! live PTY adopter may forward new terminal-protocol replies.
#![cfg(target_os = "linux")]

use aw_terminal::{Options, Terminal};
use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

fn bash() -> PathBuf {
    std::env::var_os("AW_BASH")
        .map(PathBuf::from)
        .or_else(|| option_env!("AW_DEFAULT_BASH").map(PathBuf::from))
        .unwrap_or_else(|| "/bin/bash".into())
}
fn readable(t: &Terminal) -> String {
    let s = t.snapshot(0);
    s.cells
        .chunks(usize::from(s.cols))
        .map(|row| {
            row.iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn wait(t: &Terminal, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !readable(t).contains(needle) {
        assert!(
            Instant::now() < deadline,
            "original shell did not emit {needle}"
        );
        thread::sleep(Duration::from_millis(15));
    }
}

#[test]
fn checkpoint_replay_is_passive_but_new_live_queries_produce_only_new_replies() {
    let mut options = Options::bash(std::env::temp_dir(), bash(), "fixture");
    options.args = vec![
        "--noprofile".into(),
        "--norc".into(),
        "-c".into(),
        "printf 'ORIGINAL_READY'; read -r result; printf 'RETURN=%s' \"$result\"; sleep 1".into(),
    ];
    let original = Terminal::spawn(options, || {}).unwrap();
    wait(&original, "ORIGINAL_READY");

    let gate = original.quiesce(Duration::from_secs(2)).unwrap();
    let checkpoint = gate.checkpoint().unwrap();
    let original_snapshot = gate.snapshot();
    let wire = checkpoint.to_wire().unwrap();
    let mut receiver = aw_terminal::Checkpoint::from_wire(&wire)
        .unwrap()
        .replay()
        .unwrap();

    assert_eq!(checkpoint.terminal_id(), original.id);
    assert!(receiver.screen().contents().contains("ORIGINAL_READY"));
    assert_eq!(
        receiver.screen().size(),
        (original_snapshot.rows, original_snapshot.cols),
    );

    // A passive read-only recipient has no permission to answer shell
    // protocol requests or manipulate the old PTY.
    receiver.process(b"\x1b[6n");
    let after_passive = receiver.screen().contents();
    assert!(after_passive.contains("ORIGINAL_READY"));

    // A distinct active, owner-authorized consumer *will* need these
    // replies for future DSR/DA interactions. The checkpoint itself never
    // generated them, and split CSI bytes produce one bounded reply only.
    assert!(receiver.process_live(b"\x1b[").is_empty());
    let replies = receiver.process_live(b"6n");
    assert!(
        replies.starts_with(b"\x1b[") && replies.ends_with(b"R"),
        "live device-status query must be answered once"
    );
    assert!(replies.len() <= 32);
    assert!(receiver.process_live(b"safe").is_empty());
    assert!(receiver
        .process_live(b"\x1b]52;c;NEVER_PASS_TO_HOST\x07")
        .is_empty());
    assert!(!receiver.screen().contents().contains("NEVER_PASS_TO_HOST"));

    // No ownership transfer occurred: dropping a passive checkpoint and
    // the quiescence guard keeps the original shell under its original ID.
    drop(gate);
    original.input(b"accepted\r".to_vec()).unwrap();
    wait(&original, "RETURN=accepted");
    original.terminate();
}

#[test]
fn staged_kernel_escrow_retains_original_shell_on_receiver_refusal() {
    use std::{os::fd::AsFd, os::unix::net::UnixStream};

    let mut options = Options::bash(std::env::temp_dir(), bash(), "fixture");
    options.args = vec![
        "--noprofile".into(),
        "--norc".into(),
        "-c".into(),
        "printf 'STAGE_READY'; read -r word; printf 'SOURCE=%s' \"$word\"; sleep 1".into(),
    ];
    let original = Terminal::spawn(options, || {}).unwrap();
    wait(&original, "STAGE_READY");
    let identity = original.id.clone();
    let mut gate = original.quiesce(Duration::from_secs(2)).unwrap();
    let mut staged = gate.stage().unwrap();
    assert_eq!(staged.checkpoint().terminal_id(), identity);
    assert!(!staged.child_exited().unwrap());
    let (sender, receiver) = UnixStream::pair().unwrap();
    staged.send_once(&sender, &[93; 32]).unwrap();
    let (master, child) = aw_platform::fd_transfer::receive_pair(&receiver, &[93; 32]).unwrap();
    assert!(!aw_platform::pid_identity::exited(child.as_fd()).unwrap());
    assert!(
        staged.checkpoint().replay().is_ok(),
        "source retained its checkpoint"
    );
    assert!(
        staged.send_once(&sender, &[93; 32]).is_err(),
        "no implicit replay of file descriptors"
    );
    assert!(original.input(b"ILLEGAL\r".to_vec()).is_err());

    // The destination refuses to activate: its duplicated FDs close without
    // ever reading or writing. The original keeps exactly its old master,
    // child pid and VT100 state, and can resume safely.
    drop(master);
    drop(child);
    drop(staged);
    assert!(gate.stage().is_err(), "one escrow per reader generation");
    drop(gate);
    original.input(b"accepted\r".to_vec()).unwrap();
    wait(&original, "SOURCE=accepted");
    assert_eq!(original.id, identity);
    original.terminate();
}

#[test]
fn stage_receiver_child_only_inspects_no_input_or_adoption() {
    if std::env::var_os("AW_TEST_STAGE_CHILD").is_none() {
        return;
    }
    use std::{
        io::{Read, Write},
        os::{fd::AsFd, unix::net::UnixStream},
    };
    let socket = std::env::var_os("AW_STAGE_SOCKET").unwrap();
    let mut channel = UnixStream::connect(PathBuf::from(socket)).unwrap();
    channel
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let (master, process_identity) =
        aw_platform::fd_transfer::receive_pair(&channel, &[42_u8; 32]).unwrap();
    assert!(!aw_platform::pid_identity::exited(process_identity.as_fd()).unwrap());
    let mut size = [0_u8; 4];
    channel.read_exact(&mut size).unwrap();
    let len = u32::from_be_bytes(size) as usize;
    assert!(len > 0 && len <= aw_terminal::MAX_CHECKPOINT_WIRE_BYTES);
    let mut checkpoint_wire = vec![0_u8; len];
    channel.read_exact(&mut checkpoint_wire).unwrap();
    let checkpoint = aw_terminal::Checkpoint::from_wire(&checkpoint_wire).unwrap();
    let replay = checkpoint.replay().unwrap();
    assert_eq!(checkpoint.original_owner(), "fixture");
    assert!(replay.screen().contents().contains("STAGED_OUT_OF_PROCESS"));
    // Broker is only Staged; never read master or send shell input before
    // the independent original-owner commit which does not exist yet.
    channel
        .write_all(b"CHECKPOINT_AND_KERNEL_HANDLES_STAGED\n")
        .unwrap();
    drop(process_identity);
    drop(master);
}

#[test]
fn independent_process_receives_exact_staged_handles_but_cannot_commit_ownership() {
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::{fs::DirBuilderExt, net::UnixListener},
        process::{Command, Stdio},
    };

    let mut options = Options::bash(std::env::temp_dir(), bash(), "fixture");
    options.args = vec![
        "--noprofile".into(),
        "--norc".into(),
        "-c".into(),
        "printf 'STAGED_OUT_OF_PROCESS'; read -r word; printf 'OWNER_RETAINED=%s' \"$word\"; sleep 1".into(),
    ];
    let original = Terminal::spawn(options, || {}).unwrap();
    wait(&original, "STAGED_OUT_OF_PROCESS");

    let root = std::env::temp_dir().join(format!(
        "aw-terminal-private-stage-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .unwrap();
    let path = root.join("relay.sock");
    let listener = UnixListener::bind(&path).unwrap();

    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "stage_receiver_child_only_inspects_no_input_or_adoption",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("AW_TEST_STAGE_CHILD", "1")
        .env("AW_STAGE_SOCKET", &path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut guard = original.quiesce(Duration::from_secs(2)).unwrap();
    let mut staging = guard.stage().unwrap();
    let (mut channel, _) = listener.accept().unwrap();
    channel
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    staging.send_once(&channel, &[42_u8; 32]).unwrap();
    let wire = staging.checkpoint().to_wire().unwrap();
    channel
        .write_all(&(wire.len() as u32).to_be_bytes())
        .unwrap();
    channel.write_all(&wire).unwrap();
    let mut reply = String::new();
    BufReader::new(&channel).read_line(&mut reply).unwrap();
    assert_eq!(reply, "CHECKPOINT_AND_KERNEL_HANDLES_STAGED\n");
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "independent staged receiver failed: {} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    assert!(
        original.input(b"NOT_AUTHORIZED\r".to_vec()).is_err(),
        "original reader paused until rollback; no two live consumers",
    );

    drop(channel);
    drop(staging);
    drop(guard);
    original.input(b"accepted\r".to_vec()).unwrap();
    wait(&original, "OWNER_RETAINED=accepted");
    original.terminate();
    drop(listener);
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&root).unwrap();
}

#[test]
fn terminating_paused_owner_revokes_a_prepared_kernel_escrow_before_send() {
    use std::os::unix::net::UnixStream;

    let mut options = Options::bash(std::env::temp_dir(), bash(), "fixture");
    options.args = vec![
        "--noprofile".into(),
        "--norc".into(),
        "-c".into(),
        "printf 'TERMINATION_READY'; read -r word; printf 'DISALLOWED=%s' \"$word\"".into(),
    ];
    let original = Terminal::spawn(options, || {}).unwrap();
    wait(&original, "TERMINATION_READY");
    let mut paused = original.quiesce(Duration::from_secs(2)).unwrap();
    let mut staged = paused.stage().unwrap();
    assert!(!staged.child_exited().unwrap());
    original.terminate();
    let (socket, _receiver) = UnixStream::pair().unwrap();
    assert!(
        staged.send_once(&socket, &[49; 32]).is_err(),
        "original owner stop must revoke still-untransmitted kernel escrow"
    );
    drop(staged);
    drop(paused);
}
