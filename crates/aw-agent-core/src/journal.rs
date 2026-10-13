//! Host-supplied durability boundary. The SDK owns no database or files.
//! A successful acknowledgement must mean the record has been committed before
//! the next billable/model/tool action. Observational callbacks are NOT a journal.
use crate::{
    InvocationIdentity, InvocationRecord, Message, Result, ToolCall, ToolEffect, ToolRecord,
    ToolSource, ToolStatus, Turn,
};
use aw_agent_kernel::capability::{Source, SourceKind};
use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin};

pub type JournalFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

fn is_zero(value: &u64) -> bool {
    *value == 0
}

/// AW-P6-10E: exact original registered Tool source pinned alongside the
/// immutable ToolStarted intent. It is factual trusted-host journal evidence,
/// NEVER a bearer grant: ToolPolicy and Actor Envelope still independently
/// authorize the live effect. Legacy intents deserialize with `None` but do
/// not satisfy newer model-origin management provenance requirements.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSourceEvidence {
    pub kind: String,
    pub id: String,
    pub epoch: u64,
}
impl ToolSourceEvidence {
    pub fn from_source(source: &ToolSource) -> Self {
        Self {
            kind: match source.kind {
                SourceKind::Host => "host",
                SourceKind::Mcp => "mcp",
                SourceKind::Extension => "extension",
            }
            .into(),
            id: source.id.clone(),
            epoch: source.epoch,
        }
    }
    pub fn trusted_host() -> Self {
        Self::from_source(&ToolSource::trusted_host())
    }
    pub fn validate(&self) -> Result<()> {
        let kind = match self.kind.as_str() {
            "host" => SourceKind::Host,
            "mcp" => SourceKind::Mcp,
            "extension" => SourceKind::Extension,
            _ => return Err(crate::Error::Denied("unknown original tool source kind")),
        };
        Source::new(kind, &self.id, self.epoch)
            .map_err(|_| crate::Error::Denied("invalid immutable tool source identity"))?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TurnStart {
    pub turn_id: u64,
    pub prompt: String,
    pub identity: InvocationIdentity,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JournalEntry {
    InvocationStarted {
        ordinal: u32,
        identity: InvocationIdentity,
    },
    InvocationFinished {
        record: InvocationRecord,
    },
    Assistant {
        message: Message,
    },
    /// No-effect rejection of a complete provider-proposed call, recorded
    /// instead of a ToolStarted/Finished pair. Never implies an OS effect.
    ToolDenied {
        record: ToolRecord,
    },
    /// A host-authorized replacement of *arguments only*. Original Assistant
    /// provider proposal is immutable; the revised execution is independently
    /// checked against the same exact source, effect and grants.
    ToolRevised {
        original: ToolCall,
        revised: ToolCall,
        source_id: String,
        source_kind: String,
        source_epoch: u64,
        effect: ToolEffect,
    },
    ToolStarted {
        record: ToolRecord,
        /// Optional only so old private journal snapshots remain readable;
        /// missing provenance NEVER proves a newly requested model effect.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<ToolSourceEvidence>,
    },
    ToolFinished {
        record: ToolRecord,
    },
}

/// Validate an immutable no-effect rejection. There must be no ToolStarted
/// intent for this same exact call. The journal enforces the original
/// Assistant proposal and uniqueness before committing this event.
pub fn validate_denial(record: &ToolRecord) -> Result<()> {
    record.call.validate_envelope()?;
    if record.status != ToolStatus::Denied
        || record
            .output
            .as_ref()
            .is_none_or(|output| !output.is_error || output.truncated)
    {
        return Err(crate::Error::Denied("invalid no-effect tool denial record"));
    }
    record.output.as_ref().expect("checked").validate()?;
    Ok(())
}
pub fn validate_revision(
    original: &ToolCall,
    revised: &ToolCall,
    source_id: &str,
    source_kind: &str,
    source_epoch: u64,
) -> Result<()> {
    original.validate_envelope()?;
    revised.validate_envelope()?;
    if original.id != revised.id
        || original.name != revised.name
        || original.arguments == revised.arguments
        || source_id.is_empty()
        || source_id.len() > 128
        || source_epoch == 0
        || !matches!(source_kind, "host" | "mcp" | "extension")
    {
        return Err(crate::Error::Denied(
            "invalid tool argument revision identity",
        ));
    }
    Ok(())
}

/// Trusted host implementation. Return errors instead of silently dropping
/// records. Dropped futures/panics leave started records for explicit recovery;
/// replaying journal data must NEVER invoke providers or tools.
pub trait TurnJournal: Send + Sync {
    /// Current access/dependency gate before any host hook sees restored context.
    fn validate_context(&self) -> JournalFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
    /// Optional current-destination gate for derived context. This runs before
    /// hooks see the prepared history; every inference round is checked again.
    fn validate_context_as<'a>(
        &'a self,
        _identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, ()> {
        self.validate_context()
    }
    fn additional_context(&self) -> JournalFuture<'_, Vec<crate::ContextBlock>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn requires_output_gate(&self) -> JournalFuture<'_, bool> {
        Box::pin(async { Ok(false) })
    }
    /// Current persisted branch identity; no credentials or mutable user defaults.
    fn invocation_scope(&self) -> Result<Option<crate::InvocationScope>> {
        Ok(None)
    }

    fn begin<'a>(&'a self, start: TurnStart) -> JournalFuture<'a, ()>;
    fn append<'a>(&'a self, turn_id: u64, entry: JournalEntry) -> JournalFuture<'a, ()>;
    fn finish<'a>(&'a self, turn: Turn) -> JournalFuture<'a, ()>;
    fn acknowledge<'a>(&'a self, note: String) -> JournalFuture<'a, ()>;
}

