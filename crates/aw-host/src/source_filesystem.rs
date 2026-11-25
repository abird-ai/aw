//! Source-aware direct native tools. Use the existing pinned filesystem
//! backend and canonical executor; no ambient root fallback or Process.
use crate::{source_authority::SourceToolAuthority, AuthenticatedToolGrantEvidence};
use aw_agent_core::{
    tool::{ToolBindingAuthority, WeakToolBindingAuthority},
    AgentBuilder, AgentExecutionBinding, Error, Extension, HookContext, HookEvent, HookFuture,
    NamedSourceAccess, Result, SandboxDriverHandle, SandboxDriverRegistry, SourceAccess,
    SourceName, Tool, ToolContext, ToolEffect, ToolFuture,
};
use aw_store::{
    EffectiveSourceBinding, SandboxEffectAuthority, SandboxEffectOrigin, SandboxEffectOutcome,
    SandboxEffectPlan, SandboxEffectRequest, SessionStore,
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

/// An exact physical directory observation and the corresponding named
/// source mode. Neither this value nor its binding is a live authority.
#[derive(Clone)]
pub struct NativeSourceRoot {
    pub source: EffectiveSourceBinding,
    pub physical: GrantIdentity,
    pub private_artifacts: Arc<PrivateArtifactRegistry>,
}

// Initial native layouts use either one whole root or disjoint direct
// named children. The base is only a relative-path coordinate for the
// latter, never a grant. The host must exclude physical aliases/mounts.
pub(crate) fn require_named_root(
    base: &Path,
    name: &SourceName,
    path: &Path,
    count: usize,
) -> Result<()> {
    if !base.is_absolute()
        || base.parent().is_none()
        || !(count == 1 && path == base || path == base.join(name.as_str()))
    {
        return Err(Error::Denied(
            "native source must use its exact named directory",
        ));
    }
    Ok(())
}

/// One narrowed, descriptor-backed capability over the complete source
/// set exposed to this effect. Retain the actual authority separately.
pub struct ApprovedSourceLayout {
    sources: Vec<EffectiveSourceBinding>,
    root_granted: bool,
    capability: CapabilitySet,
    writable_artifacts: Vec<Arc<PrivateArtifactRegistry>>,
}

impl ApprovedSourceLayout {
    /// Pin only the independently approved roots at the selected modes.
    /// The existing backend rejects duplicate physical directory objects.
    pub fn pin(
        base: &Path,
        mut roots: Vec<NativeSourceRoot>,
        exclusions: impl IntoIterator<Item = ExclusionSpec>,
    ) -> Result<Self> {
        aw_agent_core::validate_source_set_hard_limit(roots.len())?;
        roots.sort_unstable_by(|a, b| {
            a.source
                .binding
                .binding
                .name
                .as_str()
                .cmp(b.source.binding.binding.name.as_str())
        });
        if roots.is_empty()
            || roots.windows(2).any(|pair| {
                pair[0].source.binding.binding.name == pair[1].source.binding.binding.name
            })
        {
            return Err(Error::Denied(
                "native source layout requires unique named roots",
            ));
        }
        for root in &roots {
            let binding = &root.source.binding;
            binding.binding.validate()?;
            require_named_root(
                base,
                &binding.binding.name,
                &root.physical.path,
                roots.len(),
            )?;
            if binding.binding_rev == 0
                || !root.physical.read
                || !binding.binding.access.permits(root.source.access)
                || (root.source.access == SourceAccess::ReadWrite && !root.physical.write)
            {
                return Err(Error::Denied(
                    "native source directory exceeds independent approval",
                ));
            }
        }
        let capability = CapabilitySet::open_prepared_single_link_with_exclusions(
            base,
            roots.iter().map(|root| {
                let grant = match root.source.access {
                    SourceAccess::ReadOnly => GrantSpec::read(&root.physical.path),
                    SourceAccess::ReadWrite => GrantSpec::read_write(&root.physical.path),
                };
                PreparedGrant::new(grant, root.private_artifacts.clone())
            }),
            exclusions,
        )
        .map_err(|_| Error::Denied("authorized native layout could not be pinned"))?;
        let actual = capability
            .identities()
            .map_err(|_| Error::Denied("native layout identity unavailable"))?;
        if capability.base() != base || actual.len() != roots.len() {
            return Err(Error::Conflict(
                "native source layout changed during preparation",
            ));
        }
        // Both lists have the same order: canonical name, hence canonical
        // sibling path (or one base root). No path-derived authority.
        for (actual, root) in actual.iter().zip(&roots) {
            if actual.path != root.physical.path
                || actual.volume != root.physical.volume
                || actual.object != root.physical.object
                || !actual.read
                || actual.write != (root.source.access == SourceAccess::ReadWrite)
            {
                return Err(Error::Conflict(
                    "authorized source directory changed during preparation",
                ));
            }
        }
        let root_granted = roots[0].physical.path == base;
        let writable_artifacts = roots
            .iter()
            .filter(|root| root.source.access == SourceAccess::ReadWrite)
            .map(|root| root.private_artifacts.clone())
            .collect();
        Ok(Self {
            sources: roots.into_iter().map(|root| root.source).collect(),
            root_granted,
            capability,
            writable_artifacts,
        })
    }

    /// A successful tool result is not proof that its mutation-private
    /// objects were retired. No record scan on read-only effects.
    fn artifacts_retired(&self) -> bool {
        self.writable_artifacts
            .iter()
            .all(|registry| registry.records().is_ok_and(|records| records.is_empty()))
    }

    #[cfg(feature = "git-source-effects")]
    pub(crate) fn reconcile_private_artifacts(&self) -> Result<()> {
        self.capability
            .reconcile_private_artifacts()
            .map_err(|_| Error::Conflict("native private artifact cleanup unresolved"))?;
        if !self.artifacts_retired() {
            return Err(Error::Conflict(
                "native private artifact custody remains unresolved",
            ));
        }
        Ok(())
    }
}

/// The existing native executor realizes exactly the admitted named roots,
/// not an ambient parent or another Process.
pub struct NativeFilesystemDriver;

impl NativeFilesystemDriver {
    fn prepare(
        &self,
        plan: &SandboxEffectPlan,
        layout: ApprovedSourceLayout,
    ) -> Result<ApprovedSourceLayout> {
        if layout.sources != plan.grant.sources
            || layout.capability.base() != plan.sandbox.tool_root
            || (layout.root_granted && plan.sandbox.sources.len() != 1)
        {
            return Err(Error::Denied(
                "native source layout differs from authorized Sandbox",
            ));
        }
        Ok(layout)
    }
}

/// A live policy decision and the exact narrowed capability it authorizes.
pub struct NativeSourceAdmission<A> {
    pub authority: A,
    pub layout: ApprovedSourceLayout,
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
        let NativeSourceAdmission { authority, layout } =
            tokio::task::spawn_blocking(move || resolve(&resolve_context, tool))
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
            sources: layout
                .sources
                .iter()
                .map(|source| NamedSourceAccess {
                    name: source.binding.binding.name.clone(),
                    access: source.access,
                })
                .collect(),
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
            |driver, plan, _| Ok((driver.prepare(plan, layout)?, request)),
        )?;
        let live = prepared.authority().tool.clone();
        let ((layout, request), authority, lease) = prepared.admit_with_authority()?;
        // The actual worker owns the Store lease as well as the pinned
        // directory and live policy. A dropped awaiter must not lose a
        // factual completion or release the original Store owner.
        tokio::task::spawn_blocking(move || {
            let (result, outcome) = if cancellation.is_cancelled() || !live.is_live() {
                (Err(Error::Cancelled), SandboxEffectOutcome::NotStarted)
            } else {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    request.execute(&layout.capability, &|| cancellation.is_cancelled())
                })) {
                    Ok(result) => {
                        let outcome = if result.is_ok() && layout.artifacts_retired() {
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
            drop(layout);
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
