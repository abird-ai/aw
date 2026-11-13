//! One host composition boundary. Does not own another Agent, Actor, Job or
//! Session engine. The initial path remains direct, local and userless.

#![forbid(unsafe_code)]

use aw_agent_core::{
    tool::{ToolBindingAuthority, ToolBindingSnapshot},
    AgentBuilder, Error, PrincipalId, Result, ToolEffect, ToolPolicy,
};
use aw_policy::{EffectivePolicy, Strategy};
use aw_tools::{
    coordination::ControlAncestry, execution_ref::ResolvedExecutionRef, operation::OperationId,
};
use aw_tools_adapter::LocalToolOptions;
use sha2::{Digest as _, Sha256};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

pub use aw_policy::{
    AuthenticatedGrantPath, AuthorizedGrant, AuthorizedPolicy, Decision as PolicyDecision, Effect,
    GrantRestrictions, GrantSubject, OperationContext, Policy, PolicyContext,
    PolicyDecision as ResolvedPolicyDecision, PolicyError, PolicyLayer, PolicyScope, Routes,
    SourceKind,
};

#[cfg(feature = "sessions")]
pub use aw_session::{CoordinatorBinding, Sessions};

#[derive(Clone, Debug)]
pub struct AuthenticatedToolGrantEvidence {
    actor: PrincipalId,
    operation: String,
    target_domain: String,
    resource_scope: String,
    grant_scope: String,
    proof_ref: String,
    binding: ToolBindingSnapshot,
}

impl AuthenticatedToolGrantEvidence {
    /// Capture one exact currently permitted tool binding from the live
    /// ToolBindingAuthority. Resource/domain scope is trusted host semantic
    /// evidence and remains independently enforced by the original executor.
    pub fn capture(
        authority: &ToolBindingAuthority,
        actor: &PrincipalId,
        tool_name: &str,
        operation: impl Into<String>,
        target_domain: impl Into<String>,
        resource_scope: impl Into<String>,
        grant_scope: impl Into<String>,
    ) -> Result<Self> {
        let binding = authority.permitted_binding(actor, tool_name)?;
        let value = Self {
            actor: actor.clone(),
            operation: operation.into(),
            target_domain: target_domain.into(),
            resource_scope: resource_scope.into(),
            grant_scope: grant_scope.into(),
            proof_ref: format!(
                "tb:r{}:a{}",
                binding.registry_revision, binding.authority_revision
            ),
            binding,
        };
        value.as_policy_path().validate().map_err(|_| {
            Error::Denied("live tool binding cannot form bounded policy grant evidence")
        })?;
        Ok(value)
    }

    pub fn revalidate(&self, authority: &ToolBindingAuthority) -> Result<()> {
        authority.revalidate(&self.actor, &self.binding)
    }

    pub fn binding(&self) -> &ToolBindingSnapshot {
        &self.binding
    }

    pub fn operation_context(&self) -> OperationContext<'_> {
        OperationContext {
            operation: &self.operation,
            source_kind: match self.binding.source.kind_label() {
                "host" => SourceKind::Host,
                "mcp" => SourceKind::Mcp,
                "extension" => SourceKind::Extension,
                _ => unreachable!("ToolSource exposes a closed SourceKind"),
            },
            source_id: &self.binding.source.id,
            source_epoch: self.binding.source.epoch,
            effect: match self.binding.effect {
                ToolEffect::Read => Effect::Read,
                ToolEffect::Write => Effect::Write,
                ToolEffect::Process => Effect::Process,
                ToolEffect::Control => Effect::Control,
                ToolEffect::Unknown => Effect::Unknown,
            },
            site: &self.binding.execution_site,
            target_domain: &self.target_domain,
            resource_scope: &self.resource_scope,
        }
    }

    pub fn as_policy_path(&self) -> AuthenticatedGrantPath<'_> {
        let operation = self.operation_context();
        AuthenticatedGrantPath {
            issuer: "host:tool-binding",
            issuer_authority_epoch: self.binding.authority_revision,
            grant_scope: &self.grant_scope,
            proof_ref: &self.proof_ref,
            subject: GrantSubject::DirectIdentity {
                identity: self.actor.as_str(),
            },
            restrictions: GrantRestrictions {
                operation: operation.operation,
                source_kind: operation.source_kind,
                source_id: operation.source_id,
                source_epoch: operation.source_epoch,
                effect: operation.effect,
                site: operation.site,
                target_domain: operation.target_domain,
                resource_scope: operation.resource_scope,
            },
        }
    }

    /// Stable digest of the exact captured binding + semantic/resource scope.
    /// Revoke/regrant changes authority_revision and therefore this digest.
    pub fn authority_sha256(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(b"aw-authenticated-tool-grant-v1\0");
        for part in [
            self.actor.as_str().as_bytes(),
            self.operation.as_bytes(),
            self.target_domain.as_bytes(),
            self.resource_scope.as_bytes(),
            self.grant_scope.as_bytes(),
            self.binding.name.as_bytes(),
            self.binding.source.id.as_bytes(),
            self.binding.execution_site.as_bytes(),
        ] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part);
        }
        hash.update(self.binding.registry_revision.to_be_bytes());
        hash.update(self.binding.authority_revision.to_be_bytes());
        hash.update(self.binding.source.epoch.to_be_bytes());
        hash.update([match self.binding.effect {
            ToolEffect::Read => 1,
            ToolEffect::Write => 2,
            ToolEffect::Process => 3,
            ToolEffect::Control => 4,
            ToolEffect::Unknown => 5,
        }]);
        format!("{:x}", hash.finalize())
    }
}

