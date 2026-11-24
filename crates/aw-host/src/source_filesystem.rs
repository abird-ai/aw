//! Source-aware direct native tools. Use the existing pinned filesystem
//! backend and canonical executor; no ambient root fallback or Process.
use crate::{source_authority::SourceToolAuthority, AuthenticatedToolGrantEvidence};
use aw_agent_core::{
    tool::{ToolBindingAuthority, WeakToolBindingAuthority},
    AgentBuilder, AgentExecutionBinding, Error, Extension, HookContext, HookEvent, HookFuture,
    NamedSourceAccess, Result, SandboxDriverHandle, SandboxDriverRegistry, SourceAccess, Tool,
    ToolContext, ToolEffect, ToolFuture,
};
use aw_store::{
    SandboxEffectAuthority, SandboxEffectOrigin, SandboxEffectOutcome, SandboxEffectPlan,
    SandboxEffectRequest, SessionStore, VersionedSourceBinding,
};
use aw_tools::{
    filesystem::{access::Action, schema::Tool as FsTool},
    OperationId,
};
use aw_tools_adapter::{
    native_filesystem::{
        CapabilitySet, ExclusionSpec, GrantIdentity, GrantSpec, PreparedGrant,
        PreparedNativeRequest, PrivateArtifactRegistry,
    },
    operation_router::OperationRouter,
    LocalToolLimits, LocalToolOptions,
};
use std::{marker::PhantomData, path::Path, sync::Arc};

/// Pinned, already narrowed native directory capability. The trusted
/// resolver independently authorizes the binding and physical root.
pub struct ApprovedSourceDirectory {
    binding: VersionedSourceBinding,
    identity: GrantIdentity,
    access: SourceAccess,
    capability: CapabilitySet,
}

impl ApprovedSourceDirectory {
    /// Pin the actual directory while the host's source authority is live.
    /// Exclusions narrow (never extend) the approved directory.
    pub fn pin(
        binding: VersionedSourceBinding,
        identity: GrantIdentity,
        access: SourceAccess,
        private_artifacts: Arc<PrivateArtifactRegistry>,
        exclusions: impl IntoIterator<Item = ExclusionSpec>,
    ) -> Result<Self> {
        if !identity.path.is_absolute()
            || identity.path.parent().is_none()
            || !identity.read
            || !binding.binding.access.permits(access)
            || (access == SourceAccess::ReadWrite && !identity.write)
        {
            return Err(Error::Denied(
                "native source directory is not independently authorized",
            ));
        }
        let grant = match access {
            SourceAccess::ReadOnly => GrantSpec::read(&identity.path),
            SourceAccess::ReadWrite => GrantSpec::read_write(&identity.path),
        };
        let capability = CapabilitySet::open_prepared_with_exclusions(
            &identity.path,
            [PreparedGrant::new(grant, private_artifacts)],
            exclusions,
        )
        .map_err(|_| Error::Denied("authorized source directory could not be pinned"))?;
        let actual = capability
            .identities()
            .map_err(|_| Error::Denied("source directory identity unavailable"))?;
        let [actual] = actual.as_slice() else {
            return Err(Error::Denied(
                "native source capability has unexpected roots",
            ));
        };
        if actual.path != identity.path
            || actual.volume != identity.volume
            || actual.object != identity.object
            || !actual.read
            || actual.write != (access == SourceAccess::ReadWrite)
            || capability.base() != Path::new(&identity.path)
        {
            return Err(Error::Conflict(
                "authorized source directory changed during preparation",
            ));
        }
        Ok(Self {
            binding,
            identity,
            access,
            capability,
        })
    }
}

/// Concrete native realization for the initial single-directory Sandbox.
/// Multi-source layouts and overlapping roots require an explicit complete
/// physical policy; never approximate them using the whole workspace.
pub struct NativeFilesystemDriver;

impl NativeFilesystemDriver {
    fn prepare(
        &self,
        plan: &SandboxEffectPlan,
        directory: ApprovedSourceDirectory,
    ) -> Result<CapabilitySet> {
        let [source] = plan.grant.sources.as_slice() else {
            return Err(Error::Denied(
                "native source realization requires exactly one directory",
            ));
        };
        if plan.sandbox.sources.len() != 1
            || source.binding != directory.binding
            || directory.identity.path != plan.sandbox.tool_root
            || source.access != directory.access
        {
            return Err(Error::Denied(
                "native source directory differs from the authorized Sandbox",
            ));
        }
        // No reopening a path after verification. The exact narrowed
        // descriptor-backed capability moves into the original worker.
        Ok(directory.capability)
    }
}

