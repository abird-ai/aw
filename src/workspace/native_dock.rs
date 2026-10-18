//! Opt-in same-machine, same-owner second-terminal PTY presentation.
//!
//! A temporary private Unix socket and one-use invitation file admit a second
//! native display to *the already running PTY*. The original Workspace
//! process, Pool and Agent remain the sole owners. No background daemon,
//! shell replay, saved layout token, provider access or new PTY is created.
//! If that owner process dies the client disconnects and fails closed.
use super::{terminal_control::INVITE_TTL, terminals};
use aw::{Error, Result};
use aw_terminal::Snapshot;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::{self, BufRead, BufReader, IsTerminal, Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

const PROTOCOL: u8 = 1;
const MAX_WIRE: usize = 8 * 1024 * 1024;
const MAX_CLIENT: usize = 262_144;
const INPUT_BATCH: usize = 8 * 1024;
const HEARTBEAT_TTL: Duration = Duration::from_secs(4);
const FRAME_INTERVAL: Duration = Duration::from_millis(180);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Invite {
    version: u8,
    socket: PathBuf,
    terminal: String,
    ticket: String,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Hello { version: u8, ticket: String },
    Heartbeat,
    Input { bytes: Vec<u8> },
    Resize { rows: u16, cols: u16 },
    Detach,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Reply {
    Ready { version: u8, terminal: String },
    Frame { snapshot: Snapshot },
    /// Origin has fenced the recipient epoch and regained exclusive control.
    Detached { terminal: String },
    Error { message: String },
}

/// A directory created with unpredictable CSPRNG bytes, mode 0700, containing
/// only a 0600 invitation file and the owner-bound AF_UNIX socket.
struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0.join("invite"));
        let _ = fs::remove_file(self.0.join("socket"));
        let _ = fs::remove_dir(&self.0);
    }
}
fn fail(message: impl Into<String>) -> Error {
    Error::Config(message.into())
}
fn send(stream: &mut UnixStream, value: &Reply) -> io::Result<()> {
    let mut encoded = serde_json::to_vec(value)?;
    if encoded.len() > MAX_WIRE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bounded terminal frame overflow"));
    }
    encoded.push(b'\n');
    stream.write_all(&encoded)
}
fn request(reader: &mut BufReader<UnixStream>, max: usize) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    let started = Instant::now();
    loop {
        match reader.fill_buf() {
            Ok([]) => return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof, "native owner connection closed"
            )),
            Ok(available) => {
                let count = available.iter().position(|byte| *byte == b'\n')
                    .map_or(available.len(), |index| index + 1);
                if line.len() + count > max {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "native input frame exceeds limit"));
                }
                line.extend_from_slice(&available[..count]);
                reader.consume(count);
                if line.last() == Some(&b'\n') {
                    line.pop();
                    return Ok(Some(line));
                }
            }
            Err(error) if matches!(error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                if line.is_empty() { return Ok(None); }
                if started.elapsed() >= HEARTBEAT_TTL {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "incomplete native input frame"));
                }
            }
            Err(error) => return Err(error),
        }
    }
}
fn serve(mut stream: UnixStream, pool: &terminals::Pool, invite: &Invite) -> io::Result<bool> {
    stream.set_read_timeout(Some(Duration::from_millis(130)))?;
    stream.set_write_timeout(Some(Duration::from_millis(850)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let until = Instant::now() + Duration::from_secs(4);
    let hello = loop {
        match request(&mut reader, 256)? {
            Some(value) => break serde_json::from_slice::<Request>(&value)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
            None if Instant::now() < until => {}
            None => return Err(io::Error::new(io::ErrorKind::TimedOut, "no explicit terminal hello")),
        }
    };
    let Request::Hello { version, ticket } = hello else {
        send(&mut stream, &Reply::Error { message: "explicit terminal hello required".into() })?;
        return Ok(false);
    };
    if version != PROTOCOL {
        send(&mut stream, &Reply::Error { message: "unsupported native dock protocol".into() })?;
        return Ok(false);
    }
    if ticket.len() != 64 {
        send(&mut stream, &Reply::Error { message: "invalid or expired terminal invite".into() })?;
        return Ok(false);
    }
    // This is the *only* control transfer. The original renderer is read-only
    // until the non-cloneable RAII lease is dropped. The Pool checks the
    // original retained terminal identity, state, ticket digest, input drain
    // and monotonic control epoch under a single mutex before admission.
    let lease = match pool.accept_dock(&invite.terminal, &ticket) {
        Ok(lease) => lease,
        Err(error) => {
            send(&mut stream, &Reply::Error { message: error.to_string() })?;
            return Ok(false);
        }
    };
    send(&mut stream, &Reply::Ready { version: PROTOCOL, terminal: invite.terminal.clone() })?;
    let mut last_seen = Instant::now();
    let mut detached_by_request = false;
    let mut last_frame = Instant::now() - FRAME_INTERVAL;
    let mut last_version = None;
    loop {
        if pool.is_shutting_down() || last_seen.elapsed() > HEARTBEAT_TTL { break; }
        if last_frame.elapsed() >= FRAME_INTERVAL {
            let snapshot = match pool.snapshot_docked(&lease, 0) {
                Ok(snapshot) => snapshot,
                Err(_) => break,
            };
            if last_version != Some(snapshot.version) || last_frame.elapsed() >= Duration::from_secs(1) {
                send(&mut stream, &Reply::Frame { snapshot: snapshot.clone() })?;
                last_version = Some(snapshot.version);
            }
            last_frame = Instant::now();
        }
        match request(&mut reader, MAX_CLIENT) {
            Ok(Some(frame)) => {
                let request = serde_json::from_slice::<Request>(&frame)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                last_seen = Instant::now();
                match request {
                    Request::Heartbeat => {}
                    Request::Resize { rows, cols } => {
                        if let Err(error) = pool.resize_docked(&lease, rows, cols) {
                            let _ = send(&mut stream, &Reply::Error {
                                message: format!("PTY resize denied: {error}; control returning to origin"),
                            });
                            break;
                        }
                    }
                    Request::Input { bytes } => {
                        if let Err(error) = pool.input_docked(&lease, bytes) {
                            // Never echo submitted input or replay uncertain
                            // shell side effects after an I/O denial.
                            let _ = send(&mut stream, &Reply::Error {
                                message: format!("PTY input rejected: {error}; earlier shell effects may remain"),
                            });
                            break;
                        }
                    }
                    Request::Detach => { detached_by_request = true; break; }
                    Request::Hello { .. } => break, // replay is not a fresh controller
                }
            }
            Ok(None) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error),
        }
    }
    drop(lease); // fence this exact epoch; return control only after input settles
    if detached_by_request {
        let until = Instant::now() + Duration::from_secs(3);
        while pool.dock_mode(&invite.terminal) != Some(super::terminal_control::Mode::Local)
            && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(12));
        }
        if pool.dock_mode(&invite.terminal) != Some(super::terminal_control::Mode::Local) {
            send(&mut stream, &Reply::Error {
                message: "terminal input still settling; detach outcome unknown".into(),
            })?;
            return Ok(true);
        }
        // Only a receipt *after* the owner regained control makes a recipient
        // F12 detach successful. Lost receipt remains UNKNOWN, never replayed.
        send(&mut stream, &Reply::Detached { terminal: invite.terminal.clone() })?;
    }
    Ok(true)
}

