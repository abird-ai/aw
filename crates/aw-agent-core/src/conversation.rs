//! Bounded in-memory projection of a durable or ephemeral branch. Only complete traces become context.
use crate::{
    completion::CompletionMeta,
    limits::{MAX_ATTEMPTS, MAX_CONTEXT_BYTES, MAX_PROMPT_BYTES},
    provider::Provider,
    Error, Result,
};
use serde::{Deserialize, Serialize};
use std::{future::Future, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: u64,
    pub role: Role,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<crate::ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Provider-private continuation state (e.g. native thinking context).
    /// Never rendered, exported or sent to a different provider automatically.
    #[serde(skip)]
    pub provider_context: Option<String>,
}
impl Message {
    pub fn text(id: u64, role: Role, content: impl Into<String>) -> Self {
        Self {
            id,
            role,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            name: None,
            provider_context: None,
        }
    }
    pub fn context_bytes(&self) -> usize {
        self.content
            .len()
            .saturating_add(self.provider_context.as_ref().map_or(0, String::len))
            .saturating_add(
                self.tool_calls
                    .iter()
                    .map(|c| c.name.len() + c.id.len() + c.arguments.to_string().len())
                    .sum::<usize>(),
            )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InvocationRecord {
    pub ordinal: u32,
    pub identity: crate::InvocationIdentity,
    pub meta: Option<CompletionMeta>,
    pub outcome: String,
    /// Unreported tokens/cost are unknown, never assumed zero.
    pub usage_verified: bool,
}
/// Complete totals are None when any matching call omitted that metric. No
/// price/currency is invented for local, subscription or API accounts.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsageSummary {
    pub model_calls: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub calls_without_complete_usage: u64,
}
impl Default for UsageSummary {
    fn default() -> Self {
        Self {
            model_calls: 0,
            input_tokens: Some(0),
            output_tokens: Some(0),
            calls_without_complete_usage: 0,
        }
    }
}
pub fn usage_for(turns: &[Turn], actor: &crate::PrincipalId) -> UsageSummary {
    let mut out = UsageSummary::default();
    for call in turns
        .iter()
        .flat_map(|t| &t.invocations)
        .filter(|i| &i.identity.actor == actor)
    {
        out.model_calls += 1;
        let input = call.meta.as_ref().and_then(|m| m.usage.input_tokens);
        let output = call.meta.as_ref().and_then(|m| m.usage.output_tokens);
        out.calls_without_complete_usage += u64::from(input.is_none() || output.is_none());
        out.input_tokens = out
            .input_tokens
            .zip(input)
            .and_then(|(a, b)| a.checked_add(b));
        out.output_tokens = out
            .output_tokens
            .zip(output)
            .and_then(|(a, b)| a.checked_add(b));
    }
    out
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Completed,
    Cancelled,
    Incomplete,
    Failed,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    pub id: u64,
    pub prompt: String,
    pub answer: String,
    pub model: String,
    pub profile: String,
    pub outcome: Outcome,
    pub meta: Option<CompletionMeta>,
    pub error: Option<String>,
    #[serde(default)]
    pub actor: Option<crate::PrincipalId>,
    #[serde(default)]
    pub tool_records: Vec<crate::ToolRecord>,
    #[serde(default)]
    pub invocations: Vec<InvocationRecord>,
    #[serde(default)]
    pub context_messages: Vec<Message>,
    #[serde(default)]
    pub needs_reconciliation: bool,
}

#[derive(Debug)]
pub struct Conversation {
    system: String,
    turns: Vec<Turn>,
    next_id: u64,
    summary: Option<String>,
    compacted_through: u64,
    summary_provenance_sha256: Option<String>,
    context_blocks: Vec<crate::ContextBlock>,
}
const MAX_HISTORY_BYTES: usize = 4 * 1024 * 1024;

impl Conversation {
    pub fn new(system: String) -> Result<Self> {
        if system.len() > MAX_PROMPT_BYTES {
            return Err(Error::Limit("system prompt exceeds 128 KiB"));
        }
        Ok(Self {
            system,
            turns: Vec::new(),
            next_id: 1,
            summary: None,
            compacted_through: 0,
            summary_provenance_sha256: None,
            context_blocks: Vec::new(),
        })
    }
    /// Restore a bounded, validated ancestry without contacting a provider.
    pub fn from_history(system: String, state: &crate::HistoryState) -> Result<Self> {
        let mut out = Self::new(system)?;
        if !matches!(state.version, 1 | 2)
            || (state.version == 1 && !state.context_blocks.is_empty())
            || state.next_turn_id == 0
            || state.next_turn_id > (1u64 << 42)
            || state.turns.len() > MAX_ATTEMPTS
            || state.compacted_through >= state.next_turn_id
            || state
                .summary_provenance_sha256
                .as_ref()
                .is_some_and(|digest| {
                    !crate::context::valid_digest(digest)
                        || state.summary.is_none()
                        || state.compacted_through == 0
                })
            || (state.compacted_through != 0
                && (state.summary.is_none() || state.summary_provenance_sha256.is_none()))
        {
            return Err(Error::Limit(
                "unsupported/oversized active history; use an explicit context checkpoint",
            ));
        }
        if state
            .summary
            .as_ref()
            .is_some_and(|s| s.len() > MAX_PROMPT_BYTES)
        {
            return Err(Error::Limit("context summary exceeds 128 KiB"));
        }
        if state.context_blocks.len() > crate::context::MAX_CONTEXT_BLOCKS {
            return Err(Error::Limit("too many context imports"));
        }
        let mut slot = 0;
        for block in &state.context_blocks {
            block.validate(state.turns.len())?;
            if block.before_turn < slot {
                return Err(Error::Protocol("context blocks out of chronological order"));
            }
            slot = block.before_turn;
        }
        let mut previous = state.compacted_through;
        let mut total = out.system.len()
            + state.summary.as_ref().map_or(0, String::len)
            + state
                .context_blocks
                .iter()
                .map(|b| b.message(0).context_bytes())
                .sum::<usize>();
        if total > MAX_HISTORY_BYTES {
            return Err(Error::Limit("context blocks exceed memory budget"));
        }
        for t in &state.turns {
            if t.id <= previous
                || t.id >= state.next_turn_id
                || t.prompt.len() > MAX_PROMPT_BYTES
                || t.answer.len() > crate::limits::MAX_OUTPUT_BYTES
                || t.tool_records.len() > 128
                || t.invocations.len() > 32
                || t.context_messages.len() > crate::limits::MAX_MESSAGES
            {
                return Err(Error::Protocol("invalid restored turn bounds or order"));
            }
            if t.profile.is_empty()
                || t.profile.len() > 256
                || t.model.is_empty()
                || t.model.len() > 256
                || t.profile.chars().any(char::is_control)
                || t.model.chars().any(char::is_control)
            {
                return Err(Error::Protocol("invalid restored account/model label"));
            }
            for r in &t.tool_records {
                r.call.validate_envelope()?;
                if let Some(o) = &r.output {
                    o.validate()?;
                }
            }
            let bytes = t
                .context_messages
                .iter()
                .map(Message::context_bytes)
                .sum::<usize>();
            if bytes > crate::tool::MAX_TRACE_BYTES
                || t.context_messages
                    .iter()
                    .any(|m| m.provider_context.is_some())
            {
                return Err(Error::Protocol(
                    "invalid restored trace or private provider state",
                ));
            }
            if t.outcome == Outcome::Completed && !t.context_messages.is_empty() {
                crate::validate_messages(&t.context_messages)?;
                if t.context_messages
                    .first()
                    .is_none_or(|m| m.role != Role::User || m.content != t.prompt)
                    || t.context_messages
                        .last()
                        .is_none_or(|m| m.role != Role::Assistant || !m.tool_calls.is_empty())
                {
                    return Err(Error::Protocol(
                        "restored complete trace must have its prompt and completed answer",
                    ));
                }
            }
            total = total.saturating_add(
                serde_json::to_vec(t)
                    .map_err(|_| Error::Protocol("invalid restored turn"))?
                    .len(),
            );
            if total > MAX_HISTORY_BYTES {
                return Err(Error::Limit(
                    "active restored history exceeds memory budget; create a checkpoint",
                ));
            }
            previous = t.id;
        }
        out.turns = state.turns.clone();
        out.next_id = state.next_turn_id;
        out.summary = state.summary.clone();
        out.compacted_through = state.compacted_through;
        out.summary_provenance_sha256 = state.summary_provenance_sha256.clone();
        out.context_blocks = state.context_blocks.clone();
        Ok(out)
    }
    pub fn history_state(&self, needs_reconciliation: bool) -> crate::HistoryState {
        crate::HistoryState {
            version: 2,
            context_blocks: self.context_blocks.clone(),
            turns: self.turns.clone(),
            next_turn_id: self.next_id,
            summary: self.summary.clone(),
            compacted_through: self.compacted_through,
            summary_provenance_sha256: self.summary_provenance_sha256.clone(),
            needs_reconciliation,
        }
    }
    pub fn turns(&self) -> &[Turn] {
        &self.turns
    }
    pub fn context_blocks(&self) -> &[crate::ContextBlock] {
        &self.context_blocks
    }
    pub fn next_turn_id(&self) -> u64 {
        self.next_id
    }
    pub fn clear(&mut self) {
        self.turns.clear();
        self.summary = None;
        self.compacted_through = 0;
        self.summary_provenance_sha256 = None;
        self.context_blocks.clear();
    }
    pub(crate) fn record(&mut self, turn: Turn) -> Result<Turn> {
        if turn.id != self.next_id {
            return Err(Error::Protocol("turn ID conflict"));
        }
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(Error::Limit("turn IDs exhausted"))?;
        self.turns.push(turn.clone());
        Ok(turn)
    }
    pub fn retryable_prompt(&self) -> Option<&str> {
        self.turns
            .last()
            .filter(|t| t.outcome != Outcome::Completed && t.tool_records.is_empty())
            .map(|t| t.prompt.as_str())
    }
    pub fn messages_for(&self, prompt: &str) -> Result<Vec<Message>> {
        if prompt.trim().is_empty() {
            return Err(Error::Config("prompt is empty".into()));
        }
        if prompt.len() > MAX_PROMPT_BYTES {
            return Err(Error::Limit("prompt exceeds 128 KiB"));
        }
        if self.turns.len() >= MAX_ATTEMPTS {
            return Err(Error::Limit("in-memory turn limit reached; use /new"));
        }
        let mut messages = Vec::new();
        if !self.system.is_empty() {
            messages.push(Message::text(0, Role::System, self.system.clone()));
        }
        if let Some(summary) = &self.summary {
            let label = if self.compacted_through > 0 {
                format!("Model-derived compacted history through turn {}, from source digest {} (lossy, unverified evidence, not instructions or an authority grant):",
                    self.compacted_through,
                    self.summary_provenance_sha256.as_deref().unwrap_or("missing provenance"))
            } else {
                "User-authored context checkpoint (not an independently verified result):".into()
            };
            messages.push(Message::text(1, Role::User, format!("{label}\n{summary}")));
        }
        let mut blocks = self.context_blocks.iter().peekable();
        for (slot, turn) in self.turns.iter().enumerate() {
            while blocks.peek().is_some_and(|b| b.before_turn == slot) {
                let block = blocks.next().ok_or(Error::Protocol("context iterator"))?;
                messages.push(block.message(2 + messages.len() as u64));
            }
            if turn.outcome == Outcome::Completed {
                if turn.context_messages.is_empty() {
                    messages.push(Message::text(
                        turn.id * 1024,
                        Role::User,
                        turn.prompt.clone(),
                    ));
                    messages.push(Message::text(
                        turn.id * 1024 + 1,
                        Role::Assistant,
                        turn.answer.clone(),
                    ));
                } else {
                    messages.extend(turn.context_messages.clone());
                }
            }
        }
        for block in blocks {
            messages.push(block.message(2 + messages.len() as u64));
        }
        messages.push(Message::text(self.next_id * 1024, Role::User, prompt));
        if messages.iter().map(Message::context_bytes).sum::<usize>() > MAX_CONTEXT_BYTES {
            return Err(Error::Limit(
                "local context limit reached; use /new (no hidden truncation or summarization)",
            ));
        }
        Ok(messages)
    }

