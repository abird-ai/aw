//! Explicitly selected, versioned process extensions. An external process is
//! OS-user trusted code, NOT a sandbox. Discovery never grants hooks or tools.
//! The normal Agent owns all provider, effect-journal and authorization steps.
#![forbid(unsafe_code)]

use aw_agent_core::{
    agent::Agent,
    extension::{Extension, HookContext, HookEvent, HookFuture},
    tool::{
        ToolContext, ToolEffect, ToolFuture, ToolOutput, ToolSource, ToolSpec,
        MAX_TOOL_ARGUMENT_BYTES, MAX_TOOL_RESULT_BYTES,
    },
    Error, Result, Tool,
};
use aw_agent_kernel::{
    capability::SourceKind,
    hook_contract::{HookFlow, Stage},
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncWriteExt, BufWriter},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
};
use tokio_util::codec::{FramedRead, LinesCodec};

const ABI: u32 = 2;
const MAX_SERVERS: usize = 8;
const MAX_FRAME: usize = 256 * 1024;
const MAX_MANIFEST: usize = 128 * 1024;
const MAX_TOOLS: usize = 32;
const MAX_HOOKS: usize = 3;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

fn label(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

#[derive(Clone, Debug, clap::Subcommand)]
pub enum Action {
    /// Render an explicitly named advertised text template as untrusted data.
    Render {
        server: String,
        prompt: String,
        #[arg(value_name = "NAME=VALUE")]
        args: Vec<String>,
    },
}

#[derive(Clone, Debug, clap::Args, Default)]
#[group(skip)]
pub struct Options {
    /// Opt into one absolute external program: ID=/ABS/PROGRAM (no shell/PATH lookup).
    #[arg(long = "extension-process", value_name = "ID=/ABS/PROGRAM")]
    pub processes: Vec<String>,
    /// Repeatable literal child argument: ID=ARG, no interpolation or shell.
    #[arg(long = "extension-arg", value_name = "ID=ARG")]
    pub child_args: Vec<String>,
    /// Grant exactly one advertised external tool: ID:TOOL.
    #[arg(long = "extension-allow", value_name = "ID:TOOL")]
    pub extension_tool_allow: Vec<String>,
    /// Opt into an advertised typed hook: ID:prompt|tool.call|tool.result.
    #[arg(long = "extension-hook", value_name = "ID:HOOK")]
    pub hooks: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct Spec {
    id: String,
    program: PathBuf,
    args: Vec<String>,
}
type Selection = BTreeSet<(String, String)>;

fn assignment(raw: &str) -> Result<(&str, &str)> {
    let (id, value) = raw
        .split_once('=')
        .ok_or(Error::Denied("extension option requires ID=VALUE"))?;
    if !label(id, 24) || value.is_empty() {
        return Err(Error::Denied("invalid extension ID/value"));
    }
    Ok((id, value))
}
fn selected(raw: &str) -> Result<(String, String)> {
    let (id, value) = raw
        .split_once(':')
        .ok_or(Error::Denied("extension approval requires ID:NAME"))?;
    if !label(id, 24) || !(label(value, 48) || approved_hook(value)) {
        return Err(Error::Denied("invalid extension approval ID/name"));
    }
    Ok((id.into(), value.into()))
}
fn approved_hook(value: &str) -> bool {
    matches!(value, "prompt" | "tool.call" | "tool.result")
}
impl Options {
    pub fn is_empty(&self) -> bool {
        self.processes.is_empty()
            && self.child_args.is_empty()
            && self.extension_tool_allow.is_empty()
            && self.hooks.is_empty()
    }

    /// All host selections are checked before any child is spawned, provider
    /// call attempted, or durable Session opened.
    pub fn parsed(&self) -> Result<(Vec<Spec>, Selection, Selection)> {
        if self.processes.len() > MAX_SERVERS
            || self.extension_tool_allow.len() > 32
            || self.hooks.len() > MAX_SERVERS * MAX_HOOKS
        {
            return Err(Error::Limit("too many extension declarations/approvals"));
        }
        let mut specs = BTreeMap::new();
        for raw in &self.processes {
            let (id, value) = assignment(raw)?;
            let program = PathBuf::from(value);
            if !program.is_absolute() || !program.is_file() {
                return Err(Error::Denied(
                    "external extension requires an absolute existing executable",
                ));
            }
            if specs
                .insert(
                    id.to_owned(),
                    Spec {
                        id: id.into(),
                        program,
                        args: Vec::new(),
                    },
                )
                .is_some()
            {
                return Err(Error::Denied("duplicate extension ID"));
            }
        }
        for raw in &self.child_args {
            let (id, arg) = assignment(raw)?;
            let spec = specs
                .get_mut(id)
                .ok_or(Error::Denied("unknown extension for argument"))?;
            if spec.args.len() >= 16 || arg.len() > 1024 || arg.chars().any(char::is_control) {
                return Err(Error::Limit("invalid/too many extension arguments"));
            }
            spec.args.push(arg.into());
        }
        let mut tools = BTreeSet::new();
        for raw in &self.extension_tool_allow {
            let value = selected(raw)?;
            if !specs.contains_key(&value.0) || !tools.insert(value) {
                return Err(Error::Denied("invalid/duplicate extension tool approval"));
            }
        }
        let mut hooks = BTreeSet::new();
        for raw in &self.hooks {
            let value = selected(raw)?;
            if !specs.contains_key(&value.0) || !approved_hook(&value.1) || !hooks.insert(value) {
                return Err(Error::Denied("invalid/duplicate extension hook approval"));
            }
        }
        if specs.is_empty() && !self.is_empty() {
            return Err(Error::Denied(
                "extension approvals require an explicit process",
            ));
        }
        Ok((specs.into_values().collect(), tools, hooks))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDescriptor {
    name: String,
    description: String,
    input_schema: Value,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptDescriptor {
    name: String,
    description: String,
    #[serde(default)]
    arguments: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Data-only manifest version, NOT trusted proof of executable integrity.
    pub protocol: u32,
    pub name: String,
    pub revision: String,
    #[serde(default)]
    pub tools: Vec<ToolDescriptor>,
    #[serde(default)]
    pub hooks: Vec<String>,
    #[serde(default)]
    pub prompts: Vec<PromptDescriptor>,
}
impl Manifest {
    fn validate(&self) -> Result<()> {
        if self.protocol != ABI
            || !label(&self.name, 64)
            || !label(&self.revision, 64)
            || self.tools.len() > MAX_TOOLS
            || self.hooks.len() > MAX_HOOKS
            || self.prompts.len() > 32
        {
            return Err(Error::Denied("unsupported/unbounded extension manifest"));
        }
        let mut tools = BTreeSet::new();
        for tool in &self.tools {
            if !label(&tool.name, 40)
                || !tools.insert(&tool.name)
                || tool.description.len() > 8192
                || tool.description.chars().any(char::is_control)
            {
                return Err(Error::Denied("invalid/duplicate extension tool descriptor"));
            }
        }
        let mut hooks = BTreeSet::new();
        for hook in &self.hooks {
            if !approved_hook(hook) || !hooks.insert(hook) {
                return Err(Error::Denied("invalid/duplicate extension hook descriptor"));
            }
        }
        let mut prompts = BTreeSet::new();
        for prompt in &self.prompts {
            if !label(&prompt.name, 48)
                || !prompts.insert(&prompt.name)
                || prompt.description.len() > 4096
                || prompt.arguments.len() > 16
            {
                return Err(Error::Denied("invalid extension prompt descriptor"));
            }
            let mut names = BTreeSet::new();
            for name in &prompt.arguments {
                if !label(name, 64) || !names.insert(name) {
                    return Err(Error::Denied("invalid extension prompt argument"));
                }
            }
        }
        Ok(())
    }
}
#[derive(Serialize)]
struct Request<'a> {
    v: u32,
    id: u64,
    method: &'a str,
    params: &'a Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    v: u32,
    id: u64,
    ok: bool,
    result: Option<Value>,
}

struct Io {
    child: Child,
    writer: BufWriter<ChildStdin>,
    reader: FramedRead<ChildStdout, LinesCodec>,
}
struct Client {
    io: Mutex<Io>,
    next_id: AtomicU64,
    dead: AtomicBool,
}
/// A cancelled request can have had an external effect. Poison the connection
/// on ANY dropped/incomplete response; never pair a stale reply with a new ID.
struct InFlight<'a> {
    client: &'a Client,
    finished: bool,
}
impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.client.poison();
        }
    }
}
impl Client {
    fn poison(&self) {
        self.dead.store(true, Ordering::Release);
        if let Ok(mut io) = self.io.try_lock() {
            let _ = io.child.start_kill();
        }
    }
    async fn call(&self, method: &str, params: &Value, deadline: Duration) -> Result<Value> {
        if self.dead.load(Ordering::Acquire) {
            return Err(Error::Denied(
                "extension process is unavailable; restart/regrant required",
            ));
        }
        let mut guard = InFlight {
            client: self,
            finished: false,
        };
        let outcome = tokio::time::timeout(deadline, async {
            let mut io = self.io.lock().await;
            if self.dead.load(Ordering::Acquire) {
                return Err(Error::Denied("extension process unavailable"));
            }
            let id = self
                .next_id
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                    value.checked_add(1)
                })
                .map_err(|_| Error::Limit("extension request ID exhausted"))?;
            let bytes = serde_json::to_vec(&Request {
                v: ABI,
                id,
                method,
                params,
            })
            .map_err(|_| Error::Protocol("cannot serialize extension request"))?;
            if bytes.len() > MAX_FRAME - 1 {
                return Err(Error::Limit("extension request exceeds frame budget"));
            }
            io.writer.write_all(&bytes).await?;
            io.writer.write_all(b"\n").await?;
            io.writer.flush().await?;
            let frame = io
                .reader
                .next()
                .await
                .ok_or(Error::Transport("extension closed before responding"))?
                .map_err(|_| Error::Transport("extension response exceeded frame/UTF-8 bounds"))?;
            let response: Response = serde_json::from_str(&frame)
                .map_err(|_| Error::Protocol("malformed extension response"))?;
            if response.v != ABI || response.id != id || !response.ok || response.result.is_none() {
                return Err(Error::Denied(
                    "extension response denied/mismatched request",
                ));
            }
            Ok(response.result.expect("validated result presence"))
        })
        .await;
        match outcome {
            Ok(Ok(result)) => {
                guard.finished = true;
                Ok(result)
            }
            Ok(Err(error)) => Err(error),
            Err(_) => Err(Error::Timeout),
        }
    }
}
async fn connect(spec: &Spec) -> Result<(Arc<Client>, Manifest)> {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env_clear()
        .kill_on_drop(true);
    let mut child = cmd
        .spawn()
        .map_err(|_| Error::Transport("cannot start selected extension"))?;
    let writer = child
        .stdin
        .take()
        .ok_or(Error::Protocol("extension stdin unavailable"))?;
    let reader = child
        .stdout
        .take()
        .ok_or(Error::Protocol("extension stdout unavailable"))?;
    let conn = Arc::new(Client {
        io: Mutex::new(Io {
            child,
            writer: BufWriter::new(writer),
            reader: FramedRead::new(reader, LinesCodec::new_with_max_length(MAX_FRAME)),
        }),
        next_id: AtomicU64::new(1),
        dead: AtomicBool::new(false),
    });
    let value = conn.call("describe", &json!({}), HANDSHAKE_TIMEOUT).await?;
    if serde_json::to_vec(&value)
        .map_err(|_| Error::Protocol("extension manifest serialization"))?
        .len()
        > MAX_MANIFEST
    {
        return Err(Error::Limit("extension manifest exceeds 128 KiB"));
    }
    let manifest: Manifest = serde_json::from_value(value)
        .map_err(|_| Error::Denied("extension manifest has unknown/invalid fields"))?;
    manifest.validate()?;
    Ok((conn, manifest))
}

