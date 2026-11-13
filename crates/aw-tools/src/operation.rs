//! Canonical semantic operation identity. Provider/model spellings are a compact
//! projection; policy, replay and SDK code use stable semantic IDs.

use crate::filesystem::schema::Tool as FsTool;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OperationId {
    Fs(FsTool),
    Bash,
    Pwsh,
    Powershell,
    Batch,
    Agent,
    JobsList,
    JobsGet,
    JobsRead,
    JobsAck,
    JobsSend,
    JobsCancel,
    JobsSubscribe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionClass {
    Native,
    Process,
    Graph,
    Agent,
    Control,
}

impl OperationId {
    pub const MODEL_SURFACE_MAX: usize = 25;

    pub fn from_model_name(name: &str) -> Option<Self> {
        if let Some(tool) = FsTool::from_name(name) {
            return Some(Self::Fs(tool));
        }
        Some(match name {
            "bash" => Self::Bash,
            "pwsh" => Self::Pwsh,
            "powershell" => Self::Powershell,
            "batch" => Self::Batch,
            "agent" => Self::Agent,
            "jobs_list" => Self::JobsList,
            "jobs_get" => Self::JobsGet,
            "jobs_read" => Self::JobsRead,
            "jobs_ack" => Self::JobsAck,
            "jobs_send" => Self::JobsSend,
            "jobs_cancel" => Self::JobsCancel,
            "jobs_subscribe" => Self::JobsSubscribe,
            _ => return None,
        })
    }

    pub fn from_semantic_name(name: &str) -> Option<Self> {
        if let Some(rest) = name.strip_prefix("fs.") {
            return FsTool::from_name(rest).map(Self::Fs);
        }
        Some(match name {
            "bash" => Self::Bash,
            "pwsh" => Self::Pwsh,
            "powershell" => Self::Powershell,
            "batch" => Self::Batch,
            "agent" => Self::Agent,
            "jobs.list" => Self::JobsList,
            "jobs.get" => Self::JobsGet,
            "jobs.read" => Self::JobsRead,
            "jobs.ack" => Self::JobsAck,
            "jobs.send" => Self::JobsSend,
            "jobs.cancel" => Self::JobsCancel,
            "jobs.subscribe" => Self::JobsSubscribe,
            _ => return None,
        })
    }

    pub fn model_name(self) -> &'static str {
        match self {
            Self::Fs(tool) => tool.name(),
            Self::Bash => "bash",
            Self::Pwsh => "pwsh",
            Self::Powershell => "powershell",
            Self::Batch => "batch",
            Self::Agent => "agent",
            Self::JobsList => "jobs_list",
            Self::JobsGet => "jobs_get",
            Self::JobsRead => "jobs_read",
            Self::JobsAck => "jobs_ack",
            Self::JobsSend => "jobs_send",
            Self::JobsCancel => "jobs_cancel",
            Self::JobsSubscribe => "jobs_subscribe",
        }
    }

    pub fn semantic_name(self) -> &'static str {
        match self {
            Self::Fs(FsTool::Ls) => "fs.ls",
            Self::Fs(FsTool::Find) => "fs.find",
            Self::Fs(FsTool::Grep) => "fs.grep",
            Self::Fs(FsTool::Read) => "fs.read",
            Self::Fs(FsTool::Write) => "fs.write",
            Self::Fs(FsTool::Edit) => "fs.edit",
            Self::Fs(FsTool::Patch) => "fs.patch",
            Self::Fs(FsTool::Copy) => "fs.cp",
            Self::Fs(FsTool::Move) => "fs.mv",
            Self::Fs(FsTool::Remove) => "fs.rm",
            Self::Fs(FsTool::Bread) => "fs.bread",
            Self::Fs(FsTool::Bwrite) => "fs.bwrite",
            Self::Fs(FsTool::Bedit) => "fs.bedit",
            Self::Bash => "bash",
            Self::Pwsh => "pwsh",
            Self::Powershell => "powershell",
            Self::Batch => "batch",
            Self::Agent => "agent",
            Self::JobsList => "jobs.list",
            Self::JobsGet => "jobs.get",
            Self::JobsRead => "jobs.read",
            Self::JobsAck => "jobs.ack",
            Self::JobsSend => "jobs.send",
            Self::JobsCancel => "jobs.cancel",
            Self::JobsSubscribe => "jobs.subscribe",
        }
    }

    pub const fn class(self) -> ExecutionClass {
        match self {
            Self::Fs(_) => ExecutionClass::Native,
            Self::Bash | Self::Pwsh | Self::Powershell => ExecutionClass::Process,
            Self::Batch => ExecutionClass::Graph,
            Self::Agent => ExecutionClass::Agent,
            Self::JobsList
            | Self::JobsGet
            | Self::JobsRead
            | Self::JobsAck
            | Self::JobsSend
            | Self::JobsCancel
            | Self::JobsSubscribe => ExecutionClass::Control,
        }
    }

    /// Provider-facing batch remains conservative: exact DL-150 filesystem
    /// Graph leaves plus the three prepared shell interpreters only.
    pub const fn provider_graph_eligible(self) -> bool {
        match self {
            Self::Fs(tool) => tool.graph_eligible(),
            Self::Bash | Self::Pwsh | Self::Powershell => true,
            _ => false,
        }
    }
}

