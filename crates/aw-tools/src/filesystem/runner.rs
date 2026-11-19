//! Shared post-launch process contract for Graph-owned ShellNodes.
//!
//! Hosts retain process-launch policy, OS handles and reattachment custody.
//! After launch they report only these bounded facts to the common Job runtime,
//! which owns status classification. Output truncation is observability only;
//! an unproven stop or incomplete reader/drain state is `unknown`.
use super::{batch::Status, status::Primary};
use crate::execution_stop::StopReason;

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
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    /// False means the host could not prove both output readers/writers
    /// quiesced after the process tree settlement attempt.
    pub complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessTerminal {
    pub exit: ProcessExit,
    /// Semantic reason AW proved it stopped the process, if any.
    pub stop: Option<StopReason>,
    pub output: ProcessOutputMeta,
}

impl ProcessTerminal {
    pub fn status(self) -> Status {
        let primary = if !self.output.complete {
            Primary::Unknown
        } else if self.stop.is_some() {
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
            incomplete: false,
        }
    }

    pub fn status_detail(self) -> String {
        let mut fields = vec![match self.exit {
            ProcessExit::Code(code) => format!("exit={code}"),
            ProcessExit::Signal(signal) => format!("exit=signal:{signal}"),
            ProcessExit::Unknown => "exit=unknown".into(),
        }];
        if self.output.stdout_truncated {
            fields.push("stdout_truncated=true".into());
        }
        if self.output.stderr_truncated {
            fields.push("stderr_truncated=true".into());
        }
        if let Some(stop) = self.stop {
            fields.push(format!("stop={}", stop.as_str()));
        }
        if !self.output.complete {
            fields.push("output_complete=false".into());
        }
        fields.join(" ")
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
            .stop
            .map(|reason| format!(" stop={}", reason.as_str()))
            .unwrap_or_default();
        format!(
            "{} {}{} stdout={} stderr={} stdout_truncated={} stderr_truncated={} output_complete={}",
            self.status().primary.as_str(),
            exit,
            stop,
            self.output.stdout_bytes,
            self.output.stderr_bytes,
            self.output.stdout_truncated,
            self.output.stderr_truncated,
            self.output.complete
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
            complete,
        }
    }

    #[test]
    fn truncation_is_observability_not_graph_incomplete() {
        let terminal = ProcessTerminal {
            exit: ProcessExit::Code(0),
            stop: None,
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
    }

    #[test]
    fn known_nonzero_and_signal_are_fail() {
        for exit in [ProcessExit::Code(9), ProcessExit::Signal(9)] {
            assert_eq!(
                ProcessTerminal {
                    exit,
                    stop: None,
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
            output: output(true, false),
        };
        assert!(signal.status_detail().contains("exit=signal:11"));
        assert!(signal.compact_result().contains("exit=signal:11"));
    }

    #[test]
    fn unknown_stop_or_unquiesced_output_is_unknown() {
        assert_eq!(
            StopProof::Unknown(output(true, false))
                .terminal()
                .status()
                .primary,
            Primary::Unknown
        );
        assert_eq!(
            ProcessTerminal {
                exit: ProcessExit::Code(0),
                stop: None,
                output: output(false, false),
            }
            .status()
            .primary,
            Primary::Unknown
        );
    }

    #[test]
    fn proven_stop_is_failure_even_when_physical_exit_is_unknown() {
        let terminal = ProcessTerminal {
            exit: ProcessExit::Unknown,
            stop: Some(StopReason::Timeout),
            output: output(true, false),
        };
        assert_eq!(terminal.status().primary, Primary::Fail);
        assert!(terminal.status_detail().contains("stop=timeout"));
        assert!(terminal.compact_result().contains("stop=timeout"));

        let uncertain_output = ProcessTerminal {
            output: output(false, false),
            ..terminal
        };
        assert_eq!(uncertain_output.status().primary, Primary::Unknown);
    }
}
