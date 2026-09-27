//! Portable allocation-bounded deterministic turn/tool transitions.
//! The hosted Agent and embedded consumer must use the same sequencing rule.
//! No transport, database, credential, scheduler or allocator lives here.
use alloc::{
    string::{String, ToString},
    vec::Vec,
};

pub const MAX_ROUNDS: u32 = 32;
pub const MAX_CALLS: u32 = 128;
/// One complete model round can propose up to the existing per-turn limit.
/// Host context/output budgets remain independent and must still be enforced.
pub const MAX_BATCH: usize = MAX_CALLS as usize;
/// Hard resource cap for concurrent effects; default is smaller and configurable.
pub const MAX_READ_WAVE: usize = MAX_BATCH;
pub const DEFAULT_PARALLEL_READS: usize = 16;
pub const DEFAULT_PARALLEL_BASH: usize = 16;

/// A wave's execution class is host-authorized, never model-selected.
/// The model may request concurrency for individually approved Bash tools;
/// its request cannot change the tool's declared Process effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaveKind {
    Read,
    Bash,
}

/// Choose a maximal bounded prefix of exact Read effects. Unknown, Write and
/// Process are hard barriers, even when they advertise advisory readOnlyHint.
/// Less than two reads is a sequential tool, preserving existing single-step
/// trace and journal behavior in both hosted and embedded Agents.
pub fn read_wave_len(effects: &[super::capability::Effect], limit: usize) -> usize {
    let cap = limit.clamp(1, MAX_READ_WAVE);
    let n = effects
        .iter()
        .take(cap)
        .take_while(|effect| **effect == super::capability::Effect::Read)
        .count();
    if n >= 2 {
        n
    } else {
        0
    }
}
pub const MAX_CALL_ID_BYTES: usize = 128;
pub const MAX_CALL_NAME_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidLimits,
    InvalidTurn,
    InvalidTransition,
    Exhausted,
    InvalidCall,
    DuplicateCall,
    Denied,
    Capacity,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    ReadyModel,
    WaitingModel,
    BatchProposed,
    AwaitingDenials,
    ReadyTool,
    AwaitingIntent,
    ReadyEffect,
    Executing,
    AwaitingReceipt,
    ReadWavePreparing,
    ReadWaveAwaitingIntent,
    ReadWaveReady,
    ReadWaveRunning,
    ReadWaveSettled,
    Completed,
    Incomplete,
    Blocked,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    ModelStarted(u32),
    TextCompleted,
    TextIncomplete,
    BatchProposed(u16),
    BatchAdmitted(u16),
    BatchRejected(u16),
    DenialRecorded(u16),
    ToolIntent(u16),
    IntentRecorded(u16),
    EffectStarted(u16),
    EffectSettled(u16),
    ReceiptRecorded(u16),
    ReadWaveProposed(u16),
    ReadWaveIntent(u16),
    ReadWaveIntentRecorded(u16),
    ReadWaveStarted(u16),
    ReadWaveSettled(u16),
    ReadWaveReceiptRecorded(u16),
    BashWaveProposed(u16),
    BashWaveIntent(u16),
    BashWaveIntentRecorded(u16),
    BashWaveStarted(u16),
    BashWaveSettled(u16),
    BashWaveReceiptRecorded(u16),
    Blocked,
}
#[derive(Clone, Copy, Debug)]
pub struct ProposedCall<'a> {
    pub id: &'a str,
    pub name: &'a str,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_rounds: u32,
    pub max_calls: u32,
}
impl Limits {
    pub fn new(max_rounds: u32, max_calls: u32) -> Result<Self, Error> {
        if max_rounds == 0 || max_rounds > MAX_ROUNDS || max_calls > MAX_CALLS {
            return Err(Error::InvalidLimits);
        }
        Ok(Self {
            max_rounds,
            max_calls,
        })
    }
}

