//! Durable named bindings within the logical Sandbox owner.
use crate::{
    db, durability, now, tool_root_key, Error, PrincipalId, Result, SessionStore,
    SourceMutationSet, StoreMode,
};
use aw_agent_core::{
    validate_source_set_hard_limit, CheckpointPolicy, DriverKey, SourceAccess, SourceBinding,
    SourceDriverRegistry, SourceDriverRegistryReader, SourceName, SourceSetLimit,
    HARD_MAX_SOURCES_PER_SANDBOX,
};
use rusqlite::{params, Connection, Row, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub(crate) const RETENTION_CLEANUP_PREFIX: &str = "sandbox-retention:";

use crate::source_compatibility::compatibility;
pub use crate::source_compatibility::SourceCompatibility as SourceBindingCompatibility;

/// Project-authorized metadata only; opaque source identities and materialization
/// locators require their own source disclosure authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceBindingSummary {
    pub name: SourceName,
    pub driver: DriverKey,
    pub binding_schema: u32,
    pub access: SourceAccess,
    pub checkpoint_policy: CheckpointPolicy,
    pub binding_rev: u64,
    pub compatibility: SourceBindingCompatibility,
}

/// Exact durable binding revision, for trusted source planning. Neither the
/// binding nor its revision grants source access.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionedSourceBinding {
    pub binding: SourceBinding,
    pub binding_rev: u64,
}

impl VersionedSourceBinding {
    pub(crate) fn validate(&self) -> Result<()> {
        self.binding.validate()?;
        if self.binding_rev == 0 {
            return Err(Error::Storage("invalid durable SourceBinding revision"));
        }
        Ok(())
    }
}

struct StoredBinding {
    binding: SourceBinding,
    binding_rev: u64,
    compatibility: SourceBindingCompatibility,
}

impl StoredBinding {
    fn decode(row: &Row<'_>) -> Result<Self> {
        let name = SourceName::new(db(row.get::<_, String>(0))?)?;
        let driver: DriverKey = serde_json::from_str(&db(row.get::<_, String>(1))?)
            .map_err(|_| Error::Storage("invalid durable SourceDriver key"))?;
        let access = match db(row.get::<_, String>(5))?.as_str() {
            "ro" => SourceAccess::ReadOnly,
            "rw" => SourceAccess::ReadWrite,
            _ => return Err(Error::Storage("invalid durable SourceBinding access")),
        };
        let checkpoint_policy = match db(row.get::<_, String>(6))?.as_str() {
            "required" => CheckpointPolicy::Required,
            "ephemeral" => CheckpointPolicy::Ephemeral,
            _ => return Err(Error::Storage("invalid durable checkpoint policy")),
        };
        let binding = SourceBinding {
            name,
            driver,
            binding_schema: db(row.get(2))?,
            source_ref: db(row.get(3))?,
            materialization: db(row.get(4))?,
            access,
            checkpoint_policy,
        };
        binding.validate()?;
        let binding_rev = db(row.get::<_, u64>(7))?;
        if binding_rev == 0 {
            return Err(Error::Storage("invalid durable SourceBinding revision"));
        }
        Ok(Self {
            binding,
            binding_rev,
            compatibility: SourceBindingCompatibility::parse(&db(row.get::<_, String>(8))?)?,
        })
    }

    fn summary(self) -> SourceBindingSummary {
        SourceBindingSummary {
            name: self.binding.name,
            driver: self.binding.driver,
            binding_schema: self.binding.binding_schema,
            access: self.binding.access,
            checkpoint_policy: self.binding.checkpoint_policy,
            binding_rev: self.binding_rev,
            compatibility: self.compatibility,
        }
    }
}

pub(super) fn available(connection: &Connection) -> Result<bool> {
    let version: i64 = db(connection.pragma_query_value(None, "user_version", |row| row.get(0)))?;
    Ok(version >= 55)
}

pub(super) fn has_sources(connection: &Connection, sandbox: &str) -> Result<bool> {
    if !available(connection)? {
        return Ok(false);
    }
    db(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sandbox_source_bindings_current WHERE sandbox_id=?1)",
        [sandbox],
        |row| row.get(0),
    ))
}

