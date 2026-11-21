//! Replay-stable durable admission facts for unified host policy decisions.
//!
//! This module owns no policy configuration, ToolPolicy grant, provider, Agent
//! runner or effect executor. It derives immutable admission/guardian/approval
//! facts from the existing Session audit and current writer/ACL authority.

use crate::{
    audit, branch_in, context, db, now, principal_is_session_owner, session_participation,
    workspace_writer, Error, PrincipalId, Result, SessionStore,
};
use aw_agent_core::{
    AuxiliaryInput, AuxiliaryJournal, AuxiliaryResult, InvocationContext, InvocationIdentity,
    InvocationPurpose, JournalFuture, Outcome,
};
use aw_policy::{Decision, PolicyDecision, Strategy};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;

const ACTION: &str = "policy.admission";
const MAX_ADMISSIONS_PER_TURN: usize = 64;
const MAX_APPROVERS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyAdmissionState {
    Claimed,
    GuardianStarted,
    GuardianSettled,
    GuardianUnknown,
    Allowed,
    Denied,
    AskPending,
    AskApproved,
    AskDenied,
    AskExpired,
    Narrowed,
    Consumed,
}

impl PolicyAdmissionState {
    pub const fn needs_inspection(self) -> bool {
        matches!(self, Self::GuardianStarted | Self::GuardianUnknown)
    }

