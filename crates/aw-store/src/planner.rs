use super::*;
use aw_agent_core::{
    InvocationContext, InvocationIdentity, InvocationPurpose, PlannerInput, PlannerJournal,
    PlannerResult,
};
use aw_pipeline::{
    join_planner_results, planner_topological_order, PlannerEvidence, PlannerJoinReceipt,
    PlannerProposal, PlannerTaskResult, MAX_PLANNER_TASK_IMPORT_BYTES,
};
use aw_routing::{target as routing_target, RoutingMode, RoutingStrategyDefinition};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannerOperationInfo {
    pub id: String,
    pub root_id: String,
    pub parent_session_id: String,
    pub parent_branch_id: String,
    pub parent_turn_id: u64,
    pub strategy_revision_id: String,
    pub planner_identity: InvocationIdentity,
    pub state: String,
    pub input_sha256: String,
    pub proposal_sha256: Option<String>,
    pub proposal: Option<PlannerProposal>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub planned_at: Option<i64>,
    pub settled_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannerTaskRunInfo {
    pub planner_operation_id: String,
    pub task_id: String,
    pub ordinal: usize,
    pub role: String,
    pub dependencies: Vec<String>,
    pub state: String,
    pub subagent_id: Option<String>,
    pub child_session_id: Option<String>,
    pub answer_sha256: Option<String>,
    pub answer_truncated: bool,
    pub started_at: Option<i64>,
    pub settled_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannerJoinInfo {
    pub planner_operation_id: String,
    pub manifest: serde_json::Value,
    pub joined_sha256: String,
    pub created_at: i64,
}

fn bounded_error(value: Option<&str>) -> Option<String> {
    value.map(|value| value.chars().take(4096).collect())
}

fn planner_operation_row(
    c: &Connection,
    id: &str,
) -> Result<(PlannerOperationInfo, PlannerEvidence)> {
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
        Option<String>,
        Option<String>,
        i64,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<String>,
    )> = db(c
        .query_row(
            "SELECT root_id,parent_session_id,parent_branch_id,parent_turn_id,
                    strategy_revision_id,planner_identity_json,state,input_sha256,evidence_json,
                    COALESCE(proposal_sha256,''),proposal_json,metadata_json,created_at,started_at,
                    planned_at,settled_at,error
             FROM planner_operations WHERE id=?1",
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
                ))
            },
        )
        .optional())?;
    let Some((
        root_id,
        parent_session_id,
        parent_branch_id,
        parent_turn_id,
        strategy_revision_id,
        identity_json,
        state,
        input_sha256,
        evidence_json,
        proposal_sha256,
        proposal_json,
        _metadata_json,
        created_at,
        started_at,
        planned_at,
        settled_at,
        error,
    )) = row
    else {
        return Err(Error::Denied("planner operation unavailable"));
    };
    let evidence: PlannerEvidence = decode(&evidence_json)?;
    aw_pipeline::validate_planner_evidence(&evidence)
        .map_err(|error| Error::Config(error.to_string()))?;
    let proposal = proposal_json
        .as_deref()
        .map(decode::<PlannerProposal>)
        .transpose()?;
    Ok((
        PlannerOperationInfo {
            id: id.to_owned(),
            root_id,
            parent_session_id,
            parent_branch_id,
            parent_turn_id,
            strategy_revision_id,
            planner_identity: decode(&identity_json)?,
            state,
            input_sha256,
            proposal_sha256: (!proposal_sha256.is_empty()).then_some(proposal_sha256),
            proposal,
            created_at,
            started_at,
            planned_at,
            settled_at,
            error,
        },
        evidence,
    ))
}

