//! Typed tools are proposals, never executable streamed fragments. Native
//! implementations are trusted Rust code; ToolPolicy is an application grant,
//! not an OS filesystem/network sandbox.
use crate::{CancellationToken, Error, InvocationIdentity, InvocationScope, PrincipalId, Result};
use aw_agent_kernel::capability::{Capability, Catalog, Effect, Grant, Source, SourceKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};
pub const MAX_TOOL_CALLS_PER_ROUND: usize = aw_agent_kernel::turn::MAX_BATCH;
pub const MAX_TOOL_ARGUMENT_BYTES: usize = 64 * 1024;
pub const MIN_TOOL_RESULT_BYTES: usize = 512;
pub const MAX_TOOL_RESULT_BYTES: usize = 128 * 1024;
pub const MAX_TRACE_BYTES: usize = 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolEffect {
    Read,
    Write,
    Process,
    /// Lifecycle/input/subscription authority. This never inherits filesystem
    /// Read/Write permission and remains independently source/site granted.
    Control,
    /// Conservative default for effects whose safety is not established by
    /// the trusted host. Untrusted tool hints never downgrade Unknown to Read.
    Unknown,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub effect: ToolEffect,
}
/// Authoritative host-only approval for a transformed tool proposal. This
/// never changes Source/Effect/ToolPolicy/Envelope: those are rechecked
/// independently. No revision policy is installed by default. The host
/// must make a semantically informed authorization decision; removing JSON
/// fields is NOT universally a narrower operation.
pub trait ToolRevisionPolicy: Send + Sync {
    fn authorize(
        &self,
        original: &ToolCall,
        revised: &ToolCall,
        spec: &ToolSpec,
        source: &ToolSource,
        identity: &crate::InvocationIdentity,
    ) -> Result<()>;
}
/// Prevent a panicking host policy from unwinding across an in-flight
/// journaled Agent Turn. A policy is trusted local Rust code, not a
/// source-of-authority supplied by a model or extension.
pub(crate) fn check_revision_policy(
    policy: &dyn ToolRevisionPolicy,
    original: &ToolCall,
    revised: &ToolCall,
    spec: &ToolSpec,
    source: &ToolSource,
    identity: &crate::InvocationIdentity,
) -> Result<()> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        policy.authorize(original, revised, spec, source, identity)
    }))
    .map_err(|_| Error::Denied("host tool-revision policy panicked"))?
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}
impl ToolCall {
    pub fn validate_envelope(&self) -> Result<()> {
        for s in [&self.id, &self.name] {
            if s.is_empty()
                || s.len() > 128
                || !s
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_.:-".contains(&c))
            {
                return Err(Error::Protocol("invalid tool-call ID/name"));
            }
        }
        if !self.arguments.is_object()
            || serde_json::to_vec(&self.arguments)
                .map_err(|_| Error::Protocol("invalid tool arguments"))?
                .len()
                > MAX_TOOL_ARGUMENT_BYTES
        {
            return Err(Error::Limit("tool arguments must be a bounded JSON object"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
    pub truncated: bool,
}
impl ToolOutput {
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
            truncated: false,
        }
    }
    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
            truncated: false,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.content.len() > MAX_TOOL_RESULT_BYTES {
            Err(Error::Limit("tool output too large"))
        } else {
            Ok(())
        }
    }
}
/// One exact portable capability predicate is shared by ToolPolicy and the
/// Actor Envelope ceiling. An envelope can only NARROW current host grants.
pub(crate) fn envelope_permits_tool(
    envelope: &aw_agent_kernel::envelope::Envelope,
    name: &str,
    source: &ToolSource,
    effect: &ToolEffect,
) -> bool {
    use aw_agent_kernel::envelope::{Access, ExactGrant};
    let effect = kernel_effect(effect);
    // Filesystem access and execution Control are orthogonal. ReadOnly blocks
    // filesystem mutation, not an independently exact-granted lifecycle/input
    // capability. Unknown remains conservative and Process keeps its own gate.
    if (envelope.spec().access == Access::ReadOnly
        && matches!(effect, Effect::Write | Effect::Unknown))
        || ((effect == Effect::Process || effect == Effect::Unknown) && !envelope.spec().process)
    {
        return false;
    }
    envelope.allows(&ExactGrant {
        name: name.to_owned(),
        kind: source.kind,
        source: source.id.clone(),
        source_epoch: source.epoch,
        effect,
    })
}
struct ExecutionOwnershipState {
    active: AtomicUsize,
    unresolved: AtomicBool,
    notify: tokio::sync::Notify,
    cancellation: CancellationToken,
}

/// Ephemeral host-issued ownership of one causal execution scope. This is
/// factual live coordination evidence only: it grants no tool, provider,
/// resource or Job authority and is never a durable Agent identity.
#[derive(Clone)]
pub struct ExecutionOwnershipScope {
    owner_ref: String,
    state: Arc<ExecutionOwnershipState>,
}

impl std::fmt::Debug for ExecutionOwnershipScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionOwnershipScope")
            .field("owner_ref", &self.owner_ref)
            .field("active", &self.active())
            .field("unresolved", &self.is_unresolved())
            .finish()
    }
}

impl ExecutionOwnershipScope {
    pub fn from_trusted_host(
        owner_ref: impl Into<String>,
        cancellation: CancellationToken,
    ) -> Result<Self> {
        let owner_ref = owner_ref.into();
        if !owner_ref.starts_with('/')
            || owner_ref.len() > 2048
            || !owner_ref.is_ascii()
            || owner_ref.chars().any(char::is_control)
        {
            return Err(Error::Denied("invalid execution ownership ref"));
        }
        Ok(Self {
            owner_ref,
            state: Arc::new(ExecutionOwnershipState {
                active: AtomicUsize::new(0),
                unresolved: AtomicBool::new(false),
                notify: tokio::sync::Notify::new(),
                cancellation,
            }),
        })
    }

