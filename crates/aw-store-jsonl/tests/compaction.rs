//! R10 genuine JSONL durable summaries / active-tail/500+ turn proof.
use aw_agent_core::{
    Agent, AgentOptions, CompactionPlan, CompactionPolicy, CompletionFuture, CompletionMeta,
    DeltaSink, InvocationIdentity, Outcome, PrincipalId, Provider, ProviderSelection, Result, Turn,
    TurnStart, Usage,
};
use aw_store_api::{SessionBackend, SessionBinding, SessionCursor};
use aw_store_jsonl::JsonlStore;
use std::{
    future::pending,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

fn dir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    reserve_dir(&std::env::temp_dir(), stamp, &NEXT)
}
fn reserve_dir(base: &Path, stamp: u128, next: &AtomicUsize) -> PathBuf {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    for _ in 0..32 {
        let nonce = next.fetch_add(1, Ordering::Relaxed);
        let value = base.join(format!(
            "aw-r10-jsonl-{}-{stamp}-{nonce}",
            std::process::id()
        ));
        match builder.create(&value) {
            Ok(()) => return value,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("cannot reserve fixture directory {value:?}: {error}"),
        }
    }
    panic!("fixture directory candidates already exist under {base:?}");
}

#[test]
fn repeated_timestamp_roots_are_exclusively_reserved() {
    let base = dir();
    let existing = base.join(format!("aw-r10-jsonl-{}-0-0", std::process::id()));
    std::fs::create_dir(&existing).unwrap();
    std::fs::write(existing.join("sentinel"), b"keep existing bytes").unwrap();
    let next = Arc::new(AtomicUsize::new(0));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let base = base.clone();
            let next = next.clone();
            std::thread::spawn(move || {
                let root = reserve_dir(&base, 0, &next);
                let store = JsonlStore::open(root.join("jsonl")).unwrap();
                (root, store)
            })
        })
        .collect();
    // Keep every owner alive together so accidental shared roots also fail.
    let roots: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    let distinct: std::collections::HashSet<_> = roots.iter().map(|(root, _)| root).collect();
    assert_eq!(distinct.len(), 8);
    assert!(!distinct.contains(&existing));
    assert_eq!(
        std::fs::read(existing.join("sentinel")).unwrap(),
        b"keep existing bytes"
    );
    drop(roots);
    std::fs::remove_dir_all(base).unwrap();
}
fn binding(root: &Path) -> SessionBinding {
    SessionBinding {
        working_directory: root.into(),
        tool_root: root.into(),
        system_prompt: "You are a safe fixture".into(),
        profile_hint: "p".into(),
        model_hint: "m".into(),
        destination_fingerprint: "a".repeat(64),
    }
}
fn owner() -> PrincipalId {
    PrincipalId::new("local:r10fixture").unwrap()
}
fn actor(agent: &Agent) -> InvocationIdentity {
    agent.default_identity().unwrap()
}
fn stored_turn(id: u64, actor: &InvocationIdentity) -> Turn {
    Turn {
        id,
        prompt: format!("User prompt {id}"),
        answer: format!("Assistant answer {id}"),
        model: actor.model.clone(),
        profile: actor.account_id.clone(),
        outcome: Outcome::Completed,
        meta: Some(CompletionMeta {
            finish_reason: "stop".into(),
            usage: Usage::default(),
        }),
        error: None,
        actor: Some(actor.actor.clone()),
        tool_records: vec![],
        invocations: vec![],
        context_messages: vec![],
        needs_reconciliation: false,
    }
}
fn append_turn(cursor: &Arc<dyn SessionCursor>, id: u64, who: &InvocationIdentity) {
    let journal = cursor.journal();
    futures_lite::future::block_on(journal.begin(TurnStart {
        turn_id: id,
        prompt: format!("User prompt {id}"),
        identity: who.clone(),
    }))
    .unwrap();
    futures_lite::future::block_on(journal.finish(stored_turn(id, who))).unwrap();
}
struct FakeSummary(Arc<AtomicUsize>);
impl Provider for FakeSummary {
    fn stream<'a>(
        &'a self,
        _: &'a [aw_agent_core::Message],
        _model: &'a str,
        sink: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            sink(
                "Compacted evidence of earlier complete turns. Preserve uncertainty and decisions.",
            )?;
            Ok(CompletionMeta {
                finish_reason: "stop".into(),
                usage: Usage::default(),
            })
        })
    }
}
fn setup(root: &Path) -> (JsonlStore, Arc<dyn SessionCursor>, Agent, Arc<AtomicUsize>) {
    let store = JsonlStore::open(root.join("jsonl")).unwrap();
    let actor = owner();
    let (_, _, cursor) = store.create(&actor, "test", binding(root)).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let provider: Arc<dyn Provider> = Arc::new(FakeSummary(count.clone()));
    let descriptor =
        aw_agent_core::AccountDescriptor::personal("p", actor.clone(), "local", ["m".to_string()]);
    let agent = Agent::builder(provider, AgentOptions::new("p", "m"))
        .account(descriptor, actor, "local")
        .build()
        .unwrap();
    (store, cursor, agent, count)
}
fn selected(agent: &Agent) -> ProviderSelection {
    agent
        .provider_accounts()
        .select(&owner(), "p", "m")
        .unwrap()
}
async fn compact(
    cursor: &Arc<dyn SessionCursor>,
    agent: &Agent,
    sequence: usize,
) -> Result<aw_agent_core::HistoryState> {
    let current = cursor.view()?;
    let policy = CompactionPolicy {
        keep_recent: 8,
        trigger_turns: 16,
        ..CompactionPolicy::default()
    };
    let plan = CompactionPlan::prepare(
        &current.history,
        cursor.session_id(),
        &format!("run_{sequence}"),
        &policy,
    )?;
    let journal = cursor.compaction_journal(plan.clone())?;
    let result = agent
        .summary_executor()?
        .execute(&selected(agent), &plan.input, journal.as_ref(), pending())
        .await?;
    assert_eq!(result.outcome, Outcome::Completed);
    cursor.apply_compaction(&plan.request_id)
}

