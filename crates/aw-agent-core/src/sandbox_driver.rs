//! Typed realization definitions. A registered driver is implementation,
//! not a Sandbox grant, a launch authorization or a Process owner.
use crate::{
    publication,
    registry::{NativeDefinition, StagedRegistry},
    DriverKey, Error, Result,
};
use std::{collections::BTreeMap, marker::PhantomData, sync::Arc};

const MAX_SANDBOX_DRIVERS: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxDriverDescriptor {
    pub driver: DriverKey,
    pub registration_revision: u64,
}

/// Native handle bound to the exact registration and its owning registry.
/// Unrelated additions do not invalidate a live handle.
pub struct SandboxDriverHandle<T> {
    descriptor: SandboxDriverDescriptor,
    owner: Arc<()>,
    implementation: PhantomData<fn() -> T>,
}

impl<T> Clone for SandboxDriverHandle<T> {
    fn clone(&self) -> Self {
        Self {
            descriptor: self.descriptor.clone(),
            owner: self.owner.clone(),
            implementation: PhantomData,
        }
    }
}

impl<T> SandboxDriverHandle<T> {
    pub fn descriptor(&self) -> &SandboxDriverDescriptor {
        &self.descriptor
    }
}

#[derive(Clone)]
struct Entry {
    descriptor: SandboxDriverDescriptor,
    implementation: NativeDefinition,
}

type Snapshot = BTreeMap<DriverKey, Entry>;

/// One staged native definition leaf. Add-only until the execution owner
/// can prove that no live or unresolved effect depends on a definition.
#[derive(Clone)]
pub struct SandboxDriverRegistry {
    owner: Arc<StagedRegistry<Snapshot>>,
    identity: Arc<()>,
}

pub struct SandboxDriverRegistryReader {
    reader: publication::Reader<Snapshot>,
    identity: Arc<()>,
}

impl Default for SandboxDriverRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SandboxDriverRegistry {
    pub fn new() -> Self {
        Self {
            owner: Arc::new(StagedRegistry::new(Snapshot::new())),
            identity: Arc::new(()),
        }
    }

    pub fn revision(&self) -> Result<u64> {
        Ok(self.owner.current()?.generation)
    }

    pub fn reader(&self) -> Result<SandboxDriverRegistryReader> {
        Ok(SandboxDriverRegistryReader {
            reader: self.owner.reader_factory().reader()?,
            identity: self.identity.clone(),
        })
    }

    pub fn register<T: Send + Sync + 'static>(
        &self,
        driver: DriverKey,
        implementation: T,
    ) -> Result<SandboxDriverHandle<T>> {
        self.register_if(driver, implementation, None)
    }

    pub fn register_if<T: Send + Sync + 'static>(
        &self,
        driver: DriverKey,
        implementation: T,
        expected_revision: Option<u64>,
    ) -> Result<SandboxDriverHandle<T>> {
        driver.validate()?;
        let stage = self.owner.stage(
            expected_revision,
            |current, revision| {
                if current.contains_key(&driver) {
                    return Err(Error::Conflict("SandboxDriver is already registered"));
                }
                let descriptor = SandboxDriverDescriptor {
                    driver: driver.clone(),
                    registration_revision: revision,
                };
                let mut next = current.clone();
                next.insert(
                    driver,
                    Entry {
                        descriptor: descriptor.clone(),
                        implementation: NativeDefinition::new(implementation),
                    },
                );
                Ok((
                    next,
                    SandboxDriverHandle {
                        descriptor,
                        owner: self.identity.clone(),
                        implementation: PhantomData,
                    },
                ))
            },
            |next| {
                if next.len() > MAX_SANDBOX_DRIVERS {
                    return Err(Error::Limit("too many registered SandboxDrivers"));
                }
                Ok(())
            },
        )?;
        self.owner.publish(stage).map(|(_, handle)| handle)
    }

    pub fn list(&self) -> Result<Vec<SandboxDriverDescriptor>> {
        self.reader()?.list()
    }
}

impl SandboxDriverRegistryReader {
    pub fn revision(&self) -> Result<u64> {
        Ok(self.reader.snapshot()?.generation)
    }

    pub fn get<T: Send + Sync + 'static>(&self, handle: &SandboxDriverHandle<T>) -> Result<Arc<T>> {
        if !Arc::ptr_eq(&self.identity, &handle.owner) {
            return Err(Error::Denied(
                "SandboxDriver handle belongs to another registry",
            ));
        }
        let snapshot = self.reader.snapshot()?;
        let entry = snapshot
            .value
            .get(&handle.descriptor.driver)
            .ok_or(Error::Denied("SandboxDriver handle is stale"))?;
        if entry.descriptor != handle.descriptor {
            return Err(Error::Denied("SandboxDriver handle is stale"));
        }
        entry.implementation.get::<T>()
    }

    pub fn list(&self) -> Result<Vec<SandboxDriverDescriptor>> {
        Ok(self
            .reader
            .snapshot()?
            .value
            .values()
            .map(|entry| entry.descriptor.clone())
            .collect())
    }
}
