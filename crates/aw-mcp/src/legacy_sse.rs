//! Deprecated MCP 2024-11-05 transport-only bridge.
//! The official rmcp SDK still handles typed JSON-RPC, tool schemas, lifecycle
//! and tool calls. This module owns only the two-endpoint GET SSE / POST wire.
//! No fallback, reconnect, ambiguous effect replay or alternate Agent loop.
use aw_agent_core::{Error, Result};
use futures_util::StreamExt;
use reqwest::{
    header::{ACCEPT, CONTENT_TYPE},
    Client, Url,
};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncWriteExt, DuplexStream},
    task::JoinHandle,
};
use tokio_util::{
    codec::{FramedRead, LinesCodec},
    sync::CancellationToken,
};
use zeroize::Zeroizing;

const MAX_FRAME: usize = 128 * 1024;
const MAX_POST: usize = MAX_FRAME;
const MAX_ENDPOINT_BYTES: usize = 2048;
const POST_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Transport task ownership is scoped to the lifetime of the MCP connection.
pub(super) struct Pumps {
    stop: CancellationToken,
    inbound: JoinHandle<()>,
    outbound: JoinHandle<()>,
}
impl Drop for Pumps {
    fn drop(&mut self) {
        self.stop.cancel();
        self.inbound.abort();
        self.outbound.abort();
    }
}

