//! Unified model-facing finite Job controls. These tools project the one
//! JobManager/Process owner; they do not create a second scheduler or store.

use crate::{
    agent_jobs::{
        AgentJobCancel, AgentJobGet, AgentJobListRequest, AgentJobRead, AgentJobSend,
        AgentJobSubscribe, AgentSendInput, AgentSendMode, AgentWakeRecipient,
    },
    job_runtime::{ControlReplayDecision, ControlReplayReceipt, JobRuntime},
    Error, Result,
};
use aw_agent_core::{Tool, ToolContext, ToolEffect, ToolFuture, ToolOutput, ToolSpec};
use aw_tools::{
    filesystem::{
        job::{Lifecycle, NodeState},
        job_manager::{FiniteJobKind, OutputPage, OutputStream, Snapshot, MAX_JOB_LIST_PAGE},
    },
    job_event::{EventRule, EventRuleSet, JobEventKind, JobWakeMode},
};
use base64::Engine as _;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use std::{sync::Arc, time::Duration};

const MAX_WAIT_MS: u64 = 30_000;

pub(crate) fn parse_event_rules(value: &Value) -> Result<EventRuleSet> {
    let events = value
        .as_array()
        .ok_or(Error::Denied("Job event rules must be an array"))?;
    if events.len() > aw_tools::job_event::MAX_EVENT_RULES {
        return Err(Error::Denied("too many Job event rules"));
    }
    let mut rules = Vec::with_capacity(events.len());
    for raw in events {
        let tuple = raw
            .as_array()
            .ok_or(Error::Denied("Job event rule must be a tuple"))?;
        if !(1..=2).contains(&tuple.len()) {
            return Err(Error::Denied(
                "Job event rule must be [event] or [event,mode]",
            ));
        }
        let event = match tuple[0].as_str() {
            Some("result") => JobEventKind::Result,
            Some("settled") => JobEventKind::Settled,
            Some("attention") => JobEventKind::Attention,
            _ => return Err(Error::Denied("unknown Job event name")),
        };
        let mode = match tuple.get(1).and_then(Value::as_str) {
            None | Some("auto") => JobWakeMode::Auto,
            Some("queue") => JobWakeMode::Queue,
            Some("steer") => JobWakeMode::Steer,
            Some("interrupt") => JobWakeMode::Interrupt,
            Some(_) => return Err(Error::Denied("unknown Job wake mode")),
        };
        rules.push(EventRule::new(event, mode));
    }
    EventRuleSet::new(rules).map_err(Error::Denied)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JobsKind {
    List,
    Get,
    Read,
    Ack,
    Send,
    Cancel,
    Subscribe,
}

#[derive(Clone)]
pub(crate) struct JobsTool {
    runtime: Arc<JobRuntime>,
    kind: JobsKind,
}

impl JobsTool {
    pub(crate) fn new(runtime: Arc<JobRuntime>, kind: JobsKind) -> Self {
        Self { runtime, kind }
    }

    fn object<'a>(
        arguments: &'a Value,
        allowed: &[&str],
    ) -> Result<&'a serde_json::Map<String, Value>> {
        let object = arguments
            .as_object()
            .ok_or(Error::Denied("jobs_* arguments must be an object"))?;
        if object.keys().any(|key| !allowed.contains(&key.as_str())) {
            return Err(Error::Denied("unknown jobs_* argument"));
        }
        Ok(object)
    }

    fn reference(arguments: &Value, default_current: bool) -> Result<&str> {
        let reference = match arguments.get("ref") {
            None | Some(Value::Null) if default_current => ".",
            Some(Value::String(reference)) => reference.as_str(),
            _ => return Err(Error::Denied("jobs_* requires a valid ref")),
        };
        if reference.is_empty() || reference.len() > 2048 || reference.chars().any(char::is_control)
        {
            return Err(Error::Denied("invalid execution ref"));
        }
        Ok(reference)
    }

    fn revision(arguments: &Value) -> Result<Option<u64>> {
        match arguments.get("rev") {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value
                .as_u64()
                .filter(|value| *value != 0)
                .map(Some)
                .ok_or(Error::Denied("rev must be a non-zero integer")),
        }
    }

    fn view(arguments: &Value) -> Result<Option<&str>> {
        match arguments.get("view") {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(value))
                if matches!(
                    value.as_str(),
                    "result"
                        | "stdout"
                        | "stderr"
                        | "messages"
                        | "tool_calls"
                        | "tool_outputs"
                        | "trace"
                ) =>
            {
                Ok(Some(value.as_str()))
            }
            _ => Err(Error::Denied("invalid jobs_read view")),
        }
    }

    fn filter(arguments: &Value) -> Result<Option<Value>> {
        let Some(filter) = arguments.get("filter").filter(|value| !value.is_null()) else {
            return Ok(None);
        };
        match filter {
            Value::String(value) if matches!(value.as_str(), "default" | "none") => {
                Ok(Some(filter.clone()))
            }
            Value::Object(object) => {
                const ALLOWED: &[&str] = &[
                    "result",
                    "messages",
                    "tool_calls",
                    "tool_outputs",
                    "tools",
                    "exclude",
                ];
                if object.keys().any(|key| !ALLOWED.contains(&key.as_str())) {
                    return Err(Error::Denied("unknown Agent history filter field"));
                }
                for name in [
                    "result",
                    "messages",
                    "tool_calls",
                    "tool_outputs",
                    "exclude",
                ] {
                    if object.get(name).is_some_and(|value| !value.is_boolean()) {
                        return Err(Error::Denied("Agent history filter flags must be boolean"));
                    }
                }
                let result = object.get("result").and_then(Value::as_bool) == Some(true);
                let messages = object.get("messages").and_then(Value::as_bool) == Some(true);
                let tool_calls = object.get("tool_calls").and_then(Value::as_bool) == Some(true);
                let tool_outputs =
                    object.get("tool_outputs").and_then(Value::as_bool) == Some(true);
                if !(result || messages || tool_calls || tool_outputs) {
                    return Err(Error::Denied(
                        "Agent history filter requires at least one true category",
                    ));
                }
                if let Some(tools) = object.get("tools") {
                    if !(tool_calls || tool_outputs) {
                        return Err(Error::Denied(
                            "Agent history filter tools require tool_calls or tool_outputs",
                        ));
                    }
                    let tools = tools
                        .as_array()
                        .ok_or(Error::Denied("Agent history filter tools must be an array"))?;
                    let mut seen = std::collections::BTreeSet::new();
                    if tools.len() > 64
                        || tools.iter().any(|value| {
                            value.as_str().is_none_or(|tool| {
                                tool.is_empty() || tool.len() > 128 || !seen.insert(tool.to_owned())
                            })
                        })
                    {
                        return Err(Error::Denied(
                            "Agent history filter tools must be unique bounded names",
                        ));
                    }
                }
                Ok(Some(filter.clone()))
            }
            _ => Err(Error::Denied("invalid Agent history filter")),
        }
    }

    fn send_mode(arguments: &Value) -> Result<AgentSendMode> {
        match arguments.get("mode").and_then(Value::as_str) {
            None | Some("auto") => Ok(AgentSendMode::Auto),
            Some("queue") => Ok(AgentSendMode::Queue),
            Some("steer") => Ok(AgentSendMode::Steer),
            Some("interrupt") => Ok(AgentSendMode::Interrupt),
            Some(_) => Err(Error::Denied("invalid jobs_send mode")),
        }
    }

    fn send_input(arguments: &Value) -> Result<AgentSendInput> {
        let input = arguments
            .get("input")
            .and_then(Value::as_object)
            .ok_or(Error::Denied("jobs_send requires typed input"))?;
        if let Some(text) = input.get("text").and_then(Value::as_str) {
            return Ok(AgentSendInput::Text(text.to_owned()));
        }
        if input.get("eof").and_then(Value::as_bool) == Some(true) {
            return Ok(AgentSendInput::Eof);
        }
        Err(Error::Denied("invalid jobs_send input"))
    }

    fn event_rules(arguments: &Value) -> Result<EventRuleSet> {
        let events = arguments
            .get("events")
            .ok_or(Error::Denied("jobs_subscribe requires events array"))?;
        parse_event_rules(events)
    }

    fn canonical_event_value(rules: &EventRuleSet) -> Value {
        Value::Array(
            rules
                .as_slice()
                .iter()
                .map(|rule| json!([rule.event.as_str(), rule.mode.as_str()]))
                .collect(),
        )
    }

    fn encoding(arguments: &Value) -> Result<&str> {
        match arguments.get("encoding").and_then(Value::as_str) {
            None | Some("text") => Ok("text"),
            Some("base64") => Ok("base64"),
            Some(_) => Err(Error::Denied("encoding must be text or base64")),
        }
    }

    fn state_label(snapshot: &Snapshot) -> &'static str {
        match snapshot.lifecycle {
            Lifecycle::Running => "running",
            Lifecycle::Cancelling | Lifecycle::Revoking => "cancelling",
            Lifecycle::Terminal if snapshot.stop_reason.is_some() => "cancelled",
            Lifecycle::Terminal => "settled",
        }
    }

    fn render_snapshot(reference: &str, snapshot: &Snapshot) -> String {
        let mut line = format!(
            "{reference} {} {} rev={}",
            snapshot.label,
            Self::state_label(snapshot),
            snapshot.sequence
        );
        if snapshot.lifecycle != Lifecycle::Terminal && snapshot.total > 1 {
            line.push_str(&format!(" {}/{}", snapshot.finished, snapshot.total));
        }
        if snapshot.lifecycle == Lifecycle::Terminal {
            let outcome = snapshot
                .summary
                .map_or("unknown", |summary| summary.0.as_str());
            line.push_str(&format!(" outcome={outcome}"));
            if let Some((_, more, incomplete)) = snapshot.summary {
                if more {
                    line.push_str(" more");
                }
                if incomplete {
                    line.push_str(" incomplete");
                }
            }
        }
        line
    }

    fn control_fingerprint(
        operation: &str,
        canonical_ref: &str,
        arguments: &Value,
    ) -> Result<[u8; 32]> {
        let mut semantic = arguments.clone();
        let object = semantic
            .as_object_mut()
            .ok_or(Error::Denied("jobs control arguments must be an object"))?;
        object.insert("ref".into(), Value::String(canonical_ref.to_owned()));
        let encoded = serde_json::to_vec(&semantic)
            .map_err(|_| Error::Protocol("cannot encode control replay fingerprint"))?;
        let mut hash = Sha256::new();
        hash.update(b"aw-jobs-control-request-v1\0");
        hash.update((operation.len() as u64).to_le_bytes());
        hash.update(operation.as_bytes());
        hash.update((encoded.len() as u64).to_le_bytes());
        hash.update(encoded);
        Ok(hash.finalize().into())
    }

    fn known_control_failure(error: &Error) -> Option<ToolOutput> {
        match error {
            Error::Denied(message) | Error::BeforeDispatch(message) => {
                Some(ToolOutput::error(format!("denied {message}")))
            }
            Error::Conflict(message) => Some(ToolOutput::error(format!("conflict {message}"))),
            Error::Limit(message) => Some(ToolOutput::error(format!("limit {message}"))),
            _ => None,
        }
    }

    fn finish_control_output_full(
        runtime: &JobRuntime,
        key: [u8; 32],
        canonical_ref: Option<&str>,
        recipient_ref: Option<&str>,
        recipient_authority_ref: Option<&str>,
        output: ToolOutput,
    ) -> ToolOutput {
        let receipt = ControlReplayReceipt {
            canonical_ref: canonical_ref.map(str::to_owned),
            recipient_ref: recipient_ref.map(str::to_owned),
            recipient_authority_ref: recipient_authority_ref.map(str::to_owned),
            output: output.clone(),
        };
        if runtime.finish_control_replay(key, receipt).is_err() {
            runtime.mark_control_replay_unknown(key);
            ToolOutput::error("unknown control receipt; inspect before retry")
        } else {
            output
        }
    }

    fn finish_control_output(
        runtime: &JobRuntime,
        key: [u8; 32],
        canonical_ref: Option<&str>,
        output: ToolOutput,
    ) -> ToolOutput {
        Self::finish_control_output_full(runtime, key, canonical_ref, None, None, output)
    }

    fn finish_subscribe_output(
        runtime: &JobRuntime,
        key: [u8; 32],
        canonical_ref: &str,
        recipient: &AgentWakeRecipient,
        output: ToolOutput,
    ) -> ToolOutput {
        Self::finish_control_output_full(
            runtime,
            key,
            Some(canonical_ref),
            Some(&recipient.canonical_ref),
            Some(&recipient.authority_ref),
            output,
        )
    }

    fn finish_control_error(
        runtime: &JobRuntime,
        key: [u8; 32],
        canonical_ref: Option<&str>,
        error: Error,
    ) -> Result<ToolOutput> {
        if let Some(output) = Self::known_control_failure(&error) {
            Ok(Self::finish_control_output(
                runtime,
                key,
                canonical_ref,
                output,
            ))
        } else {
            runtime.mark_control_replay_unknown(key);
            Err(error)
        }
    }

    fn render_output_page(
        page: &OutputPage,
        offset: u64,
        encoding: &str,
        budget: usize,
    ) -> Result<String> {
        let render = |bytes: &[u8]| -> Result<String> {
            let next = offset
                .checked_add(bytes.len() as u64)
                .ok_or(Error::Limit("Job output continuation offset overflow"))?;
            let has_more = next < page.retained_bytes;
            let mut content = if encoding == "base64" {
                base64::engine::general_purpose::STANDARD.encode(bytes)
            } else {
                String::from_utf8_lossy(bytes).into_owned()
            };
            let mut metadata = Vec::new();
            if has_more {
                metadata.push(format!("more offset={next}"));
            }
            if page.truncated {
                metadata.push(format!(
                    "truncated retained={} total={}",
                    page.retained_bytes, page.total_bytes
                ));
            }
            if !metadata.is_empty() {
                if !content.is_empty() && !content.ends_with('\n') {
                    content.push('\n');
                }
                content.push_str(&metadata.join("\n"));
            }
            Ok(content)
        };

        if page.bytes.is_empty() {
            let rendered = render(&[])?;
            return (rendered.len() <= budget)
                .then_some(rendered)
                .ok_or(Error::Limit(
                    "Job output metadata exceeds configured result ceiling",
                ));
        }
        let mut low = 0usize;
        let mut high = page.bytes.len();
        let mut best = None;
        while low <= high {
            let mid = low + (high - low) / 2;
            let rendered = render(&page.bytes[..mid])?;
            if rendered.len() <= budget {
                best = Some((mid, rendered));
                low = mid.saturating_add(1);
            } else if mid == 0 {
                break;
            } else {
                high = mid - 1;
            }
        }
        let (kept, rendered) = best.ok_or(Error::Limit(
            "Job output metadata exceeds configured result ceiling",
        ))?;
        if kept == 0 {
            return Err(Error::Limit(
                "one Job output byte cannot fit configured result ceiling",
            ));
        }
        Ok(rendered)
    }

    fn stream_for(
        manager: &aw_tools::filesystem::job_manager::JobManager,
        id: &str,
        kind: FiniteJobKind,
        node: Option<usize>,
        view: Option<&str>,
    ) -> Result<(usize, OutputStream)> {
        let coordinate = match node {
            Some(node) => node,
            None => 1,
        };
        let plans = manager
            .plans(id)
            .map_err(|_| Error::Denied("Job plan is unavailable"))?;
        let plan = plans
            .get(coordinate.saturating_sub(1))
            .ok_or(Error::Denied("Job node is out of range"))?;
        let shell = plan.tool().is_shell();

        let stream = match view {
            Some("result") => OutputStream::Result,
            Some("stdout") if shell => OutputStream::Stdout,
            Some("stderr") if shell => OutputStream::Stderr,
            Some("stdout" | "stderr") => {
                return Err(Error::Denied(
                    "native Job nodes expose only the result view",
                ))
            }
            Some(_) => return Err(Error::Denied("invalid Job view")),
            None if node.is_some() => OutputStream::Result,
            None if kind == FiniteJobKind::Process => OutputStream::Stdout,
            None => OutputStream::Result,
        };
        Ok((coordinate, stream))
    }

    fn graph_result_page(
        manager: &aw_tools::filesystem::job_manager::JobManager,
        id: &str,
        offset: u64,
        limit: usize,
        encoding: &str,
        budget: usize,
    ) -> Result<String> {
        let (_, spool_limit) = manager
            .spool_usage()
            .map_err(|_| Error::Protocol("Job spool quota unavailable"))?;
        let render_limit = usize::try_from(spool_limit)
            .unwrap_or(usize::MAX)
            .min(16 * 1024 * 1024);
        let rendered = manager
            .render_graph_result(id, render_limit)
            .map_err(|error| match error {
                aw_tools::filesystem::job_manager::Error::Busy => {
                    Error::Conflict("Graph result is not terminal")
                }
                _ => Error::Protocol("Graph result projection failed"),
            })?
            .ok_or(Error::Limit(
                "Graph result is too large for aggregate view; read node refs",
            ))?;
        let bytes = rendered.as_bytes();
        let start = usize::try_from(offset)
            .ok()
            .filter(|offset| *offset <= bytes.len())
            .ok_or(Error::Denied("jobs_read offset is past Graph result"))?;
        let end = start.saturating_add(limit).min(bytes.len());
        let page = OutputPage {
            bytes: bytes[start..end].to_vec(),
            next_offset: (end < bytes.len()).then_some(end as u64),
            writer_finished: true,
            truncated: false,
            total_bytes: bytes.len() as u64,
            retained_bytes: bytes.len() as u64,
        };
        Self::render_output_page(&page, offset, encoding, budget)
    }

    async fn conditional_snapshot(
        host: &crate::job_host::JobHost,
        id: &str,
        observed: Option<u64>,
        wait_ms: u64,
    ) -> Result<Snapshot> {
        let mut snapshot = host
            .manager()
            .get(id)
            .map_err(|_| Error::Denied("Job is not exposed or missing"))?;
        let baseline = observed.unwrap_or(snapshot.sequence);
        if snapshot.sequence != baseline || wait_ms == 0 {
            return Ok(snapshot);
        }
        let started = std::time::Instant::now();
        let wait = Duration::from_millis(wait_ms);
        while started.elapsed() < wait {
            tokio::time::sleep(
                Duration::from_millis(10).min(wait.saturating_sub(started.elapsed())),
            )
            .await;
            snapshot = host
                .manager()
                .get(id)
                .map_err(|_| Error::Denied("Job is not exposed or missing"))?;
            if snapshot.sequence != baseline {
                break;
            }
        }
        Ok(snapshot)
    }

    fn spec_for(&self) -> ToolSpec {
        let ref_schema = json!({"type":"string","minLength":1,"maxLength":2048});
        let rev_schema = json!({"type":["integer","null"],"minimum":1});
        match self.kind {
            JobsKind::List => ToolSpec {
                name: "jobs_list".into(),
                description: "List recent exposed Jobs newest first with stable opaque cursor paging. Status only; never reads or acknowledges retained output.".into(),
                parameters: json!({"type":"object","properties":{
                    "cursor":{"type":["string","null"],"minLength":1,"maxLength":512},
                    "limit":{"type":["integer","null"],"minimum":1,"maximum":self.runtime.job_list_limit().min(MAX_JOB_LIST_PAGE)}
                },"additionalProperties":false}),
                effect: ToolEffect::Read,
            },
            JobsKind::Get => ToolSpec {
                name: "jobs_get".into(),
                description: "Observe one Job/ref and revision. rev is a conditional observation token; wait_ms may long-poll for a different revision without acknowledging output.".into(),
                parameters: json!({"type":"object","properties":{
                    "ref":ref_schema,
                    "rev":rev_schema,
                    "wait_ms":{"type":["integer","null"],"minimum":0,"maximum":MAX_WAIT_MS}
                },"additionalProperties":false}),
                effect: ToolEffect::Read,
            },
            JobsKind::Read => ToolSpec {
                name: "jobs_read".into(),
                description: "Read the natural result or one kind-supported view. Agent refs additionally support messages/tool_calls/tool_outputs/trace and bounded history filters; Process/Graph retained streams use raw offsets. Reading never ACKs.".into(),
                parameters: json!({"type":"object","properties":{
                    "ref":ref_schema,
                    "view":{"type":["string","null"],"enum":["result","stdout","stderr","messages","tool_calls","tool_outputs","trace",null]},
                    "filter":{"oneOf":[
                        {"type":"string","enum":["default","none"]},
                        {"type":"object","properties":{
                            "result":{"type":"boolean"},"messages":{"type":"boolean"},
                            "tool_calls":{"type":"boolean"},"tool_outputs":{"type":"boolean"},
                            "tools":{"type":"array","maxItems":64,"items":{"type":"string","minLength":1,"maxLength":128}},
                            "exclude":{"type":"boolean"}
                        },"additionalProperties":false},
                        {"type":"null"}
                    ]},
                    "cursor":{"type":["string","null"],"minLength":1,"maxLength":512},
                    "offset":{"type":["integer","null"],"minimum":0},
                    "limit":{"type":["integer","null"],"minimum":1,"maximum":self.runtime.result_limit().min(aw_tools::task_ops::MAX_TASK_OUTPUT_PAGE)},
                    "encoding":{"type":["string","null"],"enum":["text","base64",null]},
                    "wait_ms":{"type":["integer","null"],"minimum":0,"maximum":MAX_WAIT_MS}
                },"additionalProperties":false}),
                effect: ToolEffect::Read,
            },
            JobsKind::Ack => ToolSpec {
                name: "jobs_ack".into(),
                description: "Explicitly ACK an exact ref/view through a raw byte offset after safe receipt. No revision token; ACK is monotonic and never implied by read.".into(),
                parameters: json!({"type":"object","properties":{
                    "ref":ref_schema,
                    "view":{"type":["string","null"],"enum":["result","stdout","stderr",null]},
                    "offset":{"type":"integer","minimum":0}
                },"required":["ref","offset"],"additionalProperties":false}),
                effect: ToolEffect::Control,
            },
            JobsKind::Send => ToolSpec {
                name: "jobs_send".into(),
                description: "Send typed input to the resolved Job. mode is auto by default; Process Jobs initially support only auto, which writes live stdin or EOF after independent current process.input authorization.".into(),
                parameters: json!({"type":"object","properties":{
                    "ref":ref_schema,
                    "rev":rev_schema,
                    "mode":{"type":["string","null"],"enum":["auto","queue","steer","interrupt",null]},
                    "input":{"oneOf":[
                        {"type":"object","properties":{"text":{"type":"string","maxLength":65536}},"required":["text"],"additionalProperties":false},
                        {"type":"object","properties":{"eof":{"const":true}},"required":["eof"],"additionalProperties":false}
                    ]}
                },"required":["input"],"additionalProperties":false}),
                effect: ToolEffect::Control,
            },
            JobsKind::Cancel => ToolSpec {
                name: "jobs_cancel".into(),
                description: "Idempotently request cancellation of one Job. rev is an optional compare-and-act precondition; no restart capability exists.".into(),
                parameters: json!({"type":"object","properties":{
                    "ref":ref_schema,
                    "rev":rev_schema
                },"required":["ref"],"additionalProperties":false}),
                effect: ToolEffect::Control,
            },
            JobsKind::Subscribe => ToolSpec {
                name: "jobs_subscribe".into(),
                description: "Replace this Agent's future wake rule on one explicit Job source. Empty events clears the rule; no historical event fires retroactively.".into(),
                parameters: json!({"type":"object","properties":{
                    "ref":ref_schema,
                    "rev":rev_schema,
                    "events":{
                        "type":"array",
                        "maxItems":aw_tools::job_event::MAX_EVENT_RULES,
                        "items":{
                            "type":"array",
                            "minItems":1,
                            "maxItems":2,
                            "prefixItems":[
                                {"type":"string","enum":["result","settled","attention"]},
                                {"type":"string","enum":["auto","queue","steer","interrupt"]}
                            ]
                        }
                    }
                },"required":["ref","events"],"additionalProperties":false}),
                effect: ToolEffect::Control,
            },
        }
    }
}

