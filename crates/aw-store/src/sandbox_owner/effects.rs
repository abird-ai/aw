//! Trusted host handoff for source-aware realization. The driver prepares a
//! capability; the existing effect owner starts and settles it. No Process
//! ownership, runtime dispatch or admission-state changes live here.
use super::{
    parse_sandbox_ref, snapshot_with_sources, SandboxGrantObservation, SandboxGrantPolicy,
    SandboxSnapshot,
};
use crate::{
    db, durability, label, now, store_scope::StoreScope, workspace_writer, Error, Result,
    SessionStore, SourceMutationBarrier, SourceMutationOutcome, SourceMutationSet, StoreMode,
};
use aw_agent_core::tool::ToolBindingSnapshot;
use aw_agent_core::{
    NamedSourceAccess, PrincipalId, SandboxDriverDescriptor, SandboxDriverHandle,
    SandboxDriverRegistryReader, SourceAccess, ToolCall,
};
use rusqlite::{params, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::future::Future;

pub(crate) mod journal;
pub use journal::{
    NativeTurnReconciliation, SandboxGraphNodeRecovery, SandboxToolRecovery,
    SandboxToolRecoverySelection, SandboxToolTerminalRef, SourceTurnReconciliation,
};

/// A live decision of the trusted policy/tool authority owner. Implementors
/// must guarantee these exact decisions through launch handoff until dropped.
/// Returning a copy of persisted grant evidence is not such a guarantee.
pub trait SandboxEffectAuthority {
    fn policy(&self) -> &SandboxGrantPolicy;
    fn tool_ceiling(&self) -> &[NamedSourceAccess];

    /// Exact live tool binding for an original journaled tool effect.
    /// Non-tool trusted host operations have no such binding.
    fn tool_binding(&self) -> Option<&ToolBindingSnapshot> {
        None
    }

    /// Original constructor authority, required only for a peer Graph node.
    fn constructor_binding(&self) -> Option<&ToolBindingSnapshot> {
        None
    }

    /// Compose the original synchronous authority owner's final admission
    /// fence with the Store commit. A live external guard remains owned by
    /// self through launch; a ToolBinding publication lock need not cross
    /// an await. Its revocation after admission uses normal Process stop.
    fn with_admission<T>(&self, admit: impl FnOnce() -> Result<T>) -> Result<T>
    where
        Self: Sized,
    {
        admit()
    }
}

/// Direct tools use their original journal identity. A separate trusted
/// host operation may supply its own owned ID, but cannot claim a tool
/// binding or use the reserved journal-linked effect namespace.
#[derive(Clone, Debug, PartialEq)]
pub enum SandboxEffectOrigin {
    Host(String),
    Tool { turn_id: u64, call: ToolCall },
    GraphNode(Box<aw_tools::filesystem::job_manager::RunningNode>),
}

/// Trusted host effect request, not a model selector. This seam does not
/// admit a new Agent Turn or open its held execution admission.
#[derive(Clone, Debug, PartialEq)]
pub struct SandboxEffectRequest {
    pub actor: PrincipalId,
    pub agent_ref: String,
    pub origin: SandboxEffectOrigin,
    /// Complete source set the capability may expose, including reads.
    pub sources: Vec<NamedSourceAccess>,
}

impl SandboxEffectRequest {
    fn canonicalize(&mut self, store: &SessionStore) -> Result<()> {
        label(&self.agent_ref, 2048)?;
        match &self.origin {
            SandboxEffectOrigin::Host(id) => {
                if id.is_empty()
                    || id.len() > 128
                    || id.starts_with(journal::EFFECT_PREFIX)
                    || !id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-:".contains(&b))
                {
                    return Err(Error::Config("invalid host source effect identity".into()));
                }
            }
            SandboxEffectOrigin::GraphNode(node) => {
                node.task()
                    .validate()
                    .map_err(|_| Error::Denied("invalid original Graph Task"))?;
            }
            SandboxEffectOrigin::Tool { turn_id, call } => {
                call.validate_envelope()?;
                if *turn_id == 0 {
                    return Err(Error::Config(
                        "source effect requires an original Turn".into(),
                    ));
                }
            }
        }
        store.source_set_limit().admit(self.sources.len())?;
        for source in &self.sources {
            source.name.validate()?;
        }
        self.sources
            .sort_unstable_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
        if self.sources.is_empty()
            || self
                .sources
                .windows(2)
                .any(|pair| pair[0].name == pair[1].name)
        {
            return Err(Error::Conflict(
                "source effect requires a nonempty unique source set",
            ));
        }
        Ok(())
    }
}

/// Exact authorized inputs for a registered driver. Each source's effective
/// access is the requested mode; the binding maximum remains metadata.
/// The driver must expose only these sources at these effective modes and
/// obey the live host policy, including physical layout and containment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxEffectPlan {
    pub sandbox: SandboxSnapshot,
    pub grant: SandboxGrantObservation,
    pub driver: SandboxDriverDescriptor,
    pub effect_id: String,
}

