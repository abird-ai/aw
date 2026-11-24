//! Source-bearing checkpoint preparation. The host authorizes source
//! capture and supplies read-only driver planning; no effect or publication
//! occurs here. A Prepared receipt and its complete local barrier commit
//! together.
use crate::{
    agent_owner::{self, AgentKey, AgentPermission},
    construction_receipt::{
        self, ConstructionCandidate, ConstructionOperation, ConstructionPreparation,
        PreparationState,
    },
    db, history_owner, id, key, label,
    sandbox_owner::source_bindings::{self, VersionedSourceBinding},
    session_in,
    store_scope::StoreScope,
    user_for_principal, Error, Result, SessionStore, SourceMutationBarrier, SourceMutationSet,
};
use aw_agent_core::{
    validate_source_set_hard_limit, CheckpointPolicy, PreparedSourceSet, PrincipalId,
    SourceCoverage, SourceDriverRegistryReader, SourceName, SourceOperationKind, SourceSetLimit,
};
use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Automatic or explicitly named source selection. An empty Auto set
/// uses the common history-only publication path with Auto coverage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode", content = "names")]
pub enum CheckpointCaptureSelection {
    Auto,
    Named(Vec<SourceName>),
}

impl CheckpointCaptureSelection {
    fn canonicalize(mut self) -> Result<Self> {
        if let Self::Named(names) = &mut self {
            validate_source_set_hard_limit(names.len())?;
            for name in names.iter() {
                name.validate()?;
            }
            names.sort_unstable_by(|a, b| a.as_str().cmp(b.as_str()));
            if names.is_empty() || names.windows(2).any(|pair| pair[0] == pair[1]) {
                return Err(Error::Config(
                    "selected checkpoint sources must be nonempty and unique".into(),
                ));
            }
        }
        Ok(self)
    }

    fn names(&self) -> Option<&[SourceName]> {
        match self {
            Self::Auto => None,
            Self::Named(names) => Some(names),
        }
    }

    pub fn coverage(&self) -> SourceCoverage {
        match self {
            Self::Auto => SourceCoverage::AutoAllRequired,
            Self::Named(_) => SourceCoverage::Selected,
        }
    }
}

/// Durable, exact Store-owned capture inputs; not source authority.
/// Current compatibility is diagnostic, so it is not part of this identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CaptureBoundary {
    pub(crate) agent_ref: String,
    pub(crate) sandbox_ref: String,
    pub(crate) sandbox_rev: u64,
    pub(crate) history_base_id: String,
    pub(crate) access_generation: [u8; 32],
    pub(crate) selection: CheckpointCaptureSelection,
    pub(crate) bindings: Vec<VersionedSourceBinding>,
    pub(crate) checkpoint_note: Option<String>,
}

impl CaptureBoundary {
    pub(crate) fn validate(&self, sources: &PreparedSourceSet) -> Result<()> {
        label(&self.agent_ref, 2048)?;
        label(&self.sandbox_ref, 2048)?;
        label(&self.history_base_id, 128)?;
        if let Some(note) = &self.checkpoint_note {
            label(note, 2048)?;
        }
        validate_source_set_hard_limit(self.bindings.len())?;
        if self.sandbox_rev == 0
            || self.bindings.is_empty()
            || self.bindings.len() != sources.sources.len()
            || sources.operation != SourceOperationKind::Capture
        {
            return Err(Error::Conflict("invalid checkpoint capture boundary"));
        }
        // Both collections are bounded. The descriptor is driver-owned,
        // but the name and driver must match the exact selected binding.
        // Canonical name order means no lookup or extra indexing is needed.
        for (binding, source) in self.bindings.iter().zip(&sources.sources) {
            binding.validate()?;
            if binding.binding.name != source.name
                || binding.binding.driver != source.prepared.descriptor.driver
            {
                return Err(Error::Conflict(
                    "prepared descriptor does not match selected SourceBinding",
                ));
            }
            if self.selection == CheckpointCaptureSelection::Auto
                && binding.binding.checkpoint_policy != CheckpointPolicy::Required
            {
                return Err(Error::Conflict(
                    "automatic capture includes an ephemeral binding",
                ));
            }
        }
        if self
            .bindings
            .windows(2)
            .any(|pair| pair[0].binding.name.as_str() >= pair[1].binding.name.as_str())
        {
            return Err(Error::Conflict(
                "capture bindings are not unique and canonical",
            ));
        }
        if let Some(names) = self.selection.names() {
            if names.len() != self.bindings.len()
                || !names
                    .iter()
                    .zip(&self.bindings)
                    .all(|(name, binding)| *name == binding.binding.name)
            {
                return Err(Error::Conflict(
                    "capture bindings do not match selected names",
                ));
            }
        }
        Ok(())
    }

    fn mutation_set(&self) -> Result<SourceMutationSet> {
        SourceMutationSet::from_binding_refs(self.bindings.iter().map(|binding| &binding.binding))
    }
}

fn capture_sandbox_in(connection: &Connection, origin: AgentKey<'_>) -> Result<(String, u64)> {
    db(connection.query_row(
        "SELECT s.sandbox_id,s.rev FROM agent_state_current a
         JOIN sandboxes_current s ON s.sandbox_id=a.sandbox_id
         JOIN workspaces w ON w.workspace_id=a.project_handle
         WHERE a.session_id=?1 AND a.branch_id=?2
           AND a.lifecycle='open' AND s.lifecycle='open' AND w.archived_at IS NULL
           AND a.domain_handle=s.domain_handle AND a.identity_handle=s.identity_handle
           AND a.project_handle=s.project_handle",
        params![origin.session, origin.branch],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))
}

pub(super) fn require_empty_auto_in(connection: &Connection, origin: AgentKey<'_>) -> Result<()> {
    let (sandbox, _) = capture_sandbox_in(connection, origin)?;
    let required: bool = db(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sandbox_source_bindings_current
         WHERE sandbox_id=?1 AND checkpoint_policy='required')",
        [&sandbox],
        |row| row.get(0),
    ))?;
    if required {
        return Err(Error::Conflict(
            "automatic checkpoint requires captured sources",
        ));
    }
    Ok(())
}

