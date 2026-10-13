use aw_agent_core::*;
use std::{
    future::pending,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
struct MemoryJournal {
    fail: &'static str,
    events: Mutex<Vec<&'static str>>,
}
impl MemoryJournal {
    fn record(&self, name: &'static str) -> Result<()> {
        self.events.lock().unwrap().push(name);
        if self.fail == name {
            Err(Error::Storage("fixture journal failure"))
        } else {
            Ok(())
        }
    }
}
impl TurnJournal for MemoryJournal {
    fn begin<'a>(&'a self, _: TurnStart) -> JournalFuture<'a, ()> {
        Box::pin(async move { self.record("begin") })
    }
    fn append<'a>(&'a self, _: u64, e: JournalEntry) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            self.record(match e {
                JournalEntry::InvocationStarted { .. } => "invocation_start",
                JournalEntry::InvocationFinished { .. } => "invocation_finish",
                JournalEntry::Assistant { .. } => "assistant",
                JournalEntry::ToolStarted { .. } => "tool_start",
                JournalEntry::ToolFinished { .. } => "tool_finish",
                JournalEntry::ToolDenied { .. } => "tool_denied",
                JournalEntry::ToolRevised { .. } => "tool_revised",
            })
        })
    }
    fn finish<'a>(&'a self, _: Turn) -> JournalFuture<'a, ()> {
        Box::pin(async move { self.record("finish") })
    }
    fn acknowledge<'a>(&'a self, _: String) -> JournalFuture<'a, ()> {
        Box::pin(async move { self.record("acknowledge") })
    }
}
struct Echo(Arc<AtomicUsize>);
impl Provider for Echo {
    fn stream<'a>(
        &'a self,
        _: &'a [Message],
        _: &'a str,
        s: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            s("done")?;
            Ok(CompletionMeta {
                finish_reason: "stop".into(),
                ..Default::default()
            })
        })
    }
}
fn setup(fail: &'static str) -> (Agent, Arc<AtomicUsize>, Arc<MemoryJournal>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let j = Arc::new(MemoryJournal {
        fail,
        events: Mutex::new(vec![]),
    });
    let mut a = Agent::builder(
        Arc::new(Echo(calls.clone())),
        AgentOptions::new("fixture", "model"),
    )
    .build()
    .unwrap();
    a.load_history(HistoryState::default(), Some(j.clone()))
        .unwrap();
    (a, calls, j)
}
#[tokio::test]
async fn durable_order_precedes_provider_and_final_observer() {
    let (mut a, c, j) = setup("");
    a.prompt("hi", pending::<()>(), |e| {
        if matches!(e, AgentEvent::Finished { .. }) {
            assert_eq!(j.events.lock().unwrap().last(), Some(&"finish"));
        }
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(c.load(Ordering::SeqCst), 1);
    assert_eq!(
        *j.events.lock().unwrap(),
        vec![
            "begin",
            "invocation_start",
            "invocation_finish",
            "assistant",
            "finish"
        ]
    );
}
#[tokio::test]
async fn begin_error_prevents_provider() {
    let (mut a, c, _) = setup("begin");
    assert!(a.prompt("hi", pending::<()>(), |_| Ok(())).await.is_err());
    assert_eq!(c.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn invocation_intent_failure_prevents_model_dispatch() {
    let (mut a, c, _) = setup("invocation_start");
    let t = a.prompt("hi", pending::<()>(), |_| Ok(())).await.unwrap();
    assert_eq!(t.outcome, Outcome::Failed);
    assert_eq!(c.load(Ordering::SeqCst), 0);
    assert!(a
        .prompt("again", pending::<()>(), |_| Ok(()))
        .await
        .is_err());
}
#[tokio::test]
async fn final_write_failure_blocks_retries_and_acknowledgement() {
    let (mut a, c, _) = setup("finish");
    assert!(a.prompt("hi", pending::<()>(), |_| Ok(())).await.is_err());
    assert!(a.turns().is_empty());
    assert!(a.needs_reconciliation());
    assert!(a
        .prompt("again", pending::<()>(), |_| Ok(()))
        .await
        .is_err());
    assert!(a.acknowledge_reconciliation("inspected").await.is_err());
    assert_eq!(c.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn started_observer_error_is_durably_settled_without_inference() {
    let (mut a, c, j) = setup("");
    let t = a
        .prompt("hi", pending::<()>(), |e| {
            if matches!(e, AgentEvent::Started { .. }) {
                Err(Error::Protocol("observer refused"))
            } else {
                Ok(())
            }
        })
        .await
        .unwrap();
    assert_eq!(t.outcome, Outcome::Failed);
    assert_eq!(c.load(Ordering::SeqCst), 0);
    assert_eq!(*j.events.lock().unwrap(), vec!["begin", "finish"]);
}
#[test]
fn serialized_history_has_no_account_handles_or_credentials() {
    let (a, _, _) = setup("");
    let serialized = serde_json::to_string(&a.history_state()).unwrap();
    assert!(!serialized.contains("api_key_env"));
    assert!(!serialized.contains("base_url"));
    assert!(!serialized.contains("provider_context"));
}
#[test]
fn restore_rejects_invalid_next_id() {
    let (mut a, _, _) = setup("");
    let state = HistoryState {
        next_turn_id: u64::MAX,
        ..HistoryState::default()
    };
    assert!(a.load_history(state, None).is_err());
}
#[tokio::test]
async fn restore_refuses_hidden_provider_continuations() {
    let (mut a, _, _) = setup("");
    a.prompt("hi", pending::<()>(), |_| Ok(())).await.unwrap();
    let mut state = a.history_state();
    state.turns[0].context_messages[0].provider_context = Some("not portable".into());
    assert!(a.load_history(state, None).is_err());
}
#[tokio::test]
async fn history_load_preserves_current_account_and_grants() {
    let (mut a, _, _) = setup("");
    a.prompt("hi", pending::<()>(), |_| Ok(())).await.unwrap();
    let h = a.history_state();
    let mut other = Agent::builder(
        Arc::new(Echo(Arc::new(AtomicUsize::new(0)))),
        AgentOptions::new("new-account", "new-model"),
    )
    .build()
    .unwrap();
    other.load_history(h, None).unwrap();
    assert_eq!(other.default_identity().unwrap().account_id, "new-account");
    assert!(other.tool_specs().is_empty());
}
#[tokio::test]
async fn explicit_ack_is_a_journal_action() {
    let (mut a, _, j) = setup("");
    let state = HistoryState {
        needs_reconciliation: true,
        ..HistoryState::default()
    };
    a.load_history(state, Some(j.clone())).unwrap();
    a.acknowledge_reconciliation("inspected the actual target")
        .await
        .unwrap();
    assert_eq!(*j.events.lock().unwrap(), vec!["acknowledge"]);
    assert!(!a.needs_reconciliation());
}

/// AW-P6-10F: the original Tool may factually finish its Write effect while
/// its Host source is revoked. The immutable ToolFinished MUST be recorded
/// before any model-facing result is released, and no paid follow-on request
/// may execute under the revoked original source.
struct RevokedAfterWrite {
    available: Arc<AtomicBool>,
    executions: Arc<AtomicUsize>,
}
impl Tool for RevokedAfterWrite {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "revoke_write".into(),
            description: "original effect revocation fixture".into(),
            parameters: serde_json::json!({"type":"object","properties":{},"additionalProperties":false}),
            effect: ToolEffect::Write,
        }
    }
    fn is_available(&self) -> bool {
        self.available.load(Ordering::SeqCst)
    }
    fn validate(&self, args: &serde_json::Value) -> Result<()> {
        if args.as_object().is_some_and(serde_json::Map::is_empty) {
            Ok(())
        } else {
            Err(Error::Denied("invalid original Write proposal"))
        }
    }
    fn execute<'a>(&'a self, _: ToolContext, _: &'a serde_json::Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.executions.fetch_add(1, Ordering::SeqCst);
            // Simulated concurrent source invalidation AFTER an actual effect.
            // The revocation must NOT retroactively erase its factual receipt.
            self.available.store(false, Ordering::SeqCst);
            Ok(ToolOutput::text("original external effect was applied"))
        })
    }
}
struct ProposeOriginalWrite(Arc<AtomicUsize>);
impl Provider for ProposeOriginalWrite {
    fn supports_tools(&self) -> bool {
        true
    }
    fn stream<'a>(
        &'a self,
        _: &'a [Message],
        _: &'a str,
        _: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async { Err(Error::Protocol("original test needs typed tool reply")) })
    }
    fn stream_tools<'a>(
        &'a self,
        _: &'a [Message],
        _: &'a str,
        _: &'a [ToolSpec],
        _: &'a mut DeltaSink<'_>,
    ) -> ModelFuture<'a> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(ModelReply {
                meta: CompletionMeta {
                    finish_reason: "tool_calls".into(),
                    ..Default::default()
                },
                tool_calls: vec![ToolCall {
                    id: "write_and_revoke".into(),
                    name: "revoke_write".into(),
                    arguments: serde_json::json!({}),
                }],
                ..Default::default()
            })
        })
    }
}
#[tokio::test]
async fn aw_p6_10f_failed_original_tool_intent_append_does_not_enter_os_effect_or_retry_paid_call()
{
    // Fault-inject a failing ORIGINAL immutable ToolStarted append after the
    // paid model has proposed a Write but BEFORE any external effect starts.
    let available = Arc::new(AtomicBool::new(true));
    let executions = Arc::new(AtomicUsize::new(0));
    let paid_calls = Arc::new(AtomicUsize::new(0));
    let journal = Arc::new(MemoryJournal {
        fail: "tool_start",
        events: Mutex::new(vec![]),
    });
    let mut policy = ToolPolicy::default();
    policy
        .permit_host(
            PrincipalId::new("local").unwrap(),
            "revoke_write",
            &ToolEffect::Write,
        )
        .unwrap();
    let mut agent = Agent::builder(
        Arc::new(ProposeOriginalWrite(paid_calls.clone())),
        AgentOptions::new("journal-intent-fault", "model"),
    )
    .tool_policy(policy)
    .tool(RevokedAfterWrite {
        available,
        executions: executions.clone(),
    })
    .build()
    .unwrap();
    agent
        .load_history(HistoryState::default(), Some(journal.clone()))
        .unwrap();
    let _ = agent
        .prompt("Write after durable start", pending::<()>(), |_| Ok(()))
        .await;
    assert_eq!(
        paid_calls.load(Ordering::SeqCst),
        1,
        "original paid proposal occurs only once"
    );
    assert_eq!(
        executions.load(Ordering::SeqCst),
        0,
        "no Tool effect without durable ToolStarted ACK"
    );
    assert!(journal.events.lock().unwrap().contains(&"tool_start"));
    assert!(!journal.events.lock().unwrap().contains(&"tool_finish"));
    assert!(
        agent.needs_reconciliation(),
        "failed durable original intent is not a retry permit"
    );
    assert!(agent
        .prompt("never replay uncertain proposal", pending::<()>(), |_| Ok(
            ()
        ))
        .await
        .is_err());
    assert_eq!(paid_calls.load(Ordering::SeqCst), 1);
    assert_eq!(executions.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn aw_p6_10f_write_source_revoked_after_effect_preserves_receipt_but_blocks_next_paid_round()
{
    let available = Arc::new(AtomicBool::new(true));
    let executions = Arc::new(AtomicUsize::new(0));
    let paid_calls = Arc::new(AtomicUsize::new(0));
    let journal = Arc::new(MemoryJournal {
        fail: "",
        events: Mutex::new(vec![]),
    });
    let mut policy = ToolPolicy::default();
    policy
        .permit_host(
            PrincipalId::new("local").unwrap(),
            "revoke_write",
            &ToolEffect::Write,
        )
        .unwrap();
    let mut agent = Agent::builder(
        Arc::new(ProposeOriginalWrite(paid_calls.clone())),
        AgentOptions::new("source-race", "model"),
    )
    .tool_policy(policy)
    .tool(RevokedAfterWrite {
        available: available.clone(),
        executions: executions.clone(),
    })
    .build()
    .unwrap();
    agent
        .load_history(HistoryState::default(), Some(journal.clone()))
        .unwrap();
    let result = agent
        .prompt("perform one authorized Write", pending::<()>(), |_| Ok(()))
        .await
        .unwrap();
    assert_eq!(paid_calls.load(Ordering::SeqCst), 1);
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert!(!available.load(Ordering::SeqCst));
    assert_eq!(
        result.outcome,
        Outcome::Failed,
        "revoked source must block new model-visible tool result"
    );
    let events = journal.events.lock().unwrap().clone();
    let started = events.iter().position(|e| *e == "tool_start").unwrap();
    let received = events.iter().position(|e| *e == "tool_finish").unwrap();
    assert!(
        started < received,
        "original Tool intent must precede factual completion"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == "tool_start")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == "tool_finish")
            .count(),
        1
    );
    assert!(
        agent.needs_reconciliation(),
        "already-executed Write still needs inspection on failed Turn"
    );
    assert!(agent
        .prompt("do not retry this Write", pending::<()>(), |_| Ok(()))
        .await
        .is_err());
    assert_eq!(
        paid_calls.load(Ordering::SeqCst),
        1,
        "revocation must forbid second paid attempt"
    );
    assert_eq!(
        executions.load(Ordering::SeqCst),
        1,
        "revocation must forbid OS Tool replay"
    );
}

