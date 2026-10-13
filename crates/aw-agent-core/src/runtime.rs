//! Hosted Session-bound coordination service over Actor/Message/Envelope.
//! Does NOT run providers, tools, processes, or persist a second journal.
//! All operations are host-only until the authenticated Session adapter exists.
use crate::{
    extension::HookEvent,
    hooks::{Context as HookContext, Dispatch, HookId, HookPipeline, Registration},
    CancellationToken, Error, PrincipalId, Result,
};
use aw_agent_kernel::{
    actor::{ActorDescriptor, ActorDirectory, ActorKey, ActorKind, ActorState, RuntimeKey},
    capability::Effect,
    envelope::{Budget, Envelope, EnvelopeSpec, ExactGrant},
    hook_contract::{Continuation, HookFlow, Stage},
    message::{
        Accepted, ActorMessage, Boundary, Class, ControlKind, Kind, Mailbox, Scope, TurnTarget,
    },
};
use std::{
    collections::BTreeMap,
    num::NonZeroU64,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

/// Minted from an authenticated owning Session, not input from a model or RPC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionBinding {
    session_id: String,
    writer_epoch: Option<NonZeroU64>,
}
impl SessionBinding {
    pub fn from_trusted_host(
        session_id: impl Into<String>,
        writer_epoch: Option<NonZeroU64>,
    ) -> Result<Self> {
        let session_id = session_id.into();
        if session_id.is_empty()
            || session_id.len() > 128
            || session_id.chars().any(char::is_control)
        {
            return Err(Error::Config(
                "invalid authenticated Session binding".into(),
            ));
        }
        Ok(Self {
            session_id,
            writer_epoch,
        })
    }
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    pub fn writer_epoch(&self) -> Option<NonZeroU64> {
        self.writer_epoch
    }
}
/// Trusted host's scoped proof that an already journal-admitted child Session
/// has a verified execution binding at a DIFFERENT site. Not deserializable
/// from untrusted RPC, model prompts, extension manifest or persisted text.
/// Construct only after checking the authoritative SessionStore worktree.
#[derive(Clone, Debug)]
pub struct SiteTransferBinding {
    parent_session: String,
    child_session: String,
    from_site: String,
    to_site: String,
    execution_binding_id: String,
}
impl SiteTransferBinding {
    pub fn from_trusted_host(
        parent_session: impl Into<String>,
        child_session: impl Into<String>,
        from_site: impl Into<String>,
        to_site: impl Into<String>,
        execution_binding_id: impl Into<String>,
    ) -> Result<Self> {
        let result = Self {
            parent_session: parent_session.into(),
            child_session: child_session.into(),
            from_site: from_site.into(),
            to_site: to_site.into(),
            execution_binding_id: execution_binding_id.into(),
        };
        if result.parent_session == result.child_session
            || result.from_site == result.to_site
            || result.execution_binding_id.len() > 128
            || result.execution_binding_id.is_empty()
            || result.execution_binding_id.chars().any(char::is_control)
            || result.parent_session.is_empty()
            || result.child_session.is_empty()
        {
            return Err(Error::Denied(
                "invalid cross-site Session/working-root transfer",
            ));
        }
        aw_agent_kernel::capability::validate_actor_site("local", &result.from_site)
            .map_err(|_| Error::Denied("invalid parent execution site"))?;
        aw_agent_kernel::capability::validate_actor_site("local", &result.to_site)
            .map_err(|_| Error::Denied("invalid child execution site"))?;
        Ok(result)
    }
}

/// One typed cross-site bind transaction; its `transfer` must be minted
/// only after the host verifies the exact durable managed worktree.
pub struct SiteChildRequest<'a> {
    pub parent: ActorKey,
    pub parent_epoch: Option<NonZeroU64>,
    pub child: SessionBinding,
    pub spec: EnvelopeSpec,
    pub transfer: &'a SiteTransferBinding,
    pub label: &'a str,
}

