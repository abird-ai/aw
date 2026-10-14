//! An ordinary Rust library implementing the public aw SDK extension trait.
//! The literal gate is a mechanics example, NOT a production secret detector.
use aw_agent_core::{
    Error, Extension, HookContext, HookEvent, HookFlow, HookFuture, Message, Role,
};

pub struct ExamplePolicy;
impl Extension for ExamplePolicy {
    fn name(&self) -> &str {
        "example-policy"
    }
    fn on_hook<'a>(&'a self, _: HookContext<'a>, event: HookEvent) -> HookFuture<'a> {
        Box::pin(async move {
            match event {
                HookEvent::Prompt {
                    input,
                    mut messages,
                } => {
                    if input.contains("DEMO_BLOCK") {
                        return Err(Error::Denied(
                            "example policy rejected this demonstration marker",
                        ));
                    }
                    messages.insert(
                        0,
                        Message::text(
                            u64::MAX,
                            Role::System,
                            "Keep the response brief. Do not claim to have executed tools.",
                        ),
                    );
                    Ok(HookFlow::Next(Some(HookEvent::Prompt { input, messages })))
                }
                other => Ok(HookFlow::Next(Some(other))),
            }
        })
    }
}