#[tokio::test]
async fn hundreds_of_turns_survive_bounded_projection_checkpoints_and_restart_without_replay() {
    let root = dir();
    let (store, cursor, agent, count) = setup(&root);
    let who = actor(&agent);
    let mut previous = 0;
    for id in 1..=520 {
        append_turn(&cursor, id, &who);
        let state = cursor.view().unwrap().history;
        if state.turns.len() == 40 {
            let projected = compact(&cursor, &agent, id as usize).await.unwrap();
            assert_eq!(projected.turns.len(), 8);
            assert_eq!(projected.next_turn_id, id + 1);
            assert!(projected.compacted_through > previous);
            previous = projected.compacted_through;
            assert!(projected.summary_provenance_sha256.is_some());
            // New Agent may attach existing history and continue without
            // recreating credentials or any summary provider calls.
            let mut reloaded = Agent::builder(
                Arc::new(FakeSummary(count.clone())),
                AgentOptions::new("p", "m"),
            )
            .build()
            .unwrap();
            reloaded
                .load_history(projected, Some(cursor.journal()))
                .unwrap();
            assert!(reloaded.history_state().turns.len() <= 40);
        }
    }
    let last = cursor.view().unwrap();
    assert_eq!(last.history.next_turn_id, 521);
    assert!(last.history.turns.len() <= 40);
    assert!(last.history.compacted_through > 400);
    assert_eq!(last.session.next_turn, 521);
    assert_eq!(last.branch.revision, 520);
    let calls = count.load(Ordering::SeqCst);
    assert!(calls >= 10);
    let file = root
        .join("jsonl")
        .join(format!("{}.jsonl", cursor.session_id()));
    let durable = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        durable.matches("\"type\":\"turn_finished\"").count(),
        520,
        "raw immutable full conversation history must not be trimmed"
    );
    assert_eq!(
        durable.matches("\"type\":\"projection_compacted\"").count(),
        calls
    );
    drop(agent);
    drop(cursor);
    drop(store);
    let reopened = JsonlStore::open(root.join("jsonl")).unwrap();
    let existing = reopened.list(&owner()).unwrap();
    assert_eq!(existing.len(), 1);
    let restored = reopened.open(&owner(), &existing[0].id, "main").unwrap();
    assert_eq!(restored.view().unwrap().history.next_turn_id, 521);
    assert_eq!(
        count.load(Ordering::SeqCst),
        calls,
        "restart must not invoke provider"
    );
    drop(restored);
    drop(reopened);
    let _ = std::fs::remove_dir_all(root);
}
#[tokio::test]
async fn stale_plan_never_spends_or_mutates_a_different_head() {
    let root = dir();
    let (store, cursor, agent, count) = setup(&root);
    let who = actor(&agent);
    for n in 1..=26 {
        append_turn(&cursor, n, &who);
    }
    let policy = CompactionPolicy::default();
    let plan = CompactionPlan::prepare(
        &cursor.view().unwrap().history,
        cursor.session_id(),
        "stale_head",
        &policy,
    )
    .unwrap();
    append_turn(&cursor, 27, &who);
    assert!(cursor.compaction_journal(plan).is_err());
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(cursor.view().unwrap().history.next_turn_id, 28);
    drop(agent);
    drop(cursor);
    drop(store);
    let _ = std::fs::remove_dir_all(root);
}
#[tokio::test]
async fn interrupted_paid_compaction_blocks_invisible_retry_until_inspected() {
    let root = dir();
    let (store, cursor, agent, count) = setup(&root);
    let who = actor(&agent);
    for n in 1..=20 {
        append_turn(&cursor, n, &who);
    }
    let policy = CompactionPolicy::default();
    let plan = CompactionPlan::prepare(
        &cursor.view().unwrap().history,
        cursor.session_id(),
        "interrupt_1",
        &policy,
    )
    .unwrap();
    let journal = cursor.compaction_journal(plan.clone()).unwrap();
    aw_agent_core::SummaryJournal::begin(journal.as_ref(), &plan.input, &who)
        .await
        .unwrap();
    assert!(cursor.view().unwrap().history.needs_reconciliation);
    assert!(cursor.compaction_journal(plan.clone()).is_err());
    assert!(cursor.apply_compaction(&plan.request_id).is_err());
    assert_eq!(count.load(Ordering::SeqCst), 0);
    drop(journal);
    drop(cursor);
    drop(store);
    let reopened = JsonlStore::open(root.join("jsonl")).unwrap();
    let id = reopened.list(&owner()).unwrap()[0].id.clone();
    let cursor = reopened.open(&owner(), &id, "main").unwrap();
    assert!(cursor.view().unwrap().history.needs_reconciliation);
    assert!(cursor.compaction_journal(plan).is_err());
    cursor
        .journal()
        .acknowledge("explicit inspection of unknown summary billing".into())
        .await
        .unwrap();
    assert!(!cursor.view().unwrap().history.needs_reconciliation);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    drop(cursor);
    drop(reopened);
    drop(agent);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn settled_summary_after_crash_can_apply_without_a_new_paid_invocation() {
    let root = dir();
    let (store, cursor, agent, count) = setup(&root);
    let who = actor(&agent);
    for id in 1..=26 {
        append_turn(&cursor, id, &who);
    }
    let plan = CompactionPlan::prepare(
        &cursor.view().unwrap().history,
        cursor.session_id(),
        "once_only",
        &CompactionPolicy::default(),
    )
    .unwrap();
    let journal = cursor.compaction_journal(plan.clone()).unwrap();
    let invocation = aw_agent_core::SummaryJournal::begin(journal.as_ref(), &plan.input, &who)
        .await
        .unwrap();
    assert_eq!(invocation.ordinal, 1);
    aw_agent_core::SummaryJournal::finish(
        journal.as_ref(),
        &aw_agent_core::SummaryResult {
            outcome: Outcome::Completed,
            text: "Previously paid model summary, no repeat invocation".into(),
            meta: None,
            error: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        cursor.compaction_ready().unwrap(),
        Some(plan.request_id.clone())
    );
    assert!(cursor.compaction_journal(plan.clone()).is_err());
    assert_eq!(count.load(Ordering::SeqCst), 0);
    drop(journal);
    drop(cursor);
    drop(store);
    let store = JsonlStore::open(root.join("jsonl")).unwrap();
    let session = store.list(&owner()).unwrap()[0].id.clone();
    let cursor = store.open(&owner(), &session, "main").unwrap();
    assert_eq!(
        cursor.compaction_ready().unwrap(),
        Some(plan.request_id.clone())
    );
    let projected = cursor.apply_compaction(&plan.request_id).unwrap();
    assert_eq!(projected.compacted_through, 10);
    assert_eq!(projected.turns.len(), 16);
    assert_eq!(projected.next_turn_id, 27);
    assert_eq!(
        count.load(Ordering::SeqCst),
        0,
        "completed summary was NEVER regenerated"
    );
    assert_eq!(cursor.compaction_ready().unwrap(), None);
    assert!(cursor.apply_compaction(&plan.request_id).is_err());
    append_turn(&cursor, 27, &who);
    assert_eq!(cursor.view().unwrap().history.next_turn_id, 28);
    let bytes =
        std::fs::read_to_string(root.join("jsonl").join(format!("{session}.jsonl"))).unwrap();
    assert_eq!(bytes.matches("\"type\":\"turn_finished\"").count(), 27);
    assert_eq!(bytes.matches("\"type\":\"compaction_started\"").count(), 1);
    assert_eq!(
        bytes.matches("\"type\":\"projection_compacted\"").count(),
        1
    );
    drop(cursor);
    drop(store);
    drop(agent);
    let _ = std::fs::remove_dir_all(root);
}
