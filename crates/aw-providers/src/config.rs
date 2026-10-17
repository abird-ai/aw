use crate::{Error, Result};
use fs2::FileExt;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

pub const MAX_CONFIG_BYTES: usize = 128 * 1024;
pub use aw_agent_core::limits::{
    MAX_ATTEMPTS, MAX_CONTEXT_BYTES, MAX_OUTPUT_BYTES, MAX_PROMPT_BYTES,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    /// OpenAI-compatible Chat Completions wire protocol.
    Openai,
    /// OpenAI Responses-compatible wire protocol.
    OpenaiResponses,
    /// Native Anthropic Messages protocol, not OpenAI-compatible Chat.
    Anthropic,
    /// Native Google Gemini generateContent wire protocol.
    Gemini,
    Ollama,
}

impl ProviderKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::OpenaiResponses => "responses",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
            Self::Ollama => "ollama",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenField {
    #[default]
    MaxTokens,
    MaxCompletionTokens,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub kind: ProviderKind,
    /// OpenAI-compatible API prefix (often /v1); Ollama server root (no /api/chat).
    pub base_url: String,
    pub model: Option<String>,
    /// Additional explicitly allowed model identifiers for this endpoint/account.
    /// `model` remains the optional default for CLI shorthand compatibility.
    #[serde(default)]
    pub models: Vec<String>,
    pub api_key_env: Option<String>,
    /// Optional SecretStore-backed OAuth connector ID. This is metadata only;
    /// access/refresh/id token values never enter ordinary profile configuration.
    #[serde(default)]
    pub oauth_connector: Option<String>,
    /// Stable host identifier used only by supported OAuth providers that bind a
    /// registration to a local agent host (currently Sign in with ChatGPT).
    #[serde(default)]
    pub oauth_host_id: Option<String>,
    #[serde(default)]
    pub allow_insecure_http: bool,
    #[serde(default)]
    pub token_field: TokenField,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    /// Provider-neutral reasoning effort for compatible OpenAI-style APIs.
    /// "instant" is serialized as the API's "none" effort. Unsupported
    /// providers may conservatively collapse non-instant effort to think=true.
    #[serde(default)]
    pub reasoning_effort: Option<ReasoningEffort>,
    /// Ollama compatibility switch retained for profiles that explicitly use
    /// boolean thinking. Runtime /think takes precedence when set.
    pub think: Option<bool>,
    pub ca_file: Option<PathBuf>,
    /// OpenAI-compatible: request final usage, only on servers known to support it.
    #[serde(default)]
    pub include_usage: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StatusItem {
    Model,
    Profile,
    Provider,
    Effort,
    Directory,
    Agent,
    Branch,
    Tools,
    Session,
    SessionTps,
    State,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningEffort {
    Instant,
    Low,
    #[default]
    Medium,
    High,
    Xhigh,
    Max,
}

impl ReasoningEffort {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Instant => "instant",
            Self::Low => "low",
            Self::Medium => "med",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }

    pub const fn api_value(self) -> &'static str {
        match self {
            Self::Instant => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "instant" | "none" => Self::Instant,
            "low" => Self::Low,
            "med" | "medium" => Self::Medium,
            "high" => Self::High,
            "xhigh" | "x-high" => Self::Xhigh,
            "max" => Self::Max,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StatusRegion {
    AboveComposer,
    BelowComposer,
    FullscreenStatus,
    /// Version-driven semantic regions; render only when a host allocates
    /// layout space explicitly. No new default pane/header/overlay.
    LeftStatus,
    RightStatus,
    BelowStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusLineConfig {
    pub region: StatusRegion,
    #[serde(default)]
    pub items: Vec<StatusItem>,
    /// Optional right-aligned group. Empty preserves the historical flat row.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub right_items: Vec<StatusItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TuiMode {
    Scrollback,
    Inline,
    Fullscreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UiConfig {
    /// Ordered status rows owned entirely by the built-in status-line extension.
    /// Rows may project above/below the inline composer or into fullscreen status.
    pub status_lines: Vec<StatusLineConfig>,
    pub tui_mode: TuiMode,
    pub theme: ThemeMode,
    /// Optional user-approved semantic RGB accent; absent preserves the
    /// exact historical System/Light/Dark palette in every feature cut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme_accent: Option<String>,
    /// Horizontal padding for the standalone Agent composer.
    pub editor_padding_x: u8,
    /// Horizontal padding for the standalone Agent transcript.
    pub output_padding: u8,
    /// Horizontal padding for the embedded Workspace Agent composer.
    pub workspace_editor_padding_x: u8,
    /// Horizontal padding for Workspace content/transcript views.
    pub workspace_output_padding: u8,
    pub autocomplete_max_visible: u8,
    pub show_performance: bool,
    pub show_ttft: bool,
    pub show_waterfall: bool,
    pub show_thinking: bool,
    pub show_tool_calls: bool,
    /// P6-8C: optional presentation-only factual Step/Turn/FinalOutput
    /// notices. No new header/sidebar, provider/tool event or persisted log.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub show_event_notices: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UiConfigWire {
    #[serde(default)]
    status_lines: Option<Vec<StatusLineConfig>>,
    /// Legacy Phase-34/37 shorthand. Accepted on read only and normalized into
    /// one below-composer status row; new saves emit status_lines.
    #[serde(default)]
    status_line: Option<Vec<StatusItem>>,
    #[serde(default)]
    tui_mode: Option<TuiMode>,
    #[serde(default)]
    theme: Option<ThemeMode>,
    #[serde(default)]
    theme_accent: Option<String>,
    #[serde(default)]
    editor_padding_x: Option<u8>,
    #[serde(default)]
    output_padding: Option<u8>,
    #[serde(default)]
    workspace_editor_padding_x: Option<u8>,
    #[serde(default)]
    workspace_output_padding: Option<u8>,
    #[serde(default)]
    autocomplete_max_visible: Option<u8>,
    #[serde(default)]
    show_performance: Option<bool>,
    #[serde(default)]
    show_ttft: Option<bool>,
    #[serde(default)]
    show_waterfall: Option<bool>,
    #[serde(default)]
    show_thinking: Option<bool>,
    #[serde(default)]
    show_tool_calls: Option<bool>,
    #[serde(default)]
    show_event_notices: Option<bool>,
}

fn flat_status_line(region: StatusRegion, items: Vec<StatusItem>) -> StatusLineConfig {
    StatusLineConfig {
        region,
        items,
        right_items: Vec::new(),
    }
}

fn legacy_agent_status_lines() -> Vec<StatusLineConfig> {
    vec![
        flat_status_line(
            StatusRegion::BelowComposer,
            vec![StatusItem::Directory, StatusItem::Branch],
        ),
        flat_status_line(
            StatusRegion::BelowComposer,
            vec![StatusItem::Model, StatusItem::Profile, StatusItem::State],
        ),
        flat_status_line(
            StatusRegion::BelowComposer,
            vec![StatusItem::Tools, StatusItem::Session],
        ),
        flat_status_line(
            StatusRegion::FullscreenStatus,
            vec![
                StatusItem::Model,
                StatusItem::Directory,
                StatusItem::Branch,
                StatusItem::Profile,
                StatusItem::Tools,
            ],
        ),
    ]
}

/// Exact Phase-57/58 default that may have been persisted by Settings.
/// Only this exact shape migrates; arbitrary user layouts remain untouched.
fn previous_default_status_lines() -> Vec<StatusLineConfig> {
    vec![
        flat_status_line(
            StatusRegion::AboveComposer,
            vec![
                StatusItem::Model,
                StatusItem::Directory,
                StatusItem::Branch,
                StatusItem::Profile,
                StatusItem::Tools,
                StatusItem::State,
            ],
        ),
        flat_status_line(
            StatusRegion::FullscreenStatus,
            vec![
                StatusItem::Model,
                StatusItem::Directory,
                StatusItem::Branch,
                StatusItem::Profile,
                StatusItem::Tools,
            ],
        ),
    ]
}

fn default_status_lines() -> Vec<StatusLineConfig> {
    let first = StatusLineConfig {
        region: StatusRegion::BelowComposer,
        items: vec![StatusItem::Directory, StatusItem::Agent],
        right_items: vec![StatusItem::Tools, StatusItem::State],
    };
    let second = StatusLineConfig {
        region: StatusRegion::BelowComposer,
        items: vec![StatusItem::SessionTps],
        right_items: vec![StatusItem::Model, StatusItem::Provider, StatusItem::Effort],
    };
    vec![
        first.clone(),
        second.clone(),
        StatusLineConfig {
            region: StatusRegion::FullscreenStatus,
            ..first
        },
        StatusLineConfig {
            region: StatusRegion::FullscreenStatus,
            ..second
        },
    ]
}

impl<'de> Deserialize<'de> for UiConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = UiConfigWire::deserialize(deserializer)?;
        if wire.status_lines.is_some() && wire.status_line.is_some() {
            return Err(serde::de::Error::custom(
                "use status_lines or legacy status_line, not both",
            ));
        }
        let defaults = Self::default();
        let status_lines = match (wire.status_lines, wire.status_line) {
            (Some(lines), None)
                if lines == legacy_agent_status_lines()
                    || lines == previous_default_status_lines() =>
            {
                defaults.status_lines.clone()
            }
            (Some(lines), None) => lines,
            (None, Some(items)) => vec![StatusLineConfig {
                region: StatusRegion::AboveComposer,
                items,
                right_items: Vec::new(),
            }],
            (None, None) => defaults.status_lines.clone(),
            (Some(_), Some(_)) => unreachable!("validated above"),
        };
        Ok(Self {
            status_lines,
            tui_mode: wire.tui_mode.unwrap_or(defaults.tui_mode),
            theme: wire.theme.unwrap_or(defaults.theme),
            theme_accent: wire.theme_accent,
            editor_padding_x: wire.editor_padding_x.unwrap_or(defaults.editor_padding_x),
            output_padding: wire.output_padding.unwrap_or(defaults.output_padding),
            workspace_editor_padding_x: wire
                .workspace_editor_padding_x
                .unwrap_or(defaults.workspace_editor_padding_x),
            workspace_output_padding: wire
                .workspace_output_padding
                .unwrap_or(defaults.workspace_output_padding),
            autocomplete_max_visible: wire
                .autocomplete_max_visible
                .unwrap_or(defaults.autocomplete_max_visible),
            show_performance: wire.show_performance.unwrap_or(defaults.show_performance),
            show_ttft: wire.show_ttft.unwrap_or(defaults.show_ttft),
            show_waterfall: wire.show_waterfall.unwrap_or(defaults.show_waterfall),
            show_thinking: wire.show_thinking.unwrap_or(defaults.show_thinking),
            show_tool_calls: wire.show_tool_calls.unwrap_or(defaults.show_tool_calls),
            show_event_notices: wire.show_event_notices.unwrap_or(false),
        })
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            // Agent modes keep persistent status above the composer so the
            // prompt is the final default active row group. Fullscreen keeps
            // its dedicated compact status projection.
            status_lines: default_status_lines(),
            tui_mode: TuiMode::Inline,
            theme: ThemeMode::System,
            theme_accent: None,
            editor_padding_x: 0,
            output_padding: 0,
            // Workspace owns pane chrome, so one cell of breathing room keeps
            // content minimal without visually touching pane borders.
            workspace_editor_padding_x: 1,
            workspace_output_padding: 1,
            autocomplete_max_visible: 5,
            show_performance: true,
            show_ttft: false,
            show_waterfall: false,
            show_thinking: true,
            show_tool_calls: true,
            show_event_notices: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProfileUpdate {
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub default_model: Option<String>,
    pub models: Vec<String>,
    pub api_key_env: Option<String>,
    pub oauth_connector: Option<String>,
    pub oauth_host_id: Option<String>,
    pub allow_insecure_http: bool,
    pub token_field: TokenField,
    pub include_usage: bool,
    pub make_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillConfig {
    pub path: PathBuf,
    #[serde(default)]
    pub description: Option<String>,
}

/// User-owned, source-pinned project data approval. These are not provider,
/// tool, filesystem, network or plugin authorizations; every file is verified
/// again at admission and an edit requires explicit reapproval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectResourceTrust {
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    pub default_profile: Option<String>,
    pub system_prompt: Option<String>,
    /// Stable per-installation host identity used by the supported ChatGPT
    /// dynamic-registration flow. This is not a credential or user identity.
    #[serde(default)]
    pub chatgpt_host_id: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
    #[serde(default)]
    pub skills: BTreeMap<String, SkillConfig>,
    /// Explicitly approved canonical project roots and bounded source hashes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub project_resource_trust: BTreeMap<String, ProjectResourceTrust>,
    /// AW-P6-8A: INERT, non-secret values for currently registered extension
    /// settings. Validation against a compiled/trusted manifest is required
    /// before use. No provider/tool/credential authority in config data.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extension_settings: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub ui: UiConfig,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub profile_name: String,
    pub profile: Profile,
    pub system_prompt: String,
    pub request_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_output_bytes: usize,
}

/// Canonical provider destination/auth-reference identity, shared by the full
/// SQLite owner and lightweight JSONL Agent. No secret value enters this hash.
/// A changed value behind the same env/SecretStore reference still needs
/// explicit operator judgment; the digest cannot attest secret custody.
pub fn destination_fingerprint(profile: &Profile) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&(
                format!("{:?}", profile.kind),
                &profile.base_url,
                &profile.api_key_env,
                &profile.oauth_connector,
                &profile.oauth_host_id,
                &profile.ca_file
            ))
            .map_err(|_| Error::Config("invalid destination config".into()))?
        )
    ))
}