fn schema(tool: &ToolDescriptor) -> Result<jsonschema::Validator> {
    let encoded = serde_json::to_vec(&tool.input_schema)
        .map_err(|_| Error::Protocol("invalid extension JSON Schema"))?;
    if encoded.len() > 65536 || tool.input_schema["type"] != "object" {
        return Err(Error::Denied(
            "extension tool requires bounded object schema",
        ));
    }
    let mut work = vec![(&tool.input_schema, 0usize)];
    let mut visited = 0usize;
    while let Some((value, depth)) = work.pop() {
        visited += 1;
        if visited > 4096 || depth > 32 {
            return Err(Error::Limit(
                "extension JSON Schema structural budget exceeded",
            ));
        }
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    // Only ordinary local static references are supported in
                    // this process ABI. Dynamic, recursive and remote references
                    // create a second resolution authority and are refused.
                    if matches!(key.as_str(), "$dynamicRef" | "$recursiveRef" | "$id") {
                        return Err(Error::Denied(
                            "extension dynamic/external JSON Schema references forbidden",
                        ));
                    }
                    if key == "$ref"
                        && !child.as_str().is_some_and(|r| {
                            r.starts_with("#/$defs/") || r.starts_with("#/definitions/")
                        })
                    {
                        return Err(Error::Denied(
                            "extension external JSON Schema references forbidden",
                        ));
                    }
                    if key == "$schema" && child != "https://json-schema.org/draft/2020-12/schema" {
                        return Err(Error::Denied("unsupported extension JSON Schema draft"));
                    }
                    work.push((child, depth + 1));
                }
            }
            Value::Array(items) => work.extend(items.iter().map(|v| (v, depth + 1))),
            _ => (),
        }
    }
    jsonschema::draft202012::new(&tool.input_schema)
        .map_err(|_| Error::Denied("extension JSON Schema invalid/unresolvable"))
}
fn epoch() -> Result<u64> {
    let mut bytes = [0; 8];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Denied("entropy unavailable for extension source epoch"))?;
    Ok(u64::from_le_bytes(bytes).max(1))
}