fn planner_task_row(
    c: &Connection,
    operation_id: &str,
    task_id: &str,
) -> Result<PlannerTaskRunInfo> {
    let row: Option<(
        usize,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        Option<i64>,
        Option<i64>,
        Option<String>,
        String,
    )> = db(c
        .query_row(
            "SELECT ordinal,role,state,subagent_id,child_session_id,answer_sha256,
                    answer_truncated,started_at,settled_at,error,dependencies_json
             FROM planner_task_runs WHERE planner_operation_id=?1 AND task_id=?2",
            params![operation_id, task_id],
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
                ))
            },
        )
        .optional())?;
    let Some((
        ordinal,
        role,
        state,
        subagent_id,
        child_session_id,
        answer_sha256,
        answer_truncated,
        started_at,
        settled_at,
        error,
        dependencies_json,
    )) = row
    else {
        return Err(Error::Denied("planner task unavailable"));
    };
    Ok(PlannerTaskRunInfo {
        planner_operation_id: operation_id.to_owned(),
        task_id: task_id.to_owned(),
        ordinal,
        role,
        dependencies: decode(&dependencies_json)?,
        state,
        subagent_id,
        child_session_id,
        answer_sha256,
        answer_truncated: answer_truncated != 0,
        started_at,
        settled_at,
        error,
    })
}

fn planner_task_rows(c: &Connection, operation_id: &str) -> Result<Vec<PlannerTaskRunInfo>> {
    let mut q = db(c.prepare(
        "SELECT task_id FROM planner_task_runs WHERE planner_operation_id=?1 ORDER BY ordinal",
    ))?;
    let rows = db(q.query_map([operation_id], |r| r.get::<_, String>(0)))?;
    let ids = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    ids.into_iter()
        .map(|task_id| planner_task_row(c, operation_id, &task_id))
        .collect()
}

fn validate_planner_strategy(
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
        return Err(Error::Denied("planner routing revision unavailable"));
    };
    if creator != owner_user_id || owner != owner_user_id {
        return Err(Error::Denied(
            "planner routing revision belongs to another owner",
        ));
    }
    let definition: RoutingStrategyDefinition = decode(&body)?;
    aw_routing::validate_strategy(&definition).map_err(|error| Error::Config(error.to_string()))?;
    if definition.role != "planner" || definition.mode != RoutingMode::Fixed {
        return Err(Error::Denied(
            "Phase 49 planner strategy must be fixed with role=planner",
        ));
    }
    let target_id = definition
        .fixed_target
        .as_deref()
        .ok_or(Error::Storage("fixed planner strategy lost target"))?;
    let target =
        routing_target(&definition, target_id).map_err(|error| Error::Config(error.to_string()))?;
    if target.account_id != identity.account_id || target.model != identity.model {
        return Err(Error::Denied(
            "planner working identity differs from fixed planner target",
        ));
    }
    Ok(definition)
}

