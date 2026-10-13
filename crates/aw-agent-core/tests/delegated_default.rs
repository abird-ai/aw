//! Trusted-host registration, not a transport/authentication implementation.
use aw_agent_core::*;
use std::{collections::BTreeSet, sync::Arc};
struct Delegated;
impl Provider for Delegated {
    fn delegated_binding(&self) -> Option<DelegatedBinding> {
        Some(DelegatedBinding {
            account_id: "personal".into(),
            executor_id: "laptop".into(),
        })
    }
    fn stream<'a>(
        &'a self,
        _: &'a [Message],
        _: &'a str,
        _: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async { Err(Error::Denied("unscoped fixture never runs")) })
    }
}
struct Local;
impl Provider for Local {
    fn stream<'a>(
        &'a self,
        _: &'a [Message],
        _: &'a str,
        _: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async { Ok(CompletionMeta::default()) })
    }
}
fn person() -> PrincipalId {
    PrincipalId::new("internal:test:alice").unwrap()
}
fn descriptor() -> AccountDescriptor {
    AccountDescriptor::personal("personal", person(), "laptop", ["m1".into(), "m2".into()])
}
fn build() -> Result<Agent> {
    Agent::builder(Arc::new(Delegated), AgentOptions::new("personal", "m1"))
        .delegated_account(descriptor(), person(), "owner")
        .build()
}
#[test]
fn explicit_personal_default_does_not_require_server_credentials() {
    let a = build().unwrap();
    let i = a.default_identity().unwrap();
    assert_eq!(i.actor, person());
    assert_eq!(i.payer, BillingOwner::User(person()));
    assert_eq!(i.inference_executor, "laptop");
}
#[test]
fn ordinary_registration_still_refuses_nonlocal_default() {
    assert!(
        Agent::builder(Arc::new(Delegated), AgentOptions::new("personal", "m1"))
            .account(descriptor(), person(), "owner")
            .build()
            .is_err()
    );
}
#[test]
fn local_provider_cannot_masquerade_as_device() {
    assert!(
        Agent::builder(Arc::new(Local), AgentOptions::new("personal", "m1"))
            .delegated_account(descriptor(), person(), "owner")
            .build()
            .is_err()
    );
}
#[test]
fn wrong_principal_cannot_use_personal_default() {
    assert!(
        Agent::builder(Arc::new(Delegated), AgentOptions::new("personal", "m1"))
            .delegated_account(
                descriptor(),
                PrincipalId::new("internal:test:bob").unwrap(),
                "owner"
            )
            .build()
            .is_err()
    );
}
#[test]
fn wrong_account_binding_is_denied() {
    let mut d = descriptor();
    d.id = "different".into();
    assert!(
        Agent::builder(Arc::new(Delegated), AgentOptions::new("different", "m1"))
            .delegated_account(d, person(), "owner")
            .build()
            .is_err()
    );
}
#[test]
fn delegated_set_model_only_selects_pregranted_model() {
    let mut a = build().unwrap();
    a.set_model("m2").unwrap();
    assert_eq!(a.default_identity().unwrap().model, "m2");
    assert!(a.set_model("ungranted").is_err());
    assert_eq!(a.options().model, "m2");
}
#[test]
fn delegated_set_model_keeps_existing_lease_valid() {
    let mut a = build().unwrap();
    let lease = a
        .provider_accounts()
        .select(&person(), "personal", "m1")
        .unwrap();
    a.set_model("m2").unwrap();
    assert!(lease.revalidate().is_ok());
}
#[test]
fn revoked_delegation_is_not_recreated_by_model_change() {
    let mut a = build().unwrap();
    a.provider_accounts().revoke("personal").unwrap();
    assert!(a.set_model("m1").is_err());
    assert!(a.default_identity().is_err());
}
#[test]
fn declared_local_model_allowlist_is_not_expanded() {
    let mut a = build().unwrap();
    assert!(a.set_model("new").is_err());
    let descriptors = a.provider_accounts().visible_accounts(&person()).unwrap();
    assert_eq!(
        descriptors[0].allowed_models,
        BTreeSet::from(["m1".into(), "m2".into()])
    );
}
