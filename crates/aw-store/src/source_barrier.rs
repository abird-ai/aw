//! Store-local source mutation exclusion, using the existing coordination
//! owner. No source grant, provider authority or external-effect replay.
use crate::{
    coordination::{self, ProcessIdentity, SOURCE_MUTATION_SCOPE_PREFIX},
    db,
    store_scope::StoreScope,
    Error, LocalCoordinationLease, Result, SessionStore, StoreMode,
};
use aw_agent_core::{validate_source_set_hard_limit, DriverKey, SourceAnchor, SourceBinding};
use rusqlite::{Transaction, TransactionBehavior};
use sha2::{Digest, Sha256};

/// Canonical, deduplicated mutation scopes of a complete selected SourceSet.
/// Names, checkout paths, access maxima and artifact schema versions do not
/// partition a driver's stable source identity into separate writer gates.
/// Scope is local to this Store; cross-Store/provider exclusion is separate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceMutationSet {
    count: usize,
    scopes: Vec<String>,
}

impl SourceMutationSet {
    pub fn from_bindings(bindings: &[SourceBinding]) -> Result<Self> {
        Self::from_binding_refs(bindings.iter())
    }

    pub(crate) fn from_binding_refs<'a>(
        bindings: impl ExactSizeIterator<Item = &'a SourceBinding> + Clone,
    ) -> Result<Self> {
        validate_source_set_hard_limit(bindings.len())?;
        for binding in bindings.clone() {
            binding.validate()?;
        }
        Self::from_identities(
            bindings.len(),
            bindings.map(|b| (&b.driver, b.source_ref.as_str())),
        )
    }

    pub fn from_anchors(anchors: &[SourceAnchor]) -> Result<Self> {
        validate_source_set_hard_limit(anchors.len())?;
        for anchor in anchors {
            anchor.validate()?;
        }
        Self::from_identities(
            anchors.len(),
            anchors.iter().map(|a| (&a.driver, a.source_ref.as_str())),
        )
    }

    pub(crate) fn with_incoming_anchor<'a>(
        bindings: impl ExactSizeIterator<Item = &'a SourceBinding> + Clone,
        anchor: &'a SourceAnchor,
    ) -> Result<Self> {
        // A replacement consumes one named slot, but fences both the
        // old and incoming source identities until the atomic change.
        let count = bindings.len() + usize::from(!bindings.clone().any(|b| b.name == anchor.name));
        validate_source_set_hard_limit(count)?;
        anchor.validate()?;
        for binding in bindings.clone() {
            binding.validate()?;
        }
        Self::from_identities(
            count,
            bindings
                .map(|b| (&b.driver, b.source_ref.as_str()))
                .chain(std::iter::once((
                    &anchor.driver,
                    anchor.source_ref.as_str(),
                ))),
        )
    }

    fn from_identities<'a>(
        count: usize,
        identities: impl Iterator<Item = (&'a DriverKey, &'a str)>,
    ) -> Result<Self> {
        let mut scopes = Vec::with_capacity(count);
        for (driver, source) in identities {
            let driver = crate::json(driver)?;
            let mut hash = Sha256::new();
            hash.update(b"aw-source-mutation-scope-v1\0");
            for part in [driver.as_bytes(), source.as_bytes()] {
                hash.update((part.len() as u64).to_be_bytes());
                hash.update(part);
            }
            scopes.push(format!(
                "{SOURCE_MUTATION_SCOPE_PREFIX}{:x}",
                hash.finalize()
            ));
        }
        scopes.sort_unstable();
        scopes.dedup();
        Ok(Self { count, scopes })
    }

    pub(crate) fn require_unleased_in(&self, tx: &Transaction<'_>) -> Result<()> {
        if self.scopes.is_empty() {
            return Ok(());
        }
        coordination::require_unleased_sources_in(tx, &self.scopes)
    }
}

