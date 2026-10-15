use super::*;
use aw_agent_core::{
    InvocationContext, InvocationIdentity, InvocationPurpose, InvocationScope, JournalFuture,
    SummaryInput, SummaryJournal, SummaryResult,
};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct ContextJournal {
    store: SessionStore,
    actor: PrincipalId,
    plan_id: String,
    request_id: String,
    active: Arc<Mutex<Option<String>>>,
}
fn state(result: &SummaryResult) -> &'static str {
    match result.outcome {
        Outcome::Completed => "ready",
        Outcome::Cancelled => "cancelled",
        Outcome::Incomplete => "incomplete",
        Outcome::Failed => "failed",
    }
}
pub(crate) fn text_hash(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_artifact(
    c: &Connection,
    plan: &ContextPlan,
    actor: &PrincipalId,
    text: &str,
    method: &str,
    identity: Option<InvocationIdentity>,
    meta: Option<aw_agent_core::CompletionMeta>,
    operation: Option<&str>,
    from: Option<String>,
) -> Result<SummaryArtifact> {
    if text.trim().is_empty() || text.len() > plan.max_output_bytes {
        return Err(Error::Limit("summary text limit"));
    }
    let actor_user = ensure_user_for_principal(c, actor)?;
    let count: i64 = db(c.query_row(
        "SELECT count(*) FROM context_artifacts WHERE owner=?1 OR owner_user_id=?2",
        params![actor.as_str(), actor_user],
        |r| r.get(0),
    ))?;
    if count >= MAX_PLANS {
        return Err(Error::Limit("summary artifact retention limit"));
    }
    let a = SummaryArtifact {
        version: 1,
        id: id("ca")?,
        owner: actor.clone(),
        method: method.into(),
        text: text.into(),
        text_sha256: text_hash(text),
        plan: plan.clone(),
        generated_from: from,
        identity,
        meta,
        imported_unverified: false,
        created_at: now(),
    };
    db(c.execute("INSERT INTO context_artifacts(id,owner,plan_id,operation_id,body,created_at,owner_user_id) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![a.id,actor.as_str(),plan.id,operation,json(&a)?,now(),actor_user]))?;
    for s in &plan.dependencies {
        db(c.execute("INSERT INTO context_artifact_sources(artifact_id,session_id,node_id,digest) VALUES(?1,?2,?3,?4)",params![a.id,s.session_id,s.node_id,s.sha256]))?;
    }
    Ok(a)
}
fn generation_fingerprint(input: &SummaryInput, identity: &InvocationIdentity) -> Result<String> {
    let mut identity = identity.clone();
    identity.grant_revision = 0;
    digest(&(input, identity))
}
impl SessionStore {
    /// An exact retried generation is lookup-only even after restart. Registry
    /// refresh versions are excluded from identity matching, never from grants.
    pub fn context_generation_status(
        &self,
        actor: &PrincipalId,
        plan_id: &str,
        request: &str,
        identity: &InvocationIdentity,
    ) -> Result<Option<SummaryOperation>> {
        request_id(request)?;
        let c = self.connection()?;
        if !same_user(&c, &identity.actor, actor)? {
            return Err(Error::Denied("summary actor mismatch"));
        }
        let plan = plan_in(&c, actor, plan_id)?;
        source_use(&c, actor, &plan.dependencies, Some(&identity.account_id))?;
        let Some((old, operation)) = operation_for_request(&c, actor, request)? else {
            return Ok(None);
        };
        if old != generation_fingerprint(&plan.input()?, identity)? {
            return Err(Error::Conflict(
                "summary request ID belongs to different evidence/account/model",
            ));
        }
        Ok(Some(operation))
    }
    pub fn summary_journal(
        &self,
        actor: &PrincipalId,
        plan_id: &str,
        request: &str,
    ) -> Result<ContextJournal> {
        request_id(request)?;
        self.context_plan(actor, plan_id)?;
        Ok(ContextJournal {
            store: self.clone(),
            actor: actor.clone(),
            plan_id: plan_id.into(),
            request_id: request.into(),
            active: Arc::new(Mutex::new(None)),
        })
    }
    /// Human/manual generation is explicitly attributed and idempotent; it does
    /// not fabricate model usage. Editing produces another immutable artifact.
    pub fn context_manual(
        &self,
        actor: &PrincipalId,
        plan_id: &str,
        request: &str,
        text: &str,
        from: Option<String>,
    ) -> Result<SummaryArtifact> {
        request_id(request)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let p = plan_in(&tx, actor, plan_id)?;
        if let Some(ref source) = from {
            let old = check_artifact(&tx, actor, source, true)?;
            if old.plan.id != p.id {
                return Err(Error::Denied(
                    "edited summary must keep its original source plan",
                ));
            }
        }
        let fingerprint = digest(&("human", plan_id, text, &from))?;
        if let Some((f, op)) = operation_for_request(&tx, actor, request)? {
            if f != fingerprint {
                return Err(Error::Conflict(
                    "summary request ID already used for another input",
                ));
            }
            return check_artifact(
                &tx,
                actor,
                &op.artifact_id
                    .ok_or(Error::Conflict("prior summary operation is not ready"))?,
                true,
            );
        }
        let actor_user = ensure_user_for_principal(&tx, actor)?;
        let count: i64 = db(tx.query_row(
            "SELECT count(*) FROM context_operations WHERE actor=?1 OR actor_user_id=?2",
            params![actor.as_str(), actor_user],
            |r| r.get(0),
        ))?;
        if count >= MAX_PLANS {
            return Err(Error::Limit("summary operation retention bound reached"));
        }
        let operation = id("cs")?;
        db(tx.execute("INSERT INTO context_operations(id,actor,request_id,plan_id,fingerprint,turn_id,state,created_at,actor_user_id) VALUES(?1,?2,?3,?4,?5,0,'started',?6,?7)",params![operation,actor.as_str(),request,plan_id,fingerprint,now(),actor_user]))?;
        let a = insert_artifact(
            &tx,
            &p,
            actor,
            text,
            if from.is_some() {
                "human_edited"
            } else {
                "human"
            },
            None,
            None,
            Some(&operation),
            from,
        )?;
        db(tx.execute(
            "UPDATE context_operations SET state='ready',artifact_id=?1,settled_at=?2 WHERE id=?3",
            params![a.id, now(), operation],
        ))?;
        audit(
            &tx,
            &p.target.session_id,
            actor,
            "context.manual",
            serde_json::json!({"operation":operation,"artifact":a.id,"plan":p.id}),
        )?;
        let txid = crate::durability::tx_id()?;
        crate::durability::commit_event(
            &tx,
            &txid,
            "context.artifact_created",
            "context_artifact",
            &a.id,
            Some(actor),
            serde_json::json!({"session_id":p.target.session_id,"plan_id":p.id,"operation_id":operation,"method":a.method}),
        )?;
        db(tx.commit())?;
        Ok(a)
    }
    pub fn context_operation(
        &self,
        actor: &PrincipalId,
        id_or_request: &str,
    ) -> Result<SummaryOperation> {
        let c = self.connection()?;
        let actor_user = user_for_principal(&c, actor)?;
        let id:Option<String>=db(c.query_row("SELECT id FROM context_operations WHERE ((actor_user_id IS NULL AND actor=?1) OR (?3 IS NOT NULL AND actor_user_id=?3)) AND (id=?2 OR request_id=?2) ORDER BY id=?2 DESC LIMIT 1",params![actor.as_str(),id_or_request,actor_user],|r|r.get(0)).optional())?;
        let op = operation_in(
            &c,
            &id.ok_or(Error::Denied("summary operation unavailable"))?,
        )?;
        plan_in(&c, actor, &op.plan_id)?;
        Ok(op)
    }
}
fn operation_for_request(
    c: &Connection,
    actor: &PrincipalId,
    request: &str,
) -> Result<Option<(String, SummaryOperation)>> {
    let actor_user = user_for_principal(c, actor)?;
    let row: Option<(String, String)> = db(c
        .query_row(
            "SELECT fingerprint,id FROM context_operations WHERE ((actor_user_id IS NULL AND actor=?1) OR (?3 IS NOT NULL AND actor_user_id=?3)) AND request_id=?2 ORDER BY created_at LIMIT 1",
            params![actor.as_str(), request, actor_user],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional())?;
    row.map(|(f, id)| Ok((f, operation_in(c, &id)?)))
        .transpose()
}
pub(crate) fn operation_in(c: &Connection, id: &str) -> Result<SummaryOperation> {
    let row=db(c.query_row("SELECT id,request_id,plan_id,state,artifact_id,identity_json,metadata_json,error FROM context_operations WHERE id=?1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,Option<String>>(5)?,r.get::<_,Option<String>>(6)?,r.get::<_,Option<String>>(7)?))))?;
    Ok(SummaryOperation {
        id: row.0,
        request_id: row.1,
        plan_id: row.2,
        state: row.3,
        artifact_id: row.4,
        identity: row.5.as_deref().map(decode).transpose()?,
        meta: row.6.as_deref().map(decode).transpose()?,
        error: row.7,
    })
}
impl ContextJournal {
    fn authorized(
        &self,
        input: &SummaryInput,
        identity: &InvocationIdentity,
    ) -> Result<ContextPlan> {
        let c = self.store.connection()?;
        if !same_user(&c, &identity.actor, &self.actor)? || input.plan_id != self.plan_id {
            return Err(Error::Denied("summary caller/plan mismatch"));
        }
        let p = plan_in(&c, &self.actor, &self.plan_id)?;
        source_use(&c, &self.actor, &p.dependencies, Some(&identity.account_id))?;
        ensure_reviewed_sources(&c, &p.dependencies)?;
        if json(&p.input()?)? != json(input)? {
            return Err(Error::Denied(
                "exact authorized evidence cannot be replaced by the host request",
            ));
        }
        Ok(p)
    }
    fn begin_sync(
        &self,
        input: &SummaryInput,
        identity: &InvocationIdentity,
    ) -> Result<InvocationContext> {
        self.authorized(input, identity)?;
        let mut active = self
            .active
            .lock()
            .map_err(|_| Error::Storage("summary operation lock poisoned"))?;
        if active.is_some() {
            return Err(Error::Conflict("summary invocation already admitted"));
        }
        let process = self.store.process_identity()?;
        let mut c = self.store.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let p = plan_in(&tx, &self.actor, &self.plan_id)?;
        source_use(
            &tx,
            &self.actor,
            &p.dependencies,
            Some(&identity.account_id),
        )?;
        ensure_reviewed_sources(&tx, &p.dependencies)?;
        if operation_for_request(&tx, &self.actor, &self.request_id)?.is_some() {
            return Err(Error::Conflict(
                "summary request already recorded; query status, never automatically regenerate",
            ));
        }
        let actor_user = ensure_user_for_principal(&tx, &self.actor)?;
        let count: i64 = db(tx.query_row(
            "SELECT count(*) FROM context_operations WHERE actor=?1 OR actor_user_id=?2",
            params![self.actor.as_str(), actor_user],
            |r| r.get(0),
        ))?;
        if count >= MAX_PLANS {
            return Err(Error::Limit("summary operation retention bound reached"));
        }
        let s = session_in(&tx, &p.target.session_id)?;
        if s.next_turn > (1u64 << 42) - 1 {
            return Err(Error::Limit("session invocation identifiers exhausted"));
        }
        let imported: bool = db(tx.query_row(
            "SELECT import_review_needed<>0 FROM sessions WHERE id=?1",
            [&s.id],
            |r| r.get(0),
        ))?;
        if imported {
            return Err(Error::Denied(
                "review imported target before a paid summary",
            ));
        }
        let operation = id("cs")?;
        let fingerprint = generation_fingerprint(input, identity)?;
        db(tx.execute("INSERT INTO context_operations(id,actor,request_id,plan_id,fingerprint,turn_id,identity_json,state,created_at,actor_user_id,owner_process_id,owner_store_generation) VALUES(?1,?2,?3,?4,?5,?6,?7,'started',?8,?9,?10,?11)",params![operation,self.actor.as_str(),self.request_id,p.id,fingerprint,s.next_turn,json(identity)?,now(),actor_user,process.as_ref().map(|p|p.process_id.as_str()),process.as_ref().map(|p|p.store_generation)]))?;
        // Summary attempts share session-global ordinal allocation, but do not
        // move the branch head or hold its writer lock during inference.
        db(tx.execute(
            "UPDATE sessions SET next_turn=next_turn+1 WHERE id=?1",
            [&s.id],
        ))?;
        db(tx.commit())?;
        *active = Some(operation.clone());
        Ok(InvocationContext {
            purpose: InvocationPurpose::Summary {
                operation_id: operation,
                input_sha256: input.digest()?,
                sources: p.dependencies,
            },
            scope: Some(InvocationScope {
                session_id: s.id,
                branch_id: p.target.branch_id,
            }),
            turn_id: s.next_turn,
            ordinal: 1,
            identity: identity.clone(),
        })
    }
    fn finish_sync(&self, result: &SummaryResult) -> Result<()> {
        let operation = self
            .active
            .lock()
            .map_err(|_| Error::Storage("summary lock poisoned"))?
            .clone()
            .ok_or(Error::Storage("summary start receipt missing"))?;
        let mut c = self.store.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let prior = operation_in(&tx, &operation)?;
        if prior.state != "started" {
            return Err(Error::Conflict("summary already settled"));
        }
        let mut outcome = state(result);
        let mut artifact = None;
        let mut error = result.error.clone();
        if result.outcome == Outcome::Completed {
            match plan_in(&tx, &self.actor, &self.plan_id) {
                Ok(p) => {
                    let a = insert_artifact(
                        &tx,
                        &p,
                        &self.actor,
                        &result.text,
                        "model",
                        prior.identity,
                        result.meta.clone(),
                        Some(&operation),
                        None,
                    )?;
                    artifact = Some(a.id);
                }
                Err(_) => {
                    outcome = "failed";
                    error = Some("source/destination access changed; output withheld".into());
                }
            }
        }
        db(tx.execute("UPDATE context_operations SET state=?1,metadata_json=?2,artifact_id=?3,error=?4,settled_at=?5 WHERE id=?6 AND state='started'",params![outcome,result.meta.as_ref().map(json).transpose()?,artifact,error,now(),operation]))?;
        let txid = crate::durability::tx_id()?;
        crate::durability::commit_event(
            &tx,
            &txid,
            "context.generation_settled",
            "context_operation",
            &operation,
            Some(&self.actor),
            serde_json::json!({"session_id":persistence_session(&tx,&self.plan_id)?,"state":outcome,"artifact_id":artifact}),
        )?;
        db(tx.commit())?;
        if result.outcome == Outcome::Completed && outcome != "ready" {
            return Err(Error::Denied(
                "summary output withheld after current-access check",
            ));
        }
        Ok(())
    }
}
fn persistence_session(c: &Connection, plan_id: &str) -> Result<String> {
    db(c.query_row(
        "SELECT target_session FROM context_plans WHERE id=?1",
        [plan_id],
        |r| r.get(0),
    ))
}
impl SummaryJournal for ContextJournal {
    fn authorize<'a>(
        &'a self,
        input: &'a SummaryInput,
        identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, ()> {
        let this = self.clone();
        let input = input.clone();
        let identity = identity.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || this.authorized(&input, &identity).map(|_| ()))
                .await
                .map_err(|_| Error::Storage("summary authorization worker failed"))?
        })
    }
    fn begin<'a>(
        &'a self,
        input: &'a SummaryInput,
        identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, InvocationContext> {
        let this = self.clone();
        let input = input.clone();
        let identity = identity.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || this.begin_sync(&input, &identity))
                .await
                .map_err(|_| Error::Storage("summary begin worker failed"))?
        })
    }
    fn finish<'a>(&'a self, result: &'a SummaryResult) -> JournalFuture<'a, ()> {
        let this = self.clone();
        let result = result.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || this.finish_sync(&result))
                .await
                .map_err(|_| Error::Storage("summary settlement worker failed"))?
        })
    }
}
