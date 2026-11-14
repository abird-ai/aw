use aw::{
    config::Settings, Agent, AgentOptions, CancellationToken, Error, LocalToolConcurrency,
    LocalToolLimits, LocalToolOptions, Result, ToolMode,
};
use aw_agent_core::{
    AgentExecutionBinding, AuxiliaryInput, AuxiliaryKind, ExecutionOwnershipLease,
    InvocationIdentity, JournalEntry, JournalFuture, ProviderAccounts, ProviderSelection, Tool,
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
    execution_ref::{ExecutionRef, ExecutionSegment, Handle, ResolvedExecutionRef},
    filesystem::schema::Tool as FsTool,
    operation::OperationId,
    remote_start::{
        RemoteSessionAction, RemoteStartIntent, REMOTE_START_VERSION,
        arguments_sha256 as remote_arguments_sha256,
    },
};
use aw_tools_adapter::operation_router::OperationRouter;
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

struct PrepareRequest {
    args: SpawnArgs,
    mode: SubagentMode,
    attach_legacy_child_actor: bool,
    constructor_fingerprint: Option<String>,
    tool_mode_override: Option<ToolMode>,
    allow_recursive_tools: bool,
    policy_gate: Option<PolicyGateSpec>,
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
    store: SessionStore,
    service: Option<Service>,
    authority: aw_agent_core::tool::ToolBindingAuthority,
    config: PolicyOperationConfig,
    remote_sites: BTreeMap<String, aw_mcp::RemoteBridge>,
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
            store: service.inner.store.clone(),
            service: Some(service),
            authority,
            config,
            remote_sites: BTreeMap::new(),
        })
    }

    pub fn local(
        store: SessionStore,
        authority: aw_agent_core::tool::ToolBindingAuthority,
        config: PolicyOperationConfig,
    ) -> Result<Self> {
        if config.enabled {
            PolicyOperationConfig::deterministic(config.policy, config.revision)?;
        }
        Ok(Self {
            store,
            service: None,
            authority,
            config,
            remote_sites: BTreeMap::new(),
        })
    }

    fn delegation_service(&self) -> Result<&Service> {
        self.service.as_ref().ok_or(Error::Denied(
            "operation route requires unavailable Agent delegation/guardian service",
        ))
    }

    pub fn with_remote_site(
        mut self,
        site: impl Into<String>,
        bridge: aw_mcp::RemoteBridge,
    ) -> Result<Self> {
        let site = site.into();
        if site.is_empty()
            || site.len() > 128
            || site.chars().any(char::is_control)
            || site == "local"
            || self.remote_sites.insert(site, bridge).is_some()
        {
            return Err(Error::Denied("invalid or duplicate remote execution site"));
        }
        Ok(self)
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

    fn request_digest(operation: OperationId, arguments: &Value) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"aw-policy-operation-semantic-request-v1\0");
        hash.update(operation.semantic_name().as_bytes());
        hash.update(Self::canonical_json_hash(arguments));
        hash.finalize().into()
    }

    fn request_sha256(operation: OperationId, arguments: &Value) -> String {
        let digest = Self::request_digest(operation, arguments);
        let mut encoded = String::with_capacity(64);
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in digest {
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
        encoded
    }

    fn remote_wire_ref(canonical: &str) -> Result<String> {
        let resolved = ExecutionRef::parse(canonical)
            .map_err(|_| Error::Denied("invalid canonical remote Job ref"))?
            .resolve(None, None)
            .map_err(|_| Error::Denied("remote Job ref must already be canonical"))?;
        match resolved.suffix() {
            [ExecutionSegment::Job(job)] => Ok(format!("j:{}", job.as_str())),
            [ExecutionSegment::Job(job), ExecutionSegment::Node(node)] => {
                Ok(format!("j:{}/n:{}", job.as_str(), node.get()))
            }
            _ => Err(Error::Denied(
                "remote dotlink control/read requires an exact finite Job or node ref",
            )),
        }
    }

    fn remote_arguments(
        operation: OperationId,
        canonical_target: &str,
        arguments: &Value,
    ) -> Result<Value> {
        let mut remote = arguments
            .as_object()
            .cloned()
            .ok_or(Error::Denied("remote operation arguments must be an object"))?;
        match operation {
            OperationId::JobsGet
            | OperationId::JobsRead
            | OperationId::JobsAck
            | OperationId::JobsSend
            | OperationId::JobsCancel => {
                remote.insert(
                    "ref".into(),
                    Value::String(Self::remote_wire_ref(canonical_target)?),
                );
            }
            OperationId::JobsList
            | OperationId::Fs(_)
            | OperationId::Bash
            | OperationId::Pwsh
            | OperationId::Powershell
            | OperationId::Batch => {}
            _ => return Err(Error::Denied("operation has no initial dotlink bridge mapping")),
        }
        Ok(Value::Object(remote))
    }

    fn remote_read(operation: OperationId) -> bool {
        match operation {
            OperationId::Fs(tool) => {
                tool.action() == aw_tools::filesystem::access::Action::Read
            }
            OperationId::JobsList | OperationId::JobsGet | OperationId::JobsRead => true,
            _ => false,
        }
    }

    fn remote_effect(operation: OperationId) -> bool {
        match operation {
            OperationId::Fs(tool) => {
                tool.action() != aw_tools::filesystem::access::Action::Read
            }
            OperationId::Bash
            | OperationId::Pwsh
            | OperationId::Powershell
            | OperationId::Batch
            | OperationId::JobsAck
            | OperationId::JobsSend
            | OperationId::JobsCancel => true,
            _ => false,
        }
    }

    fn remote_session_action(operation: OperationId) -> Result<RemoteSessionAction> {
        if matches!(
            operation,
            OperationId::JobsAck | OperationId::JobsSend | OperationId::JobsCancel
        ) {
            Ok(RemoteSessionAction::Control)
        } else if matches!(
            operation,
            OperationId::Fs(_)
                | OperationId::Bash
                | OperationId::Pwsh
                | OperationId::Powershell
                | OperationId::Batch
        ) {
            Ok(RemoteSessionAction::Submit)
        } else {
            Err(Error::Denied("operation does not require remote Start"))
        }
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
        let selected_remote_site = self.remote_sites.contains_key(&ctx.execution_site);
        if !self.config.enabled && !selected_remote_site {
            return original.execute(ctx, &arguments).await;
        }
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

        if ctx.actor != ctx.identity.actor || ctx.source != aw_agent_core::ToolSource::trusted_host() {
            return Err(Error::Denied(
                "policy operation source/actor is not an authenticated Host invocation",
            ));
        }
        let invocation = format!("turn:{}/call:{}", ctx.turn_id, ctx.call_id);
        if Self::remote_effect(operation) {
            if let Some(existing) = self.store.remote_start_lookup_attempt(
                &ctx.actor,
                &scope.session_id,
                &invocation,
                &ctx.call_id,
            )? {
                if existing.intent.original_request_sha256
                    != Self::request_digest(operation, &arguments)
                {
                    return Err(Error::Conflict(
                        "remote Start replay identity was reused with a changed original request",
                    ));
                }
                let bridge = self
                    .remote_sites
                    .get(&existing.provenance.source.site)
                    .ok_or(Error::Conflict(
                        "committed remote Start site is no longer connected; preserve the original attempt",
                    ))?;
                let remote_arguments =
                    Self::remote_arguments(operation, &existing.intent.canonical_ref, &arguments)?;
                return bridge
                    .call_effect(&ctx, operation, &remote_arguments, &existing)
                    .await;
            }
        }

        original.validate(&arguments)?;
        if ctx.execution_site.is_empty() {
            return Err(Error::Denied(
                "policy operation execution site is not an authenticated Host invocation",
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

        let session = self.store.session(&ctx.actor, &scope.session_id)?;
        let writer = self.store.workspace_writer(
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
            Some(self.delegation_service()?.exact_selection(&ctx)?)
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
        let claimed = self.store.policy_admission_claim(&ctx.actor, request)?;

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
                    .delegation_service()?
                    .inner
                    .settings
                    .request_timeout
                    .min(Duration::from_secs(60)),
            };
            aw::policy_runtime::run_admission_guardian(
                aw::policy_runtime::AdmissionGuardianRequest {
                    store: &self.store,
                    actor: &ctx.actor,
                    request: claimed.request.clone(),
                    authorized,
                    selected,
                    input: &input,
                    decision_revision: self.config.revision,
                },
                ctx.cancellation.clone().cancelled(),
            )
            .await?
        } else {
            match claimed.state {
                PolicyAdmissionState::Claimed => {
                    let route = effective.strategy();
                    let digest = Self::decision_digest(self.config.revision, route, "allow");
                    self.store.policy_admission_record_decision(
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
        let remote_bridge = if strategy == aw_policy::Strategy::Local {
            self.remote_sites.get(&ctx.execution_site).cloned()
        } else {
            None
        };
        let remote_arguments = remote_bridge
            .as_ref()
            .map(|_| Self::remote_arguments(operation, &target, &arguments))
            .transpose()?;
        if remote_bridge.is_some()
            && !Self::remote_read(operation)
            && !Self::remote_effect(operation)
        {
            return Err(Error::Denied("operation is not supported by the initial dotlink bridge"));
        }
        let remote_intent = match remote_bridge.as_ref() {
            Some(bridge) if Self::remote_effect(operation) => {
                let lease = bridge.lease().await?;
                let wire_arguments = remote_arguments
                    .as_ref()
                    .ok_or(Error::Protocol("remote arguments disappeared"))?;
                Some(RemoteStartIntent {
                    version: REMOTE_START_VERSION,
                    original_invocation_id: invocation.clone(),
                    turn_id: ctx.turn_id,
                    call_id: ctx.call_id.clone(),
                    effect_attempt_id: ctx.call_id.clone(),
                    operation: operation.semantic_name().into(),
                    canonical_ref: target.clone(),
                    arguments_sha256: remote_arguments_sha256(wire_arguments),
                    original_request_sha256: Self::request_digest(operation, &arguments),
                    session_action: Self::remote_session_action(operation)?,
                    target: lease,
                })
            }
            _ => None,
        };

        evidence.revalidate(&self.authority)?;
        let consumed = self.store.policy_admission_consume(
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

        if let Some(bridge) = remote_bridge {
            let wire_arguments = remote_arguments
                .as_ref()
                .ok_or(Error::Protocol("remote arguments disappeared"))?;
            if Self::remote_read(operation) {
                return bridge.call_read(&ctx, operation, wire_arguments).await;
            }
            let intent = remote_intent.ok_or(Error::Protocol("remote Start intent disappeared"))?;
            let authorization = aw::policy_runtime::authorize_remote_start(
                &self.store,
                &self.authority,
                &ctx.actor,
                &evidence,
                &scope.session_id,
                &consumed.id,
                intent,
            )?;
            return bridge
                .call_effect(&ctx, operation, wire_arguments, &authorization)
                .await;
        }

        let original_executor = Arc::new(PreparedOriginalOperation {
            original,
            ctx: ctx.clone(),
            arguments,
        });
        let delegated = if strategy == aw_policy::Strategy::DelegateAgent {
            let request = Self::delegation_request(operation, &ctx, &target)?;
            let service = self.delegation_service()?;
            Some(DelegateExecution {
                request,
                port: service.policy_delegate_port(&ctx),
                delegate_binding: None,
                delegate_admission: service.policy_delegate_admission(&ctx),
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
                    PrepareRequest {
                        args: SpawnArgs {
                            task: task.clone(),
                            title: Some(format!("Delegate: {}", operation.semantic_name())),
                        },
                        mode: SubagentMode::Foreground,
                        attach_legacy_child_actor: true,
                        constructor_fingerprint: None,
                        tool_mode_override: Some(ToolMode::Off),
                        allow_recursive_tools: false,
                        policy_gate: Some(gate_spec),
                    },
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
        };
        let mode = if background {
            SubagentMode::Background
        } else {
            SubagentMode::Foreground
        };
        Ok((
            self.prepare(
                ctx,
                PrepareRequest {
                    args: spawn,
                    mode,
                    attach_legacy_child_actor: false,
                    constructor_fingerprint: Some(constructor_fingerprint.to_owned()),
                    tool_mode_override: None,
                    allow_recursive_tools: true,
                    policy_gate: None,
                },
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
        request: PrepareRequest,
    ) -> Result<Prepared> {
        let PrepareRequest {
            args,
            mode,
            attach_legacy_child_actor,
            constructor_fingerprint,
            tool_mode_override,
            allow_recursive_tools,
            policy_gate,
        } = request;
        let constructor_fingerprint = constructor_fingerprint.as_deref();
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
                service.attach_delegated(crate::child_actors::DelegatedAttachRequest {
                    durable: &info,
                    agent: &agent,
                    selected: &selected,
                    task: &args.task,
                    next_turn: turn,
                    gate_name: &gate.name,
                    gate_effect: kernel_effect(&gate.effect),
                })
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
                .unwrap_or_default();
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
                Service::spawn_stable_background_worker(
                    self.service.inner.clone(),
                    prepared,
                    runtime_state,
                    active,
                )?;
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
        sync::{
            atomic::{AtomicUsize, Ordering},
            Mutex,
        },
        time::Duration,
    };

    async fn execute_agent_fixture(
        service: &Service,
        mut ctx: ToolContext,
        arguments: &Value,
    ) -> Result<ToolOutput> {
        if service.agent_jobs_router().is_err() {
            service.bind_agent_jobs_router(Arc::new(aw::agent_jobs::Router::new(
                service.inner.store.clone(),
                ctx.actor.clone(),
                LocalToolLimits::default().jobs.max_retained_jobs,
            )?))?;
        }
        if ctx.execution_owner.is_none() {
            let scope = ctx
                .scope
                .as_ref()
                .ok_or(Error::Denied("Agent fixture requires a durable parent scope"))?;
            ctx.execution_owner = Some(aw_agent_core::AgentExecutionBinding::from_trusted_host(
                "/d:0/i:0/p:0/a:0",
                scope.session_id.clone(),
                scope.branch_id.clone(),
            )?);
        }
        service.agent_tool().execute(ctx, arguments).await
    }

    #[derive(Clone, Copy, Debug)]
    enum ModelFirstWorkflow {
        Read,
        Graph,
        BashHandoff,
    }

    impl ModelFirstWorkflow {
        fn label(self) -> &'static str {
            match self {
                Self::Read => "read",
                Self::Graph => "graph3",
                Self::BashHandoff => "bash_handoff_jobs",
            }
        }
    }

    struct ModelFirstProvider {
        workflow: ModelFirstWorkflow,
        calls: Arc<AtomicUsize>,
        first_tool: Arc<Mutex<Option<String>>>,
        job_ref: Arc<Mutex<Option<String>>>,
    }

    impl ModelFirstProvider {
        fn tool_reply(call_id: &str, name: &str, arguments: Value) -> ModelReply {
            ModelReply {
                meta: CompletionMeta {
                    finish_reason: "tool_calls".into(),
                    usage: Usage {
                        input_tokens: Some(8),
                        output_tokens: Some(4),
                    },
                },
                tool_calls: vec![ToolCall {
                    id: call_id.into(),
                    name: name.into(),
                    arguments,
                }],
                provider_context: None,
            }
        }

        fn final_reply(sink: &mut DeltaSink<'_>, answer: &str) -> Result<ModelReply> {
            sink(answer)?;
            Ok(ModelReply {
                meta: CompletionMeta {
                    finish_reason: "stop".into(),
                    usage: Usage {
                        input_tokens: Some(8),
                        output_tokens: Some(4),
                    },
                },
                ..Default::default()
            })
        }

        fn last_tool_content(messages: &[Message]) -> Result<&str> {
            messages
                .iter()
                .rev()
                .find(|message| message.role == Role::Tool)
                .map(|message| message.content.as_str())
                .ok_or(Error::Protocol("model-first fixture expected factual tool result"))
        }

        fn capture_job_ref(&self, messages: &[Message]) -> Result<String> {
            let content = Self::last_tool_content(messages)?;
            let reference = content
                .split_whitespace()
                .next()
                .filter(|reference| {
                    reference.starts_with("j:")
                        || (reference.starts_with("/d:") && reference.contains("/j:"))
                })
                .ok_or(Error::Protocol("Bash handoff did not return compact/canonical Job ref"))?
                .to_owned();
            *self.job_ref.lock().unwrap() = Some(reference.clone());
            Ok(reference)
        }
    }

    impl Provider for ModelFirstProvider {
        fn supports_tools(&self) -> bool {
            true
        }

        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("model-first fixture requires tool protocol")) })
        }

        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            _: &'a str,
            tools: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                let round = self.calls.fetch_add(1, Ordering::SeqCst);
                let names = tools
                    .iter()
                    .map(|tool| tool.name.as_str())
                    .collect::<std::collections::BTreeSet<_>>();
                for removed in ["job_start", "job_restart", "subagent", "team", "plan", "review_release"] {
                    assert!(!names.contains(removed), "removed route leaked into provider catalog: {removed}");
                }
                let reply = match (self.workflow, round) {
                    (ModelFirstWorkflow::Read, 0) => {
                        assert!(names.contains("read"));
                        *self.first_tool.lock().unwrap() = Some("read".into());
                        Self::tool_reply(
                            "mf-read",
                            "read",
                            json!({"path":"read.txt","numbered":false}),
                        )
                    }
                    (ModelFirstWorkflow::Read, 1) => {
                        return Self::final_reply(sink, "read workflow complete")
                    }
                    (ModelFirstWorkflow::Graph, 0) => {
                        assert!(names.contains("batch"));
                        *self.first_tool.lock().unwrap() = Some("batch".into());
                        Self::tool_reply(
                            "mf-graph",
                            "batch",
                            json!({"ops":[
                                ["read",{"path":"a.txt","numbered":false},"*"],
                                ["read",{"path":"b.txt","numbered":false},"*"],
                                ["read",{"path":"c.txt","numbered":false},"*"]
                            ]}),
                        )
                    }
                    (ModelFirstWorkflow::Graph, 1) => {
                        return Self::final_reply(sink, "graph workflow complete")
                    }
                    (ModelFirstWorkflow::BashHandoff, 0) => {
                        assert!(names.contains("bash"));
                        *self.first_tool.lock().unwrap() = Some("bash".into());
                        Self::tool_reply(
                            "mf-bash",
                            "bash",
                            json!({"command":"printf 0123456789","background":true}),
                        )
                    }
                    (ModelFirstWorkflow::BashHandoff, 1) => {
                        assert!(names.contains("jobs_get"));
                        let reference = self.capture_job_ref(messages)?;
                        Self::tool_reply(
                            "mf-jobs-get",
                            "jobs_get",
                            json!({"ref":reference,"wait_ms":3000}),
                        )
                    }
                    (ModelFirstWorkflow::BashHandoff, 2) => {
                        assert!(names.contains("jobs_read"));
                        let reference = self
                            .job_ref
                            .lock()
                            .unwrap()
                            .clone()
                            .ok_or(Error::Protocol("model-first fixture lost Job ref"))?;
                        Self::tool_reply(
                            "mf-jobs-read",
                            "jobs_read",
                            json!({
                                "ref":reference,"view":"stdout","offset":0,"limit":4,"encoding":"text"
                            }),
                        )
                    }
                    (ModelFirstWorkflow::BashHandoff, 3) => {
                        assert!(names.contains("jobs_ack"));
                        let content = Self::last_tool_content(messages)?;
                        assert!(content.contains("0123"), "{content}");
                        let offset = content
                            .split_whitespace()
                            .find_map(|part| part.strip_prefix("offset="))
                            .and_then(|value| value.parse::<u64>().ok())
                            .ok_or(Error::Protocol("jobs_read did not expose continuation offset"))?;
                        assert_eq!(offset, 4);
                        let reference = self
                            .job_ref
                            .lock()
                            .unwrap()
                            .clone()
                            .ok_or(Error::Protocol("model-first fixture lost Job ref"))?;
                        Self::tool_reply(
                            "mf-jobs-ack",
                            "jobs_ack",
                            json!({"ref":reference,"view":"stdout","offset":offset}),
                        )
                    }
                    (ModelFirstWorkflow::BashHandoff, 4) => {
                        return Self::final_reply(sink, "bash handoff workflow complete")
                    }
                    _ => return Err(Error::Protocol("unexpected model-first provider round")),
                };
                Ok(reply)
            })
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn model_first_local_workflows_measure_first_valid_call_rounds_and_result_bytes() {
        let bash = std::env::var_os("AW_BASH")
            .map(PathBuf::from)
            .expect("model-first Bash workflow requires pinned AW_BASH");
        let mut first_valid = 0usize;
        let workflows = [
            (ModelFirstWorkflow::Read, "read", 2usize, vec!["read"]),
            (ModelFirstWorkflow::Graph, "batch", 2usize, vec!["batch"]),
            (
                ModelFirstWorkflow::BashHandoff,
                "bash",
                5usize,
                vec!["bash", "jobs_get", "jobs_read", "jobs_ack"],
            ),
        ];

        let workflow_count = workflows.len();
        for (workflow, expected_first, expected_rounds, expected_tools) in workflows {
            let root = std::env::temp_dir().join(format!(
                "aw-model-first-{}-{}-{}",
                workflow.label(),
                std::process::id(),
                unix_micros()
            ));
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join("read.txt"), "read-value\n").unwrap();
            std::fs::write(root.join("a.txt"), "a\n").unwrap();
            std::fs::write(root.join("b.txt"), "b\n").unwrap();
            std::fs::write(root.join("c.txt"), "c\n").unwrap();

            let calls = Arc::new(AtomicUsize::new(0));
            let first_tool = Arc::new(Mutex::new(None));
            let provider = Arc::new(ModelFirstProvider {
                workflow,
                calls: calls.clone(),
                first_tool: first_tool.clone(),
                job_ref: Arc::new(Mutex::new(None)),
            });
            let actor = PrincipalId::new("local").unwrap();
            let builder = Agent::builder(
                provider,
                AgentOptions::new("fixture-account", "fixture-model"),
            );
            let mut options = LocalToolOptions::new(&root, ToolMode::Trusted);
            options.bash = Some(bash.clone());
            options.selected_tools = vec!["read".into(), "bash".into(), "batch".into()];
            let mut agent = aw_tools_adapter::configure(builder, actor.clone(), &options)
                .unwrap()
                .build()
                .unwrap();
            let store = SessionStore::in_memory().unwrap();
            let (session, branch) = store
                .create(
                    &actor,
                    &format!("model-first {}", workflow.label()),
                    aw_store::SessionBinding {
                        working_directory: root.clone(),
                        tool_root: root.clone(),
                        system_prompt: "model-first fixture".into(),
                        profile_hint: "fixture-account".into(),
                        model_hint: "fixture-model".into(),
                        destination_fingerprint: "0".repeat(64),
                    },
                )
                .unwrap();
            let (view, handle) = store.activate(&actor, &session.id, &branch.id).unwrap();
            agent
                .load_history(view.history, Some(Arc::new(handle)))
                .unwrap();
            let binding = store
                .agent_binding(&actor, &session.id, &branch.id)
                .unwrap();
            agent
                .bind_execution_owner(
                    AgentExecutionBinding::from_trusted_host(
                        format!(
                            "/d:{}/i:{}/p:{}/a:{}",
                            binding.domain_handle,
                            binding.identity_handle,
                            binding.project_handle,
                            binding.agent_handle,
                        ),
                        binding.session_id,
                        binding.branch_id,
                    )
                    .unwrap(),
                )
                .unwrap();
            let turn = tokio::time::timeout(
                Duration::from_secs(8),
                agent.prompt(
                    &format!("model-first workflow {}", workflow.label()),
                    std::future::pending::<()>(),
                    |_| Ok(()),
                ),
            )
            .await
            .expect("model-first workflow timed out")
            .unwrap();
            assert_eq!(
                turn.outcome,
                aw::conversation::Outcome::Completed,
                "workflow={} answer={} tools={:?}",
                workflow.label(),
                turn.answer,
                turn.tool_records
                    .iter()
                    .map(|record| (
                        record.call.name.as_str(),
                        record.status.clone(),
                        record.output.as_ref().map(|output| output.content.as_str())
                    ))
                    .collect::<Vec<_>>()
            );
            let actual_tools = turn
                .tool_records
                .iter()
                .map(|record| record.call.name.as_str())
                .collect::<Vec<_>>();
            assert_eq!(actual_tools, expected_tools);
            assert_eq!(calls.load(Ordering::SeqCst), expected_rounds);
            if first_tool.lock().unwrap().as_deref() == Some(expected_first) {
                first_valid += 1;
            }
            let result_bytes = turn
                .tool_records
                .iter()
                .filter_map(|record| record.output.as_ref())
                .map(|output| output.content.len())
                .sum::<usize>();
            println!(
                "model-first workflow={} first={} provider_rounds={} tool_calls={} result_bytes={} approx_result_tokens={}",
                workflow.label(),
                expected_first,
                expected_rounds,
                turn.tool_records.len(),
                result_bytes,
                result_bytes.div_ceil(4),
            );
            let _ = std::fs::remove_dir_all(root);
        }
        assert_eq!(first_valid, workflow_count);
        println!(
            "model-first first_valid_call_rate={}/{} deterministic_schema_valid=100%",
            first_valid,
            workflow_count
        );
    }

    struct ModelFirstAgentProvider {
        parent_calls: Arc<AtomicUsize>,
        child_calls: Arc<AtomicUsize>,
        first_tool: Arc<Mutex<Option<String>>>,
    }

    impl ModelFirstAgentProvider {
        fn is_child(messages: &[Message]) -> bool {
            messages
                .iter()
                .any(|message| message.role == Role::User && message.content == "model-first child task")
        }
    }

    impl Provider for ModelFirstAgentProvider {
        fn supports_tools(&self) -> bool {
            true
        }

        fn stream<'a>(
            &'a self,
            messages: &'a [Message],
            _: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                if Self::is_child(messages) {
                    self.child_calls.fetch_add(1, Ordering::SeqCst);
                    sink("model-first child answer")?;
                    return Ok(CompletionMeta {
                        finish_reason: "stop".into(),
                        usage: Usage {
                            input_tokens: Some(6),
                            output_tokens: Some(4),
                        },
                    });
                }
                Err(Error::Protocol("parent Agent workflow requires tool protocol"))
            })
        }

        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            _: &'a str,
            tools: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                if Self::is_child(messages) {
                    self.child_calls.fetch_add(1, Ordering::SeqCst);
                    assert!(
                        tools.iter().all(|tool| tool.name == "jobs_subscribe"),
                        "stable model-only child may expose only jobs_subscribe: {:?}",
                        tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>()
                    );
                    sink("model-first child answer")?;
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

                let round = self.parent_calls.fetch_add(1, Ordering::SeqCst);
                let names = tools
                    .iter()
                    .map(|tool| tool.name.as_str())
                    .collect::<std::collections::BTreeSet<_>>();
                assert!(names.contains("agent"));
                for removed in ["subagent", "team", "plan", "review_release"] {
                    assert!(!names.contains(removed));
                }
                match round {
                    0 => {
                        *self.first_tool.lock().unwrap() = Some("agent".into());
                        Ok(ModelReply {
                            meta: CompletionMeta {
                                finish_reason: "tool_calls".into(),
                                usage: Usage {
                                    input_tokens: Some(8),
                                    output_tokens: Some(4),
                                },
                            },
                            tool_calls: vec![ToolCall {
                                id: "mf-agent".into(),
                                name: "agent".into(),
                                arguments: json!({
                                    "task":"model-first child task",
                                    "background":false
                                }),
                            }],
                            provider_context: None,
                        })
                    }
                    1 => {
                        let result = messages
                            .iter()
                            .rev()
                            .find(|message| message.role == Role::Tool)
                            .ok_or(Error::Protocol("parent Agent workflow missing child result"))?;
                        assert!(result.content.contains("model-first child answer"));
                        sink("parent Agent workflow complete")?;
                        Ok(ModelReply {
                            meta: CompletionMeta {
                                finish_reason: "stop".into(),
                                usage: Usage {
                                    input_tokens: Some(8),
                                    output_tokens: Some(4),
                                },
                            },
                            ..Default::default()
                        })
                    }
                    _ => Err(Error::Protocol("unexpected parent Agent workflow round")),
                }
            })
        }
    }

    #[tokio::test]
    async fn model_first_agent_constructor_measures_parent_and_child_rounds() {
        let actor = PrincipalId::new("local:model-first-agent").unwrap();
        let parent_calls = Arc::new(AtomicUsize::new(0));
        let child_calls = Arc::new(AtomicUsize::new(0));
        let first_tool = Arc::new(Mutex::new(None));
        let provider = Arc::new(ModelFirstAgentProvider {
            parent_calls: parent_calls.clone(),
            child_calls: child_calls.clone(),
            first_tool: first_tool.clone(),
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

        let store = SessionStore::in_memory().unwrap();
        let (parent, branch) = store
            .create(&actor, "model-first Agent parent", binding())
            .unwrap();
        let (view, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        let mut agent = Agent::builder_from_selection(
            selected,
            AgentOptions::new("fixture-account", "fixture-model"),
        )
        .build()
        .unwrap();
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
        agent.register_host_tool(service.agent_tool()).unwrap();

        let turn = tokio::time::timeout(
            Duration::from_secs(8),
            agent.prompt(
                "model-first Agent workflow",
                std::future::pending::<()>(),
                |_| Ok(()),
            ),
        )
        .await
        .expect("model-first Agent workflow timed out")
        .unwrap();
        assert_eq!(turn.outcome, aw::conversation::Outcome::Completed);
        assert_eq!(turn.answer, "parent Agent workflow complete");
        assert_eq!(turn.tool_records.len(), 1);
        assert_eq!(turn.tool_records[0].call.name, "agent");
        assert_eq!(first_tool.lock().unwrap().as_deref(), Some("agent"));
        assert_eq!(parent_calls.load(Ordering::SeqCst), 2);
        assert_eq!(child_calls.load(Ordering::SeqCst), 1);
        let result_bytes = turn.tool_records[0]
            .output
            .as_ref()
            .map_or(0, |output| output.content.len());
        println!(
            "model-first workflow=agent first=agent parent_provider_rounds=2 child_provider_rounds=1 total_provider_rounds=3 tool_calls=1 result_bytes={} approx_result_tokens={}",
            result_bytes,
            result_bytes.div_ceil(4),
        );
        drop(router);
    }

    #[test]
    fn final_agent_constructor_schema_footprint_is_measured() {
        let actor = PrincipalId::new("local:schema-agent").unwrap();
        let accounts = ProviderAccounts::new("local").unwrap();
        struct ProviderFixture;
        impl Provider for ProviderFixture {
            fn stream<'a>(
                &'a self,
                _: &'a [Message],
                _: &'a str,
                _: &'a mut DeltaSink<'_>,
            ) -> CompletionFuture<'a> {
                Box::pin(async { Err(Error::Protocol("provider unused")) })
            }
        }
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor,
                    "local",
                    ["fixture-model".into()],
                ),
                Arc::new(ProviderFixture),
            )
            .unwrap();
        let service = Service::new(
            SessionStore::in_memory().unwrap(),
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
        let spec = service.agent_tool().spec();
        let bytes = serde_json::to_vec(&spec).unwrap().len();
        println!(
            "final-agent-schema tools=1 bytes={} approx_tokens={} name={}",
            bytes,
            bytes.div_ceil(4),
            spec.name
        );
        assert_eq!(spec.name, "agent");
        assert_eq!(spec.parameters["required"], json!(["task"]));
        assert!(spec.parameters["properties"].get("mode").is_none());
    }

    #[test]
    fn remote_dotlink_projection_keeps_only_finite_job_refs_and_exact_fs_arguments() {
        let canonical_job = "/d:local/i:owner/p:repo/a:0/j:job-1";
        let canonical_node = "/d:local/i:owner/p:repo/a:0/j:job-1/n:3";
        assert_eq!(
            PolicyOperationRouter::remote_wire_ref(canonical_job).unwrap(),
            "j:job-1"
        );
        assert_eq!(
            PolicyOperationRouter::remote_wire_ref(canonical_node).unwrap(),
            "j:job-1/n:3"
        );
        let projected = PolicyOperationRouter::remote_arguments(
            OperationId::JobsRead,
            canonical_node,
            &json!({"ref":".","view":"stdout","offset":0,"limit":128}),
        )
        .unwrap();
        assert_eq!(projected["ref"], "j:job-1/n:3");
        let fs = json!({"path":"src/lib.rs","line":1,"lines":10,"numbered":false});
        assert_eq!(
            PolicyOperationRouter::remote_arguments(
                OperationId::Fs(FsTool::Read),
                "/d:local/i:owner/p:repo/a:0/t:1",
                &fs,
            )
            .unwrap(),
            fs
        );
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
                if messages.iter().any(|message| message.content == "level2 task") {
                    assert_eq!(names, vec!["jobs_subscribe"]);
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
                assert_eq!(names, vec!["agent", "jobs_subscribe"]);
                assert!(messages.iter().any(|message| message.content == "level1 task"));
                self.calls.fetch_add(1, Ordering::SeqCst);
                if messages.last().is_some_and(|message| message.role == Role::Tool) {
                    let result = messages.last().unwrap();
                    assert_eq!(result.tool_call_id.as_deref(), Some("recursive-leaf"));
                    assert!(result.content.contains(" agent idle "), "{}", result.content);
                    assert!(result.content.contains("level2 answer"), "{}", result.content);
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
                            name: "agent".into(),
                            arguments: json!({
                                "task": "level2 task",
                                "title": "Leaf child",
                                "background": false
                            }),
                        }],
                        provider_context: None,
                    })
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
                assert_eq!(names, vec!["edit", "jobs_subscribe", "read", "write"]);
                assert!(!names.contains(&"agent"));
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
                assert_eq!(names, vec!["jobs_subscribe", "read"]);
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
            name: "agent".into(),
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
        let local_config = PolicyOperationConfig::deterministic(local_policy, 11).unwrap();
        assert_eq!(local_config.policy, local_policy);
        assert_eq!(local_config.revision, 11);
        assert!(!local_config.guardian);
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
            id: "call-recursive-root".into(), name: "agent".into(),
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
                            name: "agent".into(),
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
        let output = execute_agent_fixture(&service,
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
                &json!({"task":"level1 task","title":"First child","background":false}),
            )
            .await
            .unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains(" agent idle "), "{}", output.content);
        assert!(output.content.contains("level1 answer"), "{}", output.content);
        assert_eq!(calls.load(Ordering::SeqCst), 3);

        let first = store
            .subagents_for_parent(&actor, &parent.id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(first.depth, Some(1));
        assert_eq!(first.state, "running");
        let second = store
            .subagents_for_parent(&actor, &first.child_session_id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(second.depth, Some(2));
        assert_eq!(second.state, "running");
        assert_eq!(second.parent_session_id, first.child_session_id);
        assert_eq!(store.subagent(&actor,&first.id).unwrap().state,"running");
        assert_eq!(store.subagent(&actor,&second.id).unwrap().state,"running");

        assert_eq!(second.delegation_root_id, first.delegation_root_id);
        let root = store
            .delegation_root(&actor, first.delegation_root_id.as_deref().unwrap())
            .unwrap();
        assert_eq!(root.children_created, 2);
        assert_eq!(root.active_children, 2);
        assert_eq!(root.model_attempts_started, 3);
        assert_eq!(root.model_attempts_finished, 3);
        assert_eq!(root.limits.max_depth, 2);
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
            name: "agent".into(),
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
                            name: "agent".into(),
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
        let output = execute_agent_fixture(&service,
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
                    "background":false
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
        assert_eq!(relation.state, "running");
        assert_eq!(relation.tool_mode,"workspace");
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
            .all(|record| record.call.name != "agent" && record.call.name != "bash"));
        assert_eq!(execution.state, "bound");
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
            id: "call-subagent-read-only".into(), name: "agent".into(),
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
                            name: "agent".into(),
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
        let output = execute_agent_fixture(&service,
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
                &json!({"task":"read task","background":false}),
            )
            .await
            .unwrap();
        assert!(output.content.contains(" agent idle "), "{}", output.content);
        assert!(
            output.content.contains("read-only child answer"),
            "{}",
            output.content
        );
        let relation = store
            .subagents_for_parent(&actor, &parent.id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(relation.state, "running");
        assert_eq!(relation.tool_mode, "read-only");
        let execution = store
            .subagent_execution_binding_for_subagent(&actor, &relation.id)
            .unwrap()
            .unwrap();
        assert_eq!(execution.state, "bound");
        assert!(execution.execution_root.join("base.txt").exists());
        assert!(store.session(&actor, &relation.child_session_id).is_ok());
    }

}
