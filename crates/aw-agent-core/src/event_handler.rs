//! Typed durable EventHandler definition registry.
//!
//! Definitions are inert. Store activation owns service authority, baseline,
//! retention and receipts; this leaf owns only stable definition identity and
//! concrete native implementation type.

use crate::{
    publication,
    registry::{self, StagedRegistry},
    Error, Result,
};
use serde::{Deserialize, Serialize};
use std::{
    any::{Any, TypeId},
    collections::BTreeMap,
    marker::PhantomData,
    sync::Arc,
};

const MAX_EVENT_HANDLERS: usize = 1024;
const MAX_EVENT_PATTERN_BYTES: usize = 2048;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventHandlerDescriptor {
    pub extension: String,
    pub id: String,
    pub pattern: String,
    pub schema_version: u32,
    pub registration_revision: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct EventHandlerKey<H> {
    extension: String,
    id: String,
    pattern: String,
    schema_version: u32,
    registration_revision: u64,
    implementation: PhantomData<fn() -> H>,
}

impl<H> Clone for EventHandlerKey<H> {
    fn clone(&self) -> Self {
        Self {
            extension: self.extension.clone(),
            id: self.id.clone(),
            pattern: self.pattern.clone(),
            schema_version: self.schema_version,
            registration_revision: self.registration_revision,
            implementation: PhantomData,
        }
    }
}

impl<H> EventHandlerKey<H> {
    pub fn extension(&self) -> &str {
        &self.extension
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub const fn registration_revision(&self) -> u64 {
        self.registration_revision
    }

    pub fn descriptor(&self) -> EventHandlerDescriptor {
        EventHandlerDescriptor {
            extension: self.extension.clone(),
            id: self.id.clone(),
            pattern: self.pattern.clone(),
            schema_version: self.schema_version,
            registration_revision: self.registration_revision,
        }
    }
}

#[derive(Clone)]
struct EventHandlerEntry {
    descriptor: EventHandlerDescriptor,
    type_id: TypeId,
    implementation: Arc<dyn Any + Send + Sync>,
}

#[derive(Clone, Default)]
struct EventHandlerSnapshot {
    entries: BTreeMap<(String, String), EventHandlerEntry>,
}

fn validate_pattern(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > MAX_EVENT_PATTERN_BYTES
        || !value.is_ascii()
        || value.chars().any(char::is_control)
    {
        return Err(Error::Config(
            "EventHandler pattern must be bounded printable ASCII".into(),
        ));
    }
    Ok(())
}

fn validate_snapshot(snapshot: &EventHandlerSnapshot) -> Result<()> {
    if snapshot.entries.len() > MAX_EVENT_HANDLERS {
        return Err(Error::Limit("too many registered EventHandlers"));
    }
    for ((extension, id), entry) in &snapshot.entries {
        if &entry.descriptor.extension != extension
            || &entry.descriptor.id != id
            || entry.descriptor.schema_version == 0
            || entry.descriptor.registration_revision == 0
        {
            return Err(Error::Config(
                "invalid EventHandler registry snapshot".into(),
            ));
        }
        registry::validate_extension_id(extension)?;
        registry::validate_local_id(id)?;
        validate_pattern(&entry.descriptor.pattern)?;
    }
    Ok(())
}

#[derive(Clone)]
pub struct EventHandlerRegistry {
    owner: Arc<StagedRegistry<EventHandlerSnapshot>>,
}

pub struct EventHandlerRegistryReader {
    reader: publication::Reader<EventHandlerSnapshot>,
}

impl Default for EventHandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHandlerRegistry {
    pub fn new() -> Self {
        Self {
            owner: Arc::new(StagedRegistry::new(EventHandlerSnapshot::default())),
        }
    }

    pub fn revision(&self) -> Result<u64> {
        Ok(self.owner.current()?.generation)
    }

    pub fn reader(&self) -> Result<EventHandlerRegistryReader> {
        Ok(EventHandlerRegistryReader {
            reader: self.owner.reader_factory().reader()?,
        })
    }

    pub fn register<H: Send + Sync + 'static>(
        &self,
        extension: &str,
        id: &str,
        pattern: &str,
        schema_version: u32,
        handler: H,
    ) -> Result<EventHandlerKey<H>> {
        self.register_if(extension, id, pattern, schema_version, handler, None)
    }

    pub fn register_if<H: Send + Sync + 'static>(
        &self,
        extension: &str,
        id: &str,
        pattern: &str,
        schema_version: u32,
        handler: H,
        expected_revision: Option<u64>,
    ) -> Result<EventHandlerKey<H>> {
        registry::validate_extension_id(extension)?;
        registry::validate_local_id(id)?;
        validate_pattern(pattern)?;
        if schema_version == 0 {
            return Err(Error::Config(
                "EventHandler schema version must be positive".into(),
            ));
        }

        let extension = extension.to_owned();
        let id = id.to_owned();
        let pattern = pattern.to_owned();
        let implementation: Arc<dyn Any + Send + Sync> = Arc::new(handler);
        let stage = self.owner.stage(
            expected_revision,
            |current, next_revision| {
                if current
                    .entries
                    .contains_key(&(extension.clone(), id.clone()))
                {
                    return Err(Error::Conflict("EventHandler is already registered"));
                }
                let descriptor = EventHandlerDescriptor {
                    extension: extension.clone(),
                    id: id.clone(),
                    pattern: pattern.clone(),
                    schema_version,
                    registration_revision: next_revision,
                };
                let mut next = current.clone();
                next.entries.insert(
                    (extension.clone(), id.clone()),
                    EventHandlerEntry {
                        descriptor,
                        type_id: TypeId::of::<H>(),
                        implementation,
                    },
                );
                Ok((
                    next,
                    EventHandlerKey {
                        extension,
                        id,
                        pattern,
                        schema_version,
                        registration_revision: next_revision,
                        implementation: PhantomData,
                    },
                ))
            },
            validate_snapshot,
        )?;
        self.owner.publish(stage).map(|(_, key)| key)
    }

    pub fn remove<H: Send + Sync + 'static>(
        &self,
        key: &EventHandlerKey<H>,
        expected_registry_revision: Option<u64>,
    ) -> Result<u64> {
        let extension = key.extension.clone();
        let id = key.id.clone();
        let schema_version = key.schema_version;
        let registration_revision = key.registration_revision;
        let stage = self.owner.stage(
            expected_registry_revision,
            |current, _next_revision| {
                let current_entry = current
                    .entries
                    .get(&(extension.clone(), id.clone()))
                    .ok_or(Error::Denied("EventHandler handle is stale"))?;
                if current_entry.type_id != TypeId::of::<H>()
                    || current_entry.descriptor.schema_version != schema_version
                    || current_entry.descriptor.registration_revision != registration_revision
                {
                    return Err(Error::Denied("EventHandler handle is stale"));
                }
                let mut next = current.clone();
                next.entries.remove(&(extension.clone(), id.clone()));
                Ok((next, ()))
            },
            validate_snapshot,
        )?;
        self.owner.publish(stage).map(|(revision, ())| revision)
    }

    pub fn list(&self) -> Result<Vec<EventHandlerDescriptor>> {
        let current = self.owner.current()?;
        Ok(current
            .value
            .entries
            .values()
            .map(|entry| entry.descriptor.clone())
            .collect())
    }
}

