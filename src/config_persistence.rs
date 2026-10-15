//! Coalesced application configuration persistence.
//!
//! UI interactions update in-memory typed state immediately. This service owns
//! bounded background writes of the latest complete FileConfig and provides an
//! explicit flush barrier for mode handoff/process exit.
use aw::{config, Error, Result};
use std::{
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

enum Message {
    Wake,
    Flush(mpsc::SyncSender<std::result::Result<(), String>>),
    Stop(mpsc::SyncSender<std::result::Result<(), String>>),
}

pub struct Service {
    tx: mpsc::SyncSender<Message>,
    latest: Arc<Mutex<Option<config::FileConfig>>>,
    last_error: Arc<Mutex<Option<String>>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Service {
    pub fn new(path: Option<PathBuf>, initial: config::FileConfig) -> Self {
        // A single wake token is enough: every queue() replaces the latest
        // snapshot. This keeps UI persistence nonblocking and memory-bounded.
        let (tx, rx) = mpsc::sync_channel(1);
        let latest = Arc::new(Mutex::new(None));
        let worker_latest = latest.clone();
        let last_error = Arc::new(Mutex::new(None));
        let worker_error = last_error.clone();
        let baseline = Arc::new(Mutex::new(initial));
        let worker_baseline = baseline.clone();
        let thread =
            thread::spawn(move || worker(path, rx, worker_latest, worker_baseline, worker_error));
        Self {
            tx,
            latest,
            last_error,
            thread: Some(thread),
        }
    }

    pub fn queue(&self, next: config::FileConfig) -> Result<()> {
        *self
            .latest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(next);
        match self.tx.try_send(Message::Wake) {
            Ok(()) | Err(mpsc::TrySendError::Full(Message::Wake)) => {}
            Err(mpsc::TrySendError::Disconnected(_)) => {
                return Err(Error::Storage(
                    "configuration persistence worker unavailable",
                ));
            }
            Err(mpsc::TrySendError::Full(_)) => {}
        }
        // queue() means the latest in-memory snapshot was accepted for
        // persistence. A previous asynchronous disk error does not retroactively
        // reject that UI mutation; last_error()/flush() report persistence health
        // and the retained snapshot remains retryable.
        Ok(())
    }

    pub fn last_error(&self) -> Option<String> {
        self.last_error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn flush(&self) -> Result<()> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx
            .send(Message::Flush(tx))
            .map_err(|_| Error::Storage("configuration persistence worker unavailable"))?;
        match rx.recv() {
            Ok(_) => match self.last_error() {
                Some(error) => Err(Error::Config(format!(
                    "configuration persistence failed: {error}"
                ))),
                None => Ok(()),
            },
            Err(_) => Err(Error::Storage(
                "configuration persistence worker stopped before flush",
            )),
        }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        let (tx, rx) = mpsc::sync_channel(1);
        if self.tx.send(Message::Stop(tx)).is_ok() {
            let _ = rx.recv();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn worker(
    path: Option<PathBuf>,
    rx: mpsc::Receiver<Message>,
    latest: Arc<Mutex<Option<config::FileConfig>>>,
    baseline: Arc<Mutex<config::FileConfig>>,
    last_error: Arc<Mutex<Option<String>>>,
) {
    const COALESCE: Duration = Duration::from_millis(40);

    while let Ok(message) = rx.recv() {
        match message {
            Message::Wake => loop {
                match rx.recv_timeout(COALESCE) {
                    Ok(Message::Wake) => {}
                    Ok(Message::Flush(reply)) => {
                        persist(&path, &latest, &baseline, &last_error);
                        let _ = reply.send(current_result(&last_error));
                        break;
                    }
                    Ok(Message::Stop(reply)) => {
                        persist(&path, &latest, &baseline, &last_error);
                        let _ = reply.send(current_result(&last_error));
                        return;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        persist(&path, &latest, &baseline, &last_error);
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        persist(&path, &latest, &baseline, &last_error);
                        return;
                    }
                }
            },
            Message::Flush(reply) => {
                persist(&path, &latest, &baseline, &last_error);
                let _ = reply.send(current_result(&last_error));
            }
            Message::Stop(reply) => {
                persist(&path, &latest, &baseline, &last_error);
                let _ = reply.send(current_result(&last_error));
                return;
            }
        }
    }
    persist(&path, &latest, &baseline, &last_error);
}

fn persist(
    path: &Option<PathBuf>,
    latest: &Arc<Mutex<Option<config::FileConfig>>>,
    baseline: &Arc<Mutex<config::FileConfig>>,
    last_error: &Arc<Mutex<Option<String>>>,
) {
    let pending = latest
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    let Some(config) = pending else {
        return;
    };
    let before = baseline
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    match config::save_changes(path.as_deref(), &before, &config).map_err(|error| error.to_string())
    {
        Ok(_) => {
            // Baseline tracks this process's last desired snapshot, not the
            // merged disk result. External fields absent from the in-memory
            // host therefore never become accidental removals on the next UI
            // save.
            *baseline
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = config.clone();
            *last_error
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        }
        Err(error) => {
            // Keep the newest pending snapshot. If no newer queue() raced with
            // this failed write, restore the failed snapshot so flush()/Stop or
            // the next Wake can retry it instead of losing user configuration.
            let mut pending = latest
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if pending.is_none() {
                *pending = Some(config);
            }
            *last_error
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(error);
        }
    }
}

fn current_result(last_error: &Arc<Mutex<Option<String>>>) -> std::result::Result<(), String> {
    match last_error
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
    {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_write_retains_latest_snapshot_for_explicit_retry() {
        let root = std::env::temp_dir().join(format!(
            "aw-config-persistence-retry-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        std::fs::create_dir(&path).unwrap();

        let service = Service::new(Some(path.clone()), config::FileConfig::default());
        let mut next = config::FileConfig::default();
        next.ui.editor_padding_x = 3;
        service.queue(next).unwrap();
        assert!(service.flush().is_err());
        assert!(service.last_error().is_some());

        std::fs::remove_dir(&path).unwrap();
        let mut newer = config::FileConfig::default();
        newer.ui.editor_padding_x = 2;
        service.queue(newer).unwrap();
        service.flush().unwrap();
        assert!(service.last_error().is_none());
        assert_eq!(config::load(Some(&path)).unwrap().ui.editor_padding_x, 2);

        drop(service);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn newest_config_wins_and_flush_is_an_explicit_barrier() {
        let root = std::env::temp_dir().join(format!(
            "aw-config-persistence-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        let service = Service::new(Some(path.clone()), config::FileConfig::default());

        for padding in 0..=3 {
            let mut next = config::FileConfig::default();
            next.ui.editor_padding_x = padding;
            service.queue(next).unwrap();
        }
        service.flush().unwrap();

        let loaded = config::load(Some(&path)).unwrap();
        assert_eq!(loaded.ui.editor_padding_x, 3);
        drop(service);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn coalesced_ui_save_preserves_external_skill_registry_change() {
        let root = std::env::temp_dir().join(format!(
            "aw-config-persistence-merge-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        let initial = config::FileConfig::default();
        config::save(Some(&path), &initial).unwrap();
        let service = Service::new(Some(path.clone()), initial.clone());

        let skill = root.join("review.md");
        std::fs::write(&skill, "review").unwrap();
        config::mutate(Some(&path), |cfg| {
            cfg.skills.insert(
                "review".into(),
                config::SkillConfig {
                    path: skill.clone(),
                    description: None,
                },
            );
            Ok(())
        })
        .unwrap();

        let mut local = initial;
        local.ui.output_padding = 2;
        service.queue(local).unwrap();
        service.flush().unwrap();
        let loaded = config::load(Some(&path)).unwrap();
        assert_eq!(loaded.ui.output_padding, 2);
        assert!(loaded.skills.contains_key("review"));
        drop(service);
        let _ = std::fs::remove_dir_all(root);
    }
}
