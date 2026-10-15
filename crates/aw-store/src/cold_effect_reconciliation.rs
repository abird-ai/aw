//! AW-P6-10W1: private, bounded, non-executing effect-reconciliation PREPARE.
//! A count is an inspection obligation, never proof that a paid or external
//! effect did not happen. No disposition here mutates original durable facts.
#![cfg(target_os = "linux")]
#![allow(dead_code)] // deliberately not exposed to any shipped execution surface

use crate::cold_quarantine_review::ColdQuarantineInspection;
use crate::{actor_admission, actor_control, actor_queue, QueueState};
use aw_agent_core::{Error, JournalEntry, Result, ToolSourceEvidence};
use rusqlite::{Connection, OpenFlags};
use std::collections::BTreeMap;
use std::{
    fs::{File, Metadata},
    os::unix::fs::MetadataExt,
    path::Path,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EvidenceClass {
    UnfinishedRun,
    ProviderReceipt,
    PossiblyPaidAttempt,
    ExternalEffectHazard,
    ExecutionLease,
    ToolIntent,
    ToolReceiptMismatch,
    QueuePending,
    QueueClaimed,
    QueueAccepted,
    ActorInput,
    ActorControl,
    RecoveryHint,
    RetrySchedule,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct ReconciliationObligation {
    pub(crate) kind: EvidenceClass,
    pub(crate) count: usize,
    /// Every nonzero item requires independent original-source examination;
    /// even Pending queue items cannot be promoted to executable work.
    pub(crate) source_evidence_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct ColdEffectPreparation {
    pub(crate) generation: u64,
    pub(crate) obligations: Vec<ReconciliationObligation>,
    pub(crate) requires_independent_human_origin: bool,
    pub(crate) requires_cross_clone_exclusion: bool,
    pub(crate) permits_original_effect_replay: bool,
    pub(crate) runnable: bool,
}

fn classify(report: &ColdQuarantineInspection) -> ColdEffectPreparation {
    let candidates = [
        (EvidenceClass::UnfinishedRun, report.unfinished_runs),
        (
            EvidenceClass::ProviderReceipt,
            report.uncertain_provider_receipts,
        ),
        (
            EvidenceClass::PossiblyPaidAttempt,
            report.possibly_paid_attempts,
        ),
        (
            EvidenceClass::ExternalEffectHazard,
            report.unresolved_effect_hazards,
        ),
        (
            EvidenceClass::ExecutionLease,
            report.hazardous_execution_leases,
        ),
        (EvidenceClass::ToolIntent, report.incomplete_tool_intents),
        (
            EvidenceClass::ToolReceiptMismatch,
            report.mismatched_tool_receipts,
        ),
        (EvidenceClass::QueuePending, report.queued_pending),
        (EvidenceClass::QueueClaimed, report.queued_claimed),
        (EvidenceClass::QueueAccepted, report.queued_accepted),
        (
            EvidenceClass::ActorInput,
            report.actor_input_needs_inspection,
        ),
        (
            EvidenceClass::ActorControl,
            report.actor_control_needs_inspection,
        ),
        (EvidenceClass::RecoveryHint, report.open_recovery_hints),
        (
            EvidenceClass::RetrySchedule,
            report.retry_schedules_requiring_owner,
        ),
    ];
    ColdEffectPreparation {
        generation: report.generation,
        obligations: candidates
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .map(|(kind, count)| ReconciliationObligation {
                kind,
                count,
                source_evidence_required: true,
            })
            .collect(),
        requires_independent_human_origin: true,
        requires_cross_clone_exclusion: true,
        permits_original_effect_replay: false,
        runnable: false,
    }
}

/// Exact original SQLite keys, never a new identifier or a replay ticket.
/// These records remain subject to the owner/SourceKind/site/Actor checks
/// performed at the ORIGINAL execution boundary, not by this inspector.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct OriginalEffectIdentity {
    pub(crate) kind: EvidenceClass,
    pub(crate) run_id: String,
    pub(crate) original_key: String,
}

const MAX_EXACT_EFFECTS: usize = 32_768;

/// Read only after authenticated original R/S/T inspection. Refuse a truncated
/// or inconsistent view rather than silently treating missing effects as safe.
fn exact_provider_and_run_evidence(
    conn: &Connection,
    report: &ColdQuarantineInspection,
) -> Result<Vec<OriginalEffectIdentity>> {
    let queries = [
        (EvidenceClass::UnfinishedRun,
         "SELECT id,id FROM runs WHERE status='running' ORDER BY id LIMIT 32769",
         report.unfinished_runs),
        (EvidenceClass::ProviderReceipt,
         "SELECT run_id,CAST(ordinal AS TEXT) FROM invocation_receipts WHERE state IN ('started','unknown') ORDER BY run_id,ordinal LIMIT 32769",
         report.uncertain_provider_receipts),
        (EvidenceClass::PossiblyPaidAttempt,
         "SELECT run_id,id FROM llm_attempts WHERE dispatch_state IN ('unknown','provider_started') ORDER BY id LIMIT 32769",
         report.possibly_paid_attempts),
        (EvidenceClass::ExternalEffectHazard,
         "SELECT run_id,run_id FROM hazards WHERE state IN ('running','needs_inspection') ORDER BY run_id LIMIT 32769",
         report.unresolved_effect_hazards),
    ];
    let mut evidence = Vec::new();
    for (kind, sql, expected) in queries {
        let mut stmt = crate::db(conn.prepare(sql))?;
        let rows = crate::db(stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        }))?;
        let facts = crate::db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        if facts.len() != expected || facts.len() > MAX_EXACT_EFFECTS {
            return Err(Error::Denied(
                "cold reconciliation original fact count changed",
            ));
        }
        for (run_id, original_key) in facts {
            if run_id.is_empty() || original_key.is_empty() {
                return Err(Error::Denied("cold reconciliation lacks original identity"));
            }
            evidence.push(OriginalEffectIdentity {
                kind,
                run_id,
                original_key,
            });
        }
        if evidence.len() > MAX_EXACT_EFFECTS {
            return Err(Error::Limit(
                "cold reconciliation exact facts exceed budget",
            ));
        }
    }
    Ok(evidence)
}

