//! Native Gemini GenerateContent (stateless, streamed SSE) adapter.
//! Provider-private per-part thought signatures are preserved in memory only,
//! scoped to the chosen provider/model/endpoint; they are not journaled.
use super::decode::Decoded;
use crate::{
    config::{Profile, ReasoningEffort},
    Error, Message, Result,
};
use aw_agent_core::{
    completion::CompletionMeta, tool::MAX_TOOL_CALLS_PER_ROUND, ModelReply, Role, ToolCall,
    ToolSpec,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_PARTS: usize = 64;
const MAX_TEXT: usize = 512 * 1024;
const MAX_PRIVATE: usize = 512 * 1024;
const MAX_SIGNATURE: usize = 128 * 1024;
const GENERATED_ID_PREFIX: &str = "aw_gemini_legacy_";
static NEXT_LEGACY_ID: AtomicU64 = AtomicU64::new(1);

fn private_parts(message: &Message, model: &str, profile: &Profile) -> Result<Option<Vec<Value>>> {
    let Some(raw) = message.provider_context.as_deref() else {
        return Ok(None);
    };
    if raw.len() > MAX_PRIVATE {
        return Err(Error::Limit("Gemini private continuation exceeds 512 KiB"));
    }
    let saved: Value = serde_json::from_str(raw)
        .map_err(|_| Error::Protocol("invalid Gemini private continuation"))?;
    if saved["provider"] != "gemini"
        || saved["model"] != model
        || saved["endpoint"] != profile.base_url
    {
        return Err(Error::Denied(
            "Gemini continuation belongs to another provider/model/destination",
        ));
    }
    let parts = saved["parts"]
        .as_array()
        .ok_or(Error::Protocol("Gemini private continuation missing parts"))?;
    let ids = saved["call_ids"].as_array().ok_or(Error::Protocol(
        "Gemini private continuation missing tool IDs",
    ))?;
    if parts.is_empty() || parts.len() > MAX_PARTS || ids.len() > MAX_TOOL_CALLS_PER_ROUND {
        return Err(Error::Limit(
            "Gemini private continuation part count invalid",
        ));
    }
    let mut text = String::new();
    let mut calls = Vec::new();
    let mut index = 0usize;
    for part in parts {
        validate_part(part)?;
        if let Some(chunk) = part.get("text").and_then(Value::as_str) {
            if !part["thought"].as_bool().unwrap_or(false) {
                append(&mut text, chunk, MAX_TEXT)?;
            }
        } else if let Some(call) = part.get("functionCall") {
            let id = ids
                .get(index)
                .and_then(Value::as_str)
                .ok_or(Error::Protocol("Gemini saved tool index mismatch"))?;
            let c = ToolCall {
                id: id.to_owned(),
                name: read_str(call, "name")?.to_owned(),
                arguments: call.get("args").cloned().unwrap_or_else(|| json!({})),
            };
            c.validate_envelope()?;
            // If Gemini supplied an id, it must still agree with the saved
            // call mapping. Legacy-generated IDs never change the provider part.
            if let Some(provider_id) = call.get("id").and_then(Value::as_str) {
                if provider_id != id {
                    return Err(Error::Protocol("Gemini function ID mismatched"));
                }
            } else if !id.starts_with(GENERATED_ID_PREFIX) {
                return Err(Error::Protocol("Gemini unsigned legacy call ID invalid"));
            }
            calls.push(c);
            index += 1;
        }
    }
    if calls.len() != ids.len() || text != message.content || calls != message.tool_calls {
        return Err(Error::Protocol(
            "Gemini continuation differs from public assistant content",
        ));
    }
    Ok(Some(parts.clone()))
}

fn read_str<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(Error::Protocol("Gemini streaming field missing or invalid"))
}

fn append(target: &mut String, content: &str, limit: usize) -> Result<()> {
    if target.len().saturating_add(content.len()) > limit {
        return Err(Error::Limit(
            "Gemini streamed content exceeds bounded limit",
        ));
    }
    target.push_str(content);
    Ok(())
}

