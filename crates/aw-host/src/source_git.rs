//! Concrete Git-to-native realization. The original live host decision is
//! supplied by the authority owner; a stored SandboxGrant is never a grant
//! to probe or mount arbitrary Git materialization locators.
use crate::{
    source_approval::{
        NativeSourceApproval, NativeSourceApprovalGuard, NativeSourceApprovalSpec,
        NativeSourceDirectorySpec,
    },
    source_filesystem::{
        ApprovedSourceLayout, NativeFilesystemDriver, NativeSourceAdmission, NativeSourceRoot,
        SourceFilesystemRoute,
    },
};
use aw_agent_core::{
    Agent, AgentBuilder, AgentExecutionBinding, Error, PrincipalId, Result, SandboxDriverHandle,
    SandboxDriverRegistry, SourceAccess, SourceDriverHandle, SourceName, ToolContext, ToolEffect,
};
use aw_git::{
    source_driver::{GitSourceDriver, GitSourceFreeLayout, GitSourceLocation},
    Repository,
};
use aw_store::{
    SandboxEffectAuthority, SandboxEffectOutcome, SandboxExecution, SandboxGrantStatus,
    SandboxSnapshot, SandboxToolRecovery, SandboxToolRecoverySelection, SessionStore,
    VersionedSourceBinding,
};
use aw_tools::filesystem::{access::Action, schema::Tool as FsTool};
use aw_tools_adapter::{
    native_filesystem::{
        reopen_source_artifacts, validate_source_artifact_state, ExclusionSpec, GrantIdentity,
        GrantSpec,
    },
    LocalToolOptions,
};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Trusted metadata-only final detach, using the original Sandbox source
/// mutation owner. The host independently authorizes the exact old source
/// and already-empty private layout, protecting it from aliasing, external
/// writers and ambient source mappings through commit. No materialization
/// is removed and no Agent is activated. The authorization callback must
/// not reenter the Store while its transaction is held.
pub fn detach_final_source<F: crate::source_checkpoint::GitSourceFence>(
    store: &SessionStore,
    handle: &SourceDriverHandle<GitSourceDriver>,
    actor: &PrincipalId,
    sandbox: &str,
    name: &SourceName,
    expected_rev: u64,
    authorize: impl FnOnce(
        &SandboxSnapshot,
        &VersionedSourceBinding,
    ) -> Result<(F, GitSourceFreeLayout)>,
) -> Result<SandboxSnapshot> {
    store.sandbox_sources_detach_final(actor, sandbox, name, expected_rev, |snapshot, binding| {
        let driver = store.source_driver(handle)?;
        let (fence, layout) = authorize(snapshot, binding)?;
        let [location] = fence.locations() else {
            return Err(Error::Denied(
                "final Git detach requires one independently approved source",
            ));
        };
        if snapshot.tool_root != layout.root || snapshot.working_directory != layout.root {
            return Err(Error::Denied(
                "approved source-free layout differs from Sandbox",
            ));
        }
        let pins = driver.verify_source_free_layout(location, &binding.binding, &layout)?;
        Ok((pins, fence))
    })
}

/// A trusted host's independently authorized source selection. The
/// observed binding revision, exact physical location and ceiling must
/// come from the host's current authority, not persisted grant evidence.
#[derive(Clone, Debug)]
pub struct GitSourceApprovalDirectory {
    pub location: GitSourceLocation,
    pub binding_rev: u64,
    pub access: SourceAccess,
}

const MAX_APPROVAL_CONFIG_BYTES: usize = 1024 * 1024;

/// Host-owned configuration, never a serialized durable SandboxGrant.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfiguredGitApproval {
    version: u16,
    store_directory: PathBuf,
    store_generation: u64,
    manager: PrincipalId,
    actor: PrincipalId,
    agent_ref: String,
    sandbox_ref: String,
    sandbox_rev: u64,
    policy_ref: String,
    base: PathBuf,
    directories: Vec<ConfiguredGitDirectory>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfiguredGitDirectory {
    name: SourceName,
    repository: PathBuf,
    repository_identity: (u64, u64),
    binding_rev: u64,
    access: SourceAccess,
}

// A complete verified repository set. Git administration is protected even
// when another named root shares the backing repository.
fn git_layout_exclusions(repos: &[Repository]) -> Result<Vec<ExclusionSpec>> {
    let mut identities = BTreeSet::new();
    let mut exclusions = Vec::new();
    for repo in repos {
        if !identities.insert(
            repo.root_identity()
                .map_err(|_| Error::Conflict("Git checkout identity unavailable"))?,
        ) {
            return Err(Error::Denied("native source roots alias the same checkout"));
        }
        for admin in [
            repo.root().join(".git"),
            repo.git_dir().to_owned(),
            repo.common_dir().to_owned(),
        ] {
            if repos.iter().any(|other| other.root().starts_with(&admin)) {
                return Err(Error::Denied(
                    "native source root overlaps Git administration",
                ));
            }
            if repos.iter().any(|other| admin.starts_with(other.root())) {
                exclusions.push(ExclusionSpec::read_write(admin));
            }
        }
    }
    Ok(exclusions)
}