struct AgentRecord {
    binding: SessionBinding,
    envelope: Envelope,
    inbox: Mailbox,
    active: Option<ActiveTurn>,
    pause_current: bool,
}
struct ActiveTurn {
    turn_id: u64,
    message: ActorMessage,
    token: CancellationToken,
    /// An aborted runner may have sent billable/model requests or tools.
    /// It is NEVER silently claimed again from the queue.
    uncertain: bool,
}
#[derive(Clone, Debug)]
pub struct ClaimedInput {
    pub message: ActorMessage,
    pub turn_id: u64,
    pub cancellation: CancellationToken,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryDecision {
    Proceed,
    Paused,
    Interrupt,
}
/// Typed pending-input revision. Target Turn, sender ID and sequence remain
/// immutable; only untrusted payload and scheduling priority may change.
pub struct PendingInputEdit {
    pub sender: ActorKey,
    pub id: u64,
    pub payload: String,
    pub priority: i8,
    pub current_turn: u64,
}

/// OS Process actors inherit the *same* authorized Session and envelope as
/// their owning Agent. OS pipes/PTY and reaping remain hosted I/O resources,
/// not additional transcript or tool execution engines.
struct ProcessRecord {
    binding: SessionBinding,
    envelope: Envelope,
}
/// Queue, consume, or suspend an admitted control. None is a factual
/// model/tool/process effect receipt.
pub enum Delivery {
    Queued(Accepted),
    /// A synchronous, authenticated Control was delivered without consuming
    /// queue capacity, and is not a model/tool execution receipt.
    Applied(Accepted),
    Consumed,
    Deferred(Continuation),
}
/// One asynchronously deferred Agent hook under the SAME current Actor,
/// Envelope revision and active Session Turn. This is in-memory coordination,
/// not a separate journal or permission engine.
struct PendingAgentHook {
    token: Continuation,
    stage: Stage,
    turn_id: u64,
    epoch: Option<NonZeroU64>,
    deadline: Instant,
    tx: oneshot::Sender<HookFlow<HookEvent>>,
}
struct DeferredMessage {
    message: ActorMessage,
    stage: Stage,
    next_index: usize,
    registry_version: u64,
    envelope_revision: u64,
    writer_epoch: Option<NonZeroU64>,
    current_turn: u64,
    deadline: Instant,
}
/// Per owner/site logical Runtime. The owning Agent and durable store remain
/// separate: an accepted message means queued, never executed or paid.
pub struct HostRuntime {
    owner: PrincipalId,
    site: String,
    actors: ActorDirectory,
    agents: BTreeMap<ActorKey, AgentRecord>,
    processes: BTreeMap<ActorKey, ProcessRecord>,
    inbox_limit: usize,
    hooks: HookPipeline,
    deferred: BTreeMap<u64, DeferredMessage>,
    pending_hooks: BTreeMap<u64, PendingAgentHook>,
    next_continuation: u64,
}
impl HostRuntime {
    pub fn new(
        owner: PrincipalId,
        site: impl Into<String>,
        key: RuntimeKey,
        max_actors: usize,
        inbox_limit: usize,
    ) -> Result<Self> {
        let site = site.into();
        aw_agent_kernel::capability::validate_actor_site(owner.as_str(), &site)
            .map_err(|_| Error::Config("invalid runtime owner/site".into()))?;
        let actors = ActorDirectory::new(key, max_actors)
            .map_err(|_| Error::Limit("invalid Actor capacity"))?;
        if inbox_limit == 0 || inbox_limit > aw_agent_kernel::message::MAX_INBOX {
            return Err(Error::Limit("invalid Actor inbox capacity"));
        }
        Ok(Self {
            owner,
            site,
            actors,
            agents: BTreeMap::new(),
            processes: BTreeMap::new(),
            inbox_limit,
            hooks: HookPipeline::default(),
            deferred: BTreeMap::new(),
            pending_hooks: BTreeMap::new(),
            next_continuation: 0,
        })
    }
    pub fn site(&self) -> &str {
        &self.site
    }
    pub fn owner(&self) -> &PrincipalId {
        &self.owner
    }
    pub fn actor_count(&self) -> usize {
        self.actors.count()
    }
    fn verify_owner(&self, principal: &PrincipalId) -> Result<()> {
        if principal != &self.owner {
            return Err(Error::Denied("runtime owner mismatch"));
        }
        Ok(())
    }
    fn bound(
        &self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
    ) -> Result<&AgentRecord> {
        self.verify_owner(principal)?;
        if self.actors.check(key).is_err() {
            return Err(Error::Denied("stale/nonexistent Actor"));
        }
        let r = self
            .agents
            .get(&key)
            .ok_or(Error::Denied("Actor Session not bound"))?;
        if r.binding.writer_epoch != epoch {
            return Err(Error::Conflict("Session writer epoch changed"));
        }
        Ok(r)
    }
    fn check_child_budget(&self, parent: ActorKey, budget: Budget) -> Result<()> {
        let used = self
            .actors
            .snapshot()
            .iter()
            .filter(|actor| actor.parent == Some(parent))
            .count();
        if used >= usize::from(budget.max_children) {
            return Err(Error::Limit("parent Actor child budget exhausted"));
        }
        Ok(())
    }
    /// Metadata-only: proves an existing Session binding, never creates an
    /// alternate Agent or grants any tool. OS Process actor I/O is separately
    /// hosted and cannot execute before an authenticated journaled intent.
    pub fn bind_agent(
        &mut self,
        principal: &PrincipalId,
        session: SessionBinding,
        parent: Option<ActorKey>,
        spec: EnvelopeSpec,
        label: &str,
    ) -> Result<ActorKey> {
        self.verify_owner(principal)?;
        if spec.actor != self.owner.as_str() || (parent.is_none() && spec.site != self.site) {
            return Err(Error::Denied(
                "root Envelope actor/site differs from owning Runtime",
            ));
        }
        // Child binding does not inherit the Runtime root site implicitly:
        // Envelope::narrow below requires the EXACT immediate parent's site,
        // including a previously and explicitly site-transferred parent.

        if self
            .agents
            .values()
            .any(|r| r.binding.session_id == session.session_id)
        {
            return Err(Error::Conflict("Session already bound"));
        }
        let env = match parent {
            None => Envelope::new(spec).map_err(|_| Error::Denied("invalid root Envelope"))?,
            Some(p) => {
                self.actors
                    .check(p)
                    .map_err(|_| Error::Denied("invalid parent Actor"))?;
                let parent = self
                    .agents
                    .get(&p)
                    .ok_or(Error::Denied("unbound parent Session"))?;
                self.check_child_budget(p, parent.envelope.budget())?;
                parent
                    .envelope
                    .narrow(spec)
                    .map_err(|_| Error::Denied("child cannot widen parent Envelope"))?
            }
        };
        let revision =
            NonZeroU64::new(env.revision()).ok_or(Error::Denied("invalid Envelope revision"))?;
        let key = self
            .actors
            .spawn(ActorKind::Agent, parent, label, revision)
            .map_err(|_| Error::Limit("Actor capacity, parent or lifecycle denied"))?;
        let inbox =
            Mailbox::new(key, self.inbox_limit).map_err(|_| Error::Limit("Actor inbox bound"))?;
        self.agents.insert(
            key,
            AgentRecord {
                binding: session,
                envelope: env,
                inbox,
                active: None,
                pause_current: false,
            },
        );
        Ok(key)
    }
    /// Bind a *previously admitted durable* child Session under the current
    /// owning parent Actor and writer epoch. This is a metadata/provenance
    /// service only; it neither starts an Agent nor manufactures Session/tool
    /// authority. Ordinary child execution still goes through the one Agent.
    pub fn bind_child_agent(
        &mut self,
        principal: &PrincipalId,
        parent: ActorKey,
        parent_epoch: Option<NonZeroU64>,
        child: SessionBinding,
        spec: EnvelopeSpec,
        label: &str,
    ) -> Result<ActorKey> {
        let parent_record = self.bound(principal, parent, parent_epoch)?;
        let parent_desc = self
            .actors
            .check(parent)
            .map_err(|_| Error::Denied("stale parent Actor"))?;
        if parent_desc.kind != ActorKind::Agent
            || !matches!(parent_desc.state, ActorState::Running | ActorState::Waiting)
            || parent_record.envelope.budget().max_depth <= parent_desc.depth
        {
            return Err(Error::Denied("parent Actor cannot start another child"));
        }
        if child.session_id() == parent_record.binding.session_id() {
            return Err(Error::Denied("child Session must not alias parent"));
        }
        self.bind_agent(principal, child, Some(parent), spec, label)
    }
    /// Explicit cross-site parent->child authorization; unlike Envelope::narrow
    /// this validates an approved *site transfer* and remints the same exact
    /// source/effect subset at the new site. It NEVER mutates an existing
    /// Actor's site, tool grants, payer, Session or paid/effect journal.
    /// The trusted caller must first verify the Store's bound managed worktree
    /// and mint the matching SiteTransferBinding. No generic RPC entry point.
    pub fn bind_site_child_agent(
        &mut self,
        principal: &PrincipalId,
        request: SiteChildRequest<'_>,
    ) -> Result<ActorKey> {
        let SiteChildRequest {
            parent,
            parent_epoch,
            child,
            spec,
            transfer,
            label,
        } = request;
        let parent_record = self.bound(principal, parent, parent_epoch)?;
        let parent_desc = self
            .actors
            .check(parent)
            .map_err(|_| Error::Denied("stale cross-site parent Actor"))?;
        if !matches!(parent_desc.kind, ActorKind::Agent)
            || !matches!(parent_desc.state, ActorState::Running | ActorState::Waiting)
            || parent_desc.depth >= parent_record.envelope.budget().max_depth
            || transfer.parent_session != parent_record.binding.session_id
            || transfer.child_session != child.session_id
            || transfer.from_site != parent_record.envelope.site()
            || transfer.to_site != spec.site
            || spec.site == parent_record.envelope.site()
            || spec.actor != self.owner.as_str()
            || spec.revision <= parent_record.envelope.revision()
            || self
                .agents
                .values()
                .any(|r| r.binding.session_id == child.session_id)
        {
            return Err(Error::Denied(
                "site transfer does not match authenticated parent/child Session",
            ));
        }
        let ceiling = parent_record.envelope.spec();
        if !spec.budget.fits_within(ceiling.budget)
            || (ceiling.access == aw_agent_kernel::envelope::Access::ReadOnly
                && spec.access == aw_agent_kernel::envelope::Access::ReadWrite)
            || spec.network > ceiling.network
            || (spec.process && !ceiling.process)
            || (spec.tty && !ceiling.tty)
            || (spec.microphone && !ceiling.microphone)
            || (spec.speaker && !ceiling.speaker)
            || spec
                .grants
                .iter()
                .any(|g| !parent_record.envelope.allows(g))
        {
            return Err(Error::Denied(
                "site transfer attempted to widen exact parent Envelope",
            ));
        }
        let envelope =
            Envelope::new(spec).map_err(|_| Error::Denied("invalid cross-site child Envelope"))?;
        self.check_child_budget(parent, ceiling.budget)?;
        let revision = NonZeroU64::new(envelope.revision())
            .ok_or(Error::Denied("invalid target Envelope revision"))?;
        let key = self
            .actors
            .spawn(ActorKind::Agent, Some(parent), label, revision)
            .map_err(|_| Error::Limit("site-transfer Actor capacity or lineage invalid"))?;
        let inbox = Mailbox::new(key, self.inbox_limit)
            .map_err(|_| Error::Limit("site-transfer mailbox capacity invalid"))?;
        self.agents.insert(
            key,
            AgentRecord {
                binding: child,
                envelope,
                inbox,
                active: None,
                pause_current: false,
            },
        );
        Ok(key)
    }
    fn process_bound(
        &self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
    ) -> Result<&ProcessRecord> {
        self.verify_owner(principal)?;
        let descriptor = self
            .actors
            .check(key)
            .map_err(|_| Error::Denied("stale/nonexistent Process Actor"))?;
        if descriptor.kind != ActorKind::Process {
            return Err(Error::Denied("expected Process Actor"));
        }
        let record = self
            .processes
            .get(&key)
            .ok_or(Error::Denied("unbound Process Actor"))?;
        if record.binding.writer_epoch != epoch {
            return Err(Error::Conflict(
                "Process Actor Session writer epoch changed",
            ));
        }
        Ok(record)
    }
    /// Reserve exactly one Process Actor beneath an existing Agent Session.
    /// No OS process is spawned here. The host must have obtained a durable
    /// intent receipt and an exact Process grant before calling its I/O port.
    /// The grant is source/epoch/effect-specific, not inferred from the name.
    pub fn bind_process(
        &mut self,
        principal: &PrincipalId,
        parent: ActorKey,
        epoch: Option<NonZeroU64>,
        spec: EnvelopeSpec,
        spawn_grant: &ExactGrant,
        label: &str,
    ) -> Result<ActorKey> {
        let parent_record = self.bound(principal, parent, epoch)?;
        if spec.actor != self.owner.as_str() || spec.site != self.site {
            return Err(Error::Denied(
                "Process Actor cannot impersonate another principal/site",
            ));
        }
        self.check_child_budget(parent, parent_record.envelope.budget())?;
        if !parent_record.envelope.spec().process
            || !spec.process
            || spawn_grant.effect != Effect::Process
            || !parent_record.envelope.allows(spawn_grant)
        {
            return Err(Error::Denied(
                "Process Actor needs explicit parent Process grant",
            ));
        }
        let envelope = parent_record
            .envelope
            .narrow(spec)
            .map_err(|_| Error::Denied("Process Actor cannot widen parent Envelope"))?;
        let binding = parent_record.binding.clone();
        let revision = NonZeroU64::new(envelope.revision())
            .ok_or(Error::Denied("invalid Process Envelope revision"))?;
        let key = self
            .actors
            .spawn(ActorKind::Process, Some(parent), label, revision)
            .map_err(|_| Error::Limit("Process Actor capacity/lineage/lifecycle denied"))?;
        self.processes
            .insert(key, ProcessRecord { binding, envelope });
        Ok(key)
    }
    /// Current host-authenticated Process envelope, rechecked before any
    /// subprocess write, attach, resize, disclosure or effectful control.
    pub fn process_envelope(
        &self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
    ) -> Result<&Envelope> {
        Ok(&self.process_bound(principal, key, epoch)?.envelope)
    }
    pub fn process_actor(
        &self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
    ) -> Result<ActorDescriptor> {
        self.process_bound(principal, key, epoch)?;
        self.actors
            .check(key)
            .cloned()
            .map_err(|_| Error::Denied("stale Process Actor"))
    }
    pub fn record_process_state(
        &mut self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
        next: ActorState,
    ) -> Result<()> {
        self.process_bound(principal, key, epoch)?;
        // The caller must first obtain factual OS/journal evidence. A process
        // that may still be effecting external state must end in Unknown.
        self.actors
            .transition(key, next)
            .map_err(|_| Error::Conflict("illegal Process Actor transition"))
    }
    pub fn release_process(
        &mut self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
    ) -> Result<()> {
        let descriptor = self.process_actor(principal, key, epoch)?;
        if descriptor.state == ActorState::Unknown || !descriptor.state.terminal() {
            return Err(Error::Conflict("unsettled Process Actor"));
        }
        self.actors
            .release(key)
            .map_err(|_| Error::Conflict("Process Actor has descendants or stale key"))?;
        self.processes.remove(&key);
        Ok(())
    }
    pub fn actor(
        &self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
    ) -> Result<ActorDescriptor> {
        self.bound(principal, key, epoch)?;
        self.actors
            .check(key)
            .cloned()
            .map_err(|_| Error::Denied("stale Actor"))
    }
    pub fn envelope(
        &self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
    ) -> Result<&Envelope> {
        Ok(&self.bound(principal, key, epoch)?.envelope)
    }
    pub fn session_binding(
        &self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
    ) -> Result<SessionBinding> {
        Ok(self.bound(principal, key, epoch)?.binding.clone())
    }
    /// Facts are recorded only after the existing owning Session/journal
    /// proves a transition. This method itself is not a durable receipt.
    pub fn record_state(
        &mut self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
        next: ActorState,
    ) -> Result<()> {
        self.bound(principal, key, epoch)?;
        self.actors
            .transition(key, next)
            .map_err(|_| Error::Conflict("illegal Actor lifecycle transition"))
    }
    pub fn add_hook(
        &mut self,
        principal: &PrincipalId,
        registration: Registration,
    ) -> Result<HookId> {
        self.verify_owner(principal)?;
        self.hooks.register(registration)
    }
    pub fn remove_hook(&mut self, principal: &PrincipalId, id: HookId) -> Result<bool> {
        self.verify_owner(principal)?;
        Ok(self.hooks.unregister(id))
    }
    pub fn hook_observer_failures(&self) -> u64 {
        self.hooks.observe_failures()
    }
    pub fn deferred_count(&self) -> usize {
        self.deferred.len()
    }
    /// Trusted host poll service: expire one-shot deferred admissions without
    /// silently treating them as delivered. The returned IDs must be reported
    /// as expired/denied by the owning host; no effects were performed.
    pub fn expire_deferred(&mut self, principal: &PrincipalId, now: Instant) -> Result<Vec<u64>> {
        self.verify_owner(principal)?;
        let expired: Vec<_> = self
            .deferred
            .iter()
            .filter_map(|(id, item)| (item.deadline <= now).then_some(*id))
            .collect();
        for id in &expired {
            self.deferred.remove(id);
        }
        Ok(expired)
    }