impl SessionStore {
    pub fn planner_admit(
        &self,
        actor: &PrincipalId,
        parent_session: &str,
        parent_branch: &str,
        parent_turn_id: u64,
        limits: crate::subagents::DelegationLimits,
        child_routing_revision_id: Option<&str>,
        input: &PlannerInput,
        identity: &InvocationIdentity,
    ) -> Result<PlannerOperationInfo> {
        input.validate()?;
        let limits = limits.validate()?;
        if identity.actor != *actor {
            return Err(Error::Denied("planner actor changed"));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (_parent, branch) = crate::subagents::require_active_parent_turn(
            &tx,
            actor,
            parent_session,
            parent_branch,
            parent_turn_id,
        )?;
        let user = ensure_user_for_principal(&tx, actor)?;
        crate::subagents::validate_child_routing_revision(&tx, &user, child_routing_revision_id)?;
        let _strategy =
            validate_planner_strategy(&tx, &user, &input.strategy_revision_id, identity)?;
        let existing: Option<String> = db(tx
            .query_row(
                "SELECT id FROM delegation_roots
                 WHERE root_session_id=?1 AND root_branch_id=?2 AND root_turn_id=?3",
                params![parent_session, branch.id, parent_turn_id],
                |r| r.get(0),
            )
            .optional())?;
        let root_id = if let Some(root_id) = existing {
            let root = crate::subagents::delegation_root_row(&tx, &root_id)?;
            if root.limits != limits
                || root.child_routing_revision_id.as_deref() != child_routing_revision_id
            {
                return Err(Error::Conflict(
                    "delegation limits or child routing revision changed before planner admission",
                ));
            }
            match root.planner_routing_revision_id.as_deref() {
                Some(revision) if revision != input.strategy_revision_id => {
                    return Err(Error::Conflict(
                        "planner routing revision changed inside the delegation root",
                    ));
                }
                None => {
                    db(tx.execute(
                        "UPDATE delegation_roots SET planner_routing_revision_id=?1,updated_at=?2
                         WHERE id=?3 AND planner_routing_revision_id IS NULL",
                        params![input.strategy_revision_id, now(), root_id],
                    ))?;
                }
                _ => {}
            }
            root_id
        } else {
            let root_id = id("dgr")?;
            let at = now();
            let deadline_at = at.saturating_add(i64::from(limits.deadline_seconds) * 1_000_000);
            db(tx.execute(
                "INSERT INTO delegation_roots(
                    id,root_session_id,root_branch_id,root_turn_id,created_by_user_id,state,
                    max_children_created,max_active_children,max_model_attempts,max_depth,
                    deadline_seconds,deadline_at,created_at,updated_at,recursive_max_depth,
                    child_routing_revision_id,planner_routing_revision_id
                 ) VALUES(?1,?2,?3,?4,?5,'active',?6,?7,?8,1,?9,?10,?11,?11,?12,?13,?14)",
                params![
                    root_id,
                    parent_session,
                    branch.id,
                    parent_turn_id,
                    user,
                    limits.max_children_created,
                    limits.max_active_children,
                    limits.max_model_attempts,
                    limits.deadline_seconds,
                    deadline_at,
                    at,
                    limits.max_depth,
                    child_routing_revision_id,
                    input.strategy_revision_id,
                ],
            ))?;
            root_id
        };
        let root = crate::subagents::delegation_root_row(&tx, &root_id)?;
        if root.state != "active" || now() >= root.deadline_at {
            return Err(Error::Limit("delegation root deadline is exhausted"));
        }
        let remaining_children = root
            .limits
            .max_children_created
            .saturating_sub(root.children_created);
        if u32::from(input.evidence.max_tasks) > remaining_children {
            return Err(Error::Limit(
                "planner max_tasks exceeds remaining delegation child budget",
            ));
        }
        let operation_id = id("planop")?;
        let at = now();
        db(tx.execute(
            "INSERT INTO planner_operations(
                id,root_id,actor_user_id,parent_session_id,parent_branch_id,parent_turn_id,
                parent_branch_head,parent_branch_revision,strategy_revision_id,planner_identity_json,
                input_sha256,evidence_json,state,created_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,'admitted',?13)",
            params![
                operation_id,
                root_id,
                user,
                parent_session,
                branch.id,
                parent_turn_id,
                branch.head,
                branch.revision,
                input.strategy_revision_id,
                json(identity)?,
                input.digest()?,
                json(&input.evidence)?,
                at,
            ],
        ))?;
        let out = planner_operation_row(&tx, &operation_id)?.0;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn planner_journal(
        &self,
        actor: PrincipalId,
        operation_id: &str,
    ) -> Result<PlannerJournalHandle> {
        let c = self.connection()?;
        let (op, _) = planner_operation_row(&c, operation_id)?;
        if op.planner_identity.actor != actor || op.state != "admitted" {
            return Err(Error::Conflict(
                "planner operation is not awaiting a planner attempt",
            ));
        }
        Ok(PlannerJournalHandle {
            store: self.clone(),
            actor,
            operation_id: operation_id.to_owned(),
        })
    }

