struct ReadFixture {
    router: Router,
    state: AppState,
    store: SessionStore,
    actor: aw_agent_core::PrincipalId,
    binding: aw_store::SessionBinding,
    calls: Arc<AtomicUsize>,
    root: PathBuf,
}
impl Drop for ReadFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn read_fixture() -> ReadFixture {
    let root = std::env::temp_dir().join(format!("aw-browser-reader-{}", random_token().unwrap()));
    std::fs::create_dir_all(&root).unwrap();
    let context = ExecutionContext::capture(&root).unwrap();
    let actor = sessions::actor(&context, false).unwrap();
    let mut profile = config::builtins().remove("ollama").unwrap();
    profile.model = Some("test-model".into());
    let settings = config::Settings {
        profile_name: "ollama".into(),
        profile: profile.clone(),
        system_prompt: "private-system-sentinel".into(),
        request_timeout: Duration::from_secs(10),
        idle_timeout: Duration::from_secs(10),
        max_output_bytes: config::MAX_OUTPUT_BYTES,
    };
    let tools = LocalToolOptions {
        root: root.clone(),
        mode: crate::ToolMode::Off,
        selected_tools: vec![],
        execution_site: "local".into(),
        bash: None,
    };
    let binding = sessions::binding(&settings, &context, &tools).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut options = AgentOptions::new("ollama", "test-model");
    options.system_prompt = settings.system_prompt.clone();
    let agent = Agent::builder(Arc::new(Echo(calls.clone())), options)
        .account(
            aw_agent_core::AccountDescriptor::personal(
                "ollama",
                actor.clone(),
                "local",
                ["test-model".into()],
            ),
            actor.clone(),
            "local",
        )
        .build()
        .unwrap();
    let state_dir = root.join("state");
    let store = SessionStore::open(&state_dir).unwrap();
    let mut profiles = config::builtins();
    profiles.insert("ollama".into(), profile);
    let local = LocalWebOptions {
        config_path: Some(root.join("config.toml")),
        file_config: config::FileConfig::default(),
        profiles,
        settings,
        context: context.clone(),
        tools,
        state_dir: Some(state_dir),
        store: Some(store.clone()),
        title: None,
        domain_selector: None,
        workspace_selector: None,
        resources: ResourceSummary::default(),
    };
    let (router, state) = application_scoped(
        agent,
        None,
        ApplicationScope {
            workspace_options: None,
            address: HOST.parse().unwrap(),
            pairing_code: CODE.into(),
            local_owner_uid: context.effective_uid,
            owned_sessions_only: false,
            authority: Authority::LocalWeb,
            local_settings: Some(local),
        },
    )
    .unwrap();
    ReadFixture {
        router,
        state,
        store,
        actor,
        binding,
        calls,
        root,
    }
}
fn target_body(
    state: &AppState,
    store: &SessionStore,
    session: &str,
    branch: &str,
) -> serde_json::Value {
    serde_json::json!({"store_id":store.store_id().unwrap(),
        "source_epoch":lock(&state.0.session.state).snapshot.instance,
        "session_id":session,"branch_id":branch})
}
async fn json_response(
    router: &Router,
    method: &str,
    path: &str,
    body: serde_json::Value,
    cookie: Option<&str>,
) -> (StatusCode, serde_json::Value, usize) {
    let response = router
        .clone()
        .oneshot(request(method, path, body, cookie))
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 5 * 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap(), bytes.len())
}

