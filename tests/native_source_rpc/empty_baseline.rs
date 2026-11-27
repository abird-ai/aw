//! Real configured empty-baseline task restore. SQLite CLI is used only
//! in the explicit fault fixture, with the Store owner stopped, to fail
//! final publication after original encrypted custody. No runtime hook.
use super::{frame, frames, Temp};
use aw::{config, sessions};
use aw_agent_core::PrincipalId;
use aw_store::{AgentAdmission, QueueState, SecretStore, SessionBinding};
use aw_store_api::SessionBackend;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    net::TcpListener,
    path::Path,
    process::{Command, Output, Stdio},
};

#[test]
#[ignore = "requires sqlite3 CLI for private fixture publication fault; run explicitly"]
fn configured_empty_baseline_tasks_recover_and_abandon_original_custody() {
    let mut nonce = [0; 16];
    getrandom::fill(&mut nonce).unwrap();
    let temp = Temp(std::env::temp_dir().join(format!(
        "aw-empty-baseline-task-{:032x}",
        u128::from_be_bytes(nonce)
    )));
    aw_platform::ensure_private_dir(&temp.0).unwrap();
    let root = temp.0.canonicalize().unwrap();
    let state = root.join("state");
    let controller_root = root.join("controller");
    let fresh = root.join("fresh");
    let pending = root.join("pending");
    let abandoned = root.join("abandoned");
    let wrong = root.join("wrong");
    let absent = root.join("absent");
    for path in [&controller_root, &fresh, &pending, &abandoned, &wrong] {
        aw_platform::ensure_private_dir(path).unwrap();
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let mut profile = config::builtins().remove("llama").unwrap();
    profile.base_url = url.clone();
    profile.api_key_env = None;
    profile.model = Some("fixture".into());
    profile.allow_insecure_http = true;
    let binding = SessionBinding {
        working_directory: controller_root.clone(),
        tool_root: controller_root.clone(),
        system_prompt: "fixture".into(),
        profile_hint: "llama".into(),
        model_hint: "fixture".into(),
        destination_fingerprint: sessions::destination_fingerprint(&profile).unwrap(),
    };
    let config_path = root.join("config.toml");
    aw_platform::open_private_new(&config_path).unwrap();
    let actor = PrincipalId::new(aw_platform::local_principal().unwrap()).unwrap();
    let open = || sessions::open_sqlite_owner(&state).unwrap();
    let store = open();
    let (session, branch) = store
        .create(&actor, "baseline controller", binding.clone())
        .unwrap();
    let info = store
        .agent_binding(&actor, &session.id, &branch.id)
        .unwrap();
    let project = format!(
        "/d:{}/i:{}/p:{}",
        info.domain_handle, info.identity_handle, info.project_handle
    );
    let controller = format!("{project}/a:{}", info.agent_handle);
    let before = store.agent_get(&actor, &controller).unwrap();
    let history = store
        .checkpoint_create_history_only(&actor, &controller, None, "fixture:history")
        .unwrap();
    let auto = store
        .checkpoint_create_empty_auto(&actor, &controller, None, "fixture:auto")
        .unwrap();
    drop(store);

    let command = || {
        let mut c = Command::new(env!("CARGO_BIN_EXE_aw"));
        c.current_dir(&controller_root);
        c
    };
    let run = |mut c: Command, input: &[u8]| -> Output {
        let mut child = c
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    };
    let rpc = |path: &Path, requests: &[Value]| {
        let mut c = command();
        c.args(["rpc", "--agent-stdio", "--agent-actor-owner", "--state-dir"])
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
                "--tools-mode",
                "off",
                "--agent-checkpoint-restore-root",
            ])
            .arg(path);
        let input = requests
            .iter()
            .map(|r| format!("{r}\n"))
            .collect::<String>();
        frames(&run(c, input.as_bytes()))
    };
    fn reply<'a>(events: &'a [Value], id: &str, kind: &str) -> &'a Value {
        &events
            .iter()
            .find(|e| e["id"] == id && e["type"] == kind)
            .unwrap_or_else(|| panic!("missing {kind} for {id}: {events:?}"))["data"]
    }
    let task = "Restore history — fixture initial task.\n";
    let restore = |id: &str, checkpoint: &str| {
        frame(
            id,
            "checkpoint_restore",
            json!({"id":checkpoint,"title":id,"task":task}),
        )
    };
    let offline = |path: &Path, checkpoint: &str, id: &str, mode: &str, payload: &[u8]| {
        let mut c = command();
        c.args(["checkpoint-restore", "--state-dir"])
            .arg(&state)
            .arg(path)
            .args([
                checkpoint,
                id,
                "--session-title",
                id,
                "--agent-rpc-request",
                mode,
                "--agent-task-stdin",
                "--agent-baseline-controller",
                &controller,
            ]);
        run(c, payload)
    };
    let verify = |value: &Value, path: &Path| {
        let store = open();
        let reference = value["agent"]["ref"].as_str().unwrap();
        let agent = store.agent_get(&actor, reference).unwrap();
        assert_eq!(agent.admission, AgentAdmission::Open);
        assert_eq!(agent.project_ref, project);
        assert_eq!(
            store
                .sandbox_grant_status(&actor, reference)
                .unwrap()
                .revision,
            0
        );
        let execution = store.agent_execution_binding(&actor, reference).unwrap();
        let sandbox = store
            .sandbox_get(&actor, agent.sandbox_ref.as_deref().unwrap())
            .unwrap();
        assert_eq!(sandbox.tool_root, path);
        assert!(sandbox.sources.is_empty());
        let queue = store
            .actor_queue_list(&actor, execution.session_id())
            .unwrap();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].state, QueueState::Pending);
        assert_eq!(queue[0].request.recipient_site, "local");
        assert_eq!(
            store
                .local_secret_store()
                .unwrap()
                .get(&queue[0].payload_ref)
                .unwrap()
                .as_slice(),
            task.as_bytes()
        );
        assert!(!value.to_string().contains("fixture initial task"));
        queue[0].clone()
    };

    let request = restore("fresh", &history.id);
    let events = rpc(
        &fresh,
        &[
            frame(
                "invalid",
                "checkpoint_restore",
                json!({"id":history.id,"task":""}),
            ),
            frame(
                "inherit",
                "checkpoint_restore",
                json!({"id":history.id,"task":task,"access":"inherit"}),
            ),
            request.clone(),
        ],
    );
    reply(&events, "invalid", "error");
    reply(&events, "inherit", "error");
    let result = reply(&events, "fresh", "response").clone();
    let first = verify(&result, &fresh);
    assert_eq!(result["source_coverage"], "none");
    assert_eq!(
        reply(&rpc(&absent, &[request]), "fresh", "response"),
        &result
    );
    let output = offline(
        &absent,
        &history.id,
        "fresh",
        "--agent-recover",
        task.as_bytes(),
    );
    assert!(
        output.status.success(),
        "applied replay: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["ref"],
        result["agent"]["ref"]
    );
    assert!(!offline(
        &absent,
        &history.id,
        "fresh",
        "--agent-abandon",
        task.as_bytes()
    )
    .status
    .success());

    // Fault injection only in this disposable database. Original Store
    // publication must roll back while retaining its real task custody.
    let db = state.join("sessions.sqlite3");
    let sql = |query: &str| {
        let output = Command::new("sqlite3")
            .arg("-batch")
            .arg(&db)
            .arg(query)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "fixture sqlite3: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    let interrupt = |path: &Path, checkpoint: &str, id: &str| -> Value {
        sql("CREATE TRIGGER fixture_fail_baseline BEFORE INSERT ON audit
            WHEN NEW.action='construction.preparation'
              AND json_extract(NEW.data_json,'$.operation')='checkpoint_restore'
              AND json_extract(NEW.data_json,'$.state')='applied'
            BEGIN SELECT RAISE(ABORT,'fixture baseline publication'); END;");
        let events = rpc(path, &[restore(id, checkpoint)]);
        reply(&events, id, "error");
        sql("DROP TRIGGER fixture_fail_baseline");
        let receipt: Value = serde_json::from_str(&sql(&format!(
            "SELECT data_json FROM audit WHERE action='construction.preparation'
             AND json_extract(data_json,'$.request_id')='rpc:stdio:checkpoint-restore:{id}'
             AND json_extract(data_json,'$.state')='unknown'"
        )))
        .unwrap();
        let store = open();
        assert!(store
            .session(&actor, receipt["candidate"]["session"].as_str().unwrap())
            .is_err());
        assert!(store
            .sandbox_get(&actor, receipt["candidate"]["sandbox"].as_str().unwrap())
            .is_err());
        assert_eq!(
            store
                .local_secret_store()
                .unwrap()
                .get(receipt["task"]["payload_ref"].as_str().unwrap())
                .unwrap()
                .as_slice(),
            task.as_bytes()
        );
        receipt
    };

    let receipt = interrupt(&pending, &auto.id, "pending");
    let retry = rpc(&pending, &[restore("pending", &auto.id)]);
    assert_eq!(reply(&retry, "pending", "error")["needs_inspection"], true);
    assert!(
        !offline(&pending, &auto.id, "pending", "--agent-recover", b"changed")
            .status
            .success()
    );
    assert!(!offline(
        &wrong,
        &auto.id,
        "pending",
        "--agent-recover",
        task.as_bytes()
    )
    .status
    .success());
    fs::write(pending.join("foreign"), b"keep").unwrap();
    assert!(!offline(
        &pending,
        &auto.id,
        "pending",
        "--agent-recover",
        task.as_bytes()
    )
    .status
    .success());
    assert_eq!(fs::read(pending.join("foreign")).unwrap(), b"keep");
    fs::remove_file(pending.join("foreign")).unwrap();
    let output = offline(
        &pending,
        &auto.id,
        "pending",
        "--agent-recover",
        task.as_bytes(),
    );
    assert!(
        output.status.success(),
        "original recovery: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let recovered: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(recovered["sandbox_ref"], receipt["candidate"]["sandbox"]);
    assert_eq!(recovered["source_coverage"], "auto_all_required");
    let replay = rpc(&absent, &[restore("pending", &auto.id)]);
    let recovered = reply(&replay, "pending", "response");
    let queue = verify(recovered, &pending);
    assert_eq!(queue.payload_ref, receipt["task"]["payload_ref"]);
    assert_eq!(
        queue.request.request_id,
        receipt["task"]["queue_request_id"]
    );
    assert_eq!(queue.payload_sha256, receipt["task"]["payload_sha256"]);

    let receipt = interrupt(&abandoned, &history.id, "abandoned");
    fs::write(abandoned.join("foreign"), b"keep").unwrap();
    assert!(!offline(
        &absent,
        &history.id,
        "abandoned",
        "--agent-abandon",
        b"changed"
    )
    .status
    .success());
    for _ in 0..2 {
        let output = offline(
            &absent,
            &history.id,
            "abandoned",
            "--agent-abandon",
            task.as_bytes(),
        );
        assert!(
            output.status.success(),
            "abandonment: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap()["status"],
            "rejected"
        );
    }
    assert_eq!(fs::read(abandoned.join("foreign")).unwrap(), b"keep");
    reply(
        &rpc(&abandoned, &[restore("abandoned", &history.id)]),
        "abandoned",
        "error",
    );
    let store = open();
    assert!(store
        .local_secret_store()
        .unwrap()
        .get(receipt["task"]["payload_ref"].as_str().unwrap())
        .is_err());
    assert!(store
        .session(&actor, receipt["candidate"]["session"].as_str().unwrap())
        .is_err());
    assert_eq!(store.agent_get(&actor, &controller).unwrap(), before);
    let original = store
        .agent_execution_binding(&actor, result["agent"]["ref"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        store
            .actor_queue_list(&actor, original.session_id())
            .unwrap(),
        vec![first]
    );
    assert!(matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
    assert!(!absent.exists());
}
