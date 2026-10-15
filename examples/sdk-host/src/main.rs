//! Run with `cargo run -p aw-sdk-example`. No HTTP, CLI editor or server dependency.
use aw_agent_core::{
    Agent, AgentEvent, AgentOptions, CompletionFuture, CompletionMeta, DeltaSink, Message,
    Provider, Result,
};
use aw_example_extension::ExamplePolicy;
use std::{future::pending, sync::Arc};

struct LocalFixture;
impl Provider for LocalFixture {
    fn stream<'a>(
        &'a self,
        messages: &'a [Message],
        _: &'a str,
        sink: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async move {
            let input = messages.last().map_or("", |m| m.content.as_str());
            sink("[SDK fixture, not an LLM] ")?;
            sink(input)?;
            Ok(CompletionMeta {
                finish_reason: "stop".into(),
                ..Default::default()
            })
        })
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let mut agent = Agent::builder(Arc::new(LocalFixture), AgentOptions::new("fixture", "echo"))
        .extension(ExamplePolicy)
        .build()?;
    let turn = agent
        .prompt("Hello from another Rust crate.", pending(), |event| {
            if let AgentEvent::Delta { text, .. } = event {
                print!("{text}");
            }
            Ok(())
        })
        .await?;
    println!(
        "\nOutcome: {:?}; retained turns: {}",
        turn.outcome,
        agent.turns().len()
    );
    Ok(())
}
