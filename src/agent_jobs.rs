//! Durable AW Agent/Turn projection for the unified jobs_* surface.
//!
//! SessionStore remains the only Agent/history owner and actor_owner::Service
//! remains the only live queue/control owner. This router is a thin resolver and
//! sanitizer; it stores no duplicate Agent lifecycle or transcript state.

use crate::{actor_owner, Error, Result};
use aw_agent_core::{
    tool::{ToolBindingAuthority, ToolBindingSnapshot, ToolCapabilitySnapshot},
    Outcome, PrincipalId, ToolContext, ToolEffect, ToolOutput, ToolSource, ToolStatus,
};
use aw_store::{QueueState, SessionStore};
use aw_store_api::{AgentBindingInfo, SessionBackend, SessionView};
use aw_tools::{
    execution_ref::{AgentRef, ExecutionRef, ExecutionSegment},
    job_event::JobEventDelivery,
};
use aw_tools_adapter::agent_jobs::{
    AgentEventFuture, AgentJobCancel, AgentJobFuture, AgentJobGet, AgentJobListFuture,
    AgentJobListItem, AgentJobListPage, AgentJobListRequest, AgentJobRead, AgentJobRouter,
    AgentJobSend, AgentJobSubscribe, AgentSendInput, AgentSendMode, AgentWakeRecipient,
    FiniteExposureOwner,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const MAX_HISTORY_TURNS: usize = 200;
const MAX_CURSOR_BYTES: usize = 512;

pub trait LiveAgentWake: Send + Sync {
    fn wake(&self) -> Result<()>;
    fn close(&self) -> Result<()> {
        Err(Error::Denied(
            "this live Agent owner does not support host close",
        ))
    }
    fn is_closing(&self) -> bool {
        false
    }
}

#[derive(Clone)]
struct LiveOwner {
    owner: actor_owner::Service,
    wake: Option<Arc<dyn LiveAgentWake>>,
    wake_authority: ToolBindingAuthority,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum WakeAuthority {
    Visible(ToolBindingSnapshot),
    Hidden(ToolCapabilitySnapshot),
}

impl WakeAuthority {
    fn name(&self) -> &str {
        match self {
            Self::Visible(binding) => &binding.name,
            Self::Hidden(binding) => &binding.name,
        }
    }

    fn source(&self) -> &ToolSource {
        match self {
            Self::Visible(binding) => &binding.source,
            Self::Hidden(binding) => &binding.source,
        }
    }

    fn effect(&self) -> &ToolEffect {
        match self {
            Self::Visible(binding) => &binding.effect,
            Self::Hidden(binding) => &binding.effect,
        }
    }

    fn execution_site(&self) -> &str {
        match self {
            Self::Visible(binding) => &binding.execution_site,
            Self::Hidden(binding) => &binding.execution_site,
        }
    }

    fn authority_revision(&self) -> u64 {
        match self {
            Self::Visible(binding) => binding.authority_revision,
            Self::Hidden(binding) => binding.authority_revision,
        }
    }

    fn registry_revision(&self) -> u64 {
        match self {
            Self::Visible(binding) => binding.registry_revision,
            Self::Hidden(_) => 0,
        }
    }
}

#[derive(Clone)]
struct WakeAdmission {
    recipient_ref: String,
    actor: PrincipalId,
    session_id: String,
    membership_epoch: u64,
    audience_epoch: u64,
    owner: bool,
    authority: WakeAuthority,
}

#[derive(Clone)]
pub struct Router {
    store: SessionStore,
    actor: PrincipalId,
    cursor_key: [u8; 32],
    exposure_generation: String,
    finite_capacity: usize,
    owners: Arc<Mutex<HashMap<String, LiveOwner>>>,
    wake_admissions: Arc<Mutex<HashMap<String, WakeAdmission>>>,
}

#[derive(Clone)]
enum Target {
    Agent {
        canonical: String,
        binding: AgentBindingInfo,
    },
    Turn {
        canonical: String,
        binding: AgentBindingInfo,
        turn: u64,
    },
}

impl Router {
    pub fn new(store: SessionStore, actor: PrincipalId, finite_capacity: usize) -> Result<Self> {
        if finite_capacity == 0 || finite_capacity > 8_192 {
            return Err(Error::Limit("invalid Agent jobs finite exposure capacity"));
        }
        let mut cursor_key = [0u8; 32];
        getrandom::fill(&mut cursor_key)
            .map_err(|_| Error::Storage("OS randomness unavailable for Agent history cursors"))?;
        let exposure_generation = format!(
            "jxg_{}",
            cursor_key[..16]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        Ok(Self {
            store,
            actor,
            cursor_key,
            exposure_generation,
            finite_capacity,
            owners: Arc::new(Mutex::new(HashMap::new())),
            wake_admissions: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn register_live_owner(
        &self,
        canonical_agent_ref: &str,
        owner: actor_owner::Service,
        wake_authority: ToolBindingAuthority,
    ) -> Result<()> {
        self.register_live_owner_with_wake(canonical_agent_ref, owner, None, wake_authority)
    }

    pub fn register_live_owner_with_wake(
        &self,
        canonical_agent_ref: &str,
        owner: actor_owner::Service,
        wake: Option<Arc<dyn LiveAgentWake>>,
        wake_authority: ToolBindingAuthority,
    ) -> Result<()> {
        let parsed = ExecutionRef::parse(canonical_agent_ref)
            .map_err(|_| Error::Denied("invalid canonical Agent owner ref"))?;
        let resolved = parsed
            .resolve(None, None)
            .map_err(|_| Error::Denied("invalid canonical Agent owner ref"))?;
        if !resolved.suffix().is_empty() {
            return Err(Error::Denied(
                "live Agent owner registration needs an Agent root ref",
            ));
        }
        self.owners
            .lock()
            .map_err(|_| Error::Storage("Agent jobs owner registry poisoned"))?
            .insert(
                resolved.agent_ref().canonical(),
                LiveOwner {
                    owner,
                    wake,
                    wake_authority,
                },
            );
        Ok(())
    }

    pub fn clear_live_owner(&self, canonical_agent_ref: &str) -> Result<()> {
        self.owners
            .lock()
            .map_err(|_| Error::Storage("Agent jobs owner registry poisoned"))?
            .remove(canonical_agent_ref);
        Ok(())
    }

    fn canonical(info: &AgentBindingInfo) -> String {
        format!(
            "/d:{}/i:{}/p:{}/a:{}",
            info.domain_handle, info.identity_handle, info.project_handle, info.agent_handle
        )
    }

    fn current_agent(ctx: &ToolContext) -> Result<AgentRef> {
        let binding = ctx.execution_owner.as_ref().ok_or(Error::Denied(
            "Agent jobs routing requires a bound execution owner",
        ))?;
        let parsed = ExecutionRef::parse(binding.canonical_ref())
            .map_err(|_| Error::Denied("invalid current Agent execution owner"))?;
        let resolved = parsed
            .resolve(None, None)
            .map_err(|_| Error::Denied("invalid current Agent execution owner"))?;
        if !resolved.suffix().is_empty() {
            return Err(Error::Denied(
                "current execution owner is not an Agent root",
            ));
        }
        Ok(resolved.agent_ref().clone())
    }

    fn resolve(&self, ctx: &ToolContext, reference: &str) -> Result<Option<Target>> {
        if ctx.identity.actor != ctx.actor {
            return Err(Error::Denied(
                "Agent jobs request identity differs from authenticated caller",
            ));
        }
        let current = Self::current_agent(ctx)?;
        let parent = ctx
            .execution_owner
            .as_ref()
            .and_then(|owner| {
                self.store
                    .subagent_for_child_session(&self.actor, owner.session_id())
                    .ok()
                    .flatten()
            })
            .and_then(|relation| {
                self.store
                    .agent_binding(
                        &self.actor,
                        &relation.parent_session_id,
                        &relation.parent_branch_id,
                    )
                    .ok()
            })
            .and_then(|binding| {
                ExecutionRef::parse(&Self::canonical(&binding))
                    .ok()?
                    .resolve(None, None)
                    .ok()
                    .map(|resolved| resolved.agent_ref().clone())
            });
        let parsed = ExecutionRef::parse(reference)
            .map_err(|_| Error::Denied("invalid Agent execution ref"))?;
        let resolved = parsed
            .resolve(Some(&current), parent.as_ref())
            .map_err(|_| Error::Denied("Agent execution ref needs unavailable parent/context"))?;

        // Finite Job refs remain authoritative in JobHost/JobManager.
        if matches!(
            resolved.suffix(),
            [ExecutionSegment::Turn(_), ExecutionSegment::Job(_)]
                | [
                    ExecutionSegment::Turn(_),
                    ExecutionSegment::Job(_),
                    ExecutionSegment::Node(_)
                ]
        ) {
            return Ok(None);
        }

        let canonical_agent = resolved.agent_ref().canonical();
        let binding = self
            .store
            .agent_bindings(&self.actor)?
            .into_iter()
            .find(|binding| Self::canonical(binding) == canonical_agent)
            .ok_or(Error::Denied("Agent ref is unavailable or not authorized"))?;
        self.store.authorize_session_participation(
            &ctx.actor,
            &binding.session_id,
            aw_store::SessionParticipationAction::View,
        )?;

        match resolved.suffix() {
            [] => Ok(Some(Target::Agent {
                canonical: canonical_agent,
                binding,
            })),
            [ExecutionSegment::Turn(turn)] => {
                let turn_id = turn
                    .as_str()
                    .parse::<u64>()
                    .ok()
                    .filter(|value| *value != 0 && value.to_string() == turn.as_str())
                    .ok_or(Error::Denied("Turn handle is not canonical decimal"))?;
                Ok(Some(Target::Turn {
                    canonical: resolved.canonical(),
                    binding,
                    turn: turn_id,
                }))
            }
            _ => Ok(None),
        }
    }

    fn view(&self, viewer: &PrincipalId, binding: &AgentBindingInfo) -> Result<SessionView> {
        self.store
            .read_branch_view(viewer, &binding.session_id, &binding.branch_id)
    }

    fn revision(&self, binding: &AgentBindingInfo) -> Result<u64> {
        self.store
            .agent_public_revision(&self.actor, &binding.session_id, &binding.branch_id)
    }

    fn queue_facts(&self, binding: &AgentBindingInfo) -> Result<Vec<aw_store::ActorQueueFact>> {
        self.store
            .actor_queue_list(&self.actor, &binding.session_id)
    }

    fn agent_state(
        &self,
        binding: &AgentBindingInfo,
        active_run: bool,
        needs_reconciliation: bool,
    ) -> Result<(&'static str, usize)> {
        if let Some(relation) = self
            .store
            .subagent_for_child_session(&self.actor, &binding.session_id)?
        {
            if relation.state != "running" {
                return Ok(("closed", 0));
            }
            if self
                .store
                .subagent_close_requested(&self.actor, &binding.session_id)?
            {
                return Ok(("closing", 0));
            }
        }
        let queue = self.queue_facts(binding)?;
        let queued = queue
            .iter()
            .filter(|fact| {
                matches!(
                    fact.state,
                    QueueState::Pending | QueueState::Claimed | QueueState::Accepted
                )
            })
            .count();
        let controls = self
            .store
            .actor_control_list(&self.actor, &binding.session_id)?;
        let blocked = needs_reconciliation
            || queue.iter().any(|fact| fact.state.needs_inspection())
            || controls.iter().any(|fact| fact.state.needs_inspection());
        let state = if blocked {
            "blocked"
        } else if active_run {
            "running"
        } else {
            "idle"
        };
        Ok((state, queued))
    }

    fn outcome_label(outcome: Outcome) -> &'static str {
        match outcome {
            Outcome::Completed => "ok",
            Outcome::Cancelled => "cancelled",
            Outcome::Incomplete => "unknown",
            Outcome::Failed => "fail",
        }
    }

    fn snapshot(&self, target: &Target) -> Result<(u64, String)> {
        match target {
            Target::Agent { canonical, binding } => {
                let session = self.store.session(&self.actor, &binding.session_id)?;
                let branch = self
                    .store
                    .branches(&self.actor, &binding.session_id)?
                    .into_iter()
                    .find(|branch| branch.id == binding.branch_id)
                    .ok_or(Error::Denied("Agent branch is unavailable"))?;
                let active_turn = self.store.active_turn_for_branch(
                    &self.actor,
                    &binding.session_id,
                    &binding.branch_id,
                )?;
                let needs_reconciliation = if branch.active_run.is_none() {
                    self.view(&self.actor, binding)?
                        .history
                        .needs_reconciliation
                } else {
                    false
                };
                let revision = self.revision(binding)?;
                let (state, queued) =
                    self.agent_state(binding, branch.active_run.is_some(), needs_reconciliation)?;
                let current_turn = active_turn.unwrap_or(session.next_turn);
                Ok((
                    revision,
                    format!(
                        "{canonical} agent {state} rev={revision} current_turn=t:{current_turn} queued={queued}"
                    ),
                ))
            }
            Target::Turn {
                canonical,
                binding,
                turn,
            } => {
                let session = self.store.session(&self.actor, &binding.session_id)?;
                let branch = self
                    .store
                    .branches(&self.actor, &binding.session_id)?
                    .into_iter()
                    .find(|branch| branch.id == binding.branch_id)
                    .ok_or(Error::Denied("Agent branch is unavailable"))?;
                let revision = self.revision(binding)?;
                let active_turn = self.store.active_turn_for_branch(
                    &self.actor,
                    &binding.session_id,
                    &binding.branch_id,
                )?;

                if active_turn == Some(*turn) {
                    let cancelling = self
                        .store
                        .actor_control_list(&self.actor, &binding.session_id)?
                        .iter()
                        .any(|fact| {
                            fact.admission.branch_id == binding.branch_id
                                && fact.admission.turn_id == *turn
                                && fact.admission.action == aw_store::ActorControlAction::CancelTurn
                                && matches!(
                                    fact.state,
                                    aw_store::ActorControlState::Prepared
                                        | aw_store::ActorControlState::Applied
                                        | aw_store::ActorControlState::Unknown
                                )
                        });
                    return Ok((
                        revision,
                        format!(
                            "{canonical} turn {} rev={revision}",
                            if cancelling { "cancelling" } else { "running" }
                        ),
                    ));
                }

                // A queued future Input is observable even while an earlier Turn
                // is active. It is waiting, never a synthetic blocked Turn.
                if *turn == session.next_turn {
                    let waiting = self.queue_facts(binding)?.iter().any(|fact| {
                        matches!(
                            fact.state,
                            QueueState::Pending | QueueState::Claimed | QueueState::Accepted
                        )
                    });
                    if waiting {
                        return Ok((revision, format!("{canonical} turn waiting rev={revision}")));
                    }
                }

                // Transcript history is read only once this branch has no active
                // owner run; active execution ownership is never acquired for
                // observation.
                if branch.active_run.is_none() {
                    let view = self.view(&self.actor, binding)?;
                    if let Some(record) =
                        view.history.turns.iter().find(|record| record.id == *turn)
                    {
                        return Ok((
                            revision,
                            format!(
                                "{canonical} turn settled rev={revision} outcome={}",
                                Self::outcome_label(record.outcome)
                            ),
                        ));
                    }
                }

                Err(Error::Denied("Turn is unavailable or not yet admitted"))
            }
        }
    }

    fn normalized_selection(request: &AgentJobRead) -> Result<String> {
        if let Some(view) = &request.view {
            return Ok(format!("view:{view}"));
        }
        if let Some(filter) = &request.filter {
            return serde_json::to_string(filter)
                .map(|encoded| format!("filter:{encoded}"))
                .map_err(|_| Error::Protocol("cannot encode Agent history filter"));
        }
        Ok("filter:default".into())
    }

    fn digest_hex(value: [u8; 32]) -> String {
        value.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn cursor_mac(&self, payload: &[u8]) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"aw-agent-history-cursor-v1\0");
        hash.update(self.cursor_key);
        hash.update(payload);
        hash.finalize().into()
    }

    fn disclosure_fingerprint(
        &self,
        viewer: &PrincipalId,
        binding: &AgentBindingInfo,
    ) -> Result<[u8; 32]> {
        let participation = self.store.authorize_session_participation(
            viewer,
            &binding.session_id,
            aw_store::SessionParticipationAction::View,
        )?;
        let mut hash = Sha256::new();
        hash.update(b"aw-agent-history-disclosure-v1\0");
        for part in [
            viewer.as_str().as_bytes(),
            binding.session_id.as_bytes(),
            binding.branch_id.as_bytes(),
        ] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part);
        }
        hash.update(participation.membership_epoch.to_be_bytes());
        hash.update(participation.audience_epoch.to_be_bytes());
        hash.update([u8::from(participation.owner)]);
        Ok(hash.finalize().into())
    }

    fn encode_cursor(
        &self,
        canonical: &str,
        selection: &str,
        disclosure: [u8; 32],
        index: usize,
    ) -> Result<String> {
        let payload = serde_json::to_vec(&json!({
            "v": 1u8,
            "r": canonical,
            "s": selection,
            "d": Self::digest_hex(disclosure),
            "i": index,
        }))
        .map_err(|_| Error::Protocol("cannot encode Agent history cursor"))?;
        let mut bytes = payload.clone();
        bytes.extend_from_slice(&self.cursor_mac(&payload));
        let encoded = URL_SAFE_NO_PAD.encode(bytes);
        if encoded.len() > MAX_CURSOR_BYTES {
            return Err(Error::Limit("Agent history cursor exceeds bound"));
        }
        Ok(encoded)
    }

    fn decode_cursor(
        &self,
        encoded: &str,
        canonical: &str,
        selection: &str,
        disclosure: [u8; 32],
    ) -> Result<usize> {
        if encoded.is_empty() || encoded.len() > MAX_CURSOR_BYTES {
            return Err(Error::Denied("invalid Agent history cursor"));
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| Error::Denied("invalid Agent history cursor"))?;
        if bytes.len() <= 32 {
            return Err(Error::Denied("invalid Agent history cursor"));
        }
        let (payload, supplied) = bytes.split_at(bytes.len() - 32);
        if self.cursor_mac(payload).as_slice() != supplied {
            return Err(Error::Denied("stale or unauthorized Agent history cursor"));
        }
        let value: Value = serde_json::from_slice(payload)
            .map_err(|_| Error::Denied("invalid Agent history cursor"))?;
        if value.get("v").and_then(Value::as_u64) != Some(1)
            || value.get("r").and_then(Value::as_str) != Some(canonical)
            || value.get("s").and_then(Value::as_str) != Some(selection)
            || value.get("d").and_then(Value::as_str) != Some(Self::digest_hex(disclosure).as_str())
        {
            return Err(Error::Denied(
                "Agent history cursor selection/disclosure authority changed",
            ));
        }
        value
            .get("i")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(Error::Denied("invalid Agent history cursor"))
    }

    fn bounded_text(&self, ctx: &ToolContext, value: &str) -> Result<ToolOutput> {
        let budget = ctx
            .result_budget_bytes
            .min(aw_agent_core::tool::MAX_TOOL_RESULT_BYTES);
        if value.len() <= budget {
            return Ok(ToolOutput::text(value.to_owned()));
        }
        if budget < 16 {
            return Err(Error::Limit("Agent result cannot fit Tool result budget"));
        }
        let marker = "\n[truncated]";
        let mut end = budget.saturating_sub(marker.len()).min(value.len());
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        let mut content = value[..end].to_owned();
        content.push_str(marker);
        Ok(ToolOutput {
            content,
            is_error: false,
            truncated: true,
        })
    }

    fn turn_records<'a>(
        target: &Target,
        view: &'a SessionView,
    ) -> Result<Vec<&'a aw_agent_core::Turn>> {
        match target {
            Target::Agent { .. } => Ok(view.history.turns.iter().collect()),
            Target::Turn { turn, .. } => view
                .history
                .turns
                .iter()
                .find(|record| record.id == *turn)
                .map(|record| vec![record])
                .ok_or(Error::Denied(
                    "active/uncommitted Turn history is not readable; use jobs_get",
                )),
        }
    }

    fn tool_status(status: &ToolStatus) -> &'static str {
        match status {
            ToolStatus::Running => "running",
            ToolStatus::Succeeded => "succeeded",
            ToolStatus::Failed => "failed",
            ToolStatus::Denied => "denied",
            ToolStatus::Interrupted => "interrupted",
        }
    }

    fn message_records(turn: &aw_agent_core::Turn) -> Vec<Value> {
        let mut records = vec![json!({
            "turn": turn.id,
            "kind": "message",
            "role": "user",
            "content": turn.prompt,
        })];
        if !turn.answer.is_empty() {
            records.push(json!({
                "turn": turn.id,
                "kind": "message",
                "role": "assistant",
                "content": turn.answer,
            }));
        }
        records
    }

    fn tool_call_records(
        turn: &aw_agent_core::Turn,
        tools: Option<&[String]>,
        exclude: bool,
    ) -> Vec<Value> {
        turn.tool_records
            .iter()
            .filter(|record| {
                let matched =
                    tools.is_none_or(|tools| tools.iter().any(|tool| tool == &record.call.name));
                if exclude {
                    !matched
                } else {
                    matched
                }
            })
            .map(|record| {
                json!({
                    "turn": turn.id,
                    "kind": "tool_call",
                    "id": record.call.id,
                    "tool": record.call.name,
                    "status": Self::tool_status(&record.status),
                    // Arguments are intentionally omitted until current resource
                    // disclosure can be revalidated independently.
                    "arguments_redacted": true,
                })
            })
            .collect()
    }

    fn history_records(
        &self,
        target: &Target,
        request: &AgentJobRead,
        view: &SessionView,
    ) -> Result<Vec<Value>> {
        let turns = Self::turn_records(target, view)?;
        let selected_view = request.view.as_deref();
        if matches!(selected_view, Some("tool_outputs" | "trace")) {
            return Err(Error::Denied(
                "historical tool output/trace disclosure requires current resource revalidation",
            ));
        }
        if matches!(selected_view, Some("stdout" | "stderr")) {
            return Err(Error::Denied("Agent history has no stdout/stderr view"));
        }

        if selected_view.is_none()
            && request
                .filter
                .as_ref()
                .is_none_or(|filter| filter == "default")
        {
            let answer = turns.last().map(|turn| turn.answer.as_str()).unwrap_or("");
            return Ok(vec![json!({
                "kind": "result",
                "turn": turns.last().map(|turn| turn.id),
                "content": answer,
            })]);
        }

        if selected_view == Some("result") {
            let answer = turns.last().map(|turn| turn.answer.as_str()).unwrap_or("");
            return Ok(vec![json!({
                "kind": "result",
                "turn": turns.last().map(|turn| turn.id),
                "content": answer,
            })]);
        }

        if selected_view == Some("messages") {
            return Ok(turns.into_iter().flat_map(Self::message_records).collect());
        }
        if selected_view == Some("tool_calls") {
            return Ok(turns
                .into_iter()
                .flat_map(|turn| Self::tool_call_records(turn, None, false))
                .collect());
        }

        let Some(filter) = request.filter.as_ref() else {
            return Err(Error::Denied("invalid Agent history selection"));
        };
        if filter == "none" {
            let mut records = Vec::new();
            for turn in turns {
                records.extend(Self::message_records(turn));
                records.extend(Self::tool_call_records(turn, None, false));
                if !turn.answer.is_empty() {
                    records.push(json!({
                        "turn": turn.id,
                        "kind": "result",
                        "content": turn.answer,
                    }));
                }
            }
            records.push(json!({
                "kind": "projection",
                "redacted": ["tool_outputs", "trace", "tool_arguments"],
            }));
            return Ok(records);
        }

        let object = filter
            .as_object()
            .ok_or(Error::Denied("invalid Agent history filter"))?;
        let exclude = object
            .get("exclude")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let result = object.get("result").and_then(Value::as_bool) == Some(true);
        let messages = object.get("messages").and_then(Value::as_bool) == Some(true);
        let tool_calls = object.get("tool_calls").and_then(Value::as_bool) == Some(true);
        let tool_outputs = object.get("tool_outputs").and_then(Value::as_bool) == Some(true);
        if tool_outputs && !exclude {
            return Err(Error::Denied(
                "historical tool output disclosure requires current resource revalidation",
            ));
        }
        let tools = object.get("tools").and_then(Value::as_array).map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        });

        let mut records = Vec::new();
        for turn in turns {
            if messages != exclude {
                records.extend(Self::message_records(turn));
            }
            if tool_calls {
                records.extend(Self::tool_call_records(turn, tools.as_deref(), exclude));
            } else if exclude {
                records.extend(Self::tool_call_records(turn, None, false));
            }
            if result != exclude && !turn.answer.is_empty() {
                records.push(json!({
                    "turn": turn.id,
                    "kind": "result",
                    "content": turn.answer,
                }));
            }
        }
        Ok(records)
    }

    fn render_history(
        &self,
        ctx: &ToolContext,
        target: &Target,
        request: &AgentJobRead,
    ) -> Result<ToolOutput> {
        if request.wait_ms != 0 || request.cursor.is_some() && matches!(target, Target::Turn { .. })
        {
            return Err(Error::Denied(
                "Agent history wait/cursor is valid only for root historical paging",
            ));
        }
        let view = match target {
            Target::Agent { binding, .. } | Target::Turn { binding, .. } => {
                self.view(&ctx.actor, binding)?
            }
        };
        if view.history.needs_reconciliation {
            return Err(Error::Denied(
                "unreconciled Agent history is not disclosed; inspect the original owner",
            ));
        }
        let canonical = match target {
            Target::Agent { canonical, .. } | Target::Turn { canonical, .. } => canonical,
        };
        let selection = Self::normalized_selection(request)?;
        let binding = match target {
            Target::Agent { binding, .. } | Target::Turn { binding, .. } => binding,
        };
        let disclosure = self.disclosure_fingerprint(&ctx.actor, binding)?;
        let records = self.history_records(target, request, &view)?;

        // Natural/latest result is one value, not a history page.
        if request
            .view
            .as_deref()
            .is_none_or(|value| value == "result")
            && request
                .filter
                .as_ref()
                .is_none_or(|filter| filter == "default")
        {
            let content = records
                .first()
                .and_then(|value| value.get("content"))
                .and_then(Value::as_str)
                .unwrap_or("");
            return self.bounded_text(ctx, content);
        }

        let start = request
            .cursor
            .as_deref()
            .map(|cursor| self.decode_cursor(cursor, canonical, &selection, disclosure))
            .transpose()?
            .unwrap_or(0);
        if start > records.len() {
            return Err(Error::Denied(
                "Agent history cursor is past retained projection",
            ));
        }
        let record_limit = request.limit.clamp(1, MAX_HISTORY_TURNS);
        let budget = ctx
            .result_budget_bytes
            .min(aw_agent_core::tool::MAX_TOOL_RESULT_BYTES);
        let mut lines = Vec::new();
        let mut index = start;
        while index < records.len() && lines.len() < record_limit {
            let line = serde_json::to_string(&records[index])
                .map_err(|_| Error::Protocol("cannot encode Agent history record"))?;
            let candidate_len =
                lines.iter().map(String::len).sum::<usize>() + lines.len() + line.len();
            if candidate_len > budget {
                break;
            }
            lines.push(line);
            index += 1;
        }
        if lines.is_empty() && index < records.len() {
            return Err(Error::Limit(
                "one Agent history record cannot fit Tool result budget",
            ));
        }
        if index < records.len() {
            let cursor = self.encode_cursor(canonical, &selection, disclosure, index)?;
            let marker = format!("more cursor={cursor}");
            let candidate_len =
                lines.iter().map(String::len).sum::<usize>() + lines.len() + marker.len();
            if candidate_len > budget {
                return Err(Error::Limit(
                    "Agent history continuation cannot fit Tool result budget",
                ));
            }
            lines.push(marker);
        }
        Ok(ToolOutput::text(lines.join("\n")))
    }

    fn finite_binding(&self, owner: &FiniteExposureOwner) -> Result<AgentBindingInfo> {
        if owner.actor != self.actor.as_str() {
            return Err(Error::Denied(
                "finite Job exposure principal differs from router owner",
            ));
        }
        self.store
            .agent_bindings(&self.actor)?
            .into_iter()
            .find(|binding| {
                Self::canonical(binding) == owner.canonical_agent_ref
                    && binding.session_id == owner.session_id
                    && binding.branch_id == owner.branch_id
            })
            .ok_or(Error::Denied(
                "finite Job exposure owner is unavailable or unauthorized",
            ))
    }

    fn list_scope_fingerprint(&self, binding: &AgentBindingInfo) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"aw-agent-job-list-scope-v1\0");
        for part in [
            self.exposure_generation.as_bytes(),
            binding.domain_handle.as_bytes(),
            binding.identity_handle.as_bytes(),
            binding.project_handle.as_bytes(),
        ] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part);
        }
        hash.finalize().into()
    }

    fn list_cursor_mac(&self, payload: &[u8]) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"aw-agent-job-list-cursor-v1\0");
        hash.update(self.cursor_key);
        hash.update(payload);
        hash.finalize().into()
    }

    fn encode_list_cursor(
        &self,
        binding: &AgentBindingInfo,
        authority: [u8; 32],
        position: u64,
    ) -> Result<String> {
        if position == 0 || authority == [0; 32] {
            return Err(Error::Denied("invalid mixed Job-list cursor state"));
        }
        let mut payload = Vec::with_capacity(1 + 32 + 32 + 8);
        payload.push(1);
        payload.extend_from_slice(&self.list_scope_fingerprint(binding));
        payload.extend_from_slice(&authority);
        payload.extend_from_slice(&position.to_be_bytes());
        let mac = self.list_cursor_mac(&payload);
        payload.extend_from_slice(&mac);
        let encoded = URL_SAFE_NO_PAD.encode(payload);
        if encoded.len() > MAX_CURSOR_BYTES {
            return Err(Error::Limit("mixed Job-list cursor exceeds bound"));
        }
        Ok(encoded)
    }

    fn decode_list_cursor(
        &self,
        encoded: &str,
        binding: &AgentBindingInfo,
        authority: [u8; 32],
    ) -> Result<u64> {
        if encoded.is_empty() || encoded.len() > MAX_CURSOR_BYTES {
            return Err(Error::Denied("invalid mixed Job-list cursor"));
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| Error::Denied("invalid mixed Job-list cursor"))?;
        const PAYLOAD: usize = 1 + 32 + 32 + 8;
        if bytes.len() != PAYLOAD + 32 {
            return Err(Error::Denied("invalid mixed Job-list cursor"));
        }
        let (payload, supplied) = bytes.split_at(PAYLOAD);
        if self.list_cursor_mac(payload).as_slice() != supplied {
            return Err(Error::Denied("stale or unauthorized mixed Job-list cursor"));
        }
        if payload[0] != 1
            || payload[1..33] != self.list_scope_fingerprint(binding)
            || payload[33..65] != authority
        {
            return Err(Error::Denied(
                "mixed Job-list cursor scope or authority changed",
            ));
        }
        let position = u64::from_be_bytes(
            payload[65..73]
                .try_into()
                .map_err(|_| Error::Denied("invalid mixed Job-list cursor"))?,
        );
        if position == 0 {
            return Err(Error::Denied("invalid mixed Job-list cursor position"));
        }
        Ok(position)
    }

    fn authorize_parent_agent_control(
        &self,
        ctx: &ToolContext,
        binding: &AgentBindingInfo,
    ) -> Result<aw_store::SubagentInfo> {
        let scope = ctx
            .scope
            .as_ref()
            .ok_or(Error::Denied("Agent control requires durable caller scope"))?;
        let caller = ctx
            .execution_owner
            .as_ref()
            .ok_or(Error::Denied("Agent control lacks caller identity"))?;
        if ctx.cancellation.is_cancelled()
            || ctx.turn_id == 0
            || ctx.call_id.is_empty()
            || ctx.call_id.len() > 256
            || ctx.actor != self.actor
            || ctx.identity.actor != self.actor
            || ctx.source != aw_agent_core::ToolSource::trusted_host()
            || ctx.effect != aw_agent_core::ToolEffect::Control
            || caller.session_id() != scope.session_id
            || caller.branch_id() != scope.branch_id
        {
            return Err(Error::Denied("invalid parent Agent control context"));
        }
        let relation = self
            .store
            .subagent_for_child_session(&self.actor, &binding.session_id)?
            .ok_or(Error::Denied(
                "target Agent has no delegated parent relation",
            ))?;
        if relation.parent_session_id != scope.session_id
            || relation.parent_branch_id != scope.branch_id
            || self.store.active_turn_for_branch(
                &self.actor,
                &scope.session_id,
                &scope.branch_id,
            )? != Some(ctx.turn_id)
        {
            return Err(Error::Denied(
                "Agent control caller is not the active direct parent",
            ));
        }
        Ok(relation)
    }

    fn wake_authority_ref(
        recipient_ref: &str,
        actor: &PrincipalId,
        session_id: &str,
        membership_epoch: u64,
        audience_epoch: u64,
        owner: bool,
        authority: &WakeAuthority,
    ) -> String {
        let source = authority.source();
        let mut hash = Sha256::new();
        hash.update(b"aw-agent-wake-recipient-admission-v2\0");
        for part in [
            recipient_ref.as_bytes(),
            actor.as_str().as_bytes(),
            session_id.as_bytes(),
            authority.name().as_bytes(),
            source.kind_label().as_bytes(),
            source.id.as_bytes(),
            authority.execution_site().as_bytes(),
        ] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part);
        }
        hash.update(membership_epoch.to_be_bytes());
        hash.update(audience_epoch.to_be_bytes());
        hash.update([u8::from(owner)]);
        hash.update(authority.registry_revision().to_be_bytes());
        hash.update(authority.authority_revision().to_be_bytes());
        hash.update(source.epoch.to_be_bytes());
        hash.update([match authority.effect() {
            ToolEffect::Read => 0,
            ToolEffect::Write => 1,
            ToolEffect::Process => 2,
            ToolEffect::Control => 3,
            ToolEffect::Unknown => 4,
        }]);
        format!("jwa_{:x}", hash.finalize())
    }

    fn mint_wake_recipient(&self, ctx: &ToolContext) -> Result<Option<AgentWakeRecipient>> {
        if ctx.identity.actor != ctx.actor {
            return Err(Error::Denied(
                "wake recipient identity differs from authenticated caller",
            ));
        }
        let execution = ctx
            .execution_owner
            .as_ref()
            .ok_or(Error::Denied("wake recipient requires a bound Agent"))?;
        let canonical = Self::current_agent(ctx)?.canonical();
        let live = self
            .owners
            .lock()
            .map_err(|_| Error::Storage("Agent jobs owner registry poisoned"))?
            .get(&canonical)
            .cloned();
        let Some(live) = live else {
            return Ok(None);
        };
        if live.wake.is_none() {
            return Ok(None);
        }
        let participation = self.store.authorize_session_participation(
            &ctx.actor,
            execution.session_id(),
            aw_store::SessionParticipationAction::Control,
        )?;
        let authority = match live
            .wake_authority
            .permitted_binding(&ctx.actor, "jobs_subscribe")
        {
            Ok(binding) => WakeAuthority::Visible(binding),
            Err(_) => WakeAuthority::Hidden(live.wake_authority.permitted_capability_binding(
                &ctx.actor,
                "jobs_subscribe",
                &ToolSource::trusted_host(),
                &ToolEffect::Control,
            )?),
        };
        if authority.effect() != &ToolEffect::Control {
            return Err(Error::Denied(
                "jobs_subscribe recipient authority is not Control",
            ));
        }
        let authority_ref = Self::wake_authority_ref(
            &canonical,
            &ctx.actor,
            execution.session_id(),
            participation.membership_epoch,
            participation.audience_epoch,
            participation.owner,
            &authority,
        );
        let admission = WakeAdmission {
            recipient_ref: canonical.clone(),
            actor: ctx.actor.clone(),
            session_id: execution.session_id().to_owned(),
            membership_epoch: participation.membership_epoch,
            audience_epoch: participation.audience_epoch,
            owner: participation.owner,
            authority,
        };
        let mut admissions = self
            .wake_admissions
            .lock()
            .map_err(|_| Error::Storage("Agent wake admission registry poisoned"))?;
        if !admissions.contains_key(&authority_ref)
            && admissions.len() >= self.finite_capacity.clamp(64, 8_192)
        {
            return Err(Error::Limit("Agent wake admission registry is full"));
        }
        admissions.insert(authority_ref.clone(), admission);
        Ok(Some(AgentWakeRecipient {
            canonical_ref: canonical,
            authority_ref,
        }))
    }

    fn revalidate_wake_admission(
        &self,
        recipient: &AgentWakeRecipient,
        expected_actor: Option<&PrincipalId>,
    ) -> Result<WakeAdmission> {
        let admission = self
            .wake_admissions
            .lock()
            .map_err(|_| Error::Storage("Agent wake admission registry poisoned"))?
            .get(&recipient.authority_ref)
            .cloned()
            .ok_or(Error::Denied(
                "wake recipient admission is stale or unknown",
            ))?;
        if admission.recipient_ref != recipient.canonical_ref
            || expected_actor.is_some_and(|actor| actor != &admission.actor)
        {
            return Err(Error::Denied("wake recipient admission identity changed"));
        }
        let participation = self.store.authorize_session_participation(
            &admission.actor,
            &admission.session_id,
            aw_store::SessionParticipationAction::Control,
        )?;
        if participation.membership_epoch != admission.membership_epoch
            || participation.audience_epoch != admission.audience_epoch
            || participation.owner != admission.owner
        {
            return Err(Error::Denied(
                "wake participant membership/audience changed after registration",
            ));
        }
        let live = self
            .owners
            .lock()
            .map_err(|_| Error::Storage("Agent jobs owner registry poisoned"))?
            .get(&admission.recipient_ref)
            .cloned()
            .ok_or(Error::Denied("wake recipient is no longer live"))?;
        if live.wake.is_none() {
            return Err(Error::Denied(
                "wake recipient no longer owns bounded automatic work",
            ));
        }
        match &admission.authority {
            WakeAuthority::Visible(binding) => {
                let current = live
                    .wake_authority
                    .permitted_binding(&admission.actor, "jobs_subscribe")?;
                if &current != binding {
                    return Err(Error::Denied(
                        "jobs_subscribe grant changed after wake registration",
                    ));
                }
            }
            WakeAuthority::Hidden(binding) => live
                .wake_authority
                .revalidate_capability(&admission.actor, binding)?,
        }
        Ok(admission)
    }

    fn live_owner(&self, canonical: &str) -> Result<LiveOwner> {
        self.owners
            .lock()
            .map_err(|_| Error::Storage("Agent jobs owner registry poisoned"))?
            .get(canonical)
            .cloned()
            .ok_or(Error::Conflict(
                "Agent is not attached to a live queue/control owner",
            ))
    }
}

