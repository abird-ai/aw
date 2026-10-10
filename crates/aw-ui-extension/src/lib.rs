//! Transport-neutral UI extension contract.
//!
//! UI extensions are presentation-only. They receive a bounded immutable
//! projection and return semantic presentation lines. Registration does not
//! grant provider credentials, model/tool authority, filesystem/process/network
//! access, or agent-hook authority.
//!
//! Extensions are compiled into the single aw binary today. The serde request,
//! response, manifest and contribution types deliberately form a future
//! extension-host protocol without forcing IPC into the current implementation.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, sync::Arc};
use thiserror::Error;

pub mod prompt;
pub mod settings;
pub mod status_events;

pub const MAX_UI_EXTENSIONS: usize = 64;
pub const MAX_LINES_PER_CONTRIBUTION: usize = 32;
pub const MAX_SPANS_PER_LINE: usize = 32;
pub const MAX_TEXT_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Slot {
    AboveComposer,
    BelowComposer,
    AfterTurn,
    FullscreenStatus,
    /// v2 presentation-only opt-in regions; never force more UI chrome.
    LeftStatus,
    RightStatus,
    BelowStatus,
    AfterStep,
    AfterFinalOutput,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tone {
    #[default]
    Normal,
    Muted,
    Accent,
    Model,
    Profile,
    Provider,
    Effort,
    Directory,
    Agent,
    Branch,
    Tools,
    Session,
    State,
    Performance,
    Good,
    Warning,
    Error,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Span {
    pub text: String,
    #[serde(default)]
    pub tone: Tone,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
}

impl Span {
    pub fn text(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            text: text.into(),
            tone,
            bold: false,
            italic: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Line {
    pub spans: Vec<Span>,
}

impl Line {
    pub fn plain(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            spans: vec![Span::text(text, tone)],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.spans.iter().all(|span| span.text.is_empty())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contribution {
    pub extension: String,
    pub slot: Slot,
    pub order: i16,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub protocol_version: u32,
    pub slots: Vec<Slot>,
    /// Host/extension-registry settings definitions. Empty is omitted to
    /// preserve the original v1 presentation-only manifest on the wire.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<settings::SettingDefinition>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Performance {
    pub elapsed_secs: f64,
    pub first_token_secs: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub output_tps: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub model: String,
    pub profile: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub effort: String,
    pub directory: String,
    #[serde(default)]
    pub agent: String,
    pub branch: String,
    pub tools: String,
    pub session: String,
    pub state: String,
    #[serde(default)]
    pub session_tps: Option<f64>,
    #[serde(default)]
    pub performance: Option<Performance>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Request {
    Manifest,
    Project {
        slot: Slot,
        context: Box<Context>,
    },
    /// Only v2 registered extensions; non-provider factual UI event signal.
    Event {
        event: Box<status_events::EventContext>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Response {
    Manifest { manifest: Manifest },
    Project { contributions: Vec<Contribution> },
    Event { contributions: Vec<Contribution> },
    Error { message: String },
}

pub trait UiExtension: Send + Sync {
    fn manifest(&self) -> Manifest;
    fn project(&self, slot: Slot, context: &Context) -> Vec<Contribution>;
    /// Typed factual presentation only; no Agent/Session mutation or Hook.
    fn event(&self, _event: &status_events::EventContext) -> Vec<Contribution> {
        Vec::new()
    }
}

/// Typed host boundary used by the registry. Today compiled Rust extensions are
/// wrapped by InProcessExtensionHost; a future child-process extension host can
/// implement the same request/response contract over framed IPC without changing
/// the registry, slots, contribution validation, or TUI renderers.
pub trait ExtensionHost: Send + Sync {
    fn request(&self, request: Request) -> Result<Response, RegistryError>;
}

pub struct InProcessExtensionHost {
    extension: Arc<dyn UiExtension>,
}

impl InProcessExtensionHost {
    pub fn new(extension: Arc<dyn UiExtension>) -> Self {
        Self { extension }
    }
}

impl ExtensionHost for InProcessExtensionHost {
    fn request(&self, request: Request) -> Result<Response, RegistryError> {
        Ok(match request {
            Request::Manifest => Response::Manifest {
                manifest: self.extension.manifest(),
            },
            Request::Project { slot, context } => Response::Project {
                contributions: self.extension.project(slot, context.as_ref()),
            },
            Request::Event { event } => Response::Event {
                contributions: self.extension.event(event.as_ref()),
            },
        })
    }
}

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("too many UI extensions")]
    TooMany,
    #[error("invalid or duplicate UI extension id")]
    InvalidId,
    #[error("invalid UI extension contribution")]
    InvalidContribution,
    #[error("UI extension host returned an invalid response")]
    InvalidHostResponse,
    #[error("UI extension host rejected projection")]
    HostRejected,
    #[error("UI extension supplied invalid or impersonated settings")]
    InvalidSettings,
}

#[derive(Clone)]
struct HostedExtension {
    manifest: Manifest,
    host: Arc<dyn ExtensionHost>,
}

#[derive(Clone, Default)]
pub struct Registry {
    extensions: Vec<HostedExtension>,
    settings: settings::SettingsRegistry,
}

impl Registry {
    pub fn new(
        extensions: impl IntoIterator<Item = Arc<dyn UiExtension>>,
    ) -> Result<Self, RegistryError> {
        Self::from_hosts(extensions.into_iter().map(|extension| {
            Arc::new(InProcessExtensionHost::new(extension)) as Arc<dyn ExtensionHost>
        }))
    }

    pub fn from_hosts(
        hosts: impl IntoIterator<Item = Arc<dyn ExtensionHost>>,
    ) -> Result<Self, RegistryError> {
        let hosts = hosts.into_iter().collect::<Vec<_>>();
        if hosts.len() > MAX_UI_EXTENSIONS {
            return Err(RegistryError::TooMany);
        }
        let mut ids = BTreeSet::new();
        let mut extensions = Vec::with_capacity(hosts.len());
        let mut definitions = Vec::new();
        for host in hosts {
            let manifest = match host.request(Request::Manifest)? {
                Response::Manifest { manifest } => manifest,
                _ => return Err(RegistryError::InvalidHostResponse),
            };
            if !valid_id(&manifest.id)
                || !matches!(manifest.protocol_version, 1 | 2)
                || (manifest.protocol_version == 1
                    && manifest.slots.iter().any(|slot| {
                        matches!(
                            slot,
                            Slot::LeftStatus
                                | Slot::RightStatus
                                | Slot::BelowStatus
                                | Slot::AfterStep
                                | Slot::AfterFinalOutput
                        )
                    }))
                || manifest.slots.len() > 8
                || !ids.insert(manifest.id.clone())
            {
                return Err(RegistryError::InvalidId);
            }
            if manifest
                .settings
                .iter()
                .any(|setting| setting.extension != manifest.id)
            {
                return Err(RegistryError::InvalidSettings);
            }
            definitions.extend(manifest.settings.clone());
            extensions.push(HostedExtension { manifest, host });
        }
        let settings = settings::SettingsRegistry::new(definitions)
            .map_err(|_| RegistryError::InvalidSettings)?;
        Ok(Self {
            extensions,
            settings,
        })
    }

    pub fn settings(&self) -> &settings::SettingsRegistry {
        &self.settings
    }

    pub fn project(&self, slot: Slot, context: &Context) -> Result<Vec<Line>, RegistryError> {
        let mut contributions = Vec::new();
        for extension in &self.extensions {
            if !extension.manifest.slots.contains(&slot) {
                continue;
            }
            let projected = match extension.host.request(Request::Project {
                slot,
                context: Box::new(context.clone()),
            })? {
                Response::Project { contributions } => contributions,
                Response::Error { .. } => return Err(RegistryError::HostRejected),
                _ => return Err(RegistryError::InvalidHostResponse),
            };
            for contribution in projected {
                if contribution.extension != extension.manifest.id
                    || contribution.slot != slot
                    || contribution.lines.len() > MAX_LINES_PER_CONTRIBUTION
                    || contribution.lines.iter().any(|line| {
                        line.spans.len() > MAX_SPANS_PER_LINE
                            || line.spans.iter().any(|span| {
                                span.text.len() > MAX_TEXT_BYTES
                                    || span
                                        .text
                                        .chars()
                                        .any(|c| c.is_control() && !matches!(c, '\t'))
                            })
                    })
                {
                    return Err(RegistryError::InvalidContribution);
                }
                contributions.push(contribution);
            }
        }
        contributions.sort_by(|a, b| {
            a.order
                .cmp(&b.order)
                .then_with(|| a.extension.cmp(&b.extension))
        });
        Ok(contributions
            .into_iter()
            .flat_map(|contribution| contribution.lines)
            .filter(|line| !line.is_empty())
            .collect())
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Demo(&'static str, Slot, i16);

    impl UiExtension for Demo {
        fn manifest(&self) -> Manifest {
            Manifest {
                id: self.0.into(),
                protocol_version: 1,
                slots: vec![self.1],
                settings: Vec::new(),
            }
        }

        fn project(&self, slot: Slot, _: &Context) -> Vec<Contribution> {
            vec![Contribution {
                extension: self.0.into(),
                slot,
                order: self.2,
                lines: vec![Line::plain(self.0, Tone::Normal)],
            }]
        }
    }

    #[test]
    fn registry_is_ordered_and_slot_scoped() {
        let registry = Registry::new([
            Arc::new(Demo("later", Slot::BelowComposer, 20)) as Arc<dyn UiExtension>,
            Arc::new(Demo("first", Slot::BelowComposer, 10)) as Arc<dyn UiExtension>,
            Arc::new(Demo("other", Slot::AboveComposer, 0)) as Arc<dyn UiExtension>,
        ])
        .unwrap();
        let lines = registry
            .project(Slot::BelowComposer, &Context::default())
            .unwrap();
        assert_eq!(lines[0].spans[0].text, "first");
        assert_eq!(lines[1].spans[0].text, "later");
    }

    #[test]
    fn duplicate_ids_fail_closed() {
        assert!(Registry::new([
            Arc::new(Demo("same", Slot::BelowComposer, 0)) as Arc<dyn UiExtension>,
            Arc::new(Demo("same", Slot::AboveComposer, 0)) as Arc<dyn UiExtension>,
        ])
        .is_err());
    }

    #[test]
    fn aw_p6_8a_extension_settings_register_through_existing_ui_manifest_boundary() {
        use settings::{SettingDefinition, SettingKind, SettingValue};
        struct SettingsHost {
            owner: &'static str,
            spoof: bool,
        }
        impl UiExtension for SettingsHost {
            fn manifest(&self) -> Manifest {
                Manifest {
                    id: self.owner.into(),
                    protocol_version: 1,
                    slots: Vec::new(),
                    settings: vec![SettingDefinition {
                        extension: if self.spoof { "other" } else { self.owner }.into(),
                        name: "visible".into(),
                        kind: SettingKind::Boolean,
                        default: Some(SettingValue::Boolean(false)),
                        description: String::new(),
                    }],
                }
            }
            fn project(&self, _: Slot, _: &Context) -> Vec<Contribution> {
                Vec::new()
            }
        }
        let registered = Registry::new([
            Arc::new(SettingsHost {
                owner: "alpha",
                spoof: false,
            }) as Arc<dyn UiExtension>,
            Arc::new(SettingsHost {
                owner: "beta",
                spoof: false,
            }) as Arc<dyn UiExtension>,
        ])
        .unwrap();
        assert_eq!(registered.settings().definitions().count(), 2);
        assert!(registered.settings().definition("alpha", "visible").is_ok());
        assert_eq!(
            registered
                .settings()
                .resolve("beta", "visible", None, None, None)
                .unwrap()
                .value,
            Some(SettingValue::Boolean(false))
        );
        assert!(
            matches!(
                Registry::new([Arc::new(SettingsHost {
                    owner: "alpha",
                    spoof: true
                }) as Arc<dyn UiExtension>]),
                Err(RegistryError::InvalidSettings)
            ),
            "one extension cannot impersonate another owner's config namespace"
        );
    }

    struct ProtocolOnlyHost;

    impl ExtensionHost for ProtocolOnlyHost {
        fn request(&self, request: Request) -> Result<Response, RegistryError> {
            Ok(match request {
                Request::Manifest => Response::Manifest {
                    manifest: Manifest {
                        id: "protocol-only".into(),
                        protocol_version: 1,
                        slots: vec![Slot::AboveComposer],
                        settings: Vec::new(),
                    },
                },
                Request::Project { slot, .. } => Response::Project {
                    contributions: vec![Contribution {
                        extension: "protocol-only".into(),
                        slot,
                        order: 5,
                        lines: vec![Line::plain("external-shape", Tone::Accent)],
                    }],
                },
                Request::Event { .. } => Response::Error {
                    message: "v1 extension cannot receive factual event requests".into(),
                },
            })
        }
    }

    #[test]
    fn registry_accepts_protocol_host_without_compiled_extension_trait() {
        let registry =
            Registry::from_hosts([Arc::new(ProtocolOnlyHost) as Arc<dyn ExtensionHost>]).unwrap();
        let lines = registry
            .project(Slot::AboveComposer, &Context::default())
            .unwrap();
        assert_eq!(lines[0].spans[0].text, "external-shape");
    }
}