fn bindings_in(connection: &Connection, sandbox: &str) -> Result<Vec<StoredBinding>> {
    let mut query = db(connection.prepare(
        "SELECT source_name,driver_key,binding_schema,source_ref,materialization,
                access,checkpoint_policy,binding_rev,compatibility
         FROM sandbox_source_bindings_current WHERE sandbox_id=?1
         ORDER BY source_name LIMIT ?2",
    ))?;
    let mut rows = db(query.query(params![sandbox, HARD_MAX_SOURCES_PER_SANDBOX + 1]))?;
    let mut bindings = Vec::new();
    while let Some(row) = db(rows.next())? {
        validate_source_set_hard_limit(bindings.len() + 1)?;
        bindings.push(StoredBinding::decode(row)?);
    }
    Ok(bindings)
}

/// All retained binding identities, including missing/incompatible drivers.
/// Logical retirement is not a new source effect or source-set admission.
pub(super) fn require_unleased_layout_in(
    tx: &rusqlite::Transaction<'_>,
    sandbox: &str,
) -> Result<()> {
    let bindings = bindings_in(tx, sandbox)?;
    SourceMutationSet::from_binding_refs(bindings.iter().map(|stored| &stored.binding))?
        .require_unleased_in(tx)
}

pub(super) fn summaries(
    connection: &Connection,
    sandbox: &str,
) -> Result<Vec<SourceBindingSummary>> {
    Ok(bindings_in(connection, sandbox)?
        .into_iter()
        .map(StoredBinding::summary)
        .collect())
}

/// Select exactly the required bindings (None) or the named subset (Some).
/// Decode once in canonical name order. Compatibility is resolved against the
/// live registry snapshot, not a persisted diagnostic bit.
pub(crate) fn select_in(
    connection: &Connection,
    sandbox: &str,
    names: Option<&[SourceName]>,
    limit: SourceSetLimit,
    registry: &SourceDriverRegistryReader,
) -> Result<Vec<VersionedSourceBinding>> {
    let requested = if let Some(names) = names {
        limit.admit(names.len())?;
        let mut set = BTreeSet::new();
        for name in names {
            name.validate()?;
            if !set.insert(name.as_str()) {
                return Err(Error::Conflict("duplicate selected SourceBinding name"));
            }
        }
        if set.is_empty() {
            return Err(Error::Config(
                "named source selection must be nonempty".into(),
            ));
        }
        Some(set)
    } else {
        None
    };
    let mut selected = Vec::new();
    for stored in bindings_in(connection, sandbox)? {
        let include = match &requested {
            Some(names) => names.contains(stored.binding.name.as_str()),
            None => stored.binding.checkpoint_policy == CheckpointPolicy::Required,
        };
        if include {
            limit.admit(selected.len() + 1)?;
            registry.resolve(&stored.binding.pin()?)?;
            selected.push(VersionedSourceBinding {
                binding: stored.binding,
                binding_rev: stored.binding_rev,
            });
        }
    }
    if requested
        .as_ref()
        .is_some_and(|names| names.len() != selected.len())
    {
        return Err(Error::Denied("selected SourceBinding is unavailable"));
    }
    Ok(selected)
}

/// Complete-set admission before any driver work or publication.
pub(crate) fn validate_set(
    limit: SourceSetLimit,
    registry: &SourceDriverRegistry,
    bindings: &[SourceBinding],
) -> Result<()> {
    limit.admit(bindings.len())?;
    if bindings.is_empty() {
        return Ok(());
    }
    let registry = registry.reader()?;
    let mut names = BTreeSet::new();
    for binding in bindings {
        binding.validate()?;
        if !names.insert(binding.name.as_str()) {
            return Err(Error::Conflict("duplicate SourceBinding name"));
        }
        registry.resolve(&binding.pin()?)?;
    }
    Ok(())
}

/// Called only inside the initial Sandbox publication transaction, after
/// complete-set admission. No external effect or source grant is implied.
pub(super) fn insert(
    connection: &Connection,
    sandbox: &str,
    bindings: &[SourceBinding],
) -> Result<()> {
    insert_at_revision(connection, sandbox, bindings, 1)
}

