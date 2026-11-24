//! Shared same-Store DL-200 Event selector placement proof.
//!
//! This owns only selector-to-Store completeness and affinity. Concrete source
//! visibility remains with each resource owner; no global topology generation or
//! merged cursor is introduced.

use crate::{
    agent_owner,
    event_scan::{EventPlacement, EventSource},
    resources,
    store_scope::StoreScope,
    Error, PrincipalId, Result, SessionParticipationAction,
};
use aw_tools::event_subject::EventPattern;
use rusqlite::Connection;
use sha2::{Digest as _, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EventSelectorPlacement {
    Agent {
        reference: String,
    },
    Project {
        reference: String,
    },
    /// Broad Agent-subtree selector with wildcard Project+Agent coordinates.
    /// The full canonical Event pattern is retained as the private affinity key;
    /// Domain/Principal filters are decoded once for source/completeness checks.
    Universe {
        reference: String,
        domain: Option<String>,
        identity: Option<String>,
    },
}

impl EventSelectorPlacement {
    pub(crate) fn from_pattern(pattern: &EventPattern) -> Result<Self> {
        if let Some(reference) = pattern.exact_agent_ref() {
            return Ok(Self::Agent { reference });
        }
        if let Some(reference) = pattern.wildcard_agent_project_ref() {
            return Ok(Self::Project { reference });
        }
        if let Some(universe) = pattern.wildcard_project_agent_universe() {
            return Ok(Self::Universe {
                reference: pattern.as_str().to_owned(),
                domain: universe.domain().map(str::to_owned),
                identity: universe.identity().map(str::to_owned),
            });
        }
        Err(Error::Denied(
            "Event selector placement is not provably single-store",
        ))
    }

    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::Agent { .. } => "agent",
            Self::Project { .. } => "project",
            Self::Universe { .. } => "universe",
        }
    }

    pub(crate) fn reference(&self) -> &str {
        match self {
            Self::Agent { reference }
            | Self::Project { reference }
            | Self::Universe { reference, .. } => reference,
        }
    }

    pub(crate) fn from_stored(kind: &str, reference: String) -> Result<Self> {
        match kind {
            "agent" => Ok(Self::Agent { reference }),
            "project" => Ok(Self::Project { reference }),
            "universe" => {
                let pattern = EventPattern::parse_absolute(&reference)
                    .map_err(|_| Error::Storage("invalid stored Event universe pattern"))?;
                let universe = pattern
                    .wildcard_project_agent_universe()
                    .ok_or(Error::Storage("invalid stored Event universe placement"))?;
                Ok(Self::Universe {
                    reference,
                    domain: universe.domain().map(str::to_owned),
                    identity: universe.identity().map(str::to_owned),
                })
            }
            _ => Err(Error::Storage("invalid Event selector placement kind")),
        }
    }

    pub(crate) fn token(&self, scope: &StoreScope) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(match self {
            Self::Agent { .. } => b"aw-event-agent-placement-v1\0".as_slice(),
            Self::Project { .. } => b"aw-event-project-placement-v1\0".as_slice(),
            Self::Universe { .. } => b"aw-event-universe-placement-v1\0".as_slice(),
        });
        hash.update((scope.id().as_str().len() as u64).to_be_bytes());
        hash.update(scope.id().as_str().as_bytes());
        hash.update((self.reference().len() as u64).to_be_bytes());
        hash.update(self.reference().as_bytes());
        hash.finalize().into()
    }

    pub(crate) fn source_matches(
        &self,
        connection: &Connection,
        actor: &PrincipalId,
        source: &EventSource,
        agent_ref: &str,
    ) -> Result<bool> {
        match self {
            Self::Agent { reference } => Ok(agent_ref == reference),
            Self::Project { reference } => {
                let (domain, identity, workspace) = project_coordinates(reference)?;
                if source.domain != domain
                    || source.identity.as_deref() != Some(identity)
                    || source.project.as_deref() != Some(workspace)
                {
                    return Ok(false);
                }
                resources::has_active_domain_membership(connection, actor, domain)
            }
            Self::Universe {
                domain, identity, ..
            } => {
                if source.agent.is_none()
                    || domain
                        .as_deref()
                        .is_some_and(|expected| source.domain != expected)
                    || identity
                        .as_deref()
                        .is_some_and(|expected| source.identity.as_deref() != Some(expected))
                {
                    return Ok(false);
                }
                resources::has_active_domain_membership(connection, actor, &source.domain)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EventPlacementSnapshot {
    Agent { session: String, branch: String },
    Project { domain: String, workspace: String },
    Universe,
}

/// Admission proof for a selector under the creating/reading principal.
///
/// Exact Agent selectors require current view. Exact-Project Agent-wildcards
/// require current Domain membership. Broad Project-wildcard universes require
/// every currently-authorized matching Domain to be wholly served by this Store.
pub(crate) fn admit(
    connection: &Connection,
    actor: &PrincipalId,
    placement: &EventSelectorPlacement,
) -> Result<EventPlacementSnapshot> {
    match placement {
        EventSelectorPlacement::Agent { reference } => {
            let Some((session, branch)) = agent_owner::key_for_ref_optional(connection, reference)?
            else {
                return Err(Error::Denied(
                    "Event selector unavailable or not authorized",
                ));
            };
            agent_owner::authorize_access(
                connection,
                actor,
                agent_owner::AgentKey::new(&session, &branch),
                SessionParticipationAction::View,
            )
            .map_err(|error| match error {
                Error::Denied(_) => Error::Denied("Event selector unavailable or not authorized"),
                other => other,
            })?;
            Ok(EventPlacementSnapshot::Agent { session, branch })
        }
        EventSelectorPlacement::Project { reference } => {
            let (domain, _identity, workspace) = project_coordinates(reference)?;
            let Some(current) =
                resources::active_local_hosted_workspace_exact_in(connection, workspace)?
            else {
                return Err(Error::Denied(
                    "Event selector unavailable or not authorized",
                ));
            };
            if current.domain_id != domain
                || !resources::has_active_domain_membership(connection, actor, domain)?
            {
                return Err(Error::Denied(
                    "Event selector unavailable or not authorized",
                ));
            }
            Ok(EventPlacementSnapshot::Project {
                domain: domain.to_owned(),
                workspace: workspace.to_owned(),
            })
        }
        EventSelectorPlacement::Universe { domain, .. } => {
            let domains = resources::authorized_event_domain_placements(
                connection,
                actor,
                domain.as_deref(),
            )?;
            if domain.is_some() && domains.is_empty() {
                return Err(Error::Denied(
                    "Event selector unavailable or not authorized",
                ));
            }
            if !universe_is_local(connection, &domains)? {
                return Err(Error::Denied(
                    "Event selector spans an unavailable Event store",
                ));
            }
            Ok(EventPlacementSnapshot::Universe)
        }
    }
}

/// Current structural/completeness snapshot under the caller's current Domain
/// authority. Loss of concrete source visibility does not itself stale a cursor;
/// source authorization handles that separately.
pub(crate) fn snapshot(
    connection: &Connection,
    actor: &PrincipalId,
    placement: &EventSelectorPlacement,
) -> Result<Option<EventPlacementSnapshot>> {
    match placement {
        EventSelectorPlacement::Agent { reference } => {
            Ok(agent_owner::key_for_ref_optional(connection, reference)?
                .map(|(session, branch)| EventPlacementSnapshot::Agent { session, branch }))
        }
        EventSelectorPlacement::Project { reference } => {
            let (domain, _identity, workspace) = project_coordinates(reference)?;
            let current = resources::active_local_hosted_workspace_exact_in(connection, workspace)?;
            Ok(current
                .filter(|current| current.domain_id == domain)
                .map(|_| EventPlacementSnapshot::Project {
                    domain: domain.to_owned(),
                    workspace: workspace.to_owned(),
                }))
        }
        EventSelectorPlacement::Universe { domain, .. } => {
            let domains = resources::authorized_event_domain_placements(
                connection,
                actor,
                domain.as_deref(),
            )?;
            Ok(
                universe_is_local(connection, &domains)?
                    .then_some(EventPlacementSnapshot::Universe),
            )
        }
    }
}

pub(crate) fn state(
    connection: &Connection,
    actor: &PrincipalId,
    placement: &EventSelectorPlacement,
    expected: &EventPlacementSnapshot,
) -> Result<EventPlacement> {
    match snapshot(connection, actor, placement) {
        Ok(Some(current)) if current == *expected => Ok(EventPlacement::Current),
        Ok(_) => Ok(EventPlacement::Stale),
        Err(Error::Conflict(_)) => Ok(EventPlacement::Unknown),
        Err(other) => Err(other),
    }
}

fn universe_is_local(
    connection: &Connection,
    domains: &[resources::EventDomainPlacement],
) -> Result<bool> {
    let scope = StoreScope::load(connection)?;
    Ok(domains.iter().all(|domain| {
        domain.authority_store_id == scope.id().as_str() && domain.all_active_workspaces_local
    }))
}

fn project_coordinates(reference: &str) -> Result<(&str, &str, &str)> {
    let mut segments = reference
        .strip_prefix('/')
        .ok_or(Error::Storage("invalid Event Project placement ref"))?
        .split('/');
    let domain = segments
        .next()
        .and_then(|value| value.strip_prefix("d:"))
        .filter(|value| !value.is_empty())
        .ok_or(Error::Storage("invalid Event Project placement Domain"))?;
    let identity = segments
        .next()
        .and_then(|value| value.strip_prefix("i:"))
        .filter(|value| !value.is_empty())
        .ok_or(Error::Storage("invalid Event Project placement Principal"))?;
    let workspace = segments
        .next()
        .and_then(|value| value.strip_prefix("p:"))
        .filter(|value| !value.is_empty())
        .ok_or(Error::Storage("invalid Event Project placement Workspace"))?;
    if segments.next().is_some() {
        return Err(Error::Storage("invalid Event Project placement ref"));
    }
    Ok((domain, identity, workspace))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EventHandlerAuthority, EventHandlerPoll, EventHandlerStart, EventReadAfter,
        EventReadOutcome, SessionBinding, SessionStore,
    };
    use aw_agent_core::EventHandlerRegistry;
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

    #[test]
    fn authorized_same_store_universe_is_complete_and_remote_scope_stales_all_consumers() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let (session, branch) = store.create(&actor, "universe source", binding()).unwrap();
        let agent_binding = store
            .agent_binding(&actor, &session.id, &branch.id)
            .unwrap();
        let agent_ref = format!(
            "/d:{}/i:{}/p:{}/a:{}",
            agent_binding.domain_handle,
            agent_binding.identity_handle,
            agent_binding.project_handle,
            agent_binding.agent_handle
        );

        let second_domain = store
            .create_domain(&actor, "second-event-domain", None)
            .unwrap();
        let second_workspace = store
            .workspace(&actor, Some(&second_domain.id), "local")
            .unwrap();
        let (second_session, second_branch) = {
            let mut connection = store.connection().unwrap();
            let tx = connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .unwrap();
            let created = crate::resources::create_in_workspace_tx(
                &tx,
                &actor,
                "second-domain agent",
                &binding(),
                &second_workspace.id,
                None,
            )
            .unwrap();
            tx.commit().unwrap();
            created
        };
        let second_binding = store
            .agent_binding(&actor, &second_session.id, &second_branch.id)
            .unwrap();
        let second_agent_ref = format!(
            "/d:{}/i:{}/p:{}/a:{}",
            second_binding.domain_handle,
            second_binding.identity_handle,
            second_binding.project_handle,
            second_binding.agent_handle
        );
        assert_ne!(agent_binding.domain_handle, second_binding.domain_handle);
        let pattern = "d:*.i:*.p:*.a:*.e:#ready";

        let read_cursor = match store
            .events_read_page(&actor, pattern, EventReadAfter::Now, 16)
            .unwrap()
        {
            EventReadOutcome::Page(page) => {
                assert!(page.events.is_empty());
                page.cursor
            }
            other => panic!("unexpected broad Event cursor outcome: {other:?}"),
        };

        let subscription = store
            .events_subscribe(&actor, pattern, &agent_ref, true)
            .unwrap();
        assert!(subscription.changed);

        let registry = EventHandlerRegistry::new();
        let handler_key = registry
            .register("fixture", "universe", pattern, 1, ())
            .unwrap();
        let handler_reader = registry.reader().unwrap();
        let handler = handler_reader.descriptor(&handler_key).unwrap();
        let authority =
            EventHandlerAuthority::new(actor.clone(), "svc:fixture:universe", 1).unwrap();
        let activation = store
            .events_handler_activate(&handler, &authority, EventHandlerStart::Now)
            .unwrap();
        assert!(activation.changed);

        store
            .events_publish(
                &actor,
                &agent_ref,
                "#ready",
                Some(&serde_json::json!({"scope":"first-domain"})),
            )
            .unwrap();
        store
            .events_publish(
                &actor,
                &second_agent_ref,
                "#ready",
                Some(&serde_json::json!({"scope":"second-domain"})),
            )
            .unwrap();

        let page = match store
            .events_read_page(
                &actor,
                pattern,
                EventReadAfter::Cursor(read_cursor.clone()),
                16,
            )
            .unwrap()
        {
            EventReadOutcome::Page(page) => page,
            other => panic!("unexpected broad Event read outcome: {other:?}"),
        };
        assert_eq!(page.events.len(), 2);
        assert_eq!(
            page.events
                .iter()
                .map(|event| event.data.clone().unwrap())
                .collect::<Vec<_>>(),
            vec![
                serde_json::json!({"scope":"first-domain"}),
                serde_json::json!({"scope":"second-domain"})
            ]
        );

        let subscription_run = store
            .event_subscription_deliver(&agent_ref, pattern, 16)
            .unwrap();
        assert!(!subscription_run.lagged);
        assert_eq!(subscription_run.accepted, 1);
        assert_eq!(subscription_run.skipped, 1);
        assert_eq!(subscription_run.ready_inputs.len(), 1);

        for expected_scope in ["first-domain", "second-domain"] {
            let handler_delivery = match store.events_handler_poll(&handler, &authority).unwrap() {
                EventHandlerPoll::Delivery(delivery) => delivery,
                other => panic!("unexpected universe Handler delivery: {other:?}"),
            };
            assert_eq!(
                handler_delivery.event.data,
                Some(serde_json::json!({"scope":expected_scope}))
            );
            store
                .events_handler_ack(&handler, &authority, &handler_delivery.receipt)
                .unwrap();
        }

        // A Domain outside this actor's authority may live elsewhere without
        // perturbing this actor's placement/completeness token.
        let hidden_actor = PrincipalId::new("hidden-universe-user").unwrap();
        let _ = store
            .create(&hidden_actor, "hidden universe seed", binding())
            .unwrap();
        let hidden_domain = store
            .create_domain(&hidden_actor, "hidden-remote-domain", None)
            .unwrap();
        {
            let connection = store.connection().unwrap();
            connection
                .execute(
                    "UPDATE domains SET authority_store_id='remote-hidden-store'
                     WHERE domain_id=?1",
                    [&hidden_domain.id],
                )
                .unwrap();
        }

        assert!(matches!(
            store
                .events_read_page(
                    &actor,
                    pattern,
                    EventReadAfter::Cursor(read_cursor.clone()),
                    16,
                )
                .unwrap(),
            EventReadOutcome::Page(_)
        ));
        assert!(
            !store
                .event_subscription_deliver(&agent_ref, pattern, 16)
                .unwrap()
                .lagged
        );
        assert_eq!(
            store.events_handler_poll(&handler, &authority).unwrap(),
            EventHandlerPoll::Empty
        );

        // Once one Domain actually authorized for this actor moves to another
        // Event authority store, every ordered consumer must reject the partial
        // world instead of silently continuing on this Store.
        {
            let connection = store.connection().unwrap();
            connection
                .execute(
                    "UPDATE domains SET authority_store_id='remote-authorized-store'
                     WHERE domain_id=?1",
                    [&second_domain.id],
                )
                .unwrap();
        }

        assert_eq!(
            store
                .events_read_page(&actor, pattern, EventReadAfter::Cursor(read_cursor), 16,)
                .unwrap(),
            EventReadOutcome::CursorStale
        );
        assert!(
            store
                .event_subscription_deliver(&agent_ref, pattern, 16)
                .unwrap()
                .lagged
        );
        assert_eq!(
            store.events_handler_poll(&handler, &authority).unwrap(),
            EventHandlerPoll::Lagged
        );
    }
}
