//! Shared source-first DL-200 Event authority and envelope validation.
//!
//! Placement proves one Store; this module proves one concrete Agent source and
//! validates the immutable Event envelope. Protected disclosure manifests remain
//! Unknown until the real manifest/provider authorizer is composed.

use crate::{
    agent_owner,
    event_placement::EventSelectorPlacement,
    event_projection,
    event_scan::{EventAuthorization, EventCandidate, EventSource},
    Error, PrincipalId, Result, SessionParticipationAction,
};
use rusqlite::Connection;
use std::collections::HashMap;

const MAX_EVENT_DATA_BYTES: usize = 4096;
const MAX_EVENT_DATA_DEPTH: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EventSourceEvidence {
    pub(crate) agent_ref: String,
    pub(crate) access: agent_owner::AgentAccessEvidence,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EventEnvelope {
    pub(crate) data: Option<serde_json::Value>,
    pub(crate) actor: Option<String>,
    pub(crate) turn: Option<u64>,
    pub(crate) causal_root: Option<String>,
    pub(crate) causal_hop: u64,
}

pub(crate) fn authorize_agent_ref(
    connection: &Connection,
    actor: &PrincipalId,
    agent_ref: &str,
) -> Result<EventAuthorization<agent_owner::AgentAccessEvidence>> {
    let Some((session, branch)) = agent_owner::key_for_ref_optional(connection, agent_ref)? else {
        return Ok(EventAuthorization::Unknown);
    };
    match agent_owner::authorize_access(
        connection,
        actor,
        agent_owner::AgentKey::new(&session, &branch),
        SessionParticipationAction::View,
    ) {
        Ok(evidence) => Ok(EventAuthorization::Allow(evidence)),
        Err(Error::Denied(_)) => Ok(EventAuthorization::Deny),
        Err(Error::Conflict(_)) => Ok(EventAuthorization::Unknown),
        Err(other) => Err(other),
    }
}

pub(crate) fn source_agent_ref(source: &EventSource) -> Result<String> {
    let identity = source
        .identity
        .as_deref()
        .ok_or(Error::Storage("Agent Event source lacks Principal handle"))?;
    let project = source
        .project
        .as_deref()
        .ok_or(Error::Storage("Agent Event source lacks Project handle"))?;
    let agent = source
        .agent
        .as_deref()
        .ok_or(Error::Storage("Event selector matched a non-Agent source"))?;
    let root = format!("/d:{}/i:{identity}/p:{project}/a:{agent}", source.domain);
    let belongs = source.reference == root
        || source
            .reference
            .strip_prefix(&root)
            .is_some_and(|suffix| suffix.starts_with("/t:"));
    if !belongs {
        return Err(Error::Storage(
            "Event projection source disagrees with its Agent root",
        ));
    }
    Ok(root)
}

pub(crate) fn source_visibility(
    connection: &Connection,
    actor: &PrincipalId,
    placement: &EventSelectorPlacement,
    source: &EventSource,
    cache: &mut HashMap<String, EventAuthorization<agent_owner::AgentAccessEvidence>>,
) -> Result<EventAuthorization<EventSourceEvidence>> {
    let agent_ref = source_agent_ref(source)?;
    match placement {
        EventSelectorPlacement::Agent { reference } if &agent_ref != reference => {
            return Err(Error::Storage(
                "exact-Agent Event scan escaped its placement root",
            ));
        }
        EventSelectorPlacement::Project { reference }
            if !agent_ref
                .strip_prefix(reference)
                .is_some_and(|suffix| suffix.starts_with("/a:")) =>
        {
            return Err(Error::Storage(
                "Project Event scan escaped its placement root",
            ));
        }
        _ => {}
    }

    let authority = if let Some(authority) = cache.get(&agent_ref) {
        authority.clone()
    } else {
        let authority = authorize_agent_ref(connection, actor, &agent_ref)?;
        cache.insert(agent_ref.clone(), authority.clone());
        authority
    };
    Ok(match authority {
        EventAuthorization::Allow(access) => {
            EventAuthorization::Allow(EventSourceEvidence { agent_ref, access })
        }
        EventAuthorization::Deny => EventAuthorization::Deny,
        EventAuthorization::Unknown => EventAuthorization::Unknown,
    })
}

pub(crate) fn candidate_authority(
    candidate: &EventCandidate,
) -> Result<EventAuthorization<EventEnvelope>> {
    let concrete = aw_tools::event_subject::EventSubject::parse(&candidate.subject)
        .map_err(|_| Error::Storage("Event projection subject is not canonical"))?;
    if concrete.source_ref() != candidate.source.reference {
        return Err(Error::Storage(
            "Event projection source disagrees with immutable subject",
        ));
    }
    let marker = candidate
        .payload
        .get(event_projection::EVENT_MARKER)
        .and_then(serde_json::Value::as_object)
        .ok_or(Error::Storage("DL-200 Event envelope is unavailable"))?;
    if marker.get("version").and_then(serde_json::Value::as_u64)
        != Some(event_projection::EVENT_MARKER_VERSION)
        || marker.get("subject").and_then(serde_json::Value::as_str)
            != Some(candidate.subject.as_str())
        || marker.get("event_rev").and_then(serde_json::Value::as_u64) != Some(candidate.event_rev)
    {
        return Err(Error::Storage(
            "DL-200 Event envelope disagrees with its projection",
        ));
    }
    if marker.get("disclosure_manifest").is_some() {
        return Ok(EventAuthorization::Unknown);
    }
    if let Some(data) = marker.get("data") {
        validate_data(data).map_err(|_| Error::Storage("invalid retained DL-200 Event data"))?;
    }

    let actor = marker
        .get("actor")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .or_else(|| candidate.actor.clone());
    if let (Some(marker_actor), Some(commit_actor)) = (actor.as_deref(), candidate.actor.as_deref())
    {
        if marker_actor != commit_actor {
            return Err(Error::Storage(
                "DL-200 Event actor provenance disagrees with immutable commit actor",
            ));
        }
    }
    let turn = marker.get("turn").and_then(serde_json::Value::as_u64);
    if turn == Some(0) {
        return Err(Error::Storage("invalid DL-200 Event Turn provenance"));
    }
    let causal_hop = marker
        .get("causal_hop")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if causal_hop > 16 {
        return Err(Error::Storage("invalid DL-200 Event causal hop"));
    }
    let causal_root = marker
        .get("causal_root")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    if causal_root.as_ref().is_some_and(|root| {
        root.is_empty()
            || root.len() > 256
            || !root.is_ascii()
            || root.chars().any(char::is_control)
    }) {
        return Err(Error::Storage("invalid DL-200 Event causal root"));
    }
    Ok(EventAuthorization::Allow(EventEnvelope {
        data: marker.get("data").cloned(),
        actor,
        turn,
        causal_root,
        causal_hop,
    }))
}

pub(crate) fn validate_data(data: &serde_json::Value) -> Result<()> {
    if !data.is_object() {
        return Err(Error::Denied("Event data must be a JSON object"));
    }
    if json_depth(data, 0)? > MAX_EVENT_DATA_DEPTH {
        return Err(Error::Limit("Event data nesting exceeds 8 levels"));
    }
    let encoded =
        serde_json::to_vec(data).map_err(|_| Error::Storage("cannot encode Event data"))?;
    if encoded.len() > MAX_EVENT_DATA_BYTES {
        return Err(Error::Limit("Event data exceeds 4096 bytes"));
    }
    Ok(())
}

fn json_depth(value: &serde_json::Value, depth: usize) -> Result<usize> {
    if depth > MAX_EVENT_DATA_DEPTH {
        return Ok(depth);
    }
    match value {
        serde_json::Value::Array(values) => values.iter().try_fold(depth, |max, value| {
            Ok(max.max(json_depth(value, depth + 1)?))
        }),
        serde_json::Value::Object(values) => values.values().try_fold(depth, |max, value| {
            Ok(max.max(json_depth(value, depth + 1)?))
        }),
        _ => Ok(depth),
    }
}
