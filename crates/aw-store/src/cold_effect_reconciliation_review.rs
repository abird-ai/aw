//! AW-P6-10W8: original-owner, exact-effect, NON-EXECUTING review receipts.
//! Review is NOT settlement, fresh work consent, global clone arbitration,
//! human presence, a SourceGrant, a WriterEpoch, or restored-State activation.
#![allow(dead_code)] // private derived service; no shipping CLI/MCP/RPC surface

use super::*;
use aw_agent_core::PrincipalId;
use rusqlite::{params, types::ValueRef, ToSql, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};

const REVIEW_KIND: &str = "store.cold_effect_original_owner_review_requested";
/// A failed COMMIT, sync or post-commit security read does NOT mean the
/// immutable original review record was rolled back. The only safe operator
/// workflow is to inspect the original audit before any future request; it
/// never licenses historical provider/Tool/OS/Queue replay or activation.
const REVIEW_AUDIT_OUTCOME_UNKNOWN: &str =
    "cold original owner review audit outcome unknown after commit; inspect original immutable receipt; never automatically retry or execute";

const MAX_REVIEW_PER_RESTORE: i64 = 1024;
const MAX_SOURCE_ROWS: usize = 8192;
const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;

/// The original v33 primary key, not a natural-language task or model command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub(crate) enum ColdEffectSubject {
    ProviderRun {
        kind: EvidenceClass,
        run_id: String,
        original_key: String,
    },
    Tool {
        kind: EvidenceClass,
        run_id: String,
        call_id: String,
    },
    Actor {
        kind: EvidenceClass,
        session_id: String,
        request_id: String,
    },
    Recovery {
        kind: EvidenceClass,
        original_key: String,
    },
}

/// Never a disposition of the ORIGINAL effect. Both are only audit requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ColdEffectReviewIntent {
    KeepQuarantined,
    RequestFreshProposalReview,
}

impl ColdEffectReviewIntent {
    fn label(self) -> &'static str {
        match self {
            Self::KeepQuarantined => "keep_quarantined",
            Self::RequestFreshProposalReview => "request_fresh_proposal_review_only",
        }
    }
}

/// Private source-bound preview. Hashes are comparisons, NEVER bearer grants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ColdEffectReviewPlan {
    pub(crate) store_id: String,
    pub(crate) generation: u64,
    pub(crate) subject: ColdEffectSubject,
    pub(crate) original_evidence_sha256: String,
    pub(crate) original_snapshot_sha256: String,
    pub(crate) runnable: bool,
    pub(crate) permits_original_effect_replay: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ColdEffectReviewReceipt {
    pub(crate) request_id: String,
    pub(crate) generation: u64,
    pub(crate) subject: ColdEffectSubject,
    pub(crate) intent: ColdEffectReviewIntent,
    pub(crate) original_evidence_sha256: String,
    pub(crate) runnable: bool,
    pub(crate) original_effect_settled: bool,
    pub(crate) fresh_authority_granted: bool,
}

fn hashed(label: &[u8], input: &[u8]) -> String {
    let mut sha = Sha256::new();
    sha.update(label);
    sha.update((input.len() as u64).to_be_bytes());
    sha.update(input);
    format!("{:x}", sha.finalize())
}

// Stream canonical JSON directly into the digest: bounded W7 evidence may
// grow with many Sessions but should never require a second multi-MB copy.
// The v2 hash domain is intentionally fixed separately from pre-release W8
// fixtures; a later representation change MUST mint a new audit version.
const MAX_SNAPSHOT_DIGEST_BYTES: usize = 128 * 1024 * 1024;