/// Immutable per-session latest-fact identities, without the encrypted
/// Queue body or untrusted display text. These are obligations, not dispatches.
/// Original recorded custody is FACTUAL historical provenance, never a
/// portable grant to execute work on a restored or different Actor/site.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct OriginalQueueClaimProvenance {
    pub(crate) recipient_actor: String,
    pub(crate) claim_message_id: u64,
    pub(crate) turn_id: u64,
    pub(crate) writer_epoch: u64,
    pub(crate) owner_process_id: String,
    pub(crate) owner_store_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OriginalActorEvidenceSource {
    Queue {
        sender: String,
        recipient_site: String,
        enqueue_message_id: u64,
        claim: Option<OriginalQueueClaimProvenance>,
    },
    Input {
        sender: String,
        target: String,
        message_id: u64,
        turn_id: u64,
    },
    Control {
        sender: String,
        target: String,
        message_id: u64,
        turn_id: u64,
        input_request_id: String,
        owner_process_id: Option<String>,
        owner_store_generation: Option<u64>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct OriginalActorEffectIdentity {
    pub(crate) kind: EvidenceClass,
    pub(crate) session_id: String,
    pub(crate) branch_id: String,
    pub(crate) request_id: String,
    pub(crate) writer_epoch: u64,
    pub(crate) provenance: OriginalActorEvidenceSource,
}

fn exact_actor_evidence(
    conn: &Connection,
    report: &ColdQuarantineInspection,
) -> Result<Vec<OriginalActorEffectIdentity>> {
    let mut query = crate::db(conn.prepare("SELECT id FROM sessions ORDER BY id LIMIT 8193"))?;
    let rows = crate::db(query.query_map([], |row| row.get::<_, String>(0)))?;
    let sessions = crate::db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if sessions.len() != report.sessions || sessions.len() > 8192 {
        return Err(Error::Denied(
            "cold reconciliation Session inventory changed",
        ));
    }
    let mut result = Vec::new();
    for session in sessions {
        if session.is_empty() {
            return Err(Error::Denied(
                "cold reconciliation missing Session identity",
            ));
        }
        let mut queue = BTreeMap::new();
        for f in actor_queue::events(conn, &session)? {
            if f.request.session_id != session {
                return Err(Error::Denied("cold Queue fact Session mismatch"));
            }
            queue.insert(f.request.request_id.clone(), f);
        }
        for f in queue.into_values() {
            let kind = match f.state {
                QueueState::Pending => Some(EvidenceClass::QueuePending),
                QueueState::Claimed => Some(EvidenceClass::QueueClaimed),
                QueueState::Accepted => Some(EvidenceClass::QueueAccepted),
                _ => None,
            };
            if let Some(kind) = kind {
                // A claim is an irreversible original-process admission fact.
                // Require its literal owner Process incarnation and epoch;
                // never infer a missing claim from Queue State alone.
                let claim = match (kind, f.claim) {
                    (EvidenceClass::QueuePending, None) => None,
                    (EvidenceClass::QueueClaimed | EvidenceClass::QueueAccepted, Some(claim)) => {
                        let process_id = claim.owner_process_id.ok_or(Error::Denied(
                            "cold claimed Queue lacks original owner Process identity",
                        ))?;
                        let process_generation =
                            claim.owner_store_generation.ok_or(Error::Denied(
                                "cold claimed Queue lacks original owner Process generation",
                            ))?;
                        if claim.recipient_actor.is_empty()
                            || process_id.is_empty()
                            || process_generation == 0
                            || claim.message_id == 0
                            || claim.turn_id == 0
                            || claim.writer_epoch != f.request.writer_epoch
                        {
                            return Err(Error::Denied("cold Queue claim provenance inconsistent"));
                        }
                        Some(OriginalQueueClaimProvenance {
                            recipient_actor: claim.recipient_actor,
                            claim_message_id: claim.message_id,
                            turn_id: claim.turn_id,
                            writer_epoch: claim.writer_epoch,
                            owner_process_id: process_id,
                            owner_store_generation: process_generation,
                        })
                    }
                    _ => return Err(Error::Denied("cold Queue state and claim contradict")),
                };
                if f.request.sender.is_empty()
                    || f.request.recipient_site.is_empty()
                    || f.request.message_id == 0
                {
                    return Err(Error::Denied("cold queued original source/site missing"));
                }
                result.push(OriginalActorEffectIdentity {
                    kind,
                    session_id: session.clone(),
                    branch_id: f.request.branch_id,
                    request_id: f.request.request_id,
                    writer_epoch: f.request.writer_epoch,
                    provenance: OriginalActorEvidenceSource::Queue {
                        sender: f.request.sender,
                        recipient_site: f.request.recipient_site,
                        enqueue_message_id: f.request.message_id,
                        claim,
                    },
                });
            }
        }
        let mut inputs = BTreeMap::new();
        for f in actor_admission::events(conn, &session)? {
            if f.admission.session_id != session {
                return Err(Error::Denied("cold Input fact Session mismatch"));
            }
            inputs.insert(f.admission.request_id.clone(), f);
        }
        for f in inputs.into_values().filter(|f| f.state.needs_inspection()) {
            let admission = f.admission;
            if admission.sender.is_empty()
                || admission.target.is_empty()
                || admission.message_id == 0
                || admission.turn_id == 0
            {
                return Err(Error::Denied("cold Actor Input lacks original source/Turn"));
            }
            result.push(OriginalActorEffectIdentity {
                kind: EvidenceClass::ActorInput,
                session_id: session.clone(),
                branch_id: admission.branch_id,
                request_id: admission.request_id,
                writer_epoch: admission.writer_epoch,
                provenance: OriginalActorEvidenceSource::Input {
                    sender: admission.sender,
                    target: admission.target,
                    message_id: admission.message_id,
                    turn_id: admission.turn_id,
                },
            });
        }
        let mut controls = BTreeMap::new();
        for f in actor_control::events(conn, &session)? {
            if f.admission.session_id != session {
                return Err(Error::Denied("cold Control fact Session mismatch"));
            }
            controls.insert(f.admission.request_id.clone(), f);
        }
        for f in controls
            .into_values()
            .filter(|f| f.state.needs_inspection())
        {
            let admission = f.admission;
            if admission.sender.is_empty()
                || admission.target.is_empty()
                || admission.input_request_id.is_empty()
                || admission.message_id == 0
                || admission.turn_id == 0
                || f.owner_process_id.is_some() != f.owner_store_generation.is_some()
                || f.owner_process_id.as_deref() == Some("")
                || f.owner_store_generation == Some(0)
            {
                return Err(Error::Denied(
                    "cold Actor Control source/Process inconsistent",
                ));
            }
            result.push(OriginalActorEffectIdentity {
                kind: EvidenceClass::ActorControl,
                session_id: session.clone(),
                branch_id: admission.branch_id,
                request_id: admission.request_id,
                writer_epoch: admission.writer_epoch,
                provenance: OriginalActorEvidenceSource::Control {
                    sender: admission.sender,
                    target: admission.target,
                    message_id: admission.message_id,
                    turn_id: admission.turn_id,
                    input_request_id: admission.input_request_id,
                    owner_process_id: f.owner_process_id,
                    owner_store_generation: f.owner_store_generation,
                },
            });
        }
        if result.len() > MAX_EXACT_EFFECTS {
            return Err(Error::Limit("cold Actor evidence exceeds bounded review"));
        }
    }
    let expected = [
        (EvidenceClass::QueuePending, report.queued_pending),
        (EvidenceClass::QueueClaimed, report.queued_claimed),
        (EvidenceClass::QueueAccepted, report.queued_accepted),
        (
            EvidenceClass::ActorInput,
            report.actor_input_needs_inspection,
        ),
        (
            EvidenceClass::ActorControl,
            report.actor_control_needs_inspection,
        ),
    ];
    for (kind, count) in expected {
        if result.iter().filter(|r| r.kind == kind).count() != count {
            return Err(Error::Denied(
                "cold Actor fact counts differ from original inspection",
            ));
        }
    }
    if result
        .iter()
        .any(|r| r.request_id.is_empty() || r.branch_id.is_empty() || r.writer_epoch == 0)
    {
        return Err(Error::Denied(
            "cold Actor evidence has incomplete original authority identity",
        ));
    }
    Ok(result)
}

/// Original immutable Tool journal identity and its factual start/finish
/// multiplicities. An unmatched start stays UNKNOWN, not an executable retry.
/// Original SourceKind/ID/epoch and execution site are FACTS, never grants.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct OriginalToolEffectIdentity {
    pub(crate) kind: EvidenceClass,
    pub(crate) run_id: String,
    pub(crate) call_id: String,
    pub(crate) starts: usize,
    pub(crate) finishes: usize,
    pub(crate) original_source: Option<ToolSourceEvidence>,
    pub(crate) original_site: String,
}

#[derive(Default)]
struct OriginalToolObservation {
    starts: usize,
    finishes: usize,
    unresolved_receipt: bool,
    source: Option<ToolSourceEvidence>,
    site: String,
}

fn exact_tool_evidence(
    conn: &Connection,
    report: &ColdQuarantineInspection,
) -> Result<Vec<OriginalToolEffectIdentity>> {
    let mut query = crate::db(conn.prepare(
        "SELECT run_id,kind,payload FROM run_events WHERE kind IN ('tool_started','tool_finished') ORDER BY seq LIMIT 32769",
    ))?;
    let rows = crate::db(query.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    }))?;
    let facts = crate::db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if facts.len() > MAX_EXACT_EFFECTS {
        return Err(Error::Limit(
            "original Tool receipts exceed cold review budget",
        ));
    }
    let mut counts: BTreeMap<(String, String), OriginalToolObservation> = BTreeMap::new();
    for (run_id, kind, payload) in facts {
        let event: JournalEntry = crate::decode(&payload)?;
        let (call_id, site, started, source, unresolved_receipt) = match (kind.as_str(), event) {
            ("tool_started", JournalEntry::ToolStarted { record, source }) => {
                (record.call.id, record.execution_site, true, source, false)
            }
            ("tool_finished", JournalEntry::ToolFinished { record }) => {
                let unresolved =
                    crate::cold_quarantine_review::original_tool_receipt_needs_inspection(&record);
                (
                    record.call.id,
                    record.execution_site,
                    false,
                    None,
                    unresolved,
                )
            }
            _ => return Err(Error::Denied("original Tool receipt kind/payload mismatch")),
        };
        if run_id.is_empty() || call_id.is_empty() || site.is_empty() {
            return Err(Error::Denied(
                "original Tool receipt lacks exact source/site identity",
            ));
        }
        let observation = counts.entry((run_id, call_id)).or_default();
        if !observation.site.is_empty() && observation.site != site {
            return Err(Error::Denied("original Tool call execution site changed"));
        }
        observation.site = site;
        if started {
            if let Some(ref evidence) = source {
                evidence.validate()?;
            }
            if observation.starts > 0 && observation.source != source {
                return Err(Error::Denied(
                    "original Tool call source attribution changed",
                ));
            }
            observation.starts += 1;
            observation.source = source;
        } else {
            // A finished-without-start journal fact is itself a mismatch
            // requiring source review, NOT grounds to fabricate a start.
            observation.finishes += 1;
            observation.unresolved_receipt |= unresolved_receipt;
        }
    }
    let mut result = Vec::new();
    let mut incomplete = 0;
    let mut mismatched = 0;
    for ((run_id, call_id), observation) in counts {
        let starts = observation.starts;
        let finishes = observation.finishes;
        let (unresolved, mismatch) = crate::cold_quarantine_review::original_tool_obligations(
            starts,
            finishes,
            observation.unresolved_receipt,
        );
        incomplete += unresolved;
        mismatched += mismatch;
        let kind = if unresolved > 0 {
            Some(EvidenceClass::ToolIntent)
        } else if mismatch > 0 {
            Some(EvidenceClass::ToolReceiptMismatch)
        } else {
            None
        };
        if let Some(kind) = kind {
            result.push(OriginalToolEffectIdentity {
                kind,
                run_id,
                call_id,
                starts,
                finishes,
                original_source: observation.source,
                original_site: observation.site,
            });
        }
    }
    if incomplete != report.incomplete_tool_intents || mismatched != report.mismatched_tool_receipts
    {
        return Err(Error::Denied(
            "original Tool journal differs from quarantine inspection",
        ));
    }
    Ok(result)
}

