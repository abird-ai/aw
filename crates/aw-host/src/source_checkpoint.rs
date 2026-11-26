//! Concrete Git checkpoint lifecycle. Store owns Prepared custody,
//! recovery, history and final publication; the host owns physical authority.
//! Blocking trusted-host operations, not an Actor/model/RPC admission route.
use aw_agent_core::{
    Error, ExternalEvidence, NamedPreparedSource, PreparedSourceOperation, PreparedSourceSet,
    PrincipalId, RecoveryInspection, Result, SourceAnchor, SourceBinding, SourceDriverHandle,
    SourceName, SourceOperationKind, SourcePublicationFence, SourcePublicationProof, SourceSet,
};
use aw_git::source_driver::{GitRestoreLocation, GitSourceAnchor, GitSourceDriver};
use aw_store::{
    CheckpointRestoreLayout, CheckpointRestoreRequest, CheckpointRestoreResult,
    InspectedCheckpointRestore, InspectedSandboxAttachment, InspectedSandboxCreate,
    PreparedSandboxAttachment, SandboxAttachment, SandboxCreateFrom, SandboxCreateSources,
    SandboxMaterializationRequest, SandboxRecoverySelection, SandboxSnapshot, SandboxSourceRelease,
    SessionBinding, SessionStore, VersionedSourceBinding,
};
use std::{
    collections::HashSet,
    fs,
    path::{Component, Path},
};

mod capture;
pub use capture::{
    abandon_git_capture_exact, reconcile_retired_git, GitCheckpointCaptureHost, GitSourceFence,
};

/// Live, independently authorized, whole-layout physical/provider fence.
///
/// The trusted authority owner must guarantee current actor/source disclosure,
/// repository administration and retention, destination isolation (including
/// aliases and other Stores), and the permitted restore or cleanup effect until
/// this guard drops. Retirement additionally requires authority over the entire
/// retained execution layout, including any external physical dependencies.
/// Scratch and partial-restore recovery require the original private candidate
/// and reserved Git artifact namespace to have remained protected and unpublished,
/// with no surviving writer or unaccounted alias.
/// Locations are immutable, in canonical source-name order.
/// Neither a path pin, Git worktree lock nor a saved evidence handle suffices.
/// Acquisition before Prepared must be read-only, not a materializing effect.
pub trait GitRestoreFence {
    fn locations(&self) -> &[GitRestoreLocation];

    /// For replacement only, the additional original checkout origin.
    /// locations() describes the final named layout; the old and incoming
    /// selected locations share exactly the same protected destination.
    fn replaced_location(&self) -> Option<&GitRestoreLocation> {
        None
    }
}

// Borrow the actual whole-set guard through the Store publication transaction.
// The inspected identity labels its exact checked result; it grants nothing.
struct Publication<'a, F> {
    _fence: &'a F,
    evidence: ExternalEvidence,
}

impl<F> SourcePublicationFence for Publication<'_, F> {
    fn evidence(&self) -> &ExternalEvidence {
        &self.evidence
    }
}

fn approved_anchors(
    anchors: &SourceSet,
    fence: &impl GitRestoreFence,
) -> Result<Vec<GitSourceAnchor>> {
    let locations = fence.locations();
    if anchors.anchors.len() != locations.len()
        || anchors
            .anchors
            .iter()
            .zip(locations)
            .any(|(anchor, location)| anchor.name != location.name)
    {
        return Err(Error::Denied(
            "Git restore approval must cover the exact named SourceSet",
        ));
    }
    require_disjoint_locations(locations)?;
    anchors
        .anchors
        .iter()
        .map(GitSourceAnchor::try_from)
        .collect()
}

fn require_disjoint_locations(locations: &[GitRestoreLocation]) -> Result<()> {
    // Cheap lexical checks in addition to (not in place of) the host's
    // physical alias/administration/whole-layout isolation guarantee.
    for (i, location) in locations.iter().enumerate() {
        for other in &locations[i + 1..] {
            if location.target.starts_with(&other.target)
                || other.target.starts_with(&location.target)
            {
                return Err(Error::Denied("Git restore destinations overlap"));
            }
        }
        if locations.iter().any(|other| {
            location.target.starts_with(&other.repository)
                || other.repository.starts_with(&location.target)
        }) {
            return Err(Error::Denied("Git restore destination overlaps an origin"));
        }
    }
    Ok(())
}

/// Only layouts whose complete release can be proven without deleting any
/// additional user or host files. The physical fence must exclude outside
/// users, aliases and mounts, not merely hold the directory open.
enum ManagedGitLayout<'a> {
    Checkout(&'a Path),
    Directory(&'a Path),
}

impl<'a> ManagedGitLayout<'a> {
    fn check(sandbox: &'a SandboxSnapshot, locations: &[GitRestoreLocation]) -> Result<Self> {
        let root = sandbox.tool_root.as_path();
        let cwd = sandbox
            .working_directory
            .strip_prefix(root)
            .map_err(|_| Error::Denied("Git retirement requires cwd inside the managed layout"))?;
        if cwd
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(Error::Denied(
                "Git retirement requires a contained working directory",
            ));
        }
        if let [location] = locations {
            if location.target == root {
                return Ok(Self::Checkout(root));
            }
        }
        if locations.is_empty()
            || locations
                .iter()
                .any(|location| location.target.parent() != Some(root))
            || root.canonicalize()? != root
        {
            return Err(Error::Denied("unsupported managed Git retirement layout"));
        }
        // The approved parent was already identity-checked by the driver.
        // Refuse foreign entries before deleting any managed checkout.
        let targets = locations
            .iter()
            .map(|location| location.target.as_path())
            .collect::<HashSet<_>>();
        for entry in fs::read_dir(root)? {
            if !targets.contains(entry?.path().as_path()) {
                return Err(Error::Conflict(
                    "managed Git layout contains unrelated files",
                ));
            }
        }
        Ok(Self::Directory(root))
    }

    fn require_released(self) -> Result<()> {
        match self {
            Self::Checkout(root) => match fs::symlink_metadata(root) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
                Ok(_) => Err(Error::Conflict("managed Git checkout remains present")),
            },
            Self::Directory(root) => {
                if root.canonicalize()? != root || fs::read_dir(root)?.next().transpose()?.is_some()
                {
                    return Err(Error::Conflict("managed Git layout is not empty"));
                }
                Ok(())
            }
        }
    }
}

fn verify_managed_materializations<'a>(
    driver: &GitSourceDriver,
    snapshot: &'a SandboxSnapshot,
    bindings: &[VersionedSourceBinding],
    prepared: &[NamedPreparedSource],
    fence: &impl GitRestoreFence,
) -> Result<ManagedGitLayout<'a>> {
    let locations = fence.locations();
    if prepared.len() != bindings.len()
        || bindings.len() != locations.len()
        || bindings
            .iter()
            .zip(prepared)
            .zip(locations)
            .any(|((binding, source), location)| {
                binding.binding.name != source.name || source.name != location.name
            })
    {
        return Err(Error::Conflict(
            "retained Git bindings differ from the original restore",
        ));
    }
    require_disjoint_locations(locations)?;
    for ((binding, source), location) in bindings.iter().zip(prepared).zip(locations) {
        driver.verify_restored_materialization_location(
            location,
            &source.prepared,
            &binding.binding,
        )?;
    }
    ManagedGitLayout::check(snapshot, locations)
}

fn verify_remaining_sources(
    driver: &GitSourceDriver,
    release: &SandboxSourceRelease,
    fence: &impl GitRestoreFence,
) -> Result<()> {
    // Other checkouts need their original identity, not original contents:
    // independently modified siblings are not part of the release.
    for (index, (binding, location)) in release.bindings().iter().zip(fence.locations()).enumerate()
    {
        if index != release.selected() {
            verify_retained_checkout(driver, &binding.binding, location)?;
        }
    }
    Ok(())
}

fn verify_retained_checkout(
    driver: &GitSourceDriver,
    original: &SourceBinding,
    location: &GitRestoreLocation,
) -> Result<()> {
    if driver.binding(
        &location.target,
        original.name.clone(),
        original.access,
        original.checkpoint_policy,
    )? != *original
    {
        return Err(Error::Conflict("retained Git sibling changed identity"));
    }
    Ok(())
}

// Every retained root and incoming target is independently approved.
// Sibling content need not remain at its original captured version.
fn attachment_location<'a>(
    driver: &GitSourceDriver,
    attachment: &SandboxAttachment,
    fence: &'a impl GitRestoreFence,
) -> Result<&'a GitRestoreLocation> {
    let snapshot = attachment.snapshot();
    let locations = fence.locations();
    let replaced = attachment.replaced();
    if locations.len() != attachment.bindings().len() + usize::from(replaced.is_none())
        || replaced.is_some() != fence.replaced_location().is_some()
        || snapshot.working_directory != snapshot.tool_root
        || locations
            .windows(2)
            .any(|pair| pair[0].name.as_str() >= pair[1].name.as_str())
        || locations
            .iter()
            .any(|location| location.target != snapshot.tool_root.join(location.name.as_str()))
    {
        return Err(Error::Denied(
            "attachment requires exact private named-child layout",
        ));
    }
    require_disjoint_locations(locations)?;
    if !matches!(
        ManagedGitLayout::check(snapshot, locations)?,
        ManagedGitLayout::Directory(_)
    ) {
        return Err(Error::Denied(
            "attachment cannot reinterpret a whole-root layout",
        ));
    }
    let mut existing = attachment.bindings().iter().zip(attachment.preparations());
    let mut incoming = None;
    for location in locations {
        if location.name == attachment.anchor().name {
            incoming = Some(location);
            if let Some((old_binding, old_source)) = replaced {
                let (binding, source) = existing
                    .next()
                    .ok_or(Error::Conflict("replacement source set changed"))?;
                let old_location = fence
                    .replaced_location()
                    .expect("checked replacement fence");
                if binding != old_binding
                    || source != old_source
                    || old_location.name != location.name
                    || old_location.target != location.target
                    || old_location.parent_identity != location.parent_identity
                    || locations.iter().any(|site| {
                        site.target.starts_with(&old_location.repository)
                            || old_location.repository.starts_with(&site.target)
                    })
                {
                    return Err(Error::Denied(
                        "replacement requires the exact old origin and named destination",
                    ));
                }
                // The old selected target may be absent or already
                // replaced. Its original descriptor still fixes origin.
                driver.verify_restored_materialization_location(
                    old_location,
                    &source.prepared,
                    &binding.binding,
                )?;
            }
        } else {
            let (binding, source) = existing
                .next()
                .ok_or(Error::Conflict("attachment source set changed"))?;
            if source.name != location.name || binding.binding.name != location.name {
                return Err(Error::Conflict("attachment source set changed"));
            }
            driver.verify_restored_materialization_location(
                location,
                &source.prepared,
                &binding.binding,
            )?;
            verify_retained_checkout(driver, &binding.binding, location)?;
        }
    }
    if existing.next().is_some() {
        return Err(Error::Conflict("attachment source set is incomplete"));
    }
    incoming.ok_or(Error::Denied("incoming attachment is not approved"))
}

fn plan_attachment(
    driver: &GitSourceDriver,
    attachment: &SandboxAttachment,
    fence: &impl GitRestoreFence,
    prepare_id: &str,
) -> Result<PreparedSourceOperation> {
    let location = attachment_location(driver, attachment, fence)?;
    let anchor = GitSourceAnchor::try_from(attachment.anchor())?;
    if let Some((binding, source)) = attachment.replaced() {
        driver.plan_restore_replacing_approved(
            location,
            &anchor,
            prepare_id,
            fence
                .replaced_location()
                .expect("checked replacement fence"),
            &source.prepared,
            &binding.binding,
        )
    } else {
        driver.plan_restore_approved(location, &anchor, prepare_id)
    }
}

fn verify_attachment(
    driver: &GitSourceDriver,
    attachment: &SandboxAttachment,
    prepared: &PreparedSourceOperation,
    fence: &impl GitRestoreFence,
) -> Result<()> {
    let location = attachment_location(driver, attachment, fence)?;
    let anchor = GitSourceAnchor::try_from(attachment.anchor())?;
    driver.verify_restore_location(location, &anchor, prepared)
}

fn approved_parent(
    bindings: &[VersionedSourceBinding],
    fence: &impl GitRestoreFence,
) -> Result<()> {
    let locations = fence.locations();
    if bindings.len() != locations.len()
        || bindings
            .iter()
            .zip(locations)
            .any(|(source, location)| source.binding.name != location.name)
    {
        return Err(Error::Denied(
            "Git derive approval must cover exact required parent sources",
        ));
    }
    require_disjoint_locations(locations)
}

fn plan(
    driver: &GitSourceDriver,
    input: SandboxCreateSources<'_>,
    fence: &impl GitRestoreFence,
    prepare_id: &str,
) -> Result<PreparedSourceSet> {
    let (operation, sources) = match input {
        SandboxCreateSources::Baseline(anchors) => {
            let anchors = approved_anchors(anchors, fence)?;
            (
                SourceOperationKind::Restore,
                anchors
                    .iter()
                    .zip(fence.locations())
                    .map(|(anchor, location)| {
                        Ok(NamedPreparedSource {
                            name: anchor.name.clone(),
                            prepared: driver.plan_restore_approved(location, anchor, prepare_id)?,
                        })
                    })
                    .collect::<Result<_>>()?,
            )
        }
        SandboxCreateSources::Parent(bindings) => {
            approved_parent(bindings, fence)?;
            (
                SourceOperationKind::Derive,
                bindings
                    .iter()
                    .zip(fence.locations())
                    .map(|(source, location)| {
                        Ok(NamedPreparedSource {
                            name: source.binding.name.clone(),
                            prepared: driver.plan_derive_approved(
                                location,
                                &source.binding,
                                prepare_id,
                            )?,
                        })
                    })
                    .collect::<Result<_>>()?,
            )
        }
    };
    Ok(PreparedSourceSet {
        prepare_id: prepare_id.to_owned(),
        operation,
        sources,
    })
}

fn require_locations(
    driver: &GitSourceDriver,
    input: SandboxCreateSources<'_>,
    prepared: &PreparedSourceSet,
    fence: &impl GitRestoreFence,
) -> Result<()> {
    if prepared.sources.len() != fence.locations().len() {
        return Err(Error::Conflict(
            "Git materialization preparation differs from approved source set",
        ));
    }
    match input {
        SandboxCreateSources::Baseline(anchors) => {
            let anchors = approved_anchors(anchors, fence)?;
            if prepared.operation != SourceOperationKind::Restore {
                return Err(Error::Conflict("not a Git restore preparation"));
            }
            for ((anchor, source), location) in
                anchors.iter().zip(&prepared.sources).zip(fence.locations())
            {
                if source.name != anchor.name {
                    return Err(Error::Conflict(
                        "Git restore preparation names another source",
                    ));
                }
                driver.verify_restore_location(location, anchor, &source.prepared)?;
            }
        }
        SandboxCreateSources::Parent(bindings) => {
            approved_parent(bindings, fence)?;
            if prepared.operation != SourceOperationKind::Derive {
                return Err(Error::Conflict("not a Git derive preparation"));
            }
            for ((binding, source), location) in bindings
                .iter()
                .zip(&prepared.sources)
                .zip(fence.locations())
            {
                if source.name != binding.binding.name {
                    return Err(Error::Conflict(
                        "Git derive preparation names another source",
                    ));
                }
                driver.verify_derive_location(location, &binding.binding, &source.prepared)?;
            }
        }
    }
    Ok(())
}