pub fn builtins() -> BTreeMap<String, Profile> {
    let mk = |kind, url: &str, key: Option<&str>, token_field| Profile {
        kind,
        base_url: url.into(),
        model: None,
        models: Vec::new(),
        api_key_env: key.map(str::to_owned),
        oauth_connector: None,
        oauth_host_id: None,
        allow_insecure_http: false,
        token_field,
        max_tokens: None,
        temperature: None,
        reasoning_effort: None,
        think: None,
        ca_file: None,
        include_usage: key.is_some(),
    };
    BTreeMap::from([
        (
            "ollama".into(),
            mk(
                ProviderKind::Ollama,
                "http://127.0.0.1:11434",
                None,
                TokenField::MaxTokens,
            ),
        ),
        (
            "llama".into(),
            mk(
                ProviderKind::Openai,
                "http://127.0.0.1:8080/v1",
                None,
                TokenField::MaxTokens,
            ),
        ),
        (
            "openai".into(),
            mk(
                ProviderKind::Openai,
                "https://api.openai.com/v1",
                Some("OPENAI_API_KEY"),
                TokenField::MaxCompletionTokens,
            ),
        ),
        (
            "anthropic".into(),
            mk(
                ProviderKind::Anthropic,
                "https://api.anthropic.com/v1",
                Some("ANTHROPIC_API_KEY"),
                TokenField::MaxTokens,
            ),
        ),
        (
            "gemini".into(),
            mk(
                ProviderKind::Gemini,
                "https://generativelanguage.googleapis.com/v1beta",
                Some("GEMINI_API_KEY"),
                TokenField::MaxTokens,
            ),
        ),
        (
            "openrouter".into(),
            mk(
                ProviderKind::Openai,
                "https://openrouter.ai/api/v1",
                Some("OPENROUTER_API_KEY"),
                TokenField::MaxTokens,
            ),
        ),
        (
            "xai".into(),
            mk(
                ProviderKind::Openai,
                "https://api.x.ai/v1",
                Some("XAI_API_KEY"),
                TokenField::MaxCompletionTokens,
            ),
        ),
        (
            "deepseek".into(),
            mk(
                ProviderKind::Openai,
                "https://api.deepseek.com/v1",
                Some("DEEPSEEK_API_KEY"),
                TokenField::MaxTokens,
            ),
        ),
        (
            "groq".into(),
            mk(
                ProviderKind::Openai,
                "https://api.groq.com/openai/v1",
                Some("GROQ_API_KEY"),
                TokenField::MaxTokens,
            ),
        ),
        (
            "mistral".into(),
            mk(
                ProviderKind::Openai,
                "https://api.mistral.ai/v1",
                Some("MISTRAL_API_KEY"),
                TokenField::MaxTokens,
            ),
        ),
        (
            "together".into(),
            mk(
                ProviderKind::Openai,
                "https://api.together.xyz/v1",
                Some("TOGETHER_API_KEY"),
                TokenField::MaxTokens,
            ),
        ),
        (
            "openai-responses".into(),
            mk(
                ProviderKind::OpenaiResponses,
                "https://api.openai.com/v1",
                Some("OPENAI_API_KEY"),
                TokenField::MaxTokens,
            ),
        ),
    ])
}

