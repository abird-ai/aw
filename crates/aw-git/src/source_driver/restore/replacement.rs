//! Replacement composes the original release recipe and one new restore.
//! Neither a retained descriptor nor an observed checkout is authority.
use super::{
    absent, check_destination, overlaps, require_marker_absent, require_released, DerivePlan,
    GitRestoreLocation, GitSourceAnchor, GitSourceDriver, PrivateIndexes, RestorePlan,
};
use crate::Repository;
use aw_agent_core::{Error, PreparedSourceOperation, Result, SourceBinding, SourceOperationKind};

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

    /// Continue an explicitly recovered replacement only after the new
    /// candidate and its registration are provably absent. Finish the
    /// original release recipe (including any private capture retention)
    /// before executing the same incoming descriptor. No partial checkout
    /// or uncertain registration is retried.
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
        // Require that the old pathname is already absent. An intact old
        // checkout is not a post-release gap; it can instead be abandoned.
        absent(&plan.target)?;
        self.require_restore_absent(incoming)?;
        self.release_restored_materialization(old, old_binding)?;
        self.require_restore_absent(incoming)?;
        self.execute_restore(incoming)
    }
}
