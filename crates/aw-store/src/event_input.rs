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
use std::collections::HashMap;

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
    admit_subscription_candidate_inner(
        connection,
        expected,
        candidate,
        preliminary_source,
        preliminary_envelope,
        LegacyAdmission::Automatic,
    )
}

pub(crate) fn admit_legacy_shadow_candidate(
    connection: &Connection,
    expected: &StoredSubscription,
    candidate: &EventCandidate,
    preliminary_source: &EventSourceEvidence,
    preliminary_envelope: &EventEnvelope,
) -> Result<EventInputAdmission> {
    admit_subscription_candidate_inner(
        connection,
        expected,
        candidate,
        preliminary_source,
        preliminary_envelope,
        LegacyAdmission::Forced {
            allow_pre_cutover: false,
        },
    )
}

pub(crate) fn admit_legacy_bridge_candidate(
    connection: &Connection,
    expected: &StoredSubscription,
    candidate: &EventCandidate,
    preliminary_source: &EventSourceEvidence,
    preliminary_envelope: &EventEnvelope,
) -> Result<EventInputAdmission> {
    admit_subscription_candidate_inner(
        connection,
        expected,
        candidate,
        preliminary_source,
        preliminary_envelope,
        LegacyAdmission::Forced {
            allow_pre_cutover: true,
        },
    )
}

pub(crate) fn has_legacy_shadow(
    connection: &Connection,
    recipient_ref: &str,
    subject: &str,
) -> Result<bool> {
    Ok(legacy_compat(connection, recipient_ref, subject)?
        .is_some_and(|compat| compat.shadow_existing))
}

#[derive(Clone, Copy)]
enum LegacyAdmission {
    Automatic,
    Forced { allow_pre_cutover: bool },
}

