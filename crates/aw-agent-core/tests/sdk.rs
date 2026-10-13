//! Black-box SDK consumer tests: no terminal, HTTP or web dependencies.
use aw_agent_core::*;
use std::{
    future::pending,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

struct Echo {
    calls: Arc<AtomicUsize>,
    contexts: Arc<Mutex<Vec<Vec<Message>>>>,
}
impl Provider for Echo {
    fn stream<'a>(
        &'a self,
        messages: &'a [Message],
        _: &'a str,
        sink: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.contexts.lock().unwrap().push(messages.to_vec());
            sink("answer")?;
            Ok(CompletionMeta {
                finish_reason: "stop".into(),
                ..Default::default()
            })
        })
    }
}
#[allow(clippy::type_complexity)]
fn fixture() -> (
    Arc<dyn Provider>,
    Arc<AtomicUsize>,
    Arc<Mutex<Vec<Vec<Message>>>>,
) {
    let calls = Arc::new(AtomicUsize::new(0));
    let contexts = Arc::new(Mutex::new(Vec::new()));
    (
        Arc::new(Echo {
            calls: calls.clone(),
            contexts: contexts.clone(),
        }),
        calls,
        contexts,
    )
}
struct Deny;
impl Extension for Deny {
    fn name(&self) -> &str {
        "deny"
    }
    fn on_hook<'a>(&'a self, _: HookContext<'a>, event: HookEvent) -> HookFuture<'a> {
        Box::pin(async move {
            if matches!(event, HookEvent::Prompt { .. }) {
                Err(Error::Denied("fixture denial"))
            } else {
                Ok(HookFlow::Next(Some(event)))
            }
        })
    }
}
struct AddContext;
impl Extension for AddContext {
    fn name(&self) -> &str {
        "context"
    }
    fn on_hook<'a>(&'a self, _: HookContext<'a>, event: HookEvent) -> HookFuture<'a> {
        Box::pin(async move {
            match event {
                HookEvent::Prompt {
                    input,
                    mut messages,
                } => {
                    messages.insert(0, Message::text(999, Role::System, "extra"));
                    Ok(HookFlow::Next(Some(HookEvent::Prompt { input, messages })))
                }
                other => Ok(HookFlow::Next(Some(other))),
            }
        })
    }
}
struct Rewrite;
impl Extension for Rewrite {
    fn name(&self) -> &str {
        "rewrite"
    }
    fn on_hook<'a>(&'a self, _: HookContext<'a>, event: HookEvent) -> HookFuture<'a> {
        Box::pin(async move {
            match event {
                HookEvent::Prompt {
                    input,
                    mut messages,
                } => {
                    messages.last_mut().unwrap().content = "different user words".into();
                    Ok(HookFlow::Next(Some(HookEvent::Prompt { input, messages })))
                }
                other => Ok(HookFlow::Next(Some(other))),
            }
        })
    }
}
struct Hanging;
impl Extension for Hanging {
    fn name(&self) -> &str {
        "hanging"
    }
    fn on_hook<'a>(&'a self, _: HookContext<'a>, event: HookEvent) -> HookFuture<'a> {
        Box::pin(async move {
            if matches!(event, HookEvent::Prompt { .. }) {
                pending::<Result<HookFlow<HookEvent>>>().await
            } else {
                Ok(HookFlow::Next(Some(event)))
            }
        })
    }
}
#[tokio::test]
async fn core_constructs_without_dispatch_or_observer() {
    let (p, c, _) = fixture();
    let a = Agent::builder(p, AgentOptions::new("test", "model"))
        .build()
        .unwrap();
    assert!(a.turns().is_empty());
    assert_eq!(c.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn prompt_emits_ordered_events_and_returns_completed_turn() {
    let (p, c, _) = fixture();
    let mut a = Agent::builder(p, AgentOptions::new("test", "model"))
        .build()
        .unwrap();
    let mut events = Vec::new();
    let t = a
        .prompt("hello", pending(), |e| {
            events.push(e.clone());
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(t.outcome, Outcome::Completed);
    assert_eq!(c.load(Ordering::SeqCst), 1);
    assert!(matches!(
        events.as_slice(),
        [
            AgentEvent::Started { .. },
            AgentEvent::Delta { .. },
            AgentEvent::InvocationFinished { .. },
            AgentEvent::Finished { .. }
        ]
    ));
}
#[tokio::test]
async fn multiple_turns_reuse_only_completed_context() {
    let (p, _, contexts) = fixture();
    let mut a = Agent::builder(p, AgentOptions::new("test", "m"))
        .build()
        .unwrap();
    for text in ["one", "two", "three"] {
        a.prompt(text, pending(), |_| Ok(())).await.unwrap();
    }
    let ctx = contexts.lock().unwrap();
    assert_eq!(ctx[2].len(), 5);
    assert_eq!(ctx[2][0].content, "one");
}
#[tokio::test]
async fn deny_prevents_network_and_history_mutation() {
    let (p, c, _) = fixture();
    let mut a = Agent::builder(p, AgentOptions::new("test", "m"))
        .extension(Deny)
        .build()
        .unwrap();
    let mut event = None;
    assert!(a
        .prompt("hello", pending(), |e| {
            event = Some(e.clone());
            Ok(())
        })
        .await
        .is_err());
    assert_eq!(c.load(Ordering::SeqCst), 0);
    assert!(a.turns().is_empty());
    assert!(matches!(event, Some(AgentEvent::Rejected { .. })));
}
#[tokio::test]
async fn context_injection_does_not_rewrite_history() {
    let (p, _, contexts) = fixture();
    let mut a = Agent::builder(p, AgentOptions::new("test", "m"))
        .extension(AddContext)
        .build()
        .unwrap();
    let t = a.prompt("hello", pending(), |_| Ok(())).await.unwrap();
    assert_eq!(t.prompt, "hello");
    assert_eq!(contexts.lock().unwrap()[0][0].content, "extra");
}
#[tokio::test]
async fn cannot_silently_rewrite_latest_prompt() {
    let (p, c, _) = fixture();
    let mut a = Agent::builder(p, AgentOptions::new("test", "m"))
        .extension(Rewrite)
        .build()
        .unwrap();
    assert!(matches!(
        a.prompt("hello", pending(), |_| Ok(())).await,
        Err(Error::Denied(_))
    ));
    assert_eq!(c.load(Ordering::SeqCst), 0);
}
#[test]
fn duplicate_extension_names_fail_registration() {
    let (p, _, _) = fixture();
    assert!(Agent::builder(p, AgentOptions::new("t", "m"))
        .extension(Deny)
        .extension(Deny)
        .build()
        .is_err());
}
#[tokio::test]
async fn gate_timeout_is_fail_closed() {
    let (p, c, _) = fixture();
    let mut o = AgentOptions::new("t", "m");
    o.hook_timeout = Duration::from_millis(1);
    let mut a = Agent::builder(p, o).extension(Hanging).build().unwrap();
    assert!(matches!(
        a.prompt("hello", pending(), |_| Ok(())).await,
        Err(Error::Timeout)
    ));
    assert_eq!(c.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn cancellation_works_during_hook() {
    let (p, c, _) = fixture();
    let mut a = Agent::builder(p, AgentOptions::new("t", "m"))
        .extension(Hanging)
        .build()
        .unwrap();
    let t = CancellationToken::new();
    t.cancel();
    assert!(matches!(
        a.prompt("hello", t.cancelled(), |_| Ok(())).await,
        Err(Error::Cancelled)
    ));
    assert_eq!(c.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn final_observer_error_does_not_claim_rollback() {
    let (p, _, _) = fixture();
    let mut a = Agent::builder(p, AgentOptions::new("t", "m"))
        .build()
        .unwrap();
    let t = a
        .prompt("hello", pending(), |e| {
            if matches!(e, AgentEvent::Finished { .. }) {
                Err(Error::Protocol("observer gone"))
            } else {
                Ok(())
            }
        })
        .await
        .unwrap();
    assert_eq!(t.outcome, Outcome::Completed);
    assert_eq!(a.turns().len(), 1);
}
#[tokio::test]
async fn delta_observer_failure_is_an_incomplete_attempt() {
    let (p, _, _) = fixture();
    let mut a = Agent::builder(p, AgentOptions::new("t", "m"))
        .build()
        .unwrap();
    let t = a
        .prompt("hello", pending(), |e| {
            if matches!(e, AgentEvent::Delta { .. }) {
                Err(Error::Protocol("sink failed"))
            } else {
                Ok(())
            }
        })
        .await
        .unwrap();
    assert_eq!(t.outcome, Outcome::Failed);
    assert_eq!(a.retryable_prompt(), Some("hello"));
}
#[tokio::test]
async fn subscribe_is_observational() {
    let (p, c, _) = fixture();
    let mut a = Agent::builder(p, AgentOptions::new("t", "m"))
        .build()
        .unwrap();
    let mut rx = a.subscribe();
    a.prompt("hello", pending(), |_| Ok(())).await.unwrap();
    assert_eq!(c.load(Ordering::SeqCst), 1);
    assert!(matches!(rx.try_recv().unwrap(), AgentEvent::Started { .. }));
}
#[test]
fn limits_and_bad_models_rejected() {
    let (p, _, _) = fixture();
    let mut o = AgentOptions::new("t", "");
    assert!(Agent::builder(p.clone(), o.clone()).build().is_err());
    o.model = "m".into();
    o.max_output_bytes = 0;
    assert!(Agent::builder(p, o).build().is_err());
}
#[tokio::test]
async fn reset_clears_history_and_model_change_preserves_it() {
    let (p, _, _) = fixture();
    let mut a = Agent::builder(p, AgentOptions::new("t", "first"))
        .build()
        .unwrap();
    a.prompt("hello", pending(), |_| Ok(())).await.unwrap();
    a.set_model("second").unwrap();
    assert_eq!(a.turns()[0].model, "first");
    a.reset().unwrap();
    assert!(a.turns().is_empty());
    assert_eq!(a.options().model, "second");
}
