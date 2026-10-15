//! Separate consumer of the headless SDK + storage. Fixture output, not inference.
use aw_agent_core::{
    Agent, AgentOptions, CompletionFuture, CompletionMeta, DeltaSink, Message, PrincipalId,
    Provider, Result,
};
use aw_store::{SessionBinding, SessionStore};
use std::{future::pending, sync::Arc};
struct Fixture;
impl Provider for Fixture {
    fn stream<'a>(
        &'a self,
        messages: &'a [Message],
        _: &'a str,
        sink: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async move {
            sink(&format!(
                "[SDK fixture, not an LLM] {} context messages.",
                messages.len()
            ))?;
            Ok(CompletionMeta {
                finish_reason: "stop".into(),
                ..Default::default()
            })
        })
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let store = SessionStore::in_memory()?;
    let mut agent = Agent::builder(
        Arc::new(Fixture),
        AgentOptions::new("fixture", "fake-model"),
    )
    .build()?;
    let actor: PrincipalId = agent.actor().clone();
    let binding = SessionBinding {
        working_directory: std::env::current_dir()?,
        tool_root: std::env::current_dir()?,
        system_prompt: String::new(),
        profile_hint: "fixture".into(),
        model_hint: "fake-model".into(),
        destination_fingerprint: "0".repeat(64),
    };
    let (session, branch) = store.create(&actor, "SDK storage example", binding)?;
    let (view, journal) = store.activate(&actor, &session.id, &branch.id)?;
    agent.load_history(view.history, Some(Arc::new(journal)))?;
    agent
        .prompt(
            "Remember that the chosen color is blue.",
            pending::<()>(),
            |_| Ok(()),
        )
        .await?;
    let head = store
        .branches(&actor, &session.id)?
        .into_iter()
        .find(|b| b.id == branch.id)
        .ok_or(aw_agent_core::Error::Storage("branch missing"))?
        .head;
    let fork = store.fork(&actor, &session.id, "alternative", &head)?;
    let (view, journal) = store.activate(&actor, &session.id, &fork.id)?;
    let mut resumed = Agent::builder(
        Arc::new(Fixture),
        AgentOptions::new("fixture", "fake-model"),
    )
    .build()?;
    resumed.load_history(view.history, Some(Arc::new(journal)))?;
    let next = resumed
        .prompt("Continue this explicit fork.", pending::<()>(), |_| Ok(()))
        .await?;
    println!("{}\n{} branches; {} nodes; one original ancestry, current account still configured by host.",next.answer,store.branches(&actor,&session.id)?.len(),store.tree(&actor,&session.id)?.len());
    Ok(())
}
