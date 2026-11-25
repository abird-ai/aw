//! Explicit offline retirement of exact original managed Git worktrees.
//! Logical Sandbox/Agent closure remains with their existing owners.
use crate::git_maintenance::{
    open_private_approval, overlaps, pin_named_layout, require_existing_store, require_store_scope,
    validate_named_locations,
};
use aw::{Error, PrincipalId, Result};
use aw_agent_core::NamedPreparedSource;
use aw_git::source_driver::{GitRestoreLocation, GitSourceLocation};
use aw_host::source_checkpoint::{reconcile_closed_git_sandbox, GitRestoreFence};
use aw_platform::DirectoryIdentity;
use aw_store::{SandboxSnapshot, SessionStore, VersionedSourceBinding};
use serde::Deserialize;
use std::{
    fs::File,
    path::{Path, PathBuf},
};

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
}

pub(crate) struct Approval {
    config: Config,
    directory: PathBuf,
    _lease: File,
}

impl Approval {
    pub(crate) fn open(path: &Path, manager: &PrincipalId) -> Result<Self> {
        let parent = path
            .parent()
            .ok_or(Error::Denied("Sandbox approval directory required"))?;
        let (lease, mut config): (_, Config) = open_private_approval(path)?;
        if config.version != 1
            || &config.manager != manager
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
            return Err(Error::Denied("invalid closed Sandbox maintenance approval"));
        }
        validate_named_locations(&mut config.repositories)?;
        if config.repositories.is_empty() {
            return Err(Error::Denied(
                "closed Sandbox cleanup requires exact managed sources",
            ));
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
        require_store_scope(store, &c.store_directory, &c.store_id, c.store_generation)?;
        reconcile_closed_git_sandbox(
            store,
            &c.manager,
            &c.sandbox,
            c.sandbox_rev,
            |snapshot, bindings, prepared| self.fence(snapshot, bindings, prepared),
        )
    }

    fn fence(
        &self,
        snapshot: &SandboxSnapshot,
        bindings: &[VersionedSourceBinding],
        prepared: &[NamedPreparedSource],
    ) -> Result<Fence<'_>> {
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
        let (locations, pins) = pin_named_layout(
            &c.destination_root,
            c.destination_identity,
            &c.repositories,
            &[&c.store_directory, &self.directory],
            true,
        )?;
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