fn validate_git_artifact_state(repos: &[Repository], state_root: &Path) -> Result<()> {
    validate_source_artifact_state(
        state_root,
        repos
            .iter()
            .flat_map(|repo| [repo.root(), repo.git_dir(), repo.common_dir()]),
    )
}

/// Reuse the driver actually registered in the Store. No additional
/// registry, provider lookup, credential owner or host policy publication.
pub struct GitNativeSourceResolver {
    store: SessionStore,
    driver: SourceDriverHandle<GitSourceDriver>,
}

impl GitNativeSourceResolver {
    pub fn new(store: SessionStore, driver: SourceDriverHandle<GitSourceDriver>) -> Result<Self> {
        store.source_driver(&driver)?;
        Ok(Self { store, driver })
    }

    /// Explicit trusted-host activation of `root/git-native.json`. The
    /// same existing private root owns the source artifact namespaces.
    /// The caller must independently protect that root, its ancestors and
    /// the selected physical layout against other writers/mount aliases.
    /// No ambient discovery, implicit grant, recovery or Agent activation.
    /// Each activation issues a fresh live authority; revoke the old owner
    /// before changing configuration or reactivating. Editing the file alone
    /// does not withdraw a live decision.
    pub fn provision_configured_approval(
        &self,
        manager: &PrincipalId,
        root: &Path,
    ) -> Result<NativeSourceApproval> {
        self.provision_configured_approval_with(manager, root, |_, _, _| Ok(()))
            .map(|(approval, ())| approval)
    }