fn admit_subscription_candidate_inner(
    connection: &Connection,
    expected: &StoredSubscription,
    candidate: &EventCandidate,
    preliminary_source: &EventSourceEvidence,
    preliminary_envelope: &EventEnvelope,
    legacy_admission: LegacyAdmission,
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

    let legacy = legacy_compat(connection, &current.recipient_ref, &current.subject)?;
    let (use_legacy, allow_pre_cutover_legacy) = match legacy_admission {
        LegacyAdmission::Automatic => (
            legacy
                .as_ref()
                .is_some_and(|compat| !compat.shadow_existing),
            false,
        ),
        LegacyAdmission::Forced { allow_pre_cutover } => {
            if legacy.is_none() {
                return Err(Error::Storage(
                    "forced legacy Event admission has no compatibility sidecar",
                ));
            }
            (true, allow_pre_cutover)
        }
    };
    let creator_ref = if use_legacy {
        &legacy
            .as_ref()
            .expect("legacy admission requires compatibility state")
            .legacy_created_by
    } else {
        &current.created_by
    };
    let creator = PrincipalId::new(creator_ref.clone())?;

    if use_legacy {
        let legacy = legacy
            .as_ref()
            .expect("legacy admission requires compatibility state");
        let wake = match crate::agent_job_events::wake_admission_in(
            connection,
            &legacy.legacy_authority_ref,
        ) {
            Ok(Some(wake)) => wake,
            Ok(None) | Err(Error::Denied(_)) => return Ok(EventInputAdmission::ProvenDenied),
            Err(Error::Conflict(_)) => return Ok(EventInputAdmission::AuthorizationUnavailable),
            Err(other) => return Err(other),
        };
        if wake.recipient_ref != current.recipient_ref
            || wake.actor != creator.as_str()
            || wake.authority_ref != legacy.legacy_authority_ref
        {
            return Err(Error::Storage(
                "legacy Event compatibility wake authority identity changed",
            ));
        }
    }

    match event_placement::snapshot(connection, &creator, &placement) {
        Ok(Some(_)) => {}
        Ok(None) => return Ok(EventInputAdmission::PlacementStale),
        Err(Error::Conflict(_)) => return Ok(EventInputAdmission::AuthorizationUnavailable),
        Err(other) => return Err(other),
    }
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

    let mut source_cache =
        HashMap::<String, EventAuthorization<agent_owner::AgentAccessEvidence>>::new();
    let source_evidence = match event_authority::source_visibility(
        connection,
        &creator,
        &placement,
        &candidate.source,
        &mut source_cache,
    )? {
        EventAuthorization::Allow(evidence) => evidence,
        EventAuthorization::Deny => return Ok(EventInputAdmission::ProvenDenied),
        EventAuthorization::Unknown => return Ok(EventInputAdmission::AuthorizationUnavailable),
    };
    if source_evidence.agent_ref == current.recipient_ref {
        return Ok(EventInputAdmission::ProvenDenied);
    }
    let compare_preliminary = !use_legacy
        || legacy
            .as_ref()
            .is_some_and(|compat| !compat.shadow_existing);
    if compare_preliminary && source_evidence != *preliminary_source {
        return Ok(EventInputAdmission::AuthorizationUnavailable);
    }
    let source_agent_ref = source_evidence.agent_ref;
    let source_access = source_evidence.access;

    let preliminary_current = event_authority::candidate_envelope(candidate)?;
    if preliminary_current != *preliminary_envelope {
        return Ok(EventInputAdmission::AuthorizationUnavailable);
    }

    if use_legacy {
        let legacy = legacy
            .as_ref()
            .expect("legacy admission requires compatibility state");
        let concrete = aw_tools::event_subject::EventSubject::parse(&candidate.subject)
            .map_err(|_| Error::Storage("legacy Event compatibility subject is invalid"))?;
        if candidate.source.reference != legacy.source_ref
            || concrete.event_name() != legacy.event_name
        {
            return Err(Error::Storage(
                "legacy Event compatibility subscription escaped its exact rule",
            ));
        }
        if candidate.event_rev < legacy.cutover_rev && !allow_pre_cutover_legacy {
            return Ok(EventInputAdmission::ProvenDenied);
        }
    }

    let storage_candidate = if use_legacy {
        let legacy = legacy
            .as_ref()
            .expect("legacy admission requires compatibility state");
        canonical_legacy_candidate(
            connection,
            &legacy.source_ref,
            candidate.event_rev,
            &current.recipient_ref,
            legacy.legacy_generation,
            &legacy.mode,
        )?
        .ok_or(Error::Storage(
            "legacy Event compatibility revision has no canonical projected Event",
        ))?
    } else {
        candidate.clone()
    };
    let candidate = &storage_candidate;
    let storage_source = match event_authority::source_visibility(
        connection,
        &creator,
        &placement,
        &candidate.source,
        &mut source_cache,
    )? {
        EventAuthorization::Allow(evidence) => evidence,
        EventAuthorization::Deny => return Ok(EventInputAdmission::ProvenDenied),
        EventAuthorization::Unknown => return Ok(EventInputAdmission::AuthorizationUnavailable),
    };
    if storage_source.agent_ref != source_agent_ref || storage_source.access != source_access {
        return Ok(EventInputAdmission::AuthorizationUnavailable);
    }
    let envelope = match event_authority::candidate_authority(connection, &creator, candidate)? {
        EventAuthorization::Allow(envelope) => envelope,
        EventAuthorization::Deny => return Ok(EventInputAdmission::ProvenDenied),
        EventAuthorization::Unknown => return Ok(EventInputAdmission::AuthorizationUnavailable),
    };
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

    let delivery_id = if use_legacy {
        let legacy = legacy
            .as_ref()
            .expect("legacy admission requires compatibility state");
        legacy_delivery_id(
            &legacy.source_ref,
            candidate.event_rev,
            &current.recipient_ref,
            legacy.legacy_generation,
            &legacy.mode,
        )
    } else {
        delivery_id(
            &current.recipient_ref,
            &current.subject,
            current.generation,
            &candidate.event_id,
        )
    };
    if let Some(existing) = existing_delivery(connection, &delivery_id)? {
        if use_legacy {
            let legacy = legacy
                .as_ref()
                .expect("legacy admission requires compatibility state");
            let compatible: bool = db(connection.query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM event_legacy_input_compat
                      WHERE delivery_id=?1 AND source_ref=?2 AND event_rev=?3
                        AND recipient_ref=?4 AND legacy_generation=?5 AND mode=?6
                 )",
                params![
                    delivery_id,
                    legacy.source_ref,
                    candidate.event_rev,
                    current.recipient_ref,
                    legacy.legacy_generation,
                    legacy.mode,
                ],
                |row| row.get(0),
            ))?;
            if !compatible {
                return Err(Error::Storage(
                    "legacy Event compatibility delivery identity collision",
                ));
            }
            return Ok(EventInputAdmission::Deduplicated { delivery_id });
        }
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
            creator.as_str(),
            source_access.access_generation.as_slice(),
            recipient_access.access_generation.as_slice(),
            now(),
            retained_manifest,
            schema_json,
        ],
    ))?;
    if use_legacy {
        let legacy = legacy
            .as_ref()
            .expect("legacy admission requires compatibility state");
        db(connection.execute(
            "INSERT INTO event_legacy_input_compat(
                 delivery_id,source_ref,event_rev,recipient_ref,legacy_generation,mode,created_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                delivery_id,
                legacy.source_ref,
                candidate.event_rev,
                current.recipient_ref,
                legacy.legacy_generation,
                legacy.mode,
                now(),
            ],
        ))?;
    }
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

