use aw::{
    config::Settings, Agent, AgentOptions, CancellationToken, Error, LocalToolConcurrency,
    LocalToolLimits, LocalToolOptions, Result, ToolMode,
};
use aw_agent_core::{
    AgentExecutionBinding, AuxiliaryInput, AuxiliaryKind, ExecutionOwnershipLease,
    InvocationIdentity, JournalEntry, JournalFuture, PrincipalId, ProviderAccounts,
    ProviderSelection, Tool,
    ToolContext, ToolEffect, ToolFuture, ToolOutput, ToolSpec, TurnJournal, TurnStart,
};
#[cfg(feature = "routing")]
use aw_agent_core::{InvocationContext, RoutingExecutor, RoutingInput, RoutingJournal, RoutingResult};
#[cfg(feature = "routing")]
use aw_routing::{target as routing_target, RoutingMode};
use aw_store::{
    DelegationLimits, PolicyAdmissionRequest, PolicyAdmissionState, QueueState, SessionStore,
    SubagentInfo, SubagentMode,
};
use aw_host::{
    execute_with_strategy, AuthenticatedToolGrantEvidence, DelegateAdmissionAuthority,
    DelegateAgentPort, DelegateExecution, DelegatedExecutionGate, DelegationRequest,
    HostExecutionFuture, OriginalOperationExecutor,
};
use aw_store_api::SessionBackend;
use aw_tools::{
    coordination::{ApprovalId, ApprovalState, ApprovalTarget, PendingApproval, PendingApprovalSet},
    execution_ref::{ExecutionRef, ExecutionSegment, Handle, NodeCoordinate, ResolvedExecutionRef, Revision},
    filesystem::schema::Tool as FsTool,
    operation::OperationId,
};
use aw_tools_adapter::operation_router::{
    GraphFootprintOverride, GraphNodeExecutionFuture, GraphNodeOriginalExecutor,
    GraphNodePolicyRequest, GraphPolicyFuture, GraphPolicyRequest, GraphPolicyRouter,
    GraphPresealDecision, GraphPresealFuture, GraphPresealNode, GraphPresealRequest,
    OperationRouter,
};
#[cfg(feature = "routing")]
use aw_store::{RoutingSelectionSource, RoutingStrategyRevisionInfo};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_ACTIVE_BACKGROUND: usize = 4;
const MAX_TOOL_ANSWER_BYTES: usize = 96 * 1024;

#[derive(Clone)]
pub struct Service {
    inner: Arc<Inner>,
    lineage: Option<Lineage>,
}

#[derive(Clone, Debug)]
struct Lineage {
    root_id: String,
    allocation_id: String,
    depth: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct BudgetConfig {
    limits: DelegationLimits,
}
impl BudgetConfig {
    pub fn new(
        max_children_created: u32,
        max_active_children: u32,
        max_model_attempts: u32,
        max_depth: u8,
        deadline_seconds: u32,
    ) -> Result<Self> {
        Ok(Self {
            limits: DelegationLimits {
                max_children_created,
                max_active_children,
                max_model_attempts,
                max_depth,
                deadline_seconds,
            }
            .validate()?,
        })
    }
}
impl Default for BudgetConfig {
    fn default() -> Self {
        Self::new(32, 4, 64, 1, 3600).expect("default delegation budget is valid")
    }
}

#[cfg(feature = "routing")]
#[derive(Clone)]
pub struct ChildRoutingConfig {
    strategy: RoutingStrategyRevisionInfo,
    profiles: std::collections::BTreeMap<String, aw::config::Profile>,
    executor: RoutingExecutor,
    idle_timeout: Duration,
    request_timeout: Duration,
}
#[cfg(feature = "routing")]
impl ChildRoutingConfig {
    pub fn new(
        strategy: RoutingStrategyRevisionInfo,
        file_config: &aw::config::FileConfig,
        executor: RoutingExecutor,
        idle_timeout: Duration,
        request_timeout: Duration,
    ) -> Result<Self> {
        if strategy.definition.role != "worker" {
            return Err(Error::Config(
                "subagent routing strategy must have role=worker".into(),
            ));
        }
        let profiles = crate::routing_runtime::preflight_strategy(file_config, &strategy.definition)?;
        Ok(Self {
            strategy,
            profiles,
            executor,
            idle_timeout,
            request_timeout,
        })
    }
}
#[cfg(not(feature = "routing"))]
#[derive(Clone)]
pub struct ChildRoutingConfig;

#[derive(Clone)]
pub struct ServiceConfig {
    child_tool_mode: ToolMode,
    child_tool_concurrency: LocalToolConcurrency,
    child_tool_limits: LocalToolLimits,
    budget: BudgetConfig,
    child_routing: Option<ChildRoutingConfig>,
    state_dir: PathBuf,
    trusted_git: Option<PathBuf>,
    child_actors: Option<crate::child_actors::Service>,
}
impl ServiceConfig {
    pub fn new(
        child_tool_mode: ToolMode,
        budget: BudgetConfig,
        state_dir: PathBuf,
        trusted_git: Option<PathBuf>,
    ) -> Self {
        Self {
            child_tool_mode,
            child_tool_concurrency: LocalToolConcurrency::default(),
            child_tool_limits: LocalToolLimits::default(),
            budget,
            child_routing: None,
            state_dir,
            trusted_git,
            child_actors: None,
        }
    }

    pub fn with_child_routing(mut self, child_routing: Option<ChildRoutingConfig>) -> Self {
        self.child_routing = child_routing;
        self
    }
    /// Host-only resource ceilings inherited by every tool-enabled child.
    /// These are never parsed from model subagent arguments.
    pub fn with_child_tool_runtime(
        mut self,
        concurrency: LocalToolConcurrency,
        limits: LocalToolLimits,
    ) -> Result<Self> {
        if self.child_tool_mode == ToolMode::Off {
            self.child_tool_concurrency = concurrency;
            self.child_tool_limits = limits;
            return Ok(self);
        }
        self.child_tool_concurrency = concurrency.validate()?;
        let names: &[&str] = match self.child_tool_mode {
            ToolMode::ReadOnly => &["read"],
            ToolMode::Workspace => &["read", "write", "edit"],
            ToolMode::Off => unreachable!("handled above"),
            ToolMode::Trusted => &["read", "write", "edit"],
        };
        self.child_tool_limits = limits.validate_for_names(names, false, false)?;
        Ok(self)
    }
    /// Explicit host-only opt-in to Actor metadata/control for existing
    /// durable child Sessions; the ordinary child path remains unchanged.
    #[allow(dead_code)] // AW-P6-7 hosted opt-in until AW-P6-9 root Session owner wiring
    pub fn with_child_actors(mut self, service: crate::child_actors::Service) -> Self {
        self.child_actors=Some(service);
        self
    }
}

struct Inner {
    store: SessionStore,
    accounts: ProviderAccounts,
    settings: Settings,
    child_tool_mode: ToolMode,
    child_tool_concurrency: LocalToolConcurrency,
    child_tool_limits: LocalToolLimits,
    budget: BudgetConfig,
    #[cfg(feature = "routing")]
    child_routing: Option<ChildRoutingConfig>,
    #[cfg(feature = "workspace")]
    state_dir: PathBuf,
    #[cfg(feature = "workspace")]
    git: Option<PathBuf>,
    active_background: AtomicUsize,
    child_actors: Option<crate::child_actors::Service>,
    agent_jobs: Mutex<Option<Arc<aw::agent_jobs::Router>>>,
}

struct BudgetJournal {
    inner: Arc<dyn TurnJournal>,
    store: SessionStore,
    actor: aw_agent_core::PrincipalId,
    root_id: String,
    child_session_id: String,
}
impl TurnJournal for BudgetJournal {
    fn validate_context(&self) -> JournalFuture<'_, ()> {
        self.inner.validate_context()
    }
    fn validate_context_as<'a>(&'a self, identity: &'a InvocationIdentity) -> JournalFuture<'a, ()> {
        self.inner.validate_context_as(identity)
    }
    fn additional_context(&self) -> JournalFuture<'_, Vec<aw_agent_core::ContextBlock>> {
        self.inner.additional_context()
    }
    fn requires_output_gate(&self) -> JournalFuture<'_, bool> {
        self.inner.requires_output_gate()
    }
    fn invocation_scope(&self) -> Result<Option<aw_agent_core::InvocationScope>> {
        self.inner.invocation_scope()
    }
    fn begin<'a>(&'a self, start: TurnStart) -> JournalFuture<'a, ()> {
        self.inner.begin(start)
    }
    fn append<'a>(&'a self, turn_id: u64, entry: JournalEntry) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            if let JournalEntry::InvocationStarted { ordinal, identity } = &entry {
                self.store.reserve_delegation_model_attempt(
                    &self.actor,
                    &self.root_id,
                    &self.child_session_id,
                    turn_id,
                    *ordinal,
                    identity,
                )?;
                if let Err(error) = self.store.consume_delegation_continuation_for_turn(
                    &self.actor,
                    &self.child_session_id,
                    turn_id,
                ) {
                    let detail = format!(
                        "continuation conversion failed after model-attempt reservation: {error}"
                    );
                    let bounded = detail.chars().take(2048).collect::<String>();
                    let _ = self.store.mark_delegation_model_attempt_unknown(
                        &self.actor,
                        &self.root_id,
                        &self.child_session_id,
                        turn_id,
                        *ordinal,
                        &bounded,
                    );
                    return Err(error);
                }
            }
            if let Err(error) = self.inner.append(turn_id, entry.clone()).await {
                if let JournalEntry::InvocationStarted { ordinal, .. } = &entry {
                    let detail = format!("ordinary invocation intent failed after budget reservation: {error}");
                    let bounded = detail.chars().take(2048).collect::<String>();
                    let _ = self.store.mark_delegation_model_attempt_unknown(
                        &self.actor,
                        &self.root_id,
                        &self.child_session_id,
                        turn_id,
                        *ordinal,
                        &bounded,
                    );
                }
                return Err(error);
            }
            if let JournalEntry::InvocationFinished { record } = &entry {
                self.store.finish_delegation_model_attempt(
                    &self.actor,
                    &self.root_id,
                    &self.child_session_id,
                    turn_id,
                    record,
                )?;
            }
            Ok(())
        })
    }
    fn finish<'a>(&'a self, turn: aw_agent_core::Turn) -> JournalFuture<'a, ()> {
        self.inner.finish(turn)
    }
    fn acknowledge<'a>(&'a self, note: String) -> JournalFuture<'a, ()> {
        self.inner.acknowledge(note)
    }
}

#[cfg(feature = "routing")]
struct BudgetRoutingJournal {
    inner: Arc<dyn RoutingJournal>,
    store: SessionStore,
    actor: aw_agent_core::PrincipalId,
    root_id: String,
    allocation_id: String,
    operation_id: String,
    classifier_identity: InvocationIdentity,
}
#[cfg(feature = "routing")]
impl RoutingJournal for BudgetRoutingJournal {
    fn authorize<'a>(
        &'a self,
        input: &'a RoutingInput,
        identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, ()> {
        self.inner.authorize(input, identity)
    }

    fn begin<'a>(
        &'a self,
        input: &'a RoutingInput,
        identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, InvocationContext> {
        Box::pin(async move {
            if identity != &self.classifier_identity {
                return Err(Error::Denied(
                    "delegation routing classifier identity changed before dispatch",
                ));
            }
            self.store.reserve_delegation_routing_attempt(
                &self.actor,
                &self.root_id,
                &self.allocation_id,
                &self.operation_id,
                identity,
            )?;
            match self.inner.begin(input, identity).await {
                Ok(context) => Ok(context),
                Err(error) => {
                    let detail = format!(
                        "ordinary routing intent failed after delegation budget reservation: {error}"
                    );
                    let bounded = detail.chars().take(2048).collect::<String>();
                    let _ = self.store.mark_delegation_routing_attempt_unknown(
                        &self.actor,
                        &self.allocation_id,
                        &bounded,
                    );
                    Err(error)
                }
            }
        })
    }

    fn finish<'a>(&'a self, result: &'a RoutingResult) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            self.inner.finish(result).await?;
            self.store.finish_delegation_routing_attempt(
                &self.actor,
                &self.allocation_id,
                &self.classifier_identity,
                result,
            )?;
            Ok(())
        })
    }
}

fn unix_micros() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
        .min(i64::MAX as u128) as i64
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpawnArgs {
    task: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    mode: ModeArg,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentSpawnArgs {
    task: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    background: bool,
}

#[derive(Clone)]
struct AgentConstructor {
    service: Service,
}


#[derive(Clone)]
struct PolicyGateSpec {
    tool: Arc<dyn Tool>,
    name: String,
    effect: ToolEffect,
}

#[derive(Clone)]
struct DelegatedOriginalTool {
    gate: Arc<DelegatedExecutionGate>,
    name: String,
    effect: ToolEffect,
}

impl Tool for DelegatedOriginalTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name.clone(),
            description: "Execute exactly the prepared original trusted operation once. This tool takes no arguments; its factual ToolOutput is owned by the original executor, not by this delegate Agent.".into(),
            parameters: json!({
                "type":"object",
                "properties":{},
                "additionalProperties":false
            }),
            effect: self.effect.clone(),
        }
    }

    fn validate(&self, arguments: &Value) -> Result<()> {
        if arguments.as_object().is_none_or(|object| !object.is_empty()) {
            return Err(Error::Denied(
                "delegated original execution tool accepts no model-controlled arguments",
            ));
        }
        Ok(())
    }

    fn execute<'a>(&'a self, _ctx: ToolContext, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(arguments)?;
            self.gate.execute_original().await
        })
    }
}

fn delegated_operation_effect(operation: OperationId) -> Result<ToolEffect> {
    Ok(match operation {
        OperationId::Fs(
            FsTool::Ls | FsTool::Find | FsTool::Grep | FsTool::Read | FsTool::Bread,
        ) => ToolEffect::Read,
        OperationId::Fs(
            FsTool::Write
            | FsTool::Edit
            | FsTool::Patch
            | FsTool::Copy
            | FsTool::Move
            | FsTool::Remove
            | FsTool::Bwrite
            | FsTool::Bedit,
        ) => ToolEffect::Write,
        OperationId::Bash | OperationId::Pwsh | OperationId::Powershell | OperationId::Agent => {
            ToolEffect::Process
        }
        // Whole-Graph delegation coordinates the already-prepared Batch as
        // one opaque original executor. The existing Batch/JobManager remains
        // the sole Graph owner; the delegate receives no node arguments here.
        OperationId::Batch => ToolEffect::Control,
        OperationId::JobsList
        | OperationId::JobsGet
        | OperationId::JobsRead
        | OperationId::JobsAck
        | OperationId::JobsSend
        | OperationId::JobsCancel
        | OperationId::JobsSubscribe => ToolEffect::Control,
    })
}

fn kernel_effect(effect: &ToolEffect) -> aw_agent_kernel::capability::Effect {
    match effect {
        ToolEffect::Read => aw_agent_kernel::capability::Effect::Read,
        ToolEffect::Write => aw_agent_kernel::capability::Effect::Write,
        ToolEffect::Process => aw_agent_kernel::capability::Effect::Process,
        ToolEffect::Control => aw_agent_kernel::capability::Effect::Control,
        ToolEffect::Unknown => aw_agent_kernel::capability::Effect::Unknown,
    }
}



#[derive(Clone, Copy, Debug)]
pub struct PolicyOperationConfig {
    policy: aw_policy::Policy,
    revision: u64,
    enabled: bool,
    guardian: bool,
}

impl PolicyOperationConfig {
    pub const fn passthrough() -> Self {
        Self {
            policy: aw_policy::Policy {
                deny: false,
                routes: aw_policy::Routes::LOCAL,
                prefer: Some(aw_policy::Strategy::Local),
                require_preferred: true,
                max_model_rounds: None,
            },
            revision: 1,
            enabled: false,
            guardian: false,
        }
    }

    pub fn deterministic(policy: aw_policy::Policy, revision: u64) -> Result<Self> {
        if revision == 0 {
            return Err(Error::Denied("policy operation revision must be nonzero"));
        }
        aw_policy::resolve_scoped(&[aw_policy::PolicyLayer {
            scope: aw_policy::PolicyScope::ToolCall,
            revision,
            policy: &policy,
        }])
        .map_err(|_| Error::Denied("invalid deterministic operation policy"))?;
        Ok(Self {
            policy,
            revision,
            enabled: true,
            guardian: false,
        })
    }

    pub fn with_guardian(mut self, enabled: bool) -> Self {
        self.guardian = enabled;
        self
    }

    pub const fn policy(self) -> aw_policy::Policy {
        self.policy
    }

    pub const fn revision(self) -> u64 {
        self.revision
    }
}

struct PreparedOriginalOperation {
    original: Arc<dyn Tool>,
    ctx: ToolContext,
    arguments: Value,
}

impl OriginalOperationExecutor for PreparedOriginalOperation {
    fn execute<'a>(&'a self) -> HostExecutionFuture<'a, ToolOutput> {
        let original = self.original.clone();
        let ctx = self.ctx.clone();
        let arguments = self.arguments.clone();
        Box::pin(async move {
            original.validate(&arguments)?;
            original.execute(ctx, &arguments).await
        })
    }
}

#[derive(Clone)]
pub struct PolicyOperationRouter {
    service: Service,
    authority: aw_agent_core::tool::ToolBindingAuthority,
    config: PolicyOperationConfig,
}

impl PolicyOperationRouter {
    pub fn new(
        service: Service,
        authority: aw_agent_core::tool::ToolBindingAuthority,
        config: PolicyOperationConfig,
    ) -> Result<Self> {
        if config.enabled {
            PolicyOperationConfig::deterministic(config.policy, config.revision)?;
        }
        Ok(Self {
            service,
            authority,
            config,
        })
    }

    fn current_agent_ref(ctx: &ToolContext) -> Result<ResolvedExecutionRef> {
        let owner = ctx
            .execution_owner
            .as_ref()
            .ok_or(Error::Denied(
                "policy operation routing requires a bound Agent owner",
            ))?;
        ExecutionRef::parse(owner.canonical_ref())
            .map_err(|_| Error::Denied("invalid bound Agent execution ref"))?
            .resolve(None, None)
            .map_err(|_| Error::Denied("invalid bound Agent execution ref"))
    }

    fn project_scope(agent: &ResolvedExecutionRef) -> String {
        let a = agent.agent_ref();
        format!("/d:{}/i:{}/p:{}", a.domain, a.identity, a.project)
    }

    fn target_domain(agent: &ResolvedExecutionRef) -> String {
        format!("d:{}", agent.agent_ref().domain)
    }

    fn canonical_target(
        operation: OperationId,
        ctx: &ToolContext,
        arguments: &Value,
    ) -> Result<String> {
        let current = Self::current_agent_ref(ctx)?;
        let ref_value = match operation {
            OperationId::JobsGet
            | OperationId::JobsRead
            | OperationId::JobsSend => arguments.get("ref").and_then(Value::as_str).or(Some(".")),
            OperationId::JobsAck | OperationId::JobsCancel => {
                arguments.get("ref").and_then(Value::as_str)
            }
            _ => None,
        };
        if let Some(reference) = ref_value {
            return ExecutionRef::parse(reference)
                .map_err(|_| Error::Denied("invalid policy-routed execution ref"))?
                .resolve(Some(current.agent_ref()), None)
                .map(|resolved| resolved.canonical())
                .map_err(|_| Error::Denied("policy-routed ref needs unavailable context"));
        }
        Ok(current.canonical())
    }

    fn canonical_json_hash(value: &Value) -> [u8; 32] {
        fn write_value(hash: &mut Sha256, value: &Value) {
            match value {
                Value::Null => hash.update([0]),
                Value::Bool(value) => {
                    hash.update([1, u8::from(*value)]);
                }
                Value::Number(value) => {
                    hash.update([2]);
                    hash.update(value.to_string().as_bytes());
                }
                Value::String(value) => {
                    hash.update([3]);
                    hash.update((value.len() as u64).to_be_bytes());
                    hash.update(value.as_bytes());
                }
                Value::Array(values) => {
                    hash.update([4]);
                    hash.update((values.len() as u64).to_be_bytes());
                    for value in values {
                        write_value(hash, value);
                    }
                }
                Value::Object(values) => {
                    hash.update([5]);
                    hash.update((values.len() as u64).to_be_bytes());
                    let mut keys = values.keys().collect::<Vec<_>>();
                    keys.sort_unstable();
                    for key in keys {
                        hash.update((key.len() as u64).to_be_bytes());
                        hash.update(key.as_bytes());
                        write_value(hash, &values[key]);
                    }
                }
            }
        }
        let mut hash = Sha256::new();
        hash.update(b"aw-policy-operation-request-v1\0");
        write_value(&mut hash, value);
        hash.finalize().into()
    }

    fn request_sha256(operation: OperationId, arguments: &Value) -> String {
        let mut hash = Sha256::new();
        hash.update(b"aw-policy-operation-semantic-request-v1\0");
        hash.update(operation.semantic_name().as_bytes());
        hash.update(Self::canonical_json_hash(arguments));
        format!("{:x}", hash.finalize())
    }

    fn policy_sha256(&self) -> String {
        let policy = self.config.policy;
        let mut hash = Sha256::new();
        hash.update(b"aw-policy-operation-config-v1\0");
        hash.update(self.config.revision.to_be_bytes());
        hash.update([
            u8::from(policy.deny),
            u8::from(policy.routes.contains(aw_policy::Strategy::Local)),
            u8::from(policy.routes.contains(aw_policy::Strategy::DelegateAgent)),
            match policy.prefer {
                None => 0,
                Some(aw_policy::Strategy::Local) => 1,
                Some(aw_policy::Strategy::DelegateAgent) => 2,
            },
            u8::from(policy.require_preferred),
            u8::from(self.config.guardian),
        ]);
        hash.update(policy.max_model_rounds.unwrap_or(0).to_be_bytes());
        format!("{:x}", hash.finalize())
    }

    fn decision_digest(
        revision: u64,
        route: aw_policy::Strategy,
        label: &str,
    ) -> String {
        let mut hash = Sha256::new();
        hash.update(b"aw-policy-operation-decision-v1\0");
        hash.update(revision.to_be_bytes());
        hash.update([match route {
            aw_policy::Strategy::Local => 1,
            aw_policy::Strategy::DelegateAgent => 2,
        }]);
        hash.update(label.as_bytes());
        format!("{:x}", hash.finalize())
    }

    fn selected_route(
        fact: &aw_store::PolicyAdmissionFact,
        fallback: aw_policy::Strategy,
    ) -> Result<aw_policy::Strategy> {
        match fact.decision_route.as_deref() {
            None => Ok(fallback),
            Some("local") => Ok(aw_policy::Strategy::Local),
            Some("delegate_agent") => Ok(aw_policy::Strategy::DelegateAgent),
            Some(_) => Err(Error::Protocol("invalid persisted policy route")),
        }
    }

    fn delegation_request(
        operation: OperationId,
        ctx: &ToolContext,
        target: &str,
    ) -> Result<DelegationRequest> {
        let mut request = DelegationRequest::transient(operation);
        let is_control = matches!(
            operation,
            OperationId::JobsAck | OperationId::JobsSend | OperationId::JobsCancel
        );
        let waits = matches!(operation, OperationId::JobsGet | OperationId::JobsRead);
        if !is_control && !waits {
            return Ok(request);
        }
        let current = Self::current_agent_ref(ctx)?;
        let current_turn = ResolvedExecutionRef::new(
            current.agent_ref().clone(),
            vec![ExecutionSegment::Turn(
                Handle::new(ctx.turn_id.to_string())
                    .map_err(|_| Error::Denied("invalid current Turn handle"))?,
            )],
        )
        .map_err(|_| Error::Denied("invalid current Turn ref"))?;
        let target = ExecutionRef::parse(target)
            .map_err(|_| Error::Denied("invalid delegated control target"))?
            .resolve(Some(current.agent_ref()), None)
            .map_err(|_| Error::Denied("delegated control target needs context"))?;
        request = request.with_control_target(
            target,
            current_turn,
            vec![current],
            Vec::new(),
        )?;
        Ok(request)
    }

    async fn route(
        &self,
        operation: OperationId,
        ctx: ToolContext,
        arguments: Value,
        original: Arc<dyn Tool>,
    ) -> Result<ToolOutput> {
        if !self.config.enabled {
            return original.execute(ctx, &arguments).await;
        }
        original.validate(&arguments)?;
        let scope = ctx.scope.as_ref().ok_or(Error::Denied(
            "policy operation routing requires a durable Session/branch",
        ))?;
        let owner = ctx.execution_owner.as_ref().ok_or(Error::Denied(
            "policy operation routing requires an immutable Agent binding",
        ))?;
        if owner.session_id() != scope.session_id || owner.branch_id() != scope.branch_id {
            return Err(Error::Denied(
                "policy operation scope differs from bound Agent identity",
            ));
        }
        if ctx.actor != ctx.identity.actor
            || ctx.source != aw_agent_core::ToolSource::trusted_host()
            || ctx.execution_site.is_empty()
        {
            return Err(Error::Denied(
                "policy operation source/actor/site is not an authenticated Host invocation",
            ));
        }

        let current = Self::current_agent_ref(&ctx)?;
        let project_scope = Self::project_scope(&current);
        let target_domain = Self::target_domain(&current);
        let target = Self::canonical_target(operation, &ctx, &arguments)?;
        let evidence = AuthenticatedToolGrantEvidence::capture(
            &self.authority,
            &ctx.actor,
            operation.model_name(),
            operation.semantic_name(),
            target_domain.clone(),
            target.clone(),
            project_scope.clone(),
        )?;
        let binding = evidence.binding();
        if binding.source != ctx.source
            || binding.effect != ctx.effect
            || binding.execution_site != ctx.execution_site
        {
            return Err(Error::Denied(
                "policy operation context differs from exact live ToolBinding",
            ));
        }

        let invocation = format!("turn:{}/call:{}", ctx.turn_id, ctx.call_id);
        let operation_context = evidence.operation_context();
        let policy_context = aw_policy::PolicyContext {
            actor: ctx.actor.as_str(),
            domain: &target_domain,
            active_team: None,
            workspace: &project_scope,
            session: Some(owner.canonical_ref()),
            graph: None,
            node: None,
            invocation: &invocation,
            operation: operation_context,
        };
        let layer = aw_policy::PolicyLayer {
            scope: aw_policy::PolicyScope::ToolCall,
            revision: self.config.revision,
            policy: &self.config.policy,
        };
        let grant = evidence.as_policy_path();
        let authorized = aw_policy::resolve_authorized(&policy_context, &[grant], &[layer])
            .map_err(|_| Error::Denied("effective operation policy denied"))?;
        let effective = authorized.effective();

        let session = self.service.inner.store.session(&ctx.actor, &scope.session_id)?;
        let writer = self.service.inner.store.workspace_writer(
            &ctx.actor,
            None,
            &session.workspace_id,
        )?;
        if writer.state != "active" {
            return Err(Error::Denied("policy operation writer is no longer active"));
        }
        let policy_sha = self.policy_sha256();
        let authority_sha = evidence.authority_sha256();
        let selected_for_guardian = if self.config.guardian {
            Some(self.service.exact_selection(&ctx)?)
        } else {
            None
        };
        let request = PolicyAdmissionRequest {
            session_id: scope.session_id.clone(),
            branch_id: scope.branch_id.clone(),
            writer_epoch: writer.writer_epoch,
            turn_id: ctx.turn_id,
            call_id: ctx.call_id.clone(),
            canonical_target: target.clone(),
            request_sha256: Self::request_sha256(operation, &arguments),
            policy_revision_sha256: policy_sha.clone(),
            authority_sha256: authority_sha.clone(),
            candidate_local: effective.routes().contains(aw_policy::Strategy::Local),
            candidate_delegate_agent: effective
                .routes()
                .contains(aw_policy::Strategy::DelegateAgent),
            guardian_required: self.config.guardian,
            guardian_identity: selected_for_guardian
                .as_ref()
                .map(|selected| selected.identity().clone()),
            guardian_site: selected_for_guardian
                .as_ref()
                .map(|selected| selected.identity().inference_executor.clone()),
            approval_scope: None,
            authorized_approvers: Vec::new(),
            approval_expires_at: None,
        };
        let claimed = self
            .service
            .inner
            .store
            .policy_admission_claim(&ctx.actor, request)?;

        let decided = if self.config.guardian {
            let selected = selected_for_guardian
                .as_ref()
                .ok_or(Error::Denied("guardian selection disappeared"))?;
            let scratch = Agent::builder_from_selection(
                selected.clone(),
                AgentOptions::new(
                    selected.identity().account_id.clone(),
                    selected.identity().model.clone(),
                ),
            )
            .build()?;
            let guardian_prompt = format!(
                "Return strict JSON only with decision allow or deny and optional route local or delegate_agent. Operation: {}. Effect: {:?}. Site: {}. Target scope: {}. Do not request or infer hidden arguments.",
                operation.semantic_name(),
                ctx.effect,
                ctx.execution_site,
                target,
            );
            let input = AuxiliaryInput {
                kind: AuxiliaryKind::Side,
                messages: scratch
                    .auxiliary_messages(selected, &guardian_prompt)
                    .await?,
                max_output_bytes: 8 * 1024,
                timeout: self
                    .service
                    .inner
                    .settings
                    .request_timeout
                    .min(Duration::from_secs(60)),
            };
            aw::policy_runtime::run_admission_guardian(
                &self.service.inner.store,
                &ctx.actor,
                claimed.request.clone(),
                authorized,
                selected,
                &input,
                self.config.revision,
                ctx.cancellation.clone().cancelled(),
            )
            .await?
        } else {
            match claimed.state {
                PolicyAdmissionState::Claimed => {
                    let route = effective.strategy();
                    let digest = Self::decision_digest(self.config.revision, route, "allow");
                    self.service.inner.store.policy_admission_record_decision(
                        &ctx.actor,
                        &scope.session_id,
                        &claimed.id,
                        aw_policy::PolicyDecision {
                            decision: aw_policy::Decision::Allow,
                            revision: self.config.revision,
                            route: Some(route),
                            approval_scope: None,
                            narrow_ref: None,
                        },
                        &digest,
                    )?
                }
                _ => claimed,
            }
        };

        if decided.state == PolicyAdmissionState::Consumed {
            return Err(Error::Conflict(
                "policy admission already consumed; recover the original factual receipt",
            ));
        }
        match decided.state {
            PolicyAdmissionState::Denied
            | PolicyAdmissionState::AskDenied
            | PolicyAdmissionState::AskExpired => {
                return Err(Error::Denied("operation denied by admitted host policy"));
            }
            PolicyAdmissionState::AskPending => {
                return Err(Error::Conflict(
                    "operation awaits explicit policy approval before execution",
                ));
            }
            PolicyAdmissionState::Narrowed => {
                return Err(Error::Denied(
                    "direct Tool narrowing requires the pre-effect revision admission path",
                ));
            }
            PolicyAdmissionState::GuardianStarted
            | PolicyAdmissionState::GuardianUnknown
            | PolicyAdmissionState::GuardianSettled
            | PolicyAdmissionState::Claimed => {
                return Err(Error::Conflict(
                    "policy admission is unresolved and cannot execute",
                ));
            }
            PolicyAdmissionState::Allowed | PolicyAdmissionState::AskApproved => {}
            PolicyAdmissionState::Consumed => unreachable!("handled above"),
        }

        let strategy = Self::selected_route(&decided, effective.strategy())?;
        evidence.revalidate(&self.authority)?;
        let consumed = self.service.inner.store.policy_admission_consume(
            &ctx.actor,
            &scope.session_id,
            &decided.id,
            &policy_sha,
            &authority_sha,
            Some(strategy),
            None,
        )?;
        if consumed.state != PolicyAdmissionState::Consumed {
            return Err(Error::Conflict(
                "policy admission did not reach executable consumed state",
            ));
        }

        let original_executor = Arc::new(PreparedOriginalOperation {
            original,
            ctx: ctx.clone(),
            arguments,
        });
        let delegated = if strategy == aw_policy::Strategy::DelegateAgent {
            let request = Self::delegation_request(operation, &ctx, &target)?;
            Some(DelegateExecution {
                request,
                port: self.service.policy_delegate_port(&ctx),
                delegate_binding: None,
                delegate_admission: self.service.policy_delegate_admission(&ctx),
            })
        } else {
            None
        };
        execute_with_strategy(
            strategy,
            operation,
            original_executor,
            evidence,
            self.authority.clone(),
            delegated,
        )
        .await
    }
}

impl OperationRouter for PolicyOperationRouter {
    fn execute<'a>(
        &'a self,
        operation: OperationId,
        ctx: ToolContext,
        arguments: &'a Value,
        original: Arc<dyn Tool>,
    ) -> ToolFuture<'a> {
        let arguments = arguments.clone();
        Box::pin(async move { self.route(operation, ctx, arguments, original).await })
    }
}


#[derive(Clone, Debug, PartialEq)]
pub struct GraphNarrowProposal {
    pub narrow_ref: String,
    pub arguments: Value,
}

/// Trusted host-only argument revision source. Policy/guardian output can select
/// one opaque narrow_ref, but never supplies replacement arguments directly.
pub trait GraphNarrowAuthority: Send + Sync {
    fn proposal(
        &self,
        ctx: &ToolContext,
        node: &GraphPresealNode,
    ) -> Result<Option<GraphNarrowProposal>>;
}