    /// Compose another independently authorized host operation with this
    /// configured decision. Acquire its live physical fence after Store
    /// authorization, before probing any Git checkout or artifact namespace.
    /// The returned fence and approval must both survive that operation.
    pub fn provision_configured_approval_with<F>(
        &self,
        manager: &PrincipalId,
        root: &Path,
        authorize: impl FnOnce(
            &SandboxSnapshot,
            &NativeSourceApprovalSpec<GitSourceApprovalDirectory>,
            &[VersionedSourceBinding],
        ) -> Result<F>,
    ) -> Result<(NativeSourceApproval, F)> {
        if !aw_platform::secure_private_files_supported()
            || !root.is_absolute()
            || root.canonicalize()? != root
        {
            return Err(Error::Denied(
                "configured Git approval requires canonical private host state",
            ));
        }
        aw_platform::validate_private_dir(root)?;
        let file = aw_platform::open_private_existing(&root.join("git-native.json"), false)?;
        let mut bytes = Vec::new();
        file.take(MAX_APPROVAL_CONFIG_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_APPROVAL_CONFIG_BYTES {
            return Err(Error::Limit("Git approval configuration exceeds 1 MiB"));
        }
        let config: ConfiguredGitApproval = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Config("invalid Git native approval configuration".into()))?;
        let process = self.store.local_process_info()?;
        let store_directory = self.store.directory().ok_or(Error::Denied(
            "configured Git approval requires a durable Store",
        ))?;
        if config.version != 1
            || &config.manager != manager
            || process.process_kind != "owner_lock"
            || process.state != "active"
            || process.store_generation != config.store_generation
            || store_directory.canonicalize()? != config.store_directory
        {
            return Err(Error::Denied(
                "configured Git approval differs from its host manager or Store",
            ));
        }
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce)
            .map_err(|_| Error::Storage("Git approval authority randomness unavailable"))?;
        let authority_ref = format!("host:git-native:{}", u128::from_be_bytes(nonce));
        let spec = NativeSourceApprovalSpec {
            agent_ref: config.agent_ref,
            actor: config.actor,
            policy_ref: config.policy_ref,
            authority_ref,
            authority_generation: 1,
            base: config.base,
            directories: config
                .directories
                .into_iter()
                .map(|directory| GitSourceApprovalDirectory {
                    location: GitSourceLocation {
                        name: directory.name,
                        repository: directory.repository,
                        repository_identity: directory.repository_identity,
                    },
                    binding_rev: directory.binding_rev,
                    access: directory.access,
                })
                .collect(),
        };
        self.provision_approval_in(
            manager,
            config.sandbox_rev,
            Some(&config.sandbox_ref),
            spec,
            root,
            authorize,
        )
    }

    /// Provision a live native approval from explicit, independently
    /// authorized physical sources. The trusted host transfers its
    /// approval decision to the returned owner (withdraw with revoke);
    /// it must exclude aliases/mounts and protect artifact state. Never
    /// infer this approval from a stored SandboxGrant or a model path.
    /// Only durable artifact namespaces are provisioned, no source effect.
    /// Grant publication is a separate existing CAS operation.
    pub fn provision_approval(
        &self,
        manager: &PrincipalId,
        expected_sandbox_rev: u64,
        spec: NativeSourceApprovalSpec<GitSourceApprovalDirectory>,
        state_root: &Path,
    ) -> Result<NativeSourceApproval> {
        self.provision_approval_in(
            manager,
            expected_sandbox_rev,
            None,
            spec,
            state_root,
            |_, _, _| Ok(()),
        )
        .map(|(approval, ())| approval)
    }

    fn provision_approval_in<F>(
        &self,
        manager: &PrincipalId,
        expected_sandbox_rev: u64,
        expected_sandbox: Option<&str>,
        mut spec: NativeSourceApprovalSpec<GitSourceApprovalDirectory>,
        state_root: &Path,
        authorize: impl FnOnce(
            &SandboxSnapshot,
            &NativeSourceApprovalSpec<GitSourceApprovalDirectory>,
            &[VersionedSourceBinding],
        ) -> Result<F>,
    ) -> Result<(NativeSourceApproval, F)> {
        spec.validate(&self.store)?;
        spec.directories
            .sort_unstable_by(|a, b| a.location.name.as_str().cmp(b.location.name.as_str()));
        let names = spec
            .directories
            .iter()
            .map(|directory| directory.location.name.clone())
            .collect::<Vec<_>>();
        let (sandbox, sources) = self.store.sandbox_grant_sources(
            manager,
            &spec.actor,
            &spec.agent_ref,
            expected_sandbox_rev,
            &names,
        )?;
        if expected_sandbox.is_some_and(|expected| expected != sandbox.canonical_ref)
            || sandbox.tool_root != spec.base
            || (spec.directories.len() == 1
                && spec.directories[0].location.repository == spec.base
                && sandbox.sources.len() != 1)
        {
            return Err(Error::Denied(
                "native source approval differs from Sandbox layout",
            ));
        }
        // Match the complete configured set before any physical probes, even
        // from the independently authorized operation callback.
        for (source, directory) in sources.iter().zip(&spec.directories) {
            crate::source_filesystem::require_named_root(
                &spec.base,
                &directory.location.name,
                &directory.location.repository,
                names.len(),
            )?;
            if source.binding.name != directory.location.name
                || source.binding_rev != directory.binding_rev
                || !source.binding.access.permits(directory.access)
                || &source.binding.driver != self.driver.driver()
            {
                return Err(Error::Denied("approved native source binding changed"));
            }
        }
        let fence = authorize(&sandbox, &spec, &sources)?;
        if expected_sandbox.is_some() {
            // Config and artifact state must not be inside the layout, even
            // a sibling outside this independently selected grant. Check only
            // after the original manager/actor/Store selection is authorized.
            validate_source_artifact_state(state_root, [spec.base.as_path()])?;
        }
        let driver = self.store.source_driver(&self.driver)?;
        let mut repos = Vec::with_capacity(sources.len());
        // Verify the entire selected set before creating artifact state.
        for (source, directory) in sources.iter().zip(&spec.directories) {
            let location = &directory.location;
            repos.push(driver.verify_materialization(
                &source.binding,
                &location.repository,
                location.repository_identity,
            )?);
        }
        // Validate every selected checkout and Git admin directory before the
        // first durable namespace is created. Per-root validation alone can
        // mutate a later source or its Git backing before failing on it.
        git_layout_exclusions(&repos)?;
        validate_git_artifact_state(&repos, state_root)?;
        let directories = sources
            .iter()
            .zip(&spec.directories)
            .map(|(source, directory)| {
                let grant = match directory.access {
                    SourceAccess::ReadOnly => GrantSpec::read(&directory.location.repository),
                    SourceAccess::ReadWrite => {
                        GrantSpec::read_write(&directory.location.repository)
                    }
                };
                NativeSourceDirectorySpec::durable(source.clone(), grant, state_root)
            })
            .collect::<Result<Vec<_>>>()?;
        let approval = NativeSourceApproval::pin(
            self.store.clone(),
            NativeSourceApprovalSpec {
                agent_ref: spec.agent_ref,
                actor: spec.actor,
                policy_ref: spec.policy_ref,
                authority_ref: spec.authority_ref,
                authority_generation: spec.authority_generation,
                base: spec.base,
                directories,
            },
        )?;
        {
            let guard = approval.read()?;
            for (root, directory) in guard.roots().iter().zip(&spec.directories) {
                let location = &directory.location;
                if root.physical.path != location.repository
                    || (root.physical.volume, root.physical.object) != location.repository_identity
                {
                    return Err(Error::Conflict(
                        "approved native source changed during pinning",
                    ));
                }
            }
            // No stale metadata can silently follow a name during setup.
            // The existing grant CAS rechecks the exact binding pins again.
            let (current, current_sources) = self.store.sandbox_grant_sources(
                manager,
                &guard.policy().actor,
                &guard.policy().agent_ref,
                expected_sandbox_rev,
                &names,
            )?;
            if current != sandbox
                || current_sources != sources
                || guard.policy().sandbox_ref != sandbox.canonical_ref
            {
                return Err(Error::Conflict(
                    "native source approval changed during provisioning",
                ));
            }
        }
        drop(repos);
        Ok((approval, fence))
    }

    /// Blocking, read-only preparation. Called by the native route on a
    /// worker with an independently resolved live host policy guard and
    /// physical directory approval. The exact Store grant is checked
    /// before inspecting protected Git materialization metadata.
    pub fn resolve(
        &self,
        ctx: &ToolContext,
        tool: FsTool,
        authority: &impl SandboxEffectAuthority,
        base: &Path,
        roots: &[NativeSourceRoot],
    ) -> Result<ApprovedSourceLayout> {
        let policy = authority.policy();
        if ctx
            .execution_owner
            .as_ref()
            .is_none_or(|owner| owner.canonical_ref() != policy.agent_ref)
            || ctx.identity.actor != ctx.actor
            || policy.actor != ctx.actor
        {
            return Err(Error::Denied(
                "Git native authority belongs to another invocation",
            ));
        }
        let write = tool.action() != Action::Read;
        let observation = self.store.sandbox_grant_preflight(
            &ctx.actor,
            &policy.agent_ref,
            policy,
            authority.tool_ceiling(),
        )?;
        if observation.sources.len() != roots.len() || roots.is_empty() {
            return Err(Error::Denied(
                "Git native approval requires its complete named source set",
            ));
        }
        if write
            && !observation
                .sources
                .iter()
                .any(|source| source.access == SourceAccess::ReadWrite)
        {
            return Err(Error::Denied("Git native mutation has no writable source"));
        }
        let mut selected = Vec::with_capacity(roots.len());
        for (mut source, root) in observation.sources.into_iter().zip(roots) {
            if source.binding != root.source.binding {
                return Err(Error::Denied(
                    "Git source identity differs from independent approval",
                ));
            }
            if !write {
                source.access = SourceAccess::ReadOnly;
            }
            selected.push(NativeSourceRoot {
                source,
                physical: root.physical.clone(),
                private_artifacts: root.private_artifacts.clone(),
            });
        }
        self.pin_layout(base, selected)
    }

    fn pin_layout(
        &self,
        base: &Path,
        roots: Vec<NativeSourceRoot>,
    ) -> Result<ApprovedSourceLayout> {
        let driver = self.store.source_driver(&self.driver)?;
        let mut repos = Vec::with_capacity(roots.len());
        for root in &roots {
            if &root.source.binding.binding.driver != self.driver.driver() {
                return Err(Error::Denied(
                    "Git source identity differs from registered driver",
                ));
            }
            let physical = &root.physical;
            let repo = driver.verify_materialization(
                &root.source.binding.binding,
                &physical.path,
                (physical.volume, physical.object),
            )?;
            repos.push(repo);
        }
        let exclusions = git_layout_exclusions(&repos)?;
        let layout = Self::pin_verified_layout(base, roots, exclusions, false);
        // Keep verified Git pins through actual capability preparation.
        drop(repos);
        layout
    }

    fn pin_verified_layout(
        base: &Path,
        roots: Vec<NativeSourceRoot>,
        exclusions: Vec<ExclusionSpec>,
        recover: bool,
    ) -> Result<ApprovedSourceLayout> {
        if recover {
            // The lower artifact owner reconciles exact records using
            // descriptor-relative paths. Its cleanup is not a model
            // filesystem operation; reject any record outside the
            // original writable source or overlapping Git exclusions.
            for root in &roots {
                for record in root
                    .private_artifacts
                    .records()
                    .map_err(|_| Error::Conflict("native artifact records unavailable"))?
                {
                    let valid_parent = record.parent_path.is_empty()
                        || (aw_tools::filesystem::path::validate(&record.parent_path).is_ok()
                            && !aw_tools::filesystem::path::is_absolute(&record.parent_path)
                            && !aw_tools::filesystem::path::is_unc(&record.parent_path));
                    if root.source.access != SourceAccess::ReadWrite
                        || !valid_parent
                        || aw_tools::filesystem::path::validate(&record.name).is_err()
                        || record.name == "."
                        || record.name.contains('/')
                        || aw_tools::filesystem::path::is_absolute(&record.name)
                    {
                        return Err(Error::Conflict(
                            "artifact record exceeds original native custody",
                        ));
                    }
                    let parent = if record.parent_path == "." {
                        ""
                    } else {
                        &record.parent_path
                    };
                    let path = root.physical.path.join(parent).join(&record.name);
                    if exclusions.iter().any(|excluded| {
                        path.starts_with(&excluded.path) || excluded.path.starts_with(&path)
                    }) {
                        return Err(Error::Conflict(
                            "artifact recovery overlaps protected Git administration",
                        ));
                    }
                }
            }
        }
        ApprovedSourceLayout::pin(base, roots, exclusions)
    }

    /// Recover an original native tool only. Independent authorization must
    /// guarantee permanent quiescence of the original native worker, exact
    /// source/destination and original durable artifact namespace custody,
    /// excluding surviving or external writers and aliases through release.
    /// An empty registry is not proof. This never repeats the tool or
    /// changes its terminal result or broader Run hazard.
    pub fn recover<F: crate::source_checkpoint::GitSourceFence>(
        &self,
        registry: &SandboxDriverRegistry,
        native: &SandboxDriverHandle<NativeFilesystemDriver>,
        manager: &PrincipalId,
        target: NativeSourceRecoveryTarget<'_>,
        authorize: impl FnOnce(&SandboxToolRecovery) -> Result<(F, PathBuf)>,
    ) -> Result<SandboxEffectOutcome> {
        Self::recover_in(
            &self.store,
            manager,
            target,
            None,
            SandboxToolRecoverySelection {
                driver: &native.descriptor().driver,
                sandbox_ref: None,
                effects: &[ToolEffect::Read, ToolEffect::Write],
                source: &aw_agent_core::ToolSource::trusted_host(),
                execution_site: None,
            },
            || {
                registry.reader()?.get(native)?;
                Ok(self.driver.clone())
            },
            authorize,
        )
    }

    /// Exact offline consumer. Acquire registered actual drivers only
    /// for missing physical proof; original terminal replay needs neither
    /// Git discovery nor access to old artifact directories.
    pub fn recover_configured<F: crate::source_checkpoint::GitSourceFence>(
        store: &SessionStore,
        manager: &PrincipalId,
        target: NativeSourceRecoveryTarget<'_>,
        sandbox: &str,
        authorize: impl FnOnce(&SandboxToolRecovery) -> Result<(F, PathBuf)>,
    ) -> Result<SandboxEffectOutcome> {
        Self::recover_configured_in(store, manager, target, None, sandbox, authorize)
    }

    /// Exact offline recovery of one originally claimed native Graph node.
    /// The same independent Git/artifact fence and inspector serve both
    /// routes. The outer constructor never substitutes for the node.
    pub fn recover_configured_graph_node<F: crate::source_checkpoint::GitSourceFence>(
        store: &SessionStore,
        manager: &PrincipalId,
        target: NativeSourceRecoveryTarget<'_>,
        coordinate: usize,
        sandbox: &str,
        authorize: impl FnOnce(&SandboxToolRecovery) -> Result<(F, PathBuf)>,
    ) -> Result<SandboxEffectOutcome> {
        Self::recover_configured_in(store, manager, target, Some(coordinate), sandbox, authorize)
    }

    fn recover_configured_in<F: crate::source_checkpoint::GitSourceFence>(
        store: &SessionStore,
        manager: &PrincipalId,
        target: NativeSourceRecoveryTarget<'_>,
        coordinate: Option<usize>,
        sandbox: &str,
        authorize: impl FnOnce(&SandboxToolRecovery) -> Result<(F, PathBuf)>,
    ) -> Result<SandboxEffectOutcome> {
        let key = aw_agent_core::DriverKey::builtin("native-filesystem")?;
        Self::recover_in(
            store,
            manager,
            target,
            coordinate,
            SandboxToolRecoverySelection {
                driver: &key,
                sandbox_ref: Some(sandbox),
                effects: &[ToolEffect::Read, ToolEffect::Write],
                source: &aw_agent_core::ToolSource::trusted_host(),
                execution_site: None,
            },
            || {
                let registry = SandboxDriverRegistry::new();
                let native = registry.register(key.clone(), NativeFilesystemDriver)?;
                registry.reader()?.get(&native)?;
                store.source_driver_handle::<GitSourceDriver>(&aw_agent_core::DriverKey::builtin(
                    "git",
                )?)
            },
            authorize,
        )
    }

    fn recover_in<F: crate::source_checkpoint::GitSourceFence>(
        store: &SessionStore,
        manager: &PrincipalId,
        target: NativeSourceRecoveryTarget<'_>,
        coordinate: Option<usize>,
        selection: SandboxToolRecoverySelection<'_>,
        git: impl FnOnce() -> Result<SourceDriverHandle<GitSourceDriver>>,
        authorize: impl FnOnce(&SandboxToolRecovery) -> Result<(F, PathBuf)>,
    ) -> Result<SandboxEffectOutcome> {
        let expected_driver = selection.driver;
        let mut held = None;
        let mut physical = None;
        let inspect = |recovery: &SandboxToolRecovery| {
            let tool = FsTool::from_name(recovery.tool_name()).ok_or(Error::Denied(
                "original effect is not a native filesystem tool",
            ))?;
            let effect = if tool.action() == Action::Read {
                ToolEffect::Read
            } else {
                ToolEffect::Write
            };
            if &recovery.driver != expected_driver
                || recovery.effect() != effect
                || recovery.node.as_ref().map(|node| node.binding.coordinate) != coordinate
                || !recovery.source.is_trusted_host()
                || tool == FsTool::Remove
            {
                return Err(Error::Denied(
                    "original effect is not owned by this native driver",
                ));
            }
            let git = git()?;
            let (fence, state_root) = authorize(recovery)?;
            held = Some(fence);
            let locations = held
                .as_ref()
                .expect("live native recovery fence")
                .locations();
            if locations.len() != recovery.sources.len() || locations.is_empty() {
                return Err(Error::Conflict(
                    "native recovery requires exact original source set",
                ));
            }
            let sandbox = store.sandbox_get(manager, &recovery.sandbox_ref)?;
            if locations.len() == 1
                && locations[0].repository == sandbox.tool_root
                && sandbox.sources.len() != 1
            {
                return Err(Error::Denied(
                    "native recovery cannot grant a multi-source parent",
                ));
            }
            let driver = store.source_driver(&git)?;
            let mut repos = Vec::with_capacity(locations.len());
            for (source, location) in recovery.sources.iter().zip(locations) {
                if source.binding.binding.name != location.name {
                    return Err(Error::Conflict("native recovery source selection changed"));
                }
                crate::source_filesystem::require_named_root(
                    &sandbox.tool_root,
                    &location.name,
                    &location.repository,
                    locations.len(),
                )?;
                if &source.binding.binding.driver != git.driver() {
                    return Err(Error::Denied(
                        "Git source identity differs from registered driver",
                    ));
                }
                repos.push(driver.verify_materialization(
                    &source.binding.binding,
                    &location.repository,
                    location.repository_identity,
                )?);
            }
            let exclusions = git_layout_exclusions(&repos)?;
            validate_git_artifact_state(&repos, &state_root)?;
            let mut roots = Vec::with_capacity(locations.len());
            for (source, location) in recovery.sources.iter().zip(locations) {
                roots.push(NativeSourceRoot {
                    source: source.clone(),
                    physical: GrantIdentity {
                        path: location.repository.clone(),
                        volume: location.repository_identity.0,
                        object: location.repository_identity.1,
                        read: true,
                        write: source.access == SourceAccess::ReadWrite,
                    },
                    private_artifacts: reopen_source_artifacts(&location.repository, &state_root)?,
                });
            }
            let layout = Self::pin_verified_layout(&sandbox.tool_root, roots, exclusions, true)?;
            // Retain the original driver's checkout and administration
            // identities alongside the capability through settlement.
            physical = Some((layout, repos));
            physical
                .as_ref()
                .expect("pinned native recovery layout")
                .0
                .reconcile_private_artifacts()?;
            Ok(SandboxEffectOutcome::Settled)
        };
        let result = match coordinate {
            Some(coordinate) => store.sandbox_recover_graph_node_exact(
                manager,
                target.agent_ref,
                target.turn_id,
                target.call_id,
                coordinate,
                selection,
                inspect,
            ),
            None => store.sandbox_recover_tool_source_effect_exact(
                manager,
                target.agent_ref,
                target.turn_id,
                target.call_id,
                selection,
                inspect,
            ),
        };
        // Keep the independent authority and physical pins through the
        // original source owner settlement (also on failure).
        drop(physical);
        drop(held);
        result
    }
}

