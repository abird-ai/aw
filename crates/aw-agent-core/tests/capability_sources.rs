use aw_agent_core::*;
use aw_agent_kernel::capability::SourceKind;
use serde_json::{json, Value};
use std::{
    future::pending,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

struct Proposed {
    name: &'static str,
}
impl Provider for Proposed {
    fn supports_tools(&self) -> bool {
        true
    }
    fn stream<'a>(
        &'a self,
        _: &'a [Message],
        _: &'a str,
        _: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async { Err(Error::Protocol("unexpected text-only generation")) })
    }
    fn stream_tools<'a>(
        &'a self,
        messages: &'a [Message],
        _: &'a str,
        _: &'a [ToolSpec],
        sink: &'a mut DeltaSink<'_>,
    ) -> ModelFuture<'a> {
        Box::pin(async move {
            if messages.last().is_some_and(|m| m.role == Role::Tool) {
                sink("completed")?;
                Ok(ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "stop".into(),
                        usage: Usage {
                            input_tokens: Some(2),
                            output_tokens: Some(1),
                        },
                    },
                    ..Default::default()
                })
            } else {
                Ok(ModelReply {
                    meta: CompletionMeta {
                        finish_reason: "tool_calls".into(),
                        ..Default::default()
                    },
                    tool_calls: vec![ToolCall {
                        id: "c1".into(),
                        name: self.name.into(),
                        arguments: json!({}),
                    }],
                    ..Default::default()
                })
            }
        })
    }
}
struct CountingTool {
    effect: ToolEffect,
    spec_calls: Arc<AtomicUsize>,
    executions: Arc<AtomicUsize>,
}
impl Tool for CountingTool {
    fn spec(&self) -> ToolSpec {
        self.spec_calls.fetch_add(1, Ordering::SeqCst);
        ToolSpec {
            name: "shared_name".into(),
            description: "test bounded capability".into(),
            parameters: json!({"type":"object","properties":{},"additionalProperties":false}),
            effect: self.effect.clone(),
        }
    }
    fn validate(&self, value: &Value) -> Result<()> {
        if value.as_object().is_some_and(|o| o.is_empty()) {
            Ok(())
        } else {
            Err(Error::Denied("unknown parameters"))
        }
    }
    fn execute<'a>(&'a self, _: ToolContext, _: &'a Value) -> ToolFuture<'a> {
        Box::pin(async {
            self.executions.fetch_add(1, Ordering::SeqCst);
            Ok(ToolOutput::text("done"))
        })
    }
}
struct RevocableTool(Arc<AtomicBool>);
impl Tool for RevocableTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "ext__source__echo".into(),
            description: "source liveness is not itself authority".into(),
            parameters: json!({"type":"object"}),
            effect: ToolEffect::Unknown,
        }
    }
    fn is_available(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn validate(&self, _: &Value) -> Result<()> {
        Ok(())
    }
    fn execute<'a>(&'a self, _: ToolContext, _: &'a Value) -> ToolFuture<'a> {
        Box::pin(async { Ok(ToolOutput::text("ok")) })
    }
}
#[test]
fn external_source_liveness_hides_advertisements_and_blocks_dispatch() {
    let actor = PrincipalId::new("local").unwrap();
    let alive = Arc::new(AtomicBool::new(true));
    let source = ToolSource::new(SourceKind::Extension, "source", 123).unwrap();
    let mut tools = ToolRegistry::default();
    tools
        .register_from(Arc::new(RevocableTool(alive.clone())), source.clone())
        .unwrap();
    let mut policy = ToolPolicy::default();
    policy
        .permit_source(
            actor.clone(),
            "ext__source__echo",
            &source,
            &ToolEffect::Unknown,
        )
        .unwrap();
    assert!(tools.allows(&actor, &policy, "ext__source__echo"));
    assert_eq!(tools.visible(&actor, &policy).len(), 1);
    alive.store(false, Ordering::Release);
    assert!(
        !tools.allows(&actor, &policy, "ext__source__echo"),
        "a poisoned remote process must not dispatch another effect"
    );
    assert!(
        tools.visible(&actor, &policy).is_empty(),
        "the model must not see a dead remote tool after connection failure"
    );
    assert_eq!(
        tools.revision(),
        1,
        "process liveness cannot mint a new trust revision"
    );
}

fn setup_kind(kind: SourceKind) -> (Agent, Arc<AtomicUsize>, Arc<AtomicUsize>, ToolSource) {
    let executions = Arc::new(AtomicUsize::new(0));
    let spec_calls = Arc::new(AtomicUsize::new(0));
    let source = ToolSource::new(kind, "files", 7).unwrap();
    let mut policy = ToolPolicy::default();
    // Even an exact built-in Host grant cannot authorize a same-named remote tool.
    policy
        .permit_host(
            PrincipalId::new("local").unwrap(),
            "shared_name",
            &ToolEffect::Write,
        )
        .unwrap();
    let mut agent = Agent::builder(
        Arc::new(Proposed {
            name: "shared_name",
        }),
        AgentOptions::new("p", "m"),
    )
    .tool_policy(policy)
    .build()
    .unwrap();
    agent
        .register_source_tool(
            Arc::new(CountingTool {
                effect: ToolEffect::Write,
                spec_calls: spec_calls.clone(),
                executions: executions.clone(),
            }),
            source.clone(),
        )
        .unwrap();
    (agent, executions, spec_calls, source)
}