#[derive(Clone)]
pub struct PolicyGraphConfig {
    policy: aw_policy::Policy,
    revision: u64,
    guardian: bool,
    authorized_approvers: Vec<PrincipalId>,
    approval_ttl: Duration,
    narrow_authority: Option<Arc<dyn GraphNarrowAuthority>>,
}

impl PolicyGraphConfig {
    pub fn deterministic(policy: aw_policy::Policy, revision: u64) -> Result<Self> {
        if revision == 0 {
            return Err(Error::Denied("Graph policy revision must be nonzero"));
        }
        aw_policy::resolve_scoped(&[aw_policy::PolicyLayer {
            scope: aw_policy::PolicyScope::Node,
            revision,
            policy: &policy,
        }])
        .map_err(|_| Error::Denied("invalid Graph policy"))?;
        Ok(Self {
            policy,
            revision,
            guardian: false,
            authorized_approvers: Vec::new(),
            approval_ttl: Duration::from_secs(300),
            narrow_authority: None,
        })
    }

    pub fn with_narrow_authority(
        mut self,
        authority: Arc<dyn GraphNarrowAuthority>,
    ) -> Self {
        self.narrow_authority = Some(authority);
        self
    }

    pub fn with_guardian(
        mut self,
        approvers: Vec<PrincipalId>,
        approval_ttl: Duration,
    ) -> Result<Self> {
        if approval_ttl.is_zero() || approval_ttl > Duration::from_secs(86_400) {
            return Err(Error::Limit("Graph approval TTL must be 1..86400 seconds"));
        }
        let mut names = approvers
            .iter()
            .map(|principal| principal.as_str().to_owned())
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        if names.len() != approvers.len() || names.len() > 32 {
            return Err(Error::Denied(
                "Graph policy approvers must be unique and bounded",
            ));
        }
        self.guardian = true;
        self.authorized_approvers = approvers;
        self.approval_ttl = approval_ttl;
        Ok(self)
    }
}

#[derive(Clone)]
struct GraphNodeStatusExecutor {
    original: Arc<dyn GraphNodeOriginalExecutor>,
    status: Arc<Mutex<Option<aw_tools::filesystem::batch::Status>>>,
}

impl OriginalOperationExecutor for GraphNodeStatusExecutor {
    fn execute<'a>(&'a self) -> HostExecutionFuture<'a, ToolOutput> {
        let original = self.original.clone();
        let status = self.status.clone();
        Box::pin(async move {
            let factual = original.execute().await?;
            *status
                .lock()
                .map_err(|_| Error::Storage("Graph node delegate status lock poisoned"))? =
                Some(factual);
            Ok(ToolOutput::text("graph node factual execution completed"))
        })
    }
}

#[derive(Clone)]
struct GraphAdmission {
    admission_id: String,
    session_id: String,
    policy_sha256: String,
    authority_sha256: String,
    evidence: AuthenticatedToolGrantEvidence,
    route: aw_policy::Strategy,
    replay_identity: [u8; 32],
}

#[derive(Clone)]
struct PendingGraphState {
    session_id: String,
    set: PendingApprovalSet,
    by_coordinate: BTreeMap<u32, String>,
}

#[derive(Clone)]
struct PresealedGraphAdmission {
    target: String,
    evidence: AuthenticatedToolGrantEvidence,
    authorized: aw_policy::AuthorizedPolicy,
    fact: aw_store::PolicyAdmissionFact,
    original_replay_identity: [u8; 32],
}

#[derive(Clone)]
pub struct PolicyGraphRouter {
    service: Service,
    authority: aw_agent_core::tool::ToolBindingAuthority,
    config: PolicyGraphConfig,
    pending: Arc<Mutex<BTreeMap<String, PendingGraphState>>>,
    presealed: Arc<Mutex<BTreeMap<(String, u32), PresealedGraphAdmission>>>,
    admitted: Arc<Mutex<BTreeMap<(String, u32), GraphAdmission>>>,
    wake: Arc<tokio::sync::Notify>,
}

struct PreparedGraphAdmission {
    node: aw_tools_adapter::operation_router::GraphPolicyNode,
    target: String,
    evidence: AuthenticatedToolGrantEvidence,
    authorized: aw_policy::AuthorizedPolicy,
    fact: aw_store::PolicyAdmissionFact,
}

impl PolicyGraphRouter {
    pub fn new(
        service: Service,
        authority: aw_agent_core::tool::ToolBindingAuthority,
        config: PolicyGraphConfig,
    ) -> Result<Self> {
        PolicyGraphConfig::deterministic(config.policy, config.revision)?;
        Ok(Self {
            service,
            authority,
            config,
            pending: Arc::new(Mutex::new(BTreeMap::new())),
            presealed: Arc::new(Mutex::new(BTreeMap::new())),
            admitted: Arc::new(Mutex::new(BTreeMap::new())),
            wake: Arc::new(tokio::sync::Notify::new()),
        })
    }

    fn graph_ref(ctx: &ToolContext, job_id: &str) -> Result<ResolvedExecutionRef> {
        let current = PolicyOperationRouter::current_agent_ref(ctx)?;
        ResolvedExecutionRef::new(
            current.agent_ref().clone(),
            vec![
                ExecutionSegment::Turn(
                    Handle::new(ctx.turn_id.to_string())
                        .map_err(|_| Error::Denied("invalid Graph Turn handle"))?,
                ),
                ExecutionSegment::Job(
                    Handle::new(job_id.to_owned())
                        .map_err(|_| Error::Denied("invalid Graph Job handle"))?,
                ),
            ],
        )
        .map_err(|_| Error::Denied("cannot construct canonical Graph ref"))
    }

    fn node_ref(graph: &ResolvedExecutionRef, coordinate: u32) -> Result<ResolvedExecutionRef> {
        let mut suffix = graph.suffix().to_vec();
        suffix.push(ExecutionSegment::Node(
            NodeCoordinate::new(coordinate)
                .map_err(|_| Error::Denied("invalid Graph node coordinate"))?,
        ));
        ResolvedExecutionRef::new(graph.agent_ref().clone(), suffix)
            .map_err(|_| Error::Denied("cannot construct canonical Graph node ref"))
    }

    fn policy_sha256(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(b"aw-graph-policy-config-v1\0");
        hash.update(self.config.revision.to_be_bytes());
        let policy = self.config.policy;
        hash.update([
            u8::from(policy.deny),
            u8::from(policy.routes.contains(aw_policy::Strategy::Local)),
            u8::from(policy.routes.contains(aw_policy::Strategy::DelegateAgent)),
            match policy.prefer {
                None => 0,
                Some(aw_policy::Strategy::Local) => 1,
                Some(aw_policy::Strategy::DelegateAgent) => 2,
            },
            u8::from(policy.require_preferred),
            u8::from(self.config.guardian),
        ]);
        hash.update(policy.max_model_rounds.unwrap_or(0).to_be_bytes());
        format!("{:x}", hash.finalize())
    }

    fn request_sha256_parts(
        graph_fingerprint: [u8; 32],
        coordinate: u32,
        operation: OperationId,
        replay_identity: [u8; 32],
        footprint_fingerprint: [u8; 32],
        dependencies: &[u32],
    ) -> String {
        let mut hash = Sha256::new();
        hash.update(b"aw-graph-policy-node-request-v1\0");
        hash.update(graph_fingerprint);
        hash.update(coordinate.to_be_bytes());
        hash.update(operation.semantic_name().as_bytes());
        hash.update(replay_identity);
        hash.update(footprint_fingerprint);
        for dependency in dependencies {
            hash.update(dependency.to_be_bytes());
        }
        format!("{:x}", hash.finalize())
    }

    fn request_sha256(
        request: &GraphPolicyRequest,
        node: &aw_tools_adapter::operation_router::GraphPolicyNode,
    ) -> String {
        Self::request_sha256_parts(
            request.graph_fingerprint,
            node.coordinate,
            node.operation,
            node.replay_identity,
            node.footprint_fingerprint,
            &node.dependencies,
        )
    }

    fn preseal_request_sha256(request: &GraphPresealRequest, node: &GraphPresealNode) -> String {
        Self::request_sha256_parts(
            request.graph_fingerprint,
            node.coordinate,
            node.operation,
            node.replay_identity,
            node.footprint.fingerprint(),
            &node.dependencies,
        )
    }

    fn validate_narrow_proposal(
        node: &GraphPresealNode,
        proposal: GraphNarrowProposal,
    ) -> Result<GraphNarrowProposal> {
        if proposal.narrow_ref.is_empty()
            || proposal.narrow_ref.len() > 256
            || !proposal.narrow_ref.is_ascii()
            || !proposal
                .narrow_ref
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._~:/@-".contains(&byte))
            || !proposal.arguments.is_object()
            || serde_json::to_vec(&proposal.arguments)
                .map_err(|_| Error::Denied("cannot encode Graph narrow arguments"))?
                .len()
                > aw_agent_core::tool::MAX_TOOL_ARGUMENT_BYTES
            || proposal.arguments == node.arguments
        {
            return Err(Error::Denied(
                "invalid or no-op host Graph argument narrowing proposal",
            ));
        }
        Ok(proposal)
    }

    fn apply_narrowed_arguments(
        graph: &mut Value,
        coordinate: u32,
        arguments: Value,
    ) -> Result<()> {
        let ops = graph
            .get_mut("ops")
            .and_then(Value::as_array_mut)
            .ok_or(Error::Protocol("pre-seal Graph lost ops array"))?;
        let index = usize::try_from(coordinate.saturating_sub(1))
            .map_err(|_| Error::Protocol("Graph narrow coordinate overflow"))?;
        let node = ops
            .get_mut(index)
            .and_then(Value::as_array_mut)
            .ok_or(Error::Protocol("Graph narrow coordinate is unavailable"))?;
        if node.len() < 2 {
            return Err(Error::Protocol("Graph narrow node is malformed"));
        }
        node[1] = arguments;
        Ok(())
    }

    fn preseal_policy_node(node: &GraphPresealNode) -> aw_tools_adapter::operation_router::GraphPolicyNode {
        aw_tools_adapter::operation_router::GraphPolicyNode {
            coordinate: node.coordinate,
            operation: node.operation,
            replay_identity: node.replay_identity,
            footprint_fingerprint: node.footprint.fingerprint(),
            dependencies: node.dependencies.clone(),
        }
    }

    fn approval_id(admission_id: &str) -> ApprovalId {
        let mut hash = Sha256::new();
        hash.update(b"aw-graph-policy-approval-v1\0");
        hash.update(admission_id.as_bytes());
        ApprovalId(hash.finalize().into())
    }

    fn fingerprint_strings(domain: &[String], label: &[u8]) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(label);
        let mut values = domain.to_vec();
        values.sort();
        for value in values {
            hash.update((value.len() as u64).to_be_bytes());
            hash.update(value.as_bytes());
        }
        hash.finalize().into()
    }

    fn graph_admission_identity(
        ctx: &ToolContext,
        request: &GraphPolicyRequest,
    ) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"aw-graph-policy-admission-set-v1\0");
        hash.update(request.graph_fingerprint);
        hash.update(ctx.turn_id.to_be_bytes());
        hash.update(ctx.call_id.as_bytes());
        hash.finalize().into()
    }

    pub fn pending_approvals(&self, job_id: &str) -> Option<PendingApprovalSet> {
        self.pending
            .lock()
            .ok()
            .and_then(|pending| pending.get(job_id).map(|state| state.set.clone()))
    }

    pub fn decide_graph_approval(
        &self,
        job_id: &str,
        coordinate: u32,
        approver: &PrincipalId,
        approve: bool,
    ) -> Result<aw_store::PolicyAdmissionFact> {
        let (session, admission_id, approval_id) = {
            let pending = self
                .pending
                .lock()
                .map_err(|_| Error::Storage("Graph approval set lock poisoned"))?;
            let state = pending
                .get(job_id)
                .ok_or(Error::Denied("Graph has no pending approval set"))?;
            let admission_id = state
                .by_coordinate
                .get(&coordinate)
                .cloned()
                .ok_or(Error::Denied("Graph node is not awaiting approval"))?;
            (
                state.session_id.clone(),
                admission_id.clone(),
                Self::approval_id(&admission_id),
            )
        };
        let fact = self.service.inner.store.policy_admission_decide(
            approver,
            &session,
            &admission_id,
            approve,
        )?;
        if let Ok(mut pending) = self.pending.lock() {
            if let Some(state) = pending.get_mut(job_id) {
                if let Some(entry) = state.set.entry_mut(approval_id) {
                    let projected = match fact.state {
                        PolicyAdmissionState::AskApproved => ApprovalState::Approved,
                        PolicyAdmissionState::AskDenied => ApprovalState::Denied,
                        PolicyAdmissionState::AskExpired => ApprovalState::Expired,
                        _ => ApprovalState::Pending,
                    };
                    if projected != ApprovalState::Pending {
                        let _ = entry.resolve(projected);
                    }
                }
            }
        }
        self.wake.notify_waiters();
        Ok(fact)
    }

    async fn guardian_decide(
        &self,
        ctx: &ToolContext,
        prepared: &PreparedGraphAdmission,
        approval_scope: Option<&str>,
        admission_narrow_ref: Option<&str>,
    ) -> Result<aw_store::PolicyAdmissionFact> {
        if !self.config.guardian {
            let effective = prepared.authorized.effective();
            let route = effective.strategy();
            let digest =
                PolicyOperationRouter::decision_digest(self.config.revision, route, "allow");
            return self.service.inner.store.policy_admission_record_decision(
                &ctx.actor,
                &prepared.fact.request.session_id,
                &prepared.fact.id,
                aw_policy::PolicyDecision {
                    decision: aw_policy::Decision::Allow,
                    revision: self.config.revision,
                    route: Some(route),
                    approval_scope: None,
                    narrow_ref: None,
                },
                &digest,
            );
        }
        let selected = self.service.exact_selection(ctx)?;
        let scratch = Agent::builder_from_selection(
            selected.clone(),
            AgentOptions::new(
                selected.identity().account_id.clone(),
                selected.identity().model.clone(),
            ),
        )
        .build()?;
        let ask_instruction = approval_scope.map_or_else(
            || "Ask is unavailable for this node.".to_owned(),
            |scope| format!(
                "If human approval is required, return decision=ask and approval_scope exactly {scope:?}."
            ),
        );
        let narrow_instruction = admission_narrow_ref.map_or_else(
            || "Argument Narrow is unavailable at this stage; do not return decision=narrow.".to_owned(),
            |reference| format!(
                "Admission-time argument Narrow is available only by returning decision=narrow and narrow_ref exactly {reference:?}. The replacement arguments are trusted-host authored and are not disclosed to you."
            ),
        );
        let prompt = format!(
            "Return strict JSON only: decision allow|deny|ask|narrow, optional route local|delegate_agent.              Graph node {} operation {}. Effect {:?}. Site {}. Target {}. Dependencies {:?}.              Arguments and private results are intentionally undisclosed. {} {}",
            prepared.node.coordinate,
            prepared.node.operation.semantic_name(),
            ctx.effect,
            ctx.execution_site,
            prepared.target,
            prepared.node.dependencies,
            ask_instruction,
            narrow_instruction,
        );
        let input = AuxiliaryInput {
            kind: AuxiliaryKind::Side,
            messages: scratch.auxiliary_messages(&selected, &prompt).await?,
            max_output_bytes: 8 * 1024,
            timeout: self
                .service
                .inner
                .settings
                .request_timeout
                .min(Duration::from_secs(60)),
        };
        aw::policy_runtime::run_admission_guardian(
            &self.service.inner.store,
            &ctx.actor,
            prepared.fact.request.clone(),
            prepared.authorized,
            &selected,
            &input,
            self.config.revision,
            ctx.cancellation.clone().cancelled(),
        )
        .await
    }

    fn route_for_fact(
        fact: &aw_store::PolicyAdmissionFact,
        fallback: aw_policy::Strategy,
    ) -> Result<aw_policy::Strategy> {
        PolicyOperationRouter::selected_route(fact, fallback)
    }

    async fn wait_for_approvals(
        &self,
        ctx: &ToolContext,
        job_id: &str,
    ) -> Result<()> {
        loop {
            let snapshot = {
                let pending = self
                    .pending
                    .lock()
                    .map_err(|_| Error::Storage("Graph approval set lock poisoned"))?;
                pending
                    .get(job_id)
                    .cloned()
                    .ok_or(Error::Conflict("Graph approval set disappeared"))?
            };
            let mut ready = true;
            let mut failed = false;
            for (coordinate, admission_id) in &snapshot.by_coordinate {
                let fact = self.service.inner.store.policy_admission_fact(
                    &ctx.actor,
                    &snapshot.session_id,
                    admission_id,
                )?;
                let state = match fact.state {
                    PolicyAdmissionState::AskApproved => ApprovalState::Approved,
                    PolicyAdmissionState::AskDenied => ApprovalState::Denied,
                    PolicyAdmissionState::AskExpired => ApprovalState::Expired,
                    PolicyAdmissionState::AskPending => {
                        ready = false;
                        ApprovalState::Pending
                    }
                    _ => {
                        return Err(Error::Conflict(
                            "Graph Ask admission left its approval lifecycle",
                        ))
                    }
                };
                if matches!(state, ApprovalState::Denied | ApprovalState::Expired) {
                    failed = true;
                }
                if state != ApprovalState::Pending {
                    if let Ok(mut pending) = self.pending.lock() {
                        if let Some(graph) = pending.get_mut(job_id) {
                            if let Some(entry) =
                                graph.set.entry_mut(Self::approval_id(admission_id))
                            {
                                let _ = entry.resolve(state);
                            }
                        }
                    }
                }
                let _ = coordinate;
            }
            if failed {
                self.pending.lock().ok().map(|mut pending| pending.remove(job_id));
                return Err(Error::Denied("Graph approval denied or expired"));
            }
            if ready {
                self.pending
                    .lock()
                    .map_err(|_| Error::Storage("Graph approval set lock poisoned"))?
                    .remove(job_id);
                return Ok(());
            }
            tokio::select! {
                _ = ctx.cancellation.cancelled() => return Err(Error::Cancelled),
                _ = self.wake.notified() => {},
                _ = tokio::time::sleep(Duration::from_millis(35)) => {},
            }
        }
    }
}

impl GraphPolicyRouter for PolicyGraphRouter {
    fn before_seal<'a>(
        &'a self,
        ctx: &'a ToolContext,
        request: GraphPresealRequest,
    ) -> GraphPresealFuture<'a> {
        Box::pin(async move {
            if request.nodes.is_empty() || request.nodes.len() > 64 {
                return Err(Error::Limit("Graph pre-seal policy node set must be 1..64"));
            }
            for pair in request.nodes.windows(2) {
                if pair[0].coordinate >= pair[1].coordinate {
                    return Err(Error::Denied(
                        "Graph pre-seal node coordinates must be canonical and sorted",
                    ));
                }
            }
            let scope = ctx.scope.as_ref().ok_or(Error::Denied(
                "durable Graph pre-seal policy requires a Session/branch scope",
            ))?;
            let owner = ctx.execution_owner.as_ref().ok_or(Error::Denied(
                "durable Graph pre-seal policy requires a bound Agent owner",
            ))?;
            if owner.session_id() != scope.session_id || owner.branch_id() != scope.branch_id {
                return Err(Error::Denied(
                    "Graph pre-seal policy scope differs from bound Agent owner",
                ));
            }

            let graph_ref = Self::graph_ref(ctx, &request.job_id)?;
            let current = PolicyOperationRouter::current_agent_ref(ctx)?;
            let project_scope = PolicyOperationRouter::project_scope(&current);
            let target_domain = PolicyOperationRouter::target_domain(&current);
            let session = self
                .service
                .inner
                .store
                .session(&ctx.actor, &scope.session_id)?;
            let writer = self.service.inner.store.workspace_writer(
                &ctx.actor,
                None,
                &session.workspace_id,
            )?;
            if writer.state != "active" {
                return Err(Error::Denied("Graph pre-seal policy writer is not active"));
            }

            let policy_sha = self.policy_sha256();
            let selected = if self.config.guardian {
                Some(self.service.exact_selection(ctx)?)
            } else {
                None
            };
            let approvers = self
                .config
                .authorized_approvers
                .iter()
                .map(|principal| principal.as_str().to_owned())
                .collect::<Vec<_>>();
            let expiry = (!approvers.is_empty()).then(|| {
                unix_micros().saturating_add(
                    self.config
                        .approval_ttl
                        .as_micros()
                        .min(i64::MAX as u128) as i64,
                )
            });

            // Claim every original node before any guardian/provider work. The
            // request hash uses the original draft fingerprint/replay identity,
            // never the later narrowed Graph.
            let mut prepared = Vec::with_capacity(request.nodes.len());
            let mut proposals = BTreeMap::<u32, GraphNarrowProposal>::new();
            let mut footprint_overrides = Vec::new();
            for node in &request.nodes {
                let target = Self::node_ref(&graph_ref, node.coordinate)?.canonical();
                let evidence = AuthenticatedToolGrantEvidence::capture(
                    &self.authority,
                    &ctx.actor,
                    node.operation.model_name(),
                    node.operation.semantic_name(),
                    target_domain.clone(),
                    target.clone(),
                    project_scope.clone(),
                )?;
                let operation = evidence.operation_context();
                let invocation = format!(
                    "turn:{}/call:{}/graph:{}/node:{}",
                    ctx.turn_id, ctx.call_id, request.job_id, node.coordinate
                );
                let graph_name = graph_ref.canonical();
                let policy_context = aw_policy::PolicyContext {
                    actor: ctx.actor.as_str(),
                    domain: &target_domain,
                    active_team: None,
                    workspace: &project_scope,
                    session: Some(owner.canonical_ref()),
                    graph: Some(&graph_name),
                    node: Some(node.coordinate),
                    invocation: &invocation,
                    operation,
                };
                let layer = aw_policy::PolicyLayer {
                    scope: aw_policy::PolicyScope::Node,
                    revision: self.config.revision,
                    policy: &self.config.policy,
                };
                let grant = evidence.as_policy_path();
                let authorized =
                    aw_policy::resolve_authorized(&policy_context, &[grant], &[layer])
                        .map_err(|_| {
                            Error::Denied("Graph pre-seal node policy denied exact authority")
                        })?;
                let effective = authorized.effective();
                if effective
                    .routes()
                    .contains(aw_policy::Strategy::DelegateAgent)
                {
                    footprint_overrides.push(GraphFootprintOverride {
                        coordinate: node.coordinate,
                        footprint: aw_tools::filesystem::footprint::Footprint::unknown(true, true),
                    });
                }

                let proposal = if self.config.guardian {
                    match self.config.narrow_authority.as_ref() {
                        Some(authority) => authority
                            .proposal(ctx, node)?
                            .map(|proposal| Self::validate_narrow_proposal(node, proposal))
                            .transpose()?,
                        None => None,
                    }
                } else {
                    None
                };
                if let Some(proposal) = proposal {
                    proposals.insert(node.coordinate, proposal);
                }

                let fact = self.service.inner.store.policy_admission_claim(
                    &ctx.actor,
                    PolicyAdmissionRequest {
                        session_id: scope.session_id.clone(),
                        branch_id: scope.branch_id.clone(),
                        writer_epoch: writer.writer_epoch,
                        turn_id: ctx.turn_id,
                        call_id: ctx.call_id.clone(),
                        canonical_target: target.clone(),
                        request_sha256: Self::preseal_request_sha256(&request, node),
                        policy_revision_sha256: policy_sha.clone(),
                        authority_sha256: evidence.authority_sha256(),
                        candidate_local: effective.routes().contains(aw_policy::Strategy::Local),
                        candidate_delegate_agent: effective
                            .routes()
                            .contains(aw_policy::Strategy::DelegateAgent),
                        guardian_required: self.config.guardian,
                        guardian_identity: selected
                            .as_ref()
                            .map(|selected| selected.identity().clone()),
                        guardian_site: selected
                            .as_ref()
                            .map(|selected| selected.identity().inference_executor.clone()),
                        approval_scope: (!approvers.is_empty()).then(|| target.clone()),
                        authorized_approvers: approvers.clone(),
                        approval_expires_at: expiry,
                    },
                )?;
                prepared.push(PreparedGraphAdmission {
                    node: Self::preseal_policy_node(node),
                    target,
                    evidence,
                    authorized,
                    fact,
                });
            }

            let mut revised_graph = request.original_graph.clone();
            let mut staged = Vec::with_capacity(prepared.len());
            for prepared_node in prepared {
                let approval_scope = prepared_node.fact.request.approval_scope.as_deref();
                let narrow_ref = proposals
                    .get(&prepared_node.node.coordinate)
                    .map(|proposal| proposal.narrow_ref.as_str());
                let mut fact = match prepared_node.fact.state {
                    PolicyAdmissionState::Claimed => {
                        self.guardian_decide(
                            ctx,
                            &prepared_node,
                            approval_scope,
                            narrow_ref,
                        )
                        .await?
                    }
                    _ => prepared_node.fact.clone(),
                };

                match fact.state {
                    PolicyAdmissionState::Narrowed => {
                        let proposal = proposals
                            .get(&prepared_node.node.coordinate)
                            .ok_or(Error::Denied(
                                "guardian requested Graph Narrow without a trusted host revision",
                            ))?;
                        if fact.narrow_ref.as_deref() != Some(proposal.narrow_ref.as_str()) {
                            return Err(Error::Denied(
                                "guardian Graph Narrow reference differs from trusted host revision",
                            ));
                        }
                        Self::apply_narrowed_arguments(
                            &mut revised_graph,
                            prepared_node.node.coordinate,
                            proposal.arguments.clone(),
                        )?;
                        prepared_node.evidence.revalidate(&self.authority)?;
                        let route =
                            Self::route_for_fact(&fact, prepared_node.authorized.effective().strategy())?;
                        fact = self.service.inner.store.policy_admission_consume(
                            &ctx.actor,
                            &scope.session_id,
                            &fact.id,
                            &policy_sha,
                            &prepared_node.evidence.authority_sha256(),
                            Some(route),
                            Some(&proposal.narrow_ref),
                        )?;
                        if fact.state != PolicyAdmissionState::Consumed {
                            return Err(Error::Conflict(
                                "pre-seal Graph Narrow was not durably consumed",
                            ));
                        }
                    }
                    PolicyAdmissionState::Allowed
                    | PolicyAdmissionState::AskPending
                    | PolicyAdmissionState::AskApproved => {}
                    PolicyAdmissionState::Denied
                    | PolicyAdmissionState::AskDenied
                    | PolicyAdmissionState::AskExpired => {
                        return Err(Error::Denied(
                            "Graph node denied during pre-seal policy admission",
                        ))
                    }
                    PolicyAdmissionState::GuardianStarted
                    | PolicyAdmissionState::GuardianUnknown
                    | PolicyAdmissionState::GuardianSettled => {
                        return Err(Error::Conflict(
                            "Graph pre-seal guardian outcome is unresolved; do not replay",
                        ))
                    }
                    PolicyAdmissionState::Consumed => {
                        return Err(Error::Conflict(
                            "Graph pre-seal policy admission was already consumed",
                        ))
                    }
                    PolicyAdmissionState::Claimed => {
                        return Err(Error::Conflict(
                            "Graph pre-seal policy admission remained undecided",
                        ))
                    }
                }

                staged.push((
                    (request.job_id.clone(), prepared_node.node.coordinate),
                    PresealedGraphAdmission {
                        target: prepared_node.target,
                        evidence: prepared_node.evidence,
                        authorized: prepared_node.authorized,
                        fact,
                        original_replay_identity: prepared_node.node.replay_identity,
                    },
                ));
            }

            let mut live = self
                .presealed
                .lock()
                .map_err(|_| Error::Storage("Graph pre-seal admission registry lock poisoned"))?;
            if staged.iter().any(|(key, _)| live.contains_key(key)) {
                return Err(Error::Conflict(
                    "Graph pre-seal admission already exists for this Job",
                ));
            }
            live.extend(staged);
            Ok(GraphPresealDecision {
                graph: revised_graph,
                footprint_overrides,
            })
        })
    }

    fn abandon_preseal(&self, job_id: &str) -> Result<()> {
        self.presealed
            .lock()
            .map_err(|_| Error::Storage("Graph pre-seal admission registry lock poisoned"))?
            .retain(|(job, _), _| job != job_id);
        Ok(())
    }

    fn before_run<'a>(
        &'a self,
        ctx: &'a ToolContext,
        request: GraphPolicyRequest,
    ) -> GraphPolicyFuture<'a> {
        Box::pin(async move {
            if request.nodes.is_empty() || request.nodes.len() > 64 {
                return Err(Error::Limit("Graph policy node set must be 1..64"));
            }
            for pair in request.nodes.windows(2) {
                if pair[0].coordinate >= pair[1].coordinate {
                    return Err(Error::Denied(
                        "Graph policy node coordinates must be canonical and sorted",
                    ));
                }
            }
            let scope = ctx.scope.as_ref().ok_or(Error::Denied(
                "durable Graph policy requires a Session/branch scope",
            ))?;
            let owner = ctx.execution_owner.as_ref().ok_or(Error::Denied(
                "durable Graph policy requires a bound Agent owner",
            ))?;
            if owner.session_id() != scope.session_id || owner.branch_id() != scope.branch_id {
                return Err(Error::Denied(
                    "Graph policy scope differs from bound Agent owner",
                ));
            }
            let graph_ref = Self::graph_ref(ctx, &request.job_id)?;
            let current = PolicyOperationRouter::current_agent_ref(ctx)?;
            let project_scope = PolicyOperationRouter::project_scope(&current);
            let target_domain = PolicyOperationRouter::target_domain(&current);
            let session = self
                .service
                .inner
                .store
                .session(&ctx.actor, &scope.session_id)?;
            let writer = self.service.inner.store.workspace_writer(
                &ctx.actor,
                None,
                &session.workspace_id,
            )?;
            if writer.state != "active" {
                return Err(Error::Denied("Graph policy writer is not active"));
            }
            let policy_sha = self.policy_sha256();
            let selected = if self.config.guardian {
                Some(self.service.exact_selection(ctx)?)
            } else {
                None
            };
            let approvers = self
                .config
                .authorized_approvers
                .iter()
                .map(|principal| principal.as_str().to_owned())
                .collect::<Vec<_>>();
            let expiry = (!approvers.is_empty()).then(|| {
                unix_micros().saturating_add(
                    self.config
                        .approval_ttl
                        .as_micros()
                        .min(i64::MAX as u128) as i64,
                )
            });

            // Batch pre-seal admission stages the original claims before the
            // Graph is sealed. Direct/internal start_graph callers may still
            // reach this path without that stage; preserve the existing
            // claim-before-guardian behavior as a fallback.
            let presealed = {
                let mut live = self
                    .presealed
                    .lock()
                    .map_err(|_| Error::Storage("Graph pre-seal admission registry lock poisoned"))?;
                let count = live
                    .keys()
                    .filter(|(job, _)| job == &request.job_id)
                    .count();
                if count == 0 {
                    None
                } else {
                    if count != request.nodes.len() {
                        live.retain(|(job, _), _| job != &request.job_id);
                        return Err(Error::Conflict(
                            "Graph pre-seal admission set is incomplete",
                        ));
                    }
                    let mut staged = Vec::with_capacity(request.nodes.len());
                    for node in &request.nodes {
                        let entry = live
                            .remove(&(request.job_id.clone(), node.coordinate))
                            .ok_or(Error::Conflict(
                                "Graph pre-seal admission coordinate is missing",
                            ))?;
                        staged.push((node.clone(), entry));
                    }
                    Some(staged)
                }
            };

            let mut prepared = Vec::with_capacity(request.nodes.len());
            if let Some(staged) = presealed {
                for (node, entry) in staged {
                    let target = Self::node_ref(&graph_ref, node.coordinate)?.canonical();
                    if target != entry.target
                        || entry.original_replay_identity == [0; 32]
                        || entry.evidence.operation_context().operation
                            != node.operation.semantic_name()
                        || entry.evidence.binding().name != node.operation.model_name()
                    {
                        return Err(Error::Denied(
                            "sealed Graph differs from its pre-seal policy identity",
                        ));
                    }
                    entry.evidence.revalidate(&self.authority)?;
                    let fact = self.service.inner.store.policy_admission_fact(
                        &ctx.actor,
                        &scope.session_id,
                        &entry.fact.id,
                    )?;
                    if fact.request.policy_revision_sha256 != policy_sha
                        || fact.request.authority_sha256 != entry.evidence.authority_sha256()
                    {
                        return Err(Error::Conflict(
                            "pre-seal Graph policy/authority generation changed before seal",
                        ));
                    }
                    prepared.push(PreparedGraphAdmission {
                        node,
                        target,
                        evidence: entry.evidence,
                        authorized: entry.authorized,
                        fact,
                    });
                }
            } else {
                // Fallback used by direct/internal Graph starts that did not
                // traverse Batch's async pre-seal admission stage.
                for node in &request.nodes {
                    let target = Self::node_ref(&graph_ref, node.coordinate)?.canonical();
                    let evidence = AuthenticatedToolGrantEvidence::capture(
                        &self.authority,
                        &ctx.actor,
                        node.operation.model_name(),
                        node.operation.semantic_name(),
                        target_domain.clone(),
                        target.clone(),
                        project_scope.clone(),
                    )?;
                    let operation = evidence.operation_context();
                    let invocation = format!(
                        "turn:{}/call:{}/graph:{}/node:{}",
                        ctx.turn_id, ctx.call_id, request.job_id, node.coordinate
                    );
                    let graph_name = graph_ref.canonical();
                    let policy_context = aw_policy::PolicyContext {
                        actor: ctx.actor.as_str(),
                        domain: &target_domain,
                        active_team: None,
                        workspace: &project_scope,
                        session: Some(owner.canonical_ref()),
                        graph: Some(&graph_name),
                        node: Some(node.coordinate),
                        invocation: &invocation,
                        operation,
                    };
                    let layer = aw_policy::PolicyLayer {
                        scope: aw_policy::PolicyScope::Node,
                        revision: self.config.revision,
                        policy: &self.config.policy,
                    };
                    let grant = evidence.as_policy_path();
                    let authorized = aw_policy::resolve_authorized(
                        &policy_context,
                        &[grant],
                        &[layer],
                    )
                    .map_err(|_| Error::Denied("Graph node policy denied exact authority"))?;
                    let effective = authorized.effective();
                    let fact = self.service.inner.store.policy_admission_claim(
                        &ctx.actor,
                        PolicyAdmissionRequest {
                            session_id: scope.session_id.clone(),
                            branch_id: scope.branch_id.clone(),
                            writer_epoch: writer.writer_epoch,
                            turn_id: ctx.turn_id,
                            call_id: ctx.call_id.clone(),
                            canonical_target: target.clone(),
                            request_sha256: Self::request_sha256(&request, node),
                            policy_revision_sha256: policy_sha.clone(),
                            authority_sha256: evidence.authority_sha256(),
                            candidate_local: effective.routes().contains(aw_policy::Strategy::Local),
                            candidate_delegate_agent: effective
                                .routes()
                                .contains(aw_policy::Strategy::DelegateAgent),
                            guardian_required: self.config.guardian,
                            guardian_identity: selected
                                .as_ref()
                                .map(|selected| selected.identity().clone()),
                            guardian_site: selected
                                .as_ref()
                                .map(|selected| selected.identity().inference_executor.clone()),
                            approval_scope: (!approvers.is_empty()).then(|| target.clone()),
                            authorized_approvers: approvers.clone(),
                            approval_expires_at: expiry,
                        },
                    )?;
                    prepared.push(PreparedGraphAdmission {
                        node: node.clone(),
                        target,
                        evidence,
                        authorized,
                        fact,
                    });
                }
            }

            let mut asks = Vec::new();
            let mut executable = Vec::new();
            for prepared_node in &prepared {
                let approval_scope = prepared_node.fact.request.approval_scope.as_deref();
                let fact = match prepared_node.fact.state {
                    PolicyAdmissionState::Claimed => {
                        self.guardian_decide(ctx, prepared_node, approval_scope, None)
                            .await?
                    }
                    _ => prepared_node.fact.clone(),
                };
                match fact.state {
                    PolicyAdmissionState::Allowed | PolicyAdmissionState::AskApproved => {
                        executable.push((prepared_node, fact));
                    }
                    PolicyAdmissionState::AskPending => asks.push((prepared_node, fact)),
                    PolicyAdmissionState::Denied
                    | PolicyAdmissionState::AskDenied
                    | PolicyAdmissionState::AskExpired => {
                        return Err(Error::Denied("Graph node denied by admitted policy"))
                    }
                    PolicyAdmissionState::Narrowed => {
                        return Err(Error::Denied(
                            "post-seal Graph node policy cannot mutate prepared arguments",
                        ))
                    }
                    PolicyAdmissionState::GuardianStarted
                    | PolicyAdmissionState::GuardianUnknown
                    | PolicyAdmissionState::GuardianSettled => {
                        return Err(Error::Conflict(
                            "Graph guardian outcome is unresolved; do not replay",
                        ))
                    }
                    PolicyAdmissionState::Consumed
                        if fact.narrow_ref.is_some() =>
                    {
                        executable.push((prepared_node, fact));
                    }
                    PolicyAdmissionState::Consumed => {
                        return Err(Error::Conflict(
                            "Graph policy admission already consumed by another runner",
                        ))
                    }
                    PolicyAdmissionState::Claimed => {
                        return Err(Error::Conflict(
                            "Graph policy admission remained undecided",
                        ))
                    }
                }
            }

            if !asks.is_empty() {
                let admission_identity = Self::graph_admission_identity(ctx, &request);
                let decision_revision = Revision::new(self.config.revision)
                    .map_err(|_| Error::Denied("invalid Graph policy decision revision"))?;
                let approver_names = approvers.clone();
                let approver_fingerprint = Self::fingerprint_strings(
                    &approver_names,
                    b"aw-graph-policy-approvers-v1\0",
                );
                let entries = asks
                    .iter()
                    .map(|(prepared_node, fact)| {
                        let scope = fact
                            .request
                            .approval_scope
                            .as_ref()
                            .ok_or(Error::Denied("Graph Ask lacks approval scope"))?;
                        let approval_scope = Self::fingerprint_strings(
                            &[scope.clone()],
                            b"aw-graph-policy-approval-scope-v1\0",
                        );
                        Ok(PendingApproval {
                            approval_id: Self::approval_id(&fact.id),
                            target: ApprovalTarget::Node(
                                NodeCoordinate::new(prepared_node.node.coordinate)
                                    .map_err(|_| Error::Denied("invalid approval node"))?,
                            ),
                            policy_decision_revision: Revision::new(
                                fact.decision_revision.unwrap_or(self.config.revision),
                            )
                            .map_err(|_| Error::Denied("invalid approval decision revision"))?,
                            approval_scope,
                            authorized_approvers: approver_fingerprint,
                            expiry_unix_ms: fact
                                .request
                                .approval_expires_at
                                .unwrap_or_default()
                                .max(0) as u64
                                / 1_000,
                            state: ApprovalState::Pending,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let set = PendingApprovalSet::new(
                    graph_ref.clone(),
                    admission_identity,
                    decision_revision,
                    entries,
                    64,
                )
                .map_err(|_| Error::Denied("invalid Graph approval set"))?;
                let by_coordinate = asks
                    .iter()
                    .map(|(prepared_node, fact)| (prepared_node.node.coordinate, fact.id.clone()))
                    .collect::<BTreeMap<_, _>>();
                self.pending
                    .lock()
                    .map_err(|_| Error::Storage("Graph approval set lock poisoned"))?
                    .insert(
                        request.job_id.clone(),
                        PendingGraphState {
                            session_id: scope.session_id.clone(),
                            set,
                            by_coordinate,
                        },
                    );
                self.wait_for_approvals(ctx, &request.job_id).await?;
                executable.clear();
                for prepared_node in &prepared {
                    let fact = self.service.inner.store.policy_admission_fact(
                        &ctx.actor,
                        &scope.session_id,
                        &prepared_node.fact.id,
                    )?;
                    if !matches!(
                        fact.state,
                        PolicyAdmissionState::Allowed
                            | PolicyAdmissionState::AskApproved
                            | PolicyAdmissionState::Consumed
                    ) || (fact.state == PolicyAdmissionState::Consumed
                        && fact.narrow_ref.is_none())
                    {
                        return Err(Error::Denied(
                            "Graph approvals did not produce an executable complete set",
                        ));
                    }
                    executable.push((prepared_node, fact));
                }
            }

            let mut admitted = BTreeMap::new();
            for (prepared_node, fact) in executable {
                prepared_node.evidence.revalidate(&self.authority)?;
                let effective = prepared_node.authorized.effective();
                let route = Self::route_for_fact(&fact, effective.strategy())?;
                let consumed = if fact.state == PolicyAdmissionState::Consumed {
                    fact.clone()
                } else {
                    self.service.inner.store.policy_admission_consume(
                        &ctx.actor,
                        &scope.session_id,
                        &fact.id,
                        &policy_sha,
                        &prepared_node.evidence.authority_sha256(),
                        Some(route),
                        None,
                    )?
                };
                if consumed.state != PolicyAdmissionState::Consumed {
                    return Err(Error::Conflict(
                        "Graph node policy admission was not consumed",
                    ));
                }
                admitted.insert(
                    (request.job_id.clone(), prepared_node.node.coordinate),
                    GraphAdmission {
                        admission_id: fact.id.clone(),
                        session_id: scope.session_id.clone(),
                        policy_sha256: policy_sha.clone(),
                        authority_sha256: prepared_node.evidence.authority_sha256(),
                        evidence: prepared_node.evidence.clone(),
                        route,
                        replay_identity: prepared_node.node.replay_identity,
                    },
                );
            }
            self.admitted
                .lock()
                .map_err(|_| Error::Storage("Graph admission registry lock poisoned"))?
                .extend(admitted);
            Ok(())
        })
    }

    fn before_node<'a>(
        &'a self,
        ctx: &'a ToolContext,
        request: GraphNodePolicyRequest,
    ) -> GraphPolicyFuture<'a> {
        Box::pin(async move {
            let admission = self
                .admitted
                .lock()
                .map_err(|_| Error::Storage("Graph admission registry lock poisoned"))?
                .get(&(request.job_id.clone(), request.coordinate))
                .cloned()
                .ok_or(Error::Denied("Graph node lacks admitted policy evidence"))?;
            if admission.replay_identity != request.replay_identity {
                return Err(Error::Denied(
                    "Graph node replay identity changed after policy admission",
                ));
            }
            admission.evidence.revalidate(&self.authority)?;
            if admission.authority_sha256 != admission.evidence.authority_sha256()
                || admission.policy_sha256 != self.policy_sha256()
            {
                return Err(Error::Denied(
                    "Graph node policy/authority/route changed before activation",
                ));
            }
            let fact = self.service.inner.store.policy_admission_fact(
                &ctx.actor,
                &admission.session_id,
                &admission.admission_id,
            )?;
            if fact.state != PolicyAdmissionState::Consumed {
                return Err(Error::Conflict(
                    "Graph node policy admission is no longer consumed",
                ));
            }
            Ok(())
        })
    }

    fn execute_node<'a>(
        &'a self,
        ctx: &'a ToolContext,
        request: GraphNodePolicyRequest,
        original: Arc<dyn GraphNodeOriginalExecutor>,
    ) -> GraphNodeExecutionFuture<'a> {
        Box::pin(async move {
            self.before_node(ctx, request.clone()).await?;
            let admission = self
                .admitted
                .lock()
                .map_err(|_| Error::Storage("Graph admission registry lock poisoned"))?
                .get(&(request.job_id.clone(), request.coordinate))
                .cloned()
                .ok_or(Error::Denied("Graph node lacks admitted policy route"))?;
            match admission.route {
                aw_policy::Strategy::Local => original.execute().await,
                aw_policy::Strategy::DelegateAgent => {
                    let status = Arc::new(Mutex::new(None));
                    let original_executor: Arc<dyn OriginalOperationExecutor> =
                        Arc::new(GraphNodeStatusExecutor {
                            original,
                            status: status.clone(),
                        });
                    execute_with_strategy(
                        aw_policy::Strategy::DelegateAgent,
                        request.operation,
                        original_executor,
                        admission.evidence.clone(),
                        self.authority.clone(),
                        Some(DelegateExecution {
                            request: DelegationRequest::transient(request.operation),
                            port: self.service.policy_delegate_port(ctx),
                            delegate_binding: None,
                            delegate_admission: self.service.policy_delegate_admission(ctx),
                        }),
                    )
                    .await?;
                    let factual = status
                        .lock()
                        .map_err(|_| Error::Storage("Graph node delegate status lock poisoned"))?
                        .take()
                        .ok_or(Error::Conflict(
                            "DelegateAgent returned without the factual Graph node executor",
                        ))?;
                    Ok(factual)
                }
            }
        })
    }
}

