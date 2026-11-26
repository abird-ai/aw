//! Vault definitions, not credentials or grants. The original staged
//! registry owns publication; typed native handles retain its exact identity.
use crate::{
    publication,
    registry::{NativeDefinition, StagedRegistry},
    DriverKey, Error, Result,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, marker::PhantomData, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultProjection {
    Broker,
    Socket,
    File,
    Env,
}

/// Opaque provider-owned entry identity, never a credential or target path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultEntryRef {
    pub provider: DriverKey,
    pub entry: String,
}

impl VaultEntryRef {
    pub fn validate(&self) -> Result<()> {
        self.provider.validate()?;
        if self.entry.is_empty()
            || self.entry.len() > 512
            || !self
                .entry
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
        {
            return Err(Error::Config("invalid opaque Vault entry identity".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultProviderDescriptor {
    pub provider: DriverKey,
    pub registration_revision: u64,
    pub projections: Vec<VaultProjection>,
}

pub struct VaultProviderHandle<T> {
    descriptor: VaultProviderDescriptor,
    owner: Arc<()>,
    implementation: PhantomData<fn() -> T>,
}

impl<T> Clone for VaultProviderHandle<T> {
    fn clone(&self) -> Self {
        Self {
            descriptor: self.descriptor.clone(),
            owner: self.owner.clone(),
            implementation: PhantomData,
        }
    }
}

impl<T> VaultProviderHandle<T> {
    pub fn descriptor(&self) -> &VaultProviderDescriptor {
        &self.descriptor
    }
}

#[derive(Clone)]
struct Entry {
    descriptor: VaultProviderDescriptor,
    implementation: NativeDefinition,
}

type Snapshot = BTreeMap<DriverKey, Entry>;

#[derive(Clone)]
pub struct VaultProviderRegistry {
    owner: Arc<StagedRegistry<Snapshot>>,
    identity: Arc<()>,
}

pub struct VaultProviderRegistryReader {
    reader: publication::Reader<Snapshot>,
    identity: Arc<()>,
    _owner: std::sync::Weak<StagedRegistry<Snapshot>>,
}

impl Default for VaultProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl VaultProviderRegistry {
    pub fn new() -> Self {
        Self {
            owner: Arc::new(StagedRegistry::new(Snapshot::new())),
            identity: Arc::new(()),
        }
    }

    pub fn revision(&self) -> Result<u64> {
        Ok(self.owner.current()?.generation)
    }

    pub fn reader(&self) -> Result<VaultProviderRegistryReader> {
        Ok(VaultProviderRegistryReader {
            reader: self.owner.reader_factory().reader()?,
            identity: self.identity.clone(),
            _owner: Arc::downgrade(&self.owner),
        })
    }

    pub fn register<T: Send + Sync + 'static>(
        &self,
        provider: DriverKey,
        projections: Vec<VaultProjection>,
        implementation: T,
    ) -> Result<VaultProviderHandle<T>> {
        self.register_if(provider, projections, implementation, None)
    }

    pub fn register_if<T: Send + Sync + 'static>(
        &self,
        provider: DriverKey,
        mut projections: Vec<VaultProjection>,
        implementation: T,
        expected_revision: Option<u64>,
    ) -> Result<VaultProviderHandle<T>> {
        provider.validate()?;
        projections.sort_unstable();
        projections.dedup();
        if projections.is_empty() {
            return Err(Error::Config(
                "VaultProvider needs a projection kind".into(),
            ));
        }
        let stage = self.owner.stage(
            expected_revision,
            |current, revision| {
                if current.contains_key(&provider) {
                    return Err(Error::Conflict("VaultProvider is already registered"));
                }
                let descriptor = VaultProviderDescriptor {
                    provider: provider.clone(),
                    registration_revision: revision,
                    projections,
                };
                let mut next = current.clone();
                next.insert(
                    provider,
                    Entry {
                        descriptor: descriptor.clone(),
                        implementation: NativeDefinition::new(implementation),
                    },
                );
                Ok((
                    next,
                    VaultProviderHandle {
                        descriptor,
                        owner: self.identity.clone(),
                        implementation: PhantomData,
                    },
                ))
            },
            |next| {
                if next.len() > 1024 {
                    return Err(Error::Limit("too many registered VaultProviders"));
                }
                Ok(())
            },
        )?;
        self.owner.publish(stage).map(|(_, handle)| handle)
    }

    /// Quiescent definition withdrawal only. No grant, provider entry or
    /// SecretStore mutation. Old handles fail closed; current durable
    /// grants may be unavailable until the original provider returns.
    pub fn remove<T: Send + Sync + 'static>(
        &mut self,
        handle: &VaultProviderHandle<T>,
        expected_revision: Option<u64>,
    ) -> Result<u64> {
        if Arc::get_mut(&mut self.owner).is_none() {
            return Err(Error::Conflict(
                "VaultProvider registry has live owners or readers",
            ));
        }
        if !Arc::ptr_eq(&self.identity, &handle.owner) {
            return Err(Error::Denied("VaultProvider belongs to another registry"));
        }
        let stage = self.owner.stage(
            expected_revision,
            |current, _| {
                let entry = current
                    .get(&handle.descriptor.provider)
                    .filter(|entry| entry.descriptor == handle.descriptor)
                    .ok_or(Error::Denied("VaultProvider registration unavailable"))?;
                drop(entry.implementation.get::<T>()?);
                entry.implementation.require_idle()?;
                let mut next = current.clone();
                next.remove(&handle.descriptor.provider);
                Ok((next, ()))
            },
            |_| Ok(()),
        )?;
        self.owner.publish(stage).map(|(revision, ())| revision)
    }
}

impl VaultProviderRegistryReader {
    pub fn revision(&self) -> Result<u64> {
        Ok(self.reader.snapshot()?.generation)
    }

    pub fn get<T: Send + Sync + 'static>(&self, handle: &VaultProviderHandle<T>) -> Result<Arc<T>> {
        if !Arc::ptr_eq(&self.identity, &handle.owner) {
            return Err(Error::Denied("VaultProvider belongs to another registry"));
        }
        let snapshot = self.reader.snapshot()?;
        let entry = snapshot
            .value
            .get(&handle.descriptor.provider)
            .filter(|entry| entry.descriptor == handle.descriptor)
            .ok_or(Error::Denied("VaultProvider registration unavailable"))?;
        entry.implementation.get::<T>()
    }

    pub fn require_projection(
        &self,
        entry: &VaultEntryRef,
        projection: VaultProjection,
    ) -> Result<()> {
        entry.validate()?;
        let snapshot = self.reader.snapshot()?;
        if snapshot
            .value
            .get(&entry.provider)
            .is_none_or(|definition| !definition.descriptor.projections.contains(&projection))
        {
            return Err(Error::Denied("VaultProvider projection unavailable"));
        }
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<VaultProviderDescriptor>> {
        Ok(self
            .reader
            .snapshot()?
            .value
            .values()
            .map(|entry| entry.descriptor.clone())
            .collect())
    }
}
