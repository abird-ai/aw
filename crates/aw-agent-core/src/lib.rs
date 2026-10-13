//! The UI-independent aw agent SDK. No HTTP client, terminal, web server,
//! subprocess, account discovery or durable store is initialized here.
//!
//! Use [`Agent::builder`] to provide a model transport and optional trusted Rust
//! [`Extension`] modules. Both shipped frontends call this same SDK.
//! APIs are pre-1.0 and versioned with the workspace. Nothing is a stable Rust ABI.
pub mod accounts;
pub mod agent;
pub mod auxiliary;
pub mod cancellation;
pub mod compaction;
pub mod completion;
/// Host-only control adapter over one Agent; no independent executor.
pub mod control;
pub mod conversation;
pub mod error;
pub mod extension;
/// One typed nonblocking Next/Defer hook service; no alternate Agent engine.
pub mod hooks;
pub mod journal;
pub mod limits;
#[cfg(feature = "planner")]
pub mod planner;
pub mod provider;
/// Hosted immutable publication with explicitly owned concurrent readers.
pub mod publication;
pub mod retry;
#[cfg(feature = "reviewer")]
pub mod reviewer;
#[cfg(feature = "routing")]
pub mod routing;
/// Session-bound orchestration service; the original Agent and journal stay authoritative.
pub mod runtime;
pub mod tool;

pub use accounts::{
    AccountDescriptor, AccountOwner, BillingOwner, CredentialCustody, InvocationIdentity,
    PrincipalId, ProviderAccounts, ProviderSelection, SelectionReader,
};
pub use agent::{Agent, AgentBuilder, AgentEvent, AgentOptions};
pub use auxiliary::{
    AuxiliaryExecutor, AuxiliaryInput, AuxiliaryJournal, AuxiliaryKind, AuxiliaryResult,
    MAX_AUXILIARY_OUTPUT,
};
pub use aw_agent_kernel::capability::{Effect as CapabilityEffect, SourceKind};
pub use aw_agent_kernel::hook_contract::{HookFlow, Stage as HookStage};
pub use cancellation::CancellationToken;
pub use compaction::{
    CompactionPlan, CompactionPolicy, Decision as CompactionDecision, Mode as CompactionMode,
};
pub use completion::{CompletionMeta, Usage};
pub use conversation::{
    usage_for, Conversation, InvocationRecord, Message, Outcome, Role, Turn, UsageSummary,
};
pub use error::{Error, Result};
pub use extension::{DecisionKind, DecisionPhase, Extension, HookContext, HookEvent, HookFuture};
pub use journal::{
    HistoryState, JournalEntry, JournalFuture, ToolSourceEvidence, TurnJournal, TurnStart,
};
#[cfg(feature = "planner")]
pub use planner::{PlannerExecutor, PlannerInput, PlannerJournal, PlannerResult};
pub use provider::{CompletionFuture, DeltaSink, Provider};
pub use provider::{DelegatedBinding, InvocationContext, InvocationScope, ModelFuture, ModelReply};
pub use retry::{
    RetryPolicy as PreDispatchRetryPolicy, MAX_PRE_DISPATCH_RETRIES, MAX_PROVIDER_ATTEMPTS_PER_TURN,
};
#[cfg(feature = "reviewer")]
pub use reviewer::{ReviewExecutor, ReviewInput, ReviewJournal, ReviewResult};
#[cfg(feature = "routing")]
pub use routing::{RoutingExecutor, RoutingInput, RoutingJournal, RoutingResult};
pub use tool::{
    Tool, ToolCall, ToolContext, ToolEffect, ToolFuture, ToolOutput, ToolPolicy, ToolRecord,
    ToolRegistry, ToolRevisionPolicy, ToolSource, ToolSpec, ToolStatus,
};

/// Revalidate the complete provider-bound context after trusted extensions.
/// Byte budgets are explicit, not a claim about a model's tokenizer/window.
pub fn validate_messages(messages: &[Message]) -> Result<()> {
    if messages.is_empty() || messages.len() > limits::MAX_MESSAGES {
        return Err(Error::Limit("context must contain 1..1024 messages"));
    }
    let bytes = messages
        .iter()
        .try_fold(0usize, |n, m| n.checked_add(m.context_bytes()))
        .ok_or(Error::Limit("context size overflow"))?;
    if bytes > limits::MAX_CONTEXT_BYTES {
        return Err(Error::Limit(
            "local context limit reached; use /new (no hidden truncation)",
        ));
    }
    let mut pending = std::collections::BTreeSet::new();
    for message in messages {
        if message.role == Role::Tool {
            if !message.tool_calls.is_empty() || message.provider_context.is_some() {
                return Err(Error::Protocol(
                    "tool results contain invalid assistant-only fields",
                ));
            }
            let id = message
                .tool_call_id
                .as_deref()
                .ok_or(Error::Protocol("tool result missing call ID"))?;
            if !pending.remove(id) {
                return Err(Error::Protocol("orphan/duplicate tool result"));
            }
        } else {
            if message.tool_call_id.is_some()
                || (message.role != Role::Assistant && message.provider_context.is_some())
            {
                return Err(Error::Protocol(
                    "unexpected tool/private fields on context message",
                ));
            }
            if !pending.is_empty() {
                return Err(Error::Protocol("incomplete tool batch in context"));
            }
            if !message.tool_calls.is_empty() && message.role != Role::Assistant {
                return Err(Error::Protocol("tool calls on non-assistant message"));
            }
            for call in &message.tool_calls {
                call.validate_envelope()?;
                if !pending.insert(call.id.clone()) {
                    return Err(Error::Protocol("duplicate tool call"));
                }
            }
        }
    }
    if !pending.is_empty() {
        return Err(Error::Protocol(
            "missing tool results in provider-bound context",
        ));
    }
    Ok(())
}

pub mod context;
pub use context::{
    summary_message_digest, valid_digest, validate_evidence_ref, ContextBlock, ContextBlockKind,
    EvidenceRef, InvocationPurpose, SummaryExecutor, SummaryInput, SummaryJournal, SummaryResult,
    MAX_CONTEXT_BLOCKS, MAX_SUMMARY_INPUT, MAX_SUMMARY_OUTPUT,
};
