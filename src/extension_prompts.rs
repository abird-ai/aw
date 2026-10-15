//! AW-P6-8A approved, opt-in prompt contributions over the SAME original
//! Agent builder. Extension metadata NEVER grants tools or provider access;
//! all project text is an inert, pinned *data reference*. There is no runtime
//! prompt mutation of an already journaled Session/Agent.
use crate::{config, project_resources::Catalog, Error, Result};
use aw_ui_extension::prompt::{Document, PromptError, PromptRegistry};
use std::{collections::BTreeMap, path::Path};

fn issue(error: PromptError) -> Error {
    match error {
        PromptError::Limit => Error::Limit("approved prompt contribution exceeds bounded size"),
        PromptError::InvalidContribution | PromptError::UnverifiedProjectSource => {
            Error::Denied("invalid or unapproved host prompt contribution source")
        }
    }
}
/// Resolve every project template through the existing no-follow Catalog
/// and re-check its *current* complete approved digest set against config.
/// Arbitrary paths passed by a model, UI extension, or JSON are not read.
pub fn compose(
    registry: &PromptRegistry,
    base: &Document,
    config_path: Option<&Path>,
    project: Option<(&Path, &Path)>,
) -> Result<String> {
    let sources = if registry.requires_project() {
        let (root, cwd) = project.ok_or(Error::Denied(
            "project prompt requires explicit approved root and cwd",
        ))?;
        let cfg = config::load(config_path)?;
        let fresh = Catalog::scan(root, cwd)?;
        fresh.verify(&cfg)?;
        Some(
            fresh
                .templates
                .into_iter()
                .map(|(name, source)| {
                    let digest = config::project_resource_digest(source.content.as_bytes());
                    (name, (digest, source.content))
                })
                .collect::<BTreeMap<_, _>>(),
        )
    } else {
        None
    };
    registry.render(base, sources.as_ref()).map_err(issue)
}

/// Explicit host-owned composition only, BEFORE the normal original
/// AgentBuilder is constructed. Does not add an extension Tool, register an
/// external source, update ToolPolicy or contact a provider.
pub fn configured_agent_builder(
    settings: &config::Settings,
    registry: &PromptRegistry,
    config_path: Option<&Path>,
    project: Option<(&Path, &Path)>,
) -> Result<crate::AgentBuilder> {
    let document = Document {
        preamble: settings.system_prompt.clone(),
        ..Document::default()
    };
    let rendered = compose(registry, &document, config_path, project)?;
    let mut effective = settings.clone();
    effective.system_prompt = rendered;
    crate::configured_agent_builder(&effective)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_ui_extension::prompt::{Anchor, Content, Placement, PromptContribution};
    use std::{fs, path::PathBuf};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "aw-8a-prompts-{}-{}",
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
    fn contribution(sha: String) -> PromptRegistry {
        PromptRegistry::new([PromptContribution {
            extension: "host.example".into(),
            id: "guide".into(),
            anchor: Anchor::Preamble,
            placement: Placement::After,
            priority: 1,
            content: Content::Project {
                name: "guide".into(),
                sha256: sha,
            },
        }])
        .unwrap()
    }
    #[test]
    fn aw_p6_8a_real_project_prompt_requires_fresh_hash_trust_and_revocation() {
        let tmp = Temp::new();
        let root = tmp.0.join("project");
        fs::create_dir_all(root.join(".agents/prompts")).unwrap();
        let file = root.join(".agents/prompts/guide.md");
        let body = "source data must not grant tools";
        fs::write(&file, body).unwrap();
        let reg = contribution(config::project_resource_digest(body.as_bytes()));
        let base = Document {
            preamble: "trusted system".into(),
            ..Document::default()
        };
        let path = tmp.0.join("config.toml");
        config::mutate(Some(&path), |_| Ok(())).unwrap();
        assert!(compose(&reg, &base, Some(&path), Some((&root, &root))).is_err());
        Catalog::scan(&root, &root)
            .unwrap()
            .trust(Some(&path))
            .unwrap();
        let output = compose(&reg, &base, Some(&path), Some((&root, &root))).unwrap();
        assert!(output.starts_with("trusted system"));
        assert!(output.contains("│ source data must not grant tools"));
        assert!(!output.contains("tool grant: enabled"));
        fs::write(&file, "source changed without approval").unwrap();
        assert!(
            compose(&reg, &base, Some(&path), Some((&root, &root))).is_err(),
            "prompt must not use stale cached hash-verified project text"
        );
        fs::write(&file, body).unwrap();
        Catalog::revoke(&root, Some(&path)).unwrap();
        assert!(
            compose(&reg, &base, Some(&path), Some((&root, &root))).is_err(),
            "revoked project trust must block rendered prompt even when bytes revert"
        );
    }
    #[test]
    fn aw_p6_8a_original_builder_receives_prompt_without_new_tool_grants() {
        let reg = PromptRegistry::new([PromptContribution {
            extension: "host.example".into(),
            id: "append".into(),
            anchor: Anchor::Preamble,
            placement: Placement::After,
            priority: 0,
            content: Content::Host {
                text: "verified host suffix".into(),
            },
        }])
        .unwrap();
        let mut profile = config::builtins().get("llama").unwrap().clone();
        profile.model = Some("fixture-model".into());
        let settings = config::Settings {
            profile_name: "llama".into(),
            profile,
            system_prompt: "original system".into(),
            request_timeout: std::time::Duration::from_secs(5),
            idle_timeout: std::time::Duration::from_secs(5),
            max_output_bytes: 4096,
        };
        let builder = configured_agent_builder(&settings, &reg, None, None).unwrap();
        let agent = builder.build().unwrap();
        assert_eq!(
            agent.options().system_prompt,
            "original system\n\nverified host suffix"
        );
        assert!(
            agent.tool_specs().is_empty(),
            "prompt registry never registers or grants model Tools"
        );
    }
}
