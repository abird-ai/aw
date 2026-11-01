//! Model-facing projections over the one live `JobRuntime`.
//! `batch`, direct shells and `job_start` all construct the same bounded Graph;
//! execution always goes through `JobHost`, never recursive public-tool dispatch.
use crate::{
    job_runtime::{JobRuntime, StartDecision},
    shell, Error, Result,
};
use aw_agent_core::{Tool, ToolContext, ToolEffect, ToolFuture, ToolOutput, ToolSpec};
use aw_tools::filesystem::{
    batch::{self},
    job::Lifecycle,
    job_manager::{OutputStream, MAX_JOB_LIST_PAGE},
    status::Primary,
};
use base64::Engine as _;
use serde_json::{json, Value};
use std::sync::Arc;

struct HandoffGuard {
    host: Arc<crate::job_host::JobHost>,
    id: String,
    armed: bool,
}
impl HandoffGuard {
    fn new(host: Arc<crate::job_host::JobHost>, id: String) -> Self {
        Self {
            host,
            id,
            armed: true,
        }
    }
    fn expose(&mut self) -> Result<()> {
        if self.armed {
            self.host.expose(&self.id)?;
            self.armed = false;
        }
        Ok(())
    }
    fn disarm(&mut self) {
        self.armed = false;
    }
}
impl Drop for HandoffGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.host.expose(&self.id);
        }
    }
}

fn graph_error(primary: Primary) -> bool {
    matches!(primary, Primary::Fail | Primary::Mixed | Primary::Unknown)
}

fn handoff_receipt(id: &str, snapshot: &aw_tools::filesystem::job_manager::Snapshot) -> ToolOutput {
    let content = if snapshot.lifecycle == aw_tools::filesystem::job::Lifecycle::Terminal {
        let primary = snapshot
            .summary
            .map_or(Primary::Unknown, |summary| summary.0);
        format!("{} {id} result=job_output", primary.as_str())
    } else {
        format!("running {id} {}/{}", snapshot.finished, snapshot.total)
    };
    ToolOutput {
        content,
        is_error: snapshot
            .summary
            .is_some_and(|summary| graph_error(summary.0)),
        truncated: false,
    }
}

#[derive(Clone)]
pub(crate) struct JobShell {
    runtime: Arc<JobRuntime>,
    shell: Arc<shell::NativeShell>,
}
impl JobShell {
    pub(crate) fn new(runtime: Arc<JobRuntime>, shell: Arc<shell::NativeShell>) -> Self {
        Self { runtime, shell }
    }

    fn retained_text(
        host: &crate::job_host::JobHost,
        id: &str,
        stream: OutputStream,
    ) -> Result<(String, bool)> {
        let page = host
            .manager()
            .output_page(id, 1, stream, 0, aw_tools::task_ops::MAX_TASK_OUTPUT_PAGE)
            .map_err(|_| Error::Protocol("completed shell Job output unavailable"))?;
        if page.next_offset.is_some() {
            return Err(Error::Protocol(
                "completed shell Job output exceeds direct compatibility page",
            ));
        }
        Ok((
            String::from_utf8_lossy(&page.bytes).into_owned(),
            page.truncated,
        ))
    }
}
impl Tool for JobShell {
    fn spec(&self) -> ToolSpec {
        self.shell.spec()
    }

    fn validate(&self, arguments: &Value) -> Result<()> {
        self.shell.validate(arguments)
    }

    fn can_run_parallel(&self, first: &Value, second: &Value) -> bool {
        self.shell.can_run_parallel(first, second)
    }

    fn execute<'a>(&'a self, ctx: ToolContext, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(arguments)?;
            let direct_budget = ctx.result_budget(self.runtime.result_limit())?;
            let graph = json!({"ops":[[self.shell.spec().name, arguments.clone()]]});
            let mut started = match self.runtime.start_replay(&ctx, &graph, false)? {
                StartDecision::New(started) => started,
                StartDecision::Existing { id, host } => {
                    host.expose(&id)?;
                    let snapshot = host
                        .manager()
                        .runtime_snapshot(&id)
                        .map_err(|_| Error::Protocol("replayed shell Job is unavailable"))?;
                    return Ok(handoff_receipt(&id, &snapshot));
                }
                StartDecision::Unknown => {
                    return Ok(ToolOutput::error(
                        "unknown shell Job admission; do not retry automatically",
                    ));
                }
            };
            let mut handoff = HandoffGuard::new(started.host.clone(), started.id.clone());
            let completed = tokio::select! {
                result = &mut started.task => result,
                _ = ctx.cancellation.cancelled() => {
                    handoff.expose()?;
                    let _ = self.runtime.cancel(&ctx, &started.id)?;
                    return Err(Error::Cancelled);
                },
            };
            let snapshot = match completed {
                Ok(Ok(snapshot)) => snapshot,
                Ok(Err(error)) => {
                    handoff.expose()?;
                    return Err(error);
                }
                Err(_) => {
                    let _ = started.host.settle_runner_abort(&started.id);
                    handoff.expose()?;
                    return Err(Error::Protocol(
                        "shell Job runner failed; Job exposed for inspection",
                    ));
                }
            };
            let (result, _) =
                Self::retained_text(&started.host, &started.id, OutputStream::Result)?;
            let (stdout, stdout_truncated) =
                Self::retained_text(&started.host, &started.id, OutputStream::Stdout)?;
            let (stderr, stderr_truncated) =
                Self::retained_text(&started.host, &started.id, OutputStream::Stderr)?;
            let exit = result
                .split_whitespace()
                .find_map(|field| field.strip_prefix("exit="))
                .unwrap_or("unknown");
            let primary = snapshot
                .summary
                .map_or(Primary::Unknown, |summary| summary.0);
            let direct = ToolOutput {
                content: format!("exit: {exit}\nstdout:\n{stdout}\nstderr:\n{stderr}"),
                is_error: primary != Primary::Ok,
                truncated: stdout_truncated || stderr_truncated,
            };
            match exit {
                "deadline" => Err(Error::Timeout),
                "cancelled" => Err(Error::Cancelled),
                _ if primary == Primary::Unknown => {
                    Err(Error::Transport("process outcome uncertain"))
                }
                _ if direct.content.len() > direct_budget => {
                    handoff.expose()?;
                    let output = handoff_receipt(&started.id, &snapshot);
                    output.validate()?;
                    Ok(output)
                }
                _ => {
                    direct.validate()?;
                    handoff.disarm();
                    started
                        .host
                        .manager()
                        .discard_unexposed_terminal(&started.id)
                        .map_err(|_| Error::Protocol("cannot retire completed hidden shell Job"))?;
                    Ok(direct)
                }
            }
        })
    }
}