pub use crate::source_authority::SourceToolRecoveryTarget as NativeSourceRecoveryTarget;

/// Explicit native-only host composition for independently approved named
/// Git sources. All runtime instances use the same live approval, not a copy of
/// the durable grant. No automatic approval, activation or policy registry.
#[derive(Clone)]
pub struct GitNativeSourceHost {
    approval: NativeSourceApproval,
    resolver: Arc<GitNativeSourceResolver>,
    registry: SandboxDriverRegistry,
    native: SandboxDriverHandle<NativeFilesystemDriver>,
}

/// A current independent policy guard and the actual verified Git pins.
struct GitExecutionFence {
    approval: NativeSourceApprovalGuard,
    _repositories: Vec<Repository>,
}

impl SandboxEffectAuthority for GitExecutionFence {
    fn policy(&self) -> &aw_store::SandboxGrantPolicy {
        self.approval.policy()
    }
    fn tool_ceiling(&self) -> &[aw_agent_core::NamedSourceAccess] {
        self.approval.tool_ceiling()
    }
}

impl GitNativeSourceHost {
    pub fn new(
        approval: NativeSourceApproval,
        git: SourceDriverHandle<GitSourceDriver>,
        registry: SandboxDriverRegistry,
        native: SandboxDriverHandle<NativeFilesystemDriver>,
    ) -> Result<Self> {
        registry.reader()?.get(&native)?;
        let resolver = GitNativeSourceResolver::new(approval.store().clone(), git.clone())?;
        {
            let guard = approval.read()?;
            let driver = approval.store().source_driver(&git)?;
            for root in guard.roots() {
                let physical = &root.physical;
                driver.verify_materialization(
                    &root.source.binding.binding,
                    &physical.path,
                    (physical.volume, physical.object),
                )?;
            }
        }
        Ok(Self {
            approval,
            resolver: Arc::new(resolver),
            registry,
            native,
        })
    }

