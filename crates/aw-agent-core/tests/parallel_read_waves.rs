//! R9 actual hosted parallel-read wave tests. Never infer concurrency from
//! source inspection: barriers prove simultaneous dispatch, bounded count,
//! deterministic journal/continuation order and absence of detached effects.
use aw_agent_core::{
    journal::{JournalEntry, JournalFuture, TurnJournal, TurnStart},
    *,
};
use aw_agent_kernel::turn::Event;
use serde_json::{json, Value};
use std::{
    future::{pending, Future},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::Barrier;

#[derive(Default)]
struct Proof {
    log: Mutex<Vec<String>>,
    active: AtomicUsize,
    max_active: AtomicUsize,
    effects: AtomicUsize,
    writes: AtomicUsize,
    finished: AtomicUsize,
    denied: AtomicBool,
    model_parallel: AtomicBool,
    provider_calls: AtomicUsize,
}
impl Proof {
    fn emit(&self, value: impl Into<String>) {
        self.log.lock().unwrap().push(value.into());
    }
    fn entries(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
    fn entered_read(&self) {
        let count = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(count, Ordering::SeqCst);
    }
}
struct ActiveRead(Arc<Proof>);
impl Drop for ActiveRead {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Clone)]
enum Behavior {
    Immediate,
    Barrier(Arc<Barrier>, Duration),
    Hang,
    Fail,
    ErrorOutput,
    BigOutput(usize),
    RevokeDuringRead(Arc<Barrier>),
}
#[derive(Clone)]
struct FixtureTool {
    name: String,
    effect: ToolEffect,
    behavior: Behavior,
    proof: Arc<Proof>,
}
impl Tool for FixtureTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name.clone(),
            description: "R9 bounded wave test tool".into(),
            parameters: json!({"type":"object","properties":{"parallel":{"type":"boolean"}},"additionalProperties":false}),
            effect: self.effect.clone(),
        }
    }
    fn is_available(&self) -> bool {
        !self.proof.denied.load(Ordering::SeqCst)
    }
    fn validate(&self, arguments: &Value) -> Result<()> {
        if arguments.as_object().is_some_and(|map| {
            map.is_empty() || (map.len() == 1 && map.get("parallel").is_some_and(Value::is_boolean))
        }) {
            Ok(())
        } else {
            Err(Error::Denied("unexpected arguments"))
        }
    }
    fn execute<'a>(&'a self, _ctx: ToolContext, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(arguments)?;
            let read = self.effect == ToolEffect::Read;
            let can_overlap = read || self.name == "bash";
            if can_overlap {
                self.proof.entered_read();
            } else {
                assert_eq!(
                    self.proof.active.load(Ordering::SeqCst),
                    0,
                    "write/process/unknown executed while a parallel wave was active"
                );
            }
            if !read {
                self.proof.writes.fetch_add(1, Ordering::SeqCst);
            }
            let _guard = can_overlap.then(|| ActiveRead(self.proof.clone()));
            self.proof.effects.fetch_add(1, Ordering::SeqCst);
            self.proof.emit(format!("effect:{}", self.name));
            let result = match &self.behavior {
                Behavior::Immediate => Ok(ToolOutput::text(format!("out:{}", self.name))),
                Behavior::Barrier(barrier, delay) => {
                    barrier.wait().await;
                    tokio::time::sleep(*delay).await;
                    Ok(ToolOutput::text(format!("out:{}", self.name)))
                }
                Behavior::Hang => pending::<Result<ToolOutput>>().await,
                Behavior::Fail => Err(Error::Transport("R9 test-only tool crash")),
                Behavior::ErrorOutput => Ok(ToolOutput::error("fixture-error")),
                Behavior::BigOutput(bytes) => Ok(ToolOutput::text("x".repeat(*bytes))),
                Behavior::RevokeDuringRead(barrier) => {
                    barrier.wait().await;
                    self.proof.denied.store(true, Ordering::SeqCst);
                    Ok(ToolOutput::text("revoked-content-must-not-leak"))
                }
            };
            if result.is_ok() {
                self.proof.finished.fetch_add(1, Ordering::SeqCst);
                self.proof.emit(format!("done:{}", self.name));
            }
            result
        })
    }
}

