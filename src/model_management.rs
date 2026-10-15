//! AW-P6-9D1 — model-visible *observation-only* managed `agent`, `queue`,
//! and `process` Tools on the SAME original ToolRegistry / Agent TurnMachine.
//!
//! This is a trusted owning-host SDK, NEVER registered by default. A caller
//! must present the actual durable `actor_owner::Service`, current journaled
//! Agent, and an explicit model-observer allowlist. Each exact Host source /
//! epoch / `Read` effect must also already exist in the bound Actor Envelope.
//! `register_host_tool` independently issues only the matching ToolPolicy
//! Read permission; no model/project/Plugin can mint either source grant.
//!
//! All actual calls recheck original current Actor generation, writer epoch,
//! immutable Dispatching Input, actual original Runtime active Turn, selected
//! account/payer and ToolContext Session/branch/site/turn. There is no new
//! Process owner, external network listener, Tool executor or replay engine.
//! Mutating queue/enqueue/cancel/approve, process stdin/signals, subagent
//! spawn/transfer and paid-provider dispatch are intentionally NOT exposed.
use crate::{
    actor_owner::{self, ActorOperation, Service},
    Agent, Error, Result,
};
use aw_agent_core::{
    InvocationIdentity, Tool, ToolContext, ToolEffect, ToolFuture, ToolOutput, ToolSource, ToolSpec,
};
use aw_agent_kernel::actor::ActorKey;
use aw_tools::process::PipeProcess;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};
use tokio::sync::Mutex as AsyncMutex;

