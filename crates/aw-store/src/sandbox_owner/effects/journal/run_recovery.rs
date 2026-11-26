//! Exact logical reconciliation of one original direct native Run.
//! Physical recovery stays with each existing source-effect owner. No
//! provider receipt, immutable Turn, source release or grant is rewritten.
use super::*;
use crate::{
    actor_admission::InputAdmissionState,
    sandbox_owner::{self, SandboxLifecycle, SandboxSnapshot},
    secret_store::SecretStore,
    QueueState, StoreMode,
};
use aw_tools::filesystem::{access::Action, schema::Tool};
use std::collections::BTreeSet;

const NOTE: &str =
    "Original native source effect releases verified; original Run retained, no replay";

/// Data-only result of reconciliation, not a new execution receipt.
/// Inspected Input/queue custody is not a successful Turn or provider proof.
#[derive(Debug, Serialize)]
pub struct NativeTurnReconciliation {
    pub turn: u64,
    pub source_effects: usize,
    pub hazard_cleared: bool,
    pub input: InputAdmissionState,
    pub queue: Option<QueueState>,
    pub changed: bool,
}

/// Require every original effect's exact positive release. No missing
/// claim, provider result, terminal ToolRecord or human note is proof.
fn require_releases(
    c: &Transaction<'_>,
    run: &str,
    agent: &str,
    sandbox: &SandboxSnapshot,
    turn: u64,
    actor: &PrincipalId,
) -> Result<(usize, bool)> {
    let driver = DriverKey::builtin("native-filesystem")?;
    let git = DriverKey::builtin("git")?;
    if sandbox.sources.is_empty() || sandbox.sources.iter().any(|source| source.driver != git) {
        return Err(Error::Denied(
            "native Run recovery requires its original Git source layout",
        ));
    }
    // Validate the complete original versioned binding digest without
    // requiring a current driver registration or probing a checkout.
    let bindings =
        sandbox_owner::source_bindings::versioned_bindings_in(c, &sandbox.canonical_ref)?;
    let mut query = db(c.prepare_cached(
        "SELECT seq,payload FROM run_events WHERE run_id=?1 ORDER BY seq LIMIT 513",
    ))?;
    let mut rows = db(query.query([run]))?;
    let mut starts = BTreeSet::new();
    let mut writes = false;
    let mut count = 0;
    while let Some(row) = db(rows.next())? {
        count += 1;
        if count > 512 {
            return Err(Error::Limit("original Run journal exceeds bound"));
        }
        let seq: u64 = db(row.get(0))?;
        match crate::decode::<JournalEntry>(&db(row.get::<_, String>(1))?)? {
            JournalEntry::ToolStarted { record, .. } => {
                let tool = Tool::from_name(&record.call.name)
                    .ok_or(Error::Denied("Run contains a non-native tool intent"))?;
                let effect = if tool.action() == Action::Read {
                    ToolEffect::Read
                } else {
                    ToolEffect::Write
                };
                if tool == Tool::Remove
                    || record.effect != effect
                    || record.execution_site != "local"
                    || !starts.insert(record.call.id.clone())
                {
                    return Err(Error::Denied(
                        "Run contains an unsupported or ambiguous native intent",
                    ));
                }
                let observed = observe(c, agent, turn, &record.call.id)?;
                if observed.intent.run_id != run
                    || observed.intent.started_seq != seq
                    || &observed.intent.actor != actor
                    || observed.source != ToolSourceEvidence::trusted_host()
                    || observed.active
                {
                    return Err(Error::Denied(
                        "native intent differs from original settled Run",
                    ));
                }
                let claim = require_claim(c, &observed.intent)?;
                if claim.driver != driver
                    || claim.sandbox_ref != sandbox.canonical_ref
                    || claim.sources.len() != sandbox.sources.len()
                {
                    return Err(Error::Denied(
                        "Run effect differs from original complete native source custody",
                    ));
                }
                verify_claim_bindings(&claim, bindings.clone())?;
                let (terminal, _) = terminal_in(c, &observed)?;
                if released_in(c, &observed, &terminal)?.is_none() {
                    return Err(Error::Conflict(
                        "original native effect has no positive source release",
                    ));
                }
                writes |= effect == ToolEffect::Write;
            }
            JournalEntry::ToolFinished { record } if !starts.contains(&record.call.id) => {
                return Err(Error::Conflict(
                    "original ToolFinished has no matching intent",
                ))
            }
            JournalEntry::ToolRevised { .. } => {
                return Err(Error::Denied(
                    "native Run recovery does not admit revised tool intents",
                ))
            }
            _ => {}
        }
    }
    Ok((starts.len(), writes))
}

