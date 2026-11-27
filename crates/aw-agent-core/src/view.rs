//! Typed pull/read definitions. A View receives an already-authorized
//! host projection, never a resource locator with implicit read authority.
//! Native callers retain typed queries/results; codecs are opt-in at the wire.
use crate::{
    publication,
    registry::{self, NativeDefinition, StagedRegistry},
    Error, Result,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::{any::Any, collections::BTreeMap, io, marker::PhantomData, sync::Arc};

pub const MAX_VIEW_NAMES: usize = 16;
pub const MAX_VIEW_NAME_BYTES: usize = 97;
pub const MAX_VIEW_RESULT_BYTES: usize = 32 * 1024;

/// Bounded public errors: never serialize provider/host diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewFailure {
    Unknown,
    Denied,
    Unavailable,
    StaleSchema,
    ExecutionFailed,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ViewOutcome {
    Ok { value: Value },
    Error { code: ViewFailure },
}

impl From<ViewFailure> for ViewOutcome {
    fn from(code: ViewFailure) -> Self {
        Self::Error { code }
    }
}

/// Trusted native code; neither registration nor invocation grants authority.
/// This read-only leaf is for host presentation, NOT model/durable admission.
/// Such admission additionally needs the originating disclosure manifest.
pub trait View: Send + Sync + 'static {
    type Resource: Send + Sync + 'static;
    type Query: Send + Sync + 'static;
    type Output: Send + Sync + 'static;
    fn read(
        &self,
        resource: &Self::Resource,
        query: Self::Query,
    ) -> std::result::Result<Self::Output, ViewFailure>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewDescriptor {
    pub name: String,
    pub schema_version: u32,
    pub registration_revision: u64,
    pub wire: bool,
}

pub struct ViewHandle<V> {
    descriptor: ViewDescriptor,
    identity: Arc<()>,
    view: PhantomData<fn() -> V>,
}

impl<V> Clone for ViewHandle<V> {
    fn clone(&self) -> Self {
        Self {
            descriptor: self.descriptor.clone(),
            identity: self.identity.clone(),
            view: PhantomData,
        }
    }
}

impl<V> ViewHandle<V> {
    pub fn descriptor(&self) -> &ViewDescriptor {
        &self.descriptor
    }
}

type WireRead =
    fn(&NativeDefinition, &dyn Any, Value, usize) -> std::result::Result<Value, ViewFailure>;

#[derive(Clone)]
struct Entry {
    descriptor: ViewDescriptor,
    implementation: NativeDefinition,
    wire: Option<WireRead>,
}
type Snapshot = BTreeMap<String, Entry>;

#[derive(Clone)]
pub struct ViewRegistry {
    owner: Arc<StagedRegistry<Snapshot>>,
    identity: Arc<()>,
}

pub struct ViewRegistryReader {
    reader: publication::Reader<Snapshot>,
    identity: Arc<()>,
    _owner: std::sync::Weak<StagedRegistry<Snapshot>>,
}

impl Default for ViewRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewRegistry {
    pub fn new() -> Self {
        Self {
            owner: Arc::new(StagedRegistry::new(Snapshot::new())),
            identity: Arc::new(()),
        }
    }

    pub fn revision(&self) -> Result<u64> {
        Ok(self.owner.current()?.generation)
    }

    pub fn reader(&self) -> Result<ViewRegistryReader> {
        Ok(ViewRegistryReader {
            reader: self.owner.reader_factory().reader()?,
            identity: self.identity.clone(),
            _owner: Arc::downgrade(&self.owner),
        })
    }

    pub fn register<V: View>(
        &self,
        extension: &str,
        id: &str,
        schema_version: u32,
        view: V,
        expected_revision: Option<u64>,
    ) -> Result<ViewHandle<V>> {
        self.register_in(extension, id, schema_version, view, None, expected_revision)
    }

    pub fn register_wire<V: View>(
        &self,
        extension: &str,
        id: &str,
        schema_version: u32,
        view: V,
        expected_revision: Option<u64>,
    ) -> Result<ViewHandle<V>>
    where
        V::Query: DeserializeOwned,
        V::Output: Serialize,
    {
        self.register_in(
            extension,
            id,
            schema_version,
            view,
            Some(wire_read::<V>),
            expected_revision,
        )
    }

    fn register_in<V: View>(
        &self,
        extension: &str,
        id: &str,
        schema_version: u32,
        view: V,
        wire: Option<WireRead>,
        expected_revision: Option<u64>,
    ) -> Result<ViewHandle<V>> {
        registry::validate_extension_id(extension)?;
        registry::validate_local_id(id)?;
        if schema_version == 0 {
            return Err(Error::Config("View schema version must be positive".into()));
        }
        let name = format!("{extension}.{id}");
        let stage = self.owner.stage(
            expected_revision,
            |current, revision| {
                if current.contains_key(&name) {
                    return Err(Error::Conflict("View is already registered"));
                }
                let descriptor = ViewDescriptor {
                    name: name.clone(),
                    schema_version,
                    registration_revision: revision,
                    wire: wire.is_some(),
                };
                let mut next = current.clone();
                next.insert(
                    name,
                    Entry {
                        descriptor: descriptor.clone(),
                        implementation: NativeDefinition::new(view),
                        wire,
                    },
                );
                Ok((
                    next,
                    ViewHandle {
                        descriptor,
                        identity: self.identity.clone(),
                        view: PhantomData,
                    },
                ))
            },
            |next| {
                if next.len() > 1024 {
                    return Err(Error::Limit("too many registered Views"));
                }
                Ok(())
            },
        )?;
        self.owner.publish(stage).map(|(_, handle)| handle)
    }

    pub fn list(&self) -> Result<Vec<ViewDescriptor>> {
        Ok(self
            .owner
            .current()?
            .value
            .values()
            .map(|entry| entry.descriptor.clone())
            .collect())
    }

    /// Quiescent definition withdrawal. No resource/authority changes.
    pub fn remove<V: View>(
        &mut self,
        handle: &ViewHandle<V>,
        expected_revision: Option<u64>,
    ) -> Result<u64> {
        if !Arc::ptr_eq(&self.identity, &handle.identity) {
            return Err(Error::Denied("View belongs to another registry"));
        }
        if Arc::get_mut(&mut self.owner).is_none() {
            return Err(Error::Conflict("View registry has live owners or readers"));
        }
        let stage = self.owner.stage(
            expected_revision,
            |current, _| {
                let entry = current
                    .get(&handle.descriptor.name)
                    .filter(|entry| entry.descriptor == handle.descriptor)
                    .ok_or(Error::Denied("View registration unavailable"))?;
                drop(entry.implementation.get::<V>()?);
                entry.implementation.require_idle()?;
                let mut next = current.clone();
                next.remove(&handle.descriptor.name);
                Ok((next, ()))
            },
            |_| Ok(()),
        )?;
        self.owner.publish(stage).map(|(revision, ())| revision)
    }
}

impl ViewRegistryReader {
    pub fn get<V: View>(&self, handle: &ViewHandle<V>) -> Result<Arc<V>> {
        if !Arc::ptr_eq(&self.identity, &handle.identity) {
            return Err(Error::Denied("View belongs to another registry"));
        }
        let snapshot = self.reader.snapshot()?;
        snapshot
            .value
            .get(&handle.descriptor.name)
            .filter(|entry| entry.descriptor == handle.descriptor)
            .ok_or(Error::Denied("View registration unavailable"))?
            .implementation
            .get()
    }

    /// One wire facet over a trusted, already-authorized typed projection.
    /// Neither a missing definition nor an execution/encoding failure can
    /// invalidate the caller's core resource read.
    pub fn read_wire<R: Send + Sync + 'static>(
        &self,
        name: &str,
        resource: &R,
        query: Value,
        max_bytes: usize,
    ) -> ViewOutcome {
        let result = (|| {
            let snapshot = self
                .reader
                .snapshot()
                .map_err(|_| ViewFailure::Unavailable)?;
            let entry = snapshot.value.get(name).ok_or(ViewFailure::Unknown)?;
            let read = entry.wire.ok_or(ViewFailure::Unavailable)?;
            read(
                &entry.implementation,
                resource,
                query,
                max_bytes.min(MAX_VIEW_RESULT_BYTES),
            )
        })();
        match result {
            Ok(value) => ViewOutcome::Ok { value },
            Err(code) => code.into(),
        }
    }
}

