//! AW-P6-10U: a strictly INTERNAL signed human-device authorization PREREQUISITE
//! for future operator activation. NO runnable activation, marker removal,
//! WriterEpoch grant, Process registration, Queue/paid/Tool replay or public
//! API. The separate future runnable transition is deliberately UNIMPLEMENTED.
#![cfg(target_os = "linux")]
#![allow(dead_code)] // intentionally unreachable from shipping Host/CLI/RPC/Tool

use crate::cold_effect_reconciliation::{
    self as cold, ColdFilesystemAnchors, ColdReviewAuditRow, COLD_AUDIT_OUTCOME_UNKNOWN,
};
use crate::{cold_quarantine_review, db, identity_continuity, now};
use aw_agent_core::{Error, PrincipalId, Result};
use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

const AUTH_CHALLENGE_KIND: &str = "store.cold_activation_device_challenge_issued";
const AUTH_CONSENT_KIND: &str = "store.cold_activation_device_consent_verified";
const OWNER_REVIEW_KIND: &str = "store.cold_quarantine_review_requested";
const MAX_DEVICE_CHALLENGES: i64 = 32;
const CHALLENGE_TTL_MICROS: i64 = 5 * 60 * 1_000_000;
const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsentTranscript {
    domain: String,
    store_id: String,
    restore_generation: u64,
    request_id: String,
    owner_principal: String,
    domain_id: String,
    nonce_hex: String,
    archive_manifest_sha256: String,
    inspection_sha256: String,
    expires_at_micros: i64,
    exact_decision: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ColdDeviceChallenge {
    /// Exact domain-separated UTF-8 bytes the already-trusted external device
    /// must sign with its original ED25519 PRIVATE key (never stored in aw).
    pub(crate) transcript: String,
}

fn sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn inspection_sha256(report: &cold_quarantine_review::ColdQuarantineInspection) -> Result<String> {
    let canonical = serde_json::to_vec(report)
        .map_err(|_| Error::Storage("cannot encode bounded cold review counts"))?;
    Ok(sha256(&canonical))
}
fn archive_sha256(source: &Path) -> Result<String> {
    // The separate S/T source-independent validation happens first, including
    // exact complete v1 manifest and AEAD/SQLite proof. Hash only its exact
    // bounded private manifest, NOT unverified paths or plaintext secret data.
    let path = source.join("manifest.json");
    let mut file = aw_platform::open_private_existing(&path, false)?;
    let mut data = Vec::new();
    (&mut file)
        .take(MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut data)?;
    if data.is_empty() || data.len() > MAX_MANIFEST_BYTES {
        return Err(Error::Limit(
            "cold archive complete manifest exceeds bounded limit",
        ));
    }
    Ok(sha256(&data))
}
fn bounded_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(Error::Denied(
            "invalid exact cold activation request identifier",
        ));
    }
    Ok(())
}
fn principal_owns_all(c: &Connection, actor: &PrincipalId) -> Result<()> {
    cold_quarantine_review::require_original_cold_owner_of_all_sessions(c, actor)
}
fn original_event(
    c: &Connection,
    store_id: &str,
    generation: u64,
    kind: &str,
    request_id: &str,
) -> Result<serde_json::Value> {
    let mut query = db(c.prepare(
        "SELECT event_id,store_id,process_id,scope_kind,scope_key,lease_generation,payload_json
        FROM local_coordination_events WHERE kind=?1 AND store_generation=?2
          AND json_extract(payload_json,'$.request_id')=?3 ORDER BY seq LIMIT 2",
    ))?;
    let rows = db(
        query.query_map(params![kind, generation, request_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, String>(6)?,
            ))
        }),
    )?;
    let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    let [row] = rows.as_slice() else {
        return Err(Error::Denied(
            "cold prerequisite needs exactly one original immutable event",
        ));
    };
    if row.0.is_empty()
        || row.1 != store_id
        || row.2.is_some()
        || row.3 != "cold_restore"
        || row.4 != request_id
        || row.5.is_some()
    {
        return Err(Error::Denied(
            "cold prerequisite original event envelope changed",
        ));
    }
    serde_json::from_str(&row.6)
        .map_err(|_| Error::Denied("original cold prerequisite event is malformed"))
}
fn original_owner_review(
    c: &Connection,
    store_id: &str,
    generation: u64,
    request_id: &str,
) -> Result<String> {
    let fact = original_event(c, store_id, generation, OWNER_REVIEW_KIND, request_id)?;
    if fact["review_version"] != 1
        || fact["store_generation"] != generation
        || fact["intent"] != "request_separate_activation_review"
        || fact["activation_authorized"] != false
    {
        return Err(Error::Denied(
            "only an original nonactivating owner review request can start device consent",
        ));
    }
    let actor = fact["actor"]
        .as_str()
        .ok_or(Error::Denied("original review lacks a trusted principal"))?;
    principal_owns_all(c, &PrincipalId::new(actor.to_owned())?)?;
    Ok(actor.to_owned())
}
fn event_count(c: &Connection, kind: &str, generation: u64) -> Result<i64> {
    db(c.query_row(
        "SELECT count(*) FROM local_coordination_events WHERE kind=?1 AND store_generation=?2",
        params![kind, generation],
        |row| row.get(0),
    ))
}
fn current_review(
    archive: &Path,
    state: &Path,
) -> Result<(String, cold_quarantine_review::ColdQuarantineInspection)> {
    cold::with_verified_cold_snapshot(
        archive,
        state,
        |_, _, report, store_id, _| {
            require_clear_review(&report)?;
            Ok((store_id.to_owned(), report))
        },
        || Ok(()),
    )
}