fn setup() -> (Agent, Arc<AtomicUsize>, Arc<AtomicUsize>, ToolSource) {
    setup_kind(SourceKind::Mcp)
}

#[tokio::test]
async fn extension_origin_requires_exact_host_epoch_and_revocation_fences_tool() {
    let (mut agent, executions, _, origin) = setup_kind(SourceKind::Extension);
    let actor = PrincipalId::new("local").unwrap();
    assert!(
        agent.tool_specs().is_empty(),
        "trusted Host grants must not authorize external source tools"
    );
    assert_eq!(
        agent
            .prompt("no implicit grant", pending(), |_| Ok(()))
            .await
            .unwrap()
            .outcome,
        Outcome::Completed
    );
    assert_eq!(executions.load(Ordering::SeqCst), 0);
    let wrong = ToolSource::new(SourceKind::Extension, "files", 8).unwrap();
    assert!(agent
        .permit_registered_source_tool(actor.clone(), "shared_name", &wrong)
        .is_err());
    agent
        .permit_registered_source_tool(actor.clone(), "shared_name", &origin)
        .unwrap();
    assert_eq!(agent.tool_specs().len(), 1);
    assert_eq!(
        agent
            .prompt("permitted", pending(), |_| Ok(()))
            .await
            .unwrap()
            .outcome,
        Outcome::Completed
    );
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    agent.revoke_registered_source_tool(&actor, "shared_name", &origin);
    assert!(agent.tool_specs().is_empty());
    assert_eq!(
        agent
            .prompt("revoked", pending(), |_| Ok(()))
            .await
            .unwrap()
            .outcome,
        Outcome::Completed
    );
    assert_eq!(executions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn remote_tool_never_inherits_a_name_grant_and_rechecks_revocation() {
    let (mut agent, n, specs, source) = setup();
    assert!(agent.tool_specs().is_empty());
    assert_eq!(
        specs.load(Ordering::SeqCst),
        1,
        "tool schema cached at registration"
    );
    let failed = agent
        .prompt("blocked", pending(), |_| Ok(()))
        .await
        .unwrap();
    assert_eq!(failed.outcome, Outcome::Completed);
    assert_eq!(failed.tool_records.len(), 1);
    assert_eq!(failed.tool_records[0].status, ToolStatus::Denied);
    assert!(failed.tool_records[0].output.as_ref().unwrap().is_error);
    assert_eq!(n.load(Ordering::SeqCst), 0);

    let actor = PrincipalId::new("local").unwrap();
    let wrong = ToolSource::new(SourceKind::Mcp, "files", 8).unwrap();
    assert!(agent
        .permit_registered_source_tool(actor.clone(), "shared_name", &wrong)
        .is_err());
    assert!(agent
        .permit_registered_source_tool(actor.clone(), "shared_name", &source)
        .is_ok());
    assert_eq!(agent.tool_specs().len(), 1);
    assert_eq!(specs.load(Ordering::SeqCst), 1);
    let completed = agent
        .prompt("allowed", pending(), |_| Ok(()))
        .await
        .unwrap();
    assert_eq!(completed.outcome, Outcome::Completed);
    assert_eq!(n.load(Ordering::SeqCst), 1);

    agent.revoke_registered_source_tool(&actor, "shared_name", &source);
    assert!(agent.tool_specs().is_empty());
    let denied = agent
        .prompt("revoked", pending(), |_| Ok(()))
        .await
        .unwrap();
    assert_eq!(denied.outcome, Outcome::Completed);
    assert_eq!(denied.tool_records[0].status, ToolStatus::Denied);
    assert_eq!(n.load(Ordering::SeqCst), 1);
}

#[test]
fn exact_source_grant_rejects_site_actor_and_effect_mismatch() {
    let actor = PrincipalId::new("local").unwrap();
    let source = ToolSource::new(SourceKind::Extension, "extension-a", 3).unwrap();
    let mut policy = ToolPolicy::default();
    policy
        .permit_source(actor.clone(), "shared_name", &source, &ToolEffect::Read)
        .unwrap();
    let mut tools = ToolRegistry::default();
    tools
        .register_from(
            Arc::new(CountingTool {
                effect: ToolEffect::Write,
                spec_calls: Arc::new(AtomicUsize::new(0)),
                executions: Arc::new(AtomicUsize::new(0)),
            }),
            source.clone(),
        )
        .unwrap();
    // Cannot grant Read and then dispatch Write with same source revision.
    assert!(!tools.allows(&actor, &policy, "shared_name"));
    policy
        .permit_source(actor.clone(), "shared_name", &source, &ToolEffect::Write)
        .unwrap();
    assert!(tools.allows(&actor, &policy, "shared_name"));
    assert!(!tools.allows(&PrincipalId::new("other").unwrap(), &policy, "shared_name"));
    policy.execution_site = "elsewhere".into();
    assert!(!tools.allows(&actor, &policy, "shared_name"));
}

#[test]
fn registration_name_collision_fails_for_two_origins_without_replacement() {
    let (mut agent, n, _, original) = setup();
    let next = ToolSource::new(SourceKind::Extension, "other", 12).unwrap();
    assert!(agent
        .register_source_tool(
            Arc::new(CountingTool {
                effect: ToolEffect::Read,
                spec_calls: Arc::new(AtomicUsize::new(0)),
                executions: n,
            }),
            next
        )
        .is_err());
    let actor = PrincipalId::new("local").unwrap();
    assert!(agent
        .permit_registered_source_tool(actor, "shared_name", &original)
        .is_ok());
    assert_eq!(agent.tool_specs().len(), 1);
}

#[test]
fn builtin_host_registration_requires_exact_source_grant() {
    let actor = PrincipalId::new("local").unwrap();
    let mut policy = ToolPolicy::default();
    policy
        .permit_host(actor.clone(), "shared_name", &ToolEffect::Read)
        .unwrap();
    let mut registry = ToolRegistry::default();
    let spec_calls = Arc::new(AtomicUsize::new(0));
    registry
        .register(Arc::new(CountingTool {
            effect: ToolEffect::Read,
            spec_calls: spec_calls.clone(),
            executions: Arc::new(AtomicUsize::new(0)),
        }))
        .unwrap();
    assert!(registry.allows(&actor, &policy, "shared_name"));
    assert_eq!(registry.visible(&actor, &policy).len(), 1);
    assert_eq!(spec_calls.load(Ordering::SeqCst), 1);
    assert_eq!(registry.revision(), 1);
}

#[test]
fn unknown_source_effect_is_not_a_read_grant() {
    let actor = PrincipalId::new("local").unwrap();
    let source = ToolSource::new(SourceKind::Mcp, "unreviewed", 44).unwrap();
    let mut policy = ToolPolicy::default();
    policy
        .permit_source(actor.clone(), "shared_name", &source, &ToolEffect::Read)
        .unwrap();
    let mut registry = ToolRegistry::default();
    registry
        .register_from(
            Arc::new(CountingTool {
                effect: ToolEffect::Unknown,
                spec_calls: Arc::new(AtomicUsize::new(0)),
                executions: Arc::new(AtomicUsize::new(0)),
            }),
            source.clone(),
        )
        .unwrap();
    assert!(!registry.allows(&actor, &policy, "shared_name"));
    // A trusted host can consciously grant a conservative unknown effect.
    policy
        .permit_source(actor.clone(), "shared_name", &source, &ToolEffect::Unknown)
        .unwrap();
    assert!(registry.allows(&actor, &policy, "shared_name"));
}

struct ChangingSpec {
    changed: Arc<std::sync::atomic::AtomicBool>,
    spec_calls: Arc<AtomicUsize>,
}
impl Tool for ChangingSpec {
    fn spec(&self) -> ToolSpec {
        self.spec_calls.fetch_add(1, Ordering::SeqCst);
        ToolSpec {
            name: "shared_name".into(),
            description: "mutable metadata for regression".into(),
            parameters: json!({"type":"object","additionalProperties":false}),
            effect: if self.changed.load(Ordering::SeqCst) {
                ToolEffect::Write
            } else {
                ToolEffect::Read
            },
        }
    }
    fn validate(&self, v: &Value) -> Result<()> {
        if v.as_object().is_some_and(|o| o.is_empty()) {
            Ok(())
        } else {
            Err(Error::Denied("bad parameters"))
        }
    }
    fn execute<'a>(&'a self, _: ToolContext, _: &'a Value) -> ToolFuture<'a> {
        Box::pin(async { Ok(ToolOutput::text("unchanged behavior")) })
    }
}
#[tokio::test]
async fn admitted_schema_and_effect_cannot_be_swapped_after_registration() {
    let changed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let spec_calls = Arc::new(AtomicUsize::new(0));
    let mut policy = ToolPolicy::default();
    policy
        .permit_host(
            PrincipalId::new("local").unwrap(),
            "shared_name",
            &ToolEffect::Read,
        )
        .unwrap();
    let mut agent = Agent::builder(
        Arc::new(Proposed {
            name: "shared_name",
        }),
        AgentOptions::new("p", "m"),
    )
    .shared_tool(Arc::new(ChangingSpec {
        changed: changed.clone(),
        spec_calls: spec_calls.clone(),
    }))
    .tool_policy(policy)
    .build()
    .unwrap();
    changed.store(true, Ordering::SeqCst);
    assert_eq!(spec_calls.load(Ordering::SeqCst), 1);
    assert!(matches!(agent.tool_specs()[0].effect, ToolEffect::Read));
    let settled = agent.prompt("read", pending(), |_| Ok(())).await.unwrap();
    assert_eq!(settled.outcome, Outcome::Completed);
    assert_eq!(settled.tool_records.len(), 1);
    assert!(matches!(settled.tool_records[0].effect, ToolEffect::Read));
    assert_eq!(spec_calls.load(Ordering::SeqCst), 1);
}
