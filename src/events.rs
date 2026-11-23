//! Store-backed host composition for model-facing DL-200 Event tools.
//!
//! The adapter owns the model tool/capability. This module only resolves the
//! already-bound current Agent and calls the authoritative Store owners.

use crate::{agent_jobs, Error, Result};
use aw_agent_core::ToolContext;
use aw_tools::event_subject::{EventPattern, EventSubject};
use aw_tools_adapter::events::EventRouter;
use serde_json::Value;

impl EventRouter for agent_jobs::Router {
    fn publish(&self, ctx: &ToolContext, event: &str, data: Option<&Value>) -> Result<Value> {
        let (_canonical, session, branch) = self.current_event_agent(ctx)?;
        let outcome =
            self.event_store()
                .events_publish_turn(&session, &branch, ctx.turn_id, event, data)?;
        serde_json::to_value(outcome)
            .map_err(|_| Error::Protocol("cannot encode model Event publication result"))
    }

    fn subscribe(&self, ctx: &ToolContext, subject: &str, enabled: bool) -> Result<Value> {
        let (canonical, _session, _branch) = self.current_event_agent(ctx)?;
        let subject = contextualize_pattern(&canonical, subject)?;
        let update = self
            .event_store()
            .events_subscribe(&ctx.actor, &subject, &canonical, enabled)?;
        serde_json::to_value(update)
            .map_err(|_| Error::Protocol("cannot encode model Event subscription result"))
    }
}

fn contextualize_pattern(current_agent_ref: &str, value: &str) -> Result<String> {
    let project_ref = current_agent_ref
        .rsplit_once("/a:")
        .map(|(project, _)| project)
        .ok_or(Error::Denied("current Event Agent has no Project root"))?;
    let identity_ref = project_ref
        .rsplit_once("/p:")
        .map(|(identity, _)| identity)
        .ok_or(Error::Denied("current Event Agent has no Principal root"))?;
    let domain_ref = identity_ref
        .rsplit_once("/i:")
        .map(|(domain, _)| domain)
        .ok_or(Error::Denied("current Event Agent has no Domain root"))?;

    let raw = if EventPattern::parse_absolute(value).is_ok() {
        value.to_owned()
    } else {
        let base = if value == ">" || value.starts_with("e:") || value.starts_with("t:") {
            subject_prefix(current_agent_ref)?
        } else if value.starts_with("a:") {
            subject_prefix(project_ref)?
        } else if value.starts_with("p:") {
            subject_prefix(identity_ref)?
        } else if value.starts_with("i:") {
            subject_prefix(domain_ref)?
        } else {
            return Err(Error::Denied(
                "Event subject must be absolute or relative to Agent/Project hierarchy",
            ));
        };
        format!("{base}.{value}")
    };

    let pattern = EventPattern::parse_absolute(&raw)
        .map_err(|_| Error::Denied("invalid Event subscription pattern"))?;
    let exact_same_project = pattern.exact_agent_ref().is_some_and(|agent| {
        agent
            .strip_prefix(project_ref)
            .is_some_and(|suffix| suffix.starts_with("/a:"))
    });
    let wildcard_current_project = pattern
        .wildcard_agent_project_ref()
        .is_some_and(|project| project == project_ref);
    if !(exact_same_project || wildcard_current_project) {
        return Err(Error::Denied(
            "model Event subscription is limited to the current Project placement scope",
        ));
    }
    Ok(pattern.as_str().to_owned())
}

