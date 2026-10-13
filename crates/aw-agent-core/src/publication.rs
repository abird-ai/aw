//! Hosted immutable publication. Readers own a registered handle and return
//! owned snapshots; no left-right guard escapes this module.
use crate::{Error, Result};
use left_right::{Absorb, ReadHandle, ReadHandleFactory, WriteHandle};
use std::marker::PhantomData;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

/// One complete immutable version. A generation advances only on publication.
#[derive(Debug)]
pub struct Version<T> {
    pub generation: u64,
    pub value: T,
}

struct Slot<T>(Arc<Version<T>>);
impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Absorb<Arc<Version<T>>> for Slot<T> {
    fn absorb_first(&mut self, next: &mut Arc<Version<T>>, _: &Self) {
        self.0 = next.clone();
    }
    fn absorb_second(&mut self, next: Arc<Version<T>>, _: &Self) {
        self.0 = next;
    }
    fn sync_with(&mut self, first: &Self) {
        self.0 = first.0.clone();
    }
}
struct Writer<T> {
    current: Arc<Version<T>>,
    handle: WriteHandle<Slot<T>, Arc<Version<T>>>,
}

/// One logical publisher. Its short writer lock serializes existing producers;
/// readers never acquire it. Mutation runs once, before deterministic absorption.
pub struct Publisher<T> {
    writer: Mutex<Writer<T>>,
    available: Arc<AtomicBool>,
}

/// Share this factory, then register one reader per consumer/task at setup.
/// Registering a reader takes an upstream lock; it is not a hot read operation.
/// Its auto traits must reflect the published value, not just the upstream raw
/// pointer. Non-thread-safe values cannot yield a transferable/shared factory.
///
/// ```compile_fail
/// use aw_agent_core::publication::ReaderFactory;
/// fn send<T: Send>() {}
/// send::<ReaderFactory<std::cell::Cell<u8>>>();
/// ```
/// ```compile_fail
/// use aw_agent_core::publication::ReaderFactory;
/// fn sync<T: Sync>() {}
/// sync::<ReaderFactory<std::cell::Cell<u8>>>();
/// ```
/// ```compile_fail
/// use aw_agent_core::publication::ReaderFactory;
/// fn send<T: Send>() {}
/// send::<ReaderFactory<std::rc::Rc<u8>>>();
/// ```
/// ```compile_fail
/// use aw_agent_core::publication::ReaderFactory;
/// fn sync<T: Sync>() {}
/// sync::<ReaderFactory<std::rc::Rc<u8>>>();
/// ```
pub struct ReaderFactory<T> {
    factory: ReadHandleFactory<Slot<T>>,
    available: Arc<AtomicBool>,
    value: PhantomData<Arc<Version<T>>>,
}
impl<T> Clone for ReaderFactory<T> {
    fn clone(&self) -> Self {
        Self {
            factory: self.factory.clone(),
            available: self.available.clone(),
            value: PhantomData,
        }
    }
}

/// An explicitly owned consumer: Send when T is Send + Sync, deliberately !Sync.
pub struct Reader<T> {
    handle: ReadHandle<Slot<T>>,
    available: Arc<AtomicBool>,
}

pub fn channel<T: Send + Sync>(initial: T) -> (Publisher<T>, ReaderFactory<T>) {
    channel_at(initial, 0)
}
fn channel_at<T: Send + Sync>(initial: T, generation: u64) -> (Publisher<T>, ReaderFactory<T>) {
    let current = Arc::new(Version {
        generation,
        value: initial,
    });
    let (handle, reader) = left_right::new_from_empty(Slot(current.clone()));
    let available = Arc::new(AtomicBool::new(true));
    let factory = ReaderFactory {
        factory: reader.factory(),
        available: available.clone(),
        value: PhantomData,
    };
    (
        Publisher {
            writer: Mutex::new(Writer { current, handle }),
            available,
        },
        factory,
    )
}

fn check(available: &AtomicBool) -> Result<()> {
    if available.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err(Error::Storage("published state unavailable"))
    }
}

