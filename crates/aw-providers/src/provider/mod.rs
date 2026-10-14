//! Real HTTP adapters. No model discovery, auth refresh, redirects or retries.
mod anthropic;
pub mod decode;
pub mod framing;
mod gemini;

use crate::{
    config::{Profile, ProviderKind, TokenField},
    Error, Message, Result,
};
use decode::{Decoded, OllamaDecoder, OpenAiDecoder, ResponsesDecoder};
use framing::{Lines, Sse};
use futures_util::StreamExt;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{io::Read, time::Duration};
use zeroize::Zeroizing;

pub use aw_agent_core::provider::{CompletionFuture, DeltaSink, ModelFuture, ModelReply, Provider};
use aw_agent_core::{Role, ToolSpec};

pub struct HttpProvider {
    client: reqwest::Client,
    profile: Profile,
    bearer: Option<Zeroizing<String>>,
    // Pins the selected environment credential for this provider object's
    // lifetime. A changed secret value cannot silently change billing/custody
    // mid-session or across a streamed tool-use continuation.
    pinned_env_credential: tokio::sync::OnceCell<[u8; 32]>,
    idle_timeout: Duration,
}
const MAX_WIRE_BYTES: usize = 32 * 1024 * 1024;

impl HttpProvider {
    /// Construct at first admitted request, not while displaying --help/profiles.
    pub fn new(profile: Profile, idle_timeout: Duration) -> Result<Self> {
        Self::build(profile, None, idle_timeout)
    }

    /// Explicit server/shared credential path. The bearer value is in-memory
    /// only and is never inserted into Profile/config/history metadata.
    pub fn new_with_bearer(
        profile: Profile,
        bearer: Zeroizing<String>,
        idle_timeout: Duration,
    ) -> Result<Self> {
        if profile.api_key_env.is_some() {
            return Err(Error::Config(
                "SecretStore-backed provider profile must not also select api_key_env".into(),
            ));
        }
        if bearer.trim().is_empty() || bearer.contains('\r') || bearer.contains('\n') {
            return Err(Error::Config(
                "selected server credential is empty or invalid".into(),
            ));
        }
        Self::build(profile, Some(bearer), idle_timeout)
    }

    fn build(
        profile: Profile,
        bearer: Option<Zeroizing<String>>,
        idle_timeout: Duration,
    ) -> Result<Self> {
        profile.validate()?;
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .no_proxy()
            .user_agent(concat!("aw/", env!("CARGO_PKG_VERSION")));
        if let Some(path) = &profile.ca_file {
            let file = std::fs::File::open(path)?;
            let mut bytes = Vec::new();
            file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 1024 * 1024 {
                return Err(Error::Limit("CA file exceeds 1 MiB"));
            }
            let cert = reqwest::Certificate::from_pem(&bytes)
                .map_err(|_| Error::Config("invalid PEM certificate".into()))?;
            builder = builder.add_root_certificate(cert);
        }
        let client = builder.build().map_err(crate::transport)?;
        Ok(Self {
            client,
            profile,
            bearer,
            pinned_env_credential: tokio::sync::OnceCell::new(),
            idle_timeout,
        })
    }

    async fn selected_env_credential(&self, name: &str) -> Result<Zeroizing<String>> {
        let key = Zeroizing::new(std::env::var(name).map_err(|_| {
            Error::Config(format!(
                "set the environment variable {name} before sending"
            ))
        })?);
        if key.trim().is_empty() || key.contains('\r') || key.contains('\n') {
            return Err(Error::Config("selected API key is empty or invalid".into()));
        }
        let digest: [u8; 32] = Sha256::digest(key.as_bytes()).into();
        let original = self
            .pinned_env_credential
            .get_or_init(|| async { digest })
            .await;
        if original != &digest {
            return Err(Error::Denied(
                "provider credential changed since this Agent was initialized; start a new provider context",
            ));
        }
        Ok(key)
    }