struct Script {
    names: Vec<(String, ToolEffect)>,
    requests: Arc<Mutex<Vec<Vec<Message>>>>,
    proof: Arc<Proof>,
}
impl Provider for Script {
    fn supports_tools(&self) -> bool {
        true
    }
    fn stream<'a>(
        &'a self,
        _: &'a [Message],
        _: &'a str,
        _: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async { Err(Error::Protocol("unexpected text-only provider")) })
    }
    fn stream_tools<'a>(
        &'a self,
        messages: &'a [Message],
        _: &'a str,
        _: &'a [ToolSpec],
        sink: &'a mut DeltaSink<'_>,
    ) -> ModelFuture<'a> {
        Box::pin(async move {
            self.proof.provider_calls.fetch_add(1, Ordering::SeqCst);
            self.requests.lock().unwrap().push(messages.to_vec());
            if messages.iter().any(|message| message.role == Role::Tool) {
                sink("complete")?;
                Ok(ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "stop".into(),
                        usage: Usage::default(),
                    },
                    ..Default::default()
                })
            } else {
                Ok(ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "tool_calls".into(),
                        usage: Usage::default(),
                    },
                    tool_calls: self
                        .names
                        .iter()
                        .enumerate()
                        .map(|(i, (name, effect))| ToolCall {
                            id: format!("call_{i}"),
                            name: name.clone(),
                            arguments: if (*effect == ToolEffect::Read || name == "bash")
                                && self.proof.model_parallel.load(Ordering::SeqCst)
                            {
                                json!({"parallel":true})
                            } else {
                                json!({})
                            },
                        })
                        .collect(),
                    ..Default::default()
                })
            }
        })
    }
}
struct Journal {
    proof: Arc<Proof>,
    fail_intent: Option<usize>,
    fail_receipt: Option<usize>,
    intents: AtomicUsize,
    receipts: AtomicUsize,
}
impl TurnJournal for Journal {
    fn begin<'a>(&'a self, _: TurnStart) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            self.proof.emit("turn.begin");
            Ok(())
        })
    }
    fn append<'a>(&'a self, _turn: u64, entry: JournalEntry) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            let name = match entry {
                JournalEntry::ToolStarted { record, source } => {
                    assert_eq!(
                        source.as_ref(),
                        Some(&ToolSourceEvidence::trusted_host()),
                        "original Agent must pin exact live Host source before wave effects"
                    );
                    let nth = self.intents.fetch_add(1, Ordering::SeqCst) + 1;
                    self.proof.emit(format!("intent:{}", record.call.name));
                    if self.fail_intent == Some(nth) {
                        return Err(Error::Storage("R9 test intent flush failure"));
                    }
                    return Ok(());
                }
                JournalEntry::ToolFinished { record } => {
                    let nth = self.receipts.fetch_add(1, Ordering::SeqCst) + 1;
                    self.proof
                        .emit(format!("receipt:{}:{:?}", record.call.name, record.status));
                    if self.fail_receipt == Some(nth) {
                        return Err(Error::Storage("R9 test receipt flush failure"));
                    }
                    return Ok(());
                }
                JournalEntry::Assistant { .. } => "assistant",
                JournalEntry::InvocationStarted { .. } => "model.start",
                JournalEntry::InvocationFinished { .. } => "model.finish",
                JournalEntry::ToolDenied { .. } => "tool.denied",
                JournalEntry::ToolRevised { .. } => "tool.revised",
            };
            self.proof.emit(name);
            Ok(())
        })
    }
    fn finish<'a>(&'a self, _: Turn) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            self.proof.emit("turn.finish");
            Ok(())
        })
    }
    fn acknowledge<'a>(&'a self, _: String) -> JournalFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}