struct SnapshotHashWriter {
    digest: Sha256,
    bytes: usize,
}
impl std::io::Write for SnapshotHashWriter {
    fn write(&mut self, value: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(value.len())
            .filter(|n| *n <= MAX_SNAPSHOT_DIGEST_BYTES)
            .ok_or(std::io::Error::other(
                "original identity snapshot exceeds bounded digest budget",
            ))?;
        self.digest.update(value);
        Ok(value.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn fingerprint_snapshot(snapshot: &ColdEffectSnapshot) -> Result<String> {
    let mut writer = SnapshotHashWriter {
        digest: Sha256::new(),
        bytes: 0,
    };
    writer.digest.update(b"aw.w8.original.snapshot.v2\0");
    serde_json::to_writer(&mut writer, snapshot)
        .map_err(|_| Error::Limit("cannot stream bounded original risk identity snapshot"))?;
    writer.digest.update((writer.bytes as u64).to_be_bytes());
    Ok(format!("{:x}", writer.digest.finalize()))
}

fn evidence_payload(snapshot: &ColdEffectSnapshot, key: &ColdEffectSubject) -> Result<Vec<u8>> {
    let encoded = match key {
        ColdEffectSubject::ProviderRun {
            kind,
            run_id,
            original_key,
        } => snapshot
            .provider_and_runs
            .iter()
            .find(|item| {
                item.kind == *kind && item.run_id == *run_id && item.original_key == *original_key
            })
            .map(serde_json::to_vec),
        ColdEffectSubject::Tool {
            kind,
            run_id,
            call_id,
        } => snapshot
            .tools
            .iter()
            .find(|item| item.kind == *kind && item.run_id == *run_id && item.call_id == *call_id)
            .map(serde_json::to_vec),
        ColdEffectSubject::Actor {
            kind,
            session_id,
            request_id,
        } => snapshot
            .actor
            .iter()
            .find(|item| {
                item.kind == *kind
                    && item.session_id == *session_id
                    && item.request_id == *request_id
            })
            .map(serde_json::to_vec),
        ColdEffectSubject::Recovery { kind, original_key } => snapshot
            .recovery
            .iter()
            .find(|item| item.kind == *kind && item.original_key == *original_key)
            .map(serde_json::to_vec),
    }
    .ok_or(Error::Denied(
        "original cold effect not present as an unresolved obligation",
    ))?
    .map_err(|_| Error::Storage("cannot encode exact original effect evidence"))?;
    if encoded.len() > 8192 {
        return Err(Error::Limit(
            "original effect metadata exceeds private review limit",
        ));
    }
    Ok(encoded)
}

fn validated_subject(key: &ColdEffectSubject) -> Result<Vec<u8>> {
    let encoded =
        serde_json::to_vec(key).map_err(|_| Error::Denied("invalid original effect selector"))?;
    if encoded.is_empty() || encoded.len() > 2048 {
        return Err(Error::Limit("original effect selector exceeds bound"));
    }
    let fields: Vec<&str> = match key {
        ColdEffectSubject::ProviderRun {
            run_id,
            original_key,
            ..
        } => vec![run_id, original_key],
        ColdEffectSubject::Tool {
            run_id, call_id, ..
        } => vec![run_id, call_id],
        ColdEffectSubject::Actor {
            session_id,
            request_id,
            ..
        } => vec![session_id, request_id],
        ColdEffectSubject::Recovery { original_key, .. } => vec![original_key],
    };
    if fields
        .iter()
        .any(|s| s.is_empty() || s.len() > 512 || s.chars().any(char::is_control))
    {
        return Err(Error::Denied("original effect key is missing or unbounded"));
    }
    Ok(encoded)
}

/// Include ALL original SQL columns and bounded immutable payload bytes for
/// the selected original source. Only SHA256 leaves the read scope. Changing
/// a Tool argument/Queue claim/Provider attempt with the same key changes its
/// evidence hash and invalidates the owner's preflight review plan.
fn source_rows(c: &Connection, sql: &str, bindings: &[&dyn ToSql], sha: &mut Sha256) -> Result<()> {
    source_rows_impl(c, sql, bindings, sha, true)
}

/// Related original rows may legitimately be absent, but the primary exact
/// selected effect row must exist. The source tag/count is hashed either way.
fn source_rows_optional(
    c: &Connection,
    sql: &str,
    bindings: &[&dyn ToSql],
    sha: &mut Sha256,
) -> Result<()> {
    source_rows_impl(c, sql, bindings, sha, false)
}

fn source_rows_impl(
    c: &Connection,
    sql: &str,
    bindings: &[&dyn ToSql],
    sha: &mut Sha256,
    require_one: bool,
) -> Result<()> {
    let mut statement = crate::db(c.prepare(sql))?;
    let columns = statement.column_count();
    let names = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut rows = crate::db(statement.query(bindings))?;
    let mut count = 0;
    let mut budget = 0usize;
    sha.update((sql.len() as u64).to_be_bytes());
    sha.update(sql.as_bytes());
    for name in names {
        sha.update((name.len() as u64).to_be_bytes());
        sha.update(name.as_bytes());
    }
    while let Some(row) = crate::db(rows.next())? {
        count += 1;
        if count > MAX_SOURCE_ROWS {
            return Err(Error::Limit(
                "original effect receipt rows exceed private review budget",
            ));
        }
        for i in 0..columns {
            let value = crate::db(row.get_ref(i))?;
            let (tag, bytes): (u8, std::borrow::Cow<'_, [u8]>) = match value {
                ValueRef::Null => (0, std::borrow::Cow::Borrowed(&[])),
                ValueRef::Integer(n) => (1, std::borrow::Cow::Owned(n.to_be_bytes().to_vec())),
                ValueRef::Real(n) => (
                    2,
                    std::borrow::Cow::Owned(n.to_bits().to_be_bytes().to_vec()),
                ),
                ValueRef::Text(data) => (3, std::borrow::Cow::Borrowed(data)),
                ValueRef::Blob(data) => (4, std::borrow::Cow::Borrowed(data)),
            };
            budget = budget
                .checked_add(bytes.len())
                .ok_or(Error::Limit("effect review evidence size overflow"))?;
            if budget > MAX_SOURCE_BYTES {
                return Err(Error::Limit(
                    "original effect receipt bytes exceed review budget",
                ));
            }
            sha.update([tag]);
            sha.update((bytes.len() as u64).to_be_bytes());
            sha.update(&bytes);
        }
    }
    if count == 0 && require_one {
        return Err(Error::Denied("original effect source record disappeared"));
    }
    sha.update((count as u64).to_be_bytes());
    Ok(())
}

fn fingerprint_source_rows(
    c: &Connection,
    key: &ColdEffectSubject,
    sha: &mut Sha256,
) -> Result<()> {
    match key {
        ColdEffectSubject::ProviderRun { kind, run_id, original_key } => {
            // A potentially paid original attempt belongs to an exact Run,
            // payer/credential identity and immutable invocation receipt set.
            // Bind those causally related originals without claiming invoices.
            match kind {
                EvidenceClass::UnfinishedRun => source_rows(c,
                    "SELECT * FROM runs WHERE id=?1 AND status='running' ORDER BY id",
                    &[run_id], sha)?,
                EvidenceClass::ProviderReceipt => source_rows(c,
                    "SELECT * FROM invocation_receipts WHERE run_id=?1 AND CAST(ordinal AS TEXT)=?2 AND state IN ('started','unknown') ORDER BY ordinal",
                    &[run_id, original_key], sha)?,
                EvidenceClass::PossiblyPaidAttempt => source_rows(c,
                    "SELECT * FROM llm_attempts WHERE run_id=?1 AND id=?2 AND dispatch_state IN ('unknown','provider_started') ORDER BY id",
                    &[run_id, original_key], sha)?,
                EvidenceClass::ExternalEffectHazard => source_rows(c,
                    "SELECT * FROM hazards WHERE run_id=?1 AND state IN ('running','needs_inspection') ORDER BY run_id",
                    &[run_id], sha)?,
                _ => return Err(Error::Denied("incorrect original Provider/Run effect category")),
            }
            source_rows(c, "SELECT * FROM runs WHERE id=?1 ORDER BY id", &[run_id], sha)?;
            source_rows_optional(c,
                "SELECT * FROM invocation_receipts WHERE run_id=?1 ORDER BY ordinal",
                &[run_id], sha)?;
            source_rows_optional(c,
                "SELECT * FROM llm_attempts WHERE run_id=?1 ORDER BY attempt_no",
                &[run_id], sha)?;
            source_rows_optional(c,
                "SELECT * FROM llm_operations WHERE id IN (SELECT operation_id FROM llm_attempts WHERE run_id=?1) ORDER BY id",
                &[run_id], sha)
        }
        ColdEffectSubject::Tool { run_id, call_id, .. } => source_rows(c,
            "SELECT * FROM run_events WHERE run_id=?1 AND (
             (kind IN ('tool_started','tool_finished') AND json_extract(payload,'$.record.call.id')=?2)
             OR (kind='tool_revised' AND (json_extract(payload,'$.original.id')=?2 OR json_extract(payload,'$.revised.id')=?2))
             OR (kind='assistant' AND EXISTS (
               SELECT 1 FROM json_each(payload,'$.message.tool_calls')
               WHERE json_extract(value,'$.id')=?2))) ORDER BY seq",
            &[run_id, call_id], sha),
        ColdEffectSubject::Actor { kind, session_id, request_id } => {
            // Include all original immutable revisions, not merely the latest
            // summarized Queue claim or Actor Input/Control admission.
            match kind {
                EvidenceClass::QueuePending | EvidenceClass::QueueClaimed | EvidenceClass::QueueAccepted =>
                    source_rows(c,
                        "SELECT * FROM audit WHERE session_id=?1 AND action='actor.queue' AND json_extract(data_json,'$.request.request_id')=?2 ORDER BY seq",
                        &[session_id, request_id], sha),
                EvidenceClass::ActorInput | EvidenceClass::ActorControl => {
                    let action = if *kind == EvidenceClass::ActorInput { "actor.input" } else { "actor.control" };
                    source_rows(c,
                        "SELECT * FROM audit WHERE session_id=?1 AND action=?2 AND json_extract(data_json,'$.admission.request_id')=?3 ORDER BY seq",
                        &[session_id, &action, request_id], sha)
                }
                _ => Err(Error::Denied("incorrect original Actor effect category")),
            }
        }
        ColdEffectSubject::Recovery { kind, original_key } => match kind {
            EvidenceClass::ExecutionLease => source_rows(c,
                "SELECT * FROM local_coordination_leases WHERE scope_kind='execution' AND state='hazard' AND scope_key=?1 ORDER BY scope_key",
                &[original_key], sha),
            EvidenceClass::RecoveryHint => source_rows(c,
                "SELECT * FROM recovery_hints WHERE state='open' AND id=?1 ORDER BY id",
                &[original_key], sha),
            EvidenceClass::RetrySchedule => source_rows(c,
                "SELECT * FROM retry_schedules WHERE state IN ('waiting_user','scheduled','claimed','blocked') AND operation_id=?1 ORDER BY operation_id",
                &[original_key], sha),
            _ => Err(Error::Denied("incorrect original recovery effect category")),
        },
    }
}

fn plan_from_snapshot(
    c: &Connection,
    snapshot: &ColdEffectSnapshot,
    store_id: &str,
    generation: u64,
    subject: &ColdEffectSubject,
) -> Result<ColdEffectReviewPlan> {
    let selector = validated_subject(subject)?;
    let identity = evidence_payload(snapshot, subject)?;
    let mut digest = Sha256::new();
    digest.update(b"aw.w8.original.effect.receipts.v1\0");
    for part in [
        store_id.as_bytes(),
        &generation.to_be_bytes(),
        &selector,
        &identity,
    ] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    fingerprint_source_rows(c, subject, &mut digest)?;
    Ok(ColdEffectReviewPlan {
        store_id: store_id.to_owned(),
        generation,
        subject: subject.clone(),
        original_evidence_sha256: format!("{:x}", digest.finalize()),
        original_snapshot_sha256: fingerprint_snapshot(snapshot)?,
        runnable: false,
        permits_original_effect_replay: false,
    })
}

/// W7's original P/Q/R/S/T archive + dual quarantine fences wrap the exact
/// same source snapshot. This is a review PREVIEW, not a consent challenge.
pub(crate) fn prepare_exact_original_effect_review(
    archive: &Path,
    cold_state: &Path,
    subject: &ColdEffectSubject,
) -> Result<ColdEffectReviewPlan> {
    super::with_verified_cold_snapshot(
        archive,
        cold_state,
        |tx, snapshot, _, store_id, generation| {
            plan_from_snapshot(tx, &snapshot, store_id, generation, subject)
        },
        || Ok(()),
    )
}

fn bounded_request_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(Error::Denied(
            "original effect review requires one bounded request identity",
        ));
    }
    Ok(())
}

