//! Bounded decoders. Tools are buffered until an explicit complete response;
//! streamed argument fragments are never execution notifications.
use crate::{Error, Result};
pub use aw_agent_core::completion::{CompletionMeta, Usage};
use aw_agent_core::{
    tool::{MAX_TOOL_ARGUMENT_BYTES, MAX_TOOL_CALLS_PER_ROUND},
    ModelReply, ToolCall,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
};
fn reason(s: &str) -> &'static str {
    match s {
        "stop" => "stop",
        "length" => "length",
        "content_filter" => "content_filter",
        "tool_calls" => "tool_calls",
        _ => "other",
    }
}
#[derive(Debug, Default)]
pub struct Decoded {
    pub text: Option<String>,
    pub done: bool,
}
#[derive(Default)]
struct Fragments {
    id: String,
    name: String,
    arguments: String,
}
#[derive(Default)]
pub struct OpenAiDecoder {
    pub meta: CompletionMeta,
    allow_tools: bool,
    calls: BTreeMap<u64, Fragments>,
}
impl OpenAiDecoder {
    pub fn with_tools(allow: bool) -> Self {
        Self {
            allow_tools: allow,
            ..Self::default()
        }
    }
    pub fn event(&mut self, data: &str) -> Result<Decoded> {
        if data.trim() == "[DONE]" {
            if !self.calls.is_empty() && self.meta.finish_reason != "tool_calls" {
                return Err(Error::Protocol(
                    "tool stream ended without explicit tool_calls finish",
                ));
            }
            if self.meta.finish_reason.is_empty() {
                self.meta.finish_reason = "stop".into();
            }
            return Ok(Decoded {
                text: None,
                done: true,
            });
        }
        if data.is_empty() {
            return Ok(Decoded::default());
        }
        let o: Value = serde_json::from_str(data)
            .map_err(|_| Error::Protocol("invalid chat-completions JSON"))?;
        if o.get("error").is_some_and(|v| !v.is_null()) {
            return Err(Error::Protocol("provider error event (body withheld)"));
        }
        if let Some(u) = o.get("usage").filter(|v| !v.is_null()) {
            if let Some(n) = u.get("prompt_tokens").and_then(Value::as_u64) {
                self.meta.usage.input_tokens = Some(n);
            }
            if let Some(n) = u.get("completion_tokens").and_then(Value::as_u64) {
                self.meta.usage.output_tokens = Some(n);
            }
        }
        let choices = o
            .get("choices")
            .and_then(Value::as_array)
            .ok_or(Error::Protocol("missing choices"))?;
        let Some(choice) = choices.first() else {
            return Ok(Decoded::default());
        };
        if choices.len() != 1
            || choice
                .get("index")
                .and_then(Value::as_u64)
                .is_some_and(|i| i != 0)
        {
            return Err(Error::Protocol("multiple choices unsupported"));
        }
        let d = choice
            .get("delta")
            .ok_or(Error::Protocol("missing delta"))?;
        if d.get("function_call").is_some_and(|v| !v.is_null()) {
            return Err(Error::Protocol("legacy function_call protocol unsupported"));
        }
        if let Some(calls) = d.get("tool_calls").filter(|v| !v.is_null()) {
            let array = calls
                .as_array()
                .ok_or(Error::Protocol("invalid tool delta"))?;
            if !array.is_empty() && !self.allow_tools {
                return Err(Error::Protocol("tools not enabled"));
            }
            for call in array {
                let index = call
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or(Error::Protocol("tool delta has no index"))?;
                if index >= MAX_TOOL_CALLS_PER_ROUND as u64 {
                    return Err(Error::Limit("too many tool calls"));
                }
                if call
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|t| t != "function")
                {
                    return Err(Error::Protocol("unknown tool-call type"));
                }
                let f = self.calls.entry(index).or_default();
                append(&mut f.id, call.get("id"), 128)?;
                if let Some(fun) = call.get("function") {
                    if !fun.is_object() {
                        return Err(Error::Protocol("invalid tool function delta"));
                    }
                    append(&mut f.name, fun.get("name"), 128)?;
                    append(
                        &mut f.arguments,
                        fun.get("arguments"),
                        MAX_TOOL_ARGUMENT_BYTES,
                    )?;
                }
            }
        }
        if let Some(r) = choice.get("finish_reason").and_then(Value::as_str) {
            if r == "tool_calls" && !self.allow_tools {
                return Err(Error::Protocol("tools not enabled"));
            }
            self.meta.finish_reason = reason(r).into();
        }
        Ok(Decoded {
            text: text(d.get("content"))?,
            done: false,
        })
    }
    pub fn finish(self) -> Result<ModelReply> {
        let mut calls = Vec::new();
        for (_, c) in self.calls {
            let arguments = serde_json::from_str(&c.arguments).map_err(|_| {
                Error::Protocol("incomplete/malformed tool arguments; no execution")
            })?;
            let call = ToolCall {
                id: c.id,
                name: c.name,
                arguments,
            };
            call.validate_envelope()?;
            calls.push(call);
        }
        if self.meta.finish_reason == "tool_calls" && calls.is_empty() {
            return Err(Error::Protocol("tool finish without a call"));
        }
        Ok(ModelReply {
            meta: self.meta,
            tool_calls: calls,
            provider_context: None,
        })
    }
}
#[derive(Default)]
pub struct ResponsesDecoder {
    pub meta: CompletionMeta,
    allow_tools: bool,
    calls: BTreeMap<u64, Fragments>,
    reasoning: Vec<Value>,
}