    fn responses_body(&self, messages: &[Message], model: &str) -> Value {
        let mut input = Vec::new();
        let mut instructions = Vec::new();
        for message in messages {
            if message.role == Role::System {
                if !message.content.is_empty() {
                    instructions.push(message.content.as_str());
                }
                continue;
            }
            if message.role == Role::Tool {
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": message.tool_call_id,
                    "output": message.content,
                }));
                continue;
            }
            if let Some(private) = &message.provider_context {
                let items: Value = serde_json::from_str(private).unwrap_or(Value::Null);
                if let Value::Array(items) = items {
                    input.extend(items);
                }
            }
            if !message.content.is_empty() || message.tool_calls.is_empty() {
                input.push(json!({
                    "role": message.role,
                    "content": message.content,
                }));
            }
            for call in &message.tool_calls {
                input.push(json!({
                    "type": "function_call",
                    "call_id": call.id,
                    "name": call.name,
                    "arguments": call.arguments.to_string(),
                }));
            }
        }
        let mut body = json!({
            "model": model,
            "input": input,
            "stream": true,
            "store": false,
        });
        if !instructions.is_empty() {
            body["instructions"] = json!(instructions.join("\n\n"));
        }
        if let Some(n) = self.profile.max_tokens {
            body["max_output_tokens"] = json!(n);
        }
        if let Some(effort) = self.profile.reasoning_effort {
            body["reasoning"] = json!({"effort": effort.api_value()});
        }
        if self.profile.reasoning_effort.is_none()
            || self.profile.reasoning_effort == Some(crate::config::ReasoningEffort::Instant)
        {
            if let Some(t) = self.profile.temperature {
                body["temperature"] = json!(t);
            }
        }
        body
    }

    pub fn body(&self, messages: &[Message], model: &str) -> Value {
        if self.profile.kind == ProviderKind::Gemini {
            return gemini::body(&self.profile, messages, model, &[]).unwrap_or(Value::Null);
        }
        if self.profile.kind == ProviderKind::Anthropic {
            // Diagnostic/test convenience only. Execution calls the fallible
            // builder and refuses invalid provider-private continuation.
            return anthropic::body(&self.profile, messages, model, &[]).unwrap_or(Value::Null);
        }
        if self.profile.kind == ProviderKind::OpenaiResponses {
            return self.responses_body(messages, model);
        }
        let messages:Vec<Value>=messages.iter().map(|m| {
            let mut value=json!({"role":m.role,"content":m.content});
            if !m.tool_calls.is_empty() {
                value["tool_calls"]=Value::Array(m.tool_calls.iter().enumerate().map(|(index,c)|match self.profile.kind {
                    ProviderKind::Openai=>json!({"id":c.id,"type":"function","function":{"name":c.name,"arguments":c.arguments.to_string()}}),
                    ProviderKind::OpenaiResponses=>unreachable!("Responses requests use responses_body"),
                    ProviderKind::Anthropic=>unreachable!("Anthropic requests use native body"),
                    ProviderKind::Gemini=>unreachable!("Gemini requests use native body"),
                    ProviderKind::Ollama=>json!({"type":"function","function":{"index":index,"name":c.name,"arguments":c.arguments}}),
                }).collect());
            }
            if m.role==Role::Tool {
                match self.profile.kind {
                    ProviderKind::Openai=>{value["tool_call_id"]=json!(m.tool_call_id);},
                    ProviderKind::OpenaiResponses=>unreachable!("Responses requests use responses_body"),
                    ProviderKind::Anthropic=>unreachable!("Anthropic requests use native body"),
                    ProviderKind::Gemini=>unreachable!("Gemini requests use native body"),
                    ProviderKind::Ollama=>{value["tool_name"]=json!(m.name);},
                }
            }
            if self.profile.kind==ProviderKind::Ollama {
                if let Some(private)=&m.provider_context{value["thinking"]=json!(private);}
            }
            value
        }).collect();
        let mut body = json!({"model":model,"messages":messages,"stream":true});
        match self.profile.kind {
            ProviderKind::Openai => {
                if let Some(n) = self.profile.max_tokens {
                    body[match self.profile.token_field {
                        TokenField::MaxTokens => "max_tokens",
                        TokenField::MaxCompletionTokens => "max_completion_tokens",
                    }] = json!(n);
                }
                if let Some(effort) = self.profile.reasoning_effort {
                    body["reasoning_effort"] = json!(effort.api_value());
                }
                if self.profile.reasoning_effort.is_none()
                    || self.profile.reasoning_effort
                        == Some(crate::config::ReasoningEffort::Instant)
                {
                    if let Some(t) = self.profile.temperature {
                        body["temperature"] = json!(t);
                    }
                }
                if self.profile.include_usage {
                    body["stream_options"] = json!({"include_usage":true});
                }
            }
            ProviderKind::OpenaiResponses => {
                unreachable!("Responses requests use responses_body")
            }
            ProviderKind::Anthropic => {
                unreachable!("Anthropic requests use native body")
            }
            ProviderKind::Gemini => {
                unreachable!("Gemini requests use native body")
            }
            ProviderKind::Ollama => {
                let mut options = Map::new();
                if let Some(n) = self.profile.max_tokens {
                    options.insert("num_predict".into(), json!(n));
                }
                if let Some(t) = self.profile.temperature {
                    options.insert("temperature".into(), json!(t));
                }
                if !options.is_empty() {
                    body["options"] = Value::Object(options);
                }
                // Keep provider-private reasoning out of the public transcript by
                // default. Reasoning-capable Ollama models can otherwise emit only
                // `message.thinking` and finish with empty visible content.
                let think = self.profile.think.unwrap_or_else(|| {
                    self.profile
                        .reasoning_effort
                        .is_some_and(|effort| effort != crate::config::ReasoningEffort::Instant)
                });
                body["think"] = json!(think);
            }
        }
        body
    }

    async fn execute(
        &self,
        messages: &[Message],
        model: &str,
        tools: &[ToolSpec],
        sink: &mut DeltaSink<'_>,
    ) -> Result<ModelReply> {
        aw_agent_core::validate_messages(messages)?;
        let mut body = if self.profile.kind == ProviderKind::Gemini {
            gemini::body(&self.profile, messages, model, tools)?
        } else if self.profile.kind == ProviderKind::Anthropic {
            anthropic::body(&self.profile, messages, model, tools)?
        } else {
            self.body(messages, model)
        };
        if !tools.is_empty()
            && !matches!(
                self.profile.kind,
                ProviderKind::Anthropic | ProviderKind::Gemini
            )
        {
            let definitions = tools
                .iter()
                .map(|tool| match self.profile.kind {
                    ProviderKind::OpenaiResponses => json!({
                        "type": "function",
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                        "strict": false,
                    }),
                    ProviderKind::Openai | ProviderKind::Ollama => json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.parameters,
                        }
                    }),
                    ProviderKind::Anthropic => unreachable!("Anthropic tools are built natively"),
                    ProviderKind::Gemini => unreachable!("Gemini tools are built natively"),
                })
                .collect::<Vec<_>>();
            if self.profile.kind == ProviderKind::OpenaiResponses
                && self.profile.oauth_connector.is_some()
            {
                // ChatGPT-plan direct routing currently accepts client functions
                // through an additional_tools developer input item rather than
                // arbitrary top-level tool configurations.
                let input = body
                    .get_mut("input")
                    .and_then(Value::as_array_mut)
                    .ok_or(Error::Protocol("Responses request input is not an array"))?;
                input.insert(
                    0,
                    json!({
                        "type": "additional_tools",
                        "role": "developer",
                        "tools": definitions,
                    }),
                );
            } else {
                body["tools"] = Value::Array(definitions);
            }
        }
        let mut request = self
            .client
            .post(self.profile.inference_endpoint(model)?)
            .json(&body);
        if self.profile.kind == ProviderKind::Anthropic {
            request = request.header("anthropic-version", "2023-06-01");
        }
        if let Some(key) = &self.bearer {
            request = if self.profile.kind == ProviderKind::Gemini {
                request.header("x-goog-api-key", key.as_str())
            } else {
                request.bearer_auth(key.as_str())
            };
        } else if let Some(name) = &self.profile.api_key_env {
            let key = self.selected_env_credential(name).await?;
            request = if self.profile.kind == ProviderKind::Gemini {
                request.header("x-goog-api-key", key.as_str())
            } else {
                request.bearer_auth(key.as_str())
            };
        } else if self.profile.kind == ProviderKind::Gemini {
            return Err(Error::Config(
                "Gemini API requires a selected credential reference".into(),
            ));
        }
        let response = request.send().await.map_err(crate::transport)?;
        if !response.status().is_success() {
            return Err(Error::http(response.status().as_u16()));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        match self.profile.kind {
            ProviderKind::Openai if content_type != "text/event-stream" => {
                return Err(Error::Protocol(
                    "expected SSE text/event-stream; this adapter requires streaming chat completions",
                ))
            }
            ProviderKind::OpenaiResponses if content_type != "text/event-stream" => {
                return Err(Error::Protocol(
                    "expected SSE text/event-stream; this adapter requires streaming Responses API",
                ))
            }
            ProviderKind::Anthropic if content_type != "text/event-stream" => {
                return Err(Error::Protocol("expected native Anthropic SSE text/event-stream"))
            }
            ProviderKind::Gemini if content_type != "text/event-stream" => {
                return Err(Error::Protocol("expected native Gemini SSE text/event-stream"))
            }
            ProviderKind::Ollama
                if !matches!(
                    content_type.as_str(),
                    "application/x-ndjson" | "application/json" | "application/jsonl"
                ) =>
            {
                return Err(Error::Protocol("expected an Ollama JSON-line stream"))
            }
            _ => {}
        }
        let mut stream = response.bytes_stream();
        let mut lines = Lines::default();
        let mut sse = Sse::default();
        let mut openai = OpenAiDecoder::with_tools(!tools.is_empty());
        let mut responses = ResponsesDecoder::with_tools(!tools.is_empty());
        let mut ollama = OllamaDecoder::with_tools(!tools.is_empty());
        let mut claude = anthropic::Decoder::new(!tools.is_empty(), model, &self.profile);
        let mut google = gemini::Decoder::new(!tools.is_empty(), model, &self.profile);
        let mut wire_bytes = 0usize;
        loop {
            let chunk = tokio::time::timeout(self.idle_timeout, stream.next())
                .await
                .map_err(|_| Error::Timeout)?;
            let Some(chunk) = chunk else {
                break;
            };
            let chunk = chunk.map_err(crate::transport)?;
            wire_bytes = wire_bytes.saturating_add(chunk.len());
            if wire_bytes > MAX_WIRE_BYTES {
                return Err(Error::Limit("provider stream exceeds 32 MiB"));
            }
            for line in lines.push(&chunk)? {
                let item = match self.profile.kind {
                    ProviderKind::Openai => match sse.line(&line)? {
                        Some(event) => openai.event(&event)?,
                        None => continue,
                    },
                    ProviderKind::OpenaiResponses => match sse.line(&line)? {
                        Some(event) => responses.event(&event)?,
                        None => continue,
                    },
                    ProviderKind::Ollama => ollama.line(&line)?,
                    ProviderKind::Anthropic => match sse.line(&line)? {
                        Some(event) => claude.event(&event)?,
                        None => continue,
                    },
                    ProviderKind::Gemini => match sse.line(&line)? {
                        Some(event) => google.event(&event)?,
                        None => continue,
                    },
                };
                if emit(item, sink)? {
                    return match self.profile.kind {
                        ProviderKind::Openai => openai.finish(),
                        ProviderKind::OpenaiResponses => responses.finish(),
                        ProviderKind::Ollama => ollama.finish(),
                        ProviderKind::Anthropic => claude.finish(),
                        ProviderKind::Gemini => google.finish(),
                    };
                }
            }
        }
        if let Some(line) = lines.finish()? {
            if self.profile.kind == ProviderKind::Ollama && emit(ollama.line(&line)?, sink)? {
                return ollama.finish();
            }
        }
        if self.profile.kind == ProviderKind::Gemini {
            // Gemini SSE has no [DONE] marker. Require explicit finishReason
            // and successful EOF; a truncated stream cannot dispatch tools.
            return google.finish();
        }
        Err(Error::Protocol("stream ended without its completion marker; partial output was not committed to context"))
    }
}
fn emit(item: Decoded, sink: &mut DeltaSink<'_>) -> Result<bool> {
    if let Some(text) = item.text {
        if !text.is_empty() {
            sink(&text)?;
        }
    }
    Ok(item.done)
}
impl Provider for HttpProvider {
    fn supports_tools(&self) -> bool {
        true
    }
    fn stream_tools<'a>(
        &'a self,
        messages: &'a [Message],
        model: &'a str,
        tools: &'a [ToolSpec],
        sink: &'a mut DeltaSink<'_>,
    ) -> ModelFuture<'a> {
        Box::pin(self.execute(messages, model, tools, sink))
    }
    fn stream<'a>(
        &'a self,
        messages: &'a [Message],
        model: &'a str,
        sink: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async move { Ok(self.execute(messages, model, &[], sink).await?.meta) })
    }
}