struct RemoteTool {
    spec: ToolSpec,
    name: String,
    validator: jsonschema::Validator,
    client: Arc<Client>,
}
impl Tool for RemoteTool {
    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }
    fn is_available(&self) -> bool {
        !self.client.dead.load(Ordering::Acquire)
    }
    fn validate(&self, args: &Value) -> Result<()> {
        if self.client.dead.load(Ordering::Acquire) {
            return Err(Error::Denied("extension tool unavailable"));
        }
        if !args.is_object()
            || serde_json::to_vec(args)
                .map_err(|_| Error::Protocol("invalid extension arguments"))?
                .len()
                > MAX_TOOL_ARGUMENT_BYTES
            || !self.validator.is_valid(args)
        {
            return Err(Error::Denied(
                "extension tool arguments not authorized by schema",
            ));
        }
        Ok(())
    }
    fn execute<'a>(&'a self, ctx: ToolContext, args: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(args)?;
            let request = json!({"name":self.name,"arguments":args,"turn":ctx.turn_id});
            let response = tokio::select! {
                _ = ctx.cancellation.cancelled() => return Err(Error::Cancelled),
                result = self.client.call("tool.execute", &request, REQUEST_TIMEOUT) => result?,
            };
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Output {
                content: String,
                is_error: bool,
            }
            let output: Output = serde_json::from_value(response).map_err(|_| {
                self.client.poison();
                Error::Protocol("invalid extension tool result")
            })?;
            if output.content.len() > MAX_TOOL_RESULT_BYTES {
                self.client.poison();
                return Err(Error::Limit("extension output exceeds 128 KiB"));
            }
            Ok(ToolOutput {
                content: output.content,
                is_error: output.is_error,
                truncated: false,
            })
        })
    }
}