pub fn default_config_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            return Some(path.join("abird/aw/config.toml"));
        }
    }
    std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config/abird/aw/config.toml"))
}

/// Never auto-load a repository-local config. It could select a new endpoint.
pub fn load(path: Option<&Path>) -> Result<FileConfig> {
    use std::io::Read;
    let (path, explicit) = match path {
        Some(path) => (path.to_owned(), true),
        None => match default_config_path() {
            Some(path) => (path, false),
            None => return Ok(FileConfig::default()),
        },
    };
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) if !explicit && e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FileConfig::default())
        }
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    file.take((MAX_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(Error::Limit("configuration exceeds 128 KiB"));
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| Error::Config("configuration must be UTF-8".into()))?;
    parse(text, path.parent())
}

pub fn validate_skill_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name.bytes().enumerate().all(|(index, byte)| {
            if index == 0 {
                byte.is_ascii_alphanumeric()
            } else {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
            }
        })
    {
        return Err(Error::Config(
            "skill names must be portable 1..64 byte labels starting with a letter or digit".into(),
        ));
    }
    Ok(())
}

/// Stable digest of an approved resource's exact UTF-8 bytes.
pub fn project_resource_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn validate_project_resource_trust(
    trusted: &BTreeMap<String, ProjectResourceTrust>,
) -> Result<()> {
    if trusted.len() > 32 {
        return Err(Error::Limit("too many approved project resource roots"));
    }
    for (root, approval) in trusted {
        if root.len() > 4096
            || !Path::new(root).is_absolute()
            || approval.files.is_empty()
            || approval.files.len() > 96
        {
            return Err(Error::Config(
                "invalid approved project root/source count".into(),
            ));
        }
        for (name, sha) in &approval.files {
            if name.len() > 2048
                || name.is_empty()
                || name.split('/').any(|c| {
                    c.is_empty()
                        || c == "."
                        || c == ".."
                        || c.chars().any(char::is_control)
                        || c.contains('\\')
                })
                || sha.len() != 64
                || !sha
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(Error::Config("invalid project source hash/path".into()));
            }
        }
    }
    Ok(())
}

/// Structural safety bound independently of extension registration. A config
/// file must not encode bearer/secret values under a new arbitrary type, nor
/// bypass strict manifest validation when that extension is actually loaded.
/// At most 64 extensions x 32 keys and 256 overall; no recursive JSON.
pub fn validate_extension_settings(
    settings: &BTreeMap<String, BTreeMap<String, serde_json::Value>>,
) -> Result<()> {
    if settings.len() > 64 {
        return Err(Error::Limit("too many configured extensions"));
    }
    let mut total = 0usize;
    for (ext, entries) in settings {
        validate_skill_name(ext)?;
        if entries.is_empty() || entries.len() > 32 {
            return Err(Error::Limit("extension settings must contain 1..32 keys"));
        }
        total += entries.len();
        if total > 256 {
            return Err(Error::Limit("more than 256 extension settings"));
        }
        for (name, value) in entries {
            validate_skill_name(name)?;
            let obj = value
                .as_object()
                .ok_or(Error::Denied("extension value must be typed"))?;
            if obj.len() != 2 || !obj.contains_key("type") || !obj.contains_key("value") {
                return Err(Error::Denied("extension value has extra or missing fields"));
            }
            let valid = match (
                obj.get("type").and_then(serde_json::Value::as_str),
                obj.get("value"),
            ) {
                (Some("boolean"), Some(v)) => v.is_boolean(),
                (Some("integer"), Some(v)) => v.as_i64().is_some(),
                (Some("text"), Some(v)) => v
                    .as_str()
                    .is_some_and(|s| s.len() <= 2048 && !s.chars().any(char::is_control)),
                _ => false,
            };
            if !valid {
                return Err(Error::Denied("invalid non-secret extension setting"));
            }
        }
    }
    Ok(())
}

pub fn validate_profile_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b":._-@".contains(&c))
    {
        return Err(Error::Config(
            "profile names must be portable 1..128 byte labels".into(),
        ));
    }
    Ok(())
}

pub fn parse(text: &str, relative_to: Option<&Path>) -> Result<FileConfig> {
    // The TOML error's Display may quote config values, so do not print it.
    let mut cfg: FileConfig = toml::from_str(text).map_err(|_| {
        Error::Config(
            "invalid TOML or unknown configuration field; see examples/config.toml".into(),
        )
    })?;
    for (name, p) in &mut cfg.profiles {
        validate_profile_name(name)?;
        if let (Some(base), Some(ca)) = (relative_to, &p.ca_file) {
            if ca.is_relative() {
                p.ca_file = Some(base.join(ca));
            }
        }
        p.validate()?;
    }
    if cfg.skills.len() > 128 {
        return Err(Error::Limit(
            "configuration allows at most 128 registered skills",
        ));
    }
    for (name, skill) in &mut cfg.skills {
        validate_skill_name(name)?;
        if skill.path.as_os_str().is_empty() {
            return Err(Error::Config(
                "registered skill path may not be empty".into(),
            ));
        }
        if let Some(description) = &skill.description {
            if description.len() > 512 || description.chars().any(char::is_control) {
                return Err(Error::Config(
                    "skill description must be at most 512 bytes without control characters".into(),
                ));
            }
        }
        if let Some(base) = relative_to {
            if skill.path.is_relative() {
                skill.path = base.join(&skill.path);
            }
        }
    }
    validate_project_resource_trust(&cfg.project_resource_trust)?;
    validate_extension_settings(&cfg.extension_settings)?;
    if let Some(default_profile) = &cfg.default_profile {
        validate_profile_name(default_profile)?;
    }
    if let Some(host_id) = &cfg.chatgpt_host_id {
        validate_oauth_host_id(host_id)?;
    }
    if let Some(system) = &cfg.system_prompt {
        if system.len() > MAX_PROMPT_BYTES {
            return Err(Error::Limit("system prompt too large"));
        }
    }
    cfg.ui.validate()?;
    Ok(cfg)
}