/// W4 original coordination/recovery evidence. No copied Process incarnation,
/// retry schedule or writer epoch from cold State can be an executable grant.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct OriginalRecoveryIdentity {
    pub(crate) kind: EvidenceClass,
    pub(crate) original_key: String,
    pub(crate) related_id: String,
    pub(crate) original_generation: u64,
    pub(crate) owner_process_id: Option<String>,
}

/// Exact non-secret primary keys and custody provenance, rather than counts
/// that could silently hide two distinct unfinished effects.
fn exact_recovery_evidence(
    conn: &Connection,
    report: &ColdQuarantineInspection,
) -> Result<Vec<OriginalRecoveryIdentity>> {
    let queries = [
        (EvidenceClass::ExecutionLease,
         "SELECT scope_key,COALESCE(run_id,''),generation,holder_process_id FROM local_coordination_leases WHERE scope_kind='execution' AND state='hazard' ORDER BY scope_key LIMIT 32769",
         report.hazardous_execution_leases),
        (EvidenceClass::RecoveryHint,
         "SELECT id,operation_id,0,'' FROM recovery_hints WHERE state='open' ORDER BY id LIMIT 32769",
         report.open_recovery_hints),
        (EvidenceClass::RetrySchedule,
         "SELECT operation_id,COALESCE(claimed_by_client,''),generation,'' FROM retry_schedules WHERE state IN ('waiting_user','scheduled','claimed','blocked') ORDER BY operation_id LIMIT 32769",
         report.retry_schedules_requiring_owner),
    ];
    let mut evidence = Vec::new();
    for (kind, sql, expected) in queries {
        let mut stmt = crate::db(conn.prepare(sql))?;
        let rows = crate::db(stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
            ))
        }))?;
        let records = crate::db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        if records.len() != expected || records.len() > MAX_EXACT_EFFECTS {
            return Err(Error::Denied(
                "cold recovery evidence differs from original inspection",
            ));
        }
        for (original_key, related_id, generation, owner_process) in records {
            if original_key.is_empty()
                || generation < 0
                || (kind != EvidenceClass::RecoveryHint && generation == 0)
                || (kind == EvidenceClass::RecoveryHint && related_id.is_empty())
                || (kind == EvidenceClass::ExecutionLease && owner_process.is_empty())
            {
                return Err(Error::Denied(
                    "cold recovery evidence missing original provenance",
                ));
            }
            evidence.push(OriginalRecoveryIdentity {
                kind,
                original_key,
                related_id,
                original_generation: generation as u64,
                owner_process_id: if owner_process.is_empty() {
                    None
                } else {
                    Some(owner_process)
                },
            });
        }
        if evidence.len() > MAX_EXACT_EFFECTS {
            return Err(Error::Limit(
                "cold recovery evidence exceeds bounded review",
            ));
        }
    }
    Ok(evidence)
}

