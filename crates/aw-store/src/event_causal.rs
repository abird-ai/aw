//! Durable DL-200 causal execution scope.
//!
//! EventInput facts own the accepted root/hop. A bound Turn records that exact
//! parent scope on its run; descendant Event publication derives from the run.
//! No ambient task-local/global causal context is authoritative.

use crate::{agent_owner, db, durability, now, Error, PrincipalId, Result};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest as _, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CausalScope {
    pub(crate) root: String,
    /// Hop of the Event that caused this work.
    pub(crate) parent_event_hop: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DescendantEventScope {
    Fresh,
    Inherited { root: String, event_hop: u64 },
    Exhausted { root: String },
}

pub(crate) fn event_input_scope_for_turn(
    connection: &Connection,
    delivery_id: &str,
    session: &str,
    branch: &str,
) -> Result<CausalScope> {
    let row: Option<(String, u64)> = db(connection
        .query_row(
            "SELECT causal_root,causal_hop
             FROM agent_event_inputs
             WHERE delivery_id=?1
               AND recipient_session_id=?2
               AND recipient_branch_id=?3
               AND NOT EXISTS(
                   SELECT 1 FROM runs WHERE event_input_delivery_id=?1
               )",
            params![delivery_id, session, branch],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional())?;
    let Some((root, parent_event_hop)) = row else {
        return Err(Error::Conflict(
            "EventInput is unavailable, belongs to another Agent, or is already bound",
        ));
    };
    if parent_event_hop > 16 {
        return Err(Error::Storage("EventInput causal hop exceeds v1 bound"));
    }
    Ok(CausalScope {
        root,
        parent_event_hop,
    })
}

pub(crate) fn descendant_event_scope(
    connection: &Connection,
    source_ref: &str,
    turn: Option<u64>,
    actor: &PrincipalId,
) -> Result<DescendantEventScope> {
    let Some(turn) = turn else {
        return Ok(DescendantEventScope::Fresh);
    };
    let agent_ref = agent_root(source_ref)?;
    let Some((session, branch)) = agent_owner::key_for_ref_optional(connection, agent_ref)? else {
        return Err(Error::Storage(
            "causal Event source Agent is unavailable from authoritative owner",
        ));
    };
    let row: Option<(Option<String>, Option<u64>)> = db(connection
        .query_row(
            "SELECT event_causal_root,event_causal_parent_hop
             FROM runs
             WHERE session_id=?1 AND branch_id=?2 AND turn_id=?3
             ORDER BY started_at DESC LIMIT 1",
            params![session, branch, turn],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional())?;
    let Some((root, parent_hop)) = row else {
        return Ok(DescendantEventScope::Fresh);
    };
    match (root, parent_hop) {
        (None, None) => Ok(DescendantEventScope::Fresh),
        (Some(root), Some(parent_hop)) if parent_hop < 16 => Ok(DescendantEventScope::Inherited {
            root,
            event_hop: parent_hop + 1,
        }),
        (Some(root), Some(16)) => {
            record_exhausted_attention(connection, &root, "hop_limit", actor, &session)?;
            Ok(DescendantEventScope::Exhausted { root })
        }
        _ => Err(Error::Storage(
            "run causal root/hop columns are inconsistent",
        )),
    }
}

pub(crate) fn observe_event_hop(connection: &Connection, root: &str, event_hop: u64) -> Result<()> {
    if event_hop > 16 {
        return Err(Error::Storage("descendant Event hop exceeds v1 bound"));
    }
    let changed = db(connection.execute(
        "UPDATE event_causal_roots_current
         SET max_event_hop=MAX(max_event_hop,?1),updated_at=?2
         WHERE root_id=?3",
        params![event_hop, now(), root],
    ))?;
    if changed != 1 {
        return Err(Error::Storage(
            "descendant Event causal root is unavailable",
        ));
    }
    Ok(())
}

pub(crate) fn record_exhausted_attention(
    connection: &Connection,
    root: &str,
    reason: &str,
    actor: &PrincipalId,
    session: &str,
) -> Result<bool> {
    if !matches!(reason, "delivery_limit" | "hop_limit") {
        return Err(Error::Storage("invalid causal exhaustion reason"));
    }
    let inserted = db(connection.execute(
        "INSERT OR IGNORE INTO event_causal_attention(root_id,reason,created_at)
         VALUES(?1,?2,?3)",
        params![root, reason, now()],
    ))?;
    if inserted == 0 {
        return Ok(false);
    }
    let txid = durability::tx_id()?;
    let entity_id = format!("{:x}", Sha256::digest(root.as_bytes()));
    durability::commit_event(
        connection,
        &txid,
        "event.causal_exhausted",
        "event_causal_root",
        &entity_id,
        Some(actor),
        serde_json::json!({
            "session_id": session,
            "causal_root": root,
            "reason": reason,
            "non_deliverable": true
        }),
    )?;
    Ok(true)
}

fn agent_root(source_ref: &str) -> Result<&str> {
    let agent_end = ["/t:", "/j:", "/n:"]
        .into_iter()
        .filter_map(|suffix| source_ref.find(suffix))
        .min()
        .unwrap_or(source_ref.len());
    let root = &source_ref[..agent_end];
    if !root.starts_with("/d:")
        || !root.contains("/i:")
        || !root.contains("/p:")
        || !root.contains("/a:")
    {
        return Err(Error::Storage("Event source has no canonical Agent root"));
    }
    Ok(root)
}