impl ResponsesDecoder {
    pub fn with_tools(allow: bool) -> Self {
        Self {
            allow_tools: allow,
            ..Self::default()
        }
    }

    pub fn event(&mut self, data: &str) -> Result<Decoded> {
        if data.trim() == "[DONE]" {
            return Err(Error::Protocol(
                "Responses stream ended without response.completed",
            ));
        }
        if data.trim().is_empty() {
            return Ok(Decoded::default());
        }
        let event: Value = serde_json::from_str(data)
            .map_err(|_| Error::Protocol("invalid Responses API event JSON"))?;
        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .ok_or(Error::Protocol("Responses API event has no type"))?;
        match kind {
            "response.output_text.delta" | "response.refusal.delta" => Ok(Decoded {
                text: text(event.get("delta"))?,
                done: false,
            }),
            "response.output_item.added" => {
                let Some(item) = event.get("item") else {
                    return Err(Error::Protocol("Responses output item missing"));
                };
                if item.get("type").and_then(Value::as_str) == Some("function_call") {
                    if !self.allow_tools {
                        return Err(Error::Protocol("tools not enabled"));
                    }
                    let index = event.get("output_index").and_then(Value::as_u64).ok_or(
                        Error::Protocol("Responses function call has no output index"),
                    )?;
                    if index >= MAX_TOOL_CALLS_PER_ROUND as u64 {
                        return Err(Error::Limit("too many tool calls"));
                    }
                    let call = self.calls.entry(index).or_default();
                    append(&mut call.id, item.get("call_id"), 128)?;
                    append(&mut call.name, item.get("name"), 128)?;
                    append(
                        &mut call.arguments,
                        item.get("arguments"),
                        MAX_TOOL_ARGUMENT_BYTES,
                    )?;
                }
                Ok(Decoded::default())
            }
            "response.function_call_arguments.delta" => {
                if !self.allow_tools {
                    return Err(Error::Protocol("tools not enabled"));
                }
                let index = event
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .ok_or(Error::Protocol("Responses tool delta has no output index"))?;
                let call = self.calls.get_mut(&index).ok_or(Error::Protocol(
                    "Responses tool delta preceded its call item",
                ))?;
                append(
                    &mut call.arguments,
                    event.get("delta"),
                    MAX_TOOL_ARGUMENT_BYTES,
                )?;
                Ok(Decoded::default())
            }
            "response.output_item.done" => {
                if let Some(item) = event.get("item") {
                    if item.get("type").and_then(Value::as_str) == Some("reasoning") {
                        let encoded = serde_json::to_vec(item)
                            .map_err(|_| Error::Protocol("invalid Responses reasoning item"))?;
                        let existing: usize = self
                            .reasoning
                            .iter()
                            .map(|item| serde_json::to_vec(item).map_or(0, |bytes| bytes.len()))
                            .sum();
                        if existing.saturating_add(encoded.len()) > 128 * 1024 {
                            return Err(Error::Limit(
                                "provider-private reasoning continuation exceeds 128 KiB",
                            ));
                        }
                        if !item.get("encrypted_content").is_some_and(Value::is_string) {
                            return Err(Error::Protocol(
                                "stateless Responses reasoning item lacks encrypted continuation",
                            ));
                        }
                        self.reasoning.push(item.clone());
                    }
                }
                Ok(Decoded::default())
            }
            "response.function_call_arguments.done" => {
                if let (Some(index), Some(arguments)) = (
                    event.get("output_index").and_then(Value::as_u64),
                    event.get("arguments").and_then(Value::as_str),
                ) {
                    let call = self.calls.get_mut(&index).ok_or(Error::Protocol(
                        "Responses tool completion preceded its call item",
                    ))?;
                    if arguments.len() > MAX_TOOL_ARGUMENT_BYTES {
                        return Err(Error::Limit("tool delta too large"));
                    }
                    // Some servers stream all argument deltas and repeat the final
                    // aggregate in .done. Prefer the explicit final aggregate.
                    call.arguments.clear();
                    call.arguments.push_str(arguments);
                }
                Ok(Decoded::default())
            }
            "response.completed" => {
                let response = event
                    .get("response")
                    .ok_or(Error::Protocol("Responses completion missing response"))?;
                if let Some(usage) = response.get("usage") {
                    self.meta.usage.input_tokens =
                        usage.get("input_tokens").and_then(Value::as_u64);
                    self.meta.usage.output_tokens =
                        usage.get("output_tokens").and_then(Value::as_u64);
                }
                self.meta.finish_reason = if self.calls.is_empty() {
                    "stop".into()
                } else {
                    "tool_calls".into()
                };
                Ok(Decoded {
                    text: None,
                    done: true,
                })
            }
            "response.incomplete" => Err(Error::Protocol(
                "Responses API returned an incomplete response",
            )),
            "response.failed" | "error" => {
                Err(Error::Protocol("Responses API error event (body withheld)"))
            }
            // Reasoning/continuation events, lifecycle markers, annotations and
            // completed text items are intentionally not exposed as transcript
            // text. Provider-private reasoning remains private.
            _ => Ok(Decoded::default()),
        }
    }

