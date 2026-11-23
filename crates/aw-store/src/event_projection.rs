//! Rebuildable DL-200 Event subject projection.
//!
//! `commit_log` remains the sole durable Event/fact history owner. This module
//! owns only parsed subject lookup plus one StoreScope-bound contiguous safe
//! watermark. Missing/corrupt projection state is rebuilt from immutable rows;
//! it never invents Event truth, Event revisions or consumer progress.

use crate::{
    db,
    store_scope::{StorePos, StoreScope},
    Error, Result,
};
use aw_tools::event_subject::EventSubject;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
pub(crate) const EVENT_MARKER: &str = "_aw_dl200_event";
pub(crate) const EVENT_MARKER_VERSION: u64 = 1;
const STORE_SCHEMA_VERSION: i64 = 44;

#[derive(Clone, Copy)]
struct SubjectParts<'a> {
    source_kind: &'static str,
    domain: &'a str,
    identity: Option<&'a str>,
    project: Option<&'a str>,
    sandbox: Option<&'a str>,
    agent: Option<&'a str>,
    turn: Option<&'a str>,
    job: Option<&'a str>,
    node: Option<u32>,
}

#[derive(Clone, Debug)]
struct Projection {
    subject: EventSubject,
    event_rev: u64,
}

#[derive(Clone, Debug)]
struct ProjectionState {
    store_id: String,
    restore_generation: u64,
    generation: u64,
    indexed_through: StorePos,
    dirty: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct EventProjectionSnapshot {
    pub(crate) scope: StoreScope,
    pub(crate) generation: u64,
    pub(crate) indexed_through: StorePos,
    pub(crate) usable: bool,
}

impl ProjectionState {
    fn matches_scope(&self, scope: &StoreScope) -> bool {
        self.store_id == scope.id().as_str()
            && self.restore_generation == scope.restore_generation()
    }
}

pub(crate) fn seed(connection: &Connection) -> Result<()> {
    let existing: bool = db(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM event_projection_state WHERE singleton=1)",
        [],
        |row| row.get(0),
    ))?;
    if existing {
        return Err(Error::Storage(
            "Event projection state already exists during v44 migration",
        ));
    }
    let projected: u64 = db(connection.query_row(
        "SELECT count(*) FROM event_projection_index",
        [],
        |row| row.get(0),
    ))?;
    if projected != 0 {
        return Err(Error::Storage(
            "Event projection index unexpectedly populated during v44 migration",
        ));
    }
    let scope = StoreScope::load(connection)?;
    let tail = commit_tail(connection)?;
    db(connection.execute(
        "INSERT INTO event_projection_state(
             singleton,store_id,restore_generation,generation,indexed_through,dirty
         ) VALUES(1,?1,?2,1,?3,0)",
        params![scope.id().as_str(), scope.restore_generation(), tail.get()],
    ))?;
    Ok(())
}

pub(crate) fn is_event_payload(payload: &serde_json::Value) -> bool {
    payload.get(EVENT_MARKER).is_some()
}

pub(crate) fn project_commit_event(
    connection: &Connection,
    event_id: &str,
    commit_schema_version: i64,
    payload: &serde_json::Value,
    position: StorePos,
) -> Result<()> {
    // One binary can execute older schema migrations before v44 activates.
    // Until activation there deliberately is no DL-200 Event projection owner.
    let store_schema: i64 =
        db(connection.pragma_query_value(None, "user_version", |row| row.get(0)))?;
    if store_schema < STORE_SCHEMA_VERSION {
        return Ok(());
    }

    let state = state(connection)?;
    let scope = StoreScope::load(connection)?;
    if !state.matches_scope(&scope) {
        return Err(Error::Storage(
            "Event projection StoreScope is stale; reopen owner store to rebuild",
        ));
    }
    if state.dirty {
        return Err(Error::Storage(
            "Event projection is dirty; reopen owner store to rebuild",
        ));
    }
    let expected = state
        .indexed_through
        .get()
        .checked_add(1)
        .ok_or(Error::Limit("Event projection position exhausted"))?;
    if position.get() != expected {
        return Err(Error::Storage(
            "Event projection commit position is not contiguous",
        ));
    }

    let projected = parse_projection(commit_schema_version, payload)?;
    if let Some(projected) = projected.as_ref() {
        insert(connection, event_id, position, projected)?;
    }
    let changed = db(connection.execute(
        "UPDATE event_projection_state
         SET indexed_through=?1,dirty=0
         WHERE singleton=1 AND generation=?2 AND indexed_through=?3 AND dirty=?4",
        params![
            position.get(),
            state.generation,
            state.indexed_through.get(),
            projected.is_some()
        ],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict(
            "Event projection progress changed concurrently",
        ));
    }
    Ok(())
}

