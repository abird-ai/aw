//! Concrete Git-to-native realization. The original live host decision is
//! supplied by the authority owner; a stored SandboxGrant is never a grant
//! to probe or mount arbitrary Git materialization locators.
use crate::{
    source_approval::{NativeSourceApproval, NativeSourceApprovalGuard},
    source_filesystem::{
        ApprovedSourceLayout, NativeFilesystemDriver, NativeSourceAdmission, NativeSourceRoot,
        SourceFilesystemRoute,
    },
};
use aw_agent_core::{
    AgentBuilder, AgentExecutionBinding, Error, PrincipalId, Result, SandboxDriverHandle,
    SandboxDriverRegistry, SourceAccess, SourceDriverHandle, SourceName, ToolContext, ToolEffect,
    ToolSourceEvidence,
};
use aw_git::source_driver::{GitSourceDriver, GitSourceFreeLayout};
use aw_store::{
    SandboxEffectAuthority, SandboxEffectOutcome, SandboxGrantStatus, SandboxSnapshot,
    SandboxToolRecovery, SessionStore, VersionedSourceBinding,
};
use aw_tools::filesystem::{access::Action, schema::Tool as FsTool};
use aw_tools_adapter::{
    native_filesystem::{reopen_source_artifacts, ExclusionSpec, GrantIdentity},
    LocalToolOptions,
};
use std::{
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
        self.pin_layout(base, selected, false)
    }

    fn pin_layout(
        &self,
        base: &Path,
        roots: Vec<NativeSourceRoot>,
        recover: bool,
    ) -> Result<ApprovedSourceLayout> {
        let driver = self.store.source_driver(&self.driver)?;
        let mut repos = Vec::with_capacity(roots.len());
        let mut exclusions = Vec::new();
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
            // All approved roots must exclude Git administration, even
            // when linked worktrees share backing with another checkout.
            for admin in [
                repo.root().join(".git"),
                repo.git_dir().to_owned(),
                repo.common_dir().to_owned(),
            ] {
                if roots
                    .iter()
                    .any(|root| root.physical.path.starts_with(&admin))
                {
                    return Err(Error::Denied(
                        "native source root overlaps Git administration",
                    ));
                }
                if roots
                    .iter()
                    .any(|root| admin.starts_with(&root.physical.path))
                {
                    exclusions.push(ExclusionSpec::read_write(admin));
                }
            }
            repos.push(repo);
        }
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
        let layout = ApprovedSourceLayout::pin(base, roots, exclusions)?;
        // Retain every checkout pin through actual capability preparation.
        drop(repos);
        Ok(layout)
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
        let mut held = None;
        let mut physical = None;
        let result = self.store.sandbox_recover_tool_source_effect(
            manager,
            target.agent_ref,
            target.turn_id,
            target.call_id,
            |recovery| {
                registry.reader()?.get(native)?;
                let tool = FsTool::from_name(&recovery.original.call.name).ok_or(Error::Denied(
                    "original effect is not a native filesystem tool",
                ))?;
                let effect = if tool.action() == Action::Read {
                    ToolEffect::Read
                } else {
                    ToolEffect::Write
                };
                if recovery.driver != native.descriptor().driver
                    || recovery.original.effect != effect
                    || recovery.source != ToolSourceEvidence::trusted_host()
                    || tool == FsTool::Remove
                {
                    return Err(Error::Denied(
                        "original effect is not owned by this native driver",
                    ));
                }
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
                let sandbox = self.store.sandbox_get(manager, &recovery.sandbox_ref)?;
                if locations.len() == 1
                    && locations[0].repository == sandbox.tool_root
                    && sandbox.sources.len() != 1
                {
                    return Err(Error::Denied(
                        "native recovery cannot grant a multi-source parent",
                    ));
                }
                let mut roots = Vec::with_capacity(locations.len());
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
                    roots.push(NativeSourceRoot {
                        source: source.clone(),
                        physical: GrantIdentity {
                            path: location.repository.clone(),
                            volume: location.repository_identity.0,
                            object: location.repository_identity.1,
                            read: true,
                            write: source.access == SourceAccess::ReadWrite,
                        },
                        private_artifacts: reopen_source_artifacts(
                            &location.repository,
                            &state_root,
                        )?,
                    });
                }
                let layout = self.pin_layout(&sandbox.tool_root, roots, true)?;
                physical = Some(layout);
                physical
                    .as_ref()
                    .expect("pinned native recovery layout")
                    .reconcile_private_artifacts()?;
                Ok(SandboxEffectOutcome::Settled)
            },
        );
        // Keep the independent authority and physical pins through the
        // original source owner settlement (also on failure).
        drop(physical);
        drop(held);
        result
    }
}

/// Original journal identity, not a new recovery operation ID.
pub struct NativeSourceRecoveryTarget<'a> {
    pub agent_ref: &'a str,
    pub turn_id: u64,
    pub call_id: &'a str,
}

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
        // A fresh slot belongs to this exact builder, not to the reusable
        // options or a failed/retried activation.
        let mut options = options.clone();
        options.operation_router = Arc::new(Default::default());
        let host = self.clone();
        let route = Arc::new(SourceFilesystemRoute::new(
            store.clone(),
            agent,
            self.registry.clone(),
            self.native.clone(),
            builder.tool_binding_authority(),
            options.limits,
            move |ctx, tool| host.resolve(ctx, tool),
        )?);
        route.configure(builder, actor, &options)
    }
}
