//! Bounded binary tools; media rendering remains a separate AW provider port.
use crate::{platform_fs, tool_failure};
use aw_agent_core::*;
use aw_platform::scoped_fs::Root;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
const READ_WINDOW: usize = 24 * 1024;
const MAX_DATA: usize = 48 * 1024;
const MAX_EDIT_RANGES: usize = 32;
const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
#[derive(Clone, Copy, Debug, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum BreadFormat {
    #[default]
    Mcp,
    Base64,
    Hex,
}

/// Resource IDs encode each component independently: punctuation cannot
/// become a fragment, query or unintended path separator.
fn resource_uri(path: &str) -> Result<String> {
    let mut uri = url::Url::parse("abird://file/")
        .map_err(|_| Error::Protocol("invalid binary resource URI base"))?;
    uri.path_segments_mut()
        .map_err(|_| Error::Protocol("invalid binary resource URI path"))?
        .pop_if_empty()
        .extend(path.split('/'));
    Ok(uri.into())
}

#[derive(Clone, Copy, Debug, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum Encoding {
    #[default]
    Base64,
    Hex,
}
impl Encoding {
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
    format: BreadFormat,
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
    #[serde(default)]
    create_only: bool,
    expected_sha256: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BeditArgs {
    path: String,
    #[serde(deserialize_with = "bounded_ranges")]
    edits: Vec<Range>,
    sha256: Option<String>,
}
fn bounded_ranges<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> std::result::Result<Vec<Range>, D::Error> {
    aw_tools_core::mutation::bounded_array::<D, Range, MAX_EDIT_RANGES>(de, "bedit", "ranges")
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

pub(crate) struct Binary {
    root: Arc<Root>,
    op: Operation,
}
impl Binary {
    pub(crate) fn new(root: Arc<Root>, op: Operation) -> Self {
        Self { root, op }
    }
}
#[cfg(test)]
fn plan_edits(original: &[u8], ranges: Vec<Range>) -> Result<(Vec<u8>, usize)> {
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
    let mut written = 0usize;
    for (offset, length, _, new_bytes) in sorted {
        written = written.saturating_add(new_bytes.len());
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
    Ok((out, written))
}
struct PreparedRange {
    offset: usize,
    end: usize,
    data: Vec<u8>,
}
/// Decode only requested bytes, not the original file or its replacement.
fn prepare_ranges(
    ranges: Vec<Range>,
    source_size: usize,
) -> Result<(Vec<PreparedRange>, usize, usize)> {
    if ranges.is_empty() || ranges.len() > MAX_EDIT_RANGES {
        return Err(Error::Limit("bedit requires 1..=32 ranges"));
    }
    let mut requested = Vec::with_capacity(ranges.len());
    for (order, range) in ranges.into_iter().enumerate() {
        let data = range.encoding.decode(&range.data)?;
        let length = range.length.unwrap_or(data.len());
        let end = range
            .offset
            .checked_add(length)
            .ok_or(Error::Limit("binary range overflow"))?;
        if end > source_size {
            return Err(Error::Denied("binary range is past EOF"));
        }
        requested.push((range.offset, length, order, data));
    }
    requested.sort_by_key(|(offset, length, order, _)| (*offset, *length, *order));
    let (mut cursor, mut output_len, mut written) = (0usize, 0usize, 0usize);
    let mut prepared = Vec::with_capacity(requested.len());
    for (offset, length, _, data) in requested {
        if offset < cursor {
            return Err(Error::Denied("binary edit ranges overlap"));
        }
        output_len = output_len
            .checked_add(offset - cursor)
            .and_then(|n| n.checked_add(data.len()))
            .ok_or(Error::Limit("binary output size overflow"))?;
        written = written
            .checked_add(data.len())
            .ok_or(Error::Limit("binary payload overflow"))?;
        cursor = offset + length;
        prepared.push(PreparedRange {
            offset,
            end: cursor,
            data,
        });
    }
    output_len = output_len
        .checked_add(source_size - cursor)
        .ok_or(Error::Limit("binary output size overflow"))?;
    if output_len > MAX_FILE_BYTES {
        return Err(Error::Limit("binary edit exceeds 64 MiB"));
    }
    Ok((prepared, written, output_len))
}
impl Tool for Binary {
    fn spec(&self) -> ToolSpec {
        use aw_tools_core::filesystem::schema::{SchemaCaps, Tool as C};
        let tool = match self.op {
            Operation::Read => C::Bread,
            Operation::Write => C::Bwrite,
            Operation::Edit => C::Bedit,
        };
        crate::core_spec(
            tool,
            SchemaCaps {
                binary_data_bytes: MAX_DATA,
                binary_edit_ranges: MAX_EDIT_RANGES,
                parallel_reads: true,
                ..Default::default()
            },
        )
    }
    fn validate(&self, value: &Value) -> Result<()> {
        match self.op {
            Operation::Read => {
                let a: BreadArgs = parse(value)?;
                crate::model_path(&a.path)?;
                if a.limit == 0 || a.limit > 65536 {
                    return Err(Error::Limit("binary window too large"));
                }
                if matches!(a.format, BreadFormat::Mcp) && a.offset != 0 {
                    return Err(Error::Denied("format=mcp requires offset=0"));
                }
                if !matches!(a.format, BreadFormat::Mcp) && a.limit > READ_WINDOW {
                    return Err(Error::Limit("encoded binary window exceeds 24 KiB"));
                }
                let _ = a.parallel;
            }
            Operation::Write => {
                let a: BwriteArgs = parse(value)?;
                crate::model_path(&a.path)?;
                a.encoding.decode(&a.data)?;
                crate::mutate::validate_digest(a.expected_sha256.as_deref())?;
            }
            Operation::Edit => {
                let a: BeditArgs = parse(value)?;
                crate::model_path(&a.path)?;
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
            let answer = aw_tools_core::asynchronous::run_blocking(move || -> Result<ToolOutput> {
                if ctx.cancellation.is_cancelled() {
                    return Err(Error::Cancelled);
                }
                match op {
                    Operation::Read => {
                        let a: BreadArgs = parse(&value)?;
                        if matches!(a.format, BreadFormat::Mcp) {
                            let bytes = platform_fs(root.read_bytes(&a.path, a.limit))?;
                            let packet=aw_tools_core::media::MediaPacket::from_complete_bytes(bytes)
                                .map_err(|_|Error::Limit("inline media exceeds 64 KiB"))?;
                            let descriptor=packet.descriptor();
                            let mime=descriptor.mime;
                            let kind=descriptor.kind.as_str();
                            // Transport-neutral encoding. A future typed adapter
                            // can project this into real MCP ContentBlock media.
                            let output = json!({
                                "type":kind, "mime_type":mime, "format":"mcp",
                                "path":a.path,"data":BASE64.encode(packet.bytes()),
                                "uri":if kind=="resource"{Some(resource_uri(&a.path)?)}else{None},
                                "bytes":packet.bytes().len(),
                                "sha256":descriptor.sha256_hex(),
                                "mime_source":if descriptor.signature_identified{"signature"}else{"fallback"},
                                "transport":"encoded_json"
                            });
                            Ok(ToolOutput::text(output.to_string()))
                        } else {
                            let (data, total, truncated) =
                                platform_fs(root.read_binary_window(&a.path, a.offset, a.limit))?;
                            let (format, encoded) = match a.format {
                                BreadFormat::Base64 => ("base64", BASE64.encode(&data)),
                                BreadFormat::Hex => (
                                    "hex",
                                    data.iter().map(|b| format!("{b:02x}")).collect::<String>(),
                                ),
                                BreadFormat::Mcp => unreachable!(),
                            };
                            Ok(ToolOutput {
                                content: json!({"path":a.path,"data":encoded,"format":format,
                                    "encoding":format,"offset":a.offset,"bytes":data.len(),
                                    "total_size":total,"total_bytes":total,"truncated":truncated})
                                .to_string(),
                                is_error: false,
                                truncated,
                            })
                        }
                    }
                    Operation::Write => {
                        let a: BwriteArgs = parse(&value)?;
                        let bytes = a.encoding.decode(&a.data)?;
                        if !a.create_only && a.expected_sha256.is_none(){
                            let result=aw_tools_core::filesystem::operations::write_bytes(
                                root.as_ref(),&a.path,&bytes,MAX_FILE_BYTES,
                                &||ctx.cancellation.is_cancelled()
                            ).map_err(crate::file_operation_error)?;
                            Ok(ToolOutput::text(result.as_str()))
                        }else{
                            let changed=crate::mutate::write(
                                &root,&a.path,&bytes,a.create_only,
                                a.expected_sha256.as_deref(),
                                &||ctx.cancellation.is_cancelled()
                            )?;
                            Ok(ToolOutput::text(
                                aw_tools_core::mutation::compact_success(
                                    usize::from(changed),None
                                )
                            ))
                        }
                    }
                    Operation::Edit => {
                        let a: BeditArgs = parse(&value)?;
                        let _guard = platform_fs(
                            root.lock_mutations_with_cancel(std::slice::from_ref(&a.path), &|| {
                                ctx.cancellation.is_cancelled()
                            }),
                        )?;
                        {
                            use aw_tools_core::binary::{
                                digest_limited,splices_are_unchanged,stream_splice_with_stop,
                                verify_preimage,ByteSplice,
                            };
                            use std::io::{Seek, SeekFrom};
                            let mut source = platform_fs(root.open_regular(&a.path))?;
                            let (size, digest) = digest_limited(&mut source, MAX_FILE_BYTES)
                                .map_err(|_| {
                                    Error::Limit("binary source exceeds 64 MiB or changed")
                                })?;
                            let (prepared,_bytes_written,size_after)=
                                prepare_ranges(a.edits,size)?;
                            if let Some(sha) = a.sha256 {
                                let actual = digest
                                    .iter()
                                    .map(|b| format!("{b:02x}"))
                                    .collect::<String>();
                                if actual != sha.to_ascii_lowercase() {
                                    return Ok(ToolOutput::error(
                                        "bedit: sha256 precondition mismatch",
                                    ));
                                }
                            }
                            let splices = prepared
                                .iter()
                                .map(|r| ByteSplice {
                                    offset: r.offset,
                                    end: r.end,
                                    data: &r.data,
                                })
                                .collect::<Vec<_>>();
                            let changed = !(size == size_after
                                && splices_are_unchanged(&mut source, &splices).map_err(|_| {
                                    Error::Protocol("binary source changed during no-op check")
                                })?);
                            if !changed {
                                if ctx.cancellation.is_cancelled(){return Err(Error::Cancelled);}
                                let mut current=platform_fs(root.open_regular(&a.path))?;
                                verify_preimage(&mut current,(size,digest),MAX_FILE_BYTES)
                                    .map_err(|_|Error::Protocol(
                                        "binary target changed during no-op; inspect workspace"
                                    ))?;
                            }
                            if changed {
                                if ctx.cancellation.is_cancelled() {
                                    return Err(Error::Cancelled);
                                }
                                let pending = platform_fs(root.stage_stream_update(
                                    &a.path,
                                    (size as u64, digest),
                                    |output| {
                                        source.seek(SeekFrom::Start(0))?;
                                        stream_splice_with_stop(
                                            &mut source,
                                            output,
                                            (size, digest),
                                            &splices,
                                            &||ctx.cancellation.is_cancelled(),
                                        )
                                        .map_err(|_| {
                                            aw_platform::scoped_fs::ScopedError::Denied(
                                                "binary source changed during staged edit",
                                            )
                                        })?;
                                        Ok(())
                                    },
                                    &|| ctx.cancellation.is_cancelled(),
                                ))?;
                                if ctx.cancellation.is_cancelled() {
                                    return Err(Error::Cancelled);
                                }
                                platform_fs(pending.commit())?;
                            }
                            Ok(ToolOutput::text(
                                aw_tools_core::mutation::compact_success(
                                    usize::from(changed),None)
                            ))
                        }
                    }
                }
            })
            .await
            .map_err(|_| {
                Error::Protocol("binary worker failed; inspect workspace before retrying")
            })?;
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
        assert_eq!(plan_edits(b"abcd", ranges).unwrap().0, b"aZcYd");
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
