//! Canonical direct filesystem execution over one already-authorized backend.
//!
//! Product adapters own authority, journaling, transport and presentation.
//! This module owns the transport-neutral mapping from canonical typed requests
//! to shared filesystem/search/mutation semantics so AW and dotlink do not
//! maintain parallel 13-tool dispatch implementations.

use super::{
    backend::{ExplicitPathBackend, SecuredFilesystem},
    discovery, operations,
    request::{self, BinaryFormat, Request, RequestKind},
    status, Filter, Kind, MAX_RENDER_BYTES, MAX_WALK_DEPTH,
};
use crate::{
    binary::PlanError as BinaryPlanError,
    media::MediaPacket,
    search::{self, GrepLimits, GrepOptions},
    MAX_MUTATION_PLAN_BYTES, MAX_READ_WINDOW_BYTES,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde_json::json;

/// Host-selected semantic ceilings. These may narrow shared engine maxima but
/// never authorize a path or widen an already-prepared filesystem capability.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub request: request::Limits,
    /// Complete semantic result ceiling for discovery/grep/text-status rendering.
    pub result_bytes: usize,
    /// Exact source-data bytes available to one `read` page. This is independent
    /// of rendered-result bytes because adapters may project raw source data
    /// through a separate transport budget.
    pub read_data_bytes: usize,
    /// Complete encoded base64/hex result ceiling. Encoded binary expands
    /// independently of both raw read bytes and ordinary text rendering.
    pub binary_result_bytes: usize,
    /// Per-file UTF-8 preimage ceiling for edit/patch updates.
    pub text_read_bytes: usize,
    /// Per-file UTF-8 replacement/add-file ceiling for edit/patch.
    pub text_write_bytes: usize,
    /// Per-file raw byte ceiling for bwrite/bedit.
    pub binary_file_bytes: usize,
    /// Omitted bread limit. Encoded windows are additionally capped by the
    /// configured encoded-read schema ceiling.
    pub binary_default_window: usize,
    /// Work/candidate/time ceilings for grep. `output_bytes` is ignored and
    /// replaced with the effective `result_bytes` above.
    pub grep: GrepLimits,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            request: request::Limits::default(),
            result_bytes: 96 * 1024,
            read_data_bytes: 96 * 1024 - 256,
            binary_result_bytes: 96 * 1024,
            text_read_bytes: 1024 * 1024,
            text_write_bytes: 1024 * 1024,
            binary_file_bytes: MAX_READ_WINDOW_BYTES,
            binary_default_window: 24 * 1024,
            grep: GrepLimits::default(),
        }
    }
}
impl Limits {
    pub fn validate(self) -> Result<Self, Error<std::convert::Infallible>> {
        self.request
            .validate()
            .map_err(|error| Error::Invalid(error.to_string()))?;
        if !(512..=MAX_RENDER_BYTES).contains(&self.result_bytes)
            || !(1..=MAX_READ_WINDOW_BYTES).contains(&self.read_data_bytes)
            || !(512..=MAX_RENDER_BYTES).contains(&self.binary_result_bytes)
            || self.text_read_bytes == 0
            || self.text_read_bytes > MAX_MUTATION_PLAN_BYTES
            || self.text_write_bytes == 0
            || self.text_write_bytes > MAX_MUTATION_PLAN_BYTES
            || self.binary_file_bytes == 0
            || self.binary_file_bytes > MAX_READ_WINDOW_BYTES
            || self.binary_default_window == 0
            || self.binary_default_window > self.request.schema.binary_read_bytes
            || self.grep.candidates == 0
            || self.grep.scan_bytes == 0
            || self.grep.scan_files == 0
            || self.grep.text_file_bytes == 0
            || self.grep.duration.is_zero()
        {
            return Err(Error::Limit("invalid canonical direct-execution ceilings"));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone)]
pub enum Body {
    /// Ordinary canonical text/status or encoded binary-window result.
    Text(String),
    /// Exact UTF-8 source bytes plus an orthogonal continuation/fault status.
    /// Complete reads have `status=None` and are byte-exact source text.
    Read {
        data: String,
        status: Option<String>,
        next: Option<(usize, usize)>,
        numbered: bool,
    },
    /// Complete typed-media candidate; the product decides how/where bytes may
    /// be attached. This is not provider permission.
    Media { path: String, packet: MediaPacket },
}

#[derive(Debug, Clone)]
pub struct Output {
    pub body: Body,
    pub is_error: bool,
    pub truncated: bool,
}
impl Output {
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            body: Body::Text(content.into()),
            is_error: false,
            truncated: false,
        }
    }
    pub fn fail(message: impl AsRef<str>) -> Self {
        let message = message.as_ref();
        let content = if message.starts_with("fail")
            || message.starts_with("mixed")
            || message.starts_with("unknown")
        {
            message.to_owned()
        } else {
            format!("fail {message}")
        };
        Self {
            body: Body::Text(content),
            is_error: true,
            truncated: false,
        }
    }
}

