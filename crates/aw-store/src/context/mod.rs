//! Authorized source selection, immutable artifacts and additive/compacted
//! projection. There is no model call in preparation, inspection or application.
pub(crate) mod apply;
pub(crate) mod materialized;
pub(crate) mod operations;
mod types;
use super::*;
use aw_agent_core::{ContextBlock, ContextBlockKind, EvidenceRef};
pub use materialized::{MaterializedContextProjection, MaterializedContextRequest};
pub use operations::ContextJournal;
use std::collections::{BTreeMap, BTreeSet};
pub use types::*;

pub const MAX_SOURCES: usize = 16;
pub const MAX_DEPENDENCIES: usize = 512;
pub const MAX_PLANS: i64 = 4096;
pub(crate) fn digest(value: &impl serde::Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(json(value)?.as_bytes())))
}
pub(crate) fn request_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(Error::Config(
            "request ID must be 1..128 portable characters".into(),
        ));
    }
    Ok(())
}
pub(crate) fn authority(c: &Connection) -> Result<String> {
    db(c.query_row(
        "SELECT value FROM context_meta WHERE key='authority'",
        [],
        |r| r.get(0),
    ))
}
pub(crate) fn dependencies(c: &Connection, node: &str) -> Result<Vec<(String, String)>> {
    let mut q=db(c.prepare("SELECT source_session,source_node FROM node_dependencies WHERE node_id=?1 ORDER BY source_session,source_node LIMIT 513"))?;
    let rows = db(q.query_map([node], |r| Ok((r.get(0)?, r.get(1)?))))?;
    let out = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if out.len() > MAX_DEPENDENCIES {
        return Err(Error::Limit("derived context dependency bound exceeded"));
    }
    Ok(out)
}
/// Conservative transitive dependency gate. Descendant answers retain these
/// constraints even when a checkpoint hides the imported block from projection.
pub(crate) fn authorize_node(
    c: &Connection,
    session: &str,
    node: &str,
    actor: &PrincipalId,
) -> Result<()> {
    authorize(c, session, actor, false)?;
    let exists: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM nodes WHERE session_id=?1 AND id=?2)",
        params![session, node],
        |r| r.get(0),
    ))?;
    if !exists {
        return Err(Error::Denied("source unavailable or not permitted"));
    }
    for (s, n) in dependencies(c, node)? {
        authorize(c, &s, actor, false)?;
        let exists: bool = db(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE session_id=?1 AND id=?2)",
            params![s, n],
            |r| r.get(0),
        ))?;
        if !exists {
            return Err(Error::Denied("derived context source unavailable"));
        }
    }
    check_derivations(c, node)?;
    let private: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM context_private_sessions WHERE session_id=?1)",
        [session],
        |r| r.get(0),
    ))?;
    if private && !principal_is_session_owner(c, session, actor)? {
        return Err(Error::Denied(
            "externally imported context is owner-private",
        ));
    }
    Ok(())
}
pub(crate) fn authorize_tree(c: &Connection, session: &str, actor: &PrincipalId) -> Result<()> {
    authorize(c, session, actor, false)?;
    let mut q=db(c.prepare("SELECT id FROM nodes WHERE session_id=?1 AND id IN (SELECT node_id FROM node_dependencies) LIMIT 10001"))?;
    let ids = db(db(q.query_map([session], |r| r.get::<_, String>(0)))?
        .collect::<rusqlite::Result<Vec<_>>>())?;
    if ids.len() > MAX_SESSION_NODES {
        return Err(Error::Limit("derived tree exceeds view bound"));
    }
    for n in ids {
        authorize_node(c, session, &n, actor)?;
    }
    let s = session_in(c, session)?;
    authorize_node(c, session, &s.root_node, actor)
}
pub(crate) fn audience(c: &Connection, session: &str) -> Result<BTreeSet<PrincipalId>> {
    let s = session_in(c, session)?;
    let owner = match session_owner_user(c, session) {
        Ok(user) => stable_user_principal(&user)?,
        Err(_) => s.owner,
    };
    let mut out = BTreeSet::from([owner]);
    let mut q = db(c.prepare(
        "SELECT principal,principal_user_id FROM members WHERE session_id=?1 LIMIT 1025",
    ))?;
    for row in db(q.query_map([session], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    }))? {
        let (principal, user) = db(row)?;
        out.insert(match user {
            Some(user) => stable_user_principal(&user)?,
            None => PrincipalId::new(principal)?,
        });
    }
    if out.len() > 1025 {
        return Err(Error::Limit("audience too large"));
    }
    Ok(out)
}
pub(crate) fn check_sources(
    c: &Connection,
    actor: &PrincipalId,
    refs: &[EvidenceRef],
) -> Result<()> {
    let auth = authority(c)?;
    for s in refs {
        if s.authority != auth {
            return Err(Error::Denied(
                "cross-authority summary use is not supported",
            ));
        }
        authorize_node(c, &s.session_id, &s.node_id, actor)?;
        if digest(&node_in(c, &s.session_id, &s.node_id)?)? != s.sha256 {
            return Err(Error::Storage("immutable evidence digest mismatch"));
        }
    }
    Ok(())
}
pub(crate) fn disclosure(
    c: &Connection,
    refs: &[EvidenceRef],
    viewers: &BTreeSet<PrincipalId>,
) -> Result<()> {
    for viewer in viewers {
        check_sources(c, viewer, refs)?;
    }
    Ok(())
}
pub(crate) fn target(
    c: &Connection,
    actor: &PrincipalId,
    t: &TargetHead,
    require_unchanged: bool,
) -> Result<BranchInfo> {
    authorize(c, &t.session_id, actor, true)?;
    authorize_node(c, &t.session_id, &t.head, actor)?;
    let b = branch_in(c, &t.session_id, &t.branch_id)?;
    if require_unchanged && (b.head != t.head || b.revision != t.revision || b.active_run.is_some())
    {
        return Err(Error::Conflict(
            "target moved or busy; candidate retained; choose a target explicitly",
        ));
    }
    Ok(b)
}
pub(crate) fn path(c: &Connection, session: &str, head: &str) -> Result<Vec<Node>> {
    let mut out = Vec::new();
    let mut id = Some(head.to_owned());
    let mut bytes = 0usize;
    while let Some(n) = id {
        if out.len() >= MAX_SESSION_NODES {
            return Err(Error::Limit("ancestry too deep"));
        }
        let node = node_in(c, session, &n)?;
        bytes = bytes.saturating_add(json(&node.payload)?.len());
        if bytes > MAX_EXPORT_BYTES {
            return Err(Error::Limit(
                "source path too large; select a narrower scope",
            ));
        }
        id = node.parent_id.clone();
        out.push(node);
    }
    out.reverse();
    Ok(out)
}
pub(crate) fn artifact_in(c: &Connection, id: &str) -> Result<SummaryArtifact> {
    let body: Option<String> = db(c
        .query_row(
            "SELECT body FROM context_artifacts WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional())?;
    decode(&body.ok_or(Error::Denied("summary unavailable or not permitted"))?)
}
pub(crate) fn artifact_refs(c: &Connection, id: &str) -> Result<Vec<EvidenceRef>> {
    let a = authority(c)?;
    let mut q=db(c.prepare("SELECT session_id,node_id,digest FROM context_artifact_sources WHERE artifact_id=?1 ORDER BY session_id,node_id LIMIT 513"))?;
    let values = db(db(q.query_map([id], |r| {
        Ok(EvidenceRef {
            authority: a.clone(),
            session_id: r.get(0)?,
            node_id: r.get(1)?,
            sha256: r.get(2)?,
        })
    }))?
    .collect::<rusqlite::Result<Vec<_>>>())?;
    if values.len() > MAX_DEPENDENCIES {
        return Err(Error::Limit("artifact dependency limit"));
    }
    Ok(values)
}
pub(crate) fn artifact_owner_principal(
    c: &Connection,
    artifact_id: &str,
    historical_owner: &PrincipalId,
) -> Result<PrincipalId> {
    let owner_user: Option<String> = db(c
        .query_row(
            "SELECT owner_user_id FROM context_artifacts WHERE id=?1",
            [artifact_id],
            |r| r.get(0),
        )
        .optional())?
    .flatten();
    owner_user
        .as_deref()
        .map(stable_user_principal)
        .transpose()
        .map(|stable| stable.unwrap_or_else(|| historical_owner.clone()))
}

pub(crate) fn artifact_owned_by(
    c: &Connection,
    artifact_id: &str,
    historical_owner: &PrincipalId,
    actor: &PrincipalId,
) -> Result<bool> {
    let owner = artifact_owner_principal(c, artifact_id, historical_owner)?;
    same_user(c, &owner, actor)
}

pub(crate) fn check_artifact(
    c: &Connection,
    actor: &PrincipalId,
    id: &str,
    require_owner: bool,
) -> Result<SummaryArtifact> {
    let a = artifact_in(c, id)?;
    if require_owner && !artifact_owned_by(c, id, &a.owner, actor)? {
        return Err(Error::Denied(
            "summary candidate is private to its requester",
        ));
    }
    let refs = artifact_refs(c, id)?;
    check_sources(c, actor, &refs)?;
    let owner = artifact_owner_principal(c, id, &a.owner)?;
    source_use(
        c,
        &owner,
        &refs,
        a.identity.as_ref().map(|i| i.account_id.as_str()),
    )?;
    if !a.imported_unverified {
        check_release_versions(c, &owner, &a.plan.release_versions)?;
    }
    if operations::text_hash(&a.text) != a.text_sha256 {
        return Err(Error::Storage("summary content digest mismatch"));
    }
    Ok(a)
}
/// Return semantic nodes in active order. Compaction retains a suffix of this
/// sequence, not a lossy guess at raw ancestry or an isolated replacement string.
pub(crate) fn projected(c: &Connection, session: &str, head: &str) -> Result<Vec<Node>> {
    let mut out = Vec::<Node>::new();
    for n in path(c, session, head)? {
        match n.kind.as_str() {
            "turn" | "context_import" => out.push(n),
            "checkpoint" => {
                out.clear();
                out.push(n);
            }
            "context_compaction" => {
                let payload: ContextNode = serde_json::from_value(n.payload.clone())
                    .map_err(|_| Error::Storage("bad compaction record"))?;
                let suffix = if let Some(keep) = &payload.keep_from {
                    let i = out
                        .iter()
                        .position(|x| &x.id == keep)
                        .ok_or(Error::Storage("compaction kept boundary absent"))?;
                    out.split_off(i)
                } else {
                    Vec::new()
                };
                out.clear();
                out.push(n);
                out.extend(suffix);
            }
            "root" | "prompt" => {}
            _ => return Err(Error::Storage("unknown context node kind")),
        }
    }
    Ok(out)
}
pub(crate) fn block(c: &Connection, n: &Node, before_turn: usize) -> Result<ContextBlock> {
    let data: ContextNode = serde_json::from_value(n.payload.clone())
        .map_err(|_| Error::Storage("bad context node"))?;
    let artifact = artifact_in(c, &data.artifact_id)?;
    Ok(ContextBlock {
        before_turn,
        node_id: n.id.clone(),
        artifact_id: artifact.id,
        kind: if n.kind == "context_compaction" {
            ContextBlockKind::Compaction
        } else {
            ContextBlockKind::Import
        },
        text: artifact.text,
        sources: artifact.plan.dependencies,
    })
}
pub(crate) fn projected_history(
    c: &Connection,
    s: &SessionInfo,
    b: &BranchInfo,
) -> Result<HistoryState> {
    let mut turns = Vec::new();
    let mut summary = None;
    let mut context_blocks = Vec::new();
    for n in projected(c, &s.id, &b.head)? {
        match n.kind.as_str() {
            "turn" => turns.push(
                serde_json::from_value(n.payload).map_err(|_| Error::Storage("invalid turn"))?,
            ),
            "checkpoint" => {
                summary = Some(
                    n.payload["summary"]
                        .as_str()
                        .ok_or(Error::Storage("invalid checkpoint"))?
                        .to_owned(),
                )
            }
            _ => context_blocks.push(block(c, &n, turns.len())?),
        }
    }
    let imported: i64 = db(c.query_row(
        "SELECT import_review_needed FROM sessions WHERE id=?1",
        [&s.id],
        |r| r.get(0),
    ))?;
    let physical_root = key(&s.binding)?;
    let active: i64 =
        if crate::subagents::model_only_child_can_ignore_parent_hazard(c, &s.id, &physical_root)? {
            0
        } else {
            db(c.query_row(
                "SELECT count(*) FROM hazards WHERE root_key=?1 AND state<>'settled'",
                [&physical_root],
                |r| r.get(0),
            ))?
        };
    Ok(HistoryState {
        version: 2,
        turns,
        context_blocks,
        next_turn_id: s.next_turn,
        summary,
        compacted_through: 0,
        summary_provenance_sha256: None,
        needs_reconciliation: active > 0 || imported != 0,
    })
}
pub(crate) fn evidence(
    c: &Connection,
    session: &SessionInfo,
    n: &Node,
) -> Result<Option<Evidence>> {
    let content = match n.kind.as_str() {
        "root" | "prompt" => return Ok(None),
        "turn" => {
            let t: aw_agent_core::Turn = serde_json::from_value(n.payload.clone())
                .map_err(|_| Error::Storage("invalid source turn"))?;
            serde_json::json!({"prompt":t.prompt,"answer":t.answer,"outcome":t.outcome,"error":t.error,
                "tool_records":t.tool_records,"model":t.model,"imported_unverified":session.imported,
                "one_prompt_context":prompt_evidence(c,n.parent_id.as_deref().unwrap_or(""))?})
        }
        "checkpoint" => {
            serde_json::json!({"human_authored_unverified_checkpoint":n.payload["summary"]})
        }
        "context_import" | "context_compaction" => {
            let p: ContextNode = serde_json::from_value(n.payload.clone())
                .map_err(|_| Error::Storage("invalid source import"))?;
            let a = artifact_in(c, &p.artifact_id)?;
            serde_json::json!({"derived_summary":a.text,"artifact_id":a.id,"method":a.method,
                "inherited_sources":a.plan.dependencies,"imported_unverified":a.imported_unverified})
        }
        _ => return Err(Error::Storage("unknown source node")),
    };
    Ok(Some(Evidence {
        source: EvidenceRef {
            authority: authority(c)?,
            session_id: session.id.clone(),
            node_id: n.id.clone(),
            sha256: digest(n)?,
        },
        kind: n.kind.clone(),
        actor: n.actor.clone(),
        execution_context: session
            .binding
            .working_directory
            .to_string_lossy()
            .into_owned(),
        content,
    }))
}
fn refs_for(c: &Connection, nodes: &[(String, Node)]) -> Result<Vec<EvidenceRef>> {
    let mut refs = BTreeMap::new();
    for (s, n) in nodes {
        refs.insert((s.clone(), n.id.clone()), ());
        for d in dependencies(c, &n.id)? {
            refs.insert(d, ());
        }
    }
    if refs.len() > MAX_DEPENDENCIES {
        return Err(Error::Limit("source dependency fan-out too large"));
    }
    let a = authority(c)?;
    refs.into_keys()
        .map(|(s, n)| {
            Ok(EvidenceRef {
                authority: a.clone(),
                session_id: s.clone(),
                node_id: n.clone(),
                sha256: digest(&node_in(c, &s, &n)?)?,
            })
        })
        .collect()
}
impl SessionStore {
    /// Authorized planning is local and has no inference side effect. It pins
    /// exact nodes and refuses oversized evidence rather than truncating it.
    pub fn context_prepare(
        &self,
        actor: &PrincipalId,
        request: PrepareContext,
    ) -> Result<ContextPlan> {
        if request.sources.len() > MAX_SOURCES
            || request.focus.len() > 4096
            || !(1024..=aw_agent_core::context::MAX_SUMMARY_INPUT)
                .contains(&request.max_input_bytes)
        {
            return Err(Error::Limit("context source/input limit"));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        target(&tx, actor, &request.target, true)?;
        let total: i64 = db(tx.query_row(
            "SELECT count(*) FROM context_plans WHERE actor=?1",
            [actor.as_str()],
            |r| r.get(0),
        ))?;
        if total >= MAX_PLANS {
            return Err(Error::Limit("summary plan retention bound reached"));
        }
        let target_path = path(&tx, &request.target.session_id, &request.target.head)?;
        let mut selected = Vec::<(String, Node)>::new();
        let mut sources = Vec::new();
        let mut keep_from = None;
        let mut selected_bytes = 0usize;
        match request.kind {
            PlanKind::Compact { keep_recent_turns } => {
                if !request.sources.is_empty() || keep_recent_turns > 256 {
                    return Err(Error::Config(
                        "compaction uses its active target prefix, not arbitrary sources".into(),
                    ));
                }
                let mut projection =
                    projected(&tx, &request.target.session_id, &request.target.head)?;
                let positions: Vec<_> = projection
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| n.kind == "turn")
                    .map(|(i, _)| i)
                    .collect();
                let cut = if keep_recent_turns == 0 {
                    projection.len()
                } else if positions.len() <= keep_recent_turns {
                    0
                } else {
                    positions[positions.len() - keep_recent_turns]
                };
                keep_from = projection.get(cut).map(|n| n.id.clone());
                projection.truncate(cut);
                selected.extend(
                    projection
                        .into_iter()
                        .map(|n| (request.target.session_id.clone(), n)),
                );
            }
            PlanKind::MaterializedFacts => {
                return Err(Error::Denied(
                    "materialized-fact context must use explicit project_materialized_context; it is not a summary plan",
                ));
            }
            PlanKind::Import => {
                if request.sources.is_empty() {
                    return Err(Error::Config("select at least one source".into()));
                }
                for input in request.sources {
                    authorize(&tx, &input.session_id, actor, false)?;
                    let b = branch_in(&tx, &input.session_id, &input.branch)?;
                    let mut p = path(&tx, &input.session_id, &b.head)?;
                    if let Some(head) = input.head {
                        let at = p
                            .iter()
                            .position(|n| n.id == head)
                            .ok_or(Error::Denied("source snapshot not in this branch"))?;
                        p.truncate(at + 1);
                    } else if p.last().is_some_and(|n| n.kind == "prompt") {
                        p.pop();
                    }
                    let head = p
                        .last()
                        .ok_or(Error::Denied("source unavailable"))?
                        .id
                        .clone();
                    if p.last().is_some_and(|n| n.kind == "prompt") {
                        return Err(Error::Conflict(
                            "source boundary must be committed and settled",
                        ));
                    }
                    authorize_node(&tx, &input.session_id, &head, actor)?;
                    let common = if input.session_id == request.target.session_id {
                        let target_ids: BTreeSet<_> =
                            target_path.iter().map(|n| n.id.as_str()).collect();
                        p.iter()
                            .rev()
                            .find(|n| target_ids.contains(n.id.as_str()))
                            .map(|n| n.id.clone())
                    } else {
                        None
                    };
                    let start = match &input.selection {
                        Selection::Standalone => 0,
                        Selection::Divergent => {
                            let common = common.as_ref().ok_or(Error::Config(
                                "different sessions need standalone or explicit range".into(),
                            ))?;
                            p.iter()
                                .position(|n| &n.id == common)
                                .ok_or(Error::Storage("missing common ancestor"))?
                                + 1
                        }
                        Selection::Range { start_exclusive } => {
                            let i = p
                                .iter()
                                .position(|n| &n.id == start_exclusive)
                                .ok_or(Error::Config("range start is not an ancestor".into()))?;
                            if p[i].kind == "prompt" {
                                return Err(Error::Config(
                                    "range cannot split a tool/turn span".into(),
                                ));
                            }
                            i + 1
                        }
                    };
                    // Standalone uses the current semantic source projection, not
                    // pre-checkpoint omitted data masquerading as active context.
                    let entries = if matches!(input.selection, Selection::Standalone) {
                        projected(&tx, &input.session_id, &head)?
                    } else {
                        p[start..].to_vec()
                    };
                    for n in entries {
                        selected_bytes = selected_bytes.saturating_add(json(&n.payload)?.len());
                        if selected_bytes > MAX_EXPORT_BYTES {
                            return Err(Error::Limit(
                                "combined source scan exceeds bound; select narrower ranges",
                            ));
                        }
                        selected.push((input.session_id.clone(), n));
                    }
                    sources.push(PinnedSource {
                        session_id: input.session_id,
                        branch_id: b.id,
                        branch_name: b.name,
                        head,
                        revision: b.revision,
                        selection: input.selection,
                        common_ancestor: common,
                    });
                }
            }
        }
        let mut used = BTreeSet::new();
        let mut selected_once = Vec::new();
        let mut duplicate_nodes_omitted = 0;
        for (s, n) in selected {
            if matches!(n.kind.as_str(), "root" | "prompt") {
                continue;
            }
            if !used.insert((s.clone(), n.id.clone())) {
                duplicate_nodes_omitted += 1;
                continue;
            }
            authorize_node(&tx, &s, &n.id, actor)?;
            selected_once.push((s, n));
        }
        if selected_once.is_empty() {
            return Err(Error::Config(
                "no selected divergent/prefix evidence; use plain navigation".into(),
            ));
        }
        let dependencies = refs_for(&tx, &selected_once)?;
        check_sources(&tx, actor, &dependencies)?;
        source_use(&tx, actor, &dependencies, None)?;
        // Candidate stays private; the destination audience must already be a
        // subset before any model call is offered, and is checked again on apply.
        disclosure(
            &tx,
            &dependencies,
            &audience(&tx, &request.target.session_id)?,
        )?;
        let mut ev = Vec::new();
        let mut evidence_bytes = 0usize;
        for (s, n) in &selected_once {
            if let Some(e) = evidence(&tx, &session_in(&tx, s)?, n)? {
                evidence_bytes = evidence_bytes.saturating_add(json(&e)?.len());
                if evidence_bytes > request.max_input_bytes {
                    return Err(Error::Limit(
                        "selected evidence exceeds summary budget; nothing omitted silently",
                    ));
                }
                ev.push(e);
            }
        }
        let release_versions = release_versions(&tx, actor, &dependencies)?;
        let plan = ContextPlan {
            version: 1,
            id: id("cp")?,
            actor: actor.clone(),
            target: request.target,
            kind: request.kind,
            sources,
            evidence: ev,
            dependencies,
            release_versions,
            duplicate_nodes_omitted,
            keep_from,
            focus: request.focus,
            max_output_bytes: request.max_output_bytes,
            timeout_seconds: request.timeout_seconds,
            created_at: now(),
        };
        let input = plan.input()?;
        let messages = input.messages()?;
        if messages
            .iter()
            .map(aw_agent_core::Message::context_bytes)
            .sum::<usize>()
            > request.max_input_bytes
        {
            return Err(Error::Limit(
                "selected evidence exceeds summary input budget; nothing omitted silently",
            ));
        }
        let digest = input.digest()?;
        let actor_user = ensure_user_for_principal(&tx, actor)?;
        db(tx.execute("INSERT INTO context_plans(id,actor,target_session,body,digest,created_at,actor_user_id) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![plan.id,actor.as_str(),plan.target.session_id,json(&plan)?,digest,now(),actor_user]))?;
        db(tx.commit())?;
        Ok(plan)
    }
    pub fn context_plan(&self, actor: &PrincipalId, id: &str) -> Result<ContextPlan> {
        let c = self.connection()?;
        plan_in(&c, actor, id)
    }
    pub fn context_artifact(&self, actor: &PrincipalId, id: &str) -> Result<SummaryArtifact> {
        let c = self.connection()?;
        check_artifact(&c, actor, id, true)
    }
    pub fn context_target(
        &self,
        actor: &PrincipalId,
        session: &str,
        branch: &str,
    ) -> Result<TargetHead> {
        let c = self.connection()?;
        authorize(&c, session, actor, true)?;
        let b = branch_in(&c, session, branch)?;
        authorize_node(&c, session, &b.head, actor)?;
        Ok(TargetHead::from(&b))
    }
}
pub(crate) fn plan_in(c: &Connection, actor: &PrincipalId, id: &str) -> Result<ContextPlan> {
    let actor_user = user_for_principal(c, actor)?;
    let body: Option<String> = db(c
        .query_row(
            "SELECT body FROM context_plans WHERE id=?1 AND ((actor_user_id IS NULL AND actor=?2) OR (?3 IS NOT NULL AND actor_user_id=?3))",
            params![id, actor.as_str(), actor_user],
            |r| r.get(0),
        )
        .optional())?;
    let plan: ContextPlan =
        decode(&body.ok_or(Error::Denied("summary plan unavailable or not permitted"))?)?;
    check_sources(c, actor, &plan.dependencies)?;
    source_use(c, actor, &plan.dependencies, None)?;
    check_release_versions(c, actor, &plan.release_versions)?;
    target(c, actor, &plan.target, false)?;
    disclosure(
        c,
        &plan.dependencies,
        &audience(c, &plan.target.session_id)?,
    )?;
    Ok(plan)
}

/// A newly granted viewer must already be entitled to every dependency. This
/// is conservative whole-session disclosure, not automatic declassification.
pub(crate) fn authorize_dependencies_for_grant(
    c: &Connection,
    session: &str,
    member: &PrincipalId,
) -> Result<()> {
    let private: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM context_private_sessions WHERE session_id=?1)",
        [session],
        |r| r.get(0),
    ))?;
    if private && !principal_is_session_owner(c, session, member)? {
        return Err(Error::Denied("external context remains owner-private"));
    }
    let mut q=db(c.prepare("SELECT DISTINCT d.source_session,d.source_node FROM node_dependencies d JOIN nodes n ON n.id=d.node_id WHERE n.session_id=?1 LIMIT 513"))?;
    let rows = db(db(q.query_map([session], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    }))?
    .collect::<rusqlite::Result<Vec<_>>>())?;
    if rows.len() > MAX_DEPENDENCIES {
        return Err(Error::Limit("session dependency grant limit"));
    }
    for (s, n) in rows {
        if s != session {
            authorize_node(c, &s, &n, member)?;
        }
    }
    Ok(())
}

