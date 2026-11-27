//! Restore an empty checkpoint into an explicitly selected, existing private
//! source-free baseline. The Store owns history, Agent/Sandbox publication
//! and replay. No directory creation, source materialization or activation.
use aw_agent_core::{Error, PrincipalId, Result};
use aw_store::{
    AgentHistoryForkAccess, CheckpointLifecycle, CheckpointRestoreRequest, CheckpointRestoreResult,
    SessionBinding, SessionStore,
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
    initial_task_allowed: bool,
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
            initial_task_allowed: true,
        }
    }

    /// A configured controller may narrow fresh task admission when its
    /// current template cannot be reconstructed for offline recovery.
    /// Applied replay remains independent of this new-effect gate.
    pub fn with_initial_task_allowed(mut self, allowed: bool) -> Self {
        self.initial_task_allowed = allowed;
        self
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

    fn require_task_request(&self, request: &CheckpointRestoreRequest) -> Result<()> {
        if request.task.is_none() || request.access != AgentHistoryForkAccess::Private {
            return Err(Error::Denied(
                "empty-baseline task restore requires private access and the original task",
            ));
        }
        if let Some(project) = request.project.as_deref() {
            self.require_project(project)?;
        }
        Ok(())
    }

    fn select_empty_layout(
        &self,
        request: &CheckpointRestoreRequest,
    ) -> Result<(SessionBinding, aw_platform::DirectoryIdentity)> {
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
        Ok((binding, pin))
    }

    /// The original baseline constructor, optionally with one encrypted
    /// initial Pending Input. No model call or Agent activation. Replay
    /// before current layout inspection or fresh effect admission.
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
            self.require_task_request(request)?;
            if !self.initial_task_allowed {
                return Err(Error::Denied(
                    "empty-baseline task restore requires the saved controller template",
                ));
            }
        }
        let (binding, pin) = self.select_empty_layout(request)?;
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

    /// Explicit inspection only: never recreate the task or layout.
    pub fn resume(
        &self,
        request: &CheckpointRestoreRequest,
        request_id: &str,
    ) -> Result<CheckpointRestoreResult> {
        self.require_task_request(request)?;
        if let Some(result) = self.store.checkpoint_replay_empty_baseline_task(
            &self.actor,
            request,
            &self.project,
            request_id,
        )? {
            return Ok(result);
        }
        let (binding, pin) = self.select_empty_layout(request)?;
        let result = self.store.checkpoint_resume_empty_baseline_task(
            &self.actor,
            request,
            &self.project,
            request_id,
            &binding,
        );
        drop(pin);
        result
    }

    /// Discard only the original unpublished encrypted task. No layout
    /// or directory was owned by the operation; never inspect/remove it.
    pub fn abandon(&self, request: &CheckpointRestoreRequest, request_id: &str) -> Result<()> {
        self.require_task_request(request)?;
        self.store.checkpoint_abandon_empty_baseline_task(
            &self.actor,
            request,
            &self.project,
            request_id,
        )
    }
}
