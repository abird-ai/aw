//! Shared DL-200 Event retention floor.
//!
//! The retained interval is the union of the last seven days and at least the
//! newest 65,536 Event rows. Consumers own their own stale/lagged response.

use crate::{db, now, store_scope::StorePos, Result};
use rusqlite::{params, Connection, OptionalExtension};

const EVENT_RETENTION_MICROS: i64 = 7 * 24 * 60 * 60 * 1_000_000;
const MIN_RETAINED_EVENTS: i64 = 65_536;

pub(crate) fn position_is_expired(
    connection: &Connection,
    position: StorePos,
    at: i64,
) -> Result<bool> {
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

pub(crate) fn expired_now(connection: &Connection, position: StorePos) -> Result<bool> {
    position_is_expired(connection, position, now())
}