    /// Independent host approval and durable grant issuance are explicit
    /// operations, separate from the Actor's eventual activation.
    pub fn publish_grant(
        &self,
        manager: &PrincipalId,
        expected_revision: u64,
    ) -> Result<SandboxGrantStatus> {
        self.approval.publish_grant(manager, expected_revision)
    }

    /// Close new live admission without waiting for admitted workers.
    pub fn request_withdrawal(&self) -> bool {
        self.approval.request_withdrawal()
    }

    pub async fn revoke(&self) -> bool {
        self.approval.revoke().await
    }

    pub fn preflight(
        &self,
        store: &SessionStore,
        actor: &PrincipalId,
        agent: &AgentExecutionBinding,
        tools: &LocalToolOptions,
    ) -> Result<()> {
        let observed = self.approval.preflight(store, actor, agent)?;
        let writable = observed
            .sources
            .iter()
            .any(|source| source.access == SourceAccess::ReadWrite);
        if tools.selected_tools.is_empty()
            || tools.selected_tools.iter().any(|name| {
                FsTool::from_name(name).is_none_or(|tool| {
                    tool == FsTool::Remove || (tool.action() != Action::Read && !writable)
                })
            })
        {
            return Err(Error::Denied(
                "Git native host tool selection exceeds source approval",
            ));
        }
        Ok(())
    }