pub type HostExecutionFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

/// Already-prepared original semantic executor. It owns the exact arguments,
/// sandbox/resource checks, replay identity and factual receipt contract.
pub trait OriginalOperationExecutor: Send + Sync {
    fn execute<'a>(&'a self) -> HostExecutionFuture<'a, aw_agent_core::ToolOutput>;
}

/// Current delegate-side Agent/provider/payer/disclosure authority. The host
/// implementation must revalidate from authoritative owners; this trait never
/// receives or creates a ToolPolicy grant.
pub trait DelegateAdmissionAuthority: Send + Sync {
    fn revalidate(&self, request: &DelegationRequest) -> Result<()>;
}

/// Delegate coordination is intentionally result-free. The Agent may reason or
/// converse, but native success exists only if it invokes the supplied original
/// execution gate.
pub trait DelegateAgentPort: Send + Sync {
    fn drive<'a>(
        &'a self,
        request: &'a DelegationRequest,
        gate: Arc<DelegatedExecutionGate>,
    ) -> HostExecutionFuture<'a, ()>;
}

/// Host-authenticated routing context. control_target is the original resolved
/// jobs.* target, not the delegate Agent. When present, canonical ownership/wait
/// ancestry is checked mechanically before any paid child admission.
#[derive(Clone, Debug)]
pub struct DelegationRequest {
    operation: OperationId,
    delegate_agent: Option<ResolvedExecutionRef>,
    control_target: Option<ResolvedExecutionRef>,
    self_ref: Option<ResolvedExecutionRef>,
    ownership_ancestors: Vec<ResolvedExecutionRef>,
    wait_dependencies: Vec<ResolvedExecutionRef>,
}

impl DelegationRequest {
    pub fn new(operation: OperationId, delegate_agent: ResolvedExecutionRef) -> Self {
        Self {
            operation,
            delegate_agent: Some(delegate_agent),
            control_target: None,
            self_ref: None,
            ownership_ancestors: Vec::new(),
            wait_dependencies: Vec::new(),
        }
    }

    pub fn transient(operation: OperationId) -> Self {
        Self {
            operation,
            delegate_agent: None,
            control_target: None,
            self_ref: None,
            ownership_ancestors: Vec::new(),
            wait_dependencies: Vec::new(),
        }
    }

    pub fn with_delegate_agent(mut self, delegate_agent: ResolvedExecutionRef) -> Self {
        self.delegate_agent = Some(delegate_agent);
        self
    }

    pub fn with_control_target(
        mut self,
        target: ResolvedExecutionRef,
        self_ref: ResolvedExecutionRef,
        ownership_ancestors: Vec<ResolvedExecutionRef>,
        wait_dependencies: Vec<ResolvedExecutionRef>,
    ) -> Result<Self> {
        self.control_target = Some(target);
        self.self_ref = Some(self_ref);
        self.ownership_ancestors = ownership_ancestors;
        self.wait_dependencies = wait_dependencies;
        self.validate()?;
        Ok(self)
    }

    pub const fn operation(&self) -> OperationId {
        self.operation
    }

    pub fn delegate_agent(&self) -> Option<&ResolvedExecutionRef> {
        self.delegate_agent.as_ref()
    }

    pub fn control_target(&self) -> Option<&ResolvedExecutionRef> {
        self.control_target.as_ref()
    }

    fn validate(&self) -> Result<()> {
        match (&self.control_target, &self.self_ref) {
            (Some(target), Some(self_ref)) => {
                let ancestry = ControlAncestry {
                    self_ref,
                    ownership_ancestors: &self.ownership_ancestors,
                    wait_dependencies: &self.wait_dependencies,
                };
                if ancestry.rejects_target(target) {
                    return Err(Error::Denied(
                        "transparent delegation would create a self/ancestor/wait control cycle",
                    ));
                }
            }
            (None, None)
                if self.ownership_ancestors.is_empty() && self.wait_dependencies.is_empty() => {}
            _ => {
                return Err(Error::Denied(
                    "delegated control requires complete canonical ancestry evidence",
                ))
            }
        }
        Ok(())
    }
}

enum DelegatedExecutionState {
    Fresh,
    Running,
    Settled(aw_agent_core::ToolOutput),
    Failed,
}

/// One-shot bridge from an ordinary delegate Agent back to the exact original
/// trusted executor. It deliberately has no method for accepting Agent prose as
/// a receipt.
pub struct DelegatedExecutionGate {
    request: DelegationRequest,
    original_executor: Arc<dyn OriginalOperationExecutor>,
    original_evidence: AuthenticatedToolGrantEvidence,
    original_authority: ToolBindingAuthority,
    delegate_binding: Mutex<Option<(AuthenticatedToolGrantEvidence, ToolBindingAuthority)>>,
    delegate_admission: Arc<dyn DelegateAdmissionAuthority>,
    state: Mutex<DelegatedExecutionState>,
}

impl DelegatedExecutionGate {
    fn new(
        request: DelegationRequest,
        original_executor: Arc<dyn OriginalOperationExecutor>,
        original_evidence: AuthenticatedToolGrantEvidence,
        original_authority: ToolBindingAuthority,
        delegate_admission: Arc<dyn DelegateAdmissionAuthority>,
    ) -> Result<Self> {
        request.validate()?;
        validate_operation_evidence(request.operation, &original_evidence)?;
        Ok(Self {
            request,
            original_executor,
            original_evidence,
            original_authority,
            delegate_binding: Mutex::new(None),
            delegate_admission,
            state: Mutex::new(DelegatedExecutionState::Fresh),
        })
    }

