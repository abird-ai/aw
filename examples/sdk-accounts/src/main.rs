//! Example transport only. This does not authenticate users or call a real LLM.
use aw_agent_core::*;
use std::{collections::BTreeSet, future::pending, sync::Arc};
struct Fixture(&'static str);
impl Provider for Fixture {
    fn stream<'a>(
        &'a self,
        _: &'a [Message],
        _: &'a str,
        sink: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async move {
            sink(self.0)?;
            Ok(CompletionMeta {
                finish_reason: "stop".into(),
                usage: Usage {
                    input_tokens: Some(4),
                    output_tokens: Some(2),
                },
            })
        })
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let alice = PrincipalId::new("alice")?;
    let bob = PrincipalId::new("bob")?;
    // All fixtures happen to execute in one trusted test host. In deployment,
    // user-device registries stay on each device; remote key forwarding is forbidden.
    let accounts = ProviderAccounts::new("fixture-host")?;
    accounts.register(
        AccountDescriptor::personal(
            "alice-account",
            alice.clone(),
            "fixture-host",
            ["fixture".into()],
        ),
        Arc::new(Fixture("[fixture] Alice account response")),
    )?;
    accounts.register(
        AccountDescriptor::personal(
            "bob-account",
            bob.clone(),
            "fixture-host",
            ["fixture".into()],
        ),
        Arc::new(Fixture("[fixture] Bob account response")),
    )?;
    accounts.register(
        AccountDescriptor {
            id: "shared-account".into(),
            owner: AccountOwner::Server("demo-server".into()),
            payer: BillingOwner::Shared("demo-team".into()),
            credential_custody: CredentialCustody::Server("fixture-host".into()),
            credential_version: None,
            inference_executor: "fixture-host".into(),
            allowed_models: BTreeSet::from(["fixture".into()]),
            grants: BTreeSet::from([alice.clone(), bob.clone()]),
            shared_with_all_authenticated: false,
        },
        Arc::new(Fixture("[fixture] Shared account response")),
    )?;
    let mut conversation = Agent::builder(
        Arc::new(Fixture("unused")),
        AgentOptions::new("default", "fixture"),
    )
    .build()?;
    for (actor, account) in [
        (&alice, "alice-account"),
        (&bob, "bob-account"),
        (&alice, "shared-account"),
    ] {
        let selected = accounts.select(actor, account, "fixture")?;
        let turn = conversation
            .prompt_as(&selected, "Continue this shared thread.", pending(), |_| {
                Ok(())
            })
            .await?;
        println!(
            "{}",
            serde_json::to_string(&turn.invocations)
                .map_err(|_| Error::Protocol("example serialization"))?
        );
    }
    assert!(accounts.select(&bob, "alice-account", "fixture").is_err());
    println!(
        "{} completed turns; no credentials, network or billing used in this fixture.",
        conversation.turns().len()
    );
    Ok(())
}
