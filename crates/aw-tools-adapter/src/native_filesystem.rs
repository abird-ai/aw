//! The original canonical native executor over a host-prepared capability.
//! No ambient-root fallback, policy decisions or additional tool registry.
use super::{
    canonical_direct_error, canonical_direct_limits, canonical_direct_output, canonical_request,
    legacy_filesystem_request, tool_failure, LocalToolLimits,
};
use aw_agent_core::{Error, Result, ToolContext, ToolOutput};
use aw_platform::scoped_fs::ScopedError;
use aw_tools::filesystem::{
    access::Action,
    backend::{ExplicitPathBackend, SecuredFilesystem},
    direct,
    request::Request,
    schema::Tool,
};
use serde_json::Value;

pub use crate::private_artifacts::{
    open_source_artifacts, reopen_source_artifacts, validate_source_artifact_state,
};
pub use aw_platform::scoped_fs::capability::{
    CapabilitySet, ExclusionSpec, GrantIdentity, GrantSpec, PreparedGrant,
};
pub use aw_tools::filesystem::private_artifact::Registry as PrivateArtifactRegistry;

/// Fixed host route for the original sealed foreground Graph's native
/// nodes. Preparation and actual source claims remain with the source owner;
/// the existing JobManager owns the node lifecycle and retained result.
pub struct SourceGraphNodeRequest<'a> {
    pub context: &'a ToolContext,
    pub node: &'a aw_tools::filesystem::job_manager::RunningNode,
    pub constructor: &'a aw_agent_core::tool::ToolBindingSnapshot,
    pub binding: &'a aw_agent_core::tool::ToolBindingSnapshot,
    pub cancellation: aw_agent_core::CancellationToken,
}
pub struct NativeNodeResult {
    pub output: ToolOutput,
    pub status: aw_tools::filesystem::batch::Status,
}
pub type SourceGraphNodeFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<NativeNodeResult>> + Send + 'a>>;

pub trait SourceGraphExecutor: Send + Sync {
    fn execute_node<'a>(&'a self, request: SourceGraphNodeRequest<'a>)
        -> SourceGraphNodeFuture<'a>;
    /// The original JobManager invokes this inside its exact terminal
    /// transition persist callback, before projecting the terminal.
    fn settle_node(
        &self,
        context: &ToolContext,
        node: &aw_tools::filesystem::job_manager::RunningNode,
        transition: aw_tools::filesystem::job::Transition,
    ) -> Result<()>;
}

/// Parse once before effect admission. Both the original single-root adapter
/// and a source-aware capability backend use the same limits and projection.
pub struct PreparedNativeRequest {
    request: Request,
    limits: direct::Limits,
    result_bytes: usize,
}

impl PreparedNativeRequest {
    pub fn new(
        tool: Tool,
        arguments: &Value,
        limits: LocalToolLimits,
        ctx: &ToolContext,
    ) -> Result<Self> {
        if legacy_filesystem_request(tool, arguments) {
            return Err(Error::Denied(
                "source-aware native execution requires canonical arguments",
            ));
        }
        Self::from_canonical(tool, arguments, limits, ctx)
    }

    pub(crate) fn from_canonical(
        tool: Tool,
        arguments: &Value,
        limits: LocalToolLimits,
        ctx: &ToolContext,
    ) -> Result<Self> {
        let request = canonical_request(tool, arguments, limits)?;
        let result_bytes = ctx.result_budget(limits.result_bytes)?;
        let limits = canonical_direct_limits(limits, result_bytes, &request);
        Ok(Self {
            request,
            limits,
            result_bytes,
        })
    }

    /// The original Job's sealed semantic result budget, not its outer
    /// constructor projection budget.
    pub fn for_graph_node(
        tool: Tool,
        arguments: &Value,
        limits: LocalToolLimits,
        result_bytes: usize,
    ) -> Result<Self> {
        if !tool.graph_eligible()
            || tool == Tool::Remove
            || legacy_filesystem_request(tool, arguments)
            || result_bytes == 0
            || result_bytes > limits.result_bytes
        {
            return Err(Error::Denied("invalid source Graph native request"));
        }
        let request = canonical_request(tool, arguments, limits)?;
        let limits = canonical_direct_limits(limits, result_bytes, &request);
        Ok(Self {
            request,
            limits,
            result_bytes,
        })
    }

    /// Same native engine and canonical Graph result projection as the
    /// ordinary local Job node. No alternate filesystem execution.
    pub fn execute_graph<B>(
        self,
        backend: &B,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<NativeNodeResult>
    where
        B: SecuredFilesystem<Error = ScopedError> + ExplicitPathBackend,
    {
        let tool = self.request.tool();
        let output = direct::execute(backend, self.request, self.limits, cancelled)
            .map_err(canonical_direct_error)?;
        let status = direct::graph_status(tool, &output);
        let content = direct::graph_text(&output).ok_or(Error::Protocol(
            "direct-only media result reached source Graph",
        ))?;
        Ok(NativeNodeResult {
            output: ToolOutput {
                content,
                is_error: output.is_error,
                truncated: output.truncated,
            },
            status,
        })
    }

    /// Blocking, no detached work. The original caller owns the worker and
    /// must retain effect custody until this returns or preserve uncertainty.
    pub fn execute<B>(self, backend: &B, cancelled: &dyn Fn() -> bool) -> Result<ToolOutput>
    where
        B: SecuredFilesystem<Error = ScopedError> + ExplicitPathBackend,
    {
        let mutating = self.request.tool().action() != Action::Read;
        let result = direct::execute(backend, self.request, self.limits, cancelled);
        let projected = match result {
            Ok(output) => canonical_direct_output(output, self.result_bytes),
            Err(error) => Err(canonical_direct_error(error)),
        };
        tool_failure(projected, mutating)
    }
}
