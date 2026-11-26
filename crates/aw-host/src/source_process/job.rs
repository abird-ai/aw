//! Fixed foreground Process Job composition. The registered driver owns
//! actual physical source isolation and proof; the existing Job/Process and
//! original Tool journal own lifetime, result, and source settlement.
use super::{
    ordinary_projection, run_fenced_source_process, signing_projection, ProcessToolAuthority,
    SourceProcessCompletion,
};
use crate::AuthenticatedToolGrantEvidence;
use aw_agent_core::{
    tool::WeakToolBindingAuthority, Agent, AgentBuilder, AgentExecutionBinding, Error, Extension,
    HookContext, HookEvent, HookFlow, HookFuture, NamedSourceAccess, PrincipalId, Result,
    SandboxDriverHandle, SandboxDriverRegistry, ToolEffect, ToolSource, VaultEntryRef,
    VaultProjection, VaultProviderHandle, VaultProviderRegistry,
};
use aw_store::{
    SandboxEffectAuthority, SandboxEffectOrigin, SandboxEffectOutcome, SandboxEffectPlan,
    SandboxEffectRequest, SandboxExecution, SessionStore, SigningProcessProof,
    SigningVaultProvider,
};
use aw_tools::{filesystem::runner::ProcessTerminal, OperationId};
use aw_tools_adapter::{
    job_runtime::JobRuntime,
    process::{
        ContainmentRequirement, PipeLaunchFailure, PreparedPipeLaunch, SourceJobProcessCompletion,
        SourceJobProcessExecutor, SourceJobProcessFuture, SourceJobProcessRequest,
    },
    LocalToolOptions,
};
use sha2::{Digest as _, Sha256};
use std::{marker::PhantomData, sync::Arc};

/// Trusted physical realization of an exact foreground Process. Preparation
/// must be inert and pin/enforce the entire effective source set, network,
/// device, executable and other host policy. Returning an ambient shell
/// command merely rooted at the Sandbox is NOT an implementation of this
/// contract. The witness belongs to this prepared effect, not a PID scan.
pub trait SourceProcessDriver: Send + Sync + 'static {
    type Pins: Send + 'static;
    type Witness: Send + 'static;
    type Fence;
    fn operation(&self) -> OperationId;
    fn prepare(
        &self,
        plan: &SandboxEffectPlan,
        arguments: &serde_json::Value,
    ) -> Result<(PreparedPipeLaunch<Self::Pins>, Self::Witness)>;
    /// Explicit provider-specific opt-in. The driver must admit only an
    /// actual reviewed proof consumer under its independent physical policy,
    /// including the proof's result/disclosure contract. Never project a
    /// credential into an arbitrary shell merely because its tool is granted.
    /// No proof/key is supplied at preparation; it is derived at final spawn.
    fn prepare_with_signing_proof(
        &self,
        _plan: &SandboxEffectPlan,
        _arguments: &serde_json::Value,
        _selection: &SigningProcessProof,
    ) -> Result<(PreparedPipeLaunch<Self::Pins>, Self::Witness)> {
        Err(Error::Denied(
            "source Process driver has no reviewed signing projection",
        ))
    }

    fn prove_quiescent(
        &self,
        witness: Self::Witness,
        terminal: &ProcessTerminal,
    ) -> Result<Self::Fence>;
}

/// Fixed host-selected provider integration. Only the existing derived
/// signing proof is projected, never a raw Vault key or a caller-selected
/// environment name. The actual provider and grant are rechecked at spawn.
pub struct SigningProcessProjection {
    registry: VaultProviderRegistry,
    handle: VaultProviderHandle<SigningVaultProvider>,
    selection: SigningProcessProof,
    binding: [u8; 32],
}