#[derive(Clone)]
pub struct Batch {
    runtime: Arc<JobRuntime>,
}
impl Batch {
    pub fn new(runtime: Arc<JobRuntime>) -> Self {
        Self { runtime }
    }
}
impl Tool for Batch {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "batch".into(),
            description: "Run a bounded Graph with compact [tool,args,when?] nodes. Eligible native tools and prepared shell interpreters share one Job; node authority is rechecked individually. Results are coordinate-keyed and long/oversized execution hands off the same Job to job_* controls.".into(),
            parameters: batch::parameters_for(self.runtime.node_tools()),
            // Composite outer effect: the wrapper may contain reads or writes.
            // Node effects are independently source/effect/grant checked.
            effect: ToolEffect::Unknown,
        }
    }

    fn validate(&self, arguments: &Value) -> Result<()> {
        self.runtime.validate_graph(arguments)
    }

    fn execute<'a>(&'a self, ctx: ToolContext, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(arguments)?;
            let direct_budget = ctx.result_budget(self.runtime.result_limit())?;
            let mut started = match self.runtime.start_replay(&ctx, arguments, false)? {
                StartDecision::New(started) => started,
                StartDecision::Existing { id, host } => {
                    host.expose(&id)?;
                    let snapshot = host
                        .manager()
                        .runtime_snapshot(&id)
                        .map_err(|_| Error::Protocol("replayed batch Job is unavailable"))?;
                    return Ok(handoff_receipt(&id, &snapshot));
                }
                StartDecision::Unknown => {
                    return Ok(ToolOutput::error(
                        "unknown batch Job admission; do not retry automatically",
                    ));
                }
            };
            let mut handoff = HandoffGuard::new(started.host.clone(), started.id.clone());
            let completed = tokio::select! {
                result = &mut started.task => result,
                _ = ctx.cancellation.cancelled() => {
                    handoff.expose()?;
                    let _ = self.runtime.cancel(&ctx, &started.id)?;
                    return Err(Error::Cancelled);
                },
            };
            let snapshot = match completed {
                Ok(Ok(snapshot)) => snapshot,
                Ok(Err(error)) => {
                    handoff.expose()?;
                    return Err(error);
                }
                Err(_) => {
                    let _ = started.host.settle_runner_abort(&started.id);
                    handoff.expose()?;
                    return Err(Error::Protocol(
                        "batch runner task failed; Job exposed for inspection",
                    ));
                }
            };
            let rendered = started
                .host
                .manager()
                .render_graph_result(&started.id, direct_budget)
                .map_err(|_| Error::Protocol("cannot render completed batch Graph"))?;
            if let Some(content) = rendered {
                let primary = snapshot
                    .summary
                    .map_or(Primary::Unknown, |summary| summary.0);
                let output = ToolOutput {
                    content,
                    is_error: graph_error(primary),
                    truncated: false,
                };
                output.validate()?;
                handoff.disarm();
                started
                    .host
                    .manager()
                    .discard_unexposed_terminal(&started.id)
                    .map_err(|_| Error::Protocol("cannot retire completed hidden batch Job"))?;
                return Ok(output);
            }
            handoff.expose()?;
            let output = handoff_receipt(&started.id, &snapshot);
            output.validate()?;
            Ok(output)
        })
    }
}

/// Control schemas are defined here before the registration gate so the final
/// surface can be measured/tested without prematurely advertising `job_start`.
pub fn job_list_parameters() -> Value {
    job_list_parameters_with_limit(MAX_JOB_LIST_PAGE)
}
fn job_list_parameters_with_limit(max_limit: usize) -> Value {
    let max_limit = max_limit.clamp(1, MAX_JOB_LIST_PAGE);
    let default_limit = 64.min(max_limit);
    json!({"type":"object","properties":{
        "offset":{"type":"integer","minimum":0,"default":0},
        "limit":{"type":"integer","minimum":1,"maximum":max_limit,"default":default_limit}
    },"additionalProperties":false})
}
pub fn job_id_parameters() -> Value {
    json!({"type":"object","properties":{
        "id":{"type":"string","minLength":1,"maxLength":256}
    },"required":["id"],"additionalProperties":false})
}
pub fn job_output_parameters() -> Value {
    job_output_parameters_with_limit(aw_tools::task_ops::MAX_TASK_OUTPUT_PAGE)
}
fn job_output_parameters_with_limit(result_limit: usize) -> Value {
    let max_raw = result_limit.clamp(1, aw_tools::task_ops::MAX_TASK_OUTPUT_PAGE);
    json!({"type":"object","properties":{
        "id":{"type":"string","minLength":1,"maxLength":256},
        "node":{"type":["integer","null"],"minimum":1,"maximum":64},
        "stream":{"type":["string","null"],"enum":["result","stdout","stderr",null]},
        "offset":{"type":"integer","minimum":0,"default":0},
        "max_output_bytes":{"type":["integer","null"],"minimum":1,"maximum":max_raw},
        "encoding":{"type":"string","enum":["text","base64"],"default":"text"}
    },"required":["id"],"additionalProperties":false})
}
pub fn job_ack_parameters() -> Value {
    json!({"type":"object","properties":{
        "id":{"type":"string","minLength":1,"maxLength":256},
        "node":{"type":["integer","null"],"minimum":1,"maximum":64},
        "stream":{"type":["string","null"],"enum":["result","stdout","stderr",null]},
        "offset":{"type":"integer","minimum":0}
    },"required":["id","offset"],"additionalProperties":false})
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobToolKind {
    Start,
    List,
    Get,
    Output,
    Ack,
    Cancel,
    Restart,
}

#[derive(Clone)]
pub struct JobControl {
    runtime: Arc<JobRuntime>,
    kind: JobToolKind,
}
impl JobControl {
    pub fn new(runtime: Arc<JobRuntime>, kind: JobToolKind) -> Self {
        Self { runtime, kind }
    }

    fn id(value: &Value) -> Result<&str> {
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .ok_or(Error::Denied("Job control requires id"))?;
        if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
            return Err(Error::Denied("invalid Job id"));
        }
        Ok(id)
    }

    fn node(
        manager: &aw_tools::filesystem::job_manager::JobManager,
        id: &str,
        value: &Value,
    ) -> Result<usize> {
        let snapshot = manager
            .get(id)
            .map_err(|_| Error::Denied("Job is not exposed or missing"))?;
        let requested = value.get("node").and_then(Value::as_u64);
        let coordinate = match (snapshot.total, requested) {
            (1, None) => 1,
            (1, Some(1)) => 1,
            (1, Some(_)) => return Err(Error::Denied("one-node Job has only node 1")),
            (_, None) => return Err(Error::Denied("multi-node Job requires node")),
            (_, Some(node)) => usize::try_from(node)
                .ok()
                .filter(|node| (1..=snapshot.total).contains(node))
                .ok_or(Error::Denied("Job node is out of range"))?,
        };
        Ok(coordinate)
    }

    fn stream(
        manager: &aw_tools::filesystem::job_manager::JobManager,
        id: &str,
        node: usize,
        value: &Value,
    ) -> Result<OutputStream> {
        let plans = manager
            .plans(id)
            .map_err(|_| Error::Denied("Job plan is unavailable"))?;
        let shell = plans
            .get(node.saturating_sub(1))
            .ok_or(Error::Denied("Job node is out of range"))?
            .tool()
            .is_shell();
        match value.get("stream").and_then(Value::as_str) {
            None if shell => Ok(OutputStream::Stdout),
            None => Ok(OutputStream::Result),
            Some("result") => Ok(OutputStream::Result),
            Some("stdout") if shell => Ok(OutputStream::Stdout),
            Some("stderr") if shell => Ok(OutputStream::Stderr),
            Some("stdout" | "stderr") => Err(Error::Denied(
                "native Job nodes expose only the result stream",
            )),
            _ => Err(Error::Denied("invalid Job output stream")),
        }
    }

    fn render_list_page(
        page: aw_tools::filesystem::job_manager::ListPage,
        offset: usize,
        budget: usize,
    ) -> Result<String> {
        let rows = page
            .jobs
            .iter()
            .map(|job| job.list_row())
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return Ok(String::new());
        }
        let mut selected = Vec::<String>::new();
        for row in rows.iter() {
            let consumed = selected.len() + 1;
            let more = consumed < rows.len() || page.next_offset.is_some();
            let mut candidate = selected.clone();
            candidate.push(row.clone());
            if more {
                candidate.push(format!("more offset={}", offset + consumed));
            }
            let rendered = candidate.join("\n");
            if rendered.len() > budget {
                break;
            }
            selected.push(row.clone());
        }
        if selected.is_empty() {
            return Err(Error::Limit(
                "one job_list row cannot fit configured result ceiling",
            ));
        }
        let consumed = selected.len();
        if consumed < rows.len() || page.next_offset.is_some() {
            selected.push(format!("more offset={}", offset + consumed));
        }
        Ok(selected.join("\n"))
    }

    fn render_output_page(
        page: &aw_tools::filesystem::job_manager::OutputPage,
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

    fn validate_object(arguments: &Value, allowed: &[&str]) -> Result<()> {
        let object = arguments
            .as_object()
            .ok_or(Error::Denied("Job control arguments must be an object"))?;
        if object.keys().any(|key| !allowed.contains(&key.as_str())) {
            return Err(Error::Denied("unknown Job control argument"));
        }
        Ok(())
    }
}