impl EventHandlerRegistryReader {
    pub fn revision(&self) -> Result<u64> {
        Ok(self.reader.snapshot()?.generation)
    }

    pub fn descriptor<H: Send + Sync + 'static>(
        &self,
        key: &EventHandlerKey<H>,
    ) -> Result<EventHandlerDescriptor> {
        let snapshot = self.reader.snapshot()?;
        let entry = revalidate_snapshot(&snapshot.value, key)?;
        Ok(entry.descriptor.clone())
    }

    pub fn get<H: Send + Sync + 'static>(&self, key: &EventHandlerKey<H>) -> Result<Arc<H>> {
        let snapshot = self.reader.snapshot()?;
        let entry = revalidate_snapshot(&snapshot.value, key)?;
        entry
            .implementation
            .clone()
            .downcast::<H>()
            .map_err(|_| Error::Denied("EventHandler implementation type changed"))
    }

    pub fn list(&self) -> Result<Vec<EventHandlerDescriptor>> {
        let snapshot = self.reader.snapshot()?;
        Ok(snapshot
            .value
            .entries
            .values()
            .map(|entry| entry.descriptor.clone())
            .collect())
    }
}

fn revalidate_snapshot<'a, H: Send + Sync + 'static>(
    snapshot: &'a EventHandlerSnapshot,
    key: &EventHandlerKey<H>,
) -> Result<&'a EventHandlerEntry> {
    let entry = snapshot
        .entries
        .get(&(key.extension.clone(), key.id.clone()))
        .ok_or(Error::Denied("EventHandler handle is stale"))?;
    if entry.type_id != TypeId::of::<H>()
        || entry.descriptor.pattern != key.pattern
        || entry.descriptor.schema_version != key.schema_version
        || entry.descriptor.registration_revision != key.registration_revision
    {
        return Err(Error::Denied("EventHandler handle is stale"));
    }
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    struct Indexer(u64);

    #[derive(Debug)]
    struct Replacement;

    #[test]
    fn handler_leaf_reuses_staged_registry_and_pins_exact_registration() {
        let registry = EventHandlerRegistry::new();
        let initial = registry.revision().unwrap();
        let key = registry
            .register(
                "histate",
                "decision_indexer",
                "d:acme.i:alice.p:core.a:*.e:#histate_decision_accepted",
                1,
                Indexer(7),
            )
            .unwrap();
        assert_eq!(key.registration_revision(), initial + 1);

        let reader = registry.reader().unwrap();
        assert_eq!(reader.get(&key).unwrap().as_ref(), &Indexer(7));

        let other = registry
            .register(
                "histate",
                "other",
                "d:acme.i:alice.p:core.a:*.e:#other",
                1,
                Replacement,
            )
            .unwrap();
        reader.descriptor(&key).unwrap();
        registry.remove(&other, None).unwrap();
        reader.descriptor(&key).unwrap();

        let before_invalid = registry.revision().unwrap();
        assert!(registry
            .register(
                "histate",
                "bad space",
                "not a control
pattern",
                1,
                Replacement
            )
            .is_err());
        assert_eq!(registry.revision().unwrap(), before_invalid);

        registry.remove(&key, None).unwrap();
        assert!(reader.descriptor(&key).is_err());
        let replacement = registry
            .register(
                "histate",
                "decision_indexer",
                "d:acme.i:alice.p:core.a:*.e:#histate_decision_accepted",
                1,
                Replacement,
            )
            .unwrap();
        assert!(replacement.registration_revision() > key.registration_revision());
        assert!(reader.descriptor(&key).is_err());
    }
}
