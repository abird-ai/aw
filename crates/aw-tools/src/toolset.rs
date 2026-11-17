//! Validated, immutable catalog configuration. No host authority, model,
//! transport, clock, scheduler or execution state lives here.
use crate::{
    filesystem::{
        access::Action,
        schema::{self, SchemaCaps, Tool},
    },
    operation::OperationId,
    search::{GrepLimits, MAX_GREP_CONTEXT},
    ToolEffect,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Toolset-owned grep defaults and bounded search work.
#[derive(Clone, Copy, Debug)]
pub struct GrepProfile {
    pub default_context: usize,
    pub max_context: usize,
    pub limits: GrepLimits,
}
impl Default for GrepProfile {
    fn default() -> Self {
        Self {
            default_context: 0,
            max_context: MAX_GREP_CONTEXT,
            limits: GrepLimits::default(),
        }
    }
}

/// Limits for compact Job interfaces; a host may choose stricter defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobProfile {
    pub get_tail_default: usize,
    pub get_tail_max: usize,
    pub get_wait_max_ms: u64,
    pub read_wait_max_ms: u64,
    pub list_page_max: usize,
}
impl Default for JobProfile {
    fn default() -> Self {
        Self {
            get_tail_default: 512,
            get_tail_max: 1024,
            get_wait_max_ms: 30_000,
            read_wait_max_ms: 30_000,
            list_page_max: 256,
        }
    }
}
pub const MAX_GET_TAIL_BYTES: usize = 64 * 1024;
pub const MAX_JOB_WAIT_MS: u64 = 300_000;
pub const MAX_JOB_LIST_PAGE: usize = 256;

/// Optional *model receipt* metadata, never inserted into raw output pages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReceiptProfile {
    pub unix_ts: bool,
    /// Host-selected advisory turn checkpoint; never a process/Job deadline.
    pub checkpoint_minutes: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolsetError {
    Invalid(&'static str),
    MissingTimestamp,
    ResultLimit,
}

impl std::fmt::Display for ToolsetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => f.write_str(message),
            Self::MissingTimestamp => f.write_str("timestamped receipt requires host timestamp"),
            Self::ResultLimit => f.write_str("compact receipt exceeds configured result budget"),
        }
    }
}
impl std::error::Error for ToolsetError {}

/// One model-facing descriptor, including host-independent effect and title.
/// The actual host policy chooses which capabilities are registered.
/// Metadata-only per-operation overrides. Never changes tool names,
/// effects, schema shapes, execution or authority.
#[derive(Clone, Debug, Default)]
pub struct ToolText {
    title: Option<String>,
    description: Option<String>,
    suffixes: Vec<String>,
    arguments: BTreeMap<String, String>,
}
impl ToolText {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn title(mut self, text: impl Into<String>) -> Self {
        self.title = Some(text.into());
        self
    }
    /// Replace the canonical top-level prose; suffix is preferable when only
    /// adding a host-specific hint.
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }
    pub fn description_suffix(mut self, text: impl Into<String>) -> Self {
        self.suffixes.push(text.into());
        self
    }
    /// Root argument name or a JSON Pointer to a nested schema *property*
    /// object (not the description leaf). Missing fields fail closed.
    pub fn argument(mut self, path: impl Into<String>, text: impl Into<String>) -> Self {
        self.arguments.insert(path.into(), text.into());
        self
    }
    fn merge(&mut self, other: Self) {
        if other.title.is_some() {
            self.title = other.title;
        }
        if other.description.is_some() {
            self.description = other.description;
        }
        self.suffixes.extend(other.suffixes);
        self.arguments.extend(other.arguments);
    }
    fn compile(&mut self, schema: SchemaCaps, jobs: JobProfile) -> Result<(), ToolsetError> {
        if let Some(title) = self.title.as_mut() {
            *title = expand_text(title, schema, jobs)?;
            validate_text(title, 128)?;
        }
        if let Some(description) = self.description.as_mut() {
            *description = expand_text(description, schema, jobs)?;
            validate_text(description, 4096)?;
        }
        for suffix in &mut self.suffixes {
            *suffix = expand_text(suffix, schema, jobs)?;
            validate_text(suffix, 2048)?;
        }
        for (path, description) in &mut self.arguments {
            if path.is_empty() || path.len() > 512 || path.chars().any(char::is_control) {
                return Err(ToolsetError::Invalid("invalid argument description path"));
            }
            *description = expand_text(description, schema, jobs)?;
            validate_text(description, 2048)?;
        }
        let text_bytes = self.title.as_ref().map_or(0, String::len)
            + self.description.as_ref().map_or(0, String::len)
            + self.suffixes.iter().map(String::len).sum::<usize>()
            + self
                .arguments
                .iter()
                .map(|(key, value)| key.len() + value.len())
                .sum::<usize>();
        if self.suffixes.len() > 16 || self.arguments.len() > 128 || text_bytes > 16 * 1024 {
            return Err(ToolsetError::Invalid(
                "tool metadata exceeds bounded catalog limits",
            ));
        }
        Ok(())
    }
}

fn validate_text(text: &str, max: usize) -> Result<(), ToolsetError> {
    if text.trim().is_empty() || text.len() > max || text.chars().any(char::is_control) {
        return Err(ToolsetError::Invalid(
            "tool metadata must be nonempty, bounded and single-line",
        ));
    }
    Ok(())
}

