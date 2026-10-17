// Docking acceptance uses the real authenticated Axum router and retained
// SurfaceHost, with synthetic Files only. No paid providers, tools or Sessions.
async fn request_dock(
    router: &Router,
    cookie: Option<&str>,
    action: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let reply = router
        .clone()
        .oneshot(request("POST", &format!("/api/dock/{action}"), body, cookie))
        .await
        .unwrap();
    let status = reply.status();
    let bytes = to_bytes(reply.into_body(), 2 * 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn dock_window(router: &Router, cookie: &str) -> serde_json::Value {
    let (status, value) =
        request_dock(router, Some(cookie), "register", serde_json::json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(value["window_id"].as_str().is_some_and(valid_token));
    assert!(value["window_proof"].as_str().is_some_and(valid_token));
    value
}

fn credentials(window: &serde_json::Value, ticket: &str) -> serde_json::Value {
    serde_json::json!({
        "window_id":window["window_id"],
        "window_proof":window["window_proof"],
        "ticket":ticket,
    })
}

fn prepare_body(window: &serde_json::Value, path: &str) -> serde_json::Value {
    serde_json::json!({
        "window_id":window["window_id"],
        "window_proof":window["window_proof"],
        "source_id":window["source_id"],
        "scope_id":window["scope_id"],
        "owner_epoch":window["owner_epoch"],
        "document_id":"workspace-document-37",
        "path":path,
    })
}

async fn dock_prepared(router: &Router, cookie: &str, origin: &serde_json::Value) -> String {
    let (status, response) =
        request_dock(router, Some(cookie), "prepare", prepare_body(origin, "sample.rs")).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let ticket = response["ticket"].as_str().unwrap();
    assert!(valid_token(ticket));
    ticket.to_owned()
}

#[tokio::test]
async fn docking_needs_original_surface_owner_and_authenticated_browser() {
    let (router, _, _) = app();
    assert_eq!(
        request_dock(&router, None, "register", serde_json::json!({})).await.0,
        StatusCode::UNAUTHORIZED
    );
    let cookie = paired(&router).await;
    assert_eq!(
        request_dock(&router, Some(&cookie), "register", serde_json::json!({})).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn read_only_file_handoff_is_two_distinct_windows_and_exact_one_use() {
    let (router, app, calls, _root) = surface_app();
    let cookie = paired(&router).await;
    let before = serde_json::to_value(&lock(&app.0.session.state).snapshot).unwrap();
    let origin = dock_window(&router, &cookie).await;
    let receiver = dock_window(&router, &cookie).await;
    assert_ne!(origin["window_id"], receiver["window_id"]);
    let ticket = dock_prepared(&router, &cookie, &origin).await;
    let original = credentials(&origin, &ticket);
    let receiving = credentials(&receiver, &ticket);
    assert_eq!(
        request_dock(&router, Some(&cookie), "inspect", original.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "status", receiving.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    let (status, descriptor) =
        request_dock(&router, Some(&cookie), "inspect", receiving.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(descriptor["kind"], "file");
    assert_eq!(descriptor["path"], "sample.rs");
    assert_eq!(descriptor["scope_id"], origin["scope_id"]);
    assert_eq!(descriptor["source_id"], origin["source_id"]);
    assert_eq!(descriptor["owner_epoch"], before["instance"]);
    assert!(!descriptor.to_string().contains("<script>"));
    assert_eq!(
        request_dock(&router, Some(&cookie), "release", original.clone()).await.0,
        StatusCode::CONFLICT
    );

    // In real browser operation the receiver independently issues this exact
    // authorized File read and waits for CodeMirror before calling Accept.
    let own_read = router.clone().oneshot(request(
        "POST",
        "/api/workspace/action",
        serde_json::json!({
            "op":"view",
            "source_id":descriptor["source_id"],
            "scope_id":descriptor["scope_id"],
            "request":{"op":"file","path":descriptor["path"]}
        }),
        Some(&cookie),
    )).await.unwrap();
    assert_eq!(own_read.status(), StatusCode::OK);
    let bytes = to_bytes(own_read.into_body(), 5 * 1024 * 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("<script>not HTML</script>"));
    assert_eq!(
        request_dock(&router, Some(&cookie), "accept", receiving.clone()).await.1["state"],
        "accepted"
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "accept", receiving.clone()).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "decline", receiving.clone()).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "cancel", original.clone()).await.0,
        StatusCode::CONFLICT
    );
    let (status, confirmed) =
        request_dock(&router, Some(&cookie), "status", original.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(confirmed["state"], "accepted");
    assert_eq!(confirmed["document_id"], "workspace-document-37");
    assert_eq!(
        request_dock(&router, Some(&cookie), "release", original.clone()).await.1["state"],
        "released"
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "release", original.clone()).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "inspect", receiving).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "status", original).await.1["state"],
        "released"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        serde_json::to_value(&lock(&app.0.session.state).snapshot).unwrap(),
        before
    );
    assert!(lock(&app.0.session.persistence).is_none());
}

#[tokio::test]
async fn docking_forgeries_and_failed_preparation_do_not_create_tokens() {
    let (router, _app, calls, _root) = surface_app();
    let cookie = paired(&router).await;
    let origin = dock_window(&router, &cookie).await;
    for (field, replacement, expected) in [
        ("source_id", serde_json::json!("wrong-source"), StatusCode::FORBIDDEN),
        ("scope_id", serde_json::json!("root"), StatusCode::FORBIDDEN),
        ("owner_epoch", serde_json::json!("stale-epoch"), StatusCode::FORBIDDEN),
        ("path", serde_json::json!("../escape"), StatusCode::BAD_REQUEST),
        ("path", serde_json::json!("/etc/passwd"), StatusCode::BAD_REQUEST),
        ("path", serde_json::json!("missing.rs"), StatusCode::FORBIDDEN),
        ("document_id", serde_json::json!("forged-view"), StatusCode::BAD_REQUEST),
        ("window_proof", serde_json::json!("0".repeat(64)), StatusCode::FORBIDDEN),
    ] {
        let mut body = prepare_body(&origin, "sample.rs");
        body[field] = replacement;
        assert_eq!(
            request_dock(&router, Some(&cookie), "prepare", body).await.0,
            expected,
            "{field}"
        );
    }
    let ticket = dock_prepared(&router, &cookie, &origin).await;
    let mut forged = credentials(&origin, &ticket);
    forged["window_proof"] = serde_json::json!("0".repeat(64));
    assert_eq!(
        request_dock(&router, Some(&cookie), "status", forged).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "status", credentials(&origin, &"1".repeat(64))).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn docking_expiry_cancel_wrong_owner_and_competing_receiver_are_fail_closed() {
    let (router, app, calls, root) = surface_app();
    let cookie = paired(&router).await;
    let origin = dock_window(&router, &cookie).await;
    let receiver_a = dock_window(&router, &cookie).await;
    let receiver_b = dock_window(&router, &cookie).await;
    let ticket = dock_prepared(&router, &cookie, &origin).await;
    let original = credentials(&origin, &ticket);
    let a = credentials(&receiver_a, &ticket);
    let b = credentials(&receiver_b, &ticket);
    let (accepted_a, accepted_b) = tokio::join!(
        request_dock(&router, Some(&cookie), "accept", a.clone()),
        request_dock(&router, Some(&cookie), "accept", b.clone())
    );
    assert_eq!(
        [accepted_a.0, accepted_b.0].iter().filter(|&&status| status == StatusCode::OK).count(),
        1
    );
    assert!([accepted_a.0, accepted_b.0].iter().all(
        |status| matches!(*status, StatusCode::OK | StatusCode::CONFLICT)
    ));
    assert_eq!(
        request_dock(&router, Some(&cookie), "cancel", original.clone()).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "release", original.clone()).await.0,
        StatusCode::OK
    );
    let fresh = dock_prepared(&router, &cookie, &origin).await;
    assert_eq!(
        request_dock(&router, Some(&cookie), "cancel", credentials(&origin, &fresh)).await.1["state"],
        "cancelled"
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "inspect", credentials(&receiver_a, &fresh)).await.0,
        StatusCode::NOT_FOUND
    );

    let stale_receiver = dock_prepared(&router, &cookie, &origin).await;
    assert_eq!(
        request_dock(&router, Some(&cookie), "accept", credentials(&receiver_a, &stale_receiver)).await.0,
        StatusCode::OK
    );
    lock(&app.0.dock).expire_window_for_test(receiver_a["window_id"].as_str().unwrap());
    assert_eq!(
        request_dock(&router, Some(&cookie), "status", credentials(&origin, &stale_receiver)).await.1["state"],
        "receiver_unavailable"
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "release", credentials(&origin, &stale_receiver)).await.0,
        StatusCode::CONFLICT
    );

    let expired = dock_prepared(&router, &cookie, &origin).await;
    lock(&app.0.dock).expire_ticket_for_test(&expired);
    assert_eq!(
        request_dock(&router, Some(&cookie), "inspect", credentials(&receiver_b, &expired)).await.0,
        StatusCode::NOT_FOUND
    );
    let expired_window = dock_window(&router, &cookie).await;
    lock(&app.0.dock).expire_window_for_test(expired_window["window_id"].as_str().unwrap());
    assert_eq!(
        request_dock(&router, Some(&cookie), "prepare", prepare_body(&expired_window, "sample.rs")).await.0,
        StatusCode::FORBIDDEN
    );
    let (new_router, _, _, _new_root) = surface_app();
    let new_cookie = paired(&new_router).await;
    assert_eq!(
        request_dock(&new_router, Some(&new_cookie), "inspect", a).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(root.0.join("sample.rs").exists());
}

#[tokio::test]
async fn docking_revocation_and_replaced_root_never_disclose_replacement_bytes() {
    let (router, app, calls, root) = surface_app();
    let cookie = paired(&router).await;
    let origin = dock_window(&router, &cookie).await;
    let receiver = dock_window(&router, &cookie).await;
    let ticket = dock_prepared(&router, &cookie, &origin).await;
    let moved = root.0.with_extension("backup");
    std::fs::rename(&root.0, &moved).unwrap();
    std::fs::create_dir(&root.0).unwrap();
    std::fs::write(root.0.join("sample.rs"), "replacement-private-content").unwrap();
    let denial = request_dock(
        &router, Some(&cookie), "inspect", credentials(&receiver, &ticket)
    ).await;
    assert_eq!(denial.0, StatusCode::FORBIDDEN);
    assert!(!denial.1.to_string().contains("replacement-private-content"));
    std::fs::remove_dir_all(&root.0).unwrap();
    std::fs::rename(&moved, &root.0).unwrap();

    lock(&app.0.auth).cookie_hash = None;
    assert_eq!(
        request_dock(&router, Some(&cookie), "inspect", credentials(&receiver, &ticket)).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "status", credentials(&origin, &ticket)).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn accepted_handoff_rechecks_file_before_original_release() {
    let (router, app, calls, root) = surface_app();
    let cookie = paired(&router).await;
    let origin = dock_window(&router, &cookie).await;
    let receiver = dock_window(&router, &cookie).await;
    let ticket = dock_prepared(&router, &cookie, &origin).await;
    let original = credentials(&origin, &ticket);
    assert_eq!(
        request_dock(&router, Some(&cookie), "accept", credentials(&receiver, &ticket)).await.0,
        StatusCode::OK
    );
    let file = root.0.join("sample.rs");
    let saved = root.0.join("saved-synthetic.rs");
    std::fs::rename(&file, &saved).unwrap();
    assert_eq!(
        request_dock(&router, Some(&cookie), "release", original.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "status", original.clone()).await.1["state"],
        "accepted"
    );
    std::fs::rename(&saved, &file).unwrap();

    let moved = root.0.with_extension("original-pinned");
    std::fs::rename(&root.0, &moved).unwrap();
    std::fs::create_dir(&root.0).unwrap();
    std::fs::write(root.0.join("sample.rs"), "untrusted replacement").unwrap();
    assert_eq!(
        request_dock(&router, Some(&cookie), "release", original.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    std::fs::remove_dir_all(&root.0).unwrap();
    std::fs::rename(&moved, &root.0).unwrap();
    assert_eq!(
        request_dock(&router, Some(&cookie), "release", original.clone()).await.0,
        StatusCode::OK
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "status", original).await.1["state"],
        "released"
    );
    assert!(lock(&app.0.session.persistence).is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn declined_receiver_aborts_pending_handoff_without_erasing_other_views() {
    let (router, app, calls, _root) = surface_app();
    let cookie = paired(&router).await;
    let before = serde_json::to_value(&lock(&app.0.session.state).snapshot).unwrap();
    let origin = dock_window(&router, &cookie).await;
    let refused = dock_window(&router, &cookie).await;
    let other = dock_window(&router, &cookie).await;
    let ticket = dock_prepared(&router, &cookie, &origin).await;
    let original = credentials(&origin, &ticket);
    let rejection = credentials(&refused, &ticket);
    assert_eq!(
        request_dock(&router, Some(&cookie), "decline", original.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "decline", rejection.clone()).await.1["state"],
        "declined"
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "status", original.clone()).await.1["state"],
        "declined"
    );
    for candidate in [&refused, &other] {
        assert_eq!(
            request_dock(&router, Some(&cookie), "accept", credentials(candidate, &ticket)).await.0,
            StatusCode::CONFLICT
        );
    }
    assert_eq!(
        request_dock(&router, Some(&cookie), "release", original.clone()).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "cancel", original.clone()).await.1["state"],
        "cancelled"
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "status", original.clone()).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request_dock(&router, Some(&cookie), "decline", rejection).await.0,
        StatusCode::NOT_FOUND
    );

    let accepted = dock_prepared(&router, &cookie, &origin).await;
    assert_eq!(
        request_dock(&router, Some(&cookie), "accept", credentials(&other, &accepted)).await.0,
        StatusCode::OK
    );
    for candidate in [&other, &refused] {
        assert_eq!(
            request_dock(&router, Some(&cookie), "decline", credentials(candidate, &accepted)).await.0,
            StatusCode::CONFLICT
        );
    }
    assert_eq!(
        request_dock(&router, Some(&cookie), "release", credentials(&origin, &accepted)).await.0,
        StatusCode::OK
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        serde_json::to_value(&lock(&app.0.session.state).snapshot).unwrap(),
        before
    );
}

fn synthetic_file_presentation(text: &str) -> serde_json::Value {
    let digest = hash(format!("file\0{text}").as_bytes())
        .iter().map(|b| format!("{b:02x}")).collect::<String>();
    serde_json::json!({"version":1,"fingerprint":digest,"query":"not HTML",
        "selected":0,"focus":"find",
        "panes":[{"anchor":3,"head":7,"from":1,"top":245,"left":0}]})
}
#[tokio::test]
async fn file_presentation_is_bounded_and_rechecked_on_owner_read_and_release() {
    let (router,state,calls,root)=surface_app();
    let cookie=paired(&router).await;
    let origin=dock_window(&router,&cookie).await;
    let receiver=dock_window(&router,&cookie).await;
    let original=std::fs::read_to_string(root.0.join("sample.rs")).unwrap();
    let view=synthetic_file_presentation(&original);
    let mut prepared_body=prepare_body(&origin,"sample.rs");
    prepared_body["presentation"]=view.clone();
    let (status,prepared)=request_dock(&router,Some(&cookie),"prepare",prepared_body).await;
    assert_eq!(status,StatusCode::OK,"{prepared}");
    assert!(prepared.get("presentation").is_none(),"sender does not receive search query");
    let ticket=prepared["ticket"].as_str().unwrap();
    let origin_env=credentials(&origin,ticket);
    let receiver_env=credentials(&receiver,ticket);
    let (status,descriptor)=request_dock(&router,Some(&cookie),"inspect",receiver_env.clone()).await;
    assert_eq!(status,StatusCode::OK);
    assert_eq!(descriptor["presentation"],view);
    assert!(!descriptor.to_string().contains(&original),"source plaintext never transferred");
    std::fs::write(root.0.join("sample.rs"),"changed authorized source bytes").unwrap();
    assert_eq!(request_dock(&router,Some(&cookie),"accept",receiver_env.clone()).await.0,
        StatusCode::CONFLICT);
    assert_eq!(request_dock(&router,Some(&cookie),"status",origin_env.clone()).await.1["state"],
        "prepared");
    std::fs::write(root.0.join("sample.rs"),&original).unwrap();
    assert_eq!(request_dock(&router,Some(&cookie),"accept",receiver_env).await.0,StatusCode::OK);
    std::fs::write(root.0.join("sample.rs"),"changed after receiver accepted").unwrap();
    assert_eq!(request_dock(&router,Some(&cookie),"release",origin_env.clone()).await.0,
        StatusCode::CONFLICT);
    std::fs::write(root.0.join("sample.rs"),&original).unwrap();
    assert_eq!(request_dock(&router,Some(&cookie),"release",origin_env).await.1["state"],
        "released");
    assert!(lock(&state.0.session.persistence).is_none());
    assert_eq!(calls.load(Ordering::SeqCst),0);
}
#[tokio::test]
async fn malformed_presentation_rejected_before_ticket_admission() {
    let (router,_state,calls,root)=surface_app();
    let cookie=paired(&router).await;
    let origin=dock_window(&router,&cookie).await;
    let data=std::fs::read_to_string(root.0.join("sample.rs")).unwrap();
    let valid=synthetic_file_presentation(&data);
    for (key,replacement) in [
        ("version",serde_json::json!(2)),
        ("fingerprint",serde_json::json!("e".repeat(63))),
        ("fingerprint",serde_json::json!("G".repeat(64))),
        ("query",serde_json::json!("long".repeat(150))),
        ("query",serde_json::json!("contains\nnewline")),
        ("selected",serde_json::json!(4096)),
        ("selected",serde_json::json!(-2)),
        ("focus",serde_json::json!("pane1")),
        ("focus",serde_json::json!("invalid")),
        ("panes",serde_json::json!([])),
        ("panes",serde_json::json!([{"anchor":4194305,"head":0,"from":0,"top":0,"left":0}])),
        ("panes",serde_json::json!([{"anchor":0,"head":0,"from":0,"top":33554433,"left":0}])),
    ] {
        let mut presentation=valid.clone();
        presentation[key]=replacement;
        let mut body=prepare_body(&origin,"sample.rs");
        body["presentation"]=presentation;
        assert_eq!(request_dock(&router,Some(&cookie),"prepare",body).await.0,
            StatusCode::BAD_REQUEST,"{key}");
    }
    let mut extra=prepare_body(&origin,"sample.rs");
    let mut presentation=valid;
    presentation["content"]=serde_json::json!("source text not permitted");
    extra["presentation"]=presentation;
    assert_eq!(request_dock(&router,Some(&cookie),"prepare",extra).await.0,
        StatusCode::BAD_REQUEST);
    assert_eq!(calls.load(Ordering::SeqCst),0);
}
