//! Account selection over a shared history. No HTTP or credentials in this fixture.
use aw_agent_core::*;
use std::{
    collections::BTreeSet,
    future::pending,
    sync::{Arc, Mutex},
};
type SeenMessages = Arc<Mutex<Vec<Vec<Message>>>>;
struct Named {
    label: &'static str,
    seen: SeenMessages,
}
impl Provider for Named {
    fn stream<'a>(
        &'a self,
        m: &'a [Message],
        _: &'a str,
        s: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async move {
            self.seen.lock().unwrap().push(m.to_vec());
            s(self.label)?;
            Ok(CompletionMeta {
                finish_reason: "stop".into(),
                usage: Usage {
                    input_tokens: Some(7),
                    output_tokens: Some(3),
                },
            })
        })
    }
}
fn p(id: &str) -> PrincipalId {
    PrincipalId::new(id).unwrap()
}
fn account(id: &str, user: &str) -> AccountDescriptor {
    AccountDescriptor::personal(id, p(user), "device", ["m".into()])
}
fn fake(label: &'static str) -> (Arc<dyn Provider>, SeenMessages) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    (
        Arc::new(Named {
            label,
            seen: seen.clone(),
        }),
        seen,
    )
}
fn shared() -> AccountDescriptor {
    AccountDescriptor {
        id: "team".into(),
        owner: AccountOwner::Server("host".into()),
        payer: BillingOwner::Shared("engineering".into()),
        credential_custody: CredentialCustody::Server("device".into()),
        credential_version: None,
        inference_executor: "device".into(),
        allowed_models: BTreeSet::from(["m".into()]),
        grants: BTreeSet::from([p("alice"), p("bob")]),
        shared_with_all_authenticated: false,
    }
}
#[test]
fn personal_is_not_selectable_by_other_user() {
    let pool = ProviderAccounts::new("device").unwrap();
    pool.register(account("alice-main", "alice"), fake("a").0)
        .unwrap();
    assert!(pool.select(&p("bob"), "alice-main", "m").is_err());
    assert!(pool.visible_accounts(&p("bob")).unwrap().is_empty());
}
#[test]
fn shared_requires_explicit_grant() {
    let pool = ProviderAccounts::new("device").unwrap();
    pool.register(shared(), fake("team").0).unwrap();
    assert!(pool.select(&p("alice"), "team", "m").is_ok());
    assert!(pool.select(&p("bob"), "team", "m").is_ok());
    assert!(pool.select(&p("eve"), "team", "m").is_err());
}
#[test]
fn explicit_common_all_is_supported() {
    let pool = ProviderAccounts::new("device").unwrap();
    let mut a = shared();
    a.shared_with_all_authenticated = true;
    pool.register(a, fake("team").0).unwrap();
    assert!(pool.select(&p("verified-user"), "team", "m").is_ok());
}
#[test]
fn unknown_model_does_not_fallback() {
    let pool = ProviderAccounts::new("device").unwrap();
    pool.register(shared(), fake("team").0).unwrap();
    assert!(pool.select(&p("alice"), "team", "other").is_err());
}
#[test]
fn user_device_credentials_cannot_be_registered_on_remote_server() {
    let pool = ProviderAccounts::new("remote-server").unwrap();
    assert!(pool
        .register(account("personal", "alice"), fake("a").0)
        .is_err());
}
#[test]
fn mismatched_custody_and_executor_refused() {
    let pool = ProviderAccounts::new("device").unwrap();
    let mut a = account("a", "alice");
    a.credential_custody = CredentialCustody::UserDevice("different-device".into());
    assert!(pool.register(a, fake("a").0).is_err());
}
#[test]
fn personal_account_cannot_be_shared_by_accidental_flag() {
    let pool = ProviderAccounts::new("device").unwrap();
    let mut a = account("a", "alice");
    a.shared_with_all_authenticated = true;
    assert!(pool.register(a, fake("a").0).is_err());
}
#[test]
fn revoked_selection_cannot_dispatch_again() {
    let pool = ProviderAccounts::new("device").unwrap();
    pool.register(shared(), fake("team").0).unwrap();
    let a = pool.select(&p("alice"), "team", "m").unwrap();
    pool.revoke("team").unwrap();
    assert!(a.revalidate().is_err());
}
#[test]
fn modified_grant_invalidates_existing_lease() {
    let pool = ProviderAccounts::new("device").unwrap();
    pool.register(shared(), fake("team").0).unwrap();
    let a = pool.select(&p("alice"), "team", "m").unwrap();
    let mut changed = shared();
    changed.grants = BTreeSet::from([p("bob")]);
    pool.register(changed, fake("new").0).unwrap();
    assert!(a.revalidate().is_err());
}
#[test]
fn unrelated_update_keeps_lease_valid() {
    let pool = ProviderAccounts::new("device").unwrap();
    pool.register(shared(), fake("team").0).unwrap();
    let a = pool.select(&p("alice"), "team", "m").unwrap();
    pool.register(account("bob", "bob"), fake("b").0).unwrap();
    assert!(a.revalidate().is_ok());
}
#[test]
fn metadata_has_no_secret_transport_fields() {
    let pool = ProviderAccounts::new("device").unwrap();
    pool.register(shared(), fake("team").0).unwrap();
    let selected = pool.select(&p("alice"), "team", "m").unwrap();
    let json = serde_json::to_value(selected.identity()).unwrap();
    for k in ["api_key", "token", "headers", "base_url", "api_key_env"] {
        assert!(json.get(k).is_none());
    }
}
#[tokio::test]
async fn users_share_conversation_not_provider_or_bill() {
    let pool = ProviderAccounts::new("device").unwrap();
    let (a, ac) = fake("A answer");
    let (b, bc) = fake("B answer");
    let (t, tc) = fake("Team answer");
    pool.register(account("a", "alice"), a.clone()).unwrap();
    pool.register(account("b", "bob"), b).unwrap();
    pool.register(shared(), t).unwrap();
    let mut agent = Agent::builder(a, AgentOptions::new("unused-default", "m"))
        .build()
        .unwrap();
    for (user, id, text) in [
        ("alice", "a", "first"),
        ("bob", "b", "second"),
        ("alice", "team", "third"),
    ] {
        let selected = pool.select(&p(user), id, "m").unwrap();
        let turn = agent
            .prompt_as(&selected, text, pending(), |_| Ok(()))
            .await
            .unwrap();
        assert_eq!(turn.outcome, Outcome::Completed);
        assert_eq!(turn.actor, Some(p(user)));
        assert_eq!(turn.invocations[0].identity.account_id, id);
    }
    assert_eq!(ac.lock().unwrap().len(), 1);
    assert_eq!(bc.lock().unwrap()[0].len(), 3);
    assert_eq!(tc.lock().unwrap()[0].len(), 5);
    assert_eq!(
        agent.turns()[0].invocations[0].identity.payer,
        BillingOwner::User(p("alice"))
    );
    assert_eq!(
        agent.turns()[1].invocations[0].identity.payer,
        BillingOwner::User(p("bob"))
    );
    assert_eq!(
        agent.turns()[2].invocations[0].identity.payer,
        BillingOwner::Shared("engineering".into())
    );
    assert_eq!(agent.options().profile, "unused-default");
}
#[tokio::test]
async fn revoked_before_prompt_never_calls_provider() {
    let pool = ProviderAccounts::new("device").unwrap();
    let (provider, seen) = fake("a");
    pool.register(shared(), provider.clone()).unwrap();
    let lease = pool.select(&p("alice"), "team", "m").unwrap();
    pool.revoke("team").unwrap();
    let mut a = Agent::builder(provider, AgentOptions::new("default", "m"))
        .build()
        .unwrap();
    assert!(a
        .prompt_as(&lease, "hello", pending(), |_| Ok(()))
        .await
        .is_err());
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn personal_billing_cannot_point_to_someone_else() {
    let pool = ProviderAccounts::new("device").unwrap();
    let mut a = account("alice", "alice");
    a.payer = BillingOwner::User(p("bob"));
    assert!(pool.register(a, fake("x").0).is_err());
}
#[tokio::test]
async fn usage_aggregates_by_caller_not_shared_transcript() {
    let pool = ProviderAccounts::new("device").unwrap();
    pool.register(shared(), fake("team").0).unwrap();
    let mut a = Agent::builder(fake("unused").0, AgentOptions::new("unused", "m"))
        .build()
        .unwrap();
    for who in ["alice", "bob", "alice"] {
        let lease = pool.select(&p(who), "team", "m").unwrap();
        a.prompt_as(&lease, "hello", pending(), |_| Ok(()))
            .await
            .unwrap();
    }
    let alice = usage_for(a.turns(), &p("alice"));
    let bob = usage_for(a.turns(), &p("bob"));
    assert_eq!(alice.model_calls, 2);
    assert_eq!(alice.input_tokens, Some(14));
    assert_eq!(bob.input_tokens, Some(7));
}

#[test]
fn malformed_deserialized_principal_is_rejected() {
    assert!(serde_json::from_str::<PrincipalId>(r#""bad\nprincipal""#).is_err());
    assert!(serde_json::from_str::<PrincipalId>(r#""alice""#).is_ok());
}

#[test]
fn personal_account_cannot_be_declared_server_custodied() {
    let pool = ProviderAccounts::new("device").unwrap();
    let mut a = account("a", "alice");
    a.credential_custody = CredentialCustody::Server("device".into());
    assert!(pool.register(a, fake("x").0).is_err());
}
#[test]
fn server_account_cannot_masquerade_as_personal_device_custody() {
    let pool = ProviderAccounts::new("device").unwrap();
    let mut a = shared();
    a.credential_custody = CredentialCustody::UserDevice("device".into());
    assert!(pool.register(a, fake("x").0).is_err());
}
