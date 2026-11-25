//! Offline captured-source restore under an independently selected Git
//! administrative and destination-layout approval. Never activates an Agent.
use crate::git_maintenance::{
    open_private_approval, pin_named_layout, require_existing_store, require_named_destination,
    require_store_scope, validate_named_locations, PinnedGitLayout,
};
use aw::{Error, PrincipalId, Result};
use aw_agent_core::{DriverKey, SourceSet};
use aw_git::source_driver::{GitSourceAnchor, GitSourceDriver, GitSourceLocation};
use aw_host::source_checkpoint::GitCheckpointRestoreHost;
use aw_store::{
    AgentHistoryForkAccess, CheckpointRestoreRequest, CheckpointRestoreResult, SessionBinding,
    SessionStore,
};
use serde::Deserialize;
use std::{
    fs::File,
    path::{Path, PathBuf},
};

#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Existing private Store directory.
    #[arg(long = "state-dir")]
    state_dir: PathBuf,
    /// Private host approval file. The operator independently excludes all
    /// other writers, aliases, retention users and destination access.
    approval: PathBuf,
    /// Exact captured checkpoint ID.
    checkpoint: String,
    /// Stable original invocation identity (retain for recovery).
    request_id: String,
    /// Optional destination Agent title, part of the original request.
    #[arg(long = "session-title")]
    title: Option<String>,
    /// Inspect/publish the original prepared restore; never rematerialize.
    #[arg(long = "agent-recover", conflicts_with = "abandon")]
    recover: bool,
    /// Reconcile the original unpublished worktrees, never a published Agent.
    #[arg(long = "agent-abandon", conflicts_with = "recover")]
    abandon: bool,
}

// Separate from native filesystem access or retired-retention permission:
// this selects the complete protected destination and original Git backing.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u16,
    store_directory: PathBuf,
    store_id: String,
    store_generation: u64,
    manager: PrincipalId,
    checkpoint: String,
    binding: SessionBinding,
    destination_identity: (u64, u64),
    repositories: Vec<GitSourceLocation>,
}

struct Approval {
    config: Config,
    directory: PathBuf,
    _lease: File,
}

impl Approval {
    fn open(
        store: &SessionStore,
        actor: &PrincipalId,
        checkpoint: &str,
        path: &Path,
    ) -> Result<Self> {
        let directory = path
            .parent()
            .ok_or(Error::Denied("restore approval directory required"))?;
        let (lease, mut config): (_, Config) = open_private_approval(path)?;
        if config.version != 1 || &config.manager != actor || config.checkpoint != checkpoint {
            return Err(Error::Denied(
                "restore approval differs from host manager or checkpoint",
            ));
        }
        require_store_scope(
            store,
            &config.store_directory,
            &config.store_id,
            config.store_generation,
        )?;
        require_named_destination(
            &config.binding.tool_root,
            &config.binding.working_directory,
            &config.store_directory,
            directory,
        )?;
        validate_named_locations(&mut config.repositories)?;
        if config.repositories.is_empty() {
            return Err(Error::Denied(
                "captured restore requires approved Git sources",
            ));
        }
        Ok(Self {
            config,
            directory: directory.to_owned(),
            _lease: lease,
        })
    }

    fn fence(
        &self,
        layout: &SessionBinding,
        anchors: &SourceSet,
        recover: bool,
    ) -> Result<PinnedGitLayout<'_>> {
        let c = &self.config;
        if layout != &c.binding
            || c.repositories.len() != anchors.anchors.len()
            || anchors.anchors.is_empty()
        {
            return Err(Error::Denied(
                "restore approval differs from exact captured source set",
            ));
        }
        // Check the whole original anchor set before probing physical paths.
        for (anchor, location) in anchors.anchors.iter().zip(&c.repositories) {
            let anchor = GitSourceAnchor::try_from(anchor)?;
            if anchor.name != location.name || anchor.checkpoint_id != c.checkpoint {
                return Err(Error::Denied(
                    "restore approval names another captured source",
                ));
            }
        }
        pin_named_layout(
            &self._lease,
            &layout.tool_root,
            c.destination_identity,
            &c.repositories,
            &[&c.store_directory, &self.directory],
            recover,
        )
    }
}