impl SigningProcessProjection {
    pub fn new(
        registry: VaultProviderRegistry,
        handle: VaultProviderHandle<SigningVaultProvider>,
        selection: SigningProcessProof,
    ) -> Result<Self> {
        selection.validate()?;
        let reader = registry.reader()?;
        reader.get(&handle)?;
        let descriptor = handle.descriptor();
        reader.require_projection(
            &VaultEntryRef {
                provider: descriptor.provider.clone(),
                entry: selection.entry.clone(),
            },
            VaultProjection::Env,
        )?;
        // This is only an in-memory sealed plan identity. No proof, key,
        // challenge or entry details are added to Agent history or receipts.
        let mut hash = Sha256::new();
        hash.update(b"aw-source-process-signing-projection-v1\0");
        let provider = &descriptor.provider;
        match &provider.namespace {
            aw_agent_core::DriverNamespace::Builtin => hash.update([0]),
            aw_agent_core::DriverNamespace::Extension(extension) => {
                hash.update([1]);
                hash.update((extension.len() as u64).to_le_bytes());
                hash.update(extension.as_bytes());
            }
        }
        for value in [
            provider.local_id.as_bytes(),
            selection.name.as_str().as_bytes(),
            selection.entry.as_bytes(),
            selection.scope.as_bytes(),
            &selection.challenge,
        ] {
            hash.update((value.len() as u64).to_le_bytes());
            hash.update(value);
        }
        hash.update(descriptor.registration_revision.to_le_bytes());
        hash.update(selection.grant_rev.to_le_bytes());
        let binding = hash.finalize().into();
        Ok(Self {
            registry,
            handle,
            selection,
            binding,
        })
    }
}

/// One explicit registered Process and its fixed provider projection.
/// The native+Process host uses this typed selection, never model input.
pub struct SigningSourceProcess<D> {
    pub driver: SandboxDriverHandle<D>,
    pub projection: SigningProcessProjection,
}

/// Explicit embedding-host composition. A registered implementation and an
/// independently held live source decision are both required. Does not
/// publish a grant, open admission or select an OS backend implicitly.
pub struct SourceProcessHost<D, A, F> {
    registry: SandboxDriverRegistry,
    driver: SandboxDriverHandle<D>,
    acquire: Arc<F>,
    signing: Option<Arc<SigningProcessProjection>>,
    _authority: PhantomData<fn() -> A>,
}

