//! AW-P6-9D2B2A: independently Host/Write-granted model request to CLOSE
//! stdin of an already authorized original host Process (NOT a model spawn,
//! subprocess shell, PID lookup, generic signal, Agent cancel or OS freezer).
//! One immutable SQLite v33 Process intent is committed BEFORE dropping the
//! original PipeProcess stdin. Crash/ambiguous outcomes require inspection;
//! there is no retry, second runner, new ToolRegistry or new Process owner.
use crate::{actor_owner::Service, model_management::ProcessRegistry, Agent, Error, Result};
use aw_agent_core::{
    InvocationIdentity, Tool, ToolContext, ToolEffect, ToolFuture, ToolOutput, ToolSource, ToolSpec,
};
use aw_store::ModelProcessState;
use serde_json::{json, Value};
use std::sync::Arc;

pub const TOOL_NAME: &str = "process_close_stdin";
#[derive(Clone)]
struct CloseStdinTool {
    owner: Service,
    provider_identity: InvocationIdentity,
    registry: ProcessRegistry,
}
fn input(args: &Value) -> Result<&str> {
    let obj = args.as_object().ok_or(Error::Denied(
        "model Process arguments require exact typed object",
    ))?;
    if obj.len() != 2 || obj.get("op").and_then(Value::as_str) != Some("close_stdin") {
        return Err(Error::Denied(
            "model Process permits only close_stdin, no other operation",
        ));
    }
    let handle = obj
        .get("handle")
        .and_then(Value::as_str)
        .ok_or(Error::Denied("model Process handle is missing"))?;
    if handle.is_empty()
        || handle.len() > 64
        || !handle
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    {
        return Err(Error::Denied(
            "model Process handle is not a host-registered opaque ID",
        ));
    }
    Ok(handle)
}
impl Tool for CloseStdinTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name:TOOL_NAME.into(),
            description:"Close stdin on ONE exact previously host-authorized and journaled Process. Does not spawn/kill/signal, send bytes, expose PID or output, or run another Agent. Original SQLite intent prepared first, no replay after uncertainty.".into(),
            parameters:json!({"type":"object","properties":{
                "op":{"const":"close_stdin"},
                "handle":{"type":"string","minLength":1,"maxLength":64}
            },"required":["op","handle"],"additionalProperties":false}),
            effect:ToolEffect::Write,
        }
    }
    fn is_available(&self) -> bool {
        self.owner.verify_rpc_state().is_ok()
    }
    fn validate(&self, args: &Value) -> Result<()> {
        input(args).map(|_| ())
    }
    fn execute<'a>(&'a self, ctx: ToolContext, args: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let handle = input(args)?;
            self.owner.verify_model_observation(&ctx)?;
            if ctx.identity != self.provider_identity {
                return Err(Error::Denied(
                    "Process Write Tool provider/payer/credential binding changed",
                ));
            }
            self.owner
                .require_model_tool_grant(TOOL_NAME, &ToolEffect::Write)?;
            // Resolving this registry does not create a Process. An unknown,
            // stale or other-Session handle is denied before any SQLite fact.
            let process = self.registry.resolve(&self.owner, handle)?;
            let mut guard = process.try_lock().map_err(|_| {
                Error::Conflict("original Process is busy; do not wait or retry automatically")
            })?;
            if !guard.stdin_open() {
                return Err(Error::Denied("original Process stdin is not open"));
            }
            // This is the irreversible intent boundary. Every future error
            // (owner revocation, crash, cancelled Turn or loss of Process)
            // leaves Prepared/Unknown for inspection, NEVER automatic retry.
            let intent = self.owner.prepare_model_process_close(&ctx, handle)?;
            self.owner.verify_model_observation(&ctx)?;
            guard.close_stdin();
            let applied =
                self.owner
                    .finish_model_process_close(&ctx, &intent, ModelProcessState::Applied)?;
            self.owner.verify_model_observation(&ctx)?;
            let result = ToolOutput::text(
                serde_json::to_string(&json!({
                    "kind":"original_process_stdin_close",
                    "handle":handle,"state":applied.state,
                    "fact":applied.request.intent_id,
                    "scope":"exact_original_owner",
                    "process_exit":"NOT_PROVEN",
                    "delivery":"NOT_PROVEN",
                    "retry":"NEVER_AUTOMATIC"
                }))
                .map_err(|_| Error::Protocol("cannot encode reduced Process effect receipt"))?,
            );
            result.validate()?;
            Ok(result)
        })
    }
}
/// Explicit local host SDK. The original Agent/ToolPolicy receives the
/// matching Host/Write grant ONLY after the current original Envelope has
/// already permitted this exact action/site/epoch. A ProcessRegistry is an
/// existing host-owned pointer to a separately granted/journaled Process;
/// this Tool cannot select PID/command/root, spawn or terminate anything.
pub fn install_close_stdin(
    owner: &Service,
    agent: &mut Agent,
    registry: ProcessRegistry,
) -> Result<()> {
    owner.verify_rpc_state()?;
    owner.verify_rpc_agent(agent)?;
    if agent.registered_tool_names().iter().any(|n| n == TOOL_NAME) {
        return Err(Error::Conflict(
            "model Process stdin close already installed",
        ));
    }
    owner.require_model_tool_grant(TOOL_NAME, &ToolEffect::Write)?;
    let identity = agent.default_identity()?;
    agent.register_host_tool(Arc::new(CloseStdinTool {
        owner: owner.clone(),
        provider_identity: identity.clone(),
        registry,
    }))?;
    if let Err(error) = owner.verify_rpc_agent(agent) {
        agent.revoke_registered_source_tool(
            &identity.actor,
            TOOL_NAME,
            &ToolSource::trusted_host(),
        );
        return Err(error);
    }
    Ok(())
}
/// AW-P6-9D2B2B1: Termination is a DISTINCT destructive Host/Write
/// capability from 9D2B2A stdin-close. A normal ProcessRegistry::register_owned
/// handle is deliberately NOT terminable; only an independently approved
/// register_terminable_owned handle may be used.
pub const TERMINATE_TOOL_NAME: &str = "process_terminate";
#[derive(Clone)]
struct TerminateTool {
    owner: Service,
    provider_identity: InvocationIdentity,
    registry: ProcessRegistry,
}
fn termination_input(args: &Value) -> Result<&str> {
    let obj = args.as_object().ok_or(Error::Denied(
        "model Process termination requires an exact typed object",
    ))?;
    if obj.len() != 2 || obj.get("op").and_then(Value::as_str) != Some("terminate") {
        return Err(Error::Denied(
            "model termination permits only terminate, not arbitrary signals",
        ));
    }
    let handle = obj
        .get("handle")
        .and_then(Value::as_str)
        .ok_or(Error::Denied(
            "model termination requires opaque host-registered handle",
        ))?;
    if handle.is_empty()
        || handle.len() > 64
        || !handle
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    {
        return Err(Error::Denied(
            "model termination target must be an opaque registered Process",
        ));
    }
    Ok(handle)
}
impl Tool for TerminateTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name:TERMINATE_TOOL_NAME.into(),
            description:"Request bounded termination ONLY of one explicitly host-approved existing original Process with its unique bound incarnation. Original SQLite Prepared intent precedes OS termination. No raw PID, signals, executable path, arbitrary stdin, Agent cancellation, process spawn or retry after uncertainty.".into(),
            parameters:json!({"type":"object","properties":{
                "op":{"const":"terminate"},
                "handle":{"type":"string","minLength":1,"maxLength":64}
            },"required":["op","handle"],"additionalProperties":false}),
            effect:ToolEffect::Write,
        }
    }
    fn is_available(&self) -> bool {
        self.owner.verify_rpc_state().is_ok()
    }
    fn validate(&self, args: &Value) -> Result<()> {
        termination_input(args).map(|_| ())
    }
    fn execute<'a>(&'a self, ctx: ToolContext, args: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let handle = termination_input(args)?;
            self.owner.verify_model_observation(&ctx)?;
            if ctx.identity != self.provider_identity {
                return Err(Error::Denied(
                    "model termination changed original account/payer/credential",
                ));
            }
            self.owner
                .require_model_tool_grant(TERMINATE_TOOL_NAME, &ToolEffect::Write)?;
            // Separate explicit host registration binds a random original
            // Process incarnation; observation-only registrations fail here.
            let (process, binding) = self.registry.resolve_terminable(&self.owner, handle)?;
            let mut guard = process.try_lock().map_err(|_| {
                Error::Conflict("original Process is currently busy, no termination replay")
            })?;
            if !guard.termination_available()? {
                return Err(Error::Denied(
                    "original Process already exited or termination previously attempted",
                ));
            }
            // Durable intent must commit BEFORE touching any original OS
            // process group. Crash here leaves Prepared and blocks replay.
            let intent = self
                .owner
                .prepare_model_process_terminate(&ctx, handle, &binding)?;
            self.owner.verify_model_observation(&ctx)?;
            let termination = guard.terminate().await?;
            let completed = matches!(termination.outcome, aw_tools::process::Outcome::Cancelled)
                && termination.output_complete;
            let terminal = if completed {
                ModelProcessState::Applied
            } else {
                ModelProcessState::Unknown
            };
            let fact = self
                .owner
                .finish_model_process_terminate(&ctx, &intent, terminal)?;
            if !completed {
                return Err(Error::Conflict("OS Process termination outcome uncertain; inspect immutable original receipt, never replay"));
            }
            self.owner.verify_model_observation(&ctx)?;
            // This is a bounded direct-child wait receipt, NOT proof that
            // every OS group descendant exited or a paid provider settled.
            let output = ToolOutput::text(
                serde_json::to_string(&json!({
                    "kind":"original_process_termination",
                    "handle":handle,"state":fact.state,
                    "fact":fact.request.intent_id,
                    "direct_child_reaped":true,
                    "process_group":"BEST_EFFORT",
                    "provider_settlement":"NOT_PROVEN",
                    "retry":"NEVER_AUTOMATIC"
                }))
                .map_err(|_| {
                    Error::Protocol("cannot encode reduced original Process termination receipt")
                })?,
            );
            output.validate()?;
            Ok(output)
        })
    }
}
/// Opt-in current owning host ONLY. `register_terminable_owned` must have
/// separately approved this exact original Process and its random incarnation.
/// This module never grants permission to a model or ProcessRegistry handle.
pub fn install_terminate(
    owner: &Service,
    agent: &mut Agent,
    registry: ProcessRegistry,
) -> Result<()> {
    owner.verify_rpc_state()?;
    owner.verify_rpc_agent(agent)?;
    if agent
        .registered_tool_names()
        .iter()
        .any(|n| n == TERMINATE_TOOL_NAME)
    {
        return Err(Error::Conflict(
            "model Process terminate Tool already installed",
        ));
    }
    owner.require_model_tool_grant(TERMINATE_TOOL_NAME, &ToolEffect::Write)?;
    let identity = agent.default_identity()?;
    agent.register_host_tool(Arc::new(TerminateTool {
        owner: owner.clone(),
        provider_identity: identity.clone(),
        registry,
    }))?;
    if let Err(error) = owner.verify_rpc_agent(agent) {
        agent.revoke_registered_source_tool(
            &identity.actor,
            TERMINATE_TOOL_NAME,
            &ToolSource::trusted_host(),
        );
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aw_p6_9d2b2b1_process_terminate_rejects_model_supplied_os_authority() {
        assert_eq!(
            termination_input(&json!({"op":"terminate","handle":"host_bound"})).unwrap(),
            "host_bound"
        );
        for args in [
            json!({"op":"terminate","handle":""}),
            json!({"op":"terminate","handle":"/proc/1"}),
            json!({"op":"terminate","handle":"h".repeat(65)}),
            json!({"op":"terminate","handle":"host_bound","pid":42}),
            json!({"op":"terminate","handle":"host_bound","actor_key":"forged"}),
            json!({"op":"terminate","handle":"host_bound","writer_epoch":99}),
            json!({"op":"terminate","handle":"host_bound","source_epoch":1}),
            json!({"op":"terminate","handle":"host_bound","signal":"SIGKILL"}),
            json!({"op":"terminate","handle":"host_bound","stdin":"echo attacker"}),
            json!({"op":"terminate","handle":"host_bound","program":"/bin/sh"}),
            json!({"op":"terminate","handle":"host_bound","grant":true}),
            json!({"op":"kill","handle":"host_bound"}),
            json!({"op":"cancel_agent","handle":"host_bound"}),
        ] {
            assert!(
                termination_input(&args).is_err(),
                "untrusted model may never supply OS/Actor human control authority: {args}"
            );
        }
    }
    #[test]
    fn aw_p6_9d2b2a_model_process_rejects_authority_extra_fields_and_arbitrary_io() {
        assert_eq!(
            input(&json!({"op":"close_stdin","handle":"owned_1"})).unwrap(),
            "owned_1"
        );
        for args in [
            json!({"op":"kill","handle":"owned_1"}),
            json!({"op":"signal","handle":"owned_1","signal":"TERM"}),
            json!({"op":"write","handle":"owned_1","stdin":"rm -rf /"}),
            json!({"op":"close_stdin","handle":"/proc/1/fd/0"}),
            json!({"op":"close_stdin","handle":""}),
            json!({"op":"close_stdin","handle":"owned_1","pid":42}),
            json!({"op":"close_stdin","handle":"owned_1","writer_epoch":100}),
            json!({"op":"close_stdin","handle":"owned_1","actor_key":"forged"}),
            json!({"op":"close_stdin","handle":"owned_1","tool_grant":true}),
            json!({"op":"close_stdin","handle":"a".repeat(65)}),
        ] {
            assert!(
                input(&args).is_err(),
                "untrusted model Process authority rejected: {args}"
            );
        }
    }
}
