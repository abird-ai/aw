//! Durable opaque DL-200 Event read cursors for exact Agent-scoped selectors.
//!
//! This is the first real EventScan consumer. The public page never exposes a
//! StorePos, hidden-row count, source authorization evidence or manifest identity.

use crate::{
    agent_owner, db,
    event_authority::{self, EventEnvelope},
    event_placement::{self, EventSelectorPlacement},
    event_projection, event_retention,
    event_scan::{
        self, EventAuthorization, EventCandidate, EventPlacement, EventScanFence, EventScanResult,
        MAX_EVENT_SCAN_ROWS,
    },
    id, now,
    store_scope::{StorePos, StoreScope},
    Error, PrincipalId, Result, SessionStore,
};
use aw_agent_core::EventSchemaRef;
use aw_tools::event_subject::EventPattern;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;

const MAX_VISIBLE_EVENTS: usize = 256;
const MAX_CURSORS_PER_PRINCIPAL: u64 = 64;
const CURSOR_IDLE_MICROS: i64 = 24 * 60 * 60 * 1_000_000;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<EventSchemaRef>,
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
    placement_kind: String,
    placement_ref: String,
    placement_token: [u8; 32],
    store_id: String,
    restore_generation: u64,
    position: StorePos,
    last_used_at: i64,
}

impl SessionStore {
    /// Wait for one authorized Event page without polling.
    ///
    /// The process-local watch is only a wake accelerator. The durable cursor and
    /// Event projection are re-read after registration and once more at timeout,
    /// so missed/external-process wakes cannot become durable loss.
    pub async fn events_read(
        &self,
        actor: &PrincipalId,
        subject: &str,
        after: EventReadAfter,
        limit: usize,
        wait_ms: u64,
    ) -> Result<EventReadOutcome> {
        if wait_ms > 30_000 {
            return Err(Error::Limit("Event read wait_ms must be 0..30000"));
        }
        if wait_ms == 0 {
            return self.events_read_page(actor, subject, after, limit);
        }

        let mut wake = self.event_wake_receiver();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(wait_ms);
        let mut after = after;
        loop {
            let outcome = self.events_read_page(actor, subject, after.clone(), limit)?;
            let EventReadOutcome::Page(page) = outcome else {
                return Ok(outcome);
            };
            if !page.events.is_empty() {
                return Ok(EventReadOutcome::Page(page));
            }
            after = EventReadAfter::Cursor(page.cursor.clone());

            match tokio::time::timeout_at(deadline, wake.changed()).await {
                Ok(Ok(())) => continue,
                Ok(Err(_)) | Err(_) => {
                    // Final durable recheck covers timeout races and Event commits
                    // made through another trusted-local process.
                    return self.events_read_page(actor, subject, after, limit);
                }
            }
        }
    }

    /// Read one bounded Event page for a selector wholly contained by one exact Agent.
    ///
    /// Broad Project/Domain selectors remain rejected until the placement owner
    /// can prove one-store completeness for the caller.
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
        let placement = EventSelectorPlacement::from_pattern(&pattern)?;
        let selector_sha256 = selector_hash(pattern.as_str());

        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let at = now();

