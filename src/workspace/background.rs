//! Bounded background execution for human Workspace UI work.
//!
//! The native UI thread submits blocking/CPU-heavy presentation work here and
//! only consumes typed results. Agent/model/durable owner authority deliberately
//! remains on the separate owner worker.
use aw::{Error, Result};
use std::{
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
};

struct Task {
    run: Box<dyn FnOnce() + Send + 'static>,
    on_panic: Box<dyn FnOnce() + Send + 'static>,
}

struct State {
    queued: AtomicUsize,
    running: AtomicUsize,
    completed: AtomicU64,
    panicked: AtomicU64,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub queued: usize,
    pub running: usize,
    pub completed: u64,
    pub panicked: u64,
}

#[derive(Clone)]
pub struct Executor {
    tx: mpsc::SyncSender<Task>,
    state: Arc<State>,
}

impl Executor {
    pub fn new() -> Result<Self> {
        let workers = thread::available_parallelism()
            .map_or(2, usize::from)
            .clamp(2, 4);
        Self::with_limits(workers, 64)
    }

    fn with_limits(workers: usize, queue: usize) -> Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<Task>(queue.max(1));
        let rx = Arc::new(Mutex::new(rx));
        let state = Arc::new(State {
            queued: AtomicUsize::new(0),
            running: AtomicUsize::new(0),
            completed: AtomicU64::new(0),
            panicked: AtomicU64::new(0),
        });
        for index in 0..workers.max(1) {
            let rx = rx.clone();
            let state = state.clone();
            thread::Builder::new()
                .name(format!("aw-ui-bg-{index}"))
                .spawn(move || loop {
                    let task = {
                        let guard = rx.lock().unwrap_or_else(|e| e.into_inner());
                        guard.recv()
                    };
                    let Ok(task) = task else { break };
                    state.queued.fetch_sub(1, Ordering::AcqRel);
                    state.running.fetch_add(1, Ordering::AcqRel);
                    let Task { run, on_panic } = task;
                    let result = crate::tui::terminal_session::catch_isolated_panic(run);
                    if result.is_err() {
                        state.panicked.fetch_add(1, Ordering::AcqRel);
                        let _ = crate::tui::terminal_session::catch_isolated_panic(on_panic);
                    }
                    state.running.fetch_sub(1, Ordering::AcqRel);
                    state.completed.fetch_add(1, Ordering::AcqRel);
                })?;
        }
        Ok(Self { tx, state })
    }

    #[cfg(test)]
    pub fn try_submit(
        &self,
        label: &'static str,
        task: impl FnOnce() + Send + 'static,
    ) -> Result<()> {
        self.try_submit_with_panic(label, task, || {})
    }

    pub fn try_submit_with_panic(
        &self,
        label: &'static str,
        task: impl FnOnce() + Send + 'static,
        on_panic: impl FnOnce() + Send + 'static,
    ) -> Result<()> {
        self.state.queued.fetch_add(1, Ordering::AcqRel);
        match self.tx.try_send(Task {
            run: Box::new(task),
            on_panic: Box::new(on_panic),
        }) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => {
                self.state.queued.fetch_sub(1, Ordering::AcqRel);
                Err(Error::Conflict(match label {
                    "files" => "UI background queue full while loading files; retry explicitly",
                    "git" => "UI background queue full while loading Git data; retry explicitly",
                    "syntax" => "UI background queue full while highlighting; plain text remains available",
                    "terminal" => "UI background queue full while launching terminal; retry explicitly",
                    "layout" => "UI background queue full while saving/loading layout; retry explicitly",
                    "find" => "UI background queue full while searching view; retry explicitly",
                    _ => "UI background queue full; retry explicitly",
                }))
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                self.state.queued.fetch_sub(1, Ordering::AcqRel);
                Err(Error::Storage("UI background executor unavailable"))
            }
        }
    }

    #[cfg(test)]
    pub fn stats(&self) -> Stats {
        Stats {
            queued: self.state.queued.load(Ordering::Acquire),
            running: self.state.running.load(Ordering::Acquire),
            completed: self.state.completed.load(Ordering::Acquire),
            panicked: self.state.panicked.load(Ordering::Acquire),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc as channel, time::Duration};

    #[test]
    fn bounded_executor_runs_tasks_off_caller_and_reports_progress() {
        let executor = Executor::with_limits(2, 2).unwrap();
        let caller = thread::current().id();
        let (tx, rx) = channel::channel();
        executor
            .try_submit("test", move || tx.send(thread::current().id()).unwrap())
            .unwrap();
        let worker = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_ne!(caller, worker);
        let until = std::time::Instant::now() + Duration::from_secs(1);
        while executor.stats().completed == 0 && std::time::Instant::now() < until {
            thread::yield_now();
        }
        assert_eq!(executor.stats().completed, 1);
    }

    #[test]
    fn panicked_job_isolated_and_invokes_failure_callback() {
        let executor = Executor::with_limits(1, 2).unwrap();
        let (tx, rx) = channel::channel();
        executor
            .try_submit_with_panic(
                "test",
                || panic!("fixture background panic"),
                move || tx.send("panicked").unwrap(),
            )
            .unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), "panicked");
        let until = std::time::Instant::now() + Duration::from_secs(1);
        while executor.stats().panicked == 0 && std::time::Instant::now() < until {
            thread::yield_now();
        }
        assert_eq!(executor.stats().panicked, 1);
    }

    #[test]
    fn queue_full_is_nonblocking_and_explicit() {
        let executor = Executor::with_limits(1, 1).unwrap();
        let (gate_tx, gate_rx) = channel::sync_channel::<()>(0);
        executor
            .try_submit("test", move || { let _ = gate_rx.recv(); })
            .unwrap();
        // Give the worker a chance to consume the first task, then occupy queue.
        std::thread::sleep(Duration::from_millis(20));
        let (done_tx, _done_rx) = channel::channel::<()>();
        executor.try_submit("test", move || { let _ = done_tx.send(()); }).unwrap();
        let start = std::time::Instant::now();
        let error = executor.try_submit("test", || {}).unwrap_err();
        assert!(start.elapsed() < Duration::from_millis(50));
        assert!(error.to_string().contains("queue full"));
        drop(gate_tx);
    }
}
