//! AW-P6-8A owning-host adapter for presentation-only extension settings.
//!
//! Definitions come from explicitly installed TRUSTED Rust extensions, not
//! model/project text. User values are inert config data, written through the
//! existing atomic locked CAS. Project values are rescanned through the
//! existing no-follow, hash-pinned ProjectCatalog on EACH resolution; untrusted
//! or changed source data never becomes a setting. CLI values are temporary.
//! Secret setting values are not supported on this data channel: they MUST
//! use separately reviewed encrypted current-owner SecretStore custody.
use crate::{config, project_resources::Catalog, Error, Result};
use aw_ui_extension::settings::{
    ResolvedSetting, SettingError, SettingKind, SettingValue, SettingsRegistry,
};
use std::{collections::BTreeMap, path::Path};

fn issue(error: SettingError) -> Error {
    match error {
        SettingError::NotRegistered | SettingError::SecretDenied => Error::Denied(
            "unregistered or private extension setting requires separate owner authority",
        ),
        SettingError::Limit => Error::Limit("extension setting registry exceeds bounds"),
        SettingError::Duplicate | SettingError::InvalidDefinition | SettingError::InvalidValue => {
            Error::Denied("extension setting failed registered type, value or bound validation")
        }
    }
}

fn saved_value(
    cfg: &config::FileConfig,
    extension: &str,
    name: &str,
) -> Result<Option<SettingValue>> {
    cfg.extension_settings
        .get(extension)
        .and_then(|settings| settings.get(name))
        .map(|value| {
            serde_json::from_value::<SettingValue>(value.clone()).map_err(|_| {
                Error::Denied("saved extension setting contains an unrecognized or secret value")
            })
        })
        .transpose()
}

/// A project setting may be sourced ONLY from `.agents/settings/E.K.json`
/// after a *fresh* descriptor-relative scan and equality with the operator's
/// explicit approved digest snapshot. A caller cannot insert an arbitrary
/// pre-verified project value, skip approval or reuse a prior stale scan.
fn project_value(
    cfg: &config::FileConfig,
    extension: &str,
    name: &str,
    project: &Path,
    cwd: &Path,
) -> Result<Option<SettingValue>> {
    let fresh = Catalog::scan(project, cwd)?;
    fresh.verify(cfg)?;
    let file = format!("{extension}.{name}");
    fresh
        .extension_settings
        .get(&file)
        .map(|source| {
            if source.path != format!(".agents/settings/{file}.json") {
                return Err(Error::Denied(
                    "project extension setting does not match pinned path",
                ));
            }
            let value: serde_json::Value = serde_json::from_str(&source.content)
                .map_err(|_| Error::Denied("project extension setting must be typed JSON"))?;
            config::validate_extension_settings(&BTreeMap::from([(
                extension.to_owned(),
                BTreeMap::from([(name.to_owned(), value.clone())]),
            )]))?;
            serde_json::from_value(value).map_err(|_| Error::Denied("invalid project setting type"))
        })
        .transpose()
}

/// Resolves CLI > freshly hash-verified project > durable user > default.
/// There is no implicit project selection: a caller must identify one approved
/// root/cwd, and any trust or source revision mismatch is a hard DENY, never a
/// fallback that could hide malicious changed project data.
pub fn resolve(
    registry: &SettingsRegistry,
    path: Option<&Path>,
    extension: &str,
    name: &str,
    cli: Option<&SettingValue>,
    project: Option<(&Path, &Path)>,
) -> Result<ResolvedSetting> {
    let definition = registry.definition(extension, name).map_err(issue)?;
    if matches!(definition.kind, SettingKind::Secret { .. }) {
        return Err(Error::Denied(
            "secret settings require a separately authorized encrypted vault",
        ));
    }
    let cfg = config::load(path)?;
    let user = saved_value(&cfg, extension, name)?;
    let project = project
        .map(|(root, cwd)| project_value(&cfg, extension, name, root, cwd))
        .transpose()?
        .flatten();
    registry
        .resolve(extension, name, cli, project.as_ref(), user.as_ref())
        .map_err(issue)
}

