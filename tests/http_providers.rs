//! Actual loopback HTTP integration fixtures. No provider accounts are used.
use aw::{
    config,
    conversation::{Conversation, Outcome},
    provider::HttpProvider,
};
use serde_json::Value;
use std::{future::pending, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};

async fn fixture(
    status: u16,
    content_type: &str,
    body: Vec<u8>,
) -> (String, oneshot::Receiver<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = oneshot::channel();
    let content_type = content_type.to_owned();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];
        loop {
            let n = socket.read(&mut buffer).await.unwrap();
            if n == 0 {
                return;
            }
            request.extend_from_slice(&buffer[..n]);
            assert!(request.len() < 2 * 1024 * 1024);
            if let Some(end) = request.windows(4).position(|x| x == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]);
                let len = headers
                    .lines()
                    .find_map(|l| {
                        l.split_once(':')
                            .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                            .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                    })
                    .unwrap();
                if request.len() >= end + 4 + len {
                    let value: Value =
                        serde_json::from_slice(&request[end + 4..end + 4 + len]).unwrap();
                    let _ = tx.send(value);
                    break;
                }
            }
        }
        let header=format!("HTTP/1.1 {status} fixture\r\nContent-Type: {content_type}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n");
        if socket.write_all(header.as_bytes()).await.is_err() {
            return;
        }
        // Small chunks include splits inside JSON and multibyte UTF-8.
        for part in body.chunks(7) {
            if socket
                .write_all(format!("{:x}\r\n", part.len()).as_bytes())
                .await
                .is_err()
            {
                return;
            }
            if socket.write_all(part).await.is_err() {
                return;
            }
            if socket.write_all(b"\r\n").await.is_err() {
                return;
            }
            tokio::task::yield_now().await;
        }
        let _ = socket.write_all(b"0\r\n\r\n").await;
    });
    (format!("http://{addr}"), rx)
}
fn sse(text: &str) -> Vec<u8> {
    format!("data: {}\r\n\r\ndata: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n",serde_json::json!({"choices":[{"index":0,"delta":{"content":text}}]})).into_bytes()
}
#[tokio::test]
async fn openai_stream_and_request_shape() {
    let (base, request) = fixture(200, "text/event-stream", sse("héllo 🐦")).await;
    let mut p = config::builtins().remove("llama").unwrap();
    p.base_url = format!("{base}/v1");
    let provider = HttpProvider::new(p, Duration::from_secs(2)).unwrap();
    let mut c = Conversation::new("system".into()).unwrap();
    let mut seen = String::new();
    let t = c
        .send(
            &provider,
            "fixture",
            "model",
            "question".into(),
            Duration::from_secs(5),
            1024,
            pending(),
            |s| {
                seen.push_str(s);
                Ok(())
            },
        )
        .await
        .unwrap();
    assert_eq!(t.outcome, Outcome::Completed);
    assert_eq!(seen, "héllo 🐦");
    let sent = request.await.unwrap();
    assert_eq!(sent["stream"], true);
    assert_eq!(sent["messages"][0]["role"], "system");
    assert_eq!(sent["messages"][1]["content"], "question");
    assert!(sent.get("tools").is_none());
    assert!(sent["messages"][0].get("id").is_none());
}
#[tokio::test]
async fn ollama_stream_and_usage() {
    let body="{\"message\":{\"content\":\"hello\"},\"done\":false}\n{\"message\":{\"content\":\" world\"},\"done\":true,\"done_reason\":\"stop\",\"eval_count\":2}";
    let (base, request) = fixture(200, "application/x-ndjson", body.as_bytes().to_vec()).await;
    let mut p = config::builtins().remove("ollama").unwrap();
    p.base_url = base;
    let provider = HttpProvider::new(p, Duration::from_secs(2)).unwrap();
    let mut c = Conversation::new("".into()).unwrap();
    let t = c
        .send(
            &provider,
            "fixture",
            "model",
            "question".into(),
            Duration::from_secs(5),
            1024,
            pending(),
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(t.outcome, Outcome::Completed);
    assert_eq!(t.answer, "hello world");
    assert_eq!(t.meta.as_ref().unwrap().usage.output_tokens, Some(2));
    let sent = request.await.unwrap();
    assert_eq!(sent["think"], false);
}
#[tokio::test]
async fn status_body_is_not_echoed() {
    let (base, _) = fixture(
        401,
        "application/json",
        br#"{"error":"SECRET_API_KEY"}"#.to_vec(),
    )
    .await;
    let mut p = config::builtins().remove("llama").unwrap();
    p.base_url = base;
    let provider = HttpProvider::new(p, Duration::from_secs(2)).unwrap();
    let mut c = Conversation::new("".into()).unwrap();
    let t = c
        .send(
            &provider,
            "fixture",
            "model",
            "question".into(),
            Duration::from_secs(5),
            1024,
            pending(),
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(t.outcome, Outcome::Failed);
    assert!(!t.error.as_ref().unwrap().contains("SECRET_API_KEY"));
    assert!(t.error.as_ref().unwrap().contains("401"));
}
#[tokio::test]
async fn truncated_stream_does_not_pollute_context() {
    let (base, _) = fixture(
        200,
        "text/event-stream",
        b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n".to_vec(),
    )
    .await;
    let mut p = config::builtins().remove("llama").unwrap();
    p.base_url = base;
    let provider = HttpProvider::new(p, Duration::from_secs(2)).unwrap();
    let mut c = Conversation::new("".into()).unwrap();
    let t = c
        .send(
            &provider,
            "fixture",
            "model",
            "question".into(),
            Duration::from_secs(5),
            1024,
            pending(),
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(t.answer, "partial");
    assert_eq!(t.outcome, Outcome::Failed);
    assert_eq!(c.messages_for("next").unwrap().len(), 1);
}
#[tokio::test]
async fn redirects_are_not_followed() {
    let (base, _) = fixture(302, "text/plain", b"redirect".to_vec()).await;
    let mut p = config::builtins().remove("llama").unwrap();
    p.base_url = base;
    let provider = HttpProvider::new(p, Duration::from_secs(2)).unwrap();
    let mut c = Conversation::new("".into()).unwrap();
    let t = c
        .send(
            &provider,
            "fixture",
            "model",
            "question".into(),
            Duration::from_secs(5),
            1024,
            pending(),
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert!(t.error.as_ref().unwrap().contains("302"));
}
#[tokio::test]
async fn non_streaming_response_is_explicitly_rejected() {
    let (base, _) = fixture(200, "application/json", b"{}".to_vec()).await;
    let mut p = config::builtins().remove("llama").unwrap();
    p.base_url = base;
    let provider = HttpProvider::new(p, Duration::from_secs(2)).unwrap();
    let mut c = Conversation::new("".into()).unwrap();
    let t = c
        .send(
            &provider,
            "fixture",
            "model",
            "question".into(),
            Duration::from_secs(5),
            1024,
            pending(),
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(t.outcome, Outcome::Failed);
    assert!(t.error.as_ref().unwrap().contains("SSE"));
}
