//! Restore an empty checkpoint into an explicitly selected, existing private
//! source-free baseline. The Store owns history, Agent/Sandbox publication
//! and replay. No directory creation, source materialization or activation.
use aw_agent_core::{Error, PrincipalId, Result};
use aw_store::{
    CheckpointLifecycle, CheckpointRestoreRequest, CheckpointRestoreResult, SessionBinding,
    SessionStore,
};
use std::path::PathBuf;

/// Trusted host policy for one Project's empty baseline. The host must
/// independently exclude all other access to the destination and its
/// ancestors (including physical aliases, mounts and non-cooperating
/// writers) throughout publication. Path selection, permissions and pins
/// are observations, not physical isolation or a new source grant.
pub struct EmptyCheckpointRestore {
    store: SessionStore,
    actor: PrincipalId,
    project: String,
    template: SessionBinding,
    root: PathBuf,
}

impl EmptyCheckpointRestore {
    /// Explicit host selection, never a model-supplied path or a layout
    /// reconstructed from the checkpoint. Do not inspect the directory
    /// here: an exact replay must work even after the layout is consumed.
    pub fn from_trusted_host(
        store: SessionStore,
        actor: PrincipalId,
        project: String,
        template: SessionBinding,
        root: PathBuf,
    ) -> Self {
        Self {
            store,
            actor,
            project,
            template,
            root,
        }
    }

    fn require_project(&self, project: &str) -> Result<()> {
        if project != self.project {
            return Err(Error::Denied(
                "cross-Project checkpoint restore unavailable",
            ));
        }
        Ok(())
    }

    fn pin_empty_root(&self) -> Result<aw_platform::DirectoryIdentity> {
        let root = &self.root;
        if !aw_platform::secure_private_files_supported()
            || !root.is_absolute()
            || root.parent().is_none()
            || root.canonicalize()? != *root
        {
            return Err(Error::Denied(
                "restore requires a canonical private baseline directory",
            ));
        }
        aw_platform::validate_private_dir(root)?;
        let pin = aw_platform::DirectoryIdentity::open(root)?;
        let identity = pin.identity()?;
        let store_directory = self
            .store
            .directory()
            .ok_or(Error::Denied("restore requires durable Store state"))?;
        // Do not let this explicit baseline selection overlap the current
        // host's own state or execution layout. The Store also denies known
        // occupied layouts in the publication transaction. Other physical
        // aliases and external writers remain the trusted host's boundary.
        for protected in [
            store_directory,
            self.template.tool_root.as_path(),
            self.template.working_directory.as_path(),
        ] {
            let protected = protected.canonicalize()?;
            if root.starts_with(&protected)
                || protected.starts_with(root)
                || aw_platform::DirectoryIdentity::open(&protected)?.identity()? == identity
            {
                return Err(Error::Denied(
                    "restore baseline overlaps protected host layout",
                ));
            }
        }
        if std::fs::read_dir(root)?.next().transpose()?.is_some() || !pin.is_current(root)? {
            return Err(Error::Conflict("restore baseline is not empty or changed"));
        }
        Ok(pin)
    }

    /// Restore without a task; task-bearing and captured-source restores
    /// require their original additional authority/activation path. Replay
    /// before any current layout inspection or new admission. The caller
    /// supplies its stable authenticated request identity.
    pub fn restore(
        &self,
        request: &CheckpointRestoreRequest,
        request_id: &str,
    ) -> Result<CheckpointRestoreResult> {
        if let Some(result) =
            self.store
                .checkpoint_replay_restore(&self.actor, request, request_id)?
        {
            self.require_project(&result.agent.project_ref)?;
            return Ok(result);
        }
        if request.task.is_some() {
            return Err(Error::Denied(
                "task-bearing checkpoint restore unavailable on empty baseline port",
            ));
        }
        let checkpoint = self
            .store
            .checkpoint_get_metadata(&self.actor, &request.checkpoint)?;
        let (source_project, _) = checkpoint
            .source_agent_ref
            .rsplit_once("/a:")
            .ok_or(Error::Storage("checkpoint source Agent identity invalid"))?;
        self.require_project(source_project)?;
        self.require_project(request.project.as_deref().unwrap_or(source_project))?;
        if checkpoint.lifecycle != CheckpointLifecycle::Live || !checkpoint.sources.is_empty() {
            return Err(Error::Conflict(
                "restore requires a live checkpoint with no captured sources",
            ));
        }
        let pin = self.pin_empty_root()?;
        let mut binding = self.template.clone();
        binding.tool_root = self.root.clone();
        binding.working_directory = self.root.clone();
        // Retain the observation through the original atomic baseline
        // publication. This path never activates the new Agent or queues work.
        let result = self.store.checkpoint_restore_baseline(
            &self.actor,
            request,
            &binding,
            &[],
            request_id,
            "local",
        );
        drop(pin);
        let result = result?;
        self.require_project(&result.agent.project_ref)?;
        Ok(result)
    }
}
