// Separate browser cookie jars are distinct clients of ONE owner principal.
// No new Actor, account, model/tool authority or durable browser session.

async fn issue_browser_invite(router: &Router, cookie: &str) -> (StatusCode, serde_json::Value) {
    let response = router.clone().oneshot(request(
        "POST", "/api/browser/invite", serde_json::json!({}), Some(cookie),
    )).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
async fn pair_browser_code(router: &Router, code: &str) -> (StatusCode, String) {
    let response = router.clone().oneshot(request(
        "POST", "/api/pair", serde_json::json!({"code":code}), None,
    )).await.unwrap();
    let status = response.status();
    let cookie = response.headers().get(header::SET_COOKIE)
        .and_then(|value| value.to_str().ok()).unwrap_or("").to_string();
    (status, cookie)
}
pub(super) async fn sibling_cookie(router: &Router, owner_cookie: &str) -> String {
    let (status, invitation) = issue_browser_invite(router, owner_cookie).await;
    assert_eq!(status, StatusCode::OK, "{invitation}");
    let code = invitation["code"].as_str().unwrap();
    assert!(valid_token(code));
    assert_eq!(invitation["role"], "same_owner_browser");
    assert_eq!(invitation["max_browsers"], MAX_OWNER_BROWSER_SESSIONS);
    let (status, cookie) = pair_browser_code(router, code).await;
    assert_eq!(status, StatusCode::OK);
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
    assert_ne!(cookie.split(';').next().unwrap(), owner_cookie);
    cookie.split(';').next().unwrap().into()
}

#[tokio::test]
async fn invitations_require_actual_paired_browser_and_never_grant_localweb() {
    let (router, _, calls) = app();
    assert_eq!(router.clone().oneshot(request(
        "POST", "/api/browser/invite", serde_json::json!({}), None,
    )).await.unwrap().status(), StatusCode::UNAUTHORIZED);
    let primary = paired(&router).await;
    let wrong = format!("aw_session_8787={}", "0".repeat(64));
    assert_eq!(router.clone().oneshot(request(
        "POST", "/api/browser/invite", serde_json::json!({}), Some(&wrong),
    )).await.unwrap().status(), StatusCode::UNAUTHORIZED);
    let mut no_origin = request(
        "POST", "/api/browser/invite", serde_json::json!({}), Some(&primary),
    );
    no_origin.headers_mut().remove(header::ORIGIN);
    assert_eq!(router.clone().oneshot(no_origin).await.unwrap().status(),
        StatusCode::FORBIDDEN);
    assert_eq!(router.clone().oneshot(request(
        "POST", "/api/browser/invite", serde_json::json!({"unexpected":true}),
        Some(&primary),
    )).await.unwrap().status(), StatusCode::BAD_REQUEST);
    let (local, _, local_root) = local_app();
    assert_eq!(local.oneshot(request(
        "POST", "/api/browser/invite", serde_json::json!({}), None,
    )).await.unwrap().status(), StatusCode::FORBIDDEN);
    std::fs::remove_dir_all(local_root).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn explicit_same_owner_invite_pairs_distinct_cookies_without_extending_session() {
    let (router, app, calls) = app();
    let primary = paired(&router).await;
    let old_expiry = lock(&app.0.auth).session_expires.unwrap();
    let secondary = sibling_cookie(&router, &primary).await;
    assert_ne!(primary, secondary);
    assert_eq!(lock(&app.0.auth).session_expires, Some(old_expiry));
    for cookie in [&primary, &secondary] {
        assert_eq!(router.clone().oneshot(request(
            "GET", "/api/session", serde_json::Value::Null, Some(cookie),
        )).await.unwrap().status(), StatusCode::OK);
    }
    let (used, _) = issue_browser_invite(&router, &secondary).await;
    assert_eq!(used, StatusCode::OK, "an explicitly paired sibling is the same owner");
    assert_eq!(lock(&app.0.auth).sibling_cookie_hashes.len(), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn invitation_rotation_replay_expiry_cap_and_issue_rate_are_bounded() {
    let (router, app, calls) = app();
    let primary = paired(&router).await;
    let (_, old) = issue_browser_invite(&router, &primary).await;
    let old_code = old["code"].as_str().unwrap();
    let (_, updated) = issue_browser_invite(&router, &primary).await;
    let next_code = updated["code"].as_str().unwrap();
    assert_ne!(old_code, next_code);
    assert_eq!(pair_browser_code(&router, old_code).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(pair_browser_code(&router, next_code).await.0, StatusCode::OK);
    assert_eq!(pair_browser_code(&router, next_code).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(pair_browser_code(&router, CODE).await.0, StatusCode::UNAUTHORIZED,
        "the original startup code must remain one-use");
    let (_, third) = issue_browser_invite(&router, &primary).await;
    let _ = pair_browser_code(&router, third["code"].as_str().unwrap()).await;
    let (_, fourth) = issue_browser_invite(&router, &primary).await;
    let _ = pair_browser_code(&router, fourth["code"].as_str().unwrap()).await;
    assert_eq!(lock(&app.0.auth).sibling_cookie_hashes.len(), 3);
    assert_eq!(issue_browser_invite(&router, &primary).await.0,
        StatusCode::TOO_MANY_REQUESTS, "original + three sibling sessions is the cap");
    let missing = format!("aw_session_8787={}", "f".repeat(64));
    assert_eq!(router.clone().oneshot(request(
        "GET", "/api/session", serde_json::Value::Null, Some(&missing)
    )).await.unwrap().status(), StatusCode::UNAUTHORIZED);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn expired_invitation_and_owner_expiry_revoke_every_cookie_without_repair() {
    let (router, app, calls) = app();
    let primary = paired(&router).await;
    let secondary = sibling_cookie(&router, &primary).await;
    let (_, fresh) = issue_browser_invite(&router, &primary).await;
    let code = fresh["code"].as_str().unwrap();
    lock(&app.0.auth).invite.as_mut().unwrap().1 = Instant::now();
    assert_eq!(pair_browser_code(&router, code).await.0, StatusCode::UNAUTHORIZED);
    {
        let mut auth = lock(&app.0.auth);
        auth.session_expires = Some(Instant::now());
    }
    for cookie in [&primary, &secondary] {
        assert_eq!(router.clone().oneshot(request(
            "GET", "/api/session", serde_json::Value::Null, Some(cookie),
        )).await.unwrap().status(), StatusCode::UNAUTHORIZED);
    }
    assert_eq!(issue_browser_invite(&router, &primary).await.0,
        StatusCode::UNAUTHORIZED);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn logging_out_either_cookie_revokes_all_existing_and_pending_browsers() {
    let (router, app, calls) = app();
    let primary = paired(&router).await;
    let secondary = sibling_cookie(&router, &primary).await;
    let (_, next) = issue_browser_invite(&router, &primary).await;
    let invite = next["code"].as_str().unwrap();
    assert_eq!(router.clone().oneshot(request(
        "POST", "/api/logout", serde_json::json!({}), Some(&secondary),
    )).await.unwrap().status(), StatusCode::OK);
    for cookie in [&primary, &secondary] {
        assert_eq!(router.clone().oneshot(request(
            "GET", "/api/session", serde_json::Value::Null, Some(cookie)
        )).await.unwrap().status(), StatusCode::UNAUTHORIZED);
    }
    assert_eq!(pair_browser_code(&router, invite).await.0, StatusCode::UNAUTHORIZED);
    assert!(lock(&app.0.auth).sibling_cookie_hashes.is_empty());
    assert!(lock(&app.0.auth).invite.is_none());
    assert!(!auth_alive(&app));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}


#[tokio::test]
async fn cancelling_unused_browser_invitation_revokes_code_without_revoking_paired_clients() {
    let (router, app, calls) = app();
    let primary = paired(&router).await;
    let (status, pending) = issue_browser_invite(&router, &primary).await;
    assert_eq!(status, StatusCode::OK);
    let code = pending["code"].as_str().unwrap();
    assert_eq!(router.clone().oneshot(request(
        "POST", "/api/browser/invite/revoke", serde_json::json!({}), None,
    )).await.unwrap().status(), StatusCode::UNAUTHORIZED);
    let response = router.clone().oneshot(request(
        "POST", "/api/browser/invite/revoke", serde_json::json!({}), Some(&primary),
    )).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(pair_browser_code(&router, code).await.0, StatusCode::UNAUTHORIZED);
    assert!(lock(&app.0.auth).invite.is_none());

    let sibling = sibling_cookie(&router, &primary).await;
    let response = router.clone().oneshot(request(
        "POST", "/api/browser/invite/revoke", serde_json::json!({}), Some(&sibling),
    )).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK,
        "already issued sibling cookies cannot be revoked by cancelling an invitation");
    for cookie in [&primary, &sibling] {
        assert_eq!(router.clone().oneshot(request(
            "GET", "/api/session", serde_json::Value::Null, Some(cookie),
        )).await.unwrap().status(), StatusCode::OK);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn browser_invitation_issuance_is_throttled_independent_of_code_attempts() {
    let (router, app, calls) = app();
    let primary = paired(&router).await;
    let mut codes = Vec::new();
    for _ in 0..MAX_BROWSER_INVITES_PER_MINUTE {
        let (status, invite) = issue_browser_invite(&router, &primary).await;
        assert_eq!(status, StatusCode::OK);
        codes.push(invite["code"].as_str().unwrap().to_owned());
    }
    assert_eq!(issue_browser_invite(&router, &primary).await.0,
        StatusCode::TOO_MANY_REQUESTS);
    for code in codes.iter().take(codes.len() - 1) {
        assert_eq!(pair_browser_code(&router, code).await.0,
            StatusCode::UNAUTHORIZED, "rotated invitation cannot be reused");
    }
    assert_eq!(pair_browser_code(&router, codes.last().unwrap()).await.0, StatusCode::OK);
    assert_eq!(lock(&app.0.auth).sibling_cookie_hashes.len(), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