    fn revalidate_all(&self) -> Result<()> {
        self.request.validate()?;
        validate_operation_evidence(self.request.operation, &self.original_evidence)?;
        self.original_evidence
            .revalidate(&self.original_authority)?;
        let (evidence, authority) = self
            .delegate_binding
            .lock()
            .map_err(|_| Error::Denied("delegate authority binding poisoned"))?
            .clone()
            .ok_or(Error::Denied(
                "delegate has not bound a live one-shot operation capability",
            ))?;
        validate_delegate_operation_evidence(self.request.operation, &evidence)?;
        evidence.revalidate(&authority)?;
        self.delegate_admission.revalidate(&self.request)
    }

    pub fn request(&self) -> &DelegationRequest {
        &self.request
    }

    /// Bind exactly one current delegate-side ToolBinding generation after the
    /// host has created/selected the delegate Agent and registered its one-shot
    /// operation gate. Rebinding or a stale grant is never accepted.
    pub fn bind_delegate_tool(
        &self,
        authority: &ToolBindingAuthority,
        actor: &PrincipalId,
        tool_name: &str,
    ) -> Result<()> {
        let original = self.original_evidence.operation_context();
        let evidence = AuthenticatedToolGrantEvidence::capture(
            authority,
            actor,
            tool_name,
            self.request.operation.semantic_name(),
            original.target_domain,
            original.resource_scope,
            &self.original_evidence.grant_scope,
        )?;
        self.bind_delegate_authority(evidence, authority.clone())
    }

    pub fn bind_delegate_authority(
        &self,
        evidence: AuthenticatedToolGrantEvidence,
        authority: ToolBindingAuthority,
    ) -> Result<()> {
        validate_delegate_operation_evidence(self.request.operation, &evidence)?;
        evidence.revalidate(&authority)?;
        let mut slot = self
            .delegate_binding
            .lock()
            .map_err(|_| Error::Denied("delegate authority binding poisoned"))?;
        if slot.is_some() {
            return Err(Error::Conflict(
                "delegate operation authority is already bound for this gate",
            ));
        }
        *slot = Some((evidence, authority));
        Ok(())
    }

    /// Execute the original prepared operation at most once. Every authority is
    /// revalidated immediately before crossing into the factual executor.
    pub async fn execute_original(&self) -> Result<aw_agent_core::ToolOutput> {
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| Error::Denied("delegated execution gate poisoned"))?;
            match &*state {
                DelegatedExecutionState::Fresh => {
                    *state = DelegatedExecutionState::Running;
                }
                DelegatedExecutionState::Running => {
                    return Err(Error::Conflict(
                        "original delegated operation is already executing",
                    ));
                }
                DelegatedExecutionState::Settled(_) => {
                    return Err(Error::Conflict(
                        "original delegated operation already has a factual receipt",
                    ));
                }
                DelegatedExecutionState::Failed => {
                    return Err(Error::Conflict(
                        "failed delegated operation cannot be replayed through the same gate",
                    ));
                }
            }
        }

        if let Err(error) = self.revalidate_all() {
            if let Ok(mut state) = self.state.lock() {
                *state = DelegatedExecutionState::Failed;
            }
            return Err(error);
        }

        match self.original_executor.execute().await {
            Ok(output) => {
                output.validate()?;
                let mut state = self
                    .state
                    .lock()
                    .map_err(|_| Error::Denied("delegated execution gate poisoned"))?;
                *state = DelegatedExecutionState::Settled(output.clone());
                Ok(output)
            }
            Err(error) => {
                if let Ok(mut state) = self.state.lock() {
                    *state = DelegatedExecutionState::Failed;
                }
                Err(error)
            }
        }
    }

    /// Factual native receipt only. No child answer/summary is consulted.
    pub fn factual_receipt(&self) -> Result<aw_agent_core::ToolOutput> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::Denied("delegated execution gate poisoned"))?;
        match &*state {
            DelegatedExecutionState::Settled(output) => Ok(output.clone()),
            DelegatedExecutionState::Fresh => Err(Error::Denied(
                "delegate never invoked the original trusted executor",
            )),
            DelegatedExecutionState::Running => Err(Error::Conflict(
                "original delegated operation is still running",
            )),
            DelegatedExecutionState::Failed => Err(Error::Conflict(
                "original delegated operation failed without a factual success receipt",
            )),
        }
    }
}

pub struct DelegateExecution {
    pub request: DelegationRequest,
    pub port: Arc<dyn DelegateAgentPort>,
    /// Pre-existing delegates may supply an already-captured one-shot binding.
    /// Transient delegate ports leave this None and bind after child creation.
    pub delegate_binding: Option<(AuthenticatedToolGrantEvidence, ToolBindingAuthority)>,
    pub delegate_admission: Arc<dyn DelegateAdmissionAuthority>,
}

fn validate_operation_evidence(
    operation: OperationId,
    evidence: &AuthenticatedToolGrantEvidence,
) -> Result<()> {
    let context = evidence.operation_context();
    if context.operation != operation.semantic_name()
        || evidence.binding().name != operation.model_name()
    {
        return Err(Error::Denied(
            "captured grant evidence differs from the original semantic operation",
        ));
    }
    Ok(())
}

