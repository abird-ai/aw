//! Actual std Agent vs independent no_std+alloc embedded host execution.
//! Both must emit the same data-free turn transition trace from the single
//! aw-agent-kernel machine, not two independently implemented schedulers.
use aw_agent_core::{
    journal::{JournalEntry, JournalFuture, TurnJournal, TurnStart},
    tool::{ToolContext, ToolSource},
    Agent, AgentOptions, CompletionFuture, CompletionMeta, DeltaSink, Error, HistoryState, Message,
    ModelFuture, ModelReply, Outcome, Provider, Result, Role, Tool, ToolCall, ToolEffect,
    ToolFuture, ToolOutput, ToolSpec, Usage,
};
use aw_agent_kernel::{capability::SourceKind, turn::Event};
use serde_json::{json, Value};
use std::{
    future::pending,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

#[derive(Default)]
struct Proof {
    journal: Mutex<Vec<&'static str>>,
    effects: AtomicUsize,
    provider_calls: AtomicUsize,
}
impl Proof {
    fn events(&self) -> Vec<&'static str> {
        self.journal.lock().unwrap().clone()
    }
}
struct ProviderFixture(Arc<Proof>);
impl Provider for ProviderFixture {
    fn supports_tools(&self) -> bool {
        true
    }
    fn stream<'a>(
        &'a self,
        _: &'a [Message],
        _: &'a str,
        _: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async { Err(Error::Protocol("unexpected text-only stream")) })
    }
    fn stream_tools<'a>(
        &'a self,
        messages: &'a [Message],
        _: &'a str,
        tools: &'a [ToolSpec],
        sink: &'a mut DeltaSink<'_>,
    ) -> ModelFuture<'a> {
        Box::pin(async move {
            self.0.provider_calls.fetch_add(1, Ordering::SeqCst);
            if messages.iter().any(|msg| msg.role == Role::Tool) {
                if messages.last().is_none_or(|msg| msg.content != "ok:device") {
                    return Err(Error::Protocol("wrong tool continuation context"));
                }
                sink("turn finished")?;
                return Ok(ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "stop".into(),
                        usage: Usage::default(),
                    },
                    ..Default::default()
                });
            }
            if tools.len() != 1
                || tools[0].name != "device_echo"
                || tools[0].effect != ToolEffect::Unknown
            {
                return Err(Error::Protocol("wrong current external source grant"));
            }
            Ok(ModelReply {
                meta: CompletionMeta {
                    finish_reason: "tool_calls".into(),
                    usage: Usage::default(),
                },
                tool_calls: vec![ToolCall {
                    id: "call_one".into(),
                    name: "device_echo".into(),
                    arguments: json!({"payload":"device"}),
                }],
                ..Default::default()
            })
        })
    }
}
struct Echo(Arc<Proof>);
impl Tool for Echo {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "device_echo".into(),
            description: "Bounded independently scoped echo".into(),
            parameters: json!({"type":"object","properties":{"payload":{"type":"string"}},"required":["payload"],"additionalProperties":false}),
            effect: ToolEffect::Unknown,
        }
    }
    fn validate(&self, arguments: &Value) -> Result<()> {
        if arguments["payload"] == "device" {
            Ok(())
        } else {
            Err(Error::Denied("unexpected echo payload"))
        }
    }
    fn execute<'a>(&'a self, _: ToolContext, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(arguments)?;
            let events = self.0.events();
            if !events.contains(&"tool_intent") {
                return Err(Error::Protocol("tool executed before journal intent"));
            }
            self.0.effects.fetch_add(1, Ordering::SeqCst);
            Ok(ToolOutput::text("ok:device"))
        })
    }
}
struct JournalFixture {
    proof: Arc<Proof>,
    fail_receipt: bool,
}
impl TurnJournal for JournalFixture {
    fn begin<'a>(&'a self, _: TurnStart) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            self.proof.journal.lock().unwrap().push("turn");
            Ok(())
        })
    }
    fn append<'a>(&'a self, _turn: u64, entry: JournalEntry) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            let kind = match entry {
                JournalEntry::InvocationStarted { .. } => "model_intent",
                JournalEntry::InvocationFinished { .. } => "model_result",
                JournalEntry::Assistant { .. } => "assistant",
                JournalEntry::ToolStarted { .. } => "tool_intent",
                JournalEntry::ToolFinished { .. } => "tool_receipt",
                JournalEntry::ToolDenied { .. } => "tool_denied",
                JournalEntry::ToolRevised { .. } => "tool_revised",
            };
            self.proof.journal.lock().unwrap().push(kind);
            if kind == "tool_receipt" && self.fail_receipt {
                return Err(Error::Storage("simulated durable receipt failure"));
            }
            Ok(())
        })
    }
    fn finish<'a>(&'a self, _: aw_agent_core::Turn) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            self.proof.journal.lock().unwrap().push("finished");
            Ok(())
        })
    }
    fn acknowledge<'a>(&'a self, _: String) -> JournalFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}
fn agent(proof: Arc<Proof>, fail_receipt: bool) -> Agent {
    let provider = Arc::new(ProviderFixture(proof.clone()));
    let mut agent = Agent::builder(provider, AgentOptions::new("model-account", "fixture"))
        .build()
        .unwrap();
    let origin = ToolSource::new(SourceKind::Extension, "device-mcp", 42).unwrap();
    agent
        .register_source_tool(Arc::new(Echo(proof.clone())), origin.clone())
        .unwrap();
    agent
        .permit_registered_source_tool(agent.actor().clone(), "device_echo", &origin)
        .unwrap();
    agent
        .load_history(
            HistoryState::default(),
            Some(Arc::new(JournalFixture {
                proof,
                fail_receipt,
            })),
        )
        .unwrap();
    agent
}
#[tokio::test]
async fn hosted_and_no_std_embedded_emit_identical_real_turn_transitions() {
    let proof = Arc::new(Proof::default());
    let mut hosted = agent(proof.clone(), false);
    assert!(hosted.portable_trace().is_empty());
    let result = hosted
        .prompt("Please echo device", pending(), |_| Ok(()))
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::Completed);
    assert_eq!(result.answer, "turn finished");
    assert_eq!(proof.effects.load(Ordering::SeqCst), 1);
    assert_eq!(proof.provider_calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        proof.events(),
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
    let embedded_events = aw_embedded_example::actual_portable_trace()
        .expect("real no_std embedder must drive same kernel");
    assert_eq!(hosted.portable_trace(), embedded_events.as_slice());
    assert_eq!(
        hosted.portable_trace(),
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
#[tokio::test]
async fn failed_durable_receipt_prevents_replay_in_hosted_shared_kernel() {
    let proof = Arc::new(Proof::default());
    let mut hosted = agent(proof.clone(), true);
    let first = hosted
        .prompt("Please echo device", pending(), |_| Ok(()))
        .await
        .unwrap();
    assert_eq!(first.outcome, Outcome::Failed);
    assert!(first.needs_reconciliation);
    assert!(hosted.needs_reconciliation());
    assert!(hosted.portable_trace().contains(&Event::Blocked));
    assert_eq!(proof.effects.load(Ordering::SeqCst), 1);
    let before = proof.provider_calls.load(Ordering::SeqCst);
    assert!(hosted
        .prompt("retry forbidden", pending(), |_| Ok(()))
        .await
        .is_err());
    assert_eq!(proof.effects.load(Ordering::SeqCst), 1);
    assert_eq!(proof.provider_calls.load(Ordering::SeqCst), before);
}