fn inspect(
    driver: &GitSourceDriver,
    input: SandboxCreateSources<'_>,
    prepared: &PreparedSourceSet,
    fence: &impl GitRestoreFence,
) -> Result<Vec<RecoveryInspection<SourceBinding>>> {
    require_locations(driver, input, prepared, fence)?;
    prepared
        .sources
        .iter()
        .map(|source| match input {
            SandboxCreateSources::Baseline(_) => driver.inspect_restore(&source.prepared),
            SandboxCreateSources::Parent(_) => driver.inspect_derive(&source.prepared),
        })
        .collect()
}

fn execute(
    driver: &GitSourceDriver,
    input: SandboxCreateSources<'_>,
    prepared: &PreparedSourceSet,
    fence: &impl GitRestoreFence,
) -> Result<()> {
    require_locations(driver, input, prepared, fence)?;
    for source in &prepared.sources {
        match input {
            SandboxCreateSources::Baseline(_) => driver.execute_restore(&source.prepared)?,
            SandboxCreateSources::Parent(_) => driver.execute_derive(&source.prepared)?,
        };
    }
    Ok(())
}

fn publication_proofs<'a, F: GitRestoreFence>(
    driver: &GitSourceDriver,
    input: SandboxCreateSources<'_>,
    sources: &PreparedSourceSet,
    bindings: &[SourceBinding],
    evidence: &[ExternalEvidence],
    fence: &'a F,
) -> Result<Vec<SourcePublicationProof<Publication<'a, F>>>> {
    require_locations(driver, input, sources, fence)?;
    sources
        .sources
        .iter()
        .zip(bindings)
        .zip(evidence)
        .map(|((source, binding), evidence)| {
            match input {
                SandboxCreateSources::Baseline(_) => {
                    driver.revalidate_restore(&source.prepared, binding)?
                }
                SandboxCreateSources::Parent(_) => {
                    driver.revalidate_derive(&source.prepared, binding)?
                }
            };
            Ok(SourcePublicationProof::Held(Publication {
                _fence: fence,
                evidence: evidence.clone(),
            }))
        })
        .collect()
}

fn release_materializations(
    driver: &GitSourceDriver,
    input: SandboxCreateSources<'_>,
    sources: &PreparedSourceSet,
    fence: &impl GitRestoreFence,
) -> Result<()> {
    require_locations(driver, input, sources, fence)?;
    for source in &sources.sources {
        match input {
            SandboxCreateSources::Baseline(_) => {
                driver.release_prepared_restore(&source.prepared)?
            }
            SandboxCreateSources::Parent(_) => driver.release_prepared_derive(&source.prepared)?,
        }
    }
    Ok(())
}

fn require_derive_layout(
    layout: &SessionBinding,
    input: SandboxCreateSources<'_>,
    fence: &impl GitRestoreFence,
) -> Result<()> {
    if let SandboxCreateSources::Parent(bindings) = input {
        let cwd = layout
            .working_directory
            .strip_prefix(&layout.tool_root)
            .map_err(|_| Error::Denied("derived cwd must stay within its layout"))?;
        if cwd
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(Error::Denied("derived cwd must stay within its layout"));
        }
        for location in fence.locations() {
            crate::source_filesystem::require_named_root(
                &layout.tool_root,
                &location.name,
                &location.target,
                bindings.len(),
            )?;
        }
    }
    Ok(())
}

/// Uses the actual SourceDriver registered in the Store; no second driver
/// registry, authority state, replay ledger or restore constructor.
pub struct GitCheckpointRestoreHost<'a> {
    store: &'a SessionStore,
    driver: &'a SourceDriverHandle<GitSourceDriver>,
}

impl<'a> GitCheckpointRestoreHost<'a> {
    pub fn new(store: &'a SessionStore, driver: &'a SourceDriverHandle<GitSourceDriver>) -> Self {
        Self { store, driver }
    }

