//! AW-P6-8C: bounded review UI/SDK projection and *explicit current-owner*
//! human decision facade over the original Phase50 guardian `review_human_approvals`.
//! No new approval store, Model/Hook-granted approver, release shortcut,
//! provider executor, browser cookie-to-Session inference, or durable replay.
//! The existing authenticated `aw review list/show/approve/deny` CLI owns its
//! original human user principal; this host adapter narrows further to one
//! currently bound Runtime Actor/Session/branch and immutable source digest.
use crate::{actor_owner::Service, Error, Result};
use aw_store::{ReviewApprovalInfo, SessionStore};
use serde::{Deserialize, Serialize};

pub const MAX_VISIBLE_REVIEWS: usize = 32;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuardianApprovalCard {
    pub review_id: String,
    pub session_id: String,
    pub branch_id: String,
    pub turn_id: u64,
    pub evidence_sha256: String,
    pub guardian_reason: String,
    pub guardian_summary: String,
    pub state: String,
    pub requested_at: i64,
}
fn sanitized(text: &str) -> String {
    crate::presentation::safe_text(text)
        .chars()
        .take(256)
        .filter(|c| !c.is_control())
        .collect()
}
/// This is presentation data; the guardian verdict is untrusted evidence,
/// NOT a policy grant. Never render raw policy/evidence/Account/Tool content.
fn card(status: &aw_store::ReviewStatusInfo) -> Result<GuardianApprovalCard> {
    let op = &status.operation;
    let approval = status.approval.as_ref().ok_or(Error::Denied(
        "guardian review is not requesting human approval",
    ))?;
    if op.state != "waiting_human" || approval.state != "waiting" || op.parent_turn_id == 0 {
        return Err(Error::Denied(
            "guardian review is not currently awaiting this human",
        ));
    }
    let verdict = op.verdict.as_ref().ok_or(Error::Storage(
        "human-review guardian has no immutable verdict",
    ))?;
    Ok(GuardianApprovalCard {
        review_id: op.id.clone(),
        session_id: op.parent_session_id.clone(),
        branch_id: op.parent_branch_id.clone(),
        turn_id: op.parent_turn_id,
        evidence_sha256: op.source_sha256.clone(),
        guardian_reason: sanitized(&verdict.reason_code),
        guardian_summary: sanitized(&verdict.summary),
        state: approval.state.clone(),
        requested_at: approval.created_at,
    })
}
fn binding(owner: &Service) -> Result<(&aw_agent_core::PrincipalId, &str, &str)> {
    owner.verify_rpc_state()?;
    let (principal, session, branch, _, _) = owner.bound_peer_scope();
    Ok((principal, session, branch))
}
/// Native authenticated owner/CLI presentation of only one exact selected
/// Session and canonical Branch. This does NOT create the Actor owner runtime:
/// it reuses original local user identity and review authorization in Store.
/// Decisions still require original `aw review approve/deny` or the stricter
/// generation-bound host `decide` path; a raw browser cannot invoke either.
pub fn pending_for_user(
    store: &SessionStore,
    principal: &aw_agent_core::PrincipalId,
    session: &str,
    branch: &str,
) -> Result<Vec<GuardianApprovalCard>> {
    store.session(principal, session)?;
    if !store
        .branches(principal, session)?
        .iter()
        .any(|b| b.id == branch)
    {
        return Err(Error::Denied(
            "review approval list requires canonical owner-owned Session branch",
        ));
    }
    let mut cards = Vec::new();
    for status in store.reviews(principal)? {
        if status.operation.parent_session_id != session
            || status.operation.parent_branch_id != branch
            || status.operation.state != "waiting_human"
            || status
                .approval
                .as_ref()
                .is_none_or(|a| a.state != "waiting")
        {
            continue;
        }
        cards.push(card(&status)?);
        if cards.len() > MAX_VISIBLE_REVIEWS {
            return Err(Error::Limit(
                "too many pending guardian approvals; inspect exact review IDs",
            ));
        }
    }
    Ok(cards)
}

pub fn pending(store: &SessionStore, owner: &Service) -> Result<Vec<GuardianApprovalCard>> {
    let (principal, session, branch) = binding(owner)?;
    let cards = pending_for_user(store, principal, session, branch)?;
    owner.verify_rpc_state()?;
    Ok(cards)
}
pub fn inspect(
    store: &SessionStore,
    owner: &Service,
    review_id: &str,
) -> Result<GuardianApprovalCard> {
    let (principal, session, branch) = binding(owner)?;
    let status = store.review_status(principal, review_id)?;
    if status.operation.parent_session_id != session || status.operation.parent_branch_id != branch
    {
        return Err(Error::Denied(
            "guardian review belongs to another parent branch or Session",
        ));
    }
    let result = card(&status)?;
    owner.verify_rpc_state()?;
    Ok(result)
}
/// Reuse the ORIGINAL guardian's atomic user-authorized `review_decide_human`.
/// The current user must compare the exact immutable source SHA that was
/// visually reviewed. A decision is not evidence release or permission to run
/// Process/Tool/Model; `review_release` remains a separate original gate.
pub fn decide(
    store: &SessionStore,
    owner: &Service,
    review_id: &str,
    expected_evidence_sha256: &str,
    approve: bool,
) -> Result<ReviewApprovalInfo> {
    if expected_evidence_sha256.len() != 64
        || !expected_evidence_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error::Denied(
            "guardian decision requires exact current source SHA256",
        ));
    }
    let observed = inspect(store, owner, review_id)?;
    if observed.evidence_sha256 != expected_evidence_sha256 {
        return Err(Error::Conflict(
            "reviewed guardian source digest changed; reload approval",
        ));
    }
    let (principal, _, _) = binding(owner)?;
    store.review_decide_human(principal, review_id, approve)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aw_p6_8c_guardian_view_does_not_promote_verdict_or_tool_policy() {
        // Typed JSON views cannot contain arbitrary provider routes, tools or
        // remote grant fields; the original operation carries those privately.
        let view = GuardianApprovalCard {
            review_id: "rev_1".into(),
            session_id: "s_1".into(),
            branch_id: "b_1".into(),
            turn_id: 2,
            evidence_sha256: "a".repeat(64),
            guardian_reason: sanitized("needs_human\u{202e}spoof"),
            guardian_summary: sanitized("untrusted\nmodel suggestion"),
            state: "waiting".into(),
            requested_at: 10,
        };
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 9);
        assert!(!json.to_string().contains("\u{202e}"));
        assert!(!json.to_string().contains("\n"));
        assert!(
            serde_json::from_value::<GuardianApprovalCard>(serde_json::json!({
                "review_id":"rev_1","session_id":"s_1","branch_id":"b_1","turn_id":2,
                "evidence_sha256":"a".repeat(64),"guardian_reason":"review",
                "guardian_summary":"human","state":"waiting","requested_at":10,
                "tool_grant":"bash"
            }))
            .is_err()
        );
    }
}