fn private_directory() -> Result<(PathBuf, Cleanup)> {
    for _ in 0..4 {
        let mut bytes = [0_u8; 8];
        getrandom::fill(&mut bytes)
            .map_err(|_| fail("cannot create secure local docking directory"))?;
        let suffix = bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        let dir = std::env::temp_dir().join(format!("aw-pty-{suffix}"));
        match DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => return Ok((dir.clone(), Cleanup(dir))),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(fail(error.to_string())),
        }
    }
    Err(fail("unable to allocate unique native docking socket"))
}

/// Explicit human action from a focused, already running terminal. The
/// invitation is *not* a credential for the Agent or any other PTY.
pub fn invite(pool: terminals::Pool, terminal_id: &str) -> Result<PathBuf> {
    let (directory, cleanup) = private_directory()?;
    let listener = UnixListener::bind(directory.join("socket"))
        .map_err(|error| fail(format!("native docking socket: {error}")))?;
    listener.set_nonblocking(true).map_err(|error| fail(error.to_string()))?;
    let dock = pool.prepare_dock(terminal_id)?;
    let invitation = Invite {
        version: PROTOCOL, socket: directory.join("socket"),
        terminal: dock.terminal_id, ticket: dock.ticket,
    };
    let invite_path = directory.join("invite");
    let id = invitation.terminal.clone();
    let write_invite = (|| -> Result<()> {
        let bytes = serde_json::to_vec(&invitation).map_err(|error| fail(error.to_string()))?;
        let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600)
            .open(&invite_path).map_err(|error| fail(error.to_string()))?;
        file.write_all(&bytes).map_err(|error| fail(error.to_string()))?;
        file.sync_all().map_err(|error| fail(error.to_string()))
    })();
    if let Err(error) = write_invite {
        let _ = pool.cancel_dock(&id);
        return Err(error);
    }
    let retained_pool = pool.clone();
    let server_id = id.clone();
    let task = std::thread::Builder::new().name("aw-native-dock".into()).spawn(move || {
        let pool = retained_pool;
        let id = server_id;
        let _cleanup = cleanup;
        let deadline = Instant::now() + INVITE_TTL;
        loop {
            if Instant::now() >= deadline || pool.is_shutting_down()
                || !pool.has_invitation(&id, &invitation.ticket) {
                break;
            }
            match listener.accept() {
                Ok((client, _)) => match serve(client, &pool, &invitation) {
                    Ok(true) => break, // used once; never automatically replay
                    Ok(false) => {}
                    Err(_) => {} // malformed/untrusted clients cannot transfer
                },
                Err(error) if error.kind() == io::ErrorKind::WouldBlock =>
                    std::thread::sleep(Duration::from_millis(30)),
                Err(_) => break,
            }
        }
        let _ = pool.cancel_dock(&id);
    });
    if let Err(error) = task {
        // A failure starting the listener cannot retain an unseen,
        // unconsumable invitation against the original PTY.
        let _ = pool.cancel_dock(&id);
        return Err(fail(format!("native docking listener unavailable: {error}")));
    }
    Ok(invite_path)
}

