use super::{frame, frames, Temp};
use aw::{config, sessions};
use aw_agent_core::{
    CheckpointPolicy, PrincipalId, SourceAccess, SourceDriverRegistry, SourceName,
};
use aw_git::source_driver::GitSourceDriver;
use aw_store::{
    AgentAdmission, AgentHistoryForkAccess, CheckpointRestoreRequest, SessionBinding, SessionStore,
};
use aw_store_api::SessionBackend;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

// Keep physical maintenance separate from the live native-source owner
// above: one ordinary local controller, explicitly configured exact
// approvals, real stdio transport, and no model or tool invocation.
#[test]
fn configured_construction_and_retention_stdio_use_original_owners() {
    use aw_agent_core::{
        NamedPreparedSource, PreparedSourceSet, SourceAnchor, SourceCoverage, SourceOperationKind,
        SourceSet,
    };
    use aw_git::source_driver::{GitRestoreLocation, GitSourceAnchor, GitSourceLocation};
    use aw_platform::DirectoryIdentity;
    use aw_store::{CheckpointLifecycle, SandboxCreateFrom, SourceMutationSet};

    let mut nonce = [0; 16];
    getrandom::fill(&mut nonce).unwrap();
    let temp = Temp(std::env::temp_dir().join(format!(
        "aw-maintenance-rpc-{:032x}",
        u128::from_be_bytes(nonce)
    )));
    aw_platform::ensure_private_dir(&temp.0).unwrap();
    let root = temp.0.canonicalize().unwrap();
    let state = root.join("state");
    let initial = root.join("controller");
    let repo = root.join("origin");
    let approvals = root.join("approvals");
    let restored_root = root.join("restored");
    let baseline_root = root.join("baseline");
    let derived_root = root.join("derived");
    for path in [
        &initial,
        &repo,
        &approvals,
        &restored_root,
        &baseline_root,
        &derived_root,
    ] {
        aw_platform::ensure_private_dir(path).unwrap();
    }
    let executable = std::env::var_os("AW_GIT")
        .map(PathBuf::from)
        .or_else(|| option_env!("AW_DEFAULT_GIT").map(PathBuf::from))
        .expect("trusted Git executable");
    let git = |args: &[&str]| {
        let output = Command::new(&executable)
            .current_dir(&repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Git: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .unwrap()
            .trim_end()
            .to_owned()
    };
    git(&["init", "-q"]);
    git(&["config", "user.name", "Fixture"]);
    git(&["config", "user.email", "fixture@example.invalid"]);
    fs::write(repo.join("tracked"), b"original").unwrap();
    git(&["add", "tracked"]);
    git(&["commit", "--no-gpg-sign", "-qm", "fixture"]);
    let head = git(&["rev-parse", "HEAD"]);
    let index = fs::read(repo.join(".git/index")).unwrap();
    fs::write(repo.join("tracked"), b"captured dirty version").unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let mut profile = config::builtins().remove("llama").unwrap();
    profile.base_url = url.clone();
    profile.api_key_env = None;
    profile.model = Some("fixture".into());
    profile.allow_insecure_http = true;
    let fingerprint = sessions::destination_fingerprint(&profile).unwrap();
    let layout = |path: &Path| SessionBinding {
        working_directory: path.to_owned(),
        tool_root: path.to_owned(),
        system_prompt: "fixture".into(),
        profile_hint: "llama".into(),
        model_hint: "fixture".into(),
        destination_fingerprint: fingerprint.clone(),
    };
    let config_path = root.join("config.toml");
    aw_platform::open_private_new(&config_path).unwrap();
    let actor = PrincipalId::new(aw_platform::local_principal().unwrap()).unwrap();
    let registry = SourceDriverRegistry::new();
    let handle = GitSourceDriver::new(&executable)
        .unwrap()
        .register(&registry)
        .unwrap();
    let open =
        || SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
    let store = open();
    let (session, branch) = store
        .create(&actor, "maintenance controller", layout(&initial))
        .unwrap();
    let controller = store
        .agent_binding(&actor, &session.id, &branch.id)
        .unwrap();
    let controller_ref = format!(
        "/d:{}/i:{}/p:{}/a:{}",
        controller.domain_handle,
        controller.identity_handle,
        controller.project_handle,
        controller.agent_handle,
    );
    let controller_before = store.agent_get(&actor, &controller_ref).unwrap();
    let project = &controller_before.project_ref;
    let history = store
        .checkpoint_create_history_only(&actor, &controller_ref, None, "fixture:history")
        .unwrap();
    let name = SourceName::new("code").unwrap();
    let binding = store
        .source_driver(&handle)
        .unwrap()
        .binding(
            &repo,
            name.clone(),
            SourceAccess::ReadWrite,
            CheckpointPolicy::Required,
        )
        .unwrap();
    let source = store
        .checkpoint_restore_baseline(
            &actor,
            &CheckpointRestoreRequest {
                checkpoint: history.id,
                project: None,
                title: None,
                access: AgentHistoryForkAccess::Private,
                task: None,
            },
            &layout(&repo),
            std::slice::from_ref(&binding),
            "fixture:source",
            "local",
        )
        .unwrap()
        .agent;
    assert_eq!(source.admission, AgentAdmission::Held);
    let parent = store
        .sandbox_get(&actor, source.sandbox_ref.as_deref().unwrap())
        .unwrap();
    let location = GitSourceLocation {
        name: name.clone(),
        repository: repo.clone(),
        repository_identity: DirectoryIdentity::open(&repo).unwrap().identity().unwrap(),
    };
    let identity = |path: &Path| DirectoryIdentity::open(path).unwrap().identity().unwrap();
    let approval = |fields: Value| {
        let mut common = json!({
            "version":1,"store_directory":state,"store_id":store.store_id().unwrap(),
            "store_generation":store.local_process_info().unwrap().store_generation,
            "manager":actor
        });
        common
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        common
    };
    let write_private = |path: &Path, config: &Value| {
        aw_platform::open_private_new(path)
            .unwrap()
            .write_all(&serde_json::to_vec(config).unwrap())
            .unwrap();
    };
    let native_path = approvals.join("git-native.json");
    write_private(
        &native_path,
        &json!({
            "version":1,"store_directory":state,
            "store_generation":store.local_process_info().unwrap().store_generation,
            "manager":actor,"actor":actor,"agent_ref":source.canonical_ref,
            "sandbox_ref":parent.canonical_ref,"sandbox_rev":parent.rev,
            "policy_ref":"host:maintenance-fixture","base":repo,
            "directories":[{"name":name,"repository":repo,
                "repository_identity":location.repository_identity,
                "binding_rev":parent.sources[0].binding_rev,"access":"read_write"}]
        }),
    );
    // Freeze only the ordinary independent approval header; the Store
    // owner must be released before every actual subprocess.
    let common = approval(json!({}));
    drop(store);
    let approval = |fields: Value| {
        let mut config = common.clone();
        config
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        config
    };

    let command = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aw"));
        command.current_dir(&initial).env("AW_GIT", &executable);
        command
    };
    let invoke = |options: &[(&str, &Path)], flags: &[&str], requests: &[Value]| {
        let mut command = command();
        command
            .args(["rpc", "--agent-stdio", "--agent-actor-owner", "--state-dir"])
            .arg(&state)
            .args([
                "--session-id",
                &session.id,
                "--session-branch",
                &branch.id,
                "--config-file",
            ])
            .arg(&config_path)
            .args([
                "--provider-profile",
                "llama",
                "--provider-base-url",
                &url,
                "--model-id",
                "fixture",
                "--security-allow-insecure-http",
                "--provider-timeout",
                "2",
                "--provider-idle-timeout",
                "2",
                "--tools-mode",
                "off",
            ]);
        for (flag, path) in options {
            command.arg(flag).arg(path);
        }
        let mut child = command
            .args(flags)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        for request in requests {
            writeln!(stdin, "{request}").unwrap();
        }
        drop(stdin);
        frames(&child.wait_with_output().unwrap())
    };
    fn reply<'a>(events: &'a [Value], id: &str, kind: &str) -> &'a Value {
        &events
            .iter()
            .find(|event| event["id"] == id && event["type"] == kind)
            .unwrap_or_else(|| panic!("missing {kind} for {id}: {events:?}"))["data"]
    }
    let methods = [
        "checkpoint_create",
        "checkpoint_restore",
        "sandbox_create",
        "sandbox_sources_set",
        "checkpoint_retention_reconcile",
        "sandbox_retention_reconcile",
    ];
    let disabled = invoke(
        &[],
        &[],
        &methods
            .iter()
            .map(|method| frame(method, method, json!({})))
            .collect::<Vec<_>>(),
    );
    for method in methods {
        assert_eq!(disabled[0]["data"]["capabilities"][method], false);
        assert_eq!(
            reply(&disabled, method, "error")["code"],
            "unsupported_method"
        );
    }
    let capture_params =
        json!({"ref":source.canonical_ref,"sources":"auto","note":"maintenance fixture"});
    let capture = frame("capture", "checkpoint_create", capture_params.clone());
    let events = invoke(
        &[("--agent-checkpoint-capture-root", &approvals)],
        &[],
        &[
            frame(
                "path",
                "checkpoint_create",
                json!({"ref":source.canonical_ref,"path":repo}),
            ),
            frame(
                "duplicate",
                "checkpoint_create",
                json!({"ref":source.canonical_ref,"sources":"none"}),
            ),
            frame("duplicate", "checkpoint_create", capture_params.clone()),
            capture.clone(),
        ],
    );
    assert_eq!(events[0]["data"]["capabilities"]["checkpoint_create"], true);
    assert_eq!(
        reply(&events, "path", "error")["code"],
        "invalid_parameters"
    );
    assert!(
        events
            .iter()
            .any(|event| event["id"] == "duplicate"
                && event["data"]["code"] == "duplicate_request_id")
    );
    let captured = reply(&events, "capture", "response").clone();
    let checkpoint = captured["id"].as_str().unwrap();
    let retention_ref = format!("refs/aw/checkpoints/{checkpoint}/code");
    let captured_commit = git(&["rev-parse", &retention_ref]);
    let require_redacted = |value: &Value| {
        let encoded = value.to_string();
        for protected in [
            root.to_str().unwrap(),
            binding.source_ref.as_str(),
            binding.materialization.as_str(),
            captured_commit.as_str(),
            retention_ref.as_str(),
        ] {
            assert!(
                !encoded.contains(protected),
                "private source metadata in {value}"
            );
        }
    };
    require_redacted(&captured);
    assert_eq!(captured["sources"].as_array().unwrap().len(), 1);
    let absent = root.join("absent");
    let replay = invoke(
        &[("--agent-checkpoint-capture-root", &absent)],
        &[],
        &[capture],
    );
    assert_eq!(reply(&replay, "capture", "response"), &captured);
    let offline = |subcommand: &str, path: &Path, target: &str, id: &str, args: &[&str]| {
        command()
            .args([subcommand, "--state-dir"])
            .arg(&state)
            .arg(path)
            .args([target, id])
            .args(args)
            .output()
            .unwrap()
    };
    assert!(!offline(
        "checkpoint-capture",
        &absent,
        &source.canonical_ref,
        "capture",
        &["--agent-rpc-request", "--agent-note", "maintenance fixture"]
    )
    .status
    .success());
    let result = offline(
        "checkpoint-capture",
        &absent,
        &source.canonical_ref,
        "capture",
        &[
            "--agent-rpc-request",
            "--agent-recover",
            "--agent-note",
            "maintenance fixture",
        ],
    );
    assert!(
        result.status.success(),
        "offline capture replay: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap()["id"],
        checkpoint
    );

    let store = open();
    let checkpoint_snapshot = store.checkpoint_get_metadata(&actor, checkpoint).unwrap();
    let anchor = SourceAnchor::try_from(&GitSourceAnchor {
        driver: binding.driver.clone(),
        anchor_schema: checkpoint_snapshot.sources[0].anchor_schema,
        source_ref: binding.source_ref.clone(),
        name: name.clone(),
        access: binding.access,
        checkpoint_policy: binding.checkpoint_policy,
        checkpoint_id: checkpoint.to_owned(),
        base_commit: head.clone(),
        captured_commit: captured_commit.clone(),
    })
    .unwrap();
    assert_eq!(
        store.agent_get(&actor, &source.canonical_ref).unwrap(),
        source
    );
    drop(store);
    let baseline = SourceSet {
        coverage: SourceCoverage::Selected,
        anchors: vec![anchor.clone()],
    };
    let restore_path = approvals.join("restore.json");
    let create_path = approvals.join("create.json");
    let derive_path = approvals.join("derive.json");
    write_private(
        &restore_path,
        &approval(json!({
            "checkpoint":checkpoint,"binding":layout(&restored_root),
            "destination_identity":identity(&restored_root),"repositories":[location]
        })),
    );
    write_private(
        &create_path,
        &approval(json!({
            "project":project,"baseline":baseline,"binding":layout(&baseline_root),
            "destination_identity":identity(&baseline_root),"repositories":[location]
        })),
    );
    // Capture/restore and retained baseline stay immutable; derive sees
    // the live parent version instead.
    fs::write(repo.join("tracked"), b"current parent version").unwrap();
    write_private(
        &derive_path,
        &approval(json!({
            "parent":parent.canonical_ref,"parent_rev":parent.rev,"binding":layout(&derived_root),
            "destination_identity":identity(&derived_root),"repositories":[location]
        })),
    );
    let constructors = [
        (
            "--agent-checkpoint-restore-approval",
            restore_path.as_path(),
        ),
        ("--agent-sandbox-create-approval", create_path.as_path()),
        ("--agent-sandbox-derive-approval", derive_path.as_path()),
    ];
    let restore = frame(
        "restore",
        "checkpoint_restore",
        json!({"id":checkpoint,"title":"restored"}),
    );
    let create = frame("baseline", "sandbox_create", json!({}));
    let derive = frame(
        "derive",
        "sandbox_create",
        json!({"from":parent.canonical_ref}),
    );
    let events = invoke(
        &constructors,
        &[],
        &[
            frame(
                "task",
                "checkpoint_restore",
                json!({"id":checkpoint,"task":"not authorized"}),
            ),
            frame(
                "foreign",
                "sandbox_create",
                json!({"project":"/d:foreign/i:foreign/p:foreign"}),
            ),
            restore.clone(),
            create.clone(),
            derive.clone(),
        ],
    );
    assert_eq!(
        events[0]["data"]["capabilities"]["checkpoint_restore"],
        true
    );
    assert_eq!(events[0]["data"]["capabilities"]["sandbox_create"], true);
    reply(&events, "task", "error");
    reply(&events, "foreign", "error");
    let restored = reply(&events, "restore", "response").clone();
    let standalone = reply(&events, "baseline", "response").clone();
    let derived = reply(&events, "derive", "response").clone();
    assert_eq!(restored["agent"]["admission"], "held");
    assert_eq!(derived["parent"], parent.canonical_ref);
    for result in [&restored, &standalone, &derived] {
        require_redacted(result);
    }
    assert_eq!(
        fs::read(restored_root.join("code/tracked")).unwrap(),
        b"captured dirty version"
    );
    assert_eq!(
        fs::read(baseline_root.join("code/tracked")).unwrap(),
        b"captured dirty version"
    );
    assert_eq!(
        fs::read(derived_root.join("code/tracked")).unwrap(),
        b"current parent version"
    );
    let replay_options = constructors.map(|(flag, _)| (flag, absent.as_path()));
    let replay = invoke(&replay_options, &[], &[restore, create, derive]);
    assert_eq!(reply(&replay, "restore", "response"), &restored);
    assert_eq!(reply(&replay, "baseline", "response"), &standalone);
    assert_eq!(reply(&replay, "derive", "response"), &derived);
    let changed = invoke(
        &replay_options,
        &[],
        &[
            frame(
                "baseline",
                "sandbox_create",
                json!({"from":parent.canonical_ref}),
            ),
            frame(
                "restore",
                "checkpoint_restore",
                json!({"id":checkpoint,"title":"different"}),
            ),
        ],
    );
    reply(&changed, "baseline", "error");
    reply(&changed, "restore", "error");
    // Qualify actual offline recovery, not only terminal replay. The
    // original Store owner performs a genuine Prepared restore effect,
    // then deliberately loses publication. No injected failure hook.
    let pending_root = root.join("pending");
    aw_platform::ensure_private_dir(&pending_root).unwrap();
    let pending_path = approvals.join("pending.json");
    write_private(
        &pending_path,
        &approval(json!({
            "project":project,"baseline":baseline,"binding":layout(&pending_root),
            "destination_identity":identity(&pending_root),"repositories":[location]
        })),
    );
    let store = open();
    let driver = store.source_driver(&handle).unwrap();
    let prepared = store
        .sandbox_prepare_create(
            &actor,
            project,
            &layout(&pending_root),
            SandboxCreateFrom::Baseline(baseline.clone()),
            "rpc:stdio:sandbox-create:pending",
            |prepare_id, _, _| {
                Ok(PreparedSourceSet {
                    prepare_id: prepare_id.into(),
                    operation: SourceOperationKind::Restore,
                    sources: vec![NamedPreparedSource {
                        name: name.clone(),
                        prepared: driver.plan_restore_approved(
                            &GitRestoreLocation {
                                name: name.clone(),
                                repository: repo.clone(),
                                repository_identity: location.repository_identity,
                                target: pending_root.join("code"),
                                parent_identity: identity(&pending_root),
                            },
                            &GitSourceAnchor::try_from(&anchor)?,
                            prepare_id,
                        )?,
                    }],
                })
            },
        )
        .unwrap();
    let candidate = prepared.sandbox_ref().to_owned();
    prepared.revalidate(&layout(&pending_root)).unwrap();
    driver
        .execute_restore(&prepared.sources().sources[0].prepared)
        .unwrap();
    prepared.mark_unknown().unwrap();
    drop(driver);
    assert!(store.sandbox_get(&actor, &candidate).is_err());
    drop(store);
    let pending = frame("pending", "sandbox_create", json!({}));
    let pending_options = [("--agent-sandbox-create-approval", pending_path.as_path())];
    let retry = invoke(&pending_options, &[], std::slice::from_ref(&pending));
    assert_eq!(reply(&retry, "pending", "error")["needs_inspection"], true);
    let result = offline(
        "sandbox-create",
        &pending_path,
        project,
        "pending",
        &["--agent-rpc-request", "--agent-recover"],
    );
    assert!(
        result.status.success(),
        "offline constructor recovery: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let recovered: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(recovered["ref"], candidate);
    assert_eq!(recovered["rev"], 1);
    let replay = invoke(&pending_options, &[], &[pending]);
    assert_eq!(reply(&replay, "pending", "response"), &recovered);
    require_redacted(&recovered);

    let baseline_ref = standalone["ref"].as_str().unwrap();
    let restored_ref = restored["agent"]["ref"].as_str().unwrap();
    let sandbox_ref = restored["sandbox"].as_str().unwrap();
    let store = open();
    let restored_before = store.agent_get(&actor, restored_ref).unwrap();
    let sandbox = store.sandbox_get(&actor, sandbox_ref).unwrap();
    drop(store);

    // Actual setter transport across a final release and reattachment on
    // the bound held Agent, with the v61 source-only boundary intact.
    let setter_path = approvals.join("setter.json");
    let release_config = approval(json!({
        "sandbox":sandbox_ref,"sandbox_rev":sandbox.rev,"source":name,
        "destination_root":restored_root,"destination_identity":identity(&restored_root),
        "repositories":[location]
    }));
    write_private(&setter_path, &release_config);
    let setter = [("--agent-sandbox-source-approval", setter_path.as_path())];
    let release = frame(
        "release",
        "sandbox_sources_set",
        json!({
            "ref":sandbox_ref,"rev":sandbox.rev,"name":name,"binding":null
        }),
    );
    let events = invoke(&setter, &[], std::slice::from_ref(&release));
    assert_eq!(
        events[0]["data"]["capabilities"]["sandbox_sources_set"],
        true
    );
    let empty = reply(&events, "release", "response").clone();
    require_redacted(&empty);
    assert_eq!(empty["source_count"], 0);
    assert_eq!(fs::read_dir(&restored_root).unwrap().count(), 0);
    let store = open();
    let execution = store.agent_execution_binding(&actor, restored_ref).unwrap();
    assert_eq!(
        store.agent_get(&actor, restored_ref).unwrap(),
        restored_before
    );
    assert!(store
        .activate(&actor, execution.session_id(), execution.branch_id())
        .is_err());
    drop(store);
    let attach_config = approval(json!({
        "sandbox":sandbox_ref,"sandbox_rev":empty["rev"],"anchor":anchor,
        "destination_root":restored_root,"destination_identity":identity(&restored_root),
        "repositories":[location]
    }));
    fs::write(&setter_path, serde_json::to_vec(&attach_config).unwrap()).unwrap();
    let attach = frame(
        "attach",
        "sandbox_sources_set",
        json!({
            "ref":sandbox_ref,"rev":empty["rev"],"name":name,
            "binding":{"source":{"driver":anchor.driver,"source_ref":anchor.source_ref},"checkpoint":"required","access":"rw"}
        }),
    );
    let events = invoke(&setter, &[], std::slice::from_ref(&attach));
    let attached = reply(&events, "attach", "response").clone();
    require_redacted(&attached);
    assert_eq!(attached["rev"], empty["rev"].as_u64().unwrap() + 1);
    assert_eq!(attached["source_count"], 1);
    assert_eq!(
        fs::read(restored_root.join("code/tracked")).unwrap(),
        b"captured dirty version"
    );
    let result = offline(
        "sandbox-source-attach",
        &setter_path,
        sandbox_ref,
        "attach",
        &["--agent-rpc-invocation", "--agent-recover"],
    );
    assert!(
        result.status.success(),
        "offline attachment replay: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    fs::write(&setter_path, serde_json::to_vec(&release_config).unwrap()).unwrap();
    let events = invoke(&setter, &[], &[release]);
    assert_eq!(reply(&events, "release", "response"), &attached);

    let checkpoint_path = approvals.join("checkpoint-retention.json");
    let sandbox_path = approvals.join("sandbox-retention.json");
    let baseline_rev = standalone["rev"].as_u64().unwrap();
    write_private(
        &checkpoint_path,
        &approval(json!({"checkpoint":checkpoint,"repositories":[location]})),
    );
    write_private(
        &sandbox_path,
        &approval(json!({
            "sandbox":baseline_ref,"sandbox_rev":baseline_rev+1,
            "destination_root":baseline_root,"destination_identity":identity(&baseline_root),
            "repositories":[location]
        })),
    );
    let retention = [
        (
            "--agent-checkpoint-retention-approval",
            checkpoint_path.as_path(),
        ),
        ("--agent-sandbox-retention-approval", sandbox_path.as_path()),
    ];
    let cp_cleanup = frame(
        "cp-cleanup",
        "checkpoint_retention_reconcile",
        json!({"id":checkpoint}),
    );
    let sbx_cleanup = frame(
        "sbx-cleanup",
        "sandbox_retention_reconcile",
        json!({"ref":baseline_ref,"rev":baseline_rev+1}),
    );
    let events = invoke(&retention, &[], &[cp_cleanup.clone(), sbx_cleanup.clone()]);
    for method in [
        "checkpoint_retention_reconcile",
        "sandbox_retention_reconcile",
    ] {
        assert_eq!(events[0]["data"]["capabilities"][method], true);
    }
    assert_eq!(
        events[0]["data"]["capabilities"]["checkpoint_delete"],
        false
    );
    reply(&events, "cp-cleanup", "error");
    reply(&events, "sbx-cleanup", "error");
    assert_eq!(git(&["rev-parse", &retention_ref]), captured_commit);
    assert!(baseline_root.join("code/tracked").is_file());
    let events = invoke(
        &[],
        &["--agent-checkpoint-delete"],
        &[
            frame("retire", "checkpoint_delete", json!({"id":checkpoint})),
            frame(
                "close",
                "sandbox_close",
                json!({"ref":baseline_ref,"rev":baseline_rev}),
            ),
        ],
    );
    reply(&events, "retire", "response");
    reply(&events, "close", "response");
    assert_eq!(git(&["rev-parse", &retention_ref]), captured_commit);
    assert!(baseline_root.join("code/tracked").is_file());
    // Real subprocess failure after the original closed-layout claim:
    // preserve independently modified contents and retain source custody
    // until exact reconciliation under the same approval.
    let edited = baseline_root.join("code/tracked");
    fs::write(&edited, b"independent edit").unwrap();
    let unresolved = invoke(&retention, &[], std::slice::from_ref(&sbx_cleanup));
    assert_eq!(
        reply(&unresolved, "sbx-cleanup", "error")["needs_inspection"],
        true
    );
    assert_eq!(fs::read(&edited).unwrap(), b"independent edit");
    let store = open();
    let closed = store.sandbox_get(&actor, baseline_ref).unwrap();
    assert_eq!(closed.rev, baseline_rev + 1);
    assert_eq!(closed.sources.len(), 1);
    let set = SourceMutationSet::from_bindings(std::slice::from_ref(&binding)).unwrap();
    assert!(store
        .acquire_source_mutation_barrier(&set, "fixture:unsettled-cleanup")
        .is_err());
    assert!(store
        .create(&actor, "reserved layout", layout(&baseline_root))
        .is_err());
    drop(store);
    fs::write(&edited, b"captured dirty version").unwrap();
    // Settle the source barrier first; checkpoint retention uses the
    // same original source identity and cannot race unresolved cleanup.
    let events = invoke(
        &retention,
        &[],
        &[
            frame(
                "wrong-checkpoint",
                "checkpoint_retention_reconcile",
                json!({"id":"other"}),
            ),
            frame(
                "wrong-revision",
                "sandbox_retention_reconcile",
                json!({"ref":baseline_ref,"rev":baseline_rev+2}),
            ),
            sbx_cleanup.clone(),
            cp_cleanup.clone(),
        ],
    );
    reply(&events, "wrong-checkpoint", "error");
    reply(&events, "wrong-revision", "error");
    assert_eq!(
        reply(&events, "cp-cleanup", "response")["status"],
        "settled"
    );
    let released = reply(&events, "sbx-cleanup", "response").clone();
    require_redacted(&released);
    assert_eq!(released["source_count"], 0);
    assert_eq!(released["rev"], baseline_rev + 2);
    assert!(git(&["for-each-ref", "--format=%(refname)", &retention_ref]).is_empty());
    assert_eq!(fs::read_dir(&baseline_root).unwrap().count(), 0);
    let events = invoke(&retention, &[], &[cp_cleanup, sbx_cleanup]);
    assert_eq!(
        reply(&events, "cp-cleanup", "response")["status"],
        "settled"
    );
    assert_eq!(reply(&events, "sbx-cleanup", "response"), &released);

    let store = open();
    let retired = store.checkpoint_get_metadata(&actor, checkpoint).unwrap();
    assert_eq!(retired.lifecycle, CheckpointLifecycle::Retired);
    assert!(retired.sources.is_empty());
    assert_eq!(
        store.agent_get(&actor, restored_ref).unwrap(),
        restored_before
    );
    assert_eq!(
        store.agent_get(&actor, &source.canonical_ref).unwrap(),
        source
    );
    assert_eq!(
        store.agent_get(&actor, &controller_ref).unwrap(),
        controller_before
    );
    assert_eq!(
        store
            .sandbox_grant_status(&actor, restored_ref)
            .unwrap()
            .revision,
        0
    );
    assert_eq!(
        fs::read(restored_root.join("code/tracked")).unwrap(),
        b"captured dirty version"
    );
    assert_eq!(
        fs::read(derived_root.join("code/tracked")).unwrap(),
        b"current parent version"
    );
    assert_eq!(git(&["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read(repo.join("tracked")).unwrap(),
        b"current parent version"
    );
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "maintenance must not call the provider"
    );
}
