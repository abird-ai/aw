//! Versioned bounded headless JSONL projection and stdio RPC adapter for
//! the ONE hosted Agent. No provider/tool/Session implementation lives here.
//! The adapter never serializes provider-private continuation, auth headers,
//! credential stores or mutable source grants.
use aw_agent_core::{AgentEvent, CancellationToken, Error, Outcome, Result, Turn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    io::{self, BufRead, BufReader, Read, Write},
    sync::{mpsc, Arc, Mutex},
    thread,
};

pub const VERSION: u8 = 1;
pub const MAX_REQUEST_BYTES: usize = 192 * 1024;
pub const MAX_EVENT_BYTES: usize = 64 * 1024;
pub const MAX_FRAME_BYTES: usize = 640 * 1024;
const MAX_IDS: usize = 1024;
const MAX_INPUT_QUEUE: usize = 32;
const MAX_HISTORY: usize = 12;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Output {
    #[default]
    Text,
    Jsonl,
}

#[derive(Clone, Debug, Serialize)]
pub struct State {
    pub profile: String,
    pub model: Option<String>,
    pub session: Option<String>,
    pub next_turn: u64,
    pub turn_count: usize,
    pub needs_reconciliation: bool,
    pub tool_names: Vec<String>,
}

/// A transport-independent authenticated current-Turn signaling port.
/// The live Runtime + immutable Control audit remain the SOLE authority;
/// reader threads never generate a Control acknowledgement themselves.
pub trait LiveControlPort: Send + Sync {
    fn apply(&self, operation: Value) -> Result<Value>;
}

/// Concurrent owner-bound Agent lifecycle port. The transport supplies only
/// the target ref/revision; Project, provider, Sandbox and execution authority
/// remain fixed by the already-bound host.
pub trait AgentClosePort: Send + Sync {
    fn apply(&self, reference: &str, expected_rev: Option<u64>) -> Result<Value>;
}

/// Concurrent owner-bound Agent constructor. The transport supplies only the
/// sealed host API fields plus its authenticated request identity; provider,
/// model, execution site and durable current-Agent context remain host-owned.
pub trait AgentCreatePort: Send + Sync {
    fn apply(&self, request_id: &str, params: Value) -> Result<Value>;
}

/// Concurrent authenticated Agent state projection/mutation surface. Durable
/// Agent state, ACLs and revision semantics remain owned by SessionStore.
pub trait AgentStatePort: Send + Sync {
    fn apply(&self, method: &str, params: Value) -> Result<Value>;
}

/// Concurrent host staged-input surface. The transport request ID is the
/// durable mutation identity for add/edit/delete/send; list remains a pure
/// snapshot read.
pub trait AgentInputsPort: Send + Sync {
    fn apply(&self, request_id: &str, method: &str, params: Value) -> Result<Value>;
}

/// Concurrent current-Project Sandbox read/lifecycle surface. Physical
/// materialization and Sandbox creation remain unavailable until the
/// SourceDriver/checkpoint owner can satisfy prepared-publication semantics.
pub trait SandboxPort: Send + Sync {
    fn apply(&self, method: &str, params: Value) -> Result<Value>;
}

/// Authenticated checkpoint metadata only. No source disclosure authority,
/// constructor, restore or driver execution belongs to this read port.
pub trait CheckpointReadPort: Send + Sync {
    fn apply(&self, method: &str, params: Value) -> Result<Value>;
}

/// Explicit host checkpoint-create capability. The authenticated envelope
/// ID is the original constructor replay identity, not a model parameter.
pub trait CheckpointCreatePort: Send + Sync {
    fn create(&self, request_id: &str, params: Value) -> Result<Value>;
}

/// Explicit host restore capability; the host owns destination layout and
/// source authority, never the RPC parameters.
pub trait CheckpointRestorePort: Send + Sync {
    fn restore(&self, request_id: &str, params: Value) -> Result<Value>;
}

/// Explicit logical retirement capability, independent of creation, restore
/// and physical source retention cleanup.
pub trait CheckpointDeletePort: Send + Sync {
    fn delete(&self, request_id: &str, params: Value) -> Result<Value>;
}

/// Explicit host-owned Vault binding capability. No broker invocation or
/// secret material is exposed through this port.
pub trait AgentVaultPort: Send + Sync {
    fn apply(&self, request_id: &str, method: &str, params: Value) -> Result<Value>;
}

pub trait Driver {
    /// Must execute using aw's existing, exactly selected Agent/journal.
    fn prompt(
        &mut self,
        text: String,
        cancel: CancellationToken,
        emit: &mut (dyn FnMut(&AgentEvent) -> Result<()> + Send),
    ) -> Result<Turn>;
    fn state(&self) -> State;
    fn turns(&self) -> &[Turn];
    fn template(&self, _name: &str, _text: &str, _vars: &[String]) -> Result<String> {
        Err(Error::Denied(
            "headless template unavailable without exact source approval",
        ))
    }
    fn can_template(&self) -> bool {
        false
    }
    /// Already-rendered explicit template: the host must avoid applying any
    /// default CLI template a second time.
    fn prompt_prepared(
        &mut self,
        text: String,
        cancel: CancellationToken,
        emit: &mut (dyn FnMut(&AgentEvent) -> Result<()> + Send),
    ) -> Result<Turn> {
        self.prompt(text, cancel, emit)
    }
    fn compact(&mut self) -> Result<u64> {
        Err(Error::Denied(
            "this session backend does not support active-tail compaction",
        ))
    }
    fn can_compact(&self) -> bool {
        false
    }
    fn set_model(&mut self, _model: &str) -> Result<()> {
        Err(Error::Denied(
            "provider/model modification requires a separate authorized session",
        ))
    }
    fn set_effort(&mut self, _effort: &str) -> Result<()> {
        Err(Error::Denied(
            "reasoning effort change not supported by this headless host",
        ))
    }
    /// Optional owner-bound R12 Actor service. This flag is NOT controlled
    /// by any RPC request. Default and JSONL/stateless hosts fail closed.
    fn actor_available(&self) -> bool {
        false
    }
    fn live_control_port(&self) -> Option<Arc<dyn LiveControlPort>> {
        None
    }
    fn agent_close_port(&self) -> Option<Arc<dyn AgentClosePort>> {
        None
    }
    fn agent_create_port(&self) -> Option<Arc<dyn AgentCreatePort>> {
        None
    }
    fn agent_state_port(&self) -> Option<Arc<dyn AgentStatePort>> {
        None
    }
    fn agent_inputs_port(&self) -> Option<Arc<dyn AgentInputsPort>> {
        None
    }
    fn sandbox_port(&self) -> Option<Arc<dyn SandboxPort>> {
        None
    }
    fn checkpoint_read_port(&self) -> Option<Arc<dyn CheckpointReadPort>> {
        None
    }
    fn checkpoint_create_port(&self) -> Option<Arc<dyn CheckpointCreatePort>> {
        None
    }
    fn checkpoint_restore_port(&self) -> Option<Arc<dyn CheckpointRestorePort>> {
        None
    }
    fn checkpoint_delete_port(&self) -> Option<Arc<dyn CheckpointDeletePort>> {
        None
    }
    fn agent_vault_port(&self) -> Option<Arc<dyn AgentVaultPort>> {
        None
    }
    fn actor_query(&mut self, _operation: Value) -> Result<Value> {
        Err(Error::Denied(
            "RPC has no current authenticated Actor owner",
        ))
    }
    fn actor_op(&mut self, _operation: Value) -> Result<Value> {
        Err(Error::Denied(
            "RPC has no current authenticated Actor owner",
        ))
    }
    fn actor_enqueue(&mut self, _p: &Value) -> Result<Value> {
        Err(Error::Denied(
            "RPC has no current authenticated Actor owner",
        ))
    }
    fn actor_notice_read(&mut self, _request_id: &str) -> Result<Value> {
        Err(Error::Denied(
            "RPC has no current authenticated Actor owner",
        ))
    }
    fn actor_step(
        &mut self,
        _cancel: CancellationToken,
        _emit: &mut (dyn FnMut(&AgentEvent) -> Result<()> + Send),
    ) -> Result<Option<Turn>> {
        Err(Error::Denied(
            "RPC has no current authenticated Actor owner",
        ))
    }
    fn can_sessions(&self) -> bool {
        false
    }
    fn new_session(&mut self) -> Result<String> {
        Err(Error::Denied(
            "durable Session creation not supported by this headless host",
        ))
    }
    fn open_session(&mut self, _session: &str) -> Result<()> {
        Err(Error::Denied(
            "durable Session switching not supported by this headless host",
        ))
    }
}

