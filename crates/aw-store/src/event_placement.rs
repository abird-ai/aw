//! Shared same-Store DL-200 Event selector placement proof.
//!
//! This owns only selector-to-Store affinity. Concrete source visibility remains
//! with each resource owner (Agent ACL today), and no global topology generation
//! is invented here.

use crate::{
    agent_owner, event_scan::EventPlacement, resources, store_scope::StoreScope, Error,
    PrincipalId, Result, SessionParticipationAction,
};
use aw_tools::event_subject::EventPattern;
use rusqlite::Connection;
use sha2::{Digest as _, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EventSelectorPlacement {
    Agent { reference: String },
    Project { reference: String },
}

impl EventSelectorPlacement {
    pub(crate) fn from_pattern(pattern: &EventPattern) -> Result<Self> {
        if let Some(reference) = pattern.exact_agent_ref() {
            return Ok(Self::Agent { reference });
        }
        if let Some(reference) = pattern.wildcard_agent_project_ref() {
            return Ok(Self::Project { reference });
        }
        Err(Error::Denied(
            "Event selector placement is not provably single-store",
        ))
    }

    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::Agent { .. } => "agent",
            Self::Project { .. } => "project",
        }
    }

    pub(crate) fn reference(&self) -> &str {
        match self {
            Self::Agent { reference } | Self::Project { reference } => reference,
        }
    }

    pub(crate) fn token(&self, scope: &StoreScope) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(match self {
            Self::Agent { .. } => b"aw-event-agent-placement-v1\0".as_slice(),
            Self::Project { .. } => b"aw-event-project-placement-v1\0".as_slice(),
        });
        hash.update((scope.id().as_str().len() as u64).to_be_bytes());
        hash.update(scope.id().as_str().as_bytes());
        hash.update((self.reference().len() as u64).to_be_bytes());
        hash.update(self.reference().as_bytes());
        hash.finalize().into()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EventPlacementSnapshot {
    Agent { session: String, branch: String },
    Project { domain: String, workspace: String },
}

/// Admission proof for a selector under the creating/reading principal.
///
/// Exact Agent selectors require current view. Broad exact-Project Agent-wildcard
/// selectors additionally require current Domain membership as the discovery
/// gate; concrete Agent visibility is still rechecked per Event later.
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
            let (domain, workspace) = project_coordinates(reference)?;
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
    }
}

pub(crate) fn snapshot(
    connection: &Connection,
    placement: &EventSelectorPlacement,
) -> Result<Option<EventPlacementSnapshot>> {
    match placement {
        EventSelectorPlacement::Agent { reference } => {
            Ok(agent_owner::key_for_ref_optional(connection, reference)?
                .map(|(session, branch)| EventPlacementSnapshot::Agent { session, branch }))
        }
        EventSelectorPlacement::Project { reference } => {
            let (domain, workspace) = project_coordinates(reference)?;
            let current = resources::active_local_hosted_workspace_exact_in(connection, workspace)?;
            Ok(current
                .filter(|current| current.domain_id == domain)
                .map(|_| EventPlacementSnapshot::Project {
                    domain: domain.to_owned(),
                    workspace: workspace.to_owned(),
                }))
        }
    }
}

pub(crate) fn state(
    connection: &Connection,
    placement: &EventSelectorPlacement,
    expected: &EventPlacementSnapshot,
) -> Result<EventPlacement> {
    match snapshot(connection, placement) {
        Ok(Some(current)) if current == *expected => Ok(EventPlacement::Current),
        Ok(_) => Ok(EventPlacement::Stale),
        Err(Error::Conflict(_)) => Ok(EventPlacement::Unknown),
        Err(other) => Err(other),
    }
}

fn project_coordinates(reference: &str) -> Result<(&str, &str)> {
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
    if identity.is_empty() || segments.next().is_some() {
        return Err(Error::Storage("invalid Event Project placement ref"));
    }
    Ok((domain, workspace))
}
