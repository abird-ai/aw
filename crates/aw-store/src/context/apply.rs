use super::*;

pub(crate) fn add_dependencies(c: &Connection, node: &str, refs: &[EvidenceRef]) -> Result<()> {
    for s in refs {
        let source_seq: i64 = db(c.query_row(
            "SELECT seq FROM nodes WHERE session_id=?1 AND id=?2",
            params![s.session_id, s.node_id],
            |r| r.get(0),
        ))?;
        let target_seq: i64 =
            db(c.query_row("SELECT seq FROM nodes WHERE id=?1", [node], |r| r.get(0)))?;
        if source_seq >= target_seq {
            return Err(Error::Denied(
                "context evidence must precede its derived node",
            ));
        }
        db(c.execute("INSERT OR IGNORE INTO node_dependencies(node_id,source_session,source_node) VALUES(?1,?2,?3)",params![node,s.session_id,s.node_id]))?;
    }
    dependencies(c, node)?;
    Ok(())
}
pub(crate) fn private_audience(
    c: &Connection,
    artifact: &SummaryArtifact,
    viewers: &BTreeSet<PrincipalId>,
) -> Result<()> {
    let owner_only = if viewers.len() == 1 {
        match viewers.iter().next() {
            Some(viewer) => artifact_owned_by(c, &artifact.id, &artifact.owner, viewer)?,
            None => false,
        }
    } else {
        false
    };
    if artifact.imported_unverified && !owner_only {
        return Err(Error::Denied(
            "imported external summary remains owner-private",
        ));
    }
    Ok(())
}
impl SessionStore {
    /// Reuse is a local transaction with no model call. Source reads, audience
    /// and exact head are rechecked. IDs are independent of generation IDs.
    pub fn context_apply(
        &self,
        actor: &PrincipalId,
        request: ApplyContext,
    ) -> Result<ContextApplied> {
        request_id(&request.request_id)?;
        if !request.confirmed_disclosure {
            return Err(Error::Denied(
                "explicit summary-to-destination disclosure confirmation required",
            ));
        }
        let fingerprint = digest(&request)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let a = check_artifact(&tx, actor, &request.artifact_id, true)?;
        let actor_user = ensure_user_for_principal(&tx, actor)?;
        let prior:Option<(String,String)>=db(tx.query_row("SELECT fingerprint,result_json FROM context_applications WHERE ((actor_user_id IS NULL AND actor=?1) OR actor_user_id=?3) AND request_id=?2 ORDER BY created_at LIMIT 1",params![actor.as_str(),request.request_id,actor_user],|r|Ok((r.get(0)?,r.get(1)?))).optional())?;
        if let Some((fp, body)) = prior {
            if fp != fingerprint {
                return Err(Error::Conflict(
                    "context application ID already belongs to another request",
                ));
            }
            let result: ContextApplied = decode(&body)?;
            authorize_tree(&tx, &result.session.id, actor)?;
            return Ok(result);
        }
        let applications: i64 = db(tx.query_row(
            "SELECT count(*) FROM context_applications WHERE actor=?1 OR actor_user_id=?2",
            params![actor.as_str(), actor_user],
            |r| r.get(0),
        ))?;
        if applications >= 65_536 {
            return Err(Error::Limit(
                "context application retention bound reached; no hidden receipt expiry",
            ));
        }
        let original = target(&tx, actor, &request.target, true)?;
        let anchor = session_in(&tx, &original.session_id)?;
        if !matches!(request.mode, ApplyMode::NextPrompt) {
            let _writer =
                crate::workspace_writer::require_workspace_writer_for_session(&tx, &anchor.id)?;
        }
        let refs = artifact_refs(&tx, &a.id)?;
        super::materialized::authorize_artifact_for_session(&tx, actor, &a.id, &anchor.id)?;
        let mut session = anchor.clone();
        let mut branch = original.clone();
        let viewers = match &request.mode {
            ApplyMode::NewSession { .. } => BTreeSet::from([actor.clone()]),
            _ => audience(&tx, &anchor.id)?,
        };
        private_audience(&tx, &a, &viewers)?;
        disclosure(&tx, &refs, &viewers)?;
        if matches!(a.plan.kind, PlanKind::MaterializedFacts) && request.target != a.plan.target {
            return Err(Error::Conflict(
                "materialized-fact context applies only to the exact target head/revision it was projected for",
            ));
        }
        if matches!(a.plan.kind, PlanKind::Compact { .. })
            && (!matches!(request.mode, ApplyMode::Compact) || request.target != a.plan.target)
        {
            return Err(Error::Conflict(
                "compaction applies only to its exact prepared target",
            ));
        }
        if matches!(request.mode, ApplyMode::Compact)
            && !matches!(a.plan.kind, PlanKind::Compact { .. })
        {
            return Err(Error::Denied(
                "additive import cannot be relabeled as compaction",
            ));
        }
        match &request.mode {
            ApplyMode::Fork { name } => {
                label(name, 80)?;
                if name.starts_with("b_") {
                    return Err(Error::Config("reserved branch name".into()));
                }
                let count: i64 = db(tx.query_row(
                    "SELECT count(*) FROM branches WHERE session_id=?1",
                    [&session.id],
                    |r| r.get(0),
                ))?;
                if count >= 1024 {
                    return Err(Error::Limit("branch bound"));
                }
                let bid = id("b")?;
                db(tx.execute("INSERT INTO branches(id,session_id,name,head,created_at) VALUES(?1,?2,?3,?4,?5)",params![bid,session.id,name,request.target.head,now()]))?;
                branch = branch_in(&tx, &session.id, &bid)?;
            }
            ApplyMode::NewSession { title } => {
                label(title, 160)?;
                let result = crate::resources::create_in_workspace_tx(
                    &tx,
                    actor,
                    title,
                    &anchor.binding,
                    &anchor.workspace_id,
                    None,
                )?;
                session = result.0;
                branch = result.1;
            }
            _ => {}
        }
        let txid = crate::durability::tx_id()?;
        let mut node_id = None;
        let mut pending_id = None;
        let mut reused = false;
        if matches!(request.mode, ApplyMode::NextPrompt) {
            let count:i64=db(tx.query_row("SELECT count(*) FROM context_pending WHERE (actor=?1 OR actor_user_id=?4) AND session_id=?2 AND branch_id=?3 AND state='pending'",params![actor.as_str(),session.id,branch.id,actor_user],|r|r.get(0)))?;
            if count >= 8 {
                return Err(Error::Limit(
                    "at most eight explicit next-prompt attachments",
                ));
            }
            let pid = id("cpending")?;
            db(tx.execute("INSERT INTO context_pending(id,actor,session_id,branch_id,head,revision,artifact_id,state,created_at,actor_user_id) VALUES(?1,?2,?3,?4,?5,?6,?7,'pending',?8,?9)",params![pid,actor.as_str(),session.id,branch.id,branch.head,branch.revision,a.id,now(),actor_user]))?;
            pending_id = Some(pid);
        } else {
            // Reusing the same artifact on this ancestry focuses its existing
            // reference. It never triggers regeneration or duplicate billing.
            if matches!(request.mode, ApplyMode::Append) {
                for n in path(&tx, &session.id, &branch.head)? {
                    if n.kind == "context_import"
                        && n.payload["artifact_id"].as_str() == Some(a.id.as_str())
                    {
                        node_id = Some(n.id);
                        reused = true;
                        break;
                    }
                }
            }
            if !reused {
                let count: i64 = db(tx.query_row(
                    "SELECT count(*) FROM nodes WHERE session_id=?1",
                    [&session.id],
                    |r| r.get(0),
                ))?;
                if count >= MAX_SESSION_NODES as i64 {
                    return Err(Error::Limit("session node bound"));
                }
                let node = id("n")?;
                let compact = matches!(request.mode, ApplyMode::Compact);
                let payload = ContextNode {
                    artifact_id: a.id.clone(),
                    keep_from: if compact {
                        a.plan.keep_from.clone()
                    } else {
                        None
                    },
                };
                insert_node(
                    &tx,
                    &session.id,
                    &node,
                    Some(&branch.head),
                    if compact {
                        "context_compaction"
                    } else {
                        "context_import"
                    },
                    actor,
                    serde_json::to_value(payload).map_err(|_| Error::Storage("bad import"))?,
                )?;
                super::materialized::link_artifact_facts_to_node(&tx, &a.id, &node)?;
                add_dependencies(&tx, &node, &refs)?;
                add_derivations(&tx, &node, actor, &refs, &a.plan.release_versions)?;
                let changed=db(tx.execute("UPDATE branches SET head=?1,revision=revision+1 WHERE id=?2 AND head=?3 AND revision=?4 AND active_run IS NULL",params![node,branch.id,branch.head,branch.revision]))?;
                if changed != 1 {
                    return Err(Error::Conflict("destination changed while attaching"));
                }
                crate::durability::ref_event(
                    &tx,
                    &branch,
                    &node,
                    branch.revision + 1,
                    actor,
                    if compact {
                        "context_compaction"
                    } else {
                        "context_import"
                    },
                    Some(&node),
                    &txid,
                )?;
                node_id = Some(node);
                db(tx.execute(
                    "UPDATE sessions SET updated_at=?1 WHERE id=?2",
                    params![now(), session.id],
                ))?;
                if a.imported_unverified {
                    db(tx.execute(
                        "INSERT OR IGNORE INTO context_private_sessions(session_id) VALUES(?1)",
                        [&session.id],
                    ))?;
                }
            }
        }
        branch = branch_in(&tx, &session.id, &branch.id)?;
        // Validate projected byte/order limits before committing a broken view.
        if pending_id.is_none() {
            Conversation::from_history(
                session.binding.system_prompt.clone(),
                &projected_history(&tx, &session, &branch)?,
            )?;
        }
        let pending = pending_id.is_some();
        let result = ContextApplied {
            session: session_in(&tx, &session.id)?,
            branch,
            node_id,
            pending_id,
            reused,
        };
        db(tx.execute("INSERT INTO context_applications(actor,request_id,fingerprint,artifact_id,result_json,created_at,actor_user_id) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![actor.as_str(),request.request_id,fingerprint,a.id,json(&result)?,now(),actor_user]))?;
        audit(
            &tx,
            &session.id,
            actor,
            "context.applied",
            serde_json::json!({"artifact_id":a.id,"result":result,"request_id":request.request_id}),
        )?;
        crate::durability::commit_event(
            &tx,
            &txid,
            "context.applied",
            "context_artifact",
            &a.id,
            Some(actor),
            serde_json::json!({"session_id":session.id,"branch_id":result.branch.id,"artifact_id":a.id,"request_id":request.request_id,"reused":reused,"pending":pending}),
        )?;
        db(tx.commit())?;
        Ok(result)
    }
    /// Read an earlier exact apply receipt without rebasing or replaying it.
    pub fn context_application(
        &self,
        actor: &PrincipalId,
        request: &str,
    ) -> Result<ContextApplied> {
        request_id(request)?;
        let c = self.connection()?;
        let actor_user = user_for_principal(&c, actor)?;
        let row:Option<(String,String)>=db(c.query_row("SELECT artifact_id,result_json FROM context_applications WHERE ((actor_user_id IS NULL AND actor=?1) OR (?3 IS NOT NULL AND actor_user_id=?3)) AND request_id=?2 ORDER BY created_at LIMIT 1",params![actor.as_str(),request,actor_user],|r|Ok((r.get(0)?,r.get(1)?))).optional())?;
        let (artifact, body) = row.ok_or(Error::Denied("context application unavailable"))?;
        check_artifact(&c, actor, &artifact, true)?;
        let result: ContextApplied = decode(&body)?;
        authorize_tree(&c, &result.session.id, actor)?;
        Ok(result)
    }
    pub fn context_pending(
        &self,
        actor: &PrincipalId,
        session: &str,
        branch: &str,
    ) -> Result<Vec<(String, String)>> {
        let c = self.connection()?;
        authorize(&c, session, actor, true)?;
        let b = branch_in(&c, session, branch)?;
        let rows = pending_rows(&c, actor, session, &b.id)?;
        let mut out = Vec::new();
        for (id, artifact, _, _) in rows {
            check_artifact(&c, actor, &artifact, true)?;
            out.push((id, artifact));
        }
        Ok(out)
    }
    pub fn context_discard_pending(&self, actor: &PrincipalId, id: &str) -> Result<()> {
        let c = self.connection()?;
        let actor_user = user_for_principal(&c, actor)?;
        db(c.execute("UPDATE context_pending SET state='discarded' WHERE id=?1 AND ((actor_user_id IS NULL AND actor=?2) OR (?3 IS NOT NULL AND actor_user_id=?3)) AND state='pending'",params![id,actor.as_str(),actor_user]))?;
        Ok(())
    }
}
fn pending_rows(
    c: &Connection,
    actor: &PrincipalId,
    session: &str,
    branch: &str,
) -> Result<Vec<(String, String, String, u64)>> {
    let actor_user = user_for_principal(c, actor)?;
    let mut q=db(c.prepare("SELECT id,artifact_id,head,revision FROM context_pending WHERE ((actor_user_id IS NULL AND actor=?1) OR (?4 IS NOT NULL AND actor_user_id=?4)) AND session_id=?2 AND branch_id=?3 AND state='pending' ORDER BY created_at,id LIMIT 9"))?;
    let out = db(db(
        q.query_map(params![actor.as_str(), session, branch, actor_user], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        }),
    )?
    .collect::<rusqlite::Result<Vec<_>>>())?;
    if out.len() > 8 {
        return Err(Error::Limit("pending context bound exceeded"));
    }
    Ok(out)
}
pub(crate) fn pending_context(
    c: &Connection,
    actor: &PrincipalId,
    session: &str,
    b: &BranchInfo,
) -> Result<Vec<ContextBlock>> {
    let mut out = Vec::new();
    for (id, artifact, head, revision) in pending_rows(c, actor, session, &b.id)? {
        if b.head != head || b.revision != revision {
            return Err(Error::Conflict(
                "next-prompt attachment target moved; explicitly discard or reattach",
            ));
        }
        let a = check_artifact(c, actor, &artifact, true)?;
        super::materialized::authorize_artifact_for_session(c, actor, &artifact, session)?;
        let viewers = audience(c, session)?;
        private_audience(c, &a, &viewers)?;
        disclosure(c, &artifact_refs(c, &artifact)?, &viewers)?;
        out.push(ContextBlock {
            before_turn: 0,
            node_id: id,
            artifact_id: artifact,
            kind: ContextBlockKind::Import,
            text: a.text,
            sources: a.plan.dependencies,
        });
    }
    Ok(out)
}
pub(crate) fn consume_pending(
    c: &Connection,
    actor: &PrincipalId,
    session: &str,
    b: &BranchInfo,
    prompt: &str,
) -> Result<()> {
    for block in pending_context(c, actor, session, b)? {
        let refs = artifact_refs(c, &block.artifact_id)?;
        let a = artifact_in(c, &block.artifact_id)?;
        super::materialized::link_artifact_facts_to_node(c, &block.artifact_id, prompt)?;
        add_dependencies(c, prompt, &refs)?;
        add_derivations(c, prompt, actor, &refs, &a.plan.release_versions)?;
        if a.imported_unverified {
            db(c.execute(
                "INSERT OR IGNORE INTO context_private_sessions(session_id) VALUES(?1)",
                [session],
            ))?;
        }
        let actor_user = user_for_principal(c, actor)?;
        db(c.execute("UPDATE context_pending SET state='consumed' WHERE id=?1 AND ((actor_user_id IS NULL AND actor=?2) OR (?3 IS NOT NULL AND actor_user_id=?3)) AND state='pending'",params![block.node_id,actor.as_str(),actor_user]))?;
        db(c.execute("INSERT OR IGNORE INTO context_prompt_attachments(prompt_node,artifact_id) VALUES(?1,?2)",params![prompt,block.artifact_id]))?;
        audit(
            c,
            session,
            actor,
            "context.prompt_attachment",
            serde_json::json!({"prompt_node":prompt,"artifact_id":block.artifact_id,"attachment_id":block.node_id}),
        )?;
    }
    Ok(())
}

fn add_derivations(
    c: &Connection,
    node: &str,
    actor: &PrincipalId,
    refs: &[EvidenceRef],
    releases: &BTreeMap<String, String>,
) -> Result<()> {
    for source in refs {
        if let Some(grant) = releases.get(&source.session_id) {
            let actor_user = ensure_user_for_principal(c, actor)?;
            db(c.execute("INSERT OR IGNORE INTO context_derivations(node_id,source_session,requester,grant_id,requester_user_id) VALUES(?1,?2,?3,?4,?5)",params![node,source.session_id,actor.as_str(),grant,actor_user]))?;
        }
        db(c.execute("INSERT OR IGNORE INTO context_derivations(node_id,source_session,requester,grant_id,requester_user_id) SELECT ?1,source_session,requester,grant_id,requester_user_id FROM context_derivations WHERE node_id=?2",params![node,source.node_id]))?;
    }
    super::check_derivations(c, node)
}