    pub const fn executable(self) -> bool {
        matches!(self, Self::Allowed | Self::AskApproved | Self::Narrowed)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyAdmissionSubject {
    pub actor: PrincipalId,
    pub membership_epoch: u64,
    pub audience_epoch: u64,
    pub owner: bool,
}

impl PolicyAdmissionSubject {
    pub fn from_participation(
        evidence: &session_participation::SessionParticipationEvidence,
    ) -> Self {
        Self {
            actor: evidence.actor.clone(),
            membership_epoch: evidence.membership_epoch,
            audience_epoch: evidence.audience_epoch,
            owner: evidence.owner,
        }
    }

    fn same_identity(&self, other: &Self) -> bool {
        self.actor == other.actor
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyAdmissionRequest {
    pub session_id: String,
    pub branch_id: String,
    pub writer_epoch: u64,
    pub turn_id: u64,
    pub call_id: String,
    pub canonical_target: String,
    pub request_sha256: String,
    pub subject: PolicyAdmissionSubject,
    pub policy_revision_sha256: String,
    pub authority_sha256: String,
    pub candidate_local: bool,
    pub candidate_delegate_agent: bool,
    pub guardian_required: bool,
    pub guardian_identity: Option<InvocationIdentity>,
    pub guardian_site: Option<String>,
    pub approval_scope: Option<String>,
    pub authorized_approvers: Vec<String>,
    pub approval_expires_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyAdmissionFact {
    pub id: String,
    pub request: PolicyAdmissionRequest,
    pub state: PolicyAdmissionState,
    pub revision: u64,
    /// True when the indexed replay owner retained only the immutable
    /// never-retry identity/outcome tombstone. Mutable policy/provider/approval
    /// detail in `request` is then non-authoritative placeholder data.
    #[serde(default, skip_serializing_if = "is_false")]
    pub compacted: bool,
    pub guardian_process_id: Option<String>,
    pub guardian_store_generation: Option<u64>,
    pub guardian_input_sha256: Option<String>,
    pub guardian_result_sha256: Option<String>,
    pub guardian_meta: Option<aw_agent_core::CompletionMeta>,
    pub decision_revision: Option<u64>,
    pub decision_sha256: Option<String>,
    pub decision_route: Option<String>,
    pub narrow_ref: Option<String>,
    pub approval_decided_by: Option<String>,
    pub note: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyAdmissionTombstone {
    replay_tombstone_version: u8,
    state: PolicyAdmissionState,
    decision_route: Option<String>,
    narrow_ref: Option<String>,
}

#[derive(Debug)]
struct PolicyAdmissionCurrentRow {
    admission_id: String,
    session_id: String,
    branch_id: String,
    turn_id: u64,
    call_id: String,
    canonical_target: String,
    subject_actor: String,
    request_sha256: String,
    revision: u64,
    fact_json: String,
}

fn bounded_ascii(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.is_ascii()
        && !value.chars().any(char::is_control)
}

fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_request(request: &PolicyAdmissionRequest) -> Result<()> {
    if !bounded_ascii(&request.session_id, 128)
        || !bounded_ascii(&request.branch_id, 128)
        || request.writer_epoch == 0
        || request.turn_id == 0
        || !bounded_ascii(&request.call_id, 256)
        || !bounded_ascii(&request.canonical_target, 2_048)
        || !digest(&request.request_sha256)
        || request.subject.membership_epoch == 0
        || request.subject.audience_epoch == 0
        || !digest(&request.policy_revision_sha256)
        || !digest(&request.authority_sha256)
        || !(request.candidate_local || request.candidate_delegate_agent)
        || request.authorized_approvers.len() > MAX_APPROVERS
    {
        return Err(Error::Denied("invalid policy admission request"));
    }

    if request.subject.actor.as_str().is_empty() {
        return Err(Error::Denied("policy admission subject is invalid"));
    }

    if request.guardian_required {
        let identity = request.guardian_identity.as_ref().ok_or(Error::Denied(
            "guardian policy admission needs a pinned invocation identity",
        ))?;
        if identity.grant_revision == 0
            || identity.actor != request.subject.actor
            || request
                .guardian_site
                .as_deref()
                .is_none_or(|site| !bounded_ascii(site, 128))
        {
            return Err(Error::Denied(
                "guardian policy admission identity/site is invalid",
            ));
        }
    } else if request.guardian_identity.is_some() || request.guardian_site.is_some() {
        return Err(Error::Denied(
            "non-guardian policy admission cannot reserve hidden provider authority",
        ));
    }

    let mut seen = BTreeMap::<&str, ()>::new();
    for approver in &request.authorized_approvers {
        if !bounded_ascii(approver, 128) || seen.insert(approver.as_str(), ()).is_some() {
            return Err(Error::Denied(
                "policy admission approvers must be unique bounded principals",
            ));
        }
    }
    if request.authorized_approvers.is_empty() {
        if request.approval_scope.is_some() || request.approval_expires_at.is_some() {
            return Err(Error::Denied(
                "policy admission cannot reserve approval metadata without approvers",
            ));
        }
    } else if request
        .approval_scope
        .as_deref()
        .is_none_or(|scope| !bounded_ascii(scope, 1_024))
        || request.approval_expires_at.is_none_or(|expiry| expiry <= 0)
    {
        return Err(Error::Denied(
            "policy admission approval reservation is incomplete",
        ));
    }
    Ok(())
}

pub(crate) fn admission_id(owner: &PrincipalId, request: &PolicyAdmissionRequest) -> String {
    let mut hash = Sha256::new();
    hash.update(b"aw-policy-admission-replay-v1\0");
    for part in [
        owner.as_str().as_bytes(),
        request.session_id.as_bytes(),
        request.branch_id.as_bytes(),
        &request.turn_id.to_be_bytes(),
        request.call_id.as_bytes(),
        request.canonical_target.as_bytes(),
    ] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    format!("pa_{:x}", hash.finalize())
}

fn replay_matches(a: &PolicyAdmissionRequest, b: &PolicyAdmissionRequest) -> bool {
    a.session_id == b.session_id
        && a.branch_id == b.branch_id
        && a.turn_id == b.turn_id
        && a.call_id == b.call_id
        // A contextual ref may resolve to a different canonical target after
        // reparenting. The original request fingerprint is the stable intent;
        // replay must retain the admitted target, never resolve it again.
        && a.request_sha256 == b.request_sha256
        && a.subject.same_identity(&b.subject)
}

/// Find the original claim before using the caller's *current* resolved target.
/// This existing index also detects historical ambiguous call-ID reuse: never
/// choose one of several prior targets or start another paid/effectful attempt.
fn current_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PolicyAdmissionCurrentRow> {
    Ok(PolicyAdmissionCurrentRow {
        admission_id: row.get(0)?,
        session_id: row.get(1)?,
        branch_id: row.get(2)?,
        turn_id: row.get(3)?,
        call_id: row.get(4)?,
        canonical_target: row.get(5)?,
        subject_actor: row.get(6)?,
        request_sha256: row.get(7)?,
        revision: row.get(8)?,
        fact_json: row.get(9)?,
    })
}

fn decode_current(row: PolicyAdmissionCurrentRow) -> Result<PolicyAdmissionFact> {
    let value: serde_json::Value = serde_json::from_str(&row.fact_json)
        .map_err(|_| Error::Storage("cannot decode policy admission current row"))?;
    if value.get("replay_tombstone_version").is_none() {
        return serde_json::from_value(value)
            .map_err(|_| Error::Storage("cannot decode policy admission fact"));
    }
    let tombstone: PolicyAdmissionTombstone = serde_json::from_value(value)
        .map_err(|_| Error::Storage("cannot decode policy admission replay tombstone"))?;
    if tombstone.replay_tombstone_version != 1 || tombstone.state != PolicyAdmissionState::Consumed
    {
        return Err(Error::Storage(
            "unsupported policy admission replay tombstone",
        ));
    }
    let actor = PrincipalId::new(row.subject_actor)?;
    let candidate_local = tombstone.decision_route.as_deref() != Some("delegate_agent");
    let candidate_delegate_agent = tombstone.decision_route.as_deref() == Some("delegate_agent");
    Ok(PolicyAdmissionFact {
        id: row.admission_id,
        request: PolicyAdmissionRequest {
            session_id: row.session_id,
            branch_id: row.branch_id,
            writer_epoch: 1,
            turn_id: row.turn_id,
            call_id: row.call_id,
            canonical_target: row.canonical_target,
            request_sha256: row.request_sha256,
            subject: PolicyAdmissionSubject {
                actor,
                membership_epoch: 1,
                audience_epoch: 1,
                owner: false,
            },
            policy_revision_sha256: "0".repeat(64),
            authority_sha256: "0".repeat(64),
            candidate_local,
            candidate_delegate_agent,
            guardian_required: false,
            guardian_identity: None,
            guardian_site: None,
            approval_scope: None,
            authorized_approvers: Vec::new(),
            approval_expires_at: None,
        },
        state: tombstone.state,
        revision: row.revision,
        compacted: true,
        guardian_process_id: None,
        guardian_store_generation: None,
        guardian_input_sha256: None,
        guardian_result_sha256: None,
        guardian_meta: None,
        decision_revision: None,
        decision_sha256: None,
        decision_route: tombstone.decision_route,
        narrow_ref: tombstone.narrow_ref,
        approval_decided_by: None,
        note: None,
    })
}

fn current_fact_for_call(
    c: &rusqlite::Connection,
    session: &str,
    branch: &str,
    turn_id: u64,
    call_id: &str,
) -> Result<Option<PolicyAdmissionFact>> {
    let mut stmt = db(c.prepare(
        "SELECT admission_id,session_id,branch_id,turn_id,call_id,canonical_target,
                subject_actor,request_sha256,revision,fact_json
         FROM policy_admission_current
         WHERE session_id=?1 AND branch_id=?2 AND turn_id=?3 AND call_id=?4
         LIMIT 2",
    ))?;
    let mut rows = db(stmt.query(params![session, branch, turn_id, call_id]))?;
    let Some(row) = db(rows.next())? else {
        return Ok(None);
    };
    let row = current_row(row).map_err(|_| Error::Storage("cannot read policy admission row"))?;
    if db(rows.next())?.is_some() {
        return Err(Error::Conflict(
            "original policy ToolCall has ambiguous legacy replay targets",
        ));
    }
    decode_current(row).map(Some)
}

fn current_fact(c: &rusqlite::Connection, id: &str) -> Result<Option<PolicyAdmissionFact>> {
    let row: Option<PolicyAdmissionCurrentRow> = db(c
        .query_row(
            "SELECT admission_id,session_id,branch_id,turn_id,call_id,canonical_target,
                    subject_actor,request_sha256,revision,fact_json
             FROM policy_admission_current WHERE admission_id=?1",
            [id],
            current_row,
        )
        .optional())?;
    row.map(decode_current).transpose()
}

fn compact_consumed_current(c: &rusqlite::Connection, fact: &PolicyAdmissionFact) -> Result<()> {
    if fact.state != PolicyAdmissionState::Consumed {
        return Err(Error::Storage(
            "only consumed policy admissions may compact to replay tombstones",
        ));
    }
    let tombstone = PolicyAdmissionTombstone {
        replay_tombstone_version: 1,
        state: fact.state,
        decision_route: fact.decision_route.clone(),
        narrow_ref: fact.narrow_ref.clone(),
    };
    let encoded = serde_json::to_string(&tombstone)
        .map_err(|_| Error::Storage("cannot serialize policy admission replay tombstone"))?;
    let changed = db(c.execute(
        "UPDATE policy_admission_current
         SET fact_json=?1,updated_at=?2
         WHERE admission_id=?3 AND revision=?4",
        params![encoded, now(), fact.id, fact.revision],
    ))?;
    if changed != 1 {
        return Err(Error::Storage(
            "policy admission replay tombstone lost its exact current row",
        ));
    }
    Ok(())
}

fn store_current(c: &rusqlite::Connection, fact: &PolicyAdmissionFact) -> Result<()> {
    let encoded = serde_json::to_string(fact)
        .map_err(|_| Error::Storage("cannot serialize policy admission fact"))?;
    let current: Option<(u64, String)> = db(c
        .query_row(
            "SELECT revision,fact_json FROM policy_admission_current WHERE admission_id=?1",
            [&fact.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional())?;
    match current {
        None if fact.revision == 1 => {}
        Some((revision, existing)) if revision == fact.revision && existing == encoded => {
            return Ok(());
        }
        Some((revision, _)) if revision.checked_add(1) == Some(fact.revision) => {}
        None => {
            return Err(Error::Storage(
                "policy admission projection must begin at revision one",
            ));
        }
        Some(_) => {
            return Err(Error::Storage(
                "policy admission projection revision is not monotonic",
            ));
        }
    }
    db(c.execute(
        "INSERT INTO policy_admission_current(
            admission_id,session_id,branch_id,turn_id,call_id,canonical_target,
            subject_actor,request_sha256,revision,fact_json,updated_at
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
         ON CONFLICT(admission_id) DO UPDATE SET
            session_id=excluded.session_id,
            branch_id=excluded.branch_id,
            turn_id=excluded.turn_id,
            call_id=excluded.call_id,
            canonical_target=excluded.canonical_target,
            subject_actor=excluded.subject_actor,
            request_sha256=excluded.request_sha256,
            revision=excluded.revision,
            fact_json=excluded.fact_json,
            updated_at=excluded.updated_at",
        params![
            fact.id,
            fact.request.session_id,
            fact.request.branch_id,
            fact.request.turn_id,
            fact.request.call_id,
            fact.request.canonical_target,
            fact.request.subject.actor.as_str(),
            fact.request.request_sha256,
            fact.revision,
            encoded,
            now(),
        ],
    ))?;
    Ok(())
}

pub(crate) fn backfill_current(c: &rusqlite::Connection) -> Result<()> {
    let mut query = db(c.prepare(
        "SELECT data_json FROM audit
         WHERE action=?1
         ORDER BY seq",
    ))?;
    let rows = db(query.query_map([ACTION], |row| row.get::<_, String>(0)))?;
    for row in rows {
        let encoded = db(row)?;
        let mut value: serde_json::Value = serde_json::from_str(&encoded)
            .map_err(|_| Error::Storage("cannot decode legacy policy admission fact"))?;
        let request = value
            .get_mut("request")
            .and_then(serde_json::Value::as_object_mut)
            .ok_or(Error::Storage(
                "legacy policy admission request is malformed",
            ))?;
        if !request.contains_key("subject") {
            let session_id = request
                .get("session_id")
                .and_then(serde_json::Value::as_str)
                .ok_or(Error::Storage(
                    "legacy policy admission Session identity is malformed",
                ))?;
            let actor = crate::session_in(c, session_id)?.owner;
            request.insert(
                "subject".into(),
                serde_json::json!({
                    "actor": actor,
                    "membership_epoch": 1,
                    "audience_epoch": 1,
                    "owner": true,
                }),
            );
        }
        let fact: PolicyAdmissionFact = serde_json::from_value(value)
            .map_err(|_| Error::Storage("cannot migrate legacy policy admission fact"))?;
        store_current(c, &fact)?;
    }
    Ok(())
}

pub(crate) fn fact_in(
    connection: &rusqlite::Connection,
    session: &str,
    id: &str,
) -> Result<PolicyAdmissionFact> {
    let fact = current_fact(connection, id)?
        .ok_or(Error::Denied("policy admission fact is unavailable"))?;
    if fact.request.session_id != session {
        return Err(Error::Denied(
            "policy admission fact belongs to another Session",
        ));
    }
    Ok(fact)
}

fn audit_fact(
    c: &rusqlite::Connection,
    actor: &PrincipalId,
    fact: &PolicyAdmissionFact,
) -> Result<()> {
    store_current(c, fact)?;
    audit(
        c,
        &fact.request.session_id,
        actor,
        ACTION,
        serde_json::to_value(fact)
            .map_err(|_| Error::Storage("cannot serialize policy admission fact"))?,
    )
}

fn require_subject(c: &rusqlite::Connection, request: &PolicyAdmissionRequest) -> Result<()> {
    let current = session_participation::authorize_in(
        c,
        &request.subject.actor,
        &request.session_id,
        session_participation::SessionParticipationAction::Submit,
    )?;
    if current.membership_epoch != request.subject.membership_epoch
        || current.audience_epoch != request.subject.audience_epoch
        || current.owner != request.subject.owner
    {
        return Err(Error::Denied(
            "policy admission subject participation changed after admission",
        ));
    }
    Ok(())
}

fn require_owner_writer(
    c: &rusqlite::Connection,
    owner: &PrincipalId,
    request: &PolicyAdmissionRequest,
) -> Result<()> {
    if !principal_is_session_owner(c, &request.session_id, owner)? {
        return Err(Error::Denied(
            "policy admission resource owner differs from the durable Session owner",
        ));
    }
    context::authorize_tree(c, &request.session_id, owner)?;
    let writer = workspace_writer::verify_writer_epoch_for_session(
        c,
        &request.session_id,
        request.writer_epoch,
    )?;
    if writer.state != "active" {
        return Err(Error::Denied(
            "policy admission writer epoch is no longer active",
        ));
    }
    let branch = branch_in(c, &request.session_id, &request.branch_id)?;
    if branch.id != request.branch_id {
        return Err(Error::Denied(
            "policy admission requires the exact canonical branch ID",
        ));
    }
    Ok(())
}

fn require_active_turn(
    c: &rusqlite::Connection,
    owner: &PrincipalId,
    request: &PolicyAdmissionRequest,
) -> Result<()> {
    require_owner_writer(c, owner, request)?;
    require_subject(c, request)?;
    let branch = branch_in(c, &request.session_id, &request.branch_id)?;
    let run = branch.active_run.as_deref().ok_or(Error::Conflict(
        "policy admission requires an active original Turn",
    ))?;
    let active_turn: u64 = db(c.query_row(
        "SELECT turn_id FROM runs
         WHERE id=?1 AND session_id=?2 AND branch_id=?3 AND status='running'",
        params![run, request.session_id, request.branch_id],
        |row| row.get(0),
    ))?;
    if active_turn != request.turn_id {
        return Err(Error::Conflict(
            "policy admission targets a different active Turn",
        ));
    }
    Ok(())
}

fn next_fact(
    current: &PolicyAdmissionFact,
    state: PolicyAdmissionState,
) -> Result<PolicyAdmissionFact> {
    let mut next = current.clone();
    next.state = state;
    next.revision = next
        .revision
        .checked_add(1)
        .ok_or(Error::Limit("policy admission revision exhausted"))?;
    Ok(next)
}

fn route_label(route: Option<Strategy>) -> Option<&'static str> {
    route.map(|route| match route {
        Strategy::Local => "local",
        Strategy::DelegateAgent => "delegate_agent",
    })
}

fn validate_decision(
    request: &PolicyAdmissionRequest,
    decision: &PolicyDecision<'_>,
) -> Result<()> {
    if decision.revision == 0 {
        return Err(Error::Denied("policy decision revision is invalid"));
    }
    if let Some(route) = decision.route {
        let permitted = match route {
            Strategy::Local => request.candidate_local,
            Strategy::DelegateAgent => request.candidate_delegate_agent,
        };
        if !permitted {
            return Err(Error::Denied(
                "policy decision selected a route outside the captured candidates",
            ));
        }
    }
    match decision.decision {
        Decision::Allow => {
            if decision.approval_scope.is_some() || decision.narrow_ref.is_some() {
                return Err(Error::Denied("invalid Allow policy decision evidence"));
            }
        }
        Decision::Deny => {
            if decision.route.is_some()
                || decision.approval_scope.is_some()
                || decision.narrow_ref.is_some()
            {
                return Err(Error::Denied("invalid Deny policy decision evidence"));
            }
        }
        Decision::Ask => {
            if request.authorized_approvers.is_empty()
                || decision.approval_scope != request.approval_scope.as_deref()
                || decision.narrow_ref.is_some()
                || request
                    .approval_expires_at
                    .is_none_or(|expiry| expiry <= now())
            {
                return Err(Error::Denied(
                    "Ask policy decision lacks its pre-reserved approval owner",
                ));
            }
        }
        Decision::Narrow => {
            if decision
                .narrow_ref
                .is_none_or(|reference| !bounded_ascii(reference, 256))
                || decision.approval_scope.is_some()
            {
                return Err(Error::Denied("invalid Narrow policy decision evidence"));
            }
        }
    }
    Ok(())
}

#[derive(Clone)]
pub struct PolicyGuardianJournal {
    store: SessionStore,
    actor: PrincipalId,
    session: String,
    admission_id: String,
}

impl PolicyGuardianJournal {
    pub fn new(
        store: SessionStore,
        actor: PrincipalId,
        session: impl Into<String>,
        admission_id: impl Into<String>,
    ) -> Self {
        Self {
            store,
            actor,
            session: session.into(),
            admission_id: admission_id.into(),
        }
    }

    fn authorize_current(
        &self,
        input: &AuxiliaryInput,
        identity: &InvocationIdentity,
    ) -> Result<PolicyAdmissionFact> {
        input.validate()?;
        let fact =
            self.store
                .policy_admission_fact(&self.actor, &self.session, &self.admission_id)?;
        let expected = fact
            .request
            .guardian_identity
            .as_ref()
            .ok_or(Error::Denied(
                "policy admission has no guardian invocation identity",
            ))?;
        if expected != identity
            || fact.request.guardian_site.as_deref() != Some(identity.inference_executor.as_str())
            || !matches!(
                fact.state,
                PolicyAdmissionState::Claimed | PolicyAdmissionState::GuardianStarted
            )
        {
            return Err(Error::Denied(
                "policy guardian identity/site/state changed before dispatch",
            ));
        }
        if fact.state == PolicyAdmissionState::GuardianStarted
            && fact.guardian_input_sha256.as_deref() != Some(input.digest()?.as_str())
        {
            return Err(Error::Denied(
                "policy guardian input changed after paid-attempt admission",
            ));
        }
        Ok(fact)
    }
}

impl AuxiliaryJournal for PolicyGuardianJournal {
    fn authorize<'a>(
        &'a self,
        input: &'a AuxiliaryInput,
        identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            self.authorize_current(input, identity)?;
            Ok(())
        })
    }

    fn begin<'a>(
        &'a self,
        input: &'a AuxiliaryInput,
        identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, InvocationContext> {
        Box::pin(async move {
            let before = self.authorize_current(input, identity)?;
            let digest = input.digest()?;
            let started = self.store.policy_admission_start_guardian(
                &self.session,
                &self.admission_id,
                &digest,
            )?;
            if started.request != before.request
                || started.request.guardian_identity.as_ref() != Some(identity)
            {
                return Err(Error::Conflict(
                    "policy guardian admission changed during durable start",
                ));
            }
            Ok(InvocationContext {
                purpose: InvocationPurpose::Auxiliary {
                    operation_id: self.admission_id.clone(),
                    input_sha256: digest,
                    auxiliary_kind: input.kind.label().into(),
                },
                scope: Some(aw_agent_core::InvocationScope {
                    session_id: started.request.session_id.clone(),
                    branch_id: started.request.branch_id.clone(),
                }),
                turn_id: started.request.turn_id,
                ordinal: 1,
                identity: identity.clone(),
            })
        })
    }

    fn finish<'a>(&'a self, result: &'a AuxiliaryResult) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            self.store.policy_admission_finish_guardian(
                &self.actor,
                &self.session,
                &self.admission_id,
                result,
            )?;
            Ok(())
        })
    }
}