fn insert_at_revision(
    connection: &Connection,
    sandbox: &str,
    bindings: &[SourceBinding],
    revision: u64,
) -> Result<()> {
    if bindings.is_empty() {
        return Ok(());
    }
    let mut insert = db(connection.prepare(
        "INSERT INTO sandbox_source_bindings_current(
             sandbox_id,source_name,driver_key,binding_schema,source_ref,materialization,
             access,checkpoint_policy,binding_rev,compatibility
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'compatible')",
    ))?;
    for binding in bindings {
        let driver = serde_json::to_string(&binding.driver)
            .map_err(|_| Error::Storage("cannot encode SourceDriver key"))?;
        let access = match binding.access {
            SourceAccess::ReadOnly => "ro",
            SourceAccess::ReadWrite => "rw",
        };
        let policy = match binding.checkpoint_policy {
            CheckpointPolicy::Required => "required",
            CheckpointPolicy::Ephemeral => "ephemeral",
        };
        db(insert.execute(params![
            sandbox,
            binding.name.as_str(),
            driver,
            binding.binding_schema,
            binding.source_ref,
            binding.materialization,
            access,
            policy,
            revision,
        ]))?;
    }
    Ok(())
}

impl SessionStore {
    /// Trusted host publication of an already-materialized binding change.
    /// The caller independently authorizes old/new source metadata and
    /// layout, verifies the actual driver materialization, and holds that
    /// authority through this commit. This method does not detach, delete,
    /// materialize, remount or perform any other external effect.
    ///
    /// The complete old layout and incoming source are fenced in the one
    /// immediate transaction. No durable lease/receipt is needed for a
    /// metadata-only mutation. Effects requiring external work must use
    /// their original Prepared custody before publishing.
    pub fn sandbox_sources_set(
        &self,
        actor: &PrincipalId,
        sandbox_ref: &str,
        name: &SourceName,
        binding: Option<&SourceBinding>,
        expected_rev: u64,
    ) -> Result<super::SandboxSnapshot> {
        self.set_sources(
            actor,
            sandbox_ref,
            name,
            binding,
            expected_rev,
            None::<fn(&super::SandboxSnapshot, &VersionedSourceBinding) -> Result<()>>,
        )
    }

    /// Trusted metadata-only realization of sources.set(name, null) for
    /// the last binding. The existing layout must already be independently
    /// approved as isolated and source-free, with no retained Agent binding.
    /// The verifier returns live physical/provider authority held through
    /// commit. It may observe but must not mutate or reenter the Store.
    /// Effectful removal still requires the original Prepared custody.
    pub fn sandbox_sources_detach_final<F>(
        &self,
        actor: &PrincipalId,
        sandbox_ref: &str,
        name: &SourceName,
        expected_rev: u64,
        verify: impl FnOnce(&super::SandboxSnapshot, &VersionedSourceBinding) -> Result<F>,
    ) -> Result<super::SandboxSnapshot> {
        self.set_sources(actor, sandbox_ref, name, None, expected_rev, Some(verify))
    }