fn subject_prefix(resource_ref: &str) -> Result<String> {
    let subject = EventSubject::from_resource_ref(resource_ref, "#context")
        .map_err(|_| Error::Denied("invalid current Event resource scope"))?;
    subject
        .as_str()
        .strip_suffix(".e:#context")
        .map(str::to_owned)
        .ok_or(Error::Protocol(
            "canonical Event subject lost its final Event token",
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::{
        AccountOwner, BillingOwner, CancellationToken, CredentialCustody, InvocationIdentity,
        ToolEffect, ToolSource, TurnJournal, TurnStart,
    };
    use aw_store::{EventReadAfter, EventReadOutcome, SessionBinding, SessionStore};
    use aw_store_api::SessionBackend as _;
    use std::path::PathBuf;

    fn binding() -> SessionBinding {
        SessionBinding {
            working_directory: PathBuf::from("/tmp"),
            tool_root: PathBuf::from("/tmp"),
            system_prompt: String::new(),
            profile_hint: "fixture".into(),
            model_hint: "model".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    fn identity(actor: &aw_agent_core::PrincipalId) -> InvocationIdentity {
        InvocationIdentity {
            actor: actor.clone(),
            account_id: "fixture".into(),
            account_owner: AccountOwner::User(actor.clone()),
            payer: BillingOwner::User(actor.clone()),
            credential_custody: CredentialCustody::UserDevice("fixture".into()),
            credential_version: None,
            inference_executor: "fixture".into(),
            model: "model".into(),
            grant_revision: 1,
        }
    }

    #[test]
    fn model_subject_context_is_project_bounded() {
        let current = "/d:acme/i:alice/p:core/a:one";
        assert_eq!(
            contextualize_pattern(current, "e:#ready").unwrap(),
            "d:acme.i:alice.p:core.a:one.e:#ready"
        );
        assert_eq!(
            contextualize_pattern(current, "a:two.e:#ready").unwrap(),
            "d:acme.i:alice.p:core.a:two.e:#ready"
        );
        assert_eq!(
            contextualize_pattern(current, "a:*.e:#ready").unwrap(),
            "d:acme.i:alice.p:core.a:*.e:#ready"
        );
        assert!(contextualize_pattern(current, "d:acme.i:alice.p:other.a:*.e:#ready").is_err());
        assert!(contextualize_pattern(current, "d:other.i:alice.p:core.a:one.e:#ready").is_err());
    }

    #[tokio::test]
    async fn model_event_tools_route_through_bound_store_agent() {
        let store = SessionStore::in_memory().unwrap();
        let actor = aw_agent_core::PrincipalId::new("local").unwrap();
        let (session, branch) = store.create(&actor, "events", binding()).unwrap();
        let agent = store
            .agent_binding(&actor, &session.id, &branch.id)
            .unwrap();
        let canonical = format!(
            "/d:{}/i:{}/p:{}/a:{}",
            agent.domain_handle, agent.identity_handle, agent.project_handle, agent.agent_handle
        );
        let identity = identity(&actor);
        let (_, journal) = store.activate(&actor, &session.id, &branch.id).unwrap();
        journal
            .begin(TurnStart {
                turn_id: 1,
                prompt: "event tool turn".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();

        let router = agent_jobs::Router::new(store.clone(), actor.clone(), 8).unwrap();
        let ctx = ToolContext {
            actor: actor.clone(),
            turn_id: 1,
            call_id: "events-tool-fixture".into(),
            source: ToolSource::trusted_host(),
            effect: ToolEffect::Control,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity,
            scope: Some(aw_agent_core::InvocationScope {
                session_id: session.id.clone(),
                branch_id: branch.id.clone(),
            }),
            execution_owner: Some(
                aw_agent_core::AgentExecutionBinding::from_trusted_host(
                    canonical.clone(),
                    session.id.clone(),
                    branch.id.clone(),
                )
                .unwrap(),
            ),
            execution_scope: None,
            live_step_gate: None,
        };

        let subscribed = EventRouter::subscribe(&router, &ctx, "e:#ready", true).unwrap();
        assert_eq!(subscribed["enabled"], true);
        let subject = EventSubject::from_resource_ref(&canonical, "#ready").unwrap();
        assert_eq!(subscribed["subject"], subject.as_str());
        assert!(EventRouter::subscribe(
            &router,
            &ctx,
            "d:other.i:local.p:other.a:*.e:#ready",
            true,
        )
        .is_err());

        let cursor = match store
            .events_read_page(&actor, subject.as_str(), EventReadAfter::Now, 16)
            .unwrap()
        {
            EventReadOutcome::Page(page) => {
                assert!(page.events.is_empty());
                page.cursor
            }
            other => panic!("unexpected Event cursor outcome: {other:?}"),
        };

        let published =
            EventRouter::publish(&router, &ctx, "#ready", Some(&serde_json::json!({"n":1})))
                .unwrap();
        assert_eq!(published["status"], "published");
        assert_eq!(published["result"]["turn"], 1);
        assert_eq!(published["result"]["subject"], subject.as_str());

        let page = match store
            .events_read_page(&actor, subject.as_str(), EventReadAfter::Cursor(cursor), 16)
            .unwrap()
        {
            EventReadOutcome::Page(page) => page,
            other => panic!("unexpected Event read outcome: {other:?}"),
        };
        assert_eq!(page.events.len(), 1);
        assert_eq!(page.events[0].subject, subject.as_str());
        assert_eq!(page.events[0].turn, Some(1));
        assert_eq!(page.events[0].data, Some(serde_json::json!({"n":1})));
        assert!(page.events[0].schema.is_none());

        let disabled = EventRouter::subscribe(&router, &ctx, "e:#ready", false).unwrap();
        assert_eq!(disabled["enabled"], false);
        assert!(disabled["changed"].as_bool().unwrap());
    }
}
