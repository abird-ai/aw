use aw_agent_core::*;
use serde_json::{json, Value};
use std::{
    future::pending,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
struct Script {
    calls: Vec<ToolCall>,
    requests: Arc<Mutex<Vec<Vec<Message>>>>,
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
        Box::pin(async { Err(Error::Protocol("wrong adapter path")) })
    }
    fn stream_tools<'a>(
        &'a self,
        m: &'a [Message],
        _: &'a str,
        _: &'a [ToolSpec],
        s: &'a mut DeltaSink<'_>,
    ) -> ModelFuture<'a> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(m.to_vec());
            if m.last().is_some_and(|m| m.role == Role::Tool) {
                s("done")?;
                Ok(ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "stop".into(),
                        usage: Usage {
                            input_tokens: Some(3),
                            output_tokens: Some(2),
                        },
                    },
                    ..Default::default()
                })
            } else {
                Ok(ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "tool_calls".into(),
                        ..Default::default()
                    },
                    tool_calls: self.calls.clone(),
                    provider_context: None,
                })
            }
        })
    }
}
struct Counter {
    count: Arc<AtomicUsize>,
    effect: ToolEffect,
    fail: bool,
    slow: bool,
}
impl Tool for Counter {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "count".into(),
            description: "test-only counter".into(),
            parameters: json!({"type":"object","properties":{"value":{"type":"integer"}},"required":["value"],"additionalProperties":false}),
            effect: self.effect.clone(),
        }
    }
    fn validate(&self, v: &Value) -> Result<()> {
        if v.as_object().is_none_or(|o| o.len() != 1)
            || v.get("value").and_then(Value::as_i64).is_none()
        {
            Err(Error::Denied("expected value integer"))
        } else {
            Ok(())
        }
    }
    fn execute<'a>(&'a self, _: ToolContext, _: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.count.fetch_add(1, Ordering::SeqCst);
            if self.slow {
                return pending().await;
            }
            Ok(if self.fail {
                ToolOutput::error("fixture failure")
            } else {
                ToolOutput::text("42")
            })
        })
    }
}
type SeenRequests = Arc<Mutex<Vec<Vec<Message>>>>;
fn call(id: &str, name: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        arguments: json!({"value":1}),
    }
}
fn setup(
    calls: Vec<ToolCall>,
    effect: ToolEffect,
    fail: bool,
    slow: bool,
) -> (Agent, Arc<AtomicUsize>, SeenRequests) {
    let count = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut policy = ToolPolicy::default();
    policy
        .permit_host(PrincipalId::new("local").unwrap(), "count", &effect)
        .unwrap();
    if slow {
        policy.timeout = Duration::from_millis(5);
    }
    let a = Agent::builder(
        Arc::new(Script {
            calls,
            requests: requests.clone(),
        }),
        AgentOptions::new("p", "m"),
    )
    .tool(Counter {
        count: count.clone(),
        effect,
        fail,
        slow,
    })
    .tool_policy(policy)
    .build()
    .unwrap();
    (a, count, requests)
}
#[tokio::test]
async fn complete_loop_records_tools_and_each_model_invocation() {
    let (mut a, n, requests) = setup(vec![call("c1", "count")], ToolEffect::Read, false, false);
    let t = a.prompt("hello", pending(), |_| Ok(())).await.unwrap();
    assert_eq!(t.outcome, Outcome::Completed);
    assert_eq!(n.load(Ordering::SeqCst), 1);
    assert_eq!(t.invocations.len(), 2);
    let r = requests.lock().unwrap();
    assert_eq!(r[1][1].tool_calls.len(), 1);
    assert_eq!(r[1][2].role, Role::Tool);
    assert_eq!(r[1][2].tool_call_id.as_deref(), Some("c1"));
}
#[tokio::test]
async fn unknown_tool_never_executes() {
    let (mut a, n, _) = setup(
        vec![call("c1", "not_allowed")],
        ToolEffect::Read,
        false,
        false,
    );
    let t = a.prompt("hello", pending(), |_| Ok(())).await.unwrap();
    assert_eq!(t.outcome, Outcome::Completed);
    assert_eq!(t.tool_records.len(), 1);
    assert_eq!(t.tool_records[0].status, ToolStatus::Denied);
    assert_eq!(t.tool_records[0].call.id, "c1");
    assert_eq!(n.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn validate_entire_batch_before_first_effect() {
    let (mut a, n, seen) = setup(
        vec![call("c1", "count"), call("c2", "not_allowed")],
        ToolEffect::Write,
        false,
        false,
    );
    let turn = a.prompt("hello", pending(), |_| Ok(())).await.unwrap();
    assert_eq!(turn.outcome, Outcome::Completed);
    assert_eq!(n.load(Ordering::SeqCst), 0);
    assert_eq!(turn.tool_records.len(), 2);
    assert_eq!(turn.invocations.len(), 2);
    let records = &turn.tool_records;
    assert_eq!(records[0].call.id, "c1");
    assert_eq!(records[1].call.id, "c2");
    assert!(records
        .iter()
        .all(|r| r.status == ToolStatus::Denied && r.output.as_ref().is_some_and(|o| o.is_error)));
    // The model must receive a complete, ID-correlated ToolResult for each
    // original proposal, including the otherwise permitted sibling.
    let requests = seen.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let second = &requests[1];
    assert_eq!(second.iter().filter(|m| m.role == Role::Tool).count(), 2);
    for id in ["c1", "c2"] {
        let msg = second
            .iter()
            .find(|m| m.role == Role::Tool && m.tool_call_id.as_deref() == Some(id))
            .unwrap();
        assert!(msg.content.starts_with("ERROR:"));
    }
    assert_eq!(
        second
            .iter()
            .find(|m| m.role == Role::Assistant && m.tool_calls.len() == 2)
            .unwrap()
            .tool_calls[0]
            .id,
        "c1"
    );
}
#[tokio::test]
async fn invalid_argument_blocks_before_execute() {
    let mut c = call("c1", "count");
    c.arguments = json!({"value":"bad"});
    let (mut a, n, _) = setup(vec![c], ToolEffect::Write, false, false);
    a.prompt("hello", pending(), |_| Ok(())).await.unwrap();
    assert_eq!(n.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn duplicate_ids_block_whole_batch() {
    let (mut a, n, _) = setup(
        vec![call("same", "count"), call("same", "count")],
        ToolEffect::Write,
        false,
        false,
    );
    a.prompt("hello", pending(), |_| Ok(())).await.unwrap();
    assert_eq!(n.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn tool_error_is_paired_then_model_can_explain() {
    let (mut a, n, r) = setup(vec![call("c", "count")], ToolEffect::Read, true, false);
    let t = a.prompt("hello", pending(), |_| Ok(())).await.unwrap();
    assert_eq!(n.load(Ordering::SeqCst), 1);
    assert_eq!(t.outcome, Outcome::Completed);
    assert!(r.lock().unwrap()[1][2].content.starts_with("ERROR:"));
}
#[tokio::test]
async fn interrupted_write_requires_inspection_even_after_reset() {
    let (mut a, n, _) = setup(vec![call("c", "count")], ToolEffect::Write, false, true);
    let t = a.prompt("hello", pending(), |_| Ok(())).await.unwrap();
    assert_eq!(n.load(Ordering::SeqCst), 1);
    assert!(t.needs_reconciliation);
    assert!(a.retryable_prompt().is_none());
    a.reset().unwrap();
    assert!(a.needs_reconciliation());
    assert!(a.prompt("new", pending(), |_| Ok(())).await.is_err());
    a.acknowledge_interrupted_tools().unwrap();
    assert!(!a.needs_reconciliation());
}
#[test]
fn orphan_tool_result_context_is_rejected() {
    let mut m = Message::text(1, Role::Tool, "unpaired");
    m.tool_call_id = Some("not-present".into());
    assert!(validate_messages(&[m]).is_err());
}
#[test]
fn duplicate_tool_results_rejected() {
    let mut a = Message::text(1, Role::Assistant, "");
    a.tool_calls = vec![call("x", "count")];
    let mut b = Message::text(2, Role::Tool, "result");
    b.tool_call_id = Some("x".into());
    assert!(validate_messages(&[a, b.clone(), b]).is_err());
}
#[test]
fn incomplete_call_in_context_is_rejected() {
    let mut a = Message::text(1, Role::Assistant, "");
    a.tool_calls = vec![call("x", "count")];
    assert!(validate_messages(&[a]).is_err());
}
struct DenyTool;
impl Extension for DenyTool {
    fn name(&self) -> &str {
        "deny_tool"
    }
    fn on_hook<'a>(&'a self, _: HookContext<'a>, event: HookEvent) -> HookFuture<'a> {
        Box::pin(async move {
            if matches!(event, HookEvent::ToolCall { .. }) {
                Err(Error::Denied("test denial"))
            } else {
                Ok(HookFlow::Next(Some(event)))
            }
        })
    }
}
#[tokio::test]
async fn extension_gate_can_deny_before_effect() {
    let count = Arc::new(AtomicUsize::new(0));
    let mut policy = ToolPolicy::default();
    policy
        .permit_host(
            PrincipalId::new("local").unwrap(),
            "count",
            &ToolEffect::Write,
        )
        .unwrap();
    let mut a = Agent::builder(
        Arc::new(Script {
            calls: vec![call("x", "count")],
            requests: Arc::new(Mutex::new(Vec::new())),
        }),
        AgentOptions::new("p", "m"),
    )
    .tool(Counter {
        count: count.clone(),
        effect: ToolEffect::Write,
        fail: false,
        slow: false,
    })
    .tool_policy(policy)
    .extension(DenyTool)
    .build()
    .unwrap();
    assert_eq!(
        a.prompt("hello", pending(), |_| Ok(()))
            .await
            .unwrap()
            .outcome,
        Outcome::Completed
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn no_default_tool_grant() {
    let count = Arc::new(AtomicUsize::new(0));
    let mut a = Agent::builder(
        Arc::new(Script {
            calls: vec![call("x", "count")],
            requests: Arc::new(Mutex::new(Vec::new())),
        }),
        AgentOptions::new("p", "m"),
    )
    .tool(Counter {
        count: count.clone(),
        effect: ToolEffect::Read,
        fail: false,
        slow: false,
    })
    .build()
    .unwrap();
    a.prompt("hello", pending(), |_| Ok(())).await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn tool_result_cannot_smuggle_nested_tool_calls() {
    let mut a = Message::text(1, Role::Assistant, "");
    a.tool_calls = vec![call("x", "count")];
    let mut b = Message::text(2, Role::Tool, "result");
    b.tool_call_id = Some("x".into());
    b.tool_calls = vec![call("y", "count")];
    assert!(validate_messages(&[a, b]).is_err());
}