fn require_clear_review(report: &cold_quarantine_review::ColdQuarantineInspection) -> Result<()> {
    if report.outstanding_review || report.activation_authorized {
        return Err(Error::Denied(
            "cold consent has unresolved original Queue, paid, Tool or Actor effects",
        ));
    }
    Ok(())
}

fn parse_claim(challenge: &ColdDeviceChallenge) -> Result<ConsentTranscript> {
    if challenge.transcript.is_empty() || challenge.transcript.len() > 4096 {
        return Err(Error::Denied(
            "cold consent requires one bounded original transcript",
        ));
    }
    let claim: ConsentTranscript = serde_json::from_str(&challenge.transcript)
        .map_err(|_| Error::Denied("cold device transcript is malformed"))?;
    if serde_json::to_string(&claim)
        .map_err(|_| Error::Denied("cannot encode original transcript"))?
        != challenge.transcript
    {
        return Err(Error::Denied(
            "noncanonical or forged copied cold device challenge",
        ));
    }
    bounded_id(&claim.request_id)?;
    if claim.domain != "aw.cold.activation.device.consent.v1"
        || claim.exact_decision != "approve_future_separately_gated_activation_no_replay"
        || claim.nonce_hex.len() != 64
        || !claim.nonce_hex.bytes().all(|b| b.is_ascii_hexdigit())
        || claim.expires_at_micros <= now()
    {
        return Err(Error::Denied(
            "expired or incorrectly scoped cold device consent",
        ));
    }
    Ok(claim)
}