#[derive(Debug)]
pub enum Error<E> {
    Invalid(String),
    Limit(&'static str),
    Cancelled,
    Backend(E),
    /// A visible publication may have happened; never retry automatically.
    UnknownCommit(E),
    /// Multiple publication units may already be visible; inspect before any
    /// retry. The backend error is retained when one exists.
    PartialCommit(Option<E>),
}

fn model_root(path: &str) -> &str {
    if path.is_empty() || path == "." {
        ""
    } else {
        path
    }
}

fn discovery_error<E>(error: discovery::Error<E>) -> Error<E> {
    match error {
        discovery::Error::InvalidOptions(reason) => Error::Invalid(reason.into()),
        discovery::Error::Walk(error) => match error {
            super::walk::WalkError::Source(error) => Error::Backend(error),
            super::walk::WalkError::Cancelled => Error::Cancelled,
            super::walk::WalkError::IgnoreLimit => {
                Error::Limit("gitignore search budget exhausted")
            }
            super::walk::WalkError::InvalidIgnore => {
                Error::Invalid("invalid gitignore pattern".into())
            }
        },
    }
}

fn grep_error<E>(error: search::GrepError<E>) -> Error<E> {
    match error {
        search::GrepError::Query(_) => Error::Invalid("invalid grep query".into()),
        search::GrepError::Source(error) => Error::Backend(error),
        search::GrepError::Walk(error) => match error {
            super::walk::WalkError::Source(error) => Error::Backend(error),
            super::walk::WalkError::Cancelled => Error::Cancelled,
            super::walk::WalkError::IgnoreLimit => {
                Error::Limit("gitignore search budget exhausted")
            }
            super::walk::WalkError::InvalidIgnore => {
                Error::Invalid("invalid gitignore pattern".into())
            }
        },
        search::GrepError::Cancelled => Error::Cancelled,
        search::GrepError::ResultLimit(reason) => Error::Limit(reason),
    }
}

fn edit_error(error: super::exact_edit::EditError) -> String {
    use super::exact_edit::EditError;
    match error {
        EditError::Empty => "edit old must not be empty".into(),
        EditError::Missing(id) => format!("edit {id} old not found"),
        EditError::Ambiguous { edit, matches } => {
            format!("edit {edit} old matches {matches} times; set all=true or use patch")
        }
        EditError::Overlap { first, second } => format!("edits {first},{second} overlap"),
        EditError::TooMany => "edits must contain 1..512 tuples".into(),
        EditError::OutputLimit => "resulting edit exceeds file byte limit".into(),
    }
}
fn binary_plan_error(error: BinaryPlanError) -> &'static str {
    match error {
        BinaryPlanError::Empty | BinaryPlanError::TooMany => "invalid binary edit count",
        BinaryPlanError::RangeOverflow => "binary range overflow",
        BinaryPlanError::PastEof => "binary range is past EOF",
        BinaryPlanError::Overlap => "binary edit ranges overlap",
        BinaryPlanError::PayloadOverflow => "binary payload overflow",
        BinaryPlanError::OutputOverflow => "binary output size overflow",
        BinaryPlanError::OutputTooLarge => "binary edit exceeds file byte limit",
    }
}