#[derive(Clone, Debug)]
struct LegacySubscriptionCompat {
    source_ref: String,
    event_name: String,
    legacy_generation: u64,
    mode: String,
    cutover_rev: u64,
    shadow_existing: bool,
    legacy_created_by: String,
    legacy_authority_ref: String,
}

fn canonical_legacy_candidate(
    connection: &Connection,
    source_ref: &str,
    event_rev: u64,
    recipient_ref: &str,
    legacy_generation: u64,
    mode: &str,
) -> Result<Option<EventCandidate>> {
    let mut query = db(connection.prepare(
        "SELECT event_name
         FROM event_legacy_subscription_compat
         WHERE source_ref=?1 AND recipient_ref=?2
           AND legacy_generation=?3 AND mode=?4
         ORDER BY CASE event_name
             WHEN 'result' THEN 0
             WHEN 'settled' THEN 1
             WHEN 'attention' THEN 2
             ELSE 3
         END,event_name",
    ))?;
    let rows = db(query.query_map(
        params![source_ref, recipient_ref, legacy_generation, mode],
        |row| row.get::<_, String>(0),
    ))?;
    for event_name in rows {
        let event_name = db(event_name)?;
        if let Some(candidate) = crate::event_scan::candidate_by_source_revision_name(
            connection,
            source_ref,
            event_rev,
            &event_name,
        )? {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn legacy_compat(
    connection: &Connection,
    recipient_ref: &str,
    subject: &str,
) -> Result<Option<LegacySubscriptionCompat>> {
    db(connection
        .query_row(
            "SELECT source_ref,event_name,legacy_generation,mode,cutover_rev,
                    shadow_existing,legacy_created_by,legacy_authority_ref
             FROM event_legacy_subscription_compat
             WHERE recipient_ref=?1 AND subject=?2",
            params![recipient_ref, subject],
            |row| {
                Ok(LegacySubscriptionCompat {
                    source_ref: row.get(0)?,
                    event_name: row.get(1)?,
                    legacy_generation: row.get(2)?,
                    mode: row.get(3)?,
                    cutover_rev: row.get(4)?,
                    shadow_existing: row.get::<_, i64>(5)? != 0,
                    legacy_created_by: row.get(6)?,
                    legacy_authority_ref: row.get(7)?,
                })
            },
        )
        .optional())
}

fn legacy_delivery_id(
    source_ref: &str,
    event_rev: u64,
    recipient_ref: &str,
    legacy_generation: u64,
    mode: &str,
) -> String {
    let mut hash = Sha256::new();
    hash.update(b"aw-legacy-event-delivery-v1\0");
    for part in [
        source_ref.as_bytes(),
        recipient_ref.as_bytes(),
        mode.as_bytes(),
    ] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    hash.update(event_rev.to_be_bytes());
    hash.update(legacy_generation.to_be_bytes());
    format!("edi_{:x}", hash.finalize())
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
