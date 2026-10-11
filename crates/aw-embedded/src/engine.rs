use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use aw_agent_kernel::{
    capability::{Capability, Effect, Grant},
    turn::{Error as TransitionError, Event, Limits, Phase, ProposedCall, TurnMachine},
};
use serde_json::Value;

pub const MAX_PROMPT_BYTES: usize = 128 * 1024;
pub const MAX_CONTEXT_BYTES: usize = 1024 * 1024;
pub const MAX_RESULT_BYTES: usize = 128 * 1024;
pub const MAX_ARGUMENT_BYTES: usize = 64 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 512 * 1024;
pub const MAX_TRACE: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Transition(TransitionError),
    NotStarted,
    InvalidConfiguration,
    InvalidResponse,
    UnavailableClock,
    UnavailableEntropy,
    RegressedClock,
    Deadline,
    TooLarge,
    Denied,
    JournalUnavailable,
    ProviderUnavailable,
    ToolUnavailable,
    Failed,
}
impl From<TransitionError> for Error {
    fn from(value: TransitionError) -> Self {
        Self::Transition(value)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortError {
    Unavailable,
    Denied,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finish {
    Final,
    Tools,
    Incomplete,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: String,
    pub call_id: Option<String>,
    pub tool_name: Option<String>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ModelReply {
    pub text: String,
    pub finish: Finish,
    pub calls: Vec<ToolCall>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct Identity<'a> {
    pub actor: &'a str,
    pub site: &'a str,
    pub account: &'a str,
    pub model: &'a str,
}
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub max_rounds: u32,
    pub max_calls: u32,
    /// The host's monotonic clock unit. The host enforces deadlines inside any
    /// potentially blocking provider/tool callback; this adapter checks around
    /// each call and never assumes a callback can be preempted.
    pub deadline_ticks: u64,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            max_rounds: 8,
            max_calls: 32,
            deadline_ticks: 600_000,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    NeedsModel,
    NeedsTool,
    Completed,
    Incomplete,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Created,
    NeedsModel,
    NeedsTool,
    Completed,
    Incomplete,
    Blocked,
}

/// Immutable host-chosen registration and exact source-bound grant. Offered
/// metadata is not authority; every effect rechecks Grant::permits.
#[derive(Clone, Copy)]
pub struct ToolRegistration<'a> {
    pub capability: Capability<'a>,
    pub grant: Grant<'a>,
}

#[derive(Clone, Copy, Debug)]
pub enum JournalRecord<'a> {
    TurnStarted {
        turn_id: u64,
        nonce: u64,
        actor: &'a str,
        account: &'a str,
        model: &'a str,
        prompt: &'a str,
    },
    ModelStarted {
        turn_id: u64,
        ordinal: u32,
    },
    ModelSettled {
        turn_id: u64,
        ordinal: u32,
        finish: Finish,
    },
    Assistant {
        turn_id: u64,
        text: &'a str,
        tools: usize,
    },
    ToolIntent {
        turn_id: u64,
        call: &'a ToolCall,
        effect: Effect,
    },
    ToolReceipt {
        turn_id: u64,
        call: &'a ToolCall,
        output: &'a ToolResult,
    },
    TurnFinished {
        turn_id: u64,
        outcome: Progress,
    },
}
/// Explicit embedded host ports. The host owns clock/entropy/credentials,
/// provider, tool executor and genuinely durable append+flush implementation.
/// Successful append means the record is committed, not merely queued.
/// No implicit retry/reconnect or default credentials are defined here.
pub trait HostPorts {
    fn monotonic_ticks(&mut self) -> Option<u64>;
    fn secure_entropy(&mut self, output: &mut [u8]) -> bool;
    fn append(&mut self, record: JournalRecord<'_>) -> Result<(), PortError>;
    fn model(
        &mut self,
        messages: &[Message],
        identity: Identity<'_>,
    ) -> Result<ModelReply, PortError>;
    fn registration(&self, name: &str) -> Option<ToolRegistration<'_>>;
    fn validate_tool(&self, call: &ToolCall) -> Result<(), PortError>;
    fn execute_tool(&mut self, call: &ToolCall) -> Result<ToolResult, PortError>;
}

/// Thin host-side data projection around the exact shared TurnMachine.
/// A host drives model_step/tool_step; there is deliberately NO private
/// Agent/tool loop, second policy catalog or automatic effect execution.
pub struct EmbeddedTurn<'a> {
    pub identity: Identity<'a>,
    pub options: Options,
    machine: TurnMachine,
    status: Status,
    messages: Vec<Message>,
    calls: Vec<ToolCall>,
    next_call: usize,
    answer: String,
    started_at: Option<u64>,
    last_clock: u64,
    nonce: u64,
    trace: Vec<Event>,
}
impl<'a> EmbeddedTurn<'a> {
    pub fn new(turn_id: u64, identity: Identity<'a>, options: Options) -> Result<Self, Error> {
        if identity.actor.is_empty()
            || identity.actor.len() > 128
            || identity.site.is_empty()
            || identity.site.len() > 128
            || identity.account.is_empty()
            || identity.account.len() > 256
            || identity.model.is_empty()
            || identity.model.len() > 256
            || identity.actor.chars().any(char::is_control)
            || identity.site.chars().any(char::is_control)
            || identity.account.chars().any(char::is_control)
            || identity.model.chars().any(char::is_control)
            || options.deadline_ticks == 0
        {
            return Err(Error::InvalidConfiguration);
        }
        Ok(Self {
            identity,
            options,
            machine: TurnMachine::new(
                turn_id,
                Limits::new(options.max_rounds, options.max_calls)?,
            )?,
            status: Status::Created,
            messages: Vec::new(),
            calls: Vec::new(),
            next_call: 0,
            answer: String::new(),
            started_at: None,
            last_clock: 0,
            nonce: 0,
            trace: Vec::new(),
        })
    }
    pub fn status(&self) -> Status {
        self.status
    }
    pub fn phase(&self) -> Phase {
        self.machine.phase()
    }
    pub fn trace(&self) -> &[Event] {
        &self.trace
    }
    pub fn answer(&self) -> &str {
        &self.answer
    }
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }
    pub fn nonce(&self) -> Option<u64> {
        self.started_at.map(|_| self.nonce)
    }
    pub fn needs_reconciliation(&self) -> bool {
        self.status == Status::Blocked && self.machine.needs_reconciliation_if_failed()
    }
    fn event(&mut self, event: Event) -> Result<(), Error> {
        if self.trace.len() >= MAX_TRACE {
            return Err(Error::TooLarge);
        }
        self.trace.try_reserve(1).map_err(|_| Error::TooLarge)?;
        self.trace.push(event);
        Ok(())
    }
    fn clock(&mut self, host: &mut impl HostPorts) -> Result<u64, Error> {
        let now = host.monotonic_ticks().ok_or(Error::UnavailableClock)?;
        if self.started_at.is_some() {
            if now < self.last_clock {
                return Err(Error::RegressedClock);
            }
            if now.saturating_sub(self.started_at.unwrap_or(now)) > self.options.deadline_ticks {
                return Err(Error::Deadline);
            }
        }
        self.last_clock = now;
        Ok(now)
    }
    fn reject(&mut self, error: Error) -> Error {
        self.machine.fail();
        self.status = Status::Blocked;
        error
    }
    fn map_append(result: Result<(), PortError>) -> Result<(), Error> {
        result.map_err(|_| Error::JournalUnavailable)
    }
    fn authorized(&self, host: &impl HostPorts, name: &str) -> Result<Effect, Error> {
        let offer = host.registration(name).ok_or(Error::Denied)?;
        if offer.capability.name != name
            || !offer
                .grant
                .permits(self.identity.actor, self.identity.site, &offer.capability)
        {
            return Err(Error::Denied);
        }
        Ok(offer.capability.effect)
    }
    fn fits(&self, additional: usize) -> bool {
        let current = self.messages.iter().map(|m| m.content.len()).sum::<usize>();
        current.saturating_add(additional) <= MAX_CONTEXT_BYTES
    }
    /// Explicit start. Construction never reads entropy, a clock or any data.
    pub fn start(&mut self, host: &mut impl HostPorts, prompt: &str) -> Result<(), Error> {
        if self.status != Status::Created {
            return Err(Error::NotStarted);
        }
        let outcome = (|| {
            if prompt.trim().is_empty() || prompt.len() > MAX_PROMPT_BYTES {
                return Err(Error::TooLarge);
            }
            let now = self.clock(host)?;
            let mut bytes = [0u8; 8];
            if !host.secure_entropy(&mut bytes) {
                return Err(Error::UnavailableEntropy);
            }
            let nonce = u64::from_le_bytes(bytes);
            if nonce == 0 {
                return Err(Error::UnavailableEntropy);
            }
            Self::map_append(host.append(JournalRecord::TurnStarted {
                turn_id: self.machine.turn_id(),
                nonce,
                actor: self.identity.actor,
                account: self.identity.account,
                model: self.identity.model,
                prompt,
            }))?;
            self.messages.push(Message {
                role: Role::User,
                content: prompt.to_string(),
                call_id: None,
                tool_name: None,
            });
            self.started_at = Some(now);
            self.nonce = nonce;
            self.status = Status::NeedsModel;
            Ok(())
        })();
        outcome.map_err(|error| self.reject(error))
    }
    /// Host-invoked paid model attempt, never launched by a background loop.
    pub fn model_step(&mut self, host: &mut impl HostPorts) -> Result<Progress, Error> {
        if self.status != Status::NeedsModel {
            return Err(Error::NotStarted);
        }
        let outcome = self.model_step_inner(host);
        outcome.map_err(|error| self.reject(error))
    }
    fn model_step_inner(&mut self, host: &mut impl HostPorts) -> Result<Progress, Error> {
        self.clock(host)?;
        let event = self.machine.begin_model()?;
        self.event(event)?;
        let round = self.machine.rounds();
        Self::map_append(host.append(JournalRecord::ModelStarted {
            turn_id: self.machine.turn_id(),
            ordinal: round,
        }))?;
        self.clock(host)?;
        let reply = host
            .model(&self.messages, self.identity)
            .map_err(|_| Error::ProviderUnavailable)?;
        self.clock(host)?;
        if reply.text.len() > MAX_OUTPUT_BYTES.saturating_sub(self.answer.len())
            || reply.calls.len() > aw_agent_kernel::turn::MAX_BATCH
            || !self.fits(reply.text.len())
        {
            return Err(Error::TooLarge);
        }
        let mut call_bytes = 0usize;
        for call in &reply.calls {
            let length = serde_json::to_vec(&call.arguments)
                .map_err(|_| Error::InvalidResponse)?
                .len();
            if length > MAX_ARGUMENT_BYTES || !call.arguments.is_object() {
                return Err(Error::TooLarge);
            }
            call_bytes = call_bytes.saturating_add(length);
        }
        if !self.fits(
            reply
                .text
                .len()
                .saturating_add(call_bytes)
                .saturating_add(reply.calls.len().saturating_mul(MAX_RESULT_BYTES + 128)),
        ) {
            return Err(Error::TooLarge);
        }
        if reply.calls.is_empty() {
            let complete = reply.finish == Finish::Final;
            let text_exists = !reply.text.is_empty() || !self.answer.is_empty();
            let event = self.machine.complete_text(complete, text_exists)?;
            Self::map_append(host.append(JournalRecord::ModelSettled {
                turn_id: self.machine.turn_id(),
                ordinal: round,
                finish: reply.finish,
            }))?;
            Self::map_append(host.append(JournalRecord::Assistant {
                turn_id: self.machine.turn_id(),
                text: &reply.text,
                tools: 0,
            }))?;
            let status = if complete && text_exists {
                Status::Completed
            } else {
                Status::Incomplete
            };
            let progress = if status == Status::Completed {
                Progress::Completed
            } else {
                Progress::Incomplete
            };
            Self::map_append(host.append(JournalRecord::TurnFinished {
                turn_id: self.machine.turn_id(),
                outcome: progress,
            }))?;
            self.answer.push_str(&reply.text);
            self.messages.push(Message {
                role: Role::Assistant,
                content: reply.text,
                call_id: None,
                tool_name: None,
            });
            self.status = status;
            self.event(event)?;
            return Ok(progress);
        }
        if reply.finish != Finish::Tools {
            return Err(Error::InvalidResponse);
        }
        let proposed = reply
            .calls
            .iter()
            .map(|call| ProposedCall {
                id: &call.id,
                name: &call.name,
            })
            .collect::<Vec<_>>();
        let event = self.machine.propose_batch(&proposed, true)?;
        // Validate current grants and complete schemas for every call before
        // admitting the batch. A source advertisement alone gives no access.
        for call in &reply.calls {
            self.authorized(host, &call.name)?;
            host.validate_tool(call).map_err(|_| Error::Denied)?;
        }
        Self::map_append(host.append(JournalRecord::ModelSettled {
            turn_id: self.machine.turn_id(),
            ordinal: round,
            finish: reply.finish,
        }))?;
        Self::map_append(host.append(JournalRecord::Assistant {
            turn_id: self.machine.turn_id(),
            text: &reply.text,
            tools: reply.calls.len(),
        }))?;
        self.event(event)?;
        let admitted = self.machine.admit_batch()?;
        self.event(admitted)?;
        self.answer.push_str(&reply.text);
        self.messages.push(Message {
            role: Role::Assistant,
            content: reply.text,
            call_id: None,
            tool_name: None,
        });
        self.calls = reply.calls;
        self.next_call = 0;
        self.status = Status::NeedsTool;
        Ok(Progress::NeedsTool)
    }
    /// Executes exactly ONE host-approved effect after a durable journal
    /// intent, then waits for a durable receipt before next tool/model work.
    pub fn tool_step(&mut self, host: &mut impl HostPorts) -> Result<Progress, Error> {
        if self.status != Status::NeedsTool {
            return Err(Error::NotStarted);
        }
        let outcome = self.tool_step_inner(host);
        outcome.map_err(|error| self.reject(error))
    }
    fn tool_step_inner(&mut self, host: &mut impl HostPorts) -> Result<Progress, Error> {
        self.clock(host)?;
        let call = self
            .calls
            .get(self.next_call)
            .cloned()
            .ok_or(Error::InvalidResponse)?;
        let effect = self.authorized(host, &call.name)?;
        host.validate_tool(&call).map_err(|_| Error::Denied)?;
        let event = self.machine.prepare_tool(&call.id, effect)?;
        self.event(event)?;
        Self::map_append(host.append(JournalRecord::ToolIntent {
            turn_id: self.machine.turn_id(),
            call: &call,
            effect,
        }))?;
        let event = self.machine.intent_recorded()?;
        self.event(event)?;
        self.clock(host)?;
        // A permission may be revoked while journal append/flush ran.
        self.authorized(host, &call.name)?;
        host.validate_tool(&call).map_err(|_| Error::Denied)?;
        let event = self.machine.start_effect(true)?;
        self.event(event)?;
        let output = host
            .execute_tool(&call)
            .map_err(|_| Error::ToolUnavailable)?;
        self.clock(host)?;
        if output.content.len() > MAX_RESULT_BYTES
            || !self.fits(output.content.len().saturating_add(128))
        {
            return Err(Error::TooLarge);
        }
        let event = self.machine.settle_effect()?;
        self.event(event)?;
        Self::map_append(host.append(JournalRecord::ToolReceipt {
            turn_id: self.machine.turn_id(),
            call: &call,
            output: &output,
        }))?;
        let event = self.machine.receipt_recorded()?;
        self.event(event)?;
        self.messages.push(Message {
            role: Role::Tool,
            content: if output.is_error {
                let mut value = String::from("ERROR: ");
                value.push_str(&output.content);
                value
            } else {
                output.content
            },
            call_id: Some(call.id.clone()),
            tool_name: Some(call.name.clone()),
        });
        self.next_call += 1;
        if self.next_call == self.calls.len() {
            self.calls.clear();
            self.next_call = 0;
            self.status = Status::NeedsModel;
            Ok(Progress::NeedsModel)
        } else {
            Ok(Progress::NeedsTool)
        }
    }
    /// Explicit locally requested cancellation: no implicit replay. The host
    /// must inspect any earlier unknown/write/process intent before resuming.
    pub fn cancel(&mut self) {
        self.reject(Error::Failed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use aw_agent_kernel::capability::{Source, SourceKind};
    struct Fake {
        clock: u64,
        entropy: bool,
        journal: bool,
        fail_intent: bool,
        fail_receipt: bool,
        revoke_on_intent: bool,
        clock_available: bool,
        entropy_zero: bool,
        tool_calls: u32,
        model_calls: u32,
        ticks: u32,
        entries: Vec<&'static str>,
        grant_active: bool,
    }
    impl Fake {
        fn new() -> Self {
            Self {
                clock: 10,
                entropy: true,
                journal: true,
                fail_intent: false,
                fail_receipt: false,
                revoke_on_intent: false,
                clock_available: true,
                entropy_zero: false,
                tool_calls: 0,
                model_calls: 0,
                ticks: 0,
                entries: Vec::new(),
                grant_active: true,
            }
        }
    }
    impl HostPorts for Fake {
        fn monotonic_ticks(&mut self) -> Option<u64> {
            if !self.clock_available {
                return None;
            }
            self.ticks += 1;
            self.clock += 1;
            Some(self.clock)
        }
        fn secure_entropy(&mut self, output: &mut [u8]) -> bool {
            if !self.entropy {
                return false;
            }
            output.copy_from_slice(if self.entropy_zero {
                &[0, 0, 0, 0, 0, 0, 0, 0]
            } else {
                &[1, 2, 3, 4, 5, 6, 7, 8]
            });
            true
        }
        fn append(&mut self, record: JournalRecord<'_>) -> Result<(), PortError> {
            if !self.journal {
                return Err(PortError::Unavailable);
            }
            let kind = match record {
                JournalRecord::TurnStarted { .. } => "turn",
                JournalRecord::ModelStarted { .. } => "model_intent",
                JournalRecord::ModelSettled { .. } => "model_result",
                JournalRecord::Assistant { .. } => "assistant",
                JournalRecord::ToolIntent { .. } => "tool_intent",
                JournalRecord::ToolReceipt { .. } => "tool_receipt",
                JournalRecord::TurnFinished { .. } => "finished",
            };
            self.entries.push(kind);
            if kind == "tool_intent" && self.revoke_on_intent {
                self.grant_active = false;
            }
            if kind == "tool_intent" && self.fail_intent {
                return Err(PortError::Failed);
            }
            if kind == "tool_receipt" && self.fail_receipt {
                return Err(PortError::Failed);
            }
            Ok(())
        }
        fn model(
            &mut self,
            messages: &[Message],
            _identity: Identity<'_>,
        ) -> Result<ModelReply, PortError> {
            self.model_calls += 1;
            if messages.iter().any(|msg| msg.role == Role::Tool) {
                Ok(ModelReply {
                    text: "done".into(),
                    finish: Finish::Final,
                    calls: Vec::new(),
                })
            } else {
                Ok(ModelReply {
                    text: String::new(),
                    finish: Finish::Tools,
                    calls: vec![ToolCall {
                        id: "c1".into(),
                        name: "echo".into(),
                        arguments: serde_json::json!({"text":"hello"}),
                    }],
                })
            }
        }
        fn registration(&self, name: &str) -> Option<ToolRegistration<'_>> {
            if !self.grant_active || name != "echo" {
                return None;
            }
            let cap = Capability::new(
                "echo",
                Source::new(SourceKind::Extension, "box", 7).ok()?,
                Effect::Unknown,
            )
            .ok()?;
            Some(ToolRegistration {
                capability: cap,
                grant: Grant::new("user", "device", cap).ok()?,
            })
        }
        fn validate_tool(&self, call: &ToolCall) -> Result<(), PortError> {
            if call.arguments["text"] == "hello" {
                Ok(())
            } else {
                Err(PortError::Denied)
            }
        }
        fn execute_tool(&mut self, _call: &ToolCall) -> Result<ToolResult, PortError> {
            self.tool_calls += 1;
            Ok(ToolResult {
                content: "echo:hello".into(),
                is_error: false,
            })
        }
    }
    fn identity() -> Identity<'static> {
        Identity {
            actor: "user",
            site: "device",
            account: "acct",
            model: "mock",
        }
    }
    fn run_turn(host: &mut Fake) -> EmbeddedTurn<'static> {
        let mut engine = EmbeddedTurn::new(1, identity(), Options::default()).unwrap();
        engine.start(host, "hello").unwrap();
        assert_eq!(engine.model_step(host), Ok(Progress::NeedsTool));
        assert_eq!(engine.tool_step(host), Ok(Progress::NeedsModel));
        assert_eq!(engine.model_step(host), Ok(Progress::Completed));
        engine
    }
    #[test]
    fn real_host_driven_tool_turn() {
        let mut host = Fake::new();
        let mut engine = EmbeddedTurn::new(1, identity(), Options::default()).unwrap();
        assert_eq!(host.model_calls, 0);
        assert_eq!(host.entries.len(), 0);
        engine.start(&mut host, "hello").unwrap();
        assert_eq!(engine.model_step(&mut host), Ok(Progress::NeedsTool));
        assert_eq!(engine.phase(), Phase::ReadyTool);
        assert_eq!(engine.tool_step(&mut host), Ok(Progress::NeedsModel));
        assert_eq!(engine.model_step(&mut host), Ok(Progress::Completed));
        assert_eq!(engine.answer(), "done");
        assert_eq!(host.tool_calls, 1);
        assert_eq!(host.model_calls, 2);
        assert_eq!(
            host.entries,
            vec![
                "turn",
                "model_intent",
                "model_result",
                "assistant",
                "tool_intent",
                "tool_receipt",
                "model_intent",
                "model_result",
                "assistant",
                "finished"
            ]
        );
        assert_eq!(
            engine.trace(),
            &[
                Event::ModelStarted(1),
                Event::BatchProposed(1),
                Event::BatchAdmitted(1),
                Event::ToolIntent(0),
                Event::IntentRecorded(0),
                Event::EffectStarted(0),
                Event::EffectSettled(0),
                Event::ReceiptRecorded(0),
                Event::ModelStarted(2),
                Event::TextCompleted,
            ]
        );
    }
    #[test]
    fn missing_capabilities_and_denied_grants_never_execute() {
        let mut host = Fake::new();
        host.entropy = false;
        let mut engine = EmbeddedTurn::new(1, identity(), Options::default()).unwrap();
        assert_eq!(
            engine.start(&mut host, "hello"),
            Err(Error::UnavailableEntropy)
        );
        assert_eq!(host.entries.len(), 0);
        assert_eq!(host.tool_calls, 0);
        let mut host = Fake::new();
        host.journal = false;
        let mut engine = EmbeddedTurn::new(1, identity(), Options::default()).unwrap();
        assert_eq!(
            engine.start(&mut host, "hello"),
            Err(Error::JournalUnavailable)
        );
        assert_eq!(host.model_calls, 0);
        let mut host = Fake::new();
        host.grant_active = false;
        let mut engine = EmbeddedTurn::new(1, identity(), Options::default()).unwrap();
        engine.start(&mut host, "hello").unwrap();
        assert_eq!(engine.model_step(&mut host), Err(Error::Denied));
        assert_eq!(host.tool_calls, 0);
    }
    #[test]
    fn effect_intent_and_receipt_failures_block_and_cannot_replay() {
        let mut host = Fake::new();
        host.fail_intent = true;
        let mut engine = EmbeddedTurn::new(1, identity(), Options::default()).unwrap();
        engine.start(&mut host, "hello").unwrap();
        engine.model_step(&mut host).unwrap();
        assert_eq!(engine.tool_step(&mut host), Err(Error::JournalUnavailable));
        assert!(engine.needs_reconciliation());
        assert_eq!(engine.tool_step(&mut host), Err(Error::NotStarted));
        assert_eq!(host.tool_calls, 0);

        let mut host = Fake::new();
        host.fail_receipt = true;
        let mut engine = EmbeddedTurn::new(1, identity(), Options::default()).unwrap();
        engine.start(&mut host, "hello").unwrap();
        engine.model_step(&mut host).unwrap();
        assert_eq!(engine.tool_step(&mut host), Err(Error::JournalUnavailable));
        assert!(engine.needs_reconciliation());
        assert_eq!(host.tool_calls, 1);
        assert_eq!(engine.model_step(&mut host), Err(Error::NotStarted));
        assert_eq!(host.tool_calls, 1);
    }
    #[test]
    fn clock_failure_and_explicit_cancellation_are_terminal() {
        let mut host = Fake::new();
        let mut engine = EmbeddedTurn::new(
            1,
            identity(),
            Options {
                deadline_ticks: 1,
                ..Options::default()
            },
        )
        .unwrap();
        engine.start(&mut host, "hello").unwrap();
        assert_eq!(engine.model_step(&mut host), Err(Error::Deadline));
        assert_eq!(host.model_calls, 0);
        let mut host = Fake::new();
        let mut engine = EmbeddedTurn::new(1, identity(), Options::default()).unwrap();
        engine.start(&mut host, "hello").unwrap();
        engine.model_step(&mut host).unwrap();
        engine.cancel();
        assert!(
            !engine.needs_reconciliation(),
            "tool was admitted, no intent written yet"
        );
        assert_eq!(engine.tool_step(&mut host), Err(Error::NotStarted));
    }
    #[test]
    fn revocation_during_intent_flush_blocks_first_effect_without_replay() {
        let mut host = Fake::new();
        host.revoke_on_intent = true;
        let mut engine = EmbeddedTurn::new(1, identity(), Options::default()).unwrap();
        engine.start(&mut host, "hello").unwrap();
        assert_eq!(engine.model_step(&mut host), Ok(Progress::NeedsTool));
        assert_eq!(engine.tool_step(&mut host), Err(Error::Denied));
        assert_eq!(host.tool_calls, 0);
        assert_eq!(
            host.entries
                .iter()
                .filter(|row| **row == "tool_intent")
                .count(),
            1
        );
        assert!(!host.entries.contains(&"tool_receipt"));
        assert!(
            engine.needs_reconciliation(),
            "unknown intent must be inspected even when grant revoked"
        );
        assert_eq!(engine.tool_step(&mut host), Err(Error::NotStarted));
    }
    #[test]
    fn clock_unavailability_regression_and_zero_entropy_fail_closed() {
        let mut host = Fake::new();
        host.clock_available = false;
        let mut engine = EmbeddedTurn::new(1, identity(), Options::default()).unwrap();
        assert_eq!(
            engine.start(&mut host, "hello"),
            Err(Error::UnavailableClock)
        );
        assert!(host.entries.is_empty());

        let mut host = Fake::new();
        host.entropy_zero = true;
        let mut engine = EmbeddedTurn::new(1, identity(), Options::default()).unwrap();
        assert_eq!(
            engine.start(&mut host, "hello"),
            Err(Error::UnavailableEntropy)
        );
        assert!(host.entries.is_empty());

        let mut host = Fake::new();
        let mut engine = EmbeddedTurn::new(1, identity(), Options::default()).unwrap();
        engine.start(&mut host, "hello").unwrap();
        host.clock = 0;
        assert_eq!(engine.model_step(&mut host), Err(Error::RegressedClock));
        assert_eq!(host.model_calls, 0);
    }
    #[test]
    fn separately_driven_full_turn_has_no_implicit_extra_steps() {
        let mut host = Fake::new();
        let engine = run_turn(&mut host);
        assert_eq!(engine.status(), Status::Completed);
        assert_eq!(host.tool_calls, 1);
        assert!(!engine.needs_reconciliation());
    }
}
