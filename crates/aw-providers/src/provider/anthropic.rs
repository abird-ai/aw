//! Native Anthropic Messages wire adapter: bounded streaming blocks, exact
//! tool-use/result pairing and opaque same-model thinking continuation.
//! No durable/private history exports, independent Agent loop or hidden retries.
use crate::{config::Profile, Error, Message, Result};
use aw_agent_core::{
    completion::CompletionMeta,
    tool::{MAX_TOOL_ARGUMENT_BYTES, MAX_TOOL_CALLS_PER_ROUND},
    ModelReply, Role, ToolCall, ToolSpec,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

use super::decode::Decoded;

const MAX_PRIVATE_CONTEXT: usize = 512 * 1024;
const MAX_THINKING_BYTES: usize = 384 * 1024;
const MAX_TEXT_BYTES: usize = 512 * 1024;

fn str_field<'a>(obj: &'a Value, key: &str) -> Result<&'a str> {
    obj.get(key)
        .and_then(Value::as_str)
        .ok_or(Error::Protocol("missing/invalid Anthropic event field"))
}
fn usize_field(obj: &Value, key: &str) -> Result<usize> {
    obj.get(key)
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or(Error::Protocol(
            "missing/invalid Anthropic content block index",
        ))
}
// Anthropic reports uncached, cache-write and cache-read input tokens
// separately. Preserve the total without asserting unreported usage is zero.
fn input_usage(usage: &Value) -> Result<Option<u64>> {
    let Some(mut total) = usage.get("input_tokens").and_then(Value::as_u64) else {
        return Ok(None);
    };
    for key in ["cache_creation_input_tokens", "cache_read_input_tokens"] {
        if let Some(value) = usage.get(key) {
            let amount = value
                .as_u64()
                .ok_or(Error::Protocol("invalid Anthropic cache token usage"))?;
            total = total
                .checked_add(amount)
                .ok_or(Error::Limit("Anthropic token usage overflow"))?;
        }
    }
    Ok(Some(total))
}

fn append(out: &mut String, part: &str, cap: usize) -> Result<()> {
    if out.len().saturating_add(part.len()) > cap {
        return Err(Error::Limit(
            "Anthropic streaming block exceeds a bounded limit",
        ));
    }
    out.push_str(part);
    Ok(())
}

/// Opaque provider-private blocks are re-included ONLY within an active, same
/// model and destination context. The SDK does not journal/serialize this data.
/// Any mismatch fails before a new network request (no history fallback).
fn private_blocks(message: &Message, model: &str, profile: &Profile) -> Result<Vec<Value>> {
    let Some(raw) = message.provider_context.as_deref() else {
        return Ok(Vec::new());
    };
    if raw.len() > MAX_PRIVATE_CONTEXT {
        return Err(Error::Limit(
            "Anthropic private continuation exceeds 512 KiB",
        ));
    }
    let saved: Value = serde_json::from_str(raw)
        .map_err(|_| Error::Protocol("invalid Anthropic private continuation"))?;
    if saved["provider"] != "anthropic"
        || saved["model"] != model
        || saved["endpoint"] != profile.base_url
    {
        return Err(Error::Denied(
            "Anthropic continuation belongs to another destination/model",
        ));
    }
    let parts = saved["blocks"]
        .as_array()
        .ok_or(Error::Protocol("Anthropic continuation has no blocks"))?;
    if parts.len() > 64 {
        return Err(Error::Limit("too many Anthropic continuation blocks"));
    }
    let mut text = String::new();
    let mut tools = Vec::new();
    for part in parts {
        match part["type"].as_str() {
            Some("thinking") => {
                if str_field(part, "signature")?.is_empty() || part["thinking"].as_str().is_none() {
                    return Err(Error::Protocol("incomplete Anthropic thinking signature"));
                }
            }
            Some("redacted_thinking") => {
                if str_field(part, "data")?.is_empty() {
                    return Err(Error::Protocol("empty Anthropic redacted thinking"));
                }
            }
            Some("text") => append(&mut text, str_field(part, "text")?, MAX_TEXT_BYTES)?,
            Some("tool_use") => {
                let call = ToolCall {
                    id: str_field(part, "id")?.to_owned(),
                    name: str_field(part, "name")?.to_owned(),
                    arguments: part
                        .get("input")
                        .cloned()
                        .ok_or(Error::Protocol("Anthropic tool input missing"))?,
                };
                call.validate_envelope()?;
                tools.push(call);
            }
            _ => return Err(Error::Protocol("unexpected Anthropic continuation block")),
        }
    }
    if text != message.content || tools != message.tool_calls {
        return Err(Error::Protocol(
            "Anthropic continuation differs from public message",
        ));
    }
    Ok(parts.clone())
}