#[derive(Clone)]
pub struct PolicyDelegateAdmission {
    service: Service,
    parent_context: ToolContext,
}

impl DelegateAdmissionAuthority for PolicyDelegateAdmission {
    fn revalidate(&self, request: &DelegationRequest) -> Result<()> {
        if self.parent_context.cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if request.delegate_agent().is_some() {
            return Err(Error::Denied(
                "transient AW policy delegate admission cannot retarget an existing Agent",
            ));
        }
        let scope = self
            .parent_context
            .scope
            .as_ref()
            .ok_or(Error::Denied(
                "policy delegation requires a durable parent Session/branch",
            ))?;
        let owner = self
            .parent_context
            .execution_owner
            .as_ref()
            .ok_or(Error::Denied(
                "policy delegation requires a bound parent Agent identity",
            ))?;
        if owner.session_id() != scope.session_id
            || owner.branch_id() != scope.branch_id
            || self.parent_context.actor != self.parent_context.identity.actor
        {
            return Err(Error::Denied(
                "policy delegate parent identity differs from durable execution owner",
            ));
        }
        self.service.exact_selection(&self.parent_context)?;
        let branch = self
            .service
            .inner
            .store
            .branches(&self.parent_context.actor, &scope.session_id)?
            .into_iter()
            .find(|branch| branch.id == scope.branch_id)
            .ok_or(Error::Denied(
                "policy delegate parent branch is unavailable",
            ))?;
        if branch.active_run.is_none() {
            return Err(Error::Conflict(
                "policy delegation requires the original active parent Turn",
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct PolicyDelegatePort {
    service: Service,
    parent_context: ToolContext,
}

impl DelegateAgentPort for PolicyDelegatePort {
    fn drive<'a>(
        &'a self,
        request: &'a DelegationRequest,
        gate: Arc<DelegatedExecutionGate>,
    ) -> HostExecutionFuture<'a, ()> {
        Box::pin(async move {
            if request.delegate_agent().is_some() {
                return Err(Error::Denied(
                    "transient AW policy delegate port cannot retarget an existing Agent",
                ));
            }
            if self.parent_context.cancellation.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let operation = request.operation();
            let effect = delegated_operation_effect(operation)?;
            // Preserve the original capability identity even for Batch.
            // The delegate tool still accepts no arguments and routes only to
            // the already-prepared original executor, so Graph arguments stay
            // opaque without inventing a new child capability name.
            let name = operation.model_name().to_owned();
            let gate_spec = PolicyGateSpec {
                tool: Arc::new(DelegatedOriginalTool {
                    gate: gate.clone(),
                    name: name.clone(),
                    effect: effect.clone(),
                }),
                name: name.clone(),
                effect: effect.clone(),
            };
            let task = format!(
                "You are the operation-scoped execution delegate for {}. You have exactly one tool named {}. Decide whether to execute the already-prepared operation, and if executing, invoke that tool exactly once. Do not invent arguments, do not call unrelated tools, and do not treat your prose as the operation result. The parent receives only the factual ToolOutput produced by that tool.",
                operation.semantic_name(),
                name,
            );
            let prepared = self
                .service
                .prepare(
                    &self.parent_context,
                    SpawnArgs {
                        task: task.clone(),
                        title: Some(format!("Delegate: {}", operation.semantic_name())),
                        mode: ModeArg::Foreground,
                    },
                    SubagentMode::Foreground,
                    true,
                    None,
                    Some(ToolMode::Off),
                    false,
                    Some(gate_spec),
                )
                .await?;
            if prepared.actor_lease.is_none() {
                let _ = self.service.inner.store.settle_subagent(
                    &self.parent_context.actor,
                    &prepared.info.id,
                    "failed",
                    Some("policy delegation requires the authenticated child Actor runtime"),
                );
                return Err(Error::Denied(
                    "policy delegation requires the authenticated child Actor runtime",
                ));
            }

            let authority = prepared.agent.tool_binding_authority();
            gate.bind_delegate_tool(
                &authority,
                &self.parent_context.actor,
                &name,
            )?;

            let result = Service::run_prepared(
                self.service.inner.clone(),
                prepared,
                self.parent_context.cancellation.clone(),
            )
            .await;
            result.map(|_| ())
        })
    }
}

#[derive(Clone, Copy, Debug)]
enum LiveAgentCommand {
    Wake,
    Close,
}

#[derive(Clone)]
struct LiveAgentWakeHandle {
    sender: mpsc::Sender<LiveAgentCommand>,
    closing: Arc<AtomicBool>,
}
impl aw::agent_jobs::LiveAgentWake for LiveAgentWakeHandle {
    fn wake(&self) -> Result<()> {
        if self.closing.load(Ordering::Acquire) {
            return Err(Error::Conflict("Agent is closing and accepts no future input"));
        }
        self.sender
            .send(LiveAgentCommand::Wake)
            .map_err(|_| Error::Conflict("live Agent worker is unavailable"))
    }

    fn close(&self) -> Result<()> {
        self.closing.store(true, Ordering::Release);
        self.sender
            .send(LiveAgentCommand::Close)
            .map_err(|_| Error::Conflict("live Agent worker is unavailable"))
    }

    fn is_closing(&self) -> bool {
        self.closing.load(Ordering::Acquire)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ModeArg {
    #[default]
    Foreground,
    Background,
}

struct ActiveBackground {
    counter: Arc<Inner>,
}
impl Drop for ActiveBackground {
    fn drop(&mut self) {
        self.counter
            .active_background
            .fetch_sub(1, Ordering::AcqRel);
    }
}

struct Prepared {
    info: SubagentInfo,
    agent: Agent,
    selection: ProviderSelection,
    task: String,
    actor_lease: Option<crate::child_actors::ChildLease>,
    execution_lease: Option<ExecutionOwnershipLease>,
}

struct StableAgentRuntime {
    router: Arc<aw::agent_jobs::Router>,
    canonical_ref: String,
    owner: aw::actor_owner::Service,
    receiver: mpsc::Receiver<LiveAgentCommand>,
    closing: Arc<AtomicBool>,
}

impl Service {
    pub fn new(
        store: SessionStore,
        accounts: ProviderAccounts,
        settings: Settings,
        config: ServiceConfig,
    ) -> Result<Self> {
        let ServiceConfig {
            child_tool_mode,
            child_tool_concurrency,
            child_tool_limits,
            budget,
            child_routing,
            state_dir: _state_dir,
            trusted_git: _trusted_git,
            child_actors,
        } = config;
        if child_tool_mode == ToolMode::Trusted {
            return Err(Error::Denied(
                "Phase 42 child Agents do not support trusted/bash tools",
            ));
        }
        if let Some(actors) = child_actors.as_ref() {
            // Fail before private state/Git worktree provisioning when the
            // opted-in Runtime owner has not delegated exact target tool grants.
            actors.preflight_child_tool_mode(child_tool_mode)?;
        }
        #[cfg(not(feature = "routing"))]
        if child_routing.is_some() {
            return Err(Error::Denied(
                "this build does not include child routing",
            ));
        }
        #[cfg(not(feature = "workspace"))]
        if child_tool_mode != ToolMode::Off {
            return Err(Error::Denied(
                "this build needs the workspace/Git feature for tool-enabled subagents",
            ));
        }
        #[cfg(feature = "workspace")]
        let git = if child_tool_mode == ToolMode::Off {
            None
        } else {
            let git = _trusted_git.ok_or_else(|| {
                Error::Config("tool-enabled subagents require an explicit trusted Git executable".into())
            })?;
            if !git.is_absolute() || !git.is_file() {
                return Err(Error::Config(
                    "tool-enabled subagents require an absolute trusted AW_GIT executable".into(),
                ));
            }
            aw_platform::ensure_private_dir(&_state_dir)
                .map_err(|_| Error::Denied("subagent state directory is not private"))?;
            let subagents = _state_dir.join("subagents");
            aw_platform::ensure_private_dir(&subagents)
                .map_err(|_| Error::Denied("subagent state directory is not private"))?;
            let worktrees = subagents.join("worktrees");
            aw_platform::ensure_private_dir(&worktrees)
                .map_err(|_| Error::Denied("subagent worktree directory is not private"))?;
            Some(git.canonicalize()?)
        };
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                accounts,
                settings,
                child_tool_mode,
                child_tool_concurrency,
                child_tool_limits,
                budget,
                #[cfg(feature = "routing")]
                child_routing,
                #[cfg(feature = "workspace")]
                state_dir: _state_dir,
                #[cfg(feature = "workspace")]
                git,
                active_background: AtomicUsize::new(0),
                child_actors,
                agent_jobs: Mutex::new(None),
            }),
            lineage: None,
        })
    }

    fn for_child(&self, root_id: String, allocation_id: String, depth: u8) -> Self {
        Self {
            inner: self.inner.clone(),
            lineage: Some(Lineage {
                root_id,
                allocation_id,
                depth,
            }),
        }
    }

    pub fn tool(&self) -> Arc<dyn Tool> {
        Arc::new(self.clone())
    }

    pub fn policy_delegate_port(
        &self,
        parent_context: &ToolContext,
    ) -> Arc<dyn DelegateAgentPort> {
        Arc::new(PolicyDelegatePort {
            service: self.clone(),
            parent_context: parent_context.clone(),
        })
    }

    pub fn policy_delegate_admission(
        &self,
        parent_context: &ToolContext,
    ) -> Arc<dyn DelegateAdmissionAuthority> {
        Arc::new(PolicyDelegateAdmission {
            service: self.clone(),
            parent_context: parent_context.clone(),
        })
    }

    pub fn bind_agent_jobs_router(&self, router: Arc<aw::agent_jobs::Router>) -> Result<()> {
        let mut slot = self
            .inner
            .agent_jobs
            .lock()
            .map_err(|_| Error::Storage("Agent jobs router binding poisoned"))?;
        if let Some(existing) = slot.as_ref() {
            if !Arc::ptr_eq(existing, &router) {
                return Err(Error::Conflict(
                    "subagent Service cannot be rebound to another Agent jobs router",
                ));
            }
            return Ok(());
        }
        *slot = Some(router);
        Ok(())
    }

    fn agent_jobs_router(&self) -> Result<Arc<aw::agent_jobs::Router>> {
        self.inner
            .agent_jobs
            .lock()
            .map_err(|_| Error::Storage("Agent jobs router binding poisoned"))?
            .clone()
            .ok_or(Error::Denied(
                "stable agent constructor requires the live Agent jobs router",
            ))
    }

    pub fn agent_tool(&self) -> Arc<dyn Tool> {
        Arc::new(AgentConstructor {
            service: self.clone(),
        })
    }
    #[cfg(all(feature = "pipelines", feature = "routing"))]
    pub fn delegation_limits(&self) -> DelegationLimits {
        self.inner.budget.limits
    }

    #[cfg(all(feature = "pipelines", feature = "routing"))]
    pub fn child_route_revision(&self) -> Option<&str> {
        self.child_routing_revision_id()
    }

    /// Optional Actor Step authority over existing paid decision executors.
    /// Store journal/owner budgets remain mandatory even when this is absent.
    #[cfg(feature = "pipelines")]
    pub fn actor_authority(&self) -> Option<crate::child_actors::Service> {
        self.inner.child_actors.clone()
    }
    pub fn authorize_decision(
        &self,
        ctx: &ToolContext,
        kind: aw_agent_core::DecisionKind,
        selected: &ProviderSelection,
    ) -> Result<()> {
        if let Some(actors) = &self.inner.child_actors {
            actors.authorize_decision_step(ctx, kind, selected)?;
        }
        Ok(())
    }
    fn exact_selection(&self, ctx: &ToolContext) -> Result<ProviderSelection> {
        let selected = self
            .inner
            .accounts
            .select(&ctx.actor, &ctx.identity.account_id, &ctx.identity.model)?;
        if selected.identity() != &ctx.identity {
            return Err(Error::Denied(
                "parent provider/account/model authority changed; spawn was not dispatched",
            ));
        }
        Ok(selected)
    }

    fn child_routing_revision_id(&self) -> Option<&str> {
        #[cfg(feature = "routing")]
        {
            self.inner
                .child_routing
                .as_ref()
                .map(|routing| routing.strategy.revision_id.as_str())
        }
        #[cfg(not(feature = "routing"))]
        {
            None
        }
    }

    async fn select_child_target(
        &self,
        ctx: &ToolContext,
        root: &aw_store::DelegationRootInfo,
        allocation: &aw_store::DelegationChildAllocationInfo,
        parent_selection: ProviderSelection,
        task: &str,
    ) -> Result<ProviderSelection> {
        #[cfg(feature = "routing")]
        {
            let Some(config) = self.inner.child_routing.as_ref() else {
                return Ok(parent_selection);
            };
            if root.child_routing_revision_id.as_deref()
                != Some(config.strategy.revision_id.as_str())
            {
                return Err(Error::Conflict(
                    "delegation root child routing revision differs from host configuration",
                ));
            }
            let scope = ctx.scope.as_ref().ok_or(Error::Denied(
                "routed subagents require a durable direct parent Session/branch",
            ))?;
            let scratch = Agent::builder_from_selection(
                parent_selection.clone(),
                AgentOptions::new(
                    parent_selection.identity().account_id.clone(),
                    parent_selection.identity().model.clone(),
                ),
            )
            .build()?;
            let remaining_micros = root.deadline_at.saturating_sub(unix_micros());
            if remaining_micros <= 0 {
                return Err(Error::Limit("delegation root deadline is exhausted"));
            }
            let input = crate::routing_runtime::routing_input(
                &config.strategy.revision_id,
                &config.strategy.definition,
                "worker",
                "subagents",
                task,
                config
                    .request_timeout
                    .min(Duration::from_micros(remaining_micros as u64)),
            )?;
            let input_sha = input.digest()?;
            let candidate_sha = input.candidate_ids_digest()?;
            let strategy = &config.strategy.definition;

            let (selection, operation) = match strategy.mode {
                RoutingMode::Fixed => {
                    let target_id = strategy
                        .fixed_target
                        .as_deref()
                        .ok_or(Error::Storage("fixed child route lost its target"))?;
                    let target = routing_target(strategy, target_id)
                        .map_err(|error| Error::Config(error.to_string()))?;
                    let selection = crate::routing_runtime::register_target(
                        &scratch,
                        &self.inner.store,
                        &config.profiles,
                        target,
                        config.idle_timeout,
                    )?;
                    selection.revalidate()?;
                    let operation = self.inner.store.routing_admit_resolved(
                        &ctx.actor,
                        &config.strategy.revision_id,
                        &scope.session_id,
                        &scope.branch_id,
                        "worker",
                        "subagents",
                        &input_sha,
                        &candidate_sha,
                        RoutingSelectionSource::Fixed,
                        target_id,
                    )?;
                    (selection, operation)
                }
                RoutingMode::RouteOnce => {
                    let affinity = self.inner.store.routing_affinity(
                        &ctx.actor,
                        &scope.session_id,
                        &scope.branch_id,
                        "worker",
                        "subagents",
                    )?;
                    if let Some(affinity) = affinity
                        .as_ref()
                        .filter(|value| value.strategy_revision_id == config.strategy.revision_id)
                    {
                        let target = routing_target(strategy, &affinity.target_id)
                            .map_err(|error| Error::Config(error.to_string()))?;
                        let selection = crate::routing_runtime::register_target(
                            &scratch,
                            &self.inner.store,
                            &config.profiles,
                            target,
                            config.idle_timeout,
                        )?;
                        selection.revalidate()?;
                        let operation = self.inner.store.routing_admit_resolved(
                            &ctx.actor,
                            &config.strategy.revision_id,
                            &scope.session_id,
                            &scope.branch_id,
                            "worker",
                            "subagents",
                            &input_sha,
                            &candidate_sha,
                            RoutingSelectionSource::Affinity,
                            &affinity.target_id,
                        )?;
                        (selection, operation)
                    } else {
                        self.classify_child_target(
                            ctx,
                            root,
                            allocation,
                            &scratch,
                            config,
                            input,
                            affinity.map(|value| value.revision),
                        )
                        .await?
                    }
                }
                RoutingMode::RouteEachPrompt => {
                    self.classify_child_target(
                        ctx,
                        root,
                        allocation,
                        &scratch,
                        config,
                        input,
                        None,
                    )
                    .await?
                }
            };
            self.inner.store.routing_consume_for_delegation(
                &ctx.actor,
                &operation.id,
                &allocation.id,
                selection.identity(),
            )?;
            Ok(selection)
        }
        #[cfg(not(feature = "routing"))]
        {
            let _ = (ctx, root, allocation, task);
            Ok(parent_selection)
        }
    }

    #[cfg(feature = "routing")]
    #[allow(clippy::too_many_arguments)]
    async fn classify_child_target(
        &self,
        ctx: &ToolContext,
        root: &aw_store::DelegationRootInfo,
        allocation: &aw_store::DelegationChildAllocationInfo,
        scratch: &Agent,
        config: &ChildRoutingConfig,
        input: RoutingInput,
        affinity_expected_revision: Option<u64>,
    ) -> Result<(ProviderSelection, aw_store::RoutingOperationInfo)> {
        let strategy = &config.strategy.definition;
        let classifier = strategy
            .classifier
            .as_ref()
            .ok_or(Error::Storage("classified child route lost its classifier"))?;
        let classifier_selection = crate::routing_runtime::register_target(
            scratch,
            &self.inner.store,
            &config.profiles,
            classifier,
            config.idle_timeout,
        )?;
        self.authorize_decision(
            ctx,
            aw_agent_core::DecisionKind::Routing,
            &classifier_selection,
        )?;
        let scope = ctx.scope.as_ref().ok_or(Error::Denied(
            "classified child routing requires direct parent Session/branch",
        ))?;
        let operation = self.inner.store.routing_admit_classifier(
            &ctx.actor,
            &config.strategy.revision_id,
            &scope.session_id,
            &scope.branch_id,
            "worker",
            "subagents",
            &input.digest()?,
            &input.candidate_ids_digest()?,
        )?;
        let inner_journal = match self
            .inner
            .store
            .routing_classifier_journal(ctx.actor.clone(), &operation.id)
        {
            Ok(journal) => journal,
            Err(error) => {
                self.inner.store.routing_settle_unstarted(
                    &ctx.actor,
                    &operation.id,
                    matches!(error, Error::Cancelled),
                    &error.to_string(),
                )?;
                return Err(error);
            }
        };
        let journal = BudgetRoutingJournal {
            inner: Arc::new(inner_journal),
            store: self.inner.store.clone(),
            actor: ctx.actor.clone(),
            root_id: root.id.clone(),
            allocation_id: allocation.id.clone(),
            operation_id: operation.id.clone(),
            classifier_identity: classifier_selection.identity().clone(),
        };
        let result = match config
            .executor
            .execute(
                &classifier_selection,
                &input,
                &journal,
                ctx.cancellation.cancelled(),
            )
            .await
        {
            Ok(result) => result,
            Err(error) => {
                self.inner.store.routing_settle_unstarted(
                    &ctx.actor,
                    &operation.id,
                    matches!(error, Error::Cancelled),
                    &error.to_string(),
                )?;
                return Err(error);
            }
        };
        if result.outcome != aw::conversation::Outcome::Completed {
            return Err(Error::Protocol(
                "child routing classifier did not produce one valid completed candidate; inspect `aw route status` before retrying",
            ));
        }
        let target_id = result
            .proposal
            .as_ref()
            .ok_or(Error::Protocol("completed child routing decision has no candidate"))?
            .candidate_id
            .clone();
        if strategy.mode == RoutingMode::RouteOnce {
            self.inner.store.routing_affinity_establish(
                &ctx.actor,
                &operation.id,
                affinity_expected_revision,
            )?;
        }
        let target = routing_target(strategy, &target_id)
            .map_err(|error| Error::Config(error.to_string()))?;
        let selection = crate::routing_runtime::register_target(
            scratch,
            &self.inner.store,
            &config.profiles,
            target,
            config.idle_timeout,
        )?;
        selection.revalidate()?;
        let operation = self
            .inner
            .store
            .routing_operation(&ctx.actor, &operation.id)?;
        Ok((selection, operation))
    }

    fn default_title(task: &str) -> String {
        let mut title = String::from("Subagent: ");
        for ch in task.trim().chars().take(72) {
            title.push(ch);
        }
        if title == "Subagent: " {
            "Subagent".into()
        } else {
            title
        }
    }

    fn validate_args(arguments: &Value) -> Result<SpawnArgs> {
        let args: SpawnArgs = serde_json::from_value(arguments.clone()).map_err(|_| {
            Error::Denied("subagent arguments must contain task, optional title, optional mode")
        })?;
        if args.task.trim().is_empty()
            || args.task.len() > aw::config::MAX_PROMPT_BYTES
            || args.task.chars().any(|ch| ch == '\0')
        {
            return Err(Error::Config(
                "subagent task needs 1..131072 bounded UTF-8 bytes".into(),
            ));
        }
        if let Some(title) = &args.title {
            if title.trim().is_empty()
                || title.len() > 160
                || title.chars().any(char::is_control)
            {
                return Err(Error::Config(
                    "subagent title needs 1..160 printable bytes".into(),
                ));
            }
        }
        Ok(args)
    }

    fn validate_agent_args(arguments: &Value) -> Result<AgentSpawnArgs> {
        let args: AgentSpawnArgs = serde_json::from_value(arguments.clone()).map_err(|_| {
            Error::Denied(
                "agent arguments must contain task, optional title, optional background",
            )
        })?;
        if args.task.trim().is_empty()
            || args.task.len() > aw::config::MAX_PROMPT_BYTES
            || args.task.contains('\0')
        {
            return Err(Error::Config(
                "agent task needs 1..131072 bounded UTF-8 bytes".into(),
            ));
        }
        if let Some(title) = &args.title {
            if title.trim().is_empty()
                || title.len() > 160
                || title.chars().any(char::is_control)
            {
                return Err(Error::Config(
                    "agent title needs 1..160 printable bytes".into(),
                ));
            }
        }
        Ok(args)
    }

    fn agent_constructor_fingerprint(
        ctx: &ToolContext,
        args: &AgentSpawnArgs,
    ) -> Result<String> {
        let owner = ctx
            .execution_owner
            .as_ref()
            .ok_or(Error::Denied("agent constructor requires a bound parent Agent"))?;
        let scope = ctx
            .scope
            .as_ref()
            .ok_or(Error::Denied("agent constructor requires a durable parent scope"))?;
        let request = json!({
            "task": args.task,
            "title": args.title,
            "background": args.background,
            "parent_agent": owner.canonical_ref(),
            "session": scope.session_id,
            "branch": scope.branch_id,
            "turn": ctx.turn_id,
            "source_kind": ctx.source.kind_label(),
            "source_id": ctx.source.id,
            "source_epoch": ctx.source.epoch,
            "execution_site": ctx.execution_site,
            "identity": ctx.identity,
        });
        let encoded = serde_json::to_vec(&request)
            .map_err(|_| Error::Protocol("cannot encode Agent constructor fingerprint"))?;
        let mut hash = Sha256::new();
        hash.update(b"aw-agent-constructor-request-v1\0");
        hash.update(encoded);
        Ok(format!("{:x}", hash.finalize()))
    }

    async fn prepare_agent(
        &self,
        ctx: &ToolContext,
        args: AgentSpawnArgs,
        constructor_fingerprint: &str,
    ) -> Result<(Prepared, bool)> {
        let background = args.background;
        let spawn = SpawnArgs {
            task: args.task,
            title: args.title,
            mode: if background {
                ModeArg::Background
            } else {
                ModeArg::Foreground
            },
        };
        let mode = if background {
            SubagentMode::Background
        } else {
            SubagentMode::Foreground
        };
        Ok((
            self.prepare(
                ctx,
                spawn,
                mode,
                false,
                Some(constructor_fingerprint),
                None,
                true,
                None,
            )
            .await?,
            background,
        ))
    }

    fn reserve_background(inner: &Arc<Inner>) -> Result<ActiveBackground> {
        loop {
            let current = inner.active_background.load(Ordering::Acquire);
            if current >= MAX_ACTIVE_BACKGROUND {
                return Err(Error::Limit(
                    "at most four background subagents may run at once",
                ));
            }
            if inner
                .active_background
                .compare_exchange(
                    current,
                    current + 1,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return Ok(ActiveBackground {
                    counter: inner.clone(),
                });
            }
        }
    }

    #[cfg(feature = "workspace")]
    fn provision_execution_binding(
        &self,
        ctx: &ToolContext,
        child_tool_mode: ToolMode,
        delegation_root_id: &str,
        delegation_allocation_id: &str,
    ) -> Result<Option<String>> {
        let inner = &self.inner;
        if child_tool_mode == ToolMode::Off {
            return Ok(None);
        }
        let scope = ctx.scope.as_ref().ok_or(Error::Denied(
            "tool-enabled subagents require a durable parent Session/branch",
        ))?;
        let parent = inner.store.session(&ctx.actor, &scope.session_id)?;
        let git = inner.git.as_ref().ok_or(Error::Denied(
            "tool-enabled subagents require a trusted Git executable",
        ))?;
        let repo = aw_git::Repository::discover(git, &parent.binding.tool_root)
            .map_err(|error| Error::Config(format!("cannot prepare isolated child worktree: {error}")))?;
        let repo_root = repo.root().canonicalize()?;
        let parent_cwd = parent.binding.working_directory.canonicalize()?;
        let parent_tool_root = parent.binding.tool_root.canonicalize()?;
        if parent_cwd.strip_prefix(&repo_root).is_err()
            || parent_tool_root.strip_prefix(&repo_root).is_err()
        {
            return Err(Error::Denied(
                "parent cwd/tool root must stay inside one Git worktree for Phase 42",
            ));
        }
        let base_revision = repo
            .clean_head_for_managed_worktree()
            .map_err(|error| Error::Config(format!("cannot snapshot parent worktree: {error}")))?;
        let managed_parent = inner.state_dir.join("subagents").join("worktrees");
        let managed_parent = managed_parent.canonicalize()?;
        if managed_parent.starts_with(&repo_root) {
            return Err(Error::Denied(
                "aw managed-worktree directory cannot live inside the parent repository",
            ));
        }
        if let Some(lineage) = &self.lineage {
            let parent_execution = inner
                .store
                .subagent_execution_binding_for_child_session(&ctx.actor, &scope.session_id)?
                .ok_or(Error::Denied(
                    "recursive tool-enabled delegation requires the direct parent managed binding",
                ))?;
            if parent_execution.state != "bound"
                || parent_execution.delegation_root_id.as_deref() != Some(&lineage.root_id)
                || parent_execution.delegation_allocation_id.as_deref()
                    != Some(&lineage.allocation_id)
                || parent_execution.execution_root.canonicalize()? != repo_root
            {
                return Err(Error::Denied(
                    "recursive tool-enabled delegation parent binding does not match lineage",
                ));
            }
        } else if repo_root.starts_with(&managed_parent) {
            return Err(Error::Denied(
                "top-level parent repository cannot already be an aw managed subagent worktree",
            ));
        }
        let binding_id = aw_store::new_subagent_execution_binding_id()?;
        let execution_root = managed_parent.join(&binding_id);
        inner.store.plan_subagent_execution_binding(
            &ctx.actor,
            &binding_id,
            &scope.session_id,
            &scope.branch_id,
            ctx.turn_id,
            delegation_root_id,
            delegation_allocation_id,
            child_tool_mode.storage_label(),
            &repo_root,
            &execution_root,
            &base_revision,
        )?;
        match repo.create_managed_worktree(&execution_root, &base_revision) {
            Ok(worktree) => {
                let actual = worktree.path.canonicalize().map_err(Error::Io)?;
                if actual != execution_root || worktree.head.as_deref() != Some(&base_revision) {
                    let _ = inner.store.mark_subagent_execution_unknown(
                        &ctx.actor,
                        &binding_id,
                        "managed worktree verification differed from the planned path/revision",
                    );
                    return Err(Error::Storage(
                        "managed worktree outcome requires inspection; no automatic retry",
                    ));
                }
                inner
                    .store
                    .mark_subagent_execution_ready(&ctx.actor, &binding_id)?;
                Ok(Some(binding_id))
            }
            Err(error) => {
                let detail = format!("managed worktree creation outcome unknown: {error}");
                let _ = inner
                    .store
                    .mark_subagent_execution_unknown(&ctx.actor, &binding_id, &detail);
                Err(Error::Storage(
                    "managed worktree creation outcome requires inspection; no automatic retry",
                ))
            }
        }
    }

    #[cfg(not(feature = "workspace"))]
    fn provision_execution_binding(
        &self,
        _ctx: &ToolContext,
        child_tool_mode: ToolMode,
        _delegation_root_id: &str,
        _delegation_allocation_id: &str,
    ) -> Result<Option<String>> {
        if child_tool_mode == ToolMode::Off {
            Ok(None)
        } else {
            Err(Error::Denied(
                "this build does not include managed Git worktrees",
            ))
        }
    }

    async fn prepare(
        &self,
        ctx: &ToolContext,
        args: SpawnArgs,
        mode: SubagentMode,
        attach_legacy_child_actor: bool,
        constructor_fingerprint: Option<&str>,
        tool_mode_override: Option<ToolMode>,
        allow_recursive_tools: bool,
        policy_gate: Option<PolicyGateSpec>,
    ) -> Result<Prepared> {
        let inner = &self.inner;
        let child_tool_mode = tool_mode_override.unwrap_or(inner.child_tool_mode);
        if ctx.cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let scope = ctx.scope.as_ref().ok_or(Error::Denied(
            "subagents require a durable parent Session/branch",
        ))?;
        let execution_scope = ctx.execution_scope.clone();
        let execution_lease = execution_scope
            .as_ref()
            .map(|scope| scope.acquire())
            .transpose()?;

        let parent_selection = {
            let selected = inner
                .accounts
                .select(&ctx.actor, &ctx.identity.account_id, &ctx.identity.model)?;
            if selected.identity() != &ctx.identity {
                return Err(Error::Denied(
                    "parent provider/account/model authority changed; spawn was not dispatched",
                ));
            }
            selected
        };

        let title = args
            .title
            .clone()
            .unwrap_or_else(|| Self::default_title(&args.task));
        let task_sha256 = format!("{:x}", Sha256::digest(args.task.as_bytes()));
        let (root, allocation) = if let Some(lineage) = &self.lineage {
            if lineage.depth >= inner.budget.limits.max_depth {
                return Err(Error::Limit("delegation recursive depth limit reached"));
            }
            inner.store.reserve_delegation_descendant(
                &ctx.actor,
                &lineage.root_id,
                &lineage.allocation_id,
                &scope.session_id,
                &scope.branch_id,
                ctx.turn_id,
                inner.budget.limits,
                self.child_routing_revision_id(),
                &ctx.identity,
            )?
        } else {
            inner.store.reserve_delegation_child(
                &ctx.actor,
                &scope.session_id,
                &scope.branch_id,
                ctx.turn_id,
                inner.budget.limits,
                self.child_routing_revision_id(),
                1,
            )?
        };
        let selected = match self
            .select_child_target(
                ctx,
                &root,
                &allocation,
                parent_selection,
                &args.task,
            )
            .await
        {
            Ok(selection) => selection,
            Err(error) => {
                let detail = error.to_string();
                let bounded = detail.chars().take(2048).collect::<String>();
                let _ = inner.store.fail_delegation_child_allocation(
                    &ctx.actor,
                    &allocation.id,
                    &bounded,
                );
                return Err(error);
            }
        };
        let execution_binding_id = match self.provision_execution_binding(
            ctx,
            child_tool_mode,
            &root.id,
            &allocation.id,
        ) {
            Ok(value) => value,
            Err(error) => {
                let detail = error.to_string();
                let bounded = detail.chars().take(2048).collect::<String>();
                let _ = inner.store.fail_delegation_child_allocation(
                    &ctx.actor,
                    &allocation.id,
                    &bounded,
                );
                return Err(error);
            }
        };
        let tool_mode=child_tool_mode.storage_label().to_owned();
        let creation = if let Some(fingerprint) = constructor_fingerprint {
            inner.store.create_agent_job_session(
                &ctx.actor,
                &scope.session_id,
                &scope.branch_id,
                ctx.turn_id,
                &title,
                &task_sha256,
                selected.identity(),
                &root.id,
                &allocation.id,
                allocation.depth,
                &tool_mode,
                execution_binding_id.as_deref(),
                mode,
                &ctx.call_id,
                fingerprint,
            )
        } else {
            inner.store.create_subagent_session(
                &ctx.actor,
                &scope.session_id,
                &scope.branch_id,
                ctx.turn_id,
                &title,
                &task_sha256,
                selected.identity(),
                &root.id,
                &allocation.id,
                allocation.depth,
                &tool_mode,
                execution_binding_id.as_deref(),
                mode,
            )
        };
        let (info, child, child_branch) = match creation {
            Ok(value) => value,
            Err(error) => {
                let detail = error.to_string();
                let bounded = detail.chars().take(2048).collect::<String>();
                let _ = inner.store.fail_delegation_child_allocation(
                    &ctx.actor,
                    &allocation.id,
                    &bounded,
                );
                return Err(error);
            }
        };

        let mut options = AgentOptions::new(
            selected.identity().account_id.clone(),
            selected.identity().model.clone(),
        );
        let child_scope = match child_tool_mode {
            ToolMode::Off => "You have no local file or shell tools.",
            ToolMode::ReadOnly => {
                "You may read bounded files only inside your isolated managed Git worktree."
            }
            ToolMode::Workspace => {
                "You may read/write/edit bounded files only inside your isolated managed Git worktree."
            }
            ToolMode::Trusted => unreachable!("constructor rejects trusted child tools"),
        };
        let recursive_scope = if !allow_recursive_tools {
            "You cannot create or delegate to another Agent in this operation-scoped child.".to_owned()
        } else if allocation.depth < root.limits.max_depth {
            format!(
                "You may delegate bounded sub-tasks through the subagent tool. Descendants share this root's limits and you cannot choose their provider/model/root/tool authority. Current depth {}/{}.",
                allocation.depth, root.limits.max_depth
            )
        } else {
            format!(
                "You are at the delegation depth limit {}/{} and cannot spawn subagents.",
                allocation.depth, root.limits.max_depth
            )
        };
        options.system_prompt = format!(
            "{}

You are a bounded aw subagent in an isolated child Session. Complete only the delegated task.              Return a concise result for the parent Agent. Do not assume access to the parent transcript.              {child_scope} {recursive_scope} You have no shell/Git/network management authority.",
            inner.settings.system_prompt
        );
        let remaining_micros = root.deadline_at.saturating_sub(unix_micros());
        if remaining_micros <= 0 {
            let detail = "delegation root deadline expired before child start";
            let _ = inner.store.fail_admitted_subagent(&ctx.actor, &info.id, detail);
            return Err(Error::Limit("delegation root deadline is exhausted"));
        }
        options.request_timeout = inner
            .settings
            .request_timeout
            .min(Duration::from_micros(remaining_micros as u64));
        options.max_output_bytes = inner.settings.max_output_bytes;

        if let Err(error) = inner.store.mark_subagent_running(&ctx.actor, &info.id) {
            let detail = error.to_string();
            let bounded = detail.chars().take(2048).collect::<String>();
            let _ = inner
                .store
                .fail_admitted_subagent(&ctx.actor, &info.id, &bounded);
            return Err(error);
        }
        let setup = (|| -> Result<Agent> {
            let mut builder = Agent::builder_from_selection(selected.clone(), options);
            let stable_agent = !attach_legacy_child_actor;
            let visible_subscribe = stable_agent && selected.supports_tools();
            if child_tool_mode != ToolMode::Off || visible_subscribe {
                let mut child_tools =
                    LocalToolOptions::new(child.binding.tool_root.clone(), child_tool_mode);
                // Preserve the Phase-42 exact filesystem child set. A stable
                // Agent additionally receives only the Agent-plane subscription
                // control, even when filesystem ToolMode is Off.
                child_tools.selected_tools = match child_tool_mode {
                    ToolMode::ReadOnly => vec!["read".into()],
                    ToolMode::Workspace => vec!["read".into(), "write".into(), "edit".into()],
                    ToolMode::Off => Vec::new(),
                    ToolMode::Trusted => unreachable!("child mode validated"),
                };
                if visible_subscribe {
                    child_tools.selected_tools.push("jobs_subscribe".into());
                    child_tools.allow_wake = true;
                    let router_slot =
                        Arc::new(aw_tools_adapter::agent_jobs::AgentJobRouterSlot::default());
                    router_slot.replace(self.agent_jobs_router()?)?;
                    child_tools.agent_jobs = router_slot;
                }
                child_tools.execution_site = format!("subagent:{}", info.id);
                child_tools.concurrency = inner.child_tool_concurrency;
                child_tools.limits = inner.child_tool_limits;
                if child_tool_mode != ToolMode::Off {
                    child_tools.private_state_dir = Some(inner.state_dir.clone());
                }
                builder = aw_tools_adapter::configure(builder, ctx.actor.clone(), &child_tools)?;
            }
            let mut agent = builder.build()?;
            if let Some(scope) = execution_scope.as_ref() {
                agent.bind_execution_scope(scope.clone())?;
            }
            if stable_agent {
                // Wake authority is host-owned and does not require the model
                // provider to support ToolCalls. Tool-capable providers may
                // additionally receive the visible jobs_subscribe surface.
                agent.permit_host_capability(
                    ctx.actor.clone(),
                    "jobs_subscribe",
                    &ToolEffect::Control,
                )?;
            }
            if let Some(gate) = policy_gate.as_ref() {
                agent.register_host_tool(gate.tool.clone())?;
            }
            if allow_recursive_tools && allocation.depth < root.limits.max_depth {
                let descendant_service =
                    self.for_child(root.id.clone(), allocation.id.clone(), allocation.depth);
                agent.register_host_tool(descendant_service.tool())?;
                if !attach_legacy_child_actor {
                    // Stable Agents inherit only the same already-bound router
                    // and delegation root. This adds no provider/root/tool
                    // authority and legacy one-shot subagents keep their exact
                    // historical tool surface until hard cutover.
                    let _ = descendant_service.agent_jobs_router()?;
                    agent.register_host_tool(descendant_service.agent_tool())?;
                }
            }
            let (view, handle) = inner
                .store
                .activate(&ctx.actor, &child.id, &child_branch.id)?;
            let journal: Arc<dyn TurnJournal> = Arc::new(BudgetJournal {
                inner: Arc::new(handle),
                store: inner.store.clone(),
                actor: ctx.actor.clone(),
                root_id: root.id.clone(),
                child_session_id: child.id.clone(),
            });
            agent.load_history(view.history, Some(journal))?;
            let execution = inner
                .store
                .agent_binding(&ctx.actor, &child.id, &child_branch.id)?;
            agent.bind_execution_owner(AgentExecutionBinding::from_trusted_host(
                format!(
                    "/d:{}/i:{}/p:{}/a:{}",
                    execution.domain_handle,
                    execution.identity_handle,
                    execution.project_handle,
                    execution.agent_handle,
                ),
                execution.session_id,
                execution.branch_id,
            )?)?;
            Ok(agent)
        })();
        let agent = match setup {
            Ok(agent) => agent,
            Err(error) => {
                let detail = error.to_string();
                let bounded = detail.chars().take(2048).collect::<String>();
                let _ = inner
                    .store
                    .settle_subagent(&ctx.actor, &info.id, "failed", Some(&bounded));
                return Err(error);
            }
        };

        let actor_lease=if attach_legacy_child_actor {
            if let Some(service)=&inner.child_actors {
            let turn=agent.history_state().next_turn_id;
            let attached = if let Some(gate) = policy_gate.as_ref() {
                service.attach_delegated(
                    &info,
                    &agent,
                    &selected,
                    &args.task,
                    turn,
                    &gate.name,
                    kernel_effect(&gate.effect),
                )
            } else {
                service.attach(&info, &agent, &selected, &args.task, turn)
            };
            match attached {
                Ok(lease)=>Some(lease),
                Err(error)=>{
                    // The normal SessionStore remains the owner of lifecycle
                    // facts. No unbound Actor may silently execute a child.
                    let note=error.to_string();
                    let bounded=note.chars().take(2048).collect::<String>();
                    let _=inner.store.settle_subagent(&ctx.actor,&info.id,
                        "failed",Some(&bounded));
                    return Err(error);
                }
            }
        } else {None}
        } else {
            None
        };
        Ok(Prepared {
            info,
            agent,
            selection: selected,
            task: args.task,
            actor_lease,
            execution_lease,
        })
    }

    fn bounded_answer(answer: &str) -> (String, bool) {
        if answer.len() <= MAX_TOOL_ANSWER_BYTES {
            return (answer.to_owned(), false);
        }
        let mut end = MAX_TOOL_ANSWER_BYTES;
        while !answer.is_char_boundary(end) {
            end -= 1;
        }
        (answer[..end].to_owned(), true)
    }


    fn replay_agent_constructor(
        &self,
        ctx: &ToolContext,
        args: &AgentSpawnArgs,
        fingerprint: &str,
    ) -> Result<Option<ToolOutput>> {
        let scope = ctx
            .scope
            .as_ref()
            .ok_or(Error::Denied("Agent constructor replay requires durable parent scope"))?;
        let Some((relation, stored_fingerprint)) = self.inner.store.agent_constructor_replay(
            &ctx.actor,
            &scope.session_id,
            &scope.branch_id,
            ctx.turn_id,
            &ctx.call_id,
        )? else {
            return Ok(None);
        };
        if stored_fingerprint != fingerprint {
            return Err(Error::Denied(
                "Agent constructor call identity was reused with changed semantics",
            ));
        }
        let binding = self
            .inner
            .store
            .agent_bindings(&ctx.actor)?
            .into_iter()
            .find(|binding| binding.session_id == relation.child_session_id)
            .ok_or(Error::Storage(
                "Agent constructor replay child has no Session-derived binding",
            ))?;
        let canonical = format!(
            "/d:{}/i:{}/p:{}/a:{}",
            binding.domain_handle,
            binding.identity_handle,
            binding.project_handle,
            binding.agent_handle,
        );
        let revision = self.inner.store.agent_public_revision(
            &ctx.actor,
            &binding.session_id,
            &binding.branch_id,
        )?;
        if relation.state != "running" {
            return Ok(Some(ToolOutput::text(format!(
                "{canonical} agent closed rev={revision} unchanged"
            ))));
        }
        if self
            .inner
            .store
            .subagent_close_requested(&ctx.actor, &binding.session_id)?
        {
            return Ok(Some(ToolOutput::text(format!(
                "{canonical} agent closing rev={revision} unchanged"
            ))));
        }
        let branch = self
            .inner
            .store
            .branches(&ctx.actor, &binding.session_id)?
            .into_iter()
            .find(|branch| branch.id == binding.branch_id)
            .ok_or(Error::Storage("Agent constructor replay branch is missing"))?;
        if branch.active_run.is_some() {
            return Ok(Some(ToolOutput::text(format!(
                "{canonical} agent running rev={revision} unchanged"
            ))));
        }
        let view = self
            .inner
            .store
            .open(&ctx.actor, &binding.session_id, &binding.branch_id)?
            .view()?;
        let state = if view.history.needs_reconciliation {
            "blocked"
        } else if view.history.turns.is_empty() {
            "waiting"
        } else {
            "idle"
        };
        if args.background {
            return Ok(Some(ToolOutput::text(format!(
                "{canonical} agent {state} rev={revision} unchanged"
            ))));
        }
        let Some(turn) = view.history.turns.last() else {
            return Ok(Some(ToolOutput::text(format!(
                "{canonical} agent {state} rev={revision} unchanged"
            ))));
        };
        let outcome = match turn.outcome {
            aw::conversation::Outcome::Completed => "ok",
            aw::conversation::Outcome::Cancelled => "cancelled",
            aw::conversation::Outcome::Incomplete => "unknown",
            aw::conversation::Outcome::Failed => "fail",
        };
        let (answer, truncated) = Self::bounded_answer(&turn.answer);
        let mut text = format!(
            "{canonical} agent {state} rev={revision} turn=t:{} outcome={outcome} unchanged",
            turn.id
        );
        if !answer.is_empty() {
            text.push('\n');
            text.push_str(&answer);
        }
        if truncated {
            text.push_str("\n[answer truncated]");
        }
        Ok(Some(ToolOutput::text(text)))
    }

    fn attach_stable_agent_runtime(&self, prepared: &Prepared) -> Result<StableAgentRuntime> {
        if prepared.actor_lease.is_some() {
            return Err(Error::Conflict(
                "stable Agent must not share the legacy one-shot child Actor owner",
            ));
        }
        let binding = prepared
            .agent
            .execution_binding()
            .ok_or(Error::Storage(
                "stable Agent is missing its immutable Session-derived identity",
            ))?;
        if binding.session_id() != prepared.info.child_session_id {
            return Err(Error::Denied(
                "stable Agent relation and execution identity differ",
            ));
        }
        let owner = aw::actor_owner::Service::attach_for_agent(
            self.inner.store.clone(),
            binding.session_id(),
            binding.branch_id(),
            &prepared.agent,
        )?;
        let router = self.agent_jobs_router()?;
        let (sender, receiver) = mpsc::channel();
        let closing = Arc::new(AtomicBool::new(false));
        let wake: Arc<dyn aw::agent_jobs::LiveAgentWake> = Arc::new(LiveAgentWakeHandle {
            sender,
            closing: closing.clone(),
        });
        let canonical_ref = binding.canonical_ref().to_owned();
        router.register_live_owner_with_wake(
            &canonical_ref,
            owner.clone(),
            Some(wake),
            prepared.agent.tool_binding_authority(),
        )?;
        Ok(StableAgentRuntime {
            router,
            canonical_ref,
            owner,
            receiver,
            closing,
        })
    }

    async fn run_stable_claimed_turn(
        prepared: &mut Prepared,
        owner: &aw::actor_owner::Service,
        cancellation: CancellationToken,
    ) -> Result<aw_agent_core::Turn> {
        owner
            .run_once_selected_with_cancel(
                &mut prepared.agent,
                &prepared.selection,
                cancellation,
                |_| Ok(()),
            )
            .await?
            .ok_or(Error::Conflict(
                "accepted stable Agent Input produced no claimed Turn",
            ))
    }

    fn finish_scoped_initial_turn(
        prepared: &mut Prepared,
        result: &Result<aw_agent_core::Turn>,
    ) {
        if let Some(scope) = prepared.agent.execution_scope().cloned() {
            let unresolved = match result {
                Ok(turn) => turn.needs_reconciliation,
                Err(Error::Cancelled) => false,
                Err(_) => true,
            };
            if unresolved {
                scope.mark_unresolved();
            }
            prepared.agent.clear_execution_scope();
        }
        prepared.execution_lease.take();
    }

    async fn run_one_stable_queued_turn(
        prepared: &mut Prepared,
        owner: &aw::actor_owner::Service,
    ) -> Result<bool> {
        if owner.admit_next_queued(&prepared.agent)?.is_none() {
            return Ok(false);
        }
        let _ = Self::run_stable_claimed_turn(
            prepared,
            owner,
            CancellationToken::new(),
        )
        .await?;
        Ok(true)
    }

    fn close_stable_agent(
        inner: &Arc<Inner>,
        prepared: &Prepared,
        runtime: &StableAgentRuntime,
        reason: &str,
    ) -> Result<()> {
        runtime.closing.store(true, Ordering::Release);
        let (_principal, session, _branch, _site, _epoch) =
            runtime.owner.bound_peer_scope();
        for fact in runtime.owner.queue_list()? {
            if fact.state != QueueState::Pending {
                continue;
            }
            let request_id = fact.request.request_id.clone();
            runtime.owner.operate(
                aw::actor_owner::ActorOperation::DeleteQueue {
                    request_id: request_id.clone(),
                },
            )?;
            if request_id.starts_with("jc_") {
                // Queue custody is already deleted. A failed budget-release
                // audit leaks capacity conservatively but can never resurrect
                // runnable work or permit overspend.
                let _ = inner.store.cancel_delegation_continuation(
                    &prepared.info.identity.actor,
                    session,
                    &request_id,
                );
            }
        }
        inner.store.settle_subagent(
            &prepared.info.identity.actor,
            &prepared.info.id,
            "cancelled",
            Some(reason),
        )?;
        runtime.router.clear_live_owner(&runtime.canonical_ref)?;
        Ok(())
    }

    fn fail_stable_worker_creation(
        inner: &Arc<Inner>,
        prepared: &Prepared,
        runtime: &StableAgentRuntime,
        detail: &str,
    ) {
        let bounded = detail.chars().take(2048).collect::<String>();
        let _ = inner.store.settle_subagent(
            &prepared.info.identity.actor,
            &prepared.info.id,
            "failed",
            Some(&bounded),
        );
        let _ = runtime.router.clear_live_owner(&runtime.canonical_ref);
    }

    fn spawn_stable_idle_worker(
        inner: Arc<Inner>,
        mut prepared: Prepared,
        runtime_state: StableAgentRuntime,
        active: ActiveBackground,
    ) -> Result<()> {
        let name = format!(
            "aw-agent-{}",
            prepared
                .info
                .id
                .chars()
                .rev()
                .take(8)
                .collect::<String>()
        );
        let spawn = thread::Builder::new().name(name).spawn(move || {
            let _active = active;
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .max_blocking_threads(4)
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    Self::fail_stable_worker_creation(
                        &inner,
                        &prepared,
                        &runtime_state,
                        &format!("cannot start stable Agent runtime: {error}"),
                    );
                    return;
                }
            };

            loop {
                match runtime_state.receiver.recv() {
                    Ok(LiveAgentCommand::Wake) => {
                        if runtime_state.closing.load(Ordering::Acquire) {
                            continue;
                        }
                        // Exactly one accepted queue item per wake. More queued
                        // items already have their own wake receipts, while an
                        // explicit close bit preempts those pending wake signals.
                        let _ = runtime.block_on(Self::run_one_stable_queued_turn(
                            &mut prepared,
                            &runtime_state.owner,
                        ));
                    }
                    Ok(LiveAgentCommand::Close) => {
                        let _ = Self::close_stable_agent(
                            &inner,
                            &prepared,
                            &runtime_state,
                            "Agent closed by jobs_cancel",
                        );
                        break;
                    }
                    Err(_) => {
                        Self::fail_stable_worker_creation(
                            &inner,
                            &prepared,
                            &runtime_state,
                            "stable Agent wake channel disconnected",
                        );
                        break;
                    }
                }
            }
        });
        spawn.map(|_| ()).map_err(Error::Io)
    }

    fn spawn_stable_background_worker(
        inner: Arc<Inner>,
        mut prepared: Prepared,
        runtime_state: StableAgentRuntime,
        active: ActiveBackground,
    ) -> Result<()> {
        let (ready_tx, ready_rx) = mpsc::sync_channel::<std::result::Result<(), String>>(1);
        let name = format!(
            "aw-agent-{}",
            prepared
                .info
                .id
                .chars()
                .rev()
                .take(8)
                .collect::<String>()
        );
        let cleanup_inner = inner.clone();
        let cleanup_actor = prepared.info.identity.actor.clone();
        let cleanup_id = prepared.info.id.clone();
        let cleanup_router = runtime_state.router.clone();
        let cleanup_ref = runtime_state.canonical_ref.clone();
        let spawn = thread::Builder::new().name(name).spawn(move || {
            let _active = active;
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .max_blocking_threads(4)
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let detail = format!("cannot start stable Agent runtime: {error}");
                    let _ = ready_tx.send(Err(detail.clone()));
                    Self::fail_stable_worker_creation(
                        &inner,
                        &prepared,
                        &runtime_state,
                        &detail,
                    );
                    return;
                }
            };
            if let Err(error) = runtime_state
                .owner
                .submit_bound_text(&prepared.agent, &prepared.task)
            {
                let detail = format!("cannot admit stable Agent initial Turn: {error}");
                let _ = ready_tx.send(Err(detail.clone()));
                Self::fail_stable_worker_creation(
                    &inner,
                    &prepared,
                    &runtime_state,
                    &detail,
                );
                return;
            }
            // The handoff caller cannot return before this exact durable Input
            // admission exists. Paid inference may begin only afterwards.
            if ready_tx.send(Ok(())).is_err() {
                Self::fail_stable_worker_creation(
                    &inner,
                    &prepared,
                    &runtime_state,
                    "stable Agent constructor disappeared before handoff",
                );
                return;
            }
            let first_cancel = prepared
                .agent
                .execution_scope()
                .map(|scope| scope.cancellation())
                .unwrap_or_else(CancellationToken::new);
            let first = runtime.block_on(Self::run_stable_claimed_turn(
                &mut prepared,
                &runtime_state.owner,
                first_cancel,
            ));
            Self::finish_scoped_initial_turn(&mut prepared, &first);

            loop {
                match runtime_state.receiver.recv() {
                    Ok(LiveAgentCommand::Wake) => {
                        if runtime_state.closing.load(Ordering::Acquire) {
                            continue;
                        }
                        let _ = runtime.block_on(Self::run_one_stable_queued_turn(
                            &mut prepared,
                            &runtime_state.owner,
                        ));
                    }
                    Ok(LiveAgentCommand::Close) => {
                        let _ = Self::close_stable_agent(
                            &inner,
                            &prepared,
                            &runtime_state,
                            "Agent closed by jobs_cancel",
                        );
                        break;
                    }
                    Err(_) => {
                        Self::fail_stable_worker_creation(
                            &inner,
                            &prepared,
                            &runtime_state,
                            "stable Agent wake channel disconnected",
                        );
                        break;
                    }
                }
            }
        });
        if let Err(error) = spawn {
            let detail = format!("cannot spawn stable Agent worker: {error}");
            let bounded = detail.chars().take(2048).collect::<String>();
            let _ = cleanup_inner.store.settle_subagent(
                &cleanup_actor,
                &cleanup_id,
                "failed",
                Some(&bounded),
            );
            let _ = cleanup_router.clear_live_owner(&cleanup_ref);
            return Err(Error::Io(error));
        }
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(detail)) => Err(Error::StorageDetail(detail)),
            Err(_) => Err(Error::Storage(
                "stable Agent worker ended before first-Turn admission",
            )),
        }
    }

    async fn run_prepared(
        inner: Arc<Inner>,
        mut prepared: Prepared,
        cancellation: CancellationToken,
    ) -> Result<ToolOutput> {
        let actor = prepared.info.identity.actor.clone();
        let result = if let Some(lease)=prepared.actor_lease.as_ref() {
            lease.run_once(&mut prepared.agent,&prepared.selection,cancellation).await
        } else {
            prepared.agent.prompt_as(&prepared.selection,prepared.task,
                cancellation.cancelled(), |_|Ok(())).await
        };

        match result {
            Ok(turn) if turn.outcome == aw::conversation::Outcome::Completed => {
                inner.store.settle_subagent(&actor,&prepared.info.id,"completed",None)?;
                if let Some(lease)=&prepared.actor_lease {
                    lease.observe_settled(turn.needs_reconciliation)?;
                }
                let (answer, truncated) = Self::bounded_answer(&turn.answer);
                let body = json!({
                    "subagent_id": prepared.info.id,
                    "session_id": prepared.info.child_session_id,
                    "state": "completed",
                    "answer": answer,
                    "answer_truncated": truncated,
                });
                Ok(ToolOutput::text(body.to_string()))
            }
            Ok(turn) => {
                let state = if turn.outcome == aw::conversation::Outcome::Cancelled {
                    "cancelled"
                } else {
                    "failed"
                };
                let detail = turn
                    .error
                    .as_deref()
                    .unwrap_or("subagent did not complete successfully");
                inner.store.settle_subagent(&actor,&prepared.info.id,state,Some(detail))?;
                if let Some(lease)=&prepared.actor_lease {
                    lease.observe_settled(turn.needs_reconciliation)?;
                }
                Ok(ToolOutput::error(
                    json!({
                        "subagent_id": prepared.info.id,
                        "session_id": prepared.info.child_session_id,
                        "state": state,
                        "error": detail,
                    })
                    .to_string(),
                ))
            }
            Err(error) => {
                let state = if matches!(error, Error::Cancelled) {
                    "cancelled"
                } else {
                    "failed"
                };
                let detail = error.to_string();
                let bounded = detail.chars().take(2048).collect::<String>();
                let settled=inner.store.settle_subagent(
                    &actor,&prepared.info.id,state,Some(&bounded));
                if settled.is_ok() {
                    if let Some(lease)=&prepared.actor_lease {
                        // Aborted provider/effect may be unknown, never
                        // misreported as an independently settled Actor.
                        let _=lease.observe_settled(true);
                    }
                }
                Err(error)
            }
        }
    }
}


