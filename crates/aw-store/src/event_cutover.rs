//! Final DL-200 delivery cutover from v36 Job-only wake ownership.
//!
//! The cutover is one Store transaction. Legacy source JSON remains frozen
//! compatibility truth for pre-cutover revisions only; all new delivery uses the
//! normal DL-200 subscription/EventInput owner.

use crate::{
    actor_admission, actor_queue, agent_job_events, agent_owner, db, durability, event_authority,
    event_input, event_placement::EventSelectorPlacement, event_projection, event_scan,
    event_source, event_subscription, history_owner, now, Error, PrincipalId, QueueState, Result,
};
use aw_tools::{
    event_subject::{EventPattern, EventSubject},
    job_event::{JobEventDelivery, JobEventKind, JobWakeMode, SourceEventBook},
};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, HashMap};

const MAX_LEGACY_SOURCES: usize = 16_384;
const MAX_LEGACY_SESSIONS: usize = 16_384;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyEventInputWake {
    pub delivery_id: String,
    pub recipient_ref: String,
    pub source_ref: String,
    pub event_rev: u64,
    pub mode: String,
}

#[derive(Clone)]
struct LegacySourceRow {
    source_ref: String,
    book: SourceEventBook,
    pending: Vec<JobEventDelivery>,
}

impl crate::SessionStore {
    pub fn legacy_event_delivery_targets_for_agent(
        &self,
        session: &str,
        branch: &str,
        limit: usize,
    ) -> Result<Vec<(String, String)>> {
        if limit == 0 || limit > 4096 {
            return Err(Error::Limit(
                "legacy Event compatibility target limit must be 1..4096",
            ));
        }
        let connection = self.connection()?;
        if !compat_schema_active(&connection)? {
            return Ok(Vec::new());
        }
        let mut query = db(connection.prepare(
            "SELECT DISTINCT c.recipient_ref,c.subject
             FROM event_legacy_subscription_compat c
             JOIN agent_job_event_sources s ON s.source_ref=c.source_ref
             JOIN event_subscriptions_current e
               ON e.recipient_ref=c.recipient_ref AND e.subject=c.subject
             WHERE s.session_id=?1 AND s.branch_id=?2 AND e.state='active'
             ORDER BY c.recipient_ref,c.subject LIMIT ?3",
        ))?;
        let rows = db(
            query.query_map(params![session, branch, (limit + 1) as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            }),
        )?;
        let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        if rows.len() > limit {
            return Err(Error::Limit(
                "legacy Event compatibility target bound exceeded",
            ));
        }
        Ok(rows)
    }

    pub fn legacy_event_input_wake(
        &self,
        delivery_id: &str,
    ) -> Result<Option<LegacyEventInputWake>> {
        let connection = self.connection()?;
        if !compat_schema_active(&connection)? {
            return Ok(None);
        }
        legacy_event_input_wake_in(&connection, delivery_id)
    }

    pub fn legacy_event_inputs_for_recipient(
        &self,
        recipient_ref: &str,
        limit: usize,
    ) -> Result<Vec<LegacyEventInputWake>> {
        if limit == 0 || limit > 4096 {
            return Err(Error::Limit(
                "legacy EventInput reconciliation limit must be 1..4096",
            ));
        }
        let connection = self.connection()?;
        if !compat_schema_active(&connection)? {
            return Ok(Vec::new());
        }
        let mut query = db(connection.prepare(
            "SELECT c.delivery_id,c.recipient_ref,c.source_ref,c.event_rev,c.mode
             FROM event_legacy_input_compat c
             JOIN agent_event_inputs i ON i.delivery_id=c.delivery_id
             WHERE c.recipient_ref=?1
               AND NOT EXISTS(
                   SELECT 1 FROM runs r WHERE r.event_input_delivery_id=c.delivery_id
               )
             ORDER BY i.created_at,c.event_rev,c.delivery_id LIMIT ?2",
        ))?;
        let rows = db(
            query.query_map(params![recipient_ref, (limit + 1) as i64], |row| {
                Ok(LegacyEventInputWake {
                    delivery_id: row.get(0)?,
                    recipient_ref: row.get(1)?,
                    source_ref: row.get(2)?,
                    event_rev: row.get(3)?,
                    mode: row.get(4)?,
                })
            }),
        )?;
        let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        if rows.len() > limit {
            return Err(Error::Limit(
                "legacy EventInput reconciliation bound exceeded",
            ));
        }
        Ok(rows)
    }
}

