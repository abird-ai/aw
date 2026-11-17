//! Compact model receipts from typed trusted facts. No execution, output
//! disclosure, owner delivery or clock occurs in this module.
use crate::toolset::ToolsetError;
use std::fmt::Write as _;

#[cfg(all(feature = "batch", feature = "durable"))]
use crate::filesystem::{
    job::{Lifecycle, NodeState},
    job_manager::Snapshot,
    runner::{ProcessExit, ProcessTerminal},
};

/// An ACK is either an explicit decision to finish retention, or proof of an
/// observed prefix. Rendering this receipt does not perform either effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AckMode {
    Finish,
    Through(u64),
}

/// An optional model handoff hint; only the host knows whether a returned
/// reference is the same durable Job that should be resumed, never re-run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextAction {
    ObserveJob,
}

/// Only trusted, typed, already-authorized facts may become a model receipt.
/// Raw file pages, tool arguments and arbitrary prose are deliberately absent.
#[derive(Clone, Copy, Debug)]
pub enum Receipt<'a> {
    #[cfg(all(feature = "batch", feature = "durable"))]
    JobStatus {
        reference: &'a str,
        snapshot: &'a Snapshot,
        terminal: Option<ProcessTerminal>,
        next: Option<NextAction>,
    },
    #[cfg(all(feature = "batch", feature = "durable"))]
    NodeStatus {
        reference: &'a str,
        tool: &'a str,
        revision: u64,
        state: NodeState,
        journal_unknown: bool,
        terminal: Option<ProcessTerminal>,
    },
    Ack {
        reference: &'a str,
        mode: AckMode,
        changed: bool,
        released: bool,
    },
}

fn validate_reference(reference: &str) -> Result<(), ToolsetError> {
    if reference.is_empty()
        || reference.len() > 2048
        || !reference.bytes().all(|c| c.is_ascii_graphic())
    {
        return Err(ToolsetError::Invalid("invalid compact receipt reference"));
    }
    Ok(())
}

#[cfg(all(feature = "batch", feature = "durable"))]
fn append_terminal(content: &mut String, terminal: ProcessTerminal) {
    content.push_str(" exit=");
    match terminal.exit {
        ProcessExit::Code(code) => write!(content, "{code}"),
        ProcessExit::Signal => content.write_str("signal"),
        ProcessExit::Cancelled => content.write_str("cancelled"),
        ProcessExit::DeadlineExceeded => content.write_str("deadline"),
        ProcessExit::Unknown => content.write_str("unknown"),
    }
    .expect("writing into a String never fails");
    write!(
        content,
        " out={} err={}",
        terminal.output.stdout_bytes, terminal.output.stderr_bytes
    )
    .expect("writing into a String never fails");
    if terminal.output.stdout_truncated {
        content.push_str(" out_truncated");
    }
    if terminal.output.stderr_truncated {
        content.push_str(" err_truncated");
    }
    if !terminal.output.complete {
        content.push_str(" output_incomplete");
    }
}

