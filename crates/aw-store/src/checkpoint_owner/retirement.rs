//! Logical retirement and exact retained-anchor reconciliation.
//! The retired row + immutable anchors own cleanup intent, not a new ledger.
use super::{
    row_in, snapshot_in, sources_in, CheckpointLifecycle, CheckpointRow, CheckpointSnapshot,
};
use crate::{
    agent_owner::{self, AgentKey, AgentPermission},
    construction_receipt, db, durability, label, Error, Result, SessionStore,
    SourceMutationOutcome, SourceMutationSet, StoreMode,
};
use aw_agent_core::{PrincipalId, SourceSet};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

pub(crate) const RETENTION_CLEANUP_PREFIX: &str = "checkpoint-retention:";

fn require_manager(c: &Connection, actor: &PrincipalId, checkpoint: &str) -> Result<CheckpointRow> {
    let row = row_in(c, checkpoint)?;
    let key = AgentKey::new(&row.session, &row.branch);
    agent_owner::authorize_permission(c, actor, key, AgentPermission::Manage)?;
    if agent_owner::canonical_ref(c, key)? != row.snapshot.source_agent_ref {
        return Err(Error::Storage("checkpoint source Agent identity changed"));
    }
    Ok(row)
}

fn require_no_restore(c: &Connection, store: &SessionStore, checkpoint: &str) -> Result<()> {
    // An unresolved restore retains the original immutable checkpoint and
    // history base. Do not retire its last durable history root or discard
    // anchor descriptors while the original operation still needs them.
    let registry = store.inner.source_drivers.reader()?;
    construction_receipt::visit_unresolved_sources(c, &registry, |receipt, _, _| {
        if receipt
            .restore_boundary
            .as_ref()
            .is_some_and(|boundary| boundary.checkpoint() == checkpoint)
        {
            return Err(Error::Conflict("checkpoint has an unresolved restore"));
        }
        Ok(())
    })
}

fn retained_in(c: &Connection, row: &CheckpointRow) -> Result<Option<SourceSet>> {
    if row.snapshot.lifecycle != CheckpointLifecycle::Retired {
        return Err(Error::Conflict(
            "checkpoint retention cleanup requires retirement",
        ));
    }
    let exists: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM checkpoint_sources WHERE checkpoint_id=?1)",
        [&row.snapshot.id],
        |r| r.get(0),
    ))?;
    if !exists {
        return Ok(None);
    }
    Ok(Some(
        sources_in(c, &row.snapshot.id, row.snapshot.source_coverage)?.0,
    ))
}

fn require_unshared_in(c: &Connection, checkpoint: &str) -> Result<()> {
    // Retention identity belongs to a driver and stable source. A live
    // anchor can share it even if its captured name or schema differs.
    let shared: bool = db(c.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM checkpoint_sources owned
            JOIN checkpoint_sources other
              ON other.driver_key=owned.driver_key AND other.source_ref=owned.source_ref
             AND other.retention=owned.retention
            JOIN checkpoints c ON c.checkpoint_id=other.checkpoint_id
            WHERE owned.checkpoint_id=?1 AND c.lifecycle='live'
        )",
        [checkpoint],
        |row| row.get(0),
    ))?;
    if shared {
        return Err(Error::Conflict(
            "checkpoint retention is still required by a live anchor",
        ));
    }
    Ok(())
}

fn cleanup_id(checkpoint: &str) -> String {
    format!(
        "{RETENTION_CLEANUP_PREFIX}{:x}",
        Sha256::digest(checkpoint.as_bytes())
    )
}

