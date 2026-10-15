//! AW-P6-10T: INTERNAL owner-private, bounded, READ-ONLY pre-activation review
//! of an original S quarantined v33 Store and the EXACT published source R V1.
//! No operator/MCP/Web/Agent callable activation; no stored consent, new
//! authority, model/provider/Tool invocation, Queue claim or implicit replay.
#![cfg(target_os = "linux")]
#![allow(dead_code)] // private future operator inspection path, not a public API

use crate::{actor_admission, actor_control, actor_queue, db, QueueState};
use aw_agent_core::{Error, JournalEntry, Result, ToolRecord, ToolStatus};
use rusqlite::{Connection, OpenFlags};
use std::{collections::BTreeMap, path::Path};

const MAX_REVIEW_SESSIONS: usize = 8_192;
const MAX_REVIEW_EVENTS: usize = 32_768;
const REVIEW_KIND: &str = "store.cold_quarantine_review_requested";
// A COMMIT error or any failed acknowledgment afterward may conceal a durable
// immutable request. Preserve uncertainty; inspect, never automatically retry.
const REVIEW_AUDIT_OUTCOME_UNKNOWN: &str =
    "cold quarantine review audit outcome unknown after commit; inspect original immutable receipt; never automatically retry or execute";

// Missing or contradictory original output leaves the effect unresolved.
// The Agent derives Succeeded/Failed from is_error; retain legacy facts that
// disagree for inspection. Interrupted stays uncertain with any output.
// Share this rule across T's counts and W7's exact immutable identities.
pub(crate) fn original_tool_receipt_needs_inspection(record: &ToolRecord) -> bool {
    match record.status {
        ToolStatus::Interrupted => true,
        ToolStatus::Succeeded => record.output.as_ref().is_none_or(|output| output.is_error),
        ToolStatus::Failed => record.output.as_ref().is_none_or(|output| !output.is_error),
        ToolStatus::Running | ToolStatus::Denied => false,
    }
}

// Keep the factual start/finish multiplicities and historical mismatch
// precedence. A matched 1/1 receipt may still leave the effect unresolved.
pub(crate) fn original_tool_obligations(
    started: usize,
    finished: usize,
    unresolved_receipt: bool,
) -> (usize, usize) {
    if started > finished {
        (started - finished, 0)
    } else if finished > started || started > 1 || finished > 1 {
        (0, finished.abs_diff(started).max(1))
    } else if unresolved_receipt {
        (1, 0)
    } else {
        (0, 0)
    }
}

/// Only bounded non-secret counts. This does not reveal account IDs, original
/// external tool command arguments, plaintext ToolOutput, queued Input bodies,
/// billing identities, ciphertext or provider credentials to any caller.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct ColdQuarantineInspection {
    pub(crate) generation: u64,
    pub(crate) sessions: usize,
    pub(crate) unfinished_runs: usize,
    pub(crate) uncertain_provider_receipts: usize,
    pub(crate) possibly_paid_attempts: usize,
    pub(crate) unresolved_effect_hazards: usize,
    pub(crate) hazardous_execution_leases: usize,
    pub(crate) incomplete_tool_intents: usize,
    pub(crate) mismatched_tool_receipts: usize,
    pub(crate) queued_pending: usize,
    pub(crate) queued_claimed: usize,
    pub(crate) queued_accepted: usize,
    pub(crate) actor_input_needs_inspection: usize,
    pub(crate) actor_control_needs_inspection: usize,
    pub(crate) open_recovery_hints: usize,
    pub(crate) retry_schedules_requiring_owner: usize,
    /// Data-only classification. This NEVER means permission to activate.
    pub(crate) outstanding_review: bool,
    /// Always false in AW-P6-10T. A later separate authenticated owner-origin
    /// authorization protocol must handle the review and runnable transition.
    pub(crate) activation_authorized: bool,
}