/// Outcome of a *trusted* owner's source-operation settlement. Unknown is
/// not a release: every participating scope remains blocked pending recovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceMutationOutcome {
    Settled,
    Unknown,
}

/// Non-cloneable, owned source barrier. Retains the original Store owner
/// across worker handoff, even when the submitting task is dropped. Acquisition and settlement
/// are complete-set transactions. Dropping without settlement deliberately
/// keeps the durable claims; owner restart turns them into hazards. Never
/// release unknown effects merely because the Rust token went out of scope.
#[must_use = "settle the source barrier explicitly; abandoned claims remain fenced"]
pub struct SourceMutationBarrier {
    store: SessionStore,
    store_scope: StoreScope,
    process: ProcessIdentity,
    leases: Vec<LocalCoordinationLease>,
}

/// Read-only verification of the original complete lease set, transferable
/// to the launch lane. Cannot acquire, recover or settle those leases.
pub(crate) struct SourceMutationPin {
    store_scope: StoreScope,
    process: ProcessIdentity,
    leases: Vec<LocalCoordinationLease>,
}

impl SourceMutationPin {
    pub(crate) fn require_held_in(&self, tx: &Transaction<'_>) -> Result<()> {
        require_held_in(tx, &self.store_scope, &self.process, &self.leases)
    }
}

fn require_held_in(
    tx: &Transaction<'_>,
    scope: &StoreScope,
    process: &ProcessIdentity,
    leases: &[LocalCoordinationLease],
) -> Result<()> {
    if StoreScope::load(tx)? != *scope {
        return Err(Error::Conflict("source barrier Store scope changed"));
    }
    coordination::require_source_leases_in(tx, process, leases)
}

impl SourceMutationBarrier {
    pub(crate) fn store(&self) -> &SessionStore {
        &self.store
    }

    pub(crate) fn launch_pin(&self) -> SourceMutationPin {
        SourceMutationPin {
            store_scope: self.store_scope.clone(),
            process: self.process.clone(),
            leases: self.leases.clone(),
        }
    }

    /// The final owner transaction must still hold the exact whole set.
    pub(crate) fn require_held_in(&self, tx: &Transaction<'_>) -> Result<()> {
        require_held_in(tx, &self.store_scope, &self.process, &self.leases)
    }

    /// Exact operation identity and whole held set, for the shared constructor.
    pub(crate) fn require_preparation_in(
        &self,
        tx: &Transaction<'_>,
        prepare_id: &str,
    ) -> Result<()> {
        if self.leases.is_empty()
            || self
                .leases
                .iter()
                .any(|lease| lease.run_id.as_deref() != Some(prepare_id))
        {
            return Err(Error::Conflict(
                "source mutation barrier belongs to another preparation",
            ));
        }
        self.require_held_in(tx)
    }

    /// Settle in the same transaction as resource/receipt publication.
    /// Another writer cannot acquire the scopes until this commit succeeds.
    pub(crate) fn settle_in(
        &self,
        tx: &Transaction<'_>,
        outcome: SourceMutationOutcome,
    ) -> Result<()> {
        self.require_held_in(tx)?;
        coordination::settle_source_leases_in(
            tx,
            &self.process,
            &self.leases,
            outcome == SourceMutationOutcome::Unknown,
        )
    }

    /// For a host-owned operation with no pending AW publication transaction.
    /// Settled requires proof of no unresolved external effect. The eventual
    /// construction owner instead calls settle_in with its final transaction.
    pub fn settle(self, outcome: SourceMutationOutcome) -> Result<()> {
        let mut connection = self.store.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        self.settle_in(&tx, outcome)?;
        db(tx.commit())
    }
}