pub struct Emitter<W: Write> {
    writer: W,
    sequence: u64,
    diagnostics: bool,
}
impl<W: Write> Emitter<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            sequence: 0,
            diagnostics: false,
        }
    }
    pub fn diagnostics(mut self, enabled: bool) -> Self {
        self.diagnostics = enabled;
        self
    }
    pub fn emit(&mut self, kind: &str, id: Option<&str>, data: Value) -> Result<()> {
        if self.sequence == u64::MAX {
            return Err(Error::Limit("headless event sequence overflow"));
        }
        self.sequence += 1;
        let value = json!({"v":VERSION,"seq":self.sequence,"type":kind,"id":id,"data":data});
        let encoded = serde_json::to_vec(&value)
            .map_err(|_| Error::Protocol("headless event encoding failed"))?;
        if encoded.len() > MAX_FRAME_BYTES {
            return Err(Error::Limit("headless event frame exceeds bound"));
        }
        self.writer.write_all(&encoded)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        if self.diagnostics {
            eprintln!(
                "aw protocol v1: type={kind} frame_bytes={} data=redacted",
                encoded.len()
            );
        }
        Ok(())
    }
    pub fn event(&mut self, id: Option<&str>, event: &AgentEvent) -> Result<()> {
        match event {
            AgentEvent::Started{turn_id,profile,model} => self.emit(
                "started",id,json!({"turn_id":turn_id,"profile":profile,"model":model})),
            AgentEvent::Delta{turn_id,text} => {
                // UTF-8 safe chunks avoid a giant unbuffered stdout line.
                let mut offset=0;
                while offset<text.len() {
                    let mut end=(offset + MAX_EVENT_BYTES/2).min(text.len());
                    while !text.is_char_boundary(end) {end-=1;}
                    self.emit("delta",id,json!({"turn_id":turn_id,"text":&text[offset..end]}))?;
                    offset=end;
                }
                Ok(())
            }
            AgentEvent::ToolStarted{turn_id,record} => self.emit("tool_started",id,json!({
                "turn_id":turn_id,"call_id":record.call.id,"name":record.call.name,
                "effect":record.effect,"status":record.status,
            })),
            AgentEvent::ToolFinished{turn_id,record} => self.emit("tool_finished",id,json!({
                "turn_id":turn_id,"call_id":record.call.id,"name":record.call.name,
                "effect":record.effect,"status":record.status,
                "result":record.output.as_ref().map(|o|{
                    let mut bytes=o.content.len().min(16*1024);
                    while !o.content.is_char_boundary(bytes) {bytes-=1;}
                    json!({"text":&o.content[..bytes],"truncated":o.truncated || bytes<o.content.len(),
                           "is_error":o.is_error})
                })
            })),
            AgentEvent::InvocationFinished{turn_id,record} => self.emit(
                "attempt_finished",id,json!({
                    "turn_id":turn_id,"ordinal":record.ordinal,"outcome":record.outcome,
                    "usage_verified":record.usage_verified,
                    "usage":record.meta.as_ref().map(|m|&m.usage),
                })),
            AgentEvent::Finished{turn} => self.emit("finished",id,public_turn(turn)),
            AgentEvent::Rejected{..} => self.emit(
                "rejected",id,json!({"code":"admission_denied",
                    "note":"no provider/tool grant is inferred from this request"})),
        }
    }
}

fn public_turn(turn: &Turn) -> Value {
    json!({"turn_id":turn.id,"outcome":turn.outcome,"answer":turn.answer,
        "model":turn.model,"profile":turn.profile,
        "needs_reconciliation":turn.needs_reconciliation,
        "attempts":turn.invocations.iter().map(|v|json!({
            "ordinal":v.ordinal,"outcome":v.outcome,"usage_verified":v.usage_verified,
        })).collect::<Vec<_>>()})
}
pub fn error_code(error: &Error) -> &'static str {
    match error {
        Error::Denied(_) => "denied",
        Error::Limit(_) => "limit",
        Error::Conflict(_) => "conflict",
        Error::Timeout => "timeout",
        Error::Cancelled => "cancelled",
        Error::Config(_) => "invalid_configuration",
        Error::Protocol(_) => "protocol",
        Error::BeforeDispatch(_) => "not_dispatched",
        Error::Http { .. } => "http",
        Error::Transport(_) => "transport_ambiguous",
        Error::Storage(_) => "storage",
        _ => "failed",
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    v: u8,
    id: String,
    method: String,
    #[serde(default = "empty_params")]
    params: Value,
}
fn empty_params() -> Value {
    json!({})
}
impl Request {
    fn validate(&self) -> Result<()> {
        if self.v != VERSION {
            return Err(Error::Protocol("unsupported RPC version"));
        }
        if self.id.is_empty()
            || self.id.len() > 64
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(Error::Protocol("invalid RPC request ID"));
        }
        if self.method.is_empty()
            || self.method.len() > 64
            || !self
                .method
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'_')
        {
            return Err(Error::Protocol("invalid RPC method"));
        }
        if !self.params.is_null() && !self.params.is_object() {
            return Err(Error::Protocol("RPC params must be an object"));
        }
        Ok(())
    }
}
fn param_text<'a>(p: &'a Value, name: &str, max: usize) -> Result<&'a str> {
    let text = p
        .get(name)
        .and_then(Value::as_str)
        .ok_or(Error::Protocol("required RPC text parameter missing"))?;
    if text.is_empty() || text.len() > max || text.chars().any(|c| c == '\0') {
        return Err(Error::Limit("invalid or oversized RPC text argument"));
    }
    Ok(text)
}
fn validate_method_params(method: &str, p: &Value) -> Result<()> {
    let permitted: Option<&[&str]> = match method {
        "get_state" | "available_models" | "new_session" | "compact" => Some(&[]),
        "get_messages" => Some(&["limit"]),
        "prompt" => Some(&["text"]),
        "prompt_template" => Some(&["name", "text", "variables"]),
        "abort" => Some(&["target_id"]),
        "open_session" => Some(&["session"]),
        "set_model" => Some(&["model"]),
        "set_effort" => Some(&["effort"]),
        "actor_query" | "actor_op" => Some(&["operation"]),
        "agent_create" => Some(&["from", "project", "title", "task", "access", "sandbox"]),
        "agent_close" => Some(&["ref", "rev"]),
        "agent_list" => Some(&["project", "limit", "cursor"]),
        "agent_get" => Some(&["ref", "include"]),
        "agent_edit" => Some(&["ref", "title", "rev"]),
        "agent_admission" => Some(&["ref", "state", "rev"]),
        "agent_access_list" => Some(&["ref"]),
        "agent_access_set" => Some(&["ref", "principal", "permissions", "rev"]),
        "agent_inputs_list" => Some(&["ref"]),
        "agent_inputs_add" => Some(&["ref", "items", "rev"]),
        "agent_inputs_edit" => Some(&["ref", "id", "text", "intent", "before", "after", "rev"]),
        "agent_inputs_delete" => Some(&["ref", "ids", "rev"]),
        "agent_inputs_send" => Some(&["ref", "ids", "rev"]),
        "sandbox_list" => Some(&["project", "limit", "cursor"]),
        "sandbox_get" => Some(&["ref", "include"]),
        "sandbox_sources_list" => Some(&["ref"]),
        "checkpoint_list" => Some(&["ref", "limit", "cursor"]),
        "checkpoint_get" => Some(&["id", "include"]),
        "checkpoint_create" => Some(&["ref", "sources", "note"]),
        "checkpoint_restore" => Some(&["id", "project", "title", "task", "access"]),
        "checkpoint_delete" => Some(&["id"]),
        "agent_vault_list" => Some(&["ref"]),
        "agent_vault_set" => Some(&["ref", "name", "binding", "rev"]),
        "sandbox_close" => Some(&["ref", "rev"]),
        "actor_enqueue" => Some(&[
            "request_id",
            "message_id",
            "text",
            "priority",
            "target_turn",
        ]),
        "actor_step" => Some(&[]),
        "actor_notice_read" => Some(&["request_id"]),
        _ => None,
    };
    if let Some(names) = permitted {
        let object = p
            .as_object()
            .ok_or(Error::Protocol("RPC method params must be an object"))?;
        if object.keys().any(|name| !names.contains(&name.as_str())) {
            return Err(Error::Protocol(
                "unexpected RPC parameter; no authority flag accepted",
            ));
        }
    }
    Ok(())
}
fn vars(p: &Value) -> Result<Vec<String>> {
    let Some(args) = p.get("variables") else {
        return Ok(Vec::new());
    };
    let arr = args
        .as_array()
        .ok_or(Error::Protocol("RPC variables must be a list"))?;
    if arr.len() > 16 {
        return Err(Error::Limit("too many RPC template variables"));
    }
    let mut result = Vec::new();
    for entry in arr {
        let item = entry
            .as_str()
            .ok_or(Error::Protocol("non-text RPC variable"))?;
        if item.len() > 4096 {
            return Err(Error::Limit("RPC template variable too large"));
        }
        result.push(item.to_owned());
    }
    Ok(result)
}
fn bounded_history(turns: &[Turn], limit: usize) -> Value {
    let items = turns
        .iter()
        .rev()
        .take(limit.min(MAX_HISTORY))
        .rev()
        .map(|t| {
            let mut prompt = t.prompt.len().min(16 * 1024);
            while !t.prompt.is_char_boundary(prompt) {
                prompt -= 1;
            }
            let mut answer = t.answer.len().min(32 * 1024);
            while !t.answer.is_char_boundary(answer) {
                answer -= 1;
            }
            json!({"id":t.id,"prompt":&t.prompt[..prompt],"answer":&t.answer[..answer],
             "truncated":prompt<t.prompt.len() || answer<t.answer.len(),
             "outcome":t.outcome,"needs_reconciliation":t.needs_reconciliation})
        })
        .collect::<Vec<_>>();
    json!({"turns":items,"bounded":true})
}

