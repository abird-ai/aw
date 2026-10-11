//! Independent Rust stdio RPC client for aw. This crate does NOT depend on
//! aw-agent-core, its tools, Session implementation, or private provider data.
//! Losing a pipe after sending a request is ALWAYS classified as uncertain.
//! Inspect durable state on an explicit fresh connection; never auto-resend.
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fmt,
    io::{BufRead, Write},
};
pub const VERSION: u8 = 1;
pub const MAX_FRAME: usize = 640 * 1024;
pub const MAX_REQUEST: usize = 192 * 1024;
#[derive(Clone, Debug, Deserialize)]
pub struct Frame {
    pub v: u8,
    pub seq: u64,
    #[serde(rename = "type")]
    pub kind: String,
    pub id: Option<String>,
    pub data: Value,
}
#[derive(Debug)]
pub enum Error {
    TransportUncertain,
    Protocol(&'static str),
    Rejected(String),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TransportUncertain=>write!(f,"RPC transport ended; prior effect status is UNKNOWN. Inspect state; never auto-retry."),
            Self::Protocol(message)=>write!(f,"RPC protocol error: {message}"),
            Self::Rejected(code)=>write!(f,"RPC request rejected/settled with status: {code}"),
        }
    }
}
impl std::error::Error for Error {}
pub struct Client<R: BufRead, W: Write> {
    reader: R,
    writer: W,
    sequence: u64,
    request_sequence: u64,
    used: BTreeSet<String>,
    pub hello: Frame,
}
pub struct ResultFrame {
    pub accepted: bool,
    pub terminal: Frame,
}
fn read_frame<R: BufRead>(reader: &mut R, previous: &mut u64) -> Result<Frame, Error> {
    let mut encoded = Vec::<u8>::new();
    let mut over = false;
    loop {
        let chunk = reader.fill_buf().map_err(|_| Error::TransportUncertain)?;
        if chunk.is_empty() {
            return Err(Error::TransportUncertain);
        }
        let count = chunk
            .iter()
            .position(|b| *b == b'\n')
            .map_or(chunk.len(), |i| i + 1);
        let room = MAX_FRAME.saturating_add(1).saturating_sub(encoded.len());
        encoded.extend_from_slice(&chunk[..count.min(room)]);
        if room < count {
            over = true;
        }
        let newline = chunk.get(count.saturating_sub(1)) == Some(&b'\n');
        reader.consume(count);
        if newline {
            break;
        }
    }
    if over || encoded.len() > MAX_FRAME {
        return Err(Error::Protocol("oversized response frame"));
    }
    let frame: Frame =
        serde_json::from_slice(&encoded).map_err(|_| Error::Protocol("invalid JSONL response"))?;
    if frame.v != VERSION || frame.seq <= *previous || frame.kind.len() > 64 {
        return Err(Error::Protocol(
            "wrong version or non-monotonic response sequence",
        ));
    }
    *previous = frame.seq;
    Ok(frame)
}
impl<R: BufRead, W: Write> Client<R, W> {
    pub fn connect(mut reader: R, writer: W) -> Result<Self, Error> {
        let mut sequence = 0;
        let hello = read_frame(&mut reader, &mut sequence)?;
        if hello.kind != "hello"
            || hello.data["protocol"] != "aw.rpc"
            || hello.data["transport"] != "stdio"
        {
            return Err(Error::Protocol("not the versioned aw.rpc stdio handshake"));
        }
        Ok(Self {
            reader,
            writer,
            sequence,
            request_sequence: 0,
            used: BTreeSet::new(),
            hello,
        })
    }
    pub fn call(
        &mut self,
        method: &str,
        params: Value,
        mut event: impl FnMut(&Frame),
    ) -> Result<ResultFrame, Error> {
        if method.is_empty()
            || method.len() > 64
            || !method.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
        {
            return Err(Error::Protocol("invalid method identifier"));
        }
        if !params.is_object() {
            return Err(Error::Protocol("parameters must be typed JSON object"));
        }
        if self.used.len() >= 1024 {
            return Err(Error::Protocol("request ID budget exhausted"));
        }
        self.request_sequence += 1;
        let id = format!("client_{}", self.request_sequence);
        if !self.used.insert(id.clone()) {
            return Err(Error::Protocol("duplicate client request ID"));
        }
        let command = json!({"v":VERSION,"id":id,"method":method,"params":params});
        let bytes = serde_json::to_vec(&command)
            .map_err(|_| Error::Protocol("cannot serialize RPC request"))?;
        if bytes.len() > MAX_REQUEST {
            return Err(Error::Protocol("outbound request too large"));
        }
        // The transport might close after receiving these bytes. NEVER
        // claim the provider or tool did not execute on a failed write/flush.
        self.writer
            .write_all(&bytes)
            .map_err(|_| Error::TransportUncertain)?;
        self.writer
            .write_all(b"\n")
            .map_err(|_| Error::TransportUncertain)?;
        self.writer.flush().map_err(|_| Error::TransportUncertain)?;
        let mut accepted = false;
        loop {
            let frame = read_frame(&mut self.reader, &mut self.sequence)?;
            if frame.id.as_deref() != Some(&id) {
                return Err(Error::Protocol(
                    "unexpected concurrent or mismatched response ID",
                ));
            }
            match frame.kind.as_str() {
                "accepted" => {
                    accepted = true;
                    event(&frame);
                }
                "response" | "settled" | "error" => {
                    if frame.kind == "settled" && !accepted {
                        return Err(Error::Protocol("settled effect without accepted receipt"));
                    }
                    if frame.kind == "error" {
                        return Err(Error::Rejected(
                            frame.data["code"].as_str().unwrap_or("unspecified").into(),
                        ));
                    }
                    return Ok(ResultFrame {
                        accepted,
                        terminal: frame,
                    });
                }
                "started" | "delta" | "tool_started" | "tool_finished" | "attempt_finished"
                | "finished" | "rejected" => event(&frame),
                _ => return Err(Error::Protocol("unknown RPC event kind")),
            }
        }
    }
    pub fn prompt(
        &mut self,
        text: &str,
        on_event: impl FnMut(&Frame),
    ) -> Result<ResultFrame, Error> {
        self.call("prompt", json!({"text":text}), on_event)
    }
    pub fn state(&mut self) -> Result<Value, Error> {
        Ok(self.call("get_state", json!({}), |_| {})?.terminal.data)
    }
    pub fn messages(&mut self, limit: u8) -> Result<Value, Error> {
        Ok(self
            .call("get_messages", json!({"limit":limit}), |_| {})?
            .terminal
            .data)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_versioned_stream_without_agent_dependency() {
        let input=[
            "{\"v\":1,\"seq\":1,\"type\":\"hello\",\"id\":null,\"data\":{\"protocol\":\"aw.rpc\",\"transport\":\"stdio\"}}",
            "{\"v\":1,\"seq\":2,\"type\":\"response\",\"id\":\"client_1\",\"data\":{\"next_turn\":1}}"
        ].join("\n")+"\n";
        let mut client = Client::connect(std::io::Cursor::new(input), Vec::<u8>::new()).unwrap();
        assert_eq!(client.state().unwrap()["next_turn"], 1);
        assert!(String::from_utf8(client.writer)
            .unwrap()
            .contains("client_1"));
    }
    #[test]
    fn lost_pipe_is_never_classified_as_not_dispatched() {
        let input="{\"v\":1,\"seq\":1,\"type\":\"hello\",\"id\":null,\"data\":{\"protocol\":\"aw.rpc\",\"transport\":\"stdio\"}}\n";
        let mut client = Client::connect(std::io::Cursor::new(input), Vec::<u8>::new()).unwrap();
        assert!(matches!(
            client.prompt("effectful", |_| {}),
            Err(Error::TransportUncertain)
        ));
    }
}
