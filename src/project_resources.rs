//! Explicit, source-pinned project resources. Discovery reads only beneath a
//! caller-selected canonical project root via aw's existing descriptor-relative,
//! no-follow, hardlink-rejecting scoped filesystem. Discovery is inert data;
//! user-approved digests gate admission and never grant tools/provider access.
use crate::{config, Error, Result};
use aw_tools::browse::{EntryKind, Listing, WorkspaceFiles};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};

const MAX_SOURCES: usize = 96;
const MAX_TOTAL_BYTES: usize = 512 * 1024;
const MAX_SOURCE_BYTES: usize = 64 * 1024;
const MAX_DEPTH: usize = 24;
const MAX_VARS: usize = 16;

#[derive(Clone, Debug)]
pub struct Source {
    pub path: String,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct Catalog {
    pub root: PathBuf,
    pub approved_files: BTreeMap<String, String>,
    pub instructions: Vec<Source>,
    pub templates: BTreeMap<String, Source>,
    /// Pinned, inert `.agents/settings/<extension>.<name>.json` values.
    /// These are NOT host/provider/tool grants; a typed host registry still
    /// validates the value and the project trust must be current every read.
    pub extension_settings: BTreeMap<String, Source>,
    pub skills: BTreeMap<String, Source>,
    pub themes: BTreeMap<String, Source>,
}

fn directory(files: &WorkspaceFiles, path: &str) -> Result<Listing> {
    let listing = files.list(path)?;
    if listing.incomplete {
        return Err(Error::Denied(
            "project directory scan is incomplete; refusing to pin partial trust",
        ));
    }
    Ok(listing)
}

fn optional_directory(
    files: &WorkspaceFiles,
    parent: &Listing,
    name: &str,
) -> Result<Option<Listing>> {
    match parent.entries.iter().find(|entry| entry.name == name) {
        None => Ok(None),
        Some(entry) if entry.kind == EntryKind::Directory => {
            Ok(Some(directory(files, &entry.path)?))
        }
        Some(_) => Err(Error::Denied(
            "project resource directory cannot be symlinked or unsupported",
        )),
    }
}

fn pin(
    files: &WorkspaceFiles,
    catalog: &mut Catalog,
    path: &str,
    total: &mut usize,
) -> Result<Source> {
    if catalog.approved_files.len() >= MAX_SOURCES {
        return Err(Error::Limit("project resource count exceeds 96"));
    }
    let content = files.read(path)?;
    if content.len() > MAX_SOURCE_BYTES {
        return Err(Error::Limit("project resource exceeds 64 KiB"));
    }
    *total = total.saturating_add(content.len());
    if *total > MAX_TOTAL_BYTES {
        return Err(Error::Limit("project resources exceed 512 KiB"));
    }
    if content.contains('\0') {
        return Err(Error::Denied(
            "NUL bytes not supported in project text resources",
        ));
    }
    if catalog
        .approved_files
        .insert(
            path.to_owned(),
            config::project_resource_digest(content.as_bytes()),
        )
        .is_some()
    {
        return Err(Error::Protocol("duplicate project resource path"));
    }
    Ok(Source {
        path: path.into(),
        content,
    })
}

impl Catalog {
    pub fn scan(project: &Path, cwd: &Path) -> Result<Self> {
        let root = project.canonicalize()?;
        // Config authorization keys are strings; lossy conversion would
        // collapse distinct non-UTF-8 filesystem names into one grant.
        if root.to_str().is_none() {
            return Err(Error::Denied("project trust root must be UTF-8"));
        }
        if !root.is_dir() {
            return Err(Error::Denied("project trust root must be a directory"));
        }
        let cwd = cwd.canonicalize()?;
        let subpath = cwd.strip_prefix(&root).map_err(|_| {
            Error::Denied(
                "project resource root must be an ancestor of the active working directory",
            )
        })?;
        let mut components = Vec::<String>::new();
        for component in subpath.components() {
            match component {
                Component::Normal(name) => {
                    let value = name
                        .to_str()
                        .ok_or(Error::Denied("project resource directory must be UTF-8"))?;
                    if value.is_empty()
                        || value == "."
                        || value == ".."
                        || value.contains('\\')
                        || value.chars().any(char::is_control)
                    {
                        return Err(Error::Denied("invalid project relative component"));
                    }
                    components.push(value.to_owned());
                }
                _ => {
                    return Err(Error::Denied(
                        "project ancestry must contain normal components",
                    ))
                }
            }
        }
        if components.len() > MAX_DEPTH {
            return Err(Error::Limit(
                "project instruction ancestry exceeds 24 directories",
            ));
        }
        let files = WorkspaceFiles::open(&root)?;
        let mut catalog = Self {
            root,
            approved_files: BTreeMap::new(),
            instructions: Vec::new(),
            templates: BTreeMap::new(),
            extension_settings: BTreeMap::new(),
            skills: BTreeMap::new(),
            themes: BTreeMap::new(),
        };
        let mut total = 0usize;
        let mut listing = directory(&files, "")?;
        // Farther ancestors first, nearer path can refine instructions, but
        // every file is still ordinary preapproved instruction data, not grant.
        for depth in 0..=components.len() {
            for name in ["AGENTS.md", "CLAUDE.md", "AGENTS.override.md"] {
                if let Some(entry) = listing.entries.iter().find(|entry| entry.name == name) {
                    if entry.kind != EntryKind::File {
                        return Err(Error::Denied(
                            "project instruction source is not an ordinary file",
                        ));
                    }
                    let source = pin(&files, &mut catalog, &entry.path, &mut total)?;
                    catalog.instructions.push(source);
                }
            }
            if let Some(child) = components.get(depth) {
                let entry = listing
                    .entries
                    .iter()
                    .find(|entry| &entry.name == child)
                    .ok_or(Error::Denied("project ancestor changed while scanning"))?;
                if entry.kind != EntryKind::Directory {
                    return Err(Error::Denied(
                        "project ancestor is symlinked or unsupported",
                    ));
                }
                listing = directory(&files, &entry.path)?;
            }
        }
        // Named templates, skills and themes are deliberately rooted at the
        // explicit project root, not implicitly loaded from arbitrary subtrees.
        let top = directory(&files, "")?;
        if let Some(agents) = optional_directory(&files, &top, ".agents")? {
            if let Some(prompts) = optional_directory(&files, &agents, "prompts")? {
                for entry in prompts.entries {
                    if let Some(name) = entry.name.strip_suffix(".md") {
                        config::validate_skill_name(name)?;
                        if entry.kind != EntryKind::File {
                            return Err(Error::Denied("project prompt is not an ordinary file"));
                        }
                        let source = pin(&files, &mut catalog, &entry.path, &mut total)?;
                        if catalog.templates.insert(name.to_owned(), source).is_some() {
                            return Err(Error::Config("duplicate project prompt".into()));
                        }
                    }
                }
            }
            if let Some(settings) = optional_directory(&files, &agents, "settings")? {
                for entry in settings.entries {
                    let Some(name) = entry.name.strip_suffix(".json") else {
                        return Err(Error::Denied(
                            "project extension setting must be named <extension>.<name>.json",
                        ));
                    };
                    if entry.kind != EntryKind::File || !name.contains('.') {
                        return Err(Error::Denied(
                            "project extension setting must be one ordinary named file",
                        ));
                    }
                    let (extension, setting) = name
                        .rsplit_once('.')
                        .ok_or(Error::Denied("invalid extension setting name"))?;
                    config::validate_skill_name(extension)?;
                    config::validate_skill_name(setting)?;
                    let source = pin(&files, &mut catalog, &entry.path, &mut total)?;
                    if source.content.len() > 4096 {
                        return Err(Error::Limit("project extension setting JSON exceeds 4KiB"));
                    }
                    if catalog
                        .extension_settings
                        .insert(name.to_owned(), source)
                        .is_some()
                    {
                        return Err(Error::Denied("duplicate project extension setting"));
                    }
                }
            }
            if let Some(skills) = optional_directory(&files, &agents, "skills")? {
                for entry in skills.entries {
                    if entry.kind != EntryKind::Directory {
                        return Err(Error::Denied("project skill directory is unsupported"));
                    }
                    config::validate_skill_name(&entry.name)?;
                    let inside = directory(&files, &entry.path)?;
                    if let Some(document) =
                        inside.entries.iter().find(|item| item.name == "SKILL.md")
                    {
                        if document.kind != EntryKind::File {
                            return Err(Error::Denied("SKILL.md must be an ordinary file"));
                        }
                        let source = pin(&files, &mut catalog, &document.path, &mut total)?;
                        parse_skill(&entry.name, &source.content)?;
                        catalog.skills.insert(entry.name, source);
                    }
                }
            }
            if let Some(themes) = optional_directory(&files, &agents, "themes")? {
                for entry in themes.entries {
                    if let Some(name) = entry.name.strip_suffix(".json") {
                        config::validate_skill_name(name)?;
                        if entry.kind != EntryKind::File {
                            return Err(Error::Denied("theme must be an ordinary JSON file"));
                        }
                        let source = pin(&files, &mut catalog, &entry.path, &mut total)?;
                        // Typed parse at discovery, before any display/theme mutation.
                        let parsed: NamedTheme = serde_json::from_str(&source.content)
                            .map_err(|_| Error::Config("invalid typed named-theme JSON".into()))?;
                        parsed.validate(name)?;
                        catalog.themes.insert(name.to_owned(), source);
                    }
                }
            }
        }
        if catalog.approved_files.is_empty() {
            return Err(Error::Denied(
                "selected project contains no approved resource candidates",
            ));
        }
        Ok(catalog)
    }