    /// New captured-source restore, or exact Applied replay. Layout selection
    /// and independent approval are not consulted on replay. The host selects
    /// an inert layout and authorizes the complete named set before Git reads
    /// or effects; the returned live guard spans the actual Store commit.
    /// Baseline versions are selected explicitly by restore_versioned_baseline.
    /// Non-Git SourceSets require their own host route.
    pub fn restore<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        request: &CheckpointRestoreRequest,
        request_id: &str,
        execution_site: &str,
        select_layout: impl FnOnce() -> Result<SessionBinding>,
        authorize: impl FnOnce(&SessionBinding, &SourceSet) -> Result<F>,
    ) -> Result<CheckpointRestoreResult> {
        self.restore_selected(
            actor,
            request,
            request_id,
            execution_site,
            || Ok((select_layout()?, None)),
            authorize,
        )
    }

    /// Restore history into a fresh, independently selected versioned
    /// destination baseline. Selection is read-only and occurs only for new
    /// work. The host owns retention of every chosen version and current
    /// destination policy; this never captures or alters the checkpoint.
    pub fn restore_versioned_baseline<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        request: &CheckpointRestoreRequest,
        request_id: &str,
        execution_site: &str,
        select: impl FnOnce() -> Result<(SessionBinding, SourceSet)>,
        authorize: impl FnOnce(&SessionBinding, &SourceSet) -> Result<F>,
    ) -> Result<CheckpointRestoreResult> {
        self.restore_selected(
            actor,
            request,
            request_id,
            execution_site,
            || {
                let (layout, sources) = select()?;
                Ok((layout, Some(sources)))
            },
            authorize,
        )
    }

    fn restore_selected<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        request: &CheckpointRestoreRequest,
        request_id: &str,
        execution_site: &str,
        select: impl FnOnce() -> Result<(SessionBinding, Option<SourceSet>)>,
        authorize: impl FnOnce(&SessionBinding, &SourceSet) -> Result<F>,
    ) -> Result<CheckpointRestoreResult> {
        if let Some(result) = self
            .store
            .checkpoint_replay_restore(actor, request, request_id)?
        {
            return Ok(result);
        }
        let (layout, baseline) = select()?;
        let driver = self.store.source_driver(self.driver)?;
        let mut fence = None;
        let prepared = self.store.checkpoint_prepare_restore_with_layout(
            actor,
            request,
            CheckpointRestoreLayout {
                binding: &layout,
                baseline,
            },
            request_id,
            execution_site,
            |prepare_id, _, anchors| {
                let approval = authorize(&layout, anchors)?;
                let sources = plan(
                    &driver,
                    SandboxCreateSources::Baseline(anchors),
                    &approval,
                    prepare_id,
                )?;
                fence = Some(approval);
                Ok(sources)
            },
        )?;
        let fence = fence.expect("successful preparation called planner");
        // All original descriptors and locations are checked before the
        // first Git mutation. An uncertain result never retries execution.
        let executed = (|| {
            prepared.revalidate(&layout)?;
            execute(
                &driver,
                SandboxCreateSources::Baseline(prepared.anchors()),
                prepared.sources(),
                &fence,
            )
        })();
        if let Err(error) = executed {
            prepared.mark_unknown()?;
            return Err(error);
        }
        let inspected = prepared.inspect(|registry, anchors, sources| {
            let driver = registry.get(self.driver)?;
            inspect(
                &driver,
                SandboxCreateSources::Baseline(anchors),
                sources,
                &fence,
            )
        })?;
        self.publish(inspected, &layout, &fence)
    }

    /// Explicit recovery of the original unresolved request. The host must
    /// freshly authorize the original descriptors and physical locations.
    /// Inspect only; never select a new materialization or execute again.
    pub fn resume<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        request_id: &str,
        layout: &SessionBinding,
        authorize: impl FnOnce(&SessionBinding, &SourceSet, &PreparedSourceSet) -> Result<F>,
    ) -> Result<CheckpointRestoreResult> {
        self.resume_in(actor, request_id, layout, None, authorize)
    }

    /// Exact captured-source request and layout recovery; no alternate
    /// baseline, candidate or semantic request can be inspected here.
    pub fn resume_captured_exact<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        request: &CheckpointRestoreRequest,
        request_id: &str,
        layout: &SessionBinding,
        authorize: impl FnOnce(&SessionBinding, &SourceSet, &PreparedSourceSet) -> Result<F>,
    ) -> Result<CheckpointRestoreResult> {
        self.resume_in(actor, request_id, layout, Some(request), authorize)
    }

    fn resume_in<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        request_id: &str,
        layout: &SessionBinding,
        request: Option<&CheckpointRestoreRequest>,
        authorize: impl FnOnce(&SessionBinding, &SourceSet, &PreparedSourceSet) -> Result<F>,
    ) -> Result<CheckpointRestoreResult> {
        let mut fence = None;
        let inspector = |registry: &aw_agent_core::SourceDriverRegistryReader,
                         anchors: &SourceSet,
                         sources: &PreparedSourceSet| {
            // Keep the authority through the Store's Unknown settlement even
            // when driver inspection fails, not just on successful inspection.
            fence = Some(authorize(layout, anchors, sources)?);
            let driver = registry.get(self.driver)?;
            inspect(
                &driver,
                SandboxCreateSources::Baseline(anchors),
                sources,
                fence.as_ref().expect("approved"),
            )
        };
        let inspected = match request {
            Some(request) => self.store.checkpoint_inspect_captured_restore_exact(
                actor, request, layout, request_id, inspector,
            ),
            None => self
                .store
                .checkpoint_inspect_restore(actor, request_id, inspector),
        }?;
        self.publish(
            inspected,
            layout,
            &fence.expect("successful inspection called inspector"),
        )
    }

    fn publish<F: GitRestoreFence>(
        &self,
        inspected: InspectedCheckpointRestore<'_>,
        layout: &SessionBinding,
        fence: &F,
    ) -> Result<CheckpointRestoreResult> {
        inspected.publish(layout, |registry, anchors, sources, bindings, evidence| {
            let driver = registry.get(self.driver)?;
            publication_proofs(
                &driver,
                SandboxCreateSources::Baseline(anchors),
                sources,
                bindings,
                evidence,
                fence,
            )
        })
    }

    /// Explicitly abandon an original unpublished candidate, with independent
    /// cleanup authority. No broad prune, new descriptor or Applied cleanup.
    /// The Store retains the returned live guard through Rejected settlement.
    pub fn abandon<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        request_id: &str,
        authorize: impl FnOnce(&SourceSet, &PreparedSourceSet) -> Result<F>,
    ) -> Result<()> {
        self.abandon_in(actor, request_id, None, authorize)
    }

    /// Abandon the exact original captured restore and its selected layout.
    /// A separately selected baseline or published candidate is not eligible.
    pub fn abandon_captured_exact<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        request: &CheckpointRestoreRequest,
        request_id: &str,
        layout: &SessionBinding,
        authorize: impl FnOnce(&SourceSet, &PreparedSourceSet) -> Result<F>,
    ) -> Result<()> {
        self.abandon_in(actor, request_id, Some((request, layout)), authorize)
    }

    fn abandon_in<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        request_id: &str,
        exact: Option<(&CheckpointRestoreRequest, &SessionBinding)>,
        authorize: impl FnOnce(&SourceSet, &PreparedSourceSet) -> Result<F>,
    ) -> Result<()> {
        let mut fence = None;
        let reconcile = |registry: &aw_agent_core::SourceDriverRegistryReader,
                         anchors: &SourceSet,
                         sources: &PreparedSourceSet| {
            fence = Some(authorize(anchors, sources)?);
            let driver = registry.get(self.driver)?;
            release_materializations(
                &driver,
                SandboxCreateSources::Baseline(anchors),
                sources,
                fence.as_ref().expect("approved"),
            )
        };
        match exact {
            Some((request, layout)) => self.store.checkpoint_abandon_captured_restore_exact(
                actor, request, layout, request_id, reconcile,
            ),
            None => self
                .store
                .checkpoint_abandon_restore(actor, request_id, reconcile),
        }
    }

    /// Standalone fresh Sandbox from independently retained Project
    /// baseline versions. Same materialization planning, execution and
    /// publication proofs as checkpoint restore, but no Agent/history.
    pub fn create_sandbox_from_baseline<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        request_id: &str,
        select: impl FnOnce() -> Result<(SessionBinding, SourceSet)>,
        authorize: impl FnOnce(&SessionBinding, &SourceSet) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        self.create_sandbox_selected(
            actor,
            project_ref,
            None,
            request_id,
            None,
            || {
                let (layout, baseline) = select()?;
                Ok((layout, SandboxCreateFrom::Baseline(baseline)))
            },
            |layout, input| {
                let SandboxCreateSources::Baseline(anchors) = input else {
                    return Err(Error::Conflict("expected versioned baseline"));
                };
                authorize(layout, anchors)
            },
        )
    }

    /// Derive an isolated child from the complete current required source
    /// set of an open same-Project parent. No public checkpoint or Agent.
    /// The independent host fence protects parent content, destination,
    /// Git administration and retained child backing through publication.
    pub fn derive_sandbox<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        parent: &str,
        request_id: &str,
        select_layout: impl FnOnce() -> Result<SessionBinding>,
        authorize: impl FnOnce(&SessionBinding, &[VersionedSourceBinding]) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        self.derive_sandbox_in(
            actor,
            project_ref,
            parent,
            request_id,
            None,
            select_layout,
            authorize,
        )
    }

    /// Independently approved exact current parent revision.
    #[allow(clippy::too_many_arguments)]
    pub fn derive_sandbox_exact<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        parent: &str,
        request_id: &str,
        parent_rev: u64,
        select_layout: impl FnOnce() -> Result<SessionBinding>,
        authorize: impl FnOnce(&SessionBinding, &[VersionedSourceBinding]) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        self.derive_sandbox_in(
            actor,
            project_ref,
            parent,
            request_id,
            Some(parent_rev),
            select_layout,
            authorize,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn derive_sandbox_in<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        parent: &str,
        request_id: &str,
        parent_rev: Option<u64>,
        select_layout: impl FnOnce() -> Result<SessionBinding>,
        authorize: impl FnOnce(&SessionBinding, &[VersionedSourceBinding]) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        self.create_sandbox_selected(
            actor,
            project_ref,
            Some(parent),
            request_id,
            parent_rev,
            || Ok((select_layout()?, SandboxCreateFrom::Parent(parent))),
            |layout, input| {
                let SandboxCreateSources::Parent(bindings) = input else {
                    return Err(Error::Conflict("expected parent sources"));
                };
                authorize(layout, bindings)
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn create_sandbox_selected<'b, F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        from: Option<&str>,
        request_id: &str,
        parent_rev: Option<u64>,
        select: impl FnOnce() -> Result<(SessionBinding, SandboxCreateFrom<'b>)>,
        authorize: impl FnOnce(&SessionBinding, SandboxCreateSources<'_>) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        if let Some(result) =
            self.store
                .sandbox_replay_create(actor, project_ref, from, request_id)?
        {
            return Ok(result);
        }
        let (layout, selection) = select()?;
        let driver = self.store.source_driver(self.driver)?;
        let mut fence = None;
        let prepare = |prepare_id: &str, _: &str, input: SandboxCreateSources<'_>| {
            let approval = authorize(&layout, input)?;
            require_derive_layout(&layout, input, &approval)?;
            let sources = plan(&driver, input, &approval, prepare_id)?;
            fence = Some(approval);
            Ok(sources)
        };
        let prepared = match parent_rev {
            Some(rev) => self.store.sandbox_prepare_derive_exact(
                actor,
                project_ref,
                from.ok_or(Error::Denied("exact derivation requires parent"))?,
                rev,
                &layout,
                request_id,
                prepare,
            ),
            None => self.store.sandbox_prepare_create(
                actor,
                project_ref,
                &layout,
                selection,
                request_id,
                prepare,
            ),
        }?;
        let fence = fence.expect("successful Sandbox preparation called planner");
        let executed = (|| {
            prepared.revalidate(&layout)?;
            execute(&driver, prepared.input(), prepared.sources(), &fence)
        })();
        if let Err(error) = executed {
            prepared.mark_unknown()?;
            return Err(error);
        }
        let inspected = prepared.inspect(|registry, input, sources| {
            let driver = registry.get(self.driver)?;
            inspect(&driver, input, sources, &fence)
        })?;
        self.publish_sandbox(inspected, &layout, &fence)
    }

    pub fn resume_sandbox_create<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        from: Option<&str>,
        request_id: &str,
        layout: &SessionBinding,
        authorize: impl FnOnce(
            &SessionBinding,
            SandboxCreateSources<'_>,
            &PreparedSourceSet,
        ) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        self.resume_sandbox_create_in(
            actor,
            project_ref,
            from,
            request_id,
            layout,
            None,
            authorize,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn resume_sandbox_derive_exact<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        parent: &str,
        request_id: &str,
        layout: &SessionBinding,
        parent_rev: u64,
        authorize: impl FnOnce(
            &SessionBinding,
            SandboxCreateSources<'_>,
            &PreparedSourceSet,
        ) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        self.resume_sandbox_create_exact(
            actor,
            project_ref,
            request_id,
            layout,
            SandboxRecoverySelection::Parent {
                reference: parent,
                rev: parent_rev,
            },
            authorize,
        )
    }

    /// Recover the original independently selected baseline or parent
    /// through the same inspector and held publication path.
    pub fn resume_sandbox_create_exact<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        request_id: &str,
        layout: &SessionBinding,
        selection: SandboxRecoverySelection<'_>,
        authorize: impl FnOnce(
            &SessionBinding,
            SandboxCreateSources<'_>,
            &PreparedSourceSet,
        ) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        self.resume_sandbox_create_in(
            actor,
            project_ref,
            selection.parent_ref(),
            request_id,
            layout,
            Some(selection),
            authorize,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn resume_sandbox_create_in<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        from: Option<&str>,
        request_id: &str,
        layout: &SessionBinding,
        selection: Option<SandboxRecoverySelection<'_>>,
        authorize: impl FnOnce(
            &SessionBinding,
            SandboxCreateSources<'_>,
            &PreparedSourceSet,
        ) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        let mut fence = None;
        let inspector = |registry: &aw_agent_core::SourceDriverRegistryReader,
                         input: SandboxCreateSources<'_>,
                         sources: &PreparedSourceSet| {
            fence = Some(authorize(layout, input, sources)?);
            let approval = fence.as_ref().expect("approved");
            require_derive_layout(layout, input, approval)?;
            let driver = registry.get(self.driver)?;
            inspect(&driver, input, sources, approval)
        };
        let inspected = match selection {
            Some(selection) => self.store.sandbox_inspect_create_exact(
                actor,
                project_ref,
                request_id,
                layout,
                selection,
                inspector,
            ),
            None => {
                self.store
                    .sandbox_inspect_create(actor, project_ref, from, request_id, inspector)
            }
        }?;
        self.publish_sandbox(
            inspected,
            layout,
            &fence.expect("successful Sandbox inspection called inspector"),
        )
    }

    fn publish_sandbox<F: GitRestoreFence>(
        &self,
        inspected: InspectedSandboxCreate<'_>,
        layout: &SessionBinding,
        fence: &F,
    ) -> Result<SandboxSnapshot> {
        inspected.publish(layout, |registry, input, sources, bindings, evidence| {
            let driver = registry.get(self.driver)?;
            publication_proofs(&driver, input, sources, bindings, evidence, fence)
        })
    }

    pub fn abandon_sandbox_create<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        from: Option<&str>,
        request_id: &str,
        authorize: impl FnOnce(SandboxCreateSources<'_>, &PreparedSourceSet) -> Result<F>,
    ) -> Result<()> {
        let mut fence = None;
        self.store.sandbox_abandon_create(
            actor,
            project_ref,
            from,
            request_id,
            |registry, input, sources| {
                fence = Some(authorize(input, sources)?);
                let driver = registry.get(self.driver)?;
                release_materializations(&driver, input, sources, fence.as_ref().expect("approved"))
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn abandon_sandbox_derive_exact<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        parent: &str,
        request_id: &str,
        layout: &SessionBinding,
        parent_rev: u64,
        authorize: impl FnOnce(SandboxCreateSources<'_>, &PreparedSourceSet) -> Result<F>,
    ) -> Result<()> {
        self.abandon_sandbox_create_exact(
            actor,
            project_ref,
            request_id,
            layout,
            SandboxRecoverySelection::Parent {
                reference: parent,
                rev: parent_rev,
            },
            authorize,
        )
    }

    pub fn abandon_sandbox_create_exact<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        request_id: &str,
        layout: &SessionBinding,
        selection: SandboxRecoverySelection<'_>,
        authorize: impl FnOnce(SandboxCreateSources<'_>, &PreparedSourceSet) -> Result<F>,
    ) -> Result<()> {
        let mut fence = None;
        self.store.sandbox_abandon_create_exact(
            actor,
            project_ref,
            request_id,
            layout,
            selection,
            |registry, input, sources| {
                fence = Some(authorize(input, sources)?);
                let approval = fence.as_ref().expect("approved");
                require_derive_layout(layout, input, approval)?;
                let driver = registry.get(self.driver)?;
                release_materializations(&driver, input, sources, approval)
            },
        )
    }

    /// Attach one independently retained version to an unused managed
    /// Sandbox. The complete old+incoming layout is protected by one
    /// live fence, and its exact original restore is Prepared before use.
    pub fn attach_sandbox_source<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        expected_rev: u64,
        request_id: &str,
        anchor: SourceAnchor,
        authorize: impl FnOnce(&SandboxAttachment) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        self.materialize_sandbox_source(
            actor,
            sandbox,
            expected_rev,
            request_id,
            (anchor, false),
            authorize,
        )
    }

    /// Replace an occupied named source using a single Prepared claim and
    /// final publication. Independent authority must permit disposal of
    /// the exact unchanged old materialization and the incoming version.
    pub fn replace_sandbox_source<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        expected_rev: u64,
        request_id: &str,
        anchor: SourceAnchor,
        authorize: impl FnOnce(&SandboxAttachment) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        self.materialize_sandbox_source(
            actor,
            sandbox,
            expected_rev,
            request_id,
            (anchor, true),
            authorize,
        )
    }

    fn materialize_sandbox_source<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        expected_rev: u64,
        request_id: &str,
        selection: (SourceAnchor, bool),
        authorize: impl FnOnce(&SandboxAttachment) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        let (anchor, replace) = selection;
        let replay = if replace {
            self.store.sandbox_replay_replacement(
                actor,
                sandbox,
                expected_rev,
                request_id,
                &anchor,
            )?
        } else {
            self.store.sandbox_replay_attachment(
                actor,
                sandbox,
                expected_rev,
                request_id,
                &anchor,
            )?
        };
        if let Some(result) = replay {
            return Ok(result);
        }
        let driver = self.store.source_driver(self.driver)?;
        let mut fence = None;
        let plan = |attachment: &SandboxAttachment, prepare_id: &str| {
            let approval = authorize(attachment)?;
            let prepared = plan_attachment(&driver, attachment, &approval, prepare_id)?;
            fence = Some(approval);
            Ok(prepared)
        };
        let prepared = if replace {
            self.store.sandbox_prepare_replacement(
                actor,
                sandbox,
                expected_rev,
                request_id,
                anchor,
                plan,
            )?
        } else {
            self.store.sandbox_prepare_attachment(
                actor,
                sandbox,
                expected_rev,
                request_id,
                anchor,
                plan,
            )?
        };
        let fence = fence.expect("successful attachment called planner");
        let result = (|| {
            prepared.revalidate()?;
            verify_attachment(&driver, prepared.observation(), prepared.source(), &fence)?;
            if let Some((old, source)) = prepared.observation().replaced() {
                driver.verify_replacement_untouched(
                    &source.prepared,
                    &old.binding,
                    prepared.source(),
                )?;
                driver.release_restored_materialization(&source.prepared, &old.binding)?;
                driver.require_restore_absent(prepared.source())?;
            }
            driver.execute_restore(prepared.source())?;
            Ok(())
        })();
        if let Err(error) = result {
            prepared.mark_unknown()?;
            return Err(error);
        }
        self.inspect_and_publish_attachment(prepared, &fence)
    }

    fn inspect_and_publish_attachment<F: GitRestoreFence>(
        &self,
        prepared: PreparedSandboxAttachment<'_>,
        fence: &F,
    ) -> Result<SandboxSnapshot> {
        let inspected = prepared.inspect(|registry, attachment, prepared| {
            let driver = registry.get(self.driver)?;
            verify_attachment(&driver, attachment, prepared, fence)?;
            driver.inspect_restore(prepared)
        })?;
        self.publish_attachment(inspected, fence)
    }

    /// Explicit recovery after releasing the old checkout. Normal
    /// resume is inspect-only. Retain a completed incoming checkout;
    /// reconcile only exact unpublished partial effects before retry.
    pub fn continue_sandbox_replacement<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        request_id: &str,
        authorize: impl FnOnce(&SandboxAttachment, &PreparedSourceOperation) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        let prepared = self
            .store
            .sandbox_recover_attachment(actor, sandbox, request_id)?;
        let authorized = (|| {
            if prepared.observation().replaced().is_none() {
                return Err(Error::Denied("not a replacement"));
            }
            authorize(prepared.observation(), prepared.source())
        })();
        let fence = match authorized {
            Ok(fence) => fence,
            Err(error) => {
                prepared.mark_unknown()?;
                return Err(error);
            }
        };
        let result = (|| {
            prepared.revalidate()?;
            let attachment = prepared.observation();
            let (old, source) = attachment.replaced().expect("replacement checked");
            let driver = self.store.source_driver(self.driver)?;
            verify_attachment(&driver, attachment, prepared.source(), &fence)?;
            driver.continue_replacement(&source.prepared, &old.binding, prepared.source())?;
            Ok(())
        })();
        if let Err(error) = result {
            prepared.mark_unknown()?;
            return Err(error);
        }
        self.inspect_and_publish_attachment(prepared, &fence)
    }

    pub fn resume_sandbox_attachment<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        request_id: &str,
        authorize: impl FnOnce(&SandboxAttachment, &PreparedSourceOperation) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        self.resume_sandbox_attachment_in(actor, sandbox, request_id, None, authorize)
    }

    pub fn resume_sandbox_attachment_exact<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        request_id: &str,
        request: SandboxMaterializationRequest<'_>,
        authorize: impl FnOnce(&SandboxAttachment, &PreparedSourceOperation) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        self.resume_sandbox_attachment_in(actor, sandbox, request_id, Some(request), authorize)
    }

    fn resume_sandbox_attachment_in<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        request_id: &str,
        request: Option<SandboxMaterializationRequest<'_>>,
        authorize: impl FnOnce(&SandboxAttachment, &PreparedSourceOperation) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        let mut fence = None;
        let inspect = |registry: &aw_agent_core::SourceDriverRegistryReader,
                       attachment: &SandboxAttachment,
                       prepared: &PreparedSourceOperation| {
            fence = Some(authorize(attachment, prepared)?);
            let approval = fence.as_ref().expect("approved");
            let driver = registry.get(self.driver)?;
            verify_attachment(&driver, attachment, prepared, approval)?;
            driver.inspect_restore(prepared)
        };
        let inspected = match request {
            Some(request) => self
                .store
                .sandbox_inspect_attachment_exact(actor, sandbox, request_id, request, inspect),
            None => self
                .store
                .sandbox_inspect_attachment(actor, sandbox, request_id, inspect),
        }?;
        self.publish_attachment(
            inspected,
            &fence.expect("successful attachment inspection called inspector"),
        )
    }

    fn publish_attachment<F: GitRestoreFence>(
        &self,
        inspected: InspectedSandboxAttachment<'_>,
        fence: &F,
    ) -> Result<SandboxSnapshot> {
        inspected.publish(|registry, attachment, prepared, binding, evidence| {
            let driver = registry.get(self.driver)?;
            verify_attachment(&driver, attachment, prepared, fence)?;
            if let Some((old, source)) = attachment.replaced() {
                driver.revalidate_replacement(&source.prepared, &old.binding, prepared, binding)?;
            } else {
                driver.revalidate_restore(prepared, binding)?;
            }
            Ok(SourcePublicationProof::Held(Publication {
                _fence: fence,
                evidence: evidence.clone(),
            }))
        })
    }

    pub fn abandon_sandbox_attachment<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        request_id: &str,
        authorize: impl FnOnce(&SandboxAttachment, &PreparedSourceOperation) -> Result<F>,
    ) -> Result<()> {
        self.abandon_sandbox_attachment_in(actor, sandbox, request_id, None, authorize)
    }

    pub fn abandon_sandbox_attachment_exact<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        request_id: &str,
        request: SandboxMaterializationRequest<'_>,
        authorize: impl FnOnce(&SandboxAttachment, &PreparedSourceOperation) -> Result<F>,
    ) -> Result<()> {
        self.abandon_sandbox_attachment_in(actor, sandbox, request_id, Some(request), authorize)
    }

    fn abandon_sandbox_attachment_in<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        request_id: &str,
        request: Option<SandboxMaterializationRequest<'_>>,
        authorize: impl FnOnce(&SandboxAttachment, &PreparedSourceOperation) -> Result<F>,
    ) -> Result<()> {
        // Keep the actual guard even when verification/cleanup fails, until
        // the Store has factually settled Unknown or Rejected.
        let mut fence = None;
        let reconcile = |registry: &aw_agent_core::SourceDriverRegistryReader,
                         attachment: &SandboxAttachment,
                         prepared: &PreparedSourceOperation| {
            fence = Some(authorize(attachment, prepared)?);
            let approval = fence.as_ref().expect("approved");
            let driver = registry.get(self.driver)?;
            verify_attachment(&driver, attachment, prepared, approval)?;
            if let Some((old, source)) = attachment.replaced() {
                // Only an intact original with no new registration
                // may reject a replacement. Never lose the old row.
                driver.verify_replacement_untouched(&source.prepared, &old.binding, prepared)?;
            } else {
                driver.release_prepared_restore(prepared)?;
                attachment_location(&driver, attachment, approval)?;
            }
            Ok(())
        };
        match request {
            Some(request) => self
                .store
                .sandbox_abandon_attachment_exact(actor, sandbox, request_id, request, reconcile),
            None => self
                .store
                .sandbox_abandon_attachment(actor, sandbox, request_id, reconcile),
        }
    }

    /// Remove one original managed source from an unused open Sandbox.
    /// The independently protected named parent remains the same layout
    /// after removal, including final-source removal. The fence covers
    /// every retained source and its backing, never just a target path.
    pub fn release_sandbox_source<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        name: &SourceName,
        expected_rev: u64,
        authorize: impl FnOnce(&SandboxSourceRelease) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        let driver = self.store.source_driver(self.driver)?;
        self.store.sandbox_release_source(
            actor,
            sandbox,
            name,
            expected_rev,
            |release| {
                let fence = authorize(release)?;
                let snapshot = release.snapshot();
                let layout = verify_managed_materializations(
                    &driver,
                    snapshot,
                    release.bindings(),
                    release.preparations(),
                    &fence,
                )?;
                if !matches!(layout, ManagedGitLayout::Directory(_))
                    || snapshot.working_directory != snapshot.tool_root
                    || fence.locations().iter().any(|location| {
                        location.target != snapshot.tool_root.join(location.name.as_str())
                    })
                {
                    return Err(Error::Denied(
                        "source removal requires a private named-child layout with stable cwd",
                    ));
                }
                verify_remaining_sources(&driver, release, &fence)?;
                Ok(fence)
            },
            |release, fence| {
                let index = release.selected();
                driver.release_restored_materialization(
                    &release.preparations()[index].prepared,
                    &release.bindings()[index].binding,
                )?;
                let layout = ManagedGitLayout::check(release.snapshot(), fence.locations())?;
                verify_remaining_sources(&driver, release, fence)?;
                if release.bindings().len() == 1 {
                    layout.require_released()?;
                }
                Ok(())
            },
        )
    }

    /// Reconcile a closed, retained Sandbox created by a Git restore or
    /// standalone construction (baseline or derive). Only exact original
    /// retained bindings, including previously proven released subsets, are supported.
    /// Authorize the whole layout and exact Applied descriptors independently.
    /// Keep the external fence through Store settlement; never remove shared
    /// backing or unrelated files. Already released layouts do not reauthorize.
    pub fn reconcile_closed_sandbox<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        authorize: impl FnOnce(
            &SandboxSnapshot,
            &[VersionedSourceBinding],
            &[NamedPreparedSource],
        ) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        reconcile_closed_with(
            self.store,
            actor,
            sandbox,
            None,
            || self.store.source_driver(self.driver),
            authorize,
        )
    }
}