#[tokio::test]
async fn unsaved_catalog_and_exact_branches_share_owner_without_activation_or_effects() {
    let f = read_fixture();
    let (a, main) = f
        .store
        .create(&f.actor, "A · α", f.binding.clone())
        .unwrap();
    let alternate = f
        .store
        .fork(&f.actor, &a.id, "alternate", &main.head)
        .unwrap();
    let (b, second) = f.store.create(&f.actor, "B", f.binding.clone()).unwrap();
    let before_a = f.store.export_jsonl(&f.actor, &a.id).unwrap();
    let before_b = f.store.export_jsonl(&f.actor, &b.id).unwrap();
    let before_snapshot = serde_json::to_value(&lock(&f.state.0.session.state).snapshot).unwrap();
    assert!(
        SessionStore::open(&f.root.join("state")).is_err(),
        "the existing exclusive owner remains retained"
    );
    let (status, catalog, _) = json_response(
        &f.router,
        "GET",
        "/api/sessions",
        serde_json::Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(catalog["store_id"], f.store.store_id().unwrap());
    assert_eq!(catalog["source_epoch"], before_snapshot["instance"]);
    assert_eq!(catalog["sessions"].as_array().unwrap().len(), 2);
    assert!(!catalog.to_string().contains("system_prompt"));
    let _held_agent = f.state.0.session.agent.lock().await;
    for (session, branch) in [
        (&a.id, &main.id),
        (&a.id, &alternate.id),
        (&b.id, &second.id),
    ] {
        let (status, view, bytes) = json_response(
            &f.router,
            "POST",
            "/api/agent-view",
            target_body(&f.state, &f.store, session, branch),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{view}");
        assert_eq!(view["session_id"], *session);
        assert_eq!(view["branch_id"], *branch);
        assert!(view["revision"].is_string());
        assert_eq!(view["retained_bytes"], bytes);
        assert_eq!(view["expires_in_ms"], 15000);
        assert_eq!(view["turns"], serde_json::json!([]));
        assert!(!view["title"].as_str().unwrap().contains('\u{1b}'));
    }
    assert!(lock(&f.state.0.session.persistence).is_none());
    assert_eq!(
        serde_json::to_value(&lock(&f.state.0.session.state).snapshot).unwrap(),
        before_snapshot
    );
    assert_eq!(f.store.export_jsonl(&f.actor, &a.id).unwrap(), before_a);
    assert_eq!(f.store.export_jsonl(&f.actor, &b.id).unwrap(), before_b);
    assert_eq!(f.store.list(&f.actor).unwrap().len(), 2);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn reader_denies_wrong_store_epoch_alias_and_unknown_authority_fields() {
    let f = read_fixture();
    let (session, branch) = f
        .store
        .create(&f.actor, "Exact", f.binding.clone())
        .unwrap();
    let original = target_body(&f.state, &f.store, &session.id, &branch.id);
    for (field, value, expected) in [
        ("store_id", "different-store", StatusCode::FORBIDDEN),
        ("source_epoch", "prior-owner", StatusCode::FORBIDDEN),
        ("branch_id", "main", StatusCode::FORBIDDEN),
        ("session_id", "unknown-session", StatusCode::FORBIDDEN),
        ("actor", "unix:0", StatusCode::BAD_REQUEST),
        ("path", "/other/state", StatusCode::BAD_REQUEST),
    ] {
        let mut body = original.clone();
        body[field] = value.into();
        assert_eq!(
            json_response(&f.router, "POST", "/api/agent-view", body, None)
                .await
                .0,
            expected,
            "{field}"
        );
    }
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    assert!(lock(&f.state.0.session.persistence).is_none());
}

#[tokio::test]
async fn pairing_never_grants_cli_history_and_unchanged_head_revoke_is_effective() {
    let (router, state, calls, active) = persistent_app();
    let owner = aw_agent_core::PrincipalId::new("different-cli-owner").unwrap();
    let (session, branch) = active
        .store
        .create(&owner, "Private", active.binding.clone())
        .unwrap();
    let body = target_body(&state, &active.store, &session.id, &branch.id);
    assert_eq!(
        json_response(&router, "POST", "/api/agent-view", body.clone(), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let cookie = paired(&router).await;
    assert_eq!(
        json_response(
            &router,
            "POST",
            "/api/agent-view",
            body.clone(),
            Some(&cookie)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    active
        .store
        .grant(&owner, &session.id, &active.actor, Some(false))
        .unwrap();
    let (status, before, _) = json_response(
        &router,
        "POST",
        "/api/agent-view",
        body.clone(),
        Some(&cookie),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    active
        .store
        .grant(&owner, &session.id, &active.actor, None)
        .unwrap();
    assert_eq!(
        json_response(&router, "POST", "/api/agent-view", body, Some(&cookie))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let branch_after = active
        .store
        .branches(&owner, &session.id)
        .unwrap()
        .remove(0);
    assert_eq!(before["head"], branch_after.head);
    assert_eq!(before["revision"], branch_after.revision.to_string());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unsaved_explicit_activation_reuses_store_and_creates_no_session() {
    let f = read_fixture();
    let (session, branch) = f
        .store
        .create(&f.actor, "Existing", f.binding.clone())
        .unwrap();
    let (status, value, _) = json_response(
        &f.router,
        "POST",
        "/api/session/open",
        serde_json::json!({"expected_revision":0,"session_id":session.id,"branch":branch.id}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["saved"]["session_id"], session.id);
    assert_eq!(value["saved"]["branch_id"], branch.id);
    assert_eq!(f.store.list(&f.actor).unwrap().len(), 1);
    assert_eq!(
        current_store(&f.state).unwrap().store_id().unwrap(),
        f.store.store_id().unwrap()
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn first_prompt_admits_original_unsaved_target_and_retries_without_another_session() {
    let f = read_fixture();
    let body = serde_json::json!({"request_id":"first","expected_revision":0,"account_id":"ollama","model":"test-model","prompt":"hello"});
    for invalid in [
        serde_json::json!({"session_id":"forged"}),
        serde_json::json!({"branch_id":"forged"}),
        serde_json::json!({"expected_revision":9}),
        serde_json::json!({"model":"not-allowed"}),
    ] {
        let mut bad = body.clone();
        for (key, value) in invalid.as_object().unwrap() {
            bad[key] = value.clone();
        }
        assert_ne!(
            json_response(&f.router, "POST", "/api/prompts", bad, None)
                .await
                .0,
            StatusCode::ACCEPTED
        );
        assert!(f.store.list(&f.actor).unwrap().is_empty());
    }
    let (status, receipt, _) =
        json_response(&f.router, "POST", "/api/prompts", body.clone(), None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{receipt}");
    wait_settled(&f.state).await;
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    assert_eq!(f.store.list(&f.actor).unwrap().len(), 1);
    let p = lock(&f.state.0.session.persistence).clone().unwrap();
    assert_eq!(p.view().unwrap().history.turns.len(), 1);
    let (retry_status, retry_receipt, _) =
        json_response(&f.router, "POST", "/api/prompts", body, None).await;
    assert_eq!(retry_status, StatusCode::ACCEPTED);
    assert_eq!(retry_receipt["run_id"], receipt["run_id"]);
    assert_eq!(retry_receipt["revision"], receipt["revision"]);
    assert_eq!(retry_receipt["state"], "settled");
    let local = {
        let settings = lock(f.state.0.local_settings.as_ref().unwrap());
        LocalWebOptions {
            config_path: settings.config_path.clone(),
            file_config: settings.file_config.clone(),
            profiles: settings.profiles.clone(),
            settings: settings.settings.clone(),
            context: settings.context.clone(),
            tools: settings.tools.clone(),
            state_dir: settings.state_dir.clone(),
            store: Some(f.store.clone()),
            title: None,
            domain_selector: None,
            workspace_selector: None,
            resources: ResourceSummary::default(),
        }
    };
    let mut options = AgentOptions::new("ollama", "test-model");
    options.system_prompt = local.settings.system_prompt.clone();
    let restarted_agent = Agent::builder(Arc::new(Echo(f.calls.clone())), options)
        .account(
            aw_agent_core::AccountDescriptor::personal(
                "ollama",
                f.actor.clone(),
                "local",
                ["test-model".into()],
            ),
            f.actor.clone(),
            "local",
        )
        .build()
        .unwrap();
    let (restarted_router, restarted) = application_scoped(
        restarted_agent,
        None,
        ApplicationScope {
            workspace_options: None,
            address: HOST.parse().unwrap(),
            pairing_code: CODE.into(),
            local_owner_uid: local.context.effective_uid,
            owned_sessions_only: false,
            authority: Authority::LocalWeb,
            local_settings: Some(local),
        },
    )
    .unwrap();
    restarted.0.session.accounts.revoke("ollama").unwrap();
    let retried_body = serde_json::json!({"request_id":"first","expected_revision":0,"account_id":"ollama","model":"test-model","prompt":"hello"});
    let (restart_status, restart_receipt, _) = json_response(
        &restarted_router,
        "POST",
        "/api/prompts",
        retried_body,
        None,
    )
    .await;
    assert_eq!(restart_status, StatusCode::ACCEPTED);
    assert_eq!(restart_receipt, retry_receipt);
    assert!(lock(&restarted.0.session.persistence).is_none());
    assert!(lock(&restarted.0.session.state).snapshot.saved.is_none());
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    assert_eq!(f.store.list(&f.actor).unwrap().len(), 1);
}

#[tokio::test]
async fn reader_admission_busy_history_and_expired_transport_fail_closed() {
    use aw_agent_core::{TurnJournal, TurnStart};
    let (router, state, calls, p) = persistent_app();
    let cookie = paired(&router).await;
    let body = target_body(
        &state,
        &p.store,
        p.handle.session_id(),
        &p.handle.branch().unwrap().id,
    );
    let permits = (0..4)
        .map(|_| {
            state
                .0
                .branch_read_slots
                .clone()
                .try_acquire_owned()
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        json_response(
            &router,
            "POST",
            "/api/agent-view",
            body.clone(),
            Some(&cookie)
        )
        .await
        .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    drop(permits);
    let identity = state
        .0
        .session
        .agent
        .lock()
        .await
        .default_identity()
        .unwrap();
    p.handle
        .begin(TurnStart {
            turn_id: 1,
            prompt: "held".into(),
            identity,
        })
        .await
        .unwrap();
    assert_eq!(
        json_response(
            &router,
            "POST",
            "/api/agent-view",
            body.clone(),
            Some(&cookie)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    state.0.shutdown.cancel();
    assert_eq!(
        json_response(&router, "POST", "/api/agent-view", body, Some(&cookie))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn imported_unknown_history_is_inspectable_without_execution() {
    let f = read_fixture();
    let (source, _) = f
        .store
        .create(&f.actor, "Exported", f.binding.clone())
        .unwrap();
    let exported = f.store.export_jsonl(&f.actor, &source.id).unwrap();
    let (imported, branch) = f
        .store
        .import_jsonl(&f.actor, &exported, f.binding.clone())
        .unwrap();
    let (status, view, _) = json_response(
        &f.router,
        "POST",
        "/api/agent-view",
        target_body(&f.state, &f.store, &imported.id, &branch.id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["needs_inspection"], true);
    assert!(lock(&f.state.0.session.persistence).is_none());
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn transcript_read_omits_invocation_identity_and_provider_continuation() {
    let (router, state, calls, p) = persistent_app();
    let cookie = paired(&router).await;
    let prompt = saved_prompt(&state, "seed-privacy");
    assert_eq!(
        json_response(&router, "POST", "/api/prompts", prompt, Some(&cookie))
            .await
            .0,
        StatusCode::ACCEPTED
    );
    wait_settled(&state).await;
    let stored = p.view().unwrap();
    assert!(!stored.history.turns[0].invocations.is_empty());
    let body = target_body(&state, &p.store, p.handle.session_id(), &stored.branch.id);
    let (status, public, _) =
        json_response(&router, "POST", "/api/agent-view", body, Some(&cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(public["turns"][0]["invocations"], serde_json::json!([]));
    let wire = public.to_string();
    for private in [
        "provider_context",
        "credential_custody",
        "account_owner",
        "inference_executor",
        "destination_fingerprint",
        "system_prompt",
    ] {
        assert!(!wire.contains(private), "{private}");
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "read made no further invocation"
    );
}

#[tokio::test]
async fn remote_target_remains_discoverable_but_transcript_read_never_falls_back() {
    let f = read_fixture();
    let machine = f.store.local_machine().unwrap();
    let (session, branch) = f
        .store
        .create_in_workspace(
            &f.actor,
            "Machine target",
            f.binding.clone(),
            None,
            None,
            Some(aw_store::SessionExecutionTarget::Machine {
                machine_id: machine.id,
            }),
        )
        .unwrap();
    let (status, catalog, _) = json_response(
        &f.router,
        "GET",
        "/api/sessions",
        serde_json::Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(catalog["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["id"] == session.id));
    assert_eq!(
        json_response(
            &f.router,
            "POST",
            "/api/agent-view",
            target_body(&f.state, &f.store, &session.id, &branch.id),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    assert!(lock(&f.state.0.session.persistence).is_none());
}
