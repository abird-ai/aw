//! AW-P6-8A: a pure, bounded, *trusted host only* typed extension settings
//! registry. It owns definitions, NOT authority, secrets, file access or
//! execution. A saved value is inert until a currently registered definition
//! validates it. Project values must be verified by the host against its
//! approved exact-source digest before being passed to `resolve`.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const MAX_SETTING_DEFINITIONS: usize = 256;
pub const MAX_EXTENSION_SETTINGS: usize = 32;
pub const MAX_SETTING_TEXT: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum SettingValue {
    Boolean(bool),
    Integer(i64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SettingKind {
    Boolean,
    Integer {
        min: i64,
        max: i64,
    },
    Text {
        max_bytes: u16,
    },
    Select {
        choices: Vec<String>,
    },
    /// No plaintext may enter ordinary config, project, RPC or UI. This
    /// variant cannot be set/read without a future explicit vault authority.
    Secret {
        max_bytes: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingDefinition {
    pub extension: String,
    pub name: String,
    pub kind: SettingKind,
    pub default: Option<SettingValue>,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingSource {
    Cli,
    TrustedProject,
    User,
    Default,
    Unset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedSetting {
    pub extension: String,
    pub name: String,
    pub value: Option<SettingValue>,
    pub source: SettingSource,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SettingError {
    #[error("invalid extension setting definition")]
    InvalidDefinition,
    #[error("duplicate extension setting")]
    Duplicate,
    #[error("too many extension settings")]
    Limit,
    #[error("extension setting is not registered")]
    NotRegistered,
    #[error("extension setting value fails its exact registered type or bounds")]
    InvalidValue,
    #[error("private extension setting requires separately authenticated vault authority")]
    SecretDenied,
}

fn label(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_alphanumeric() || (i != 0 && b"._-".contains(&b)))
}
fn printable(s: &str, maximum: usize) -> bool {
    s.len() <= maximum && !s.chars().any(char::is_control)
}
impl SettingKind {
    pub fn validate_value(&self, value: &SettingValue) -> Result<(), SettingError> {
        let valid = match (self, value) {
            (Self::Boolean, SettingValue::Boolean(_)) => true,
            (Self::Integer { min, max }, SettingValue::Integer(n)) => min <= n && n <= max,
            (Self::Text { max_bytes }, SettingValue::Text(s)) => {
                printable(s, usize::from(*max_bytes))
            }
            (Self::Select { choices }, SettingValue::Text(s)) => {
                choices.iter().any(|choice| choice == s)
            }
            (Self::Secret { .. }, _) => return Err(SettingError::SecretDenied),
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(SettingError::InvalidValue)
        }
    }
    fn validate_definition(&self) -> bool {
        match self {
            Self::Boolean => true,
            Self::Integer { min, max } => min <= max,
            Self::Text { max_bytes } | Self::Secret { max_bytes } => {
                (1..=MAX_SETTING_TEXT as u16).contains(max_bytes)
            }
            Self::Select { choices } => {
                !choices.is_empty()
                    && choices.len() <= 32
                    && choices
                        .iter()
                        .all(|choice| !choice.is_empty() && printable(choice, 128))
                    && choices.iter().collect::<BTreeSet<_>>().len() == choices.len()
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct SettingsRegistry {
    definitions: BTreeMap<(String, String), SettingDefinition>,
}
impl SettingsRegistry {
    pub fn new(
        definitions: impl IntoIterator<Item = SettingDefinition>,
    ) -> Result<Self, SettingError> {
        let mut registry = Self::default();
        for definition in definitions {
            registry.add_setting(definition)?;
        }
        Ok(registry)
    }
    /// Register a definition from an explicitly trusted extension manifest.
    /// There is no group grant or automatic tool/Agent registration.
    pub fn add_setting(&mut self, definition: SettingDefinition) -> Result<(), SettingError> {
        if !label(&definition.extension)
            || !label(&definition.name)
            || !definition.kind.validate_definition()
            || !printable(&definition.description, 512)
            || matches!(definition.kind, SettingKind::Secret { .. }) && definition.default.is_some()
            || definition
                .default
                .as_ref()
                .is_some_and(|value| definition.kind.validate_value(value).is_err())
        {
            return Err(SettingError::InvalidDefinition);
        }
        let key = (definition.extension.clone(), definition.name.clone());
        if self.definitions.contains_key(&key) {
            return Err(SettingError::Duplicate);
        }
        if self.definitions.len() >= MAX_SETTING_DEFINITIONS {
            return Err(SettingError::Limit);
        }
        let count = self
            .definitions
            .keys()
            .filter(|(extension, _)| extension == &definition.extension)
            .count();
        if count >= MAX_EXTENSION_SETTINGS
            || count == 0
                && self
                    .definitions
                    .keys()
                    .map(|(extension, _)| extension)
                    .collect::<BTreeSet<_>>()
                    .len()
                    >= super::MAX_UI_EXTENSIONS
        {
            return Err(SettingError::Limit);
        }
        self.definitions.insert(key, definition);
        Ok(())
    }
    pub fn definition(
        &self,
        extension: &str,
        name: &str,
    ) -> Result<&SettingDefinition, SettingError> {
        self.definitions
            .get(&(extension.to_owned(), name.to_owned()))
            .ok_or(SettingError::NotRegistered)
    }
    pub fn definitions(&self) -> impl Iterator<Item = &SettingDefinition> {
        self.definitions.values()
    }
    pub fn validate_user_value(
        &self,
        extension: &str,
        name: &str,
        value: &SettingValue,
    ) -> Result<(), SettingError> {
        self.definition(extension, name)?.kind.validate_value(value)
    }
    /// Caller MUST already have verified project trust and source revision
    /// before supplying the project layer. A pure registry is not a grant.
    pub fn resolve(
        &self,
        extension: &str,
        name: &str,
        cli: Option<&SettingValue>,
        project: Option<&SettingValue>,
        user: Option<&SettingValue>,
    ) -> Result<ResolvedSetting, SettingError> {
        let definition = self.definition(extension, name)?;
        if matches!(definition.kind, SettingKind::Secret { .. }) {
            if cli.is_some() || project.is_some() || user.is_some() {
                return Err(SettingError::SecretDenied);
            }
            return Ok(ResolvedSetting {
                extension: extension.into(),
                name: name.into(),
                value: None,
                source: SettingSource::Unset,
            });
        }
        // Invalid earlier-layer values MUST be rejected, not silently
        // shadowed by a higher layer: stale user config/project source cannot
        // smuggle a different data type that becomes effective later.
        for layer in [cli, project, user].into_iter().flatten() {
            definition.kind.validate_value(layer)?;
        }
        let (source, value) = if let Some(value) = cli {
            (SettingSource::Cli, Some(value.clone()))
        } else if let Some(value) = project {
            (SettingSource::TrustedProject, Some(value.clone()))
        } else if let Some(value) = user {
            (SettingSource::User, Some(value.clone()))
        } else if let Some(value) = &definition.default {
            (SettingSource::Default, Some(value.clone()))
        } else {
            (SettingSource::Unset, None)
        };
        Ok(ResolvedSetting {
            extension: extension.into(),
            name: name.into(),
            value,
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn defs() -> SettingsRegistry {
        SettingsRegistry::new([
            SettingDefinition {
                extension: "extension.foo".into(),
                name: "visible".into(),
                kind: SettingKind::Boolean,
                default: Some(SettingValue::Boolean(false)),
                description: "Set visibility".into(),
            },
            SettingDefinition {
                extension: "extension.foo".into(),
                name: "budget".into(),
                kind: SettingKind::Integer { min: 0, max: 8 },
                default: Some(SettingValue::Integer(2)),
                description: "Bounded extension runtime setting".into(),
            },
            SettingDefinition {
                extension: "extension.foo".into(),
                name: "mode".into(),
                kind: SettingKind::Select {
                    choices: vec!["quiet".into(), "full".into()],
                },
                default: Some(SettingValue::Text("quiet".into())),
                description: "Mode".into(),
            },
            SettingDefinition {
                extension: "extension.foo".into(),
                name: "credential".into(),
                kind: SettingKind::Secret { max_bytes: 128 },
                default: None,
                description: "Private vault only".into(),
            },
        ])
        .unwrap()
    }
    #[test]
    fn typed_precedence_registry_8a_rejects_invalid_masked_lower_layers() {
        let reg = defs();
        let value = reg
            .resolve("extension.foo", "visible", None, None, None)
            .unwrap();
        assert_eq!(value.source, SettingSource::Default);
        assert_eq!(value.value, Some(SettingValue::Boolean(false)));
        let user = SettingValue::Boolean(true);
        let project = SettingValue::Boolean(false);
        let cli = SettingValue::Boolean(true);
        assert_eq!(
            reg.resolve("extension.foo", "visible", None, None, Some(&user))
                .unwrap()
                .source,
            SettingSource::User
        );
        assert_eq!(
            reg.resolve(
                "extension.foo",
                "visible",
                None,
                Some(&project),
                Some(&user)
            )
            .unwrap()
            .source,
            SettingSource::TrustedProject
        );
        assert_eq!(
            reg.resolve(
                "extension.foo",
                "visible",
                Some(&cli),
                Some(&project),
                Some(&user)
            )
            .unwrap()
            .source,
            SettingSource::Cli
        );
        assert_eq!(
            reg.resolve(
                "extension.foo",
                "budget",
                Some(&SettingValue::Integer(2)),
                None,
                Some(&SettingValue::Integer(100))
            )
            .unwrap_err(),
            SettingError::InvalidValue
        );
        assert_eq!(
            reg.resolve("extension.foo", "credential", None, None, Some(&user))
                .unwrap_err(),
            SettingError::SecretDenied
        );
        assert_eq!(
            reg.resolve("extension.unknown", "value", None, None, None)
                .unwrap_err(),
            SettingError::NotRegistered
        );
    }
    #[test]
    fn typed_registry_rejects_duplicates_oversize_control_and_secrets() {
        let r = defs();
        let mut invalid = r.definition("extension.foo", "visible").unwrap().clone();
        assert_eq!(
            SettingsRegistry::new([invalid.clone(), invalid.clone()]).err(),
            Some(SettingError::Duplicate)
        );
        invalid.name = "../injected".into();
        assert_eq!(
            SettingsRegistry::new([invalid]).err(),
            Some(SettingError::InvalidDefinition)
        );
        let secret = SettingDefinition {
            extension: "secrets".into(),
            name: "token".into(),
            kind: SettingKind::Secret { max_bytes: 1024 },
            default: Some(SettingValue::Text("exfil".into())),
            description: String::new(),
        };
        assert_eq!(
            SettingsRegistry::new([secret]).err(),
            Some(SettingError::InvalidDefinition)
        );
        assert!(!SettingKind::Select {
            choices: vec!["a".into(), "a".into()]
        }
        .validate_definition());
        assert_eq!(
            SettingKind::Text { max_bytes: 5 }
                .validate_value(&SettingValue::Text("🦉🦉".into()))
                .unwrap_err(),
            SettingError::InvalidValue
        );
        assert_eq!(
            SettingKind::Text { max_bytes: 20 }
                .validate_value(&SettingValue::Text("evil\u{1b}[31m".into()))
                .unwrap_err(),
            SettingError::InvalidValue
        );
    }
}