fn verified_challenge_on_connection(
    c: &Connection,
    archive: &Path,
    report: &cold_quarantine_review::ColdQuarantineInspection,
    store_id: &str,
    evidence: (&ColdDeviceChallenge, &str, &str),
) -> Result<(
    ConsentTranscript,
    identity_continuity::AuthenticatedStableUser,
)> {
    let (challenge, public_key, signature) = evidence;
    let claim = parse_claim(challenge)?;
    require_clear_review(report)?;
    if claim.store_id != store_id
        || claim.restore_generation != report.generation
        || claim.archive_manifest_sha256 != archive_sha256(archive)?
        || claim.inspection_sha256 != inspection_sha256(report)?
        || original_owner_review(c, store_id, report.generation, &claim.request_id)?
            != claim.owner_principal
    {
        return Err(Error::Denied(
            "cold device proof no longer matches original Store/archive/risk/owner",
        ));
    }
    let issued = original_event(
        c,
        store_id,
        report.generation,
        AUTH_CHALLENGE_KIND,
        &claim.request_id,
    )?;
    if issued["version"] != 1
        || issued["transcript_sha256"] != sha256(challenge.transcript.as_bytes())
        || issued["archive_manifest_sha256"] != claim.archive_manifest_sha256
        || issued["inspection_sha256"] != claim.inspection_sha256
        || issued["domain_id"] != claim.domain_id
        || issued["restore_generation"] != report.generation
        || issued["expires_at_micros"] != claim.expires_at_micros
        || issued["activation_authorized"] != false
        || issued["process_granted"] != false
    {
        return Err(Error::Denied(
            "original issued cold device challenge changed",
        ));
    }
    let human = identity_continuity::authenticate_portable_device_signature_in(
        c,
        &claim.domain_id,
        public_key,
        challenge.transcript.as_bytes(),
        signature,
    )?;
    if human.method != "device_key"
        || !matches!(human.domain_role.as_deref(), Some("owner" | "admin"))
        || issued["owner_user_id"] != human.user_id
    {
        return Err(Error::Denied(
            "device proof is not the original stable Domain owner",
        ));
    }
    principal_owns_all(c, &crate::stable_user_principal(&human.user_id)?)?;
    Ok((claim, human))
}

/// AW-P6-10V: a copied `...device_consent_verified` event contains only a
/// SHA-256 of the original signature. It is NOT an independently verifiable
/// key-possession receipt on its own. Require the exact original Ed25519
/// transcript and actual detached signature AGAIN before even a dry-run.
/// No owner Store opens, Process/writer grants, replay or public API.
#[derive(Clone, Debug)]
pub(crate) struct ReverifiedColdDeviceConsent {
    pub(crate) store_id: String,
    pub(crate) generation: u64,
    pub(crate) user_id: String,
    pub(crate) request_id: String,
    pub(crate) transcript_sha256: String,
    pub(crate) inspection_sha256: String,
}

pub(crate) fn reverify_signed_cold_consent_evidence(
    archive: &Path,
    state: &Path,
    challenge: &ColdDeviceChallenge,
    public_key_hex: &str,
    signature_b64: &str,
) -> Result<ReverifiedColdDeviceConsent> {
    reverify_signed_cold_consent_evidence_impl(
        archive,
        state,
        challenge,
        public_key_hex,
        signature_b64,
        |_| Ok(()),
    )
}

#[cfg(test)]
pub(crate) fn reverify_signed_cold_consent_evidence_with_hook<F: FnOnce(&Path) -> Result<()>>(
    archive: &Path,
    state: &Path,
    challenge: &ColdDeviceChallenge,
    public_key_hex: &str,
    signature_b64: &str,
    after_snapshot: F,
) -> Result<ReverifiedColdDeviceConsent> {
    reverify_signed_cold_consent_evidence_impl(
        archive,
        state,
        challenge,
        public_key_hex,
        signature_b64,
        after_snapshot,
    )
}

fn reverify_signed_cold_consent_evidence_impl<F: FnOnce(&Path) -> Result<()>>(
    archive: &Path,
    state: &Path,
    challenge: &ColdDeviceChallenge,
    public_key_hex: &str,
    signature_b64: &str,
    after_snapshot: F,
) -> Result<ReverifiedColdDeviceConsent> {
    parse_claim(challenge)?;
    cold::with_verified_cold_snapshot(
        archive,
        state,
        |c, _, report, id, _| {
            reverify_signed_cold_consent_evidence_on_connection(
                c,
                archive,
                &report,
                id,
                challenge,
                public_key_hex,
                signature_b64,
            )
        },
        || after_snapshot(state),
    )
}