/// Compare-and-swap this exact user setting under config::mutate's existing
/// file-system lock; never write whole extension groups or replace unrelated
/// profile/skills/UI config. `expected=None` requires this key be unset.
/// A successful SET is DATA only; no action/tool/provider grant is issued.
pub fn set(
    registry: &SettingsRegistry,
    path: Option<&Path>,
    extension: &str,
    name: &str,
    expected: Option<&SettingValue>,
    value: SettingValue,
) -> Result<ResolvedSetting> {
    registry
        .validate_user_value(extension, name, &value)
        .map_err(issue)?;
    let (committed, ()) = config::mutate(path, |cfg| {
        let prior = saved_value(cfg, extension, name)?;
        if prior.as_ref() != expected {
            return Err(Error::Conflict(
                "extension setting changed since expected revision; reload",
            ));
        }
        cfg.extension_settings
            .entry(extension.to_owned())
            .or_default()
            .insert(
                name.into(),
                serde_json::to_value(&value)
                    .map_err(|_| Error::Storage("cannot encode typed extension setting"))?,
            );
        config::validate_extension_settings(&cfg.extension_settings)?;
        Ok(())
    })?;
    let user = saved_value(&committed, extension, name)?;
    registry
        .resolve(extension, name, None, None, user.as_ref())
        .map_err(issue)
}

/// Exact CAS removal reveals a default but never copies it into user config.
/// Resetting a current secret key from this presentation-only API is forbidden.
pub fn reset(
    registry: &SettingsRegistry,
    path: Option<&Path>,
    extension: &str,
    name: &str,
    expected: Option<&SettingValue>,
) -> Result<ResolvedSetting> {
    let definition = registry.definition(extension, name).map_err(issue)?;
    if matches!(definition.kind, SettingKind::Secret { .. }) {
        return Err(Error::Denied(
            "secret reset requires separate vault owner authorization",
        ));
    }
    let (_committed, ()) = config::mutate(path, |cfg| {
        let prior = saved_value(cfg, extension, name)?;
        if prior.as_ref() != expected {
            return Err(Error::Conflict(
                "extension setting changed before reset; reload",
            ));
        }
        if let Some(group) = cfg.extension_settings.get_mut(extension) {
            group.remove(name);
            if group.is_empty() {
                cfg.extension_settings.remove(extension);
            }
        }
        config::validate_extension_settings(&cfg.extension_settings)?;
        Ok(())
    })?;
    registry
        .resolve(extension, name, None, None, None)
        .map_err(issue)
}