fn observe_in(
    connection: &Connection,
    actor: &PrincipalId,
    origin: AgentKey<'_>,
    selection: &CheckpointCaptureSelection,
    note: Option<&str>,
    limit: SourceSetLimit,
    registry: &SourceDriverRegistryReader,
) -> Result<CaptureBoundary> {
    let access =
        agent_owner::authorize_permission(connection, actor, origin, AgentPermission::View)?;
    let agent_ref = agent_owner::canonical_ref(connection, origin)?;
    let (sandbox_ref, sandbox_rev) = capture_sandbox_in(connection, origin)?;
    crate::sandbox_owner::require_layout_quiescent_in(
        connection,
        &sandbox_ref,
        &key(&session_in(connection, origin.session)?.binding)?,
    )?;
    let bindings =
        source_bindings::select_in(connection, &sandbox_ref, selection.names(), limit, registry)?;
    if bindings.is_empty() {
        return Err(Error::Conflict(
            "automatic selection has no required sources; use empty Auto capture",
        ));
    }
    let history = history_owner::seal_in(connection, origin.session, origin.branch)?;
    Ok(CaptureBoundary {
        agent_ref,
        sandbox_ref,
        sandbox_rev,
        history_base_id: history.id,
        access_generation: access.access_generation,
        selection: selection.clone(),
        bindings,
        checkpoint_note: note.map(str::to_owned),
    })
}

fn require_new_request(connection: &Connection, creator: &str, request_id: &str) -> Result<()> {
    let existing: bool = db(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM checkpoints WHERE created_by=?1 AND request_id=?2)",
        params![creator, request_id],
        |row| row.get(0),
    ))?;
    if existing
        || construction_receipt::lookup_in(
            connection,
            ConstructionOperation::CheckpointCreate,
            creator,
            request_id,
        )?
        .is_some()
    {
        return Err(Error::Conflict(
            "checkpoint request is non-retryable; inspect original result or receipt",
        ));
    }
    Ok(())
}

pub(super) fn fingerprint(
    reference: &str,
    selection: &CheckpointCaptureSelection,
    note: Option<&str>,
) -> Result<String> {
    let request = crate::json(&(reference, selection, note))?;
    let mut hash = Sha256::new();
    hash.update(b"aw-checkpoint-create-sources-v1\0");
    hash.update(request.as_bytes());
    Ok(format!("{:x}", hash.finalize()))
}

mod recovery;
pub use recovery::InspectedCheckpointCapture;

/// Trusted, live Prepared checkpoint capture. No Clone, no implicit release.
/// Driver effects still require independently held source authority. Only
/// inspected, revalidated results can enter the final publication boundary.
#[must_use = "retain the prepared capture until publication or recovery"]
pub struct PreparedCheckpointCapture<'a> {
    store: &'a SessionStore,
    actor: PrincipalId,
    receipt: ConstructionPreparation,
    barrier: SourceMutationBarrier<'a>,
}

impl PreparedCheckpointCapture<'_> {
    pub fn checkpoint_id(&self) -> &str {
        self.receipt.candidate.prepare_id()
    }

    pub fn sources(&self) -> &PreparedSourceSet {
        self.receipt
            .sources
            .as_ref()
            .expect("constructed with sources")
    }

    pub fn bindings(&self) -> &[VersionedSourceBinding] {
        &self.boundary().bindings
    }

    fn boundary(&self) -> &CaptureBoundary {
        self.receipt
            .capture_boundary
            .as_ref()
            .expect("constructed with capture boundary")
    }

    fn require_receipt_in(&self, tx: &Transaction<'_>) -> Result<()> {
        construction_receipt::require_current_sources_in(
            tx,
            &self.actor,
            &self.receipt,
            &self.barrier,
        )
    }

    /// Recheck the owner-specific boundary inside the publication transaction.
    /// The shared constructor separately checks the receipt and barrier.
    fn require_current_boundary_in(
        &self,
        tx: &Transaction<'_>,
        registry: &SourceDriverRegistryReader,
    ) -> Result<()> {
        let boundary = self.boundary();
        let observed = observe_in(
            tx,
            &self.actor,
            self.receipt.origin(),
            &boundary.selection,
            boundary.checkpoint_note.as_deref(),
            self.store.inner.source_limit,
            registry,
        )?;
        if &observed != boundary {
            return Err(Error::Conflict("checkpoint capture boundary changed"));
        }
        for source in &self.sources().sources {
            registry.resolve(&source.prepared.descriptor.pin()?)?;
        }
        Ok(())
    }

    fn require_current_in(&self, tx: &Transaction<'_>) -> Result<SourceDriverRegistryReader> {
        self.require_receipt_in(tx)?;
        let registry = self.store.inner.source_drivers.reader()?;
        self.require_current_boundary_in(tx, &registry)?;
        Ok(registry)
    }

    /// Revalidate before driver execution/inspection. This is not a
    /// publication token or a substitute for the final in-transaction check.
    pub fn revalidate(&self) -> Result<()> {
        let mut connection = self.store.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        self.require_current_in(&tx)?;
        db(tx.rollback())
    }

    /// Record uncertain effect outcome and preserve the complete barrier as
    /// hazards in the same commit. Current view revocation does not allow
    /// forgetting the receipt or freeing an unresolved effect.
    pub fn mark_unknown(self) -> Result<()> {
        construction_receipt::mark_sources_unknown(
            self.store,
            &self.actor,
            &self.receipt,
            &self.barrier,
        )
    }
}

