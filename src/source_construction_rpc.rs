//! Explicit local host construction capabilities. All materialization,
//! durable intent, recovery and publication remain with the existing
//! independently approved Git constructors. No request supplies paths.
use super::{
    agent_create_optional_string, agent_snapshot_value, bounded_source_metadata,
    checkpoint_restore_request, checkpoint_snapshot_value, sandbox_snapshot_value,
    StdioAgentStatePort, StdioSandboxPort,
};
use aw::headless::{CheckpointCreatePort, CheckpointRestorePort};
use aw::{Error, Result};
use aw_agent_core::SourceName;
use aw_store::{AgentHistoryForkAccess, CheckpointCaptureRequest, CheckpointCaptureSelection};
use serde_json::Value;
use std::{path::PathBuf, sync::Arc};

/// One original invocation namespace shared by stdio dispatch and explicit
/// offline inspection. An RPC receipt cannot be recreated by a fresh CLI run.
#[derive(Clone, Copy)]
pub(crate) enum Operation {
    SandboxCreate,
    SandboxDerive,
    CheckpointCapture,
    CheckpointRestore,
}

impl Operation {
    pub(crate) fn rpc_id(self, id: &str) -> String {
        let name = match self {
            Self::SandboxCreate | Self::SandboxDerive => "sandbox-create",
            Self::CheckpointCapture => "checkpoint",
            Self::CheckpointRestore => "checkpoint-restore",
        };
        format!("rpc:stdio:{name}:{id}")
    }

    pub(crate) fn offline_id(
        self,
        id: &str,
        rpc_request: bool,
        inspection: bool,
    ) -> Result<String> {
        if rpc_request {
            if !inspection {
                return Err(Error::Denied("RPC request namespace is recovery-only"));
            }
            return Ok(self.rpc_id(id));
        }
        let name = match self {
            Self::SandboxCreate => "sandbox-create",
            Self::SandboxDerive => "sandbox-derive",
            Self::CheckpointCapture => "checkpoint-capture",
            Self::CheckpointRestore => "checkpoint-restore",
        };
        Ok(format!("cli:{name}:{id}"))
    }
}

#[derive(clap::Args, Clone, Debug, Default)]
pub(crate) struct Approvals {
    /// Private exact retained-version Project baseline approval.
    #[arg(long = "sandbox-create-approval", requires = "rpc_actor_owner")]
    pub sandbox_create_approval: Option<PathBuf>,
    /// Private exact same-Project parent Sandbox derivation approval.
    #[arg(long = "sandbox-derive-approval", requires = "rpc_actor_owner")]
    pub sandbox_derive_approval: Option<PathBuf>,
    /// Private git-native.json directory independently authorizing Git capture.
    #[arg(long = "checkpoint-capture-root", requires = "rpc_actor_owner")]
    pub checkpoint_capture_root: Option<PathBuf>,
    /// Private exact captured-source restore approval (taskless, private).
    #[arg(
        long = "checkpoint-restore-approval",
        requires = "rpc_actor_owner",
        conflicts_with = "rpc_checkpoint_restore_root"
    )]
    pub checkpoint_restore_approval: Option<PathBuf>,
}

impl Approvals {
    pub fn sandbox_enabled(&self) -> bool {
        self.sandbox_create_approval.is_some() || self.sandbox_derive_approval.is_some()
    }

    pub fn is_empty(&self) -> bool {
        !self.sandbox_enabled()
            && self.checkpoint_capture_root.is_none()
            && self.checkpoint_restore_approval.is_none()
    }

    pub(super) fn create_sandbox(
        &self,
        state: &StdioSandboxPort,
        request_id: &str,
        params: Value,
    ) -> Result<Value> {
        let requested = agent_create_optional_string(&params, "project")?;
        let project = state.project(requested.as_deref())?;
        let from = agent_create_optional_string(&params, "from")?;
        let id = Operation::SandboxCreate.rpc_id(request_id);
        let snapshot = if let Some(parent) = from {
            if parent.rsplit_once("/s:").map(|(project, _)| project) != Some(project) {
                return Err(Error::Denied(
                    "same-Project canonical Sandbox parent required",
                ));
            }
            let path = self
                .sandbox_derive_approval
                .as_ref()
                .ok_or(Error::Denied("Sandbox derivation approval unavailable"))?;
            crate::sandbox_derive::derive(&state.store, &state.caller, path, &parent, &id, false)?
        } else {
            let path = self
                .sandbox_create_approval
                .as_ref()
                .ok_or(Error::Denied("Sandbox baseline approval unavailable"))?;
            crate::sandbox_create::create(&state.store, &state.caller, path, project, &id, false)?
        };
        state.require_current_project(&snapshot)?;
        bounded_source_metadata(sandbox_snapshot_value(&snapshot, true))
    }
}

