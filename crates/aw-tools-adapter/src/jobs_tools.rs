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
    budget::SerializedResultBytes,
    execution_ref::JobRef,
    filesystem::job_manager::{
        FiniteJobKind, OutputPage, OutputPosition, OutputStream, Snapshot, MAX_JOB_LIST_PAGE,
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

struct TailRequest<'a> {
    id: &'a str,
    node: Option<usize>,
    kind: FiniteJobKind,
    requested: Option<u64>,
    budget: usize,
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

    fn render_snapshot(reference: &JobRef, snapshot: &Snapshot) -> Result<String> {
        aw_tools::receipt::render_plain(
            aw_tools::receipt::Receipt::JobStatus {
                reference,
                snapshot,
                terminal: None,
                next: None,
            },
            aw_tools::filesystem::job_manager::MAX_COMPACT_JOB_STATUS_BYTES,
        )
        .map_err(|_| Error::Protocol("cannot render typed compact Job status"))
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
        budget: SerializedResultBytes,
    ) -> Result<(String, usize)> {
        let budget = budget.get();
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
                .then_some((rendered, 0))
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
        Ok((rendered, kept))
    }

    fn stream_for(
        manager: &aw_tools::filesystem::job_manager::JobManager,
        id: &str,
        kind: FiniteJobKind,
        node: Option<usize>,
        view: Option<&str>,
    ) -> Result<(usize, OutputStream)> {
        let coordinate = node.unwrap_or(1);
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
        budget: SerializedResultBytes,
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
                aw_tools::filesystem::job_manager::Error::ReplayUnknown => Error::Conflict(
                    "Graph result journal outcome unknown; reconcile before reading aggregate output",
                ),
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
            offset: 0,
            bytes: bytes[start..end].to_vec(),
            next_offset: (end < bytes.len()).then_some(end as u64),
            writer_finished: true,
            truncated: false,
            total_bytes: bytes.len() as u64,
            retained_bytes: bytes.len() as u64,
        };
        Ok(Self::render_output_page(&page, offset, encoding, budget)?.0)
    }

    /// Finite Job waits are settle-or-deadline, never revision-triggered.
    /// Subscribe before reading; the watch generation is an invalidation
    /// signal only. The JobManager remains the source of truth.
    async fn conditional_snapshot(
        host: &crate::job_host::JobHost,
        id: &str,
        node: Option<usize>,
        _observed: Option<u64>,
        wait_ms: u64,
        cancellation: &aw_agent_core::CancellationToken,
    ) -> Result<Snapshot> {
        let manager = host.manager();
        if wait_ms == 0 {
            return manager
                .get(id)
                .map_err(|_| Error::Denied("Job is not exposed or missing"));
        }
        let mut changes = manager
            .subscribe_changes(id)
            .map_err(|_| Error::Denied("Job is not exposed or missing"))?;
        let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_ms);
        loop {
            let _ = *changes.borrow_and_update();
            let snapshot = manager
                .get(id)
                .map_err(|_| Error::Denied("Job is not exposed or missing"))?;
            let settled = if let Some(node) = node {
                match manager.node_state(id, node) {
                    Ok(aw_tools::filesystem::job::NodeState::Terminal { .. }) => true,
                    Ok(_) => false,
                    Err(_) => return Err(Error::Denied("Job node is unavailable")),
                }
            } else {
                snapshot.lifecycle == aw_tools::filesystem::job::Lifecycle::Terminal
            };
            // A journal-unknown Job cannot publish trustworthy new lifecycle
            // transitions until its original host reconciles the journal.
            // End observation promptly, without claiming execution terminality.
            if settled || snapshot.journal_unknown || tokio::time::Instant::now() >= deadline {
                return Ok(snapshot);
            }
            tokio::select! {
                changed = changes.changed() => {
                    if changed.is_err() {
                        return Err(Error::Protocol("Job notification source closed"));
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    return manager.get(id).map_err(|_| Error::Denied(
                        "Job is not exposed or missing"
                    ));
                }
                _ = cancellation.cancelled() => return Err(Error::Cancelled),
            }
        }
    }

    /// Byte-aware finite-stream observation. A revision alone never completes
    /// this wait. Subscribe before inspecting retained bytes so publication
    /// between the check and await is observed without polling.
    async fn wait_for_output(
        host: &crate::job_host::JobHost,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        offset: u64,
        wait_ms: u64,
        cancellation: &aw_agent_core::CancellationToken,
    ) -> Result<()> {
        if wait_ms == 0 {
            return Ok(());
        }
        let manager = host.manager();
        let mut changes = manager
            .subscribe_changes(id)
            .map_err(|_| Error::Denied("Job is not exposed or missing"))?;
        let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_ms);
        loop {
            let _ = *changes.borrow_and_update();
            match manager.output_summary(id, coordinate, stream) {
                Ok((summary, finished)) => {
                    if summary.retained_bytes > offset || finished || summary.truncated {
                        return Ok(());
                    }
                }
                Err(aw_tools::filesystem::job_manager::Error::OutputMissing) => {
                    // The Process node may not have attached its spool yet.
                }
                Err(aw_tools::filesystem::job_manager::Error::OutputReleased) => {
                    return Ok(());
                }
                Err(_) => return Err(Error::Denied("Job output view is unavailable")),
            }
            // A Graph node may settle skipped/failed without ever attaching
            // an output stream. Do not keep its reader waiting for siblings.
            match manager.node_state(id, coordinate) {
                Ok(aw_tools::filesystem::job::NodeState::Terminal { .. }) => return Ok(()),
                Ok(_) => {}
                Err(_) => return Err(Error::Denied("Job node is unavailable")),
            }
            let snapshot = manager
                .get(id)
                .map_err(|_| Error::Denied("Job is not exposed or missing"))?;
            // When no requested bytes are ready, do not wait until the caller
            // deadline on a journal-fenced stream; do not fabricate EOF.
            if snapshot.journal_unknown {
                return Err(Error::Conflict(
                    "Job journal outcome unknown; reconcile before waiting for output",
                ));
            }
            if snapshot.lifecycle == aw_tools::filesystem::job::Lifecycle::Terminal
                || tokio::time::Instant::now() >= deadline
            {
                return Ok(());
            }
            tokio::select! {
                changed = changes.changed() => {
                    if changed.is_err() {
                        return Err(Error::Protocol("Job output notification source closed"));
                    }
                }
                _ = tokio::time::sleep_until(deadline) => return Ok(()),
                _ = cancellation.cancelled() => return Err(Error::Cancelled),
            }
        }
    }

    fn read_position(arguments: &Value) -> OutputPosition {
        match arguments.get("offset") {
            Some(Value::Number(number)) if number.as_i64().is_some_and(|n| n < 0) => {
                OutputPosition::Tail(number.as_i64().unwrap().unsigned_abs())
            }
            Some(Value::Number(number)) => OutputPosition::Absolute(number.as_u64().unwrap_or(0)),
            _ => OutputPosition::Absolute(0),
        }
    }

    /// Only real Process streams can carry a status tail. A Graph root
    /// retains a compact node/status tree, not a concatenation of logs.
    fn process_tail_coordinate(
        manager: &aw_tools::filesystem::job_manager::JobManager,
        id: &str,
        node: Option<usize>,
        kind: FiniteJobKind,
    ) -> Result<Option<usize>> {
        if kind == FiniteJobKind::Process && node.is_none() {
            return Ok(Some(1));
        }
        if let Some(node) = node {
            let plans = manager
                .plans(id)
                .map_err(|_| Error::Denied("Job plan unavailable"))?;
            if plans
                .get(node.saturating_sub(1))
                .is_some_and(|plan| plan.tool().is_shell())
            {
                return Ok(Some(node));
            }
        }
        Ok(None)
    }

    fn append_process_tail(
        &self,
        ctx: &ToolContext,
        host: &crate::job_host::JobHost,
        request: TailRequest<'_>,
        content: &mut String,
    ) -> Result<()> {
        let TailRequest {
            id,
            node,
            kind,
            requested,
            budget,
        } = request;
        let tail = requested.unwrap_or(self.runtime.job_profile().get_tail_default as u64) as usize;
        if tail == 0 {
            return Ok(());
        }
        let Some(coordinate) = Self::process_tail_coordinate(host.manager(), id, node, kind)?
        else {
            if requested.is_some() {
                return Err(Error::Denied(
                    "tail is supported only by Process Job streams",
                ));
            }
            return Ok(());
        };
        if !self.runtime.payload_read_authorized(ctx) || !host.payload_authorized(&ctx.actor, id) {
            if requested.is_some() {
                return Err(Error::Denied("Job output tail is not currently authorized"));
            }
            content.push_str("\ntail=denied");
            return Ok(());
        }
        // Lossy UTF-8 can expand up to threefold. Reserve status/markers first;
        // a small budget gives a truthful marker rather than an invalid reply.
        let capacity = budget.saturating_sub(content.len().saturating_add(128)) / 3;
        let take = tail.min(capacity);
        if take == 0 {
            content.push_str("\ntail=budget");
            return Ok(());
        }
        let owner = host
            .manager()
            .job_output_owner(id)
            .map_err(|_| Error::Denied("Job output owner unavailable"))?;
        let output = host
            .manager()
            .process_tail_for_owner(id, coordinate, owner, take);
        // Hold secret bytes in a local projection until the last live Actor
        // Envelope and producer grant check. Never append before rechecking.
        let mut projected = String::new();
        match output {
            Ok((stdout, stderr)) => {
                for (stream, page) in [("stdout", stdout), ("stderr", stderr)] {
                    if let Some(page) = page {
                        let end = page.offset + page.bytes.len() as u64;
                        projected.push_str(&format!(
                            "\n{stream} [{}..{}]{}:",
                            page.offset,
                            end,
                            if page.writer_finished { " eof" } else { "" }
                        ));
                        projected.push('\n');
                        projected.push_str(&String::from_utf8_lossy(&page.bytes));
                    }
                }
            }
            Err(aw_tools::filesystem::job_manager::Error::OutputReleased) => {
                projected.push_str("\ntail=released");
            }
            Err(aw_tools::filesystem::job_manager::Error::OutputMissing) => {
                projected.push_str("\ntail=unavailable");
            }
            Err(_) => return Err(Error::Denied("Job output tail unavailable")),
        }
        if take < tail {
            projected.push_str("\ntail=capped");
        }
        if !self.runtime.payload_read_authorized(ctx) || !host.payload_authorized(&ctx.actor, id) {
            if requested.is_some() {
                return Err(Error::Denied("Job output tail revoked before delivery"));
            }
            content.push_str("\ntail=denied");
            return Ok(());
        }
        if content.len().saturating_add(projected.len()) > budget {
            return Err(Error::Limit("Job status and tail exceed result budget"));
        }
        content.push_str(&projected);
        Ok(())
    }

    fn spec_for(&self) -> ToolSpec {
        let profile = self.runtime.job_profile();
        let ref_schema = json!({"type":"string","minLength":1,"maxLength":2048});
        let rev_schema = json!({"type":["integer","null"],"minimum":1});
        let spec = match self.kind {
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
                description: "Get compact Job status and, for Process output, the last 512 raw bytes total across stdout/stderr. tail=0 disables output; explicit tail caps at 1024 combined. Never ACKs.".into(),
                parameters: json!({"type":"object","properties":{
                    "ref":ref_schema,
                    "rev":rev_schema,
                    "wait_ms":{"type":["integer","null"],"minimum":0,"maximum":MAX_WAIT_MS},
                    "tail":{"type":["integer","null"],"minimum":0,"maximum":1024}
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
                    "offset":{"type":["integer","null"]},
                    "limit":{"type":["integer","null"],"minimum":1,"maximum":self.runtime.job_profile().read_page_max_bytes},
                    "encoding":{"type":["string","null"],"enum":["text","base64",null]},
                    "wait_ms":{"type":["integer","null"],"minimum":0,"maximum":MAX_WAIT_MS}
                },"additionalProperties":false}),
                effect: ToolEffect::Read,
            },
            JobsKind::Ack => ToolSpec {
                name: "jobs_ack".into(),
                description: "Finish this caller's terminal Job output with ref alone, including unread bytes; offset explicitly ACKs only an already-delivered contiguous raw prefix. Never ACKs another owner.".into(),
                parameters: json!({"type":"object","properties":{
                    "ref":ref_schema,
                    "view":{"type":["string","null"],"enum":["result","stdout","stderr",null]},
                    "offset":{"type":"integer","minimum":0}
                },"required":["ref"],"additionalProperties":false}),
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
        };
        // The model descriptor comes from the same validated tool profile
        // as dispatch limits. AW alone supplies Agent-history specialization.
        let factory = aw_tools::toolset::Toolset::builder()
            .jobs(profile)
            .only([])
            .build()
            .expect("validated AW Jobs profile");
        let operation = match self.kind {
            JobsKind::List => aw_tools::OperationId::JobsList,
            JobsKind::Get => aw_tools::OperationId::JobsGet,
            JobsKind::Read => aw_tools::OperationId::JobsRead,
            JobsKind::Ack => aw_tools::OperationId::JobsAck,
            JobsKind::Send => aw_tools::OperationId::JobsSend,
            JobsKind::Cancel => aw_tools::OperationId::JobsCancel,
            JobsKind::Subscribe => aw_tools::OperationId::JobsSubscribe,
        };
        let descriptor = factory
            .describe_operation(operation, spec.parameters, &spec.description)
            .expect("AW Jobs schema is a configured object");
        ToolSpec {
            name: descriptor.name.into(),
            description: descriptor.description,
            parameters: descriptor.parameters,
            effect: spec.effect,
        }
    }
}

