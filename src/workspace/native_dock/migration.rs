//! Linux original-shell custody transaction. Private local terminal authority;
//! no Agent/Store/model access. Missing proof is UNKNOWN, never an activation.
use super::*;
use aw_terminal::{Adoption, Checkpoint, EscrowIdentity};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use std::process::{Command, Stdio};

const VERSION: u8 = 1;
const ADMISSION: Duration = Duration::from_secs(10);
const RELEASE_WAIT: Duration = Duration::from_secs(60);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Transaction {
    version: u8,
    nonce: [u8; 32],
    source_pid: u32,
    broker_pid: u32,
    terminal: String,
    root: PathBuf,
    owner: String,
    site: String,
    epoch: u64,
    identity: EscrowIdentity,
    checkpoint: [u8; 32],
    release_key: [u8; 32],
}
#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u8,
    transaction: [u8; 32],
    terminal: String,
    epoch: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Release {
    record: Record,
    signature: Vec<u8>,
}
fn release_message(record: &Record) -> Result<Vec<u8>> {
    let mut message = b"aw-pty-source-released-v1\0".to_vec();
    message.extend(serde_json::to_vec(record).map_err(|e| fail(e.to_string()))?);
    Ok(message)
}
fn read_release(path: &Path, transaction: &Transaction) -> Option<Release> {
    let release: Release = read_json(path).ok()?;
    if release.record != transaction.record().ok()? {
        return None;
    }
    let signature: [u8; 64] = release.signature.as_slice().try_into().ok()?;
    VerifyingKey::from_bytes(&transaction.release_key)
        .ok()?
        .verify_strict(
            &release_message(&release.record).ok()?,
            &Signature::from_bytes(&signature),
        )
        .ok()?;
    Some(release)
}
impl Transaction {
    fn record(&self) -> Result<Record> {
        Ok(Record {
            version: VERSION,
            transaction: Sha256::digest(serde_json::to_vec(self).map_err(|e| fail(e.to_string()))?)
                .into(),
            terminal: self.terminal.clone(),
            epoch: self.epoch,
        })
    }
    fn matches(&self, checkpoint: &Checkpoint, wire: &[u8]) -> bool {
        self.version == VERSION
            && self.epoch == checkpoint.epoch()
            && self.terminal == checkpoint.terminal_id()
            && self.root == *checkpoint.original_root()
            && self.owner == checkpoint.original_owner()
            && self.site == checkpoint.original_site()
            && self.checkpoint == <[u8; 32]>::from(Sha256::digest(wire))
    }
}
fn terminal_error(error: aw_terminal::Error) -> Error {
    fail(error.to_string())
}

fn create_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.sync_all()
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    // NOFOLLOW is supplied by the platform's private read helper.
    let mut file = aw_platform::open_private_existing(path, false).map_err(Error::Io)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(16_385)
        .read_to_end(&mut bytes)
        .map_err(Error::Io)?;
    if bytes.len() > 16_384 {
        return Err(Error::Denied("oversized migration record"));
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::Denied("invalid migration record"))
}
fn read_exact_until(
    socket: &mut UnixStream,
    mut bytes: &mut [u8],
    deadline: Instant,
) -> io::Result<()> {
    while !bytes.is_empty() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "migration frame deadline",
            ));
        }
        socket.set_read_timeout(Some(remaining))?;
        match socket.read(bytes) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "migration frame incomplete",
                ))
            }
            Ok(count) => bytes = &mut bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