/// An ORIGINAL owner may append one immutable review REQUEST for ONE exact
/// outstanding source effect. No actor, provider, Tool or Queue operation,
/// no original status mutation or billing settlement, no runnable activation.
pub(crate) fn record_original_owner_effect_review(
    archive: &Path,
    cold_state: &Path,
    owner: &PrincipalId,
    request_id: &str,
    plan: &ColdEffectReviewPlan,
    intent: ColdEffectReviewIntent,
) -> Result<ColdEffectReviewReceipt> {
    record_original_owner_effect_review_impl(
        archive,
        cold_state,
        owner,
        request_id,
        plan,
        intent,
        |_| Ok(()),
        |_| Ok(()),
    )
}

/// Deliberate test-only race/crash seam; production does not expose callback.
#[cfg(test)]
pub(crate) fn record_original_owner_effect_review_with_hook<F: FnOnce(&Path) -> Result<()>>(
    archive: &Path,
    cold_state: &Path,
    owner: &PrincipalId,
    request_id: &str,
    plan: &ColdEffectReviewPlan,
    intent: ColdEffectReviewIntent,
    before_transaction: F,
) -> Result<ColdEffectReviewReceipt> {
    record_original_owner_effect_review_impl(
        archive,
        cold_state,
        owner,
        request_id,
        plan,
        intent,
        before_transaction,
        |_| Ok(()),
    )
}

