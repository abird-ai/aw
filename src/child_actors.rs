//! AW-P6-7 hosted child-Agent binding over existing, authoritative SQLite
//! subagent Sessions. This is NOT another Agent, store, provider or scheduler.
//! The normal CLI does not install it until an authenticated root Actor owner
//! exists. Its only execution path is AgentSessionController + original Agent.
use aw::ToolMode;
use aw_agent_core::{
    control::AgentSessionController,
    runtime::{Delivery, HostRuntime, SessionBinding, SiteChildRequest, SiteTransferBinding},
    Agent, CancellationToken, DecisionKind, Error, PrincipalId, ProviderSelection, Result,
    ToolContext,
};
use aw_agent_kernel::{
    actor::{ActorDescriptor, ActorKey, ActorKind, ActorState},
    capability::{Effect, SourceKind},
    envelope::{Access, Envelope, Network},
    message::{ActorMessage, ControlKind, Kind, Scope, TurnTarget},
};
use aw_store::{SessionStore, SubagentInfo};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    num::NonZeroU64,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

#[derive(Clone)]
pub struct Service {
    inner: Arc<Inner>,
}
struct Inner {
    store: SessionStore,
    runtime: Arc<Mutex<HostRuntime>>,
    owner: PrincipalId,
    root: ActorKey,
    root_epoch: Option<NonZeroU64>,
    root_session: String,
    // Volatile identity mapping only, never durable replay state or grant.
    children: Mutex<BTreeMap<String, BoundChild>>,
}
#[derive(Clone)]
struct BoundChild {
    key: ActorKey,
    epoch: Option<NonZeroU64>,
    subagent_id: String,
    next_message: Arc<AtomicU64>,
}
pub struct ChildLease {
    host: Service,
    pub key: ActorKey,
    parent: ActorKey,
    parent_epoch: Option<NonZeroU64>,
    child_epoch: Option<NonZeroU64>,
    subagent_id: String,
    child_session: String,
    controller: AgentSessionController,
}
impl Service {
    pub fn new(
        store: SessionStore,
        runtime: Arc<Mutex<HostRuntime>>,
        owner: PrincipalId,
        root: ActorKey,
        root_epoch: Option<NonZeroU64>,
        root_session: impl Into<String>,
    ) -> Result<Self> {
        let root_session = root_session.into();
        let stored = store.session(&owner, &root_session)?;
        if stored.owner != owner {
            return Err(Error::Denied(
                "child Actor owner must own the actual root Session",
            ));
        }
        {
            let runtime = runtime
                .lock()
                .map_err(|_| Error::Storage("Actor Runtime lock poisoned"))?;
            let binding = runtime.session_binding(&owner, root, root_epoch)?;
            let descriptor = runtime.actor(&owner, root, root_epoch)?;
            if binding.session_id() != root_session
                || descriptor.kind != ActorKind::Agent
                || descriptor.parent.is_some()
            {
                return Err(Error::Denied(
                    "root Actor must be bound to exact owned Session",
                ));
            }
        }
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                runtime,
                owner,
                root,
                root_epoch,
                root_session,
                children: Mutex::new(BTreeMap::new()),
            }),
        })
    }
    /// Step-stage decision admission on the SAME authenticated parent Actor,
    /// actual owned Session branch and current provider/account/grant identity.
    /// The existing routing/planner/guardian journal is still the sole paid
    /// attempt/budget authority; this check never starts a provider.
    pub fn authorize_decision_step(
        &self,
        context: &ToolContext,
        kind: DecisionKind,
        selected: &ProviderSelection,
    ) -> Result<()> {
        selected.revalidate()?;
        if context.cancellation.is_cancelled()
            || context.turn_id == 0
            || context.actor != self.inner.owner
            || context.identity.actor != self.inner.owner
            || selected.identity().actor != self.inner.owner
        {
            return Err(Error::Denied(
                "decision Step owner, payer or cancellation invalid",
            ));
        }
        if !matches!(
            kind,
            DecisionKind::Routing
                | DecisionKind::Planner
                | DecisionKind::Guardian
                | DecisionKind::Summary
        ) {
            return Err(Error::Denied("unknown host decision Step kind"));
        }
        let scope = context.scope.as_ref().ok_or(Error::Denied(
            "decision Step requires a durable authenticated parent Session/branch",
        ))?;
        let parent = self
            .inner
            .store
            .session(&self.inner.owner, &scope.session_id)?;
        if parent.owner != self.inner.owner {
            return Err(Error::Denied("decision Step Session owner changed"));
        }
        let branch = self
            .inner
            .store
            .branches(&self.inner.owner, &scope.session_id)?
            .into_iter()
            .find(|b| b.id == scope.branch_id)
            .ok_or(Error::Denied("decision Step parent branch missing"))?;
        if branch.active_run.is_none() {
            return Err(Error::Conflict(
                "decision Step needs existing active journaled parent Turn",
            ));
        }
        let children = self
            .inner
            .children
            .lock()
            .map_err(|_| Error::Storage("child Actor registry lock poisoned"))?;
        let binding = if scope.session_id == self.inner.root_session {
            BoundChild {
                key: self.inner.root,
                epoch: self.inner.root_epoch,
                subagent_id: String::new(),
                next_message: Arc::new(AtomicU64::new(2)),
            }
        } else {
            children
                .get(&scope.session_id)
                .cloned()
                .ok_or(Error::Denied(
                    "decision Step has no live bound parent Actor",
                ))?
        };
        let runtime = self
            .inner
            .runtime
            .lock()
            .map_err(|_| Error::Storage("Actor Runtime lock poisoned"))?;
        let descriptor = runtime.actor(&self.inner.owner, binding.key, binding.epoch)?;
        let envelope = runtime.envelope(&self.inner.owner, binding.key, binding.epoch)?;
        if !matches!(descriptor.state, ActorState::Running | ActorState::Waiting)
            || envelope.site() != context.execution_site
        {
            return Err(Error::Denied(
                "decision Step parent Actor not running at exact site",
            ));
        }
        if scope.session_id != self.inner.root_session {
            let info = self
                .inner
                .store
                .subagent(&self.inner.owner, &binding.subagent_id)?;
            if info.child_session_id != scope.session_id || info.state != "running" {
                return Err(Error::Denied(
                    "decision Step child Session no longer running",
                ));
            }
        }
        Ok(())
    }
    /// Fail before ANY filesystem/Git side effect if the root Actor's exact
    /// current Envelope did not explicitly delegate this built-in tool mode.
    /// This is host-only potential authority; it never registers a tool.
    pub fn preflight_child_tool_mode(&self, mode: ToolMode) -> Result<()> {
        let runtime = self
            .inner
            .runtime
            .lock()
            .map_err(|_| Error::Storage("Actor Runtime lock poisoned"))?;
        let envelope =
            runtime.envelope(&self.inner.owner, self.inner.root, self.inner.root_epoch)?;
        let needed = match mode {
            ToolMode::Off => &[][..],
            ToolMode::ReadOnly => &[("read", Effect::Read)][..],
            ToolMode::Workspace => &[
                ("read", Effect::Read),
                ("write", Effect::Write),
                ("edit", Effect::Write),
            ][..],
            ToolMode::Trusted => return Err(Error::Denied("trusted child Bash never permitted")),
        };
        for (name, effect) in needed {
            let grant = aw_agent_kernel::envelope::ExactGrant {
                name: (*name).into(),
                kind: SourceKind::Host,
                source: "trusted-host".into(),
                source_epoch: 1,
                effect: *effect,
            };
            if !envelope.allows(&grant) {
                return Err(Error::Denied(
                    "root Actor lacks exact host grant for cross-site child tool mode",
                ));
            }
        }
        Ok(())
    }
    /// Verify the authoritative managed worktree is fully bound and still
    /// exactly the child Session's canonical tool root before minting a new
    /// execution-site authority. No strings from a model are trusted here.
    fn authorize_site_transfer(
        &self,
        info: &SubagentInfo,
        child: &aw_store::SessionInfo,
        source_site: &str,
        target_site: &str,
    ) -> Result<SiteTransferBinding> {
        if target_site != format!("subagent:{}", info.id)
            || !matches!(info.tool_mode.as_str(), "read-only" | "workspace")
        {
            return Err(Error::Denied(
                "child execution site is not its managed worktree site",
            ));
        }
        let stored = self
            .inner
            .store
            .subagent_execution_binding_for_subagent(&self.inner.owner, &info.id)?
            .ok_or(Error::Denied(
                "tool-enabled child has no durable execution binding",
            ))?;
        if stored.state != "bound"
            || stored.child_session_id.as_deref() != Some(&info.child_session_id)
            || stored.parent_session_id != info.parent_session_id
            || stored.parent_branch_id != info.parent_branch_id
            || stored.parent_turn_id != info.parent_turn_id
            || stored.tool_mode != info.tool_mode
            || stored.delegation_root_id != info.delegation_root_id
            || stored.delegation_allocation_id != info.delegation_allocation_id
            || stored.execution_root == stored.parent_repo_root
        {
            return Err(Error::Denied(
                "stored worktree/site does not match durable child lineage",
            ));
        }
        let canonical = stored.execution_root.canonicalize().map_err(Error::Io)?;
        let tool_root = child.binding.tool_root.canonicalize().map_err(Error::Io)?;
        let cwd = child
            .binding
            .working_directory
            .canonicalize()
            .map_err(Error::Io)?;
        let repo = stored.parent_repo_root.canonicalize().map_err(Error::Io)?;
        let parent = self
            .inner
            .store
            .session(&self.inner.owner, &info.parent_session_id)?;
        let parent_root = parent.binding.tool_root.canonicalize().map_err(Error::Io)?;
        if canonical != tool_root
            || canonical != cwd
            || !parent_root.starts_with(&repo)
            || canonical.starts_with(&repo)
            || canonical == repo
        {
            return Err(Error::Denied(
                "execution-site worktree escaped verified parent repository",
            ));
        }
        SiteTransferBinding::from_trusted_host(
            &info.parent_session_id,
            &info.child_session_id,
            source_site,
            target_site,
            &stored.id,
        )
    }
    /// Bind only AFTER the original SessionStore has committed child creation
    /// and Running lifecycle. Tool-enabled children with a different execution
    /// site are REJECTED until a separately authorized cross-site Actor bridge
    /// exists; they continue to use the unchanged existing child path when
    /// this opt-in service is absent.
    pub fn attach(
        &self,
        durable: &SubagentInfo,
        agent: &Agent,
        selected: &ProviderSelection,
        task: &str,
        next_turn: u64,
    ) -> Result<ChildLease> {
        selected.revalidate()?;
        let info = self.inner.store.subagent(&self.inner.owner, &durable.id)?;
        if info.id != durable.id
            || info.state != "running"
            || info.parent_session_id != durable.parent_session_id
            || info.parent_branch_id != durable.parent_branch_id
            || info.parent_turn_id != durable.parent_turn_id
            || info.child_session_id != durable.child_session_id
            || info.depth != durable.depth
            || info.task_sha256 != durable.task_sha256
            || info.tool_mode != durable.tool_mode
            || info.mode != durable.mode
            || info.delegation_root_id != durable.delegation_root_id
            || info.delegation_allocation_id != durable.delegation_allocation_id
            || info.identity != *selected.identity()
            || info.identity.actor != self.inner.owner
            || info.identity != agent.default_identity()?
            || next_turn != agent.history_state().next_turn_id
            || next_turn == 0
            || task.trim().is_empty()
            || task.len() > aw::config::MAX_PROMPT_BYTES
            || format!("{:x}", Sha256::digest(task.as_bytes())) != info.task_sha256
        {
            return Err(Error::Denied(
                "child Actor requires exact running durable Session/selection",
            ));
        }
        let child = self
            .inner
            .store
            .session(&self.inner.owner, &info.child_session_id)?;
        if child.owner != self.inner.owner || child.id != info.child_session_id {
            return Err(Error::Denied("child Actor Session ownership mismatch"));
        }
        let mut children = self
            .inner
            .children
            .lock()
            .map_err(|_| Error::Storage("child Actor registry lock poisoned"))?;
        if children.contains_key(&info.child_session_id) {
            return Err(Error::Conflict(
                "child Session already attached; never replay its task",
            ));
        }
        let parent = if info.parent_session_id == self.inner.root_session {
            BoundChild {
                key: self.inner.root,
                epoch: self.inner.root_epoch,
                subagent_id: String::new(),
                next_message: Arc::new(AtomicU64::new(2)),
            }
        } else {
            children
                .get(&info.parent_session_id)
                .cloned()
                .ok_or(Error::Denied(
                    "child's direct parent has no live authenticated Actor",
                ))?
        };
        let mut runtime = self
            .inner
            .runtime
            .lock()
            .map_err(|_| Error::Storage("Actor Runtime lock poisoned"))?;
        let parent_actor = runtime.actor(&self.inner.owner, parent.key, parent.epoch)?;
        let ceiling = runtime
            .envelope(&self.inner.owner, parent.key, parent.epoch)?
            .clone();
        let transfer = if ceiling.site() != agent.tool_execution_site() {
            Some(self.authorize_site_transfer(
                &info,
                &child,
                ceiling.site(),
                agent.tool_execution_site(),
            )?)
        } else if info.tool_mode != "off" {
            return Err(Error::Denied(
                "tool-enabled child must use verified isolated execution site",
            ));
        } else {
            None
        };
        if info.depth != parent_actor.depth.checked_add(1)
            || info.delegation_root_id.is_none()
            || info.delegation_allocation_id.is_none()
            || parent_actor.depth >= ceiling.budget().max_depth
        {
            return Err(Error::Limit(
                "durable delegation depth/root/allocation differs from Actor lineage ceiling",
            ));
        }
        let grants = agent.requested_tool_envelope_grants()?;
        // A local child Actor is not a general network-capable capability
        // container. Pin to the *existing, audited* subagent tool taxonomy;
        // a registered host/MCP extension cannot smuggle remote I/O into a
        // narrower Network::Deny Envelope merely by claiming Effect::Read.
        if grants.iter().any(|grant| {
            grant.kind != SourceKind::Host
                || grant.source != "trusted-host"
                || grant.source_epoch != 1
                || !matches!(
                    (grant.name.as_str(), grant.effect),
                    ("read", Effect::Read)
                        | ("write" | "edit", Effect::Write)
                        | ("subagent", Effect::Process)
                )
        }) {
            return Err(Error::Denied(
                "child Actor permits only original exact host read/write/edit/delegation tools",
            ));
        }
        if grants.iter().any(|grant| !ceiling.allows(grant)) {
            return Err(Error::Denied(
                "child requested tool authority absent from parent Envelope",
            ));
        }
        // Child receives only its currently granted/registered tools, never
        // the parent's other capabilities. Actual ToolPolicy still narrows.
        let has_process = grants
            .iter()
            .any(|g| matches!(g.effect, Effect::Process | Effect::Unknown));
        let writes = grants
            .iter()
            .any(|g| matches!(g.effect, Effect::Write | Effect::Process | Effect::Unknown));
        let mut spec = ceiling.spec().clone();
        if transfer.is_some() {
            // A new Actor gets its new site. The existing parent Actor's
            // Envelope and provider/session identity never move in place.
            spec.site = agent.tool_execution_site().to_owned();
        }
        spec.revision = ceiling
            .revision()
            .checked_add(1)
            .ok_or(Error::Limit("Envelope generation exhausted"))?;
        spec.grants = grants;
        spec.access = if writes {
            Access::ReadWrite
        } else {
            Access::ReadOnly
        };
        spec.process = has_process;
        spec.tty = false;
        spec.microphone = false;
        spec.speaker = false;
        spec.network = Network::Deny;
        // The durable delegation root/child allocation remains the stricter
        // source of child count, depth and model-budget truth.
        // Match every currently exposed tool's exact source/epoch/effect
        // against the new reduced ceiling BEFORE admitting the Child Actor.
        agent.validate_runtime_envelope(
            &Envelope::new(spec.clone())
                .map_err(|_| Error::Denied("invalid narrowed child Envelope"))?,
        )?;
        let session = SessionBinding::from_trusted_host(info.child_session_id.clone(), None)?;
        let key = if let Some(ref transfer) = transfer {
            runtime.bind_site_child_agent(
                &self.inner.owner,
                SiteChildRequest {
                    parent: parent.key,
                    parent_epoch: parent.epoch,
                    child: session,
                    spec,
                    transfer,
                    label: &info.title,
                },
            )?
        } else {
            runtime.bind_child_agent(
                &self.inner.owner,
                parent.key,
                parent.epoch,
                session,
                spec,
                &info.title,
            )?
        };
        runtime.record_state(&self.inner.owner, key, None, ActorState::Running)?;
        drop(runtime);
        let controller = AgentSessionController::new(
            self.inner.runtime.clone(),
            self.inner.owner.clone(),
            key,
            None,
        )?;
        let lease = ChildLease {
            host: self.clone(),
            key,
            parent: parent.key,
            parent_epoch: parent.epoch,
            child_epoch: None,
            subagent_id: info.id.clone(),
            child_session: info.child_session_id.clone(),
            controller,
        };
        children.insert(
            info.child_session_id,
            BoundChild {
                key,
                epoch: None,
                subagent_id: info.id,
                next_message: Arc::new(AtomicU64::new(2)),
            },
        );
        // The single durable child task is admitted exactly once through
        // the SAME Actor Message hooks. If a hook consumes/defers or rejects
        // it, no child provider is started and its Actor is marked Unknown
        // for explicit owner inspection; never auto-retry a phantom input.
        if let Err(error) = lease.admit_task(task, next_turn) {
            let mut r = self
                .inner
                .runtime
                .lock()
                .map_err(|_| Error::Storage("Actor Runtime lock poisoned"))?;
            let _ = r.record_state(&self.inner.owner, key, None, ActorState::Unknown);
            return Err(error);
        }
        Ok(lease)
    }
    /// Host-only list: query the original SQLite SessionStore for factual
    /// child progress; Actor state alone is never a durable completion fact.
    pub fn list(&self) -> Result<Vec<(SubagentInfo, ActorDescriptor)>> {
        let children = self
            .inner
            .children
            .lock()
            .map_err(|_| Error::Storage("child Actor registry lock poisoned"))?;
        let runtime = self
            .inner
            .runtime
            .lock()
            .map_err(|_| Error::Storage("Actor Runtime lock poisoned"))?;
        children
            .iter()
            .map(|(session, bound)| {
                let info = self
                    .inner
                    .store
                    .subagent(&self.inner.owner, &bound.subagent_id)?;
                if info.child_session_id != *session {
                    return Err(Error::Conflict(
                        "attached Actor lost its durable child Session",
                    ));
                }
                let actor = runtime.actor(&self.inner.owner, bound.key, bound.epoch)?;
                Ok((info, actor))
            })
            .collect()
    }
    /// Exactly scoped parent→child control. Only the owning host with the
    /// current durable parent/child Session association can signal it. This
    /// memory-only acceptance is never a paid/effect receipt or a durable CAS.
    pub fn signal(&self, subagent_id: &str, kind: ControlKind, turn: u64) -> Result<Delivery> {
        if turn == 0
            || !matches!(
                kind,
                ControlKind::Cancel(Scope::Turn)
                    | ControlKind::Freeze(Scope::Turn | Scope::Actor)
                    | ControlKind::Resume(Scope::Turn | Scope::Actor)
            )
        {
            return Err(Error::Denied("unsupported or unscoped child control"));
        }
        let info = self.inner.store.subagent(&self.inner.owner, subagent_id)?;
        if info.state != "running" {
            return Err(Error::Conflict("child already settled"));
        }
        let children = self
            .inner
            .children
            .lock()
            .map_err(|_| Error::Storage("child Actor registry lock poisoned"))?;
        let target = children
            .get(&info.child_session_id)
            .ok_or(Error::Denied("no authenticated live child Actor"))?;
        if target.subagent_id != info.id {
            return Err(Error::Denied("child Session/Actor mapping changed"));
        }
        let parent = if info.parent_session_id == self.inner.root_session {
            BoundChild {
                key: self.inner.root,
                epoch: self.inner.root_epoch,
                subagent_id: String::new(),
                next_message: Arc::new(AtomicU64::new(2)),
            }
        } else {
            children
                .get(&info.parent_session_id)
                .cloned()
                .ok_or(Error::Denied("parent Actor not live"))?
        };
        let mut runtime = self
            .inner
            .runtime
            .lock()
            .map_err(|_| Error::Storage("Actor Runtime lock poisoned"))?;
        let descriptor = runtime.actor(&self.inner.owner, target.key, target.epoch)?;
        if descriptor.parent != Some(parent.key) {
            return Err(Error::Denied("child not directly owned by that Actor"));
        }
        runtime.actor(&self.inner.owner, parent.key, parent.epoch)?;
        // The one-Turn child runner is not a general idle mailbox: freezing
        // it before claiming the initial task would falsely settle it as a
        // missing Turn. Controls here require the actual active reservation.
        if !runtime.has_active_turn(&self.inner.owner, target.key, target.epoch, turn)? {
            return Err(Error::Conflict("child has no active Turn to control"));
        }
        let seq = target
            .next_message
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| Error::Limit("child control sequence exhausted"))?;
        runtime.send(
            &self.inner.owner,
            target.epoch,
            ActorMessage {
                from: parent.key,
                to: target.key,
                id: seq,
                source_sequence: seq,
                kind: Kind::Control(kind),
                turn: TurnTarget::Exact(turn),
                priority: 0,
                payload: String::new(),
                content_type: aw_agent_kernel::message::ContentType::Text,
            },
            turn,
        )
    }
    /// Host-only factual view: never publish a child answer or its private
    /// transcript as root Agent output by querying Actor metadata.
    pub fn status(&self, subagent_id: &str) -> Result<(SubagentInfo, ActorDescriptor)> {
        let info = self.inner.store.subagent(&self.inner.owner, subagent_id)?;
        let children = self
            .inner
            .children
            .lock()
            .map_err(|_| Error::Storage("child Actor registry lock poisoned"))?;
        let bound = children
            .get(&info.child_session_id)
            .ok_or(Error::Denied("child has no current Actor attachment"))?;
        if bound.subagent_id != info.id {
            return Err(Error::Denied("child Actor binding mismatch"));
        }
        let runtime = self
            .inner
            .runtime
            .lock()
            .map_err(|_| Error::Storage("Actor Runtime lock poisoned"))?;
        let actor = runtime.actor(&self.inner.owner, bound.key, bound.epoch)?;
        Ok((info, actor))
    }
    /// Read the immutable current narrow child authority projection. This
    /// does not grant any tool or override its current ToolPolicy/source epoch.
    pub fn envelope(&self, subagent_id: &str) -> Result<Envelope> {
        let info = self.inner.store.subagent(&self.inner.owner, subagent_id)?;
        let children = self
            .inner
            .children
            .lock()
            .map_err(|_| Error::Storage("child Actor registry lock poisoned"))?;
        let bound = children
            .get(&info.child_session_id)
            .ok_or(Error::Denied("child Actor not currently attached"))?;
        if bound.subagent_id != info.id {
            return Err(Error::Denied("child Session/Actor owner mismatch"));
        }
        let runtime = self
            .inner
            .runtime
            .lock()
            .map_err(|_| Error::Storage("Actor Runtime lock poisoned"))?;
        Ok(runtime
            .envelope(&self.inner.owner, bound.key, bound.epoch)?
            .clone())
    }
    /// Explicit bounded wait service; polls authoritative durable facts and
    /// creates no background provider or orphan task of its own.
    pub async fn wait(
        &self,
        subagent_id: &str,
        cancel: CancellationToken,
        deadline: std::time::Duration,
    ) -> Result<SubagentInfo> {
        if deadline.is_zero() || deadline > std::time::Duration::from_secs(3600) {
            return Err(Error::Limit("bounded child wait deadline required"));
        }
        tokio::time::timeout(deadline, async {
            loop {
                let (info, _) = self.status(subagent_id)?;
                if info.settled_at.is_some() {
                    return Ok(info);
                }
                tokio::select! {
                    _=cancel.cancelled()=>return Err(Error::Cancelled),
                    _=tokio::time::sleep(std::time::Duration::from_millis(35))=>{},
                }
            }
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    /// Read the direct child's status notice from an authenticated parent
    /// Actor. A nested child's notice goes to its own parent, never to root
    /// implicitly. A result is a data-only message, never transcript publish.
    pub fn poll_child_notification(
        &self,
        parent_subagent_id: &str,
        turn: u64,
    ) -> Result<Option<ActorMessage>> {
        let info = self
            .inner
            .store
            .subagent(&self.inner.owner, parent_subagent_id)?;
        let children = self
            .inner
            .children
            .lock()
            .map_err(|_| Error::Storage("child Actor registry lock poisoned"))?;
        let parent = children
            .get(&info.child_session_id)
            .ok_or(Error::Denied("no authenticated parent child Actor"))?;
        if parent.subagent_id != info.id {
            return Err(Error::Denied("child Actor parent Session changed"));
        }
        let mut runtime = self
            .inner
            .runtime
            .lock()
            .map_err(|_| Error::Storage("Actor Runtime lock poisoned"))?;
        runtime.poll_notification(&self.inner.owner, parent.key, parent.epoch, turn)
    }
    /// Release a factually settled child ONLY after its direct parent has
    /// consumed its final Notify. Unknown Actor effects can never be released
    /// by this service (they require external owner reconciliation).
    pub fn release_settled(&self, subagent_id: &str) -> Result<()> {
        let info = self.inner.store.subagent(&self.inner.owner, subagent_id)?;
        if !matches!(info.state.as_str(), "completed" | "failed" | "cancelled")
            || info.settled_at.is_none()
        {
            return Err(Error::Conflict(
                "child not factually settled and releasable",
            ));
        }
        let mut children = self
            .inner
            .children
            .lock()
            .map_err(|_| Error::Storage("child Actor registry lock poisoned"))?;
        let entry = children
            .get(&info.child_session_id)
            .ok_or(Error::Denied("child has no current Actor attachment"))?;
        if entry.subagent_id != info.id {
            return Err(Error::Denied("reused Session/Actor identity"));
        }
        let entry = entry.clone();
        let parent = if info.parent_session_id == self.inner.root_session {
            BoundChild {
                key: self.inner.root,
                epoch: self.inner.root_epoch,
                subagent_id: String::new(),
                next_message: Arc::new(AtomicU64::new(2)),
            }
        } else {
            children
                .get(&info.parent_session_id)
                .cloned()
                .ok_or(Error::Denied("parent Actor absent; cannot safely release"))?
        };
        let mut runtime = self
            .inner
            .runtime
            .lock()
            .map_err(|_| Error::Storage("Actor Runtime lock poisoned"))?;
        if runtime.has_pending_child_notice(
            &self.inner.owner,
            parent.key,
            parent.epoch,
            entry.key,
        )? {
            return Err(Error::Conflict(
                "final child status notification unconsumed",
            ));
        }
        runtime.release(&self.inner.owner, entry.key, entry.epoch)?;
        children.remove(&info.child_session_id);
        Ok(())
    }
    /// Parent notice is a bounded data-only ActorMessage, not root transcript.
    pub fn poll_root_notification(&self, turn: u64) -> Result<Option<ActorMessage>> {
        let mut runtime = self
            .inner
            .runtime
            .lock()
            .map_err(|_| Error::Storage("Actor Runtime lock poisoned"))?;
        runtime.poll_notification(
            &self.inner.owner,
            self.inner.root,
            self.inner.root_epoch,
            turn,
        )
    }
}
impl ChildLease {
    /// Only the original Session Turn is admitted. Further queued followups
    /// would be misleading for the existing one-Turn subagent runner, so this
    /// service does not advertise a general multi-Turn send API.
    pub fn admit_task(&self, task: &str, next_turn: u64) -> Result<()> {
        if task.is_empty() || task.len() > aw::config::MAX_PROMPT_BYTES || next_turn == 0 {
            return Err(Error::Denied("invalid bounded child task"));
        }
        let stored = self
            .host
            .inner
            .store
            .subagent(&self.host.inner.owner, &self.subagent_id)?;
        if stored.state != "running"
            || stored.child_session_id != self.child_session
            || format!("{:x}", Sha256::digest(task.as_bytes())) != stored.task_sha256
        {
            return Err(Error::Denied(
                "initial child Input differs from the durable task",
            ));
        }
        let mut runtime = self
            .host
            .inner
            .runtime
            .lock()
            .map_err(|_| Error::Storage("Actor Runtime lock poisoned"))?;
        runtime.actor(&self.host.inner.owner, self.parent, self.parent_epoch)?;
        let result = runtime.send(
            &self.host.inner.owner,
            self.child_epoch,
            ActorMessage {
                id: 1,
                source_sequence: 1,
                from: self.parent,
                to: self.key,
                kind: Kind::Input,
                turn: TurnTarget::Exact(next_turn),
                priority: 0,
                payload: task.to_owned(),
                content_type: aw_agent_kernel::message::ContentType::Text,
            },
            next_turn,
        )?;
        if !matches!(result, Delivery::Queued(_)) {
            return Err(Error::Denied(
                "child task was not admitted to its one Actor inbox",
            ));
        }
        Ok(())
    }
    /// The underlying SessionStore must *already* have committed this exact
    /// state. This observer can emit bounded parent notice but cannot publish
    /// a child answer as a root transcript or recover an unknown effect.
    pub fn observe_settled(&self, unknown_effect: bool) -> Result<()> {
        let stored = self
            .host
            .inner
            .store
            .subagent(&self.host.inner.owner, &self.subagent_id)?;
        if stored.child_session_id != self.child_session || stored.settled_at.is_none() {
            return Err(Error::Denied(
                "child Actor cannot settle before durable child Session",
            ));
        }
        let next = if unknown_effect || stored.state == "interrupted" {
            ActorState::Unknown
        } else {
            match stored.state.as_str() {
                "completed" => ActorState::Settled,
                "failed" => ActorState::Failed,
                "cancelled" => ActorState::Cancelled,
                _ => return Err(Error::Conflict("child Session not durably terminal")),
            }
        };
        let mut runtime = self
            .host
            .inner
            .runtime
            .lock()
            .map_err(|_| Error::Storage("Actor Runtime lock poisoned"))?;
        runtime.record_state(&self.host.inner.owner, self.key, self.child_epoch, next)?;
        let payload = json!({
            "subagent_id":stored.id,"state":stored.state,
            "needs_inspection":next==ActorState::Unknown
        })
        .to_string();
        // Notify never steers an ancestor, carries no answer/credential or
        // effect authority, and does not imply parent publication.
        let result = runtime.send(
            &self.host.inner.owner,
            self.parent_epoch,
            ActorMessage {
                from: self.key,
                to: self.parent,
                id: 1,
                source_sequence: 1,
                priority: 0,
                kind: Kind::Notify,
                turn: TurnTarget::Auto,
                payload,
                content_type: aw_agent_kernel::message::ContentType::Text,
            },
            stored.parent_turn_id,
        )?;
        if !matches!(result, Delivery::Queued(_)) {
            return Err(Error::Conflict("parent notification deferred or consumed"));
        }
        Ok(())
    }
    pub async fn run_once(
        &self,
        agent: &mut Agent,
        selected: &ProviderSelection,
        cancel: CancellationToken,
    ) -> Result<aw_agent_core::Turn> {
        self.controller
            .run_once_selected_with_cancel(agent, selected, cancel, |_| Ok(()))
            .await?
            .ok_or(Error::Conflict(
                "child task was not admitted at its exact safe Turn boundary",
            ))
    }
}