impl Tool for JobControl {
    fn spec(&self) -> ToolSpec {
        let (name, description, parameters, effect) = match self.kind {
            JobToolKind::Start => (
                "job_start",
                "Start the same bounded Graph as batch and expose its live Job id immediately. Eligible native tools and prepared shell interpreters use the same Job runtime.",
                batch::parameters_for(self.runtime.node_tools()),
                ToolEffect::Unknown,
            ),
            JobToolKind::List => (
                "job_list",
                "List recent exposed Jobs newest first. Use offset/limit for live logical paging; rows contain compact status only, never bulk output.",
                job_list_parameters_with_limit(self.runtime.job_list_limit()),
                ToolEffect::Read,
            ),
            JobToolKind::Get => (
                "job_get",
                "Get compact status for one exposed Job. This never acknowledges or deletes retained output.",
                job_id_parameters(),
                ToolEffect::Read,
            ),
            JobToolKind::Output => (
                "job_output",
                "Read one retained Job node stream by raw byte offset. Text is UTF-8-lossy; base64 is byte-exact. Reading never advances cleanup.",
                job_output_parameters_with_limit(self.runtime.result_limit()),
                ToolEffect::Read,
            ),
            JobToolKind::Ack => (
                "job_ack",
                "Acknowledge a raw retained-byte offset after safe receipt. Cleanup occurs only after every owner releases that prefix.",
                job_ack_parameters(),
                ToolEffect::Write,
            ),
            JobToolKind::Cancel => (
                "job_cancel",
                "Idempotently request cancellation of one exposed live Job. Pending nodes skip; already-started native effects settle safely.",
                job_id_parameters(),
                ToolEffect::Write,
            ),
            JobToolKind::Restart => (
                "job_restart",
                "Explicitly create a fresh Job from one retained terminal Graph after full current schema/authority revalidation. This may repeat side effects.",
                job_id_parameters(),
                ToolEffect::Write,
            ),
        };
        ToolSpec {
            name: name.into(),
            description: description.into(),
            parameters,
            effect,
        }
    }

    fn validate(&self, arguments: &Value) -> Result<()> {
        match self.kind {
            JobToolKind::Start => self.runtime.validate_graph(arguments),
            JobToolKind::List => {
                Self::validate_object(arguments, &["offset", "limit"])?;
                let offset = arguments.get("offset").and_then(Value::as_u64).unwrap_or(0);
                let max_limit = self.runtime.job_list_limit() as u64;
                let limit = arguments
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(64.min(max_limit));
                if usize::try_from(offset).is_err() || !(1..=max_limit).contains(&limit) {
                    return Err(Error::Denied("invalid job_list offset/limit"));
                }
                Ok(())
            }
            JobToolKind::Get | JobToolKind::Cancel | JobToolKind::Restart => {
                Self::validate_object(arguments, &["id"])?;
                Self::id(arguments).map(|_| ())
            }
            JobToolKind::Output => {
                Self::validate_object(
                    arguments,
                    &[
                        "id",
                        "node",
                        "stream",
                        "offset",
                        "max_output_bytes",
                        "encoding",
                    ],
                )?;
                Self::id(arguments)?;
                if arguments.get("offset").and_then(Value::as_u64).is_none()
                    && arguments.get("offset").is_some()
                {
                    return Err(Error::Denied("invalid job_output offset"));
                }
                if let Some(limit) = arguments.get("max_output_bytes") {
                    if !limit.is_null()
                        && !limit.as_u64().is_some_and(|value| {
                            (1..=self
                                .runtime
                                .result_limit()
                                .min(aw_tools::task_ops::MAX_TASK_OUTPUT_PAGE)
                                as u64)
                                .contains(&value)
                        })
                    {
                        return Err(Error::Denied("invalid job_output byte limit"));
                    }
                }
                if let Some(node) = arguments.get("node") {
                    if !node.is_null()
                        && !node.as_u64().is_some_and(|value| (1..=64).contains(&value))
                    {
                        return Err(Error::Denied("invalid Job node"));
                    }
                }
                if let Some(stream) = arguments.get("stream") {
                    if !stream.is_null()
                        && !stream
                            .as_str()
                            .is_some_and(|value| matches!(value, "result" | "stdout" | "stderr"))
                    {
                        return Err(Error::Denied("invalid Job stream"));
                    }
                }
                if let Some(encoding) = arguments.get("encoding") {
                    if !encoding
                        .as_str()
                        .is_some_and(|value| matches!(value, "text" | "base64"))
                    {
                        return Err(Error::Denied("invalid job_output encoding"));
                    }
                }
                Ok(())
            }
            JobToolKind::Ack => {
                Self::validate_object(arguments, &["id", "node", "stream", "offset"])?;
                Self::id(arguments)?;
                if arguments.get("offset").and_then(Value::as_u64).is_none() {
                    return Err(Error::Denied("job_ack requires non-negative offset"));
                }
                if let Some(node) = arguments.get("node") {
                    if !node.is_null()
                        && !node.as_u64().is_some_and(|value| (1..=64).contains(&value))
                    {
                        return Err(Error::Denied("invalid Job node"));
                    }
                }
                if let Some(stream) = arguments.get("stream") {
                    if !stream.is_null()
                        && !stream
                            .as_str()
                            .is_some_and(|value| matches!(value, "result" | "stdout" | "stderr"))
                    {
                        return Err(Error::Denied("invalid Job stream"));
                    }
                }
                Ok(())
            }
        }
    }