impl SessionStore {
    /// Trusted lower host coordination only. Caller must preflight exact
    /// source authority/compatibility and own the complete selected set;
    /// effectful construction must still durably claim Prepared before its
    /// first effect. This gate cannot fence non-participating external writers,
    /// other Stores, or provider authority; their live fences are separate.
    /// The source-bearing runtime remains closed until all its writer/effect
    /// admission paths participate in this same owner.
    pub fn acquire_source_mutation_barrier(
        &self,
        set: &SourceMutationSet,
        prepare_id: &str,
    ) -> Result<SourceMutationBarrier> {
        if prepare_id.starts_with(crate::sandbox_owner::JOURNALED_SOURCE_EFFECT_PREFIX)
            || prepare_id.starts_with(crate::checkpoint_owner::RETENTION_CLEANUP_PREFIX)
            || prepare_id.starts_with(crate::sandbox_owner::RETENTION_CLEANUP_PREFIX)
            || prepare_id.starts_with(crate::sandbox_owner::SOURCE_RELEASE_PREFIX)
        {
            return Err(Error::Denied(
                "reserved source claims require their original owner",
            ));
        }
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let barrier = self.acquire_source_mutation_barrier_in(&tx, set, prepare_id)?;
        db(tx.commit())?;
        Ok(barrier)
    }

    /// Claim in the caller's existing Prepared transaction. Neither the
    /// barrier nor the receipt becomes visible if that transaction rolls back.
    pub(crate) fn acquire_source_mutation_barrier_in(
        &self,
        tx: &Transaction<'_>,
        set: &SourceMutationSet,
        prepare_id: &str,
    ) -> Result<SourceMutationBarrier> {
        self.inner.source_limit.admit(set.count)?;
        self.claim_source_mutation_barrier_in(tx, set, prepare_id)
    }