pub const MAX_MODEL_QUEUE_ROWS: usize = 32;
pub const MAX_OBSERVED_PROCESSES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Observer {
    Agent,
    Queue,
    Process,
}
impl Observer {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Queue => "queue",
            Self::Process => "process",
        }
    }
}
fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}
#[derive(Clone)]
struct BoundProcess {
    /// Runtime and Actor generation from the ORIGINAL registering owner.
    /// Matching only Session/site/writer would let a new Actor generation
    /// inherit an old Process, potentially including destructive control.
    actor: ActorKey,
    session: String,
    branch: String,
    site: String,
    principal: aw_agent_core::PrincipalId,
    epoch: u64,
    process: Arc<AsyncMutex<PipeProcess>>,
    /// Only a separately host-approved Process may be eligible for model
    /// termination. Observation-only or close-stdin registration remains OFF.
    termination_binding: Option<String>,
}
impl BoundProcess {
    fn bound_to(&self, owner: &Service) -> bool {
        let (principal, session, branch, site, epoch) = owner.bound_peer_scope();
        self.actor == owner.bound_actor_key()
            && &self.principal == principal
            && self.session == session
            && self.branch == branch
            && self.site == site
            && self.epoch == epoch
    }
}
/// Registry holds only BORROWED/SHARED handles to original already started,
/// independently source-granted and journaled `PipeProcess` instances. No
/// OS PID lookup, process spawn, Kill or raw stdout/stderr is supported.
#[derive(Clone, Default)]
pub struct ProcessRegistry {
    inner: Arc<Mutex<BTreeMap<String, BoundProcess>>>,
}
impl ProcessRegistry {
    /// Trusted local host ONLY: call after the original Process effect has
    /// been permitted, spawned and recorded in the existing TurnJournal.
    /// The model can query `handle` but cannot register arbitrary PIDs/paths.
    pub fn register_owned(
        &self,
        owner: &Service,
        agent: &Agent,
        handle: &str,
        process: Arc<AsyncMutex<PipeProcess>>,
    ) -> Result<()> {
        self.register_owned_inner(owner, agent, handle, process, None)
    }
    /// TRUSTED HOST ONLY and separate from register_owned: affirmatively
    /// authorizes termination of ONE already approved original Process.
    /// Requires that the original host independently journaled its Process
    /// creation/grant and approved destructive termination. Neither a model
    /// handle nor ordinary Process Read/Close permissions provide this flag.
    pub fn register_terminable_owned(
        &self,
        owner: &Service,
        agent: &Agent,
        handle: &str,
        process: Arc<AsyncMutex<PipeProcess>>,
    ) -> Result<()> {
        owner.verify_rpc_state()?;
        owner.verify_rpc_agent(agent)?;
        let mut randomness = [0u8; 32];
        getrandom::fill(&mut randomness).map_err(|_| {
            Error::Storage("OS randomness unavailable for original Process binding")
        })?;
        let immutable_binding = randomness
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        self.register_owned_inner(owner, agent, handle, process, Some(immutable_binding))
    }
    fn register_owned_inner(
        &self,
        owner: &Service,
        agent: &Agent,
        handle: &str,
        process: Arc<AsyncMutex<PipeProcess>>,
        termination_binding: Option<String>,
    ) -> Result<()> {
        if !token(handle) || handle.len() > 64 {
            return Err(Error::Denied("invalid opaque Process observation handle"));
        }
        owner.verify_rpc_state()?;
        owner.verify_rpc_agent(agent)?;
        let (principal, session, branch, site, epoch) = owner.bound_peer_scope();
        let mut entries = self
            .inner
            .lock()
            .map_err(|_| Error::Storage("Process observation registry poisoned"))?;
        if entries.len() >= MAX_OBSERVED_PROCESSES || entries.contains_key(handle) {
            return Err(Error::Conflict(
                "Process observation handle collision or bounded capacity",
            ));
        }
        entries.insert(
            handle.into(),
            BoundProcess {
                actor: owner.bound_actor_key(),
                session: session.into(),
                branch: branch.into(),
                site: site.into(),
                principal: principal.clone(),
                epoch,
                process,
                termination_binding,
            },
        );
        Ok(())
    }
    pub fn unregister_owned(&self, owner: &Service, handle: &str) -> Result<()> {
        owner.verify_rpc_state()?;
        let mut entries = self
            .inner
            .lock()
            .map_err(|_| Error::Storage("Process observation registry poisoned"))?;
        let owned = entries
            .get(handle)
            .ok_or(Error::Denied("Process observation is not registered"))?;
        if !owned.bound_to(owner) {
            return Err(Error::Denied(
                "Process observation owned by another Actor/Session",
            ));
        }
        entries.remove(handle);
        Ok(())
    }
    /// A model termination tool receives ONLY the specifically host-approved
    /// original Process handle and its random incarnation binding. No PID,
    /// OS executable or Process list is ever exposed to the model.
    pub(crate) fn resolve_terminable(
        &self,
        owner: &Service,
        handle: &str,
    ) -> Result<(Arc<AsyncMutex<PipeProcess>>, String)> {
        owner.verify_rpc_state()?;
        let entries = self
            .inner
            .lock()
            .map_err(|_| Error::Storage("original Process registry poisoned"))?;
        let item = entries.get(handle).ok_or(Error::Denied(
            "unregistered host Process cannot be terminated",
        ))?;
        if !item.bound_to(owner) {
            return Err(Error::Denied(
                "model termination Process belongs to another owner/site/epoch",
            ));
        }
        let binding = item.termination_binding.clone().ok_or(Error::Denied(
            "original Process was only registered for observation, not termination",
        ))?;
        Ok((item.process.clone(), binding))
    }
    pub(crate) fn resolve(
        &self,
        owner: &Service,
        handle: &str,
    ) -> Result<Arc<AsyncMutex<PipeProcess>>> {
        owner.verify_rpc_state()?;
        let entries = self
            .inner
            .lock()
            .map_err(|_| Error::Storage("Process observation registry poisoned"))?;
        let owned = entries
            .get(handle)
            .ok_or(Error::Denied("Process observation handle unknown"))?;
        if !owned.bound_to(owner) {
            return Err(Error::Denied("Process observation origin changed"));
        }
        Ok(owned.process.clone())
    }
}

#[derive(Clone)]
struct ModelObserverTool {
    kind: Observer,
    owner: Service,
    original_identity: InvocationIdentity,
    processes: Option<ProcessRegistry>,
}
/// Exact semantic validation after schema validation; raw model JSON may
/// never supply ActorKey, Envelope, writer epoch, source grant, PID or an
/// unbounded private payload, even if the provider emits a malformed call.
fn validate_operation(kind: Observer, args: &Value) -> Result<()> {
    let args = args
        .as_object()
        .ok_or(Error::Denied("model observer arguments must be object"))?;
    match kind {
        Observer::Agent => {
            if args.len() != 1 || args.get("op").and_then(Value::as_str) != Some("status") {
                return Err(Error::Denied("agent model observer permits only status"));
            }
        }
        Observer::Queue => match args.get("op").and_then(Value::as_str) {
            Some("list") if args.len() == 1 => {}
            Some("query")
                if args.len() == 2
                    && args
                        .get("request_id")
                        .and_then(Value::as_str)
                        .is_some_and(token) => {}
            _ => {
                return Err(Error::Denied(
                    "queue model observer permits only list or exact query",
                ))
            }
        },
        Observer::Process => {
            if args.len() != 2
                || args.get("op").and_then(Value::as_str) != Some("status")
                || !args
                    .get("handle")
                    .and_then(Value::as_str)
                    .is_some_and(|value| token(value) && value.len() <= 64)
            {
                return Err(Error::Denied(
                    "process model observer permits only opaque registered status",
                ));
            }
        }
    }
    Ok(())
}