pub(crate) fn snapshot(connection: &Connection) -> Result<EventProjectionSnapshot> {
    let current = state(connection)?;
    let scope = StoreScope::load(connection)?;
    Ok(EventProjectionSnapshot {
        usable: current.matches_scope(&scope) && !current.dirty,
        scope,
        generation: current.generation,
        indexed_through: current.indexed_through,
    })
}

pub(crate) fn normalize(connection: &mut Connection) -> Result<()> {
    let current = state(connection)?;
    let scope = StoreScope::load(connection)?;
    let tail = commit_tail(connection)?;
    if current.matches_scope(&scope) && current.indexed_through == tail && !current.dirty {
        return Ok(());
    }
    rebuild(connection, current.generation)
}

fn rebuild(connection: &mut Connection, generation: u64) -> Result<()> {
    let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let scope = StoreScope::load(&tx)?;
    let tail = commit_tail(&tx)?;
    db(tx.execute("DELETE FROM event_projection_index", []))?;

    let mut query = db(tx.prepare(
        "SELECT seq,event_id,schema_version,payload_json
         FROM commit_log ORDER BY seq",
    ))?;
    let mut rows = db(query.query([]))?;
    while let Some(row) = db(rows.next())? {
        let position = StorePos::new(
            row.get::<_, u64>(0)
                .map_err(|_| Error::Storage("invalid Event projection commit position"))?,
        );
        let event_id: String = row
            .get(1)
            .map_err(|_| Error::Storage("invalid Event projection event id"))?;
        let commit_schema_version: i64 = row
            .get(2)
            .map_err(|_| Error::Storage("invalid Event projection schema version"))?;
        let payload: String = row
            .get(3)
            .map_err(|_| Error::Storage("invalid Event projection payload"))?;
        let payload: serde_json::Value = serde_json::from_str(&payload)
            .map_err(|_| Error::Storage("cannot decode Event projection payload"))?;
        if let Some(projected) = parse_projection(commit_schema_version, &payload)? {
            insert(&tx, &event_id, position, &projected)?;
        }
    }
    drop(rows);
    drop(query);

    let next_generation = generation
        .checked_add(1)
        .ok_or(Error::Limit("Event projection generation exhausted"))?;
    let changed = db(tx.execute(
        "UPDATE event_projection_state
         SET store_id=?1,restore_generation=?2,generation=?3,indexed_through=?4,dirty=0
         WHERE singleton=1 AND generation=?5",
        params![
            scope.id().as_str(),
            scope.restore_generation(),
            next_generation,
            tail.get(),
            generation
        ],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict(
            "Event projection generation changed during rebuild",
        ));
    }
    db(tx.commit())?;
    Ok(())
}

