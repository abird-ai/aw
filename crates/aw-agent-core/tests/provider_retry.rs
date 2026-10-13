//! R10 actual hosted Agent retry receipt tests. BeforeDispatch is a
//! trusted adapter proof: Transport/Timeout/HTTP/stream fragments cannot
//! enable automatic hidden replay, and tool effects are never retried.
use aw_agent_core::{
    journal::{JournalEntry, JournalFuture, TurnJournal, TurnStart},
    Agent, AgentOptions, CompletionFuture, CompletionMeta, DeltaSink, Error, Message, Outcome,
    Provider, Usage,
};
use std::{
    future::pending,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
struct ProviderFixture {
    attempts: Arc<AtomicUsize>,
    predispatch: usize,
    partial: bool,
    ambiguous: bool,
}
impl Provider for ProviderFixture {
    fn stream<'a>(
        &'a self,
        _: &'a [Message],
        _model: &'a str,
        sink: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async move {
            let index = self.attempts.fetch_add(1, Ordering::SeqCst);
            if self.partial {
                sink("partial")?;
            }
            if self.ambiguous {
                return Err(Error::Transport("request may have been dispatched"));
            }
            if index < self.predispatch {
                return Err(Error::BeforeDispatch("no bytes dispatched"));
            }
            sink("complete")?;
            Ok(CompletionMeta {
                finish_reason: "stop".into(),
                usage: Usage::default(),
            })
        })
    }
}
#[derive(Default)]
struct Journal(Arc<Mutex<Vec<String>>>);
impl TurnJournal for Journal {
    fn begin<'a>(&'a self, _: TurnStart) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            self.0.lock().unwrap().push("turn".into());
            Ok(())
        })
    }
    fn append<'a>(&'a self, _: u64, entry: JournalEntry) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            let value = match entry {
                JournalEntry::InvocationStarted { ordinal, .. } => format!("start:{ordinal}"),
                JournalEntry::InvocationFinished { record } => {
                    format!("finish:{}:{}", record.ordinal, record.outcome)
                }
                JournalEntry::Assistant { .. } => "assistant".into(),
                JournalEntry::ToolStarted { .. }
                | JournalEntry::ToolFinished { .. }
                | JournalEntry::ToolDenied { .. }
                | JournalEntry::ToolRevised { .. } => {
                    panic!("provider retry must never invent a tool side effect")
                }
            };
            self.0.lock().unwrap().push(value);
            Ok(())
        })
    }
    fn finish<'a>(&'a self, _: aw_agent_core::Turn) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            self.0.lock().unwrap().push("finished".into());
            Ok(())
        })
    }
    fn acknowledge<'a>(&'a self, _: String) -> JournalFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}
fn agent(
    predispatch: usize,
    partial: bool,
    ambiguous: bool,
    retries: u8,
) -> (Agent, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>) {
    let count = Arc::new(AtomicUsize::new(0));
    let audit = Arc::new(Mutex::new(Vec::new()));
    let provider = Arc::new(ProviderFixture {
        attempts: count.clone(),
        predispatch,
        partial,
        ambiguous,
    });
    let mut options = AgentOptions::new("p", "m");
    options.retry = aw_agent_core::PreDispatchRetryPolicy::new(retries).unwrap();
    let mut host = Agent::builder(provider, options).build().unwrap();
    host.load_history(
        aw_agent_core::HistoryState::default(),
        Some(Arc::new(Journal(audit.clone()))),
    )
    .unwrap();
    (host, count, audit)
}
#[tokio::test]
async fn user_opted_pre_dispatch_attempts_are_individually_journaled_and_bound() {
    let (mut host, count, history) = agent(2, false, false, 2);
    let turn = host.prompt("hello", pending(), |_| Ok(())).await.unwrap();
    assert_eq!(turn.outcome, Outcome::Completed);
    assert_eq!(turn.answer, "complete");
    assert_eq!(count.load(Ordering::SeqCst), 3);
    assert_eq!(turn.invocations.len(), 3);
    assert_eq!(
        turn.invocations
            .iter()
            .map(|r| r.ordinal)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(
        turn.invocations
            .iter()
            .map(|r| r.outcome.as_str())
            .collect::<Vec<_>>(),
        vec!["not_dispatched", "not_dispatched", "response_received"]
    );
    assert_eq!(
        *history.lock().unwrap(),
        vec![
            "turn",
            "start:1",
            "finish:1:not_dispatched",
            "start:2",
            "finish:2:not_dispatched",
            "start:3",
            "finish:3:response_received",
            "assistant",
            "finished",
        ]
    );
}
#[tokio::test]
async fn default_retry_off_and_after_exhaustion_are_not_silent() {
    let (mut host, count, _) = agent(2, false, false, 0);
    let turn = host.prompt("x", pending(), |_| Ok(())).await.unwrap();
    assert_eq!(turn.outcome, Outcome::Failed);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(turn.invocations.len(), 1);
    let (mut host, count, _) = agent(3, false, false, 1);
    let turn = host.prompt("x", pending(), |_| Ok(())).await.unwrap();
    assert_eq!(turn.outcome, Outcome::Failed);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert_eq!(turn.invocations.len(), 2);
}
#[tokio::test]
async fn streaming_fragments_and_ambiguous_transport_never_auto_retry() {
    for (partial, ambiguous) in [(true, false), (false, true), (true, true)] {
        let (mut host, count, _) = agent(4, partial, ambiguous, 2);
        let turn = host.prompt("x", pending(), |_| Ok(())).await.unwrap();
        assert_eq!(turn.outcome, Outcome::Failed);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(turn.invocations.len(), 1);
    }
}