fn validate_part(part: &Value) -> Result<()> {
    let object = part
        .as_object()
        .ok_or(Error::Protocol("Gemini content part must be an object"))?;
    if object.get("thoughtSignature").is_some() {
        let signature = read_str(part, "thoughtSignature")?;
        if signature.is_empty() || signature.len() > MAX_SIGNATURE {
            return Err(Error::Limit(
                "Gemini thought signature is empty or oversized",
            ));
        }
    }
    if object.contains_key("thought") && !part["thought"].is_boolean() {
        return Err(Error::Protocol("Gemini thought marker is invalid"));
    }
    match (
        object.get("text").and_then(Value::as_str),
        object.get("functionCall"),
    ) {
        (Some(text), None) => {
            if text.len() > MAX_TEXT {
                return Err(Error::Limit("Gemini text part too large"));
            }
        }
        (None, Some(call)) => {
            let name = read_str(call, "name")?;
            let args = call.get("args").cloned().unwrap_or_else(|| json!({}));
            let placeholder = ToolCall {
                id: call
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("missing_legacy_id")
                    .to_owned(),
                name: name.to_owned(),
                arguments: args,
            };
            placeholder.validate_envelope()?;
        }
        _ => {
            return Err(Error::Protocol(
                "Gemini returned unsupported or ambiguous content part",
            ))
        }
    }
    Ok(())
}

pub(super) fn body(
    profile: &Profile,
    messages: &[Message],
    model: &str,
    tools: &[ToolSpec],
) -> Result<Value> {
    let mut instructions = Vec::new();
    let mut contents: Vec<Value> = Vec::new();
    let mut pending = BTreeMap::<String, String>::new();
    for message in messages {
        match message.role {
            Role::System => {
                if !message.content.is_empty() {
                    instructions.push(message.content.clone());
                }
            }
            Role::User => {
                if !pending.is_empty() {
                    return Err(Error::Protocol(
                        "Gemini tool results missing before user turn",
                    ));
                }
                contents.push(json!({"role":"user","parts":[{"text":message.content}]}));
            }
            Role::Assistant => {
                if !pending.is_empty() {
                    return Err(Error::Protocol(
                        "Gemini assistant preceded incomplete tool batch",
                    ));
                }
                let mut parts = if let Some(saved) = private_parts(message, model, profile)? {
                    saved
                } else {
                    let mut constructed = Vec::new();
                    if !message.content.is_empty() {
                        constructed.push(json!({"text":message.content}));
                    }
                    for call in &message.tool_calls {
                        call.validate_envelope()?;
                        let mut function = json!({"name":call.name,"args":call.arguments});
                        if !call.id.starts_with(GENERATED_ID_PREFIX) {
                            function["id"] = json!(call.id);
                        }
                        constructed.push(json!({"functionCall":function}));
                    }
                    constructed
                };
                if parts.is_empty() {
                    parts.push(json!({"text":""}));
                }
                for call in &message.tool_calls {
                    call.validate_envelope()?;
                    if pending.insert(call.id.clone(), call.name.clone()).is_some() {
                        return Err(Error::Protocol(
                            "Gemini duplicate tool-call ID in transcript",
                        ));
                    }
                }
                contents.push(json!({"role":"model","parts":parts}));
            }
            Role::Tool => {
                let id = message
                    .tool_call_id
                    .as_deref()
                    .ok_or(Error::Protocol("Gemini tool result missing call ID"))?;
                let name = pending.remove(id).ok_or(Error::Protocol(
                    "Gemini tool result does not match preceding call",
                ))?;
                if message.name.as_deref().is_some_and(|actual| actual != name) {
                    return Err(Error::Protocol(
                        "Gemini tool name differs from original call",
                    ));
                }
                let response = if let Some(error) = message.content.strip_prefix("ERROR: ") {
                    json!({"error":error})
                } else {
                    json!({"result":message.content})
                };
                let mut function = json!({"name":name,"response":response});
                if !id.starts_with(GENERATED_ID_PREFIX) {
                    function["id"] = json!(id);
                }
                let part = json!({"functionResponse":function});
                if let Some(last) = contents.last_mut() {
                    let group = last["role"] == "user"
                        && last["parts"].as_array().is_some_and(|parts| {
                            parts
                                .first()
                                .is_some_and(|p| p.get("functionResponse").is_some())
                        });
                    if group {
                        last["parts"].as_array_mut().unwrap().push(part);
                        continue;
                    }
                }
                contents.push(json!({"role":"user","parts":[part]}));
            }
        }
    }
    if !pending.is_empty() {
        return Err(Error::Protocol(
            "Gemini cannot request next model step with pending tool results",
        ));
    }
    let mut body = json!({"contents":contents});
    if !instructions.is_empty() {
        body["systemInstruction"] = json!({"parts":[{"text":instructions.join("\n\n")}]});
    }
    if !tools.is_empty() {
        body["tools"] = json!([{"functionDeclarations":tools.iter().map(|tool| {
            // Keep the full common JSON Schema for Gemini's dedicated field.
            // Host-side Tool.validate always enforces the original restrictions.
            json!({"name":tool.name,"description":tool.description,"parametersJsonSchema":tool.parameters})
        }).collect::<Vec<_>>()}]);
    }
    let mut generation = serde_json::Map::new();
    if let Some(max_tokens) = profile.max_tokens {
        generation.insert("maxOutputTokens".into(), json!(max_tokens));
    }
    if let Some(effort) = profile.reasoning_effort {
        let short_name = model.strip_prefix("models/").unwrap_or(model);
        let thinking = if short_name.starts_with("gemini-2.5-") {
            if effort == ReasoningEffort::Instant && !short_name.starts_with("gemini-2.5-pro") {
                json!({"thinkingBudget":0})
            } else {
                return Err(Error::Config(
                    "Gemini 2.5 requires model-specific thinking budgets; only instant on supported Flash/Lite is mapped".into(),
                ));
            }
        } else {
            match effort {
                ReasoningEffort::Low => json!({"thinkingLevel":"low"}),
                ReasoningEffort::Medium => json!({"thinkingLevel":"medium"}),
                ReasoningEffort::High => json!({"thinkingLevel":"high"}),
                ReasoningEffort::Instant => return Err(Error::Config(
                    "Gemini 3 cannot guarantee thinking disabled; choose low or let model decide"
                        .into(),
                )),
                ReasoningEffort::Xhigh | ReasoningEffort::Max => {
                    return Err(Error::Config(
                        "Gemini has no exact xhigh/max thinking level".into(),
                    ))
                }
            }
        };
        generation.insert("thinkingConfig".into(), thinking);
    }
    if let Some(temp) = profile.temperature {
        generation.insert("temperature".into(), json!(temp));
    }
    if !generation.is_empty() {
        body["generationConfig"] = Value::Object(generation);
    }
    Ok(body)
}