fn compat_schema_active(connection: &Connection) -> Result<bool> {
    let version: i64 = db(connection.pragma_query_value(None, "user_version", |row| row.get(0)))?;
    Ok(version >= 54)
}

fn legacy_event_input_wake_in(
    connection: &Connection,
    delivery_id: &str,
) -> Result<Option<LegacyEventInputWake>> {
    db(connection
        .query_row(
            "SELECT delivery_id,recipient_ref,source_ref,event_rev,mode
             FROM event_legacy_input_compat WHERE delivery_id=?1",
            [delivery_id],
            |row| {
                Ok(LegacyEventInputWake {
                    delivery_id: row.get(0)?,
                    recipient_ref: row.get(1)?,
                    source_ref: row.get(2)?,
                    event_rev: row.get(3)?,
                    mode: row.get(4)?,
                })
            },
        )
        .optional())
}

pub(crate) fn cutover_all_in(connection: &Connection) -> Result<()> {
    if connection.is_autocommit() {
        return Err(Error::Storage(
            "Event delivery cutover requires the schema-owner transaction",
        ));
    }
    let sources = legacy_sources(connection)?;
    for source in &sources {
        cutover_source_in(connection, source)?;
    }
    attach_accepted_legacy_input_sidecars(connection)?;
    Ok(())
}

fn legacy_sources(connection: &Connection) -> Result<Vec<LegacySourceRow>> {
    let mut query = db(connection.prepare(
        "SELECT source_ref,event_book_json,pending_deliveries_json
         FROM agent_job_event_sources ORDER BY source_ref LIMIT ?1",
    ))?;
    let rows = db(query.query_map([(MAX_LEGACY_SOURCES + 1) as i64], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    }))?;
    let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if rows.len() > MAX_LEGACY_SOURCES {
        return Err(Error::Limit(
            "legacy Event delivery source count exceeds cutover bound",
        ));
    }
    rows.into_iter()
        .map(|(source_ref, book_json, pending_json)| {
            let book: SourceEventBook = serde_json::from_str(&book_json)
                .map_err(|_| Error::Storage("cannot decode legacy Event source book"))?;
            book.validate().map_err(Error::Storage)?;
            let pending: Vec<JobEventDelivery> = serde_json::from_str(&pending_json)
                .map_err(|_| Error::Storage("cannot decode legacy Event pending deliveries"))?;
            for delivery in &pending {
                delivery.validate().map_err(Error::Storage)?;
                if delivery.source_ref != source_ref {
                    return Err(Error::Storage(
                        "legacy pending delivery escaped its source owner",
                    ));
                }
            }
            Ok(LegacySourceRow {
                source_ref,
                book,
                pending,
            })
        })
        .collect()
}