/// Inject a genuine OS process exit after commit, before explicit fsync, or
/// run a pretransaction fault. Excluded from all non-test builds.
#[cfg(test)]
pub(crate) fn record_original_owner_effect_review_with_crash_seams<
    F: FnOnce(&Path) -> Result<()>,
    G: FnOnce(&Path) -> Result<()>,
>(
    archive: &Path,
    cold_state: &Path,
    owner: &PrincipalId,
    request_id: &str,
    plan: &ColdEffectReviewPlan,
    intent: ColdEffectReviewIntent,
    before_transaction: F,
    after_commit: G,
) -> Result<ColdEffectReviewReceipt> {
    record_original_owner_effect_review_impl(
        archive,
        cold_state,
        owner,
        request_id,
        plan,
        intent,
        before_transaction,
        after_commit,
    )
}

fn record_original_owner_effect_review_impl<
    F: FnOnce(&Path) -> Result<()>,
    G: FnOnce(&Path) -> Result<()>,
>(
    archive: &Path,
    cold_state: &Path,
    owner: &PrincipalId,
    request_id: &str,
    plan: &ColdEffectReviewPlan,
    intent: ColdEffectReviewIntent,
    before_transaction: F,
    after_commit: G,
) -> Result<ColdEffectReviewReceipt> {
    bounded_request_id(request_id)?;
    if plan.runnable || plan.permits_original_effect_replay {
        return Err(Error::Denied(
            "original effect review preview claims forbidden authority",
        ));
    }
    // First independently verify original archived source, encrypted custody,
    // exact original v33 effects and both cold quarantine guards.
    let preflight = prepare_exact_original_effect_review(archive, cold_state, &plan.subject)?;
    if preflight != *plan {
        return Err(Error::Denied(
            "original effect review preview became stale or forged",
        ));
    }
    before_transaction(cold_state)?;
    let (store_id, generation) =
        crate::composite_staging::verify_cold_for_inspection(archive, cold_state)?;
    if store_id != plan.store_id || generation != plan.generation {
        return Err(Error::Denied(
            "original owner review Store or generation changed",
        ));
    }
    let anchors = super::ColdFilesystemAnchors::capture(cold_state)?;
    let sqlite = cold_state.join("sessions.sqlite3");
    let mut conn = crate::db(Connection::open_with_flags(
        &sqlite,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ))?;
    let tx = crate::db(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
    // Re-read *all* original T risks and exact selected W7 evidence IN the
    // same transaction as the future immutable coordination event INSERT.
    let (snapshot, _) = super::original_facts_on_connection(&tx, &store_id, generation, || Ok(()))?;
    let fresh = plan_from_snapshot(&tx, &snapshot, &store_id, generation, &plan.subject)?;
    if fresh != *plan {
        return Err(Error::Denied(
            "original effect source changed before review audit transaction",
        ));
    }
    crate::cold_quarantine_review::require_original_cold_owner_of_all_sessions(&tx, owner)?;
    // Old Process/WriterEpoch/claim authority is never transferred. Review is
    // only permitted while the copied State is still wholly non-executable.
    let active: i64 = crate::db(tx.query_row(
        "SELECT (SELECT count(*) FROM local_process_incarnations WHERE state='active')
              + (SELECT count(*) FROM workspace_writer_state WHERE state='active')
              + (SELECT count(*) FROM local_coordination_leases WHERE state='active')
              + (SELECT count(*) FROM local_maintenance_lease WHERE state='active')",
        [],
        |r| r.get(0),
    ))?;
    if active != 0 {
        return Err(Error::Denied(
            "original cold review detected live copied execution authority",
        ));
    }
    let prior: i64 = crate::db(tx.query_row(
        "SELECT count(*) FROM local_coordination_events WHERE kind=?1 AND store_id=?2 AND store_generation=?3",
        params![REVIEW_KIND, store_id, generation], |r| r.get(0)))?;
    if prior >= MAX_REVIEW_PER_RESTORE {
        return Err(Error::Limit("original effect review audit budget exceeded"));
    }
    let subject_json = serde_json::to_string(&plan.subject)
        .map_err(|_| Error::Storage("cannot encode original review effect identity"))?;
    let subject_key = hashed(b"aw.w8.original.subject.v1\0", subject_json.as_bytes());
    let duplicate: bool = crate::db(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM local_coordination_events
          WHERE kind=?1 AND store_id=?2 AND store_generation=?3
            AND (scope_key=?4 OR json_extract(payload_json,'$.request_id')=?5))",
        params![REVIEW_KIND, store_id, generation, subject_key, request_id],
        |r| r.get(0),
    ))?;
    if duplicate {
        return Err(Error::Conflict(
            "original effect already has a review request or request ID",
        ));
    }
    // Run the full ORIGINAL Q/R/S/T source/custody/dual-denial verification
    // while the BEGIN IMMEDIATE lock is still held, BEFORE audit publication.
    anchors.verify(cold_state)?;
    let (actual_id, actual_gen) =
        crate::composite_staging::verify_cold_for_inspection(archive, cold_state)?;
    if actual_id != store_id || actual_gen != generation {
        return Err(Error::Denied(
            "original effect review post-race archive/quarantine mismatch",
        ));
    }
    anchors.verify(cold_state)?;
    let payload = serde_json::json!({
        "version": 1, "request_id": request_id, "original_owner": owner.as_str(),
        "store_id": store_id, "restored_generation": generation,
        "subject": plan.subject, "intent": intent.label(),
        "original_evidence_sha256": plan.original_evidence_sha256,
        "original_snapshot_sha256": plan.original_snapshot_sha256,
        "review_only": true,
        "independent_live_human_attested": false,
        "globally_unique_new_authority": false,
        "original_effect_settled": false,
        "unknown_provider_cost_preserved": true,
        "fresh_authority_granted": false,
        "activation_authorized": false, "runnable": false,
        "automatic_replay_allowed": false,
    });
    let encoded = serde_json::to_string(&payload)
        .map_err(|_| Error::Storage("cannot encode original effect owner review fact"))?;
    if encoded.len() > 16_384 {
        return Err(Error::Limit("original effect owner review fact too large"));
    }
    let event_id = crate::id("coord")?;
    let created_at = crate::now();
    crate::db(tx.execute(
        "INSERT INTO local_coordination_events(
          event_id,store_id,store_generation,process_id,kind,scope_kind,scope_key,
          lease_generation,payload_json,created_at)
         VALUES(?1,?2,?3,NULL,?4,'cold_restore',?5,NULL,?6,?7)",
        params![
            event_id,
            store_id,
            generation,
            REVIEW_KIND,
            subject_key,
            encoded,
            created_at
        ],
    ))?;
    let event_seq = tx.last_insert_rowid();
    // SQLite may have committed even when the COMMIT acknowledgement fails.
    // Never expose a generic failure that callers might mistake for a safe
    // rollback and then automatically submit a second audit or old effect.
    crate::db(tx.commit()).map_err(|_| Error::Storage(REVIEW_AUDIT_OUTCOME_UNKNOWN))?;
    // Everything after the first irreversible commit is an UNCERTAIN outcome
    // until the immutable row, original custody and filesystem are rechecked.
    // Even an injected test fault cannot turn that result into a retry grant.
    let postcommit = (|| -> Result<()> {
        after_commit(cold_state)?;
        drop(conn);
        anchors.verify_after_review_commit(cold_state)?;
        let file = aw_platform::open_private_existing(&sqlite, false)?;
        file.sync_all()?;
        aw_platform::sync_parent(&sqlite)?;
        // An audit receipt is not a readiness verdict. Original outstanding
        // effects remain UNKNOWN and neither quarantine guard is removed.
        super::with_verified_cold_snapshot(
            archive,
            cold_state,
            |tx, snapshot, _, after_id, after_generation| {
                if after_id != store_id
                    || after_generation != generation
                    || plan_from_snapshot(tx, &snapshot, after_id, after_generation, &plan.subject)?
                        != *plan
                {
                    return Err(Error::Denied(
                        "cold owner audit post-commit original evidence changed",
                    ));
                }
                super::ColdReviewAuditRow {
                    seq: event_seq,
                    event_id: &event_id,
                    store_id: &store_id,
                    generation,
                    kind: REVIEW_KIND,
                    scope_key: &subject_key,
                    payload_json: &encoded,
                    created_at,
                }
                .verify(tx)?;
                Ok(())
            },
            || anchors.verify_after_review_commit(cold_state),
        )
    })();
    postcommit.map_err(|_| Error::Storage(REVIEW_AUDIT_OUTCOME_UNKNOWN))?;
    Ok(ColdEffectReviewReceipt {
        request_id: request_id.to_owned(),
        generation,
        subject: plan.subject.clone(),
        intent,
        original_evidence_sha256: plan.original_evidence_sha256.clone(),
        runnable: false,
        original_effect_settled: false,
        fresh_authority_granted: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_snapshot() -> ColdEffectSnapshot {
        let report = ColdQuarantineInspection {
            generation: 2,
            sessions: 1,
            unfinished_runs: 0,
            uncertain_provider_receipts: 0,
            possibly_paid_attempts: 0,
            unresolved_effect_hazards: 0,
            hazardous_execution_leases: 0,
            incomplete_tool_intents: 0,
            mismatched_tool_receipts: 0,
            queued_pending: 0,
            queued_claimed: 0,
            queued_accepted: 0,
            actor_input_needs_inspection: 0,
            actor_control_needs_inspection: 0,
            open_recovery_hints: 0,
            retry_schedules_requiring_owner: 0,
            outstanding_review: false,
            activation_authorized: false,
        };
        ColdEffectSnapshot {
            preparation: super::super::classify(&report),
            provider_and_runs: Vec::new(),
            actor: Vec::new(),
            tools: Vec::new(),
            recovery: Vec::new(),
        }
    }

    #[test]
    fn original_provider_payer_or_credentials_change_invalidates_same_run_id_review() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE runs(id TEXT,status TEXT,identity_json TEXT);
                         CREATE TABLE invocation_receipts(run_id TEXT,ordinal INTEGER,state TEXT);
                         CREATE TABLE llm_attempts(id TEXT,run_id TEXT,operation_id TEXT,attempt_no INTEGER,dispatch_state TEXT);
                         CREATE TABLE llm_operations(id TEXT,identity_json TEXT);
                         INSERT INTO runs VALUES('run-a','running','{\"payer\":\"first\"}');
                         INSERT INTO llm_attempts VALUES('attempt-a','run-a','operation-a',1,'provider_started');
                         INSERT INTO llm_operations VALUES('operation-a','{\"credential\":\"first\"}');").unwrap();
        let mut original = empty_snapshot();
        original.provider_and_runs.push(OriginalEffectIdentity {
            kind: EvidenceClass::UnfinishedRun,
            run_id: "run-a".into(),
            original_key: "run-a".into(),
        });
        let key = ColdEffectSubject::ProviderRun {
            kind: EvidenceClass::UnfinishedRun,
            run_id: "run-a".into(),
            original_key: "run-a".into(),
        };
        let first = plan_from_snapshot(&c, &original, "store-a", 2, &key).unwrap();
        c.execute(
            "UPDATE llm_operations SET identity_json='{\"credential\":\"second\"}'",
            [],
        )
        .unwrap();
        let changed = plan_from_snapshot(&c, &original, "store-a", 2, &key).unwrap();
        assert_ne!(
            first.original_evidence_sha256,
            changed.original_evidence_sha256
        );
        c.execute("UPDATE runs SET identity_json='{\"payer\":\"second\"}'", [])
            .unwrap();
        assert_ne!(
            changed.original_evidence_sha256,
            plan_from_snapshot(&c, &original, "store-a", 2, &key)
                .unwrap()
                .original_evidence_sha256
        );
        assert!(!first.runnable && !changed.permits_original_effect_replay);
    }

    #[test]
    fn original_tool_revision_changes_invalidate_same_call_id_review() {
        use aw_agent_core::{
            JournalEntry, ToolCall, ToolEffect, ToolRecord, ToolSourceEvidence, ToolStatus,
        };
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE run_events(seq INTEGER,run_id TEXT,kind TEXT,payload TEXT);")
            .unwrap();
        let running = ToolRecord {
            call: ToolCall {
                id: "call-a".into(),
                name: "read".into(),
                arguments: serde_json::json!({"path":"original"}),
            },
            effect: ToolEffect::Read,
            execution_site: "site-a".into(),
            status: ToolStatus::Running,
            output: None,
        };
        c.execute(
            "INSERT INTO run_events VALUES(1,'run-a','tool_started',?1)",
            [serde_json::to_string(&JournalEntry::ToolStarted {
                record: running.clone(),
                source: Some(ToolSourceEvidence::trusted_host()),
            })
            .unwrap()],
        )
        .unwrap();
        let mut snapshot = empty_snapshot();
        snapshot.tools.push(OriginalToolEffectIdentity {
            kind: EvidenceClass::ToolIntent,
            run_id: "run-a".into(),
            call_id: "call-a".into(),
            starts: 1,
            finishes: 0,
            original_source: Some(ToolSourceEvidence::trusted_host()),
            original_site: "site-a".into(),
        });
        let key = ColdEffectSubject::Tool {
            kind: EvidenceClass::ToolIntent,
            run_id: "run-a".into(),
            call_id: "call-a".into(),
        };
        let first = plan_from_snapshot(&c, &snapshot, "store-a", 2, &key).unwrap();
        // A different original argument under the same Tool ID cannot be
        // transformed into the existing owner's verified review evidence.
        let mut changed = running;
        changed.call.arguments = serde_json::json!({"path":"different"});
        c.execute(
            "UPDATE run_events SET payload=?1",
            [serde_json::to_string(&JournalEntry::ToolStarted {
                record: changed,
                source: Some(ToolSourceEvidence::trusted_host()),
            })
            .unwrap()],
        )
        .unwrap();
        let second = plan_from_snapshot(&c, &snapshot, "store-a", 2, &key).unwrap();
        assert_ne!(
            first.original_evidence_sha256,
            second.original_evidence_sha256
        );
        assert_eq!(first.subject, second.subject);
        assert_eq!(
            first.original_snapshot_sha256,
            second.original_snapshot_sha256
        );
    }

    #[test]
    fn original_assistant_proposal_and_tool_revision_are_exactly_bound() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE run_events(seq INTEGER,run_id TEXT,kind TEXT,payload TEXT);")
            .unwrap();
        let running = serde_json::json!({
            "type":"tool_started",
            "record":{"call":{"id":"call-a","name":"bash","arguments":{"command":"a"}},
                "effect":"process","execution_site":"site-a","status":"running","output":null},
            "source":{"kind":"host","id":"trusted-host","epoch":1},
        });
        let assistant = serde_json::json!({"type":"assistant", "message":{
            "tool_calls":[{"id":"call-a","name":"bash","arguments":{"command":"a"}}],
            "content":"untrusted proposal"}});
        let revised = serde_json::json!({"type":"tool_revised",
            "original":{"id":"call-a","name":"bash","arguments":{"command":"a"}},
            "revised":{"id":"call-a","name":"bash","arguments":{"command":"b"}},
            "source_id":"trusted-host","source_kind":"host","source_epoch":1,
            "effect":"process"});
        for (index, (kind, payload)) in [
            ("assistant", assistant),
            ("tool_revised", revised.clone()),
            ("tool_started", running),
        ]
        .into_iter()
        .enumerate()
        {
            c.execute(
                "INSERT INTO run_events VALUES(?1,'run-a',?2,?3)",
                rusqlite::params![index as i64 + 1, kind, payload.to_string()],
            )
            .unwrap();
        }
        let mut snapshot = empty_snapshot();
        snapshot.tools.push(OriginalToolEffectIdentity {
            kind: EvidenceClass::ToolIntent,
            run_id: "run-a".into(),
            call_id: "call-a".into(),
            starts: 1,
            finishes: 0,
            original_source: Some(aw_agent_core::ToolSourceEvidence::trusted_host()),
            original_site: "site-a".into(),
        });
        let selector = ColdEffectSubject::Tool {
            kind: EvidenceClass::ToolIntent,
            run_id: "run-a".into(),
            call_id: "call-a".into(),
        };
        let first = plan_from_snapshot(&c, &snapshot, "store-a", 2, &selector).unwrap();
        c.execute(
            "UPDATE run_events SET payload=?1 WHERE kind='tool_revised'",
            [serde_json::json!({"type":"tool_revised",
                "original":{"id":"call-a","name":"bash","arguments":{"command":"a"}},
                "revised":{"id":"call-a","name":"bash","arguments":{"command":"unapproved"}},
                "source_id":"trusted-host","source_kind":"host","source_epoch":1,
                "effect":"process"})
            .to_string()],
        )
        .unwrap();
        let second = plan_from_snapshot(&c, &snapshot, "store-a", 2, &selector).unwrap();
        assert_ne!(
            first.original_evidence_sha256, second.original_evidence_sha256,
            "same Tool ID with different original revision must invalidate owner review"
        );
        c.execute(
            "UPDATE run_events SET payload=?1 WHERE kind='assistant'",
            [serde_json::json!({"type":"assistant", "message":{
                "tool_calls":[{"id":"call-a","name":"bash","arguments":{"command":"unapproved"}}],
                "content":"changed"}})
            .to_string()],
        )
        .unwrap();
        let third = plan_from_snapshot(&c, &snapshot, "store-a", 2, &selector).unwrap();
        assert_ne!(
            second.original_evidence_sha256, third.original_evidence_sha256,
            "original Assistant proposal is factual provenance, not just ToolStarted"
        );
    }

    #[test]
    fn original_selector_wrong_kind_and_unbounded_input_fail_closed() {
        let c = Connection::open_in_memory().unwrap();
        let snapshot = empty_snapshot();
        let missing = ColdEffectSubject::Recovery {
            kind: EvidenceClass::PossiblyPaidAttempt,
            original_key: "uncertain-charge".into(),
        };
        assert!(plan_from_snapshot(&c, &snapshot, "store-a", 2, &missing).is_err());
        let oversized = ColdEffectSubject::Tool {
            kind: EvidenceClass::ToolIntent,
            run_id: "x".repeat(1024),
            call_id: "call-a".into(),
        };
        assert!(validated_subject(&oversized).is_err());
        assert!(bounded_request_id("invalid/new/request").is_err());
    }
}