    pub fn trust(&self, file: Option<&Path>) -> Result<()> {
        let root = self
            .root
            .to_str()
            .ok_or(Error::Denied("project trust root must be UTF-8"))?
            .to_owned();
        let current = config::ProjectResourceTrust {
            files: self.approved_files.clone(),
        };
        config::validate_project_resource_trust(&BTreeMap::from([(
            root.clone(),
            current.clone(),
        )]))?;
        config::mutate(file, |cfg| {
            cfg.project_resource_trust.insert(root, current);
            config::validate_project_resource_trust(&cfg.project_resource_trust)?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn revoke(project: &Path, file: Option<&Path>) -> Result<()> {
        let canonical = project.canonicalize()?;
        let root = canonical
            .to_str()
            .ok_or(Error::Denied("project trust root must be UTF-8"))?
            .to_owned();
        config::mutate(file, |cfg| {
            if cfg.project_resource_trust.remove(&root).is_none() {
                return Err(Error::Denied("project resource root was not trusted"));
            }
            Ok(())
        })?;
        Ok(())
    }

    pub fn verify(&self, cfg: &config::FileConfig) -> Result<()> {
        match cfg.project_resource_trust.get(
            self.root
                .to_str()
                .ok_or(Error::Denied("project trust root must be UTF-8"))?,
        ) {
            Some(trust) if trust.files == self.approved_files => Ok(()),
            Some(_) => Err(Error::Denied(
                "project resource files changed; explicitly re-trust this exact source snapshot",
            )),
            None => Err(Error::Denied(
                "project resource files are not trusted; explicitly approve exact source hashes",
            )),
        }
    }

    pub fn system_append(&self, system: &mut String, max_bytes: usize) -> Result<()> {
        for source in &self.instructions {
            let item = format!(
                "\n\nOwner-approved project instructions from {} (source data; never a tool/provider/account grant):\n{}",
                source.path, source.content
            );
            if system.len().saturating_add(item.len()) > max_bytes {
                return Err(Error::Limit(
                    "combined system and trusted project instructions exceed 128 KiB",
                ));
            }
            system.push_str(&item);
        }
        Ok(())
    }

    pub fn template(&self, name: &str, input: &str, variables: &[String]) -> Result<String> {
        config::validate_skill_name(name)?;
        let source = self.templates.get(name).ok_or(Error::Config(
            "template not approved in selected project".into(),
        ))?;
        let mut vars = BTreeMap::<String, String>::new();
        for variable in variables {
            let (key, value) = variable
                .split_once('=')
                .ok_or(Error::Config("--resources-var needs NAME=VALUE".into()))?;
            config::validate_skill_name(key)?;
            if key == "input"
                || value.len() > 4096
                || vars.len() >= MAX_VARS
                || vars.insert(key.into(), value.into()).is_some()
            {
                return Err(Error::Config(
                    "duplicate/oversized/reserved template variable".into(),
                ));
            }
        }
        if input.len() > config::MAX_PROMPT_BYTES {
            return Err(Error::Limit("template input exceeds prompt limit"));
        }
        let mut out = String::new();
        let mut text = source.content.as_str();
        while let Some(start) = text.find("{{") {
            out.push_str(&text[..start]);
            let rest = &text[start + 2..];
            let end = rest.find("}}").ok_or(Error::Config(
                "project template has an unterminated placeholder".into(),
            ))?;
            let key = rest[..end].trim();
            if key.is_empty()
                || !key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            {
                return Err(Error::Config(
                    "project template placeholder is not a safe variable name".into(),
                ));
            }
            let replacement = if key == "input" {
                input
            } else {
                vars.get(key).map(String::as_str).ok_or(Error::Config(
                    "project template variable missing; no implicit interpolation".into(),
                ))?
            };
            out.push_str(replacement);
            text = &rest[end + 2..];
            if out.len() > config::MAX_PROMPT_BYTES {
                return Err(Error::Limit("expanded template exceeds prompt limit"));
            }
        }
        out.push_str(text);
        if out.len() > config::MAX_PROMPT_BYTES {
            return Err(Error::Limit("expanded template exceeds prompt limit"));
        }
        if out.trim().is_empty() {
            return Err(Error::Config("expanded prompt template is empty".into()));
        }
        Ok(out)
    }

    pub fn skill(&self, name: &str) -> Result<String> {
        config::validate_skill_name(name)?;
        let text = &self
            .skills
            .get(name)
            .ok_or(Error::Config(
                "project skill not found in the approved snapshot".into(),
            ))?
            .content;
        let (_, body) = parse_skill(name, text)?;
        Ok(body.to_owned())
    }

    pub fn theme(&self, name: &str) -> Result<NamedTheme> {
        config::validate_skill_name(name)?;
        let source = self
            .themes
            .get(name)
            .ok_or(Error::Config("named project theme was not approved".into()))?;
        let parsed: NamedTheme = serde_json::from_str(&source.content)
            .map_err(|_| Error::Config("invalid named theme JSON".into()))?;
        parsed.validate(name)?;
        Ok(parsed)
    }
}

fn parse_skill<'a>(name: &str, content: &'a str) -> Result<(Option<String>, &'a str)> {
    let Some(after) = content.strip_prefix("---\n") else {
        return Ok((None, content));
    };
    let (head, tail) = after.split_once("\n---\n").ok_or(Error::Config(
        "SKILL.md frontmatter must have a closing --- line".into(),
    ))?;
    if head.len() > 4096 {
        return Err(Error::Limit("SKILL.md frontmatter exceeds 4 KiB"));
    }
    let mut description = None;
    let mut seen = BTreeSet::new();
    for line in head.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or(Error::Config("unsupported SKILL.md metadata line".into()))?;
        let key = key.trim();
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
        if !matches!(key, "name" | "description") || !seen.insert(key) {
            return Err(Error::Config(
                "SKILL.md metadata only permits unique name/description".into(),
            ));
        }
        match key {
            "name" if value != name => {
                return Err(Error::Denied(
                    "SKILL.md name differs from explicitly selected directory name",
                ))
            }
            "description" if value.len() > 512 || value.chars().any(char::is_control) => {
                return Err(Error::Limit("SKILL.md description too long"))
            }
            "description" => description = Some(value.to_owned()),
            _ => {}
        }
    }
    if tail.trim().is_empty() {
        return Err(Error::Config("SKILL.md instruction body is empty".into()));
    }
    Ok((description, tail))
}

/// Only named semantic slots are accepted; no scripts/terminal escapes or
/// arbitrary raw CSS/rules. Palette rendering remains one existing UI authority.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NamedTheme {
    pub name: String,
    pub base: config::ThemeMode,
    /// Optional semantic accent only, hex sRGB, not a terminal control.
    #[serde(default)]
    pub accent: Option<String>,
}
impl NamedTheme {
    fn validate(&self, expected: &str) -> Result<()> {
        if self.name != expected {
            return Err(Error::Denied(
                "theme filename differs from typed theme name",
            ));
        }
        if let Some(accent) = &self.accent {
            if accent.len() != 7
                || !accent.starts_with('#')
                || !accent.bytes().skip(1).all(|b| b.is_ascii_hexdigit())
            {
                return Err(Error::Config("theme accent must be #RRGGBB".into()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn setup() -> PathBuf {
        let root = crate::test_support::temp_dir("project-resources");
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::create_dir_all(root.join(".agents/prompts")).unwrap();
        fs::create_dir_all(root.join(".agents/skills/review")).unwrap();
        fs::create_dir_all(root.join(".agents/themes")).unwrap();
        fs::write(root.join("AGENTS.md"), "root instruction").unwrap();
        fs::write(root.join("sub/AGENTS.md"), "nested instruction").unwrap();
        fs::write(
            root.join(".agents/prompts/review.md"),
            "Review {{input}} in {{language}}",
        )
        .unwrap();
        fs::write(
            root.join(".agents/skills/review/SKILL.md"),
            "---\nname: review\ndescription: read-only review\n---\nReview carefully.",
        )
        .unwrap();
        fs::write(
            root.join(".agents/themes/ocean.json"),
            r##"{"name":"ocean","base":"dark","accent":"#66aaff"}"##,
        )
        .unwrap();
        root
    }
    #[test]
    fn project_trust_is_hash_bound_revocable_and_ancestor_ordered() {
        let root = setup();
        let config_path = root.join("user_config.toml");
        let catalog = Catalog::scan(&root, &root.join("sub")).unwrap();
        assert_eq!(catalog.instructions.len(), 2);
        assert!(catalog.instructions[0].path.ends_with("AGENTS.md"));
        assert!(catalog.instructions[1].path.ends_with("sub/AGENTS.md"));
        assert!(catalog.verify(&config::FileConfig::default()).is_err());
        catalog.trust(Some(&config_path)).unwrap();
        let cfg = config::load(Some(&config_path)).unwrap();
        catalog.verify(&cfg).unwrap();
        let mut sys = "host instructions".to_owned();
        catalog.system_append(&mut sys, 4096).unwrap();
        assert!(sys.find("root instruction") < sys.find("nested instruction"));
        assert_eq!(
            catalog
                .template("review", "code", &["language=rust".into()])
                .unwrap(),
            "Review code in rust"
        );
        assert!(catalog.template("review", "code", &[]).is_err());
        assert_eq!(catalog.skill("review").unwrap(), "Review carefully.");
        let theme = catalog.theme("ocean").unwrap();
        assert_eq!(theme.base, config::ThemeMode::Dark);
        fs::write(root.join("sub/AGENTS.md"), "tampered instructions").unwrap();
        assert!(Catalog::scan(&root, &root.join("sub"))
            .unwrap()
            .verify(&cfg)
            .is_err());
        Catalog::revoke(&root, Some(&config_path)).unwrap();
        assert!(config::load(Some(&config_path))
            .unwrap()
            .project_resource_trust
            .is_empty());
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn discovery_rejects_symlinked_project_sources_and_path_escape() {
        let root = setup();
        assert!(Catalog::scan(&root.join("sub"), &root).is_err());
        #[cfg(unix)]
        {
            let source = root.join("AGENTS.md");
            fs::remove_file(&source).unwrap();
            std::os::unix::fs::symlink("/etc/passwd", &source).unwrap();
            assert!(Catalog::scan(&root, &root.join("sub")).is_err());
        }
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn missing_or_oversized_source_and_template_variables_fail_closed() {
        let root = setup();
        let prompt = root.join(".agents/prompts/review.md");
        fs::write(&prompt, "x".repeat(MAX_SOURCE_BYTES + 1)).unwrap();
        assert!(Catalog::scan(&root, &root).is_err());
        fs::write(&prompt, "{{missing}}").unwrap();
        let cat = Catalog::scan(&root, &root).unwrap();
        assert!(cat.template("review", "", &[]).is_err());
        assert!(cat
            .template("review", "", &["missing=a".into(), "missing=b".into()])
            .is_err());
        assert!(cat.template("review", "", &["missing=a".into()]).is_ok());
        let _ = fs::remove_dir_all(root);
    }
}
