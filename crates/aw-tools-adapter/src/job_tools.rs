//! Model-facing projections over the one live `JobRuntime`.
//! `batch`, direct shells and `job_start` all construct the same bounded Graph;
//! execution always goes through `JobHost`, never recursive public-tool dispatch.
use crate::{
    job_runtime::{JobRuntime, StartDecision},
    shell, Error, Result,
};
use aw_agent_core::{Tool, ToolContext, ToolEffect, ToolFuture, ToolOutput, ToolSpec};
use aw_tools::{
    filesystem::{
        batch::{self},
        job::Lifecycle,
        job_manager::{FiniteJobKind, OutputStream, MAX_JOB_LIST_PAGE},
        status::Primary,
    },
    job_event::EventRuleSet,
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
    fn retire_inline(&mut self) -> Result<()> {
        self.host.discard_unexposed_terminal(&self.id)?;
        self.disarm();
        Ok(())
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

fn handoff_receipt(
    host: &crate::job_host::JobHost,
    id: &str,
    snapshot: &aw_tools::filesystem::job_manager::Snapshot,
    budget: usize,
) -> Result<ToolOutput> {
    let reference = host.job_ref(id, None)?;
    let content = aw_tools::receipt::render_handoff(
        &reference,
        snapshot,
        if snapshot.lifecycle == aw_tools::filesystem::job::Lifecycle::Terminal
            && snapshot.total == 1
        {
            host.manager()
                .process_terminal(id, 1)
                .map_err(|_| Error::Protocol("Job Process terminal facts unavailable"))?
        } else {
            None
        },
        budget.min(aw_tools::filesystem::job_manager::MAX_COMPACT_JOB_STATUS_BYTES),
    )
    .map_err(|_| Error::Protocol("cannot render typed compact handoff receipt"))?;
    Ok(ToolOutput {
        content,
        is_error: snapshot
            .summary
            .is_some_and(|summary| graph_error(summary.0)),
        truncated: false,
    })
}

fn background(arguments: &Value) -> Result<bool> {
    match arguments.get("background") {
        None => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        _ => Err(Error::Denied("background must be a boolean")),
    }
}

fn constructor_event_rules_schema() -> Value {
    json!({
        "type":"array",
        "maxItems":aw_tools::job_event::MAX_EVENT_RULES,
        "items":{
            "type":"array",
            "minItems":1,
            "maxItems":2,
            "prefixItems":[
                {"type":"string","enum":["result","settled"]},
                {"type":"string","enum":["auto","queue"]}
            ]
        }
    })
}

fn constructor_wake_rules(arguments: &Value, runtime: &JobRuntime) -> Result<Option<EventRuleSet>> {
    let Some(value) = arguments.get("wake") else {
        return Ok(None);
    };
    if !runtime.wake_capable() {
        return Err(Error::Denied(
            "constructor wake is unavailable for this Agent/host",
        ));
    }
    let rules = crate::jobs_tools::parse_event_rules(value)?;
    for rule in rules.as_slice() {
        if !matches!(
            (rule.event, rule.mode),
            (
                aw_tools::job_event::JobEventKind::Result
                    | aw_tools::job_event::JobEventKind::Settled,
                aw_tools::job_event::JobWakeMode::Auto | aw_tools::job_event::JobWakeMode::Queue
            )
        ) {
            return Err(Error::Denied(
                "finite constructor wake supports only result/settled with auto/queue",
            ));
        }
    }
    Ok(Some(rules))
}

fn semantic_constructor_arguments(arguments: &Value) -> Result<Value> {
    let mut semantic = arguments.clone();
    let object = semantic
        .as_object_mut()
        .ok_or(Error::Denied("constructor arguments must be an object"))?;
    object.remove("background");
    object.remove("wake");
    Ok(semantic)
}

fn batch_parameters_with_background(
    tools: Vec<aw_tools::filesystem::batch::NodeTool>,
    wake_capable: bool,
) -> Value {
    let mut parameters = batch::parameters_for(tools);
    if let Some(properties) = parameters
        .get_mut("properties")
        .and_then(Value::as_object_mut)
    {
        properties.insert(
            "background".into(),
            json!({"type":"boolean","default":false}),
        );
        if wake_capable {
            properties.insert("wake".into(), constructor_event_rules_schema());
        }
    }
    parameters
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
        let mut spec = self.shell.spec();
        if let Some(properties) = spec
            .parameters
            .get_mut("properties")
            .and_then(Value::as_object_mut)
        {
            properties.insert(
                "background".into(),
                json!({"type":"boolean","default":false}),
            );
            if self.runtime.wake_capable() {
                properties.insert("wake".into(), constructor_event_rules_schema());
            }
        }
        spec.description.push_str(
            " Set background=true to return the same Process Job immediately; interactive=true requires background=true.",
        );
        spec
    }

    fn validate(&self, arguments: &Value) -> Result<()> {
        let run_background = background(arguments)?;
        let _ = constructor_wake_rules(arguments, &self.runtime)?;
        let semantic = semantic_constructor_arguments(arguments)?;
        self.shell.validate(&semantic)?;
        if semantic
            .get("interactive")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && !run_background
        {
            return Err(Error::Denied(
                "interactive shell requires background=true so jobs_send can control stdin",
            ));
        }
        Ok(())
    }

    fn can_run_parallel(&self, first: &Value, second: &Value) -> bool {
        self.shell.can_run_parallel(first, second)
    }

    fn execute<'a>(&'a self, ctx: ToolContext, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(arguments)?;
            let run_background = background(arguments)?;
            let wake = match constructor_wake_rules(arguments, &self.runtime)? {
                Some(rules) => self.runtime.prepare_initial_wake(&ctx, rules)?,
                None => None,
            };
            let semantic = semantic_constructor_arguments(arguments)?;
            let direct_budget = ctx.result_budget(self.runtime.result_limit())?;
            let graph = json!({"ops":[[self.shell.spec().name, semantic]]});
            let decision = match wake {
                Some(wake) => self.runtime.start_replay_with_initial_wake(
                    &ctx,
                    &graph,
                    run_background,
                    wake,
                )?,
                None => self.runtime.start_replay(&ctx, &graph, run_background)?,
            };
            let mut started = match decision {
                StartDecision::New(started) => {
                    started
                        .host
                        .manager()
                        .set_finite_kind(&started.id, FiniteJobKind::Process)
                        .map_err(|_| Error::Protocol("cannot bind Process Job projection"))?;
                    started
                }
                StartDecision::Existing { id, host } => {
                    host.manager()
                        .set_finite_kind(&id, FiniteJobKind::Process)
                        .map_err(|_| Error::Denied("replayed Job kind changed"))?;
                    host.expose(&id)?;
                    let snapshot = host
                        .manager()
                        .runtime_snapshot(&id)
                        .map_err(|_| Error::Protocol("replayed shell Job is unavailable"))?;
                    return handoff_receipt(&host, &id, &snapshot, direct_budget);
                }
                StartDecision::Unknown => {
                    return Ok(ToolOutput::error(
                        "unknown shell Job admission; do not retry automatically",
                    ));
                }
            };
            let mut handoff = HandoffGuard::new(started.host.clone(), started.id.clone());
            if run_background {
                handoff.expose()?;
                let snapshot = started
                    .host
                    .manager()
                    .runtime_snapshot(&started.id)
                    .map_err(|_| Error::Protocol("new Process Job status unavailable"))?;
                drop(started.task);
                return handoff_receipt(&started.host, &started.id, &snapshot, direct_budget);
            }
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
            if snapshot.journal_unknown {
                handoff.expose()?;
                return handoff_receipt(&started.host, &started.id, &snapshot, direct_budget);
            }
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
            let content = if primary == Primary::Fail
                && started
                    .host
                    .manager()
                    .process_terminal(&started.id, 1)
                    .map_err(|_| Error::Protocol("shell Process terminal facts unavailable"))?
                    .is_none()
            {
                // A known no-launch failure has no exit status. Keep its
                // actual retained diagnostic instead of projecting an exit.
                result.clone()
            } else {
                format!("exit: {exit}\nstdout:\n{stdout}\nstderr:\n{stderr}")
            };
            let direct = ToolOutput {
                content,
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
                    let output =
                        handoff_receipt(&started.host, &started.id, &snapshot, direct_budget)?;
                    output.validate()?;
                    Ok(output)
                }
                _ => {
                    direct.validate()?;
                    handoff.retire_inline()?;
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
            description: "Run a bounded Graph with compact [tool,args,when?] nodes. Eligible native tools and prepared shell interpreters share one Job; node authority is rechecked individually. Results are coordinate-keyed. Set background=true to return the same Graph Job immediately.".into(),
            parameters: batch_parameters_with_background(
                self.runtime.node_tools(),
                self.runtime.wake_capable(),
            ),
            // Construction/lifecycle is Control; every node independently keeps
            // its original semantic effect and exact current authority.
            effect: ToolEffect::Control,
        }
    }

    fn validate(&self, arguments: &Value) -> Result<()> {
        let _ = background(arguments)?;
        let _ = constructor_wake_rules(arguments, &self.runtime)?;
        let semantic = semantic_constructor_arguments(arguments)?;
        self.runtime.validate_graph(&semantic)
    }

    fn execute<'a>(&'a self, ctx: ToolContext, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            if ctx.execution_scope.is_some() {
                return Err(Error::Denied(
                    "batch cannot start while this work is owned by an active Graph node",
                ));
            }
            self.validate(arguments)?;
            let run_background = background(arguments)?;
            let wake = match constructor_wake_rules(arguments, &self.runtime)? {
                Some(rules) => self.runtime.prepare_initial_wake(&ctx, rules)?,
                None => None,
            };
            let semantic = semantic_constructor_arguments(arguments)?;
            let direct_budget = ctx.result_budget(self.runtime.result_limit())?;
            let decision = match wake {
                Some(wake) => {
                    self.runtime
                        .start_replay_presealed_with_initial_wake(
                            &ctx,
                            &semantic,
                            run_background,
                            Some(wake),
                        )
                        .await?
                }
                None => {
                    self.runtime
                        .start_replay_presealed(&ctx, &semantic, run_background)
                        .await?
                }
            };
            let mut started = match decision {
                StartDecision::New(started) => {
                    started
                        .host
                        .manager()
                        .set_finite_kind(&started.id, FiniteJobKind::Graph)
                        .map_err(|_| Error::Protocol("cannot bind Graph Job projection"))?;
                    started
                }
                StartDecision::Existing { id, host } => {
                    host.manager()
                        .set_finite_kind(&id, FiniteJobKind::Graph)
                        .map_err(|_| Error::Denied("replayed Job kind changed"))?;
                    host.expose(&id)?;
                    let snapshot = host
                        .manager()
                        .runtime_snapshot(&id)
                        .map_err(|_| Error::Protocol("replayed batch Job is unavailable"))?;
                    return handoff_receipt(&host, &id, &snapshot, direct_budget);
                }
                StartDecision::Unknown => {
                    return Ok(ToolOutput::error(
                        "unknown batch Job admission; do not retry automatically",
                    ));
                }
            };
            let mut handoff = HandoffGuard::new(started.host.clone(), started.id.clone());
            if run_background {
                handoff.expose()?;
                let snapshot = started
                    .host
                    .manager()
                    .runtime_snapshot(&started.id)
                    .map_err(|_| Error::Protocol("new Graph Job status unavailable"))?;
                drop(started.task);
                return handoff_receipt(&started.host, &started.id, &snapshot, direct_budget);
            }
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
            if snapshot.journal_unknown {
                handoff.expose()?;
                return handoff_receipt(&started.host, &started.id, &snapshot, direct_budget);
            }
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
                handoff.retire_inline()?;
                return Ok(output);
            }
            handoff.expose()?;
            let output = handoff_receipt(&started.host, &started.id, &snapshot, direct_budget)?;
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

    fn decimal_u64_len(mut value: u64) -> usize {
        let mut digits = 1;
        while value >= 10 {
            value /= 10;
            digits += 1;
        }
        digits
    }

    /// Largest raw-byte prefix whose UTF-8-lossy projection plus continuation
    /// metadata fits `budget`. Only stable lossy token boundaries are
    /// considered: a trailing partial UTF-8 sequence is never used as a size
    /// oracle because completing it can make the rendered string *smaller*.
    fn text_prefix_that_fits(
        page: &aw_tools::filesystem::job_manager::OutputPage,
        offset: u64,
        budget: usize,
    ) -> Result<usize> {
        let rendered_len = |kept: usize, content_len: usize, ends_newline: bool| {
            let next = offset
                .checked_add(kept as u64)
                .ok_or(Error::Limit("Job output continuation offset overflow"))?;
            let has_more = next < page.retained_bytes;
            let metadata_count = usize::from(has_more) + usize::from(page.truncated);
            let mut total = content_len;
            if metadata_count != 0 {
                if content_len != 0 && !ends_newline {
                    total = total.saturating_add(1);
                }
                if has_more {
                    total = total
                        .saturating_add("more offset=".len())
                        .saturating_add(Self::decimal_u64_len(next));
                }
                if page.truncated {
                    total = total
                        .saturating_add("truncated retained=".len())
                        .saturating_add(Self::decimal_u64_len(page.retained_bytes))
                        .saturating_add(" total=".len())
                        .saturating_add(Self::decimal_u64_len(page.total_bytes));
                }
                if metadata_count > 1 {
                    total = total.saturating_add(metadata_count - 1);
                }
            }
            Ok::<_, Error>(total)
        };

        let mut best = None;
        if rendered_len(0, 0, false)? <= budget {
            best = Some(0);
        }
        let bytes = page.bytes.as_slice();
        let mut raw = 0usize;
        let mut projected = 0usize;
        while raw < bytes.len() {
            let remaining = &bytes[raw..];
            let error = match std::str::from_utf8(remaining) {
                Ok(valid) => {
                    for ch in valid.chars() {
                        raw += ch.len_utf8();
                        projected += ch.len_utf8();
                        if rendered_len(raw, projected, ch == '\n')? <= budget {
                            best = Some(raw);
                        }
                    }
                    break;
                }
                Err(error) => error,
            };
            let valid_len = error.valid_up_to();
            if valid_len != 0 {
                let valid =
                    std::str::from_utf8(&remaining[..valid_len]).expect("validated UTF-8 prefix");
                for ch in valid.chars() {
                    raw += ch.len_utf8();
                    projected += ch.len_utf8();
                    if rendered_len(raw, projected, ch == '\n')? <= budget {
                        best = Some(raw);
                    }
                }
            }
            let invalid = error.error_len().unwrap_or(bytes.len() - raw);
            raw = raw
                .checked_add(invalid)
                .ok_or(Error::Limit("Job output prefix overflow"))?;
            projected = projected.saturating_add('\u{fffd}'.len_utf8());
            if rendered_len(raw, projected, false)? <= budget {
                best = Some(raw);
            }
        }
        Ok(best.unwrap_or(0))
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
        // The final raw page can be smaller than an intermediate rendering
        // because `more offset=N` disappears at EOF. Always test it first.
        let full = render(&page.bytes)?;
        if full.len() <= budget {
            return Ok(full);
        }

        let kept = if encoding == "text" {
            Self::text_prefix_that_fits(page, offset, budget)?
        } else {
            // Base64 output size is monotonic while continuation metadata is
            // present, so binary search remains exact after the full-page test.
            let mut low = 0usize;
            let mut high = page.bytes.len().saturating_sub(1);
            let mut best = 0usize;
            while low <= high {
                let mid = low + (high - low) / 2;
                let rendered = render(&page.bytes[..mid])?;
                if rendered.len() <= budget {
                    best = mid;
                    low = mid.saturating_add(1);
                } else if mid == 0 {
                    break;
                } else {
                    high = mid - 1;
                }
            }
            best
        };
        if kept == 0 {
            return Err(Error::Limit(
                "one Job output byte cannot fit configured result ceiling",
            ));
        }
        let rendered = render(&page.bytes[..kept])?;
        debug_assert!(rendered.len() <= budget);
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
                ToolEffect::Control,
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
                ToolEffect::Control,
            ),
            JobToolKind::Cancel => (
                "job_cancel",
                "Idempotently request cancellation of one exposed live Job. Pending nodes skip; already-started native effects settle safely.",
                job_id_parameters(),
                ToolEffect::Control,
            ),
            JobToolKind::Restart => (
                "job_restart",
                "Explicitly create a fresh Job from one retained terminal Graph after full current schema/authority revalidation. This may repeat side effects.",
                job_id_parameters(),
                ToolEffect::Control,
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
                    let reference = host.job_ref(id, Some(node))?;
                    let content = aw_tools::receipt::render_plain(
                        aw_tools::receipt::Receipt::Ack {
                            reference: &reference,
                            mode: aw_tools::receipt::AckMode::Through(through),
                            changed: ack.changed,
                            released: ack.released,
                        },
                        direct_budget,
                    )
                    .map_err(|error| match error {
                        aw_tools::toolset::ToolsetError::ResultLimit => {
                            Error::Limit("Job ACK receipt exceeds current result budget")
                        }
                        _ => Error::Protocol("cannot render typed Job ACK receipt"),
                    })?;
                    Ok(ToolOutput::text(content))
                }
                JobToolKind::Cancel => {
                    let id = Self::id(arguments)?;
                    let (changed, snapshot) = self.runtime.cancel(&ctx, id)?;
                    let host = self.runtime.host_for(&ctx)?;
                    let reference = host.job_ref(id, None)?;
                    let content = aw_tools::receipt::render_plain(
                        aw_tools::receipt::Receipt::Cancel {
                            reference: &reference,
                            revision: snapshot.sequence,
                            changed,
                        },
                        direct_budget,
                    )
                    .map_err(|error| match error {
                        aw_tools::toolset::ToolsetError::ResultLimit => {
                            Error::Limit("Job cancellation receipt exceeds current result budget")
                        }
                        _ => Error::Protocol("cannot render typed Job cancellation receipt"),
                    })?;
                    Ok(ToolOutput::text(content))
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
            journal_unknown: false,
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
            offset: 0,
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

    fn handoff_job_id(content: &str) -> &str {
        let reference = content
            .split_whitespace()
            .next()
            .expect("handoff receipt needs canonical ref");
        if let Some(flat) = reference.strip_prefix("j:") {
            return flat.split('/').next().expect("flat Job ref has a handle");
        }
        reference
            .rsplit_once("/j:")
            .map(|(_, id)| id)
            .expect("handoff receipt needs finite Job segment")
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
        let (runtime_session, runtime_branch) = runtime.test_scope_identity();
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
            execution_owner: Some(
                aw_agent_core::AgentExecutionBinding::from_trusted_host(
                    "/d:0/i:0/p:0/a:0",
                    runtime_session,
                    runtime_branch,
                )
                .unwrap(),
            ),
            execution_scope: None,
            live_step_gate: None,
        };
        (root, agent, runtime, ctx)
    }

    #[test]
    fn failed_inline_retirement_keeps_handoff_guard_armed_and_exposes_same_job() {
        let (root, _agent, runtime, ctx) = fixture();
        let host = runtime.host_for(&ctx).unwrap();
        let prepared = host
            .prepare(
                &ctx,
                &json!({"ops":[["read",{"path":"a.txt","numbered":false}]]}),
            )
            .unwrap();
        let id = host.admit(prepared).unwrap();
        assert!(matches!(
            host.manager().get(&id),
            Err(aw_tools::filesystem::job_manager::Error::NotExposed)
        ));
        {
            let mut handoff = HandoffGuard::new(host.clone(), id.clone());
            assert!(
                handoff.retire_inline().is_err(),
                "a running hidden Job is not eligible for optimistic retirement"
            );
        }
        let snapshot = host.manager().get(&id).unwrap();
        assert_eq!(
            snapshot.lifecycle,
            aw_tools::filesystem::job::Lifecycle::Running
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    fn shell_fixture() -> (
        PathBuf,
        Agent,
        Arc<JobRuntime>,
        ToolContext,
        Arc<shell::NativeShell>,
    ) {
        shell_fixture_with_containment(Default::default())
    }

    #[cfg(unix)]
    fn shell_fixture_with_containment(
        containment: crate::process::ContainmentRequirement,
    ) -> (
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
        tools.local_process_containment = containment;
        let configured = crate::configure_with_runtime(builder, actor.clone(), &tools).unwrap();
        let agent = configured.builder.build().unwrap();
        let runtime = configured.job_runtime.unwrap();
        let (runtime_session, runtime_branch) = runtime.test_scope_identity();
        let shell = Arc::new(
            shell::NativeShell::new(root.canonicalize().unwrap(), bash)
                .unwrap()
                .requiring_containment(containment),
        );
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
            execution_owner: Some(
                aw_agent_core::AgentExecutionBinding::from_trusted_host(
                    "/d:0/i:0/p:0/a:0",
                    runtime_session,
                    runtime_branch,
                )
                .unwrap(),
            ),
            execution_scope: None,
            live_step_gate: None,
        };
        (root, agent, runtime, ctx, shell)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn graph_owned_background_process_holds_scope_until_terminal() {
        let (root, _agent, runtime, mut ctx, shell) = shell_fixture();
        let invocation_scope = aw_agent_core::InvocationScope {
            session_id: "fixture-session".into(),
            branch_id: "main".into(),
        };
        aw_agent_core::InvocationScopeLifecycle::replace_scope(
            runtime.as_ref(),
            None,
            Some(&invocation_scope),
        )
        .unwrap();
        ctx.scope = Some(invocation_scope.clone());
        ctx.execution_owner = Some(
            aw_agent_core::AgentExecutionBinding::from_trusted_host(
                "/d:0/i:0/p:0/a:0",
                invocation_scope.session_id.clone(),
                invocation_scope.branch_id.clone(),
            )
            .unwrap(),
        );
        let scope = aw_agent_core::ExecutionOwnershipScope::from_trusted_host(
            "/d:0/i:0/p:0/a:0/t:1/j:outer/n:1",
            CancellationToken::new(),
        )
        .unwrap();
        ctx.execution_scope = Some(scope.clone());
        ctx.call_id = "scoped-background-process".into();
        let tool = JobShell::new(runtime.clone(), shell);
        let receipt = tool
            .execute(
                ctx.clone(),
                &json!({
                    "command":"sleep 0.2; printf done",
                    "background":true
                }),
            )
            .await
            .unwrap();
        assert!(!receipt.is_error, "{}", receipt.content);
        assert_eq!(
            scope.active(),
            1,
            "background Process must keep the owning Graph-node scope live after handoff"
        );
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while scope.active() != 0 {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(!scope.is_unresolved());
        let _ = fs::remove_dir_all(root);
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
    async fn configured_shell_containment_refuses_direct_finite_and_graph_launches() {
        use crate::process::ContainmentRequirement;
        let (root, _agent, runtime, mut ctx, shell) =
            shell_fixture_with_containment(ContainmentRequirement::ClosedDescendants);
        let arguments = json!({"command":"printf spawned > marker"});
        let denial = "required closed descendant containment is unavailable";
        // The standalone executor uses the same minimum as the Job executor.
        assert!(matches!(
            shell.execute(ctx.clone(), &arguments).await,
            Err(Error::Denied(message)) if message == denial
        ));
        let direct = JobShell::new(runtime.clone(), shell.clone());
        let output = direct.execute(ctx.clone(), &arguments).await.unwrap();
        assert!(
            output.is_error && output.content.contains(denial),
            "{}",
            output.content
        );
        // The background finite constructor and a multi-node Graph both retain
        // exact no-launch facts under the original Job and output owners.
        let get = JobControl::new(runtime.clone(), JobToolKind::Get);
        let host = runtime.host_for(&ctx).unwrap();
        let strict_binding = shell.runtime_binding().unwrap();
        let graph = Batch::new(runtime.clone());
        ctx.call_id = "strict-finite".into();
        let finite = direct
            .execute(
                ctx.clone(),
                &json!({
                    "command":"printf spawned > marker", "background":true
                }),
            )
            .await
            .unwrap();
        ctx.call_id = "strict-graph".into();
        ctx.effect = ToolEffect::Control;
        let batch = graph
            .execute(
                ctx.clone(),
                &json!({
                    "ops":[["bash",arguments],["bash",{"command":"printf spawned > graph-marker"},"*"]],
                    "background":true
                }),
            )
            .await
            .unwrap();
        for (receipt, nodes) in [(finite, 1), (batch, 2)] {
            let id = handoff_job_id(&receipt.content).to_owned();
            let _ = wait_terminal(&get, ctx.clone(), &id).await;
            let snapshot = host.manager().runtime_snapshot(&id).unwrap();
            assert_eq!(snapshot.summary.unwrap().0, Primary::Fail);
            assert_eq!(snapshot.finished, nodes);
            assert!(!snapshot.journal_unknown);
            for plan in host.manager().plans(&id).unwrap() {
                assert_eq!(plan.runtime_binding(), strict_binding);
            }
            for coordinate in 1..=nodes {
                assert!(host
                    .manager()
                    .process_terminal(&id, coordinate)
                    .unwrap()
                    .is_none());
                for stream in [OutputStream::Stdout, OutputStream::Stderr] {
                    let (summary, finished) = host
                        .manager()
                        .output_summary(&id, coordinate, stream)
                        .unwrap();
                    assert!(finished);
                    assert_eq!(summary.total_bytes, 0);
                }
                let result = host
                    .manager()
                    .output_page(&id, coordinate, OutputStream::Result, 0, 4096)
                    .unwrap();
                assert!(String::from_utf8_lossy(&result.bytes).contains(denial));
            }
        }
        assert!(!root.join("marker").exists());
        assert!(!root.join("graph-marker").exists());
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
        let rendered = wait_terminal(&get, ctx.clone(), &id).await;
        assert!(rendered.starts_with("cancelled bash "));
        assert!(rendered.contains("stop=cancelled"));
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
        assert!(acknowledged.content.starts_with("j:"));
        assert!(acknowledged.content.contains(" ok ack="));
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
        let (root, agent, _runtime, mut ctx, shell) = shell_fixture();
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
        let (runtime_session, runtime_branch) = runtime.test_scope_identity();
        ctx.execution_owner = Some(
            aw_agent_core::AgentExecutionBinding::from_trusted_host(
                "/d:0/i:0/p:0/a:0",
                runtime_session,
                runtime_branch,
            )
            .unwrap(),
        );
        runtime.register_shell(shell.clone()).unwrap();
        let tool = JobShell::new(runtime.clone(), shell);
        let output = tool
            .execute(
                ctx.clone(),
                &json!({"command":"head -c 6000 /dev/zero | tr '\\0' x; head -c 6000 /dev/zero | tr '\\0' y >&2"}),
            )
            .await
            .unwrap();
        assert!(
            output.content.starts_with("j:") && output.content.contains(" bash settled "),
            "{}",
            output.content
        );
        assert!(output.content.len() <= tool_limits.result_bytes);
        let id = handoff_job_id(&output.content).to_owned();
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
    async fn graph_owned_direct_batch_rejects_before_job_admission() {
        let (root, _agent, runtime, mut ctx) = fixture();
        ctx.execution_scope = Some(
            aw_agent_core::ExecutionOwnershipScope::from_trusted_host(
                "/d:0/i:0/p:0/a:0/t:1/j:outer/n:1",
                CancellationToken::new(),
            )
            .unwrap(),
        );
        let tool = Batch::new(runtime.clone());
        let arguments = json!({"ops":[["read",{"path":"a.txt","numbered":false}]]});
        let result = tool.execute(ctx.clone(), &arguments).await;
        assert!(matches!(
            result,
            Err(Error::Denied(
                "batch cannot start while this work is owned by an active Graph node"
            ))
        ));
        assert!(
            runtime
                .host_for(&ctx)
                .unwrap()
                .manager()
                .list(0, 64)
                .unwrap()
                .jobs
                .is_empty(),
            "nested batch denial must happen before Job admission"
        );
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
        let (root, agent, _runtime, mut ctx) = fixture();
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
        let (runtime_session, runtime_branch) = runtime.test_scope_identity();
        ctx.execution_owner = Some(
            aw_agent_core::AgentExecutionBinding::from_trusted_host(
                "/d:0/i:0/p:0/a:0",
                runtime_session,
                runtime_branch,
            )
            .unwrap(),
        );
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
            receipt.content.starts_with("j:") && receipt.content.contains(" settled "),
            "{}",
            receipt.content
        );
        let id = handoff_job_id(&receipt.content);
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
        assert!(
            receipt.content.starts_with("j:") && receipt.content.contains(" graph settled "),
            "{}",
            receipt.content
        );
        let id = handoff_job_id(&receipt.content).to_owned();
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
            receipt.content.starts_with("j:") && receipt.content.contains(" settled "),
            "{}",
            receipt.content
        );
        let id = handoff_job_id(&receipt.content).to_owned();
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
        fs::write(root.join("b.txt"), "left").unwrap();
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
        let (runtime_session, runtime_branch) = runtime.test_scope_identity();
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
            execution_owner: Some(
                aw_agent_core::AgentExecutionBinding::from_trusted_host(
                    "/d:0/i:0/p:0/a:0",
                    runtime_session,
                    runtime_branch,
                )
                .unwrap(),
            ),
            execution_scope: None,
            live_step_gate: None,
        };
        let batch = Batch::new(runtime);
        let args = json!({"ops":[
            ["edit",{"path":"a.txt","edits":[["old","new"]]}],
            ["edit",{"path":"b.txt","edits":[["left","right"]]}]
        ]});
        let first = batch.execute(ctx.clone(), &args).await.unwrap();
        assert!(!first.is_error, "{}", first.content);
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "new");
        assert_eq!(fs::read_to_string(root.join("b.txt")).unwrap(), "right");
        let second = batch.execute(ctx, &args).await.unwrap();
        assert!(second.is_error);
        assert!(second.content.starts_with("unknown batch Job admission"));
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "new");
        assert_eq!(fs::read_to_string(root.join("b.txt")).unwrap(), "right");
        let _ = fs::remove_dir_all(root);
    }

    /// An authenticated Actor can narrow a peer tool without revoking the
    /// registered current ToolPolicy grant. It remains live after capture.
    struct LiveEnvelopeGate {
        read: std::sync::atomic::AtomicBool,
        ack: std::sync::atomic::AtomicBool,
    }
    impl aw_agent_core::control::AgentStepGate for LiveEnvelopeGate {
        fn with_authorized_tool_commit(
            &self,
            name: &str,
            source: &aw_agent_core::ToolSource,
            effect: &aw_agent_core::ToolEffect,
            commit: &mut dyn FnMut() -> aw_agent_core::Result<()>,
        ) -> aw_agent_core::Result<()> {
            // Test-only live permission switch; production controller holds
            // the authoritative Actor runtime mutex for the whole effect.
            self.allow_tool(name, source, effect)?;
            commit()
        }

        fn checkpoint<'a>(
            &'a self,
            _turn_id: u64,
            _round: u32,
            _point: aw_agent_core::control::AgentGatePoint,
        ) -> aw_agent_core::JournalFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }

        fn allow_tool(
            &self,
            name: &str,
            _source: &aw_agent_core::ToolSource,
            effect: &aw_agent_core::ToolEffect,
        ) -> aw_agent_core::Result<()> {
            use std::sync::atomic::Ordering;
            let ok = match name {
                "jobs_read" if *effect == ToolEffect::Read => self.read.load(Ordering::Acquire),
                "jobs_ack" if *effect == ToolEffect::Control => self.ack.load(Ordering::Acquire),
                _ => false,
            };
            ok.then_some(()).ok_or(aw_agent_core::Error::Denied(
                "current Actor Envelope forbids this exact peer tool",
            ))
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unknown_journal_get_returns_promptly_and_read_wait_does_not_claim_eof() {
        use aw_tools::filesystem::{
            job::{JournalError, NodeState},
            job_manager::Error as SharedError,
        };
        let (root, _agent, runtime, mut ctx, shell) = shell_fixture();
        ctx.call_id = "unknown-journal-observer".into();
        let started = JobShell::new(runtime.clone(), shell)
            .execute(
                ctx.clone(),
                &json!({
                    "command":"sleep 0.90; printf late", "background":true
                }),
            )
            .await
            .unwrap();
        let id = handoff_job_id(&started.content).to_owned();
        let host = runtime.host_for(&ctx).unwrap();
        let reference = host.execution_ref(&id, None).unwrap().canonical();

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(300);
        loop {
            if matches!(
                host.manager().node_state(&id, 1).unwrap(),
                NodeState::Running { .. }
            ) {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "Process did not start"
            );
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let current = host.manager().get(&id).unwrap();
        assert_eq!(
            host.manager().cancel(&id, current.sequence + 1, |_, _| Err(
                JournalError::OutcomeUnknown
            ),),
            Err(SharedError::Job(
                aw_tools::filesystem::job::Error::JournalUnknown
            ))
        );
        assert!(host.manager().get(&id).unwrap().journal_unknown);
        assert_eq!(
            host.manager().get(&id).unwrap().lifecycle,
            aw_tools::filesystem::job::Lifecycle::Running,
            "journal uncertainty cannot claim terminal execution"
        );

        let get =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Get);
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(350),
            get.execute(
                ctx.clone(),
                &json!({
                    "ref":reference,"tail":0,"wait_ms":1200
                }),
            ),
        )
        .await
        .expect("journal-fenced get must not wait for settlement")
        .unwrap();
        assert!(result.content.contains("unknown"), "{}", result.content);
        assert!(
            result.content.contains("journal=unknown"),
            "{}",
            result.content
        );
        assert!(!result.content.contains("settled"), "{}", result.content);

        let read =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Read);
        let rejected = tokio::time::timeout(
            std::time::Duration::from_millis(350),
            read.execute(
                ctx.clone(),
                &json!({
                    "ref":reference,"view":"stdout","offset":0,"limit":128,"wait_ms":1200
                }),
            ),
        )
        .await
        .expect("unready output must not wait to deadline after journal fence");
        assert!(
            matches!(
                rejected,
                Err(Error::Conflict(
                    "Job journal outcome unknown; reconcile before waiting for output"
                ))
            ),
            "{rejected:?}"
        );

        tokio::time::sleep(std::time::Duration::from_millis(950)).await;
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn finite_jobs_get_waits_for_settlement_not_output_or_revision() {
        let (root, _agent, runtime, mut ctx, shell) = shell_fixture();
        ctx.call_id = "wait-settlement".into();
        let handoff = JobShell::new(runtime.clone(), shell)
            .execute(
                ctx.clone(),
                &json!({"command":"printf early; sleep 0.30; printf late", "background":true}),
            )
            .await
            .unwrap();
        let id = handoff_job_id(&handoff.content).to_owned();
        let host = runtime.host_for(&ctx).unwrap();
        let reference = host.execution_ref(&id, None).unwrap().canonical();
        let get =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Get);
        // A short deadline returns the current status, even if output arrived
        // and the original rev=1 is stale. It is not a revision wait.
        let deadline = get
            .execute(
                ctx.clone(),
                &json!({"ref":reference,"rev":1,"wait_ms":35,"tail":0}),
            )
            .await
            .unwrap();
        assert!(deadline.content.contains("running"), "{}", deadline.content);

        let settled = get
            .execute(
                ctx.clone(),
                &json!({"ref":reference,"rev":1,"wait_ms":1500,"tail":0}),
            )
            .await
            .unwrap();
        assert!(settled.content.contains("settled"), "{}", settled.content);
        assert_eq!(
            host.manager().get(&id).unwrap().lifecycle,
            aw_tools::filesystem::job::Lifecycle::Terminal
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn finite_job_wait_cancellation_does_not_cancel_the_underlying_process() {
        let (root, _agent, runtime, mut ctx, shell) = shell_fixture();
        ctx.call_id = "wait-cancel".into();
        let handoff = JobShell::new(runtime.clone(), shell)
            .execute(
                ctx.clone(),
                &json!({"command":"sleep 0.55; printf survived", "background":true}),
            )
            .await
            .unwrap();
        let id = handoff_job_id(&handoff.content).to_owned();
        let host = runtime.host_for(&ctx).unwrap();
        let reference = host.execution_ref(&id, None).unwrap().canonical();
        let get =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Get);
        let read =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Read);

        let mut observer = ctx.clone();
        observer.cancellation = CancellationToken::new();
        let cancel = observer.cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            cancel.cancel();
        });
        assert!(matches!(
            get.execute(
                observer,
                &json!({
                    "ref":reference, "tail":0, "wait_ms":1100,
                })
            )
            .await,
            Err(Error::Cancelled)
        ));

        let mut observer = ctx.clone();
        observer.cancellation = CancellationToken::new();
        let cancel = observer.cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            cancel.cancel();
        });
        assert!(matches!(
            read.execute(
                observer,
                &json!({
                    "ref":reference, "view":"stdout", "offset":0,
                    "limit":64, "wait_ms":1100,
                })
            )
            .await,
            Err(Error::Cancelled)
        ));
        // A separate observer's cancellation must not cancel or ACK the
        // actual Process. The original execution still settles normally.
        let complete = get
            .execute(
                ctx.clone(),
                &json!({
                    "ref":reference, "tail":0, "wait_ms":1100
                }),
            )
            .await
            .unwrap();
        assert!(complete.content.contains("settled"), "{}", complete.content);
        let data = read
            .execute(
                ctx.clone(),
                &json!({
                    "ref":reference, "view":"stdout", "offset":0,
                    "limit":64, "wait_ms":0,
                }),
            )
            .await
            .unwrap();
        assert_eq!(data.content, "survived");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn node_get_waits_for_its_own_settlement_without_waiting_for_graph_root() {
        let (root, _agent, runtime, mut ctx, _shell) = shell_fixture();
        ctx.call_id = "wait-graph-node".into();
        let handoff = Batch::new(runtime.clone())
            .execute(
                ctx.clone(),
                &json!({"ops":[
                ["bash",{"command":"printf first"},"*"],
                ["bash",{"command":"sleep 0.65; printf second"},"*"]
            ],"background":true}),
            )
            .await
            .unwrap();
        let id = handoff_job_id(&handoff.content).to_owned();
        let host = runtime.host_for(&ctx).unwrap();
        let node = host.execution_ref(&id, Some(1)).unwrap().canonical();
        let root_ref = host.execution_ref(&id, None).unwrap().canonical();
        let get =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Get);
        let first = get
            .execute(
                ctx.clone(),
                &json!({
                    "ref":node,"rev":1,"wait_ms":1500,"tail":0
                }),
            )
            .await
            .unwrap();
        assert!(first.content.contains("settled"), "{}", first.content);
        assert_eq!(
            host.manager().get(&id).unwrap().lifecycle,
            aw_tools::filesystem::job::Lifecycle::Running,
            "a settled node should not require unrelated sibling settlement"
        );
        let whole = get
            .execute(
                ctx.clone(),
                &json!({
                    "ref":root_ref,"rev":1,"wait_ms":1500,"tail":0
                }),
            )
            .await
            .unwrap();
        assert!(whole.content.contains("settled"), "{}", whole.content);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn skipped_node_read_does_not_wait_for_sibling() {
        let (root, _agent, runtime, mut ctx, _shell) = shell_fixture();
        fs::write(root.join("a.txt"), "a").unwrap();
        ctx.call_id = "skipped-node-read".into();
        let started = Batch::new(runtime.clone())
            .execute(
                ctx.clone(),
                &json!({
                    "ops":[
                        ["bash", {"command":"false"}, "*"],
                        ["read", {"path":"a.txt","numbered":false}, "1.ok"],
                        ["bash", {"command":"sleep 1.2; printf sibling"}, "*"]
                    ],
                    "background":true
                }),
            )
            .await
            .unwrap();
        let id = handoff_job_id(&started.content).to_owned();
        let host = runtime.host_for(&ctx).unwrap();
        let node = host.execution_ref(&id, Some(2)).unwrap().canonical();
        let get =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Get);
        let status = get
            .execute(ctx.clone(), &json!({"ref":node,"tail":0,"wait_ms":1000}))
            .await
            .unwrap();
        assert!(status.content.contains("settled"), "{}", status.content);
        assert_eq!(
            host.manager().get(&id).unwrap().lifecycle,
            aw_tools::filesystem::job::Lifecycle::Running
        );
        let read =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Read);
        let outcome = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            read.execute(
                ctx.clone(),
                &json!({
                    "ref":node,"view":"result","offset":0,"limit":64,"wait_ms":1200
                }),
            ),
        )
        .await
        .expect("settled node read must not wait for sibling");
        assert!(matches!(
            outcome,
            Err(Error::Denied("Job output view is unavailable"))
        ));
        assert_eq!(
            host.manager().get(&id).unwrap().lifecycle,
            aw_tools::filesystem::job::Lifecycle::Running
        );
        // Finish an exact settled Skip with no output streams while its
        // independent sibling still runs. No false missing-output error.
        let ack =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Ack);
        let finished = ack
            .execute(ctx.clone(), &json!({"ref":node}))
            .await
            .unwrap();
        let (receipt, provenance) = finished.content.split_once(" provenance=").unwrap();
        assert!(receipt.starts_with("j:"));
        assert!(receipt.ends_with(" unchanged ack=all released=true"));
        let provenance: serde_json::Value = serde_json::from_str(provenance).unwrap();
        let expected = host.finite_job_provenance(&id).unwrap();
        assert_eq!(
            provenance,
            json!({
                "job": expected.canonical_job_ref,
                "agent": expected.original_agent_ref,
                "turn": expected.original_turn,
                "actor": expected.initiating_actor,
            })
        );
        let again = ack
            .execute(ctx.clone(), &json!({"ref":node}))
            .await
            .unwrap();
        assert_eq!(again.content, finished.content);
        assert!(
            ack.execute(ctx.clone(), &json!({"ref":node,"view":"result"}))
                .await
                .is_err(),
            "an explicit nonexistent output stream is still invalid"
        );
        let root_ref = host.execution_ref(&id, None).unwrap().canonical();
        let whole = get
            .execute(
                ctx.clone(),
                &json!({"ref":root_ref,"tail":0,"wait_ms":1500}),
            )
            .await
            .unwrap();
        assert!(whole.content.contains("settled"), "{}", whole.content);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn late_attached_graph_result_wakes_reader_and_only_credits_delivery() {
        use aw_tools::filesystem::job_manager::OutputStream;
        let (root, _agent, runtime, mut ctx, _shell) = shell_fixture();
        fs::write(root.join("a.txt"), "late-result").unwrap();
        ctx.call_id = "late-attach-graph".into();
        let started = Batch::new(runtime.clone())
            .execute(
                ctx.clone(),
                &json!({
                    "ops":[
                        ["bash",{"command":"sleep 0.30"},"*"],
                        ["read",{"path":"a.txt","numbered":false},"1.ok"]
                    ],"background":true
                }),
            )
            .await
            .unwrap();
        let id = handoff_job_id(&started.content).to_owned();
        let host = runtime.host_for(&ctx).unwrap();
        let node = host.execution_ref(&id, Some(2)).unwrap().canonical();
        assert!(
            matches!(
                host.manager().output_summary(&id, 2, OutputStream::Result),
                Err(aw_tools::filesystem::job_manager::Error::OutputMissing)
            ),
            "dependent result must not be attached before predecessor completes"
        );
        let read =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Read);
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            read.execute(
                ctx.clone(),
                &json!({
                    "ref":node,"view":"result","offset":0,"limit":128,"wait_ms":1500
                }),
            ),
        )
        .await
        .expect("late output attachment must wake the reader")
        .unwrap();
        assert_eq!(output.content, "late-result");
        let owner = host.manager().job_output_owner(&id).unwrap();
        let state = host
            .manager()
            .output_owner_state(&id, 2, OutputStream::Result, owner)
            .unwrap();
        assert_eq!(state.through, 0, "delivery is not an ACK");
        assert!(!state.acknowledged);
        // A successful page makes exactly its projected raw prefix eligible
        // for an *explicit* ACK; the page itself does not acknowledge.
        host.manager()
            .acknowledge_delivered_output(
                &id,
                2,
                OutputStream::Result,
                owner,
                b"late-result".len() as u64,
            )
            .unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn revoking_read_during_stream_wait_prevents_secret_delivery_or_credit() {
        use aw_tools::filesystem::job_manager::OutputStream;
        use std::sync::atomic::{AtomicBool, Ordering};
        let (root, _agent, runtime, mut ctx, shell) = shell_fixture();
        ctx.call_id = "revoke-mid-wait".into();
        let started = JobShell::new(runtime.clone(), shell)
            .execute(
                ctx.clone(),
                &json!({
                    "command":"sleep 0.35; printf NEVER_DELIVER",
                    "background":true
                }),
            )
            .await
            .unwrap();
        let id = handoff_job_id(&started.content).to_owned();
        let host = runtime.host_for(&ctx).unwrap();
        let reference = host.execution_ref(&id, None).unwrap().canonical();
        let gate = Arc::new(LiveEnvelopeGate {
            read: AtomicBool::new(true),
            ack: AtomicBool::new(false),
        });
        ctx.live_step_gate = Some(gate.clone());
        let read =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Read);
        let observer = tokio::spawn(async move {
            read.execute(
                ctx,
                &json!({
                    "ref":reference,"view":"stdout","offset":0,"limit":128,"wait_ms":1500
                }),
            )
            .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        gate.read.store(false, Ordering::Release);
        let rejected = tokio::time::timeout(std::time::Duration::from_secs(2), observer)
            .await
            .expect("read wait must finish on byte arrival")
            .unwrap();
        assert!(
            matches!(
                rejected,
                Err(Error::Denied("Job payload authority revoked while waiting"))
                    | Err(Error::Denied(
                        "Job output authority revoked before delivery"
                    ))
                    | Err(Error::Denied("Job payload authority is not live"))
            ),
            "{rejected:?}"
        );
        let owner = host.manager().job_output_owner(&id).unwrap();
        let state = host
            .manager()
            .output_owner_state(&id, 1, OutputStream::Stdout, owner)
            .unwrap();
        assert_eq!(state.through, 0);
        assert!(!state.acknowledged);
        assert_eq!(
            host.manager()
                .acknowledge_delivered_output(&id, 1, OutputStream::Stdout, owner, 1),
            Err(aw_tools::filesystem::job_manager::Error::InvalidOutput),
            "revoked waiting read never certifies any secret byte",
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn finite_jobs_read_waits_for_bytes_then_eof_without_ack() {
        use aw_tools::filesystem::job_manager::OutputStream;
        let (root, _agent, runtime, mut ctx, shell) = shell_fixture();
        ctx.call_id = "wait-stream".into();
        let handoff = JobShell::new(runtime.clone(), shell)
            .execute(
                ctx.clone(),
                &json!({"command":"sleep 0.10; printf chunk-one; sleep 0.28", "background":true}),
            )
            .await
            .unwrap();
        let id = handoff_job_id(&handoff.content).to_owned();
        let host = runtime.host_for(&ctx).unwrap();
        let reference = host.execution_ref(&id, None).unwrap().canonical();
        let owner = host.manager().job_output_owner(&id).unwrap();
        let read =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Read);
        let first = read
            .execute(
                ctx.clone(),
                &json!({
                    "ref":reference,"view":"stdout","offset":0,
                    "limit":64,"wait_ms":1000
                }),
            )
            .await
            .unwrap();
        assert!(
            !first.is_error && first.content.contains("chunk-one"),
            "{}",
            first.content
        );
        // Waiting/reading does not ACK; only the bounded result projection
        // grants its exact retained prefix.
        let span = b"chunk-one".len() as u64;
        let ack = host.manager().acknowledge_delivered_output(
            &id,
            1,
            OutputStream::Stdout,
            owner,
            span + 1,
        );
        assert!(ack.is_err());

        let eof = read
            .execute(
                ctx.clone(),
                &json!({
                    "ref":reference,"view":"stdout","offset":span,
                    "limit":64,"wait_ms":1000
                }),
            )
            .await
            .unwrap();
        assert!(!eof.is_error, "{}", eof.content);
        assert_eq!(eof.content, "");
        assert!(
            host.manager()
                .output_summary(&id, 1, OutputStream::Stdout,)
                .unwrap()
                .1,
            "writer must have closed before byte wait finishes"
        );
        assert!(matches!(
            read.execute(
                ctx.clone(),
                &json!({
                    "ref":reference,"view":"stdout","offset":-3,
                    "limit":64,"wait_ms":1
                })
            )
            .await,
            Err(Error::Denied(
                "negative-offset jobs_read is an immediate tail snapshot"
            ))
        ));
        let tail = read
            .execute(
                ctx.clone(),
                &json!({
                    "ref":reference,"view":"stdout","offset":-3,
                    "limit":64,"wait_ms":0
                }),
            )
            .await
            .unwrap();
        assert_eq!(tail.content, "one");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn jobs_get_tail_requires_independent_live_read_envelope() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let (root, _agent, runtime, mut ctx, shell) = shell_fixture();
        ctx.call_id = "opaque-shell-receipt".into();
        let handoff = JobShell::new(runtime.clone(), shell)
            .execute(
                ctx.clone(),
                &json!({"command":"printf SECRET_OUTPUT_12345", "background":true}),
            )
            .await
            .unwrap();
        let id = handoff_job_id(&handoff.content).to_owned();
        let host = runtime.host_for(&ctx).unwrap();
        let canonical = host.execution_ref(&id, None).unwrap().canonical();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if host.manager().get(&id).unwrap().lifecycle
                    == aw_tools::filesystem::job::Lifecycle::Terminal
                {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();

        assert!(runtime.payload_read_authorized(&ctx));
        let gate = Arc::new(LiveEnvelopeGate {
            read: AtomicBool::new(false),
            ack: AtomicBool::new(true),
        });
        ctx.live_step_gate = Some(gate.clone());
        let get =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Get);
        let default = get
            .execute(ctx.clone(), &json!({"ref":canonical}))
            .await
            .unwrap();
        assert!(
            !default.content.contains("SECRET_OUTPUT"),
            "{}",
            default.content
        );
        assert!(
            default.content.contains("tail=denied"),
            "{}",
            default.content
        );
        assert!(matches!(
            get.execute(ctx.clone(), &json!({"ref":canonical,"tail":64}))
                .await,
            Err(Error::Denied("Job output tail is not currently authorized"))
        ));
        let status = get
            .execute(ctx.clone(), &json!({"ref":canonical,"tail":0}))
            .await
            .unwrap();
        assert!(!status.content.contains("tail=denied"));
        assert!(!status.content.contains("SECRET_OUTPUT"));

        gate.read.store(true, Ordering::Release);
        let allowed = get
            .execute(ctx.clone(), &json!({"ref":canonical,"tail":64}))
            .await
            .unwrap();
        assert!(
            allowed.content.contains("SECRET_OUTPUT_12345"),
            "{}",
            allowed.content
        );
        gate.read.store(false, Ordering::Release);
        assert!(!runtime.payload_read_authorized(&ctx));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn job_control_commit_orders_exact_policy_revocation_and_rejects_stale_grant() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            mpsc,
        };

        let (root, mut agent, runtime, ctx) = fixture();
        assert!(runtime.control_authorized(&ctx, "jobs_ack"));
        let (start_tx, start_rx) = mpsc::channel();
        let (attempt_tx, attempt_rx) = mpsc::channel();
        let executed = Arc::new(AtomicBool::new(false));
        let executed_for_revocation = executed.clone();
        let actor = ctx.actor.clone();
        let change = std::thread::spawn(move || {
            start_rx.recv().unwrap();
            attempt_tx.send(()).unwrap();
            agent
                .revoke_registered_source_tool(&actor, "jobs_ack", &ToolSource::trusted_host())
                .unwrap();
            assert!(
                executed_for_revocation.load(Ordering::SeqCst),
                "Control grant publication must wait for in-flight effect"
            );
        });
        let value = runtime
            .with_control_commit(&ctx, "jobs_ack", || {
                start_tx.send(()).unwrap();
                attempt_rx.recv().unwrap();
                executed.store(true, Ordering::SeqCst);
                Ok(42)
            })
            .unwrap();
        assert_eq!(value, 42);
        change.join().unwrap();
        assert!(!runtime.control_authorized(&ctx, "jobs_ack"));
        assert!(matches!(
            runtime.with_control_commit(&ctx, "jobs_ack", || -> Result<()> {
                panic!("revoked grant executed")
            }),
            Err(Error::Denied(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn control_gate_failing_after_effect_reports_uncertain_not_denied() {
        struct FaultAfterCommit;
        impl aw_agent_core::control::AgentStepGate for FaultAfterCommit {
            fn checkpoint<'a>(
                &'a self,
                _: u64,
                _: u32,
                _: aw_agent_core::control::AgentGatePoint,
            ) -> aw_agent_core::JournalFuture<'a, ()> {
                Box::pin(async { Ok(()) })
            }
            fn allow_tool(
                &self,
                _: &str,
                _: &ToolSource,
                _: &ToolEffect,
            ) -> aw_agent_core::Result<()> {
                Ok(())
            }
            fn with_authorized_tool_commit(
                &self,
                _: &str,
                _: &ToolSource,
                _: &ToolEffect,
                commit: &mut dyn FnMut() -> aw_agent_core::Result<()>,
            ) -> aw_agent_core::Result<()> {
                commit()?;
                Err(aw_agent_core::Error::Denied(
                    "injected post-effect gate failure",
                ))
            }
        }
        let (root, _agent, runtime, mut ctx) = fixture();
        ctx.live_step_gate = Some(Arc::new(FaultAfterCommit));
        let executed = std::cell::Cell::new(0usize);
        assert!(matches!(
            runtime.with_control_commit(&ctx, "jobs_ack", || {
                executed.set(executed.get() + 1);
                Ok(7)
            }),
            Err(Error::Transport(
                "Job Control outcome uncertain; inspect before retry"
            ))
        ));
        assert_eq!(
            executed.get(),
            1,
            "no implicit retry after an uncertain effect"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn live_envelope_independently_gates_finite_read_and_ack() {
        use aw_tools::filesystem::job_manager::OutputStream;
        use std::sync::atomic::{AtomicBool, Ordering};

        let (root, _agent, runtime, mut ctx) = fixture();
        fs::write(root.join("a.txt"), "x".repeat(2048)).unwrap();
        ctx.result_budget_bytes = 512;
        ctx.call_id = "live-envelope-job".into();
        let handoff = Batch::new(runtime.clone())
            .execute(
                ctx.clone(),
                &json!({"ops":[["read",{"path":"a.txt","numbered":false}]]}),
            )
            .await
            .unwrap();
        let id = handoff_job_id(&handoff.content).to_owned();
        let host = runtime.host_for(&ctx).unwrap();
        let reference = host.execution_ref(&id, Some(1)).unwrap().canonical();
        let original = host
            .manager()
            .output_summary(&id, 1, OutputStream::Result)
            .unwrap()
            .0;
        assert!(original.retained_bytes > 512);

        // The original ToolPolicy grants both, but the captured Actor gate
        // can independently and live-revocably deny either operation.
        assert!(runtime.payload_read_authorized(&ctx));
        assert!(runtime.control_authorized(&ctx, "jobs_ack"));
        let gate = Arc::new(LiveEnvelopeGate {
            read: AtomicBool::new(false),
            ack: AtomicBool::new(false),
        });
        ctx.live_step_gate = Some(gate.clone());
        assert!(!runtime.payload_read_authorized(&ctx));
        assert!(!runtime.control_authorized(&ctx, "jobs_ack"));

        let read =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Read);
        assert!(matches!(
            read.execute(
                ctx.clone(),
                &json!({
                    "ref":reference,"view":"result","offset":0,"limit":100
                })
            )
            .await,
            Err(Error::Denied("Job payload authority is not live"))
        ));
        let ack =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Ack);
        assert!(matches!(
            ack.execute(
                ctx.clone(),
                &json!({
                    "ref":reference
                })
            )
            .await,
            Err(Error::Denied("Job ACK Control authority is not live"))
        ));
        assert_eq!(
            host.manager()
                .output_summary(&id, 1, OutputStream::Result)
                .unwrap()
                .0
                .retained_bytes,
            original.retained_bytes,
            "denied read/Control must not consume or release the original payload"
        );

        gate.read.store(true, Ordering::Release);
        assert!(runtime.payload_read_authorized(&ctx));
        let page = read
            .execute(
                ctx.clone(),
                &json!({
                    "ref":reference,"view":"result","offset":0,"limit":100
                }),
            )
            .await
            .unwrap();
        assert!(!page.is_error);
        gate.read.store(false, Ordering::Release);
        assert!(!runtime.payload_read_authorized(&ctx));

        gate.ack.store(true, Ordering::Release);
        assert!(runtime.control_authorized(&ctx, "jobs_ack"));
        let released = ack
            .execute(ctx.clone(), &json!({"ref":reference}))
            .await
            .unwrap();
        assert!(released.content.contains("ack=all"), "{}", released.content);
        assert!(host
            .manager()
            .output_summary(&id, 1, OutputStream::Result)
            .is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn finite_job_namespace_is_single_agent_audience_and_actor_bound() {
        let (root, _agent, runtime, mut ctx) = fixture();
        fs::write(root.join("a.txt"), "x".repeat(2048)).unwrap();
        ctx.call_id = "single-audience".into();
        ctx.result_budget_bytes = 512;
        let receipt = Batch::new(runtime.clone())
            .execute(
                ctx.clone(),
                &json!({"ops":[["read",{"path":"a.txt","numbered":false}]]}),
            )
            .await
            .unwrap();
        let id = handoff_job_id(&receipt.content).to_owned();
        let host = runtime.host_for(&ctx).unwrap();
        assert!(host.payload_authorized(&ctx.actor, &id));

        let (session, branch) = runtime.test_scope_identity();
        let mut other_agent = ctx.clone();
        other_agent.execution_owner = Some(
            aw_agent_core::AgentExecutionBinding::from_trusted_host(
                "/d:0/i:0/p:0/a:1",
                &session,
                &branch,
            )
            .unwrap(),
        );
        assert!(
            matches!(
                runtime.host_for(&other_agent),
                Err(Error::Conflict(
                    "live Job namespace cannot be rebound to another Agent"
                ))
            ),
            "another Agent's Job controls must not reuse this Job namespace"
        );

        let mut other_actor = ctx.clone();
        other_actor.actor = PrincipalId::new("different-actor").unwrap();
        let same_host = runtime.host_for(&other_actor).unwrap();
        assert!(
            !same_host.payload_authorized(&other_actor.actor, &id),
            "a second Actor cannot inherit the producing Actor's result read"
        );
        assert!(
            host.manager().job_output_owner(&id).is_ok(),
            "the existing single-Agent audience retains the stable Job lease"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn finite_jobs_read_only_credits_the_final_bounded_page() {
        use aw_tools::filesystem::job_manager::OutputStream;
        let (root, _agent, runtime, mut ctx) = fixture();
        fs::write(root.join("a.txt"), "x".repeat(2048)).unwrap();
        ctx.call_id = "credit-batch".into();
        ctx.result_budget_bytes = 512;
        // Use the canonical finite-Graph constructor, not the legacy
        // JobControl::Start projection with no FiniteJobKind metadata.
        let batch = Batch::new(runtime.clone());
        let started = batch
            .execute(
                ctx.clone(),
                &json!({"ops":[
                    ["read",{"path":"a.txt","numbered":false,"lines":4}]
                ]}),
            )
            .await
            .unwrap();
        assert!(started
            .content
            .contains(" next=jobs_get|jobs_read no-rerun"));
        let id = handoff_job_id(&started.content).to_owned();
        let host = runtime.host_for(&ctx).unwrap();
        let owner = host.manager().job_output_owner(&id).unwrap();
        // A Graph root synthesizes an aggregate result (not an owned byte
        // stream). Node ref n:1 targets the exact retained result lease.
        let reference = host.execution_ref(&id, Some(1)).unwrap().canonical();
        let summary = host
            .manager()
            .output_summary(&id, 1, OutputStream::Result)
            .unwrap()
            .0;
        assert!(summary.retained_bytes > 512);
        let read =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Read);
        assert_eq!(
            host.manager()
                .acknowledge_delivered_output(&id, 1, OutputStream::Result, owner, 1),
            Err(aw_tools::filesystem::job_manager::Error::InvalidOutput),
            "unconsumed native result has no delivered proof"
        );
        ctx.result_budget_bytes = 512;
        let page = read
            .execute(
                ctx.clone(),
                &json!({
                    "ref":reference,"view":"result","offset":0,"limit":4096,"encoding":"base64"
                }),
            )
            .await
            .unwrap();
        assert!(!page.is_error, "{}", page.content);
        assert!(page.content.len() <= 512);
        let next: u64 = page
            .content
            .lines()
            .find_map(|line| line.strip_prefix("more offset="))
            .unwrap()
            .parse()
            .unwrap();
        assert!(next > 0 && next < summary.retained_bytes);
        assert_eq!(
            host.manager().acknowledge_delivered_output(
                &id,
                1,
                OutputStream::Result,
                owner,
                next + 1
            ),
            Err(aw_tools::filesystem::job_manager::Error::InvalidOutput),
            "bytes fetched but omitted by the final projector are not ACKable"
        );
        let acknowledged = host
            .manager()
            .acknowledge_delivered_output(&id, 1, OutputStream::Result, owner, next)
            .unwrap();
        assert!(acknowledged.changed && !acknowledged.released);

        // A later successfully rendered page still cannot advance the prefix
        // across unread bytes.
        let skipped = read
            .execute(
                ctx.clone(),
                &json!({
                    "ref":reference,"view":"result","offset":next+100,
                    "limit":80,"encoding":"base64"
                }),
            )
            .await
            .unwrap();
        assert!(!skipped.is_error);
        assert_eq!(
            host.manager().acknowledge_delivered_output(
                &id,
                1,
                OutputStream::Result,
                owner,
                next + 101
            ),
            Err(aw_tools::filesystem::job_manager::Error::InvalidOutput)
        );
        // Only the next contiguous page extends the ACKable prefix.
        let contiguous = read
            .execute(
                ctx.clone(),
                &json!({
                    "ref":reference,"view":"result","offset":next,
                    "limit":80,"encoding":"base64"
                }),
            )
            .await
            .unwrap();
        let next_contiguous: u64 = contiguous
            .content
            .lines()
            .find_map(|line| line.strip_prefix("more offset="))
            .unwrap()
            .parse()
            .unwrap();
        assert!(next_contiguous > next);
        host.manager()
            .acknowledge_delivered_output(&id, 1, OutputStream::Result, owner, next_contiguous)
            .unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn unified_jobs_read_omitted_limit_uses_shared_page_default() {
        use aw_tools::toolset::JobProfile;
        let (root, agent, _default_runtime, mut ctx) = fixture();
        let line = format!("{}\n", "x".repeat(20));
        fs::write(root.join("large.txt"), line.repeat(100)).unwrap();
        let limits = crate::LocalToolLimits {
            job_profile: JobProfile {
                read_page_default_bytes: 512,
                read_page_max_bytes: 1024,
                ..Default::default()
            },
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
        let (session, branch) = runtime.test_scope_identity();
        ctx.execution_owner = Some(
            aw_agent_core::AgentExecutionBinding::from_trusted_host(
                "/d:0/i:0/p:0/a:0",
                session,
                branch,
            )
            .unwrap(),
        );
        ctx.call_id = "default-page-batch".into();
        // Force durable handoff; retained output is much larger than this
        // deliberately tiny configured read-page default.
        ctx.result_budget_bytes = 512;
        let started = Batch::new(runtime.clone())
            .execute(
                ctx.clone(),
                &json!({"ops":[
                    ["read",{"path":"large.txt","numbered":false,"lines":200}]
                ]}),
            )
            .await
            .unwrap();
        let id = handoff_job_id(&started.content).to_owned();
        let host = runtime.host_for(&ctx).unwrap();
        ctx.result_budget_bytes = aw_agent_core::tool::MAX_TOOL_RESULT_BYTES;
        let reference = host.execution_ref(&id, Some(1)).unwrap().canonical();
        let read = crate::jobs_tools::JobsTool::new(runtime, crate::jobs_tools::JobsKind::Read);
        let page = read
            .execute(
                ctx,
                &json!({"ref":reference,"view":"result","offset":0,"encoding":"text"}),
            )
            .await
            .unwrap();
        assert_eq!(
            page.content.lines().last(),
            Some("more offset=512"),
            "omitted limit must use JobProfile.read_page_default_bytes"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn one_byte_jobs_read_page_uses_normal_serialized_envelope() {
        use aw_tools::toolset::JobProfile;
        let (root, agent, _default_runtime, mut ctx) = fixture();
        fs::write(
            root.join("one-byte-page.txt"),
            "abcdef
",
        )
        .unwrap();
        let limits = crate::LocalToolLimits {
            job_profile: JobProfile {
                read_page_default_bytes: 1,
                read_page_max_bytes: 1,
                ..Default::default()
            },
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
        let (session, branch) = runtime.test_scope_identity();
        ctx.execution_owner = Some(
            aw_agent_core::AgentExecutionBinding::from_trusted_host(
                "/d:0/i:0/p:0/a:0",
                session,
                branch,
            )
            .unwrap(),
        );
        ctx.call_id = "one-byte-page-batch".into();
        // Force durable handoff with an ordinary factual response envelope.
        ctx.result_budget_bytes = aw_agent_core::tool::MIN_TOOL_RESULT_BYTES;
        let started = Batch::new(runtime.clone())
            .execute(
                ctx.clone(),
                &json!({
                    "background":true,
                    "ops":[["read",{"path":"one-byte-page.txt","numbered":false,"lines":8}]]
                }),
            )
            .await
            .unwrap();
        let id = handoff_job_id(&started.content).to_owned();
        let host = runtime.host_for(&ctx).unwrap();
        let reference = host.execution_ref(&id, Some(1)).unwrap().canonical();
        let read = crate::jobs_tools::JobsTool::new(runtime, crate::jobs_tools::JobsKind::Read);
        let page = read
            .execute(
                ctx,
                &json!({"ref":reference,"view":"result","offset":0,"limit":1,"wait_ms":1000,"encoding":"text"}),
            )
            .await
            .unwrap();
        assert!(
            page.content.len() > 1,
            "serialized envelope metadata is not raw page data"
        );
        assert!(page.content.len() <= aw_agent_core::tool::MIN_TOOL_RESULT_BYTES);
        assert_eq!(
            page.content.lines().last(),
            Some("more offset=1"),
            "one raw byte must advance by exactly one byte inside the larger response envelope"
        );
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
        assert!(acked.content.starts_with("j:"));
        assert!(acked.content.contains(" ok ack="));
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
        assert!(unchanged.content.starts_with("j:"));
        assert!(unchanged.content.contains(" cancel=unchanged"));

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
        tool_limits.jobs.max_exposed_jobs = 4;
        tool_limits.jobs.max_hidden_jobs = 4;
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
    fn shared_toolset_builder_configures_real_aw_job_catalog() {
        use aw_tools::toolset::{JobProfile, Toolset};
        let (root, agent, _, _) = fixture();
        let factory = Toolset::builder()
            .jobs(JobProfile {
                get_tail_default: 256,
                get_tail_max: 384,
                get_wait_max_ms: 1200,
                read_page_default_bytes: 512,
                read_page_max_bytes: 2048,
                read_wait_max_ms: 0,
                list_page_max: 10,
            })
            .build()
            .unwrap();
        let limits = crate::LocalToolLimits::default()
            .with_toolset(&factory)
            .validate()
            .unwrap();
        let runtime = JobRuntime::new_with_native_tools(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            limits,
            crate::LocalToolConcurrency::default(),
            aw_tools::filesystem::schema::Tool::GRAPH_NATIVE.to_vec(),
        )
        .unwrap();
        let get =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Get);
        let read =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Read);
        let list = crate::jobs_tools::JobsTool::new(runtime, crate::jobs_tools::JobsKind::List);
        assert_eq!(get.spec().parameters["properties"]["tail"]["default"], 256);
        assert_eq!(get.spec().parameters["properties"]["tail"]["maximum"], 384);
        assert_eq!(
            get.spec().parameters["properties"]["wait_ms"]["maximum"],
            1200
        );
        assert_eq!(
            read.spec().parameters["properties"]["wait_ms"]["maximum"],
            0
        );
        assert_eq!(
            read.spec().parameters["properties"]["limit"]["default"],
            512
        );
        assert_eq!(
            read.spec().parameters["properties"]["limit"]["maximum"],
            2048
        );
        assert_eq!(read.result_ceiling_bytes(), 2048);
        assert_eq!(list.spec().parameters["properties"]["limit"]["maximum"], 10);
        assert!(get.spec().description.contains("256 raw bytes combined"));
        get.validate(&json!({"tail":384,"wait_ms":1200})).unwrap();
        assert!(get.validate(&json!({"tail":385})).is_err());
        assert!(get.validate(&json!({"wait_ms":1201})).is_err());
        assert!(read.validate(&json!({"wait_ms":1})).is_err());
        read.validate(&json!({"limit":2048})).unwrap();
        assert!(read.validate(&json!({"limit":2049})).is_err());
    }

    #[test]
    fn compact_get_read_ack_models_expose_bounded_tail_and_quick_finish() {
        let (_, _, runtime, _) = fixture();
        let get =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Get);
        let read =
            crate::jobs_tools::JobsTool::new(runtime.clone(), crate::jobs_tools::JobsKind::Read);
        let ack = crate::jobs_tools::JobsTool::new(runtime, crate::jobs_tools::JobsKind::Ack);
        assert_eq!(get.spec().parameters["properties"]["tail"]["maximum"], 1024);
        assert_eq!(
            read.result_ceiling_bytes(),
            aw_agent_core::tool::MAX_AGENT_TOOL_RESULT_BYTES
        );
        assert_eq!(
            get.result_ceiling_bytes(),
            aw_agent_core::tool::MAX_TOOL_RESULT_BYTES
        );
        get.validate(&json!({})).unwrap();
        get.validate(&json!({"tail":0})).unwrap();
        get.validate(&json!({"tail":1024})).unwrap();
        assert!(get.validate(&json!({"tail":1025})).is_err());
        assert!(get.validate(&json!({"tail":-1})).is_err());
        assert!(get.validate(&json!({"tail_bytes":512})).is_err());
        assert!(read.spec().parameters["properties"]["offset"]
            .get("minimum")
            .is_none());
        read.validate(&json!({"offset":-512})).unwrap();
        read.validate(&json!({"offset":0})).unwrap();
        assert!(read.validate(&json!({"offset":"-512"})).is_err());
        assert_eq!(ack.spec().parameters["required"], json!(["ref"]));
        ack.validate(&json!({"ref":"j:1"})).unwrap();
        ack.validate(&json!({"ref":"j:1","offset":0})).unwrap();
        assert!(ack.validate(&json!({"ref":"j:1","offset":-1})).is_err());
    }

    #[test]
    fn base64_output_fitter_checks_complete_page_before_continuation_overhead() {
        let page = aw_tools::filesystem::job_manager::OutputPage {
            offset: 0,
            bytes: b"x".to_vec(),
            next_offset: None,
            writer_finished: true,
            truncated: false,
            total_bytes: 1,
            retained_bytes: 1,
        };
        assert_eq!(
            JobControl::render_output_page(&page, 0, "base64", 4).unwrap(),
            "eA=="
        );
    }

    #[test]
    fn text_output_fitter_does_not_miss_complete_utf8_page_at_budget_boundary() {
        let mut bytes = vec![b'a'; 510];
        bytes.extend_from_slice("é".as_bytes());
        let page = aw_tools::filesystem::job_manager::OutputPage {
            offset: 0,
            bytes,
            next_offset: None,
            writer_finished: true,
            truncated: false,
            total_bytes: 512,
            retained_bytes: 512,
        };
        let rendered = JobControl::render_output_page(&page, 0, "text", 512).unwrap();
        assert_eq!(rendered.len(), 512, "{rendered:?}");
        assert!(!rendered.contains("more offset="), "{rendered:?}");
        assert!(rendered.ends_with('é'));
    }

    #[test]
    fn batch_and_control_schemas_remain_small_and_strict() {
        let (_root, _agent, runtime, _ctx) = fixture();
        let spec = Batch::new(runtime).spec();
        assert_eq!(spec.name, "batch");
        assert_eq!(
            spec.parameters,
            batch_parameters_with_background(
                aw_tools::filesystem::schema::Tool::GRAPH_NATIVE
                    .into_iter()
                    .map(aw_tools::filesystem::batch::NodeTool::Native)
                    .collect(),
                false,
            )
        );
        assert!(serde_json::to_vec(&spec).unwrap().len() < 2048);
        assert_eq!(job_list_parameters()["properties"]["limit"]["default"], 64);
        assert_eq!(job_id_parameters()["required"][0], "id");
    }
}