fn operation_result<E>(
    result: Result<super::status::Primary, operations::Error<E>>,
) -> Result<Output, Error<E>> {
    use operations::Error as E;
    match result {
        Ok(primary) => Ok(Output::text(primary.as_str())),
        Err(E::InvalidPath(_)) => Ok(Output::fail("invalid exact path")),
        Err(E::TooLarge) => Ok(Output::fail("file exceeds configured byte ceiling")),
        Err(E::InvalidText) => Ok(Output::fail(
            "binary/control-containing content; use binary tools",
        )),
        Err(E::Edit(error)) => Ok(Output::fail(edit_error(error))),
        Err(E::BinaryPlan(error)) => Ok(Output::fail(binary_plan_error(error))),
        Err(E::BinaryPrecondition) => Ok(Output::fail("sha256 precondition mismatch")),
        Err(E::BinarySourceChanged) => Ok(Output::fail(
            "binary source changed during edit; re-read before retrying",
        )),
        Err(E::Patch(reason)) => Ok(Output::fail(reason)),
        Err(E::DestinationExists) => Ok(Output::fail("destination exists")),
        Err(E::Cancelled) => Err(Error::Cancelled),
        Err(E::PartialCancelled { .. }) => Err(Error::PartialCommit(None)),
        Err(E::PatchCommitUnknown { error, .. }) | Err(E::UnknownCommit(error)) => {
            Err(Error::UnknownCommit(error))
        }
        Err(E::PartialCommit { error, .. }) => Err(Error::PartialCommit(Some(error))),
        Err(E::Backend(error)) => Err(Error::Backend(error)),
    }
}

