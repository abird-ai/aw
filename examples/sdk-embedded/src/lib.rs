//! Real independent no_std + alloc user of the same hosted TurnMachine.
//! This example supplies deliberate deterministic clock, RNG, journal, model
//! and tool callbacks, then drives one complete tool-bearing Agent request.
//! No libc/std/OS/Tokio/SQLite/network dependency is linked into this crate.
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;

use alloc::{string::String, vec::Vec};
use aw_agent_kernel::capability::{Capability, Effect, Grant, Source, SourceKind};
use aw_agent_kernel::turn::Event;
use aw_embedded::{
    EmbeddedTurn, Finish, HostPorts, Identity, JournalRecord, Message, ModelReply, Options,
    PortError, Progress, Role, ToolCall, ToolRegistration, ToolResult,
};
use serde_json::json;

struct Device {
    tick: u64,
    journal_events: Vec<u8>,
    model_calls: u32,
    tool_calls: u32,
}
impl Device {
    fn new() -> Self {
        Self {
            tick: 100,
            journal_events: Vec::new(),
            model_calls: 0,
            tool_calls: 0,
        }
    }
}
impl HostPorts for Device {
    fn monotonic_ticks(&mut self) -> Option<u64> {
        self.tick += 1;
        Some(self.tick)
    }
    fn secure_entropy(&mut self, output: &mut [u8]) -> bool {
        if output.len() != 8 {
            return false;
        }
        output.copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        true
    }
    fn append(&mut self, record: JournalRecord<'_>) -> Result<(), PortError> {
        // A real host must commit/flush this record before returning Ok.
        // This fake device is an explicit deterministic in-memory test port.
        let code = match record {
            JournalRecord::TurnStarted { .. } => 1,
            JournalRecord::ModelStarted { .. } => 2,
            JournalRecord::ModelSettled { .. } => 3,
            JournalRecord::Assistant { .. } => 4,
            JournalRecord::ToolIntent { .. } => 5,
            JournalRecord::ToolReceipt { .. } => 6,
            JournalRecord::TurnFinished { .. } => 7,
        };
        self.journal_events.push(code);
        Ok(())
    }
    fn model(
        &mut self,
        messages: &[Message],
        identity: Identity<'_>,
    ) -> Result<ModelReply, PortError> {
        if identity.actor != "device-user"
            || identity.account != "model-account"
            || identity.model != "fixture"
        {
            return Err(PortError::Denied);
        }
        self.model_calls += 1;
        if messages.iter().any(|msg| msg.role == Role::Tool) {
            if messages.last().is_none_or(|msg| msg.content != "ok:device") {
                return Err(PortError::Failed);
            }
            Ok(ModelReply {
                text: String::from("turn finished"),
                finish: Finish::Final,
                calls: Vec::new(),
            })
        } else {
            Ok(ModelReply {
                text: String::new(),
                finish: Finish::Tools,
                calls: alloc::vec![ToolCall {
                    id: String::from("call_one"),
                    name: String::from("device_echo"),
                    arguments: json!({"payload":"device"}),
                }],
            })
        }
    }
    fn registration(&self, name: &str) -> Option<ToolRegistration<'_>> {
        if name != "device_echo" {
            return None;
        }
        let source = Source::new(SourceKind::Extension, "device-mcp", 42).ok()?;
        let cap = Capability::new("device_echo", source, Effect::Unknown).ok()?;
        let grant = Grant::new("device-user", "rom:local", cap).ok()?;
        Some(ToolRegistration {
            capability: cap,
            grant,
        })
    }
    fn validate_tool(&self, call: &ToolCall) -> Result<(), PortError> {
        if call.arguments["payload"] == "device" {
            Ok(())
        } else {
            Err(PortError::Denied)
        }
    }
    fn execute_tool(&mut self, call: &ToolCall) -> Result<ToolResult, PortError> {
        if call.name != "device_echo" {
            return Err(PortError::Denied);
        }
        if !self.journal_events.contains(&5) {
            return Err(PortError::Failed);
        }
        self.tool_calls += 1;
        Ok(ToolResult {
            content: String::from("ok:device"),
            is_error: false,
        })
    }
}
/// Return a stable nonzero word only if the *actual* embedded Engine executed
/// a real mock provider/tool/journal turn through the shared kernel.
pub fn run_demo() -> u32 {
    let mut host = Device::new();
    let identity = Identity {
        actor: "device-user",
        site: "rom:local",
        account: "model-account",
        model: "fixture",
    };
    let mut turn = match EmbeddedTurn::new(1, identity, Options::default()) {
        Ok(engine) => engine,
        Err(_) => return 0,
    };
    if host.model_calls != 0 || host.tool_calls != 0 || !host.journal_events.is_empty() {
        return 0;
    }
    if turn.start(&mut host, "Please echo device").is_err() {
        return 0;
    }
    if turn.model_step(&mut host) != Ok(Progress::NeedsTool) {
        return 0;
    }
    if turn.tool_step(&mut host) != Ok(Progress::NeedsModel) {
        return 0;
    }
    if turn.model_step(&mut host) != Ok(Progress::Completed) {
        return 0;
    }
    if turn.answer() != "turn finished" || host.model_calls != 2 || host.tool_calls != 1 {
        return 0;
    }
    if host.journal_events != [1, 2, 3, 4, 5, 6, 2, 3, 4, 7] {
        return 0;
    }
    if turn.trace()
        != [
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
    {
        return 0;
    }
    0xA80E0001
}
/// Independently execute an E3 mock device turn and return the actual shared
/// kernel event stream for direct comparison to a standard Agent invocation.
/// This performs real host model/tool/journal callbacks, not a static fixture.
pub fn actual_portable_trace() -> Option<Vec<Event>> {
    let mut host = Device::new();
    let identity = Identity {
        actor: "device-user",
        site: "rom:local",
        account: "model-account",
        model: "fixture",
    };
    let mut turn = EmbeddedTurn::new(1, identity, Options::default()).ok()?;
    turn.start(&mut host, "Please echo device").ok()?;
    if turn.model_step(&mut host).ok()? != Progress::NeedsTool {
        return None;
    }
    if turn.tool_step(&mut host).ok()? != Progress::NeedsModel {
        return None;
    }
    if turn.model_step(&mut host).ok()? != Progress::Completed {
        return None;
    }
    if host.tool_calls != 1 || host.model_calls != 2 {
        return None;
    }
    Some(turn.trace().to_vec())
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_real_host_driven_turn_matches_expected_trace() {
        assert_eq!(run_demo(), 0xA80E0001);
    }
}