    fn validate_message(
        &self,
        principal: &PrincipalId,
        epoch: Option<NonZeroU64>,
        msg: &ActorMessage,
        current_turn: u64,
    ) -> Result<Stage> {
        self.verify_owner(principal)?;
        let from = self
            .actors
            .check(msg.from)
            .map_err(|_| Error::Denied("invalid sender"))?;
        let to = self
            .actors
            .check(msg.to)
            .map_err(|_| Error::Denied("invalid target"))?;
        if to.state.terminal() || (from.state.terminal() && msg.kind.class() != Class::Notify) {
            return Err(Error::Denied(
                "closed Actor cannot accept controls or new work",
            ));
        }
        // A terminal child may emit its one final factual notification before
        // identity release. Notification never steers or grants its parent.
        if msg.kind.class() == Class::Input && to.state == ActorState::CancelRequested {
            return Err(Error::Denied("cancel-requested Actor refuses new work"));
        }
        let stage = match msg.kind.class() {
            Class::Input | Class::Control => {
                if msg.from != msg.to && to.parent != Some(msg.from) {
                    return Err(Error::Denied("only owner/parent may control child"));
                }
                if let aw_agent_kernel::message::Kind::Control(op) = msg.kind {
                    use aw_agent_kernel::message::{ControlKind, Scope};
                    // Runtime-wide shutdown is not a child/actor operation.
                    // Only a separately authenticated root service may issue it.
                    if matches!(
                        op,
                        ControlKind::Cancel(Scope::Runtime)
                            | ControlKind::Freeze(Scope::Runtime)
                            | ControlKind::Resume(Scope::Runtime)
                    ) {
                        return Err(Error::Denied(
                            "runtime-wide controls need root host authorization",
                        ));
                    }
                    if !matches!(
                        op,
                        ControlKind::Cancel(Scope::Turn)
                            | ControlKind::Freeze(Scope::Turn | Scope::Actor)
                            | ControlKind::Resume(Scope::Turn | Scope::Actor)
                    ) {
                        return Err(Error::Denied(
                            "this scoped control requires a privileged owner service",
                        ));
                    }
                    // A cancelled Turn must already be active; an old or
                    // future control cannot arm cancellation for a later Turn.
                    if matches!(
                        op,
                        ControlKind::Cancel(Scope::Turn)
                            | ControlKind::Freeze(Scope::Turn)
                            | ControlKind::Resume(Scope::Turn)
                    ) {
                        let active = self
                            .bound(principal, msg.to, epoch)?
                            .active
                            .as_ref()
                            .ok_or(Error::Conflict("no active Turn for this control"))?;
                        if active.turn_id != current_turn
                            || matches!(msg.turn, TurnTarget::Exact(target) if target != active.turn_id)
                        {
                            return Err(Error::Conflict("control targeted a different Turn"));
                        }
                    }
                }
                if msg.kind.class() == Class::Input {
                    Stage::Input
                } else {
                    Stage::Signal
                }
            }
            Class::Notify => {
                if from.parent != Some(msg.to) {
                    return Err(Error::Denied("only child may notify parent"));
                }
                Stage::Event
            }
            Class::Hook => return Err(Error::Denied("external hook injection refused")),
        };
        self.bound(principal, msg.to, epoch)?;
        self.agents
            .get(&msg.to)
            .ok_or(Error::Denied("unbound target"))?
            .inbox
            .preflight_or_control(&self.actors, msg, current_turn)
            .map_err(|e| match e {
                aw_agent_kernel::message::Error::QueueFull => Error::Limit("Actor inbox full"),
                aw_agent_kernel::message::Error::TurnSettled => {
                    Error::Conflict("target Turn settled")
                }
                _ => Error::Denied("invalid Actor message, priority or sequence"),
            })?;
        Ok(stage)
    }
    #[cfg(test)]
    pub(crate) fn pending_agent_hook_count(&self) -> usize {
        self.pending_hooks.len()
    }
    pub(crate) fn reserve_agent_hook(
        &mut self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
        turn: u64,
        stage: Stage,
    ) -> Result<(Continuation, oneshot::Receiver<HookFlow<HookEvent>>)> {
        let record = self.bound(principal, key, epoch)?;
        if record.active.as_ref().is_none_or(|active| {
            active.turn_id != turn || active.uncertain || active.token.is_cancelled()
        }) {
            return Err(Error::Conflict("Agent hook has no current active Turn"));
        }
        if self.pending_hooks.len() >= 64 {
            return Err(Error::Limit("Runtime deferred hook capacity exhausted"));
        }
        let token = self.mint_continuation(principal, key, epoch)?;
        let (tx, rx) = oneshot::channel();
        self.pending_hooks.insert(
            token.id.get(),
            PendingAgentHook {
                token,
                stage,
                turn_id: turn,
                epoch,
                deadline: Instant::now() + Duration::from_secs(60),
                tx,
            },
        );
        Ok((token, rx))
    }
    pub(crate) fn abandon_agent_hook(&mut self, token: Continuation) {
        if self
            .pending_hooks
            .get(&token.id.get())
            .is_some_and(|entry| entry.token == token)
        {
            self.pending_hooks.remove(&token.id.get());
        }
    }
    pub(crate) fn verify_agent_hook(
        &self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
        token: Continuation,
    ) -> Result<()> {
        if token.owner != key || token.generation != key.generation {
            return Err(Error::Denied(
                "Runtime hook continuation Actor generation changed",
            ));
        }
        let record = self.bound(principal, key, epoch)?;
        if record.envelope.revision() != token.envelope_revision.get()
            || record
                .active
                .as_ref()
                .is_none_or(|active| active.uncertain || active.token.is_cancelled())
        {
            return Err(Error::Denied(
                "Runtime hook continuation Envelope/Turn invalidated",
            ));
        }
        Ok(())
    }
    pub(crate) fn resolve_agent_hook(
        &mut self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
        token: Continuation,
        flow: HookFlow<HookEvent>,
    ) -> Result<()> {
        self.verify_agent_hook(principal, key, epoch, token)?;
        if matches!(flow, HookFlow::Defer(_)) {
            return Err(Error::Denied("deferred hook may resolve only with Next"));
        }
        let entry = self
            .pending_hooks
            .get(&token.id.get())
            .ok_or(Error::Denied("missing or already resolved Agent hook"))?;
        if entry.token != token
            || entry.epoch != epoch
            || entry.deadline <= Instant::now()
            || self
                .bound(principal, key, epoch)?
                .active
                .as_ref()
                .is_none_or(|active| active.turn_id != entry.turn_id)
        {
            return Err(Error::Denied("stale or expired Agent hook continuation"));
        }
        let _stage = entry.stage;
        let entry = self.pending_hooks.remove(&token.id.get()).expect("checked");
        entry
            .tx
            .send(flow)
            .map_err(|_| Error::Denied("Agent hook receiver disconnected"))
    }
    fn mint_continuation(
        &mut self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
    ) -> Result<Continuation> {
        let revision = self.bound(principal, key, epoch)?.envelope.revision();
        let id = self
            .next_continuation
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .ok_or(Error::Limit("Runtime continuations exhausted"))?;
        self.next_continuation = id.get();
        Ok(Continuation {
            owner: key,
            id,
            generation: key.generation,
            envelope_revision: NonZeroU64::new(revision)
                .ok_or(Error::Denied("invalid Envelope revision"))?,
        })
    }
    fn apply_hooks(
        &self,
        stage: Stage,
        msg: ActorMessage,
        token: Continuation,
        from_index: usize,
    ) -> Result<Dispatch> {
        self.hooks.dispatch(
            HookContext {
                stage,
                actor: msg.to,
                envelope_revision: token.envelope_revision.get(),
                continuation: token,
            },
            msg,
            from_index,
        )
    }
    fn finish_hook(
        &mut self,
        principal: &PrincipalId,
        epoch: Option<NonZeroU64>,
        turn: u64,
        stage: Stage,
        token: Continuation,
        dispatch: Dispatch,
    ) -> Result<Delivery> {
        match dispatch {
            Dispatch::Next(None) => Ok(Delivery::Consumed),
            Dispatch::Next(Some(msg)) => {
                if self.validate_message(principal, epoch, &msg, turn)? != stage {
                    return Err(Error::Denied("hook changed message stage"));
                }
                if let Kind::Control(operation) = msg.kind {
                    // One synchronous Message control path. In particular,
                    // full Input queues cannot starve an authorized cancel.
                    let record = self.agents.get_mut(&msg.to).expect("verified bound Actor");
                    let admitted = record
                        .inbox
                        .accept_control(&self.actors, &msg, turn)
                        .map_err(|_| Error::Conflict("Actor control sequence changed"))?;
                    match operation {
                        ControlKind::Cancel(Scope::Turn) => record
                            .active
                            .as_ref()
                            .expect("checked active Turn")
                            .token
                            .cancel(),
                        ControlKind::Freeze(Scope::Turn) => record.pause_current = true,
                        ControlKind::Resume(Scope::Turn) => record.pause_current = false,
                        ControlKind::Freeze(Scope::Actor) => {
                            record.inbox.set_paused(true);
                            record.pause_current = true;
                        }
                        ControlKind::Resume(Scope::Actor) => {
                            record.inbox.set_paused(false);
                            record.pause_current = false;
                        }
                        _ => unreachable!("control validated at admission"),
                    }
                    return Ok(Delivery::Applied(admitted));
                }
                let accepted = self
                    .agents
                    .get_mut(&msg.to)
                    .expect("verified bound Actor")
                    .inbox
                    .send(&self.actors, msg, turn)
                    .map_err(|_| Error::Conflict("Actor inbox changed before admission"))?;
                Ok(Delivery::Queued(accepted))
            }
            Dispatch::Deferred {
                message,
                next_index,
            } => {
                if self.deferred.len() >= 64 {
                    return Err(Error::Limit("too many pending Runtime continuations"));
                }
                self.deferred.insert(
                    token.id.get(),
                    DeferredMessage {
                        message,
                        stage,
                        next_index,
                        registry_version: self.hooks.version(),
                        envelope_revision: token.envelope_revision.get(),
                        writer_epoch: epoch,
                        current_turn: turn,
                        deadline: Instant::now() + Duration::from_secs(30),
                    },
                );
                Ok(Delivery::Deferred(token))
            }
        }
    }
    /// Always follows the one typed hook pipeline. Gate transformations may
    /// only alter untrusted payloads, then every grant/target/queue rule is
    /// rechecked. No hook may manufacture an approval or bypass the journal.
    pub fn send(
        &mut self,
        principal: &PrincipalId,
        epoch: Option<NonZeroU64>,
        msg: ActorMessage,
        turn: u64,
    ) -> Result<Delivery> {
        let stage = self.validate_message(principal, epoch, &msg, turn)?;
        if self
            .deferred
            .values()
            .any(|d| d.message.from == msg.from && d.message.to == msg.to && d.message.id == msg.id)
        {
            return Err(Error::Denied("duplicate pending Runtime continuation"));
        }
        let token = self.mint_continuation(principal, msg.to, epoch)?;
        let decision = self.apply_hooks(stage, msg, token, 0)?;
        self.finish_hook(principal, epoch, turn, stage, token, decision)
    }
    /// One-shot host-authorized resume. No provider/tool execution is implied,
    /// even when the deferred handler was an approval/reviewer backend.
    pub fn resume(
        &mut self,
        principal: &PrincipalId,
        epoch: Option<NonZeroU64>,
        token: Continuation,
        updated: Option<ActorMessage>,
    ) -> Result<Delivery> {
        self.verify_owner(principal)?;
        let Some(entry) = self.deferred.get(&token.id.get()) else {
            return Err(Error::Denied("missing or already resumed continuation"));
        };
        if token.owner != entry.message.to
            || token.generation != token.owner.generation
            || token.envelope_revision.get() != entry.envelope_revision
            || entry.writer_epoch != epoch
        {
            return Err(Error::Denied("invalid Runtime continuation identity/epoch"));
        }
        self.bound(principal, token.owner, epoch)?;
        // An attempted resume consumes its one-shot reservation. A stale hook,
        // changed authority or timed-out request must fail closed, not replay.
        let entry = self.deferred.remove(&token.id.get()).expect("checked key");
        if entry.deadline <= Instant::now()
            || entry.registry_version != self.hooks.version()
            || self
                .bound(principal, token.owner, epoch)?
                .envelope
                .revision()
                != entry.envelope_revision
        {
            return Err(Error::Denied("stale/expired/revoked hook continuation"));
        }
        let Some(msg) = updated else {
            return Ok(Delivery::Consumed);
        };
        if !HookPipeline::same_address(&entry.message, &msg) {
            return Err(Error::Denied("resumed hook changed Actor authority"));
        }
        let stage = self.validate_message(principal, epoch, &msg, entry.current_turn)?;
        if stage != entry.stage {
            return Err(Error::Denied("resumed hook changed stage"));
        }
        let next = self.mint_continuation(principal, msg.to, epoch)?;
        let decision = self.apply_hooks(stage, msg, next, entry.next_index)?;
        self.finish_hook(principal, epoch, entry.current_turn, stage, next, decision)
    }
    /// Claim an Input only at a between-Turn safe point. It is removed from
    /// pending but retained as an *active* reservation until the real Agent
    /// has returned a settled Turn. A dropped/failed runner leaves Unknown,
    /// never an automatically retried or retargeted model/tool request.
    pub(crate) fn claim_next_input(
        &mut self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
        turn: u64,
    ) -> Result<Option<ClaimedInput>> {
        let record = self.bound(principal, key, epoch)?;
        if record.active.is_some() {
            return Err(Error::Conflict(
                "previous claimed Turn is live or uncertain",
            ));
        }
        if self
            .actors
            .check(key)
            .is_ok_and(|a| a.state == ActorState::CancelRequested || a.state.terminal())
        {
            return Err(Error::Denied("closed Actor cannot start a Turn"));
        }
        if record.inbox.paused() {
            return Ok(None);
        }
        if record.inbox.has_stale_sources(&self.actors) {
            return Err(Error::Conflict(
                "stale sender Actor must be explicitly rejected",
            ));
        }
        if record
            .inbox
            .snapshot()
            .iter()
            .any(|msg| matches!(msg.turn,TurnTarget::Exact(target) if target < turn))
        {
            return Err(Error::Conflict(
                "expired exact-Turn Input needs explicit host rejection before dispatch",
            ));
        }
        let record = self.agents.get_mut(&key).expect("verified Actor");
        let Some(message) = record
            .inbox
            .take_class(turn, Boundary::TurnEnd, Some(Class::Input))
        else {
            return Ok(None);
        };
        let cancellation = CancellationToken::new();
        record.active = Some(ActiveTurn {
            turn_id: turn,
            message: message.clone(),
            token: cancellation.clone(),
            uncertain: false,
        });
        Ok(Some(ClaimedInput {
            message,
            turn_id: turn,
            cancellation,
        }))
    }
    /// Read-only step gate, callable without holding a Runtime lock during
    /// awaits. A negative-priority Input means interrupt at the NEXT safe
    /// boundary and then dispatch that Input as its own journaled Turn.
    pub(crate) fn boundary_decision(
        &self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
        turn: u64,
        round: u32,
        before_tools: bool,
    ) -> Result<BoundaryDecision> {
        let record = self.bound(principal, key, epoch)?;
        let active = record
            .active
            .as_ref()
            .ok_or(Error::Conflict("no claimed Turn"))?;
        if active.turn_id != turn || active.uncertain {
            return Err(Error::Conflict(
                "Turn controller lost its current reservation",
            ));
        }
        if active.token.is_cancelled() {
            return Ok(BoundaryDecision::Interrupt);
        }
        if record.pause_current || record.inbox.paused() {
            return Ok(BoundaryDecision::Paused);
        }
        // The first newly admitted Input may be followed by another urgent
        // Input: do NOT cancel before the first provider request just because
        // a second Input is pending. After the first provider response, an
        // urgent steer may prevent its proposed tool batch or next model call.
        if (round > 1 || before_tools) && record.inbox.has_urgent_input(turn) {
            return Ok(BoundaryDecision::Interrupt);
        }
        Ok(BoundaryDecision::Proceed)
    }
    /// Same-path completion: only the already-settled Agent Turn may close
    /// this claim. The controller never synthesizes an Assistant/tool receipt.
    pub(crate) fn finish_claim(
        &mut self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
        turn: u64,
        settled: bool,
    ) -> Result<()> {
        let record = self.bound(principal, key, epoch)?;
        if record.active.as_ref().is_none_or(|a| a.turn_id != turn) {
            return Err(Error::Conflict("stale controller completion"));
        }
        let record = self.agents.get_mut(&key).expect("checked Actor");
        if settled {
            record.active = None;
            record.pause_current = false;
        } else {
            record.active.as_mut().expect("checked").uncertain = true;
        }
        Ok(())
    }
    /// A control-plane query, not transcript access: the current Session
    /// owner can verify whether an exact Turn is actively reserved.
    pub fn has_active_turn(
        &self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
        turn: u64,
    ) -> Result<bool> {
        Ok(self
            .bound(principal, key, epoch)?
            .active
            .as_ref()
            .is_some_and(|active| active.turn_id == turn && !active.uncertain))
    }
    pub(crate) fn active_input(
        &self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
    ) -> Result<Option<ActorMessage>> {
        Ok(self
            .bound(principal, key, epoch)?
            .active
            .as_ref()
            .map(|a| a.message.clone()))
    }
    /// Explicit host inspection only; caller must also reconcile the Agent's
    /// own durable hazards before freeing an uncertain claim.
    pub(crate) fn discard_uncertain(
        &mut self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
    ) -> Result<()> {
        let record = self.bound(principal, key, epoch)?;
        if record.active.as_ref().is_none_or(|a| !a.uncertain) {
            return Err(Error::Conflict("no uncertain claimed Turn to inspect"));
        }
        self.agents.get_mut(&key).expect("checked").active = None;
        Ok(())
    }
    /// Existing accepted data can only be edited while still pending and by
    /// its owning controller; it goes through the SAME typed Input hooks.
    pub fn update_pending_input(
        &mut self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
        edit: PendingInputEdit,
    ) -> Result<()> {
        let PendingInputEdit {
            sender,
            id,
            payload,
            priority,
            current_turn,
        } = edit;
        let record = self.bound(principal, key, epoch)?;
        let original = record
            .inbox
            .snapshot()
            .iter()
            .find(|m| m.from == sender && m.id == id && m.kind == Kind::Input)
            .ok_or(Error::Conflict("Input already dispatched or not found"))?
            .clone();
        let parent = self
            .actors
            .check(key)
            .map_err(|_| Error::Denied("stale Actor"))?
            .parent;
        if sender != key && parent != Some(sender) {
            return Err(Error::Denied(
                "caller does not control pending Input source",
            ));
        }
        if matches!(original.turn,TurnTarget::Exact(turn) if turn<current_turn) {
            return Err(Error::Conflict("Input target Turn already settled"));
        }
        let mut replacement = original.clone();
        replacement.payload = payload;
        replacement.priority = priority;
        let token = self.mint_continuation(principal, key, epoch)?;
        let result = self.apply_hooks(Stage::Input, replacement.clone(), token, 0)?;
        let next = match result {
            Dispatch::Next(Some(m)) if HookPipeline::same_address(&replacement, &m) => m,
            _ => {
                return Err(Error::Denied(
                    "pending Input update was consumed, deferred or retargeted",
                ))
            }
        };
        self.agents
            .get_mut(&key)
            .expect("checked")
            .inbox
            .update_input(sender, id, next.payload, priority)
            .map_err(|_| Error::Denied("invalid pending Input modification"))
    }
    pub fn delete_pending_input(
        &mut self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
        sender: ActorKey,
        id: u64,
    ) -> Result<ActorMessage> {
        self.bound(principal, key, epoch)?;
        let parent = self
            .actors
            .check(key)
            .map_err(|_| Error::Denied("stale Actor"))?
            .parent;
        if sender != key && parent != Some(sender) {
            return Err(Error::Denied(
                "caller does not control pending Input source",
            ));
        }
        self.agents
            .get_mut(&key)
            .expect("checked")
            .inbox
            .delete_input(sender, id)
            .map_err(|_| Error::Conflict("Input not pending"))
    }