fn counted(c: &Connection, sql: &str) -> Result<usize> {
    let value: i64 = db(c.query_row(sql, [], |row| row.get(0)))?;
    let count = usize::try_from(value)
        .map_err(|_| Error::Limit("quarantine inspection returned invalid negative count"))?;
    if count > MAX_REVIEW_EVENTS {
        return Err(Error::Limit(
            "cold quarantine inspection exceeds 32768 evidence rows",
        ));
    }
    Ok(count)
}

/// Original P/Q/R/S verification and file anchors wrap ONE pinned SQLite
/// snapshot of the original v33 evidence, shared with W7/W8 and checked again
/// before returning. Does not call `SessionStore::open`, `Agent`, `TurnMachine`,
/// `actor_queue_claim_next` or any original external provider/Tool executor.
pub(crate) fn inspect_quarantined_cold_restore(
    complete_archive: &Path,
    restored_state: &Path,
) -> Result<ColdQuarantineInspection> {
    inspect_quarantined_cold_restore_impl(complete_archive, restored_state, || Ok(()))
}

#[cfg(test)]
pub(crate) fn inspect_quarantined_cold_restore_with_hook<F: FnOnce() -> Result<()>>(
    complete_archive: &Path,
    restored_state: &Path,
    after_inspection: F,
) -> Result<ColdQuarantineInspection> {
    inspect_quarantined_cold_restore_impl(complete_archive, restored_state, after_inspection)
}

fn inspect_quarantined_cold_restore_impl<F: FnOnce() -> Result<()>>(
    complete_archive: &Path,
    restored_state: &Path,
    after_inspection: F,
) -> Result<ColdQuarantineInspection> {
    crate::cold_effect_reconciliation::with_verified_cold_snapshot(
        complete_archive,
        restored_state,
        |_, _, report, _, _| Ok(report),
        after_inspection,
    )
}