    pub fn finish(self) -> Result<ModelReply> {
        let mut calls = Vec::new();
        for (_, call) in self.calls {
            if call.id.is_empty() || call.name.is_empty() {
                return Err(Error::Protocol("incomplete Responses function call"));
            }
            let arguments = serde_json::from_str(&call.arguments).map_err(|_| {
                Error::Protocol("incomplete/malformed tool arguments; no execution")
            })?;
            let call = ToolCall {
                id: call.id,
                name: call.name,
                arguments,
            };
            call.validate_envelope()?;
            calls.push(call);
        }
        if self.meta.finish_reason == "tool_calls" && calls.is_empty() {
            return Err(Error::Protocol("tool finish without a call"));
        }
        let provider_context = if self.reasoning.is_empty() {
            None
        } else {
            Some(
                serde_json::to_string(&self.reasoning)
                    .map_err(|_| Error::Protocol("cannot encode Responses continuation"))?,
            )
        };
        Ok(ModelReply {
            meta: self.meta,
            tool_calls: calls,
            provider_context,
        })
    }
}

fn append(out: &mut String, value: Option<&Value>, max: usize) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.is_null() {
        return Ok(());
    }
    let part = value
        .as_str()
        .ok_or(Error::Protocol("expected string delta"))?;
    if out.len().saturating_add(part.len()) > max {
        return Err(Error::Limit("tool delta too large"));
    }
    out.push_str(part);
    Ok(())
}
fn text(value: Option<&Value>) -> Result<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(v)) => Ok(Some(v.clone())),
        _ => Err(Error::Protocol("non-text content unsupported")),
    }
}
static OLLAMA_INVOCATION: AtomicU64 = AtomicU64::new(1);
pub struct OllamaDecoder {
    pub meta: CompletionMeta,
    allow_tools: bool,
    calls: Vec<ToolCall>,
    thinking: String,
    saw_thinking: bool,
    saw_visible_text: bool,
    nonce: u64,
}
impl Default for OllamaDecoder {
    fn default() -> Self {
        Self {
            meta: CompletionMeta::default(),
            allow_tools: false,
            calls: Vec::new(),
            thinking: String::new(),
            saw_thinking: false,
            saw_visible_text: false,
            nonce: OLLAMA_INVOCATION.fetch_add(1, Ordering::Relaxed),
        }
    }
}
impl OllamaDecoder {
    pub fn with_tools(allow: bool) -> Self {
        Self {
            allow_tools: allow,
            ..Self::default()
        }
    }
    pub fn line(&mut self, line: &str) -> Result<Decoded> {
        if line.trim().is_empty() {
            return Ok(Decoded::default());
        }
        let o: Value =
            serde_json::from_str(line).map_err(|_| Error::Protocol("invalid Ollama JSON"))?;
        if o.get("error").is_some_and(|v| !v.is_null()) {
            return Err(Error::Protocol("Ollama error (body withheld)"));
        }
        let done = o
            .get("done")
            .and_then(Value::as_bool)
            .ok_or(Error::Protocol("missing Ollama done flag"))?;
        let m = o.get("message");
        if let Some(array) = m.and_then(|v| v.get("tool_calls")).filter(|v| !v.is_null()) {
            let array = array
                .as_array()
                .ok_or(Error::Protocol("invalid native tool calls"))?;
            if !array.is_empty() && !self.allow_tools {
                return Err(Error::Protocol("tools not enabled"));
            }
            if self.calls.len() + array.len() > MAX_TOOL_CALLS_PER_ROUND {
                return Err(Error::Limit("too many tool calls"));
            }
            // Native Ollama emits complete argument objects, not the OpenAI
            // string fragments. Do not guess/merge two incompatible protocols.
            for item in array {
                let f = item
                    .get("function")
                    .ok_or(Error::Protocol("tool function missing"))?;
                let name = f
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or(Error::Protocol("tool name missing"))?;
                let arguments = f
                    .get("arguments")
                    .cloned()
                    .ok_or(Error::Protocol("tool arguments missing"))?;
                let call = ToolCall {
                    id: format!("ollama_{}_{}", self.nonce, self.calls.len()),
                    name: name.into(),
                    arguments,
                };
                call.validate_envelope()?;
                self.calls.push(call);
            }
        }
        let thinking = m
            .and_then(|v| v.get("thinking"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        if thinking.is_some() {
            self.saw_thinking = true;
        }
        if self.allow_tools {
            append(
                &mut self.thinking,
                m.and_then(|v| v.get("thinking")),
                128 * 1024,
            )?;
        }
        let visible = text(m.and_then(|v| v.get("content")))?;
        if visible.as_deref().is_some_and(|value| !value.is_empty()) {
            self.saw_visible_text = true;
        }
        if done {
            self.meta.finish_reason = reason(
                o.get("done_reason")
                    .and_then(Value::as_str)
                    .unwrap_or("stop"),
            )
            .into();
            self.meta.usage.input_tokens = o.get("prompt_eval_count").and_then(Value::as_u64);
            self.meta.usage.output_tokens = o.get("eval_count").and_then(Value::as_u64);
        }
        Ok(Decoded {
            text: visible,
            done,
        })
    }
    pub fn finish(self) -> Result<ModelReply> {
        if self.calls.is_empty() && self.saw_thinking && !self.saw_visible_text {
            return Err(Error::Protocol(
                "Ollama returned private reasoning without a visible answer; ordinary aw turns request think=false",
            ));
        }
        Ok(ModelReply {
            meta: self.meta,
            tool_calls: self.calls,
            provider_context: if self.thinking.is_empty() {
                None
            } else {
                Some(self.thinking)
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn responses_stream_text_and_complete() {
        let mut decoder = ResponsesDecoder::with_tools(false);
        assert_eq!(
            decoder
                .event(r#"{"type":"response.output_text.delta","delta":"hello"}"#)
                .unwrap()
                .text
                .as_deref(),
            Some("hello")
        );
        assert!(decoder
            .event(r#"{"type":"response.completed","response":{"usage":{"input_tokens":7,"output_tokens":2}}}"#)
            .unwrap()
            .done);
        let reply = decoder.finish().unwrap();
        assert_eq!(reply.meta.usage.input_tokens, Some(7));
        assert_eq!(reply.meta.usage.output_tokens, Some(2));
        assert_eq!(reply.meta.finish_reason, "stop");
    }

    #[test]
    fn responses_done_marker_without_completed_event_is_not_success() {
        let mut decoder = ResponsesDecoder::with_tools(false);
        assert!(decoder.event("[DONE]").is_err());
    }

    #[test]
    fn responses_incomplete_is_not_success() {
        let mut decoder = ResponsesDecoder::with_tools(false);
        assert!(decoder
            .event(
                r#"{"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}}}"#
            )
            .is_err());
    }

    #[test]
    fn responses_reasoning_is_opaque_provider_context() {
        let mut decoder = ResponsesDecoder::with_tools(true);
        decoder
            .event(
                r#"{"type":"response.output_item.done","item":{"id":"rs_1","type":"reasoning","summary":[],"encrypted_content":"opaque"}}"#,
            )
            .unwrap();
        decoder
            .event(
                r#"{"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","call_id":"call_1","name":"read","arguments":""}}"#,
            )
            .unwrap();
        decoder
            .event(
                r#"{"type":"response.function_call_arguments.done","output_index":1,"arguments":"{\"path\":\"a\"}"}"#,
            )
            .unwrap();
        decoder
            .event(r#"{"type":"response.completed","response":{"usage":{}}}"#)
            .unwrap();
        let reply = decoder.finish().unwrap();
        assert_eq!(reply.tool_calls.len(), 1);
        let private = reply.provider_context.unwrap();
        assert!(private.contains("encrypted_content"));
        assert!(private.contains("opaque"));
    }

    #[test]
    fn openai_text_then_done() {
        let mut d = OpenAiDecoder::default();
        assert_eq!(
            d.event(r#"{"choices":[{"index":0,"delta":{"content":"hi"},"finish_reason":null}]}"#)
                .unwrap()
                .text
                .as_deref(),
            Some("hi")
        );
        assert!(d.event("[DONE]").unwrap().done);
    }
    #[test]
    fn openai_usage_only() {
        let mut d = OpenAiDecoder::default();
        d.event(r#"{"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":3}}"#)
            .unwrap();
        assert_eq!(d.meta.usage.input_tokens, Some(12));
    }
    #[test]
    fn length_is_not_complete() {
        let mut d = OpenAiDecoder::default();
        d.event(r#"{"choices":[{"delta":{},"finish_reason":"length"}]}"#)
            .unwrap();
        d.event("[DONE]").unwrap();
        assert!(!d.meta.complete());
    }
    #[test]
    fn tool_calls_are_not_executed() {
        assert!(OpenAiDecoder::default()
            .event(r#"{"choices":[{"delta":{"tool_calls":[{}]}}]}"#)
            .is_err());
    }
    #[test]
    fn error_payload_never_echoed() {
        let e = OpenAiDecoder::default()
            .event(r#"{"error":{"message":"SECRET"}}"#)
            .unwrap_err();
        assert!(!e.to_string().contains("SECRET"));
    }
    #[test]
    fn reasoning_is_not_output() {
        assert!(OpenAiDecoder::default()
            .event(r#"{"choices":[{"delta":{"reasoning_content":"private"}}]}"#)
            .unwrap()
            .text
            .is_none());
    }
    #[test]
    fn ollama_final_usage() {
        let mut d = OllamaDecoder::default();
        let v = d
            .line(r#"{"message":{"content":"ok"},"done":true,"eval_count":2}"#)
            .unwrap();
        assert!(v.done);
        assert_eq!(d.meta.usage.output_tokens, Some(2));
    }
    #[test]
    fn malformed_record_rejected() {
        assert!(OllamaDecoder::default().line("not json").is_err());
        assert!(OpenAiDecoder::default().event("{}").is_err());
    }
    #[test]
    fn missing_done_is_not_success() {
        assert!(OllamaDecoder::default()
            .line(r#"{"message":{"content":"ok"}}"#)
            .is_err());
    }
    #[test]
    fn unknown_finish_reason_is_bounded_and_incomplete() {
        let mut d = OllamaDecoder::default();
        d.line(r#"{"message":{"content":"x"},"done":true,"done_reason":"unknown-server-value"}"#)
            .unwrap();
        assert_eq!(d.meta.finish_reason, "other");
        assert!(!d.meta.complete());
    }
    #[test]
    fn non_text_rejected() {
        assert!(OpenAiDecoder::default()
            .event(r#"{"choices":[{"delta":{"content":[1]}}]}"#)
            .is_err());
    }
}

#[cfg(test)]
mod tool_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn fragmented_openai_tool_is_buffered_until_done() {
        let mut d = OpenAiDecoder::with_tools(true);
        d.event(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"read","arguments":"{\"path\":"}}]},"finish_reason":null}]}).to_string()).unwrap();
        d.event(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"a.txt\"}"}}]},"finish_reason":"tool_calls"}]}).to_string()).unwrap();
        d.event("[DONE]").unwrap();
        let r = d.finish().unwrap();
        assert_eq!(r.tool_calls[0].arguments, json!({"path":"a.txt"}));
    }
    #[test]
    fn incomplete_tool_json_is_never_executable() {
        let mut d = OpenAiDecoder::with_tools(true);
        d.event(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c","function":{"name":"read","arguments":"{"}}]},"finish_reason":"tool_calls"}]}).to_string()).unwrap();
        d.event("[DONE]").unwrap();
        assert!(d.finish().is_err());
    }
    #[test]
    fn native_argument_strings_not_guessed_as_json() {
        let mut d = OllamaDecoder::with_tools(true);
        assert!(d.line(&json!({"message":{"tool_calls":[{"function":{"name":"read","arguments":"{}"}}]},"done":true}).to_string()).is_err());
    }
    #[test]
    fn tool_call_without_finish_marker_is_rejected() {
        let mut d = OpenAiDecoder::with_tools(true);
        d.event(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c","function":{"name":"read","arguments":"{}"}}]},"finish_reason":null}]}).to_string()).unwrap();
        assert!(d.event("[DONE]").is_err());
    }
    #[test]
    fn tool_call_cap_is_enforced() {
        let mut d = OpenAiDecoder::with_tools(true);
        // Shared MAX_BATCH increased from 16 to 128 in R9. The first
        // out-of-range index remains rejected without allocating 129 calls.
        assert!(d
            .event(
                &json!({"choices":[{"delta":{"tool_calls":[{
                    "index":aw_agent_core::tool::MAX_TOOL_CALLS_PER_ROUND,
                    "id":"c","function":{"name":"x","arguments":"{}"}
                }]}}]})
                .to_string()
            )
            .is_err());
    }
    #[test]
    fn thinking_only_without_tools_is_not_exposed_or_accepted_as_blank_output() {
        let mut d = OllamaDecoder::default();
        let x = d
            .line(
                &json!({"message":{"thinking":"private-chain","content":""},"done":true})
                    .to_string(),
            )
            .unwrap();
        assert!(x.text.as_deref().is_none_or(str::is_empty));
        let error = d.finish().unwrap_err().to_string();
        assert!(error.contains("without a visible answer"));
        assert!(!error.contains("private-chain"));
    }

    #[test]
    fn native_thinking_is_continuation_not_text() {
        let mut d = OllamaDecoder::with_tools(true);
        let x=d.line(&json!({"message":{"thinking":"private-continuation","tool_calls":[{"function":{"name":"read","arguments":{"path":"a"}}}]},"done":true}).to_string()).unwrap();
        assert!(x.text.is_none());
        let r = d.finish().unwrap();
        assert_eq!(r.provider_context.as_deref(), Some("private-continuation"));
        assert_eq!(r.tool_calls.len(), 1);
        assert!(!serde_json::to_string(&r)
            .unwrap()
            .contains("private-continuation"));
    }
    #[test]
    fn usage_only_trailer_is_preserved_after_tools() {
        let mut d = OpenAiDecoder::with_tools(true);
        d.event(&json!({"choices":[{"delta":{},"finish_reason":"stop"}]}).to_string())
            .unwrap();
        d.event(
            &json!({"choices":[],"usage":{"prompt_tokens":9,"completion_tokens":7}}).to_string(),
        )
        .unwrap();
        d.event("[DONE]").unwrap();
        let r = d.finish().unwrap();
        assert_eq!(r.meta.usage.input_tokens, Some(9));
    }
}
