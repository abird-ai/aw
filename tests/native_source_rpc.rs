//! Actual executable, private Store/Git fixture and loopback provider.
//! The model only sees the approved native filesystem schema. Every
//! queue/Turn goes through the real stdio owner; no test-only host route.
#![cfg(all(feature = "workspace", unix))]

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
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn frames(output: &Output) -> Vec<Value> {
    assert!(
        output.status.success(),
        "RPC failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn frame(id: &str, method: &str, params: Value) -> Value {
    json!({"v":1,"id":id,"method":method,"params":params})
}

fn settled<'a>(events: &'a [Value], id: &str) -> &'a Value {
    &events
        .iter()
        .find(|event| event["id"] == id && event["type"] == "settled")
        .unwrap_or_else(|| panic!("missing settlement for {id}: {events:?}"))["data"]
}

fn fixture_provider() -> (String, Arc<AtomicUsize>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let thread = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(180);
        while counter.load(Ordering::SeqCst) < 6 {
            assert!(Instant::now() < deadline, "provider fixture deadline");
            let (mut socket, _) = match listener.accept() {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(error) => panic!("provider accept: {error}"),
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
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse().unwrap())
                        })
                        .unwrap();
                    if bytes.len() >= end + 4 + length {
                        break serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap();
                    }
                }
            };
            let mut names: Vec<_> = request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tool| tool["function"]["name"].as_str().unwrap())
                .collect();
            names.sort_unstable();
            assert_eq!(names, ["read", "write"]);
            let ordinal = counter.fetch_add(1, Ordering::SeqCst);
            let (delta, reason) = match ordinal % 3 {
                0 => (
                    json!({"tool_calls":[{"index":0,"id":format!("read-{ordinal}"),"type":"function",
                    "function":{"name":"read","arguments":json!({"path":"tracked"}).to_string()}}]}),
                    "tool_calls",
                ),
                1 => {
                    assert!(request["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|message| message["role"] == "tool"
                            && message["content"].to_string().contains("fixture data")));
                    (
                        json!({"tool_calls":[{"index":0,"id":format!("write-{ordinal}"),"type":"function",
                        "function":{"name":"write","arguments":json!({"path":"created","content":"native rpc"}).to_string()}}]}),
                        "tool_calls",
                    )
                }
                _ => (json!({"content":"done"}), "stop"),
            };
            let body = format!(
                "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"index":0,"delta":delta}]}),
                json!({"choices":[{"index":0,"delta":{},"finish_reason":reason}]})
            );
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        }
    });
    (url, calls, thread)
}

