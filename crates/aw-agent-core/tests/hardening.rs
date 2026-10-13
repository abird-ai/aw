//! Regression source for the Phase 11 boundary review. Run with Cargo; these
//! fixtures are not a substitute for actual Rust compilation or OS isolation.
use aw_agent_core::*;
use std::{
    future::pending,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

fn meta() -> CompletionMeta {
    CompletionMeta {
        finish_reason: "stop".into(),
        usage: Usage {
            input_tokens: Some(4),
            output_tokens: Some(3),
        },
    }
}

struct IgnoringSink {
    calls: Arc<AtomicUsize>,
    fragments: Vec<&'static str>,
}
impl Provider for IgnoringSink {
    fn stream<'a>(
        &'a self,
        _: &'a [Message],
        _: &'a str,
        sink: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            // Deliberately violate the Provider contract. The core must keep a
            // sticky rejection instead of accepting a later success response.
            for fragment in &self.fragments {
                let _ignored = sink(fragment);
            }
            Ok(meta())
        })
    }
}
fn agent(limit: usize) -> (Agent, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut options = AgentOptions::new("model-account", "fixture-model");
    options.max_output_bytes = limit;
    let agent = Agent::builder(
        Arc::new(IgnoringSink {
            calls: calls.clone(),
            fragments: vec!["safe", " too long", "tail"],
        }),
        options,
    )
    .build()
    .unwrap();
    (agent, calls)
}
fn selection(agent: &Agent) -> ProviderSelection {
    agent
        .provider_accounts()
        .select(agent.actor(), "model-account", "fixture-model")
        .unwrap()
}
fn input() -> SummaryInput {
    let evidence = EvidenceRef {
        authority: "a_fixture".into(),
        session_id: "s_source".into(),
        node_id: "n_source".into(),
        sha256: "a".repeat(64),
    };
    SummaryInput {
        plan_id: "cp_fixture".into(),
        focus: "Summarize evidence".into(),
        evidence_json: serde_json::json!({"evidence":"fixture", "dependencies":[evidence.clone()]})
            .to_string(),
        sources: vec![evidence],
        max_output_bytes: 4096,
        timeout_seconds: 5,
    }
}

struct Journal {
    allowed: Arc<AtomicBool>,
    started: AtomicBool,
    settled: Mutex<Vec<SummaryResult>>,
    deny_at_begin: bool,
    revoke_at_begin: Option<ProviderAccounts>,
    wrong_identity: bool,
    wrong_purpose: bool,
    hang_authorize: bool,
}
impl Default for Journal {
    fn default() -> Self {
        Self {
            allowed: Arc::new(AtomicBool::new(true)),
            started: AtomicBool::new(false),
            settled: Mutex::new(Vec::new()),
            deny_at_begin: false,
            revoke_at_begin: None,
            wrong_identity: false,
            wrong_purpose: false,
            hang_authorize: false,
        }
    }
}
impl SummaryJournal for Journal {
    fn authorize<'a>(
        &'a self,
        _: &'a SummaryInput,
        _: &'a InvocationIdentity,
    ) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            if self.hang_authorize {
                return pending::<Result<()>>().await;
            }
            if self.allowed.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(Error::Denied("fixture source revoked"))
            }
        })
    }
    fn begin<'a>(
        &'a self,
        input: &'a SummaryInput,
        identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, InvocationContext> {
        Box::pin(async move {
            assert!(!self.started.swap(true, Ordering::SeqCst));
            if self.deny_at_begin {
                self.allowed.store(false, Ordering::SeqCst);
            }
            if let Some(accounts) = &self.revoke_at_begin {
                accounts.revoke(&identity.account_id)?;
            }
            let mut actor = identity.clone();
            if self.wrong_identity {
                actor.model = "not-the-selected-model".into();
            }
            Ok(InvocationContext {
                scope: Some(InvocationScope {
                    session_id: "s_target".into(),
                    branch_id: "b_target".into(),
                }),
                turn_id: 1,
                ordinal: 1,
                identity: actor,
                purpose: if self.wrong_purpose {
                    InvocationPurpose::Chat
                } else {
                    InvocationPurpose::Summary {
                        operation_id: "op_fixture".into(),
                        input_sha256: input.digest()?,
                        sources: input.sources.clone(),
                    }
                },
            })
        })
    }
    fn finish<'a>(&'a self, result: &'a SummaryResult) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            self.settled.lock().unwrap().push(result.clone());
            Ok(())
        })
    }
}

