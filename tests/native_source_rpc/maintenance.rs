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
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

// Keep physical maintenance separate from the live native-source owner:
// one ordinary local controller, exact approvals, and real stdio.
// Only a subsequent independent source activation can dispatch a task.
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
    let destination_controller_root = root.join("destination-controller");
    aw_platform::ensure_private_dir(&destination_controller_root).unwrap();
    let destination_workspace = store
        .create_workspace(&actor, None, "restore-destination")
        .unwrap();
    let (destination_session, destination_branch) = store
        .create_in_workspace(
            &actor,
            "destination controller",
            layout(&destination_controller_root),
            Some(&destination_workspace.domain_id),
            Some(&destination_workspace.id),
            None,
        )
        .unwrap();
    let destination_binding = store
        .agent_binding(&actor, &destination_session.id, &destination_branch.id)
        .unwrap();
    let destination_project = format!(
        "/d:{}/i:{}/p:{}",
        destination_binding.domain_handle,
        destination_binding.identity_handle,
        destination_binding.project_handle
    );
    assert_ne!(&destination_project, project);
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
    let rpc_command = |session: &str, branch: &str, tools: &str| {
        let mut command = command();
        command
            .args(["rpc", "--agent-stdio", "--agent-actor-owner", "--state-dir"])
            .arg(&state)
            .args([
                "--session-id",
                session,
                "--session-branch",
                branch,
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
                "10",
                "--provider-idle-timeout",
                "10",
                "--tools-mode",
                tools,
            ]);
        command
    };
    fn run_frames(mut command: Command, requests: &[Value]) -> Output {
        let mut child = command
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
        child.wait_with_output().unwrap()
    }
    let invoke = |options: &[(&str, &Path)], flags: &[&str], requests: &[Value]| {
        let mut command = rpc_command(&session.id, &branch.id, "off");
        for (flag, path) in options {
            command.arg(flag).arg(path);
        }
        command.args(flags);
        frames(&run_frames(command, requests))
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
    // The exact task is supplied as stdin, never argv or an approval-file
    // field. Recovery/abandonment use the same original semantic identity.
    let offline_task_in_project =
        |path: &Path, id: &str, title: &str, mode: &str, payload: &[u8], project: Option<&str>| {
            let mut command = command();
            command.arg("checkpoint-restore");
            if let Some(project) = project {
                command.args(["--agent-project", project]);
            }
            let mut child = command
                .arg("--state-dir")
                .arg(&state)
                .arg(path)
                .args([
                    checkpoint,
                    id,
                    "--session-title",
                    title,
                    "--agent-rpc-request",
                    mode,
                    "--agent-task-stdin",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(payload).unwrap();
            child.wait_with_output().unwrap()
        };
    let offline_task = |path: &Path, id: &str, title: &str, mode: &str, payload: &[u8]| {
        offline_task_in_project(path, id, title, mode, payload, None)
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
    let task = "Read the captured source — fixture task.\n";
    let restore = frame(
        "restore",
        "checkpoint_restore",
        json!({"id":checkpoint,"title":"restored","task":task}),
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
                json!({"id":checkpoint,"task":""}),
            ),
            frame(
                "foreign",
                "sandbox_create",
                json!({"project":"/d:foreign/i:foreign/p:foreign"}),
            ),
            frame(
                "task-inherit",
                "checkpoint_restore",
                json!({"id":checkpoint,"task":task,"access":"inherit"}),
            ),
            frame(
                "task-project",
                "checkpoint_restore",
                json!({"id":checkpoint,"task":task,"project":project}),
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
    reply(&events, "task-inherit", "error");
    reply(&events, "task-project", "error");
    reply(&events, "foreign", "error");
    let restored = reply(&events, "restore", "response").clone();
    let standalone = reply(&events, "baseline", "response").clone();
    let derived = reply(&events, "derive", "response").clone();
    assert_eq!(restored["agent"]["admission"], "held");
    assert!(!restored.to_string().contains("fixture task"));
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
                json!({"id":checkpoint,"title":"restored","task":"changed task"}),
            ),
        ],
    );
    reply(&changed, "baseline", "error");
    reply(&changed, "restore", "error");
    let terminal = offline_task(
        &absent,
        "restore",
        "restored",
        "--agent-recover",
        task.as_bytes(),
    );
    assert!(
        terminal.status.success(),
        "task replay: {}",
        String::from_utf8_lossy(&terminal.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&terminal.stdout).unwrap()["ref"],
        restored["agent"]["ref"]
    );
    assert!(!offline_task(
        &absent,
        "restore",
        "restored",
        "--agent-recover",
        b"changed task"
    )
    .status
    .success());

    // Original task-bearing captured restore custody, including real
    // prepared Git work and encrypted initial Input, can be recovered
    // or abandoned only with the exact original invocation and task.
    let interrupted_root = root.join("interrupted-task");
    let abandoned_root = root.join("abandoned-task");
    let interrupted_path = approvals.join("interrupted-task.json");
    let abandoned_path = approvals.join("abandoned-task.json");
    let interrupt_restore = |target: &Path, path: &Path, id: &str, destination: Option<&str>| {
        aw_platform::ensure_private_dir(target).unwrap();
        write_private(
            path,
            &approval(json!({
                "checkpoint":checkpoint,"destination_project":destination,"binding":layout(target),
                "destination_identity":identity(target),"repositories":[location]
            })),
        );
        let store = open();
        let driver = store.source_driver(&handle).unwrap();
        let request = CheckpointRestoreRequest {
            checkpoint: checkpoint.to_owned(),
            project: destination.map(str::to_owned),
            title: Some(id.to_owned()),
            access: AgentHistoryForkAccess::Private,
            task: Some(task.to_owned()),
        };
        let target_layout = layout(target);
        let prepared = store
            .checkpoint_prepare_restore(
                &actor,
                &request,
                &target_layout,
                &format!("rpc:stdio:checkpoint-restore:{id}"),
                "local",
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
                                    target: target.join("code"),
                                    parent_identity: identity(target),
                                },
                                &GitSourceAnchor::try_from(&anchor)?,
                                prepare_id,
                            )?,
                        }],
                    })
                },
            )
            .unwrap();
        let candidate = (
            prepared.prepare_id().to_owned(),
            prepared.sandbox_ref().to_owned(),
        );
        prepared.revalidate(&target_layout).unwrap();
        driver
            .execute_restore(&prepared.sources().sources[0].prepared)
            .unwrap();
        prepared.mark_unknown().unwrap();
        assert!(store.sandbox_get(&actor, &candidate.1).is_err());
        assert!(store.session(&actor, &candidate.0).is_err());
        candidate
    };
    let candidate = interrupt_restore(&interrupted_root, &interrupted_path, "task-recovery", None);
    let interrupted = frame(
        "task-recovery",
        "checkpoint_restore",
        json!({"id":checkpoint,"title":"task-recovery","task":task}),
    );
    let options = [(
        "--agent-checkpoint-restore-approval",
        interrupted_path.as_path(),
    )];
    let retry = invoke(&options, &[], std::slice::from_ref(&interrupted));
    assert_eq!(
        reply(&retry, "task-recovery", "error")["needs_inspection"],
        true
    );
    assert!(!offline_task(
        &interrupted_path,
        "task-recovery",
        "task-recovery",
        "--agent-recover",
        b"changed task"
    )
    .status
    .success());
    assert!(!offline(
        "checkpoint-restore",
        &interrupted_path,
        checkpoint,
        "task-recovery",
        &[
            "--agent-rpc-request",
            "--agent-recover",
            "--session-title",
            "task-recovery"
        ]
    )
    .status
    .success());
    let result = offline_task(
        &interrupted_path,
        "task-recovery",
        "task-recovery",
        "--agent-recover",
        task.as_bytes(),
    );
    assert!(
        result.status.success(),
        "task recovery: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let recovered_task: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(recovered_task["sandbox_ref"], candidate.1);
    assert_eq!(recovered_task["admission"], "held");
    let replay = invoke(&options, &[], &[interrupted]);
    assert_eq!(
        reply(&replay, "task-recovery", "response")["agent"]["ref"],
        recovered_task["ref"]
    );
    let store = open();
    let restored_execution = store
        .agent_execution_binding(&actor, recovered_task["ref"].as_str().unwrap())
        .unwrap();
    assert_eq!(restored_execution.session_id(), candidate.0);
    let recovered_queue = store.actor_queue_list(&actor, &candidate.0).unwrap();
    assert_eq!(recovered_queue.len(), 1);
    assert_eq!(recovered_queue[0].state, aw_store::QueueState::Pending);
    assert_eq!(
        fs::read(interrupted_root.join("code/tracked")).unwrap(),
        b"captured dirty version"
    );
    drop(store);

    let candidate = interrupt_restore(&abandoned_root, &abandoned_path, "task-abandon", None);
    assert!(!offline_task(
        &abandoned_path,
        "task-abandon",
        "task-abandon",
        "--agent-abandon",
        b"changed task"
    )
    .status
    .success());
    let rejected = offline_task(
        &abandoned_path,
        "task-abandon",
        "task-abandon",
        "--agent-abandon",
        task.as_bytes(),
    );
    assert!(
        rejected.status.success(),
        "task abandonment: {}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&rejected.stdout).unwrap()["status"],
        "rejected"
    );
    assert_eq!(fs::read_dir(&abandoned_root).unwrap().count(), 0);
    assert!(offline_task(
        &absent,
        "task-abandon",
        "task-abandon",
        "--agent-abandon",
        task.as_bytes()
    )
    .status
    .success());
    let retry = invoke(
        &[("--agent-checkpoint-restore-approval", &abandoned_path)],
        &[],
        &[frame(
            "task-abandon",
            "checkpoint_restore",
            json!({"id":checkpoint,"title":"task-abandon","task":task}),
        )],
    );
    reply(&retry, "task-abandon", "error");
    let store = open();
    assert!(store.session(&actor, &candidate.0).is_err());
    assert!(store.sandbox_get(&actor, &candidate.1).is_err());
    drop(store);

    // Explicit cross-Project private restore: the controller is bound
    // to the destination Project, while the immutable source checkpoint
    // stays in its original Project. Old/default approvals cannot move it.
    let cross_root = root.join("cross-project");
    aw_platform::ensure_private_dir(&cross_root).unwrap();
    let cross_path = approvals.join("cross-project.json");
    let legacy_approval = approval(json!({
        "checkpoint":checkpoint,"binding":layout(&cross_root),
        "destination_identity":identity(&cross_root),"repositories":[location]
    }));
    write_private(&cross_path, &legacy_approval);
    let cross_request = frame(
        "cross-restore",
        "checkpoint_restore",
        json!({"id":checkpoint,"project":destination_project,"title":"cross restored"}),
    );
    let cross_rpc = |path: &Path, requests: &[Value]| {
        let mut command = rpc_command(&destination_session.id, &destination_branch.id, "off");
        command.current_dir(&destination_controller_root);
        command.arg("--agent-checkpoint-restore-approval").arg(path);
        frames(&run_frames(command, requests))
    };
    let denied = cross_rpc(&cross_path, std::slice::from_ref(&cross_request));
    reply(&denied, "cross-restore", "error");
    assert_eq!(fs::read_dir(&cross_root).unwrap().count(), 0);
    let mut cross_approval = legacy_approval;
    // Even an otherwise valid destination owned by this actor in another
    // Domain is not part of this first host promotion.
    let store = open();
    let other_domain = store
        .create_domain(
            &actor,
            "other-restore-domain",
            Some(&destination_binding.domain_handle),
        )
        .unwrap();
    let other_root = root.join("other-domain-controller");
    aw_platform::ensure_private_dir(&other_root).unwrap();
    let (other_session, other_branch) = store
        .create_in_workspace(
            &actor,
            "other controller",
            layout(&other_root),
            Some(&other_domain.id),
            None,
            None,
        )
        .unwrap();
    let other_binding = store
        .agent_binding(&actor, &other_session.id, &other_branch.id)
        .unwrap();
    let other_project = format!(
        "/d:{}/i:{}/p:{}",
        other_binding.domain_handle, other_binding.identity_handle, other_binding.project_handle
    );
    assert_ne!(
        other_binding.domain_handle,
        destination_binding.domain_handle
    );
    drop(store);
    cross_approval["destination_project"] = json!(other_project);
    fs::write(&cross_path, serde_json::to_vec(&cross_approval).unwrap()).unwrap();
    let denied = offline(
        "checkpoint-restore",
        &cross_path,
        checkpoint,
        "cross-domain",
        &["--agent-project", &other_project],
    );
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr)
        .contains("cross-Domain captured restore unavailable"));
    assert_eq!(fs::read_dir(&cross_root).unwrap().count(), 0);
    cross_approval["destination_project"] = json!(destination_project);
    fs::write(&cross_path, serde_json::to_vec(&cross_approval).unwrap()).unwrap();
    let denied = invoke(
        &[("--agent-checkpoint-restore-approval", &cross_path)],
        &[],
        std::slice::from_ref(&cross_request),
    );
    reply(&denied, "cross-restore", "error");
    assert_eq!(fs::read_dir(&cross_root).unwrap().count(), 0);
    let denied = cross_rpc(
        &cross_path,
        &[
            frame("implicit", "checkpoint_restore", json!({"id":checkpoint})),
            frame(
                "foreign",
                "checkpoint_restore",
                json!({"id":checkpoint,"project":project}),
            ),
            frame(
                "inherit",
                "checkpoint_restore",
                json!({"id":checkpoint,"project":destination_project,"access":"inherit"}),
            ),
        ],
    );
    for id in ["implicit", "foreign", "inherit"] {
        reply(&denied, id, "error");
    }
    assert_eq!(fs::read_dir(&cross_root).unwrap().count(), 0);
    let result = cross_rpc(&cross_path, std::slice::from_ref(&cross_request));
    let cross = reply(&result, "cross-restore", "response").clone();
    assert_eq!(cross["agent"]["project"], destination_project);
    assert_eq!(cross["agent"]["admission"], "held");
    require_redacted(&cross);
    assert_eq!(
        fs::read(cross_root.join("code/tracked")).unwrap(),
        b"captured dirty version"
    );
    let replay = cross_rpc(&absent, std::slice::from_ref(&cross_request));
    assert_eq!(reply(&replay, "cross-restore", "response"), &cross);
    let denied = invoke(
        &[("--agent-checkpoint-restore-approval", &absent)],
        &[],
        std::slice::from_ref(&cross_request),
    );
    reply(&denied, "cross-restore", "error");
    let result = offline(
        "checkpoint-restore",
        &absent,
        checkpoint,
        "cross-restore",
        &[
            "--agent-rpc-request",
            "--agent-recover",
            "--session-title",
            "cross restored",
            "--agent-project",
            &destination_project,
        ],
    );
    assert!(
        result.status.success(),
        "cross-Project replay: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap()["ref"],
        cross["agent"]["ref"]
    );
    let store = open();
    let reference = cross["agent"]["ref"].as_str().unwrap();
    let snapshot = store.agent_get(&actor, reference).unwrap();
    assert_eq!(snapshot.project_ref, destination_project);
    assert_eq!(
        store
            .sandbox_grant_status(&actor, reference)
            .unwrap()
            .revision,
        0
    );
    let execution = store.agent_execution_binding(&actor, reference).unwrap();
    assert!(store
        .activate(&actor, execution.session_id(), execution.branch_id())
        .is_err());
    assert!(store
        .actor_queue_list(&actor, execution.session_id())
        .unwrap()
        .is_empty());
    drop(store);

    // Original explicit destination is part of the exact Prepared task
    // receipt. Missing/changed destination cannot inspect or abandon it.
    let cross_pending_root = root.join("cross-project-pending");
    let cross_pending_path = approvals.join("cross-project-pending.json");
    let candidate = interrupt_restore(
        &cross_pending_root,
        &cross_pending_path,
        "cross-pending",
        Some(&destination_project),
    );
    let pending = frame(
        "cross-pending",
        "checkpoint_restore",
        json!({"id":checkpoint,"project":destination_project,"title":"cross-pending","task":task}),
    );
    let retry = cross_rpc(&cross_pending_path, std::slice::from_ref(&pending));
    assert_eq!(
        reply(&retry, "cross-pending", "error")["needs_inspection"],
        true
    );
    assert!(!offline_task(
        &cross_pending_path,
        "cross-pending",
        "cross-pending",
        "--agent-recover",
        task.as_bytes()
    )
    .status
    .success());
    assert!(!offline_task_in_project(
        &cross_pending_path,
        "cross-pending",
        "cross-pending",
        "--agent-abandon",
        task.as_bytes(),
        Some(project)
    )
    .status
    .success());
    let result = offline_task_in_project(
        &cross_pending_path,
        "cross-pending",
        "cross-pending",
        "--agent-recover",
        task.as_bytes(),
        Some(&destination_project),
    );
    assert!(
        result.status.success(),
        "cross-Project task recovery: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let recovered: Value = serde_json::from_slice::<Value>(&result.stdout).unwrap();
    assert_eq!(recovered["sandbox_ref"], candidate.1);
    let replay = cross_rpc(&absent, &[pending]);
    let snapshot = reply(&replay, "cross-pending", "response");
    assert_eq!(snapshot["agent"]["ref"], recovered["ref"]);
    assert_eq!(snapshot["agent"]["project"], destination_project);
    assert_eq!(snapshot["agent"]["admission"], "held");
    let store = open();
    let queue = store.actor_queue_list(&actor, &candidate.0).unwrap();
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].state, aw_store::QueueState::Pending);
    assert_eq!(
        fs::read(cross_pending_root.join("code/tracked")).unwrap(),
        b"captured dirty version"
    );
    drop(store);

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
    let execution = store.agent_execution_binding(&actor, restored_ref).unwrap();
    let queue_before = store
        .actor_queue_list(&actor, execution.session_id())
        .unwrap();
    assert_eq!(queue_before.len(), 1);
    assert_eq!(queue_before[0].state, aw_store::QueueState::Pending);
    assert_eq!(queue_before[0].request.recipient_site, "local");
    assert!(queue_before[0].claim.is_none());
    let owner = store.session(&actor, execution.session_id()).unwrap().owner;
    assert!(store
        .actor_queue_claim_next(
            &owner,
            execution.session_id(),
            execution.branch_id(),
            "local",
            "fixture",
            queue_before[0].request.writer_epoch,
        )
        .is_err());
    assert!(store
        .agent_admission(
            &actor,
            restored_ref,
            AgentAdmission::Open,
            Some(restored_before.rev)
        )
        .is_err());
    assert!(store
        .activate(&actor, execution.session_id(), execution.branch_id())
        .is_err());
    assert_eq!(
        store
            .actor_queue_list(&actor, execution.session_id())
            .unwrap(),
        queue_before
    );
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
    assert_eq!(
        store.agent_get(&actor, restored_ref).unwrap(),
        restored_before
    );
    assert!(store
        .activate(&actor, execution.session_id(), execution.branch_id())
        .is_err());
    assert_eq!(
        store
            .actor_queue_list(&actor, execution.session_id())
            .unwrap(),
        queue_before
    );
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
    assert_eq!(
        store
            .actor_queue_list(&actor, execution.session_id())
            .unwrap(),
        queue_before
    );
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "maintenance must not call the provider"
    );

    // Independent post-publication authority, pinned to the reattached
    // binding and exact held Agent. No original checkpoint grant is reused.
    let sandbox = store.sandbox_get(&actor, sandbox_ref).unwrap();
    let checkout = restored_root.join("code");
    let native_root = root.join("restored-native");
    aw_platform::ensure_private_dir(&native_root).unwrap();
    write_private(
        &native_root.join("git-native.json"),
        &json!({
            "version":1,"store_directory":state,
            "store_generation":store.local_process_info().unwrap().store_generation,
            "manager":actor,"actor":owner,"agent_ref":restored_ref,
            "sandbox_ref":sandbox_ref,"sandbox_rev":sandbox.rev,
            "policy_ref":"host:restored-task-native","base":restored_root,
            "directories":[{"name":name,"repository":checkout,
                "repository_identity":identity(&checkout),
                "binding_rev":sandbox.sources[0].binding_rev,"access":"read_only"}]
        }),
    );
    drop(store);
    // Without explicit current grant + held-Admission CAS, legacy or
    // incomplete native activation cannot consume the Pending task.
    assert!(!run_frames(
        rpc_command(execution.session_id(), execution.branch_id(), "off"),
        &[]
    )
    .status
    .success());
    let store = open();
    assert_eq!(
        store
            .actor_queue_list(&actor, execution.session_id())
            .unwrap(),
        queue_before
    );
    drop(store);
    let provider = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(90);
        for ordinal in 0..2 {
            let mut socket = loop {
                assert!(Instant::now() < deadline, "restored-task provider deadline");
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(error) => panic!("provider: {error}"),
                }
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut bytes = Vec::new();
            let request: Value = loop {
                let mut chunk = [0; 4096];
                let n = socket.read(&mut chunk).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
                assert!(bytes.len() < 2 * 1024 * 1024);
                if let Some(end) = bytes.windows(4).position(|x| x == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    if bytes.len() >= end + 4 + length {
                        break serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap();
                    }
                }
            };
            let messages = request["messages"].as_array().unwrap();
            assert!(messages
                .iter()
                .any(|message| message["role"] == "user" && message["content"] == task));
            let names: Vec<_> = request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tool| tool["function"]["name"].as_str().unwrap())
                .collect();
            assert_eq!(names, ["read"]);
            let (delta, reason) = if ordinal == 0 {
                (
                    json!({"tool_calls":[{"index":0,"id":"restored-task-read","type":"function",
                    "function":{"name":"read","arguments":json!({"path":"code/tracked"}).to_string()}}]}),
                    "tool_calls",
                )
            } else {
                assert!(messages.iter().any(|message| message["role"] == "tool"
                    && message["content"]
                        .to_string()
                        .contains("captured dirty version")));
                (json!({"content":"restored task completed"}), "stop")
            };
            let body = format!(
                "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"index":0,"delta":delta}]}),
                json!({"choices":[{"index":0,"delta":{},"finish_reason":reason}]})
            );
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        }
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    });
    let mut native = rpc_command(execution.session_id(), execution.branch_id(), "read-only");
    native
        .arg("--agent-native-source-root")
        .arg(&native_root)
        .args([
            "--agent-native-source-grant-rev",
            "0",
            "--agent-native-source-open-rev",
        ])
        .arg(restored_before.rev.to_string())
        .args(["--tools-allow", "read"]);
    let events = frames(&run_frames(
        native,
        &[
            frame("restored-task", "actor_step", json!({})),
            frame("no-replay", "actor_step", json!({})),
        ],
    ));
    assert_eq!(
        super::settled(&events, "restored-task")["result"]["outcome"],
        "completed"
    );
    assert_eq!(super::settled(&events, "no-replay")["status"], "idle");
    provider.join().unwrap();
    let store = open();
    let queue = store
        .actor_queue_list(&actor, execution.session_id())
        .unwrap();
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].request, queue_before[0].request);
    assert_eq!(queue[0].payload_sha256, queue_before[0].payload_sha256);
    assert_eq!(queue[0].state, aw_store::QueueState::Settled);
    let view = store
        .read_agent_branch_view(&owner, execution.session_id(), execution.branch_id())
        .unwrap();
    assert_eq!(view.history.turns.len(), 1);
    assert_eq!(view.history.turns[0].prompt, task);
    assert_eq!(
        fs::read(checkout.join("tracked")).unwrap(),
        b"captured dirty version"
    );
}