    fn set_sources<F>(
        &self,
        actor: &PrincipalId,
        sandbox_ref: &str,
        name: &SourceName,
        binding: Option<&SourceBinding>,
        expected_rev: u64,
        verify_final: Option<
            impl FnOnce(&super::SandboxSnapshot, &VersionedSourceBinding) -> Result<F>,
        >,
    ) -> Result<super::SandboxSnapshot> {
        if self.inner.mode != StoreMode::OwnerLock {
            return Err(Error::Denied(
                "source binding mutation requires the exclusive Store owner",
            ));
        }
        name.validate()?;
        if let Some(binding) = binding {
            binding.validate()?;
            if &binding.name != name {
                return Err(Error::Conflict(
                    "source binding name differs from selection",
                ));
            }
        }
        let key = super::parse_sandbox_ref(sandbox_ref)?;
        let sandbox = key.canonical();
        let final_fence;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        super::authorize_project(&tx, actor, &key.project, true)?;
        let current = super::snapshot_row(&tx, &sandbox)?;
        if current.rev != expected_rev || current.lifecycle != super::SandboxLifecycle::Open {
            return Err(Error::Conflict("Sandbox revision or lifecycle changed"));
        }
        let bindings = bindings_in(&tx, &sandbox)?;
        let old = bindings.iter().find(|stored| &stored.binding.name == name);
        if verify_final.is_some() && (bindings.len() > 1 || (!bindings.is_empty() && old.is_none()))
        {
            return Err(Error::Conflict(
                "final detach requires the exact sole source",
            ));
        }
        if old.map(|stored| &stored.binding) == binding {
            return super::snapshot_with_sources(&tx, &sandbox);
        }
        require_no_retained_children_in(&tx, &sandbox)?;
        if current.parent_sandbox_ref.is_some() {
            return Err(Error::Conflict(
                "derived Sandbox source changes require effectful structural custody",
            ));
        }
        let tool_root = current
            .tool_root
            .to_str()
            .ok_or(Error::Storage("invalid Sandbox tool root"))?;
        let cwd = current
            .working_directory
            .to_str()
            .ok_or(Error::Storage("invalid Sandbox working directory"))?;
        super::require_no_retained_layout_in(&tx, tool_root, cwd)?;
        let count = bindings.len() - usize::from(old.is_some()) + usize::from(binding.is_some());
        if count == 0 && verify_final.is_none() {
            // Never turn a protected source layout into an ambient root
            // merely by deleting the last binding row.
            return Err(Error::Conflict(
                "final source removal requires a verified source-free layout",
            ));
        }
        if binding.is_some() {
            self.source_set_limit().admit(count)?;
        }
        // All existing revisions are minted from the Sandbox revision.
        // Refuse inconsistent state instead of reusing an old grant pin.
        if bindings
            .iter()
            .any(|stored| stored.binding_rev > current.rev)
        {
            return Err(Error::Storage(
                "source binding revision exceeds Sandbox revision",
            ));
        }
        let next_rev = current
            .rev
            .checked_add(1)
            .filter(|rev| *rev <= i64::MAX as u64)
            .ok_or(Error::Limit("Sandbox revision exhausted"))?;

        // First/last source transitions cannot reinterpret an existing
        // Agent's ambient layout or another known open layout. Physical
        // aliases and cross-Store isolation remain the host's responsibility.
        if bindings.is_empty() || count == 0 {
            let used: bool = db(tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_state_current WHERE sandbox_id=?1)
                 OR EXISTS(SELECT 1 FROM sandboxes_current
                   WHERE sandbox_id<>?1 AND lifecycle='open'
                     AND (tool_root IN (?2,?3) OR working_directory IN (?2,?3)))",
                params![sandbox, tool_root, cwd],
                |row| row.get(0),
            ))?;
            if used {
                return Err(Error::Conflict(
                    "source-bearing layout transition requires an unused isolated Sandbox",
                ));
            }
        } else {
            let executable: bool = db(tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_state_current
                 WHERE sandbox_id=?1 AND admission<>'held')",
                [&sandbox],
                |row| row.get(0),
            ))?;
            if executable {
                return Err(Error::Conflict(
                    "source mutation requires held Agent admission",
                ));
            }
        }
        super::require_layout_quiescent_in(&tx, &sandbox, &tool_root_key(&current.tool_root)?)?;
        SourceMutationSet::from_binding_refs(bindings.iter().map(|stored| &stored.binding))?
            .require_unleased_in(&tx)?;
        if let Some(binding) = binding {
            self.inner
                .source_drivers
                .reader()?
                .resolve(&binding.pin()?)?;
            // Check the incoming source even at the exact hard set bound.
            // Both checks share the same write transaction, so no claim
            // can interleave between them or with the publication.
            SourceMutationSet::from_bindings(std::slice::from_ref(binding))?
                .require_unleased_in(&tx)?;
        }
        final_fence = if count == 0 {
            let old = old.ok_or(Error::Conflict("final source binding disappeared"))?;
            self.inner
                .source_drivers
                .reader()?
                .resolve(&old.binding.pin()?)?;
            let verify = verify_final.ok_or(Error::Conflict(
                "source-free layout verification unavailable",
            ))?;
            Some(verify(
                &super::snapshot_with_sources(&tx, &sandbox)?,
                &VersionedSourceBinding {
                    binding: old.binding.clone(),
                    binding_rev: old.binding_rev,
                },
            )?)
        } else {
            None
        };
        if old.is_some() {
            let changed = db(tx.execute(
                "DELETE FROM sandbox_source_bindings_current WHERE sandbox_id=?1 AND source_name=?2",
                params![sandbox, name.as_str()],
            ))?;
            if changed != 1 {
                return Err(Error::Conflict("source binding changed during mutation"));
            }
        }
        if let Some(binding) = binding {
            insert_at_revision(&tx, &sandbox, std::slice::from_ref(binding), next_rev)?;
        }
        let changed = db(tx.execute(
            "UPDATE sandboxes_current SET rev=?1,updated_at=?2
             WHERE sandbox_id=?3 AND lifecycle='open' AND rev=?4",
            params![next_rev, now(), sandbox, expected_rev],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict(
                "Sandbox revision changed during source mutation",
            ));
        }
        durability::commit_event(
            &tx,
            &durability::tx_id()?,
            "sandbox.sources.changed",
            "sandbox",
            &sandbox,
            Some(actor),
            serde_json::json!({
                "sync_scope":"local","aspect":"sources","source_name":name.as_str(),
                "rev":next_rev,
            }),
        )?;
        let result = super::snapshot_with_sources(&tx, &sandbox)?;
        db(tx.commit())?;
        drop(final_fence);
        Ok(result)
    }
}