impl AgentJobRouter for Router {
    fn resolve_control_ref(&self, ctx: &ToolContext, reference: &str) -> Result<Option<String>> {
        Ok(self.resolve(ctx, reference)?.map(|target| match target {
            Target::Agent { canonical, .. } | Target::Turn { canonical, .. } => canonical,
        }))
    }

    fn wake_recipient(&self, ctx: &ToolContext) -> Result<Option<AgentWakeRecipient>> {
        self.mint_wake_recipient(ctx)
    }

    fn revalidate_wake_recipient(
        &self,
        ctx: &ToolContext,
        recipient: &AgentWakeRecipient,
    ) -> Result<()> {
        let canonical = Self::current_agent(ctx)?.canonical();
        if canonical != recipient.canonical_ref {
            return Err(Error::Denied("jobs_subscribe replay recipient changed"));
        }
        self.revalidate_wake_admission(recipient, Some(&ctx.actor))
            .map(|_| ())
    }

    fn reserve_finite(
        &self,
        owner: &FiniteExposureOwner,
        canonical_ref: &str,
        capacity: usize,
    ) -> Result<()> {
        let binding = self.finite_binding(owner)?;
        self.store.finite_job_exposure_reserve(
            &self.actor,
            &binding,
            &self.exposure_generation,
            canonical_ref,
            capacity.min(self.finite_capacity),
        )?;
        Ok(())
    }

