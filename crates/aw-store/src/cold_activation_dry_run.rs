//! AW-P6-10V: internal Linux-only, source-independent cold activation DRY RUN.
//! NO runnable activation, Process registration, human-presence assertion,
//! WriterEpoch grants, Queue claims, paid/Tool/OS retries or model-visible API.
//! The original U consent event includes HASHES but not a re-verifiable raw
//! signature: require the actual detached signature and exact transcript again.
#![cfg(target_os = "linux")]
#![allow(dead_code)] // intentionally inaccessible through shipping CLI/Host/SDK

use crate::cold_effect_reconciliation::{
    self as cold, ColdFilesystemAnchors, ColdReviewAuditRow, COLD_AUDIT_OUTCOME_UNKNOWN,
};
use crate::{cold_activation_consent, cold_quarantine_review, db, now};
use aw_agent_core::{Error, Result};
use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};
use std::path::Path;

const DRY_RUN_KIND: &str = "store.cold_activation_dry_run_recorded";
const MAX_DRY_RUNS_PER_RESTORE: i64 = 32;
const MAX_WRITER_EPOCHS: usize = 8192;

/// A precise, non-secret, non-exported eligibility *diagnostic* only.
/// Even a cryptographically verified U device proof remains insufficient for
/// runnable activation absent independently verified live human intent,
/// source/WriterEpoch reauthorization and external-effect reconciliation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ColdActivationDryRun {
    pub(crate) generation: u64,
    pub(crate) active_writers: usize,
    pub(crate) active_processes: usize,
    pub(crate) would_require_fresh_writer_epochs: usize,
    pub(crate) consent_evidence_reverified: bool,
    pub(crate) requires_fresh_human_origin: bool,
    pub(crate) activation_authorized: bool,
    pub(crate) runnable: bool,
    /// Private U evidence identity; no signature, plaintext secret or user key.
    original_request_id: String,
    original_owner_user_id: String,
    transcript_sha256: String,
    inspection_sha256: String,
    store_id: String,
}

fn bounded_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(Error::Denied(
            "dry-run requires one bounded exact original request ID",
        ));
    }
    Ok(())
}

fn counted(c: &Connection, sql: &str) -> Result<usize> {
    let rows: i64 = db(c.query_row(sql, [], |row| row.get(0)))?;
    let count = usize::try_from(rows).map_err(|_| {
        Error::Limit("cold dry-run cannot accept invalid negative process/writer count")
    })?;
    if count > MAX_WRITER_EPOCHS {
        return Err(Error::Limit(
            "cold dry-run process/writer evidence exceeds review budget",
        ));
    }
    Ok(count)
}

/// V1: verify the original U actual signature + immutable challenge/consent
/// and the FULL R/S/T original State/SecretStore/hazard rules, READ ONLY.
/// No signal, scheduler/Tool/Queue execution, Process or writer mutation.
pub(crate) fn inspect_cold_activation_dry_run(
    archive: &Path,
    cold_state: &Path,
    challenge: &cold_activation_consent::ColdDeviceChallenge,
    device_public_key: &str,
    detached_signature: &str,
) -> Result<ColdActivationDryRun> {
    inspect_cold_activation_dry_run_impl(
        archive,
        cold_state,
        challenge,
        device_public_key,
        detached_signature,
        |_| Ok(()),
    )
}

#[cfg(test)]
pub(crate) fn inspect_cold_activation_dry_run_with_hook<F: FnOnce(&Path) -> Result<()>>(
    archive: &Path,
    cold_state: &Path,
    challenge: &cold_activation_consent::ColdDeviceChallenge,
    device_public_key: &str,
    detached_signature: &str,
    after_snapshot: F,
) -> Result<ColdActivationDryRun> {
    inspect_cold_activation_dry_run_impl(
        archive,
        cold_state,
        challenge,
        device_public_key,
        detached_signature,
        after_snapshot,
    )
}

fn inspect_cold_activation_dry_run_impl<F: FnOnce(&Path) -> Result<()>>(
    archive: &Path,
    cold_state: &Path,
    challenge: &cold_activation_consent::ColdDeviceChallenge,
    device_public_key: &str,
    detached_signature: &str,
    after_snapshot: F,
) -> Result<ColdActivationDryRun> {
    cold::with_verified_cold_snapshot(
        archive,
        cold_state,
        |c, _, report, id, _| {
            inspect_on_connection(
                c,
                archive,
                &report,
                id,
                (challenge, device_public_key, detached_signature),
            )
        },
        || after_snapshot(cold_state),
    )
}

