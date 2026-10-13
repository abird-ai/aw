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
    sync::Arc,
    time::Duration,
};
pub const MAX_TOOL_CALLS_PER_ROUND: usize = aw_agent_kernel::turn::MAX_BATCH;
pub const MAX_TOOL_ARGUMENT_BYTES: usize = 64 * 1024;
pub const MAX_TOOL_RESULT_BYTES: usize = 128 * 1024;
pub const MAX_TRACE_BYTES: usize = 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolEffect {
    Read,
    Write,
    Process,
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
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    if (envelope.spec().access == Access::ReadOnly && effect != Effect::Read)
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
#[derive(Clone)]
pub struct ToolContext {
    pub actor: PrincipalId,
    pub turn_id: u64,
    pub execution_site: String,
    pub cancellation: CancellationToken,
    /// Exact provider/account/model identity selected and revalidated for this
    /// model round. This is evidence for a tool; it does not grant a tool the
    /// ability to select a different account/model.
    pub identity: InvocationIdentity,
    /// Exact durable Session/branch scope when the parent invocation is
    /// journaled. Memory-only callers remain unscoped.
    pub scope: Option<InvocationScope>,
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
    fn execute<'a>(&'a self, ctx: ToolContext, arguments: &'a Value) -> ToolFuture<'a>;
}
/// Host-controlled tool origin. An external source revision must be minted by
/// the trusted registry owner; a server-reported version string is not authority.
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
            max_parallel_reads: aw_agent_kernel::turn::DEFAULT_PARALLEL_READS,
            allow_parallel_bash: true,
            max_parallel_bash: aw_agent_kernel::turn::DEFAULT_PARALLEL_BASH,
            timeout: Duration::from_secs(30),
        }
    }
}
impl ToolPolicy {
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
        self.tools
            .insert(name.clone(), RegisteredTool { tool, spec, source });
        self.revision = next_revision;
        Ok(name)
    }
    pub fn revision(&self) -> u64 {
        self.revision
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
