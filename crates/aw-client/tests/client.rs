#![cfg(target_os = "linux")]
use aw_client::{framing, local, protocol::*, Client, Command, Error};
use serde_json::{json, Value};
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
    time::Duration,
};
use tokio::io::{duplex, AsyncWriteExt};

fn prompt() -> PromptInput {
    PromptInput {
        request_id: "once".into(),
        expected_revision: 1,
        prompt: "Hello".into(),
        account_id: "mine".into(),
        model: "small".into(),
        session_id: None,
        branch_id: None,
        retry_operation_id: None,
    }
}
fn frame(value: &impl serde::Serialize) -> Vec<u8> {
    let b = serde_json::to_vec(value).unwrap();
    let mut out = (b.len() as u32).to_be_bytes().to_vec();
    out.extend(b);
    out
}
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "awc-{}-{}",
            std::process::id(),
            aw_client::request_id().unwrap()
        ));
        std::fs::create_dir(&p).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self(p)
    }
    fn socket(&self) -> PathBuf {
        self.0.join("o.sock")
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn hello() -> Hello {
    Hello {
        protocol: VERSION,
        instance: "a".repeat(64),
        principal: format!("unix:{}", local::effective_uid().unwrap()),
        owner_uid: local::effective_uid().unwrap(),
        package_version: "test".into(),
        capabilities: vec![],
        shared_active_view: true,
        inference_location: "owner".into(),
    }
}
async fn serve_hello(socket: &mut tokio::net::UnixStream, reader: &mut framing::Reader) {
    let first: Request = reader
        .next(socket, MAX_REQUEST_BYTES)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.id, 1);
    framing::write(
        socket,
        &Response {
            version: VERSION,
            id: 1,
            packet: Packet::Result {
                value: serde_json::to_value(hello()).unwrap(),
            },
        },
        MAX_RESPONSE_BYTES,
    )
    .await
    .unwrap();
}
#[test]
fn valid_prompt_contract() {
    assert!(prompt().validate().is_ok());
}
#[test]
fn blank_prompt_refused() {
    let mut p = prompt();
    p.prompt = " \n".into();
    assert!(p.validate().is_err());
}
#[test]
fn oversized_prompt_refused() {
    let mut p = prompt();
    p.prompt = "x".repeat(MAX_PROMPT_BYTES + 1);
    assert!(p.validate().is_err());
}
#[test]
fn invalid_request_id_refused() {
    let mut p = prompt();
    p.request_id = "../identity".into();
    assert!(p.validate().is_err());
}
#[test]
fn half_saved_target_refused() {
    let mut p = prompt();
    p.session_id = Some("s1".into());
    assert!(p.validate().is_err());
}
#[test]
fn actor_and_key_fields_refused() {
    for key in ["actor", "api_key", "endpoint", "tool_root", "payer"] {
        let mut p = serde_json::to_value(prompt()).unwrap();
        p[key] = json!("untrusted");
        assert!(serde_json::from_value::<PromptInput>(p).is_err());
    }
}
#[test]
fn control_char_model_refused() {
    let mut p = prompt();
    p.model = "m\u{1b}[1m".into();
    assert!(p.validate().is_err());
}
#[test]
fn prompt_command_roundtrip() {
    let c = Command::Prompt(prompt());
    let j = serde_json::to_value(&c).unwrap();
    assert_eq!(j["op"], "prompt");
    assert!(serde_json::from_value::<Command>(j)
        .unwrap()
        .validate()
        .is_ok());
}
#[test]
fn unknown_command_rejected() {
    assert!(serde_json::from_value::<Command>(json!({"op":"shell","command":"whoami"})).is_err());
}
#[test]
fn extra_snapshot_arguments_refused() {
    assert!(serde_json::from_value::<Command>(
        json!({"op":"inspect","session_id":"s","branch":"b","api_key":"secret"})
    )
    .is_err());
}
#[test]
fn root_request_extra_authority_refused() {
    assert!(serde_json::from_value::<Request>(
        json!({"version":1,"id":1,"expected_instance":null,"command":{"op":"hello"},"uid":0})
    )
    .is_err());
}
#[test]
fn zero_run_refused() {
    assert!(Command::Cancel { run_id: 0 }.validate().is_err());
    assert!(Command::Follow { run_id: 0 }.validate().is_err());
}
#[test]
fn checkpoint_is_bounded() {
    assert!(Command::Checkpoint {
        expected_revision: 0,
        summary: " ".into()
    }
    .validate()
    .is_err());
}
#[test]
fn random_ids_are_distinct() {
    assert_ne!(
        aw_client::request_id().unwrap(),
        aw_client::request_id().unwrap()
    );
}
#[tokio::test]
async fn framing_roundtrip() {
    let (mut w, mut r) = duplex(1024);
    framing::write(&mut w, &json!({"ok":"λ"}), 1024)
        .await
        .unwrap();
    let v: Value = framing::Reader::new()
        .next(&mut r, 1024)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v["ok"], "λ");
}
#[tokio::test]
async fn coalesced_frames_remain_separate() {
    let (mut w, mut r) = duplex(1024);
    w.write_all(&[frame(&json!(1)), frame(&json!(2))].concat())
        .await
        .unwrap();
    let mut d = framing::Reader::new();
    assert_eq!(
        d.next::<_, Value>(&mut r, 1024).await.unwrap().unwrap(),
        json!(1)
    );
    assert_eq!(
        d.next::<_, Value>(&mut r, 1024).await.unwrap().unwrap(),
        json!(2)
    );
}
#[tokio::test]
async fn partial_header_survives_cancelled_poll() {
    let (mut w, mut r) = duplex(1024);
    let f = frame(&json!({"x":1}));
    w.write_all(&f[..2]).await.unwrap();
    let mut d = framing::Reader::new();
    assert!(
        tokio::time::timeout(Duration::from_millis(5), d.next::<_, Value>(&mut r, 1024))
            .await
            .is_err()
    );
    w.write_all(&f[2..]).await.unwrap();
    assert_eq!(
        d.next::<_, Value>(&mut r, 1024).await.unwrap().unwrap(),
        json!({"x":1})
    );
}
#[tokio::test]
async fn partial_body_survives_cancelled_poll() {
    let (mut w, mut r) = duplex(1024);
    let f = frame(&json!({"x":"🌿"}));
    w.write_all(&f[..7]).await.unwrap();
    let mut d = framing::Reader::new();
    assert!(
        tokio::time::timeout(Duration::from_millis(5), d.next::<_, Value>(&mut r, 1024))
            .await
            .is_err()
    );
    w.write_all(&f[7..]).await.unwrap();
    assert_eq!(
        d.next::<_, Value>(&mut r, 1024).await.unwrap().unwrap()["x"],
        "🌿"
    );
}
#[tokio::test]
async fn clean_eof() {
    let (w, mut r) = duplex(64);
    drop(w);
    assert!(framing::Reader::new()
        .next::<_, Value>(&mut r, 64)
        .await
        .unwrap()
        .is_none());
}
#[tokio::test]
async fn truncated_header() {
    let (mut w, mut r) = duplex(64);
    w.write_all(&[0, 0]).await.unwrap();
    drop(w);
    assert!(framing::Reader::new()
        .next::<_, Value>(&mut r, 64)
        .await
        .is_err());
}
#[tokio::test]
async fn truncated_body() {
    let (mut w, mut r) = duplex(64);
    w.write_all(&[0, 0, 0, 5, b'{']).await.unwrap();
    drop(w);
    assert!(framing::Reader::new()
        .next::<_, Value>(&mut r, 64)
        .await
        .is_err());
}
#[tokio::test]
async fn zero_and_large_header_refused_before_body() {
    for len in [0u32, 4096] {
        let (mut w, mut r) = duplex(64);
        w.write_all(&len.to_be_bytes()).await.unwrap();
        assert!(framing::Reader::new()
            .next::<_, Value>(&mut r, 1024)
            .await
            .is_err());
    }
}
#[tokio::test]
async fn malformed_json_refused() {
    let (mut w, mut r) = duplex(64);
    w.write_all(&[0, 0, 0, 1, b'{']).await.unwrap();
    assert!(framing::Reader::new()
        .next::<_, Value>(&mut r, 64)
        .await
        .is_err());
}
#[tokio::test]
async fn outgoing_limit_refused() {
    let (mut w, _r) = duplex(64);
    assert!(framing::write(&mut w, &"too long", 2).await.is_err());
}
#[test]
fn relative_and_dot_paths_refused() {
    let uid = local::effective_uid().unwrap();
    for p in [
        "relative.sock",
        "/tmp/../x.sock",
        "/tmp/./x.sock",
        "/tmp//x.sock",
    ] {
        assert!(local::private_parent(std::path::Path::new(p), uid).is_err());
    }
}
#[test]
fn permissive_parent_refused() {
    let d = Dir::new();
    std::fs::set_permissions(&d.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(local::private_parent(&d.socket(), local::effective_uid().unwrap()).is_err());
}
#[test]
fn symlink_parent_refused() {
    let d = Dir::new();
    let link = d.0.join("link");
    std::os::unix::fs::symlink(&d.0, &link).unwrap();
    assert!(local::private_parent(&link.join("socket"), local::effective_uid().unwrap()).is_err());
}
#[tokio::test]
async fn bind_is_private_and_refuses_existing() {
    let d = Dir::new();
    let bound = local::BoundSocket::bind(&d.socket()).unwrap();
    assert_eq!(std::fs::metadata(d.socket()).unwrap().mode() & 0o777, 0o600);
    assert!(local::BoundSocket::bind(&d.socket()).is_err());
    drop(bound);
    assert!(!d.socket().exists());
}
#[tokio::test]
async fn cleanup_does_not_delete_replacement() {
    let d = Dir::new();
    let old = local::BoundSocket::bind(&d.socket()).unwrap();
    std::fs::remove_file(d.socket()).unwrap();
    let new = local::BoundSocket::bind(&d.socket()).unwrap();
    drop(old);
    assert!(d.socket().exists());
    drop(new);
}
#[tokio::test]
async fn verified_peer_handshake_and_snapshot() {
    let d = Dir::new();
    let bound = local::BoundSocket::bind(&d.socket()).unwrap();
    let task = tokio::spawn(async move {
        let (mut s, _) = bound.listener.accept().await.unwrap();
        assert_eq!(
            s.peer_cred().unwrap().uid(),
            local::effective_uid().unwrap()
        );
        let mut r = framing::Reader::new();
        serve_hello(&mut s, &mut r).await;
        let c: Request = r.next(&mut s, MAX_REQUEST_BYTES).await.unwrap().unwrap();
        assert_eq!(c.expected_instance, Some("a".repeat(64)));
        framing::write(
            &mut s,
            &Response {
                version: 1,
                id: c.id,
                packet: Packet::Result {
                    value: json!({"fixture":true}),
                },
            },
            MAX_RESPONSE_BYTES,
        )
        .await
        .unwrap();
    });
    let mut c = Client::connect(&d.socket()).await.unwrap();
    let v: Value = c.call(Command::Accounts).await.unwrap();
    assert_eq!(v["fixture"], true);
    task.await.unwrap();
}
#[tokio::test]
async fn bad_server_identity_refused() {
    let d = Dir::new();
    let bound = local::BoundSocket::bind(&d.socket()).unwrap();
    let t = tokio::spawn(async move {
        let (mut s, _) = bound.listener.accept().await.unwrap();
        let _: Request = framing::Reader::new()
            .next(&mut s, MAX_REQUEST_BYTES)
            .await
            .unwrap()
            .unwrap();
        let mut h = hello();
        h.principal = "someone_else".into();
        framing::write(
            &mut s,
            &Response {
                version: 1,
                id: 1,
                packet: Packet::Result {
                    value: serde_json::to_value(h).unwrap(),
                },
            },
            MAX_RESPONSE_BYTES,
        )
        .await
        .unwrap();
    });
    assert!(Client::connect(&d.socket()).await.is_err());
    t.await.unwrap();
}
#[tokio::test]
async fn cancelled_rpc_poisoned_without_replay() {
    let d = Dir::new();
    let bound = local::BoundSocket::bind(&d.socket()).unwrap();
    let t = tokio::spawn(async move {
        let (mut s, _) = bound.listener.accept().await.unwrap();
        let mut r = framing::Reader::new();
        serve_hello(&mut s, &mut r).await;
        let _: Request = r.next(&mut s, MAX_REQUEST_BYTES).await.unwrap().unwrap();
        tokio::time::sleep(Duration::from_millis(40)).await;
    });
    let mut c = Client::connect(&d.socket()).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(5), c.call::<Value>(Command::Snapshot))
            .await
            .is_err()
    );
    assert!(!c.is_connected());
    assert!(matches!(
        c.call::<Value>(Command::Accounts).await,
        Err(Error::Closed)
    ));
    t.await.unwrap();
}
#[tokio::test]
async fn follow_end_restores_command_mode() {
    let d = Dir::new();
    let bound = local::BoundSocket::bind(&d.socket()).unwrap();
    let t = tokio::spawn(async move {
        let (mut s, _) = bound.listener.accept().await.unwrap();
        let mut r = framing::Reader::new();
        serve_hello(&mut s, &mut r).await;
        let q: Request = r.next(&mut s, MAX_REQUEST_BYTES).await.unwrap().unwrap();
        for packet in [
            Packet::Result {
                value: json!({"following":1}),
            },
            Packet::Event {
                sequence: 1,
                event: "delta".into(),
                data: json!({"run_id":1,"text":"x"}),
            },
            Packet::End {
                reason: "done".into(),
            },
        ] {
            framing::write(
                &mut s,
                &Response {
                    version: 1,
                    id: q.id,
                    packet,
                },
                MAX_RESPONSE_BYTES,
            )
            .await
            .unwrap();
        }
        let q: Request = r.next(&mut s, MAX_REQUEST_BYTES).await.unwrap().unwrap();
        framing::write(
            &mut s,
            &Response {
                version: 1,
                id: q.id,
                packet: Packet::Result { value: json!([]) },
            },
            MAX_RESPONSE_BYTES,
        )
        .await
        .unwrap();
    });
    let mut c = Client::connect(&d.socket()).await.unwrap();
    assert!(c
        .call::<Value>(Command::Follow { run_id: 1 })
        .await
        .is_err());
    c.start_follow(1).await.unwrap();
    assert!(c.call::<Value>(Command::Accounts).await.is_err());
    assert!(matches!(
        c.next_event().await.unwrap(),
        Some(Packet::Event { .. })
    ));
    assert!(matches!(
        c.next_event().await.unwrap(),
        Some(Packet::End { .. })
    ));
    let _: Value = c.call(Command::Accounts).await.unwrap();
    t.await.unwrap();
}

