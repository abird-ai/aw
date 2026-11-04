//! Canonical model-facing filesystem request decoding and validation.
//!
//! Product adapters may translate legacy inputs into these shapes, but neither
//! AW nor dotlink should maintain a second canonical parser. Runtime authority,
//! presentation budgets, transport projection and filesystem effects remain
//! host-owned.

use super::{lifecycle, path, Filter, Kind, MetaKey, Selection, Sort, TimeRange, MAX_WALK_DEPTH};
use crate::{
    binary::{RequestedSplice, MAX_EDIT_RANGES},
    filesystem::schema::{SchemaCaps, Tool},
    patch::{parse_patch, PatchOp},
    read::MAX_OFFSET_SCAN_BYTES,
    search::{self, GrepLimits, GrepMode, GrepOptions},
    text, MAX_MUTATION_PLAN_BYTES,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Deserialize;
use serde_json::Value;
use std::fmt;

const DEFAULT_DISCOVERY_LIMIT: usize = 200;
const DEFAULT_LS_LIMIT: usize = 4096;
const DEFAULT_READ_LINES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub schema: SchemaCaps,
    /// Aggregate old+new UTF-8 bytes accepted by one canonical edit request.
    pub edit_text_bytes: usize,
    /// Aggregate decoded binary payload bytes accepted by one bwrite/bedit
    /// request. `SchemaCaps::binary_data_bytes` independently bounds each
    /// encoded JSON string before decoding.
    pub binary_decoded_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            schema: SchemaCaps::default(),
            edit_text_bytes: SchemaCaps::default().text_write_bytes,
            binary_decoded_bytes: SchemaCaps::default().binary_data_bytes,
        }
    }
}
impl Limits {
    pub fn validate(self) -> Result<Self, Error> {
        self.schema.validate().map_err(Error::Invalid)?;
        if self.edit_text_bytes == 0 || self.edit_text_bytes > MAX_MUTATION_PLAN_BYTES {
            return Err(Error::Invalid(
                "edit text ceiling exceeds shared engine safety bounds",
            ));
        }
        if self.binary_decoded_bytes == 0 || self.binary_decoded_bytes > MAX_MUTATION_PLAN_BYTES {
            return Err(Error::Invalid(
                "decoded binary payload ceiling exceeds shared engine safety bounds",
            ));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Shape(&'static str),
    Invalid(&'static str),
    Detail(String),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shape(reason) | Self::Invalid(reason) => f.write_str(reason),
            Self::Detail(reason) => f.write_str(reason),
        }
    }
}
impl std::error::Error for Error {}

fn decode<T: serde::de::DeserializeOwned>(value: &Value, name: &'static str) -> Result<T, Error> {
    serde_json::from_value(value.clone()).map_err(|_| Error::Shape(name))
}
fn dot() -> String {
    ".".into()
}
fn default_discovery_limit() -> usize {
    DEFAULT_DISCOVERY_LIMIT
}
fn default_ls_limit() -> usize {
    DEFAULT_LS_LIMIT
}
fn validate_path(value: &str) -> Result<(), Error> {
    path::validate(value).map_err(|_| Error::Invalid("invalid canonical filesystem path"))
}
fn validate_discovery_path(value: &str) -> Result<(), Error> {
    if value.is_empty() || value == "." {
        return Ok(());
    }
    validate_path(value)
}
fn validate_meta(meta: &[MetaKey]) -> Result<(), Error> {
    if meta.len() > 5 {
        return Err(Error::Invalid("at most five metadata columns are allowed"));
    }
    let mut seen = Vec::with_capacity(meta.len());
    for key in meta {
        if seen.contains(key) {
            return Err(Error::Invalid("duplicate metadata column"));
        }
        seen.push(*key);
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ls {
    #[serde(default = "dot")]
    pub path: String,
    #[serde(default)]
    pub sort: Sort,
    #[serde(default)]
    pub meta: Vec<MetaKey>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub gitignore: bool,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_ls_limit")]
    pub limit: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Find {
    #[serde(default = "dot")]
    pub path: String,
    pub glob: Option<String>,
    pub regex: Option<String>,
    pub depth: Option<usize>,
    #[serde(default)]
    pub kind: Kind,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub gitignore: bool,
    pub mtime: Option<TimeRange>,
    pub ctime: Option<TimeRange>,
    pub btime: Option<TimeRange>,
    #[serde(default)]
    pub sort: Sort,
    #[serde(default)]
    pub meta: Vec<MetaKey>,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_discovery_limit")]
    pub limit: usize,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GrepOutput {
    #[default]
    Content,
    Files,
    Count,
}
impl From<GrepOutput> for GrepMode {
    fn from(value: GrepOutput) -> Self {
        match value {
            GrepOutput::Content => Self::Content,
            GrepOutput::Files => Self::Files,
            GrepOutput::Count => Self::Count,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grep {
    pub pattern: String,
    #[serde(default = "dot")]
    pub path: String,
    pub glob: Option<String>,
    pub depth: Option<usize>,
    #[serde(default)]
    pub literal: bool,
    #[serde(default)]
    pub ignore_case: bool,
    #[serde(default)]
    pub mode: GrepOutput,
    #[serde(default)]
    pub context: usize,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub gitignore: bool,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_discovery_limit")]
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Read {
    pub path: String,
    pub line: usize,
    pub lines: usize,
    pub byte: usize,
    pub numbered: bool,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRead {
    path: String,
    line: Option<usize>,
    lines: Option<usize>,
    byte: Option<usize>,
    numbered: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Write {
    pub path: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditOp {
    pub old: String,
    pub new: String,
    pub all: bool,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum WireEditOp {
    Two((String, String)),
    Three((String, String, bool)),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub path: String,
    pub edits: Vec<EditOp>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireEdit {
    path: String,
    edits: Vec<WireEditOp>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    pub patch: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Copy {
    pub from: String,
    pub to: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Move {
    pub from: String,
    pub to: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Remove {
    pub paths: Vec<String>,
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BinaryFormat {
    #[default]
    Mcp,
    Base64,
    Hex,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BinaryEncoding {
    #[default]
    Base64,
    Hex,
}
impl BinaryEncoding {
    pub fn decode(self, data: &str) -> Result<Vec<u8>, Error> {
        match self {
            Self::Base64 => BASE64
                .decode(data)
                .map_err(|_| Error::Invalid("invalid base64 payload")),
            Self::Hex => decode_hex(data),
        }
    }
}
fn decode_hex(data: &str) -> Result<Vec<u8>, Error> {
    if !data.len().is_multiple_of(2) || !data.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Invalid("invalid hex payload"));
    }
    data.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_nibble(pair[0]).ok_or(Error::Invalid("invalid hex payload"))?;
            let low = hex_nibble(pair[1]).ok_or(Error::Invalid("invalid hex payload"))?;
            Ok((high << 4) | low)
        })
        .collect()
}
fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bread {
    pub path: String,
    #[serde(default)]
    pub format: BinaryFormat,
    #[serde(default)]
    pub offset: u64,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bwrite {
    pub path: String,
    pub data: Vec<u8>,
    pub encoding: BinaryEncoding,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireBwrite {
    path: String,
    data: String,
    #[serde(default)]
    encoding: BinaryEncoding,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinaryEdit {
    pub offset: usize,
    pub length: Option<usize>,
    pub data: Vec<u8>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireBinaryEdit {
    offset: usize,
    length: Option<usize>,
    #[serde(default)]
    data: String,
    #[serde(default)]
    encoding: BinaryEncoding,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bedit {
    pub path: String,
    pub edits: Vec<BinaryEdit>,
    pub sha256: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireBedit {
    path: String,
    edits: Vec<WireBinaryEdit>,
    sha256: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathAccess {
    Read,
    Write,
    ReadWrite,
}

/// Canonical path intent for host-side authority preparation/scheduling.
/// This never grants access; it only describes what the already-validated
/// request will ask the secured backend to authorize.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessPath {
    pub path: String,
    pub access: PathAccess,
    /// Content/discovery tools may follow an explicitly named final symlink;
    /// lifecycle operations act on the final namespace entry itself.
    pub follow_final: bool,
}

#[derive(Clone, Debug)]
pub enum RequestKind {
    Ls(Ls),
    Find(Find),
    Grep(Grep),
    Read(Read),
    Write(Write),
    Edit(Edit),
    Patch(Patch),
    Copy(Copy),
    Move(Move),
    Remove(Remove),
    Bread(Bread),
    Bwrite(Bwrite),
    Bedit(Bedit),
}

/// Executable canonical request. The inner typed representation is inspectable
/// for footprint/budget planning, but only this module can mint the validated
/// wrapper. Consumers cannot bypass parser/schema limits by constructing a
/// `RequestKind` directly and handing it to the shared executor.
#[derive(Clone, Debug)]
pub struct Request {
    kind: RequestKind,
}
impl Request {
    pub fn kind(&self) -> &RequestKind {
        &self.kind
    }

    pub fn access_paths(&self) -> Result<Vec<AccessPath>, Error> {
        use PathAccess::{Read, ReadWrite, Write};
        let one = |path: &str, access, follow_final| {
            vec![AccessPath {
                path: path.to_owned(),
                access,
                follow_final,
            }]
        };
        Ok(match &self.kind {
            RequestKind::Ls(args) => one(&args.path, Read, true),
            RequestKind::Find(args) => one(&args.path, Read, true),
            RequestKind::Grep(args) => one(&args.path, Read, true),
            RequestKind::Read(args) => one(&args.path, Read, true),
            RequestKind::Write(args) => one(&args.path, Write, true),
            RequestKind::Edit(args) => one(&args.path, ReadWrite, true),
            RequestKind::Patch(args) => {
                let operations =
                    parse_patch(&args.patch).map_err(|error| Error::Detail(error.to_string()))?;
                operations
                    .into_iter()
                    .map(|operation| match operation {
                        PatchOp::Update { path, .. } => AccessPath {
                            path,
                            access: ReadWrite,
                            follow_final: true,
                        },
                        PatchOp::Add { path, .. } => AccessPath {
                            path,
                            access: Write,
                            follow_final: false,
                        },
                    })
                    .collect()
            }
            RequestKind::Copy(args) => vec![
                AccessPath {
                    path: args.from.clone(),
                    access: Read,
                    follow_final: false,
                },
                AccessPath {
                    path: args.to.clone(),
                    access: Write,
                    follow_final: false,
                },
            ],
            RequestKind::Move(args) => vec![
                AccessPath {
                    path: args.from.clone(),
                    access: Write,
                    follow_final: false,
                },
                AccessPath {
                    path: args.to.clone(),
                    access: Write,
                    follow_final: false,
                },
            ],
            RequestKind::Remove(args) => args
                .paths
                .iter()
                .map(|path| AccessPath {
                    path: path.clone(),
                    access: Write,
                    follow_final: false,
                })
                .collect(),
            RequestKind::Bread(args) => one(&args.path, Read, true),
            RequestKind::Bwrite(args) => one(&args.path, Write, true),
            RequestKind::Bedit(args) => one(&args.path, ReadWrite, true),
        })
    }
    pub(crate) fn into_kind(self) -> RequestKind {
        self.kind
    }
}

pub fn parse(tool: Tool, value: &Value, limits: Limits) -> Result<Request, Error> {
    let limits = limits.validate()?;
    let kind = match tool {
        Tool::Ls => parse_ls(value).map(RequestKind::Ls),
        Tool::Find => parse_find(value).map(RequestKind::Find),
        Tool::Grep => parse_grep(value).map(RequestKind::Grep),
        Tool::Read => parse_read(value, limits).map(RequestKind::Read),
        Tool::Write => parse_write(value, limits).map(RequestKind::Write),
        Tool::Edit => parse_edit(value, limits).map(RequestKind::Edit),
        Tool::Patch => parse_patch_request(value, limits).map(RequestKind::Patch),
        Tool::Copy => parse_copy(value).map(RequestKind::Copy),
        Tool::Move => parse_move(value).map(RequestKind::Move),
        Tool::Remove => parse_remove(value).map(RequestKind::Remove),
        Tool::Bread => parse_bread(value, limits).map(RequestKind::Bread),
        Tool::Bwrite => parse_bwrite(value, limits).map(RequestKind::Bwrite),
        Tool::Bedit => parse_bedit(value, limits).map(RequestKind::Bedit),
    }?;
    Ok(Request { kind })
}

fn parse_ls(value: &Value) -> Result<Ls, Error> {
    let request: Ls = decode(value, "invalid ls arguments")?;
    validate_discovery_path(&request.path)?;
    validate_meta(&request.meta)?;
    Selection::new(request.sort, request.offset, request.limit).map_err(Error::Invalid)?;
    Ok(request)
}
fn parse_find(value: &Value) -> Result<Find, Error> {
    let request: Find = decode(value, "invalid find arguments")?;
    validate_discovery_path(&request.path)?;
    validate_meta(&request.meta)?;
    if request
        .depth
        .is_some_and(|depth| !(1..=MAX_WALK_DEPTH).contains(&depth))
    {
        return Err(Error::Invalid("invalid discovery depth"));
    }
    Selection::new(request.sort, request.offset, request.limit).map_err(Error::Invalid)?;
    Filter::compile(
        request.glob.as_deref(),
        request.regex.as_deref(),
        request.kind,
        request.mtime,
        request.ctime,
        request.btime,
    )
    .map_err(Error::Invalid)?;
    Ok(request)
}
fn parse_grep(value: &Value) -> Result<Grep, Error> {
    let request: Grep = decode(value, "invalid grep arguments")?;
    validate_discovery_path(&request.path)?;
    let context = request.context;
    let options = GrepOptions {
        pattern: &request.pattern,
        path: if request.path == "." {
            ""
        } else {
            &request.path
        },
        backend_path: None,
        glob: request.glob.as_deref(),
        depth: request.depth.unwrap_or(MAX_WALK_DEPTH),
        literal: request.literal,
        ignore_case: request.ignore_case,
        mode: request.mode.into(),
        before: context,
        after: context,
        hidden: request.hidden,
        gitignore: request.gitignore,
        offset: request.offset,
        limit: request.limit,
        // Request validity must not depend on an unrelated binary-read
        // ceiling. The host supplies the actual result/render budget at
        // execution time; use the shared engine render maximum only to satisfy
        // the bounded grep-query invariant here.
        limits: GrepLimits {
            output_bytes: super::MAX_RENDER_BYTES,
            ..Default::default()
        },
    };
    search::validate_grep(&options).map_err(|_| Error::Invalid("invalid grep query"))?;
    Ok(request)
}
fn parse_read(value: &Value, limits: Limits) -> Result<Read, Error> {
    let wire: WireRead = decode(value, "invalid read arguments")?;
    validate_path(&wire.path)?;
    let request = Read {
        path: wire.path,
        line: wire.line.unwrap_or(1),
        lines: wire
            .lines
            .unwrap_or_else(|| DEFAULT_READ_LINES.min(limits.schema.read_line_limit)),
        byte: wire.byte.unwrap_or(0),
        numbered: wire.numbered.unwrap_or(true),
    };
    if request.line == 0
        || !(1..=limits.schema.read_line_limit).contains(&request.lines)
        || request.byte > MAX_OFFSET_SCAN_BYTES
    {
        return Err(Error::Invalid("invalid read line/lines/byte cursor"));
    }
    Ok(request)
}
fn parse_write(value: &Value, limits: Limits) -> Result<Write, Error> {
    let request: Write = decode(value, "invalid write arguments")?;
    validate_path(&request.path)?;
    if request.content.len() > limits.schema.text_write_bytes {
        return Err(Error::Invalid(
            "write payload exceeds configured byte ceiling",
        ));
    }
    if text::has_binary_control(request.content.as_bytes()) {
        return Err(Error::Invalid(
            "write content contains binary control bytes",
        ));
    }
    Ok(request)
}
fn parse_edit(value: &Value, limits: Limits) -> Result<Edit, Error> {
    let wire: WireEdit = decode(value, "invalid edit arguments")?;
    validate_path(&wire.path)?;
    if wire.edits.is_empty()
        || wire.edits.len() > crate::filesystem::exact_edit::MAX_EXACT_REPLACEMENTS
    {
        return Err(Error::Invalid("edit requires 1..=512 replacements"));
    }
    let mut total = 0usize;
    let mut edits = Vec::with_capacity(wire.edits.len());
    for item in wire.edits {
        let (old, new, all) = match item {
            WireEditOp::Two((old, new)) => (old, new, false),
            WireEditOp::Three((old, new, all)) => (old, new, all),
        };
        if old.is_empty() {
            return Err(Error::Invalid("edit old text must not be empty"));
        }
        if text::has_binary_control(old.as_bytes()) || text::has_binary_control(new.as_bytes()) {
            return Err(Error::Invalid("edit text contains binary control bytes"));
        }
        total = total
            .checked_add(old.len())
            .and_then(|value| value.checked_add(new.len()))
            .ok_or(Error::Invalid("edit text byte count overflow"))?;
        if total > limits.edit_text_bytes {
            return Err(Error::Invalid(
                "combined edit text exceeds configured byte ceiling",
            ));
        }
        edits.push(EditOp { old, new, all });
    }
    Ok(Edit {
        path: wire.path,
        edits,
    })
}
fn parse_patch_request(value: &Value, limits: Limits) -> Result<Patch, Error> {
    let request: Patch = decode(value, "invalid patch arguments")?;
    if request.patch.len() > limits.schema.patch_bytes {
        return Err(Error::Invalid("patch text exceeds configured byte ceiling"));
    }
    if text::has_binary_control(request.patch.as_bytes()) {
        return Err(Error::Invalid("patch contains binary control bytes"));
    }
    let operations =
        parse_patch(&request.patch).map_err(|error| Error::Detail(error.to_string()))?;
    for operation in operations {
        let section_path = match operation {
            PatchOp::Update { path, .. } | PatchOp::Add { path, .. } => path,
        };
        validate_path(&section_path)?;
    }
    Ok(request)
}
fn parse_copy(value: &Value) -> Result<Copy, Error> {
    let request: Copy = decode(value, "invalid cp arguments")?;
    lifecycle::pair(&request.from, &request.to, true)
        .map_err(|_| Error::Invalid("invalid or overlapping copy paths"))?;
    Ok(request)
}
fn parse_move(value: &Value) -> Result<Move, Error> {
    let request: Move = decode(value, "invalid mv arguments")?;
    lifecycle::exact_path(&request.from).map_err(|_| Error::Invalid("invalid move source path"))?;
    lifecycle::exact_path(&request.to)
        .map_err(|_| Error::Invalid("invalid move destination path"))?;
    // Moving one namespace entry to itself is a valid provable no-op and
    // returns `unchanged`; only a distinct destination inside a directory
    // source is structurally invalid.
    if request.from != request.to {
        lifecycle::pair(&request.from, &request.to, true)
            .map_err(|_| Error::Invalid("invalid or overlapping move paths"))?;
    }
    Ok(request)
}
fn parse_remove(value: &Value) -> Result<Remove, Error> {
    let request: Remove = decode(value, "invalid rm arguments")?;
    lifecycle::removals(request.paths.iter().map(String::as_str))
        .map_err(|_| Error::Invalid("invalid or overlapping removal paths"))?;
    Ok(request)
}
fn parse_bread(value: &Value, limits: Limits) -> Result<Bread, Error> {
    let request: Bread = decode(value, "invalid bread arguments")?;
    validate_path(&request.path)?;
    if request.format == BinaryFormat::Mcp && request.offset != 0 {
        return Err(Error::Invalid("format=mcp requires offset=0"));
    }
    if let Some(limit) = request.limit {
        let maximum = match request.format {
            BinaryFormat::Mcp => limits.schema.binary_read_bytes,
            BinaryFormat::Base64 | BinaryFormat::Hex => limits.schema.binary_encoded_read_bytes,
        };
        if limit == 0 || limit > maximum {
            return Err(Error::Invalid(
                "binary read limit exceeds configured ceiling",
            ));
        }
    }
    Ok(request)
}
fn parse_bwrite(value: &Value, limits: Limits) -> Result<Bwrite, Error> {
    let wire: WireBwrite = decode(value, "invalid bwrite arguments")?;
    validate_path(&wire.path)?;
    if wire.data.len() > limits.schema.binary_data_bytes {
        return Err(Error::Invalid(
            "encoded binary payload exceeds configured byte ceiling",
        ));
    }
    let data = wire.encoding.decode(&wire.data)?;
    if data.len() > limits.binary_decoded_bytes {
        return Err(Error::Invalid(
            "decoded binary payload exceeds configured byte ceiling",
        ));
    }
    Ok(Bwrite {
        path: wire.path,
        data,
        encoding: wire.encoding,
    })
}
fn parse_bedit(value: &Value, limits: Limits) -> Result<Bedit, Error> {
    let wire: WireBedit = decode(value, "invalid bedit arguments")?;
    validate_path(&wire.path)?;
    if wire.edits.is_empty() || wire.edits.len() > limits.schema.binary_edit_ranges {
        return Err(Error::Invalid(
            "bedit requires a bounded non-empty edit list",
        ));
    }
    if let Some(sha) = wire.sha256.as_deref() {
        if sha.len() != 64 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::Invalid(
                "sha256 must contain exactly 64 hexadecimal characters",
            ));
        }
    }
    let mut edits = Vec::with_capacity(wire.edits.len());
    let mut decoded_total = 0usize;
    for edit in wire.edits {
        if edit.data.len() > limits.schema.binary_data_bytes {
            return Err(Error::Invalid(
                "encoded binary edit payload exceeds configured byte ceiling",
            ));
        }
        let data = edit.encoding.decode(&edit.data)?;
        decoded_total = decoded_total
            .checked_add(data.len())
            .ok_or(Error::Invalid("decoded binary payload byte count overflow"))?;
        if decoded_total > limits.binary_decoded_bytes {
            return Err(Error::Invalid(
                "decoded binary edit payload exceeds configured byte ceiling",
            ));
        }
        edits.push(BinaryEdit {
            offset: edit.offset,
            length: edit.length,
            data,
        });
    }
    if edits.len() > MAX_EDIT_RANGES {
        return Err(Error::Invalid("too many binary edit ranges"));
    }
    Ok(Bedit {
        path: wire.path,
        edits,
        sha256: wire.sha256,
    })
}

impl Bedit {
    pub fn requested_splices(self) -> Vec<RequestedSplice> {
        self.edits
            .into_iter()
            .map(|edit| RequestedSplice {
                offset: edit.offset,
                length: edit.length,
                data: edit.data,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn access_paths_single_source_final_symlink_and_lifecycle_intent() {
        let read = parse(Tool::Read, &json!({"path":"a"}), Limits::default()).unwrap();
        assert_eq!(
            read.access_paths().unwrap(),
            [AccessPath {
                path: "a".into(),
                access: PathAccess::Read,
                follow_final: true,
            }]
        );
        let copy = parse(
            Tool::Copy,
            &json!({"from":"src","to":"dst"}),
            Limits::default(),
        )
        .unwrap();
        assert_eq!(
            copy.access_paths().unwrap(),
            [
                AccessPath {
                    path: "src".into(),
                    access: PathAccess::Read,
                    follow_final: false,
                },
                AccessPath {
                    path: "dst".into(),
                    access: PathAccess::Write,
                    follow_final: false,
                },
            ]
        );
        let patch = parse(
            Tool::Patch,
            &json!({"patch":"*** Begin Patch\n*** Update File: old.txt\n@@\n-old\n+new\n*** Add File: new.txt\n+new\n*** End Patch\n"}),
            Limits::default(),
        )
        .unwrap();
        assert_eq!(
            patch.access_paths().unwrap(),
            [
                AccessPath {
                    path: "old.txt".into(),
                    access: PathAccess::ReadWrite,
                    follow_final: true,
                },
                AccessPath {
                    path: "new.txt".into(),
                    access: PathAccess::Write,
                    follow_final: false,
                },
            ]
        );
    }

    #[test]
    fn canonical_defaults_and_unknown_fields_are_shared() {
        let request = parse(Tool::Read, &json!({"path":"a.txt"}), Limits::default()).unwrap();
        let RequestKind::Read(read) = request.kind() else {
            panic!("read request");
        };
        assert_eq!(
            (read.line, read.lines, read.byte, read.numbered),
            (1, 4096, 0, true)
        );
        assert!(parse(
            Tool::Read,
            &json!({"path":"a.txt","legacy":true}),
            Limits::default(),
        )
        .is_err());

        let request = parse(Tool::Grep, &json!({"pattern":"needle"}), Limits::default()).unwrap();
        let RequestKind::Grep(grep) = request.kind() else {
            panic!("grep request");
        };
        assert_eq!(grep.context, 0);
        assert_eq!(grep.limit, 200);
        assert_eq!(grep.path, ".");
    }

    #[test]
    fn canonical_edit_patch_and_lifecycle_validation_is_single_sourced() {
        let limits = Limits {
            edit_text_bytes: 12,
            ..Limits::default()
        };
        assert!(parse(
            Tool::Edit,
            &json!({"path":"a","edits":[["one","two"],["x","y",true]]}),
            limits,
        )
        .is_ok());
        assert!(parse(
            Tool::Edit,
            &json!({"path":"a","edits":[["123456","789012"]]}),
            Limits {
                edit_text_bytes: 8,
                ..Limits::default()
            },
        )
        .is_err());
        assert!(parse(
            Tool::Patch,
            &json!({"patch":"*** Begin Patch\n*** Add File: new.txt\n+hello\n*** End Patch\n"}),
            Limits::default(),
        )
        .is_ok());
        assert!(parse(
            Tool::Copy,
            &json!({"from":"dir","to":"dir/child"}),
            Limits::default(),
        )
        .is_err());
        assert!(parse(
            Tool::Move,
            &json!({"from":"same.txt","to":"same.txt"}),
            Limits::default(),
        )
        .is_ok());
        assert!(parse(
            Tool::Move,
            &json!({"from":"dir","to":"dir/child"}),
            Limits::default(),
        )
        .is_err());
        assert!(parse(
            Tool::Remove,
            &json!({"paths":["a","a/b"],"recursive":true}),
            Limits::default(),
        )
        .is_err());
    }

    #[test]
    fn grep_validation_is_independent_of_binary_read_ceiling() {
        let limits = Limits {
            schema: SchemaCaps {
                binary_read_bytes: 1,
                binary_encoded_read_bytes: 1,
                ..SchemaCaps::default()
            },
            ..Limits::default()
        };
        assert!(parse(Tool::Grep, &json!({"pattern":"needle","limit":200}), limits,).is_ok());
    }

    #[test]
    fn binary_wire_validation_decodes_once_and_preserves_omitted_bread_limit() {
        let limits = Limits {
            schema: SchemaCaps {
                binary_data_bytes: 16,
                binary_read_bytes: 64,
                binary_encoded_read_bytes: 24,
                ..SchemaCaps::default()
            },
            ..Limits::default()
        };
        let request = parse(
            Tool::Bread,
            &json!({"path":"a.bin","format":"base64"}),
            limits,
        )
        .unwrap();
        let RequestKind::Bread(bread) = request.kind() else {
            panic!("bread request");
        };
        assert_eq!(bread.limit, None);
        assert!(parse(
            Tool::Bread,
            &json!({"path":"a.bin","format":"hex","limit":25}),
            limits,
        )
        .is_err());

        let request = parse(
            Tool::Bwrite,
            &json!({"path":"a.bin","data":"AQID","encoding":"base64"}),
            limits,
        )
        .unwrap();
        let RequestKind::Bwrite(write) = request.kind() else {
            panic!("bwrite request");
        };
        assert_eq!(write.data, [1, 2, 3]);

        let request = parse(
            Tool::Bedit,
            &json!({"path":"a.bin","edits":[{"offset":0,"length":0,"data":"ff","encoding":"hex"}]}),
            limits,
        )
        .unwrap();
        let RequestKind::Bedit(edit) = request.kind() else {
            panic!("bedit request");
        };
        assert_eq!(edit.edits[0].data, [0xff]);
    }

    #[test]
    fn decoded_binary_payload_has_independent_aggregate_ceiling() {
        let limits = Limits {
            schema: SchemaCaps {
                binary_data_bytes: 32,
                ..SchemaCaps::default()
            },
            binary_decoded_bytes: 4,
            ..Limits::default()
        };
        assert!(parse(
            Tool::Bwrite,
            &json!({"path":"a.bin","data":"01020304","encoding":"hex"}),
            limits,
        )
        .is_ok());
        assert!(parse(
            Tool::Bwrite,
            &json!({"path":"a.bin","data":"0102030405","encoding":"hex"}),
            limits,
        )
        .is_err());
        assert!(parse(
            Tool::Bedit,
            &json!({"path":"a.bin","edits":[
                {"offset":0,"length":0,"data":"0102","encoding":"hex"},
                {"offset":0,"length":0,"data":"0304","encoding":"hex"}
            ]}),
            limits,
        )
        .is_ok());
        assert!(parse(
            Tool::Bedit,
            &json!({"path":"a.bin","edits":[
                {"offset":0,"length":0,"data":"0102","encoding":"hex"},
                {"offset":0,"length":0,"data":"030405","encoding":"hex"}
            ]}),
            limits,
        )
        .is_err());
    }

    #[test]
    fn narrowed_read_ceiling_also_narrows_omitted_default() {
        let limits = Limits {
            schema: SchemaCaps {
                read_line_limit: 8,
                ..SchemaCaps::default()
            },
            ..Limits::default()
        };
        let request = parse(Tool::Read, &json!({"path":"a.txt"}), limits).unwrap();
        let RequestKind::Read(read) = request.kind() else {
            panic!("read request");
        };
        assert_eq!(read.lines, 8);
    }

    #[test]
    fn configured_schema_caps_drive_parser_acceptance() {
        let limits = Limits {
            schema: SchemaCaps {
                text_write_bytes: 4,
                patch_bytes: 64,
                read_line_limit: 3,
                ..SchemaCaps::default()
            },
            ..Limits::default()
        };
        assert!(parse(Tool::Write, &json!({"path":"a","content":"1234"}), limits).is_ok());
        assert!(parse(Tool::Write, &json!({"path":"a","content":"12345"}), limits).is_err());
        assert!(parse(Tool::Read, &json!({"path":"a","lines":4}), limits).is_err());
    }
}