pub(super) fn body(
    profile: &Profile,
    messages: &[Message],
    model: &str,
    tools: &[ToolSpec],
) -> Result<Value> {
    let mut system = Vec::new();
    let mut requests = Vec::new();
    for message in messages {
        match message.role {
            Role::System => {
                if !message.content.is_empty() {
                    system.push(message.content.clone());
                }
            }
            Role::User => {
                requests.push(json!({
                    "role": "user",
                    "content": [{"type":"text", "text":message.content}]
                }));
            }
            Role::Tool => {
                let id = message
                    .tool_call_id
                    .as_deref()
                    .ok_or(Error::Protocol("Anthropic tool result missing tool_use_id"))?;
                // Consecutive tool_result items must be part of one user message,
                // not separate provider turns. Tool IDs are SDK validated.
                let result = json!({
                    "type":"tool_result",
                    "tool_use_id": id,
                    "content": message.content,
                    "is_error": message.content.starts_with("ERROR:")
                });
                if requests.last().is_some_and(|m: &Value| {
                    m["role"] == "user"
                        && m.get("content")
                            .and_then(Value::as_array)
                            .is_some_and(|c| c.first().is_some_and(|v| v["type"] == "tool_result"))
                }) {
                    requests.last_mut().unwrap()["content"]
                        .as_array_mut()
                        .unwrap()
                        .push(result);
                } else {
                    requests.push(json!({"role":"user","content":[result]}));
                }
            }
            Role::Assistant => {
                // An in-flight provider snapshot carries exact block order,
                // including public content. Never duplicate the tool calls.
                let mut blocks = private_blocks(message, model, profile)?;
                if blocks.is_empty() {
                    if !message.content.is_empty() {
                        blocks.push(json!({"type":"text","text":message.content}));
                    }
                    for call in &message.tool_calls {
                        call.validate_envelope()?;
                        blocks.push(json!({"type":"tool_use","id":call.id,
                            "name":call.name,"input":call.arguments}));
                    }
                }
                if blocks.is_empty() {
                    blocks.push(json!({"type":"text","text":""}));
                }
                requests.push(json!({"role":"assistant","content":blocks}));
            }
        }
    }
    let mut body = json!({
        "model": model,
        "max_tokens": profile.max_tokens.unwrap_or(4096),
        "stream": true,
        "messages": requests
    });
    if !system.is_empty() {
        body["system"] = json!(system.join("\n\n"));
    }
    if let Some(effort) = profile.reasoning_effort {
        if effort == crate::config::ReasoningEffort::Instant {
            // Some modern models require thinking and will reject this. Do not
            // silently reinterpret an explicit no-thinking request.
            body["thinking"] = json!({"type":"disabled"});
        } else {
            body["thinking"] = json!({"type":"adaptive"});
            body["output_config"] = json!({"effort":effort.api_value()});
        }
    }
    if let Some(temp) = profile.temperature {
        if profile.reasoning_effort.is_none()
            || profile.reasoning_effort == Some(crate::config::ReasoningEffort::Instant)
        {
            body["temperature"] = json!(temp);
        }
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(
            tools
                .iter()
                .map(|tool| {
                    json!({
                        "name":tool.name,
                        "description":tool.description,
                        "input_schema":tool.parameters
                    })
                })
                .collect(),
        );
    }
    Ok(body)
}