/// Lazily creates the HTTP client on the first call. Configuration/SDK construction
/// has no socket, credentials read, model discovery or server-side inference.
pub struct LazyHttpProvider {
    profile: Profile,
    idle_timeout: Duration,
    client: tokio::sync::OnceCell<HttpProvider>,
}
impl LazyHttpProvider {
    pub fn new(profile: Profile, idle_timeout: Duration) -> Result<Self> {
        profile.validate()?;
        Ok(Self {
            profile,
            idle_timeout,
            client: tokio::sync::OnceCell::new(),
        })
    }
}
impl Provider for LazyHttpProvider {
    fn supports_tools(&self) -> bool {
        true
    }
    fn stream_tools<'a>(
        &'a self,
        messages: &'a [Message],
        model: &'a str,
        tools: &'a [ToolSpec],
        sink: &'a mut DeltaSink<'_>,
    ) -> ModelFuture<'a> {
        Box::pin(async move {
            let client = self
                .client
                .get_or_try_init(|| async {
                    HttpProvider::new(self.profile.clone(), self.idle_timeout)
                })
                .await?;
            client.stream_tools(messages, model, tools, sink).await
        })
    }
    fn stream<'a>(
        &'a self,
        messages: &'a [Message],
        model: &'a str,
        sink: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async move {
            let client = self
                .client
                .get_or_try_init(|| async {
                    HttpProvider::new(self.profile.clone(), self.idle_timeout)
                })
                .await?;
            client.stream(messages, model, sink).await
        })
    }
}

