//! AW-P6-9D2A: opt-in model-origin **one future encrypted queued Input**
//! through the ORIGINAL journaled Agent ToolRegistry, ToolPolicy, Actor
//! Envelope, writer-fenced SQLite v33 ActorQueue and private SecretStore.
//!
//! This is NOT a generic `actor_op` passthrough, Process executor, Actor
//! Control, extra model runner, or authorization created by an untrusted model.
//! The trusted host must prebind the original Actor Envelope with an exact
//! Host/trusted-host/epoch1/**Write** grant for name `queue_enqueue`, then
//! explicitly install this tool on the SAME original journaled Agent.
//! Every invocation checks original immutable Dispatching Input and active
//! Runtime Turn, provider/payer, current writer, site and exact source grant.
//! An origin Turn can enqueue AT MOST ONE immutable model job, even if its
//! provider produces multiple tool IDs or tries to replay after an error.
use crate::{actor_owner::Service, Agent, Error, Result};
use aw_agent_core::{
    InvocationIdentity, Tool, ToolContext, ToolEffect, ToolFuture, ToolOutput, ToolSource, ToolSpec,
};
use aw_store::ModelQueueDispositionKind;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;

pub const MAX_MODEL_ENQUEUE_BYTES: usize = 4096;
pub const TOOL_NAME: &str = "queue_enqueue";
#[derive(Clone)]
struct EnqueueTool {
    owner: Service,
    originating_identity: InvocationIdentity,
}
fn text_arg(arguments: &Value) -> Result<&str> {
    let obj = arguments.as_object().ok_or(Error::Denied(
        "model queue enqueue arguments must be exactly one typed object",
    ))?;
    if obj.len() != 2 || obj.get("op").and_then(Value::as_str) != Some("enqueue") {
        return Err(Error::Denied(
            "model queue permits only one future enqueue op with text",
        ));
    }
    let text = obj
        .get("text")
        .and_then(Value::as_str)
        .ok_or(Error::Denied("model queue future Input must be typed text"))?;
    if text.is_empty()
        || text.len() > MAX_MODEL_ENQUEUE_BYTES
        || text.contains('\0')
        || text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(Error::Denied(
            "model queue Input must be safe 1..4096-byte UTF-8 text",
        ));
    }
    Ok(text)
}
impl Tool for EnqueueTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name:TOOL_NAME.into(),
            description:"Queue at most ONE bounded encrypted future user-visible Input for this original Session+branch+Turn. Durable Pending receipt only. Never run the next provider Turn, control Process, grant tools or retry uncertain effects.".into(),
            parameters:json!({"type":"object","properties":{
                "op":{"const":"enqueue"},
                "text":{"type":"string","minLength":1,"maxLength":MAX_MODEL_ENQUEUE_BYTES}
            },"required":["op","text"],"additionalProperties":false}),
            effect:ToolEffect::Write,
        }
    }
    fn is_available(&self) -> bool {
        self.owner.verify_rpc_state().is_ok()
    }
    fn validate(&self, arguments: &Value) -> Result<()> {
        text_arg(arguments).map(|_| ())
    }
    fn execute<'a>(&'a self, ctx: ToolContext, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let text = text_arg(arguments)?;
            self.owner.verify_model_observation(&ctx)?;
            if ctx.identity != self.originating_identity {
                return Err(Error::Denied(
                    "model queue proposal changed original provider/payer/credential",
                ));
            }
            self.owner
                .require_model_tool_grant(TOOL_NAME, &ToolEffect::Write)?;
            let pending = self.owner.enqueue_model_future_text(&ctx, text)?;
            // The ORIGINAL immutable SQLite Pending receipt says only owner
            // acceptance. It cannot imply model execution or paid settlement.
            let output = ToolOutput::text(
                serde_json::to_string(&json!({
                    "kind":"queued_future_input","request_id":pending.request_id,
                    "state":pending.state,"revision":pending.revision,
                    "target":"auto","priority":0,
                    "execution":"NOT_DISPATCHED","retry":"NEVER_AUTOMATIC",
                }))
                .map_err(|_| Error::Storage("cannot encode model queue accepted receipt"))?,
            );
            output.validate()?;
            Ok(output)
        })
    }
}
/// Install ONE explicit **Write** effect only when this existing original
/// HostRuntime Actor Envelope already grants this exact tool name/source/
/// epoch/effect. The original `Agent::register_host_tool` then adds the same
/// ToolPolicy Write grant; the Agent rechecks both before/after dispatch and
/// records original ToolStarted/ToolFinished facts in its existing Journal.
/// This never enables ordinary CLI/R12/remote/TUI sessions by default.
pub fn install_queue_enqueue(owner: &Service, agent: &mut Agent) -> Result<()> {
    owner.verify_rpc_state()?;
    owner.verify_rpc_agent(agent)?;
    if agent
        .registered_tool_names()
        .iter()
        .any(|name| name == TOOL_NAME)
    {
        return Err(Error::Conflict("model queue enqueue already installed"));
    }
    owner.require_model_tool_grant(TOOL_NAME, &ToolEffect::Write)?;
    let identity = agent.default_identity()?;
    agent.register_host_tool(Arc::new(EnqueueTool {
        owner: owner.clone(),
        originating_identity: identity.clone(),
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
/// P6-9D2B1: TWO separate exact Host/Write capabilities. Neither is a
/// generic ActorOperation or privilege to change a user's queued Input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DispositionTool {
    Update,
    Delete,
}
impl DispositionTool {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Update => "queue_update",
            Self::Delete => "queue_delete",
        }
    }
    const fn kind(self) -> ModelQueueDispositionKind {
        match self {
            Self::Update => ModelQueueDispositionKind::Update,
            Self::Delete => ModelQueueDispositionKind::Delete,
        }
    }
}
#[derive(Clone)]
struct ModelQueueDispositionTool {
    owner: Service,
    identity: InvocationIdentity,
    tool: DispositionTool,
}
struct DispositionArguments<'a> {
    request_id: &'a str,
    expected_revision: u64,
    replacement: Option<&'a str>,
}
fn disposition_args(tool: DispositionTool, args: &Value) -> Result<DispositionArguments<'_>> {
    let map = args.as_object().ok_or(Error::Denied(
        "model queue disposition must be an exact typed object",
    ))?;
    if map.get("op").and_then(Value::as_str)
        != Some(match tool {
            DispositionTool::Update => "update",
            DispositionTool::Delete => "delete",
        })
        || map.len()
            != match tool {
                DispositionTool::Update => 4,
                DispositionTool::Delete => 3,
            }
    {
        return Err(Error::Denied(
            "model queue disposition contains unsupported action or authority field",
        ));
    }
    let request_id = map
        .get("request_id")
        .and_then(Value::as_str)
        .ok_or(Error::Denied("missing exact model queue target ID"))?;
    if !request_id.starts_with("mq_")
        || request_id.len() != 67
        || !request_id[3..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error::Denied(
            "model may only mutate an exact prior model-created mq_ queue ID",
        ));
    }
    let revision = map
        .get("expected_revision")
        .and_then(Value::as_u64)
        .filter(|rev| *rev > 0)
        .ok_or(Error::Denied(
            "queue disposition needs positive exact CAS revision",
        ))?;
    let replacement = if tool == DispositionTool::Update {
        let body = map
            .get("text")
            .and_then(Value::as_str)
            .ok_or(Error::Denied(
                "model queue update requires replacement text",
            ))?;
        if body.is_empty()
            || body.len() > MAX_MODEL_ENQUEUE_BYTES
            || body.contains('\0')
            || body
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(Error::Denied(
                "model disposition text requires bounded safe UTF-8",
            ));
        }
        Some(body)
    } else {
        None
    };
    Ok(DispositionArguments {
        request_id,
        expected_revision: revision,
        replacement,
    })
}
impl Tool for ModelQueueDispositionTool {
    fn spec(&self) -> ToolSpec {
        let mut fields = json!({"op":{"const":match self.tool {
            DispositionTool::Update=>"update",DispositionTool::Delete=>"delete"}},
            "request_id":{"type":"string","pattern":"^mq_[a-f0-9]{64}$"},
            "expected_revision":{"type":"integer","minimum":1}});
        let mut required = vec!["op", "request_id", "expected_revision"];
        if self.tool == DispositionTool::Update {
            fields
                .as_object_mut()
                .expect("static schema object")
                .insert(
                    "text".into(),
                    json!({"type":"string","minLength":1,"maxLength":MAX_MODEL_ENQUEUE_BYTES}),
                );
            required.push("text");
        }
        ToolSpec {
            name:self.tool.name().into(),
            description:match self.tool {
                DispositionTool::Update=>"Replace only the encrypted body of one previous model-owned Pending Input at its exact revision. One model queue disposition per original Turn; no priority/recipient change or future execution.",
                DispositionTool::Delete=>"Retire only a previous model-owned Pending Input at exact revision, never a claimed/accepted User Input. One model queue disposition per original Turn; no provider dispatch.",
            }.into(),
            parameters:json!({"type":"object","properties":fields,
                "required":required,"additionalProperties":false}),
            effect:ToolEffect::Write,
        }
    }
    fn is_available(&self) -> bool {
        self.owner.verify_rpc_state().is_ok()
    }
    fn validate(&self, args: &Value) -> Result<()> {
        disposition_args(self.tool, args).map(|_| ())
    }
    fn execute<'a>(&'a self, ctx: ToolContext, args: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let typed = disposition_args(self.tool, args)?;
            self.owner.verify_model_observation(&ctx)?;
            if ctx.identity != self.identity {
                return Err(Error::Denied(
                    "model queue disposition changed original provider/account/payer",
                ));
            }
            self.owner
                .require_model_tool_grant(self.tool.name(), &ToolEffect::Write)?;
            let receipt = self.owner.dispose_model_queue(
                &ctx,
                self.tool.kind(),
                typed.request_id,
                typed.expected_revision,
                typed.replacement,
            )?;
            let output = ToolOutput::text(
                serde_json::to_string(&json!({
                    "kind":"queue_model_disposition","action":match self.tool {
                        DispositionTool::Update=>"updated",DispositionTool::Delete=>"deleted"},
                    "request_id":receipt.request_id,"revision":receipt.revision,
                    "state":receipt.state,"execution":"NOT_DISPATCHED",
                    "retry":"NEVER_AUTOMATIC",
                }))
                .map_err(|_| Error::Protocol("cannot encode redacted model disposition receipt"))?,
            );
            output.validate()?;
            Ok(output)
        })
    }
}
/// Host-only, all-or-nothing installation. Each action requires its OWN
/// preexisting exact Host/trusted-host/epoch1/**Write** Actor Envelope grant,
/// and its own original ToolPolicy source/effect grant. The model may not
/// substitute D1 Read permission, an extension source or raw RPC ActorOp.
pub fn install_queue_dispositions(
    owner: &Service,
    agent: &mut Agent,
    requested: &BTreeSet<DispositionTool>,
) -> Result<()> {
    if requested.is_empty() || requested.len() > 2 {
        return Err(Error::Denied(
            "explicit 1..2 model queue disposition allowlist required",
        ));
    }
    owner.verify_rpc_state()?;
    owner.verify_rpc_agent(agent)?;
    let prior = agent.registered_tool_names();
    if prior.len().saturating_add(requested.len()) > 32
        || requested
            .iter()
            .any(|item| prior.iter().any(|name| name == item.name()))
    {
        return Err(Error::Conflict(
            "model disposition tool already registered or no capacity",
        ));
    }
    for item in requested {
        owner.require_model_tool_grant(item.name(), &ToolEffect::Write)?;
    }
    let original = agent.default_identity()?;
    let mut installed = Vec::new();
    for item in requested {
        let result = agent.register_host_tool(Arc::new(ModelQueueDispositionTool {
            owner: owner.clone(),
            identity: original.clone(),
            tool: *item,
        }));
        if let Err(error) = result {
            for previous in installed {
                agent.revoke_registered_source_tool(
                    &original.actor,
                    previous,
                    &ToolSource::trusted_host(),
                );
            }
            return Err(error);
        }
        installed.push(item.name());
    }
    if let Err(error) = owner.verify_rpc_agent(agent) {
        for installed in installed {
            agent.revoke_registered_source_tool(
                &original.actor,
                installed,
                &ToolSource::trusted_host(),
            );
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aw_p6_9d2b_model_queue_update_delete_schema_denies_privilege_and_wrong_target() {
        let mq = format!("mq_{}", "a".repeat(64));
        let ok_update = json!({"op":"update","request_id":mq,
            "expected_revision":1,"text":"bounded updated text"});
        let ok_delete = json!({"op":"delete","request_id":format!("mq_{}","a".repeat(64)),
            "expected_revision":1});
        assert!(disposition_args(DispositionTool::Update, &ok_update).is_ok());
        assert!(disposition_args(DispositionTool::Delete, &ok_delete).is_ok());
        let target = format!("mq_{}", "a".repeat(64));
        for (kind, args) in [
            (
                DispositionTool::Update,
                json!({"op":"update","request_id":target,"text":"body"}),
            ),
            (
                DispositionTool::Update,
                json!({"op":"update","request_id":target,"expected_revision":1,"text":"body","priority":-3}),
            ),
            (
                DispositionTool::Update,
                json!({"op":"update","request_id":target,"expected_revision":1,"text":"body","actor_key":"forged"}),
            ),
            (
                DispositionTool::Update,
                json!({"op":"update","request_id":target,"expected_revision":1,"text":"body","writer_epoch":9}),
            ),
            (
                DispositionTool::Update,
                json!({"op":"update","request_id":target,"expected_revision":1,"text":""}),
            ),
            (
                DispositionTool::Update,
                json!({"op":"update","request_id":target,"expected_revision":1,"text":"\u{001b}[31m"}),
            ),
            (
                DispositionTool::Delete,
                json!({"op":"delete","request_id":target,"expected_revision":1,"text":"smuggle"}),
            ),
            (
                DispositionTool::Delete,
                json!({"op":"delete","request_id":target,"expected_revision":0}),
            ),
            (
                DispositionTool::Delete,
                json!({"op":"delete","request_id":target,"expected_revision":1,"target":"other"}),
            ),
            (
                DispositionTool::Delete,
                json!({"op":"delete","request_id":"user_request","expected_revision":1}),
            ),
            (
                DispositionTool::Delete,
                json!({"op":"delete","request_id":"mq_zz","expected_revision":1}),
            ),
            (
                DispositionTool::Delete,
                json!({"op":"cancel","request_id":target,"expected_revision":1}),
            ),
        ] {
            assert!(
                disposition_args(kind, &args).is_err(),
                "model queue disposition may not supply unreviewed authority: {args}"
            );
        }
    }

    #[test]
    fn aw_p6_9d2a_enqueued_tool_denies_all_client_supplied_authority_fields() {
        let valid = json!({"op":"enqueue","text":"future"});
        assert_eq!(text_arg(&valid).unwrap(), "future");
        for input in [
            json!({"op":"cancel","text":"future"}),
            json!({"op":"enqueue","text":""}),
            json!({"op":"enqueue","text":"a".repeat(4097)}),
            json!({"op":"enqueue","text":"line\0break"}),
            json!({"op":"enqueue","text":"safe","actor_key":"forged"}),
            json!({"op":"enqueue","text":"safe","writer_epoch":99}),
            json!({"op":"enqueue","text":"safe","priority":-20}),
            json!({"op":"enqueue","text":"safe","message_id":123}),
            json!({"op":"enqueue","text":"safe","session_id":"other"}),
            json!({"op":"enqueue","text":"safe","source":"trusted-host"}),
            json!({"op":"enqueue","text":"safe","tool_grant":true}),
            json!({"op":"enqueue","text":"safe","target":"other_actor"}),
        ] {
            assert!(
                text_arg(&input).is_err(),
                "must deny untrusted model authority: {input}"
            );
        }
    }
}