enum Inbound {
    Request(Request, bool),
    LiveResult(Request, std::result::Result<Value, &'static str>),
    Invalid,
}
fn read_bounded_line<R: BufRead>(input: &mut R) -> io::Result<Option<(Vec<u8>, bool)>> {
    let mut line = Vec::new();
    let mut over = false;
    loop {
        let buf = input.fill_buf()?;
        if buf.is_empty() {
            return Ok((!line.is_empty()).then_some((line, over)));
        }
        let len = buf
            .iter()
            .position(|b| *b == b'\n')
            .map_or(buf.len(), |i| i + 1);
        let available = MAX_REQUEST_BYTES
            .saturating_add(1)
            .saturating_sub(line.len());
        let take = len.min(available);
        line.extend_from_slice(&buf[..take]);
        if take < len {
            over = true;
        }
        let newline = buf.get(len.saturating_sub(1)) == Some(&b'\n');
        input.consume(len);
        if line.len() > MAX_REQUEST_BYTES {
            over = true;
        }
        if newline {
            return Ok(Some((line, over)));
        }
    }
}
#[derive(Default)]
struct ReaderPorts {
    live_control: Option<Arc<dyn LiveControlPort>>,
    agent_create: Option<Arc<dyn AgentCreatePort>>,
    agent_close: Option<Arc<dyn AgentClosePort>>,
    agent_state: Option<Arc<dyn AgentStatePort>>,
    agent_inputs: Option<Arc<dyn AgentInputsPort>>,
    sandbox: Option<Arc<dyn SandboxPort>>,
    checkpoint_read: Option<Arc<dyn CheckpointReadPort>>,
}

fn stdin_reader<R: Read + Send + 'static>(
    input: R,
    tx: mpsc::SyncSender<Inbound>,
    active: Arc<Mutex<Option<(String, CancellationToken)>>>,
    ports: ReaderPorts,
) {
    let mut reader = BufReader::new(input);
    loop {
        let line = match read_bounded_line(&mut reader) {
            Ok(Some(v)) => v,
            Ok(None) => break,
            Err(_) => break,
        };
        let mut parsed = if line.1 {
            Inbound::Invalid
        } else {
            match serde_json::from_slice::<Request>(&line.0) {
                Ok(value) if value.validate().is_ok() => Inbound::Request(value, false),
                _ => Inbound::Invalid,
            }
        };
        if let Inbound::Request(ref request, ref mut cancel_requested) = parsed {
            if request.method == "abort"
                && validate_method_params(&request.method, &request.params).is_ok()
            {
                if let Some(target) = request.params.get("target_id").and_then(Value::as_str) {
                    if let Ok(guard) = active.lock() {
                        if let Some((id, token)) = guard.as_ref() {
                            if target == id {
                                token.cancel();
                                *cancel_requested = true;
                            }
                        }
                    }
                }
            }
        }
        // The original R12 input-reader thread exists for cooperative Abort.
        // A live Control must be applied BEFORE the owning RPC thread unblocks
        // from the current provider. Its reply is queued for orderly bounded
        // output after the original Turn reaches a factual state.
        if let Inbound::Request(request, _) = &parsed {
            if request.method == "actor_op"
                && request
                    .params
                    .get("operation")
                    .and_then(|value| value.get("op"))
                    .and_then(Value::as_str)
                    == Some("control")
            {
                if let Some(port) = &ports.live_control {
                    let outcome = (|| -> Result<Value> {
                        validate_method_params(&request.method, &request.params)?;
                        let operation = request
                            .params
                            .get("operation")
                            .cloned()
                            .ok_or(Error::Protocol("Actor Control operation missing"))?;
                        let cancel =
                            operation.get("action").and_then(Value::as_str) == Some("cancel_turn");
                        let fact = port.apply(operation)?;
                        if cancel {
                            if let Ok(guard) = active.lock() {
                                if let Some((_, token)) = guard.as_ref() {
                                    token.cancel();
                                }
                            }
                        }
                        Ok(fact)
                    })();
                    parsed =
                        Inbound::LiveResult(request.clone(), outcome.map_err(|e| error_code(&e)));
                }
            } else if request.method == "agent_create" {
                if let Some(port) = &ports.agent_create {
                    let outcome = (|| -> Result<Value> {
                        validate_method_params(&request.method, &request.params)?;
                        port.apply(&request.id, request.params.clone())
                    })();
                    parsed =
                        Inbound::LiveResult(request.clone(), outcome.map_err(|e| error_code(&e)));
                }
            } else if request.method == "agent_close" {
                if let Some(port) = &ports.agent_close {
                    let outcome = (|| -> Result<Value> {
                        validate_method_params(&request.method, &request.params)?;
                        let reference = request
                            .params
                            .get("ref")
                            .map(|value| {
                                value
                                    .as_str()
                                    .ok_or(Error::Protocol("agent_close ref must be a string"))
                            })
                            .transpose()?
                            .unwrap_or(".");
                        if reference.is_empty()
                            || reference.len() > 2048
                            || !reference.is_ascii()
                            || reference.chars().any(char::is_control)
                        {
                            return Err(Error::Protocol("invalid agent_close ref"));
                        }
                        let expected_rev = request
                            .params
                            .get("rev")
                            .map(|value| {
                                value.as_u64().ok_or(Error::Protocol(
                                    "agent_close rev must be a nonnegative integer",
                                ))
                            })
                            .transpose()?;
                        port.apply(reference, expected_rev)
                    })();
                    parsed =
                        Inbound::LiveResult(request.clone(), outcome.map_err(|e| error_code(&e)));
                }
            } else if matches!(
                request.method.as_str(),
                "agent_list"
                    | "agent_get"
                    | "agent_edit"
                    | "agent_admission"
                    | "agent_access_list"
                    | "agent_access_set"
            ) {
                if let Some(port) = &ports.agent_state {
                    let outcome = (|| -> Result<Value> {
                        validate_method_params(&request.method, &request.params)?;
                        port.apply(&request.method, request.params.clone())
                    })();
                    parsed =
                        Inbound::LiveResult(request.clone(), outcome.map_err(|e| error_code(&e)));
                }
            } else if matches!(
                request.method.as_str(),
                "agent_inputs_list"
                    | "agent_inputs_add"
                    | "agent_inputs_edit"
                    | "agent_inputs_delete"
                    | "agent_inputs_send"
            ) {
                if let Some(port) = &ports.agent_inputs {
                    let outcome = (|| -> Result<Value> {
                        validate_method_params(&request.method, &request.params)?;
                        port.apply(&request.id, &request.method, request.params.clone())
                    })();
                    parsed =
                        Inbound::LiveResult(request.clone(), outcome.map_err(|e| error_code(&e)));
                }
            } else if matches!(
                request.method.as_str(),
                "sandbox_list" | "sandbox_get" | "sandbox_sources_list" | "sandbox_close"
            ) {
                if let Some(port) = &ports.sandbox {
                    let outcome = (|| -> Result<Value> {
                        validate_method_params(&request.method, &request.params)?;
                        port.apply(&request.method, request.params.clone())
                    })();
                    parsed =
                        Inbound::LiveResult(request.clone(), outcome.map_err(|e| error_code(&e)));
                }
            } else if matches!(
                request.method.as_str(),
                "checkpoint_list" | "checkpoint_get"
            ) {
                if let Some(port) = &ports.checkpoint_read {
                    let outcome = (|| -> Result<Value> {
                        validate_method_params(&request.method, &request.params)?;
                        port.apply(&request.method, request.params.clone())
                    })();
                    parsed =
                        Inbound::LiveResult(request.clone(), outcome.map_err(|e| error_code(&e)));
                }
            }
        }
        if tx.send(parsed).is_err() {
            break;
        }
    }
}
pub fn serve_stdio<D: Driver, R: Read + Send + 'static, W: Write + Send>(
    driver: &mut D,
    reader: R,
    writer: W,
    diagnostics: bool,
) -> Result<()> {
    let mut out = Emitter::new(writer).diagnostics(diagnostics);
    let initial = driver.state();
    let mut capabilities = json!({
            "prompt":true,"abort":true,"get_state":true,
            "get_messages":true,"available_models":true,
            "prompt_template":driver.can_template(),"compact":driver.can_compact(),
            "new_session":driver.can_sessions(),"open_session":driver.can_sessions(),
            "set_model":"configured_models_only; no durable Session rebind",
            "set_effort":"host/provider-validated; no durable Session rebind",
            "trusted_bash":false,"extension_control":false,
            "actor_query":driver.actor_available(),
            "actor_op":driver.actor_available(),
            "actor_enqueue":driver.actor_available(),
            "actor_step":driver.actor_available(),
            "actor_notice_read":driver.actor_available(),
            "agent_create":driver.agent_create_port().is_some(),
            "agent_close":driver.agent_close_port().is_some(),
            "agent_list":driver.agent_state_port().is_some(),
            "agent_get":driver.agent_state_port().is_some(),
            "agent_edit":driver.agent_state_port().is_some(),
            "agent_admission":driver.agent_state_port().is_some(),
            "agent_access_list":driver.agent_state_port().is_some(),
            "agent_access_set":driver.agent_state_port().is_some(),
            "agent_inputs_list":driver.agent_inputs_port().is_some(),
            "agent_inputs_add":driver.agent_inputs_port().is_some(),
            "agent_inputs_edit":driver.agent_inputs_port().is_some(),
            "agent_inputs_delete":driver.agent_inputs_port().is_some(),
            "agent_inputs_send":driver.agent_inputs_port().is_some(),
            "sandbox_list":driver.sandbox_port().is_some(),
            "sandbox_get":driver.sandbox_port().is_some(),
            "sandbox_create":false,
            "sandbox_close":driver.sandbox_port().is_some(),
    });
    capabilities["sandbox_sources_list"] = json!(driver.sandbox_port().is_some());
    let checkpoint_read = driver.checkpoint_read_port().is_some();
    capabilities["checkpoint_list"] = json!(checkpoint_read);
    capabilities["checkpoint_get"] = json!(checkpoint_read);
    let checkpoint_create = driver.checkpoint_create_port();
    capabilities["checkpoint_create"] = json!(checkpoint_create.is_some());
    let checkpoint_restore = driver.checkpoint_restore_port();
    capabilities["checkpoint_restore"] = json!(checkpoint_restore.is_some());
    let checkpoint_delete = driver.checkpoint_delete_port();
    capabilities["checkpoint_delete"] = json!(checkpoint_delete.is_some());
    let agent_vault = driver.agent_vault_port();
    capabilities["agent_vault_list"] = json!(agent_vault.is_some());
    capabilities["agent_vault_set"] = json!(agent_vault.is_some());
    out.emit("hello",None,json!({
        "protocol":"aw.rpc","version":VERSION,"transport":"stdio",
        "capabilities":capabilities,
        "profile":initial.profile,"model":initial.model,
        "warning":"client MUST inspect durable state after transport loss; never replay an uncertain prompt"
    }))?;
    let active = Arc::new(Mutex::new(None::<(String, CancellationToken)>));
    let (tx, rx) = mpsc::sync_channel(MAX_INPUT_QUEUE);
    let guard = active.clone();
    let ports = ReaderPorts {
        live_control: driver.live_control_port(),
        agent_create: driver.agent_create_port(),
        agent_close: driver.agent_close_port(),
        agent_state: driver.agent_state_port(),
        agent_inputs: driver.agent_inputs_port(),
        sandbox: driver.sandbox_port(),
        checkpoint_read: driver.checkpoint_read_port(),
    };
    let read = thread::spawn(move || stdin_reader(reader, tx, guard, ports));
    let mut seen = BTreeSet::<String>::new();
    while let Ok(message) = rx.recv() {
        let (request, abort_requested, live_result) = match message {
            Inbound::Invalid => {
                out.emit(
                    "error",
                    None,
                    json!({"code":"invalid_frame","status":"rejected"}),
                )?;
                continue;
            }
            Inbound::Request(r, requested) => (r, requested, None),
            Inbound::LiveResult(r, result) => (r, false, Some(result)),
        };
        let id = request.id.as_str();
        if !seen.insert(request.id.clone()) {
            out.emit(
                "error",
                Some(id),
                json!({"code":"duplicate_request_id","status":"rejected",
                "note":"inspect durable Session; never resend uncertain prompt"}),
            )?;
            continue;
        }
        if seen.len() > MAX_IDS {
            out.emit(
                "error",
                Some(id),
                json!({"code":"request_id_budget_exhausted","status":"rejected"}),
            )?;
            break;
        }
        if let Some(result) = live_result {
            match result {
                Ok(value) => out.emit("response", Some(id), value)?,
                Err(code) => out.emit(
                    "error",
                    Some(id),
                    json!({
                        "code":code,"status":"rejected_or_unknown","needs_inspection":true
                    }),
                )?,
            }
            continue;
        }
        // Keep all legacy/unbound R12 Actor methods categorically unsupported
        // without parsing raw actor_key or taking any untrusted privilege hint.
        let unbound_actor = matches!(
            request.method.as_str(),
            "actor_query" | "actor_op" | "actor_enqueue" | "actor_step" | "actor_notice_read"
        ) && !driver.actor_available();
        if !unbound_actor && validate_method_params(&request.method, &request.params).is_err() {
            out.emit(
                "error",
                Some(id),
                json!({"code":"invalid_parameters","status":"rejected"}),
            )?;
            continue;
        }
        match request.method.as_str() {
            // This constructor is serialized after the connection's request-ID
            // gate, not dispatched speculatively by the live input reader.
            "checkpoint_create" if checkpoint_create.is_some() => {
                let port = checkpoint_create.as_ref().expect("enabled checkpoint port");
                match port.create(id, request.params) {
                    Ok(value) => out.emit("response", Some(id), value)?,
                    Err(error) => out.emit("error", Some(id), json!({"code":error_code(&error),"status":"rejected_or_unknown","needs_inspection":true}))?,
                }
            }
            "checkpoint_restore" if checkpoint_restore.is_some() => {
                let port = checkpoint_restore.as_ref().expect("enabled checkpoint restore port");
                match port.restore(id, request.params) {
                    Ok(value) => out.emit("response", Some(id), value)?,
                    Err(error) => out.emit("error", Some(id), json!({"code":error_code(&error),"status":"rejected_or_unknown","needs_inspection":true}))?,
                }
            }
            "checkpoint_delete" if checkpoint_delete.is_some() => {
                let port = checkpoint_delete.as_ref().expect("enabled checkpoint delete port");
                match port.delete(id, request.params) {
                    Ok(value) => out.emit("response", Some(id), value)?,
                    Err(error) => out.emit("error", Some(id), json!({"code":error_code(&error),"status":"rejected_or_unknown","needs_inspection":true}))?,
                }
            }
            "agent_vault_list" | "agent_vault_set" if agent_vault.is_some() => {
                let port = agent_vault.as_ref().expect("enabled Vault binding port");
                match port.apply(id, &request.method, request.params) {
                    Ok(value) => out.emit("response", Some(id), value)?,
                    Err(error) => out.emit("error", Some(id), json!({"code":error_code(&error),"status":"rejected_or_unknown","needs_inspection":true}))?,
                }
            }
            "actor_query" if driver.actor_available() => {
                let result = request.params.get("operation")
                    .cloned().ok_or(Error::Protocol("missing typed Actor query"))
                    .and_then(|value| driver.actor_query(value));
                match result {
                    Ok(value) => out.emit("response",Some(id),value)?,
                    Err(error) => out.emit("error",Some(id),json!({
                        "code":error_code(&error),"status":"rejected"}))?,
                }
            }
            "actor_notice_read" if driver.actor_available() => {
                let result = param_text(&request.params,"request_id",128)
                    .and_then(|req| driver.actor_notice_read(req));
                match result {
                    Ok(value) => out.emit("response",Some(id),value)?,
                    Err(error) => out.emit("error",Some(id),json!({
                        "code":error_code(&error),"status":"rejected"}))?,
                }
            }
            "actor_op" | "actor_enqueue" if driver.actor_available() => {
                // The external frame is NOT a grant and an "accepted" frame
                // is NOT evidence that the SQLite mutation was committed.
                out.emit("accepted",Some(id),json!({
                    "status":"accepted","note":"inspect the immutable Actor fact after transport loss; do not retry"}))?;
                let result = if request.method == "actor_op" {
                    request.params.get("operation")
                        .cloned().ok_or(Error::Protocol("missing typed Actor operation"))
                        .and_then(|value|driver.actor_op(value))
                } else {driver.actor_enqueue(&request.params)};
                match result {
                    Ok(value)=>out.emit("settled",Some(id),json!({
                        "status":"settled","result":value}))?,
                    Err(error)=>out.emit("settled",Some(id),json!({
                        "status":"unknown_or_rejected","code":error_code(&error),
                        "needs_inspection":true}))?,
                }
            }
            "actor_step" if driver.actor_available() => {
                let cancel=CancellationToken::new();
                *active.lock().map_err(|_|Error::Storage("RPC cancellation state poisoned"))?=
                    Some((id.to_owned(),cancel.clone()));
                out.emit("accepted",Some(id),json!({
                    "status":"accepted","note":"only original journaled Agent may claim an encrypted queued Input; no replay on loss"}))?;
                let mut event=|e:&AgentEvent|out.event(Some(id),e);
                let result=driver.actor_step(cancel,&mut event);
                *active.lock().map_err(|_|Error::Storage("RPC cancellation state poisoned"))?=None;
                match result {
                    Ok(Some(turn))=>out.emit("settled",Some(id),json!({
                        "status":"settled","result":public_turn(&turn),
                        "needs_reconciliation":turn.needs_reconciliation}))?,
                    Ok(None)=>out.emit("settled",Some(id),json!({"status":"idle","model_called":false}))?,
                    Err(error)=>out.emit("settled",Some(id),json!({
                        "status":"unknown_or_rejected","code":error_code(&error),
                        "needs_inspection":true}))?,
                }
            }
            "get_state" => out.emit("response",Some(id),serde_json::to_value(driver.state())
                .map_err(|_|Error::Protocol("state encoding failed"))?)?,
            "available_models" => {
                let state=driver.state();
                out.emit("response",Some(id),json!({
                    "configured_model":state.model,
                    "profile":state.profile,
                    "discovery_performed":false,
                    "note":"model catalog discovery and account authorization are separate explicit operations",
                }))?;
            }
            "get_messages" => {
                let limit=request.params.get("limit").and_then(Value::as_u64).unwrap_or(12);
                if limit==0 || limit>MAX_HISTORY as u64 {
                    out.emit("error",Some(id),json!({"code":"invalid_limit","status":"rejected"}))?;
                }else {
                    out.emit("response",Some(id),bounded_history(driver.turns(),limit as usize))?;
                }
            }
            "prompt"|"prompt_template" => {
                let result=(||->Result<String>{
                    let text=param_text(&request.params,"text",128*1024)?.to_owned();
                    if request.method=="prompt_template" {
                        let name=param_text(&request.params,"name",64)?;
                        driver.template(name,&text,&vars(&request.params)?)
                    }else{Ok(text)}
                })();
                match result {
                    Err(e)=>out.emit("error",Some(id),json!({"code":error_code(&e),"status":"rejected"}))?,
                    Ok(prompt)=>{
                        let cancel=CancellationToken::new();
                        *active.lock().map_err(|_|Error::Storage("RPC cancellation state poisoned"))?=
                            Some((id.to_owned(),cancel.clone()));
                        out.emit("accepted",Some(id),json!({"status":"accepted",
                            "note":"request admission, not external effect settlement"}))?;
                        let mut event=|e:&AgentEvent|out.event(Some(id),e);
                        let outcome=if request.method=="prompt_template" {
                            driver.prompt_prepared(prompt,cancel,&mut event)
                        }else{
                            driver.prompt(prompt,cancel,&mut event)
                        };
                        *active.lock().map_err(|_|Error::Storage("RPC cancellation state poisoned"))?=None;
                        match outcome{
                            Ok(turn)=>out.emit("settled",Some(id),json!({
                                "status":"settled","result":public_turn(&turn),
                                "needs_reconciliation":turn.needs_reconciliation,
                            }))?,
                            Err(e)=>out.emit("settled",Some(id),json!({
                                "status":"unknown_or_rejected","code":error_code(&e),
                                "needs_inspection":true,
                            }))?,
                        }
                    }
                }
            }
            "set_model" => {
                let input=param_text(&request.params,"model",256)
                    .and_then(|name|driver.set_model(name));
                match input {
                    Ok(())=>out.emit("response",Some(id),json!({"status":"updated",
                        "profile":driver.state().profile,"model":driver.state().model}))?,
                    Err(e)=>out.emit("error",Some(id),json!({"code":error_code(&e),
                        "status":"rejected"}))?,
                }
            }
            "set_effort" => {
                let input=param_text(&request.params,"effort",32)
                    .and_then(|value|driver.set_effort(value));
                match input {
                    Ok(())=>out.emit("response",Some(id),json!({"status":"updated"}))?,
                    Err(e)=>out.emit("error",Some(id),json!({"code":error_code(&e),
                        "status":"rejected"}))?,
                }
            }
            "new_session" if driver.can_sessions() => {
                out.emit("accepted",Some(id),json!({"status":"accepted",
                    "note":"durable Session creation, not model call; inspect on pipe loss"}))?;
                match driver.new_session() {
                    Ok(session)=>out.emit("settled",Some(id),
                        json!({"status":"settled","session":session}))?,
                    Err(error)=>out.emit("settled",Some(id),json!({
                        "status":"unknown_or_rejected","code":error_code(&error),
                        "needs_inspection":true}))?,
                }
            }
            "open_session" if driver.can_sessions() => {
                match param_text(&request.params,"session",128) {
                    Err(error)=>out.emit("error",Some(id),json!({
                        "status":"rejected","code":error_code(&error)}))?,
                    Ok(session)=>{
                        out.emit("accepted",Some(id),json!({"status":"accepted",
                            "note":"exact Session binding verification; never replays prior effects"}))?;
                        match driver.open_session(session) {
                            Ok(())=>out.emit("settled",Some(id),
                                json!({"status":"settled","session":driver.state().session}))?,
                            Err(error)=>out.emit("settled",Some(id),json!({
                                "status":"unknown_or_rejected","code":error_code(&error),
                                "needs_inspection":true}))?,
                        }
                    }
                }
            }
            "abort" => out.emit("response",Some(id),json!({
                "status":if abort_requested {"signalled"} else {"not_active"},
                "cancel_requested":abort_requested,
                "note":"abort is cooperative; it does not undo already-dispatched work",
            }))?,
            "compact" if driver.can_compact() => {
                out.emit("accepted",Some(id),json!({"status":"accepted",
                    "note":"separately paid tool-free summary; prior request state unknown on pipe loss"}))?;
                let settled=driver.compact();
                match settled {
                    Ok(through)=>out.emit("settled",Some(id),
                        json!({"status":"settled","compacted_through":through}))?,
                    Err(e)=>out.emit("settled",Some(id),json!({
                        "status":"unknown_or_rejected","code":error_code(&e),
                        "needs_inspection":true,
                    }))?,
                }
            }
            _ => out.emit("error",Some(id),json!({"code":"unsupported_method",
                "status":"rejected",
                "note":"only advertised methods are enabled; no implicit shell/extension/session grants"}))?,
        }
    }
    // Reader exits on EOF and drops the bounded sender. Joining avoids
    // leaving background reads/tasks across embedded host shutdown.
    read.join()
        .map_err(|_| Error::Storage("RPC input reader stopped unexpectedly"))?;
    Ok(())
}
pub fn emit_print<D: Driver, W: Write + Send>(
    driver: &mut D,
    prompt: String,
    writer: W,
    diagnostics: bool,
) -> Result<u8> {
    let mut out = Emitter::new(writer).diagnostics(diagnostics);
    let cancel = CancellationToken::new();
    let mut event = |e: &AgentEvent| out.event(None, e);
    let outcome = driver.prompt(prompt, cancel, &mut event);
    match outcome {
        Ok(turn) => {
            out.emit(
                "settled",
                None,
                json!({"result":public_turn(&turn),
                "needs_reconciliation":turn.needs_reconciliation}),
            )?;
            Ok(if turn.outcome == Outcome::Completed {
                0
            } else {
                1
            })
        }
        Err(e) => {
            out.emit(
                "error",
                None,
                json!({"code":error_code(&e),"needs_inspection":true}),
            )?;
            Ok(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_reader_refuses_long_lines_without_large_allocation() {
        let bytes = [
            vec![b'a'; MAX_REQUEST_BYTES * 2],
            b"\n".to_vec(),
            b"{}\n".to_vec(),
        ]
        .concat();
        let mut r = BufReader::new(bytes.as_slice());
        let first = read_bounded_line(&mut r).unwrap().unwrap();
        assert!(first.1 && first.0.len() <= MAX_REQUEST_BYTES + 1);
        assert_eq!(read_bounded_line(&mut r).unwrap().unwrap().0, b"{}\n");
        assert!(read_bounded_line(&mut r).unwrap().is_none());
    }
    #[test]
    fn frames_are_exactly_versioned_and_never_leak_provider_private_context() {
        let mut out = Emitter::new(Vec::new());
        out.event(
            Some("req1"),
            &AgentEvent::Delta {
                turn_id: 1,
                text: "payload".into(),
            },
        )
        .unwrap();
        let data = String::from_utf8(out.writer).unwrap();
        assert!(data.contains(r#""v":1"#));
        assert!(data.contains(r#""type":"delta""#));
        assert!(!data.contains("Authorization"));
        assert_eq!(data.lines().count(), 1);
    }
    #[test]
    fn duplicate_and_invalid_ids_are_rejected_before_effects() {
        for id in ["", "a b", "a\nb", "?", "x/1"] {
            assert!(Request {
                v: 1,
                id: id.into(),
                method: "prompt".into(),
                params: Value::Null
            }
            .validate()
            .is_err());
        }
        assert!(Request {
            v: 2,
            id: "ok".into(),
            method: "prompt".into(),
            params: Value::Null
        }
        .validate()
        .is_err());
    }
}

#[cfg(test)]
mod r12_protocol_tests {
    use super::*;
    use std::io::Cursor;

    struct Fake {
        turns: Vec<Turn>,
        calls: usize,
    }
    impl Driver for Fake {
        fn prompt(
            &mut self,
            text: String,
            _cancel: CancellationToken,
            event: &mut (dyn FnMut(&AgentEvent) -> Result<()> + Send),
        ) -> Result<Turn> {
            self.calls += 1;
            let id = self.calls as u64;
            event(&AgentEvent::Started {
                turn_id: id,
                profile: "safe".into(),
                model: "m".into(),
            })?;
            event(&AgentEvent::Delta {
                turn_id: id,
                text: "response".into(),
            })?;
            let turn = Turn {
                id,
                prompt: text,
                answer: "response".into(),
                model: "m".into(),
                profile: "safe".into(),
                outcome: Outcome::Completed,
                meta: None,
                error: None,
                actor: None,
                tool_records: vec![],
                invocations: vec![],
                context_messages: vec![],
                needs_reconciliation: false,
            };
            event(&AgentEvent::Finished { turn: turn.clone() })?;
            self.turns.push(turn.clone());
            Ok(turn)
        }
        fn state(&self) -> State {
            State {
                profile: "safe".into(),
                model: Some("m".into()),
                session: None,
                next_turn: (self.turns.len() + 1) as u64,
                turn_count: self.turns.len(),
                needs_reconciliation: false,
                tool_names: vec![],
            }
        }
        fn turns(&self) -> &[Turn] {
            &self.turns
        }
    }
    #[test]
    fn independent_stdio_service_rejects_duplicate_unknown_and_unsupported_without_dispatch() {
        let mut fake = Fake {
            turns: vec![],
            calls: 0,
        };
        let lines=[
            r#"{"v":1,"id":"get","method":"get_state","params":{}}"#,
            r#"{"v":1,"id":"run","method":"prompt","params":{"text":"one"}}"#,
            r#"{"v":1,"id":"run","method":"prompt","params":{"text":"never"}}"#,
            r#"{"v":1,"id":"bad","method":"prompt","params":{"text":"never","tools_mode":"trusted"}}"#,
            r#"{"v":1,"id":"bash","method":"trusted_bash","params":{"command":"echo never"}}"#,
            r#"{"v":1,"id":"actorread","method":"actor_query","params":{"actor_key":"r9g1a1g1"}}"#,
            r#"{"v":1,"id":"actorctrl","method":"actor_op","params":{"operation":"cancel","turn_id":1}}"#,
            r#"{"v":1,"id":"cp-delete","method":"checkpoint_delete","params":{"id":"cp_fixture"}}"#,
            r#"{"v":1,"id":"vault-set","method":"agent_vault_set","params":{"name":"signing","binding":null,"rev":1}}"#,
        ].join("\n")+"\n";
        let mut bytes = Vec::new();
        serve_stdio(&mut fake, Cursor::new(lines), &mut bytes, false).unwrap();
        assert_eq!(fake.calls, 1);
        let json = String::from_utf8(bytes).unwrap();
        let frames = json
            .lines()
            .map(|s| serde_json::from_str::<Value>(s).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(frames[0]["type"], "hello");
        assert_eq!(
            frames[0]["data"]["capabilities"]["checkpoint_delete"],
            false
        );
        assert_eq!(frames[0]["data"]["capabilities"]["agent_vault_set"], false);
        assert_eq!(frames[0]["data"]["capabilities"]["agent_vault_list"], false);
        assert_eq!(frames[1]["type"], "response");
        assert!(frames
            .iter()
            .any(|v| v["type"] == "accepted" && v["id"] == "run"));
        assert!(frames
            .iter()
            .any(|v| v["type"] == "settled" && v["id"] == "run"));
        assert!(frames
            .iter()
            .any(|v| v["data"]["code"] == "duplicate_request_id"));
        assert!(frames
            .iter()
            .any(|v| v["data"]["code"] == "invalid_parameters"));
        assert!(frames
            .iter()
            .any(|v| v["data"]["code"] == "unsupported_method"));
        assert_eq!(
            frames
                .iter()
                .filter(|v| v["data"]["code"] == "unsupported_method")
                .count(),
            5,
            "unbound stdio must never permit raw Actor, checkpoint or Vault mutation authority"
        );
        assert_eq!(frames.iter().filter(|v| v["type"] == "started").count(), 1);
    }
    #[test]
    fn oversize_frame_is_rejected_but_next_valid_request_still_works_without_extra_memory() {
        let mut bytes = vec![b'x'; MAX_REQUEST_BYTES + 50_000];
        bytes.extend_from_slice(b"\n");
        bytes.extend_from_slice(br#"{"v":1,"id":"state","method":"get_state"}"#);
        bytes.push(b'\n');
        let mut fake = Fake {
            turns: vec![],
            calls: 0,
        };
        let mut output = Vec::new();
        serve_stdio(&mut fake, Cursor::new(bytes), &mut output, false).unwrap();
        assert_eq!(fake.calls, 0);
        let frames = String::from_utf8(output).unwrap();
        assert_eq!(frames.lines().count(), 3);
        let parsed = frames
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(parsed[1]["data"]["code"], "invalid_frame");
        assert_eq!(parsed[2]["type"], "response");
        assert_eq!(parsed[2]["id"], "state");
        assert_eq!(parsed[2]["data"]["next_turn"], 1);
    }
    #[test]
    fn public_turn_projection_excludes_private_signed_context() {
        let turn = Turn {
            id: 1,
            prompt: "user".into(),
            answer: "public".into(),
            model: "m".into(),
            profile: "p".into(),
            outcome: Outcome::Completed,
            meta: None,
            error: None,
            actor: None,
            tool_records: vec![],
            invocations: vec![],
            context_messages: vec![aw_agent_core::Message {
                id: 9,
                role: aw_agent_core::conversation::Role::Assistant,
                content: "visible".into(),
                tool_calls: vec![],
                tool_call_id: None,
                name: None,
                provider_context: Some("PRIVATE_NATIVE_THINKING_SIGNATURE".into()),
            }],
            needs_reconciliation: false,
        };
        let serialized = public_turn(&turn).to_string();
        assert!(serialized.contains("public"));
        assert!(!serialized.contains("PRIVATE_NATIVE_THINKING_SIGNATURE"));
        assert!(!serialized.contains("provider_context"));
        let mut writer = Emitter::new(Vec::new());
        writer.event(None, &AgentEvent::Finished { turn }).unwrap();
        let stdout = String::from_utf8(writer.writer).unwrap();
        assert!(!stdout.contains("PRIVATE_NATIVE_THINKING_SIGNATURE"));
    }
}

#[cfg(test)]
mod aw_p6_9c_protocol_tests {
    use super::*;
    use std::io::Cursor;

    #[derive(Default)]
    struct FakeAgentCreatePort {
        calls: Mutex<Vec<(String, Value)>>,
    }
    impl AgentCreatePort for FakeAgentCreatePort {
        fn apply(&self, request_id: &str, params: Value) -> Result<Value> {
            self.calls
                .lock()
                .unwrap()
                .push((request_id.to_owned(), params));
            Ok(json!({"ref":"/d:0/i:0/p:0/a:created","state":"open"}))
        }
    }

    #[derive(Default)]
    struct FakeAgentStatePort {
        calls: Mutex<Vec<(String, Value)>>,
    }
    impl AgentStatePort for FakeAgentStatePort {
        fn apply(&self, method: &str, params: Value) -> Result<Value> {
            self.calls.lock().unwrap().push((method.to_owned(), params));
            Ok(json!({"ref":"/d:d/i:i/p:p/a:a","rev":1}))
        }
    }

    #[derive(Default)]
    struct FakeAgentInputsPort {
        calls: Mutex<Vec<(String, String, Value)>>,
    }
    impl AgentInputsPort for FakeAgentInputsPort {
        fn apply(&self, request_id: &str, method: &str, params: Value) -> Result<Value> {
            self.calls
                .lock()
                .unwrap()
                .push((request_id.to_owned(), method.to_owned(), params));
            Ok(json!({"agent_ref":"/d:d/i:i/p:p/a:a","inputs_rev":1,"items":[]}))
        }
    }

    #[derive(Default)]
    struct FakeSandboxPort {
        calls: Mutex<Vec<(String, Value)>>,
    }
    impl SandboxPort for FakeSandboxPort {
        fn apply(&self, method: &str, params: Value) -> Result<Value> {
            self.calls.lock().unwrap().push((method.to_owned(), params));
            Ok(json!({"items":[],"next_cursor":null}))
        }
    }

    impl CheckpointReadPort for FakeSandboxPort {
        fn apply(&self, method: &str, params: Value) -> Result<Value> {
            SandboxPort::apply(self, method, params)
        }
    }

    impl CheckpointCreatePort for FakeSandboxPort {
        fn create(&self, request_id: &str, params: Value) -> Result<Value> {
            self.calls
                .lock()
                .unwrap()
                .push((format!("checkpoint_create:{request_id}"), params));
            Ok(json!({"id":"cp_fixture"}))
        }
    }

    impl CheckpointRestorePort for FakeSandboxPort {
        fn restore(&self, request_id: &str, params: Value) -> Result<Value> {
            self.calls
                .lock()
                .unwrap()
                .push((format!("checkpoint_restore:{request_id}"), params));
            Ok(json!({"agent":{"ref":"/d:d/i:i/p:p/a:restored"}}))
        }
    }

    impl CheckpointDeletePort for FakeSandboxPort {
        fn delete(&self, request_id: &str, params: Value) -> Result<Value> {
            self.calls
                .lock()
                .unwrap()
                .push((format!("checkpoint_delete:{request_id}"), params));
            Ok(json!({"status":"retired"}))
        }
    }

    impl AgentVaultPort for FakeSandboxPort {
        fn apply(&self, request_id: &str, method: &str, params: Value) -> Result<Value> {
            self.calls
                .lock()
                .unwrap()
                .push((format!("{method}:{request_id}"), params));
            Ok(json!({"status":"applied","rev":2}))
        }
    }

    struct BoundFake {
        query_calls: usize,
        mutation_calls: usize,
        enqueue_calls: usize,
        step_calls: usize,
        prompts: usize,
        create: Arc<FakeAgentCreatePort>,
        state: Arc<FakeAgentStatePort>,
        inputs: Arc<FakeAgentInputsPort>,
        sandbox: Arc<FakeSandboxPort>,
    }
    impl Driver for BoundFake {
        fn actor_available(&self) -> bool {
            true
        }
        fn agent_create_port(&self) -> Option<Arc<dyn AgentCreatePort>> {
            Some(self.create.clone())
        }
        fn agent_state_port(&self) -> Option<Arc<dyn AgentStatePort>> {
            Some(self.state.clone())
        }
        fn agent_inputs_port(&self) -> Option<Arc<dyn AgentInputsPort>> {
            Some(self.inputs.clone())
        }
        fn sandbox_port(&self) -> Option<Arc<dyn SandboxPort>> {
            Some(self.sandbox.clone())
        }
        fn checkpoint_read_port(&self) -> Option<Arc<dyn CheckpointReadPort>> {
            Some(self.sandbox.clone())
        }
        fn checkpoint_create_port(&self) -> Option<Arc<dyn CheckpointCreatePort>> {
            Some(self.sandbox.clone())
        }
        fn checkpoint_restore_port(&self) -> Option<Arc<dyn CheckpointRestorePort>> {
            Some(self.sandbox.clone())
        }
        fn checkpoint_delete_port(&self) -> Option<Arc<dyn CheckpointDeletePort>> {
            Some(self.sandbox.clone())
        }
        fn agent_vault_port(&self) -> Option<Arc<dyn AgentVaultPort>> {
            Some(self.sandbox.clone())
        }
        fn actor_query(&mut self, operation: Value) -> Result<Value> {
            self.query_calls += 1;
            Ok(json!({"kind":"queued","opaque_no_keys":true,"observed":operation}))
        }
        fn actor_op(&mut self, operation: Value) -> Result<Value> {
            self.mutation_calls += 1;
            Ok(json!({"kind":"queue","revision":2,"observed":operation}))
        }
        fn actor_enqueue(&mut self, params: &Value) -> Result<Value> {
            self.enqueue_calls += 1;
            Ok(json!({"kind":"queue","request_id":params["request_id"],"state":"pending"}))
        }
        fn actor_notice_read(&mut self, request_id: &str) -> Result<Value> {
            Ok(json!({"kind":"notice_private","request_id":request_id,"text":"data only"}))
        }
        fn actor_step(
            &mut self,
            _: CancellationToken,
            _: &mut (dyn FnMut(&AgentEvent) -> Result<()> + Send),
        ) -> Result<Option<Turn>> {
            self.step_calls += 1;
            Ok(None)
        }
        fn prompt(
            &mut self,
            _: String,
            _: CancellationToken,
            _: &mut (dyn FnMut(&AgentEvent) -> Result<()> + Send),
        ) -> Result<Turn> {
            self.prompts += 1;
            Err(Error::Denied("fake opt-in driver cannot run raw prompts"))
        }
        fn state(&self) -> State {
            State {
                profile: "test".into(),
                model: Some("m".into()),
                session: Some("s_1".into()),
                next_turn: 1,
                turn_count: 0,
                needs_reconciliation: false,
                tool_names: vec![],
            }
        }
        fn turns(&self) -> &[Turn] {
            &[]
        }
    }
    #[test]
    fn aw_p6_9c_strict_frame_params_and_no_raw_actor_key_grants() {
        let frames=[
            r#"{"v":1,"id":"ok1","method":"actor_query","params":{"operation":{"op":"list_queue"}}}"#,
            r#"{"v":1,"id":"raw","method":"actor_query","params":{"operation":{"op":"list_queue"},"actor_key":"r1g1a1g1"}}"#,
            r#"{"v":1,"id":"write","method":"actor_op","params":{"operation":{"op":"inspect_queue","request_id":"q","note":"review"}}}"#,
            r#"{"v":1,"id":"send","method":"actor_enqueue","params":{"request_id":"q","message_id":1,"text":"private"}}"#,
            r#"{"v":1,"id":"send","method":"actor_enqueue","params":{"request_id":"second","message_id":2,"text":"never accepted"}}"#,
            r#"{"v":1,"id":"grant","method":"actor_enqueue","params":{"request_id":"evil","message_id":3,"text":"never accepted","grant":"tool:process"}}"#,
            r#"{"v":1,"id":"read","method":"actor_notice_read","params":{"request_id":"notice_1"}}"#,
            r#"{"v":1,"id":"step","method":"actor_step","params":{}}"#,
            r#"{"v":1,"id":"create","method":"agent_create","params":{"title":"child","task":"work","access":"private","sandbox":"auto"}}"#,
            r#"{"v":1,"id":"create-provider","method":"agent_create","params":{"title":"child","provider":"evil"}}"#,
            r#"{"v":1,"id":"agents","method":"agent_list","params":{"limit":8}}"#,
            r#"{"v":1,"id":"agent-get","method":"agent_get","params":{"ref":"."}}"#,
            r#"{"v":1,"id":"agent-edit","method":"agent_edit","params":{"ref":".","title":"renamed","rev":1}}"#,
            r#"{"v":1,"id":"agent-admission","method":"agent_admission","params":{"ref":".","state":"held","rev":1}}"#,
            r#"{"v":1,"id":"agent-access","method":"agent_access_list","params":{"ref":"."}}"#,
            r#"{"v":1,"id":"agent-grant","method":"agent_access_set","params":{"ref":".","principal":"/d:d/i:other","permissions":["view"],"rev":1}}"#,
            r#"{"v":1,"id":"agent-provider","method":"agent_get","params":{"ref":".","provider":"evil"}}"#,
            r#"{"v":1,"id":"agent-sandbox","method":"agent_edit","params":{"ref":".","title":"x","rev":1,"sandbox":"/d:d/i:i/p:p/s:0"}}"#,
            r#"{"v":1,"id":"agent-owner","method":"agent_access_set","params":{"ref":".","principal":"/d:d/i:other","permissions":["view"],"rev":1,"owner":"/d:d/i:evil"}}"#,
            r#"{"v":1,"id":"inputs-list","method":"agent_inputs_list","params":{"ref":"."}}"#,
            r#"{"v":1,"id":"inputs-raw","method":"agent_inputs_add","params":{"items":[],"provider":"evil"}}"#,
            r#"{"v":1,"id":"sandboxes","method":"sandbox_list","params":{"limit":8}}"#,
            r#"{"v":1,"id":"sources","method":"sandbox_sources_list","params":{"ref":"/d:d/i:i/p:p/s:0"}}"#,
            r#"{"v":1,"id":"checkpoints","method":"checkpoint_list","params":{"ref":".","limit":8}}"#,
            r#"{"v":1,"id":"checkpoint","method":"checkpoint_get","params":{"id":"cp_fixture"}}"#,
            r#"{"v":1,"id":"checkpoint-raw","method":"checkpoint_get","params":{"id":"cp_fixture","source_ref":"private"}}"#,
            r#"{"v":1,"id":"checkpoint-restore","method":"checkpoint_restore","params":{"id":"cp_fixture"}}"#,
            r#"{"v":1,"id":"checkpoint-restore","method":"checkpoint_restore","params":{"id":"other"}}"#,
            r#"{"v":1,"id":"restore-root","method":"checkpoint_restore","params":{"id":"cp_fixture","root":"/private"}}"#,
            r#"{"v":1,"id":"cp-delete","method":"checkpoint_delete","params":{"id":"cp_fixture"}}"#,
            r#"{"v":1,"id":"cp-delete","method":"checkpoint_delete","params":{"id":"other"}}"#,
            r#"{"v":1,"id":"delete-raw","method":"checkpoint_delete","params":{"id":"cp_fixture","cleanup":true}}"#,
            r#"{"v":1,"id":"vault-list","method":"agent_vault_list","params":{}}"#,
            r#"{"v":1,"id":"vault-set","method":"agent_vault_set","params":{"name":"signing","binding":null,"rev":1}}"#,
            r#"{"v":1,"id":"vault-set","method":"agent_vault_set","params":{"name":"other","binding":null,"rev":1}}"#,
            r#"{"v":1,"id":"vault-raw","method":"agent_vault_set","params":{"name":"signing","binding":null,"rev":1,"root":"/private"}}"#,
            r#"{"v":1,"id":"cp-create","method":"checkpoint_create","params":{"sources":"none"}}"#,
            r#"{"v":1,"id":"cp-create","method":"checkpoint_create","params":{"sources":"auto"}}"#,
            r#"{"v":1,"id":"cp-raw","method":"checkpoint_create","params":{"sources":"none","authority":true}}"#,
            r#"{"v":1,"id":"sandbox-layout","method":"sandbox_get","params":{"ref":"/d:d/i:i/p:p/s:0","layout":"raw"}}"#,
            r#"{"v":1,"id":"danger","method":"trusted_bash","params":{"command":"echo no"}}"#,
        ].join("\n")+"\n";
        let create = Arc::new(FakeAgentCreatePort::default());
        let state = Arc::new(FakeAgentStatePort::default());
        let inputs = Arc::new(FakeAgentInputsPort::default());
        let sandbox = Arc::new(FakeSandboxPort::default());
        let mut fake = BoundFake {
            query_calls: 0,
            mutation_calls: 0,
            enqueue_calls: 0,
            step_calls: 0,
            prompts: 0,
            create: create.clone(),
            state: state.clone(),
            inputs: inputs.clone(),
            sandbox: sandbox.clone(),
        };
        let mut bytes = Vec::new();
        serve_stdio(&mut fake, Cursor::new(frames), &mut bytes, false).unwrap();
        let rows: Vec<Value> = String::from_utf8(bytes)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows[0]["data"]["capabilities"]["actor_query"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["agent_create"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["agent_list"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["agent_get"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["agent_edit"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["agent_admission"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["agent_access_list"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["agent_access_set"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["agent_inputs_list"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["agent_inputs_add"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["agent_inputs_edit"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["agent_inputs_delete"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["agent_inputs_send"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["sandbox_list"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["sandbox_get"], true);
        for method in ["sandbox_sources_list", "checkpoint_list", "checkpoint_get"] {
            assert_eq!(rows[0]["data"]["capabilities"][method], true);
        }
        assert_eq!(rows[0]["data"]["capabilities"]["checkpoint_create"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["checkpoint_restore"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["checkpoint_delete"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["agent_vault_list"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["agent_vault_set"], true);
        assert_eq!(rows[0]["data"]["capabilities"]["sandbox_create"], false);
        assert_eq!(rows[0]["data"]["capabilities"]["sandbox_close"], true);
        assert_eq!(fake.query_calls, 1);
        assert_eq!(fake.mutation_calls, 1);
        assert_eq!(fake.enqueue_calls, 1);
        assert_eq!(fake.step_calls, 1);
        assert_eq!(fake.prompts, 0);
        let create_calls = create.calls.lock().unwrap();
        assert_eq!(create_calls.len(), 1);
        assert_eq!(create_calls[0].0, "create");
        assert_eq!(create_calls[0].1["task"], "work");
        let state_calls = state.calls.lock().unwrap();
        assert_eq!(state_calls.len(), 6);
        assert_eq!(
            state_calls
                .iter()
                .map(|(method, _)| method.as_str())
                .collect::<Vec<_>>(),
            [
                "agent_list",
                "agent_get",
                "agent_edit",
                "agent_admission",
                "agent_access_list",
                "agent_access_set",
            ]
        );
        let input_calls = inputs.calls.lock().unwrap();
        assert_eq!(input_calls.len(), 1);
        assert_eq!(input_calls[0].0, "inputs-list");
        assert_eq!(input_calls[0].1, "agent_inputs_list");
        let sandbox_calls = sandbox.calls.lock().unwrap();
        assert_eq!(
            sandbox_calls
                .iter()
                .map(|(method, _)| method.as_str())
                .collect::<Vec<_>>(),
            [
                "sandbox_list",
                "sandbox_sources_list",
                "checkpoint_list",
                "checkpoint_get",
                "checkpoint_restore:checkpoint-restore",
                "checkpoint_delete:cp-delete",
                "agent_vault_list:vault-list",
                "agent_vault_set:vault-set",
                "checkpoint_create:cp-create"
            ]
        );
        assert_eq!(sandbox_calls[0].1["limit"], 8);
        assert!(rows
            .iter()
            .any(|r| r["id"] == "vault-set" && r["data"]["status"] == "applied"));
        assert!(rows
            .iter()
            .any(|r| r["id"] == "vault-set" && r["data"]["code"] == "duplicate_request_id"));
        assert!(rows
            .iter()
            .any(|r| r["id"] == "vault-raw" && r["data"]["code"] == "invalid_parameters"));
        assert!(rows
            .iter()
            .any(|r| r["id"] == "cp-delete" && r["data"]["status"] == "retired"));
        assert!(rows
            .iter()
            .any(|r| r["id"] == "cp-delete" && r["data"]["code"] == "duplicate_request_id"));
        assert!(rows
            .iter()
            .any(|r| r["id"] == "delete-raw" && r["data"]["code"] == "invalid_parameters"));
        assert!(rows
            .iter()
            .any(|r| r["id"] == "cp-create" && r["data"]["id"] == "cp_fixture"));
        assert!(rows
            .iter()
            .any(|r| r["id"] == "cp-create" && r["data"]["code"] == "duplicate_request_id"));
        assert!(rows
            .iter()
            .any(|r| r["id"] == "cp-raw" && r["data"]["code"] == "invalid_parameters"));
        assert!(rows
            .iter()
            .any(|r| r["id"] == "checkpoint-raw" && r["data"]["code"] == "protocol"));
        assert!(rows.iter().any(|r| r["id"] == "checkpoint-restore"
            && r["data"]["agent"]["ref"] == "/d:d/i:i/p:p/a:restored"));
        assert!(rows.iter().any(
            |r| r["id"] == "checkpoint-restore" && r["data"]["code"] == "duplicate_request_id"
        ));
        assert!(rows
            .iter()
            .any(|r| r["id"] == "restore-root" && r["data"]["code"] == "invalid_parameters"));
        assert!(rows
            .iter()
            .any(|r| r["id"] == "raw" && r["data"]["code"] == "invalid_parameters"));
        assert!(rows
            .iter()
            .any(|r| r["id"] == "grant" && r["data"]["code"] == "invalid_parameters"));
        assert!(rows
            .iter()
            .any(|r| r["id"] == "create-provider" && r["data"]["code"] == "protocol"));
        for id in ["agent-provider", "agent-sandbox", "agent-owner"] {
            assert!(rows
                .iter()
                .any(|r| r["id"] == id && r["data"]["code"] == "protocol"));
        }
        assert!(rows
            .iter()
            .any(|r| r["id"] == "inputs-raw" && r["data"]["code"] == "protocol"));
        assert!(rows
            .iter()
            .any(|r| r["id"] == "sandbox-layout" && r["data"]["code"] == "protocol"));
        assert!(rows
            .iter()
            .any(|r| r["id"] == "send" && r["data"]["code"] == "duplicate_request_id"));
        assert!(rows
            .iter()
            .any(|r| r["id"] == "danger" && r["data"]["code"] == "unsupported_method"));
        assert!(rows
            .iter()
            .any(|r| r["id"] == "step" && r["data"]["status"] == "idle"));
        let write: Vec<&Value> = rows.iter().filter(|r| r["id"] == "write").collect();
        assert_eq!(write[0]["type"], "accepted");
        assert_eq!(write[1]["type"], "settled");
    }
    #[test]
    fn aw_p6_9c_unbound_driver_remains_unadvertised_and_unsupported() {
        // Existing original R12 mock is deliberately not changed or granted
        // Actor authority. Default trait path denies every new method.
        struct NoActor;
        impl Driver for NoActor {
            fn prompt(
                &mut self,
                _: String,
                _: CancellationToken,
                _: &mut (dyn FnMut(&AgentEvent) -> Result<()> + Send),
            ) -> Result<Turn> {
                panic!("actor queries must never dispatch provider");
            }
            fn state(&self) -> State {
                State {
                    profile: "safe".into(),
                    model: None,
                    session: None,
                    next_turn: 1,
                    turn_count: 0,
                    needs_reconciliation: false,
                    tool_names: vec![],
                }
            }
            fn turns(&self) -> &[Turn] {
                &[]
            }
        }
        let cases = [
            "actor_query",
            "actor_op",
            "actor_enqueue",
            "actor_step",
            "actor_notice_read",
        ];
        let body = cases
            .iter()
            .enumerate()
            .map(|(i, method)| {
                json!({
                    "v":1,"id":format!("r{i}"),"method":method,"params":{"actor_key":"forged"}
                })
                .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let mut out = Vec::new();
        serve_stdio(&mut NoActor, Cursor::new(body), &mut out, false).unwrap();
        let result: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(result.len(), 6);
        assert_eq!(result[0]["data"]["capabilities"]["actor_query"], false);
        assert_eq!(result[0]["data"]["capabilities"]["agent_list"], false);
        assert_eq!(result[0]["data"]["capabilities"]["agent_access_set"], false);
        for event in result.iter().skip(1) {
            assert_eq!(event["data"]["code"], "unsupported_method", "{event}");
        }
    }
}