/// Exact closed Sandbox cleanup using the installed driver only for live
/// retained custody. No activation or source effect on settled replay.
pub fn reconcile_closed_git_sandbox<F: GitRestoreFence>(
    store: &SessionStore,
    actor: &PrincipalId,
    sandbox: &str,
    expected_rev: u64,
    authorize: impl FnOnce(
        &SandboxSnapshot,
        &[VersionedSourceBinding],
        &[NamedPreparedSource],
    ) -> Result<F>,
) -> Result<SandboxSnapshot> {
    reconcile_closed_with(
        store,
        actor,
        sandbox,
        Some(expected_rev),
        || {
            let handle = store.source_driver_handle::<GitSourceDriver>(
                &aw_agent_core::DriverKey::builtin("git")?,
            )?;
            store.source_driver(&handle)
        },
        authorize,
    )
}

fn reconcile_closed_with<F: GitRestoreFence>(
    store: &SessionStore,
    actor: &PrincipalId,
    sandbox: &str,
    expected_rev: Option<u64>,
    resolve: impl FnOnce() -> Result<std::sync::Arc<GitSourceDriver>>,
    authorize: impl FnOnce(
        &SandboxSnapshot,
        &[VersionedSourceBinding],
        &[NamedPreparedSource],
    ) -> Result<F>,
) -> Result<SandboxSnapshot> {
    store.sandbox_reconcile_closed_sources_fenced(
        actor,
        sandbox,
        expected_rev,
        |snapshot, bindings| {
            let prepared =
                store.sandbox_materialization_preparations(actor, &snapshot.canonical_ref)?;
            let fence = authorize(snapshot, bindings, &prepared)?;
            let driver = resolve()?;
            verify_managed_materializations(&driver, snapshot, bindings, &prepared, &fence)?;
            Ok((fence, driver, prepared))
        },
        |snapshot, bindings, (fence, driver, prepared)| {
            let layout = ManagedGitLayout::check(snapshot, fence.locations())?;
            for (binding, source) in bindings.iter().zip(prepared) {
                driver.release_restored_materialization(&source.prepared, &binding.binding)?;
            }
            layout.require_released()
        },
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use aw_agent_core::{
        CheckpointPolicy, SourceAccess, SourceDriverRegistry, SourceName, SourceSetLimit,
    };
    use aw_store::{AgentAdmission, AgentHistoryForkAccess, CheckpointCaptureSelection};
    use rusqlite::Connection;
    use std::{
        fs,
        os::unix::fs::{DirBuilderExt, MetadataExt},
        path::{Path, PathBuf},
        process::Command,
        sync::{RwLock, RwLockReadGuard},
        time::{SystemTime, UNIX_EPOCH},
    };

    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn private_dir(path: &Path) {
        fs::DirBuilder::new().mode(0o700).create(path).unwrap();
    }

    fn git(executable: &Path, root: &Path, args: &[&str]) -> Vec<u8> {
        let output = Command::new(executable)
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    fn layout(root: &Path) -> SessionBinding {
        SessionBinding {
            working_directory: root.to_owned(),
            tool_root: root.to_owned(),
            system_prompt: "fixture".into(),
            profile_hint: "fixture".into(),
            model_hint: "fixture".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    fn identity(path: &Path) -> (u64, u64) {
        let metadata = fs::metadata(path).unwrap();
        (metadata.dev(), metadata.ino())
    }

    fn count_agents(database: &Path) -> u64 {
        Connection::open(database)
            .unwrap()
            .query_row("SELECT count(*) FROM agent_state_current", [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    // Fixture authority is a real held read guard. All writers to this
    // isolated fixture are under the test's ownership, not a production
    // claim that a mutex excludes arbitrary filesystem writers.
    struct Fence<'a, L = GitRestoreLocation> {
        locations: Vec<L>,
        _guard: RwLockReadGuard<'a, ()>,
        authority: &'a RwLock<()>,
        database: &'a Path,
        expected_agents: u64,
        expected_retained: Option<u64>,
    }

    impl GitRestoreFence for Fence<'_> {
        fn locations(&self) -> &[GitRestoreLocation] {
            &self.locations
        }
    }

    struct ReplacementFence<'a> {
        retained: Fence<'a>,
        original: GitRestoreLocation,
    }

    impl GitRestoreFence for ReplacementFence<'_> {
        fn locations(&self) -> &[GitRestoreLocation] {
            self.retained.locations()
        }
        fn replaced_location(&self) -> Option<&GitRestoreLocation> {
            Some(&self.original)
        }
    }

    impl GitSourceFence for Fence<'_, aw_git::source_driver::GitSourceLocation> {
        fn locations(&self) -> &[aw_git::source_driver::GitSourceLocation] {
            &self.locations
        }
    }

    impl<L> Drop for Fence<'_, L> {
        fn drop(&mut self) {
            assert!(self.authority.try_write().is_err());
            assert_eq!(count_agents(self.database), self.expected_agents);
            if let Some(expected) = self.expected_retained {
                let count: u64 = Connection::open(self.database)
                    .unwrap()
                    .query_row("SELECT count(*) FROM checkpoint_sources", [], |row| {
                        row.get(0)
                    })
                    .unwrap();
                assert_eq!(count, expected);
            }
        }
    }

    #[test]
    fn git_restore_composes_prepared_custody_fenced_publication_and_exact_recovery() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp = Temp(std::env::temp_dir().join(format!(
            "aw-host-git-restore-{}-{nonce}",
            std::process::id()
        )));
        private_dir(&temp.0);
        let root = temp.0.canonicalize().unwrap();
        let source_root = root.join("sources");
        private_dir(&source_root);
        let executable = std::env::var_os("AW_GIT")
            .map(PathBuf::from)
            .or_else(|| option_env!("AW_DEFAULT_GIT").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/usr/bin/git"));
        let registry = SourceDriverRegistry::new();
        let driver = GitSourceDriver::new(&executable)
            .unwrap()
            .register(&registry)
            .unwrap();
        let limit = SourceSetLimit::new(2).unwrap();
        let state = root.join("state");
        let database = state.join("sessions.sqlite3");
        let store = SessionStore::open_with_sources(&state, registry.clone(), limit).unwrap();
        let git_driver = store.source_driver(&driver).unwrap();
        let actor = PrincipalId::new("internal:fixture:host-restore").unwrap();
        let (session, branch) = store
            .create(&actor, "initial", layout(&root.join("initial")))
            .unwrap();
        let reference: String = Connection::open(&database).unwrap().query_row(
            "SELECT '/d:' || domain_handle || '/i:' || identity_handle || '/p:' || project_handle || '/a:' || agent_handle FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
            [&session.id, &branch.id], |row| row.get(0),
        ).unwrap();
        let history = store
            .checkpoint_create_history_only(&actor, &reference, None, "fixture:history")
            .unwrap();
        let mut bindings = Vec::new();
        let mut original_heads = Vec::new();
        for name in ["code", "data"] {
            let repo = source_root.join(name);
            private_dir(&repo);
            git(&executable, &repo, &["init", "--initial-branch=main"]);
            git(&executable, &repo, &["config", "user.name", "Fixture"]);
            git(
                &executable,
                &repo,
                &["config", "user.email", "fixture@example.invalid"],
            );
            fs::write(repo.join("tracked"), b"base").unwrap();
            git(&executable, &repo, &["add", "tracked"]);
            git(
                &executable,
                &repo,
                &["commit", "--no-gpg-sign", "-m", "fixture"],
            );
            original_heads.push(git(&executable, &repo, &["rev-parse", "HEAD"]));
            fs::write(repo.join("tracked"), name.as_bytes()).unwrap();
            fs::write(repo.join("untracked"), b"dirty").unwrap();
            bindings.push(
                git_driver
                    .binding(
                        &repo,
                        SourceName::new(name).unwrap(),
                        SourceAccess::ReadWrite,
                        CheckpointPolicy::Required,
                    )
                    .unwrap(),
            );
        }
        let request = CheckpointRestoreRequest {
            checkpoint: history.id,
            project: None,
            title: None,
            access: AgentHistoryForkAccess::Private,
            task: None,
        };
        let history_request = request.clone();
        let empty_auto = store
            .checkpoint_create_empty_auto(&actor, &reference, None, "fixture:empty-auto")
            .unwrap();
        // Use the ordinary lower owners to create and capture the source
        // Agent. No synthetic binding or receipt insertion in the fixture.
        let source = store
            .checkpoint_restore_baseline(
                &actor,
                &request,
                &layout(&source_root),
                &bindings,
                "fixture:baseline",
                "local",
            )
            .unwrap();
        let authority = RwLock::new(());
        let capture_locations = || {
            ["code", "data"]
                .into_iter()
                .map(|name| {
                    let repo = source_root.join(name);
                    aw_git::source_driver::GitSourceLocation {
                        name: SourceName::new(name).unwrap(),
                        repository_identity: identity(&repo),
                        repository: repo,
                    }
                })
                .collect::<Vec<_>>()
        };
        let capture_fence = |locations, expected_agents, expected_retained| Fence {
            locations,
            _guard: authority.read().unwrap(),
            authority: &authority,
            database: &database,
            expected_agents,
            expected_retained: Some(expected_retained),
        };
        // The final metadata-only detach may use an independently
        // reserved empty layout, but never turn a real source checkout
        // or a shared/used layout into an ambient source-free root.
        use aw_git::source_driver::GitSourceFreeLayout;
        let unused_root = root.join("unused-layout");
        private_dir(&unused_root);
        let unused = store
            .sandbox_create_fresh_with_sources(
                &actor,
                &source.agent.project_ref,
                &layout(&unused_root),
                &bindings[..1],
            )
            .unwrap();
        let name = &bindings[0].name;
        let approved = GitSourceFreeLayout {
            root: unused_root.clone(),
            identity: identity(&unused_root),
        };
        let detach = |revision, layout_approval: GitSourceFreeLayout| {
            crate::source_git::detach_final_source(
                &store,
                &driver,
                &actor,
                &unused.canonical_ref,
                name,
                revision,
                |snapshot, binding| {
                    assert_eq!(snapshot.canonical_ref, unused.canonical_ref);
                    assert_eq!(&binding.binding, &bindings[0]);
                    assert_eq!(binding.binding_rev, snapshot.sources[0].binding_rev);
                    Ok((
                        capture_fence(vec![capture_locations().remove(0)], 2, 0),
                        layout_approval,
                    ))
                },
            )
        };
        let no_verify = |_: &SandboxSnapshot, _: &VersionedSourceBinding| -> Result<()> {
            panic!("Store refusal/replay must not inspect protected physical locations")
        };
        assert!(store
            .sandbox_sources_set(&actor, &unused.canonical_ref, name, None, unused.rev)
            .is_err());
        let set = aw_store::SourceMutationSet::from_bindings(&bindings[..1]).unwrap();
        let barrier = store
            .acquire_source_mutation_barrier(&set, "fixture:detach-active")
            .unwrap();
        assert!(store
            .sandbox_sources_detach_final(
                &actor,
                &unused.canonical_ref,
                name,
                unused.rev,
                no_verify
            )
            .is_err());
        barrier
            .settle(aw_store::SourceMutationOutcome::Settled)
            .unwrap();
        let shared = store
            .sandbox_create_fresh(&actor, &unused.project_ref, &layout(&unused_root))
            .unwrap();
        assert!(store
            .sandbox_sources_detach_final(
                &actor,
                &unused.canonical_ref,
                name,
                unused.rev,
                no_verify
            )
            .is_err());
        store
            .sandbox_close(&actor, &shared.canonical_ref, Some(shared.rev))
            .unwrap();
        assert!(store
            .sandbox_sources_detach_final(
                &actor,
                &unused.canonical_ref,
                name,
                unused.rev + 1,
                no_verify
            )
            .is_err());

        // The actual driver checks physical identity and refuses to
        // adopt unrelated content in the future source-free layout.
        let mut wrong = approved.clone();
        wrong.identity = identity(&source_root.join("code"));
        assert!(detach(unused.rev, wrong).is_err());
        fs::write(unused_root.join("protected"), b"keep").unwrap();
        assert!(detach(unused.rev, approved.clone()).is_err());
        assert_eq!(fs::read(unused_root.join("protected")).unwrap(), b"keep");
        fs::remove_file(unused_root.join("protected")).unwrap();
        assert_eq!(
            store.sandbox_get(&actor, &unused.canonical_ref).unwrap(),
            unused
        );

        let before_repo = git(
            &executable,
            &source_root.join("code"),
            &["rev-parse", "HEAD"],
        );
        let before_index = fs::read(source_root.join("code/.git/index")).unwrap();
        let detached = detach(unused.rev, approved.clone()).unwrap();
        assert!(detached.sources.is_empty());
        assert_eq!(detached.rev, unused.rev + 1);
        assert_eq!(detached.layout_digest, unused.layout_digest);
        assert_eq!(detached.tool_root, unused.tool_root);
        assert_eq!(detached.working_directory, unused.working_directory);
        assert_eq!(
            store
                .sandbox_sources_detach_final(
                    &actor,
                    &unused.canonical_ref,
                    name,
                    detached.rev,
                    no_verify
                )
                .unwrap(),
            detached
        );
        let reattached = store
            .sandbox_sources_set(
                &actor,
                &unused.canonical_ref,
                name,
                Some(&bindings[0]),
                detached.rev,
            )
            .unwrap();
        assert_eq!(reattached.sources[0].binding_rev, detached.rev + 1);
        let detached = detach(reattached.rev, approved).unwrap();
        assert_eq!(detached.rev, reattached.rev + 1);
        assert!(detached.sources.is_empty());
        assert_eq!(
            git(
                &executable,
                &source_root.join("code"),
                &["rev-parse", "HEAD"]
            ),
            before_repo
        );
        assert_eq!(
            fs::read(source_root.join("code/.git/index")).unwrap(),
            before_index
        );
        assert_eq!(fs::read(source_root.join("code/tracked")).unwrap(), b"code");
        assert_eq!(fs::read_dir(&unused_root).unwrap().count(), 0);
        store
            .sandbox_close(&actor, &unused.canonical_ref, Some(detached.rev))
            .unwrap();

        let capture_host = GitCheckpointCaptureHost::new(&store, &driver);
        let mut incomplete = capture_locations();
        incomplete.pop();
        assert!(capture_host
            .capture(
                &actor,
                &source.agent.canonical_ref,
                CheckpointCaptureSelection::Auto,
                None,
                "fixture:capture",
                |_| Ok(capture_fence(incomplete, 2, 0))
            )
            .is_err());
        assert!(store
            .checkpoint_replay_capture(
                &actor,
                &source.agent.canonical_ref,
                CheckpointCaptureSelection::Auto,
                None,
                "fixture:capture"
            )
            .unwrap()
            .is_none());

        let fail_capture = || {
            Connection::open(&database)
                .unwrap()
                .execute_batch(
                    "CREATE TRIGGER fixture_fail_capture BEFORE INSERT ON checkpoint_sources
                 BEGIN SELECT RAISE(ABORT,'fixture capture publication failure'); END;",
                )
                .unwrap()
        };
        let clear_capture_failure = || {
            Connection::open(&database)
                .unwrap()
                .execute_batch("DROP TRIGGER fixture_fail_capture;")
                .unwrap()
        };
        fail_capture();
        assert!(capture_host
            .capture(
                &actor,
                &source.agent.canonical_ref,
                CheckpointCaptureSelection::Auto,
                None,
                "fixture:capture",
                |_| Ok(capture_fence(capture_locations(), 2, 0))
            )
            .is_err());
        for name in ["code", "data"] {
            assert!(!git(
                &executable,
                &source_root.join(name),
                &["for-each-ref", "refs/aw/checkpoints/"]
            )
            .is_empty());
        }
        assert!(capture_host
            .capture::<Fence<'_, aw_git::source_driver::GitSourceLocation>>(
                &actor,
                &source.agent.canonical_ref,
                CheckpointCaptureSelection::Auto,
                None,
                "fixture:capture",
                |_| panic!("unresolved capture authorized again")
            )
            .is_err());
        drop(store);
        clear_capture_failure();
        let store = SessionStore::open_with_sources(&state, registry.clone(), limit).unwrap();
        let capture_host = GitCheckpointCaptureHost::new(&store, &driver);
        let mut wrong = capture_locations();
        wrong[0].repository = wrong[1].repository.clone();
        wrong[0].repository_identity = wrong[1].repository_identity;
        assert!(capture_host
            .resume(&actor, "fixture:capture", |_, _| Ok(capture_fence(
                wrong, 2, 0
            )))
            .is_err());
        let checkpoint = capture_host
            .resume(&actor, "fixture:capture", |_, _| {
                Ok(capture_fence(capture_locations(), 2, 2))
            })
            .unwrap();
        assert_eq!(
            capture_host
                .capture::<Fence<'_, aw_git::source_driver::GitSourceLocation>>(
                    &actor,
                    &source.agent.canonical_ref,
                    CheckpointCaptureSelection::Auto,
                    None,
                    "fixture:capture",
                    |_| panic!("Applied capture authorized again")
                )
                .unwrap(),
            checkpoint
        );

        // An independent abandoned capture must not release the published refs.
        let before = git(
            &executable,
            &source_root.join("code"),
            &["for-each-ref", "refs/aw/checkpoints/"],
        );
        fail_capture();
        assert!(capture_host
            .capture(
                &actor,
                &source.agent.canonical_ref,
                CheckpointCaptureSelection::Auto,
                None,
                "fixture:abandon-capture",
                |_| Ok(capture_fence(capture_locations(), 2, 2))
            )
            .is_err());
        clear_capture_failure();
        capture_host
            .abandon(&actor, "fixture:abandon-capture", |_, _| {
                Ok(capture_fence(capture_locations(), 2, 2))
            })
            .unwrap();
        assert_eq!(
            git(
                &executable,
                &source_root.join("code"),
                &["for-each-ref", "refs/aw/checkpoints/"]
            ),
            before
        );
        capture_host
            .abandon::<Fence<'_, aw_git::source_driver::GitSourceLocation>>(
                &actor,
                "fixture:abandon-capture",
                |_, _| panic!("terminal capture cleanup authorized again"),
            )
            .unwrap();
        let request = CheckpointRestoreRequest {
            checkpoint: checkpoint.id,
            task: Some("restore fixture".into()),
            ..request
        };
        let locations = |destination: &Path| {
            ["code", "data"]
                .into_iter()
                .map(|name| {
                    let repo = source_root.join(name);
                    GitRestoreLocation {
                        name: SourceName::new(name).unwrap(),
                        repository_identity: identity(&repo),
                        repository: repo,
                        target: destination.join(name),
                        parent_identity: identity(destination),
                    }
                })
                .collect::<Vec<_>>()
        };
        let fence = |locations, expected_agents| Fence {
            locations,
            _guard: authority.read().unwrap(),
            authority: &authority,
            database: &database,
            expected_agents,
            expected_retained: None,
        };
        let destination = root.join("restored");
        private_dir(&destination);
        let host = GitCheckpointRestoreHost::new(&store, &driver);
        let mut incomplete = locations(&destination);
        incomplete.pop();
        // Fixture policy selects these independently retained versions via
        // the real authorized restore observation, never fabricated anchors.
        let mut baseline = None;
        assert!(host
            .restore(
                &actor,
                &request,
                "fixture:restore",
                "local",
                || Ok(layout(&destination)),
                |_, set| {
                    baseline = Some(SourceSet {
                        coverage: aw_agent_core::SourceCoverage::Selected,
                        anchors: set.anchors.clone(),
                    });
                    Ok(fence(incomplete, 2))
                }
            )
            .is_err());
        let baseline = baseline.expect("authorized retained source observation");
        assert!(store
            .checkpoint_replay_restore(&actor, &request, "fixture:restore")
            .unwrap()
            .is_none());
        assert!(!destination.join("code").exists());

        // Abort the real final Store transaction after both effects.
        let fail_publication = || {
            Connection::open(&database)
                .unwrap()
                .execute_batch(
                    "CREATE TRIGGER fixture_fail_restore BEFORE INSERT ON agent_state_current
             BEGIN SELECT RAISE(ABORT,'fixture publication failure'); END;",
                )
                .unwrap()
        };
        let clear_failure = || {
            Connection::open(&database)
                .unwrap()
                .execute_batch("DROP TRIGGER fixture_fail_restore;")
                .unwrap()
        };
        fail_publication();
        assert!(host
            .restore(
                &actor,
                &request,
                "fixture:restore",
                "local",
                || Ok(layout(&destination)),
                |_, _| Ok(fence(locations(&destination), 2))
            )
            .is_err());
        for name in ["code", "data"] {
            assert_eq!(
                fs::read(destination.join(name).join("tracked")).unwrap(),
                name.as_bytes()
            );
            assert_eq!(
                fs::read(destination.join(name).join("untracked")).unwrap(),
                b"dirty"
            );
        }
        assert!(host
            .restore::<Fence<'_>>(
                &actor,
                &request,
                "fixture:restore",
                "local",
                || panic!("unresolved replay selected layout"),
                |_, _| panic!("unresolved replay authorized")
            )
            .is_err());
        drop(store);
        clear_failure();
        let store = SessionStore::open_with_sources(&state, registry.clone(), limit).unwrap();
        let host = GitCheckpointRestoreHost::new(&store, &driver);
        let other = root.join("wrong");
        private_dir(&other);
        assert!(host
            .resume(
                &actor,
                "fixture:restore",
                &layout(&destination),
                |_, _, _| Ok(fence(locations(&other), 2))
            )
            .is_err());
        assert!(!other.join("code").exists());
        let restored = host
            .resume(
                &actor,
                "fixture:restore",
                &layout(&destination),
                |_, _, _| Ok(fence(locations(&destination), 3)),
            )
            .unwrap();
        assert_eq!(restored.agent.admission, AgentAdmission::Held);
        assert_eq!(
            host.restore::<Fence<'_>>(
                &actor,
                &request,
                "fixture:restore",
                "local",
                || panic!("applied replay selected layout"),
                |_, _| panic!("applied replay authorized")
            )
            .unwrap(),
            restored
        );
        assert!(host
            .abandon::<Fence<'_>>(&actor, "fixture:restore", |_, _| panic!(
                "applied result cleanup authorized"
            ))
            .is_err());

        let abandoned = root.join("abandoned");
        private_dir(&abandoned);
        fail_publication();
        assert!(host
            .restore(
                &actor,
                &request,
                "fixture:abandon",
                "local",
                || Ok(layout(&abandoned)),
                |_, _| Ok(fence(locations(&abandoned), 3))
            )
            .is_err());
        clear_failure();
        assert!(abandoned.join("data").exists());
        host.abandon(&actor, "fixture:abandon", |_, _| {
            Ok(fence(locations(&abandoned), 3))
        })
        .unwrap();
        assert!(!abandoned.join("code").exists());
        assert!(!abandoned.join("data").exists());
        host.abandon::<Fence<'_>>(&actor, "fixture:abandon", |_, _| {
            panic!("terminal abandonment authorized again")
        })
        .unwrap();

        // A fresh baseline is independent of the checkpoint's historical
        // sources. Required current baseline versions are valid for None,
        // not empty Auto; captured checkpoints may never substitute them.
        let baseline_root = root.join("versioned-baseline");
        private_dir(&baseline_root);
        let auto_request = CheckpointRestoreRequest {
            checkpoint: empty_auto.id,
            ..history_request.clone()
        };
        for invalid in [&request, &auto_request] {
            assert!(host
                .restore_versioned_baseline::<Fence<'_>>(
                    &actor,
                    invalid,
                    "fixture:invalid-baseline",
                    "local",
                    || Ok((layout(&baseline_root), baseline.clone())),
                    |_, _| panic!("invalid baseline reached driver authorization")
                )
                .is_err());
        }
        assert!(!baseline_root.join("code").exists());

        let baseline_request = CheckpointRestoreRequest {
            task: Some("baseline initial task".into()),
            ..history_request
        };
        fail_publication();
        assert!(host
            .restore_versioned_baseline(
                &actor,
                &baseline_request,
                "fixture:versioned-baseline",
                "local",
                || Ok((layout(&baseline_root), baseline.clone())),
                |_, _| Ok(fence(locations(&baseline_root), 3)),
            )
            .is_err());
        assert_eq!(
            fs::read(baseline_root.join("code/tracked")).unwrap(),
            b"code"
        );
        assert!(store
            .checkpoint_delete(&actor, &baseline_request.checkpoint)
            .is_err());
        assert!(host
            .restore_versioned_baseline::<Fence<'_>>(
                &actor,
                &baseline_request,
                "fixture:versioned-baseline",
                "local",
                || panic!("unresolved baseline selected another version"),
                |_, _| panic!("unresolved baseline authorized again")
            )
            .is_err());
        drop(store);
        clear_failure();
        let missing = SessionStore::open(&state).unwrap();
        assert!(GitCheckpointRestoreHost::new(&missing, &driver)
            .resume::<Fence<'_>>(
                &actor,
                "fixture:versioned-baseline",
                &layout(&baseline_root),
                |_, _, _| panic!("missing driver authorized baseline recovery")
            )
            .is_err());
        drop(missing);
        let store = SessionStore::open_with_sources(&state, registry.clone(), limit).unwrap();
        let host = GitCheckpointRestoreHost::new(&store, &driver);
        let versioned = host
            .resume(
                &actor,
                "fixture:versioned-baseline",
                &layout(&baseline_root),
                |_, sources, _| {
                    assert_eq!(sources, &baseline);
                    Ok(fence(locations(&baseline_root), 4))
                },
            )
            .unwrap();
        assert_eq!(
            versioned.source_coverage,
            aw_agent_core::SourceCoverage::None
        );
        assert_eq!(versioned.agent.admission, AgentAdmission::Held);
        assert!(store
            .checkpoint_get_metadata(&actor, &baseline_request.checkpoint)
            .unwrap()
            .sources
            .is_empty());
        assert_eq!(
            host.restore_versioned_baseline::<Fence<'_>>(
                &actor,
                &baseline_request,
                "fixture:versioned-baseline",
                "local",
                || panic!("Applied baseline selected new versions"),
                |_, _| panic!("Applied baseline authorized again")
            )
            .unwrap(),
            versioned
        );

        // Standalone versioned baseline uses the same Prepared materialization
        // path but an actual Project origin, with no new Agent or Session.
        let standalone_root = root.join("standalone-baseline");
        private_dir(&standalone_root);
        let project = versioned.agent.project_ref.clone();
        let request_id = "fixture:standalone-baseline";
        let count_sessions = || -> u64 {
            Connection::open(&database)
                .unwrap()
                .query_row("SELECT count(*) FROM sessions", [], |r| r.get(0))
                .unwrap()
        };
        let sessions_before = count_sessions();
        Connection::open(&database)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fixture_fail_sandbox BEFORE INSERT ON sandboxes_current
             BEGIN SELECT RAISE(ABORT,'fixture standalone publish failure'); END;",
            )
            .unwrap();
        assert!(host
            .create_sandbox_from_baseline(
                &actor,
                &project,
                request_id,
                || Ok((layout(&standalone_root), baseline.clone())),
                |_, _| Ok(fence(locations(&standalone_root), 4)),
            )
            .is_err());
        assert_eq!(count_sessions(), sessions_before);
        assert_eq!(
            fs::read(standalone_root.join("code/tracked")).unwrap(),
            b"code"
        );
        let (candidate, origin): (String, String) = Connection::open(&database)
            .unwrap()
            .query_row(
                "SELECT json_extract(data_json,'$.candidate.sandbox'), project_ref FROM audit
             WHERE action='construction.preparation' AND session_id IS NULL
               AND json_extract(data_json,'$.operation')='sandbox_create'
               AND json_extract(data_json,'$.state')='prepared'
               AND json_extract(data_json,'$.request_id')=?1",
                [request_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(origin, project);
        assert!(store.sandbox_get(&actor, &candidate).is_err());
        assert!(host
            .create_sandbox_from_baseline::<Fence<'_>>(
                &actor,
                &project,
                request_id,
                || panic!("unresolved Sandbox cannot reselect versions"),
                |_, _| panic!("unresolved Sandbox cannot reauthorize a new effect"),
            )
            .is_err());
        drop(store);
        Connection::open(&database)
            .unwrap()
            .execute_batch("DROP TRIGGER fixture_fail_sandbox;")
            .unwrap();
        let missing = SessionStore::open(&state).unwrap();
        assert!(GitCheckpointRestoreHost::new(&missing, &driver)
            .resume_sandbox_create::<Fence<'_>>(
                &actor,
                &project,
                None,
                request_id,
                &layout(&standalone_root),
                |_, _, _| panic!("missing driver cannot inspect original Project custody"),
            )
            .is_err());
        drop(missing);
        let store = SessionStore::open_with_sources(&state, registry.clone(), limit).unwrap();
        let host = GitCheckpointRestoreHost::new(&store, &driver);
        let standalone = host
            .resume_sandbox_create(
                &actor,
                &project,
                None,
                request_id,
                &layout(&standalone_root),
                |_, sources, _| {
                    let SandboxCreateSources::Baseline(sources) = sources else {
                        panic!("baseline selection changed")
                    };
                    assert_eq!(sources, &baseline);
                    Ok(fence(locations(&standalone_root), 4))
                },
            )
            .unwrap();
        assert_eq!(standalone.canonical_ref, candidate);
        assert_eq!(standalone.sources.len(), 2);
        assert_eq!(standalone.rev, 1);
        assert_eq!(count_sessions(), sessions_before);
        assert_eq!(count_agents(&database), 4);
        assert_eq!(
            host.create_sandbox_from_baseline::<Fence<'_>>(
                &actor,
                &project,
                request_id,
                || panic!("Applied Sandbox cannot reselect versions"),
                |_, _| panic!("Applied Sandbox cannot reauthorize materialization"),
            )
            .unwrap(),
            standalone
        );
        assert!(host
            .abandon_sandbox_create::<Fence<'_>>(&actor, &project, None, request_id, |_, _| panic!(
                "Applied Sandbox is not a private candidate"
            ),)
            .is_err());
        // Derive the actual mutable parent state, not its historical
        // baseline anchor. The linked parent checkout is also retained
        // as a physical dependency of the child.
        let child_root = root.join("derived-standalone");
        private_dir(&child_root);
        let child_locations = |destination: &Path| {
            ["code", "data"]
                .into_iter()
                .map(|name| {
                    let repository = standalone_root.join(name);
                    GitRestoreLocation {
                        name: SourceName::new(name).unwrap(),
                        repository_identity: identity(&repository),
                        repository,
                        target: destination.join(name),
                        parent_identity: identity(destination),
                    }
                })
                .collect::<Vec<_>>()
        };
        let parent_ref = &standalone.canonical_ref;
        let parent_code = standalone_root.join("code");
        fs::write(parent_code.join("tracked"), b"parent changed").unwrap();
        fs::write(
            parent_code.join("new-child-input"),
            b"untracked parent content",
        )
        .unwrap();
        let parent_head = git(&executable, &parent_code, &["rev-parse", "HEAD"]);
        let parent_index = git(&executable, &parent_code, &["ls-files", "--stage"]);
        let checkpoint_count = || -> u64 {
            Connection::open(&database)
                .unwrap()
                .query_row("SELECT count(*) FROM checkpoints", [], |row| row.get(0))
                .unwrap()
        };
        let checkpoints_before = checkpoint_count();
        let derived_request = "fixture:derive-standalone";
        assert!(host
            .derive_sandbox::<Fence<'_>>(
                &actor,
                &project,
                parent_ref,
                request_id,
                || panic!("baseline request cannot be reused as derive"),
                |_, _| panic!("baseline request cannot authorize derive"),
            )
            .is_err());

        // A first source may have completed while another never started.
        // Abandon exactly those private resources, without touching the
        // parent or a public checkpoint.
        let partial_root = root.join("abandoned-derive");
        private_dir(&partial_root);
        let approval = fence(child_locations(&partial_root), 4);
        let prepared = store
            .sandbox_prepare_create(
                &actor,
                &project,
                &layout(&partial_root),
                SandboxCreateFrom::Parent(parent_ref),
                "fixture:abandoned-derive",
                |prepare_id, _, input| {
                    let implementation = store.source_driver(&driver)?;
                    plan(&implementation, input, &approval, prepare_id)
                },
            )
            .unwrap();
        let partial_ref = prepared.sandbox_ref().to_owned();
        let implementation = store.source_driver(&driver).unwrap();
        implementation
            .execute_derive(&prepared.sources().sources[0].prepared)
            .unwrap();
        assert!(partial_root.join("code/tracked").is_file());
        assert!(!partial_root.join("data").exists());
        prepared.mark_unknown().unwrap();
        drop(approval);
        host.abandon_sandbox_create(
            &actor,
            &project,
            Some(parent_ref),
            "fixture:abandoned-derive",
            |_, _| Ok(fence(child_locations(&partial_root), 4)),
        )
        .unwrap();
        assert!(store.sandbox_get(&actor, &partial_ref).is_err());
        assert_eq!(fs::read_dir(&partial_root).unwrap().count(), 0);

        Connection::open(&database)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fixture_fail_derive BEFORE INSERT ON sandboxes_current
             BEGIN SELECT RAISE(ABORT,'fixture derived publish failure'); END;",
            )
            .unwrap();
        assert!(host
            .derive_sandbox(
                &actor,
                &project,
                parent_ref,
                derived_request,
                || Ok(layout(&child_root)),
                |_, sources| {
                    assert_eq!(sources.len(), 2);
                    Ok(fence(child_locations(&child_root), 4))
                },
            )
            .is_err());
        assert_eq!(
            fs::read(child_root.join("code/tracked")).unwrap(),
            b"parent changed"
        );
        assert_eq!(
            fs::read(child_root.join("code/new-child-input")).unwrap(),
            b"untracked parent content"
        );
        assert_eq!(
            git(&executable, &parent_code, &["rev-parse", "HEAD"]),
            parent_head
        );
        assert_eq!(
            git(&executable, &parent_code, &["ls-files", "--stage"]),
            parent_index
        );
        assert_eq!(
            fs::read(parent_code.join("tracked")).unwrap(),
            b"parent changed"
        );
        assert!(store
            .sandbox_close(&actor, parent_ref, Some(standalone.rev))
            .is_err());
        assert!(host
            .derive_sandbox::<Fence<'_>>(
                &actor,
                &project,
                parent_ref,
                derived_request,
                || panic!("unresolved derive cannot select another layout"),
                |_, _| panic!("unresolved derive cannot authorize a new effect"),
            )
            .is_err());
        drop(store);
        Connection::open(&database)
            .unwrap()
            .execute_batch("DROP TRIGGER fixture_fail_derive;")
            .unwrap();
        let store = SessionStore::open_with_sources(&state, registry.clone(), limit).unwrap();
        let host = GitCheckpointRestoreHost::new(&store, &driver);
        let child = host.resume_sandbox_create(
            &actor, &project, Some(parent_ref), derived_request, &layout(&child_root),
            |_, input, prepared| {
                assert!(matches!(input, SandboxCreateSources::Parent(bindings) if bindings.len() == 2));
                assert_eq!(prepared.operation, SourceOperationKind::Derive);
                Ok(fence(child_locations(&child_root), 4))
            },
        ).unwrap();
        assert_eq!(
            child.parent_sandbox_ref.as_deref(),
            Some(parent_ref.as_str())
        );
        assert_eq!(child.sources.len(), 2);
        assert_eq!(count_sessions(), sessions_before);
        assert_eq!(checkpoint_count(), checkpoints_before);
        assert_eq!(
            host.derive_sandbox::<Fence<'_>>(
                &actor,
                &project,
                parent_ref,
                derived_request,
                || panic!("Applied child cannot select another layout"),
                |_, _| panic!("Applied child cannot authorize materialization again"),
            )
            .unwrap(),
            child
        );
        // Parent content can move on without changing the isolated child.
        fs::write(parent_code.join("tracked"), b"code").unwrap();
        fs::remove_file(parent_code.join("new-child-input")).unwrap();
        assert_eq!(
            fs::read(child_root.join("code/tracked")).unwrap(),
            b"parent changed"
        );
        assert!(store
            .sandbox_sources_set(
                &actor,
                parent_ref,
                &standalone.sources[0].name,
                None,
                standalone.rev
            )
            .is_err());
        // Remove one managed source under original construction custody.
        // Retain the entire old set across a lost final Store publication.
        let removed_name = child.sources[0].name.clone();
        assert_eq!(removed_name.as_str(), "code");
        assert!(host
            .release_sandbox_source::<Fence<'_>>(
                &actor,
                parent_ref,
                &standalone.sources[0].name,
                standalone.rev,
                |_| panic!("retained child must fence parent source removal"),
            )
            .is_err());
        assert!(store
            .sandbox_sources_set(&actor, &child.canonical_ref, &removed_name, None, child.rev,)
            .is_err());
        let selected_retention = format!(
            "refs/aw/checkpoints/{}/code",
            child.canonical_ref.rsplit_once("/s:").unwrap().1
        );
        assert!(!git(
            &executable,
            &parent_code,
            &["for-each-ref", &selected_retention]
        )
        .is_empty());
        let sibling = child_root.join("data/tracked");
        fs::write(&sibling, b"independent sibling edits").unwrap();
        Connection::open(&database).unwrap().execute_batch(
            "CREATE TRIGGER fixture_fail_source_release BEFORE UPDATE OF rev ON sandboxes_current
             BEGIN SELECT RAISE(ABORT,'fixture source release publication failure'); END;",
        ).unwrap();
        assert!(host
            .release_sandbox_source(
                &actor,
                &child.canonical_ref,
                &removed_name,
                child.rev,
                |release| {
                    assert_eq!(release.snapshot(), &child);
                    assert_eq!(release.bindings().len(), 2);
                    assert_eq!(
                        release.preparations()[release.selected()]
                            .prepared
                            .operation,
                        SourceOperationKind::Derive
                    );
                    Ok(fence(child_locations(&child_root), 4))
                },
            )
            .is_err());
        assert!(!child_root.join("code").exists());
        assert!(git(
            &executable,
            &parent_code,
            &["for-each-ref", &selected_retention]
        )
        .is_empty());
        assert_eq!(fs::read(&sibling).unwrap(), b"independent sibling edits");
        assert_eq!(
            store.sandbox_get(&actor, &child.canonical_ref).unwrap(),
            child
        );
        assert!(store
            .sandbox_close(&actor, &child.canonical_ref, Some(child.rev))
            .is_err());
        drop(store);
        Connection::open(&database)
            .unwrap()
            .execute_batch("DROP TRIGGER fixture_fail_source_release;")
            .unwrap();
        let store = SessionStore::open_with_sources(&state, registry.clone(), limit).unwrap();
        let host = GitCheckpointRestoreHost::new(&store, &driver);
        // A different source cannot recover the unresolved original claim.
        assert!(store
            .sandbox_release_source(
                &actor,
                &child.canonical_ref,
                &child.sources[1].name,
                child.rev,
                |_| Ok(()),
                |_, _| panic!("different source must not recover original release"),
            )
            .is_err());
        let reduced = host
            .release_sandbox_source(
                &actor,
                &child.canonical_ref,
                &removed_name,
                child.rev,
                |_| Ok(fence(child_locations(&child_root), 4)),
            )
            .unwrap();
        assert_eq!(reduced.rev, child.rev + 1);
        assert_eq!(reduced.sources, child.sources[1..]);
        assert_eq!(reduced.layout_digest, child.layout_digest);
        assert_eq!(fs::read(&sibling).unwrap(), b"independent sibling edits");
        assert!(host
            .release_sandbox_source::<Fence<'_>>(
                &actor,
                &child.canonical_ref,
                &removed_name,
                child.rev,
                |_| panic!("stale release must not inspect physical locations"),
            )
            .is_err());
        assert_eq!(
            store
                .sandbox_sources_set(
                    &actor,
                    &child.canonical_ref,
                    &removed_name,
                    None,
                    reduced.rev,
                )
                .unwrap(),
            reduced
        );
        // Reattach the released name from an independently retained
        // version. Its descriptor must not follow the old derived name.
        let attachment_anchor = baseline.anchors[0].clone();
        let attachment_locations = || {
            locations(&child_root)
                .into_iter()
                .take(1)
                .chain(child_locations(&child_root).into_iter().skip(1))
                .collect::<Vec<_>>()
        };
        let attachment_request = "fixture:source-attachment";
        assert!(host
            .attach_sandbox_source::<Fence<'_>>(
                &actor,
                &reduced.canonical_ref,
                reduced.rev,
                attachment_request,
                baseline.anchors[1].clone(),
                |_| panic!("occupied source name must not plan"),
            )
            .is_err());

        // An unpublished candidate can be abandoned independently of
        // both the original derived source and the retained sibling.
        let approval = fence(attachment_locations(), 4);
        let prepared = store
            .sandbox_prepare_attachment(
                &actor,
                &reduced.canonical_ref,
                reduced.rev,
                "fixture:abandoned-attachment",
                attachment_anchor.clone(),
                |attachment, prepare_id| {
                    let implementation = store.source_driver(&driver)?;
                    let location = attachment_location(&implementation, attachment, &approval)?;
                    implementation.plan_restore_approved(
                        location,
                        &GitSourceAnchor::try_from(attachment.anchor())?,
                        prepare_id,
                    )
                },
            )
            .unwrap();
        implementation.execute_restore(prepared.source()).unwrap();
        prepared.mark_unknown().unwrap();
        drop(approval);
        host.abandon_sandbox_attachment(
            &actor,
            &reduced.canonical_ref,
            "fixture:abandoned-attachment",
            |_, _| Ok(fence(attachment_locations(), 4)),
        )
        .unwrap();
        host.abandon_sandbox_attachment::<Fence<'_>>(
            &actor,
            &reduced.canonical_ref,
            "fixture:abandoned-attachment",
            |_, _| panic!("Rejected attachment must not inspect again"),
        )
        .unwrap();
        assert!(!child_root.join("code").exists());
        assert_eq!(fs::read(&sibling).unwrap(), b"independent sibling edits");

        Connection::open(&database)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fixture_fail_attachment BEFORE UPDATE OF rev ON sandboxes_current
             BEGIN SELECT RAISE(ABORT,'fixture attachment publication failure'); END;",
            )
            .unwrap();
        assert!(host
            .attach_sandbox_source(
                &actor,
                &reduced.canonical_ref,
                reduced.rev,
                attachment_request,
                attachment_anchor.clone(),
                |attachment| {
                    assert_eq!(attachment.snapshot(), &reduced);
                    assert_eq!(attachment.bindings().len(), 1);
                    Ok(fence(attachment_locations(), 4))
                },
            )
            .is_err());
        assert_eq!(fs::read(child_root.join("code/tracked")).unwrap(), b"code");
        assert_eq!(
            store.sandbox_get(&actor, &reduced.canonical_ref).unwrap(),
            reduced
        );
        assert_eq!(fs::read(&sibling).unwrap(), b"independent sibling edits");
        assert!(store
            .sandbox_close(&actor, &reduced.canonical_ref, Some(reduced.rev))
            .is_err());
        assert!(host
            .attach_sandbox_source::<Fence<'_>>(
                &actor,
                &reduced.canonical_ref,
                reduced.rev,
                attachment_request,
                attachment_anchor.clone(),
                |_| panic!("unresolved attachment cannot plan again"),
            )
            .is_err());
        drop(store);
        Connection::open(&database)
            .unwrap()
            .execute_batch("DROP TRIGGER fixture_fail_attachment;")
            .unwrap();
        let missing = SessionStore::open(&state).unwrap();
        assert!(GitCheckpointRestoreHost::new(&missing, &driver)
            .resume_sandbox_attachment::<Fence<'_>>(
                &actor,
                &reduced.canonical_ref,
                attachment_request,
                |_, _| panic!("missing driver must not inspect attachment"),
            )
            .is_err());
        drop(missing);
        let store = SessionStore::open_with_sources(&state, registry.clone(), limit).unwrap();
        let host = GitCheckpointRestoreHost::new(&store, &driver);
        let child = host
            .resume_sandbox_attachment(
                &actor,
                &reduced.canonical_ref,
                attachment_request,
                |attachment, prepared| {
                    assert_eq!(attachment.anchor(), &attachment_anchor);
                    assert_eq!(prepared.operation, SourceOperationKind::Restore);
                    Ok(fence(attachment_locations(), 4))
                },
            )
            .unwrap();
        assert_eq!(child.rev, reduced.rev + 1);
        assert_eq!(child.sources.len(), 2);
        assert_eq!(child.sources[0].binding_rev, child.rev);
        assert_eq!(child.sources[1], reduced.sources[0]);
        assert_eq!(child.layout_digest, reduced.layout_digest);
        assert_eq!(
            host.attach_sandbox_source::<Fence<'_>>(
                &actor,
                &child.canonical_ref,
                reduced.rev,
                attachment_request,
                attachment_anchor.clone(),
                |_| panic!("Applied attachment must not authorize materialization again"),
            )
            .unwrap(),
            child
        );
        assert!(store
            .sandbox_sources_set(
                &actor,
                &child.canonical_ref,
                &attachment_anchor.name,
                None,
                child.rev
            )
            .is_err());
        assert_eq!(fs::read(&sibling).unwrap(), b"independent sibling edits");
        // Replacement at the exact configured two-name limit. A third
        // backing identity must join the same old+incoming source claim.
        let replacement_repo = source_root.join("replacement");
        private_dir(&replacement_repo);
        git(
            &executable,
            &replacement_repo,
            &["init", "--initial-branch=main"],
        );
        git(
            &executable,
            &replacement_repo,
            &["config", "user.name", "Fixture"],
        );
        git(
            &executable,
            &replacement_repo,
            &["config", "user.email", "fixture@example.invalid"],
        );
        fs::write(replacement_repo.join("tracked"), b"replacement version").unwrap();
        git(&executable, &replacement_repo, &["add", "tracked"]);
        git(
            &executable,
            &replacement_repo,
            &["commit", "--no-gpg-sign", "-m", "fixture"],
        );
        let implementation = store.source_driver(&driver).unwrap();
        let replacement_binding = implementation
            .binding(
                &replacement_repo,
                SourceName::new("data").unwrap(),
                SourceAccess::ReadWrite,
                CheckpointPolicy::Required,
            )
            .unwrap();
        let replacement_capture = implementation
            .plan_capture(
                &replacement_binding,
                "cp_fixture_replacement",
                "op_fixture_replacement",
            )
            .unwrap();
        let replacement_anchor = SourceAnchor::try_from(
            &implementation
                .execute_capture(&replacement_capture)
                .unwrap(),
        )
        .unwrap();
        let replacement_locations = || {
            let mut locations = attachment_locations();
            locations[1].repository = replacement_repo.clone();
            locations[1].repository_identity = identity(&replacement_repo);
            locations
        };
        let replacement_fence = || ReplacementFence {
            retained: fence(replacement_locations(), 4),
            original: child_locations(&child_root).pop().unwrap(),
        };
        let replacement_request = "fixture:replacement";
        // Refuse to discard post-publication edits in the old selected
        // checkout. No new claim or deletion before exact read-only plan.
        assert!(host
            .replace_sandbox_source(
                &actor,
                &child.canonical_ref,
                child.rev,
                replacement_request,
                replacement_anchor.clone(),
                |_| Ok(replacement_fence()),
            )
            .is_err());
        assert_eq!(fs::read(&sibling).unwrap(), b"independent sibling edits");
        assert_eq!(
            store.sandbox_get(&actor, &child.canonical_ref).unwrap(),
            child
        );
        fs::write(&sibling, b"data").unwrap();
        let unaffected = child_root.join("code/tracked");
        fs::write(&unaffected, b"independent retained code").unwrap();

        // Untouched replacement can be rejected without releasing the
        // original published materialization.
        let approval = replacement_fence();
        let prepared = store
            .sandbox_prepare_replacement(
                &actor,
                &child.canonical_ref,
                child.rev,
                "fixture:abandoned-replacement",
                replacement_anchor.clone(),
                |attachment, prepare_id| {
                    plan_attachment(&implementation, attachment, &approval, prepare_id)
                },
            )
            .unwrap();
        prepared.mark_unknown().unwrap();
        drop(approval);
        host.abandon_sandbox_attachment(
            &actor,
            &child.canonical_ref,
            "fixture:abandoned-replacement",
            |_, _| Ok(replacement_fence()),
        )
        .unwrap();
        assert_eq!(fs::read(&sibling).unwrap(), b"data");

        // Stop between old release and the first incoming materialization.
        // The original binding/rev and all three identities remain fenced.
        let approval = replacement_fence();
        let prepared = store
            .sandbox_prepare_replacement(
                &actor,
                &child.canonical_ref,
                child.rev,
                replacement_request,
                replacement_anchor.clone(),
                |attachment, prepare_id| {
                    plan_attachment(&implementation, attachment, &approval, prepare_id)
                },
            )
            .unwrap();
        let (old, descriptor) = prepared.observation().replaced().unwrap();
        assert_eq!(descriptor.prepared.operation, SourceOperationKind::Derive);
        implementation
            .verify_replacement_untouched(&descriptor.prepared, &old.binding, prepared.source())
            .unwrap();
        implementation
            .release_restored_materialization(&descriptor.prepared, &old.binding)
            .unwrap();
        assert!(store
            .acquire_source_mutation_barrier(
                &aw_store::SourceMutationSet::from_bindings(std::slice::from_ref(
                    &replacement_binding
                ))
                .unwrap(),
                "fixture:incoming-replacement",
            )
            .is_err());
        prepared.mark_unknown().unwrap();
        drop(approval);
        assert!(!child_root.join("data").exists());
        assert_eq!(
            store.sandbox_get(&actor, &child.canonical_ref).unwrap(),
            child
        );
        assert!(store
            .sandbox_close(&actor, &child.canonical_ref, Some(child.rev))
            .is_err());
        assert!(host
            .resume_sandbox_attachment(
                &actor,
                &child.canonical_ref,
                replacement_request,
                |_, _| Ok(replacement_fence()),
            )
            .is_err());
        assert!(!child_root.join("data").exists());
        assert!(host
            .abandon_sandbox_attachment(
                &actor,
                &child.canonical_ref,
                replacement_request,
                |_, _| Ok(replacement_fence()),
            )
            .is_err());

        // Model a lost checkout after the original post-release gap:
        // the real driver has installed its exact marked registration
        // and index, but an expected file was not materialized.
        let prepared = store
            .sandbox_recover_attachment(&actor, &child.canonical_ref, replacement_request)
            .unwrap();
        let approval = replacement_fence();
        prepared.revalidate().unwrap();
        verify_attachment(
            &implementation,
            prepared.observation(),
            prepared.source(),
            &approval,
        )
        .unwrap();
        let (old, source) = prepared.observation().replaced().unwrap();
        implementation
            .continue_replacement(&source.prepared, &old.binding, prepared.source())
            .unwrap();
        fs::remove_file(&sibling).unwrap();
        fs::write(child_root.join("data/foreign"), b"keep").unwrap();
        prepared.mark_unknown().unwrap();
        drop(approval);
        assert!(host
            .resume_sandbox_attachment(
                &actor,
                &child.canonical_ref,
                replacement_request,
                |_, _| Ok(replacement_fence()),
            )
            .is_err());
        assert!(host
            .continue_sandbox_replacement(
                &actor,
                &child.canonical_ref,
                replacement_request,
                |_, _| Ok(replacement_fence()),
            )
            .is_err());
        assert_eq!(fs::read(child_root.join("data/foreign")).unwrap(), b"keep");
        assert!(!sibling.exists());
        assert_eq!(
            store.sandbox_get(&actor, &child.canonical_ref).unwrap(),
            child
        );
        fs::remove_file(child_root.join("data/foreign")).unwrap();

        // Explicit continuation reconciles only the exact partial
        // candidate, then the single final publication can itself fail.
        Connection::open(&database)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fixture_fail_replacement BEFORE UPDATE OF rev ON sandboxes_current
             BEGIN SELECT RAISE(ABORT,'fixture replacement publication failure'); END;",
            )
            .unwrap();
        assert!(host
            .continue_sandbox_replacement(
                &actor,
                &child.canonical_ref,
                replacement_request,
                |_, _| Ok(replacement_fence()),
            )
            .is_err());
        assert_eq!(fs::read(&sibling).unwrap(), b"replacement version");
        assert_eq!(
            store.sandbox_get(&actor, &child.canonical_ref).unwrap(),
            child
        );
        // An exact completed candidate must not be destroyed/recreated
        // by an explicit continuation after failed Store publication.
        let completed_identity = identity(&child_root.join("data"));
        assert!(host
            .continue_sandbox_replacement(
                &actor,
                &child.canonical_ref,
                replacement_request,
                |_, _| Ok(replacement_fence()),
            )
            .is_err());
        assert_eq!(identity(&child_root.join("data")), completed_identity);
        assert_eq!(fs::read(&sibling).unwrap(), b"replacement version");
        assert_eq!(fs::read(&unaffected).unwrap(), b"independent retained code");
        drop(store);
        Connection::open(&database)
            .unwrap()
            .execute_batch("DROP TRIGGER fixture_fail_replacement;")
            .unwrap();
        let store = SessionStore::open_with_sources(&state, registry.clone(), limit).unwrap();
        let host = GitCheckpointRestoreHost::new(&store, &driver);
        let replaced = host
            .resume_sandbox_attachment(&actor, &child.canonical_ref, replacement_request, |_, _| {
                Ok(replacement_fence())
            })
            .unwrap();
        assert_eq!(replaced.rev, child.rev + 1);
        assert_eq!(replaced.sources.len(), 2);
        assert_eq!(replaced.sources[0], child.sources[0]);
        assert_eq!(replaced.sources[1].binding_rev, replaced.rev);
        assert_eq!(replaced.layout_digest, child.layout_digest);
        assert_eq!(
            host.replace_sandbox_source::<ReplacementFence<'_>>(
                &actor,
                &child.canonical_ref,
                child.rev,
                replacement_request,
                replacement_anchor.clone(),
                |_| panic!("Applied replacement must not authorize again"),
            )
            .unwrap(),
            replaced
        );
        assert!(host
            .attach_sandbox_source::<Fence<'_>>(
                &actor,
                &child.canonical_ref,
                child.rev,
                replacement_request,
                replacement_anchor.clone(),
                |_| panic!("replacement request cannot replay attachment"),
            )
            .is_err());
        let capture_ref = format!(
            "refs/aw/checkpoints/{}/data",
            child.canonical_ref.rsplit_once("/s:").unwrap().1
        );
        assert!(git(
            &executable,
            &standalone_root.join("data"),
            &["for-each-ref", &capture_ref]
        )
        .is_empty());
        let child = replaced;
        // Retire independently prepared bindings through their actual
        // descriptors, not by assuming all came from the initial create.
        fs::write(&unaffected, b"code").unwrap();
        let closed = store
            .sandbox_close(&actor, parent_ref, Some(standalone.rev))
            .unwrap();
        assert_eq!(closed.sources.len(), 2);
        let parent_cleanup = || {
            host.reconcile_closed_sandbox::<Fence<'_>>(&actor, parent_ref, |_, _, _| {
                panic!("retained child must fence parent cleanup")
            })
        };
        assert!(parent_cleanup().is_err());
        store
            .sandbox_close(&actor, &child.canonical_ref, Some(child.rev))
            .unwrap();
        assert!(parent_cleanup().is_err());
        let retired = host
            .reconcile_closed_sandbox(&actor, &child.canonical_ref, |_, bindings, prepared| {
                assert_eq!(bindings.len(), 2);
                assert_eq!(prepared[0].name.as_str(), "code");
                assert_eq!(prepared[0].prepared.operation, SourceOperationKind::Restore);
                assert_eq!(prepared[1].name.as_str(), "data");
                assert_eq!(prepared[1].prepared.operation, SourceOperationKind::Restore);
                assert_ne!(
                    prepared[0].prepared.descriptor.prepare_id,
                    prepared[1].prepared.descriptor.prepare_id
                );
                Ok(fence(replacement_locations(), 4))
            })
            .unwrap();
        assert!(retired.sources.is_empty());
        assert_eq!(fs::read_dir(&child_root).unwrap().count(), 0);
        implementation
            .release_prepared_capture(&replacement_capture)
            .unwrap();
        let capture_ref_prefix = format!(
            "refs/aw/checkpoints/{}/",
            child.canonical_ref.rsplit_once("/s:").unwrap().1
        );
        assert!(git(
            &executable,
            &parent_code,
            &["for-each-ref", &capture_ref_prefix]
        )
        .is_empty());
        assert_eq!(count_sessions(), sessions_before);
        let released = host
            .reconcile_closed_sandbox(
                &actor,
                &standalone.canonical_ref,
                |_, bindings, prepared| {
                    assert_eq!(bindings.len(), 2);
                    assert_eq!(
                        prepared[0].prepared.descriptor.prepare_id,
                        standalone.canonical_ref.rsplit_once("/s:").unwrap().1
                    );
                    Ok(fence(locations(&standalone_root), 4))
                },
            )
            .unwrap();
        assert!(released.sources.is_empty());
        assert_eq!(fs::read_dir(&standalone_root).unwrap().count(), 0);
        assert_eq!(count_sessions(), sessions_before);
        // Final-source removal leaves the private parent layout itself
        // intact and empty. Ordinary source-free use is never inferred
        // while the final physical materialization is still retained.
        let final_root = root.join("final-source-release");
        private_dir(&final_root);
        let final_locations = || locations(&final_root).into_iter().take(1).collect();
        let empty = store
            .sandbox_create_fresh(&actor, &project, &layout(&final_root))
            .unwrap();
        let first_anchor = baseline.anchors[0].clone();
        let first_request = "fixture:first-source";
        // Foreign contents cannot become an implicitly managed layout.
        fs::write(final_root.join("foreign"), b"keep").unwrap();
        assert!(host
            .attach_sandbox_source(
                &actor,
                &empty.canonical_ref,
                empty.rev,
                first_request,
                first_anchor.clone(),
                |_| Ok(fence(final_locations(), 4)),
            )
            .is_err());
        assert!(!final_root.join("code").exists());
        fs::remove_file(final_root.join("foreign")).unwrap();

        // An untouched first-source claim reserves the empty layout
        // independently of which backing identity another caller selects.
        let approval = fence(final_locations(), 4);
        let prepared = store
            .sandbox_prepare_attachment(
                &actor,
                &empty.canonical_ref,
                empty.rev,
                "fixture:abandoned-first-source",
                first_anchor.clone(),
                |attachment, prepare_id| {
                    assert!(attachment.bindings().is_empty());
                    plan_attachment(&implementation, attachment, &approval, prepare_id)
                },
            )
            .unwrap();
        assert!(store
            .sandbox_prepare_attachment(
                &actor,
                &empty.canonical_ref,
                empty.rev,
                "fixture:competing-first-source",
                baseline.anchors[1].clone(),
                |_, _| panic!("layout reservation must precede another backing's plan"),
            )
            .is_err());
        assert!(store
            .sandbox_close(&actor, &empty.canonical_ref, Some(empty.rev))
            .is_err());
        assert!(store
            .sandbox_create_fresh(&actor, &project, &layout(&final_root))
            .is_err());
        assert!(store
            .create(&actor, "reserved first source", layout(&final_root))
            .is_err());
        prepared.mark_unknown().unwrap();
        drop(approval);
        host.abandon_sandbox_attachment(
            &actor,
            &empty.canonical_ref,
            "fixture:abandoned-first-source",
            |_, _| Ok(fence(final_locations(), 4)),
        )
        .unwrap();
        // The same claim's rejection releases the layout, without
        // consuming an identity, source binding or Sandbox revision.
        let alias = store
            .sandbox_create_fresh(&actor, &project, &layout(&final_root))
            .unwrap();
        store
            .sandbox_close(&actor, &alias.canonical_ref, Some(alias.rev))
            .unwrap();
        assert_eq!(
            store.sandbox_get(&actor, &empty.canonical_ref).unwrap(),
            empty
        );

        Connection::open(&database).unwrap().execute_batch(
            "CREATE TRIGGER fixture_fail_first_attachment BEFORE UPDATE OF rev ON sandboxes_current
             BEGIN SELECT RAISE(ABORT,'fixture first attachment publication failure'); END;",
        ).unwrap();
        assert!(host
            .attach_sandbox_source(
                &actor,
                &empty.canonical_ref,
                empty.rev,
                first_request,
                first_anchor.clone(),
                |_| Ok(fence(final_locations(), 4)),
            )
            .is_err());
        assert_eq!(fs::read(final_root.join("code/tracked")).unwrap(), b"code");
        assert_eq!(
            store.sandbox_get(&actor, &empty.canonical_ref).unwrap(),
            empty
        );
        assert!(store
            .sandbox_sources_set(
                &actor,
                &empty.canonical_ref,
                &bindings[1].name,
                Some(&bindings[1]),
                empty.rev,
            )
            .is_err());
        assert!(store
            .create(&actor, "uncertain first source", layout(&final_root))
            .is_err());
        drop(store);
        Connection::open(&database)
            .unwrap()
            .execute_batch("DROP TRIGGER fixture_fail_first_attachment;")
            .unwrap();
        let missing = SessionStore::open(&state).unwrap();
        assert!(missing
            .sandbox_close(&actor, &empty.canonical_ref, Some(empty.rev))
            .is_err());
        assert!(missing
            .create(
                &actor,
                "unresolved first source after restart",
                layout(&final_root)
            )
            .is_err());
        assert!(GitCheckpointRestoreHost::new(&missing, &driver)
            .resume_sandbox_attachment::<Fence<'_>>(
                &actor,
                &empty.canonical_ref,
                first_request,
                |_, _| panic!("missing driver must not inspect first source"),
            )
            .is_err());
        drop(missing);
        let store = SessionStore::open_with_sources(&state, registry.clone(), limit).unwrap();
        let host = GitCheckpointRestoreHost::new(&store, &driver);
        let last = host
            .resume_sandbox_attachment(&actor, &empty.canonical_ref, first_request, |_, _| {
                Ok(fence(final_locations(), 4))
            })
            .unwrap();
        assert_eq!(last.rev, empty.rev + 1);
        assert_eq!(last.sources[0].binding_rev, last.rev);
        assert_eq!(last.layout_digest, empty.layout_digest);
        assert_eq!(
            host.attach_sandbox_source::<Fence<'_>>(
                &actor,
                &empty.canonical_ref,
                empty.rev,
                first_request,
                first_anchor.clone(),
                |_| panic!("Applied first attachment must not authorize again"),
            )
            .unwrap(),
            last
        );
        assert!(store
            .create(&actor, "source-bearing ambient alias", layout(&final_root))
            .is_err());
        assert!(store
            .sandbox_sources_set(
                &actor,
                &last.canonical_ref,
                &bindings[1].name,
                Some(&bindings[1]),
                last.rev,
            )
            .is_err());
        assert_eq!(count_sessions(), sessions_before);
        // Same common-repository replacement: one original restore
        // registration is replaced by another at the exact same path.
        let mut narrowed = baseline.anchors[0].clone();
        narrowed.access = SourceAccess::ReadOnly;
        let last = host
            .replace_sandbox_source(
                &actor,
                &last.canonical_ref,
                last.rev,
                "fixture:same-origin-replacement",
                narrowed,
                |_| {
                    let locations: Vec<GitRestoreLocation> = final_locations();
                    Ok(ReplacementFence {
                        original: locations[0].clone(),
                        retained: fence(locations, 4),
                    })
                },
            )
            .unwrap();
        assert_eq!(last.sources[0].access, SourceAccess::ReadOnly);
        assert_eq!(last.sources[0].binding_rev, 3);
        assert_eq!(fs::read(final_root.join("code/tracked")).unwrap(), b"code");
        fs::write(final_root.join("foreign"), b"keep").unwrap();
        assert!(host
            .release_sandbox_source(
                &actor,
                &last.canonical_ref,
                &last.sources[0].name,
                last.rev,
                |_| Ok(fence(final_locations(), 4)),
            )
            .is_err());
        assert!(final_root.join("code/tracked").is_file());
        fs::remove_file(final_root.join("foreign")).unwrap();
        let empty = host
            .release_sandbox_source(
                &actor,
                &last.canonical_ref,
                &last.sources[0].name,
                last.rev,
                |_| Ok(fence(final_locations(), 4)),
            )
            .unwrap();
        assert!(empty.sources.is_empty());
        assert_eq!(empty.rev, last.rev + 1);
        assert_eq!(empty.layout_digest, last.layout_digest);
        assert!(final_root.is_dir());
        assert_eq!(fs::read_dir(&final_root).unwrap().count(), 0);
        // First-source reattachment after final removal uses its own
        // Prepared descriptor, with no initial construction receipt.
        let again = host
            .attach_sandbox_source(
                &actor,
                &empty.canonical_ref,
                empty.rev,
                "fixture:first-source-again",
                first_anchor,
                |_| Ok(fence(final_locations(), 4)),
            )
            .unwrap();
        assert_eq!(again.sources[0].binding_rev, again.rev);
        assert_eq!(again.rev, empty.rev + 1);
        store
            .sandbox_close(&actor, &again.canonical_ref, Some(again.rev))
            .unwrap();
        let retired = host
            .reconcile_closed_sandbox(&actor, &again.canonical_ref, |_, bindings, sources| {
                assert_eq!(bindings.len(), 1);
                assert_eq!(bindings[0].binding_rev, again.rev);
                assert_eq!(sources.len(), 1);
                Ok(fence(final_locations(), 4))
            })
            .unwrap();
        assert!(retired.sources.is_empty());
        assert_eq!(fs::read_dir(&final_root).unwrap().count(), 0);
        assert_eq!(count_sessions(), sessions_before);

        // A lost result before the first materialization may be abandoned
        // through the same exact Project claim, not another create.
        let abandoned_root = root.join("abandoned-standalone");
        private_dir(&abandoned_root);
        let approval = fence(locations(&abandoned_root), 4);
        let prepared = store
            .sandbox_prepare_create(
                &actor,
                &project,
                &layout(&abandoned_root),
                SandboxCreateFrom::Baseline(baseline.clone()),
                "fixture:abandoned-standalone",
                |prepare_id, _, anchors| {
                    let implementation = store.source_driver(&driver)?;
                    plan(&implementation, anchors, &approval, prepare_id)
                },
            )
            .unwrap();
        let abandoned = prepared.sandbox_ref().to_owned();
        prepared.mark_unknown().unwrap();
        drop(approval);
        host.abandon_sandbox_create(
            &actor,
            &project,
            None,
            "fixture:abandoned-standalone",
            |_, _| Ok(fence(locations(&abandoned_root), 4)),
        )
        .unwrap();
        host.abandon_sandbox_create::<Fence<'_>>(
            &actor,
            &project,
            None,
            "fixture:abandoned-standalone",
            |_, _| panic!("Rejected Sandbox cannot inspect again"),
        )
        .unwrap();
        assert!(store.sandbox_get(&actor, &abandoned).is_err());
        assert_eq!(fs::read_dir(&abandoned_root).unwrap().count(), 0);

        // Empty Auto accepts only independently selected ephemeral versions.
        // Reuse the ordinary Prepared cleanup, including task-free receipts.
        let ephemeral_root = root.join("ephemeral-baseline");
        private_dir(&ephemeral_root);
        let mut ephemeral = baseline.clone();
        for anchor in &mut ephemeral.anchors {
            anchor.checkpoint_policy = CheckpointPolicy::Ephemeral;
        }
        fail_publication();
        assert!(host
            .restore_versioned_baseline(
                &actor,
                &auto_request,
                "fixture:ephemeral-baseline",
                "local",
                || Ok((layout(&ephemeral_root), ephemeral)),
                |_, _| Ok(fence(locations(&ephemeral_root), 4))
            )
            .is_err());
        clear_failure();
        assert!(ephemeral_root.join("data/tracked").is_file());
        host.abandon(&actor, "fixture:ephemeral-baseline", |_, _| {
            Ok(fence(locations(&ephemeral_root), 4))
        })
        .unwrap();
        assert!(fs::read_dir(&ephemeral_root).unwrap().next().is_none());
        assert!(store
            .checkpoint_get_metadata(&actor, &auto_request.checkpoint)
            .unwrap()
            .sources
            .is_empty());

        // Retire the published layout through its original Agent, queue and
        // Sandbox owners. The checkpoint need not retain its anchor rows.
        let sandbox = restored.agent.sandbox_ref.as_deref().unwrap();
        assert!(host
            .reconcile_closed_sandbox::<Fence<'_>>(&actor, sandbox, |_, _, _| {
                panic!("open Sandbox cleanup authorized")
            })
            .is_err());
        let agent = store
            .agent_execution_binding(&actor, &restored.agent.canonical_ref)
            .unwrap();
        let queue = store.actor_queue_list(&actor, agent.session_id()).unwrap();
        assert_eq!(queue.len(), 1);
        store
            .actor_queue_delete(
                &actor,
                agent.session_id(),
                &queue[0].request.request_id,
                queue[0].request.writer_epoch,
            )
            .unwrap();
        store
            .agent_request_close(&actor, &restored.agent.canonical_ref, None)
            .unwrap();
        let closed_agent = store
            .agent_finish_close(&actor, &restored.agent.canonical_ref)
            .unwrap();
        let closed = store.sandbox_close(&actor, sandbox, None).unwrap();
        store
            .checkpoint_delete(&actor, &request.checkpoint)
            .unwrap();
        let capture_host = GitCheckpointCaptureHost::new(&store, &driver);
        // Physical retention release succeeds, but the Store transaction
        // fails. Exact original anchors remain in Unknown custody; replay
        // reconciles already-absent refs rather than recapturing anything.
        Connection::open(&database)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fixture_fail_retirement BEFORE DELETE ON checkpoint_sources
             BEGIN SELECT RAISE(ABORT,'fixture retirement publication failure'); END;",
            )
            .unwrap();
        assert!(capture_host
            .reconcile_retired(&actor, &request.checkpoint, |_| {
                Ok(capture_fence(capture_locations(), 4, 2))
            })
            .is_err());
        for name in ["code", "data"] {
            assert!(git(
                &executable,
                &source_root.join(name),
                &["for-each-ref", "refs/aw/checkpoints/"]
            )
            .is_empty());
        }
        Connection::open(&database)
            .unwrap()
            .execute_batch("DROP TRIGGER fixture_fail_retirement;")
            .unwrap();
        capture_host
            .reconcile_retired(&actor, &request.checkpoint, |_| {
                Ok(capture_fence(capture_locations(), 4, 0))
            })
            .unwrap();
        capture_host
            .reconcile_retired::<Fence<'_, aw_git::source_driver::GitSourceLocation>>(
                &actor,
                &request.checkpoint,
                |_| panic!("released retention authorized again"),
            )
            .unwrap();
        assert!(host
            .reconcile_closed_sandbox(&actor, sandbox, |_, _, _| {
                Ok(fence(locations(&other), 4))
            })
            .is_err());
        let foreign = destination.join("unrelated");
        fs::write(&foreign, b"keep").unwrap();
        assert!(host
            .reconcile_closed_sandbox(&actor, sandbox, |_, _, _| {
                Ok(fence(locations(&destination), 4))
            })
            .is_err());
        assert_eq!(fs::read(&foreign).unwrap(), b"keep");
        assert!(destination.join("code").exists() && destination.join("data").exists());
        fs::remove_file(&foreign).unwrap();

        // A real partial release leaves the whole original layout reserved.
        // Later user work is not discarded even when an earlier checkout
        // was removed successfully.
        let modified = destination.join("data/tracked");
        fs::write(&modified, b"user work").unwrap();
        assert!(host
            .reconcile_closed_sandbox(&actor, sandbox, |_, _, _| {
                Ok(fence(locations(&destination), 4))
            })
            .is_err());
        assert!(!destination.join("code").exists());
        assert_eq!(fs::read(&modified).unwrap(), b"user work");
        assert_eq!(store.sandbox_get(&actor, sandbox).unwrap(), closed);
        assert!(store
            .create(&actor, "retained layout", layout(&destination))
            .is_err());
        drop(store);

        // The original Applied descriptors and closed binding set survive
        // restart, retirement and a lower new-admission limit.
        let store =
            SessionStore::open_with_sources(&state, registry, SourceSetLimit::new(1).unwrap())
                .unwrap();
        let host = GitCheckpointRestoreHost::new(&store, &driver);
        fs::write(&modified, b"data").unwrap();
        let released = host
            .reconcile_closed_sandbox(&actor, sandbox, |_, _, _| {
                Ok(fence(locations(&destination), 4))
            })
            .unwrap();
        assert!(released.sources.is_empty());
        assert_eq!(released.rev, closed.rev + 1);
        assert!(fs::read_dir(&destination).unwrap().next().is_none());
        assert_eq!(
            host.reconcile_closed_sandbox::<Fence<'_>>(&actor, sandbox, |_, _, _| {
                panic!("released Sandbox cleanup authorized again")
            })
            .unwrap(),
            released
        );
        assert_eq!(
            store
                .agent_get(&actor, &restored.agent.canonical_ref)
                .unwrap(),
            closed_agent
        );
        assert!(store
            .activate(&actor, agent.session_id(), agent.branch_id())
            .is_err());
        assert!(baseline_root.join("code/tracked").is_file());
        assert!(baseline_root.join("data/tracked").is_file());
        assert!(authority.try_write().is_ok());
        for (name, head) in ["code", "data"].into_iter().zip(original_heads) {
            let repo = source_root.join(name);
            assert_eq!(git(&executable, &repo, &["rev-parse", "HEAD"]), head);
            assert_eq!(fs::read(repo.join("tracked")).unwrap(), name.as_bytes());
            assert!(git(&executable, &repo, &["diff", "--cached", "--name-only"]).is_empty());
        }
    }
}