pub(super) fn bearer(name: Option<&str>) -> Result<Option<Arc<Zeroizing<String>>>> {
    let Some(name) = name else {
        return Ok(None);
    };
    let value = Zeroizing::new(
        std::env::var(name).map_err(|_| Error::Denied("selected MCP SSE bearer unavailable"))?,
    );
    if value.is_empty() || value.len() > 4096 || value.contains(['\r', '\n']) {
        return Err(Error::Denied("invalid selected MCP SSE bearer"));
    }
    Ok(Some(Arc::new(value)))
}
fn request_auth(
    request: reqwest::RequestBuilder,
    bearer: Option<&Arc<Zeroizing<String>>>,
) -> reqwest::RequestBuilder {
    if let Some(key) = bearer {
        request.bearer_auth(key.as_str())
    } else {
        request
    }
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct SseEvent {
    kind: String,
    data: String,
}
#[derive(Default)]
pub(super) struct SseFrames {
    pending: Vec<u8>,
    kind: String,
    data: String,
}
impl SseFrames {
    /// Handles split UTF-8, split lines, comments, CRLF, multiline SSE data.
    /// Each line and event is hard-bounded to prevent untrusted growth.
    pub(super) fn feed(&mut self, input: &[u8]) -> Result<Vec<SseEvent>> {
        if self.pending.len().saturating_add(input.len()) > MAX_FRAME {
            return Err(Error::Limit("legacy MCP SSE line exceeds 128 KiB"));
        }
        self.pending.extend_from_slice(input);
        let mut events = Vec::new();
        while let Some(index) = self.pending.iter().position(|b| *b == b'\n') {
            let mut remainder = self.pending.split_off(index + 1);
            std::mem::swap(&mut remainder, &mut self.pending);
            let mut line = &remainder[..index];
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
            if line.is_empty() {
                if !self.kind.is_empty() || !self.data.is_empty() {
                    events.push(SseEvent {
                        kind: std::mem::take(&mut self.kind),
                        data: std::mem::take(&mut self.data),
                    });
                }
                continue;
            }
            if line[0] == b':' {
                continue;
            }
            let text = std::str::from_utf8(line)
                .map_err(|_| Error::Protocol("legacy MCP SSE event is not UTF-8"))?;
            let (key, value) = text.split_once(':').unwrap_or((text, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match key {
                "event" => {
                    if value.len() > 64 {
                        return Err(Error::Limit("legacy MCP SSE event name too long"));
                    }
                    self.kind = value.into();
                }
                "data" => {
                    if self.data.len().saturating_add(value.len() + 1) > MAX_FRAME {
                        return Err(Error::Limit("legacy MCP SSE event body too large"));
                    }
                    if !self.data.is_empty() {
                        self.data.push('\n');
                    }
                    self.data.push_str(value);
                }
                "id" | "retry" => { /* No SSE reconnect/resume */ }
                _ => { /* Ignore forward-compatible SSE fields */ }
            }
        }
        Ok(events)
    }
}
/// An untrusted endpoint event must never retarget API credentials or a tool
/// POST to a different scheme, host or effective port. Same-origin opaque
/// session-id query strings are allowed solely on the announced POST URL.
pub(super) fn endpoint_url(start: &Url, announced: &str) -> Result<Url> {
    if announced.is_empty()
        || announced.len() > MAX_ENDPOINT_BYTES
        || announced.chars().any(char::is_control)
        || announced.starts_with("//")
    {
        return Err(Error::Denied("legacy MCP endpoint URI invalid"));
    }
    let to = start
        .join(announced)
        .map_err(|_| Error::Denied("legacy MCP endpoint URI cannot be resolved"))?;
    if to.scheme() != start.scheme()
        || to.host_str() != start.host_str()
        || to.port_or_known_default() != start.port_or_known_default()
        || !to.has_host()
        || to.username() != ""
        || to.password().is_some()
        || to.fragment().is_some()
        || to
            .query()
            .is_some_and(|q| q.len() > 1024 || q.chars().any(char::is_control))
    {
        return Err(Error::Denied(
            "legacy MCP advertised POST endpoint crosses trusted origin",
        ));
    }
    if to.path().is_empty() || to.path().len() > 2048 {
        return Err(Error::Denied("legacy MCP POST path invalid/oversized"));
    }
    Ok(to)
}
fn message_bytes(event: SseEvent) -> Result<Vec<u8>> {
    if event.kind != "message" || event.data.is_empty() || event.data.len() > MAX_FRAME {
        return Err(Error::Protocol("invalid legacy MCP SSE message event"));
    }
    let value: serde_json::Value = serde_json::from_str(&event.data)
        .map_err(|_| Error::Protocol("legacy MCP response is not JSON-RPC JSON"))?;
    if !value.is_object()
        || value["jsonrpc"] != "2.0"
        || !(value.get("id").is_some() || value.get("method").is_some())
    {
        return Err(Error::Protocol("legacy MCP event is not JSON-RPC 2.0"));
    }
    let mut bytes = event.data.into_bytes();
    bytes.push(b'\n');
    Ok(bytes)
}
/// Returns a standard RMCP async read/write client transport and owned tasks.
pub(super) async fn open(
    client: Client,
    uri: &str,
    bearer: Option<Arc<Zeroizing<String>>>,
) -> Result<(
    rmcp::transport::async_rw::AsyncRwTransport<rmcp::RoleClient, DuplexStream, DuplexStream>,
    Pumps,
)> {
    let start = Url::parse(uri).map_err(|_| Error::Denied("invalid legacy MCP SSE URL"))?;
    let response = request_auth(
        client
            .get(start.clone())
            .header(ACCEPT, "text/event-stream"),
        bearer.as_ref(),
    )
    .send()
    .await
    .map_err(|_| Error::Transport("legacy MCP SSE connect failed"))?;
    if !response.status().is_success() {
        return Err(Error::http(response.status().as_u16()));
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if !content_type
        .split(';')
        .next()
        .is_some_and(|s| s.trim().eq_ignore_ascii_case("text/event-stream"))
    {
        return Err(Error::Protocol(
            "legacy MCP GET did not return an SSE stream",
        ));
    }
    let mut stream = Box::pin(response.bytes_stream());
    let mut frames = SseFrames::default();
    let mut post_uri = None;
    let mut queued = Vec::new();
    // Absolute budget: a malicious server cannot keep resetting a relative
    // handshake timeout by sending endless SSE heartbeat comments.
    let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
    while post_uri.is_none() {
        let chunk = tokio::time::timeout_at(deadline, stream.next())
            .await
            .map_err(|_| Error::Timeout)?
            .ok_or(Error::Protocol("legacy MCP SSE closed without endpoint"))?
            .map_err(|_| Error::Transport("legacy MCP SSE announcement failed"))?;
        for event in frames.feed(&chunk)? {
            if post_uri.is_none() {
                if event.kind != "endpoint" {
                    return Err(Error::Protocol(
                        "legacy MCP SSE first event is not endpoint",
                    ));
                }
                post_uri = Some(endpoint_url(&start, &event.data)?);
            } else {
                queued.push(message_bytes(event)?);
            }
        }
    }
    let post_uri = post_uri.ok_or(Error::Protocol("legacy MCP endpoint missing"))?;
    let (sdk_reader, mut sse_writer) = tokio::io::duplex(MAX_FRAME);
    let (sdk_writer, post_reader) = tokio::io::duplex(MAX_FRAME);
    let stop = CancellationToken::new();
    let reader_stop = stop.clone();
    let inbound = tokio::spawn(async move {
        let _: Result<()> = async {
            for event in queued {
                sse_writer.write_all(&event).await?;
            }
            while let Some(chunk) = tokio::select! {
                _ = reader_stop.cancelled() => None,
                incoming = stream.next() => incoming
            } {
                let chunk =
                    chunk.map_err(|_| Error::Transport("legacy MCP SSE connection lost"))?;
                for event in frames.feed(&chunk)? {
                    let data = message_bytes(event)?;
                    sse_writer.write_all(&data).await?;
                }
            }
            Err(Error::Transport(
                "legacy MCP SSE ended; no automatic replay",
            ))
        }
        .await;
        reader_stop.cancel();
    });
    let writer_stop = stop.clone();
    let outbound = tokio::spawn(async move {
        let _: Result<()> = async {
            let mut lines = FramedRead::new(post_reader, LinesCodec::new_with_max_length(MAX_POST));
            loop {
                let line = tokio::select! {
                    _ = writer_stop.cancelled() => break,
                    item = lines.next() => match item {
                        Some(Ok(line)) => line,
                        Some(Err(_)) => return Err(Error::Limit("MCP legacy outbound JSON-RPC too large")),
                        None => break,
                    }
                };
                let message: serde_json::Value = serde_json::from_str(&line)
                    .map_err(|_| Error::Protocol("invalid outbound MCP JSON-RPC"))?;
                if message["jsonrpc"] != "2.0"
                    || !(message.get("id").is_some() || message.get("method").is_some())
                {
                    return Err(Error::Protocol("invalid outbound MCP request envelope"));
                }
                let mut request = client.post(post_uri.clone())
                    .header(ACCEPT, "application/json")
                    .header(CONTENT_TYPE, "application/json")
                    .body(line);
                request = request_auth(request, bearer.as_ref());
                let response = tokio::select! {
                    _ = writer_stop.cancelled() => break,
                    sent = tokio::time::timeout(POST_TIMEOUT, request.send()) =>
                        sent.map_err(|_| Error::Timeout)?
                            .map_err(|_| Error::Transport("legacy MCP POST transport failed"))?,
                };
                // The SSE stream owns JSON-RPC results. POST is an acknowledgement.
                if !matches!(response.status().as_u16(), 200 | 202 | 204) {
                    return Err(Error::http(response.status().as_u16()));
                }
            }
            Ok(())
        }.await;
        writer_stop.cancel();
    });
    let transport = rmcp::transport::async_rw::AsyncRwTransport::new_client(sdk_reader, sdk_writer);
    Ok((
        transport,
        Pumps {
            stop,
            inbound,
            outbound,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parser_handles_split_lines_crlf_multiline_and_bounds() {
        let mut frames = SseFrames::default();
        assert!(frames
            .feed(b": alive\r\nevent: endpoint\r\n")
            .unwrap()
            .is_empty());
        let endpoint = frames
            .feed(b"data: /messages?sessionId=s1\r\n\r\n")
            .unwrap();
        assert_eq!(endpoint[0].data, "/messages?sessionId=s1");
        let mut frames = SseFrames::default();
        assert!(frames
            .feed(b"event: message\ndata: {\"jsonrpc\":\"2.0\",\n")
            .unwrap()
            .is_empty());
        let result = frames.feed(b"data: \"id\":1,\"result\":{}}\n\n").unwrap();
        assert!(message_bytes(result.into_iter().next().unwrap()).is_ok());
        assert!(SseFrames::default()
            .feed(&vec![b'a'; MAX_FRAME + 1])
            .is_err());
    }
    #[test]
    fn announced_post_endpoint_is_same_origin_only() {
        let base = Url::parse("https://mcp.example.test/sse").unwrap();
        assert_eq!(
            endpoint_url(&base, "/messages?sessionId=opaque")
                .unwrap()
                .as_str(),
            "https://mcp.example.test/messages?sessionId=opaque"
        );
        for bad in [
            "https://evil.example/messages",
            "http://mcp.example.test/messages",
            "https://mcp.example.test:444/messages",
            "//evil.example/messages",
            "https://person:pass@mcp.example.test/messages",
            "/messages#fragment",
            "/messages\nInjected",
            "",
        ] {
            assert!(endpoint_url(&base, bad).is_err(), "{bad}");
        }
        let loopback = Url::parse("http://127.0.0.1:38242/sse").unwrap();
        assert!(endpoint_url(&loopback, "/messages?sessionId=x").is_ok());
        assert!(endpoint_url(&loopback, "http://127.0.0.1:6552/messages").is_err());
    }
    #[test]
    fn invalid_jsonrpc_or_wrong_events_are_denied() {
        assert!(message_bytes(SseEvent {
            kind: "message".into(),
            data: "{}".into()
        })
        .is_err());
        assert!(message_bytes(SseEvent {
            kind: "event".into(),
            data: "{}".into()
        })
        .is_err());
        assert!(message_bytes(SseEvent {
            kind: "message".into(),
            data: r#"{"jsonrpc":"2.0","id":1,"result":{}}"#.into()
        })
        .is_ok());
    }
}