    pub fn resolve(
        &self,
        ctx: &ToolContext,
        tool: FsTool,
    ) -> Result<NativeSourceAdmission<NativeSourceApprovalGuard>> {
        let guard = self.approval.read()?;
        if ctx.execution_owner.as_ref() != Some(guard.agent()) {
            return Err(Error::Denied(
                "Git native approval belongs to another Agent binding",
            ));
        }
        let layout = self
            .resolver
            .resolve(ctx, tool, &guard, guard.base(), guard.roots())?;
        Ok(NativeSourceAdmission {
            authority: guard,
            layout,
        })
    }

    /// Build the original native-only Agent and its matching live source
    /// execution scope together. No implicit durable admission change.
    pub fn build_execution(
        &self,
        store: &SessionStore,
        actor: PrincipalId,
        execution: AgentExecutionBinding,
        builder: AgentBuilder,
        options: &LocalToolOptions,
    ) -> Result<(Agent, SandboxExecution)> {
        let mut agent = self
            .configure(store, actor.clone(), execution.clone(), builder, options)?
            .build()?;
        agent.bind_execution_owner(execution.clone())?;
        let base = self.approval.read()?.base().to_owned();
        let base_pin = aw_platform::DirectoryIdentity::open(&base)?;
        let host = self.clone();
        let source = SandboxExecution::from_trusted_host(
            store,
            actor,
            execution,
            base,
            &agent.tool_binding_authority(),
            move || host.execution_fence(&base_pin),
        )?;
        Ok((agent, source))
    }