struct ProcessHook {
    name: String,
    allowed: BTreeSet<String>,
    client: Arc<Client>,
}
impl ProcessHook {
    async fn gate(&self, phase: &str, params: Value) -> Result<()> {
        if !self.allowed.contains(phase) {
            return Ok(());
        }
        let response = self.client.call(phase, &params, REQUEST_TIMEOUT).await?;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Decision {
            allow: bool,
        }
        let decision: Decision = serde_json::from_value(response).map_err(|_| {
            self.client.poison();
            Error::Protocol("invalid extension hook decision")
        })?;
        if !decision.allow {
            return Err(Error::Denied("external extension hook veto"));
        }
        Ok(())
    }
}
impl Extension for ProcessHook {
    fn name(&self) -> &str {
        &self.name
    }
    fn wants_stage(&self, stage: Stage) -> bool {
        match stage {
            Stage::Prompt => self.allowed.contains("prompt"),
            Stage::Tool => {
                self.allowed.contains("tool.call") || self.allowed.contains("tool.result")
            }
            _ => false,
        }
    }
    fn on_hook<'a>(&'a self, ctx: HookContext<'a>, event: HookEvent) -> HookFuture<'a> {
        Box::pin(async move {
            match &event {
                HookEvent::Prompt { input, .. } => {
                    self.gate("prompt", json!({"turn":ctx.turn_id,"prompt":input}))
                        .await?;
                }
                HookEvent::ToolCall { call, spec } => {
                    self.gate(
                        "tool.call",
                        json!({"turn":ctx.turn_id,"tool":call.name,"id":call.id,
                        "arguments":call.arguments,"effect":spec.effect}),
                    )
                    .await?;
                }
                HookEvent::ToolResult { call, receipt, .. } => {
                    // Only factual, already-journaled result fields go to this
                    // explicitly selected hook. Private thought never escapes.
                    self.gate(
                        "tool.result",
                        json!({"turn":ctx.turn_id,"tool":call.name,
                        "id":call.id,"is_error":receipt.is_error,"content":receipt.content}),
                    )
                    .await?;
                }
                HookEvent::Step { .. } | HookEvent::Observation { .. } => {}
            };
            Ok(HookFlow::Next(Some(event)))
        })
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Inspected {
    pub id: String,
    pub name: String,
    pub revision: String,
    pub tools: Vec<String>,
    pub hooks: Vec<String>,
    pub prompts: Vec<String>,
}
pub async fn inspect(options: &Options) -> Result<Vec<Inspected>> {
    let (specs, tools, hooks) = options.parsed()?;
    if !tools.is_empty() || !hooks.is_empty() {
        return Err(Error::Denied(
            "extension discovery cannot grant tools or hooks",
        ));
    }
    let mut rows = Vec::new();
    for spec in specs {
        let (_, manifest) = connect(&spec).await?;
        rows.push(Inspected {
            id: spec.id,
            name: manifest.name,
            revision: manifest.revision,
            tools: manifest.tools.into_iter().map(|t| t.name).collect(),
            hooks: manifest.hooks,
            prompts: manifest.prompts.into_iter().map(|p| p.name).collect(),
        });
    }
    Ok(rows)
}

/// Explicitly render one advertised process template as *untrusted data*.
/// It never becomes a system message, capability, account or action itself.
pub async fn render(options: &Options, id: &str, prompt: &str, args: &[String]) -> Result<String> {
    let (specs, tools, hooks) = options.parsed()?;
    if !tools.is_empty() || !hooks.is_empty() {
        return Err(Error::Denied(
            "extension prompt inspection cannot grant hooks/tools",
        ));
    }
    let spec = specs
        .iter()
        .find(|s| s.id == id)
        .ok_or(Error::Denied("prompt extension not selected"))?;
    let (client, manifest) = connect(spec).await?;
    let desc = manifest
        .prompts
        .iter()
        .find(|p| p.name == prompt)
        .ok_or(Error::Denied("prompt not advertised"))?;
    if args.len() > 16 {
        return Err(Error::Limit("too many prompt variables"));
    }
    let mut map = BTreeMap::new();
    for arg in args {
        let (name, value) = arg
            .split_once('=')
            .ok_or(Error::Denied("prompt variable requires NAME=VALUE"))?;
        if !label(name, 64)
            || value.len() > 2048
            || value.contains('\0')
            || map.insert(name.to_owned(), value.to_owned()).is_some()
        {
            return Err(Error::Denied("invalid/duplicate prompt variable"));
        }
    }
    if map.len() != desc.arguments.len() || map.keys().any(|name| !desc.arguments.contains(name)) {
        return Err(Error::Denied(
            "extension prompt variables must match advertised template",
        ));
    }
    let result = client
        .call(
            "prompt.render",
            &json!({"name":prompt,"arguments":map}),
            REQUEST_TIMEOUT,
        )
        .await?;
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Text {
        text: String,
    }
    let text: Text = serde_json::from_value(result)
        .map_err(|_| Error::Protocol("invalid extension prompt text"))?;
    if text.text.len() > 128 * 1024 {
        return Err(Error::Limit("extension prompt too large"));
    }
    Ok(text.text)
}

/// Compose a *single* typed Agent. The same ToolRegistry/ToolPolicy/journal
/// handles process tools. Hooks are added only when the host explicitly opted
/// into a name advertised by that exact process.
pub async fn attach(agent: &mut Agent, options: &Options) -> Result<usize> {
    let (specs, tool_grants, hook_grants) = options.parsed()?;
    let mut prepared = Vec::new();
    let mut names = BTreeSet::new();
    let mut offered = BTreeSet::new();
    for spec in specs {
        let (client, manifest) = connect(&spec).await?;
        let source = ToolSource::new(SourceKind::Extension, &spec.id, epoch()?)?;
        let mut tools = Vec::new();
        for tool in manifest.tools {
            let local = format!("ext__{}__{}", spec.id, tool.name);
            if !label(&local, 64) || !names.insert(local.clone()) {
                return Err(Error::Denied(
                    "extension tool collision/invalid scoped identifier",
                ));
            }
            offered.insert((spec.id.clone(), tool.name.clone()));
            if tool_grants.contains(&(spec.id.clone(), tool.name.clone())) {
                let validator = schema(&tool)?;
                tools.push((
                    tool.name,
                    ToolSpec {
                        name: local,
                        description: tool.description,
                        parameters: tool.input_schema,
                        effect: ToolEffect::Unknown,
                    },
                    validator,
                ));
            }
        }
        let allowed = hook_grants
            .iter()
            .filter(|(id, _)| id == &spec.id)
            .map(|(_, hook)| hook.clone())
            .collect::<BTreeSet<_>>();
        if allowed.iter().any(|hook| !manifest.hooks.contains(hook)) {
            return Err(Error::Denied("extension hook grant not advertised"));
        }
        prepared.push((spec.id, client, source, tools, allowed));
    }
    if tool_grants.iter().any(|grant| !offered.contains(grant)) {
        return Err(Error::Denied("extension tool grant not advertised"));
    }
    let registered: BTreeSet<_> = agent.registered_tool_names().into_iter().collect();
    let count = prepared
        .iter()
        .map(|(_, _, _, tools, _)| tools.len())
        .sum::<usize>();
    if count + registered.len() > MAX_TOOLS || names.iter().any(|name| registered.contains(name)) {
        return Err(Error::Denied(
            "extension tool name collision or registry capacity exceeded",
        ));
    }
    let existing_hooks: BTreeSet<_> = agent.registered_extension_names().into_iter().collect();
    let new_hooks = prepared
        .iter()
        .filter(|(_, _, _, _, allowed)| !allowed.is_empty())
        .count();
    if existing_hooks.len() + new_hooks > aw_agent_core::limits::MAX_EXTENSIONS
        || prepared
            .iter()
            .filter(|(_, _, _, _, allowed)| !allowed.is_empty())
            .any(|(id, _, _, _, _)| existing_hooks.contains(&format!("ext_{id}")))
    {
        return Err(Error::Denied("extension hook registry collision/capacity"));
    }
    let actor = agent.actor().clone();
    for (id, client, source, tools, hooks) in prepared {
        if !hooks.is_empty() {
            agent.register_host_extension(Arc::new(ProcessHook {
                name: format!("ext_{id}"),
                allowed: hooks,
                client: client.clone(),
            }))?;
        }
        for (remote, spec, validator) in tools {
            let local = spec.name.clone();
            agent.register_source_tool(
                Arc::new(RemoteTool {
                    spec,
                    name: remote,
                    validator,
                    client: client.clone(),
                }),
                source.clone(),
            )?;
            agent.permit_registered_source_tool(actor.clone(), &local, &source)?;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn option_grants_never_infer_execution() {
        let opts = Options {
            extension_tool_allow: vec!["x:tool".into()],
            ..Options::default()
        };
        assert!(opts.parsed().is_err());
        assert!(Options {
            hooks: vec!["x:prompt".into()],
            ..Options::default()
        }
        .parsed()
        .is_err());
        assert!(!approved_hook("transform_context"));
        assert!(!approved_hook("after_dispatch"));
        assert!(selected("x:../escape").is_err());
    }
    #[test]
    fn remote_descriptors_and_effects_are_not_permissions() {
        let manifest:Manifest=serde_json::from_value(json!({
            "protocol":2,"name":"fixture","revision":"v1","tools":[{"name":"echo","description":"echo",
                "input_schema":{"type":"object","properties":{"message":{"type":"string"}}}}],
            "hooks":["prompt"],"prompts":[{"name":"greet","description":"user data","arguments":["name"]}]
        })).unwrap();
        manifest.validate().unwrap();
        assert!(schema(&manifest.tools[0])
            .unwrap()
            .is_valid(&json!({"message":"ok"})));
        assert!(serde_json::from_value::<Manifest>(
            json!({"protocol":2,"name":"x","revision":"v1","tools":[],"unsafe":true})
        )
        .is_err());
        assert!(schema(&ToolDescriptor{name:"bad".into(),description:"bad".into(),input_schema:json!({"type":"object","properties":{"x":{"$ref":"https://evil.invalid/schema"}}})}).is_err());
        for forbidden in [
            json!({"$dynamicRef":"https://evil.invalid/schema"}),
            json!({"$recursiveRef":"https://evil.invalid/schema"}),
            json!({"$id":"https://evil.invalid/schema"}),
            json!({"$ref":"relative.json"}),
            json!({"$schema":"http://json-schema.org/draft-07/schema#"}),
            json!({"$ref":17}),
        ] {
            let input_schema = json!({"type":"object","properties":{"x":forbidden}});
            assert!(schema(&ToolDescriptor {
                name: "bad".into(),
                description: "bad".into(),
                input_schema
            })
            .is_err());
        }
        assert!(schema(&ToolDescriptor{name:"good".into(),description:"good".into(),input_schema:json!({
            "type":"object","$defs":{"entry":{"type":"string"}},"properties":{"x":{"$ref":"#/$defs/entry"}}
        })}).is_ok());
    }
}
