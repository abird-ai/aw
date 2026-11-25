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