#[derive(PartialEq, Eq)]
struct Boundary {
    plan: SandboxEffectPlan,
    writer: workspace_writer::WorkspaceWriterInfo,
    store_scope: StoreScope,
    intent: Option<journal::Intent>,
}

impl Boundary {
    fn mutation_set(&self) -> Result<SourceMutationSet> {
        SourceMutationSet::from_binding_refs(
            self.plan
                .grant
                .sources
                .iter()
                .map(|source| &source.binding.binding),
        )
    }
}

fn observe_in(
    store: &SessionStore,
    tx: &Transaction<'_>,
    request: &SandboxEffectRequest,
    driver: &SandboxDriverDescriptor,
    authority: &impl SandboxEffectAuthority,
    admitted: Option<&journal::Intent>,
) -> Result<Boundary> {
    let (effect_id, intent) = match (
        &request.origin,
        authority.tool_binding(),
        authority.constructor_binding(),
    ) {
        (SandboxEffectOrigin::Host(id), None, None) => (id.clone(), None),
        (SandboxEffectOrigin::Tool { turn_id, call }, Some(binding), None) => {
            let intent = journal::admit_in(tx, request, *turn_id, call, binding, admitted)?;
            (intent.effect_id(), Some(intent))
        }
        (SandboxEffectOrigin::GraphNode(node), Some(binding), Some(constructor)) => {
            let intent = journal::node::admit_in(
                tx,
                request,
                node,
                binding,
                constructor,
                &driver.driver,
                admitted,
            )?;
            (intent.effect_id(), Some(intent))
        }
        _ => {
            return Err(Error::Denied(
                "tool source effects require their original journal intent",
            ))
        }
    };
    let mut grant = store.sandbox_grant_preflight_in(
        tx,
        &request.actor,
        &request.agent_ref,
        authority.policy(),
        authority.tool_ceiling(),
    )?;
    // Both collections are in canonical name order. Consume the exact
    // requested subset once; a narrower permission is a refusal, not a
    // silently different capability.
    let mut permitted = grant.sources.into_iter().peekable();
    let mut selected = Vec::with_capacity(request.sources.len());
    for requested in &request.sources {
        while permitted
            .peek()
            .is_some_and(|source| source.binding.binding.name.as_str() < requested.name.as_str())
        {
            permitted.next();
        }
        let mut source = permitted
            .next()
            .filter(|source| {
                source.binding.binding.name == requested.name
                    && source.access.permits(requested.access)
            })
            .ok_or(Error::Denied(
                "requested source access exceeds current Sandbox authority",
            ))?;
        source.access = requested.access;
        selected.push(source);
    }
    grant.sources = selected;
    let sandbox = snapshot_with_sources(tx, &grant.sandbox_ref)?;
    if intent.as_ref().is_some_and(|intent| intent.node.is_some())
        && grant.sources.len() != sandbox.sources.len()
    {
        return Err(Error::Denied(
            "source Graph node requires the complete approved source set",
        ));
    }
    let project = parse_sandbox_ref(&grant.sandbox_ref)?.project;
    let writer = workspace_writer::require_workspace_writer(tx, &project.project)?;
    Ok(Boundary {
        plan: SandboxEffectPlan {
            sandbox,
            grant,
            driver: driver.clone(),
            effect_id,
        },
        writer,
        store_scope: StoreScope::load(tx)?,
        intent,
    })
}

/// Read-only prepared driver capability and live host authority. No effect
/// or source lease exists until start atomically revalidates and admits it.
#[must_use = "start only through the fenced handoff"]
pub struct PreparedSandboxEffect<'a, P, A> {
    store: &'a SessionStore,
    request: SandboxEffectRequest,
    boundary: Boundary,
    // Drop inert capability pins before the guard authorizing them.
    prepared: P,
    authority: A,
}

