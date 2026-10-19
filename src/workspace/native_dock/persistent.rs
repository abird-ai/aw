//! Dedicated PTY owner. Explicit new-shell launch and Linux original-PTY
//! adoption share the retained Pool/attach loop; only start launches a shell.
use super::*;
use std::process::{Command, Stdio};

const BROKER_IDLE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

fn validate_shell(root: &Path, shell: &Path) -> Result<(PathBuf, PathBuf)> {
    if !root.is_absolute() || !shell.is_absolute() {
        return Err(Error::Denied("persistent PTY requires absolute root and Bash paths"));
    }
    let root = root.canonicalize().map_err(Error::Io)?;
    let shell = shell.canonicalize().map_err(Error::Io)?;
    if !root.is_dir() || !shell.is_file()
        || shell.file_name() != Some(std::ffi::OsStr::new("bash")) {
        return Err(Error::Denied("persistent PTY requires an existing directory and absolute Bash executable"));
    }
    Ok((root, shell))
}

pub(super) fn write_invitation(path: &Path, invitation: &Invite) -> Result<()> {
    let next = path.with_file_name("invite-next");
    let data = serde_json::to_vec(invitation)
        .map_err(|error| fail(error.to_string()))?;
    let mut file = OpenOptions::new()
        .write(true).create_new(true).mode(0o600)
        .open(&next).map_err(Error::Io)?;
    file.write_all(&data).map_err(Error::Io)?;
    file.sync_all().map_err(Error::Io)?;
    drop(file);
    fs::rename(&next, path).map_err(Error::Io)
}

/// Explicitly create one independent native process and shell. This does
/// not adopt an already running Workspace PTY and never opens Agent State.
pub fn start_persistent(root: &Path, shell: &Path) -> Result<u8> {
    let (root, shell) = validate_shell(root, shell)?;
    let (directory, cleanup) = private_directory()?;
    let binary = std::env::current_exe().map_err(Error::Io)?;
    let mut command = Command::new(binary);
    command
        .arg("workspace").arg("persistent").arg("serve")
        .arg(&root)
        .arg(&shell)
        .arg(&directory)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .env_clear();
    // No credential-bearing provider, SSH, loader or tool environment enters
    // the terminal-only owner; child command separately filters its own env.
    for key in ["HOME", "PATH", "LANG", "LC_CTYPE"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    // Preserve only the resolved socket parent, not an arbitrary inherited
    // environment. The child must validate the *same* private endpoint when
    // the user runs with a non-default TMPDIR.
    let temp_root = directory.parent()
        .ok_or(Error::Denied("missing native broker temporary directory"))?;
    command.env("TMPDIR", temp_root);
    let mut child = aw_platform::spawn_detached_session(&mut command).map_err(Error::Io)?;
    // Parent no longer owns the directory: dedicated owner removes it when
    // the original shell exits or is explicitly stopped.
    cleanup.transfer();
    let invite_path = directory.join("invite");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if invite_path.exists() && read_invite(&invite_path).is_ok() {
            println!("{}", attach_hint(&invite_path));
            println!("aw workspace persistent stop {}", quoted_path(&invite_path));
            return Ok(0);
        }
        if let Some(status) = child.try_wait().map_err(Error::Io)? {
            Cleanup(directory);
            return Err(fail(format!("persistent PTY owner exited during startup: {status}")));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            Cleanup(directory);
            return Err(fail("persistent PTY owner did not start within ten seconds"));
        }
        std::thread::sleep(Duration::from_millis(35));
    }
}

/// Never make a hidden "serve" CLI into a generic directory cleaner.
/// Only a fresh, AW-generated private endpoint may become a broker owner.
pub(super) fn validate_new_broker_directory(directory: &Path) -> Result<()> {
    let basename = directory.file_name().and_then(|name| name.to_str());
    if !directory.is_absolute() || directory.parent() != Some(std::env::temp_dir().as_path())
        || !basename.is_some_and(|name|
            name.strip_prefix("aw-pty-").is_some_and(|nonce|
                nonce.len() == 16 && nonce.bytes().all(|byte| byte.is_ascii_hexdigit())))
    {
        return Err(Error::Denied("broker needs a fresh AW-generated private endpoint"));
    }
    aw_platform::validate_private_dir(directory).map_err(Error::Io)?;
    for filename in ["socket", "invite", "invite-next"] {
        if fs::symlink_metadata(directory.join(filename)).is_ok() {
            return Err(Error::Denied("broker endpoint already occupied"));
        }
    }
    Ok(())
}

/// Dedicated owner process with *one* retained PTY and an explicitly bounded
/// lifetime. It never opens a provider, Agent, Session or Workspace Store.
pub fn serve_persistent(root: &Path, shell: &Path, directory: &Path) -> Result<u8> {
    let (root, shell) = validate_shell(root, shell)?;
    validate_new_broker_directory(directory)?;
    let _cleanup = Cleanup(directory.to_path_buf());
    let listener = UnixListener::bind(directory.join("socket")).map_err(Error::Io)?;
    listener.set_nonblocking(true).map_err(Error::Io)?;
    let pool = terminals::Pool::new();
    // One retained shell per isolated broker. Its ID embeds this process
    // lifetime, not a restorable layout or a borrowed Agent identity.
    let id = pool.launch(root, shell, "trusted-local-persistent-pty".into())?;
    serve_pool(&pool, &id, directory, listener)?;
    Ok(0)
}

