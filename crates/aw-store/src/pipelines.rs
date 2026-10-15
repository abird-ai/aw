use super::*;
use aw_pipeline::{
    apply_deterministic, definition_sha256, text_sha256, PipelineDefinition, PipelineStep,
};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_PIPELINE_RUNS: i64 = 100_000;
const MAX_PIPELINE_REVISIONS: i64 = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineRevisionInfo {
    pub pipeline_id: String,
    pub revision_id: String,
    pub revision_no: u64,
    pub name: String,
    pub owner_user_id: String,
    pub definition_sha256: String,
    pub definition: PipelineDefinition,
    pub created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineBindingScope {
    User,
    Session,
}

impl PipelineBindingScope {
    fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Session => "session",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineBindingMode {
    Gate,
    Transform,
    Observer,
    Task,
}
impl PipelineBindingMode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Gate => "gate",
            Self::Transform => "transform",
            Self::Observer => "observer",
            Self::Task => "task",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineBindingInfo {
    pub id: String,
    pub pipeline_id: String,
    pub revision_id: String,
    pub scope: PipelineBindingScope,
    pub scope_user_id: Option<String>,
    pub scope_session_id: Option<String>,
    pub mode: PipelineBindingMode,
    pub revision: u64,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineRunInfo {
    pub id: String,
    pub pipeline_id: String,
    pub revision_id: String,
    pub binding_id: Option<String>,
    pub actor_user_id: String,
    pub session_id: Option<String>,
    pub branch_id: Option<String>,
    pub request_id: String,
    pub state: String,
    pub run_revision: u64,
    pub next_step_index: usize,
    pub input: String,
    pub current: String,
    pub output: Option<String>,
    pub deadline_at: i64,
    pub failure_code: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineStepAttemptInfo {
    pub id: String,
    pub run_id: String,
    pub step_index: usize,
    pub step_id: String,
    pub kind: String,
    pub state: String,
    pub input_sha256: String,
    pub output_sha256: Option<String>,
    pub receipt: Option<serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineApprovalInfo {
    pub id: String,
    pub run_id: String,
    pub step_attempt_id: String,
    pub message: String,
    pub state: String,
    pub requested_by_user_id: String,
    pub decided_by_user_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PipelineDirective {
    Agent {
        run: PipelineRunInfo,
        attempt: PipelineStepAttemptInfo,
        role: String,
        input: String,
    },
    Approval {
        run: PipelineRunInfo,
        approval: PipelineApprovalInfo,
    },
    UnknownExternal {
        run: PipelineRunInfo,
        attempt: PipelineStepAttemptInfo,
    },
    Completed {
        run: PipelineRunInfo,
    },
    Failed {
        run: PipelineRunInfo,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineAgentReceipt {
    pub turn_id: u64,
    pub outcome: String,
    pub model: String,
    pub profile: String,
    pub session_id: Option<String>,
    pub branch_id: Option<String>,
    pub model_calls: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub calls_without_complete_usage: u64,
}

fn pipeline_error<T>(value: aw_pipeline::Result<T>) -> Result<T> {
    value.map_err(|error| match error {
        aw_pipeline::PipelineError::Definition(_) => Error::Config(error.to_string()),
        aw_pipeline::PipelineError::TextLimit => {
            Error::Limit("pipeline text exceeds configured bound")
        }
        aw_pipeline::PipelineError::CheckFailed => {
            Error::Denied("pipeline deterministic check failed")
        }
    })
}

fn stable_actor_user(c: &Connection, actor: &PrincipalId) -> Result<String> {
    ensure_user_for_principal(c, actor)
}

fn definition_row(
    c: &Connection,
    selector: &str,
    revision: Option<&str>,
) -> Result<PipelineRevisionInfo> {
    let row: Option<(String, String, i64, String, String, String, String, i64)> = if let Some(
        revision,
    ) = revision
    {
        db(c.query_row(
            "SELECT p.id,r.revision_id,r.revision_no,p.name,p.owner_user_id,r.definition_sha256,r.definition_json,r.created_at FROM pipelines p JOIN pipeline_revisions r ON r.pipeline_id=p.id WHERE (p.id=?1 OR p.name=?1) AND (r.revision_id=?2 OR CAST(r.revision_no AS TEXT)=?2) AND p.status='active'",
            params![selector,revision],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?)),
        ).optional())?
    } else {
        db(c.query_row(
            "SELECT p.id,r.revision_id,r.revision_no,p.name,p.owner_user_id,r.definition_sha256,r.definition_json,r.created_at FROM pipelines p JOIN pipeline_revisions r ON r.pipeline_id=p.id WHERE (p.id=?1 OR p.name=?1) AND p.status='active' ORDER BY r.revision_no DESC LIMIT 1",
            [selector],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?)),
        ).optional())?
    };
    let Some((
        pipeline_id,
        revision_id,
        revision_no,
        name,
        owner_user_id,
        digest,
        body,
        created_at,
    )) = row
    else {
        return Err(Error::Denied("pipeline unavailable or not permitted"));
    };
    let definition: PipelineDefinition = decode(&body)?;
    pipeline_error(aw_pipeline::validate_definition(&definition))?;
    Ok(PipelineRevisionInfo {
        pipeline_id,
        revision_id,
        revision_no: revision_no as u64,
        name,
        owner_user_id,
        definition_sha256: digest,
        definition,
        created_at,
    })
}

fn definition_row_owned(
    c: &Connection,
    owner_user_id: &str,
    selector: &str,
    revision: Option<&str>,
) -> Result<PipelineRevisionInfo> {
    let pipeline_id: Option<String> = db(c
        .query_row(
            "SELECT id FROM pipelines WHERE owner_user_id=?1 AND (id=?2 OR name=?2) AND status='active'",
            params![owner_user_id, selector],
            |r| r.get(0),
        )
        .optional())?;
    let pipeline_id = pipeline_id.ok_or(Error::Denied("pipeline unavailable or not permitted"))?;
    definition_row(c, &pipeline_id, revision)
}

fn run_row(c: &Connection, run_id: &str) -> Result<PipelineRunInfo> {
    let row:Option<(String,String,Option<String>,String,Option<String>,Option<String>,String,String,i64,i64,Vec<u8>,Vec<u8>,Option<Vec<u8>>,i64,Option<String>)>=db(c.query_row(
        "SELECT pipeline_id,revision_id,binding_id,actor_user_id,session_id,branch_id,request_id,state,run_revision,next_step_index,input_utf8,current_utf8,output_utf8,deadline_at,failure_code FROM pipeline_runs WHERE id=?1",
        [run_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?,r.get(11)?,r.get(12)?,r.get(13)?,r.get(14)?))).optional())?;
    let Some((
        pipeline_id,
        revision_id,
        binding_id,
        actor_user_id,
        session_id,
        branch_id,
        request_id,
        state,
        run_revision,
        next_step_index,
        input,
        current,
        output,
        deadline_at,
        failure_code,
    )) = row
    else {
        return Err(Error::Denied("pipeline run unavailable"));
    };
    Ok(PipelineRunInfo {
        id: run_id.to_owned(),
        pipeline_id,
        revision_id,
        binding_id,
        actor_user_id,
        session_id,
        branch_id,
        request_id,
        state,
        run_revision: run_revision as u64,
        next_step_index: next_step_index as usize,
        input: String::from_utf8(input)
            .map_err(|_| Error::Storage("pipeline input is not UTF-8"))?,
        current: String::from_utf8(current)
            .map_err(|_| Error::Storage("pipeline current value is not UTF-8"))?,
        output: output
            .map(String::from_utf8)
            .transpose()
            .map_err(|_| Error::Storage("pipeline output is not UTF-8"))?,
        deadline_at,
        failure_code,
    })
}

fn attempt_row(c: &Connection, attempt_id: &str) -> Result<PipelineStepAttemptInfo> {
    let row:Option<(String,i64,String,String,String,String,Option<String>,Option<String>)>=db(c.query_row("SELECT run_id,step_index,step_id,kind,state,input_sha256,output_sha256,receipt_json FROM pipeline_step_attempts WHERE id=?1",[attempt_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).optional())?;
    let Some((run_id, step_index, step_id, kind, state, input_sha256, output_sha256, receipt_json)) =
        row
    else {
        return Err(Error::Denied("pipeline step attempt unavailable"));
    };
    Ok(PipelineStepAttemptInfo {
        id: attempt_id.to_owned(),
        run_id,
        step_index: step_index as usize,
        step_id,
        kind,
        state,
        input_sha256,
        output_sha256,
        receipt: receipt_json
            .map(|v| {
                serde_json::from_str(&v)
                    .map_err(|_| Error::Storage("invalid pipeline step receipt"))
            })
            .transpose()?,
    })
}

fn approval_row(c: &Connection, id: &str) -> Result<PipelineApprovalInfo> {
    let row:Option<(String,String,String,String,String,Option<String>)>=db(c.query_row("SELECT run_id,step_attempt_id,message,state,requested_by_user_id,decided_by_user_id FROM pipeline_approvals WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional())?;
    let Some((run_id, step_attempt_id, message, state, requested_by_user_id, decided_by_user_id)) =
        row
    else {
        return Err(Error::Denied("pipeline approval unavailable"));
    };
    Ok(PipelineApprovalInfo {
        id: id.to_owned(),
        run_id,
        step_attempt_id,
        message,
        state,
        requested_by_user_id,
        decided_by_user_id,
    })
}

fn authorize_run(
    c: &Connection,
    actor: &PrincipalId,
    run_id: &str,
) -> Result<(String, PipelineRunInfo)> {
    let user = stable_actor_user(c, actor)?;
    let run = run_row(c, run_id)?;
    if run.actor_user_id != user {
        return Err(Error::Denied("pipeline run unavailable"));
    }
    Ok((user, run))
}

fn request_fingerprint(
    revision_id: &str,
    binding_id: Option<&str>,
    session: Option<&str>,
    branch: Option<&str>,
    input: &str,
) -> String {
    let mut h = Sha256::new();
    h.update(b"aw-pipeline-run-v1\0");
    h.update(revision_id.as_bytes());
    h.update(b"\0");
    h.update(binding_id.unwrap_or("").as_bytes());
    h.update(b"\0");
    h.update(session.unwrap_or("").as_bytes());
    h.update(b"\0");
    h.update(branch.unwrap_or("").as_bytes());
    h.update(b"\0");
    h.update(input.as_bytes());
    format!("{:x}", h.finalize())
}

fn binding_mode(value: &str) -> Result<PipelineBindingMode> {
    match value {
        "gate" => Ok(PipelineBindingMode::Gate),
        "transform" => Ok(PipelineBindingMode::Transform),
        "observer" => Ok(PipelineBindingMode::Observer),
        "task" => Ok(PipelineBindingMode::Task),
        _ => Err(Error::Storage("invalid pipeline binding mode")),
    }
}
fn binding_scope(value: &str) -> Result<PipelineBindingScope> {
    match value {
        "user" => Ok(PipelineBindingScope::User),
        "session" => Ok(PipelineBindingScope::Session),
        _ => Err(Error::Storage("invalid pipeline binding scope")),
    }
}

impl SessionStore {
    pub fn pipeline_define(
        &self,
        actor: &PrincipalId,
        definition: PipelineDefinition,
    ) -> Result<PipelineRevisionInfo> {
        pipeline_error(aw_pipeline::validate_definition(&definition))?;
        let digest = pipeline_error(definition_sha256(&definition))?;
        let body = json(&definition)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = stable_actor_user(&tx, actor)?;
        let existing: Option<String> = db(tx
            .query_row(
                "SELECT id FROM pipelines WHERE owner_user_id=?1 AND name=?2 AND status='active'",
                params![user, definition.name],
                |r| r.get(0),
            )
            .optional())?;
        let pipeline_id = match existing {
            Some(id) => id,
            None => {
                let id = id("pipe")?;
                db(tx.execute("INSERT INTO pipelines(id,owner_user_id,name,status,created_at) VALUES(?1,?2,?3,'active',?4)",params![id,user,definition.name,now()]))?;
                id
            }
        };
        if let Some(revision_id)=db(tx.query_row("SELECT revision_id FROM pipeline_revisions WHERE pipeline_id=?1 AND definition_sha256=?2",params![pipeline_id,digest],|r|r.get::<_,String>(0)).optional())?{let out=definition_row(&tx,&pipeline_id,Some(&revision_id))?;db(tx.commit())?;return Ok(out);}
        let count: i64 = db(tx.query_row(
            "SELECT count(*) FROM pipeline_revisions WHERE pipeline_id=?1",
            [&pipeline_id],
            |r| r.get(0),
        ))?;
        if count >= MAX_PIPELINE_REVISIONS {
            return Err(Error::Limit("pipeline revision limit reached"));
        }
        let revision_no: i64 = db(tx.query_row(
            "SELECT coalesce(max(revision_no),0)+1 FROM pipeline_revisions WHERE pipeline_id=?1",
            [&pipeline_id],
            |r| r.get(0),
        ))?;
        let revision_id = id("prev")?;
        let at = now();
        db(tx.execute("INSERT INTO pipeline_revisions(revision_id,pipeline_id,revision_no,definition_sha256,definition_json,created_by_user_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![revision_id,pipeline_id,revision_no,digest,body,user,at]))?;
        let out = definition_row(&tx, &pipeline_id, Some(&revision_id))?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn pipeline_revision(
        &self,
        actor: &PrincipalId,
        selector: &str,
        revision: Option<&str>,
    ) -> Result<PipelineRevisionInfo> {
        let c = self.connection()?;
        let user = stable_actor_user(&c, actor)?;
        let info = definition_row_owned(&c, &user, selector, revision)?;
        Ok(info)
    }

    pub fn pipelines(&self, actor: &PrincipalId) -> Result<Vec<PipelineRevisionInfo>> {
        let c = self.connection()?;
        let user = stable_actor_user(&c, actor)?;
        let mut q=db(c.prepare("SELECT p.id FROM pipelines p WHERE p.owner_user_id=?1 AND p.status='active' ORDER BY p.name,p.id LIMIT 512"))?;
        let rows = db(q.query_map([user], |r| r.get::<_, String>(0)))?;
        let ids = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        ids.into_iter()
            .map(|id| definition_row(&c, &id, None))
            .collect()
    }

    pub fn pipeline_bind(
        &self,
        actor: &PrincipalId,
        selector: &str,
        revision: Option<&str>,
        scope: PipelineBindingScope,
        scope_id: &str,
        mode: PipelineBindingMode,
    ) -> Result<PipelineBindingInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = stable_actor_user(&tx, actor)?;
        let def = definition_row_owned(&tx, &user, selector, revision)?;
        let (scope_user, scope_session) = match scope {
            PipelineBindingScope::User => {
                let target = if scope_id == "self" {
                    user.clone()
                } else {
                    scope_id.to_owned()
                };
                if target != user {
                    return Err(Error::Denied("Phase 39 user bindings are owner-self only"));
                }
                (Some(target), None)
            }
            PipelineBindingScope::Session => {
                let session = session_in(&tx, scope_id)?;
                if !principal_is_session_owner(&tx, &session.id, actor)? {
                    return Err(Error::Denied("Session owner required to bind pipeline"));
                }
                (None, Some(session.id))
            }
        };
        let id = id("pbind")?;
        let at = now();
        db(tx.execute("INSERT INTO pipeline_bindings(id,pipeline_id,revision_id,scope_kind,scope_user_id,scope_session_id,mode,trigger_kind,revision,status,created_by_user_id,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,'manual',1,'active',?8,?9,?9)",params![id,def.pipeline_id,def.revision_id,scope.as_str(),scope_user,scope_session,mode.as_str(),user,at]))?;
        let out = PipelineBindingInfo {
            id: id.clone(),
            pipeline_id: def.pipeline_id,
            revision_id: def.revision_id,
            scope,
            scope_user_id: scope_user,
            scope_session_id: scope_session,
            mode,
            revision: 1,
            status: "active".into(),
        };
        db(tx.commit())?;
        Ok(out)
    }

    pub fn pipeline_binding_set(
        &self,
        actor: &PrincipalId,
        binding_id: &str,
        expected_revision: u64,
        enabled: bool,
        revision_id: Option<&str>,
    ) -> Result<PipelineBindingInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = stable_actor_user(&tx, actor)?;
        let row:Option<(String,String,String,Option<String>,Option<String>,String,i64)>=db(tx.query_row("SELECT b.pipeline_id,p.owner_user_id,b.scope_kind,b.scope_user_id,b.scope_session_id,b.mode,b.revision FROM pipeline_bindings b JOIN pipelines p ON p.id=b.pipeline_id WHERE b.id=?1",[binding_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional())?;
        let Some((pipeline_id, owner, scope_kind, scope_user_id, scope_session_id, mode, revision)) =
            row
        else {
            return Err(Error::Denied("pipeline binding unavailable"));
        };
        if owner != user {
            return Err(Error::Denied("only pipeline owner may update binding"));
        }
        if revision as u64 != expected_revision {
            return Err(Error::Conflict("pipeline binding revision changed"));
        }
        let next_revision_id = if let Some(rid) = revision_id {
            let d = definition_row(&tx, &pipeline_id, Some(rid))?;
            d.revision_id
        } else {
            db(tx.query_row(
                "SELECT revision_id FROM pipeline_bindings WHERE id=?1",
                [binding_id],
                |r| r.get(0),
            ))?
        };
        let changed=db(tx.execute("UPDATE pipeline_bindings SET revision_id=?1,revision=revision+1,status=?2,updated_at=?3 WHERE id=?4 AND revision=?5",params![next_revision_id,if enabled{"active"}else{"disabled"},now(),binding_id,expected_revision]))?;
        if changed != 1 {
            return Err(Error::Conflict("pipeline binding revision changed"));
        }
        let out = PipelineBindingInfo {
            id: binding_id.to_owned(),
            pipeline_id,
            revision_id: next_revision_id,
            scope: binding_scope(&scope_kind)?,
            scope_user_id,
            scope_session_id,
            mode: binding_mode(&mode)?,
            revision: expected_revision + 1,
            status: if enabled {
                "active".into()
            } else {
                "disabled".into()
            },
        };
        db(tx.commit())?;
        Ok(out)
    }

    pub fn pipeline_bindings(&self, actor: &PrincipalId) -> Result<Vec<PipelineBindingInfo>> {
        let c = self.connection()?;
        let user = stable_actor_user(&c, actor)?;
        let mut q=db(c.prepare("SELECT b.id,b.pipeline_id,b.revision_id,b.scope_kind,b.scope_user_id,b.scope_session_id,b.mode,b.revision,b.status FROM pipeline_bindings b JOIN pipelines p ON p.id=b.pipeline_id WHERE p.owner_user_id=?1 ORDER BY b.created_at,b.id LIMIT 1024"))?;
        let rows = db(q.query_map([user], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, String>(8)?,
            ))
        }))?;
        let mut out = Vec::new();
        for row in rows {
            let (
                id,
                pipeline_id,
                revision_id,
                scope,
                scope_user_id,
                scope_session_id,
                mode,
                revision,
                status,
            ) = db(row)?;
            out.push(PipelineBindingInfo {
                id,
                pipeline_id,
                revision_id,
                scope: binding_scope(&scope)?,
                scope_user_id,
                scope_session_id,
                mode: binding_mode(&mode)?,
                revision: revision as u64,
                status,
            });
        }
        Ok(out)
    }

    pub fn pipeline_admit_manual(
        &self,
        actor: &PrincipalId,
        selector: &str,
        revision: Option<&str>,
        binding_id: Option<&str>,
        request_id: &str,
        input: &str,
        session: Option<&str>,
        branch: Option<&str>,
    ) -> Result<PipelineRunInfo> {
        if request_id.starts_with("sched:") {
            return Err(Error::Config(
                "pipeline request IDs beginning with sched: are reserved for durable schedule occurrences".into(),
            ));
        }
        self.pipeline_admit(
            actor, selector, revision, binding_id, request_id, input, session, branch,
        )
    }

    pub(crate) fn pipeline_admit_scheduled(
        &self,
        actor: &PrincipalId,
        selector: &str,
        revision: Option<&str>,
        binding_id: Option<&str>,
        request_id: &str,
        input: &str,
    ) -> Result<PipelineRunInfo> {
        if !request_id.starts_with("sched:") {
            return Err(Error::Config(
                "scheduled pipeline admission requires the reserved sched: request namespace"
                    .into(),
            ));
        }
        self.pipeline_admit(
            actor, selector, revision, binding_id, request_id, input, None, None,
        )
    }

    fn pipeline_admit(
        &self,
        actor: &PrincipalId,
        selector: &str,
        revision: Option<&str>,
        binding_id: Option<&str>,
        request_id: &str,
        input: &str,
        session: Option<&str>,
        branch: Option<&str>,
    ) -> Result<PipelineRunInfo> {
        if request_id.is_empty()
            || request_id.len() > 128
            || !request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
        {
            return Err(Error::Config("invalid pipeline request ID".into()));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = stable_actor_user(&tx, actor)?;
        let (def, selected_binding) = if let Some(binding_id) = binding_id {
            let row:Option<(String,String,String,Option<String>,Option<String>,String,String)>=db(tx.query_row("SELECT pipeline_id,revision_id,scope_kind,scope_user_id,scope_session_id,mode,status FROM pipeline_bindings WHERE id=?1",[binding_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional())?;
            let Some((
                pipeline_id,
                bound_revision_id,
                scope_kind,
                scope_user,
                scope_session,
                mode,
                status,
            )) = row
            else {
                return Err(Error::Denied("pipeline binding unavailable"));
            };
            if status != "active" || mode != "task" {
                return Err(Error::Denied(
                    "pipeline binding is not active task/manual authority",
                ));
            }
            let def = definition_row(&tx, &pipeline_id, Some(&bound_revision_id))?;
            if selector != def.pipeline_id && selector != def.name {
                return Err(Error::Denied("pipeline selector does not match binding"));
            }
            if revision.is_some_and(|requested| {
                requested != def.revision_id && requested != def.revision_no.to_string()
            }) {
                return Err(Error::Denied(
                    "requested pipeline revision differs from binding",
                ));
            }
            match scope_kind.as_str() {
                "user" if scope_user.as_deref() == Some(&user) => {}
                "session" => {
                    let target = scope_session.ok_or(Error::Denied("invalid session binding"))?;
                    if session != Some(target.as_str()) {
                        return Err(Error::Denied(
                            "pipeline binding requires exact Session target",
                        ));
                    }
                    authorize(&tx, &target, actor, false)?;
                }
                _ => return Err(Error::Denied("pipeline binding does not authorize caller")),
            }
            (def, Some(binding_id.to_owned()))
        } else {
            (definition_row_owned(&tx, &user, selector, revision)?, None)
        };
        if input.len() > def.definition.limits.max_input_bytes as usize {
            return Err(Error::Limit("pipeline input exceeds revision limit"));
        }
        let target_branch = if let Some(session) = session {
            authorize(&tx, session, actor, false)?;
            let branch = branch.ok_or_else(|| {
                Error::Config("Session-scoped pipeline run requires exact branch".into())
            })?;
            Some(branch_in(&tx, session, branch)?.id)
        } else {
            if branch.is_some() {
                return Err(Error::Config("pipeline branch requires Session".into()));
            }
            None
        };
        let fp = request_fingerprint(
            &def.revision_id,
            selected_binding.as_deref(),
            session,
            target_branch.as_deref(),
            input,
        );
        if let Some((existing_fp,run_id))=db(tx.query_row("SELECT request_fingerprint,id FROM pipeline_runs WHERE actor_user_id=?1 AND request_id=?2",params![user,request_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).optional())?{if existing_fp!=fp{return Err(Error::Conflict("pipeline request ID already belongs to different input/revision"));}let out=run_row(&tx,&run_id)?;db(tx.commit())?;return Ok(out);}
        let count: i64 = db(tx.query_row("SELECT count(*) FROM pipeline_runs", [], |r| r.get(0)))?;
        if count >= MAX_PIPELINE_RUNS {
            return Err(Error::Limit("pipeline run retention limit reached"));
        }
        let run_id = id("prun")?;
        let at = now();
        let deadline = at.saturating_add(def.definition.limits.max_wall_seconds as i64 * 1_000_000);
        db(tx.execute("INSERT INTO pipeline_runs(id,pipeline_id,revision_id,binding_id,actor_user_id,session_id,branch_id,request_id,request_fingerprint,state,run_revision,next_step_index,input_utf8,current_utf8,deadline_at,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'admitted',1,0,?10,?10,?11,?12,?12)",params![run_id,def.pipeline_id,def.revision_id,selected_binding,user,session,target_branch,request_id,fp,input.as_bytes(),deadline,at]))?;
        let out = run_row(&tx, &run_id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn pipeline_run(&self, actor: &PrincipalId, run_id: &str) -> Result<PipelineRunInfo> {
        let c = self.connection()?;
        authorize_run(&c, actor, run_id).map(|(_, run)| run)
    }

    pub fn pipeline_next(&self, actor: &PrincipalId, run_id: &str) -> Result<PipelineDirective> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (_, mut run) = authorize_run(&tx, actor, run_id)?;
        if run.state == "completed" {
            db(tx.commit())?;
            return Ok(PipelineDirective::Completed { run });
        }
        if run.state == "failed" || run.state == "cancelled" {
            db(tx.commit())?;
            return Ok(PipelineDirective::Failed { run });
        }
        // Once an external agent step has a durable started intent, preserve
        // uncertainty ahead of wall-clock timeout. A deadline must never erase
        // evidence that provider effects may already have happened.
        if let Some(attempt_id) = db(tx
            .query_row(
                "SELECT id FROM pipeline_step_attempts WHERE run_id=?1 AND step_index=?2 AND state='started'",
                params![run_id, run.next_step_index],
                |r| r.get::<_, String>(0),
            )
            .optional())?
        {
            let attempt = attempt_row(&tx, &attempt_id)?;
            db(tx.commit())?;
            return Ok(PipelineDirective::UnknownExternal { run, attempt });
        }
        if now() > run.deadline_at {
            db(tx.execute("UPDATE pipeline_runs SET state='failed',failure_code='deadline_exceeded',run_revision=run_revision+1,updated_at=?1 WHERE id=?2",params![now(),run_id]))?;
            run = run_row(&tx, run_id)?;
            db(tx.commit())?;
            return Ok(PipelineDirective::Failed { run });
        }
        let def = definition_row(&tx, &run.pipeline_id, Some(&run.revision_id))?;
        loop {
            if run.next_step_index >= def.definition.steps.len() {
                db(tx.execute("UPDATE pipeline_runs SET state='completed',output_utf8=current_utf8,run_revision=run_revision+1,updated_at=?1 WHERE id=?2",params![now(),run_id]))?;
                run = run_row(&tx, run_id)?;
                db(tx.commit())?;
                return Ok(PipelineDirective::Completed { run });
            }
            if let Some(attempt_id) = db(tx
                .query_row(
                    "SELECT id FROM pipeline_step_attempts WHERE run_id=?1 AND step_index=?2",
                    params![run_id, run.next_step_index],
                    |r| r.get::<_, String>(0),
                )
                .optional())?
            {
                let attempt = attempt_row(&tx, &attempt_id)?;
                match attempt.state.as_str() {
                    "started" => {
                        db(tx.commit())?;
                        return Ok(PipelineDirective::UnknownExternal { run, attempt });
                    }
                    "waiting_approval" => {
                        let approval_id: String = db(tx.query_row(
                            "SELECT id FROM pipeline_approvals WHERE step_attempt_id=?1",
                            [attempt_id],
                            |r| r.get(0),
                        ))?;
                        let approval = approval_row(&tx, &approval_id)?;
                        db(tx.commit())?;
                        return Ok(PipelineDirective::Approval { run, approval });
                    }
                    "failed" => {
                        db(tx.commit())?;
                        return Ok(PipelineDirective::Failed { run });
                    }
                    "completed" => {
                        return Err(Error::Storage(
                            "pipeline completed step did not advance cursor",
                        ))
                    }
                    _ => return Err(Error::Storage("invalid pipeline step state")),
                }
            }
            let step = &def.definition.steps[run.next_step_index];
            let input = run.current.clone();
            let input_digest = text_sha256(&input);
            match step {
                PipelineStep::Check { .. } | PipelineStep::Transform { .. } => {
                    let attempt_id = id("pstep")?;
                    let at = now();
                    match apply_deterministic(
                        step,
                        &input,
                        def.definition.limits.max_output_bytes as usize,
                    ) {
                        Ok((output, receipt)) => {
                            db(tx.execute("INSERT INTO pipeline_step_attempts(id,run_id,step_index,step_id,kind,state,input_sha256,output_sha256,receipt_json,started_at,settled_at) VALUES(?1,?2,?3,?4,?5,'completed',?6,?7,?8,?9,?9)",params![attempt_id,run_id,run.next_step_index,step.id(),step.kind_name(),receipt.input_sha256,receipt.output_sha256,json(&receipt)?,at]))?;
                            db(tx.execute("UPDATE pipeline_runs SET current_utf8=?1,next_step_index=next_step_index+1,run_revision=run_revision+1,updated_at=?2 WHERE id=?3",params![output.as_bytes(),at,run_id]))?;
                            run = run_row(&tx, run_id)?;
                        }
                        Err(error) => {
                            let code = match error {
                                aw_pipeline::PipelineError::CheckFailed => "check_failed",
                                aw_pipeline::PipelineError::TextLimit => "text_limit",
                                aw_pipeline::PipelineError::Definition(_) => "invalid_definition",
                            };
                            db(tx.execute("INSERT INTO pipeline_step_attempts(id,run_id,step_index,step_id,kind,state,input_sha256,receipt_json,started_at,settled_at) VALUES(?1,?2,?3,?4,?5,'failed',?6,?7,?8,?8)",params![attempt_id,run_id,run.next_step_index,step.id(),step.kind_name(),input_digest,serde_json::json!({"error":error.to_string()}).to_string(),at]))?;
                            db(tx.execute("UPDATE pipeline_runs SET state='failed',failure_code=?1,run_revision=run_revision+1,updated_at=?2 WHERE id=?3",params![code,at,run_id]))?;
                            run = run_row(&tx, run_id)?;
                            db(tx.commit())?;
                            return Ok(PipelineDirective::Failed { run });
                        }
                    }
                }
                PipelineStep::Approval {
                    id: step_id,
                    message,
                } => {
                    let attempt_id = id("pstep")?;
                    let approval_id = id("pappr")?;
                    let user = run.actor_user_id.clone();
                    let at = now();
                    db(tx.execute("INSERT INTO pipeline_step_attempts(id,run_id,step_index,step_id,kind,state,input_sha256,started_at) VALUES(?1,?2,?3,?4,'approval','waiting_approval',?5,?6)",params![attempt_id,run_id,run.next_step_index,step_id,input_digest,at]))?;
                    db(tx.execute("INSERT INTO pipeline_approvals(id,run_id,step_attempt_id,message,state,requested_by_user_id,created_at) VALUES(?1,?2,?3,?4,'waiting',?5,?6)",params![approval_id,run_id,attempt_id,message,user,at]))?;
                    db(tx.execute("UPDATE pipeline_runs SET state='waiting_approval',run_revision=run_revision+1,updated_at=?1 WHERE id=?2",params![at,run_id]))?;
                    run = run_row(&tx, run_id)?;
                    let approval = approval_row(&tx, &approval_id)?;
                    db(tx.commit())?;
                    return Ok(PipelineDirective::Approval { run, approval });
                }
                PipelineStep::Agent { id: step_id, role } => {
                    let attempt_id = id("pstep")?;
                    let at = now();
                    db(tx.execute("INSERT INTO pipeline_step_attempts(id,run_id,step_index,step_id,kind,state,input_sha256,started_at) VALUES(?1,?2,?3,?4,'agent','started',?5,?6)",params![attempt_id,run_id,run.next_step_index,step_id,input_digest,at]))?;
                    db(tx.execute("UPDATE pipeline_runs SET state='running_external',run_revision=run_revision+1,updated_at=?1 WHERE id=?2",params![at,run_id]))?;
                    run = run_row(&tx, run_id)?;
                    let attempt = attempt_row(&tx, &attempt_id)?;
                    let role = role.clone();
                    db(tx.commit())?;
                    return Ok(PipelineDirective::Agent {
                        run,
                        attempt,
                        role,
                        input,
                    });
                }
                PipelineStep::Emit { id: step_id } => {
                    let attempt_id = id("pstep")?;
                    let at = now();
                    let digest = text_sha256(&input);
                    db(tx.execute("INSERT INTO pipeline_step_attempts(id,run_id,step_index,step_id,kind,state,input_sha256,output_sha256,receipt_json,started_at,settled_at) VALUES(?1,?2,?3,?4,'emit','completed',?5,?5,?6,?7,?7)",params![attempt_id,run_id,run.next_step_index,step_id,digest,serde_json::json!({"output_sha256":digest}).to_string(),at]))?;
                    db(tx.execute("UPDATE pipeline_runs SET state='completed',output_utf8=current_utf8,next_step_index=next_step_index+1,run_revision=run_revision+1,updated_at=?1 WHERE id=?2",params![at,run_id]))?;
                    run = run_row(&tx, run_id)?;
                    db(tx.commit())?;
                    return Ok(PipelineDirective::Completed { run });
                }
            }
        }
    }

    pub fn pipeline_note_agent_target(
        &self,
        actor: &PrincipalId,
        run_id: &str,
        attempt_id: &str,
        session_id: &str,
        branch_id: &str,
    ) -> Result<PipelineStepAttemptInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (_, run) = authorize_run(&tx, actor, run_id)?;
        let attempt = attempt_row(&tx, attempt_id)?;
        if attempt.run_id != run_id
            || attempt.step_index != run.next_step_index
            || attempt.state != "started"
            || attempt.kind != "agent"
        {
            return Err(Error::Conflict("pipeline agent attempt no longer current"));
        }
        authorize(&tx, session_id, actor, false)?;
        let _ = branch_in(&tx, session_id, branch_id)?;
        let changed = db(tx.execute(
            "UPDATE pipeline_step_attempts
             SET receipt_json=?1
             WHERE id=?2 AND state='started' AND receipt_json IS NULL",
            params![
                serde_json::json!({
                    "intent": "agent",
                    "session_id": session_id,
                    "branch_id": branch_id,
                })
                .to_string(),
                attempt_id
            ],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict(
                "pipeline agent target already recorded or attempt settled",
            ));
        }
        let out = attempt_row(&tx, attempt_id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn pipeline_settle_agent(
        &self,
        actor: &PrincipalId,
        run_id: &str,
        attempt_id: &str,
        output: &str,
        receipt: &PipelineAgentReceipt,
    ) -> Result<PipelineRunInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (_, run) = authorize_run(&tx, actor, run_id)?;
        if output.len() > aw_pipeline::MAX_PIPELINE_TEXT_BYTES {
            return Err(Error::Limit("pipeline agent output exceeds bound"));
        }
        let attempt = attempt_row(&tx, attempt_id)?;
        if attempt.run_id != run_id
            || attempt.step_index != run.next_step_index
            || attempt.state != "started"
            || attempt.kind != "agent"
        {
            return Err(Error::Conflict("pipeline agent attempt no longer current"));
        }
        let at = now();
        let digest = text_sha256(output);
        let changed=db(tx.execute("UPDATE pipeline_step_attempts SET state='completed',output_sha256=?1,receipt_json=?2,settled_at=?3 WHERE id=?4 AND state='started'",params![digest,json(receipt)?,at,attempt_id]))?;
        if changed != 1 {
            return Err(Error::Conflict("pipeline agent attempt already settled"));
        }
        let advanced = db(tx.execute(
            "UPDATE pipeline_runs
             SET state='admitted',current_utf8=?1,next_step_index=next_step_index+1,
                 run_revision=run_revision+1,updated_at=?2
             WHERE id=?3 AND state='running_external' AND next_step_index=?4",
            params![output.as_bytes(), at, run_id, run.next_step_index],
        ))?;
        if advanced != 1 {
            return Err(Error::Conflict(
                "pipeline run changed before agent settlement",
            ));
        }
        let out = run_row(&tx, run_id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn pipeline_fail_agent(
        &self,
        actor: &PrincipalId,
        run_id: &str,
        attempt_id: &str,
        code: &str,
    ) -> Result<PipelineRunInfo> {
        if code.is_empty()
            || code.len() > 64
            || !code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(Error::Config("invalid pipeline failure code".into()));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (_, run) = authorize_run(&tx, actor, run_id)?;
        let attempt = attempt_row(&tx, attempt_id)?;
        if attempt.run_id != run_id
            || attempt.step_index != run.next_step_index
            || attempt.state != "started"
        {
            return Err(Error::Conflict("pipeline agent attempt no longer current"));
        }
        let at = now();
        let mut failure_receipt = attempt
            .receipt
            .clone()
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(object) = failure_receipt.as_object_mut() {
            object.insert(
                "failure_code".into(),
                serde_json::Value::String(code.to_owned()),
            );
        } else {
            failure_receipt = serde_json::json!({"failure_code":code});
        }
        let settled = db(tx.execute(
            "UPDATE pipeline_step_attempts
             SET state='failed',receipt_json=?1,settled_at=?2
             WHERE id=?3 AND state='started'",
            params![failure_receipt.to_string(), at, attempt_id],
        ))?;
        if settled != 1 {
            return Err(Error::Conflict("pipeline agent attempt already settled"));
        }
        let failed = db(tx.execute(
            "UPDATE pipeline_runs
             SET state='failed',failure_code=?1,run_revision=run_revision+1,updated_at=?2
             WHERE id=?3 AND state='running_external' AND next_step_index=?4",
            params![code, at, run_id, run.next_step_index],
        ))?;
        if failed != 1 {
            return Err(Error::Conflict(
                "pipeline run changed before agent failure settlement",
            ));
        }
        let out = run_row(&tx, run_id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn pipeline_approve(
        &self,
        actor: &PrincipalId,
        approval_id: &str,
        approve: bool,
    ) -> Result<PipelineRunInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let approval = approval_row(&tx, approval_id)?;
        let (user, run) = authorize_run(&tx, actor, &approval.run_id)?;
        if approval.requested_by_user_id != user {
            return Err(Error::Denied(
                "Phase 39 approval belongs to initiating user",
            ));
        }
        if approval.state != "waiting" || run.state != "waiting_approval" {
            return Err(Error::Conflict("pipeline approval no longer waiting"));
        }
        let at = now();
        let state = if approve { "approved" } else { "denied" };
        db(tx.execute("UPDATE pipeline_approvals SET state=?1,decided_by_user_id=?2,decided_at=?3 WHERE id=?4 AND state='waiting'",params![state,user,at,approval_id]))?;
        if approve {
            db(tx.execute("UPDATE pipeline_step_attempts SET state='completed',output_sha256=input_sha256,receipt_json=?1,settled_at=?2 WHERE id=?3 AND state='waiting_approval'",params![serde_json::json!({"decision":"approved","approver_user_id":user}).to_string(),at,approval.step_attempt_id]))?;
            db(tx.execute("UPDATE pipeline_runs SET state='admitted',next_step_index=next_step_index+1,run_revision=run_revision+1,updated_at=?1 WHERE id=?2",params![at,run.id]))?;
        } else {
            db(tx.execute("UPDATE pipeline_step_attempts SET state='failed',receipt_json=?1,settled_at=?2 WHERE id=?3 AND state='waiting_approval'",params![serde_json::json!({"decision":"denied","approver_user_id":user}).to_string(),at,approval.step_attempt_id]))?;
            db(tx.execute("UPDATE pipeline_runs SET state='failed',failure_code='approval_denied',run_revision=run_revision+1,updated_at=?1 WHERE id=?2",params![at,run.id]))?;
        }
        let out = run_row(&tx, &run.id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn pipeline_steps(
        &self,
        actor: &PrincipalId,
        run_id: &str,
    ) -> Result<Vec<PipelineStepAttemptInfo>> {
        let c = self.connection()?;
        authorize_run(&c, actor, run_id)?;
        let mut q = db(
            c.prepare("SELECT id FROM pipeline_step_attempts WHERE run_id=?1 ORDER BY step_index")
        )?;
        let rows = db(q.query_map([run_id], |r| r.get::<_, String>(0)))?;
        let ids = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        ids.into_iter().map(|id| attempt_row(&c, &id)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_pipeline::{CheckSpec, PipelineLimits, TransformSpec};
    use std::path::PathBuf;

    fn binding() -> SessionBinding {
        SessionBinding {
            working_directory: PathBuf::from("/tmp"),
            tool_root: PathBuf::from("/tmp"),
            system_prompt: "system".into(),
            profile_hint: "fixture".into(),
            model_hint: "fixture".into(),
            destination_fingerprint: "a".repeat(64),
        }
    }
    fn def() -> PipelineDefinition {
        PipelineDefinition {
            name: "phase39".into(),
            description: String::new(),
            limits: PipelineLimits::default(),
            steps: vec![
                PipelineStep::Check {
                    id: "nonempty".into(),
                    check: CheckSpec::NonEmpty,
                },
                PipelineStep::Transform {
                    id: "trim".into(),
                    transform: TransformSpec::Trim,
                },
                PipelineStep::Approval {
                    id: "approval".into(),
                    message: "Continue".into(),
                },
                PipelineStep::Agent {
                    id: "agent".into(),
                    role: "main".into(),
                },
                PipelineStep::Emit { id: "emit".into() },
            ],
        }
    }
    fn setup() -> (SessionStore, PrincipalId) {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("internal:pipeline:alice").unwrap();
        (store, actor)
    }
    #[test]
    fn revisions_are_immutable_and_deduplicate_exact_definition() {
        let (store, a) = setup();
        let first = store.pipeline_define(&a, def()).unwrap();
        let same = store.pipeline_define(&a, def()).unwrap();
        assert_eq!(first.revision_id, same.revision_id);
        let c = store.connection().unwrap();
        assert!(c
            .execute(
                "UPDATE pipeline_revisions SET revision_no=9 WHERE revision_id=?1",
                [first.revision_id]
            )
            .is_err());
    }
    #[test]
    fn binding_cas_and_manual_scope_are_explicit() {
        let (store, a) = setup();
        let p = store.pipeline_define(&a, def()).unwrap();
        let b = store
            .pipeline_bind(
                &a,
                &p.pipeline_id,
                None,
                PipelineBindingScope::User,
                "self",
                PipelineBindingMode::Task,
            )
            .unwrap();
        assert!(store
            .pipeline_binding_set(&a, &b.id, 9, false, None)
            .is_err());
        let off = store
            .pipeline_binding_set(&a, &b.id, 1, false, None)
            .unwrap();
        assert_eq!(off.revision, 2);
        assert_eq!(off.status, "disabled");
    }
    #[test]
    fn manual_pipeline_request_cannot_preclaim_schedule_namespace() {
        let (store, a) = setup();
        let p = store.pipeline_define(&a, def()).unwrap();
        assert!(store
            .pipeline_admit_manual(
                &a,
                &p.pipeline_id,
                None,
                None,
                "sched:forged:1",
                "hello",
                None,
                None,
            )
            .is_err());
    }

    #[test]
    fn exact_request_dedup_and_changed_input_conflict() {
        let (store, a) = setup();
        let p = store.pipeline_define(&a, def()).unwrap();
        let one = store
            .pipeline_admit_manual(&a, &p.pipeline_id, None, None, "req1", " hi ", None, None)
            .unwrap();
        let two = store
            .pipeline_admit_manual(&a, &p.pipeline_id, None, None, "req1", " hi ", None, None)
            .unwrap();
        assert_eq!(one.id, two.id);
        assert!(matches!(
            store.pipeline_admit_manual(
                &a,
                &p.pipeline_id,
                None,
                None,
                "req1",
                "different",
                None,
                None
            ),
            Err(Error::Conflict(_))
        ));
    }
    #[test]
    fn deterministic_steps_pause_for_approval_then_agent_then_emit() {
        let (store, a) = setup();
        let p = store.pipeline_define(&a, def()).unwrap();
        let run = store
            .pipeline_admit_manual(&a, &p.pipeline_id, None, None, "req2", " hi ", None, None)
            .unwrap();
        let approval = match store.pipeline_next(&a, &run.id).unwrap() {
            PipelineDirective::Approval { run, approval } => {
                assert_eq!(run.current, "hi");
                approval
            }
            other => panic!("unexpected {other:?}"),
        };
        store.pipeline_approve(&a, &approval.id, true).unwrap();
        let (agent_attempt, input) = match store.pipeline_next(&a, &run.id).unwrap() {
            PipelineDirective::Agent { attempt, input, .. } => (attempt, input),
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(input, "hi");
        assert!(matches!(
            store.pipeline_next(&a, &run.id).unwrap(),
            PipelineDirective::UnknownExternal { .. }
        ));
        let receipt = PipelineAgentReceipt {
            turn_id: 1,
            outcome: "completed".into(),
            model: "fixture".into(),
            profile: "fixture".into(),
            session_id: None,
            branch_id: None,
            model_calls: 1,
            input_tokens: Some(2),
            output_tokens: Some(3),
            calls_without_complete_usage: 0,
        };
        store
            .pipeline_settle_agent(&a, &run.id, &agent_attempt.id, " answer ", &receipt)
            .unwrap();
        let done = match store.pipeline_next(&a, &run.id).unwrap() {
            PipelineDirective::Completed { run } => run,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(done.output.as_deref(), Some(" answer "));
    }
    #[test]
    fn unresolved_external_intent_keeps_inspectable_target_even_after_deadline() {
        let (store, a) = setup();
        let p = store.pipeline_define(&a, def()).unwrap();
        let run = store
            .pipeline_admit_manual(
                &a,
                &p.pipeline_id,
                None,
                None,
                "req-target",
                "hello",
                None,
                None,
            )
            .unwrap();
        let approval = match store.pipeline_next(&a, &run.id).unwrap() {
            PipelineDirective::Approval { approval, .. } => approval,
            other => panic!("unexpected {other:?}"),
        };
        store.pipeline_approve(&a, &approval.id, true).unwrap();
        let attempt = match store.pipeline_next(&a, &run.id).unwrap() {
            PipelineDirective::Agent { attempt, .. } => attempt,
            other => panic!("unexpected {other:?}"),
        };
        let (session, branch) = store.create(&a, "agent-target", binding()).unwrap();
        let noted = store
            .pipeline_note_agent_target(&a, &run.id, &attempt.id, &session.id, &branch.id)
            .unwrap();
        assert_eq!(
            noted.receipt.as_ref().unwrap()["session_id"],
            serde_json::Value::String(session.id.clone())
        );
        let c = store.connection().unwrap();
        c.execute(
            "UPDATE pipeline_runs SET deadline_at=0 WHERE id=?1",
            [&run.id],
        )
        .unwrap();
        drop(c);
        match store.pipeline_next(&a, &run.id).unwrap() {
            PipelineDirective::UnknownExternal {
                run: unknown,
                attempt,
            } => {
                assert_eq!(unknown.state, "running_external");
                assert_eq!(
                    attempt.receipt.as_ref().unwrap()["branch_id"],
                    serde_json::Value::String(branch.id)
                );
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn denied_approval_stops_before_external_step() {
        let (store, a) = setup();
        let p = store.pipeline_define(&a, def()).unwrap();
        let run = store
            .pipeline_admit_manual(&a, &p.pipeline_id, None, None, "req3", "hello", None, None)
            .unwrap();
        let approval = match store.pipeline_next(&a, &run.id).unwrap() {
            PipelineDirective::Approval { approval, .. } => approval,
            _ => panic!(),
        };
        let failed = store.pipeline_approve(&a, &approval.id, false).unwrap();
        assert_eq!(failed.state, "failed");
        assert_eq!(store.pipeline_steps(&a, &run.id).unwrap().len(), 3);
    }
}