impl SessionStore {
    /// Recover a factual admission using the authenticated original ToolCall,
    /// before selecting a new route, resolving a contextual target or checking
    /// today's effect authority. This never authorizes a new execution.
    pub fn policy_admission_replay(
        &self,
        actor: &PrincipalId,
        session: &str,
        branch: &str,
        turn_id: u64,
        call_id: &str,
        original_request_sha256: &str,
    ) -> Result<Option<PolicyAdmissionFact>> {
        if !bounded_ascii(session, 128)
            || !bounded_ascii(branch, 128)
            || turn_id == 0
            || !bounded_ascii(call_id, 256)
            || !digest(original_request_sha256)
        {
            return Err(Error::Denied("invalid policy replay lookup identity"));
        }
        let connection = self.connection()?;
        // Source/recipient audience is current even for historical facts.
        session_participation::authorize_in(
            &connection,
            actor,
            session,
            session_participation::SessionParticipationAction::View,
        )?;
        let Some(existing) = current_fact_for_call(&connection, session, branch, turn_id, call_id)?
        else {
            return Ok(None);
        };
        if existing.request.subject.actor != *actor
            || existing.request.request_sha256 != original_request_sha256
        {
            return Err(Error::Conflict(
                "policy replay identity was reused with changed original ToolCall or actor",
            ));
        }
        Ok(Some(existing))
    }

