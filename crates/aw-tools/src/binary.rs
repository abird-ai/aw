//! Bounded binary tools; media rendering remains a separate AW provider port.
use crate::{platform_fs, tool_failure};
use aw_agent_core::*;
use aw_platform::scoped_fs::Root;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
const READ_WINDOW: usize = 24 * 1024;
const MAX_DATA: usize = 48 * 1024;
const MAX_EDIT_RANGES: usize = 32;
const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
#[derive(Clone, Copy, Debug, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum Encoding {
    #[default]
    Base64,
    Hex,
}
impl Encoding {
    fn encode(self, bytes: &[u8]) -> String {
        match self {
            Self::Base64 => BASE64.encode(bytes),
            Self::Hex => bytes.iter().map(|b| format!("{b:02x}")).collect(),
        }
    }
    fn decode(self, data: &str) -> Result<Vec<u8>> {
        if data.len() > MAX_DATA {
            return Err(Error::Limit("encoded binary payload exceeds 48 KiB"));
        }
        let bytes = match self {
            Self::Base64 => BASE64
                .decode(data)
                .map_err(|_| Error::Denied("invalid base64"))?,
            Self::Hex => {
                if !data.len().is_multiple_of(2) {
                    return Err(Error::Denied("invalid hex"));
                }
                data.as_bytes()
                    .chunks_exact(2)
                    .map(|pair| {
                        let text =
                            std::str::from_utf8(pair).map_err(|_| Error::Denied("invalid hex"))?;
                        u8::from_str_radix(text, 16).map_err(|_| Error::Denied("invalid hex"))
                    })
                    .collect::<Result<Vec<_>>>()?
            }
        };
        Ok(bytes)
    }
}
fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|_| Error::Denied("invalid, missing or unknown binary arguments"))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BreadArgs {
    path: String,
    #[serde(default)]
    offset: u64,
    #[serde(default = "read_window")]
    limit: usize,
    #[serde(default)]
    format: Encoding,
    #[serde(default)]
    parallel: bool,
}
fn read_window() -> usize {
    READ_WINDOW
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BwriteArgs {
    path: String,
    data: String,
    #[serde(default)]
    encoding: Encoding,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BeditArgs {
    path: String,
    edits: Vec<Range>,
    sha256: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Range {
    offset: usize,
    length: Option<usize>,
    #[serde(default)]
    data: String,
    #[serde(default)]
    encoding: Encoding,
}
#[derive(Clone, Copy)]
pub(crate) enum Operation {
    Read,
    Write,
    Edit,
}
impl Operation {
    fn name(self) -> &'static str {
        match self {
            Self::Read => "bread",
            Self::Write => "bwrite",
            Self::Edit => "bedit",
        }
    }
}
pub(crate) struct Binary {
    root: Arc<Root>,
    op: Operation,
}
impl Binary {
    pub(crate) fn new(root: Arc<Root>, op: Operation) -> Self {
        Self { root, op }
    }
}
fn plan_edits(original: &[u8], ranges: Vec<Range>) -> Result<Vec<u8>> {
    if ranges.is_empty() || ranges.len() > MAX_EDIT_RANGES {
        return Err(Error::Limit("bedit requires 1..=32 ranges"));
    }
    let mut sorted = Vec::with_capacity(ranges.len());
    for (i, range) in ranges.into_iter().enumerate() {
        let bytes = range.encoding.decode(&range.data)?;
        let length = range.length.unwrap_or(bytes.len());
        let end = range
            .offset
            .checked_add(length)
            .ok_or(Error::Limit("binary range overflows"))?;
        if end > original.len() {
            return Err(Error::Denied("binary range is past EOF"));
        }
        sorted.push((range.offset, length, i, bytes));
    }
    sorted.sort_by_key(|r| (r.0, r.1, r.2));
    let mut out = Vec::new();
    let mut cursor = 0;
    for (offset, length, _, new_bytes) in sorted {
        if offset < cursor {
            return Err(Error::Denied("binary edit ranges overlap"));
        }
        out.extend_from_slice(&original[cursor..offset]);
        out.extend_from_slice(&new_bytes);
        if out.len() > MAX_FILE_BYTES {
            return Err(Error::Limit("binary edit result exceeds 64 MiB"));
        }
        cursor = offset + length;
    }
    out.extend_from_slice(&original[cursor..]);
    if out.len() > MAX_FILE_BYTES {
        return Err(Error::Limit("binary edit result exceeds 64 MiB"));
    }
    Ok(out)
}
impl Tool for Binary {
    fn spec(&self) -> ToolSpec {
        let (description,parameters,effect)=match self.op {
            Operation::Read=>("Read a granted binary file as base64 or hex, with zero-based byte offset and bounded byte limit. Typed MCP media is not yet supported by AW's text-only ToolOutput. Set parallel=true for an independent read.",
                json!({"type":"object","properties":{"path":{"type":"string"},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":READ_WINDOW},"format":{"type":"string","enum":["base64","hex"]},"parallel":{"type":"boolean"}},"required":["path"],"additionalProperties":false}),ToolEffect::Read),
            Operation::Write=>("Create or atomically replace a binary file from base64/hex bytes; respects AW's bounded model argument budget.",
                json!({"type":"object","properties":{"path":{"type":"string"},"data":{"type":"string","maxLength":MAX_DATA},"encoding":{"type":"string","enum":["base64","hex"]}},"required":["path","data"],"additionalProperties":false}),ToolEffect::Write),
            Operation::Edit=>("Apply up to 32 bounded non-overlapping byte ranges, addressed against the original file, with an optional SHA-256 precondition. Atomic per file; no-op does not write.",
                json!({"type":"object","properties":{"path":{"type":"string"},"sha256":{"type":"string"},"edits":{"type":"array","minItems":1,"maxItems":MAX_EDIT_RANGES,"items":{"type":"object","properties":{"offset":{"type":"integer","minimum":0},"length":{"type":"integer","minimum":0},"data":{"type":"string","maxLength":MAX_DATA},"encoding":{"type":"string","enum":["base64","hex"]}},"required":["offset"],"additionalProperties":false}}},"required":["path","edits"],"additionalProperties":false}),ToolEffect::Write),
        };
        ToolSpec {
            name: self.op.name().into(),
            description: description.into(),
            parameters,
            effect,
        }
    }
    fn validate(&self, value: &Value) -> Result<()> {
        match self.op {
            Operation::Read => {
                let a: BreadArgs = parse(value)?;
                platform_fs(aw_platform::scoped_fs::validate_path(&a.path))?;
                if a.limit == 0 || a.limit > READ_WINDOW {
                    return Err(Error::Limit("binary window too large"));
                }
                let _ = a.parallel;
            }
            Operation::Write => {
                let a: BwriteArgs = parse(value)?;
                platform_fs(aw_platform::scoped_fs::validate_path(&a.path))?;
                a.encoding.decode(&a.data)?;
            }
            Operation::Edit => {
                let a: BeditArgs = parse(value)?;
                platform_fs(aw_platform::scoped_fs::validate_path(&a.path))?;
                if a.edits.is_empty() || a.edits.len() > MAX_EDIT_RANGES {
                    return Err(Error::Limit("too many binary edits"));
                }
                for r in &a.edits {
                    if r.data.len() > MAX_DATA {
                        return Err(Error::Limit("encoded binary data too large"));
                    }
                }
                if let Some(hash) = a.sha256 {
                    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                        return Err(Error::Denied("sha256 must be 64 hexadecimal characters"));
                    }
                }
            }
        }
        Ok(())
    }
    fn execute<'a>(&'a self, ctx: ToolContext, value: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(value)?;
            let value = value.clone();
            let root = self.root.clone();
            let op = self.op;
            let answer=tokio::task::spawn_blocking(move||->Result<ToolOutput>{
                if ctx.cancellation.is_cancelled(){return Err(Error::Cancelled);}
                match op {
                    Operation::Read=>{
                        let a:BreadArgs=parse(&value)?;
                        let (data,total,truncated)=platform_fs(root.read_binary_window(&a.path,a.offset,a.limit))?;
                        Ok(ToolOutput{
                            content:json!({"path":a.path,"data":a.format.encode(&data),"encoding":match a.format{Encoding::Base64=>"base64",Encoding::Hex=>"hex"},"offset":a.offset,"total_bytes":total,"truncated":truncated}).to_string(),
                            is_error:false,truncated,
                        })
                    }
                    Operation::Write=>{
                        let a:BwriteArgs=parse(&value)?;
                        let bytes=a.encoding.decode(&a.data)?;
                        let _guard=platform_fs(root.lock_mutations(std::slice::from_ref(&a.path)))?;
                        let staged=platform_fs(root.stage_replace(&a.path,&bytes,&||ctx.cancellation.is_cancelled()))?;
                        if ctx.cancellation.is_cancelled(){return Err(Error::Cancelled);}
                        platform_fs(staged.commit())?;
                        Ok(ToolOutput::text(json!({"path":a.path,"bytes_written":bytes.len(),"changed":true}).to_string()))
                    }
                    Operation::Edit=>{
                        let a:BeditArgs=parse(&value)?;
                        let _guard=platform_fs(root.lock_mutations(std::slice::from_ref(&a.path)))?;
                        let original=platform_fs(root.read_bytes(&a.path,MAX_FILE_BYTES))?;
                        if let Some(sha)=a.sha256 {
                            if format!("{:x}",Sha256::digest(&original))!=sha.to_ascii_lowercase(){
                                return Ok(ToolOutput::error("bedit: sha256 precondition mismatch"));
                            }
                        }
                        let count=a.edits.len();
                        let proposed=plan_edits(&original,a.edits)?;
                        let changed=original!=proposed;
                        if changed{
                            let staged=platform_fs(root.stage_update(&a.path,&original,&proposed,&||ctx.cancellation.is_cancelled()))?;
                            if ctx.cancellation.is_cancelled(){return Err(Error::Cancelled);}
                            platform_fs(staged.commit())?;
                        }
                        Ok(ToolOutput::text(json!({"path":a.path,"ranges":count,"changed":changed,"size_before":original.len(),"size_after":proposed.len(),"bytes_written":if changed {proposed.len()} else {0}}).to_string()))
                    }
                }
            }).await.map_err(|_|Error::Protocol("binary worker failed; inspect workspace before retrying"))?;
            tool_failure(answer, !matches!(op, Operation::Read))
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_ranges_are_original_relative_and_check_overlap() {
        let ranges = vec![
            Range {
                offset: 1,
                length: Some(1),
                data: "Wg==".into(),
                encoding: Encoding::Base64,
            },
            Range {
                offset: 3,
                length: Some(0),
                data: "WQ==".into(),
                encoding: Encoding::Base64,
            },
        ];
        assert_eq!(plan_edits(b"abcd", ranges).unwrap(), b"aZcYd");
        let ranges = vec![
            Range {
                offset: 1,
                length: Some(2),
                data: String::new(),
                encoding: Encoding::Base64,
            },
            Range {
                offset: 2,
                length: Some(1),
                data: String::new(),
                encoding: Encoding::Base64,
            },
        ];
        assert!(plan_edits(b"abcd", ranges).is_err());
    }
}