    fn execution_fence(
        &self,
        base_pin: &aw_platform::DirectoryIdentity,
    ) -> Result<GitExecutionFence> {
        let approval = self.approval.read()?;
        if !base_pin.is_current(approval.base())? {
            return Err(Error::Denied("native source base identity changed"));
        }
        self.registry.reader()?.get(&self.native)?;
        let driver = self.resolver.store.source_driver(&self.resolver.driver)?;
        let mut repositories = Vec::with_capacity(approval.roots().len());
        for root in approval.roots() {
            let physical = &root.physical;
            repositories.push(driver.verify_materialization(
                &root.source.binding.binding,
                &physical.path,
                (physical.volume, physical.object),
            )?);
        }
        Ok(GitExecutionFence {
            approval,
            _repositories: repositories,
        })
    }

    fn native_route(
        &self,
        store: &SessionStore,
        agent: AgentExecutionBinding,
        builder: &AgentBuilder,
        options: &LocalToolOptions,
    ) -> Result<
        Arc<
            impl aw_tools_adapter::native_filesystem::SourceGraphExecutor
                + aw_agent_core::Extension
                + 'static,
        >,
    > {
        let host = self.clone();
        Ok(Arc::new(SourceFilesystemRoute::new(
            store.clone(),
            agent,
            self.registry.clone(),
            self.native.clone(),
            builder.tool_binding_authority(),
            options.limits,
            move |ctx, tool| host.resolve(ctx, tool),
        )?))
    }

    /// Explicit foreground native Graph on the original approved Git
    /// source scope and existing JobRuntime. The ordinary native-only CLI
    /// continues to use its narrower configuration.
    pub fn build_execution_with_graph(
        &self,
        store: &SessionStore,
        actor: PrincipalId,
        execution: AgentExecutionBinding,
        builder: AgentBuilder,
        options: &LocalToolOptions,
    ) -> Result<(
        Agent,
        SandboxExecution,
        Arc<aw_tools_adapter::job_runtime::JobRuntime>,
    )> {
        let mut native_options = options.clone();
        native_options.selected_tools.retain(|name| name != "batch");
        self.preflight(store, &actor, &execution, &native_options)?;
        let route = self.native_route(store, execution.clone(), &builder, options)?;
        let configured = aw_tools_adapter::configure_source_graph(
            builder,
            actor.clone(),
            options,
            route.clone(),
        )?;
        let jobs = configured
            .job_runtime
            .ok_or(Error::Storage("source Graph Job runtime unavailable"))?;
        let mut agent = configured.builder.shared_extension(route).build()?;
        agent.bind_execution_owner(execution.clone())?;
        let base = self.approval.read()?.base().to_owned();
        if base != options.root {
            return Err(Error::Denied("source Graph root differs from approval"));
        }
        let base_pin = aw_platform::DirectoryIdentity::open(&base)?;
        let host = self.clone();
        let source = SandboxExecution::from_trusted_host(
            store,
            actor,
            execution,
            base,
            &agent.tool_binding_authority(),
            move || host.execution_fence(&base_pin),
        )?;
        Ok((agent, source, jobs))
    }

