//! Concrete Git-to-native realization. The original live host decision is
//! supplied by the authority owner; a stored SandboxGrant is never a grant
//! to probe or mount arbitrary Git materialization locators.
use crate::{
    source_approval::{NativeSourceApproval, NativeSourceApprovalGuard},
    source_filesystem::{
        ApprovedSourceDirectory, NativeFilesystemDriver, NativeSourceAdmission,
        SourceFilesystemRoute,
    },
};
use aw_agent_core::{
    AgentBuilder, AgentExecutionBinding, Error, PrincipalId, Result, SandboxDriverHandle,
    SandboxDriverRegistry, SourceAccess, SourceDriverHandle, ToolContext,
};
use aw_git::source_driver::GitSourceDriver;
use aw_store::{SandboxEffectAuthority, SandboxGrantStatus, SessionStore};
use aw_tools::filesystem::{access::Action, schema::Tool as FsTool};
use aw_tools_adapter::{
    native_filesystem::{ExclusionSpec, GrantIdentity, PrivateArtifactRegistry},
    LocalToolOptions,
};
use std::sync::Arc;

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
    pub fn resolve<A: SandboxEffectAuthority>(
        &self,
        ctx: &ToolContext,
        tool: FsTool,
        authority: A,
        physical: GrantIdentity,
        private_artifacts: Arc<PrivateArtifactRegistry>,
    ) -> Result<NativeSourceAdmission<A>> {
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
        let access = if tool.action() == Action::Read {
            SourceAccess::ReadOnly
        } else {
            SourceAccess::ReadWrite
        };
        if !physical.read || (access == SourceAccess::ReadWrite && !physical.write) {
            return Err(Error::Denied(
                "Git native access exceeds physical host approval",
            ));
        }
        let observation = self.store.sandbox_grant_preflight(
            &ctx.actor,
            &policy.agent_ref,
            policy,
            authority.tool_ceiling(),
        )?;
        let [source] = observation.sources.as_slice() else {
            return Err(Error::Denied(
                "Git native realization requires one authorized source",
            ));
        };
        if &source.binding.binding.driver != self.driver.driver() || !source.access.permits(access)
        {
            return Err(Error::Denied(
                "Git source identity or access is not authorized",
            ));
        }
        let driver = self.store.source_driver(&self.driver)?;
        let repo = driver.verify_materialization(
            &source.binding.binding,
            &physical.path,
            (physical.volume, physical.object),
        )?;

        // Native file operations do not own Git's private retention refs or
        // shared administration directories. A worktree's .git pointer is
        // excluded too. Reuse the platform's existing exclusion mechanism.
        let exclusions = [
            repo.root().join(".git"),
            repo.git_dir().to_owned(),
            repo.common_dir().to_owned(),
        ]
        .into_iter()
        .filter(|path| path.starts_with(repo.root()))
        .map(ExclusionSpec::read_write);
        let directory = ApprovedSourceDirectory::pin(
            source.binding.clone(),
            physical,
            access,
            private_artifacts,
            exclusions,
        )?;
        // Retain the original checkout pin until the capability has pinned
        // that exact physical object. No pathname reopening during launch.
        drop(repo);
        Ok(NativeSourceAdmission {
            authority,
            directory,
        })
    }
}

/// Explicit native-only host composition for one independently approved Git
/// source. All runtime instances use the same live approval, not a copy of
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
            let physical = guard.physical();
            approval
                .store()
                .source_driver(&git)?
                .verify_materialization(
                    &guard.source().binding,
                    &physical.path,
                    (physical.volume, physical.object),
                )?;
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
        let [source] = observed.sources.as_slice() else {
            return Err(Error::Denied(
                "Git native host requires one approved source",
            ));
        };
        if tools.selected_tools.is_empty()
            || tools.selected_tools.iter().any(|name| {
                FsTool::from_name(name).is_none_or(|tool| {
                    tool == FsTool::Remove
                        || (tool.action() != Action::Read
                            && source.access != SourceAccess::ReadWrite)
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
        let physical = guard.physical().clone();
        let private = guard.private_artifacts();
        self.resolver.resolve(ctx, tool, guard, physical, private)
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