fn cutover_source_in(connection: &Connection, legacy: &LegacySourceRow) -> Result<()> {
    let expected_next = legacy.book.next_event_revision().map_err(Error::Storage)?;
    let cutover = event_source::cutover_delivery_in(connection, &legacy.source_ref, expected_next)?;

    for subscription in legacy.book.subscriptions() {
        for rule in subscription.rules.as_slice() {
            let _ = install_compat_subscription(
                connection,
                &legacy.source_ref,
                rule.event,
                &subscription.recipient_ref,
                &subscription.recipient_authority_ref,
                subscription.generation,
                effective_mode(rule.mode),
                cutover,
            )?;
        }
    }

    for delivery in &legacy.pending {
        if delivery.event_rev >= cutover {
            return Err(Error::Storage(
                "legacy pending delivery crossed the durable cutover revision",
            ));
        }
        let mode = effective_mode(delivery.mode);
        let Some(subscription) = install_compat_subscription(
            connection,
            &legacy.source_ref,
            delivery.event,
            &delivery.recipient_ref,
            &delivery.recipient_authority_ref,
            delivery.subscription_generation,
            mode,
            cutover,
        )?
        else {
            // The old runtime would ACK a delivery whose recipient/source
            // authority is now definitively unavailable. Do not preserve a
            // wake that can no longer pass current admission.
            continue;
        };
        bridge_pending_delivery(connection, delivery, mode, &subscription)?;
    }

    db(connection.execute(
        "UPDATE agent_job_event_sources
         SET pending_deliveries_json='[]',updated_at=?1
         WHERE source_ref=?2",
        params![now(), legacy.source_ref],
    ))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn install_compat_subscription(
    connection: &Connection,
    source_ref: &str,
    event: JobEventKind,
    recipient_ref: &str,
    recipient_authority_ref: &str,
    legacy_generation: u64,
    mode: JobWakeMode,
    cutover_rev: u64,
) -> Result<Option<event_subscription::StoredSubscription>> {
    if legacy_generation == 0 {
        return Err(Error::Storage(
            "legacy Event subscription generation is invalid",
        ));
    }
    let Some(wake) = agent_job_events::wake_admission_in(connection, recipient_authority_ref)?
    else {
        return Ok(None);
    };
    if wake.recipient_ref != recipient_ref || wake.authority_ref != recipient_authority_ref {
        return Err(Error::Storage(
            "legacy Event subscription wake authority identity changed",
        ));
    }
    let actor = PrincipalId::new(wake.actor)?;
    let Some((recipient_session, recipient_branch)) =
        agent_owner::key_for_ref_optional(connection, recipient_ref)?
    else {
        return Ok(None);
    };
    let recipient = agent_owner::AgentKey::new(&recipient_session, &recipient_branch);
    let (lifecycle, _) = agent_owner::lifecycle(connection, recipient)?;
    if lifecycle != "open" {
        return Ok(None);
    }
    let (access, permission) =
        match event_subscription::authorize_management(connection, &actor, recipient) {
            Ok(value) => value,
            Err(Error::Denied(_)) => return Ok(None),
            Err(other) => return Err(other),
        };

    let subject = EventSubject::from_resource_ref(source_ref, event.as_str())
        .map_err(|_| Error::Storage("legacy Event source is not canonical"))?;
    let pattern = EventPattern::parse_absolute(subject.as_str())
        .map_err(|_| Error::Storage("legacy Event compatibility pattern is invalid"))?;
    let placement = EventSelectorPlacement::from_pattern(&pattern)?;
    match crate::event_placement::admit(connection, &actor, &placement) {
        Ok(_) => {}
        Err(Error::Denied(_)) => return Ok(None),
        Err(other) => return Err(other),
    }

    let projection = event_projection::snapshot(connection)?;
    if !projection.usable {
        return Err(Error::Storage(
            "Event projection is unavailable during delivery cutover",
        ));
    }
    let token = placement.token(&projection.scope);
    if let Some(existing) =
        event_subscription::subscription_row(connection, recipient_ref, subject.as_str())?
    {
        if existing.state != "active"
            || existing.store_id != projection.scope.id().as_str()
            || existing.restore_generation != projection.scope.restore_generation()
            || existing.placement_kind != placement.kind()
            || existing.placement_ref != placement.reference()
            || existing.placement_token != token
        {
            return Err(Error::Conflict(
                "legacy Event cutover collides with an incompatible DL-200 subscription",
            ));
        }
        let compat: Option<(String, String, u64, String, u64, bool, String, String)> =
            db(connection
                .query_row(
                    "SELECT source_ref,event_name,legacy_generation,mode,cutover_rev,
                            shadow_existing,legacy_created_by,legacy_authority_ref
                     FROM event_legacy_subscription_compat
                     WHERE recipient_ref=?1 AND subject=?2",
                    params![recipient_ref, subject.as_str()],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get::<_, i64>(5)? != 0,
                            row.get(6)?,
                            row.get(7)?,
                        ))
                    },
                )
                .optional())?;
        if let Some(compat) = compat {
            let expected = (
                source_ref.to_owned(),
                event.as_str().to_owned(),
                legacy_generation,
                mode.as_str().to_owned(),
                cutover_rev,
            );
            if (compat.0, compat.1, compat.2, compat.3, compat.4) != expected
                || compat.6 != actor.as_str()
                || compat.7 != recipient_authority_ref
                || (!compat.5
                    && (existing.created_by != actor.as_str()
                        || existing.generation != legacy_generation))
            {
                return Err(Error::Conflict(
                    "legacy Event compatibility subscription identity changed",
                ));
            }
        } else {
            db(connection.execute(
                "INSERT INTO event_legacy_subscription_compat(
                     recipient_ref,subject,source_ref,event_name,legacy_generation,mode,
                     cutover_rev,shadow_existing,legacy_created_by,legacy_authority_ref,created_at
                 ) VALUES(?1,?2,?3,?4,?5,?6,?7,1,?8,?9,?10)",
                params![
                    recipient_ref,
                    subject.as_str(),
                    source_ref,
                    event.as_str(),
                    legacy_generation,
                    mode.as_str(),
                    cutover_rev,
                    actor.as_str(),
                    recipient_authority_ref,
                    now(),
                ],
            ))?;
        }
        return Ok(Some(existing));
    }

    let baseline = projection.indexed_through.get();
    let at = now();
    db(connection.execute(
        "INSERT INTO event_subscriptions_current(
             recipient_ref,subject,placement_kind,placement_ref,placement_token,
             store_id,restore_generation,generation,baseline_position,scan_position,
             created_by,authority_ref,recipient_permission,recipient_access_generation,
             recipient_grant_revision,recipient_acl_revision,recipient_is_owner,
             state,lag_reason,created_at,updated_at
         ) VALUES(
             ?1,?2,?3,?4,?5,?6,?7,?8,?9,?9,
             ?10,?11,?12,?13,?14,?15,?16,'active',NULL,?17,?17
         )",
        params![
            recipient_ref,
            subject.as_str(),
            placement.kind(),
            placement.reference(),
            token.as_slice(),
            projection.scope.id().as_str(),
            projection.scope.restore_generation(),
            legacy_generation,
            baseline,
            actor.as_str(),
            recipient_authority_ref,
            permission,
            access.access_generation.as_slice(),
            access.grant_revision,
            access.acl_revision,
            i64::from(access.owner),
            at,
        ],
    ))?;
    db(connection.execute(
        "INSERT INTO event_legacy_subscription_compat(
             recipient_ref,subject,source_ref,event_name,legacy_generation,mode,
             cutover_rev,shadow_existing,legacy_created_by,legacy_authority_ref,created_at
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,0,?8,?9,?10)",
        params![
            recipient_ref,
            subject.as_str(),
            source_ref,
            event.as_str(),
            legacy_generation,
            mode.as_str(),
            cutover_rev,
            actor.as_str(),
            recipient_authority_ref,
            at,
        ],
    ))?;
    event_subscription::subscription_row(connection, recipient_ref, subject.as_str())
}