impl Tool for JobsTool {
    fn spec(&self) -> ToolSpec {
        self.spec_for()
    }

    fn validate(&self, arguments: &Value) -> Result<()> {
        match self.kind {
            JobsKind::List => {
                Self::object(arguments, &["cursor", "limit"])?;
                if arguments
                    .get("cursor")
                    .is_some_and(|value| !value.is_null() && value.as_str().is_none())
                {
                    return Err(Error::Denied("cursor must be a string"));
                }
                if let Some(limit) = arguments.get("limit").filter(|value| !value.is_null()) {
                    let limit = limit
                        .as_u64()
                        .and_then(|value| usize::try_from(value).ok())
                        .ok_or(Error::Denied("invalid jobs_list limit"))?;
                    if !(1..=self.runtime.job_list_limit()).contains(&limit) {
                        return Err(Error::Denied("jobs_list limit exceeds host ceiling"));
                    }
                }
            }
            JobsKind::Get => {
                Self::object(arguments, &["ref", "rev", "wait_ms"])?;
                let _ = Self::reference(arguments, true)?;
                let _ = Self::revision(arguments)?;
                if let Some(wait) = arguments.get("wait_ms").filter(|value| !value.is_null()) {
                    if wait.as_u64().is_none_or(|value| value > MAX_WAIT_MS) {
                        return Err(Error::Denied("wait_ms exceeds 30 seconds"));
                    }
                }
            }
            JobsKind::Read => {
                Self::object(
                    arguments,
                    &[
                        "ref", "view", "filter", "cursor", "offset", "limit", "encoding", "wait_ms",
                    ],
                )?;
                let _ = Self::reference(arguments, true)?;
                let view = Self::view(arguments)?;
                let filter = Self::filter(arguments)?;
                if view.is_some() && filter.is_some() {
                    return Err(Error::Denied(
                        "jobs_read view and filter are mutually exclusive",
                    ));
                }
                let _ = Self::encoding(arguments)?;
                if arguments.get("cursor").is_some_and(|value| {
                    !value.is_null()
                        && value
                            .as_str()
                            .is_none_or(|cursor| cursor.is_empty() || cursor.len() > 512)
                }) {
                    return Err(Error::Denied("invalid jobs_read cursor"));
                }
                if arguments
                    .get("wait_ms")
                    .filter(|value| !value.is_null())
                    .is_some_and(|wait| wait.as_u64().is_none_or(|value| value > MAX_WAIT_MS))
                {
                    return Err(Error::Denied("wait_ms exceeds 30 seconds"));
                }
                if arguments
                    .get("offset")
                    .filter(|value| !value.is_null())
                    .is_some_and(|value| value.as_u64().is_none())
                {
                    return Err(Error::Denied("offset must be a non-negative integer"));
                }
            }
            JobsKind::Ack => {
                Self::object(arguments, &["ref", "view", "offset"])?;
                let _ = Self::reference(arguments, false)?;
                let _ = Self::view(arguments)?;
                if arguments.get("offset").and_then(Value::as_u64).is_none() {
                    return Err(Error::Denied("jobs_ack requires raw offset"));
                }
            }
            JobsKind::Send => {
                Self::object(arguments, &["ref", "rev", "mode", "input"])?;
                let _ = Self::reference(arguments, true)?;
                let _ = Self::revision(arguments)?;
                let _ = Self::send_mode(arguments)?;
                let _ = Self::send_input(arguments)?;
                match arguments.get("mode") {
                    None | Some(Value::Null) => {}
                    Some(Value::String(mode))
                        if matches!(mode.as_str(), "auto" | "queue" | "steer" | "interrupt") => {}
                    _ => {
                        return Err(Error::Denied(
                            "jobs_send mode must be auto, queue, steer or interrupt",
                        ))
                    }
                }
                let input = arguments
                    .get("input")
                    .and_then(Value::as_object)
                    .ok_or(Error::Denied("jobs_send requires typed input"))?;
                let text = input.get("text");
                let eof = input.get("eof");
                if input.len() != 1
                    || (text.is_some() == eof.is_some())
                    || text.is_some_and(|value| {
                        value.as_str().is_none_or(|text| text.len() > 64 * 1024)
                    })
                    || eof.is_some_and(|value| value.as_bool() != Some(true))
                {
                    return Err(Error::Denied(
                        "jobs_send input must be exactly {text:string} or {eof:true}",
                    ));
                }
            }
            JobsKind::Cancel => {
                Self::object(arguments, &["ref", "rev"])?;
                let _ = Self::reference(arguments, false)?;
                let _ = Self::revision(arguments)?;
            }
            JobsKind::Subscribe => {
                Self::object(arguments, &["ref", "rev", "events"])?;
                let _ = Self::reference(arguments, false)?;
                let _ = Self::revision(arguments)?;
                let _ = Self::event_rules(arguments)?;
            }
        }
        Ok(())
    }