    /// Existing materialization custody fixes the exact release recipe.
    /// The owner supplies its immutable retirement identity (or exact
    /// structural CAS); claim once or recover the original hazard, never
    /// steal a live claim. A lowered limit cannot strand a retained set.
    pub(crate) fn reconcile_retained_sources_in(
        &self,
        tx: &Transaction<'_>,
        set: &SourceMutationSet,
        prepare_id: &str,
    ) -> Result<SourceMutationBarrier> {
        let unresolved: bool = db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM local_coordination_leases
             WHERE scope_kind='execution' AND scope_key GLOB 'source-mutation:*'
               AND run_id=?1 AND state IN ('active','hazard'))",
            [prepare_id],
            |row| row.get(0),
        ))?;
        if unresolved {
            self.recover_source_mutation_barrier_in(tx, set, prepare_id)
        } else {
            self.claim_source_mutation_barrier_in(tx, set, prepare_id)
        }
    }

    fn claim_source_mutation_barrier_in(
        &self,
        tx: &Transaction<'_>,
        set: &SourceMutationSet,
        prepare_id: &str,
    ) -> Result<SourceMutationBarrier> {
        if set.scopes.is_empty() {
            return Err(Error::Config(
                "source mutation barrier requires a nonempty set".into(),
            ));
        }
        if prepare_id.is_empty()
            || prepare_id.len() > 128
            || !prepare_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-:".contains(&b))
        {
            return Err(Error::Config("invalid source preparation identity".into()));
        }
        if self.inner.mode != StoreMode::OwnerLock {
            return Err(Error::Denied(
                "source mutation barrier requires the exclusive Store owner",
            ));
        }
        let process = self.coordinated_process()?;
        let store_scope = StoreScope::load(tx)?;
        let leases = coordination::acquire_source_leases_in(tx, &process, &set.scopes, prepare_id)?;
        Ok(SourceMutationBarrier {
            store: self.clone(),
            store_scope,
            process,
            leases,
        })
    }

    /// Private receipt-owner recovery. The original named set may exceed a
    /// newly lowered admission limit; inspection creates no new source effect.
    pub(crate) fn recover_source_mutation_barrier_in(
        &self,
        tx: &Transaction<'_>,
        set: &SourceMutationSet,
        prepare_id: &str,
    ) -> Result<SourceMutationBarrier> {
        if self.inner.mode != StoreMode::OwnerLock {
            return Err(Error::Denied(
                "source recovery requires the exclusive Store owner",
            ));
        }
        let process = self.coordinated_process()?;
        let store_scope = StoreScope::load(tx)?;
        let leases = coordination::recover_source_leases_in(tx, &process, &set.scopes, prepare_id)?;
        Ok(SourceMutationBarrier {
            store: self.clone(),
            store_scope,
            process,
            leases,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::{
        CheckpointPolicy, PrincipalId, SchemaSupport, SourceAccess, SourceDriverRegistry,
        SourceDriverSchemas, SourceName, SourceSetLimit,
    };
    use aw_store_api::SessionBackend;
    use std::{path::PathBuf, time::Duration};

    struct PrivateTemp(PathBuf);
    impl Drop for PrivateTemp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn source_mutation_barrier_fences_complete_sets_and_preserves_hazards() {
        let temp = PrivateTemp(std::env::temp_dir().join(crate::id("aw-source-barrier").unwrap()));
        let actor = PrincipalId::new("internal:fixture:source-barrier").unwrap();
        let driver = DriverKey::builtin("fixture-git").unwrap();
        let schema = SchemaSupport::new(1, 1).unwrap();
        let registry = SourceDriverRegistry::new();
        registry
            .register(
                driver.clone(),
                SourceDriverSchemas {
                    binding: schema,
                    anchor: schema,
                    recovery: schema,
                },
                (),
            )
            .unwrap();
        let limit = SourceSetLimit::new(2).unwrap();
        let store = SessionStore::open_with_sources(&temp.0, registry.clone(), limit).unwrap();
        let layout = crate::SessionBinding {
            working_directory: PathBuf::from("/tmp"),
            tool_root: PathBuf::from("/tmp"),
            system_prompt: "system".into(),
            profile_hint: "fixture".into(),
            model_hint: "model".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (agent, branch) = store.create(&actor, "origin", layout.clone()).unwrap();
        let agent = store.agent_binding(&actor, &agent.id, &branch.id).unwrap();
        let project = format!(
            "/d:{}/i:{}/p:{}",
            agent.domain_handle, agent.identity_handle, agent.project_handle
        );
        let binding = |name: &str, source: &str, materialization: &str| SourceBinding {
            name: SourceName::new(name).unwrap(),
            driver: driver.clone(),
            binding_schema: 1,
            source_ref: source.into(),
            materialization: materialization.into(),
            access: SourceAccess::ReadWrite,
            checkpoint_policy: CheckpointPolicy::Required,
        };
        let a = binding("code", "repo-a", "/private/worktree-a");
        let b = binding("data", "repo-b", "/private/worktree-b");
        let alias = binding("other", "repo-a", "/private/other-checkout");
        let c = binding("independent", "repo-c", "/private/worktree-c");
        let set = SourceMutationSet::from_bindings(&[a.clone(), b.clone()]).unwrap();
        let aliased = SourceMutationSet::from_bindings(std::slice::from_ref(&alias)).unwrap();
        let independent = SourceMutationSet::from_bindings(std::slice::from_ref(&c)).unwrap();
        let anchor = SourceAnchor {
            name: alias.name.clone(),
            driver: driver.clone(),
            anchor_schema: 1,
            source_ref: alias.source_ref.clone(),
            access: SourceAccess::ReadOnly,
            checkpoint_policy: CheckpointPolicy::Required,
            identity: "version".into(),
            retention: "retention".into(),
            fields: Vec::new(),
        };
        assert_eq!(SourceMutationSet::from_anchors(&[anchor]).unwrap(), aliased);
        let repeated =
            SourceMutationSet::from_bindings(&[a.clone(), alias.clone(), b.clone()]).unwrap();
        assert_eq!(repeated.scopes, set.scopes);
        assert!(matches!(
            store.acquire_source_mutation_barrier(&repeated, "too-many"),
            Err(Error::Limit(_))
        ));

        let barrier = store
            .acquire_source_mutation_barrier(&set, "capture")
            .unwrap();
        assert!(store
            .acquire_source_mutation_barrier(&set, "capture")
            .is_err());
        assert!(store
            .acquire_source_mutation_barrier(&aliased, "alias")
            .is_err());
        let overlap = SourceMutationSet::from_bindings(&[c.clone(), alias.clone()]).unwrap();
        assert!(store
            .acquire_source_mutation_barrier(&overlap, "overlap")
            .is_err());
        // A failed overlapping set must not leave claims on its free members.
        store
            .acquire_source_mutation_barrier(&independent, "independent")
            .unwrap()
            .settle(SourceMutationOutcome::Settled)
            .unwrap();

        // The existing real Sandbox publication path shares this exclusion.
        assert!(store
            .sandbox_create_fresh_with_sources(
                &actor,
                &project,
                &layout,
                std::slice::from_ref(&alias)
            )
            .is_err());
        store
            .sandbox_create_fresh_with_sources(&actor, &project, &layout, std::slice::from_ref(&c))
            .unwrap();
        assert!(store
            .acquire_local_maintenance_lease("backup", Duration::from_secs(30))
            .is_err());

        // A generic raw lease cannot renew or settle a reserved source scope.
        let raw = &barrier.leases[0];
        assert_eq!(raw.expires_at, i64::MAX);
        assert!(store
            .acquire_local_execution_lease(
                &raw.scope_key,
                raw.run_id.as_deref(),
                Duration::from_secs(1)
            )
            .is_err());
        assert!(store
            .acquire_local_write_lease(&raw.scope_key, Duration::from_secs(1))
            .is_err());
        assert!(store.settle_local_lease(raw, false).is_err());

        // The final owner transaction can settle atomically with publication.
        // Rollback retains all claims, not just whichever scope was last.
        {
            let mut connection = store.connection().unwrap();
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            barrier
                .settle_in(&tx, SourceMutationOutcome::Settled)
                .unwrap();
            tx.rollback().unwrap();
        }
        assert!(store
            .acquire_source_mutation_barrier(&aliased, "after-rollback")
            .is_err());
        {
            let mut connection = store.connection().unwrap();
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            barrier.require_held_in(&tx).unwrap();
            barrier
                .settle_in(&tx, SourceMutationOutcome::Settled)
                .unwrap();
            tx.commit().unwrap();
        }
        assert!(barrier.settle(SourceMutationOutcome::Settled).is_err());
        let barrier = store
            .acquire_source_mutation_barrier(&set, "uncertain")
            .unwrap();
        let raw = barrier.leases[0].clone();
        barrier.settle(SourceMutationOutcome::Unknown).unwrap();
        assert!(store
            .resolve_local_execution_hazard(&raw.scope_key, raw.generation, "raw release")
            .is_err());
        assert!(store
            .acquire_source_mutation_barrier(&aliased, "after-unknown")
            .is_err());

        // An abandoned owner-lifetime claim survives a lost caller/owner.
        {
            let _abandoned = store
                .acquire_source_mutation_barrier(&independent, "abandoned")
                .unwrap();
        }
        drop(store);
        let reopened = SessionStore::open_with_sources(&temp.0, registry, limit).unwrap();
        assert!(reopened
            .acquire_source_mutation_barrier(&set, "restart")
            .is_err());
        assert!(reopened
            .acquire_source_mutation_barrier(&independent, "restart-independent")
            .is_err());
        assert!(reopened
            .sandbox_create_fresh_with_sources(&actor, &project, &layout, &[a])
            .is_err());
        let connection = reopened.connection().unwrap();
        let count: i64 = connection.query_row(
            "SELECT count(*) FROM local_coordination_leases
             WHERE scope_kind='execution' AND scope_key LIKE 'source-mutation:%' AND state='hazard'",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(count, 3);
    }
}