#[tokio::test]
async fn ignored_output_limit_cannot_complete_chat_or_reenter_model_context() {
    let (mut a, calls) = agent(4);
    let turn = a
        .prompt("fixture", pending::<()>(), |_| Ok(()))
        .await
        .unwrap();
    assert_eq!(turn.outcome, Outcome::Failed);
    assert_eq!(turn.answer, "safe");
    assert_eq!(turn.invocations[0].outcome, "stream_rejected");
    assert_eq!(
        turn.invocations[0]
            .meta
            .as_ref()
            .unwrap()
            .usage
            .output_tokens,
        Some(3)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let state = a.history_state();
    let conversation = Conversation::from_history(String::new(), &state).unwrap();
    assert!(!conversation
        .messages_for("new")
        .unwrap()
        .iter()
        .any(|m| m.role == Role::Assistant));
}

#[tokio::test]
async fn ignored_observer_rejection_is_sticky_and_no_more_deltas_are_delivered() {
    let (mut a, _) = agent(4096);
    let mut observations = 0;
    let turn = a
        .prompt("fixture", pending::<()>(), |event| {
            if matches!(event, AgentEvent::Delta { .. }) {
                observations += 1;
                return Err(Error::Denied("observer rejected output"));
            }
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(observations, 1);
    assert_eq!(turn.outcome, Outcome::Failed);
    assert_eq!(turn.invocations.len(), 1);
}

#[tokio::test]
async fn ignored_summary_limit_keeps_usage_but_never_a_partial_candidate() {
    let (a, calls) = agent(4096);
    let mut source = input();
    source.max_output_bytes = 4;
    let journal = Journal::default();
    let result = a
        .summary_executor()
        .unwrap()
        .execute(&selection(&a), &source, &journal, pending::<()>())
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::Failed);
    assert!(result.text.is_empty());
    assert_eq!(result.meta.unwrap().usage.output_tokens, Some(3));
    assert_eq!(journal.settled.lock().unwrap().len(), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn source_revoked_during_begin_is_rechecked_before_provider_poll() {
    let (a, calls) = agent(4096);
    let journal = Journal {
        deny_at_begin: true,
        ..Journal::default()
    };
    let result = a
        .summary_executor()
        .unwrap()
        .execute(&selection(&a), &input(), &journal, pending::<()>())
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::Failed);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(journal.started.load(Ordering::SeqCst));
    assert_eq!(journal.settled.lock().unwrap().len(), 1);
    assert!(result.meta.is_none());
}

#[tokio::test]
async fn account_revoked_during_begin_does_not_dispatch_or_fallback() {
    let (a, calls) = agent(4096);
    let journal = Journal {
        revoke_at_begin: Some(a.provider_accounts()),
        ..Journal::default()
    };
    let result = a
        .summary_executor()
        .unwrap()
        .execute(&selection(&a), &input(), &journal, pending::<()>())
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::Failed);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(journal.settled.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn mismatched_journal_identity_is_not_used_for_inference() {
    let (a, calls) = agent(4096);
    let journal = Journal {
        wrong_identity: true,
        ..Journal::default()
    };
    let result = a
        .summary_executor()
        .unwrap()
        .execute(&selection(&a), &input(), &journal, pending::<()>())
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::Failed);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn summary_cannot_be_smuggled_through_a_chat_purpose() {
    let (a, calls) = agent(4096);
    let journal = Journal {
        wrong_purpose: true,
        ..Journal::default()
    };
    let result = a
        .summary_executor()
        .unwrap()
        .execute(&selection(&a), &input(), &journal, pending::<()>())
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::Failed);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn initial_authorization_can_be_cancelled_without_a_start_intent() {
    let (a, calls) = agent(4096);
    let journal = Journal {
        hang_authorize: true,
        ..Journal::default()
    };
    let source = input();
    let selected = selection(&a);
    let executor = a.summary_executor().unwrap();
    let result = tokio::time::timeout(
        Duration::from_millis(500),
        executor.execute(&selected, &source, &journal, async {
            tokio::time::sleep(Duration::from_millis(10)).await
        }),
    )
    .await
    .expect("initial source gate did not observe cancellation");
    assert!(matches!(result, Err(Error::Cancelled)));
    assert!(!journal.started.load(Ordering::SeqCst));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

struct RevokeAfter {
    accounts: Option<ProviderAccounts>,
    allowed: Arc<AtomicBool>,
}
impl Extension for RevokeAfter {
    fn name(&self) -> &str {
        "revoke-after-summary"
    }
    fn on_hook<'a>(&'a self, context: HookContext<'a>, event: HookEvent) -> HookFuture<'a> {
        Box::pin(async move {
            if matches!(
                event,
                HookEvent::Step {
                    kind: DecisionKind::Summary,
                    phase: DecisionPhase::After,
                    ..
                }
            ) {
                if let Some(accounts) = &self.accounts {
                    accounts.revoke(&context.identity.account_id)?;
                }
                self.allowed.store(false, Ordering::SeqCst);
            }
            Ok(HookFlow::Next(Some(event)))
        })
    }
}
#[tokio::test]
async fn source_changed_during_output_hook_withholds_candidate() {
    let (a, _) = agent(4096);
    let journal = Journal::default();
    let executor = SummaryExecutor::new(
        vec![Arc::new(RevokeAfter {
            accounts: None,
            allowed: journal.allowed.clone(),
        })],
        Duration::from_secs(1),
    )
    .unwrap();
    let result = executor
        .execute(&selection(&a), &input(), &journal, pending::<()>())
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::Failed);
    assert!(result.text.is_empty());
    assert!(result.meta.is_some());
}
#[tokio::test]
async fn account_changed_during_output_hook_withholds_candidate() {
    let (a, _) = agent(4096);
    let journal = Journal::default();
    let executor = SummaryExecutor::new(
        vec![Arc::new(RevokeAfter {
            accounts: Some(a.provider_accounts()),
            allowed: Arc::new(AtomicBool::new(true)),
        })],
        Duration::from_secs(1),
    )
    .unwrap();
    let result = executor
        .execute(&selection(&a), &input(), &journal, pending::<()>())
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::Failed);
    assert!(result.text.is_empty());
    assert!(result.meta.is_some());
}

struct Named(&'static str);
impl Extension for Named {
    fn name(&self) -> &str {
        self.0
    }
}
#[test]
fn standalone_summary_extensions_obey_the_agent_registry_contract() {
    assert!(SummaryExecutor::new(vec![Arc::new(Named(""))], Duration::from_secs(1)).is_err());
    assert!(SummaryExecutor::new(
        vec![Arc::new(Named("same")), Arc::new(Named("same"))],
        Duration::from_secs(1)
    )
    .is_err());
    assert!(
        SummaryExecutor::new(vec![Arc::new(Named("../invalid"))], Duration::from_secs(1)).is_err()
    );
    assert!(
        SummaryExecutor::new(vec![Arc::new(Named("valid-hook"))], Duration::from_secs(1)).is_ok()
    );
}

struct TurnGate {
    allowed: AtomicBool,
    accounts: Option<ProviderAccounts>,
    revoke_source_at_model_intent: bool,
}
impl TurnJournal for TurnGate {
    fn validate_context(&self) -> JournalFuture<'_, ()> {
        Box::pin(async move {
            if self.allowed.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(Error::Denied("fixture source gate closed"))
            }
        })
    }
    fn begin<'a>(&'a self, _: TurnStart) -> JournalFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn append<'a>(&'a self, _: u64, entry: JournalEntry) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            if let JournalEntry::InvocationStarted { identity, .. } = entry {
                if let Some(accounts) = &self.accounts {
                    accounts.revoke(&identity.account_id)?;
                }
                if self.revoke_source_at_model_intent {
                    self.allowed.store(false, Ordering::SeqCst);
                }
            }
            Ok(())
        })
    }
    fn finish<'a>(&'a self, _: Turn) -> JournalFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn acknowledge<'a>(&'a self, _: String) -> JournalFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn chat_account_revoked_in_model_intent_never_enters_provider() {
    let (mut a, calls) = agent(4096);
    let journal = TurnGate {
        allowed: AtomicBool::new(true),
        accounts: Some(a.provider_accounts()),
        revoke_source_at_model_intent: false,
    };
    a.load_history(HistoryState::default(), Some(Arc::new(journal)))
        .unwrap();
    let turn = a
        .prompt("fixture", pending::<()>(), |_| Ok(()))
        .await
        .unwrap();
    assert_eq!(turn.outcome, Outcome::Failed);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(turn.invocations[0].meta.is_none());
}

