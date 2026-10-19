//! Ephemeral local/native PTY presentation control, never an Agent or tool
//! authority. Exactly one frontend may submit input/size/termination per PTY.
//! A read-only snapshot is always available from the original retained owner.
//!
//! Do not serialize this registry into saved layouts, State or provider context.
use aw_terminal::Terminal;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(super) const INVITE_TTL: Duration = Duration::from_secs(90);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    Local,
    Offered,
    Docked,
    Settling,
    #[cfg(target_os = "linux")]
    Migrating,
}

struct Offer {
    digest: [u8; 32],
    until: Instant,
}

enum Owner {
    Local,
    Docked(u64),
    Settling,
    #[cfg(target_os = "linux")]
    Migrating,
    #[cfg(target_os = "linux")]
    MigrationUnknown,
}
struct Entry {
    epoch: u64,
    owner: Owner,
    offer: Option<Offer>,
}
impl Entry {
    fn mode(&self) -> Mode {
        match self.owner {
            Owner::Local if self.offer.is_some() => Mode::Offered,
            Owner::Local => Mode::Local,
            Owner::Docked(_) => Mode::Docked,
            Owner::Settling => Mode::Settling,
            #[cfg(target_os = "linux")]
            Owner::Migrating | Owner::MigrationUnknown => Mode::Migrating,
        }
    }
}
#[derive(Default)]
struct Registry {
    entries: BTreeMap<String, Entry>,
    closed: bool,
}

/// Secret tokens are given to a human-approved destination only. The pool
/// stores SHA-256 digests, not raw secrets or socket addresses.
pub struct Invitation {
    pub ticket: String,
    pub terminal_id: String,
}