/// Host-driven state: external effects cannot begin before journal intent
/// acknowledgement and receipt acknowledgement precedes any next action.
/// Uncertain effects cannot be silently retried after errors.
#[derive(Debug)]
pub struct TurnMachine {
    turn_id: u64,
    limits: Limits,
    phase: Phase,
    rounds: u32,
    calls_used: u32,
    seen: Vec<String>,
    pending: Vec<String>,
    index: usize,
    wave_len: usize,
    wave_kind: Option<WaveKind>,
    wave_intents: usize,
    wave_receipts: usize,
    unsafe_intent_seen: bool,
}
fn valid_call_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_CALL_ID_BYTES
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
}
fn valid_call_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_CALL_NAME_BYTES
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
}
impl TurnMachine {
    pub fn new(turn_id: u64, limits: Limits) -> Result<Self, Error> {
        if turn_id == 0 {
            return Err(Error::InvalidTurn);
        }
        Ok(Self {
            turn_id,
            limits,
            phase: Phase::ReadyModel,
            rounds: 0,
            calls_used: 0,
            seen: Vec::new(),
            pending: Vec::new(),
            index: 0,
            wave_len: 0,
            wave_kind: None,
            wave_intents: 0,
            wave_receipts: 0,
            unsafe_intent_seen: false,
        })
    }
    pub fn turn_id(&self) -> u64 {
        self.turn_id
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn rounds(&self) -> u32 {
        self.rounds
    }
    pub fn calls_used(&self) -> u32 {
        self.calls_used
    }
    pub fn needs_reconciliation_if_failed(&self) -> bool {
        self.unsafe_intent_seen
    }
    pub fn pending_call_id(&self) -> Option<&str> {
        if matches!(
            self.phase,
            Phase::ReadyTool
                | Phase::AwaitingIntent
                | Phase::ReadyEffect
                | Phase::Executing
                | Phase::AwaitingReceipt
        ) {
            self.pending.get(self.index).map(String::as_str)
        } else {
            None
        }
    }
    pub fn begin_model(&mut self) -> Result<Event, Error> {
        if self.phase != Phase::ReadyModel {
            return Err(Error::InvalidTransition);
        }
        if self.rounds >= self.limits.max_rounds {
            return Err(Error::Exhausted);
        }
        self.rounds += 1;
        self.phase = Phase::WaitingModel;
        Ok(Event::ModelStarted(self.rounds))
    }
    pub fn complete_text(&mut self, complete: bool, nonempty: bool) -> Result<Event, Error> {
        if self.phase != Phase::WaitingModel {
            return Err(Error::InvalidTransition);
        }
        if complete && nonempty {
            self.phase = Phase::Completed;
            Ok(Event::TextCompleted)
        } else {
            self.phase = Phase::Incomplete;
            Ok(Event::TextIncomplete)
        }
    }
    /// Atomically validate an entire model-proposed batch. IDs must be unique
    /// across the turn; incomplete proposals can never enter an effect phase.
    pub fn propose_batch(
        &mut self,
        calls: &[ProposedCall<'_>],
        valid_finish: bool,
    ) -> Result<Event, Error> {
        if self.phase != Phase::WaitingModel {
            return Err(Error::InvalidTransition);
        }
        if !valid_finish {
            return Err(Error::InvalidCall);
        }
        if calls.is_empty()
            || calls.len() > MAX_BATCH
            || self.calls_used.saturating_add(calls.len() as u32) > self.limits.max_calls
        {
            return Err(Error::Exhausted);
        }
        for (index, call) in calls.iter().enumerate() {
            if !valid_call_id(call.id)
                || !valid_call_name(call.name)
                || self.seen.iter().any(|past| past == call.id)
                || calls[..index].iter().any(|prior| prior.id == call.id)
            {
                return Err(
                    if self.seen.iter().any(|past| past == call.id)
                        || calls[..index].iter().any(|prior| prior.id == call.id)
                    {
                        Error::DuplicateCall
                    } else {
                        Error::InvalidCall
                    },
                );
            }
        }
        // Reject an alloc capacity error before changing protocol state.
        self.pending
            .try_reserve_exact(calls.len())
            .map_err(|_| Error::Capacity)?;
        self.seen
            .try_reserve(calls.len())
            .map_err(|_| Error::Capacity)?;
        self.pending.clear();
        for call in calls {
            self.pending.push(call.id.to_string());
        }
        self.index = 0;
        self.phase = Phase::BatchProposed;
        Ok(Event::BatchProposed(calls.len() as u16))
    }
    /// Host already validated all current grants, schemas, hooks and capacity.
    pub fn admit_batch(&mut self) -> Result<Event, Error> {
        if self.phase != Phase::BatchProposed {
            return Err(Error::InvalidTransition);
        }
        for id in &self.pending {
            self.seen.push(id.clone());
        }
        self.phase = Phase::ReadyTool;
        Ok(Event::BatchAdmitted(self.pending.len() as u16))
    }
    /// Reject the WHOLE complete provider batch before ANY effect intent.
    /// All call IDs are still paired with one explicit no-effect denial record.
    /// Denied calls never enter Executing, nor mark a possible external hazard.
    pub fn reject_batch(&mut self) -> Result<Event, Error> {
        if self.phase != Phase::BatchProposed {
            return Err(Error::InvalidTransition);
        }
        self.seen.extend(self.pending.iter().cloned());
        self.phase = Phase::AwaitingDenials;
        Ok(Event::BatchRejected(self.pending.len() as u16))
    }
    /// The owning host first appends JournalEntry::ToolDenied and receives
    /// its factual durability acknowledgement. Only then may this machine
    /// advance to the next provider request. No ToolStarted/ToolFinished pair
    /// can be mistaken for a denial; the tool was never executed.
    pub fn denial_recorded(&mut self, id: &str) -> Result<Event, Error> {
        if self.phase != Phase::AwaitingDenials
            || self.pending.get(self.index).is_none_or(|item| item != id)
        {
            return Err(Error::InvalidTransition);
        }
        let index = self.index as u16;
        self.index += 1;
        self.calls_used += 1;
        if self.index == self.pending.len() {
            self.pending.clear();
            self.index = 0;
            self.phase = Phase::ReadyModel;
        }
        Ok(Event::DenialRecorded(index))
    }
    /// Begin 2..=MAX_BATCH adjacent already-admitted, host-classified
    /// effects. Bash is ALWAYS Process (unsafe on interruption) and requires
    /// independent user consent + model concurrency request in the std host.
    pub fn begin_wave(&mut self, kind: WaveKind, size: usize) -> Result<Event, Error> {
        if self.phase != Phase::ReadyTool
            || !(2..=MAX_READ_WAVE).contains(&size)
            || self.index.saturating_add(size) > self.pending.len()
            || self.calls_used.saturating_add(size as u32) > self.limits.max_calls
        {
            return Err(Error::InvalidTransition);
        }
        self.wave_kind = Some(kind);
        self.wave_len = size;
        self.wave_intents = 0;
        self.wave_receipts = 0;
        self.phase = Phase::ReadWavePreparing;
        Ok(match kind {
            WaveKind::Read => Event::ReadWaveProposed(size as u16),
            WaveKind::Bash => Event::BashWaveProposed(size as u16),
        })
    }
    /// R9 compatibility: existing read callers use the same single machine.
    pub fn begin_read_wave(&mut self, size: usize) -> Result<Event, Error> {
        self.begin_wave(WaveKind::Read, size)
    }
    /// All intents are appended in proposal order; no effect starts until the
    /// final intent is acknowledged. Exact tool effect is never downgraded.
    pub fn prepare_wave_intent(
        &mut self,
        id: &str,
        effect: super::capability::Effect,
    ) -> Result<Event, Error> {
        let Some(kind) = self.wave_kind else {
            return Err(Error::InvalidTransition);
        };
        let expected = match kind {
            WaveKind::Read => super::capability::Effect::Read,
            WaveKind::Bash => super::capability::Effect::Process,
        };
        if self.phase != Phase::ReadWavePreparing
            || effect != expected
            || self
                .pending
                .get(self.index + self.wave_intents)
                .is_none_or(|next| next != id)
        {
            return Err(Error::InvalidTransition);
        }
        if kind == WaveKind::Bash {
            self.unsafe_intent_seen = true;
        }
        self.phase = Phase::ReadWaveAwaitingIntent;
        Ok(match kind {
            WaveKind::Read => Event::ReadWaveIntent(self.wave_intents as u16),
            WaveKind::Bash => Event::BashWaveIntent(self.wave_intents as u16),
        })
    }
    pub fn prepare_read_intent(
        &mut self,
        id: &str,
        effect: super::capability::Effect,
    ) -> Result<Event, Error> {
        if self.wave_kind != Some(WaveKind::Read) {
            return Err(Error::InvalidTransition);
        }
        self.prepare_wave_intent(id, effect)
    }
    pub fn wave_intent_recorded(&mut self) -> Result<Event, Error> {
        if self.phase != Phase::ReadWaveAwaitingIntent {
            return Err(Error::InvalidTransition);
        }
        let index = self.wave_intents;
        self.wave_intents += 1;
        self.phase = if self.wave_intents == self.wave_len {
            Phase::ReadWaveReady
        } else {
            Phase::ReadWavePreparing
        };
        Ok(match self.wave_kind.ok_or(Error::InvalidTransition)? {
            WaveKind::Read => Event::ReadWaveIntentRecorded(index as u16),
            WaveKind::Bash => Event::BashWaveIntentRecorded(index as u16),
        })
    }
    pub fn read_intent_recorded(&mut self) -> Result<Event, Error> {
        if self.wave_kind != Some(WaveKind::Read) {
            return Err(Error::InvalidTransition);
        }
        self.wave_intent_recorded()
    }
    /// The std host rechecks EVERY actor/source grant after flush, then
    /// again in each future. A denied grant never starts its wave.
    pub fn start_wave(&mut self, currently_authorized: bool) -> Result<Event, Error> {
        if self.phase != Phase::ReadWaveReady {
            return Err(Error::InvalidTransition);
        }
        if !currently_authorized {
            return Err(Error::Denied);
        }
        self.calls_used += self.wave_len as u32;
        self.phase = Phase::ReadWaveRunning;
        Ok(match self.wave_kind.ok_or(Error::InvalidTransition)? {
            WaveKind::Read => Event::ReadWaveStarted(self.wave_len as u16),
            WaveKind::Bash => Event::BashWaveStarted(self.wave_len as u16),
        })
    }
    pub fn start_read_wave(&mut self, currently_authorized: bool) -> Result<Event, Error> {
        if self.wave_kind != Some(WaveKind::Read) {
            return Err(Error::InvalidTransition);
        }
        self.start_wave(currently_authorized)
    }
    /// Cancellation/tool error blocks the entire wave and never causes replay.
    pub fn settle_wave(&mut self) -> Result<Event, Error> {
        if self.phase != Phase::ReadWaveRunning {
            return Err(Error::InvalidTransition);
        }
        self.phase = Phase::ReadWaveSettled;
        Ok(match self.wave_kind.ok_or(Error::InvalidTransition)? {
            WaveKind::Read => Event::ReadWaveSettled(self.wave_len as u16),
            WaveKind::Bash => Event::BashWaveSettled(self.wave_len as u16),
        })
    }
    pub fn settle_read_wave(&mut self) -> Result<Event, Error> {
        if self.wave_kind != Some(WaveKind::Read) {
            return Err(Error::InvalidTransition);
        }
        self.settle_wave()
    }
    /// Factual receipts must be acknowledged in original order before
    /// releasing a next wave, mutation or model invocation.
    pub fn wave_receipt_recorded(&mut self) -> Result<Event, Error> {
        if self.phase != Phase::ReadWaveSettled {
            return Err(Error::InvalidTransition);
        }
        let kind = self.wave_kind.ok_or(Error::InvalidTransition)?;
        let finished = self.wave_receipts as u16;
        self.wave_receipts += 1;
        if self.wave_receipts == self.wave_len {
            self.index += self.wave_len;
            self.wave_len = 0;
            self.wave_kind = None;
            self.wave_intents = 0;
            self.wave_receipts = 0;
            if self.index == self.pending.len() {
                self.pending.clear();
                self.index = 0;
                self.phase = Phase::ReadyModel;
            } else {
                self.phase = Phase::ReadyTool;
            }
        }
        Ok(match kind {
            WaveKind::Read => Event::ReadWaveReceiptRecorded(finished),
            WaveKind::Bash => Event::BashWaveReceiptRecorded(finished),
        })
    }
    pub fn read_receipt_recorded(&mut self) -> Result<Event, Error> {
        if self.wave_kind != Some(WaveKind::Read) {
            return Err(Error::InvalidTransition);
        }
        self.wave_receipt_recorded()
    }
    /// Host prepares a journal intent before entering an external tool.
    pub fn prepare_tool(
        &mut self,
        id: &str,
        effect: super::capability::Effect,
    ) -> Result<Event, Error> {
        if self.phase != Phase::ReadyTool
            || self.pending.get(self.index).is_none_or(|item| item != id)
        {
            return Err(Error::InvalidTransition);
        }
        if effect != super::capability::Effect::Read {
            self.unsafe_intent_seen = true;
        }
        self.phase = Phase::AwaitingIntent;
        Ok(Event::ToolIntent(self.index as u16))
    }
    /// Only after host journal append acknowledgement; memory-only is volatile.
    pub fn intent_recorded(&mut self) -> Result<Event, Error> {
        if self.phase != Phase::AwaitingIntent {
            return Err(Error::InvalidTransition);
        }
        self.phase = Phase::ReadyEffect;
        Ok(Event::IntentRecorded(self.index as u16))
    }
    /// Recheck grant after journaling and before tool entry, never just at discovery.
    pub fn start_effect(&mut self, currently_authorized: bool) -> Result<Event, Error> {
        if self.phase != Phase::ReadyEffect {
            return Err(Error::InvalidTransition);
        }
        if !currently_authorized {
            return Err(Error::Denied);
        }
        self.phase = Phase::Executing;
        self.calls_used += 1;
        Ok(Event::EffectStarted(self.index as u16))
    }
    pub fn settle_effect(&mut self) -> Result<Event, Error> {
        if self.phase != Phase::Executing {
            return Err(Error::InvalidTransition);
        }
        self.phase = Phase::AwaitingReceipt;
        Ok(Event::EffectSettled(self.index as u16))
    }
    /// Only after persisted receipt; next call/model never begins prematurely.
    pub fn receipt_recorded(&mut self) -> Result<Event, Error> {
        if self.phase != Phase::AwaitingReceipt {
            return Err(Error::InvalidTransition);
        }
        let index = self.index as u16;
        self.index += 1;
        if self.index == self.pending.len() {
            self.pending.clear();
            self.index = 0;
            self.phase = Phase::ReadyModel;
        } else {
            self.phase = Phase::ReadyTool;
        }
        Ok(Event::ReceiptRecorded(index))
    }
    pub fn fail(&mut self) -> Event {
        self.phase = Phase::Blocked;
        Event::Blocked
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn limits() -> Limits {
        Limits::new(3, 4).unwrap()
    }
    fn single() -> [ProposedCall<'static>; 1] {
        [ProposedCall {
            id: "call_1",
            name: "echo",
        }]
    }
    #[test]
    fn tool_cycle_enforces_journal_and_receipt_order() {
        let mut m = TurnMachine::new(9, limits()).unwrap();
        assert_eq!(m.begin_model(), Ok(Event::ModelStarted(1)));
        assert_eq!(
            m.propose_batch(&single(), true),
            Ok(Event::BatchProposed(1))
        );
        assert_eq!(m.start_effect(true), Err(Error::InvalidTransition));
        assert_eq!(m.admit_batch(), Ok(Event::BatchAdmitted(1)));
        assert_eq!(
            m.prepare_tool("call_1", super::super::capability::Effect::Unknown),
            Ok(Event::ToolIntent(0))
        );
        assert_eq!(m.start_effect(true), Err(Error::InvalidTransition));
        assert_eq!(m.intent_recorded(), Ok(Event::IntentRecorded(0)));
        assert_eq!(m.start_effect(false), Err(Error::Denied));
        assert_eq!(m.calls_used(), 0);
        assert_eq!(m.start_effect(true), Ok(Event::EffectStarted(0)));
        assert_eq!(m.begin_model(), Err(Error::InvalidTransition));
        assert_eq!(m.receipt_recorded(), Err(Error::InvalidTransition));
        assert_eq!(m.settle_effect(), Ok(Event::EffectSettled(0)));
        assert_eq!(m.receipt_recorded(), Ok(Event::ReceiptRecorded(0)));
        assert_eq!(m.begin_model(), Ok(Event::ModelStarted(2)));
        assert_eq!(m.complete_text(true, true), Ok(Event::TextCompleted));
        assert!(m.needs_reconciliation_if_failed());
        assert_eq!(m.begin_model(), Err(Error::InvalidTransition));
    }
    #[test]
    fn batches_validate_all_ids_and_bounds_before_effect() {
        let mut m = TurnMachine::new(1, limits()).unwrap();
        m.begin_model().unwrap();
        let duplicate = [
            ProposedCall {
                id: "id",
                name: "a",
            },
            ProposedCall {
                id: "id",
                name: "b",
            },
        ];
        assert_eq!(m.propose_batch(&duplicate, true), Err(Error::DuplicateCall));
        assert_eq!(m.phase(), Phase::WaitingModel);
        assert_eq!(m.propose_batch(&single(), false), Err(Error::InvalidCall));
        assert_eq!(
            m.propose_batch(&single(), true),
            Ok(Event::BatchProposed(1))
        );
        m.admit_batch().unwrap();
        m.prepare_tool("call_1", super::super::capability::Effect::Read)
            .unwrap();
        assert!(!m.needs_reconciliation_if_failed());
        m.intent_recorded().unwrap();
        m.start_effect(true).unwrap();
        m.settle_effect().unwrap();
        m.receipt_recorded().unwrap();
        m.begin_model().unwrap();
        assert_eq!(m.propose_batch(&single(), true), Err(Error::DuplicateCall));
    }
    #[test]
    fn complete_multitool_batch_requires_each_exact_receipt_before_next_effect() {
        let mut m = TurnMachine::new(11, Limits::new(2, 2).unwrap()).unwrap();
        m.begin_model().unwrap();
        let both = [
            ProposedCall {
                id: "first",
                name: "read",
            },
            ProposedCall {
                id: "second",
                name: "write",
            },
        ];
        assert_eq!(m.propose_batch(&both, true), Ok(Event::BatchProposed(2)));
        m.admit_batch().unwrap();
        assert_eq!(m.pending_call_id(), Some("first"));
        assert_eq!(
            m.prepare_tool("second", super::super::capability::Effect::Write),
            Err(Error::InvalidTransition)
        );
        m.prepare_tool("first", super::super::capability::Effect::Read)
            .unwrap();
        m.intent_recorded().unwrap();
        m.start_effect(true).unwrap();
        assert_eq!(
            m.prepare_tool("second", super::super::capability::Effect::Write),
            Err(Error::InvalidTransition)
        );
        m.settle_effect().unwrap();
        assert_eq!(m.begin_model(), Err(Error::InvalidTransition));
        assert_eq!(m.receipt_recorded(), Ok(Event::ReceiptRecorded(0)));
        assert_eq!(m.pending_call_id(), Some("second"));
        m.prepare_tool("second", super::super::capability::Effect::Write)
            .unwrap();
        m.intent_recorded().unwrap();
        m.start_effect(true).unwrap();
        m.settle_effect().unwrap();
        m.receipt_recorded().unwrap();
        assert_eq!(m.phase(), Phase::ReadyModel);
        assert_eq!(m.calls_used(), 2);
        assert!(m.needs_reconciliation_if_failed());
        m.begin_model().unwrap();
        assert_eq!(
            m.propose_batch(
                &[ProposedCall {
                    id: "new",
                    name: "echo"
                }],
                true
            ),
            Err(Error::Exhausted)
        );
        m.complete_text(true, true).unwrap();
    }
    #[test]
    fn parallel_read_waves_are_explicitly_bounded_by_effect_and_barrier() {
        use super::super::capability::Effect;
        assert_eq!(
            read_wave_len(
                &[Effect::Read, Effect::Read, Effect::Read, Effect::Unknown],
                4
            ),
            3
        );
        assert_eq!(
            read_wave_len(
                &[Effect::Read, Effect::Read, Effect::Write, Effect::Read],
                4
            ),
            2
        );
        assert_eq!(
            read_wave_len(&[Effect::Read, Effect::Unknown, Effect::Read], 4),
            0
        );
        assert_eq!(
            read_wave_len(&[Effect::Process, Effect::Read, Effect::Read], 4),
            0
        );
        assert_eq!(read_wave_len(&[Effect::Read; 8], 100), 8);
        assert_eq!(read_wave_len(&[Effect::Read; 3], 1), 0);
    }
    #[test]
    fn no_parallel_read_may_run_before_every_durable_intent_or_before_ordered_receipts() {
        use super::super::capability::Effect;
        let mut machine = TurnMachine::new(5, Limits::new(2, 3).unwrap()).unwrap();
        let proposed = [
            ProposedCall {
                id: "r1",
                name: "read",
            },
            ProposedCall {
                id: "r2",
                name: "read",
            },
            ProposedCall {
                id: "mut",
                name: "write",
            },
        ];
        machine.begin_model().unwrap();
        machine.propose_batch(&proposed, true).unwrap();
        machine.admit_batch().unwrap();
        assert_eq!(machine.begin_read_wave(2), Ok(Event::ReadWaveProposed(2)));
        assert_eq!(machine.start_read_wave(true), Err(Error::InvalidTransition));
        assert_eq!(
            machine.prepare_read_intent("r1", Effect::Unknown),
            Err(Error::InvalidTransition)
        );
        assert_eq!(
            machine.prepare_read_intent("r2", Effect::Read),
            Err(Error::InvalidTransition)
        );
        assert_eq!(
            machine.prepare_read_intent("r1", Effect::Read),
            Ok(Event::ReadWaveIntent(0))
        );
        assert_eq!(machine.start_read_wave(true), Err(Error::InvalidTransition));
        machine.read_intent_recorded().unwrap();
        machine.prepare_read_intent("r2", Effect::Read).unwrap();
        machine.read_intent_recorded().unwrap();
        assert_eq!(machine.start_read_wave(false), Err(Error::Denied));
        assert_eq!(machine.start_read_wave(true), Ok(Event::ReadWaveStarted(2)));
        assert_eq!(machine.begin_model(), Err(Error::InvalidTransition));
        assert_eq!(
            machine.prepare_tool("mut", Effect::Write),
            Err(Error::InvalidTransition)
        );
        assert_eq!(
            machine.read_receipt_recorded(),
            Err(Error::InvalidTransition)
        );
        machine.settle_read_wave().unwrap();
        assert_eq!(
            machine.read_receipt_recorded(),
            Ok(Event::ReadWaveReceiptRecorded(0))
        );
        assert_eq!(machine.begin_model(), Err(Error::InvalidTransition));
        assert_eq!(
            machine.prepare_tool("mut", Effect::Write),
            Err(Error::InvalidTransition)
        );
        assert_eq!(
            machine.read_receipt_recorded(),
            Ok(Event::ReadWaveReceiptRecorded(1))
        );
        assert_eq!(machine.phase(), Phase::ReadyTool);
        assert_eq!(machine.pending_call_id(), Some("mut"));
        machine.prepare_tool("mut", Effect::Write).unwrap();
        machine.intent_recorded().unwrap();
        machine.start_effect(true).unwrap();
        machine.settle_effect().unwrap();
        machine.receipt_recorded().unwrap();
        assert_eq!(machine.phase(), Phase::ReadyModel);
        assert_eq!(machine.calls_used(), 3);
        assert!(machine.needs_reconciliation_if_failed());
    }
    #[test]
    fn configured_sixteen_read_wave_and_larger_are_safely_bounded() {
        use super::super::capability::Effect;
        assert_eq!(DEFAULT_PARALLEL_READS, 16);
        assert_eq!(
            read_wave_len(&[Effect::Read; 16], DEFAULT_PARALLEL_READS),
            16
        );
        assert_eq!(read_wave_len(&[Effect::Read; 32], 32), 32);
        assert_eq!(read_wave_len(&[Effect::Read; 32], 1), 0);
        let mut m = TurnMachine::new(4, Limits::new(2, 64).unwrap()).unwrap();
        let calls = (0..16)
            .map(|i| alloc::format!("r{i}"))
            .collect::<alloc::vec::Vec<_>>();
        let proposed = calls
            .iter()
            .map(|id| ProposedCall { id, name: "read" })
            .collect::<alloc::vec::Vec<_>>();
        m.begin_model().unwrap();
        m.propose_batch(&proposed, true).unwrap();
        m.admit_batch().unwrap();
        m.begin_read_wave(16).unwrap();
        for id in &calls {
            m.prepare_read_intent(id, Effect::Read).unwrap();
            m.read_intent_recorded().unwrap();
        }
        m.start_read_wave(true).unwrap();
        m.settle_read_wave().unwrap();
        for _ in 0..16 {
            m.read_receipt_recorded().unwrap();
        }
        assert_eq!(m.phase(), Phase::ReadyModel);
        assert_eq!(m.calls_used(), 16);
    }
    #[test]
    fn parallel_bash_retains_process_effect_and_requires_receipts() {
        use super::super::capability::Effect;
        let mut m = TurnMachine::new(8, Limits::new(2, 4).unwrap()).unwrap();
        m.begin_model().unwrap();
        m.propose_batch(
            &[
                ProposedCall {
                    id: "cmd1",
                    name: "bash",
                },
                ProposedCall {
                    id: "cmd2",
                    name: "bash",
                },
            ],
            true,
        )
        .unwrap();
        m.admit_batch().unwrap();
        assert_eq!(
            m.begin_wave(WaveKind::Bash, 2),
            Ok(Event::BashWaveProposed(2))
        );
        assert_eq!(
            m.prepare_wave_intent("cmd1", Effect::Read),
            Err(Error::InvalidTransition)
        );
        assert_eq!(
            m.prepare_wave_intent("cmd1", Effect::Process),
            Ok(Event::BashWaveIntent(0))
        );
        assert!(m.needs_reconciliation_if_failed());
        assert_eq!(m.start_wave(true), Err(Error::InvalidTransition));
        m.wave_intent_recorded().unwrap();
        m.prepare_wave_intent("cmd2", Effect::Process).unwrap();
        m.wave_intent_recorded().unwrap();
        assert_eq!(m.start_wave(false), Err(Error::Denied));
        assert_eq!(m.start_wave(true), Ok(Event::BashWaveStarted(2)));
        assert_eq!(m.begin_model(), Err(Error::InvalidTransition));
        assert_eq!(m.settle_wave(), Ok(Event::BashWaveSettled(2)));
        assert_eq!(
            m.wave_receipt_recorded(),
            Ok(Event::BashWaveReceiptRecorded(0))
        );
        assert_eq!(m.begin_model(), Err(Error::InvalidTransition));
        assert_eq!(
            m.wave_receipt_recorded(),
            Ok(Event::BashWaveReceiptRecorded(1))
        );
        assert_eq!(m.calls_used(), 2);
        assert!(m.needs_reconciliation_if_failed());
        assert_eq!(m.phase(), Phase::ReadyModel);
    }
    #[test]
    fn canceled_read_wave_stops_before_start_or_reuse() {
        use super::super::capability::Effect;
        let mut m = TurnMachine::new(12, Limits::new(1, 2).unwrap()).unwrap();
        m.begin_model().unwrap();
        m.propose_batch(
            &[
                ProposedCall { id: "a", name: "r" },
                ProposedCall { id: "b", name: "r" },
            ],
            true,
        )
        .unwrap();
        m.admit_batch().unwrap();
        m.begin_read_wave(2).unwrap();
        m.prepare_read_intent("a", Effect::Read).unwrap();
        m.read_intent_recorded().unwrap();
        m.fail();
        assert_eq!(m.start_read_wave(true), Err(Error::InvalidTransition));
        assert!(!m.needs_reconciliation_if_failed());
    }
    #[test]
    fn aborts_and_budgets_are_terminal_without_replay() {
        assert!(Limits::new(0, 4).is_err());
        assert!(TurnMachine::new(0, limits()).is_err());
        let mut m = TurnMachine::new(2, Limits::new(1, 1).unwrap()).unwrap();
        m.begin_model().unwrap();
        assert_eq!(
            m.propose_batch(&single(), true),
            Ok(Event::BatchProposed(1))
        );
        m.admit_batch().unwrap();
        m.prepare_tool("call_1", super::super::capability::Effect::Write)
            .unwrap();
        assert_eq!(m.fail(), Event::Blocked);
        assert!(m.needs_reconciliation_if_failed());
        assert_eq!(m.intent_recorded(), Err(Error::InvalidTransition));
    }
    #[test]
    fn rejected_batch_has_no_effect_intents_and_is_fully_receipted() {
        let mut machine = TurnMachine::new(1, Limits::new(3, 4).unwrap()).unwrap();
        assert_eq!(machine.begin_model(), Ok(Event::ModelStarted(1)));
        let calls = [
            ProposedCall {
                id: "a",
                name: "read",
            },
            ProposedCall {
                id: "b",
                name: "write",
            },
        ];
        assert_eq!(
            machine.propose_batch(&calls, true),
            Ok(Event::BatchProposed(2))
        );
        assert_eq!(machine.reject_batch(), Ok(Event::BatchRejected(2)));
        assert_eq!(machine.begin_model(), Err(Error::InvalidTransition));
        assert_eq!(
            machine.prepare_tool("a", super::super::capability::Effect::Read),
            Err(Error::InvalidTransition)
        );
        assert_eq!(machine.denial_recorded("b"), Err(Error::InvalidTransition));
        assert_eq!(machine.denial_recorded("a"), Ok(Event::DenialRecorded(0)));
        assert_eq!(machine.phase(), Phase::AwaitingDenials);
        assert_eq!(machine.denial_recorded("b"), Ok(Event::DenialRecorded(1)));
        assert_eq!(machine.phase(), Phase::ReadyModel);
        assert_eq!(machine.calls_used(), 2);
        assert!(!machine.needs_reconciliation_if_failed());
        assert_eq!(machine.begin_model(), Ok(Event::ModelStarted(2)));
        assert_eq!(
            machine.propose_batch(&calls, true),
            Err(Error::DuplicateCall)
        );
    }
    #[test]
    fn reject_only_before_any_admitted_tool_effect() {
        let mut machine = TurnMachine::new(1, limits()).unwrap();
        machine.begin_model().unwrap();
        machine.propose_batch(&single(), true).unwrap();
        machine.admit_batch().unwrap();
        assert_eq!(machine.reject_batch(), Err(Error::InvalidTransition));
    }
}