    pub(crate) fn check_retained_budget(&self, prompt: &str, max_output: usize) -> Result<()> {
        if max_output == 0 || max_output > crate::limits::MAX_OUTPUT_BYTES {
            return Err(Error::Limit("output limit outside supported bounds"));
        }
        let retained = self.system.len()
            + self.summary.as_ref().map_or(0, String::len)
            + self
                .context_blocks
                .iter()
                .map(|b| b.message(0).context_bytes())
                .sum::<usize>()
            + self
                .turns
                .iter()
                .map(|t| {
                    t.prompt.len()
                        + t.answer.len()
                        + t.context_messages
                            .iter()
                            .map(Message::context_bytes)
                            .sum::<usize>()
                        + t.tool_records
                            .iter()
                            .map(|r| r.output.as_ref().map_or(0, |o| o.content.len()))
                            .sum::<usize>()
                })
                .sum::<usize>();
        if retained
            .saturating_add(prompt.len())
            .saturating_add(max_output)
            .saturating_add(crate::tool::MAX_TRACE_BYTES * 2)
            > MAX_HISTORY_BYTES
        {
            return Err(Error::Limit("in-memory history budget reached; use /new"));
        }
        Ok(())
    }

    /// Cancellation and output are frontend-supplied. This core has no TTY, files,
    /// process, listener, model routing or persistence side effects.
    #[allow(clippy::too_many_arguments)] // Small Phase 0 frontend boundary; no hidden global state.
    pub async fn send<C, F>(
        &mut self,
        provider: &dyn Provider,
        profile: &str,
        model: &str,
        prompt: String,
        timeout: Duration,
        max_output: usize,
        cancel: C,
        on_delta: F,
    ) -> Result<&Turn>
    where
        C: Future<Output = ()> + Send,
        F: FnMut(&str) -> Result<()> + Send,
    {
        let messages = self.messages_for(&prompt)?;
        self.send_prepared(
            provider, profile, model, prompt, messages, timeout, max_output, cancel, on_delta,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn send_prepared<C, F>(
        &mut self,
        provider: &dyn Provider,
        profile: &str,
        model: &str,
        prompt: String,
        messages: Vec<Message>,
        timeout: Duration,
        max_output: usize,
        cancel: C,
        mut on_delta: F,
    ) -> Result<&Turn>
    where
        C: Future<Output = ()> + Send,
        F: FnMut(&str) -> Result<()> + Send,
    {
        crate::validate_messages(&messages)?;
        self.check_retained_budget(&prompt, max_output)?;
        let mut answer = String::new();
        let result = {
            let mut sink = |text: &str| -> Result<()> {
                if answer.len().saturating_add(text.len()) > max_output {
                    return Err(Error::Limit(
                        "response text limit reached; partial response not committed to context",
                    ));
                }
                answer.push_str(text);
                on_delta(text)
            };
            tokio::select! {
                biased;
                _=cancel=>Err(Error::Cancelled),
                _=tokio::time::sleep(timeout)=>Err(Error::Timeout),
                result=provider.stream(&messages,model,&mut sink)=>result,
            }
        };
        let (outcome, meta, error) = match result {
            Ok(meta) if meta.complete() && !answer.is_empty() => {
                (Outcome::Completed, Some(meta), None)
            }
            Ok(meta) => {
                let why = if answer.is_empty() {
                    "provider completed without an answer"
                } else {
                    "provider stopped incompletely (output limit/filter/other reason); not added to context"
                };
                (Outcome::Incomplete, Some(meta), Some(why.into()))
            }
            Err(Error::Cancelled) => (Outcome::Cancelled, None, Some(Error::Cancelled.to_string())),
            Err(e) => (Outcome::Failed, None, Some(e.to_string())),
        };
        let id = self.next_id;
        self.next_id += 1;
        self.turns.push(Turn {
            id,
            prompt,
            answer,
            model: model.into(),
            profile: profile.into(),
            outcome,
            meta,
            error,
            actor: None,
            tool_records: Vec::new(),
            invocations: Vec::new(),
            context_messages: Vec::new(),
            needs_reconciliation: false,
        });
        // The push above always establishes a last element. Avoid exposing an
        // unchecked reference even if future edits change this implementation.
        self.turns
            .last()
            .ok_or(Error::Protocol("internal turn bookkeeping failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{CompletionFuture, DeltaSink};
    use std::future::pending;
    struct Fake {
        fail: bool,
    }
    impl Provider for Fake {
        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                sink("hello")?;
                if self.fail {
                    return Err(Error::Protocol("fixture failure"));
                }
                Ok(CompletionMeta {
                    finish_reason: "stop".into(),
                    ..Default::default()
                })
            })
        }
    }
    struct Slow;
    impl Provider for Slow {
        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(pending())
        }
    }
    #[test]
    fn blank_and_large_prompts_rejected() {
        let c = Conversation::new("system".into()).unwrap();
        assert!(c.messages_for(" ").is_err());
        assert!(c.messages_for(&"x".repeat(MAX_PROMPT_BYTES + 1)).is_err());
    }
    #[tokio::test]
    async fn completed_turns_form_context() {
        let mut c = Conversation::new("system".into()).unwrap();
        c.send(
            &Fake { fail: false },
            "fake",
            "fixture",
            "one".into(),
            Duration::from_secs(2),
            100,
            pending(),
            |_| Ok(()),
        )
        .await
        .unwrap();
        let m = c.messages_for("two").unwrap();
        assert_eq!(m.len(), 4);
        assert_eq!(m[1].content, "one");
        assert_eq!(m[2].content, "hello");
    }
    #[tokio::test]
    async fn failed_text_is_recorded_not_reused() {
        let mut c = Conversation::new("".into()).unwrap();
        c.send(
            &Fake { fail: true },
            "fake",
            "fixture",
            "one".into(),
            Duration::from_secs(2),
            100,
            pending(),
            |_| Ok(()),
        )
        .await
        .unwrap();
        assert_eq!(c.turns()[0].answer, "hello");
        assert_eq!(c.turns()[0].outcome, Outcome::Failed);
        assert_eq!(c.messages_for("two").unwrap().len(), 1);
        assert_eq!(c.retryable_prompt(), Some("one"));
    }
    #[tokio::test]
    async fn cancellation_is_recorded() {
        let mut c = Conversation::new("".into()).unwrap();
        c.send(
            &Slow,
            "fake",
            "fixture",
            "one".into(),
            Duration::from_secs(2),
            100,
            async {},
            |_| Ok(()),
        )
        .await
        .unwrap();
        assert_eq!(c.turns()[0].outcome, Outcome::Cancelled);
    }
    #[tokio::test]
    async fn timeout_is_recorded() {
        let mut c = Conversation::new("".into()).unwrap();
        c.send(
            &Slow,
            "fake",
            "fixture",
            "one".into(),
            Duration::from_millis(1),
            100,
            pending(),
            |_| Ok(()),
        )
        .await
        .unwrap();
        assert_eq!(c.turns()[0].outcome, Outcome::Failed);
    }
    #[tokio::test]
    async fn output_limit_is_not_success() {
        let mut c = Conversation::new("".into()).unwrap();
        c.send(
            &Fake { fail: false },
            "fake",
            "fixture",
            "one".into(),
            Duration::from_secs(2),
            2,
            pending(),
            |_| Ok(()),
        )
        .await
        .unwrap();
        assert_ne!(c.turns()[0].outcome, Outcome::Completed);
    }
    #[tokio::test]
    async fn three_turns_and_clear() {
        let mut c = Conversation::new("system".into()).unwrap();
        for p in ["one", "two", "three"] {
            c.send(
                &Fake { fail: false },
                "fake",
                "fixture",
                p.into(),
                Duration::from_secs(2),
                100,
                pending(),
                |_| Ok(()),
            )
            .await
            .unwrap();
        }
        assert_eq!(c.messages_for("four").unwrap().len(), 8);
        c.clear();
        assert_eq!(c.messages_for("new").unwrap().len(), 2);
    }
}