/// Additional owner permission for cross-owner source use. A session read grant
/// is not an inference-destination grant. Account labels refer to trusted current
/// registrations; they are not proof of a remotely attested physical endpoint.
pub(crate) fn source_use(
    c: &Connection,
    actor: &PrincipalId,
    refs: &[EvidenceRef],
    account: Option<&str>,
) -> Result<()> {
    let mut sessions = BTreeSet::new();
    for s in refs {
        sessions.insert(s.session_id.clone());
    }
    for session in sessions {
        authorize(c, &session, actor, false)?;
        if principal_is_session_owner(c, &session, actor)? {
            continue;
        }
        let actor_user = user_for_principal(c, actor)?;
        let grants:Option<String>=db(c.query_row("SELECT accounts_json FROM context_source_grants WHERE session_id=?1 AND ((principal_user_id IS NULL AND principal=?2) OR (?3 IS NOT NULL AND principal_user_id=?3))",params![session,actor.as_str(),actor_user],|r|r.get(0)).optional())?;
        let allowed: Vec<String> =
            decode(&grants.ok_or(Error::Denied("source owner has not granted context use"))?)?;
        if allowed.is_empty() || account.is_some_and(|id| !allowed.iter().any(|v| v == id)) {
            return Err(Error::Denied(
                "summary account is outside the source owner's explicit release",
            ));
        }
    }
    Ok(())
}
fn check_derivations(c: &Connection, node: &str) -> Result<()> {
    let mut q=db(c.prepare("SELECT source_session,requester,grant_id,requester_user_id FROM context_derivations WHERE node_id=?1 LIMIT 513"))?;
    let rows = db(db(q.query_map([node], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
        ))
    }))?
    .collect::<rusqlite::Result<Vec<_>>>())?;
    if rows.len() > MAX_DEPENDENCIES {
        return Err(Error::Limit("derivation authority bound"));
    }
    for (session, requester, grant, requester_user_id) in rows {
        let actor = requester_user_id
            .as_deref()
            .map(stable_user_principal)
            .transpose()?
            .unwrap_or(PrincipalId::new(requester)?);
        authorize(c, &session, &actor, false)?;
        if !principal_is_session_owner(c, &session, &actor)? {
            let actor_user = user_for_principal(c, &actor)?;
            let active:bool=db(c.query_row("SELECT EXISTS(SELECT 1 FROM context_source_grants WHERE session_id=?1 AND ((principal_user_id IS NULL AND principal=?2) OR (?4 IS NOT NULL AND principal_user_id=?4)) AND grant_id=?3 AND json_array_length(accounts_json)>0)",params![session,actor.as_str(),grant,actor_user],|r|r.get(0)))?;
            if !active {
                return Err(Error::Denied("derived content release was revoked"));
            }
        }
    }
    Ok(())
}
impl SessionStore {
    /// Owner-only source sharing; this does not permit appending to the source,
    /// borrow a provider key, or create a public/team-wide release.
    pub fn context_grant_read(
        &self,
        owner: &PrincipalId,
        session: &str,
        reader: &PrincipalId,
        allow: bool,
        accounts: Vec<String>,
    ) -> Result<()> {
        if accounts.len() > 16 || (!allow && !accounts.is_empty()) {
            return Err(Error::Config("invalid source-account release".into()));
        }
        for account in &accounts {
            PrincipalId::new(account.clone())?;
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        if !principal_is_session_owner(&tx, session, owner)? || same_user(&tx, reader, owner)? {
            return Err(Error::Denied(
                "only the source owner can grant another reader",
            ));
        }
        if allow {
            authorize_dependencies_for_grant(&tx, session, reader)?;
            let reader_user = ensure_user_for_principal(&tx, reader)?;
            db(tx.execute("INSERT INTO members(session_id,principal,can_append,principal_user_id) VALUES(?1,?2,0,?3) ON CONFLICT(session_id,principal) DO UPDATE SET can_append=0,principal_user_id=excluded.principal_user_id",params![session,reader.as_str(),reader_user]))?;
        } else {
            let reader_user = user_for_principal(&tx, reader)?;
            db(tx.execute(
                "DELETE FROM members WHERE session_id=?1 AND (principal=?2 OR (?3 IS NOT NULL AND principal_user_id=?3))",
                params![session, reader.as_str(), reader_user],
            ))?;
        }
        let reader_user = user_for_principal(&tx, reader)?;
        db(tx.execute(
            "DELETE FROM context_source_grants WHERE session_id=?1 AND (principal=?2 OR (?3 IS NOT NULL AND principal_user_id=?3))",
            params![session, reader.as_str(), reader_user],
        ))?;
        if allow && !accounts.is_empty() {
            let reader_user = ensure_user_for_principal(&tx, reader)?;
            db(tx.execute("INSERT INTO context_source_grants(session_id,principal,accounts_json,grant_id,principal_user_id) VALUES(?1,?2,?3,?4,?5)",params![session,reader.as_str(),json(&accounts)?,id("release")?,reader_user]))?;
        }
        audit(
            &tx,
            session,
            owner,
            "context.source_release",
            serde_json::json!({"reader":reader,"read":allow,"summary_accounts":accounts}),
        )?;
        db(tx.commit())?;
        Ok(())
    }
}

fn release_versions(
    c: &Connection,
    actor: &PrincipalId,
    refs: &[EvidenceRef],
) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for s in refs {
        if !principal_is_session_owner(c, &s.session_id, actor)? {
            let actor_user = user_for_principal(c, actor)?;
            let id: String = db(c.query_row(
                "SELECT grant_id FROM context_source_grants WHERE session_id=?1 AND ((principal_user_id IS NULL AND principal=?2) OR (?3 IS NOT NULL AND principal_user_id=?3))",
                params![s.session_id, actor.as_str(), actor_user],
                |r| r.get(0),
            ))?;
            out.insert(s.session_id.clone(), id);
        }
    }
    Ok(out)
}
fn check_release_versions(
    c: &Connection,
    actor: &PrincipalId,
    versions: &BTreeMap<String, String>,
) -> Result<()> {
    for (session, expected) in versions {
        let actor_user = user_for_principal(c, actor)?;
        let valid:bool=db(c.query_row("SELECT EXISTS(SELECT 1 FROM context_source_grants WHERE session_id=?1 AND ((principal_user_id IS NULL AND principal=?2) OR (?4 IS NOT NULL AND principal_user_id=?4)) AND grant_id=?3)",params![session,actor.as_str(),expected,actor_user],|r|r.get(0)))?;
        if !valid {
            return Err(Error::Denied(
                "source release changed; derived context withheld",
            ));
        }
    }
    Ok(())
}