impl<P, A: SandboxEffectAuthority> PreparedSandboxEffect<'_, P, A> {
    pub fn plan(&self) -> &SandboxEffectPlan {
        &self.boundary.plan
    }

    pub fn authority(&self) -> &A {
        &self.authority
    }
}

impl<P, A: SandboxEffectAuthority> PreparedSandboxEffect<'_, P, A> {
    /// One-shot start whose future itself owns the complete start boundary.
    /// The original effect owner must already have durable intent.
    /// For queued/detached launch ownership use start_with_authority instead.
    /// A dropped/panicking start leaves the durable source claims unresolved.
    /// The result is returned even if launch failed: its owner must settle
    /// the lease according to actual start/terminal evidence.
    pub async fn start<R, F: Future<Output = R>>(
        self,
        start: impl FnOnce(P) -> F,
    ) -> Result<(R, SandboxEffectLease)> {
        self.start_with_authority(|prepared, authority| async move {
            let result = start(prepared).await;
            drop(authority);
            result
        })
        .await
    }

    /// For a start handed to another execution owner (e.g. the bounded
    /// Process spawn lane), transfer the live authority together with the
    /// prepared capability. That owner must retain it through actual launch,
    /// even if this caller stops awaiting the result.
    pub async fn start_with_authority<R, F: Future<Output = R>>(
        self,
        start: impl FnOnce(P, A) -> F,
    ) -> Result<(R, SandboxEffectLease)> {
        let (prepared, authority, lease) = self.admit_with_authority()?;
        let result = start(prepared, authority).await;
        Ok((result, lease))
    }

    /// Admit once and transfer the live capability, authority and source
    /// custody to the actual worker. The caller must keep the whole tuple
    /// together through factual settlement, including when its awaiter is
    /// dropped. No second claim or new execution admission is introduced.
    pub fn admit_with_authority(self) -> Result<(P, A, SandboxEffectLease)> {
        let (prepared, authority, lease, _, _) = self.admit()?;
        Ok((prepared, authority, lease))
    }

    /// Opt-in handoff for a driver that must revalidate at actual OS
    /// launch. The original claim is consumed only once; the lane gets an
    /// owned, non-cloneable revalidation boundary, not settlement authority.
    pub async fn start_with_launch_authority<R, F: Future<Output = R>>(
        self,
        start: impl FnOnce(P, A, SandboxEffectLaunch) -> F,
    ) -> Result<(R, SandboxEffectLease)> {
        let (prepared, authority, boundary, lease) = self.admit_with_launch_authority()?;
        let result = start(prepared, authority, boundary).await;
        Ok((result, lease))
    }

    /// Transfer both the one-shot launch boundary and original settlement
    /// custody to the actual Process owner, without borrowing the submitter.
    pub fn admit_with_launch_authority(
        self,
    ) -> Result<(P, A, SandboxEffectLaunch, SandboxEffectLease)> {
        if self.boundary.intent.is_none()
            || self
                .authority
                .tool_binding()
                .is_none_or(|binding| binding.effect != aw_agent_core::ToolEffect::Process)
        {
            return Err(Error::Denied(
                "fenced launch requires original Process tool intent",
            ));
        }
        let (prepared, authority, lease, request, boundary) = self.admit()?;
        let launch = SandboxEffectLaunch {
            store: lease.barrier.store().clone(),
            request,
            boundary,
            pin: lease.barrier.launch_pin(),
        };
        Ok((prepared, authority, launch, lease))
    }

    fn admit(self) -> Result<(P, A, SandboxEffectLease, SandboxEffectRequest, Boundary)> {
        let Self {
            store,
            request,
            boundary,
            authority,
            prepared,
        } = self;
        let barrier = authority.with_admission(|| {
            let mut connection = store.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let current = observe_in(
                store,
                &tx,
                &request,
                &boundary.plan.driver,
                &authority,
                None,
            )?;
            if current != boundary {
                return Err(Error::Conflict(
                    "Sandbox effect boundary changed during preparation",
                ));
            }
            let set = current.mutation_set()?;
            let barrier =
                store.acquire_source_mutation_barrier_in(&tx, &set, &current.plan.effect_id)?;
            if let Some(intent) = &current.intent {
                journal::claim_effect_in(&tx, intent, &current.plan, &current.store_scope)?;
            }
            db(tx.commit())?;
            Ok(barrier)
        })?;
        let write_capable = boundary
            .plan
            .grant
            .sources
            .iter()
            .any(|source| source.access == SourceAccess::ReadWrite);
        let lease = SandboxEffectLease {
            barrier,
            actor: request.actor.clone(),
            sandbox: boundary.plan.grant.sandbox_ref.clone(),
            effect_id: boundary.plan.effect_id.clone(),
            intent: boundary.intent.clone(),
            write_capable,
        };
        Ok((prepared, authority, lease, request, boundary))
    }
}