        match after {
            EventReadAfter::Now => {
                cleanup_expired_cursors(&tx, actor, at)?;
                let _placement_snapshot = event_placement::admit(&tx, actor, &placement)?;

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
                let token = placement.token(&projection.scope);
                let cursor = id("evc")?;
                db(tx.execute(
                    "INSERT INTO event_read_cursors_current(
                         cursor_id,principal,selector_sha256,placement_ref,placement_token,
                         store_id,restore_generation,position,created_at,last_used_at,placement_kind
                     ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9,?10)",
                    params![
                        cursor,
                        actor.as_str(),
                        selector_sha256.as_slice(),
                        placement.reference(),
                        token.as_slice(),
                        projection.scope.id().as_str(),
                        projection.scope.restore_generation(),
                        projection.indexed_through.get(),
                        at,
                        placement.kind(),
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
                    &placement,
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
    placement: &EventSelectorPlacement,
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
        || row.placement_kind != placement.kind()
        || row.placement_ref != placement.reference()
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
    let Some(placement_snapshot) = event_placement::snapshot(&tx, placement)? else {
        return Ok(EventReadOutcome::CursorStale);
    };
    if row.placement_token != placement.token(&scope) {
        return Ok(EventReadOutcome::CursorStale);
    }
    if event_retention::position_is_expired(&tx, row.position, at)? {
        return Ok(EventReadOutcome::CursorExpired);
    }

    let fence = EventScanFence::capture(&tx)?;
    let original_position = row.position;
    let mut position = row.position;
    let mut examined = 0usize;
    let mut events = Vec::with_capacity(limit);
    let mut authority_cache =
        HashMap::<String, EventAuthorization<agent_owner::AgentAccessEvidence>>::new();
    let mut visible_evidence = HashMap::<String, agent_owner::AgentAccessEvidence>::new();

    while events.len() < limit && examined < MAX_EVENT_SCAN_ROWS {
        let remaining = MAX_EVENT_SCAN_ROWS - examined;
        let result = event_scan::scan_events(
            &tx,
            &fence,
            position,
            pattern,
            remaining,
            || event_placement::state(&tx, placement, &placement_snapshot),
            |source| {
                event_authority::source_visibility(
                    &tx,
                    actor,
                    placement,
                    source,
                    &mut authority_cache,
                )
            },
            |candidate, _source_evidence| {
                event_authority::candidate_authority(&tx, actor, candidate)
            },
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
                    candidate_evidence,
                } = *candidate;
                visible_evidence
                    .entry(source_evidence.agent_ref)
                    .or_insert(source_evidence.access);
                events.push(project_event(&event, candidate_evidence));
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
    match event_placement::state(&tx, placement, &placement_snapshot)? {
        EventPlacement::Current => {}
        EventPlacement::Stale => return Ok(EventReadOutcome::CursorStale),
        EventPlacement::Unknown => {
            touch_cursor(&tx, &cursor, original_position, at)?;
            db(tx.commit())?;
            return Ok(EventReadOutcome::AuthorizationUnavailable { cursor });
        }
    }
    for (agent_ref, expected) in visible_evidence {
        match event_authority::authorize_agent_ref(&tx, actor, &agent_ref)? {
            EventAuthorization::Allow(current) if current == expected => {}
            _ => {
                touch_cursor(&tx, &cursor, original_position, at)?;
                db(tx.commit())?;
                return Ok(EventReadOutcome::AuthorizationUnavailable { cursor });
            }
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
            "SELECT principal,selector_sha256,placement_kind,placement_ref,placement_token,store_id,
                    restore_generation,position,last_used_at
             FROM event_read_cursors_current WHERE cursor_id=?1",
            [cursor],
            |row| {
                let selector: Vec<u8> = row.get(1)?;
                let placement: Vec<u8> = row.get(4)?;
                Ok((
                    row.get::<_, String>(0)?,
                    selector,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    placement,
                    row.get::<_, String>(5)?,
                    row.get::<_, u64>(6)?,
                    row.get::<_, u64>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            },
        )
        .optional())?
    .map(
        |(
            principal,
            selector,
            placement_kind,
            placement_ref,
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
            if !matches!(placement_kind.as_str(), "agent" | "project") {
                return Err(Error::Storage("invalid Event cursor placement kind"));
            }
            Ok(CursorRow {
                principal,
                selector_sha256,
                placement_kind,
                placement_ref,
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

fn project_event(candidate: &EventCandidate, envelope: EventEnvelope) -> EventReadEvent {
    EventReadEvent {
        id: candidate.event_id.clone(),
        subject: candidate.subject.clone(),
        event_rev: candidate.event_rev,
        data: envelope.data,
        actor: envelope.actor,
        turn: envelope.turn,
        causal_root: envelope.causal_root,
        causal_hop: envelope.causal_hop,
        schema: envelope.schema,
    }
}

fn selector_hash(subject: &str) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"aw-event-selector-v1\0");
    hash.update((subject.len() as u64).to_be_bytes());
    hash.update(subject.as_bytes());
    hash.finalize().into()
}

fn cleanup_expired_cursors(connection: &Connection, actor: &PrincipalId, at: i64) -> Result<()> {
    db(connection.execute(
        "DELETE FROM event_read_cursors_current
         WHERE principal=?1 AND last_used_at<=?2",
        params![actor.as_str(), at.saturating_sub(CURSOR_IDLE_MICROS)],
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
