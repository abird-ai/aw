//! Pure, bounded identity and lifecycle for the two runtime actor kinds.
//! Neither an ActorKey nor a process ID is authority: hosted callers must also
//! validate their authenticated Session, principal, site and Envelope.
use alloc::{string::String, vec::Vec};
use core::num::NonZeroU64;

pub const MAX_ACTORS: usize = 64;
pub const MAX_ACTOR_DEPTH: u8 = 8;
pub const MAX_ACTOR_LABEL_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RuntimeKey {
    pub id: NonZeroU64,
    pub generation: NonZeroU64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ActorKey {
    pub runtime: RuntimeKey,
    pub id: NonZeroU64,
    pub generation: NonZeroU64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorKind {
    Agent,
    Process,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorState {
    Starting,
    Running,
    Waiting,
    Stalled,
    Frozen,
    CancelRequested,
    Settled,
    Failed,
    Cancelled,
    /// An external process/effect cannot be established as finished.
    Unknown,
}
impl ActorState {
    pub const fn terminal(self) -> bool {
        matches!(
            self,
            Self::Settled | Self::Failed | Self::Cancelled | Self::Unknown
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Capacity,
    InvalidKey,
    StaleGeneration,
    InvalidParent,
    DepthExceeded,
    InvalidLabel,
    InvalidTransition,
    ExhaustedGeneration,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorDescriptor {
    pub key: ActorKey,
    pub kind: ActorKind,
    pub parent: Option<ActorKey>,
    pub depth: u8,
    pub label: String,
    pub state: ActorState,
    pub envelope_revision: NonZeroU64,
}
#[derive(Clone, Debug)]
struct Slot {
    generation: NonZeroU64,
    actor: Option<ActorDescriptor>,
}
/// The slot generation is retained after release so a stale ActorKey cannot
/// refer to a later allocation. Bounded vector, no OS access or global state.
pub struct ActorDirectory {
    runtime: RuntimeKey,
    slots: Vec<Slot>,
    limit: usize,
}
impl ActorDirectory {
    pub fn new(runtime: RuntimeKey, limit: usize) -> Result<Self, Error> {
        if limit == 0 || limit > MAX_ACTORS {
            return Err(Error::Capacity);
        }
        Ok(Self {
            runtime,
            slots: Vec::new(),
            limit,
        })
    }
    pub fn runtime(&self) -> RuntimeKey {
        self.runtime
    }
    pub fn count(&self) -> usize {
        self.slots.iter().filter(|s| s.actor.is_some()).count()
    }
    pub fn spawn(
        &mut self,
        kind: ActorKind,
        parent: Option<ActorKey>,
        label: &str,
        envelope_revision: NonZeroU64,
    ) -> Result<ActorKey, Error> {
        if label.is_empty()
            || label.len() > MAX_ACTOR_LABEL_BYTES
            || label.chars().any(char::is_control)
        {
            return Err(Error::InvalidLabel);
        }
        let depth = match parent {
            Some(key) => {
                let parent = self.get(key).ok_or(Error::InvalidParent)?;
                if parent.state.terminal() || parent.state == ActorState::CancelRequested {
                    return Err(Error::InvalidParent);
                }
                parent.depth.checked_add(1).ok_or(Error::DepthExceeded)?
            }
            None => 0,
        };
        if depth > MAX_ACTOR_DEPTH {
            return Err(Error::DepthExceeded);
        }
        let slot = self.slots.iter().position(|s| s.actor.is_none());
        let index = if let Some(index) = slot {
            index
        } else {
            if self.slots.len() == self.limit {
                return Err(Error::Capacity);
            }
            self.slots.try_reserve(1).map_err(|_| Error::Capacity)?;
            self.slots.push(Slot {
                generation: NonZeroU64::new(1).expect("one is nonzero"),
                actor: None,
            });
            self.slots.len() - 1
        };
        let s = &mut self.slots[index];
        let id = NonZeroU64::new((index as u64) + 1).expect("index starts at one");
        let key = ActorKey {
            runtime: self.runtime,
            id,
            generation: s.generation,
        };
        s.actor = Some(ActorDescriptor {
            key,
            kind,
            parent,
            depth,
            label: label.into(),
            state: ActorState::Starting,
            envelope_revision,
        });
        Ok(key)
    }
    fn slot(&self, key: ActorKey) -> Result<&Slot, Error> {
        if key.runtime != self.runtime {
            return Err(Error::InvalidKey);
        }
        let index = usize::try_from(key.id.get() - 1).map_err(|_| Error::InvalidKey)?;
        let slot = self.slots.get(index).ok_or(Error::InvalidKey)?;
        if slot.generation != key.generation {
            return Err(Error::StaleGeneration);
        }
        Ok(slot)
    }
    pub fn get(&self, key: ActorKey) -> Option<&ActorDescriptor> {
        self.slot(key).ok()?.actor.as_ref()
    }
    pub fn check(&self, key: ActorKey) -> Result<&ActorDescriptor, Error> {
        self.slot(key)?.actor.as_ref().ok_or(Error::InvalidKey)
    }
    pub fn set_envelope_revision(
        &mut self,
        key: ActorKey,
        revision: NonZeroU64,
    ) -> Result<(), Error> {
        self.check(key)?;
        self.slots[(key.id.get() - 1) as usize]
            .actor
            .as_mut()
            .expect("checked")
            .envelope_revision = revision;
        Ok(())
    }
    /// Only legal lifecycle edges; advisory Stalled and cooperative Frozen are
    /// not terminal states. No completion can overwrite an Unknown outcome.
    pub fn transition(&mut self, key: ActorKey, next: ActorState) -> Result<(), Error> {
        let current = self.check(key)?.state;
        if current == next {
            return Ok(());
        }
        let valid = match current {
            ActorState::Starting => matches!(
                next,
                ActorState::Running
                    | ActorState::Failed
                    | ActorState::Cancelled
                    | ActorState::Unknown
            ),
            ActorState::Running | ActorState::Waiting | ActorState::Stalled => matches!(
                next,
                ActorState::Running
                    | ActorState::Waiting
                    | ActorState::Stalled
                    | ActorState::Frozen
                    | ActorState::CancelRequested
                    | ActorState::Settled
                    | ActorState::Failed
                    | ActorState::Cancelled
                    | ActorState::Unknown
            ),
            ActorState::Frozen => matches!(
                next,
                ActorState::Running | ActorState::CancelRequested | ActorState::Unknown
            ),
            ActorState::CancelRequested => matches!(
                next,
                ActorState::Cancelled
                    | ActorState::Settled
                    | ActorState::Failed
                    | ActorState::Unknown
            ),
            ActorState::Settled
            | ActorState::Failed
            | ActorState::Cancelled
            | ActorState::Unknown => false,
        };
        if !valid {
            return Err(Error::InvalidTransition);
        }
        self.slots[(key.id.get() - 1) as usize]
            .actor
            .as_mut()
            .expect("checked")
            .state = next;
        Ok(())
    }
    /// Children are released before their parent. The owner must have recorded
    /// terminal evidence before release; no unfinished process is discarded.
    pub fn release(&mut self, key: ActorKey) -> Result<(), Error> {
        let a = self.check(key)?;
        // Unknown external effects require an owning host's explicit durable
        // reconciliation. A pure registry cannot assert that proof and must
        // retain the identity rather than silently freeing/reusing its slot.
        if !a.state.terminal() || a.state == ActorState::Unknown {
            return Err(Error::InvalidTransition);
        }
        if self
            .slots
            .iter()
            .any(|slot| slot.actor.as_ref().is_some_and(|x| x.parent == Some(key)))
        {
            return Err(Error::InvalidTransition);
        }
        let index = (key.id.get() - 1) as usize;
        let next = self.slots[index]
            .generation
            .get()
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .ok_or(Error::ExhaustedGeneration)?;
        self.slots[index].actor = None;
        self.slots[index].generation = next;
        Ok(())
    }
    pub fn snapshot(&self) -> Vec<ActorDescriptor> {
        self.slots
            .iter()
            .filter_map(|slot| slot.actor.clone())
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn nz(n: u64) -> NonZeroU64 {
        NonZeroU64::new(n).unwrap()
    }
    fn new_dir(limit: usize) -> ActorDirectory {
        ActorDirectory::new(
            RuntimeKey {
                id: nz(1),
                generation: nz(2),
            },
            limit,
        )
        .unwrap()
    }
    #[test]
    fn exact_generation_blocks_aba() {
        let mut d = new_dir(1);
        let old = d.spawn(ActorKind::Agent, None, "root", nz(1)).unwrap();
        assert_eq!(
            d.spawn(ActorKind::Process, None, "other", nz(1)),
            Err(Error::Capacity)
        );
        d.transition(old, ActorState::Running).unwrap();
        d.transition(old, ActorState::Settled).unwrap();
        d.release(old).unwrap();
        let new = d.spawn(ActorKind::Process, None, "process", nz(1)).unwrap();
        assert_eq!(new.id, old.id);
        assert_ne!(new.generation, old.generation);
        assert_eq!(d.check(old), Err(Error::StaleGeneration));
        assert_eq!(
            d.transition(old, ActorState::Cancelled),
            Err(Error::StaleGeneration)
        );
        assert!(d.check(new).is_ok());
    }
    #[test]
    fn lifecycle_and_lineage_are_bounded() {
        let mut d = new_dir(10);
        let root = d.spawn(ActorKind::Agent, None, "root", nz(1)).unwrap();
        let mut parent = root;
        for _ in 0..MAX_ACTOR_DEPTH {
            parent = d
                .spawn(ActorKind::Agent, Some(parent), "child", nz(1))
                .unwrap();
        }
        assert_eq!(
            d.spawn(ActorKind::Agent, Some(parent), "too-deep", nz(1)),
            Err(Error::DepthExceeded)
        );
        assert_eq!(d.release(root), Err(Error::InvalidTransition));
        d.transition(root, ActorState::Running).unwrap();
        d.transition(root, ActorState::Frozen).unwrap();
        assert_eq!(
            d.transition(root, ActorState::Settled),
            Err(Error::InvalidTransition)
        );
        d.transition(root, ActorState::CancelRequested).unwrap();
        d.transition(root, ActorState::Unknown).unwrap();
        assert_eq!(
            d.transition(root, ActorState::Running),
            Err(Error::InvalidTransition)
        );
        assert_eq!(d.release(root), Err(Error::InvalidTransition)); // children still exist
    }
    #[test]
    fn wrong_runtime_invalid_parent_and_invalid_label() {
        let mut d = new_dir(2);
        let root = d.spawn(ActorKind::Agent, None, "root", nz(1)).unwrap();
        let wrong = ActorKey {
            runtime: RuntimeKey {
                id: nz(5),
                generation: nz(2),
            },
            ..root
        };
        assert_eq!(d.check(wrong), Err(Error::InvalidKey));
        assert_eq!(
            d.spawn(ActorKind::Process, Some(wrong), "sub", nz(1)),
            Err(Error::InvalidParent)
        );
        assert_eq!(
            d.spawn(ActorKind::Process, None, "\u{1b}bad", nz(1)),
            Err(Error::InvalidLabel)
        );
    }
}
