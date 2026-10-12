//! Small host callback projection for the exact shared EmbeddedTurn.
use crate::{AwE5CallbacksV1, Binding, MAX_CALLBACK_JSON, MAX_RESULT_JSON, MAX_TOOLS};
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use aw_agent_kernel::capability::{Capability, Effect, Grant, Source, SourceKind};
use aw_embedded::{
    Finish, HostPorts, Identity, JournalRecord, Message, ModelReply, PortError, Progress, Role,
    ToolCall, ToolRegistration, ToolResult, MAX_ARGUMENT_BYTES, MAX_OUTPUT_BYTES, MAX_RESULT_BYTES,
};
use serde_json::{json, Value};

pub(crate) struct Ports {
    pub(crate) cb: AwE5CallbacksV1,
    pub(crate) actor: String,
    pub(crate) site: String,
    pub(crate) bindings: Vec<Binding>,
    // Model attempts may be billable even when callbacks fail. Track only
    // COMMITTED journal acknowledgments, never a presumed provider rollback.
    pub(crate) model_started_committed: u32,
    pub(crate) model_settled_committed: u32,
}
pub(crate) fn source_code(kind: SourceKind) -> u32 {
    match kind {
        SourceKind::Host => 1,
        SourceKind::Mcp => 2,
        SourceKind::Extension => 3,
    }
}
pub(crate) fn effect_name(effect: Effect) -> &'static str {
    match effect {
        Effect::Read => "read",
        Effect::Write => "write",
        Effect::Process => "process",
        Effect::Unknown => "unknown",
    }
}
pub(crate) fn progress_code(value: Progress) -> u32 {
    match value {
        Progress::NeedsModel => 2,
        Progress::NeedsTool => 3,
        Progress::Completed => 4,
        Progress::Incomplete => 5,
    }
}
fn only_keys(value: &Value, fields: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|obj| obj.keys().all(|key| fields.contains(&key.as_str())))
}
fn parse_reply(value: Value) -> Result<ModelReply, PortError> {
    if !only_keys(&value, &["text", "finish", "calls"]) {
        return Err(PortError::Failed);
    }
    let text = value
        .get("text")
        .and_then(Value::as_str)
        .ok_or(PortError::Failed)?;
    if text.len() > MAX_OUTPUT_BYTES {
        return Err(PortError::Failed);
    }
    let finish = match value.get("finish").and_then(Value::as_str) {
        Some("final") => Finish::Final,
        Some("tools") => Finish::Tools,
        Some("incomplete") => Finish::Incomplete,
        _ => return Err(PortError::Failed),
    };
    let input = value
        .get("calls")
        .and_then(Value::as_array)
        .ok_or(PortError::Failed)?;
    if input.len() > MAX_TOOLS {
        return Err(PortError::Failed);
    }
    let mut calls = Vec::new();
    calls
        .try_reserve_exact(input.len())
        .map_err(|_| PortError::Failed)?;
    for entry in input {
        if !only_keys(entry, &["id", "name", "arguments"]) {
            return Err(PortError::Failed);
        }
        let id = entry
            .get("id")
            .and_then(Value::as_str)
            .ok_or(PortError::Failed)?;
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .ok_or(PortError::Failed)?;
        let arguments = entry.get("arguments").ok_or(PortError::Failed)?;
        if id.is_empty()
            || id.len() > 128
            || name.is_empty()
            || name.len() > 64
            || !arguments.is_object()
            || serde_json::to_vec(arguments)
                .map_err(|_| PortError::Failed)?
                .len()
                > MAX_ARGUMENT_BYTES
        {
            return Err(PortError::Failed);
        }
        calls.push(ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: arguments.clone(),
        });
    }
    Ok(ModelReply {
        text: text.to_string(),
        finish,
        calls,
    })
}
impl Ports {
    fn translate_status(code: u32) -> PortError {
        match code {
            1 => PortError::Unavailable,
            2 => PortError::Denied,
            _ => PortError::Failed,
        }
    }
    fn exchange(&mut self, callback: crate::Exchange, request: Value) -> Result<Value, PortError> {
        let input = serde_json::to_vec(&request).map_err(|_| PortError::Failed)?;
        if input.len() > MAX_CALLBACK_JSON {
            return Err(PortError::Failed);
        }
        let mut output = Vec::new();
        output
            .try_reserve_exact(MAX_RESULT_JSON)
            .map_err(|_| PortError::Failed)?;
        output.resize(MAX_RESULT_JSON, 0);
        let mut written = 0usize;
        let code = callback(
            self.cb.context,
            input.as_ptr(),
            input.len(),
            output.as_mut_ptr(),
            output.len(),
            &mut written,
        );
        if code != 0 {
            return Err(Self::translate_status(code));
        }
        if written == 0 || written > output.len() {
            return Err(PortError::Failed);
        }
        serde_json::from_slice(&output[..written]).map_err(|_| PortError::Failed)
    }
    fn gate(&self, callback: crate::Gate, request: Value) -> bool {
        match serde_json::to_vec(&request) {
            Ok(input) if input.len() <= MAX_CALLBACK_JSON => {
                callback(self.cb.context, input.as_ptr(), input.len()) == 0
            }
            _ => false,
        }
    }
}
impl HostPorts for Ports {
    fn monotonic_ticks(&mut self) -> Option<u64> {
        let callback = self.cb.clock?;
        let mut ticks = 0u64;
        (callback(self.cb.context, &mut ticks) == 0).then_some(ticks)
    }
    fn secure_entropy(&mut self, out: &mut [u8]) -> bool {
        self.cb
            .entropy
            .is_some_and(|cb| cb(self.cb.context, out.as_mut_ptr(), out.len()) == 0)
    }
    fn append(&mut self, record: JournalRecord<'_>) -> Result<(), PortError> {
        let (kind, value) = match record {
            JournalRecord::TurnStarted {
                turn_id,
                nonce,
                actor,
                account,
                model,
                prompt,
            } => (
                1,
                json!({"v":1,"turn_id":turn_id,"nonce":nonce,
                    "actor":actor,"account":account,"model":model,"prompt":prompt}),
            ),
            JournalRecord::ModelStarted { turn_id, ordinal } => {
                (2, json!({"v":1,"turn_id":turn_id,"ordinal":ordinal}))
            }
            JournalRecord::ModelSettled {
                turn_id,
                ordinal,
                finish,
            } => (
                3,
                json!({"v":1,"turn_id":turn_id,"ordinal":ordinal,
                    "finish":match finish {Finish::Final=>"final",Finish::Tools=>"tools",Finish::Incomplete=>"incomplete"}}),
            ),
            JournalRecord::Assistant {
                turn_id,
                text,
                tools,
            } => (
                4,
                json!({"v":1,"turn_id":turn_id,"text":text,"tools":tools}),
            ),
            JournalRecord::ToolIntent {
                turn_id,
                call,
                effect,
            } => (
                5,
                json!({"v":1,"turn_id":turn_id,"call":{"id":call.id,"name":call.name,
                    "arguments":call.arguments},"effect":effect_name(effect)}),
            ),
            JournalRecord::ToolReceipt {
                turn_id,
                call,
                output,
            } => (
                6,
                json!({"v":1,"turn_id":turn_id,"id":call.id,"name":call.name,
                    "content":output.content,"is_error":output.is_error}),
            ),
            JournalRecord::TurnFinished { turn_id, outcome } => (
                7,
                json!({"v":1,"turn_id":turn_id,"outcome":progress_code(outcome)}),
            ),
        };
        let data = serde_json::to_vec(&value).map_err(|_| PortError::Failed)?;
        if data.len() > MAX_CALLBACK_JSON {
            return Err(PortError::Failed);
        }
        let callback = self.cb.journal.ok_or(PortError::Unavailable)?;
        match callback(self.cb.context, kind, data.as_ptr(), data.len()) {
            0 => {
                if kind == 2 {
                    self.model_started_committed = self.model_started_committed.saturating_add(1);
                } else if kind == 3 {
                    self.model_settled_committed = self.model_settled_committed.saturating_add(1);
                }
                Ok(())
            }
            code => Err(Self::translate_status(code)),
        }
    }
    fn model(
        &mut self,
        messages: &[Message],
        identity: Identity<'_>,
    ) -> Result<ModelReply, PortError> {
        let messages=messages.iter().map(|item|json!({
            "role":match item.role {Role::User=>"user",Role::Assistant=>"assistant",Role::Tool=>"tool"},
            "content":item.content,"call_id":item.call_id,"tool_name":item.tool_name,
        })).collect::<Vec<_>>();
        let callback = self.cb.model.ok_or(PortError::Unavailable)?;
        let request = json!({"v":1,"identity":{
            "actor":identity.actor,"site":identity.site,
            "account":identity.account,"model":identity.model
        },"messages":messages});
        parse_reply(self.exchange(callback, request)?)
    }
    fn registration(&self, name: &str) -> Option<ToolRegistration<'_>> {
        let binding = self.bindings.iter().find(|item| item.name == name)?;
        let callback = self.cb.tool_authorize?;
        let request = json!({"v":1,"name":binding.name,
            "source_id":binding.source_id,"source_epoch":binding.epoch,
            "source_kind":source_code(binding.kind),"effect":effect_name(binding.effect),
            "actor":self.actor,"site":self.site});
        if !self.gate(callback, request) {
            return None;
        }
        let source = Source::new(binding.kind, &binding.source_id, binding.epoch).ok()?;
        let capability = Capability::new(&binding.name, source, binding.effect).ok()?;
        let grant = Grant::new(&self.actor, &self.site, capability).ok()?;
        Some(ToolRegistration { capability, grant })
    }
    fn validate_tool(&self, call: &ToolCall) -> Result<(), PortError> {
        let callback = self.cb.tool_validate.ok_or(PortError::Denied)?;
        self.gate(
            callback,
            json!({"v":1,"call":{
                "id":call.id,"name":call.name,"arguments":call.arguments
            }}),
        )
        .then_some(())
        .ok_or(PortError::Denied)
    }
    fn execute_tool(&mut self, call: &ToolCall) -> Result<ToolResult, PortError> {
        let callback = self.cb.tool_execute.ok_or(PortError::Unavailable)?;
        let value = self.exchange(
            callback,
            json!({"v":1,"call":{
                "id":call.id,"name":call.name,"arguments":call.arguments
            }}),
        )?;
        if !only_keys(&value, &["content", "is_error"]) {
            return Err(PortError::Failed);
        }
        let content = value
            .get("content")
            .and_then(Value::as_str)
            .ok_or(PortError::Failed)?;
        let is_error = value
            .get("is_error")
            .and_then(Value::as_bool)
            .ok_or(PortError::Failed)?;
        if content.len() > MAX_RESULT_BYTES {
            return Err(PortError::Failed);
        }
        Ok(ToolResult {
            content: content.to_string(),
            is_error,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hostile_model_json_is_rejected_before_agent_dispatch() {
        assert!(parse_reply(json!({"text":"ok","finish":"final","calls":[]})).is_ok());
        assert!(parse_reply(json!({"text":"x","finish":"final","calls":[],
                                 "grant":"shell"}))
        .is_err());
        assert!(parse_reply(json!({"text":"x","finish":"final"})).is_err());
        assert!(parse_reply(json!({"text":"","finish":"tools","calls":[{
            "id":"a","name":"echo","arguments":null
        }]}))
        .is_err());
    }
}