impl Tool for JobsTool {
    fn spec(&self) -> ToolSpec {
        self.spec_for()
    }

    fn result_ceiling_bytes(&self) -> usize {
        if self.kind == JobsKind::Read {
            self.runtime
                .job_profile()
                .read_page_max_bytes
                .min(aw_agent_core::tool::MAX_AGENT_TOOL_RESULT_BYTES)
        } else {
            aw_agent_core::tool::MAX_TOOL_RESULT_BYTES
        }
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
                Self::object(arguments, &["ref", "rev", "wait_ms", "tail"])?;
                let _ = Self::reference(arguments, true)?;
                let _ = Self::revision(arguments)?;
                if arguments
                    .get("tail")
                    .filter(|value| !value.is_null())
                    .is_some_and(|tail| {
                        tail.as_u64()
                            .is_none_or(|n| n > self.runtime.job_profile().get_tail_max as u64)
                    })
                {
                    return Err(Error::Denied(
                        "jobs_get tail exceeds configured combined-byte ceiling",
                    ));
                }
                if let Some(wait) = arguments.get("wait_ms").filter(|value| !value.is_null()) {
                    if wait
                        .as_u64()
                        .is_none_or(|value| value > self.runtime.job_profile().get_wait_max_ms)
                    {
                        return Err(Error::Denied(
                            "jobs_get wait_ms exceeds configured Job wait ceiling",
                        ));
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
                    .get("limit")
                    .filter(|value| !value.is_null())
                    .is_some_and(|limit| {
                        limit
                            .as_u64()
                            .and_then(|value| usize::try_from(value).ok())
                            .is_none_or(|value| {
                                self.runtime
                                    .job_profile()
                                    .resolve_read_page(Some(value))
                                    .is_err()
                            })
                    })
                {
                    return Err(Error::Denied(
                        "jobs_read limit exceeds configured Job page ceiling",
                    ));
                }
                if arguments
                    .get("wait_ms")
                    .filter(|value| !value.is_null())
                    .is_some_and(|wait| {
                        wait.as_u64()
                            .is_none_or(|value| value > self.runtime.job_profile().read_wait_max_ms)
                    })
                {
                    return Err(Error::Denied(
                        "jobs_read wait_ms exceeds configured Job wait ceiling",
                    ));
                }
                if arguments
                    .get("offset")
                    .filter(|value| !value.is_null())
                    .is_some_and(|value| value.as_i64().is_none() && value.as_u64().is_none())
                {
                    return Err(Error::Denied("offset must be a signed byte integer"));
                }
            }
            JobsKind::Ack => {
                Self::object(arguments, &["ref", "view", "offset"])?;
                let _ = Self::reference(arguments, false)?;
                let _ = Self::view(arguments)?;
                if arguments
                    .get("offset")
                    .is_some_and(|v| v.as_u64().is_none())
                {
                    return Err(Error::Denied(
                        "jobs_ack offset must be an absolute nonnegative byte count",
                    ));
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
            let read_budget = if self.kind == JobsKind::Read {
                let bytes =
                    ctx.extended_result_budget(aw_agent_core::tool::MAX_AGENT_TOOL_RESULT_BYTES)?;
                Some(
                    SerializedResultBytes::new(bytes)
                        .expect("validated ToolContext result budget is non-zero"),
                )
            } else {
                None
            };
            let budget = read_budget
                .map(SerializedResultBytes::get)
                .unwrap_or(ctx.result_budget(self.runtime.result_limit())?);
            let host = self.runtime.host_for(&ctx)?;
            // Mark delivery only after the *entire* tool response validates.
            // A tail/status peek never mints ACKable byte proof.
            let mut delivered = None;
            let mut finite_read_id = None;
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
                                let row = if let Some(output) = router
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
                                            Ok(snapshot) => {
                                                let reference = host.job_ref(&id, None)?;
                                                Some(Self::render_snapshot(&reference, &snapshot)?)
                                            }
                                            Err(_) => None,
                                        },
                                        Ok((_, Some(_))) => {
                                            return Err(Error::Protocol(
                                                "jobs_list durable index contains descendant ref",
                                            ))
                                        }
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
                        let reference = host.job_ref(&snapshot.id, None)?;
                        let row = Self::render_snapshot(&reference, &snapshot)?;
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
                            if arguments.get("tail").and_then(Value::as_u64).unwrap_or(0) != 0 {
                                return Err(Error::Denied(
                                    "Agent/Turn status does not have Process tails",
                                ));
                            }
                            return Ok(output);
                        }
                    }
                    let (id, node) = host.resolve_job_ref(&reference)?;
                    let kind = host
                        .manager()
                        .finite_kind(&id)
                        .map_err(|_| Error::Denied("Job kind unavailable"))?;
                    let snapshot = Self::conditional_snapshot(
                        &host,
                        &id,
                        node,
                        revision,
                        wait_ms,
                        &ctx.cancellation,
                    )
                    .await?;
                    let reference = host.job_ref(&id, node)?;
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
                        let state = host
                            .manager()
                            .node_state(&id, node)
                            .map_err(|_| Error::Denied("Job node is unavailable"))?;
                        aw_tools::receipt::render_plain(
                            aw_tools::receipt::Receipt::NodeStatus {
                                reference: &reference,
                                tool: kind,
                                revision: snapshot.sequence,
                                state,
                                journal_unknown: snapshot.journal_unknown,
                                terminal: if matches!(
                                    state,
                                    aw_tools::filesystem::job::NodeState::Terminal { .. }
                                ) {
                                    host.manager().process_terminal(&id, node).map_err(|_| {
                                        Error::Protocol("Job Process terminal facts unavailable")
                                    })?
                                } else {
                                    None
                                },
                            },
                            aw_tools::filesystem::job_manager::MAX_COMPACT_JOB_STATUS_BYTES,
                        )
                        .map_err(|_| Error::Protocol("cannot render typed compact node status"))?
                    } else if snapshot.lifecycle == aw_tools::filesystem::job::Lifecycle::Terminal
                        && snapshot.total == 1
                    {
                        aw_tools::receipt::render_plain(
                            aw_tools::receipt::Receipt::JobStatus {
                                reference: &reference,
                                snapshot: &snapshot,
                                terminal: host.manager().process_terminal(&id, 1).map_err(
                                    |_| Error::Protocol("Job Process terminal facts unavailable"),
                                )?,
                                next: None,
                            },
                            aw_tools::filesystem::job_manager::MAX_COMPACT_JOB_STATUS_BYTES,
                        )
                        .map_err(|_| Error::Protocol("cannot render typed compact Job status"))?
                    } else {
                        Self::render_snapshot(&reference, &snapshot)?
                    };
                    let mut content = content;
                    self.append_process_tail(
                        &ctx,
                        &host,
                        TailRequest {
                            id: &id,
                            node,
                            kind,
                            requested: arguments.get("tail").and_then(Value::as_u64),
                            budget,
                        },
                        &mut content,
                    )?;
                    ToolOutput::text(content)
                }
                JobsKind::Read => {
                    let reference = Self::reference(arguments, true)?.to_owned();
                    let position = Self::read_position(arguments);
                    let view = Self::view(arguments)?.map(str::to_owned);
                    let filter = Self::filter(arguments)?;
                    let cursor = arguments
                        .get("cursor")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    let requested_limit = arguments
                        .get("limit")
                        .and_then(Value::as_u64)
                        .and_then(|value| usize::try_from(value).ok());
                    let limit = self
                        .runtime
                        .job_profile()
                        .resolve_read_page(requested_limit)
                        .map_err(|_| Error::Denied("invalid jobs_read raw page size"))?
                        .get();
                    let read_budget =
                        read_budget.expect("jobs_read always resolves serialized result budget");
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
                            if matches!(position, OutputPosition::Tail(_)) {
                                return Err(Error::Denied(
                                    "Agent history uses cursors, not negative offsets",
                                ));
                            }
                            return Ok(output);
                        }
                    }
                    if filter.is_some()
                        || cursor.is_some()
                        || view.as_deref().is_some_and(|view| {
                            matches!(view, "messages" | "tool_calls" | "tool_outputs" | "trace")
                        })
                    {
                        return Err(Error::Denied(
                            "Agent history fields are invalid for finite Job output",
                        ));
                    }
                    if wait_ms != 0 && matches!(position, OutputPosition::Tail(_)) {
                        return Err(Error::Denied(
                            "negative-offset jobs_read is an immediate tail snapshot",
                        ));
                    }
                    let (id, node) = host.resolve_job_ref(&reference)?;
                    finite_read_id = Some(id.clone());
                    if !self.runtime.payload_read_authorized(&ctx)
                        || !host.payload_authorized(&ctx.actor, &id)
                    {
                        return Err(Error::Denied("Job payload authority is not live"));
                    }
                    let kind = host
                        .manager()
                        .finite_kind(&id)
                        .map_err(|_| Error::Denied("Job kind is unavailable"))?;
                    let encoding = Self::encoding(arguments)?;
                    let view = view.as_deref();
                    let content = if kind == FiniteJobKind::Graph && node.is_none() {
                        let OutputPosition::Absolute(offset) = position else {
                            return Err(Error::Denied(
                                "Graph aggregate result needs an absolute offset",
                            ));
                        };
                        if view.is_some_and(|view| view != "result") {
                            return Err(Error::Denied(
                                "Graph root exposes only the result view; use n:<coordinate> for streams",
                            ));
                        }
                        if wait_ms != 0 {
                            let _ = Self::conditional_snapshot(
                                &host,
                                &id,
                                None,
                                None,
                                wait_ms,
                                &ctx.cancellation,
                            )
                            .await?;
                        }
                        Self::graph_result_page(
                            host.manager(),
                            &id,
                            offset,
                            limit,
                            encoding,
                            read_budget,
                        )?
                    } else {
                        let (coordinate, stream) =
                            Self::stream_for(host.manager(), &id, kind, node, view)?;
                        if let OutputPosition::Absolute(offset) = position {
                            Self::wait_for_output(
                                &host,
                                &id,
                                coordinate,
                                stream,
                                offset,
                                wait_ms,
                                &ctx.cancellation,
                            )
                            .await?;
                        }
                        if !self.runtime.payload_read_authorized(&ctx)
                            || !host.payload_authorized(&ctx.actor, &id)
                        {
                            return Err(Error::Denied(
                                "Job payload authority revoked while waiting",
                            ));
                        }
                        let owner = host
                            .manager()
                            .job_output_owner(&id)
                            .map_err(|_| Error::Denied("Job output owner is unavailable"))?;
                        let page = host
                            .manager()
                            .output_page_for_owner_at(
                                &id, coordinate, stream, owner, position, limit,
                            )
                            .map_err(|_| Error::Denied("Job output view is unavailable"))?;
                        let (rendered, credited) =
                            Self::render_output_page(&page, page.offset, encoding, read_budget)?;
                        delivered = Some((id, coordinate, stream, owner, page.offset, credited));
                        rendered
                    };
                    ToolOutput::text(content)
                }
                JobsKind::Ack => {
                    let (id, node) = host.resolve_job_ref(Self::reference(arguments, false)?)?;
                    let reference = host.job_ref(&id, node)?;
                    if !self.runtime.control_authorized(&ctx, "jobs_ack")
                        || !host.payload_authorized(&ctx.actor, &id)
                    {
                        return Err(Error::Denied("Job ACK Control authority is not live"));
                    }
                    let kind = host
                        .manager()
                        .finite_kind(&id)
                        .map_err(|_| Error::Denied("Job kind is unavailable"))?;
                    let view = Self::view(arguments)?;
                    let offset = arguments.get("offset").and_then(Value::as_u64);
                    let owner = host
                        .manager()
                        .job_output_owner(&id)
                        .map_err(|_| Error::Denied("Job output owner is unavailable"))?;
                    let ack = if let Some(through) = offset {
                        if kind == FiniteJobKind::Graph && node.is_none() {
                            return Err(Error::Denied(
                                "Graph root result is synthesized; ACK exact node output offsets",
                            ));
                        }
                        let (coordinate, stream) =
                            Self::stream_for(host.manager(), &id, kind, node, view)?;
                        self.runtime.with_control_commit(&ctx, "jobs_ack", || {
                            host.manager()
                                .acknowledge_delivered_output(&id, coordinate, stream, owner, through)
                                .map_err(|_| Error::Denied(
                                    "Job ACK exceeds the contiguous delivered prefix for this owner",
                                ))
                        })?
                    } else {
                        let stream = match view {
                            None => None,
                            Some("result") => Some(OutputStream::Result),
                            Some("stdout") => Some(OutputStream::Stdout),
                            Some("stderr") => Some(OutputStream::Stderr),
                            _ => return Err(Error::Denied("invalid retained Job ACK view")),
                        };
                        if kind == FiniteJobKind::Graph && node.is_none() && stream.is_some() {
                            return Err(Error::Denied(
                                "Graph root has no ACKable aggregate stream; omit view to finish owned node outputs",
                            ));
                        }
                        let ack = self.runtime.with_control_commit(&ctx, "jobs_ack", || {
                            host.manager()
                                .finish_output_for_owner(&id, node, stream, owner)
                                .map_err(|error| match error {
                                    aw_tools::filesystem::job_manager::Error::Busy => {
                                        Error::Conflict("Job output is not yet terminal")
                                    }
                                    aw_tools::filesystem::job_manager::Error::ReplayUnknown => {
                                        Error::Conflict(
                                            "Job effect is unknown; reconcile before finishing output",
                                        )
                                    }
                                    _ => Error::Denied(
                                        "Job output cannot be finished under the current owner",
                                    ),
                                })
                        })?;
                        let content = aw_tools::receipt::render_plain(
                            aw_tools::receipt::Receipt::Ack {
                                reference: &reference,
                                mode: aw_tools::receipt::AckMode::Finish,
                                changed: ack.changed,
                                released: ack.released,
                            },
                            budget,
                        )
                        .map_err(|error| match error {
                            aw_tools::toolset::ToolsetError::ResultLimit => {
                                Error::Limit("Job ACK receipt exceeds current result budget")
                            }
                            _ => Error::Protocol("cannot render typed Job ACK receipt"),
                        })?;
                        return Ok(ToolOutput::text(content));
                    };
                    let through = offset.expect("through ACK branch has an offset");
                    let content = aw_tools::receipt::render_plain(
                        aw_tools::receipt::Receipt::Ack {
                            reference: &reference,
                            mode: aw_tools::receipt::AckMode::Through(through),
                            changed: ack.changed,
                            released: ack.released,
                        },
                        budget,
                    )
                    .map_err(|error| match error {
                        aw_tools::toolset::ToolsetError::ResultLimit => {
                            Error::Limit("Job ACK receipt exceeds current result budget")
                        }
                        _ => Error::Protocol("cannot render typed Job ACK receipt"),
                    })?;
                    ToolOutput::text(content)
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
                        ControlReplayDecision::Unknown => ToolOutput::error(
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
                                let canonical_ref = host.job_ref(&id, node)?;
                                let canonical = canonical_ref.to_string();
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
                        ControlReplayDecision::Unknown => ToolOutput::error(
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
                                let canonical_ref = host.job_ref(&id, node)?;
                                let canonical = canonical_ref.to_string();
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
                                        let content = aw_tools::receipt::render_plain(
                                            aw_tools::receipt::Receipt::Cancel {
                                                reference: &canonical_ref,
                                                revision: snapshot.sequence,
                                                changed,
                                            },
                                            budget,
                                        )
                                        .map_err(|error| match error {
                                            aw_tools::toolset::ToolsetError::ResultLimit => {
                                                Error::Limit(
                                                    "Job cancellation receipt exceeds current result budget",
                                                )
                                            }
                                            _ => Error::Protocol(
                                                "cannot render typed Job cancellation receipt",
                                            ),
                                        })?;
                                        let output = ToolOutput::text(content);
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
                        ControlReplayDecision::Unknown => ToolOutput::error(
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
                                                recipient: recipient.clone(),
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
                                let canonical_ref = host.job_ref(&id, node)?;
                                let canonical = canonical_ref.to_string();
                                self.runtime.bind_control_replay_target(key, &canonical)?;
                                match host.subscribe_finite(
                                    crate::job_host::FiniteSubscriptionRequest {
                                        actor: &ctx.actor,
                                        id: &id,
                                        node,
                                        expected_revision: revision,
                                        recipient_ref: &recipient.canonical_ref,
                                        recipient_authority_ref: &recipient.authority_ref,
                                        rules,
                                    },
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
            if let Some(read_budget) = read_budget {
                output.validate_with_limit(read_budget.get())?;
            } else {
                output.validate()?;
            }
            // Synthetic Graph-root projections also need a final live check:
            // no delivery ledger entry exists for their synthesized result.
            if let Some(id) = finite_read_id.as_deref() {
                if !self.runtime.payload_read_authorized(&ctx)
                    || !host.payload_authorized(&ctx.actor, id)
                {
                    return Err(Error::Denied(
                        "Job output authority revoked before delivery",
                    ));
                }
            }
            if let Some((id, coordinate, stream, owner, start, count)) = delivered {
                if !self.runtime.payload_read_authorized(&ctx)
                    || !host.payload_authorized(&ctx.actor, &id)
                {
                    return Err(Error::Denied(
                        "Job output authority revoked before delivery",
                    ));
                }
                host.manager()
                    .record_delivered_output(&id, coordinate, stream, owner, start, count)
                    .map_err(|_| {
                        Error::Denied("Job output owner cannot record this delivered prefix")
                    })?;
            }
            Ok(output)
        })
    }
}

#[cfg(test)]
mod delivery_tests {
    use super::*;

    fn serialized(bytes: usize) -> SerializedResultBytes {
        SerializedResultBytes::new(bytes).expect("test serialized budget")
    }

    #[test]
    fn serialized_envelope_narrows_projection_without_changing_raw_page() {
        let page = OutputPage {
            offset: 0,
            bytes: vec![0xff; 700],
            next_offset: None,
            writer_finished: true,
            truncated: false,
            total_bytes: 700,
            retained_bytes: 700,
        };
        let (lossy, used) =
            JobsTool::render_output_page(&page, 0, "text", serialized(512)).unwrap();
        assert!(used > 0 && used < page.bytes.len());
        assert!(lossy.len() <= 512);
        assert!(lossy.contains(&format!("more offset={used}")));
        assert!(!lossy.contains("more offset=700"));
        let (b64, raw) = JobsTool::render_output_page(&page, 0, "base64", serialized(512)).unwrap();
        assert!(raw > used && raw < page.bytes.len());
        assert!(b64.len() <= 512);
        assert!(b64.contains(&format!("more offset={raw}")));
        assert_eq!(page.bytes.len(), 700);
        assert_eq!(page.retained_bytes, 700);
        assert!(JobsTool::render_output_page(&page, 0, "base64", serialized(1)).is_err());
    }

    #[test]
    fn empty_page_carries_no_raw_credit_even_if_metadata_fits() {
        let page = OutputPage {
            offset: 0,
            bytes: Vec::new(),
            next_offset: None,
            writer_finished: true,
            truncated: true,
            total_bytes: 100,
            retained_bytes: 0,
        };
        let (rendered, used) =
            JobsTool::render_output_page(&page, 0, "text", serialized(512)).unwrap();
        assert_eq!(used, 0);
        assert_eq!(rendered, "truncated retained=0 total=100");
        assert!(JobsTool::render_output_page(&page, 0, "text", serialized(1)).is_err());
    }
}