/// Only an active ancestry/context projection, not the complete persisted tree.
/// No provider instance, environment name, endpoint, grant or credential is restored.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryState {
    pub version: u32,
    pub turns: Vec<Turn>,
    #[serde(default)]
    pub context_blocks: Vec<crate::ContextBlock>,
    pub next_turn_id: u64,
    pub summary: Option<String>,
    /// Last compacted attempt ID; only the active tail is in turns.
    /// A durable backend retains all original attempts in its immutable log.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub compacted_through: u64,
    /// Digest of the exact prior public evidence summarized into summary.
    /// Provenance evidence, never a restored capability/grant or instruction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_provenance_sha256: Option<String>,
    pub needs_reconciliation: bool,
}
impl Default for HistoryState {
    fn default() -> Self {
        Self {
            version: 2,
            context_blocks: Vec::new(),
            turns: Vec::new(),
            next_turn_id: 1,
            summary: None,
            compacted_through: 0,
            summary_provenance_sha256: None,
            needs_reconciliation: false,
        }
    }
}

#[cfg(test)]
mod r10_wire_tests {
    use super::*;
    #[test]
    fn old_history_wire_omits_new_checkpoint_metadata_until_compacted() {
        let original = HistoryState::default();
        let value = serde_json::to_value(&original).unwrap();
        let object = value.as_object().unwrap();
        assert!(!object.contains_key("compacted_through"));
        assert!(!object.contains_key("summary_provenance_sha256"));
        let restored: HistoryState = serde_json::from_value(value).unwrap();
        assert_eq!(restored.compacted_through, 0);
        assert!(restored.summary_provenance_sha256.is_none());
        let mut compacted = original;
        compacted.compacted_through = 12;
        compacted.summary = Some("unverified summarized history".into());
        compacted.summary_provenance_sha256 = Some("a".repeat(64));
        let value = serde_json::to_value(&compacted).unwrap();
        assert_eq!(value["compacted_through"], 12);
        let restored: HistoryState = serde_json::from_value(value).unwrap();
        assert_eq!(restored.compacted_through, 12);
        assert_eq!(restored.summary_provenance_sha256, Some("a".repeat(64)));
    }
}