/// W6: the very same original T risk rules, reusable inside ONE pinned
/// original SQLite read transaction with its per-effect identity inventory.
/// Caller must independently verify original P/Q/R/S and BOTH quarantines.
pub(crate) fn inspect_original_cold_risks_on_connection(
    c: &Connection,
    generation: u64,
) -> Result<ColdQuarantineInspection> {
    let mut result = ColdQuarantineInspection {
        generation,
        sessions: 0,
        unfinished_runs: counted(c, "SELECT count(*) FROM runs WHERE status='running'")?,
        uncertain_provider_receipts: counted(
            c,
            "SELECT count(*) FROM invocation_receipts WHERE state IN ('started','unknown')",
        )?,
        possibly_paid_attempts: counted(
            c,
            "SELECT count(*) FROM llm_attempts WHERE dispatch_state IN ('unknown','provider_started')",
        )?,
        unresolved_effect_hazards: counted(
            c,
            "SELECT count(*) FROM hazards WHERE state IN ('running','needs_inspection')",
        )?,
        hazardous_execution_leases: counted(
            c,
            "SELECT count(*) FROM local_coordination_leases
             WHERE scope_kind='execution' AND state='hazard'",
        )?,
        incomplete_tool_intents: 0,
        mismatched_tool_receipts: 0,
        queued_pending: 0,
        queued_claimed: 0,
        queued_accepted: 0,
        actor_input_needs_inspection: 0,
        actor_control_needs_inspection: 0,
        open_recovery_hints: counted(
            c, "SELECT count(*) FROM recovery_hints WHERE state='open'",
        )?,
        retry_schedules_requiring_owner: counted(
            c,
            "SELECT count(*) FROM retry_schedules
             WHERE state IN ('waiting_user','scheduled','claimed','blocked')",
        )?,
        outstanding_review: true,
        activation_authorized: false,
    };

    let sessions = {
        let mut query = db(c.prepare("SELECT id FROM sessions ORDER BY id LIMIT 8193"))?;
        let rows = db(query.query_map([], |row| row.get::<_, String>(0)))?;
        db(rows.collect::<rusqlite::Result<Vec<_>>>())?
    };
    if sessions.len() > MAX_REVIEW_SESSIONS {
        return Err(Error::Limit(
            "cold restore has too many Sessions for bounded review",
        ));
    }
    result.sessions = sessions.len();
    let mut inspected_events = 0usize;
    for session in sessions {
        let mut queue = BTreeMap::new();
        for fact in actor_queue::events(c, &session)? {
            queue.insert(fact.request.request_id.clone(), fact);
            inspected_events += 1;
        }
        for fact in queue.into_values() {
            match fact.state {
                QueueState::Pending => result.queued_pending += 1,
                QueueState::Claimed => result.queued_claimed += 1,
                QueueState::Accepted => result.queued_accepted += 1,
                QueueState::Settled | QueueState::Deleted | QueueState::Inspected => {}
            }
        }
        let mut inputs = BTreeMap::new();
        for fact in actor_admission::events(c, &session)? {
            inputs.insert(fact.admission.request_id.clone(), fact);
            inspected_events += 1;
        }
        result.actor_input_needs_inspection += inputs
            .into_values()
            .filter(|fact| fact.state.needs_inspection())
            .count();
        let mut controls = BTreeMap::new();
        for fact in actor_control::events(c, &session)? {
            controls.insert(fact.admission.request_id.clone(), fact);
            inspected_events += 1;
        }
        result.actor_control_needs_inspection += controls
            .into_values()
            .filter(|fact| fact.state.needs_inspection())
            .count();
        if inspected_events > MAX_REVIEW_EVENTS {
            return Err(Error::Limit(
                "cold restore original Actor event history exceeds review budget",
            ));
        }
    }

    // Journal remains the original immutable factual tool source. Compare
    // exact (run,call.id) pairs, not only global start/finish event totals,
    // so two unrelated calls cannot cancel each other's missing receipt.
    // Retain the original started effect identity, not only the textual call
    // ID: a different Tool/site/effect with the same ID must NEVER forge a
    // terminal receipt and clear a possibly effectful original intent.
    let mut tool_calls: BTreeMap<(String, String), (usize, usize, Option<ToolRecord>, bool)> =
        BTreeMap::new();
    {
        let mut query = db(c.prepare(
            "SELECT run_id,kind,payload FROM run_events
             WHERE kind IN ('tool_started','tool_finished') ORDER BY seq LIMIT 32769",
        ))?;
        let rows = db(query.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        }))?;
        let events = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        if events.len() > MAX_REVIEW_EVENTS {
            return Err(Error::Limit(
                "original Tool evidence exceeds bounded quarantine review",
            ));
        }
        for (run, kind, payload) in events {
            let event: JournalEntry = crate::decode(&payload)?;
            let (record, started) = match (kind.as_str(), &event) {
                ("tool_started", JournalEntry::ToolStarted { record, source }) => {
                    if let Some(source) = source {
                        source.validate()?;
                    }
                    if record.status != ToolStatus::Running || record.output.is_some() {
                        return Err(Error::Denied(
                            "cold original ToolStarted is not a running intent",
                        ));
                    }
                    (record, true)
                }
                ("tool_finished", JournalEntry::ToolFinished { record }) => {
                    if matches!(record.status, ToolStatus::Running | ToolStatus::Denied) {
                        return Err(Error::Denied(
                            "cold ToolFinished does not terminally settle its effect",
                        ));
                    }
                    if let Some(output) = &record.output {
                        output.validate()?;
                    }
                    (record, false)
                }
                _ => return Err(Error::Denied("original Tool receipt kind/payload mismatch")),
            };
            record.call.validate_envelope()?;
            if run.is_empty() || record.execution_site.is_empty() {
                return Err(Error::Denied(
                    "cold Tool journal lacks original Run or site",
                ));
            }
            let counts = tool_calls.entry((run, record.call.id.clone())).or_default();
            if started {
                // Preserve the historical incomplete/duplicate categories,
                // but never accept a later-started call after an earlier finish.
                if counts.1 != 0 {
                    return Err(Error::Denied(
                        "cold Tool journal starts after an earlier terminal receipt",
                    ));
                }
                if counts.2.as_ref().is_some_and(|original| {
                    original.call != record.call
                        || original.effect != record.effect
                        || original.execution_site != record.execution_site
                }) {
                    return Err(Error::Denied(
                        "cold Tool journal duplicate start changed identity",
                    ));
                }
                counts.0 += 1;
                if counts.2.is_none() {
                    counts.2 = Some(record.clone());
                }
            } else {
                if let Some(original) = &counts.2 {
                    if original.call != record.call
                        || original.effect != record.effect
                        || original.execution_site != record.execution_site
                    {
                        return Err(Error::Denied(
                            "cold Tool terminal does not match original effect intent",
                        ));
                    }
                }
                // No original start is still a mismatch requiring inspection;
                // never invent a missing intent. A later start is prohibited.
                counts.1 += 1;
                counts.3 |= original_tool_receipt_needs_inspection(record);
            }
        }
    }
    for (started, finished, _, unresolved_receipt) in tool_calls.into_values() {
        let (incomplete, mismatched) =
            original_tool_obligations(started, finished, unresolved_receipt);
        result.incomplete_tool_intents += incomplete;
        result.mismatched_tool_receipts += mismatched;
    }
    let risky = [
        result.unfinished_runs,
        result.uncertain_provider_receipts,
        result.possibly_paid_attempts,
        result.unresolved_effect_hazards,
        result.hazardous_execution_leases,
        result.incomplete_tool_intents,
        result.mismatched_tool_receipts,
        result.queued_pending,
        result.queued_claimed,
        result.queued_accepted,
        result.actor_input_needs_inspection,
        result.actor_control_needs_inspection,
        result.open_recovery_hints,
        result.retry_schedules_requiring_owner,
    ];
    result.outstanding_review = risky.into_iter().any(|count| count != 0);
    // Even an empty snapshot with zero hazards is NOT an activating grant.
    result.activation_authorized = false;
    Ok(result)
}