/// Resolve all original retained bindings, not a checkpoint selection or a
/// current host limit. The closed row cannot be structurally mutated.
fn closed_retained_in(
    connection: &Connection,
    sandbox: &str,
) -> Result<(super::SandboxSnapshot, Vec<VersionedSourceBinding>)> {
    let mut snapshot = super::snapshot_row(connection, sandbox)?;
    if snapshot.lifecycle != super::SandboxLifecycle::Closed {
        return Err(Error::Conflict(
            "source retirement requires a closed Sandbox",
        ));
    }
    let mut bindings = Vec::new();
    for stored in bindings_in(connection, sandbox)? {
        let version = VersionedSourceBinding {
            binding: stored.binding.clone(),
            binding_rev: stored.binding_rev,
        };
        snapshot.sources.push(stored.summary());
        bindings.push(version);
    }
    Ok((snapshot, bindings))
}

// A derived child can retain the parent's physical origin even after
// logical close. Its original cleanup must settle before that backing
// may be replaced or retired. Lineage remains independent of Agent ACL.
fn require_no_retained_children_in(c: &Connection, sandbox: &str) -> Result<()> {
    let retained: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sandboxes_current s
         JOIN sandbox_source_bindings_current b ON b.sandbox_id=s.sandbox_id
         WHERE s.parent_sandbox_id=?1)",
        [sandbox],
        |row| row.get(0),
    ))?;
    if retained {
        return Err(Error::Conflict(
            "Sandbox materialization is still required by a retained child",
        ));
    }
    Ok(())
}

fn require_unshared_materializations_in(
    c: &Connection,
    snapshot: &super::SandboxSnapshot,
) -> Result<()> {
    require_no_retained_children_in(c, &snapshot.canonical_ref)?;
    let root = snapshot
        .tool_root
        .to_str()
        .ok_or(Error::Storage("invalid Sandbox tool root"))?;
    let cwd = snapshot
        .working_directory
        .to_str()
        .ok_or(Error::Storage("invalid Sandbox working directory"))?;
    // Exact known mutable materializations and dependent open layouts.
    // The source itself may be shared by isolated checkouts: never reclaim
    // shared provider/object backing merely because one checkout closes.
    let shared: bool = db(c.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sandbox_source_bindings_current owned
             JOIN sandbox_source_bindings_current other
               ON other.driver_key=owned.driver_key AND other.source_ref=owned.source_ref
              AND other.materialization=owned.materialization
             JOIN sandboxes_current s ON s.sandbox_id=other.sandbox_id
             WHERE owned.sandbox_id=?1 AND s.lifecycle='open'
         ) OR EXISTS(
             SELECT 1 FROM sandboxes_current s
             WHERE s.lifecycle='open' AND (
                 s.parent_sandbox_id=?1 OR (
                     (s.tool_root IN (?2,?3) OR s.working_directory IN (?2,?3))
                     AND EXISTS(SELECT 1 FROM sandbox_source_bindings_current b
                                WHERE b.sandbox_id=s.sandbox_id)
                 )
             )
         )",
        params![snapshot.canonical_ref, root, cwd],
        |row| row.get(0),
    ))?;
    if shared {
        return Err(Error::Conflict(
            "Sandbox materialization is still required by an open Sandbox",
        ));
    }
    Ok(())
}

impl SessionStore {
    /// Owner-private restore provenance for physical reconciliation. The
    /// trusted host independently authorizes source/provider metadata.
    /// This does not confer source access or authorize disposal; it works
    /// even after the source checkpoint has released its anchor rows.
    pub fn sandbox_restore_preparation(
        &self,
        actor: &PrincipalId,
        sandbox_ref: &str,
    ) -> Result<aw_agent_core::PreparedSourceSet> {
        let key = super::parse_sandbox_ref(sandbox_ref)?;
        let sandbox = key.canonical();
        let mut connection = self.connection()?;
        let tx = db(connection.transaction())?;
        super::authorize_project(&tx, actor, &key.project, true)?;
        if super::snapshot_row(&tx, &sandbox)?.lifecycle != super::SandboxLifecycle::Closed
            || !has_sources(&tx, &sandbox)?
        {
            return Err(Error::Denied(
                "restore cleanup requires retained closed Sandbox",
            ));
        }
        let sources = crate::construction_receipt::restored_sources_for_sandbox_in(&tx, &sandbox)?;
        let registry = self.inner.source_drivers.reader()?;
        for source in &sources.sources {
            registry.resolve(&source.prepared.descriptor.pin()?)?;
        }
        db(tx.commit())?;
        Ok(sources)
    }