pub(super) struct Decoder {
    model: String,
    endpoint: String,
    allow_tools: bool,
    parts: Vec<Value>,
    calls: Vec<ToolCall>,
    meta: CompletionMeta,
    seen_finish: bool,
    candidate_seen: bool,
    visible_bytes: usize,
}
impl Decoder {
    pub(super) fn new(allow_tools: bool, model: &str, profile: &Profile) -> Self {
        Self {
            model: model.to_owned(),
            endpoint: profile.base_url.clone(),
            allow_tools,
            parts: Vec::new(),
            calls: Vec::new(),
            meta: CompletionMeta::default(),
            seen_finish: false,
            candidate_seen: false,
            visible_bytes: 0,
        }
    }

    pub(super) fn event(&mut self, data: &str) -> Result<Decoded> {
        if data.trim().is_empty() {
            return Err(Error::Protocol("empty Gemini SSE data frame"));
        }
        let chunk: Value = serde_json::from_str(data)
            .map_err(|_| Error::Protocol("invalid Gemini stream JSON"))?;
        if chunk.get("error").is_some_and(|v| !v.is_null()) {
            return Err(Error::Protocol("Gemini stream error (details withheld)"));
        }
        if chunk["promptFeedback"]
            .get("blockReason")
            .and_then(Value::as_str)
            .is_some_and(|s| s != "BLOCK_REASON_UNSPECIFIED")
        {
            return Err(Error::Protocol("Gemini prompt blocked (details withheld)"));
        }
        if let Some(usage) = chunk.get("usageMetadata") {
            if let Some(count) = usage.get("promptTokenCount").and_then(Value::as_u64) {
                self.meta.usage.input_tokens = Some(count);
            }
            if let Some(count) = usage.get("candidatesTokenCount").and_then(Value::as_u64) {
                let thoughts = usage
                    .get("thoughtsTokenCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                self.meta.usage.output_tokens = Some(
                    count
                        .checked_add(thoughts)
                        .ok_or(Error::Limit("Gemini reported token usage overflow"))?,
                );
            }
        }
        let Some(candidates) = chunk.get("candidates") else {
            return Ok(Decoded::default());
        };
        let candidates = candidates
            .as_array()
            .ok_or(Error::Protocol("Gemini candidate list is invalid"))?;
        // Gemini can send usage-only bookkeeping after the final candidate.
        // Such frames never change a completed tool proposal or grant.
        if candidates.is_empty() && chunk.get("usageMetadata").is_some() {
            return Ok(Decoded::default());
        }
        if candidates.len() != 1 || candidates[0]["index"].as_u64().is_some_and(|i| i != 0) {
            return Err(Error::Protocol(
                "Gemini requires exactly one response candidate",
            ));
        }
        if self.seen_finish {
            return Err(Error::Protocol(
                "Gemini returned candidate after finishReason",
            ));
        }
        self.candidate_seen = true;
        let candidate = &candidates[0];
        if candidate["content"]
            .get("role")
            .and_then(Value::as_str)
            .is_some_and(|role| role != "model")
        {
            return Err(Error::Protocol("Gemini response content has invalid role"));
        }
        let mut text = String::new();
        if let Some(parts) = candidate["content"].get("parts") {
            let parts = parts
                .as_array()
                .ok_or(Error::Protocol("Gemini candidate parts invalid"))?;
            if parts.len().saturating_add(self.parts.len()) > MAX_PARTS {
                return Err(Error::Limit("Gemini content part count exceeded"));
            }
            for part in parts {
                validate_part(part)?;
                if let Some(call) = part.get("functionCall") {
                    if !self.allow_tools {
                        return Err(Error::Protocol(
                            "Gemini proposed tools while none were granted",
                        ));
                    }
                    if self.calls.len() >= MAX_TOOL_CALLS_PER_ROUND {
                        return Err(Error::Limit("too many Gemini tool calls"));
                    }
                    let id = if let Some(id) = call.get("id").and_then(Value::as_str) {
                        if id.starts_with(GENERATED_ID_PREFIX) {
                            return Err(Error::Protocol("Gemini returned reserved local call ID"));
                        }
                        id.to_owned()
                    } else {
                        if self
                            .model
                            .strip_prefix("models/")
                            .unwrap_or(&self.model)
                            .starts_with("gemini-3")
                        {
                            return Err(Error::Protocol(
                                "Gemini 3 call missing native function ID",
                            ));
                        }
                        format!(
                            "{GENERATED_ID_PREFIX}{}",
                            NEXT_LEGACY_ID.fetch_add(1, Ordering::Relaxed)
                        )
                    };
                    let proposal = ToolCall {
                        id,
                        name: read_str(call, "name")?.to_owned(),
                        arguments: call.get("args").cloned().unwrap_or_else(|| json!({})),
                    };
                    proposal.validate_envelope()?;
                    if self.calls.iter().any(|item| item.id == proposal.id) {
                        return Err(Error::Protocol("Gemini reused a tool-call ID"));
                    }
                    self.calls.push(proposal);
                } else if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                    if !part["thought"].as_bool().unwrap_or(false) {
                        if self.visible_bytes.saturating_add(part_text.len()) > MAX_TEXT {
                            return Err(Error::Limit("Gemini visible response exceeds 512 KiB"));
                        }
                        self.visible_bytes += part_text.len();
                        text.push_str(part_text);
                    }
                }
                self.parts.push(part.clone());
            }
        }
        if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
            if reason == "FINISH_REASON_UNSPECIFIED" {
                return Ok(Decoded {
                    text: if text.is_empty() { None } else { Some(text) },
                    done: false,
                });
            }
            self.meta.finish_reason = match reason {
                "STOP" => "stop",
                "MAX_TOKENS" => "length",
                "SAFETY" | "RECITATION" | "PROHIBITED_CONTENT" | "SPII" | "BLOCKLIST"
                | "IMAGE_SAFETY" => "content_filter",
                _ => return Err(Error::Protocol("unsupported Gemini finish reason")),
            }
            .into();
            self.seen_finish = true;
        }
        Ok(Decoded {
            text: if text.is_empty() { None } else { Some(text) },
            done: false,
        })
    }

    pub(super) fn finish(self) -> Result<ModelReply> {
        if !self.seen_finish || !self.candidate_seen {
            return Err(Error::Protocol(
                "Gemini stream ended without settled finishReason",
            ));
        }
        if !self.calls.is_empty() && self.meta.finish_reason != "stop" {
            return Err(Error::Protocol(
                "Gemini tool proposal was not a complete STOP response",
            ));
        }
        if !self.calls.is_empty()
            && self
                .model
                .strip_prefix("models/")
                .unwrap_or(&self.model)
                .starts_with("gemini-3")
        {
            let first_call = self
                .parts
                .iter()
                .find(|part| part.get("functionCall").is_some())
                .ok_or(Error::Protocol("Gemini function call has no signed part"))?;
            if first_call
                .get("thoughtSignature")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                return Err(Error::Protocol(
                    "Gemini 3 function call is missing required thoughtSignature",
                ));
            }
        }
        let mut private = None;
        if self.parts.iter().any(|part| {
            part.get("functionCall").is_some()
                || part.get("thoughtSignature").is_some()
                || part["thought"].as_bool().unwrap_or(false)
        }) {
            let envelope = json!({
                "provider":"gemini", "model":self.model, "endpoint":self.endpoint,
                "parts":self.parts,
                "call_ids":self.calls.iter().map(|call| call.id.clone()).collect::<Vec<_>>(),
            })
            .to_string();
            if envelope.len() > MAX_PRIVATE {
                return Err(Error::Limit("Gemini private continuation exceeds 512 KiB"));
            }
            private = Some(envelope);
        }
        Ok(ModelReply {
            meta: CompletionMeta {
                finish_reason: if !self.calls.is_empty() {
                    "tool_calls".into()
                } else {
                    self.meta.finish_reason
                },
                usage: self.meta.usage,
            },
            tool_calls: self.calls,
            provider_context: private,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProviderKind, TokenField};
    use aw_agent_core::tool::ToolEffect;

    fn profile() -> Profile {
        crate::config::builtins().remove("gemini").unwrap()
    }
    fn payload(v: Value) -> String {
        v.to_string()
    }
    fn read_tool() -> ToolSpec {
        ToolSpec {
            name: "read".into(),
            description: "bounded file reader".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            effect: ToolEffect::Read,
        }
    }
    fn read_turn(decoder: &mut Decoder, chunks: &[Value]) -> Result<String> {
        let mut visible = String::new();
        for chunk in chunks {
            if let Some(part) = decoder.event(&payload(chunk.clone()))?.text {
                visible.push_str(&part);
            }
        }
        Ok(visible)
    }
    fn call(id: Option<&str>, path: &str, signature: Option<&str>) -> Value {
        let mut part = json!({"functionCall":{"name":"read","args":{"path":path}}});
        if let Some(id) = id {
            part["functionCall"]["id"] = json!(id)
        }
        if let Some(signature) = signature {
            part["thoughtSignature"] = json!(signature);
        }
        part
    }
    #[test]
    fn native_root_endpoint_and_model_segments_cannot_escape_path() {
        let p = profile();
        assert_eq!(p.kind, ProviderKind::Gemini);
        assert_eq!(p.token_field, TokenField::MaxTokens);
        assert_eq!(
            p.endpoint().unwrap().as_str(),
            "https://generativelanguage.googleapis.com/v1beta/models"
        );
        assert_eq!(p.inference_endpoint("gemini-3.8-flash").unwrap().as_str(),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.8-flash:streamGenerateContent?alt=sse");
        assert_eq!(
            p.inference_endpoint("models/gemini-3.8-flash").unwrap(),
            p.inference_endpoint("gemini-3.8-flash").unwrap()
        );
        for bad in [
            "..",
            "../secret",
            "name?key=secret",
            "//evil",
            "name/extra",
            "",
            ":bad",
            "x#fragment",
        ] {
            assert!(p.inference_endpoint(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn native_request_tool_results_are_grouped_by_exact_provider_id_and_name() {
        let p = profile();
        let mut assistant = Message::text(2, Role::Assistant, "Checking");
        assistant.tool_calls = vec![
            ToolCall {
                id: "function-call-1".into(),
                name: "read".into(),
                arguments: json!({"path":"a"}),
            },
            ToolCall {
                id: "function-call-2".into(),
                name: "read".into(),
                arguments: json!({"path":"b"}),
            },
        ];
        let mut result_a = Message::text(3, Role::Tool, "contents-a");
        result_a.tool_call_id = Some("function-call-1".into());
        result_a.name = Some("read".into());
        let mut result_b = Message::text(4, Role::Tool, "ERROR: missing");
        result_b.tool_call_id = Some("function-call-2".into());
        result_b.name = Some("read".into());
        let data = body(
            &p,
            &[
                Message::text(0, Role::System, "system"),
                Message::text(1, Role::User, "read two"),
                assistant.clone(),
                result_a.clone(),
                result_b.clone(),
            ],
            "gemini-3.8-flash",
            &[read_tool()],
        )
        .unwrap();
        assert_eq!(data["systemInstruction"]["parts"][0]["text"], "system");
        assert_eq!(data["contents"].as_array().unwrap().len(), 3);
        assert_eq!(data["contents"][1]["role"], "model");
        assert_eq!(
            data["contents"][1]["parts"][1]["functionCall"]["id"],
            "function-call-1"
        );
        assert_eq!(data["contents"][2]["role"], "user");
        let results = data["contents"][2]["parts"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["functionResponse"]["id"], "function-call-1");
        assert_eq!(results[1]["functionResponse"]["id"], "function-call-2");
        assert_eq!(
            results[1]["functionResponse"]["response"]["error"],
            "missing"
        );
        assert_eq!(
            data["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"],
            read_tool().parameters
        );
        let mut wrong = result_a.clone();
        wrong.tool_call_id = Some("other".into());
        assert!(body(&p, &[assistant.clone(), wrong], "gemini-3.8-flash", &[]).is_err());
        let mut wrong = result_a;
        wrong.name = Some("write".into());
        assert!(body(&p, &[assistant, wrong], "gemini-3.8-flash", &[]).is_err());
    }
    #[test]
    fn signed_function_call_and_visible_text_roundtrip_without_private_serialization() {
        let p = profile();
        let mut decoder = Decoder::new(true, "gemini-3.8-flash", &p);
        let chunks = [
            json!({"candidates":[{"index":0,"content":{"role":"model",
                "parts":[{"text":"Checking "},{"text":"opaque thinking","thought":true,"thoughtSignature":"private-thought"}]}}]}),
            json!({"candidates":[{"index":0,"content":{"role":"model",
                "parts":[call(Some("function-call-id"),"a.txt",Some("signed-call"))]},
                "finishReason":"STOP"}],
                "usageMetadata":{"promptTokenCount":20,"candidatesTokenCount":4,"thoughtsTokenCount":9}}),
        ];
        let visible = read_turn(&mut decoder, &chunks).unwrap();
        assert_eq!(visible, "Checking ");
        let reply = decoder.finish().unwrap();
        assert_eq!(reply.meta.finish_reason, "tool_calls");
        assert_eq!(reply.meta.usage.input_tokens, Some(20));
        assert_eq!(reply.meta.usage.output_tokens, Some(13));
        assert_eq!(reply.tool_calls.len(), 1);
        assert_eq!(reply.tool_calls[0].id, "function-call-id");
        let mut assistant = Message::text(5, Role::Assistant, visible);
        assistant.tool_calls = reply.tool_calls;
        assistant.provider_context = reply.provider_context;
        assert!(serde_json::to_string(&assistant)
            .unwrap()
            .find("signed-call")
            .is_none());
        let mut output = Message::text(6, Role::Tool, "ok");
        output.name = Some("read".into());
        output.tool_call_id = Some("function-call-id".into());
        let rebuilt = body(
            &p,
            &[assistant.clone(), output],
            "gemini-3.8-flash",
            &[read_tool()],
        )
        .unwrap();
        let parts = rebuilt["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts[0]["text"], "Checking ");
        assert_eq!(parts[1]["thoughtSignature"], "private-thought");
        assert_eq!(parts[2]["thoughtSignature"], "signed-call");
        assert_eq!(parts[2]["functionCall"]["id"], "function-call-id");
        let mut forged = assistant.clone();
        forged.content = "tampered".into();
        assert!(body(&p, &[forged], "gemini-3.8-flash", &[]).is_err());
        let mut forged = assistant.clone();
        forged.tool_calls[0].arguments = json!({"path":"other"});
        assert!(body(&p, &[forged], "gemini-3.8-flash", &[]).is_err());
        assert!(body(&p, &[assistant.clone()], "gemini-2.5-flash", &[]).is_err());
        let mut other = p.clone();
        other.base_url = "https://another.googleapis.com/v1beta".into();
        assert!(body(&other, &[assistant], "gemini-3.8-flash", &[]).is_err());
    }
    #[test]
    fn parallel_calls_keep_first_only_signature_and_sequential_steps_keep_both() {
        let p = profile();
        let mut first = Decoder::new(true, "gemini-3.8-flash", &p);
        let steps = [json!({"candidates":[{"index":0,"content":{"parts":[
            call(Some("fc-1"),"a",Some("signed-1")),
            call(Some("fc-2"),"b",None)]},"finishReason":"STOP"}]})];
        read_turn(&mut first, &steps).unwrap();
        let first = first.finish().unwrap();
        assert_eq!(first.tool_calls.len(), 2);
        let mut assistant = Message::text(1, Role::Assistant, "");
        assistant.provider_context = first.provider_context;
        assistant.tool_calls = first.tool_calls;
        let mut a = Message::text(2, Role::Tool, "A");
        a.name = Some("read".into());
        a.tool_call_id = Some("fc-1".into());
        let mut b = Message::text(3, Role::Tool, "B");
        b.name = Some("read".into());
        b.tool_call_id = Some("fc-2".into());
        let first_body = body(
            &p,
            &[assistant.clone(), a.clone(), b.clone()],
            "gemini-3.8-flash",
            &[read_tool()],
        )
        .unwrap();
        assert_eq!(
            first_body["contents"][0]["parts"][0]["thoughtSignature"],
            "signed-1"
        );
        assert!(first_body["contents"][0]["parts"][1]
            .get("thoughtSignature")
            .is_none());
        assert_eq!(
            first_body["contents"][1]["parts"].as_array().unwrap().len(),
            2
        );
        let mut second = Decoder::new(true, "gemini-3.8-flash", &p);
        let steps = [json!({"candidates":[{"index":0,"content":{"parts":[
            call(Some("fc-3"),"c",Some("signed-2"))]},"finishReason":"STOP"}]})];
        read_turn(&mut second, &steps).unwrap();
        let second = second.finish().unwrap();
        let mut assistant2 = Message::text(4, Role::Assistant, "");
        assistant2.provider_context = second.provider_context;
        assistant2.tool_calls = second.tool_calls;
        let mut c = Message::text(5, Role::Tool, "C");
        c.name = Some("read".into());
        c.tool_call_id = Some("fc-3".into());
        let body = body(
            &p,
            &[assistant, a, b, assistant2, c],
            "gemini-3.8-flash",
            &[read_tool()],
        )
        .unwrap();
        assert_eq!(
            body["contents"][0]["parts"][0]["thoughtSignature"],
            "signed-1"
        );
        assert_eq!(
            body["contents"][2]["parts"][0]["thoughtSignature"],
            "signed-2"
        );
        assert_eq!(
            body["contents"][3]["parts"][0]["functionResponse"]["id"],
            "fc-3"
        );
    }
    #[test]
    fn legacy_missing_provider_call_id_has_stable_local_pairing_without_forged_remote_id() {
        let p = profile();
        let mut decoder = Decoder::new(true, "gemini-2.5-flash", &p);
        let values = [json!({"candidates":[{"content":{"parts":[
            call(None,"a",None)]},"finishReason":"STOP"}]})];
        read_turn(&mut decoder, &values).unwrap();
        let reply = decoder.finish().unwrap();
        assert_eq!(reply.tool_calls.len(), 1);
        assert!(reply.tool_calls[0].id.starts_with(GENERATED_ID_PREFIX));
        let mut model = Message::text(1, Role::Assistant, "");
        model.provider_context = reply.provider_context;
        model.tool_calls = reply.tool_calls.clone();
        let mut output = Message::text(2, Role::Tool, "A");
        output.name = Some("read".into());
        output.tool_call_id = Some(reply.tool_calls[0].id.clone());
        let request = body(&p, &[model, output], "gemini-2.5-flash", &[]).unwrap();
        assert!(request["contents"][0]["parts"][0]["functionCall"]
            .get("id")
            .is_none());
        assert!(request["contents"][1]["parts"][0]["functionResponse"]
            .get("id")
            .is_none());
    }
    #[test]
    fn incomplete_or_unsigned_or_ungranted_tool_streams_fail_closed() {
        let p = profile();
        let mut incomplete = Decoder::new(true, "gemini-3.8-flash", &p);
        incomplete
            .event(&payload(json!({"candidates":[{"content":{"parts":[
            call(Some("id1"),"a",Some("signature"))]}}]})))
            .unwrap();
        assert!(incomplete.finish().is_err());
        let mut unsigned = Decoder::new(true, "gemini-3.8-flash", &p);
        unsigned
            .event(&payload(json!({"candidates":[{"content":{"parts":[
            call(Some("id1"),"a",None)]},"finishReason":"STOP"}]})))
            .unwrap();
        assert!(unsigned.finish().is_err());
        let mut missing_id = Decoder::new(true, "gemini-3.8-flash", &p);
        assert!(missing_id
            .event(&payload(json!({"candidates":[{"content":{"parts":[
            call(None,"a",Some("signature"))]},"finishReason":"STOP"}]})))
            .is_err());
        let mut reserved = Decoder::new(true, "gemini-3.8-flash", &p);
        assert!(reserved
            .event(&payload(json!({"candidates":[{"content":{"parts":[
            call(Some("aw_gemini_legacy_123"),"a",Some("signature"))]},"finishReason":"STOP"}]})))
            .is_err());
        let mut denied = Decoder::new(false, "gemini-3.8-flash", &p);
        assert!(denied
            .event(&payload(json!({"candidates":[{"content":{"parts":[
            call(Some("id1"),"a",Some("signature"))]},"finishReason":"STOP"}]})))
            .is_err());
        let mut too_long = Decoder::new(true, "gemini-3.8-flash", &p);
        assert!(too_long
            .event(&payload(json!({"candidates":[{"content":{"parts":[
            {"text":"x".repeat(MAX_TEXT+1)}]},"finishReason":"STOP"}]})))
            .is_err());
        let mut blocked = Decoder::new(true, "gemini-3.8-flash", &p);
        assert!(blocked
            .event(&payload(json!({"promptFeedback":{"blockReason":"SAFETY"}})))
            .is_err());
        let mut dup = Decoder::new(true, "gemini-3.8-flash", &p);
        assert!(dup
            .event(&payload(json!({"candidates":[{"content":{"parts":[
            call(Some("dup"),"a",Some("sig")),call(Some("dup"),"b",None)]},
            "finishReason":"STOP"}]})))
            .is_err());
        let mut shortened = Decoder::new(true, "gemini-3.8-flash", &p);
        shortened
            .event(&payload(json!({"candidates":[{"content":{"parts":[
            call(Some("id1"),"a",Some("sig"))]},"finishReason":"MAX_TOKENS"}]})))
            .unwrap();
        assert!(shortened.finish().is_err());
        let mut more = Decoder::new(false, "gemini-3.8-flash", &p);
        more.event(&payload(
            json!({"candidates":[{"content":{"parts":[{"text":"okay"}]},
            "finishReason":"STOP"}]}),
        ))
        .unwrap();
        assert!(more.event(&payload(json!({"candidates":[{"content":{"parts":[{"text":"late"}]},"finishReason":"STOP"}]}))).is_err());
    }
    #[test]
    fn usage_only_frame_after_stop_updates_reported_counters_without_extra_output() {
        let p = profile();
        let mut decoder = Decoder::new(false, "gemini-3.8-flash", &p);
        let chunks = [
            json!({"candidates":[{"content":{"parts":[{"text":"Hello"}]},
                   "finishReason":"FINISH_REASON_UNSPECIFIED"}]}),
            json!({"candidates":[{"finishReason":"STOP"}]}),
            json!({"candidates":[],"usageMetadata":{"promptTokenCount":7,
                "candidatesTokenCount":2,"thoughtsTokenCount":1}}),
        ];
        assert_eq!(read_turn(&mut decoder, &chunks).unwrap(), "Hello");
        let reply = decoder.finish().unwrap();
        assert_eq!(reply.meta.finish_reason, "stop");
        assert_eq!(reply.meta.usage.input_tokens, Some(7));
        assert_eq!(reply.meta.usage.output_tokens, Some(3));
    }

    #[test]
    fn explicit_effort_never_silently_maps_no_thinking_to_minimal() {
        let mut p = profile();
        let messages = [Message::text(1, Role::User, "hello")];
        for (effort, level) in [
            (ReasoningEffort::Low, "low"),
            (ReasoningEffort::Medium, "medium"),
            (ReasoningEffort::High, "high"),
        ] {
            p.reasoning_effort = Some(effort);
            assert_eq!(
                body(&p, &messages, "gemini-3.8-flash", &[]).unwrap()["generationConfig"]
                    ["thinkingConfig"]["thinkingLevel"],
                level
            );
        }
        p.reasoning_effort = Some(ReasoningEffort::Instant);
        assert!(body(&p, &messages, "gemini-3.8-flash", &[]).is_err());
        assert_eq!(
            body(&p, &messages, "gemini-2.5-flash", &[]).unwrap()["generationConfig"]
                ["thinkingConfig"]["thinkingBudget"],
            0
        );
        assert!(body(&p, &messages, "gemini-2.5-pro", &[]).is_err());
        p.reasoning_effort = Some(ReasoningEffort::High);
        assert!(body(&p, &messages, "gemini-2.5-pro", &[]).is_err());
        p.reasoning_effort = Some(ReasoningEffort::Max);
        assert!(p.validate().is_err());
    }
}
