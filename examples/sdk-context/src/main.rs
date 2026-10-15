//! A labelled, deterministic SDK fixture. No CLI/web, credentials, file tools,
//! project reads or real LLM. The store is SQLite in memory.
use aw_agent_core::{
    Agent, AgentOptions, CompletionFuture, CompletionMeta, DeltaSink, Message, Outcome, Provider,
    Result,
};
use aw_store::{
    ApplyContext, ApplyMode, PlanKind, PrepareContext, Selection, SessionBinding, SessionStore,
    SourceSelection,
};
use std::{future::pending, path::PathBuf, sync::Arc};

struct Fixture;
impl Provider for Fixture {
    fn stream<'a>(
        &'a self,
        _messages: &'a [Message],
        _model: &'a str,
        sink: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async move {
            sink("[SDK fixture, not an LLM] Preserve the public API. Test outcomes remain attributed to the original source; no files or tests were touched.")?;
            Ok(CompletionMeta {
                finish_reason: "stop".into(),
                usage: Default::default(),
            })
        })
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let mut agent = Agent::builder(
        Arc::new(Fixture),
        AgentOptions::new("fixture", "fixture-model"),
    )
    .build()?;
    let store = SessionStore::in_memory()?;
    let actor = agent.actor().clone();
    let binding = SessionBinding {
        working_directory: PathBuf::from("/explicit-fixture-only"),
        tool_root: PathBuf::from("/explicit-fixture-only"),
        system_prompt: String::new(),
        profile_hint: "fixture".into(),
        model_hint: "fixture-model".into(),
        destination_fingerprint: "0".repeat(64),
    };
    let (session, branch) = store.create(&actor, "SDK context example", binding)?;
    let (view, journal) = store.activate(&actor, &session.id, &branch.id)?;
    agent.load_history(view.history, Some(Arc::new(journal)))?;
    let first = agent
        .prompt("Record source findings.", pending::<()>(), |_| Ok(()))
        .await?;
    assert_eq!(first.outcome, Outcome::Completed);
    let source = store.context_target(&actor, &session.id, &branch.id)?;
    let target_branch = store.fork(&actor, &session.id, "implementation", &session.root_node)?;
    let target = store.context_target(&actor, &session.id, &target_branch.id)?;
    let plan = store.context_prepare(
        &actor,
        PrepareContext {
            target: target.clone(),
            sources: vec![SourceSelection {
                session_id: session.id.clone(),
                branch: branch.id,
                head: Some(source.head),
                selection: Selection::Divergent,
            }],
            kind: PlanKind::Import,
            focus: "Decisions and qualified test findings".into(),
            max_input_bytes: 131_072,
            max_output_bytes: 4096,
            timeout_seconds: 10,
        },
    )?;
    let identity = agent.default_identity()?;
    let selected =
        agent
            .provider_accounts()
            .select(&actor, &identity.account_id, &identity.model)?;
    let journal = store.summary_journal(&actor, &plan.id, "sdk-summary")?;
    let result = agent
        .summary_executor()?
        .execute(&selected, &plan.input()?, &journal, pending::<()>())
        .await?;
    assert_eq!(result.outcome, Outcome::Completed);
    let operation = store.context_operation(&actor, "sdk-summary")?;
    let artifact = operation
        .artifact_id
        .ok_or(aw_agent_core::Error::Storage("fixture candidate missing"))?;
    let applied = store.context_apply(
        &actor,
        ApplyContext {
            artifact_id: artifact,
            target,
            mode: ApplyMode::Append,
            request_id: "sdk-apply".into(),
            confirmed_disclosure: true,
        },
    )?;
    let (view, handle) = store.activate(&actor, &session.id, &applied.branch.id)?;
    assert_eq!(view.history.context_blocks.len(), 1);
    agent.load_history(view.history, Some(Arc::new(handle)))?;
    let next = agent
        .prompt(
            "Continue using the attached findings.",
            pending::<()>(),
            |_| Ok(()),
        )
        .await?;
    assert_eq!(next.outcome, Outcome::Completed);
    println!("SDK fixture only: source untouched, one summary invocation, one additive import, one new continuation. No provider secret or tool grant was transferred.");
    Ok(())
}