/// One-shot, owned final launch boundary of the original admitted source
/// Process. No new journal intent, replay or authority to settle the effect.
#[must_use = "consume only at the original launch boundary"]
pub struct SandboxEffectLaunch {
    store: SessionStore,
    request: SandboxEffectRequest,
    boundary: Boundary,
    pin: crate::source_barrier::SourceMutationPin,
}

impl SandboxEffectLaunch {
    /// Final ordinary Process launch. Hold the live source/tool decision
    /// and original Store exclusion through OS spawn and scope attachment.
    /// The synchronous callback must not perform unrelated writes or await.
    pub fn with_source_authority<R>(
        self,
        source: &impl SandboxEffectAuthority,
        operation: impl FnOnce() -> Result<R>,
    ) -> Result<R> {
        self.with_launch_in(source, |_| operation())
    }

    /// Trusted provider-specific projection. Hold the actual source/tool
    /// authority and provider guard across the read-only Store transaction
    /// and synchronous final spawn. No writes, async work or second effect
    /// belong in the callback. Existing active intent, not future admission.
    pub fn with_vault_projection<R>(
        self,
        source: &impl SandboxEffectAuthority,
        registry: &aw_agent_core::VaultProviderRegistryReader,
        vault: &impl crate::VaultGrantAuthority,
        expected_grant_rev: u64,
        operation: impl FnOnce() -> Result<R>,
    ) -> Result<R> {
        let target = &vault.policy().target;
        if target.agent_ref != self.request.agent_ref
            || target.actor != self.request.actor
            || !source
                .tool_binding()
                .is_some_and(|binding| binding.effect == aw_agent_core::ToolEffect::Process)
        {
            return Err(Error::Denied(
                "Vault projection differs from original Process authority",
            ));
        }
        self.with_launch_in(source, |tx| {
            crate::vault_grants::require_projection_in(
                tx,
                registry,
                vault,
                expected_grant_rev,
                aw_agent_core::VaultProjection::Env,
            )?;
            operation()
        })
    }

    fn with_launch_in<R>(
        self,
        source: &impl SandboxEffectAuthority,
        operation: impl FnOnce(&Transaction<'_>) -> Result<R>,
    ) -> Result<R> {
        source.with_admission(|| {
            let mut connection = self.store.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let current = observe_in(
                &self.store,
                &tx,
                &self.request,
                &self.boundary.plan.driver,
                source,
                self.boundary.intent.as_ref(),
            )?;
            if current != self.boundary {
                return Err(Error::Conflict("original Process launch boundary changed"));
            }
            self.pin.require_held_in(&tx)?;
            // No commit or fallible settlement after an OS spawn: this
            // transaction only excludes writers while the effect is launched.
            operation(&tx)
        })
    }

    pub fn effect_id(&self) -> &str {
        &self.boundary.plan.effect_id
    }
}

/// Factual source-effect settlement, supplied by the original effect owner.
/// Settled includes unsuccessful effects that may have changed sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxEffectOutcome {
    NotStarted,
    Settled,
    Unknown,
}

/// Holds every participating source (reads included) until the original
/// effect owner can prove settlement. Dropping does not release claims.
#[must_use = "settle against factual effect evidence; abandoned claims remain fenced"]
pub struct SandboxEffectLease {
    barrier: SourceMutationBarrier,
    actor: PrincipalId,
    sandbox: String,
    effect_id: String,
    intent: Option<journal::Intent>,
    write_capable: bool,
}

