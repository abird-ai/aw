//! Shared Process facts for Graph-owned ShellNodes.
//!
//! Hosts retain launch policy while the shared Process runtime owns the OS
//! lifetime and reports only these bounded facts to the common Job runtime,
//! which owns status classification. Output truncation is observability only;
//! incomplete reader/drain state is an independent `Status::incomplete` fact
//! and never erases an otherwise known physical/semantic process outcome.
use super::{batch::Status, status::Primary};
use crate::execution_stop::{StopReason, StopRecord};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Occupancy {
    Empty,
    Live,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessExit {
    Code(i32),
    /// Signal that ended the waited process. This is physical exit evidence,
    /// not a claim about who sent it or why.
    Signal(i32),
    /// Exit/stop/reattachment could not be proven.
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcessOutputMeta {
    /// Bytes observed by AW. Exact produced bytes are known only when the stream is complete.
    pub stdout_bytes: u64,
    /// Bytes observed by AW. Exact produced bytes are known only when the stream is complete.
    pub stderr_bytes: u64,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub stdout_complete: bool,
    pub stderr_complete: bool,
}

impl ProcessOutputMeta {
    pub const fn complete(&self) -> bool {
        self.stdout_complete && self.stderr_complete
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessTerminal {
    pub exit: ProcessExit,
    /// Final factual semantic/housekeeping stop record, if any.
    pub stop: Option<StopRecord>,
    /// Live contained work observed before final cleanup.
    pub occupancy: Occupancy,
    pub output: ProcessOutputMeta,
}

impl ProcessTerminal {
    pub const fn stop_reason(self) -> Option<StopReason> {
        match self.stop {
            Some(record) => record.terminal_reason(),
            None => None,
        }
    }

    pub fn status(self) -> Status {
        let primary = if self.stop_reason().is_some() {
            Primary::Fail
        } else {
            match self.exit {
                ProcessExit::Code(0) => Primary::Ok,
                ProcessExit::Code(_) | ProcessExit::Signal(_) => Primary::Fail,
                ProcessExit::Unknown => Primary::Unknown,
            }
        };
        Status {
            primary,
            // Retained stdout/stderr truncation is observability only.
            more: false,
            incomplete: !self.output.complete()
                || self.stop.is_some_and(|record| {
                    record.evidence == crate::execution_stop::StopEvidence::Unknown
                }),
        }
    }

    /// Compact Graph status detail. Stream lists contain exactly the
    /// affected streams; an omitted field means neither is affected.
    /// Full byte counts and booleans remain in the typed terminal and
    /// dedicated ShellNode result.
    pub fn status_detail(self) -> String {
        use std::fmt::Write as _;
        let mut detail = match self.exit {
            ProcessExit::Code(code) => format!("exit={code}"),
            ProcessExit::Signal(signal) => format!("exit=signal:{signal}"),
            ProcessExit::Unknown => "exit=unknown".into(),
        };
        if let Some(reason) = self.stop_reason() {
            write!(detail, " stop={}", reason.as_str()).expect("String write");
        }
        for (field, stdout, stderr) in [
            (
                "truncated",
                self.output.stdout_truncated,
                self.output.stderr_truncated,
            ),
            (
                "incomplete",
                !self.output.stdout_complete,
                !self.output.stderr_complete,
            ),
        ] {
            let streams = match (stdout, stderr) {
                (true, true) => "stdout,stderr",
                (true, false) => "stdout",
                (false, true) => "stderr",
                (false, false) => continue,
            };
            write!(detail, " {field}={streams}").expect("String write");
        }
        detail
    }

    /// Compact ShellNode `result` metadata. stdout/stderr bytes themselves
    /// remain only in their dedicated retained streams.
    pub fn compact_result(self) -> String {
        let exit = match self.exit {
            ProcessExit::Code(code) => format!("exit={code}"),
            ProcessExit::Signal(signal) => format!("exit=signal:{signal}"),
            ProcessExit::Unknown => "exit=unknown".into(),
        };
        let stop = self
            .stop_reason()
            .map(|reason| format!(" stop={}", reason.as_str()))
            .unwrap_or_default();
        format!(
            "{} {}{} stdout={} stderr={} stdout_truncated={} stderr_truncated={} stdout_complete={} stderr_complete={}",
            self.status().primary.as_str(),
            exit,
            stop,
            self.output.stdout_bytes,
            self.output.stderr_bytes,
            self.output.stdout_truncated,
            self.output.stderr_truncated,
            self.output.stdout_complete,
            self.output.stderr_complete
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessObservation {
    Running {
        pid: u32,
    },
    Terminal(ProcessTerminal),
    /// The host has an execution identity but cannot prove whether it is still
    /// running or terminal. The node must not be relaunched.
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopProof {
    Confirmed(ProcessTerminal),
    /// A termination/reattachment attempt happened but did not prove stop.
    Unknown(ProcessOutputMeta),
}

impl StopProof {
    pub fn terminal(self) -> ProcessTerminal {
        match self {
            Self::Confirmed(terminal) => terminal,
            Self::Unknown(output) => ProcessTerminal {
                exit: ProcessExit::Unknown,
                stop: None,
                occupancy: Occupancy::Unknown,
                output,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(complete: bool, truncated: bool) -> ProcessOutputMeta {
        ProcessOutputMeta {
            stdout_bytes: 100,
            stderr_bytes: 7,
            stdout_truncated: truncated,
            stderr_truncated: false,
            stdout_complete: complete,
            stderr_complete: complete,
        }
    }

    #[test]
    fn truncation_is_observability_not_graph_incomplete() {
        let terminal = ProcessTerminal {
            exit: ProcessExit::Code(0),
            stop: None,
            occupancy: Occupancy::Empty,
            output: output(true, true),
        };
        assert_eq!(
            terminal.status(),
            Status {
                primary: Primary::Ok,
                more: false,
                incomplete: false,
            }
        );
        let compact = terminal.compact_result();
        assert!(compact.contains("stdout_truncated=true"));
        assert!(!compact.contains("incomplete"));
        assert_eq!(terminal.status_detail(), "exit=0 truncated=stdout");
        let terminal = ProcessTerminal {
            output: ProcessOutputMeta {
                stdout_truncated: false,
                stderr_truncated: true,
                stdout_complete: false,
                stderr_complete: true,
                ..terminal.output
            },
            ..terminal
        };
        assert_eq!(
            terminal.status_detail(),
            "exit=0 truncated=stderr incomplete=stdout"
        );
    }

    #[test]
    fn known_nonzero_and_signal_are_fail() {
        for exit in [ProcessExit::Code(9), ProcessExit::Signal(9)] {
            assert_eq!(
                ProcessTerminal {
                    exit,
                    stop: None,
                    occupancy: Occupancy::Empty,
                    output: output(true, false),
                }
                .status()
                .primary,
                Primary::Fail
            );
        }
        let signal = ProcessTerminal {
            exit: ProcessExit::Signal(11),
            stop: None,
            occupancy: Occupancy::Empty,
            output: output(true, false),
        };
        assert!(signal.status_detail().contains("exit=signal:11"));
        assert!(signal.compact_result().contains("exit=signal:11"));
    }

    #[test]
    fn unknown_exit_is_unknown_and_unquiesced_output_is_independently_incomplete() {
        assert_eq!(
            StopProof::Unknown(output(true, false))
                .terminal()
                .status()
                .primary,
            Primary::Unknown
        );
        for (exit, expected) in [
            (ProcessExit::Code(0), Primary::Ok),
            (ProcessExit::Code(9), Primary::Fail),
            (ProcessExit::Signal(9), Primary::Fail),
            (ProcessExit::Unknown, Primary::Unknown),
        ] {
            let status = ProcessTerminal {
                exit,
                stop: None,
                occupancy: Occupancy::Unknown,
                output: output(false, false),
            }
            .status();
            assert_eq!(status.primary, expected);
            assert!(status.incomplete);
            assert!(!status.more);
        }
    }

    #[test]
    fn proven_stop_is_failure_even_when_physical_exit_is_unknown() {
        let terminal = ProcessTerminal {
            exit: ProcessExit::Unknown,
            stop: Some(StopRecord::stopped(StopReason::Timeout)),
            occupancy: Occupancy::Empty,
            output: output(true, false),
        };
        assert_eq!(terminal.status().primary, Primary::Fail);
        assert!(terminal.status_detail().contains("stop=timeout"));
        assert!(terminal.compact_result().contains("stop=timeout"));

        let uncertain_output = ProcessTerminal {
            output: output(false, false),
            ..terminal
        };
        let status = uncertain_output.status();
        assert_eq!(status.primary, Primary::Fail);
        assert!(status.incomplete);
    }
}