fn require_offline_request(request: &CheckpointRestoreRequest) -> Result<()> {
    if request.task.is_some()
        || request.project.is_some()
        || request.access != AgentHistoryForkAccess::Private
    {
        return Err(Error::Denied(
            "offline captured restore requires same-Project private taskless construction",
        ));
    }
    Ok(())
}

pub(crate) fn restore(
    store: &SessionStore,
    actor: &PrincipalId,
    path: &Path,
    request: &CheckpointRestoreRequest,
    request_id: &str,
    recover: bool,
) -> Result<CheckpointRestoreResult> {
    require_offline_request(request)?;
    // Applied replay requires neither physical approval nor Git lookup.
    // Incomplete operations never become new invocations.
    match store.checkpoint_replay_restore(actor, request, request_id) {
        Ok(Some(result)) => return Ok(result),
        Ok(None) if recover => return Err(Error::Denied("original prepared restore unavailable")),
        Ok(None) => (),
        Err(Error::Conflict(_)) if recover => (), // exact original owner checks again
        Err(error) => return Err(error),
    }
    let approval = Approval::open(store, actor, &request.checkpoint, path)?;
    let handle = store.source_driver_handle::<GitSourceDriver>(&DriverKey::builtin("git")?)?;
    let host = GitCheckpointRestoreHost::new(store, &handle);
    if recover {
        host.resume_captured_exact(
            actor,
            request,
            request_id,
            &approval.config.binding,
            |layout, anchors, _| approval.fence(layout, anchors, true),
        )
    } else {
        host.restore(
            actor,
            request,
            request_id,
            "local",
            || Ok(approval.config.binding.clone()),
            |layout, anchors| approval.fence(layout, anchors, false),
        )
    }
}

/// Exact offline abandonment; no new layout, task, source or published
/// Agent cleanup. A terminal Rejected replay does not reopen host state.
pub(crate) fn abandon(
    store: &SessionStore,
    actor: &PrincipalId,
    path: &Path,
    request: &CheckpointRestoreRequest,
    request_id: &str,
) -> Result<()> {
    require_offline_request(request)?;
    if store.checkpoint_replay_abandoned_captured_restore(actor, request, request_id)? {
        return Ok(());
    }
    let approval = Approval::open(store, actor, &request.checkpoint, path)?;
    let handle = store.source_driver_handle::<GitSourceDriver>(&DriverKey::builtin("git")?)?;
    GitCheckpointRestoreHost::new(store, &handle).abandon_captured_exact(
        actor,
        request,
        request_id,
        &approval.config.binding,
        |anchors, _| approval.fence(&approval.config.binding, anchors, true),
    )
}

pub(crate) fn run(args: Args) -> Result<u8> {
    let actor = PrincipalId::new(aw_platform::local_principal()?)?;
    require_existing_store(&args.state_dir)?;
    let store = aw::sessions::open_sqlite_owner(&args.state_dir)?;
    let request = CheckpointRestoreRequest {
        checkpoint: args.checkpoint,
        project: None,
        title: args.title,
        access: AgentHistoryForkAccess::Private,
        task: None,
    };
    let request_id = format!("cli:checkpoint-restore:{}", args.request_id);
    if args.abandon {
        abandon(&store, &actor, &args.approval, &request, &request_id)?;
        super::print_json(&serde_json::json!({"status":"rejected"}))?;
        return Ok(0);
    }
    let result = restore(
        &store,
        &actor,
        &args.approval,
        &request,
        &request_id,
        args.recover,
    )?;
    super::print_json(&serde_json::json!({"ref":result.agent.canonical_ref,
        "sandbox_ref":result.agent.sandbox_ref,"admission":result.agent.admission,
        "source_coverage":result.source_coverage}))?;
    Ok(0)
}
