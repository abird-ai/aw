use super::*;
use aw_pipeline::{
    join_team_members, team_definition_sha256, validate_team_definition, TeamDefinition,
    TeamJoinReceipt,
};
#[cfg(feature = "routing")]
use aw_pipeline::{TeamJoinMember, MAX_TEAM_MEMBER_IMPORT_BYTES};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_TEAM_TEMPLATES: i64 = 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamRevisionInfo {
    pub team_id: String,
    pub revision_id: String,
    pub revision_no: u64,
    pub name: String,
    pub owner_user_id: String,
    pub definition_sha256: String,
    pub definition: TeamDefinition,
    pub created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamRunInfo {
    pub id: String,
    pub revision_id: String,
    pub actor_user_id: String,
    pub parent_session_id: String,
    pub parent_branch_id: String,
    pub parent_turn_id: u64,
    pub state: String,
    pub task_sha256: String,
    pub publisher_kind: String,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub settled_at: Option<i64>,
    pub failure_code: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamRunMemberInfo {
    pub team_run_id: String,
    pub member_id: String,
    pub ordinal: usize,
    pub role: String,
    pub state: String,
    pub subagent_id: Option<String>,
    pub child_session_id: Option<String>,
    pub answer_sha256: Option<String>,
    pub answer_truncated: bool,
    pub started_at: Option<i64>,
    pub settled_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamJoinInfo {
    pub team_run_id: String,
    pub policy: String,
    pub manifest: serde_json::Value,
    pub joined_sha256: String,
    pub created_at: i64,
}

fn team_error<T>(value: aw_pipeline::Result<T>) -> Result<T> {
    value.map_err(|error| match error {
        aw_pipeline::PipelineError::Definition(_) => Error::Config(error.to_string()),
        aw_pipeline::PipelineError::TextLimit => {
            Error::Limit("team joined result exceeds configured bound")
        }
        aw_pipeline::PipelineError::CheckFailed => {
            Error::Denied("unexpected deterministic team check failure")
        }
    })
}

fn team_revision_row(
    c: &rusqlite::Connection,
    team_id: &str,
    revision: Option<&str>,
) -> Result<TeamRevisionInfo> {
    let row: Option<(String, u64, String, String, String, String, i64)> =
        if let Some(revision) = revision {
            if let Ok(number) = revision.parse::<u64>() {
                db(c.query_row(
                    "SELECT r.revision_id,r.revision_no,t.name,t.owner_user_id,
                        r.definition_sha256,r.definition_json,r.created_at
                 FROM team_template_revisions r
                 JOIN team_templates t ON t.id=r.team_id
                 WHERE r.team_id=?1 AND r.revision_no=?2",
                    params![team_id, number],
                    |r| {
                        Ok((
                            r.get(0)?,
                            r.get(1)?,
                            r.get(2)?,
                            r.get(3)?,
                            r.get(4)?,
                            r.get(5)?,
                            r.get(6)?,
                        ))
                    },
                )
                .optional())?
            } else {
                db(c.query_row(
                    "SELECT r.revision_id,r.revision_no,t.name,t.owner_user_id,
                        r.definition_sha256,r.definition_json,r.created_at
                 FROM team_template_revisions r
                 JOIN team_templates t ON t.id=r.team_id
                 WHERE r.team_id=?1 AND r.revision_id=?2",
                    params![team_id, revision],
                    |r| {
                        Ok((
                            r.get(0)?,
                            r.get(1)?,
                            r.get(2)?,
                            r.get(3)?,
                            r.get(4)?,
                            r.get(5)?,
                            r.get(6)?,
                        ))
                    },
                )
                .optional())?
            }
        } else {
            db(c.query_row(
                "SELECT r.revision_id,r.revision_no,t.name,t.owner_user_id,
                    r.definition_sha256,r.definition_json,r.created_at
             FROM team_template_revisions r
             JOIN team_templates t ON t.id=r.team_id
             WHERE r.team_id=?1
             ORDER BY r.revision_no DESC LIMIT 1",
                [team_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                },
            )
            .optional())?
        };
    let Some((revision_id, revision_no, name, owner_user_id, definition_sha256, body, created_at)) =
        row
    else {
        return Err(Error::Denied("team revision unavailable"));
    };
    let definition: TeamDefinition = decode(&body)?;
    team_error(validate_team_definition(&definition))?;
    Ok(TeamRevisionInfo {
        team_id: team_id.to_owned(),
        revision_id,
        revision_no,
        name,
        owner_user_id,
        definition_sha256,
        definition,
        created_at,
    })
}

fn team_owned_row(
    c: &rusqlite::Connection,
    owner_user_id: &str,
    selector: &str,
    revision: Option<&str>,
) -> Result<TeamRevisionInfo> {
    let team_id: Option<String> = db(c
        .query_row(
            "SELECT id FROM team_templates
             WHERE owner_user_id=?1 AND status='active' AND (id=?2 OR name=?2)",
            params![owner_user_id, selector],
            |r| r.get(0),
        )
        .optional())?;
    let team_id = team_id.ok_or(Error::Denied("team template unavailable"))?;
    let info = team_revision_row(c, &team_id, revision)?;
    if info.owner_user_id != owner_user_id {
        return Err(Error::Denied("team template belongs to another owner"));
    }
    Ok(info)
}

fn team_run_row(c: &rusqlite::Connection, id: &str) -> Result<TeamRunInfo> {
    let row: Option<(
        String,
        String,
        String,
        String,
        u64,
        String,
        String,
        String,
        i64,
        Option<i64>,
        Option<i64>,
        Option<String>,
    )> = db(c
        .query_row(
            "SELECT revision_id,actor_user_id,parent_session_id,parent_branch_id,parent_turn_id,
                    state,task_sha256,publisher_kind,created_at,started_at,settled_at,failure_code
             FROM team_runs WHERE id=?1",
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
                ))
            },
        )
        .optional())?;
    let Some((
        revision_id,
        actor_user_id,
        parent_session_id,
        parent_branch_id,
        parent_turn_id,
        state,
        task_sha256,
        publisher_kind,
        created_at,
        started_at,
        settled_at,
        failure_code,
    )) = row
    else {
        return Err(Error::Denied("team run unavailable"));
    };
    Ok(TeamRunInfo {
        id: id.to_owned(),
        revision_id,
        actor_user_id,
        parent_session_id,
        parent_branch_id,
        parent_turn_id,
        state,
        task_sha256,
        publisher_kind,
        created_at,
        started_at,
        settled_at,
        failure_code,
    })
}