    pub fn owner_ref(&self) -> &str {
        &self.owner_ref
    }

    pub fn same_instance(&self, other: &Self) -> bool {
        self.owner_ref == other.owner_ref && Arc::ptr_eq(&self.state, &other.state)
    }

    pub fn acquire(&self) -> Result<ExecutionOwnershipLease> {
        self.state
            .active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map_err(|_| Error::Limit("execution ownership work counter exhausted"))?;
        Ok(ExecutionOwnershipLease {
            state: self.state.clone(),
            released: false,
        })
    }

    pub fn active(&self) -> usize {
        self.state.active.load(Ordering::Acquire)
    }

    pub fn mark_unresolved(&self) {
        self.state.unresolved.store(true, Ordering::Release);
        self.state.notify.notify_waiters();
    }

    pub fn is_unresolved(&self) -> bool {
        self.state.unresolved.load(Ordering::Acquire)
    }

    pub fn cancellation(&self) -> CancellationToken {
        self.state.cancellation.clone()
    }

    pub async fn wait_quiescent(&self) {
        loop {
            if self.active() == 0 {
                return;
            }
            let notified = self.state.notify.notified();
            if self.active() == 0 {
                return;
            }
            notified.await;
        }
    }
}

pub struct ExecutionOwnershipLease {
    state: Arc<ExecutionOwnershipState>,
    released: bool,
}

impl ExecutionOwnershipLease {
    pub fn release(mut self) {
        self.release_inner();
    }

    fn release_inner(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        let previous = self.state.active.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "execution ownership lease underflow");
        if previous <= 1 {
            self.state.notify.notify_waiters();
        }
    }
}

impl Drop for ExecutionOwnershipLease {
    fn drop(&mut self) {
        self.release_inner();
    }
}

#[derive(Clone)]
pub struct ToolContext {
    pub actor: PrincipalId,
    pub turn_id: u64,
    /// Exact host/tool-call identity. Agent dispatch uses ToolCall.id; direct
    /// host invocations must supply an explicit synthetic identity.
    pub call_id: String,
    /// Exact admitted origin/effect for this invocation. These are factual
    /// context, not additional authority.
    pub source: ToolSource,
    pub effect: ToolEffect,
    pub execution_site: String,
    pub cancellation: CancellationToken,
    /// Maximum bytes this invocation may return without host-side truncation.
    /// This is a presentation/resource ceiling, never tool authority. Composite
    /// runtimes may replace it with a separately captured semantic node budget.
    pub result_budget_bytes: usize,
    /// Exact provider/account/model identity selected and revalidated for this
    /// model round. This is evidence for a tool; it does not grant a tool the
    /// ability to select a different account/model.
    pub identity: InvocationIdentity,
    /// Exact durable Session/branch scope when the parent invocation is
    /// journaled. Memory-only callers remain unscoped.
    pub scope: Option<InvocationScope>,
    /// Stable Session-derived Agent owner. Factual routing evidence only; this
    /// does not grant access to the referenced Agent or any Job below it.
    pub execution_owner: Option<crate::AgentExecutionBinding>,
    /// Optional live causal owner (for example one active Graph node). Unlike
    /// the stable Agent execution owner, this is temporary work ownership and
    /// must not survive into a later independently admitted Turn.
    pub execution_scope: Option<ExecutionOwnershipScope>,
}
impl ToolContext {
    pub fn result_budget(&self, ceiling: usize) -> Result<usize> {
        let budget = self
            .result_budget_bytes
            .min(ceiling)
            .min(MAX_TOOL_RESULT_BYTES);
        if budget < MIN_TOOL_RESULT_BYTES {
            Err(Error::Config(
                "tool result budget must reserve at least 512 bytes".into(),
            ))
        } else {
            Ok(budget)
        }
    }
}

pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<ToolOutput>> + Send + 'a>>;
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    /// Current source liveness only, never an authority grant. Once a remote
    /// source fails closed its tool can disappear from advertised specs and
    /// fails the same authorization recheck just before effect execution.
    /// Host tools default to available; ToolPolicy remains authoritative.
    fn is_available(&self) -> bool {
        true
    }
    /// Must check the complete schema and semantic constraints. Advertising a
    /// JSON schema does not validate data; untrusted tool arguments still need this.
    fn validate(&self, arguments: &Value) -> Result<()>;
    /// Conservative scheduling hint for two calls to this same trusted
    /// tool instance. Never a grant, sandbox or mutation lock.
    fn can_run_parallel(&self, _first: &Value, _second: &Value) -> bool {
        false
    }
    fn execute<'a>(&'a self, ctx: ToolContext, arguments: &'a Value) -> ToolFuture<'a>;
}
/// A trusted tool's scheduling hint is still untrusted computation over
/// model arguments. Panics conservatively become a serialization barrier.
pub(crate) fn guarded_parallel_hint(tool: &dyn Tool, first: &Value, second: &Value) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tool.can_run_parallel(first, second)
    }))
    .unwrap_or(false)
}
/// Host-controlled tool origin. An external source revision must be minted by
/// the trusted registry owner; a server-reported version string is not authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolBindingSnapshot {
    pub name: String,
    pub registry_revision: u64,
    /// Host-minted generation of the effective registry+policy publication.
    /// Revoke/regrant always advances this so stale prepared work cannot regain
    /// authority merely because the same source/effect is visible again.
    pub authority_revision: u64,
    pub source: ToolSource,
    pub effect: ToolEffect,
    pub execution_site: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ToolSource {
    pub kind: SourceKind,
    pub id: String,
    pub epoch: u64,
}
impl ToolSource {
    pub fn new(kind: SourceKind, id: impl Into<String>, epoch: u64) -> Result<Self> {
        let value = Self {
            kind,
            id: id.into(),
            epoch,
        };
        value.as_kernel()?;
        Ok(value)
    }
    pub fn trusted_host() -> Self {
        Self {
            kind: SourceKind::Host,
            id: "trusted-host".into(),
            epoch: 1,
        }
    }
    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            SourceKind::Host => "host",
            SourceKind::Mcp => "mcp",
            SourceKind::Extension => "extension",
        }
    }
    fn as_kernel(&self) -> Result<Source<'_>> {
        Source::new(self.kind, &self.id, self.epoch)
            .map_err(|_| Error::Config("invalid tool source identity or epoch".into()))
    }
}
fn kernel_effect(effect: &ToolEffect) -> Effect {
    match effect {
        ToolEffect::Read => Effect::Read,
        ToolEffect::Write => Effect::Write,
        ToolEffect::Process => Effect::Process,
        ToolEffect::Control => Effect::Control,
        ToolEffect::Unknown => Effect::Unknown,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExactToolGrant {
    name: String,
    source: ToolSource,
    effect: Effect,
    site: String,
}
impl ExactToolGrant {
    fn permits(&self, actor: &str, site: &str, cap: &Capability<'_>) -> bool {
        let descriptor = Capability::new(
            &self.name,
            match self.source.as_kernel() {
                Ok(source) => source,
                Err(_) => return false,
            },
            self.effect,
        );
        descriptor
            .ok()
            .and_then(|capability| Grant::new(actor, &self.site, capability).ok())
            .is_some_and(|grant| grant.permits(actor, site, cap))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolParallelism {
    pub max_parallel_reads: usize,
    pub allow_parallel_shell: bool,
    pub max_parallel_shell: usize,
}
impl Default for ToolParallelism {
    fn default() -> Self {
        Self {
            max_parallel_reads: aw_agent_kernel::turn::DEFAULT_PARALLEL_READS,
            allow_parallel_shell: true,
            max_parallel_shell: aw_agent_kernel::turn::DEFAULT_PARALLEL_BASH,
        }
    }
}
impl ToolParallelism {
    pub fn validate(self) -> Result<Self> {
        if !(1..=aw_agent_kernel::turn::MAX_READ_WAVE).contains(&self.max_parallel_reads)
            || !(1..=aw_agent_kernel::turn::MAX_READ_WAVE).contains(&self.max_parallel_shell)
        {
            return Err(Error::Config(
                "tool parallel ceilings must be within 1..=128".into(),
            ));
        }
        Ok(self)
    }
}

/// Host-runtime listener for bounded tool-policy resource changes. This is
/// coordination only: it cannot mint grants, change tool sources, or create
/// invocation scope. Validation is two-phase so a rejected update leaves every
/// runtime and the Agent policy unchanged.
pub trait ToolPolicyLifecycle: Send + Sync {
    fn validate_parallelism(&self, _next: ToolParallelism) -> Result<()> {
        Ok(())
    }

    /// Called only after every listener accepted the snapshot. Implementations
    /// must make this a non-failing local publication.
    fn apply_parallelism(&self, _next: ToolParallelism) {}
}

#[derive(Clone, Debug)]
pub struct ToolPolicy {
    pub execution_site: String,
    /// Exact host-issued grants for every origin including built-in Host tools.
    pub source_grants: BTreeMap<PrincipalId, BTreeSet<ExactToolGrant>>,
    pub max_model_rounds: u32,
    pub max_tool_calls: u32,
    /// Read wave size is host-selectable, default 16, hard resource bound 128.
    /// Advisory remote readOnlyHint never changes the actual effect type.
    pub max_parallel_reads: usize,
    /// The model may select concurrent already-granted trusted Bash via
    /// parallel=true; the host may enforce serial execution at any time.
    /// Granting Bash execution itself remains a distinct ToolPolicy decision.
    pub allow_parallel_bash: bool,
    pub max_parallel_bash: usize,
    pub timeout: Duration,
}
impl Default for ToolPolicy {
    fn default() -> Self {
        Self {
            execution_site: "local".into(),
            source_grants: BTreeMap::new(),
            max_model_rounds: 8,
            max_tool_calls: aw_agent_kernel::turn::MAX_CALLS,
            max_parallel_reads: ToolParallelism::default().max_parallel_reads,
            allow_parallel_bash: ToolParallelism::default().allow_parallel_shell,
            max_parallel_bash: ToolParallelism::default().max_parallel_shell,
            timeout: Duration::from_secs(30),
        }
    }
}
impl ToolPolicy {
    pub fn has_grants(&self) -> bool {
        self.source_grants.values().any(|grants| !grants.is_empty())
    }

    pub fn parallelism(&self) -> ToolParallelism {
        ToolParallelism {
            max_parallel_reads: self.max_parallel_reads,
            allow_parallel_shell: self.allow_parallel_bash,
            max_parallel_shell: self.max_parallel_bash,
        }
    }

    pub(crate) fn set_parallelism(&mut self, value: ToolParallelism) {
        self.max_parallel_reads = value.max_parallel_reads;
        self.allow_parallel_bash = value.allow_parallel_shell;
        self.max_parallel_bash = value.max_parallel_shell;
    }

    /// Trusted host only: a built-in tool's cached name and effect are
    /// granted through the *same* source-bound predicate as MCP/extensions.
    /// A name by itself cannot authorize anything.
    pub fn permit_host(
        &mut self,
        actor: PrincipalId,
        name: &str,
        effect: &ToolEffect,
    ) -> Result<()> {
        self.permit_source(actor, name, &ToolSource::trusted_host(), effect)
    }
    /// Called only by a trusted host after intentional actor authorization.
    /// No registration, MCP hint or model proposal can issue this grant.
    pub fn permit_source(
        &mut self,
        actor: PrincipalId,
        name: &str,
        source: &ToolSource,
        effect: &ToolEffect,
    ) -> Result<()> {
        let descriptor = Capability::new(name, source.as_kernel()?, kernel_effect(effect))
            .map_err(|_| Error::Config("invalid scoped tool capability".into()))?;
        Grant::new(actor.as_str(), &self.execution_site, descriptor)
            .map_err(|_| Error::Config("invalid scoped tool grant identity/site".into()))?;
        self.source_grants
            .entry(actor)
            .or_default()
            .insert(ExactToolGrant {
                name: name.into(),
                source: source.clone(),
                effect: descriptor.effect,
                site: self.execution_site.clone(),
            });
        Ok(())
    }
    pub fn revoke_source(&mut self, actor: &PrincipalId, name: &str, source: &ToolSource) {
        if let Some(grants) = self.source_grants.get_mut(actor) {
            grants.retain(|grant| grant.name != name || grant.source != *source);
        }
    }
    fn allows_registration(&self, actor: &PrincipalId, registered: &RegisteredTool) -> bool {
        let cap = match registered.capability() {
            Ok(cap) => cap,
            Err(_) => return false,
        };
        self.source_grants.get(actor).is_some_and(|grants| {
            grants
                .iter()
                .any(|grant| grant.permits(actor.as_str(), &self.execution_site, &cap))
        })
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if self.max_model_rounds == 0
            || self.max_model_rounds > 32
            || self.max_tool_calls > 128
            || !(1..=aw_agent_kernel::turn::MAX_READ_WAVE).contains(&self.max_parallel_reads)
            || !(1..=aw_agent_kernel::turn::MAX_READ_WAVE).contains(&self.max_parallel_bash)
            || self.timeout.is_zero()
            || self.timeout > Duration::from_secs(600)
        {
            return Err(Error::Config("invalid bounded tool policy".into()));
        }
        if self.execution_site.is_empty()
            || self.execution_site.len() > 128
            || self.execution_site.chars().any(char::is_control)
        {
            return Err(Error::Config("invalid tool executor label".into()));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct RegisteredTool {
    tool: Arc<dyn Tool>,
    spec: ToolSpec,
    source: ToolSource,
    registered_revision: u64,
}
impl RegisteredTool {
    fn capability(&self) -> Result<Capability<'_>> {
        Capability::new(
            &self.spec.name,
            self.source.as_kernel()?,
            kernel_effect(&self.spec.effect),
        )
        .map_err(|_| Error::Config("invalid registered tool capability".into()))
    }
}
#[derive(Clone, Default)]
pub struct ToolRegistry {
    pub(crate) tools: BTreeMap<String, RegisteredTool>,
    revision: u64,
}
impl ToolRegistry {
    pub fn register(&mut self, tool: Arc<dyn Tool>) -> Result<()> {
        self.register_with_name(tool, ToolSource::trusted_host())
            .map(|_| ())
    }
    /// Trusted host registration with explicit origin. No grant is created.
    pub fn register_from(&mut self, tool: Arc<dyn Tool>, source: ToolSource) -> Result<()> {
        self.register_with_name(tool, source).map(|_| ())
    }
    pub(crate) fn register_with_name(
        &mut self,
        tool: Arc<dyn Tool>,
        source: ToolSource,
    ) -> Result<String> {
        let spec = tool.spec();
        let candidate =
            Capability::new(&spec.name, source.as_kernel()?, kernel_effect(&spec.effect))
                .map_err(|_| Error::Config("invalid tool definition or source".into()))?;
        if spec.description.len() > 8192 || !spec.parameters.is_object() {
            return Err(Error::Config("invalid tool definition".into()));
        }
        if serde_json::to_vec(&spec.parameters)
            .map_err(|_| Error::Config("invalid tool schema".into()))?
            .len()
            > 65536
        {
            return Err(Error::Limit("tool schema too large"));
        }
        if self.tools.len() >= 32 {
            return Err(Error::Config("at most 32 registered tools".into()));
        }
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(Error::Limit("tool registry revisions exhausted"))?;
        let existing = self
            .tools
            .values()
            .map(RegisteredTool::capability)
            .collect::<Result<Vec<_>>>()?;
        let mut descriptors = existing;
        descriptors.push(candidate);
        Catalog::new(next_revision, &descriptors).map_err(|e| match e {
            aw_agent_kernel::capability::Error::DuplicateCapability => {
                Error::Config("tool names must be unique across all origins".into())
            }
            _ => Error::Config("invalid tool registry snapshot".into()),
        })?;
        let name = spec.name.clone();
        self.tools.insert(
            name.clone(),
            RegisteredTool {
                tool,
                spec,
                source,
                registered_revision: next_revision,
            },
        );
        self.revision = next_revision;
        Ok(name)
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    fn binding_revision(&self, name: &str) -> Option<u64> {
        self.tools.get(name).map(|item| item.registered_revision)
    }
    pub fn visible(&self, actor: &PrincipalId, policy: &ToolPolicy) -> Vec<ToolSpec> {
        self.tools
            .values()
            .filter(|item| item.tool.is_available() && policy.allows_registration(actor, item))
            .map(|item| item.spec.clone())
            .collect()
    }
    pub fn allows(&self, actor: &PrincipalId, policy: &ToolPolicy, name: &str) -> bool {
        self.tools
            .get(name)
            .is_some_and(|item| item.tool.is_available() && policy.allows_registration(actor, item))
    }
    /// Every actual tool admission, dispatch and post-effect disclosure must
    /// recheck BOTH the trusted current ToolPolicy and the Actor Envelope.
    /// A previously unavailable MCP/extension tool becoming live after Turn
    /// admission must NOT inherit a different Actor's Envelope or grant.
    pub(crate) fn allows_bound(
        &self,
        actor: &PrincipalId,
        policy: &ToolPolicy,
        name: &str,
        gate: Option<&dyn crate::control::AgentStepGate>,
    ) -> bool {
        self.tools.get(name).is_some_and(|item| {
            item.tool.is_available()
                && policy.allows_registration(actor, item)
                && gate.is_none_or(|g| g.allow_tool(name, &item.source, &item.spec.effect).is_ok())
        })
    }
    pub fn source_of(&self, name: &str) -> Option<&ToolSource> {
        self.tools.get(name).map(|item| &item.source)
    }
    pub fn effect_of(&self, name: &str) -> Option<&ToolEffect> {
        self.tools.get(name).map(|item| &item.spec.effect)
    }
    /// Always use the exact spec captured when the tool was admitted. A tool
    /// implementation cannot change its effect/schema by later mutating spec().
    pub(crate) fn get(&self, name: &str) -> Option<(Arc<dyn Tool>, ToolSpec)> {
        self.tools
            .get(name)
            .map(|item| (item.tool.clone(), item.spec.clone()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct BindingKey {
    actor: String,
    name: String,
    source_kind: u8,
    source_id: String,
    source_epoch: u64,
    effect: u8,
    site: String,
}
fn binding_effect_key(effect: &ToolEffect) -> u8 {
    match effect {
        ToolEffect::Read => 0,
        ToolEffect::Write => 1,
        ToolEffect::Process => 2,
        ToolEffect::Control => 3,
        ToolEffect::Unknown => 4,
    }
}
fn binding_source_key(kind: SourceKind) -> u8 {
    match kind {
        SourceKind::Host => 0,
        SourceKind::Mcp => 1,
        SourceKind::Extension => 2,
    }
}
fn binding_key(
    actor: &PrincipalId,
    name: &str,
    source: &ToolSource,
    effect: &ToolEffect,
    site: &str,
) -> BindingKey {
    BindingKey {
        actor: actor.as_str().to_owned(),
        name: name.to_owned(),
        source_kind: binding_source_key(source.kind),
        source_id: source.id.clone(),
        source_epoch: source.epoch,
        effect: binding_effect_key(effect),
        site: site.to_owned(),
    }
}

fn revalidate_binding_view(
    view: &BindingView,
    actor: &PrincipalId,
    binding: &ToolBindingSnapshot,
) -> Result<()> {
    let key = binding_key(
        actor,
        &binding.name,
        &binding.source,
        &binding.effect,
        &binding.execution_site,
    );
    if view.registry.binding_revision(&binding.name) != Some(binding.registry_revision)
        || view.generations.get(&key).copied() != Some(binding.authority_revision)
        || binding.execution_site != view.policy.execution_site
        || view.registry.source_of(&binding.name) != Some(&binding.source)
        || view.registry.effect_of(&binding.name) != Some(&binding.effect)
        || !view.registry.allows(actor, &view.policy, &binding.name)
    {
        return Err(Error::Denied("captured tool binding is no longer live"));
    }
    Ok(())
}

#[derive(Clone, Default)]
struct BindingView {
    registry: ToolRegistry,
    policy: ToolPolicy,
    generations: BTreeMap<BindingKey, u64>,
    next_generation: u64,
    publication_revision: u64,
}

/// Read-only live view of the current tool registry + grants for long-lived
/// host services such as Job execution. It can never register or grant tools.
struct ToolBindingAuthorityInner {
    view: RwLock<BindingView>,
    changes: tokio::sync::watch::Sender<u64>,
}
#[derive(Clone)]
pub struct ToolBindingAuthority {
    inner: Arc<ToolBindingAuthorityInner>,
}
impl Default for ToolBindingAuthority {
    fn default() -> Self {
        let (changes, _) = tokio::sync::watch::channel(0u64);
        Self {
            inner: Arc::new(ToolBindingAuthorityInner {
                view: RwLock::new(BindingView::default()),
                changes,
            }),
        }
    }
}
impl ToolBindingAuthority {
    pub(crate) fn publish(&self, registry: &ToolRegistry, policy: &ToolPolicy) -> Result<()> {
        let mut view = self
            .inner
            .view
            .write()
            .map_err(|_| Error::Denied("tool binding authority poisoned"))?;
        let current = policy
            .source_grants
            .iter()
            .flat_map(|(actor, grants)| {
                grants.iter().map(move |grant| {
                    binding_key(
                        actor,
                        &grant.name,
                        &grant.source,
                        match grant.effect {
                            Effect::Read => &ToolEffect::Read,
                            Effect::Write => &ToolEffect::Write,
                            Effect::Process => &ToolEffect::Process,
                            Effect::Control => &ToolEffect::Control,
                            Effect::Unknown => &ToolEffect::Unknown,
                        },
                        &grant.site,
                    )
                })
            })
            .collect::<BTreeSet<_>>();
        let previous = std::mem::take(&mut view.generations);
        let mut generations = BTreeMap::new();
        let mut next_generation = view.next_generation;
        for key in current {
            let generation = if let Some(existing) = previous.get(&key) {
                *existing
            } else {
                next_generation = next_generation
                    .checked_add(1)
                    .ok_or(Error::Limit("tool authority generations exhausted"))?;
                next_generation
            };
            generations.insert(key, generation);
        }
        view.registry = registry.clone();
        view.policy = policy.clone();
        view.generations = generations;
        view.next_generation = next_generation;
        view.publication_revision = view
            .publication_revision
            .checked_add(1)
            .ok_or(Error::Limit("tool authority publications exhausted"))?;
        let revision = view.publication_revision;
        drop(view);
        self.inner.changes.send_replace(revision);
        Ok(())
    }

    pub fn permitted_bindings(&self, actor: &PrincipalId) -> Result<Vec<ToolBindingSnapshot>> {
        let view = self
            .inner
            .view
            .read()
            .map_err(|_| Error::Denied("tool binding authority poisoned"))?;
        let mut out = Vec::new();
        for name in view.registry.tools.keys() {
            if !view.registry.allows(actor, &view.policy, name) {
                continue;
            }
            let source = view
                .registry
                .source_of(name)
                .ok_or(Error::Denied("tool source unavailable"))?
                .clone();
            let effect = view
                .registry
                .effect_of(name)
                .ok_or(Error::Denied("tool effect unavailable"))?
                .clone();
            let key = binding_key(actor, name, &source, &effect, &view.policy.execution_site);
            let authority_revision = *view
                .generations
                .get(&key)
                .ok_or(Error::Denied("tool grant generation unavailable"))?;
            out.push(ToolBindingSnapshot {
                name: name.clone(),
                registry_revision: view
                    .registry
                    .binding_revision(name)
                    .ok_or(Error::Denied("tool registry binding unavailable"))?,
                authority_revision,
                source,
                effect,
                execution_site: view.policy.execution_site.clone(),
            });
        }
        Ok(out)
    }

    pub fn permitted_binding(
        &self,
        actor: &PrincipalId,
        name: &str,
    ) -> Result<ToolBindingSnapshot> {
        let view = self
            .inner
            .view
            .read()
            .map_err(|_| Error::Denied("tool binding authority poisoned"))?;
        if !view.registry.allows(actor, &view.policy, name) {
            return Err(Error::Denied("tool is not currently permitted"));
        }
        let source = view
            .registry
            .source_of(name)
            .ok_or(Error::Denied("tool source unavailable"))?
            .clone();
        let effect = view
            .registry
            .effect_of(name)
            .ok_or(Error::Denied("tool effect unavailable"))?
            .clone();
        let key = binding_key(actor, name, &source, &effect, &view.policy.execution_site);
        let authority_revision = *view
            .generations
            .get(&key)
            .ok_or(Error::Denied("tool grant generation unavailable"))?;
        Ok(ToolBindingSnapshot {
            name: name.to_owned(),
            registry_revision: view
                .registry
                .binding_revision(name)
                .ok_or(Error::Denied("tool registry binding unavailable"))?,
            authority_revision,
            source,
            effect,
            execution_site: view.policy.execution_site.clone(),
        })
    }

    /// Check one exact current capability that is intentionally not a
    /// registered model tool (for example live Process stdin authority).
    /// This never advertises or registers a tool and never infers authority
    /// from the capability name.
    pub fn permits_capability(
        &self,
        actor: &PrincipalId,
        name: &str,
        source: &ToolSource,
        effect: &ToolEffect,
    ) -> Result<()> {
        let view = self
            .inner
            .view
            .read()
            .map_err(|_| Error::Denied("tool binding authority poisoned"))?;
        let descriptor = Capability::new(name, source.as_kernel()?, kernel_effect(effect))
            .map_err(|_| Error::Denied("invalid exact capability"))?;
        let permitted = view.policy.source_grants.get(actor).is_some_and(|grants| {
            grants.iter().any(|grant| {
                grant.permits(actor.as_str(), &view.policy.execution_site, &descriptor)
            })
        });
        permitted
            .then_some(())
            .ok_or(Error::Denied("exact capability is not currently granted"))
    }

    /// Wait until this exact captured binding is no longer live. Unrelated
    /// publications merely wake and revalidate; they never revoke this binding.
    /// A poisoned/unavailable authority fails closed as not-live.
    pub async fn wait_until_not_live(&self, actor: &PrincipalId, binding: &ToolBindingSnapshot) {
        let mut changes = self.inner.changes.subscribe();
        loop {
            if self.revalidate(actor, binding).is_err() {
                return;
            }
            if changes.changed().await.is_err() {
                return;
            }
        }
    }

    pub fn revalidate(&self, actor: &PrincipalId, binding: &ToolBindingSnapshot) -> Result<()> {
        let view = self
            .inner
            .view
            .read()
            .map_err(|_| Error::Denied("tool binding authority poisoned"))?;
        revalidate_binding_view(&view, actor, binding)
    }

    /// Serialize one durable authorization commit against ToolBinding
    /// publication/revocation. Revocation needs the write guard: if it commits
    /// first this revalidation denies, while a caller holding this read guard
    /// may durably commit exactly one already-authorized attempt before the
    /// revocation publication can proceed.
    pub fn with_revalidated_binding<T>(
        &self,
        actor: &PrincipalId,
        binding: &ToolBindingSnapshot,
        commit: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let view = self
            .inner
            .view
            .read()
            .map_err(|_| Error::Denied("tool binding authority poisoned"))?;
        revalidate_binding_view(&view, actor, binding)?;
        commit()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Running,
    Succeeded,
    Failed,
    Denied,
    Interrupted,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolRecord {
    pub call: ToolCall,
    pub effect: ToolEffect,
    pub execution_site: String,
    pub status: ToolStatus,
    pub output: Option<ToolOutput>,
}

#[cfg(test)]
mod binding_authority_tests {
    use super::*;

    struct ReadTool(&'static str);
    impl Tool for ReadTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: self.0.into(),
                description: "fixture".into(),
                parameters: serde_json::json!({"type":"object"}),
                effect: ToolEffect::Read,
            }
        }
        fn validate(&self, _: &Value) -> Result<()> {
            Ok(())
        }
        fn execute<'a>(&'a self, _: ToolContext, _: &'a Value) -> ToolFuture<'a> {
            Box::pin(async { Ok(ToolOutput::text("ok")) })
        }
    }

    #[test]
    fn unrelated_registration_and_grant_preserve_exact_binding_generation() {
        let actor = PrincipalId::new("a").unwrap();
        let mut registry = ToolRegistry::default();
        registry.register(Arc::new(ReadTool("one"))).unwrap();
        let mut policy = ToolPolicy::default();
        policy
            .permit_host(actor.clone(), "one", &ToolEffect::Read)
            .unwrap();
        let authority = ToolBindingAuthority::default();
        authority.publish(&registry, &policy).unwrap();
        let original = authority.permitted_binding(&actor, "one").unwrap();

        registry.register(Arc::new(ReadTool("two"))).unwrap();
        policy
            .permit_host(actor.clone(), "two", &ToolEffect::Read)
            .unwrap();
        authority.publish(&registry, &policy).unwrap();
        let current = authority.permitted_binding(&actor, "one").unwrap();
        assert_eq!(original, current);
        authority.revalidate(&actor, &original).unwrap();
    }

    #[tokio::test]
    async fn binding_change_wait_ignores_unrelated_publish_and_finishes_on_exact_revoke() {
        let actor = PrincipalId::new("a").unwrap();
        let mut registry = ToolRegistry::default();
        registry.register(Arc::new(ReadTool("one"))).unwrap();
        registry.register(Arc::new(ReadTool("two"))).unwrap();
        let mut policy = ToolPolicy::default();
        policy
            .permit_host(actor.clone(), "one", &ToolEffect::Read)
            .unwrap();
        let authority = ToolBindingAuthority::default();
        authority.publish(&registry, &policy).unwrap();
        let binding = authority.permitted_binding(&actor, "one").unwrap();
        let waiter = {
            let authority = authority.clone();
            let actor = actor.clone();
            let binding = binding.clone();
            tokio::spawn(async move {
                authority.wait_until_not_live(&actor, &binding).await;
            })
        };
        policy
            .permit_host(actor.clone(), "two", &ToolEffect::Read)
            .unwrap();
        authority.publish(&registry, &policy).unwrap();
        tokio::task::yield_now().await;
        assert!(
            !waiter.is_finished(),
            "unrelated grant must not revoke binding"
        );
        policy.revoke_source(&actor, "one", &ToolSource::trusted_host());
        authority.publish(&registry, &policy).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("exact revoke must wake binding waiter")
            .unwrap();
    }

    #[test]
    fn durable_commit_guard_serializes_exact_revocation_publication() {
        use std::{sync::mpsc, thread, time::Duration};

        let actor = PrincipalId::new("a").unwrap();
        let mut registry = ToolRegistry::default();
        registry.register(Arc::new(ReadTool("one"))).unwrap();
        let mut policy = ToolPolicy::default();
        policy
            .permit_host(actor.clone(), "one", &ToolEffect::Read)
            .unwrap();
        let authority = ToolBindingAuthority::default();
        authority.publish(&registry, &policy).unwrap();
        let binding = authority.permitted_binding(&actor, "one").unwrap();

        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let guarded = {
            let authority = authority.clone();
            let actor = actor.clone();
            let binding = binding.clone();
            thread::spawn(move || {
                authority
                    .with_revalidated_binding(&actor, &binding, || {
                        entered_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        Ok(())
                    })
                    .unwrap();
            })
        };
        entered_rx.recv().unwrap();

        policy.revoke_source(&actor, "one", &ToolSource::trusted_host());
        let (published_tx, published_rx) = mpsc::channel();
        let publisher = {
            let authority = authority.clone();
            thread::spawn(move || {
                authority.publish(&registry, &policy).unwrap();
                published_tx.send(()).unwrap();
            })
        };
        assert!(
            published_rx
                .recv_timeout(Duration::from_millis(50))
                .is_err(),
            "revocation publication crossed a live durable-commit read guard"
        );

        release_tx.send(()).unwrap();
        guarded.join().unwrap();
        published_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        publisher.join().unwrap();
        assert!(authority.revalidate(&actor, &binding).is_err());
    }

    #[test]
    fn revoke_then_regrant_mints_new_exact_grant_generation() {
        let actor = PrincipalId::new("a").unwrap();
        let mut registry = ToolRegistry::default();
        registry.register(Arc::new(ReadTool("one"))).unwrap();
        let mut policy = ToolPolicy::default();
        policy
            .permit_host(actor.clone(), "one", &ToolEffect::Read)
            .unwrap();
        let authority = ToolBindingAuthority::default();
        authority.publish(&registry, &policy).unwrap();
        let original = authority.permitted_binding(&actor, "one").unwrap();

        policy.revoke_source(&actor, "one", &ToolSource::trusted_host());
        authority.publish(&registry, &policy).unwrap();
        assert!(authority.revalidate(&actor, &original).is_err());
        policy
            .permit_host(actor.clone(), "one", &ToolEffect::Read)
            .unwrap();
        authority.publish(&registry, &policy).unwrap();
        let current = authority.permitted_binding(&actor, "one").unwrap();
        assert_eq!(current.registry_revision, original.registry_revision);
        assert!(current.authority_revision > original.authority_revision);
        assert!(authority.revalidate(&actor, &original).is_err());
        authority.revalidate(&actor, &current).unwrap();
    }

    #[test]
    fn hidden_process_input_capability_is_current_but_never_provider_visible() {
        let actor = PrincipalId::new("a").unwrap();
        let registry = ToolRegistry::default();
        let mut policy = ToolPolicy::default();
        policy
            .permit_host(actor.clone(), "process.input", &ToolEffect::Control)
            .unwrap();
        let authority = ToolBindingAuthority::default();
        authority.publish(&registry, &policy).unwrap();

        assert!(registry.visible(&actor, &policy).is_empty());
        authority
            .permits_capability(
                &actor,
                "process.input",
                &ToolSource::trusted_host(),
                &ToolEffect::Control,
            )
            .unwrap();

        policy.revoke_source(&actor, "process.input", &ToolSource::trusted_host());
        authority.publish(&registry, &policy).unwrap();
        assert!(authority
            .permits_capability(
                &actor,
                "process.input",
                &ToolSource::trusted_host(),
                &ToolEffect::Control,
            )
            .is_err());
    }
}

#[cfg(test)]
mod execution_ownership_tests {
    use super::*;

    #[tokio::test]
    async fn ownership_scope_waits_for_every_lease_and_shares_cancellation() {
        let cancellation = CancellationToken::new();
        let scope = ExecutionOwnershipScope::from_trusted_host(
            "/d:0/i:0/p:0/a:0/t:1/j:g/n:1",
            cancellation.clone(),
        )
        .unwrap();
        let first = scope.acquire().unwrap();
        let second = scope.acquire().unwrap();
        assert_eq!(scope.active(), 2);

        let waiter = {
            let scope = scope.clone();
            tokio::spawn(async move {
                scope.wait_quiescent().await;
            })
        };
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());

        drop(first);
        tokio::task::yield_now().await;
        assert_eq!(scope.active(), 1);
        assert!(!waiter.is_finished());

        scope.cancellation().cancel();
        cancellation.cancelled().await;
        assert!(cancellation.is_cancelled());

        drop(second);
        waiter.await.unwrap();
        assert_eq!(scope.active(), 0);
        scope.mark_unresolved();
        assert!(scope.is_unresolved());
    }

    #[test]
    fn ownership_scope_identity_is_instance_exact() {
        let one = ExecutionOwnershipScope::from_trusted_host(
            "/d:0/i:0/p:0/a:0/t:1/j:g/n:1",
            CancellationToken::new(),
        )
        .unwrap();
        let clone = one.clone();
        let other = ExecutionOwnershipScope::from_trusted_host(
            "/d:0/i:0/p:0/a:0/t:1/j:g/n:1",
            CancellationToken::new(),
        )
        .unwrap();
        assert!(one.same_instance(&clone));
        assert!(!one.same_instance(&other));
    }
}

#[cfg(test)]
mod parallel_hint_tests {
    use super::*;
    struct PanickingHostTool;
    impl Tool for PanickingHostTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "panic_sched".into(),
                description: "host fixture".into(),
                parameters: serde_json::json!({"type":"object"}),
                effect: ToolEffect::Process,
            }
        }
        fn validate(&self, _: &Value) -> Result<()> {
            Ok(())
        }
        fn can_run_parallel(&self, _: &Value, _: &Value) -> bool {
            panic!("untrusted scheduling hook panic");
        }
        fn execute<'a>(&'a self, _: ToolContext, _: &'a Value) -> ToolFuture<'a> {
            Box::pin(async { Ok(ToolOutput::text("unused")) })
        }
    }
    #[test]
    fn panicking_scheduling_hint_is_serial_barrier() {
        let value = serde_json::json!({"parallel":true,"writes":[]});
        assert!(!guarded_parallel_hint(&PanickingHostTool, &value, &value));
    }
}

#[cfg(test)]
mod control_effect_tests {
    use super::*;
    use aw_agent_kernel::{
        capability::{Effect, SourceKind},
        envelope::{Access, Budget, Envelope, EnvelopeSpec, ExactGrant, Network},
    };

    #[test]
    fn filesystem_readonly_is_orthogonal_to_exact_control_grant() {
        let source = ToolSource::trusted_host();
        let envelope = Envelope::new(EnvelopeSpec {
            actor: "actor".into(),
            site: "local".into(),
            revision: 1,
            access: Access::ReadOnly,
            network: Network::Deny,
            process: false,
            tty: false,
            microphone: false,
            speaker: false,
            budget: Budget {
                max_model_rounds: 1,
                max_tool_calls: 1,
                max_children: 0,
                max_depth: 0,
                max_output_bytes: 1024,
            },
            grants: vec![ExactGrant {
                name: "jobs_cancel".into(),
                kind: SourceKind::Host,
                source: source.id.clone(),
                source_epoch: source.epoch,
                effect: Effect::Control,
            }],
        })
        .unwrap();
        assert!(envelope_permits_tool(
            &envelope,
            "jobs_cancel",
            &source,
            &ToolEffect::Control
        ));
        assert!(!envelope_permits_tool(
            &envelope,
            "jobs_cancel",
            &source,
            &ToolEffect::Write
        ));
        assert!(!envelope_permits_tool(
            &envelope,
            "other",
            &source,
            &ToolEffect::Control
        ));
    }
}