#[tokio::test]
async fn oversized_handshake_metadata_is_rejected() {
    let d = Dir::new();
    let bound = local::BoundSocket::bind(&d.socket()).unwrap();
    let t = tokio::spawn(async move {
        let (mut socket, _) = bound.listener.accept().await.unwrap();
        let _: Request = framing::Reader::new()
            .next(&mut socket, MAX_REQUEST_BYTES)
            .await
            .unwrap()
            .unwrap();
        let mut h = hello();
        h.capabilities = vec!["x".to_owned(); 33];
        framing::write(
            &mut socket,
            &Response {
                version: VERSION,
                id: 1,
                packet: Packet::Result {
                    value: serde_json::to_value(h).unwrap(),
                },
            },
            MAX_RESPONSE_BYTES,
        )
        .await
        .unwrap();
    });
    assert!(matches!(
        Client::connect(&d.socket()).await,
        Err(Error::Protocol(_))
    ));
    t.await.unwrap();
}

#[tokio::test]
async fn unsolicited_streaming_reply_poisons_ordinary_call() {
    let d = Dir::new();
    let bound = local::BoundSocket::bind(&d.socket()).unwrap();
    let t = tokio::spawn(async move {
        let (mut socket, _) = bound.listener.accept().await.unwrap();
        let mut reader = framing::Reader::new();
        serve_hello(&mut socket, &mut reader).await;
        let request: Request = reader
            .next(&mut socket, MAX_REQUEST_BYTES)
            .await
            .unwrap()
            .unwrap();
        framing::write(
            &mut socket,
            &Response {
                version: VERSION,
                id: request.id,
                packet: Packet::Event {
                    sequence: 1,
                    event: "unexpected".into(),
                    data: json!({}),
                },
            },
            MAX_RESPONSE_BYTES,
        )
        .await
        .unwrap();
    });
    let mut client = Client::connect(&d.socket()).await.unwrap();
    assert!(matches!(
        client.call::<Value>(Command::Snapshot).await,
        Err(Error::Protocol(_))
    ));
    assert!(!client.is_connected());
    assert!(matches!(
        client.call::<Value>(Command::Accounts).await,
        Err(Error::Closed)
    ));
    t.await.unwrap();
}
