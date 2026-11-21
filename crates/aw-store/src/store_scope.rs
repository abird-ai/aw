//! Store-local identity and ordered-position mechanics.
//!
//! These are private persistence primitives, not a public resource namespace or
//! global clock. `StorePos` is meaningful only together with the `StoreScope`
//! that owns the corresponding `commit_log.seq`.

use crate::{db, Error, Result};
use rusqlite::Connection;
use std::num::NonZeroU64;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct StoreId(String);

impl StoreId {
    fn new(value: String) -> Result<Self> {
        if value.is_empty() {
            return Err(Error::Storage("invalid Store identity"));
        }
        Ok(Self(value))
    }

    pub(crate) fn load(connection: &Connection) -> Result<Self> {
        let value: String = db(connection.query_row(
            "SELECT value FROM store_meta WHERE key='store_id'",
            [],
            |row| row.get(0),
        ))?;
        Self::new(value)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct RestoreGeneration(NonZeroU64);

impl RestoreGeneration {
    fn new(value: &str) -> Result<Self> {
        let generation = value
            .parse::<u64>()
            .ok()
            .and_then(NonZeroU64::new)
            .ok_or(Error::Storage("invalid restore generation"))?;
        Ok(Self(generation))
    }

    const fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct StoreScope {
    id: StoreId,
    restore_generation: RestoreGeneration,
}

impl StoreScope {
    pub(crate) fn load(connection: &Connection) -> Result<Self> {
        // One statement keeps the frequent coordination path to two indexed
        // point lookups without paying two SQLite prepare/step round trips.
        let (id, restore_generation): (String, String) = db(connection.query_row(
            "SELECT
                (SELECT value FROM store_meta WHERE key='store_id'),
                (SELECT value FROM store_meta WHERE key='restore_generation')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ))?;
        Ok(Self {
            id: StoreId::new(id)?,
            restore_generation: RestoreGeneration::new(&restore_generation)?,
        })
    }

    pub(crate) fn id(&self) -> &StoreId {
        &self.id
    }

    pub(crate) const fn restore_generation(&self) -> u64 {
        self.restore_generation.get()
    }
}

/// Position in one StoreScope's immutable `commit_log.seq`.
///
/// Zero is the natural "before the first commit" projection floor; committed
/// rows themselves are positive.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct StorePos(u64);

impl StorePos {
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn get(self) -> u64 {
        self.0
    }

    pub(crate) fn from_insert_rowid(value: i64) -> Result<Self> {
        let value = u64::try_from(value)
            .ok()
            .filter(|value| *value != 0)
            .ok_or(Error::Storage("commit log sequence overflow"))?;
        Ok(Self(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_reads_existing_identity_and_changes_with_restore_generation() {
        let store = crate::SessionStore::in_memory().unwrap();
        let connection = store.connection().unwrap();
        let before = StoreScope::load(&connection).unwrap();
        assert!(!before.id().as_str().is_empty());
        let next = before.restore_generation().checked_add(1).unwrap();
        connection
            .execute(
                "UPDATE store_meta SET value=?1 WHERE key='restore_generation'",
                [next.to_string()],
            )
            .unwrap();
        let after = StoreScope::load(&connection).unwrap();
        assert_eq!(before.id(), after.id());
        assert_ne!(before, after);
        assert_eq!(after.restore_generation(), next);
    }

    #[test]
    fn store_position_is_zero_cost_and_keeps_projection_floor() {
        assert_eq!(std::mem::size_of::<StorePos>(), std::mem::size_of::<u64>());
        assert_eq!(StorePos::new(0).get(), 0);
        assert_eq!(StorePos::from_insert_rowid(8).unwrap(), StorePos::new(8));
        assert!(StorePos::from_insert_rowid(0).is_err());
        assert!(StorePos::new(8) > StorePos::new(7));
    }
}