/// Original U proof on the caller's already verified, pinned v33 snapshot.
/// Also used on V's actual write transaction; no second SQLite connection.
pub(crate) fn reverify_signed_cold_consent_evidence_on_connection(
    c: &Connection,
    archive: &Path,
    report: &cold_quarantine_review::ColdQuarantineInspection,
    store_id: &str,
    challenge: &ColdDeviceChallenge,
    public_key: &str,
    signature: &str,
) -> Result<ReverifiedColdDeviceConsent> {
    let (claim, human) = verified_challenge_on_connection(
        c,
        archive,
        report,
        store_id,
        (challenge, public_key, signature),
    )?;
    let consent = original_event(
        c,
        store_id,
        report.generation,
        AUTH_CONSENT_KIND,
        &claim.request_id,
    )?;
    let transcript_hash = sha256(challenge.transcript.as_bytes());
    if consent["version"] != 1
        || consent["transcript_sha256"] != transcript_hash
        || consent["signature_sha256"] != sha256(signature.as_bytes())
        || consent["source_manifest_sha256"] != claim.archive_manifest_sha256
        || consent["inspection_sha256"] != claim.inspection_sha256
        || consent["identity_method"] != "device_key"
        || consent["decision"] != "consented_to_future_activation_review_only"
        || consent["activation_authorized"] != false
        || consent["runnable"] != false
        || consent["replay_paid_tool_queue"] != false
        || consent["original_user_id"] != human.user_id
        || consent["device_binding_id"] != human.binding_id
        || consent["user_security_epoch"] != human.user_security_epoch
        || consent["domain_security_epoch"] != human.domain_security_epoch
    {
        return Err(Error::Denied(
            "original immutable cold consent evidence changed",
        ));
    }
    Ok(ReverifiedColdDeviceConsent {
        store_id: store_id.to_owned(),
        generation: report.generation,
        user_id: human.user_id,
        request_id: claim.request_id,
        transcript_sha256: transcript_hash,
        inspection_sha256: claim.inspection_sha256,
    })
}

fn challenge_owner_on_connection(
    c: &Connection,
    store_id: &str,
    generation: u64,
    domain_id: &str,
    request_id: &str,
) -> Result<(String, String)> {
    let original_actor = original_owner_review(c, store_id, generation, request_id)?;
    // Only existing imported ORIGINAL active trust grants/bindings qualify;
    // no fresh local Unix label or unbound Ed25519 public key is consent.
    let actor = PrincipalId::new(original_actor.clone())?;
    let user_id = crate::user_for_principal(c, &actor)?.ok_or(Error::Denied(
        "original review principal has no current stable user",
    ))?;
    let has_device: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM portable_identity_bindings b
         JOIN identity_continuity_grants g ON g.domain_id=b.domain_id
           AND g.source_node_id=b.source_node_id AND g.status='active'
         JOIN users u ON u.user_id=b.user_id AND u.status='active'
         JOIN domains d ON d.domain_id=b.domain_id AND d.status='active'
         JOIN domain_memberships m ON m.domain_id=d.domain_id AND m.user_id=u.user_id
           AND m.status='active' AND m.role IN ('owner','admin')
         WHERE b.domain_id=?1 AND b.user_id=?2 AND b.kind='device_key'
           AND b.status='active' AND b.user_security_epoch=u.security_epoch
           AND b.domain_security_epoch=d.security_epoch)",
        params![domain_id, user_id],
        |row| row.get(0),
    ))?;
    if !has_device {
        return Err(Error::Denied("cold approval needs original active device-key trust and Domain owner/admin membership"));
    }
    Ok((original_actor, user_id))
}

/// U1. Issue one 5-minute cryptographically random, immutable original-v33
/// challenge bound to exact complete-archive receipt, restored generation,
/// current inspected risk-count digest and prior original Session-owner review.
/// This is an INTERNAL consent prerequisite, not activation or a provider call.
pub(crate) fn issue_cold_activation_device_challenge(
    archive: &Path,
    state: &Path,
    domain_id: &str,
    request_id: &str,
) -> Result<ColdDeviceChallenge> {
    issue_cold_activation_device_challenge_impl(
        archive,
        state,
        domain_id,
        request_id,
        (|_| Ok(()), |_| Ok(())),
    )
}

