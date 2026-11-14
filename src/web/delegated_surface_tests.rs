fn persistent_surface_app(
) -> (
    Router,
    AppState,
    Arc<AtomicUsize>,
    SurfaceFixture,
    PersistentSession,
    PrincipalId,
) {
    let root = SurfaceFixture::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let agent = Agent::builder(
        Arc::new(Echo(calls.clone())),
        AgentOptions::new("fixture", "test-model"),
    )
    .build()
    .unwrap();
    let store = aw_store::SessionStore::in_memory().unwrap();
    let binding = aw_store::SessionBinding {
        working_directory: root.0.clone(),
        tool_root: root.0.clone(),
        system_prompt: String::new(),
        profile_hint: "fixture".into(),
        model_hint: "test-model".into(),
        destination_fingerprint: "0".repeat(64),
    };
    let persistent = PersistentSession::create(
        store,
        agent.actor().clone(),
        binding,
        "delegated surface owner",
    )
    .unwrap();
    persistent.store.current_user(&persistent.actor).unwrap();
    let recipient = PrincipalId::new("unix:62102").unwrap();
    persistent.store.current_user(&recipient).unwrap();
    let options = WorkspaceOptions {
        root: root.0.clone(),
        git: std::env::var_os("AW_GIT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/bin/git")),
        bash: PathBuf::from("/bin/bash"),
        actor: "ignored-and-rebound".into(),
        allow_terminals: false,
        sites: vec![],
    };
    let (router, app) = application_with_workspace(
        agent,
        Some(persistent.clone()),
        Some(options),
        HOST.parse().unwrap(),
        CODE,
    )
    .unwrap();
    (router, app, calls, root, persistent, recipient)
}

fn surface_target(app: &AppState, resource: surface_read::Resource) -> surface_read::Target {
    let caps = app.0.surfaces.as_ref().unwrap().capabilities();
    surface_read::Target {
        source_id: caps.source_id,
        scope_id: caps.view_root.id,
        owner_epoch: lock(&app.0.session.state).snapshot.instance.clone(),
        resource,
    }
}

fn surface_binding(
    app: &AppState,
    recipient: &PrincipalId,
    target: &surface_read::Target,
) -> delegation::Binding {
    let resource = match &target.resource {
        surface_read::Resource::File { path } => delegation::Resource::File {
            scope_id: target.scope_id.clone(),
            path: path.clone(),
        },
        surface_read::Resource::Review { review_id } => delegation::Resource::Review {
            scope_id: target.scope_id.clone(),
            review_id: review_id.clone(),
        },
    };
    delegation::Binding {
        owner: app.0.session.actor.clone(),
        recipient: recipient.clone(),
        source_id: target.source_id.clone(),
        site: app.0.authority.clone(),
        resource,
        parent: delegation::ParentAuthority::Surface {
            owner_epoch: target.owner_epoch.clone(),
        },
    }
}

fn issue_surface_grant(
    app: &AppState,
    persistent: &PersistentSession,
    recipient: &PrincipalId,
    target: &surface_read::Target,
) -> (delegation::Issued, delegation::Witness, delegation::Binding) {
    let binding = surface_binding(app, recipient, target);
    let issued = lock(&app.0.delegation)
        .grant(
            &persistent.store,
            binding.clone(),
            Duration::from_secs(90),
        )
        .unwrap();
    let witness = lock(&app.0.delegation)
        .authorize(&persistent.store, &issued.id, &binding)
        .unwrap();
    (issued, witness, binding)
}

fn surface_parent(target: &surface_read::Target) -> delegation::ParentAuthority {
    delegation::ParentAuthority::Surface {
        owner_epoch: target.owner_epoch.clone(),
    }
}

