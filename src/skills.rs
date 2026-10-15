//! Explicit named skill admission for one Agent run.
//!
//! Skills are user-selected UTF-8 instruction resources. This module performs
//! no discovery and grants no tool/execution authority.

use aw::{config::SkillConfig, Error, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
    str::FromStr,
};

pub const MAX_SELECTED_SKILLS: usize = 16;
pub const MAX_SKILL_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillSpec {
    name: String,
    source: Option<PathBuf>,
}

impl SkillSpec {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    #[cfg(test)]
    pub fn is_registered_reference(&self) -> bool {
        self.source.is_none()
    }
}

impl FromStr for SkillSpec {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let (name, source) = match value.split_once('=') {
            Some((name, source)) => (name, Some(source)),
            None => (value, None),
        };
        aw::config::validate_skill_name(name).map_err(|_| {
            "skill name must be a portable 1..64 byte label starting with a letter or digit"
                .to_owned()
        })?;
        let source = match source {
            Some(source) if source.is_empty() || source.chars().any(char::is_control) => {
                return Err("skill path must be a non-empty path without control characters".into())
            }
            Some(source) => Some(PathBuf::from(source)),
            None => None,
        };
        Ok(Self {
            name: name.to_owned(),
            source,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedSkill {
    name: String,
    source: String,
    text: String,
    bytes: usize,
}

impl LoadedSkill {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    #[cfg(any(feature = "durable", test))]
    pub fn text(&self) -> &str {
        &self.text
    }
}

fn read_skill(path: &Path) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(Error::Config("skill source must be a regular file".into()));
    }
    let mut data = Vec::new();
    file.take((MAX_SKILL_BYTES + 1) as u64)
        .read_to_end(&mut data)?;
    if data.len() > MAX_SKILL_BYTES {
        return Err(Error::Limit("skill file exceeds 64 KiB"));
    }
    Ok(data)
}

pub fn load(
    specs: &[SkillSpec],
    registry: &BTreeMap<String, SkillConfig>,
) -> Result<Vec<LoadedSkill>> {
    if specs.len() > MAX_SELECTED_SKILLS {
        return Err(Error::Limit("at most 16 skills may be selected"));
    }
    let mut names = BTreeSet::new();
    let mut sources = BTreeSet::new();
    let mut loaded = Vec::with_capacity(specs.len());

    for spec in specs {
        aw::config::validate_skill_name(spec.name())?;
        if !names.insert(spec.name().to_owned()) {
            return Err(Error::Config("duplicate skill name".into()));
        }
        let source = match spec.source() {
            Some(path) => path.to_path_buf(),
            None => registry
                .get(spec.name())
                .ok_or_else(|| Error::Config(format!("unknown registered skill {}", spec.name())))?
                .path
                .clone(),
        };
        if !sources.insert(source.clone()) {
            return Err(Error::Config("duplicate skill source path".into()));
        }
        let bytes = read_skill(&source)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| Error::Config("skill file must be UTF-8".into()))?;
        loaded.push(LoadedSkill {
            name: spec.name().to_owned(),
            source: source.display().to_string(),
            bytes: text.len(),
            text,
        });
    }
    Ok(loaded)
}