    /// Trusted closed-Sandbox physical reconciliation. Independently
    /// authorize and inspect the exact original materializations and whole
    /// layout, preserving all other live backing dependencies. Success
    /// means proven release (including a prior release) AND a safe layout
    /// for reuse, not just missing driver metadata. The returned guard
    /// retains any external physical/provider fences through final commit.
    /// Never infer authority from the closed row or remove shared backing.
    pub fn sandbox_reconcile_closed_sources<F>(
        &self,
        actor: &PrincipalId,
        sandbox_ref: &str,
        reconcile: impl FnOnce(&super::SandboxSnapshot, &[VersionedSourceBinding]) -> Result<F>,
    ) -> Result<super::SandboxSnapshot> {
        if self.inner.mode != StoreMode::OwnerLock {
            return Err(Error::Denied(
                "Sandbox retention cleanup requires the exclusive Store owner",
            ));
        }
        let key = super::parse_sandbox_ref(sandbox_ref)?;
        let sandbox = key.canonical();
        let prepare_id = format!(
            "{RETENTION_CLEANUP_PREFIX}{:x}",
            Sha256::digest(sandbox.as_bytes())
        );
        let (original, bindings, next_rev, barrier) = {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            super::authorize_project(&tx, actor, &key.project, true)?;
            let (original, bindings) = closed_retained_in(&tx, &sandbox)?;
            if bindings.is_empty() {
                return Ok(original);
            }
            let next_rev = original
                .rev
                .checked_add(1)
                .filter(|r| *r <= i64::MAX as u64)
                .ok_or(Error::Limit("Sandbox revision exhausted"))?;
            super::require_close_quiescent_in(&tx, &original, true)?;
            require_unshared_materializations_in(&tx, &original)?;
            let registry = self.inner.source_drivers.reader()?;
            for binding in &bindings {
                registry.resolve(&binding.binding.pin()?)?;
            }
            let set = SourceMutationSet::from_binding_refs(bindings.iter().map(|b| &b.binding))?;
            let barrier = self.reconcile_retained_sources_in(&tx, &set, &prepare_id)?;
            db(tx.commit())?;
            (original, bindings, next_rev, barrier)
        };
        let fence = match reconcile(&original, &bindings) {
            Ok(fence) => fence,
            Err(error) => {
                barrier.settle(crate::SourceMutationOutcome::Unknown)?;
                return Err(error);
            }
        };
        let finish = (|| {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            barrier.require_preparation_in(&tx, &prepare_id)?;
            let (current, retained) = closed_retained_in(&tx, &sandbox)?;
            if current != original || retained != bindings {
                return Err(Error::Conflict("closed Sandbox retention changed"));
            }
            super::require_close_quiescent_in(&tx, &current, true)?;
            require_unshared_materializations_in(&tx, &current)?;
            // Factual release, not a new grant or effect. Retain the
            // historical closed Sandbox/Agent identities and replay facts.
            if db(tx.execute(
                "DELETE FROM sandbox_source_bindings_current WHERE sandbox_id=?1",
                [&sandbox],
            ))? != bindings.len()
            {
                return Err(Error::Conflict("closed Sandbox source set changed"));
            }
            if db(tx.execute(
                "UPDATE sandboxes_current SET rev=?1,updated_at=?2
                 WHERE sandbox_id=?3 AND lifecycle='closed' AND rev=?4",
                params![next_rev, now(), sandbox, original.rev],
            ))? != 1
            {
                return Err(Error::Conflict("closed Sandbox revision changed"));
            }
            durability::commit_event(
                &tx,
                &durability::tx_id()?,
                "sandbox.retention.released",
                "sandbox",
                &sandbox,
                Some(actor),
                serde_json::json!({"sync_scope":"local","aspect":"sources","rev":next_rev}),
            )?;
            barrier.settle_in(&tx, crate::SourceMutationOutcome::Settled)?;
            let result = super::snapshot_with_sources(&tx, &sandbox)?;
            db(tx.commit())?;
            Ok(result)
        })();
        if finish.is_err() {
            barrier.settle(crate::SourceMutationOutcome::Unknown)?;
        }
        // Keep the independent provider/physical fence through publication.
        drop(fence);
        finish
    }
}

