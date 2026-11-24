//! Inspect the original prepared operation, including after owner restart.
//! Recovery only reclaims existing hazards; it does not execute, plan, or
//! publish external effects.

use super::*;
use aw_agent_core::{ExternalEvidence, RecoveryInspection, SourceAnchor, SourceSet};

/// Exact inspected anchors and evidence, under the original operation's
/// complete live Store-local barrier. Evidence identities are not provider
/// fences. Publication must independently revalidate authority, the exact
/// driver results and any required live provider fence through commit.
#[must_use = "retain the inspected capture until publication or recovery"]
pub struct InspectedCheckpointCapture<'a> {
    capture: PreparedCheckpointCapture<'a>,
    sources: SourceSet,
    evidence: Vec<ExternalEvidence>,
}

impl InspectedCheckpointCapture<'_> {
    pub fn checkpoint_id(&self) -> &str {
        self.capture.checkpoint_id()
    }

    pub fn source_set(&self) -> &SourceSet {
        &self.sources
    }

    /// Canonical source order, matching source_set().anchors.
    pub fn evidence(&self) -> &[ExternalEvidence] {
        &self.evidence
    }

    /// Store-side boundary check only, not publication authorization.
    pub fn revalidate(&self) -> Result<()> {
        self.capture.revalidate()
    }

    pub fn mark_unknown(self) -> Result<()> {
        self.capture.mark_unknown()
    }
}

fn require_inspection_authority_in(
    tx: &Transaction<'_>,
    actor: &PrincipalId,
    receipt: &ConstructionPreparation,
    registry: &SourceDriverRegistryReader,
) -> Result<()> {
    receipt.require_actor(tx, actor)?;
    let boundary = receipt.capture_boundary.as_ref().ok_or(Error::Conflict(
        "capture receipt has no exact recovery boundary",
    ))?;
    let sources = receipt
        .sources
        .as_ref()
        .ok_or(Error::Storage("capture descriptors missing"))?;
    if receipt.operation != ConstructionOperation::CheckpointCreate || !receipt.state.unresolved() {
        return Err(Error::Conflict("checkpoint capture is not unresolved"));
    }
    agent_owner::authorize_permission(tx, actor, receipt.origin(), AgentPermission::View)?;
    if agent_owner::canonical_ref(tx, receipt.origin())? != boundary.agent_ref {
        return Err(Error::Conflict("capture origin identity changed"));
    }
    boundary.validate(sources)?;
    for (binding, source) in boundary.bindings.iter().zip(&sources.sources) {
        registry.resolve(&binding.binding.pin()?)?;
        registry.resolve(&source.prepared.descriptor.pin()?)?;
    }
    Ok(())
}

impl<'a> PreparedCheckpointCapture<'a> {
    fn inspection_registry(&self) -> Result<SourceDriverRegistryReader> {
        let mut connection = self.store.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        self.require_receipt_in(&tx)?;
        let registry = self.store.inner.source_drivers.reader()?;
        require_inspection_authority_in(&tx, &self.actor, &self.receipt, &registry)?;
        db(tx.commit())?;
        Ok(registry)
    }

