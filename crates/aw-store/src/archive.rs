use super::*;
use aw_agent_core::{InvocationIdentity, Turn, TurnStart};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
};

fn encoded_line(value: &ExportLine) -> Result<Vec<u8>> {
    let mut b =
        serde_json::to_vec(value).map_err(|_| Error::Storage("export serialization failed"))?;
    b.push(b'\n');
    Ok(b)
}
fn valid_object_id(s: &str) -> bool {
    s.len() <= 128
        && !s.is_empty()
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
}
fn validate_identity(i: &InvocationIdentity) -> Result<()> {
    for s in [&i.account_id, &i.inference_executor] {
        PrincipalId::new(s.clone())?;
    }
    match &i.account_owner {
        aw_agent_core::AccountOwner::User(p) => {
            PrincipalId::new(p.as_str())?;
        }
        aw_agent_core::AccountOwner::Server(s) => {
            PrincipalId::new(s.clone())?;
        }
    }
    match &i.payer {
        aw_agent_core::BillingOwner::User(p) => {
            PrincipalId::new(p.as_str())?;
        }
        aw_agent_core::BillingOwner::Shared(s) => {
            PrincipalId::new(s.clone())?;
        }
    }
    match &i.credential_custody {
        aw_agent_core::CredentialCustody::UserDevice(s)
        | aw_agent_core::CredentialCustody::Server(s)
        | aw_agent_core::CredentialCustody::BrowserMemory(s) => {
            PrincipalId::new(s.clone())?;
        }
    }
    label(&i.model, 256)
}
impl SessionStore {
    /// Complete idle tree and branch refs. Does not export login tokens, configured
    /// provider secrets/grants, private runtime continuations or active requests.
    pub fn export_jsonl(&self, actor: &PrincipalId, session: &str) -> Result<Vec<u8>> {
        let c = self.connection()?;
        super::context::authorize_tree(&c, session, actor)?;
        if session_in(&c, session)?.owner != *actor {
            return Err(Error::Denied(
                "read-only session access does not grant a portable export release",
            ));
        }
        let pending: i64 = db(c.query_row(
            "SELECT count(*) FROM runs WHERE session_id=?1 AND status='running'",
            [session],
            |r| r.get(0),
        ))?;
        if pending != 0 {
            return Err(Error::Conflict("settle or recover runs before exporting"));
        }
        let nodes = nodes_in(&c, session)?;
        let mut artifacts = BTreeMap::new();
        let mut release = BTreeMap::new();
        let mut prompt_contexts = Vec::new();
        for n in &nodes {
            if n.kind == "prompt" {
                let ids = super::context::prompt_artifacts(&c, &n.id)?;
                for id in &ids {
                    let a = super::context::check_artifact(&c, actor, id, false)?;
                    artifacts.insert(id.clone(), a);
                }
                if !ids.is_empty() {
                    prompt_contexts.push((n.id.clone(), ids));
                }
            }
            for source in super::context::dependencies(&c, &n.id)? {
                // Exporting a self-contained copy is a disclosure. Read membership
                // alone cannot release another owner's revocable information.
                if session_in(&c, &source.0)?.owner != *actor {
                    return Err(Error::Denied(
                        "cross-owner derived export needs a separately approved release",
                    ));
                }
                let sn = node_in(&c, &source.0, &source.1)?;
                release.insert(
                    source.clone(),
                    aw_agent_core::EvidenceRef {
                        authority: super::context::authority(&c)?,
                        session_id: source.0,
                        node_id: source.1,
                        sha256: super::context::digest(&sn)?,
                    },
                );
            }
            if n.kind == "context_import" || n.kind == "context_compaction" {
                let reference: crate::ContextNode = serde_json::from_value(n.payload.clone())
                    .map_err(|_| Error::Storage("invalid context node"))?;
                let a = super::context::check_artifact(&c, actor, &reference.artifact_id, false)?;
                artifacts.insert(a.id.clone(), a);
            }
        }
        let version = if artifacts.is_empty() && release.is_empty() {
            1
        } else {
            2
        };
        let mut out = encoded_line(&ExportLine::Header {
            format: "aw.session-tree".into(),
            version,
            session: session_in(&c, session)?,
        })?;
        if !release.is_empty() {
            out.extend(encoded_line(&ExportLine::PrivateRelease {
                sources: release.into_values().collect(),
            })?);
        }
        for artifact in artifacts.into_values() {
            out.extend(encoded_line(&ExportLine::Artifact { artifact })?);
            if out.len() > MAX_EXPORT_BYTES {
                return Err(Error::Limit("artifact export exceeds budget"));
            }
        }
        for (prompt_node, artifact_ids) in prompt_contexts {
            out.extend(encoded_line(&ExportLine::PromptContext {
                prompt_node,
                artifact_ids,
            })?);
        }
        for node in nodes {
            out.extend(encoded_line(&ExportLine::Node { node })?);
            if out.len() > MAX_EXPORT_BYTES {
                return Err(Error::Limit("export exceeds 32 MiB"));
            }
        }
        let mut q =
            db(c.prepare("SELECT id FROM branches WHERE session_id=?1 ORDER BY created_at,id"))?;
        let ids = db(db(q.query_map([session], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
        for b in ids {
            out.extend(encoded_line(&ExportLine::Branch {
                branch: branch_in(&c, session, &b)?,
            })?);
        }
        let digest = format!("{:x}", Sha256::digest(&out));
        out.extend(encoded_line(&ExportLine::Footer { sha256: digest })?);
        if out.len() > MAX_EXPORT_BYTES {
            return Err(Error::Limit("export exceeds 32 MiB"));
        }
        Ok(out)
    }
    pub fn export_file(&self, actor: &PrincipalId, session: &str, path: &Path) -> Result<()> {
        // Portable user-authorized export is a sensitive, owner-only file.
        // Complete authority and bounds checks BEFORE create_new, then only
        // remove a truncated artifact if THIS request actually created it.
        // A pre-existing file/symlink must never be unlinked on rejection.
        let data = self.export_jsonl(actor, session)?;
        let mut file = private_new(path)?;
        let result: Result<()> = (|| {
            file.write_all(&data)?;
            file.sync_all()?;
            aw_platform::sync_parent(path)?;
            Ok(())
        })();
        // Close our owned file handle before cleanup; Unix permits unlinking
        // an open inode, but a future owner-only Windows backend may not.
        drop(file);
        if result.is_err() {
            let _ = std::fs::remove_file(path);
        }
        result
    }
    /// Imports history only, into a NEW ID/owner and an explicitly selected local
    /// binding. Source authors survive as unverified provenance; no grants, live
    /// requests, model/tool execution or billable ledger entries are imported.
    pub fn import_jsonl(
        &self,
        owner: &PrincipalId,
        data: &[u8],
        binding: SessionBinding,
    ) -> Result<(SessionInfo, BranchInfo)> {
        validate_binding(&binding)?;
        if data.len() > MAX_EXPORT_BYTES || !data.ends_with(b"\n") {
            return Err(Error::Limit(
                "JSONL import must be bounded and newline terminated",
            ));
        }
        let text =
            std::str::from_utf8(data).map_err(|_| Error::Config("import must be UTF-8".into()))?;
        let mut prompt_contexts = BTreeMap::<String, Vec<String>>::new();
        let mut version = 0;
        let mut artifacts = BTreeMap::<String, crate::SummaryArtifact>::new();
        let mut release = None;
        let mut header = None;
        let mut nodes = Vec::<Node>::new();
        let mut branches = Vec::<BranchInfo>::new();
        let mut end = false;
        let mut offset = 0usize;
        for (n, line) in text.split_inclusive('\n').enumerate() {
            if end {
                return Err(Error::Protocol("data after export footer"));
            }
            if line.len() > 4 * 1024 * 1024 {
                return Err(Error::Limit("import line too large"));
            }
            let item: ExportLine = serde_json::from_str(line)
                .map_err(|_| Error::Protocol("invalid JSONL record/schema"))?;
            match item {
                ExportLine::Header {
                    format,
                    version: v,
                    session,
                } => {
                    if n != 0 || format != "aw.session-tree" || !(1..=2).contains(&v) {
                        return Err(Error::Protocol("unsupported export header"));
                    }
                    version = v;
                    header = Some(session);
                }
                ExportLine::PrivateRelease { sources } => {
                    if version != 2
                        || header.is_none()
                        || !nodes.is_empty()
                        || release.is_some()
                        || sources.len() > super::context::MAX_DEPENDENCIES
                    {
                        return Err(Error::Protocol("invalid source-release envelope"));
                    }
                    for s in &sources {
                        aw_agent_core::validate_evidence_ref(s)?;
                    }
                    release = Some(sources);
                }
                ExportLine::Artifact { artifact } => {
                    if version != 2
                        || header.is_none()
                        || !nodes.is_empty()
                        || artifacts.len() >= aw_agent_core::MAX_CONTEXT_BLOCKS
                        || artifacts.contains_key(&artifact.id)
                    {
                        return Err(Error::Protocol("invalid artifact order/count"));
                    }
                    if !valid_object_id(&artifact.id)
                        || artifact.version != 1
                        || artifact.text.trim().is_empty()
                        || artifact.text.len() > aw_agent_core::MAX_SUMMARY_OUTPUT
                        || artifact.text_sha256
                            != super::context::operations::text_hash(&artifact.text)
                    {
                        return Err(Error::Protocol("invalid summary artifact"));
                    }
                    artifact.plan.validate_stored()?;
                    if let Some(identity) = &artifact.identity {
                        validate_identity(identity)?;
                    }
                    artifacts.insert(artifact.id.clone(), artifact);
                }
                ExportLine::PromptContext {
                    prompt_node,
                    artifact_ids,
                } => {
                    if version != 2
                        || header.is_none()
                        || !nodes.is_empty()
                        || artifact_ids.is_empty()
                        || artifact_ids.len() > 8
                        || prompt_contexts.len() >= MAX_SESSION_NODES
                        || prompt_contexts.contains_key(&prompt_node)
                        || artifact_ids.iter().any(|id| !artifacts.contains_key(id))
                    {
                        return Err(Error::Protocol("invalid prompt context references"));
                    }
                    prompt_contexts.insert(prompt_node, artifact_ids);
                }
                ExportLine::Node { node } => {
                    if header.is_none() || !branches.is_empty() || nodes.len() >= MAX_SESSION_NODES
                    {
                        return Err(Error::Protocol("invalid node order or count"));
                    }
                    nodes.push(node);
                }
                ExportLine::Branch { branch } => {
                    if header.is_none() || branches.len() >= 1024 {
                        return Err(Error::Protocol("invalid branch count"));
                    }
                    branches.push(branch);
                }
                ExportLine::Footer { sha256 } => {
                    if sha256 != format!("{:x}", Sha256::digest(&data[..offset])) {
                        return Err(Error::Protocol("export checksum mismatch"));
                    }
                    end = true;
                }
            }
            offset += line.len();
        }
        let source = header.ok_or(Error::Protocol("missing export header"))?;
        if !end || nodes.is_empty() || branches.is_empty() {
            return Err(Error::Protocol("incomplete export"));
        }
        label(&source.title, 160)?;
        let mut seen = BTreeMap::<String, Node>::new();
        let mut max_turn = 0u64;
        let mut turn_ids = BTreeSet::new();
        for (index, n) in nodes.iter_mut().enumerate() {
            if !valid_object_id(&n.id) || seen.contains_key(&n.id) || n.created_at < 0 {
                return Err(Error::Protocol("invalid/duplicate imported node"));
            }
            match n.kind.as_str() {
                "root" => {
                    if index != 0 || n.parent_id.is_some() || n.id != source.root_node {
                        return Err(Error::Protocol("invalid imported root"));
                    }
                    n.payload = serde_json::json!({"format":"aw.session","version":1});
                }
                "prompt" => {
                    let parent = n
                        .parent_id
                        .as_ref()
                        .and_then(|id| seen.get(id))
                        .ok_or(Error::Protocol("missing imported parent"))?;
                    if parent.kind == "prompt" {
                        return Err(Error::Protocol("nested unresolved prompt"));
                    }
                    let start: TurnStart = serde_json::from_value(n.payload.clone())
                        .map_err(|_| Error::Protocol("invalid imported prompt"))?;
                    if start.turn_id == 0
                        || start.turn_id > (1u64 << 42) - 1
                        || start.prompt.trim().is_empty()
                        || start.prompt.len() > aw_agent_core::limits::MAX_PROMPT_BYTES
                        || start.identity.actor != n.actor
                    {
                        return Err(Error::Protocol(
                            "invalid imported prompt identity or bounds",
                        ));
                    }
                    validate_identity(&start.identity)?;
                    n.payload = serde_json::to_value(start)
                        .map_err(|_| Error::Protocol("invalid prompt"))?;
                }
                "turn" => {
                    let parent = n
                        .parent_id
                        .as_ref()
                        .and_then(|id| seen.get(id))
                        .ok_or(Error::Protocol("missing turn parent"))?;
                    if parent.kind != "prompt" {
                        return Err(Error::Protocol("turn must complete a prompt"));
                    }
                    let mut t: Turn = serde_json::from_value(n.payload.clone())
                        .map_err(|_| Error::Protocol("invalid turn payload"))?;
                    if t.id == 0 || t.id > (1u64 << 42) - 1 || !turn_ids.insert(t.id) {
                        return Err(Error::Protocol("duplicate or invalid turn ID"));
                    }
                    let start: TurnStart = serde_json::from_value(parent.payload.clone())
                        .map_err(|_| Error::Protocol("invalid prompt"))?;
                    if start.turn_id != t.id
                        || start.prompt != t.prompt
                        || t.actor.as_ref() != Some(&n.actor)
                        || t.actor.as_ref() != Some(&start.identity.actor)
                        || t.profile != start.identity.account_id
                        || t.model != start.identity.model
                    {
                        return Err(Error::Protocol("turn does not match authored parent"));
                    }
                    for (i, inv) in t.invocations.iter_mut().enumerate() {
                        validate_identity(&inv.identity)?;
                        if inv.ordinal != (i + 1) as u32 {
                            return Err(Error::Protocol("invalid invocation order"));
                        }
                        inv.usage_verified = false;
                    }
                    super::types::validate_turn(&t)?;
                    max_turn = max_turn.max(t.id);
                    n.payload =
                        serde_json::to_value(t).map_err(|_| Error::Protocol("invalid turn"))?;
                }
                "checkpoint" => {
                    let p = n
                        .parent_id
                        .as_ref()
                        .and_then(|id| seen.get(id))
                        .ok_or(Error::Protocol("missing checkpoint parent"))?;
                    if p.kind == "prompt" {
                        return Err(Error::Protocol("checkpoint cannot split an active prompt"));
                    }
                    let text = n.payload["summary"]
                        .as_str()
                        .ok_or(Error::Protocol("invalid checkpoint"))?;
                    if text.trim().is_empty()
                        || text.len() > aw_agent_core::limits::MAX_PROMPT_BYTES
                    {
                        return Err(Error::Limit("checkpoint bounds"));
                    }
                    n.payload = serde_json::json!({"summary":text,"authored_by_user":true});
                }
                "context_import" | "context_compaction" => {
                    let parent = n
                        .parent_id
                        .as_ref()
                        .and_then(|id| seen.get(id))
                        .ok_or(Error::Protocol("missing context parent"))?;
                    if parent.kind == "prompt" {
                        return Err(Error::Protocol("context cannot split unresolved prompt"));
                    }
                    let data: crate::ContextNode = serde_json::from_value(n.payload.clone())
                        .map_err(|_| Error::Protocol("invalid context node"))?;
                    if !artifacts.contains_key(&data.artifact_id) {
                        return Err(Error::Protocol("context artifact missing"));
                    }
                    if n.kind == "context_import" && data.keep_from.is_some() {
                        return Err(Error::Protocol("additive import cannot cut context"));
                    }
                    if let Some(keep) = &data.keep_from {
                        let mut cursor = Some(parent.id.clone());
                        let mut found = false;
                        while let Some(id) = cursor {
                            let p = seen
                                .get(&id)
                                .ok_or(Error::Protocol("invalid keep ancestry"))?;
                            if &id == keep {
                                found = p.kind != "prompt";
                                break;
                            }
                            cursor = p.parent_id.clone();
                        }
                        if !found {
                            return Err(Error::Protocol(
                                "compaction keep boundary is not a safe ancestor",
                            ));
                        }
                    }
                }
                _ => return Err(Error::Protocol("unknown imported node kind")),
            }
            seen.insert(n.id.clone(), n.clone());
        }
        // Every source prompt must be settled; no imported pending effect is runnable.
        for n in nodes.iter().filter(|n| n.kind == "prompt") {
            if nodes
                .iter()
                .filter(|child| child.kind == "turn" && child.parent_id.as_deref() == Some(&n.id))
                .count()
                != 1
            {
                return Err(Error::Protocol(
                    "export contains an unresolved or multiply-finished prompt",
                ));
            }
        }
        for prompt in prompt_contexts.keys() {
            if seen.get(prompt).is_none_or(|n| n.kind != "prompt") {
                return Err(Error::Protocol("attachment needs an imported prompt"));
            }
        }
        let mut names = BTreeSet::new();
        let mut branch_ids = BTreeSet::new();
        for b in &branches {
            label(&b.name, 80)?;
            if b.name.starts_with("b_") {
                return Err(Error::Protocol("reserved imported branch name"));
            }
            if b.active_run.is_some()
                || b.session_id != source.id
                || !valid_object_id(&b.id)
                || !branch_ids.insert(b.id.clone())
                || !names.insert(b.name.clone())
                || seen.get(&b.head).is_none_or(|n| n.kind == "prompt")
            {
                return Err(Error::Protocol("invalid exported branch"));
            }
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (created, default) = create_in(&tx, owner, &source.title, &binding)?;
        let private_release = !artifacts.is_empty() || release.is_some();
        let mut artifact_map = BTreeMap::new();
        for (_, mut a) in artifacts {
            let old = a.id.clone();
            a.id = id("ca")?;
            a.owner = owner.clone();
            a.imported_unverified = true;
            // Provenance is retained in the body, not registered as local sources,
            // invocation receipts, memberships or evidence of native authorship.
            db(tx.execute("INSERT INTO context_artifacts(id,owner,plan_id,operation_id,body,created_at) VALUES(?1,?2,NULL,NULL,?3,?4)",params![a.id,owner.as_str(),json(&a)?,now()]))?;
            artifact_map.insert(old, a.id);
        }
        if private_release {
            db(tx.execute(
                "INSERT INTO context_private_sessions(session_id) VALUES(?1)",
                [&created.id],
            ))?;
        }
        let mut mapped = BTreeMap::from([(source.root_node.clone(), created.root_node.clone())]);
        for n in nodes.iter().skip(1) {
            let new = id("n")?;
            let mut payload = n.payload.clone();
            if n.kind == "context_import" || n.kind == "context_compaction" {
                let mut data: crate::ContextNode = serde_json::from_value(payload)
                    .map_err(|_| Error::Protocol("invalid context data"))?;
                data.artifact_id = artifact_map
                    .get(&data.artifact_id)
                    .ok_or(Error::Protocol("missing remapped artifact"))?
                    .clone();
                data.keep_from = data
                    .keep_from
                    .map(|id| {
                        mapped
                            .get(&id)
                            .cloned()
                            .ok_or(Error::Protocol("invalid retained-tail mapping"))
                    })
                    .transpose()?;
                payload = serde_json::to_value(data)
                    .map_err(|_| Error::Protocol("invalid context mapping"))?;
            }
            let parent = n
                .parent_id
                .as_ref()
                .and_then(|p| mapped.get(p))
                .ok_or(Error::Protocol("invalid imported mapping"))?;
            db(tx.execute("INSERT INTO nodes(id,session_id,parent_id,kind,actor,payload,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![new,created.id,parent,n.kind,n.actor.as_str(),json(&payload)?,n.created_at]))?;
            mapped.insert(n.id.clone(), new);
        }
        for (prompt, ids) in prompt_contexts {
            for id in ids {
                db(tx.execute(
                    "INSERT INTO context_prompt_attachments(prompt_node,artifact_id) VALUES(?1,?2)",
                    params![
                        mapped
                            .get(&prompt)
                            .ok_or(Error::Protocol("missing attachment prompt"))?,
                        artifact_map
                            .get(&id)
                            .ok_or(Error::Protocol("missing attachment artifact"))?
                    ],
                ))?;
            }
        }
        let mut selected = None;
        for (i, b) in branches.iter().enumerate() {
            let head = mapped
                .get(&b.head)
                .ok_or(Error::Protocol("missing imported head"))?;
            if i == 0 {
                db(tx.execute(
                    "UPDATE branches SET name=?1,head=?2 WHERE id=?3",
                    params![b.name, head, default.id],
                ))?;
                selected = Some(default.id.clone());
            } else {
                db(tx.execute("INSERT INTO branches(id,session_id,name,head,created_at) VALUES(?1,?2,?3,?4,?5)",params![id("b")?,created.id,b.name,head,now()]))?;
            }
        }
        db(tx.execute(
            "UPDATE sessions SET next_turn=?1,import_review_needed=1 WHERE id=?2",
            params![max_turn + 1, created.id],
        ))?;
        audit(
            &tx,
            &created.id,
            owner,
            "session.imported",
            serde_json::json!({"source_session":source.id,"sha256":format!("{:x}",Sha256::digest(data)),"authors_and_usage":"unverified imported provenance; excluded from native accounting"}),
        )?;
        // Validate the semantic projection too, not only raw parent ordering.
        // Imported views remain behind explicit review and a private-owner fence.
        for b in &branches {
            let local = mapped
                .get(&b.head)
                .ok_or(Error::Protocol("missing mapped branch"))?;
            let _ = super::context::projected(&tx, &created.id, local)?;
        }
        let session = session_in(&tx, &created.id)?;
        let branch = branch_in(
            &tx,
            &created.id,
            &selected.ok_or(Error::Protocol("missing import branch"))?,
        )?;
        db(tx.commit())?;
        Ok((session, branch))
    }
}
