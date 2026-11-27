//! Shared schema-pin compatibility check for the existing durable owners.
//! Diagnostics are not driver authority or proof of external materialization.
use crate::{db, Error, Inner, Result, SessionStore, StoreMode};
use aw_agent_core::{
    DriverArtifact, DriverKey, DriverSchemaPin, RetainedSourceArtifact, SourceDriverCompatibility,
    SourceDriverHandle, SourceDriverRegistry, SourceDriverRegistryReader, SourceDriverSchemas,
};
use rusqlite::{Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCompatibility {
    Compatible,
    RecoveryRequired,
}

impl SourceCompatibility {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "compatible" => Ok(Self::Compatible),
            "recovery_required" => Ok(Self::RecoveryRequired),
            _ => Err(Error::Storage("invalid source compatibility")),
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Compatible => "compatible",
            Self::RecoveryRequired => "recovery_required",
        }
    }
}

pub(crate) fn compatibility(
    registry: &SourceDriverRegistryReader,
    pin: &DriverSchemaPin,
) -> Result<SourceCompatibility> {
    match registry.resolve(pin) {
        Ok(_) => Ok(SourceCompatibility::Compatible),
        Err(Error::Denied(_)) => Ok(SourceCompatibility::RecoveryRequired),
        Err(error) => Err(error),
    }
}

pub(crate) fn revalidate(
    connection: &mut Connection,
    registry: &SourceDriverRegistry,
    exclusive_owner_recovery: bool,
) -> Result<()> {
    let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let registry = registry.reader()?;
    crate::sandbox_owner::source_bindings::revalidate_in(&tx, &registry, exclusive_owner_recovery)?;
    crate::checkpoint_owner::revalidate_in(&tx, &registry)?;
    // Compatibility is derived on demand from retained descriptors; never
    // rewrite an immutable receipt or treat compatibility as live authority.
    crate::construction_receipt::visit_unresolved_sources(&tx, &registry, |_, _, _| Ok(()))?;
    db(tx.commit())
}

/// Definition-level, owner-private diagnostic. No resource references,
/// source locators, recovery descriptors or secret material are disclosed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceDependencyKind {
    Binding,
    CheckpointAnchor,
    RetainedMaterialization,
    UnresolvedConstruction,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceDriverDependency {
    pub kind: SourceDependencyKind,
    pub pin: DriverSchemaPin,
    pub count: u64,
    pub compatibility: SourceCompatibility,
}

fn visit_artifacts_in(
    connection: &Connection,
    driver: &DriverKey,
    mut visit: impl FnMut(RetainedSourceArtifact<'_>) -> Result<()>,
) -> Result<()> {
    crate::sandbox_owner::source_bindings::visit_driver_artifacts(connection, driver, &mut visit)?;
    crate::checkpoint_owner::visit_driver_artifacts(connection, driver, &mut visit)?;
    crate::construction_receipt::visit_unresolved_source_inputs(connection, |_, source, input| {
        if &source.prepared.descriptor.driver == driver
            || input
                .map(|input| input.pin())
                .transpose()?
                .is_some_and(|pin| &pin.driver == driver)
        {
            visit(RetainedSourceArtifact::Recovery { source, input })?;
        }
        Ok(())
    })
}

fn visit_pins(
    artifact: RetainedSourceArtifact<'_>,
    mut visit: impl FnMut(SourceDependencyKind, DriverSchemaPin) -> Result<()>,
) -> Result<()> {
    use SourceDependencyKind::*;
    match artifact {
        RetainedSourceArtifact::Binding(binding) => visit(Binding, binding.pin()?),
        RetainedSourceArtifact::Anchor { anchor, .. } => visit(CheckpointAnchor, anchor.pin()?),
        RetainedSourceArtifact::Materialization { source, .. } => {
            visit(RetainedMaterialization, source.prepared.descriptor.pin()?)
        }
        RetainedSourceArtifact::Recovery { source, input } => {
            visit(UnresolvedConstruction, source.prepared.descriptor.pin()?)?;
            if let Some(input) = input {
                visit(UnresolvedConstruction, input.pin()?)?;
            }
            Ok(())
        }
    }
}

fn dependencies_in(
    connection: &Connection,
    registry: &SourceDriverRegistryReader,
    driver: &DriverKey,
) -> Result<Vec<SourceDriverDependency>> {
    driver.validate()?;
    // Stream exact owning rows, aggregate only bounded nonsecret schema
    // groups. Malformed custody refuses maintenance.
    let mut counts = BTreeMap::<(SourceDependencyKind, DriverArtifact, u32), u64>::new();
    let mut add = |kind, pin: DriverSchemaPin| -> Result<()> {
        if &pin.driver == driver {
            let key = (kind, pin.artifact, pin.schema_version);
            if counts.len() >= 4096 && !counts.contains_key(&key) {
                return Err(Error::Limit(
                    "source dependency schema group bound exceeded",
                ));
            }
            let count = counts.entry(key).or_default();
            *count = count
                .checked_add(1)
                .ok_or(Error::Limit("source dependency count exhausted"))?;
        }
        Ok(())
    };
    visit_artifacts_in(connection, driver, |artifact| {
        visit_pins(artifact, &mut add)
    })?;
    counts
        .into_iter()
        .map(|((kind, artifact, version), count)| {
            let pin = DriverSchemaPin::new(driver.clone(), artifact, version)?;
            Ok(SourceDriverDependency {
                kind,
                compatibility: compatibility(registry, &pin)?,
                pin,
                count,
            })
        })
        .collect()
}

fn require_unused_in(
    connection: &Connection,
    registry: &SourceDriverRegistry,
    driver: &DriverKey,
) -> Result<()> {
    if !dependencies_in(connection, &registry.reader()?, driver)?.is_empty() {
        return Err(Error::Conflict(
            "SourceDriver has retained durable dependencies",
        ));
    }
    require_no_unresolved_mutation(connection)
}

fn require_no_unresolved_mutation(connection: &Connection) -> Result<()> {
    // Source mutation scopes intentionally hash their identity; do not
    // infer a driver from a hash or from missing bindings. Until every
    // unresolved effect is attributed, any such custody holds retirement.
    let unresolved: bool = db(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM local_coordination_leases
         WHERE scope_kind='execution' AND scope_key GLOB 'source-mutation:*'
           AND state IN ('active','hazard'))",
        [],
        |row| row.get(0),
    ))?;
    if unresolved {
        return Err(Error::Conflict(
            "unresolved source mutation custody prevents registry maintenance",
        ));
    }
    Ok(())
}