#[tokio::test]
async fn chat_source_revoked_in_model_intent_never_enters_provider() {
    let (mut a, calls) = agent(4096);
    a.load_history(
        HistoryState::default(),
        Some(Arc::new(TurnGate {
            allowed: AtomicBool::new(true),
            accounts: None,
            revoke_source_at_model_intent: true,
        })),
    )
    .unwrap();
    let turn = a
        .prompt("fixture", pending::<()>(), |_| Ok(()))
        .await
        .unwrap();
    assert_eq!(turn.outcome, Outcome::Failed);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

struct ProposeTool;
impl Provider for ProposeTool {
    fn stream<'a>(
        &'a self,
        _: &'a [Message],
        _: &'a str,
        _: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async { Err(Error::Protocol("tool fixture requires stream_tools")) })
    }
    fn supports_tools(&self) -> bool {
        true
    }
    fn stream_tools<'a>(
        &'a self,
        _: &'a [Message],
        _: &'a str,
        _: &'a [ToolSpec],
        _: &'a mut DeltaSink<'_>,
    ) -> ModelFuture<'a> {
        Box::pin(async {
            Ok(ModelReply {
                meta: CompletionMeta {
                    finish_reason: "tool_calls".into(),
                    usage: Usage::default(),
                },
                tool_calls: vec![ToolCall {
                    id: "call_1".into(),
                    name: "count".into(),
                    arguments: serde_json::json!({}),
                }],
                provider_context: None,
            })
        })
    }
}
struct CountTool(Arc<AtomicUsize>);
impl Tool for CountTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "count".into(),
            description: "Counting fixture, no I/O".into(),
            effect: ToolEffect::Read,
            parameters: serde_json::json!({"type":"object"}),
        }
    }
    fn validate(&self, _: &serde_json::Value) -> Result<()> {
        Ok(())
    }
    fn execute<'a>(&'a self, _: ToolContext, _: &'a serde_json::Value) -> ToolFuture<'a> {
        // Count entry itself, not just future polling, so eager invocation fails.
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(ToolOutput::text("counted")) })
    }
}
#[tokio::test]
async fn account_revocation_by_started_observer_prevents_tool_entry() {
    let calls = Arc::new(AtomicUsize::new(0));
    let actor = PrincipalId::new("local").unwrap();
    let mut policy = ToolPolicy::default();
    policy
        .permit_host(actor.clone(), "count", &ToolEffect::Read)
        .unwrap();
    let descriptor = AccountDescriptor::personal(
        "model-account",
        actor.clone(),
        "local",
        ["fixture-model".to_owned()],
    );
    let mut a = Agent::builder(
        Arc::new(ProposeTool),
        AgentOptions::new("model-account", "fixture-model"),
    )
    .account(descriptor, actor, "local")
    .tool(CountTool(calls.clone()))
    .tool_policy(policy)
    .build()
    .unwrap();
    let accounts = a.provider_accounts();
    let turn = a
        .prompt("fixture", pending::<()>(), |event| {
            if matches!(event, AgentEvent::ToolStarted { .. }) {
                accounts.revoke("model-account")?;
            }
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(turn.outcome, Outcome::Failed);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(turn.tool_records[0].status, ToolStatus::Interrupted);
}
