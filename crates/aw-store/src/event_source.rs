//! Generic source-local DL-200 Event revision and immutable emission mechanics.
//!
//! Resource lifecycle remains with each source owner. This module only allocates
//! one monotonic Event revision per concrete source transition and writes bounded
//! immutable Event envelopes into the existing commit_log transaction.

use crate::{db, durability, event_authority, event_projection, now, Error, PrincipalId, Result};
use aw_tools::{event_subject::EventSubject, job_event::SourceEventBook};
use rusqlite::{params, Connection};
use sha2::{Digest as _, Sha256};

const STORE_SCHEMA_VERSION: i64 = 45;
const MAX_EVENTS_PER_TRANSITION: usize = 32;

#[derive(Clone, Copy)]
pub(crate) struct SystemEvent<'a> {
    pub(crate) name: &'a str,
    pub(crate) data: Option<&'a serde_json::Value>,
}

impl<'a> SystemEvent<'a> {
    pub(crate) const fn new(name: &'a str) -> Self {
        Self { name, data: None }
    }
}

pub(crate) fn seed_legacy_sources(connection: &Connection) -> Result<()> {
    let mut query = db(connection.prepare(
        "SELECT source_ref,event_book_json
         FROM agent_job_event_sources
         ORDER BY source_ref",
    ))?;
    let rows = db(query.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }))?;
    for row in rows {
        let (source_ref, encoded) = db(row)?;
        EventSubject::from_resource_ref(&source_ref, "changed")
            .map_err(|_| Error::Storage("legacy Event source ref is not canonical"))?;
        let book: SourceEventBook = serde_json::from_str(&encoded)
            .map_err(|_| Error::Storage("cannot decode legacy Event source revision state"))?;
        book.validate().map_err(Error::Storage)?;
        let next_event_rev = book.next_event_revision().map_err(Error::Storage)?;
        db(connection.execute(
            "INSERT INTO event_sources_current(
                 source_ref,next_event_rev,delivery_cutover_rev,updated_at
             ) VALUES(?1,?2,NULL,?3)",
            params![source_ref, next_event_rev, now()],
        ))?;
    }
    Ok(())
}

pub(crate) fn next_event_revision(connection: &Connection, source_ref: &str) -> Result<u64> {
    EventSubject::from_resource_ref(source_ref, "changed")
        .map_err(|_| Error::Storage("Event source ref is not canonical"))?;
    if !schema_active(connection)? {
        return Ok(1);
    }
    db(connection.query_row(
        "SELECT COALESCE(
             (SELECT next_event_rev FROM event_sources_current WHERE source_ref=?1),
             1
         )",
        [source_ref],
        |row| row.get(0),
    ))
}

pub(crate) fn emit_system_events(
    connection: &Connection,
    source_ref: &str,
    actor: &PrincipalId,
    turn: Option<u64>,
    events: &[SystemEvent<'_>],
    expected_revision: Option<u64>,
) -> Result<Option<u64>> {
    if events.is_empty() {
        return Ok(None);
    }
    if events.len() > MAX_EVENTS_PER_TRANSITION {
        return Err(Error::Limit("too many Events in one source transition"));
    }
    if connection.is_autocommit() {
        return Err(Error::Storage(
            "DL-200 Event emission requires an existing Store transaction",
        ));
    }
    if !schema_active(connection)? {
        return Ok(None);
    }
    if turn == Some(0) {
        return Err(Error::Storage("invalid Event Turn provenance"));
    }

    for (index, event) in events.iter().enumerate() {
        if event.name.starts_with('#') {
            return Err(Error::Denied(
                "authoritative system Event emission cannot use a custom Event name",
            ));
        }
        EventSubject::from_resource_ref(source_ref, event.name)
            .map_err(|_| Error::Storage("invalid authoritative Event subject"))?;
        if events[..index].iter().any(|prior| prior.name == event.name) {
            return Err(Error::Storage(
                "duplicate Event name in one source transition",
            ));
        }
        if let Some(data) = event.data {
            event_authority::validate_data(data)?;
        }
    }

    let event_rev = reserve_revision(connection, source_ref, expected_revision)?;
    let txid = durability::tx_id()?;
    let entity_id = format!("{:x}", Sha256::digest(source_ref.as_bytes()));
    for event in events {
        let subject = EventSubject::from_resource_ref(source_ref, event.name)
            .map_err(|_| Error::Storage("invalid authoritative Event subject"))?;
        let mut envelope = serde_json::json!({
            "version": event_projection::EVENT_MARKER_VERSION,
            "subject": subject.as_str(),
            "event_rev": event_rev,
            "actor": actor.as_str(),
            "causal_hop": 0
        });
        let object = envelope
            .as_object_mut()
            .expect("Event envelope fixture is always an object");
        if let Some(turn) = turn {
            object.insert("turn".into(), serde_json::Value::from(turn));
        }
        if let Some(data) = event.data {
            object.insert("data".into(), data.clone());
        }
        let mut payload = serde_json::Map::with_capacity(1);
        payload.insert(event_projection::EVENT_MARKER.to_owned(), envelope);
        durability::commit_event(
            connection,
            &txid,
            "event.emitted",
            "event_source",
            &entity_id,
            Some(actor),
            serde_json::Value::Object(payload),
        )?;
    }
    Ok(Some(event_rev))
}

fn schema_active(connection: &Connection) -> Result<bool> {
    let version: i64 = db(connection.pragma_query_value(None, "user_version", |row| row.get(0)))?;
    Ok(version >= STORE_SCHEMA_VERSION)
}

fn reserve_revision(
    connection: &Connection,
    source_ref: &str,
    expected_revision: Option<u64>,
) -> Result<u64> {
    EventSubject::from_resource_ref(source_ref, "changed")
        .map_err(|_| Error::Storage("Event source ref is not canonical"))?;
    db(connection.execute(
        "INSERT OR IGNORE INTO event_sources_current(
             source_ref,next_event_rev,delivery_cutover_rev,updated_at
         ) VALUES(?1,1,NULL,?2)",
        params![source_ref, now()],
    ))?;
    let current: u64 = db(connection.query_row(
        "SELECT next_event_rev FROM event_sources_current WHERE source_ref=?1",
        [source_ref],
        |row| row.get(0),
    ))?;
    if expected_revision.is_some_and(|expected| expected != current) {
        return Err(Error::Storage(
            "legacy and DL-200 Event source revisions diverged",
        ));
    }
    let next = current
        .checked_add(1)
        .filter(|value| i64::try_from(*value).is_ok())
        .ok_or(Error::Limit("Event source revision exhausted"))?;
    let changed = db(connection.execute(
        "UPDATE event_sources_current
         SET next_event_rev=?1,updated_at=?2
         WHERE source_ref=?3 AND next_event_rev=?4",
        params![next, now(), source_ref, current],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict(
            "Event source revision changed concurrently",
        ));
    }
    Ok(current)
}