fn validate_delegate_operation_evidence(
    operation: OperationId,
    evidence: &AuthenticatedToolGrantEvidence,
) -> Result<()> {
    let context = evidence.operation_context();
    if context.operation != operation.semantic_name()
        || !matches!(evidence.binding().name.as_str(), "delegate_execute")
            && evidence.binding().name != operation.model_name()
    {
        return Err(Error::Denied(
            "delegate capability differs from the original semantic operation",
        ));
    }
    Ok(())
}

/// Execute one already-prepared semantic operation under the selected strategy.
/// DelegateAgent can coordinate only; it cannot substitute text for the
/// original executor's factual receipt.
pub async fn execute_with_strategy(
    strategy: Strategy,
    operation: OperationId,
    original_executor: Arc<dyn OriginalOperationExecutor>,
    original_evidence: AuthenticatedToolGrantEvidence,
    original_authority: ToolBindingAuthority,
    delegated: Option<DelegateExecution>,
) -> Result<aw_agent_core::ToolOutput> {
    validate_operation_evidence(operation, &original_evidence)?;
    original_evidence.revalidate(&original_authority)?;

    match strategy {
        Strategy::Local => {
            if delegated.is_some() {
                return Err(Error::Denied(
                    "Local execution cannot carry a hidden delegate admission",
                ));
            }
            let output = original_executor.execute().await?;
            output.validate()?;
            Ok(output)
        }
        Strategy::DelegateAgent => {
            let DelegateExecution {
                request,
                port,
                delegate_binding,
                delegate_admission,
            } = delegated.ok_or(Error::Denied(
                "DelegateAgent strategy requires an authenticated delegate owner",
            ))?;
            if request.operation != operation {
                return Err(Error::Denied(
                    "delegate request changed the original semantic operation",
                ));
            }
            request.validate()?;
            delegate_admission.revalidate(&request)?;

            let gate = Arc::new(DelegatedExecutionGate::new(
                request.clone(),
                original_executor,
                original_evidence,
                original_authority,
                delegate_admission,
            )?);
            if let Some((evidence, authority)) = delegate_binding {
                gate.bind_delegate_authority(evidence, authority)?;
            }
            let coordination = port.drive(&request, gate.clone()).await;
            // If the native operation already settled, a late delegate error
            // cannot erase or replace that factual effect/result.
            if let Ok(receipt) = gate.factual_receipt() {
                return Ok(receipt);
            }
            coordination?;
            gate.factual_receipt()
        }
    }
}

/// A zero-guardian local host. Current grants still come from ToolPolicy and
/// the adapter's exact native resource controls, not this preference.
pub struct LocalHost<'a> {
    actor: PrincipalId,
    tools: &'a LocalToolOptions,
    effective: EffectivePolicy,
}

impl<'a> LocalHost<'a> {
    pub fn new(actor: PrincipalId, tools: &'a LocalToolOptions) -> Self {
        Self {
            actor,
            tools,
            effective: EffectivePolicy::local(),
        }
    }

    /// Trusted host may supply narrower effective preferences. DelegateAgent
    /// execution will be wired here later, not emulated by the native adapter.
    pub fn with_effective_policy(mut self, effective: EffectivePolicy) -> Result<Self> {
        if effective.strategy() != Strategy::Local || !effective.routes().contains(Strategy::Local)
        {
            return Err(Error::Denied("DelegateAgent host route not yet available"));
        }
        self.effective = self
            .effective
            .narrow(effective)
            .map_err(|_| Error::Denied("host policy cannot widen or retarget authority"))?;
        Ok(self)
    }

    pub fn effective_policy(&self) -> EffectivePolicy {
        self.effective
    }

    /// Apply pure policy constraints only after the embedding host/store has
    /// authenticated at least one exact positive grant path for this operation.
    /// This method does not turn that evidence into a ToolPolicy grant; the
    /// original adapter/Envelope/resource owners still revalidate effects.
    pub fn with_authenticated_policy(
        self,
        context: &PolicyContext<'_>,
        grants: &[AuthenticatedGrantPath<'_>],
        layers: &[PolicyLayer<'_>],
    ) -> Result<Self> {
        if context.actor != self.actor.as_str() {
            return Err(Error::Denied(
                "policy context actor differs from local host principal",
            ));
        }
        if context.operation.site != self.tools.execution_site {
            return Err(Error::Denied(
                "policy context execution site differs from local host site",
            ));
        }
        let authorized = aw_policy::resolve_authorized(context, grants, layers)
            .map_err(|_| Error::Denied("authenticated host policy denied"))?;
        self.with_effective_policy(authorized.effective())
    }

    /// Apply authenticated scoped constraints. This cannot invent tools or
    /// silently enable an Agent route unsupported by the current local host.
    pub fn with_scoped_policy(self, layers: &[aw_policy::PolicyLayer<'_>]) -> Result<Self> {
        let effective = aw_policy::resolve_scoped(layers)
            .map_err(|_| Error::Denied("effective host policy denied"))?;
        self.with_effective_policy(effective)
    }

    /// Consumes the one-shot local host, moving its actor into the adapter.
    /// No extra PrincipalId allocation or clone in the default bootstrap.
    pub fn configure(self, builder: AgentBuilder) -> Result<AgentBuilder> {
        if self.effective.strategy() != Strategy::Local
            || !self.effective.routes().contains(Strategy::Local)
        {
            return Err(Error::Denied("host route is not Local"));
        }
        let concurrency = self.tools.concurrency.validate()?;
        let mut policy = builder.tool_policy_snapshot();
        let has_grants = policy.has_grants();
        if policy.execution_site != self.tools.execution_site {
            if policy.execution_site == ToolPolicy::default().execution_site && !has_grants {
                policy.execution_site = self.tools.execution_site.clone();
            } else {
                return Err(Error::Config(
                    "host cannot retarget an existing tool execution site".into(),
                ));
            }
        }
        if has_grants {
            // A pre-granted embedding policy may only be narrowed.
            policy.max_parallel_reads = policy
                .max_parallel_reads
                .min(concurrency.parallelism.max_parallel_reads);
            policy.allow_parallel_bash &= concurrency.parallelism.allow_parallel_shell;
            policy.max_parallel_bash = policy
                .max_parallel_bash
                .min(concurrency.parallelism.max_parallel_shell);
        } else {
            // Stock AW construction intentionally selects its own host ceilings.
            policy.max_parallel_reads = concurrency.parallelism.max_parallel_reads;
            policy.allow_parallel_bash = concurrency.parallelism.allow_parallel_shell;
            policy.max_parallel_bash = concurrency.parallelism.max_parallel_shell;
        }
        if let Some(ceiling) = self.effective.max_model_rounds() {
            policy.max_model_rounds = policy.max_model_rounds.min(ceiling);
        }
        aw_tools_adapter::configure(builder.tool_policy(policy), self.actor, self.tools)
    }
}

/// Existing local callers can adopt the host boundary without new state,
/// membership services, provider inference or Session persistence.
pub fn configure_local(
    builder: AgentBuilder,
    actor: PrincipalId,
    tools: &LocalToolOptions,
) -> Result<AgentBuilder> {
    LocalHost::new(actor, tools).configure(builder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::{
        provider::{CompletionFuture, DeltaSink, Provider},
        Agent, AgentOptions, Message, ToolEffect,
    };
    use aw_tools_adapter::{LocalToolConcurrency, ToolMode};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct NoInference;
    impl Provider for NoInference {
        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Denied("test provider must not run")) })
        }
    }

