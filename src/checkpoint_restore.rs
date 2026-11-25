//! Offline captured-source restore under an independently selected Git
//! administrative and destination-layout approval. Never activates an Agent.
use crate::git_maintenance::{
    canonical_private_directory, open_private_approval, overlaps, pin_private_checkout,
    require_existing_store, require_store_scope,
};
use aw::{Error, PrincipalId, Result};
use aw_agent_core::{validate_source_set_hard_limit, DriverKey, SourceSet};
use aw_git::source_driver::{
    GitRestoreLocation, GitSourceAnchor, GitSourceDriver, GitSourceLocation,
};
use aw_host::source_checkpoint::{GitCheckpointRestoreHost, GitRestoreFence};
use aw_platform::DirectoryIdentity;
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
    #[arg(long = "agent-recover")]
    recover: bool,
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
        let root = &config.binding.tool_root;
        if &config.binding.working_directory != root
            || !root.is_absolute()
            || root.parent().is_none()
            || overlaps(directory, &config.store_directory)
            || overlaps(root, directory)
            || overlaps(root, &config.store_directory)
        {
            return Err(Error::Denied(
                "restore requires a disjoint private named layout",
            ));
        }
        validate_source_set_hard_limit(config.repositories.len())?;
        if config.repositories.is_empty() {
            return Err(Error::Denied(
                "captured restore requires approved Git sources",
            ));
        }
        for source in &config.repositories {
            source.name.validate()?;
        }
        config
            .repositories
            .sort_unstable_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
        if config
            .repositories
            .windows(2)
            .any(|pair| pair[0].name == pair[1].name)
        {
            return Err(Error::Denied("duplicate restore source approval"));
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
    ) -> Result<Fence<'_>> {
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
        let root = &layout.tool_root;
        canonical_private_directory(root)?;
        let parent = DirectoryIdentity::open(root)?;
        let identity = parent.identity()?;
        if identity != c.destination_identity
            || identity == DirectoryIdentity::open(&c.store_directory)?.identity()?
            || identity == DirectoryIdentity::open(&self.directory)?.identity()?
        {
            return Err(Error::Conflict(
                "approved restore destination changed or aliases host state",
            ));
        }
        // All materializations are named direct children of this reserved
        // parent. Fresh work requires emptiness; recovery only admits the
        // original named entries, never unrelated files or symlink targets.
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            if !recover
                || !entry.file_type()?.is_dir()
                || !c
                    .repositories
                    .iter()
                    .any(|source| entry.file_name() == source.name.as_str())
            {
                return Err(Error::Conflict(
                    "restore destination contains unapproved entries",
                ));
            }
        }
        let mut pins = Vec::with_capacity(1 + c.repositories.len() * 3);
        pins.push(parent);
        let mut locations = Vec::with_capacity(c.repositories.len());
        for source in &c.repositories {
            pins.extend(pin_private_checkout(
                source,
                &[root, &c.store_directory, &self.directory],
            )?);
            locations.push(GitRestoreLocation {
                name: source.name.clone(),
                repository: source.repository.clone(),
                repository_identity: source.repository_identity,
                target: root.join(source.name.as_str()),
                parent_identity: identity,
            });
        }
        Ok(Fence {
            _approval: self,
            locations,
            _pins: pins,
        })
    }
}

struct Fence<'a> {
    _approval: &'a Approval,
    locations: Vec<GitRestoreLocation>,
    _pins: Vec<DirectoryIdentity>,
}

impl GitRestoreFence for Fence<'_> {
    fn locations(&self) -> &[GitRestoreLocation] {
        &self.locations
    }
}

pub(crate) fn restore(
    store: &SessionStore,
    actor: &PrincipalId,
    path: &Path,
    request: &CheckpointRestoreRequest,
    request_id: &str,
    recover: bool,
) -> Result<CheckpointRestoreResult> {
    if request.task.is_some()
        || request.project.is_some()
        || request.access != AgentHistoryForkAccess::Private
    {
        return Err(Error::Denied(
            "offline captured restore requires same-Project private taskless construction",
        ));
    }
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
    let result = restore(
        &store,
        &actor,
        &args.approval,
        &request,
        &format!("cli:checkpoint-restore:{}", args.request_id),
        args.recover,
    )?;
    super::print_json(&serde_json::json!({"ref":result.agent.canonical_ref,
        "sandbox_ref":result.agent.sandbox_ref,"admission":result.agent.admission,
        "source_coverage":result.source_coverage}))?;
    Ok(0)
}