impl<D, A, F> SourceProcessHost<D, A, F>
where
    D: SourceProcessDriver,
    A: SandboxEffectAuthority + Send + 'static,
    F: Fn() -> Result<A> + Send + Sync + 'static,
{
    pub fn new(
        registry: SandboxDriverRegistry,
        driver: SandboxDriverHandle<D>,
        acquire: F,
    ) -> Result<Self> {
        registry.reader()?.get(&driver)?;
        Ok(Self {
            registry,
            driver,
            acquire: Arc::new(acquire),
            signing: None,
            _authority: PhantomData,
        })
    }

    /// Opt in to the fixed signing projection for this host's registered
    /// Process. A refusal never falls back to ordinary execution.
    pub fn with_signing_projection(mut self, projection: SigningProcessProjection) -> Self {
        self.signing = Some(Arc::new(projection));
        self
    }

    /// Build the original Agent, its matching independent source scope and
    /// the original finite Job runtime. Only the exact foreground Process
    /// constructor is registered; no native/ambient root or dynamic router.
    pub fn build_execution(
        &self,
        store: &SessionStore,
        actor: PrincipalId,
        execution: AgentExecutionBinding,
        builder: AgentBuilder,
        options: &LocalToolOptions,
    ) -> Result<(Agent, SandboxExecution, Arc<JobRuntime>)> {
        self.build_with_native(store, actor, execution, builder, options, None)
    }

    pub(crate) fn build_with_native(
        &self,
        store: &SessionStore,
        actor: PrincipalId,
        execution: AgentExecutionBinding,
        builder: AgentBuilder,
        options: &LocalToolOptions,
        native: Option<Arc<dyn aw_tools_adapter::native_filesystem::SourceGraphExecutor>>,
    ) -> Result<(Agent, SandboxExecution, Arc<JobRuntime>)> {
        let driver = self.registry.reader()?.get(&self.driver)?;
        let route = Arc::new(SourceProcessRoute {
            store: store.clone(),
            agent: execution.clone(),
            actor: actor.clone(),
            registry: self.registry.clone(),
            driver: self.driver.clone(),
            acquire: self.acquire.clone(),
            tools: builder.tool_binding_authority().downgrade(),
            operation: driver.operation(),
            containment: options.local_process_containment,
            signing: self.signing.clone(),
            _authority: PhantomData::<fn() -> A>,
        });
        // The Agent extension/source handle own the route; the shell
        // registry only retains a weak reference. Avoid a Store/registry cycle.
        let configured = match native {
            Some(native) if options.selected_tools.iter().any(|name| name == "batch") => {
                aw_tools_adapter::configure_source_graph_with_process(
                    builder,
                    actor.clone(),
                    options,
                    native,
                    route.operation,
                    route.clone(),
                )?
            }
            Some(native) => aw_tools_adapter::configure_source_tools(
                builder,
                actor.clone(),
                options,
                native,
                route.operation,
                route.clone(),
            )?,
            None => aw_tools_adapter::configure_source_process(
                builder,
                actor.clone(),
                options,
                route.operation,
                route.clone(),
            )?,
        };
        let jobs = configured
            .job_runtime
            .ok_or(Error::Storage("source Process Job owner unavailable"))?;
        let mut agent = configured.builder.shared_extension(route.clone()).build()?;
        agent.bind_execution_owner(execution.clone())?;
        let source = SandboxExecution::from_trusted_host(
            store,
            actor,
            execution,
            options.root.clone(),
            &agent.tool_binding_authority(),
            move || {
                route.revalidate()?;
                (route.acquire)()
            },
        )?;
        Ok((agent, source, jobs))
    }
}

struct SourceProcessRoute<D, A, F> {
    store: SessionStore,
    agent: AgentExecutionBinding,
    actor: PrincipalId,
    registry: SandboxDriverRegistry,
    driver: SandboxDriverHandle<D>,
    acquire: Arc<F>,
    tools: WeakToolBindingAuthority,
    operation: OperationId,
    containment: ContainmentRequirement,
    signing: Option<Arc<SigningProcessProjection>>,
    _authority: PhantomData<fn() -> A>,
}