fn read_invite(path: &Path) -> Result<Invite> {
    if !path.is_absolute() || path.file_name().is_none_or(|file| file != "invite") {
        return Err(Error::Denied("use an absolute owner-generated native invitation file"));
    }
    let meta = fs::symlink_metadata(path).map_err(|error| fail(error.to_string()))?;
    let parent = path.parent().ok_or(Error::Denied("invalid invitation directory"))?;
    let folder = fs::symlink_metadata(parent).map_err(|error| fail(error.to_string()))?;
    if !meta.file_type().is_file() || !folder.file_type().is_dir()
        || meta.mode() & 0o077 != 0 || folder.mode() & 0o077 != 0
        || meta.uid() != folder.uid() || meta.len() > 4096 || meta.len() == 0 {
        return Err(Error::Denied("native invitation must be private and bounded"));
    }
    // Refuse a pathname swapped between metadata checks and opening the
    // invitation. Read from the *opened inode* with a hard 4096-byte cap.
    let file = OpenOptions::new().read(true).open(path).map_err(Error::Io)?;
    let opened = file.metadata().map_err(Error::Io)?;
    if !opened.is_file() || opened.dev() != meta.dev() || opened.ino() != meta.ino()
        || opened.uid() != folder.uid() || opened.mode() & 0o077 != 0 {
        return Err(Error::Denied("native invitation identity replaced"));
    }
    let mut bytes = Vec::with_capacity(opened.len() as usize);
    file.take(4097).read_to_end(&mut bytes).map_err(Error::Io)?;
    if bytes.len() > 4096 {
        return Err(Error::Denied("native invitation exceeds maximum size"));
    }
    let invitation: Invite = serde_json::from_slice(&bytes)
        .map_err(|_| Error::Denied("malformed native invitation"))?;
    if invitation.version != PROTOCOL || invitation.socket != parent.join("socket")
        || invitation.ticket.len() != 64
        || !invitation.ticket.bytes().all(|byte| byte.is_ascii_hexdigit())
        || invitation.terminal.len() > 128 {
        return Err(Error::Denied("invalid native invitation binding"));
    }
    Ok(invitation)
}

