//! Standalone fresh Sandbox construction using the original Prepared receipt
//! and restore materialization contracts. No Agent/session is manufactured.
use super::{
    authorize_project, layout, parse_project_ref, parse_sandbox_ref, publish_fresh_in,
    require_fresh_layout_in, snapshot_with_sources, SandboxSnapshot,
};
use crate::{
    construction_receipt::{
        self, require_matching_sources, validate_inspected_bindings, ConstructionCandidate,
        ConstructionOperation, ConstructionPreparation, PreparationState,
    },
    db, id, label,
    store_scope::StoreScope,
    user_for_principal, workspace_writer, Error, PrincipalId, Result, SessionBinding, SessionStore,
    SourceMutationBarrier, SourceMutationSet, StoreMode,
};
use aw_agent_core::{
    ExternalEvidence, PreparedSourceSet, RecoveryInspection, SourceBinding,
    SourceDriverRegistryReader, SourcePublicationFence, SourcePublicationProof, SourceSet,
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
    pub(crate) baseline: SourceSet,
}

impl SandboxCreateBoundary {
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
            || self.baseline.anchors.is_empty()
            || self
                .baseline
                .anchors
                .windows(2)
                .any(|pair| pair[0].name.as_str() >= pair[1].name.as_str())
        {
            return Err(Error::Storage(
                "invalid standalone Sandbox construction boundary",
            ));
        }
        self.baseline.validate()?;
        require_matching_sources(
            &self.baseline,
            sources.ok_or(Error::Storage("Sandbox construction descriptors missing"))?,
        )
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
        require_fresh_layout_in(tx, &self.project_ref, binding)
    }
}