enum Block {
    Text {
        content: String,
        closed: bool,
    },
    Tool {
        id: String,
        name: String,
        arguments: String,
        input: Value,
        closed: bool,
    },
    Thinking {
        thinking: String,
        signature: String,
        closed: bool,
    },
    Redacted {
        data: String,
        closed: bool,
    },
}
impl Block {
    fn close(&mut self) -> Result<()> {
        match self {
            Block::Text { closed, .. }
            | Block::Tool { closed, .. }
            | Block::Thinking { closed, .. }
            | Block::Redacted { closed, .. } => {
                if *closed {
                    return Err(Error::Protocol("duplicate Anthropic content block close"));
                }
                *closed = true;
                Ok(())
            }
        }
    }
    fn is_closed(&self) -> bool {
        match self {
            Block::Text { closed, .. }
            | Block::Tool { closed, .. }
            | Block::Thinking { closed, .. }
            | Block::Redacted { closed, .. } => *closed,
        }
    }
}

pub(super) struct Decoder {
    pub(super) meta: CompletionMeta,
    blocks: BTreeMap<usize, Block>,
    allow_tools: bool,
    started: bool,
    delta_received: bool,
    ended: bool,
    model: String,
    endpoint: String,
}

impl Decoder {
    pub(super) fn new(allow_tools: bool, model: &str, profile: &Profile) -> Self {
        Self {
            meta: CompletionMeta::default(),
            blocks: BTreeMap::new(),
            allow_tools,
            started: false,
            delta_received: false,
            ended: false,
            model: model.to_owned(),
            endpoint: profile.base_url.clone(),
        }
    }