impl SandboxEffectLease {
    /// The original tool returns before the Agent writes ToolFinished.
    /// Keep a positive, trusted physical settlement fact with the exact
    /// claims; the original receipt owner can release them after its commit.
    /// Unknown carries no positive proof and remains an ordinary hazard.
    pub fn defer_until_tool_finished(self, outcome: SandboxEffectOutcome) -> Result<()> {
        if self
            .intent
            .as_ref()
            .is_some_and(|intent| intent.node.is_some())
        {
            return Err(Error::Denied(
                "Graph node requires its original node settlement",
            ));
        }
        self.defer_until_owner_settled(outcome)
    }

    /// Original finite worker proof, projected only by its owning receipt
    /// callback. Select the exact admitted identity, never caller input.
    pub fn defer_until_original_settled(self, outcome: SandboxEffectOutcome) -> Result<()> {
        self.defer_until_owner_settled(outcome)
    }

    pub fn defer_until_node_settled(self, outcome: SandboxEffectOutcome) -> Result<()> {
        if self
            .intent
            .as_ref()
            .is_none_or(|intent| intent.node.is_none())
        {
            return Err(Error::Denied(
                "source custody does not belong to a Graph node",
            ));
        }
        self.defer_until_owner_settled(outcome)
    }

    fn defer_until_owner_settled(self, outcome: SandboxEffectOutcome) -> Result<()> {
        if outcome == SandboxEffectOutcome::Unknown {
            return self.settle(outcome);
        }
        let intent = self.intent.as_ref().ok_or(Error::Conflict(
            "deferred source settlement requires an original tool intent",
        ))?;
        let mut connection = self.barrier.store().connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        journal::record_quiescence_in(&tx, intent, &self.actor, outcome)?;
        self.barrier
            .settle_in(&tx, SourceMutationOutcome::Unknown)?;
        db(tx.commit())
    }

    /// No current grant check here: revocation governs new effects, not
    /// factual settlement. Unknown preserves all source hazards. A known
    /// settled write-capable effect advances Sandbox rev once with release.
    pub fn settle(self, outcome: SandboxEffectOutcome) -> Result<()> {
        self.settle_with(outcome, false)
    }

    // Only the original offline recovery owner may consume independently
    // established proof for a closed Graph without its node callback.
    fn settle_recovered(self, outcome: SandboxEffectOutcome) -> Result<()> {
        self.settle_with(outcome, true)
    }

    fn settle_with(self, outcome: SandboxEffectOutcome, recovery: bool) -> Result<()> {
        let mut connection = self.barrier.store().connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        match self.settle_in(&tx, outcome, recovery) {
            Ok(()) => db(tx.commit()),
            Err(error) => {
                db(tx.rollback())?;
                drop(connection);
                self.barrier.settle(SourceMutationOutcome::Unknown)?;
                Err(error)
            }
        }
    }

    fn settle_in(
        &self,
        tx: &Transaction<'_>,
        outcome: SandboxEffectOutcome,
        recovery: bool,
    ) -> Result<()> {
        if let Some(intent) = &self.intent {
            if outcome != SandboxEffectOutcome::Unknown {
                journal::release_in(tx, intent, &self.actor, outcome, recovery)?;
            }
        }
        self.settle_sources_in(tx, outcome)
    }

    // Only after the original direct or node release fact has committed
    // within this transaction. Physical revision/custody is shared.
    fn settle_sources_in(&self, tx: &Transaction<'_>, outcome: SandboxEffectOutcome) -> Result<()> {
        if outcome == SandboxEffectOutcome::Settled && self.write_capable {
            let changed = db(tx.execute(
                "UPDATE sandboxes_current SET rev=rev+1,updated_at=?1
                 WHERE sandbox_id=?2 AND lifecycle='open' AND rev<9223372036854775807",
                params![now(), self.sandbox],
            ))?;
            if changed != 1 {
                return Err(Error::Conflict(
                    "source effect Sandbox revision unavailable",
                ));
            }
            durability::commit_event(
                tx,
                &durability::tx_id()?,
                "sandbox.sources.changed",
                "sandbox",
                &self.sandbox,
                Some(&self.actor),
                serde_json::json!({"sync_scope":"local","aspect":"sources","effect_id":self.effect_id}),
            )?;
        }
        self.barrier.settle_in(
            tx,
            if outcome == SandboxEffectOutcome::Unknown {
                SourceMutationOutcome::Unknown
            } else {
                SourceMutationOutcome::Settled
            },
        )
    }
}