    /// Claim the original replay identity and bounded guardian/approval slots
    /// before any auxiliary model call. Exact redelivery returns the same fact.
    pub fn policy_admission_claim(
        &self,
        request: PolicyAdmissionRequest,
    ) -> Result<PolicyAdmissionFact> {
        validate_request(&request)?;
        if request
            .guardian_identity
            .as_ref()
            .is_some_and(|identity| identity.actor != request.subject.actor)
        {
            return Err(Error::Denied(
                "policy guardian identity actor differs from the admitted subject",
            ));
        }
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let owner = crate::session_in(&tx, &request.session_id)?.owner;
        // Replay lookup uses the immutable authenticated ToolCall identity,
        // not its newly resolved target or changed writer/policy generations.
        let id = admission_id(&owner, &request);
        if let Some(existing) = current_fact_for_call(
            &tx,
            &request.session_id,
            &request.branch_id,
            request.turn_id,
            &request.call_id,
        )? {
            if !replay_matches(&existing.request, &request) {
                return Err(Error::Conflict(
                    "policy replay identity was reused with changed invocation/subject",
                ));
            }
            // Factual old work survives revocation; private receipt disclosure
            // does not. The higher result projector also applies source and
            // dependent-content disclosure before returning output bytes.
            session_participation::authorize_in(
                &tx,
                &request.subject.actor,
                &request.session_id,
                session_participation::SessionParticipationAction::View,
            )?;
            db(tx.commit())?;
            return Ok(existing);
        }

        require_active_turn(&tx, &owner, &request)?;
        let unique: usize = db(tx.query_row(
            "SELECT count(*) FROM policy_admission_current
             WHERE session_id=?1 AND branch_id=?2 AND turn_id=?3",
            params![request.session_id, request.branch_id, request.turn_id],
            |row| row.get(0),
        ))?;
        if unique >= MAX_ADMISSIONS_PER_TURN {
            return Err(Error::Limit(
                "policy admission count exceeds the bounded active-Turn budget",
            ));
        }
        let fact = PolicyAdmissionFact {
            id,
            request,
            state: PolicyAdmissionState::Claimed,
            revision: 1,
            compacted: false,
            guardian_process_id: None,
            guardian_store_generation: None,
            guardian_input_sha256: None,
            guardian_result_sha256: None,
            guardian_meta: None,
            decision_revision: None,
            decision_sha256: None,
            decision_route: None,
            narrow_ref: None,
            approval_decided_by: None,
            note: None,
        };
        audit_fact(&tx, &fact.request.subject.actor, &fact)?;
        db(tx.commit())?;
        Ok(fact)
    }