struct BoundedJson {
    bytes: Vec<u8>,
    limit: usize,
}
impl io::Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("View output exceeds bound"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn wire_read<V: View>(
    implementation: &NativeDefinition,
    resource: &dyn Any,
    query: Value,
    max_bytes: usize,
) -> std::result::Result<Value, ViewFailure>
where
    V::Query: DeserializeOwned,
    V::Output: Serialize,
{
    let resource = resource
        .downcast_ref::<V::Resource>()
        .ok_or(ViewFailure::Unknown)?;
    let view = implementation
        .get::<V>()
        .map_err(|_| ViewFailure::Unavailable)?;
    let query = serde_json::from_value(query).map_err(|_| ViewFailure::ExecutionFailed)?;
    let output = view.read(resource, query)?;
    let mut writer = BoundedJson {
        bytes: Vec::new(),
        limit: max_bytes,
    };
    serde_json::to_writer(&mut writer, &output).map_err(|_| ViewFailure::ExecutionFailed)?;
    serde_json::from_slice(&writer.bytes).map_err(|_| ViewFailure::ExecutionFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Echo;
    impl View for Echo {
        type Resource = String;
        type Query = usize;
        type Output = String;
        fn read(
            &self,
            resource: &String,
            count: usize,
        ) -> std::result::Result<String, ViewFailure> {
            if count > 4 {
                return Err(ViewFailure::Denied);
            }
            Ok(resource.repeat(count))
        }
    }
    #[test]
    fn typed_staged_views_and_bounded_wire_failures() {
        let mut registry = ViewRegistry::new();
        let key = registry
            .register_wire("fixture", "echo", 1, Echo, Some(0))
            .unwrap();
        assert!(registry
            .register_wire("fixture", "echo", 1, Echo, None)
            .is_err());
        assert!(registry
            .register("fixture", "native", 1, Echo, Some(0))
            .is_err());
        let native = registry
            .register("fixture", "native", 1, Echo, Some(1))
            .unwrap();
        let reader = registry.reader().unwrap();
        let resource = "test".to_owned();
        assert_eq!(
            reader.get(&native).unwrap().read(&resource, 2).unwrap(),
            "testtest"
        );
        assert_eq!(
            reader.read_wire("fixture.echo", &resource, 2.into(), 64),
            ViewOutcome::Ok {
                value: "testtest".into()
            }
        );
        for (name, q, limit, code) in [
            ("fixture.missing", 2.into(), 64, ViewFailure::Unknown),
            ("fixture.native", 2.into(), 64, ViewFailure::Unavailable),
            ("fixture.echo", 5.into(), 64, ViewFailure::Denied),
            ("fixture.echo", 2.into(), 4, ViewFailure::ExecutionFailed),
        ] {
            assert_eq!(reader.read_wire(name, &resource, q, limit), code.into());
        }
        assert_eq!(
            reader.read_wire("fixture.echo", &(), 2.into(), 64),
            ViewFailure::Unknown.into()
        );
        assert!(ViewRegistry::new().reader().unwrap().get(&key).is_err());
        let live = reader.get(&key).unwrap();
        assert!(registry.remove(&key, None).is_err());
        drop(reader);
        assert!(registry.remove(&key, None).is_err());
        drop(live);
        assert_eq!(registry.remove(&key, Some(2)).unwrap(), 3);
        let replacement = registry
            .register_wire("fixture", "echo", 2, Echo, Some(3))
            .unwrap();
        let reader = registry.reader().unwrap();
        assert!(reader.get(&key).is_err());
        assert_eq!(replacement.descriptor().schema_version, 2);
        assert_eq!(registry.list().unwrap().len(), 2);
    }
}
