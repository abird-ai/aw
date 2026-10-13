//! Typed hosted hook contract. One semantic hook vocabulary for the Agent,
//! suboperations and Actor Runtime: ten stages; ONLY Next or Defer actions.
//! A linked native extension is privileged same-UID code, not an OS sandbox.
//! External process hooks are selected by the host and treated as verdict DATA.
//!
//! This hosted adapter never replaces ToolPolicy, provider/Session validation,
//! paid-attempt receipts, the portable TurnMachine, or durable effect intents.
use crate::{
    conversation::{Message, Role},
    tool::{ToolCall, ToolOutput, ToolSpec},
    Error, InvocationIdentity, Result,
};
use aw_agent_kernel::hook_contract::{HookFlow, Stage};
use std::{future::Future, pin::Pin, sync::Arc};

pub type HookFuture<'a> = Pin<Box<dyn Future<Output = Result<HookFlow<HookEvent>>> + Send + 'a>>;

/// Immutable provider/account/actor provenance. This is not an authorization
/// object: the owning host rechecks the lease after each await.
#[derive(Clone, Copy, Debug)]
pub struct HookContext<'a> {
    pub turn_id: u64,
    /// Only present when an authenticated Actor Session owns a bounded
    /// continuation reservation. A standalone Agent cannot self-authorize.
    pub continuation: Option<aw_agent_kernel::hook_contract::Continuation>,
    pub profile: &'a str,
    pub model: &'a str,
    pub identity: &'a InvocationIdentity,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecisionKind {
    Summary,
    Routing,
    Planner,
    Guardian,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecisionPhase {
    Before,
    After,
}

/// A typed projection of the real execution seam. Do not mutate or export
/// provider-private signed reasoning as regular prompt/model content.
#[derive(Clone, Debug)]
pub enum HookEvent {
    /// Prepared context for a new Agent or auxiliary invocation. System
    /// contributions may be prepended, but the original context is immutable.
    Prompt {
        input: String,
        messages: Vec<Message>,
    },
    /// Complete source-authorized proposal; call/spec identity and arguments
    /// are pinned by the provider's already durable assistant evidence.
    ToolCall { call: ToolCall, spec: ToolSpec },
    /// Immutable factual receipt plus separate model-visible projection.
    /// A hook may redact *visible.content* but cannot rewrite the receipt.
    ToolResult {
        call: ToolCall,
        receipt: ToolOutput,
        visible: ToolOutput,
    },
    /// A separately journaled tool-free decision; candidate/identity/charge
    /// evidence is immutable, and a hook may only continue or veto.
    Step {
        kind: DecisionKind,
        phase: DecisionPhase,
        evidence: String,
    },
    /// Read-only public-safe event projection after its owning journal fact
    /// (if any) was persisted. Must not contain private signed thought, raw
    /// credentials, process output or mutable receipt/authorization fields.
    Observation { stage: Stage, label: String },
}
impl HookEvent {
    pub const fn stage(&self) -> Stage {
        match self {
            Self::Prompt { .. } => Stage::Prompt,
            Self::ToolCall { .. } | Self::ToolResult { .. } => Stage::Tool,
            Self::Step { .. } => Stage::Step,
            Self::Observation { stage, .. } => *stage,
        }
    }
    fn same_message(a: &Message, b: &Message) -> bool {
        a.id == b.id
            && a.role == b.role
            && a.content == b.content
            && a.tool_calls == b.tool_calls
            && a.tool_call_id == b.tool_call_id
            && a.name == b.name
            && a.provider_context.is_none()
            && b.provider_context.is_none()
    }
    /// Every Next replacement is checked against the original authoritative
    /// event, not just the previous hook's output. This stops a malicious
    /// intermediate hook from laundering identity or factual receipt changes.
    pub(crate) fn validate_next(original: &Self, next: &Self) -> Result<()> {
        match (original, next) {
            (
                Self::Prompt {
                    input: from,
                    messages: previous,
                },
                Self::Prompt {
                    input: to,
                    messages: updated,
                },
            ) => {
                if from != to
                    || updated.len() < previous.len()
                    || updated.len() > crate::limits::MAX_MESSAGES
                {
                    return Err(Error::Denied(
                        "hook changed original prompt identity/context",
                    ));
                }
                let added = updated.len() - previous.len();
                if updated[..added].iter().any(|m| {
                    m.role != Role::System
                        || m.provider_context.is_some()
                        || m.tool_call_id.is_some()
                        || !m.tool_calls.is_empty()
                }) {
                    return Err(Error::Denied(
                        "prompt hooks may contribute only bounded system-prefix text",
                    ));
                }
                if !previous
                    .iter()
                    .zip(&updated[added..])
                    .all(|(a, b)| Self::same_message(a, b))
                {
                    return Err(Error::Denied(
                        "hook rewrote existing provider context/evidence",
                    ));
                }
                crate::validate_messages(updated)?;
                Ok(())
            }
            (Self::ToolCall { call: a, spec: x }, Self::ToolCall { call: b, spec: y }) => {
                // The original provider proposal remains immutable. A
                // changed argument payload is just a *candidate revision*;
                // the Agent requires a separate host policy, exact grants
                // and committed ToolRevised evidence before any effect.
                if a.id != b.id
                    || a.name != b.name
                    || x.name != y.name
                    || x.effect != y.effect
                    || x.description != y.description
                    || x.parameters != y.parameters
                {
                    return Err(Error::Denied(
                        "tool hook cannot rewrite an already recorded call/schema",
                    ));
                }
                b.validate_envelope()
            }
            (
                Self::ToolResult {
                    call: a,
                    receipt: r,
                    visible: v,
                },
                Self::ToolResult {
                    call: b,
                    receipt: s,
                    visible: w,
                },
            ) => {
                if a != b
                    || r.content != s.content
                    || r.is_error != s.is_error
                    || r.truncated != s.truncated
                    || v.is_error != w.is_error
                    || v.truncated != w.truncated
                {
                    return Err(Error::Denied(
                        "tool result hook cannot change factual receipt/status",
                    ));
                }
                w.validate()
            }
            (
                Self::Step {
                    kind: a,
                    phase: p,
                    evidence: e,
                },
                Self::Step {
                    kind: b,
                    phase: q,
                    evidence: f,
                },
            ) if a == b && p == q && e == f => Ok(()),
            (
                Self::Observation { stage: a, label: x },
                Self::Observation { stage: b, label: y },
            ) if a == b
                && x == y
                && matches!(
                    a,
                    Stage::Turn | Stage::Message | Stage::Output | Stage::Event | Stage::Lifecycle
                ) =>
            {
                Ok(())
            }
            _ => Err(Error::Denied(
                "hook changed stage/kind or immutable evidence",
            )),
        }
    }
    /// No extension may claim observational authority; only the owning Agent
    /// produces these stage projections with bounded non-sensitive labels.
    pub fn observation(stage: Stage, label: impl Into<String>) -> Result<Self> {
        if !matches!(
            stage,
            Stage::Turn | Stage::Message | Stage::Output | Stage::Event | Stage::Lifecycle
        ) {
            return Err(Error::Denied(
                "observation stage requires a typed authoritative event",
            ));
        }
        let label = label.into();
        if label.len() > 256 || label.chars().any(char::is_control) {
            return Err(Error::Limit(
                "observation label must be bounded and printable",
            ));
        }
        Ok(Self::Observation { stage, label })
    }
    pub fn prompt(self) -> Result<Vec<Message>> {
        match self {
            Self::Prompt { messages, .. } => Ok(messages),
            _ => Err(Error::Protocol("hook returned the wrong stage")),
        }
    }
    pub fn tool_call(self) -> Result<ToolCall> {
        match self {
            Self::ToolCall { call, .. } => Ok(call),
            _ => Err(Error::Protocol("hook returned the wrong tool-call stage")),
        }
    }
    pub fn visible_result(self) -> Result<ToolOutput> {
        match self {
            Self::ToolResult { visible, .. } => Ok(visible),
            _ => Err(Error::Protocol("hook returned the wrong tool-result stage")),
        }
    }
}

