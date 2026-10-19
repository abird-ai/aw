//! Exact synthetic Linux PTYs; no production grants or provider effects.
#![cfg(target_os = "linux")]
use aw_terminal::{Adoption, Checkpoint, Options, Status, Terminal};
use std::{
    os::{fd::AsFd, unix::net::UnixStream},
    thread,
    time::{Duration, Instant},
};

fn until(mut test: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !test() {
        assert!(Instant::now() < end, "PTY transition timed out");
        thread::sleep(Duration::from_millis(5));
    }
}
fn contents(t: &Terminal) -> String {
    t.snapshot(0)
        .cells
        .iter()
        .map(|c| c.text.as_str())
        .collect()
}
fn terminal(script: &str) -> Terminal {
    let shell: std::path::PathBuf = std::env::var_os("AW_BASH")
        .map(Into::into)
        .unwrap_or_else(|| "/bin/bash".into());
    let mut options = Options::bash(std::env::temp_dir(), shell, "synthetic-adoption");
    options.args = vec![
        "--noprofile".into(),
        "--norc".into(),
        "-c".into(),
        script.into(),
    ];
    let terminal = Terminal::spawn(options, || {}).unwrap();
    until(|| contents(&terminal).contains("READY"));
    terminal
}
#[test]
fn inert_drop_and_wrong_identity_preserve_original_custody() {
    for wrong in [0, 1, 2] {
        let original =
            terminal("printf READY; read -r word; printf 'ACCEPTED:%s' \"$word\"; read -r done");
        let mut gate = original.quiesce(Duration::from_secs(2)).unwrap();
        let mut staged = gate.stage().unwrap();
        let checkpoint = Checkpoint::from_wire(&staged.checkpoint().to_wire().unwrap()).unwrap();
        let mut identity = staged.identity().unwrap();
        let (source, recipient) = UnixStream::pair().unwrap();
        staged.send_once(&source, &[19; 32]).unwrap();
        let (master, pidfd) =
            aw_platform::fd_transfer::receive_pair(&recipient, &[19; 32]).unwrap();
        let observed =
            aw_platform::pty_poll::duplicate(std::os::fd::AsRawFd::as_raw_fd(&pidfd)).unwrap();
        if wrong == 1 {
            identity.child_pid += 1;
        }
        if wrong == 2 {
            identity.pty_number += 1;
        }
        let inert = Adoption::prepare(checkpoint, master, pidfd, identity);
        assert_eq!(inert.is_err(), wrong != 0);
        drop(inert);
        assert!(!aw_platform::pid_identity::exited(observed.as_fd()).unwrap());
        drop(staged);
        drop(gate);
        original.input(b"once\n".to_vec()).unwrap();
        until(|| contents(&original).contains("ACCEPTED:once"));
        original.terminate();
    }
}
#[test]
fn adopted_reactor_retains_exact_parser_and_answers_new_queries_once() {
    let original = terminal("printf '\\033[?1049h\\033[?2004h\\033[31;1mREADY_界'; read -r word; printf 'ACCEPTED:%s PID:%s\\033[6n' \"$word\" \"$$\"; IFS= read -r -d R reply; printf 'ONE_REPLY'; IFS= read -r discard; printf WAITING; IFS= read -r done");
    let mut gate = original.quiesce(Duration::from_secs(2)).unwrap();
    let mut staged = gate.stage().unwrap();
    let identity = staged.identity().unwrap();
    let checkpoint = Checkpoint::from_wire(&staged.checkpoint().to_wire().unwrap()).unwrap();
    let (source, recipient) = UnixStream::pair().unwrap();
    staged.send_once(&source, &[37; 32]).unwrap();
    let (master, pidfd) = aw_platform::fd_transfer::receive_pair(&recipient, &[37; 32]).unwrap();
    let escrow = Adoption::prepare(checkpoint, master, pidfd, identity).unwrap();
    staged
        .release_source()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap();
    drop(gate);
    let retired_version = original.snapshot(0).version;
    let adopted = escrow.activate(|| {}).unwrap();
    assert_eq!(original.id, adopted.id);
    assert_eq!(original.root, adopted.root);
    assert!(adopted.snapshot(0).alternate_screen);
    assert!(adopted.snapshot(0).bracketed_paste);
    assert!(contents(&adopted).contains("READY_界"));
    adopted.input(b"once\n".to_vec()).unwrap();
    until(|| contents(&adopted).contains(&format!("ACCEPTED:once PID:{}", identity.child_pid)));
    // Canonical input buffers the terminal's response until a newline.
    adopted.input(b"\n".to_vec()).unwrap();
    until(|| contents(&adopted).contains("WAITING"));
    assert_eq!(original.snapshot(0).version, retired_version);
    original.terminate();
    drop(original);
    assert_eq!(adopted.status(), Status::Running);
    adopted.resize(15, 60).unwrap();
    until(|| adopted.snapshot(0).rows == 15);
    adopted.terminate();
    until(|| !matches!(adopted.status(), Status::Running | Status::Stopping));
    assert!(adopted.status().exited());
    assert_ne!(adopted.status(), Status::Exited(0));
}
#[test]
fn release_publication_survives_caller_ack_loss_and_publisher_panic() {
    for panic in [false, true] {
        let original = terminal("printf READY; read -r done");
        let mut gate = original.quiesce(Duration::from_secs(2)).unwrap();
        let mut staged = gate.stage().unwrap();
        let (source, recipient) = UnixStream::pair().unwrap();
        staged.send_once(&source, &[17; 32]).unwrap();
        let (master, pidfd) =
            aw_platform::fd_transfer::receive_pair(&recipient, &[17; 32]).unwrap();
        let published = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let result = published.clone();
        let pending = staged
            .release_source_and_publish(move |_| {
                result.store(true, std::sync::atomic::Ordering::Release);
                assert!(!panic, "synthetic release publication fault");
                Ok(())
            })
            .unwrap();
        drop(pending);
        drop(gate);
        until(|| published.load(std::sync::atomic::Ordering::Acquire));
        assert!(!aw_platform::pid_identity::exited(pidfd.as_fd()).unwrap());
        aw_platform::pid_identity::signal(
            pidfd.as_fd(),
            aw_platform::pid_identity::ProcessSignal::Kill,
        )
        .unwrap();
        until(|| {
            matches!(
                aw_platform::pid_identity::inspect_exit(pidfd.as_fd()).unwrap(),
                aw_platform::pid_identity::ExitObservation::Exited { status: None }
            )
        });
        drop(master);
    }
}