impl UiConfig {
    pub fn validate(&self) -> Result<()> {
        if self.theme_accent.as_ref().is_some_and(|accent| {
            accent.len() != 7
                || !accent.starts_with('#')
                || !accent.bytes().skip(1).all(|b| b.is_ascii_hexdigit())
        }) {
            return Err(Error::Config(
                "theme_accent must be #RRGGBB or omitted".into(),
            ));
        }
        if self.status_lines.len() > 32 {
            return Err(Error::Limit("status_lines allows at most 32 rows"));
        }
        for line in &self.status_lines {
            if line.items.is_empty() && line.right_items.is_empty() {
                return Err(Error::Config("status_lines rows may not be empty".into()));
            }
            if line.items.len().saturating_add(line.right_items.len()) > 16 {
                return Err(Error::Limit("each status line allows at most 16 items"));
            }
            let mut status_items = std::collections::BTreeSet::new();
            for item in line.items.iter().chain(&line.right_items) {
                if !status_items.insert(*item) {
                    return Err(Error::Config(
                        "a status_lines row contains duplicate items".into(),
                    ));
                }
            }
        }
        if self.editor_padding_x > 3 {
            return Err(Error::Config("editor_padding_x must be 0..=3".into()));
        }
        if self.output_padding > 3 {
            return Err(Error::Config("output_padding must be 0..=3".into()));
        }
        if self.workspace_editor_padding_x > 3 {
            return Err(Error::Config(
                "workspace_editor_padding_x must be 0..=3".into(),
            ));
        }
        if self.workspace_output_padding > 3 {
            return Err(Error::Config(
                "workspace_output_padding must be 0..=3".into(),
            ));
        }
        if !(3..=20).contains(&self.autocomplete_max_visible) {
            return Err(Error::Config(
                "autocomplete_max_visible must be 3..=20".into(),
            ));
        }
        Ok(())
    }
}

impl Profile {
    pub fn validate(&self) -> Result<()> {
        self.endpoint()?;
        if let Some(model) = &self.model {
            validate_model(model)?;
        }
        if self.models.len() > 128 {
            return Err(Error::Limit("profile allows at most 128 models"));
        }
        let mut unique = std::collections::BTreeSet::new();
        for model in &self.models {
            validate_model(model)?;
            if !unique.insert(model) {
                return Err(Error::Config(
                    "profile model list contains duplicates".into(),
                ));
            }
        }
        if let Some(env) = &self.api_key_env {
            if env.is_empty()
                || !env.bytes().enumerate().all(|(i, b)| {
                    b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit())
                })
            {
                return Err(Error::Config(
                    "api_key_env must be an environment-variable name, not a token".into(),
                ));
            }
        }
        if let Some(connector) = &self.oauth_connector {
            validate_profile_name(connector)?;
        }
        if self.api_key_env.is_some() && self.oauth_connector.is_some() {
            return Err(Error::Config(
                "a profile cannot select both api_key_env and oauth_connector".into(),
            ));
        }
        if self.oauth_connector.is_some() && self.kind != ProviderKind::OpenaiResponses {
            return Err(Error::Config(
                "OAuth-backed inference currently requires the Responses provider adapter".into(),
            ));
        }
        if self.oauth_connector.is_some() != self.oauth_host_id.is_some() {
            return Err(Error::Config(
                "oauth_connector and oauth_host_id must be configured together".into(),
            ));
        }
        if let Some(host_id) = &self.oauth_host_id {
            validate_oauth_host_id(host_id)?;
        }
        if self.max_tokens == Some(0) {
            return Err(Error::Config("max_tokens must be positive".into()));
        }
        if self
            .temperature
            .is_some_and(|v| !v.is_finite() || !(0.0..=2.0).contains(&v))
        {
            return Err(Error::Config(
                "temperature must be finite and between 0 and 2".into(),
            ));
        }
        if self.kind != ProviderKind::Ollama && self.think.is_some() {
            return Err(Error::Config(
                "think is supported only by the native Ollama adapter".into(),
            ));
        }
        if self.think.is_some() && self.reasoning_effort.is_some() {
            return Err(Error::Config(
                "configure either reasoning_effort or the legacy Ollama think flag, not both"
                    .into(),
            ));
        }
        if self.kind == ProviderKind::Gemini {
            if let Some(effort) = self.reasoning_effort {
                if matches!(effort, ReasoningEffort::Xhigh | ReasoningEffort::Max) {
                    return Err(Error::Config(
                        "Gemini supports only explicitly native low/medium/high thinking; xhigh/max have no exact equivalent".into(),
                    ));
                }
            }
        }
        if self.kind == ProviderKind::Ollama
            && self.reasoning_effort.is_some_and(|effort| {
                !matches!(effort, ReasoningEffort::Instant | ReasoningEffort::High)
            })
        {
            return Err(Error::Config(
                "native Ollama reasoning is boolean; use reasoning_effort='instant' or 'high'"
                    .into(),
            ));
        }
        Ok(())
    }

    pub fn endpoint(&self) -> Result<Url> {
        let mut url = Url::parse(&self.base_url)
            .map_err(|_| Error::Config("invalid provider base_url".into()))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::Config(
                "base_url must be HTTP(S), without userinfo, query or fragment".into(),
            ));
        }
        let host = url.host_str().unwrap_or("");
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let local = host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|v| v.is_loopback());
        if url.scheme() == "http" && !local && !self.allow_insecure_http {
            return Err(Error::Config(
                "non-loopback HTTP needs --allow-insecure-http (traffic is unencrypted)".into(),
            ));
        }
        let prefix = url.path().trim_end_matches('/');
        if prefix.ends_with("/chat/completions")
            || prefix.ends_with("/responses")
            || prefix.ends_with("/messages")
            || prefix.ends_with("/models")
            || prefix.ends_with("/api/chat")
        {
            return Err(Error::Config(
                "base_url must be the API prefix/server root, not the final inference endpoint"
                    .into(),
            ));
        }
        let suffix = match self.kind {
            ProviderKind::Openai => "chat/completions",
            ProviderKind::OpenaiResponses => "responses",
            ProviderKind::Anthropic => "messages",
            // This is only a validated API-root placeholder: inference needs
            // the selected model in its URL and uses inference_endpoint().
            ProviderKind::Gemini => "models",
            ProviderKind::Ollama => "api/chat",
        };
        let path = format!("{prefix}/{suffix}");
        url.set_path(&path);
        Ok(url)
    }
}

impl Profile {
    /// Selected-model endpoint for native APIs whose model is part of the URL.
    /// Keep model IDs as one URL segment, never accept arbitrary relative paths
    /// or a user-controlled query component.
    pub fn inference_endpoint(&self, model: &str) -> Result<Url> {
        let mut url = self.endpoint()?;
        if self.kind != ProviderKind::Gemini {
            return Ok(url);
        }
        let name = model.strip_prefix("models/").unwrap_or(model);
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.len() > 128
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(Error::Config(
                "Gemini model must be a bounded model ID, not a URL/path".into(),
            ));
        }
        let prefix = url.path().trim_end_matches("/models");
        url.set_path(&format!("{prefix}/models/{name}:streamGenerateContent"));
        url.query_pairs_mut().append_pair("alt", "sse");
        Ok(url)
    }
}

pub fn validate_oauth_host_id(host_id: &str) -> Result<()> {
    let uuid = host_id
        .strip_prefix("urn:uuid:")
        .ok_or_else(|| Error::Config("oauth_host_id must use urn:uuid:<uuid>".into()))?;
    if uuid.len() != 36
        || !uuid.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
    {
        return Err(Error::Config(
            "oauth_host_id must be a canonical hyphenated UUID URN".into(),
        ));
    }
    Ok(())
}

pub fn validate_model(model: &str) -> Result<()> {
    if model.trim().is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
        return Err(Error::Config(
            "model must be a nonempty, printable identifier of at most 256 bytes".into(),
        ));
    }
    Ok(())
}

impl Profile {
    pub fn effective_models(&self) -> std::collections::BTreeSet<String> {
        let mut out = self
            .models
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        if let Some(model) = &self.model {
            out.insert(model.clone());
        }
        out
    }
}

