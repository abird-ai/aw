//! Project-scoped Job-root exposure ordering.
//! Stores only owner/ref identity + immutable list position. Lifecycle, output
//! and Agent history remain in their authoritative owners.

use crate::{Error, Result};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Mutex,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ExposureOwner {
    Finite(String),
    /// Phase-3 publication hook; no Agent root is inserted before Agent Job wiring.
    #[allow(dead_code)]
    Agent(String),
}

impl ExposureOwner {
    fn key(&self) -> String {
        match self {
            Self::Finite(id) => format!("f:{id}"),
            Self::Agent(reference) => format!("a:{reference}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    owner: ExposureOwner,
    position: u64,
    published: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExposureCursor {
    pub position: u64,
    pub owner_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExposureItem {
    pub owner: ExposureOwner,
    pub position: u64,
}

struct State {
    next_position: u64,
    entries: HashMap<String, Entry>,
    published: BTreeMap<(u64, String), String>,
}

pub(crate) struct ExposureIndex {
    max_entries: usize,
    state: Mutex<State>,
}

impl ExposureIndex {
    pub(crate) fn new(max_entries: usize) -> Result<Self> {
        if max_entries == 0 {
            return Err(Error::Config(
                "Job exposure index requires positive capacity".into(),
            ));
        }
        Ok(Self {
            max_entries,
            state: Mutex::new(State {
                next_position: 1,
                entries: HashMap::new(),
                published: BTreeMap::new(),
            }),
        })
    }

    /// Reserve recoverability capacity before an effect can start. No public
    /// position exists until the same owner is actually exposed.
    pub(crate) fn reserve_finite(&self, id: &str) -> Result<()> {
        let owner = ExposureOwner::Finite(id.to_owned());
        let key = owner.key();
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Protocol("Job exposure index poisoned"))?;
        if state.entries.contains_key(&key) {
            return Ok(());
        }
        if state.entries.len() >= self.max_entries {
            return Err(Error::Limit(
                "Job exposure recoverability capacity exhausted",
            ));
        }
        let position = state.next_position;
        state.next_position = state
            .next_position
            .checked_add(1)
            .ok_or(Error::Limit("Job exposure sequence exhausted"))?;
        state.entries.insert(
            key,
            Entry {
                owner,
                position,
                published: false,
            },
        );
        Ok(())
    }

    pub(crate) fn publish_finite(&self, id: &str) -> Result<u64> {
        self.publish(ExposureOwner::Finite(id.to_owned()))
    }

    /// Phase-3 hook. The Session/Agent owner must commit Agent creation and
    /// this publication atomically at its own authoritative boundary.
    #[allow(dead_code)]
    pub(crate) fn publish_agent(&self, canonical_ref: &str) -> Result<u64> {
        self.publish(ExposureOwner::Agent(canonical_ref.to_owned()))
    }

    fn publish(&self, owner: ExposureOwner) -> Result<u64> {
        let key = owner.key();
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Protocol("Job exposure index poisoned"))?;
        if !state.entries.contains_key(&key) {
            if state.entries.len() >= self.max_entries {
                return Err(Error::Limit("Job exposure index capacity exhausted"));
            }
            let position = state.next_position;
            state.next_position = state
                .next_position
                .checked_add(1)
                .ok_or(Error::Limit("Job exposure sequence exhausted"))?;
            state.entries.insert(
                key.clone(),
                Entry {
                    owner: owner.clone(),
                    position,
                    published: false,
                },
            );
        }
        let (position, already_published) = {
            let entry = state
                .entries
                .get_mut(&key)
                .ok_or(Error::Protocol("Job exposure reservation disappeared"))?;
            entry.owner = owner;
            (entry.position, entry.published)
        };
        if !already_published {
            state.published.insert((position, key.clone()), key.clone());
            state
                .entries
                .get_mut(&key)
                .ok_or(Error::Protocol("Job exposure reservation disappeared"))?
                .published = true;
        }
        Ok(position)
    }

    /// Inline hidden completion can release only an unpublished reservation.
    pub(crate) fn release_unpublished_finite(&self, id: &str) -> Result<()> {
        let key = ExposureOwner::Finite(id.to_owned()).key();
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Protocol("Job exposure index poisoned"))?;
        match state.entries.get(&key) {
            None => Ok(()),
            Some(entry) if !entry.published => {
                state.entries.remove(&key);
                Ok(())
            }
            Some(_) => Err(Error::Conflict(
                "published Job exposure cannot be released as hidden",
            )),
        }
    }

    pub(crate) fn list(
        &self,
        cursor: Option<&ExposureCursor>,
        limit: usize,
    ) -> Result<(Vec<ExposureItem>, Option<ExposureCursor>)> {
        if limit == 0 || limit > self.max_entries {
            return Err(Error::Denied("invalid Job exposure list limit"));
        }
        let state = self
            .state
            .lock()
            .map_err(|_| Error::Protocol("Job exposure index poisoned"))?;
        let mut all = state
            .published
            .iter()
            .rev()
            .filter_map(|((position, owner_key), key)| {
                let older = cursor.is_none_or(|cursor| {
                    *position < cursor.position
                        || (*position == cursor.position && owner_key > &cursor.owner_key)
                });
                older.then(|| {
                    state.entries.get(key).map(|entry| ExposureItem {
                        owner: entry.owner.clone(),
                        position: *position,
                    })
                })?
            })
            .collect::<Vec<_>>();
        let has_more = all.len() > limit;
        all.truncate(limit);
        let next = has_more.then(|| {
            let last = all.last().expect("has_more implies one returned exposure");
            ExposureCursor {
                position: last.position,
                owner_key: last.owner.key(),
            }
        });
        Ok((all, next))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserve_before_publish_and_mixed_owners_share_one_order() {
        let index = ExposureIndex::new(8).unwrap();
        index.reserve_finite("j1").unwrap();
        assert!(index.list(None, 8).unwrap().0.is_empty());
        assert_eq!(index.publish_finite("j1").unwrap(), 1);
        assert_eq!(index.publish_agent("/d:d/i:i/p:p/a:1").unwrap(), 2);
        index.reserve_finite("j2").unwrap();
        assert_eq!(index.publish_finite("j2").unwrap(), 3);

        let (first, cursor) = index.list(None, 2).unwrap();
        assert_eq!(
            first.iter().map(|item| item.position).collect::<Vec<_>>(),
            vec![3, 2]
        );
        let (second, _) = index.list(cursor.as_ref(), 2).unwrap();
        assert_eq!(
            second.iter().map(|item| item.position).collect::<Vec<_>>(),
            vec![1]
        );
    }

    #[test]
    fn hidden_inline_completion_releases_reservation_but_never_published_entry() {
        let index = ExposureIndex::new(1).unwrap();
        index.reserve_finite("hidden").unwrap();
        index.release_unpublished_finite("hidden").unwrap();
        index.reserve_finite("visible").unwrap();
        index.publish_finite("visible").unwrap();
        assert!(index.release_unpublished_finite("visible").is_err());
        assert!(index.reserve_finite("other").is_err());
    }

    #[test]
    fn publication_is_idempotent_and_does_not_reorder_activity() {
        let index = ExposureIndex::new(4).unwrap();
        index.reserve_finite("j1").unwrap();
        let first = index.publish_finite("j1").unwrap();
        let again = index.publish_finite("j1").unwrap();
        assert_eq!(first, again);
        index.publish_agent("/d:d/i:i/p:p/a:1").unwrap();
        let (items, _) = index.list(None, 4).unwrap();
        assert_eq!(items[1].position, first);
    }
}
