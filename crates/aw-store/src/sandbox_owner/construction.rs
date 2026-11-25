//! Standalone fresh Sandbox construction using the original Prepared receipt
//! and restore materialization contracts. No Agent/session is manufactured.
use super::{
    authorize_project, layout, parse_project_ref, parse_sandbox_ref, publish_constructed_in,
    require_fresh_layout_in, require_layout_quiescent_in, snapshot_with_sources,
    source_bindings::{self, VersionedSourceBinding},
    SandboxLifecycle, SandboxSnapshot,
};
use crate::{
    construction_receipt::{
        self, require_matching_sources, validate_inspected_bindings,
        validate_materialized_bindings, ConstructionCandidate, ConstructionOperation,
        ConstructionPreparation, PreparationState,
    },
    db, id, label,
    store_scope::StoreScope,
    user_for_principal, workspace_writer, Error, PrincipalId, Result, SessionBinding, SessionStore,
    SourceMutationBarrier, SourceMutationSet, StoreMode,
};
use aw_agent_core::{
    CheckpointPolicy, DriverSchemaPin, ExternalEvidence, PreparedSourceSet, RecoveryInspection,
    SourceBinding, SourceDriverRegistryReader, SourceOperationKind, SourcePublicationFence,
    SourcePublicationProof, SourceSet, SourceSetLimit,
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SandboxCreateBoundary {
    pub(crate) project_ref: String,
    writer_epoch: u64,
    layout_digest: String,
    // Independently retained immutable baseline versions. Not a copy of
    // another Sandbox's current mutable state or a public checkpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    baseline: Option<SourceSet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent: Option<ParentDerivation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParentDerivation {
    reference: String,
    rev: u64,
    bindings: Vec<VersionedSourceBinding>,
}

/// Trusted selection. The mutable parent's exact bindings are observed
/// by the Sandbox owner, not supplied by the caller.
pub enum SandboxCreateFrom<'a> {
    Baseline(SourceSet),
    Parent(&'a str),
}

#[derive(Clone, Copy)]
pub enum SandboxCreateSources<'a> {
    Baseline(&'a SourceSet),
    Parent(&'a [VersionedSourceBinding]),
}

impl SandboxCreateSources<'_> {
    fn mutation_set(self) -> Result<SourceMutationSet> {
        match self {
            Self::Baseline(set) => SourceMutationSet::from_anchors(&set.anchors),
            Self::Parent(bindings) => {
                SourceMutationSet::from_binding_refs(bindings.iter().map(|source| &source.binding))
            }
        }
    }

    fn len(self) -> usize {
        match self {
            Self::Baseline(set) => set.anchors.len(),
            Self::Parent(bindings) => bindings.len(),
        }
    }

    fn pin(self, index: usize) -> Result<DriverSchemaPin> {
        match self {
            Self::Baseline(set) => set.anchors[index].pin(),
            Self::Parent(bindings) => bindings[index].binding.pin(),
        }
    }

    fn validate_inspection(
        self,
        registry: &SourceDriverRegistryReader,
        results: Vec<RecoveryInspection<SourceBinding>>,
    ) -> Result<(Vec<SourceBinding>, Vec<ExternalEvidence>)> {
        match self {
            Self::Baseline(anchors) => validate_inspected_bindings(registry, anchors, results),
            Self::Parent(sources) => validate_materialized_bindings(
                registry,
                sources.len(),
                results,
                |index, binding| {
                    let original = &sources[index].binding;
                    if binding.name != original.name
                        || binding.driver != original.driver
                        || binding.source_ref != original.source_ref
                        || binding.access != original.access
                        || binding.checkpoint_policy != original.checkpoint_policy
                        || binding.materialization == original.materialization
                    {
                        return Err(Error::Conflict(
                            "derived binding differs from isolated parent source",
                        ));
                    }
                    Ok(())
                },
            ),
        }
    }
}

fn observe_parent(
    c: &Connection,
    project: &str,
    reference: &str,
    registry: &SourceDriverRegistryReader,
    limit: SourceSetLimit,
) -> Result<ParentDerivation> {
    let key = parse_sandbox_ref(reference)?;
    if key.project.canonical() != project || key.canonical() != reference {
        return Err(Error::Denied(
            "Sandbox derivation requires exact same Project",
        ));
    }
    let parent = snapshot_with_sources(c, reference)?;
    if parent.lifecycle != SandboxLifecycle::Open {
        return Err(Error::Conflict("Sandbox derivation requires open parent"));
    }
    require_layout_quiescent_in(c, reference, &crate::tool_root_key(&parent.tool_root)?)?;
    if parent.working_directory != parent.tool_root {
        require_layout_quiescent_in(
            c,
            reference,
            &crate::tool_root_key(&parent.working_directory)?,
        )?;
    }
    let bindings = source_bindings::select_in(c, reference, None, limit, registry)?;
    // Do not silently omit/recreate ephemeral bindings without destination policy.
    if bindings.is_empty() || bindings.len() != parent.sources.len() {
        return Err(Error::Denied(
            "effectful derivation requires nonempty all-required parent sources",
        ));
    }
    Ok(ParentDerivation {
        reference: reference.to_owned(),
        rev: parent.rev,
        bindings,
    })
}

impl SandboxCreateBoundary {
    fn input(&self) -> SandboxCreateSources<'_> {
        match (&self.baseline, &self.parent) {
            (Some(baseline), None) => SandboxCreateSources::Baseline(baseline),
            (None, Some(parent)) => SandboxCreateSources::Parent(&parent.bindings),
            _ => unreachable!("validated Sandbox construction selection"),
        }
    }

    pub(crate) fn parent_ref(&self) -> Option<&str> {
        self.parent.as_ref().map(|parent| parent.reference.as_str())
    }

    pub(crate) fn operation(&self) -> SourceOperationKind {
        if self.parent.is_some() {
            SourceOperationKind::Derive
        } else {
            SourceOperationKind::Restore
        }
    }

    pub(crate) fn source_pin(&self, index: usize) -> Result<DriverSchemaPin> {
        self.input().pin(index)
    }

    pub(crate) fn require_compatible(&self, registry: &SourceDriverRegistryReader) -> Result<()> {
        let input = self.input();
        for index in 0..input.len() {
            registry.resolve(&input.pin(index)?)?;
        }
        Ok(())
    }

    pub(crate) fn validate(
        &self,
        candidate: &ConstructionCandidate,
        sources: Option<&PreparedSourceSet>,
    ) -> Result<()> {
        let project = parse_project_ref(&self.project_ref)?;
        let ConstructionCandidate::Sandbox { sandbox } = candidate else {
            return Err(Error::Storage(
                "Sandbox construction requires a fresh Sandbox candidate",
            ));
        };
        if parse_sandbox_ref(sandbox)?.project != project
            || project.canonical() != self.project_ref
            || self.writer_epoch == 0
            || self.layout_digest.len() != 64
            || !self
                .layout_digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.baseline.is_some() == self.parent.is_some()
        {
            return Err(Error::Storage(
                "invalid standalone Sandbox construction boundary",
            ));
        }
        let sources = sources.ok_or(Error::Storage("Sandbox construction descriptors missing"))?;
        match self.input() {
            SandboxCreateSources::Baseline(baseline) => {
                baseline.validate()?;
                if baseline.anchors.is_empty()
                    || baseline
                        .anchors
                        .windows(2)
                        .any(|pair| pair[0].name.as_str() >= pair[1].name.as_str())
                {
                    return Err(Error::Storage("invalid standalone baseline source set"));
                }
                require_matching_sources(baseline, sources)
            }
            SandboxCreateSources::Parent(bindings) => {
                let parent = self.parent.as_ref().expect("parent selection");
                let key = parse_sandbox_ref(&parent.reference)?;
                sources.validate()?;
                if key.project != project
                    || key.canonical() != parent.reference
                    || parent.reference == *sandbox
                    || parent.rev == 0
                    || bindings.is_empty()
                    || bindings.len() != sources.sources.len()
                    || sources.operation != SourceOperationKind::Derive
                    || bindings
                        .windows(2)
                        .any(|pair| pair[0].binding.name.as_str() >= pair[1].binding.name.as_str())
                {
                    return Err(Error::Storage("invalid Sandbox parent derivation boundary"));
                }
                for (binding, source) in bindings.iter().zip(&sources.sources) {
                    binding.validate()?;
                    if binding.binding.name != source.name
                        || binding.binding.driver != source.prepared.descriptor.driver
                        || binding.binding.checkpoint_policy != CheckpointPolicy::Required
                    {
                        return Err(Error::Conflict(
                            "derive descriptors differ from complete required parent sources",
                        ));
                    }
                }
                Ok(())
            }
        }
    }

    pub(crate) fn authorize(
        &self,
        connection: &Connection,
        actor: &PrincipalId,
        write: bool,
    ) -> Result<()> {
        authorize_project(
            connection,
            actor,
            &parse_project_ref(&self.project_ref)?,
            write,
        )
    }

    fn require_current(
        &self,
        tx: &Transaction<'_>,
        actor: &PrincipalId,
        binding: &SessionBinding,
        registry: &SourceDriverRegistryReader,
    ) -> Result<()> {
        self.authorize(tx, actor, true)?;
        let project = parse_project_ref(&self.project_ref)?;
        if workspace_writer::writer_row(tx, &project.project)?.writer_epoch != self.writer_epoch
            || layout(binding)?.digest != self.layout_digest
        {
            return Err(Error::Conflict(
                "Sandbox construction authority or layout changed",
            ));
        }
        if let Some(parent) = &self.parent {
            let current = observe_parent(
                tx,
                &self.project_ref,
                &parent.reference,
                registry,
                SourceSetLimit::default(),
            )?;
            if current != *parent {
                return Err(Error::Conflict("Sandbox derivation parent changed"));
            }
        }
        require_fresh_layout_in(tx, &self.project_ref, binding)
    }
}