async fn send_async(writer: &mut tokio::io::WriteHalf<tokio::net::UnixStream>, request: Request) -> Result<()> {
    let mut data = serde_json::to_vec(&request).map_err(|error| fail(error.to_string()))?;
    if data.len() > MAX_CLIENT { return Err(Error::Denied("native control request too large")); }
    data.push(b'\n');
    writer.write_all(&data).await.map_err(Error::Io)
}

/// An untrusted or malformed owner frame must not allocate unbounded memory.
/// Read no more than the schema's maximum payload plus its delimiter.
async fn read_bounded_frame<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    line: &mut Vec<u8>,
    max: usize,
) -> Result<Option<Vec<u8>>> {
    loop {
        let available = reader.fill_buf().await.map_err(Error::Io)?;
        if available.is_empty() {
            return if line.is_empty() { Ok(None) }
                else { Err(Error::Denied("incomplete native owner frame")) };
        }
        let count = available.iter().position(|byte| *byte == b'\n')
            .map_or(available.len(), |position| position + 1);
        if line.len().saturating_add(count) > max.saturating_add(1) {
            return Err(Error::Denied("native owner frame exceeds limit"));
        }
        line.extend_from_slice(&available[..count]);
        reader.consume(count);
        if line.last() == Some(&b'\n') {
            line.pop();
            return Ok(Some(std::mem::take(line)));
        }
    }
}

async fn flush_input(
    writer: &mut tokio::io::WriteHalf<tokio::net::UnixStream>,
    pending: &mut Vec<u8>,
) -> Result<()> {
    if !pending.is_empty() {
        // Never retry a potentially accepted PTY write after an ambiguous
        // transport failure. Control effects are not replayable UI state.
        send_async(writer, Request::Input { bytes: std::mem::take(pending) }).await?;
    }
    Ok(())
}

