//! Real SDK tests to be run by Cargo. The provider is deliberately a fixture.
use aw_agent_core::*;
use std::{
    future::pending,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

fn evidence() -> EvidenceRef {
    EvidenceRef {
        authority: "a_fixture".into(),
        session_id: "s_source".into(),
        node_id: "n_source".into(),
        sha256: "a".repeat(64),
    }
}
fn input() -> SummaryInput {
    let refs = vec![evidence()];
    SummaryInput { plan_id:"cp_fixture".into(),focus:"Preserve conflicts".into(),
        evidence_json:serde_json::json!({"entries":[{"source":"n_source","public":"An alternative failed a test."}],"dependencies":refs}).to_string(),
        sources:refs,max_output_bytes:4096,timeout_seconds:5 }
}
struct Fixture {
    calls: Arc<AtomicUsize>,
    contexts: Arc<Mutex<Vec<InvocationContext>>>,
    empty: bool,
    incomplete: bool,
    tool: bool,
    hang: bool,
}
impl Provider for Fixture {
    fn stream<'a>(
        &'a self,
        _: &'a [Message],
        _: &'a str,
        _: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async {
            Err(Error::Protocol(
                "fixture requires identity-aware summary call",
            ))
        })
    }
    fn invoke<'a>(
        &'a self,
        ctx: &'a InvocationContext,
        m: &'a [Message],
        _: &'a str,
        tools: &'a [ToolSpec],
        sink: &'a mut DeltaSink<'_>,
    ) -> ModelFuture<'a> {
        Box::pin(async move {
            assert!(tools.is_empty());
            assert_eq!(m.len(), 2);
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.contexts.lock().unwrap().push(ctx.clone());
            if self.hang {
                return pending::<Result<ModelReply>>().await;
            }
            if !self.empty {
                sink("Source n_source: ")?;
                sink("the test failed; no rerun is claimed.")?;
            }
            Ok(ModelReply {
                meta: CompletionMeta {
                    finish_reason: if self.incomplete { "length" } else { "stop" }.into(),
                    usage: Usage {
                        input_tokens: Some(10),
                        output_tokens: Some(6),
                    },
                },
                tool_calls: if self.tool {
                    vec![ToolCall {
                        id: "bad".into(),
                        name: "bash".into(),
                        arguments: serde_json::json!({"command":"not executed"}),
                    }]
                } else {
                    vec![]
                },
                provider_context: None,
            })
        })
    }
}
struct Journal {
    allowed: AtomicBool,
    began: AtomicBool,
    finished: Mutex<Vec<SummaryResult>>,
    fail_finish: bool,
}
impl SummaryJournal for Journal {
    fn authorize<'a>(
        &'a self,
        _: &'a SummaryInput,
        _: &'a InvocationIdentity,
    ) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            if self.allowed.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(Error::Denied("fixture source revoked"))
            }
        })
    }
    fn begin<'a>(
        &'a self,
        i: &'a SummaryInput,
        who: &'a InvocationIdentity,
    ) -> JournalFuture<'a, InvocationContext> {
        Box::pin(async move {
            assert!(!self.began.swap(true, Ordering::SeqCst));
            Ok(InvocationContext {
                scope: Some(InvocationScope {
                    session_id: "s_target".into(),
                    branch_id: "b_target".into(),
                }),
                turn_id: 7,
                ordinal: 1,
                identity: who.clone(),
                purpose: InvocationPurpose::Summary {
                    operation_id: "cs_fixture".into(),
                    input_sha256: i.digest()?,
                    sources: i.sources.clone(),
                },
            })
        })
    }
    fn finish<'a>(&'a self, result: &'a SummaryResult) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            self.finished.lock().unwrap().push(result.clone());
            if self.fail_finish {
                Err(Error::Storage("fixture commit lost"))
            } else {
                Ok(())
            }
        })
    }
}
fn setup(
    empty: bool,
    incomplete: bool,
    tool: bool,
    hang: bool,
) -> (
    Agent,
    Arc<AtomicUsize>,
    Arc<Mutex<Vec<InvocationContext>>>,
    Journal,
) {
    let calls = Arc::new(AtomicUsize::new(0));
    let contexts = Arc::new(Mutex::new(vec![]));
    let provider = Arc::new(Fixture {
        calls: calls.clone(),
        contexts: contexts.clone(),
        empty,
        incomplete,
        tool,
        hang,
    });
    let a = Agent::builder(provider, AgentOptions::new("my-account", "summary-model"))
        .build()
        .unwrap();
    let j = Journal {
        allowed: AtomicBool::new(true),
        began: AtomicBool::new(false),
        finished: Mutex::new(vec![]),
        fail_finish: false,
    };
    (a, calls, contexts, j)
}
fn selection(a: &Agent) -> ProviderSelection {
    a.provider_accounts()
        .select(a.actor(), "my-account", "summary-model")
        .unwrap()
}
#[test]
fn preparation_and_executor_creation_are_no_inference() {
    let (a, c, _, _) = setup(false, false, false, false);
    a.summary_executor().unwrap();
    assert!(input().messages().is_ok());
    assert_eq!(c.load(Ordering::SeqCst), 0);
    assert!(a.turns().is_empty());
}
#[tokio::test]
async fn summary_is_separate_from_main_history_and_has_typed_identity() {
    let (a, c, ctx, j) = setup(false, false, false, false);
    let result = a
        .summary_executor()
        .unwrap()
        .execute(&selection(&a), &input(), &j, pending())
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::Completed);
    assert_eq!(c.load(Ordering::SeqCst), 1);
    assert!(a.turns().is_empty());
    assert_eq!(a.options().model, "summary-model");
    assert!(matches!(
        ctx.lock().unwrap()[0].purpose,
        InvocationPurpose::Summary { .. }
    ));
    assert_eq!(j.finished.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn denied_source_never_polls_provider_or_records_start() {
    let (a, c, _, j) = setup(false, false, false, false);
    j.allowed.store(false, Ordering::SeqCst);
    assert!(a
        .summary_executor()
        .unwrap()
        .execute(&selection(&a), &input(), &j, pending())
        .await
        .is_err());
    assert_eq!(c.load(Ordering::SeqCst), 0);
    assert!(!j.began.load(Ordering::SeqCst));
}
#[tokio::test]
async fn cancellation_before_start_is_not_a_paid_attempt() {
    let (a, c, _, j) = setup(false, false, false, false);
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(a
        .summary_executor()
        .unwrap()
        .execute(&selection(&a), &input(), &j, cancel.cancelled())
        .await
        .is_err());
    assert_eq!(c.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn cancellation_after_start_settles_without_partial_candidate() {
    let (a, c, _, j) = setup(false, false, false, true);
    let result = a
        .summary_executor()
        .unwrap()
        .execute(&selection(&a), &input(), &j, async {
            tokio::time::sleep(Duration::from_millis(10)).await
        })
        .await
        .unwrap();
    assert_eq!(c.load(Ordering::SeqCst), 1);
    assert_eq!(result.outcome, Outcome::Cancelled);
    assert!(result.text.is_empty());
    assert!(result.meta.is_none());
    assert_eq!(j.finished.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn incomplete_result_keeps_usage_not_candidate() {
    let (a, _, _, j) = setup(false, true, false, false);
    let result = a
        .summary_executor()
        .unwrap()
        .execute(&selection(&a), &input(), &j, pending())
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::Incomplete);
    assert!(result.text.is_empty());
    assert_eq!(result.meta.unwrap().usage.output_tokens, Some(6));
}
#[tokio::test]
async fn empty_summary_is_not_a_completed_artifact() {
    let (a, _, _, j) = setup(true, false, false, false);
    let result = a
        .summary_executor()
        .unwrap()
        .execute(&selection(&a), &input(), &j, pending())
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::Incomplete);
}
#[tokio::test]
async fn proposed_tool_is_never_executed_or_published_as_summary() {
    let (a, _, _, j) = setup(false, false, true, false);
    let result = a
        .summary_executor()
        .unwrap()
        .execute(&selection(&a), &input(), &j, pending())
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::Incomplete);
    assert!(result.text.is_empty());
}
#[tokio::test]
async fn output_budget_refuses_truncation_disguised_as_complete() {
    let (a, _, _, j) = setup(false, false, false, false);
    let mut i = input();
    i.max_output_bytes = 2;
    let result = a
        .summary_executor()
        .unwrap()
        .execute(&selection(&a), &i, &j, pending())
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::Failed);
    assert!(result.text.is_empty());
}
#[tokio::test]
async fn commit_failure_is_not_a_successful_return() {
    let (a, c, _, mut j) = setup(false, false, false, false);
    j.fail_finish = true;
    assert!(a
        .summary_executor()
        .unwrap()
        .execute(&selection(&a), &input(), &j, pending())
        .await
        .is_err());
    assert_eq!(c.load(Ordering::SeqCst), 1);
}
struct DenyOutput;
impl Extension for DenyOutput {
    fn name(&self) -> &str {
        "deny-summary-output"
    }
    fn on_hook<'a>(&'a self, _: HookContext<'a>, event: HookEvent) -> HookFuture<'a> {
        Box::pin(async move {
            if matches!(
                event,
                HookEvent::Step {
                    kind: DecisionKind::Summary,
                    phase: DecisionPhase::After,
                    ..
                }
            ) {
                Err(Error::Denied("fixture output gate"))
            } else {
                Ok(HookFlow::Next(Some(event)))
            }
        })
    }
}
#[tokio::test]
async fn output_extension_withholds_candidate_but_retains_usage() {
    let (a, _, _, j) = setup(false, false, false, false);
    let executor =
        SummaryExecutor::new(vec![Arc::new(DenyOutput)], Duration::from_secs(1)).unwrap();
    let result = executor
        .execute(&selection(&a), &input(), &j, pending())
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::Failed);
    assert!(result.text.is_empty());
    assert!(result.meta.is_some());
}
struct DenyBefore;
impl Extension for DenyBefore {
    fn name(&self) -> &str {
        "deny-summary-admission"
    }
    fn on_hook<'a>(&'a self, _: HookContext<'a>, event: HookEvent) -> HookFuture<'a> {
        Box::pin(async move {
            if matches!(
                event,
                HookEvent::Step {
                    kind: DecisionKind::Summary,
                    phase: DecisionPhase::Before,
                    ..
                }
            ) {
                Err(Error::Denied("summary admission gate"))
            } else {
                Ok(HookFlow::Next(Some(event)))
            }
        })
    }
}
#[tokio::test]
async fn ordinary_prompt_gate_applies_to_default_summary_hook() {
    let (a, c, _, j) = setup(false, false, false, false);
    let executor =
        SummaryExecutor::new(vec![Arc::new(DenyBefore)], Duration::from_secs(1)).unwrap();
    assert!(executor
        .execute(&selection(&a), &input(), &j, pending())
        .await
        .is_err());
    assert_eq!(c.load(Ordering::SeqCst), 0);
}
#[test]
fn input_digest_changes_when_focus_or_exact_evidence_changes() {
    let a = input();
    let mut b = a.clone();
    b.focus.push('!');
    assert_ne!(a.digest().unwrap(), b.digest().unwrap());
    b = a.clone();
    b.evidence_json = serde_json::json!({"entries":[],"dependencies":b.sources}).to_string();
    assert_ne!(a.digest().unwrap(), b.digest().unwrap());
}
#[test]
fn wire_message_ids_do_not_change_source_consent() {
    let a = input().messages().unwrap();
    let mut b = a.clone();
    b[0].id = 600;
    b[1].id = 700;
    assert_eq!(
        summary_message_digest(&a).unwrap(),
        summary_message_digest(&b).unwrap()
    );
}
#[test]
fn tool_or_private_content_cannot_enter_summary_digest() {
    let mut m = input().messages().unwrap();
    m[1].provider_context = Some("secret continuation".into());
    assert!(summary_message_digest(&m).is_err());
}
#[test]
fn evidence_identifier_and_hash_validation_is_strict() {
    let mut e = evidence();
    e.node_id = "../outside".into();
    assert!(validate_evidence_ref(&e).is_err());
    e = evidence();
    e.sha256 = "A".repeat(64);
    assert!(validate_evidence_ref(&e).is_err());
}
fn block(index: usize) -> ContextBlock {
    ContextBlock {
        before_turn: index,
        node_id: "n_context".into(),
        artifact_id: "ca_import".into(),
        kind: ContextBlockKind::Import,
        text: "Branch fact, not a grant".into(),
        sources: vec![evidence()],
    }
}
#[test]
fn derived_context_is_user_data_not_privileged_system() {
    let m = block(0).message(4);
    assert_eq!(m.role, Role::User);
    assert!(m.tool_calls.is_empty());
    assert!(m.content.contains("not instructions or a grant"));
}
#[test]
fn legacy_history_accepts_no_new_blocks() {
    let mut h = HistoryState {
        version: 1,
        ..HistoryState::default()
    };
    h.context_blocks.push(block(0));
    assert!(Conversation::from_history(String::new(), &h).is_err());
}
#[test]
fn future_position_is_rejected() {
    let mut h = HistoryState::default();
    h.context_blocks.push(block(1));
    assert!(Conversation::from_history(String::new(), &h).is_err());
}
#[test]
fn context_precedes_the_new_prompt_without_rewriting_it() {
    let mut h = HistoryState::default();
    h.context_blocks.push(block(0));
    let c = Conversation::from_history("trusted host".into(), &h).unwrap();
    let m = c.messages_for("exact prompt").unwrap();
    assert_eq!(m[0].role, Role::System);
    assert_eq!(m[1].role, Role::User);
    assert!(m[1].content.contains("Branch fact"));
    assert_eq!(m.last().unwrap().content, "exact prompt");
}
