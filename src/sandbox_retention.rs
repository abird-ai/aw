//! Explicit offline release or retirement of exact original managed Git worktrees.
//! Logical Sandbox/Agent closure remains with their existing owners.
use crate::git_maintenance::{
    open_private_approval, overlaps, pin_named_layout, require_existing_store, require_store_scope,
    validate_named_locations, PinnedGitLayout,
};
use aw::{Error, PrincipalId, Result};
use aw_agent_core::{DriverKey, NamedPreparedSource, SourceName};
use aw_git::source_driver::{GitSourceDriver, GitSourceLocation};
use aw_host::source_checkpoint::{reconcile_closed_git_sandbox, GitCheckpointRestoreHost};
use aw_store::{SandboxSnapshot, SessionStore, VersionedSourceBinding};
use serde::Deserialize;
use std::{
    fs::File,
    path::{Path, PathBuf},
};

#[derive(clap::Args, Debug)]
pub(crate) struct ReleaseArgs {
    /// Existing private Store directory.
    #[arg(long = "state-dir")]
    state_dir: PathBuf,
    /// Private approval of exact removal and the entire retained layout.
    approval: PathBuf,
    /// Exact canonical unused open Sandbox.
    sandbox: String,
    /// Named original source to remove; revision is in the approval.
    source: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u16,
    store_directory: PathBuf,
    store_id: String,
    store_generation: u64,
    manager: PrincipalId,
    sandbox: String,
    sandbox_rev: u64,
    destination_root: PathBuf,
    destination_identity: (u64, u64),
    repositories: Vec<GitSourceLocation>,
    /// Present only for removal of one named source from an open Sandbox.
    /// Closed-Sandbox retirement must not consume this decision.
    #[serde(default)]
    source: Option<SourceName>,
}

pub(crate) struct Approval {
    config: Config,
    directory: PathBuf,
    _lease: File,
}

impl Approval {
    pub(crate) fn open(path: &Path, manager: &PrincipalId) -> Result<Self> {
        Self::open_in(path, manager, None)
    }

    fn open_in(path: &Path, manager: &PrincipalId, source: Option<&SourceName>) -> Result<Self> {
        let parent = path
            .parent()
            .ok_or(Error::Denied("Sandbox approval directory required"))?;
        let (lease, mut config): (_, Config) = open_private_approval(path)?;
        if config.version != 1
            || &config.manager != manager
            || config.source.as_ref() != source
            || config.sandbox_rev == 0
            || config.store_generation == 0
            || config.sandbox.len() > 2048
            || config.sandbox.is_empty()
            || !config.destination_root.is_absolute()
            || config.destination_root.parent().is_none()
            || overlaps(parent, &config.store_directory)
            || overlaps(&config.destination_root, &config.store_directory)
            || overlaps(&config.destination_root, parent)
        {
            return Err(Error::Denied("invalid Sandbox maintenance approval"));
        }
        validate_named_locations(&mut config.repositories)?;
        if config.repositories.is_empty() {
            return Err(Error::Denied(
                "Sandbox cleanup requires exact managed sources",
            ));
        }
        if let Some(source) = source {
            source.validate()?;
            if !config
                .repositories
                .iter()
                .any(|location| &location.name == source)
            {
                return Err(Error::Denied(
                    "source release approval omits the selected source",
                ));
            }
        }
        require_existing_store(&config.store_directory)?;
        Ok(Self {
            config,
            directory: parent.to_owned(),
            _lease: lease,
        })
    }

    pub(crate) fn reconcile(&self, store: &SessionStore) -> Result<SandboxSnapshot> {
        let c = &self.config;
        if c.source.is_some() {
            return Err(Error::Denied(
                "named source approval does not authorize closed Sandbox cleanup",
            ));
        }
        require_store_scope(store, &c.store_directory, &c.store_id, c.store_generation)?;
        reconcile_closed_git_sandbox(
            store,
            &c.manager,
            &c.sandbox,
            c.sandbox_rev,
            |snapshot, bindings, prepared| self.fence(snapshot, bindings, prepared),
        )
    }

    fn release(&self, store: &SessionStore, sandbox: &str) -> Result<SandboxSnapshot> {
        let c = &self.config;
        let name = c.source.as_ref().ok_or(Error::Denied(
            "named source release requires separate approval",
        ))?;
        if c.sandbox != sandbox {
            return Err(Error::Denied(
                "source release approval names another Sandbox",
            ));
        }
        require_store_scope(store, &c.store_directory, &c.store_id, c.store_generation)?;
        // Original CAS/name is the complete semantic request. A settled
        // replay has no physical work, even after a later reattachment.
        if let Some(result) =
            store.sandbox_replay_source_release(&c.manager, sandbox, name, c.sandbox_rev)?
        {
            return Ok(result);
        }
        let handle = store.source_driver_handle::<GitSourceDriver>(&DriverKey::builtin("git")?)?;
        GitCheckpointRestoreHost::new(store, &handle).release_sandbox_source(
            &c.manager,
            sandbox,
            name,
            c.sandbox_rev,
            |release| {
                self.fence(
                    release.snapshot(),
                    release.bindings(),
                    release.preparations(),
                )
            },
        )
    }

    fn fence(
        &self,
        snapshot: &SandboxSnapshot,
        bindings: &[VersionedSourceBinding],
        prepared: &[NamedPreparedSource],
    ) -> Result<PinnedGitLayout<'_>> {
        let c = &self.config;
        if snapshot.canonical_ref != c.sandbox
            || snapshot.rev != c.sandbox_rev
            || snapshot.tool_root != c.destination_root
            || snapshot.working_directory != c.destination_root
            || bindings.len() != c.repositories.len()
            || bindings.len() != prepared.len()
            || bindings.iter().zip(&c.repositories).zip(prepared).any(
                |((binding, location), source)| {
                    binding.binding.name != location.name || source.name != location.name
                },
            )
        {
            return Err(Error::Denied(
                "cleanup approval differs from exact retained Sandbox layout",
            ));
        }
        // Only private named children. No arbitrary source binding is a
        // cleanup recipe: the original Applied descriptor is checked by
        // the Git host before the cleanup claim and each release.
        pin_named_layout(
            &self._lease,
            &c.destination_root,
            c.destination_identity,
            &c.repositories,
            &[&c.store_directory, &self.directory],
            true,
        )
    }
}

pub(crate) fn release(
    store: &SessionStore,
    actor: &PrincipalId,
    path: &Path,
    sandbox: &str,
    source: &SourceName,
) -> Result<SandboxSnapshot> {
    Approval::open_in(path, actor, Some(source))?.release(store, sandbox)
}

pub(crate) fn run_release(args: ReleaseArgs) -> Result<u8> {
    let actor = PrincipalId::new(aw_platform::local_principal()?)?;
    let source = SourceName::new(args.source)?;
    require_existing_store(&args.state_dir)?;
    let store = aw::sessions::open_sqlite_owner(&args.state_dir)?;
    let result = release(&store, &actor, &args.approval, &args.sandbox, &source)?;
    super::print_json(&super::sandbox_snapshot_value(&result, true))?;
    Ok(0)
}

pub(crate) fn run(path: &Path) -> Result<u8> {
    let actor = PrincipalId::new(aw_platform::local_principal()?)?;
    let approval = Approval::open(path, &actor)?;
    let store = aw::sessions::open_sqlite_owner(&approval.config.store_directory)?;
    let result = approval.reconcile(&store)?;
    super::print_json(
        &serde_json::json!({"ref":result.canonical_ref,"rev":result.rev,"status":"settled"}),
    )?;
    Ok(0)
}