fn fingerprint(project: &str, request_id: &str) -> Result<String> {
    label(request_id, 256)?;
    let mut hash = Sha256::new();
    hash.update(b"aw-sandbox-create-baseline-v1\0");
    hash.update(crate::json(&(project, request_id))?.as_bytes());
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
    let expected = fingerprint(project_ref, request_id)?;
    let receipt = construction_receipt::lookup_in(
        c,
        ConstructionOperation::SandboxCreate,
        &creator,
        request_id,
    )?;
    if let Some(receipt) = &receipt {
        receipt.require_actor(c, actor)?;
        if receipt.fingerprint != expected
            || receipt
                .sandbox_boundary
                .as_ref()
                .is_none_or(|boundary| boundary.project_ref != project_ref)
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
    request: &str,
) -> Result<String> {
    let (creator, receipt) = prior(c, actor, project, request)?;
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
    for (anchor, source) in boundary.baseline.anchors.iter().zip(&sources.sources) {
        registry.resolve(&anchor.pin()?)?;
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
    pub fn anchors(&self) -> &SourceSet {
        &self.boundary().baseline
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
        self.require_current_in(&tx)?;
        self.boundary().require_current(&tx, &self.actor, binding)?;
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
            &SourceSet,
            &PreparedSourceSet,
        ) -> Result<Vec<RecoveryInspection<SourceBinding>>>,
    ) -> Result<InspectedSandboxCreate<'a>> {
        let result = (|| {
            let registry = self.inspection_registry()?;
            let results = inspect(&registry, self.anchors(), self.sources())?;
            let (bindings, evidence) =
                validate_inspected_bindings(&registry, self.anchors(), results)?;
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
        reconcile: impl FnOnce(&SourceDriverRegistryReader, &SourceSet, &PreparedSourceSet) -> Result<F>,
    ) -> Result<()> {
        construction_receipt::reconcile_unpublished_sources(
            self.store,
            &self.actor,
            &self.receipt,
            &self.barrier,
            || {
                let registry = self.inspection_registry()?;
                reconcile(&registry, self.anchors(), self.sources())
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
            &SourceSet,
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
                create.anchors(),
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
                        .require_current(tx, &create.actor, binding)?;
                    create.receipt.candidate.require_absent_in(tx)?;
                    compatible(&registry, create.boundary(), create.sources())?;
                    let fresh = publish_fresh_in(
                        tx,
                        &create.actor,
                        create.sandbox_ref(),
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
        request_id: &str,
    ) -> Result<Option<SandboxSnapshot>> {
        owner(self)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction())?;
        let (_, receipt) = prior(&tx, actor, project_ref, request_id)?;
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
        mut baseline: SourceSet,
        request_id: &str,
        plan: impl FnOnce(&str, &str, &SourceSet) -> Result<PreparedSourceSet>,
    ) -> Result<PreparedSandboxCreate<'_>> {
        owner(self)?;
        baseline.admit(self.source_set_limit())?;
        if baseline.anchors.is_empty() {
            return Err(Error::Denied(
                "effectful Sandbox baseline requires retained sources",
            ));
        }
        baseline
            .anchors
            .sort_unstable_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
        let set = SourceMutationSet::from_anchors(&baseline.anchors)?;
        let (creator, writer_epoch, scope) = {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let creator = require_new(&tx, actor, project_ref, request_id)?;
            let project = parse_project_ref(project_ref)?;
            authorize_project(&tx, actor, &project, true)?;
            let writer_epoch = workspace_writer::writer_row(&tx, &project.project)?.writer_epoch;
            require_fresh_layout_in(&tx, project_ref, binding)?;
            set.require_unleased_in(&tx)?;
            let registry = self.inner.source_drivers.reader()?;
            for anchor in &baseline.anchors {
                registry.resolve(&anchor.pin()?)?;
            }
            let scope = StoreScope::load(&tx)?;
            db(tx.commit())?;
            (creator, writer_epoch, scope)
        };
        let candidate = ConstructionCandidate::Sandbox {
            sandbox: format!("{project_ref}/s:{}", id("sbx")?),
        };
        let ConstructionCandidate::Sandbox { sandbox } = &candidate else {
            unreachable!()
        };
        let mut sources = plan(candidate.prepare_id(), sandbox, &baseline)?;
        sources.admit(self.source_set_limit())?;
        sources
            .sources
            .sort_unstable_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
        let boundary = SandboxCreateBoundary {
            project_ref: project_ref.to_owned(),
            writer_epoch,
            layout_digest: layout(binding)?.digest,
            baseline,
        };
        boundary.validate(&candidate, Some(&sources))?;
        let receipt = ConstructionPreparation {
            operation: ConstructionOperation::SandboxCreate,
            scope: creator.clone(),
            creator,
            parent_session_id: String::new(),
            parent_branch_id: String::new(),
            request_id: request_id.to_owned(),
            fingerprint: fingerprint(project_ref, request_id)?,
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
            || require_new(&tx, actor, project_ref, request_id)? != receipt.creator
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
            .require_current(&tx, actor, binding)?;
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
        request_id: &str,
        abandon: bool,
    ) -> Result<Option<PreparedSandboxCreate<'_>>> {
        owner(self)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (_, receipt) = prior(&tx, actor, project_ref, request_id)?;
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
        let set = SourceMutationSet::from_anchors(&boundary.baseline.anchors)?;
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
        request_id: &str,
        inspect: impl FnOnce(
            &SourceDriverRegistryReader,
            &SourceSet,
            &PreparedSourceSet,
        ) -> Result<Vec<RecoveryInspection<SourceBinding>>>,
    ) -> Result<InspectedSandboxCreate<'_>> {
        self.resume_sandbox_create(actor, project_ref, request_id, false)?
            .ok_or(Error::Conflict(
                "Sandbox construction has no unresolved candidate",
            ))?
            .inspect(inspect)
    }

    pub fn sandbox_abandon_create<F>(
        &self,
        actor: &PrincipalId,
        project_ref: &str,
        request_id: &str,
        reconcile: impl FnOnce(&SourceDriverRegistryReader, &SourceSet, &PreparedSourceSet) -> Result<F>,
    ) -> Result<()> {
        match self.resume_sandbox_create(actor, project_ref, request_id, true)? {
            Some(prepared) => prepared.abandon(reconcile),
            None => Ok(()),
        }
    }
}
