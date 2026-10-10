//! Independent Rust process extension fixture / minimal SDK consumer.
//! No linking to Agent internals, credentials, SQLite or aw's runtime.
#![forbid(unsafe_code)]
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

const MAX_FRAME: usize = 256 * 1024;

/// Read one complete line without an unbounded allocation. Do not use
/// BufRead::lines here: that grows an owned String before a length check.
fn bounded_line(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "unterminated extension request",
                ))
            };
        }
        let size = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        if line.len().saturating_add(size) > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "extension request exceeds 256 KiB",
            ));
        }
        line.extend_from_slice(&available[..size]);
        reader.consume(size);
        if line.last() == Some(&b'\n') {
            return Ok(Some(line));
        }
    }
}

fn respond(input: &Value) -> Value {
    let id = input.get("id").and_then(Value::as_u64).unwrap_or(0);
    let reply = if input.get("v").and_then(Value::as_u64) != Some(2) {
        None
    } else {
        let params = input.get("params").cloned().unwrap_or_else(|| json!({}));
        match input.get("method").and_then(Value::as_str) {
            Some("describe") => Some(json!({
                "protocol":2,"name":"rust-hello","revision":"v1",
                "tools":[{"name":"echo","description":"Echo bounded user-selected text (external effect not assumed read-only)",
                    "input_schema":{"type":"object","properties":{"message":{"type":"string","maxLength":80}},
                        "required":["message"],"additionalProperties":false}}],
                "hooks":["prompt","tool.call","tool.result"],
                "prompts":[{"name":"hello","description":"Generate inert prompt text",
                    "arguments":["name"]}]
            })),
            Some("tool.execute") => {
                let message = params["arguments"]["message"].as_str().unwrap_or("");
                Some(json!({"content":format!("extension-ok: {message}"),"is_error":false}))
            }
            Some("prompt") => Some(json!({"allow": !params["prompt"]
                .as_str().unwrap_or("").contains("DENY_BEFORE_PROMPT")})),
            Some("tool.call") => Some(json!({"allow":params["arguments"]["message"]
                .as_str() != Some("DENY_BEFORE_TOOL")})),
            Some("tool.result") => Some(json!({"allow":true})),
            Some("prompt.render") => Some(json!({
                "text":format!("Hello, {}! (untrusted extension data)",
                    params["arguments"]["name"].as_str().unwrap_or(""))
            })),
            _ => None,
        }
    };
    match reply {
        Some(result) => json!({"v":2,"id":id,"ok":true,"result":result}),
        None => json!({"v":2,"id":id,"ok":false,"result":null}),
    }
}

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    let mut input = stdin.lock();
    while let Some(line) = bounded_line(&mut input)? {
        let request: Value = match serde_json::from_slice(&line) {
            Ok(value) => value,
            Err(_) => break,
        };
        let response = respond(&request);
        serde_json::to_writer(&mut stdout, &response)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn bounded_framing_preserves_next_request_without_read_ahead() {
        let mut input = Cursor::new(b"{\"v\":1}\n{\"v\":1}\n".to_vec());
        assert_eq!(bounded_line(&mut input).unwrap().unwrap(), b"{\"v\":1}\n");
        assert_eq!(bounded_line(&mut input).unwrap().unwrap(), b"{\"v\":1}\n");
        assert!(bounded_line(&mut input).unwrap().is_none());
    }

    #[test]
    fn oversized_unterminated_lines_fail_closed() {
        let mut big = vec![b'x'; MAX_FRAME + 1];
        big.push(b'\n');
        let mut input = Cursor::new(big);
        assert_eq!(
            bounded_line(&mut input).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let mut input = Cursor::new(b"{\"v\":1}".to_vec());
        assert_eq!(
            bounded_line(&mut input).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}