/// One hook callback in the Rust public SDK. Multiple verbs or legacy
/// before_*/after_* methods are deliberately NOT part of this trait.
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    /// Cheap stage-interest hint for the shared dispatch hot path. This does
    /// not carry authority; mandatory ToolPolicy/journal gates always run.
    fn wants_stage(&self, _stage: Stage) -> bool {
        true
    }
    fn on_hook<'a>(&'a self, _: HookContext<'a>, event: HookEvent) -> HookFuture<'a> {
        Box::pin(async move { Ok(HookFlow::Next(Some(event))) })
    }
}
pub(crate) fn validate_extensions(extensions: &[Arc<dyn Extension>]) -> Result<()> {
    if extensions.len() > crate::limits::MAX_EXTENSIONS {
        return Err(Error::Limit("too many extensions"));
    }
    let mut names = std::collections::HashSet::new();
    for extension in extensions {
        let name = extension.name();
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || !names.insert(name.to_owned())
        {
            return Err(Error::Config("invalid or duplicate extension name".into()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ten_stage_vocabulary_stays_kernel_owned() {
        let stages = [
            Stage::Turn,
            Stage::Step,
            Stage::Input,
            Stage::Prompt,
            Stage::Message,
            Stage::Output,
            Stage::Tool,
            Stage::Signal,
            Stage::Event,
            Stage::Lifecycle,
        ];
        assert_eq!(stages.len(), 10);
        assert_eq!(
            HookEvent::Step {
                kind: DecisionKind::Summary,
                phase: DecisionPhase::Before,
                evidence: String::new()
            }
            .stage(),
            Stage::Step
        );
    }
}