/// A live source-policy decision from the existing trusted host resolver.
/// Driver preparation additionally checks the exact selected binding and pin.
pub struct NativeSourceAdmission<A> {
    pub authority: A,
    pub directory: ApprovedSourceDirectory,
}

/// Explicit Agent-bound native route. The same object observes the original
/// post-journal ToolResult, so callers need no extra pending-lease map.
/// Unsupported operations, Graph children and other Agents fail closed.
pub struct SourceFilesystemRoute<A, F> {
    store: SessionStore,
    agent: AgentExecutionBinding,
    registry: SandboxDriverRegistry,
    driver: SandboxDriverHandle<NativeFilesystemDriver>,
    tool_authority: WeakToolBindingAuthority,
    limits: LocalToolLimits,
    resolve: Arc<F>,
    _authority: PhantomData<fn() -> A>,
}

impl<A, F> SourceFilesystemRoute<A, F>
where
    A: SandboxEffectAuthority + Send + 'static,
    F: Fn(&ToolContext, FsTool) -> Result<NativeSourceAdmission<A>> + Send + Sync + 'static,
{
    pub fn new(
        store: SessionStore,
        agent: AgentExecutionBinding,
        registry: SandboxDriverRegistry,
        driver: SandboxDriverHandle<NativeFilesystemDriver>,
        tool_authority: ToolBindingAuthority,
        limits: LocalToolLimits,
        resolve: F,
    ) -> Result<Self> {
        registry.reader()?.get(&driver)?;
        Ok(Self {
            store,
            agent,
            registry,
            driver,
            tool_authority: tool_authority.downgrade(),
            limits: limits.validate_direct()?,
            resolve: Arc::new(resolve),
            _authority: PhantomData,
        })
    }

    /// Configure the exact native-only host selection, without opening
    /// an ambient workspace root. Both schema registration and execution
    /// share these limits. No source grant or held Agent admission is minted.
    pub fn configure(
        self: Arc<Self>,
        builder: AgentBuilder,
        actor: aw_agent_core::PrincipalId,
        options: &LocalToolOptions,
    ) -> Result<AgentBuilder> {
        if !self
            .tool_authority
            .belongs_to(&builder.tool_binding_authority())
            || self.limits != options.limits
        {
            return Err(Error::Denied(
                "source route belongs to another tool authority or limit selection",
            ));
        }
        let builder = aw_tools_adapter::configure_routed_native(builder, actor, options)?;
        options.operation_router.install(self.clone())?;
        Ok(builder.shared_extension(self))
    }

    async fn execute_native(
        &self,
        tool: FsTool,
        ctx: ToolContext,
        arguments: &serde_json::Value,
    ) -> Result<aw_agent_core::ToolOutput> {
        if ctx.execution_scope.is_some()
            || ctx.execution_owner.as_ref() != Some(&self.agent)
            || ctx.scope.as_ref().is_none_or(|scope| {
                scope.session_id != self.agent.session_id()
                    || scope.branch_id != self.agent.branch_id()
            })
        {
            return Err(Error::Denied(
                "native source route requires its exact direct Agent invocation",
            ));
        }
        let access = if tool.action() == Action::Read {
            SourceAccess::ReadOnly
        } else {
            SourceAccess::ReadWrite
        };
        let expected_effect = if access == SourceAccess::ReadOnly {
            ToolEffect::Read
        } else {
            ToolEffect::Write
        };
        if ctx.effect != expected_effect
            || ctx.identity.actor != ctx.actor
            || ctx.source != aw_agent_core::ToolSource::trusted_host()
        {
            return Err(Error::Denied(
                "native source operation differs from its original tool effect",
            ));
        }
        let request = PreparedNativeRequest::new(tool, arguments, self.limits, &ctx)?;
        if ctx.cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let resolve = self.resolve.clone();
        let resolve_context = ctx.clone();
        let NativeSourceAdmission {
            authority,
            directory,
        } = tokio::task::spawn_blocking(move || resolve(&resolve_context, tool))
            .await
            .map_err(|_| Error::Protocol("source materialization resolver failed"))??;
        let policy = authority.policy();
        if policy.agent_ref != self.agent.canonical_ref() {
            return Err(Error::Denied(
                "native source policy belongs to another Agent",
            ));
        }
        let domain = self
            .agent
            .canonical_ref()
            .split('/')
            .nth(1)
            .ok_or(Error::Denied("native source Agent domain unavailable"))?;
        let operation = OperationId::Fs(tool);
        let tool_authority = self.tool_authority.upgrade()?;
        let evidence = AuthenticatedToolGrantEvidence::capture(
            &tool_authority,
            &ctx.actor,
            operation.model_name(),
            operation.semantic_name(),
            domain,
            &policy.sandbox_ref,
            &policy.sandbox_ref,
        )?;
        let effect = SandboxEffectRequest {
            actor: ctx.actor.clone(),
            agent_ref: self.agent.canonical_ref().into(),
            origin: SandboxEffectOrigin::Tool {
                turn_id: ctx.turn_id,
                call: aw_agent_core::ToolCall {
                    id: ctx.call_id.clone(),
                    name: operation.model_name().into(),
                    arguments: arguments.clone(),
                },
            },
            sources: vec![NamedSourceAccess {
                name: directory.binding.binding.name.clone(),
                access,
            }],
        };
        let cancellation = ctx.cancellation.clone();
        let authority =
            SourceToolAuthority::new(authority, tool_authority, evidence)?.for_invocation(ctx)?;
        let reader = self.registry.reader()?;
        let prepared = self.store.sandbox_prepare_source_effect(
            effect,
            &reader,
            &self.driver,
            |_| Ok(authority),
            |driver, plan, _| Ok((driver.prepare(plan, directory)?, request)),
        )?;
        let live = prepared.authority().tool.clone();
        let ((capability, request), authority, lease) = prepared.admit_with_authority()?;
        // The actual worker owns the Store lease as well as the pinned
        // directory and live policy. A dropped awaiter must not lose a
        // factual completion or release the original Store owner.
        tokio::task::spawn_blocking(move || {
            let (result, outcome) = if cancellation.is_cancelled() || !live.is_live() {
                (Err(Error::Cancelled), SandboxEffectOutcome::NotStarted)
            } else {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    request.execute(&capability, &|| cancellation.is_cancelled())
                })) {
                    Ok(result) => {
                        let outcome = if result.is_ok() {
                            SandboxEffectOutcome::Settled
                        } else {
                            SandboxEffectOutcome::Unknown
                        };
                        (result, outcome)
                    }
                    Err(_) => (
                        Err(Error::Protocol(
                            "native source worker panicked; inspect before retry",
                        )),
                        SandboxEffectOutcome::Unknown,
                    ),
                }
            };
            drop(capability);
            // Persist positive quiescence (or Unknown) while the worker
            // still holds its independent authority, before any result
            // can reach the original ToolFinished owner.
            lease.defer_until_tool_finished(outcome)?;
            drop(authority);
            result
        })
        .await
        .map_err(|_| {
            Error::Protocol("native source worker outcome uncertain; inspect before retry")
        })?
    }
}