/// W7: owner-private descriptor anchors catch local path replacement and
/// file mutations during an inspection even when the replacement bytes are
/// identical. This is NOT an atomic multi-file snapshot or root-host defense.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ColdFileIdentity {
    device: u64,
    inode: u64,
    size: u64,
    uid: u32,
    gid: u32,
    mode: u32,
    modified_seconds: i64,
    modified_nanos: i64,
    changed_seconds: i64,
    changed_nanos: i64,
}

impl ColdFileIdentity {
    fn same_object_and_custody(&self, other: &Self) -> bool {
        self.device == other.device
            && self.inode == other.inode
            && self.uid == other.uid
            && self.gid == other.gid
            && self.mode == other.mode
    }

    fn of(meta: &Metadata) -> Self {
        Self {
            device: meta.dev(),
            inode: meta.ino(),
            size: meta.len(),
            uid: meta.uid(),
            gid: meta.gid(),
            mode: meta.mode(),
            modified_seconds: meta.mtime(),
            modified_nanos: meta.mtime_nsec(),
            changed_seconds: meta.ctime(),
            changed_nanos: meta.ctime_nsec(),
        }
    }
}

struct AnchoredColdFile {
    handle: File,
    original: ColdFileIdentity,
    is_directory: bool,
}

impl AnchoredColdFile {
    fn open_at(path: &Path, is_directory: bool) -> Result<Self> {
        let handle = if is_directory {
            File::open(path)?
        } else {
            aw_platform::open_private_existing(path, false)?
        };
        let meta = handle.metadata()?;
        if meta.is_dir() != is_directory || (!is_directory && !meta.is_file()) {
            return Err(Error::Denied(
                "cold snapshot anchor is not expected file type",
            ));
        }
        Ok(Self {
            handle,
            original: ColdFileIdentity::of(&meta),
            is_directory,
        })
    }

    fn verify_at(&self, path: &Path) -> Result<()> {
        // Check both the held original descriptor and a fresh no-follow
        // platform-validated file open; a rename cannot silently retarget us.
        let held = ColdFileIdentity::of(&self.handle.metadata()?);
        let reopened = Self::open_at(path, self.is_directory)?;
        if held != self.original || reopened.original != self.original {
            return Err(Error::Denied(
                "cold snapshot original filesystem identity changed",
            ));
        }
        Ok(())
    }

    // The original review transaction legitimately changes SQLite bytes and
    // directory timestamps. Only that write boundary permits mutable metadata;
    // it must still reference the original private object, never a replacement.
    fn verify_after_local_write(&self, path: &Path) -> Result<()> {
        let held = ColdFileIdentity::of(&self.handle.metadata()?);
        let reopened = Self::open_at(path, self.is_directory)?;
        if !self.original.same_object_and_custody(&held)
            || !self.original.same_object_and_custody(&reopened.original)
        {
            return Err(Error::Denied(
                "cold review original filesystem object or custody changed",
            ));
        }
        Ok(())
    }
}

pub(crate) const COLD_AUDIT_OUTCOME_UNKNOWN: &str =
    "cold non-executing audit outcome unknown after commit; inspect original immutable receipt; never automatically retry or execute";

/// Exact immutable original v33 review row, borrowed only for acknowledgment.
/// Sharing this predicate adds no persisted type, authority or review service.
pub(crate) struct ColdReviewAuditRow<'a> {
    pub(crate) seq: i64,
    pub(crate) event_id: &'a str,
    pub(crate) store_id: &'a str,
    pub(crate) generation: u64,
    pub(crate) kind: &'a str,
    pub(crate) scope_key: &'a str,
    pub(crate) payload_json: &'a str,
    pub(crate) created_at: i64,
}

impl ColdReviewAuditRow<'_> {
    pub(crate) fn verify(&self, tx: &Connection) -> Result<()> {
        let exact: bool = crate::db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM local_coordination_events
                      WHERE seq=?1 AND event_id=?2 AND store_id=?3 AND store_generation=?4
                        AND process_id IS NULL AND kind=?5 AND scope_kind='cold_restore'
                        AND scope_key=?6 AND lease_generation IS NULL
                        AND payload_json=?7 AND created_at=?8)",
            rusqlite::params![
                self.seq,
                self.event_id,
                self.store_id,
                self.generation,
                self.kind,
                self.scope_key,
                self.payload_json,
                self.created_at
            ],
            |row| row.get(0),
        ))?;
        if !exact {
            return Err(Error::Denied(
                "cold owner audit exact committed receipt needs inspection",
            ));
        }
        Ok(())
    }

    /// Acknowledgment only, after the caller's COMMIT attempt and connection
    /// close. This never grants authority or retries a durable operation.
    pub(crate) fn acknowledge<T, F, H>(
        &self,
        archive: &Path,
        state: &Path,
        anchors: &ColdFilesystemAnchors,
        inspect: F,
        after_commit: H,
    ) -> Result<T>
    where
        F: FnOnce(
            &Connection,
            ColdEffectSnapshot,
            ColdQuarantineInspection,
            &str,
            u64,
        ) -> Result<T>,
        H: FnOnce(&Path) -> Result<()>,
    {
        let result = (|| {
            after_commit(state)?;
            anchors.verify_after_review_commit(state)?;
            let sqlite = state.join("sessions.sqlite3");
            aw_platform::open_private_existing(&sqlite, false)?.sync_all()?;
            aw_platform::sync_parent(&sqlite)?;
            with_verified_cold_snapshot(
                archive,
                state,
                |c, snapshot, report, id, generation| {
                    if id != self.store_id || generation != self.generation {
                        return Err(Error::Denied(
                            "cold audit original Store or generation changed",
                        ));
                    }
                    self.verify(c)?;
                    inspect(c, snapshot, report, id, generation)
                },
                || anchors.verify_after_review_commit(state),
            )
        })();
        result.map_err(|_| Error::Storage(COLD_AUDIT_OUTCOME_UNKNOWN))
    }
}

pub(crate) struct ColdFilesystemAnchors {
    root: AnchoredColdFile,
    sqlite: AnchoredColdFile,
    quarantine: AnchoredColdFile,
}

impl ColdFilesystemAnchors {
    pub(crate) fn capture(root: &Path) -> Result<Self> {
        aw_platform::validate_private_dir(root)?;
        Ok(Self {
            root: AnchoredColdFile::open_at(root, true)?,
            sqlite: AnchoredColdFile::open_at(&root.join("sessions.sqlite3"), false)?,
            quarantine: AnchoredColdFile::open_at(
                &root.join(crate::composite_staging::COLD_QUARANTINE_MARKER),
                false,
            )?,
        })
    }

    pub(crate) fn verify(&self, root: &Path) -> Result<()> {
        aw_platform::validate_private_dir(root)?;
        self.root.verify_at(root)?;
        self.sqlite.verify_at(&root.join("sessions.sqlite3"))?;
        self.quarantine
            .verify_at(&root.join(crate::composite_staging::COLD_QUARANTINE_MARKER))?;
        Ok(())
    }

    pub(crate) fn verify_after_review_commit(&self, root: &Path) -> Result<()> {
        aw_platform::validate_private_dir(root)?;
        self.root.verify_after_local_write(root)?;
        self.sqlite
            .verify_after_local_write(&root.join("sessions.sqlite3"))?;
        // The review never writes the independent filesystem denial marker.
        self.quarantine
            .verify_at(&root.join(crate::composite_staging::COLD_QUARANTINE_MARKER))?;
        Ok(())
    }