pub fn append_to_system(
    system: &mut String,
    skills: &[LoadedSkill],
    max_total: usize,
) -> Result<()> {
    let additions = skills
        .iter()
        .map(|skill| {
            let header = format!("\n\nExplicit user-selected skill \"{}\":\n", skill.name);
            (header, skill)
        })
        .collect::<Vec<_>>();
    let added_bytes = additions.iter().fold(0usize, |total, (header, skill)| {
        total
            .saturating_add(header.len())
            .saturating_add(skill.text.len())
    });
    if system.len().saturating_add(added_bytes) > max_total {
        return Err(Error::Limit(
            "combined system/instructions/skills exceed 128 KiB",
        ));
    }
    for (header, skill) in additions {
        system.push_str(&header);
        system.push_str(&skill.text);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> PathBuf {
        crate::test_support::temp_dir("skills")
    }

    #[test]
    fn specs_require_portable_named_paths() {
        let spec: SkillSpec = "rust-review=./skills/rust.md".parse().unwrap();
        assert_eq!(spec.name(), "rust-review");
        assert_eq!(spec.source(), Some(Path::new("./skills/rust.md")));
        let registered: SkillSpec = "rust-review".parse().unwrap();
        assert!(registered.is_registered_reference());
        for invalid in [
            "=empty-name",
            ".hidden=skill.md",
            "bad/name=skill.md",
            "bad name=skill.md",
            "name=",
        ] {
            assert!(invalid.parse::<SkillSpec>().is_err(), "{invalid}");
        }
    }

    #[test]
    fn loader_rejects_duplicates_and_keeps_contents_local_to_loaded_skill() {
        let root = temp_dir();
        let path = root.join("review.md");
        fs::write(&path, "review carefully").unwrap();
        let spec = SkillSpec {
            name: "review".into(),
            source: Some(path.clone()),
        };
        let loaded = load(std::slice::from_ref(&spec), &BTreeMap::new()).unwrap();
        assert_eq!(loaded[0].name(), "review");
        assert_eq!(loaded[0].source(), path.display().to_string());
        assert_eq!(loaded[0].text(), "review carefully");
        assert!(load(&[spec.clone(), spec], &BTreeMap::new()).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn loader_enforces_count_unique_sources_utf8_and_per_skill_size() {
        let root = temp_dir();
        let path = root.join("skill.md");
        fs::write(&path, "ok").unwrap();
        let first = SkillSpec {
            name: "one".into(),
            source: Some(path.clone()),
        };
        let second = SkillSpec {
            name: "two".into(),
            source: Some(path.clone()),
        };
        assert!(load(&[first, second], &BTreeMap::new()).is_err());

        let many = (0..=MAX_SELECTED_SKILLS)
            .map(|index| SkillSpec {
                name: format!("skill{index}"),
                source: Some(root.join(format!("{index}.md"))),
            })
            .collect::<Vec<_>>();
        assert!(load(&many, &BTreeMap::new()).is_err());

        let invalid = root.join("invalid.md");
        fs::write(&invalid, [0xff, 0xfe]).unwrap();
        assert!(load(
            &[SkillSpec {
                name: "invalid".into(),
                source: Some(invalid)
            }],
            &BTreeMap::new()
        )
        .is_err());

        let oversized = root.join("oversized.md");
        fs::write(&oversized, vec![b'x'; MAX_SKILL_BYTES + 1]).unwrap();
        assert!(load(
            &[SkillSpec {
                name: "oversized".into(),
                source: Some(oversized)
            }],
            &BTreeMap::new()
        )
        .is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registered_reference_resolves_only_named_catalog_entry() {
        let root = temp_dir();
        let path = root.join("registered.md");
        fs::write(&path, "registered instructions").unwrap();
        let registry = BTreeMap::from([(
            "review".into(),
            SkillConfig {
                path: path.clone(),
                description: Some("Review".into()),
            },
        )]);
        let spec: SkillSpec = "review".parse().unwrap();
        let loaded = load(&[spec], &registry).unwrap();
        assert_eq!(loaded[0].source(), path.display().to_string());
        assert!(load(&["missing".parse().unwrap()], &registry).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn provider_envelope_uses_name_and_contents_but_not_local_source_path() {
        let skill = LoadedSkill {
            name: "review".into(),
            source: "/private/home/skills/review.md".into(),
            text: "check invariants".into(),
            bytes: "check invariants".len(),
        };
        let mut system = "base".to_owned();
        append_to_system(&mut system, &[skill], 1024).unwrap();
        assert!(system.contains("Explicit user-selected skill \"review\":"));
        assert!(system.contains("check invariants"));
        assert!(!system.contains("/private/home"));
    }

    #[test]
    fn combined_budget_is_checked_before_mutating_with_oversized_skill() {
        let skill = LoadedSkill {
            name: "review".into(),
            source: "review.md".into(),
            text: "x".repeat(20),
            bytes: 20,
        };
        let mut system = "base".to_owned();
        let before = system.clone();
        assert!(append_to_system(&mut system, &[skill], 10).is_err());
        assert_eq!(system, before);
    }
}
