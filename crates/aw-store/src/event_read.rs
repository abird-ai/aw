//! Durable opaque DL-200 Event read cursors for exact Agent-scoped selectors.
//!
//! This is the first real EventScan consumer. The public page never exposes a
//! StorePos, hidden-row count, source authorization evidence or manifest identity.

use crate::{
    agent_owner, db, event_projection,
    event_scan::{
        self, EventAuthorization, EventCandidate, EventPlacement, EventScanFence, EventScanResult,
        EventSource, MAX_EVENT_SCAN_ROWS,
    },
    id, now,
    store_scope::{StorePos, StoreScope},
    Error, PrincipalId, Result, SessionParticipationAction, SessionStore,
};
use aw_tools::event_subject::{EventPattern, EventSubject};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const MAX_VISIBLE_EVENTS: usize = 256;
const MAX_CURSORS_PER_PRINCIPAL: u64 = 64;
const CURSOR_IDLE_MICROS: i64 = 24 * 60 * 60 * 1_000_000;
const EVENT_RETENTION_MICROS: i64 = 7 * 24 * 60 * 60 * 1_000_000;
const MIN_RETAINED_EVENTS: i64 = 65_536;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventReadAfter {
    Now,
    Cursor(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventReadEvent {
    pub id: String,
    pub subject: String,
    pub event_rev: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causal_root: Option<String>,
    pub causal_hop: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventReadPage {
    pub cursor: String,
    pub events: Vec<EventReadEvent>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status", content = "result")]
pub enum EventReadOutcome {
    Page(EventReadPage),
    AuthorizationUnavailable { cursor: String },
    CursorStale,
    CursorExpired,
}

struct CursorRow {
    principal: String,
    selector_sha256: [u8; 32],
    agent_ref: String,
    placement_token: [u8; 32],
    store_id: String,
    restore_generation: u64,
    position: StorePos,
    last_used_at: i64,
}

impl SessionStore {
    /// Read one bounded Event page for a selector wholly contained by one exact Agent.
    ///
    /// Higher host/SDK layers may add wait/wake behavior around this single-attempt
    /// primitive. Broad Project/Domain selectors remain rejected until the placement
    /// owner can prove one-store completeness for the caller.
    pub fn events_read_page(
        &self,
        actor: &PrincipalId,
        subject: &str,
        after: EventReadAfter,
        limit: usize,
    ) -> Result<EventReadOutcome> {
        if limit == 0 || limit > MAX_VISIBLE_EVENTS {
            return Err(Error::Limit("Event read limit must be 1..256"));
        }
        let pattern = EventPattern::parse_absolute(subject)
            .map_err(|_| Error::Denied("invalid Event subject pattern"))?;
        let agent_ref = pattern.exact_agent_ref().ok_or(Error::Denied(
            "Event selector placement is not provably single-store",
        ))?;
        let selector_sha256 = selector_hash(pattern.as_str());

        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let at = now();

        match after {
            EventReadAfter::Now => {
                cleanup_expired_cursors(&tx, at)?;
                let Some((session, branch)) = agent_owner::key_for_ref_optional(&tx, &agent_ref)?
                else {
                    return Err(Error::Denied(
                        "Event selector unavailable or not authorized",
                    ));
                };
                agent_owner::authorize_access(
                    &tx,
                    actor,
                    agent_owner::AgentKey::new(&session, &branch),
                    SessionParticipationAction::View,
                )
                .map_err(|error| match error {
                    Error::Denied(_) => {
                        Error::Denied("Event selector unavailable or not authorized")
                    }
                    other => other,
                })?;

                let live: u64 = db(tx.query_row(
                    "SELECT count(*) FROM event_read_cursors_current
                     WHERE principal=?1",
                    [actor.as_str()],
                    |row| row.get(0),
                ))?;
                if live >= MAX_CURSORS_PER_PRINCIPAL {
                    return Err(Error::Limit("Event read cursor capacity exhausted"));
                }

                let projection = event_projection::snapshot(&tx)?;
                if !projection.usable {
                    return Err(Error::Storage(
                        "Event projection is unavailable; owner normalization required",
                    ));
                }
                let token = placement_token(&projection.scope, &agent_ref);
                let cursor = id("evc")?;
                db(tx.execute(
                    "INSERT INTO event_read_cursors_current(
                         cursor_id,principal,selector_sha256,agent_ref,placement_token,
                         store_id,restore_generation,position,created_at,last_used_at
                     ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)",
                    params![
                        cursor,
                        actor.as_str(),
                        selector_sha256.as_slice(),
                        agent_ref,
                        token.as_slice(),
                        projection.scope.id().as_str(),
                        projection.scope.restore_generation(),
                        projection.indexed_through.get(),
                        at,
                    ],
                ))?;
                db(tx.commit())?;
                Ok(EventReadOutcome::Page(EventReadPage {
                    cursor,
                    events: Vec::new(),
                }))
            }
            EventReadAfter::Cursor(cursor) => {
                if cursor.is_empty()
                    || cursor.len() > 96
                    || !cursor.is_ascii()
                    || cursor.chars().any(char::is_control)
                {
                    return Err(Error::Denied("invalid Event read cursor"));
                }
                read_cursor_page(
                    tx,
                    actor,
                    &pattern,
                    &agent_ref,
                    selector_sha256,
                    cursor,
                    limit,
                    at,
                )
            }
        }
    }
}

fn read_cursor_page(
    tx: rusqlite::Transaction<'_>,
    actor: &PrincipalId,
    pattern: &EventPattern,
    agent_ref: &str,
    selector_sha256: [u8; 32],
    cursor: String,
    limit: usize,
    at: i64,
) -> Result<EventReadOutcome> {
    let Some(row) = cursor_row(&tx, &cursor)? else {
        return Ok(EventReadOutcome::CursorExpired);
    };
    if row.principal != actor.as_str()
        || row.selector_sha256 != selector_sha256
        || row.agent_ref != agent_ref
    {
        return Err(Error::Denied(
            "Event read cursor belongs to another selector",
        ));
    }
    if row.last_used_at <= at.saturating_sub(CURSOR_IDLE_MICROS) {
        db(tx.execute(
            "DELETE FROM event_read_cursors_current WHERE cursor_id=?1",
            [&cursor],
        ))?;
        db(tx.commit())?;
        return Ok(EventReadOutcome::CursorExpired);
    }

    let scope = StoreScope::load(&tx)?;
    if row.store_id != scope.id().as_str() || row.restore_generation != scope.restore_generation() {
        return Ok(EventReadOutcome::CursorStale);
    }
    let Some(agent_key) = current_agent_key(&tx, agent_ref)? else {
        return Ok(EventReadOutcome::CursorStale);
    };
    if row.placement_token != placement_token(&scope, agent_ref) {
        return Ok(EventReadOutcome::CursorStale);
    }
    if cursor_behind_retention(&tx, row.position, at)? {
        return Ok(EventReadOutcome::CursorExpired);
    }

    let fence = EventScanFence::capture(&tx)?;
    let page_authority = current_source_authority(&tx, actor, &agent_key)?;
    let original_position = row.position;
    let mut position = row.position;
    let mut examined = 0usize;
    let mut events = Vec::with_capacity(limit);

    while events.len() < limit && examined < MAX_EVENT_SCAN_ROWS {
        let remaining = MAX_EVENT_SCAN_ROWS - examined;
        let result = event_scan::scan_events(
            &tx,
            &fence,
            position,
            pattern,
            remaining,
            || placement_state(&tx, agent_ref, &agent_key),
            |source| source_visibility(agent_ref, source, &page_authority),
            |candidate, _source_evidence| candidate_authority(candidate),
        )?;
        match result {
            EventScanResult::Candidate {
                candidate,
                examined: scanned,
            } => {
                examined = examined.saturating_add(scanned);
                let event_scan::EventScanCandidate {
                    event,
                    source_evidence,
                    candidate_evidence: (),
                } = *candidate;
                match &page_authority {
                    EventAuthorization::Allow(page_evidence)
                        if *page_evidence == source_evidence => {}
                    _ => {
                        return Err(Error::Storage(
                            "EventScan source evidence disagrees with page authority",
                        ))
                    }
                }
                events.push(project_event(&event)?);
                position = event.position;
            }
            EventScanResult::ProvenSkip {
                through,
                examined: scanned,
            } => {
                examined = examined.saturating_add(scanned);
                position = through;
            }
            EventScanResult::End { through } => {
                position = through;
                break;
            }
            EventScanResult::BlockedUnknown => {
                touch_cursor(&tx, &cursor, original_position, at)?;
                db(tx.commit())?;
                return Ok(EventReadOutcome::AuthorizationUnavailable { cursor });
            }
            EventScanResult::Stale => return Ok(EventReadOutcome::CursorStale),
        }
    }

    let current_scope = StoreScope::load(&tx)?;
    if current_scope != scope {
        return Ok(EventReadOutcome::CursorStale);
    }
    match placement_state(&tx, agent_ref, &agent_key)? {
        EventPlacement::Current => {}
        EventPlacement::Stale => return Ok(EventReadOutcome::CursorStale),
        EventPlacement::Unknown => {
            touch_cursor(&tx, &cursor, original_position, at)?;
            db(tx.commit())?;
            return Ok(EventReadOutcome::AuthorizationUnavailable { cursor });
        }
    }
    if !events.is_empty() {
        let final_authority = current_source_authority(&tx, actor, &agent_key)?;
        if final_authority != page_authority {
            touch_cursor(&tx, &cursor, original_position, at)?;
            db(tx.commit())?;
            return Ok(EventReadOutcome::AuthorizationUnavailable { cursor });
        }
    }
    let changed = db(tx.execute(
        "UPDATE event_read_cursors_current
         SET position=?1,last_used_at=?2
         WHERE cursor_id=?3 AND principal=?4 AND position=?5
           AND store_id=?6 AND restore_generation=?7",
        params![
            position.get(),
            at,
            cursor,
            actor.as_str(),
            original_position.get(),
            scope.id().as_str(),
            scope.restore_generation(),
        ],
    ))?;
    if changed != 1 {
        return Ok(EventReadOutcome::CursorStale);
    }
    db(tx.commit())?;
    Ok(EventReadOutcome::Page(EventReadPage { cursor, events }))
}

fn cursor_row(connection: &Connection, cursor: &str) -> Result<Option<CursorRow>> {
    db(connection
        .query_row(
            "SELECT principal,selector_sha256,agent_ref,placement_token,store_id,
                    restore_generation,position,last_used_at
             FROM event_read_cursors_current WHERE cursor_id=?1",
            [cursor],
            |row| {
                let selector: Vec<u8> = row.get(1)?;
                let placement: Vec<u8> = row.get(3)?;
                Ok((
                    row.get::<_, String>(0)?,
                    selector,
                    row.get::<_, String>(2)?,
                    placement,
                    row.get::<_, String>(4)?,
                    row.get::<_, u64>(5)?,
                    row.get::<_, u64>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )
        .optional())?
    .map(
        |(
            principal,
            selector,
            agent_ref,
            placement,
            store_id,
            restore_generation,
            position,
            last_used_at,
        )| {
            let selector_sha256: [u8; 32] = selector
                .try_into()
                .map_err(|_| Error::Storage("invalid Event cursor selector digest"))?;
            let placement_token: [u8; 32] = placement
                .try_into()
                .map_err(|_| Error::Storage("invalid Event cursor placement token"))?;
            Ok(CursorRow {
                principal,
                selector_sha256,
                agent_ref,
                placement_token,
                store_id,
                restore_generation,
                position: StorePos::new(position),
                last_used_at,
            })
        },
    )
    .transpose()
}

fn current_agent_key(connection: &Connection, agent_ref: &str) -> Result<Option<(String, String)>> {
    agent_owner::key_for_ref_optional(connection, agent_ref)
}

fn placement_state(
    connection: &Connection,
    agent_ref: &str,
    expected: &(String, String),
) -> Result<EventPlacement> {
    match current_agent_key(connection, agent_ref) {
        Ok(Some(current)) if current == *expected => Ok(EventPlacement::Current),
        Ok(_) => Ok(EventPlacement::Stale),
        Err(Error::Conflict(_)) => Ok(EventPlacement::Unknown),
        Err(other) => Err(other),
    }
}

fn current_source_authority(
    connection: &Connection,
    actor: &PrincipalId,
    agent_key: &(String, String),
) -> Result<EventAuthorization<agent_owner::AgentAccessEvidence>> {
    match agent_owner::authorize_access(
        connection,
        actor,
        agent_owner::AgentKey::new(&agent_key.0, &agent_key.1),
        SessionParticipationAction::View,
    ) {
        Ok(evidence) => Ok(EventAuthorization::Allow(evidence)),
        Err(Error::Denied(_)) => Ok(EventAuthorization::Deny),
        Err(Error::Conflict(_)) => Ok(EventAuthorization::Unknown),
        Err(other) => Err(other),
    }
}

fn source_visibility(
    agent_ref: &str,
    source: &EventSource,
    authority: &EventAuthorization<agent_owner::AgentAccessEvidence>,
) -> Result<EventAuthorization<agent_owner::AgentAccessEvidence>> {
    let belongs = source.reference == agent_ref
        || source
            .reference
            .strip_prefix(agent_ref)
            .is_some_and(|suffix| suffix.starts_with("/t:"));
    if !belongs || source.agent.is_none() {
        return Err(Error::Storage(
            "Agent-scoped Event projection contains a source outside its Agent root",
        ));
    }
    Ok(authority.clone())
}

fn candidate_authority(candidate: &EventCandidate) -> Result<EventAuthorization<()>> {
    let concrete = EventSubject::parse(&candidate.subject)
        .map_err(|_| Error::Storage("Event projection subject is not canonical"))?;
    if concrete.source_ref() != candidate.source.reference {
        return Err(Error::Storage(
            "Event projection source disagrees with immutable subject",
        ));
    }
    let marker = event_marker(candidate)?;
    if marker.get("disclosure_manifest").is_some() {
        return Ok(EventAuthorization::Unknown);
    }
    Ok(EventAuthorization::Allow(()))
}

fn event_marker(candidate: &EventCandidate) -> Result<&serde_json::Map<String, serde_json::Value>> {
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
    Ok(marker)
}

fn project_event(candidate: &EventCandidate) -> Result<EventReadEvent> {
    let marker = event_marker(candidate)?;
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
    Ok(EventReadEvent {
        id: candidate.event_id.clone(),
        subject: candidate.subject.clone(),
        event_rev: candidate.event_rev,
        data: marker.get("data").cloned(),
        actor,
        turn,
        causal_root,
        causal_hop,
    })
}

fn selector_hash(subject: &str) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"aw-event-selector-v1\0");
    hash.update((subject.len() as u64).to_be_bytes());
    hash.update(subject.as_bytes());
    hash.finalize().into()
}

fn placement_token(scope: &StoreScope, agent_ref: &str) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"aw-event-agent-placement-v1\0");
    hash.update((scope.id().as_str().len() as u64).to_be_bytes());
    hash.update(scope.id().as_str().as_bytes());
    hash.update((agent_ref.len() as u64).to_be_bytes());
    hash.update(agent_ref.as_bytes());
    hash.finalize().into()
}

fn cleanup_expired_cursors(connection: &Connection, at: i64) -> Result<()> {
    db(connection.execute(
        "DELETE FROM event_read_cursors_current WHERE last_used_at<=?1",
        [at.saturating_sub(CURSOR_IDLE_MICROS)],
    ))?;
    Ok(())
}

fn touch_cursor(connection: &Connection, cursor: &str, position: StorePos, at: i64) -> Result<()> {
    let changed = db(connection.execute(
        "UPDATE event_read_cursors_current SET last_used_at=?1
         WHERE cursor_id=?2 AND position=?3",
        params![at, cursor, position.get()],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict("Event read cursor changed concurrently"));
    }
    Ok(())
}

fn cursor_behind_retention(connection: &Connection, position: StorePos, at: i64) -> Result<bool> {
    let count_floor: Option<u64> = db(connection
        .query_row(
            "SELECT commit_position FROM event_projection_index
             ORDER BY commit_position DESC LIMIT 1 OFFSET ?1",
            [MIN_RETAINED_EVENTS - 1],
            |row| row.get(0),
        )
        .optional())?;
    let Some(count_floor) = count_floor else {
        return Ok(false);
    };
    let cutoff = at.saturating_sub(EVENT_RETENTION_MICROS);
    let time_floor: Option<u64> = db(connection.query_row(
        "SELECT MIN(p.commit_position)
         FROM event_projection_index p
         JOIN commit_log c ON c.seq=p.commit_position
         WHERE c.created_at>=?1",
        [cutoff],
        |row| row.get(0),
    ))?;
    let retained_from = time_floor.map_or(count_floor, |time_floor| time_floor.min(count_floor));
    if position.get() >= retained_from {
        return Ok(false);
    }
    db(connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM event_projection_index
             WHERE commit_position>?1 AND commit_position<?2
         )",
        params![position.get(), retained_from],
        |row| row.get(0),
    ))
}