#[test]
fn aw_p6_10e_original_tool_source_evidence_is_bounded_backward_readable_and_non_authorizing() {
    let record = ToolRecord {
        call: ToolCall {
            id: "original-intent".into(),
            name: "queue_enqueue".into(),
            arguments: serde_json::json!({"op":"enqueue","text":"original"}),
        },
        effect: ToolEffect::Write,
        execution_site: "local".into(),
        status: ToolStatus::Running,
        output: None,
    };
    // Existing historical private ToolStarted receipts remain readable:
    // optional provenance MUST NOT silently become Host authorization.
    let original_json = serde_json::json!({"type":"tool_started","record":record});
    let original: JournalEntry = serde_json::from_value(original_json).unwrap();
    assert!(matches!(
        original,
        JournalEntry::ToolStarted { source: None, .. }
    ));
    assert!(serde_json::to_value(&original)
        .unwrap()
        .get("source")
        .is_none());

    let exact = ToolSourceEvidence::trusted_host();
    exact.validate().unwrap();
    assert_eq!(exact.kind, "host");
    assert_eq!(exact.id, "trusted-host");
    assert_eq!(exact.epoch, 1);
    let pinned = JournalEntry::ToolStarted {
        record,
        source: Some(exact.clone()),
    };
    let serialized = serde_json::to_value(&pinned).unwrap();
    assert_eq!(serialized["source"]["kind"], "host");
    assert_eq!(serialized["source"]["epoch"], 1);
    let reread: JournalEntry = serde_json::from_value(serialized).unwrap();
    assert!(matches!(reread, JournalEntry::ToolStarted { source: Some(s), .. } if s == exact));
    for (kind, id, epoch) in [
        ("unknown", "trusted-host", 1),
        ("host", "", 1),
        ("host", "trusted-host", 0),
        ("host", "../../unsafe", 1),
    ] {
        assert!(ToolSourceEvidence {
            kind: kind.into(),
            id: id.into(),
            epoch,
        }
        .validate()
        .is_err());
    }
    let extension = ToolSource::new(SourceKind::Extension, "ext-a", 17).unwrap();
    let captured = ToolSourceEvidence::from_source(&extension);
    assert_eq!(captured.kind, "extension");
    assert_eq!(captured.id, "ext-a");
    assert_eq!(captured.epoch, 17);
    captured.validate().unwrap();
}