    #[test]
    fn userless_local_is_inference_free_and_does_not_grant_tools() {
        let opts = LocalToolOptions::new(".", ToolMode::Off);
        let host = LocalHost::new(PrincipalId::new("host:local").unwrap(), &opts);
        assert_eq!(host.effective_policy().strategy(), Strategy::Local);
        assert_eq!(host.effective_policy().routes(), Routes::LOCAL);
    }

    #[test]
    fn trusted_scope_policy_narrows_local_without_issuing_grants() {
        let opts = LocalToolOptions::new(".", ToolMode::Off);
        let host = LocalHost::new(PrincipalId::new("host:local").unwrap(), &opts);
        let policy = Policy {
            max_model_rounds: Some(3),
            ..Policy::default()
        };
        let layers = [aw_policy::PolicyLayer {
            scope: aw_policy::PolicyScope::Host,
            revision: 1,
            policy: &policy,
        }];
        let host = host.with_scoped_policy(&layers).unwrap();
        assert_eq!(host.effective_policy().strategy(), Strategy::Local);
        assert_eq!(host.effective_policy().max_model_rounds(), Some(3));
    }

    #[test]
    fn repeated_local_policy_application_only_narrows() {
        let opts = LocalToolOptions::new(".", ToolMode::Off);
        let host = LocalHost::new(PrincipalId::new("host:local").unwrap(), &opts);
        let narrow = Policy {
            max_model_rounds: Some(2),
            ..Policy::default()
        };
        let host = host
            .with_scoped_policy(&[aw_policy::PolicyLayer {
                scope: aw_policy::PolicyScope::Session,
                revision: 1,
                policy: &narrow,
            }])
            .unwrap();
        assert_eq!(host.effective_policy().max_model_rounds(), Some(2));
        let wider = Policy::default();
        let host = host
            .with_scoped_policy(&[aw_policy::PolicyLayer {
                scope: aw_policy::PolicyScope::Session,
                revision: 2,
                policy: &wider,
            }])
            .unwrap();
        assert_eq!(host.effective_policy().max_model_rounds(), Some(2));
        assert_eq!(host.effective_policy().routes(), Routes::LOCAL);
    }

    #[test]
    fn granted_builder_cannot_widen_scheduler_or_retarget_site() {
        let actor = PrincipalId::new("host:local").unwrap();
        let mut policy = ToolPolicy {
            max_parallel_reads: 2,
            max_parallel_bash: 1,
            allow_parallel_bash: false,
            ..ToolPolicy::default()
        };
        policy
            .permit_host(actor.clone(), "read", &ToolEffect::Read)
            .unwrap();
        let builder = Agent::builder(Arc::new(NoInference), AgentOptions::new("test", "model"))
            .tool_policy(policy);
        let mut opts = LocalToolOptions::new(".", ToolMode::Off);
        opts.concurrency = LocalToolConcurrency {
            graph_width: 2,
            parallelism: aw_agent_core::ToolParallelism {
                max_parallel_reads: 16,
                max_parallel_shell: 8,
                allow_parallel_shell: true,
            },
        };
        let configured = LocalHost::new(actor.clone(), &opts)
            .configure(builder)
            .unwrap();
        let actual = configured.tool_policy_snapshot();
        assert_eq!(actual.max_parallel_reads, 2);
        assert_eq!(actual.max_parallel_bash, 1);
        assert!(!actual.allow_parallel_bash);
        assert!(actual.has_grants());

        let mut retargeted = opts.clone();
        retargeted.execution_site = "remote-site".into();
        let builder = Agent::builder(Arc::new(NoInference), AgentOptions::new("test", "model"))
            .tool_policy(actual);
        assert!(matches!(
            LocalHost::new(actor, &retargeted).configure(builder),
            Err(Error::Config(_))
        ));
    }