/// AW-P6-10T: a trusted original owner can record a non-activating review
/// request for a later, separately authenticated human authorization protocol.
/// This is NOT approval, replay consent, a process/actor grant or a runnable
/// restoration operation. No model, MCP, extension, web or RPC surface calls it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ColdReviewIntent {
    KeepQuarantined,
    RequestSeparateActivationReview,
}
impl ColdReviewIntent {
    fn label(self) -> &'static str {
        match self {
            Self::KeepQuarantined => "keep_quarantined",
            Self::RequestSeparateActivationReview => "request_separate_activation_review",
        }
    }
}

/// Original T Session-owner predicate shared by W8's narrowly scoped audit.
/// It is an ownership check only; never evidence of independent live-human
/// presence, global clone uniqueness or a fresh Process/WriterEpoch grant.
pub(crate) fn require_original_cold_owner_of_all_sessions(
    c: &Connection,
    operator: &aw_agent_core::PrincipalId,
) -> Result<()> {
    let sessions = {
        let mut stmt = db(c.prepare("SELECT id FROM sessions ORDER BY id LIMIT 8193"))?;
        let identities = db(stmt.query_map([], |row| row.get::<_, String>(0)))?;
        db(identities.collect::<rusqlite::Result<Vec<_>>>())?
    };
    if sessions.is_empty() || sessions.len() > MAX_REVIEW_SESSIONS {
        return Err(Error::Denied(
            "review requires one bounded ORIGINAL Session owner identity",
        ));
    }
    for session in &sessions {
        if !crate::principal_is_session_owner(c, session, operator)? {
            return Err(Error::Denied(
                "quarantine review requires the authentic original owner of EVERY copied Session",
            ));
        }
    }
    Ok(())
}

