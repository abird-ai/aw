use aw_agent_core::*;
use std::{
    future::pending,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

struct Fixture {
    calls: Arc<AtomicUsize>,
    contexts: Arc<Mutex<Vec<InvocationContext>>>,
    tool: bool,
    oversized: bool,
}

impl Provider for Fixture {
    fn stream<'a>(
        &'a self,
        _: &'a [Message],
        _: &'a str,
        _: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async { Err(Error::Protocol("identity-aware call required")) })
    }

    fn invoke<'a>(
        &'a self,
        context: &'a InvocationContext,
        messages: &'a [Message],
        _: &'a str,
        tools: &'a [ToolSpec],
        sink: &'a mut DeltaSink<'_>,
    ) -> ModelFuture<'a> {
        Box::pin(async move {
            assert!(
                tools.is_empty(),
                "auxiliary inference must never receive tools"
            );
            assert_eq!(messages.last().map(|m| m.role), Some(Role::User));
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.contexts.lock().unwrap().push(context.clone());
            if self.oversized {
                sink(&"x".repeat(1024))?;
            } else {
                sink("auxiliary answer")?;
            }
            Ok(ModelReply {
                meta: CompletionMeta {
                    finish_reason: "stop".into(),
                    usage: Usage {
                        input_tokens: Some(7),
                        output_tokens: Some(3),
                    },
                },
                tool_calls: if self.tool {
                    vec![ToolCall {
                        id: "bad".into(),
                        name: "bash".into(),
                        arguments: serde_json::json!({"command":"never"}),
                    }]
                } else {
                    Vec::new()
                },
                provider_context: None,
            })
        })
    }
}

struct Journal {
    allowed: AtomicBool,
    began: AtomicBool,
    finished: Mutex<Vec<AuxiliaryResult>>,
    fail_finish: bool,
}

impl AuxiliaryJournal for Journal {
    fn authorize<'a>(
        &'a self,
        _: &'a AuxiliaryInput,
        _: &'a InvocationIdentity,
    ) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            if self.allowed.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(Error::Denied("auxiliary denied"))
            }
        })
    }

    fn begin<'a>(
        &'a self,
        input: &'a AuxiliaryInput,
        identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, InvocationContext> {
        Box::pin(async move {
            assert!(!self.began.swap(true, Ordering::SeqCst));
            Ok(InvocationContext {
                purpose: InvocationPurpose::Auxiliary {
                    operation_id: "aux_fixture".into(),
                    input_sha256: input.digest()?,
                    auxiliary_kind: input.kind.label().into(),
                },
                scope: Some(InvocationScope {
                    session_id: "s_fixture".into(),
                    branch_id: "b_fixture".into(),
                }),
                turn_id: 0,
                ordinal: 1,
                identity: identity.clone(),
            })
        })
    }

    fn finish<'a>(&'a self, result: &'a AuxiliaryResult) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            self.finished.lock().unwrap().push(result.clone());
            if self.fail_finish {
                Err(Error::Storage("auxiliary settlement lost"))
            } else {
                Ok(())
            }
        })
    }
}

fn setup(
    tool: bool,
    oversized: bool,
    fail_finish: bool,
) -> (
    Agent,
    Arc<AtomicUsize>,
    Arc<Mutex<Vec<InvocationContext>>>,
    Journal,
) {
    let calls = Arc::new(AtomicUsize::new(0));
    let contexts = Arc::new(Mutex::new(Vec::new()));
    let provider = Arc::new(Fixture {
        calls: calls.clone(),
        contexts: contexts.clone(),
        tool,
        oversized,
    });
    let agent = Agent::builder(provider, AgentOptions::new("my-account", "aux-model"))
        .build()
        .unwrap();
    let journal = Journal {
        allowed: AtomicBool::new(true),
        began: AtomicBool::new(false),
        finished: Mutex::new(Vec::new()),
        fail_finish,
    };
    (agent, calls, contexts, journal)
}

fn selected(agent: &Agent) -> ProviderSelection {
    agent
        .provider_accounts()
        .select(agent.actor(), "my-account", "aux-model")
        .unwrap()
}

fn input(limit: usize) -> AuxiliaryInput {
    AuxiliaryInput {
        kind: AuxiliaryKind::Side,
        messages: vec![
            Message::text(1, Role::System, "context"),
            Message::text(2, Role::User, "side question"),
        ],
        max_output_bytes: limit,
        timeout: Duration::from_secs(5),
    }
}

#[tokio::test]
async fn auxiliary_is_tool_free_journaled_and_never_enters_chat_history() {
    let (agent, calls, contexts, journal) = setup(false, false, false);
    let result = AuxiliaryExecutor
        .execute(&selected(&agent), &input(4096), &journal, pending())
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::Completed);
    assert_eq!(result.text, "auxiliary answer");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(agent.turns().is_empty());
    assert!(matches!(
        contexts.lock().unwrap()[0].purpose,
        InvocationPurpose::Auxiliary { .. }
    ));
    assert!(journal.began.load(Ordering::SeqCst));
    assert_eq!(journal.finished.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn cancellation_before_start_is_not_a_paid_auxiliary_attempt() {
    let (agent, calls, _, journal) = setup(false, false, false);
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(AuxiliaryExecutor
        .execute(
            &selected(&agent),
            &input(4096),
            &journal,
            cancel.cancelled()
        )
        .await
        .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(!journal.began.load(Ordering::SeqCst));
    assert!(journal.finished.lock().unwrap().is_empty());
}

#[tokio::test]
async fn denied_auxiliary_never_starts_or_polls_provider() {
    let (agent, calls, _, journal) = setup(false, false, false);
    journal.allowed.store(false, Ordering::SeqCst);
    assert!(AuxiliaryExecutor
        .execute(&selected(&agent), &input(4096), &journal, pending())
        .await
        .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(!journal.began.load(Ordering::SeqCst));
}

#[tokio::test]
async fn proposed_tool_is_never_accepted_as_auxiliary_answer() {
    let (agent, _, _, journal) = setup(true, false, false);
    let result = AuxiliaryExecutor
        .execute(&selected(&agent), &input(4096), &journal, pending())
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::Incomplete);
    assert!(result.text.is_empty());
}

#[tokio::test]
async fn output_limit_and_settlement_failure_never_publish_success() {
    let (agent, calls, _, journal) = setup(false, true, false);
    let result = AuxiliaryExecutor
        .execute(&selected(&agent), &input(16), &journal, pending())
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::Failed);
    assert!(result.text.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let (agent, calls, _, journal) = setup(false, false, true);
    assert!(AuxiliaryExecutor
        .execute(&selected(&agent), &input(4096), &journal, pending())
        .await
        .is_err());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "settlement failure must not cause automatic replay"
    );
}
