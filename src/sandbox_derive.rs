//! Explicit offline same-Project Git Sandbox derivation. All construction,
//! source claims, recovery and publication remain with the original owner.
use crate::git_maintenance::{
    canonical_private_directory, open_private_approval, overlaps, pin_named_layout,
    require_existing_store, require_named_destination, require_store_scope,
    validate_named_locations, PinnedGitLayout,
};
use aw::{Error, PrincipalId, Result};
use aw_agent_core::{CheckpointPolicy, DriverKey};
use aw_git::source_driver::{GitSourceDriver, GitSourceLocation};
use aw_host::source_checkpoint::GitCheckpointRestoreHost;
use aw_platform::DirectoryIdentity;
use aw_store::{
    SandboxCreateSources, SandboxLifecycle, SandboxSnapshot, SessionBinding, SessionStore,
    VersionedSourceBinding,
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
    /// Private host approval; exclude other users of the parent, Git backing
    /// and destination throughout construction and any interruption.
    approval: PathBuf,
    /// Exact canonical same-Project parent Sandbox.
    parent: String,
    /// Stable original invocation identity (retain for recovery).
    request_id: String,
    /// Inspect and publish the original prepared child, never derive again.
    #[arg(long = "agent-recover", conflicts_with = "abandon")]
    recover: bool,
    /// Reconcile only the original unpublished child and its private custody.
    #[arg(long = "agent-abandon", conflicts_with = "recover")]
    abandon: bool,
    /// Inspect the original authenticated stdio RPC request ID instead of
    /// the standalone CLI namespace. Requires recovery or abandonment.
    #[arg(long = "rpc-request")]
    rpc_request: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u16,
    store_directory: PathBuf,
    store_id: String,
    store_generation: u64,
    manager: PrincipalId,
    parent: String,
    parent_rev: u64,
    binding: SessionBinding,
    destination_identity: (u64, u64),
    repositories: Vec<GitSourceLocation>,
}

struct Approval {
    config: Config,
    directory: PathBuf,
    parent_root: PathBuf,
    _lease: File,
}

impl Approval {
    fn open(store: &SessionStore, actor: &PrincipalId, parent: &str, path: &Path) -> Result<Self> {
        let directory = path
            .parent()
            .ok_or(Error::Denied("derive approval directory required"))?;
        let (lease, mut config): (_, Config) = open_private_approval(path)?;
        if config.version != 1
            || &config.manager != actor
            || config.parent != parent
            || config.parent_rev == 0
        {
            return Err(Error::Denied(
                "derive approval differs from host manager or parent",
            ));
        }
        require_store_scope(
            store,
            &config.store_directory,
            &config.store_id,
            config.store_generation,
        )?;
        let root = &config.binding.tool_root;
        let snapshot = store.sandbox_get(actor, parent)?;
        if snapshot.canonical_ref != parent
            || snapshot.rev != config.parent_rev
            || snapshot.lifecycle != SandboxLifecycle::Open
        {
            return Err(Error::Conflict("approved parent Sandbox changed"));
        }
        // First cut: complete named Git sources in a private root and a
        // separate fresh named-child layout. No linked checkout or arbitrary
        // overlapping parent layout is admitted by this offline host.
        let parent_root = snapshot.tool_root;
        require_named_destination(
            root,
            &config.binding.working_directory,
            &config.store_directory,
            directory,
        )?;
        if snapshot.working_directory != parent_root
            || !parent_root.is_absolute()
            || parent_root.parent().is_none()
            || overlaps(root, &parent_root)
            || overlaps(&parent_root, directory)
            || overlaps(&parent_root, &config.store_directory)
        {
            return Err(Error::Denied(
                "derive requires disjoint private named layouts",
            ));
        }
        validate_named_locations(&mut config.repositories)?;
        if config.repositories.is_empty()
            || config.repositories.len() != snapshot.sources.len()
            || config
                .repositories
                .iter()
                .zip(&snapshot.sources)
                .any(|(location, source)| {
                    location.name != source.name
                        || !(config.repositories.len() == 1 && location.repository == parent_root
                            || location.repository == parent_root.join(location.name.as_str()))
                })
        {
            return Err(Error::Denied(
                "derive approval must cover the complete named parent layout",
            ));
        }
        Ok(Self {
            config,
            directory: directory.to_owned(),
            parent_root,
            _lease: lease,
        })
    }