fn read_frame(socket: &mut UnixStream) -> Result<Vec<u8>> {
    let deadline = Instant::now() + ADMISSION;
    let mut size = [0_u8; 8];
    read_exact_until(socket, &mut size, deadline).map_err(Error::Io)?;
    let size = u64::from_be_bytes(size);
    if size > aw_terminal::MAX_CHECKPOINT_WIRE_BYTES as u64 {
        return Err(Error::Denied("oversized migration checkpoint"));
    }
    let mut bytes = vec![0; size as usize];
    read_exact_until(socket, &mut bytes, deadline).map_err(Error::Io)?;
    Ok(bytes)
}
fn send_frame(socket: &mut UnixStream, bytes: &[u8]) -> Result<()> {
    if bytes.len() > aw_terminal::MAX_CHECKPOINT_WIRE_BYTES {
        return Err(Error::Denied("oversized migration frame"));
    }
    let deadline = Instant::now() + ADMISSION;
    for mut part in [&(bytes.len() as u64).to_be_bytes()[..], bytes] {
        while !part.is_empty() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(fail(
                    "migration write deadline; source outcome must be inspected",
                ));
            }
            socket
                .set_write_timeout(Some(remaining))
                .map_err(Error::Io)?;
            match socket.write(part) {
                Ok(0) => return Err(fail("migration write closed")),
                Ok(count) => part = &part[count..],
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(Error::Io(error)),
            }
        }
    }
    Ok(())
}
fn record_matches(path: &Path, expected: &Record) -> bool {
    matches!(read_json::<Record>(path), Ok(actual) if actual == *expected)
}
fn timeout(message: &str, directory: &Path) -> Error {
    fail(format!(
        "{message}; inspect: aw workspace persistent inspect {} — never replay input",
        quoted_path(directory)
    ))
}

struct BrokerChild(Option<std::process::Child>);
impl BrokerChild {
    fn id(&self) -> u32 {
        self.0.as_ref().expect("owned broker child").id()
    }
    fn try_wait(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
        self.0.as_mut().expect("owned broker child").try_wait()
    }
}
impl Drop for BrokerChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            // Parent wait only; never kill an ambiguously released custodian.
            let _ = std::thread::Builder::new()
                .name("aw-broker-wait".into())
                .spawn(move || {
                    let _ = child.wait();
                });
        }
    }
}