    pub fn planner_settle_unstarted(
        &self,
        actor: &PrincipalId,
        operation_id: &str,
        cancelled: bool,
        error: &str,
    ) -> Result<()> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (op, _) = planner_operation_row(&tx, operation_id)?;
        let user = ensure_user_for_principal(&tx, actor)?;
        if op.planner_identity.actor != *actor
            || !principal_is_session_owner(&tx, &op.parent_session_id, actor)?
        {
            return Err(Error::Denied("planner operation belongs to another owner"));
        }
        let state = if cancelled { "cancelled" } else { "failed" };
        let changed = db(tx.execute(
            "UPDATE planner_operations SET state=?1,error=?2,settled_at=?3
             WHERE id=?4 AND actor_user_id=?5 AND state='admitted'",
            params![state, bounded_error(Some(error)), now(), operation_id, user],
        ))?;
        if changed == 0 && op.state != state {
            return Err(Error::Conflict(
                "planner operation already entered provider intent",
            ));
        }
        db(tx.commit())?;
        Ok(())
    }

    pub fn planner_operation(
        &self,
        actor: &PrincipalId,
        operation_id: &str,
    ) -> Result<PlannerOperationInfo> {
        let c = self.connection()?;
        let (op, _) = planner_operation_row(&c, operation_id)?;
        authorize(&c, &op.parent_session_id, actor, false)?;
        Ok(op)
    }

    pub fn planner_tasks(
        &self,
        actor: &PrincipalId,
        operation_id: &str,
    ) -> Result<Vec<PlannerTaskRunInfo>> {
        let c = self.connection()?;
        let (op, _) = planner_operation_row(&c, operation_id)?;
        authorize(&c, &op.parent_session_id, actor, false)?;
        planner_task_rows(&c, operation_id)
    }

    pub fn planner_start_task(
        &self,
        actor: &PrincipalId,
        operation_id: &str,
        task_id: &str,
    ) -> Result<PlannerTaskRunInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (op, _) = planner_operation_row(&tx, operation_id)?;
        if !principal_is_session_owner(&tx, &op.parent_session_id, actor)?
            || !matches!(op.state.as_str(), "planned" | "running")
        {
            return Err(Error::Denied("planner task is not runnable by this owner"));
        }
        crate::subagents::require_active_parent_turn(
            &tx,
            actor,
            &op.parent_session_id,
            &op.parent_branch_id,
            op.parent_turn_id,
        )?;
        let task = planner_task_row(&tx, operation_id, task_id)?;
        if task.state != "pending" {
            return Err(Error::Conflict("planner task is not pending"));
        }
        for dependency in &task.dependencies {
            if planner_task_row(&tx, operation_id, dependency)?.state != "completed" {
                return Err(Error::Conflict(
                    "planner task dependencies are not completed",
                ));
            }
        }
        let at = now();
        if op.state == "planned" {
            db(tx.execute(
                "UPDATE planner_operations SET state='running' WHERE id=?1 AND state='planned'",
                [operation_id],
            ))?;
        }
        let changed = db(tx.execute(
            "UPDATE planner_task_runs SET state='running',started_at=?1
             WHERE planner_operation_id=?2 AND task_id=?3 AND state='pending'",
            params![at, operation_id, task_id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("planner task start raced"));
        }
        let out = planner_task_row(&tx, operation_id, task_id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn planner_complete_task(
        &self,
        actor: &PrincipalId,
        operation_id: &str,
        task_id: &str,
        subagent_id: &str,
        child_session_id: &str,
        answer: &str,
        answer_truncated: bool,
    ) -> Result<PlannerTaskRunInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (op, _) = planner_operation_row(&tx, operation_id)?;
        if !principal_is_session_owner(&tx, &op.parent_session_id, actor)? {
            return Err(Error::Denied("planner operation belongs to another owner"));
        }
        let relation = crate::subagents::row_for_team(&tx, subagent_id)?;
        if relation.parent_session_id != op.parent_session_id
            || relation.parent_branch_id != op.parent_branch_id
            || relation.parent_turn_id != op.parent_turn_id
            || relation.child_session_id != child_session_id
            || relation.state != "completed"
        {
            return Err(Error::Denied(
                "planner task child does not match the exact planner parent turn",
            ));
        }
        let child_session = session_in(&tx, child_session_id)?;
        let child_branch = branch_in(&tx, child_session_id, "main")?;
        let child_history = history_in(&tx, &child_session, &child_branch)?;
        if child_history.turns.len() != 1 || child_history.turns[0].outcome != Outcome::Completed {
            return Err(Error::Conflict(
                "planner task child has no single authoritative completed answer",
            ));
        }
        let authoritative = &child_history.turns[0].answer;
        let matches = if answer_truncated {
            answer.len() < authoritative.len() && authoritative.starts_with(answer)
        } else {
            authoritative == answer
        };
        if !matches {
            return Err(Error::Conflict(
                "planner imported answer differs from authoritative child Session answer",
            ));
        }
        let digest = format!("{:x}", Sha256::digest(answer.as_bytes()));
        let at = now();
        let changed = db(tx.execute(
            "UPDATE planner_task_runs
             SET state='completed',subagent_id=?1,child_session_id=?2,answer_sha256=?3,
                 answer_truncated=?4,settled_at=?5
             WHERE planner_operation_id=?6 AND task_id=?7 AND state='running'",
            params![
                subagent_id,
                child_session_id,
                digest,
                i64::from(answer_truncated),
                at,
                operation_id,
                task_id,
            ],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("planner task is not running"));
        }
        let out = planner_task_row(&tx, operation_id, task_id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn planner_cancel(
        &self,
        actor: &PrincipalId,
        operation_id: &str,
        reason: &str,
    ) -> Result<PlannerOperationInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (op, _) = planner_operation_row(&tx, operation_id)?;
        if !principal_is_session_owner(&tx, &op.parent_session_id, actor)? {
            return Err(Error::Denied("planner operation belongs to another owner"));
        }
        let at = now();
        db(tx.execute(
            "UPDATE planner_task_runs
             SET state='interrupted',started_at=COALESCE(started_at,?1),settled_at=?1,
                 error='parent cancelled planned execution; no automatic replay'
             WHERE planner_operation_id=?2 AND state IN ('pending','running')",
            params![at, operation_id],
        ))?;
        let changed = db(tx.execute(
            "UPDATE planner_operations SET state='cancelled',settled_at=?1,error=?2
             WHERE id=?3 AND state IN ('planned','running')",
            params![at, bounded_error(Some(reason)), operation_id],
        ))?;
        if changed != 1 && op.state != "cancelled" {
            return Err(Error::Conflict("planner operation is not cancellable"));
        }
        let out = planner_operation_row(&tx, operation_id)?.0;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn planner_fail_task(
        &self,
        actor: &PrincipalId,
        operation_id: &str,
        task_id: &str,
        error: &str,
    ) -> Result<PlannerOperationInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (op, _) = planner_operation_row(&tx, operation_id)?;
        if !principal_is_session_owner(&tx, &op.parent_session_id, actor)? {
            return Err(Error::Denied("planner operation belongs to another owner"));
        }
        let at = now();
        db(tx.execute(
            "UPDATE planner_task_runs
             SET state='failed',started_at=COALESCE(started_at,?1),settled_at=?1,error=?2
             WHERE planner_operation_id=?3 AND task_id=?4 AND state IN ('pending','running')",
            params![at, bounded_error(Some(error)), operation_id, task_id],
        ))?;
        db(tx.execute(
            "UPDATE planner_task_runs
             SET state='interrupted',started_at=?1,settled_at=?1,
                 error='not run because another planned task failed'
             WHERE planner_operation_id=?2 AND state='pending'",
            params![at, operation_id],
        ))?;
        db(tx.execute(
            "UPDATE planner_operations SET state='failed',settled_at=?1,error=?2
             WHERE id=?3 AND state IN ('planned','running')",
            params![at, bounded_error(Some(error)), operation_id],
        ))?;
        let out = planner_operation_row(&tx, operation_id)?.0;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn planner_join(
        &self,
        actor: &PrincipalId,
        operation_id: &str,
        receipt: &PlannerJoinReceipt,
    ) -> Result<(PlannerOperationInfo, PlannerJoinInfo)> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (op, evidence) = planner_operation_row(&tx, operation_id)?;
        if !principal_is_session_owner(&tx, &op.parent_session_id, actor)? || op.state != "running"
        {
            return Err(Error::Denied(
                "planner operation is not joinable by this owner",
            ));
        }
        let proposal = op
            .proposal
            .as_ref()
            .ok_or(Error::Storage("planned operation lost proposal"))?;
        let verified = join_planner_results(&evidence, proposal, receipt.tasks.clone())
            .map_err(|error| Error::Config(error.to_string()))?;
        if verified.joined_sha256 != receipt.joined_sha256
            || verified.joined_text != receipt.joined_text
        {
            return Err(Error::Conflict(
                "planner join receipt differs from deterministic join",
            ));
        }
        let rows = planner_task_rows(&tx, operation_id)?;
        if rows.iter().any(|row| row.state != "completed") {
            return Err(Error::Conflict("planner join has incomplete task rows"));
        }
        for (stored, joined) in rows.iter().zip(receipt.tasks.iter()) {
            let digest = format!("{:x}", Sha256::digest(joined.answer.as_bytes()));
            if stored.task_id != joined.id
                || stored.role != joined.role
                || stored.answer_sha256.as_deref() != Some(digest.as_str())
                || stored.answer_truncated != joined.answer_truncated
            {
                return Err(Error::Conflict(
                    "planner join task evidence differs from durable task receipt",
                ));
            }
        }
        let manifest = serde_json::json!({
            "publisher": "parent_agent",
            "tasks": rows.iter().map(|row| serde_json::json!({
                "task_id": row.task_id,
                "ordinal": row.ordinal,
                "role": row.role,
                "dependencies": row.dependencies,
                "subagent_id": row.subagent_id,
                "child_session_id": row.child_session_id,
                "answer_sha256": row.answer_sha256,
                "answer_truncated": row.answer_truncated,
            })).collect::<Vec<_>>()
        });
        let at = now();
        db(tx.execute(
            "INSERT INTO planner_join_receipts(planner_operation_id,manifest_json,joined_sha256,created_at)
             VALUES(?1,?2,?3,?4)",
            params![operation_id, manifest.to_string(), receipt.joined_sha256, at],
        ))?;
        db(tx.execute(
            "UPDATE planner_operations SET state='joined',settled_at=?1 WHERE id=?2 AND state='running'",
            params![at, operation_id],
        ))?;
        let joined = PlannerJoinInfo {
            planner_operation_id: operation_id.to_owned(),
            manifest,
            joined_sha256: receipt.joined_sha256.clone(),
            created_at: at,
        };
        let out = planner_operation_row(&tx, operation_id)?.0;
        db(tx.commit())?;
        Ok((out, joined))
    }

    pub(crate) fn reconstruct_planner_join_in(
        c: &Connection,
        operation_id: &str,
    ) -> Result<PlannerJoinReceipt> {
        let (op, evidence) = planner_operation_row(c, operation_id)?;
        if op.state != "joined" {
            return Err(Error::Conflict("planner operation is not durably joined"));
        }
        let proposal = op
            .proposal
            .as_ref()
            .ok_or(Error::Storage("joined planner operation lost proposal"))?;
        let rows = planner_task_rows(c, operation_id)?;
        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            if row.state != "completed" {
                return Err(Error::Conflict(
                    "planner review source has incomplete tasks",
                ));
            }
            let child_session_id = row
                .child_session_id
                .as_deref()
                .ok_or(Error::Storage("planner task lost child Session provenance"))?;
            let child_session = session_in(c, child_session_id)?;
            let child_branch = branch_in(c, child_session_id, "main")?;
            let child_history = history_in(c, &child_session, &child_branch)?;
            if child_history.turns.len() != 1
                || child_history.turns[0].outcome != Outcome::Completed
            {
                return Err(Error::Conflict(
                    "planner task child has no single authoritative completed answer",
                ));
            }
            let authoritative = &child_history.turns[0].answer;
            let answer = if row.answer_truncated {
                if authoritative.len() <= MAX_PLANNER_TASK_IMPORT_BYTES {
                    return Err(Error::Conflict(
                        "planner task truncation receipt does not match authoritative answer",
                    ));
                }
                let mut end = MAX_PLANNER_TASK_IMPORT_BYTES;
                while !authoritative.is_char_boundary(end) {
                    end -= 1;
                }
                authoritative[..end].to_owned()
            } else {
                if authoritative.len() > MAX_PLANNER_TASK_IMPORT_BYTES {
                    return Err(Error::Conflict(
                        "planner task untruncated receipt exceeds import bound",
                    ));
                }
                authoritative.clone()
            };
            let digest = format!("{:x}", Sha256::digest(answer.as_bytes()));
            if row.answer_sha256.as_deref() != Some(digest.as_str()) {
                return Err(Error::Conflict(
                    "planner task authoritative answer no longer matches durable receipt",
                ));
            }
            results.push(PlannerTaskResult {
                id: row.task_id,
                role: row.role,
                answer,
                answer_truncated: row.answer_truncated,
            });
        }
        let rebuilt = join_planner_results(&evidence, proposal, results)
            .map_err(|error| Error::Config(error.to_string()))?;
        let stored_digest: Option<String> = db(c
            .query_row(
                "SELECT joined_sha256 FROM planner_join_receipts
                 WHERE planner_operation_id=?1",
                [operation_id],
                |r| r.get(0),
            )
            .optional())?;
        if stored_digest.as_deref() != Some(rebuilt.joined_sha256.as_str()) {
            return Err(Error::Conflict(
                "reconstructed planner join differs from immutable join receipt",
            ));
        }
        Ok(rebuilt)
    }

    pub fn planner_join_receipt(
        &self,
        actor: &PrincipalId,
        operation_id: &str,
    ) -> Result<Option<PlannerJoinInfo>> {
        let c = self.connection()?;
        let (op, _) = planner_operation_row(&c, operation_id)?;
        authorize(&c, &op.parent_session_id, actor, false)?;
        let row: Option<(String, String, i64)> = db(c
            .query_row(
                "SELECT manifest_json,joined_sha256,created_at FROM planner_join_receipts
                 WHERE planner_operation_id=?1",
                [operation_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional())?;
        row.map(|(manifest, joined_sha256, created_at)| {
            Ok(PlannerJoinInfo {
                planner_operation_id: operation_id.to_owned(),
                manifest: decode(&manifest)?,
                joined_sha256,
                created_at,
            })
        })
        .transpose()
    }

    pub(crate) fn recover_planners_after_owner_restart(&self, c: &Connection) -> Result<()> {
        if self.trusted_local_direct() {
            return Ok(());
        }
        let at = now();
        db(c.execute(
            "UPDATE planner_task_runs
             SET state='interrupted',started_at=COALESCE(started_at,?1),settled_at=?1,
                 error='owner restarted before planned task/join completion; no automatic replay'
             WHERE state IN ('pending','running')
               AND planner_operation_id IN (
                 SELECT id FROM planner_operations WHERE state IN ('planned','running')
               )",
            [at],
        ))?;
        db(c.execute(
            "UPDATE planner_operations
             SET state='interrupted',settled_at=?1,error='owner restarted after plan completion; no automatic task replay'
             WHERE state IN ('admitted','planned','running')",
            [at],
        ))?;
        db(c.execute(
            "UPDATE planner_operations
             SET state='unknown',settled_at=?1,error='owner restarted after planner provider intent; no automatic replay'
             WHERE state='started'",
            [at],
        ))?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct PlannerJournalHandle {
    store: SessionStore,
    actor: PrincipalId,
    operation_id: String,
}

impl PlannerJournalHandle {
    fn authorize_in(
        &self,
        c: &Connection,
        input: &PlannerInput,
        identity: &InvocationIdentity,
        states: &[&str],
    ) -> Result<(PlannerOperationInfo, PlannerEvidence)> {
        let user = ensure_user_for_principal(c, &self.actor)?;
        let (op, evidence) = planner_operation_row(c, &self.operation_id)?;
        if !states.contains(&op.state.as_str())
            || op.planner_identity != *identity
            || identity.actor != self.actor
            || op.strategy_revision_id != input.strategy_revision_id
            || op.input_sha256 != input.digest()?
        {
            return Err(Error::Denied("planner operation/input authority mismatch"));
        }
        let root = crate::subagents::delegation_root_row(c, &op.root_id)?;
        if root.planner_routing_revision_id.as_deref() != Some(op.strategy_revision_id.as_str())
            || root.state != "active"
            || now() >= root.deadline_at
        {
            return Err(Error::Denied(
                "planner delegation root is no longer active/authorized",
            ));
        }
        let (_parent, branch) = crate::subagents::require_active_parent_turn(
            c,
            &self.actor,
            &op.parent_session_id,
            &op.parent_branch_id,
            op.parent_turn_id,
        )?;
        let stored: Option<(String, u64)> = db(c
            .query_row(
                "SELECT parent_branch_head,parent_branch_revision FROM planner_operations
                 WHERE id=?1 AND actor_user_id=?2",
                params![self.operation_id, user],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional())?;
        let Some((head, revision)) = stored else {
            return Err(Error::Denied("planner operation belongs to another owner"));
        };
        if branch.head != head || branch.revision != revision {
            return Err(Error::Conflict(
                "planner source branch changed before planner result publication",
            ));
        }
        let _ = validate_planner_strategy(c, &user, &op.strategy_revision_id, identity)?;
        Ok((op, evidence))
    }
}

impl PlannerJournal for PlannerJournalHandle {
    fn authorize<'a>(
        &'a self,
        input: &'a PlannerInput,
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
        input: &'a PlannerInput,
        identity: &'a InvocationIdentity,
    ) -> aw_agent_core::JournalFuture<'a, InvocationContext> {
        Box::pin(async move {
            let mut c = self.store.connection()?;
            let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let (op, _) = self.authorize_in(&tx, input, identity, &["admitted"])?;
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
                return Err(Error::Conflict("planner attempt raced root model limit"));
            }
            db(tx.execute(
                "UPDATE planner_operations SET state='started',started_at=?1
                 WHERE id=?2 AND state='admitted'",
                params![at, op.id],
            ))?;
            let ordinal = root.model_attempts_started.saturating_add(1);
            let context = InvocationContext {
                purpose: InvocationPurpose::Planner {
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

    fn finish<'a>(&'a self, result: &'a PlannerResult) -> aw_agent_core::JournalFuture<'a, ()> {
        Box::pin(async move {
            let mut c = self.store.connection()?;
            let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let (op, evidence) = planner_operation_row(&tx, &self.operation_id)?;
            if op.planner_identity.actor != self.actor || op.state != "started" {
                return Err(Error::Conflict("planner operation is not settleable"));
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
                let proposal = result
                    .proposal
                    .as_ref()
                    .ok_or(Error::Protocol("completed planner result has no proposal"))?;
                let order = planner_topological_order(&evidence, proposal)
                    .map_err(|error| Error::Config(error.to_string()))?;
                let body = json(proposal)?;
                let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
                db(tx.execute(
                    "UPDATE planner_operations
                     SET state='planned',proposal_sha256=?1,proposal_json=?2,metadata_json=?3,
                         planned_at=?4,error=NULL
                     WHERE id=?5 AND state='started'",
                    params![digest, body, metadata_json, at, op.id],
                ))?;
                let task_map = proposal
                    .tasks
                    .iter()
                    .map(|task| (task.id.as_str(), task))
                    .collect::<std::collections::BTreeMap<_, _>>();
                for (ordinal, task_id) in order.iter().enumerate() {
                    let task = task_map
                        .get(task_id.as_str())
                        .copied()
                        .ok_or(Error::Storage("validated planner task disappeared"))?;
                    db(tx.execute(
                        "INSERT INTO planner_task_runs(
                            planner_operation_id,task_id,ordinal,role,dependencies_json,state
                         ) VALUES(?1,?2,?3,?4,?5,'pending')",
                        params![op.id, task.id, ordinal, task.role, json(&task.depends_on)?],
                    ))?;
                }
            } else {
                let state = if result.outcome == Outcome::Cancelled {
                    "cancelled"
                } else {
                    "failed"
                };
                db(tx.execute(
                    "UPDATE planner_operations
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