fn bridge_pending_delivery(
    connection: &Connection,
    delivery: &JobEventDelivery,
    mode: JobWakeMode,
    subscription: &event_subscription::StoredSubscription,
) -> Result<()> {
    let candidate = ensure_legacy_candidate(connection, delivery)?;
    let legacy_creator: String = db(connection.query_row(
        "SELECT legacy_created_by FROM event_legacy_subscription_compat
         WHERE recipient_ref=?1 AND subject=?2",
        params![subscription.recipient_ref, subscription.subject],
        |row| row.get(0),
    ))?;
    let creator = PrincipalId::new(legacy_creator)?;
    let placement = EventSelectorPlacement::from_stored(
        &subscription.placement_kind,
        subscription.placement_ref.clone(),
    )?;
    let mut cache =
        HashMap::<String, event_scan::EventAuthorization<agent_owner::AgentAccessEvidence>>::new();
    let source = match event_authority::source_visibility(
        connection,
        &creator,
        &placement,
        &candidate.source,
        &mut cache,
    )? {
        event_scan::EventAuthorization::Allow(evidence) => evidence,
        event_scan::EventAuthorization::Deny => return Ok(()),
        event_scan::EventAuthorization::Unknown => {
            return Err(Error::Conflict(
                "legacy pending Event source authority is unavailable at cutover",
            ))
        }
    };
    let envelope = match event_authority::candidate_authority(connection, &creator, &candidate)? {
        event_scan::EventAuthorization::Allow(envelope) => envelope,
        event_scan::EventAuthorization::Deny => return Ok(()),
        event_scan::EventAuthorization::Unknown => {
            return Err(Error::Conflict(
                "legacy pending Event disclosure authority is unavailable at cutover",
            ))
        }
    };

    // The compat sidecar stores the normalized scheduling hint. Verify this
    // exact pending identity resolves to the same translated rule.
    let stored_mode: String = db(connection.query_row(
        "SELECT mode FROM event_legacy_subscription_compat
         WHERE recipient_ref=?1 AND subject=?2",
        params![subscription.recipient_ref, subscription.subject],
        |row| row.get(0),
    ))?;
    if stored_mode != mode.as_str() {
        return Err(Error::Storage(
            "legacy pending Event mode disagrees with translated subscription",
        ));
    }

    match event_input::admit_legacy_bridge_candidate(
        connection,
        subscription,
        &candidate,
        &source,
        &envelope,
    )? {
        event_input::EventInputAdmission::Accepted { .. }
        | event_input::EventInputAdmission::Deduplicated { .. }
        | event_input::EventInputAdmission::ProvenDenied => Ok(()),
        event_input::EventInputAdmission::AuthorizationUnavailable
        | event_input::EventInputAdmission::PlacementStale => Err(Error::Conflict(
            "legacy pending Event cannot be safely bridged at cutover",
        )),
    }
}

