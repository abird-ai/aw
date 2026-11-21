use super::{Error, Result, SpawnControl};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Mutex,
    },
};
use tokio::{
    process::{Child, Command},
    runtime::Handle,
    sync::{mpsc, oneshot},
    time::sleep_until,
};

const SPAWN_QUEUE: usize = 64;

pub(crate) struct Spawned {
    // Scope first so its containment Drop runs before Child's kill-on-drop.
    pub(crate) platform: aw_platform::DirectScope,
    pub(crate) child: Child,
    pub(crate) pid: u32,
}

struct Request {
    command: Command,
    runtime: Handle,
    control: Option<LaneControl>,
    gate: Arc<LaunchGate>,
    reply: oneshot::Sender<Result<Spawned>>,
}

struct LaneControl {
    deadline: tokio::time::Instant,
    cancelled: Box<dyn Fn() -> bool + Send + 'static>,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LaunchState {
    Pending,
    Cancelled,
    TimedOut,
    Launching,
}

struct LaunchGate(AtomicU8);

impl LaunchGate {
    fn new() -> Self {
        Self(AtomicU8::new(LaunchState::Pending as u8))
    }

    fn state(raw: u8) -> LaunchState {
        match raw {
            value if value == LaunchState::Pending as u8 => LaunchState::Pending,
            value if value == LaunchState::Cancelled as u8 => LaunchState::Cancelled,
            value if value == LaunchState::TimedOut as u8 => LaunchState::TimedOut,
            value if value == LaunchState::Launching as u8 => LaunchState::Launching,
            _ => unreachable!("invalid Linux spawn launch state"),
        }
    }

    fn transition(&self, next: LaunchState) -> LaunchState {
        match self.0.compare_exchange(
            LaunchState::Pending as u8,
            next as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => next,
            Err(current) => Self::state(current),
        }
    }
}

fn pre_spawn_result(state: LaunchState) -> Option<Result<Spawned>> {
    match state {
        LaunchState::Cancelled => Some(Err(Error::CancelledBeforeSpawn)),
        LaunchState::TimedOut => Some(Err(Error::TimedOutBeforeSpawn)),
        LaunchState::Pending | LaunchState::Launching => None,
    }
}

#[derive(Clone)]
struct Facility {
    sender: mpsc::Sender<Request>,
    poisoned: Arc<AtomicBool>,
}

static FACILITY: Mutex<Option<Facility>> = Mutex::new(None);

fn facility() -> Result<Facility> {
    let mut slot = FACILITY.lock().map_err(|_| {
        Error::SetupUncertain("Linux spawn facility lock is poisoned; restart required")
    })?;
    if let Some(facility) = slot.as_ref() {
        return Ok(facility.clone());
    }

    let (sender, mut receiver) = mpsc::channel::<Request>(SPAWN_QUEUE);
    let poisoned = Arc::new(AtomicBool::new(false));
    let lane_poisoned = poisoned.clone();
    std::thread::Builder::new()
        .name("aw-process-spawn".into())
        .spawn(move || {
            while let Some(request) = receiver.blocking_recv() {
                if lane_poisoned.load(Ordering::Acquire) {
                    let _ = request.reply.send(Err(Error::SetupUncertain(
                        "Linux spawn facility is poisoned; restart required",
                    )));
                    continue;
                }
                let result = catch_unwind(AssertUnwindSafe(|| {
                    launch(
                        request.command,
                        request.runtime,
                        request.control,
                        request.gate,
                        &request.reply,
                    )
                }));
                let result = match result {
                    Ok(result) => result,
                    Err(_) => {
                        lane_poisoned.store(true, Ordering::Release);
                        Err(Error::SetupUncertain(
                            "Linux spawn facility panicked; restart required",
                        ))
                    }
                };
                // If the caller disappeared after launch, dropping the returned
                // Spawned package tears down its exact scope/child.
                let _ = request.reply.send(result);
            }
        })
        .map_err(Error::Io)?;

    let facility = Facility { sender, poisoned };
    *slot = Some(facility.clone());
    Ok(facility)
}

