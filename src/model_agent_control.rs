//! AW-P6-9D2B2B2: original model can opt into one Host/Write-granted
//! *future-turn admission hold*, NEVER cancel/freeze the currently paid Turn.
//! This model Tool uses the original Agent/ToolRegistry/ToolPolicy/TurnJournal
//! and SQLite v33 writer/immutable actor.input audit. There is no Control
//! message, Process signal, new runner, background queue drain or human grant.
use crate::{actor_owner::Service, Agent, Error, Result};
use aw_agent_core::{
    InvocationIdentity, Tool, ToolContext, ToolEffect, ToolFuture, ToolOutput, ToolSource, ToolSpec,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub const TOOL_NAME: &str = "agent_pause_after_turn";
#[derive(Clone)]
struct PauseAfterTurnTool {
    owner: Service,
    original_identity: InvocationIdentity,
}
fn validate(args: &Value) -> Result<()> {
    let object = args.as_object().ok_or(Error::Denied(
        "model Agent future pause takes one strict typed object",
    ))?;
    if object.len() != 1 || object.get("op").and_then(Value::as_str) != Some("pause_after_turn") {
        return Err(Error::Denied(
            "model Agent self-cancel/freeze/resume, child commands and arbitrary owner fields are forbidden"));
    }
    Ok(())
}
impl Tool for PauseAfterTurnTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name:TOOL_NAME.into(),
            description:"Request that the CURRENT original Agent Turn finish normally, then hold FUTURE original Session/branch Input admissions and queued Input claims until the separately authenticated owner explicitly resumes. Never interrupt your current provider/Tool receipt, self-cancel, change another Agent or grant human approval.".into(),
            parameters:json!({"type":"object","properties":{
                "op":{"const":"pause_after_turn"}
            },"required":["op"],"additionalProperties":false}),
            effect:ToolEffect::Write,
        }
    }
    fn is_available(&self) -> bool {
        self.owner.verify_rpc_state().is_ok()
    }
    fn validate(&self, args: &Value) -> Result<()> {
        validate(args)
    }
    fn execute<'a>(&'a self, ctx: ToolContext, args: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            validate(args)?;
            self.owner.verify_model_observation(&ctx)?;
            if ctx.identity != self.original_identity {
                return Err(Error::Denied(
                    "model future Agent pause changed provider/account/payer/credential identity",
                ));
            }
            self.owner
                .require_model_tool_grant(TOOL_NAME, &ToolEffect::Write)?;
            let fact = self.owner.request_model_pause_after_turn(&ctx)?;
            let output = ToolOutput::text(
                serde_json::to_string(&json!({
                    "kind":"original_agent_future_admission_hold",
                    "fact":fact.request.intent_id,
                    "state":fact.state,
                    "scope":"AFTER_CURRENT_ORIGINAL_TURN",
                    "current_turn_interrupted":false,
                    "future_input_admissions":"HELD",
                    "release":"EXPLICIT_AUTHENTICATED_OWNER_ONLY",
                    "retry":"NEVER_AUTOMATIC"
                }))
                .map_err(|_| {
                    Error::Protocol("cannot encode bounded original Agent pause receipt")
                })?,
            );
            output.validate()?;
            Ok(output)
        })
    }
}
/// Trusted owning Host SDK ONLY. The original Actor Envelope MUST already
/// contain the exact Host/trusted-host/epoch1/**Write** grant for this name.
/// Original Agent::register_host_tool supplies only the matching ToolPolicy
/// grant; ordinary CLI, TUI, RPC, AwNet, project text and models cannot mint
/// either permission or call the separate original owner release method.
pub fn install_pause_after_turn(owner: &Service, agent: &mut Agent) -> Result<()> {
    owner.verify_rpc_state()?;
    owner.verify_rpc_agent(agent)?;
    if agent
        .registered_tool_names()
        .iter()
        .any(|name| name == TOOL_NAME)
    {
        return Err(Error::Conflict(
            "model Agent future pause tool already registered",
        ));
    }
    owner.require_model_tool_grant(TOOL_NAME, &ToolEffect::Write)?;
    let identity = agent.default_identity()?;
    agent.register_host_tool(Arc::new(PauseAfterTurnTool {
        owner: owner.clone(),
        original_identity: identity.clone(),
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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aw_p6_9d2b2b2_model_cannot_immediate_cancel_freeze_child_or_grant_human_approval() {
        assert!(validate(&json!({"op":"pause_after_turn"})).is_ok());
        for args in [
            json!({"op":"cancel"}),
            json!({"op":"freeze"}),
            json!({"op":"freeze_actor"}),
            json!({"op":"resume"}),
            json!({"op":"pause_after_turn","writer_epoch":8}),
            json!({"op":"pause_after_turn","actor_key":"r2g2a1g1"}),
            json!({"op":"pause_after_turn","turn_id":17}),
            json!({"op":"pause_after_turn","target":"child"}),
            json!({"op":"pause_after_turn","human_approved":true}),
            json!({"op":"pause_after_turn","model":"another-provider"}),
            json!({"op":"pause_after_turn","source":"trusted-host"}),
            json!({"op":"pause_after_turn","resume":true}),
            json!({"op":"pause_after_turn","cancel_current_turn":true}),
        ] {
            assert!(
                validate(&args).is_err(),
                "model Agent Tool may not mint Control: {args}"
            );
        }
    }
}
