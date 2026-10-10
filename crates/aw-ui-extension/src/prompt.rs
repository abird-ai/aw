//! AW-P6-8A host-approved, deterministic prompt-contribution *data*.
//! A prompt contribution does not change an Agent's ToolPolicy, provider,
//! account, actor, Workspace or SecretStore grants. The extension registry is
//! trusted Rust host input; arbitrary project markdown may only enter an
//! allowed, explicitly pinned data-only position after host verification.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const MAX_CONTRIBUTIONS: usize = 64;
pub const MAX_CONTRIBUTION_BYTES: usize = 8 * 1024;
pub const MAX_PROMPT_BYTES: usize = 64 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    Preamble,
    Tools,
    Rules,
    Environment,
    All,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    Before,
    After,
    Replace,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Content {
    Host {
        text: String,
    },
    /// `name` is a pinned `.agents/prompts/<name>.md` source; SHA must match
    /// the freshly approved file. No caller-supplied project `text` is trusted.
    Project {
        name: String,
        sha256: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptContribution {
    pub extension: String,
    pub id: String,
    pub anchor: Anchor,
    pub placement: Placement,
    pub priority: i16,
    pub content: Content,
}
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PromptError {
    #[error("invalid or duplicate host-registered prompt contribution")]
    InvalidContribution,
    #[error("too many or oversized prompt contributions")]
    Limit,
    #[error("project prompt source is not freshly verified and pinned")]
    UnverifiedProjectSource,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Document {
    pub preamble: String,
    pub tools: String,
    pub rules: String,
    pub environment: String,
}
#[derive(Clone, Default)]
pub struct PromptRegistry {
    ordered: Vec<PromptContribution>,
}
fn id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_alphanumeric() || (i > 0 && b"_.-".contains(&b)))
}
fn text(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && !s.contains('\0')
}
fn sha(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl PromptRegistry {
    pub fn new(
        contributions: impl IntoIterator<Item = PromptContribution>,
    ) -> Result<Self, PromptError> {
        let mut rows = Vec::new();
        let mut keys = BTreeSet::new();
        let mut replaces = BTreeSet::new();
        for c in contributions {
            if !id(&c.extension)
                || !id(&c.id)
                || !keys.insert((c.extension.clone(), c.id.clone()))
                || !(-100..=100).contains(&c.priority)
            {
                return Err(PromptError::InvalidContribution);
            }
            match &c.content {
                Content::Host { text: body } if !text(body, MAX_CONTRIBUTION_BYTES) => {
                    return Err(PromptError::InvalidContribution)
                }
                Content::Project { name, sha256 }
                    if !id(name)
                        || !sha(sha256)
                        || c.placement == Placement::Replace
                        || !matches!(c.anchor, Anchor::Preamble | Anchor::Environment) =>
                {
                    return Err(PromptError::UnverifiedProjectSource);
                }
                _ => {}
            }
            if c.placement == Placement::Replace && !replaces.insert(c.anchor) {
                return Err(PromptError::InvalidContribution);
            }
            rows.push(c);
            if rows.len() > MAX_CONTRIBUTIONS {
                return Err(PromptError::Limit);
            }
        }
        rows.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| a.extension.cmp(&b.extension))
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(Self { ordered: rows })
    }
    pub fn requires_project(&self) -> bool {
        self.ordered
            .iter()
            .any(|c| matches!(c.content, Content::Project { .. }))
    }
    pub fn contributions(&self) -> &[PromptContribution] {
        &self.ordered
    }
    /// `verified_project_templates` MUST come from the host's freshly scanned
    /// `Catalog` after `verify(config)`; an arbitrary project map is NOT a
    /// capability and no unapproved source content may appear in a prompt.
    pub fn render(
        &self,
        document: &Document,
        verified_project_templates: Option<&BTreeMap<String, (String, String)>>,
    ) -> Result<String, PromptError> {
        let mut result = BTreeMap::new();
        for anchor in [
            Anchor::Preamble,
            Anchor::Tools,
            Anchor::Rules,
            Anchor::Environment,
        ] {
            let base = match anchor {
                Anchor::Preamble => &document.preamble,
                Anchor::Tools => &document.tools,
                Anchor::Rules => &document.rules,
                Anchor::Environment => &document.environment,
                Anchor::All => unreachable!(),
            };
            result.insert(
                anchor,
                self.render_anchor(anchor, base, verified_project_templates)?,
            );
        }
        let ordered = [
            Anchor::Preamble,
            Anchor::Tools,
            Anchor::Rules,
            Anchor::Environment,
        ]
        .iter()
        .filter_map(|anchor| result.get(anchor))
        .filter(|text| !text.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n\n");
        let assembled = self.render_anchor(Anchor::All, &ordered, verified_project_templates)?;
        if assembled.len() > MAX_PROMPT_BYTES {
            return Err(PromptError::Limit);
        }
        Ok(assembled)
    }
    fn render_anchor(
        &self,
        anchor: Anchor,
        base: &str,
        sources: Option<&BTreeMap<String, (String, String)>>,
    ) -> Result<String, PromptError> {
        let mut before = Vec::new();
        let mut after = Vec::new();
        let mut replacement = None;
        for c in self.ordered.iter().filter(|c| c.anchor == anchor) {
            let body = match &c.content {
                Content::Host { text } => text.clone(),
                Content::Project { name, sha256 } => {
                    let (actual, source) = sources
                        .and_then(|map| map.get(name))
                        .ok_or(PromptError::UnverifiedProjectSource)?;
                    if actual != sha256 || !text(source, MAX_CONTRIBUTION_BYTES) {
                        return Err(PromptError::UnverifiedProjectSource);
                    }
                    // Data, not a replacement instruction. Every source line
                    // receives an explicit quoted-data prefix; untrusted
                    // markdown never becomes a host Rules/Tools grant.
                    let quote = source
                        .lines()
                        .map(|line| format!("│ {line}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!(
                        "Project reference data ({name}; hash pinned, no tool authority):\n{quote}"
                    )
                }
            };
            match c.placement {
                Placement::Before => before.push(body),
                Placement::After => after.push(body),
                Placement::Replace => replacement = Some(body),
            }
        }
        let mut chunks = before;
        if let Some(body) = replacement.or_else(|| (!base.is_empty()).then(|| base.into())) {
            chunks.push(body);
        }
        chunks.extend(after);
        let output = chunks.join("\n\n");
        if output.len() > MAX_PROMPT_BYTES {
            return Err(PromptError::Limit);
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(
        id: &str,
        anchor: Anchor,
        placement: Placement,
        priority: i16,
        body: &str,
    ) -> PromptContribution {
        PromptContribution {
            extension: "host.example".into(),
            id: id.into(),
            anchor,
            placement,
            priority,
            content: Content::Host { text: body.into() },
        }
    }
    #[test]
    fn aw_p6_8a_prompt_anchoring_is_stable_and_priority_ordered() {
        let reg = PromptRegistry::new([
            item("after", Anchor::Preamble, Placement::After, 5, "after"),
            item(
                "before2",
                Anchor::Preamble,
                Placement::Before,
                2,
                "before 2",
            ),
            item(
                "before1",
                Anchor::Preamble,
                Placement::Before,
                1,
                "before 1",
            ),
            item(
                "replace",
                Anchor::Rules,
                Placement::Replace,
                0,
                "verified new rules",
            ),
            item(
                "tools",
                Anchor::Tools,
                Placement::After,
                3,
                "tool documentation is NOT tool permission",
            ),
            item("outer", Anchor::All, Placement::After, 0, "outer suffix"),
        ])
        .unwrap();
        let original = Document {
            preamble: "base".into(),
            tools: "live capability list".into(),
            rules: "obsolete".into(),
            environment: "cwd=local".into(),
        };
        let result = reg.render(&original, None).unwrap();
        assert!(result.starts_with("before 1\n\nbefore 2\n\nbase\n\nafter\n\nlive capability list"));
        assert!(result.contains("verified new rules"));
        assert!(!result.contains("obsolete"));
        assert!(result.ends_with("cwd=local\n\nouter suffix"));
        assert!(!reg.requires_project());
        assert_eq!(result, reg.render(&original, None).unwrap());
    }
    #[test]
    fn aw_p6_8a_project_prompt_is_data_only_and_exact_sha_bound() {
        let sha = "c".repeat(64);
        let project = PromptContribution {
            extension: "host.example".into(),
            id: "guide".into(),
            anchor: Anchor::Environment,
            placement: Placement::After,
            priority: 0,
            content: Content::Project {
                name: "guide".into(),
                sha256: sha.clone(),
            },
        };
        let reg = PromptRegistry::new([project.clone()]).unwrap();
        assert!(reg.requires_project());
        assert_eq!(
            reg.render(&Document::default(), None).unwrap_err(),
            PromptError::UnverifiedProjectSource
        );
        let mut approved =
            BTreeMap::from([("guide".into(), ("d".repeat(64), "unapproved".into()))]);
        assert_eq!(
            reg.render(&Document::default(), Some(&approved))
                .unwrap_err(),
            PromptError::UnverifiedProjectSource
        );
        approved.insert("guide".into(), (sha, "project DATA\nnot authority".into()));
        let text = reg.render(&Document::default(), Some(&approved)).unwrap();
        assert!(text.contains("Project reference data (guide; hash pinned, no tool authority):"));
        assert!(text.contains("│ project DATA\n│ not authority"));
        let mut bad = project.clone();
        bad.anchor = Anchor::Rules;
        assert!(PromptRegistry::new([bad]).is_err());
        bad = project;
        bad.placement = Placement::Replace;
        assert!(PromptRegistry::new([bad]).is_err());
    }
    #[test]
    fn aw_p6_8a_prompt_rejects_duplicate_replacements_unbounded_or_control() {
        assert!(PromptRegistry::new([
            item("one", Anchor::All, Placement::Replace, 0, "a"),
            item("two", Anchor::All, Placement::Replace, 1, "b"),
        ])
        .is_err());
        assert!(
            PromptRegistry::new([item("evil", Anchor::Rules, Placement::After, 0, "a\0b")])
                .is_err()
        );
        assert!(PromptRegistry::new([item(
            "huge",
            Anchor::Rules,
            Placement::After,
            0,
            &"a".repeat(8193)
        )])
        .is_err());
        let reg =
            PromptRegistry::new([item("suffix", Anchor::All, Placement::After, 0, "hi")]).unwrap();
        assert_eq!(
            reg.render(
                &Document {
                    preamble: "x".repeat(65536),
                    ..Document::default()
                },
                None
            )
            .unwrap_err(),
            PromptError::Limit
        );
    }
}