/// Resolve only known, typed configuration variables. No arbitrary templating
/// callbacks or independent profile values are retained.
fn expand_text(input: &str, schema: SchemaCaps, jobs: JobProfile) -> Result<String, ToolsetError> {
    let vars = [
        ("${grep.max_context}", schema.grep_context_max.to_string()),
        (
            "${grep.default_context}",
            schema.grep_context_default.to_string(),
        ),
        (
            "${jobs.get_tail_default}",
            jobs.get_tail_default.to_string(),
        ),
        ("${jobs.get_tail_max}", jobs.get_tail_max.to_string()),
        ("${jobs.get_wait_max_ms}", jobs.get_wait_max_ms.to_string()),
        (
            "${jobs.read_wait_max_ms}",
            jobs.read_wait_max_ms.to_string(),
        ),
        ("${jobs.list_page_max}", jobs.list_page_max.to_string()),
    ];
    let mut resolved = input.to_owned();
    for (token, value) in vars {
        if resolved.contains(token) {
            resolved = resolved.replace(token, &value);
        }
    }
    if resolved.contains("${") {
        return Err(ToolsetError::Invalid("unknown tool description variable"));
    }
    Ok(resolved)
}

fn argument_pointer(path: &str) -> String {
    if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/properties/{}", path.replace('~', "~0").replace('/', "~1"))
    }
}
fn argument_property<'a>(
    parameters: &'a mut Value,
    path: &str,
) -> Result<&'a mut serde_json::Map<String, Value>, ToolsetError> {
    let pointer = argument_pointer(path);
    if !pointer.starts_with("/properties/") {
        return Err(ToolsetError::Invalid(
            "argument path must point to a property",
        ));
    }
    let value = parameters
        .pointer_mut(&pointer)
        .and_then(Value::as_object_mut)
        .ok_or(ToolsetError::Invalid(
            "unknown tool argument description target",
        ))?;
    if !value.contains_key("type")
        && !value.contains_key("oneOf")
        && !value.contains_key("$ref")
        && !value.contains_key("const")
        && !value.contains_key("allOf")
    {
        return Err(ToolsetError::Invalid(
            "argument description target is not a schema property",
        ));
    }
    Ok(value)
}
fn apply_text_to_descriptor(
    text: &ToolText,
    title: &mut String,
    description: &mut String,
    parameters: &mut Value,
) -> Result<(), ToolsetError> {
    if let Some(value) = &text.title {
        title.clone_from(value);
    }
    if let Some(value) = &text.description {
        description.clone_from(value);
    }
    for suffix in &text.suffixes {
        if !description.is_empty() {
            description.push(' ');
        }
        description.push_str(suffix);
    }
    for (path, doc) in &text.arguments {
        argument_property(parameters, path)?
            .insert("description".into(), Value::String(doc.clone()));
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct Descriptor {
    pub name: &'static str,
    pub title: String,
    pub description: String,
    pub parameters: Value,
    pub effect: ToolEffect,
}

#[derive(Clone, Debug)]
pub struct Toolset {
    schema: SchemaCaps,
    grep: GrepProfile,
    jobs: JobProfile,
    receipts: ReceiptProfile,
    enabled: Vec<Tool>,
    texts: BTreeMap<OperationId, ToolText>,
}
#[derive(Clone, Debug)]
pub struct ToolsetBuilder {
    schema: SchemaCaps,
    grep: Option<GrepProfile>,
    jobs: JobProfile,
    receipts: ReceiptProfile,
    enabled: Vec<Tool>,
    texts: BTreeMap<OperationId, ToolText>,
}
impl Default for ToolsetBuilder {
    fn default() -> Self {
        Self {
            schema: SchemaCaps::default(),
            grep: None,
            jobs: JobProfile::default(),
            receipts: ReceiptProfile::default(),
            enabled: Tool::ALL.to_vec(),
            texts: BTreeMap::new(),
        }
    }
}
impl ToolsetBuilder {
    /// Configured schema ceilings. Grep defaults may be overridden separately.
    pub fn filesystem(mut self, caps: SchemaCaps) -> Self {
        self.schema = caps;
        self
    }
    pub fn grep(mut self, profile: GrepProfile) -> Self {
        self.grep = Some(profile);
        self
    }
    pub fn jobs(mut self, profile: JobProfile) -> Self {
        self.jobs = profile;
        self
    }
    pub fn receipts(mut self, profile: ReceiptProfile) -> Self {
        self.receipts = profile;
        self
    }
    /// Extend or replace the text of one canonical operation. Repeated calls
    /// merge; the most recent title/description/argument wins, hints append.
    pub fn tool_text(mut self, op: OperationId, text: ToolText) -> Self {
        self.texts.entry(op).or_default().merge(text);
        self
    }
    /// Select *available definitions*, not grants. Empty is a valid catalog.
    pub fn only(mut self, tools: impl IntoIterator<Item = Tool>) -> Self {
        self.enabled = tools.into_iter().collect();
        self
    }
    pub fn build(self) -> Result<Toolset, ToolsetError> {
        let mut schema = self.schema;
        let mut grep = self.grep.unwrap_or_else(|| GrepProfile {
            default_context: schema.grep_context_default,
            max_context: schema.grep_context_max,
            ..GrepProfile::default()
        });
        schema.grep_context_default = grep.default_context;
        schema.grep_context_max = grep.max_context;
        if grep.max_context > MAX_GREP_CONTEXT || grep.default_context > grep.max_context {
            return Err(ToolsetError::Invalid("invalid grep context profile"));
        }
        grep.limits.context_max = grep.max_context;
        if self.jobs.get_tail_default > self.jobs.get_tail_max
            || self.jobs.get_tail_max > MAX_GET_TAIL_BYTES
            || self.jobs.get_wait_max_ms > MAX_JOB_WAIT_MS
            || self.jobs.read_wait_max_ms > MAX_JOB_WAIT_MS
            || self.jobs.list_page_max == 0
            || self.jobs.list_page_max > MAX_JOB_LIST_PAGE
        {
            return Err(ToolsetError::Invalid("invalid Job profile"));
        }
        if self
            .receipts
            .checkpoint_minutes
            .is_some_and(|min| min == 0 || min > 240)
            || (self.receipts.checkpoint_minutes.is_some() && !self.receipts.unix_ts)
        {
            return Err(ToolsetError::Invalid(
                "turn guidance requires a timestamped receipt",
            ));
        }
        let mut enabled = Vec::with_capacity(self.enabled.len());
        for tool in self.enabled {
            if enabled.contains(&tool) {
                return Err(ToolsetError::Invalid("duplicate tool"));
            }
            schema.validate_for(tool).map_err(ToolsetError::Invalid)?;
            enabled.push(tool);
        }
        // Bound work without requiring an enabled grep tool.
        if grep.limits.candidates == 0
            || grep.limits.scan_bytes == 0
            || grep.limits.scan_files == 0
            || grep.limits.text_file_bytes == 0
            || grep.limits.output_bytes == 0
            || grep.limits.duration.is_zero()
        {
            return Err(ToolsetError::Invalid("invalid grep work limits"));
        }
        let mut texts = self.texts;
        for (op, text) in &mut texts {
            text.compile(schema, self.jobs)?;
            if let OperationId::Fs(tool) = op {
                let mut parameters = schema::spec(*tool, schema).parameters;
                for path in text.arguments.keys() {
                    let _ = argument_property(&mut parameters, path)?;
                }
            }
        }
        Ok(Toolset {
            schema,
            grep,
            jobs: self.jobs,
            receipts: self.receipts,
            enabled,
            texts,
        })
    }
}

impl Toolset {
    pub fn builder() -> ToolsetBuilder {
        ToolsetBuilder::default()
    }
    pub fn schema_caps(&self) -> SchemaCaps {
        self.schema
    }
    pub fn grep_profile(&self) -> GrepProfile {
        self.grep
    }
    pub fn jobs_profile(&self) -> JobProfile {
        self.jobs
    }
    pub fn receipt_profile(&self) -> ReceiptProfile {
        self.receipts
    }
    pub fn tools(&self) -> &[Tool] {
        &self.enabled
    }
    pub fn grep_limits(&self) -> GrepLimits {
        self.grep.limits
    }
    pub fn has_text_override(&self, op: OperationId) -> bool {
        self.texts.contains_key(&op)
    }
    /// One metadata overlay shared by model/SDK adapters. Only the original
    /// description and documented parameter text can change; names/effects/
    /// shapes/limits are never touched.
    pub fn override_descriptions(
        &self,
        op: OperationId,
        title: &mut String,
        description: &mut String,
        parameters: &mut Value,
    ) -> Result<(), ToolsetError> {
        if let Some(text) = self.texts.get(&op) {
            apply_text_to_descriptor(text, title, description, parameters)?;
        }
        Ok(())
    }

    #[cfg(feature = "rw")]
    pub fn parse_filesystem(
        &self,
        tool: Tool,
        arguments: &Value,
        mut limits: crate::filesystem::request::Limits,
    ) -> Result<crate::filesystem::request::Request, crate::filesystem::request::Error> {
        if !self.enabled.contains(&tool) {
            return Err(crate::filesystem::request::Error::Invalid(
                "tool is disabled",
            ));
        }
        limits.schema = self.schema;
        crate::filesystem::request::parse(tool, arguments, limits)
    }
    /// Canonical filesystem metadata from the same limits the request parser
    /// and secured executor accept. Never grants filesystem authority.
    pub fn filesystem_descriptor(&self, tool: Tool) -> Option<Descriptor> {
        self.enabled.contains(&tool).then(|| {
            let mut descriptor = Self::describe_filesystem(tool, self.schema);
            self.override_descriptions(
                OperationId::Fs(tool),
                &mut descriptor.title,
                &mut descriptor.description,
                &mut descriptor.parameters,
            )
            .expect("filesystem field overrides were validated at build");
            descriptor
        })
    }

    /// Metadata-only projection; tools not enabled yet can be described before
    /// host admission. Actual registration MUST validate the selected tool.
    pub fn describe_filesystem(tool: Tool, caps: SchemaCaps) -> Descriptor {
        let schema = schema::spec(tool, caps);
        let mut parameters = schema.parameters;
        describe_properties(&mut parameters, tool, caps);
        let description = match tool {
            Tool::Grep => format!(
                "Search an exact file or recursive directory, by pattern/glob; context/before/after at most {} lines per side. mode=content/files/count; bounded paged output.",
                caps.grep_context_max,
            ),
            _ => schema.description,
        };
        Descriptor {
            name: schema.name,
            title: operation_title(OperationId::Fs(tool)).to_owned(),
            description,
            parameters,
            effect: match schema.action {
                Action::Read => ToolEffect::Read,
                Action::Shell => ToolEffect::Process,
                _ => ToolEffect::Write,
            },
        }
    }
    /// Original host-owned schema for a non-filesystem operation is preserved.
    /// The factory supplies canonical titles, truthful config-dependent text,
    /// and tight advertised common limits, never a new host authority layer.
    pub fn describe_operation(
        &self,
        op: OperationId,
        mut parameters: Value,
        host_description: &str,
    ) -> Result<Descriptor, ToolsetError> {
        if let OperationId::Fs(tool) = op {
            return self
                .filesystem_descriptor(tool)
                .ok_or(ToolsetError::Invalid("filesystem tool is disabled"));
        }
        if !parameters.is_object() {
            return Err(ToolsetError::Invalid("tool parameters must be an object"));
        }
        let description = match op {
            OperationId::JobsGet => {
                set_bound(
                    &mut parameters,
                    "tail",
                    self.jobs.get_tail_max,
                    Some(self.jobs.get_tail_default),
                );
                set_bound(
                    &mut parameters,
                    "wait_ms",
                    self.jobs.get_wait_max_ms as usize,
                    None,
                );
                format!("{host_description} Process tail defaults to {} raw bytes combined across stdout/stderr (maximum {}, tail=0 disables it).",
                    self.jobs.get_tail_default, self.jobs.get_tail_max)
            }
            OperationId::JobsRead => {
                set_bound(
                    &mut parameters,
                    "wait_ms",
                    self.jobs.read_wait_max_ms as usize,
                    None,
                );
                format!("{host_description} Signed retained-byte offsets use -N for an immediate end snapshot; wait_ms=0 is nonblocking.")
            }
            OperationId::JobsList => {
                set_bound(&mut parameters, "limit", self.jobs.list_page_max, None);
                set_minimum(&mut parameters, "limit", 1);
                host_description.to_owned()
            }
            _ => host_description.to_owned(),
        };
        let mut description = description;
        if self.receipts.unix_ts
            && matches!(
                op,
                OperationId::Bash
                    | OperationId::Pwsh
                    | OperationId::Powershell
                    | OperationId::Batch
                    | OperationId::JobsList
                    | OperationId::JobsGet
                    | OperationId::JobsAck
                    | OperationId::JobsCancel
            )
        {
            description.push_str(
                " Compact receipts include ts (UTC Unix seconds) when the host supplies it.",
            );
        }
        let mut descriptor = Descriptor {
            name: op.model_name(),
            title: operation_title(op).to_owned(),
            description,
            parameters,
            effect: match op {
                OperationId::JobsList | OperationId::JobsGet | OperationId::JobsRead => {
                    ToolEffect::Read
                }
                OperationId::JobsAck
                | OperationId::JobsSend
                | OperationId::JobsCancel
                | OperationId::JobsSubscribe
                | OperationId::Batch => ToolEffect::Control,
                OperationId::Bash
                | OperationId::Pwsh
                | OperationId::Powershell
                | OperationId::Agent => ToolEffect::Process,
                OperationId::Fs(_) => unreachable!("handled above"),
            },
        };
        self.override_descriptions(
            op,
            &mut descriptor.title,
            &mut descriptor.description,
            &mut descriptor.parameters,
        )?;
        if descriptor.description.trim().is_empty() {
            return Err(ToolsetError::Invalid("tool description is empty"));
        }
        Ok(descriptor)
    }
    /// Host can pass these instructions to its model. No timer is scheduled.
    pub fn model_instructions(&self) -> Option<String> {
        let minutes = self.receipts.checkpoint_minutes?;
        Some(format!(
            "Compact shell/Job receipts include ts=UTC Unix seconds. Use the first ts in a user turn as its anchor; aim to checkpoint after about {minutes} minutes at a coherent boundary, recording what is done and pending. This is advisory, not an execution timeout."
        ))
    }
    /// Render a *typed* factual receipt, never arbitrary raw tool output.
    /// The host supplies UTC epoch seconds; the factory has no clock.
    pub fn render_receipt(
        &self,
        receipt: crate::receipt::Receipt<'_>,
        now_unix: Option<u64>,
        budget: usize,
    ) -> Result<String, ToolsetError> {
        let footer = if self.receipts.unix_ts {
            Some(format!(
                "ts={}",
                now_unix.ok_or(ToolsetError::MissingTimestamp)?
            ))
        } else {
            None
        };
        let reserved = footer.as_ref().map_or(0, |value| value.len() + 1);
        let body_budget = budget
            .checked_sub(reserved)
            .ok_or(ToolsetError::ResultLimit)?;
        let mut content = crate::receipt::render_plain(receipt, body_budget)?;
        if let Some(footer) = footer {
            content.push('\n');
            content.push_str(&footer);
        }
        Ok(content)
    }
}

fn set_bound(parameters: &mut Value, name: &str, max: usize, default: Option<usize>) {
    if let Some(property) = parameters
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .and_then(|properties| properties.get_mut(name))
        .and_then(Value::as_object_mut)
    {
        let effective = property
            .get("maximum")
            .and_then(Value::as_u64)
            .map_or(max, |existing| {
                max.min(usize::try_from(existing).unwrap_or(usize::MAX))
            });
        property.insert("maximum".into(), json!(effective));
        if let Some(default) = default {
            property.insert("default".into(), json!(default.min(effective)));
        }
    }
}

fn set_minimum(parameters: &mut Value, name: &str, minimum: usize) {
    if let Some(property) = parameters
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .and_then(|properties| properties.get_mut(name))
        .and_then(Value::as_object_mut)
    {
        let effective = property
            .get("minimum")
            .and_then(Value::as_u64)
            .map_or(minimum, |existing| {
                minimum.max(usize::try_from(existing).unwrap_or(usize::MAX))
            });
        property.insert("minimum".into(), json!(effective));
    }
}

pub const fn operation_title(op: OperationId) -> &'static str {
    match op {
        OperationId::Fs(Tool::Ls) => "List directory",
        OperationId::Fs(Tool::Find) => "Find paths",
        OperationId::Fs(Tool::Grep) => "Search file content",
        OperationId::Fs(Tool::Read) => "Read text",
        OperationId::Fs(Tool::Write) => "Write text",
        OperationId::Fs(Tool::Edit) => "Edit text",
        OperationId::Fs(Tool::Patch) => "Apply patch",
        OperationId::Fs(Tool::Copy) => "Copy path",
        OperationId::Fs(Tool::Move) => "Move path",
        OperationId::Fs(Tool::Remove) => "Remove path",
        OperationId::Fs(Tool::Bread) => "Read binary",
        OperationId::Fs(Tool::Bwrite) => "Write binary",
        OperationId::Fs(Tool::Bedit) => "Edit binary",
        OperationId::Bash => "Run Bash",
        OperationId::Pwsh | OperationId::Powershell => "Run PowerShell",
        OperationId::Batch => "Run tool Graph",
        OperationId::Agent => "Start Agent",
        OperationId::JobsList => "List Jobs",
        OperationId::JobsGet => "Get Job status",
        OperationId::JobsRead => "Read Job output",
        OperationId::JobsAck => "Release Job output",
        OperationId::JobsSend => "Send Job input",
        OperationId::JobsCancel => "Cancel Job",
        OperationId::JobsSubscribe => "Subscribe to Job events",
    }
}

// Descriptions derive from the existing canonical field inventory, not a
// second input schema/decoder. Recursion covers nested edits and $defs ranges.
fn describe_properties(value: &mut Value, tool: Tool, caps: SchemaCaps) {
    match value {
        Value::Object(map) => {
            if let Some(properties) = map.get_mut("properties").and_then(Value::as_object_mut) {
                for (name, property) in properties {
                    if property.get("description").is_none() {
                        let doc = property_description(tool, name, caps);
                        if let Some(property) = property.as_object_mut() {
                            property.insert("description".into(), Value::String(doc));
                        }
                    }
                }
            }
            for child in map.values_mut() {
                describe_properties(child, tool, caps);
            }
        }
        Value::Array(items) => {
            for item in items {
                describe_properties(item, tool, caps);
            }
        }
        _ => {}
    }
}

fn property_description(tool: Tool, name: &str, caps: SchemaCaps) -> String {
    match (tool, name) {
        (Tool::Grep, "pattern") => "Regular expression, or literal text with literal=true.".into(),
        (Tool::Grep, "context") => format!("Default context for both sides, 0..={}; each explicit side overrides it.", caps.grep_context_max),
        (Tool::Grep, "before" | "after") => format!("Context lines on this side, 0..={}; overrides symmetric context.", caps.grep_context_max),
        (Tool::Grep, "mode") => "content=matching lines, files=matching paths, count=per-file hit counts.".into(),
        (Tool::Read, "line") => "Starting 1-based line number (default 1).".into(),
        (Tool::Read, "lines") => format!("Maximum lines to read, at most {}.", caps.read_line_limit),
        (Tool::Read, "byte") => "Exact byte continuation within the first selected line.".into(),
        (Tool::Bread, "format") => "mcp complete typed media, or bounded base64/hex byte windows.".into(),
        (Tool::Bread, "limit") => format!("Raw bytes to read; maximum {} (encoded windows may be tighter).", caps.binary_read_bytes),
        (Tool::Write | Tool::Bwrite, "content" | "data") => "Content to write atomically, within the configured payload ceiling.".into(),
        (Tool::Edit | Tool::Bedit, "edits") => "Bounded exact, original-relative replacement coordinates; no implicit best-effort writes.".into(),
        (Tool::Copy | Tool::Move, "from") => "Exact source path, subject to current host authority.".into(),
        (Tool::Copy | Tool::Move, "to") => "Exact destination path; never silently overwrite an existing entry.".into(),
        (Tool::Remove, "paths") => "Exact paths, no glob expansion; all are preflighted before mutation.".into(),
        (Tool::Remove, "recursive") => "Allow removing non-empty directories.".into(),
        (_, "path") => "File or directory relative to the host's authorized root.".into(),
        (_, "glob" | "regex") => "Optional pathname or content discovery filter.".into(),
        (_, "depth") => "Maximum directory traversal depth.".into(),
        (_, "offset") => "Absolute continuation or entry offset in this operation's units.".into(),
        (_, "limit") => "Maximum results or records; capped by the advertised schema.".into(),
        (_, "meta") => "Optional metadata columns; defaults to compact kind/path.".into(),
        (_, "sort") => "Result ordering, deterministic within a page.".into(),
        (_, "kind") => "Entry kind selector.".into(),
        (_, "before" | "after") => "ISO-8601 timestamp bound.".into(),
        (_, "hidden") => "Include normally hidden entries when permitted.".into(),
        (_, "gitignore") => "Apply .gitignore rules during discovery.".into(),
        (_, "literal") => "Match literal text instead of a regular expression.".into(),
        (_, "ignore_case") => "Match without case sensitivity.".into(),
        (_, "numbered") => "Prefix content lines with line numbers.".into(),
        (_, "parallel") => "Host scheduling hint; never changes access grants.".into(),
        (_, "btime" | "ctime" | "mtime") => "Filter entries by creation/change/modification time.".into(),
        (_, "encoding") => "Base64 or hex for byte input/output.".into(),
        (_, "sha256") => "Optional exact original-content hash precondition.".into(),
        (_, "length") => "Byte count covered by this replacement.".into(),
        (_, "old_text") => "Exact text to find in the original snapshot.".into(),
        (_, "new_text") => "Replacement text (may be empty).".into(),
        _ => "Operation-specific bounded input; unknown fields are rejected.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filesystem::schema::Tool;

    #[test]
    fn configured_profiles_drive_catalog_defaults_and_limits() {
        let compact = Toolset::builder()
            .grep(GrepProfile {
                default_context: 2,
                max_context: 8,
                ..Default::default()
            })
            .jobs(JobProfile {
                get_tail_default: 256,
                get_tail_max: 800,
                ..Default::default()
            })
            .only([Tool::Grep, Tool::Read])
            .build()
            .unwrap();
        assert_eq!(compact.tools(), &[Tool::Grep, Tool::Read]);
        assert!(compact.filesystem_descriptor(Tool::Edit).is_none());
        let grep = compact.filesystem_descriptor(Tool::Grep).unwrap();
        assert_eq!(grep.title, "Search file content");
        assert_eq!(grep.parameters["properties"]["context"]["default"], 2);
        assert_eq!(grep.parameters["properties"]["before"]["maximum"], 8);
        assert!(grep.description.contains("8 lines"));
        assert_eq!(compact.schema_caps().grep_context_default, 2);
        assert_eq!(compact.grep_profile().limits.context_max, 8);
        let get = compact
            .describe_operation(
                OperationId::JobsGet,
                json!({"type":"object","properties":{
                    "tail":{"type":"integer"},"wait_ms":{"type":"integer"}
                }}),
                "",
            )
            .unwrap();
        assert_eq!(get.parameters["properties"]["tail"]["default"], 256);
        assert_eq!(get.parameters["properties"]["tail"]["maximum"], 800);
        assert!(get.description.contains("256 raw bytes combined"));
        let list = compact
            .describe_operation(
                OperationId::JobsList,
                json!({"type":"object","properties":{
                    "limit":{"type":"integer","minimum":0,"maximum":4096}
                }}),
                "List recent Jobs.",
            )
            .unwrap();
        assert_eq!(list.parameters["properties"]["limit"]["minimum"], 1);
        assert_eq!(list.parameters["properties"]["limit"]["maximum"], 256);
        let all = Toolset::builder().build().unwrap();
        assert_eq!(all.tools().len(), 13);
        assert_eq!(all.jobs_profile().get_tail_default, 512);
    }

    #[test]
    fn metadata_hooks_merge_and_expand_from_validated_profiles_without_mutating_schema() {
        let op = OperationId::Fs(Tool::Grep);
        let toolset = Toolset::builder()
            .grep(GrepProfile {
                max_context: 8,
                ..Default::default()
            })
            .tool_text(
                op,
                ToolText::new()
                    .title("Search source")
                    .description("Search source code.")
                    .description_suffix("Up to ${grep.max_context} context lines.")
                    .argument("pattern", "Expression or literal.")
                    .argument("context", "At most ${grep.max_context} lines per side."),
            )
            .tool_text(
                op,
                ToolText::new()
                    .title("Search files")
                    .description_suffix("Use read for full context.")
                    .argument("pattern", "Match text or regex."),
            )
            .only([Tool::Grep])
            .build()
            .unwrap();
        let descriptor = toolset.filesystem_descriptor(Tool::Grep).unwrap();
        assert_eq!(descriptor.title, "Search files");
        assert_eq!(
            descriptor.description,
            "Search source code. Up to 8 context lines. Use read for full context."
        );
        assert_eq!(
            descriptor.parameters["properties"]["pattern"]["description"],
            "Match text or regex."
        );
        assert_eq!(
            descriptor.parameters["properties"]["context"]["description"],
            "At most 8 lines per side."
        );
        assert_eq!(descriptor.name, "grep");
        assert_eq!(descriptor.effect, ToolEffect::Read);
        assert_eq!(descriptor.parameters["properties"]["context"]["maximum"], 8);
        assert!(descriptor.parameters["properties"]
            .get("invented")
            .is_none());
        assert!(toolset.has_text_override(op));
        assert!(!toolset.has_text_override(OperationId::Fs(Tool::Read)));
    }

    #[test]
    fn nested_argument_overrides_are_description_only_and_unknown_targets_fail() {
        let op = OperationId::Fs(Tool::Edit);
        let nested = "/properties/edits/items/prefixItems/0";
        let tools = Toolset::builder()
            .tool_text(
                op,
                ToolText::new().argument(nested, "Find this exact old-text value."),
            )
            .build()
            .unwrap();
        let descriptor = tools.filesystem_descriptor(Tool::Edit).unwrap();
        assert_eq!(
            descriptor
                .parameters
                .pointer(&format!("{nested}/description"))
                .unwrap(),
            "Find this exact old-text value."
        );
        assert_eq!(
            descriptor
                .parameters
                .pointer(&format!("{nested}/type"))
                .unwrap(),
            "string"
        );
        for bad in [
            "does_not_exist",
            "/properties/edits/items/prefixItems/4",
            "/properties/edits/items/prefixItems/0/type",
            "/required",
        ] {
            assert!(
                Toolset::builder()
                    .tool_text(op, ToolText::new().argument(bad, "Text"))
                    .build()
                    .is_err(),
                "{bad}"
            );
        }
        assert!(
            Toolset::builder()
                .only([Tool::Read])
                .tool_text(op, ToolText::new().argument("missing", "text"))
                .build()
                .is_err(),
            "dormant filesystem metadata is still validated"
        );
    }

    #[test]
    fn host_operation_uses_the_same_hooks_and_validates_late_fields() {
        let op = OperationId::JobsGet;
        let profile = JobProfile {
            get_tail_default: 256,
            get_tail_max: 384,
            ..JobProfile::default()
        };
        let toolset = Toolset::builder()
            .jobs(profile)
            .tool_text(
                op,
                ToolText::new()
                    .title("Inspect Job")
                    .description_suffix(
                        "Preview ${jobs.get_tail_default}/${jobs.get_tail_max} total bytes.",
                    )
                    .argument("tail", "Max ${jobs.get_tail_max} bytes across streams."),
            )
            .build()
            .unwrap();
        let schema = json!({"type":"object","properties":{
            "tail":{"type":"integer","maximum":2048},
            "wait_ms":{"type":"integer","maximum":30000}
        },"additionalProperties":false});
        let result = toolset
            .describe_operation(op, schema, "Inspect current status.")
            .unwrap();
        assert_eq!(result.title, "Inspect Job");
        assert!(result.description.contains("Preview 256/384 total bytes."));
        assert_eq!(result.parameters["properties"]["tail"]["maximum"], 384);
        assert_eq!(
            result.parameters["properties"]["tail"]["description"],
            "Max 384 bytes across streams."
        );
        assert_eq!(result.effect, ToolEffect::Read);
        assert!(
            toolset
                .describe_operation(
                    op,
                    json!({"type":"object","properties":{},"additionalProperties":false}),
                    "base"
                )
                .is_err(),
            "host-defined fields must exist in the real tool schema"
        );
    }

    #[test]
    fn malformed_metadata_fails_before_any_tool_registration() {
        let op = OperationId::Fs(Tool::Grep);
        for text in ["", "   ", "contains\nnewline", "bad\tcontrol"] {
            assert!(Toolset::builder()
                .tool_text(op, ToolText::new().description(text))
                .build()
                .is_err());
        }
        assert!(Toolset::builder()
            .tool_text(op, ToolText::new().description("x".repeat(4097)))
            .build()
            .is_err());
        assert!(Toolset::builder()
            .tool_text(op, ToolText::new().title("x".repeat(129)))
            .build()
            .is_err());
        assert!(Toolset::builder()
            .tool_text(
                op,
                ToolText::new().description_suffix("Unknown ${grep.magic}")
            )
            .build()
            .is_err());
    }

    #[cfg(feature = "batch")]
    #[test]
    fn focused_single_file_edits_compose_through_the_existing_graph() {
        use crate::filesystem::{batch::Graph, request};
        let op = json!({"path":"a.rs","edits":[["old","new"]]});
        let other = json!({"path":"b.rs","edits":[["x","y"]]});
        let graph =
            Graph::compile(&json!({"ops":[["edit",op],["edit",other]]}), |_, _| true).unwrap();
        assert_eq!(graph.steps().len(), 2);
        let legacy_multi = json!({"edits":[
            {"path":"a.rs","old_text":"old","new_text":"new"},
            {"path":"b.rs","old_text":"x","new_text":"y"}
        ]});
        assert!(
            request::parse(Tool::Edit, &legacy_multi, request::Limits::default()).is_err(),
            "multi-file edit is expressed as existing graph nodes, not a second edit grammar"
        );
    }

    #[test]
    fn full_host_description_override_works_without_an_old_description() {
        let catalog = Toolset::builder()
            .tool_text(
                OperationId::JobsCancel,
                ToolText::new()
                    .description("Cancel this Job; never resubmit the original command."),
            )
            .build()
            .unwrap();
        let descriptor = catalog
            .describe_operation(
                OperationId::JobsCancel,
                json!({"type":"object","properties":{},"additionalProperties":false}),
                "",
            )
            .unwrap();
        assert_eq!(
            descriptor.description,
            "Cancel this Job; never resubmit the original command."
        );
        let overlong = Toolset::builder().tool_text(
            OperationId::JobsCancel,
            (0..17).fold(ToolText::new(), |text, _| text.description_suffix("x")),
        );
        assert!(overlong.build().is_err());
    }

    #[cfg(feature = "rw")]
    #[test]
    fn configured_grep_schema_parser_and_executor_share_context_boundary() {
        use crate::filesystem::{
            request::{Limits, RequestKind},
            schema::Tool,
        };
        let tools = Toolset::builder()
            .grep(GrepProfile {
                default_context: 2,
                max_context: 8,
                ..Default::default()
            })
            .only([Tool::Grep])
            .build()
            .unwrap();
        let limits = Limits::default();
        let request = tools
            .parse_filesystem(Tool::Grep, &json!({"pattern":"hit"}), limits)
            .unwrap();
        let RequestKind::Grep(args) = request.kind() else {
            panic!("expected grep")
        };
        assert_eq!((args.context, args.before, args.after), (2, None, None));
        assert!(tools
            .parse_filesystem(
                Tool::Grep,
                &json!({"pattern":"hit","context":9,"before":0,"after":0}),
                limits
            )
            .is_err());
        assert!(tools
            .parse_filesystem(Tool::Grep, &json!({"pattern":"hit","before":9}), limits)
            .is_err());
        let accepted = tools
            .parse_filesystem(
                Tool::Grep,
                &json!({"pattern":"hit","context":2,"before":8}),
                limits,
            )
            .unwrap();
        let RequestKind::Grep(args) = accepted.kind() else {
            panic!("expected grep")
        };
        assert_eq!((args.context, args.before, args.after), (2, Some(8), None));
        assert_eq!(
            tools.filesystem_descriptor(Tool::Grep).unwrap().parameters["properties"]["context"]
                ["maximum"],
            8
        );
    }

    #[test]
    fn invalid_profiles_fail_at_construction_without_registering() {
        assert!(Toolset::builder()
            .grep(GrepProfile {
                default_context: 9,
                max_context: 8,
                ..Default::default()
            })
            .build()
            .is_err());
        assert!(Toolset::builder()
            .grep(GrepProfile {
                max_context: 129,
                ..Default::default()
            })
            .build()
            .is_err());
        assert!(Toolset::builder()
            .jobs(JobProfile {
                get_tail_default: 2,
                get_tail_max: 1,
                ..Default::default()
            })
            .build()
            .is_err());
        assert!(Toolset::builder()
            .jobs(JobProfile {
                get_tail_max: MAX_GET_TAIL_BYTES + 1,
                ..Default::default()
            })
            .build()
            .is_err());
        assert!(Toolset::builder()
            .only([Tool::Grep, Tool::Grep])
            .build()
            .is_err());
        assert!(Toolset::builder()
            .receipts(ReceiptProfile {
                unix_ts: false,
                checkpoint_minutes: Some(35)
            })
            .build()
            .is_err());
        assert!(Toolset::builder().only([]).build().is_ok());
    }

    #[test]
    fn every_filesystem_property_is_documented_and_schema_closed() {
        let tools = Toolset::builder().build().unwrap();
        for tool in Tool::ALL {
            let d = tools.filesystem_descriptor(tool).unwrap();
            assert!(!d.title.is_empty() && !d.description.is_empty());
            assert_eq!(d.parameters["additionalProperties"], false);
            fn check(v: &Value) {
                match v {
                    Value::Object(map) => {
                        if let Some(props) = map.get("properties").and_then(Value::as_object) {
                            for (_, prop) in props {
                                assert!(prop
                                    .get("description")
                                    .and_then(Value::as_str)
                                    .is_some_and(|s| !s.trim().is_empty()));
                            }
                        }
                        for value in map.values() {
                            check(value);
                        }
                    }
                    Value::Array(v) => {
                        for value in v {
                            check(value);
                        }
                    }
                    _ => {}
                }
            }
            check(&d.parameters);
        }
    }

    #[cfg(all(feature = "batch", feature = "durable"))]
    #[test]
    fn typed_job_receipt_timestamp_and_status_fit_as_one_atomic_budget() {
        use crate::{
            filesystem::{job::Lifecycle, job_manager::Snapshot, status::Primary},
            receipt::{render_plain, Receipt},
        };
        let snapshot = Snapshot {
            id: "job-1".into(),
            label: "bash".into(),
            lifecycle: Lifecycle::Terminal,
            stop_reason: None,
            journal_unknown: false,
            sequence: 5,
            finished: 1,
            total: 1,
            summary: Some((Primary::Fail, false, true)),
            detail: Some("unrendered internal detail".into()),
        };
        let receipt = Receipt::JobStatus {
            reference: "j:1",
            snapshot: &snapshot,
            terminal: None,
            next: None,
        };
        let base = render_plain(receipt, 512).unwrap();
        assert_eq!(base, "j:1 bash settled rev=5 outcome=fail incomplete");
        let tools = Toolset::builder()
            .receipts(ReceiptProfile {
                unix_ts: true,
                checkpoint_minutes: Some(35),
            })
            .build()
            .unwrap();
        let expected = format!("{base}\nts=1794737000");
        assert_eq!(
            tools
                .render_receipt(receipt, Some(1_794_737_000), expected.len())
                .unwrap(),
            expected
        );
        assert_eq!(
            tools.render_receipt(receipt, Some(1_794_737_000), expected.len() - 1),
            Err(ToolsetError::ResultLimit),
        );
        // A plain consumer remains byte-for-byte identical and requires no clock.
        let plain = Toolset::builder().build().unwrap();
        assert_eq!(
            plain.render_receipt(receipt, None, base.len()).unwrap(),
            base
        );
    }

    #[test]
    fn model_receipts_get_host_time_without_corrupting_raw_data() {
        let tools = Toolset::builder()
            .receipts(ReceiptProfile {
                unix_ts: true,
                checkpoint_minutes: Some(35),
            })
            .build()
            .unwrap();
        assert!(tools.model_instructions().unwrap().contains("35 minutes"));
        let receipt = crate::receipt::Receipt::Ack {
            reference: "j:4",
            mode: crate::receipt::AckMode::Finish,
            changed: true,
            released: false,
        };
        let body = "j:4 ok ack=all released=false";
        assert_eq!(
            tools
                .render_receipt(receipt, Some(1_794_720_000), 128)
                .unwrap(),
            format!("{body}\nts=1794720000")
        );
        assert_eq!(
            tools.render_receipt(receipt, None, 128),
            Err(ToolsetError::MissingTimestamp)
        );
        assert_eq!(
            tools.render_receipt(receipt, Some(15), 10),
            Err(ToolsetError::ResultLimit)
        );
        let plain = Toolset::builder().build().unwrap();
        assert_eq!(
            plain.render_receipt(receipt, None, body.len()).unwrap(),
            body
        );
        assert_eq!(plain.model_instructions(), None);
    }
}