fn fingerprint(project: &str, from: Option<&str>, request_id: &str) -> Result<String> {
    label(request_id, 256)?;
    let mut hash = Sha256::new();
    if let Some(parent) = from {
        let key = parse_sandbox_ref(parent)?;
        if key.project.canonical() != project || key.canonical() != parent {
            return Err(Error::Denied(
                "Sandbox derivation requires exact same Project",
            ));
        }
        hash.update(b"aw-sandbox-create-derive-v1\0");
        hash.update(crate::json(&(project, parent, request_id))?.as_bytes());
    } else {
        hash.update(b"aw-sandbox-create-baseline-v1\0");
        hash.update(crate::json(&(project, request_id))?.as_bytes());
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn owner(store: &SessionStore) -> Result<()> {
    if store.inner.mode != StoreMode::OwnerLock {
        return Err(Error::Denied(
            "effectful Sandbox construction requires exclusive Store owner",
        ));
    }
    Ok(())
}

fn prior(
    c: &Connection,
    actor: &PrincipalId,
    project_ref: &str,
    from: Option<&str>,
    request_id: &str,
) -> Result<(String, Option<ConstructionPreparation>)> {
    let project = parse_project_ref(project_ref)?;
    authorize_project(c, actor, &project, false)?;
    if project.canonical() != project_ref {
        return Err(Error::Denied(
            "Sandbox construction requires canonical Project",
        ));
    }
    let creator = user_for_principal(c, actor)?
        .ok_or(Error::Denied("Sandbox construction caller unavailable"))?;
    let expected = fingerprint(project_ref, from, request_id)?;
    let receipt = construction_receipt::lookup_in(
        c,
        ConstructionOperation::SandboxCreate,
        &creator,
        request_id,
    )?;
    if let Some(receipt) = &receipt {
        receipt.require_actor(c, actor)?;
        if receipt.fingerprint != expected
            || receipt.sandbox_boundary.as_ref().is_none_or(|boundary| {
                boundary.project_ref != project_ref || boundary.parent_ref() != from
            })
        {
            return Err(Error::Conflict(
                "Sandbox construction request differs from original",
            ));
        }
    }
    Ok((creator, receipt))
}

fn require_new(
    c: &Connection,
    actor: &PrincipalId,
    project: &str,
    from: Option<&str>,
    request: &str,
) -> Result<String> {
    let (creator, receipt) = prior(c, actor, project, from, request)?;
    if receipt.is_some() {
        return Err(Error::Conflict(
            "Sandbox construction already claimed; replay or inspect",
        ));
    }
    Ok(creator)
}

fn compatible(
    registry: &SourceDriverRegistryReader,
    boundary: &SandboxCreateBoundary,
    sources: &PreparedSourceSet,
) -> Result<()> {
    boundary.require_compatible(registry)?;
    for source in &sources.sources {
        registry.resolve(&source.prepared.descriptor.pin()?)?;
    }
    Ok(())
}

/// Original private candidate and complete source custody.
#[must_use = "retain the original candidate through publication or recovery"]
pub struct PreparedSandboxCreate<'a> {
    store: &'a SessionStore,
    actor: PrincipalId,
    receipt: ConstructionPreparation,
    barrier: SourceMutationBarrier,
}

impl<'a> PreparedSandboxCreate<'a> {
    fn boundary(&self) -> &SandboxCreateBoundary {
        self.receipt
            .sandbox_boundary
            .as_ref()
            .expect("validated Project boundary")
    }
    pub fn sandbox_ref(&self) -> &str {
        match &self.receipt.candidate {
            ConstructionCandidate::Sandbox { sandbox } => sandbox,
            _ => unreachable!("validated Sandbox candidate"),
        }
    }
    pub fn input(&self) -> SandboxCreateSources<'_> {
        self.boundary().input()
    }
    pub fn sources(&self) -> &PreparedSourceSet {
        self.receipt.sources.as_ref().expect("validated source set")
    }

    fn require_current_in(&self, tx: &Transaction<'_>) -> Result<SourceDriverRegistryReader> {
        construction_receipt::require_unpublished_sources_in(
            tx,
            &self.actor,
            &self.receipt,
            &self.barrier,
        )?;
        self.boundary().authorize(tx, &self.actor, true)?;
        let registry = self.store.inner.source_drivers.reader()?;
        compatible(&registry, self.boundary(), self.sources())?;
        Ok(registry)
    }

    pub fn revalidate(&self, binding: &SessionBinding) -> Result<()> {
        let mut connection = self.store.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let registry = self.require_current_in(&tx)?;
        self.boundary()
            .require_current(&tx, &self.actor, binding, &registry)?;
        db(tx.rollback())
    }

    pub fn mark_unknown(self) -> Result<()> {
        construction_receipt::mark_sources_unknown(
            self.store,
            &self.actor,
            &self.receipt,
            &self.barrier,
        )
    }

    fn inspection_registry(&self) -> Result<SourceDriverRegistryReader> {
        let mut connection = self.store.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let registry = self.require_current_in(&tx)?;
        db(tx.commit())?;
        Ok(registry)
    }

    pub fn inspect(
        self,
        inspect: impl FnOnce(
            &SourceDriverRegistryReader,
            SandboxCreateSources<'_>,
            &PreparedSourceSet,
        ) -> Result<Vec<RecoveryInspection<SourceBinding>>>,
    ) -> Result<InspectedSandboxCreate<'a>> {
        let result = (|| {
            let registry = self.inspection_registry()?;
            let results = inspect(&registry, self.input(), self.sources())?;
            let (bindings, evidence) = self.input().validate_inspection(&registry, results)?;
            self.inspection_registry()?;
            Ok((bindings, evidence))
        })();
        match result {
            Ok((bindings, evidence)) => Ok(InspectedSandboxCreate {
                create: self,
                bindings,
                evidence,
            }),
            Err(error) => {
                self.mark_unknown()?;
                Err(error)
            }
        }
    }

    pub fn abandon<F>(
        self,
        reconcile: impl FnOnce(
            &SourceDriverRegistryReader,
            SandboxCreateSources<'_>,
            &PreparedSourceSet,
        ) -> Result<F>,
    ) -> Result<()> {
        construction_receipt::reconcile_unpublished_sources(
            self.store,
            &self.actor,
            &self.receipt,
            &self.barrier,
            || {
                let registry = self.inspection_registry()?;
                reconcile(&registry, self.input(), self.sources())
            },
        )
    }
}