impl SessionStore {
    /// Bounded live checkpoint metadata in creation order. Cursor is the
    /// exact immutable checkpoint ID (including a since-retired cursor).
    /// This is the lower host read, not a public cursor/token protocol.
    pub fn checkpoint_list_metadata(
        &self,
        actor: &PrincipalId,
        reference: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<CheckpointSnapshot>> {
        if !(1..=256).contains(&limit) {
            return Err(Error::Limit("invalid checkpoint list limit"));
        }
        label(reference, 2048)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction())?;
        let (session, branch) = agent_owner::key_for_ref(&tx, reference)?;
        let source = AgentKey::new(&session, &branch);
        agent_owner::authorize_permission(&tx, actor, source, AgentPermission::View)?;
        let cursor: Option<(i64, String)> = if let Some(after) = after {
            label(after, 128)?;
            Some(
                db(tx
                    .query_row(
                        "SELECT created_at,checkpoint_id FROM checkpoints
                 WHERE checkpoint_id=?1 AND source_session_id=?2 AND source_branch_id=?3",
                        params![after, session, branch],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional())?
                .ok_or(Error::Denied("checkpoint cursor unavailable"))?,
            )
        } else {
            None
        };
        let snapshots = {
            let mut query = db(tx.prepare_cached(
                "SELECT checkpoint_id FROM checkpoints
                 WHERE source_session_id=?1 AND source_branch_id=?2 AND lifecycle='live'
                   AND (?3 IS NULL OR (created_at,checkpoint_id)>(?3,?4))
                 ORDER BY created_at,checkpoint_id LIMIT ?5",
            ))?;
            let mut rows = db(query.query(params![
                session,
                branch,
                cursor.as_ref().map(|c| c.0),
                cursor.as_ref().map(|c| c.1.as_str()),
                limit
            ]))?;
            let mut result = Vec::new();
            while let Some(row) = db(rows.next())? {
                result.push(snapshot_in(&tx, actor, &db(row.get::<_, String>(0))?)?);
            }
            result
        };
        db(tx.commit())?;
        Ok(snapshots)
    }

    /// Retire first; physical retention remains in the immutable anchors
    /// until independently authorized, fenced reconciliation proves release.
    /// No source disclosure or driver call is needed for logical retirement.
    pub fn checkpoint_delete(&self, actor: &PrincipalId, checkpoint: &str) -> Result<()> {
        label(checkpoint, 128)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let row = require_manager(&tx, actor, checkpoint)?;
        if row.snapshot.lifecycle == CheckpointLifecycle::Retired {
            return Ok(());
        }
        require_no_restore(&tx, self, checkpoint)?;
        if db(tx.execute(
            "UPDATE checkpoints SET lifecycle='retired' WHERE checkpoint_id=?1 AND lifecycle='live'",
            [checkpoint],
        ))? != 1 {
            return Err(Error::Conflict("checkpoint retirement changed"));
        }
        durability::commit_event(
            &tx,
            &durability::tx_id()?,
            "checkpoint.retired",
            "checkpoint",
            checkpoint,
            Some(actor),
            serde_json::json!({"sync_scope":"local","checkpoint_id":checkpoint}),
        )?;
        db(tx.commit())
    }

    /// Trusted retained-source reconciliation. The caller independently
    /// authorizes source retention cleanup and keeps any provider/external
    /// fence through this call. The driver inspects the exact anchor first;
    /// only a proven absent retention or exact conditional release may
    /// succeed. Never recapture, restore, delete materializations, or GC
    /// shared backing storage. Errors preserve the complete original set
    /// and Unknown custody. Already-cleaned retirement does not call driver.
    pub fn checkpoint_reconcile_retired_sources(
        &self,
        actor: &PrincipalId,
        checkpoint: &str,
        reconcile: impl FnOnce(&SourceSet) -> Result<()>,
    ) -> Result<()> {
        label(checkpoint, 128)?;
        if self.inner.mode != StoreMode::OwnerLock {
            return Err(Error::Denied(
                "retention cleanup requires the exclusive Store owner",
            ));
        }
        let prepare_id = cleanup_id(checkpoint);
        let (set, barrier) = {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let row = require_manager(&tx, actor, checkpoint)?;
            let Some(set) = retained_in(&tx, &row)? else {
                return Ok(());
            };
            require_no_restore(&tx, self, checkpoint)?;
            require_unshared_in(&tx, checkpoint)?;
            let registry = self.inner.source_drivers.reader()?;
            for anchor in &set.anchors {
                registry.resolve(&anchor.pin()?)?;
            }
            let mutations = SourceMutationSet::from_anchors(&set.anchors)?;
            let unresolved: bool = db(tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM local_coordination_leases
                  WHERE scope_kind='execution' AND scope_key GLOB 'source-mutation:*'
                    AND run_id=?1 AND state IN ('active','hazard'))",
                [&prepare_id],
                |row| row.get(0),
            ))?;
            // The retired anchor rows are already durable cleanup intent.
            // Reclaim only the exact original hazard, never an active owner.
            let barrier = if unresolved {
                self.recover_source_mutation_barrier_in(&tx, &mutations, &prepare_id)?
            } else {
                self.claim_source_mutation_barrier_in(&tx, &mutations, &prepare_id)?
            };
            db(tx.commit())?;
            (set, barrier)
        };
        if let Err(error) = reconcile(&set) {
            barrier.settle(SourceMutationOutcome::Unknown)?;
            return Err(error);
        }

        let finish = (|| {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            barrier.require_preparation_in(&tx, &prepare_id)?;
            let row = row_in(&tx, checkpoint)?;
            if retained_in(&tx, &row)?.as_ref() != Some(&set) {
                return Err(Error::Conflict("retained checkpoint anchors changed"));
            }
            require_no_restore(&tx, self, checkpoint)?;
            require_unshared_in(&tx, checkpoint)?;
            // Factual settlement after an admitted cleanup, not a new source
            // effect. Later actor revocation cannot erase the original fact.
            if db(tx.execute(
                "DELETE FROM checkpoint_sources WHERE checkpoint_id=?1",
                [checkpoint],
            ))? != set.anchors.len()
            {
                return Err(Error::Conflict("retained checkpoint source set changed"));
            }
            durability::commit_event(
                &tx,
                &durability::tx_id()?,
                "checkpoint.retention.released",
                "checkpoint",
                checkpoint,
                Some(actor),
                serde_json::json!({"sync_scope":"local","checkpoint_id":checkpoint}),
            )?;
            barrier.settle_in(&tx, SourceMutationOutcome::Settled)?;
            db(tx.commit())
        })();
        if let Err(error) = finish {
            barrier.settle(SourceMutationOutcome::Unknown)?;
            return Err(error);
        }
        Ok(())
    }
}
