use crate::{changed, filter::Filter, Message, Shared, Status, MAX_INPUT};
use portable_pty::{Child, MasterPty, PtySize};
#[cfg(target_os = "linux")]
use std::os::fd::{AsRawFd, OwnedFd};
use std::{
    io::{Read, Write},
    sync::{atomic::Ordering, mpsc, Arc},
    time::{Duration, Instant},
};

enum ReadEvent {
    Bytes(Vec<u8>),
    #[cfg(target_os = "linux")]
    Paused(u64),
    Closed,
    Failed,
}

/// Portable PTY reactor. portable_pty owns the Unix PTY / Windows ConPTY
/// distinction. A dedicated reader thread drains output even while the view is
/// hidden; this owner thread serializes resize/input/child lifecycle.
pub(crate) fn run(
    master: Box<dyn MasterPty + Send>,
    mut child: ChildGuard,
    mut reader: Box<dyn Read + Send>,
    mut writer: Box<dyn Write + Send>,
    rx: mpsc::Receiver<Message>,
    shared: Arc<Shared>,
    #[cfg(target_os = "linux")] monitor_fd: OwnedFd,
) {
    let (output_tx, output_rx) = mpsc::sync_channel::<ReadEvent>(32);
    #[cfg(target_os = "linux")]
    let reader_state = shared.clone();
    let reader_thread = std::thread::Builder::new()
        .name("aw-terminal-reader".into())
        .spawn(move || {
            let mut buffer = [0_u8; 8192];
            #[cfg(target_os = "linux")]
            let mut last_paused_generation = None;
            loop {
                #[cfg(target_os = "linux")]
                {
                    if reader_state.reader_pause_requested.load(Ordering::Acquire) {
                        let generation = reader_state.reader_epoch.load(Ordering::Acquire);
                        if last_paused_generation != Some(generation) {
                            if output_tx.send(ReadEvent::Paused(generation)).is_err() {
                                break;
                            }
                            last_paused_generation = Some(generation);
                        }
                        std::thread::sleep(Duration::from_millis(4));
                        continue;
                    }
                    last_paused_generation = None;
                    // The reader owns an independent CLOEXEC descriptor until
                    // it exits, so poll never sees a recycled original FD.
                    match aw_platform::pty_poll::readable(
                        monitor_fd.as_raw_fd(),
                        Duration::from_millis(16),
                    ) {
                        Ok(true) => {}
                        Ok(false) => continue,
                        Err(_) => {
                            let _ = output_tx.send(ReadEvent::Failed);
                            break;
                        }
                    }
                    if reader_state.reader_pause_requested.load(Ordering::Acquire) {
                        continue;
                    }
                }
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        let _ = output_tx.send(ReadEvent::Closed);
                        break;
                    }
                    Ok(n) => {
                        if output_tx
                            .send(ReadEvent::Bytes(buffer[..n].to_vec()))
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        let _ = output_tx.send(ReadEvent::Failed);
                        break;
                    }
                }
            }
        });

    if reader_thread.is_err() {
        changed(&shared, |state| {
            state.status = Status::Failed;
            state.notice = "Cannot start PTY output reader".into();
        });
        return;
    }

    let mut filter = Filter::default();
    let mut output_closed = false;
    let mut stopping: Option<Instant> = None;
    let mut exited = false;
    #[cfg(target_os = "linux")]
    let mut quiesce_reply: Option<(u64, mpsc::SyncSender<crate::Result<()>>)> = None;

    loop {
        while let Ok(message) = rx.try_recv() {
            match message {
                Message::Input(bytes) => {
                    let len = bytes.len();
                    match writer.write_all(&bytes).and_then(|_| writer.flush()) {
                        Ok(()) => {
                            shared.pending.fetch_sub(len, Ordering::AcqRel);
                        }
                        Err(_) => {
                            shared.pending.fetch_sub(len, Ordering::AcqRel);
                            changed(&shared, |state| {
                                state.notice =
                                    "Terminal input failed; unsent bytes discarded, not replayed"
                                        .into()
                            });
                            shared.stop.store(true, Ordering::Release);
                        }
                    }
                }
                Message::Resize(rows, cols) => match master.resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                }) {
                    Ok(()) => changed(&shared, |state| {
                        state.parser.screen_mut().set_size(rows, cols)
                    }),
                    Err(_) => changed(&shared, |state| {
                        state.notice = "PTY resize failed; previous size retained".into()
                    }),
                },
                #[cfg(target_os = "linux")]
                Message::Quiesce { epoch, reply } => {
                    let state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                    if state.quiescing
                        && state.quiesce_epoch == epoch
                        && state.status == Status::Running
                        && !shared.stop.load(Ordering::Acquire)
                    {
                        // State lock synchronizes request acceptance and
                        // timeout rollback: a stale request cannot repause.
                        shared.reader_epoch.store(epoch, Ordering::Release);
                        shared.reader_pause_requested.store(true, Ordering::Release);
                        quiesce_reply = Some((epoch, reply));
                    } else {
                        let _ = reply.send(Err(crate::Error::Unavailable(
                            "PTY quiescence request was revoked",
                        )));
                    }
                }
                Message::Stop => {
                    shared.stop.store(true, Ordering::Release);
                }
            }
        }

        if shared.stop.load(Ordering::Acquire) && stopping.is_none() {
            #[cfg(target_os = "linux")]
            {
                let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                state.quiescing = false;
                shared
                    .reader_pause_requested
                    .store(false, Ordering::Release);
                quiesce_reply.take();
            }
            changed(&shared, |state| state.status = Status::Stopping);
            stopping = Some(Instant::now());
            if let Some(pid) = child.process_id() {
                aw_platform::kill_process_group(pid);
            }
            let _ = child.kill();
        }

        if !exited {
            match child.try_wait() {
                Ok(Some(status)) => {
                    exited = true;
                    changed(&shared, |state| {
                        state.status = Status::Exited(status.exit_code())
                    });
                }
                Ok(None) => {}
                Err(_) => {
                    exited = true;
                    changed(&shared, |state| {
                        state.status = Status::Failed;
                        state.notice = "PTY wait failed; inspect launched processes".into();
                    });
                }
            }
        }

        if exited && output_closed {
            break;
        }
        if stopping.is_some_and(|started| started.elapsed() > Duration::from_secs(2)) {
            if !exited {
                changed(&shared, |state| {
                    state.status = Status::Failed;
                    state.notice =
                        "Terminal shutdown uncertain; inspect external child processes".into();
                });
            }
            break;
        }

        match output_rx.recv_timeout(Duration::from_millis(50)) {
            #[cfg(target_os = "linux")]
            Ok(ReadEvent::Paused(epoch)) => {
                // A delayed acknowledgment from a timed-out generation
                // must never consume the *new* generation's reply channel.
                if quiesce_reply
                    .as_ref()
                    .is_some_and(|(pending, _)| *pending == epoch)
                {
                    if let Some((pending_epoch, reply)) = quiesce_reply.take() {
                        let state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                        if state.quiescing
                            && state.quiesce_epoch == pending_epoch
                            && state.status == Status::Running
                            && shared.pending.load(Ordering::Acquire) == 0
                            && !shared.stop.load(Ordering::Acquire)
                        {
                            let _ = reply.send(Ok(()));
                        } else if state.quiescing && state.quiesce_epoch == pending_epoch {
                            let _ = reply.send(Err(crate::Error::Unavailable(
                                "PTY quiescence was not confirmed",
                            )));
                        }
                    }
                }
            }
            Ok(ReadEvent::Bytes(bytes)) => {
                let bytes = filter.push(&bytes);
                let mut replies = Vec::new();
                changed(&shared, |state| {
                    state.parser.process(&bytes);
                    replies = std::mem::take(&mut state.parser.callbacks_mut().bytes);
                });
                if !replies.is_empty()
                    && writer
                        .write_all(&replies)
                        .and_then(|_| writer.flush())
                        .is_err()
                {
                    changed(&shared, |state| {
                        state.notice = "PTY protocol reply failed".into()
                    });
                    shared.stop.store(true, Ordering::Release);
                }
            }
            Ok(ReadEvent::Closed) => {
                output_closed = true;
            }
            Ok(ReadEvent::Failed) => {
                output_closed = true;
                if !exited {
                    changed(&shared, |state| {
                        state.notice = "PTY output closed unexpectedly".into()
                    });
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                output_closed = true;
            }
        }

        if shared.pending.load(Ordering::Acquire) > MAX_INPUT {
            shared.stop.store(true, Ordering::Release);
        }
    }

    changed(&shared, |state| {
        if matches!(state.status, Status::Running | Status::Stopping) {
            state.status = Status::Failed;
            state.notice = "Terminal closed; inspect unfinished process effects".into();
        }
    });
    shared.pending.store(0, Ordering::Release);
    if !exited {
        if let Some(pid) = child.process_id() {
            aw_platform::kill_process_group(pid);
        }
        let _ = child.kill();
        let _ = child.try_wait();
    }
    drop(writer);
    drop(master);
    (shared.notify)();
}

/// Launch/worker failure must not orphan a child merely because no public handle
/// was returned. This guard is created immediately after successful spawn.
pub(crate) struct ChildGuard {
    inner: Box<dyn Child + Send + Sync>,
}

impl ChildGuard {
    pub fn new(inner: Box<dyn Child + Send + Sync>) -> Self {
        Self { inner }
    }
}

impl std::ops::Deref for ChildGuard {
    type Target = dyn Child + Send + Sync;

    fn deref(&self) -> &Self::Target {
        self.inner.as_ref()
    }
}

impl std::ops::DerefMut for ChildGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.as_mut()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if matches!(self.inner.try_wait(), Ok(None)) {
            if let Some(pid) = self.inner.process_id() {
                aw_platform::kill_process_group(pid);
            }
            let _ = self.inner.kill();
            let _ = self.inner.wait();
        }
    }
}