#[test]
fn exact_shell_stop_retires_reader_even_if_a_descendant_holds_the_slave() {
    let root = std::env::temp_dir().join(format!(
        "aw-adopted-slave-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let shell: std::path::PathBuf = std::env::var_os("AW_BASH")
        .map(Into::into)
        .unwrap_or_else(|| "/bin/bash".into());
    let mut options = Options::bash(&root, shell, "synthetic-slave-holder");
    options.args = vec!["--noprofile".into(), "--norc".into(), "-c".into(), "(trap '' HUP TERM; while :; do IFS= read -r -t 60 ignored </dev/tty; done) & printf '%s' \"$!\" > holder.pid; printf READY; IFS= read -r done".into()];
    let original = Terminal::spawn(options, || {}).unwrap();
    until(|| contents(&original).contains("READY"));
    let holder: u32 = std::fs::read_to_string(root.join("holder.pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        std::fs::read_link(format!("/proc/{holder}/cwd")).unwrap(),
        root
    );
    let holder = aw_platform::pid_identity::open(holder).unwrap();
    struct Cleanup(std::os::fd::OwnedFd, std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = aw_platform::pid_identity::signal(
                self.0.as_fd(),
                aw_platform::pid_identity::ProcessSignal::Kill,
            );
            let _ = std::fs::remove_dir_all(&self.1);
        }
    }
    let cleanup = Cleanup(holder, root);
    let mut gate = original.quiesce(Duration::from_secs(2)).unwrap();
    let mut stage = gate.stage().unwrap();
    let identity = stage.identity().unwrap();
    let checkpoint = Checkpoint::from_wire(&stage.checkpoint().to_wire().unwrap()).unwrap();
    let (sender, receiver) = UnixStream::pair().unwrap();
    stage.send_once(&sender, &[23; 32]).unwrap();
    let (master, pidfd) = aw_platform::fd_transfer::receive_pair(&receiver, &[23; 32]).unwrap();
    let escrow = Adoption::prepare(checkpoint, master, pidfd, identity).unwrap();
    stage
        .release_source()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap();
    drop(gate);
    drop(original);
    let marker = std::sync::Arc::new(());
    let released = std::sync::Arc::downgrade(&marker);
    let adopted = escrow
        .activate(move || {
            let _ = &marker;
        })
        .unwrap();
    adopted.terminate();
    drop(adopted);
    until(|| released.upgrade().is_none());
    assert!(
        !aw_platform::pid_identity::exited(cleanup.0.as_fd()).unwrap(),
        "no guessed process-group kill"
    );
    drop(cleanup);
}

#[test]
fn streaming_cutover_drains_once_and_preserves_main_and_scrollback() {
    let root = std::env::temp_dir().join(format!(
        "aw-stream-cutover-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let shell: std::path::PathBuf = std::env::var_os("AW_BASH")
        .map(Into::into)
        .unwrap_or_else(|| "/bin/bash".into());
    let mut options = Options::bash(&root, shell, "synthetic-stream-cutover");
    options.args = vec!["--noprofile".into(), "--norc".into(), "-c".into(), "printf READY; while [[ ! -f stream ]]; do sleep 0.01; done; for ((i=0;i<3000;i++)); do printf 'STREAM_%04d\\n' \"$i\"; done; printf '\\033[?1049h\\033[?2004h\\033[1;31mALT_READY_界'; IFS= read -r back; printf '\\033[?1049lRESTORED'; IFS= read -r done; exit 7".into()];
    let original = Terminal::spawn(options, || {}).unwrap();
    until(|| contents(&original).contains("READY"));
    let mut gate = original.quiesce(Duration::from_secs(2)).unwrap();
    let mut stage = gate.stage().unwrap();
    let identity = stage.identity().unwrap();
    let checkpoint = Checkpoint::from_wire(&stage.checkpoint().to_wire().unwrap()).unwrap();
    let (sender, receiver) = UnixStream::pair().unwrap();
    stage.send_once(&sender, &[31; 32]).unwrap();
    let (master, pidfd) = aw_platform::fd_transfer::receive_pair(&receiver, &[31; 32]).unwrap();
    let escrow = Adoption::prepare(checkpoint, master, pidfd, identity).unwrap();
    std::fs::write(root.join("stream"), b"start").unwrap();
    // Force new output while the sole original reader is paused. The bounded
    // kernel PTY buffer backpressures the producer until receiver activation.
    thread::sleep(Duration::from_millis(60));
    stage
        .release_source()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap();
    drop(gate);
    let retired_version = original.snapshot(0).version;
    let adopted = escrow.activate(|| {}).unwrap();
    until(|| contents(&adopted).contains("ALT_READY_界"));
    let held = adopted.quiesce(Duration::from_secs(2)).unwrap();
    let wire = held.checkpoint().unwrap().to_wire().unwrap();
    let data: serde_json::Value = serde_json::from_slice(&wire).unwrap();
    let mut bytes = Vec::new();
    for event in data["events"].as_array().unwrap() {
        if let Some(chunk) = event.get("Bytes") {
            bytes.extend(
                chunk
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|b| b.as_u64().unwrap() as u8),
            );
        }
    }
    let text = String::from_utf8(bytes).unwrap();
    let lines = text
        .lines()
        .filter_map(|line| line.find("STREAM_").map(|at| &line[at..]))
        .collect::<Vec<_>>();
    assert_eq!(
        lines.len(),
        3000,
        "no lost or duplicated output across source retirement"
    );
    for (i, line) in lines.iter().enumerate() {
        assert_eq!(*line, format!("STREAM_{i:04}"));
    }
    drop(held);
    adopted.input(b"back\n".to_vec()).unwrap();
    until(|| !adopted.snapshot(0).alternate_screen && contents(&adopted).contains("RESTORED"));
    assert!(
        contents(&adopted).contains("STREAM_2999"),
        "inactive main buffer lost"
    );
    let history = adopted
        .snapshot(100)
        .cells
        .iter()
        .map(|c| c.text.as_str())
        .collect::<String>();
    assert!(
        history.contains("STREAM_28"),
        "bounded main scrollback lost"
    );
    assert_eq!(
        original.snapshot(0).version,
        retired_version,
        "old reader consumed new stream"
    );
    adopted.input(b"finish\n".to_vec()).unwrap();
    until(|| adopted.status().exited());
}
