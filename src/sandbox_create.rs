//! Explicit offline retained-version Project baseline. The host supplies
//! current independent metadata/materialization/retention authority; the
//! original Sandbox constructor owns the effect and its recovery.
use crate::git_maintenance::{
    open_private_approval, pin_named_layout, require_existing_store, require_named_destination,
    require_store_scope, validate_named_locations, PinnedGitLayout,
};
use aw::{Error, PrincipalId, Result};
use aw_agent_core::{CheckpointPolicy, DriverKey, SourceCoverage, SourceSet};
use aw_git::source_driver::{GitSourceAnchor, GitSourceDriver, GitSourceLocation};
use aw_host::source_checkpoint::GitCheckpointRestoreHost;
use aw_store::{
    SandboxCreateSources, SandboxRecoverySelection, SandboxSnapshot, SessionBinding, SessionStore,
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
    /// Private exact retained-version approval. Independently protect all
    /// Git backing/retention and destination users through any interruption.
    approval: PathBuf,
    /// Exact canonical destination Project.
    project: String,
    /// Stable original invocation identity (retain for recovery).
    request_id: String,
    /// Inspect and publish the original candidate; never rematerialize.
    #[arg(long = "agent-recover", conflicts_with = "abandon")]
    recover: bool,
    /// Reconcile only unpublished materializations, not the baseline refs.
    #[arg(long = "agent-abandon", conflicts_with = "recover")]
    abandon: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u16,
    store_directory: PathBuf,
    store_id: String,
    store_generation: u64,
    manager: PrincipalId,
    project: String,
    baseline: SourceSet,
    binding: SessionBinding,
    destination_identity: (u64, u64),
    repositories: Vec<GitSourceLocation>,
}

struct Approval {
    config: Config,
    directory: PathBuf,
    lease: File,
}

impl Approval {
    fn open(store: &SessionStore, actor: &PrincipalId, project: &str, path: &Path) -> Result<Self> {
        let directory = path
            .parent()
            .ok_or(Error::Denied("baseline approval directory required"))?;
        let (lease, mut config): (_, Config) = open_private_approval(path)?;
        if config.version != 1 || &config.manager != actor || config.project != project {
            return Err(Error::Denied(
                "baseline approval differs from host manager or Project",
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
        // Only the explicit complete retained-version Git baseline. No
        // ambient HEAD selection, ephemeral fallback or new retention effect.
        config.baseline.validate()?;
        if config.baseline.coverage != SourceCoverage::Selected
            || config
                .baseline
                .anchors
                .iter()
                .any(|anchor| anchor.checkpoint_policy != CheckpointPolicy::Required)
        {
            return Err(Error::Denied(
                "baseline requires explicitly selected required retained versions",
            ));
        }
        config
            .baseline
            .anchors
            .sort_unstable_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
        validate_named_locations(&mut config.repositories)?;
        if config.repositories.len() != config.baseline.anchors.len() {
            return Err(Error::Denied(
                "baseline approval must cover the complete named source set",
            ));
        }
        for (anchor, location) in config.baseline.anchors.iter().zip(&config.repositories) {
            let anchor = GitSourceAnchor::try_from(anchor)?;
            if anchor.name != location.name {
                return Err(Error::Denied("baseline approval names another source"));
            }
        }
        Ok(Self {
            config,
            directory: directory.to_owned(),
            lease,
        })
    }

    fn fence(
        &self,
        layout: &SessionBinding,
        anchors: &SourceSet,
        recover: bool,
    ) -> Result<PinnedGitLayout<'_>> {
        let c = &self.config;
        if layout != &c.binding || anchors != &c.baseline {
            return Err(Error::Denied(
                "baseline approval differs from the exact original source set or layout",
            ));
        }
        pin_named_layout(
            &self.lease,
            &layout.tool_root,
            c.destination_identity,
            &c.repositories,
            &[&c.store_directory, &self.directory],
            recover,
        )
    }

    fn recovery_fence(&self, input: SandboxCreateSources<'_>) -> Result<PinnedGitLayout<'_>> {
        let SandboxCreateSources::Baseline(anchors) = input else {
            return Err(Error::Denied("baseline cannot substitute a mutable parent"));
        };
        self.fence(&self.config.binding, anchors, true)
    }
}

pub(crate) fn create(
    store: &SessionStore,
    actor: &PrincipalId,
    path: &Path,
    project: &str,
    request_id: &str,
    recover: bool,
) -> Result<SandboxSnapshot> {
    // The approved baseline and destination are trusted host choices.
    // The semantic request is still the original Project/from=None identity.
    match store.sandbox_replay_create(actor, project, None, request_id) {
        Ok(Some(result)) => return Ok(result),
        Ok(None) if recover => return Err(Error::Denied("original prepared baseline unavailable")),
        Ok(None) => (),
        Err(Error::Conflict(_)) if recover => (),
        Err(error) => return Err(error),
    }
    let approval = Approval::open(store, actor, project, path)?;
    let c = &approval.config;
    let handle = store.source_driver_handle::<GitSourceDriver>(&DriverKey::builtin("git")?)?;
    let host = GitCheckpointRestoreHost::new(store, &handle);
    if recover {
        host.resume_sandbox_create_exact(
            actor,
            project,
            request_id,
            &c.binding,
            SandboxRecoverySelection::Baseline(&c.baseline),
            |_, input, _| approval.recovery_fence(input),
        )
    } else {
        host.create_sandbox_from_baseline(
            actor,
            project,
            request_id,
            || Ok((c.binding.clone(), c.baseline.clone())),
            |layout, anchors| approval.fence(layout, anchors, false),
        )
    }
}

pub(crate) fn abandon(
    store: &SessionStore,
    actor: &PrincipalId,
    path: &Path,
    project: &str,
    request_id: &str,
) -> Result<()> {
    if store.sandbox_replay_abandoned_create(actor, project, None, request_id)? {
        return Ok(());
    }
    let approval = Approval::open(store, actor, project, path)?;
    let c = &approval.config;
    let handle = store.source_driver_handle::<GitSourceDriver>(&DriverKey::builtin("git")?)?;
    GitCheckpointRestoreHost::new(store, &handle).abandon_sandbox_create_exact(
        actor,
        project,
        request_id,
        &c.binding,
        SandboxRecoverySelection::Baseline(&c.baseline),
        |input, _| approval.recovery_fence(input),
    )
}

pub(crate) fn run(args: Args) -> Result<u8> {
    let actor = PrincipalId::new(aw_platform::local_principal()?)?;
    require_existing_store(&args.state_dir)?;
    let store = aw::sessions::open_sqlite_owner(&args.state_dir)?;
    let id = format!("cli:sandbox-create:{}", args.request_id);
    if args.abandon {
        abandon(&store, &actor, &args.approval, &args.project, &id)?;
        super::print_json(&serde_json::json!({"status":"rejected"}))?;
    } else {
        let result = create(
            &store,
            &actor,
            &args.approval,
            &args.project,
            &id,
            args.recover,
        )?;
        super::print_json(&super::sandbox_snapshot_value(&result, true))?;
    }
    Ok(0)
}