#[tokio::test]
async fn delegated_file_grant_reads_only_exact_launch_root_resource_and_fences_publish() {
    let (router, app, calls, root, persistent, recipient) = persistent_surface_app();
    let _cookie = paired(&router).await;
    std::fs::write(
        root.0.join("delegated.txt"),
        "delegated surface alpha\nprivate exact bytes\n",
    )
    .unwrap();
    let target = surface_target(
        &app,
        surface_read::Resource::File {
            path: "delegated.txt".into(),
        },
    );
    let (issued, witness, binding) =
        issue_surface_grant(&app, &persistent, &recipient, &target);

    let projected = surface_read::read(&app, &target).await.unwrap();
    assert_eq!(
        projected["result"]["text"],
        "delegated surface alpha\nprivate exact bytes\n"
    );
    lock(&app.0.delegation)
        .revalidate(&persistent.store, &witness, &surface_parent(&target))
        .unwrap();

    let mut wrong_scope = target.clone();
    wrong_scope.scope_id = "forged_scope".into();
    assert!(surface_read::read(&app, &wrong_scope).await.is_err());

    let wrong_path = delegation::Binding {
        resource: delegation::Resource::File {
            scope_id: target.scope_id.clone(),
            path: "sample.rs".into(),
        },
        ..binding.clone()
    };
    assert!(lock(&app.0.delegation)
        .authorize(&persistent.store, &issued.id, &wrong_path)
        .is_err());

    // A completed owner read is not publication authority. Revocation between
    // read and publish fences the held bytes even when the File is unchanged.
    lock(&app.0.delegation)
        .revoke(&persistent.store, &app.0.session.actor, &issued.id)
        .unwrap();
    assert!(lock(&app.0.delegation)
        .revalidate(&persistent.store, &witness, &surface_parent(&target))
        .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

fn fixture_git(root: &std::path::Path, args: &[&str]) -> String {
    let git = std::env::var_os("AW_GIT").expect("pinned Nix AW_GIT");
    let output = std::process::Command::new(git)
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "synthetic Git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn retain_root_review(app: &AppState, root: &std::path::Path) -> (String, String) {
    fixture_git(root, &["init", "-b", "main"]);
    fixture_git(root, &["config", "user.name", "Fixture"]);
    fixture_git(root, &["config", "user.email", "fixture@example.invalid"]);
    fixture_git(root, &["add", "sample.rs"]);
    fixture_git(root, &["commit", "-m", "synthetic baseline"]);
    std::fs::write(
        root.join("sample.rs"),
        "delegated immutable review beta\n",
    )
    .unwrap();

    let host = app.0.surfaces.as_ref().unwrap();
    let caps = host.capabilities();
    let actor = app.0.session.actor.as_str();
    let changes = host
        .execute(
            actor,
            aw_surfaces::Command::View {
                source_id: caps.source_id.clone(),
                scope_id: caps.view_root.id.clone(),
                request: aw_surfaces::ViewRequest::GitList {
                    mode: aw_surfaces::GitMode::Changes,
                },
            },
        )
        .unwrap();
    let inventory_id = changes["result"]["inventory_id"].as_str().unwrap();
    let item_id = changes["result"]["items"][0]["id"].as_str().unwrap();
    let opened = host
        .execute(
            actor,
            aw_surfaces::Command::View {
                source_id: caps.source_id,
                scope_id: caps.view_root.id.clone(),
                request: aw_surfaces::ViewRequest::GitOpen {
                    inventory_id: inventory_id.into(),
                    item_id: item_id.into(),
                },
            },
        )
        .unwrap();
    assert_eq!(opened["result"]["kind"], "review");
    (
        caps.view_root.id,
        opened["result"]["id"].as_str().unwrap().to_owned(),
    )
}

#[tokio::test]
async fn delegated_review_grant_reads_exact_retained_immutable_id_without_inventory_authority() {
    let (router, app, calls, root, persistent, recipient) = persistent_surface_app();
    let _cookie = paired(&router).await;
    let (scope_id, review_id) = retain_root_review(&app, &root.0);
    let caps = app.0.surfaces.as_ref().unwrap().capabilities();
    let target = surface_read::Target {
        source_id: caps.source_id,
        scope_id,
        owner_epoch: lock(&app.0.session.state).snapshot.instance.clone(),
        resource: surface_read::Resource::Review {
            review_id: review_id.clone(),
        },
    };
    let (issued, witness, binding) =
        issue_surface_grant(&app, &persistent, &recipient, &target);

    let projected = surface_read::read(&app, &target).await.unwrap();
    assert_eq!(projected["result"]["kind"], "review");
    assert_eq!(projected["result"]["id"], review_id);
    assert!(projected.to_string().contains("delegated immutable review beta"));
    lock(&app.0.delegation)
        .revalidate(&persistent.store, &witness, &surface_parent(&target))
        .unwrap();

    let forged = delegation::Binding {
        resource: delegation::Resource::Review {
            scope_id: target.scope_id.clone(),
            review_id: "forged_review".into(),
        },
        ..binding.clone()
    };
    assert!(lock(&app.0.delegation)
        .authorize(&persistent.store, &issued.id, &forged)
        .is_err());

    let mut wrong_target = target.clone();
    wrong_target.resource = surface_read::Resource::Review {
        review_id: "forged_review".into(),
    };
    assert!(surface_read::read(&app, &wrong_target).await.is_err());

    // The exact Review read never exposes a Git inventory operation to the
    // recipient; only the owner-side retained immutable ID is readable.
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