pub(crate) fn prompt_artifacts(c: &Connection, prompt: &str) -> Result<Vec<String>> {
    let mut q=db(c.prepare("SELECT artifact_id FROM context_prompt_attachments WHERE prompt_node=?1 ORDER BY artifact_id LIMIT 9"))?;
    let out =
        db(db(q.query_map([prompt], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
    if out.len() > 8 {
        return Err(Error::Limit("prompt context attachment bound"));
    }
    Ok(out)
}
fn prompt_evidence(c: &Connection, prompt: &str) -> Result<Vec<serde_json::Value>> {
    prompt_artifacts(c,prompt)?.into_iter().map(|id|{let a=artifact_in(c,&id)?;Ok(serde_json::json!({"artifact_id":id,"text":a.text,"sources":a.plan.dependencies,"unverified":a.imported_unverified}))}).collect()
}

/// A read grant is not permission to send derived facts to an arbitrary model.
/// Check the selected current account for all inherited and one-shot sources.
/// The host still owns concrete endpoint/credential registration; IDs are not
/// cryptographic attestation of its configured physical destination.
pub(crate) fn authorize_model_context(
    c: &Connection,
    session: &str,
    branch: &BranchInfo,
    identity: &aw_agent_core::InvocationIdentity,
) -> Result<()> {
    match authorize(c, session, &identity.actor, true) {
        Ok(()) => {}
        Err(Error::Denied(_)) => {
            #[cfg(feature = "multiplayer")]
            if crate::multiplayer::draft_submit_authorized_in(c, session, &identity.actor)? {
                return authorize_model_context_after_append_authorized(
                    c, session, branch, identity,
                );
            }
            return Err(Error::Denied(
                "model context requires Session append or shared submit authority",
            ));
        }
        Err(error) => return Err(error),
    }
    authorize_model_context_after_append_authorized(c, session, branch, identity)
}

fn authorize_model_context_after_append_authorized(
    c: &Connection,
    session: &str,
    branch: &BranchInfo,
    identity: &aw_agent_core::InvocationIdentity,
) -> Result<()> {
    authorize_node(c, session, &branch.head, &identity.actor)?;
    materialized::authorize_projected_for_model(c, &identity.actor, session, branch)?;
    let mut refs = Vec::new();
    let authority = authority(c)?;
    for (session_id, node_id) in dependencies(c, &branch.head)? {
        let n = node_in(c, &session_id, &node_id)?;
        refs.push(EvidenceRef {
            authority: authority.clone(),
            session_id,
            node_id,
            sha256: digest(&n)?,
        });
    }
    for block in apply::pending_context(c, &identity.actor, session, branch)? {
        refs.extend(artifact_refs(c, &block.artifact_id)?);
    }
    if refs.len() > MAX_DEPENDENCIES * 2 {
        return Err(Error::Limit("model source closure exceeded"));
    }
    check_sources(c, &identity.actor, &refs)?;
    source_use(c, &identity.actor, &refs, Some(&identity.account_id))?;
    ensure_reviewed_sources(c, &refs)
}
pub(crate) fn ensure_reviewed_sources(c: &Connection, refs: &[EvidenceRef]) -> Result<()> {
    for s in refs {
        let review: bool = db(c.query_row(
            "SELECT import_review_needed<>0 FROM sessions WHERE id=?1",
            [&s.session_id],
            |r| r.get(0),
        ))?;
        if review {
            return Err(Error::Denied(
                "review imported source before generating or reusing model context",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod phase30_artifact_owner_regressions {
    use super::*;

    #[test]
    fn stable_artifact_owner_projection_beats_same_historical_principal_text() {
        let store = SessionStore::in_memory().unwrap();
        let local = PrincipalId::new("unix:1000").unwrap();
        let local_user = store.current_user(&local).unwrap();
        let c = store.connection().unwrap();
        c.execute(
            "INSERT INTO users(user_id,realm_id,status,display_json,security_epoch,created_at) VALUES('usr_remote_fixture','remote','active','{}',1,999)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO context_artifacts(id,owner,plan_id,operation_id,body,created_at,owner_user_id) VALUES('ca_owner_collision','unix:1000',NULL,NULL,'{}',1,'usr_remote_fixture')",
            [],
        )
        .unwrap();
        assert_ne!(local_user.user_id, "usr_remote_fixture");
        assert_eq!(
            artifact_owner_principal(&c, "ca_owner_collision", &local)
                .unwrap()
                .as_str(),
            "user:usr_remote_fixture"
        );
        assert!(!artifact_owned_by(&c, "ca_owner_collision", &local, &local).unwrap());
    }
}