    pub(super) fn event(&mut self, data: &str) -> Result<Decoded> {
        let value: Value = serde_json::from_str(data)
            .map_err(|_| Error::Protocol("invalid Anthropic streaming JSON"))?;
        let kind = str_field(&value, "type")?;
        if self.ended {
            return Err(Error::Protocol("Anthropic events after message_stop"));
        }
        match kind {
            "ping" => Ok(Decoded::default()),
            "error" => Err(Error::Protocol("Anthropic stream error (details withheld)")),
            "message_start" => {
                if self.started {
                    return Err(Error::Protocol("duplicate Anthropic message_start"));
                }
                if value["message"]["type"] != "message" || value["message"]["role"] != "assistant"
                {
                    return Err(Error::Protocol("invalid Anthropic message_start"));
                }
                if value["message"]["model"] != self.model {
                    return Err(Error::Protocol(
                        "Anthropic response model differs from request",
                    ));
                }
                self.meta.usage.input_tokens = input_usage(&value["message"]["usage"])?;
                if let Some(output) = value["message"]["usage"]["output_tokens"].as_u64() {
                    self.meta.usage.output_tokens = Some(output);
                }
                self.started = true;
                Ok(Decoded::default())
            }
            "content_block_start" => {
                if !self.started || self.delta_received {
                    return Err(Error::Protocol(
                        "Anthropic content block outside active message",
                    ));
                }
                let index = usize_field(&value, "index")?;
                if index >= 64 || self.blocks.contains_key(&index) {
                    return Err(Error::Protocol(
                        "duplicate/oversized Anthropic content index",
                    ));
                }
                let block = &value["content_block"];
                let state = match str_field(block, "type")? {
                    "text" => Block::Text {
                        content: block
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        closed: false,
                    },
                    "tool_use" => {
                        if !self.allow_tools {
                            return Err(Error::Protocol(
                                "Anthropic emitted an ungranted tool call",
                            ));
                        }
                        if self
                            .blocks
                            .values()
                            .filter(|b| matches!(b, Block::Tool { .. }))
                            .count()
                            >= MAX_TOOL_CALLS_PER_ROUND
                        {
                            return Err(Error::Limit("too many Anthropic tool calls"));
                        }
                        let id = str_field(block, "id")?.to_owned();
                        let name = str_field(block, "name")?.to_owned();
                        let input = block.get("input").cloned().unwrap_or(json!({}));
                        if !input.is_object() {
                            return Err(Error::Protocol(
                                "Anthropic tool input is not a JSON object",
                            ));
                        }
                        if serde_json::to_vec(&input)
                            .map_err(|_| {
                                Error::Protocol("invalid Anthropic initial tool arguments")
                            })?
                            .len()
                            > MAX_TOOL_ARGUMENT_BYTES
                        {
                            return Err(Error::Limit("Anthropic initial tool input exceeds limit"));
                        }
                        Block::Tool {
                            id,
                            name,
                            input,
                            arguments: String::new(),
                            closed: false,
                        }
                    }
                    "thinking" => {
                        let thinking = block.get("thinking").and_then(Value::as_str).unwrap_or("");
                        let signature =
                            block.get("signature").and_then(Value::as_str).unwrap_or("");
                        if thinking.len() > MAX_THINKING_BYTES || signature.len() > 8192 {
                            return Err(Error::Limit("Anthropic initial thinking block too large"));
                        }
                        Block::Thinking {
                            thinking: thinking.to_owned(),
                            signature: signature.to_owned(),
                            closed: false,
                        }
                    }
                    "redacted_thinking" => {
                        let data = str_field(block, "data")?;
                        if data.len() > MAX_THINKING_BYTES {
                            return Err(Error::Limit("Anthropic redacted thinking too large"));
                        }
                        Block::Redacted {
                            data: data.to_owned(),
                            closed: false,
                        }
                    }
                    _ => return Err(Error::Protocol("unsupported Anthropic content block")),
                };
                self.blocks.insert(index, state);
                if block["type"] == "text" {
                    let initial = block.get("text").and_then(Value::as_str).unwrap_or("");
                    if initial.len() > MAX_TEXT_BYTES {
                        return Err(Error::Limit("Anthropic text block too large"));
                    }
                    return Ok(Decoded {
                        text: Some(initial.to_owned()),
                        done: false,
                    });
                }
                Ok(Decoded::default())
            }
            "content_block_delta" => {
                if !self.started || self.delta_received {
                    return Err(Error::Protocol(
                        "Anthropic content delta after message completion",
                    ));
                }
                let index = usize_field(&value, "index")?;
                let block = self
                    .blocks
                    .get_mut(&index)
                    .ok_or(Error::Protocol("Anthropic delta before block start"))?;
                if block.is_closed() {
                    return Err(Error::Protocol("Anthropic delta after block close"));
                }
                let delta = &value["delta"];
                match (block, str_field(delta, "type")?) {
                    (Block::Text { content, .. }, "text_delta") => {
                        let text = str_field(delta, "text")?;
                        append(content, text, MAX_TEXT_BYTES)?;
                        Ok(Decoded {
                            text: Some(text.into()),
                            done: false,
                        })
                    }
                    (Block::Tool { arguments, .. }, "input_json_delta") => {
                        append(
                            arguments,
                            str_field(delta, "partial_json")?,
                            MAX_TOOL_ARGUMENT_BYTES,
                        )?;
                        Ok(Decoded::default())
                    }
                    (Block::Thinking { thinking, .. }, "thinking_delta") => {
                        append(thinking, str_field(delta, "thinking")?, MAX_THINKING_BYTES)?;
                        Ok(Decoded::default())
                    }
                    (Block::Thinking { signature, .. }, "signature_delta") => {
                        append(signature, str_field(delta, "signature")?, 8192)?;
                        Ok(Decoded::default())
                    }
                    _ => Err(Error::Protocol(
                        "mismatched Anthropic streaming block delta",
                    )),
                }
            }
            "content_block_stop" => {
                let index = usize_field(&value, "index")?;
                let block = self
                    .blocks
                    .get_mut(&index)
                    .ok_or(Error::Protocol("Anthropic block_stop before start"))?;
                if let Block::Tool {
                    arguments, input, ..
                } = block
                {
                    if !arguments.is_empty() {
                        let data: Value = serde_json::from_str(arguments)
                            .map_err(|_| Error::Protocol("incomplete Anthropic tool input JSON"))?;
                        if !data.is_object() {
                            return Err(Error::Protocol("Anthropic tool input must be an object"));
                        }
                        *input = data;
                    }
                }
                block.close()?;
                Ok(Decoded::default())
            }
            "message_delta" => {
                if !self.started || self.delta_received {
                    return Err(Error::Protocol("duplicate Anthropic message_delta"));
                }
                if self.blocks.values().any(|block| !block.is_closed()) {
                    return Err(Error::Protocol(
                        "Anthropic message_delta with unclosed blocks",
                    ));
                }
                let stop = str_field(&value["delta"], "stop_reason")?;
                self.meta.finish_reason = match stop {
                    "end_turn" | "stop_sequence" => "stop",
                    "tool_use" => "tool_calls",
                    "max_tokens" => "length",
                    _ => return Err(Error::Protocol("unsupported Anthropic stop reason")),
                }
                .into();
                // message_delta is the authoritative final output-token count.
                // Missing means unknown, never infer zero from message_start.
                self.meta.usage.output_tokens = value["usage"]["output_tokens"].as_u64();
                if let Some(total) = input_usage(&value["usage"])? {
                    self.meta.usage.input_tokens = Some(total);
                }
                self.delta_received = true;
                Ok(Decoded::default())
            }
            "message_stop" => {
                if !self.delta_received {
                    return Err(Error::Protocol(
                        "Anthropic message_stop without final delta",
                    ));
                }
                self.ended = true;
                Ok(Decoded {
                    text: None,
                    done: true,
                })
            }
            // Future optional event types can be ignored; unknown content
            // *blocks* and missing end markers are never silently accepted.
            _ => Ok(Decoded::default()),
        }
    }