    fn publish_finite(&self, owner: &FiniteExposureOwner, canonical_ref: &str) -> Result<()> {
        let binding = self.finite_binding(owner)?;
        self.store.finite_job_exposure_publish(
            &self.actor,
            &binding,
            &self.exposure_generation,
            canonical_ref,
        )?;
        Ok(())
    }

    fn release_unpublished_finite(
        &self,
        owner: &FiniteExposureOwner,
        canonical_ref: &str,
    ) -> Result<()> {
        let binding = self.finite_binding(owner)?;
        self.store.finite_job_exposure_release_unpublished(
            &self.actor,
            &binding,
            &self.exposure_generation,
            canonical_ref,
        )
    }

    fn list<'a>(
        &'a self,
        ctx: &'a ToolContext,
        request: AgentJobListRequest,
    ) -> AgentJobListFuture<'a> {
        Box::pin(async move {
            if request.limit == 0 || request.limit > 256 || request.authority_fingerprint == [0; 32]
            {
                return Err(Error::Denied("invalid mixed Job-list request"));
            }
            if ctx.actor != self.actor || ctx.identity.actor != self.actor {
                return Err(Error::Denied(
                    "mixed Job-list principal differs from durable Session owner",
                ));
            }
            let current = Self::current_agent(ctx)?;
            let current_ref = current.canonical();
            let bindings = self.store.agent_bindings(&self.actor)?;
            let current_binding = bindings
                .iter()
                .find(|binding| Self::canonical(binding) == current_ref)
                .cloned()
                .ok_or(Error::Denied(
                    "current Agent is unavailable from durable Job-list scope",
                ))?;

            let mut roots = Vec::<(u64, String)>::new();
            for binding in bindings.into_iter().filter(|binding| {
                binding.domain_handle == current_binding.domain_handle
                    && binding.identity_handle == current_binding.identity_handle
                    && binding.project_handle == current_binding.project_handle
            }) {
                let position = self
                    .store
                    .agent_job_exposure_position(&self.actor, &binding)?;
                roots.push((position, Self::canonical(&binding)));
            }
            for finite in self.store.finite_job_exposures(
                &self.actor,
                &current_binding,
                &self.exposure_generation,
                self.finite_capacity,
            )? {
                roots.push((finite.position, finite.canonical_ref));
            }
            roots.sort_by(|(position_a, ref_a), (position_b, ref_b)| {
                position_b.cmp(position_a).then_with(|| ref_a.cmp(ref_b))
            });
            roots.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

            let cursor = request
                .cursor
                .as_deref()
                .map(|cursor| {
                    self.decode_list_cursor(cursor, &current_binding, request.authority_fingerprint)
                })
                .transpose()?;
            let mut eligible = roots.into_iter().filter(|(position, _)| {
                cursor.is_none_or(|cursor_position| *position < cursor_position)
            });
            let mut page = eligible
                .by_ref()
                .take(request.limit.saturating_add(1))
                .collect::<Vec<_>>();
            let has_more = page.len() > request.limit;
            if has_more {
                page.truncate(request.limit);
            }
            let items = page
                .iter()
                .map(|(position, reference)| {
                    Ok(AgentJobListItem {
                        reference: reference.clone(),
                        after_cursor: self.encode_list_cursor(
                            &current_binding,
                            request.authority_fingerprint,
                            *position,
                        )?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let next_cursor = if has_more {
                items.last().map(|item| item.after_cursor.clone())
            } else {
                None
            };
            Ok(Some(AgentJobListPage { items, next_cursor }))
        })
    }

    fn get<'a>(&'a self, ctx: &'a ToolContext, request: AgentJobGet) -> AgentJobFuture<'a> {
        Box::pin(async move {
            let Some(target) = self.resolve(ctx, &request.reference)? else {
                return Ok(None);
            };
            let started = Instant::now();
            let wait = Duration::from_millis(request.wait_ms);
            let mut snapshot = self.snapshot(&target)?;
            let baseline = request.revision.unwrap_or(snapshot.0);
            while snapshot.0 == baseline && request.wait_ms != 0 && started.elapsed() < wait {
                tokio::time::sleep(
                    Duration::from_millis(10).min(wait.saturating_sub(started.elapsed())),
                )
                .await;
                snapshot = self.snapshot(&target)?;
            }
            Ok(Some(self.bounded_text(ctx, &snapshot.1)?))
        })
    }

    fn read<'a>(&'a self, ctx: &'a ToolContext, request: AgentJobRead) -> AgentJobFuture<'a> {
        Box::pin(async move {
            let Some(target) = self.resolve(ctx, &request.reference)? else {
                return Ok(None);
            };
            Ok(Some(self.render_history(ctx, &target, &request)?))
        })
    }

    fn send<'a>(&'a self, ctx: &'a ToolContext, request: AgentJobSend) -> AgentJobFuture<'a> {
        Box::pin(async move {
            let Some(target) = self.resolve(ctx, &request.reference)? else {
                return Ok(None);
            };
            let AgentSendInput::Text(text) = request.input else {
                return Err(Error::Denied("Agent jobs_send does not accept EOF"));
            };
            match target {
                Target::Agent { canonical, binding } => {
                    if !matches!(request.mode, AgentSendMode::Auto | AgentSendMode::Queue) {
                        return Err(Error::Denied(
                            "Agent-root jobs_send supports auto/queue; target an active Turn for steer/interrupt",
                        ));
                    }
                    let live = self.live_owner(&canonical)?;
                    if live.wake.as_ref().is_some_and(|wake| wake.is_closing()) {
                        return Err(Error::Conflict(
                            "Agent is closing and accepts no future input",
                        ));
                    }
                    let caller = Self::current_agent(ctx)?.canonical();
                    let fact = if caller == canonical {
                        live.owner
                            .jobs_queue_text(ctx, &canonical, request.revision, &text)?
                    } else {
                        live.owner.jobs_queue_text_from_parent(
                            ctx,
                            &canonical,
                            request.revision,
                            &text,
                        )?
                    };
                    if let Some(wake) = &live.wake {
                        wake.wake()?;
                    }
                    let revision = self.revision(&binding)?;
                    Ok(Some(ToolOutput::text(format!(
                        "{canonical} agent waiting rev={revision} queued={} queue_ref={}",
                        fact.revision, fact.request_id
                    ))))
                }
                Target::Turn {
                    canonical, binding, ..
                } => {
                    let agent_ref = Self::canonical(&binding);
                    let live = self.live_owner(&agent_ref)?;
                    match request.mode {
                        AgentSendMode::Auto | AgentSendMode::Steer => {
                            let fact = live.owner.jobs_steer_text(
                                ctx,
                                &canonical,
                                request.revision,
                                &text,
                            )?;
                            let revision = self.revision(&binding)?;
                            Ok(Some(ToolOutput::text(format!(
                                "{canonical} turn running rev={revision} steer={} segment={}",
                                fact.request.request_id,
                                fact.segment_ordinal.unwrap_or(0),
                            ))))
                        }
                        AgentSendMode::Queue => {
                            let fact = live.owner.jobs_queue_text(
                                ctx,
                                &agent_ref,
                                request.revision,
                                &text,
                            )?;
                            if let Some(wake) = &live.wake {
                                wake.wake()?;
                            }
                            let revision = self.revision(&binding)?;
                            Ok(Some(ToolOutput::text(format!(
                                "{agent_ref} agent waiting rev={revision} queued={} queue_ref={}",
                                fact.revision, fact.request_id
                            ))))
                        }
                        AgentSendMode::Interrupt => {
                            let fact = live.owner.jobs_interrupt_text(
                                ctx,
                                &canonical,
                                request.revision,
                                &text,
                            )?;
                            let revision = self.revision(&binding)?;
                            Ok(Some(ToolOutput::text(format!(
                                "{canonical} turn running rev={revision} interrupt={} segment={}",
                                fact.request.request_id,
                                fact.segment_ordinal.unwrap_or(0),
                            ))))
                        }
                    }
                }
            }
        })
    }

    fn admit_event<'a>(&'a self, delivery: JobEventDelivery) -> AgentEventFuture<'a> {
        Box::pin(async move {
            delivery
                .validate()
                .map_err(|_| Error::Denied("invalid Job event delivery"))?;
            let recipient = AgentWakeRecipient {
                canonical_ref: delivery.recipient_ref.clone(),
                authority_ref: delivery.recipient_authority_ref.clone(),
            };
            let admission = self.revalidate_wake_admission(&recipient, None)?;
            let live = self
                .owners
                .lock()
                .map_err(|_| Error::Storage("Agent jobs owner registry poisoned"))?
                .get(&admission.recipient_ref)
                .cloned()
                .ok_or(Error::Denied("Job wake recipient is not live"))?;
            let wake = live
                .wake
                .as_ref()
                .ok_or(Error::Denied(
                    "recipient Agent has no bounded automatic-work wake owner",
                ))?
                .clone();
            let (_fact, newly_admitted) = live.owner.admit_job_event(&delivery)?;
            if newly_admitted {
                wake.wake()?;
            }
            Ok(())
        })
    }

    fn subscribe<'a>(
        &'a self,
        _ctx: &'a ToolContext,
        _request: AgentJobSubscribe,
    ) -> AgentJobFuture<'a> {
        Box::pin(async {
            Err(Error::Denied(
                "Agent-source subscriptions require durable source event ownership",
            ))
        })
    }

    fn cancel<'a>(&'a self, ctx: &'a ToolContext, request: AgentJobCancel) -> AgentJobFuture<'a> {
        Box::pin(async move {
            let Some(target) = self.resolve(ctx, &request.reference)? else {
                return Ok(None);
            };
            match target {
                Target::Agent { canonical, binding } => {
                    let relation = match self
                        .store
                        .subagent_for_child_session(&self.actor, &binding.session_id)?
                    {
                        Some(relation) => relation,
                        None => {
                            return Err(Error::Denied(
                                "root/default Agent close is a host lifecycle operation",
                            ))
                        }
                    };
                    let revision = self.revision(&binding)?;
                    if relation.state != "running" {
                        return Ok(Some(ToolOutput::text(format!(
                            "{canonical} agent closed rev={revision} unchanged"
                        ))));
                    }
                    self.authorize_parent_agent_control(ctx, &binding)?;
                    if request
                        .revision
                        .is_some_and(|expected| expected != revision)
                    {
                        return Err(Error::Conflict("stale Agent revision"));
                    }
                    let scope = ctx.scope.as_ref().expect("authorized parent scope");
                    let mut hash = Sha256::new();
                    hash.update(b"aw-jobs-close-related-agent-v1\0");
                    for part in [
                        scope.session_id.as_bytes(),
                        scope.branch_id.as_bytes(),
                        &ctx.turn_id.to_be_bytes(),
                        ctx.call_id.as_bytes(),
                        canonical.as_bytes(),
                    ] {
                        hash.update((part.len() as u64).to_be_bytes());
                        hash.update(part);
                    }
                    let request_id = format!("ac_{:x}", hash.finalize());
                    self.store.request_subagent_close(
                        &self.actor,
                        &binding.session_id,
                        &request_id,
                    )?;
                    let live = self.live_owner(&canonical)?;
                    let wake = live.wake.as_ref().ok_or(Error::Denied(
                        "delegated Agent has no live close controller",
                    ))?;
                    wake.close()?;
                    let revision = self.revision(&binding)?;
                    Ok(Some(ToolOutput::text(format!(
                        "{canonical} agent closing rev={revision} control={request_id}"
                    ))))
                }
                Target::Turn {
                    canonical,
                    binding,
                    turn,
                } => {
                    let agent_ref = Self::canonical(&binding);
                    let live = self.live_owner(&agent_ref)?;
                    let fact =
                        live.owner
                            .jobs_cancel_turn(ctx, &canonical, turn, request.revision)?;
                    let revision = self.revision(&binding)?;
                    let state = match fact.state {
                        aw_store::ActorControlState::Applied => "cancelling",
                        aw_store::ActorControlState::Rejected => "unchanged",
                        aw_store::ActorControlState::Prepared
                        | aw_store::ActorControlState::Unknown
                        | aw_store::ActorControlState::Inspected => {
                            return Err(Error::Conflict(
                                "jobs_cancel control outcome is unresolved",
                            ))
                        }
                    };
                    Ok(Some(ToolOutput::text(format!(
                        "{canonical} turn {state} rev={revision} control={}",
                        fact.admission.request_id
                    ))))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::{
        AccountOwner, Agent, AgentOptions, BillingOwner, CompletionFuture, CompletionMeta,
        CredentialCustody, DeltaSink, InvocationIdentity, InvocationScope, Message, Provider, Tool,
        ToolEffect, ToolFuture, ToolSource, ToolSpec, Usage,
    };
    use aw_store::SessionBinding;
    use std::{
        future::pending,
        path::PathBuf,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };

    struct HistoryProvider;
    impl Provider for HistoryProvider {
        fn stream<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                sink(&"history-result-".repeat(24))?;
                Ok(CompletionMeta {
                    finish_reason: "stop".into(),
                    usage: Usage {
                        input_tokens: Some(3),
                        output_tokens: Some(5),
                    },
                })
            })
        }
    }

    fn binding() -> SessionBinding {
        SessionBinding {
            working_directory: PathBuf::from("/tmp"),
            tool_root: PathBuf::from("/tmp"),
            system_prompt: "system".into(),
            profile_hint: "fixture".into(),
            model_hint: "model".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    fn context(
        actor: &PrincipalId,
        binding: &AgentBindingInfo,
        authority_effect: ToolEffect,
    ) -> ToolContext {
        let owner = aw_agent_core::AgentExecutionBinding::from_trusted_host(
            Router::canonical(binding),
            binding.session_id.clone(),
            binding.branch_id.clone(),
        )
        .unwrap();
        ToolContext {
            actor: actor.clone(),
            turn_id: 1,
            call_id: "list-call".into(),
            source: ToolSource::trusted_host(),
            effect: authority_effect,
            execution_site: "local".into(),
            cancellation: aw_agent_core::CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: InvocationIdentity {
                actor: actor.clone(),
                account_id: "fixture".into(),
                account_owner: AccountOwner::User(actor.clone()),
                payer: BillingOwner::User(actor.clone()),
                credential_custody: CredentialCustody::UserDevice("fixture".into()),
                credential_version: None,
                inference_executor: "fixture".into(),
                model: "fixture".into(),
                grant_revision: 1,
            },
            scope: Some(InvocationScope {
                session_id: binding.session_id.clone(),
                branch_id: binding.branch_id.clone(),
            }),
            execution_owner: Some(owner),
            execution_scope: None,
        }
    }

    struct SubscribeTool;
    impl Tool for SubscribeTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "jobs_subscribe".into(),
                description: "fixture subscription control".into(),
                parameters: json!({
                    "type":"object",
                    "properties":{},
                    "additionalProperties":false
                }),
                effect: ToolEffect::Control,
            }
        }

        fn validate(&self, arguments: &Value) -> Result<()> {
            if arguments
                .as_object()
                .is_some_and(|object| object.is_empty())
            {
                Ok(())
            } else {
                Err(Error::Denied(
                    "fixture subscription tool takes no arguments",
                ))
            }
        }

        fn execute<'a>(&'a self, _ctx: ToolContext, _arguments: &'a Value) -> ToolFuture<'a> {
            Box::pin(async {
                Err(Error::Protocol(
                    "fixture jobs_subscribe executor must not be called directly",
                ))
            })
        }
    }

    #[derive(Default)]
    struct CountingWake {
        calls: AtomicUsize,
    }
    impl LiveAgentWake for CountingWake {
        fn wake(&self) -> Result<()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn revoked_shared_participant_cannot_receive_committed_wake_or_reuse_history_cursor() {
        use aw_tools::job_event::{
            EventRule, EventRuleSet, JobEventKind, JobWakeMode, SourceEventBook,
        };

        let alice = PrincipalId::new("internal:fixture:e9-alice").unwrap();
        let bob = PrincipalId::new("internal:fixture:e9-bob").unwrap();
        let store = SessionStore::in_memory().unwrap();
        let (session, branch) = store.create(&alice, "E9 shared Agent", binding()).unwrap();
        let (view, handle) = store.activate(&alice, &session.id, &branch.id).unwrap();
        let mut agent = Agent::builder(
            Arc::new(HistoryProvider),
            AgentOptions::new("fixture", "model"),
        )
        .account(
            aw_agent_core::AccountDescriptor::personal(
                "fixture",
                alice.clone(),
                "local",
                ["model".into()],
            ),
            alice.clone(),
            "local",
        )
        .build()
        .unwrap();
        agent
            .load_history(view.history, Some(Arc::new(handle)))
            .unwrap();
        let binding_info = store
            .agent_binding(&alice, &session.id, &branch.id)
            .unwrap();
        agent
            .bind_execution_owner(
                aw_agent_core::AgentExecutionBinding::from_trusted_host(
                    Router::canonical(&binding_info),
                    binding_info.session_id.clone(),
                    binding_info.branch_id.clone(),
                )
                .unwrap(),
            )
            .unwrap();
        agent
            .prompt("first private-ish prompt", pending::<()>(), |_| Ok(()))
            .await
            .unwrap();
        agent
            .prompt("second public prompt", pending::<()>(), |_| Ok(()))
            .await
            .unwrap();

        agent.register_host_tool(Arc::new(SubscribeTool)).unwrap();
        agent
            .permit_registered_host_tool(bob.clone(), "jobs_subscribe")
            .unwrap();
        store
            .set_session_participation(
                &alice,
                &session.id,
                &bob,
                aw_store::SessionParticipationPermissions {
                    view: true,
                    control: true,
                    ..Default::default()
                },
            )
            .unwrap();

        let owner = crate::actor_owner::Service::attach_for_agent(
            store.clone(),
            &session.id,
            &branch.id,
            &agent,
        )
        .unwrap();
        let wake = Arc::new(CountingWake::default());
        let router = Router::new(store.clone(), alice.clone(), 8).unwrap();
        router
            .register_live_owner_with_wake(
                &Router::canonical(&binding_info),
                owner,
                Some(wake.clone()),
                agent.tool_binding_authority(),
            )
            .unwrap();

        let read_ctx = context(&bob, &binding_info, ToolEffect::Read);
        let filter = json!({"messages":true});
        let first_page = router
            .read(
                &read_ctx,
                AgentJobRead {
                    reference: ".".into(),
                    view: None,
                    filter: Some(filter.clone()),
                    cursor: None,
                    limit: 1,
                    wait_ms: 0,
                },
            )
            .await
            .unwrap()
            .unwrap();
        let cursor = first_page
            .content
            .lines()
            .find_map(|line| line.strip_prefix("more cursor="))
            .expect("shared transcript page must expose an opaque continuation")
            .to_owned();

        let subscribe_ctx = context(&bob, &binding_info, ToolEffect::Control);
        let recipient = router
            .wake_recipient(&subscribe_ctx)
            .unwrap()
            .expect("shared participant has explicit bounded wake authority");
        assert_eq!(recipient.canonical_ref, Router::canonical(&binding_info));

        let mut source = SourceEventBook::default();
        let rules =
            EventRuleSet::new([EventRule::new(JobEventKind::Result, JobWakeMode::Queue)]).unwrap();
        let update = source
            .replace(&recipient.canonical_ref, &recipient.authority_ref, rules)
            .unwrap();
        let delivery = source
            .commit(
                "/d:0/i:0/p:0/a:source/t:1/j:factual",
                &[JobEventKind::Result],
            )
            .unwrap()
            .into_iter()
            .next()
            .expect("source event must be factually committed");
        assert_eq!(delivery.event_rev, 1);
        assert_eq!(delivery.subscription_generation, update.generation);

        store
            .revoke_session_participation(&alice, &session.id, &bob)
            .unwrap();

        assert!(
            router.admit_event(delivery.clone()).await.is_err(),
            "revoked participant cannot admit future wake Input"
        );
        assert_eq!(wake.calls.load(Ordering::SeqCst), 0);
        assert!(
            store
                .actor_queue_list(&alice, &session.id)
                .unwrap()
                .is_empty(),
            "receiver rejection must happen before queue custody"
        );
        assert!(
            router
                .read(
                    &read_ctx,
                    AgentJobRead {
                        reference: ".".into(),
                        view: None,
                        filter: Some(filter.clone()),
                        cursor: Some(cursor.clone()),
                        limit: 1,
                        wait_ms: 0,
                    },
                )
                .await
                .is_err(),
            "revoked participant cannot disclose transcript history"
        );

        let regrant = store
            .set_session_participation(
                &alice,
                &session.id,
                &bob,
                aw_store::SessionParticipationPermissions {
                    view: true,
                    control: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(regrant.membership_epoch > 1);
        assert!(
            router.admit_event(delivery.clone()).await.is_err(),
            "re-grant must not revive an old wake admission generation"
        );
        assert!(
            router
                .read(
                    &read_ctx,
                    AgentJobRead {
                        reference: ".".into(),
                        view: None,
                        filter: Some(filter),
                        cursor: Some(cursor),
                        limit: 1,
                        wait_ms: 0,
                    },
                )
                .await
                .is_err(),
            "re-grant must not revive a pre-revocation transcript cursor"
        );
        assert_eq!(
            delivery.event_rev, 1,
            "receiver revocation must never rewrite the factual source event"
        );
        assert_eq!(
            source.generation_for(&recipient.canonical_ref),
            Some(update.generation),
            "source-owned subscription record remains factual even when delivery is denied"
        );
    }

    #[tokio::test]
    async fn mixed_list_pages_interleave_agent_and_finite_positions_and_fence_cursor_authority() {
        let actor = PrincipalId::new("local").unwrap();
        let store = SessionStore::in_memory().unwrap();
        let (first_session, first_branch) = store.create(&actor, "first Agent", binding()).unwrap();
        let first = store
            .agent_binding(&actor, &first_session.id, &first_branch.id)
            .unwrap();
        let router = Router::new(store.clone(), actor.clone(), 8).unwrap();
        let finite_ref = format!("{}/t:1/j:finite-one", Router::canonical(&first));
        let finite_owner = FiniteExposureOwner {
            canonical_agent_ref: Router::canonical(&first),
            actor: actor.as_str().to_owned(),
            session_id: first.session_id.clone(),
            branch_id: first.branch_id.clone(),
        };
        router
            .reserve_finite(&finite_owner, &finite_ref, 8)
            .unwrap();

        let (second_session, second_branch) =
            store.create(&actor, "second Agent", binding()).unwrap();
        let second = store
            .agent_binding(&actor, &second_session.id, &second_branch.id)
            .unwrap();
        router.publish_finite(&finite_owner, &finite_ref).unwrap();

        let ctx = context(&actor, &first, ToolEffect::Read);
        let authority = [7; 32];
        let first_page = router
            .list(
                &ctx,
                AgentJobListRequest {
                    cursor: None,
                    limit: 2,
                    authority_fingerprint: authority,
                },
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first_page.items.len(), 2);
        assert_eq!(
            first_page.items[0].reference,
            Router::canonical(&second),
            "newer Agent creation must sort above an earlier finite reservation"
        );
        assert_eq!(first_page.items[1].reference, finite_ref);
        let cursor = first_page
            .next_cursor
            .clone()
            .expect("third older Agent root requires a continuation");

        let second_page = router
            .list(
                &ctx,
                AgentJobListRequest {
                    cursor: Some(cursor.clone()),
                    limit: 2,
                    authority_fingerprint: authority,
                },
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(second_page.items.len(), 1);
        assert_eq!(second_page.items[0].reference, Router::canonical(&first));
        assert!(second_page.next_cursor.is_none());

        assert!(
            router
                .list(
                    &ctx,
                    AgentJobListRequest {
                        cursor: Some(cursor.clone()),
                        limit: 2,
                        authority_fingerprint: [8; 32],
                    },
                )
                .await
                .is_err(),
            "jobs_list authority rotation must invalidate old cursor"
        );
        let mut tampered = cursor;
        tampered.push('A');
        assert!(
            router
                .list(
                    &ctx,
                    AgentJobListRequest {
                        cursor: Some(tampered),
                        limit: 2,
                        authority_fingerprint: authority,
                    },
                )
                .await
                .is_err(),
            "cursor tampering must fail closed"
        );
    }

    #[tokio::test]
    async fn agent_history_default_filter_cursor_and_disclosure_are_bounded() {
        let actor = PrincipalId::new("local").unwrap();
        let store = SessionStore::in_memory().unwrap();
        let (session, branch) = store.create(&actor, "history Agent", binding()).unwrap();
        let (view, handle) = store.activate(&actor, &session.id, &branch.id).unwrap();
        let mut agent = Agent::builder(
            Arc::new(HistoryProvider),
            AgentOptions::new("fixture", "model"),
        )
        .build()
        .unwrap();
        agent
            .load_history(view.history, Some(Arc::new(handle)))
            .unwrap();
        let binding = store
            .agent_binding(&actor, &session.id, &branch.id)
            .unwrap();
        agent
            .bind_execution_owner(
                aw_agent_core::AgentExecutionBinding::from_trusted_host(
                    Router::canonical(&binding),
                    binding.session_id.clone(),
                    binding.branch_id.clone(),
                )
                .unwrap(),
            )
            .unwrap();

        agent
            .prompt("first prompt", pending::<()>(), |_| Ok(()))
            .await
            .unwrap();
        agent
            .prompt("second prompt", pending::<()>(), |_| Ok(()))
            .await
            .unwrap();

        let router = Router::new(store.clone(), actor.clone(), 8).unwrap();
        let mut ctx = context(&actor, &binding, ToolEffect::Read);
        ctx.result_budget_bytes = 64;

        let default = router
            .read(
                &ctx,
                AgentJobRead {
                    reference: ".".into(),
                    view: None,
                    filter: None,
                    cursor: None,
                    limit: 20,
                    wait_ms: 0,
                },
            )
            .await
            .unwrap()
            .unwrap();
        assert!(default.truncated);
        assert!(default.content.ends_with("[truncated]"));
        assert!(
            !default.content.contains("first prompt"),
            "default read is latest public result, not transcript history"
        );

        ctx.result_budget_bytes = aw_agent_core::tool::MAX_TOOL_RESULT_BYTES;
        let filter = json!({"messages":true});
        let first_page = router
            .read(
                &ctx,
                AgentJobRead {
                    reference: ".".into(),
                    view: None,
                    filter: Some(filter.clone()),
                    cursor: None,
                    limit: 1,
                    wait_ms: 0,
                },
            )
            .await
            .unwrap()
            .unwrap();
        let mut lines = first_page.content.lines();
        let first_record = lines.next().expect("one history record");
        assert!(first_record.contains(r#""kind":"message""#));
        let marker = lines.next().expect("history continuation marker");
        let cursor = marker
            .strip_prefix("more cursor=")
            .expect("opaque history cursor")
            .to_owned();

        let next_page = router
            .read(
                &ctx,
                AgentJobRead {
                    reference: ".".into(),
                    view: None,
                    filter: Some(filter.clone()),
                    cursor: Some(cursor.clone()),
                    limit: 1,
                    wait_ms: 0,
                },
            )
            .await
            .unwrap()
            .unwrap();
        assert!(next_page
            .content
            .lines()
            .next()
            .is_some_and(|line| { line.contains(r#""kind":"message""#) }));

        assert!(
            router
                .read(
                    &ctx,
                    AgentJobRead {
                        reference: ".".into(),
                        view: Some("messages".into()),
                        filter: None,
                        cursor: Some(cursor),
                        limit: 1,
                        wait_ms: 0,
                    },
                )
                .await
                .is_err(),
            "history cursor is bound to its exact view/filter selection"
        );

        assert!(
            router
                .read(
                    &ctx,
                    AgentJobRead {
                        reference: ".".into(),
                        view: Some("tool_outputs".into()),
                        filter: None,
                        cursor: None,
                        limit: 20,
                        wait_ms: 0,
                    },
                )
                .await
                .is_err(),
            "historical tool outputs stay denied until current resource disclosure is revalidated"
        );
    }
}