impl SessionStore {
    /// Replay the original source-bearing constructor request without planning
    /// or inspecting external effects. Current disclosure is checked on the
    /// published result; unresolved requests cannot become fresh invocations.
    pub fn checkpoint_replay_capture(
        &self,
        actor: &PrincipalId,
        reference: &str,
        selection: CheckpointCaptureSelection,
        note: Option<&str>,
        request_id: &str,
    ) -> Result<Option<super::CheckpointSnapshot>> {
        label(reference, 2048)?;
        label(request_id, 256)?;
        if let Some(note) = note {
            label(note, 2048)?;
        }
        let selection = selection.canonicalize()?;
        let fingerprint = fingerprint(reference, &selection, note)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction())?;
        let creator = user_for_principal(&tx, actor)?
            .ok_or(Error::Denied("checkpoint caller is inactive or unknown"))?;
        let result = super::replay_in(&tx, actor, &creator, request_id, &fingerprint)?;
        db(tx.commit())?;
        Ok(result)
    }

    /// Trusted lower host seam, not an RPC or a model grant. The caller must
    /// independently admit checkpoint-create and exact source capture
    /// authority. The planner is read-only and must authorize the *complete*
    /// observed set before inspecting driver content. It receives the
    /// owner-minted candidate ID and canonical named binding revisions.
    ///
    /// Planning occurs outside the SQLite transaction. No external effect
    /// may begin until this method returns the durably Prepared capture.
    /// Full source-bearing execution stays closed pending grant realization.
    pub fn checkpoint_prepare_capture(
        &self,
        actor: &PrincipalId,
        reference: &str,
        selection: CheckpointCaptureSelection,
        note: Option<&str>,
        request_id: &str,
        plan: impl FnOnce(&str, &[VersionedSourceBinding]) -> Result<PreparedSourceSet>,
    ) -> Result<PreparedCheckpointCapture<'_>> {
        label(reference, 2048)?;
        label(request_id, 256)?;
        if let Some(note) = note {
            label(note, 2048)?;
        }
        if self.inner.mode != crate::StoreMode::OwnerLock {
            return Err(Error::Denied(
                "checkpoint capture requires the exclusive persistent Store owner",
            ));
        }
        let limit = self.inner.source_limit;
        let selection = selection.canonicalize()?;
        let fingerprint = fingerprint(reference, &selection, note)?;
        let (creator, session, branch, boundary, store_scope) = {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let creator = user_for_principal(&tx, actor)?
                .ok_or(Error::Denied("checkpoint caller is inactive or unknown"))?;
            require_new_request(&tx, &creator, request_id)?;
            let (session, branch) = agent_owner::key_for_ref(&tx, reference)?;
            let registry = self.inner.source_drivers.reader()?;
            let boundary = observe_in(
                &tx,
                actor,
                AgentKey::new(&session, &branch),
                &selection,
                note,
                limit,
                &registry,
            )?;
            boundary.mutation_set()?.require_unleased_in(&tx)?;
            let store_scope = StoreScope::load(&tx)?;
            db(tx.commit())?;
            (creator, session, branch, boundary, store_scope)
        };

        let checkpoint = id("cp")?;
        let mut sources = plan(&checkpoint, &boundary.bindings)?;
        sources.admit(limit)?;
        sources
            .sources
            .sort_unstable_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
        boundary.validate(&sources)?;
        if sources.prepare_id != checkpoint {
            return Err(Error::Conflict(
                "source descriptors belong to another checkpoint candidate",
            ));
        }
        let mutation_set = boundary.mutation_set()?;
        let receipt = ConstructionPreparation {
            operation: ConstructionOperation::CheckpointCreate,
            scope: creator.clone(),
            creator,
            parent_session_id: session,
            parent_branch_id: branch,
            request_id: request_id.to_owned(),
            fingerprint,
            state: PreparationState::Prepared,
            candidate: ConstructionCandidate::Checkpoint { checkpoint },
            task: None,
            sources: Some(sources),
            capture_boundary: Some(boundary),
            restore_boundary: None,
            result_ref: None,
            note: None,
        };

        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        if StoreScope::load(&tx)? != store_scope {
            return Err(Error::Conflict(
                "Store generation changed during checkpoint planning",
            ));
        }
        receipt.require_actor(&tx, actor)?;
        require_new_request(&tx, &receipt.creator, request_id)?;
        let registry = self.inner.source_drivers.reader()?;
        let observed = observe_in(
            &tx,
            actor,
            receipt.origin(),
            &selection,
            note,
            limit,
            &registry,
        )?;
        if receipt.capture_boundary.as_ref() != Some(&observed) {
            return Err(Error::Conflict(
                "checkpoint capture boundary changed during planning",
            ));
        }
        let barrier =
            construction_receipt::claim_sources_in(self, &tx, actor, &receipt, &mutation_set)?;
        db(tx.commit())?;
        Ok(PreparedCheckpointCapture {
            store: self,
            actor: actor.clone(),
            receipt,
            barrier,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::{
        AccountOwner, BillingOwner, CredentialCustody, DriverKey, InvocationIdentity,
        NamedPreparedSource, Outcome, PreparedSourceOperation, RecoveryDescriptor, RecoveryField,
        RecoveryFieldKind, SchemaSupport, SourceDriverRegistry, SourceDriverSchemas, Turn,
        TurnJournal, TurnStart,
    };
    use std::path::PathBuf;

    struct PrivateTemp(PathBuf);
    impl Drop for PrivateTemp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn plan_fixture(
        candidate: &str,
        bindings: &[VersionedSourceBinding],
    ) -> Result<PreparedSourceSet> {
        let sources = bindings
            .iter()
            .rev()
            .map(|version| {
                let binding = &version.binding;
                Ok(NamedPreparedSource {
                    name: binding.name.clone(),
                    prepared: PreparedSourceOperation::new(
                        SourceOperationKind::Capture,
                        RecoveryDescriptor::new(
                            binding.driver.clone(),
                            1,
                            candidate,
                            format!("{candidate}:{}", binding.name.as_str()),
                            vec![RecoveryField::new(
                                "source",
                                RecoveryFieldKind::Reference,
                                &binding.source_ref,
                            )?],
                        )?,
                    )?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(PreparedSourceSet {
            prepare_id: candidate.into(),
            operation: SourceOperationKind::Capture,
            sources,
        })
    }

    #[tokio::test]
    async fn capture_preparation_binds_exact_sources_history_and_receipt() {
        let temp = PrivateTemp(std::env::temp_dir().join(id("aw-capture-prepare").unwrap()));
        let actor = PrincipalId::new("internal:fixture:capture-owner").unwrap();
        let viewer = PrincipalId::new("internal:fixture:capture-viewer").unwrap();
        let driver = DriverKey::builtin("fixture-git").unwrap();
        let registry = SourceDriverRegistry::new();
        let schema = SchemaSupport::new(1, 1).unwrap();
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
        let store =
            SessionStore::open_with_sources(&temp.0, registry, SourceSetLimit::new(3).unwrap())
                .unwrap();
        let (session, branch) = store
            .create(
                &actor,
                "capture",
                crate::SessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let origin = AgentKey::new(&session.id, &branch.id);
        let reference = agent_owner::canonical_ref(&store.connection().unwrap(), origin).unwrap();
        let sandbox = store
            .agent_get(&actor, &reference)
            .unwrap()
            .sandbox_ref
            .unwrap();
        let domain = store.default_domain(&actor).unwrap();
        let viewer_ref = {
            let connection = store.connection().unwrap();
            let user = crate::ensure_user_for_principal(&connection, &viewer).unwrap();
            format!("/d:{}/i:{user}", domain.id)
        };
        let rev = store
            .agent_public_revision(&actor, &session.id, &branch.id)
            .unwrap();
        store
            .agent_access_set(
                &actor,
                &session.id,
                &branch.id,
                &viewer_ref,
                Some(crate::AgentPermissions {
                    view: true,
                    ..Default::default()
                }),
                rev,
            )
            .unwrap();
        assert!(store
            .checkpoint_prepare_capture(
                &viewer,
                &reference,
                CheckpointCaptureSelection::Auto,
                None,
                "host:empty",
                |_, _| panic!("empty automatic set must not inspect sources")
            )
            .is_err());

        // Real active Turn; source realization is still closed, so attach
        // fixture bindings only inside this test. No production rebind path.
        let (_, handle) = store.activate(&actor, &session.id, &branch.id).unwrap();
        let identity = InvocationIdentity {
            actor: actor.clone(),
            account_id: "fixture".into(),
            account_owner: AccountOwner::User(actor.clone()),
            payer: BillingOwner::User(actor.clone()),
            credential_custody: CredentialCustody::UserDevice("fixture".into()),
            credential_version: None,
            inference_executor: "local".into(),
            model: "fixture".into(),
            grant_revision: 1,
        };
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "settle".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        {
            let connection = store.connection().unwrap();
            for (name, policy) in [
                ("code", "required"),
                ("data", "required"),
                ("cache", "ephemeral"),
            ] {
                connection.execute(
                    "INSERT INTO sandbox_source_bindings_current(
                         sandbox_id,source_name,driver_key,binding_schema,source_ref,materialization,
                         access,checkpoint_policy,binding_rev,compatibility
                     ) VALUES(?1,?2,?3,1,?4,?5,'rw',?6,1,'compatible')",
                    params![sandbox,name,crate::json(&driver).unwrap(),
                        format!("fixture-source:{name}"),format!("fixture-checkout:{name}"),policy],
                ).unwrap();
            }
        }
        assert!(store
            .checkpoint_prepare_capture(
                &viewer,
                &reference,
                CheckpointCaptureSelection::Auto,
                None,
                "host:busy",
                |_, _| panic!("active work must refuse before planning")
            )
            .is_err());
        handle
            .finish(Turn {
                id: 1,
                prompt: "settle".into(),
                answer: "settled".into(),
                model: identity.model,
                profile: identity.account_id,
                outcome: Outcome::Completed,
                meta: None,
                error: None,
                actor: Some(actor.clone()),
                tool_records: Vec::new(),
                invocations: Vec::new(),
                context_messages: Vec::new(),
                needs_reconciliation: false,
            })
            .await
            .unwrap();
        let settled = store
            .checkpoint_create_history_only(&actor, &reference, None, "host:history")
            .unwrap();

        assert!(store
            .checkpoint_create_empty_auto(&actor, &reference, None, "host:not-empty-auto",)
            .is_err());

        // Named selection may include ephemeral sources, but not absent
        // names. Planning outside SQLite permits a concurrent logical edit;
        // stale observations cannot reserve effects.
        let named = CheckpointCaptureSelection::Named(vec![
            SourceName::new("code").unwrap(),
            SourceName::new("cache").unwrap(),
        ]);
        assert!(store
            .checkpoint_prepare_capture(
                &viewer,
                &reference,
                CheckpointCaptureSelection::Named(vec![SourceName::new("missing").unwrap()]),
                None,
                "host:missing",
                |_, _| panic!("missing source")
            )
            .is_err());
        assert!(store
            .checkpoint_prepare_capture(
                &viewer,
                &reference,
                named,
                None,
                "host:stale",
                |candidate, bindings| {
                    assert_eq!(
                        bindings
                            .iter()
                            .map(|b| b.binding.name.as_str())
                            .collect::<Vec<_>>(),
                        ["cache", "code"]
                    );
                    let plans = plan_fixture(candidate, bindings)?;
                    let connection = store.connection()?;
                    db(connection.execute(
                        "UPDATE sandbox_source_bindings_current SET binding_rev=binding_rev+1
                     WHERE sandbox_id=?1 AND source_name='code'",
                        [&sandbox],
                    ))?;
                    Ok(plans)
                }
            )
            .is_err());
        assert!(store
            .checkpoint_prepare_capture(
                &viewer,
                &reference,
                CheckpointCaptureSelection::Auto,
                None,
                "host:partial",
                |candidate, bindings| {
                    let mut plans = plan_fixture(candidate, bindings)?;
                    plans.sources.pop();
                    Ok(plans)
                }
            )
            .is_err());

        let prepared = store
            .checkpoint_prepare_capture(
                &viewer,
                &reference,
                CheckpointCaptureSelection::Auto,
                Some("capture"),
                "host:capture",
                |candidate, bindings| {
                    assert_eq!(
                        bindings
                            .iter()
                            .map(|b| b.binding.name.as_str())
                            .collect::<Vec<_>>(),
                        ["code", "data"]
                    );
                    plan_fixture(candidate, bindings)
                },
            )
            .unwrap();
        let candidate = prepared.checkpoint_id().to_owned();
        assert_eq!(prepared.boundary().history_base_id, settled.history_base_id);
        assert_eq!(prepared.bindings()[0].binding_rev, 2);
        assert_eq!(prepared.sources().sources[0].name.as_str(), "code");
        prepared.revalidate().unwrap();
        let set = prepared.boundary().mutation_set().unwrap();
        assert!(store
            .acquire_source_mutation_barrier(&set, "other")
            .is_err());
        assert!(store
            .checkpoint_prepare_capture(
                &viewer,
                &reference,
                CheckpointCaptureSelection::Auto,
                None,
                "host:blocked",
                |_, _| panic!("held source barrier must refuse before planning")
            )
            .is_err());
        assert!(store
            .checkpoint_prepare_capture(
                &viewer,
                "/not-a-target",
                CheckpointCaptureSelection::Auto,
                Some("capture"),
                "host:capture",
                |_, _| panic!("replay must not plan")
            )
            .is_err());
        assert!(store
            .checkpoint_create_history_only(&viewer, &reference, None, "host:capture")
            .is_err());
        {
            let connection = store.connection().unwrap();
            let receipt = construction_receipt::lookup_in(
                &connection,
                ConstructionOperation::CheckpointCreate,
                &prepared.receipt.creator,
                "host:capture",
            )
            .unwrap()
            .unwrap();
            assert_eq!(receipt, prepared.receipt);
            let count: i64 = connection
                .query_row(
                    "SELECT count(*) FROM local_coordination_leases WHERE scope_kind='execution'
                 AND run_id=?1 AND state='active'",
                    [&candidate],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 2);
            let count: i64 = connection
                .query_row("SELECT count(*) FROM checkpoints", [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 1);
        }

        // An exact history boundary and current view are both required at
        // revalidation. The direct history edit is a stale-boundary fixture,
        // not a supported source-bearing writer.
        {
            let connection = store.connection().unwrap();
            let current = crate::branch_in(&connection, &session.id, &branch.id)
                .unwrap()
                .head;
            connection
                .execute(
                    "UPDATE branches SET head=?1 WHERE id=?2",
                    params![branch.head, branch.id],
                )
                .unwrap();
            drop(connection);
            assert!(prepared.revalidate().is_err());
            let connection = store.connection().unwrap();
            connection
                .execute(
                    "UPDATE branches SET head=?1 WHERE id=?2",
                    params![current, branch.id],
                )
                .unwrap();
        }
        prepared.revalidate().unwrap();
        let creator = prepared.receipt.creator.clone();
        let boundary = prepared.boundary().clone();
        let inspected = prepared
            .inspect(|_, bindings, sources| {
                bindings
                    .iter()
                    .zip(&sources.sources)
                    .map(|(version, source)| {
                        let binding = &version.binding;
                        let identity = &source.prepared.descriptor.prepared_identity;
                        aw_agent_core::RecoveryInspection::completed(
                            aw_agent_core::SourceAnchor {
                                name: binding.name.clone(),
                                driver: binding.driver.clone(),
                                anchor_schema: 1,
                                source_ref: binding.source_ref.clone(),
                                access: binding.access,
                                checkpoint_policy: binding.checkpoint_policy,
                                identity: identity.clone(),
                                retention: format!("fixture:{identity}"),
                                fields: Vec::new(),
                            },
                            aw_agent_core::ExternalEvidence::immutable(
                                "fixture_version",
                                identity,
                            )?,
                        )
                    })
                    .collect()
            })
            .unwrap();
        // Revoke view after Store preflight, while the trusted external
        // verifier runs. The final transaction must recheck current ACLs.
        let rev = store
            .agent_public_revision(&actor, &session.id, &branch.id)
            .unwrap();
        assert!(inspected
            .publish::<std::convert::Infallible>(|_, _, _, _, evidence| {
                store.agent_access_set(&actor, &session.id, &branch.id, &viewer_ref, None, rev)?;
                Ok(evidence
                    .iter()
                    .cloned()
                    .map(aw_agent_core::SourcePublicationProof::Immutable)
                    .collect())
            })
            .is_err());
        assert!(store
            .checkpoint_inspect_capture(&viewer, "host:capture", |_, _, _| {
                panic!("revoked view must not disclose recovery descriptors")
            })
            .is_err());
        assert!(store
            .acquire_source_mutation_barrier(&set, "retry")
            .is_err());
        {
            let connection = store.connection().unwrap();
            let receipt = construction_receipt::lookup_in(
                &connection,
                ConstructionOperation::CheckpointCreate,
                &creator,
                "host:capture",
            )
            .unwrap()
            .unwrap();
            assert_eq!(receipt.state, PreparationState::Unknown);
            assert_eq!(receipt.capture_boundary, Some(boundary));
            let count: i64 = connection
                .query_row(
                    "SELECT count(*) FROM local_coordination_leases WHERE scope_kind='execution'
                 AND run_id=?1 AND state='hazard'",
                    [&candidate],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 2);
            assert_eq!(
                connection
                    .query_row("SELECT count(*) FROM checkpoints", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
    }

    #[test]
    fn capture_recovery_reclaims_original_hazards_without_replaying() {
        use aw_agent_core::{
            ExternalEvidence, RecoveryInspection, SourceAccess, SourceAnchor, SourceBinding,
        };

        let temp = PrivateTemp(std::env::temp_dir().join(id("aw-capture-recovery").unwrap()));
        let actor = PrincipalId::new("internal:fixture:capture-recovery").unwrap();
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
        let (session, branch) = store
            .create(&actor, "capture recovery", layout.clone())
            .unwrap();
        let origin = AgentKey::new(&session.id, &branch.id);
        let reference = agent_owner::canonical_ref(&store.connection().unwrap(), origin).unwrap();
        let original = store
            .agent_get(&actor, &reference)
            .unwrap()
            .sandbox_ref
            .unwrap();
        let project = reference.rsplit_once("/a:").unwrap().0;
        let bindings = ["code", "data"].map(|name| SourceBinding {
            name: SourceName::new(name).unwrap(),
            driver: driver.clone(),
            binding_schema: 1,
            source_ref: format!("fixture-source:{name}"),
            materialization: format!("fixture-materialization:{name}"),
            access: SourceAccess::ReadWrite,
            checkpoint_policy: CheckpointPolicy::Required,
        });
        let sandbox = store
            .sandbox_create_fresh_with_sources(&actor, project, &layout, &bindings)
            .unwrap()
            .canonical_ref;
        // Source-bearing Agent realization is intentionally closed. This
        // test-only association exercises the lower capture boundary.
        let bind_fixture = |store: &SessionStore, target: &str| {
            store.connection().unwrap().execute(
                "UPDATE agent_state_current SET sandbox_id=?1 WHERE session_id=?2 AND branch_id=?3",
                params![target, session.id, branch.id],
            ).unwrap();
        };
        bind_fixture(&store, &sandbox);
        let prepared = store
            .checkpoint_prepare_capture(
                &actor,
                &reference,
                CheckpointCaptureSelection::Auto,
                None,
                "host:recover",
                plan_fixture,
            )
            .unwrap();
        let receipt = prepared.receipt.clone();
        let candidate = prepared.checkpoint_id().to_owned();
        let set = prepared.boundary().mutation_set().unwrap();
        let original_owner = store.process_identity().unwrap().unwrap().process_id;
        assert!(store
            .checkpoint_inspect_capture(&actor, "host:recover", |_, _, _| {
                panic!("a live Prepared token cannot be stolen")
            })
            .is_err());
        // Model an effect whose response was lost. The test driver inspects
        // these exact operation-owned files, never creates them on retry.
        let marker = |name: &str| temp.0.join(format!("fixture-effect-{name}"));
        let write_effect = |source: &NamedPreparedSource| {
            std::fs::write(
                marker(source.name.as_str()),
                &source.prepared.descriptor.prepared_identity,
            )
            .unwrap();
        };
        write_effect(&prepared.sources().sources[0]);
        drop(prepared);
        // Keep the real source rows and receipt, but remove the test-only
        // association before reopen (production startup must still deny
        // source-bearing Agent realization). Reinstall the fixture below.
        bind_fixture(&store, &original);
        drop(store);

        let store = SessionStore::open(&temp.0).unwrap();
        assert!(store
            .checkpoint_inspect_capture(&actor, "host:recover", |_, _, _| {
                panic!("unavailable recovery schema must refuse before inspection")
            })
            .is_err());
        drop(store);
        let store = SessionStore::open_with_sources(&temp.0, registry.clone(), limit).unwrap();

        let inspect = |registry: &SourceDriverRegistryReader,
                       observed: &[VersionedSourceBinding],
                       sources: &PreparedSourceSet| {
            assert_eq!(sources, receipt.sources.as_ref().unwrap());
            assert_eq!(
                observed,
                receipt
                    .capture_boundary
                    .as_ref()
                    .unwrap()
                    .bindings
                    .as_slice()
            );
            assert_eq!(sources.prepare_id, candidate);
            observed
                .iter()
                .zip(&sources.sources)
                .map(|(binding, source)| {
                    registry.resolve(&source.prepared.descriptor.pin()?)?;
                    let Ok(version) = std::fs::read_to_string(marker(source.name.as_str())) else {
                        return Ok(RecoveryInspection::Unknown);
                    };
                    assert_eq!(version, source.prepared.descriptor.prepared_identity);
                    let binding = &binding.binding;
                    RecoveryInspection::completed(
                        SourceAnchor {
                            name: binding.name.clone(),
                            driver: binding.driver.clone(),
                            anchor_schema: 1,
                            source_ref: binding.source_ref.clone(),
                            access: binding.access,
                            checkpoint_policy: binding.checkpoint_policy,
                            identity: version.clone(),
                            retention: format!("fixture-retention:{version}"),
                            fields: Vec::new(),
                        },
                        if binding.name.as_str() == "data" {
                            ExternalEvidence::held_fence("fixture_provider", version)?
                        } else {
                            ExternalEvidence::immutable("fixture_version", version)?
                        },
                    )
                })
                .collect::<Result<Vec<_>>>()
        };
        // All claims survive restart, but an incomplete inspection cannot
        // publish or release even its successfully inspected member.
        assert!(store
            .checkpoint_inspect_capture(&actor, "host:recover", inspect)
            .is_err());
        {
            let connection = store.connection().unwrap();
            let current = construction_receipt::lookup_in(
                &connection,
                receipt.operation,
                &receipt.scope,
                &receipt.request_id,
            )
            .unwrap()
            .unwrap();
            assert_eq!(current.state, PreparationState::Unknown);
            assert_eq!(current.sources, receipt.sources);
            assert_eq!(current.capture_boundary, receipt.capture_boundary);
            let count: usize = connection
                .query_row(
                    "SELECT count(*) FROM local_coordination_leases WHERE scope_kind='execution'
                 AND run_id=?1 AND state='hazard' AND holder_process_id<>?2 AND generation>1",
                    params![candidate, original_owner],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 2);
        }
        assert!(store
            .acquire_source_mutation_barrier(&set, "another")
            .is_err());
        // A completed original effect becomes observable later. Inspection
        // still uses the original descriptor, with no new Prepared claim.
        write_effect(&receipt.sources.as_ref().unwrap().sources[1]);
        let inspected = store
            .checkpoint_inspect_capture(&actor, "host:recover", inspect)
            .unwrap();
        assert_eq!(inspected.checkpoint_id(), candidate);
        assert_eq!(inspected.source_set().anchors.len(), 2);
        assert_eq!(inspected.evidence().len(), 2);
        assert!(
            inspected.revalidate().is_err(),
            "stale association cannot be published"
        );
        bind_fixture(&store, &sandbox);
        inspected.revalidate().unwrap();
        assert!(store
            .checkpoint_inspect_capture(&actor, "host:recover", |_, _, _| {
                panic!("live inspection barrier cannot be stolen")
            })
            .is_err());
        inspected.mark_unknown().unwrap();
        assert!(
            store
                .checkpoint_inspect_capture(&actor, "host:recover", |_, _, _| {
                    Ok(vec![
                        RecoveryInspection::NotStarted,
                        RecoveryInspection::NotStarted,
                    ])
                })
                .is_err(),
            "NotStarted is not permission to repeat effects"
        );
        assert!(
            store
                .checkpoint_inspect_capture(
                    &actor,
                    "host:recover",
                    |registry, bindings, sources| {
                        let mut results = inspect(registry, bindings, sources)?;
                        results.swap(0, 1);
                        Ok(results)
                    }
                )
                .is_err(),
            "wrong named anchor results cannot become publishable"
        );
        assert!(store
            .checkpoint_create_history_only(&actor, &reference, None, "host:recover")
            .is_err());
        {
            let connection = store.connection().unwrap();
            let count: usize = connection
                .query_row(
                    "SELECT count(*) FROM audit WHERE action='construction.preparation'
                 AND json_extract(data_json,'$.request_id')='host:recover'
                 AND json_extract(data_json,'$.state')='prepared'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1);
            let count: usize = connection
                .query_row("SELECT count(*) FROM checkpoints", [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 0);
        }
        assert!(store
            .acquire_source_mutation_barrier(&set, "still-hazard")
            .is_err());

        // A saved provider handle is not an immutable publication proof.
        use aw_agent_core::{SourcePublicationFence, SourcePublicationProof};
        use std::sync::{RwLock, RwLockReadGuard};
        let inspected = store
            .checkpoint_inspect_capture(&actor, "host:recover", inspect)
            .unwrap();
        assert!(inspected
            .publish::<std::convert::Infallible>(|_, _, _, _, evidence| {
                Ok(evidence
                    .iter()
                    .cloned()
                    .map(SourcePublicationProof::Immutable)
                    .collect())
            })
            .is_err());

        // The fixture provider owns an actual live fence. Its destructor
        // observes the outcome while still holding the read guard, proving
        // the guard outlives the entire Store transaction.
        struct FixtureFence<'a> {
            evidence: ExternalEvidence,
            _guard: RwLockReadGuard<'a, ()>,
            provider: &'a RwLock<()>,
            store: &'a SessionStore,
            candidate: &'a str,
            committed: bool,
        }
        impl SourcePublicationFence for FixtureFence<'_> {
            fn evidence(&self) -> &ExternalEvidence {
                &self.evidence
            }
        }
        impl Drop for FixtureFence<'_> {
            fn drop(&mut self) {
                assert!(self.provider.try_write().is_err());
                let connection = self.store.connection().unwrap();
                let published: bool = connection
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM checkpoints WHERE checkpoint_id=?1)",
                        [self.candidate],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(published, self.committed);
                let fact: bool = connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM commit_log WHERE kind='checkpoint.created' AND entity_id=?1)",
                    [self.candidate], |row| row.get(0),
                ).unwrap();
                assert_eq!(fact, self.committed);
            }
        }
        let provider = RwLock::new(());
        let verify = |committed| {
            let store = &store;
            let set = &set;
            let receipt = &receipt;
            let provider = &provider;
            let candidate = &candidate;
            move |registry: &SourceDriverRegistryReader,
                  observed: &[VersionedSourceBinding],
                  sources: &PreparedSourceSet,
                  anchors: &aw_agent_core::SourceSet,
                  evidence: &[ExternalEvidence]| {
                assert_eq!(sources, receipt.sources.as_ref().unwrap());
                assert!(store
                    .acquire_source_mutation_barrier(set, "during-verify")
                    .is_err());
                let inspected = inspect(registry, observed, sources)?;
                assert_eq!(inspected.len(), anchors.anchors.len());
                let mut proofs = Vec::new();
                for ((current, anchor), expected) in
                    inspected.into_iter().zip(&anchors.anchors).zip(evidence)
                {
                    let RecoveryInspection::Completed {
                        result,
                        evidence: actual,
                    } = current
                    else {
                        return Err(Error::Conflict("fixture retention disappeared"));
                    };
                    assert_eq!(&result, anchor);
                    assert_eq!(&actual, expected);
                    proofs.push(match actual {
                        ExternalEvidence::Immutable { .. } => {
                            SourcePublicationProof::Immutable(actual)
                        }
                        ExternalEvidence::HeldFence { .. } => {
                            SourcePublicationProof::Held(FixtureFence {
                                evidence: actual,
                                _guard: provider.read().unwrap(),
                                provider,
                                store,
                                candidate,
                                committed,
                            })
                        }
                    });
                }
                Ok(proofs)
            }
        };

        // Force a real failure after the checkpoint row and first anchor
        // have been inserted. The whole transaction, including provenance,
        // must roll back; the original receipt remains recoverable.
        {
            let connection = store.connection().unwrap();
            connection.execute_batch(
                "CREATE TEMP TRIGGER fixture_fail_checkpoint BEFORE INSERT ON checkpoint_sources
                 WHEN NEW.source_name='data'
                 BEGIN SELECT RAISE(ABORT,'fixture publication failure'); END;"
            ).unwrap();
        }
        let inspected = store
            .checkpoint_inspect_capture(&actor, "host:recover", inspect)
            .unwrap();
        assert!(inspected.publish(verify(false)).is_err());
        {
            let connection = store.connection().unwrap();
            connection
                .execute_batch("DROP TRIGGER fixture_fail_checkpoint")
                .unwrap();
            assert_eq!(
                connection
                    .query_row("SELECT count(*) FROM checkpoint_sources", [], |row| row
                        .get::<_, usize>(
                        0
                    ))
                    .unwrap(),
                0
            );
            let current = construction_receipt::lookup_in(
                &connection,
                receipt.operation,
                &receipt.scope,
                &receipt.request_id,
            )
            .unwrap()
            .unwrap();
            assert_eq!(current.state, PreparationState::Unknown);
            assert_eq!(current.sources, receipt.sources);
        }

        let before_rev = store
            .agent_public_revision(&actor, &session.id, &branch.id)
            .unwrap();
        let inspected = store
            .checkpoint_inspect_capture(&actor, "host:recover", inspect)
            .unwrap();
        let snapshot = inspected.publish(verify(true)).unwrap();
        assert_eq!(snapshot.id, candidate);
        assert_eq!(
            snapshot.history_base_id,
            receipt.capture_boundary.as_ref().unwrap().history_base_id
        );
        assert_eq!(snapshot.source_coverage, SourceCoverage::AutoAllRequired);
        assert_eq!(snapshot.sources.len(), 2);
        assert_eq!(
            store
                .agent_public_revision(&actor, &session.id, &branch.id)
                .unwrap(),
            before_rev
        );
        assert!(provider.try_write().is_ok());
        let replay = store
            .checkpoint_replay_capture(
                &actor,
                &reference,
                CheckpointCaptureSelection::Auto,
                None,
                "host:recover",
            )
            .unwrap();
        assert_eq!(replay, Some(snapshot));
        assert!(store
            .checkpoint_replay_capture(
                &actor,
                &reference,
                CheckpointCaptureSelection::Auto,
                Some("changed"),
                "host:recover"
            )
            .is_err());
        assert!(store
            .checkpoint_inspect_capture(&actor, "host:recover", |_, _, _| panic!(
                "applied receipt cannot inspect again"
            ))
            .is_err());
        {
            let connection = store.connection().unwrap();
            let current = construction_receipt::lookup_in(
                &connection,
                receipt.operation,
                &receipt.scope,
                &receipt.request_id,
            )
            .unwrap()
            .unwrap();
            assert_eq!(current.state, PreparationState::Applied);
            assert_eq!(current.result_ref.as_deref(), Some(candidate.as_str()));
            assert_eq!(connection.query_row(
                "SELECT count(*) FROM local_coordination_leases WHERE scope_kind='execution' AND run_id=?1 AND state='released'",
                [&candidate], |row| row.get::<_, usize>(0),
            ).unwrap(), 2);
        }
        store
            .acquire_source_mutation_barrier(&set, "after-publication")
            .unwrap()
            .settle(crate::SourceMutationOutcome::Settled)
            .unwrap();

        assert!(store
            .checkpoint_abandon_capture::<()>(&actor, "host:recover", |_, _, _| panic!(
                "Applied checkpoint must remain retained"
            ))
            .is_err());
        let abandoned = store
            .checkpoint_prepare_capture(
                &actor,
                &reference,
                CheckpointCaptureSelection::Auto,
                None,
                "host:abandon",
                plan_fixture,
            )
            .unwrap();
        let abandoned_id = abandoned.checkpoint_id().to_owned();
        let abandoned_sources = abandoned.sources().clone();
        let paths: Vec<_> = abandoned_sources
            .sources
            .iter()
            .map(|source| {
                temp.0
                    .join(format!("abandoned-{abandoned_id}-{}", source.name.as_str()))
            })
            .collect();
        for path in &paths {
            std::fs::write(path, b"private candidate").unwrap();
        }
        assert!(abandoned
            .abandon::<()>(|_, _, sources| {
                assert_eq!(sources, &abandoned_sources);
                assert!(store
                    .acquire_source_mutation_barrier(&set, "while-abandoning")
                    .is_err());
                std::fs::remove_file(&paths[0])?;
                Err(Error::Conflict("fixture interruption during cleanup"))
            })
            .is_err());
        assert!(!paths[0].exists() && paths[1].is_file());
        assert!(store
            .checkpoint_get_metadata(&actor, &abandoned_id)
            .is_err());
        assert!(store
            .acquire_source_mutation_barrier(&set, "still-abandoned")
            .is_err());
        bind_fixture(&store, &original);
        drop(store);
        let missing = SessionStore::open(&temp.0).unwrap();
        assert!(missing
            .checkpoint_abandon_capture::<()>(&actor, "host:abandon", |_, _, _| panic!(
                "missing driver must retain original custody"
            ))
            .is_err());
        drop(missing);
        let store =
            SessionStore::open_with_sources(&temp.0, registry, SourceSetLimit::new(1).unwrap())
                .unwrap();
        store
            .checkpoint_abandon_capture(&actor, "host:abandon", |_, _, sources| {
                assert_eq!(sources, &abandoned_sources);
                assert!(!paths[0].exists());
                std::fs::remove_file(&paths[1])?;
                Ok(())
            })
            .unwrap();
        store
            .checkpoint_abandon_capture::<()>(&actor, "host:abandon", |_, _, _| {
                panic!("terminal cleanup must not repeat")
            })
            .unwrap();
        assert!(store
            .checkpoint_replay_capture(
                &actor,
                &reference,
                CheckpointCaptureSelection::Auto,
                None,
                "host:abandon",
            )
            .is_err());
        assert!(store
            .checkpoint_get_metadata(&actor, &abandoned_id)
            .is_err());
        assert!(store.checkpoint_get_metadata(&actor, &candidate).is_ok());
        {
            let connection = store.connection().unwrap();
            let rejected = construction_receipt::lookup_in(
                &connection,
                receipt.operation,
                &receipt.scope,
                "host:abandon",
            )
            .unwrap()
            .unwrap();
            assert_eq!(rejected.state, PreparationState::Rejected);
            assert_eq!(rejected.sources.as_ref(), Some(&abandoned_sources));
            assert_eq!(connection.query_row(
                "SELECT count(*) FROM local_coordination_leases WHERE run_id=?1 AND state='released'",
                [&abandoned_id], |row| row.get::<_, usize>(0),
            ).unwrap(), 2);
        }
    }
}