pub(super) struct CapturePort {
    pub state: Arc<StdioAgentStatePort>,
    pub approval_root: PathBuf,
    pub history_enabled: bool,
}

impl CheckpointCreatePort for CapturePort {
    fn create(&self, request_id: &str, params: Value) -> Result<Value> {
        let selection =
            match params.get("sources") {
                Some(value) if value.as_str() == Some("none") => {
                    if !self.history_enabled {
                        return Err(Error::Denied(
                            "history-only checkpoint capability unavailable",
                        ));
                    }
                    return self.state.create(request_id, params);
                }
                None => CheckpointCaptureSelection::Auto,
                Some(value) if value.as_str() == Some("auto") => CheckpointCaptureSelection::Auto,
                Some(Value::Array(names)) => CheckpointCaptureSelection::Named(
                    names
                        .iter()
                        .map(|name| {
                            SourceName::new(name.as_str().ok_or(Error::Protocol(
                                "checkpoint source name must be a string",
                            ))?)
                        })
                        .collect::<Result<Vec<_>>>()?,
                ),
                _ => {
                    return Err(Error::Protocol(
                        "checkpoint sources must be auto, none, or a name array",
                    ))
                }
            };
        // Canonicalize the contextual selector against this connection;
        // changing context cannot silently redirect an original request.
        let reference = self.state.current_or_canonical_ref(&params, false)?;
        if reference.rsplit_once("/a:").map(|(project, _)| project)
            != Some(self.state.current_project_ref.as_str())
        {
            return Err(Error::Denied(
                "cross-Project checkpoint capture unavailable",
            ));
        }
        let note = agent_create_optional_string(&params, "note")?;
        let request = CheckpointCaptureRequest {
            reference: &reference,
            selection,
            note: note.as_deref(),
        };
        let id = Operation::CheckpointCapture.rpc_id(request_id);
        let snapshot = crate::checkpoint_capture::capture(
            &self.state.store,
            &self.state.caller,
            &self.approval_root,
            &request,
            &id,
            false,
        )?;
        if snapshot.source_agent_ref != reference {
            return Err(Error::Conflict(
                "checkpoint result differs from original Agent",
            ));
        }
        bounded_source_metadata(checkpoint_snapshot_value(&snapshot))
    }
}

pub(super) struct RestorePort {
    pub state: Arc<StdioAgentStatePort>,
    pub approval: PathBuf,
}

impl CheckpointRestorePort for RestorePort {
    fn restore(&self, request_id: &str, params: Value) -> Result<Value> {
        let request = checkpoint_restore_request(&params)?;
        if request.project.is_some()
            || request.task.is_some()
            || request.access != AgentHistoryForkAccess::Private
        {
            return Err(Error::Denied(
                "captured restore requires default Project, private access and no task",
            ));
        }
        let id = Operation::CheckpointRestore.rpc_id(request_id);
        let store = &self.state.store;
        let actor = &self.state.caller;
        // Exact applied replay needs neither current checkpoint nor approval
        // files, and stays scoped to the connection's Project.
        let result = match store.checkpoint_replay_restore(actor, &request, &id)? {
            Some(result) => result,
            None => {
                let checkpoint = store.checkpoint_get_metadata(actor, &request.checkpoint)?;
                if checkpoint
                    .source_agent_ref
                    .rsplit_once("/a:")
                    .map(|(project, _)| project)
                    != Some(self.state.current_project_ref.as_str())
                {
                    return Err(Error::Denied(
                        "cross-Project checkpoint restore unavailable",
                    ));
                }
                crate::checkpoint_restore::restore(
                    store,
                    actor,
                    &self.approval,
                    &request,
                    &id,
                    false,
                )?
            }
        };
        if result.agent.project_ref != self.state.current_project_ref {
            return Err(Error::Denied(
                "cross-Project checkpoint restore unavailable",
            ));
        }
        bounded_source_metadata(serde_json::json!({
            "agent": agent_snapshot_value(&result.agent),
            "sandbox": result.agent.sandbox_ref,
            "source_coverage": result.source_coverage,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_rpc_namespace_is_inspection_only_and_matches_original_route() {
        for (operation, prefix, cli) in [
            (Operation::SandboxCreate, "sandbox-create", "sandbox-create"),
            (Operation::SandboxDerive, "sandbox-create", "sandbox-derive"),
            (
                Operation::CheckpointCapture,
                "checkpoint",
                "checkpoint-capture",
            ),
            (
                Operation::CheckpointRestore,
                "checkpoint-restore",
                "checkpoint-restore",
            ),
        ] {
            assert!(operation.offline_id("original", true, false).is_err());
            assert_eq!(
                operation.offline_id("original", true, true).unwrap(),
                format!("rpc:stdio:{prefix}:original")
            );
            assert_eq!(
                operation.offline_id("original", false, false).unwrap(),
                format!("cli:{cli}:original")
            );
        }
    }
}