/// Explicit trusted original host operation; requires an exclusive Pool fence.
/// The public palette is wired only after real-process acceptance tests pass.
pub fn promote(pool: &terminals::Pool, id: &str) -> Result<PathBuf> {
    let _control = pool.begin_migration(id)?;
    let terminal = pool.get(id).ok_or(Error::Denied("unknown original PTY"))?;
    let mut gate = terminal
        .quiesce(Duration::from_secs(3))
        .map_err(terminal_error)?;
    let mut escrow = gate.stage().map_err(terminal_error)?;
    let wire = escrow.checkpoint().to_wire().map_err(terminal_error)?;
    let identity = escrow.identity().map_err(terminal_error)?;
    let (directory, cleanup) = private_directory()?;
    let listener = UnixListener::bind(directory.join("control")).map_err(Error::Io)?;
    listener.set_nonblocking(true).map_err(Error::Io)?;
    let mut nonce = [0; 32];
    getrandom::fill(&mut nonce).map_err(|_| fail("secure migration identity unavailable"))?;
    let mut seed = [0; 32];
    getrandom::fill(&mut seed).map_err(|_| fail("source release signing identity unavailable"))?;
    let signing = SigningKey::from_bytes(&seed);
    use zeroize::Zeroize;
    seed.zeroize();
    let binary = std::env::current_exe().map_err(Error::Io)?;
    let mut command = Command::new(binary);
    command
        .args(["workspace", "persistent", "adopt"])
        .arg(&directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_clear();
    for key in ["HOME", "PATH", "LANG", "LC_CTYPE"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command.env(
        "TMPDIR",
        directory
            .parent()
            .ok_or(Error::Denied("missing endpoint parent"))?,
    );
    let mut broker = BrokerChild(Some(
        aw_platform::spawn_detached_session(&mut command).map_err(Error::Io)?,
    ));
    let transaction = Transaction {
        version: VERSION,
        nonce,
        source_pid: std::process::id(),
        broker_pid: broker.id(),
        terminal: id.into(),
        root: terminal.root.clone(),
        owner: terminal.owner.clone(),
        site: terminal.site.clone(),
        epoch: escrow.checkpoint().epoch(),
        identity,
        checkpoint: Sha256::digest(&wire).into(),
        release_key: signing.verifying_key().to_bytes(),
    };
    let expected = transaction.record()?;
    // From here the independent broker owns its private endpoint and cleanup.
    // Pre-release loss cannot authorize any PTY read, write, resize or signal.
    create_json(&directory.join("transaction"), &transaction).map_err(Error::Io)?;
    cleanup.transfer();
    let deadline = Instant::now() + ADMISSION;
    let mut socket = loop {
        match listener.accept() {
            Ok((socket, _)) => {
                if aw_platform::fd_transfer::peer_pid(&socket).map_err(Error::Io)? != broker.id()
                    || broker.try_wait().map_err(Error::Io)?.is_some()
                {
                    return Err(Error::Denied("migration recipient lifetime mismatch"));
                }
                break socket;
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                if broker.try_wait().map_err(Error::Io)?.is_some() {
                    return Err(fail("inert broker exited; original retained"));
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            Err(_) => {
                return Err(timeout(
                    "recipient admission timed out; original retained",
                    &directory,
                ))
            }
        }
    };
    socket
        .set_read_timeout(Some(ADMISSION))
        .map_err(Error::Io)?;
    socket
        .set_write_timeout(Some(ADMISSION))
        .map_err(Error::Io)?;
    send_frame(&mut socket, &wire)?;
    escrow.send_once(&socket, &nonce).map_err(terminal_error)?;
    drop(wire);
    // No BufReader: it must not prefetch the SCM_RIGHTS payload or later data.
    let ready: Record = serde_json::from_slice(&read_frame(&mut socket)?)
        .map_err(|_| Error::Denied("invalid inert-recipient readiness"))?;
    if ready != expected {
        return Err(Error::Denied("recipient readiness transaction mismatch"));
    }
    if broker.try_wait().map_err(Error::Io)?.is_some() {
        return Err(fail("prepared recipient exited; original custody retained"));
    }
    let publish_directory = directory.clone();
    let publication = serde_json::to_vec(&expected).map_err(|e| fail(e.to_string()))?;
    let release = escrow
        .release_source_and_publish(move |receipt| {
            let expected: Record = serde_json::from_slice(&publication)
                .map_err(|_| aw_terminal::Error::Unavailable("invalid owner release record"))?;
            if receipt.terminal_id() != expected.terminal || receipt.epoch() != expected.epoch {
                return Err(aw_terminal::Error::Unavailable(
                    "owner release identity mismatch",
                ));
            }
            let next = publish_directory.join("released-next");
            let release = Release {
                signature: signing
                    .sign(
                        &release_message(&expected)
                            .map_err(|e| aw_terminal::Error::Backend(e.to_string()))?,
                    )
                    .to_bytes()
                    .to_vec(),
                record: expected,
            };
            create_json(&next, &release)?;
            fs::rename(next, publish_directory.join("released"))?;
            fs::File::open(&publish_directory)?.sync_all()?;
            Ok(())
        })
        .map_err(terminal_error)?;
    // An ACK error never resumes this epoch. The worker owns publication.
    let _receipt = release.wait(Duration::from_secs(5));
    drop(gate);
    if terminal.source_release() == aw_terminal::SourceRelease::Retained {
        return Err(fail(
            "source worker rejected release; original custody retained, no activation",
        ));
    }
    let deadline = Instant::now() + ADMISSION;
    loop {
        let invite = directory.join("invite");
        if read_release(&directory.join("active"), &transaction).is_some()
            && read_invite(&invite).is_ok()
        {
            if broker.try_wait().map_err(Error::Io)?.is_some() {
                return Err(timeout(
                    "activated broker exited; destination custody UNKNOWN",
                    &directory,
                ));
            }
            return Ok(invite);
        }
        if Instant::now() >= deadline {
            if terminal.source_release() == aw_terminal::SourceRelease::Retained {
                return Err(fail(
                    "source worker rejected release; original custody retained, no activation",
                ));
            }
            return Err(timeout(
                "migration activation UNKNOWN; source stays fenced",
                &directory,
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Hidden independent broker admission. No executable/shell argument exists.
pub fn serve_adoption(directory: &Path) -> Result<u8> {
    persistent::validate_new_broker_directory(directory)?;
    for filename in ["prepared", "released", "released-next", "active", "failed"] {
        if fs::symlink_metadata(directory.join(filename)).is_ok() {
            return Err(Error::Denied("migration transaction already occupied"));
        }
    }
    let deadline = Instant::now() + ADMISSION;
    let transaction: Transaction = loop {
        if directory.join("transaction").exists() {
            break read_json(&directory.join("transaction"))?;
        }
        if Instant::now() >= deadline {
            return Err(fail("no original-owner transaction"));
        }
        std::thread::sleep(Duration::from_millis(15));
    };
    if transaction.version != VERSION || transaction.broker_pid != std::process::id() {
        return Err(Error::Denied("migration broker lifetime mismatch"));
    }
    VerifyingKey::from_bytes(&transaction.release_key)
        .map_err(|_| Error::Denied("invalid original release signing identity"))?;
    let mut socket = UnixStream::connect(directory.join("control")).map_err(Error::Io)?;
    if aw_platform::fd_transfer::peer_pid(&socket).map_err(Error::Io)? != transaction.source_pid {
        return Err(Error::Denied("migration source lifetime mismatch"));
    }
    let cleanup = Cleanup(directory.to_path_buf());
    socket
        .set_read_timeout(Some(ADMISSION))
        .map_err(Error::Io)?;
    socket
        .set_write_timeout(Some(ADMISSION))
        .map_err(Error::Io)?;
    let wire = read_frame(&mut socket)?;
    let checkpoint = Checkpoint::from_wire(&wire).map_err(terminal_error)?;
    if !transaction.matches(&checkpoint, &wire) {
        return Err(Error::Denied("migration checkpoint binding mismatch"));
    }
    drop(wire);
    let (master, pidfd) =
        aw_platform::fd_transfer::receive_pair(&socket, &transaction.nonce).map_err(Error::Io)?;
    let escrow = Adoption::prepare(checkpoint, master, pidfd, transaction.identity)
        .map_err(terminal_error)?;
    let expected = transaction.record()?;
    create_json(&directory.join("prepared"), &expected).map_err(Error::Io)?;
    cleanup.transfer();
    send_frame(
        &mut socket,
        &serde_json::to_vec(&expected).map_err(|e| fail(e.to_string()))?,
    )?;
    drop(socket);
    // Once readiness was emitted, release may already be admitted at source.
    // Preserve authoritative diagnosis on every later failure/UNKNOWN.
    let deadline = Instant::now() + RELEASE_WAIT;
    let release = loop {
        if let Some(release) = read_release(&directory.join("released"), &transaction) {
            break release;
        }
        if Instant::now() >= deadline {
            // Keep diagnosis, not an automatic shell replay/activation. Closing
            // inert escrow grants no signal authority; source may still own it.
            create_json(&directory.join("failed"), &expected).map_err(Error::Io)?;
            return Err(timeout("release unproved; inert escrow closed", directory));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let listener = UnixListener::bind(directory.join("socket")).map_err(Error::Io)?;
    listener.set_nonblocking(true).map_err(Error::Io)?;
    let pool = terminals::Pool::new();
    let id = pool.adopt(escrow)?;
    create_json(&directory.join("active"), &release).map_err(Error::Io)?;
    persistent::serve_pool(&pool, &id, directory, listener)?;
    drop(Cleanup(directory.to_path_buf()));
    Ok(0)
}

/// Read-only factual transaction inspection. Never activates or retries input.
pub fn inspect_migration(directory: &Path) -> Result<u8> {
    aw_platform::validate_private_dir(directory).map_err(Error::Io)?;
    let transaction: Transaction = read_json(&directory.join("transaction"))?;
    let expected = transaction.record()?;
    let state = if read_release(&directory.join("active"), &transaction).is_some() {
        if read_invite(&directory.join("invite")).is_ok() {
            "ACTIVE_RECORDED; endpoint present (connect to verify liveness)"
        } else {
            "ACTIVE_RECORDED; endpoint unavailable, custody UNKNOWN"
        }
    } else if read_release(&directory.join("released"), &transaction).is_some() {
        "SOURCE_RELEASED; destination activation UNKNOWN; source must not resume"
    } else if record_matches(&directory.join("prepared"), &expected) {
        "PREPARED; source release unproved; activation forbidden"
    } else {
        "INTENT; source release unproved; activation forbidden"
    };
    println!(
        "{} epoch={} shell_pid={} {}",
        transaction.terminal, transaction.epoch, transaction.identity.child_pid, state
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn transaction() -> Transaction {
        let signing = SigningKey::from_bytes(&[47; 32]);
        Transaction {
            version: VERSION,
            nonce: [19; 32],
            source_pid: 100,
            broker_pid: 101,
            terminal: "pty_fixture".into(),
            root: PathBuf::from("/tmp"),
            owner: "synthetic-actor".into(),
            site: "local".into(),
            epoch: 7,
            identity: EscrowIdentity {
                child_pid: 102,
                pty_number: 9,
            },
            checkpoint: [29; 32],
            release_key: signing.verifying_key().to_bytes(),
        }
    }
    #[test]
    fn signed_release_binds_every_source_and_recipient_coordinate() {
        let (directory, _cleanup) = private_directory().unwrap();
        let tx = transaction();
        let record = tx.record().unwrap();
        let release = Release {
            signature: SigningKey::from_bytes(&[47; 32])
                .sign(&release_message(&record).unwrap())
                .to_bytes()
                .to_vec(),
            record,
        };
        let path = directory.join("released");
        create_json(&path, &release).unwrap();
        assert!(read_release(&path, &tx).is_some());
        for field in [
            "version",
            "source_pid",
            "broker_pid",
            "terminal",
            "root",
            "owner",
            "site",
            "epoch",
            "identity",
            "checkpoint",
            "nonce",
            "release_key",
        ] {
            let mut changed = serde_json::to_value(&tx).unwrap();
            let value = changed.get_mut(field).unwrap();
            if value.is_string() {
                *value = serde_json::json!("changed");
            } else if value.is_number() {
                *value = serde_json::json!(99);
            } else if value.is_array() {
                value[0] = serde_json::json!(0);
            } else {
                value["child_pid"] = serde_json::json!(99);
            }
            let changed = serde_json::from_value::<Transaction>(changed).unwrap();
            assert!(
                read_release(&path, &changed).is_none(),
                "accepted changed {field}"
            );
        }
        let mut forged = serde_json::to_value(&release).unwrap();
        forged["signature"][0] = serde_json::json!(0);
        fs::write(&path, serde_json::to_vec(&forged).unwrap()).unwrap();
        assert!(read_release(&path, &tx).is_none());
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(directory.join("prepared"), &path).unwrap();
        create_json(&directory.join("prepared"), &release).unwrap();
        assert!(
            read_release(&path, &tx).is_none(),
            "symlink is not release authority"
        );
    }
    #[test]
    fn incomplete_oversized_and_expired_frames_fail_before_activation() {
        for size in [
            u64::MAX,
            aw_terminal::MAX_CHECKPOINT_WIRE_BYTES as u64 + 1,
            12,
        ] {
            let (mut sender, mut receiver) = UnixStream::pair().unwrap();
            sender.write_all(&size.to_be_bytes()).unwrap();
            drop(sender);
            assert!(read_frame(&mut receiver).is_err());
        }
        let (_sender, mut receiver) = UnixStream::pair().unwrap();
        assert!(read_exact_until(&mut receiver, &mut [0; 1], Instant::now()).is_err());
    }
}