#[cfg(test)]
mod tool_body_tests {
    use super::*;
    use aw_agent_core::{Role, ToolCall};
    #[tokio::test]
    async fn selected_environment_credential_is_pinned_per_provider_adapter() {
        // Unique variable avoids affecting other provider tests or configured
        // user account credentials. No network activity occurs in this test.
        const ENV: &str = "AW_NATIVE_R3A_PROVIDER_PIN_FIXTURE";
        let mut profile = crate::config::builtins().remove("anthropic").unwrap();
        profile.api_key_env = Some(ENV.into());
        let adapter = HttpProvider::new(profile, Duration::from_secs(1)).unwrap();
        std::env::set_var(ENV, "fixture-key-one");
        assert_eq!(
            adapter.selected_env_credential(ENV).await.unwrap().as_str(),
            "fixture-key-one"
        );
        std::env::set_var(ENV, "fixture-key-two");
        assert!(matches!(
            adapter.selected_env_credential(ENV).await,
            Err(Error::Denied(_))
        ));
        std::env::remove_var(ENV);
        assert!(adapter.selected_env_credential(ENV).await.is_err());
    }

    #[test]
    fn openai_emits_tool_call_and_result_link() {
        let p = crate::config::builtins().remove("llama").unwrap();
        let h = HttpProvider::new(p, Duration::from_secs(10)).unwrap();
        let mut a = Message::text(1, Role::Assistant, "");
        a.tool_calls.push(ToolCall {
            id: "c1".into(),
            name: "read".into(),
            arguments: json!({"path":"a"}),
        });
        let mut t = Message::text(2, Role::Tool, "result");
        t.tool_call_id = Some("c1".into());
        t.name = Some("read".into());
        let body = h.body(&[a, t], "m");
        assert_eq!(
            body["messages"][0]["tool_calls"][0]["function"]["arguments"],
            "{\"path\":\"a\"}"
        );
        assert_eq!(body["messages"][1]["tool_call_id"], "c1");
    }
    #[test]
    fn responses_system_message_becomes_instructions_not_input() {
        let p = crate::config::builtins()
            .remove("openai-responses")
            .unwrap();
        let h = HttpProvider::new(p, Duration::from_secs(10)).unwrap();
        let system = Message::text(1, Role::System, "system rules");
        let user = Message::text(2, Role::User, "hello");
        let body = h.body(&[system, user], "m");
        assert_eq!(body["instructions"], "system rules");
        assert_eq!(body["input"].as_array().unwrap().len(), 1);
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn responses_body_is_stateless_and_replays_opaque_reasoning_items() {
        let p = crate::config::builtins()
            .remove("openai-responses")
            .unwrap();
        let h = HttpProvider::new(p, Duration::from_secs(10)).unwrap();
        let mut assistant = Message::text(1, Role::Assistant, "");
        assistant.provider_context = Some(
            r#"[{"id":"rs_1","type":"reasoning","summary":[],"encrypted_content":"opaque"}]"#
                .into(),
        );
        assistant.tool_calls.push(ToolCall {
            id: "call_1".into(),
            name: "read".into(),
            arguments: json!({"path":"a"}),
        });
        let mut tool = Message::text(2, Role::Tool, "result");
        tool.tool_call_id = Some("call_1".into());
        tool.name = Some("read".into());
        let body = h.body(&[assistant, tool], "m");
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(body["input"][0]["type"], "reasoning");
        assert_eq!(body["input"][1]["type"], "function_call");
        assert_eq!(body["input"][2]["type"], "function_call_output");
        assert_eq!(body["input"][2]["call_id"], "call_1");
        assert!(body.get("messages").is_none());
    }

    #[test]
    fn reasoning_effort_uses_provider_native_wire_fields_without_silent_remapping() {
        let message = Message::text(1, Role::User, "hello");

        let mut chat = crate::config::builtins().remove("openai").unwrap();
        chat.reasoning_effort = Some(crate::config::ReasoningEffort::Xhigh);
        let provider = HttpProvider::new(chat, Duration::from_secs(10)).unwrap();
        let body = provider.body(std::slice::from_ref(&message), "m");
        assert_eq!(body["reasoning_effort"], "xhigh");
        assert!(body.get("temperature").is_none());

        let mut responses = crate::config::builtins()
            .remove("openai-responses")
            .unwrap();
        responses.reasoning_effort = Some(crate::config::ReasoningEffort::Max);
        let provider = HttpProvider::new(responses, Duration::from_secs(10)).unwrap();
        let body = provider.body(std::slice::from_ref(&message), "m");
        assert_eq!(body["reasoning"]["effort"], "max");
        assert!(body.get("temperature").is_none());

        let mut instant = crate::config::builtins().remove("openai").unwrap();
        instant.reasoning_effort = Some(crate::config::ReasoningEffort::Instant);
        instant.temperature = Some(0.5);
        let provider = HttpProvider::new(instant, Duration::from_secs(10)).unwrap();
        let body = provider.body(std::slice::from_ref(&message), "m");
        assert_eq!(body["reasoning_effort"], "none");
        assert_eq!(body["temperature"], 0.5);
    }

    #[test]
    fn ollama_reasoning_effort_is_truthfully_boolean() {
        let mut instant = crate::config::builtins().remove("ollama").unwrap();
        instant.reasoning_effort = Some(crate::config::ReasoningEffort::Instant);
        let provider = HttpProvider::new(instant, Duration::from_secs(10)).unwrap();
        let body = provider.body(&[Message::text(1, Role::User, "hello")], "m");
        assert_eq!(body["think"], false);

        let mut high = crate::config::builtins().remove("ollama").unwrap();
        high.reasoning_effort = Some(crate::config::ReasoningEffort::High);
        let provider = HttpProvider::new(high, Duration::from_secs(10)).unwrap();
        let body = provider.body(&[Message::text(1, Role::User, "hello")], "m");
        assert_eq!(body["think"], true);

        let mut invalid = crate::config::builtins().remove("ollama").unwrap();
        invalid.reasoning_effort = Some(crate::config::ReasoningEffort::Xhigh);
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn ollama_defaults_to_visible_answer_and_respects_explicit_thinking() {
        let mut profile = crate::config::builtins().remove("ollama").unwrap();
        let visible = HttpProvider::new(profile.clone(), Duration::from_secs(10)).unwrap();
        let body = visible.body(&[Message::text(1, Role::User, "hello")], "m");
        assert_eq!(body["think"], false);

        profile.think = Some(true);
        let thinking = HttpProvider::new(profile, Duration::from_secs(10)).unwrap();
        let body = thinking.body(&[Message::text(1, Role::User, "hello")], "m");
        assert_eq!(body["think"], true);
    }

    #[test]
    fn native_ollama_preserves_objects_and_tool_name() {
        let p = crate::config::builtins().remove("ollama").unwrap();
        let h = HttpProvider::new(p, Duration::from_secs(10)).unwrap();
        let mut a = Message::text(1, Role::Assistant, "");
        a.tool_calls.push(ToolCall {
            id: "c1".into(),
            name: "read".into(),
            arguments: json!({"path":"a"}),
        });
        a.provider_context = Some("private continuation".into());
        let mut t = Message::text(2, Role::Tool, "result");
        t.tool_call_id = Some("c1".into());
        t.name = Some("read".into());
        let body = h.body(&[a, t], "m");
        assert!(body["messages"][0]["tool_calls"][0]["function"]["arguments"].is_object());
        assert_eq!(body["messages"][1]["tool_name"], "read");
        assert_eq!(body["messages"][0]["thinking"], "private continuation");
    }
}