impl SessionStore {
    /// Trusted local host diagnostic, not a public Agent/Project View.
    /// Counts exact retained schema dependencies even if the definition is
    /// absent after restart. No registry or source state mutation.
    pub fn source_driver_dependencies(
        &self,
        driver: &DriverKey,
    ) -> Result<Vec<SourceDriverDependency>> {
        let mut connection = self.connection()?;
        let tx = db(connection.transaction())?;
        dependencies_in(&tx, &self.inner.source_drivers.reader()?, driver)
    }

    /// Definition maintenance requires the exclusive Store/registry owner.
    /// No active Store clone, registry clone, reader, or implementation
    /// user may race the dependency inspection and staged publication.
    pub fn remove_source_driver<T: Send + Sync + 'static>(
        &mut self,
        handle: &SourceDriverHandle<T>,
        expected_revision: Option<u64>,
    ) -> Result<u64> {
        self.with_source_driver_maintenance(|tx, registry| {
            require_unused_in(tx, registry, handle.driver())?;
            registry.remove(handle, expected_revision)
        })
    }

    /// First replacement cut is deliberately dependency-free. A declared
    /// version range alone cannot prove recovery semantics for live data.
    pub fn replace_source_driver<T: Send + Sync + 'static, U: Send + Sync + 'static>(
        &mut self,
        handle: &SourceDriverHandle<T>,
        schemas: SourceDriverSchemas,
        implementation: U,
        expected_revision: Option<u64>,
    ) -> Result<SourceDriverHandle<U>> {
        self.with_source_driver_maintenance(|tx, registry| {
            require_unused_in(tx, registry, handle.driver())?;
            registry.replace(handle, schemas, implementation, expected_revision)
        })
    }

    /// Explicit semantic replacement for a trusted native implementation of
    /// the same concrete type. Neither a schema declaration nor a caller
    /// supplied success flag can admit retained custody. No durable writes
    /// or source effects, and no recovery-required status is cleared here.
    pub fn replace_compatible_source_driver<T: SourceDriverCompatibility>(
        &mut self,
        handle: &SourceDriverHandle<T>,
        schemas: SourceDriverSchemas,
        implementation: T,
        expected_revision: Option<u64>,
    ) -> Result<SourceDriverHandle<T>> {
        self.with_source_driver_maintenance(|tx, registry| {
            require_no_unresolved_mutation(tx)?;
            registry.replace_validated(
                handle,
                schemas,
                implementation,
                expected_revision,
                |previous, candidate| {
                    candidate.validate_replacement(previous, handle.driver(), schemas)?;
                    visit_artifacts_in(tx, handle.driver(), |artifact| {
                        visit_pins(artifact, |_, pin| {
                            if &pin.driver != handle.driver()
                                || !handle.schemas().supports(&pin)
                                || !schemas.supports(&pin)
                            {
                                return Err(Error::Conflict(
                                    "SourceDriver replacement cannot cover retained artifact schema",
                                ));
                            }
                            Ok(())
                        })?;
                        candidate.validate_retained(artifact)
                    })
                },
            )
        })
    }

    fn with_source_driver_maintenance<R>(
        &mut self,
        operation: impl FnOnce(&Connection, &mut SourceDriverRegistry) -> Result<R>,
    ) -> Result<R> {
        let inner = Arc::get_mut(&mut self.inner).ok_or(Error::Conflict(
            "SourceDriver maintenance requires quiescent Store ownership",
        ))?;
        let Inner {
            mode,
            conn,
            process,
            source_drivers,
            ..
        } = inner;
        if *mode != StoreMode::OwnerLock {
            return Err(Error::Denied(
                "SourceDriver maintenance requires exclusive Store owner",
            ));
        }
        let connection = conn
            .get_mut()
            .map_err(|_| Error::Storage("store connection poisoned"))?;
        let process = process
            .get_mut()
            .map_err(|_| Error::Storage("local process identity lock poisoned"))?
            .as_ref()
            .ok_or(Error::Denied(
                "SourceDriver maintenance requires current Store process",
            ))?;
        // The exclusive Store owner excludes supported competing processes
        // and all local Store users. Still honor ordinary maintenance and
        // the current process identity before and within the SQL fence.
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        crate::coordination::validate_process(&tx, process)?;
        crate::coordination::validate_ordinary_access(&tx)?;
        // No durable writes; hold the SQL fence through publication.
        operation(&tx, source_drivers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::{
        CheckpointPolicy, SchemaSupport, SourceAccess, SourceBinding, SourceName, SourceSetLimit,
    };
    use std::path::PathBuf;

    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn exclusive_store_driver_maintenance_refuses_live_and_unknown_custody() {
        let temp = Temp(std::env::temp_dir().join(crate::id("aw-driver-maintenance").unwrap()));
        let mut registry = SourceDriverRegistry::new();
        let driver = DriverKey::builtin("fixture-driver").unwrap();
        let schema = SchemaSupport::new(1, 1).unwrap();
        let schemas = SourceDriverSchemas {
            binding: schema,
            anchor: schema,
            recovery: schema,
        };
        let handle = registry
            .register(driver.clone(), schemas, "original")
            .unwrap();
        let unused = registry
            .register(DriverKey::builtin("unused").unwrap(), schemas, ())
            .unwrap();
        let limit = SourceSetLimit::default();
        let mut store = SessionStore::open_with_sources(&temp.0, registry.clone(), limit).unwrap();
        let rev = registry.revision().unwrap();
        assert!(
            registry.remove(&handle, Some(rev)).is_err(),
            "external registry clone cannot bypass Store"
        );
        assert!(store.remove_source_driver(&handle, Some(rev)).is_err());
        drop(registry);
        let clone = store.clone();
        assert!(store.remove_source_driver(&handle, Some(rev)).is_err());
        drop(clone);
        let reader = store.inner.source_drivers.reader().unwrap();
        assert!(store.remove_source_driver(&handle, Some(rev)).is_err());
        let live = reader.get(&handle).unwrap();
        drop(reader);
        assert!(store.remove_source_driver(&handle, Some(rev)).is_err());
        drop(live);
        assert!(store
            .source_driver_dependencies(&driver)
            .unwrap()
            .is_empty());
        let next = store
            .replace_source_driver(&handle, schemas, 7u32, Some(rev))
            .unwrap();
        assert!(store
            .inner
            .source_drivers
            .reader()
            .unwrap()
            .get(&handle)
            .is_err());
        assert_eq!(
            *store
                .inner
                .source_drivers
                .reader()
                .unwrap()
                .get(&next)
                .unwrap(),
            7
        );

        // A complete-set mutation with no remaining attached binding is
        // still custody. Never infer absence of effect from a missing row.
        let set = crate::SourceMutationSet::from_bindings(&[SourceBinding {
            name: SourceName::new("original").unwrap(),
            driver: driver.clone(),
            binding_schema: 1,
            source_ref: "fixture-source".into(),
            materialization: "fixture-materialization".into(),
            access: SourceAccess::ReadWrite,
            checkpoint_policy: CheckpointPolicy::Required,
        }])
        .unwrap();
        let barrier = store
            .acquire_source_mutation_barrier(&set, "fixture-unknown")
            .unwrap();
        assert!(store.remove_source_driver(&unused, None).is_err());
        barrier
            .settle(crate::SourceMutationOutcome::Unknown)
            .unwrap();
        assert!(store
            .source_driver_dependencies(&driver)
            .unwrap()
            .is_empty());
        assert!(matches!(
            store.remove_source_driver(&unused, None),
            Err(Error::Conflict(
                "unresolved source mutation custody prevents registry maintenance"
            ))
        ));
        // Only the existing exact recovery owner can consume that hazard.
        let recovered = {
            let mut connection = store.connection().unwrap();
            let tx =
                db(connection.transaction_with_behavior(TransactionBehavior::Immediate)).unwrap();
            let recovered = store
                .recover_source_mutation_barrier_in(&tx, &set, "fixture-unknown")
                .unwrap();
            db(tx.commit()).unwrap();
            recovered
        };
        recovered
            .settle(crate::SourceMutationOutcome::Settled)
            .unwrap();
        assert_eq!(
            store.remove_source_driver(&next, Some(rev + 1)).unwrap(),
            rev + 2
        );
        assert!(store
            .inner
            .source_drivers
            .reader()
            .unwrap()
            .handle::<u32>(&driver)
            .is_err());
        assert_eq!(
            store.remove_source_driver(&unused, Some(rev + 2)).unwrap(),
            rev + 3
        );
    }
}
