//! Object-safe, transport-independent generation boundary.
use crate::{
    tool::{ToolCall, ToolSpec},
    CompletionMeta, Error, Message, Result,
};
use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ModelReply {
    pub meta: CompletionMeta,
    pub tool_calls: Vec<ToolCall>,
    #[serde(skip)]
    pub provider_context: Option<String>,
}
/// Secret-free, per-model-round identity supplied by the actual journaled loop.
/// Session-scoped delegation refuses calls made through unscoped legacy helpers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationScope {
    pub session_id: String,
    pub branch_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationContext {
    #[serde(default)]
    pub purpose: crate::InvocationPurpose,
    pub scope: Option<InvocationScope>,
    pub turn_id: u64,
    pub ordinal: u32,
    pub identity: crate::InvocationIdentity,
}
/// A trusted adapter declares a specific nonlocal account/executor binding.
/// This is a host contract, not cryptographic attestation of arbitrary Rust code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelegatedBinding {
    pub account_id: String,
    pub executor_id: String,
}

pub type ModelFuture<'a> = Pin<Box<dyn Future<Output = Result<ModelReply>> + Send + 'a>>;

pub type DeltaSink<'a> = dyn FnMut(&str) -> Result<()> + Send + 'a;
pub type CompletionFuture<'a> = Pin<Box<dyn Future<Output = Result<CompletionMeta>> + Send + 'a>>;

/// A provider emits visible text only and returns explicit completion metadata.
/// Implementations must observe callback errors, bound parsing/output buffering,
/// and be cancellation-safe when their future is dropped. Dropping does not
/// guarantee that a remote vendor stops computing or billing.
pub trait Provider: Send + Sync {
    fn delegated_binding(&self) -> Option<DelegatedBinding> {
        None
    }
    /// Default preserves existing local providers. Delegated transports require
    /// this identity-aware path, not stream() with inferred/global user settings.
    fn invoke<'a>(
        &'a self,
        _context: &'a InvocationContext,
        messages: &'a [Message],
        model: &'a str,
        tools: &'a [ToolSpec],
        sink: &'a mut DeltaSink<'_>,
    ) -> ModelFuture<'a> {
        self.stream_tools(messages, model, tools, sink)
    }

    /// Opt-in protocol capability, not a claim that every configured model supports tools.
    fn supports_tools(&self) -> bool {
        false
    }
    fn stream_tools<'a>(
        &'a self,
        messages: &'a [Message],
        model: &'a str,
        tools: &'a [ToolSpec],
        sink: &'a mut DeltaSink<'_>,
    ) -> ModelFuture<'a> {
        Box::pin(async move {
            if !tools.is_empty() {
                return Err(Error::Protocol(
                    "provider does not implement the tool-call protocol",
                ));
            }
            Ok(ModelReply {
                meta: self.stream(messages, model, sink).await?,
                tool_calls: Vec::new(),
                provider_context: None,
            })
        })
    }

    fn stream<'a>(
        &'a self,
        messages: &'a [Message],
        model: &'a str,
        sink: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a>;
}
