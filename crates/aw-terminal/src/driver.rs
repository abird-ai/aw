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
pub(crate) struct Backend {
    pub master: Master,
    pub child: Custodian,
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub filter: Filter,
    #[cfg(target_os = "linux")]
    pub monitor_fd: OwnedFd,
}
pub(crate) fn run(backend: Backend, rx: mpsc::Receiver<Message>, shared: Arc<Shared>) {
    let Backend {
        master,
        mut child,
        mut reader,
        mut writer,
        mut filter,
        #[cfg(target_os = "linux")]
        monitor_fd,
    } = backend;
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
                    if reader_state.reader_retired.load(Ordering::Acquire) {
                        break;
                    }
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
    #[cfg(target_os = "linux")]
    let reader_thread = reader_thread.expect("reader spawn already checked");

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
                        #[cfg(target_os = "linux")]
                        state.journal.resize(rows, cols);
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
                #[cfg(target_os = "linux")]
                Message::Stage { epoch, reply } => {
                    let state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                    let prepared = state.quiescing
                        && state.quiesce_epoch == epoch
                        && state.status == Status::Running
                        && state.source_release == crate::SourceRelease::Retained
                        && !shared.stop.load(Ordering::Acquire)
                        && shared.pending.load(Ordering::Acquire) == 0
                        && shared.reader_pause_requested.load(Ordering::Acquire);
                    let result = if prepared {
                        match (master.as_raw_fd(), child.process_id()) {
                            (Some(fd), Some(pid)) => (|| -> crate::Result<_> {
                                use std::os::fd::AsFd;
                                let master = aw_platform::pty_poll::duplicate(fd)?;
                                let pidfd = aw_platform::pid_identity::open(pid)?;
                                if aw_platform::pid_identity::exited(pidfd.as_fd())? {
                                    return Err(crate::Error::Unavailable(
                                        "original PTY child already exited",
                                    ));
                                }
                                Ok((master, pidfd))
                            })(),
                            _ => Err(crate::Error::Unavailable(
                                "original PTY or child process identity unavailable",
                            )),
                        }
                    } else {
                        Err(crate::Error::Unavailable(
                            "PTY escrow requires live acknowledged reader pause",
                        ))
                    };
                    let _ = reply.send(result);
                }
                #[cfg(target_os = "linux")]
                Message::Release {
                    epoch,
                    terminal_id,
                    reply,
                    publish,
                } => {
                    let state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                    let exclusive = state.source_release == crate::SourceRelease::Pending(epoch)
                        && state.quiescing
                        && state.quiesce_epoch == epoch
                        && state.status == Status::Running
                        && shared.pending.load(Ordering::Acquire) == 0
                        && shared.reader_pause_requested.load(Ordering::Acquire)
                        && !shared.stop.load(Ordering::Acquire);
                    let live = matches!(child.try_wait(), Ok(None));
                    if !exclusive || !live {
                        drop(state);
                        changed(&shared, |state| {
                            if state.source_release == crate::SourceRelease::Pending(epoch)
                                && state.quiesce_epoch == epoch
                            {
                                state.source_release = crate::SourceRelease::Retained;
                                state.quiescing = false;
                                shared
                                    .reader_pause_requested
                                    .store(false, Ordering::Release);
                            }
                        });
                        let _ = reply.send(Err(crate::Error::Unavailable(
                            "PTY source release rejected before custody changed",
                        )));
                        continue;
                    }
                    // Admission is irreversible to the caller. No RAII pause
                    // rollback or original-owner Stop may reactivate custody.
                    shared.reader_retired.store(true, Ordering::Release);
                    drop(state);
                    if reader_thread.join().is_err() {
                        changed(&shared, |state| {
                            state.status = Status::Failed;
                            state.notice = "PTY reader retirement failed; inspect custody".into();
                        });
                        let _ = reply.send(Err(crate::Error::Unavailable(
                            "PTY source release UNKNOWN after reader retirement failure",
                        )));
                        return; // original armed guard still owns failure cleanup
                    }
                    // Reader, its poll duplicate, and every sender escrow FD
                    // are closed before the final source master/writer closes.
                    drop(writer);
                    drop(master);
                    let mut waiter = child.into_waiter();
                    {
                        let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                        state.source_release = crate::SourceRelease::Released(epoch);
                        state.status = Status::Released;
                        state.notice =
                            "PTY source released; destination activation is separate".into();
                        state.version = state.version.wrapping_add(1);
                    }
                    let receipt = crate::SourceReleaseReceipt { terminal_id, epoch };
                    // Publish only after closure/disarm. The original worker
                    // owns publication even if its waiting caller disappears.
                    let publication =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            publish.map_or(Ok(()), |publish| publish(&receipt))
                        }))
                        .unwrap_or(Err(crate::Error::Unavailable(
                            "source publication panicked; inspect custody",
                        )));
                    let _ = reply.send(publication.map(|()| receipt));
                    // Observer faults must not strand the original-parent
                    // waiter or undo the already factual source release.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        (shared.notify)()
                    }));
                    // Reuse this existing worker as the original parent's
                    // non-signaling reaper. Source exit lets Linux reparent.
                    let projection = Arc::downgrade(&shared);
                    drop(shared); // a passive waiter need not retain parser/history
                    if waiter.wait().is_err() {
                        if let Some(shared) = projection.upgrade() {
                            let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                            state.notice =
                                "PTY source released; parent wait failed, inspect child custody"
                                    .into();
                            state.version = state.version.wrapping_add(1);
                        }
                    }
                    return;
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
            child.begin_stop();
        }

        #[cfg(target_os = "linux")]
        if !exited
            && child.adopted()
            && stopping.is_some_and(|started| started.elapsed() >= Duration::from_millis(500))
        {
            let _ = child.kill();
        }
        if !exited {
            match child.try_wait() {
                Ok(Some(status)) => {
                    exited = true;
                    changed(&shared, |state| state.status = status);
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
                let sanitized = filter.push(&bytes);
                let mut replies = Vec::new();
                changed(&shared, |state| {
                    #[cfg(target_os = "linux")]
                    state.journal.output(&sanitized);
                    #[cfg(target_os = "linux")]
                    {
                        state.checkpoint_filter = filter.clone();
                    }
                    state.parser.process(&sanitized);
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
        child.begin_stop();
        let _ = child.kill();
        let _ = child.try_wait();
    }
    #[cfg(target_os = "linux")]
    {
        // Descendants may retain a silent slave after the exact shell exits.
        // Retire the reader and unblock a full output channel before joining;
        // otherwise its reader/monitor FDs could retain custody indefinitely.
        shared.reader_retired.store(true, Ordering::Release);
        drop(output_rx);
        if reader_thread.join().is_err() {
            changed(&shared, |state| {
                state.status = Status::Failed;
                state.notice = "PTY reader shutdown uncertain; inspect process effects".into();
            });
        }
    }
    drop(writer);
    drop(master);
    (shared.notify)();
}

/// One reactor for original and adopted PTYs; adoption never constructs a
/// portable child handle or invents parentage/wait status.
pub(crate) enum Master {
    Original(Box<dyn MasterPty + Send>),
    #[cfg(target_os = "linux")]
    Adopted(OwnedFd),
}
impl Master {
    fn resize(&self, size: PtySize) -> std::io::Result<()> {
        match self {
            Self::Original(master) => master.resize(size).map_err(std::io::Error::other),
            #[cfg(target_os = "linux")]
            Self::Adopted(master) => {
                use std::os::fd::AsFd;
                aw_platform::pty_poll::resize(master.as_fd(), size.rows, size.cols)
            }
        }
    }
    #[cfg(target_os = "linux")]
    fn as_raw_fd(&self) -> Option<std::os::fd::RawFd> {
        match self {
            Self::Original(master) => master.as_raw_fd(),
            Self::Adopted(master) => Some(master.as_raw_fd()),
        }
    }
}
pub(crate) enum Custodian {
    Original(ChildGuard),
    #[cfg(target_os = "linux")]
    Adopted(OwnedFd),
}
impl Custodian {
    fn process_id(&self) -> Option<u32> {
        match self {
            Self::Original(child) => child.process_id(),
            #[cfg(target_os = "linux")]
            Self::Adopted(_) => None, // never signal an adopted process group
        }
    }
    #[cfg(target_os = "linux")]
    fn adopted(&self) -> bool {
        matches!(self, Self::Adopted(_))
    }
    fn try_wait(&mut self) -> std::io::Result<Option<Status>> {
        match self {
            Self::Original(child) => child
                .try_wait()
                .map(|value| value.map(|s| Status::Exited(s.exit_code()))),
            #[cfg(target_os = "linux")]
            Self::Adopted(pidfd) => {
                use aw_platform::pid_identity::{inspect_exit, ChildExit, ExitObservation};
                use std::os::fd::AsFd;
                match inspect_exit(pidfd.as_fd())? {
                    ExitObservation::Running => Ok(None),
                    ExitObservation::Exited { status } => {
                        // Observation does not grant exclusive wait ownership.
                        // The original parent's passive waiter may share this
                        // process in a trusted embedding. Never race its reap.
                        // An independently launched broker is a nonparent.
                        Ok(Some(match status {
                            Some(ChildExit::Code(code)) => Status::Exited(u32::from(code)),
                            Some(ChildExit::Signal { number, .. }) => Status::Signalled(number),
                            None => Status::ExitedUnknown,
                        }))
                    }
                }
            }
        }
    }
    fn kill(&mut self) -> std::io::Result<()> {
        match self {
            Self::Original(child) => child.kill(),
            #[cfg(target_os = "linux")]
            Self::Adopted(pidfd) => {
                use std::os::fd::AsFd;
                aw_platform::pid_identity::signal(
                    pidfd.as_fd(),
                    aw_platform::pid_identity::ProcessSignal::Kill,
                )
            }
        }
    }
    fn begin_stop(&mut self) {
        match self {
            Self::Original(child) => {
                if let Some(pid) = child.process_id() {
                    aw_platform::kill_process_group(pid);
                }
                let _ = child.kill();
            }
            #[cfg(target_os = "linux")]
            Self::Adopted(pidfd) => {
                use std::os::fd::AsFd;
                let _ = aw_platform::pid_identity::signal(
                    pidfd.as_fd(),
                    aw_platform::pid_identity::ProcessSignal::Terminate,
                );
            }
        }
    }
    #[cfg(target_os = "linux")]
    fn into_waiter(mut self) -> ChildWaiter {
        match &mut self {
            Self::Original(child) => ChildWaiter {
                inner: child.inner.take().expect("owned original child"),
            },
            Self::Adopted(_) => {
                unreachable!("adopted terminals cannot stage original-child release")
            }
        }
    }
}
impl Drop for Custodian {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        if let Self::Adopted(pidfd) = self {
            use std::os::fd::AsFd;
            // Activated custody only. Inert escrow never constructs this type.
            if matches!(
                aw_platform::pid_identity::inspect_exit(pidfd.as_fd()),
                Ok(aw_platform::pid_identity::ExitObservation::Running)
            ) {
                let _ = aw_platform::pid_identity::signal(
                    pidfd.as_fd(),
                    aw_platform::pid_identity::ProcessSignal::Kill,
                );
            }
        }
    }
}

/// Launch failure must not orphan an original child.
pub(crate) struct ChildGuard {
    inner: Option<Box<dyn Child + Send + Sync>>,
}
impl ChildGuard {
    pub fn new(inner: Box<dyn Child + Send + Sync>) -> Self {
        Self { inner: Some(inner) }
    }
}
#[cfg(target_os = "linux")]
struct ChildWaiter {
    inner: Box<dyn Child + Send + Sync>,
}
#[cfg(target_os = "linux")]
impl ChildWaiter {
    fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
        self.inner.wait()
    }
}
impl std::ops::Deref for ChildGuard {
    type Target = dyn Child + Send + Sync;
    fn deref(&self) -> &Self::Target {
        self.inner.as_deref().expect("owned original child")
    }
}
impl std::ops::DerefMut for ChildGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.as_deref_mut().expect("owned original child")
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let Some(inner) = self.inner.as_mut() else {
            return;
        };
        if matches!(inner.try_wait(), Ok(None)) {
            if let Some(pid) = inner.process_id() {
                aw_platform::kill_process_group(pid);
            }
            let _ = inner.kill();
            let _ = inner.wait();
        }
    }
}
