//! Structural removal of one originally constructed materialization.
//! The original Applied descriptor is the release recipe; the existing
//! source-coordination claim fences its exact CAS until publication.
use super::{
    next_source_rev, publish_source_change_in, remove_binding_in,
    require_unshared_materializations_in, retained_in, SourceChange, VersionedSourceBinding,
};
use crate::{
    db, sandbox_owner, tool_root_key, Error, PrincipalId, Result, SandboxLifecycle,
    SandboxSnapshot, SessionStore, SourceMutationOutcome, SourceMutationSet, StoreMode,
};
use aw_agent_core::{NamedPreparedSource, SourceName};
use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use sha2::{Digest, Sha256};

pub(crate) const SOURCE_RELEASE_PREFIX: &str = "sandbox-source-release:";

// The existing sources.changed fact proves release of this exact original
// descriptor. No additional provenance table or operation ledger.
pub(super) fn release_event_id(sandbox: &str, source: &NamedPreparedSource) -> Result<String> {
    let descriptor = &source.prepared.descriptor;
    let identity = crate::json(&(
        sandbox,
        &source.name,
        &descriptor.driver,
        &descriptor.prepare_id,
        &descriptor.prepared_identity,
    ))?;
    let mut hash = Sha256::new();
    hash.update(b"aw-sandbox-original-source-release-v1\0");
    hash.update(identity.as_bytes());
    Ok(format!("sbx-release:{:x}", hash.finalize()))
}

fn released_in(c: &Connection, sandbox: &str, source: &NamedPreparedSource) -> Result<bool> {
    db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM commit_log WHERE event_id=?1
         AND entity_type='sandbox' AND entity_id=?2 AND kind='sandbox.sources.changed'
         AND json_extract(payload_json,'$.source_name')=?3
         AND json_extract(payload_json,'$.released')=1)
         OR EXISTS(SELECT 1 FROM commit_log
         WHERE entity_type='sandbox' AND entity_id=?2 AND kind='sandbox.sources.changed'
         AND json_extract(payload_json,'$.source_name')=?3
         AND json_extract(payload_json,'$.replaced_materialization')=?1)",
        params![
            release_event_id(sandbox, source)?,
            sandbox,
            source.name.as_str()
        ],
        |row| row.get(0),
    ))
}

/// Only exact retained original descriptors. Missing names need a factual
/// release event; an old metadata-only edit is not proof of deletion.
pub(super) fn retained_preparations_in(
    c: &Connection,
    sandbox: &str,
    bindings: &[VersionedSourceBinding],
) -> Result<Vec<NamedPreparedSource>> {
    let mut original = crate::construction_receipt::restored_sources_for_sandbox_in(c, sandbox)?;
    original
        .sources
        .sort_unstable_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
    let mut current = bindings.iter().peekable();
    for source in &original.sources {
        while current
            .peek()
            .is_some_and(|binding| binding.binding.name.as_str() < source.name.as_str())
        {
            current.next();
        }
        let retained = current
            .peek()
            .is_some_and(|binding| binding.binding.name == source.name && binding.binding_rev == 1);
        if retained == released_in(c, sandbox, source)? {
            return Err(Error::Conflict(
                "original materialization release evidence differs from retention",
            ));
        }
    }
    let mut retained = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let source = if binding.binding_rev == 1 {
            let index = original
                .sources
                .binary_search_by(|s| s.name.as_str().cmp(binding.binding.name.as_str()))
                .map_err(|_| Error::Conflict("original materialization unavailable"))?;
            original.sources[index].clone()
        } else {
            super::attachment::attached_preparation_in(c, sandbox, binding)?
        };
        if binding.binding.name != source.name
            || binding.binding.driver != source.prepared.descriptor.driver
            || released_in(c, sandbox, &source)?
        {
            return Err(Error::Conflict(
                "retained source differs from original materialization",
            ));
        }
        retained.push(source);
    }
    Ok(retained)
}

/// Trusted observation, not authority. Includes the whole retained layout
/// and the exact original descriptor at the selected index.
#[derive(PartialEq, Eq)]
pub(super) struct ManagedMutation {
    pub(super) snapshot: SandboxSnapshot,
    pub(super) bindings: Vec<VersionedSourceBinding>,
    pub(super) preparations: Vec<NamedPreparedSource>,
    pub(super) writer_epoch: u64,
}

#[derive(PartialEq, Eq)]
pub struct SandboxSourceRelease {
    layout: ManagedMutation,
    selected: usize,
}

impl SandboxSourceRelease {
    pub fn snapshot(&self) -> &SandboxSnapshot {
        &self.layout.snapshot
    }
    pub fn bindings(&self) -> &[VersionedSourceBinding] {
        &self.layout.bindings
    }
    pub fn preparations(&self) -> &[NamedPreparedSource] {
        &self.layout.preparations
    }
    pub fn selected(&self) -> usize {
        self.selected
    }

    fn event_id(&self) -> Result<String> {
        release_event_id(
            &self.layout.snapshot.canonical_ref,
            &self.layout.preparations[self.selected],
        )
    }

    fn prepare_id(&self) -> Result<String> {
        // All old bindings and their revision remain until settlement.
        Ok(format!(
            "{SOURCE_RELEASE_PREFIX}{}:{}",
            self.event_id()?,
            self.layout.snapshot.rev
        ))
    }
}

