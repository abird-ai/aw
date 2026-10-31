//! Host-neutral bounded task queries/control. This is NOT a model tool or task
//! registry: the host owns durable storage, identity resolution, revocation,
//! journal CAS, and any actual cancellation effect.

use crate::{
    task::{TaskDisposition, MAX_RETAINED_TASKS},
    task_fence::TaskIdentity,
};
pub const MAX_TASK_LIST_PAGE: usize = 64;
pub const MAX_TASK_OUTPUT_PAGE: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskAction {
    Inspect,
    Output {
        stream: Stream,
        offset: u64,
        limit: usize,
    },
    Acknowledge {
        stream: Stream,
        through: u64,
    },
    RequestCancel,
}
#[derive(Clone, Debug)]
pub struct TaskInfo {
    pub binding: TaskIdentity,
    pub generation: u64,
    pub journal_sequence: u64,
    pub state: TaskDisposition,
    pub source_live: bool,
}
#[derive(Debug)]
pub enum TaskReply {
    Info(TaskInfo),
    Bytes(Vec<u8>),
    Acknowledged(bool),
    CancellationRequested(bool),
}
#[derive(Debug)]
pub enum TaskOpsError<E> {
    InvalidLimit,
    ScopeDenied,
    StaleGeneration,
    SourceRevoked,
    InvalidState,
    InconsistentHost,
    Host(E),
}
/// Existing host-owned task/session storage. No IDs are generated and every
/// method receives a host-verified TaskInfo from the same query.
pub trait HostTaskOperations {
    type Error;
    fn inspect(&self, binding: &TaskIdentity) -> Result<TaskInfo, Self::Error>;
    fn list(&self, host_scope: &TaskIdentity, limit: usize) -> Result<Vec<TaskInfo>, Self::Error>;
    fn output(
        &self,
        task: &TaskInfo,
        stream: Stream,
        offset: u64,
        limit: usize,
    ) -> Result<Vec<u8>, Self::Error>;
    fn acknowledge(
        &self,
        task: &TaskInfo,
        stream: Stream,
        through: u64,
    ) -> Result<bool, Self::Error>;
    fn request_cancel(&self, task: &TaskInfo) -> Result<bool, Self::Error>;
}
fn matches_scope(a: &TaskIdentity, b: &TaskIdentity) -> bool {
    a.actor == b.actor
        && a.source_kind == b.source_kind
        && a.source_id == b.source_id
        && a.source_epoch == b.source_epoch
        && a.execution_site == b.execution_site
        && a.session == b.session
        && a.branch == b.branch
}
fn checked<E>(
    snapshot: &TaskInfo,
    expected: &TaskIdentity,
    generation: u64,
) -> Result<(), TaskOpsError<E>> {
    if &snapshot.binding != expected || snapshot.journal_sequence == 0 {
        return Err(TaskOpsError::ScopeDenied);
    }
    if !snapshot.source_live {
        return Err(TaskOpsError::SourceRevoked);
    }
    if snapshot.generation != generation {
        return Err(TaskOpsError::StaleGeneration);
    }
    Ok(())
}
/// One bounded page, never a raw bulk listing of the host's persisted
/// task registry. The host must narrow the query at its database boundary.
pub fn list_bound<H: HostTaskOperations>(
    host: &H,
    host_scope: &TaskIdentity,
    limit: usize,
) -> Result<Vec<TaskInfo>, TaskOpsError<H::Error>> {
    if !(1..=MAX_TASK_LIST_PAGE).contains(&limit) {
        return Err(TaskOpsError::InvalidLimit);
    }
    let result = host.list(host_scope, limit).map_err(TaskOpsError::Host)?;
    let mut seen = std::collections::BTreeSet::new();
    if result
        .iter()
        .any(|row| !seen.insert(row.binding.fingerprint()))
    {
        return Err(TaskOpsError::InconsistentHost);
    }
    if result.len() > limit
        || result.len() > MAX_RETAINED_TASKS
        || result.iter().any(|row| {
            !matches_scope(&row.binding, host_scope)
                || row.journal_sequence == 0
                || !row.source_live
        })
    {
        return Err(TaskOpsError::InconsistentHost);
    }
    Ok(result)
}
pub fn run<H: HostTaskOperations>(
    host: &H,
    binding: &TaskIdentity,
    generation: u64,
    action: TaskAction,
) -> Result<TaskReply, TaskOpsError<H::Error>> {
    if let TaskAction::Output { offset, limit, .. } = action {
        if !(1..=MAX_TASK_OUTPUT_PAGE).contains(&limit)
            || offset.checked_add(limit as u64).is_none()
        {
            return Err(TaskOpsError::InvalidLimit);
        }
    }
    let task = host.inspect(binding).map_err(TaskOpsError::Host)?;
    checked(&task, binding, generation)?;
    match action {
        TaskAction::Inspect => Ok(TaskReply::Info(task)),
        TaskAction::Output {
            stream,
            offset,
            limit,
        } => {
            let bytes = host
                .output(&task, stream, offset, limit)
                .map_err(TaskOpsError::Host)?;
            if bytes.len() > limit {
                return Err(TaskOpsError::InconsistentHost);
            }
            Ok(TaskReply::Bytes(bytes))
        }
        TaskAction::Acknowledge { stream, through } => Ok(TaskReply::Acknowledged(
            host.acknowledge(&task, stream, through)
                .map_err(TaskOpsError::Host)?,
        )),
        TaskAction::RequestCancel => {
            if task.state.terminal() {
                return Err(TaskOpsError::InvalidState);
            }
            Ok(TaskReply::CancellationRequested(
                host.request_cancel(&task).map_err(TaskOpsError::Host)?,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolEffect;
    use std::sync::atomic::{AtomicUsize, Ordering};
    fn binding() -> TaskIdentity {
        TaskIdentity {
            actor: "a".into(),
            source_kind: "host".into(),
            source_id: "s".into(),
            source_epoch: 2,
            execution_site: "local".into(),
            session: "session".into(),
            branch: "main".into(),
            turn_id: 6,
            call_id: "x".into(),
            effect: ToolEffect::Process,
        }
    }
    struct Host {
        calls: AtomicUsize,
        foreign: bool,
        duplicate: bool,
    }
    impl HostTaskOperations for Host {
        type Error = ();
        fn inspect(&self, id: &TaskIdentity) -> Result<TaskInfo, ()> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(TaskInfo {
                binding: if self.foreign {
                    let mut wrong = id.clone();
                    wrong.source_epoch += 1;
                    wrong
                } else {
                    id.clone()
                },
                generation: 3,
                journal_sequence: 20,
                state: TaskDisposition::Running,
                source_live: true,
            })
        }
        fn list(&self, id: &TaskIdentity, _: usize) -> Result<Vec<TaskInfo>, ()> {
            let entry = self.inspect(id).unwrap();
            Ok(if self.duplicate {
                vec![entry.clone(), entry]
            } else {
                vec![entry]
            })
        }
        fn output(&self, _: &TaskInfo, _: Stream, _: u64, _: usize) -> Result<Vec<u8>, ()> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(b"hello".to_vec())
        }
        fn acknowledge(&self, _: &TaskInfo, _: Stream, _: u64) -> Result<bool, ()> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(true)
        }
        fn request_cancel(&self, _: &TaskInfo) -> Result<bool, ()> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(true)
        }
    }
    #[test]
    fn page_bounds_and_wrong_scope_rejected_before_effect() {
        let host = Host {
            calls: AtomicUsize::new(0),
            foreign: true,
            duplicate: false,
        };
        let id = binding();
        assert!(matches!(
            run(
                &host,
                &id,
                3,
                TaskAction::Output {
                    stream: Stream::Stdout,
                    offset: 0,
                    limit: MAX_TASK_OUTPUT_PAGE + 1
                }
            ),
            Err(TaskOpsError::InvalidLimit)
        ));
        assert_eq!(host.calls.load(Ordering::Relaxed), 0);
        assert!(matches!(
            run(
                &host,
                &id,
                3,
                TaskAction::Output {
                    stream: Stream::Stdout,
                    offset: 0,
                    limit: 2
                }
            ),
            Err(TaskOpsError::ScopeDenied)
        ));
        assert_eq!(host.calls.load(Ordering::Relaxed), 1);
        assert!(matches!(
            list_bound(&host, &id, 65),
            Err(TaskOpsError::InvalidLimit)
        ));
    }
    #[test]
    fn duplicate_task_list_entries_are_rejected() {
        let host = Host {
            calls: AtomicUsize::new(0),
            foreign: false,
            duplicate: true,
        };
        assert!(matches!(
            list_bound(&host, &binding(), 4),
            Err(TaskOpsError::InconsistentHost)
        ));
    }
    #[test]
    fn inspect_ack_cancel_use_exact_host_identity_and_generation() {
        let host = Host {
            calls: AtomicUsize::new(0),
            foreign: false,
            duplicate: false,
        };
        let id = binding();
        assert!(matches!(
            run(&host, &id, 2, TaskAction::RequestCancel),
            Err(TaskOpsError::StaleGeneration)
        ));
        let reply = run(
            &host,
            &id,
            3,
            TaskAction::Output {
                stream: Stream::Stdout,
                offset: 0,
                limit: 10,
            },
        )
        .unwrap();
        assert!(matches!(reply,TaskReply::Bytes(bytes) if bytes==b"hello"));
        assert!(matches!(
            run(
                &host,
                &id,
                3,
                TaskAction::Acknowledge {
                    stream: Stream::Stdout,
                    through: 5
                }
            )
            .unwrap(),
            TaskReply::Acknowledged(true)
        ));
        assert!(matches!(
            run(&host, &id, 3, TaskAction::RequestCancel).unwrap(),
            TaskReply::CancellationRequested(true)
        ));
        assert_eq!(list_bound(&host, &id, 5).unwrap().len(), 1);
    }
}
