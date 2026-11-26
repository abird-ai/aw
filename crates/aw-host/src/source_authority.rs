//! Live source policy and original ToolBinding/Actor admission composition.
use crate::AuthenticatedToolGrantEvidence;
use aw_agent_core::{tool::ToolBindingAuthority, Error, NamedSourceAccess, Result, ToolEffect};
use aw_store::{SandboxEffectAuthority, SandboxGrantPolicy};
use aw_tools_adapter::process::ContainmentRequirement;

/// Exact original journal coordinates, never a new operation identity.
pub struct SourceToolRecoveryTarget<'a> {
    pub agent_ref: &'a str,
    pub turn_id: u64,
    pub call_id: &'a str,
}

#[derive(Clone)]
pub(crate) struct LiveSourceTool {
    authority: ToolBindingAuthority,
    evidence: AuthenticatedToolGrantEvidence,
}

impl LiveSourceTool {
    pub(crate) fn is_live(&self) -> bool {
        self.evidence.revalidate(&self.authority).is_ok()
    }

    pub(crate) async fn wait_until_not_live(&self) {
        self.authority
            .wait_until_not_live(&self.evidence.actor, self.evidence.binding())
            .await
    }
}

/// Compose independent source policy with the original live ToolBinding.
/// This creates no new grant. Native and Process execution use this fence.
pub struct SourceToolAuthority<A> {
    source: A,
    pub(crate) tool: LiveSourceTool,
    invocation: Option<aw_agent_core::ToolContext>,
    containment: ContainmentRequirement,
}

impl<A: SandboxEffectAuthority> SourceToolAuthority<A> {
    pub fn new(
        source: A,
        authority: ToolBindingAuthority,
        evidence: AuthenticatedToolGrantEvidence,
    ) -> Result<Self> {
        let policy = source.policy();
        if evidence.actor != policy.actor
            || evidence.resource_scope != policy.sandbox_ref
            || !matches!(
                evidence.binding().effect,
                ToolEffect::Read | ToolEffect::Write | ToolEffect::Process
            )
        {
            return Err(Error::Denied(
                "tool authority does not cover the exact Sandbox actor",
            ));
        }
        evidence.revalidate(&authority)?;
        Ok(Self {
            source,
            tool: LiveSourceTool {
                authority,
                evidence,
            },
            invocation: None,
            containment: ContainmentRequirement::BestAvailable,
        })
    }

    /// Trusted host minimum, independent of the driver's prepared command.
    /// Repeated narrowing cannot weaken the guarantee.
    pub fn requiring_process_containment(mut self, requirement: ContainmentRequirement) -> Self {
        self.containment = self.containment.intersect(requirement);
        self
    }

    pub(crate) fn process_containment(&self) -> ContainmentRequirement {
        self.containment
    }

    /// Bind the existing Actor Envelope for direct runtime dispatch.
    pub fn for_invocation(mut self, ctx: aw_agent_core::ToolContext) -> Result<Self> {
        let binding = self.tool.evidence.binding();
        if self.tool.evidence.actor != ctx.actor
            || ctx.identity.actor != ctx.actor
            || binding.source != ctx.source
            || binding.effect != ctx.effect
            || binding.execution_site != ctx.execution_site
        {
            return Err(Error::Denied(
                "source effect differs from the original tool invocation",
            ));
        }
        ctx.authorize_peer_tool(binding)?;
        self.invocation = Some(ctx);
        Ok(self)
    }
}

impl<A: SandboxEffectAuthority> SandboxEffectAuthority for SourceToolAuthority<A> {
    fn policy(&self) -> &SandboxGrantPolicy {
        self.source.policy()
    }

    fn tool_ceiling(&self) -> &[NamedSourceAccess] {
        self.source.tool_ceiling()
    }

    fn tool_binding(&self) -> Option<&aw_agent_core::tool::ToolBindingSnapshot> {
        Some(self.tool.evidence.binding())
    }

    fn with_admission<T>(&self, admit: impl FnOnce() -> Result<T>) -> Result<T> {
        // Both preflight and the final Store commit use the existing tool
        // publication fence. Never hold its synchronous lock across await.
        self.source.with_admission(|| {
            self.tool.authority.with_revalidated_binding(
                &self.tool.evidence.actor,
                self.tool.evidence.binding(),
                || match &self.invocation {
                    Some(ctx) => ctx.with_peer_tool_commit(self.tool.evidence.binding(), admit),
                    None => admit(),
                },
            )
        })
    }
}