#[cfg(test)]
pub(crate) fn issue_cold_activation_device_challenge_with_hooks<F, G>(
    archive: &Path,
    state: &Path,
    domain_id: &str,
    request_id: &str,
    hooks: (F, G),
) -> Result<ColdDeviceChallenge>
where
    F: FnOnce(&Path) -> Result<()>,
    G: FnOnce(&Path) -> Result<()>,
{
    issue_cold_activation_device_challenge_impl(archive, state, domain_id, request_id, hooks)
}

fn issue_cold_activation_device_challenge_impl<F, G>(
    archive: &Path,
    state: &Path,
    domain_id: &str,
    request_id: &str,
    hooks: (F, G),
) -> Result<ColdDeviceChallenge>
where
    F: FnOnce(&Path) -> Result<()>,
    G: FnOnce(&Path) -> Result<()>,
{
    let (before_transaction, after_commit) = hooks;
    bounded_id(request_id)?;
    if domain_id.is_empty()
        || domain_id.len() > 128
        || !domain_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_:-".contains(&b))
    {
        return Err(Error::Denied("invalid original identity-continuity Domain"));
    }
    let (store_id, review) = current_review(archive, state)?;
    let original_archive_sha = archive_sha256(archive)?;
    before_transaction(state)?;
    let mut nonce = [0u8; 32];
    getrandom::fill(&mut nonce)
        .map_err(|_| Error::Storage("OS entropy unavailable for cold human challenge"))?;
    let nonce_hex = nonce
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let expires_at = now().checked_add(CHALLENGE_TTL_MICROS).ok_or(Error::Limit(
        "cold activation challenge expiration overflow",
    ))?;
    let anchors = ColdFilesystemAnchors::capture(state)?;
    let database = state.join("sessions.sqlite3");
    let _existing = aw_platform::open_private_existing(&database, false)?;
    let mut c = db(Connection::open_with_flags(
        &database,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ))?;
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let (_, fresh) =
        cold::original_facts_on_connection(&tx, &store_id, review.generation, || Ok(()))?;
    require_clear_review(&fresh)?;
    if fresh != review {
        return Err(Error::Denied(
            "cold challenge original risk report changed before audit",
        ));
    }
    let (original_actor, user_id) =
        challenge_owner_on_connection(&tx, &store_id, review.generation, domain_id, request_id)?;
    if event_count(&tx, AUTH_CHALLENGE_KIND, review.generation)? >= MAX_DEVICE_CHALLENGES {
        return Err(Error::Limit(
            "too many cold device challenges for one restored generation",
        ));
    }
    let prior: bool = db(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM local_coordination_events
         WHERE kind=?1 AND store_generation=?2 AND json_extract(payload_json,'$.request_id')=?3)",
        params![AUTH_CHALLENGE_KIND, review.generation, request_id],
        |row| row.get(0),
    ))?;
    if prior {
        return Err(Error::Conflict(
            "cold device challenge request ID already issued",
        ));
    }
    anchors.verify_source_before_review(archive, state, &store_id, review.generation)?;
    if archive_sha256(archive)? != original_archive_sha {
        return Err(Error::Denied(
            "cold challenge original manifest changed before audit",
        ));
    }
    let transcript = ConsentTranscript {
        domain: "aw.cold.activation.device.consent.v1".into(),
        store_id: store_id.clone(),
        restore_generation: review.generation,
        request_id: request_id.into(),
        owner_principal: original_actor,
        domain_id: domain_id.into(),
        nonce_hex,
        archive_manifest_sha256: original_archive_sha,
        inspection_sha256: inspection_sha256(&review)?,
        expires_at_micros: expires_at,
        exact_decision: "approve_future_separately_gated_activation_no_replay".into(),
    };
    let encoded = serde_json::to_string(&transcript)
        .map_err(|_| Error::Storage("could not encode bounded device consent transcript"))?;
    if encoded.len() > 4096 {
        return Err(Error::Limit(
            "cold device challenge transcript exceeds 4096 bytes",
        ));
    }
    let event = serde_json::json!({
        "version":1, "request_id":request_id, "domain_id":domain_id,
        "owner_user_id":user_id, "restore_generation":review.generation,
        "expires_at_micros":expires_at, "transcript_sha256":sha256(encoded.as_bytes()),
        "archive_manifest_sha256":transcript.archive_manifest_sha256,
        "inspection_sha256":transcript.inspection_sha256,
        "activation_authorized":false, "process_granted":false,
    });
    let event_id = crate::id("coord")?;
    let created_at = now();
    let event_json = serde_json::to_string(&event)
        .map_err(|_| Error::Storage("cannot encode private challenge receipt"))?;
    db(tx.execute(
        "INSERT INTO local_coordination_events(
            event_id,store_id,store_generation,process_id,kind,scope_kind,scope_key,
            lease_generation,payload_json,created_at)
         VALUES(?1,?2,?3,NULL,?4,'cold_restore',?5,NULL,?6,?7)",
        params![
            event_id,
            store_id,
            review.generation,
            AUTH_CHALLENGE_KIND,
            request_id,
            event_json,
            created_at
        ],
    ))?;
    let seq = tx.last_insert_rowid();
    db(tx.commit()).map_err(|_| Error::Storage(COLD_AUDIT_OUTCOME_UNKNOWN))?;
    drop(c);
    ColdReviewAuditRow {
        seq,
        event_id: &event_id,
        store_id: &store_id,
        generation: review.generation,
        kind: AUTH_CHALLENGE_KIND,
        scope_key: request_id,
        payload_json: &event_json,
        created_at,
    }
    .acknowledge(
        archive,
        state,
        &anchors,
        |c, _, report, id, generation| {
            require_clear_review(&report)?;
            let (actor, user) =
                challenge_owner_on_connection(c, id, generation, domain_id, request_id)?;
            if report != review
                || actor != transcript.owner_principal
                || user != user_id
                || archive_sha256(archive)? != transcript.archive_manifest_sha256
            {
                return Err(Error::Denied(
                    "cold challenge original evidence changed after commit",
                ));
            }
            let challenge = ColdDeviceChallenge {
                transcript: encoded,
            };
            parse_claim(&challenge)?;
            Ok(challenge)
        },
        after_commit,
    )
}