#[must_use = "publish or reconcile the inspected Sandbox candidate"]
pub struct InspectedSandboxCreate<'a> {
    create: PreparedSandboxCreate<'a>,
    bindings: Vec<SourceBinding>,
    evidence: Vec<ExternalEvidence>,
}

impl InspectedSandboxCreate<'_> {
    pub fn publish<F: SourcePublicationFence>(
        self,
        binding: &SessionBinding,
        verify: impl FnOnce(
            &SourceDriverRegistryReader,
            SandboxCreateSources<'_>,
            &PreparedSourceSet,
            &[SourceBinding],
            &[ExternalEvidence],
        ) -> Result<Vec<SourcePublicationProof<F>>>,
    ) -> Result<SandboxSnapshot> {
        let create = &self.create;
        let result = (|| {
            create.revalidate(binding)?;
            let registry = create.inspection_registry()?;
            let proofs = verify(
                &registry,
                create.input(),
                create.sources(),
                &self.bindings,
                &self.evidence,
            )?;
            construction_receipt::publish_sources(
                create.store,
                &create.actor,
                &create.receipt,
                &create.barrier,
                &self.evidence,
                proofs,
                |tx| {
                    create
                        .boundary()
                        .require_current(tx, &create.actor, binding, &registry)?;
                    create.receipt.candidate.require_absent_in(tx)?;
                    compatible(&registry, create.boundary(), create.sources())?;
                    let fresh = publish_constructed_in(
                        tx,
                        &create.actor,
                        (create.sandbox_ref(), create.boundary().parent_ref()),
                        binding,
                        &self.bindings,
                        &create.store.inner.source_drivers,
                        create.store.source_set_limit(),
                    )?;
                    let snapshot = snapshot_with_sources(tx, &fresh.reference)?;
                    Ok((fresh.reference, snapshot))
                },
            )
        })();
        match result {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                self.create.mark_unknown()?;
                Err(error)
            }
        }
    }
}