struct Config {
    names: Vec<(&'static str, ToolEffect, Behavior)>,
    parallel: usize,
    timeout: Duration,
    fail_intent: Option<usize>,
    fail_receipt: Option<usize>,
}
type Requests = Arc<Mutex<Vec<Vec<Message>>>>;
fn setup(config: Config) -> (Agent, Arc<Proof>, Requests) {
    let proof = Arc::new(Proof::default());
    proof.model_parallel.store(true, Ordering::SeqCst);
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut policy = ToolPolicy {
        max_parallel_reads: config.parallel,
        timeout: config.timeout,
        ..ToolPolicy::default()
    };
    for (name, effect, _) in &config.names {
        policy
            .permit_host(PrincipalId::new("local").unwrap(), name, effect)
            .unwrap();
    }
    let mut builder = Agent::builder(
        Arc::new(Script {
            names: config
                .names
                .iter()
                .map(|(name, effect, _)| (name.to_string(), effect.clone()))
                .collect(),
            requests: requests.clone(),
            proof: proof.clone(),
        }),
        AgentOptions::new("p", "m"),
    )
    .tool_policy(policy);
    let mut registered = std::collections::BTreeSet::new();
    for (name, effect, behavior) in &config.names {
        if !registered.insert(*name) {
            continue;
        }
        builder = builder.tool(FixtureTool {
            name: (*name).into(),
            effect: effect.clone(),
            behavior: behavior.clone(),
            proof: proof.clone(),
        });
    }
    let mut agent = builder.build().unwrap();
    agent
        .load_history(
            HistoryState::default(),
            Some(Arc::new(Journal {
                proof: proof.clone(),
                fail_intent: config.fail_intent,
                fail_receipt: config.fail_receipt,
                intents: AtomicUsize::new(0),
                receipts: AtomicUsize::new(0),
            })),
        )
        .unwrap();
    (agent, proof, requests)
}
fn prompt<'a>(agent: &'a mut Agent) -> impl Future<Output = Result<Turn>> + 'a {
    agent.prompt("parallel", pending(), |_| Ok(()))
}
fn verify_context(requests: &Arc<Mutex<Vec<Vec<Message>>>>, expected: &[&str]) {
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let tool_results = requests[1]
        .iter()
        .filter(|m| m.role == Role::Tool)
        .collect::<Vec<_>>();
    assert_eq!(tool_results.len(), expected.len());
    for (index, msg) in tool_results.iter().enumerate() {
        assert_eq!(msg.name.as_deref(), Some(expected[index]));
        assert_eq!(
            msg.tool_call_id.as_deref(),
            Some(format!("call_{index}").as_str())
        );
        assert_eq!(msg.content, format!("out:{}", expected[index]));
    }
}
#[tokio::test]
async fn four_reads_run_concurrently_but_receipts_and_model_results_stay_ordered() {
    let barrier = Arc::new(Barrier::new(4));
    let names = (0..4)
        .map(|i| match i {
            0 => (
                "read0",
                ToolEffect::Read,
                Behavior::Barrier(barrier.clone(), Duration::from_millis(40)),
            ),
            1 => (
                "read1",
                ToolEffect::Read,
                Behavior::Barrier(barrier.clone(), Duration::from_millis(25)),
            ),
            2 => (
                "read2",
                ToolEffect::Read,
                Behavior::Barrier(barrier.clone(), Duration::from_millis(10)),
            ),
            _ => (
                "read3",
                ToolEffect::Read,
                Behavior::Barrier(barrier.clone(), Duration::from_millis(0)),
            ),
        })
        .collect::<Vec<_>>();
    let (mut agent, proof, requests) = setup(Config {
        names,
        parallel: 4,
        timeout: Duration::from_secs(1),
        fail_intent: None,
        fail_receipt: None,
    });
    let turn = tokio::time::timeout(Duration::from_secs(2), prompt(&mut agent))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(turn.outcome, Outcome::Completed);
    assert_eq!(proof.max_active.load(Ordering::SeqCst), 4);
    assert_eq!(proof.effects.load(Ordering::SeqCst), 4);
    let log = proof.entries();
    let intents = log
        .iter()
        .filter(|row| row.starts_with("intent:"))
        .cloned()
        .collect::<Vec<_>>();
    let receipts = log
        .iter()
        .filter(|row| row.starts_with("receipt:"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        intents,
        vec![
            "intent:read0",
            "intent:read1",
            "intent:read2",
            "intent:read3"
        ]
    );
    assert_eq!(
        receipts,
        vec![
            "receipt:read0:Succeeded",
            "receipt:read1:Succeeded",
            "receipt:read2:Succeeded",
            "receipt:read3:Succeeded"
        ]
    );
    let first_effect = log
        .iter()
        .position(|row| row.starts_with("effect:"))
        .unwrap();
    assert_eq!(
        log[..first_effect]
            .iter()
            .filter(|s| s.starts_with("intent:"))
            .count(),
        4
    );
    let done = log
        .iter()
        .filter(|s| s.starts_with("done:"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        done[0], "done:read3",
        "fixture actually completes in a different order"
    );
    verify_context(&requests, &["read0", "read1", "read2", "read3"]);
    assert_eq!(
        agent
            .portable_trace()
            .iter()
            .filter(|event| matches!(event, Event::ReadWaveStarted(_)))
            .count(),
        1
    );
    assert_eq!(
        agent
            .portable_trace()
            .iter()
            .filter(|event| matches!(event, Event::ReadWaveReceiptRecorded(_)))
            .count(),
        4
    );
}
#[tokio::test]
async fn mutations_and_unknown_effects_are_serial_barriers_across_read_waves() {
    let barrier_a = Arc::new(Barrier::new(2));
    let barrier_b = Arc::new(Barrier::new(2));
    let (mut agent, proof, requests) = setup(Config {
        names: vec![
            (
                "read0",
                ToolEffect::Read,
                Behavior::Barrier(barrier_a.clone(), Duration::ZERO),
            ),
            (
                "read1",
                ToolEffect::Read,
                Behavior::Barrier(barrier_a, Duration::ZERO),
            ),
            ("write", ToolEffect::Write, Behavior::Immediate),
            (
                "read2",
                ToolEffect::Read,
                Behavior::Barrier(barrier_b.clone(), Duration::ZERO),
            ),
            (
                "read3",
                ToolEffect::Read,
                Behavior::Barrier(barrier_b, Duration::ZERO),
            ),
            ("unknown", ToolEffect::Unknown, Behavior::Immediate),
            ("read4", ToolEffect::Read, Behavior::Immediate),
        ],
        parallel: 4,
        timeout: Duration::from_secs(1),
        fail_intent: None,
        fail_receipt: None,
    });
    let turn = tokio::time::timeout(Duration::from_secs(2), prompt(&mut agent))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(turn.outcome, Outcome::Completed, "{:?}", turn.error);
    assert_eq!(proof.max_active.load(Ordering::SeqCst), 2);
    assert_eq!(proof.writes.load(Ordering::SeqCst), 2);
    assert_eq!(proof.effects.load(Ordering::SeqCst), 7);
    verify_context(
        &requests,
        &[
            "read0", "read1", "write", "read2", "read3", "unknown", "read4",
        ],
    );
    let log = proof.entries();
    let after_first_reads = log
        .iter()
        .position(|row| row == "receipt:read1:Succeeded")
        .unwrap();
    let write_start = log.iter().position(|row| row == "effect:write").unwrap();
    let second_reads = log.iter().position(|row| row == "effect:read2").unwrap();
    let after_second_reads = log
        .iter()
        .position(|row| row == "receipt:read3:Succeeded")
        .unwrap();
    let unknown_start = log.iter().position(|row| row == "effect:unknown").unwrap();
    assert!(
        after_first_reads < write_start
            && write_start < second_reads
            && after_second_reads < unknown_start
    );
    let trace = agent.portable_trace();
    assert_eq!(
        trace
            .iter()
            .filter(|event| matches!(event, Event::ReadWaveStarted(_)))
            .count(),
        2
    );
    assert!(turn.tool_records.iter().any(
        |record| record.effect == ToolEffect::Unknown && record.status == ToolStatus::Succeeded
    ));
}
#[tokio::test]
async fn parallel_read_budget_four_does_not_start_fifth_until_first_wave_receipts() {
    let barrier = Arc::new(Barrier::new(4));
    let (mut agent, proof, requests) = setup(Config {
        names: vec![
            (
                "r0",
                ToolEffect::Read,
                Behavior::Barrier(barrier.clone(), Duration::ZERO),
            ),
            (
                "r1",
                ToolEffect::Read,
                Behavior::Barrier(barrier.clone(), Duration::ZERO),
            ),
            (
                "r2",
                ToolEffect::Read,
                Behavior::Barrier(barrier.clone(), Duration::ZERO),
            ),
            (
                "r3",
                ToolEffect::Read,
                Behavior::Barrier(barrier, Duration::ZERO),
            ),
            ("r4", ToolEffect::Read, Behavior::Immediate),
        ],
        parallel: 4,
        timeout: Duration::from_secs(1),
        fail_intent: None,
        fail_receipt: None,
    });
    let turn = tokio::time::timeout(Duration::from_secs(2), prompt(&mut agent))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(turn.outcome, Outcome::Completed);
    assert_eq!(proof.max_active.load(Ordering::SeqCst), 4);
    let log = proof.entries();
    assert!(
        log.iter().position(|x| x == "receipt:r3:Succeeded")
            < log.iter().position(|x| x == "effect:r4")
    );
    verify_context(&requests, &["r0", "r1", "r2", "r3", "r4"]);
}
#[tokio::test]
async fn parallelism_one_preserves_sequential_path_without_read_wave_events() {
    let (mut agent, proof, requests) = setup(Config {
        names: vec![
            ("r0", ToolEffect::Read, Behavior::Immediate),
            ("r1", ToolEffect::Read, Behavior::Immediate),
        ],
        parallel: 1,
        timeout: Duration::from_secs(1),
        fail_intent: None,
        fail_receipt: None,
    });
    assert_eq!(
        prompt(&mut agent).await.unwrap().outcome,
        Outcome::Completed
    );
    assert_eq!(proof.max_active.load(Ordering::SeqCst), 1);
    assert!(!agent
        .portable_trace()
        .iter()
        .any(|event| matches!(event, Event::ReadWaveStarted(_))));
    verify_context(&requests, &["r0", "r1"]);
}
#[tokio::test]
async fn failed_second_read_intent_blocks_all_read_effects() {
    let (mut agent, proof, _) = setup(Config {
        names: vec![
            ("a", ToolEffect::Read, Behavior::Immediate),
            ("b", ToolEffect::Read, Behavior::Immediate),
        ],
        parallel: 4,
        timeout: Duration::from_secs(1),
        fail_intent: Some(2),
        fail_receipt: None,
    });
    let turn = prompt(&mut agent).await.unwrap();
    assert_eq!(turn.outcome, Outcome::Failed);
    assert_eq!(proof.effects.load(Ordering::SeqCst), 0);
    assert!(
        agent.needs_reconciliation(),
        "journal fault must fail closed even for reads"
    );
    assert_eq!(proof.provider_calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn receipt_failure_after_concurrent_reads_prevents_next_model_and_no_auto_replay() {
    let barrier = Arc::new(Barrier::new(2));
    let (mut agent, proof, _) = setup(Config {
        names: vec![
            (
                "a",
                ToolEffect::Read,
                Behavior::Barrier(barrier.clone(), Duration::ZERO),
            ),
            (
                "b",
                ToolEffect::Read,
                Behavior::Barrier(barrier, Duration::ZERO),
            ),
        ],
        parallel: 4,
        timeout: Duration::from_secs(1),
        fail_intent: None,
        fail_receipt: Some(1),
    });
    let turn = prompt(&mut agent).await.unwrap();
    assert_eq!(turn.outcome, Outcome::Failed);
    assert_eq!(proof.effects.load(Ordering::SeqCst), 2);
    assert_eq!(proof.provider_calls.load(Ordering::SeqCst), 1);
    assert!(agent.needs_reconciliation());
    assert!(agent.prompt("retry", pending(), |_| Ok(())).await.is_err());
    assert_eq!(proof.effects.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn revocation_on_last_intent_never_dispatches_the_read_wave() {
    let (mut agent, proof, _) = setup(Config {
        names: vec![
            ("a", ToolEffect::Read, Behavior::Immediate),
            ("b", ToolEffect::Read, Behavior::Immediate),
        ],
        parallel: 4,
        timeout: Duration::from_secs(1),
        fail_intent: None,
        fail_receipt: None,
    });
    let control = proof.clone();
    let turn = agent
        .prompt("parallel", pending(), move |event| {
            if let AgentEvent::ToolStarted { record, .. } = event {
                if record.call.name == "b" {
                    control.denied.store(true, Ordering::SeqCst);
                }
            }
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(turn.outcome, Outcome::Failed);
    assert_eq!(proof.effects.load(Ordering::SeqCst), 0);
    assert!(
        turn.error.as_deref().unwrap_or("").contains("denied")
            || turn.error.as_deref().unwrap_or("").contains("revoked")
    );
    assert!(
        agent.tool_specs().is_empty(),
        "invalid source must not remain advertised"
    );
    let rows = proof
        .entries()
        .into_iter()
        .filter(|row| row.starts_with("receipt:"))
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        vec!["receipt:a:Interrupted", "receipt:b:Interrupted"],
        "source revocation after journaled intents must close both receipts"
    );
}
#[tokio::test]
async fn failed_read_cancels_other_futures_and_closes_interrupted_receipts_in_order() {
    let (mut agent, proof, requests) = setup(Config {
        names: vec![
            ("a", ToolEffect::Read, Behavior::Hang),
            ("b", ToolEffect::Read, Behavior::Fail),
        ],
        parallel: 4,
        timeout: Duration::from_secs(2),
        fail_intent: None,
        fail_receipt: None,
    });
    let turn = tokio::time::timeout(Duration::from_secs(1), prompt(&mut agent))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(turn.outcome, Outcome::Failed);
    assert_eq!(turn.tool_records.len(), 2);
    assert!(turn
        .tool_records
        .iter()
        .all(|record| record.status == ToolStatus::Interrupted));
    assert_eq!(
        proof.active.load(Ordering::SeqCst),
        0,
        "no detached read remains active"
    );
    assert_eq!(
        proof.provider_calls.load(Ordering::SeqCst),
        1,
        "failed read cannot cause a second provider invocation"
    );
    assert_eq!(
        requests.lock().unwrap().len(),
        1,
        "no partial result disclosed"
    );
    let receipts = proof
        .entries()
        .into_iter()
        .filter(|row| row.starts_with("receipt:"))
        .collect::<Vec<_>>();
    assert_eq!(
        receipts,
        vec!["receipt:a:Interrupted", "receipt:b:Interrupted"]
    );
    assert!(
        !turn.needs_reconciliation,
        "read-only wave needs no unsafe effect reconciliation"
    );
}
#[tokio::test]
async fn canceled_wave_leaves_no_detached_reads_or_followup_mutations() {
    let (mut agent, proof, _) = setup(Config {
        names: vec![
            ("a", ToolEffect::Read, Behavior::Hang),
            ("b", ToolEffect::Read, Behavior::Hang),
            ("mut", ToolEffect::Write, Behavior::Immediate),
        ],
        parallel: 4,
        timeout: Duration::from_secs(2),
        fail_intent: None,
        fail_receipt: None,
    });
    let turn = tokio::time::timeout(
        Duration::from_secs(1),
        agent.prompt(
            "parallel",
            async { tokio::time::sleep(Duration::from_millis(35)).await },
            |_| Ok(()),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(turn.outcome, Outcome::Cancelled);
    assert_eq!(proof.active.load(Ordering::SeqCst), 0);
    assert_eq!(proof.writes.load(Ordering::SeqCst), 0);
    assert_eq!(proof.provider_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn process_effect_is_a_strict_barrier_and_never_overlaps_read_tool() {
    let barrier = Arc::new(Barrier::new(2));
    let (mut agent, proof, requests) = setup(Config {
        names: vec![
            (
                "a",
                ToolEffect::Read,
                Behavior::Barrier(barrier.clone(), Duration::ZERO),
            ),
            (
                "b",
                ToolEffect::Read,
                Behavior::Barrier(barrier, Duration::ZERO),
            ),
            ("proc", ToolEffect::Process, Behavior::Immediate),
            ("c", ToolEffect::Read, Behavior::Immediate),
        ],
        parallel: 4,
        timeout: Duration::from_secs(1),
        fail_intent: None,
        fail_receipt: None,
    });
    let result = tokio::time::timeout(Duration::from_secs(2), prompt(&mut agent))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.outcome, Outcome::Completed);
    assert_eq!(proof.max_active.load(Ordering::SeqCst), 2);
    assert_eq!(proof.writes.load(Ordering::SeqCst), 1);
    verify_context(&requests, &["a", "b", "proc", "c"]);
    let log = proof.entries();
    assert!(
        log.iter().position(|entry| entry == "receipt:b:Succeeded")
            < log.iter().position(|entry| entry == "effect:proc")
    );
    assert!(
        log.iter()
            .position(|entry| entry == "receipt:proc:Succeeded")
            < log.iter().position(|entry| entry == "effect:c")
    );
}
#[tokio::test]
async fn source_revoked_while_reads_in_flight_suppresses_every_result() {
    let barrier = Arc::new(Barrier::new(2));
    let (mut agent, proof, requests) = setup(Config {
        names: vec![
            (
                "a",
                ToolEffect::Read,
                Behavior::RevokeDuringRead(barrier.clone()),
            ),
            (
                "b",
                ToolEffect::Read,
                Behavior::Barrier(barrier, Duration::ZERO),
            ),
        ],
        parallel: 4,
        timeout: Duration::from_secs(1),
        fail_intent: None,
        fail_receipt: None,
    });
    let turn = tokio::time::timeout(Duration::from_secs(2), prompt(&mut agent))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(turn.outcome, Outcome::Failed);
    assert_eq!(proof.effects.load(Ordering::SeqCst), 2);
    assert_eq!(proof.provider_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        requests.lock().unwrap().len(),
        1,
        "revoked read data never returned to provider"
    );
    assert_eq!(proof.active.load(Ordering::SeqCst), 0);
    assert!(turn
        .tool_records
        .iter()
        .all(|r| r.status == ToolStatus::Interrupted));
    assert!(turn.tool_records.iter().all(|r| r.output.is_none()));
    assert!(!turn.needs_reconciliation);
    assert_eq!(
        proof
            .entries()
            .iter()
            .filter(|x| x.starts_with("receipt:"))
            .count(),
        2
    );
}
#[tokio::test]
async fn provider_receives_error_outputs_in_original_order() {
    let (mut agent, proof, requests) = setup(Config {
        names: vec![
            ("a", ToolEffect::Read, Behavior::Immediate),
            ("b", ToolEffect::Read, Behavior::ErrorOutput),
        ],
        parallel: 4,
        timeout: Duration::from_secs(1),
        fail_intent: None,
        fail_receipt: None,
    });
    let turn = prompt(&mut agent).await.unwrap();
    assert_eq!(turn.outcome, Outcome::Completed);
    assert_eq!(proof.effects.load(Ordering::SeqCst), 2);
    let guard = requests.lock().unwrap();
    let tools = guard[1]
        .iter()
        .filter(|m| m.role == Role::Tool)
        .collect::<Vec<_>>();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].name.as_deref(), Some("a"));
    assert_eq!(tools[1].name.as_deref(), Some("b"));
    assert_eq!(tools[1].content, "ERROR: fixture-error");
    assert_eq!(turn.tool_records[1].status, ToolStatus::Failed);
}
struct AfterTrace(Arc<Proof>);
impl Extension for AfterTrace {
    fn name(&self) -> &str {
        "R9_after_trace"
    }
    fn on_hook<'a>(&'a self, _: HookContext<'a>, event: HookEvent) -> HookFuture<'a> {
        Box::pin(async move {
            if let HookEvent::ToolResult { call, .. } = &event {
                self.0.emit(format!("hook:{}", call.name));
            }
            Ok(HookFlow::Next(Some(event)))
        })
    }
}
#[tokio::test]
async fn post_tool_hooks_are_serial_and_in_original_proposal_order() {
    let barrier = Arc::new(Barrier::new(2));
    let (mut agent, proof, _) = setup(Config {
        names: vec![
            (
                "a",
                ToolEffect::Read,
                Behavior::Barrier(barrier.clone(), Duration::from_millis(15)),
            ),
            (
                "b",
                ToolEffect::Read,
                Behavior::Barrier(barrier, Duration::ZERO),
            ),
        ],
        parallel: 4,
        timeout: Duration::from_secs(1),
        fail_intent: None,
        fail_receipt: None,
    });
    agent
        .register_host_extension(Arc::new(AfterTrace(proof.clone())))
        .unwrap();
    assert_eq!(
        prompt(&mut agent).await.unwrap().outcome,
        Outcome::Completed
    );
    let entries = proof.entries();
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.starts_with("hook:"))
            .cloned()
            .collect::<Vec<_>>(),
        vec!["hook:a", "hook:b"]
    );
    assert!(
        entries.iter().position(|e| e == "receipt:b:Succeeded")
            < entries.iter().position(|e| e == "hook:b")
    );
    assert!(
        entries.iter().position(|e| e == "hook:a") < entries.iter().position(|e| e == "hook:b")
    );
}

struct DenyAfterFirst;
impl Extension for DenyAfterFirst {
    fn name(&self) -> &str {
        "R9_after_veto"
    }
    fn on_hook<'a>(&'a self, _: HookContext<'a>, event: HookEvent) -> HookFuture<'a> {
        Box::pin(async move {
            if matches!(&event,HookEvent::ToolResult{call,..} if call.name=="a") {
                Err(Error::Denied("fixture post-read hook veto"))
            } else {
                Ok(HookFlow::Next(Some(event)))
            }
        })
    }
}
#[tokio::test]
async fn post_read_hook_veto_does_not_erase_completed_effect_receipts_or_call_model() {
    let barrier = Arc::new(Barrier::new(2));
    let (mut agent, proof, requests) = setup(Config {
        names: vec![
            (
                "a",
                ToolEffect::Read,
                Behavior::Barrier(barrier.clone(), Duration::from_millis(15)),
            ),
            (
                "b",
                ToolEffect::Read,
                Behavior::Barrier(barrier, Duration::ZERO),
            ),
        ],
        parallel: 4,
        timeout: Duration::from_secs(1),
        fail_intent: None,
        fail_receipt: None,
    });
    agent
        .register_host_extension(Arc::new(DenyAfterFirst))
        .unwrap();
    let turn = prompt(&mut agent).await.unwrap();
    assert_eq!(turn.outcome, Outcome::Failed);
    assert_eq!(proof.effects.load(Ordering::SeqCst), 2);
    assert_eq!(proof.provider_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        requests.lock().unwrap().len(),
        1,
        "post-hook denial must not expose partial results"
    );
    assert_eq!(
        turn.tool_records
            .iter()
            .filter(|r| r.status == ToolStatus::Succeeded)
            .count(),
        2
    );
    assert!(
        !turn.needs_reconciliation,
        "these were exact read effects only"
    );
    let receipts = proof
        .entries()
        .into_iter()
        .filter(|s| s.starts_with("receipt:"))
        .collect::<Vec<_>>();
    assert_eq!(
        receipts,
        vec!["receipt:a:Succeeded", "receipt:b:Succeeded"],
        "all factual receipts must be sealed BEFORE calling the first post-read hook"
    );
}

#[tokio::test]
async fn sixteen_model_selected_reads_really_overlap_under_default_ceiling() {
    let barrier = Arc::new(Barrier::new(16));
    let labels = [
        "read00", "read01", "read02", "read03", "read04", "read05", "read06", "read07", "read08",
        "read09", "read10", "read11", "read12", "read13", "read14", "read15",
    ];
    let (mut agent, proof, requests) = setup(Config {
        names: labels
            .iter()
            .map(|label| {
                (
                    *label,
                    ToolEffect::Read,
                    Behavior::Barrier(barrier.clone(), Duration::ZERO),
                )
            })
            .collect(),
        parallel: 16,
        timeout: Duration::from_secs(2),
        fail_intent: None,
        fail_receipt: None,
    });
    let turn = tokio::time::timeout(Duration::from_secs(3), prompt(&mut agent))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(turn.outcome, Outcome::Completed, "{:?}", turn.error);
    assert_eq!(proof.max_active.load(Ordering::SeqCst), 16);
    assert_eq!(proof.effects.load(Ordering::SeqCst), 16);
    verify_context(&requests, &labels);
    assert!(agent.portable_trace().contains(&Event::ReadWaveStarted(16)));
    let entries = proof.entries();
    let first = entries
        .iter()
        .position(|entry| entry.starts_with("effect:"))
        .unwrap();
    assert_eq!(
        entries[..first]
            .iter()
            .filter(|entry| entry.starts_with("intent:"))
            .count(),
        16
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.starts_with("receipt:"))
            .count(),
        16
    );
}
#[tokio::test]
async fn two_model_selected_bash_process_calls_overlap_only_before_a_write_barrier() {
    let barrier = Arc::new(Barrier::new(2));
    let (mut agent, proof, requests) = setup(Config {
        names: vec![
            (
                "bash",
                ToolEffect::Process,
                Behavior::Barrier(barrier.clone(), Duration::ZERO),
            ),
            (
                "bash",
                ToolEffect::Process,
                Behavior::Barrier(barrier, Duration::ZERO),
            ),
            ("mut", ToolEffect::Write, Behavior::Immediate),
        ],
        parallel: 16,
        timeout: Duration::from_secs(2),
        fail_intent: None,
        fail_receipt: None,
    });
    let turn = tokio::time::timeout(Duration::from_secs(3), prompt(&mut agent))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(turn.outcome, Outcome::Completed, "{:?}", turn.error);
    assert_eq!(proof.max_active.load(Ordering::SeqCst), 2);
    assert!(agent.portable_trace().contains(&Event::BashWaveStarted(2)));
    assert_eq!(turn.tool_records[0].effect, ToolEffect::Process);
    assert_eq!(turn.tool_records[1].effect, ToolEffect::Process);
    verify_context(&requests, &["bash", "bash", "mut"]);
    let log = proof.entries();
    assert!(
        log.iter()
            .position(|entry| entry == "receipt:bash:Succeeded")
            .unwrap()
            < log.iter().position(|entry| entry == "effect:mut").unwrap()
    );
}
#[tokio::test]
async fn host_bash_restriction_forces_serial_even_when_model_requests_parallel() {
    let (mut agent, proof, requests) = setup(Config {
        names: vec![
            ("bash", ToolEffect::Process, Behavior::Immediate),
            ("bash", ToolEffect::Process, Behavior::Immediate),
        ],
        parallel: 16,
        timeout: Duration::from_secs(1),
        fail_intent: None,
        fail_receipt: None,
    });
    agent.set_parallel_bash(false, 16).unwrap();
    let turn = prompt(&mut agent).await.unwrap();
    assert_eq!(turn.outcome, Outcome::Completed);
    assert_eq!(proof.max_active.load(Ordering::SeqCst), 1);
    assert!(!agent
        .portable_trace()
        .iter()
        .any(|e| matches!(e, Event::BashWaveStarted(_))));
    assert_eq!(proof.writes.load(Ordering::SeqCst), 2);
    verify_context(&requests, &["bash", "bash"]);
}

#[tokio::test]
async fn model_declines_parallel_reads_even_when_host_ceiling_is_sixteen() {
    let (mut agent, proof, requests) = setup(Config {
        names: vec![
            ("a", ToolEffect::Read, Behavior::Immediate),
            ("b", ToolEffect::Read, Behavior::Immediate),
        ],
        parallel: 16,
        timeout: Duration::from_secs(1),
        fail_intent: None,
        fail_receipt: None,
    });
    proof.model_parallel.store(false, Ordering::SeqCst);
    assert_eq!(
        prompt(&mut agent).await.unwrap().outcome,
        Outcome::Completed
    );
    assert_eq!(proof.max_active.load(Ordering::SeqCst), 1);
    assert!(!agent
        .portable_trace()
        .iter()
        .any(|e| matches!(e, Event::ReadWaveStarted(_))));
    verify_context(&requests, &["a", "b"]);
}
#[tokio::test]
async fn model_declines_parallel_bash_even_when_granted_and_host_allows() {
    let (mut agent, proof, requests) = setup(Config {
        names: vec![
            ("bash", ToolEffect::Process, Behavior::Immediate),
            ("bash", ToolEffect::Process, Behavior::Immediate),
        ],
        parallel: 16,
        timeout: Duration::from_secs(1),
        fail_intent: None,
        fail_receipt: None,
    });
    proof.model_parallel.store(false, Ordering::SeqCst);
    assert_eq!(
        prompt(&mut agent).await.unwrap().outcome,
        Outcome::Completed
    );
    assert_eq!(proof.max_active.load(Ordering::SeqCst), 1);
    assert!(!agent
        .portable_trace()
        .iter()
        .any(|e| matches!(e, Event::BashWaveStarted(_))));
    verify_context(&requests, &["bash", "bash"]);
}
#[tokio::test]
async fn failed_model_selected_bash_wave_remains_uncertain_and_never_replays() {
    let (mut agent, proof, requests) = setup(Config {
        names: vec![
            ("bash", ToolEffect::Process, Behavior::Fail),
            ("bash", ToolEffect::Process, Behavior::Fail),
        ],
        parallel: 16,
        timeout: Duration::from_secs(1),
        fail_intent: None,
        fail_receipt: None,
    });
    let turn = prompt(&mut agent).await.unwrap();
    assert_eq!(turn.outcome, Outcome::Failed);
    assert!(
        turn.needs_reconciliation,
        "Process intents must remain unsafe after failed parallel execution"
    );
    assert_eq!(turn.tool_records.len(), 2);
    assert!(turn
        .tool_records
        .iter()
        .all(|r| r.effect == ToolEffect::Process));
    assert!(turn
        .tool_records
        .iter()
        .all(|r| r.status == ToolStatus::Interrupted));
    assert_eq!(
        requests.lock().unwrap().len(),
        1,
        "no partial Process output can leak"
    );
    assert!(agent.needs_reconciliation());
    let before = proof.effects.load(Ordering::SeqCst);
    assert!(agent.prompt("retry", pending(), |_| Ok(())).await.is_err());
    assert_eq!(proof.effects.load(Ordering::SeqCst), before);
}

#[tokio::test]
async fn sixteen_large_model_selected_outputs_truncate_but_keep_complete_ordered_batch() {
    let labels = [
        "large00", "large01", "large02", "large03", "large04", "large05", "large06", "large07",
        "large08", "large09", "large10", "large11", "large12", "large13", "large14", "large15",
    ];
    let (mut agent, proof, requests) = setup(Config {
        names: labels
            .iter()
            .map(|label| (*label, ToolEffect::Read, Behavior::BigOutput(128 * 1024)))
            .collect(),
        parallel: 16,
        timeout: Duration::from_secs(2),
        fail_intent: None,
        fail_receipt: None,
    });
    let turn = tokio::time::timeout(Duration::from_secs(3), prompt(&mut agent))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(turn.outcome, Outcome::Completed, "{:?}", turn.error);
    assert_eq!(turn.tool_records.len(), 16);
    assert!(turn.tool_records.iter().all(|r| r
        .output
        .as_ref()
        .is_some_and(|o| o.truncated && o.content.len() < 128 * 1024)));
    assert_eq!(proof.effects.load(Ordering::SeqCst), 16);
    assert!(
        turn.context_messages
            .iter()
            .map(Message::context_bytes)
            .sum::<usize>()
            <= aw_agent_core::limits::MAX_CONTEXT_BYTES
    );
    let continuations = requests.lock().unwrap();
    assert_eq!(continuations.len(), 2);
    let tools = continuations[1]
        .iter()
        .filter(|msg| msg.role == Role::Tool)
        .collect::<Vec<_>>();
    assert_eq!(tools.len(), 16);
    assert!(tools
        .iter()
        .all(|msg| msg.content.ends_with("[output truncated]")));
    for (i, msg) in tools.iter().enumerate() {
        assert_eq!(
            msg.tool_call_id.as_deref(),
            Some(format!("call_{i}").as_str())
        );
    }
}