/// Enumerate redacted *public* values of only currently registered settings.
/// No mystery/unregistered keys in config are exposed as an active manifest.
pub fn list(registry: &SettingsRegistry, path: Option<&Path>) -> Result<Vec<ResolvedSetting>> {
    let cfg = config::load(path)?;
    registry
        .definitions()
        .map(|definition| {
            if matches!(definition.kind, SettingKind::Secret { .. }) {
                return Ok(ResolvedSetting {
                    extension: definition.extension.clone(),
                    name: definition.name.clone(),
                    value: None,
                    source: aw_ui_extension::settings::SettingSource::Unset,
                });
            }
            let user = saved_value(&cfg, &definition.extension, &definition.name)?;
            registry
                .resolve(
                    &definition.extension,
                    &definition.name,
                    None,
                    None,
                    user.as_ref(),
                )
                .map_err(issue)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_ui_extension::settings::{SettingDefinition, SettingKind, SettingSource};
    use std::{fs, path::PathBuf};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            Self(crate::test_support::temp_dir("extension-settings"))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn registry() -> SettingsRegistry {
        SettingsRegistry::new([
            SettingDefinition {
                extension: "extension.foo".into(),
                name: "visibility".into(),
                kind: SettingKind::Boolean,
                default: Some(SettingValue::Boolean(false)),
                description: String::new(),
            },
            SettingDefinition {
                extension: "extension.foo".into(),
                name: "count".into(),
                kind: SettingKind::Integer { min: 0, max: 9 },
                default: Some(SettingValue::Integer(1)),
                description: String::new(),
            },
            SettingDefinition {
                extension: "extension.foo".into(),
                name: "secret".into(),
                kind: SettingKind::Secret { max_bytes: 200 },
                default: None,
                description: String::new(),
            },
        ])
        .unwrap()
    }
    #[test]
    fn aw_p6_8a_real_atomic_persistence_cas_reset_and_unrelated_merge() {
        let tmp = Temp::new();
        let path = tmp.0.join("config.toml");
        let reg = registry();
        let initial = set(
            &reg,
            Some(&path),
            "extension.foo",
            "visibility",
            None,
            SettingValue::Boolean(true),
        )
        .unwrap();
        assert_eq!(initial.source, SettingSource::User);
        assert!(path.exists());
        assert_eq!(
            resolve(&reg, Some(&path), "extension.foo", "visibility", None, None)
                .unwrap()
                .value,
            Some(SettingValue::Boolean(true))
        );
        assert!(
            set(
                &reg,
                Some(&path),
                "extension.foo",
                "visibility",
                None,
                SettingValue::Boolean(false)
            )
            .is_err(),
            "stale absent CAS may not overwrite current user choice"
        );
        assert!(
            set(
                &reg,
                Some(&path),
                "extension.foo",
                "count",
                None,
                SettingValue::Integer(10)
            )
            .is_err(),
            "manifest bounds enforced before disk mutation"
        );
        let snapshot = config::load(Some(&path)).unwrap();
        set(
            &reg,
            Some(&path),
            "extension.foo",
            "count",
            None,
            SettingValue::Integer(3),
        )
        .unwrap();
        let mut independent = snapshot.clone();
        independent.ui.show_ttft = !independent.ui.show_ttft;
        let merged = config::save_changes(Some(&path), &snapshot, &independent).unwrap();
        assert_eq!(
            merged.extension_settings["extension.foo"].len(),
            2,
            "existing save_changes cannot overwrite independent extension mutations"
        );
        let count_before = Some(SettingValue::Integer(3));
        let revealed = reset(
            &reg,
            Some(&path),
            "extension.foo",
            "count",
            count_before.as_ref(),
        )
        .unwrap();
        assert_eq!(revealed.source, SettingSource::Default);
        assert_eq!(revealed.value, Some(SettingValue::Integer(1)));
        assert!(reset(&reg, Some(&path), "extension.foo", "visibility", None).is_err());
        let views = list(&reg, Some(&path)).unwrap();
        assert_eq!(views.len(), 3);
        assert!(views
            .iter()
            .find(|view| view.name == "secret")
            .unwrap()
            .value
            .is_none());
        let text = fs::read_to_string(path).unwrap();
        assert!(!text.contains("extension.foo.secret"));
        assert!(!text.contains("exfil"));
    }
    #[test]
    fn aw_p6_8a_source_repin_and_cli_priority_are_verified_each_read() {
        let tmp = Temp::new();
        let root = tmp.0.join("project");
        let settings = root.join(".agents/settings");
        fs::create_dir_all(&settings).unwrap();
        let filename = settings.join("extension.foo.visibility.json");
        fs::write(&filename, r#"{"type":"boolean","value":false}"#).unwrap();
        let path = tmp.0.join("config.toml");
        let reg = registry();
        set(
            &reg,
            Some(&path),
            "extension.foo",
            "visibility",
            None,
            SettingValue::Boolean(true),
        )
        .unwrap();
        assert!(
            resolve(
                &reg,
                Some(&path),
                "extension.foo",
                "visibility",
                None,
                Some((&root, &root))
            )
            .is_err(),
            "discovered project JSON is DATA until user approves exact fingerprint"
        );
        Catalog::scan(&root, &root)
            .unwrap()
            .trust(Some(&path))
            .unwrap();
        let project = resolve(
            &reg,
            Some(&path),
            "extension.foo",
            "visibility",
            None,
            Some((&root, &root)),
        )
        .unwrap();
        assert_eq!(project.source, SettingSource::TrustedProject);
        assert_eq!(project.value, Some(SettingValue::Boolean(false)));
        let cli = SettingValue::Boolean(true);
        assert_eq!(
            resolve(
                &reg,
                Some(&path),
                "extension.foo",
                "visibility",
                Some(&cli),
                Some((&root, &root))
            )
            .unwrap()
            .source,
            SettingSource::Cli
        );
        fs::write(&filename, r#"{"type":"boolean","value":true}"#).unwrap();
        assert!(
            resolve(
                &reg,
                Some(&path),
                "extension.foo",
                "visibility",
                Some(&cli),
                Some((&root, &root))
            )
            .is_err(),
            "CLI override cannot mask a revoked or changed pinned project file"
        );
        assert_eq!(
            resolve(&reg, Some(&path), "extension.foo", "visibility", None, None)
                .unwrap()
                .source,
            SettingSource::User
        );
        Catalog::revoke(&root, Some(&path)).unwrap();
        assert!(resolve(
            &reg,
            Some(&path),
            "extension.foo",
            "visibility",
            None,
            Some((&root, &root))
        )
        .is_err());
    }
    #[test]
    fn aw_p6_8a_secret_and_unregistered_data_never_become_active_grants() {
        let tmp = Temp::new();
        let path = tmp.0.join("config.toml");
        let reg = registry();
        assert!(set(
            &reg,
            Some(&path),
            "extension.foo",
            "secret",
            None,
            SettingValue::Text("bearer_should_never_persist".into())
        )
        .is_err());
        assert!(!path.exists());
        assert!(set(
            &reg,
            Some(&path),
            "unknown",
            "any",
            None,
            SettingValue::Boolean(true)
        )
        .is_err());
        assert!(!path.exists());
        assert!(reg
            .resolve(
                "extension.foo",
                "visibility",
                Some(&SettingValue::Text("not-a-bool".into())),
                None,
                None
            )
            .is_err());
    }
}
