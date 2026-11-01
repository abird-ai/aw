//! Exact ownership for mutation-private names. Filename shape is never proof.
//! Hosts provide durable protected storage; the registry provides one bounded
//! live view shared by discovery, exact-path admission and native cleanup.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex},
};

pub const MAX_PRIVATE_ARTIFACTS: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ObjectIdentity {
    pub volume: u64,
    pub object: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Stage,
    Backup,
    Tombstone,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub kind: Kind,
    /// Last known model/root-relative parent spelling. Recovery may use it only
    /// after proving the current parent identity still matches `parent`.
    pub parent_path: String,
    pub parent: ObjectIdentity,
    pub name: String,
    pub object: ObjectIdentity,
}
impl Record {
    fn key(&self) -> (ObjectIdentity, String) {
        (self.parent, self.name.clone())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Capacity,
    Conflict,
    Poisoned,
    Store(String),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Capacity => f.write_str("private mutation artifact capacity exhausted"),
            Self::Conflict => f.write_str("private mutation artifact identity conflict"),
            Self::Poisoned => f.write_str("private mutation artifact registry poisoned"),
            Self::Store(value) => write!(f, "private mutation artifact store: {value}"),
        }
    }
}
impl std::error::Error for Error {}

/// Protected durable storage supplied by the product host. `reserve` must be
/// durable before success; `retire` must durably remove the record. The shared
/// registry never infers ownership from a filename after either call fails.
pub trait Store: Send + Sync + 'static {
    /// True only when a successful reservation survives process loss and is
    /// available to a later recovery owner. Hosts implementing this trait own
    /// the durability guarantee; volatile stores must report false.
    fn durable(&self) -> bool {
        true
    }
    fn load(&self) -> Result<Vec<Record>, Error>;
    fn reserve(&self, record: &Record) -> Result<(), Error>;
    fn retire(&self, record: &Record) -> Result<(), Error>;
}

#[derive(Default)]
pub struct MemoryStore;
impl Store for MemoryStore {
    fn durable(&self) -> bool {
        false
    }
    fn load(&self) -> Result<Vec<Record>, Error> {
        Ok(Vec::new())
    }
    fn reserve(&self, _record: &Record) -> Result<(), Error> {
        Ok(())
    }
    fn retire(&self, _record: &Record) -> Result<(), Error> {
        Ok(())
    }
}

pub struct Registry {
    store: Arc<dyn Store>,
    records: Mutex<BTreeMap<(ObjectIdentity, String), Record>>,
}
impl Registry {
    pub fn memory() -> Arc<Self> {
        Arc::new(Self {
            store: Arc::new(MemoryStore),
            records: Mutex::new(BTreeMap::new()),
        })
    }

    pub fn open(store: Arc<dyn Store>) -> Result<Arc<Self>, Error> {
        let loaded = store.load()?;
        if loaded.len() > MAX_PRIVATE_ARTIFACTS {
            return Err(Error::Capacity);
        }
        let mut records = BTreeMap::new();
        for record in loaded {
            if records.insert(record.key(), record).is_some() {
                return Err(Error::Conflict);
            }
        }
        Ok(Arc::new(Self {
            store,
            records: Mutex::new(records),
        }))
    }

    pub fn is_durable(&self) -> bool {
        self.store.durable()
    }

    /// Reserve durable ownership before any user bytes are written to the
    /// already-created restrictive artifact. A tombstone changes the public
    /// namespace before cleanup, so a volatile registry may never authorize it.
    pub fn reserve(self: &Arc<Self>, record: Record) -> Result<Lease, Error> {
        if record.kind == Kind::Tombstone && !self.store.durable() {
            return Err(Error::Store(
                "tombstone requires durable private-artifact storage".into(),
            ));
        }
        let key = record.key();
        let mut records = self.records.lock().map_err(|_| Error::Poisoned)?;
        if records.contains_key(&key) {
            return Err(Error::Conflict);
        }
        if records.len() >= MAX_PRIVATE_ARTIFACTS {
            return Err(Error::Capacity);
        }
        self.store.reserve(&record)?;
        records.insert(key, record.clone());
        Ok(Lease {
            registry: self.clone(),
            record: Some(record),
        })
    }

    /// Ownership is exact only when namespace parent, basename and current
    /// no-follow object identity all match the durable/live record.
    pub fn owns(
        &self,
        parent: ObjectIdentity,
        name: &str,
        object: ObjectIdentity,
    ) -> Result<bool, Error> {
        let records = self.records.lock().map_err(|_| Error::Poisoned)?;
        Ok(records
            .get(&(parent, name.to_owned()))
            .is_some_and(|record| record.object == object))
    }

    pub fn records(&self) -> Result<Vec<Record>, Error> {
        let records = self.records.lock().map_err(|_| Error::Poisoned)?;
        Ok(records.values().cloned().collect())
    }

    pub fn retire_record(&self, record: &Record) -> Result<(), Error> {
        self.retire(record)
    }

    fn retire(&self, record: &Record) -> Result<(), Error> {
        let key = record.key();
        let mut records = self.records.lock().map_err(|_| Error::Poisoned)?;
        let Some(current) = records.get(&key) else {
            return Ok(());
        };
        if current != record {
            return Err(Error::Conflict);
        }
        // Durable retirement precedes removing the live ownership claim. A
        // failed store write therefore leaves the exact name protected.
        self.store.retire(record)?;
        records.remove(&key);
        Ok(())
    }
}

/// An artifact record is deliberately not retired on Drop: a lost cleanup
/// outcome must leave the exact record available for recovery, not make a
/// possibly-still-present artifact model-visible. Call `retire` only after the
/// private name is proven gone.
pub struct Lease {
    registry: Arc<Registry>,
    record: Option<Record>,
}
impl Lease {
    pub fn record(&self) -> &Record {
        self.record.as_ref().expect("live private artifact lease")
    }
    pub fn retire(mut self) -> Result<(), Error> {
        let record = self.record.take().expect("live private artifact lease");
        self.registry.retire(&record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(object: u64) -> Record {
        Record {
            kind: Kind::Stage,
            parent_path: "src".into(),
            parent: ObjectIdentity {
                volume: 1,
                object: 2,
            },
            name: ".stage".into(),
            object: ObjectIdentity { volume: 1, object },
        }
    }
    #[test]
    fn ownership_requires_exact_parent_name_and_object() {
        let registry = Registry::memory();
        let lease = registry.reserve(record(3)).unwrap();
        assert!(registry
            .owns(
                ObjectIdentity {
                    volume: 1,
                    object: 2
                },
                ".stage",
                ObjectIdentity {
                    volume: 1,
                    object: 3
                }
            )
            .unwrap());
        assert!(!registry
            .owns(
                ObjectIdentity {
                    volume: 1,
                    object: 2
                },
                ".stage",
                ObjectIdentity {
                    volume: 1,
                    object: 4
                }
            )
            .unwrap());
        lease.retire().unwrap();
        assert!(registry.records().unwrap().is_empty());
    }

    #[test]
    fn volatile_registry_refuses_tombstone_before_namespace_detach() {
        let registry = Registry::memory();
        let mut value = record(9);
        value.kind = Kind::Tombstone;
        assert!(!registry.is_durable());
        assert!(matches!(registry.reserve(value), Err(Error::Store(_))));
        assert!(registry.records().unwrap().is_empty());
    }

    #[test]
    fn dropped_lease_does_not_discard_recovery_record() {
        let registry = Registry::memory();
        drop(registry.reserve(record(5)).unwrap());
        assert_eq!(registry.records().unwrap(), [record(5)]);
    }
}
