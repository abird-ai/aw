//! Replacement composes the original release recipe and one new restore.
//! Neither a retained descriptor nor an observed checkout is authority.
use super::{
    check_destination, inspect_binding, overlaps, require_marker_absent, require_released,
    restored_repo, DerivePlan, GitRestoreLocation, GitSourceAnchor, GitSourceDriver,
    PrivateIndexes, RestorePlan,
};
use crate::Repository;
use aw_agent_core::{Error, PreparedSourceOperation, Result, SourceBinding, SourceOperationKind};
use std::fs;

impl GitSourceDriver {
    fn releasable_original(
        &self,
        prepared: &PreparedSourceOperation,
        binding: &SourceBinding,
    ) -> Result<Repository> {
        if prepared.operation == SourceOperationKind::Derive {
            let plan = DerivePlan::decode(prepared)?;
            plan.require_binding(binding)?;
            let (origin, anchor) = plan.retained(self)?;
            let anchor = anchor.ok_or(Error::Conflict("original derived retention unavailable"))?;
            return self.releasable_checkout(&origin, &plan.restore(anchor), prepared, binding);
        }
        let plan = RestorePlan::decode(prepared)?;
        let origin = plan.origin_repository(self)?;
        self.releasable_checkout(&origin, &plan, prepared, binding)
    }

    fn require_independent_replacement(
        old: &PreparedSourceOperation,
        anchor: &GitSourceAnchor,
    ) -> Result<()> {
        if old.operation == SourceOperationKind::Derive {
            let plan = DerivePlan::decode(old)?;
            // Releasing the old child removes its own retention. An
            // incoming version must not depend on that private ref.
            if plan.capture.binding.source_ref == anchor.source_ref
                && plan.capture.binding.name == anchor.name
                && plan.capture.checkpoint_id == anchor.checkpoint_id
            {
                return Err(Error::Conflict(
                    "replacement depends on the retiring child's retention",
                ));
            }
        }
        Ok(())
    }

    /// Read-only planning at an occupied named destination, allowed only
    /// for the exact independently approved, unchanged original checkout.
    /// Persist the new Restore descriptor together with original binding
    /// custody before releasing the old materialization.
    pub fn plan_restore_replacing_approved(
        &self,
        location: &GitRestoreLocation,
        anchor: &GitSourceAnchor,
        prepare_id: &str,
        old_location: &GitRestoreLocation,
        old: &PreparedSourceOperation,
        old_binding: &SourceBinding,
    ) -> Result<PreparedSourceOperation> {
        if old_location.name != location.name
            || old_location.target != location.target
            || old_location.parent_identity != location.parent_identity
            || overlaps(&location.target, &location.repository)
            || old.descriptor.prepare_id == prepare_id
        {
            return Err(Error::Denied(
                "replacement requires the exact original destination and a new identity",
            ));
        }
        self.verify_restored_materialization_location(old_location, old, old_binding)?;
        Self::require_independent_replacement(old, anchor)?;
        let child = self.releasable_original(old, old_binding)?;
        let origin = self.approved_restore_origin(location, anchor)?;
        check_destination(&origin, &location.target, child.common == origin.common)?;
        let prepared = Self::prepare_restore_at(&origin, anchor, &location.target, prepare_id)?;
        require_marker_absent(&origin, &prepared)?;
        Ok(prepared)
    }

    /// Before releasing the original, or rejecting an untouched replacement,
    /// prove its exact intact checkout and absence of the new registration.
    /// Caller has revalidated both independently approved locations and
    /// excludes all unaccounted writers/aliases throughout settlement.
    pub fn verify_replacement_untouched(
        &self,
        old: &PreparedSourceOperation,
        old_binding: &SourceBinding,
        incoming: &PreparedSourceOperation,
    ) -> Result<()> {
        let plan = RestorePlan::decode(incoming)?;
        Self::require_independent_replacement(old, &plan.anchor)?;
        if old.descriptor.prepare_id == incoming.descriptor.prepare_id {
            return Err(Error::Conflict(
                "replacement reuses original preparation identity",
            ));
        }
        let child = self.releasable_original(old, old_binding)?;
        let origin = plan.origin_repository(self)?;
        if child.root() != plan.target
            || old_binding.name != plan.anchor.name
            || overlaps(&plan.target, origin.root())
        {
            return Err(Error::Conflict(
                "replacement destination differs from original",
            ));
        }
        check_destination(&origin, &plan.target, child.common == origin.common)?;
        require_marker_absent(&origin, incoming)
    }