impl Tool for AgentConstructor {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "agent".into(),
            description: "Create one long-lived bounded Agent with an initial Turn. The Agent remains addressable after that Turn and continues only through jobs_send. background changes only response disposition; it never creates a second Agent.".into(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "task":{"type":"string","minLength":1,"maxLength":aw::config::MAX_PROMPT_BYTES},
                    "title":{"type":["string","null"],"minLength":1,"maxLength":160},
                    "background":{"type":["boolean","null"]}
                },
                "required":["task"],
                "additionalProperties":false
            }),
            effect: ToolEffect::Process,
        }
    }

    fn validate(&self, arguments: &Value) -> Result<()> {
        Service::validate_agent_args(arguments).map(|_| ())
    }

    fn execute<'a>(&'a self, ctx: ToolContext, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args = Service::validate_agent_args(arguments)?;
            self.service.exact_selection(&ctx)?;
            let fingerprint = Service::agent_constructor_fingerprint(&ctx, &args)?;
            if let Some(replay) = self
                .service
                .replay_agent_constructor(&ctx, &args, &fingerprint)?
            {
                return Ok(replay);
            }
            // Reserve the live-worker slot before creating any child Session.
            // A constructor that cannot retain the Agent must fail pre-effect.
            let active = Service::reserve_background(&self.service.inner)?;
            let (mut prepared, background) = self
                .service
                .prepare_agent(&ctx, args, &fingerprint)
                .await?;
            let runtime_state = match self.service.attach_stable_agent_runtime(&prepared) {
                Ok(runtime) => runtime,
                Err(error) => {
                    let detail = error.to_string();
                    let bounded = detail.chars().take(2048).collect::<String>();
                    let _ = self.service.inner.store.settle_subagent(
                        &prepared.info.identity.actor,
                        &prepared.info.id,
                        "failed",
                        Some(&bounded),
                    );
                    return Err(error);
                }
            };
            let canonical = runtime_state.canonical_ref.clone();
            let binding = prepared
                .agent
                .execution_binding()
                .ok_or(Error::Storage("stable Agent lost execution identity"))?
                .clone();

            if background {
                if let Err(error) = Service::spawn_stable_background_worker(
                    self.service.inner.clone(),
                    prepared,
                    runtime_state,
                    active,
                ) {
                    return Err(error);
                }
                let revision = self.service.inner.store.agent_public_revision(
                    &ctx.actor,
                    binding.session_id(),
                    binding.branch_id(),
                )?;
                return Ok(ToolOutput::text(format!(
                    "{canonical} agent waiting rev={revision}"
                )));
            }

            if let Err(error) = runtime_state
                .owner
                .submit_bound_text(&prepared.agent, &prepared.task)
            {
                Service::fail_stable_worker_creation(
                    &self.service.inner,
                    &prepared,
                    &runtime_state,
                    &format!("stable Agent initial admission failed: {error}"),
                );
                return Err(error);
            }

            let first = Service::run_stable_claimed_turn(
                &mut prepared,
                &runtime_state.owner,
                ctx.cancellation,
            )
            .await;
            Service::finish_scoped_initial_turn(&mut prepared, &first);
            let revision = self.service.inner.store.agent_public_revision(
                &ctx.actor,
                binding.session_id(),
                binding.branch_id(),
            )?;

            let response = match first {
                Ok(turn) => {
                    let state = if turn.needs_reconciliation {
                        "blocked"
                    } else {
                        "idle"
                    };
                    let outcome = match turn.outcome {
                        aw::conversation::Outcome::Completed => "ok",
                        aw::conversation::Outcome::Cancelled => "cancelled",
                        aw::conversation::Outcome::Incomplete => "unknown",
                        aw::conversation::Outcome::Failed => "fail",
                    };
                    let (answer, truncated) = Service::bounded_answer(&turn.answer);
                    let mut text = format!(
                        "{canonical} agent {state} rev={revision} turn=t:{} outcome={outcome}",
                        turn.id
                    );
                    if !answer.is_empty() {
                        text.push('\n');
                        text.push_str(&answer);
                    }
                    if truncated {
                        text.push_str("\n[answer truncated]");
                    }
                    ToolOutput::text(text)
                }
                Err(error) => ToolOutput::error(format!(
                    "{canonical} agent blocked rev={revision} initial_turn_error={error}"
                )),
            };

            Service::spawn_stable_idle_worker(
                self.service.inner.clone(),
                prepared,
                runtime_state,
                active,
            )?;
            Ok(response)
        })
    }
}

