// Real authenticated retained Git Review docking, independent of File grants.
// All Git changes are made only in ephemeral private fixture checkouts.
fn fixture_git(root: &std::path::Path, args: &[&str]) -> String {
    let git = std::env::var_os("AW_GIT").expect("pinned Nix AW_GIT");
    let output = std::process::Command::new(git)
        .arg("-C").arg(root).args(args)
        .env("GIT_TERMINAL_PROMPT", "0").output().unwrap();
    assert!(output.status.success(), "synthetic Git failed: {}",
        String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
fn init_review_fixture(root: &std::path::Path) {
    fixture_git(root, &["init", "-b", "main"]);
    fixture_git(root, &["config", "user.name", "Fixture"]);
    fixture_git(root, &["config", "user.email", "fixture@example.invalid"]);
    fixture_git(root, &["add", "sample.rs"]);
    fixture_git(root, &["commit", "-m", "synthetic baseline"]);
    std::fs::write(root.join("sample.rs"), "synthetic retained change alpha\n").unwrap();
}
async fn dock_view(
    router: &Router, cookie: &str, origin: &serde_json::Value,
    scope: &str, operation: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let reply=router.clone().oneshot(request("POST","/api/workspace/action",
        serde_json::json!({
            "op":"view","source_id":origin["source_id"],
            "scope_id":scope,"request":operation
        }),Some(cookie))).await.unwrap();
    let status=reply.status();
    let bytes=to_bytes(reply.into_body(),5*1024*1024).await.unwrap();
    (status,serde_json::from_slice(&bytes).unwrap())
}
async fn retained_review(
    router: &Router, cookie: &str, origin: &serde_json::Value, scope: &str,
) -> serde_json::Value {
    let (status, changes)=dock_view(router,cookie,origin,scope,
        serde_json::json!({"op":"git_list","mode":"changes"})).await;
    assert_eq!(status,StatusCode::OK,"{changes}");
    let list=&changes["result"];
    assert!(!list["items"].as_array().unwrap().is_empty());
    let (status,review)=dock_view(router,cookie,origin,scope,
        serde_json::json!({"op":"git_open",
            "inventory_id":list["inventory_id"],"item_id":list["items"][0]["id"]})).await;
    assert_eq!(status,StatusCode::OK,"{review}");
    assert_eq!(review["result"]["kind"],"review");
    assert_eq!(review["scope_id"],scope);
    review["result"].clone()
}
fn review_prepare_body(origin: &serde_json::Value, scope: &str, review_id: &str)
    -> serde_json::Value {
    serde_json::json!({
        "window_id":origin["window_id"],"window_proof":origin["window_proof"],
        "source_id":origin["source_id"],"scope_id":scope,
        "owner_epoch":origin["owner_epoch"],"document_id":"workspace-document-44",
        "review_id":review_id
    })
}
#[tokio::test]
async fn immutable_review_handoff_keeps_exact_retained_source_and_no_effects() {
    let (router,state,calls,root)=surface_app();
    let cookie=paired(&router).await;
    init_review_fixture(&root.0);
    let old_head=fixture_git(&root.0,&["rev-parse","HEAD"]);
    let index=fixture_git(&root.0,&["rev-parse","--path-format=absolute","--git-path","index"]);
    let old_index=std::fs::read(&index).unwrap();
    let before=serde_json::to_value(&lock(&state.0.session.state).snapshot).unwrap();
    let origin=dock_window(&router,&cookie).await;
    let receiver=dock_window(&router,&cookie).await;
    let scope=origin["scope_id"].as_str().unwrap();
    let review=retained_review(&router,&cookie,&origin,scope).await;
    let id=review["id"].as_str().unwrap();
    assert!(review.to_string().contains("synthetic retained change alpha"));
    let (status,prepared)=request_dock(&router,Some(&cookie),"prepare-review",
        review_prepare_body(&origin,scope,id)).await;
    assert_eq!(status,StatusCode::OK,"{prepared}");
    let ticket=prepared["ticket"].as_str().unwrap();
    let source=credentials(&origin,ticket);
    let dest=credentials(&receiver,ticket);
    let (status,desc)=request_dock(&router,Some(&cookie),"inspect",dest.clone()).await;
    assert_eq!(status,StatusCode::OK,"{desc}");
    assert_eq!(desc["kind"],"review");
    assert_eq!(desc["review_id"],id);
    assert_eq!(desc["scope_id"],scope);
    assert!(desc.get("path").is_none());
    assert!(!desc.to_string().contains("synthetic retained change alpha"));
    let (status,actual)=dock_view(&router,&cookie,&origin,scope,
        serde_json::json!({"op":"review","review_id":id})).await;
    assert_eq!(status,StatusCode::OK);
    assert_eq!(actual["result"],review);
    assert_eq!(request_dock(&router,Some(&cookie),"release",source.clone()).await.0,
        StatusCode::CONFLICT);
    assert_eq!(request_dock(&router,Some(&cookie),"accept",dest.clone()).await.1["state"],
        "accepted");
    confirm_dock(&router,&cookie,&source,&dest).await;
    assert_eq!(request_dock(&router,Some(&cookie),"release",source.clone()).await.1["state"],
        "released");
    assert_eq!(request_dock(&router,Some(&cookie),"accept",dest).await.0,
        StatusCode::CONFLICT);
    assert_eq!(fixture_git(&root.0,&["rev-parse","HEAD"]),old_head);
    assert_eq!(std::fs::read(index).unwrap(),old_index);
    assert_eq!(calls.load(Ordering::SeqCst),0);
    assert_eq!(serde_json::to_value(&lock(&state.0.session.state).snapshot).unwrap(),before);
    assert!(lock(&state.0.session.persistence).is_none());
}
#[tokio::test]
async fn retained_review_refuses_forged_id_scope_and_revoked_host() {
    let (router,state,calls,root)=surface_app();
    let cookie=paired(&router).await;
    init_review_fixture(&root.0);
    let origin=dock_window(&router,&cookie).await;
    let receiver=dock_window(&router,&cookie).await;
    let scope=origin["scope_id"].as_str().unwrap();
    let review=retained_review(&router,&cookie,&origin,scope).await;
    let id=review["id"].as_str().unwrap();
    for (field,value,expected) in [
        ("scope_id",serde_json::json!("root"),StatusCode::FORBIDDEN),
        ("scope_id",serde_json::json!("unknown_scope"),StatusCode::FORBIDDEN),
        ("source_id",serde_json::json!("foreign"),StatusCode::FORBIDDEN),
        ("owner_epoch",serde_json::json!("stale"),StatusCode::FORBIDDEN),
        ("review_id",serde_json::json!("invalid_review"),StatusCode::FORBIDDEN),
        ("review_id",serde_json::json!("../bad"),StatusCode::BAD_REQUEST),
    ] {
        let mut body=review_prepare_body(&origin,scope,id);
        body[field]=value;
        assert_eq!(request_dock(&router,Some(&cookie),"prepare-review",body).await.0,
            expected,"{field}");
    }
    let mut forged=review_prepare_body(&origin,scope,id);
    forged["path"]=serde_json::json!("sample.rs");
    assert_eq!(request_dock(&router,Some(&cookie),"prepare-review",forged).await.0,
        StatusCode::BAD_REQUEST);
    let (status,prepared)=request_dock(&router,Some(&cookie),"prepare-review",
        review_prepare_body(&origin,scope,id)).await;
    assert_eq!(status,StatusCode::OK);
    let ticket=prepared["ticket"].as_str().unwrap();
    state.0.surfaces.as_ref().unwrap().shutdown();
    for (operation,window) in [("inspect",&receiver),("accept",&receiver),("release",&origin)] {
        assert_eq!(request_dock(&router,Some(&cookie),operation,
            credentials(window,ticket)).await.0,StatusCode::FORBIDDEN);
    }
    assert_eq!(calls.load(Ordering::SeqCst),0);
}
#[tokio::test]
async fn review_handoff_retains_nonroot_git_worktree_scope_without_files_grant() {
    let (router,state,calls,root)=surface_app();
    let cookie=paired(&router).await;
    init_review_fixture(&root.0);
    let linked=root.0.with_extension("linked-review-fixture");
    fixture_git(&root.0,&["worktree","add","-b","other",linked.to_str().unwrap()]);
    std::fs::write(linked.join("sample.rs"),"synthetic linked-only beta\n").unwrap();
    let origin=dock_window(&router,&cookie).await;
    let receiver=dock_window(&router,&cookie).await;
    let root_scope=origin["scope_id"].as_str().unwrap();
    let (status,worktrees)=dock_view(&router,&cookie,&origin,root_scope,
        serde_json::json!({"op":"git_list","mode":"worktrees"})).await;
    assert_eq!(status,StatusCode::OK);
    let worktrees=&worktrees["result"];
    let item=worktrees["items"].as_array().unwrap().iter()
        .find(|item|item["path"].as_str()==linked.to_str()).unwrap();
    let (status,selected)=dock_view(&router,&cookie,&origin,root_scope,
        serde_json::json!({"op":"git_open",
            "inventory_id":worktrees["inventory_id"],"item_id":item["id"]})).await;
    assert_eq!(status,StatusCode::OK);
    let scope=selected["result"]["context"]["id"].as_str().unwrap();
    assert_ne!(scope,root_scope);
    let review=retained_review(&router,&cookie,&origin,scope).await;
    let id=review["id"].as_str().unwrap();
    assert!(review.to_string().contains("synthetic linked-only beta"));
    assert!(!review.to_string().contains("synthetic retained change alpha"));
    let (status,prepared)=request_dock(&router,Some(&cookie),"prepare-review",
        review_prepare_body(&origin,scope,id)).await;
    assert_eq!(status,StatusCode::OK,"{prepared}");
    let ticket=prepared["ticket"].as_str().unwrap();
    let (status,desc)=request_dock(&router,Some(&cookie),"inspect",
        credentials(&receiver,ticket)).await;
    assert_eq!(status,StatusCode::OK);
    assert_eq!(desc["scope_id"],scope);
    assert_eq!(desc["review_id"],id);
    assert_eq!(dock_view(&router,&cookie,&origin,root_scope,
        serde_json::json!({"op":"review","review_id":id})).await.0,
        StatusCode::CONFLICT);
    assert_eq!(dock_view(&router,&cookie,&origin,scope,
        serde_json::json!({"op":"file","path":"sample.rs"})).await.0,
        StatusCode::FORBIDDEN);
    assert_eq!(request_dock(&router,Some(&cookie),"accept",
        credentials(&receiver,ticket)).await.0,StatusCode::OK);
    confirm_dock(&router,&cookie,&credentials(&origin,ticket),&credentials(&receiver,ticket)).await;
    // A linked checkout can be replaced after acceptance. Never release
    // the origin on an obsolete Git directory identity.
    let retired=linked.with_extension("retired-review-checkout");
    std::fs::rename(&linked,&retired).unwrap();
    std::fs::create_dir(&linked).unwrap();
    std::fs::write(linked.join("sample.rs"),"untrusted replacement checkout").unwrap();
    assert_eq!(request_dock(&router,Some(&cookie),"release",
        credentials(&origin,ticket)).await.0,StatusCode::FORBIDDEN);
    std::fs::remove_dir_all(&linked).unwrap();
    std::fs::rename(&retired,&linked).unwrap();
    assert_eq!(request_dock(&router,Some(&cookie),"release",
        credentials(&origin,ticket)).await.0,StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst),0);
    assert!(lock(&state.0.session.persistence).is_none());
    fixture_git(&root.0,&["worktree","remove","--force",linked.to_str().unwrap()]);
}

#[tokio::test]
async fn review_dock_rejects_evicted_immutable_snapshot_without_reopening_inventory() {
    let (router, _state, calls, root) = surface_app();
    let cookie = paired(&router).await;
    init_review_fixture(&root.0);
    let origin = dock_window(&router, &cookie).await;
    let receiver = dock_window(&router, &cookie).await;
    let scope = origin["scope_id"].as_str().unwrap();
    let oldest = retained_review(&router, &cookie, &origin, scope).await;
    let old_id = oldest["id"].as_str().unwrap();
    for iteration in 0..10 {
        std::fs::write(root.0.join("sample.rs"),
            format!("synthetic modified Review {iteration} with distinct bytes\n")).unwrap();
        let newer = retained_review(&router, &cookie, &origin, scope).await;
        assert_ne!(newer["id"], old_id);
    }
    assert_eq!(dock_view(&router, &cookie, &origin, scope,
        serde_json::json!({"op":"review", "review_id":old_id})).await.0,
        StatusCode::CONFLICT);
    assert_eq!(request_dock(&router, Some(&cookie), "prepare-review",
        review_prepare_body(&origin, scope, old_id)).await.0,
        StatusCode::FORBIDDEN);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(receiver["window_proof"].as_str().is_some());
}

#[tokio::test]
async fn retained_review_presentation_preserves_two_bounded_panes_without_content_in_ticket() {
    let (router, state, calls, root) = surface_app();
    let cookie = paired(&router).await;
    init_review_fixture(&root.0);
    let origin = dock_window(&router, &cookie).await;
    let receiver = dock_window(&router, &cookie).await;
    let scope = origin["scope_id"].as_str().unwrap();
    let review = retained_review(&router, &cookie, &origin, scope).await;
    let id = review["id"].as_str().unwrap();
    let meta = serde_json::json!({
        "version":1,"fingerprint":"b".repeat(64),"query":"alpha",
        "selected":0,"focus":"pane1",
        "panes":[
            {"anchor":0,"head":3,"from":0,"top":200,"left":8},
            {"anchor":7,"head":12,"from":4,"top":340,"left":18},
        ],
    });
    let mut prepared_request = review_prepare_body(&origin, scope, id);
    prepared_request["presentation"] = meta.clone();
    let (status, prepared) = request_dock(
        &router, Some(&cookie), "prepare-review", prepared_request,
    ).await;
    assert_eq!(status, StatusCode::OK, "{prepared}");
    let ticket = prepared["ticket"].as_str().unwrap();
    let (status, description) = request_dock(
        &router, Some(&cookie), "inspect", credentials(&receiver,ticket),
    ).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(description["presentation"], meta);
    assert_eq!(description["review_id"], id);
    assert!(!description.to_string().contains("synthetic retained change alpha"));
    let mut malformed = review_prepare_body(&origin, scope, id);
    let mut one_pane = meta.clone();
    one_pane["panes"] = serde_json::json!([{"anchor":0,"head":0,"from":0,"top":0,"left":0}]);
    malformed["presentation"] = one_pane;
    assert_eq!(request_dock(&router, Some(&cookie), "prepare-review", malformed).await.0,
        StatusCode::BAD_REQUEST);
    assert_eq!(request_dock(&router, Some(&cookie), "accept",
        credentials(&receiver,ticket)).await.0, StatusCode::OK);
    confirm_dock(&router,&cookie,&credentials(&origin,ticket),
        &credentials(&receiver,ticket)).await;
    assert_eq!(request_dock(&router, Some(&cookie), "release",
        credentials(&origin,ticket)).await.0, StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(lock(&state.0.session.persistence).is_none());
}
