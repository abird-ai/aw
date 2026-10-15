//! Independent device/protocol consumer. No CLI, provider adapter, tool runner,
//! web server or model dependency. --run-fixture explicitly runs fake inference.
use aw_agent_core::{
    AccountOwner, BillingOwner, CancellationToken, CompletionFuture, CompletionMeta,
    CredentialCustody, DeltaSink, InvocationContext, InvocationIdentity, InvocationScope, Message,
    PrincipalId, Provider, Result,
};
use aw_inference::{
    protocol::{self, Binding, Event, PairingKey, Request, WireMessage},
    DeviceWorker, Grant,
};
use std::{collections::BTreeSet, sync::Arc};
struct Fixture;
impl Provider for Fixture {
    fn stream<'a>(
        &'a self,
        _: &'a [Message],
        _: &'a str,
        sink: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async move {
            sink("Explicit fixture response; this is not an LLM.\n")?;
            Ok(CompletionMeta {
                finish_reason: "stop".into(),
                ..Default::default()
            })
        })
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    if std::env::args().nth(1).as_deref() != Some("--run-fixture") {
        println!("Independent aw-inference SDK consumer. Pass --run-fixture to run an in-memory fake model; no network/provider is contacted.");
        return Ok(());
    }
    let dir = std::env::temp_dir().join(format!("aw-inference-example-{}", protocol::random_id()?));
    aw_platform::ensure_private_dir(&dir).map_err(|_| {
        aw_agent_core::Error::Denied(
            "device fixture requires platform-verified owner-only private files",
        )
    })?;
    let result = fixture(&dir).await;
    std::fs::remove_dir_all(&dir)?;
    result
}
async fn fixture(dir: &std::path::Path) -> Result<()> {
    let actor = PrincipalId::new("example-user")?;
    let time = aw_inference::grant::now()?;
    let grant = Grant {
        version: 1,
        allowed_summary_digests: Default::default(),
        binding: Binding {
            actor: actor.as_str().into(),
            account_id: "fixture-account".into(),
            executor_id: "fixture-device".into(),
            model: "fixture-model".into(),
            grant_id: "fixture-grant".into(),
        },
        scopes: vec![InvocationScope {
            session_id: "s_fixture".into(),
            branch_id: "b_fixture".into(),
        }],
        allowed_tools: BTreeSet::new(),
        issued_at: time,
        expires_at: time + 60,
        max_calls: 1,
        max_context_bytes: 4096,
        max_output_bytes: 1024,
        max_total_output_bytes: 1024,
        max_output_tokens: 128,
        timeout_seconds: 5,
    };
    let grant_path = dir.join("grant.json");
    aw_inference::private::create(&grant_path, &serde_json::to_vec(&grant).unwrap())?;
    let mut worker = DeviceWorker::open(
        Arc::new(Fixture),
        &grant_path,
        &dir.join("receipts.json"),
        &"11".repeat(32),
    )?;
    let (mut owner, local) = tokio::io::duplex(65536);
    // Fixture-only pairing capability, never an example real credential.
    let key = Arc::new(PairingKey::from_bytes([7; 32]));
    let device_key = key.clone();
    let stop = CancellationToken::new();
    let device_stop = stop.clone();
    let task = tokio::spawn(async move { worker.run_stream(local, device_key, device_stop).await });
    let (hello, nonce) = protocol::owner_handshake(&mut owner, &key, grant.binding.clone()).await?;
    let (read, write) = tokio::io::split(owner);
    let mut tx = protocol::Sender::new(
        write,
        protocol::Authenticator::new(key.clone(), &hello, &nonce, true)?,
    );
    let mut rx = protocol::Receiver::new(
        read,
        protocol::Authenticator::new(key, &hello, &nonce, true)?,
    );
    let context = InvocationContext {
        purpose: Default::default(),
        scope: Some(grant.scopes[0].clone()),
        turn_id: 1,
        ordinal: 1,
        identity: InvocationIdentity {
            actor: actor.clone(),
            account_id: grant.binding.account_id.clone(),
            account_owner: AccountOwner::User(actor.clone()),
            payer: BillingOwner::User(actor),
            credential_custody: CredentialCustody::UserDevice(grant.binding.executor_id.clone()),
            credential_version: None,
            inference_executor: grant.binding.executor_id.clone(),
            model: grant.binding.model.clone(),
            grant_revision: 1,
        },
    };
    tx.send(&Request {
        version: protocol::VERSION,
        id: Request::identity(&context)?,
        epoch: hello.epoch,
        context,
        messages: vec![WireMessage::from_message(&Message::text(
            1,
            aw_agent_core::Role::User,
            "Run the labelled fixture.",
        ))],
        tools: vec![],
    })
    .await?;
    let mut completed = false;
    while let Some(event) = rx.next::<Event>().await? {
        match event {
            Event::Delta { text, .. } => print!("{text}"),
            Event::Finished { .. } => {
                completed = true;
                break;
            }
            Event::Failed { code, .. } => return Err(code.error()),
        }
    }
    drop(tx);
    drop(rx);
    stop.cancel();
    let _ = task.await;
    if !completed {
        return Err(aw_agent_core::Error::Transport(
            "fixture stream ended without completion",
        ));
    }
    println!("One reserved inference; provider configuration was never a protocol field.");
    Ok(())
}