impl ModelObserverTool {
    fn check(&self, ctx: &ToolContext) -> Result<()> {
        self.owner.verify_model_observation(ctx)?;
        if ctx.identity != self.original_identity {
            return Err(Error::Denied(
                "model observer provider/account/payer grant changed",
            ));
        }
        self.owner.require_model_observer_grant(self.kind.name())
    }
    fn validate_args(&self, args: &Value) -> Result<()> {
        validate_operation(self.kind, args)
    }
    async fn observation(&self, ctx: &ToolContext, args: &Value) -> Result<Value> {
        match self.kind {
            Observer::Agent => Ok(json!({
                "kind":"agent","state":"active_turn","turn_id":ctx.turn_id,
                "tool_authority":"read_only", "additional_provider_dispatch":false,
            })),
            Observer::Queue => {
                let op = match args.get("op").and_then(Value::as_str) {
                    Some("list") => ActorOperation::ListQueue,
                    Some("query") => ActorOperation::QueryQueue {
                        request_id: args["request_id"]
                            .as_str()
                            .ok_or(Error::Denied("queue request ID missing"))?
                            .into(),
                    },
                    _ => return Err(Error::Denied("invalid model queue observation")),
                };
                let result = self.owner.operate_bound(op)?;
                if let actor_owner::ActorOperationResult::Queued { facts } = &result {
                    if facts.len() > MAX_MODEL_QUEUE_ROWS {
                        return Err(Error::Limit(
                            "more than 32 queue entries; use exact request-ID observation",
                        ));
                    }
                }
                Ok(actor_owner::public_operation(result))
            }
            Observer::Process => {
                let registry = self.processes.as_ref().ok_or(Error::Denied(
                    "host has no registered owned Process observation",
                ))?;
                let handle = args["handle"]
                    .as_str()
                    .ok_or(Error::Denied("Process handle missing"))?;
                let process = registry.resolve(&self.owner, handle)?;
                let guard = process
                    .try_lock()
                    .map_err(|_| Error::Conflict("original Process is currently in use"))?;
                let snapshot = guard.snapshot();
                // NEVER include PID, stdout/stderr bodies, process arguments,
                // filesystem root, host environment or credentials.
                Ok(json!({
                    "kind":"process","handle":handle,"observation":"captured_bytes_only",
                    "stdout_bytes":snapshot.stdout.total_bytes,
                    "stdout_truncated":snapshot.stdout.truncated,
                    "stderr_bytes":snapshot.stderr.total_bytes,
                    "stderr_truncated":snapshot.stderr.truncated,
                    "stdout_read_failed":snapshot.stdout.read_failed,
                    "stderr_read_failed":snapshot.stderr.read_failed,
                    "process_effect_performed":false,
                }))
            }
        }
    }
}
impl Tool for ModelObserverTool {
    fn spec(&self) -> ToolSpec {
        let parameters = match self.kind {
            Observer::Agent => json!({"type":"object","properties":{"op":{"const":"status"}},
                "required":["op"],"additionalProperties":false}),
            Observer::Queue => json!({"type":"object","properties":{
                "op":{"enum":["list","query"]},"request_id":{"type":"string","maxLength":128}},
                "required":["op"],"additionalProperties":false}),
            Observer::Process => json!({"type":"object","properties":{
                "op":{"const":"status"},"handle":{"type":"string","maxLength":64}},
                "required":["op","handle"],"additionalProperties":false}),
        };
        ToolSpec {
            name:self.kind.name().into(),
            description:match self.kind {
                Observer::Agent=>"Read factual current owning Agent Turn metadata, no state mutation",
                Observer::Queue=>"Read redacted own-branch encrypted Input queue receipt metadata; never read private payload or dispatch",
                Observer::Process=>"Read redacted counters from one already host-registered original Process handle; no PID, command, output bytes or controls",
            }.into(),
            parameters,effect:ToolEffect::Read,
        }
    }
    fn is_available(&self) -> bool {
        self.owner.verify_rpc_state().is_ok()
    }
    fn validate(&self, args: &Value) -> Result<()> {
        self.validate_args(args)
    }
    fn execute<'a>(&'a self, ctx: ToolContext, args: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate_args(args)?;
            self.check(&ctx)?;
            let value = self.observation(&ctx, args).await?;
            self.check(&ctx)?;
            let content = serde_json::to_string(&value)
                .map_err(|_| Error::Protocol("cannot serialize bounded model observation"))?;
            let output = ToolOutput::text(content);
            output.validate()?;
            Ok(output)
        })
    }
}