fn ensure_legacy_candidate(
    connection: &Connection,
    delivery: &JobEventDelivery,
) -> Result<event_scan::EventCandidate> {
    if let Some(candidate) = event_scan::candidate_by_source_revision_name(
        connection,
        &delivery.source_ref,
        delivery.event_rev,
        delivery.event.as_str(),
    )? {
        return Ok(candidate);
    }

    let subject = EventSubject::from_resource_ref(&delivery.source_ref, delivery.event.as_str())
        .map_err(|_| Error::Storage("legacy bridge Event source is not canonical"))?;
    let pattern = EventPattern::parse_absolute(subject.as_str())
        .map_err(|_| Error::Storage("legacy bridge Event subject is invalid"))?;
    let source_agent_ref = pattern
        .exact_agent_ref()
        .ok_or(Error::Storage("legacy Job Event source has no Agent root"))?;
    let manifest = history_owner::event_input_manifest(connection, &source_agent_ref, None)?;
    let mut envelope = serde_json::json!({
        "version": event_projection::EVENT_MARKER_VERSION,
        "subject": subject.as_str(),
        "event_rev": delivery.event_rev,
        "causal_hop": 0,
        "disclosure_manifest": manifest,
    });
    if let Some(turn) = turn_from_source_ref(&delivery.source_ref)? {
        envelope
            .as_object_mut()
            .expect("legacy bridge envelope is an object")
            .insert("turn".into(), serde_json::Value::from(turn));
    }
    let mut payload = serde_json::Map::with_capacity(1);
    payload.insert(event_projection::EVENT_MARKER.to_owned(), envelope);

    let event_id = legacy_event_id(
        &delivery.source_ref,
        delivery.event_rev,
        delivery.event.as_str(),
    );
    let txid = durability::tx_id()?;
    let entity_id = format!("{:x}", Sha256::digest(delivery.source_ref.as_bytes()));
    durability::commit_event_position_with_id(
        connection,
        &event_id,
        &txid,
        "event.legacy_bridged",
        "event_source",
        &entity_id,
        None,
        serde_json::Value::Object(payload),
    )?;
    event_scan::candidate_by_source_revision_name(
        connection,
        &delivery.source_ref,
        delivery.event_rev,
        delivery.event.as_str(),
    )?
    .ok_or(Error::Storage(
        "legacy bridge Event was not projected atomically",
    ))
}

