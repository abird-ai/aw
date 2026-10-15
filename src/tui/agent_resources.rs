//! Typed metadata for resources that are actually loaded into an Agent runtime.
//!
//! This module intentionally carries provenance captured at admission/setup time.
//! Rendering must never scan the filesystem or infer skills/resources from files.

use aw_agent_core::{ContextBlock, ContextBlockKind};
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SystemPromptOrigin {
    BuiltIn,
    Config,
    #[cfg(feature = "durable")]
    SavedSession,
    CommandLine,
}

impl SystemPromptOrigin {
    pub fn label(self) -> &'static str {
        match self {
            Self::BuiltIn => "built-in",
            Self::Config => "config",
            #[cfg(feature = "durable")]
            Self::SavedSession => "saved Session",
            Self::CommandLine => "--agent-system",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoadedContextKind {
    SystemPrompt,
    InstructionFile,
    Import,
    Compaction,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LoadedContextResource {
    pub kind: LoadedContextKind,
    pub source: String,
    pub bytes: usize,
    pub source_count: usize,
}

impl LoadedContextResource {
    fn system_prompt(origin: SystemPromptOrigin, bytes: usize) -> Self {
        Self {
            kind: LoadedContextKind::SystemPrompt,
            source: origin.label().into(),
            bytes,
            source_count: 0,
        }
    }

    fn instruction_file(source: String, bytes: usize) -> Self {
        Self {
            kind: LoadedContextKind::InstructionFile,
            source,
            bytes,
            source_count: 0,
        }
    }

    fn context_block(block: &ContextBlock) -> Self {
        Self {
            kind: match block.kind {
                ContextBlockKind::Import => LoadedContextKind::Import,
                ContextBlockKind::Compaction => LoadedContextKind::Compaction,
            },
            source: block.artifact_id.clone(),
            bytes: block.text.len(),
            source_count: block.sources.len(),
        }
    }

    pub fn label(&self) -> String {
        match self.kind {
            LoadedContextKind::SystemPrompt => {
                format!("system prompt · {} · {} B", self.source, self.bytes)
            }
            LoadedContextKind::InstructionFile => {
                format!("instructions · {} · {} B", self.source, self.bytes)
            }
            LoadedContextKind::Import => format!(
                "import · artifact {} · {} source{}",
                self.source,
                self.source_count,
                if self.source_count == 1 { "" } else { "s" }
            ),
            LoadedContextKind::Compaction => format!(
                "compaction · artifact {} · {} source{}",
                self.source,
                self.source_count,
                if self.source_count == 1 { "" } else { "s" }
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LoadedSkillResource {
    pub name: String,
    pub source: String,
    pub bytes: usize,
}

impl LoadedSkillResource {
    pub fn label(&self) -> String {
        format!("{} · {} · {} B", self.name, self.source, self.bytes)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct LoadedResourceSummary {
    context: Vec<LoadedContextResource>,
    skills: Vec<LoadedSkillResource>,
}

impl LoadedResourceSummary {
    pub fn with_system_prompt(origin: SystemPromptOrigin, bytes: usize) -> Self {
        let mut context = Vec::new();
        if bytes > 0 {
            context.push(LoadedContextResource::system_prompt(origin, bytes));
        }
        Self {
            context,
            skills: Vec::new(),
        }
    }

    pub fn add_instruction_file(&mut self, source: impl Into<String>, bytes: usize) {
        self.context.push(LoadedContextResource::instruction_file(
            source.into(),
            bytes,
        ));
    }

    pub fn add_skill(&mut self, name: impl Into<String>, source: impl Into<String>, bytes: usize) {
        self.skills.push(LoadedSkillResource { name: name.into(), source: source.into(), bytes });
    }

    pub fn with_context_blocks(&self, blocks: &[ContextBlock]) -> Self {
        let mut next = self.clone();
        next.context.retain(|resource| {
            !matches!(
                resource.kind,
                LoadedContextKind::Import | LoadedContextKind::Compaction
            )
        });
        next.context
            .extend(blocks.iter().map(LoadedContextResource::context_block));
        next
    }

    pub fn context(&self) -> &[LoadedContextResource] {
        &self.context
    }

    pub fn skills(&self) -> &[LoadedSkillResource] {
        &self.skills
    }

    pub fn fingerprint(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::EvidenceRef;

    fn block(kind: ContextBlockKind, artifact: &str) -> ContextBlock {
        ContextBlock {
            before_turn: 0,
            node_id: "node".into(),
            artifact_id: artifact.into(),
            kind,
            text: "derived context".into(),
            sources: vec![EvidenceRef {
                authority: "authority".into(),
                session_id: "session".into(),
                node_id: "source".into(),
                sha256: "a".repeat(64),
            }],
        }
    }

    #[test]
    fn runtime_summary_keeps_exact_instruction_source_names_without_contents() {
        let mut resources =
            LoadedResourceSummary::with_system_prompt(SystemPromptOrigin::Config, 120);
        resources.add_instruction_file("./AGENTS.md", 42);

        assert_eq!(resources.context().len(), 2);
        assert_eq!(
            resources.context()[0].label(),
            "system prompt · config · 120 B"
        );
        assert_eq!(
            resources.context()[1].label(),
            "instructions · ./AGENTS.md · 42 B"
        );
        assert!(resources.skills().is_empty());
    }

    #[test]
    fn selected_skill_is_typed_local_provenance_and_changes_fingerprint() {
        let mut resources = LoadedResourceSummary::with_system_prompt(SystemPromptOrigin::BuiltIn, 10);
        let before = resources.fingerprint();
        resources.add_skill("review", "./skills/review.md", 42);
        assert_ne!(resources.fingerprint(), before);
        assert_eq!(resources.skills().len(), 1);
        assert_eq!(resources.skills()[0].label(), "review · ./skills/review.md · 42 B");
    }

    #[test]
    fn durable_context_blocks_extend_runtime_provenance_and_change_fingerprint() {
        let resources = LoadedResourceSummary::with_system_prompt(SystemPromptOrigin::BuiltIn, 10);
        let before = resources.fingerprint();
        let resources = resources.with_context_blocks(&[
            block(ContextBlockKind::Import, "artifact_import"),
            block(ContextBlockKind::Compaction, "artifact_compaction"),
        ]);

        assert_ne!(resources.fingerprint(), before);
        assert_eq!(resources.context().len(), 3);
        assert_eq!(
            resources.context()[1].label(),
            "import · artifact artifact_import · 1 source"
        );
        assert_eq!(
            resources.context()[2].label(),
            "compaction · artifact artifact_compaction · 1 source"
        );
    }
}
