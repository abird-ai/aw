use super::*;
use aw_agent_core::{
    InvocationContext, InvocationIdentity, InvocationPurpose, ReviewInput, ReviewJournal,
    ReviewResult,
};
use aw_pipeline::{
    review_verdict_sha256, text_sha256, ReviewDecision, ReviewSourceKind, ReviewVerdict,
};
use aw_routing::{target as routing_target, RoutingMode, RoutingStrategyDefinition};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewOperationInfo {
    pub id: String,
    pub root_id: String,
    pub parent_session_id: String,
    pub parent_branch_id: String,
    pub parent_turn_id: u64,
    pub source_kind: ReviewSourceKind,
    pub source_id: String,
    pub source_sha256: String,
    pub strategy_revision_id: String,
    pub parent_identity: InvocationIdentity,
    pub guardian_identity: InvocationIdentity,
    pub input_sha256: String,
    pub policy_sha256: String,
    pub policy: String,
    pub objective_sha256: String,
    pub state: String,
    pub verdict_sha256: Option<String>,
    pub verdict: Option<ReviewVerdict>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub settled_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewApprovalInfo {
    pub review_operation_id: String,
    pub state: String,
    pub requested_by_user_id: String,
    pub decided_by_user_id: Option<String>,
    pub created_at: i64,
    pub decided_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewReleaseInfo {
    pub review_operation_id: String,
    pub source_sha256: String,
    pub basis: String,
    pub released_by_user_id: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewStatusInfo {
    pub operation: ReviewOperationInfo,
    pub approval: Option<ReviewApprovalInfo>,
    pub release: Option<ReviewReleaseInfo>,
}

#[derive(Clone, Debug)]
pub struct ReleasedReviewEvidence {
    pub operation: ReviewOperationInfo,
    pub release: ReviewReleaseInfo,
    pub evidence: String,
}

struct ReviewSource {
    root_id: String,
    parent_session_id: String,
    parent_branch_id: String,
    parent_turn_id: u64,
    joined_text: String,
    joined_sha256: String,
}

fn bounded_error(value: Option<&str>) -> Option<String> {
    value.map(|value| value.chars().take(4096).collect())
}

fn review_kind(value: &str) -> Result<ReviewSourceKind> {
    match value {
        "team" => Ok(ReviewSourceKind::Team),
        "planner" => Ok(ReviewSourceKind::Planner),
        _ => Err(Error::Storage("stored review source kind is invalid")),
    }
}

fn review_operation_row(c: &Connection, id: &str) -> Result<ReviewOperationInfo> {
    let row: Option<(
        String,
        String,
        String,
        u64,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        i64,
        Option<i64>,
        Option<i64>,
        Option<String>,
    )> = db(c
        .query_row(
            "SELECT root_id,parent_session_id,parent_branch_id,parent_turn_id,
                    source_kind,source_id,source_sha256,strategy_revision_id,
                    parent_identity_json,guardian_identity_json,input_sha256,policy_sha256,policy_text,
                    objective_sha256,state,verdict_sha256,verdict_json,
                    created_at,started_at,settled_at,error
             FROM review_operations WHERE id=?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                    r.get(11)?,
                    r.get(12)?,
                    r.get(13)?,
                    r.get(14)?,
                    r.get(15)?,
                    r.get(16)?,
                    r.get(17)?,
                    r.get(18)?,
                    r.get(19)?,
                    r.get(20)?,
                ))
            },
        )
        .optional())?;
    let Some((
        root_id,
        parent_session_id,
        parent_branch_id,
        parent_turn_id,
        source_kind,
        source_id,
        source_sha256,
        strategy_revision_id,
        parent_identity_json,
        guardian_identity_json,
        input_sha256,
        policy_sha256,
        policy,
        objective_sha256,
        state,
        verdict_sha256,
        verdict_json,
        created_at,
        started_at,
        settled_at,
        error,
    )) = row
    else {
        return Err(Error::Denied("review operation unavailable"));
    };
    Ok(ReviewOperationInfo {
        id: id.to_owned(),
        root_id,
        parent_session_id,
        parent_branch_id,
        parent_turn_id,
        source_kind: review_kind(&source_kind)?,
        source_id,
        source_sha256,
        strategy_revision_id,
        parent_identity: decode(&parent_identity_json)?,
        guardian_identity: decode(&guardian_identity_json)?,
        input_sha256,
        policy_sha256,
        policy,
        objective_sha256,
        state,
        verdict_sha256,
        verdict: verdict_json
            .as_deref()
            .map(decode::<ReviewVerdict>)
            .transpose()?,
        created_at,
        started_at,
        settled_at,
        error,
    })
}