/// Control ownership is tied to this non-cloneable RAII guard, not tab IDs,
/// socket addresses or a browser/CLI label. Drop reverts to the original owner
/// after queued remote input has drained.
pub struct Lease {
    registry: Arc<Mutex<Registry>>,
    id: String,
    generation: u64,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// Fences docking/local control across the full source custody transaction.
#[cfg(target_os = "linux")]
pub struct Migration {
    registry: Arc<Mutex<Registry>>,
    terminal: Arc<Terminal>,
}
#[cfg(target_os = "linux")]
impl Drop for Migration {
    fn drop(&mut self) {
        let mut state = lock(&self.registry);
        if let Some(entry) = state.entries.get_mut(&self.terminal.id) {
            if matches!(entry.owner, Owner::Migrating) {
                entry.owner = if self.terminal.source_release() == aw_terminal::SourceRelease::Retained {
                    Owner::Local
                } else { Owner::MigrationUnknown };
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct Control {
    registry: Arc<Mutex<Registry>>,
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
fn digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}
fn secure_equal(expected: &[u8; 32], actual: &[u8; 32]) -> bool {
    expected.iter().zip(actual).fold(0_u8, |acc, (a, b)| acc | a ^ b) == 0
}
fn denied(message: &'static str) -> aw::Error {
    aw::Error::Denied(message)
}

impl Control {
    #[cfg(target_os = "linux")]
    pub fn begin_migration(&self, terminal: Arc<Terminal>) -> aw::Result<Migration> {
        let mut state = lock(&self.registry);
        if state.closed { return Err(denied("terminal pool is shutting down")); }
        let entry = state.entries.get_mut(&terminal.id).ok_or(denied("unknown retained PTY"))?;
        Self::settle(entry, &terminal);
        if !matches!(entry.owner, Owner::Local) || entry.offer.is_some()
            || terminal.status() != aw_terminal::Status::Running {
            return Err(denied("migration requires exclusive local control and no invitation"));
        }
        entry.owner = Owner::Migrating;
        Ok(Migration { registry: self.registry.clone(), terminal })
    }
    pub fn register(&self, id: &str) -> aw::Result<()> {
        let mut state = lock(&self.registry);
        if state.closed {
            return Err(denied("terminal pool is shutting down"));
        }
        if state.entries.contains_key(id) {
            return Err(denied("terminal identity already registered"));
        }
        state.entries.insert(id.into(), Entry {
            epoch: 0, owner: Owner::Local, offer: None,
        });
        Ok(())
    }
    pub fn forget(&self, id: &str) {
        lock(&self.registry).entries.remove(id);
    }
    pub fn is_shutdown(&self) -> bool { lock(&self.registry).closed }
    pub fn shutdown(&self) {
        let mut state = lock(&self.registry);
        state.closed = true;
        for entry in state.entries.values_mut() {
            entry.offer = None;
        }
    }
    fn settle(entry: &mut Entry, terminal: &Terminal) {
        #[cfg(target_os = "linux")]
        if matches!(entry.owner, Owner::MigrationUnknown)
            && terminal.source_release() == aw_terminal::SourceRelease::Retained {
            // Only a definite source-worker rejection can restore custody.
            entry.owner = Owner::Local;
        }
        if matches!(entry.owner, Owner::Settling) && terminal.snapshot(0).pending_input == 0 {
            entry.owner = Owner::Local;
        }
    }
    pub fn mode(&self, id: &str, terminal: &Terminal) -> Option<Mode> {
        let mut state = lock(&self.registry);
        let entry = state.entries.get_mut(id)?;
        Self::settle(entry, terminal);
        if entry.offer.as_ref().is_some_and(|offer| offer.until <= Instant::now()) {
            entry.offer = None;
        }
        Some(entry.mode())
    }
    pub fn invite(&self, id: &str, terminal: &Terminal) -> aw::Result<Invitation> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes)
            .map_err(|_| aw::Error::Config("secure terminal invitation unavailable".into()))?;
        let ticket = bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        let mut state = lock(&self.registry);
        if state.closed {
            return Err(denied("terminal pool is shutting down"));
        }
        let entry = state.entries.get_mut(id).ok_or(denied("unknown live terminal"))?;
        Self::settle(entry, terminal);
        if !matches!(entry.owner, Owner::Local)
            || terminal.status() != aw_terminal::Status::Running {
            return Err(denied("terminal controller unavailable for docking"));
        }
        if entry.offer.as_ref().is_some_and(|offer| offer.until > Instant::now()) {
            return Err(denied("terminal invitation already pending; cancel it before reissuing"));
        }
        entry.offer = Some(Offer { digest: digest(&ticket), until: Instant::now() + INVITE_TTL });
        Ok(Invitation { ticket, terminal_id: id.into() })
    }
    pub fn has_offer(&self, id: &str, token: &str) -> bool {
        let state = lock(&self.registry);
        state.entries.get(id).is_some_and(|entry| {
            matches!(entry.owner, Owner::Local)
                && entry.offer.as_ref().is_some_and(|offer|
                    Instant::now() < offer.until
                    && secure_equal(&offer.digest, &digest(token)))
        })
    }
    #[cfg(test)]
    pub(super) fn expire_offer_for_test(&self, id: &str) {
        let mut state = lock(&self.registry);
        state.entries.get_mut(id).unwrap().offer.as_mut().unwrap().until =
            Instant::now() - Duration::from_millis(1);
    }
    pub fn cancel(&self, id: &str) -> aw::Result<()> {
        let mut state = lock(&self.registry);
        let entry = state.entries.get_mut(id).ok_or(denied("unknown live terminal"))?;
        if !matches!(entry.owner, Owner::Local) {
            return Err(denied("terminal controlled by another presentation"));
        }
        entry.offer = None;
        Ok(())
    }
    pub fn accept(&self, id: &str, token: &str, terminal: &Terminal) -> aw::Result<Lease> {
        let mut state = lock(&self.registry);
        if state.closed {
            return Err(denied("terminal pool is shutting down"));
        }
        let entry = state.entries.get_mut(id).ok_or(denied("unknown live terminal"))?;
        Self::settle(entry, terminal);
        if !matches!(entry.owner, Owner::Local) || terminal.status() != aw_terminal::Status::Running {
            return Err(denied("terminal is not available to this presentation"));
        }
        let offer = entry.offer.as_ref().ok_or(denied("terminal invitation not available"))?;
        if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit())
            || Instant::now() >= offer.until || !secure_equal(&offer.digest, &digest(token)) {
            return Err(denied("terminal invitation invalid or expired"));
        }
        // No previous input is allowed to cross the control boundary as
        // "newly authorized" input in the destination.
        if terminal.snapshot(0).pending_input != 0 {
            return Err(denied("terminal input is still settling; retry explicitly"));
        }
        entry.epoch = entry.epoch.checked_add(1)
            .ok_or(denied("terminal controller generation exhausted"))?;
        let generation = entry.epoch;
        entry.owner = Owner::Docked(generation);
        entry.offer = None;
        Ok(Lease { registry: self.registry.clone(), id: id.into(), generation, wake: None })
    }
    pub fn local<T>(&self, id: &str, terminal: &Terminal, use_terminal: impl FnOnce() -> aw_terminal::Result<T>) -> aw::Result<T> {
        let mut state = lock(&self.registry);
        if state.closed {
            return Err(denied("terminal pool is shutting down"));
        }
        let entry = state.entries.get_mut(id).ok_or(denied("unknown live terminal"))?;
        Self::settle(entry, terminal);
        if !matches!(entry.owner, Owner::Local) {
            return Err(denied("terminal is controlled by another native window"));
        }
        use_terminal().map_err(|error| aw::Error::Config(error.to_string()))
    }
    pub fn docked<T>(&self, session: &Lease, use_terminal: impl FnOnce() -> aw_terminal::Result<T>) -> aw::Result<T> {
        let state = lock(&self.registry);
        if state.closed {
            return Err(denied("terminal pool is shutting down"));
        }
        if !Arc::ptr_eq(&self.registry, &session.registry) {
            return Err(denied("terminal controller belongs to a different pool"));
        }
        let entry = state.entries.get(&session.id).ok_or(denied("terminal no longer available"))?;
        if !matches!(entry.owner, Owner::Docked(generation) if generation == session.generation) {
            return Err(denied("stale terminal controller generation"));
        }
        use_terminal().map_err(|error| aw::Error::Config(error.to_string()))
    }
}
impl Lease {
    pub fn terminal_id(&self) -> &str { &self.id }
    pub fn on_drop(&mut self, wake: impl Fn() + Send + Sync + 'static) {
        self.wake = Some(Arc::new(wake));
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        {
            let mut state = lock(&self.registry);
            if let Some(entry) = state.entries.get_mut(&self.id) {
                if matches!(entry.owner, Owner::Docked(generation) if generation == self.generation) {
                    entry.owner = Owner::Settling;
                    entry.offer = None;
                }
            }
        }
        // Notify only after unlocking owner state. Drawing the next frame
        // may immediately inspect the same terminal controller.
        if let Some(wake) = &self.wake { wake(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[test]
    fn migration_fence_excludes_every_controller_and_rolls_back_only_retained_source() {
        let shell = std::env::var_os("AW_BASH").map(std::path::PathBuf::from).unwrap_or_else(|| "/bin/bash".into());
        let terminal = Arc::new(Terminal::spawn(aw_terminal::Options::bash(std::env::temp_dir(), shell, "synthetic-controller"), || {}).unwrap());
        let control = Control::default();
        control.register(&terminal.id).unwrap();
        let fence = control.begin_migration(terminal.clone()).unwrap();
        assert_eq!(control.mode(&terminal.id, &terminal), Some(Mode::Migrating));
        assert!(control.local(&terminal.id, &terminal, || terminal.input(b"forbidden".to_vec())).is_err());
        assert!(control.invite(&terminal.id, &terminal).is_err());
        assert!(control.begin_migration(terminal.clone()).is_err());
        drop(fence);
        assert_eq!(control.mode(&terminal.id, &terminal), Some(Mode::Local));
        // A lost caller may leave a fenced registry entry. Only a factual
        // Retained worker result, not elapsed time or source death, settles it.
        lock(&control.registry).entries.get_mut(&terminal.id).unwrap().owner = Owner::MigrationUnknown;
        assert_eq!(control.mode(&terminal.id, &terminal), Some(Mode::Local));
        let invitation = control.invite(&terminal.id, &terminal).unwrap();
        assert!(control.begin_migration(terminal.clone()).is_err());
        control.cancel(&terminal.id).unwrap();
        assert!(!control.has_offer(&terminal.id, &invitation.ticket));
        terminal.terminate();
    }
    #[test]
    fn digest_comparison_and_token_bounds() {
        let known = digest("a".repeat(64).as_str());
        assert!(secure_equal(&known, &known));
        assert!(!secure_equal(&known, &digest("b".repeat(64).as_str())));
        assert_eq!(INVITE_TTL, Duration::from_secs(90));
    }
}
