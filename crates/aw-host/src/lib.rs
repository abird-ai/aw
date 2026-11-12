//! One host composition boundary. Does not own another Agent, Actor, Job or
//! Session engine. The initial path remains direct, local and userless.

#![forbid(unsafe_code)]

use aw_agent_core::{AgentBuilder, Error, PrincipalId, Result, ToolPolicy};
use aw_policy::{EffectivePolicy, Strategy};
use aw_tools_adapter::LocalToolOptions;

pub use aw_policy::{Decision as PolicyDecision, Policy, PolicyError, Routes};

#[cfg(feature = "sessions")]
pub use aw_session::{CoordinatorBinding, Sessions};

/// A zero-guardian local host. Current grants still come from ToolPolicy and
/// the adapter's exact native resource controls, not this preference.
pub struct LocalHost<'a> {
    actor: PrincipalId,
    tools: &'a LocalToolOptions,
    effective: EffectivePolicy,
}

impl<'a> LocalHost<'a> {
    pub fn new(actor: PrincipalId, tools: &'a LocalToolOptions) -> Self {
        Self {
            actor,
            tools,
            effective: EffectivePolicy::local(),
        }
    }

    /// Trusted host may supply narrower effective preferences. DelegateAgent
    /// execution will be wired here later, not emulated by the native adapter.
    pub fn with_effective_policy(mut self, effective: EffectivePolicy) -> Result<Self> {
        if effective.strategy() != Strategy::Local || !effective.routes().contains(Strategy::Local)
        {
            return Err(Error::Denied("DelegateAgent host route not yet available"));
        }
        self.effective = self
            .effective
            .narrow(effective)
            .map_err(|_| Error::Denied("host policy cannot widen or retarget authority"))?;
        Ok(self)
    }

    pub fn effective_policy(&self) -> EffectivePolicy {
        self.effective
    }