    pub fn policy_admission_fact(
        &self,
        reader: &PrincipalId,
        session: &str,
        id: &str,
    ) -> Result<PolicyAdmissionFact> {
        let connection = self.connection()?;
        context::authorize_tree(&connection, session, reader)?;
        fact_in(&connection, session, id)
    }

    /// Commit the single reserved paid guardian attempt before dispatch. A
    /// repeated call after this point fails rather than paying again.
    pub fn policy_admission_start_guardian(
        &self,
        session: &str,
        id: &str,
        input_sha256: &str,
    ) -> Result<PolicyAdmissionFact> {
        if !digest(input_sha256) {
            return Err(Error::Denied("invalid policy guardian input digest"));
        }
        let process = self.process_identity()?.ok_or(Error::Denied(
            "paid policy guardian requires a current owner process",
        ))?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current = fact_in(&tx, session, id)?;
        let owner = crate::session_in(&tx, session)?.owner;
        require_active_turn(&tx, &owner, &current.request)?;
        if !current.request.guardian_required || current.request.guardian_identity.is_none() {
            return Err(Error::Denied(
                "policy admission has no reserved guardian attempt",
            ));
        }
        if current.state != PolicyAdmissionState::Claimed {
            return Err(Error::Conflict(
                "policy guardian attempt was already started or settled",
            ));
        }
        let mut next = next_fact(&current, PolicyAdmissionState::GuardianStarted)?;
        next.guardian_process_id = Some(process.process_id);
        next.guardian_store_generation = Some(process.store_generation);
        next.guardian_input_sha256 = Some(input_sha256.to_owned());
        audit_fact(&tx, &current.request.subject.actor, &next)?;
        db(tx.commit())?;
        Ok(next)
    }