/// U2. Verify actual ORIGINAL active portable-device Ed25519 key/continuity
/// grant, security epochs, Domain membership and exact stable Session owner.
/// Record only an IMMUTABLE signed-consent prerequisite. NEVER activate.
pub(crate) fn record_signed_cold_activation_consent(
    archive: &Path,
    state: &Path,
    challenge: &ColdDeviceChallenge,
    public_key_hex: &str,
    signature_b64: &str,
) -> Result<()> {
    record_signed_cold_activation_consent_impl(
        archive,
        state,
        challenge,
        public_key_hex,
        signature_b64,
        (|_| Ok(()), |_| Ok(())),
    )
}

#[cfg(test)]
pub(crate) fn record_signed_cold_activation_consent_with_hooks<F, G>(
    archive: &Path,
    state: &Path,
    challenge: &ColdDeviceChallenge,
    public_key_hex: &str,
    signature_b64: &str,
    hooks: (F, G),
) -> Result<()>
where
    F: FnOnce(&Path) -> Result<()>,
    G: FnOnce(&Path) -> Result<()>,
{
    record_signed_cold_activation_consent_impl(
        archive,
        state,
        challenge,
        public_key_hex,
        signature_b64,
        hooks,
    )
}

fn record_signed_cold_activation_consent_impl<F, G>(
    archive: &Path,
    state: &Path,
    challenge: &ColdDeviceChallenge,
    public_key_hex: &str,
    signature_b64: &str,
    hooks: (F, G),
) -> Result<()>
where
    F: FnOnce(&Path) -> Result<()>,
    G: FnOnce(&Path) -> Result<()>,
{
    let (before_transaction, after_commit) = hooks;
    parse_claim(challenge)?;
    let (store_id, inspection) = cold::with_verified_cold_snapshot(
        archive,
        state,
        |c, _, report, id, _| {
            verified_challenge_on_connection(
                c,
                archive,
                &report,
                id,
                (challenge, public_key_hex, signature_b64),
            )?;
            Ok((id.to_owned(), report))
        },
        || Ok(()),
    )?;
    before_transaction(state)?;
    let anchors = ColdFilesystemAnchors::capture(state)?;
    let database = state.join("sessions.sqlite3");
    let mut c = db(Connection::open_with_flags(
        &database,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ))?;
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let (_, report) =
        cold::original_facts_on_connection(&tx, &store_id, inspection.generation, || Ok(()))?;
    if report != inspection {
        return Err(Error::Denied(
            "cold consent original risk report changed before audit",
        ));
    }
    let (transcript, human) = verified_challenge_on_connection(
        &tx,
        archive,
        &report,
        &store_id,
        (challenge, public_key_hex, signature_b64),
    )?;
    let already: bool = db(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM local_coordination_events
        WHERE kind=?1 AND store_generation=?2 AND json_extract(payload_json,'$.request_id')=?3)",
        params![
            AUTH_CONSENT_KIND,
            inspection.generation,
            transcript.request_id
        ],
        |r| r.get(0),
    ))?;
    if already {
        return Err(Error::Conflict(
            "original signed cold consent already recorded",
        ));
    }
    anchors.verify_source_before_review(archive, state, &store_id, inspection.generation)?;
    let event = serde_json::json!({
        "version":1, "request_id":transcript.request_id,
        "original_user_id":human.user_id, "device_binding_id":human.binding_id,
        "identity_method":"device_key", "user_security_epoch":human.user_security_epoch,
        "domain_security_epoch":human.domain_security_epoch,
        "source_manifest_sha256":transcript.archive_manifest_sha256,
        "inspection_sha256":transcript.inspection_sha256,
        "signature_sha256":sha256(signature_b64.as_bytes()),
        "transcript_sha256":sha256(challenge.transcript.as_bytes()),
        "decision":"consented_to_future_activation_review_only",
        "activation_authorized":false, "runnable":false,
        "replay_paid_tool_queue":false,
    });
    let event_json = serde_json::to_string(&event)
        .map_err(|_| Error::Storage("cannot encode signed consent evidence"))?;
    let event_id = crate::id("coord")?;
    let created_at = now();
    db(tx.execute("INSERT INTO local_coordination_events(
        event_id,store_id,store_generation,process_id,kind,scope_kind,scope_key,lease_generation,payload_json,created_at)
        VALUES(?1,?2,?3,NULL,?4,'cold_restore',?5,NULL,?6,?7)",
        params![event_id, store_id, inspection.generation, AUTH_CONSENT_KIND, transcript.request_id, event_json, created_at]))?;
    let seq = tx.last_insert_rowid();
    db(tx.commit()).map_err(|_| Error::Storage(COLD_AUDIT_OUTCOME_UNKNOWN))?;
    drop(c);
    ColdReviewAuditRow {
        seq,
        event_id: &event_id,
        store_id: &store_id,
        generation: inspection.generation,
        kind: AUTH_CONSENT_KIND,
        scope_key: &transcript.request_id,
        payload_json: &event_json,
        created_at,
    }
    .acknowledge(
        archive,
        state,
        &anchors,
        |c, _, report, id, _| {
            reverify_signed_cold_consent_evidence_on_connection(
                c,
                archive,
                &report,
                id,
                challenge,
                public_key_hex,
                signature_b64,
            )?;
            Ok(())
        },
        after_commit,
    )
}