pub(crate) fn record_quarantine_review_request(
    complete_archive: &Path,
    restored_state: &Path,
    operator: &aw_agent_core::PrincipalId,
    request_id: &str,
    decision: ColdReviewIntent,
) -> Result<ColdQuarantineInspection> {
    record_quarantine_review_request_impl(
        complete_archive,
        restored_state,
        operator,
        request_id,
        decision,
        |_| Ok(()),
        |_| Ok(()),
    )
}

#[cfg(test)]
pub(crate) fn record_quarantine_review_request_with_hooks<
    F: FnOnce(&Path) -> Result<()>,
    G: FnOnce(&Path) -> Result<()>,
>(
    complete_archive: &Path,
    restored_state: &Path,
    operator: &aw_agent_core::PrincipalId,
    request_id: &str,
    decision: ColdReviewIntent,
    before_transaction: F,
    after_commit: G,
) -> Result<ColdQuarantineInspection> {
    record_quarantine_review_request_impl(
        complete_archive,
        restored_state,
        operator,
        request_id,
        decision,
        before_transaction,
        after_commit,
    )
}

fn record_quarantine_review_request_impl<
    F: FnOnce(&Path) -> Result<()>,
    G: FnOnce(&Path) -> Result<()>,
>(
    complete_archive: &Path,
    restored_state: &Path,
    operator: &aw_agent_core::PrincipalId,
    request_id: &str,
    decision: ColdReviewIntent,
    before_transaction: F,
    after_commit: G,
) -> Result<ColdQuarantineInspection> {
    if request_id.is_empty()
        || request_id.len() > 128
        || !request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    {
        return Err(Error::Denied(
            "quarantine review needs one exact bounded request identity",
        ));
    }
    let (store_id, inspection) = crate::cold_effect_reconciliation::with_verified_cold_snapshot(
        complete_archive,
        restored_state,
        |_, _, report, store_id, _| Ok((store_id.to_owned(), report)),
        || Ok(()),
    )?;
    before_transaction(restored_state)?;
    let anchors =
        crate::cold_effect_reconciliation::ColdFilesystemAnchors::capture(restored_state)?;
    let database = restored_state.join("sessions.sqlite3");
    let file = aw_platform::open_private_existing(&database, false)?;
    drop(file);
    // This separate ACTUAL original SQLite write is only one immutable owner
    // review REQUEST receipt. It does not open SessionStore/register a process,
    // change a writer/Queue/provider receipt, or remove the quarantine marker.
    let mut c = db(Connection::open_with_flags(
        &database,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ))?;
    let tx = db(c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
    let (_, fresh) = crate::cold_effect_reconciliation::original_facts_on_connection(
        &tx,
        &store_id,
        inspection.generation,
        || Ok(()),
    )?;
    if fresh != inspection {
        return Err(Error::Denied(
            "quarantine review original risk report changed before audit transaction",
        ));
    }
    require_original_cold_owner_of_all_sessions(&tx, operator)?;
    let prior: i64 = db(tx.query_row(
        "SELECT count(*) FROM local_coordination_events
         WHERE kind='store.cold_quarantine_review_requested' AND store_generation=?1",
        [inspection.generation],
        |row| row.get(0),
    ))?;
    if prior >= 64 {
        return Err(Error::Limit(
            "quarantine review receipts exceed original bounded owner inspection budget",
        ));
    }
    let duplicate: bool = db(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM local_coordination_events
         WHERE kind='store.cold_quarantine_review_requested'
           AND json_extract(payload_json,'$.request_id')=?1)",
        [request_id],
        |row| row.get(0),
    ))?;
    if duplicate {
        return Err(Error::Conflict(
            "original quarantine review request identity already recorded",
        ));
    }
    // Reuse W8's original custody and dual-denial checks under the write lock.
    anchors.verify(restored_state)?;
    let (actual_id, actual_generation) =
        crate::composite_staging::verify_cold_for_inspection(complete_archive, restored_state)?;
    if actual_id != store_id || actual_generation != inspection.generation {
        return Err(Error::Denied(
            "quarantine review original archive or generation changed",
        ));
    }
    anchors.verify(restored_state)?;
    let payload = serde_json::json!({
        "review_version": 1,
        "request_id": request_id,
        "actor": operator.as_str(),
        "intent": decision.label(),
        "store_generation": inspection.generation,
        "activation_authorized": false,
        "outstanding_review": inspection.outstanding_review,
        "unknown_paid_receipts": inspection.uncertain_provider_receipts,
        "unmatched_tool_intents": inspection.incomplete_tool_intents,
        "claimed_queued_inputs": inspection.queued_claimed,
        "pending_queued_inputs": inspection.queued_pending,
    });
    let encoded = serde_json::to_string(&payload)
        .map_err(|_| Error::Storage("cannot encode private review evidence"))?;
    let event_id = crate::id("coord")?;
    let created_at = crate::now();
    db(tx.execute(
        "INSERT INTO local_coordination_events(
             event_id,store_id,store_generation,process_id,kind,scope_kind,scope_key,
             lease_generation,payload_json,created_at)
         VALUES(?1,?2,?3,NULL,'store.cold_quarantine_review_requested',
                'cold_restore',?4,NULL,?5,?6)",
        rusqlite::params![
            event_id,
            store_id,
            inspection.generation,
            request_id,
            encoded,
            created_at,
        ],
    ))?;
    let event_seq = tx.last_insert_rowid();
    db(tx.commit()).map_err(|_| Error::Storage(REVIEW_AUDIT_OUTCOME_UNKNOWN))?;
    let postcommit = (|| -> Result<()> {
        after_commit(restored_state)?;
        drop(c);
        anchors.verify_after_review_commit(restored_state)?;
        let copy = aw_platform::open_private_existing(&database, false)?;
        copy.sync_all()?;
        aw_platform::sync_parent(&database)?;
        crate::cold_effect_reconciliation::with_verified_cold_snapshot(
            complete_archive,
            restored_state,
            |tx, _, after_report, after_id, after_generation| {
                if after_id != store_id
                    || after_generation != inspection.generation
                    || after_report != inspection
                {
                    return Err(Error::Denied(
                        "quarantine review post-commit original risk report changed",
                    ));
                }
                crate::cold_effect_reconciliation::ColdReviewAuditRow {
                    seq: event_seq,
                    event_id: &event_id,
                    store_id: &store_id,
                    generation: inspection.generation,
                    kind: REVIEW_KIND,
                    scope_key: request_id,
                    payload_json: &encoded,
                    created_at,
                }
                .verify(tx)
            },
            || anchors.verify_after_review_commit(restored_state),
        )
    })();
    postcommit.map_err(|_| Error::Storage(REVIEW_AUDIT_OUTCOME_UNKNOWN))?;
    // Explicit review ONLY: keep BOTH independent denial markers unchanged.
    Ok(inspection)
}