    pub(super) fn finish(self) -> Result<ModelReply> {
        if !self.ended || !self.delta_received {
            return Err(Error::Protocol("incomplete Anthropic stream"));
        }
        let mut calls = Vec::new();
        let mut blocks = Vec::new();
        let mut requires_continuation = false;
        for (_, block) in self.blocks {
            if !block.is_closed() {
                return Err(Error::Protocol("unfinished Anthropic content block"));
            }
            match block {
                Block::Tool {
                    id, name, input, ..
                } => {
                    if !input.is_object() {
                        return Err(Error::Protocol("incomplete Anthropic tool input"));
                    }
                    let call = ToolCall {
                        id,
                        name,
                        arguments: input,
                    };
                    call.validate_envelope()?;
                    blocks.push(json!({"type":"tool_use","id":call.id,
                        "name":call.name,"input":call.arguments}));
                    calls.push(call);
                    requires_continuation = true;
                }
                Block::Thinking {
                    thinking,
                    signature,
                    ..
                } => {
                    if signature.is_empty() {
                        return Err(Error::Protocol("Anthropic thinking signature missing"));
                    }
                    blocks.push(json!({"type":"thinking","thinking":thinking,
                        "signature":signature}));
                    requires_continuation = true;
                }
                Block::Redacted { data, .. } => {
                    if data.is_empty() {
                        return Err(Error::Protocol("empty Anthropic redacted thinking"));
                    }
                    blocks.push(json!({"type":"redacted_thinking","data":data}));
                    requires_continuation = true;
                }
                Block::Text { content, .. } => {
                    blocks.push(json!({"type":"text","text":content}));
                }
            }
        }
        if !calls.is_empty() && self.meta.finish_reason != "tool_calls" {
            return Err(Error::Protocol("Anthropic tools without tool_use stop"));
        }
        if calls.is_empty() && self.meta.finish_reason == "tool_calls" {
            return Err(Error::Protocol("Anthropic tool_use stop without tools"));
        }
        // Keep every provider block in its original order only when a tool or
        // private thinking continuation must be sent again in this live round.
        // None of this opaque envelope is persisted in ordinary Session JSONL.
        let provider_context = if !requires_continuation {
            None
        } else {
            let raw = json!({"provider":"anthropic","model":self.model,
                "endpoint":self.endpoint,"blocks":blocks})
            .to_string();
            if raw.len() > MAX_PRIVATE_CONTEXT {
                return Err(Error::Limit(
                    "Anthropic private continuation exceeds 512 KiB",
                ));
            }
            Some(raw)
        };
        Ok(ModelReply {
            meta: self.meta,
            tool_calls: calls,
            provider_context,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::{tool::ToolEffect, ToolCall};

    fn profile() -> Profile {
        crate::config::builtins().remove("anthropic").unwrap()
    }
    fn event(value: Value) -> String {
        value.to_string()
    }
    fn begin() -> String {
        event(json!({"type":"message_start","message":{
            "type":"message","role":"assistant","model":"claude-test",
            "usage":{"input_tokens":12,"output_tokens":0}
        }}))
    }
    fn end(reason: &str) -> [String; 2] {
        [
            event(
                json!({"type":"message_delta","delta":{"stop_reason":reason},
            "usage":{"output_tokens":7}}),
            ),
            event(json!({"type":"message_stop"})),
        ]
    }
    fn pass(decoder: &mut Decoder, events: &[String]) -> Result<String> {
        let mut text = String::new();
        for item in events {
            let delta = decoder.event(item)?;
            if let Some(part) = delta.text {
                text.push_str(&part);
            }
        }
        Ok(text)
    }
    fn tool() -> ToolSpec {
        ToolSpec {
            name: "read".into(),
            description: "Read a file".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            effect: ToolEffect::Read,
        }
    }
    #[test]
    fn anthropic_effort_and_thinking_are_distinct_native_settings() {
        let msg = Message::text(1, Role::User, "hello");
        for effort in [
            crate::config::ReasoningEffort::Low,
            crate::config::ReasoningEffort::Medium,
            crate::config::ReasoningEffort::High,
            crate::config::ReasoningEffort::Xhigh,
            crate::config::ReasoningEffort::Max,
        ] {
            let mut p = profile();
            p.reasoning_effort = Some(effort);
            let native = body(&p, std::slice::from_ref(&msg), "claude-test", &[]).unwrap();
            assert_eq!(native["thinking"]["type"], "adaptive");
            assert_eq!(native["output_config"]["effort"], effort.api_value());
        }
        let mut instant = profile();
        instant.reasoning_effort = Some(crate::config::ReasoningEffort::Instant);
        let native = body(&instant, std::slice::from_ref(&msg), "claude-test", &[]).unwrap();
        assert_eq!(native["thinking"]["type"], "disabled");
        assert!(native.get("output_config").is_none());
    }

    #[test]
    fn cache_usage_sums_without_forging_unknown_or_overflowed_tokens() {
        assert_eq!(
            input_usage(&json!({"input_tokens":3,"cache_creation_input_tokens":4,
            "cache_read_input_tokens":5}))
            .unwrap(),
            Some(12)
        );
        assert_eq!(
            input_usage(&json!({"cache_read_input_tokens":100})).unwrap(),
            None
        );
        assert!(input_usage(&json!({"input_tokens":1,"cache_read_input_tokens":-1})).is_err());
        assert!(input_usage(&json!({"input_tokens":u64::MAX,
            "cache_read_input_tokens":1}))
        .is_err());
    }

    #[test]
    fn streaming_text_and_usage_are_bounded_and_complete() {
        let profile = profile();
        let mut decoder = Decoder::new(false, "claude-test", &profile);
        let mut events = vec![
            begin(),
            event(
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"Hi "}}),
            ),
            event(
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"there"}}),
            ),
            event(json!({"type":"content_block_stop","index":0})),
        ];
        events.extend(end("end_turn"));
        assert_eq!(pass(&mut decoder, &events).unwrap(), "Hi there");
        let result = decoder.finish().unwrap();
        assert_eq!(result.meta.finish_reason, "stop");
        assert_eq!(result.meta.usage.input_tokens, Some(12));
        assert_eq!(result.meta.usage.output_tokens, Some(7));
        assert!(result.tool_calls.is_empty());
        assert!(result.provider_context.is_none());
    }
    #[test]
    fn partial_json_tools_and_signed_thinking_continue_exactly_for_same_model() {
        let profile = profile();
        let mut decoder = Decoder::new(true, "claude-test", &profile);
        let events = vec![
            begin(),
            event(
                json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
            ),
            event(
                json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"private thoughts"}}),
            ),
            event(
                json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"signed-token"}}),
            ),
            event(json!({"type":"content_block_stop","index":0})),
            event(
                json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_123","name":"read","input":{}}}),
            ),
            event(
                json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}),
            ),
            event(
                json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"a.txt\"}"}}),
            ),
            event(json!({"type":"content_block_stop","index":1})),
            end("tool_use")[0].clone(),
            end("tool_use")[1].clone(),
        ];
        assert!(pass(&mut decoder, &events).unwrap().is_empty());
        let result = decoder.finish().unwrap();
        assert_eq!(result.meta.finish_reason, "tool_calls");
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].id, "toolu_123");
        assert_eq!(result.tool_calls[0].arguments, json!({"path":"a.txt"}));
        let mut assistant = Message::text(1, Role::Assistant, "");
        assistant.tool_calls = result.tool_calls;
        assistant.provider_context = result.provider_context;
        let mut answer = Message::text(2, Role::Tool, "the contents");
        answer.tool_call_id = Some("toolu_123".into());
        let built = body(
            &profile,
            &[assistant.clone(), answer.clone()],
            "claude-test",
            &[tool()],
        )
        .unwrap();
        assert_eq!(built["tools"][0]["input_schema"], tool().parameters);
        assert_eq!(
            built["messages"][0]["content"][0]["signature"],
            "signed-token"
        );
        assert_eq!(
            built["messages"][0]["content"][0]["thinking"],
            "private thoughts"
        );
        assert_eq!(built["messages"][0]["content"][1]["type"], "tool_use");
        assert_eq!(
            built["messages"][1]["content"][0]["tool_use_id"],
            "toolu_123"
        );
        assert!(serde_json::to_string(&assistant)
            .unwrap()
            .find("signed-token")
            .is_none());
        assert!(body(
            &profile,
            &[assistant.clone(), answer.clone()],
            "different-model",
            &[]
        )
        .is_err());
        let mut different_destination = profile.clone();
        different_destination.base_url = "https://gateway.example/v1".into();
        assert!(body(
            &different_destination,
            &[assistant, answer],
            "claude-test",
            &[]
        )
        .is_err());
    }
    #[test]
    fn tool_result_pairing_is_native_anthropic_user_content() {
        let profile = profile();
        let mut assistant = Message::text(1, Role::Assistant, "I'll check");
        assistant.tool_calls = vec![
            ToolCall {
                id: "toolu_a".into(),
                name: "read".into(),
                arguments: json!({"path":"x"}),
            },
            ToolCall {
                id: "toolu_b".into(),
                name: "read".into(),
                arguments: json!({"path":"y"}),
            },
        ];
        let mut a = Message::text(2, Role::Tool, "first");
        a.tool_call_id = Some("toolu_a".into());
        let mut b = Message::text(3, Role::Tool, "ERROR: failed");
        b.tool_call_id = Some("toolu_b".into());
        let payload = body(
            &profile,
            &[
                Message::text(0, Role::System, "system rules"),
                Message::text(0, Role::User, "check files"),
                assistant,
                a,
                b,
            ],
            "claude-test",
            &[tool()],
        )
        .unwrap();
        assert_eq!(payload["system"], "system rules");
        assert_eq!(payload["max_tokens"], 4096);
        assert_eq!(payload["messages"].as_array().unwrap().len(), 3);
        assert_eq!(payload["messages"][1]["content"][0]["type"], "text");
        assert_eq!(payload["messages"][1]["content"][1]["type"], "tool_use");
        assert_eq!(
            payload["messages"][2]["content"].as_array().unwrap().len(),
            2
        );
        assert_eq!(payload["messages"][2]["content"][1]["is_error"], true);
    }
    #[test]
    fn interleaved_thinking_and_multiple_tools_preserve_exact_provider_block_order() {
        let profile = profile();
        let mut stream = Decoder::new(true, "claude-test", &profile);
        let packets = vec![
            begin(),
            event(json!({"type":"content_block_start","index":0,
                "content_block":{"type":"text","text":"First "}})),
            event(json!({"type":"content_block_stop","index":0})),
            event(json!({"type":"content_block_start","index":1,
                "content_block":{"type":"tool_use","id":"toolu_a","name":"read","input":{"path":"a"}}})),
            event(json!({"type":"content_block_stop","index":1})),
            event(json!({"type":"content_block_start","index":2,
                "content_block":{"type":"thinking","thinking":"","signature":""}})),
            event(json!({"type":"content_block_delta","index":2,
                "delta":{"type":"signature_delta","signature":"secret-signature"}})),
            event(json!({"type":"content_block_stop","index":2})),
            event(json!({"type":"content_block_start","index":3,
                "content_block":{"type":"text","text":"next "}})),
            event(json!({"type":"content_block_stop","index":3})),
            event(json!({"type":"content_block_start","index":4,
                "content_block":{"type":"tool_use","id":"toolu_b","name":"read","input":{"path":"b"}}})),
            event(json!({"type":"content_block_stop","index":4})),
            end("tool_use")[0].clone(),
            end("tool_use")[1].clone(),
        ];
        let text = pass(&mut stream, &packets).unwrap();
        assert_eq!(text, "First next ");
        let result = stream.finish().unwrap();
        let mut assistant = Message::text(1, Role::Assistant, text);
        assistant.tool_calls = result.tool_calls;
        assistant.provider_context = result.provider_context;
        let payload = body(&profile, &[assistant.clone()], "claude-test", &[tool()]).unwrap();
        let blocks = payload["messages"][0]["content"].as_array().unwrap();
        assert_eq!(
            blocks
                .iter()
                .map(|b| b["type"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["text", "tool_use", "thinking", "text", "tool_use"]
        );
        assert_eq!(blocks[2]["signature"], "secret-signature");
        let mut forged = assistant.clone();
        forged.content.push_str("tamper");
        assert!(body(&profile, &[forged], "claude-test", &[]).is_err());
        let mut forged_tool = assistant;
        forged_tool.tool_calls[0].arguments = json!({"path":"elsewhere"});
        assert!(body(&profile, &[forged_tool], "claude-test", &[]).is_err());
    }

    #[test]
    fn malformed_partial_unsafe_or_unsigned_stream_cannot_dispatch_tools() {
        let profile = profile();
        let mut no_tools = Decoder::new(false, "claude-test", &profile);
        no_tools.event(&begin()).unwrap();
        assert!(no_tools
            .event(&event(json!({"type":"content_block_start","index":0,
            "content_block":{"type":"tool_use","id":"1","name":"read","input":{}}})))
            .is_err());
        let mut incomplete = Decoder::new(true, "claude-test", &profile);
        incomplete.event(&begin()).unwrap();
        incomplete
            .event(&event(json!({"type":"content_block_start","index":0,
            "content_block":{"type":"tool_use","id":"toolu_1","name":"read","input":{}}})))
            .unwrap();
        incomplete
            .event(&event(json!({"type":"content_block_delta","index":0,
            "delta":{"type":"input_json_delta","partial_json":"{bad"}})))
            .unwrap();
        assert!(incomplete
            .event(&event(json!({"type":"content_block_stop","index":0})))
            .is_err());
        let mut truncated = Decoder::new(true, "claude-test", &profile);
        truncated.event(&begin()).unwrap();
        assert!(truncated.finish().is_err());
        let mut unsigned = Decoder::new(true, "claude-test", &profile);
        let mut events = vec![
            begin(),
            event(json!({"type":"content_block_start","index":0,
            "content_block":{"type":"thinking","thinking":"","signature":""}})),
            event(json!({"type":"content_block_stop","index":0})),
        ];
        events.extend(end("end_turn"));
        pass(&mut unsigned, &events).unwrap();
        assert!(unsigned.finish().is_err());
        let mut wrong_model = Decoder::new(false, "claude-test", &profile);
        assert!(wrong_model
            .event(&event(json!({"type":"message_start","message":{
            "type":"message","role":"assistant","model":"wrong"}})))
            .is_err());
    }
}
