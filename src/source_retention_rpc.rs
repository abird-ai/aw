//! Explicit resource-scoped maintenance. No logical close/delete, new
//! materialization, automatic GC or caller-selected physical authority.
use super::{
    agent_create_optional_string, bounded_source_metadata, sandbox_snapshot_value, StdioSandboxPort,
};
use aw::{headless::SourceRetention, Error, Result};
use aw_store::SessionBinding;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(clap::Args, Clone, Debug, Default)]
pub(crate) struct RetentionApprovals {
    /// Private exact retired checkpoint retention approval; not delete authority.
    #[arg(long = "checkpoint-retention-approval", requires = "rpc_actor_owner")]
    pub checkpoint_retention_approval: Option<PathBuf>,
    /// Private exact closed Sandbox retention approval; not close authority.
    #[arg(long = "sandbox-retention-approval", requires = "rpc_actor_owner")]
    pub sandbox_retention_approval: Option<PathBuf>,
}

impl RetentionApprovals {
    pub fn enabled(&self, target: SourceRetention) -> bool {
        self.path(target).is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.checkpoint_retention_approval.is_none() && self.sandbox_retention_approval.is_none()
    }

    fn path(&self, target: SourceRetention) -> Option<&Path> {
        match target {
            SourceRetention::Checkpoint => self.checkpoint_retention_approval.as_deref(),
            SourceRetention::Sandbox => self.sandbox_retention_approval.as_deref(),
        }
    }

    pub(super) fn reconcile(
        &self,
        target: SourceRetention,
        state: &StdioSandboxPort,
        layout: &SessionBinding,
        params: Value,
    ) -> Result<Value> {
        let approval = self
            .path(target)
            .ok_or(Error::Denied("retention approval unavailable"))?;
        // The controller's own live execution layout cannot be selected as
        // maintenance backing or destination. The independent host approval
        // still owns exclusion of every other physical writer/alias.
        let protected = [
            layout.tool_root.as_path(),
            layout.working_directory.as_path(),
        ];
        match target {
            SourceRetention::Checkpoint => {
                let id = agent_create_optional_string(&params, "id")?
                    .ok_or(Error::Protocol("retention requires exact checkpoint ID"))?;
                crate::checkpoint_retention::reconcile_configured(
                    &state.store,
                    &state.caller,
                    approval,
                    &state.current_project_ref,
                    &id,
                    &protected,
                )?;
                Ok(json!({"status":"settled"}))
            }
            SourceRetention::Sandbox => {
                let reference = agent_create_optional_string(&params, "ref")?
                    .ok_or(Error::Protocol("retention requires exact Sandbox ref"))?;
                let revision = params
                    .get("rev")
                    .and_then(Value::as_u64)
                    .filter(|rev| *rev != 0)
                    .ok_or(Error::Protocol(
                        "retention requires original closed Sandbox revision",
                    ))?;
                if reference.rsplit_once("/s:").map(|(p, _)| p)
                    != Some(state.current_project_ref.as_str())
                {
                    return Err(Error::Denied("cross-Project Sandbox retention unavailable"));
                }
                let approval = crate::sandbox_retention::Approval::open(approval, &state.caller)?;
                let config = approval.config();
                if config.sandbox != reference || config.sandbox_rev != revision {
                    return Err(Error::Denied(
                        "retention approval differs from exact closed Sandbox revision",
                    ));
                }
                let snapshot = approval.reconcile_protected(&state.store, &protected)?;
                state.require_current_project(&snapshot)?;
                bounded_source_metadata(sandbox_snapshot_value(&snapshot, true))
            }
        }
    }
}