impl<A, F> OperationRouter for SourceFilesystemRoute<A, F>
where
    A: SandboxEffectAuthority + Send + 'static,
    F: Fn(&ToolContext, FsTool) -> Result<NativeSourceAdmission<A>> + Send + Sync + 'static,
{
    fn execute<'a>(
        &'a self,
        operation: OperationId,
        ctx: ToolContext,
        arguments: &'a serde_json::Value,
        _original: Arc<dyn Tool>,
    ) -> ToolFuture<'a> {
        Box::pin(async move {
            let OperationId::Fs(tool) = operation else {
                return Err(Error::Denied(
                    "source-aware native route does not admit this operation",
                ));
            };
            self.execute_native(tool, ctx, arguments).await
        })
    }
}

impl<A, F> Extension for SourceFilesystemRoute<A, F>
where
    A: SandboxEffectAuthority + Send + 'static,
    F: Fn(&ToolContext, FsTool) -> Result<NativeSourceAdmission<A>> + Send + Sync + 'static,
{
    fn name(&self) -> &str {
        "host-source-filesystem"
    }

    fn on_hook<'a>(&'a self, ctx: HookContext<'a>, event: HookEvent) -> HookFuture<'a> {
        Box::pin(async move {
            if let HookEvent::ToolResult { call, .. } = &event {
                if FsTool::from_name(&call.name).is_some() {
                    self.store.sandbox_settle_recorded_tool_source_effect(
                        &ctx.identity.actor,
                        self.agent.canonical_ref(),
                        ctx.turn_id,
                        &call.id,
                    )?;
                }
            }
            Ok(aw_agent_core::HookFlow::Next(Some(event)))
        })
    }
}