/// Real second terminal screen. No Host, Agent, Session or tool can be created
/// here: it only receives snapshots and forwards explicitly typed PTY bytes.
pub fn attach(invitation_path: &Path) -> Result<u8> {
    use crate::tui::terminal_session::{Surface, TerminalSession};
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(Error::Denied("native docking requires an interactive second terminal"));
    }
    let invitation = read_invite(invitation_path)?;
    let mut stream = UnixStream::connect(&invitation.socket).map_err(Error::Io)?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).map_err(Error::Io)?;
    let hello = serde_json::to_vec(&Request::Hello {
        version: PROTOCOL, ticket: invitation.ticket,
    }).map_err(|error| fail(error.to_string()))?;
    stream.write_all(&hello).map_err(Error::Io)?;
    stream.write_all(b"\n").map_err(Error::Io)?;
    // Consume exactly one reply line. Avoid a BufReader that might prefetch
    // and discard the first streamed snapshot before Tokio takes over.
    let mut first = Vec::new();
    loop {
        let mut byte = [0_u8; 1];
        stream.read_exact(&mut byte).map_err(Error::Io)?;
        if byte[0] == b'\n' { break; }
        if first.len() == 4096 {
            return Err(Error::Denied("native docking admission exceeds bound"));
        }
        first.push(byte[0]);
    }
    let reply: Reply = serde_json::from_slice(&first)
        .map_err(|_| Error::Denied("invalid native docking owner reply"))?;
    match reply {
        Reply::Ready { version, terminal } if version == PROTOCOL && terminal == invitation.terminal => {}
        Reply::Error { message } => return Err(fail(message)),
        _ => return Err(Error::Denied("native docking owner did not confirm terminal identity")),
    }
    // The invitation's one-use proof is now consumed by the *original*
    // retained Pool, not by another shell or a parallel Agent owner.
    stream.set_read_timeout(None).map_err(Error::Io)?;
    stream.set_nonblocking(true).map_err(Error::Io)?;
    let _session = TerminalSession::enter(Surface::Alternate { mouse: false })?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all().build().map_err(Error::Io)?
        .block_on(async {
            let stream = tokio::net::UnixStream::from_std(stream).map_err(Error::Io)?;
            let (read, mut write) = tokio::io::split(stream);
            let mut frames = tokio::io::BufReader::new(read);
            // Own partially read data outside Tokio select futures: a key,
            // resize or heartbeat can cancel frame reads mid-message.
            let mut partial_frame = Vec::new();
            let mut events = EventStream::new();
            let mut pulse = tokio::time::interval(Duration::from_secs(1));
            let mut flush = tokio::time::interval(Duration::from_millis(22));
            flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut pending = Vec::with_capacity(256);
            let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
            send_async(&mut write, Request::Resize {
                rows: rows.saturating_sub(1).clamp(2, 120), cols: cols.clamp(4, 300),
            }).await?;
            let mut latest: Option<Snapshot> = None;
            let mut closed = false;
            let mut detach_since: Option<Instant> = None;
            while !closed {
                tokio::select! {
                    frame = read_bounded_frame(&mut frames, &mut partial_frame, MAX_WIRE) => {
                        let Some(frame) = frame? else {
                            return Err(Error::Denied("original PTY owner disconnected; no session was replayed"));
                        };
                        match serde_json::from_slice::<Reply>(&frame)
                            .map_err(|_| Error::Denied("invalid native owner frame"))? {
                            Reply::Frame { snapshot } => {
                                if snapshot.id != invitation.terminal
                                    || usize::from(snapshot.rows) * usize::from(snapshot.cols) != snapshot.cells.len()
                                    || snapshot.rows > 120 || snapshot.cols > 300 {
                                    return Err(Error::Denied("native snapshot identity or geometry changed"));
                                }
                                latest = Some(snapshot);
                                if let Some(snapshot) = &latest {
                                    terminal.draw(|frame| terminals::draw_snapshot(
                                        frame, frame.area(), snapshot, true
                                    )).map_err(Error::Io)?;
                                }
                            }
                            Reply::Detached { terminal: id } if id == invitation.terminal
                                && detach_since.is_some() => { closed = true; }
                            Reply::Detached { .. } => return Err(Error::Denied("unsolicited native detach receipt")),
                            Reply::Error { message } => return Err(fail(message)),
                            Reply::Ready { .. } => return Err(Error::Denied("replayed native admission")),

                        }
                    }
                    incoming = events.next(), if detach_since.is_none() => {
                        match incoming {
                            Some(Ok(Event::Key(event))) => {
                                if event.kind == KeyEventKind::Release { continue; }
                                if event.code == KeyCode::F(12) && event.modifiers == KeyModifiers::NONE {
                                    flush_input(&mut write, &mut pending).await?;
                                    send_async(&mut write, Request::Detach).await?;
                                    detach_since = Some(Instant::now());
                                } else if let Some(bytes) = terminals::key(event,
                                    latest.as_ref().is_some_and(|snapshot| snapshot.application_cursor)) {
                                    if pending.len() + bytes.len() > INPUT_BATCH {
                                        flush_input(&mut write, &mut pending).await?;
                                    }
                                    let enter = event.code == KeyCode::Enter;
                                    pending.extend_from_slice(&bytes);
                                    if enter {
                                        flush_input(&mut write, &mut pending).await?;
                                    }
                                }
                            }
                            Some(Ok(Event::Paste(text))) => {
                                let bytes = aw_terminal::encode_paste(&text,
                                    latest.as_ref().is_some_and(|snapshot| snapshot.bracketed_paste))
                                    .map_err(|error| fail(error.to_string()))?;
                                flush_input(&mut write, &mut pending).await?;
                                for chunk in bytes.chunks(INPUT_BATCH) {
                                    send_async(&mut write, Request::Input { bytes: chunk.to_vec() }).await?;
                                }
                            }
                            Some(Ok(Event::Resize(cols, rows))) => {
                                flush_input(&mut write, &mut pending).await?;
                                send_async(&mut write, Request::Resize {
                                    rows: rows.saturating_sub(1).clamp(2,120), cols:cols.clamp(4,300),
                                }).await?;
                            }
                            Some(Ok(_)) => {}
                            Some(Err(error)) => return Err(Error::Io(error)),
                            None => return Err(Error::Denied("native terminal event stream closed")),
                        }
                    }
                    _ = flush.tick() => {
                        if detach_since.is_none() {
                            flush_input(&mut write, &mut pending).await?;
                        } else if detach_since.is_some_and(|at| at.elapsed() >= Duration::from_secs(5)) {
                            return Err(Error::Denied("native detach receipt unknown; original PTY not replayed"));
                        }
                    }
                    _ = pulse.tick(), if detach_since.is_none() => {
                        flush_input(&mut write, &mut pending).await?;
                        send_async(&mut write, Request::Heartbeat).await?;
                    }
                }
            }
            Ok(0)
        });
    // TerminalSession releases termios and alternate screen on every path.
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_public_or_forged_invitation_files() {
        let err = read_invite(Path::new("relative/path"));
        assert!(err.is_err());
        let err = read_invite(Path::new("/tmp/not-an-invite.txt"));
        assert!(err.is_err());
    }

    #[test]
    fn invitation_rejects_symlink_public_and_oversized_descriptors() {
        use std::os::unix::fs::symlink;
        let (root, _cleanup) = private_directory().unwrap();
        let path = root.join("invite");
        let other = root.join("other");
        fs::write(&other, b"private fixture").unwrap();
        symlink(&other, &path).unwrap();
        assert!(read_invite(&path).is_err(), "symlinked invitation is never followed");
        fs::remove_file(&path).unwrap();
        fs::remove_file(&other).unwrap();

        fs::write(&path, vec![b'x'; 4097]).unwrap();
        assert!(read_invite(&path).is_err(), "oversized descriptor is rejected");
        fs::remove_file(&path).unwrap();

        let invitation = Invite {
            version: PROTOCOL, socket: root.join("wrong-socket"),
            terminal: "synthetic_pty".into(), ticket: "a".repeat(64),
        };
        fs::write(&path, serde_json::to_vec(&invitation).unwrap()).unwrap();
        let permission = fs::metadata(&path).unwrap().permissions();
        let mut private = permission.clone();
        use std::os::unix::fs::PermissionsExt;
        private.set_mode(0o600);
        fs::set_permissions(&path, private).unwrap();
        assert!(read_invite(&path).is_err(), "socket cannot retarget another owner");
        let mut public = permission;
        public.set_mode(0o644);
        fs::set_permissions(&path, public).unwrap();
        assert!(read_invite(&path).is_err(), "public-readable descriptor is denied");
        fs::remove_file(&path).unwrap();
    }

    #[tokio::test]
    async fn cancelled_partial_native_frame_is_retained_and_oversize_is_denied() {
        let (mut writer, read) = tokio::io::duplex(512);
        let mut reader = tokio::io::BufReader::new(read);
        let mut partial = Vec::new();
        writer.write_all(b"{\"op\":").await.unwrap();
        let interrupted = tokio::time::timeout(Duration::from_millis(35),
            read_bounded_frame(&mut reader, &mut partial, 90)).await;
        assert!(interrupted.is_err(), "waiting for the rest of a split frame must suspend");
        assert_eq!(partial, b"{\"op\":", "cancelled read may not drop consumed bytes");
        writer.write_all(b"\"frame\",\"contents\":\"hello\"}\n").await.unwrap();
        let complete = read_bounded_frame(&mut reader, &mut partial, 90)
            .await.unwrap().unwrap();
        assert_eq!(complete, b"{\"op\":\"frame\",\"contents\":\"hello\"}");
        assert!(partial.is_empty());
        writer.write_all(b"1234567890\n").await.unwrap();
        assert!(read_bounded_frame(&mut reader, &mut partial, 4).await.is_err(),
            "too-large native owner frame must be rejected before allocation");
    }

    #[test]
    fn private_socket_transfers_exact_live_pty_and_disconnect_restores_source() {
        let mut random = [0_u8; 12];
        getrandom::fill(&mut random).unwrap();
        let root = std::env::temp_dir().join(format!("aw-native-dock-wire-test-{}",
            random.iter().map(|byte| format!("{byte:02x}")).collect::<String>()));
        fs::create_dir(&root).unwrap();
        let shell = std::env::var_os("AW_BASH").map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/bin/bash"));
        let pool = terminals::Pool::new();
        let id = pool.launch(root.clone(), shell, "synthetic_actor".into()).unwrap();
        let original = pool.get(&id).unwrap();
        let old_snapshot = original.snapshot(0);

        let invite_path = invite(pool.clone(), &id).unwrap();
        let token = read_invite(&invite_path).unwrap();
        assert_eq!(token.terminal, id);
        assert_eq!(token.version, PROTOCOL);
        assert_eq!(fs::metadata(&invite_path).unwrap().mode() & 0o077, 0);
        assert_eq!(fs::metadata(invite_path.parent().unwrap()).unwrap().mode() & 0o077, 0);
        assert_eq!(pool.dock_mode(&id), Some(super::super::terminal_control::Mode::Offered));
        assert!(invite(pool.clone(), &id).is_err(),
            "only one pending socket invitation belongs to this PTY");

        // A wrong proof must be denied by owner without consuming the token.
        let mut invalid = UnixStream::connect(&token.socket).unwrap();
        invalid.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        invalid.write_all(b"{\"op\":\"hello\",\"version\":1,\"ticket\":\"f\"}\n").unwrap();
        let mut reader = BufReader::new(invalid);
        let line = request(&mut reader, 8192).unwrap().unwrap();
        assert!(matches!(serde_json::from_slice::<Reply>(&line).unwrap(), Reply::Error { .. }));
        assert_eq!(pool.dock_mode(&id), Some(super::super::terminal_control::Mode::Offered));

        let mut socket = UnixStream::connect(&token.socket).unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        socket.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
        let hello = serde_json::to_vec(&Request::Hello {
            version: PROTOCOL, ticket: token.ticket,
        }).unwrap();
        socket.write_all(&hello).unwrap();
        socket.write_all(b"\n").unwrap();
        let mut reader = BufReader::new(socket.try_clone().unwrap());
        let admission = request(&mut reader, 8192).unwrap().unwrap();
        assert!(matches!(serde_json::from_slice::<Reply>(&admission).unwrap(),
            Reply::Ready { version: 1, .. }));
        assert_eq!(pool.dock_mode(&id), Some(super::super::terminal_control::Mode::Docked));
        assert!(pool.input_local(&id, b"echo SOURCE_MUST_BE_BLOCKED\n".to_vec()).is_err());
        let request_bytes = serde_json::to_vec(&Request::Input {
            bytes: b"echo SAME_RETained_PTY\n".to_vec(),
        }).unwrap();
        socket.write_all(&request_bytes).unwrap();
        socket.write_all(b"\n").unwrap();
        let received = request(&mut reader, MAX_WIRE).unwrap().unwrap();
        let Reply::Frame { snapshot } = serde_json::from_slice::<Reply>(&received).unwrap()
            else { panic!("receiver must read a real original PTY snapshot") };
        assert_eq!(snapshot.id, old_snapshot.id);
        assert_eq!(snapshot.root, root);
        assert_eq!(snapshot.owner, old_snapshot.owner);
        let bye = serde_json::to_vec(&Request::Detach).unwrap();
        socket.write_all(&bye).unwrap();
        socket.write_all(b"\n").unwrap();
        drop(reader);
        drop(socket);
        for _ in 0..120 {
            if pool.dock_mode(&id) == Some(super::super::terminal_control::Mode::Local)
                && !invite_path.exists() { break; }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert_eq!(pool.dock_mode(&id), Some(super::super::terminal_control::Mode::Local));
        assert!(!invite_path.exists(), "one-use invitation cleaned after disconnect");
        assert!(pool.input_local(&id, b"echo RETURNED_TO_ORIGIN\n".to_vec()).is_ok());
        assert_eq!(pool.get(&id).unwrap().id, old_snapshot.id);
        pool.shutdown();
        drop(pool);
        fs::remove_dir_all(root).unwrap();
    }
}