fn team_member_row(
    c: &rusqlite::Connection,
    run_id: &str,
    member_id: &str,
) -> Result<TeamRunMemberInfo> {
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
    )> = db(c
        .query_row(
            "SELECT ordinal,role,state,subagent_id,child_session_id,answer_sha256,
                    answer_truncated,started_at,settled_at,error
             FROM team_run_members WHERE team_run_id=?1 AND member_id=?2",
            params![run_id, member_id],
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
    )) = row
    else {
        return Err(Error::Denied("team member unavailable"));
    };
    Ok(TeamRunMemberInfo {
        team_run_id: run_id.to_owned(),
        member_id: member_id.to_owned(),
        ordinal,
        role,
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

impl SessionStore {
    pub fn team_define(
        &self,
        actor: &PrincipalId,
        definition: TeamDefinition,
    ) -> Result<TeamRevisionInfo> {
        team_error(validate_team_definition(&definition))?;
        let digest = team_error(team_definition_sha256(&definition))?;
        let body = json(&definition)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = ensure_user_for_principal(&tx, actor)?;
        let existing: Option<String> = db(tx
            .query_row(
                "SELECT id FROM team_templates WHERE owner_user_id=?1 AND name=?2",
                params![user, definition.name],
                |r| r.get(0),
            )
            .optional())?;
        let team_id = if let Some(id) = existing {
            let status: String = db(tx.query_row(
                "SELECT status FROM team_templates WHERE id=?1",
                [&id],
                |r| r.get(0),
            ))?;
            if status != "active" {
                return Err(Error::Denied("team template is disabled"));
            }
            id
        } else {
            let count: i64 = db(tx.query_row(
                "SELECT count(*) FROM team_templates WHERE owner_user_id=?1",
                [&user],
                |r| r.get(0),
            ))?;
            if count >= MAX_TEAM_TEMPLATES {
                return Err(Error::Limit("team template retention limit reached"));
            }
            let id = id("team")?;
            db(tx.execute(
                "INSERT INTO team_templates(id,name,owner_user_id,status,created_at)
                 VALUES(?1,?2,?3,'active',?4)",
                params![id, definition.name, user, now()],
            ))?;
            id
        };
        let previous: i64 = db(tx.query_row(
            "SELECT COALESCE(max(revision_no),0) FROM team_template_revisions WHERE team_id=?1",
            [&team_id],
            |r| r.get(0),
        ))?;
        let revision_no = previous as u64 + 1;
        let revision_id = id("teamrev")?;
        let at = now();
        db(tx.execute(
            "INSERT INTO team_template_revisions(
                revision_id,team_id,revision_no,definition_sha256,definition_json,
                created_by_user_id,created_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![revision_id, team_id, revision_no, digest, body, user, at],
        ))?;
        let out = team_revision_row(&tx, &team_id, Some(&revision_id))?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn team_revision(
        &self,
        actor: &PrincipalId,
        selector: &str,
        revision: Option<&str>,
    ) -> Result<TeamRevisionInfo> {
        let c = self.connection()?;
        let user = ensure_user_for_principal(&c, actor)?;
        team_owned_row(&c, &user, selector, revision)
    }

    pub fn teams(&self, actor: &PrincipalId) -> Result<Vec<TeamRevisionInfo>> {
        let c = self.connection()?;
        let user = ensure_user_for_principal(&c, actor)?;
        let mut q = db(c.prepare(
            "SELECT id FROM team_templates
             WHERE owner_user_id=?1 AND status='active'
             ORDER BY name,id LIMIT 512",
        ))?;
        let rows = db(q.query_map([user], |r| r.get::<_, String>(0)))?;
        let ids = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        ids.into_iter()
            .map(|id| team_revision_row(&c, &id, None))
            .collect()
    }

    pub fn team_begin(
        &self,
        actor: &PrincipalId,
        revision_id: &str,
        parent_session: &str,
        parent_branch: &str,
        parent_turn_id: u64,
        task: &str,
    ) -> Result<TeamRunInfo> {
        if task.is_empty() || task.len() > aw_pipeline::MAX_PIPELINE_TEXT_BYTES {
            return Err(Error::Limit("team task exceeds configured bound"));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = ensure_user_for_principal(&tx, actor)?;
        let (parent, branch) = crate::subagents::require_active_parent_turn(
            &tx,
            actor,
            parent_session,
            parent_branch,
            parent_turn_id,
        )?;
        if parent.id != parent_session || branch.id != parent_branch {
            return Err(Error::Conflict("team parent Session/branch changed"));
        }
        let revision: Option<(String, String)> = db(tx
            .query_row(
                "SELECT r.team_id,t.owner_user_id
                 FROM team_template_revisions r JOIN team_templates t ON t.id=r.team_id
                 WHERE r.revision_id=?1 AND t.status='active'",
                [revision_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional())?;
        let Some((team_id, owner_user_id)) = revision else {
            return Err(Error::Denied("team revision unavailable"));
        };
        if owner_user_id != user {
            return Err(Error::Denied("team revision belongs to another owner"));
        }
        let revision = team_revision_row(&tx, &team_id, Some(revision_id))?;
        let run_id = id("trun")?;
        let at = now();
        let task_sha256 = format!("{:x}", Sha256::digest(task.as_bytes()));
        db(tx.execute(
            "INSERT INTO team_runs(
                id,revision_id,actor_user_id,parent_session_id,parent_branch_id,parent_turn_id,
                state,task_sha256,publisher_kind,created_at
             ) VALUES(?1,?2,?3,?4,?5,?6,'admitted',?7,'parent_agent',?8)",
            params![
                run_id,
                revision_id,
                user,
                parent_session,
                parent_branch,
                parent_turn_id,
                task_sha256,
                at
            ],
        ))?;
        for (ordinal, member) in revision.definition.members.iter().enumerate() {
            db(tx.execute(
                "INSERT INTO team_run_members(team_run_id,member_id,ordinal,role,state)
                 VALUES(?1,?2,?3,?4,'pending')",
                params![run_id, member.id, ordinal, member.role],
            ))?;
        }
        let out = team_run_row(&tx, &run_id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn team_start_member(
        &self,
        actor: &PrincipalId,
        run_id: &str,
        member_id: &str,
    ) -> Result<TeamRunMemberInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let run = team_run_row(&tx, run_id)?;
        if !principal_is_session_owner(&tx, &run.parent_session_id, actor)? {
            return Err(Error::Denied("team run belongs to another owner"));
        }
        if !matches!(run.state.as_str(), "admitted" | "running") {
            return Err(Error::Conflict("team run is not runnable"));
        }
        let at = now();
        if run.state == "admitted" {
            db(tx.execute(
                "UPDATE team_runs SET state='running',started_at=?1
                 WHERE id=?2 AND state='admitted'",
                params![at, run_id],
            ))?;
        }
        let changed = db(tx.execute(
            "UPDATE team_run_members SET state='running',started_at=?1
             WHERE team_run_id=?2 AND member_id=?3 AND state='pending'",
            params![at, run_id, member_id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("team member is not pending"));
        }
        let out = team_member_row(&tx, run_id, member_id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn team_complete_member(
        &self,
        actor: &PrincipalId,
        run_id: &str,
        member_id: &str,
        subagent_id: &str,
        child_session_id: &str,
        answer: &str,
        answer_truncated: bool,
    ) -> Result<TeamRunMemberInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let run = team_run_row(&tx, run_id)?;
        if !principal_is_session_owner(&tx, &run.parent_session_id, actor)? {
            return Err(Error::Denied("team run belongs to another owner"));
        }
        let relation = crate::subagents::row_for_team(&tx, subagent_id)?;
        if relation.parent_session_id != run.parent_session_id
            || relation.parent_branch_id != run.parent_branch_id
            || relation.parent_turn_id != run.parent_turn_id
            || relation.child_session_id != child_session_id
            || relation.state != "completed"
        {
            return Err(Error::Denied(
                "team member child does not match the exact team parent turn",
            ));
        }
        let child_session = session_in(&tx, child_session_id)?;
        let child_branch = branch_in(&tx, child_session_id, "main")?;
        let child_history = history_in(&tx, &child_session, &child_branch)?;
        if child_history.turns.len() != 1 {
            return Err(Error::Conflict(
                "team member child history changed before join receipt",
            ));
        }
        let child_turn = &child_history.turns[0];
        if child_turn.outcome != Outcome::Completed {
            return Err(Error::Conflict(
                "team member child has no authoritative completed answer",
            ));
        }
        let answer_matches = if answer_truncated {
            answer.len() < child_turn.answer.len() && child_turn.answer.starts_with(answer)
        } else {
            child_turn.answer == answer
        };
        if !answer_matches {
            return Err(Error::Conflict(
                "team member imported answer differs from authoritative child Session answer",
            ));
        }
        let answer_sha256 = format!("{:x}", Sha256::digest(answer.as_bytes()));
        let at = now();
        let changed = db(tx.execute(
            "UPDATE team_run_members
             SET state='completed',subagent_id=?1,child_session_id=?2,answer_sha256=?3,
                 answer_truncated=?4,settled_at=?5
             WHERE team_run_id=?6 AND member_id=?7 AND state='running'",
            params![
                subagent_id,
                child_session_id,
                answer_sha256,
                i64::from(answer_truncated),
                at,
                run_id,
                member_id
            ],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("team member is not running"));
        }
        let out = team_member_row(&tx, run_id, member_id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn team_fail_member(
        &self,
        actor: &PrincipalId,
        run_id: &str,
        member_id: &str,
        code: &str,
        error: &str,
    ) -> Result<TeamRunInfo> {
        if code.is_empty()
            || code.len() > 64
            || !code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || error.len() > 4096
        {
            return Err(Error::Config("invalid bounded team failure".into()));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let run = team_run_row(&tx, run_id)?;
        if !principal_is_session_owner(&tx, &run.parent_session_id, actor)? {
            return Err(Error::Denied("team run belongs to another owner"));
        }
        let at = now();
        db(tx.execute(
            "UPDATE team_run_members
             SET state='failed',started_at=COALESCE(started_at,?1),settled_at=?1,error=?2
             WHERE team_run_id=?3 AND member_id=?4 AND state IN ('pending','running')",
            params![at, error, run_id, member_id],
        ))?;
        db(tx.execute(
            "UPDATE team_run_members
             SET state='interrupted',started_at=?1,settled_at=?1,
                 error='not run because another required team member failed'
             WHERE team_run_id=?2 AND member_id<>?3 AND state='pending'",
            params![at, run_id, member_id],
        ))?;
        db(tx.execute(
            "UPDATE team_runs
             SET state='failed',started_at=COALESCE(started_at,?1),settled_at=?1,failure_code=?2
             WHERE id=?3 AND state IN ('admitted','running')",
            params![at, code, run_id],
        ))?;
        let out = team_run_row(&tx, run_id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn team_fail_run(
        &self,
        actor: &PrincipalId,
        run_id: &str,
        code: &str,
        error: &str,
    ) -> Result<TeamRunInfo> {
        if code.is_empty()
            || code.len() > 64
            || !code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || error.len() > 4096
            || error.chars().any(char::is_control)
        {
            return Err(Error::Config("invalid bounded team failure".into()));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let run = team_run_row(&tx, run_id)?;
        if !principal_is_session_owner(&tx, &run.parent_session_id, actor)? {
            return Err(Error::Denied("team run belongs to another owner"));
        }
        if !matches!(run.state.as_str(), "admitted" | "running") {
            return Err(Error::Conflict("team run is not fail-able"));
        }
        let at = now();
        db(tx.execute(
            "UPDATE team_run_members
             SET state='failed',started_at=COALESCE(started_at,?1),settled_at=?1,error=?2
             WHERE team_run_id=?3 AND state IN ('pending','running')",
            params![at, error, run_id],
        ))?;
        let changed = db(tx.execute(
            "UPDATE team_runs
             SET state='failed',started_at=COALESCE(started_at,?1),settled_at=?1,failure_code=?2
             WHERE id=?3 AND state IN ('admitted','running')",
            params![at, code, run_id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict(
                "team run changed before failure settlement",
            ));
        }
        let out = team_run_row(&tx, run_id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn team_join(
        &self,
        actor: &PrincipalId,
        run_id: &str,
        receipt: &TeamJoinReceipt,
    ) -> Result<(TeamRunInfo, TeamJoinInfo)> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let run = team_run_row(&tx, run_id)?;
        if !principal_is_session_owner(&tx, &run.parent_session_id, actor)? {
            return Err(Error::Denied("team run belongs to another owner"));
        }
        if run.state != "running" {
            return Err(Error::Conflict("team run is not ready to join"));
        }
        let revision_id = run.revision_id.clone();
        let team_id: String = db(tx.query_row(
            "SELECT team_id FROM team_template_revisions WHERE revision_id=?1",
            [&revision_id],
            |r| r.get(0),
        ))?;
        let revision = team_revision_row(&tx, &team_id, Some(&revision_id))?;
        let verified = team_error(join_team_members(
            &revision.definition,
            receipt.members.clone(),
        ))?;
        if verified.joined_sha256 != receipt.joined_sha256
            || verified.joined_text != receipt.joined_text
        {
            return Err(Error::Conflict(
                "team join receipt differs from deterministic join",
            ));
        }
        let members = revision
            .definition
            .members
            .iter()
            .map(|member| team_member_row(&tx, run_id, &member.id))
            .collect::<Result<Vec<_>>>()?;
        if members.iter().any(|member| member.state != "completed") {
            return Err(Error::Conflict(
                "all-required team join has incomplete members",
            ));
        }
        for (stored, joined) in members.iter().zip(receipt.members.iter()) {
            let joined_digest = format!("{:x}", Sha256::digest(joined.answer.as_bytes()));
            if stored.member_id != joined.id
                || stored.role != joined.role
                || stored.answer_sha256.as_deref() != Some(joined_digest.as_str())
                || stored.answer_truncated != joined.answer_truncated
            {
                return Err(Error::Conflict(
                    "team join member evidence differs from durable member receipt",
                ));
            }
        }
        let manifest = serde_json::json!({
            "publisher": "parent_agent",
            "members": members.iter().map(|member| serde_json::json!({
                "member_id": member.member_id,
                "role": member.role,
                "subagent_id": member.subagent_id,
                "child_session_id": member.child_session_id,
                "answer_sha256": member.answer_sha256,
                "answer_truncated": member.answer_truncated,
            })).collect::<Vec<_>>()
        });
        let at = now();
        db(tx.execute(
            "INSERT INTO team_join_receipts(team_run_id,policy,manifest_json,joined_sha256,created_at)
             VALUES(?1,'all_required',?2,?3,?4)",
            params![run_id, manifest.to_string(), receipt.joined_sha256, at],
        ))?;
        let changed = db(tx.execute(
            "UPDATE team_runs SET state='joined',settled_at=?1
             WHERE id=?2 AND state='running'",
            params![at, run_id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("team run changed before join"));
        }
        let joined_run = team_run_row(&tx, run_id)?;
        let join = TeamJoinInfo {
            team_run_id: run_id.to_owned(),
            policy: "all_required".into(),
            manifest,
            joined_sha256: receipt.joined_sha256.clone(),
            created_at: at,
        };
        db(tx.commit())?;
        Ok((joined_run, join))
    }

    #[cfg(feature = "routing")]
    pub(crate) fn reconstruct_team_join_in(
        c: &rusqlite::Connection,
        run_id: &str,
    ) -> Result<TeamJoinReceipt> {
        let run = team_run_row(c, run_id)?;
        if run.state != "joined" {
            return Err(Error::Conflict("team run is not durably joined"));
        }
        let team_id: String = db(c.query_row(
            "SELECT team_id FROM team_template_revisions WHERE revision_id=?1",
            [&run.revision_id],
            |r| r.get(0),
        ))?;
        let revision = team_revision_row(c, &team_id, Some(&run.revision_id))?;
        let mut imports = Vec::with_capacity(revision.definition.members.len());
        for member in &revision.definition.members {
            let stored = team_member_row(c, run_id, &member.id)?;
            if stored.state != "completed" {
                return Err(Error::Conflict("team review source has incomplete members"));
            }
            let child_session_id = stored
                .child_session_id
                .as_deref()
                .ok_or(Error::Storage("team member lost child Session provenance"))?;
            let child_session = session_in(c, child_session_id)?;
            let child_branch = branch_in(c, child_session_id, "main")?;
            let child_history = history_in(c, &child_session, &child_branch)?;
            if child_history.turns.len() != 1
                || child_history.turns[0].outcome != Outcome::Completed
            {
                return Err(Error::Conflict(
                    "team member child has no single authoritative completed answer",
                ));
            }
            let authoritative = &child_history.turns[0].answer;
            let answer = if stored.answer_truncated {
                if authoritative.len() <= MAX_TEAM_MEMBER_IMPORT_BYTES {
                    return Err(Error::Conflict(
                        "team member truncation receipt does not match authoritative answer",
                    ));
                }
                let mut end = MAX_TEAM_MEMBER_IMPORT_BYTES;
                while !authoritative.is_char_boundary(end) {
                    end -= 1;
                }
                authoritative[..end].to_owned()
            } else {
                if authoritative.len() > MAX_TEAM_MEMBER_IMPORT_BYTES {
                    return Err(Error::Conflict(
                        "team member untruncated receipt exceeds import bound",
                    ));
                }
                authoritative.clone()
            };
            let digest = format!("{:x}", Sha256::digest(answer.as_bytes()));
            if stored.answer_sha256.as_deref() != Some(digest.as_str()) {
                return Err(Error::Conflict(
                    "team member authoritative answer no longer matches durable receipt",
                ));
            }
            imports.push(TeamJoinMember {
                id: member.id.clone(),
                role: member.role.clone(),
                answer,
                answer_truncated: stored.answer_truncated,
            });
        }
        let rebuilt = team_error(join_team_members(&revision.definition, imports))?;
        let stored_digest: Option<String> = db(c
            .query_row(
                "SELECT joined_sha256 FROM team_join_receipts WHERE team_run_id=?1",
                [run_id],
                |r| r.get(0),
            )
            .optional())?;
        if stored_digest.as_deref() != Some(rebuilt.joined_sha256.as_str()) {
            return Err(Error::Conflict(
                "reconstructed team join differs from immutable join receipt",
            ));
        }
        Ok(rebuilt)
    }

    pub fn team_run(&self, actor: &PrincipalId, run_id: &str) -> Result<TeamRunInfo> {
        let c = self.connection()?;
        let run = team_run_row(&c, run_id)?;
        authorize(&c, &run.parent_session_id, actor, false)?;
        Ok(run)
    }

    pub fn team_run_members(
        &self,
        actor: &PrincipalId,
        run_id: &str,
    ) -> Result<Vec<TeamRunMemberInfo>> {
        let c = self.connection()?;
        let run = team_run_row(&c, run_id)?;
        authorize(&c, &run.parent_session_id, actor, false)?;
        let mut q = db(c.prepare(
            "SELECT member_id FROM team_run_members WHERE team_run_id=?1 ORDER BY ordinal",
        ))?;
        let rows = db(q.query_map([run_id], |r| r.get::<_, String>(0)))?;
        let ids = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        ids.into_iter()
            .map(|member| team_member_row(&c, run_id, &member))
            .collect()
    }

    pub fn team_join_receipt(
        &self,
        actor: &PrincipalId,
        run_id: &str,
    ) -> Result<Option<TeamJoinInfo>> {
        let c = self.connection()?;
        let run = team_run_row(&c, run_id)?;
        authorize(&c, &run.parent_session_id, actor, false)?;
        let row: Option<(String, String, String, i64)> = db(c
            .query_row(
                "SELECT policy,manifest_json,joined_sha256,created_at
                 FROM team_join_receipts WHERE team_run_id=?1",
                [run_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional())?;
        row.map(|(policy, manifest, joined_sha256, created_at)| {
            Ok(TeamJoinInfo {
                team_run_id: run_id.to_owned(),
                policy,
                manifest: decode(&manifest)?,
                joined_sha256,
                created_at,
            })
        })
        .transpose()
    }

    pub(crate) fn recover_teams_after_owner_restart(&self, c: &rusqlite::Connection) -> Result<()> {
        if self.trusted_local_direct() {
            return Ok(());
        }
        let at = now();
        db(c.execute(
            "UPDATE team_run_members
             SET state='interrupted',started_at=COALESCE(started_at,?1),settled_at=?1,
                 error='owner restarted before team member/join completion; no automatic replay'
             WHERE state IN ('pending','running')
               AND team_run_id IN (
                 SELECT id FROM team_runs WHERE state IN ('admitted','running')
               )",
            [at],
        ))?;
        db(c.execute(
            "UPDATE team_runs
             SET state='interrupted',started_at=COALESCE(started_at,?1),settled_at=?1,
                 failure_code='owner_restart_interrupted'
             WHERE state IN ('admitted','running')",
            [at],
        ))?;
        Ok(())
    }
}