    fn execute<'a>(&'a self, ctx: ToolContext, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(arguments)?;
            let direct_budget = ctx.result_budget(self.runtime.result_limit())?;
            match self.kind {
                JobToolKind::Start => {
                    let (id, host, task) = match self.runtime.start_replay(&ctx, arguments, true)? {
                        StartDecision::New(started) => {
                            (started.id, started.host, Some(started.task))
                        }
                        StartDecision::Existing { id, host } => (id, host, None),
                        StartDecision::Unknown => {
                            return Ok(ToolOutput::error(
                                "unknown job_start admission; do not retry automatically",
                            ));
                        }
                    };
                    let snapshot = host
                        .manager()
                        .get(&id)
                        .map_err(|_| Error::Protocol("new Job status unavailable"))?;
                    if let Some(task) = task {
                        drop(task);
                    }
                    let output = if snapshot.lifecycle == Lifecycle::Terminal {
                        ToolOutput::text(snapshot.list_row())
                    } else {
                        ToolOutput::text(format!(
                            "running {id} {}/{}",
                            snapshot.finished, snapshot.total
                        ))
                    };
                    output.validate()?;
                    Ok(output)
                }
                JobToolKind::List => {
                    let host = self.runtime.host_for(&ctx)?;
                    let offset = arguments.get("offset").and_then(Value::as_u64).unwrap_or(0);
                    let offset = usize::try_from(offset)
                        .map_err(|_| Error::Denied("job_list offset too large"))?;
                    let limit = arguments
                        .get("limit")
                        .and_then(Value::as_u64)
                        .unwrap_or_else(|| 64.min(self.runtime.job_list_limit() as u64));
                    let limit = usize::try_from(limit)
                        .map_err(|_| Error::Denied("job_list limit too large"))?;
                    let page = host
                        .manager()
                        .list(offset, limit)
                        .map_err(|_| Error::Denied("cannot list live Jobs"))?;
                    let content = Self::render_list_page(page, offset, direct_budget)?;
                    let output = ToolOutput::text(content);
                    output.validate()?;
                    Ok(output)
                }
                JobToolKind::Get => {
                    let host = self.runtime.host_for(&ctx)?;
                    let content = host
                        .manager()
                        .render_job_status(Self::id(arguments)?)
                        .map_err(|_| Error::Denied("Job is not exposed or missing"))?;
                    if content.len() > direct_budget {
                        return Err(Error::Limit(
                            "complete Job status exceeds current result budget; retry job_get alone",
                        ));
                    }
                    let output = ToolOutput::text(content);
                    output.validate()?;
                    Ok(output)
                }
                JobToolKind::Output => {
                    let host = self.runtime.host_for(&ctx)?;
                    let id = Self::id(arguments)?;
                    if !host.payload_authorized(&ctx.actor, id) {
                        return Err(Error::Denied(
                            "Job payload authority is no longer live; status/control only",
                        ));
                    }
                    let node = Self::node(host.manager(), id, arguments)?;
                    let stream = Self::stream(host.manager(), id, node, arguments)?;
                    let offset = arguments.get("offset").and_then(Value::as_u64).unwrap_or(0);
                    let limit = arguments
                        .get("max_output_bytes")
                        .and_then(Value::as_u64)
                        .unwrap_or_else(|| {
                            self.runtime
                                .result_limit()
                                .min(aw_tools::task_ops::MAX_TASK_OUTPUT_PAGE)
                                as u64
                        }) as usize;
                    let page = host
                        .manager()
                        .output_page(id, node, stream, offset, limit)
                        .map_err(|_| Error::Denied("requested Job output is unavailable"))?;
                    let encoding = arguments
                        .get("encoding")
                        .and_then(Value::as_str)
                        .unwrap_or("text");
                    let content = Self::render_output_page(&page, offset, encoding, direct_budget)?;
                    let output = ToolOutput::text(content);
                    output.validate()?;
                    Ok(output)
                }
                JobToolKind::Ack => {
                    let host = self.runtime.host_for(&ctx)?;
                    let id = Self::id(arguments)?;
                    if !host.payload_authorized(&ctx.actor, id) {
                        return Err(Error::Denied(
                            "Job payload authority is no longer live; status/control only",
                        ));
                    }
                    let node = Self::node(host.manager(), id, arguments)?;
                    let stream = Self::stream(host.manager(), id, node, arguments)?;
                    let through = arguments
                        .get("offset")
                        .and_then(Value::as_u64)
                        .ok_or(Error::Denied("job_ack requires offset"))?;
                    let owner = host
                        .manager()
                        .job_output_owner(id)
                        .map_err(|_| Error::Denied("Job output owner unavailable"))?;
                    let ack = host
                        .manager()
                        .acknowledge_output(id, node, stream, owner, through)
                        .map_err(|_| Error::Denied("Job acknowledgement refused"))?;
                    Ok(ToolOutput::text(if ack.changed {
                        "ok"
                    } else {
                        "unchanged"
                    }))
                }
                JobToolKind::Cancel => {
                    let id = Self::id(arguments)?;
                    let (changed, snapshot) = self.runtime.cancel(&ctx, id)?;
                    let host = self.runtime.host_for(&ctx)?;
                    let state = match snapshot.lifecycle {
                        Lifecycle::Terminal => match snapshot.stop_reason {
                            Some(aw_tools::filesystem::job::StopReason::Cancelled) => "cancelled",
                            Some(aw_tools::filesystem::job::StopReason::Revoked) => "revoked",
                            None => snapshot
                                .summary
                                .map_or("unknown", |summary| summary.0.as_str()),
                        },
                        Lifecycle::Running => "running",
                        Lifecycle::Cancelling => "cancelling",
                        Lifecycle::Revoking => "revoking",
                    };
                    if changed {
                        let status = host
                            .manager()
                            .render_job_status(id)
                            .map_err(|_| Error::Protocol("cancelled Job status unavailable"))?;
                        if status.len() <= direct_budget {
                            Ok(ToolOutput::text(status))
                        } else {
                            Ok(ToolOutput::text(format!("{state} {id}")))
                        }
                    } else {
                        Ok(ToolOutput::text(format!("unchanged {state}")))
                    }
                }
                JobToolKind::Restart => {
                    let source = Self::id(arguments)?;
                    let (decision, started) = self.runtime.restart_graph(&ctx, source)?;
                    match decision {
                        crate::job_host::AdmissionDecision::New(id)
                        | crate::job_host::AdmissionDecision::Existing(id) => {
                            if let Some(started) = started {
                                drop(started.task);
                            }
                            Ok(ToolOutput::text(format!("running {id}")))
                        }
                        crate::job_host::AdmissionDecision::Unknown => Ok(ToolOutput::error(
                            "unknown restart; do not retry automatically",
                        )),
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{configure, LocalToolOptions, ToolMode};
    use aw_agent_core::{
        Agent, AgentOptions, CancellationToken, CompletionFuture, DeltaSink, Message, PrincipalId,
        Provider, ToolSource,
    };
    use aw_tools::filesystem::job_manager::Limits;
    use std::{fs, path::PathBuf};

    #[test]
    fn job_list_rows_fit_atomically_and_advance_live_offset() {
        use aw_tools::filesystem::{
            job::Lifecycle,
            job_manager::{ListPage, Snapshot},
        };
        let row = |id: &str| Snapshot {
            id: id.into(),
            label: "graph".into(),
            lifecycle: Lifecycle::Running,
            stop_reason: None,
            sequence: 1,
            finished: 1,
            total: 4,
            summary: None,
            detail: None,
        };
        let first = row("j11111111111111111111111111111111");
        let second = row("j22222222222222222222222222222222");
        let first_line = first.list_row();
        let budget = first_line.len() + "\nmore offset=8".len();
        let rendered = JobControl::render_list_page(
            ListPage {
                jobs: vec![first, second],
                next_offset: Some(9),
            },
            7,
            budget,
        )
        .unwrap();
        assert_eq!(rendered, format!("{first_line}\nmore offset=8"));
        assert!(rendered.len() <= budget);
        assert!(JobControl::render_list_page(
            ListPage {
                jobs: vec![row("j33333333333333333333333333333333")],
                next_offset: Some(1),
            },
            0,
            1,
        )
        .is_err());
    }

    #[test]
    fn job_output_projection_fits_budget_and_keeps_raw_offsets() {
        use aw_tools::filesystem::job_manager::OutputPage;
        let page = OutputPage {
            bytes: vec![0xff; 100],
            next_offset: None,
            writer_finished: true,
            truncated: false,
            total_bytes: 100,
            retained_bytes: 100,
        };
        let rendered = JobControl::render_output_page(&page, 0, "base64", 80).unwrap();
        assert!(rendered.len() <= 80);
        let mut lines = rendered.lines();
        let encoded = lines.next().unwrap();
        let raw = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        assert!(!raw.is_empty() && raw.len() < 100);
        assert_eq!(
            lines.next(),
            Some(format!("more offset={}", raw.len()).as_str())
        );

        let schema = job_output_parameters_with_limit(4096);
        assert_eq!(schema["properties"]["max_output_bytes"]["maximum"], 4096);
    }

    struct UnusedProvider;
    impl Provider for UnusedProvider {
        fn stream<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            _sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("provider unused")) })
        }
    }

    fn fixture() -> (PathBuf, Agent, Arc<JobRuntime>, ToolContext) {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let suffix = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let root =
            std::env::temp_dir().join(format!("aw-batch-tool-{}-{suffix}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.txt"), "a\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        );
        let tools = LocalToolOptions::new(&root, ToolMode::ReadOnly);
        let agent = configure(builder, actor.clone(), &tools)
            .unwrap()
            .build()
            .unwrap();
        let runtime = JobRuntime::new(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            Limits::default(),
        )
        .unwrap();
        let ctx = ToolContext {
            actor: actor.clone(),
            turn_id: 1,
            call_id: "batch-call".into(),
            source: ToolSource::trusted_host(),
            effect: ToolEffect::Unknown,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: agent.default_identity().unwrap(),
            scope: None,
        };
        (root, agent, runtime, ctx)
    }

    #[cfg(unix)]
    fn shell_fixture() -> (
        PathBuf,
        Agent,
        Arc<JobRuntime>,
        ToolContext,
        Arc<shell::NativeShell>,
    ) {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let suffix = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let root =
            std::env::temp_dir().join(format!("aw-job-shell-tool-{}-{suffix}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let bash = std::env::var_os("AW_BASH")
            .map(PathBuf::from)
            .or_else(|| {
                ["/bin/bash", "/usr/bin/bash"]
                    .iter()
                    .map(PathBuf::from)
                    .find(|path| path.is_file())
            })
            .unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        );
        let mut tools = LocalToolOptions::new(&root, ToolMode::Trusted);
        tools.bash = Some(bash.clone());
        let agent = configure(builder, actor.clone(), &tools)
            .unwrap()
            .build()
            .unwrap();
        let runtime = JobRuntime::new(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            Limits::default(),
        )
        .unwrap();
        let shell = Arc::new(shell::NativeShell::new(root.clone(), bash).unwrap());
        runtime.register_shell(shell.clone()).unwrap();
        let ctx = ToolContext {
            actor: actor.clone(),
            turn_id: 1,
            call_id: "shell-call".into(),
            source: ToolSource::trusted_host(),
            effect: ToolEffect::Process,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: agent.default_identity().unwrap(),
            scope: None,
        };
        (root, agent, runtime, ctx, shell)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn direct_job_shell_preserves_fast_aw_result_shape_and_retires_hidden_job() {
        let (root, _agent, runtime, ctx, shell) = shell_fixture();
        let tool = JobShell::new(runtime.clone(), shell);
        let output = tool
            .execute(
                ctx.clone(),
                &json!({"command":"printf direct-out; printf direct-err >&2"}),
            )
            .await
            .unwrap();
        assert!(!output.is_error);
        assert!(output
            .content
            .starts_with("exit: 0\nstdout:\ndirect-out\nstderr:\ndirect-err"));
        assert!(runtime
            .host_for(&ctx)
            .unwrap()
            .manager()
            .list(0, 64)
            .unwrap()
            .jobs
            .is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn direct_job_shell_explicit_cancel_exposes_and_cancels_same_job() {
        let (root, _agent, runtime, ctx, shell) = shell_fixture();
        let tool = JobShell::new(runtime.clone(), shell);
        let args = json!({"command":"while :; do :; done","timeout_secs":10});
        let cancel = ctx.cancellation.clone();
        let (result, ()) = tokio::join!(tool.execute(ctx.clone(), &args), async move {
            tokio::time::sleep(std::time::Duration::from_millis(15)).await;
            cancel.cancel();
        });
        assert!(matches!(result, Err(Error::Cancelled)));
        let list = JobControl::new(runtime.clone(), JobToolKind::List)
            .execute(ctx.clone(), &json!({}))
            .await
            .unwrap();
        let id = list
            .content
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().next())
            .expect("cancelled direct shell must expose its Job")
            .to_owned();
        let get = JobControl::new(runtime.clone(), JobToolKind::Get);
        assert_eq!(
            wait_terminal(&get, ctx.clone(), &id).await,
            "cancelled bash exit=cancelled"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn direct_job_shell_dropped_response_hands_off_without_cancelling_process() {
        let (root, _agent, runtime, ctx, shell) = shell_fixture();
        let tool = JobShell::new(runtime.clone(), shell);
        let args = json!({"command":"sleep 0.08; printf done"});
        let timed = tokio::time::timeout(
            std::time::Duration::from_millis(10),
            tool.execute(ctx.clone(), &args),
        )
        .await;
        assert!(
            timed.is_err(),
            "fixture must abandon the tool future before process exit"
        );
        let list = JobControl::new(runtime.clone(), JobToolKind::List)
            .execute(ctx.clone(), &json!({}))
            .await
            .unwrap();
        let id = list
            .content
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().next())
            .expect("abandoned direct shell must expose its Job")
            .to_owned();
        let get = JobControl::new(runtime.clone(), JobToolKind::Get);
        assert_eq!(
            wait_terminal(&get, ctx.clone(), &id).await,
            "ok bash exit=0"
        );
        let stdout = JobControl::new(runtime.clone(), JobToolKind::Output)
            .execute(ctx.clone(), &json!({"id":id}))
            .await
            .unwrap();
        assert_eq!(stdout.content, "done");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn job_start_and_restart_accept_shellnodes_with_stdout_default() {
        let (root, _agent, runtime, mut ctx, _shell) = shell_fixture();
        ctx.call_id = "shell-start".into();
        let start = JobControl::new(runtime.clone(), JobToolKind::Start);
        let get = JobControl::new(runtime.clone(), JobToolKind::Get);
        let output = JobControl::new(runtime.clone(), JobToolKind::Output);
        let ack = JobControl::new(runtime.clone(), JobToolKind::Ack);
        let restart = JobControl::new(runtime.clone(), JobToolKind::Restart);
        let started = start
            .execute(
                ctx.clone(),
                &json!({"ops":[["bash",{"command":"printf shell-start"}]]}),
            )
            .await
            .unwrap();
        let id = started
            .content
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_owned();
        assert_eq!(
            wait_terminal(&get, ctx.clone(), &id).await,
            "ok bash exit=0"
        );
        assert_eq!(
            output
                .execute(ctx.clone(), &json!({"id":id}))
                .await
                .unwrap()
                .content,
            "shell-start"
        );
        let result = output
            .execute(ctx.clone(), &json!({"id":id,"stream":"result"}))
            .await
            .unwrap();
        assert!(result.content.starts_with("ok exit=0"));
        let acknowledged = ack
            .execute(
                ctx.clone(),
                &json!({"id":id,"stream":"result","offset":result.content.len()}),
            )
            .await
            .unwrap();
        assert_eq!(acknowledged.content, "ok");
        assert!(output
            .execute(ctx.clone(), &json!({"id":id,"stream":"result"}))
            .await
            .is_err());
        assert_eq!(
            wait_terminal(&get, ctx.clone(), &id).await,
            "ok bash exit=0",
            "process terminal metadata must survive result payload ACK"
        );

        ctx.call_id = "shell-restart".into();
        ctx.effect = ToolEffect::Write;
        let restarted = restart
            .execute(ctx.clone(), &json!({"id":id}))
            .await
            .unwrap();
        let restarted_id = restarted
            .content
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_owned();
        assert_ne!(restarted_id, id);
        assert_eq!(
            wait_terminal(&get, ctx.clone(), &restarted_id).await,
            "ok bash exit=0"
        );
        assert_eq!(
            output
                .execute(ctx, &json!({"id":restarted_id}))
                .await
                .unwrap()
                .content,
            "shell-start"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn direct_job_shell_hands_off_when_fast_result_exceeds_host_result_ceiling() {
        let (root, agent, _runtime, ctx, shell) = shell_fixture();
        let tool_limits = crate::LocalToolLimits {
            result_bytes: 8 * 1024,
            ..Default::default()
        };
        let tool_limits = tool_limits.validate().unwrap();
        let runtime = JobRuntime::new_with_native_tools(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            tool_limits,
            crate::LocalToolConcurrency::default(),
            aw_tools::filesystem::schema::Tool::GRAPH_NATIVE.to_vec(),
        )
        .unwrap();
        runtime.register_shell(shell.clone()).unwrap();
        let tool = JobShell::new(runtime.clone(), shell);
        let output = tool
            .execute(
                ctx.clone(),
                &json!({"command":"head -c 6000 /dev/zero | tr '\\0' x; head -c 6000 /dev/zero | tr '\\0' y >&2"}),
            )
            .await
            .unwrap();
        assert!(output.content.starts_with("ok j"), "{}", output.content);
        assert!(output.content.ends_with("result=job_output"));
        assert!(output.content.len() <= tool_limits.result_bytes);
        let id = output.content.split_whitespace().nth(1).unwrap().to_owned();
        let stdout = JobControl::new(runtime.clone(), JobToolKind::Output)
            .execute(ctx.clone(), &json!({"id":id,"stream":"stdout"}))
            .await
            .unwrap();
        assert_eq!(stdout.content.len(), 6000);
        let stderr = JobControl::new(runtime, JobToolKind::Output)
            .execute(ctx, &json!({"id":id,"stream":"stderr"}))
            .await
            .unwrap();
        assert_eq!(stderr.content.len(), 6000);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn direct_job_shell_preserves_nonzero_tool_error_compatibility() {
        let (root, _agent, runtime, ctx, shell) = shell_fixture();
        let tool = JobShell::new(runtime, shell);
        let output = tool
            .execute(ctx, &json!({"command":"printf nope; exit 7"}))
            .await
            .unwrap();
        assert!(output.is_error);
        assert!(output.content.starts_with("exit: 7\nstdout:\nnope"));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn batch_returns_exact_inline_graph_and_retires_hidden_job() {
        let (root, _agent, runtime, ctx) = fixture();
        let tool = Batch::new(runtime.clone());
        let output = tool
            .execute(
                ctx.clone(),
                &json!({"ops":[["read",{"path":"a.txt","numbered":false}]]}),
            )
            .await
            .unwrap();
        assert_eq!(output.content, "1\na\n\n\nok");
        assert!(!output.is_error);
        assert_eq!(
            runtime
                .host_for(&ctx)
                .unwrap()
                .manager()
                .list(0, 64)
                .unwrap()
                .jobs
                .len(),
            0
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn configured_batch_result_ceiling_hands_off_instead_of_using_global_limit() {
        let (root, agent, _runtime, ctx) = fixture();
        fs::write(root.join("a.txt"), "A".repeat(3_000)).unwrap();
        fs::write(root.join("b.txt"), "B".repeat(3_000)).unwrap();
        fs::write(root.join("c.txt"), "C".repeat(3_000)).unwrap();
        let limits = crate::LocalToolLimits {
            result_bytes: 8 * 1024,
            ..Default::default()
        };
        let runtime = JobRuntime::new_with_native_tools(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            limits,
            crate::LocalToolConcurrency::default(),
            aw_tools::filesystem::schema::Tool::GRAPH_NATIVE.to_vec(),
        )
        .unwrap();
        let batch = Batch::new(runtime.clone());
        let receipt = batch
            .execute(
                ctx.clone(),
                &json!({"ops":[
                    ["read",{"path":"a.txt","numbered":false},"*"],
                    ["read",{"path":"b.txt","numbered":false},"*"],
                    ["read",{"path":"c.txt","numbered":false},"*"]
                ]}),
            )
            .await
            .unwrap();
        assert!(
            receipt.content.ends_with("result=job_output"),
            "{}",
            receipt.content
        );
        let id = receipt.content.split_whitespace().nth(1).unwrap();
        assert!(runtime.host_for(&ctx).unwrap().manager().get(id).is_ok());
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn oversized_completed_batch_hands_off_same_terminal_job_for_paging() {
        let (root, _agent, runtime, mut ctx) = fixture();
        for (name, byte) in [
            ("a.txt", 'A'),
            ("b.txt", 'B'),
            ("c.txt", 'C'),
            ("d.txt", 'D'),
            ("e.txt", 'E'),
        ] {
            let mut content = byte.to_string().repeat(30_000);
            content.push('\n');
            fs::write(root.join(name), content).unwrap();
        }
        ctx.call_id = "large-batch".into();
        let batch_tool = Batch::new(runtime.clone());
        let receipt = batch_tool
            .execute(
                ctx.clone(),
                &json!({"ops":[
                    ["read",{"path":"a.txt","numbered":false},"*"],
                    ["read",{"path":"b.txt","numbered":false},"*"],
                    ["read",{"path":"c.txt","numbered":false},"*"],
                    ["read",{"path":"d.txt","numbered":false},"*"],
                    ["read",{"path":"e.txt","numbered":false},"*"]
                ]}),
            )
            .await
            .unwrap();
        assert!(receipt.content.starts_with("ok j"), "{}", receipt.content);
        assert!(receipt.content.ends_with("result=job_output"));
        let id = receipt
            .content
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_owned();
        let host = runtime.host_for(&ctx).unwrap();
        let status = host.manager().get(&id).unwrap();
        assert_eq!(status.summary, Some((Primary::Ok, false, false)));
        let output = JobControl::new(runtime.clone(), JobToolKind::Output)
            .execute(
                ctx,
                &json!({"id":id,"node":1,"max_output_bytes":aw_tools::task_ops::MAX_TASK_OUTPUT_PAGE,"encoding":"text"}),
            )
            .await
            .unwrap();
        assert_eq!(output.content.len(), 30_001);
        assert!(output.content.starts_with(&"A".repeat(100)));
        assert!(output.content.ends_with('\n'));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn live_invocation_budget_hands_off_graph_and_pages_retained_full_result() {
        let (root, _agent, runtime, mut ctx) = fixture();
        let expected = "A".repeat(3000);
        fs::write(root.join("large.txt"), &expected).unwrap();
        ctx.call_id = "tight-live-batch".into();
        ctx.result_budget_bytes = 512;
        let batch = Batch::new(runtime.clone());
        let receipt = batch
            .execute(
                ctx.clone(),
                &json!({"ops":[["read",{"path":"large.txt","numbered":false}]]}),
            )
            .await
            .unwrap();
        assert!(receipt.content.len() <= ctx.result_budget_bytes);
        assert!(
            receipt.content.ends_with("result=job_output"),
            "{}",
            receipt.content
        );
        let id = receipt
            .content
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_owned();
        let host = runtime.host_for(&ctx).unwrap();
        let retained = host
            .manager()
            .output_page(&id, 1, OutputStream::Result, 0, 4096)
            .unwrap();
        assert_eq!(retained.bytes, expected.as_bytes());

        let output = JobControl::new(runtime.clone(), JobToolKind::Output);
        let mut offset = 0u64;
        let mut reconstructed = String::new();
        loop {
            let page = output
                .execute(
                    ctx.clone(),
                    &json!({
                        "id":id,"node":1,"offset":offset,
                        "max_output_bytes":4096,"encoding":"text"
                    }),
                )
                .await
                .unwrap();
            assert!(page.content.len() <= ctx.result_budget_bytes);
            let marker = page
                .content
                .lines()
                .last()
                .and_then(|line| line.strip_prefix("more offset="))
                .and_then(|value| value.parse::<u64>().ok());
            match marker {
                Some(next) => {
                    assert!(next > offset);
                    let suffix = format!("\nmore offset={next}");
                    let data = page.content.strip_suffix(&suffix).unwrap_or(&page.content);
                    reconstructed.push_str(data);
                    offset = next;
                }
                None => {
                    reconstructed.push_str(&page.content);
                    break;
                }
            }
        }
        assert_eq!(reconstructed, expected);
        let _ = fs::remove_dir_all(root);
    }

    async fn wait_terminal(tool: &JobControl, mut ctx: ToolContext, id: &str) -> String {
        ctx.call_id = format!("get-{id}");
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let output = tool.execute(ctx.clone(), &json!({"id":id})).await.unwrap();
                if !output.content.starts_with("running ")
                    && !output.content.starts_with("cancelling ")
                    && !output.content.starts_with("revoking ")
                {
                    return output.content;
                }
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("Job did not settle before the bounded test deadline")
    }

    #[tokio::test]
    async fn memory_job_start_duplicate_delivery_returns_same_job_id() {
        let (root, _agent, runtime, mut ctx) = fixture();
        ctx.call_id = "same-start".into();
        let start = JobControl::new(runtime.clone(), JobToolKind::Start);
        let args = json!({"ops":[["read",{"path":"a.txt","numbered":false}]]});
        let first = start.execute(ctx.clone(), &args).await.unwrap();
        let second = start.execute(ctx.clone(), &args).await.unwrap();
        let first_id = first.content.split_whitespace().nth(1).unwrap();
        let second_id = second.content.split_whitespace().nth(1).unwrap();
        assert_eq!(first_id, second_id);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn memory_batch_duplicate_after_inline_retirement_is_unknown_not_reexecuted() {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let suffix = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let root =
            std::env::temp_dir().join(format!("aw-batch-replay-{}-{suffix}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.txt"), "old").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        );
        let tools = LocalToolOptions::new(&root, ToolMode::Workspace);
        let agent = configure(builder, actor.clone(), &tools)
            .unwrap()
            .build()
            .unwrap();
        let runtime = JobRuntime::new(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            Limits::default(),
        )
        .unwrap();
        let ctx = ToolContext {
            actor: actor.clone(),
            turn_id: 1,
            call_id: "same-batch".into(),
            source: ToolSource::trusted_host(),
            effect: ToolEffect::Unknown,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: agent.default_identity().unwrap(),
            scope: None,
        };
        let batch = Batch::new(runtime);
        let args = json!({"ops":[["edit",{
            "path":"a.txt",
            "edits":[["old","new"]]
        }]]});
        let first = batch.execute(ctx.clone(), &args).await.unwrap();
        assert!(!first.is_error, "{}", first.content);
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "new");
        let second = batch.execute(ctx, &args).await.unwrap();
        assert!(second.is_error);
        assert!(second.content.starts_with("unknown batch Job admission"));
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "new");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn job_controls_cover_start_list_get_output_ack_cancel_and_restart() {
        let (root, _agent, runtime, mut ctx) = fixture();
        ctx.call_id = "start-call".into();
        let start = JobControl::new(runtime.clone(), JobToolKind::Start);
        let get = JobControl::new(runtime.clone(), JobToolKind::Get);
        let list = JobControl::new(runtime.clone(), JobToolKind::List);
        let output = JobControl::new(runtime.clone(), JobToolKind::Output);
        let ack = JobControl::new(runtime.clone(), JobToolKind::Ack);
        let cancel = JobControl::new(runtime.clone(), JobToolKind::Cancel);
        let restart = JobControl::new(runtime.clone(), JobToolKind::Restart);

        let started = start
            .execute(
                ctx.clone(),
                &json!({"ops":[["read",{"path":"a.txt","numbered":false}]]}),
            )
            .await
            .unwrap();
        assert!(started.content.starts_with("running j"));
        let id = started
            .content
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_owned();

        let terminal = wait_terminal(&get, ctx.clone(), &id).await;
        assert_eq!(terminal, "ok read");
        let listed = list.execute(ctx.clone(), &json!({})).await.unwrap();
        assert!(listed
            .content
            .lines()
            .any(|line| line == format!("{id} read ok")));

        let page = output
            .execute(
                ctx.clone(),
                &json!({"id":id,"encoding":"text","max_output_bytes":64}),
            )
            .await
            .unwrap();
        assert_eq!(page.content, "a\n");
        let encoded = output
            .execute(
                ctx.clone(),
                &json!({"id":id,"encoding":"base64","max_output_bytes":64}),
            )
            .await
            .unwrap();
        assert_eq!(encoded.content, "YQo=");
        let acked = ack
            .execute(ctx.clone(), &json!({"id":id,"offset":2}))
            .await
            .unwrap();
        assert_eq!(acked.content, "ok");
        assert_eq!(
            runtime
                .host_for(&ctx)
                .unwrap()
                .manager()
                .spool_usage()
                .unwrap()
                .0,
            0
        );

        let unchanged = cancel
            .execute(ctx.clone(), &json!({"id":id}))
            .await
            .unwrap();
        assert_eq!(unchanged.content, "unchanged ok");

        let mut restart_ctx = ctx.clone();
        restart_ctx.call_id = "restart-call".into();
        restart_ctx.effect = ToolEffect::Write;
        let restarted = restart
            .execute(restart_ctx.clone(), &json!({"id":id}))
            .await
            .unwrap();
        let restarted_id = restarted
            .content
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_owned();
        assert_ne!(restarted_id, id);
        assert_eq!(
            wait_terminal(&get, restart_ctx.clone(), &restarted_id).await,
            "ok read"
        );
        // Duplicate delivery of the same restart call resolves to the same new Job.
        let duplicate = restart
            .execute(restart_ctx, &json!({"id":id}))
            .await
            .unwrap();
        assert_eq!(duplicate.content, format!("running {restarted_id}"));
        assert_eq!(wait_terminal(&get, ctx.clone(), &id).await, "ok read");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn multi_node_output_requires_explicit_node_and_list_pages() {
        let (root, _agent, runtime, mut ctx) = fixture();
        fs::write(root.join("b.txt"), "b\n").unwrap();
        ctx.call_id = "multi-start".into();
        let start = JobControl::new(runtime.clone(), JobToolKind::Start);
        let get = JobControl::new(runtime.clone(), JobToolKind::Get);
        let output = JobControl::new(runtime.clone(), JobToolKind::Output);
        let list = JobControl::new(runtime.clone(), JobToolKind::List);
        let started = start
            .execute(
                ctx.clone(),
                &json!({"ops":[
                    ["read",{"path":"a.txt","numbered":false},"*"],
                    ["read",{"path":"b.txt","numbered":false},"*"]
                ]}),
            )
            .await
            .unwrap();
        let id = started
            .content
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_owned();
        let status = wait_terminal(&get, ctx.clone(), &id).await;
        assert!(status.starts_with("ok graph 2/2"));
        assert!(output
            .execute(ctx.clone(), &json!({"id":id}))
            .await
            .is_err());
        assert_eq!(
            output
                .execute(
                    ctx.clone(),
                    &json!({"id":id,"node":2,"max_output_bytes":64})
                )
                .await
                .unwrap()
                .content,
            "b\n"
        );
        let page = list
            .execute(ctx, &json!({"offset":0,"limit":1}))
            .await
            .unwrap();
        assert!(page.content.contains("more offset=1") || page.content.lines().count() == 1);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn revoked_then_regranted_tool_keeps_status_but_denies_old_payload() {
        let (root, mut agent, runtime, mut ctx) = fixture();
        ctx.call_id = "revocation-start".into();
        let start = JobControl::new(runtime.clone(), JobToolKind::Start);
        let get = JobControl::new(runtime.clone(), JobToolKind::Get);
        let output = JobControl::new(runtime.clone(), JobToolKind::Output);
        let started = start
            .execute(
                ctx.clone(),
                &json!({"ops":[["read",{"path":"a.txt","numbered":false}]]}),
            )
            .await
            .unwrap();
        let id = started
            .content
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_owned();
        assert_eq!(wait_terminal(&get, ctx.clone(), &id).await, "ok read");
        assert_eq!(
            output
                .execute(ctx.clone(), &json!({"id":id,"max_output_bytes":64}))
                .await
                .unwrap()
                .content,
            "a\n"
        );

        let actor = ctx.actor.clone();
        agent
            .revoke_registered_source_tool(&actor, "read", &ToolSource::trusted_host())
            .unwrap();
        assert_eq!(wait_terminal(&get, ctx.clone(), &id).await, "ok read");
        assert!(output
            .execute(ctx.clone(), &json!({"id":id,"max_output_bytes":64}))
            .await
            .is_err());
        agent.permit_registered_host_tool(actor, "read").unwrap();
        // Regrant is a new exact grant generation; old retained bytes stay closed.
        assert!(output
            .execute(ctx, &json!({"id":id,"max_output_bytes":64}))
            .await
            .is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn shell_node_output_infers_stdout_and_accepts_explicit_streams() {
        use aw_tools::filesystem::{
            batch::{Graph, NodeTool},
            footprint::Footprint,
            job_manager::{
                plan_graph, JobManager, Limits, NodePolicy, RuntimeAdmission, SemanticBudget,
            },
        };
        let graph = Graph::compile_with(
            &json!({"ops":[["bash",{"command":"true"}]]}),
            |name| (name == "bash").then_some(NodeTool::Bash),
            |_, args| args.is_object(),
        )
        .unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 1024,
                },
                footprint: Footprint::unknown(true, true),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        let manager = JobManager::new(Limits::default()).unwrap();
        manager
            .admit_runtime(
                graph,
                plans,
                RuntimeAdmission {
                    id: "shell",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        assert_eq!(
            JobControl::stream(&manager, "shell", 1, &json!({})).unwrap(),
            OutputStream::Stdout
        );
        assert_eq!(
            JobControl::stream(&manager, "shell", 1, &json!({"stream":"stderr"})).unwrap(),
            OutputStream::Stderr
        );
        assert_eq!(
            JobControl::stream(&manager, "shell", 1, &json!({"stream":"result"})).unwrap(),
            OutputStream::Result
        );
    }

    #[tokio::test]
    async fn job_list_schema_and_default_narrow_to_runtime_retention_limit() {
        let (root, agent, _runtime, ctx) = fixture();
        let mut tool_limits = crate::LocalToolLimits::default();
        tool_limits.jobs.max_retained_jobs = 4;
        tool_limits.jobs.max_active_nodes = tool_limits.jobs.max_active_nodes.min(4);
        let runtime = JobRuntime::new_with_native_tools(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            tool_limits,
            crate::LocalToolConcurrency {
                graph_width: 4,
                ..Default::default()
            },
            aw_tools::filesystem::schema::Tool::GRAPH_NATIVE.to_vec(),
        )
        .unwrap();
        let tool = JobControl::new(runtime, JobToolKind::List);
        let spec = tool.spec();
        assert_eq!(spec.parameters["properties"]["limit"]["maximum"], 4);
        assert_eq!(spec.parameters["properties"]["limit"]["default"], 4);
        tool.validate(&json!({})).unwrap();
        let output = tool.execute(ctx, &json!({})).await.unwrap();
        assert!(output.content.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn batch_and_control_schemas_remain_small_and_strict() {
        let (_root, _agent, runtime, _ctx) = fixture();
        let spec = Batch::new(runtime).spec();
        assert_eq!(spec.name, "batch");
        assert_eq!(spec.parameters, batch::parameters());
        assert!(serde_json::to_vec(&spec).unwrap().len() < 2048);
        assert_eq!(job_list_parameters()["properties"]["limit"]["default"], 64);
        assert_eq!(job_id_parameters()["required"][0], "id");
    }
}