impl SessionStore {
    /// Trusted host source-effect seam. Resolve the actual current policy
    /// and tool guard outside SQLite, select the exact registered native
    /// driver, then prepare an inert capability without materializing or
    /// starting an effect. The driver must pin and enforce the authorized
    /// source/layout and all other host policy constraints; it cannot use
    /// this observation as filesystem/network/device authority by itself.
    ///
    /// This does not install a runtime resolver or open held Agent
    /// admission. Concrete runtime owners must use the fenced start and
    /// factual settlement path for every source-bearing effect.
    pub fn sandbox_prepare_source_effect<D, P, A>(
        &self,
        mut request: SandboxEffectRequest,
        registry: &SandboxDriverRegistryReader,
        driver: &SandboxDriverHandle<D>,
        resolve: impl FnOnce(&SandboxEffectRequest) -> Result<A>,
        prepare: impl FnOnce(&D, &SandboxEffectPlan, &A) -> Result<P>,
    ) -> Result<PreparedSandboxEffect<'_, P, A>>
    where
        D: Send + Sync + 'static,
        A: SandboxEffectAuthority,
    {
        if self.inner.mode != StoreMode::OwnerLock {
            return Err(Error::Denied(
                "source effect requires the exclusive persistent Store owner",
            ));
        }
        request.canonicalize(self)?;
        let implementation = registry.get(driver)?;
        let authority = resolve(&request)?;
        let boundary = authority.with_admission(|| {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let boundary = observe_in(self, &tx, &request, driver.descriptor(), &authority, None)?;
            boundary.mutation_set()?.require_unleased_in(&tx)?;
            db(tx.commit())?;
            Ok(boundary)
        })?;
        let prepared = prepare(&implementation, &boundary.plan, &authority)?;
        Ok(PreparedSandboxEffect {
            store: self,
            request,
            boundary,
            authority,
            prepared,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent_owner::{self, AgentKey},
        resources, user_for_principal, SandboxPolicyEvidence, SandboxSourceGrant, SessionBinding,
        VersionedSourceBinding,
    };
    use aw_agent_core::{
        CheckpointPolicy, DriverKey, SandboxDriverRegistry, SchemaSupport, SourceBinding,
        SourceDriverRegistry, SourceDriverSchemas, SourceName, SourceSetLimit,
    };
    use std::{
        fs::{File, OpenOptions},
        io::{Read, Write},
        path::PathBuf,
        sync::{RwLock, RwLockReadGuard},
    };

    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct Authority<'a> {
        _guard: RwLockReadGuard<'a, ()>,
        policy: &'a SandboxGrantPolicy,
        tools: &'a [NamedSourceAccess],
    }

    impl SandboxEffectAuthority for Authority<'_> {
        fn policy(&self) -> &SandboxGrantPolicy {
            self.policy
        }
        fn tool_ceiling(&self) -> &[NamedSourceAccess] {
            self.tools
        }
    }

    // Real file handles for this narrow fixture driver; no claim about OS
    // containment. Opening existing files prepares but never changes them.
    struct FileDriver;
    impl FileDriver {
        fn prepare(&self, plan: &SandboxEffectPlan) -> Result<Vec<File>> {
            plan.grant
                .sources
                .iter()
                .map(|source| {
                    OpenOptions::new()
                        .read(true)
                        .write(source.access == SourceAccess::ReadWrite)
                        .open(&source.binding.binding.materialization)
                        .map_err(Error::from)
                })
                .collect()
        }
    }