    /// Factual receipt for the one started guardian attempt. Settlement is
    /// allowed after authority revocation because it records already-attempted
    /// provider work; it never authorizes another attempt.
    pub fn policy_admission_finish_guardian(
        &self,
        actor: &PrincipalId,
        session: &str,
        id: &str,
        result: &AuxiliaryResult,
    ) -> Result<PolicyAdmissionFact> {
        if result.operation_id != id {
            return Err(Error::Conflict(
                "guardian result operation differs from policy admission",
            ));
        }
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current = fact_in(&tx, session, id)?;
        if current.state != PolicyAdmissionState::GuardianStarted {
            return Err(Error::Conflict(
                "policy guardian result has no exact started attempt",
            ));
        }
        if current
            .request
            .guardian_identity
            .as_ref()
            .is_none_or(|identity| identity.actor != *actor)
        {
            return Err(Error::Denied(
                "policy guardian result belongs to another actor",
            ));
        }
        let process = self
            .process_identity()?
            .ok_or(Error::Conflict("guardian owner process disappeared"))?;
        if current.guardian_process_id.as_deref() != Some(process.process_id.as_str())
            || current.guardian_store_generation != Some(process.store_generation)
        {
            return Err(Error::Conflict(
                "guardian result belongs to another owner process incarnation",
            ));
        }

        let complete = result.outcome == Outcome::Completed
            && result.meta.as_ref().is_some_and(|meta| meta.complete())
            && !result.text.trim().is_empty();
        let mut next = next_fact(
            &current,
            if complete {
                PolicyAdmissionState::GuardianSettled
            } else {
                PolicyAdmissionState::GuardianUnknown
            },
        )?;
        next.guardian_result_sha256 = if complete {
            Some(format!("{:x}", Sha256::digest(result.text.as_bytes())))
        } else {
            None
        };
        next.guardian_meta = result.meta.clone();
        if !complete {
            next.note = Some(
                result
                    .error
                    .as_deref()
                    .unwrap_or("guardian attempt did not produce a complete factual decision")
                    .chars()
                    .take(1_024)
                    .collect(),
            );
        }
        audit_fact(&tx, actor, &next)?;
        db(tx.commit())?;
        Ok(next)
    }

