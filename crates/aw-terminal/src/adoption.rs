//! Inert Linux escrow. Only a trusted native host may activate it after
//! authenticating the exact original worker's durable release publication.
use crate::{checkpoint, driver, Error, Result, Shared, SourceRelease, State, Status, Terminal};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    os::fd::{AsFd, AsRawFd, OwnedFd},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize},
        mpsc, Arc, Mutex,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EscrowIdentity {
    pub child_pid: u32,
    pub pty_number: u32,
}

/// Preparing/dropping escrow never reads, writes, resizes or signals a PTY.
/// This contains no executable and cannot create a replacement child.
pub struct Adoption {
    checkpoint: checkpoint::Checkpoint,
    replayed: crate::Replayed,
    master: OwnedFd,
    pidfd: OwnedFd,
}
impl Adoption {
    pub fn prepare(
        checkpoint: checkpoint::Checkpoint,
        master: OwnedFd,
        pidfd: OwnedFd,
        expected: EscrowIdentity,
    ) -> Result<Self> {
        let actual = EscrowIdentity {
            child_pid: aw_platform::pid_identity::live_pid(pidfd.as_fd())?,
            pty_number: aw_platform::pty_poll::number(master.as_fd())?,
        };
        if actual != expected {
            return Err(Error::Unavailable("received PTY/process identity mismatch"));
        }
        let replayed = checkpoint.replay()?;
        Ok(Self {
            checkpoint,
            replayed,
            master,
            pidfd,
        })
    }

    /// Caller must authenticate a release record bound to this exact prepared
    /// checkpoint/recipient. FD receipt and source death do not authorize this.
    /// Creates the sole receiver reader using the existing reactor; no spawn.
    pub fn activate(self, notify: impl Fn() + Send + Sync + 'static) -> Result<Terminal> {
        if aw_platform::pid_identity::exited(self.pidfd.as_fd())? {
            return Err(Error::Unavailable("adopted child exited before activation"));
        }
        let reader = Box::new(File::from(aw_platform::pty_poll::duplicate(
            self.master.as_raw_fd(),
        )?));
        let writer = Box::new(File::from(aw_platform::pty_poll::duplicate(
            self.master.as_raw_fd(),
        )?));
        let monitor = aw_platform::pty_poll::duplicate(self.master.as_raw_fd())?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                parser: self.replayed.parser,
                version: 0,
                status: Status::Running,
                notice: "Adopted original PTY; exit status may be unavailable to this owner".into(),
                quiescing: false,
                quiesce_epoch: self.checkpoint.epoch,
                journal: checkpoint::Journal {
                    rows: self.checkpoint.rows,
                    cols: self.checkpoint.cols,
                    events: self.checkpoint.events,
                    bytes: self.checkpoint.bytes,
                    complete: true,
                },
                checkpoint_filter: self.replayed.filter.clone(),
                source_release: SourceRelease::Retained,
            }),
            stop: AtomicBool::new(false),
            pending: AtomicUsize::new(0),
            notify: Arc::new(notify),
            reader_pause_requested: AtomicBool::new(false),
            reader_epoch: AtomicU64::new(0),
            reader_retired: AtomicBool::new(false),
        });
        let (tx, rx) = mpsc::sync_channel(32);
        let state = shared.clone();
        let master = driver::Master::Adopted(self.master);
        let child = driver::Custodian::Adopted(self.pidfd);
        std::thread::Builder::new()
            .name("aw-terminal-adopted".into())
            .spawn(move || {
                driver::run(
                    driver::Backend {
                        master,
                        child,
                        reader,
                        writer,
                        filter: self.replayed.filter,
                        monitor_fd: monitor,
                    },
                    rx,
                    state,
                )
            })?;
        Ok(Terminal {
            id: self.checkpoint.id,
            root: self.checkpoint.root,
            owner: self.checkpoint.owner,
            site: self.checkpoint.site,
            shared,
            tx,
        })
    }
}