    fn execute<'a>(&'a self, ctx: ToolContext, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(arguments)?;
            let budget = ctx.result_budget(self.runtime.result_limit())?;
            let host = self.runtime.host_for(&ctx)?;
            let output = match self.kind {
                JobsKind::List => {
                    let limit = arguments
                        .get("limit")
                        .and_then(Value::as_u64)
                        .and_then(|value| usize::try_from(value).ok())
                        .unwrap_or_else(|| self.runtime.job_list_limit().min(20));
                    let cursor = arguments.get("cursor").and_then(Value::as_str);
                    if let Some(router) = self.runtime.agent_jobs_router()? {
                        let authority_fingerprint =
                            host.jobs_list_authority_fingerprint(&ctx.actor)?;
                        if let Some(page) = router
                            .list(
                                &ctx,
                                AgentJobListRequest {
                                    cursor: cursor.map(str::to_owned),
                                    limit,
                                    authority_fingerprint,
                                },
                            )
                            .await?
                        {
                            let mut rows = Vec::new();
                            // Advance only across roots actually consumed from
                            // this page. Reusing the input cursor on a tight
                            // first live row would create a non-advancing loop.
                            let mut advanced_cursor: Option<String> = None;
                            let mut stopped_early = false;
                            for (index, item) in page.items.iter().enumerate() {
                                let row =
                                    if let Some(output) = router
                                        .get(
                                            &ctx,
                                            AgentJobGet {
                                                reference: item.reference.clone(),
                                                revision: None,
                                                wait_ms: 0,
                                            },
                                        )
                                        .await?
                                    {
                                        Some(output.content)
                                    } else {
                                        match host.resolve_job_ref(&item.reference) {
                                            Ok((id, None)) => match host.manager().get(&id) {
                                                Ok(snapshot) => Some(Self::render_snapshot(
                                                    &item.reference,
                                                    &snapshot,
                                                )),
                                                Err(_) => None,
                                            },
                                            Ok((_, Some(_))) => return Err(Error::Protocol(
                                                "jobs_list durable index contains descendant ref",
                                            )),
                                            Err(Error::Denied(_)) => None,
                                            Err(error) => return Err(error),
                                        }
                                    };
                                let more_after =
                                    index + 1 < page.items.len() || page.next_cursor.is_some();
                                if let Some(row) = row {
                                    let mut candidate = rows.clone();
                                    candidate.push(row.clone());
                                    if more_after {
                                        candidate
                                            .push(format!("more cursor={}", item.after_cursor));
                                    }
                                    if candidate.join("\n").len() > budget {
                                        stopped_early = true;
                                        break;
                                    }
                                    rows.push(row);
                                }
                                advanced_cursor = Some(item.after_cursor.clone());
                            }

                            let has_more = stopped_early || page.next_cursor.is_some();
                            if has_more {
                                let continuation = advanced_cursor.ok_or(Error::Limit(
                                    "one jobs_list row cannot fit configured result ceiling",
                                ))?;
                                let marker = format!("more cursor={continuation}");
                                let mut candidate = rows.clone();
                                candidate.push(marker.clone());
                                if candidate.join("\n").len() > budget {
                                    return Err(Error::Limit(
                                        "jobs_list continuation cannot fit configured result ceiling",
                                    ));
                                }
                                rows.push(marker);
                            }
                            return Ok(ToolOutput::text(rows.join("\n")));
                        }
                    }

                    let (jobs, next) = host.list_jobs(&ctx.actor, cursor, limit)?;
                    let mut rows = Vec::new();
                    for snapshot in jobs {
                        let reference = host.execution_ref(&snapshot.id, None)?.canonical();
                        let row = Self::render_snapshot(&reference, &snapshot);
                        let mut candidate = rows.clone();
                        candidate.push(row.clone());
                        if let Some(cursor) = &next {
                            candidate.push(format!("more cursor={cursor}"));
                        }
                        if candidate.join("\n").len() > budget {
                            break;
                        }
                        rows.push(row);
                    }
                    if rows.is_empty() && next.is_some() {
                        return Err(Error::Limit(
                            "one jobs_list row cannot fit configured result ceiling",
                        ));
                    }
                    if let Some(cursor) = next {
                        rows.push(format!("more cursor={cursor}"));
                    }
                    ToolOutput::text(rows.join("\n"))
                }
                JobsKind::Get => {
                    let reference = Self::reference(arguments, true)?.to_owned();
                    let revision = Self::revision(arguments)?;
                    let wait_ms = arguments
                        .get("wait_ms")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    if let Some(router) = self.runtime.agent_jobs_router()? {
                        if let Some(output) = router
                            .get(
                                &ctx,
                                AgentJobGet {
                                    reference: reference.clone(),
                                    revision,
                                    wait_ms,
                                },
                            )
                            .await?
                        {
                            return Ok(output);
                        }
                    }
                    let (id, node) = host.resolve_job_ref(&reference)?;
                    let snapshot =
                        Self::conditional_snapshot(&host, &id, revision, wait_ms).await?;
                    let reference = host.execution_ref(&id, node)?.canonical();
                    let content = if let Some(node) = node {
                        let plans = host
                            .manager()
                            .plans(&id)
                            .map_err(|_| Error::Denied("Job plan is unavailable"))?;
                        let kind = plans
                            .get(node.saturating_sub(1))
                            .ok_or(Error::Denied("Job node is unavailable"))?
                            .tool()
                            .name();
                        match host
                            .manager()
                            .node_state(&id, node)
                            .map_err(|_| Error::Denied("Job node is unavailable"))?
                        {
                            NodeState::Pending => {
                                format!("{reference} {kind} waiting rev={}", snapshot.sequence)
                            }
                            NodeState::Claimed { .. } | NodeState::Running { .. } => {
                                format!("{reference} {kind} running rev={}", snapshot.sequence)
                            }
                            NodeState::Terminal { status, .. } => {
                                let mut line = format!(
                                    "{reference} {kind} settled rev={} outcome={}",
                                    snapshot.sequence,
                                    status.primary.as_str()
                                );
                                if status.more {
                                    line.push_str(" more");
                                }
                                if status.incomplete {
                                    line.push_str(" incomplete");
                                }
                                line
                            }
                        }
                    } else {
                        Self::render_snapshot(&reference, &snapshot)
                    };
                    ToolOutput::text(content)
                }
                JobsKind::Read => {
                    let reference = Self::reference(arguments, true)?.to_owned();
                    let view = Self::view(arguments)?.map(str::to_owned);
                    let filter = Self::filter(arguments)?;
                    let cursor = arguments
                        .get("cursor")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    let limit = arguments
                        .get("limit")
                        .and_then(Value::as_u64)
                        .and_then(|value| usize::try_from(value).ok())
                        .unwrap_or_else(|| {
                            self.runtime
                                .result_limit()
                                .min(aw_tools::task_ops::MAX_TASK_OUTPUT_PAGE)
                        });
                    let wait_ms = arguments
                        .get("wait_ms")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    if let Some(router) = self.runtime.agent_jobs_router()? {
                        if let Some(output) = router
                            .read(
                                &ctx,
                                AgentJobRead {
                                    reference: reference.clone(),
                                    view: view.clone(),
                                    filter: filter.clone(),
                                    cursor: cursor.clone(),
                                    limit,
                                    wait_ms,
                                },
                            )
                            .await?
                        {
                            return Ok(output);
                        }
                    }
                    if filter.is_some()
                        || cursor.is_some()
                        || wait_ms != 0
                        || view.as_deref().is_some_and(|view| {
                            matches!(view, "messages" | "tool_calls" | "tool_outputs" | "trace")
                        })
                    {
                        return Err(Error::Denied(
                            "Agent history fields are invalid for finite Job output",
                        ));
                    }
                    let (id, node) = host.resolve_job_ref(&reference)?;
                    if !host.payload_authorized(&ctx.actor, &id) {
                        return Err(Error::Denied("Job payload authority is not live"));
                    }
                    let kind = host
                        .manager()
                        .finite_kind(&id)
                        .map_err(|_| Error::Denied("Job kind is unavailable"))?;
                    let offset = arguments.get("offset").and_then(Value::as_u64).unwrap_or(0);
                    let encoding = Self::encoding(arguments)?;
                    let view = view.as_deref();
                    let content = if kind == FiniteJobKind::Graph && node.is_none() {
                        if view.is_some_and(|view| view != "result") {
                            return Err(Error::Denied(
                                "Graph root exposes only the result view; use n:<coordinate> for streams",
                            ));
                        }
                        Self::graph_result_page(
                            host.manager(),
                            &id,
                            offset,
                            limit,
                            encoding,
                            budget,
                        )?
                    } else {
                        let (coordinate, stream) =
                            Self::stream_for(host.manager(), &id, kind, node, view)?;
                        let owner = host
                            .manager()
                            .job_output_owner(&id)
                            .map_err(|_| Error::Denied("Job output owner is unavailable"))?;
                        let page = host
                            .manager()
                            .output_page_for_owner(&id, coordinate, stream, owner, offset, limit)
                            .map_err(|_| Error::Denied("Job output view is unavailable"))?;
                        Self::render_output_page(&page, offset, encoding, budget)?
                    };
                    ToolOutput::text(content)
                }
                JobsKind::Ack => {
                    let (id, node) = host.resolve_job_ref(Self::reference(arguments, false)?)?;
                    if !host.payload_authorized(&ctx.actor, &id) {
                        return Err(Error::Denied("Job payload authority is not live"));
                    }
                    let kind = host
                        .manager()
                        .finite_kind(&id)
                        .map_err(|_| Error::Denied("Job kind is unavailable"))?;
                    let view = Self::view(arguments)?;
                    let through = arguments
                        .get("offset")
                        .and_then(Value::as_u64)
                        .ok_or(Error::Denied("jobs_ack requires raw offset"))?;
                    let owner = host
                        .manager()
                        .job_output_owner(&id)
                        .map_err(|_| Error::Denied("Job output owner is unavailable"))?;
                    if kind == FiniteJobKind::Graph && node.is_none() {
                        return Err(Error::Denied(
                            "Graph root result is a synthesized aggregate; ACK an exact n:<coordinate> retained result instead",
                        ));
                    }
                    let (coordinate, stream) =
                        Self::stream_for(host.manager(), &id, kind, node, view)?;
                    let ack = host
                        .manager()
                        .acknowledge_delivered_output(&id, coordinate, stream, owner, through)
                        .map_err(|_| {
                            Error::Denied("Job ACK exceeds bytes delivered to this output owner")
                        })?;
                    let (changed, released) = (ack.changed, ack.released);
                    ToolOutput::text(format!(
                        "{} released={released}",
                        if changed { "ok" } else { "unchanged" }
                    ))
                }
                JobsKind::Send => {
                    let requested_ref = Self::reference(arguments, true)?.to_owned();
                    let revision = Self::revision(arguments)?;
                    let mode = Self::send_mode(arguments)?;
                    let input = Self::send_input(arguments)?;
                    let fingerprint =
                        Self::control_fingerprint("jobs.send", &requested_ref, arguments)?;
                    let claim =
                        self.runtime
                            .claim_control_replay(&ctx, "jobs.send", fingerprint)?;
                    match claim {
                        ControlReplayDecision::Existing(receipt) => {
                            if let Some(canonical) = receipt.canonical_ref.as_deref() {
                                let authorized =
                                    if let Some(router) = self.runtime.agent_jobs_router()? {
                                        router.resolve_control_ref(&ctx, canonical)?.is_some()
                                    } else {
                                        false
                                    };
                                if !authorized {
                                    let _ = host.resolve_job_ref(canonical)?;
                                }
                            }
                            receipt.output
                        }
                        ControlReplayDecision::Unknown(_) => ToolOutput::error(
                            "unknown jobs_send outcome; do not retry automatically",
                        ),
                        ControlReplayDecision::New(key) => {
                            let router = self.runtime.agent_jobs_router()?;
                            let agent_target = match router.as_ref() {
                                Some(router) => {
                                    match router.resolve_control_ref(&ctx, &requested_ref) {
                                        Ok(target) => target,
                                        Err(error) => {
                                            return Self::finish_control_error(
                                                &self.runtime,
                                                key,
                                                None,
                                                error,
                                            )
                                        }
                                    }
                                }
                                None => None,
                            };
                            if let Some(canonical) = agent_target {
                                self.runtime.bind_control_replay_target(key, &canonical)?;
                                let router = router.expect("Agent target requires router");
                                match router
                                    .send(
                                        &ctx,
                                        AgentJobSend {
                                            reference: canonical.clone(),
                                            revision,
                                            mode,
                                            input: input.clone(),
                                        },
                                    )
                                    .await
                                {
                                    Ok(Some(output)) => Self::finish_control_output(
                                        &self.runtime,
                                        key,
                                        Some(&canonical),
                                        output,
                                    ),
                                    Ok(None) => {
                                        self.runtime.mark_control_replay_unknown(key);
                                        return Err(Error::Protocol(
                                            "resolved Agent control target lost its owner",
                                        ));
                                    }
                                    Err(error) => Self::finish_control_error(
                                        &self.runtime,
                                        key,
                                        Some(&canonical),
                                        error,
                                    )?,
                                }
                            } else {
                                let (id, node) = match host.resolve_job_ref(&requested_ref) {
                                    Ok(value) => value,
                                    Err(error) => {
                                        return Self::finish_control_error(
                                            &self.runtime,
                                            key,
                                            None,
                                            error,
                                        )
                                    }
                                };
                                let canonical = host.execution_ref(&id, node)?.canonical();
                                self.runtime.bind_control_replay_target(key, &canonical)?;
                                let kind = match host.manager().finite_kind(&id) {
                                    Ok(kind) => kind,
                                    Err(_) => {
                                        return Self::finish_control_error(
                                            &self.runtime,
                                            key,
                                            Some(&canonical),
                                            Error::Denied("Job kind is unavailable"),
                                        )
                                    }
                                };
                                let coordinate = match (kind, node) {
                                    (FiniteJobKind::Process, None) => 1,
                                    (_, Some(node)) => node,
                                    (FiniteJobKind::Graph, None) => {
                                        return Self::finish_control_error(
                                            &self.runtime,
                                            key,
                                            Some(&canonical),
                                            Error::Denied(
                                                "Graph jobs_send requires an interactive node ref",
                                            ),
                                        )
                                    }
                                };
                                if mode != AgentSendMode::Auto {
                                    return Self::finish_control_error(
                                        &self.runtime,
                                        key,
                                        Some(&canonical),
                                        Error::Denied("Process jobs_send supports only mode=auto"),
                                    );
                                }
                                let (bytes, eof) = match &input {
                                    AgentSendInput::Text(text) => {
                                        (Some(text.as_bytes().to_vec()), false)
                                    }
                                    AgentSendInput::Eof => (None, true),
                                };
                                match host
                                    .send_process_input(
                                        &ctx.actor, &id, coordinate, revision, bytes, eof,
                                    )
                                    .await
                                {
                                    Ok(()) => {
                                        let snapshot = host.manager().get(&id).map_err(|_| {
                                            Error::Protocol("Job disappeared after jobs_send")
                                        })?;
                                        let output = ToolOutput::text(format!(
                                            "{canonical} rev={} ok",
                                            snapshot.sequence
                                        ));
                                        Self::finish_control_output(
                                            &self.runtime,
                                            key,
                                            Some(&canonical),
                                            output,
                                        )
                                    }
                                    Err(error) => Self::finish_control_error(
                                        &self.runtime,
                                        key,
                                        Some(&canonical),
                                        error,
                                    )?,
                                }
                            }
                        }
                    }
                }
                JobsKind::Cancel => {
                    let requested_ref = Self::reference(arguments, false)?.to_owned();
                    let revision = Self::revision(arguments)?;
                    let fingerprint =
                        Self::control_fingerprint("jobs.cancel", &requested_ref, arguments)?;
                    let claim =
                        self.runtime
                            .claim_control_replay(&ctx, "jobs.cancel", fingerprint)?;
                    match claim {
                        ControlReplayDecision::Existing(receipt) => {
                            if let Some(canonical) = receipt.canonical_ref.as_deref() {
                                let authorized =
                                    if let Some(router) = self.runtime.agent_jobs_router()? {
                                        router.resolve_control_ref(&ctx, canonical)?.is_some()
                                    } else {
                                        false
                                    };
                                if !authorized {
                                    let _ = host.resolve_job_ref(canonical)?;
                                }
                            }
                            receipt.output
                        }
                        ControlReplayDecision::Unknown(_) => ToolOutput::error(
                            "unknown jobs_cancel outcome; do not retry automatically",
                        ),
                        ControlReplayDecision::New(key) => {
                            let router = self.runtime.agent_jobs_router()?;
                            let agent_target = match router.as_ref() {
                                Some(router) => {
                                    match router.resolve_control_ref(&ctx, &requested_ref) {
                                        Ok(target) => target,
                                        Err(error) => {
                                            return Self::finish_control_error(
                                                &self.runtime,
                                                key,
                                                None,
                                                error,
                                            )
                                        }
                                    }
                                }
                                None => None,
                            };
                            if let Some(canonical) = agent_target {
                                self.runtime.bind_control_replay_target(key, &canonical)?;
                                let router = router.expect("Agent target requires router");
                                match router
                                    .cancel(
                                        &ctx,
                                        AgentJobCancel {
                                            reference: canonical.clone(),
                                            revision,
                                        },
                                    )
                                    .await
                                {
                                    Ok(Some(output)) => Self::finish_control_output(
                                        &self.runtime,
                                        key,
                                        Some(&canonical),
                                        output,
                                    ),
                                    Ok(None) => {
                                        self.runtime.mark_control_replay_unknown(key);
                                        return Err(Error::Protocol(
                                            "resolved Agent cancel target lost its owner",
                                        ));
                                    }
                                    Err(error) => Self::finish_control_error(
                                        &self.runtime,
                                        key,
                                        Some(&canonical),
                                        error,
                                    )?,
                                }
                            } else {
                                let (id, node) = match host.resolve_job_ref(&requested_ref) {
                                    Ok(value) => value,
                                    Err(error) => {
                                        return Self::finish_control_error(
                                            &self.runtime,
                                            key,
                                            None,
                                            error,
                                        )
                                    }
                                };
                                let canonical = host.execution_ref(&id, node)?.canonical();
                                self.runtime.bind_control_replay_target(key, &canonical)?;
                                if node.is_some() {
                                    return Self::finish_control_error(
                                        &self.runtime,
                                        key,
                                        Some(&canonical),
                                        Error::Denied(
                                            "jobs_cancel targets a Job, not a Graph node",
                                        ),
                                    );
                                }
                                match self.runtime.cancel_checked(&ctx, &id, revision).await {
                                    Ok((changed, snapshot)) => {
                                        let output = ToolOutput::text(format!(
                                            "{canonical} rev={} {}",
                                            snapshot.sequence,
                                            if changed { "cancelling" } else { "unchanged" }
                                        ));
                                        Self::finish_control_output(
                                            &self.runtime,
                                            key,
                                            Some(&canonical),
                                            output,
                                        )
                                    }
                                    Err(error) => Self::finish_control_error(
                                        &self.runtime,
                                        key,
                                        Some(&canonical),
                                        error,
                                    )?,
                                }
                            }
                        }
                    }
                }
                JobsKind::Subscribe => {
                    let requested_ref = Self::reference(arguments, false)?.to_owned();
                    let revision = Self::revision(arguments)?;
                    let rules = Self::event_rules(arguments)?;
                    let mut normalized = json!({
                        "ref": requested_ref,
                        "events": Self::canonical_event_value(&rules),
                    });
                    if let Some(revision) = revision {
                        normalized
                            .as_object_mut()
                            .expect("normalized subscribe object")
                            .insert("rev".into(), Value::from(revision));
                    }
                    let fingerprint =
                        Self::control_fingerprint("jobs.subscribe", &requested_ref, &normalized)?;
                    let claim =
                        self.runtime
                            .claim_control_replay(&ctx, "jobs.subscribe", fingerprint)?;
                    match claim {
                        ControlReplayDecision::Existing(receipt) => {
                            let router = self.runtime.agent_jobs_router()?.ok_or(Error::Denied(
                                "jobs_subscribe recipient router is unavailable",
                            ))?;
                            let recipient = AgentWakeRecipient {
                                canonical_ref: receipt.recipient_ref.clone().ok_or(
                                    Error::Protocol("jobs_subscribe replay lacks recipient ref"),
                                )?,
                                authority_ref: receipt.recipient_authority_ref.clone().ok_or(
                                    Error::Protocol(
                                        "jobs_subscribe replay lacks recipient authority",
                                    ),
                                )?,
                            };
                            router.revalidate_wake_recipient(&ctx, &recipient)?;
                            if let Some(canonical) = receipt.canonical_ref.as_deref() {
                                if router.resolve_control_ref(&ctx, canonical)?.is_none() {
                                    let _ = host.resolve_job_ref(canonical)?;
                                }
                            }
                            receipt.output
                        }
                        ControlReplayDecision::Unknown(_) => ToolOutput::error(
                            "unknown jobs_subscribe outcome; do not retry automatically",
                        ),
                        ControlReplayDecision::New(key) => {
                            let router =
                                match self.runtime.agent_jobs_router()? {
                                    Some(router) => router,
                                    None => return Self::finish_control_error(
                                        &self.runtime,
                                        key,
                                        None,
                                        Error::Denied(
                                            "jobs_subscribe requires an AW Agent recipient owner",
                                        ),
                                    ),
                                };
                            let recipient = match router.wake_recipient(&ctx) {
                                Ok(Some(recipient)) => recipient,
                                Ok(None) => {
                                    let output = ToolOutput::error(
                                        "denied current Agent has no bounded automatic-work wake authority",
                                    );
                                    return Ok(Self::finish_control_output_full(
                                        &self.runtime,
                                        key,
                                        None,
                                        None,
                                        None,
                                        output,
                                    ));
                                }
                                Err(error) => {
                                    return Self::finish_control_error(
                                        &self.runtime,
                                        key,
                                        None,
                                        error,
                                    )
                                }
                            };
                            let agent_target =
                                match router.resolve_control_ref(&ctx, &requested_ref) {
                                    Ok(target) => target,
                                    Err(error) => {
                                        let output =
                                            Self::known_control_failure(&error).ok_or(error)?;
                                        return Ok(Self::finish_control_output_full(
                                            &self.runtime,
                                            key,
                                            None,
                                            Some(&recipient.canonical_ref),
                                            Some(&recipient.authority_ref),
                                            output,
                                        ));
                                    }
                                };
                            if let Some(canonical) = agent_target {
                                self.runtime.bind_control_replay_target(key, &canonical)?;
                                if canonical == recipient.canonical_ref {
                                    let output = ToolOutput::error(
                                        "denied Agent event source cannot subscribe to itself",
                                    );
                                    Self::finish_subscribe_output(
                                        &self.runtime,
                                        key,
                                        &canonical,
                                        &recipient,
                                        output,
                                    )
                                } else {
                                    match router
                                        .subscribe(
                                            &ctx,
                                            AgentJobSubscribe {
                                                reference: canonical.clone(),
                                                revision,
                                                rules: rules.clone(),
                                            },
                                        )
                                        .await
                                    {
                                        Ok(Some(output)) => Self::finish_subscribe_output(
                                            &self.runtime,
                                            key,
                                            &canonical,
                                            &recipient,
                                            output,
                                        ),
                                        Ok(None) => {
                                            self.runtime.mark_control_replay_unknown(key);
                                            return Err(Error::Protocol(
                                                "resolved Agent subscription source lost its owner",
                                            ));
                                        }
                                        Err(error) => {
                                            if let Some(output) =
                                                Self::known_control_failure(&error)
                                            {
                                                Self::finish_subscribe_output(
                                                    &self.runtime,
                                                    key,
                                                    &canonical,
                                                    &recipient,
                                                    output,
                                                )
                                            } else {
                                                self.runtime.mark_control_replay_unknown(key);
                                                return Err(error);
                                            }
                                        }
                                    }
                                }
                            } else {
                                let (id, node) = match host.resolve_job_ref(&requested_ref) {
                                    Ok(value) => value,
                                    Err(error) => {
                                        let output =
                                            Self::known_control_failure(&error).ok_or(error)?;
                                        return Ok(Self::finish_control_output_full(
                                            &self.runtime,
                                            key,
                                            None,
                                            Some(&recipient.canonical_ref),
                                            Some(&recipient.authority_ref),
                                            output,
                                        ));
                                    }
                                };
                                let canonical = host.execution_ref(&id, node)?.canonical();
                                self.runtime.bind_control_replay_target(key, &canonical)?;
                                match host.subscribe_finite(
                                    &ctx.actor,
                                    &id,
                                    node,
                                    revision,
                                    &recipient.canonical_ref,
                                    &recipient.authority_ref,
                                    rules,
                                ) {
                                    Ok((_, update)) => {
                                        let output = ToolOutput::text(format!(
                                            "{canonical} subscription={} generation={}{}",
                                            if update.active { "active" } else { "cleared" },
                                            update.generation,
                                            if update.changed { "" } else { " unchanged" },
                                        ));
                                        Self::finish_subscribe_output(
                                            &self.runtime,
                                            key,
                                            &canonical,
                                            &recipient,
                                            output,
                                        )
                                    }
                                    Err(error) => {
                                        if let Some(output) = Self::known_control_failure(&error) {
                                            Self::finish_subscribe_output(
                                                &self.runtime,
                                                key,
                                                &canonical,
                                                &recipient,
                                                output,
                                            )
                                        } else {
                                            self.runtime.mark_control_replay_unknown(key);
                                            return Err(error);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            };
            output.validate()?;
            Ok(output)
        })
    }
}