fn inspect_on_connection(
    c: &Connection,
    archive: &Path,
    inspection: &cold_quarantine_review::ColdQuarantineInspection,
    store_id: &str,
    evidence: (&cold_activation_consent::ColdDeviceChallenge, &str, &str),
) -> Result<ColdActivationDryRun> {
    let (challenge, device_public_key, detached_signature) = evidence;
    let evidence = cold_activation_consent::reverify_signed_cold_consent_evidence_on_connection(
        c,
        archive,
        inspection,
        store_id,
        challenge,
        device_public_key,
        detached_signature,
    )?;
    let active_processes = counted(
        c,
        "SELECT count(*) FROM local_process_incarnations WHERE state='active'",
    )?;
    let active_writers = counted(
        c,
        "SELECT count(*) FROM workspace_writer_state WHERE state='active'",
    )?;
    let writers = counted(c, "SELECT count(*) FROM workspace_writer_state")?;
    let invalid: i64 = db(c.query_row(
        "SELECT count(*) FROM workspace_writer_state
         WHERE state!='read_only' OR writer_epoch>=9223372036854775807
           OR revision>=9223372036854775807",
        [],
        |row| row.get(0),
    ))?;
    if active_processes != 0 || active_writers != 0 || invalid != 0 {
        return Err(Error::Denied(
            "cold dry-run cannot plan fresh authority from active or overflowed original writers",
        ));
    }
    let active_leases: i64 = db(c.query_row(
        "SELECT (SELECT count(*) FROM local_coordination_leases WHERE state='active')
              + (SELECT count(*) FROM local_maintenance_lease WHERE state='active')",
        [],
        |r| r.get(0),
    ))?;
    if active_leases != 0 {
        return Err(Error::Denied(
            "cold dry-run has active original copied leases",
        ));
    }
    // T/S already authenticated immutable cold_restore_quarantine epoch events.
    // This is only an indicative COUNT of future authorizations, not a mint.
    Ok(ColdActivationDryRun {
        generation: evidence.generation,
        active_writers,
        active_processes,
        would_require_fresh_writer_epochs: writers,
        consent_evidence_reverified: true,
        requires_fresh_human_origin: true,
        activation_authorized: false,
        runnable: false,
        original_request_id: evidence.request_id,
        original_owner_user_id: evidence.user_id,
        transcript_sha256: evidence.transcript_sha256,
        inspection_sha256: evidence.inspection_sha256,
        store_id: evidence.store_id,
    })
}

/// V2: owner-private monotonic ORIGINAL immutable v33 audit of a NO-EFFECT
/// diagnostic. The DB BEGIN IMMEDIATE serializes competing original owners
/// across native processes. This never makes the restored State runnable and
/// is not a fresh consent, Actor, Human approval, Tool or WriterEpoch grant.
pub(crate) fn record_cold_activation_dry_run(
    archive: &Path,
    cold_state: &Path,
    challenge: &cold_activation_consent::ColdDeviceChallenge,
    device_public_key: &str,
    detached_signature: &str,
    dry_run_id: &str,
) -> Result<ColdActivationDryRun> {
    record_cold_activation_dry_run_with_pretransaction(
        archive,
        cold_state,
        challenge,
        device_public_key,
        detached_signature,
        dry_run_id,
        |_| Ok(()),
    )
}

/// Private deterministic fault-injection seam: the callback runs AFTER the
/// first read-only signature/AEAD/risk proof, BEFORE the original SQLite
/// BEGIN IMMEDIATE audit transaction. Shipping always passes a no-op. This
/// cannot create an execution grant or invoke models/Tools itself.
pub(crate) fn record_cold_activation_dry_run_with_pretransaction<F>(
    archive: &Path,
    cold_state: &Path,
    challenge: &cold_activation_consent::ColdDeviceChallenge,
    device_public_key: &str,
    detached_signature: &str,
    dry_run_id: &str,
    before_transaction: F,
) -> Result<ColdActivationDryRun>
where
    F: FnOnce(&Path) -> Result<()>,
{
    record_cold_activation_dry_run_impl(
        archive,
        cold_state,
        challenge,
        device_public_key,
        detached_signature,
        dry_run_id,
        (before_transaction, |_| Ok(())),
    )
}

#[cfg(test)]
pub(crate) fn record_cold_activation_dry_run_with_hooks<F, G>(
    archive: &Path,
    cold_state: &Path,
    challenge: &cold_activation_consent::ColdDeviceChallenge,
    device_public_key: &str,
    detached_signature: &str,
    dry_run_id: &str,
    hooks: (F, G),
) -> Result<ColdActivationDryRun>
where
    F: FnOnce(&Path) -> Result<()>,
    G: FnOnce(&Path) -> Result<()>,
{
    record_cold_activation_dry_run_impl(
        archive,
        cold_state,
        challenge,
        device_public_key,
        detached_signature,
        dry_run_id,
        hooks,
    )
}

