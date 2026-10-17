//! Authenticated router fixtures. Authored Rust integration tests, not a live
//! browser or provider audit. Each client has a distinct cookie jar/principal.
use super::*;
use aw_agent_core::{
    AccountDescriptor, AccountOwner, AgentOptions, BillingOwner, CompletionFuture, CompletionMeta,
    CredentialCustody, DeltaSink, Message, Provider,
};
#[cfg(feature = "multiplayer")]
use aw_collab::DraftDocument;
use aw_store::{SessionBinding, SessionStore};
use axum::body::Body;
#[cfg(feature = "multiplayer")]
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicUsize, Ordering},
};

const HOST: &str = "127.0.0.1:8787";
const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[cfg(all(feature = "multiplayer", feature = "oidc"))]
mod publication_http_measure {
    use super::*;
    include!("publication_http_measure.rs");
}
struct Fake(Arc<AtomicUsize>);
impl Provider for Fake {
    fn stream<'a>(
        &'a self,
        _: &'a [Message],
        _: &'a str,
        sink: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            sink("Explicit team fixture; no real inference.")?;
            Ok(CompletionMeta {
                finish_reason: "stop".into(),
                ..Default::default()
            })
        })
    }
}
fn descriptor(id: &str, actor: &PrincipalId) -> AccountDescriptor {
    AccountDescriptor {
        id: id.into(),
        owner: AccountOwner::Server("team:fixture".into()),
        payer: BillingOwner::Shared("team:fixture:common".into()),
        credential_custody: CredentialCustody::Server("local".into()),
        credential_version: None,
        inference_executor: "local".into(),
        allowed_models: BTreeSet::from(["fixture-model".into()]),
        grants: BTreeSet::from([actor.clone()]),
        shared_with_all_authenticated: false,
    }
}
fn setup() -> (Router, TeamState, SessionStore, Arc<AtomicUsize>) {
    setup_at(HOST.parse().unwrap())
}
fn setup_at(address: SocketAddr) -> (Router, TeamState, SessionStore, Arc<AtomicUsize>) {
    let store = SessionStore::in_memory().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let roster = Roster::new(
        "fixture",
        vec![
            Participant {
                id: "alice".into(),
                display_name: "Alice".into(),
            },
            Participant {
                id: "bob".into(),
                display_name: "Bob".into(),
            },
        ],
    )
    .unwrap();
    let mut runtimes = Vec::new();
    for id in ["alice", "bob"] {
        let actor = PrincipalId::new(roster.principal(id).unwrap()).unwrap();
        let account = format!("{id}-account");
        let provider = Arc::new(Fake(calls.clone()));
        let agent = Agent::builder(
            provider.clone(),
            AgentOptions::new(account.clone(), "fixture-model"),
        )
        .account(descriptor(&account, &actor), actor.clone(), "local")
        .build()
        .unwrap();
        agent
            .register_provider_account(descriptor("common", &actor), provider)
            .unwrap();
        let binding = SessionBinding {
            working_directory: "/team-fixture".into(),
            tool_root: "/team-fixture".into(),
            system_prompt: String::new(),
            profile_hint: account,
            model_hint: "fixture-model".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let persistence =
            PersistentSession::create(store.clone(), actor, binding, &format!("{id} private"))
                .unwrap();
        runtimes.push(ParticipantRuntime {
            participant_id: id.into(),
            agent,
            persistence,
        });
    }
    let (router, team) =
        application(roster, runtimes, address, &[ALICE.into(), BOB.into()]).unwrap();
    (router, team, store, calls)
}
fn req(method: &str, path: &str, body: serde_json::Value, cookie: Option<&str>) -> Request {
    let mut r = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST);
    if method != "GET" {
        r = r
            .header(header::ORIGIN, format!("http://{HOST}"))
            .header(header::CONTENT_TYPE, "application/json");
    }
    if let Some(cookie) = cookie {
        r = r.header(header::COOKIE, cookie);
    }
    r.body(if body.is_null() {
        Body::empty()
    } else {
        Body::from(serde_json::to_vec(&body).unwrap())
    })
    .unwrap()
}
async fn send(
    router: &Router,
    method: &str,
    path: &str,
    body: serde_json::Value,
    cookie: Option<&str>,
) -> Response {
    router
        .clone()
        .oneshot(req(method, path, body, cookie))
        .await
        .unwrap()
}
async fn data(response: Response) -> serde_json::Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap()
}
async fn login(router: &Router, code: &str) -> String {
    let response = send(
        router,
        "POST",
        "/api/pair",
        serde_json::json!({"code":code}),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}
fn saved(team: &TeamState, id: usize) -> SavedSelection {
    lock(&team.0.entries[id].app.0.session.state)
        .snapshot
        .saved
        .clone()
        .unwrap()
}
fn prompt(team: &TeamState, id: usize, request: &str) -> serde_json::Value {
    let state = lock(&team.0.entries[id].app.0.session.state);
    let saved = state.snapshot.saved.as_ref().unwrap();
    serde_json::json!({"request_id":request,"expected_revision":state.snapshot.revision,"session_id":saved.session_id,"branch_id":saved.branch_id,"prompt":"Explicit fixture question"})
}
async fn settled(team: &TeamState, id: usize) {
    for _ in 0..500 {
        if lock(&team.0.entries[id].app.0.session.state)
            .snapshot
            .pending
            .is_none()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    panic!("fixture run failed to settle");
}
#[tokio::test]
async fn anonymous_has_no_session_or_directory() {
    let (r, _, _, calls) = setup();
    for path in [
        "/api/team/me",
        "/api/session",
        "/api/sessions",
        "/api/accounts",
        "/api/tree",
    ] {
        assert_eq!(
            send(&r, "GET", path, serde_json::Value::Null, None)
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn invite_selects_authenticated_principal() {
    let (r, _, _, _) = setup();
    let a = login(&r, ALICE).await;
    let b = login(&r, BOB).await;
    assert_ne!(a, b);
    for (cookie, id) in [(a, "alice"), (b, "bob")] {
        let me = data(
            send(
                &r,
                "GET",
                "/api/team/me",
                serde_json::Value::Null,
                Some(&cookie),
            )
            .await,
        )
        .await;
        assert_eq!(me["participant"]["id"], id);
        assert_eq!(
            me["participant"]["principal"],
            format!("internal:fixture:{id}")
        );
        assert_eq!(me["tools"], false);
    }
}
#[tokio::test]
async fn forged_actor_header_cannot_choose_peer() {
    let (r, _, _, _) = setup();
    let a = login(&r, ALICE).await;
    let mut request = req("GET", "/api/team/me", serde_json::Value::Null, Some(&a));
    request.headers_mut().insert(
        "x-aw-principal",
        HeaderValue::from_static("internal:fixture:bob"),
    );
    let me = data(r.oneshot(request).await.unwrap()).await;
    assert_eq!(me["participant"]["id"], "alice");
}
#[tokio::test]
async fn duplicate_cookie_denied() {
    let (r, _, _, _) = setup();
    let a = login(&r, ALICE).await;
    let b = login(&r, BOB).await;
    let request = req(
        "GET",
        "/api/session",
        serde_json::Value::Null,
        Some(&format!("{a}; {b}")),
    );
    assert_eq!(
        r.oneshot(request).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn pairing_reuse_denied() {
    let (r, _, _, _) = setup();
    login(&r, ALICE).await;
    assert_eq!(
        send(
            &r,
            "POST",
            "/api/pair",
            serde_json::json!({"code":ALICE}),
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn must_lock_before_switching_browser_identity() {
    let (r, _, _, _) = setup();
    let a = login(&r, ALICE).await;
    assert_eq!(
        send(
            &r,
            "POST",
            "/api/pair",
            serde_json::json!({"code":BOB}),
            Some(&a)
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let b = login(&r, BOB).await;
    assert!(!b.is_empty());
}
#[tokio::test]
async fn invite_expiry_is_checked() {
    let (r, team, _, _) = setup();
    lock(&team.0.entries[0].app.0.auth).pairing_expires = Instant::now() - Duration::from_secs(1);
    assert_eq!(
        send(
            &r,
            "POST",
            "/api/pair",
            serde_json::json!({"code":ALICE}),
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    login(&r, BOB).await;
}
#[tokio::test]
async fn global_pairing_attempt_budget() {
    let (r, _, _, _) = setup();
    for _ in 0..10 {
        assert_eq!(
            send(
                &r,
                "POST",
                "/api/pair",
                serde_json::json!({"code":"invalid"}),
                None
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        send(
            &r,
            "POST",
            "/api/pair",
            serde_json::json!({"code":ALICE}),
            None
        )
        .await
        .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
}
#[tokio::test]
async fn invite_fields_cannot_assert_username_or_admin() {
    let (r, _, _, _) = setup();
    assert_eq!(
        send(
            &r,
            "POST",
            "/api/pair",
            serde_json::json!({"code":ALICE,"username":"bob","admin":true}),
            None
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    login(&r, ALICE).await;
}
#[tokio::test]
async fn pairing_body_is_bounded() {
    let (r, _, _, _) = setup();
    assert_eq!(
        send(
            &r,
            "POST",
            "/api/pair",
            serde_json::json!({"code":"a".repeat(2048)}),
            None
        )
        .await
        .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
}
#[tokio::test]
async fn exact_origin_before_pairing() {
    let (r, _, _, _) = setup();
    let mut q = req("POST", "/api/pair", serde_json::json!({"code":ALICE}), None);
    q.headers_mut().insert(
        header::ORIGIN,
        HeaderValue::from_static("https://attacker.invalid"),
    );
    assert_eq!(r.oneshot(q).await.unwrap().status(), StatusCode::FORBIDDEN);
}
#[tokio::test]
async fn duplicate_origin_denied() {
    let (r, _, _, _) = setup();
    let mut q = req("POST", "/api/pair", serde_json::json!({"code":ALICE}), None);
    q.headers_mut().append(
        header::ORIGIN,
        HeaderValue::from_static("http://127.0.0.1:8787"),
    );
    assert_eq!(r.oneshot(q).await.unwrap().status(), StatusCode::FORBIDDEN);
}
#[tokio::test]
async fn alternate_host_denied() {
    let (r, _, _, _) = setup();
    let mut q = req("GET", "/healthz", serde_json::Value::Null, None);
    q.headers_mut()
        .insert(header::HOST, HeaderValue::from_static("localhost:8787"));
    assert_eq!(r.oneshot(q).await.unwrap().status(), StatusCode::FORBIDDEN);
}
#[tokio::test]
async fn responses_are_no_store_even_when_denied() {
    let (r, _, _, _) = setup();
    let response = send(&r, "GET", "/api/team/me", serde_json::Value::Null, None).await;
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(response.headers()[header::X_FRAME_OPTIONS], "DENY");
}
#[tokio::test]
async fn public_health_exposes_no_roster() {
    let (r, _, _, _) = setup();
    let h = data(send(&r, "GET", "/healthz", serde_json::Value::Null, None).await).await;
    assert_eq!(h["phase"], 38);
    let h = h.to_string();
    assert!(!h.contains("alice"));
    assert!(!h.contains("bob"));
}
#[tokio::test]
async fn distinct_current_views() {
    let (r, team, _, _) = setup();
    let a = login(&r, ALICE).await;
    let b = login(&r, BOB).await;
    let av = data(send(&r, "GET", "/api/session", serde_json::Value::Null, Some(&a)).await).await;
    let bv = data(send(&r, "GET", "/api/session", serde_json::Value::Null, Some(&b)).await).await;
    assert_ne!(av["saved"]["session_id"], bv["saved"]["session_id"]);
    assert_eq!(av["identity"]["actor"], "internal:fixture:alice");
    assert_eq!(bv["identity"]["actor"], "internal:fixture:bob");
    assert_ne!(saved(&team, 0).session_id, saved(&team, 1).session_id);
}
#[tokio::test]
async fn own_inventory_omits_peer() {
    let (r, _, _, _) = setup();
    let a = login(&r, ALICE).await;
    let text = data(
        send(
            &r,
            "GET",
            "/api/sessions",
            serde_json::Value::Null,
            Some(&a),
        )
        .await,
    )
    .await
    .to_string();
    assert!(text.contains("alice private"));
    assert!(!text.contains("bob private"));
}
#[tokio::test]
async fn direct_peer_open_denied() {
    let (r, team, _, calls) = setup();
    let a = login(&r, ALICE).await;
    let target = saved(&team, 1);
    let body = serde_json::json!({"expected_revision":0,"session_id":target.session_id,"branch":target.branch_id});
    assert_eq!(
        send(&r, "POST", "/api/session/open", body, Some(&a))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn private_team_does_not_expose_even_sdk_granted_history() {
    let (r, team, store, _) = setup();
    let a = login(&r, ALICE).await;
    let bob = saved(&team, 1);
    let actor_a = &team.0.entries[0].app.0.session.actor;
    let actor_b = &team.0.entries[1].app.0.session.actor;
    store
        .grant(actor_b, &bob.session_id, actor_a, Some(true))
        .unwrap();
    let rows = data(
        send(
            &r,
            "GET",
            "/api/sessions",
            serde_json::Value::Null,
            Some(&a),
        )
        .await,
    )
    .await;
    assert!(!rows.to_string().contains("bob private"));
    assert_eq!(send(&r,"POST","/api/session/open",serde_json::json!({"expected_revision":0,"session_id":bob.session_id,"branch":bob.branch_id}),Some(&a)).await.status(),StatusCode::FORBIDDEN);
}
#[tokio::test]
async fn reset_and_fork_do_not_move_another_principal() {
    let (r, team, _, _) = setup();
    let a = login(&r, ALICE).await;
    let before = saved(&team, 1);
    assert_eq!(
        send(
            &r,
            "POST",
            "/api/fork",
            serde_json::json!({"expected_revision":0,"name":"alice-fork"}),
            Some(&a)
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(saved(&team, 1).branch_id, before.branch_id);
    let revision = lock(&team.0.entries[0].app.0.session.state)
        .snapshot
        .revision;
    assert_eq!(
        send(
            &r,
            "POST",
            "/api/reset",
            serde_json::json!({"expected_revision":revision}),
            Some(&a)
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(saved(&team, 1).session_id, before.session_id);
}
#[tokio::test]
async fn own_account_inventory_only() {
    let (r, _, _, _) = setup();
    let a = login(&r, ALICE).await;
    let rows = data(
        send(
            &r,
            "GET",
            "/api/accounts",
            serde_json::Value::Null,
            Some(&a),
        )
        .await,
    )
    .await
    .to_string();
    assert!(rows.contains("alice-account"));
    assert!(rows.contains("common"));
    assert!(!rows.contains("bob-account"));
}
#[tokio::test]
async fn peer_account_is_not_a_fallback() {
    let (r, team, _, calls) = setup();
    let a = login(&r, ALICE).await;
    let mut p = prompt(&team, 0, "account-denied");
    p["account_id"] = "bob-account".into();
    p["model"] = "fixture-model".into();
    assert_eq!(
        send(&r, "POST", "/api/prompts", p, Some(&a)).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn caller_cannot_assert_actor_payer_endpoint_or_key() {
    for key in ["actor", "payer", "api_key", "base_url", "credential"] {
        let (r, team, _, calls) = setup();
        let a = login(&r, ALICE).await;
        let mut p = prompt(&team, 0, "forged");
        p[key] = "injected".into();
        assert_eq!(
            send(&r, "POST", "/api/prompts", p, Some(&a)).await.status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
#[tokio::test]
async fn same_request_id_is_independently_attributed() {
    let (r, team, _, calls) = setup();
    let a = login(&r, ALICE).await;
    let b = login(&r, BOB).await;
    for (i, cookie) in [(0, a), (1, b)] {
        let mut p = prompt(&team, i, "same-id");
        p["account_id"] = "common".into();
        p["model"] = "fixture-model".into();
        assert_eq!(
            send(&r, "POST", "/api/prompts", p.clone(), Some(&cookie))
                .await
                .status(),
            StatusCode::ACCEPTED
        );
        settled(&team, i).await;
        assert_eq!(
            send(&r, "POST", "/api/prompts", p, Some(&cookie))
                .await
                .status(),
            StatusCode::ACCEPTED
        );
        let state = lock(&team.0.entries[i].app.0.session.state);
        assert_eq!(state.snapshot.turns.len(), 1);
        assert_eq!(
            state.snapshot.turns[0].invocations[0].identity.actor,
            team.0.entries[i].app.0.session.actor
        );
        assert_eq!(
            state.snapshot.turns[0].invocations[0].identity.account_id,
            "common"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn current_tree_is_private() {
    let (r, team, _, _) = setup();
    let b = login(&r, BOB).await;
    let alice = saved(&team, 0);
    let tree = data(send(&r, "GET", "/api/tree", serde_json::Value::Null, Some(&b)).await)
        .await
        .to_string();
    assert!(!tree.contains(&alice.session_id));
}
#[tokio::test]
async fn tool_and_surface_routes_are_not_granted() {
    let (r, _, _, _) = setup();
    let a = login(&r, ALICE).await;
    let capability = data(
        send(
            &r,
            "GET",
            "/api/workspace",
            serde_json::Value::Null,
            Some(&a),
        )
        .await,
    )
    .await;
    assert_eq!(capability["enabled"], false);
    assert_eq!(
        send(
            &r,
            "POST",
            "/api/workspace/action",
            serde_json::json!({"op":"files","path":""}),
            Some(&a)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
}
#[tokio::test]
async fn logout_is_per_identity() {
    let (r, _, _, _) = setup();
    let a = login(&r, ALICE).await;
    let b = login(&r, BOB).await;
    assert_eq!(
        send(&r, "POST", "/api/logout", serde_json::json!({}), Some(&a))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(&r, "GET", "/api/session", serde_json::Value::Null, Some(&a))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&r, "GET", "/api/session", serde_json::Value::Null, Some(&b))
            .await
            .status(),
        StatusCode::OK
    );
}
#[tokio::test]
async fn expiry_does_not_logout_peer() {
    let (r, team, _, _) = setup();
    let a = login(&r, ALICE).await;
    let b = login(&r, BOB).await;
    lock(&team.0.entries[0].app.0.auth).session_expires =
        Some(Instant::now() - Duration::from_secs(1));
    assert_eq!(
        send(&r, "GET", "/api/session", serde_json::Value::Null, Some(&a))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&r, "GET", "/api/session", serde_json::Value::Null, Some(&b))
            .await
            .status(),
        StatusCode::OK
    );
}
#[tokio::test]
async fn owner_stop_revokes_every_entry() {
    let (r, team, _, _) = setup();
    let a = login(&r, ALICE).await;
    shutdown(&team);
    assert_eq!(
        send(&r, "GET", "/api/session", serde_json::Value::Null, Some(&a))
            .await
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(team.0.entries[0]
        .app
        .0
        .session
        .accounts
        .select(
            &team.0.entries[0].app.0.session.actor,
            "common",
            "fixture-model"
        )
        .is_err());
}
#[test]
fn invalid_bootstrap_is_not_repaired_with_fallback() {
    let roster = Roster::new(
        "r",
        vec![Participant {
            id: "a".into(),
            display_name: "A".into(),
        }],
    )
    .unwrap();
    assert!(application(roster, vec![], HOST.parse().unwrap(), &[]).is_err());
}

#[cfg(feature = "multiplayer")]
async fn share_alice_with_bob(router: &Router, alice_cookie: &str) {
    let response = send(
        router,
        "POST",
        "/api/team/session/share",
        serde_json::json!({
            "participant_id":"bob",
            "enabled":true,
            "mode":"on"
        }),
        Some(alice_cookie),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        data(response).await
    );
}

#[cfg(feature = "multiplayer")]
async fn open_alice_for_bob(router: &Router, team: &TeamState, bob_cookie: &str) {
    let alice = saved(team, 0);
    let revision = lock(&team.0.entries[1].app.0.session.state)
        .snapshot
        .revision;
    let response = send(
        router,
        "POST",
        "/api/team/session/open-shared",
        serde_json::json!({
            "expected_revision":revision,
            "session_id":alice.session_id,
            "branch":alice.branch_id,
        }),
        Some(bob_cookie),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        data(response).await
    );
    assert_eq!(saved(team, 1).session_id, alice.session_id);
}

#[cfg(feature = "multiplayer")]
#[tokio::test]
async fn shared_session_is_explicit_non_discoverable_and_cannot_bypass_draft_submit() {
    let (router, team, _, calls) = setup();
    let alice_cookie = login(&router, ALICE).await;
    let bob_cookie = login(&router, BOB).await;
    let alice = saved(&team, 0);

    let before = data(
        send(
            &router,
            "GET",
            "/api/team/shared-sessions",
            serde_json::Value::Null,
            Some(&bob_cookie),
        )
        .await,
    )
    .await;
    assert!(!before.to_string().contains(&alice.session_id));

    share_alice_with_bob(&router, &alice_cookie).await;

    let after = data(
        send(
            &router,
            "GET",
            "/api/team/shared-sessions",
            serde_json::Value::Null,
            Some(&bob_cookie),
        )
        .await,
    )
    .await;
    assert!(after.to_string().contains(&alice.session_id));

    let ordinary_open = send(
        &router,
        "POST",
        "/api/session/open",
        serde_json::json!({
            "expected_revision":0,
            "session_id":alice.session_id,
            "branch":alice.branch_id,
        }),
        Some(&bob_cookie),
    )
    .await;
    assert_eq!(ordinary_open.status(), StatusCode::FORBIDDEN);

    open_alice_for_bob(&router, &team, &bob_cookie).await;

    let direct = send(
        &router,
        "POST",
        "/api/prompts",
        prompt(&team, 1, "bypass"),
        Some(&bob_cookie),
    )
    .await;
    assert_eq!(direct.status(), StatusCode::FORBIDDEN);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let accounts = data(
        send(
            &router,
            "GET",
            "/api/accounts",
            serde_json::Value::Null,
            Some(&bob_cookie),
        )
        .await,
    )
    .await
    .to_string();
    assert!(accounts.contains("bob-account"));
    assert!(!accounts.contains("alice-account"));
}

#[cfg(feature = "multiplayer")]
#[tokio::test]
async fn shared_draft_update_is_visible_only_after_grant_and_seals_exact_text() {
    let (router, team, _, calls) = setup();
    let alice_cookie = login(&router, ALICE).await;
    let bob_cookie = login(&router, BOB).await;
    share_alice_with_bob(&router, &alice_cookie).await;
    open_alice_for_bob(&router, &team, &bob_cookie).await;

    let opened = data(
        send(
            &router,
            "POST",
            "/api/team/draft/open",
            serde_json::json!({"activate_auto":false}),
            Some(&bob_cookie),
        )
        .await,
    )
    .await;
    assert_eq!(opened["text"], "");

    let doc = DraftDocument::new();
    let update = doc.replace_text("hello from Bob 🙂").unwrap();
    let changed_response = send(
        &router,
        "POST",
        "/api/team/draft/update",
        serde_json::json!({
            "draft_id": opened["draft_id"],
            "generation": opened["generation"],
            "document_epoch": opened["document_epoch"],
            "update_id":"bob-1",
            "update_v1_b64":BASE64.encode(update),
        }),
        Some(&bob_cookie),
    )
    .await;
    assert_eq!(changed_response.status(), StatusCode::OK);
    let changed = data(changed_response).await;
    assert_eq!(changed["text"], "hello from Bob 🙂");

    let owner_view = data(
        send(
            &router,
            "POST",
            "/api/team/draft/status",
            serde_json::json!({
                "draft_id":changed["draft_id"],
                "generation":changed["generation"],
            }),
            Some(&alice_cookie),
        )
        .await,
    )
    .await;
    assert_eq!(owner_view["text"], changed["text"]);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let revision = lock(&team.0.entries[1].app.0.session.state)
        .snapshot
        .revision;
    let denied = send(
        &router,
        "POST",
        "/api/team/draft/submit",
        serde_json::json!({
            "expected_revision":revision,
            "draft_id":changed["draft_id"],
            "generation":changed["generation"],
            "document_epoch":changed["document_epoch"],
            "accepted_revision":changed["accepted_revision"],
            "metadata_revision":changed["metadata_revision"],
            "digest_sha256":changed["candidate_sha256"],
            "account_id":"alice-account",
            "model":"fixture-model",
        }),
        Some(&bob_cookie),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let submit_body = serde_json::json!({
        "expected_revision":revision,
        "draft_id":changed["draft_id"],
        "generation":changed["generation"],
        "document_epoch":changed["document_epoch"],
        "accepted_revision":changed["accepted_revision"],
        "metadata_revision":changed["metadata_revision"],
        "digest_sha256":changed["candidate_sha256"],
        "account_id":"bob-account",
        "model":"fixture-model",
    });
    let accepted = send(
        &router,
        "POST",
        "/api/team/draft/submit",
        submit_body.clone(),
        Some(&bob_cookie),
    )
    .await;
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let first = data(accepted).await;
    assert_eq!(first["submission"]["text"], "hello from Bob 🙂");

    let duplicate = send(
        &router,
        "POST",
        "/api/team/draft/submit",
        submit_body,
        Some(&bob_cookie),
    )
    .await;
    assert_eq!(duplicate.status(), StatusCode::ACCEPTED);
    let second = data(duplicate).await;
    assert_eq!(
        first["submission"]["submission_id"],
        second["submission"]["submission_id"]
    );
    assert_eq!(first["receipt"]["run_id"], second["receipt"]["run_id"]);

    let late = send(
        &router,
        "POST",
        "/api/team/draft/update",
        serde_json::json!({
            "draft_id":changed["draft_id"],
            "generation":changed["generation"],
            "document_epoch":changed["document_epoch"],
            "update_id":"late",
            "update_v1_b64":BASE64.encode(doc.replace_text("late").unwrap()),
        }),
        Some(&bob_cookie),
    )
    .await;
    assert_eq!(late.status(), StatusCode::CONFLICT);
    settled(&team, 1).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[cfg(feature = "multiplayer")]
#[tokio::test]
async fn owner_shared_submit_still_uses_exact_submission_admission() {
    let (router, team, _, calls) = setup();
    let alice_cookie = login(&router, ALICE).await;

    let mode = send(
        &router,
        "POST",
        "/api/team/session/mode",
        serde_json::json!({"mode":"on"}),
        Some(&alice_cookie),
    )
    .await;
    assert_eq!(mode.status(), StatusCode::OK);

    let opened = data(
        send(
            &router,
            "POST",
            "/api/team/draft/open",
            serde_json::json!({"activate_auto":false}),
            Some(&alice_cookie),
        )
        .await,
    )
    .await;
    let doc = DraftDocument::new();
    let update = doc.replace_text("owner exact shared prompt").unwrap();
    let changed = data(
        send(
            &router,
            "POST",
            "/api/team/draft/update",
            serde_json::json!({
                "draft_id":opened["draft_id"],
                "generation":opened["generation"],
                "document_epoch":opened["document_epoch"],
                "update_id":"alice-owner-1",
                "update_v1_b64":BASE64.encode(update),
            }),
            Some(&alice_cookie),
        )
        .await,
    )
    .await;

    let revision = lock(&team.0.entries[0].app.0.session.state)
        .snapshot
        .revision;
    let accepted = data(
        send(
            &router,
            "POST",
            "/api/team/draft/submit",
            serde_json::json!({
                "expected_revision":revision,
                "draft_id":changed["draft_id"],
                "generation":changed["generation"],
                "document_epoch":changed["document_epoch"],
                "accepted_revision":changed["accepted_revision"],
                "metadata_revision":changed["metadata_revision"],
                "digest_sha256":changed["candidate_sha256"],
                "account_id":"alice-account",
                "model":"fixture-model",
            }),
            Some(&alice_cookie),
        )
        .await,
    )
    .await;
    let submission_id = accepted["submission"]["submission_id"]
        .as_str()
        .unwrap()
        .to_owned();

    settled(&team, 0).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let submission = data(
        send(
            &router,
            "POST",
            "/api/team/draft/submission",
            serde_json::json!({"submission_id":submission_id}),
            Some(&alice_cookie),
        )
        .await,
    )
    .await;
    assert_eq!(submission["status"], "admitted");
    assert_eq!(submission["text"], "owner exact shared prompt");
}

#[cfg(feature = "multiplayer")]
#[tokio::test]
async fn presence_is_ephemeral_permission_filtered_and_never_calls_provider() {
    let (router, team, _, calls) = setup();
    let alice_cookie = login(&router, ALICE).await;
    let bob_cookie = login(&router, BOB).await;
    share_alice_with_bob(&router, &alice_cookie).await;
    open_alice_for_bob(&router, &team, &bob_cookie).await;

    let opened = data(
        send(
            &router,
            "POST",
            "/api/team/draft/open",
            serde_json::json!({"activate_auto":false}),
            Some(&bob_cookie),
        )
        .await,
    )
    .await;
    let publish = send(
        &router,
        "POST",
        "/api/team/draft/presence",
        serde_json::json!({
            "draft_id":opened["draft_id"],
            "generation":opened["generation"],
            "client_id":"bob-tab",
            "cursor_anchor":0,
            "cursor_head":0,
            "active":true,
        }),
        Some(&bob_cookie),
    )
    .await;
    assert_eq!(publish.status(), StatusCode::OK);

    let listed = data(
        send(
            &router,
            "POST",
            "/api/team/draft/presence/list",
            serde_json::json!({
                "draft_id":opened["draft_id"],
                "generation":opened["generation"],
            }),
            Some(&alice_cookie),
        )
        .await,
    )
    .await;
    assert_eq!(listed["presence"].as_array().unwrap().len(), 1);
    assert_eq!(listed["presence"][0]["participant_id"], "bob");

    {
        let mut presence = lock(&team.0.multiplayer.presence);
        for value in presence.values_mut() {
            value.updated_at = Instant::now() - Duration::from_secs(31);
        }
    }
    let expired = data(
        send(
            &router,
            "POST",
            "/api/team/draft/presence/list",
            serde_json::json!({
                "draft_id":opened["draft_id"],
                "generation":opened["generation"],
            }),
            Some(&alice_cookie),
        )
        .await,
    )
    .await;
    assert!(expired["presence"].as_array().unwrap().is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[cfg(feature = "multiplayer")]
#[tokio::test]
async fn revoked_shared_participant_presence_is_immediately_scrubbed_in_original_session() {
    let (router, team, _, calls) = setup();
    let alice_cookie = login(&router, ALICE).await;
    let bob_cookie = login(&router, BOB).await;
    share_alice_with_bob(&router, &alice_cookie).await;
    open_alice_for_bob(&router, &team, &bob_cookie).await;
    let opened = data(
        send(
            &router,
            "POST",
            "/api/team/draft/open",
            serde_json::json!({"activate_auto":false}),
            Some(&bob_cookie),
        )
        .await,
    )
    .await;
    let target = serde_json::json!({
        "draft_id":opened["draft_id"], "generation":opened["generation"]
    });
    let posted = send(
        &router,
        "POST",
        "/api/team/draft/presence",
        serde_json::json!({
            "draft_id":opened["draft_id"],"generation":opened["generation"],
            "client_id":"bob-revoked","cursor_anchor":0,"cursor_head":0,
            "active":true
        }),
        Some(&bob_cookie),
    )
    .await;
    assert_eq!(posted.status(), StatusCode::OK);
    // Both participants share the same CRDT generation. A policy revision
    // abandons that generation; neither old cursor is valid afterward.
    let alice_draft = data(
        send(
            &router,
            "POST",
            "/api/team/draft/open",
            serde_json::json!({"activate_auto":false}),
            Some(&alice_cookie),
        )
        .await,
    )
    .await;
    assert_eq!(alice_draft["draft_id"], opened["draft_id"]);
    assert_eq!(alice_draft["generation"], opened["generation"]);
    let owner_posted = send(
        &router,
        "POST",
        "/api/team/draft/presence",
        serde_json::json!({
            "draft_id":opened["draft_id"],"generation":opened["generation"],
            "client_id":"alice-kept","cursor_anchor":0,"cursor_head":0,
            "active":true
        }),
        Some(&alice_cookie),
    )
    .await;
    assert_eq!(owner_posted.status(), StatusCode::OK);
    let before = data(
        send(
            &router,
            "POST",
            "/api/team/draft/presence/list",
            target.clone(),
            Some(&alice_cookie),
        )
        .await,
    )
    .await;
    assert_eq!(before["presence"].as_array().unwrap().len(), 2);

    let revoked = send(
        &router,
        "POST",
        "/api/team/session/share",
        serde_json::json!({"participant_id":"bob","enabled":false}),
        Some(&alice_cookie),
    )
    .await;
    assert_eq!(revoked.status(), StatusCode::OK);
    let after = data(
        send(
            &router,
            "POST",
            "/api/team/draft/presence/list",
            target.clone(),
            Some(&alice_cookie),
        )
        .await,
    )
    .await;
    assert!(
        after["presence"].as_array().unwrap().is_empty(),
        "an audience change abandons the generation for every participant"
    );
    let denied = send(
        &router,
        "POST",
        "/api/team/draft/presence/list",
        target,
        Some(&bob_cookie),
    )
    .await;
    assert!(matches!(
        denied.status(),
        StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[cfg(feature = "multiplayer")]
#[test]
fn presence_invalidation_never_clears_unrelated_session_cursors() {
    let (_, team, _, calls) = setup();
    let alice_session = saved(&team, 0).session_id;
    let bob_session = saved(&team, 1).session_id;
    assert_ne!(alice_session, bob_session);
    for (id, session) in [("alice", &alice_session), ("bob", &bob_session)] {
        lock(&team.0.multiplayer.presence).insert(
            (
                format!("draft-{id}"),
                1,
                id.to_owned(),
                format!("client-{id}"),
            ),
            multiplayer::PresenceRecord::for_test(session, id),
        );
    }
    multiplayer::invalidate_presence(&team, &alice_session);
    let state = lock(&team.0.multiplayer.presence);
    assert_eq!(state.len(), 1);
    assert_eq!(state.keys().next().unwrap().2, "bob");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[cfg(feature = "multiplayer")]
#[tokio::test]
async fn held_authorized_presence_post_cannot_resurrect_a_revoked_cursor() {
    let (router, team, _, calls) = setup();
    let alice_cookie = login(&router, ALICE).await;
    let bob_cookie = login(&router, BOB).await;
    share_alice_with_bob(&router, &alice_cookie).await;
    open_alice_for_bob(&router, &team, &bob_cookie).await;
    let draft = data(
        send(
            &router,
            "POST",
            "/api/team/draft/open",
            serde_json::json!({"activate_auto":false}),
            Some(&bob_cookie),
        )
        .await,
    )
    .await;
    let draft_ref = serde_json::json!({
        "draft_id":draft["draft_id"],"generation":draft["generation"]
    });
    let reached = Arc::new(tokio::sync::Semaphore::new(0));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    *lock(&team.0.multiplayer.held_presence_post) = Some(multiplayer::HeldPresencePost {
        client_id: "bob-held".into(),
        reached: reached.clone(),
        release: release.clone(),
    });
    let held_router = router.clone();
    let held_cookie = bob_cookie.clone();
    let held_draft = draft.clone();
    let held = tokio::spawn(async move {
        send(
            &held_router,
            "POST",
            "/api/team/draft/presence",
            serde_json::json!({
                "draft_id":held_draft["draft_id"],
                "generation":held_draft["generation"],
                "client_id":"bob-held","cursor_anchor":0,"cursor_head":0,
                "active":true
            }),
            Some(&held_cookie),
        )
        .await
    });
    // This is an actual handler request held *after* Store access/grant
    // revalidation and before the final in-memory cursor admission.
    let _reached = tokio::time::timeout(Duration::from_secs(5), reached.acquire())
        .await
        .unwrap()
        .unwrap();

    let revoked = send(
        &router,
        "POST",
        "/api/team/session/share",
        serde_json::json!({"participant_id":"bob","enabled":false}),
        Some(&alice_cookie),
    )
    .await;
    assert_eq!(revoked.status(), StatusCode::OK);
    release.add_permits(1);
    let completed = tokio::time::timeout(Duration::from_secs(5), held)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completed.status(), StatusCode::CONFLICT);
    let remaining = data(
        send(
            &router,
            "POST",
            "/api/team/draft/presence/list",
            draft_ref,
            Some(&alice_cookie),
        )
        .await,
    )
    .await;
    assert!(remaining["presence"].as_array().unwrap().is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[cfg(feature = "multiplayer")]
#[tokio::test]
async fn changing_draft_mode_retires_old_presence_without_model_effects() {
    let (router, team, _, calls) = setup();
    let alice_cookie = login(&router, ALICE).await;
    let bob_cookie = login(&router, BOB).await;
    share_alice_with_bob(&router, &alice_cookie).await;
    open_alice_for_bob(&router, &team, &bob_cookie).await;
    let draft = data(
        send(
            &router,
            "POST",
            "/api/team/draft/open",
            serde_json::json!({"activate_auto":false}),
            Some(&bob_cookie),
        )
        .await,
    )
    .await;
    let present = send(
        &router,
        "POST",
        "/api/team/draft/presence",
        serde_json::json!({
            "draft_id":draft["draft_id"],"generation":draft["generation"],
            "client_id":"bob-before-mode","cursor_anchor":0,"cursor_head":0,
            "active":true
        }),
        Some(&bob_cookie),
    )
    .await;
    assert_eq!(present.status(), StatusCode::OK);
    assert_eq!(lock(&team.0.multiplayer.presence).len(), 1);

    let switched = send(
        &router,
        "POST",
        "/api/team/session/mode",
        serde_json::json!({"mode":"off"}),
        Some(&alice_cookie),
    )
    .await;
    assert_eq!(switched.status(), StatusCode::OK);
    assert!(lock(&team.0.multiplayer.presence).is_empty());
    let old_generation = send(
        &router,
        "POST",
        "/api/team/draft/presence",
        serde_json::json!({
            "draft_id":draft["draft_id"],"generation":draft["generation"],
            "client_id":"alice-after-off","cursor_anchor":0,"cursor_head":0,
            "active":true
        }),
        Some(&alice_cookie),
    )
    .await;
    assert_eq!(old_generation.status(), StatusCode::CONFLICT);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[cfg(feature = "multiplayer")]
#[tokio::test]
async fn audience_regrant_rotates_generation_and_clears_prior_presence() {
    let (router, team, _, calls) = setup();
    let alice_cookie = login(&router, ALICE).await;
    let bob_cookie = login(&router, BOB).await;
    share_alice_with_bob(&router, &alice_cookie).await;
    open_alice_for_bob(&router, &team, &bob_cookie).await;
    let draft = data(
        send(
            &router,
            "POST",
            "/api/team/draft/open",
            serde_json::json!({"activate_auto":false}),
            Some(&bob_cookie),
        )
        .await,
    )
    .await;
    let payload = serde_json::json!({
        "draft_id":draft["draft_id"],"generation":draft["generation"],
        "client_id":"bob-old-generation","cursor_anchor":0,"cursor_head":0,
        "active":true
    });
    let posted = send(
        &router,
        "POST",
        "/api/team/draft/presence",
        payload.clone(),
        Some(&bob_cookie),
    )
    .await;
    assert_eq!(posted.status(), StatusCode::OK);
    assert_eq!(lock(&team.0.multiplayer.presence).len(), 1);
    // An accepted re-grant advances the audience revision even when the
    // participant retains read access. It abandons the old CRDT generation.
    let regrant = send(
        &router,
        "POST",
        "/api/team/session/share",
        serde_json::json!({"participant_id":"bob","enabled":true}),
        Some(&alice_cookie),
    )
    .await;
    assert_eq!(regrant.status(), StatusCode::OK);
    assert!(lock(&team.0.multiplayer.presence).is_empty());

    let old_reply = send(
        &router,
        "POST",
        "/api/team/draft/presence",
        payload,
        Some(&bob_cookie),
    )
    .await;
    assert_eq!(old_reply.status(), StatusCode::CONFLICT);
    let list = data(
        send(
            &router,
            "POST",
            "/api/team/draft/presence/list",
            serde_json::json!({"draft_id":draft["draft_id"],"generation":draft["generation"]}),
            Some(&alice_cookie),
        )
        .await,
    )
    .await;
    assert!(list["presence"].as_array().unwrap().is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[cfg(feature = "multiplayer")]
#[tokio::test]
async fn revocation_removes_shared_discovery_and_draft_access() {
    let (router, team, _, _) = setup();
    let alice_cookie = login(&router, ALICE).await;
    let bob_cookie = login(&router, BOB).await;
    share_alice_with_bob(&router, &alice_cookie).await;
    open_alice_for_bob(&router, &team, &bob_cookie).await;
    let opened = data(
        send(
            &router,
            "POST",
            "/api/team/draft/open",
            serde_json::json!({"activate_auto":false}),
            Some(&bob_cookie),
        )
        .await,
    )
    .await;

    let revoked = send(
        &router,
        "POST",
        "/api/team/session/share",
        serde_json::json!({
            "participant_id":"bob",
            "enabled":false
        }),
        Some(&alice_cookie),
    )
    .await;
    assert_eq!(revoked.status(), StatusCode::OK);

    let sessions = data(
        send(
            &router,
            "GET",
            "/api/team/shared-sessions",
            serde_json::Value::Null,
            Some(&bob_cookie),
        )
        .await,
    )
    .await;
    assert!(!sessions.to_string().contains(&saved(&team, 0).session_id));

    let status = send(
        &router,
        "POST",
        "/api/team/draft/status",
        serde_json::json!({
            "draft_id":opened["draft_id"],
            "generation":opened["generation"],
        }),
        Some(&bob_cookie),
    )
    .await;
    assert!(matches!(
        status.status(),
        StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
    ));
}