    fn authenticated_context<'a>(
        actor: &'a str,
        site: &'a str,
    ) -> (PolicyContext<'a>, AuthenticatedGrantPath<'a>) {
        let operation = OperationContext {
            operation: "tools.fs.read",
            source_kind: SourceKind::Host,
            source_id: "trusted-host",
            source_epoch: 1,
            effect: Effect::Read,
            site,
            target_domain: "d:local",
            resource_scope: "/d:local/i:owner/p:repo/path:README.md",
        };
        (
            PolicyContext {
                actor,
                domain: "d:local",
                active_team: None,
                workspace: "/d:local/i:owner/p:repo",
                session: None,
                graph: None,
                node: None,
                invocation: "host-call-1",
                operation,
            },
            AuthenticatedGrantPath {
                issuer: "i:host-admin",
                issuer_authority_epoch: 2,
                grant_scope: "/d:local/i:owner/p:repo",
                proof_ref: "grant-host-read-v2",
                subject: GrantSubject::DirectIdentity { identity: actor },
                restrictions: GrantRestrictions {
                    operation: operation.operation,
                    source_kind: operation.source_kind,
                    source_id: operation.source_id,
                    source_epoch: operation.source_epoch,
                    effect: operation.effect,
                    site: operation.site,
                    target_domain: operation.target_domain,
                    resource_scope: operation.resource_scope,
                },
            },
        )
    }

    #[test]
    fn authenticated_policy_requires_positive_path_but_never_registers_a_tool() {
        let opts = LocalToolOptions::new(".", ToolMode::Off);
        let actor = PrincipalId::new("host:local").unwrap();
        let (context, grant) = authenticated_context(actor.as_str(), "local");

        assert!(matches!(
            LocalHost::new(actor.clone(), &opts).with_authenticated_policy(&context, &[], &[]),
            Err(Error::Denied(_))
        ));

        let builder = Agent::builder(Arc::new(NoInference), AgentOptions::new("test", "model"));
        let agent = LocalHost::new(actor.clone(), &opts)
            .with_authenticated_policy(&context, &[grant], &[])
            .unwrap()
            .configure(builder)
            .unwrap()
            .build()
            .unwrap();
        assert!(
            agent.tool_specs().is_empty(),
            "policy evidence is not a ToolPolicy/tool-registry grant"
        );
    }

    #[test]
    fn authenticated_policy_context_must_match_real_host_actor_and_site() {
        let opts = LocalToolOptions::new(".", ToolMode::Off);
        let actor = PrincipalId::new("host:local").unwrap();
        let (wrong_actor, wrong_actor_grant) = authenticated_context("host:other", "local");
        assert!(matches!(
            LocalHost::new(actor.clone(), &opts).with_authenticated_policy(
                &wrong_actor,
                &[wrong_actor_grant],
                &[]
            ),
            Err(Error::Denied(_))
        ));

        let (wrong_site, wrong_site_grant) = authenticated_context(actor.as_str(), "remote");
        assert!(matches!(
            LocalHost::new(actor.clone(), &opts).with_authenticated_policy(
                &wrong_site,
                &[wrong_site_grant],
                &[]
            ),
            Err(Error::Denied(_))
        ));
    }

    #[test]
    fn captured_tool_grant_evidence_dies_on_revoke_and_does_not_revive_on_regrant() {
        let actor = PrincipalId::new("host:local").unwrap();
        let opts = LocalToolOptions::new(".", ToolMode::ReadOnly);
        let builder = Agent::builder(Arc::new(NoInference), AgentOptions::new("test", "model"));
        let mut agent = configure_local(builder, actor.clone(), &opts)
            .unwrap()
            .build()
            .unwrap();
        let authority = agent.tool_binding_authority();
        let first = AuthenticatedToolGrantEvidence::capture(
            &authority,
            &actor,
            "read",
            "tools.fs.read",
            "d:local",
            "/workspace",
            "/workspace",
        )
        .unwrap();
        first.revalidate(&authority).unwrap();
        let first_digest = first.authority_sha256();
        let operation = first.operation_context();
        let context = PolicyContext {
            actor: actor.as_str(),
            domain: "d:local",
            active_team: None,
            workspace: "/workspace",
            session: None,
            graph: None,
            node: None,
            invocation: "call:read-1",
            operation,
        };
        assert!(aw_policy::authorize_exact(&context, &[first.as_policy_path()]).is_ok());

        agent
            .revoke_registered_source_tool(
                &actor,
                "read",
                &aw_agent_core::ToolSource::trusted_host(),
            )
            .unwrap();
        assert!(first.revalidate(&authority).is_err());

        agent
            .permit_registered_host_tool(actor.clone(), "read")
            .unwrap();
        assert!(
            first.revalidate(&authority).is_err(),
            "regrant must not resurrect a captured stale generation"
        );
        let second = AuthenticatedToolGrantEvidence::capture(
            &authority,
            &actor,
            "read",
            "tools.fs.read",
            "d:local",
            "/workspace",
            "/workspace",
        )
        .unwrap();
        second.revalidate(&authority).unwrap();
        assert_ne!(
            first.binding().authority_revision,
            second.binding().authority_revision
        );
        assert_ne!(first_digest, second.authority_sha256());
    }

    fn read_agent(
        actor: &PrincipalId,
    ) -> (Agent, ToolBindingAuthority, AuthenticatedToolGrantEvidence) {
        let opts = LocalToolOptions::new(".", ToolMode::ReadOnly);
        let builder = Agent::builder(Arc::new(NoInference), AgentOptions::new("test", "model"));
        let agent = configure_local(builder, actor.clone(), &opts)
            .unwrap()
            .build()
            .unwrap();
        let authority = agent.tool_binding_authority();
        let evidence = AuthenticatedToolGrantEvidence::capture(
            &authority,
            actor,
            "read",
            "fs.read",
            "d:local",
            "/workspace",
            "/workspace",
        )
        .unwrap();
        (agent, authority, evidence)
    }

    fn agent_ref(handle: &str) -> ResolvedExecutionRef {
        aw_tools::execution_ref::ExecutionRef::parse(&format!("/d:d/i:i/p:p/a:{handle}"))
            .unwrap()
            .resolve(None, None)
            .unwrap()
    }

    struct FactualExecutor {
        calls: Arc<AtomicUsize>,
        receipt: &'static str,
    }

    impl OriginalOperationExecutor for FactualExecutor {
        fn execute<'a>(&'a self) -> HostExecutionFuture<'a, aw_agent_core::ToolOutput> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok(aw_agent_core::ToolOutput::text(self.receipt)) })
        }
    }

    struct Admission {
        checks: Arc<AtomicUsize>,
        allow: bool,
    }

    impl DelegateAdmissionAuthority for Admission {
        fn revalidate(&self, _request: &DelegationRequest) -> Result<()> {
            self.checks.fetch_add(1, Ordering::SeqCst);
            if self.allow {
                Ok(())
            } else {
                Err(Error::Denied(
                    "delegate provider/payer/disclosure authority is not live",
                ))
            }
        }
    }

    struct SkipPort {
        calls: Arc<AtomicUsize>,
    }

    impl DelegateAgentPort for SkipPort {
        fn drive<'a>(
            &'a self,
            _request: &'a DelegationRequest,
            _gate: Arc<DelegatedExecutionGate>,
        ) -> HostExecutionFuture<'a, ()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
    }

    struct ExecutePort {
        calls: Arc<AtomicUsize>,
        late_error: bool,
        double_attempt: bool,
    }

    impl DelegateAgentPort for ExecutePort {
        fn drive<'a>(
            &'a self,
            _request: &'a DelegationRequest,
            gate: Arc<DelegatedExecutionGate>,
        ) -> HostExecutionFuture<'a, ()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                let _ = gate.execute_original().await?;
                if self.double_attempt {
                    assert!(
                        gate.execute_original().await.is_err(),
                        "same delegated gate cannot execute the original twice"
                    );
                }
                if self.late_error {
                    Err(Error::Denied("delegate failed after factual execution"))
                } else {
                    Ok(())
                }
            })
        }
    }

    fn delegate_request(delegate: &ResolvedExecutionRef) -> DelegationRequest {
        DelegationRequest::new(
            OperationId::Fs(aw_tools::filesystem::schema::Tool::Read),
            delegate.clone(),
        )
    }

    #[tokio::test]
    async fn local_strategy_executes_original_once_without_delegate_activity() {
        let actor = PrincipalId::new("host:local-executor").unwrap();
        let (_agent, authority, evidence) = read_agent(&actor);
        let calls = Arc::new(AtomicUsize::new(0));
        let original = Arc::new(FactualExecutor {
            calls: calls.clone(),
            receipt: "FACTUAL",
        });
        let output = execute_with_strategy(
            Strategy::Local,
            OperationId::Fs(aw_tools::filesystem::schema::Tool::Read),
            original,
            evidence,
            authority,
            None,
        )
        .await
        .unwrap();
        assert_eq!(output.content, "FACTUAL");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn delegate_prose_without_original_execution_cannot_fabricate_success() {
        let original_actor = PrincipalId::new("host:original").unwrap();
        let delegate_actor = PrincipalId::new("host:delegate").unwrap();
        let (_original_agent, original_authority, original_evidence) = read_agent(&original_actor);
        let (_delegate_agent, delegate_authority, delegate_evidence) = read_agent(&delegate_actor);
        let delegate_ref = agent_ref("delegate");
        let request = delegate_request(&delegate_ref);
        let original_calls = Arc::new(AtomicUsize::new(0));
        let original = Arc::new(FactualExecutor {
            calls: original_calls.clone(),
            receipt: "FACTUAL",
        });
        let port_calls = Arc::new(AtomicUsize::new(0));
        let port = Arc::new(SkipPort {
            calls: port_calls.clone(),
        });
        let checks = Arc::new(AtomicUsize::new(0));
        let admission = Arc::new(Admission {
            checks: checks.clone(),
            allow: true,
        });

        assert!(matches!(
            execute_with_strategy(
                Strategy::DelegateAgent,
                request.operation(),
                original,
                original_evidence,
                original_authority,
                Some(DelegateExecution {
                    request,
                    port,
                    delegate_binding: Some((delegate_evidence, delegate_authority)),
                    delegate_admission: admission,
                }),
            )
            .await,
            Err(Error::Denied(
                "delegate never invoked the original trusted executor"
            ))
        ));
        assert_eq!(original_calls.load(Ordering::SeqCst), 0);
        assert_eq!(port_calls.load(Ordering::SeqCst), 1);
        assert!(checks.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn delegate_returns_only_original_factual_receipt_and_gate_is_one_shot() {
        let original_actor = PrincipalId::new("host:original-once").unwrap();
        let delegate_actor = PrincipalId::new("host:delegate-once").unwrap();
        let (_original_agent, original_authority, original_evidence) = read_agent(&original_actor);
        let (_delegate_agent, delegate_authority, delegate_evidence) = read_agent(&delegate_actor);
        let delegate_ref = agent_ref("delegate-once");
        let request = delegate_request(&delegate_ref);
        let original_calls = Arc::new(AtomicUsize::new(0));
        let original = Arc::new(FactualExecutor {
            calls: original_calls.clone(),
            receipt: "FACTUAL_NATIVE_RECEIPT",
        });
        let port = Arc::new(ExecutePort {
            calls: Arc::new(AtomicUsize::new(0)),
            late_error: true,
            double_attempt: true,
        });
        let admission = Arc::new(Admission {
            checks: Arc::new(AtomicUsize::new(0)),
            allow: true,
        });

        let output = execute_with_strategy(
            Strategy::DelegateAgent,
            request.operation(),
            original,
            original_evidence,
            original_authority,
            Some(DelegateExecution {
                request,
                port,
                delegate_binding: Some((delegate_evidence, delegate_authority)),
                delegate_admission: admission,
            }),
        )
        .await
        .unwrap();
        assert_eq!(output.content, "FACTUAL_NATIVE_RECEIPT");
        assert_eq!(
            original_calls.load(Ordering::SeqCst),
            1,
            "delegate cannot execute or replay the original executor twice"
        );
    }

    #[tokio::test]
    async fn delegate_authority_or_control_cycle_denies_before_original_execution() {
        let original_actor = PrincipalId::new("host:original-deny").unwrap();
        let delegate_actor = PrincipalId::new("host:delegate-deny").unwrap();
        let (_original_agent, original_authority, original_evidence) = read_agent(&original_actor);
        let (_delegate_agent, delegate_authority, delegate_evidence) = read_agent(&delegate_actor);
        let delegate_ref = agent_ref("delegate-deny");
        let original_calls = Arc::new(AtomicUsize::new(0));
        let original = Arc::new(FactualExecutor {
            calls: original_calls.clone(),
            receipt: "SHOULD_NOT_RUN",
        });
        let port_calls = Arc::new(AtomicUsize::new(0));
        let port = Arc::new(SkipPort {
            calls: port_calls.clone(),
        });
        let denied = Arc::new(Admission {
            checks: Arc::new(AtomicUsize::new(0)),
            allow: false,
        });
        let request = delegate_request(&delegate_ref);
        assert!(execute_with_strategy(
            Strategy::DelegateAgent,
            request.operation(),
            original.clone(),
            original_evidence.clone(),
            original_authority.clone(),
            Some(DelegateExecution {
                request,
                port: port.clone(),
                delegate_binding: Some((delegate_evidence.clone(), delegate_authority.clone(),)),
                delegate_admission: denied,
            }),
        )
        .await
        .is_err());
        assert_eq!(original_calls.load(Ordering::SeqCst), 0);
        assert_eq!(port_calls.load(Ordering::SeqCst), 0);

        let self_ref = agent_ref("self");
        assert!(
            DelegationRequest::new(
                OperationId::Fs(aw_tools::filesystem::schema::Tool::Read),
                delegate_ref,
            )
            .with_control_target(self_ref.clone(), self_ref, Vec::new(), Vec::new())
            .is_err(),
            "self-targeted delegated control must reject before child admission"
        );
        assert_eq!(original_calls.load(Ordering::SeqCst), 0);
        assert_eq!(port_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn stale_delegate_tool_generation_denies_before_port_or_original() {
        let original_actor = PrincipalId::new("host:original-stale").unwrap();
        let delegate_actor = PrincipalId::new("host:delegate-stale").unwrap();
        let (_original_agent, original_authority, original_evidence) = read_agent(&original_actor);
        let (mut delegate_agent, delegate_authority, delegate_evidence) =
            read_agent(&delegate_actor);
        delegate_agent
            .revoke_registered_source_tool(
                &delegate_actor,
                "read",
                &aw_agent_core::ToolSource::trusted_host(),
            )
            .unwrap();

        let delegate_ref = agent_ref("delegate-stale");
        let request = delegate_request(&delegate_ref);
        let original_calls = Arc::new(AtomicUsize::new(0));
        let original = Arc::new(FactualExecutor {
            calls: original_calls.clone(),
            receipt: "SHOULD_NOT_RUN",
        });
        let port_calls = Arc::new(AtomicUsize::new(0));
        let port = Arc::new(SkipPort {
            calls: port_calls.clone(),
        });
        let admission = Arc::new(Admission {
            checks: Arc::new(AtomicUsize::new(0)),
            allow: true,
        });
        assert!(execute_with_strategy(
            Strategy::DelegateAgent,
            request.operation(),
            original,
            original_evidence,
            original_authority,
            Some(DelegateExecution {
                request,
                port,
                delegate_binding: Some((delegate_evidence, delegate_authority)),
                delegate_admission: admission,
            }),
        )
        .await
        .is_err());
        assert_eq!(original_calls.load(Ordering::SeqCst), 0);
        assert_eq!(port_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn unsupported_delegate_does_not_silently_fallback_to_local() {
        let opts = LocalToolOptions::new(".", ToolMode::Off);
        let host = LocalHost::new(PrincipalId::new("host:local").unwrap(), &opts);
        let effective = aw_policy::resolve(&[Policy {
            prefer: Some(Strategy::DelegateAgent),
            max_model_rounds: Some(4),
            ..Policy::default()
        }])
        .unwrap();
        assert!(matches!(
            host.with_effective_policy(effective),
            Err(Error::Denied(_))
        ));
    }
}