    /// Apply authenticated scoped constraints. This cannot invent tools or
    /// silently enable an Agent route unsupported by the current local host.
    pub fn with_scoped_policy(self, layers: &[aw_policy::PolicyLayer<'_>]) -> Result<Self> {
        let effective = aw_policy::resolve_scoped(layers)
            .map_err(|_| Error::Denied("effective host policy denied"))?;
        self.with_effective_policy(effective)
    }

    /// Consumes the one-shot local host, moving its actor into the adapter.
    /// No extra PrincipalId allocation or clone in the default bootstrap.
    pub fn configure(self, builder: AgentBuilder) -> Result<AgentBuilder> {
        if self.effective.strategy() != Strategy::Local
            || !self.effective.routes().contains(Strategy::Local)
        {
            return Err(Error::Denied("host route is not Local"));
        }
        let concurrency = self.tools.concurrency.validate()?;
        let mut policy = builder.tool_policy_snapshot();
        let has_grants = policy.has_grants();
        if policy.execution_site != self.tools.execution_site {
            if policy.execution_site == ToolPolicy::default().execution_site && !has_grants {
                policy.execution_site = self.tools.execution_site.clone();
            } else {
                return Err(Error::Config(
                    "host cannot retarget an existing tool execution site".into(),
                ));
            }
        }
        if has_grants {
            // A pre-granted embedding policy may only be narrowed.
            policy.max_parallel_reads = policy
                .max_parallel_reads
                .min(concurrency.parallelism.max_parallel_reads);
            policy.allow_parallel_bash &= concurrency.parallelism.allow_parallel_shell;
            policy.max_parallel_bash = policy
                .max_parallel_bash
                .min(concurrency.parallelism.max_parallel_shell);
        } else {
            // Stock AW construction intentionally selects its own host ceilings.
            policy.max_parallel_reads = concurrency.parallelism.max_parallel_reads;
            policy.allow_parallel_bash = concurrency.parallelism.allow_parallel_shell;
            policy.max_parallel_bash = concurrency.parallelism.max_parallel_shell;
        }
        if let Some(ceiling) = self.effective.max_model_rounds() {
            policy.max_model_rounds = policy.max_model_rounds.min(ceiling);
        }
        aw_tools_adapter::configure(builder.tool_policy(policy), self.actor, self.tools)
    }
}

/// Existing local callers can adopt the host boundary without new state,
/// membership services, provider inference or Session persistence.
pub fn configure_local(
    builder: AgentBuilder,
    actor: PrincipalId,
    tools: &LocalToolOptions,
) -> Result<AgentBuilder> {
    LocalHost::new(actor, tools).configure(builder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::{
        provider::{CompletionFuture, DeltaSink, Provider},
        Agent, AgentOptions, Message, ToolEffect,
    };
    use aw_tools_adapter::{LocalToolConcurrency, ToolMode};
    use std::sync::Arc;

    struct NoInference;
    impl Provider for NoInference {
        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Denied("test provider must not run")) })
        }
    }

    #[test]
    fn userless_local_is_inference_free_and_does_not_grant_tools() {
        let opts = LocalToolOptions::new(".", ToolMode::Off);
        let host = LocalHost::new(PrincipalId::new("host:local").unwrap(), &opts);
        assert_eq!(host.effective_policy().strategy(), Strategy::Local);
        assert_eq!(host.effective_policy().routes(), Routes::LOCAL);
    }

    #[test]
    fn trusted_scope_policy_narrows_local_without_issuing_grants() {
        let opts = LocalToolOptions::new(".", ToolMode::Off);
        let host = LocalHost::new(PrincipalId::new("host:local").unwrap(), &opts);
        let policy = Policy {
            max_model_rounds: Some(3),
            ..Policy::default()
        };
        let layers = [aw_policy::PolicyLayer {
            scope: aw_policy::PolicyScope::Host,
            revision: 1,
            policy: &policy,
        }];
        let host = host.with_scoped_policy(&layers).unwrap();
        assert_eq!(host.effective_policy().strategy(), Strategy::Local);
        assert_eq!(host.effective_policy().max_model_rounds(), Some(3));
    }

    #[test]
    fn repeated_local_policy_application_only_narrows() {
        let opts = LocalToolOptions::new(".", ToolMode::Off);
        let host = LocalHost::new(PrincipalId::new("host:local").unwrap(), &opts);
        let narrow = Policy {
            max_model_rounds: Some(2),
            ..Policy::default()
        };
        let host = host
            .with_scoped_policy(&[aw_policy::PolicyLayer {
                scope: aw_policy::PolicyScope::Session,
                revision: 1,
                policy: &narrow,
            }])
            .unwrap();
        assert_eq!(host.effective_policy().max_model_rounds(), Some(2));
        let wider = Policy::default();
        let host = host
            .with_scoped_policy(&[aw_policy::PolicyLayer {
                scope: aw_policy::PolicyScope::Session,
                revision: 2,
                policy: &wider,
            }])
            .unwrap();
        assert_eq!(host.effective_policy().max_model_rounds(), Some(2));
        assert_eq!(host.effective_policy().routes(), Routes::LOCAL);
    }

    #[test]
    fn granted_builder_cannot_widen_scheduler_or_retarget_site() {
        let actor = PrincipalId::new("host:local").unwrap();
        let mut policy = ToolPolicy {
            max_parallel_reads: 2,
            max_parallel_bash: 1,
            allow_parallel_bash: false,
            ..ToolPolicy::default()
        };
        policy
            .permit_host(actor.clone(), "read", &ToolEffect::Read)
            .unwrap();
        let builder = Agent::builder(Arc::new(NoInference), AgentOptions::new("test", "model"))
            .tool_policy(policy);
        let mut opts = LocalToolOptions::new(".", ToolMode::Off);
        opts.concurrency = LocalToolConcurrency {
            graph_width: 2,
            parallelism: aw_agent_core::ToolParallelism {
                max_parallel_reads: 16,
                max_parallel_shell: 8,
                allow_parallel_shell: true,
            },
        };
        let configured = LocalHost::new(actor.clone(), &opts)
            .configure(builder)
            .unwrap();
        let actual = configured.tool_policy_snapshot();
        assert_eq!(actual.max_parallel_reads, 2);
        assert_eq!(actual.max_parallel_bash, 1);
        assert!(!actual.allow_parallel_bash);
        assert!(actual.has_grants());

        let mut retargeted = opts.clone();
        retargeted.execution_site = "remote-site".into();
        let builder = Agent::builder(Arc::new(NoInference), AgentOptions::new("test", "model"))
            .tool_policy(actual);
        assert!(matches!(
            LocalHost::new(actor, &retargeted).configure(builder),
            Err(Error::Config(_))
        ));
    }

    #[test]
    fn unsupported_delegate_does_not_silently_fallback_to_local() {
        let opts = LocalToolOptions::new(".", ToolMode::Off);
        let host = LocalHost::new(PrincipalId::new("host:local").unwrap(), &opts);
        let effective = aw_policy::resolve(&[Policy {
            prefer: Some(Strategy::DelegateAgent),
            max_model_rounds: Some(4),
            ..Policy::default()
        }])
        .unwrap();
        assert!(matches!(
            host.with_effective_policy(effective),
            Err(Error::Denied(_))
        ));
    }
}
