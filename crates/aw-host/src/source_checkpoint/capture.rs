//! Captured Git versions and their retention use the original Store owners.
//! These are blocking trusted-host operations, not Agent admission or RPCs.
use super::Publication;
use aw_agent_core::{
    Error, NamedPreparedSource, PreparedSourceSet, PrincipalId, RecoveryInspection, Result,
    SourceAnchor, SourceDriverHandle, SourceOperationKind, SourcePublicationProof, SourceSet,
};
use aw_git::source_driver::{GitSourceAnchor, GitSourceDriver, GitSourceLocation};
use aw_store::{
    CheckpointCaptureSelection, CheckpointSnapshot, InspectedCheckpointCapture, SessionStore,
    VersionedSourceBinding,
};

/// The independent live authority owner guarantees exact source disclosure,
/// checkout quiescence, Git administration and retention (including external
/// aliases, other Stores and live dependencies) for the authorized effect
/// through settlement. Locations are immutable in canonical source-name order.
/// Neither the Store barrier nor a directory pin provides this guarantee.
pub trait GitSourceFence {
    fn locations(&self) -> &[GitSourceLocation];
}

fn require_binding_locations(
    bindings: &[VersionedSourceBinding],
    locations: &[GitSourceLocation],
) -> Result<()> {
    if bindings.len() != locations.len()
        || bindings
            .iter()
            .zip(locations)
            .any(|(binding, location)| binding.binding.name != location.name)
    {
        return Err(Error::Denied(
            "Git approval must cover every selected binding",
        ));
    }
    Ok(())
}