    pub(crate) fn verify_source_before_review(
        &self,
        archive: &Path,
        state: &Path,
        expected_id: &str,
        expected_generation: u64,
    ) -> Result<()> {
        self.verify(state)?;
        let (id, generation) =
            crate::composite_staging::verify_cold_for_inspection(archive, state)?;
        if id != expected_id || generation != expected_generation {
            return Err(Error::Denied(
                "cold audit source Store or generation changed",
            ));
        }
        self.verify(state)
    }
}

/// W5 one SQLite read snapshot spanning all original W2–W4 identities.
/// This is diagnostic only: it does NOT grant authority, settle effects, or
/// claim that a copied filesystem marker is a globally unique authorization.
#[derive(Debug, serde::Serialize)]
pub(crate) struct ColdEffectSnapshot {
    pub(crate) preparation: ColdEffectPreparation,
    pub(crate) provider_and_runs: Vec<OriginalEffectIdentity>,
    pub(crate) actor: Vec<OriginalActorEffectIdentity>,
    pub(crate) tools: Vec<OriginalToolEffectIdentity>,
    pub(crate) recovery: Vec<OriginalRecoveryIdentity>,
}

#[cfg(test)]
fn collect_single_snapshot(
    conn: &mut Connection,
    expected_store_id: &str,
    expected_generation: u64,
) -> Result<ColdEffectSnapshot> {
    collect_single_snapshot_impl(conn, expected_store_id, expected_generation, || Ok(()))
}

#[cfg(test)]
fn collect_single_snapshot_with_hook<F: FnOnce() -> Result<()>>(
    conn: &mut Connection,
    expected_store_id: &str,
    expected_generation: u64,
    after_risk_counts: F,
) -> Result<ColdEffectSnapshot> {
    collect_single_snapshot_impl(
        conn,
        expected_store_id,
        expected_generation,
        after_risk_counts,
    )
}

#[cfg(test)]
fn collect_single_snapshot_impl<F: FnOnce() -> Result<()>>(
    conn: &mut Connection,
    expected_store_id: &str,
    expected_generation: u64,
    after_risk_counts: F,
) -> Result<ColdEffectSnapshot> {
    // A deferred transaction pins a single WAL read end-mark at the FIRST read.
    // Snapshot consistency is per SQLite DB; external files are separately
    // verified before and after, not atomically versioned with this DB.
    let tx = crate::db(conn.transaction_with_behavior(rusqlite::TransactionBehavior::Deferred))?;
    let (result, _) = original_facts_on_connection(
        &tx,
        expected_store_id,
        expected_generation,
        after_risk_counts,
    )?;
    // Ending the read transaction releases the pinned snapshot, not authority.
    crate::db(tx.commit())?;
    Ok(result)
}