/// Reconcile compatibility, not source state. No driver effect is executed.
/// A compatible registration is not authority to materialize or execute.
pub(crate) fn revalidate_in(
    tx: &Connection,
    registry: &SourceDriverRegistryReader,
    exclusive_owner_recovery: bool,
) -> Result<()> {
    let any: bool = db(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sandbox_source_bindings_current)",
        [],
        |row| row.get(0),
    ))?;
    if !any {
        return Ok(());
    }
    // A restored source-bearing Agent is discoverable but effect-inert
    // until the actual source realization/grant admission exists. Never
    // accept an executable or unowned legacy association on reopen.
    // Exclusive owner startup may recover original journal-linked effects
    // while admission remains held. Direct/staged paths stay quiescent-only.
    let mut query = db(tx.prepare(
        "SELECT a.session_id,a.branch_id,a.admission,b.active_run
         FROM agent_state_current a
         JOIN branches b ON b.session_id=a.session_id AND b.id=a.branch_id
         WHERE EXISTS(SELECT 1 FROM sandbox_source_bindings_current s
                      WHERE s.sandbox_id=a.sandbox_id)",
    ))?;
    let mut rows = db(query.query([]))?;
    while let Some(row) = db(rows.next())? {
        let session: String = db(row.get(0))?;
        let branch: String = db(row.get(1))?;
        let admission: String = db(row.get(2))?;
        let active_run: Option<String> = db(row.get(3))?;
        if admission != "held" {
            return Err(Error::Denied(
                "source-bearing Agent requires held admission",
            ));
        }
        let (project, layout, sandbox, _, lifecycle) =
            super::agent_project_and_layout(tx, &session, &branch)?;
        let sandbox = super::snapshot_row(
            tx,
            &sandbox.ok_or(Error::Storage("source-bearing Agent lost its Sandbox"))?,
        )?;
        if sandbox.project_ref != project.canonical()
            || sandbox.layout_digest != layout.digest
            || sandbox.working_directory.to_str() != Some(layout.working_directory.as_str())
            || sandbox.tool_root.to_str() != Some(layout.tool_root.as_str())
            || (lifecycle != "closed" && sandbox.lifecycle != super::SandboxLifecycle::Open)
        {
            return Err(Error::Conflict(
                "persisted source-bearing Agent Sandbox layout changed",
            ));
        }
        let unresolved: bool = db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM hazards WHERE session_id=?1 AND state<>'settled')
                 OR EXISTS(SELECT 1 FROM runs WHERE session_id=?1 AND status='running')",
            [&session],
            |row| row.get(0),
        ))?;
        if unresolved || active_run.is_some() {
            if !exclusive_owner_recovery {
                return Err(Error::Denied(
                    "source-bearing Agent has unresolved execution",
                ));
            }
            super::effects::journal::require_owner_recovery_in(
                tx,
                &session,
                &branch,
                active_run.as_deref(),
            )?;
        }
    }
    drop(rows);
    drop(query);
    {
        // One bounded Sandbox at a time; no global materialization/descriptor allocation.
        let mut query = db(tx.prepare(
            "SELECT s.sandbox_id FROM sandboxes_current s
             WHERE EXISTS(
                 SELECT 1 FROM sandbox_source_bindings_current b WHERE b.sandbox_id=s.sandbox_id
             ) ORDER BY s.sandbox_id",
        ))?;
        let mut rows = db(query.query([]))?;
        while let Some(row) = db(rows.next())? {
            let sandbox: String = db(row.get(0))?;
            for stored in bindings_in(tx, &sandbox)? {
                let next = compatibility(registry, &stored.binding.pin()?)?;
                if next != stored.compatibility {
                    db(tx.execute(
                        "UPDATE sandbox_source_bindings_current SET compatibility=?1
                         WHERE sandbox_id=?2 AND source_name=?3",
                        params![next.as_str(), sandbox, stored.binding.name.as_str()],
                    ))?;
                }
            }
        }
    }
    Ok(())
}