/// Build and check the factual body before any host-specific metadata is
/// appended. A whole status row must fit; never silently truncate its facts.
pub fn render_plain(receipt: Receipt<'_>, budget: usize) -> Result<String, ToolsetError> {
    let mut content = String::with_capacity(128.min(budget));
    match receipt {
        #[cfg(all(feature = "batch", feature = "durable"))]
        Receipt::JobStatus {
            reference,
            snapshot,
            terminal,
            next,
        } => {
            validate_reference(reference)?;
            if snapshot.label.is_empty()
                || snapshot.label.len() > 128
                || !snapshot.label.bytes().all(|c| c.is_ascii_graphic())
                || snapshot.finished > snapshot.total
                || snapshot.total == 0
            {
                return Err(ToolsetError::Invalid("invalid compact Job status"));
            }
            let state = if snapshot.journal_unknown {
                "unknown"
            } else {
                match snapshot.lifecycle {
                    Lifecycle::Running => "running",
                    Lifecycle::Cancelling | Lifecycle::Revoking => "cancelling",
                    Lifecycle::Terminal if snapshot.stop_reason.is_some() => "cancelled",
                    Lifecycle::Terminal => "settled",
                }
            };
            write!(
                content,
                "{reference} {} {state} rev={}",
                snapshot.label, snapshot.sequence
            )
            .expect("writing into a String never fails");
            if snapshot.total > 1 && snapshot.lifecycle != Lifecycle::Terminal {
                write!(content, " {}/{}", snapshot.finished, snapshot.total)
                    .expect("writing into a String never fails");
            }
            if terminal.is_some() && snapshot.lifecycle != Lifecycle::Terminal {
                return Err(ToolsetError::Invalid(
                    "process terminal facts require a terminal Job",
                ));
            }
            if snapshot.lifecycle == Lifecycle::Terminal {
                let outcome = snapshot
                    .summary
                    .map_or("unknown", |summary| summary.0.as_str());
                write!(content, " outcome={outcome}").expect("writing into a String never fails");
                if let Some((_, more, incomplete)) = snapshot.summary {
                    if more {
                        content.push_str(" more");
                    }
                    if incomplete {
                        content.push_str(" incomplete");
                    }
                }
            }
            if snapshot.journal_unknown {
                content.push_str(" journal=unknown");
            }
            if let Some(terminal) = terminal {
                append_terminal(&mut content, terminal);
            }
            if matches!(next, Some(NextAction::ObserveJob)) {
                content.push_str(" next=jobs_get|jobs_read no-rerun");
            }
        }
        #[cfg(all(feature = "batch", feature = "durable"))]
        Receipt::NodeStatus {
            reference,
            tool,
            revision,
            state,
            journal_unknown,
            terminal,
        } => {
            validate_reference(reference)?;
            if tool.is_empty() || tool.len() > 128 || !tool.bytes().all(|c| c.is_ascii_graphic()) {
                return Err(ToolsetError::Invalid("invalid Job node tool"));
            }
            if terminal.is_some() && !matches!(state, NodeState::Terminal { .. }) {
                return Err(ToolsetError::Invalid(
                    "process terminal facts require a settled node",
                ));
            }
            match state {
                NodeState::Pending => {
                    write!(content, "{reference} {tool} waiting rev={revision}")
                        .expect("writing into a String never fails");
                }
                NodeState::Claimed { .. } | NodeState::Running { .. } => {
                    write!(content, "{reference} {tool} running rev={revision}")
                        .expect("writing into a String never fails");
                }
                NodeState::Terminal { status, .. } => {
                    write!(
                        content,
                        "{reference} {tool} settled rev={revision} outcome={}",
                        status.primary.as_str()
                    )
                    .expect("writing into a String never fails");
                    if status.more {
                        content.push_str(" more");
                    }
                    if status.incomplete {
                        content.push_str(" incomplete");
                    }
                }
            }
            if journal_unknown {
                content.push_str(" journal=unknown");
            }
            if let Some(terminal) = terminal {
                append_terminal(&mut content, terminal);
            }
        }
        Receipt::Ack {
            reference,
            mode,
            changed,
            released,
        } => {
            validate_reference(reference)?;
            write!(
                content,
                "{reference} {} ack=",
                if changed { "ok" } else { "unchanged" }
            )
            .expect("writing into a String never fails");
            match mode {
                AckMode::Finish => content.push_str("all"),
                AckMode::Through(offset) => {
                    write!(content, "{offset}").expect("writing into a String never fails")
                }
            }
            write!(content, " released={released}").expect("writing into a String never fails");
        }
    }
    if content.len() > budget {
        return Err(ToolsetError::ResultLimit);
    }
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ack_is_a_bounded_factual_receipt_not_an_effect() {
        let finish = Receipt::Ack {
            reference: "j:7",
            mode: AckMode::Finish,
            changed: true,
            released: false,
        };
        assert_eq!(
            render_plain(finish, 64).unwrap(),
            "j:7 ok ack=all released=false"
        );
        assert_eq!(render_plain(finish, 5), Err(ToolsetError::ResultLimit));
        assert_eq!(
            render_plain(
                Receipt::Ack {
                    reference: "j:7",
                    mode: AckMode::Through(512),
                    changed: false,
                    released: true
                },
                64
            )
            .unwrap(),
            "j:7 unchanged ack=512 released=true"
        );
        assert!(render_plain(
            Receipt::Ack {
                reference: "j:7\nfalse",
                mode: AckMode::Finish,
                changed: true,
                released: false,
            },
            1024
        )
        .is_err());
    }

    #[cfg(all(feature = "durable", feature = "batch"))]
    #[test]
    fn typed_process_terminal_and_handoff_are_factual_and_bounded() {
        use crate::filesystem::{
            batch::Status,
            job::{Lifecycle, NodeState},
            runner::{ProcessExit, ProcessOutputMeta, ProcessTerminal},
            status::Primary,
        };
        let terminal = ProcessTerminal {
            exit: ProcessExit::Code(1),
            output: ProcessOutputMeta {
                stdout_bytes: 20_001,
                stderr_bytes: 732,
                stdout_truncated: true,
                stderr_truncated: false,
                complete: true,
            },
        };
        let snapshot = Snapshot {
            id: "job-7".into(),
            label: "bash".into(),
            lifecycle: Lifecycle::Terminal,
            stop_reason: None,
            journal_unknown: false,
            sequence: 4,
            finished: 1,
            total: 1,
            summary: Some((Primary::Fail, false, false)),
            detail: Some("not parsed exit=99999".into()),
        };
        let receipt = Receipt::JobStatus {
            reference: "j:7",
            snapshot: &snapshot,
            terminal: Some(terminal),
            next: Some(NextAction::ObserveJob),
        };
        let expected =
            "j:7 bash settled rev=4 outcome=fail exit=1 out=20001 err=732 out_truncated next=jobs_get|jobs_read no-rerun";
        assert_eq!(render_plain(receipt, expected.len()).unwrap(), expected);
        assert_eq!(
            render_plain(receipt, expected.len() - 1),
            Err(ToolsetError::ResultLimit)
        );
        let ordinary = Receipt::JobStatus {
            reference: "j:7",
            snapshot: &snapshot,
            terminal: Some(terminal),
            next: None,
        };
        assert!(!render_plain(ordinary, 200).unwrap().contains("next="));
        let unknown_terminal = ProcessTerminal {
            exit: ProcessExit::Unknown,
            output: ProcessOutputMeta {
                stdout_bytes: 0,
                stderr_bytes: 3,
                stdout_truncated: false,
                stderr_truncated: true,
                complete: false,
            },
        };
        let node = Receipt::NodeStatus {
            reference: "j:7/n:2",
            tool: "bash",
            revision: 4,
            state: NodeState::Terminal {
                sequence: 9,
                status: Status {
                    primary: Primary::Mixed,
                    more: true,
                    incomplete: true,
                },
            },
            journal_unknown: false,
            terminal: Some(unknown_terminal),
        };
        assert_eq!(render_plain(node, 512).unwrap(),
            "j:7/n:2 bash settled rev=4 outcome=mixed more incomplete exit=unknown out=0 err=3 err_truncated output_incomplete");
        let invalid = Receipt::NodeStatus {
            reference: "j:7/n:2",
            tool: "bash",
            revision: 4,
            state: NodeState::Running { sequence: 10 },
            journal_unknown: false,
            terminal: Some(unknown_terminal),
        };
        assert!(render_plain(invalid, 512).is_err());
        let mut running = snapshot;
        running.lifecycle = Lifecycle::Running;
        assert!(render_plain(
            Receipt::JobStatus {
                reference: "j:7",
                snapshot: &running,
                terminal: Some(terminal),
                next: None,
            },
            512
        )
        .is_err());
    }

    #[cfg(all(feature = "durable", feature = "batch"))]
    #[test]
    fn journal_unknown_receipts_cannot_imply_running_or_settled_execution() {
        use crate::filesystem::{job::Lifecycle, job::NodeState, job_manager::Snapshot};
        let snapshot = Snapshot {
            id: "unknown-1".into(),
            label: "bash".into(),
            lifecycle: Lifecycle::Running,
            stop_reason: None,
            journal_unknown: true,
            sequence: 3,
            finished: 0,
            total: 1,
            summary: None,
            detail: None,
        };
        assert_eq!(
            render_plain(
                Receipt::JobStatus {
                    reference: "j:unknown-1",
                    snapshot: &snapshot,
                    terminal: None,
                    next: None,
                },
                512
            )
            .unwrap(),
            "j:unknown-1 bash unknown rev=3 journal=unknown"
        );
        assert_eq!(
            render_plain(
                Receipt::NodeStatus {
                    reference: "j:unknown-1/n:1",
                    tool: "bash",
                    revision: 3,
                    state: NodeState::Running { sequence: 2 },
                    journal_unknown: true,
                    terminal: None,
                },
                512
            )
            .unwrap(),
            "j:unknown-1/n:1 bash running rev=3 journal=unknown"
        );
        assert_eq!(
            render_plain(
                Receipt::NodeStatus {
                    reference: "j:unknown-1/n:1",
                    tool: "bash",
                    revision: 3,
                    state: NodeState::Pending,
                    journal_unknown: true,
                    terminal: None,
                },
                512
            )
            .unwrap(),
            "j:unknown-1/n:1 bash waiting rev=3 journal=unknown"
        );
    }

    #[cfg(all(feature = "durable", feature = "batch"))]
    #[test]
    fn graph_node_receipt_preserves_exact_typed_state() {
        use crate::filesystem::{batch::Status, job::NodeState, status::Primary};
        let show = |state| {
            render_plain(
                Receipt::NodeStatus {
                    reference: "j:7/n:2",
                    tool: "bash",
                    revision: 9,
                    state,
                    journal_unknown: false,
                    terminal: None,
                },
                512,
            )
            .unwrap()
        };
        assert_eq!(show(NodeState::Pending), "j:7/n:2 bash waiting rev=9");
        assert_eq!(
            show(NodeState::Running { sequence: 3 }),
            "j:7/n:2 bash running rev=9"
        );
        let terminal = NodeState::Terminal {
            sequence: 4,
            status: Status {
                primary: Primary::Fail,
                more: true,
                incomplete: true,
            },
        };
        assert_eq!(
            show(terminal),
            "j:7/n:2 bash settled rev=9 outcome=fail more incomplete"
        );
        assert!(render_plain(
            Receipt::NodeStatus {
                reference: "j:7/n:2",
                tool: "bash\nspoof",
                revision: 9,
                state: terminal,
                journal_unknown: false,
                terminal: None,
            },
            512
        )
        .is_err());
    }

    #[cfg(all(feature = "durable", feature = "batch"))]
    #[test]
    fn job_status_uses_original_snapshot_without_reclassifying_outcome() {
        use crate::filesystem::{job::Lifecycle, status::Primary};
        let mut snapshot = Snapshot {
            id: "job-7".into(),
            label: "bash".into(),
            lifecycle: Lifecycle::Running,
            stop_reason: None,
            journal_unknown: false,
            sequence: 3,
            finished: 0,
            total: 1,
            summary: None,
            detail: None,
        };
        let render = |snapshot: &Snapshot| {
            render_plain(
                Receipt::JobStatus {
                    reference: "j:7",
                    snapshot,
                    terminal: None,
                    next: None,
                },
                512,
            )
            .unwrap()
        };
        assert_eq!(render(&snapshot), "j:7 bash running rev=3");
        snapshot.lifecycle = Lifecycle::Terminal;
        snapshot.sequence = 4;
        snapshot.finished = 1;
        snapshot.summary = Some((Primary::Mixed, true, true));
        assert_eq!(
            render(&snapshot),
            "j:7 bash settled rev=4 outcome=mixed more incomplete"
        );
        snapshot.summary = Some((Primary::Unknown, false, false));
        assert_eq!(render(&snapshot), "j:7 bash settled rev=4 outcome=unknown");
        snapshot.label = "bash\nspoof".into();
        assert!(render_plain(
            Receipt::JobStatus {
                reference: "j:7",
                snapshot: &snapshot,
                terminal: None,
                next: None,
            },
            512
        )
        .is_err());
        snapshot.label = "bash".into();
        assert!(render_plain(
            Receipt::JobStatus {
                reference: "j:7\nspoof",
                snapshot: &snapshot,
                terminal: None,
                next: None,
            },
            512
        )
        .is_err());
    }
}