fn legacy_event_id(source_ref: &str, event_rev: u64, event_name: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"aw-legacy-event-fact-v1\0");
    for part in [source_ref.as_bytes(), event_name.as_bytes()] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    hash.update(event_rev.to_be_bytes());
    format!("evl_{:x}", hash.finalize())
}

fn turn_from_source_ref(source_ref: &str) -> Result<Option<u64>> {
    for segment in source_ref.split('/') {
        if let Some(turn) = segment.strip_prefix("t:") {
            let turn = turn
                .parse::<u64>()
                .ok()
                .filter(|value| *value != 0 && value.to_string() == turn)
                .ok_or(Error::Storage("legacy Event Turn source is invalid"))?;
            return Ok(Some(turn));
        }
    }
    Ok(None)
}

fn effective_mode(mode: JobWakeMode) -> JobWakeMode {
    match mode {
        JobWakeMode::Auto | JobWakeMode::Queue => JobWakeMode::Queue,
        JobWakeMode::Steer => JobWakeMode::Steer,
        JobWakeMode::Interrupt => JobWakeMode::Interrupt,
    }
}

fn attach_accepted_legacy_input_sidecars(connection: &Connection) -> Result<()> {
    let source_roots = legacy_source_hashes(connection)?;
    let mut query = db(connection.prepare("SELECT id FROM sessions ORDER BY id LIMIT ?1"))?;
    let rows = db(query.query_map([(MAX_LEGACY_SESSIONS + 1) as i64], |row| {
        row.get::<_, String>(0)
    }))?;
    let sessions = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if sessions.len() > MAX_LEGACY_SESSIONS {
        return Err(Error::Limit(
            "legacy Event Input Session count exceeds cutover bound",
        ));
    }

    for session in sessions {
        let mut queues = BTreeMap::new();
        for fact in actor_queue::events(connection, &session)? {
            queues.insert(fact.request.request_id.clone(), fact);
        }
        if !queues.keys().any(|id| id.starts_with("jc_")) {
            continue;
        }
        let mut inputs = BTreeMap::new();
        for fact in actor_admission::events(connection, &session)? {
            inputs.insert(fact.admission.request_id.clone(), fact);
        }
        for (queue_id, queue) in queues {
            if !queue_id.starts_with("jc_") || queue.state == QueueState::Pending {
                continue;
            }
            let input_id = actor_queue::queued_input_request_id(&session, &queue_id);
            let Some(input) = inputs.get(&input_id) else {
                continue;
            };
            if !matches!(
                input.state,
                actor_admission::InputAdmissionState::Accepted
                    | actor_admission::InputAdmissionState::Dispatching
                    | actor_admission::InputAdmissionState::Settled
                    | actor_admission::InputAdmissionState::Unknown
                    | actor_admission::InputAdmissionState::Inspected
            ) {
                continue;
            }
            attach_legacy_input_sidecar(
                connection,
                &queue,
                input,
                source_roots
                    .get(&queue.request.sender)
                    .and_then(|root| root.as_deref()),
            )?;
        }
    }
    Ok(())
}