    /// Persist a deterministic or guardian decision. The decision is evidence
    /// inside the original replay claim; it never grants missing authority.
    pub fn policy_admission_record_decision(
        &self,
        session: &str,
        id: &str,
        decision: PolicyDecision<'_>,
        decision_sha256: &str,
    ) -> Result<PolicyAdmissionFact> {
        if !digest(decision_sha256) {
            return Err(Error::Denied("invalid policy decision digest"));
        }
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current = fact_in(&tx, session, id)?;
        validate_decision(&current.request, &decision)?;

        if current.request.guardian_required {
            if current.state != PolicyAdmissionState::GuardianSettled {
                return Err(Error::Conflict(
                    "guardian policy decision lacks one completed factual guardian receipt",
                ));
            }
            if current.guardian_result_sha256.as_deref() != Some(decision_sha256) {
                return Err(Error::Denied(
                    "guardian policy decision digest differs from the completed response",
                ));
            }
            let process = self
                .process_identity()?
                .ok_or(Error::Conflict("guardian owner process disappeared"))?;
            if current.guardian_process_id.as_deref() != Some(process.process_id.as_str())
                || current.guardian_store_generation != Some(process.store_generation)
            {
                return Err(Error::Conflict(
                    "guardian attempt belongs to another owner process incarnation",
                ));
            }
        } else if current.state != PolicyAdmissionState::Claimed {
            return Err(Error::Conflict(
                "deterministic policy decision is no longer claimable",
            ));
        }

        let state = match decision.decision {
            Decision::Allow => PolicyAdmissionState::Allowed,
            Decision::Deny => PolicyAdmissionState::Denied,
            Decision::Ask => PolicyAdmissionState::AskPending,
            Decision::Narrow => PolicyAdmissionState::Narrowed,
        };
        let mut next = next_fact(&current, state)?;
        next.decision_revision = Some(decision.revision);
        next.decision_sha256 = Some(decision_sha256.to_owned());
        next.decision_route = route_label(decision.route).map(str::to_owned);
        next.narrow_ref = decision.narrow_ref.map(str::to_owned);
        audit_fact(&tx, &current.request.subject.actor, &next)?;
        db(tx.commit())?;
        Ok(next)
    }

    /// Explicitly preserve uncertainty after an ambiguous paid attempt. This is
    /// never a retry authorization.
    pub fn policy_admission_mark_guardian_unknown(
        &self,
        session: &str,
        id: &str,
        note: &str,
    ) -> Result<PolicyAdmissionFact> {
        if note.trim().is_empty() || note.len() > 1_024 || note.chars().any(char::is_control) {
            return Err(Error::Denied("invalid guardian-unknown inspection note"));
        }
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current = fact_in(&tx, session, id)?;
        if current.state != PolicyAdmissionState::GuardianStarted {
            return Err(Error::Conflict(
                "only an unresolved started guardian attempt can become unknown",
            ));
        }
        let mut next = next_fact(&current, PolicyAdmissionState::GuardianUnknown)?;
        next.note = Some(note.to_owned());
        audit_fact(&tx, &current.request.subject.actor, &next)?;
        db(tx.commit())?;
        Ok(next)
    }