pub(super) fn serve_pool(pool: &terminals::Pool, id: &str, directory: &Path, listener: UnixListener) -> Result<()> {
    let mut idle_since = Instant::now();
    let invite_path = directory.join("invite");
    let mut current: Option<Invite> = None;
    while idle_since.elapsed() < BROKER_IDLE_TTL && !pool.is_shutting_down() {
        let Some(terminal) = pool.get(id) else { break };
        if terminal.status() != aw_terminal::Status::Running {
            break;
        }
        match pool.dock_mode(id) {
            Some(super::super::terminal_control::Mode::Local)
            | Some(super::super::terminal_control::Mode::Offered) => {}
            Some(super::super::terminal_control::Mode::Settling)
            | Some(super::super::terminal_control::Mode::Docked) => {
                std::thread::sleep(Duration::from_millis(30));
                continue;
            }
            #[cfg(target_os = "linux")]
            Some(super::super::terminal_control::Mode::Migrating) => break,
            None => break,
        }
        if current.as_ref().is_none_or(|invite|
            !pool.has_invitation(id, &invite.ticket)) {
            let issued = pool.prepare_dock(id)?;
            let invite = Invite {
                version: PROTOCOL,
                socket: directory.join("socket"),
                terminal: issued.terminal_id,
                ticket: issued.ticket,
            };
            if let Err(error) = write_invitation(&invite_path, &invite) {
                let _ = pool.cancel_dock(id);
                return Err(error);
            }
            current = Some(invite);
        }
        let Some(invite) = current.as_ref() else { continue };
        match listener.accept() {
            Ok((connection, _)) => {
                // Bad credentials cannot prolong an idle shell. A completed
                // admitted session resets its idle lifetime; active sessions
                // are never cut off just because the broker is old.
                if matches!(serve(connection, pool, invite, true), Ok(true)) {
                    idle_since = Instant::now();
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock =>
                std::thread::sleep(Duration::from_millis(35)),
            Err(error) => return Err(Error::Io(error)),
        }
    }
    pool.shutdown();
    if pool.all().iter().any(|terminal| !terminal.status().exited()) {
        return Err(Error::Denied("broker shutdown uncertain; inspect original process effects"));
    }
    Ok(())
}

/// Explicit PTY teardown. A stopped or missing owner is not reported as a
/// successful termination; never signal an unverified PID from a file.
pub fn stop_persistent(path: &Path) -> Result<u8> {
    let invite = read_invite(path)?;
    let mut connection = UnixStream::connect(&invite.socket).map_err(Error::Io)?;
    connection.set_read_timeout(Some(Duration::from_secs(5))).map_err(Error::Io)?;
    connection.set_write_timeout(Some(Duration::from_secs(5))).map_err(Error::Io)?;
    let mut request_bytes = serde_json::to_vec(&Request::Stop { ticket: invite.ticket })
        .map_err(|error| fail(error.to_string()))?;
    request_bytes.push(b'\n');
    connection.write_all(&request_bytes).map_err(Error::Io)?;
    let mut reader = BufReader::new(connection);
    let Some(response) = request(&mut reader, 4096).map_err(Error::Io)? else {
        return Err(Error::Denied("native PTY stop response is unknown"));
    };
    match serde_json::from_slice::<Reply>(&response)
        .map_err(|_| Error::Denied("invalid native PTY stop response"))? {
        Reply::Stopped { terminal } if terminal == invite.terminal => Ok(0),
        Reply::Error { message } => Err(fail(message)),
        _ => Err(Error::Denied("persistent PTY owner did not confirm shutdown")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broker_serve_refuses_unrelated_or_occupied_private_paths() {
        let arbitrary = std::env::temp_dir().join("aw-unrelated-user-files");
        assert!(validate_new_broker_directory(&arbitrary).is_err());
        let (directory, cleanup) = private_directory().unwrap();
        assert!(validate_new_broker_directory(&directory).is_ok());
        let occupied = directory.join("invite");
        fs::write(&occupied, b"must not erase an existing owner's invitation").unwrap();
        assert!(validate_new_broker_directory(&directory).is_err());
        assert!(occupied.exists(), "refusal must not remove an existing file");
        fs::remove_file(occupied).unwrap();
        let occupied = directory.join("socket");
        fs::write(&occupied, b"not a socket").unwrap();
        assert!(validate_new_broker_directory(&directory).is_err());
        assert!(occupied.exists());
        fs::remove_file(occupied).unwrap();
        assert!(validate_new_broker_directory(&directory).is_ok());
        drop(cleanup);
    }

    #[test]
    fn owner_paths_are_explicit_and_bounded() {
        assert_eq!(BROKER_IDLE_TTL, Duration::from_secs(24 * 60 * 60));
        assert!(validate_shell(Path::new("."), Path::new("/bin/bash")).is_err());
        assert!(validate_shell(Path::new("/tmp"), Path::new("bash")).is_err());
        let (directory, cleanup) = private_directory().unwrap();
        let file = directory.join("invite");
        assert_eq!(std::ffi::OsStr::new("invite"), file.file_name().unwrap());
        let invite = Invite {
            version: PROTOCOL, socket: directory.join("socket"),
            terminal: "synthetic-id".into(), ticket: "a".repeat(64),
        };
        write_invitation(&file, &invite).unwrap();
        assert_eq!(read_invite(&file).unwrap().ticket, invite.ticket);
        let replacement = Invite { ticket: "b".repeat(64), ..invite };
        write_invitation(&file, &replacement).unwrap();
        assert_eq!(read_invite(&file).unwrap().ticket, replacement.ticket);
        drop(cleanup);
        assert!(!file.exists());
    }
}
