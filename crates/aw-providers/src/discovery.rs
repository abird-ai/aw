//! Explicit bounded model discovery. This is descriptive catalog input, never
//! a grant to run a model, change account, or automatically select a new route.
use crate::{
    catalog::{ModelInfo, ModelOrigin, PriceOrigin, Pricing},
    config::{validate_model, Profile, ProviderKind},
    transport, Error, Result,
};
use futures_util::StreamExt;
use reqwest::Url;
use serde_json::Value;
use std::{collections::BTreeSet, io::Read, time::Duration};

const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;
const MAX_MODELS: usize = 2048;
const MAX_PAGES: usize = 16;

/// Both TUI onboarding and CLI use the same private cache and discovery source.
pub async fn discover_models(profile: &Profile) -> Result<Vec<String>> {
    crate::catalog::Cache::default_user()?
        .for_setup(profile)
        .await
}

pub async fn fetch_catalog(profile: &Profile, secret: &[u8]) -> Result<Vec<ModelInfo>> {
    profile.validate()?;
    if profile.oauth_connector.is_some() {
        return Err(Error::Denied(
            "generic discovery cannot access OAuth/plan credentials",
        ));
    }
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .no_proxy()
        .user_agent(concat!(
            "aw/",
            env!("CARGO_PKG_VERSION"),
            " model-discovery"
        ));
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
    let client = builder.build().map_err(transport)?;
    let base = discovery_endpoint(profile)?;
    if profile.kind == ProviderKind::Gemini && secret.is_empty() {
        return Err(Error::Config(
            "Gemini model discovery requires an API key".into(),
        ));
    }
    if profile.api_key_env.is_none() && !secret.is_empty() {
        return Err(Error::Denied(
            "discovery API key lacks an explicit credential reference",
        ));
    }
    let official_openrouter = profile.kind == ProviderKind::Openai
        && base.host_str() == Some("openrouter.ai")
        && base.path() == "/api/v1/models";
    let mut models = Vec::new();
    let mut names = BTreeSet::new();
    let mut cursors = BTreeSet::new();
    let mut cursor = None::<String>;
    let mut bytes_total = 0usize;
    for page in 0..MAX_PAGES {
        let mut endpoint = base.clone();
        if let Some(next) = cursor.as_deref() {
            match profile.kind {
                ProviderKind::Anthropic => {
                    endpoint.query_pairs_mut().append_pair("after_id", next);
                }
                ProviderKind::Gemini => {
                    endpoint.query_pairs_mut().append_pair("pageToken", next);
                }
                _ => return Err(Error::Protocol("provider returned unsupported pagination")),
            }
        }
        let mut request = client.get(endpoint);
        if profile.kind == ProviderKind::Anthropic {
            request = request.header("anthropic-version", "2023-06-01");
        }
        if !secret.is_empty() {
            let key = std::str::from_utf8(secret)
                .map_err(|_| Error::Denied("invalid selected discovery credential"))?;
            request = if profile.kind == ProviderKind::Gemini {
                request.header("x-goog-api-key", key)
            } else {
                request.bearer_auth(key)
            };
        }
        let response = request.send().await.map_err(transport)?;
        if !response.status().is_success() {
            return Err(Error::http(response.status().as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|v| v > MAX_PAGE_BYTES as u64)
        {
            return Err(Error::Limit("model discovery page too large"));
        }
        let mut body = Vec::new();
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.map_err(transport)?;
            bytes_total = bytes_total.saturating_add(chunk.len());
            if body.len().saturating_add(chunk.len()) > MAX_PAGE_BYTES
                || bytes_total > MAX_TOTAL_BYTES
            {
                return Err(Error::Limit(
                    "model discovery exceeds bounded response bytes",
                ));
            }
            body.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&body)
            .map_err(|_| Error::Protocol("invalid model discovery JSON"))?;
        let (entries, next) = parse_page(profile.kind, &value, official_openrouter)?;
        for info in entries {
            if names.insert(info.id.clone()) {
                if models.len() >= MAX_MODELS {
                    return Err(Error::Limit("model list too large"));
                }
                models.push(info);
            }
        }
        let Some(next) = next else {
            if models.is_empty() {
                return Err(Error::Config(
                    "provider returned no usable models; enter a model ID manually".into(),
                ));
            }
            models.sort_by(|a, b| a.id.cmp(&b.id));
            return Ok(models);
        };
        if next.is_empty()
            || next.len() > 512
            || next.chars().any(char::is_control)
            || !cursors.insert(next.clone())
        {
            return Err(Error::Protocol("invalid/cyclic model pagination cursor"));
        }
        if page + 1 == MAX_PAGES {
            return Err(Error::Limit("model list requires more than 16 pages"));
        }
        cursor = Some(next);
    }
    Err(Error::Limit("model discovery was incomplete"))
}

pub(crate) fn discovery_endpoint(profile: &Profile) -> Result<Url> {
    let _ = profile.endpoint()?;
    let mut url = Url::parse(&profile.base_url)
        .map_err(|_| Error::Config("invalid provider base_url".into()))?;
    let prefix = url.path().trim_end_matches('/');
    let suffix = match profile.kind {
        ProviderKind::Openai
        | ProviderKind::OpenaiResponses
        | ProviderKind::Anthropic
        | ProviderKind::Gemini => "models",
        ProviderKind::Ollama => "api/tags",
    };
    url.set_path(&format!("{prefix}/{suffix}"));
    Ok(url)
}

pub(crate) fn parse_page(
    kind: ProviderKind,
    value: &Value,
    openrouter: bool,
) -> Result<(Vec<ModelInfo>, Option<String>)> {
    let (field, primary) = match kind {
        ProviderKind::Openai | ProviderKind::OpenaiResponses | ProviderKind::Anthropic => {
            ("data", "id")
        }
        ProviderKind::Gemini | ProviderKind::Ollama => ("models", "name"),
    };
    let entries = value
        .get(field)
        .and_then(Value::as_array)
        .ok_or(Error::Protocol(
            "model discovery response missing model array",
        ))?;
    if entries.len() > MAX_MODELS {
        return Err(Error::Limit("model page too large"));
    }
    let mut models = Vec::new();
    let positive = |value: &Value| value.as_u64().filter(|n| *n > 0 && *n <= 1_000_000_000);
    for entry in entries {
        let Some(raw) = entry.get(primary).and_then(Value::as_str).or_else(|| {
            if kind == ProviderKind::Ollama {
                entry.get("model").and_then(Value::as_str)
            } else {
                None
            }
        }) else {
            continue;
        };
        let id = if kind == ProviderKind::Gemini {
            let Some(id) = raw.strip_prefix("models/") else {
                continue;
            };
            id
        } else {
            raw
        };
        if validate_model(id).is_err() {
            continue;
        }
        let mut info = ModelInfo::configured(id.to_owned());
        info.origin = ModelOrigin::ProviderDiscovery;
        match kind {
            ProviderKind::Anthropic => {
                info.display_name = label(entry, "display_name");
                info.context_tokens = entry.get("max_input_tokens").and_then(positive);
                info.input_tokens = info.context_tokens;
                info.output_tokens = entry.get("max_tokens").and_then(positive);
                info.supports_thinking = entry
                    .pointer("/capabilities/thinking/supported")
                    .and_then(Value::as_bool);
                info.lifecycle = entry
                    .get("lifecycle")
                    .and_then(Value::as_str)
                    .filter(|v| matches!(*v, "active" | "deprecated" | "retired"))
                    .map(str::to_owned);
            }
            ProviderKind::Gemini => {
                if entry
                    .get("supportedGenerationMethods")
                    .and_then(Value::as_array)
                    .is_some_and(|methods| {
                        !methods
                            .iter()
                            .any(|m| m.as_str() == Some("generateContent"))
                    })
                {
                    continue;
                }
                info.display_name = label(entry, "displayName");
                info.input_tokens = entry.get("inputTokenLimit").and_then(positive);
                info.context_tokens = info.input_tokens;
                info.output_tokens = entry.get("outputTokenLimit").and_then(positive);
                info.supports_thinking = entry.get("thinking").and_then(Value::as_bool);
            }
            ProviderKind::Openai | ProviderKind::OpenaiResponses => {
                info.display_name = label(entry, "name");
                info.context_tokens = entry.get("context_length").and_then(positive);
                info.input_tokens = info.context_tokens;
                info.output_tokens = entry
                    .pointer("/top_provider/max_completion_tokens")
                    .and_then(positive);
                if let Some(parameters) =
                    entry.get("supported_parameters").and_then(Value::as_array)
                {
                    info.supports_tools =
                        Some(parameters.iter().any(|p| p.as_str() == Some("tools")));
                }
                if openrouter {
                    if let Some(pricing) = entry.get("pricing") {
                        let input = pricing
                            .get("prompt")
                            .and_then(Value::as_str)
                            .and_then(|v| crate::catalog::parse_usd_per_token(v).ok());
                        let output = pricing
                            .get("completion")
                            .and_then(Value::as_str)
                            .and_then(|v| crate::catalog::parse_usd_per_token(v).ok());
                        if let (Some(input), Some(output)) = (input, output) {
                            info.pricing = Some(Pricing {
                                input_micro_usd_per_mtok: input,
                                output_micro_usd_per_mtok: output,
                                cache_read_micro_usd_per_mtok: pricing
                                    .get("input_cache_read")
                                    .and_then(Value::as_str)
                                    .and_then(|v| crate::catalog::parse_usd_per_token(v).ok()),
                                cache_write_micro_usd_per_mtok: pricing
                                    .get("input_cache_write")
                                    .and_then(Value::as_str)
                                    .and_then(|v| crate::catalog::parse_usd_per_token(v).ok()),
                                origin: PriceOrigin::ProviderReported,
                            });
                        }
                    }
                }
            }
            ProviderKind::Ollama => {
                // Ollama /api/tags contains storage metadata, not an input
                // token limit. No inference from model file sizes/names.
            }
        }
        info.validate()?;
        models.push(info);
    }
    let cursor = match kind {
        ProviderKind::Anthropic if value.get("has_more").and_then(Value::as_bool) == Some(true) => {
            Some(
                value
                    .get("last_id")
                    .and_then(Value::as_str)
                    .ok_or(Error::Protocol("Anthropic last_id missing"))?
                    .to_owned(),
            )
        }
        ProviderKind::Gemini => value
            .get("nextPageToken")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .map(str::to_owned),
        _ => None,
    };
    Ok((models, cursor))
}
fn label(entry: &Value, key: &str) -> Option<String> {
    entry
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| s.len() <= 256 && !s.chars().any(char::is_control))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TokenField;
    fn profile(kind: ProviderKind, base_url: &str) -> Profile {
        Profile {
            kind,
            base_url: base_url.into(),
            model: None,
            models: vec![],
            api_key_env: None,
            oauth_connector: None,
            oauth_host_id: None,
            allow_insecure_http: false,
            token_field: TokenField::MaxTokens,
            max_tokens: None,
            temperature: None,
            reasoning_effort: None,
            think: None,
            ca_file: None,
            include_usage: false,
        }
    }
    #[test]
    fn discovery_urls_follow_provider_wire_roots() {
        for (kind, url, expected) in [
            (
                ProviderKind::Openai,
                "https://example.test/v1",
                "https://example.test/v1/models",
            ),
            (
                ProviderKind::Anthropic,
                "https://api.anthropic.com/v1",
                "https://api.anthropic.com/v1/models",
            ),
            (
                ProviderKind::Gemini,
                "https://generativelanguage.googleapis.com/v1beta",
                "https://generativelanguage.googleapis.com/v1beta/models",
            ),
            (
                ProviderKind::Ollama,
                "http://127.0.0.1:11434",
                "http://127.0.0.1:11434/api/tags",
            ),
        ] {
            assert_eq!(
                discovery_endpoint(&profile(kind, url)).unwrap().as_str(),
                expected
            );
        }
    }
    #[test]
    fn parses_provider_metadata_and_self_reported_rates_without_inventing_unknowns() {
        let (openai, _) = parse_page(
            ProviderKind::Openai,
            &serde_json::json!({"data":[{"id":"gpt-a"},{"id":"gpt-b"}]}),
            false,
        )
        .unwrap();
        assert_eq!(openai.len(), 2);
        assert_eq!(openai[0].pricing, None);
        assert_eq!(openai[0].context_tokens, None);
        let (anthropic, next) = parse_page(ProviderKind::Anthropic,
            &serde_json::json!({"data":[{"id":"claude-test","max_input_tokens":200000,
              "max_tokens":64000,"capabilities":{"thinking":{"supported":true}},"lifecycle":"active"}],
              "has_more":true,"last_id":"claude-test"}),false).unwrap();
        assert_eq!(next.as_deref(), Some("claude-test"));
        assert_eq!(anthropic[0].input_tokens, Some(200000));
        assert_eq!(anthropic[0].supports_thinking, Some(true));
        let (gemini,_) = parse_page(ProviderKind::Gemini,
            &serde_json::json!({"models":[
              {"name":"models/gemini-test","inputTokenLimit":1048576,
               "outputTokenLimit":64000,"supportedGenerationMethods":["generateContent"],"thinking":true},
              {"name":"models/embed","supportedGenerationMethods":["embedContent"]}]}),false).unwrap();
        assert_eq!(gemini.len(), 1);
        assert_eq!(gemini[0].output_tokens, Some(64000));
        let (ollama, _) = parse_page(
            ProviderKind::Ollama,
            &serde_json::json!({"models":[{"name":"qwen:cloud"},{"model":"glm:latest"}]}),
            false,
        )
        .unwrap();
        assert_eq!(ollama.len(), 2);
        assert!(ollama.iter().all(|m| m.context_tokens.is_none()));
        let entry = serde_json::json!({"data":[{"id":"openai/example",
          "context_length":123456,"top_provider":{"max_completion_tokens":5120},
          "supported_parameters":["tools"],
          "pricing":{"prompt":"0.000003","completion":"0.000015"}}]});
        let (third_party, _) = parse_page(ProviderKind::Openai, &entry, false).unwrap();
        assert_eq!(third_party[0].pricing, None);
        let (rated, _) = parse_page(ProviderKind::Openai, &entry, true).unwrap();
        assert_eq!(
            rated[0].pricing.as_ref().unwrap().input_micro_usd_per_mtok,
            3_000_000
        );
        assert_eq!(
            rated[0].pricing.as_ref().unwrap().output_micro_usd_per_mtok,
            15_000_000
        );
        assert_eq!(
            rated[0].pricing.as_ref().unwrap().origin,
            PriceOrigin::ProviderReported
        );
    }
}