fn legacy_source_hashes(connection: &Connection) -> Result<HashMap<String, Option<String>>> {
    let mut query = db(connection
        .prepare("SELECT source_ref FROM event_sources_current ORDER BY source_ref LIMIT ?1"))?;
    let rows = db(query.query_map([(MAX_LEGACY_SOURCES + 1) as i64], |row| {
        row.get::<_, String>(0)
    }))?;
    let sources = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if sources.len() > MAX_LEGACY_SOURCES {
        return Err(Error::Limit(
            "Event source count exceeds legacy provenance bridge bound",
        ));
    }
    let mut map = HashMap::<String, Option<String>>::new();
    for source_ref in sources {
        let subject = EventSubject::from_resource_ref(&source_ref, "result")
            .map_err(|_| Error::Storage("stored Event source ref is not canonical"))?;
        let pattern = EventPattern::parse_absolute(subject.as_str())
            .map_err(|_| Error::Storage("stored Event source subject is invalid"))?;
        let root = pattern.exact_agent_ref();
        let sender = format!("job-event:{:x}", Sha256::digest(source_ref.as_bytes()));
        map.entry(sender)
            .and_modify(|existing| {
                if *existing != root {
                    *existing = None;
                }
            })
            .or_insert(root);
    }
    Ok(map)
}

fn attach_legacy_input_sidecar(
    connection: &Connection,
    queue: &crate::ActorQueueFact,
    input: &crate::ActorInputFact,
    source_agent_ref: Option<&str>,
) -> Result<()> {
    if input.admission.session_id != queue.request.session_id
        || input.admission.branch_id != queue.request.branch_id
    {
        return Err(Error::Storage(
            "legacy Event queue and accepted Input disagree on recipient",
        ));
    }
    let existing: Option<(String, String, u64)> = db(connection
        .query_row(
            "SELECT queue_request_id,branch_id,turn_id
             FROM legacy_event_input_compat WHERE input_request_id=?1",
            [&input.admission.request_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional())?;
    if let Some(existing) = existing {
        if existing
            != (
                queue.request.request_id.clone(),
                input.admission.branch_id.clone(),
                input.admission.turn_id,
            )
        {
            return Err(Error::Storage(
                "legacy Event Input compatibility identity changed",
            ));
        }
        return Ok(());
    }

    let domain: String = db(connection.query_row(
        "SELECT w.domain_id
         FROM sessions s
         JOIN workspaces w ON w.workspace_id=s.workspace_id
         WHERE s.id=?1",
        [&input.admission.session_id],
        |row| row.get(0),
    ))?;
    let factual_actor = PrincipalId::new(input.admission.sender.clone())?;
    let factual_user = crate::user_for_principal(connection, &factual_actor)?.ok_or(
        Error::Storage("legacy accepted Event Input actor has no stable user identity"),
    )?;
    let frozen_audience = [("user".to_owned(), factual_user)];

    let manifest = history_owner::legacy_protected_input_manifest(
        connection,
        &input.admission.request_id,
        source_agent_ref,
    )?;
    db(connection.execute(
        "INSERT INTO legacy_event_input_compat(
             input_request_id,queue_request_id,session_id,branch_id,turn_id,
             source_agent_ref,manifest_digest,created_at
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            input.admission.request_id,
            queue.request.request_id,
            input.admission.session_id,
            input.admission.branch_id,
            input.admission.turn_id,
            source_agent_ref,
            manifest,
            now(),
        ],
    ))?;
    for (ordinal, (principal_kind, principal_handle)) in frozen_audience.iter().enumerate() {
        db(connection.execute(
            "INSERT INTO legacy_event_input_compat_audience(
                 input_request_id,ordinal,domain_handle,principal_kind,principal_handle
             ) VALUES(?1,?2,?3,?4,?5)",
            params![
                input.admission.request_id,
                ordinal as u64,
                domain,
                principal_kind,
                principal_handle,
            ],
        ))?;
    }
    Ok(())
}
