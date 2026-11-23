//! Private staged definition-registry publication primitive.
//!
//! A leaf builds and validates one complete successor against an immutable base,
//! then CAS-publishes that already-built snapshot. The first consumer is
//! EventTypeRegistry; later leaf registries reuse this exact owner.

use crate::{publication, Error, Result};
use std::sync::Arc;

pub(crate) struct StagedRegistry<T> {
    publisher: publication::Publisher<T>,
    readers: publication::ReaderFactory<T>,
}

pub(crate) struct RegistryStage<T, R> {
    base_revision: u64,
    next: T,
    result: R,
}

impl<T> StagedRegistry<T>
where
    T: Send + Sync + 'static,
{
    pub(crate) fn new(initial: T) -> Self {
        let (publisher, readers) = publication::channel(initial);
        Self { publisher, readers }
    }

    pub(crate) fn current(&self) -> Result<Arc<publication::Version<T>>> {
        self.publisher.current()
    }

    pub(crate) fn stage<R>(
        &self,
        expected_revision: Option<u64>,
        build: impl FnOnce(&T, u64) -> Result<(T, R)>,
        validate: impl FnOnce(&T) -> Result<()>,
    ) -> Result<RegistryStage<T, R>> {
        let current = self.publisher.current()?;
        if expected_revision.is_some_and(|expected| expected != current.generation) {
            return Err(Error::Conflict("stale registry revision"));
        }
        let next_revision = current
            .generation
            .checked_add(1)
            .ok_or(Error::Limit("registry revision exhausted"))?;
        let (next, result) = build(&current.value, next_revision)?;
        validate(&next)?;
        Ok(RegistryStage {
            base_revision: current.generation,
            next,
            result,
        })
    }

    pub(crate) fn publish<R>(&self, stage: RegistryStage<T, R>) -> Result<(u64, R)> {
        let RegistryStage {
            base_revision,
            next,
            result,
        } = stage;
        self.publisher
            .update_if(Some(base_revision), move |_| Ok((next, result)))
            .map_err(|error| match error {
                Error::Conflict(_) => Error::Conflict("registry changed before staged publication"),
                other => other,
            })
    }

    pub(crate) fn reader_factory(&self) -> publication::ReaderFactory<T> {
        self.readers.clone()
    }
}