impl Tool for Service {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "subagent".into(),
            description: "Delegate one bounded task to an isolated durable child Agent Session. The trusted host may preconfigure an immutable bounded child-routing strategy, but you cannot choose provider, model, route, root, actor, budget, depth, or additional authority. Foreground waits for the result; background returns the child Session immediately.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task": {"type": "string"},
                    "title": {"type": "string"},
                    "mode": {
                        "type": "string",
                        "enum": ["foreground", "background"],
                        "default": "foreground"
                    }
                },
                "required": ["task"],
                "additionalProperties": false
            }),
            effect: ToolEffect::Process,
        }
    }

    fn validate(&self, arguments: &Value) -> Result<()> {
        Self::validate_args(arguments).map(|_| ())
    }

    fn execute<'a>(&'a self, ctx: ToolContext, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args = Self::validate_args(arguments)?;
            // Revalidate the exact selected identity before creating any child state.
            self.exact_selection(&ctx)?;
            match args.mode {
                ModeArg::Foreground => {
                    let prepared = self.prepare(&ctx, args, SubagentMode::Foreground, true, None, None, true, None).await?;
                    Self::run_prepared(self.inner.clone(), prepared, ctx.cancellation).await
                }
                ModeArg::Background => {
                    let active = Self::reserve_background(&self.inner)?;
                    let prepared = self.prepare(&ctx, args, SubagentMode::Background, true, None, None, true, None).await?;
                    let response = json!({
                        "subagent_id": prepared.info.id,
                        "session_id": prepared.info.child_session_id,
                        "state": "running",
                    })
                    .to_string();
                    let inner = self.inner.clone();
                    let spawn = thread::Builder::new()
                        .name(format!(
                            "aw-subagent-{}",
                            prepared.info.id.chars().rev().take(8).collect::<String>()
                        ))
                        .spawn(move || {
                            let _active = active;
                            let runtime = tokio::runtime::Builder::new_current_thread()
                                .enable_all()
                                .max_blocking_threads(4)
                                .build();
                            match runtime {
                                Ok(runtime) => {
                                    let _ = runtime.block_on(Self::run_prepared(
                                        inner,
                                        prepared,
                                        CancellationToken::new(),
                                    ));
                                }
                                Err(error) => {
                                    let actor = prepared.info.identity.actor.clone();
                                    let detail = format!("cannot start subagent runtime: {error}");
                                    let _ = inner.store.settle_subagent(
                                        &actor,
                                        &prepared.info.id,
                                        "failed",
                                        Some(&detail),
                                    );
                                }
                            }
                        });
                    if let Err(error) = spawn {
                        return Err(Error::Io(error));
                    }
                    Ok(ToolOutput::text(response))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw::config;
    use aw_agent_core::{
        AccountDescriptor, CompletionFuture, CompletionMeta, DeltaSink, InvocationScope, JournalEntry,
        PrincipalId, Provider, ToolCall, ToolContext, ToolEffect, ToolRecord, ToolStatus,
        TurnJournal, TurnStart, Usage,
    };
    use aw_agent_core::{Message, ModelFuture, ModelReply, Role};
    #[cfg(feature = "workspace")]
    use std::process::Command;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };




    struct GraphDelegateProvider {
        main_calls: Arc<AtomicUsize>,
        delegate_calls: Arc<AtomicUsize>,
    }

    impl Provider for GraphDelegateProvider {
        fn supports_tools(&self) -> bool {
            true
        }

        fn stream<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            _sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("Graph delegate fixture requires tool protocol")) })
        }

        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            _model: &'a str,
            tools: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                let delegated = messages.iter().any(|message| {
                    message
                        .content
                        .contains("operation-scoped execution delegate")
                });
                if delegated {
                    self.delegate_calls.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(
                        tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>(),
                        vec!["read"]
                    );
                    if messages.last().is_some_and(|message| message.role == Role::Tool) {
                        sink("delegate prose is not the node result")?;
                        return Ok(ModelReply {
                            meta: CompletionMeta {
                                finish_reason: "stop".into(),
                                usage: Usage {
                                    input_tokens: Some(3),
                                    output_tokens: Some(2),
                                },
                            },
                            ..Default::default()
                        });
                    }
                    return Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            usage: Usage {
                                input_tokens: Some(3),
                                output_tokens: Some(1),
                            },
                        },
                        tool_calls: vec![ToolCall {
                            id: "delegate-graph-node".into(),
                            name: "read".into(),
                            arguments: json!({}),
                        }],
                        provider_context: None,
                    });
                }

                self.main_calls.fetch_add(1, Ordering::SeqCst);
                assert!(tools.iter().any(|tool| tool.name == "batch"));
                if messages.last().is_some_and(|message| message.role == Role::Tool) {
                    sink("delegated graph complete")?;
                    return Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(4),
                                output_tokens: Some(2),
                            },
                        },
                        ..Default::default()
                    });
                }
                Ok(ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "tool_calls".into(),
                        usage: Usage {
                            input_tokens: Some(4),
                            output_tokens: Some(1),
                        },
                    },
                    tool_calls: vec![ToolCall {
                        id: "delegate-node-batch".into(),
                        name: "batch".into(),
                        arguments: json!({
                            "ops":[
                                ["read",{"path":"delegated.txt","numbered":false},"*"]
                            ]
                        }),
                    }],
                    provider_context: None,
                })
            })
        }
    }

    struct GraphAskProvider {
        main_calls: Arc<AtomicUsize>,
        guardian_calls: Arc<AtomicUsize>,
    }

    impl GraphAskProvider {
        fn approval_scope(messages: &[Message]) -> Result<String> {
            let prompt = messages
                .iter()
                .rev()
                .find(|message| message.role == Role::User)
                .map(|message| message.content.as_str())
                .ok_or(Error::Protocol("Graph guardian prompt missing"))?;
            let marker = "approval_scope exactly \"";
            let rest = prompt
                .split_once(marker)
                .map(|(_, rest)| rest)
                .ok_or(Error::Protocol("Graph guardian approval scope missing"))?;
            rest.split_once('"')
                .map(|(scope, _)| scope.to_owned())
                .filter(|scope| !scope.is_empty())
                .ok_or(Error::Protocol("Graph guardian approval scope invalid"))
        }
    }

    impl Provider for GraphAskProvider {
        fn supports_tools(&self) -> bool {
            true
        }

        fn stream<'a>(
            &'a self,
            messages: &'a [Message],
            _model: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                self.guardian_calls.fetch_add(1, Ordering::SeqCst);
                let scope = Self::approval_scope(messages)?;
                sink(&serde_json::json!({
                    "decision":"ask",
                    "route":"local",
                    "approval_scope":scope,
                }).to_string())?;
                Ok(CompletionMeta {
                    finish_reason: "stop".into(),
                    usage: Usage {
                        input_tokens: Some(6),
                        output_tokens: Some(4),
                    },
                })
            })
        }

        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            _model: &'a str,
            tools: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                if tools.is_empty() {
                    self.guardian_calls.fetch_add(1, Ordering::SeqCst);
                    let scope = Self::approval_scope(messages)?;
                    sink(&serde_json::json!({
                        "decision":"ask",
                        "route":"local",
                        "approval_scope":scope,
                    }).to_string())?;
                    return Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(6),
                                output_tokens: Some(4),
                            },
                        },
                        ..Default::default()
                    });
                }
                self.main_calls.fetch_add(1, Ordering::SeqCst);
                assert!(tools.iter().any(|tool| tool.name == "batch"));
                if messages.last().is_some_and(|message| message.role == Role::Tool) {
                    sink("graph complete")?;
                    return Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(3),
                                output_tokens: Some(2),
                            },
                        },
                        ..Default::default()
                    });
                }
                Ok(ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "tool_calls".into(),
                        usage: Usage {
                            input_tokens: Some(4),
                            output_tokens: Some(1),
                        },
                    },
                    tool_calls: vec![ToolCall {
                        id: "graph-ask-batch".into(),
                        name: "batch".into(),
                        arguments: json!({
                            "ops":[
                                ["write",{"path":"approved-one.txt","content":"one"},"*"],
                                ["write",{"path":"approved-two.txt","content":"two"},"*"]
                            ]
                        }),
                    }],
                    provider_context: None,
                })
            })
        }
    }

    struct GraphNarrowProvider {
        main_calls: Arc<AtomicUsize>,
        guardian_calls: Arc<AtomicUsize>,
    }

    impl GraphNarrowProvider {
        fn narrow_ref(messages: &[Message]) -> Result<String> {
            let prompt = messages
                .iter()
                .rev()
                .find(|message| message.role == Role::User)
                .map(|message| message.content.as_str())
                .ok_or(Error::Protocol("Graph narrow guardian prompt missing"))?;
            let marker = "narrow_ref exactly \"";
            let rest = prompt
                .split_once(marker)
                .map(|(_, rest)| rest)
                .ok_or(Error::Protocol("Graph narrow ref missing from guardian prompt"))?;
            rest.split_once('"')
                .map(|(reference, _)| reference.to_owned())
                .filter(|reference| !reference.is_empty())
                .ok_or(Error::Protocol("Graph narrow ref is invalid"))
        }

        fn guardian_reply(
            &self,
            messages: &[Message],
            sink: &mut DeltaSink<'_>,
        ) -> Result<CompletionMeta> {
            self.guardian_calls.fetch_add(1, Ordering::SeqCst);
            let narrow_ref = Self::narrow_ref(messages)?;
            sink(
                &serde_json::json!({
                    "decision":"narrow",
                    "route":"local",
                    "narrow_ref":narrow_ref,
                })
                .to_string(),
            )?;
            Ok(CompletionMeta {
                finish_reason: "stop".into(),
                usage: Usage {
                    input_tokens: Some(5),
                    output_tokens: Some(3),
                },
            })
        }
    }

    impl Provider for GraphNarrowProvider {
        fn supports_tools(&self) -> bool {
            true
        }

        fn stream<'a>(
            &'a self,
            messages: &'a [Message],
            _model: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move { self.guardian_reply(messages, sink) })
        }

        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            _model: &'a str,
            tools: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                if tools.is_empty() {
                    let meta = self.guardian_reply(messages, sink)?;
                    return Ok(ModelReply {
                        meta,
                        ..Default::default()
                    });
                }
                self.main_calls.fetch_add(1, Ordering::SeqCst);
                assert!(tools.iter().any(|tool| tool.name == "batch"));
                if messages.last().is_some_and(|message| message.role == Role::Tool) {
                    let result = messages.last().unwrap();
                    assert_eq!(result.tool_call_id.as_deref(), Some("graph-narrow-batch"));
                    assert!(!result.content.is_empty());
                    sink("narrow graph complete")?;
                    return Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(3),
                                output_tokens: Some(2),
                            },
                        },
                        ..Default::default()
                    });
                }
                Ok(ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "tool_calls".into(),
                        usage: Usage {
                            input_tokens: Some(4),
                            output_tokens: Some(1),
                        },
                    },
                    tool_calls: vec![ToolCall {
                        id: "graph-narrow-batch".into(),
                        name: "batch".into(),
                        arguments: json!({
                            "ops":[[
                                "write",
                                {"path":"unsafe.txt","content":"narrowed-value"},
                                "*"
                            ]]
                        }),
                    }],
                    provider_context: None,
                })
            })
        }
    }

    struct StaticGraphNarrowAuthority {
        calls: Arc<AtomicUsize>,
    }

    impl GraphNarrowAuthority for StaticGraphNarrowAuthority {
        fn proposal(
            &self,
            _ctx: &ToolContext,
            node: &GraphPresealNode,
        ) -> Result<Option<GraphNarrowProposal>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if node.coordinate != 1 || node.operation != OperationId::Fs(FsTool::Write) {
                return Ok(None);
            }
            assert_eq!(
                node.arguments.get("path").and_then(Value::as_str),
                Some("unsafe.txt")
            );
            Ok(Some(GraphNarrowProposal {
                narrow_ref: "narrow:graph-safe-path-v1".into(),
                arguments: json!({
                    "path":"safe.txt",
                    "content":"narrowed-value"
                }),
            }))
        }
    }

    struct WholeGraphDelegateProvider {
        parent_calls: Arc<AtomicUsize>,
        delegate_calls: Arc<AtomicUsize>,
    }

    impl Provider for WholeGraphDelegateProvider {
        fn supports_tools(&self) -> bool {
            true
        }

        fn stream<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            _sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async {
                Err(Error::Protocol(
                    "whole-Graph delegate fixture requires tool protocol",
                ))
            })
        }

        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            model: &'a str,
            tools: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                assert_eq!(model, "fixture-model");
                let is_delegate = messages.iter().any(|message| {
                    message
                        .content
                        .contains("operation-scoped execution delegate for batch")
                });
                if is_delegate {
                    self.delegate_calls.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(
                        tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>(),
                        vec!["batch"],
                        "whole-Graph coordinator gets exactly one opaque original-execution gate"
                    );
                    if messages.last().is_some_and(|message| message.role == Role::Tool) {
                        let tool_result = messages.last().unwrap();
                        assert_eq!(
                            tool_result.tool_call_id.as_deref(),
                            Some("delegate-whole-graph")
                        );
                        assert!(
                            !tool_result.content.is_empty(),
                            "delegate receives one factual original Batch receipt"
                        );
                        sink("delegate prose must never replace the Graph receipt")?;
                        return Ok(ModelReply {
                            meta: CompletionMeta {
                                finish_reason: "stop".into(),
                                usage: Usage {
                                    input_tokens: Some(4),
                                    output_tokens: Some(2),
                                },
                            },
                            ..Default::default()
                        });
                    }
                    return Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            usage: Usage {
                                input_tokens: Some(4),
                                output_tokens: Some(1),
                            },
                        },
                        tool_calls: vec![ToolCall {
                            id: "delegate-whole-graph".into(),
                            name: "batch".into(),
                            arguments: json!({}),
                        }],
                        provider_context: None,
                    });
                }

                self.parent_calls.fetch_add(1, Ordering::SeqCst);
                assert!(tools.iter().any(|tool| tool.name == "batch"));
                if messages.last().is_some_and(|message| message.role == Role::Tool) {
                    let result = messages.last().unwrap();
                    assert_eq!(
                        result.tool_call_id.as_deref(),
                        Some("whole-graph-batch")
                    );
                    assert!(!result.content.is_empty());
                    assert!(!result
                        .content
                        .contains("delegate prose must never replace the Graph receipt"));
                    sink("whole graph parent done")?;
                    return Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(4),
                                output_tokens: Some(2),
                            },
                        },
                        ..Default::default()
                    });
                }

                Ok(ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "tool_calls".into(),
                        usage: Usage {
                            input_tokens: Some(4),
                            output_tokens: Some(1),
                        },
                    },
                    tool_calls: vec![ToolCall {
                        id: "whole-graph-batch".into(),
                        name: "batch".into(),
                        arguments: json!({
                            "ops":[[
                                "write",
                                {"path":"whole-graph.txt","content":"one-job"},
                                "*"
                            ]]
                        }),
                    }],
                    provider_context: None,
                })
            })
        }
    }

    struct PolicyDelegateProvider {
        calls: Arc<AtomicUsize>,
    }
    impl Provider for PolicyDelegateProvider {
        fn supports_tools(&self) -> bool {
            true
        }

        fn stream<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            _sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("policy delegate fixture requires tool protocol")) })
        }

        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            _model: &'a str,
            tools: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                assert!(messages.iter().any(|message| {
                    message
                        .content
                        .contains("operation-scoped execution delegate")
                }));
                assert_eq!(
                    tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>(),
                    vec!["read"]
                );
                if messages.last().is_some_and(|message| message.role == Role::Tool) {
                    assert_eq!(
                        messages.last().unwrap().tool_call_id.as_deref(),
                        Some("delegate-original")
                    );
                    sink("delegate prose must not replace the native receipt")?;
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(4),
                                output_tokens: Some(2),
                            },
                        },
                        ..Default::default()
                    })
                } else {
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            usage: Usage {
                                input_tokens: Some(4),
                                output_tokens: Some(1),
                            },
                        },
                        tool_calls: vec![ToolCall {
                            id: "delegate-original".into(),
                            name: "read".into(),
                            arguments: json!({}),
                        }],
                        provider_context: None,
                    })
                }
            })
        }
    }

    #[derive(Clone)]
    struct FactualReadTool {
        calls: Arc<AtomicUsize>,
    }
    impl Tool for FactualReadTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "read".into(),
                description: "fixture native read".into(),
                parameters: json!({
                    "type":"object",
                    "properties":{"path":{"type":"string"}},
                    "required":["path"],
                    "additionalProperties":false
                }),
                effect: ToolEffect::Read,
            }
        }

        fn validate(&self, arguments: &Value) -> Result<()> {
            arguments
                .get("path")
                .and_then(Value::as_str)
                .filter(|path| !path.is_empty())
                .map(|_| ())
                .ok_or(Error::Denied("fixture read needs path"))
        }

        fn execute<'a>(&'a self, _ctx: ToolContext, arguments: &'a Value) -> ToolFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(ToolOutput::text(format!(
                    "native:{}",
                    arguments["path"].as_str().unwrap_or_default()
                )))
            })
        }
    }

    struct EchoProvider {
        calls: Arc<AtomicUsize>,
    }
    impl Provider for EchoProvider {
        fn stream<'a>(
            &'a self,
            messages: &'a [aw_agent_core::Message],
            model: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                assert_eq!(model, "fixture-model");
                assert!(messages.iter().any(|message| message.content == "research task"));
                assert!(!messages.iter().any(|message| message.content == "parent secret text"));
                self.calls.fetch_add(1, Ordering::SeqCst);
                sink("child answer")?;
                Ok(CompletionMeta {
                    finish_reason: "stop".into(),
                    usage: Usage {
                        input_tokens: Some(3),
                        output_tokens: Some(2),
                    },
                })
            })
        }
    }

    struct GraphAgentProvider {
        parent_calls: Arc<AtomicUsize>,
        child_calls: Arc<AtomicUsize>,
        grandchild_calls: Arc<AtomicUsize>,
    }
    impl Provider for GraphAgentProvider {
        fn supports_tools(&self) -> bool {
            true
        }

        fn stream<'a>(
            &'a self,
            messages: &'a [Message],
            model: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                assert_eq!(model, "fixture-model");
                assert!(messages
                    .iter()
                    .any(|message| message.content == "level2 graph child"));
                self.grandchild_calls.fetch_add(1, Ordering::SeqCst);
                sink("level2 graph answer")?;
                Ok(CompletionMeta {
                    finish_reason: "stop".into(),
                    usage: Usage {
                        input_tokens: Some(2),
                        output_tokens: Some(1),
                    },
                })
            })
        }

        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            model: &'a str,
            tools: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                assert_eq!(model, "fixture-model");
                if messages
                    .iter()
                    .any(|message| message.content == "level2 graph child")
                {
                    self.grandchild_calls.fetch_add(1, Ordering::SeqCst);
                    sink("level2 graph answer")?;
                    return Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(2),
                                output_tokens: Some(1),
                            },
                        },
                        ..Default::default()
                    });
                }

                if messages
                    .iter()
                    .any(|message| message.content == "level1 graph child")
                {
                    self.child_calls.fetch_add(1, Ordering::SeqCst);
                    assert!(
                        tools.iter().any(|tool| tool.name == "agent"),
                        "depth-1 child must inherit the same bounded Agent constructor"
                    );
                    if messages.last().is_some_and(|message| message.role == Role::Tool) {
                        let result = messages.last().unwrap();
                        assert_eq!(result.tool_call_id.as_deref(), Some("nested-agent"));
                        assert!(result.content.contains("level2 graph answer"));
                        sink("level1 graph answer")?;
                        return Ok(ModelReply {
                            meta: CompletionMeta {
                                finish_reason: "stop".into(),
                                usage: Usage {
                                    input_tokens: Some(4),
                                    output_tokens: Some(2),
                                },
                            },
                            ..Default::default()
                        });
                    }
                    return Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            usage: Usage {
                                input_tokens: Some(3),
                                output_tokens: Some(1),
                            },
                        },
                        tool_calls: vec![ToolCall {
                            id: "nested-agent".into(),
                            name: "agent".into(),
                            arguments: json!({
                                "task":"level2 graph child",
                                "title":"Nested graph child",
                                "background":false
                            }),
                        }],
                        provider_context: None,
                    });
                }

                self.parent_calls.fetch_add(1, Ordering::SeqCst);
                let batch = tools
                    .iter()
                    .find(|tool| tool.name == "batch")
                    .expect("parent must expose Batch");
                let node_names = batch.parameters["properties"]["ops"]["items"]["prefixItems"][0]
                    ["enum"]
                    .as_array()
                    .expect("Batch node enum");
                assert!(
                    node_names.iter().any(|name| name == "agent"),
                    "Batch schema must advertise Agent only for the qualified host"
                );
                assert!(
                    !node_names.iter().any(|name| name == "batch"),
                    "nested batch must stay absent"
                );
                if messages.last().is_some_and(|message| message.role == Role::Tool) {
                    let result = messages.last().unwrap();
                    assert_eq!(result.tool_call_id.as_deref(), Some("graph-agent-batch"));
                    assert!(result.content.contains("level1 graph answer"));
                    sink("parent graph answer")?;
                    return Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(4),
                                output_tokens: Some(2),
                            },
                        },
                        ..Default::default()
                    });
                }

                Ok(ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "tool_calls".into(),
                        usage: Usage {
                            input_tokens: Some(4),
                            output_tokens: Some(1),
                        },
                    },
                    tool_calls: vec![ToolCall {
                        id: "graph-agent-batch".into(),
                        name: "batch".into(),
                        arguments: json!({
                            "ops":[[
                                "agent",
                                {
                                    "task":"level1 graph child",
                                    "title":"Graph child",
                                    "background":false
                                },
                                "*"
                            ]]
                        }),
                    }],
                    provider_context: None,
                })
            })
        }
    }

    /// Never completes on its own; only an authenticated Actor Turn cancel
    /// can finish this real background Agent provider attempt.
    struct BlockingProvider {
        calls: Arc<AtomicUsize>,
        entered: Arc<tokio::sync::Notify>,
    }
    impl Provider for BlockingProvider {
        fn stream<'a>(
            &'a self, messages:&'a [aw_agent_core::Message], _:&'a str,
            _:&'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                assert!(messages.iter().any(|m|m.content=="background task"));
                assert!(!messages.iter().any(|m|m.content=="parent secret text"));
                self.calls.fetch_add(1,Ordering::SeqCst);
                self.entered.notify_one();
                std::future::pending::<Result<CompletionMeta>>().await
            })
        }
    }

    struct RecursiveProvider {
        calls: Arc<AtomicUsize>,
    }
    impl Provider for RecursiveProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            messages: &'a [Message],
            model: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                assert_eq!(model, "fixture-model");
                assert!(messages.iter().any(|message| message.content == "level2 task"));
                self.calls.fetch_add(1, Ordering::SeqCst);
                sink("level2 answer")?;
                Ok(CompletionMeta {
                    finish_reason: "stop".into(),
                    usage: Usage {
                        input_tokens: Some(2),
                        output_tokens: Some(1),
                    },
                })
            })
        }
        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            model: &'a str,
            tools: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                assert_eq!(model, "fixture-model");
                let names = tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>();
                if names.is_empty() {
                    assert!(messages.iter().any(|message| message.content == "level2 task"));
                    self.calls.fetch_add(1, Ordering::SeqCst);
                    sink("level2 answer")?;
                    return Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(2),
                                output_tokens: Some(1),
                            },
                        },
                        ..Default::default()
                    });
                }
                assert_eq!(names, vec!["subagent"]);
                assert!(messages.iter().any(|message| message.content == "level1 task"));
                self.calls.fetch_add(1, Ordering::SeqCst);
                if messages.last().is_some_and(|message| message.role == Role::Tool) {
                    let result = messages.last().unwrap();
                    assert_eq!(result.tool_call_id.as_deref(), Some("recursive-leaf"));
                    let body: serde_json::Value = serde_json::from_str(&result.content).unwrap();
                    assert_eq!(body["state"], "completed");
                    assert_eq!(body["answer"], "level2 answer");
                    sink("level1 answer")?;
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(4),
                                output_tokens: Some(2),
                            },
                        },
                        ..Default::default()
                    })
                } else {
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            usage: Usage {
                                input_tokens: Some(3),
                                output_tokens: Some(1),
                            },
                        },
                        tool_calls: vec![ToolCall {
                            id: "recursive-leaf".into(),
                            name: "subagent".into(),
                            arguments: json!({
                                "task": "level2 task",
                                "title": "Leaf child",
                                "mode": "foreground"
                            }),
                        }],
                        provider_context: None,
                    })
                }
            })
        }
    }

    #[cfg(feature = "workspace")]
    struct RecursiveWorkspaceProvider {
        calls: Arc<AtomicUsize>,
    }
    #[cfg(feature = "workspace")]
    impl Provider for RecursiveWorkspaceProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("recursive workspace fixture uses tool adapter")) })
        }
        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            model: &'a str,
            tools: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                assert_eq!(model, "fixture-model");
                self.calls.fetch_add(1, Ordering::SeqCst);
                let names = tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>();
                let parent_task = messages.iter().any(|m| m.content == "workspace-parent task");
                let leaf_task = messages.iter().any(|m| m.content == "workspace-leaf task");
                if parent_task {
                    assert_eq!(names, vec!["edit", "read", "subagent", "write"]);
                    if messages.last().is_some_and(|message| message.role == Role::Tool) {
                        let result = messages.last().unwrap();
                        assert_eq!(result.tool_call_id.as_deref(), Some("workspace-recursive-leaf"));
                        let body: serde_json::Value = serde_json::from_str(&result.content).unwrap();
                        assert_eq!(body["state"], "completed");
                        assert_eq!(body["answer"], "workspace-leaf answer");
                        sink("workspace-parent answer")?;
                        return Ok(ModelReply {
                            meta: CompletionMeta {
                                finish_reason: "stop".into(),
                                usage: Usage {
                                    input_tokens: Some(5),
                                    output_tokens: Some(2),
                                },
                            },
                            ..Default::default()
                        });
                    }
                    return Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            usage: Usage {
                                input_tokens: Some(4),
                                output_tokens: Some(1),
                            },
                        },
                        tool_calls: vec![ToolCall {
                            id: "workspace-recursive-leaf".into(),
                            name: "subagent".into(),
                            arguments: json!({
                                "task":"workspace-leaf task",
                                "title":"Workspace leaf",
                                "mode":"foreground"
                            }),
                        }],
                        provider_context: None,
                    });
                }
                assert!(leaf_task);
                assert_eq!(names, vec!["edit", "read", "write"]);
                if messages.last().is_some_and(|message| message.role == Role::Tool) {
                    let result = messages.last().unwrap();
                    assert_eq!(result.tool_call_id.as_deref(), Some("workspace-leaf-write"));
                    assert!(!result.content.starts_with("ERROR:"));
                    sink("workspace-leaf answer")?;
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(3),
                                output_tokens: Some(2),
                            },
                        },
                        ..Default::default()
                    })
                } else {
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            usage: Usage {
                                input_tokens: Some(3),
                                output_tokens: Some(1),
                            },
                        },
                        tool_calls: vec![ToolCall {
                            id: "workspace-leaf-write".into(),
                            name: "write".into(),
                            arguments: json!({
                                "path":"grandchild-only.txt",
                                "content":"grandchild isolated bytes\\n"
                            }),
                        }],
                        provider_context: None,
                    })
                }
            })
        }
    }

    #[cfg(feature = "workspace")]
    struct DirtyRecursiveWorkspaceProvider {
        calls: Arc<AtomicUsize>,
    }
    #[cfg(feature = "workspace")]
    impl Provider for DirtyRecursiveWorkspaceProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("dirty recursive fixture uses tool adapter")) })
        }
        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            _: &'a str,
            tools: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                assert!(messages.iter().any(|m| m.content == "dirty-parent task"));
                let names = tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>();
                assert_eq!(names, vec!["edit", "read", "subagent", "write"]);
                let last_tool = messages
                    .last()
                    .filter(|message| message.role == Role::Tool)
                    .and_then(|message| message.tool_call_id.as_deref());
                match last_tool {
                    None => Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            ..Default::default()
                        },
                        tool_calls: vec![ToolCall {
                            id: "dirty-parent-write".into(),
                            name: "write".into(),
                            arguments: json!({
                                "path":"dirty-before-delegate.txt",
                                "content":"dirty parent bytes\\n"
                            }),
                        }],
                        provider_context: None,
                    }),
                    Some("dirty-parent-write") => Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            ..Default::default()
                        },
                        tool_calls: vec![ToolCall {
                            id: "dirty-parent-subagent".into(),
                            name: "subagent".into(),
                            arguments: json!({
                                "task":"must never reach descendant provider",
                                "mode":"foreground"
                            }),
                        }],
                        provider_context: None,
                    }),
                    Some("dirty-parent-subagent") => {
                        let result = messages.last().unwrap();
                        assert!(result.content.contains("clean parent checkout"));
                        sink("dirty descendant correctly blocked")?;
                        Ok(ModelReply {
                            meta: CompletionMeta {
                                finish_reason: "stop".into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        })
                    }
                    other => panic!("unexpected tool result {other:?}"),
                }
            })
        }
    }

    #[cfg(feature = "workspace")]
    struct WorkspaceToolProvider {
        calls: Arc<AtomicUsize>,
    }
    #[cfg(feature = "workspace")]
    impl Provider for WorkspaceToolProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("workspace fixture requires tool adapter")) })
        }
        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            model: &'a str,
            tools: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                assert_eq!(model, "fixture-model");
                let names = tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>();
                assert_eq!(names, vec!["edit", "read", "write"]);
                assert!(!names.contains(&"subagent"));
                assert!(!names.contains(&"bash"));
                self.calls.fetch_add(1, Ordering::SeqCst);
                if messages.last().is_some_and(|message| message.role == Role::Tool) {
                    let result = messages.last().unwrap();
                    assert_eq!(result.tool_call_id.as_deref(), Some("child-write"));
                    assert!(!result.content.starts_with("ERROR:"));
                    sink("child workspace answer")?;
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(4),
                                output_tokens: Some(2),
                            },
                        },
                        ..Default::default()
                    })
                } else {
                    assert!(messages
                        .iter()
                        .any(|message| message.content == "workspace task"));
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            ..Default::default()
                        },
                        tool_calls: vec![ToolCall {
                            id: "child-write".into(),
                            name: "write".into(),
                            arguments: json!({
                                "path": "child-created.txt",
                                "content": "created only in child worktree\\n"
                            }),
                        }],
                        provider_context: None,
                    })
                }
            })
        }
    }

    #[cfg(feature = "workspace")]
    struct ReadOnlyToolProvider;
    #[cfg(feature = "workspace")]
    impl Provider for ReadOnlyToolProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("read-only fixture requires tool adapter")) })
        }
        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            _: &'a str,
            tools: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                let names = tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>();
                assert_eq!(names, vec!["read"]);
                if messages.last().is_some_and(|message| message.role == Role::Tool) {
                    assert!(messages.last().unwrap().content.contains("parent bytes"));
                    sink("read-only child answer")?;
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                } else {
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            ..Default::default()
                        },
                        tool_calls: vec![ToolCall {
                            id: "child-read".into(),
                            name: "read".into(),
                            arguments: json!({"path":"base.txt","offset":1,"limit":20}),
                        }],
                        provider_context: None,
                    })
                }
            })
        }
    }

    #[cfg(all(unix, feature = "workspace"))]
    struct GitFixture {
        root: PathBuf,
        repo: PathBuf,
        state: PathBuf,
        git: PathBuf,
    }
    #[cfg(all(unix, feature = "workspace"))]
    impl GitFixture {
        fn new() -> Self {
            use std::os::unix::fs::PermissionsExt;
            static SERIAL: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "aw-subagent-worktree-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
            let repo = root.join("repo");
            std::fs::create_dir(&repo).unwrap();
            let state = root.join("state");
            let git = std::env::var_os("AW_GIT")
                .map(PathBuf::from)
                .or_else(|| option_env!("AW_DEFAULT_GIT").map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("/usr/bin/git"));
            let run = |args: &[&str]| {
                let output = Command::new(&git)
                    .current_dir(&repo)
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .args(args)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "git {args:?}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            };
            run(&["init", "--initial-branch=main"]);
            run(&["config", "user.name", "Fixture"]);
            run(&["config", "user.email", "fixture@example.invalid"]);
            std::fs::write(repo.join("base.txt"), b"parent bytes\\n").unwrap();
            run(&["add", "base.txt"]);
            run(&["commit", "-m", "fixture", "--no-gpg-sign"]);
            Self {
                root,
                repo: repo.canonicalize().unwrap(),
                state,
                git: git.canonicalize().unwrap(),
            }
        }
    }
    #[cfg(all(unix, feature = "workspace"))]
    impl Drop for GitFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn binding() -> aw_store::SessionBinding {
        aw_store::SessionBinding {
            working_directory: PathBuf::from("/tmp"),
            tool_root: PathBuf::from("/tmp"),
            system_prompt: "parent instructions".into(),
            profile_hint: "fixture-account".into(),
            model_hint: "fixture-model".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    /// Opt-in host Actor test bridge. The original SQLite parent Session is
    /// still authoritative; the root Runtime adds no provider or tool grant.
    fn actor_owner(
        store: &SessionStore,
        owner: &PrincipalId,
        root_session: &str,
        max_depth: u8,
    ) -> crate::child_actors::Service {
        actor_owner_for_mode(store, owner, root_session, max_depth, ToolMode::Off)
    }
    fn actor_owner_for_mode(
        store: &SessionStore,
        owner: &PrincipalId,
        root_session: &str,
        max_depth: u8,
        mode: ToolMode,
    ) -> crate::child_actors::Service {
        actor_owner_for_mode_with_extra(store, owner, root_session, max_depth, mode, &[])
    }

    fn actor_owner_for_mode_with_extra(
        store: &SessionStore,
        owner: &PrincipalId,
        root_session: &str,
        max_depth: u8,
        mode: ToolMode,
        extra_grants: &[(&str, aw_agent_kernel::capability::Effect)],
    ) -> crate::child_actors::Service {
        use aw_agent_core::runtime::{HostRuntime, SessionBinding};
        use aw_agent_kernel::{
            actor::{ActorState, RuntimeKey},
            capability::{Effect, SourceKind},
            envelope::{Access, Budget, EnvelopeSpec, ExactGrant, Network},
        };
        use std::{num::NonZeroU64, sync::Mutex};
        let mut grants = vec![ExactGrant {
            name: "subagent".into(),
            kind: SourceKind::Host,
            source: "trusted-host".into(),
            source_epoch: 1,
            effect: Effect::Process,
        }];
        let extra: &[(&str, Effect)] = match mode {
            ToolMode::Off => &[],
            ToolMode::ReadOnly => &[("read", Effect::Read)],
            ToolMode::Workspace => &[
                ("read", Effect::Read),
                ("write", Effect::Write),
                ("edit", Effect::Write),
            ],
            ToolMode::Trusted => panic!("fixture never issues trusted Bash"),
        };
        for (name, effect) in extra.iter().chain(extra_grants.iter()) {
            grants.push(ExactGrant {
                name: (*name).into(),
                kind: SourceKind::Host,
                source: "trusted-host".into(),
                source_epoch: 1,
                effect: *effect,
            });
        }
        let nz = |value| NonZeroU64::new(value).unwrap();
        let mut runtime = HostRuntime::new(
            owner.clone(),
            "local",
            RuntimeKey {
                id: nz(81),
                generation: nz(1),
            },
            16,
            16,
        )
        .unwrap();
        let root = runtime
            .bind_agent(
                owner,
                SessionBinding::from_trusted_host(root_session, None).unwrap(),
                None,
                EnvelopeSpec {
                    actor: owner.as_str().into(),
                    site: "local".into(),
                    revision: 1,
                    access: Access::ReadWrite,
                    network: Network::Deny,
                    process: true,
                    tty: false,
                    microphone: false,
                    speaker: false,
                    budget: Budget {
                        max_model_rounds: 8,
                        max_tool_calls: 128,
                        max_children: 4,
                        max_depth,
                        max_output_bytes: 1024 * 1024,
                    },
                    grants,
                },
                "root Agent",
            )
            .unwrap();
        runtime
            .record_state(owner, root, None, ActorState::Running)
            .unwrap();
        crate::child_actors::Service::new(
            store.clone(),
            Arc::new(Mutex::new(runtime)),
            owner.clone(),
            root,
            None,
            root_session,
        )
        .unwrap()
    }

    #[test]
    fn child_tool_runtime_is_host_owned_and_narrowing() {
        let mut limits = LocalToolLimits::default();
        limits.schema.text_write_bytes = 1024;
        limits.jobs.max_active_nodes = 2;
        let concurrency = LocalToolConcurrency {
            graph_width: 2,
            parallelism: aw_agent_core::ToolParallelism {
                max_parallel_reads: 1,
                allow_parallel_shell: false,
                max_parallel_shell: 1,
            },
        };
        let config = ServiceConfig::new(
            ToolMode::ReadOnly,
            BudgetConfig::default(),
            PathBuf::from("/tmp"),
            None,
        )
        .with_child_tool_runtime(concurrency, limits)
        .unwrap();
        assert_eq!(config.child_tool_concurrency, concurrency);
        assert_eq!(config.child_tool_limits, limits);

        let mut widened = LocalToolLimits::default();
        widened.schema.text_write_bytes += 1;
        assert!(ServiceConfig::new(
            ToolMode::ReadOnly,
            BudgetConfig::default(),
            PathBuf::from("/tmp"),
            None,
        )
        .with_child_tool_runtime(concurrency, widened)
        .is_ok(), "read-only child must ignore dormant write limits");
        assert!(ServiceConfig::new(
            ToolMode::Workspace,
            BudgetConfig::default(),
            PathBuf::from("/tmp"),
            None,
        )
        .with_child_tool_runtime(concurrency, widened)
        .is_err(), "workspace child exposes write and must reject widening");
    }

    fn settings() -> Settings {
        let mut profile = config::builtins().remove("openai").unwrap();
        profile.model = Some("fixture-model".into());
        Settings {
            profile_name: "fixture-account".into(),
            profile,
            system_prompt: "parent instructions".into(),
            request_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(5),
            max_output_bytes: aw::config::MAX_OUTPUT_BYTES,
        }
    }

    #[tokio::test]
    async fn typed_routing_planner_and_guardian_step_gate_use_same_parent_actor() {
        let owner = PrincipalId::new("local:decision-owner").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    owner.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                Arc::new(EchoProvider {
                    calls: calls.clone(),
                }),
            )
            .unwrap();
        let selected = accounts
            .select(&owner, "fixture-account", "fixture-model")
            .unwrap();
        let store = SessionStore::in_memory().unwrap();
        let (root, branch) = store.create(&owner, "Root", binding()).unwrap();
        let bridge = actor_owner(&store, &owner, &root.id, 2);
        let context = ToolContext {
            actor: owner.clone(),
            turn_id: 1,
            call_id: "host-direct".into(),
            source: aw_agent_core::ToolSource::trusted_host(),
            effect: aw_agent_core::ToolEffect::Unknown,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: selected.identity().clone(),
            scope: Some(InvocationScope {
                session_id: root.id.clone(),
                branch_id: branch.id.clone(),
            }),
            execution_owner: None,
            execution_scope: None,
        };
        // Host Step authorization never starts a paid decision, and requires
        // a real existing SQLite active parent Turn.
        for kind in [
            aw_agent_core::DecisionKind::Planner,
            aw_agent_core::DecisionKind::Guardian,
            aw_agent_core::DecisionKind::Routing,
        ] {
            assert!(bridge
                .authorize_decision_step(&context, kind, &selected)
                .is_err());
        }
        let (_, journal) = store.activate(&owner, &root.id, &branch.id).unwrap();
        journal
            .begin(TurnStart {
                turn_id: 1,
                prompt: "parent paid attempt".into(),
                identity: selected.identity().clone(),
            })
            .await
            .unwrap();
        for kind in [
            aw_agent_core::DecisionKind::Planner,
            aw_agent_core::DecisionKind::Guardian,
            aw_agent_core::DecisionKind::Routing,
        ] {
            bridge
                .authorize_decision_step(&context, kind, &selected)
                .unwrap();
        }
        let mut forged = context.clone();
        forged.execution_site = "subagent:forged".into();
        assert!(bridge
            .authorize_decision_step(&forged, aw_agent_core::DecisionKind::Planner, &selected)
            .is_err());
        forged = context.clone();
        forged.scope.as_mut().unwrap().branch_id = "b_nonexistent".into();
        assert!(bridge
            .authorize_decision_step(&forged, aw_agent_core::DecisionKind::Guardian, &selected)
            .is_err());
        forged = context.clone();
        forged.actor = PrincipalId::new("local:other").unwrap();
        assert!(bridge
            .authorize_decision_step(&forged, aw_agent_core::DecisionKind::Routing, &selected)
            .is_err());
        forged = context.clone();
        forged.cancellation.cancel();
        assert!(bridge
            .authorize_decision_step(&forged, aw_agent_core::DecisionKind::Planner, &selected)
            .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn explicit_actor_bridge_rejects_cross_site_worktree_before_side_effects() {
        let actor = PrincipalId::new("local:no-site-bridge").unwrap();
        let store = SessionStore::in_memory().unwrap();
        let (root, _) = store.create(&actor, "Parent", binding()).unwrap();
        let bridge = actor_owner(&store, &actor, &root.id, 2);
        let state =
            std::env::temp_dir().join(format!("aw-p6-cross-site-refusal-{}", std::process::id()));
        assert!(!state.exists());
        let accounts = ProviderAccounts::new("local").unwrap();
        let request = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Workspace,
                BudgetConfig::default(),
                state.clone(),
                Some(PathBuf::from("/not/approved/git")),
            )
            .with_child_actors(bridge),
        );
        assert!(matches!(request, Err(Error::Denied(_))));
        assert!(
            !state.exists(),
            "invalid cross-site Actor binding must fail before Git side effects"
        );
        let partial = actor_owner_for_mode(&store, &actor, &root.id, 2, ToolMode::ReadOnly);
        let partial_request = Service::new(
            store,
            ProviderAccounts::new("local").unwrap(),
            settings(),
            ServiceConfig::new(
                ToolMode::Workspace,
                BudgetConfig::default(),
                state.clone(),
                Some(PathBuf::from("/not/approved/git")),
            )
            .with_child_actors(partial),
        );
        assert!(matches!(partial_request, Err(Error::Denied(_))));
        assert!(
            !state.exists(),
            "read-only ceiling cannot create writable child worktree"
        );
    }






    #[tokio::test]
    async fn graph_approval_does_not_survive_exact_tool_grant_revocation() {
        let state_dir = std::env::temp_dir().join(format!(
            "aw-graph-revoke-{}-{}",
            std::process::id(),
            unix_micros()
        ));
        aw_platform::ensure_private_dir(&state_dir).unwrap();
        let workspace = state_dir.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let actor = PrincipalId::new("local:graph-revoke").unwrap();
        let main_calls = Arc::new(AtomicUsize::new(0));
        let guardian_calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(GraphAskProvider {
            main_calls,
            guardian_calls: guardian_calls.clone(),
        });
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                provider,
            )
            .unwrap();
        let selected = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap();
        let identity = selected.identity().clone();

        let store = SessionStore::open(&state_dir).unwrap();
        let (parent, branch) = store
            .create(
                &actor,
                "Graph revoke parent",
                aw_store::SessionBinding {
                    working_directory: workspace.clone(),
                    tool_root: workspace.clone(),
                    system_prompt: "system".into(),
                    profile_hint: "fixture-account".into(),
                    model_hint: "fixture-model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Off,
                BudgetConfig::default(),
                state_dir.clone(),
                None,
            ),
        )
        .unwrap();

        let mut tools = LocalToolOptions::new(&workspace, ToolMode::Workspace);
        tools.selected_tools = vec!["write".into()];
        let mut agent = aw_tools_adapter::configure(
            Agent::builder_from_selection(
                selected,
                AgentOptions::new("fixture-account", "fixture-model"),
            ),
            actor.clone(),
            &tools,
        )
        .unwrap()
        .build()
        .unwrap();
        let (view, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        agent
            .load_history(view.history, Some(Arc::new(handle.clone())))
            .unwrap();
        let binding_info = store.agent_binding(&actor, &parent.id, &branch.id).unwrap();
        let execution_owner = AgentExecutionBinding::from_trusted_host(
            format!(
                "/d:{}/i:{}/p:{}/a:{}",
                binding_info.domain_handle,
                binding_info.identity_handle,
                binding_info.project_handle,
                binding_info.agent_handle,
            ),
            binding_info.session_id,
            binding_info.branch_id,
        )
        .unwrap();
        agent
            .bind_execution_owner(execution_owner.clone())
            .unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "pending Graph approval".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();

        let local_policy = aw_policy::Policy {
            deny: false,
            routes: aw_policy::Routes::LOCAL,
            prefer: Some(aw_policy::Strategy::Local),
            require_preferred: true,
            max_model_rounds: None,
        };
        let router = Arc::new(
            PolicyGraphRouter::new(
                service,
                agent.tool_binding_authority(),
                PolicyGraphConfig::deterministic(local_policy, 41)
                    .unwrap()
                    .with_guardian(vec![actor.clone()], Duration::from_secs(60))
                    .unwrap(),
            )
            .unwrap(),
        );
        let ctx = ToolContext {
            actor: actor.clone(),
            turn_id: 1,
            call_id: "graph-revoke-call".into(),
            source: aw_agent_core::ToolSource::trusted_host(),
            effect: ToolEffect::Control,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity,
            scope: Some(InvocationScope {
                session_id: parent.id.clone(),
                branch_id: branch.id.clone(),
            }),
            execution_owner: Some(execution_owner),
            execution_scope: None,
        };
        let request = GraphPolicyRequest {
            job_id: "graph-revoke-job".into(),
            graph_fingerprint: [7; 32],
            nodes: vec![aw_tools_adapter::operation_router::GraphPolicyNode {
                coordinate: 1,
                operation: OperationId::Fs(FsTool::Write),
                replay_identity: [8; 32],
                footprint_fingerprint: [9; 32],
                dependencies: Vec::new(),
            }],
        };
        let task_router = router.clone();
        let task_ctx = ctx.clone();
        let task =
            tokio::spawn(async move { task_router.before_run(&task_ctx, request).await });

        let admission_id = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(id) = router
                    .pending
                    .lock()
                    .unwrap()
                    .get("graph-revoke-job")
                    .and_then(|state| state.by_coordinate.get(&1))
                    .cloned()
                {
                    break id;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(guardian_calls.load(Ordering::SeqCst), 1);

        agent
            .revoke_registered_source_tool(
                &actor,
                "write",
                &aw_agent_core::ToolSource::trusted_host(),
            )
            .unwrap();
        router
            .decide_graph_approval("graph-revoke-job", 1, &actor, true)
            .unwrap();

        let result = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            result,
            Err(Error::Denied("captured tool binding is no longer live"))
        ));
        let fact = store
            .policy_admission_fact(&actor, &parent.id, &admission_id)
            .unwrap();
        assert_eq!(fact.state, PolicyAdmissionState::AskApproved);
        assert_eq!(guardian_calls.load(Ordering::SeqCst), 1);

        let _ = std::fs::remove_dir_all(state_dir);
    }

    #[tokio::test]
    async fn graph_node_delegate_executes_same_jobhost_node_once_and_returns_native_receipt() {
        let state_dir = std::env::temp_dir().join(format!(
            "aw-graph-node-delegate-{}-{}",
            std::process::id(),
            unix_micros()
        ));
        aw_platform::ensure_private_dir(&state_dir).unwrap();
        let workspace = state_dir.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("delegated.txt"), "native delegated value\n").unwrap();

        let actor = PrincipalId::new("local:graph-node-delegate").unwrap();
        let main_calls = Arc::new(AtomicUsize::new(0));
        let delegate_calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(GraphDelegateProvider {
            main_calls: main_calls.clone(),
            delegate_calls: delegate_calls.clone(),
        });
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                provider,
            )
            .unwrap();
        let selected = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap();

        let store = SessionStore::open(&state_dir).unwrap();
        let (parent, branch) = store
            .create(
                &actor,
                "Graph node delegate parent",
                aw_store::SessionBinding {
                    working_directory: workspace.clone(),
                    tool_root: workspace.clone(),
                    system_prompt: "system".into(),
                    profile_hint: "fixture-account".into(),
                    model_hint: "fixture-model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let child_runtime =
            actor_owner_for_mode(&store, &actor, &parent.id, 2, ToolMode::ReadOnly);
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Off,
                BudgetConfig::default(),
                state_dir.clone(),
                None,
            )
            .with_child_actors(child_runtime),
        )
        .unwrap();

        let graph_slot = Arc::new(
            aw_tools_adapter::operation_router::GraphPolicyRouterSlot::default(),
        );
        let mut tools = LocalToolOptions::new(&workspace, ToolMode::ReadOnly);
        tools.selected_tools = vec!["read".into(), "batch".into()];
        tools.graph_policy_router = graph_slot.clone();
        let mut agent = aw_tools_adapter::configure(
            Agent::builder_from_selection(
                selected.clone(),
                AgentOptions::new("fixture-account", "fixture-model"),
            ),
            actor.clone(),
            &tools,
        )
        .unwrap()
        .build()
        .unwrap();
        let (view, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        agent
            .load_history(view.history, Some(Arc::new(handle)))
            .unwrap();
        let binding_info = store.agent_binding(&actor, &parent.id, &branch.id).unwrap();
        agent
            .bind_execution_owner(
                AgentExecutionBinding::from_trusted_host(
                    format!(
                        "/d:{}/i:{}/p:{}/a:{}",
                        binding_info.domain_handle,
                        binding_info.identity_handle,
                        binding_info.project_handle,
                        binding_info.agent_handle,
                    ),
                    binding_info.session_id,
                    binding_info.branch_id,
                )
                .unwrap(),
            )
            .unwrap();

        let delegate_policy = aw_policy::Policy {
            deny: false,
            routes: aw_policy::Routes::BOTH,
            prefer: Some(aw_policy::Strategy::DelegateAgent),
            require_preferred: true,
            max_model_rounds: Some(2),
        };
        graph_slot
            .replace(Arc::new(
                PolicyGraphRouter::new(
                    service,
                    agent.tool_binding_authority(),
                    PolicyGraphConfig::deterministic(delegate_policy, 31).unwrap(),
                )
                .unwrap(),
            ))
            .unwrap();

        let turn = agent
            .prompt(
                "run delegated graph node",
                std::future::pending::<()>(),
                |_| Ok(()),
            )
            .await
            .unwrap();
        assert_eq!(turn.outcome, aw::conversation::Outcome::Completed);
        assert_eq!(turn.answer, "delegated graph complete");
        assert_eq!(main_calls.load(Ordering::SeqCst), 2);
        assert_eq!(delegate_calls.load(Ordering::SeqCst), 2);
        assert_eq!(turn.tool_records.len(), 1);
        assert_eq!(turn.tool_records[0].call.name, "batch");
        let batch_receipt = turn.tool_records[0]
            .output
            .as_ref()
            .expect("Batch factual receipt")
            .content
            .clone();
        assert!(batch_receipt.contains("native delegated value"));
        assert!(!batch_receipt.contains("delegate prose"));
        let children = store.visible_subagents(&actor).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].state, "completed");

        let _ = std::fs::remove_dir_all(state_dir);
    }

    #[tokio::test]
    async fn graph_admission_narrow_rewrites_arguments_before_seal_and_guardian_runs_once() {
        let state_dir = std::env::temp_dir().join(format!(
            "aw-graph-narrow-{}-{}",
            std::process::id(),
            unix_micros()
        ));
        aw_platform::ensure_private_dir(&state_dir).unwrap();
        let workspace = state_dir.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let actor = PrincipalId::new("local:graph-narrow").unwrap();
        let main_calls = Arc::new(AtomicUsize::new(0));
        let guardian_calls = Arc::new(AtomicUsize::new(0));
        let narrow_calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(GraphNarrowProvider {
            main_calls: main_calls.clone(),
            guardian_calls: guardian_calls.clone(),
        });
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                provider,
            )
            .unwrap();
        let selected = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap();

        let store = SessionStore::open(&state_dir).unwrap();
        let (parent, branch) = store
            .create(
                &actor,
                "Graph Narrow parent",
                aw_store::SessionBinding {
                    working_directory: workspace.clone(),
                    tool_root: workspace.clone(),
                    system_prompt: "system".into(),
                    profile_hint: "fixture-account".into(),
                    model_hint: "fixture-model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Off,
                BudgetConfig::default(),
                state_dir.clone(),
                None,
            ),
        )
        .unwrap();

        let graph_slot = Arc::new(
            aw_tools_adapter::operation_router::GraphPolicyRouterSlot::default(),
        );
        let mut tools = LocalToolOptions::new(&workspace, ToolMode::Workspace);
        tools.selected_tools = vec!["write".into(), "batch".into()];
        tools.graph_policy_router = graph_slot.clone();
        let mut agent = aw_tools_adapter::configure(
            Agent::builder_from_selection(
                selected.clone(),
                AgentOptions::new("fixture-account", "fixture-model"),
            ),
            actor.clone(),
            &tools,
        )
        .unwrap()
        .build()
        .unwrap();
        let (view, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        agent
            .load_history(view.history, Some(Arc::new(handle)))
            .unwrap();
        let binding_info = store.agent_binding(&actor, &parent.id, &branch.id).unwrap();
        agent
            .bind_execution_owner(
                AgentExecutionBinding::from_trusted_host(
                    format!(
                        "/d:{}/i:{}/p:{}/a:{}",
                        binding_info.domain_handle,
                        binding_info.identity_handle,
                        binding_info.project_handle,
                        binding_info.agent_handle,
                    ),
                    binding_info.session_id,
                    binding_info.branch_id,
                )
                .unwrap(),
            )
            .unwrap();

        let local_policy = aw_policy::Policy {
            deny: false,
            routes: aw_policy::Routes::LOCAL,
            prefer: Some(aw_policy::Strategy::Local),
            require_preferred: true,
            max_model_rounds: None,
        };
        let config = PolicyGraphConfig::deterministic(local_policy, 41)
            .unwrap()
            .with_narrow_authority(Arc::new(StaticGraphNarrowAuthority {
                calls: narrow_calls.clone(),
            }))
            .with_guardian(Vec::new(), Duration::from_secs(60))
            .unwrap();
        graph_slot
            .replace(Arc::new(
                PolicyGraphRouter::new(
                    service,
                    agent.tool_binding_authority(),
                    config,
                )
                .unwrap(),
            ))
            .unwrap();

        let turn = tokio::time::timeout(
            Duration::from_secs(8),
            agent.prompt(
                "run narrowed graph",
                std::future::pending::<()>(),
                |_| Ok(()),
            ),
        )
        .await
        .expect("pre-seal Graph Narrow must not deadlock")
        .unwrap();
        assert_eq!(turn.outcome, aw::conversation::Outcome::Completed);
        assert_eq!(turn.answer, "narrow graph complete");
        assert_eq!(main_calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            guardian_calls.load(Ordering::SeqCst),
            1,
            "one original policy claim may run at most one guardian"
        );
        assert_eq!(
            narrow_calls.load(Ordering::SeqCst),
            1,
            "trusted host revision authority is consulted exactly once"
        );
        assert!(
            !workspace.join("unsafe.txt").exists(),
            "original unsealed arguments must never execute"
        );
        assert_eq!(
            std::fs::read_to_string(workspace.join("safe.txt")).unwrap(),
            "narrowed-value"
        );
        assert_eq!(turn.tool_records.len(), 1);
        assert_eq!(turn.tool_records[0].call.name, "batch");

        let _ = std::fs::remove_dir_all(state_dir);
    }

    #[tokio::test]
    async fn graph_multi_ask_starts_zero_roots_until_all_approvals_and_is_idempotent() {
        let state_dir = std::env::temp_dir().join(format!(
            "aw-graph-multi-ask-{}-{}",
            std::process::id(),
            unix_micros()
        ));
        aw_platform::ensure_private_dir(&state_dir).unwrap();
        let workspace = state_dir.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let actor = PrincipalId::new("local:graph-ask").unwrap();
        let main_calls = Arc::new(AtomicUsize::new(0));
        let guardian_calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(GraphAskProvider {
            main_calls: main_calls.clone(),
            guardian_calls: guardian_calls.clone(),
        });
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                provider,
            )
            .unwrap();
        let selected = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap();

        let store = SessionStore::open(&state_dir).unwrap();
        let (parent, branch) = store
            .create(
                &actor,
                "Graph Ask parent",
                aw_store::SessionBinding {
                    working_directory: workspace.clone(),
                    tool_root: workspace.clone(),
                    system_prompt: "system".into(),
                    profile_hint: "fixture-account".into(),
                    model_hint: "fixture-model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Off,
                BudgetConfig::default(),
                state_dir.clone(),
                None,
            ),
        )
        .unwrap();

        let graph_slot = Arc::new(
            aw_tools_adapter::operation_router::GraphPolicyRouterSlot::default(),
        );
        let mut tools = LocalToolOptions::new(&workspace, ToolMode::Workspace);
        tools.selected_tools = vec!["write".into(), "batch".into()];
        tools.graph_policy_router = graph_slot.clone();
        let mut agent = aw_tools_adapter::configure(
            Agent::builder_from_selection(
                selected.clone(),
                AgentOptions::new("fixture-account", "fixture-model"),
            ),
            actor.clone(),
            &tools,
        )
        .unwrap()
        .build()
        .unwrap();
        let (view, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        agent
            .load_history(view.history, Some(Arc::new(handle)))
            .unwrap();
        let binding_info = store.agent_binding(&actor, &parent.id, &branch.id).unwrap();
        agent
            .bind_execution_owner(
                AgentExecutionBinding::from_trusted_host(
                    format!(
                        "/d:{}/i:{}/p:{}/a:{}",
                        binding_info.domain_handle,
                        binding_info.identity_handle,
                        binding_info.project_handle,
                        binding_info.agent_handle,
                    ),
                    binding_info.session_id,
                    binding_info.branch_id,
                )
                .unwrap(),
            )
            .unwrap();

        let local_policy = aw_policy::Policy {
            deny: false,
            routes: aw_policy::Routes::LOCAL,
            prefer: Some(aw_policy::Strategy::Local),
            require_preferred: true,
            max_model_rounds: None,
        };
        let router = Arc::new(
            PolicyGraphRouter::new(
                service,
                agent.tool_binding_authority(),
                PolicyGraphConfig::deterministic(local_policy, 21)
                    .unwrap()
                    .with_guardian(vec![actor.clone()], Duration::from_secs(60))
                    .unwrap(),
            )
            .unwrap(),
        );
        graph_slot.replace(router.clone()).unwrap();

        let task = tokio::spawn(async move {
            agent
                .prompt("run the gated graph", std::future::pending::<()>(), |_| Ok(()))
                .await
        });

        let job_id = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(job_id) = router
                    .pending
                    .lock()
                    .unwrap()
                    .keys()
                    .next()
                    .cloned()
                {
                    break job_id;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        let pending = router
            .pending_approvals(&job_id)
            .expect("Graph approval set must be published");
        assert_eq!(pending.entries().len(), 2);
        assert_eq!(guardian_calls.load(Ordering::SeqCst), 2);
        assert!(!workspace.join("approved-one.txt").exists());
        assert!(!workspace.join("approved-two.txt").exists());
        assert!(!task.is_finished());

        let first = router
            .decide_graph_approval(&job_id, 1, &actor, true)
            .unwrap();
        let duplicate = router
            .decide_graph_approval(&job_id, 1, &actor, true)
            .unwrap();
        assert_eq!(first, duplicate, "approval replay must be idempotent");
        tokio::time::sleep(Duration::from_millis(75)).await;
        assert!(!workspace.join("approved-one.txt").exists());
        assert!(!workspace.join("approved-two.txt").exists());
        assert!(!task.is_finished(), "one of two approvals cannot release Graph roots");

        router
            .decide_graph_approval(&job_id, 2, &actor, true)
            .unwrap();
        let turn = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(turn.outcome, aw::conversation::Outcome::Completed);
        assert_eq!(turn.answer, "graph complete");
        assert_eq!(std::fs::read_to_string(workspace.join("approved-one.txt")).unwrap(), "one");
        assert_eq!(std::fs::read_to_string(workspace.join("approved-two.txt")).unwrap(), "two");
        assert_eq!(guardian_calls.load(Ordering::SeqCst), 2);
        assert_eq!(main_calls.load(Ordering::SeqCst), 2);
        assert!(router.pending_approvals(&job_id).is_none());

        let _ = std::fs::remove_dir_all(state_dir);
    }

    #[tokio::test]
    async fn policy_operation_router_local_and_delegate_share_one_factual_executor() {
        let actor = PrincipalId::new("local:policy-operation").unwrap();
        let provider_calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(PolicyDelegateProvider {
            calls: provider_calls.clone(),
        });
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                provider,
            )
            .unwrap();
        let selected = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap();
        let identity = selected.identity().clone();

        let store = SessionStore::in_memory().unwrap();
        let (parent, branch) = store.create(&actor, "Policy parent", binding()).unwrap();
        let (view, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        let mut parent_agent = Agent::builder_from_selection(
            selected.clone(),
            AgentOptions::new("fixture-account", "fixture-model"),
        )
        .build()
        .unwrap();
        parent_agent
            .load_history(view.history, Some(Arc::new(handle.clone())))
            .unwrap();
        let binding_info = store.agent_binding(&actor, &parent.id, &branch.id).unwrap();
        let execution_owner = AgentExecutionBinding::from_trusted_host(
            format!(
                "/d:{}/i:{}/p:{}/a:{}",
                binding_info.domain_handle,
                binding_info.identity_handle,
                binding_info.project_handle,
                binding_info.agent_handle,
            ),
            binding_info.session_id,
            binding_info.branch_id,
        )
        .unwrap();
        parent_agent
            .bind_execution_owner(execution_owner.clone())
            .unwrap();

        let native_calls = Arc::new(AtomicUsize::new(0));
        let native: Arc<dyn Tool> = Arc::new(FactualReadTool {
            calls: native_calls.clone(),
        });
        parent_agent.register_host_tool(native.clone()).unwrap();
        let authority = parent_agent.tool_binding_authority();

        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "parent policy turn".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();

        let child_runtime = actor_owner_for_mode(
            &store,
            &actor,
            &parent.id,
            2,
            ToolMode::ReadOnly,
        );
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Off,
                BudgetConfig::default(),
                PathBuf::from("/tmp"),
                None,
            )
            .with_child_actors(child_runtime),
        )
        .unwrap();

        let base_context = ToolContext {
            actor: actor.clone(),
            turn_id: 1,
            call_id: "policy-local".into(),
            source: aw_agent_core::ToolSource::trusted_host(),
            effect: ToolEffect::Read,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity,
            scope: Some(InvocationScope {
                session_id: parent.id.clone(),
                branch_id: branch.id.clone(),
            }),
            execution_owner: Some(execution_owner),
            execution_scope: None,
        };

        let local_policy = aw_policy::Policy {
            deny: false,
            routes: aw_policy::Routes::LOCAL,
            prefer: Some(aw_policy::Strategy::Local),
            require_preferred: true,
            max_model_rounds: None,
        };
        let local_config =
            PolicyOperationConfig::deterministic(local_policy, 11)
                .unwrap()
                .with_guardian(false);
        assert_eq!(local_config.policy(), local_policy);
        assert_eq!(local_config.revision(), 11);
        let local_router =
            PolicyOperationRouter::new(service.clone(), authority.clone(), local_config).unwrap();
        let local_args = json!({"path":"local.txt"});
        let local = local_router
            .execute(
                OperationId::Fs(FsTool::Read),
                base_context.clone(),
                &local_args,
                native.clone(),
            )
            .await
            .unwrap();
        assert_eq!(local.content, "native:local.txt");
        assert_eq!(native_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            provider_calls.load(Ordering::SeqCst),
            0,
            "deterministic Local policy must perform zero auxiliary/delegate model calls"
        );

        let delegate_policy = aw_policy::Policy {
            deny: false,
            routes: aw_policy::Routes::BOTH,
            prefer: Some(aw_policy::Strategy::DelegateAgent),
            require_preferred: true,
            max_model_rounds: Some(2),
        };
        let delegate_router = PolicyOperationRouter::new(
            service,
            authority,
            PolicyOperationConfig::deterministic(delegate_policy, 12).unwrap(),
        )
        .unwrap();
        let mut delegate_context = base_context.clone();
        delegate_context.call_id = "policy-delegate".into();
        let delegate_args = json!({"path":"sealed-parent-argument.txt"});
        let delegated = delegate_router
            .execute(
                OperationId::Fs(FsTool::Read),
                delegate_context.clone(),
                &delegate_args,
                native.clone(),
            )
            .await
            .unwrap();
        assert_eq!(delegated.content, "native:sealed-parent-argument.txt");
        assert_eq!(
            native_calls.load(Ordering::SeqCst),
            2,
            "delegate must execute the original factual operation exactly once"
        );
        assert_eq!(
            provider_calls.load(Ordering::SeqCst),
            2,
            "delegate Agent gets one tool proposal round and one post-result round"
        );
        assert!(
            delegate_router
                .execute(
                    OperationId::Fs(FsTool::Read),
                    delegate_context,
                    &delegate_args,
                    native,
                )
                .await
                .is_err(),
            "consumed admission replay must not repeat the native effect"
        );
        assert_eq!(native_calls.load(Ordering::SeqCst), 2);
        assert_eq!(provider_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn whole_graph_delegate_uses_one_jobmanager_and_parent_receives_exact_batch_receipt() {
        let state_dir = std::env::temp_dir().join(format!(
            "aw-whole-graph-delegate-{}-{}",
            std::process::id(),
            unix_micros()
        ));
        aw_platform::ensure_private_dir(&state_dir).unwrap();
        let workspace = state_dir.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let actor = PrincipalId::new("local:whole-graph-delegate").unwrap();
        let parent_calls = Arc::new(AtomicUsize::new(0));
        let delegate_calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(WholeGraphDelegateProvider {
            parent_calls: parent_calls.clone(),
            delegate_calls: delegate_calls.clone(),
        });
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                provider,
            )
            .unwrap();
        let selected = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap();

        let store = SessionStore::open(&state_dir).unwrap();
        let (parent, branch) = store
            .create(
                &actor,
                "Whole Graph delegate parent",
                aw_store::SessionBinding {
                    working_directory: workspace.clone(),
                    tool_root: workspace.clone(),
                    system_prompt: "system".into(),
                    profile_hint: "fixture-account".into(),
                    model_hint: "fixture-model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();

        let mut tools = LocalToolOptions::new(&workspace, ToolMode::Workspace);
        tools.selected_tools = vec!["write".into(), "batch".into()];
        let mut agent = aw_tools_adapter::configure(
            Agent::builder_from_selection(
                selected,
                AgentOptions::new("fixture-account", "fixture-model"),
            ),
            actor.clone(),
            &tools,
        )
        .unwrap()
        .build()
        .unwrap();
        let (view, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        agent
            .load_history(view.history, Some(Arc::new(handle)))
            .unwrap();
        let binding_info = store.agent_binding(&actor, &parent.id, &branch.id).unwrap();
        agent
            .bind_execution_owner(
                AgentExecutionBinding::from_trusted_host(
                    format!(
                        "/d:{}/i:{}/p:{}/a:{}",
                        binding_info.domain_handle,
                        binding_info.identity_handle,
                        binding_info.project_handle,
                        binding_info.agent_handle,
                    ),
                    binding_info.session_id,
                    binding_info.branch_id,
                )
                .unwrap(),
            )
            .unwrap();

        let child_runtime = actor_owner_for_mode_with_extra(
            &store,
            &actor,
            &parent.id,
            2,
            ToolMode::Off,
            &[(
                "batch",
                aw_agent_kernel::capability::Effect::Control,
            )],
        );
        let service = Service::new(
            store.clone(),
            agent.provider_accounts(),
            settings(),
            ServiceConfig::new(
                ToolMode::Off,
                BudgetConfig::default(),
                state_dir.clone(),
                None,
            )
            .with_child_actors(child_runtime),
        )
        .unwrap();

        let delegate_policy = aw_policy::Policy {
            deny: false,
            routes: aw_policy::Routes::BOTH,
            prefer: Some(aw_policy::Strategy::DelegateAgent),
            require_preferred: true,
            max_model_rounds: Some(2),
        };
        let router = PolicyOperationRouter::new(
            service,
            agent.tool_binding_authority(),
            PolicyOperationConfig::deterministic(delegate_policy, 77).unwrap(),
        )
        .unwrap();
        tools.operation_router.replace(Arc::new(router)).unwrap();

        let turn = tokio::time::timeout(
            Duration::from_secs(8),
            agent.prompt(
                "run one delegated whole graph",
                std::future::pending::<()>(),
                |_| Ok(()),
            ),
        )
        .await
        .expect("whole-Graph delegate must not deadlock")
        .unwrap();
        assert_eq!(turn.outcome, aw::conversation::Outcome::Completed);
        assert_eq!(turn.answer, "whole graph parent done");
        assert_eq!(parent_calls.load(Ordering::SeqCst), 2);
        assert_eq!(delegate_calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            std::fs::read_to_string(workspace.join("whole-graph.txt")).unwrap(),
            "one-job"
        );
        assert_eq!(turn.tool_records.len(), 1);
        assert_eq!(turn.tool_records[0].call.name, "batch");
        let parent_receipt = turn.tool_records[0]
            .output
            .as_ref()
            .expect("parent Batch factual receipt")
            .content
            .clone();
        assert!(!parent_receipt.is_empty());
        assert!(!parent_receipt.contains("delegate prose must never replace"));

        let relations = store.subagents_for_parent(&actor, &parent.id).unwrap();
        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0].state, "completed");
        let child_session = relations[0].child_session_id.clone();
        let child_binding = store
            .agent_bindings(&actor)
            .unwrap()
            .into_iter()
            .find(|binding| binding.session_id == child_session)
            .expect("delegate child Agent binding");
        let child_view = store
            .open(&actor, &child_session, &child_binding.branch_id)
            .unwrap()
            .view()
            .unwrap();
        assert_eq!(child_view.history.turns.len(), 1);
        let child_turn = &child_view.history.turns[0];
        assert_eq!(child_turn.tool_records.len(), 1);
        assert_eq!(child_turn.tool_records[0].call.name, "batch");
        let child_receipt = &child_turn.tool_records[0]
            .output
            .as_ref()
            .expect("delegate gate factual receipt")
            .content;
        assert_eq!(
            child_receipt, &parent_receipt,
            "whole-Graph delegate and parent must observe the same original Batch receipt"
        );
        assert!(
            !child_turn.answer.contains("whole-graph.txt"),
            "delegate prose is not proof of Graph execution"
        );

        let _ = std::fs::remove_dir_all(state_dir);
    }

    #[tokio::test]
    async fn graph_agent_node_spawns_nested_agent_under_one_inherited_delegation_budget() {
        let state_dir = std::env::temp_dir().join(format!(
            "aw-graph-agent-node-{}-{}",
            std::process::id(),
            unix_micros()
        ));
        aw_platform::ensure_private_dir(&state_dir).unwrap();
        let workspace = state_dir.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let actor = PrincipalId::new("local:graph-agent-node").unwrap();
        let parent_calls = Arc::new(AtomicUsize::new(0));
        let child_calls = Arc::new(AtomicUsize::new(0));
        let grandchild_calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(GraphAgentProvider {
            parent_calls: parent_calls.clone(),
            child_calls: child_calls.clone(),
            grandchild_calls: grandchild_calls.clone(),
        });
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                provider,
            )
            .unwrap();
        let selected = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap();

        let store = SessionStore::open(&state_dir).unwrap();
        let (parent, branch) = store
            .create(
                &actor,
                "Graph Agent parent",
                aw_store::SessionBinding {
                    working_directory: workspace.clone(),
                    tool_root: workspace.clone(),
                    system_prompt: "system".into(),
                    profile_hint: "fixture-account".into(),
                    model_hint: "fixture-model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Off,
                BudgetConfig::new(4, 3, 8, 2, 3600).unwrap(),
                state_dir.clone(),
                None,
            ),
        )
        .unwrap();
        let router = Arc::new(
            aw::agent_jobs::Router::new(
                store.clone(),
                actor.clone(),
                LocalToolLimits::default().jobs.max_retained_jobs,
            )
            .unwrap(),
        );
        service.bind_agent_jobs_router(router.clone()).unwrap();

        let graph_host_tools = Arc::new(
            aw_tools_adapter::operation_router::GraphHostToolSlot::default(),
        );
        let mut tools = LocalToolOptions::new(&workspace, ToolMode::ReadOnly);
        tools.allow_agent_graph = true;
        tools.graph_host_tools = graph_host_tools.clone();
        let mut agent = aw_tools_adapter::configure(
            Agent::builder_from_selection(
                selected,
                AgentOptions::new("fixture-account", "fixture-model"),
            ),
            actor.clone(),
            &tools,
        )
        .unwrap()
        .build()
        .unwrap();
        let (view, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        agent
            .load_history(view.history, Some(Arc::new(handle)))
            .unwrap();
        let binding_info = store.agent_binding(&actor, &parent.id, &branch.id).unwrap();
        agent
            .bind_execution_owner(
                AgentExecutionBinding::from_trusted_host(
                    format!(
                        "/d:{}/i:{}/p:{}/a:{}",
                        binding_info.domain_handle,
                        binding_info.identity_handle,
                        binding_info.project_handle,
                        binding_info.agent_handle,
                    ),
                    binding_info.session_id,
                    binding_info.branch_id,
                )
                .unwrap(),
            )
            .unwrap();

        let agent_tool = service.agent_tool();
        agent.register_host_tool(agent_tool.clone()).unwrap();
        graph_host_tools
            .install(OperationId::Agent, agent_tool)
            .unwrap();

        let turn = tokio::time::timeout(
            Duration::from_secs(8),
            agent.prompt(
                "parent graph agent",
                std::future::pending::<()>(),
                |_| Ok(()),
            ),
        )
        .await
        .expect("nested Agent Graph must not deadlock on the parent controller")
        .unwrap();
        assert_eq!(turn.outcome, aw::conversation::Outcome::Completed);
        assert_eq!(turn.answer, "parent graph answer");
        assert_eq!(parent_calls.load(Ordering::SeqCst), 2);
        assert_eq!(child_calls.load(Ordering::SeqCst), 2);
        assert_eq!(grandchild_calls.load(Ordering::SeqCst), 1);
        assert_eq!(turn.tool_records.len(), 1);
        assert_eq!(turn.tool_records[0].call.name, "batch");
        let batch = &turn.tool_records[0].output.as_ref().unwrap().content;
        assert!(batch.contains("level1 graph answer"), "{batch}");
        assert!(batch.contains(" agent idle "), "{batch}");

        let mut relations = store.visible_subagents(&actor).unwrap();
        relations.sort_by_key(|relation| relation.depth);
        assert_eq!(relations.len(), 2);
        assert_eq!(relations[0].depth, Some(1));
        assert_eq!(relations[1].depth, Some(2));
        assert_eq!(relations[0].state, "running");
        assert_eq!(relations[1].state, "running");
        let root_id = relations[0]
            .delegation_root_id
            .clone()
            .expect("Graph child must have one delegation root");
        assert_eq!(relations[1].delegation_root_id.as_deref(), Some(root_id.as_str()));
        assert_eq!(
            relations[1].parent_session_id, relations[0].child_session_id,
            "nested Agent must be owned by the depth-1 Graph Agent"
        );
        let root = store.delegation_root(&actor, &root_id).unwrap();
        assert_eq!(root.children_created, 2);
        assert_eq!(root.active_children, 2);
        assert_eq!(
            root.model_attempts_started, 3,
            "depth-1 tool-call/final plus depth-2 final share one causal budget"
        );
        assert_eq!(root.model_attempts_finished, 3);

        // Both stable workers are intentionally still addressable. Dropping the
        // router at test end disconnects their wake handles; no second scheduler
        // or hidden child runtime is introduced by Graph execution.
        drop(router);
        let _ = std::fs::remove_dir_all(state_dir);
    }

    #[tokio::test]
    async fn stable_agent_survives_initial_turn_continues_and_closes_without_reallocation() {
        let state_dir = std::env::temp_dir().join(format!(
            "aw-stable-agent-{}-{}",
            std::process::id(),
            unix_micros()
        ));
        aw_platform::ensure_private_dir(&state_dir).unwrap();
        let actor = PrincipalId::new("local:stable-agent").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(EchoProvider {
            calls: calls.clone(),
        });
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                provider,
            )
            .unwrap();
        let selection = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap();
        let identity = selection.identity().clone();

        let store = SessionStore::open(&state_dir).unwrap();
        let (parent, branch) = store.create(&actor, "Parent", binding()).unwrap();
        let (_, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "parent secret text".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();

        let parent_binding = store.agent_binding(&actor, &parent.id, &branch.id).unwrap();
        let parent_owner = AgentExecutionBinding::from_trusted_host(
            format!(
                "/d:{}/i:{}/p:{}/a:{}",
                parent_binding.domain_handle,
                parent_binding.identity_handle,
                parent_binding.project_handle,
                parent_binding.agent_handle,
            ),
            parent_binding.session_id.clone(),
            parent_binding.branch_id.clone(),
        )
        .unwrap();

        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Off,
                BudgetConfig::default(),
                PathBuf::from("/tmp"),
                None,
            ),
        )
        .unwrap();
        let router = Arc::new(
            aw::agent_jobs::Router::new(
                store.clone(),
                actor.clone(),
                LocalToolLimits::default().jobs.max_retained_jobs,
            )
            .unwrap(),
        );
        service.bind_agent_jobs_router(router.clone()).unwrap();
        let constructor = service.agent_tool();

        let constructor_ctx = ToolContext {
            actor: actor.clone(),
            turn_id: 1,
            call_id: "create-stable-agent".into(),
            source: aw_agent_core::ToolSource::trusted_host(),
            effect: ToolEffect::Process,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: identity.clone(),
            scope: Some(InvocationScope {
                session_id: parent.id.clone(),
                branch_id: branch.id.clone(),
            }),
            execution_owner: Some(parent_owner.clone()),
            execution_scope: None,
        };
        let output = constructor
            .execute(
                constructor_ctx.clone(),
                &json!({
                    "task":"research task",
                    "title":"Stable child",
                    "background":false
                }),
            )
            .await
            .unwrap();
        assert!(!output.is_error, "{}", output.content);
        let canonical = output
            .content
            .split_whitespace()
            .next()
            .expect("compact Agent receipt")
            .to_owned();
        assert!(canonical.starts_with("/d:"));
        assert!(output.content.contains(" agent idle "));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let replay = constructor
            .execute(
                constructor_ctx.clone(),
                &json!({
                    "task":"research task",
                    "title":"Stable child",
                    "background":false
                }),
            )
            .await
            .unwrap();
        assert!(!replay.is_error, "{}", replay.content);
        assert!(replay.content.starts_with(&canonical));
        assert!(replay.content.contains(" unchanged"));
        assert!(replay.content.contains("child answer"));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "constructor replay must not pay for another child Turn"
        );
        assert!(constructor
            .execute(
                constructor_ctx,
                &json!({
                    "task":"changed task under same call id",
                    "title":"Stable child",
                    "background":false
                }),
            )
            .await
            .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let relations = store.subagents_for_parent(&actor, &parent.id).unwrap();
        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0].state, "running");
        let child_session = relations[0].child_session_id.clone();
        let child_binding = store
            .agent_bindings(&actor)
            .unwrap()
            .into_iter()
            .find(|binding| binding.session_id == child_session)
            .unwrap();
        assert_eq!(
            format!(
                "/d:{}/i:{}/p:{}/a:{}",
                child_binding.domain_handle,
                child_binding.identity_handle,
                child_binding.project_handle,
                child_binding.agent_handle,
            ),
            canonical
        );
        assert_eq!(
            store
                .open(&actor, &child_session, &child_binding.branch_id)
                .unwrap()
                .view()
                .unwrap()
                .history
                .turns
                .len(),
            1
        );

        let child_wake_ctx = ToolContext {
            actor: actor.clone(),
            turn_id: 1,
            call_id: "event-wake-recipient".into(),
            source: aw_agent_core::ToolSource::trusted_host(),
            effect: ToolEffect::Control,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: identity.clone(),
            scope: Some(InvocationScope {
                session_id: child_session.clone(),
                branch_id: child_binding.branch_id.clone(),
            }),
            execution_owner: Some(
                AgentExecutionBinding::from_trusted_host(
                    canonical.clone(),
                    child_session.clone(),
                    child_binding.branch_id.clone(),
                )
                .unwrap(),
            ),
            execution_scope: None,
        };
        let event_recipient =
            aw_tools_adapter::agent_jobs::AgentJobRouter::wake_recipient(
                router.as_ref(),
                &child_wake_ctx,
            )
            .unwrap()
            .expect("stable child must expose bounded jobs_subscribe wake authority");
        assert_eq!(event_recipient.canonical_ref, canonical);
        let event_source = format!(
            "{}/t:1/j:event-source",
            parent_owner.canonical_ref()
        );
        for (event, mode) in [
            (
                aw_tools::job_event::JobEventKind::Result,
                aw_tools::job_event::JobWakeMode::Auto,
            ),
            (
                aw_tools::job_event::JobEventKind::Settled,
                aw_tools::job_event::JobWakeMode::Queue,
            ),
        ] {
            aw_tools_adapter::agent_jobs::AgentJobRouter::admit_event(
                router.as_ref(),
                aw_tools::job_event::JobEventDelivery {
                    source_ref: event_source.clone(),
                    event,
                    event_rev: 11,
                    recipient_ref: canonical.clone(),
                    recipient_authority_ref: event_recipient.authority_ref.clone(),
                    subscription_generation: 7,
                    mode,
                },
            )
            .await
            .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let settled = store
                    .branches(&actor, &child_session)
                    .unwrap()
                    .into_iter()
                    .find(|branch| branch.id == child_binding.branch_id)
                    .is_some_and(|branch| branch.active_run.is_none());
                if settled && calls.load(Ordering::SeqCst) == 2 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            store
                .open(&actor, &child_session, &child_binding.branch_id)
                .unwrap()
                .view()
                .unwrap()
                .history
                .turns
                .len(),
            2
        );
        let event_queue = store.actor_queue_list(&actor, &child_session).unwrap();
        assert_eq!(
            event_queue
                .iter()
                .filter(|fact| fact.request.request_id.starts_with("jc_"))
                .count(),
            1,
            "same-checkpoint result+settled with one effective queue mode must coalesce to one continuation"
        );
        let after_event_root = store
            .delegation_root(
                &actor,
                relations[0]
                    .delegation_root_id
                    .as_deref()
                    .expect("stable Agent delegation root"),
            )
            .unwrap();
        assert_eq!(after_event_root.model_attempts_started, 2);
        assert_eq!(after_event_root.model_attempts_finished, 2);

        let send_ctx = ToolContext {
            actor: actor.clone(),
            turn_id: 1,
            call_id: "continue-stable-agent".into(),
            source: aw_agent_core::ToolSource::trusted_host(),
            effect: ToolEffect::Control,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: identity.clone(),
            scope: Some(InvocationScope {
                session_id: parent.id.clone(),
                branch_id: branch.id.clone(),
            }),
            execution_owner: Some(parent_owner.clone()),
            execution_scope: None,
        };
        let send = aw_tools_adapter::agent_jobs::AgentJobRouter::send(
            router.as_ref(),
            &send_ctx,
            aw_tools_adapter::agent_jobs::AgentJobSend {
                reference: canonical.clone(),
                revision: None,
                mode: aw_tools_adapter::agent_jobs::AgentSendMode::Auto,
                input: aw_tools_adapter::agent_jobs::AgentSendInput::Text(
                    "continue child".into(),
                ),
            },
        )
        .await
        .unwrap()
        .unwrap();
        assert!(!send.is_error, "{}", send.content);

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let settled = store
                    .branches(&actor, &child_session)
                    .unwrap()
                    .into_iter()
                    .find(|branch| branch.id == child_binding.branch_id)
                    .is_some_and(|branch| branch.active_run.is_none());
                if settled && calls.load(Ordering::SeqCst) == 3 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(
            store
                .open(&actor, &child_session, &child_binding.branch_id)
                .unwrap()
                .view()
                .unwrap()
                .history
                .turns
                .len(),
            3
        );
        let relations_after = store.subagents_for_parent(&actor, &parent.id).unwrap();
        assert_eq!(relations_after.len(), 1);
        assert_eq!(relations_after[0].child_session_id, child_session);
        assert_eq!(relations_after[0].state, "running");
        let root_id = relations_after[0]
            .delegation_root_id
            .as_deref()
            .expect("stable Agent must remain inside one delegation root")
            .to_owned();
        let root = store.delegation_root(&actor, &root_id).unwrap();
        assert_eq!(
            root.active_children, 1,
            "idle long-lived Agent still owns one bounded active-child slot"
        );
        assert_eq!(
            root.model_attempts_started, 3,
            "initial Turn, one coalesced event wake, and one manual continuation consume exactly three paid-attempt slots"
        );
        assert_eq!(root.model_attempts_finished, 3);
        assert_eq!(
            root.calls_without_complete_usage, 0,
            "fixture provider reports complete usage for every child Turn"
        );

        let cancel_ctx = ToolContext {
            call_id: "close-stable-agent".into(),
            ..send_ctx
        };
        let close = aw_tools_adapter::agent_jobs::AgentJobRouter::cancel(
            router.as_ref(),
            &cancel_ctx,
            aw_tools_adapter::agent_jobs::AgentJobCancel {
                reference: canonical.clone(),
                revision: None,
            },
        )
        .await
        .unwrap()
        .unwrap();
        assert!(close.content.contains(" agent closing "));

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let relation = store
                    .subagent_for_child_session(&actor, &child_session)
                    .unwrap()
                    .unwrap();
                if relation.state != "running" {
                    assert_eq!(relation.state, "cancelled");
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        let get = aw_tools_adapter::agent_jobs::AgentJobRouter::get(
            router.as_ref(),
            &ToolContext {
                actor: actor.clone(),
                turn_id: 1,
                call_id: "observe-closed-agent".into(),
                source: aw_agent_core::ToolSource::trusted_host(),
                effect: ToolEffect::Read,
                execution_site: "local".into(),
                cancellation: CancellationToken::new(),
                result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
                identity,
                scope: Some(InvocationScope {
                    session_id: parent.id,
                    branch_id: branch.id,
                }),
                execution_owner: Some(parent_owner),
                execution_scope: None,
            },
            aw_tools_adapter::agent_jobs::AgentJobGet {
                reference: canonical,
                revision: None,
                wait_ms: 0,
            },
        )
        .await
        .unwrap()
        .unwrap();
        assert!(get.content.contains(" agent closed "));
        let closed_root = store.delegation_root(&actor, &root_id).unwrap();
        assert_eq!(
            closed_root.active_children, 0,
            "whole-Agent close releases exactly the retained child slot"
        );
        assert_eq!(closed_root.model_attempts_started, 3);
        assert_eq!(closed_root.model_attempts_finished, 3);
    }

    #[tokio::test]
    async fn foreground_tool_uses_exact_parent_lease_and_isolated_child_session() {
        let actor = PrincipalId::new("local:test").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(EchoProvider {
            calls: calls.clone(),
        });
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                provider,
            )
            .unwrap();
        let selection = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap();
        let identity = selection.identity().clone();

        let store = SessionStore::in_memory().unwrap();
        let (parent, branch) = store
            .create(&actor, "Parent", binding())
            .unwrap();
        let (_, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "parent secret text".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        // The real Agent journals its Assistant proposal before the Process
        // tool intent; interrupted-parent fixtures preserve that ordering.
        let mut assistant = Message::text(2, Role::Assistant, "");
        assistant.tool_calls = vec![ToolCall {
            id: "call-subagent-fixture".into(), name: "subagent".into(),
            arguments: json!({"task":"research task"}),
        }];
        handle.append(1, JournalEntry::Assistant { message: assistant }).await.unwrap();
        handle
            .append(
                1,
                JournalEntry::ToolStarted {
                    record: ToolRecord {
                        call: ToolCall {
                            id: "call-subagent-fixture".into(),
                            name: "subagent".into(),
                            arguments: json!({"task":"research task"}),
                        },
                        effect: ToolEffect::Process,
                        execution_site: "local".into(),
                        status: ToolStatus::Running,
                        output: None,
                    },
                    source: None,
                },
            )
            .await
            .unwrap();

        let actor_service=actor_owner(&store,&actor,&parent.id,2);
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Off,
                BudgetConfig::default(),
                PathBuf::from("/tmp"),
                None,
            ).with_child_actors(actor_service.clone()),
        ).unwrap();
        assert!(service
            .validate(&json!({"task":"x","model":"other"}))
            .is_err());

        let output = service
            .execute(
                ToolContext {
                    actor: actor.clone(),
                    turn_id: 1,
                    call_id: "host-direct".into(),
                    source: aw_agent_core::ToolSource::trusted_host(),
                    effect: aw_agent_core::ToolEffect::Unknown,
                    execution_site: "local".into(),
                    cancellation: CancellationToken::new(),
                    result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
                    identity: identity.clone(),
                    scope: Some(InvocationScope {
                        session_id: parent.id.clone(),
                        branch_id: branch.id.clone(),
                    }),
                    execution_owner: None,
                    execution_scope: None,
                },
                &json!({
                    "task": "research task",
                    "title": "Research child",
                    "mode": "foreground"
                }),
            )
            .await
            .unwrap();
        assert!(!output.is_error);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let result: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(result["state"], "completed");
        assert_eq!(result["answer"], "child answer");

        let relations = store.subagents_for_parent(&actor, &parent.id).unwrap();
        assert_eq!(relations.len(), 1);
        let relation = &relations[0];
        assert_eq!(relation.identity, identity);
        assert_eq!(relation.state, "completed");
        let live=actor_service.list().unwrap();
        assert_eq!(live.len(),1);
        assert_eq!(live[0].0.id,relation.id);
        assert!(live[0].1.parent.is_some());
        assert_eq!(live[0].1.kind,aw_agent_kernel::actor::ActorKind::Agent);
        assert_eq!(live[0].1.state,aw_agent_kernel::actor::ActorState::Settled);
        assert_eq!(live[0].1.depth,1);
        let (stored,descriptor)=actor_service.status(&relation.id).unwrap();
        assert_eq!(stored.state,"completed");
        assert_eq!(descriptor.key,live[0].1.key);
        let fact=actor_service.wait(&relation.id,CancellationToken::new(),
            Duration::from_millis(200)).await.unwrap();
        assert_eq!(fact.state,"completed");
        assert!(actor_service.signal(&relation.id,
            aw_agent_kernel::message::ControlKind::Cancel(
                aw_agent_kernel::message::Scope::Turn),1).is_err());
        assert!(actor_service.release_settled(&relation.id).is_err(),
            "cannot discard a final parent notice before it is consumed");
        let notice=actor_service.poll_root_notification(1).unwrap().unwrap();
        assert_eq!(notice.from,descriptor.key);
        assert_eq!(notice.kind,aw_agent_kernel::message::Kind::Notify);
        assert!(!notice.payload.contains("child answer"));
        assert!(!notice.payload.contains("parent secret text"));
        assert_eq!(serde_json::from_str::<serde_json::Value>(&notice.payload)
            .unwrap()["state"],"completed");
        assert!(actor_service.poll_root_notification(1).unwrap().is_none());
        actor_service.release_settled(&relation.id).unwrap();
        assert!(actor_service.list().unwrap().is_empty());
        assert!(actor_service.status(&relation.id).is_err());
        assert_eq!(store.subagent(&actor,&relation.id).unwrap().state,"completed");


        let (view, _) = store
            .activate(&actor, &relation.child_session_id, "main")
            .unwrap();
        assert_eq!(view.session.title, "Research child");
        assert_eq!(view.history.turns.len(), 1);
        assert_eq!(view.history.turns[0].prompt, "research task");
        assert_eq!(view.history.turns[0].answer, "child answer");
        assert!(!view
            .history
            .turns
            .iter()
            .any(|turn| turn.prompt == "parent secret text"));

        // The parent turn is still a separate active turn; the service did not
        // mutate or complete it while running the child.
        assert!(store.branches(&actor, &parent.id).unwrap()[0]
            .active_run
            .is_some());
    }

    #[tokio::test]
    async fn attached_background_child_cancel_is_fenced_and_never_replayed() {
        use aw_agent_kernel::message::{ControlKind, Scope};
        let actor=PrincipalId::new("local:child-cancel").unwrap();
        let calls=Arc::new(AtomicUsize::new(0));
        let entered=Arc::new(tokio::sync::Notify::new());
        let accounts=ProviderAccounts::new("local").unwrap();
        accounts.register(AccountDescriptor::personal(
            "fixture-account",actor.clone(),"local",["fixture-model".into()]
        ),Arc::new(BlockingProvider{
            calls:calls.clone(),entered:entered.clone()
        })).unwrap();
        let identity=accounts.select(&actor,"fixture-account","fixture-model")
            .unwrap().identity().clone();
        let store=SessionStore::in_memory().unwrap();
        let (parent,branch)=store.create(&actor,"Parent",binding()).unwrap();
        let (_,journal)=store.activate(&actor,&parent.id,&branch.id).unwrap();
        journal.begin(TurnStart{
            turn_id:1,prompt:"parent secret text".into(),identity:identity.clone()
        }).await.unwrap();
        let mut proposal=Message::text(2,Role::Assistant,"");
        proposal.tool_calls=vec![ToolCall{
            id:"bg-child".into(),name:"subagent".into(),
            arguments:json!({"task":"background task","mode":"background"}),
        }];
        journal.append(1,JournalEntry::Assistant{message:proposal}).await.unwrap();
        journal.append(1,JournalEntry::ToolStarted{record:ToolRecord{
            call:ToolCall{id:"bg-child".into(),name:"subagent".into(),
                arguments:json!({"task":"background task","mode":"background"})},
            effect:ToolEffect::Process,execution_site:"local".into(),
            status:ToolStatus::Running,output:None
        },
                source: None,}).await.unwrap();
        let actors=actor_owner(&store,&actor,&parent.id,2);
        let service=Service::new(store.clone(),accounts,settings(),
            ServiceConfig::new(ToolMode::Off,BudgetConfig::default(),
                PathBuf::from("/tmp"),None).with_child_actors(actors.clone())).unwrap();
        let response=service.execute(ToolContext{
            actor:actor.clone(),turn_id:1,call_id:"host-direct".into(),source:aw_agent_core::ToolSource::trusted_host(),effect:aw_agent_core::ToolEffect::Unknown,execution_site:"local".into(),
            cancellation:CancellationToken::new(),
            result_budget_bytes:aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity,
            scope:Some(InvocationScope{
                session_id:parent.id.clone(),branch_id:branch.id.clone()
            }),
            execution_owner: None,
            execution_scope: None,
        },&json!({"task":"background task","mode":"background"})).await.unwrap();
        assert!(!response.is_error);
        let response:serde_json::Value=serde_json::from_str(&response.content).unwrap();
        let id=response["subagent_id"].as_str().unwrap();
        assert_eq!(response["state"],"running");
        tokio::time::timeout(Duration::from_secs(3),entered.notified())
            .await.expect("original child provider never started");
        assert_eq!(calls.load(Ordering::SeqCst),1);
        // Broad or stale signals are never executed as child cancellation.
        assert!(matches!(actors.signal(id,ControlKind::Freeze(Scope::Turn),1),
            Ok(aw_agent_core::runtime::Delivery::Applied(_))));
        assert!(matches!(actors.signal(id,ControlKind::Resume(Scope::Turn),1),
            Ok(aw_agent_core::runtime::Delivery::Applied(_))));
        assert!(actors.signal(id,ControlKind::Cancel(Scope::Subtree),1).is_err());
        assert!(actors.signal(id,ControlKind::Cancel(Scope::Turn),2).is_err());
        assert!(matches!(actors.signal(id,ControlKind::Cancel(Scope::Turn),1),
            Ok(aw_agent_core::runtime::Delivery::Applied(_))));
        let settled=actors.wait(id,CancellationToken::new(),
            Duration::from_secs(3)).await.unwrap();
        assert_eq!(settled.state,"cancelled");
        assert_eq!(calls.load(Ordering::SeqCst),1);
        assert_eq!(actors.status(id).unwrap().1.state,
            aw_agent_kernel::actor::ActorState::Cancelled);
        assert!(actors.signal(id,ControlKind::Cancel(Scope::Turn),1).is_err());
        let notice=actors.poll_root_notification(1).unwrap().unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&notice.payload)
            .unwrap()["state"],"cancelled");
        assert!(!notice.payload.contains("background task"));
        assert!(actors.poll_root_notification(1).unwrap().is_none());
    }

    #[tokio::test]
    async fn recursive_model_only_child_delegates_to_depth_two_under_same_root() {
        let actor = PrincipalId::new("local:recursive-off").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                Arc::new(RecursiveProvider {
                    calls: calls.clone(),
                }),
            )
            .unwrap();
        let identity = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap()
            .identity()
            .clone();
        let store = SessionStore::in_memory().unwrap();
        let (parent, branch) = store.create(&actor, "Parent", binding()).unwrap();
        let (_, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "parent secret text".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        // The real Agent journals its Assistant proposal before the Process
        // tool intent; interrupted-parent fixtures preserve that ordering.
        let mut assistant = Message::text(2, Role::Assistant, "");
        assistant.tool_calls = vec![ToolCall {
            id: "call-recursive-root".into(), name: "subagent".into(),
            arguments: json!({"task":"level1 task"}),
        }];
        handle.append(1, JournalEntry::Assistant { message: assistant }).await.unwrap();
        handle
            .append(
                1,
                JournalEntry::ToolStarted {
                    record: ToolRecord {
                        call: ToolCall {
                            id: "call-recursive-root".into(),
                            name: "subagent".into(),
                            arguments: json!({"task":"level1 task"}),
                        },
                        effect: ToolEffect::Process,
                        execution_site: "local".into(),
                        status: ToolStatus::Running,
                        output: None,
                    },
                    source: None,
                },
            )
            .await
            .unwrap();
        let actor_service=actor_owner(&store,&actor,&parent.id,2);
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Off,
                BudgetConfig::new(4, 2, 8, 2, 3600).unwrap(),
                PathBuf::from("/tmp"),
                None,
            ).with_child_actors(actor_service.clone()),
        )
        .unwrap();
        let output = service
            .execute(
                ToolContext {
                    actor: actor.clone(),
                    turn_id: 1,
                    call_id: "host-direct".into(),
                    source: aw_agent_core::ToolSource::trusted_host(),
                    effect: aw_agent_core::ToolEffect::Unknown,
                    execution_site: "local".into(),
                    cancellation: CancellationToken::new(),
                    result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
                    identity,
                    scope: Some(InvocationScope {
                        session_id: parent.id.clone(),
                        branch_id: branch.id.clone(),
                    }),
                    execution_owner: None,
                    execution_scope: None,
                },
                &json!({"task":"level1 task","title":"First child","mode":"foreground"}),
            )
            .await
            .unwrap();
        assert!(!output.is_error);
        let body: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(body["answer"], "level1 answer");
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        let live=actor_service.list().unwrap();
        assert_eq!(live.len(),2);
        let depth_one=live.iter().find(|(_,a)|a.depth==1).unwrap();
        let depth_two=live.iter().find(|(_,a)|a.depth==2).unwrap();
        assert_eq!(depth_two.1.parent,Some(depth_one.1.key));
        assert_eq!(depth_one.1.state,aw_agent_kernel::actor::ActorState::Settled);
        assert_eq!(depth_two.1.state,aw_agent_kernel::actor::ActorState::Settled);

        let first = store
            .subagents_for_parent(&actor, &parent.id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(first.depth, Some(1));
        assert_eq!(first.state, "completed");
        let second = store
            .subagents_for_parent(&actor, &first.child_session_id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(second.depth, Some(2));
        assert_eq!(second.state, "completed");
        assert!(actor_service.release_settled(&second.id).is_err(),
            "nested parent's unconsumed final notice must block release");
        let nested_notice=actor_service.poll_child_notification(&first.id,1)
            .unwrap().unwrap();
        assert_eq!(nested_notice.from,depth_two.1.key);
        assert_eq!(nested_notice.to,depth_one.1.key);
        assert!(nested_notice.payload.contains("completed"));
        assert!(!nested_notice.payload.contains("level2 answer"));
        actor_service.release_settled(&second.id).unwrap();
        assert!(actor_service.release_settled(&first.id).is_err());
        let root_notice=actor_service.poll_root_notification(1).unwrap().unwrap();
        assert_eq!(root_notice.from,depth_one.1.key);
        assert!(!root_notice.payload.contains("level1 answer"));
        actor_service.release_settled(&first.id).unwrap();
        assert!(actor_service.list().unwrap().is_empty());
        assert_eq!(store.subagent(&actor,&first.id).unwrap().state,"completed");
        assert_eq!(store.subagent(&actor,&second.id).unwrap().state,"completed");

        assert_eq!(second.delegation_root_id, first.delegation_root_id);
        let root = store
            .delegation_root(&actor, first.delegation_root_id.as_deref().unwrap())
            .unwrap();
        assert_eq!(root.children_created, 2);
        assert_eq!(root.active_children, 0);
        assert_eq!(root.model_attempts_started, 3);
        assert_eq!(root.model_attempts_finished, 3);
        assert_eq!(root.limits.max_depth, 2);
    }

    #[cfg(all(unix, feature = "workspace"))]
    #[tokio::test]
    async fn recursive_workspace_child_gets_clean_sibling_worktree_and_same_root() {
        let fixture = GitFixture::new();
        let actor = PrincipalId::new("local:recursive-workspace").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                Arc::new(RecursiveWorkspaceProvider {
                    calls: calls.clone(),
                }),
            )
            .unwrap();
        let identity = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap()
            .identity()
            .clone();
        let store = SessionStore::in_memory().unwrap();
        let parent_binding = aw_store::SessionBinding {
            working_directory: fixture.repo.clone(),
            tool_root: fixture.repo.clone(),
            system_prompt: "parent instructions".into(),
            profile_hint: "fixture-account".into(),
            model_hint: "fixture-model".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (parent, branch) = store.create(&actor, "Parent", parent_binding).unwrap();
        let (_, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "parent secret".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        // The real Agent journals its Assistant proposal before the Process
        // tool intent; interrupted-parent fixtures preserve that ordering.
        let mut assistant = Message::text(2, Role::Assistant, "");
        assistant.tool_calls = vec![ToolCall {
            id: "call-recursive-workspace".into(),
            name: "subagent".into(),
            arguments: json!({"task":"workspace-parent task"}),
        }];
        handle
            .append(1, JournalEntry::Assistant { message: assistant })
            .await
            .unwrap();
        handle
            .append(
                1,
                JournalEntry::ToolStarted {
                    record: ToolRecord {
                        call: ToolCall {
                            id: "call-recursive-workspace".into(),
                            name: "subagent".into(),
                            arguments: json!({"task":"workspace-parent task"}),
                        },
                        effect: ToolEffect::Process,
                        execution_site: "local".into(),
                        status: ToolStatus::Running,
                        output: None,
                    },
                    source: None,
                },
            )
            .await
            .unwrap();
        let actor_service =
            actor_owner_for_mode(&store, &actor, &parent.id, 2, ToolMode::Workspace);
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Workspace,
                BudgetConfig::new(4, 3, 8, 2, 3600).unwrap(),
                fixture.state.clone(),
                Some(fixture.git.clone()),
            )
            .with_child_actors(actor_service.clone()),
        )
        .unwrap();
        let output = service
            .execute(
                ToolContext {
                    actor: actor.clone(),
                    turn_id: 1,
                    call_id: "host-direct".into(),
                    source: aw_agent_core::ToolSource::trusted_host(),
                    effect: aw_agent_core::ToolEffect::Unknown,
                    execution_site: "local".into(),
                    cancellation: CancellationToken::new(),
                    result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
                    identity,
                    scope: Some(InvocationScope {
                        session_id: parent.id.clone(),
                        branch_id: branch.id.clone(),
                    }),
                    execution_owner: None,
                    execution_scope: None,
                },
                &json!({"task":"workspace-parent task","mode":"foreground"}),
            )
            .await
            .unwrap();
        assert!(!output.is_error);
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        assert!(!fixture.repo.join("grandchild-only.txt").exists());

        let first = store
            .subagents_for_parent(&actor, &parent.id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let second = store
            .subagents_for_parent(&actor, &first.child_session_id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(first.depth, Some(1));
        assert_eq!(second.depth, Some(2));
        let first_actor = actor_service.status(&first.id).unwrap().1;
        let second_actor = actor_service.status(&second.id).unwrap().1;
        assert_eq!(first_actor.depth, 1);
        assert_eq!(second_actor.depth, 2);
        assert_eq!(second_actor.parent, Some(first_actor.key));
        assert_eq!(
            first_actor.state,
            aw_agent_kernel::actor::ActorState::Settled
        );
        assert_eq!(
            second_actor.state,
            aw_agent_kernel::actor::ActorState::Settled
        );
        let first_env = actor_service.envelope(&first.id).unwrap();
        let second_env = actor_service.envelope(&second.id).unwrap();
        assert_eq!(first_env.site(), format!("subagent:{}", first.id));
        assert_eq!(second_env.site(), format!("subagent:{}", second.id));
        assert_ne!(first_env.site(), second_env.site());
        assert_eq!(
            first_env.spec().network,
            aw_agent_kernel::envelope::Network::Deny
        );
        assert_eq!(
            second_env.spec().network,
            aw_agent_kernel::envelope::Network::Deny
        );
        assert!(actor_service.release_settled(&second.id).is_err());
        let nested = actor_service
            .poll_child_notification(&first.id, 1)
            .unwrap()
            .unwrap();
        assert_eq!(nested.from, second_actor.key);
        assert_eq!(nested.to, first_actor.key);
        assert!(!nested.payload.contains("grandchild isolated bytes"));
        actor_service.release_settled(&second.id).unwrap();
        assert!(actor_service.release_settled(&first.id).is_err());
        let root_notice = actor_service.poll_root_notification(1).unwrap().unwrap();
        assert_eq!(root_notice.from, first_actor.key);
        actor_service.release_settled(&first.id).unwrap();
        assert!(actor_service.list().unwrap().is_empty());

        let first_exec = store
            .subagent_execution_binding_for_subagent(&actor, &first.id)
            .unwrap()
            .unwrap();
        let second_exec = store
            .subagent_execution_binding_for_subagent(&actor, &second.id)
            .unwrap()
            .unwrap();
        assert_eq!(
            first_exec.execution_root.parent(),
            second_exec.execution_root.parent()
        );
        assert!(!second_exec
            .execution_root
            .starts_with(&first_exec.execution_root));
        assert!(!first_exec
            .execution_root
            .join("grandchild-only.txt")
            .exists());
        assert_eq!(
            std::fs::read(second_exec.execution_root.join("grandchild-only.txt")).unwrap(),
            b"grandchild isolated bytes\\n"
        );
        let repo = aw_git::Repository::discover(&fixture.git, &fixture.repo).unwrap();
        let first_repo =
            aw_git::Repository::discover(&fixture.git, &first_exec.execution_root).unwrap();
        assert!(first_repo.status().unwrap().changes.is_empty());
        assert!(repo.status().unwrap().changes.is_empty());
        let root = store
            .delegation_root(&actor, first.delegation_root_id.as_deref().unwrap())
            .unwrap();
        assert_eq!(root.children_created, 2);
        assert_eq!(root.active_children, 0);
        assert_eq!(root.model_attempts_started, 4);
        assert_eq!(root.limits.max_depth, 2);

        let cleanup_second = store
            .begin_subagent_execution_cleanup(&actor, &second.id)
            .unwrap();
        repo.remove_managed_worktree(&cleanup_second.execution_root, true)
            .unwrap();
        store
            .finish_subagent_execution_cleanup(&actor, &cleanup_second.id)
            .unwrap();
        let cleanup_first = store
            .begin_subagent_execution_cleanup(&actor, &first.id)
            .unwrap();
        repo.remove_managed_worktree(&cleanup_first.execution_root, false)
            .unwrap();
        store
            .finish_subagent_execution_cleanup(&actor, &cleanup_first.id)
            .unwrap();
    }

    #[cfg(all(unix, feature = "workspace"))]
    #[tokio::test]
    async fn dirty_recursive_parent_consumes_allocation_but_never_dispatches_descendant() {
        let fixture = GitFixture::new();
        let actor = PrincipalId::new("local:dirty-recursive").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                Arc::new(DirtyRecursiveWorkspaceProvider {
                    calls: calls.clone(),
                }),
            )
            .unwrap();
        let identity = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap()
            .identity()
            .clone();
        let store = SessionStore::in_memory().unwrap();
        let parent_binding = aw_store::SessionBinding {
            working_directory: fixture.repo.clone(),
            tool_root: fixture.repo.clone(),
            system_prompt: "parent instructions".into(),
            profile_hint: "fixture-account".into(),
            model_hint: "fixture-model".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (parent, branch) = store.create(&actor, "Parent", parent_binding).unwrap();
        let (_, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "root".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        // The real Agent journals its Assistant proposal before the Process
        // tool intent; interrupted-parent fixtures preserve that ordering.
        let mut assistant = Message::text(2, Role::Assistant, "");
        assistant.tool_calls = vec![ToolCall {
            id: "root-dirty-recursive".into(), name: "subagent".into(),
            arguments: json!({"task":"dirty-parent task"}),
        }];
        handle.append(1, JournalEntry::Assistant { message: assistant }).await.unwrap();
        handle
            .append(
                1,
                JournalEntry::ToolStarted {
                    record: ToolRecord {
                        call: ToolCall {
                            id: "root-dirty-recursive".into(),
                            name: "subagent".into(),
                            arguments: json!({"task":"dirty-parent task"}),
                        },
                        effect: ToolEffect::Process,
                        execution_site: "local".into(),
                        status: ToolStatus::Running,
                        output: None,
                    },
                    source: None,
                },
            )
            .await
            .unwrap();
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Workspace,
                BudgetConfig::new(4, 2, 8, 2, 3600).unwrap(),
                fixture.state.clone(),
                Some(fixture.git.clone()),
            ),
        )
        .unwrap();
        let output = service
            .execute(
                ToolContext {
                    actor: actor.clone(),
                    turn_id: 1,
                    call_id: "host-direct".into(),
                    source: aw_agent_core::ToolSource::trusted_host(),
                    effect: aw_agent_core::ToolEffect::Unknown,
                    execution_site: "local".into(),
                    cancellation: CancellationToken::new(),
                    result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
                    identity,
                    scope: Some(InvocationScope {
                        session_id: parent.id.clone(),
                        branch_id: branch.id.clone(),
                    }),
                    execution_owner: None,
                    execution_scope: None,
                },
                &json!({"task":"dirty-parent task","mode":"foreground"}),
            )
            .await
            .unwrap();
        assert!(output.is_error);
        assert!(output.content.contains("managed child worktree requires a clean parent checkout"));
        assert_eq!(calls.load(Ordering::SeqCst), 2, "descendant provider must never run");
        let first = store
            .subagents_for_parent(&actor, &parent.id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(store.subagents_for_parent(&actor, &first.child_session_id).unwrap().len(), 0);
        let root = store
            .delegation_root(&actor, first.delegation_root_id.as_deref().unwrap())
            .unwrap();
        assert_eq!(root.children_created, 2, "failed descendant allocation stays consumed");
        assert_eq!(root.active_children, 0);
        let first_exec = store
            .subagent_execution_binding_for_subagent(&actor, &first.id)
            .unwrap()
            .unwrap();
        assert!(first_exec.execution_root.join("dirty-before-delegate.txt").exists());
        let cleanup = store
            .begin_subagent_execution_cleanup(&actor, &first.id)
            .unwrap();
        let repo = aw_git::Repository::discover(&fixture.git, &fixture.repo).unwrap();
        repo.remove_managed_worktree(&cleanup.execution_root, true)
            .unwrap();
        store
            .finish_subagent_execution_cleanup(&actor, &cleanup.id)
            .unwrap();
    }

    #[cfg(all(unix, feature = "workspace"))]
    #[tokio::test]
    async fn workspace_child_edits_only_managed_worktree_and_retains_it_for_review() {
        let fixture = GitFixture::new();
        let actor = PrincipalId::new("local:workspace-child").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                Arc::new(WorkspaceToolProvider {
                    calls: calls.clone(),
                }),
            )
            .unwrap();
        let identity = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap()
            .identity()
            .clone();
        let store = SessionStore::in_memory().unwrap();
        let parent_binding = aw_store::SessionBinding {
            working_directory: fixture.repo.clone(),
            tool_root: fixture.repo.clone(),
            system_prompt: "parent instructions".into(),
            profile_hint: "fixture-account".into(),
            model_hint: "fixture-model".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (parent, branch) = store.create(&actor, "Parent", parent_binding).unwrap();
        let (_, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "parent secret text".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        // The real Agent journals its Assistant proposal before the Process
        // tool intent; interrupted-parent fixtures preserve that ordering.
        let mut assistant = Message::text(2, Role::Assistant, "");
        assistant.tool_calls = vec![ToolCall {
            id: "call-subagent-workspace".into(),
            name: "subagent".into(),
            arguments: json!({"task":"workspace task"}),
        }];
        handle
            .append(1, JournalEntry::Assistant { message: assistant })
            .await
            .unwrap();
        handle
            .append(
                1,
                JournalEntry::ToolStarted {
                    record: ToolRecord {
                        call: ToolCall {
                            id: "call-subagent-workspace".into(),
                            name: "subagent".into(),
                            arguments: json!({"task":"workspace task"}),
                        },
                        effect: ToolEffect::Process,
                        execution_site: "local".into(),
                        status: ToolStatus::Running,
                        output: None,
                    },
                    source: None,
                },
            )
            .await
            .unwrap();

        let actor_service =
            actor_owner_for_mode(&store, &actor, &parent.id, 1, ToolMode::Workspace);
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Workspace,
                BudgetConfig::default(),
                fixture.state.clone(),
                Some(fixture.git.clone()),
            )
            .with_child_actors(actor_service.clone()),
        )
        .unwrap();
        let output = service
            .execute(
                ToolContext {
                    actor: actor.clone(),
                    turn_id: 1,
                    call_id: "host-direct".into(),
                    source: aw_agent_core::ToolSource::trusted_host(),
                    effect: aw_agent_core::ToolEffect::Unknown,
                    execution_site: "local".into(),
                    cancellation: CancellationToken::new(),
                    result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
                    identity,
                    scope: Some(InvocationScope {
                        session_id: parent.id.clone(),
                        branch_id: branch.id.clone(),
                    }),
                    execution_owner: None,
                    execution_scope: None,
                },
                &json!({
                    "task":"workspace task",
                    "title":"Editing child",
                    "mode":"foreground"
                }),
            )
            .await
            .unwrap();
        assert!(!output.is_error);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(!fixture.repo.join("child-created.txt").exists());
        assert_eq!(
            std::fs::read(fixture.repo.join("base.txt")).unwrap(),
            b"parent bytes\\n"
        );

        let relation = store
            .subagents_for_parent(&actor, &parent.id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(relation.state, "completed");
        assert_eq!(relation.tool_mode,"workspace");
        let (actor_info, descriptor) = actor_service.status(&relation.id).unwrap();
        assert_eq!(actor_info.child_session_id, relation.child_session_id);
        assert_eq!(descriptor.depth, 1);
        assert_eq!(
            descriptor.state,
            aw_agent_kernel::actor::ActorState::Settled
        );
        let bound = actor_service.envelope(&relation.id).unwrap();
        assert_eq!(bound.site(), format!("subagent:{}", relation.id));
        assert_eq!(
            bound.spec().network,
            aw_agent_kernel::envelope::Network::Deny
        );
        assert_eq!(bound.spec().grants.len(), 3);
        assert!(bound
            .spec()
            .grants
            .iter()
            .all(|g| g.kind == aw_agent_kernel::capability::SourceKind::Host));
        let execution = store
            .subagent_execution_binding_for_subagent(&actor, &relation.id)
            .unwrap()
            .unwrap();
        assert_eq!(execution.state, "bound");
        assert!(execution.execution_root.starts_with(&fixture.state));
        assert_eq!(
            std::fs::read(execution.execution_root.join("child-created.txt")).unwrap(),
            b"created only in child worktree\\n"
        );
        let (view, _) = store
            .activate(&actor, &relation.child_session_id, "main")
            .unwrap();
        assert_eq!(view.session.binding.tool_root, execution.execution_root);
        assert_eq!(view.history.turns.len(), 1);
        assert_eq!(view.history.turns[0].tool_records.len(), 1);
        assert_eq!(view.history.turns[0].tool_records[0].call.name, "write");
        assert!(view.history.turns[0]
            .tool_records
            .iter()
            .all(|record| record.call.name != "subagent" && record.call.name != "bash"));

        let cleanup = store
            .begin_subagent_execution_cleanup(&actor, &relation.id)
            .unwrap();
        let repo = aw_git::Repository::discover(&fixture.git, &fixture.repo).unwrap();
        assert!(matches!(
            repo.remove_managed_worktree(&cleanup.execution_root, false),
            Err(aw_git::Error::Stale)
        ));
        let restored = store
            .cancel_subagent_execution_cleanup(&actor, &cleanup.id)
            .unwrap();
        assert_eq!(restored.state, "bound");
        let cleanup = store
            .begin_subagent_execution_cleanup(&actor, &relation.id)
            .unwrap();
        repo.remove_managed_worktree(&cleanup.execution_root, true)
            .unwrap();
        let removed = store
            .finish_subagent_execution_cleanup(&actor, &cleanup.id)
            .unwrap();
        assert_eq!(removed.state, "removed");
        assert_eq!(
            store
                .session(&actor, &relation.child_session_id)
                .unwrap()
                .id,
            relation.child_session_id
        );
    }

    #[cfg(all(unix, feature = "workspace"))]
    #[tokio::test]
    async fn read_only_child_advertises_only_read_and_cleans_without_discard() {
        let fixture = GitFixture::new();
        let actor = PrincipalId::new("local:read-only-child").unwrap();
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                Arc::new(ReadOnlyToolProvider),
            )
            .unwrap();
        let identity = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap()
            .identity()
            .clone();
        let store = SessionStore::in_memory().unwrap();
        let parent_binding = aw_store::SessionBinding {
            working_directory: fixture.repo.clone(),
            tool_root: fixture.repo.clone(),
            system_prompt: "parent instructions".into(),
            profile_hint: "fixture-account".into(),
            model_hint: "fixture-model".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (parent, branch) = store.create(&actor, "Parent", parent_binding).unwrap();
        let (_, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "delegate".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        // The real Agent journals its Assistant proposal before the Process
        // tool intent; interrupted-parent fixtures preserve that ordering.
        let mut assistant = Message::text(2, Role::Assistant, "");
        assistant.tool_calls = vec![ToolCall {
            id: "call-subagent-read-only".into(), name: "subagent".into(),
            arguments: json!({"task":"read task"}),
        }];
        handle.append(1, JournalEntry::Assistant { message: assistant }).await.unwrap();
        handle
            .append(
                1,
                JournalEntry::ToolStarted {
                    record: ToolRecord {
                        call: ToolCall {
                            id: "call-subagent-read-only".into(),
                            name: "subagent".into(),
                            arguments: json!({"task":"read task"}),
                        },
                        effect: ToolEffect::Process,
                        execution_site: "local".into(),
                        status: ToolStatus::Running,
                        output: None,
                    },
                    source: None,
                },
            )
            .await
            .unwrap();
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::ReadOnly,
                BudgetConfig::default(),
                fixture.state.clone(),
                Some(fixture.git.clone()),
            ),
        )
        .unwrap();
        let output = service
            .execute(
                ToolContext {
                    actor: actor.clone(),
                    turn_id: 1,
                    call_id: "host-direct".into(),
                    source: aw_agent_core::ToolSource::trusted_host(),
                    effect: aw_agent_core::ToolEffect::Unknown,
                    execution_site: "local".into(),
                    cancellation: CancellationToken::new(),
                    result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
                    identity,
                    scope: Some(InvocationScope {
                        session_id: parent.id.clone(),
                        branch_id: branch.id.clone(),
                    }),
                    execution_owner: None,
                    execution_scope: None,
                },
                &json!({"task":"read task","mode":"foreground"}),
            )
            .await
            .unwrap();
        let result: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(result["answer"], "read-only child answer");
        let relation = store
            .subagents_for_parent(&actor, &parent.id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(relation.tool_mode, "read-only");
        let execution = store
            .subagent_execution_binding_for_subagent(&actor, &relation.id)
            .unwrap()
            .unwrap();
        assert!(execution.execution_root.join("base.txt").exists());
        let cleanup = store
            .begin_subagent_execution_cleanup(&actor, &relation.id)
            .unwrap();
        let repo = aw_git::Repository::discover(&fixture.git, &fixture.repo).unwrap();
        repo.remove_managed_worktree(&cleanup.execution_root, false)
            .unwrap();
        assert_eq!(
            store
                .finish_subagent_execution_cleanup(&actor, &cleanup.id)
                .unwrap()
                .state,
            "removed"
        );
        assert!(store.session(&actor, &relation.child_session_id).is_ok());
    }

    #[cfg(all(unix, feature = "workspace"))]
    #[tokio::test]
    async fn delegation_model_attempt_limit_blocks_second_provider_round() {
        let fixture = GitFixture::new();
        let actor = PrincipalId::new("local:budget-limit-child").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                Arc::new(WorkspaceToolProvider {
                    calls: calls.clone(),
                }),
            )
            .unwrap();
        let identity = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap()
            .identity()
            .clone();
        let store = SessionStore::in_memory().unwrap();
        let parent_binding = aw_store::SessionBinding {
            working_directory: fixture.repo.clone(),
            tool_root: fixture.repo.clone(),
            system_prompt: "parent instructions".into(),
            profile_hint: "fixture-account".into(),
            model_hint: "fixture-model".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (parent, branch) = store.create(&actor, "Parent", parent_binding).unwrap();
        let (_, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "delegate".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        // The real Agent journals its Assistant proposal before the Process
        // tool intent; interrupted-parent fixtures preserve that ordering.
        let mut assistant = Message::text(2, Role::Assistant, "");
        assistant.tool_calls = vec![ToolCall {
            id: "call-subagent-budget".into(), name: "subagent".into(),
            arguments: json!({"task":"workspace task"}),
        }];
        handle.append(1, JournalEntry::Assistant { message: assistant }).await.unwrap();
        handle
            .append(
                1,
                JournalEntry::ToolStarted {
                    record: ToolRecord {
                        call: ToolCall {
                            id: "call-subagent-budget".into(),
                            name: "subagent".into(),
                            arguments: json!({"task":"workspace task"}),
                        },
                        effect: ToolEffect::Process,
                        execution_site: "local".into(),
                        status: ToolStatus::Running,
                        output: None,
                    },
                    source: None,
                },
            )
            .await
            .unwrap();
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Workspace,
                BudgetConfig::new(4, 1, 1, 1, 3600).unwrap(),
                fixture.state.clone(),
                Some(fixture.git.clone()),
            ),
        )
        .unwrap();
        let output = service
            .execute(
                ToolContext {
                    actor: actor.clone(),
                    turn_id: 1,
                    call_id: "host-direct".into(),
                    source: aw_agent_core::ToolSource::trusted_host(),
                    effect: aw_agent_core::ToolEffect::Unknown,
                    execution_site: "local".into(),
                    cancellation: CancellationToken::new(),
                    result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
                    identity,
                    scope: Some(InvocationScope {
                        session_id: parent.id.clone(),
                        branch_id: branch.id.clone(),
                    }),
                    execution_owner: None,
                    execution_scope: None,
                },
                &json!({"task":"workspace task","mode":"foreground"}),
            )
            .await
            .unwrap();
        assert!(output.is_error);
        assert!(output.content.contains("delegation model-attempt limit reached"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let relation = store
            .subagents_for_parent(&actor, &parent.id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(relation.state, "failed");
        let root = store
            .delegation_root_for_session(&actor, &relation.child_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(root.model_attempts_started, 1);
        assert_eq!(root.model_attempts_finished, 1);
        assert_eq!(root.active_children, 0);
        let execution = store
            .subagent_execution_binding_for_subagent(&actor, &relation.id)
            .unwrap()
            .unwrap();
        assert!(execution.execution_root.join("child-created.txt").exists());
        let cleanup = store
            .begin_subagent_execution_cleanup(&actor, &relation.id)
            .unwrap();
        let repo = aw_git::Repository::discover(&fixture.git, &fixture.repo).unwrap();
        repo.remove_managed_worktree(&cleanup.execution_root, true)
            .unwrap();
        store
            .finish_subagent_execution_cleanup(&actor, &cleanup.id)
            .unwrap();
    }

    #[tokio::test]
    async fn background_tool_returns_immediately_and_settles_child_independently() {
        let actor = PrincipalId::new("local:background").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                Arc::new(EchoProvider {
                    calls: calls.clone(),
                }),
            )
            .unwrap();
        let identity = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap()
            .identity()
            .clone();
        let store = SessionStore::in_memory().unwrap();
        let (parent, branch) = store.create(&actor, "Parent", binding()).unwrap();
        let (_, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "parent secret text".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        // The real Agent journals its Assistant proposal before the Process
        // tool intent; interrupted-parent fixtures preserve that ordering.
        let mut assistant = Message::text(2, Role::Assistant, "");
        assistant.tool_calls = vec![ToolCall {
            id: "call-subagent-background".into(), name: "subagent".into(),
            arguments: json!({"task":"research task","mode":"background"}),
        }];
        handle.append(1, JournalEntry::Assistant { message: assistant }).await.unwrap();
        handle
            .append(
                1,
                JournalEntry::ToolStarted {
                    record: ToolRecord {
                        call: ToolCall {
                            id: "call-subagent-background".into(),
                            name: "subagent".into(),
                            arguments: json!({"task":"research task","mode":"background"}),
                        },
                        effect: ToolEffect::Process,
                        execution_site: "local".into(),
                        status: ToolStatus::Running,
                        output: None,
                    },
                    source: None,
                },
            )
            .await
            .unwrap();
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Off,
                BudgetConfig::default(),
                PathBuf::from("/tmp"),
                None,
            ),
        ).unwrap();
        let output = service
            .execute(
                ToolContext {
                    actor: actor.clone(),
                    turn_id: 1,
                    call_id: "host-direct".into(),
                    source: aw_agent_core::ToolSource::trusted_host(),
                    effect: aw_agent_core::ToolEffect::Unknown,
                    execution_site: "local".into(),
                    cancellation: CancellationToken::new(),
                    result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
                    identity,
                    scope: Some(InvocationScope {
                        session_id: parent.id.clone(),
                        branch_id: branch.id.clone(),
                    }),
                    execution_owner: None,
                    execution_scope: None,
                },
                &json!({"task":"research task","mode":"background"}),
            )
            .await
            .unwrap();
        let started: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(started["state"], "running");
        let child_session = started["session_id"].as_str().unwrap().to_owned();

        let mut settled = None;
        for _ in 0..100 {
            let relation = store
                .subagents_for_parent(&actor, &parent.id)
                .unwrap()
                .into_iter()
                .find(|item| item.child_session_id == child_session)
                .unwrap();
            if relation.state != "running" && relation.state != "admitted" {
                settled = Some(relation);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let relation = settled.expect("background child did not settle");
        assert_eq!(relation.state, "completed");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let (view, _) = store.activate(&actor, &child_session, "main").unwrap();
        assert_eq!(view.history.turns.len(), 1);
        assert_eq!(view.history.turns[0].answer, "child answer");
        assert!(store.branches(&actor, &parent.id).unwrap()[0]
            .active_run
            .is_some());
    }

}