    /// Read-only child/process notification. A notification has no tool/model
    /// effect and cannot steer a parent; it uses the same bounded Actor inbox
    /// without providing a direct Input dequeue or admission escape hatch.
    pub fn poll_notification(
        &mut self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
        turn: u64,
    ) -> Result<Option<ActorMessage>> {
        self.bound(principal, key, epoch)?;
        if self
            .agents
            .get(&key)
            .expect("verified Agent")
            .inbox
            .has_stale_sources(&self.actors)
        {
            return Err(Error::Conflict(
                "stale notification sender must be explicitly rejected",
            ));
        }
        Ok(self
            .agents
            .get_mut(&key)
            .expect("verified Agent")
            .inbox
            .take_class(turn, Boundary::TurnEnd, Some(Class::Notify)))
    }
    /// An exact, generation-fenced child notice cannot be lost by freeing a
    /// settled Actor while its parent still has the final status pending.
    pub fn has_pending_child_notice(
        &self,
        principal: &PrincipalId,
        parent: ActorKey,
        parent_epoch: Option<NonZeroU64>,
        child: ActorKey,
    ) -> Result<bool> {
        let inbox = &self.bound(principal, parent, parent_epoch)?.inbox;
        let actor = self
            .actors
            .check(child)
            .map_err(|_| Error::Denied("stale child notice source"))?;
        if actor.parent != Some(parent) {
            return Err(Error::Denied("notice source is not a direct child"));
        }
        Ok(inbox
            .snapshot()
            .iter()
            .any(|msg| msg.from == child && msg.kind == Kind::Notify))
    }
    /// Release only messages from retired sender generations, returning
    /// them to the owner for explicit rejection reporting. A reused Actor
    /// slot can never inherit the former sender's queued text.
    pub fn reject_stale_senders(
        &mut self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
    ) -> Result<Vec<ActorMessage>> {
        self.bound(principal, key, epoch)?;
        Ok(self
            .agents
            .get_mut(&key)
            .expect("verified Agent")
            .inbox
            .reject_stale_sources(&self.actors))
    }
    /// Host-owned Turn advance. Explicitly targeted missed inputs are
    /// returned for an expired/denied notice, never silently retargeted or
    /// treated as executed. This does not mutate transcript/journal history.
    pub fn reject_settled_inputs(
        &mut self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
        current_turn: u64,
    ) -> Result<Vec<ActorMessage>> {
        self.bound(principal, key, epoch)?;
        if current_turn == 0 {
            return Err(Error::Denied("invalid current Turn"));
        }
        Ok(self
            .agents
            .get_mut(&key)
            .expect("checked Actor")
            .inbox
            .reject_settled(current_turn))
    }
    pub fn pending(
        &self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
    ) -> Result<usize> {
        Ok(self.bound(principal, key, epoch)?.inbox.len())
    }
    /// No unfinished/pending or uncertain Actor is silently discarded.
    pub fn release(
        &mut self,
        principal: &PrincipalId,
        key: ActorKey,
        epoch: Option<NonZeroU64>,
    ) -> Result<()> {
        let a = self.actor(principal, key, epoch)?;
        if a.state == ActorState::Unknown
            || !a.state.terminal()
            || self.pending(principal, key, epoch)? != 0
            || self.deferred.values().any(|d| d.message.to == key)
            || self.bound(principal, key, epoch)?.active.is_some()
        {
            return Err(Error::Conflict("live/unknown Actor or accepted input"));
        }
        self.actors
            .release(key)
            .map_err(|_| Error::Conflict("Actor still has live descendants"))?;
        self.agents.remove(&key);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_kernel::{
        actor::{ActorState, RuntimeKey},
        envelope::{Access, Budget, Network},
        message::{ControlKind, Kind, Scope, TurnTarget},
    };
    fn nz(n: u64) -> NonZeroU64 {
        NonZeroU64::new(n).unwrap()
    }
    fn user() -> PrincipalId {
        PrincipalId::new("local").unwrap()
    }
    fn other() -> PrincipalId {
        PrincipalId::new("other").unwrap()
    }
    fn runtime() -> HostRuntime {
        HostRuntime::new(
            user(),
            "local",
            RuntimeKey {
                id: nz(1),
                generation: nz(1),
            },
            3,
            2,
        )
        .unwrap()
    }
    fn session(id: &str) -> SessionBinding {
        SessionBinding::from_trusted_host(id, Some(nz(1))).unwrap()
    }
    fn env() -> EnvelopeSpec {
        EnvelopeSpec {
            actor: "local".into(),
            site: "local".into(),
            revision: 1,
            access: Access::ReadWrite,
            network: Network::Deny,
            process: false,
            tty: false,
            microphone: false,
            speaker: false,
            budget: Budget {
                max_model_rounds: 8,
                max_tool_calls: 12,
                max_children: 2,
                max_depth: 2,
                max_output_bytes: 2048,
            },
            grants: vec![],
        }
    }
    fn msg(from: ActorKey, to: ActorKey, id: u64, kind: Kind) -> ActorMessage {
        ActorMessage {
            id,
            from,
            to,
            kind,
            turn: TurnTarget::Auto,
            priority: 0,
            source_sequence: id,
            payload: "go".into(),
            content_type: aw_agent_kernel::message::ContentType::Text,
        }
    }
    #[test]
    fn child_agent_never_widens_parent_site_generation_or_lifecycle() {
        let mut r = runtime();
        let root = r
            .bind_agent(&user(), session("root"), None, env(), "root")
            .unwrap();
        let mut spec = env();
        spec.revision = 2;
        spec.access = Access::ReadOnly;
        // A new root is not yet a live parent Session.
        assert!(r
            .bind_child_agent(
                &user(),
                root,
                Some(nz(1)),
                session("child"),
                spec.clone(),
                "child"
            )
            .is_err());
        r.record_state(&user(), root, Some(nz(1)), ActorState::Running)
            .unwrap();
        let mut too_wide = spec.clone();
        too_wide.budget.max_tool_calls += 1;
        assert!(r
            .bind_child_agent(
                &user(),
                root,
                Some(nz(1)),
                session("child"),
                too_wide,
                "child"
            )
            .is_err());
        let mut wrong_site = spec.clone();
        wrong_site.site = "untrusted-site".into();
        assert!(r
            .bind_child_agent(
                &user(),
                root,
                Some(nz(1)),
                session("child"),
                wrong_site,
                "child"
            )
            .is_err());
        assert!(r
            .bind_child_agent(
                &other(),
                root,
                Some(nz(1)),
                session("child"),
                spec.clone(),
                "child"
            )
            .is_err());
        assert!(r
            .bind_child_agent(
                &user(),
                root,
                Some(nz(2)),
                session("child"),
                spec.clone(),
                "child"
            )
            .is_err());
        assert!(r
            .bind_child_agent(
                &user(),
                root,
                Some(nz(1)),
                session("root"),
                spec.clone(),
                "alias"
            )
            .is_err());
        let child = r
            .bind_child_agent(
                &user(),
                root,
                Some(nz(1)),
                session("child"),
                spec.clone(),
                "child",
            )
            .unwrap();
        assert_eq!(r.actor(&user(), child, Some(nz(1))).unwrap().depth, 1);
        assert_eq!(
            r.envelope(&user(), child, Some(nz(1)))
                .unwrap()
                .spec()
                .access,
            Access::ReadOnly
        );
        assert!(r
            .bind_child_agent(
                &user(),
                root,
                Some(nz(1)),
                session("child"),
                spec.clone(),
                "duplicate"
            )
            .is_err());
        r.record_state(&user(), root, Some(nz(1)), ActorState::Frozen)
            .unwrap();
        assert!(r
            .bind_child_agent(
                &user(),
                root,
                Some(nz(1)),
                session("another"),
                spec,
                "frozen"
            )
            .is_err());
    }
    #[test]
    fn explicit_site_transfer_keeps_one_parent_actor_and_current_tool_ceiling() {
        use aw_agent_kernel::capability::SourceKind;
        let mut r = runtime();
        let mut root_spec = env();
        let read = ExactGrant {
            name: "read".into(),
            kind: SourceKind::Host,
            source: "trusted-host".into(),
            source_epoch: 1,
            effect: Effect::Read,
        };
        root_spec.grants.push(read.clone());
        let root = r
            .bind_agent(&user(), session("root"), None, root_spec.clone(), "root")
            .unwrap();
        r.record_state(&user(), root, Some(nz(1)), ActorState::Running)
            .unwrap();
        let mut target = root_spec;
        target.site = "subagent:child-one".into();
        target.revision = 2;
        target.access = Access::ReadOnly;
        target.grants = vec![read.clone()];
        target.budget.max_children = 1;
        assert!(r
            .bind_child_agent(
                &user(),
                root,
                Some(nz(1)),
                session("child"),
                target.clone(),
                "child"
            )
            .is_err());
        let transfer = SiteTransferBinding::from_trusted_host(
            "root",
            "child",
            "local",
            "subagent:child-one",
            "sxb_exact",
        )
        .unwrap();
        let key = r
            .bind_site_child_agent(
                &user(),
                SiteChildRequest {
                    parent: root,
                    parent_epoch: Some(nz(1)),
                    child: session("child"),
                    spec: target.clone(),
                    transfer: &transfer,
                    label: "child",
                },
            )
            .unwrap();
        assert_eq!(
            r.actor(&user(), key, Some(nz(1))).unwrap().parent,
            Some(root)
        );
        assert_eq!(r.actor(&user(), key, Some(nz(1))).unwrap().depth, 1);
        assert_eq!(
            r.envelope(&user(), key, Some(nz(1))).unwrap().site(),
            "subagent:child-one"
        );
        assert_eq!(
            r.envelope(&user(), key, Some(nz(1))).unwrap().spec().access,
            Access::ReadOnly
        );
        assert!(r.envelope(&user(), key, Some(nz(1))).unwrap().allows(&read));
        assert_eq!(r.actor_count(), 2);
        assert!(r
            .bind_site_child_agent(
                &user(),
                SiteChildRequest {
                    parent: root,
                    parent_epoch: Some(nz(1)),
                    child: session("child"),
                    spec: target.clone(),
                    transfer: &transfer,
                    label: "dup"
                }
            )
            .is_err());
        r.record_state(&user(), key, Some(nz(1)), ActorState::Running)
            .unwrap();
        r.send(&user(), Some(nz(1)), msg(root, key, 1, Kind::Input), 1)
            .unwrap();
        let claimed = r
            .claim_next_input(&user(), key, Some(nz(1)), 1)
            .unwrap()
            .unwrap();
        assert_eq!(claimed.message.from, root);
        r.finish_claim(&user(), key, Some(nz(1)), 1, true).unwrap();
        r.record_state(&user(), key, Some(nz(1)), ActorState::Settled)
            .unwrap();
        r.send(&user(), Some(nz(1)), msg(key, root, 1, Kind::Notify), 1)
            .unwrap();
        assert_eq!(
            r.poll_notification(&user(), root, Some(nz(1)), 1)
                .unwrap()
                .unwrap()
                .from,
            key
        );
    }
    #[test]
    fn same_site_grandchild_of_site_transferred_parent_inherits_exact_parent_site() {
        let mut r = runtime();
        let root = r
            .bind_agent(&user(), session("root"), None, env(), "root")
            .unwrap();
        r.record_state(&user(), root, Some(nz(1)), ActorState::Running)
            .unwrap();
        let mut remote = env();
        remote.site = "subagent:one".into();
        remote.revision = 2;
        let auth = SiteTransferBinding::from_trusted_host(
            "root",
            "child",
            "local",
            "subagent:one",
            "sxb_one",
        )
        .unwrap();
        let child = r
            .bind_site_child_agent(
                &user(),
                SiteChildRequest {
                    parent: root,
                    parent_epoch: Some(nz(1)),
                    child: session("child"),
                    spec: remote.clone(),
                    transfer: &auth,
                    label: "child",
                },
            )
            .unwrap();
        r.record_state(&user(), child, Some(nz(1)), ActorState::Running)
            .unwrap();
        let mut grandchild = remote.clone();
        grandchild.revision = 3;
        grandchild.budget.max_tool_calls -= 1;
        let key = r
            .bind_child_agent(
                &user(),
                child,
                Some(nz(1)),
                session("grandchild"),
                grandchild.clone(),
                "grandchild",
            )
            .unwrap();
        assert_eq!(
            r.actor(&user(), key, Some(nz(1))).unwrap().parent,
            Some(child)
        );
        assert_eq!(r.actor(&user(), key, Some(nz(1))).unwrap().depth, 2);
        assert_eq!(
            r.envelope(&user(), key, Some(nz(1))).unwrap().site(),
            "subagent:one"
        );
        let mut site_swap = grandchild;
        site_swap.site = "subagent:other".into();
        site_swap.revision += 1;
        assert!(r
            .bind_child_agent(
                &user(),
                child,
                Some(nz(1)),
                session("moved"),
                site_swap,
                "moved"
            )
            .is_err());
    }
    #[test]
    fn site_transfer_rejects_owner_epoch_source_and_new_capability() {
        use aw_agent_kernel::capability::SourceKind;
        let mut r = runtime();
        let root = r
            .bind_agent(&user(), session("root"), None, env(), "root")
            .unwrap();
        r.record_state(&user(), root, Some(nz(1)), ActorState::Running)
            .unwrap();
        let mut spec = env();
        spec.site = "subagent:child".into();
        spec.revision = 2;
        let transfer = SiteTransferBinding::from_trusted_host(
            "root",
            "child",
            "local",
            "subagent:child",
            "sxb_1",
        )
        .unwrap();
        macro_rules! bad {
            ($epoch:expr,$spec:expr,$transfer:expr) => {
                SiteChildRequest {
                    parent: root,
                    parent_epoch: $epoch,
                    child: session("child"),
                    spec: $spec,
                    transfer: $transfer,
                    label: "child",
                }
            };
        }
        assert!(r
            .bind_site_child_agent(&other(), bad!(Some(nz(1)), spec.clone(), &transfer))
            .is_err());
        assert!(r
            .bind_site_child_agent(&user(), bad!(Some(nz(2)), spec.clone(), &transfer))
            .is_err());
        let wrong = SiteTransferBinding::from_trusted_host(
            "another",
            "child",
            "local",
            "subagent:child",
            "sxb_1",
        )
        .unwrap();
        assert!(r
            .bind_site_child_agent(&user(), bad!(Some(nz(1)), spec.clone(), &wrong))
            .is_err());
        let another = SiteTransferBinding::from_trusted_host(
            "root",
            "child",
            "local",
            "subagent:other",
            "sxb_1",
        )
        .unwrap();
        assert!(r
            .bind_site_child_agent(&user(), bad!(Some(nz(1)), spec.clone(), &another))
            .is_err());
        let mut extra = spec.clone();
        extra.grants = vec![ExactGrant {
            name: "write".into(),
            kind: SourceKind::Host,
            source: "trusted-host".into(),
            source_epoch: 1,
            effect: Effect::Write,
        }];
        assert!(r
            .bind_site_child_agent(&user(), bad!(Some(nz(1)), extra, &transfer))
            .is_err());
        let mut larger = spec.clone();
        larger.budget.max_tool_calls += 1;
        assert!(r
            .bind_site_child_agent(&user(), bad!(Some(nz(1)), larger, &transfer))
            .is_err());
        assert_eq!(r.actor_count(), 1);
    }
    #[test]
    fn owned_distinct_sessions_and_child_envelope() {
        let mut r = runtime();
        let root = r
            .bind_agent(&user(), session("root"), None, env(), "root")
            .unwrap();
        assert!(r
            .bind_agent(&user(), session("root"), None, env(), "duplicate")
            .is_err());
        assert!(r
            .bind_agent(&other(), session("other"), None, env(), "wrong")
            .is_err());
        let mut narrow = env();
        narrow.access = Access::ReadOnly;
        narrow.revision = 2;
        let child = r
            .bind_agent(&user(), session("child"), Some(root), narrow, "child")
            .unwrap();
        assert_eq!(
            r.actor(&user(), child, Some(nz(1))).unwrap().parent,
            Some(root)
        );
        assert!(r.actor(&user(), child, Some(nz(2))).is_err());
        assert!(r.actor(&other(), child, Some(nz(1))).is_err());
        assert_eq!(r.actor_count(), 2);
    }
    #[test]
    fn child_notifications_not_parent_steering_and_effect_barrier() {
        let mut r = runtime();
        let root = r
            .bind_agent(&user(), session("root"), None, env(), "root")
            .unwrap();
        let child = r
            .bind_agent(&user(), session("child"), Some(root), env(), "child")
            .unwrap();
        r.record_state(&user(), root, Some(nz(1)), ActorState::Running)
            .unwrap();
        r.record_state(&user(), child, Some(nz(1)), ActorState::Running)
            .unwrap();
        assert!(r
            .send(&user(), Some(nz(1)), msg(child, root, 1, Kind::Input), 1)
            .is_err());
        r.send(&user(), Some(nz(1)), msg(root, child, 1, Kind::Input), 1)
            .unwrap();
        assert_eq!(r.pending(&user(), child, Some(nz(1))).unwrap(), 1);
        // The only production data admission is now a tracked claim.
        let admitted = r
            .claim_next_input(&user(), child, Some(nz(1)), 1)
            .unwrap()
            .unwrap();
        assert_eq!(admitted.message.id, 1);
        assert_eq!(r.pending(&user(), child, Some(nz(1))).unwrap(), 0);
        assert!(r.claim_next_input(&user(), child, Some(nz(1)), 1).is_err());
        r.send(&user(), Some(nz(1)), msg(child, root, 1, Kind::Notify), 1)
            .unwrap();
        assert!(r
            .send(
                &user(),
                Some(nz(1)),
                msg(root, child, 2, Kind::Control(ControlKind::Approve)),
                1
            )
            .is_err());
        assert!(r
            .send(
                &user(),
                Some(nz(1)),
                msg(
                    root,
                    child,
                    3,
                    Kind::Control(ControlKind::Cancel(Scope::Subtree))
                ),
                1
            )
            .is_err());
    }
    #[test]
    fn cannot_release_live_unknown_or_accepted_input() {
        let mut r = runtime();
        let root = r
            .bind_agent(&user(), session("root"), None, env(), "root")
            .unwrap();
        assert!(r.release(&user(), root, Some(nz(1))).is_err());
        r.record_state(&user(), root, Some(nz(1)), ActorState::Unknown)
            .unwrap();
        assert!(r.release(&user(), root, Some(nz(1))).is_err());
    }
    struct DeferredHook;
    impl crate::hooks::RuntimeHook for DeferredHook {
        fn handle(
            &self,
            context: crate::hooks::Context,
            _: &ActorMessage,
        ) -> aw_agent_kernel::hook_contract::HookOutcome<ActorMessage> {
            Ok(aw_agent_kernel::hook_contract::HookFlow::Defer(
                context.continuation,
            ))
        }
    }
    struct RewriteHook;
    impl crate::hooks::RuntimeHook for RewriteHook {
        fn handle(
            &self,
            _: crate::hooks::Context,
            msg: &ActorMessage,
        ) -> aw_agent_kernel::hook_contract::HookOutcome<ActorMessage> {
            let mut m = msg.clone();
            m.priority = -1; // Must fail current owner/source/priority invariants.
            Ok(aw_agent_kernel::hook_contract::HookFlow::Next(Some(m)))
        }
    }
    struct SuppressCancel;
    impl crate::hooks::RuntimeHook for SuppressCancel {
        fn handle(
            &self,
            _: crate::hooks::Context,
            _: &ActorMessage,
        ) -> aw_agent_kernel::hook_contract::HookOutcome<ActorMessage> {
            Ok(aw_agent_kernel::hook_contract::HookFlow::Next(None))
        }
    }
    #[test]
    fn hook_defer_is_one_shot_and_denies_wrong_epoch_or_duplicate() {
        use crate::hooks::Registration;
        use aw_agent_kernel::hook_contract::{Capability, Stage};
        use std::sync::Arc;
        let mut r = runtime();
        let root = r
            .bind_agent(&user(), session("root"), None, env(), "root")
            .unwrap();
        r.record_state(&user(), root, Some(nz(1)), ActorState::Running)
            .unwrap();
        r.add_hook(
            &user(),
            Registration {
                owner: "p".into(),
                stage: Stage::Input,
                capability: Capability::Gate,
                priority: 0,
                handler: Arc::new(DeferredHook),
            },
        )
        .unwrap();
        let message = msg(root, root, 1, Kind::Input);
        let Delivery::Deferred(token) = r.send(&user(), Some(nz(1)), message.clone(), 1).unwrap()
        else {
            panic!("should defer rather than claim enqueue");
        };
        assert_eq!(r.pending(&user(), root, Some(nz(1))).unwrap(), 0);
        assert_eq!(r.deferred_count(), 1);
        assert!(r.send(&user(), Some(nz(1)), message.clone(), 1).is_err());
        assert!(r
            .resume(&other(), Some(nz(1)), token, Some(message.clone()))
            .is_err());
        assert!(r
            .resume(&user(), Some(nz(2)), token, Some(message.clone()))
            .is_err());
        assert_eq!(r.deferred_count(), 1);
        let Delivery::Queued(receipt) = r
            .resume(&user(), Some(nz(1)), token, Some(message))
            .unwrap()
        else {
            panic!("resumed hook must actually enqueue once");
        };
        assert_eq!(receipt.id, 1);
        assert_eq!(r.deferred_count(), 0);
        assert!(r.resume(&user(), Some(nz(1)), token, None).is_err());
        assert_eq!(r.pending(&user(), root, Some(nz(1))).unwrap(), 1);
    }
    #[test]
    fn registry_change_or_authority_rewrite_invalidates_defer() {
        use crate::hooks::Registration;
        use aw_agent_kernel::hook_contract::{Capability, Stage};
        use std::sync::Arc;
        let mut r = runtime();
        let root = r
            .bind_agent(&user(), session("root"), None, env(), "root")
            .unwrap();
        let handler = r
            .add_hook(
                &user(),
                Registration {
                    owner: "p".into(),
                    stage: Stage::Input,
                    capability: Capability::Transform,
                    priority: 0,
                    handler: Arc::new(DeferredHook),
                },
            )
            .unwrap();
        let original = msg(root, root, 1, Kind::Input);
        let Delivery::Deferred(token) = r.send(&user(), Some(nz(1)), original.clone(), 1).unwrap()
        else {
            panic!("deferred")
        };
        r.remove_hook(&user(), handler).unwrap();
        assert!(r
            .resume(&user(), Some(nz(1)), token, Some(original))
            .is_err());
        assert_eq!(r.deferred_count(), 0);
        r.add_hook(
            &user(),
            Registration {
                owner: "p".into(),
                stage: Stage::Input,
                capability: Capability::Transform,
                priority: 0,
                handler: Arc::new(RewriteHook),
            },
        )
        .unwrap();
        assert!(r
            .send(&user(), Some(nz(1)), msg(root, root, 2, Kind::Input), 1)
            .is_err());
        assert_eq!(r.pending(&user(), root, Some(nz(1))).unwrap(), 0);
    }
    #[test]
    fn optional_hooks_cannot_swallow_authenticated_cancel() {
        use crate::hooks::Registration;
        use aw_agent_kernel::hook_contract::{Capability, Stage};
        use std::sync::Arc;
        let mut r = runtime();
        let root = r
            .bind_agent(&user(), session("root"), None, env(), "root")
            .unwrap();
        r.add_hook(
            &user(),
            Registration {
                owner: "p".into(),
                stage: Stage::Signal,
                capability: Capability::Gate,
                priority: 0,
                handler: Arc::new(SuppressCancel),
            },
        )
        .unwrap();
        // A Turn cancel is meaningful only after a real Input is claimed.
        r.send(&user(), Some(nz(1)), msg(root, root, 1, Kind::Input), 1)
            .unwrap();
        let claim = r
            .claim_next_input(&user(), root, Some(nz(1)), 1)
            .unwrap()
            .unwrap();
        let cancel = msg(
            root,
            root,
            2,
            Kind::Control(ControlKind::Cancel(Scope::Turn)),
        );
        assert!(matches!(
            r.send(&user(), Some(nz(1)), cancel, 1),
            Ok(Delivery::Applied(_))
        ));
        assert!(claim.cancellation.is_cancelled());
        assert_eq!(r.hook_observer_failures(), 1);
    }
    #[test]
    fn expired_defer_is_reported_not_replayed_or_implicitly_approved() {
        use crate::hooks::Registration;
        use aw_agent_kernel::hook_contract::{Capability, Stage};
        use std::sync::Arc;
        let mut r = runtime();
        let root = r
            .bind_agent(&user(), session("root"), None, env(), "root")
            .unwrap();
        r.add_hook(
            &user(),
            Registration {
                owner: "p".into(),
                stage: Stage::Input,
                capability: Capability::Gate,
                priority: 0,
                handler: Arc::new(DeferredHook),
            },
        )
        .unwrap();
        let original = msg(root, root, 1, Kind::Input);
        let Delivery::Deferred(token) = r.send(&user(), Some(nz(1)), original.clone(), 1).unwrap()
        else {
            panic!("should defer")
        };
        assert!(r
            .expire_deferred(&other(), Instant::now() + Duration::from_secs(31))
            .is_err());
        assert_eq!(
            r.expire_deferred(&user(), Instant::now() + Duration::from_secs(31))
                .unwrap(),
            vec![token.id.get()]
        );
        assert_eq!(r.deferred_count(), 0);
        assert_eq!(r.pending(&user(), root, Some(nz(1))).unwrap(), 0);
        assert!(r
            .resume(&user(), Some(nz(1)), token, Some(original))
            .is_err());
    }
    #[test]
    fn explicitly_targeted_missed_turn_is_returned_never_retargeted() {
        let mut r = runtime();
        let root = r
            .bind_agent(&user(), session("root"), None, env(), "root")
            .unwrap();
        let mut input = msg(root, root, 1, Kind::Input);
        input.turn = TurnTarget::Exact(2);
        assert!(matches!(
            r.send(&user(), Some(nz(1)), input, 1),
            Ok(Delivery::Queued(_))
        ));
        assert_eq!(r.pending(&user(), root, Some(nz(1))).unwrap(), 1);
        let stale = r
            .reject_settled_inputs(&user(), root, Some(nz(1)), 3)
            .unwrap();
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].turn, TurnTarget::Exact(2));
        assert_eq!(r.pending(&user(), root, Some(nz(1))).unwrap(), 0);
        assert!(r
            .claim_next_input(&user(), root, Some(nz(1)), 3)
            .unwrap()
            .is_none());
    }
    struct PanicHook;
    impl crate::hooks::RuntimeHook for PanicHook {
        fn handle(
            &self,
            _: crate::hooks::Context,
            _: &ActorMessage,
        ) -> aw_agent_kernel::hook_contract::HookOutcome<ActorMessage> {
            panic!("intentional test-only hook panic")
        }
    }
    #[test]
    fn policy_hook_panic_denies_without_enqueue_and_observer_panic_fails_open() {
        use crate::hooks::Registration;
        use aw_agent_kernel::hook_contract::{Capability, Stage};
        use std::sync::Arc;
        let mut r = runtime();
        let root = r
            .bind_agent(&user(), session("root"), None, env(), "root")
            .unwrap();
        let handler = r
            .add_hook(
                &user(),
                Registration {
                    owner: "p".into(),
                    stage: Stage::Input,
                    capability: Capability::Gate,
                    priority: 0,
                    handler: Arc::new(PanicHook),
                },
            )
            .unwrap();
        assert!(r
            .send(&user(), Some(nz(1)), msg(root, root, 1, Kind::Input), 1)
            .is_err());
        assert_eq!(r.pending(&user(), root, Some(nz(1))).unwrap(), 0);
        assert!(r.remove_hook(&user(), handler).unwrap());
        r.add_hook(
            &user(),
            Registration {
                owner: "p".into(),
                stage: Stage::Input,
                capability: Capability::Observe,
                priority: 0,
                handler: Arc::new(PanicHook),
            },
        )
        .unwrap();
        assert!(matches!(
            r.send(&user(), Some(nz(1)), msg(root, root, 1, Kind::Input), 1),
            Ok(Delivery::Queued(_))
        ));
        assert_eq!(r.hook_observer_failures(), 1);
    }
    fn process_grant() -> ExactGrant {
        ExactGrant {
            name: "bash".into(),
            kind: aw_agent_kernel::capability::SourceKind::Host,
            source: "trusted-host".into(),
            source_epoch: 1,
            effect: aw_agent_kernel::capability::Effect::Process,
        }
    }
    fn process_envelope() -> EnvelopeSpec {
        let mut spec = env();
        spec.process = true;
        spec.grants = vec![process_grant()];
        spec
    }
    #[test]
    fn process_actors_reuse_agent_session_exact_grant_and_generation() {
        let mut runtime = runtime();
        let root = runtime
            .bind_agent(&user(), session("root"), None, process_envelope(), "root")
            .unwrap();
        let mut spec = process_envelope();
        spec.revision = 2;
        let pid = runtime
            .bind_process(
                &user(),
                root,
                Some(nz(1)),
                spec.clone(),
                &process_grant(),
                "pipe",
            )
            .unwrap();
        assert_eq!(
            runtime
                .process_actor(&user(), pid, Some(nz(1)))
                .unwrap()
                .kind,
            ActorKind::Process
        );
        assert_eq!(
            runtime
                .process_actor(&user(), pid, Some(nz(1)))
                .unwrap()
                .parent,
            Some(root)
        );
        assert!(runtime.process_actor(&other(), pid, Some(nz(1))).is_err());
        assert!(runtime.process_actor(&user(), pid, Some(nz(2))).is_err());
        let mut forbidden = process_grant();
        forbidden.effect = aw_agent_kernel::capability::Effect::Read;
        assert!(runtime
            .bind_process(&user(), root, Some(nz(1)), spec.clone(), &forbidden, "bad")
            .is_err());
        let mut forged = spec.clone();
        forged.actor = "other".into();
        assert!(runtime
            .bind_process(&user(), root, Some(nz(1)), forged, &process_grant(), "bad")
            .is_err());
        assert_eq!(runtime.actor_count(), 2);
        runtime
            .record_process_state(&user(), pid, Some(nz(1)), ActorState::Running)
            .unwrap();
        assert!(runtime.release_process(&user(), pid, Some(nz(1))).is_err());
        runtime
            .record_process_state(&user(), pid, Some(nz(1)), ActorState::Settled)
            .unwrap();
        runtime.release_process(&user(), pid, Some(nz(1))).unwrap();
        assert!(runtime.process_actor(&user(), pid, Some(nz(1))).is_err());
        let reused = runtime
            .bind_process(&user(), root, Some(nz(1)), spec, &process_grant(), "new")
            .unwrap();
        assert_eq!(reused.id, pid.id);
        assert_ne!(reused.generation, pid.generation);
    }
    #[test]
    fn process_actor_denies_ungranted_process_effect_and_children_over_budget() {
        let mut runtime = runtime();
        let mut root_env = env();
        root_env.process = true;
        let root = runtime
            .bind_agent(&user(), session("root"), None, root_env, "root")
            .unwrap();
        let mut spec = process_envelope();
        spec.revision = 2;
        assert!(runtime
            .bind_process(
                &user(),
                root,
                Some(nz(1)),
                spec.clone(),
                &process_grant(),
                "denied"
            )
            .is_err());
        assert_eq!(runtime.actor_count(), 1);
        // Another root with an exact grant permits at most its declared child budget.
        let root2 = runtime
            .bind_agent(
                &user(),
                session("second"),
                None,
                process_envelope(),
                "root2",
            )
            .unwrap();
        let first = runtime
            .bind_process(
                &user(),
                root2,
                Some(nz(1)),
                spec.clone(),
                &process_grant(),
                "one",
            )
            .unwrap();
        assert_eq!(
            runtime
                .process_actor(&user(), first, Some(nz(1)))
                .unwrap()
                .kind,
            ActorKind::Process
        );
        // Runtime max=3 actors; global capacity is also an independent ceiling.
        assert!(runtime
            .bind_process(
                &user(),
                root2,
                Some(nz(1)),
                spec.clone(),
                &process_grant(),
                "over"
            )
            .is_err());
        runtime
            .record_process_state(&user(), first, Some(nz(1)), ActorState::Unknown)
            .unwrap();
        assert!(runtime
            .release_process(&user(), first, Some(nz(1)))
            .is_err());
    }
}