fn review_approval_row(c: &Connection, id: &str) -> Result<Option<ReviewApprovalInfo>> {
    let row: Option<(String, String, Option<String>, i64, Option<i64>)> = db(c
        .query_row(
            "SELECT state,requested_by_user_id,decided_by_user_id,created_at,decided_at
             FROM review_human_approvals WHERE review_operation_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional())?;
    Ok(row.map(
        |(state, requested_by_user_id, decided_by_user_id, created_at, decided_at)| {
            ReviewApprovalInfo {
                review_operation_id: id.to_owned(),
                state,
                requested_by_user_id,
                decided_by_user_id,
                created_at,
                decided_at,
            }
        },
    ))
}

fn review_release_row(c: &Connection, id: &str) -> Result<Option<ReviewReleaseInfo>> {
    let row: Option<(String, String, String, i64)> = db(c
        .query_row(
            "SELECT source_sha256,basis,released_by_user_id,created_at
             FROM review_release_receipts WHERE review_operation_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional())?;
    Ok(row.map(
        |(source_sha256, basis, released_by_user_id, created_at)| ReviewReleaseInfo {
            review_operation_id: id.to_owned(),
            source_sha256,
            basis,
            released_by_user_id,
            created_at,
        },
    ))
}

fn review_source_in(
    c: &Connection,
    kind: ReviewSourceKind,
    source_id: &str,
) -> Result<ReviewSource> {
    match kind {
        ReviewSourceKind::Team => {
            let row: Option<(String, String, u64, String)> = db(c
                .query_row(
                    "SELECT parent_session_id,parent_branch_id,parent_turn_id,state
                     FROM team_runs WHERE id=?1",
                    [source_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional())?;
            let Some((parent_session_id, parent_branch_id, parent_turn_id, state)) = row else {
                return Err(Error::Denied("team review source unavailable"));
            };
            if state != "joined" {
                return Err(Error::Conflict("team review source is not joined"));
            }
            let root_id: Option<String> = db(c
                .query_row(
                    "SELECT id FROM delegation_roots
                     WHERE root_session_id=?1 AND root_branch_id=?2 AND root_turn_id=?3",
                    params![parent_session_id, parent_branch_id, parent_turn_id],
                    |r| r.get(0),
                )
                .optional())?;
            let root_id = root_id.ok_or(Error::Storage(
                "joined team review source lost its delegation root",
            ))?;
            let receipt = SessionStore::reconstruct_team_join_in(c, source_id)?;
            Ok(ReviewSource {
                root_id,
                parent_session_id,
                parent_branch_id,
                parent_turn_id,
                joined_text: receipt.joined_text,
                joined_sha256: receipt.joined_sha256,
            })
        }
        ReviewSourceKind::Planner => {
            let row: Option<(String, String, String, u64, String)> = db(c
                .query_row(
                    "SELECT root_id,parent_session_id,parent_branch_id,parent_turn_id,state
                     FROM planner_operations WHERE id=?1",
                    [source_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .optional())?;
            let Some((root_id, parent_session_id, parent_branch_id, parent_turn_id, state)) = row
            else {
                return Err(Error::Denied("planner review source unavailable"));
            };
            if state != "joined" {
                return Err(Error::Conflict("planner review source is not joined"));
            }
            let receipt = SessionStore::reconstruct_planner_join_in(c, source_id)?;
            Ok(ReviewSource {
                root_id,
                parent_session_id,
                parent_branch_id,
                parent_turn_id,
                joined_text: receipt.joined_text,
                joined_sha256: receipt.joined_sha256,
            })
        }
    }
}

fn validate_guardian_strategy(
    c: &Connection,
    owner_user_id: &str,
    revision_id: &str,
    identity: &InvocationIdentity,
) -> Result<RoutingStrategyDefinition> {
    let row: Option<(String, String, String)> = db(c
        .query_row(
            "SELECT r.definition_json,r.created_by_user_id,s.owner_user_id
             FROM routing_strategy_revisions r
             JOIN routing_strategies s ON s.id=r.strategy_id
             WHERE r.revision_id=?1 AND s.status='active'",
            [revision_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional())?;
    let Some((body, creator, owner)) = row else {
        return Err(Error::Denied("guardian routing revision unavailable"));
    };
    if creator != owner_user_id || owner != owner_user_id {
        return Err(Error::Denied(
            "guardian routing revision belongs to another owner",
        ));
    }
    let definition: RoutingStrategyDefinition = decode(&body)?;
    aw_routing::validate_strategy(&definition).map_err(|error| Error::Config(error.to_string()))?;
    if definition.role != "guardian" || definition.mode != RoutingMode::Fixed {
        return Err(Error::Denied(
            "Phase 50 guardian strategy must be fixed with role=guardian",
        ));
    }
    let target_id = definition
        .fixed_target
        .as_deref()
        .ok_or(Error::Storage("fixed guardian strategy lost target"))?;
    let target =
        routing_target(&definition, target_id).map_err(|error| Error::Config(error.to_string()))?;
    if target.account_id != identity.account_id || target.model != identity.model {
        return Err(Error::Denied(
            "guardian working identity differs from fixed guardian target",
        ));
    }
    Ok(definition)
}

impl SessionStore {
    pub fn review_admit(
        &self,
        actor: &PrincipalId,
        parent_session: &str,
        parent_branch: &str,
        parent_turn_id: u64,
        source_id: &str,
        parent_identity: &InvocationIdentity,
        input: &ReviewInput,
        identity: &InvocationIdentity,
    ) -> Result<ReviewOperationInfo> {
        input.validate()?;
        if identity.actor != *actor || parent_identity.actor != *actor {
            return Err(Error::Denied("guardian/parent actor changed"));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = ensure_user_for_principal(&tx, actor)?;
        let source = review_source_in(&tx, input.evidence.source_kind, source_id)?;
        if source.parent_session_id != parent_session
            || source.parent_branch_id != parent_branch
            || source.parent_turn_id != parent_turn_id
            || source.joined_sha256 != input.evidence.source_sha256
            || source.joined_text != input.evidence.evidence
        {
            return Err(Error::Conflict(
                "guardian review evidence differs from the exact durable source join",
            ));
        }
        crate::subagents::require_active_parent_turn(
            &tx,
            actor,
            parent_session,
            parent_branch,
            parent_turn_id,
        )?;
        let _ = validate_guardian_strategy(&tx, &user, &input.strategy_revision_id, identity)?;
        let root = crate::subagents::delegation_root_row(&tx, &source.root_id)?;
        if root.state != "active" || now() >= root.deadline_at {
            return Err(Error::Limit("delegation root deadline is exhausted"));
        }
        match root.guardian_routing_revision_id.as_deref() {
            Some(revision) if revision != input.strategy_revision_id => {
                return Err(Error::Conflict(
                    "guardian routing revision changed inside the delegation root",
                ));
            }
            None => {
                db(tx.execute(
                    "UPDATE delegation_roots
                     SET guardian_routing_revision_id=?1,updated_at=?2
                     WHERE id=?3 AND guardian_routing_revision_id IS NULL",
                    params![input.strategy_revision_id, now(), source.root_id],
                ))?;
            }
            _ => {}
        }

        let existing: Option<String> = db(tx
            .query_row(
                "SELECT id FROM review_operations WHERE source_kind=?1 AND source_id=?2",
                params![input.evidence.source_kind.as_str(), source_id],
                |r| r.get(0),
            )
            .optional())?;
        if let Some(existing) = existing {
            let op = review_operation_row(&tx, &existing)?;
            if op.strategy_revision_id != input.strategy_revision_id
                || op.parent_identity != *parent_identity
                || op.guardian_identity != *identity
                || op.input_sha256 != input.digest()?
                || op.parent_session_id != parent_session
                || op.parent_branch_id != parent_branch
                || op.parent_turn_id != parent_turn_id
            {
                return Err(Error::Conflict(
                    "review source already has a different durable guardian admission",
                ));
            }
            db(tx.commit())?;
            return Ok(op);
        }

        let review_id = id("review")?;
        let at = now();
        db(tx.execute(
            "INSERT INTO review_operations(
                id,root_id,actor_user_id,parent_session_id,parent_branch_id,parent_turn_id,
                source_kind,source_id,source_sha256,strategy_revision_id,parent_identity_json,
                guardian_identity_json,input_sha256,policy_sha256,policy_text,objective_sha256,state,created_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,'admitted',?17)",
            params![
                review_id,
                source.root_id,
                user,
                parent_session,
                parent_branch,
                parent_turn_id,
                input.evidence.source_kind.as_str(),
                source_id,
                input.evidence.source_sha256,
                input.strategy_revision_id,
                json(parent_identity)?,
                json(identity)?,
                input.digest()?,
                text_sha256(&input.evidence.policy),
                input.evidence.policy,
                text_sha256(&input.evidence.objective),
                at,
            ],
        ))?;
        let out = review_operation_row(&tx, &review_id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn review_journal(
        &self,
        actor: PrincipalId,
        review_id: &str,
    ) -> Result<ReviewJournalHandle> {
        let c = self.connection()?;
        let op = review_operation_row(&c, review_id)?;
        if op.guardian_identity.actor != actor || op.state != "admitted" {
            return Err(Error::Conflict(
                "review operation is not awaiting a guardian attempt",
            ));
        }
        Ok(ReviewJournalHandle {
            store: self.clone(),
            actor,
            review_id: review_id.to_owned(),
        })
    }

    pub fn review_settle_unstarted(
        &self,
        actor: &PrincipalId,
        review_id: &str,
        cancelled: bool,
        error: &str,
    ) -> Result<()> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let op = review_operation_row(&tx, review_id)?;
        let user = ensure_user_for_principal(&tx, actor)?;
        if op.guardian_identity.actor != *actor
            || !principal_is_session_owner(&tx, &op.parent_session_id, actor)?
        {
            return Err(Error::Denied("review operation belongs to another owner"));
        }
        let state = if cancelled { "cancelled" } else { "failed" };
        let changed = db(tx.execute(
            "UPDATE review_operations SET state=?1,error=?2,settled_at=?3
             WHERE id=?4 AND actor_user_id=?5 AND state='admitted'",
            params![state, bounded_error(Some(error)), now(), review_id, user],
        ))?;
        if changed == 0 && op.state != state {
            return Err(Error::Conflict(
                "review operation already entered guardian provider intent",
            ));
        }
        db(tx.commit())?;
        Ok(())
    }

    pub fn review_operation(
        &self,
        actor: &PrincipalId,
        review_id: &str,
    ) -> Result<ReviewOperationInfo> {
        let c = self.connection()?;
        let op = review_operation_row(&c, review_id)?;
        if !principal_is_session_owner(&c, &op.parent_session_id, actor)? {
            return Err(Error::Denied("review operation belongs to another owner"));
        }
        Ok(op)
    }

    pub fn reviews(&self, actor: &PrincipalId) -> Result<Vec<ReviewStatusInfo>> {
        let c = self.connection()?;
        let user = ensure_user_for_principal(&c, actor)?;
        let mut q = db(c.prepare(
            "SELECT id FROM review_operations
             WHERE actor_user_id=?1 ORDER BY created_at DESC,id DESC LIMIT 1024",
        ))?;
        let rows = db(q.query_map([user], |r| r.get::<_, String>(0)))?;
        let ids = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        ids.into_iter()
            .map(|id| {
                let operation = review_operation_row(&c, &id)?;
                Ok(ReviewStatusInfo {
                    approval: review_approval_row(&c, &id)?,
                    release: review_release_row(&c, &id)?,
                    operation,
                })
            })
            .collect()
    }

    pub fn review_status(&self, actor: &PrincipalId, review_id: &str) -> Result<ReviewStatusInfo> {
        let c = self.connection()?;
        let operation = review_operation_row(&c, review_id)?;
        if !principal_is_session_owner(&c, &operation.parent_session_id, actor)? {
            return Err(Error::Denied("review operation belongs to another owner"));
        }
        Ok(ReviewStatusInfo {
            approval: review_approval_row(&c, review_id)?,
            release: review_release_row(&c, review_id)?,
            operation,
        })
    }

    pub fn review_decide_human(
        &self,
        actor: &PrincipalId,
        review_id: &str,
        approve: bool,
    ) -> Result<ReviewApprovalInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let op = review_operation_row(&tx, review_id)?;
        if op.state != "waiting_human"
            || !principal_is_session_owner(&tx, &op.parent_session_id, actor)?
        {
            return Err(Error::Denied(
                "review is not awaiting this owner's explicit approval",
            ));
        }
        let user = ensure_user_for_principal(&tx, actor)?;
        let approval = review_approval_row(&tx, review_id)?
            .ok_or(Error::Storage("waiting review lost human approval row"))?;
        if approval.requested_by_user_id != user || approval.state != "waiting" {
            return Err(Error::Conflict(
                "review human approval is no longer waiting",
            ));
        }
        let state = if approve { "approved" } else { "denied" };
        let changed = db(tx.execute(
            "UPDATE review_human_approvals
             SET state=?1,decided_by_user_id=?2,decided_at=?3
             WHERE review_operation_id=?4 AND state='waiting'",
            params![state, user, now(), review_id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("review human approval decision raced"));
        }
        let out = review_approval_row(&tx, review_id)?
            .ok_or(Error::Storage("decided review approval disappeared"))?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn review_release(
        &self,
        actor: &PrincipalId,
        review_id: &str,
        current_session: &str,
        current_branch: &str,
        current_identity: &InvocationIdentity,
    ) -> Result<ReleasedReviewEvidence> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let op = review_operation_row(&tx, review_id)?;
        if !principal_is_session_owner(&tx, &op.parent_session_id, actor)?
            || op.parent_session_id != current_session
            || op.parent_branch_id != current_branch
            || op.parent_identity != *current_identity
        {
            return Err(Error::Denied(
                "review release belongs to another parent Agent Session/branch/provider identity",
            ));
        }
        let user = ensure_user_for_principal(&tx, actor)?;
        let basis = if op.state == "pass" {
            "guardian_pass"
        } else if op.state == "waiting_human" {
            let approval = review_approval_row(&tx, review_id)?
                .ok_or(Error::Storage("waiting review lost human approval row"))?;
            if approval.requested_by_user_id != user || approval.state != "approved" {
                return Err(Error::Denied(
                    "review evidence requires explicit human approval before release",
                ));
            }
            "human_approved"
        } else {
            return Err(Error::Denied(
                "guardian review did not authorize evidence release",
            ));
        };
        let source = review_source_in(&tx, op.source_kind, &op.source_id)?;
        if source.root_id != op.root_id
            || source.parent_session_id != op.parent_session_id
            || source.parent_branch_id != op.parent_branch_id
            || source.parent_turn_id != op.parent_turn_id
            || source.joined_sha256 != op.source_sha256
        {
            return Err(Error::Conflict(
                "review source changed from the immutable reviewed snapshot",
            ));
        }
        let release = if let Some(existing) = review_release_row(&tx, review_id)? {
            if existing.source_sha256 != op.source_sha256
                || existing.basis != basis
                || existing.released_by_user_id != user
            {
                return Err(Error::Conflict(
                    "existing review release receipt differs from current authority",
                ));
            }
            existing
        } else {
            let at = now();
            db(tx.execute(
                "INSERT INTO review_release_receipts(
                    review_operation_id,source_sha256,basis,released_by_user_id,created_at
                 ) VALUES(?1,?2,?3,?4,?5)",
                params![review_id, op.source_sha256, basis, user, at],
            ))?;
            ReviewReleaseInfo {
                review_operation_id: review_id.to_owned(),
                source_sha256: op.source_sha256.clone(),
                basis: basis.to_owned(),
                released_by_user_id: user,
                created_at: at,
            }
        };
        let operation = review_operation_row(&tx, review_id)?;
        db(tx.commit())?;
        Ok(ReleasedReviewEvidence {
            operation,
            release,
            evidence: source.joined_text,
        })
    }

    pub(crate) fn recover_reviews_after_owner_restart(&self, c: &Connection) -> Result<()> {
        if self.trusted_local_direct() {
            return Ok(());
        }
        let at = now();
        db(c.execute(
            "UPDATE review_operations
             SET state='interrupted',settled_at=?1,
                 error='owner restarted before guardian provider intent; no automatic replay'
             WHERE state='admitted'",
            [at],
        ))?;
        db(c.execute(
            "UPDATE review_operations
             SET state='unknown',settled_at=?1,
                 error='owner restarted after guardian provider intent; no automatic replay'
             WHERE state='started'",
            [at],
        ))?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct ReviewJournalHandle {
    store: SessionStore,
    actor: PrincipalId,
    review_id: String,
}

impl ReviewJournalHandle {
    fn authorize_in(
        &self,
        c: &Connection,
        input: &ReviewInput,
        identity: &InvocationIdentity,
        states: &[&str],
    ) -> Result<ReviewOperationInfo> {
        let user = ensure_user_for_principal(c, &self.actor)?;
        let op = review_operation_row(c, &self.review_id)?;
        if !states.contains(&op.state.as_str())
            || op.guardian_identity != *identity
            || identity.actor != self.actor
            || op.strategy_revision_id != input.strategy_revision_id
            || op.input_sha256 != input.digest()?
            || op.policy != input.evidence.policy
            || op.policy_sha256 != text_sha256(&input.evidence.policy)
            || op.objective_sha256 != text_sha256(&input.evidence.objective)
            || op.source_kind != input.evidence.source_kind
            || op.source_sha256 != input.evidence.source_sha256
        {
            return Err(Error::Denied("guardian review input/authority mismatch"));
        }
        let root = crate::subagents::delegation_root_row(c, &op.root_id)?;
        if root.guardian_routing_revision_id.as_deref() != Some(op.strategy_revision_id.as_str())
            || root.state != "active"
            || now() >= root.deadline_at
        {
            return Err(Error::Denied(
                "guardian delegation root is no longer active/authorized",
            ));
        }
        crate::subagents::require_active_parent_turn(
            c,
            &self.actor,
            &op.parent_session_id,
            &op.parent_branch_id,
            op.parent_turn_id,
        )?;
        let source = review_source_in(c, op.source_kind, &op.source_id)?;
        if source.root_id != op.root_id
            || source.joined_sha256 != op.source_sha256
            || source.joined_text != input.evidence.evidence
        {
            return Err(Error::Conflict(
                "guardian source evidence changed before review settlement",
            ));
        }
        let _ = validate_guardian_strategy(c, &user, &op.strategy_revision_id, identity)?;
        Ok(op)
    }
}

impl ReviewJournal for ReviewJournalHandle {
    fn authorize<'a>(
        &'a self,
        input: &'a ReviewInput,
        identity: &'a InvocationIdentity,
    ) -> aw_agent_core::JournalFuture<'a, ()> {
        Box::pin(async move {
            let c = self.store.connection()?;
            self.authorize_in(&c, input, identity, &["admitted", "started"])?;
            Ok(())
        })
    }

    fn begin<'a>(
        &'a self,
        input: &'a ReviewInput,
        identity: &'a InvocationIdentity,
    ) -> aw_agent_core::JournalFuture<'a, InvocationContext> {
        Box::pin(async move {
            let mut c = self.store.connection()?;
            let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let op = self.authorize_in(&tx, input, identity, &["admitted"])?;
            let root = crate::subagents::delegation_root_row(&tx, &op.root_id)?;
            if root.model_attempts_started >= root.limits.max_model_attempts {
                return Err(Error::Limit("delegation model-attempt limit reached"));
            }
            let at = now();
            let changed = db(tx.execute(
                "UPDATE delegation_roots
                 SET model_attempts_started=model_attempts_started+1,updated_at=?1
                 WHERE id=?2 AND model_attempts_started<max_model_attempts",
                params![at, op.root_id],
            ))?;
            if changed != 1 {
                return Err(Error::Conflict(
                    "guardian attempt raced root model-attempt limit",
                ));
            }
            db(tx.execute(
                "UPDATE review_operations SET state='started',started_at=?1
                 WHERE id=?2 AND state='admitted'",
                params![at, op.id],
            ))?;
            let ordinal = root.model_attempts_started.saturating_add(1);
            let context = InvocationContext {
                purpose: InvocationPurpose::Guardian {
                    operation_id: op.id.clone(),
                    input_sha256: op.input_sha256.clone(),
                    strategy_revision_id: op.strategy_revision_id.clone(),
                },
                scope: Some(aw_agent_core::InvocationScope {
                    session_id: op.parent_session_id.clone(),
                    branch_id: op.parent_branch_id.clone(),
                }),
                turn_id: op.parent_turn_id,
                ordinal,
                identity: identity.clone(),
            };
            db(tx.commit())?;
            Ok(context)
        })
    }

    fn finish<'a>(&'a self, result: &'a ReviewResult) -> aw_agent_core::JournalFuture<'a, ()> {
        Box::pin(async move {
            let mut c = self.store.connection()?;
            let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let op = review_operation_row(&tx, &self.review_id)?;
            if op.guardian_identity.actor != self.actor || op.state != "started" {
                return Err(Error::Conflict("guardian review is not settleable"));
            }
            let root = crate::subagents::delegation_root_row(&tx, &op.root_id)?;
            let usage = result.meta.as_ref().map(|meta| &meta.usage);
            let input_tokens = usage.and_then(|usage| usage.input_tokens);
            let output_tokens = usage.and_then(|usage| usage.output_tokens);
            let complete_usage = input_tokens.is_some() && output_tokens.is_some();
            let at = now();
            db(tx.execute(
                "UPDATE delegation_roots
                 SET model_attempts_finished=model_attempts_finished+1,
                     input_tokens_known=input_tokens_known+?1,
                     output_tokens_known=output_tokens_known+?2,
                     calls_without_complete_usage=calls_without_complete_usage+?3,
                     updated_at=?4
                 WHERE id=?5",
                params![
                    input_tokens.unwrap_or(0),
                    output_tokens.unwrap_or(0),
                    i64::from(!complete_usage),
                    at,
                    root.id,
                ],
            ))?;
            let metadata_json = result.meta.as_ref().map(json).transpose()?;
            if result.outcome == Outcome::Completed {
                let verdict = result
                    .verdict
                    .as_ref()
                    .ok_or(Error::Protocol("completed guardian result has no verdict"))?;
                let state = match verdict.decision {
                    ReviewDecision::Pass => "pass",
                    ReviewDecision::Deny => "deny",
                    ReviewDecision::NeedsHuman => "waiting_human",
                    ReviewDecision::RequireEvidence => "needs_evidence",
                };
                let verdict_json = json(verdict)?;
                let verdict_sha256 = review_verdict_sha256(verdict)
                    .map_err(|_| Error::Protocol("cannot digest guardian verdict"))?;
                db(tx.execute(
                    "UPDATE review_operations
                     SET state=?1,verdict_sha256=?2,verdict_json=?3,metadata_json=?4,
                         settled_at=?5,error=NULL
                     WHERE id=?6 AND state='started'",
                    params![
                        state,
                        verdict_sha256,
                        verdict_json,
                        metadata_json,
                        at,
                        op.id
                    ],
                ))?;
                if verdict.decision == ReviewDecision::NeedsHuman {
                    let user = ensure_user_for_principal(&tx, &self.actor)?;
                    db(tx.execute(
                        "INSERT INTO review_human_approvals(
                            review_operation_id,state,requested_by_user_id,created_at
                         ) VALUES(?1,'waiting',?2,?3)",
                        params![op.id, user, at],
                    ))?;
                }
            } else {
                let state = if result.outcome == Outcome::Cancelled {
                    "cancelled"
                } else {
                    "failed"
                };
                db(tx.execute(
                    "UPDATE review_operations
                     SET state=?1,metadata_json=?2,error=?3,settled_at=?4
                     WHERE id=?5 AND state='started'",
                    params![
                        state,
                        metadata_json,
                        bounded_error(result.error.as_deref()),
                        at,
                        op.id
                    ],
                ))?;
            }
            db(tx.commit())?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_recovery_marks_only_unsettled_guardian_effects_without_replay() {
        let store = SessionStore::in_memory().unwrap();
        let c = store.connection().unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();

        let digest = "0".repeat(64);
        let verdict = r#"{"decision":"needs_human","reason_code":"human_gate","summary":"Owner approval required."}"#;

        for (id, state, started_at, settled_at, verdict_sha, verdict_json) in [
            ("review_admitted", "admitted", None, None, None, None),
            ("review_started", "started", Some(10_i64), None, None, None),
            (
                "review_waiting",
                "waiting_human",
                Some(10_i64),
                Some(20_i64),
                Some(digest.as_str()),
                Some(verdict),
            ),
        ] {
            c.execute(
                "INSERT INTO review_operations(
                    id,root_id,actor_user_id,parent_session_id,parent_branch_id,parent_turn_id,
                    source_kind,source_id,source_sha256,strategy_revision_id,parent_identity_json,
                    guardian_identity_json,input_sha256,policy_sha256,policy_text,objective_sha256,
                    state,verdict_sha256,verdict_json,created_at,started_at,settled_at
                 ) VALUES(?1,'root','user','session','branch',1,'planner',?2,?3,'route','{}','{}',
                          ?3,?3,'policy',?3,?4,?5,?6,1,?7,?8)",
                params![
                    id,
                    format!("source_{id}"),
                    digest,
                    state,
                    verdict_sha,
                    verdict_json,
                    started_at,
                    settled_at,
                ],
            )
            .unwrap();
        }
        c.execute(
            "INSERT INTO review_human_approvals(
                review_operation_id,state,requested_by_user_id,created_at
             ) VALUES('review_waiting','waiting','user',20)",
            [],
        )
        .unwrap();

        store.recover_reviews_after_owner_restart(&c).unwrap();

        let state = |id: &str| -> (String, Option<String>) {
            c.query_row(
                "SELECT state,error FROM review_operations WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        let admitted = state("review_admitted");
        assert_eq!(admitted.0, "interrupted");
        assert!(admitted
            .1
            .as_deref()
            .is_some_and(|value| value.contains("no automatic replay")));

        let started = state("review_started");
        assert_eq!(started.0, "unknown");
        assert!(started
            .1
            .as_deref()
            .is_some_and(|value| value.contains("no automatic replay")));

        assert_eq!(state("review_waiting").0, "waiting_human");
        let approval: String = c
            .query_row(
                "SELECT state FROM review_human_approvals
                 WHERE review_operation_id='review_waiting'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(approval, "waiting");
    }
}