fn plan(
    driver: &GitSourceDriver,
    bindings: &[VersionedSourceBinding],
    fence: &impl GitSourceFence,
    checkpoint: &str,
) -> Result<PreparedSourceSet> {
    let locations = fence.locations();
    require_binding_locations(bindings, locations)?;
    let sources = bindings
        .iter()
        .zip(locations)
        .map(|(binding, location)| {
            Ok(NamedPreparedSource {
                name: binding.binding.name.clone(),
                prepared: driver.plan_capture_approved(
                    location,
                    &binding.binding,
                    checkpoint,
                    checkpoint,
                )?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(PreparedSourceSet {
        prepare_id: checkpoint.to_owned(),
        operation: SourceOperationKind::Capture,
        sources,
    })
}

fn require_preparation(
    driver: &GitSourceDriver,
    bindings: &[VersionedSourceBinding],
    sources: &PreparedSourceSet,
    fence: &impl GitSourceFence,
) -> Result<()> {
    let locations = fence.locations();
    require_binding_locations(bindings, locations)?;
    if bindings.len() != sources.sources.len() {
        return Err(Error::Conflict(
            "Git capture preparation differs from selected bindings",
        ));
    }
    for ((binding, source), location) in bindings.iter().zip(&sources.sources).zip(locations) {
        if source.name != binding.binding.name {
            return Err(Error::Conflict(
                "Git capture preparation names another source",
            ));
        }
        driver.verify_capture_location(
            location,
            &binding.binding,
            &sources.prepare_id,
            &source.prepared,
        )?;
    }
    Ok(())
}

fn inspect(
    driver: &GitSourceDriver,
    bindings: &[VersionedSourceBinding],
    sources: &PreparedSourceSet,
    fence: &impl GitSourceFence,
) -> Result<Vec<RecoveryInspection<SourceAnchor>>> {
    require_preparation(driver, bindings, sources, fence)?;
    sources
        .sources
        .iter()
        .map(|source| match driver.inspect_capture(&source.prepared)? {
            RecoveryInspection::Completed { result, evidence } => {
                RecoveryInspection::completed((&result).try_into()?, evidence)
            }
            RecoveryInspection::NotStarted => Ok(RecoveryInspection::NotStarted),
            RecoveryInspection::Unknown => Ok(RecoveryInspection::Unknown),
        })
        .collect()
}

/// One registered Git implementation, using the original checkpoint owner.
pub struct GitCheckpointCaptureHost<'a> {
    store: &'a SessionStore,
    driver: &'a SourceDriverHandle<GitSourceDriver>,
}

impl<'a> GitCheckpointCaptureHost<'a> {
    pub fn new(store: &'a SessionStore, driver: &'a SourceDriverHandle<GitSourceDriver>) -> Self {
        Self { store, driver }
    }

    /// New source-bearing capture or exact Applied replay. History-only and
    /// empty Auto remain the existing metadata-only Store constructors.
    pub fn capture<F: GitSourceFence>(
        &self,
        actor: &PrincipalId,
        reference: &str,
        selection: CheckpointCaptureSelection,
        note: Option<&str>,
        request_id: &str,
        authorize: impl FnOnce(&[VersionedSourceBinding]) -> Result<F>,
    ) -> Result<CheckpointSnapshot> {
        if let Some(result) = self.store.checkpoint_replay_capture(
            actor,
            reference,
            selection.clone(),
            note,
            request_id,
        )? {
            return Ok(result);
        }
        let driver = self.store.source_driver(self.driver)?;
        let mut fence = None;
        let prepared = self.store.checkpoint_prepare_capture(
            actor,
            reference,
            selection,
            note,
            request_id,
            |checkpoint, bindings| {
                let approval = authorize(bindings)?;
                let sources = plan(&driver, bindings, &approval, checkpoint)?;
                fence = Some(approval);
                Ok(sources)
            },
        )?;
        let fence = fence.expect("successful preparation called planner");
        let executed = (|| {
            prepared.revalidate()?;
            require_preparation(&driver, prepared.bindings(), prepared.sources(), &fence)?;
            for source in &prepared.sources().sources {
                driver.execute_capture(&source.prepared)?;
            }
            Ok(())
        })();
        if let Err(error) = executed {
            prepared.mark_unknown()?;
            return Err(error);
        }
        let inspected = prepared.inspect(|registry, bindings, sources| {
            let driver = registry.get(self.driver)?;
            inspect(&driver, bindings, sources, &fence)
        })?;
        self.publish(inspected, &fence)
    }

    /// Reauthorize and inspect the original operation. No new planning,
    /// retention creation or retry of the external effect.
    pub fn resume<F: GitSourceFence>(
        &self,
        actor: &PrincipalId,
        request_id: &str,
        authorize: impl FnOnce(&[VersionedSourceBinding], &PreparedSourceSet) -> Result<F>,
    ) -> Result<CheckpointSnapshot> {
        let mut fence = None;
        let inspected = self.store.checkpoint_inspect_capture(
            actor,
            request_id,
            |registry, bindings, sources| {
                fence = Some(authorize(bindings, sources)?);
                let driver = registry.get(self.driver)?;
                inspect(
                    &driver,
                    bindings,
                    sources,
                    fence.as_ref().expect("approved"),
                )
            },
        )?;
        self.publish(
            inspected,
            &fence.expect("successful inspection called inspector"),
        )
    }

    fn publish<F: GitSourceFence>(
        &self,
        inspected: InspectedCheckpointCapture<'_>,
        fence: &F,
    ) -> Result<CheckpointSnapshot> {
        inspected.publish(|registry, bindings, sources, set, evidence| {
            let driver = registry.get(self.driver)?;
            require_preparation(&driver, bindings, sources, fence)?;
            sources
                .sources
                .iter()
                .zip(&set.anchors)
                .zip(evidence)
                .map(|((source, anchor), evidence)| {
                    driver.revalidate_capture(
                        &source.prepared,
                        &GitSourceAnchor::try_from(anchor)?,
                    )?;
                    // Content identity is immutable; retention and current
                    // external authorization are not. Borrow the actual fence.
                    Ok(SourcePublicationProof::Held(Publication {
                        _fence: fence,
                        evidence: evidence.clone(),
                    }))
                })
                .collect()
        })
    }

    pub fn abandon<F: GitSourceFence>(
        &self,
        actor: &PrincipalId,
        request_id: &str,
        authorize: impl FnOnce(&[VersionedSourceBinding], &PreparedSourceSet) -> Result<F>,
    ) -> Result<()> {
        self.store
            .checkpoint_abandon_capture(actor, request_id, |registry, bindings, sources| {
                let fence = authorize(bindings, sources)?;
                let driver = registry.get(self.driver)?;
                require_preparation(&driver, bindings, sources, &fence)?;
                for source in &sources.sources {
                    driver.release_prepared_capture(&source.prepared)?;
                }
                Ok(fence)
            })
    }

    /// Logical retirement remains the Store's checkpoint_delete. Release
    /// only exact retired refs, with independently approved repository
    /// identities and all dependencies fenced through Store settlement.
    /// A terminal replay needs no provider or filesystem access.
    pub fn reconcile_retired<F: GitSourceFence>(
        &self,
        actor: &PrincipalId,
        checkpoint: &str,
        authorize: impl FnOnce(&SourceSet) -> Result<F>,
    ) -> Result<()> {
        let mut fence = None;
        let result = self
            .store
            .checkpoint_reconcile_retired_sources(actor, checkpoint, |set| {
                fence = Some(authorize(set)?);
                let locations = fence.as_ref().expect("approved").locations();
                if locations.len() != set.anchors.len()
                    || locations
                        .iter()
                        .zip(&set.anchors)
                        .any(|(location, anchor)| location.name != anchor.name)
                {
                    return Err(Error::Denied(
                        "Git retention approval must cover every retired anchor",
                    ));
                }
                let driver = self.store.source_driver(self.driver)?;
                let anchors = set
                    .anchors
                    .iter()
                    .map(GitSourceAnchor::try_from)
                    .collect::<Result<Vec<_>>>()?;
                for (anchor, location) in anchors.iter().zip(locations) {
                    driver.verify_anchor_location(location, anchor)?;
                }
                for (anchor, location) in anchors.iter().zip(locations) {
                    driver.release_retired_anchor(&location.repository, anchor)?;
                }
                Ok(())
            });
        drop(fence);
        result
    }
}