#[test]
fn configured_native_root_stdio_uses_original_actor_and_current_grant() {
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).unwrap();
    let temp = Temp(std::env::temp_dir().join(format!(
        "aw-native-root-rpc-{:032x}",
        u128::from_be_bytes(nonce)
    )));
    aw_platform::ensure_private_dir(&temp.0).unwrap();
    let root = temp.0.canonicalize().unwrap();
    let state = root.join("state");
    let repo = root.join("repo");
    let initial = root.join("initial");
    let approvals = root.join("approval");
    for path in [&state, &repo, &initial, &approvals] {
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
            "Git fixture: {}",
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
    fs::write(repo.join("tracked"), b"fixture data").unwrap();
    git(&["add", "tracked"]);
    git(&["commit", "--no-gpg-sign", "-qm", "fixture"]);
    let head = git(&["rev-parse", "HEAD"]);
    let index = fs::read(repo.join(".git/index")).unwrap();

    let (url, calls, provider) = fixture_provider();
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
    aw_platform::open_private_new(&config_path).unwrap(); // empty config, no user credentials
    let manager = PrincipalId::new(aw_platform::local_principal().unwrap()).unwrap();
    let registry = SourceDriverRegistry::new();
    let handle = GitSourceDriver::new(&executable)
        .unwrap()
        .register(&registry)
        .unwrap();
    let store =
        SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
    let (parent, branch) = store.create(&manager, "fixture", layout(&initial)).unwrap();
    let info = store
        .agent_binding(&manager, &parent.id, &branch.id)
        .unwrap();
    let parent_ref = format!(
        "/d:{}/i:{}/p:{}/a:{}",
        info.domain_handle, info.identity_handle, info.project_handle, info.agent_handle
    );
    let history = store
        .checkpoint_create_history_only(&manager, &parent_ref, None, "fixture:history")
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
    let restored = store
        .checkpoint_restore_baseline(
            &manager,
            &CheckpointRestoreRequest {
                checkpoint: history.id,
                project: None,
                title: None,
                access: AgentHistoryForkAccess::Private,
                task: None,
            },
            &layout(&repo),
            &[binding],
            "fixture:baseline",
            "local",
        )
        .unwrap();
    let reference = restored.agent.canonical_ref;
    let execution = store.agent_execution_binding(&manager, &reference).unwrap();
    let session = execution.session_id().to_owned();
    let branch = execution.branch_id().to_owned();
    let owner = store.session(&manager, &session).unwrap().owner;
    let before = store.agent_get(&manager, &reference).unwrap();
    assert_eq!(before.admission, AgentAdmission::Held);
    let sandbox = store
        .sandbox_get(&manager, before.sandbox_ref.as_deref().unwrap())
        .unwrap();
    let mut approval = json!({
        "version":1,"store_directory":state,"store_generation":store.local_process_info().unwrap().store_generation,
        "manager":manager,"actor":owner,"agent_ref":reference,"sandbox_ref":sandbox.canonical_ref,
        "sandbox_rev":sandbox.rev,"policy_ref":"host:fixture-native-root","base":repo,
        "directories":[{"name":name,"repository":repo,
            "repository_identity":aw_platform::DirectoryIdentity::open(&repo).unwrap().identity().unwrap(),
            "binding_rev":sandbox.sources[0].binding_rev,"access":"read_write"}]
    });
    let approval_path = approvals.join("git-native.json");
    aw_platform::open_private_new(&approval_path)
        .unwrap()
        .write_all(&serde_json::to_vec(&approval).unwrap())
        .unwrap();
    drop(store);

    let invoke = |native: bool,
                  grant: Option<u64>,
                  open: Option<u64>,
                  tools: &str,
                  extras: &[&str],
                  input: &[Value]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aw"));
        command
            .current_dir(&initial)
            .env("AW_GIT", &executable)
            .args(["rpc", "--agent-stdio", "--agent-actor-owner", "--state-dir"])
            .arg(&state)
            .args([
                "--session-id",
                &session,
                "--session-branch",
                &branch,
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
            ]);
        if native {
            command
                .arg("--agent-native-source-root")
                .arg(&approvals)
                .args(["--tools-mode", "rw", "--tools-allow", tools]);
        }
        if let Some(rev) = grant {
            command
                .arg("--agent-native-source-grant-rev")
                .arg(rev.to_string());
        }
        if let Some(rev) = open {
            command
                .arg("--agent-native-source-open-rev")
                .arg(rev.to_string());
        }
        command.args(extras);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        for request in input {
            writeln!(stdin, "{request}").unwrap();
        }
        drop(stdin);
        child.wait_with_output().unwrap()
    };
    for result in [
        invoke(false, None, None, "", &[], &[]),
        invoke(true, None, Some(before.rev), "read,write", &[], &[]),
        invoke(true, Some(0), None, "read,write", &[], &[]),
        invoke(true, Some(0), Some(before.rev + 1), "read,write", &[], &[]),
        invoke(true, Some(0), Some(before.rev), "read,bash", &[], &[]),
        invoke(
            true,
            Some(0),
            Some(before.rev),
            "read,write",
            &["--agent-checkpoint-create"],
            &[],
        ),
    ] {
        assert!(
            !result.status.success(),
            "unsupported startup reached stdio"
        );
    }
    let mut wrong = approval.clone();
    wrong["directories"][0]["repository_identity"] = json!([0, 0]);
    fs::write(&approval_path, serde_json::to_vec(&wrong).unwrap()).unwrap();
    assert!(
        !invoke(true, Some(0), Some(before.rev), "read,write", &[], &[])
            .status
            .success()
    );
    fs::write(&approval_path, serde_json::to_vec(&approval).unwrap()).unwrap();
    let store =
        SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
    assert_eq!(store.agent_get(&manager, &reference).unwrap(), before);
    assert_eq!(
        store
            .sandbox_grant_status(&manager, &reference)
            .unwrap()
            .revision,
        0
    );
    drop(store);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let first = frames(&invoke(
        true,
        Some(0),
        Some(before.rev),
        "read,write",
        &[],
        &[
            frame("raw", "prompt", json!({"text":"unowned prompt"})),
            frame(
                "enqueue",
                "actor_enqueue",
                json!({"request_id":"fixture-first","message_id":1,"text":"first native task"}),
            ),
            frame("step", "actor_step", json!({})),
            frame(
                "pending",
                "actor_enqueue",
                json!({"request_id":"fixture-pending","message_id":20,"text":"second native task"}),
            ),
        ],
    ));
    assert_eq!(first[0]["data"]["capabilities"]["actor_step"], true);
    assert_eq!(first[0]["data"]["capabilities"]["prompt"], false);
    assert!(first.iter().any(|event| event["id"] == "raw"
        && event["type"] == "error"
        && event["data"]["code"] == "unsupported_method"));
    assert_eq!(first[0]["data"]["capabilities"]["agent_create"], false);
    assert_eq!(
        first[0]["data"]["capabilities"]["sandbox_sources_set"],
        false
    );
    assert_eq!(settled(&first, "enqueue")["status"], "settled");
    let turn = settled(&first, "step");
    assert_eq!(turn["status"], "settled", "{first:?}");
    assert_eq!(turn["result"]["outcome"], "completed", "{turn:?}");
    assert_eq!(settled(&first, "pending")["status"], "settled");
    assert_eq!(fs::read(repo.join("created")).unwrap(), b"native rpc");
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let store =
        SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
    assert_eq!(
        store.agent_get(&manager, &reference).unwrap().admission,
        AgentAdmission::Open
    );
    let grant = store.sandbox_grant_status(&manager, &reference).unwrap();
    assert_eq!(grant.revision, 1);
    assert_eq!(
        store
            .read_agent_branch_view(&owner, &session, &branch)
            .unwrap()
            .history
            .turns
            .len(),
        1
    );
    let current = store.sandbox_get(&manager, &sandbox.canonical_ref).unwrap();
    assert!(current.rev > sandbox.rev);
    assert_eq!(current.sources, sandbox.sources);
    drop(store);
    // A new process cannot reuse stale approval revision or grant evidence.
    assert!(
        !invoke(true, Some(grant.revision), None, "read,write", &[], &[])
            .status
            .success()
    );
    approval["sandbox_rev"] = json!(current.rev);
    fs::write(&approval_path, serde_json::to_vec(&approval).unwrap()).unwrap();
    assert!(!invoke(true, Some(0), None, "read,write", &[], &[])
        .status
        .success());
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let second = frames(&invoke(
        true,
        Some(grant.revision),
        None,
        "read,write",
        &[],
        &[
            frame("resume-step", "actor_step", json!({})),
            frame("idle", "actor_step", json!({})),
        ],
    ));
    assert_eq!(
        settled(&second, "resume-step")["result"]["outcome"],
        "completed",
        "{second:?}"
    );
    assert_eq!(settled(&second, "idle")["status"], "idle");
    provider.join().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 6);
    let store = SessionStore::open_with_sources(&state, registry, Default::default()).unwrap();
    let view = store
        .read_agent_branch_view(&owner, &session, &branch)
        .unwrap();
    assert_eq!(view.history.turns.len(), 2);
    assert_eq!(view.history.turns[1].prompt, "second native task");
    assert!(store
        .actor_queue_list(&owner, &session)
        .unwrap()
        .iter()
        .all(|fact| fact.state == aw_store::QueueState::Settled));
    assert_eq!(
        store
            .sandbox_grant_status(&manager, &reference)
            .unwrap()
            .revision,
        2
    );
    assert!(store.activate(&owner, &session, &branch).is_err());
    assert_eq!(git(&["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(repo.join("tracked")).unwrap(), b"fixture data");
}