impl SessionStore {
    /// Replay before any host baseline selection or driver invocation.
    pub fn sandbox_replay_create(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        from: Option<&str>,
        request_id: &str,
    ) -> Result<Option<SandboxSnapshot>> {
        owner(self)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction())?;
        let (_, receipt) = prior(&tx, actor, project_ref, from, request_id)?;
        let result = match receipt {
            None => None,
            Some(receipt) if receipt.state == PreparationState::Applied => {
                let reference = receipt
                    .result_ref
                    .ok_or(Error::Storage("published Sandbox result missing"))?;
                let snapshot = snapshot_with_sources(&tx, &reference)?;
                if snapshot.project_ref != project_ref {
                    return Err(Error::Storage("published Sandbox Project changed"));
                }
                Some(snapshot)
            }
            Some(_) => {
                return Err(Error::Conflict(
                    "Sandbox construction already claimed; inspect or abandon original",
                ))
            }
        };
        db(tx.commit())?;
        Ok(result)
    }

    /// Fresh Project baseline from independently retained immutable versions.
    /// Planning is read-only; the caller holds independent full-set authority
    /// and physical destination reservation. Only this returned Prepared
    /// candidate permits the first external materialization.
    pub fn sandbox_prepare_create(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        binding: &SessionBinding,
        from: SandboxCreateFrom<'_>,
        request_id: &str,
        plan: impl FnOnce(&str, &str, SandboxCreateSources<'_>) -> Result<PreparedSourceSet>,
    ) -> Result<PreparedSandboxCreate<'_>> {
        owner(self)?;
        let parent_ref = match &from {
            SandboxCreateFrom::Parent(parent) => Some(*parent),
            _ => None,
        };
        let (creator, boundary, scope) = {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let creator = require_new(&tx, actor, project_ref, parent_ref, request_id)?;
            let project = parse_project_ref(project_ref)?;
            authorize_project(&tx, actor, &project, true)?;
            let writer_epoch = workspace_writer::writer_row(&tx, &project.project)?.writer_epoch;
            require_fresh_layout_in(&tx, project_ref, binding)?;
            let registry = self.inner.source_drivers.reader()?;
            let (baseline, parent) = match from {
                SandboxCreateFrom::Baseline(mut baseline) => {
                    baseline.admit(self.source_set_limit())?;
                    if baseline.anchors.is_empty() {
                        return Err(Error::Denied(
                            "effectful Sandbox baseline requires retained sources",
                        ));
                    }
                    baseline
                        .anchors
                        .sort_unstable_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
                    (Some(baseline), None)
                }
                SandboxCreateFrom::Parent(reference) => (
                    None,
                    Some(observe_parent(
                        &tx,
                        project_ref,
                        reference,
                        &registry,
                        self.source_set_limit(),
                    )?),
                ),
            };
            let boundary = SandboxCreateBoundary {
                project_ref: project_ref.to_owned(),
                writer_epoch,
                layout_digest: layout(binding)?.digest,
                baseline,
                parent,
            };
            boundary.require_compatible(&registry)?;
            boundary.input().mutation_set()?.require_unleased_in(&tx)?;
            let scope = StoreScope::load(&tx)?;
            db(tx.commit())?;
            (creator, boundary, scope)
        };
        let candidate = ConstructionCandidate::Sandbox {
            sandbox: format!("{project_ref}/s:{}", id("sbx")?),
        };
        let ConstructionCandidate::Sandbox { sandbox } = &candidate else {
            unreachable!()
        };
        let mut sources = plan(candidate.prepare_id(), sandbox, boundary.input())?;
        sources.admit(self.source_set_limit())?;
        sources
            .sources
            .sort_unstable_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
        boundary.validate(&candidate, Some(&sources))?;
        let set = boundary.input().mutation_set()?;
        let receipt = ConstructionPreparation {
            operation: ConstructionOperation::SandboxCreate,
            scope: creator.clone(),
            creator,
            parent_session_id: String::new(),
            parent_branch_id: String::new(),
            request_id: request_id.to_owned(),
            fingerprint: fingerprint(project_ref, parent_ref, request_id)?,
            state: PreparationState::Prepared,
            candidate,
            task: None,
            sources: Some(sources),
            capture_boundary: None,
            restore_boundary: None,
            sandbox_boundary: Some(boundary),
            result_ref: None,
            note: None,
        };
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        if StoreScope::load(&tx)? != scope
            || require_new(&tx, actor, project_ref, parent_ref, request_id)? != receipt.creator
        {
            return Err(Error::Conflict(
                "Sandbox construction owner changed during planning",
            ));
        }
        receipt.candidate.require_absent_in(&tx)?;
        receipt
            .sandbox_boundary
            .as_ref()
            .expect("Project boundary")
            .require_current(&tx, actor, binding, &self.inner.source_drivers.reader()?)?;
        let barrier = construction_receipt::claim_sources_in(self, &tx, actor, &receipt, &set)?;
        db(tx.commit())?;
        Ok(PreparedSandboxCreate {
            store: self,
            actor: actor.clone(),
            receipt,
            barrier,
        })
    }

    fn resume_sandbox_create(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        from: Option<&str>,
        request_id: &str,
        abandon: bool,
    ) -> Result<Option<PreparedSandboxCreate<'_>>> {
        owner(self)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (_, receipt) = prior(&tx, actor, project_ref, from, request_id)?;
        let receipt = receipt.ok_or(Error::Denied("original Sandbox construction unavailable"))?;
        if abandon && receipt.state == PreparationState::Rejected {
            return Ok(None);
        }
        if !receipt.state.unresolved() {
            return Err(Error::Conflict("Sandbox construction is already terminal"));
        }
        let boundary = receipt
            .sandbox_boundary
            .as_ref()
            .ok_or(Error::Storage("Sandbox recovery boundary missing"))?;
        boundary.authorize(&tx, actor, true)?;
        receipt.candidate.require_absent_in(&tx)?;
        compatible(
            &self.inner.source_drivers.reader()?,
            boundary,
            receipt
                .sources
                .as_ref()
                .ok_or(Error::Storage("Sandbox recovery descriptors missing"))?,
        )?;
        let set = boundary.input().mutation_set()?;
        let barrier =
            self.recover_source_mutation_barrier_in(&tx, &set, receipt.candidate.prepare_id())?;
        construction_receipt::require_unpublished_sources_in(&tx, actor, &receipt, &barrier)?;
        db(tx.commit())?;
        Ok(Some(PreparedSandboxCreate {
            store: self,
            actor: actor.clone(),
            receipt,
            barrier,
        }))
    }

    pub fn sandbox_inspect_create(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        from: Option<&str>,
        request_id: &str,
        inspect: impl FnOnce(
            &SourceDriverRegistryReader,
            SandboxCreateSources<'_>,
            &PreparedSourceSet,
        ) -> Result<Vec<RecoveryInspection<SourceBinding>>>,
    ) -> Result<InspectedSandboxCreate<'_>> {
        self.resume_sandbox_create(actor, project_ref, from, request_id, false)?
            .ok_or(Error::Conflict(
                "Sandbox construction has no unresolved candidate",
            ))?
            .inspect(inspect)
    }

    pub fn sandbox_abandon_create<F>(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        from: Option<&str>,
        request_id: &str,
        reconcile: impl FnOnce(
            &SourceDriverRegistryReader,
            SandboxCreateSources<'_>,
            &PreparedSourceSet,
        ) -> Result<F>,
    ) -> Result<()> {
        match self.resume_sandbox_create(actor, project_ref, from, request_id, true)? {
            Some(prepared) => prepared.abandon(reconcile),
            None => Ok(()),
        }
    }
}