impl<D, A, F> SourceProcessRoute<D, A, F>
where
    D: SourceProcessDriver,
    A: SandboxEffectAuthority + Send + 'static,
    F: Fn() -> Result<A> + Send + Sync + 'static,
{
    async fn execute_job(
        &self,
        request: SourceJobProcessRequest<'_>,
    ) -> Result<SourceProcessCompletion> {
        let ctx = request.context;
        if request.operation != self.operation
            || ctx.actor != self.actor
            || ctx.identity.actor != ctx.actor
            || ctx.effect
                != if request.graph.is_some() {
                    ToolEffect::Control
                } else {
                    ToolEffect::Process
                }
            || ctx.source != ToolSource::trusted_host()
            || ctx.execution_site != "local"
            || ctx.execution_scope.is_some()
            || ctx.execution_owner.as_ref() != Some(&self.agent)
            || ctx.scope.as_ref().is_none_or(|scope| {
                scope.session_id != self.agent.session_id()
                    || scope.branch_id != self.agent.branch_id()
            })
        {
            return Err(Error::Denied(
                "source Process requires its exact direct original invocation",
            ));
        }
        if request.cancellation.is_cancelled() || ctx.cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        self.revalidate()?;
        let source = (self.acquire)()?;
        let policy = source.policy();
        if policy.agent_ref != self.agent.canonical_ref() || policy.actor != self.actor {
            return Err(Error::Denied(
                "source Process policy belongs to another Agent",
            ));
        }
        let observation = self.store.sandbox_grant_preflight(
            &self.actor,
            self.agent.canonical_ref(),
            policy,
            source.tool_ceiling(),
        )?;
        let domain = self
            .agent
            .canonical_ref()
            .split('/')
            .nth(1)
            .ok_or(Error::Denied("source Process Agent domain unavailable"))?;
        let tools = self.tools.upgrade()?;
        let evidence = AuthenticatedToolGrantEvidence::capture(
            &tools,
            &self.actor,
            self.operation.model_name(),
            self.operation.semantic_name(),
            domain,
            &policy.sandbox_ref,
            &policy.sandbox_ref,
        )?;
        let origin = if let Some(graph) = &request.graph {
            if graph.node.task().turn_id != ctx.turn_id
                || graph.node.task().call_id != ctx.call_id
                || graph.node.plan().tool().operation() != self.operation
                || graph.node.plan().arguments() != request.arguments
                || evidence.binding() != graph.binding
                || graph.constructor.claim_route
                    != Some(aw_agent_core::tool::ToolClaimRoute::ProcessGraphSourceV1)
            {
                return Err(Error::Denied(
                    "source Process differs from sealed original Graph node",
                ));
            }
            SandboxEffectOrigin::GraphNode(Box::new(graph.node.clone()))
        } else {
            SandboxEffectOrigin::Tool {
                turn_id: ctx.turn_id,
                call: aw_agent_core::ToolCall {
                    id: ctx.call_id.clone(),
                    name: self.operation.model_name().into(),
                    arguments: request.arguments.clone(),
                },
            }
        };
        let effect = SandboxEffectRequest {
            actor: self.actor.clone(),
            agent_ref: self.agent.canonical_ref().into(),
            origin,
            sources: observation
                .sources
                .iter()
                .map(|source| NamedSourceAccess {
                    name: source.binding.binding.name.clone(),
                    access: source.access,
                })
                .collect(),
        };
        let authority = ProcessToolAuthority::new(source, tools, evidence)?;
        let authority = match &request.graph {
            Some(graph) => authority.for_graph_node(ctx.clone(), graph.constructor.clone())?,
            None => authority.for_invocation(ctx.clone())?,
        }
        .requiring_process_containment(self.containment);
        let reader = self.registry.reader()?;
        let implementation = reader.get(&self.driver)?;
        let mut witness = None;
        let prepared = self.store.sandbox_prepare_source_effect(
            effect,
            &reader,
            &self.driver,
            |_| Ok(authority),
            |driver, plan, _| {
                if plan.grant.sources.len() != plan.sandbox.sources.len() {
                    return Err(Error::Denied(
                        "source Process requires the complete approved source set",
                    ));
                }
                let (launch, proof) = match &self.signing {
                    Some(signing) => driver.prepare_with_signing_proof(
                        plan,
                        request.arguments,
                        &signing.selection,
                    )?,
                    None => driver.prepare(plan, request.arguments)?,
                };
                witness = Some(proof);
                Ok(launch)
            },
        )?;
        let witness = witness.ok_or(Error::Storage("source Process witness unavailable"))?;
        let prove =
            move |terminal: &ProcessTerminal| implementation.prove_quiescent(witness, terminal);
        match &self.signing {
            Some(signing) => {
                // The reader is live, not an authorization snapshot. Its
                // typed provider and exact VaultGrant are checked inside the
                // original source/Actor/Store final spawn fence.
                let projection = signing_projection(
                    signing.registry.reader()?,
                    signing.handle.clone(),
                    signing.selection.clone(),
                );
                run_fenced_source_process(
                    prepared,
                    request.deadline,
                    request.cancellation,
                    Some(request.outputs),
                    projection,
                    prove,
                )
                .await
            }
            None => {
                run_fenced_source_process(
                    prepared,
                    request.deadline,
                    request.cancellation,
                    Some(request.outputs),
                    ordinary_projection,
                    prove,
                )
                .await
            }
        }
    }
}

