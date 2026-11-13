//! Replay-stable durable admission facts for unified host policy decisions.
//!
//! This module owns no policy configuration, ToolPolicy grant, provider, Agent
//! runner or effect executor. It derives immutable admission/guardian/approval
//! facts from the existing Session audit and current writer/ACL authority.

use crate::{
    audit, branch_in, context, db, now, principal_is_session_owner, workspace_writer, Error,
    PrincipalId, Result, SessionStore,
};
use aw_agent_core::{
    AuxiliaryInput, AuxiliaryJournal, AuxiliaryResult, InvocationContext, InvocationIdentity,
    InvocationPurpose, JournalFuture, Outcome,
};
use aw_policy::{Decision, PolicyDecision, Strategy};
use rusqlite::{params, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;

const ACTION: &str = "policy.admission";
const MAX_EVENTS: usize = 4_096;
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
pub struct PolicyAdmissionRequest {
    pub session_id: String,
    pub branch_id: String,
    pub writer_epoch: u64,
    pub turn_id: u64,
    pub call_id: String,
    pub canonical_target: String,
    pub request_sha256: String,
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
        || !digest(&request.policy_revision_sha256)
        || !digest(&request.authority_sha256)
        || !(request.candidate_local || request.candidate_delegate_agent)
        || request.authorized_approvers.len() > MAX_APPROVERS
    {
        return Err(Error::Denied("invalid policy admission request"));
    }

    if request.guardian_required {
        let identity = request.guardian_identity.as_ref().ok_or(Error::Denied(
            "guardian policy admission needs a pinned invocation identity",
        ))?;
        if identity.grant_revision == 0
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

fn events(c: &rusqlite::Connection, session: &str) -> Result<Vec<PolicyAdmissionFact>> {
    let mut query = db(c.prepare(
        "SELECT data_json FROM audit
         WHERE session_id=?1 AND action=?2
         ORDER BY seq LIMIT 4097",
    ))?;
    let rows = db(query.query_map(params![session, ACTION], |row| row.get::<_, String>(0)))?;
    let encoded = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if encoded.len() > MAX_EVENTS {
        return Err(Error::Limit(
            "policy admission audit requires explicit inspection/rotation",
        ));
    }
    encoded
        .into_iter()
        .map(|value| crate::decode::<PolicyAdmissionFact>(&value))
        .collect()
}

fn latest<'a>(facts: &'a [PolicyAdmissionFact], id: &str) -> Option<&'a PolicyAdmissionFact> {
    facts.iter().rev().find(|fact| fact.id == id)
}

pub(crate) fn fact_in(
    connection: &rusqlite::Connection,
    session: &str,
    id: &str,
) -> Result<PolicyAdmissionFact> {
    latest(&events(connection, session)?, id)
        .cloned()
        .ok_or(Error::Denied("policy admission fact is unavailable"))
}

fn audit_fact(
    c: &rusqlite::Connection,
    actor: &PrincipalId,
    fact: &PolicyAdmissionFact,
) -> Result<()> {
    audit(
        c,
        &fact.request.session_id,
        actor,
        ACTION,
        serde_json::to_value(fact)
            .map_err(|_| Error::Storage("cannot serialize policy admission fact"))?,
    )
}

fn require_owner_writer(
    c: &rusqlite::Connection,
    owner: &PrincipalId,
    request: &PolicyAdmissionRequest,
) -> Result<()> {
    if !principal_is_session_owner(c, &request.session_id, owner)? {
        return Err(Error::Denied(
            "policy admission belongs to the durable Session owner",
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
                &self.actor,
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
    /// Claim the original replay identity and bounded guardian/approval slots
    /// before any auxiliary model call. Exact redelivery returns the same fact.
    pub fn policy_admission_claim(
        &self,
        owner: &PrincipalId,
        request: PolicyAdmissionRequest,
    ) -> Result<PolicyAdmissionFact> {
        validate_request(&request)?;
        if request
            .guardian_identity
            .as_ref()
            .is_some_and(|identity| identity.actor != *owner)
        {
            return Err(Error::Denied(
                "policy guardian identity actor differs from Session owner",
            ));
        }
        let id = admission_id(owner, &request);
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        require_active_turn(&tx, owner, &request)?;
        let all = events(&tx, &request.session_id)?;
        if let Some(existing) = latest(&all, &id) {
            if existing.request != request {
                return Err(Error::Conflict(
                    "policy replay identity was reused with changed request/policy/authority",
                ));
            }
            db(tx.commit())?;
            return Ok(existing.clone());
        }
        let mut unique = BTreeMap::<&str, ()>::new();
        for fact in &all {
            if fact.request.branch_id == request.branch_id
                && fact.request.turn_id == request.turn_id
            {
                unique.insert(fact.id.as_str(), ());
            }
        }
        if unique.len() >= MAX_ADMISSIONS_PER_TURN {
            return Err(Error::Limit(
                "policy admission count exceeds the bounded active-Turn budget",
            ));
        }
        let fact = PolicyAdmissionFact {
            id,
            request,
            state: PolicyAdmissionState::Claimed,
            revision: 1,
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
        audit_fact(&tx, owner, &fact)?;
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
        latest(&events(&connection, session)?, id)
            .cloned()
            .ok_or(Error::Denied("policy admission fact is unavailable"))
    }

    /// Commit the single reserved paid guardian attempt before dispatch. A
    /// repeated call after this point fails rather than paying again.
    pub fn policy_admission_start_guardian(
        &self,
        owner: &PrincipalId,
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
        let facts = events(&tx, session)?;
        let current = latest(&facts, id)
            .cloned()
            .ok_or(Error::Denied("policy admission fact is unavailable"))?;
        require_active_turn(&tx, owner, &current.request)?;
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
        audit_fact(&tx, owner, &next)?;
        db(tx.commit())?;
        Ok(next)
    }

    /// Factual receipt for the one started guardian attempt. Settlement is
    /// allowed after authority revocation because it records already-attempted
    /// provider work; it never authorizes another attempt.
    pub fn policy_admission_finish_guardian(
        &self,
        owner: &PrincipalId,
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
        let facts = events(&tx, session)?;
        let current = latest(&facts, id)
            .cloned()
            .ok_or(Error::Denied("policy admission fact is unavailable"))?;
        if current.state != PolicyAdmissionState::GuardianStarted {
            return Err(Error::Conflict(
                "policy guardian result has no exact started attempt",
            ));
        }
        if current
            .request
            .guardian_identity
            .as_ref()
            .is_none_or(|identity| identity.actor != *owner)
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
        audit_fact(&tx, owner, &next)?;
        db(tx.commit())?;
        Ok(next)
    }

    /// Persist a deterministic or guardian decision. The decision is evidence
    /// inside the original replay claim; it never grants missing authority.
    pub fn policy_admission_record_decision(
        &self,
        owner: &PrincipalId,
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
        let facts = events(&tx, session)?;
        let current = latest(&facts, id)
            .cloned()
            .ok_or(Error::Denied("policy admission fact is unavailable"))?;
        require_active_turn(&tx, owner, &current.request)?;
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
        audit_fact(&tx, owner, &next)?;
        db(tx.commit())?;
        Ok(next)
    }

    /// Explicitly preserve uncertainty after an ambiguous paid attempt. This is
    /// never a retry authorization.
    pub fn policy_admission_mark_guardian_unknown(
        &self,
        owner: &PrincipalId,
        session: &str,
        id: &str,
        note: &str,
    ) -> Result<PolicyAdmissionFact> {
        if note.trim().is_empty() || note.len() > 1_024 || note.chars().any(char::is_control) {
            return Err(Error::Denied("invalid guardian-unknown inspection note"));
        }
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let facts = events(&tx, session)?;
        let current = latest(&facts, id)
            .cloned()
            .ok_or(Error::Denied("policy admission fact is unavailable"))?;
        require_owner_writer(&tx, owner, &current.request)?;
        if current.state != PolicyAdmissionState::GuardianStarted {
            return Err(Error::Conflict(
                "only an unresolved started guardian attempt can become unknown",
            ));
        }
        let mut next = next_fact(&current, PolicyAdmissionState::GuardianUnknown)?;
        next.note = Some(note.to_owned());
        audit_fact(&tx, owner, &next)?;
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
        let facts = events(&tx, session)?;
        let current = latest(&facts, id)
            .cloned()
            .ok_or(Error::Denied("policy admission fact is unavailable"))?;

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

    /// Revalidate captured policy/authority generations immediately before the
    /// owning executor may consume an Allow/Narrow/Approved decision.
    pub fn policy_admission_consume(
        &self,
        owner: &PrincipalId,
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
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let facts = events(&tx, session)?;
        let current = latest(&facts, id)
            .cloned()
            .ok_or(Error::Denied("policy admission fact is unavailable"))?;
        require_active_turn(&tx, owner, &current.request)?;
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
        if current.state == PolicyAdmissionState::Consumed {
            db(tx.commit())?;
            return Ok(current);
        }
        if current.state == PolicyAdmissionState::AskApproved
            && current
                .request
                .approval_expires_at
                .is_some_and(|expiry| now() >= expiry)
        {
            let mut expired = next_fact(&current, PolicyAdmissionState::AskExpired)?;
            expired.note = Some("approved policy admission expired before consumption".into());
            audit_fact(&tx, owner, &expired)?;
            db(tx.commit())?;
            return Ok(expired);
        }
        if !current.state.executable() {
            return Err(Error::Conflict(
                "policy admission has no executable decision",
            ));
        }
        let next = next_fact(&current, PolicyAdmissionState::Consumed)?;
        audit_fact(&tx, owner, &next)?;
        db(tx.commit())?;
        Ok(next)
    }
}
