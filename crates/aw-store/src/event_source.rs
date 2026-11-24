//! Generic source-local DL-200 Event revision and immutable emission mechanics.
//!
//! Resource lifecycle remains with each source owner. This module only allocates
//! one monotonic Event revision per concrete source transition and writes bounded
//! immutable Event envelopes into the existing commit_log transaction.

use crate::{
    db, durability, event_authority, event_causal, event_projection, now, Error, PrincipalId,
    Result,
};
use aw_agent_core::EventSchemaRef;
use aw_tools::{event_subject::EventSubject, job_event::SourceEventBook};
use rusqlite::{params, Connection, OptionalExtension};
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CustomEventEmission {
    pub(crate) subject: String,
    pub(crate) event_rev: u64,
    pub(crate) causal_root: Option<String>,
    pub(crate) causal_hop: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CustomEventEmitOutcome {
    Published(CustomEventEmission),
    CausalExhausted { event_rev: u64, root: String },
}

impl CustomEventEmitOutcome {
    pub(crate) const fn event_rev(&self) -> u64 {
        match self {
            Self::Published(emission) => emission.event_rev,
            Self::CausalExhausted { event_rev, .. } => *event_rev,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_custom_event(
    connection: &Connection,
    source_ref: &str,
    actor: &PrincipalId,
    turn: Option<u64>,
    name: &str,
    data: Option<&serde_json::Value>,
    schema: Option<&EventSchemaRef>,
    disclosure_manifest: &str,
) -> Result<CustomEventEmitOutcome> {
    if connection.is_autocommit() {
        return Err(Error::Storage(
            "DL-200 Event emission requires an existing Store transaction",
        ));
    }
    if !schema_active(connection)? {
        return Err(Error::Storage("custom Event schema is not active"));
    }
    if turn == Some(0) || !name.starts_with('#') {
        return Err(Error::Denied(
            "custom Event publication requires a #name and valid Turn provenance",
        ));
    }
    if disclosure_manifest.len() != 64
        || !disclosure_manifest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Error::Storage("invalid custom Event disclosure manifest"));
    }
    let subject = EventSubject::from_resource_ref(source_ref, name)
        .map_err(|_| Error::Denied("invalid custom Event subject"))?;
    if let Some(data) = data {
        event_authority::validate_data(data)?;
    }
    if let Some(schema) = schema {
        if schema.version == 0
            || schema.extension.is_empty()
            || schema.id.is_empty()
            || schema.extension.len() > 48
            || schema.id.len() > 48
        {
            return Err(Error::Denied("invalid EventType schema identity"));
        }
    }

    let event_rev = reserve_revision(connection, source_ref, None)?;
    let causal = event_causal::descendant_event_scope(connection, source_ref, turn, actor)?;
    if let event_causal::DescendantEventScope::Exhausted { root } = causal {
        return Ok(CustomEventEmitOutcome::CausalExhausted { event_rev, root });
    }
    if let event_causal::DescendantEventScope::Inherited { root, event_hop } = &causal {
        event_causal::observe_event_hop(connection, root, *event_hop)?;
    }
    let (causal_root, causal_hop) = match &causal {
        event_causal::DescendantEventScope::Fresh => (None, 0),
        event_causal::DescendantEventScope::Inherited { root, event_hop } => {
            (Some(root.clone()), *event_hop)
        }
        event_causal::DescendantEventScope::Exhausted { .. } => unreachable!(),
    };

    let mut envelope = serde_json::json!({
        "version": event_projection::EVENT_MARKER_VERSION,
        "subject": subject.as_str(),
        "event_rev": event_rev,
        "actor": actor.as_str(),
        "causal_hop": causal_hop,
        "disclosure_manifest": disclosure_manifest
    });
    let object = envelope
        .as_object_mut()
        .expect("Event envelope fixture is always an object");
    if let Some(turn) = turn {
        object.insert("turn".into(), serde_json::Value::from(turn));
    }
    if let Some(root) = causal_root.as_deref() {
        object.insert(
            "causal_root".into(),
            serde_json::Value::String(root.to_owned()),
        );
    }
    if let Some(data) = data {
        object.insert("data".into(), data.clone());
    }
    if let Some(schema) = schema {
        object.insert(
            "schema".into(),
            serde_json::to_value(schema)
                .map_err(|_| Error::Storage("cannot encode EventType schema identity"))?,
        );
    }

    let txid = durability::tx_id()?;
    let entity_id = format!("{:x}", Sha256::digest(source_ref.as_bytes()));
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
    Ok(CustomEventEmitOutcome::Published(CustomEventEmission {
        subject: subject.as_str().to_owned(),
        event_rev,
        causal_root,
        causal_hop,
    }))
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
    let causal = event_causal::descendant_event_scope(connection, source_ref, turn, actor)?;
    if matches!(causal, event_causal::DescendantEventScope::Exhausted { .. }) {
        return Ok(Some(event_rev));
    }
    if let event_causal::DescendantEventScope::Inherited { root, event_hop } = &causal {
        event_causal::observe_event_hop(connection, root, *event_hop)?;
    }
    let txid = durability::tx_id()?;
    let entity_id = format!("{:x}", Sha256::digest(source_ref.as_bytes()));
    for event in events {
        let subject = EventSubject::from_resource_ref(source_ref, event.name)
            .map_err(|_| Error::Storage("invalid authoritative Event subject"))?;
        let (causal_root, causal_hop) = match &causal {
            event_causal::DescendantEventScope::Fresh => (None, 0),
            event_causal::DescendantEventScope::Inherited { root, event_hop } => {
                (Some(root.as_str()), *event_hop)
            }
            event_causal::DescendantEventScope::Exhausted { .. } => unreachable!(),
        };
        let mut envelope = serde_json::json!({
            "version": event_projection::EVENT_MARKER_VERSION,
            "subject": subject.as_str(),
            "event_rev": event_rev,
            "actor": actor.as_str(),
            "causal_hop": causal_hop
        });
        let object = envelope
            .as_object_mut()
            .expect("Event envelope fixture is always an object");
        if let Some(turn) = turn {
            object.insert("turn".into(), serde_json::Value::from(turn));
        }
        if let Some(root) = causal_root {
            object.insert(
                "causal_root".into(),
                serde_json::Value::String(root.to_owned()),
            );
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

pub(crate) fn legacy_delivery_admission_open(
    connection: &Connection,
    source_ref: &str,
) -> Result<bool> {
    let version: i64 = db(connection.pragma_query_value(None, "user_version", |row| row.get(0)))?;
    if version >= 54 {
        return Ok(false);
    }
    Ok(delivery_cutover_revision(connection, source_ref)?.is_none())
}

pub(crate) fn delivery_cutover_revision(
    connection: &Connection,
    source_ref: &str,
) -> Result<Option<u64>> {
    db(connection
        .query_row(
            "SELECT delivery_cutover_rev
             FROM event_sources_current WHERE source_ref=?1",
            [source_ref],
            |row| row.get(0),
        )
        .optional())
    .map(|row| row.flatten())
}

pub(crate) fn cutover_delivery_in(
    connection: &Connection,
    source_ref: &str,
    expected_next_event_rev: u64,
) -> Result<u64> {
    let current: Option<(u64, Option<u64>)> = db(connection
        .query_row(
            "SELECT next_event_rev,delivery_cutover_rev
             FROM event_sources_current WHERE source_ref=?1",
            [source_ref],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional())?;
    let Some((next_event_rev, cutover)) = current else {
        return Err(Error::Storage(
            "legacy Event source has no DL-200 source revision owner",
        ));
    };
    if next_event_rev != expected_next_event_rev {
        return Err(Error::Storage(
            "legacy and DL-200 Event source revisions diverged before cutover",
        ));
    }
    if let Some(cutover) = cutover {
        if cutover != next_event_rev {
            return Err(Error::Storage(
                "stored Event delivery cutover is inconsistent",
            ));
        }
        return Ok(cutover);
    }
    let changed = db(connection.execute(
        "UPDATE event_sources_current
         SET delivery_cutover_rev=?1,updated_at=?2
         WHERE source_ref=?3 AND next_event_rev=?1 AND delivery_cutover_rev IS NULL",
        params![next_event_rev, now(), source_ref],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict(
            "Event delivery cutover changed concurrently",
        ));
    }
    Ok(next_event_rev)
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
    let version: i64 = db(connection.pragma_query_value(None, "user_version", |row| row.get(0)))?;
    let born_cutover = (version >= 54).then_some(1u64);
    db(connection.execute(
        "INSERT OR IGNORE INTO event_sources_current(
             source_ref,next_event_rev,delivery_cutover_rev,updated_at
         ) VALUES(?1,1,?2,?3)",
        params![source_ref, born_cutover, now()],
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
