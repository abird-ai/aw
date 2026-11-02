//! Bounded binary tools; media rendering remains a separate AW provider port.
use crate::{platform_fs, tool_failure};
use aw_agent_core::*;
use aw_platform::scoped_fs::Root;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
const DEFAULT_READ_WINDOW: usize = 24 * 1024;
const MAX_EDIT_RANGES: usize = aw_tools::binary::MAX_EDIT_RANGES;
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
    fn decode_with_limit(self, data: &str, max_data: usize) -> Result<Vec<u8>> {
        if data.len() > max_data {
            return Err(Error::Limit(
                "encoded binary payload exceeds configured tool ceiling",
            ));
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

fn encoded_window(
    path: &str,
    data: &[u8],
    format: BreadFormat,
    offset: u64,
    total: u64,
) -> Result<String> {
    let (format, encoded) = match format {
        BreadFormat::Base64 => ("base64", BASE64.encode(data)),
        BreadFormat::Hex => (
            "hex",
            data.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        ),
        BreadFormat::Mcp => return Err(Error::Protocol("typed media is not a byte window")),
    };
    let next = offset
        .checked_add(data.len() as u64)
        .ok_or(Error::Limit("binary continuation offset overflow"))?;
    let next = (next < total).then_some(next);
    let mut output = json!({
        "path":path,
        "data":encoded,
        "format":format,
        "encoding":format,
        "offset":offset,
        "bytes":data.len(),
        "total_size":total,
        "total_bytes":total,
        "truncated":next.is_some()
    });
    if let Some(next) = next {
        output["next_offset"] = json!(next);
        output["status"] = json!(format!("more offset={next}"));
    }
    Ok(output.to_string())
}

/// Fit one encoded binary page to the semantic result budget without changing
/// raw-byte cursor semantics. The backend read is already bounded by the
/// caller's requested window; fitting only selects a prefix of that bounded
/// buffer. A non-empty source page must always advance or fail actionably.
fn fit_encoded_window(
    path: &str,
    data: &[u8],
    format: BreadFormat,
    offset: u64,
    total: u64,
    budget: usize,
) -> Result<ToolOutput> {
    let fits = |len: usize| -> Result<Option<String>> {
        let rendered = encoded_window(path, &data[..len], format, offset, total)?;
        Ok((rendered.len() <= budget).then_some(rendered))
    };
    if data.is_empty() {
        let rendered = fits(0)?.ok_or(Error::Limit(
            "binary page metadata exceeds configured result ceiling",
        ))?;
        return Ok(ToolOutput::text(rendered));
    }

    let mut low = 0usize;
    let mut high = data.len();
    let mut best = None;
    while low <= high {
        let mid = low + (high - low) / 2;
        match fits(mid)? {
            Some(rendered) => {
                best = Some((mid, rendered));
                low = mid.saturating_add(1);
            }
            None => {
                if mid == 0 {
                    break;
                }
                high = mid - 1;
            }
        }
    }
    let (kept, rendered) = best.ok_or(Error::Limit(
        "binary page metadata exceeds configured result ceiling",
    ))?;
    if kept == 0 {
        return Err(Error::Limit(
            "one binary byte cannot fit configured result ceiling",
        ));
    }
    let next = offset
        .checked_add(kept as u64)
        .ok_or(Error::Limit("binary continuation offset overflow"))?;
    Ok(ToolOutput {
        content: rendered,
        is_error: false,
        truncated: next < total,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BreadArgs {
    path: String,
    #[serde(default)]
    offset: u64,
    limit: Option<usize>,
    #[serde(default)]
    format: BreadFormat,
    #[serde(default)]
    parallel: bool,
}
impl BreadArgs {
    fn limit(&self, schema: aw_tools::filesystem::schema::SchemaCaps) -> usize {
        let ceiling = if matches!(self.format, BreadFormat::Mcp) {
            schema.binary_read_bytes
        } else {
            schema.binary_encoded_read_bytes
        };
        self.limit
            .unwrap_or_else(|| DEFAULT_READ_WINDOW.min(ceiling))
    }
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
    aw_tools::mutation::bounded_array::<D, Range, MAX_EDIT_RANGES>(de, "bedit", "ranges")
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
    limits: crate::LocalToolLimits,
}
impl Binary {
    #[cfg(test)]
    pub(crate) fn new(root: Arc<Root>, op: Operation) -> Self {
        Self::with_limits(root, op, crate::LocalToolLimits::default())
    }
    pub(crate) fn with_limits(
        root: Arc<Root>,
        op: Operation,
        limits: crate::LocalToolLimits,
    ) -> Self {
        Self { root, op, limits }
    }
}
fn binary_plan_error(error: aw_tools::binary::PlanError) -> Error {
    use aw_tools::binary::PlanError as E;
    match error {
        E::Empty | E::TooMany => Error::Limit("bedit requires 1..=32 ranges"),
        E::RangeOverflow => Error::Limit("binary range overflow"),
        E::PastEof => Error::Denied("binary range is past EOF"),
        E::Overlap => Error::Denied("binary edit ranges overlap"),
        E::PayloadOverflow => Error::Limit("binary payload overflow"),
        E::OutputOverflow => Error::Limit("binary output size overflow"),
        E::OutputTooLarge => Error::Limit("binary edit exceeds 64 MiB"),
    }
}

/// Decode only the transport encoding here; ordering/overlap/output-size
/// semantics and filesystem execution remain canonical aw-tools behavior.
fn decode_ranges_with_limit(
    ranges: Vec<Range>,
    max_data: usize,
) -> Result<Vec<aw_tools::binary::RequestedSplice>> {
    ranges
        .into_iter()
        .map(|range| {
            Ok(aw_tools::binary::RequestedSplice {
                offset: range.offset,
                length: range.length,
                data: range.encoding.decode_with_limit(&range.data, max_data)?,
            })
        })
        .collect()
}

fn parse_sha256(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Denied("sha256 must be 64 hexadecimal characters"));
    }
    let mut digest = [0u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let pair =
            std::str::from_utf8(pair).map_err(|_| Error::Denied("sha256 must be hexadecimal"))?;
        digest[index] = u8::from_str_radix(pair, 16)
            .map_err(|_| Error::Denied("sha256 must be hexadecimal"))?;
    }
    Ok(digest)
}

#[cfg(test)]
fn prepare_ranges(ranges: Vec<Range>, source_size: usize) -> Result<aw_tools::binary::SplicePlan> {
    let requested = decode_ranges_with_limit(
        ranges,
        aw_tools::filesystem::schema::SchemaCaps::default().binary_data_bytes,
    )?;
    aw_tools::binary::plan_splices(requested, source_size, MAX_FILE_BYTES)
        .map_err(binary_plan_error)
}

impl Tool for Binary {
    fn spec(&self) -> ToolSpec {
        use aw_tools::filesystem::schema::Tool as C;
        let tool = match self.op {
            Operation::Read => C::Bread,
            Operation::Write => C::Bwrite,
            Operation::Edit => C::Bedit,
        };
        crate::core_spec(tool, self.limits.schema)
    }
    fn validate(&self, value: &Value) -> Result<()> {
        match self.op {
            Operation::Read => {
                let a: BreadArgs = parse(value)?;
                crate::model_path(&a.path)?;
                let limit = a.limit(self.limits.schema);
                if limit == 0 || limit > self.limits.schema.binary_read_bytes {
                    return Err(Error::Limit(
                        "binary window exceeds configured read ceiling",
                    ));
                }
                if !matches!(a.format, BreadFormat::Mcp)
                    && limit > self.limits.schema.binary_encoded_read_bytes
                {
                    return Err(Error::Limit(
                        "encoded binary window exceeds configured read ceiling",
                    ));
                }
                if matches!(a.format, BreadFormat::Mcp) && a.offset != 0 {
                    return Err(Error::Denied("format=mcp requires offset=0"));
                }
                let _ = a.parallel;
            }
            Operation::Write => {
                let a: BwriteArgs = parse(value)?;
                crate::model_path(&a.path)?;
                a.encoding
                    .decode_with_limit(&a.data, self.limits.schema.binary_data_bytes)?;
                crate::mutate::validate_digest(a.expected_sha256.as_deref())?;
            }
            Operation::Edit => {
                let a: BeditArgs = parse(value)?;
                crate::model_path(&a.path)?;
                if a.edits.is_empty() || a.edits.len() > self.limits.schema.binary_edit_ranges {
                    return Err(Error::Limit("too many binary edits"));
                }
                for r in &a.edits {
                    if r.data.len() > self.limits.schema.binary_data_bytes {
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
            let result_bytes = ctx.result_budget(self.limits.result_bytes)?;
            let value = value.clone();
            let root = self.root.clone();
            let op = self.op;
            let limits = self.limits;
            let answer = aw_tools::asynchronous::run_blocking(move || -> Result<ToolOutput> {
                if ctx.cancellation.is_cancelled() {
                    return Err(Error::Cancelled);
                }
                match op {
                    Operation::Read => {
                        let a: BreadArgs = parse(&value)?;
                        let content_path = crate::content_path(&root, &a.path, false)?;
                        let limit = a.limit(limits.schema);
                        if matches!(a.format, BreadFormat::Mcp) {
                            let bytes = platform_fs(root.read_bytes(&content_path, limit))?;
                            let packet=aw_tools::media::MediaPacket::from_complete_bytes(bytes)
                                .map_err(|_|Error::Limit("inline media exceeds 64 KiB"))?;
                            let descriptor=packet.descriptor();
                            let mime=descriptor.mime;
                            let kind=descriptor.kind.as_str();
                            // Transport-neutral compatibility encoding. This
                            // remains complete-object/direct-only until AW has a
                            // provider-native typed ToolOutput channel.
                            let output = json!({
                                "type":kind, "mime_type":mime, "format":"mcp",
                                "path":a.path,"data":BASE64.encode(packet.bytes()),
                                "uri":if kind=="resource"{Some(resource_uri(&a.path)?)}else{None},
                                "bytes":packet.bytes().len(),
                                "sha256":descriptor.sha256_hex(),
                                "mime_source":if descriptor.signature_identified{"signature"}else{"fallback"},
                                "transport":"encoded_json"
                            }).to_string();
                            if output.len() > result_bytes {
                                return Err(Error::Limit(
                                    "format=mcp complete object exceeds configured result ceiling; use base64/hex byte windows",
                                ));
                            }
                            Ok(ToolOutput::text(output))
                        } else {
                            let (data, total, _) =
                                platform_fs(root.read_binary_window(&content_path, a.offset, limit))?;
                            fit_encoded_window(
                                &a.path,
                                &data,
                                a.format,
                                a.offset,
                                total,
                                result_bytes,
                            )
                        }
                    }
                    Operation::Write => {
                        let a: BwriteArgs = parse(&value)?;
                        let bytes = a
                            .encoding
                            .decode_with_limit(&a.data, limits.schema.binary_data_bytes)?;
                        if !a.create_only && a.expected_sha256.is_none(){
                            let content_path = crate::content_path(&root, &a.path, true)?;
                            let result=aw_tools::filesystem::operations::write_bytes(
                                root.as_ref(),&content_path,&bytes,MAX_FILE_BYTES,
                                &||ctx.cancellation.is_cancelled()
                            ).map_err(crate::file_operation_error)?;
                            Ok(ToolOutput::text(result.as_str()))
                        }else{
                            let content_path = if a.create_only {
                                a.path.clone()
                            } else {
                                crate::content_path(&root, &a.path, false)?
                            };
                            let changed=crate::mutate::write(
                                &root,&content_path,&bytes,a.create_only,
                                a.expected_sha256.as_deref(),
                                &||ctx.cancellation.is_cancelled()
                            )?;
                            Ok(ToolOutput::text(
                                aw_tools::mutation::compact_success(
                                    usize::from(changed),None
                                )
                            ))
                        }
                    }
                    Operation::Edit => {
                        use aw_tools::filesystem::operations::{self, Error as OperationError};
                        let a: BeditArgs = parse(&value)?;
                        let content_path = crate::content_path(&root, &a.path, false)?;
                        let requested = decode_ranges_with_limit(
                            a.edits,
                            limits.schema.binary_data_bytes,
                        )?;
                        let expected = a.sha256.as_deref().map(parse_sha256).transpose()?;
                        match operations::edit_binary(
                            root.as_ref(),
                            &content_path,
                            requested,
                            expected,
                            MAX_FILE_BYTES,
                            &|| ctx.cancellation.is_cancelled(),
                        ) {
                            Ok(primary) => Ok(ToolOutput::text(primary.as_str())),
                            Err(OperationError::BinaryPlan(error)) => Err(binary_plan_error(error)),
                            Err(OperationError::BinaryPrecondition) => Ok(ToolOutput::error(
                                "bedit: sha256 precondition mismatch",
                            )),
                            Err(OperationError::BinarySourceChanged) => Ok(ToolOutput::error(
                                "bedit: source changed during edit; re-read before retrying",
                            )),
                            Err(error) => Err(crate::file_operation_error(error)),
                        }
                    }
                }
            })
            .await
            .map_err(|_| {
                Error::Protocol("binary worker failed; inspect workspace before retrying")
            })?;
            let answer = if matches!(op, Operation::Read) {
                answer.and_then(|output| {
                    if output.content.len() > self.limits.result_bytes {
                        Err(Error::Limit(
                            "binary result exceeds configured tool result ceiling; request a smaller window",
                        ))
                    } else {
                        Ok(output)
                    }
                })
            } else {
                answer
            };
            tool_failure(answer, !matches!(op, Operation::Read))
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_wire_decoder_delegates_range_semantics_to_core() {
        let plan = prepare_ranges(
            vec![
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
            ],
            4,
        )
        .unwrap();
        assert_eq!(plan.output_len, 5);
        assert_eq!(plan.payload_bytes, 2);
        assert_eq!(plan.edits[0].data, b"Z");
        assert_eq!(plan.edits[1].data, b"Y");

        let overlap = prepare_ranges(
            vec![
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
            ],
            4,
        );
        assert!(matches!(
            overlap,
            Err(Error::Denied("binary edit ranges overlap"))
        ));
    }
}