fn record_cold_activation_dry_run_impl<F, G>(
    archive: &Path,
    cold_state: &Path,
    challenge: &cold_activation_consent::ColdDeviceChallenge,
    device_public_key: &str,
    detached_signature: &str,
    dry_run_id: &str,
    hooks: (F, G),
) -> Result<ColdActivationDryRun>
where
    F: FnOnce(&Path) -> Result<()>,
    G: FnOnce(&Path) -> Result<()>,
{
    let (before_transaction, after_commit) = hooks;
    bounded_id(dry_run_id)?;
    let result = inspect_cold_activation_dry_run(
        archive,
        cold_state,
        challenge,
        device_public_key,
        detached_signature,
    )?;
    before_transaction(cold_state)?;
    let anchors = ColdFilesystemAnchors::capture(cold_state)?;
    let sqlite = cold_state.join("sessions.sqlite3");
    let mut c = db(Connection::open_with_flags(
        &sqlite,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ))?;
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let (_, report) =
        cold::original_facts_on_connection(&tx, &result.store_id, result.generation, || Ok(()))?;
    let fresh = inspect_on_connection(
        &tx,
        archive,
        &report,
        &result.store_id,
        (challenge, device_public_key, detached_signature),
    )?;
    if fresh != result {
        return Err(Error::Denied(
            "cold dry-run original consent/writer/effect evidence changed before audit",
        ));
    }
    let current: i64 = db(tx.query_row(
        "SELECT count(*) FROM local_coordination_events WHERE kind=?1 AND store_generation=?2",
        params![DRY_RUN_KIND, result.generation],
        |r| r.get(0),
    ))?;
    if current >= MAX_DRY_RUNS_PER_RESTORE {
        return Err(Error::Limit(
            "cold dry-run audit receipts exceed original generation limit",
        ));
    }
    let duplicate: bool = db(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM local_coordination_events
        WHERE kind=?1 AND store_generation=?2 AND json_extract(payload_json,'$.request_id')=?3)",
        params![DRY_RUN_KIND, result.generation, dry_run_id],
        |r| r.get(0),
    ))?;
    if duplicate {
        return Err(Error::Conflict(
            "cold dry-run audit identity already recorded by another owner process",
        ));
    }
    anchors.verify_source_before_review(
        archive,
        cold_state,
        &result.store_id,
        result.generation,
    )?;
    let payload = serde_json::json!({
        "version":1,
        "request_id":dry_run_id,
        "original_consent_request_id":result.original_request_id,
        "original_user_id":result.original_owner_user_id,
        "original_store_generation":result.generation,
        "signed_consent_transcript_sha256":result.transcript_sha256,
        "risk_summary_sha256":result.inspection_sha256,
        "device_signature_reverified":result.consent_evidence_reverified,
        "would_require_fresh_writer_epochs":result.would_require_fresh_writer_epochs,
        "requires_fresh_human_origin":true,
        "activation_authorized":false,
        "runnable":false,
        "replay_paid_tool_queue":false,
    });
    let encoded = serde_json::to_string(&payload)
        .map_err(|_| Error::Storage("cannot encode exact cold dry-run audit"))?;
    let event_id = crate::id("coord")?;
    let created_at = now();
    db(tx.execute("INSERT INTO local_coordination_events(event_id,store_id,store_generation,process_id,kind,scope_kind,scope_key,
        lease_generation,payload_json,created_at) VALUES(?1,?2,?3,NULL,?4,'cold_restore',?5,NULL,?6,?7)",
        params![event_id, result.store_id, result.generation, DRY_RUN_KIND, dry_run_id, encoded, created_at]))?;
    let seq = tx.last_insert_rowid();
    db(tx.commit()).map_err(|_| Error::Storage(COLD_AUDIT_OUTCOME_UNKNOWN))?;
    drop(c);
    ColdReviewAuditRow {
        seq,
        event_id: &event_id,
        store_id: &result.store_id,
        generation: result.generation,
        kind: DRY_RUN_KIND,
        scope_key: dry_run_id,
        payload_json: &encoded,
        created_at,
    }
    .acknowledge(
        archive,
        cold_state,
        &anchors,
        |c, _, report, id, _| {
            let after = inspect_on_connection(
                c,
                archive,
                &report,
                id,
                (challenge, device_public_key, detached_signature),
            )?;
            if after != result {
                return Err(Error::Denied(
                    "cold dry-run original evidence changed after commit",
                ));
            }
            Ok(after)
        },
        after_commit,
    )
}