    /// Resolve one pre-reserved Ask decision. The approver must remain currently
    /// authorized for the Session and in the immutable admission set.
    pub fn policy_admission_decide(
        &self,
        approver: &PrincipalId,
        session: &str,
        id: &str,
        approve: bool,
    ) -> Result<PolicyAdmissionFact> {
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        context::authorize_tree(&tx, session, approver)?;
        let current = fact_in(&tx, session, id)?;

        if matches!(
            current.state,
            PolicyAdmissionState::AskApproved | PolicyAdmissionState::AskDenied
        ) {
            let same = current.approval_decided_by.as_deref() == Some(approver.as_str())
                && approve == (current.state == PolicyAdmissionState::AskApproved);
            return if same {
                Ok(current)
            } else {
                Err(Error::Conflict(
                    "policy approval was already decided differently",
                ))
            };
        }
        if current.state != PolicyAdmissionState::AskPending {
            return Err(Error::Conflict("policy admission is not awaiting approval"));
        }
        if !current
            .request
            .authorized_approvers
            .iter()
            .any(|candidate| candidate == approver.as_str())
        {
            return Err(Error::Denied(
                "principal is not an authorized approver for this admission",
            ));
        }
        if current
            .request
            .approval_expires_at
            .is_none_or(|expiry| now() >= expiry)
        {
            let mut expired = next_fact(&current, PolicyAdmissionState::AskExpired)?;
            expired.note = Some("approval expired before decision".into());
            audit_fact(&tx, approver, &expired)?;
            db(tx.commit())?;
            return Ok(expired);
        }
        let mut next = next_fact(
            &current,
            if approve {
                PolicyAdmissionState::AskApproved
            } else {
                PolicyAdmissionState::AskDenied
            },
        )?;
        next.approval_decided_by = Some(approver.as_str().to_owned());
        audit_fact(&tx, approver, &next)?;
        db(tx.commit())?;
        Ok(next)
    }

    pub(crate) fn policy_admission_consume_in(
        connection: &rusqlite::Connection,
        session: &str,
        id: &str,
        current_policy_revision_sha256: &str,
        current_authority_sha256: &str,
        selected_route: Option<Strategy>,
        authorized_narrow_ref: Option<&str>,
    ) -> Result<PolicyAdmissionFact> {
        if !digest(current_policy_revision_sha256) || !digest(current_authority_sha256) {
            return Err(Error::Denied("invalid current policy/authority digest"));
        }
        let current = fact_in(connection, session, id)?;
        if current.state == PolicyAdmissionState::Consumed {
            return Ok(current);
        }
        if current.compacted {
            return Err(Error::Storage(
                "non-consumed policy admission unexpectedly compacted",
            ));
        }
        let owner = crate::session_in(connection, session)?.owner;
        require_active_turn(connection, &owner, &current.request)?;
        if current.request.policy_revision_sha256 != current_policy_revision_sha256
            || current.request.authority_sha256 != current_authority_sha256
        {
            return Err(Error::Conflict(
                "policy/authority generation changed before admission consumption",
            ));
        }
        if current.decision_route.as_deref() != route_label(selected_route)
            && current.decision_route.is_some()
        {
            return Err(Error::Denied(
                "selected execution route differs from the admitted policy decision",
            ));
        }
        if current.state == PolicyAdmissionState::Narrowed {
            if current.narrow_ref.as_deref() != authorized_narrow_ref {
                return Err(Error::Denied(
                    "Narrow policy decision lacks the trusted host's exact revision proof",
                ));
            }
        } else if authorized_narrow_ref.is_some() {
            return Err(Error::Denied(
                "unexpected argument-revision proof for a non-Narrow decision",
            ));
        }
        if current.state == PolicyAdmissionState::AskApproved
            && current
                .request
                .approval_expires_at
                .is_some_and(|expiry| now() >= expiry)
        {
            let mut expired = next_fact(&current, PolicyAdmissionState::AskExpired)?;
            expired.note = Some("approved policy admission expired before consumption".into());
            audit_fact(connection, &current.request.subject.actor, &expired)?;
            return Ok(expired);
        }
        if !current.state.executable() {
            return Err(Error::Conflict(
                "policy admission has no executable decision",
            ));
        }
        let next = next_fact(&current, PolicyAdmissionState::Consumed)?;
        audit_fact(connection, &current.request.subject.actor, &next)?;
        Ok(next)
    }

    pub(crate) fn policy_admission_compact_consumed_in(
        connection: &rusqlite::Connection,
        session: &str,
        id: &str,
    ) -> Result<()> {
        let current = fact_in(connection, session, id)?;
        if current.compacted {
            return Ok(());
        }
        if current.state != PolicyAdmissionState::Consumed {
            return Err(Error::Conflict(
                "only consumed policy admissions may compact",
            ));
        }
        compact_consumed_current(connection, &current)
    }

    /// Best-effort replay-detail compaction after the effect has crossed a
    /// durable recovery boundary. Failure leaves the full replay row intact.
    pub fn policy_admission_compact_consumed(&self, session: &str, id: &str) -> Result<()> {
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        Self::policy_admission_compact_consumed_in(&tx, session, id)?;
        db(tx.commit())?;
        Ok(())
    }

    /// Revalidate captured policy/authority generations immediately before the
    /// owning executor may consume an Allow/Narrow/Approved decision.
    pub fn policy_admission_consume(
        &self,
        session: &str,
        id: &str,
        current_policy_revision_sha256: &str,
        current_authority_sha256: &str,
        selected_route: Option<Strategy>,
        authorized_narrow_ref: Option<&str>,
    ) -> Result<PolicyAdmissionFact> {
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let fact = Self::policy_admission_consume_in(
            &tx,
            session,
            id,
            current_policy_revision_sha256,
            current_authority_sha256,
            selected_route,
            authorized_narrow_ref,
        )?;
        db(tx.commit())?;
        Ok(fact)
    }
}
