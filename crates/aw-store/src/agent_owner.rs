//! Narrow cutover seam for the durable Agent owner.
//!
//! Before DL-200 Agent-state activation, public revision remains the historical
//! Session-audit bridge and owner mutations commit exactly as before. The seam
//! is branch-bound now so the exclusive cutover can replace these mechanics
//! without another cross-crate mutation hunt or an online dual-write period.

use crate::{db, Error, Result};
use rusqlite::{Connection, Transaction};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AgentMutation<'a> {
    Created {
        agent: AgentKey<'a>,
        parent: Option<AgentKey<'a>>,
    },
    Changed(AgentKey<'a>),
}

impl<'a> AgentMutation<'a> {
    #[inline]
    fn key(self) -> AgentKey<'a> {
        match self {
            Self::Created { agent, .. } | Self::Changed(agent) => agent,
        }
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

/// Commit one logical transaction that creates or changes exact Agent owners.
///
/// Today this is exactly the existing SQLite commit. After the exclusive owner
/// cutover, this is the narrow place that can advance each listed Agent rev
/// immediately before the same commit. Callers pass stack arrays, so there is
/// no heap allocation or dynamic dispatch.
#[inline]
pub(crate) fn commit<const N: usize>(
    transaction: Transaction<'_>,
    mutations: [AgentMutation<'_>; N],
) -> Result<()> {
    debug_assert!(N > 0);
    debug_assert!(mutations.iter().all(|mutation| {
        let key = mutation.key();
        !key.session.is_empty() && !key.branch.is_empty()
    }));
    debug_assert!(mutations.iter().enumerate().all(|(index, mutation)| {
        let key = mutation.key();
        mutations[..index].iter().all(|prior| prior.key() != key)
    }));
    db(transaction.commit())
}
