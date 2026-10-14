//! Actual Rust/duplex tests, requiring a compiler. These are NOT substituted by
//! the independent Python wire/ledger probes in the source-candidate evidence.
use crate::{
    protocol::{
        self, Authenticator, Event, Failure, PairingKey, Receiver, Request, Sender, WireMessage,
        WireTool,
    },
    receipts::{ReceiptBook, State},
    test_support::*,
    worker::DeviceWorker,
    Result,
};
use aw_agent_core::{
    CancellationToken, CompletionFuture, CompletionMeta, DeltaSink, Message, ModelFuture,
    ModelReply, Provider, Role, ToolCall, ToolSpec,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::io::{DuplexStream, ReadHalf, WriteHalf};
struct Fake {
    calls: Arc<AtomicUsize>,
    kind: &'static str,
}
impl Provider for Fake {
    fn supports_tools(&self) -> bool {
        self.kind == "tools"
    }
    fn stream<'a>(
        &'a self,
        _: &'a [Message],
        _: &'a str,
        sink: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.kind {
                "slow" => std::future::pending::<Result<CompletionMeta>>().await,
                "large" => {
                    sink(&"x".repeat(9000))?;
                    Ok(CompletionMeta {
                        finish_reason: "stop".into(),
                        ..Default::default()
                    })
                }
                _ => {
                    sink("fixture ✓")?;
                    Ok(CompletionMeta {
                        finish_reason: "stop".into(),
                        ..Default::default()
                    })
                }
            }
        })
    }
    fn stream_tools<'a>(
        &'a self,
        messages: &'a [Message],
        model: &'a str,
        tools: &'a [ToolSpec],
        sink: &'a mut DeltaSink<'_>,
    ) -> ModelFuture<'a> {
        Box::pin(async move {
            if self.kind != "tools" {
                return Ok(ModelReply {
                    meta: self.stream(messages, model, sink).await?,
                    tool_calls: vec![],
                    provider_context: None,
                });
            }
            let step = self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(tools.len(), 1);
            if step == 0 {
                sink("inspect")?;
                Ok(ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "tool_calls".into(),
                        ..Default::default()
                    },
                    tool_calls: vec![ToolCall {
                        id: "tool-one".into(),
                        name: "read".into(),
                        arguments: serde_json::json!({"path":"x"}),
                    }],
                    provider_context: Some("PRIVATE_CONTINUATION_ONLY_ON_DEVICE".into()),
                })
            } else {
                assert!(messages.iter().any(|m| m.provider_context.as_deref()
                    == Some("PRIVATE_CONTINUATION_ONLY_ON_DEVICE")));
                sink("done")?;
                Ok(ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "stop".into(),
                        ..Default::default()
                    },
                    tool_calls: vec![],
                    provider_context: None,
                })
            }
        })
    }
}
struct Harness {
    dir: PathBuf,
    request: Request,
    tx: Sender<WriteHalf<DuplexStream>>,
    rx: Receiver<ReadHalf<DuplexStream>>,
    calls: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<Result<()>>,
}
impl Harness {
    async fn start(kind: &'static str) -> Self {
        Self::start_custom(kind, None).await
    }
    async fn start_custom(kind: &'static str, custom: Option<(crate::Grant, Request)>) -> Self {
        let dir = private_dir();
        let (mut g, mut prepared) = match custom {
            Some((g, r)) => (g, Some(r)),
            None => (grant(), None),
        };
        if kind == "tools" {
            g.allowed_tools.insert("read".into());
        }
        crate::private::create(&dir.join("grant.json"), &serde_json::to_vec(&g).unwrap()).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut worker = DeviceWorker::open(
            Arc::new(Fake {
                calls: calls.clone(),
                kind,
            }),
            &dir.join("grant.json"),
            &dir.join("receipts.json"),
            &"11".repeat(32),
        )
        .unwrap();
        let key = Arc::new(PairingKey::from_bytes([7; 32]));
        let device_key = key.clone();
        let (mut owner, device) = tokio::io::duplex(65536);
        let task = tokio::spawn(async move {
            worker
                .run_stream(device, device_key, CancellationToken::new())
                .await
        });
        let (hello, nonce) = protocol::owner_handshake(&mut owner, &key, g.binding.clone())
            .await
            .unwrap();
        let (read, write) = tokio::io::split(owner);
        let mut r = prepared.take().unwrap_or_else(|| request(&g));
        r.epoch = hello.epoch.clone();
        Self {
            dir,
            request: r,
            tx: Sender::new(
                write,
                Authenticator::new(key.clone(), &hello, &nonce, true).unwrap(),
            ),
            rx: Receiver::new(read, Authenticator::new(key, &hello, &nonce, true).unwrap()),
            calls,
            task,
        }
    }
    async fn events(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        loop {
            let event = tokio::time::timeout(Duration::from_secs(2), self.rx.next::<Event>())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let done = !matches!(&event, Event::Delta { .. });
            out.push(event);
            if done {
                return out;
            }
        }
    }
    async fn close(self) {
        let Self {
            dir, tx, rx, task, ..
        } = self;
        drop(tx);
        drop(rx);
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[tokio::test]
async fn exact_model_round_then_duplicate_is_not_reexecuted() {
    let mut h = Harness::start("text").await;
    h.tx.send(&h.request).await.unwrap();
    let events = h.events().await;
    assert!(matches!(events.last(), Some(Event::Finished { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e,Event::Delta{text,..} if text=="fixture ✓")));
    h.tx.send(&h.request).await.unwrap();
    assert!(matches!(
        h.events().await.last(),
        Some(Event::Failed {
            code: Failure::AlreadyAttempted,
            ..
        })
    ));
    assert_eq!(h.calls.load(Ordering::SeqCst), 1);
    h.close().await;
}
#[tokio::test]
async fn unauthorized_model_is_denied_before_inference() {
    let mut h = Harness::start("text").await;
    h.request.context.identity.model = "not-granted".into();
    h.tx.send(&h.request).await.unwrap();
    assert!(matches!(
        h.events().await.last(),
        Some(Event::Failed {
            code: Failure::Denied,
            ..
        })
    ));
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
    h.close().await;
}
#[tokio::test]
async fn cross_session_denied_before_any_local_secret_use() {
    let mut h = Harness::start("text").await;
    h.request.context.scope.as_mut().unwrap().session_id = "s_foreign".into();
    h.request.id = Request::identity(&h.request.context).unwrap();
    h.tx.send(&h.request).await.unwrap();
    assert!(matches!(
        h.events().await.last(),
        Some(Event::Failed {
            code: Failure::Denied,
            ..
        })
    ));
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
    h.close().await;
}
#[tokio::test]
async fn visible_output_limit_is_not_a_success() {
    let mut h = Harness::start("large").await;
    h.tx.send(&h.request).await.unwrap();
    assert!(matches!(
        h.events().await.last(),
        Some(Event::Failed {
            code: Failure::Limit,
            ..
        })
    ));
    assert_eq!(
        ReceiptBook::inspect(&h.dir.join("receipts.json"))
            .unwrap()
            .receipts[&h.request.id]
            .state,
        State::Failed
    );
    h.close().await;
}
#[tokio::test]
async fn owner_disconnect_records_uncertainty_and_drops_future() {
    let h = Harness::start("slow").await;
    let Harness {
        dir,
        request,
        mut tx,
        rx,
        calls,
        task,
        ..
    } = h;
    tx.send(&request).await.unwrap();
    for _ in 0..100 {
        if calls.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    drop(tx);
    drop(rx);
    assert!(tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    assert_eq!(
        ReceiptBook::inspect(&dir.join("receipts.json"))
            .unwrap()
            .receipts[&request.id]
            .state,
        State::Interrupted
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn deleting_grant_stops_future_processing() {
    let mut h = Harness::start("slow").await;
    h.tx.send(&h.request).await.unwrap();
    for _ in 0..100 {
        if h.calls.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    std::fs::remove_file(h.dir.join("grant.json")).unwrap();
    let Harness {
        dir, tx, rx, task, ..
    } = h;
    assert!(tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    drop(tx);
    drop(rx);
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn private_continuation_stays_local_across_tool_rounds() {
    let mut h = Harness::start("tools").await;
    h.request.tools.push(WireTool {
        name: "read".into(),
        description: "fixture schema, no local execution".into(),
        parameters: serde_json::json!({"type":"object"}),
        effect: aw_agent_core::ToolEffect::Read,
    });
    h.tx.send(&h.request).await.unwrap();
    let first = h.events().await;
    let (calls, continuation) = match first.last().unwrap() {
        Event::Finished {
            tool_calls,
            continuation,
            ..
        } => (tool_calls.clone(), continuation.clone()),
        _ => panic!("expected tool proposal"),
    };
    assert!(!serde_json::to_string(&first)
        .unwrap()
        .contains("PRIVATE_CONTINUATION"));
    assert_eq!(calls[0].name, "read");
    h.request.context.ordinal = 2;
    h.request.id = Request::identity(&h.request.context).unwrap();
    h.request.messages.push(WireMessage {
        id: 2,
        role: Role::Assistant,
        content: "inspect".into(),
        tool_calls: calls,
        tool_call_id: None,
        name: None,
        continuation,
    });
    h.request.messages.push(WireMessage {
        id: 3,
        role: Role::Tool,
        content: "fixture result from OWNER tool".into(),
        tool_calls: vec![],
        tool_call_id: Some("tool-one".into()),
        name: Some("read".into()),
        continuation: None,
    });
    h.tx.send(&h.request).await.unwrap();
    assert!(matches!(
        h.events().await.last(),
        Some(Event::Finished { .. })
    ));
    assert_eq!(h.calls.load(Ordering::SeqCst), 2);
    h.close().await;
}
#[tokio::test]
async fn modified_assistant_block_cannot_reuse_private_handle() {
    let mut h = Harness::start("tools").await;
    h.request.tools.push(WireTool {
        name: "read".into(),
        description: "fixture".into(),
        parameters: serde_json::json!({}),
        effect: aw_agent_core::ToolEffect::Read,
    });
    h.tx.send(&h.request).await.unwrap();
    let first = h.events().await;
    let token = match first.last().unwrap() {
        Event::Finished { continuation, .. } => continuation.clone(),
        _ => panic!("missing response"),
    };
    h.request.context.ordinal = 2;
    h.request.id = Request::identity(&h.request.context).unwrap();
    h.request.messages.push(WireMessage {
        id: 2,
        role: Role::Assistant,
        content: "changed".into(),
        tool_calls: vec![],
        tool_call_id: None,
        name: None,
        continuation: token,
    });
    h.tx.send(&h.request).await.unwrap();
    assert!(matches!(
        h.events().await.last(),
        Some(Event::Failed {
            code: Failure::Denied,
            ..
        })
    ));
    assert_eq!(h.calls.load(Ordering::SeqCst), 1);
    h.close().await;
}

fn summary_fixture() -> (crate::Grant, Request) {
    let mut g = grant();
    g.version = 2;
    let refs = vec![aw_agent_core::EvidenceRef {
        authority: "realm-a".into(),
        session_id: "s_source".into(),
        node_id: "n_source".into(),
        sha256: "12".repeat(32),
    }];
    let input = aw_agent_core::SummaryInput { plan_id: "cp_fixture".into(), focus: "Qualified source findings".into(),
        evidence_json: serde_json::json!({"entries": [{"text":"public fixture evidence"}], "dependencies": refs}).to_string(),
        sources: refs.clone(), max_output_bytes: 1024, timeout_seconds: 5 };
    let digest = input.digest().unwrap();
    g.allowed_summary_digests.insert(digest.clone());
    let mut r = request(&g);
    r.context.purpose = aw_agent_core::InvocationPurpose::Summary {
        operation_id: "cs_fixture".into(),
        input_sha256: digest,
        sources: refs,
    };
    r.messages = input
        .messages()
        .unwrap()
        .into_iter()
        .map(|m| WireMessage {
            id: m.id,
            role: m.role,
            content: m.content,
            tool_calls: vec![],
            tool_call_id: None,
            name: None,
            continuation: None,
        })
        .collect();
    (g, r)
}
#[tokio::test]
async fn summary_v2_worker_accepts_only_the_consented_tool_free_envelope() {
    let mut h = Harness::start_custom("text", Some(summary_fixture())).await;
    h.tx.send(&h.request).await.unwrap();
    assert!(matches!(
        h.events().await.last(),
        Some(Event::Finished { .. })
    ));
    assert_eq!(h.calls.load(Ordering::SeqCst), 1);
    h.tx.send(&h.request).await.unwrap();
    assert!(matches!(
        h.events().await.last(),
        Some(Event::Failed {
            code: Failure::AlreadyAttempted,
            ..
        })
    ));
    assert_eq!(h.calls.load(Ordering::SeqCst), 1);
    h.close().await;
}
#[tokio::test]
async fn summary_v2_worker_rejects_changed_evidence_before_provider_use() {
    let mut h = Harness::start_custom("text", Some(summary_fixture())).await;
    h.request.messages[1].content.push(' ');
    h.tx.send(&h.request).await.unwrap();
    assert!(matches!(
        h.events().await.last(),
        Some(Event::Failed { .. })
    ));
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
    h.close().await;
}
#[tokio::test]
async fn legacy_chat_grant_does_not_authorize_worker_summary() {
    let (mut g, r) = summary_fixture();
    g.version = 1;
    g.allowed_summary_digests.clear();
    let mut h = Harness::start_custom("text", Some((g, r))).await;
    h.tx.send(&h.request).await.unwrap();
    assert!(matches!(
        h.events().await.last(),
        Some(Event::Failed {
            code: Failure::Denied,
            ..
        })
    ));
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
    h.close().await;
}
