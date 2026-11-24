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
use std::collections::BTreeSet;

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
pub(super) fn validate_set(
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
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        super::authorize_project(&tx, actor, &key.project, true)?;
        let current = super::snapshot_row(&tx, &sandbox)?;
        if current.rev != expected_rev || current.lifecycle != super::SandboxLifecycle::Open {
            return Err(Error::Conflict("Sandbox revision or lifecycle changed"));
        }
        let bindings = bindings_in(&tx, &sandbox)?;
        let old = bindings.iter().find(|stored| &stored.binding.name == name);
        if old.map(|stored| &stored.binding) == binding {
            return super::snapshot_with_sources(&tx, &sandbox);
        }
        let count = bindings.len() - usize::from(old.is_some()) + usize::from(binding.is_some());
        if count == 0 {
            // Removing the last binding must not turn a protected source
            // layout into an ambient legacy root. Wait for verified
            // source-free baseline/physical retirement support.
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

        // Source-bearing admission is still held. Never retrofit the first
        // source into an existing Agent's ambient layout or adopt another
        // known open root. Physical aliases/cross-Store isolation remain
        // the trusted host's responsibility.
        if bindings.is_empty() {
            let used: bool = db(tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_state_current WHERE sandbox_id=?1)
                 OR EXISTS(SELECT 1 FROM sandboxes_current
                   WHERE sandbox_id<>?1 AND lifecycle='open' AND tool_root=?2)",
                params![
                    sandbox,
                    current
                        .tool_root
                        .to_str()
                        .ok_or(Error::Storage("invalid Sandbox tool root"))?
                ],
                |row| row.get(0),
            ))?;
            if used {
                return Err(Error::Conflict(
                    "initial source attachment requires an unused isolated Sandbox",
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
        Ok(result)
    }
}

/// Reconcile compatibility, not source state. No driver effect is executed.
/// A compatible registration is not authority to materialize or execute.
pub(crate) fn revalidate_in(tx: &Connection, registry: &SourceDriverRegistryReader) -> Result<()> {
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
    // accept an executable or unresolved legacy association on reopen.
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
        if admission != "held" || active_run.is_some() {
            return Err(Error::Denied(
                "source-bearing Agent requires held quiescent admission",
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
            || (lifecycle == "open" && sandbox.lifecycle != super::SandboxLifecycle::Open)
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
        if unresolved {
            return Err(Error::Denied(
                "source-bearing Agent has unresolved execution",
            ));
        }
    }
    drop(rows);
    drop(query);
    {
        // One bounded Sandbox at a time; no global materialization/descriptor allocation.
        let mut query = db(tx.prepare(
            "SELECT s.sandbox_id FROM sandboxes_current s
             WHERE s.lifecycle='open' AND EXISTS(
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