    #[tokio::test]
    async fn fenced_source_effect_handoff_preserves_live_authority_and_factual_settlement() {
        let temp = Temp(std::env::temp_dir().join(crate::id("aw-source-effect").unwrap()));
        let registry = SourceDriverRegistry::new();
        let driver = DriverKey::builtin("fixture-file").unwrap();
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
        let limit = SourceSetLimit::new(2).unwrap();
        let store = SessionStore::open_with_sources(&temp.0, registry.clone(), limit).unwrap();
        let actor = PrincipalId::new("internal:fixture:source-effect").unwrap();
        let layout = |root: PathBuf| SessionBinding {
            working_directory: root.clone(),
            tool_root: root,
            system_prompt: "fixture".into(),
            profile_hint: "fixture".into(),
            model_hint: "fixture".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (root, root_branch) = store
            .create(&actor, "root", layout(PathBuf::from("/tmp")))
            .unwrap();
        let root_ref = agent_owner::canonical_ref(
            &store.connection().unwrap(),
            AgentKey::new(&root.id, &root_branch.id),
        )
        .unwrap();
        let project = store.agent_get(&actor, &root_ref).unwrap().project_ref;
        let sandbox = format!("{project}/s:{}", crate::id("sbx").unwrap());
        let source_root = temp.0.join("source-layout");
        std::fs::create_dir(&source_root).unwrap();
        let bindings = ["code", "data"].map(|name| {
            let path = source_root.join(name);
            std::fs::write(&path, b"initial").unwrap();
            SourceBinding {
                name: SourceName::new(name).unwrap(),
                driver: driver.clone(),
                binding_schema: 1,
                source_ref: format!("fixture:{name}"),
                materialization: path.to_str().unwrap().into(),
                access: if name == "code" {
                    SourceAccess::ReadWrite
                } else {
                    SourceAccess::ReadOnly
                },
                checkpoint_policy: CheckpointPolicy::Required,
            }
        });
        let (session, branch) = {
            let mut connection = store.connection().unwrap();
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            let binding = layout(source_root.clone());
            let fresh = super::super::publish_fresh_in(
                &tx, &actor, &sandbox, &binding, &bindings, &registry, limit,
            )
            .unwrap();
            let project_key = parse_sandbox_ref(&sandbox).unwrap().project;
            let result = resources::create_in_workspace_with_ids_and_sandbox_tx(
                &tx,
                &actor,
                "source effect",
                &binding,
                &project_key.project,
                None,
                resources::mint_session_candidate_ids().unwrap(),
                Some(&fresh),
            )
            .unwrap();
            tx.commit().unwrap();
            result
        };
        let reference = agent_owner::canonical_ref(
            &store.connection().unwrap(),
            AgentKey::new(&session.id, &branch.id),
        )
        .unwrap();
        let policy = SandboxGrantPolicy {
            agent_ref: reference.clone(),
            sandbox_ref: sandbox.clone(),
            actor: actor.clone(),
            actor_user_id: user_for_principal(&store.connection().unwrap(), &actor)
                .unwrap()
                .unwrap(),
            evidence: SandboxPolicyEvidence {
                policy_ref: "host:fixture-policy".into(),
                policy_digest: [1; 32],
                authority_ref: "host:fixture-authority".into(),
                authority_generation: 1,
            },
            sources: bindings
                .iter()
                .cloned()
                .map(|binding| {
                    let mode = binding.access;
                    SandboxSourceGrant::from_binding(
                        &VersionedSourceBinding {
                            binding,
                            binding_rev: 1,
                        },
                        mode,
                    )
                })
                .collect(),
        };
        store
            .sandbox_grant_set(&actor, &reference, 0, Some(&policy))
            .unwrap();
        let requested = bindings
            .iter()
            .map(|binding| NamedSourceAccess {
                name: binding.name.clone(),
                access: binding.access,
            })
            .collect::<Vec<_>>();
        let request = |effect_id: &str| SandboxEffectRequest {
            actor: actor.clone(),
            agent_ref: reference.clone(),
            origin: SandboxEffectOrigin::Host(effect_id.into()),
            sources: requested.clone(),
        };
        let policy_lock = RwLock::new(());
        let resolve = |_: &SandboxEffectRequest| {
            Ok(Authority {
                _guard: policy_lock.read().unwrap(),
                policy: &policy,
                tools: &requested,
            })
        };
        let drivers = SandboxDriverRegistry::new();
        let reader = drivers.reader().unwrap();
        let key = DriverKey::builtin("fixture-realization").unwrap();
        let handle = drivers.register(key.clone(), FileDriver).unwrap();
        assert!(drivers.register(key.clone(), FileDriver).is_err());
        let foreign = SandboxDriverRegistry::new();
        foreign.register(key, FileDriver).unwrap();
        assert!(foreign.reader().unwrap().get(&handle).is_err());
        drivers
            .register(DriverKey::builtin("other").unwrap(), ())
            .unwrap();
        let prepare =
            |driver: &FileDriver, plan: &SandboxEffectPlan, _: &Authority<'_>| driver.prepare(plan);
        let set = SourceMutationSet::from_bindings(&bindings).unwrap();
        let agent_before = store.agent_get(&actor, &reference).unwrap();
        let sandbox_before = store.sandbox_get(&actor, &sandbox).unwrap();

        let mut too_broad = request("host:denied");
        too_broad.sources[1].access = SourceAccess::ReadWrite;
        assert!(store
            .sandbox_prepare_source_effect(
                too_broad,
                &reader,
                &handle,
                resolve,
                |_, _, _| -> Result<Vec<File>> { panic!("unauthorized source cannot prepare") },
            )
            .is_err());

        // Preparation is inert. Revocation committed during preparation
        // prevents the final handoff, even while the provider guard is live.
        let prepared = store
            .sandbox_prepare_source_effect(
                request("host:stale"),
                &reader,
                &handle,
                resolve,
                prepare,
            )
            .unwrap();
        store
            .acquire_source_mutation_barrier(&set, "host:before-start")
            .unwrap()
            .settle(SourceMutationOutcome::Settled)
            .unwrap();
        store
            .sandbox_grant_set(&actor, &reference, 1, None)
            .unwrap();
        assert!(prepared
            .start(|_| async { panic!("revoked grant cannot start") })
            .await
            .is_err());
        assert!(policy_lock.try_write().is_ok());
        store
            .sandbox_grant_set(&actor, &reference, 2, Some(&policy))
            .unwrap();

        let prepared = store
            .sandbox_prepare_source_effect(
                request("host:write"),
                &reader,
                &handle,
                resolve,
                prepare,
            )
            .unwrap();
        let (result, lease) = prepared
            .start(|mut files| {
                let policy_lock = &policy_lock;
                let bindings = &bindings;
                let store = &store;
                async move {
                    assert!(policy_lock.try_write().is_err());
                    for binding in bindings {
                        let scope = SourceMutationSet::from_bindings(std::slice::from_ref(binding))
                            .unwrap();
                        assert!(store
                            .acquire_source_mutation_barrier(&scope, "host:overlap")
                            .is_err());
                    }
                    files[0].write_all(b"changed").unwrap();
                    let mut data = String::new();
                    files[1].read_to_string(&mut data).unwrap();
                    assert_eq!(data, "initial");
                    assert!(files[1].write_all(b"forbidden").is_err());
                    data
                }
            })
            .await
            .unwrap();
        assert_eq!(result, "initial");
        assert!(policy_lock.try_write().is_ok());
        assert!(store
            .acquire_source_mutation_barrier(&set, "host:while-running")
            .is_err());
        // Factual settlement is independent of future authority.
        store
            .sandbox_grant_set(&actor, &reference, 3, None)
            .unwrap();
        lease.settle(SandboxEffectOutcome::Settled).unwrap();
        assert_eq!(
            store.sandbox_get(&actor, &sandbox).unwrap().rev,
            sandbox_before.rev + 1
        );
        assert_eq!(store.agent_get(&actor, &reference).unwrap(), agent_before);
        assert_eq!(std::fs::read(source_root.join("code")).unwrap(), b"changed");
        assert!(store.activate(&actor, &session.id, &branch.id).is_err());

        store
            .sandbox_grant_set(&actor, &reference, 4, Some(&policy))
            .unwrap();
        let prepared = store
            .sandbox_prepare_source_effect(
                request("host:not-started"),
                &reader,
                &handle,
                resolve,
                prepare,
            )
            .unwrap();
        let ((), lease) = prepared.start(|_| async {}).await.unwrap();
        lease.settle(SandboxEffectOutcome::NotStarted).unwrap();
        assert_eq!(
            store.sandbox_get(&actor, &sandbox).unwrap().rev,
            sandbox_before.rev + 1
        );
        let prepared = store
            .sandbox_prepare_source_effect(
                request("host:unknown"),
                &reader,
                &handle,
                resolve,
                prepare,
            )
            .unwrap();
        let ((), lease) = prepared.start(|_| async {}).await.unwrap();
        lease.settle(SandboxEffectOutcome::Unknown).unwrap();
        assert!(store
            .acquire_source_mutation_barrier(&set, "host:unknown-conflict")
            .is_err());
        drop(store);
        let store = SessionStore::open_with_sources(&temp.0, registry, limit).unwrap();
        assert!(store
            .acquire_source_mutation_barrier(&set, "host:restart-conflict")
            .is_err());
        assert_eq!(
            store.sandbox_get(&actor, &sandbox).unwrap().rev,
            sandbox_before.rev + 1
        );
        assert!(store.activate(&actor, &session.id, &branch.id).is_err());
    }
}