impl<D, A, F> SourceJobProcessExecutor for SourceProcessRoute<D, A, F>
where
    D: SourceProcessDriver,
    A: SandboxEffectAuthority + Send + 'static,
    F: Fn() -> Result<A> + Send + Sync + 'static,
{
    fn runtime_binding(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"aw-source-process-route-v1\0");
        for value in [
            self.agent.canonical_ref(),
            self.actor.as_str(),
            self.driver.descriptor().driver.local_id.as_str(),
            self.operation.model_name(),
        ] {
            hash.update((value.len() as u64).to_le_bytes());
            hash.update(value.as_bytes());
        }
        match &self.driver.descriptor().driver.namespace {
            aw_agent_core::DriverNamespace::Builtin => hash.update([0]),
            aw_agent_core::DriverNamespace::Extension(extension) => {
                hash.update([1]);
                hash.update((extension.len() as u64).to_le_bytes());
                hash.update(extension.as_bytes());
            }
        }
        hash.update(self.driver.descriptor().registration_revision.to_le_bytes());
        hash.update([match self.containment {
            ContainmentRequirement::BestAvailable => 0,
            ContainmentRequirement::ClosedDescendants => 1,
        }]);
        if let Some(signing) = &self.signing {
            hash.update(b"signing-proof-v1");
            hash.update(signing.binding);
        }
        hash.finalize().into()
    }
    fn revalidate(&self) -> Result<()> {
        if self.registry.reader()?.get(&self.driver)?.operation() != self.operation {
            return Err(Error::Denied("registered source Process operation changed"));
        }
        Ok(())
    }
    fn settle_node(
        &self,
        ctx: &aw_agent_core::ToolContext,
        node: &aw_tools::filesystem::job_manager::RunningNode,
        transition: aw_tools::filesystem::job::Transition,
    ) -> Result<()> {
        if ctx.execution_owner.as_ref() != Some(&self.agent)
            || ctx.actor != self.actor
            || ctx.turn_id != node.task().turn_id
            || ctx.call_id != node.task().call_id
            || node.plan().tool().operation() != self.operation
        {
            return Err(Error::Denied(
                "source Process settlement differs from original Graph node",
            ));
        }
        self.store.sandbox_settle_recorded_graph_node(
            &ctx.actor,
            self.agent.canonical_ref(),
            node,
            transition,
        )
    }

    fn execute<'a>(&'a self, request: SourceJobProcessRequest<'a>) -> SourceJobProcessFuture<'a> {
        Box::pin(async move {
            match self.execute_job(request).await {
                Ok(completion) => SourceJobProcessCompletion {
                    receipt: completion.receipt,
                    source_settled: matches!(
                        completion.quiescence,
                        Ok(SandboxEffectOutcome::Settled | SandboxEffectOutcome::NotStarted)
                    ),
                },
                // Refusal/uncertain preparation never invokes another executor.
                Err(error) => SourceJobProcessCompletion {
                    receipt: Err(PipeLaunchFailure::Unknown(error)),
                    source_settled: false,
                },
            }
        })
    }
}

impl<D, A, F> Extension for SourceProcessRoute<D, A, F>
where
    D: SourceProcessDriver,
    A: SandboxEffectAuthority + Send + 'static,
    F: Fn() -> Result<A> + Send + Sync + 'static,
{
    fn name(&self) -> &str {
        "host-source-process"
    }
    fn on_hook<'a>(&'a self, ctx: HookContext<'a>, event: HookEvent) -> HookFuture<'a> {
        Box::pin(async move {
            if let HookEvent::ToolResult { call, .. } = &event {
                if call.name == self.operation.model_name() {
                    self.store.sandbox_settle_recorded_tool_source_effect(
                        &ctx.identity.actor,
                        self.agent.canonical_ref(),
                        ctx.turn_id,
                        &call.id,
                    )?;
                }
            }
            Ok(HookFlow::Next(Some(event)))
        })
    }
}