/// Shared private inspection predicate: the W7 read-only path and W8's
/// ORIGINAL BEGIN IMMEDIATE review audit consult exactly the SAME v33 facts.
/// The caller owns transaction choice; this function performs ZERO writes.
pub(crate) fn original_facts_on_connection<F: FnOnce() -> Result<()>>(
    conn: &Connection,
    expected_store_id: &str,
    expected_generation: u64,
    after_risk_counts: F,
) -> Result<(ColdEffectSnapshot, ColdQuarantineInspection)> {
    // The original Store, source generation and BOTH SQL/FS quarantine
    // identities must agree with the already verified published archive.
    // Read them on THIS transaction's snapshot, not on a pre-snapshot handle.
    let (store_id, generation, sql_quarantine): (String, String, String) =
        crate::db(conn.query_row(
            "SELECT (SELECT value FROM store_meta WHERE key='store_id'),
                    (SELECT value FROM store_meta WHERE key='restore_generation'),
                    (SELECT value FROM store_meta WHERE key='cold_restore_quarantine')",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ))?;
    let source_generation = expected_generation.checked_sub(1).ok_or(Error::Denied(
        "cold snapshot cannot accept zero restoration generation",
    ))?;
    if store_id != expected_store_id
        || generation != expected_generation.to_string()
        || sql_quarantine
            != crate::coordination::cold_quarantine_identity(
                expected_store_id,
                source_generation,
                expected_generation,
            )
    {
        return Err(Error::Denied(
            "cold snapshot original Store/quarantine authority changed",
        ));
    }
    // T's full risk counts and W2–W4 exact identities share the SAME pinned
    // SQLite read snapshot; there is no stale earlier count report to trust.
    let report = crate::cold_quarantine_review::inspect_original_cold_risks_on_connection(
        conn,
        expected_generation,
    )?;
    after_risk_counts()?;
    let provider_and_runs = exact_provider_and_run_evidence(conn, &report)?;
    let actor = exact_actor_evidence(conn, &report)?;
    let tools = exact_tool_evidence(conn, &report)?;
    let recovery = exact_recovery_evidence(conn, &report)?;
    Ok((
        ColdEffectSnapshot {
            preparation: classify(&report),
            provider_and_runs,
            actor,
            tools,
            recovery,
        },
        report,
    ))
}

/// One private multi-category SQLite snapshot with original quarantine
/// verification both BEFORE and AFTER. Neither check makes this an
/// activating or externally immutable attestation. No external effects.
pub(crate) fn prepare_consistent_cold_effect_snapshot(
    original_archive: &Path,
    restored_state: &Path,
) -> Result<ColdEffectSnapshot> {
    prepare_consistent_cold_effect_snapshot_impl(original_archive, restored_state, || Ok(()))
}

/// Test-only corruption/race seam; not compiled into release builds.
#[cfg(test)]
pub(crate) fn prepare_consistent_cold_effect_snapshot_with_hook<F: FnOnce() -> Result<()>>(
    original_archive: &Path,
    restored_state: &Path,
    after_snapshot: F,
) -> Result<ColdEffectSnapshot> {
    prepare_consistent_cold_effect_snapshot_impl(original_archive, restored_state, after_snapshot)
}

fn prepare_consistent_cold_effect_snapshot_impl<F: FnOnce() -> Result<()>>(
    original_archive: &Path,
    restored_state: &Path,
    after_snapshot: F,
) -> Result<ColdEffectSnapshot> {
    with_verified_cold_snapshot(
        original_archive,
        restored_state,
        |_, snapshot, _, _, _| Ok(snapshot),
        after_snapshot,
    )
}

/// ONE source/SQLite/filesystem inspection seam for original T, W7 review
/// and the W8 owner's preflight. Visitor receives exact identities and the
/// original T report from the SAME pinned SQLite snapshot, without rescanning.
/// Both original cold quarantine guards and original ciphertext custody are
/// independently re-verified before returning a result; no activation.
pub(crate) fn with_verified_cold_snapshot<T, F, H>(
    original_archive: &Path,
    restored_state: &Path,
    inspect: F,
    after_snapshot: H,
) -> Result<T>
where
    F: FnOnce(&Connection, ColdEffectSnapshot, ColdQuarantineInspection, &str, u64) -> Result<T>,
    H: FnOnce() -> Result<()>,
{
    let (store_id, generation) =
        crate::composite_staging::verify_cold_for_inspection(original_archive, restored_state)?;
    let anchors = ColdFilesystemAnchors::capture(restored_state)?;
    let sqlite = restored_state.join("sessions.sqlite3");
    let mut conn = crate::db(Connection::open_with_flags(
        &sqlite,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ))?;
    let tx = crate::db(conn.transaction_with_behavior(rusqlite::TransactionBehavior::Deferred))?;
    let (snapshot, report) = original_facts_on_connection(&tx, &store_id, generation, || Ok(()))?;
    let result = inspect(&tx, snapshot, report, &store_id, generation)?;
    crate::db(tx.commit())?;
    drop(conn);
    after_snapshot()?;
    anchors.verify(restored_state)?;
    let (verified_store_id, verified_generation) =
        crate::composite_staging::verify_cold_for_inspection(original_archive, restored_state)?;
    if verified_store_id != store_id || verified_generation != generation {
        return Err(Error::Denied(
            "cold snapshot post-verification Store identity changed",
        ));
    }
    anchors.verify(restored_state)?;
    Ok(result)
}

// Original-owner reviewed dispositions are a separate derived service over
// W7; never a new journal, runtime, execution engine or activation surface.
#[path = "cold_effect_reconciliation_review.rs"]
pub(crate) mod review;

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> ColdQuarantineInspection {
        ColdQuarantineInspection {
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
        }
    }

    #[test]
    fn w6_wal_competing_writer_cannot_split_risk_counts_from_original_keys() {
        // Actual two-connection WAL contention: the writer commits after the
        // inspector has pinned its risk-count snapshot, before identity reads.
        let path = std::env::temp_dir().join(format!(
            "aw-w6-wal-{}-{}.sqlite3",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut inspector = Connection::open(&path).unwrap();
        inspector.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE store_meta(key TEXT,value TEXT); INSERT INTO store_meta VALUES('restore_generation','2'); CREATE TABLE sessions(id TEXT); INSERT INTO sessions VALUES('session-a'); CREATE TABLE audit(session_id TEXT,action TEXT,data_json TEXT,seq INTEGER); CREATE TABLE runs(id TEXT,status TEXT); CREATE TABLE invocation_receipts(run_id TEXT,ordinal INTEGER,state TEXT); CREATE TABLE llm_attempts(id TEXT,run_id TEXT,dispatch_state TEXT); CREATE TABLE hazards(run_id TEXT,state TEXT); CREATE TABLE run_events(run_id TEXT,kind TEXT,payload TEXT,seq INTEGER); CREATE TABLE local_coordination_leases(scope_key TEXT,run_id TEXT,generation INTEGER,holder_process_id TEXT,scope_kind TEXT,state TEXT); CREATE TABLE recovery_hints(id TEXT,operation_id TEXT,state TEXT); CREATE TABLE retry_schedules(operation_id TEXT,claimed_by_client TEXT,generation INTEGER,state TEXT);").unwrap();
        let sql_marker = crate::coordination::cold_quarantine_identity("w7-fixture", 1, 2);
        inspector
            .execute("INSERT INTO store_meta VALUES('store_id','w7-fixture')", [])
            .unwrap();
        inspector
            .execute(
                "INSERT INTO store_meta VALUES('cold_restore_quarantine',?1)",
                [&sql_marker],
            )
            .unwrap();
        let writer = Connection::open(&path).unwrap();
        writer
            .busy_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let snapshot = collect_single_snapshot_with_hook(&mut inspector, "w7-fixture", 2, || {
            writer
                .execute("INSERT INTO runs VALUES('new-run','running')", [])
                .unwrap();
            Ok(())
        })
        .unwrap();
        assert!(snapshot.provider_and_runs.is_empty());
        assert!(snapshot.preparation.obligations.is_empty());
        let later = collect_single_snapshot(&mut inspector, "w7-fixture", 2).unwrap();
        assert_eq!(later.provider_and_runs.len(), 1);
        assert_eq!(later.preparation.obligations.len(), 1);
        assert!(!later.preparation.runnable);
        drop(writer);
        drop(inspector);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}.wal", path.display()));
        let _ = std::fs::remove_file(format!("{}.shm", path.display()));
    }

    #[test]
    fn w6_snapshot_is_fail_closed_on_generation_and_uses_same_source_counts() {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE store_meta(key TEXT,value TEXT); INSERT INTO store_meta VALUES('restore_generation','2'); CREATE TABLE sessions(id TEXT); INSERT INTO sessions VALUES('session-a'); CREATE TABLE audit(session_id TEXT,action TEXT,data_json TEXT,seq INTEGER); CREATE TABLE runs(id TEXT,status TEXT); CREATE TABLE invocation_receipts(run_id TEXT,ordinal INTEGER,state TEXT); CREATE TABLE llm_attempts(id TEXT,run_id TEXT,dispatch_state TEXT); CREATE TABLE hazards(run_id TEXT,state TEXT); CREATE TABLE run_events(run_id TEXT,kind TEXT,payload TEXT,seq INTEGER); CREATE TABLE local_coordination_leases(scope_key TEXT,run_id TEXT,generation INTEGER,holder_process_id TEXT,scope_kind TEXT,state TEXT); CREATE TABLE recovery_hints(id TEXT,operation_id TEXT,state TEXT); CREATE TABLE retry_schedules(operation_id TEXT,claimed_by_client TEXT,generation INTEGER,state TEXT);").unwrap();
        let sql_marker = crate::coordination::cold_quarantine_identity("w7-fixture", 1, 2);
        c.execute("INSERT INTO store_meta VALUES('store_id','w7-fixture')", [])
            .unwrap();
        c.execute(
            "INSERT INTO store_meta VALUES('cold_restore_quarantine',?1)",
            [&sql_marker],
        )
        .unwrap();
        let snapshot = collect_single_snapshot(&mut c, "w7-fixture", 2).unwrap();
        assert!(snapshot.provider_and_runs.is_empty() && snapshot.actor.is_empty());
        assert!(!snapshot.preparation.runnable);
        // W6: an original new Run is now counted and enumerated INSIDE the
        // same transaction, rather than rejected based on stale T counts.
        c.execute("INSERT INTO runs VALUES('run-1','running')", [])
            .unwrap();
        let snapshot = collect_single_snapshot(&mut c, "w7-fixture", 2).unwrap();
        assert_eq!(snapshot.provider_and_runs.len(), 1);
        assert_eq!(snapshot.preparation.obligations[0].count, 1);
        assert!(!snapshot.preparation.runnable);
        // Same restored generation and identical effect facts still cannot
        // authorize a snapshot whose immutable original SQL denial vanished.
        c.execute(
            "UPDATE store_meta SET value='spoofed' WHERE key='cold_restore_quarantine'",
            [],
        )
        .unwrap();
        assert!(collect_single_snapshot(&mut c, "w7-fixture", 2).is_err());
        c.execute(
            "UPDATE store_meta SET value=?1 WHERE key='cold_restore_quarantine'",
            [&sql_marker],
        )
        .unwrap();
        c.execute(
            "UPDATE store_meta SET value='foreign' WHERE key='store_id'",
            [],
        )
        .unwrap();
        assert!(collect_single_snapshot(&mut c, "w7-fixture", 2).is_err());
        c.execute(
            "UPDATE store_meta SET value='w7-fixture' WHERE key='store_id'",
            [],
        )
        .unwrap();
        c.execute(
            "UPDATE store_meta SET value='3' WHERE key='restore_generation'",
            [],
        )
        .unwrap();
        assert!(collect_single_snapshot(&mut c, "w7-fixture", 2).is_err());
    }

    #[test]
    fn w4_recovery_exact_keys_and_source_incarnation() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE local_coordination_leases(scope_key TEXT,run_id TEXT,generation INTEGER,holder_process_id TEXT,scope_kind TEXT,state TEXT); CREATE TABLE recovery_hints(id TEXT,operation_id TEXT,state TEXT); CREATE TABLE retry_schedules(operation_id TEXT,claimed_by_client TEXT,generation INTEGER,state TEXT); INSERT INTO local_coordination_leases VALUES('scope-a','run-a',3,'process-a','execution','hazard'); INSERT INTO recovery_hints VALUES('hint-a','operation-a','open'); INSERT INTO retry_schedules VALUES('operation-a','client-a',5,'claimed');").unwrap();
        let mut report = empty();
        report.hazardous_execution_leases = 1;
        report.open_recovery_hints = 1;
        report.retry_schedules_requiring_owner = 1;
        let evidence = exact_recovery_evidence(&c, &report).unwrap();
        assert_eq!(evidence.len(), 3);
        assert_eq!(evidence[0].owner_process_id.as_deref(), Some("process-a"));
        assert_eq!(evidence[2].original_generation, 5);
        report.retry_schedules_requiring_owner = 0;
        assert!(exact_recovery_evidence(&c, &report).is_err());
    }

    #[test]
    fn w4_recovery_denies_missing_process_or_invalid_generation() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE local_coordination_leases(scope_key TEXT,run_id TEXT,generation INTEGER,holder_process_id TEXT,scope_kind TEXT,state TEXT); CREATE TABLE recovery_hints(id TEXT,operation_id TEXT,state TEXT); CREATE TABLE retry_schedules(operation_id TEXT,claimed_by_client TEXT,generation INTEGER,state TEXT); INSERT INTO local_coordination_leases VALUES('scope-a','run-a',1,'','execution','hazard');").unwrap();
        let mut report = empty();
        report.hazardous_execution_leases = 1;
        assert!(exact_recovery_evidence(&c, &report).is_err());
        c.execute(
            "UPDATE local_coordination_leases SET holder_process_id='process-a',generation=0",
            [],
        )
        .unwrap();
        assert!(exact_recovery_evidence(&c, &report).is_err());
    }

    #[test]
    fn aw_p6_w8g_exact_tool_evidence_preserves_original_receipt_counts() {
        original_tool_evidence_cases(&["missing", "empty", "orphan", "duplicate"]);
    }

    #[test]
    fn aw_p6_w8h_exact_tool_evidence_preserves_contradictory_receipts() {
        original_tool_evidence_cases(&["contradictory"]);
    }

    fn original_tool_evidence_cases(cases: &[&str]) {
        use aw_agent_core::{ToolCall, ToolEffect, ToolOutput, ToolRecord, ToolStatus};
        for status in [ToolStatus::Succeeded, ToolStatus::Failed] {
            for &case in cases {
                let c = Connection::open_in_memory().unwrap();
                c.execute_batch(
                    "CREATE TABLE run_events(run_id TEXT,kind TEXT,payload TEXT,seq INTEGER);",
                )
                .unwrap();
                let started = ToolRecord {
                    call: ToolCall {
                        id: "original-call".into(),
                        name: "bash".into(),
                        arguments: serde_json::json!({"command":"synthetic; never executed"}),
                    },
                    effect: ToolEffect::Process,
                    execution_site: "original-site".into(),
                    status: ToolStatus::Running,
                    output: None,
                };
                let mut finished = started.clone();
                finished.status = status.clone();
                if matches!(case, "empty" | "contradictory") {
                    let is_error = if case == "contradictory" {
                        status == ToolStatus::Succeeded
                    } else {
                        status == ToolStatus::Failed
                    };
                    finished.output = Some(if is_error {
                        ToolOutput::error("")
                    } else {
                        ToolOutput::text("")
                    });
                }
                let mut events = Vec::new();
                if case != "orphan" {
                    events.push((
                        "tool_started",
                        JournalEntry::ToolStarted {
                            record: started,
                            source: Some(ToolSourceEvidence::trusted_host()),
                        },
                    ));
                }
                events.push((
                    "tool_finished",
                    JournalEntry::ToolFinished {
                        record: finished.clone(),
                    },
                ));
                if case == "duplicate" {
                    events.push((
                        "tool_finished",
                        JournalEntry::ToolFinished { record: finished },
                    ));
                }
                for (seq, (kind, event)) in events.into_iter().enumerate() {
                    c.execute(
                        "INSERT INTO run_events VALUES(?1,?2,?3,?4)",
                        rusqlite::params![
                            "original-run",
                            kind,
                            serde_json::to_string(&event).unwrap(),
                            seq as i64
                        ],
                    )
                    .unwrap();
                }
                let mut report = empty();
                if matches!(case, "missing" | "contradictory") {
                    report.incomplete_tool_intents = 1;
                }
                if matches!(case, "orphan" | "duplicate") {
                    report.mismatched_tool_receipts = 1;
                }
                let facts = exact_tool_evidence(&c, &report).unwrap();
                if case == "empty" {
                    assert!(facts.is_empty(), "empty present output remains a receipt");
                } else {
                    assert_eq!(facts.len(), 1);
                    let fact = &facts[0];
                    assert_eq!(
                        fact.kind,
                        if matches!(case, "missing" | "contradictory") {
                            EvidenceClass::ToolIntent
                        } else {
                            EvidenceClass::ToolReceiptMismatch
                        }
                    );
                    assert_eq!(fact.run_id, "original-run");
                    assert_eq!(fact.call_id, "original-call");
                    assert_eq!(fact.original_site, "original-site");
                    assert_eq!(fact.starts, usize::from(case != "orphan"));
                    assert_eq!(fact.finishes, if case == "duplicate" { 2 } else { 1 });
                    assert_eq!(
                        fact.original_source,
                        if case == "orphan" {
                            None
                        } else {
                            Some(ToolSourceEvidence::trusted_host())
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn tool_journal_missing_original_start_is_not_reconciled() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE run_events(run_id TEXT,kind TEXT,payload TEXT,seq INTEGER);")
            .unwrap();
        let mut report = empty();
        report.incomplete_tool_intents = 1;
        assert!(exact_tool_evidence(&c, &report).is_err());
        report.incomplete_tool_intents = 0;
        assert!(exact_tool_evidence(&c, &report).unwrap().is_empty());
    }

    #[test]
    fn tool_journal_rejects_unknown_payload() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE run_events(run_id TEXT,kind TEXT,payload TEXT,seq INTEGER); INSERT INTO run_events VALUES('run-a','tool_started','{}',1);").unwrap();
        assert!(exact_tool_evidence(&c, &empty()).is_err());
    }

    #[test]
    fn w7_unowned_or_rebound_claimed_queue_cannot_lose_process_provenance() {
        use crate::actor_queue::{ActorQueueFact, ActorQueueRequest, QueueClaim, QueueTarget};
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE sessions(id TEXT); CREATE TABLE audit(session_id TEXT,action TEXT,data_json TEXT,seq INTEGER); INSERT INTO sessions VALUES('session-a');").unwrap();
        let mut fact = ActorQueueFact {
            request: ActorQueueRequest {
                request_id: "request-a".into(),
                session_id: "session-a".into(),
                branch_id: "branch-a".into(),
                recipient_site: "site-a".into(),
                sender: "sender-a".into(),
                message_id: 1,
                writer_epoch: 7,
                target: QueueTarget::Auto,
                priority: 0,
            },
            state: QueueState::Claimed,
            revision: 2,
            payload_sha256: "hash".into(),
            payload_ref: "encrypted".into(),
            note: None,
            claim: Some(QueueClaim {
                recipient_actor: "actor-a".into(),
                message_id: 2,
                turn_id: 3,
                writer_epoch: 7,
                owner_process_id: None,
                owner_store_generation: Some(4),
            }),
        };
        let insert = |fact: &ActorQueueFact| {
            c.execute("DELETE FROM audit", []).unwrap();
            c.execute(
                "INSERT INTO audit VALUES(?1,?2,?3,?4)",
                rusqlite::params![
                    "session-a",
                    "actor.queue",
                    serde_json::to_string(fact).unwrap(),
                    1
                ],
            )
            .unwrap();
        };
        let mut report = empty();
        report.queued_claimed = 1;
        insert(&fact);
        assert!(exact_actor_evidence(&c, &report).is_err());
        fact.claim.as_mut().unwrap().owner_process_id = Some("old-process-a".into());
        fact.claim.as_mut().unwrap().writer_epoch = 8;
        insert(&fact);
        assert!(exact_actor_evidence(&c, &report).is_err());
        fact.claim.as_mut().unwrap().writer_epoch = 7;
        insert(&fact);
        let result = exact_actor_evidence(&c, &report).unwrap();
        let OriginalActorEvidenceSource::Queue {
            claim: Some(origin),
            ..
        } = &result[0].provenance
        else {
            panic!("original queued Process evidence lost")
        };
        assert_eq!(origin.owner_process_id, "old-process-a");
        assert_eq!(origin.owner_store_generation, 4);
        assert_eq!(origin.writer_epoch, 7);
    }

    #[test]
    fn actor_identity_inventory_rejects_count_mismatch_and_missing_session() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE sessions(id TEXT); CREATE TABLE audit(session_id TEXT,action TEXT,data_json TEXT,seq INTEGER); INSERT INTO sessions VALUES('session-a');").unwrap();
        let mut report = empty();
        assert!(exact_actor_evidence(&c, &report).unwrap().is_empty());
        report.queued_claimed = 1;
        assert!(exact_actor_evidence(&c, &report).is_err());
        report.queued_claimed = 0;
        report.sessions = 2;
        assert!(exact_actor_evidence(&c, &report).is_err());
    }

    #[test]
    fn actor_identity_inventory_rejects_cross_session_fact() {
        use crate::actor_queue::{ActorQueueFact, ActorQueueRequest, QueueTarget};
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE sessions(id TEXT); CREATE TABLE audit(session_id TEXT,action TEXT,data_json TEXT,seq INTEGER); INSERT INTO sessions VALUES('session-a');").unwrap();
        let fact = ActorQueueFact {
            request: ActorQueueRequest {
                request_id: "queue-a".into(),
                session_id: "session-b".into(),
                branch_id: "branch-a".into(),
                recipient_site: "site-a".into(),
                sender: "sender-a".into(),
                message_id: 1,
                writer_epoch: 1,
                target: QueueTarget::Auto,
                priority: 0,
            },
            state: QueueState::Claimed,
            revision: 1,
            payload_sha256: "hash".into(),
            payload_ref: "opaque".into(),
            claim: None,
            note: None,
        };
        c.execute(
            "INSERT INTO audit VALUES(?1,?2,?3,?4)",
            rusqlite::params![
                "session-a",
                "actor.queue",
                serde_json::to_string(&fact).unwrap(),
                1
            ],
        )
        .unwrap();
        let mut report = empty();
        report.queued_claimed = 1;
        assert!(exact_actor_evidence(&c, &report).is_err());
    }

    #[test]
    fn exact_effects_reject_missing_or_ambiguous_original_facts() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE runs(id TEXT,status TEXT); CREATE TABLE invocation_receipts(run_id TEXT,ordinal INTEGER,state TEXT); CREATE TABLE llm_attempts(id TEXT,run_id TEXT,dispatch_state TEXT); CREATE TABLE hazards(run_id TEXT,state TEXT); INSERT INTO runs VALUES('run-a','running'); INSERT INTO invocation_receipts VALUES('run-a',2,'unknown'); INSERT INTO llm_attempts VALUES('attempt-a','run-a','provider_started'); INSERT INTO hazards VALUES('run-a','needs_inspection');").unwrap();
        let mut report = empty();
        report.unfinished_runs = 1;
        report.uncertain_provider_receipts = 1;
        report.possibly_paid_attempts = 1;
        report.unresolved_effect_hazards = 1;
        let facts = exact_provider_and_run_evidence(&c, &report).unwrap();
        assert_eq!(facts.len(), 4);
        assert_eq!(facts[1].original_key, "2");
        assert_eq!(facts[2].original_key, "attempt-a");
        assert_eq!(facts[3].run_id, "run-a");
        report.possibly_paid_attempts = 0;
        assert!(exact_provider_and_run_evidence(&c, &report).is_err());
    }

    #[test]
    fn exact_effects_reject_empty_source_identity() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE runs(id TEXT,status TEXT); CREATE TABLE invocation_receipts(run_id TEXT,ordinal INTEGER,state TEXT); CREATE TABLE llm_attempts(id TEXT,run_id TEXT,dispatch_state TEXT); CREATE TABLE hazards(run_id TEXT,state TEXT); INSERT INTO runs VALUES('','running');").unwrap();
        let mut report = empty();
        report.unfinished_runs = 1;
        assert!(exact_provider_and_run_evidence(&c, &report).is_err());
    }

    #[test]
    fn empty_restore_never_grants_execution_or_replay() {
        let p = classify(&empty());
        assert!(p.obligations.is_empty());
        assert!(!p.runnable && !p.permits_original_effect_replay);
        assert!(p.requires_independent_human_origin && p.requires_cross_clone_exclusion);
    }

    #[test]
    fn every_uncertain_effect_is_preserved_as_distinct_inspection_obligation() {
        let mut r = empty();
        r.uncertain_provider_receipts = 2;
        r.possibly_paid_attempts = 1;
        r.incomplete_tool_intents = 3;
        r.queued_pending = 1;
        r.queued_claimed = 1;
        r.queued_accepted = 1;
        r.actor_input_needs_inspection = 1;
        r.retry_schedules_requiring_owner = 1;
        let p = classify(&r);
        assert_eq!(p.obligations.len(), 8);
        assert!(p.obligations.iter().all(|o| o.source_evidence_required));
        assert_eq!(p.obligations.iter().map(|o| o.count).sum::<usize>(), 11);
        assert!(!p.runnable && !p.permits_original_effect_replay);
    }
}