    /// Positive absence of all original incoming effects, not merely a
    /// missing pathname. A retained version and the independent live
    /// namespace fence are still needed before explicit continuation.
    pub fn require_restore_absent(&self, prepared: &PreparedSourceOperation) -> Result<()> {
        let plan = RestorePlan::decode(prepared)?;
        let origin = plan.repository(self)?;
        require_released(&origin, &plan, prepared)
    }

    /// Read-only proof of the replacement's final physical state.
    /// The new exact checkout may now occupy the old pathname; prove the
    /// predecessor registration and any derived private retention are gone
    /// without trying to release the new checkout.
    pub fn revalidate_replacement(
        &self,
        old: &PreparedSourceOperation,
        old_binding: &SourceBinding,
        incoming: &PreparedSourceOperation,
        binding: &SourceBinding,
    ) -> Result<()> {
        let new_plan = RestorePlan::decode(incoming)?;
        Self::require_independent_replacement(old, &new_plan.anchor)?;
        if old.descriptor.prepare_id == incoming.descriptor.prepare_id
            || old_binding.name != binding.name
        {
            return Err(Error::Conflict(
                "replacement identity differs from original",
            ));
        }
        self.revalidate_restore(incoming, binding)?;
        let new_origin = new_plan.origin_repository(self)?;
        self.require_predecessor_released(old, old_binding, &new_plan, &new_origin)
    }

    // The caller has proved the incoming registration at this exact
    // destination. Absence of the old marker alone is not sufficient.
    fn require_predecessor_released(
        &self,
        old: &PreparedSourceOperation,
        old_binding: &SourceBinding,
        new_plan: &RestorePlan,
        new_origin: &Repository,
    ) -> Result<()> {
        let (origin, target) = if old.operation == SourceOperationKind::Derive {
            let plan = DerivePlan::decode(old)?;
            plan.require_binding(old_binding)?;
            let (origin, retained) = plan.retained(self)?;
            if retained.is_some() {
                return Err(Error::Conflict("replaced Git capture retention remains"));
            }
            PrivateIndexes::require_absent(
                &origin,
                &plan.capture_descriptor()?.descriptor.prepared_identity,
            )?;
            (origin, plan.target)
        } else {
            let plan = RestorePlan::decode(old)?;
            plan.require_binding(old_binding)?;
            (plan.origin_repository(self)?, plan.target)
        };
        if target != new_plan.target {
            return Err(Error::Conflict("replacement changed the named destination"));
        }
        check_destination(&origin, &target, origin.common == new_origin.common)?;
        require_marker_absent(&origin, old)
    }

    /// Explicitly continue the original unpublished replacement under
    /// independently renewed whole-set authority. An exact completed
    /// incoming checkout is retained. A partial one may be reconciled
    /// only through the existing private restore cleanup after proving
    /// the predecessor released. Execute again only after positive
    /// absence of the candidate and its original registration.
    pub fn continue_replacement(
        &self,
        old: &PreparedSourceOperation,
        old_binding: &SourceBinding,
        incoming: &PreparedSourceOperation,
    ) -> Result<SourceBinding> {
        let plan = RestorePlan::decode(incoming)?;
        Self::require_independent_replacement(old, &plan.anchor)?;
        let (_, old_target) = super::materialization_parts(old_binding)?;
        if old_target != plan.target
            || old_binding.name != plan.anchor.name
            || old.descriptor.prepare_id == incoming.descriptor.prepare_id
        {
            return Err(Error::Conflict(
                "replacement continuation differs from original",
            ));
        }
        let origin = plan.repository(self)?;
        match fs::symlink_metadata(&plan.target) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // Original post-release gap. Finish any remaining exact
                // predecessor cleanup before the incoming restore.
                require_released(&origin, &plan, incoming)?;
                self.release_restored_materialization(old, old_binding)?;
            }
            Err(error) => return Err(error.into()),
            Ok(_) => {
                // An intact old checkout, foreign path or ambiguous
                // registration fails here, before any cleanup.
                let child = restored_repo(&origin, &plan, incoming)?;
                self.require_predecessor_released(old, old_binding, &plan, &origin)?;
                let trees = plan.trees(&origin)?;
                if let Ok(binding) = inspect_binding(&origin, &child, &plan, incoming, &trees) {
                    return Ok(binding);
                }
                // Reuse exact unpublished cleanup; modified/truncated
                // leaves, foreign entries and ambiguous scratch refuse.
                self.release_prepared_plan(&plan, incoming)?;
            }
        }
        require_released(&origin, &plan, incoming)?;
        self.execute_restore(incoming)
    }
}