pub fn apply_profile_update(
    current: &FileConfig,
    update: ProfileUpdate,
) -> Result<(FileConfig, Profile)> {
    if update.name == "__unconfigured" {
        return Err(Error::Config("reserved profile name".into()));
    }
    validate_profile_name(&update.name)?;
    let profile = Profile {
        kind: update.kind,
        base_url: update.base_url,
        model: update
            .default_model
            .filter(|value| !value.trim().is_empty()),
        models: update
            .models
            .into_iter()
            .filter(|value| !value.trim().is_empty())
            .collect(),
        api_key_env: update.api_key_env.filter(|value| !value.trim().is_empty()),
        oauth_connector: update
            .oauth_connector
            .filter(|value| !value.trim().is_empty()),
        oauth_host_id: update
            .oauth_host_id
            .filter(|value| !value.trim().is_empty()),
        allow_insecure_http: update.allow_insecure_http,
        token_field: update.token_field,
        max_tokens: None,
        temperature: None,
        reasoning_effort: None,
        think: None,
        ca_file: None,
        include_usage: update.include_usage,
    };
    profile.validate()?;
    if update.make_default && profile.model.is_none() {
        return Err(Error::Config(
            "default profile needs an explicit default model".into(),
        ));
    }
    let mut next = current.clone();
    next.profiles.insert(update.name.clone(), profile.clone());
    if update.make_default {
        next.default_profile = Some(update.name);
    }
    Ok((next, profile))
}

/// Resolve the one canonical path used for configuration mutation.
pub fn resolved_config_path(path: Option<&Path>) -> Result<PathBuf> {
    let path = match path {
        Some(path) => path.to_owned(),
        None => default_config_path().ok_or_else(|| {
            Error::Config("HOME/XDG_CONFIG_HOME missing; cannot persist settings".into())
        })?,
    };
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn metadata_lock_path(path: &Path) -> Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| Error::Config("configuration path has no filename".into()))?;
    let mut lock_name = OsString::from(name);
    lock_name.push(".lock");
    Ok(path.with_file_name(lock_name))
}

fn save_unlocked(path: &Path, config: &FileConfig) -> Result<()> {
    let body = toml::to_string_pretty(config)
        .map_err(|_| Error::Config("cannot serialize configuration".into()))?;
    if body.len() > MAX_CONFIG_BYTES {
        return Err(Error::Limit("configuration exceeds 128 KiB"));
    }
    aw_platform::atomic_write_user_metadata(path, body.as_bytes())?;
    Ok(())
}

fn load_for_mutation(path: &Path) -> Result<FileConfig> {
    if path.exists() {
        load(Some(path))
    } else {
        Ok(FileConfig::default())
    }
}

/// Execute one read-modify-write mutation while holding a stable advisory lock
/// next to the metadata file. The closure always sees the latest committed file.
pub fn mutate<T>(
    path: Option<&Path>,
    change: impl FnOnce(&mut FileConfig) -> Result<T>,
) -> Result<(FileConfig, T)> {
    let path = resolved_config_path(path)?;
    let lock_path = metadata_lock_path(&path)?;
    let lock = aw_platform::open_user_metadata_lock(&lock_path)?;
    lock.lock_exclusive()?;
    let mut current = load_for_mutation(&path)?;
    let value = change(&mut current)?;
    save_unlocked(&path, &current)?;
    // Drop releases the advisory lock. Avoid an explicit unlock call so an
    // unlock error cannot obscure a successfully fsynced metadata publication.
    drop(lock);
    Ok((current, value))
}

fn apply_changed_map<K, V>(
    current: &mut BTreeMap<K, V>,
    before: &BTreeMap<K, V>,
    after: &BTreeMap<K, V>,
    conflict: &'static str,
) -> Result<()>
where
    K: Ord + Clone,
    V: Clone + PartialEq,
{
    let keys = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for key in keys {
        let old = before.get(&key);
        let desired = after.get(&key);
        if old == desired {
            continue;
        }
        let latest = current.get(&key);
        if latest != old && latest != desired {
            return Err(Error::Conflict(conflict));
        }
        match desired {
            Some(value) => {
                current.insert(key, value.clone());
            }
            None => {
                current.remove(&key);
            }
        }
    }
    Ok(())
}

/// Merge only the changed setting *keys*, not entire extension groups.
/// Unrelated concurrent setting/profile/status edits remain intact and
/// same-key stale writes fail closed under the existing config file lock.
fn apply_changed_extension_settings(
    current: &mut BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    before: &BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    after: &BTreeMap<String, BTreeMap<String, serde_json::Value>>,
) -> Result<()> {
    let names = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for extension in names {
        let empty = BTreeMap::new();
        let old = before.get(&extension).unwrap_or(&empty);
        let wanted = after.get(&extension).unwrap_or(&empty);
        if old == wanted {
            continue;
        }
        let mut live = current.get(&extension).cloned().unwrap_or_default();
        apply_changed_map(
            &mut live,
            old,
            wanted,
            "extension setting changed concurrently; refresh and retry",
        )?;
        if live.is_empty() {
            current.remove(&extension);
        } else {
            current.insert(extension, live);
        }
    }
    validate_extension_settings(current)
}

fn apply_changed_value<T: Clone + PartialEq>(
    current: &mut T,
    before: &T,
    after: &T,
    conflict: &'static str,
) -> Result<()> {
    if before == after {
        return Ok(());
    }
    if current != before && current != after {
        return Err(Error::Conflict(conflict));
    }
    *current = after.clone();
    Ok(())
}

/// Merge only fields changed by this caller into the latest disk config.
/// This prevents an older long-lived UI snapshot from erasing independent
/// profile/skill/config mutations made by another aw process.
pub fn save_changes(
    path: Option<&Path>,
    before: &FileConfig,
    after: &FileConfig,
) -> Result<FileConfig> {
    let (committed, ()) = mutate(path, |current| {
        apply_changed_value(
            &mut current.default_profile,
            &before.default_profile,
            &after.default_profile,
            "configuration default profile changed concurrently; reload and retry",
        )?;
        apply_changed_value(
            &mut current.system_prompt,
            &before.system_prompt,
            &after.system_prompt,
            "configuration system prompt changed concurrently; reload and retry",
        )?;
        apply_changed_value(
            &mut current.chatgpt_host_id,
            &before.chatgpt_host_id,
            &after.chatgpt_host_id,
            "configuration OAuth host identity changed concurrently; reload and retry",
        )?;
        apply_changed_map(
            &mut current.profiles,
            &before.profiles,
            &after.profiles,
            "configuration profile changed concurrently; reload and retry",
        )?;
        apply_changed_map(
            &mut current.skills,
            &before.skills,
            &after.skills,
            "configuration skill changed concurrently; reload and retry",
        )?;
        apply_changed_extension_settings(
            &mut current.extension_settings,
            &before.extension_settings,
            &after.extension_settings,
        )?;
        apply_changed_map(
            &mut current.project_resource_trust,
            &before.project_resource_trust,
            &after.project_resource_trust,
            "project trust changed concurrently; reload and explicitly reapprove",
        )?;
        apply_changed_value(
            &mut current.ui.status_lines,
            &before.ui.status_lines,
            &after.ui.status_lines,
            "configuration UI status_lines changed concurrently; reload and retry",
        )?;
        apply_changed_value(
            &mut current.ui.tui_mode,
            &before.ui.tui_mode,
            &after.ui.tui_mode,
            "configuration UI tui_mode changed concurrently; reload and retry",
        )?;
        apply_changed_value(
            &mut current.ui.theme,
            &before.ui.theme,
            &after.ui.theme,
            "configuration UI theme changed concurrently; reload and retry",
        )?;
        apply_changed_value(
            &mut current.ui.theme_accent,
            &before.ui.theme_accent,
            &after.ui.theme_accent,
            "configuration UI accent changed concurrently; reload and retry",
        )?;
        apply_changed_value(
            &mut current.ui.editor_padding_x,
            &before.ui.editor_padding_x,
            &after.ui.editor_padding_x,
            "configuration UI editor_padding_x changed concurrently; reload and retry",
        )?;
        apply_changed_value(
            &mut current.ui.output_padding,
            &before.ui.output_padding,
            &after.ui.output_padding,
            "configuration UI output_padding changed concurrently; reload and retry",
        )?;
        apply_changed_value(
            &mut current.ui.workspace_editor_padding_x,
            &before.ui.workspace_editor_padding_x,
            &after.ui.workspace_editor_padding_x,
            "configuration UI workspace_editor_padding_x changed concurrently; reload and retry",
        )?;
        apply_changed_value(
            &mut current.ui.workspace_output_padding,
            &before.ui.workspace_output_padding,
            &after.ui.workspace_output_padding,
            "configuration UI workspace_output_padding changed concurrently; reload and retry",
        )?;
        apply_changed_value(
            &mut current.ui.autocomplete_max_visible,
            &before.ui.autocomplete_max_visible,
            &after.ui.autocomplete_max_visible,
            "configuration UI autocomplete_max_visible changed concurrently; reload and retry",
        )?;
        apply_changed_value(
            &mut current.ui.show_performance,
            &before.ui.show_performance,
            &after.ui.show_performance,
            "configuration UI show_performance changed concurrently; reload and retry",
        )?;
        apply_changed_value(
            &mut current.ui.show_ttft,
            &before.ui.show_ttft,
            &after.ui.show_ttft,
            "configuration UI show_ttft changed concurrently; reload and retry",
        )?;
        apply_changed_value(
            &mut current.ui.show_waterfall,
            &before.ui.show_waterfall,
            &after.ui.show_waterfall,
            "configuration UI show_waterfall changed concurrently; reload and retry",
        )?;
        apply_changed_value(
            &mut current.ui.show_thinking,
            &before.ui.show_thinking,
            &after.ui.show_thinking,
            "configuration UI show_thinking changed concurrently; reload and retry",
        )?;
        apply_changed_value(
            &mut current.ui.show_tool_calls,
            &before.ui.show_tool_calls,
            &after.ui.show_tool_calls,
            "configuration UI show_tool_calls changed concurrently; reload and retry",
        )?;
        apply_changed_value(
            &mut current.ui.show_event_notices,
            &before.ui.show_event_notices,
            &after.ui.show_event_notices,
            "configuration UI show_event_notices changed concurrently; reload and retry",
        )?;
        Ok(())
    })?;
    Ok(committed)
}