    /// Read-only driver inspection, on the exact original descriptors. The
    /// trusted host must independently authorize the complete source set and
    /// dispatch each operation using the compatible registered driver. No
    /// missing/partial result authorizes another execution, even NotStarted.
    /// A changed Store boundary may be inspected but cannot be published.
    pub fn inspect(
        self,
        inspect: impl FnOnce(
            &SourceDriverRegistryReader,
            &[VersionedSourceBinding],
            &PreparedSourceSet,
        ) -> Result<Vec<RecoveryInspection<SourceAnchor>>>,
    ) -> Result<InspectedCheckpointCapture<'a>> {
        let result = (|| {
            let registry = self.inspection_registry()?;
            let boundary = self.boundary();
            let inspections = inspect(&registry, &boundary.bindings, self.sources())?;
            if inspections.len() != boundary.bindings.len() {
                return Err(Error::Conflict(
                    "inspection must account for the complete source set",
                ));
            }
            let mut anchors = Vec::with_capacity(inspections.len());
            let mut evidence = Vec::with_capacity(inspections.len());
            for (version, inspection) in boundary.bindings.iter().zip(inspections) {
                let RecoveryInspection::Completed {
                    result: anchor,
                    evidence: proof,
                } = inspection
                else {
                    return Err(Error::Conflict(
                        "checkpoint capture is incomplete; inspect or reconcile before any retry",
                    ));
                };
                anchor.validate()?;
                proof.validate()?;
                let binding = &version.binding;
                if anchor.name != binding.name
                    || anchor.driver != binding.driver
                    || anchor.source_ref != binding.source_ref
                    || anchor.access != binding.access
                    || anchor.checkpoint_policy != binding.checkpoint_policy
                {
                    return Err(Error::Conflict(
                        "inspected anchor differs from the selected binding",
                    ));
                }
                registry.resolve(&anchor.pin()?)?;
                anchors.push(anchor);
                evidence.push(proof);
            }
            let sources = SourceSet {
                coverage: boundary.selection.coverage(),
                anchors,
            };
            sources.validate()?;
            // Recheck current receipt, barrier and disclosure after calling
            // external code, without relying on the previous SQLite read.
            let registry = self.inspection_registry()?;
            for anchor in &sources.anchors {
                registry.resolve(&anchor.pin()?)?;
            }
            Ok((sources, evidence))
        })();
        match result {
            Ok((sources, evidence)) => Ok(InspectedCheckpointCapture {
                capture: self,
                sources,
                evidence,
            }),
            Err(error) => {
                self.mark_unknown()?;
                Err(error)
            }
        }
    }
}

impl SessionStore {
    /// Trusted recovery of an unresolved receipt, by its original caller
    /// and request identity. No contextual target resolution, planning,
    /// creation or automatic execution occurs. An active live barrier is
    /// never stolen. Only the complete exact original hazard set may be
    /// reclaimed. A stale capture boundary remains inspectable under current
    /// disclosure and source authority, but revalidation will deny publication.
    pub fn checkpoint_inspect_capture(
        &self,
        actor: &PrincipalId,
        request_id: &str,
        inspect: impl FnOnce(
            &SourceDriverRegistryReader,
            &[VersionedSourceBinding],
            &PreparedSourceSet,
        ) -> Result<Vec<RecoveryInspection<SourceAnchor>>>,
    ) -> Result<InspectedCheckpointCapture<'_>> {
        label(request_id, 256)?;
        if self.inner.mode != crate::StoreMode::OwnerLock {
            return Err(Error::Denied(
                "checkpoint recovery requires the exclusive Store owner",
            ));
        }
        let capture = {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let creator = user_for_principal(&tx, actor)?
                .ok_or(Error::Denied("checkpoint caller is inactive or unknown"))?;
            let receipt = construction_receipt::lookup_in(
                &tx,
                ConstructionOperation::CheckpointCreate,
                &creator,
                request_id,
            )?
            .ok_or(Error::Denied("checkpoint recovery receipt unavailable"))?;
            let registry = self.inner.source_drivers.reader()?;
            require_inspection_authority_in(&tx, actor, &receipt, &registry)?;
            let checkpoint = receipt.candidate.prepare_id();
            let published: bool = db(tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM checkpoints WHERE checkpoint_id=?1
                    OR (created_by=?2 AND request_id=?3))",
                params![checkpoint, creator, request_id],
                |row| row.get(0),
            ))?;
            if published {
                return Err(Error::Conflict(
                    "checkpoint recovery already has a published resource",
                ));
            }
            let set = receipt
                .capture_boundary
                .as_ref()
                .ok_or(Error::Conflict(
                    "capture receipt has no exact recovery boundary",
                ))?
                .mutation_set()?;
            let barrier = self.recover_source_mutation_barrier_in(&tx, &set, checkpoint)?;
            db(tx.commit())?;
            PreparedCheckpointCapture {
                store: self,
                actor: actor.clone(),
                receipt,
                barrier,
            }
        };
        capture.inspect(inspect)
    }
}
