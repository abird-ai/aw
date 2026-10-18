// Failure-oriented tests of original-owner cross-window presentation handoffs.
// Source/receiver proofs and tickets are ephemeral; tests never activate effects.

#[tokio::test]
async fn retired_accepted_receiver_and_replacement_window_cannot_release_origin() {
    let (router, app, calls, _root) = surface_app();
    let cookie = paired(&router).await;
    let origin = dock_window(&router, &cookie).await;
    let receiver = dock_window(&router, &cookie).await;
    let another = dock_window(&router, &cookie).await;
    let ticket = dock_prepared(&router, &cookie, &origin).await;
    let source = credentials(&origin, &ticket);
    let receiving = credentials(&receiver, &ticket);
    assert_eq!(request_dock(&router, Some(&cookie), "accept", receiving.clone()).await.0,
        StatusCode::OK);
    assert_eq!(request_dock(&router, Some(&cookie), "status", source.clone()).await.1["state"],
        "confirming");
    assert_eq!(request_dock(&router, Some(&cookie), "release", source.clone()).await.0,
        StatusCode::CONFLICT, "merely accepted window must not be considered live");
    assert_eq!(request_dock(&router, Some(&cookie), "confirm", source.clone()).await.0,
        StatusCode::OK);
    assert_eq!(request_dock(&router, Some(&cookie), "heartbeat",
        credentials(&another, &ticket)).await.0, StatusCode::CONFLICT,
        "another registered browser must not attest for the accepted browser");
    assert_eq!(request_dock(&router, Some(&cookie), "retire", receiving.clone()).await.1["state"],
        "retired");
    assert_eq!(request_dock(&router, Some(&cookie), "status", source.clone()).await.1["state"],
        "receiver_unavailable");
    assert_eq!(request_dock(&router, Some(&cookie), "heartbeat", receiving.clone()).await.0,
        StatusCode::FORBIDDEN, "a retired proof cannot revive a tab");
    assert_eq!(request_dock(&router, Some(&cookie), "release", source.clone()).await.0,
        StatusCode::CONFLICT);
    assert_eq!(request_dock(&router, Some(&cookie), "accept",
        credentials(&another, &ticket)).await.0, StatusCode::CONFLICT,
        "new browser cannot replace an accepted receiver");
    assert_eq!(request_dock(&router, Some(&cookie), "confirm", source.clone()).await.0,
        StatusCode::CONFLICT);
    let newer = dock_prepared(&router, &cookie, &origin).await;
    confirm_dock_after_accept(&router, &cookie, &credentials(&origin, &newer),
        &credentials(&another, &newer)).await;
    assert_eq!(request_dock(&router, Some(&cookie), "release",
        credentials(&origin, &newer)).await.0, StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(lock(&app.0.session.persistence).is_none());
}

async fn confirm_dock_after_accept(
    router: &Router, cookie: &str,
    origin: &serde_json::Value, receiver: &serde_json::Value,
) {
    assert_eq!(request_dock(router, Some(cookie), "accept", receiver.clone()).await.0,
        StatusCode::OK);
    confirm_dock(router, cookie, origin, receiver).await;
}

#[tokio::test]
async fn stale_receiver_ack_requires_new_live_response_and_never_replays_effects() {
    let (router, app, calls, _root) = surface_app();
    let cookie = paired(&router).await;
    let origin = dock_window(&router, &cookie).await;
    let receiver = dock_window(&router, &cookie).await;
    let ticket = dock_prepared(&router, &cookie, &origin).await;
    let source = credentials(&origin, &ticket);
    let receiving = credentials(&receiver, &ticket);
    confirm_dock_after_accept(&router, &cookie, &source, &receiving).await;
    lock(&app.0.dock).stale_ack_for_test(&ticket);
    assert_eq!(request_dock(&router, Some(&cookie), "status", source.clone()).await.1["state"],
        "confirming");
    assert_eq!(request_dock(&router, Some(&cookie), "release", source.clone()).await.0,
        StatusCode::CONFLICT);
    assert_eq!(request_dock(&router, Some(&cookie), "confirm", source.clone()).await.0,
        StatusCode::OK, "duplicate confirm should not reset a still usable gate");
    assert_eq!(request_dock(&router, Some(&cookie), "heartbeat", receiving.clone()).await.1["state"],
        "accepted");
    assert_eq!(request_dock(&router, Some(&cookie), "release", source.clone()).await.0,
        StatusCode::OK);
    assert_eq!(request_dock(&router, Some(&cookie), "heartbeat", receiving).await.1["state"],
        "released");
    assert_eq!(request_dock(&router, Some(&cookie), "release", source).await.0,
        StatusCode::CONFLICT);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(lock(&app.0.session.persistence).is_none());
}

#[tokio::test]
async fn browser_exit_before_accept_retired_ticket_is_declined_and_not_reopenable() {
    let (router, app, calls, _root) = surface_app();
    let cookie = paired(&router).await;
    let origin = dock_window(&router, &cookie).await;
    let receiver = dock_window(&router, &cookie).await;
    let ticket = dock_prepared(&router, &cookie, &origin).await;
    let source = credentials(&origin, &ticket);
    let receiving = credentials(&receiver, &ticket);
    assert_eq!(request_dock(&router, Some(&cookie), "retire", receiving.clone()).await.0,
        StatusCode::OK);
    assert_eq!(request_dock(&router, Some(&cookie), "status", source.clone()).await.1["state"],
        "declined");
    assert_eq!(request_dock(&router, Some(&cookie), "accept", receiving.clone()).await.0,
        StatusCode::FORBIDDEN);
    assert_eq!(request_dock(&router, Some(&cookie), "release", source.clone()).await.0,
        StatusCode::CONFLICT);
    assert_eq!(request_dock(&router, Some(&cookie), "cancel", source.clone()).await.0,
        StatusCode::OK);
    assert_eq!(request_dock(&router, Some(&cookie), "status", source).await.0,
        StatusCode::NOT_FOUND);
    let fresh = dock_window(&router, &cookie).await;
    assert_ne!(fresh["window_id"], receiver["window_id"]);
    assert_ne!(fresh["window_proof"], receiver["window_proof"]);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(lock(&app.0.session.persistence).is_none());
}

#[tokio::test]
async fn concurrent_receiver_retire_accept_and_wrong_cookie_fail_closed() {
    let (router, app, calls, _root) = surface_app();
    let cookie = paired(&router).await;
    let origin = dock_window(&router, &cookie).await;
    let receiver = dock_window(&router, &cookie).await;
    let ticket = dock_prepared(&router, &cookie, &origin).await;
    let source = credentials(&origin, &ticket);
    let receiving = credentials(&receiver, &ticket);
    let (accept, retire) = tokio::join!(
        request_dock(&router, Some(&cookie), "accept", receiving.clone()),
        request_dock(&router, Some(&cookie), "retire", receiving.clone()),
    );
    assert_eq!(retire.0, StatusCode::OK);
    assert!(matches!(accept.0, StatusCode::OK | StatusCode::FORBIDDEN | StatusCode::CONFLICT));
    assert_eq!(request_dock(&router, Some(&cookie), "release", source.clone()).await.0,
        StatusCode::CONFLICT);
    assert_eq!(request_dock(&router, None, "confirm", source.clone()).await.0,
        StatusCode::UNAUTHORIZED);
    let bogus = format!("{}={}", app.0.cookie_name, "f".repeat(64));
    assert_eq!(request_dock(&router, Some(&bogus), "confirm", source.clone()).await.0,
        StatusCode::UNAUTHORIZED);
    assert_eq!(request_dock(&router, Some(&cookie), "heartbeat", receiving).await.0,
        StatusCode::FORBIDDEN);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(lock(&app.0.session.persistence).is_none());
}

#[tokio::test]
async fn restarting_owner_does_not_recover_ephemeral_docking_window_or_ticket() {
    let (router, _app, calls, _original_root) = surface_app();
    let cookie = paired(&router).await;
    let origin = dock_window(&router, &cookie).await;
    let receiver = dock_window(&router, &cookie).await;
    let ticket = dock_prepared(&router, &cookie, &origin).await;
    let old_origin = credentials(&origin, &ticket);
    let old_receiver = credentials(&receiver, &ticket);
    let (fresh_router, _new_app, new_calls, _root) = surface_app();
    let fresh_cookie = paired(&fresh_router).await;
    // The new owner has different source/epoch and no ticket/window registry.
    assert_eq!(request_dock(&fresh_router, Some(&fresh_cookie),
        "heartbeat", old_receiver.clone()).await.0, StatusCode::FORBIDDEN);
    assert_eq!(request_dock(&fresh_router, Some(&fresh_cookie),
        "confirm", old_origin.clone()).await.0, StatusCode::FORBIDDEN);
    assert_eq!(request_dock(&fresh_router, Some(&fresh_cookie),
        "release", old_origin).await.0, StatusCode::FORBIDDEN);
    assert_eq!(request_dock(&fresh_router, Some(&fresh_cookie),
        "retire", old_receiver).await.0, StatusCode::FORBIDDEN);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(new_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn window_and_ticket_capacity_are_bounded_and_reclaimable() {
    let (router, state, calls, _root) = surface_app();
    let cookie = paired(&router).await;
    let mut windows = Vec::new();
    for _ in 0..24 {
        windows.push(dock_window(&router, &cookie).await);
    }
    assert_eq!(
        request_dock(&router, Some(&cookie), "register", serde_json::json!({})).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    lock(&state.0.dock).expire_window_for_test(
        windows.last().unwrap()["window_id"].as_str().unwrap(),
    );
    let reopened = dock_window(&router, &cookie).await;
    assert_ne!(reopened["window_id"], windows.last().unwrap()["window_id"]);
    assert_ne!(reopened["window_proof"], windows.last().unwrap()["window_proof"]);
    let owner = &windows[0];
    let mut tickets = Vec::new();
    for _ in 0..24 {
        tickets.push(dock_prepared(&router, &cookie, owner).await);
    }
    assert_eq!(
        request_dock(&router, Some(&cookie), "prepare",
            prepare_body(owner, "sample.rs")).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "cancel",
            credentials(owner, &tickets[0])).await.0,
        StatusCode::OK
    );
    let replacement = dock_prepared(&router, &cookie, owner).await;
    assert_ne!(replacement, tickets[0]);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(lock(&state.0.session.persistence).is_none());
}


#[cfg(feature = "surfaces")]
#[tokio::test]
async fn two_separately_paired_cookie_jars_can_hand_off_exact_file_under_one_owner() {
    let (router, app, calls, _root) = surface_app();
    let primary = paired(&router).await;
    let secondary = super::browser_pairing::sibling_cookie(&router, &primary).await;
    let origin = dock_window(&router, &primary).await;
    let receiver = dock_window(&router, &secondary).await;
    let ticket = dock_prepared(&router, &primary, &origin).await;
    let original = credentials(&origin, &ticket);
    let receiving = credentials(&receiver, &ticket);
    assert_eq!(request_dock(&router, Some(&secondary), "inspect",
        receiving.clone()).await.1["path"], "sample.rs");
    // The registered sibling has its *own* cookie but the exact same
    // preexisting owner actor/scope; it independently authorizes the File read.
    let scoped = router.clone().oneshot(request(
        "POST", "/api/workspace/action",
        serde_json::json!({"op":"view",
            "source_id":origin["source_id"],"scope_id":origin["scope_id"],
            "request":{"op":"file","path":"sample.rs"}}),
        Some(&secondary),
    )).await.unwrap();
    assert_eq!(scoped.status(), StatusCode::OK);
    assert_eq!(request_dock(&router, Some(&secondary), "accept",
        receiving.clone()).await.1["state"], "accepted");
    assert_eq!(request_dock(&router, Some(&primary), "confirm",
        original.clone()).await.1["state"], "confirming");
    assert_eq!(request_dock(&router, Some(&primary), "release",
        original.clone()).await.0, StatusCode::CONFLICT);
    assert_eq!(request_dock(&router, Some(&secondary), "heartbeat",
        receiving.clone()).await.1["state"], "accepted");
    assert_eq!(request_dock(&router, Some(&primary), "release",
        original.clone()).await.1["state"], "released");
    assert_eq!(request_dock(&router, Some(&secondary), "heartbeat",
        receiving).await.1["state"], "released");
    assert!(lock(&app.0.session.persistence).is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn separate_cookie_profiles_cannot_release_after_receiver_retirement_post_ack() {
    let (router, app, calls, _root) = surface_app();
    let primary = paired(&router).await;
    let secondary = super::browser_pairing::sibling_cookie(&router, &primary).await;
    let origin = dock_window(&router, &primary).await;
    let receiver = dock_window(&router, &secondary).await;
    let ticket = dock_prepared(&router, &primary, &origin).await;
    let source = credentials(&origin, &ticket);
    let recipient = credentials(&receiver, &ticket);
    assert_eq!(request_dock(&router, Some(&secondary), "accept", recipient.clone()).await.0,
        StatusCode::OK);
    assert_eq!(request_dock(&router, Some(&primary), "confirm", source.clone()).await.0,
        StatusCode::OK);
    assert_eq!(request_dock(&router, Some(&secondary), "heartbeat", recipient.clone()).await.1["state"],
        "accepted");
    // Owner processes a real accepted-window retirement after fresh ACK,
    // before the original commits release. Browser registration alone cannot
    // revive even an ACK younger than three seconds.
    assert_eq!(request_dock(&router, Some(&secondary), "retire", recipient.clone()).await.0,
        StatusCode::OK);
    assert_eq!(request_dock(&router, Some(&primary), "status", source.clone()).await.1["state"],
        "receiver_unavailable");
    assert_eq!(request_dock(&router, Some(&primary), "release", source.clone()).await.0,
        StatusCode::CONFLICT);
    assert_eq!(request_dock(&router, Some(&secondary), "heartbeat", recipient).await.0,
        StatusCode::FORBIDDEN);
    let replacement = dock_window(&router, &secondary).await;
    assert_ne!(replacement["window_id"], receiver["window_id"]);
    assert_eq!(request_dock(&router, Some(&secondary), "heartbeat",
        credentials(&replacement, &ticket)).await.0, StatusCode::CONFLICT);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(lock(&app.0.session.persistence).is_none());
}

#[tokio::test]
async fn separate_cookie_profiles_release_retire_race_is_linearized_without_double_release() {
    let (router, app, calls, _root) = surface_app();
    let primary = paired(&router).await;
    let secondary = super::browser_pairing::sibling_cookie(&router, &primary).await;
    let origin = dock_window(&router, &primary).await;
    let receiver = dock_window(&router, &secondary).await;
    let ticket = dock_prepared(&router, &primary, &origin).await;
    let source = credentials(&origin, &ticket);
    let recipient = credentials(&receiver, &ticket);
    assert_eq!(request_dock(&router, Some(&secondary), "accept", recipient.clone()).await.0,
        StatusCode::OK);
    assert_eq!(request_dock(&router, Some(&primary), "confirm", source.clone()).await.0,
        StatusCode::OK);
    assert_eq!(request_dock(&router, Some(&secondary), "heartbeat", recipient.clone()).await.0,
        StatusCode::OK);
    let (released, retired) = tokio::join!(
        request_dock(&router, Some(&primary), "release", source.clone()),
        request_dock(&router, Some(&secondary), "retire", recipient.clone()),
    );
    assert_eq!(retired.0, StatusCode::OK);
    assert!(matches!(released.0, StatusCode::OK | StatusCode::CONFLICT));
    let status = request_dock(&router, Some(&primary), "status", source.clone()).await;
    assert_eq!(status.0, StatusCode::OK);
    if released.0 == StatusCode::OK {
        // The receiver could still crash immediately after the release
        // commit: this cannot be prevented by a short transport heartbeat.
        assert_eq!(status.1["state"], "released");
    } else {
        assert_eq!(status.1["state"], "receiver_unavailable");
    }
    assert_eq!(request_dock(&router, Some(&primary), "release", source).await.0,
        StatusCode::CONFLICT, "once committed or retired, no second release is possible");
    assert_eq!(request_dock(&router, Some(&secondary), "heartbeat", recipient).await.0,
        StatusCode::FORBIDDEN);
    assert!(lock(&app.0.session.persistence).is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn logout_from_invited_browser_invalidates_inflight_owner_dock_receipt() {
    let (router, app, calls, _root) = surface_app();
    let primary = paired(&router).await;
    let secondary = super::browser_pairing::sibling_cookie(&router, &primary).await;
    let origin = dock_window(&router, &primary).await;
    let receiver = dock_window(&router, &secondary).await;
    let ticket = dock_prepared(&router, &primary, &origin).await;
    let source = credentials(&origin, &ticket);
    let recipient = credentials(&receiver, &ticket);
    assert_eq!(request_dock(&router, Some(&secondary), "accept", recipient.clone()).await.0,
        StatusCode::OK);
    assert_eq!(request_dock(&router, Some(&primary), "confirm", source.clone()).await.0,
        StatusCode::OK);
    assert_eq!(request_dock(&router, Some(&secondary), "heartbeat", recipient.clone()).await.0,
        StatusCode::OK);
    let logout = router.clone().oneshot(request("POST", "/api/logout",
        serde_json::json!({}), Some(&secondary))).await.unwrap();
    assert_eq!(logout.status(), StatusCode::OK);
    assert_eq!(request_dock(&router, Some(&primary), "release", source.clone()).await.0,
        StatusCode::UNAUTHORIZED);
    assert_eq!(request_dock(&router, Some(&secondary), "heartbeat", recipient).await.0,
        StatusCode::UNAUTHORIZED);
    assert!(!auth_alive(&app));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