pub(super) fn observe_managed_in(
    store: &SessionStore,
    tx: &Transaction<'_>,
    actor: &PrincipalId,
    sandbox: &str,
    rev: u64,
) -> Result<ManagedMutation> {
    let key = sandbox_owner::parse_sandbox_ref(sandbox)?;
    sandbox_owner::authorize_project(tx, actor, &key.project, true)?;
    let (snapshot, bindings) = retained_in(tx, sandbox)?;
    if snapshot.rev != rev || snapshot.lifecycle != SandboxLifecycle::Open {
        return Err(Error::Conflict("Sandbox revision or lifecycle changed"));
    }
    next_source_rev(rev)?;
    if bindings.is_empty() {
        return Err(Error::Conflict(
            "managed mutation requires retained sources",
        ));
    }
    require_unshared_materializations_in(tx, &snapshot)?;
    let root = snapshot
        .tool_root
        .to_str()
        .ok_or(Error::Storage("invalid Sandbox tool root"))?;
    let cwd = snapshot
        .working_directory
        .to_str()
        .ok_or(Error::Storage("invalid Sandbox working directory"))?;
    sandbox_owner::require_no_retained_layout_in(tx, root, cwd)?;
    // Initial structural realization requires an unused isolated layout.
    let used: bool = db(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_state_current WHERE sandbox_id=?1)
         OR EXISTS(SELECT 1 FROM sandboxes_current WHERE sandbox_id<>?1 AND lifecycle='open'
                   AND (tool_root IN (?2,?3) OR working_directory IN (?2,?3)))",
        params![sandbox, root, cwd],
        |row| row.get(0),
    ))?;
    if used {
        return Err(Error::Conflict(
            "managed mutation requires an unused isolated Sandbox",
        ));
    }
    sandbox_owner::require_layout_quiescent_in(tx, sandbox, &tool_root_key(&snapshot.tool_root)?)?;
    if snapshot.working_directory != snapshot.tool_root {
        sandbox_owner::require_layout_quiescent_in(
            tx,
            sandbox,
            &tool_root_key(&snapshot.working_directory)?,
        )?;
    }
    let preparation = retained_preparations_in(tx, sandbox, &bindings)?;
    let registry = store.inner.source_drivers.reader()?;
    for (binding, source) in bindings.iter().zip(&preparation) {
        registry.resolve(&binding.binding.pin()?)?;
        registry.resolve(&source.prepared.descriptor.pin()?)?;
    }
    let writer_epoch = crate::workspace_writer::writer_row(tx, &key.project.project)?.writer_epoch;
    Ok(ManagedMutation {
        snapshot,
        bindings,
        preparations: preparation,
        writer_epoch,
    })
}

fn observe_in(
    store: &SessionStore,
    tx: &Transaction<'_>,
    actor: &PrincipalId,
    sandbox: &str,
    name: &SourceName,
    rev: u64,
) -> Result<SandboxSourceRelease> {
    let layout = observe_managed_in(store, tx, actor, sandbox, rev)?;
    let selected = layout
        .bindings
        .iter()
        .position(|binding| &binding.binding.name == name)
        .ok_or(Error::Conflict("original source is not retained"))?;
    Ok(SandboxSourceRelease { layout, selected })
}

impl SessionStore {
    /// Trusted effectful realization of sources.set(name, null) for an
    /// original managed materialization. Independent authorization and
    /// read-only preflight precede the durable exact-revision release
    /// claim. The caller reconciles only the original descriptor, with
    /// its physical/provider fence held through commit. Failed effects
    /// or publication retain the whole old binding set and hazard.
    pub fn sandbox_release_source<F>(
        &self,
        actor: &PrincipalId,
        sandbox_ref: &str,
        name: &SourceName,
        expected_rev: u64,
        authorize: impl FnOnce(&SandboxSourceRelease) -> Result<F>,
        reconcile: impl FnOnce(&SandboxSourceRelease, &F) -> Result<()>,
    ) -> Result<SandboxSnapshot> {
        if self.inner.mode != StoreMode::OwnerLock {
            return Err(Error::Denied(
                "source release requires the exclusive Store owner",
            ));
        }
        name.validate()?;
        let sandbox = sandbox_owner::parse_sandbox_ref(sandbox_ref)?.canonical();
        if sandbox != sandbox_ref {
            return Err(Error::Denied("source release requires canonical Sandbox"));
        }
        let observe =
            |tx: &Transaction<'_>| observe_in(self, tx, actor, &sandbox, name, expected_rev);
        let original = {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let observation = observe(&tx)?;
            db(tx.commit())?;
            observation
        };
        let fence = authorize(&original)?;
        let prepare_id = original.prepare_id()?;
        let barrier = {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            if observe(&tx)? != original {
                return Err(Error::Conflict(
                    "Sandbox source release changed during authorization",
                ));
            }
            let set = SourceMutationSet::from_binding_refs(
                original
                    .layout
                    .bindings
                    .iter()
                    .map(|binding| &binding.binding),
            )?;
            let barrier = self.reconcile_retained_sources_in(&tx, &set, &prepare_id)?;
            db(tx.commit())?;
            barrier
        };
        let finish = (|| {
            reconcile(&original, &fence)?;
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            barrier.require_preparation_in(&tx, &prepare_id)?;
            if observe(&tx)? != original {
                return Err(Error::Conflict(
                    "Sandbox source release changed before publication",
                ));
            }
            let binding = &original.layout.bindings[original.selected];
            remove_binding_in(&tx, &sandbox, name, binding.binding_rev)?;
            publish_source_change_in(
                &tx,
                actor,
                &sandbox,
                name,
                expected_rev,
                SourceChange::Released(&original.event_id()?),
            )?;
            barrier.settle_in(&tx, SourceMutationOutcome::Settled)?;
            let result = sandbox_owner::snapshot_with_sources(&tx, &sandbox)?;
            db(tx.commit())?;
            Ok(result)
        })();
        if finish.is_err() {
            barrier.settle(SourceMutationOutcome::Unknown)?;
        }
        drop(fence);
        finish
    }
}