/// Explicit owning-host selection. Installing a tool requires BOTH the exact
/// preexisting Runtime Envelope Host/Read grant and the original Agent's
/// ToolPolicy, which this host API registers under the cached name/effect.
/// No tools are installed by an ordinary Session, prompt or provider schema.
/// Process observation requires an already owned host Process registry;
/// it never grants general OS process discovery or any ProcessEffect.
pub fn install(
    owner: &Service,
    agent: &mut Agent,
    requested: &BTreeSet<Observer>,
    processes: Option<ProcessRegistry>,
) -> Result<()> {
    if requested.is_empty() || requested.len() > 3 {
        return Err(Error::Denied(
            "explicit non-empty 1..3 model observer allowlist required",
        ));
    }
    if requested.contains(&Observer::Process) && processes.is_none() {
        return Err(Error::Denied(
            "model process inspection needs original host-owned Process registry",
        ));
    }
    owner.verify_rpc_state()?;
    owner.verify_rpc_agent(agent)?;
    let identity = agent.default_identity()?;
    let existing = agent.registered_tool_names();
    if existing.len().saturating_add(requested.len()) > 32
        || requested
            .iter()
            .any(|item| existing.iter().any(|name| name == item.name()))
    {
        return Err(Error::Conflict(
            "model management tool already registered or capacity exceeded",
        ));
    }
    // ALL exact grants are preflighted before installing any tool, so an
    // invalid/missing process/queue grant cannot partially expose `agent`.
    for item in requested {
        owner.require_model_observer_grant(item.name())?;
    }
    let mut installed: Vec<&str> = Vec::new();
    for item in requested {
        let result = agent.register_host_tool(Arc::new(ModelObserverTool {
            kind: *item,
            owner: owner.clone(),
            original_identity: identity.clone(),
            processes: processes.clone(),
        }));
        if let Err(error) = result {
            for previous in &installed {
                agent.revoke_registered_source_tool(
                    &identity.actor,
                    previous,
                    &ToolSource::trusted_host(),
                );
            }
            return Err(error);
        }
        installed.push(item.name());
    }
    if let Err(error) = owner.verify_rpc_agent(agent) {
        for name in installed {
            agent.revoke_registered_source_tool(&identity.actor, name, &ToolSource::trusted_host());
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aw_p6_9d1_exact_model_observer_operation_schemas_deny_authority_fields() {
        for (kind, args) in [
            (Observer::Agent, json!({"op":"status"})),
            (Observer::Queue, json!({"op":"list"})),
            (
                Observer::Queue,
                json!({"op":"query","request_id":"qi_receipt"}),
            ),
            (
                Observer::Process,
                json!({"op":"status","handle":"owned_42"}),
            ),
        ] {
            validate_operation(kind, &args).unwrap();
        }
        for (kind, args) in [
            (
                Observer::Agent,
                json!({"op":"status","actor_key":"r1g1a1g1"}),
            ),
            (Observer::Agent, json!({"op":"run"})),
            (Observer::Queue, json!({"op":"enqueue","text":"paid"})),
            (Observer::Queue, json!({"op":"delete","request_id":"x"})),
            (
                Observer::Queue,
                json!({"op":"query","request_id":"../../bad"}),
            ),
            (Observer::Queue, json!({"op":"list","source_epoch":99})),
            (Observer::Process, json!({"op":"kill","handle":"owned_42"})),
            (
                Observer::Process,
                json!({"op":"status","handle":"owned_42","pid":123}),
            ),
            (
                Observer::Process,
                json!({"op":"status","handle":"a".repeat(65)}),
            ),
            (Observer::Process, json!({"op":"status","handle":"/proc/1"})),
        ] {
            assert!(
                validate_operation(kind, &args).is_err(),
                "forged model op must be denied: {args}"
            );
        }
    }
}
