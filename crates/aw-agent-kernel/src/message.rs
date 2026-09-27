//! Addressed actor communication, independent of chat transcript Messages.
//! A control/inbox acknowledgement never claims the requested external effect
//! succeeded. The authenticated host validates issuer/owner/site before send.
use super::actor::{ActorDirectory, ActorKey, MAX_ACTORS};
use alloc::{string::String, vec::Vec};

pub const MAX_INBOX: usize = 64;
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;
pub const PRIORITY_MIN: i8 = -20;
pub const PRIORITY_MAX: i8 = 19;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Control,
    Input,
    Notify,
    Hook,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Turn,
    Actor,
    Subtree,
    Runtime,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
    Cancel(Scope),
    Freeze(Scope),
    Resume(Scope),
    SetModel,
    SetThinking,
    Settle,
    ProcessWrite,
    ProcessResize,
    Approve,
    Deny,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Control(ControlKind),
    Input,
    Notify,
    Hook,
}
impl Kind {
    pub const fn class(self) -> Class {
        match self {
            Self::Control(_) => Class::Control,
            Self::Input => Class::Input,
            Self::Notify => Class::Notify,
            Self::Hook => Class::Hook,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnTarget {
    Auto,
    Exact(u64),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    ExternalEffect,
    ModelInFlight,
    StepEnd,
    TurnEnd,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidActor,
    Duplicate,
    InvalidId,
    InvalidPriority,
    InvalidPayload,
    QueueFull,
    TurnSettled,
    NotSteerable,
    InvalidKind,
    InvalidSequence,
}
/// AW-P6-8C: an inert, host-inspected content type on the ORIGINAL Message
/// primitive. This is NOT a decoder, transcript format, Tool permission,
/// media device grant, or invitation for the text-only Agent to process media.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContentType {
    #[default]
    Text,
    Image,
    Audio,
    Video,
    Binary,
}
impl ContentType {
    pub const fn text_only(self) -> bool {
        matches!(self, Self::Text)
    }
    pub const fn needs_media_consent(self) -> bool {
        !self.text_only()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorMessage {
    pub id: u64,
    pub from: ActorKey,
    pub to: ActorKey,
    pub kind: Kind,
    pub turn: TurnTarget,
    pub priority: i8,
    pub source_sequence: u64,
    pub payload: String,
    /// Inert type tag: current text-only Runtime/Agent must fail-closed
    /// before any Hook, provider or Tool for a non-text Message. Future host
    /// media services need separate explicit consent and delivery receipts.
    pub content_type: ContentType,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accepted {
    pub id: u64,
    pub position: usize,
}
/// One inbox per owning Agent Actor; order is deterministic, never global.
/// Messages are admitted once and removed only when a safe boundary exists.
pub struct Mailbox {
    owner: ActorKey,
    pending: Vec<ActorMessage>,
    capacity: usize,
    last_sequence: Vec<(ActorKey, u64)>,
    paused: bool,
    urgent_streak: u8,
}
impl Mailbox {
    pub fn new(owner: ActorKey, capacity: usize) -> Result<Self, Error> {
        if capacity == 0 || capacity > MAX_INBOX {
            return Err(Error::QueueFull);
        }
        Ok(Self {
            owner,
            pending: Vec::new(),
            capacity,
            last_sequence: Vec::new(),
            paused: false,
            urgent_streak: 0,
        })
    }
    pub fn len(&self) -> usize {
        self.pending.len()
    }
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
    pub fn snapshot(&self) -> &[ActorMessage] {
        &self.pending
    }
    /// Queue pausing is a host-controlled admission decision. Controls and
    /// child notifications can still be serviced while ordinary Input waits.
    pub fn paused(&self) -> bool {
        self.paused
    }
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused
    }

    /// Existing admitted Input can be modified only while still pending.
    /// The exact sender/sequence identity and target Turn stay immutable.
    pub fn update_input(
        &mut self,
        sender: ActorKey,
        id: u64,
        payload: String,
        priority: i8,
    ) -> Result<(), Error> {
        if !(PRIORITY_MIN..=PRIORITY_MAX).contains(&priority) {
            return Err(Error::InvalidPriority);
        }
        if payload.len() > MAX_PAYLOAD_BYTES || payload.contains('\0') {
            return Err(Error::InvalidPayload);
        }
        let item = self
            .pending
            .iter_mut()
            .find(|m| m.from == sender && m.id == id && m.kind == Kind::Input)
            .ok_or(Error::InvalidId)?;
        item.payload = payload;
        item.priority = priority;
        Ok(())
    }
    /// Delete only a still-pending Input, never an accepted cancel,
    /// terminal notification or already-dispatched effect.
    pub fn delete_input(&mut self, sender: ActorKey, id: u64) -> Result<ActorMessage, Error> {
        let index = self
            .pending
            .iter()
            .position(|m| m.from == sender && m.id == id && m.kind == Kind::Input)
            .ok_or(Error::InvalidId)?;
        Ok(self.pending.remove(index))
    }

    /// Pure validation before invoking any hook. No capacity/sequence is
    /// consumed until send actually commits the message to this inbox.
    pub fn preflight(
        &self,
        actors: &ActorDirectory,
        message: &ActorMessage,
        current_turn: u64,
    ) -> Result<(), Error> {
        self.validate(actors, message, current_turn, false)
    }
    /// Controls are delivered synchronously by the authenticated hosted
    /// controller, through this same Message vocabulary and sender sequence
    /// ledger. They don't need an Input-queue slot when the queue is full.
    /// One shared validation entrypoint for addressed Messages. Control
    /// consumes the same sender sequence ledger but no Input queue capacity.
    pub fn preflight_or_control(
        &self,
        actors: &ActorDirectory,
        message: &ActorMessage,
        current_turn: u64,
    ) -> Result<(), Error> {
        if message.kind.class() == Class::Control {
            self.preflight_control(actors, message, current_turn)
        } else {
            self.preflight(actors, message, current_turn)
        }
    }
    pub fn preflight_control(
        &self,
        actors: &ActorDirectory,
        message: &ActorMessage,
        current_turn: u64,
    ) -> Result<(), Error> {
        if message.kind.class() != Class::Control {
            return Err(Error::InvalidKind);
        }
        self.validate(actors, message, current_turn, true)
    }
    fn validate(
        &self,
        actors: &ActorDirectory,
        message: &ActorMessage,
        current_turn: u64,
        skip_capacity: bool,
    ) -> Result<(), Error> {
        if message.to != self.owner
            || actors.check(message.from).is_err()
            || actors.check(message.to).is_err()
        {
            return Err(Error::InvalidActor);
        }
        // The pure mailbox key is (sender, monotonic sequence). In this core
        // contract message id is that sequence; a separate externally supplied
        // RPC correlation id is authenticated and deduplicated by the host.
        // This prevents reissuing an old accepted id using a newer sequence.
        if message.id == 0 || message.id != message.source_sequence {
            return Err(Error::InvalidId);
        }
        if message.priority < PRIORITY_MIN || message.priority > PRIORITY_MAX {
            return Err(Error::InvalidPriority);
        }
        if !message.content_type.text_only() {
            return Err(Error::InvalidKind);
        }
        if message.payload.len() > MAX_PAYLOAD_BYTES || message.payload.chars().any(|c| c == '\0') {
            return Err(Error::InvalidPayload);
        }
        if current_turn == 0 || matches!(message.turn, TurnTarget::Exact(0)) {
            return Err(Error::InvalidId);
        }
        if let TurnTarget::Exact(turn) = message.turn {
            if turn < current_turn {
                return Err(Error::TurnSettled);
            }
        }
        if let Some((_, last)) = self
            .last_sequence
            .iter()
            .find(|(source, _)| *source == message.from)
        {
            if message.source_sequence <= *last {
                return Err(Error::InvalidSequence);
            }
        }
        if self
            .pending
            .iter()
            .any(|item| item.from == message.from && item.id == message.id)
        {
            return Err(Error::Duplicate);
        }
        if !skip_capacity && self.pending.len() == self.capacity {
            return Err(Error::QueueFull);
        }
        if message.kind.class() != Class::Input && message.priority < 0 {
            // Only input can request urgent steering; controller commands are
            // routed through a separately authorized host service.
            return Err(Error::InvalidKind);
        }
        Ok(())
    }
    pub fn send(
        &mut self,
        actors: &ActorDirectory,
        message: ActorMessage,
        current_turn: u64,
    ) -> Result<Accepted, Error> {
        self.preflight(actors, &message, current_turn)?;
        self.pending.try_reserve(1).map_err(|_| Error::QueueFull)?;
        self.reserve_sequence(message.from, actors)?;
        self.record_sequence(message.from, message.source_sequence);
        self.pending.push(message);
        Ok(Accepted {
            id: self.pending.last().expect("pushed").id,
            position: self.pending.len(),
        })
    }
    fn reserve_sequence(&mut self, sender: ActorKey, actors: &ActorDirectory) -> Result<(), Error> {
        if self.last_sequence.iter().all(|(s, _)| *s != sender) {
            // In a long-lived Runtime many distinct generations may reuse an
            // actor slot. Retain only live sender generations when the fixed
            // MAX_ACTORS ceiling is reached. Stale keys cannot send again.
            if self.last_sequence.len() >= MAX_ACTORS {
                self.last_sequence
                    .retain(|(source, _)| actors.check(*source).is_ok());
            }
            if self.last_sequence.len() >= MAX_ACTORS {
                return Err(Error::QueueFull);
            }
            self.last_sequence
                .try_reserve(1)
                .map_err(|_| Error::QueueFull)?;
        }
        Ok(())
    }
    fn record_sequence(&mut self, sender: ActorKey, number: u64) {
        if let Some((_, last)) = self.last_sequence.iter_mut().find(|(s, _)| *s == sender) {
            *last = number;
        } else {
            self.last_sequence.push((sender, number));
        }
    }
    /// Commit an independently authorized synchronous Control without
    /// enqueuing it. The sequence is consumed exactly once and shared with
    /// Input admission; it cannot subsequently be replayed as another class.
    pub fn accept_control(
        &mut self,
        actors: &ActorDirectory,
        control: &ActorMessage,
        current_turn: u64,
    ) -> Result<Accepted, Error> {
        self.preflight_control(actors, control, current_turn)?;
        self.reserve_sequence(control.from, actors)?;
        self.record_sequence(control.from, control.source_sequence);
        Ok(Accepted {
            id: control.id,
            position: 0,
        })
    }

    /// A signed urgent Input remains queued until its own next admitted
    /// Turn. The hosted Agent may use this only to *interrupt* at a safe
    /// step/tool boundary. It never injects unjournaled text into a paid call.
    pub fn has_urgent_input(&self, current_turn: u64) -> bool {
        !self.paused
            && self.pending.iter().any(|msg| {
                msg.kind == Kind::Input
                    && msg.priority < 0
                    && match msg.turn {
                        TurnTarget::Auto => true,
                        TurnTarget::Exact(turn) => turn == current_turn,
                    }
            })
    }

    /// Choose a currently deliverable message, respecting exact target Turn.
    /// In-flight model may be steered only by urgent Input; an external effect
    /// is always a hard barrier until factual outcome is recorded.
    pub fn take(&mut self, current_turn: u64, boundary: Boundary) -> Option<ActorMessage> {
        self.take_class(current_turn, boundary, None)
    }
    /// Separate control drain uses this SAME inbox, not a parallel signal
    /// queue. It never consumes ordinary Input or grants an execution effect.
    pub fn take_class(
        &mut self,
        current_turn: u64,
        boundary: Boundary,
        class: Option<Class>,
    ) -> Option<ActorMessage> {
        let mut selected: Option<usize> = None;
        for (index, msg) in self.pending.iter().enumerate() {
            if class.is_some_and(|required| msg.kind.class() != required)
                || (self.paused && msg.kind == Kind::Input)
            {
                continue;
            }
            let target_eligible = match msg.turn {
                TurnTarget::Auto => true,
                TurnTarget::Exact(t) => t == current_turn,
            };
            if !target_eligible {
                continue;
            }
            let safe = match boundary {
                Boundary::ExternalEffect => false,
                Boundary::ModelInFlight => {
                    (msg.kind == Kind::Input && msg.priority < 0)
                        || matches!(msg.kind, Kind::Control(ControlKind::Cancel(_)))
                }
                Boundary::StepEnd | Boundary::TurnEnd => true,
            };
            if !safe {
                continue;
            }
            if let Some(prev) = selected {
                let old = &self.pending[prev];
                // Smaller priority is more urgent; source order is stable on ties.
                // Admission order breaks ties, even across different senders;
                // caller-provided IDs never reorder a bounded FIFO.
                if msg.priority < old.priority {
                    selected = Some(index);
                }
            } else {
                selected = Some(index);
            }
        }
        // Bounded fairness: at most four successive urgent Inputs may win
        // while a normal Input for this same boundary is ready. No auto
        // retargeting, no unfair reordering by caller-provided IDs.
        if boundary == Boundary::TurnEnd && self.urgent_streak >= 4 {
            if let Some(index) = self.pending.iter().position(|m| {
                m.kind == Kind::Input
                    && m.priority >= 0
                    && !self.paused
                    && match m.turn {
                        TurnTarget::Auto => true,
                        TurnTarget::Exact(target) => target == current_turn,
                    }
                    && class.is_none_or(|required| required == Class::Input)
            }) {
                selected = Some(index);
            }
        }
        selected.map(|i| {
            let msg = self.pending.remove(i);
            if msg.kind == Kind::Input {
                self.urgent_streak = if msg.priority < 0 {
                    self.urgent_streak.saturating_add(1)
                } else {
                    0
                };
            }
            msg
        })
    }
    /// Return stale-sender messages for explicit owner reporting. Releasing
    /// an Actor never grants its already-queued text or Notify to a successor
    /// with a reused slot/generation. No silent execution or retargeting.
    pub fn has_stale_sources(&self, actors: &ActorDirectory) -> bool {
        self.pending.iter().any(|m| actors.check(m.from).is_err())
    }
    pub fn reject_stale_sources(&mut self, actors: &ActorDirectory) -> Vec<ActorMessage> {
        let mut rejected = Vec::new();
        let mut index = 0;
        while index < self.pending.len() {
            if actors.check(self.pending[index].from).is_err() {
                rejected.push(self.pending.remove(index));
            } else {
                index += 1;
            }
        }
        rejected
    }
    /// A missed explicit Turn must not silently roll over to another Turn.
    pub fn reject_settled(&mut self, current_turn: u64) -> Vec<ActorMessage> {
        let mut rejected = Vec::new();
        let mut i = 0;
        while i < self.pending.len() {
            if matches!(self.pending[i].turn, TurnTarget::Exact(t) if t < current_turn) {
                rejected.push(self.pending.remove(i));
            } else {
                i += 1;
            }
        }
        rejected
    }
}
#[cfg(test)]
mod tests {
    use super::super::actor::{ActorKind, ActorState, RuntimeKey};
    use super::*;
    use core::num::NonZeroU64;
    fn nz(n: u64) -> NonZeroU64 {
        NonZeroU64::new(n).unwrap()
    }
    fn setup() -> (ActorDirectory, ActorKey, ActorKey) {
        let mut d = ActorDirectory::new(
            RuntimeKey {
                id: nz(1),
                generation: nz(1),
            },
            2,
        )
        .unwrap();
        let user = d.spawn(ActorKind::Agent, None, "user", nz(1)).unwrap();
        let child = d
            .spawn(ActorKind::Agent, Some(user), "child", nz(1))
            .unwrap();
        d.transition(user, ActorState::Running).unwrap();
        d.transition(child, ActorState::Running).unwrap();
        (d, user, child)
    }
    fn msg(id: u64, from: ActorKey, to: ActorKey, priority: i8) -> ActorMessage {
        ActorMessage {
            id,
            from,
            to,
            kind: Kind::Input,
            turn: TurnTarget::Auto,
            priority,
            source_sequence: id,
            payload: "go".into(),
            content_type: ContentType::Text,
        }
    }
    #[test]
    fn deterministic_order_and_external_barrier() {
        let (d, user, child) = setup();
        let mut q = Mailbox::new(child, 3).unwrap();
        q.send(&d, msg(1, user, child, 0), 1).unwrap();
        q.send(&d, msg(2, user, child, -2), 1).unwrap();
        assert!(q.take(1, Boundary::ExternalEffect).is_none());
        assert_eq!(q.take(1, Boundary::ModelInFlight).unwrap().id, 2);
        assert!(q.take(1, Boundary::ModelInFlight).is_none());
        assert_eq!(q.take(1, Boundary::StepEnd).unwrap().id, 1);
    }
    #[test]
    fn stale_target_duplicate_and_sequence_cannot_replay() {
        let (d, user, child) = setup();
        let mut q = Mailbox::new(child, 1).unwrap();
        let mut stale = msg(1, user, child, 0);
        stale.turn = TurnTarget::Exact(3);
        assert_eq!(q.send(&d, stale.clone(), 4), Err(Error::TurnSettled));
        q.send(&d, msg(1, user, child, 0), 4).unwrap();
        assert_eq!(
            q.send(&d, msg(1, user, child, 0), 4),
            Err(Error::InvalidSequence)
        );
        assert_eq!(q.send(&d, msg(2, user, child, 0), 4), Err(Error::QueueFull));
        assert!(q.take(4, Boundary::StepEnd).is_some());
        assert_eq!(
            q.send(&d, msg(1, user, child, 0), 4),
            Err(Error::InvalidSequence)
        );
        assert!(q.is_empty());
    }
    #[test]
    fn wrong_actor_and_bad_priority_denied() {
        let (d, user, child) = setup();
        let mut q = Mailbox::new(child, 1).unwrap();
        assert_eq!(
            q.send(&d, msg(1, user, user, 0), 1),
            Err(Error::InvalidActor)
        );
        assert_eq!(
            q.send(&d, msg(2, user, child, -21), 1),
            Err(Error::InvalidPriority)
        );
        assert_eq!(
            q.send(&d, msg(3, user, child, 20), 1),
            Err(Error::InvalidPriority)
        );
    }
    #[test]
    fn fifo_tie_is_stable_across_senders_and_pausing_preserves_controls() {
        let (d, root, child) = setup();
        let mut q = Mailbox::new(child, 4).unwrap();
        q.send(&d, msg(9, child, child, 0), 1).unwrap();
        q.send(&d, msg(1, root, child, 0), 1).unwrap();
        q.set_paused(true);
        assert_eq!(q.take(1, Boundary::StepEnd), None);
        let mut cancel = msg(10, child, child, 0);
        cancel.kind = Kind::Control(ControlKind::Cancel(Scope::Turn));
        q.send(&d, cancel, 1).unwrap();
        assert!(
            matches!(q.take_class(1,Boundary::StepEnd,Some(Class::Control)),
                        Some(m) if m.id==10)
        );
        q.set_paused(false);
        assert_eq!(q.take(1, Boundary::StepEnd).unwrap().id, 9);
        assert_eq!(q.take(1, Boundary::StepEnd).unwrap().id, 1);
    }
    #[test]
    fn pending_update_and_delete_cannot_replay_an_old_sequence() {
        let (d, user, child) = setup();
        let mut q = Mailbox::new(child, 2).unwrap();
        q.send(&d, msg(1, user, child, 0), 1).unwrap();
        q.update_input(user, 1, "edited".into(), -3).unwrap();
        assert_eq!(q.snapshot()[0].payload, "edited");
        assert_eq!(q.snapshot()[0].priority, -3);
        assert_eq!(
            q.update_input(user, 1, "n".into(), 25),
            Err(Error::InvalidPriority)
        );
        assert_eq!(q.delete_input(user, 1).unwrap().id, 1);
        assert_eq!(
            q.send(&d, msg(1, user, child, 0), 1),
            Err(Error::InvalidSequence)
        );
        q.send(&d, msg(2, user, child, 0), 1).unwrap();
        assert_eq!(q.take(1, Boundary::TurnEnd).unwrap().id, 2);
    }
    #[test]
    fn urgent_input_is_visible_without_consumption_and_pauses_cleanly() {
        let (d, parent, child) = setup();
        let mut q = Mailbox::new(child, 3).unwrap();
        let mut future = msg(1, parent, child, -1);
        future.turn = TurnTarget::Exact(8);
        q.send(&d, future, 7).unwrap();
        assert!(!q.has_urgent_input(7));
        assert!(q.has_urgent_input(8));
        q.set_paused(true);
        assert!(!q.has_urgent_input(8));
        q.set_paused(false);
        assert!(q.has_urgent_input(8));
        assert_eq!(q.take(8, Boundary::ExternalEffect), None);
        assert!(q.has_urgent_input(8));
    }

    #[test]
    fn authenticated_control_has_reserved_capacity_and_shared_replay_fence() {
        let (d, user, child) = setup();
        let mut q = Mailbox::new(child, 1).unwrap();
        q.send(&d, msg(1, user, child, 0), 1).unwrap();
        let mut cancel = msg(2, user, child, 0);
        cancel.kind = Kind::Control(ControlKind::Cancel(Scope::Turn));
        let receipt = q.accept_control(&d, &cancel, 1).unwrap();
        assert_eq!(receipt.id, 2);
        assert_eq!(receipt.position, 0);
        assert_eq!(q.len(), 1);
        assert_eq!(
            q.accept_control(&d, &cancel, 1),
            Err(Error::InvalidSequence)
        );
        assert_eq!(
            q.send(&d, msg(2, user, child, 0), 1),
            Err(Error::InvalidSequence)
        );
        assert_eq!(q.take(1, Boundary::TurnEnd).unwrap().id, 1);
    }
    #[test]
    fn urgent_priority_cannot_indefinitely_starve_normal_inputs() {
        let (d, user, child) = setup();
        let mut q = Mailbox::new(child, 8).unwrap();
        for id in 1..=6 {
            q.send(&d, msg(id, user, child, -5), 1).unwrap();
        }
        q.send(&d, msg(7, user, child, 0), 1).unwrap();
        for id in 1..=4 {
            assert_eq!(
                q.take_class(1, Boundary::TurnEnd, Some(Class::Input))
                    .unwrap()
                    .id,
                id
            );
        }
        assert_eq!(
            q.take_class(1, Boundary::TurnEnd, Some(Class::Input))
                .unwrap()
                .id,
            7
        );
        assert_eq!(
            q.take_class(1, Boundary::TurnEnd, Some(Class::Input))
                .unwrap()
                .id,
            5
        );
    }
    #[test]
    fn sender_history_stays_bounded_across_many_reused_actor_generations() {
        let mut d = ActorDirectory::new(
            RuntimeKey {
                id: nz(2),
                generation: nz(1),
            },
            2,
        )
        .unwrap();
        let root = d.spawn(ActorKind::Agent, None, "root", nz(1)).unwrap();
        d.transition(root, ActorState::Running).unwrap();
        let mut q = Mailbox::new(root, 2).unwrap();
        for _ in 0..(MAX_ACTORS + 45) {
            let child = d
                .spawn(ActorKind::Agent, Some(root), "child", nz(1))
                .unwrap();
            let mut notify = msg(1, child, root, 0);
            notify.kind = Kind::Notify;
            q.send(&d, notify, 1).unwrap();
            assert_eq!(
                q.take_class(1, Boundary::StepEnd, Some(Class::Notify))
                    .unwrap()
                    .from,
                child
            );
            d.transition(child, ActorState::Running).unwrap();
            d.transition(child, ActorState::Settled).unwrap();
            d.release(child).unwrap();
        }
        assert!(q.is_empty());
        let newest = d
            .spawn(ActorKind::Agent, Some(root), "child", nz(1))
            .unwrap();
        q.send(&d, msg(1, newest, root, 0), 1).unwrap();
        assert_eq!(
            q.take_class(1, Boundary::StepEnd, Some(Class::Input))
                .unwrap()
                .from,
            newest
        );
    }
    #[test]
    fn released_sender_cannot_implicitly_resume_a_queued_notification() {
        let mut d = ActorDirectory::new(
            RuntimeKey {
                id: nz(2),
                generation: nz(1),
            },
            2,
        )
        .unwrap();
        let root = d.spawn(ActorKind::Agent, None, "root", nz(1)).unwrap();
        let child = d
            .spawn(ActorKind::Agent, Some(root), "child", nz(1))
            .unwrap();
        let mut q = Mailbox::new(root, 2).unwrap();
        let mut notify = msg(1, child, root, 0);
        notify.kind = Kind::Notify;
        q.send(&d, notify, 1).unwrap();
        d.transition(child, ActorState::Running).unwrap();
        d.transition(child, ActorState::Settled).unwrap();
        d.release(child).unwrap();
        assert!(q.has_stale_sources(&d));
        let refused = q.reject_stale_sources(&d);
        assert_eq!(refused.len(), 1);
        assert_eq!(refused[0].from, child);
        let reused = d
            .spawn(ActorKind::Agent, Some(root), "different", nz(1))
            .unwrap();
        assert_ne!(reused.generation, child.generation);
        assert!(q.is_empty());
        assert_eq!(
            q.send(&d, msg(2, child, root, 0), 1),
            Err(Error::InvalidActor)
        );
    }
}

#[cfg(test)]
mod aw_p6_8c_media_seam_tests {
    use super::super::actor::{ActorKind, RuntimeKey};
    use super::*;
    use core::num::NonZeroU64;
    #[test]
    fn typed_media_messages_cannot_enter_original_text_only_agent_or_consume_sequence() {
        let number = |n| NonZeroU64::new(n).unwrap();
        let runtime = RuntimeKey {
            id: number(12),
            generation: number(1),
        };
        let mut directory = ActorDirectory::new(runtime, 4).unwrap();
        let root = directory
            .spawn(ActorKind::Agent, None, "root", number(1))
            .unwrap();
        let child = directory
            .spawn(ActorKind::Agent, Some(root), "child", number(1))
            .unwrap();
        let mut mailbox = Mailbox::new(child, 4).unwrap();
        let text = ActorMessage {
            id: 1,
            from: root,
            to: child,
            kind: Kind::Input,
            turn: TurnTarget::Auto,
            priority: 0,
            source_sequence: 1,
            payload: "text remains ordinary".into(),
            content_type: ContentType::Text,
        };
        for kind in [
            ContentType::Image,
            ContentType::Audio,
            ContentType::Video,
            ContentType::Binary,
        ] {
            let denied = ActorMessage {
                content_type: kind,
                ..text.clone()
            };
            assert_eq!(
                mailbox.preflight(&directory, &denied, 1),
                Err(Error::InvalidKind)
            );
            assert_eq!(mailbox.len(), 0);
            assert!(kind.needs_media_consent());
        }
        mailbox.send(&directory, text, 1).unwrap();
        assert_eq!(mailbox.len(), 1);
        assert!(ContentType::Text.text_only());
    }
}
