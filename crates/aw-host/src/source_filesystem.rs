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
        CapabilitySet, GrantIdentity, GrantSpec, PreparedGrant, PreparedNativeRequest,
        PrivateArtifactRegistry,
    },
    operation_router::{OperationRouter, OperationRouterSlot},
    LocalToolLimits,
};
use std::{marker::PhantomData, path::Path, sync::Arc};

/// Exact physical directory authorized by the original host/source resolver.
/// The identity must be obtained from that authority (including live source
/// materialization verification), not from untrusted model path strings.
pub struct ApprovedSourceDirectory {
    pub binding: VersionedSourceBinding,
    pub identity: GrantIdentity,
    pub private_artifacts: Arc<PrivateArtifactRegistry>,
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
        let identity = directory.identity;
        if plan.sandbox.sources.len() != 1
            || source.binding != directory.binding
            || identity.path != plan.sandbox.tool_root
            || !identity.path.is_absolute()
            || identity.path.parent().is_none()
            || !identity.read
            || (source.access == SourceAccess::ReadWrite && !identity.write)
        {
            return Err(Error::Denied(
                "native source directory differs from the authorized Sandbox",
            ));
        }
        let grant = match source.access {
            SourceAccess::ReadOnly => GrantSpec::read(&identity.path),
            SourceAccess::ReadWrite => GrantSpec::read_write(&identity.path),
        };
        let capability = CapabilitySet::open_prepared(
            &identity.path,
            [PreparedGrant::new(grant, directory.private_artifacts)],
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
            || actual.write != (source.access == SourceAccess::ReadWrite)
            || capability.base() != Path::new(&identity.path)
        {
            return Err(Error::Conflict(
                "authorized source directory changed during preparation",
            ));
        }
        Ok(capability)
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
    resolve: F,
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
            resolve,
            _authority: PhantomData,
        })
    }

    /// Install both halves on the existing host slot and Agent builder.
    /// Does not grant tools, open held Agent admission, or enable shell/Jobs.
    pub fn install(
        self: Arc<Self>,
        builder: AgentBuilder,
        slot: &OperationRouterSlot,
    ) -> Result<AgentBuilder> {
        if !self
            .tool_authority
            .belongs_to(&builder.tool_binding_authority())
        {
            return Err(Error::Denied(
                "source route belongs to another tool authority",
            ));
        }
        slot.install(self.clone())?;
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
        let NativeSourceAdmission {
            authority,
            directory,
        } = (self.resolve)(&ctx, tool)?;
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
        let (result, lease) = prepared
            .start_with_authority(|(capability, request), authority| async move {
                // Both the policy guard and pinned directory follow the actual
                // blocking worker even if the Agent drops the awaiter.
                tokio::task::spawn_blocking(move || {
                    let result = if cancellation.is_cancelled() || !live.is_live() {
                        (Err(Error::Cancelled), SandboxEffectOutcome::NotStarted)
                    } else {
                        let result = request.execute(&capability, &|| cancellation.is_cancelled());
                        let outcome = if result.is_ok() {
                            SandboxEffectOutcome::Settled
                        } else {
                            SandboxEffectOutcome::Unknown
                        };
                        (result, outcome)
                    };
                    drop(capability);
                    drop(authority);
                    result
                })
                .await
            })
            .await?;
        match result {
            Ok((result, outcome)) => {
                lease.defer_until_tool_finished(outcome)?;
                result
            }
            Err(_) => {
                lease.settle(SandboxEffectOutcome::Unknown)?;
                Err(Error::Protocol(
                    "native source worker outcome uncertain; inspect before retry",
                ))
            }
        }
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