fn binary_format(format: BinaryFormat) -> &'static str {
    match format {
        BinaryFormat::Mcp => "mcp",
        BinaryFormat::Base64 => "base64",
        BinaryFormat::Hex => "hex",
    }
}
fn encode_hex(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(data.len().saturating_mul(2));
    for &byte in data {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
fn encoded_window(
    path: &str,
    data: &[u8],
    format: BinaryFormat,
    offset: u64,
    total: u64,
) -> Result<String, Error<std::convert::Infallible>> {
    let encoded = match format {
        BinaryFormat::Base64 => BASE64.encode(data),
        BinaryFormat::Hex => encode_hex(data),
        BinaryFormat::Mcp => return Err(Error::Invalid("mcp is not an encoded window".into())),
    };
    let next = offset
        .checked_add(data.len() as u64)
        .filter(|next| *next < total);
    let format = binary_format(format);
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
fn fit_encoded_window<E>(
    path: &str,
    data: &[u8],
    format: BinaryFormat,
    offset: u64,
    total: u64,
    budget: usize,
) -> Result<Output, Error<E>> {
    let fits = |len: usize| -> Result<Option<String>, Error<E>> {
        let rendered = encoded_window(path, &data[..len], format, offset, total).map_err(
            |error| match error {
                Error::Invalid(reason) => Error::Invalid(reason),
                Error::Limit(reason) => Error::Limit(reason),
                _ => unreachable!("encoded renderer has no backend"),
            },
        )?;
        Ok((rendered.len() <= budget).then_some(rendered))
    };
    if data.is_empty() {
        let rendered = fits(0)?.ok_or(Error::Limit(
            "binary page metadata exceeds configured result ceiling",
        ))?;
        return Ok(Output::text(rendered));
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
    Ok(Output {
        body: Body::Text(rendered),
        is_error: false,
        truncated: next < total,
    })
}

fn sha256_bytes(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut digest = [0u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let pair = std::str::from_utf8(pair).ok()?;
        digest[index] = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(digest)
}

struct DiscoveryRequest<'a> {
    path: &'a str,
    depth: usize,
    hidden: bool,
    gitignore: bool,
    filter: &'a Filter,
    sort: super::Sort,
    meta: &'a [super::MetaKey],
    offset: usize,
    limit: usize,
    basenames: bool,
    output_bytes: usize,
}

fn discovery_page<B: SecuredFilesystem + ExplicitPathBackend>(
    backend: &B,
    request: DiscoveryRequest<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Output, Error<B::Error>> {
    let model = model_root(request.path);
    let resolved = if model.is_empty() {
        None
    } else {
        Some(
            backend
                .resolve_directory_path(model)
                .map_err(Error::Backend)?,
        )
    };
    let page = discovery::run(
        backend,
        discovery::Options {
            path: model,
            backend_path: resolved.as_deref(),
            depth: request.depth,
            hidden: request.hidden,
            gitignore: request.gitignore,
            filter: request.filter,
            sort: request.sort,
            meta: request.meta,
            offset: request.offset,
            limit: request.limit,
            basenames: request.basenames,
            output_bytes: request.output_bytes,
        },
        cancelled,
    )
    .map_err(discovery_error)?;
    Ok(Output {
        body: Body::Text(page.content),
        is_error: false,
        truncated: page.truncated,
    })
}

fn grep_backend_path<B: SecuredFilesystem + ExplicitPathBackend>(
    backend: &B,
    model: &str,
) -> Result<String, Error<B::Error>> {
    if model.is_empty() {
        return Ok(String::new());
    }
    let kind = backend.entry_meta(model).map_err(Error::Backend)?.kind;
    match kind {
        Kind::File => backend
            .resolve_content_path(model, false)
            .map_err(Error::Backend),
        Kind::Dir => backend
            .resolve_directory_path(model)
            .map_err(Error::Backend),
        Kind::Symlink => backend
            .resolve_directory_path(model)
            .or_else(|_| backend.resolve_content_path(model, false))
            .map_err(Error::Backend),
        _ => Err(Error::Invalid(
            "grep path must be a file or directory".into(),
        )),
    }
}

/// Execute one already-parsed canonical direct request. The backend is the
/// authority-bearing object; no ambient path is opened here.
pub fn execute<B>(
    backend: &B,
    request: Request,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<Output, Error<B::Error>>
where
    B: SecuredFilesystem + ExplicitPathBackend,
{
    let limits = limits.validate().map_err(|error| match error {
        Error::Invalid(reason) => Error::Invalid(reason),
        Error::Limit(reason) => Error::Limit(reason),
        _ => unreachable!("limit validation has no backend"),
    })?;
    if cancelled() {
        return Err(Error::Cancelled);
    }
    match request.into_kind() {
        RequestKind::Ls(args) => {
            let filter = Filter::compile(None, None, Kind::Any, None, None, None)
                .map_err(|reason| Error::Invalid(reason.into()))?;
            discovery_page(
                backend,
                DiscoveryRequest {
                    path: &args.path,
                    depth: 1,
                    hidden: args.hidden,
                    gitignore: args.gitignore,
                    filter: &filter,
                    sort: args.sort,
                    meta: &args.meta,
                    offset: args.offset,
                    limit: args.limit,
                    basenames: true,
                    output_bytes: limits.result_bytes,
                },
                cancelled,
            )
        }
        RequestKind::Find(args) => {
            let filter = Filter::compile(
                args.glob.as_deref(),
                args.regex.as_deref(),
                args.kind,
                args.mtime,
                args.ctime,
                args.btime,
            )
            .map_err(|reason| Error::Invalid(reason.into()))?;
            discovery_page(
                backend,
                DiscoveryRequest {
                    path: &args.path,
                    depth: args.depth.unwrap_or(MAX_WALK_DEPTH),
                    hidden: args.hidden,
                    gitignore: args.gitignore,
                    filter: &filter,
                    sort: args.sort,
                    meta: &args.meta,
                    offset: args.offset,
                    limit: args.limit,
                    basenames: false,
                    output_bytes: limits.result_bytes,
                },
                cancelled,
            )
        }
        RequestKind::Grep(args) => {
            let model = model_root(&args.path);
            let backend_path = grep_backend_path(backend, model)?;
            let page = search::grep(
                backend,
                GrepOptions {
                    pattern: &args.pattern,
                    path: model,
                    backend_path: Some(&backend_path),
                    glob: args.glob.as_deref(),
                    depth: args.depth.unwrap_or(MAX_WALK_DEPTH),
                    literal: args.literal,
                    ignore_case: args.ignore_case,
                    mode: args.mode.into(),
                    before: args.context,
                    after: args.context,
                    hidden: args.hidden,
                    gitignore: args.gitignore,
                    offset: args.offset,
                    limit: args.limit,
                    limits: GrepLimits {
                        text_file_bytes: limits.grep.text_file_bytes,
                        output_bytes: limits.result_bytes,
                        ..limits.grep
                    },
                },
                cancelled,
            )
            .map_err(grep_error)?;
            Ok(Output {
                body: Body::Text(page.content),
                is_error: false,
                truncated: page.truncated,
            })
        }
        RequestKind::Read(args) => {
            let path = backend
                .resolve_content_path(&args.path, false)
                .map_err(Error::Backend)?;
            let page = backend
                .read_page(
                    &path,
                    crate::read::PageRequest {
                        line: args.line,
                        lines: args.lines,
                        byte: args.byte,
                        cap: limits.read_data_bytes,
                        numbered: args.numbered,
                    },
                    cancelled,
                )
                .map_err(Error::Backend)?;
            let partial = page.next.is_some() || page.io_fault;
            let next = page.next.map(|c| (c.line, c.byte));
            let status = partial.then(|| status::read(next, page.io_fault)).flatten();
            Ok(Output {
                body: Body::Read {
                    data: page.data,
                    status,
                    next,
                    numbered: args.numbered,
                },
                is_error: page.io_fault,
                truncated: partial,
            })
        }
        RequestKind::Write(args) => {
            let path = backend
                .resolve_content_path(&args.path, true)
                .map_err(Error::Backend)?;
            operation_result(operations::write_text(
                backend,
                &path,
                &args.content,
                limits.request.schema.text_write_bytes,
                cancelled,
            ))
        }
        RequestKind::Edit(args) => {
            let path = backend
                .resolve_content_path(&args.path, false)
                .map_err(Error::Backend)?;
            let replacements = args
                .edits
                .iter()
                .map(|edit| super::exact_edit::Replacement {
                    old: &edit.old,
                    new: &edit.new,
                    all: edit.all,
                })
                .collect::<Vec<_>>();
            operation_result(operations::edit_text(
                backend,
                &path,
                &replacements,
                limits.text_read_bytes,
                limits.text_write_bytes,
                cancelled,
            ))
        }
        RequestKind::Patch(args) => operation_result(operations::patch_text(
            backend,
            &args.patch,
            limits.text_read_bytes,
            limits.text_write_bytes,
            cancelled,
        )),
        RequestKind::Copy(args) => operation_result(operations::copy_path(
            backend, &args.from, &args.to, true, cancelled,
        )),
        RequestKind::Move(args) => operation_result(operations::move_path(
            backend, &args.from, &args.to, cancelled,
        )),
        RequestKind::Remove(args) => {
            match operations::remove_paths(backend, &args.paths, args.recursive, cancelled) {
                Ok(outcome) => Ok(Output {
                    body: Body::Text(outcome.compact()),
                    is_error: outcome.error(),
                    truncated: false,
                }),
                Err(error) => operation_result::<B::Error>(Err(error)),
            }
        }
        RequestKind::Bread(args) => {
            let path = backend
                .resolve_content_path(&args.path, false)
                .map_err(Error::Backend)?;
            let format_max = match args.format {
                BinaryFormat::Mcp => limits.request.schema.binary_read_bytes,
                BinaryFormat::Base64 | BinaryFormat::Hex => {
                    limits.request.schema.binary_encoded_read_bytes
                }
            };
            let limit = args
                .limit
                .unwrap_or_else(|| limits.binary_default_window.min(format_max));
            if args.format == BinaryFormat::Mcp {
                let (bytes, _, truncated) = backend
                    .read_binary_window(&path, 0, limit, cancelled)
                    .map_err(Error::Backend)?;
                if truncated {
                    return Err(Error::Limit(
                        "format=mcp complete object exceeds configured read ceiling; use base64/hex byte windows",
                    ));
                }
                let packet = MediaPacket::from_complete_bytes_with_limit(
                    bytes,
                    limits.request.schema.binary_read_bytes,
                )
                .map_err(|_| Error::Invalid("unsupported or oversized typed media".into()))?;
                Ok(Output {
                    body: Body::Media {
                        path: args.path,
                        packet,
                    },
                    is_error: false,
                    truncated: false,
                })
            } else {
                let (data, total, _) = backend
                    .read_binary_window(&path, args.offset, limit, cancelled)
                    .map_err(Error::Backend)?;
                fit_encoded_window(
                    &args.path,
                    &data,
                    args.format,
                    args.offset,
                    total,
                    limits.binary_result_bytes,
                )
            }
        }
        RequestKind::Bwrite(args) => {
            let path = backend
                .resolve_content_path(&args.path, true)
                .map_err(Error::Backend)?;
            operation_result(operations::write_bytes(
                backend,
                &path,
                &args.data,
                limits.binary_file_bytes,
                cancelled,
            ))
        }
        RequestKind::Bedit(args) => {
            let path = backend
                .resolve_content_path(&args.path, false)
                .map_err(Error::Backend)?;
            let expected = args.sha256.as_deref().and_then(sha256_bytes);
            operation_result(operations::edit_binary(
                backend,
                &path,
                args.requested_splices(),
                expected,
                limits.binary_file_bytes,
                cancelled,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_window_fits_budget_and_advances_raw_offset() {
        let out = fit_encoded_window::<std::convert::Infallible>(
            "a.bin",
            b"0123456789",
            BinaryFormat::Hex,
            5,
            20,
            180,
        )
        .unwrap();
        let Body::Text(text) = out.body else {
            panic!("text result");
        };
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(value["bytes"].as_u64().unwrap() > 0);
        assert_eq!(
            value["next_offset"].as_u64().unwrap(),
            5 + value["bytes"].as_u64().unwrap()
        );
    }

    #[test]
    fn direct_limits_are_narrow_only_against_engine_bounds() {
        assert!(Limits::default().validate().is_ok());
        let independent = Limits {
            result_bytes: 4 * 1024,
            read_data_bytes: 8 * 1024 * 1024,
            binary_result_bytes: 16 * 1024 * 1024,
            text_read_bytes: 64 * 1024,
            text_write_bytes: 4 * 1024,
            grep: GrepLimits {
                text_file_bytes: 2 * 1024 * 1024,
                ..GrepLimits::default()
            },
            ..Limits::default()
        };
        assert!(independent.validate().is_ok());
        assert!(Limits {
            result_bytes: MAX_RENDER_BYTES + 1,
            ..Limits::default()
        }
        .validate()
        .is_err());
        assert!(Limits {
            binary_result_bytes: MAX_RENDER_BYTES + 1,
            ..Limits::default()
        }
        .validate()
        .is_err());
        assert!(Limits {
            read_data_bytes: MAX_READ_WINDOW_BYTES + 1,
            ..Limits::default()
        }
        .validate()
        .is_err());
        assert!(Limits {
            binary_file_bytes: MAX_READ_WINDOW_BYTES + 1,
            ..Limits::default()
        }
        .validate()
        .is_err());
    }
}
