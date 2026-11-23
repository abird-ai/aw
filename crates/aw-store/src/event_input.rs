//! Replay-safe typed DL-200 EventInput admission.
//!
//! Accepted EventInputs are immutable Agent input facts. This is not a pending
//! Event queue: subscription scan position remains the sole Event backlog owner.

use crate::{
    agent_owner, db,
    event_authority::{self, EventEnvelope, EventSourceEvidence},
    event_causal,
    event_placement::{self, EventSelectorPlacement},
    event_scan::{EventAuthorization, EventCandidate},
    event_subscription::{self, StoredSubscription},
    json, now,
    store_scope::StoreScope,
    Error, PrincipalId, Result,
};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest as _, Sha256};

const MAX_CAUSAL_DELIVERIES: u64 = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EventInputAdmission {
    Accepted { delivery_id: String },
    Deduplicated { delivery_id: String },
    ProvenDenied,
    AuthorizationUnavailable,
    PlacementStale,
}

pub(crate) fn admit_subscription_candidate(
    connection: &Connection,
    expected: &StoredSubscription,
    candidate: &EventCandidate,
    preliminary_source: &EventSourceEvidence,
    preliminary_envelope: &EventEnvelope,
) -> Result<EventInputAdmission> {
    let Some(current) = event_subscription::subscription_row(
        connection,
        &expected.recipient_ref,
        &expected.subject,
    )?
    else {
        return Ok(EventInputAdmission::ProvenDenied);
    };
    if current.state != "active"
        || current.generation != expected.generation
        || current.store_id != expected.store_id
        || current.restore_generation != expected.restore_generation
        || current.placement_kind != expected.placement_kind
        || current.placement_ref != expected.placement_ref
        || current.placement_token != expected.placement_token
    {
        return Ok(EventInputAdmission::ProvenDenied);
    }

    let scope = StoreScope::load(connection)?;
    if current.store_id != scope.id().as_str()
        || current.restore_generation != scope.restore_generation()
    {
        return Ok(EventInputAdmission::PlacementStale);
    }
    let placement = EventSelectorPlacement::from_stored(
        &current.placement_kind,
        current.placement_ref.clone(),
    )?;
    if current.placement_token != placement.token(&scope) {
        return Ok(EventInputAdmission::PlacementStale);
    }

    let creator = PrincipalId::new(current.created_by.clone())?;
    match event_placement::admit(connection, &creator, &placement) {
        Ok(_) => {}
        Err(Error::Denied(_)) => return Ok(EventInputAdmission::ProvenDenied),
        Err(Error::Conflict(_)) => return Ok(EventInputAdmission::AuthorizationUnavailable),
        Err(other) => return Err(other),
    }

    let Some((recipient_session, recipient_branch)) =
        agent_owner::key_for_ref_optional(connection, &current.recipient_ref)?
    else {
        return Ok(EventInputAdmission::ProvenDenied);
    };
    let recipient = agent_owner::AgentKey::new(&recipient_session, &recipient_branch);
    let (lifecycle, _) = agent_owner::lifecycle(connection, recipient)?;
    if lifecycle != "open" {
        return Ok(EventInputAdmission::ProvenDenied);
    }
    match event_subscription::authorize_management(connection, &creator, recipient) {
        Ok(_) => {}
        Err(Error::Denied(_)) => return Ok(EventInputAdmission::ProvenDenied),
        Err(Error::Conflict(_)) => return Ok(EventInputAdmission::AuthorizationUnavailable),
        Err(other) => return Err(other),
    }
    let recipient_access = match agent_owner::authorize_permission(
        connection,
        &creator,
        recipient,
        agent_owner::AgentPermission::Submit,
    ) {
        Ok(evidence) => evidence,
        Err(Error::Denied(_)) => return Ok(EventInputAdmission::ProvenDenied),
        Err(Error::Conflict(_)) => return Ok(EventInputAdmission::AuthorizationUnavailable),
        Err(other) => return Err(other),
    };

    let source_agent_ref = event_authority::source_agent_ref(&candidate.source)?;
    if source_agent_ref == current.recipient_ref {
        return Ok(EventInputAdmission::ProvenDenied);
    }
    let source_access =
        match event_authority::authorize_agent_ref(connection, &creator, &source_agent_ref)? {
            EventAuthorization::Allow(evidence) => evidence,
            EventAuthorization::Deny => return Ok(EventInputAdmission::ProvenDenied),
            EventAuthorization::Unknown => {
                return Ok(EventInputAdmission::AuthorizationUnavailable)
            }
        };
    if source_agent_ref != preliminary_source.agent_ref
        || source_access != preliminary_source.access
    {
        return Ok(EventInputAdmission::AuthorizationUnavailable);
    }

    let envelope = event_authority::candidate_envelope(candidate)?;
    if envelope != *preliminary_envelope {
        return Ok(EventInputAdmission::AuthorizationUnavailable);
    }
    let audience = match crate::history_owner::authorize_event_delivery(
        connection,
        &creator,
        &source_agent_ref,
        &current.recipient_ref,
        envelope.disclosure_manifest.as_deref(),
    )? {
        crate::history_owner::DisclosureAuthorization::Allow { audience } => audience,
        crate::history_owner::DisclosureAuthorization::Deny => {
            return Ok(EventInputAdmission::ProvenDenied)
        }
        crate::history_owner::DisclosureAuthorization::Unknown => {
            return Ok(EventInputAdmission::AuthorizationUnavailable)
        }
    };

    let retained_manifest = crate::history_owner::event_input_manifest(
        connection,
        &source_agent_ref,
        envelope.disclosure_manifest.as_deref(),
    )?;

    let delivery_id = delivery_id(
        &current.recipient_ref,
        &current.subject,
        current.generation,
        &candidate.event_id,
    );
    if let Some(existing) = existing_delivery(connection, &delivery_id)? {
        if existing.0 != current.recipient_ref
            || existing.1 != current.subject
            || existing.2 != current.generation
            || existing.3 != candidate.event_id
        {
            return Err(Error::Storage(
                "deterministic Event delivery identity collision",
            ));
        }
        return Ok(EventInputAdmission::Deduplicated { delivery_id });
    }

    let causal_root = envelope
        .causal_root
        .clone()
        .unwrap_or_else(|| candidate.event_id.clone());
    if causal_root.is_empty() || causal_root.len() > 256 {
        return Err(Error::Storage("invalid EventInput causal root"));
    }
    db(connection.execute(
        "INSERT OR IGNORE INTO event_causal_roots_current(
             root_id,max_event_hop,accepted_event_input_count,state,updated_at
         ) VALUES(?1,?2,0,'active',?3)",
        params![causal_root, envelope.causal_hop, now()],
    ))?;
    let (count, state): (u64, String) = db(connection.query_row(
        "SELECT accepted_event_input_count,state
         FROM event_causal_roots_current WHERE root_id=?1",
        [&causal_root],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    if state == "exhausted" || count >= MAX_CAUSAL_DELIVERIES {
        return Ok(EventInputAdmission::ProvenDenied);
    }
    let next_count = count
        .checked_add(1)
        .ok_or(Error::Limit("Event causal delivery count exhausted"))?;
    let next_state = if next_count == MAX_CAUSAL_DELIVERIES {
        "exhausted"
    } else {
        "active"
    };
    let budget_changed = db(connection.execute(
        "UPDATE event_causal_roots_current
         SET max_event_hop=MAX(max_event_hop,?1),
             accepted_event_input_count=?2,state=?3,updated_at=?4
         WHERE root_id=?5 AND accepted_event_input_count=?6 AND state='active'",
        params![
            envelope.causal_hop,
            next_count,
            next_state,
            now(),
            causal_root,
            count
        ],
    ))?;
    if budget_changed != 1 {
        return Err(Error::Conflict("Event causal budget changed concurrently"));
    }
    if next_count == MAX_CAUSAL_DELIVERIES {
        event_causal::record_exhausted_attention(
            connection,
            &causal_root,
            "delivery_limit",
            &creator,
            &recipient_session,
        )?;
    }

    let data_json = envelope.data.as_ref().map(json).transpose()?;
    let schema_json = envelope.schema.as_ref().map(json).transpose()?;
    db(connection.execute(
        "INSERT INTO agent_event_inputs(
             delivery_id,recipient_session_id,recipient_branch_id,recipient_ref,
             subscription_subject,subscription_generation,event_id,event_position,
             source_ref,source_agent_ref,subject,event_rev,source_actor,source_turn,
             causal_root,causal_hop,data_json,created_by,source_access_generation,
             recipient_access_generation,created_at,disclosure_manifest,event_schema_json
         ) VALUES(
             ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23
         )",
        params![
            delivery_id,
            recipient_session,
            recipient_branch,
            current.recipient_ref,
            current.subject,
            current.generation,
            candidate.event_id,
            candidate.position.get(),
            candidate.source.reference,
            source_agent_ref,
            candidate.subject,
            candidate.event_rev,
            envelope.actor,
            envelope.turn,
            causal_root,
            envelope.causal_hop,
            data_json,
            current.created_by,
            source_access.access_generation.as_slice(),
            recipient_access.access_generation.as_slice(),
            now(),
            retained_manifest,
            schema_json,
        ],
    ))?;
    for (ordinal, (principal_kind, principal_handle)) in audience.iter().enumerate() {
        db(connection.execute(
            "INSERT INTO agent_event_input_audience(
                 delivery_id,ordinal,principal_kind,principal_handle
             ) VALUES(?1,?2,?3,?4)",
            params![
                delivery_id,
                ordinal as u64,
                principal_kind,
                principal_handle
            ],
        ))?;
    }
    agent_owner::changed(connection, recipient)?;
    Ok(EventInputAdmission::Accepted { delivery_id })
}

fn delivery_id(recipient_ref: &str, subject: &str, generation: u64, event_id: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"aw-event-delivery-v1\0");
    for part in [
        recipient_ref.as_bytes(),
        subject.as_bytes(),
        event_id.as_bytes(),
    ] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    hash.update(generation.to_be_bytes());
    format!("edi_{:x}", hash.finalize())
}

fn existing_delivery(
    connection: &Connection,
    delivery_id: &str,
) -> Result<Option<(String, String, u64, String)>> {
    db(connection
        .query_row(
            "SELECT recipient_ref,subscription_subject,subscription_generation,event_id
             FROM agent_event_inputs WHERE delivery_id=?1",
            [delivery_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional())
}