fn launch(
    mut command: Command,
    runtime: Handle,
    control: Option<LaneControl>,
    gate: Arc<LaunchGate>,
    reply: &oneshot::Sender<Result<Spawned>>,
) -> Result<Spawned> {
    if reply.is_closed() {
        return Err(Error::OwnerGone(
            "process spawn caller disappeared before launch",
        ));
    }
    if let Some(control) = control.as_ref() {
        if (control.cancelled)() {
            if let Some(result) = pre_spawn_result(gate.transition(LaunchState::Cancelled)) {
                return result;
            }
        }
        if tokio::time::Instant::now() >= control.deadline {
            if let Some(result) = pre_spawn_result(gate.transition(LaunchState::TimedOut)) {
                return result;
            }
        }
    }
    if let Some(result) = pre_spawn_result(gate.transition(LaunchState::Launching)) {
        return result;
    }

    // This call is the parent-lifetime critical section. The lane enters the
    // request's runtime only so Tokio can register the Child with its reactor.
    let _runtime = runtime.enter();
    let mut child = command.spawn()?;
    let Some(pid) = child.id() else {
        let _ = child.start_kill();
        return Err(Error::SetupUncertain(
            "process launched but PID/setup outcome is uncertain",
        ));
    };
    let platform = match aw_platform::DirectScope::attach(pid) {
        Ok(scope) => scope,
        Err(_) => {
            let _ = child.start_kill();
            return Err(Error::SetupUncertain(
                "process launched but Process scope setup is uncertain",
            ));
        }
    };
    Ok(Spawned {
        platform,
        child,
        pid,
    })
}

pub(crate) async fn spawn(command: Command, control: Option<SpawnControl>) -> Result<Spawned> {
    let facility = facility()?;
    if facility.poisoned.load(Ordering::Acquire) {
        return Err(Error::SetupUncertain(
            "Linux spawn facility is poisoned; restart required",
        ));
    }
    if let Some(control) = control.as_ref() {
        control.check()?;
    }
    let runtime = Handle::try_current().map_err(|_| {
        Error::SetupUncertain("Linux Process spawn requires a current Tokio runtime")
    })?;
    let gate = Arc::new(LaunchGate::new());
    let (lane_control, mut async_control) = match control {
        Some(SpawnControl {
            deadline,
            cancelled,
            cancelled_signal,
        }) => (
            Some(LaneControl {
                deadline,
                cancelled,
            }),
            Some((deadline, cancelled_signal)),
        ),
        None => (None, None),
    };
    let (reply, result) = oneshot::channel();
    let permit = if let Some((deadline, cancelled_signal)) = async_control.as_mut() {
        let deadline = *deadline;
        let deadline_wait = sleep_until(deadline);
        tokio::pin!(deadline_wait);
        tokio::select! {
            biased;
            _ = cancelled_signal.as_mut() => {
                if let Some(result) = pre_spawn_result(gate.transition(LaunchState::Cancelled)) {
                    return result;
                }
                unreachable!("launch cannot begin before bounded queue admission");
            }
            _ = &mut deadline_wait => {
                if let Some(result) = pre_spawn_result(gate.transition(LaunchState::TimedOut)) {
                    return result;
                }
                unreachable!("launch cannot begin before bounded queue admission");
            }
            permit = facility.sender.reserve() => permit.map_err(|_| {
                Error::SetupUncertain("Linux spawn facility stopped; restart required")
            })?,
        }
    } else {
        facility
            .sender
            .reserve()
            .await
            .map_err(|_| Error::SetupUncertain("Linux spawn facility stopped; restart required"))?
    };
    permit.send(Request {
        command,
        runtime,
        control: lane_control,
        gate: gate.clone(),
        reply,
    });

    let mut result = result;
    let Some((deadline, cancelled_signal)) = async_control.as_mut() else {
        return result.await.map_err(|_| {
            Error::SetupUncertain("Linux spawn outcome is uncertain; restart required")
        })?;
    };
    let deadline_wait = sleep_until(*deadline);
    tokio::pin!(deadline_wait);
    tokio::select! {
        biased;
        _ = cancelled_signal.as_mut() => {
            if let Some(result) = pre_spawn_result(gate.transition(LaunchState::Cancelled)) {
                return result;
            }
        }
        _ = &mut deadline_wait => {
            if let Some(result) = pre_spawn_result(gate.transition(LaunchState::TimedOut)) {
                return result;
            }
        }
        reply = &mut result => {
            return reply.map_err(|_| {
                Error::SetupUncertain("Linux spawn outcome is uncertain; restart required")
            })?;
        }
    }
    result
        .await
        .map_err(|_| Error::SetupUncertain("Linux spawn outcome is uncertain; restart required"))?
}