fn state(connection: &Connection) -> Result<ProjectionState> {
    let row: Option<(String, u64, u64, u64, bool)> = db(connection
        .query_row(
            "SELECT store_id,restore_generation,generation,indexed_through,dirty
             FROM event_projection_state WHERE singleton=1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional())?;
    let Some((store_id, restore_generation, generation, indexed_through, dirty)) = row else {
        return Err(Error::Storage("Event projection state missing"));
    };
    if store_id.is_empty() || restore_generation == 0 || generation == 0 {
        return Err(Error::Storage("invalid Event projection state"));
    }
    Ok(ProjectionState {
        store_id,
        restore_generation,
        generation,
        indexed_through: StorePos::new(indexed_through),
        dirty,
    })
}

fn commit_tail(connection: &Connection) -> Result<StorePos> {
    let tail: u64 = db(connection.query_row(
        "SELECT COALESCE(MAX(seq),0) FROM commit_log",
        [],
        |row| row.get(0),
    ))?;
    Ok(StorePos::new(tail))
}

fn parse_projection(
    commit_schema_version: i64,
    payload: &serde_json::Value,
) -> Result<Option<Projection>> {
    if commit_schema_version < 4 {
        return Ok(None);
    }
    let Some(value) = payload.get(EVENT_MARKER) else {
        return Ok(None);
    };
    let marker = value
        .as_object()
        .ok_or(Error::Storage("invalid DL-200 Event projection marker"))?;
    let version = marker
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or(Error::Storage("invalid DL-200 Event projection marker"))?;
    let subject = marker
        .get("subject")
        .and_then(serde_json::Value::as_str)
        .ok_or(Error::Storage("invalid DL-200 Event projection marker"))?;
    let event_rev = marker
        .get("event_rev")
        .and_then(serde_json::Value::as_u64)
        .filter(|revision| *revision > 0)
        .ok_or(Error::Storage("invalid DL-200 Event projection marker"))?;
    if version != EVENT_MARKER_VERSION {
        return Err(Error::Storage("invalid DL-200 Event projection marker"));
    }
    let subject = EventSubject::parse(subject)
        .map_err(|_| Error::Storage("invalid canonical DL-200 Event subject"))?;
    Ok(Some(Projection { subject, event_rev }))
}

fn insert(
    connection: &Connection,
    event_id: &str,
    position: StorePos,
    projected: &Projection,
) -> Result<()> {
    let parts = source_parts(projected.subject.source_ref())?;
    db(connection.execute(
        "INSERT INTO event_projection_index(
             commit_position,event_id,subject,source_ref,source_kind,
             domain_handle,identity_handle,project_handle,sandbox_handle,agent_handle,
             turn_handle,job_handle,node_coordinate,event_name,event_rev
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
        params![
            position.get(),
            event_id,
            projected.subject.as_str(),
            projected.subject.source_ref(),
            parts.source_kind,
            parts.domain,
            parts.identity,
            parts.project,
            parts.sandbox,
            parts.agent,
            parts.turn,
            parts.job,
            parts.node,
            projected.subject.event_name(),
            projected.event_rev
        ],
    ))?;
    Ok(())
}

fn source_parts(source_ref: &str) -> Result<SubjectParts<'_>> {
    let mut domain = None;
    let mut identity = None;
    let mut project = None;
    let mut sandbox = None;
    let mut agent = None;
    let mut turn = None;
    let mut job = None;
    let mut node = None;
    let mut source_kind = None;

    for segment in source_ref
        .strip_prefix('/')
        .ok_or(Error::Storage("Event source ref is not canonical"))?
        .split('/')
    {
        let (kind, value) = segment
            .split_once(':')
            .ok_or(Error::Storage("Event source ref is not canonical"))?;
        source_kind = Some(match kind {
            "d" => {
                domain = Some(value);
                "domain"
            }
            "i" => {
                identity = Some(value);
                "principal"
            }
            "p" => {
                project = Some(value);
                "project"
            }
            "s" => {
                sandbox = Some(value);
                "sandbox"
            }
            "a" => {
                agent = Some(value);
                "agent"
            }
            "t" => {
                turn = Some(value);
                "turn"
            }
            "j" => {
                job = Some(value);
                "job"
            }
            "n" => {
                node = Some(
                    value
                        .parse::<u32>()
                        .map_err(|_| Error::Storage("invalid Event source node coordinate"))?,
                );
                "node"
            }
            _ => return Err(Error::Storage("Event source ref kind is not canonical")),
        });
    }
    Ok(SubjectParts {
        source_kind: source_kind.ok_or(Error::Storage("Event source ref is empty"))?,
        domain: domain.ok_or(Error::Storage("Event source ref lacks Domain"))?,
        identity,
        project,
        sandbox,
        agent,
        turn,
        job,
        node,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::durability;

    #[test]
    fn projection_keeps_one_gap_free_scope_bound_rebuildable_prefix() {
        let store = crate::SessionStore::in_memory().unwrap();
        let mut connection = store.connection().unwrap();

        let before = state(&connection).unwrap();
        let initial_tail = commit_tail(&connection).unwrap();
        assert_eq!(before.indexed_through, initial_tail);
        assert!(!before.dirty);

        let txid = durability::tx_id().unwrap();
        durability::commit_event(
            &connection,
            &txid,
            "fixture.non_event",
            "fixture",
            "plain",
            None,
            serde_json::json!({"ordinary":true}),
        )
        .unwrap();
        let after_non_event = state(&connection).unwrap();
        assert_eq!(
            after_non_event.indexed_through.get(),
            initial_tail.get() + 1
        );
        assert!(!after_non_event.dirty);

        let subject =
            EventSubject::from_resource_ref("/d:acme/i:alice/p:foo.bar/a:2/t:7/j:test", "result")
                .unwrap();
        let txid = durability::tx_id().unwrap();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        durability::commit_event(
            &tx,
            &txid,
            "fixture.event",
            "fixture",
            "event",
            None,
            serde_json::json!({
                EVENT_MARKER:{
                    "version":EVENT_MARKER_VERSION,
                    "subject":subject.as_str(),
                    "event_rev":3
                },
                "private_fixture":"not indexed"
            }),
        )
        .unwrap();
        tx.commit().unwrap();

        let projected: (String, String, String, String, u64) = connection
            .query_row(
                "SELECT subject,source_ref,project_handle,event_name,event_rev
                 FROM event_projection_index",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            projected.0,
            "d:acme.i:alice.p:foo%2Ebar.a:2.t:7.j:test.e:result"
        );
        assert_eq!(projected.1, "/d:acme/i:alice/p:foo.bar/a:2/t:7/j:test");
        assert_eq!(projected.2, "foo.bar");
        assert_eq!(projected.3, "result");
        assert_eq!(projected.4, 3);

        let log_count: u64 = connection
            .query_row("SELECT count(*) FROM commit_log", [], |row| row.get(0))
            .unwrap();
        connection
            .execute("DELETE FROM event_projection_index", [])
            .unwrap();
        assert!(state(&connection).unwrap().dirty);
        normalize(&mut connection).unwrap();
        let rebuilt = state(&connection).unwrap();
        assert!(!rebuilt.dirty);
        assert!(rebuilt.generation > after_non_event.generation);
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM commit_log", [], |row| {
                    row.get::<_, u64>(0)
                })
                .unwrap(),
            log_count,
            "projection rebuild must not manufacture immutable facts"
        );

        let next_restore = rebuilt.restore_generation + 1;
        connection
            .execute(
                "UPDATE store_meta SET value=?1 WHERE key='restore_generation'",
                [next_restore.to_string()],
            )
            .unwrap();
        normalize(&mut connection).unwrap();
        let rebound = state(&connection).unwrap();
        assert_eq!(rebound.restore_generation, next_restore);
        assert!(rebound.generation > rebuilt.generation);
        assert!(!rebound.dirty);
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM event_projection_index", [], |row| {
                    row.get::<_, u64>(0)
                })
                .unwrap(),
            1
        );
    }
}
