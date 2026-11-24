//! Concrete Git-to-native realization. The original live host decision is
//! supplied by the authority owner; a stored SandboxGrant is never a grant
//! to probe or mount arbitrary Git materialization locators.
use crate::source_filesystem::{ApprovedSourceDirectory, NativeSourceAdmission};
use aw_agent_core::{Error, Result, SourceAccess, SourceDriverHandle, ToolContext};
use aw_git::source_driver::GitSourceDriver;
use aw_store::{SandboxEffectAuthority, SessionStore};
use aw_tools::filesystem::{access::Action, schema::Tool as FsTool};
use aw_tools_adapter::native_filesystem::{ExclusionSpec, GrantIdentity, PrivateArtifactRegistry};
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