impl SessionStore {
    /// Reconcile only a terminal, attached direct native Run from a
    /// previous exclusive owner. All original effects must already have
    /// positive release facts (or no tool intent). Current owner/writer
    /// and Agent/Project authority are rechecked in the same transaction.
    /// Preserve provider uncertainty, independent hazards and history.
    pub fn reconcile_native_turn(
        &self,
        manager: &PrincipalId,
        agent_ref: &str,
        turn: u64,
    ) -> Result<NativeTurnReconciliation> {
        label(agent_ref, 2048)?;
        if turn == 0 || self.inner.mode != StoreMode::OwnerLock {
            return Err(Error::Denied(
                "native Turn recovery requires exact Turn and exclusive Store owner",
            ));
        }
        let process = self
            .process_identity()?
            .ok_or(Error::Denied("missing current Store owner"))?;
        let (result, payload) = {
            let mut c = self.connection()?;
            let tx = db(c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
            let (session, branch) = agent_owner::key_for_ref(&tx, agent_ref)?;
            let key = AgentKey::new(&session, &branch);
            if agent_owner::canonical_ref(&tx, key)? != agent_ref {
                return Err(Error::Denied(
                    "native recovery requires exact canonical Agent",
                ));
            }
            agent_owner::authorize_permission(&tx, manager, key, AgentPermission::Manage)?;
            let (project, layout, sandbox, _, lifecycle) =
                sandbox_owner::agent_project_and_layout(&tx, &session, &branch)?;
            sandbox_owner::authorize_project(&tx, manager, &project, true)?;
            let sandbox = sandbox_owner::snapshot_with_sources(
                &tx,
                &sandbox.ok_or(Error::Denied("native recovery requires bound Sandbox"))?,
            )?;
            if lifecycle != "open"
                || sandbox.lifecycle != SandboxLifecycle::Open
                || sandbox.project_ref != project.canonical()
                || sandbox.layout_digest != layout.digest
                || sandbox.tool_root.to_str() != Some(layout.tool_root.as_str())
                || sandbox.working_directory.to_str() != Some(layout.working_directory.as_str())
            {
                return Err(Error::Denied(
                    "native recovery differs from original Agent layout",
                ));
            }

            let row: Option<(String, String, String, bool)> = db(tx.query_row(
                "SELECT r.id,r.final_node,r.root_key,r.possible_effect FROM runs r
                 JOIN local_process_incarnations p ON p.process_id=r.owner_process_id
                 WHERE r.session_id=?1 AND r.branch_id=?2 AND r.turn_id=?3
                   AND r.status IN ('completed','failed','cancelled','incomplete','interrupted')
                   AND r.ended_at IS NOT NULL AND r.final_node IS NOT NULL AND r.attach_status='attached'
                   AND p.state IN ('stopped','fenced') AND p.process_id<>?4
                   AND NOT EXISTS(SELECT 1 FROM branches b WHERE b.session_id=?1 AND b.active_run IS NOT NULL)
                   AND NOT EXISTS(SELECT 1 FROM runs live WHERE live.session_id=?1 AND live.status='running')",
                params![session, branch, turn, process.process_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            ).optional())?;
            let (run, final_node, root_key, possible) = row.ok_or(Error::Denied(
                "native recovery requires original attached Run of a previous quiescent Store owner",
            ))?;
            let original = direct_run_in(&tx, &session, &branch, &run, turn)?;
            let node = crate::node_in(&tx, &session, &final_node)?;
            if node.kind != "turn"
                || node.parent_id.as_deref() != Some(original.prompt_node.as_str())
                || node.actor != original.actor
                || node.payload["id"].as_u64() != Some(turn)
                || crate::key(&crate::session_in(&tx, &session)?.binding)? != root_key
            {
                return Err(Error::Conflict("settled native Run identity changed"));
            }
            let (source_effects, writes) =
                require_releases(&tx, &run, agent_ref, &sandbox, turn, &original.actor)?;
            if writes != possible {
                return Err(Error::Conflict(
                    "original Run mutation intent differs from its hazard",
                ));
            }
            // Never clear another Run's hazard, import review or provider
            // attempt. The immutable source releases prove this one only.
            let hazard: Option<(String, String, String)> = db(tx
                .query_row(
                    "SELECT session_id,root_key,state FROM hazards WHERE run_id=?1",
                    [&run],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional())?;
            let hazard_cleared = match hazard {
                None if !writes => false,
                Some((s, root, state)) if writes && s == session && root == root_key => {
                    match state.as_str() {
                        "settled" => false,
                        "needs_inspection" => {
                            db(tx.execute(
                                "UPDATE hazards SET state='settled',note=?1,updated_at=?2
                             WHERE run_id=?3 AND state='needs_inspection'",
                                params![NOTE, crate::now(), run],
                            ))?;
                            crate::audit(
                                &tx,
                                &session,
                                manager,
                                "effects.inspected",
                                serde_json::json!({"run_id":run,"note":NOTE}),
                            )?;
                            true
                        }
                        _ => return Err(Error::Conflict("original Run hazard not terminal")),
                    }
                }
                _ => return Err(Error::Conflict("original Run hazard identity differs")),
            };
            let (input, input_changed) =
                crate::actor_admission::inspect_recovered_in(&tx, manager, &original.input, NOTE)?;
            let (queue, queue_changed) = crate::actor_queue::retire_recovered_in(
                &tx,
                manager,
                &original.input,
                &original.process_id,
                process.store_generation,
                input,
                NOTE,
            )?;
            let changed = hazard_cleared || input_changed || queue_changed;
            if changed {
                agent_owner::changed(&tx, key)?;
            }
            let result = NativeTurnReconciliation {
                turn,
                source_effects,
                hazard_cleared,
                input,
                queue: queue.as_ref().map(|fact| fact.state),
                changed,
            };
            db(tx.commit())?;
            (result, queue.map(|fact| fact.payload_ref))
        };
        // The original immutable terminal queue fact precedes private
        // payload retirement. Exact replay can finish a missed unlink.
        if let Some(payload) = payload {
            self.local_secret_store()?.delete(&payload)?;
        }
        Ok(result)
    }
}