    fn fence(
        &self,
        layout: &SessionBinding,
        input: SandboxCreateSources<'_>,
        recovery: bool,
    ) -> Result<PinnedGitLayout<'_>> {
        let SandboxCreateSources::Parent(bindings) = input else {
            return Err(Error::Denied("derive cannot substitute a baseline"));
        };
        self.parent_fence(layout, bindings, recovery)
    }

    fn parent_fence(
        &self,
        layout: &SessionBinding,
        bindings: &[VersionedSourceBinding],
        recovery: bool,
    ) -> Result<PinnedGitLayout<'_>> {
        let c = &self.config;
        let git = DriverKey::builtin("git")?;
        if layout != &c.binding
            || bindings.len() != c.repositories.len()
            || bindings
                .iter()
                .zip(&c.repositories)
                .any(|(source, location)| {
                    source.binding.name != location.name
                        || source.binding.driver != git
                        || source.binding.checkpoint_policy != CheckpointPolicy::Required
                })
        {
            return Err(Error::Denied(
                "derive approval differs from exact original Git sources",
            ));
        }
        canonical_private_directory(&self.parent_root)?;
        let parent = DirectoryIdentity::open(&self.parent_root)?;
        if parent.identity()? == c.destination_identity {
            return Err(Error::Denied("derived layout aliases the parent"));
        }
        let mut fence = pin_named_layout(
            &self._lease,
            &layout.tool_root,
            c.destination_identity,
            &c.repositories,
            &[&c.store_directory, &self.directory],
            recovery,
        )?;
        fence.retain_directory(parent);
        Ok(fence)
    }
}

fn project(parent: &str) -> Result<&str> {
    parent
        .rsplit_once("/s:")
        .map(|(project, _)| project)
        .ok_or(Error::Denied("derive requires canonical parent Sandbox"))
}

pub(crate) fn derive(
    store: &SessionStore,
    actor: &PrincipalId,
    approval: &Path,
    parent: &str,
    request_id: &str,
    recover: bool,
) -> Result<SandboxSnapshot> {
    let project = project(parent)?;
    // Semantic replay is the original project/from/request identity. The
    // layout and parent revision are independently selected host inputs.
    match store.sandbox_replay_create(actor, project, Some(parent), request_id) {
        Ok(Some(result)) => return Ok(result),
        Ok(None) if recover => return Err(Error::Denied("original prepared derive unavailable")),
        Ok(None) => (),
        Err(Error::Conflict(_)) if recover => (),
        Err(error) => return Err(error),
    }
    let approval = Approval::open(store, actor, parent, approval)?;
    let c = &approval.config;
    let handle = store.source_driver_handle::<GitSourceDriver>(&DriverKey::builtin("git")?)?;
    let host = GitCheckpointRestoreHost::new(store, &handle);
    if recover {
        host.resume_sandbox_derive_exact(
            actor,
            project,
            parent,
            request_id,
            &c.binding,
            c.parent_rev,
            |layout, input, _| approval.fence(layout, input, true),
        )
    } else {
        host.derive_sandbox_exact(
            actor,
            project,
            parent,
            request_id,
            c.parent_rev,
            || Ok(c.binding.clone()),
            |layout, bindings| approval.parent_fence(layout, bindings, false),
        )
    }
}

pub(crate) fn abandon(
    store: &SessionStore,
    actor: &PrincipalId,
    approval: &Path,
    parent: &str,
    request_id: &str,
) -> Result<()> {
    let project = project(parent)?;
    if store.sandbox_replay_abandoned_create(actor, project, Some(parent), request_id)? {
        return Ok(());
    }
    let approval = Approval::open(store, actor, parent, approval)?;
    let c = &approval.config;
    let handle = store.source_driver_handle::<GitSourceDriver>(&DriverKey::builtin("git")?)?;
    GitCheckpointRestoreHost::new(store, &handle).abandon_sandbox_derive_exact(
        actor,
        project,
        parent,
        request_id,
        &c.binding,
        c.parent_rev,
        |input, _| approval.fence(&c.binding, input, true),
    )
}

pub(crate) fn run(args: Args) -> Result<u8> {
    let id = crate::source_construction_rpc::Operation::SandboxDerive.offline_id(
        &args.request_id,
        args.rpc_request,
        args.recover || args.abandon,
    )?;
    let actor = PrincipalId::new(aw_platform::local_principal()?)?;
    require_existing_store(&args.state_dir)?;
    let store = aw::sessions::open_sqlite_owner(&args.state_dir)?;

    if args.abandon {
        abandon(&store, &actor, &args.approval, &args.parent, &id)?;
        super::print_json(&serde_json::json!({"status":"rejected"}))?;
    } else {
        let result = derive(
            &store,
            &actor,
            &args.approval,
            &args.parent,
            &id,
            args.recover,
        )?;
        super::print_json(&super::sandbox_snapshot_value(&result, true))?;
    }
    Ok(0)
}
