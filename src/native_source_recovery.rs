//! Explicit offline recovery of one original native Git-backed tool effect.
//! Logical Turn/queue/hazard disposition remains with their original owners.
//! The operator independently guarantees permanent native worker quiescence
//! and original artifact namespace custody, excluding external writers,
//! aliases, mounts and other Stores throughout recovery. Pins and the
//! exclusive Store/approval lock are not a kernel containment proof.
use crate::git_maintenance::{
    canonical_private_directory, open_private_approval, overlaps, pin_private_checkout,
    pin_private_linked_checkout, require_existing_store, require_store_scope, LinkedGitApproval,
};
use aw::{Error, PrincipalId, Result};
use aw_agent_core::{validate_source_set_hard_limit, SourceAccess};
use aw_git::source_driver::GitSourceLocation;
use aw_host::{
    source_checkpoint::GitSourceFence,
    source_git::{GitNativeSourceResolver, NativeSourceRecoveryTarget},
};
use aw_platform::DirectoryIdentity;
use aw_store::{SandboxEffectOutcome, SandboxToolRecovery, SessionStore};
use serde::Deserialize;
use std::{
    fs::File,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Repository {
    location: GitSourceLocation,
    binding_rev: u64,
    access: SourceAccess,
    /// V2: explicit independent linked-worktree administration/backing.
    #[serde(default)]
    linked: Option<LinkedGitApproval>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u16,
    store_directory: PathBuf,
    store_id: String,
    store_generation: u64,
    manager: PrincipalId,
    agent: String,
    sandbox: String,
    turn: u64,
    call: String,
    base: PathBuf,
    base_identity: (u64, u64),
    // The recovery approval lives in the independently protected,
    // ORIGINAL private artifact state directory, never a new namespace.
    artifact_state_identity: (u64, u64),
    repositories: Vec<Repository>,
}

struct Approval {
    config: Config,
    artifact_state: PathBuf,
    _lease: File,
}

struct Fence {
    locations: Vec<GitSourceLocation>,
    _pins: Vec<DirectoryIdentity>,
}

impl GitSourceFence for Fence {
    fn locations(&self) -> &[GitSourceLocation] {
        &self.locations
    }
}

impl Approval {
    fn open(path: &Path, manager: &PrincipalId) -> Result<Self> {
        let (lease, mut config): (_, Config) = open_private_approval(path)?;
        let root = path.parent().ok_or(Error::Denied(
            "native recovery approval requires its original artifact directory",
        ))?;
        let has_linked = config.repositories.iter().any(|repo| repo.linked.is_some());
        if !matches!((config.version, has_linked), (1, false) | (2, true))
            || &config.manager != manager
            || config.store_generation == 0
            || config.turn == 0
            || config.turn > i64::MAX as u64
            || config.agent.is_empty()
            || config.agent.len() > 2048
            || config.sandbox.is_empty()
            || config.sandbox.len() > 2048
            || config.call.is_empty()
            || config.call.len() > 128
            || !config.base.is_absolute()
            || config.base.parent().is_none()
            || overlaps(root, &config.store_directory)
            || overlaps(&config.base, &config.store_directory)
            || overlaps(&config.base, root)
        {
            return Err(Error::Denied(
                "invalid exact native source recovery approval",
            ));
        }
        validate_source_set_hard_limit(config.repositories.len())?;
        config
            .repositories
            .sort_unstable_by(|a, b| a.location.name.as_str().cmp(b.location.name.as_str()));
        if config.repositories.is_empty()
            || config
                .repositories
                .windows(2)
                .any(|p| p[0].location.name == p[1].location.name)
        {
            return Err(Error::Denied(
                "native recovery requires unique original named sources",
            ));
        }
        for repository in &config.repositories {
            repository.location.name.validate()?;
            if repository.binding_rev == 0 || !repository.location.repository.is_absolute() {
                return Err(Error::Denied("invalid native recovery source selection"));
            }
        }
        require_existing_store(&config.store_directory)?;
        Ok(Self {
            config,
            artifact_state: root.to_owned(),
            _lease: lease,
        })
    }

    fn recover(&self, store: &SessionStore) -> Result<SandboxEffectOutcome> {
        let c = &self.config;
        require_store_scope(store, &c.store_directory, &c.store_id, c.store_generation)?;
        GitNativeSourceResolver::recover_configured(
            store,
            &c.manager,
            NativeSourceRecoveryTarget {
                agent_ref: &c.agent,
                turn_id: c.turn,
                call_id: &c.call,
            },
            &c.sandbox,
            |recovery| Ok((self.fence(store, recovery)?, self.artifact_state.clone())),
        )
    }

    fn fence(&self, store: &SessionStore, recovery: &SandboxToolRecovery) -> Result<Fence> {
        let c = &self.config;
        let sandbox = store.sandbox_get(&c.manager, &c.sandbox)?;
        if recovery.agent_ref != c.agent
            || recovery.sandbox_ref != c.sandbox
            || sandbox.tool_root != c.base
            || sandbox.working_directory != c.base
            || recovery.sources.len() != c.repositories.len()
            || sandbox.sources.len() != c.repositories.len()
            || recovery
                .sources
                .iter()
                .zip(&c.repositories)
                .any(|(source, approved)| {
                    source.binding.binding.name != approved.location.name
                        || source.binding.binding_rev != approved.binding_rev
                        || !approved.access.permits(source.access)
                })
        {
            return Err(Error::Denied(
                "native recovery differs from original complete source custody",
            ));
        }
        canonical_private_directory(&c.base)?;
        let base = DirectoryIdentity::open(&c.base)?;
        let artifacts = DirectoryIdentity::open(&self.artifact_state)?;
        let state = DirectoryIdentity::open(&c.store_directory)?;
        if base.identity()? != c.base_identity
            || artifacts.identity()? != c.artifact_state_identity
            || artifacts.identity()? == state.identity()?
            || base.identity()? == state.identity()?
            || base.identity()? == artifacts.identity()?
        {
            return Err(Error::Conflict(
                "approved native recovery directory identity changed",
            ));
        }
        let protected = [c.store_directory.as_path(), self.artifact_state.as_path()];
        let mut pins = vec![base, artifacts, state];
        let mut locations = Vec::with_capacity(c.repositories.len());
        for repository in &c.repositories {
            let location = &repository.location;
            if !(c.repositories.len() == 1 && location.repository == c.base)
                && location.repository != c.base.join(location.name.as_str())
            {
                return Err(Error::Denied(
                    "native recovery requires the original named checkout layout",
                ));
            }
            // Independently pinned physical approval precedes the original
            // driver's materialization proof and artifact inspection.
            match &repository.linked {
                Some(linked) => pins.extend(pin_private_linked_checkout(
                    location, linked, &c.base, &protected,
                )?),
                None => pins.extend(pin_private_checkout(location, &protected)?),
            }
            locations.push(location.clone());
        }
        Ok(Fence {
            locations,
            _pins: pins,
        })
    }
}

pub(crate) fn run(path: &Path) -> Result<u8> {
    let manager = PrincipalId::new(aw_platform::local_principal()?)?;
    let approval = Approval::open(path, &manager)?;
    let store = aw::sessions::open_sqlite_owner(&approval.config.store_directory)?;
    let outcome = approval.recover(&store)?;
    super::print_json(&serde_json::json!({
        "ref": approval.config.agent, "turn": approval.config.turn, "call": approval.config.call,
        "outcome": outcome, "scope": "source_effect_only"
    }))?;
    Ok(if outcome == SandboxEffectOutcome::Unknown {
        3
    } else {
        0
    })
}

/// No physical approval needed for this purely factual original-owner
/// disposition: each source effect must already have its own release.
pub(crate) fn reconcile_turn(state_dir: &Path, agent: &str, turn: u64) -> Result<u8> {
    require_existing_store(state_dir)?;
    let manager = PrincipalId::new(aw_platform::local_principal()?)?;
    let store = aw::sessions::open_sqlite_owner(state_dir)?;
    let result = store.reconcile_native_turn(&manager, agent, turn)?;
    super::print_json(
        &serde_json::to_value(result)
            .map_err(|_| Error::Storage("invalid native reconciliation result"))?,
    )?;
    Ok(0)
}
