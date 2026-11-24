//! Typed custom Event definition registry.
//!
//! EventTypeRegistry owns definition/schema identity only. It owns no Event
//! instances, history, grants, subscriptions or delivery authority.

use crate::{
    registry::{self, StagedRegistry},
    Error, Result,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    any::TypeId,
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
    sync::Arc,
};

const MAX_EVENT_TYPES: usize = 1024;

pub trait EventPayload: Serialize + DeserializeOwned + Send + Sync + 'static {}
impl<T> EventPayload for T where T: Serialize + DeserializeOwned + Send + Sync + 'static {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventSchemaRef {
    pub extension: String,
    pub id: String,
    pub version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventTypeDescriptor {
    pub extension: String,
    pub id: String,
    pub event: String,
    pub schema_version: u32,
    pub registration_revision: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct EventKey<T> {
    extension: String,
    id: String,
    event: String,
    schema_version: u32,
    registration_revision: u64,
    value: PhantomData<fn() -> T>,
}

impl<T> Clone for EventKey<T> {
    fn clone(&self) -> Self {
        Self {
            extension: self.extension.clone(),
            id: self.id.clone(),
            event: self.event.clone(),
            schema_version: self.schema_version,
            registration_revision: self.registration_revision,
            value: PhantomData,
        }
    }
}

impl<T> EventKey<T> {
    pub fn extension(&self) -> &str {
        &self.extension
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn event(&self) -> &str {
        &self.event
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub const fn registration_revision(&self) -> u64 {
        self.registration_revision
    }

    pub fn schema(&self) -> EventSchemaRef {
        EventSchemaRef {
            extension: self.extension.clone(),
            id: self.id.clone(),
            version: self.schema_version,
        }
    }
}

impl<T: EventPayload> EventKey<T> {
    fn decode(
        &self,
        schema: Option<&EventSchemaRef>,
        data: Option<&serde_json::Value>,
    ) -> Result<T> {
        if schema != Some(&self.schema()) {
            return Err(Error::Denied(
                "Event does not carry this typed schema identity",
            ));
        }
        let data = data.ok_or(Error::Denied("typed Event has no payload"))?;
        serde_json::from_value(data.clone())
            .map_err(|_| Error::Denied("Event payload does not match registered Rust type"))
    }
}

#[derive(Clone)]
struct EventTypeEntry {
    descriptor: EventTypeDescriptor,
    type_id: TypeId,
}

#[derive(Clone, Default)]
struct EventTypeSnapshot {
    entries: BTreeMap<(String, String), EventTypeEntry>,
}

fn encode_extension_id(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'_' => out.push_str("__"),
            b'.' => out.push_str("_d"),
            _ => out.push(byte as char),
        }
    }
    out
}

fn wire_event(extension: &str, id: &str) -> Result<String> {
    registry::validate_extension_id(extension)?;
    registry::validate_local_id(id)?;
    let extension = encode_extension_id(extension);
    let event = format!("#{extension}_{id}");
    if event.len() > 64 {
        return Err(Error::Limit(
            "EventType qualified custom Event token exceeds 64 bytes",
        ));
    }
    Ok(event)
}

fn validate_snapshot(snapshot: &EventTypeSnapshot) -> Result<()> {
    if snapshot.entries.len() > MAX_EVENT_TYPES {
        return Err(Error::Limit("too many registered EventTypes"));
    }
    let mut events = BTreeSet::new();
    for ((extension, id), entry) in &snapshot.entries {
        if &entry.descriptor.extension != extension
            || &entry.descriptor.id != id
            || entry.descriptor.schema_version == 0
            || entry.descriptor.registration_revision == 0
            || entry.descriptor.event != wire_event(extension, id)?
            || !events.insert(entry.descriptor.event.as_str())
        {
            return Err(Error::Config("invalid EventType registry snapshot".into()));
        }
    }
    Ok(())
}

#[derive(Clone)]
pub struct EventTypeRegistry {
    owner: Arc<StagedRegistry<EventTypeSnapshot>>,
}

pub struct EventTypeRegistryReader {
    reader: crate::publication::Reader<EventTypeSnapshot>,
}

impl Default for EventTypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl EventTypeRegistry {
    pub fn new() -> Self {
        Self {
            owner: Arc::new(StagedRegistry::new(EventTypeSnapshot::default())),
        }
    }

    pub fn revision(&self) -> Result<u64> {
        Ok(self.owner.current()?.generation)
    }

    pub fn reader(&self) -> Result<EventTypeRegistryReader> {
        Ok(EventTypeRegistryReader {
            reader: self.owner.reader_factory().reader()?,
        })
    }

    pub fn register<T: EventPayload>(
        &self,
        extension: &str,
        id: &str,
        schema_version: u32,
    ) -> Result<EventKey<T>> {
        self.register_if(extension, id, schema_version, None)
    }

    pub fn register_if<T: EventPayload>(
        &self,
        extension: &str,
        id: &str,
        schema_version: u32,
        expected_revision: Option<u64>,
    ) -> Result<EventKey<T>> {
        if schema_version == 0 {
            return Err(Error::Config(
                "EventType schema version must be positive".into(),
            ));
        }
        let event = wire_event(extension, id)?;
        let extension = extension.to_owned();
        let id = id.to_owned();
        let stage = self.owner.stage(
            expected_revision,
            |current, next_revision| {
                if current
                    .entries
                    .contains_key(&(extension.clone(), id.clone()))
                {
                    return Err(Error::Conflict("EventType is already registered"));
                }
                if current
                    .entries
                    .values()
                    .any(|entry| entry.descriptor.event == event)
                {
                    return Err(Error::Conflict(
                        "EventType wire event is already registered",
                    ));
                }
                let descriptor = EventTypeDescriptor {
                    extension: extension.clone(),
                    id: id.clone(),
                    event: event.clone(),
                    schema_version,
                    registration_revision: next_revision,
                };
                let mut next = current.clone();
                next.entries.insert(
                    (extension.clone(), id.clone()),
                    EventTypeEntry {
                        descriptor,
                        type_id: TypeId::of::<T>(),
                    },
                );
                Ok((
                    next,
                    EventKey {
                        extension,
                        id,
                        event,
                        schema_version,
                        registration_revision: next_revision,
                        value: PhantomData,
                    },
                ))
            },
            validate_snapshot,
        )?;
        self.owner.publish(stage).map(|(_, key)| key)
    }

    pub fn remove<T: EventPayload>(
        &self,
        key: &EventKey<T>,
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
                    .ok_or(Error::Denied("EventType handle is stale"))?;
                if current_entry.type_id != TypeId::of::<T>()
                    || current_entry.descriptor.schema_version != schema_version
                    || current_entry.descriptor.registration_revision != registration_revision
                {
                    return Err(Error::Denied("EventType handle is stale"));
                }
                let mut next = current.clone();
                next.entries.remove(&(extension.clone(), id.clone()));
                Ok((next, ()))
            },
            validate_snapshot,
        )?;
        self.owner.publish(stage).map(|(revision, ())| revision)
    }

    pub fn revalidate<T: EventPayload>(&self, key: &EventKey<T>) -> Result<EventTypeDescriptor> {
        let current = self.owner.current()?;
        revalidate_snapshot(&current.value, key)
    }

    pub fn list(&self) -> Result<Vec<EventTypeDescriptor>> {
        let current = self.owner.current()?;
        Ok(current
            .value
            .entries
            .values()
            .map(|entry| entry.descriptor.clone())
            .collect())
    }
}