/// A writer panic must invalidate old published authority, even if its poisoned
/// writer mutex and old left-right copies remain allocated by another owner.
struct PanicFence<'a>(&'a AtomicBool);
impl Drop for PanicFence<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.store(false, Ordering::Release);
        }
    }
}
impl<T> Publisher<T> {
    /// Canonical owner lookup, for admission/mutation rather than display reads.
    pub fn current(&self) -> Result<Arc<Version<T>>> {
        check(&self.available)?;
        let writer = self.writer.lock().map_err(|_| {
            self.available.store(false, Ordering::Release);
            Error::Storage("publication writer poisoned")
        })?;
        check(&self.available)?;
        Ok(writer.current.clone())
    }

    /// Build a complete successor exactly once. An ordinary error leaves the
    /// current version intact. Do not reenter this publisher from the mutator.
    /// Publication is synchronous; read guards only clone an Arc or inspect a bounded authorization verdict.
    /// At most one replacement is unpublished, with no deferred work queue.
    pub fn update<R>(&self, update: impl FnOnce(&T) -> Result<(T, R)>) -> Result<(u64, R)> {
        check(&self.available)?;
        let mut writer = self.writer.lock().map_err(|_| {
            self.available.store(false, Ordering::Release);
            Error::Storage("publication writer poisoned")
        })?;
        check(&self.available)?;
        let _panic = PanicFence(&self.available);
        let generation = writer
            .current
            .generation
            .checked_add(1)
            .ok_or(Error::Limit("publication generation exhausted"))?;
        let (value, result) = update(&writer.current.value)?;
        let next = Arc::new(Version { generation, value });
        writer.handle.append(next.clone()).publish();
        writer.current = next;
        Ok((generation, result))
    }
}
impl<T> Drop for Publisher<T> {
    fn drop(&mut self) {
        // Mark closed before left-right waits for any remaining internal guard.
        self.available.store(false, Ordering::Release);
    }
}
impl<T> ReaderFactory<T> {
    pub fn reader(&self) -> Result<Reader<T>> {
        check(&self.available)?;
        let reader = Reader {
            handle: self.factory.handle(),
            available: self.available.clone(),
        };
        check(&self.available)?;
        Ok(reader)
    }
}
impl<T> Reader<T> {
    // Only bounded crate-owned field inspection (account authorization verdicts)
    // may use this. Never call a provider/user callback or retain a borrowed value.
    pub(crate) fn inspect<R>(&self, inspect: impl FnOnce(&T) -> R) -> Result<R> {
        check(&self.available)?;
        let result = {
            let guard = self
                .handle
                .enter()
                .ok_or(Error::Storage("publication writer closed"))?;
            inspect(&guard.0.value)
        };
        check(&self.available)?;
        Ok(result)
    }
    pub fn snapshot(&self) -> Result<Arc<Version<T>>> {
        check(&self.available)?;
        let snapshot = {
            let guard = self
                .handle
                .enter()
                .ok_or(Error::Storage("publication writer closed"))?;
            guard.0.clone()
        };
        check(&self.available)?;
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{mpsc, Barrier},
        time::Duration,
    };

    #[test]
    fn versions_are_complete_owned_and_mutation_runs_once() {
        let (publisher, factory) = channel(vec![0]);
        let reader = factory.reader().unwrap();
        let original = reader.snapshot().unwrap();
        let mut calls = 0;
        for expected in 1..40 {
            let (generation, ()) = publisher
                .update(|old| {
                    calls += 1;
                    let mut next = old.clone();
                    next.push(expected);
                    Ok((next, ()))
                })
                .unwrap();
            let current = reader.snapshot().unwrap();
            assert_eq!(generation, expected);
            assert_eq!(current.generation, expected);
            assert_eq!(current.value, (0..=expected).collect::<Vec<_>>());
            assert!(!publisher
                .writer
                .lock()
                .unwrap()
                .handle
                .has_pending_operations());
        }
        assert_eq!(calls, 39);
        assert_eq!(original.generation, 0);
        assert_eq!(original.value, vec![0]);
    }

