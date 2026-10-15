//! AW-P6-9B2B: narrow, owner-only reclamation of *provably retired* queue
//! and notice ciphertext. Never enumerate/delete orphan, provider, credential
//! or unrecognised SecretStore items: absent audit intent is not proof that a
//! blob is disposable. The original immutable event ledgers remain authority.
use crate::{
    actor_notice, actor_queue, audit, db, principal_is_session_owner, workspace_writer,
    ActorNoticeState, QueueState, SecretStore, SessionStore,
};
use aw_agent_core::{Error, PrincipalId, Result};
use rusqlite::{params, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const ACTION: &str = "actor.custody";
const MAX_RECEIPTS: usize = 8192;
const MAX_PER_SWEEP: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SweepReceipt {
    blob_ref: String,
    source: String,
    outcome: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CustodySweep {
    /// Number of retired references whose disposition was actually audited.
    pub examined: usize,
    /// Private files unlinked; NOT a forensic-erase guarantee.
    pub unlinked: usize,
    /// No file remained when checked; already removed by normal closeout.
    pub already_absent: usize,
}
fn receipts(c: &rusqlite::Connection, session: &str) -> Result<Vec<SweepReceipt>> {
    let mut query = db(c.prepare(
        "SELECT data_json FROM audit WHERE session_id=?1 AND action=?2 ORDER BY seq LIMIT 8193",
    ))?;
    let rows = db(query.query_map(params![session, ACTION], |r| r.get::<_, String>(0)))?;
    let encoded = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if encoded.len() > MAX_RECEIPTS {
        return Err(Error::Limit("Actor custody audit needs review"));
    }
    encoded
        .iter()
        .map(|s| crate::decode::<SweepReceipt>(s))
        .collect()
}
impl SessionStore {
    /// Audited, bounded retired-custody maintenance. Holds the existing SQLite
    /// writer transaction through file disposition and receipt insertion, so
    /// a concurrent revision cannot race the live-set decision. A missing
    /// ledger reference is NOT an orphan-GC authorization.
    pub fn actor_reclaim_retired_custody(
        &self,
        owner: &PrincipalId,
        session: &str,
        epoch: u64,
        limit: usize,
    ) -> Result<CustodySweep> {
        if limit == 0 || limit > MAX_PER_SWEEP {
            return Err(Error::Limit("custody sweep bound must be 1..128"));
        }
        let vault = self.local_secret_store()?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        if !principal_is_session_owner(&tx, session, owner)? {
            return Err(Error::Denied("custody sweep requires Session owner"));
        }
        let status = workspace_writer::verify_writer_epoch_for_session(&tx, session, epoch)?;
        if status.state != "active" {
            return Err(Error::Denied("custody sweep requires active writer"));
        }
        let queue = actor_queue::events(&tx, session)?;
        let notices = actor_notice::events(&tx, session)?;
        let prior = receipts(&tx, session)?;
        let mut queued = BTreeMap::new();
        let mut noticed = BTreeMap::new();
        let mut known: BTreeMap<String, String> = BTreeMap::new();
        for item in &queue {
            known.insert(item.payload_ref.clone(), "actor.queue".into());
            queued.insert(item.request.request_id.clone(), item);
        }
        for item in &notices {
            known.insert(item.payload_ref.clone(), "actor.notice".into());
            noticed.insert(item.request.request_id.clone(), item);
        }
        let mut active = BTreeSet::new();
        for item in queued.values() {
            if matches!(
                item.state,
                QueueState::Pending | QueueState::Claimed | QueueState::Accepted
            ) {
                active.insert(item.payload_ref.as_str());
            }
        }
        for item in noticed.values() {
            if matches!(
                item.state,
                ActorNoticeState::Prepared | ActorNoticeState::Accepted | ActorNoticeState::Unknown
            ) {
                active.insert(item.payload_ref.as_str());
            }
        }
        let already_reviewed: BTreeSet<_> = prior.iter().map(|r| r.blob_ref.as_str()).collect();
        let candidates: Vec<_> = known
            .into_iter()
            .filter(|(reference, _)| {
                !active.contains(reference.as_str())
                    && !already_reviewed.contains(reference.as_str())
            })
            .take(limit)
            .collect();
        if prior.len() + candidates.len() > MAX_RECEIPTS {
            return Err(Error::Limit("custody sweep requires audit review"));
        }
        let mut result = CustodySweep::default();
        for (reference, source) in candidates {
            // `exists` validates file type, private perms, bounds, symlinks
            // and the opaque ref BEFORE any unlink. Never follow a symlink.
            let exists = vault.exists(&reference)?;
            if exists {
                vault.delete(&reference)?;
                result.unlinked += 1;
            } else {
                result.already_absent += 1;
            }
            audit(
                &tx,
                session,
                owner,
                ACTION,
                serde_json::to_value(SweepReceipt {
                    blob_ref: reference,
                    source,
                    outcome: if exists { "unlinked" } else { "already_absent" }.into(),
                })
                .map_err(|_| Error::Storage("cannot encode custody audit"))?,
            )?;
            result.examined += 1;
        }
        db(tx.commit())?;
        Ok(result)
    }
}