impl EventTypeRegistryReader {
    pub fn revision(&self) -> Result<u64> {
        Ok(self.reader.snapshot()?.generation)
    }

    pub fn revalidate<T: EventPayload>(&self, key: &EventKey<T>) -> Result<EventTypeDescriptor> {
        let snapshot = self.reader.snapshot()?;
        revalidate_snapshot(&snapshot.value, key)
    }

    pub fn decode<T: EventPayload>(
        &self,
        key: &EventKey<T>,
        schema: Option<&EventSchemaRef>,
        data: Option<&serde_json::Value>,
    ) -> Result<T> {
        let snapshot = self.reader.snapshot()?;
        revalidate_snapshot(&snapshot.value, key)?;
        key.decode(schema, data)
    }

    pub fn list(&self) -> Result<Vec<EventTypeDescriptor>> {
        let snapshot = self.reader.snapshot()?;
        Ok(snapshot
            .value
            .entries
            .values()
            .map(|entry| entry.descriptor.clone())
            .collect())
    }
}

fn revalidate_snapshot<T: EventPayload>(
    snapshot: &EventTypeSnapshot,
    key: &EventKey<T>,
) -> Result<EventTypeDescriptor> {
    let entry = snapshot
        .entries
        .get(&(key.extension.clone(), key.id.clone()))
        .ok_or(Error::Denied("EventType handle is stale"))?;
    if entry.type_id != TypeId::of::<T>()
        || entry.descriptor.event != key.event
        || entry.descriptor.schema_version != key.schema_version
        || entry.descriptor.registration_revision != key.registration_revision
    {
        return Err(Error::Denied("EventType handle is stale"));
    }
    Ok(entry.descriptor.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct DecisionAccepted {
        value: u64,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct Different {
        value: String,
    }

    #[test]
    fn one_leaf_publication_is_atomic_and_handles_are_registration_local() {
        assert_eq!(
            wire_event("histate", "decision_accepted").unwrap(),
            "#histate_decision_accepted"
        );
        assert_ne!(
            wire_event("hi_state", "decision").unwrap(),
            wire_event("hi", "state_decision").unwrap(),
            "ExtensionId/local-id qualification must remain collision-safe"
        );

        let registry = EventTypeRegistry::new();
        let initial = registry.revision().unwrap();
        let key = registry
            .register::<DecisionAccepted>("histate", "decision_accepted", 1)
            .unwrap();
        assert_eq!(registry.revision().unwrap(), initial + 1);
        assert_eq!(key.registration_revision(), initial + 1);
        assert_eq!(
            registry.revalidate(&key).unwrap().event,
            "#histate_decision_accepted"
        );

        let other = registry
            .register::<Different>("histate", "other", 3)
            .unwrap();
        assert!(registry.revision().unwrap() > key.registration_revision());
        registry.revalidate(&key).unwrap();
        registry.revalidate(&other).unwrap();

        let before_invalid = registry.revision().unwrap();
        assert!(registry
            .register::<Different>("histate", "bad space", 1)
            .is_err());
        assert_eq!(registry.revision().unwrap(), before_invalid);
        assert_eq!(registry.list().unwrap().len(), 2);

        registry.remove(&other, Some(before_invalid)).unwrap();
        assert!(registry.revalidate(&other).is_err());
        registry.revalidate(&key).unwrap();

        let replacement = registry
            .register::<Different>("histate", "other", 3)
            .unwrap();
        assert!(replacement.registration_revision() > other.registration_revision());
        assert!(registry.revalidate(&other).is_err());
        registry.revalidate(&replacement).unwrap();
    }
}
