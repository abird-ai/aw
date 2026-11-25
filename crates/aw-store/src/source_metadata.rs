//! Independently authorized presentation of exact durable source identities.
//! No materialization, retention locator, grant, driver effect or new owner.
use crate::{label, store_scope::StoreScope, user_for_principal, Error, PrincipalId, Result};
use aw_agent_core::{
    validate_source_set_hard_limit, DriverKey, SourceAnchor, SourceBinding, SourceName,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Only the bounded identity projection, not the raw binding/anchor or
/// provider-specific recovery fields. Versions are immutable anchor identities.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceMetadataRef {
    pub name: SourceName,
    pub driver: DriverKey,
    pub schema: u32,
    pub source_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl SourceMetadataRef {
    fn validate(&self, anchor: bool) -> Result<()> {
        self.name.validate()?;
        self.driver.validate()?;
        if self.schema == 0 || self.version.is_some() != anchor {
            return Err(Error::Denied("invalid source metadata identity kind"));
        }
        for value in std::iter::once(&self.source_ref).chain(self.version.iter()) {
            label(value, 2048)?;
            if value.contains("://") || value.chars().any(char::is_control) {
                return Err(Error::Denied("source metadata requires opaque identities"));
            }
        }
        Ok(())
    }

    fn matches_binding(&self, binding: &SourceBinding) -> bool {
        self.name == binding.name
            && self.driver == binding.driver
            && self.schema == binding.binding_schema
            && self.source_ref == binding.source_ref
    }

    fn matches_anchor(&self, anchor: &SourceAnchor) -> bool {
        self.name == anchor.name
            && self.driver == anchor.driver
            && self.schema == anchor.anchor_schema
            && self.source_ref == anchor.source_ref
            && self.version.as_deref() == Some(anchor.identity.as_str())
    }
}

/// Explicit host/provider decision for one named source in one exact
/// resource. Sandbox revisions prevent a mutable binding following a name.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceMetadataSelection {
    Binding {
        sandbox: String,
        sandbox_rev: u64,
        binding_rev: u64,
        source: SourceMetadataRef,
    },
    Anchor {
        checkpoint: String,
        source: SourceMetadataRef,
    },
}

/// Trusted host policy input. Neither an Agent ACL nor a persisted grant
/// can manufacture this independent decision. The host must hold its
/// actual disclosure authority through the read and projection.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceMetadataSpec {
    pub store_id: String,
    pub store_generation: u64,
    pub actor: PrincipalId,
    pub actor_user_id: String,
    pub project: String,
    pub sources: Vec<SourceMetadataSelection>,
}

pub struct SourceMetadataApproval {
    store_id: String,
    store_generation: u64,
    actor: PrincipalId,
    actor_user_id: String,
    project: String,
    bindings: BTreeMap<String, BTreeMap<String, BindingSelection>>,
    anchors: BTreeMap<String, BTreeMap<String, SourceMetadataRef>>,
}

struct BindingSelection {
    sandbox_rev: u64,
    binding_rev: u64,
    source: SourceMetadataRef,
}

/// Not serializable: callers must deliberately render the normal redacted
/// snapshot plus only these independently approved source projections.
pub struct SourceMetadataView<T> {
    pub snapshot: T,
    pub disclosed: Vec<SourceMetadataRef>,
}

impl SourceMetadataApproval {
    pub fn new(spec: SourceMetadataSpec) -> Result<Self> {
        label(&spec.store_id, 128)?;
        label(&spec.actor_user_id, 128)?;
        crate::sandbox_owner::require_metadata_project_ref(&spec.project, None)?;
        if spec.store_generation == 0 {
            return Err(Error::Denied("invalid source metadata scope"));
        }
        validate_source_set_hard_limit(spec.sources.len())?;
        let mut bindings = BTreeMap::<String, BTreeMap<String, BindingSelection>>::new();
        let mut anchors = BTreeMap::<String, BTreeMap<String, SourceMetadataRef>>::new();
        for item in spec.sources {
            let duplicate = match item {
                SourceMetadataSelection::Binding {
                    sandbox,
                    sandbox_rev,
                    binding_rev,
                    source,
                } => {
                    source.validate(false)?;
                    crate::sandbox_owner::require_metadata_project_ref(
                        &spec.project,
                        Some(&sandbox),
                    )?;
                    if sandbox_rev == 0 || binding_rev == 0 {
                        return Err(Error::Denied(
                            "source disclosure requires exact Sandbox and binding revisions",
                        ));
                    }
                    bindings
                        .entry(sandbox)
                        .or_default()
                        .insert(
                            source.name.as_str().to_owned(),
                            BindingSelection {
                                sandbox_rev,
                                binding_rev,
                                source,
                            },
                        )
                        .is_some()
                }
                SourceMetadataSelection::Anchor { checkpoint, source } => {
                    label(&checkpoint, 128)?;
                    source.validate(true)?;
                    anchors
                        .entry(checkpoint)
                        .or_default()
                        .insert(source.name.as_str().to_owned(), source)
                        .is_some()
                }
            };
            if duplicate {
                return Err(Error::Denied("duplicate source metadata selection"));
            }
        }
        Ok(Self {
            store_id: spec.store_id,
            store_generation: spec.store_generation,
            actor: spec.actor,
            actor_user_id: spec.actor_user_id,
            project: spec.project,
            bindings,
            anchors,
        })
    }

    pub(crate) fn require_scope_in(&self, c: &Connection, actor: &PrincipalId) -> Result<()> {
        let scope = StoreScope::load(c)?;
        if scope.id().as_str() != self.store_id
            || scope.restore_generation() != self.store_generation
            || actor != &self.actor
            || user_for_principal(c, actor)?.as_deref() != Some(self.actor_user_id.as_str())
        {
            return Err(Error::Denied("source metadata approval scope changed"));
        }
        Ok(())
    }

    pub(crate) fn binding(
        &self,
        sandbox: &crate::SandboxSnapshot,
        binding: &SourceBinding,
        rev: u64,
    ) -> Option<SourceMetadataRef> {
        if sandbox.project_ref != self.project || sandbox.lifecycle != crate::SandboxLifecycle::Open
        {
            return None;
        }
        let selected = self
            .bindings
            .get(&sandbox.canonical_ref)?
            .get(binding.name.as_str())?;
        (selected.sandbox_rev == sandbox.rev
            && selected.binding_rev == rev
            && selected.source.matches_binding(binding))
        .then(|| selected.source.clone())
    }

    pub(crate) fn anchor(
        &self,
        checkpoint: &crate::CheckpointSnapshot,
        anchor: &SourceAnchor,
    ) -> Option<SourceMetadataRef> {
        if checkpoint
            .source_agent_ref
            .rsplit_once("/a:")
            .map(|(project, _)| project)
            != Some(self.project.as_str())
            || checkpoint.lifecycle != crate::CheckpointLifecycle::Live
        {
            return None;
        }
        let selected = self
            .anchors
            .get(&checkpoint.id)?
            .get(anchor.name.as_str())?;
        selected.matches_anchor(anchor).then(|| selected.clone())
    }
}