    /// Compose the fixed native route and one registered foreground Process
    /// driver with the same independent Git approval and exact Agent scope.
    /// No ambient shell, native root, or additional Graph grant.
    pub fn build_execution_with_process<D: crate::source_process::SourceProcessDriver>(
        &self,
        store: &SessionStore,
        actor: PrincipalId,
        execution: AgentExecutionBinding,
        builder: AgentBuilder,
        options: &LocalToolOptions,
        process: SandboxDriverHandle<D>,
    ) -> Result<(
        Agent,
        SandboxExecution,
        Arc<aw_tools_adapter::job_runtime::JobRuntime>,
    )> {
        if options.selected_tools.iter().any(|name| name == "batch") {
            return Err(Error::Denied(
                "select the explicit source Process Graph host",
            ));
        }
        self.build_native_process(store, actor, execution, builder, options, (process, None))
    }

    /// Explicit fixed foreground native/Process Graph. Both use the
    /// original independent Git approval and registered Process driver.
    pub fn build_execution_with_process_graph<D: crate::source_process::SourceProcessDriver>(
        &self,
        store: &SessionStore,
        actor: PrincipalId,
        execution: AgentExecutionBinding,
        builder: AgentBuilder,
        options: &LocalToolOptions,
        process: SandboxDriverHandle<D>,
    ) -> Result<(
        Agent,
        SandboxExecution,
        Arc<aw_tools_adapter::job_runtime::JobRuntime>,
    )> {
        if !options.selected_tools.iter().any(|name| name == "batch") {
            return Err(Error::Denied(
                "source Process Graph requires explicit batch selection",
            ));
        }
        self.build_native_process(store, actor, execution, builder, options, (process, None))
    }

    /// Explicit derived signing-proof consumer on the original native/
    /// Process host. Select batch explicitly to include the same Process
    /// in a foreground Graph. No raw credential or fallback execution.
    pub fn build_execution_with_signing_process<D: crate::source_process::SourceProcessDriver>(
        &self,
        store: &SessionStore,
        actor: PrincipalId,
        execution: AgentExecutionBinding,
        builder: AgentBuilder,
        options: &LocalToolOptions,
        process: crate::source_process::SigningSourceProcess<D>,
    ) -> Result<(
        Agent,
        SandboxExecution,
        Arc<aw_tools_adapter::job_runtime::JobRuntime>,
    )> {
        self.build_native_process(
            store,
            actor,
            execution,
            builder,
            options,
            (process.driver, Some(process.projection)),
        )
    }

    fn build_native_process<D: crate::source_process::SourceProcessDriver>(
        &self,
        store: &SessionStore,
        actor: PrincipalId,
        execution: AgentExecutionBinding,
        builder: AgentBuilder,
        options: &LocalToolOptions,
        (process, signing): (
            SandboxDriverHandle<D>,
            Option<crate::source_process::SigningProcessProjection>,
        ),
    ) -> Result<(
        Agent,
        SandboxExecution,
        Arc<aw_tools_adapter::job_runtime::JobRuntime>,
    )> {
        let mut native_options = options.clone();
        native_options
            .selected_tools
            .retain(|name| FsTool::from_name(name).is_some());
        self.preflight(store, &actor, &execution, &native_options)?;
        let native = self.native_route(store, execution.clone(), &builder, options)?;
        let base = self.approval.read()?.base().to_owned();
        if base != options.root {
            return Err(Error::Denied(
                "mixed source tool root differs from approval",
            ));
        }
        let base_pin = aw_platform::DirectoryIdentity::open(&base)?;
        let host = self.clone();
        let process = crate::source_process::SourceProcessHost::new(
            self.registry.clone(),
            process,
            move || host.execution_fence(&base_pin),
        )?;
        let process = match signing {
            Some(signing) => process.with_signing_projection(signing),
            None => process,
        };
        process.build_with_native(
            store,
            actor,
            execution,
            builder.shared_extension(native.clone()),
            options,
            Some(native),
        )
    }

    /// Construct the actual root-free tool route on the original builder.
    /// Does not open source-bearing Store/Turn admission.
    pub fn configure(
        &self,
        store: &SessionStore,
        actor: PrincipalId,
        agent: AgentExecutionBinding,
        builder: AgentBuilder,
        options: &LocalToolOptions,
    ) -> Result<AgentBuilder> {
        self.preflight(store, &actor, &agent, options)?;
        if options.operation_router.is_installed()? {
            return Err(Error::Denied(
                "Git native host cannot replace another operation router",
            ));
        }
        // The native executor is fixed to this builder; reusable options
        // cannot replace or clear the original claim-gated route.
        let route = self.native_route(store, agent, &builder, options)?;
        let builder =
            aw_tools_adapter::configure_routed_native(builder, actor, options, route.clone())?;
        Ok(builder.shared_extension(route))
    }
}