    #[test]
    fn mutation_error_and_generation_overflow_do_not_publish() {
        let (publisher, factory) = channel(7);
        let reader = factory.reader().unwrap();
        assert!(publisher
            .update::<()>(|_| Err(Error::Denied("fixture")))
            .is_err());
        assert_eq!(reader.snapshot().unwrap().generation, 0);
        assert_eq!(reader.snapshot().unwrap().value, 7);
        publisher.update(|_| Ok((8, ()))).unwrap();
        let (exhausted, factory) = channel_at(9, u64::MAX);
        let mut called = false;
        assert!(exhausted
            .update(|_| {
                called = true;
                Ok((10, ()))
            })
            .is_err());
        assert!(!called);
        assert_eq!(factory.reader().unwrap().snapshot().unwrap().value, 9);
    }

    #[test]
    fn old_guard_delays_reuse_without_false_visibility_acknowledgment() {
        let (publisher, factory) = channel(0);
        let publisher = Arc::new(publisher);
        let reader = factory.reader().unwrap();
        let observer = factory.reader().unwrap();
        let old = reader.handle.enter().unwrap();
        publisher.update(|_| Ok((1, ()))).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let (done, completion) = mpsc::channel();
        let worker = publisher.clone();
        let entered = barrier.clone();
        let task = std::thread::spawn(move || {
            entered.wait();
            done.send(worker.update(|_| Ok((2, ())))).unwrap();
        });
        barrier.wait();
        assert!(completion.recv_timeout(Duration::from_millis(50)).is_err());
        assert_eq!(observer.snapshot().unwrap().value, 1);
        drop(old);
        completion
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        task.join().unwrap();
        assert_eq!(reader.snapshot().unwrap().value, 2);
    }

    #[test]
    fn panic_and_shutdown_fail_closed_in_existing_readers() {
        let (publisher, factory) = channel(1);
        let reader = factory.reader().unwrap();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = publisher.update::<()>(|_| panic!("fixture mutation"));
        }));
        assert!(panic.is_err());
        assert!(reader.snapshot().is_err());
        assert!(factory.reader().is_err());
        assert!(publisher.current().is_err());
        let (publisher, factory) = channel(2);
        let reader = factory.reader().unwrap();
        let owned = reader.snapshot().unwrap();
        drop(publisher);
        assert!(reader.snapshot().is_err());
        assert!(factory.reader().is_err());
        assert_eq!(owned.value, 2);
    }

    #[test]
    fn shared_owners_are_send_sync_and_readers_are_send() {
        fn shared<T: Send + Sync>() {}
        fn movable<T: Send>() {}
        shared::<Publisher<String>>();
        shared::<ReaderFactory<String>>();
        movable::<Reader<String>>();
    }

    #[test]
    fn repeated_publication_has_bounded_retention_and_no_pending_operations() {
        use std::sync::atomic::AtomicUsize;
        struct Counted(Arc<AtomicUsize>);
        impl Counted {
            fn new(live: &Arc<AtomicUsize>) -> Self {
                live.fetch_add(1, Ordering::SeqCst);
                Self(live.clone())
            }
        }
        impl Drop for Counted {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let live = Arc::new(AtomicUsize::new(0));
        let (publisher, factory) = channel(Counted::new(&live));
        let reader = factory.reader().unwrap();
        let retained = reader.snapshot().unwrap();
        let mut versions = vec![Arc::downgrade(&retained)];
        for _ in 0..1_000 {
            publisher.update(|_| Ok((Counted::new(&live), ()))).unwrap();
            versions.push(Arc::downgrade(&reader.snapshot().unwrap()));
            assert!(live.load(Ordering::SeqCst) <= 3);
            assert!(
                versions
                    .iter()
                    .filter(|version| version.strong_count() != 0)
                    .count()
                    <= 3
            );
            assert!(!publisher
                .writer
                .lock()
                .unwrap()
                .handle
                .has_pending_operations());
        }
        drop(publisher);
        assert_eq!(live.load(Ordering::SeqCst), 1);
        assert!(reader.snapshot().is_err());
        drop(retained);
        assert_eq!(live.load(Ordering::SeqCst), 0);
        assert!(versions.iter().all(|version| version.strong_count() == 0));
    }
}