/// Typed SDK vocabulary. These are semantic identities, not tool grants.
pub mod tools {
    use super::{FsTool, OperationId};
    use serde_json::Value;

    pub mod fs {
        use super::{FsTool, OperationId};
        pub const LS: OperationId = OperationId::Fs(FsTool::Ls);
        pub const FIND: OperationId = OperationId::Fs(FsTool::Find);
        pub const GREP: OperationId = OperationId::Fs(FsTool::Grep);
        pub const READ: OperationId = OperationId::Fs(FsTool::Read);
        pub const WRITE: OperationId = OperationId::Fs(FsTool::Write);
        pub const EDIT: OperationId = OperationId::Fs(FsTool::Edit);
        pub const PATCH: OperationId = OperationId::Fs(FsTool::Patch);
        pub const CP: OperationId = OperationId::Fs(FsTool::Copy);
        pub const MV: OperationId = OperationId::Fs(FsTool::Move);
        pub const RM: OperationId = OperationId::Fs(FsTool::Remove);
        pub const BREAD: OperationId = OperationId::Fs(FsTool::Bread);
        pub const BWRITE: OperationId = OperationId::Fs(FsTool::Bwrite);
        pub const BEDIT: OperationId = OperationId::Fs(FsTool::Bedit);
    }

    pub mod jobs {
        use super::OperationId;
        pub const LIST: OperationId = OperationId::JobsList;
        pub const GET: OperationId = OperationId::JobsGet;
        pub const READ: OperationId = OperationId::JobsRead;
        pub const ACK: OperationId = OperationId::JobsAck;
        pub const SEND: OperationId = OperationId::JobsSend;
        pub const CANCEL: OperationId = OperationId::JobsCancel;
        pub const SUBSCRIBE: OperationId = OperationId::JobsSubscribe;
    }

    pub const BASH: OperationId = OperationId::Bash;
    pub const PWSH: OperationId = OperationId::Pwsh;
    pub const POWERSHELL: OperationId = OperationId::Powershell;
    pub const BATCH: OperationId = OperationId::Batch;
    pub const AGENT: OperationId = OperationId::Agent;

    #[derive(Clone, Copy, Debug)]
    pub struct Call<'a> {
        pub operation: OperationId,
        pub arguments: &'a Value,
    }

    pub fn call(operation: OperationId, arguments: &Value) -> Call<'_> {
        Call {
            operation,
            arguments,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_and_semantic_names_share_one_bijective_resolver() {
        let mut count = 0;
        for tool in FsTool::ALL {
            let op = OperationId::Fs(tool);
            assert_eq!(OperationId::from_model_name(op.model_name()), Some(op));
            assert_eq!(
                OperationId::from_semantic_name(op.semantic_name()),
                Some(op)
            );
            count += 1;
        }
        for op in [
            OperationId::Bash,
            OperationId::Pwsh,
            OperationId::Powershell,
            OperationId::Batch,
            OperationId::Agent,
            OperationId::JobsList,
            OperationId::JobsGet,
            OperationId::JobsRead,
            OperationId::JobsAck,
            OperationId::JobsSend,
            OperationId::JobsCancel,
            OperationId::JobsSubscribe,
        ] {
            assert_eq!(OperationId::from_model_name(op.model_name()), Some(op));
            assert_eq!(
                OperationId::from_semantic_name(op.semantic_name()),
                Some(op)
            );
            count += 1;
        }
        assert_eq!(count, OperationId::MODEL_SURFACE_MAX);
        assert_eq!(OperationId::Bash.semantic_name(), "bash");
        assert_eq!(OperationId::Pwsh.semantic_name(), "pwsh");
        assert_eq!(OperationId::Powershell.semantic_name(), "powershell");
        assert!(OperationId::from_semantic_name("process.bash").is_none());
        assert!(OperationId::from_model_name("job_start").is_none());
        assert!(OperationId::from_model_name("subagent").is_none());
    }

    #[test]
    fn provider_graph_surface_stays_conservative() {
        assert!(OperationId::Fs(FsTool::Read).provider_graph_eligible());
        assert!(!OperationId::Fs(FsTool::Bread).provider_graph_eligible());
        assert!(OperationId::Bash.provider_graph_eligible());
        assert!(!OperationId::Batch.provider_graph_eligible());
        assert!(!OperationId::JobsCancel.provider_graph_eligible());
        assert!(!OperationId::Agent.provider_graph_eligible());
    }
}