#[cfg(test)]
mod recent_correctness_review_tests {
    use super::*;
    use aw_agent_core::{ToolCall, ToolEffect, ToolRecord, ToolSourceEvidence, ToolStatus};

    fn original_risk_fixture() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE runs(id TEXT,status TEXT);
             CREATE TABLE invocation_receipts(run_id TEXT,state TEXT);
             CREATE TABLE llm_attempts(dispatch_state TEXT);
             CREATE TABLE hazards(state TEXT);
             CREATE TABLE local_coordination_leases(scope_kind TEXT,state TEXT);
             CREATE TABLE recovery_hints(state TEXT);
             CREATE TABLE retry_schedules(state TEXT);
             CREATE TABLE sessions(id TEXT);
             CREATE TABLE audit(session_id TEXT,action TEXT,data_json TEXT,seq INTEGER);
             CREATE TABLE run_events(run_id TEXT,kind TEXT,payload TEXT,seq INTEGER);",
        )
        .unwrap();
        c
    }

    fn sample_running_record() -> ToolRecord {
        ToolRecord {
            call: ToolCall {
                id: "effectful-original-call".into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command":"synthetic"}),
            },
            effect: ToolEffect::Process,
            execution_site: "site-original".into(),
            status: ToolStatus::Running,
            output: None,
        }
    }

    fn write_facts(c: &Connection, started: JournalEntry, finished: JournalEntry, reverse: bool) {
        let records = if reverse {
            [("tool_finished", finished), ("tool_started", started)]
        } else {
            [("tool_started", started), ("tool_finished", finished)]
        };
        for (seq, (kind, entry)) in records.into_iter().enumerate() {
            c.execute(
                "INSERT INTO run_events VALUES(?1,?2,?3,?4)",
                rusqlite::params![
                    "run-original",
                    kind,
                    serde_json::to_string(&entry).unwrap(),
                    seq as i64
                ],
            )
            .unwrap();
        }
    }

    #[test]
    fn review_accepts_exact_original_tool_pair_without_inventing_hazards() {
        let c = original_risk_fixture();
        let started = sample_running_record();
        let mut finished = started.clone();
        finished.status = ToolStatus::Succeeded;
        finished.output = Some(aw_agent_core::ToolOutput::text("synthetic result"));
        write_facts(
            &c,
            JournalEntry::ToolStarted {
                record: started,
                source: Some(ToolSourceEvidence::trusted_host()),
            },
            JournalEntry::ToolFinished { record: finished },
            false,
        );
        let report = inspect_original_cold_risks_on_connection(&c, 2).unwrap();
        assert_eq!(report.incomplete_tool_intents, 0);
        assert_eq!(report.mismatched_tool_receipts, 0);
        assert!(!report.outstanding_review);
        assert!(!report.activation_authorized);
    }

    #[test]
    fn aw_p6_w8g_missing_terminal_output_preserves_tool_intent() {
        for status in [ToolStatus::Succeeded, ToolStatus::Failed] {
            let c = original_risk_fixture();
            let started = sample_running_record();
            let mut finished = started.clone();
            finished.status = status.clone();
            write_facts(
                &c,
                JournalEntry::ToolStarted {
                    record: started,
                    source: Some(ToolSourceEvidence::trusted_host()),
                },
                JournalEntry::ToolFinished { record: finished },
                false,
            );
            let report = inspect_original_cold_risks_on_connection(&c, 2).unwrap();
            assert_eq!(
                report.incomplete_tool_intents, 1,
                "{status:?}: a status without the original output cannot clear the effect"
            );
            assert_eq!(report.mismatched_tool_receipts, 0);
            assert!(report.outstanding_review && !report.activation_authorized);
        }
    }

    #[test]
    fn aw_p6_w8g_empty_present_output_retains_original_receipt_semantics() {
        for (status, output) in [
            (ToolStatus::Succeeded, aw_agent_core::ToolOutput::text("")),
            (ToolStatus::Failed, aw_agent_core::ToolOutput::error("")),
        ] {
            let c = original_risk_fixture();
            let started = sample_running_record();
            let mut finished = started.clone();
            finished.status = status;
            finished.output = Some(output);
            write_facts(
                &c,
                JournalEntry::ToolStarted {
                    record: started,
                    source: Some(ToolSourceEvidence::trusted_host()),
                },
                JournalEntry::ToolFinished { record: finished },
                false,
            );
            let report = inspect_original_cold_risks_on_connection(&c, 2).unwrap();
            assert_eq!(report.incomplete_tool_intents, 0);
            assert_eq!(report.mismatched_tool_receipts, 0);
            assert!(!report.outstanding_review && !report.activation_authorized);
        }
    }

    #[test]
    fn aw_p6_w8h_contradictory_terminal_output_preserves_tool_intent() {
        for (status, output) in [
            (
                ToolStatus::Succeeded,
                aw_agent_core::ToolOutput::error("synthetic failure"),
            ),
            (
                ToolStatus::Failed,
                aw_agent_core::ToolOutput::text("synthetic success"),
            ),
        ] {
            let c = original_risk_fixture();
            let started = sample_running_record();
            let mut finished = started.clone();
            finished.status = status.clone();
            finished.output = Some(output);
            write_facts(
                &c,
                JournalEntry::ToolStarted {
                    record: started,
                    source: Some(ToolSourceEvidence::trusted_host()),
                },
                JournalEntry::ToolFinished { record: finished },
                false,
            );
            let report = inspect_original_cold_risks_on_connection(&c, 2).unwrap();
            assert_eq!(
                report.incomplete_tool_intents, 1,
                "{status:?}: contradictory original result cannot clear the effect"
            );
            assert_eq!(report.mismatched_tool_receipts, 0);
            assert!(report.outstanding_review && !report.activation_authorized);
        }
    }

    #[test]
    fn review_preserves_interrupted_tool_as_unresolved_original_effect() {
        let c = original_risk_fixture();
        let started = sample_running_record();
        let mut interrupted = started.clone();
        interrupted.status = ToolStatus::Interrupted;
        write_facts(
            &c,
            JournalEntry::ToolStarted {
                record: started,
                source: Some(ToolSourceEvidence::trusted_host()),
            },
            JournalEntry::ToolFinished {
                record: interrupted,
            },
            false,
        );
        let report = inspect_original_cold_risks_on_connection(&c, 2).unwrap();
        assert_eq!(report.incomplete_tool_intents, 1);
        assert_eq!(report.mismatched_tool_receipts, 0);
        assert!(report.outstanding_review);
        assert!(!report.activation_authorized);
    }

    #[test]
    fn review_rejects_forged_tool_event_kind_or_original_source() {
        let c = original_risk_fixture();
        let started = sample_running_record();
        let mut finished = started.clone();
        finished.status = ToolStatus::Succeeded;
        c.execute(
            "INSERT INTO run_events VALUES(?1,?2,?3,1)",
            rusqlite::params![
                "run-original",
                "tool_started",
                serde_json::to_string(&JournalEntry::ToolFinished { record: finished }).unwrap(),
            ],
        )
        .unwrap();
        assert!(inspect_original_cold_risks_on_connection(&c, 2).is_err());
        c.execute("DELETE FROM run_events", []).unwrap();
        c.execute(
            "INSERT INTO run_events VALUES(?1,?2,?3,1)",
            rusqlite::params![
                "run-original",
                "tool_started",
                serde_json::to_string(&JournalEntry::ToolStarted {
                    record: started,
                    source: Some(ToolSourceEvidence {
                        kind: "host".into(),
                        id: "trusted-host".into(),
                        epoch: 0
                    }),
                })
                .unwrap(),
            ],
        )
        .unwrap();
        assert!(inspect_original_cold_risks_on_connection(&c, 2).is_err());
    }

    #[test]
    fn review_cannot_clear_original_tool_risk_with_mismatched_same_id_terminal() {
        for case in [
            "wrong_effect",
            "wrong_arguments",
            "wrong_site",
            "not_terminal",
            "reversed",
        ] {
            let c = original_risk_fixture();
            let started = sample_running_record();
            let mut finished = started.clone();
            finished.status = ToolStatus::Succeeded;
            match case {
                "wrong_effect" => finished.effect = ToolEffect::Read,
                "wrong_arguments" => {
                    finished.call.arguments = serde_json::json!({"command":"different"})
                }
                "wrong_site" => finished.execution_site = "other-host".into(),
                "not_terminal" => finished.status = ToolStatus::Running,
                _ => {}
            }
            write_facts(
                &c,
                JournalEntry::ToolStarted {
                    record: started,
                    source: Some(ToolSourceEvidence::trusted_host()),
                },
                JournalEntry::ToolFinished { record: finished },
                case == "reversed",
            );
            let inspected = inspect_original_cold_risks_on_connection(&c, 2);
            assert!(
                inspected.is_err() || inspected.unwrap().outstanding_review,
                "{case}: mismatched same-call original Tool journal must not clear risk"
            );
        }
    }
}