/// Persist a complete secret-free configuration snapshot. Callers modifying a
/// stale long-lived snapshot should prefer `save_changes` or `mutate`.
pub fn save(path: Option<&Path>, config: &FileConfig) -> Result<PathBuf> {
    let path = resolved_config_path(path)?;
    let lock_path = metadata_lock_path(&path)?;
    let lock = aw_platform::open_user_metadata_lock(&lock_path)?;
    lock.lock_exclusive()?;
    save_unlocked(&path, config)?;
    drop(lock);
    Ok(path)
}

impl std::str::FromStr for ProviderKind {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "openai" => Ok(Self::Openai),
            "openai-responses" => Ok(Self::OpenaiResponses),
            "anthropic" => Ok(Self::Anthropic),
            "gemini" => Ok(Self::Gemini),
            "ollama" => Ok(Self::Ollama),
            _ => Err(
                "provider must be openai, openai-responses, anthropic, gemini, or ollama".into(),
            ),
        }
    }
}
impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Openai => "openai",
            Self::OpenaiResponses => "openai-responses",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
            Self::Ollama => "ollama",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    static SAVE_TEST_NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    fn profile(url: &str) -> Profile {
        let mut p = builtins().remove("llama").unwrap();
        p.base_url = url.into();
        p
    }
    #[test]
    fn skill_registry_is_bounded_explicit_and_resolves_relative_paths() {
        let cfg = parse(
            "[skills.review]\npath = 'skills/review.md'\ndescription = 'Review Rust invariants'\n",
            Some(Path::new("/tmp/aw-config")),
        )
        .unwrap();
        assert_eq!(cfg.skills.len(), 1);
        assert_eq!(
            cfg.skills["review"].path,
            PathBuf::from("/tmp/aw-config/skills/review.md")
        );
        assert!(parse("[skills.'bad/name']\npath='x'\n", None).is_err());
    }
    #[test]
    fn save_changes_preserves_unrelated_external_mutations() {
        let root = std::env::temp_dir().join(format!(
            "aw-config-merge-{}-{}",
            std::process::id(),
            SAVE_TEST_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        let before = FileConfig::default();
        save(Some(&path), &before).unwrap();

        let mut external = before.clone();
        external.skills.insert(
            "review".into(),
            SkillConfig {
                path: root.join("review.md"),
                description: Some("external skill".into()),
            },
        );
        save(Some(&path), &external).unwrap();

        let mut stale_ui = before.clone();
        stale_ui.ui.editor_padding_x = 3;
        let committed = save_changes(Some(&path), &before, &stale_ui).unwrap();
        assert_eq!(committed.ui.editor_padding_x, 3);
        assert!(committed.skills.contains_key("review"));
        let loaded = load(Some(&path)).unwrap();
        assert_eq!(loaded.ui.editor_padding_x, 3);
        assert!(loaded.skills.contains_key("review"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn save_changes_merges_independent_ui_fields() {
        let root = std::env::temp_dir().join(format!(
            "aw-config-ui-merge-{}-{}",
            std::process::id(),
            SAVE_TEST_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        let before = FileConfig::default();
        save(Some(&path), &before).unwrap();

        let mut external = before.clone();
        external.ui.theme = ThemeMode::Dark;
        external.ui.show_tool_calls = false;
        save(Some(&path), &external).unwrap();

        let mut stale = before.clone();
        stale.ui.editor_padding_x = 3;
        stale.ui.show_thinking = false;
        let committed = save_changes(Some(&path), &before, &stale).unwrap();
        assert_eq!(committed.ui.theme, ThemeMode::Dark);
        assert!(!committed.ui.show_tool_calls);
        assert_eq!(committed.ui.editor_padding_x, 3);
        assert!(!committed.ui.show_thinking);
        assert_eq!(load(Some(&path)).unwrap(), committed);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn save_changes_rejects_same_field_stale_overwrite() {
        let root = std::env::temp_dir().join(format!(
            "aw-config-conflict-{}-{}",
            std::process::id(),
            SAVE_TEST_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        let before = FileConfig::default();
        save(Some(&path), &before).unwrap();

        let mut newer = before.clone();
        newer.ui.output_padding = 1;
        save(Some(&path), &newer).unwrap();

        let mut stale = before.clone();
        stale.ui.output_padding = 3;
        assert!(save_changes(Some(&path), &before, &stale).is_err());
        assert_eq!(load(Some(&path)).unwrap().ui.output_padding, 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn mutate_serializes_collision_sensitive_registry_changes() {
        let root = std::env::temp_dir().join(format!(
            "aw-config-mutate-{}-{}",
            std::process::id(),
            SAVE_TEST_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        let source = root.join("review.md");
        std::fs::write(&source, "review").unwrap();
        let (_, ()) = mutate(Some(&path), |cfg| {
            cfg.skills.insert(
                "review".into(),
                SkillConfig {
                    path: source.clone(),
                    description: None,
                },
            );
            Ok(())
        })
        .unwrap();
        let second = mutate(Some(&path), |cfg| {
            if cfg.skills.contains_key("review") {
                return Err(Error::Conflict("skill already exists"));
            }
            Ok(())
        });
        assert!(second.is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn joins_endpoint_without_losing_prefix() {
        assert_eq!(
            profile("https://example.com/custom/v1/")
                .endpoint()
                .unwrap()
                .as_str(),
            "https://example.com/custom/v1/chat/completions"
        );
    }
    #[test]
    fn refuses_userinfo_query_fragment_and_schemes() {
        for u in [
            "ftp://host",
            "https://user:secret@host",
            "https://host/?key=x",
            "https://host/#secret",
        ] {
            assert!(profile(u).endpoint().is_err());
        }
    }
    #[test]
    fn allows_loopback_only_by_default() {
        for u in [
            "http://localhost:80",
            "http://127.0.0.1:80",
            "http://[::1]:80",
        ] {
            assert!(profile(u).endpoint().is_ok());
        }
        assert!(profile("http://192.168.1.2:80").endpoint().is_err());
    }
    #[test]
    fn remote_http_is_opt_in() {
        let mut p = profile("http://192.168.1.2:80");
        p.allow_insecure_http = true;
        assert!(p.endpoint().is_ok());
    }
    #[test]
    fn missing_and_unknown_fields_fail() {
        assert!(parse("[profiles.x]\nkind='ollama'", None).is_err());
        assert!(parse("api_key='secret'", None).is_err());
    }
    #[test]
    fn config_has_no_key_literals() {
        let e = parse("secret='SHOULD_NOT_APPEAR'", None)
            .unwrap_err()
            .to_string();
        assert!(!e.contains("SHOULD_NOT_APPEAR"));
    }
    #[test]
    fn validates_values() {
        let mut p = profile("http://127.0.0.1");
        p.max_tokens = Some(0);
        assert!(p.validate().is_err());
        p.max_tokens = None;
        p.temperature = Some(f64::NAN);
        assert!(p.validate().is_err());
    }
    #[test]
    fn effective_models_include_default_and_additional_models() {
        let mut p = profile("http://127.0.0.1");
        p.model = Some("default".into());
        p.models = vec!["small".into(), "large".into()];
        assert_eq!(
            p.effective_models().into_iter().collect::<Vec<_>>(),
            vec!["default", "large", "small"]
        );
        assert!(p.validate().is_ok());
    }

    #[test]
    fn profile_names_are_portable_account_labels() {
        for ok in ["ollama", "local-test", "team.alpha", "user@host"] {
            assert!(validate_profile_name(ok).is_ok());
        }
        for bad in ["", "has space", "path/name", "line\nbreak"] {
            assert!(validate_profile_name(bad).is_err());
        }
    }

    #[test]
    fn ui_settings_round_trip_and_validate_bounds() {
        let cfg = parse(
            "[ui]
status_line=['model','directory','branch']
tui_mode='inline'
theme='light'
editor_padding_x=0
output_padding=0
autocomplete_max_visible=7
show_performance=true
show_ttft=false
show_waterfall=false
",
            None,
        )
        .unwrap();
        assert_eq!(
            cfg.ui.status_lines,
            vec![StatusLineConfig {
                region: StatusRegion::AboveComposer,
                items: vec![StatusItem::Model, StatusItem::Directory, StatusItem::Branch],
                right_items: Vec::new(),
            }]
        );
        assert_eq!(cfg.ui.tui_mode, TuiMode::Inline);
        assert_eq!(cfg.ui.theme, ThemeMode::Light);
        for (value, expected) in [
            ("scrollback", TuiMode::Scrollback),
            ("inline", TuiMode::Inline),
            ("fullscreen", TuiMode::Fullscreen),
        ] {
            let parsed = parse(&format!("[ui]\ntui_mode='{value}'\n"), None).unwrap();
            assert_eq!(parsed.ui.tui_mode, expected);
        }
        assert_eq!(cfg.ui.editor_padding_x, 0);
        assert_eq!(cfg.ui.output_padding, 0);
        assert_eq!(cfg.ui.workspace_editor_padding_x, 1);
        assert_eq!(cfg.ui.workspace_output_padding, 1);
        assert_eq!(cfg.ui.autocomplete_max_visible, 7);
        assert!(cfg.ui.show_performance);
        assert!(!cfg.ui.show_ttft);
        assert!(!cfg.ui.show_waterfall);
        assert!(cfg.ui.show_thinking);
        assert!(cfg.ui.show_tool_calls);
        assert!(parse(
            "[ui]
status_line=['model','model']
",
            None
        )
        .is_err());
        assert!(parse(
            "[ui]
editor_padding_x=4
",
            None
        )
        .is_err());
        assert!(parse(
            "[ui]
autocomplete_max_visible=2
",
            None
        )
        .is_err());
        let cfg = parse(
            "[ui]
status_lines=[{region='above-composer',items=['state']},{region='below-composer',items=['directory','branch']},{region='below-composer',items=['model','profile']},{region='fullscreen-status',items=['model','branch']}]
",
            None,
        )
        .unwrap();
        assert_eq!(cfg.ui.status_lines.len(), 4);
        assert_eq!(cfg.ui.status_lines[0].region, StatusRegion::AboveComposer);
        assert_eq!(
            cfg.ui.status_lines[2].items,
            vec![StatusItem::Model, StatusItem::Profile]
        );
        assert!(parse(
            "[ui]
status_line=['model']
status_lines=[{region='below-composer',items=['branch']}]
",
            None
        )
        .is_err());
        assert_eq!(FileConfig::default().ui.theme, ThemeMode::System);
    }

    #[test]
    fn exact_legacy_default_status_layout_migrates_to_current_two_row_default() {
        let legacy = "[ui]\nstatus_lines=[{region='below-composer',items=['directory','branch']},{region='below-composer',items=['model','profile','state']},{region='below-composer',items=['tools','session']},{region='fullscreen-status',items=['model','directory','branch','profile','tools']}]\n";
        let parsed = parse(legacy, None).unwrap();
        assert_eq!(parsed.ui.status_lines, UiConfig::default().status_lines);
        assert_eq!(
            parsed
                .ui
                .status_lines
                .iter()
                .filter(|line| line.region == StatusRegion::BelowComposer)
                .count(),
            2
        );

        let previous = "[ui]\nstatus_lines=[{region='above-composer',items=['model','directory','branch','profile','tools','state']},{region='fullscreen-status',items=['model','directory','branch','profile','tools']}]\n";
        let parsed = parse(previous, None).unwrap();
        assert_eq!(
            parsed.ui.status_lines,
            UiConfig::default().status_lines,
            "the exact persisted Phase-57/58 default must follow the current default"
        );

        let custom = parse(
            "[ui]\nstatus_lines=[{region='below-composer',items=['branch'],right_items=['model','provider']}]\n",
            None,
        )
        .unwrap();
        assert_eq!(custom.ui.status_lines.len(), 1);
        assert_eq!(
            custom.ui.status_lines[0].region,
            StatusRegion::BelowComposer
        );
        assert_eq!(custom.ui.status_lines[0].items, vec![StatusItem::Branch]);
        assert_eq!(
            custom.ui.status_lines[0].right_items,
            vec![StatusItem::Model, StatusItem::Provider],
            "custom left/right status composition must not be migrated"
        );
    }

    #[test]
    fn profile_update_is_secret_free_validated_and_can_be_default() {
        let current = FileConfig::default();
        let (next, profile) = apply_profile_update(
            &current,
            ProfileUpdate {
                name: "cloud".into(),
                kind: ProviderKind::Ollama,
                base_url: "https://ollama.com".into(),
                default_model: Some("glm-5.3:cloud".into()),
                models: vec!["glm-5.3:cloud".into()],
                api_key_env: Some("OLLAMA_API_KEY".into()),
                oauth_connector: None,
                oauth_host_id: None,
                allow_insecure_http: false,
                token_field: TokenField::MaxTokens,
                include_usage: false,
                make_default: true,
            },
        )
        .unwrap();
        assert_eq!(next.default_profile.as_deref(), Some("cloud"));
        assert_eq!(profile.api_key_env.as_deref(), Some("OLLAMA_API_KEY"));
        assert_eq!(
            profile.endpoint().unwrap().as_str(),
            "https://ollama.com/api/chat"
        );
        assert!(apply_profile_update(
            &current,
            ProfileUpdate {
                name: "bad".into(),
                kind: ProviderKind::Openai,
                base_url: "https://api.openai.com/v1".into(),
                default_model: None,
                models: vec![],
                api_key_env: Some("raw token".into()),
                oauth_connector: None,
                oauth_host_id: None,
                allow_insecure_http: false,
                token_field: TokenField::MaxTokens,
                include_usage: false,
                make_default: true,
            },
        )
        .is_err());
    }

    #[test]
    fn save_replaces_existing_config_without_delete_gap_on_unix() {
        let root = std::env::temp_dir().join(format!(
            "aw-config-save-{}-{}",
            std::process::id(),
            SAVE_TEST_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        std::fs::write(&path, "default_profile='old'\n").unwrap();
        let cfg = FileConfig {
            default_profile: Some("ollama".into()),
            ..FileConfig::default()
        };
        save(Some(&path), &cfg).unwrap();
        let loaded = load(Some(&path)).unwrap();
        assert_eq!(loaded.default_profile.as_deref(), Some("ollama"));
        assert!(!std::fs::read_dir(&root).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".tmp-")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_terminal_endpoint() {
        assert!(profile("http://127.0.0.1/v1/chat/completions")
            .endpoint()
            .is_err());
    }
}

#[cfg(test)]
mod aw_p6_8a_extension_config_tests {
    use super::*;
    use std::{fs, path::PathBuf};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "aw-p6-8a-config-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn value(n: i64) -> serde_json::Value {
        serde_json::json!({"type":"integer","value":n})
    }
    #[test]
    fn extension_config_rejects_private_types_raw_values_unknown_fields_and_control() {
        for malformed in [
            serde_json::json!("raw bearer string"),
            serde_json::json!({"type":"secret","value":"bearer"}),
            serde_json::json!({"type":"text","value":"\u{1b}[31m"}),
            serde_json::json!({"type":"boolean","value":"true"}),
            serde_json::json!({"type":"integer","value":0,"admin":true}),
            serde_json::json!({"type":"integer","value":1.5}),
        ] {
            let settings = BTreeMap::from([(
                "ext".into(),
                BTreeMap::from([("setting".into(), malformed)]),
            )]);
            assert!(validate_extension_settings(&settings).is_err());
        }
        assert!(validate_extension_settings(&BTreeMap::from([(
            "../../escape".into(),
            BTreeMap::from([("setting".into(), value(1))])
        ),]))
        .is_err());
    }
    #[test]
    fn extension_config_stale_cas_is_key_precise_and_keeps_unrelated_settings() {
        let tmp = Temp::new();
        let config = tmp.0.join("config.toml");
        let original = FileConfig::default();
        save(Some(&config), &original).unwrap();
        let before = load(Some(&config)).unwrap();
        let mut one = before.clone();
        one.extension_settings
            .insert("ext".into(), BTreeMap::from([("one".into(), value(1))]));
        let mut two = before.clone();
        two.extension_settings
            .insert("ext".into(), BTreeMap::from([("two".into(), value(2))]));
        let first = save_changes(Some(&config), &before, &one).unwrap();
        assert_eq!(first.extension_settings["ext"].len(), 1);
        let second = save_changes(Some(&config), &before, &two).unwrap();
        assert_eq!(
            second.extension_settings["ext"].len(),
            2,
            "stale disjoint changes to same extension must merge per setting key"
        );
        let mut conflict = before.clone();
        conflict
            .extension_settings
            .insert("ext".into(), BTreeMap::from([("one".into(), value(3))]));
        assert!(
            save_changes(Some(&config), &before, &conflict).is_err(),
            "stale competing key mutation must not overwrite current config"
        );
        assert_eq!(
            load(Some(&config)).unwrap().extension_settings["ext"]["one"],
            value(1)
        );
    }
}

#[cfg(test)]
mod aw_p6_8b_status_region_config_tests {
    use super::*;
    use std::{fs, path::PathBuf};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "aw-p6-8b-status-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn aw_p6_8b_new_status_regions_persist_and_stale_layouts_are_rejected() {
        let tmp = Temp::new();
        let file = tmp.0.join("config.toml");
        let before = FileConfig::default();
        save(Some(&file), &before).unwrap();
        let mut next = before.clone();
        next.ui.status_lines = vec![
            StatusLineConfig {
                region: StatusRegion::LeftStatus,
                items: vec![StatusItem::Agent],
                right_items: vec![],
            },
            StatusLineConfig {
                region: StatusRegion::RightStatus,
                items: vec![StatusItem::Branch],
                right_items: vec![],
            },
            StatusLineConfig {
                region: StatusRegion::BelowStatus,
                items: vec![StatusItem::State],
                right_items: vec![],
            },
        ];
        let stored = save_changes(Some(&file), &before, &next).unwrap();
        assert_eq!(stored.ui.status_lines.len(), 3);
        let read = load(Some(&file)).unwrap();
        assert_eq!(read.ui.status_lines[0].region, StatusRegion::LeftStatus);
        assert_eq!(read.ui.status_lines[1].region, StatusRegion::RightStatus);
        assert_eq!(read.ui.status_lines[2].region, StatusRegion::BelowStatus);
        assert_eq!(
            read.ui.tui_mode, before.ui.tui_mode,
            "status persistence cannot silently switch TUI modes or open another pane"
        );
        let mut stale = before.clone();
        stale.ui.status_lines = vec![StatusLineConfig {
            region: StatusRegion::BelowComposer,
            items: vec![StatusItem::Tools],
            right_items: vec![],
        }];
        assert!(
            save_changes(Some(&file), &before, &stale).is_err(),
            "changing a stale status layout cannot overwrite a newer status revision"
        );
        assert_eq!(load(Some(&file)).unwrap().ui.status_lines.len(), 3);
    }
}

#[cfg(test)]
mod aw_p6_8c_optional_notices_tests {
    use super::*;
    use std::{fs, path::PathBuf};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "aw-p6-8c-config-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn aw_p6_8c_factual_event_notices_are_false_by_default_atomic_and_stale_cas() {
        let tmp = Temp::new();
        let path = tmp.0.join("settings.toml");
        let before = FileConfig::default();
        assert!(!before.ui.show_event_notices);
        assert!(
            !serde_json::to_string(&before.ui)
                .unwrap()
                .contains("show_event_notices"),
            "disabled notice flag cannot require compatibility or extra UI chrome"
        );
        save(Some(&path), &before).unwrap();
        let mut enabled = before.clone();
        enabled.ui.show_event_notices = true;
        let current = save_changes(Some(&path), &before, &enabled).unwrap();
        assert!(current.ui.show_event_notices);
        assert!(load(Some(&path)).unwrap().ui.show_event_notices);
        assert!(serde_json::to_string(&load(Some(&path)).unwrap().ui)
            .unwrap()
            .contains("show_event_notices"));
        let mut unrelated = before.clone();
        unrelated.ui.show_waterfall = !before.ui.show_waterfall;
        assert!(
            save_changes(Some(&path), &before, &unrelated).is_ok(),
            "per-key config merge keeps independent UI fields intact"
        );
        let final_cfg = load(Some(&path)).unwrap();
        assert!(final_cfg.ui.show_event_notices);
        assert_ne!(final_cfg.ui.show_waterfall, before.ui.show_waterfall);
        let mut attempt = before.clone();
        attempt.ui.show_event_notices = false;
        assert!(
            save_changes(Some(&path), &before, &attempt).is_ok(),
            "a no-op base revision cannot undo someone else's enabled event flag"
        );
        assert!(load(Some(&path)).unwrap().ui.show_event_notices);
        let mut clear = final_cfg.clone();
        clear.ui.show_event_notices = false;
        assert!(save_changes(Some(&path), &final_cfg, &clear).is_ok());
        assert!(!load(Some(&path)).unwrap().ui.show_event_notices);
    }
}
