//! Narrow cutover seam for the durable Agent owner.
//!
//! Before DL-200 Agent-state activation, public revision remains the historical
//! Session-audit bridge and owner mutations commit exactly as before. The seam
//! is branch-bound now so the exclusive cutover can replace these mechanics
//! without another cross-crate mutation hunt or an online dual-write period.

use crate::{db, Error, Result};
use rusqlite::Connection;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AgentKey<'a> {
    pub(crate) session: &'a str,
    pub(crate) branch: &'a str,
}

impl<'a> AgentKey<'a> {
    #[inline]
    pub(crate) const fn new(session: &'a str, branch: &'a str) -> Self {
        Self { session, branch }
    }
}

/// Legacy revision bridge used until the exclusive Agent-owner activation.
///
/// `branch` is intentionally part of the seam even though the old revision is
/// Session-wide. After activation this same call becomes exact-branch owner state.
pub(crate) fn revision(connection: &Connection, agent: AgentKey<'_>) -> Result<u64> {
    debug_assert!(!agent.branch.is_empty());
    let seq: u64 = db(connection.query_row(
        "SELECT COALESCE(MAX(seq),0) FROM audit WHERE session_id=?1",
        [agent.session],
        |row| row.get(0),
    ))?;
    seq.checked_add(1)
        .ok_or(Error::Limit("Agent public revision exhausted"))
}

#[inline]
fn validate(agent: AgentKey<'_>) {
    debug_assert!(!agent.session.is_empty() && !agent.branch.is_empty());
}

/// Mark one exact Agent creation inside its existing owner transaction.
///
/// This is deliberately zero-work before the exclusive owner activation. The
/// cutover replaces this body with the authoritative row insert, preserving the
/// caller's existing transaction and publication boundary.
#[inline]
pub(crate) fn created(
    _connection: &Connection,
    agent: AgentKey<'_>,
    parent: Option<AgentKey<'_>>,
) -> Result<()> {
    validate(agent);
    if let Some(parent) = parent {
        validate(parent);
        debug_assert_ne!(agent, parent);
    }
    Ok(())
}

/// Mark one exact Agent-owned state change inside the caller's transaction.
///
/// Today this compiles to no persistence work. After activation it becomes the
/// single exact-branch `rev` bump used by all existing mutation transactions.
#[inline]
pub(crate) fn changed(_connection: &Connection, agent: AgentKey<'_>) -> Result<()> {
    validate(agent);
    Ok(())
}
