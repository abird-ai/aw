//! Immutable DL-200 history/disclosure owner.
//!
//! Existing Session nodes remain the physical local journal. This module seals an
//! exact settled Agent boundary into structurally shared immutable HistorySpan /
//! HistoryBase rows and a content-addressed disclosure-manifest DAG.

use crate::{
    agent_owner, branch_in, context, db, id, json, now, principal_is_session_owner, resources,
    session_in, stable_user_principal, user_for_principal, Error, PrincipalId, Result,
    SessionBinding, SessionStore,
};
use aw_tools::execution_ref::{ExecutionRef, ExecutionSegment};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const MANIFEST_FANOUT: usize = 64;
const MAX_MANIFEST_INPUTS: usize = 16_384;
const MAX_MANIFEST_WALK_NODES: usize = 16_384;
const MAX_FORK_ACL_ENTRIES: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoryBaseRef {
    pub(crate) id: String,
    pub(crate) content_digest: String,
    pub(crate) manifest_digest: String,
    pub(crate) total_turns: u64,
    pub(crate) next_turn: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHistoryForkAccess {
    Private,
    Inherit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnFactProvenance {
    pub original_agent: String,
    pub original_turn: u64,
    pub actor: PrincipalId,
    pub created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DisclosureAuthorization {
    Allow { audience: Vec<(String, String)> },
    Deny,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedHistorySource {
    session_id: String,
    branch_id: String,
    agent_ref: String,
    current_grant_id: Option<String>,
    base: HistoryBaseRef,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ManifestEntry {
    kind: &'static str,
    reference: String,
}

fn hash_parts(domain: &[u8], parts: impl IntoIterator<Item = Vec<u8>>) -> String {
    let mut hash = Sha256::new();
    hash.update(domain);
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    format!("{:x}", hash.finalize())
}

fn hash_strings<'a>(domain: &[u8], parts: impl IntoIterator<Item = &'a str>) -> String {
    hash_parts(
        domain,
        parts.into_iter().map(|part| part.as_bytes().to_vec()),
    )
}

fn raw_node_digest(connection: &Connection, session: &str, node: &str) -> Result<String> {
    let row: (Option<String>, String, String, String, i64) = db(connection.query_row(
        "SELECT parent_id,kind,actor,payload,created_at
         FROM nodes WHERE session_id=?1 AND id=?2",
        params![session, node],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    ))?;
    Ok(hash_parts(
        b"aw-history-fact-v1\0",
        [
            session.as_bytes().to_vec(),
            node.as_bytes().to_vec(),
            row.0.unwrap_or_default().into_bytes(),
            row.1.into_bytes(),
            row.2.into_bytes(),
            row.3.into_bytes(),
            row.4.to_be_bytes().to_vec(),
        ],
    ))
}

fn authority_root(
    connection: &Connection,
    kind: &str,
    stable_key: &str,
    evidence_json: &str,
) -> Result<String> {
    let digest = hash_strings(
        b"aw-disclosure-authority-root-v1\0",
        [kind, stable_key, evidence_json],
    );
    db(connection.execute(
        "INSERT OR IGNORE INTO disclosure_authority_roots(
             root_digest,kind,stable_key,evidence_json,created_at
         ) VALUES(?1,?2,?3,?4,?5)",
        params![digest, kind, stable_key, evidence_json, now()],
    ))?;
    let actual: (String, String, String) = db(connection.query_row(
        "SELECT kind,stable_key,evidence_json
         FROM disclosure_authority_roots WHERE root_digest=?1",
        [&digest],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ))?;
    if actual
        != (
            kind.to_owned(),
            stable_key.to_owned(),
            evidence_json.to_owned(),
        )
    {
        return Err(Error::Storage("disclosure authority-root digest collision"));
    }
    Ok(digest)
}

fn insert_manifest_leaf(connection: &Connection, entries: &[ManifestEntry]) -> Result<String> {
    if entries.len() > MANIFEST_FANOUT {
        return Err(Error::Limit("disclosure manifest leaf fanout exceeded"));
    }
    let mut parts = Vec::with_capacity(entries.len() * 2 + 1);
    parts.push((entries.len() as u64).to_be_bytes().to_vec());
    for entry in entries {
        parts.push(entry.kind.as_bytes().to_vec());
        parts.push(entry.reference.as_bytes().to_vec());
    }
    let digest = hash_parts(b"aw-disclosure-manifest-leaf-v1\0", parts);
    db(connection.execute(
        "INSERT OR IGNORE INTO disclosure_manifest_nodes(
             node_digest,node_kind,item_count,created_at
         ) VALUES(?1,'leaf',?2,?3)",
        params![digest, entries.len() as u64, now()],
    ))?;
    for (ordinal, entry) in entries.iter().enumerate() {
        db(connection.execute(
            "INSERT OR IGNORE INTO disclosure_manifest_entries(
                 node_digest,ordinal,entry_kind,entry_ref
             ) VALUES(?1,?2,?3,?4)",
            params![digest, ordinal as u64, entry.kind, entry.reference],
        ))?;
    }
    let actual: (String, u64) = db(connection.query_row(
        "SELECT node_kind,item_count FROM disclosure_manifest_nodes WHERE node_digest=?1",
        [&digest],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    if actual != ("leaf".to_owned(), entries.len() as u64) {
        return Err(Error::Storage("disclosure manifest leaf digest collision"));
    }
    Ok(digest)
}

fn insert_manifest_branch(connection: &Connection, children: &[(String, u64)]) -> Result<String> {
    if children.is_empty() || children.len() > MANIFEST_FANOUT {
        return Err(Error::Limit("disclosure manifest branch fanout exceeded"));
    }
    if children.len() == 1 {
        return Ok(children[0].0.clone());
    }
    let item_count = children
        .iter()
        .try_fold(0u64, |total, (_, count)| total.checked_add(*count))
        .ok_or(Error::Limit("disclosure manifest item count exhausted"))?;
    let mut parts = Vec::with_capacity(children.len() * 2 + 1);
    parts.push((children.len() as u64).to_be_bytes().to_vec());
    for (digest, count) in children {
        parts.push(digest.as_bytes().to_vec());
        parts.push(count.to_be_bytes().to_vec());
    }
    let digest = hash_parts(b"aw-disclosure-manifest-branch-v1\0", parts);
    db(connection.execute(
        "INSERT OR IGNORE INTO disclosure_manifest_nodes(
             node_digest,node_kind,item_count,created_at
         ) VALUES(?1,'branch',?2,?3)",
        params![digest, item_count, now()],
    ))?;
    for (ordinal, (child, _)) in children.iter().enumerate() {
        db(connection.execute(
            "INSERT OR IGNORE INTO disclosure_manifest_children(
                 node_digest,ordinal,child_digest
             ) VALUES(?1,?2,?3)",
            params![digest, ordinal as u64, child],
        ))?;
    }
    let actual: (String, u64) = db(connection.query_row(
        "SELECT node_kind,item_count FROM disclosure_manifest_nodes WHERE node_digest=?1",
        [&digest],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    if actual != ("branch".to_owned(), item_count) {
        return Err(Error::Storage(
            "disclosure manifest branch digest collision",
        ));
    }
    Ok(digest)
}

fn manifest_count(connection: &Connection, digest: &str) -> Result<u64> {
    db(connection.query_row(
        "SELECT item_count FROM disclosure_manifest_nodes WHERE node_digest=?1",
        [digest],
        |row| row.get(0),
    ))
}

fn manifest_from_entries(
    connection: &Connection,
    entries: &BTreeSet<ManifestEntry>,
) -> Result<String> {
    if entries.len() > MAX_MANIFEST_INPUTS {
        return Err(Error::Limit("disclosure manifest input bound exceeded"));
    }
    if entries.is_empty() {
        return insert_manifest_leaf(connection, &[]);
    }
    let entries = entries.iter().cloned().collect::<Vec<_>>();
    let mut level = Vec::<(String, u64)>::new();
    for chunk in entries.chunks(MANIFEST_FANOUT) {
        level.push((insert_manifest_leaf(connection, chunk)?, chunk.len() as u64));
    }
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(MANIFEST_FANOUT));
        for chunk in level.chunks(MANIFEST_FANOUT) {
            let digest = insert_manifest_branch(connection, chunk)?;
            let count = chunk.iter().map(|(_, count)| *count).sum();
            next.push((digest, count));
        }
        level = next;
    }
    Ok(level.pop().expect("non-empty manifest level").0)
}

fn union_manifests(connection: &Connection, left: &str, right: &str) -> Result<String> {
    if left == right {
        return Ok(left.to_owned());
    }
    let left_count = manifest_count(connection, left)?;
    let right_count = manifest_count(connection, right)?;
    if left_count == 0 {
        return Ok(right.to_owned());
    }
    if right_count == 0 {
        return Ok(left.to_owned());
    }
    insert_manifest_branch(
        connection,
        &[
            (left.to_owned(), left_count),
            (right.to_owned(), right_count),
        ],
    )
}

fn dependency_manifest(connection: &Connection, head: &str) -> Result<String> {
    let mut entries = BTreeSet::new();

    {
        let mut query = db(connection.prepare(
            "SELECT source_session,source_node
             FROM node_dependencies WHERE node_id=?1
             ORDER BY source_session,source_node LIMIT ?2",
        ))?;
        let rows = db(
            query.query_map(params![head, (MAX_MANIFEST_INPUTS + 1) as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            }),
        )?;
        let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        if rows.len() > MAX_MANIFEST_INPUTS {
            return Err(Error::Limit("history dependency manifest bound exceeded"));
        }
        for (source_session, source_node) in rows {
            let fact_digest = raw_node_digest(connection, &source_session, &source_node)?;
            entries.insert(ManifestEntry {
                kind: "fact",
                reference: format!("legacy-node:{source_session}:{source_node}:{fact_digest}"),
            });
            let evidence = json(&serde_json::json!({"source_session":source_session}))?;
            let root = authority_root(connection, "legacy_session", &source_session, &evidence)?;
            entries.insert(ManifestEntry {
                kind: "authority_root",
                reference: root,
            });
        }
    }

    {
        let mut query = db(connection.prepare(
            "SELECT materialization_id,fact_sha256,authorization_policy_id,
                    authorization_policy_revision
             FROM node_materialized_facts WHERE node_id=?1
             ORDER BY materialization_id LIMIT ?2",
        ))?;
        let rows = db(
            query.query_map(params![head, (MAX_MANIFEST_INPUTS + 1) as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u64>(3)?,
                ))
            }),
        )?;
        let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        if rows.len() > MAX_MANIFEST_INPUTS {
            return Err(Error::Limit(
                "history materialized-fact manifest bound exceeded",
            ));
        }
        for (materialization, fact_sha256, policy, revision) in rows {
            entries.insert(ManifestEntry {
                kind: "fact",
                reference: format!("materialized:{materialization}:{fact_sha256}"),
            });
            let stable_key = format!("{policy}@{revision}");
            let evidence = json(&serde_json::json!({
                "authorization_policy_id":policy,
                "authorization_policy_revision":revision
            }))?;
            let root = authority_root(connection, "materialized_policy", &stable_key, &evidence)?;
            entries.insert(ManifestEntry {
                kind: "authority_root",
                reference: root,
            });
        }
    }

    let mut manifest = manifest_from_entries(connection, &entries)?;
    let mut query = db(connection.prepare(
        "WITH RECURSIVE ancestry(id,parent_id) AS (
             SELECT id,parent_id FROM nodes WHERE id=?1
             UNION ALL
             SELECT n.id,n.parent_id FROM nodes n
             JOIN ancestry a ON a.parent_id=n.id
         )
         SELECT DISTINCT i.disclosure_manifest
         FROM ancestry a
         JOIN runs r ON r.prompt_node=a.id
         JOIN agent_event_inputs i ON i.delivery_id=r.event_input_delivery_id
         WHERE i.disclosure_manifest IS NOT NULL
         ORDER BY i.disclosure_manifest
         LIMIT ?2",
    ))?;
    let rows = db(
        query.query_map(params![head, (MAX_MANIFEST_INPUTS + 1) as i64], |row| {
            row.get::<_, String>(0)
        }),
    )?;
    let inherited = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if inherited.len() > MAX_MANIFEST_INPUTS {
        return Err(Error::Limit(
            "EventInput disclosure manifest bound exceeded",
        ));
    }
    for inherited in inherited {
        manifest = union_manifests(connection, &manifest, &inherited)?;
    }

    let mut query = db(connection.prepare(
        "WITH RECURSIVE ancestry(id,parent_id) AS (
             SELECT id,parent_id FROM nodes WHERE id=?1
             UNION ALL
             SELECT n.id,n.parent_id FROM nodes n
             JOIN ancestry a ON a.parent_id=n.id
         )
         SELECT DISTINCT c.manifest_digest
         FROM ancestry a
         JOIN runs r ON r.prompt_node=a.id
         JOIN legacy_event_input_compat c
           ON c.session_id=r.session_id
          AND c.branch_id=r.branch_id
          AND c.turn_id=r.turn_id
         ORDER BY c.manifest_digest
         LIMIT ?2",
    ))?;
    let rows = db(
        query.query_map(params![head, (MAX_MANIFEST_INPUTS + 1) as i64], |row| {
            row.get::<_, String>(0)
        }),
    )?;
    let legacy = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if legacy.len() > MAX_MANIFEST_INPUTS {
        return Err(Error::Limit(
            "legacy Event Input disclosure manifest bound exceeded",
        ));
    }
    for legacy in legacy {
        manifest = union_manifests(connection, &manifest, &legacy)?;
    }
    Ok(manifest)
}

pub(crate) fn legacy_protected_input_manifest(
    connection: &Connection,
    input_request_id: &str,
    source_agent_ref: Option<&str>,
) -> Result<String> {
    let evidence = json(&serde_json::json!({"input_request_id":input_request_id}))?;
    let root = authority_root(connection, "legacy_protected", input_request_id, &evidence)?;
    let protected = manifest_from_entries(
        connection,
        &BTreeSet::from([ManifestEntry {
            kind: "authority_root",
            reference: root,
        }]),
    )?;
    match source_agent_ref {
        Some(source_agent_ref) => {
            let source = event_input_manifest(connection, source_agent_ref, None)?;
            union_manifests(connection, &protected, &source)
        }
        None => Ok(protected),
    }
}

pub(crate) fn event_input_manifest(
    connection: &Connection,
    source_agent_ref: &str,
    event_manifest: Option<&str>,
) -> Result<String> {
    let evidence = json(&serde_json::json!({"source_agent_ref":source_agent_ref}))?;
    let root = authority_root(connection, "agent_source", source_agent_ref, &evidence)?;
    let source_manifest = manifest_from_entries(
        connection,
        &BTreeSet::from([ManifestEntry {
            kind: "authority_root",
            reference: root,
        }]),
    )?;
    match event_manifest {
        Some(manifest) => union_manifests(connection, &source_manifest, manifest),
        None => Ok(source_manifest),
    }
}

fn event_publication_manifest_at(
    connection: &Connection,
    session: &str,
    branch: &str,
    head: &str,
) -> Result<String> {
    let agent = agent_owner::AgentKey::new(session, branch);
    let source_agent_ref = agent_owner::canonical_ref(connection, agent)?;
    let local = dependency_manifest(connection, head)?;
    let mut manifest = event_input_manifest(connection, &source_agent_ref, Some(&local))?;
    let inherited: Option<String> = db(connection.query_row(
        "SELECT b.manifest_digest
         FROM agent_state_current a
         LEFT JOIN history_bases b ON b.base_id=a.history_base_id
         WHERE a.session_id=?1 AND a.branch_id=?2",
        params![session, branch],
        |row| row.get(0),
    ))?;
    if let Some(inherited) = inherited {
        manifest = union_manifests(connection, &manifest, &inherited)?;
    }
    Ok(manifest)
}

pub(crate) fn event_publication_manifest_for_agent(
    connection: &Connection,
    session: &str,
    branch: &str,
) -> Result<String> {
    let current = branch_in(connection, session, branch)?;
    event_publication_manifest_at(connection, session, branch, &current.head)
}

pub(crate) fn event_publication_manifest_for_turn(
    connection: &Connection,
    session: &str,
    branch: &str,
    turn: u64,
) -> Result<String> {
    if turn == 0 {
        return Err(Error::Storage("invalid Event publication Turn"));
    }
    let prompt_node: String = db(connection.query_row(
        "SELECT prompt_node FROM runs
         WHERE session_id=?1 AND branch_id=?2 AND turn_id=?3
         ORDER BY started_at DESC LIMIT 1",
        params![session, branch, turn],
        |row| row.get(0),
    ))?;
    event_publication_manifest_at(connection, session, branch, &prompt_node)
}

fn base_row(connection: &Connection, base_id: &str) -> Result<HistoryBaseRef> {
    db(connection.query_row(
        "SELECT base_id,history_content_digest,manifest_digest,total_turns,next_turn
         FROM history_bases WHERE base_id=?1",
        [base_id],
        |row| {
            Ok(HistoryBaseRef {
                id: row.get(0)?,
                content_digest: row.get(1)?,
                manifest_digest: row.get(2)?,
                total_turns: row.get(3)?,
                next_turn: row.get(4)?,
            })
        },
    ))
}

fn inherited_parent_for_seal(
    connection: &Connection,
    current_base: Option<&str>,
    session_id: &str,
    branch_id: &str,
) -> Result<Option<String>> {
    let mut cursor = current_base.map(str::to_owned);
    while let Some(base_id) = cursor {
        let (parent, span_session, span_branch): (Option<String>, String, String) = db(connection
            .query_row(
                "SELECT b.parent_base_id,s.backing_session_id,s.backing_branch_id
                 FROM history_bases b
                 JOIN history_spans s ON s.span_id=b.local_span_id
                 WHERE b.base_id=?1",
                [&base_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ))?;
        if span_session == session_id && span_branch == branch_id {
            cursor = parent;
        } else {
            return Ok(Some(base_id));
        }
    }
    Ok(None)
}

fn inherited_span_chain(connection: &Connection, base_id: &str) -> Result<Vec<String>> {
    let mut chain = Vec::new();
    let mut cursor = Some(base_id.to_owned());
    while let Some(base) = cursor {
        if chain.len() >= crate::MAX_SESSION_NODES {
            return Err(Error::Limit(
                "HistoryBase ancestry exceeds projection bound",
            ));
        }
        let (parent, span): (Option<String>, String) = db(connection.query_row(
            "SELECT parent_base_id,local_span_id FROM history_bases WHERE base_id=?1",
            [&base],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ))?;
        chain.push(span);
        cursor = parent;
    }
    chain.reverse();
    Ok(chain)
}

fn verified_span_nodes(connection: &Connection, span: &str) -> Result<(String, Vec<crate::Node>)> {
    let (source_agent, backing_session): (String, String) = db(connection.query_row(
        "SELECT source_agent_ref,backing_session_id FROM history_spans WHERE span_id=?1",
        [span],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    let mut query = db(connection.prepare(
        "SELECT node_id,node_digest FROM history_span_nodes
         WHERE span_id=?1 ORDER BY ordinal LIMIT ?2",
    ))?;
    let rows = db(query.query_map(
        params![span, (crate::MAX_SESSION_NODES + 1) as i64],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    ))?;
    let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if rows.len() > crate::MAX_SESSION_NODES {
        return Err(Error::Limit("HistorySpan exceeds projection bound"));
    }
    let mut nodes = Vec::with_capacity(rows.len());
    for (node_id, expected_digest) in rows {
        let actual_digest = raw_node_digest(connection, &backing_session, &node_id)?;
        if actual_digest != expected_digest {
            return Err(Error::Storage("HistorySpan backing node digest changed"));
        }
        nodes.push(crate::node_in(connection, &backing_session, &node_id)?);
    }
    Ok((source_agent, nodes))
}

fn inherited_projected_nodes(connection: &Connection, base_id: &str) -> Result<Vec<crate::Node>> {
    let mut nodes = Vec::new();
    for span in inherited_span_chain(connection, base_id)? {
        let (_, span_nodes) = verified_span_nodes(connection, &span)?;
        if nodes.len().saturating_add(span_nodes.len()) > crate::MAX_SESSION_NODES {
            return Err(Error::Limit("inherited history exceeds projection bound"));
        }
        nodes.extend(span_nodes);
    }
    Ok(nodes)
}

pub(crate) fn effective_history(
    connection: &Connection,
    session: &crate::SessionInfo,
    branch: &crate::BranchInfo,
) -> Result<aw_agent_core::HistoryState> {
    let base_id: Option<String> = db(connection.query_row(
        "SELECT history_base_id FROM agent_state_current
         WHERE session_id=?1 AND branch_id=?2",
        params![session.id, branch.id],
        |row| row.get(0),
    ))?;
    let Some(base_id) = base_id else {
        return context::projected_history(connection, session, branch);
    };
    let mut nodes = inherited_projected_nodes(connection, &base_id)?;
    let local = context::projected(connection, &session.id, &branch.head)?;
    if nodes.len().saturating_add(local.len()) > crate::MAX_SESSION_NODES {
        return Err(Error::Limit(
            "effective Agent history exceeds projection bound",
        ));
    }
    nodes.extend(local);
    context::history_from_projected_nodes(connection, session, nodes, session.next_turn)
}

fn collect_turn_provenance(
    out: &mut BTreeMap<u64, TurnFactProvenance>,
    wanted: &BTreeSet<u64>,
    original_agent: &str,
    nodes: impl IntoIterator<Item = crate::Node>,
) -> Result<()> {
    for node in nodes {
        if node.kind != "turn" {
            continue;
        }
        let Some(turn) = node.payload.get("id").and_then(serde_json::Value::as_u64) else {
            return Err(Error::Storage("committed Turn node is missing its Turn ID"));
        };
        if !wanted.contains(&turn) {
            continue;
        }
        let provenance = TurnFactProvenance {
            original_agent: original_agent.to_owned(),
            original_turn: turn,
            actor: node.actor,
            created_at: node.created_at,
        };
        if let Some(existing) = out.get(&turn) {
            if existing != &provenance {
                return Err(Error::Storage(
                    "committed Turn has conflicting immutable provenance",
                ));
            }
        } else {
            out.insert(turn, provenance);
        }
    }
    Ok(())
}

impl SessionStore {
    /// Batch immutable attribution for already-committed Turns. This is a
    /// projection over the v40 HistoryBase/span chain plus the current local
    /// branch; it owns no authority and performs no lazy sealing/write.
    pub fn agent_turn_provenance(
        &self,
        actor: &PrincipalId,
        session_id: &str,
        branch_id: &str,
        turn_ids: &[u64],
    ) -> Result<BTreeMap<u64, TurnFactProvenance>> {
        if turn_ids.len() > crate::MAX_SESSION_NODES {
            return Err(Error::Limit(
                "Turn provenance request exceeds history bound",
            ));
        }
        let wanted = turn_ids.iter().copied().collect::<BTreeSet<_>>();
        if wanted.iter().any(|turn| *turn == 0) {
            return Err(Error::Denied("Turn provenance requires positive Turn IDs"));
        }
        if wanted.is_empty() {
            return Ok(BTreeMap::new());
        }

        let connection = self.connection()?;
        let branch = branch_in(&connection, session_id, branch_id)?;
        let agent = agent_owner::AgentKey::new(session_id, &branch.id);
        agent_owner::authorize_permission(
            &connection,
            actor,
            agent,
            agent_owner::AgentPermission::View,
        )?;
        let current_agent = agent_owner::canonical_ref(&connection, agent)?;
        let base_id: Option<String> = db(connection.query_row(
            "SELECT history_base_id FROM agent_state_current
             WHERE session_id=?1 AND branch_id=?2",
            params![session_id, branch_id],
            |row| row.get(0),
        ))?;

        let mut out = BTreeMap::new();
        if let Some(base_id) = base_id {
            for span in inherited_span_chain(&connection, &base_id)? {
                let (source_agent, nodes) = verified_span_nodes(&connection, &span)?;
                collect_turn_provenance(&mut out, &wanted, &source_agent, nodes)?;
            }
        }
        let local = context::projected(&connection, session_id, &branch.head)?;
        collect_turn_provenance(&mut out, &wanted, &current_agent, local)?;

        if out.len() != wanted.len() {
            return Err(Error::Storage(
                "committed Agent history is missing immutable Turn provenance",
            ));
        }
        Ok(out)
    }
}

fn projected_node_digests(
    connection: &Connection,
    session_id: &str,
    head: &str,
) -> Result<Vec<(String, String)>> {
    context::projected(connection, session_id, head)?
        .into_iter()
        .map(|node| {
            let digest = raw_node_digest(connection, session_id, &node.id)?;
            Ok((node.id, digest))
        })
        .collect()
}

fn seal_boundary_in(
    connection: &Connection,
    session_id: &str,
    branch_id: &str,
    head: &str,
    next_turn: u64,
) -> Result<HistoryBaseRef> {
    let branch = branch_in(connection, session_id, branch_id)?;
    if branch.id != branch_id {
        return Err(Error::Denied(
            "history sealing requires exact branch identity",
        ));
    }
    if next_turn == 0 {
        return Err(Error::Storage(
            "history sealing next-turn invariant violated",
        ));
    }
    let head_kind: String = db(connection.query_row(
        "SELECT kind FROM nodes WHERE session_id=?1 AND id=?2",
        params![session_id, head],
        |row| row.get(0),
    ))?;
    if head_kind == "prompt" {
        return Err(Error::Conflict(
            "cannot seal history at an unresolved prompt boundary",
        ));
    }

    let (current_base, source_agent): (Option<String>, String) = db(connection.query_row(
        "SELECT history_base_id,
                '/d:' || domain_handle || '/i:' || identity_handle ||
                '/p:' || project_handle || '/a:' || agent_handle
         FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
        params![session_id, branch_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    let parent_base =
        inherited_parent_for_seal(connection, current_base.as_deref(), session_id, branch_id)?;

    let projected = projected_node_digests(connection, session_id, head)?;
    if projected.is_empty() {
        if let Some(parent) = parent_base.as_deref() {
            return base_row(connection, parent);
        }
    }

    let root_node: String = db(connection.query_row(
        "SELECT root_node FROM sessions WHERE id=?1",
        [session_id],
        |row| row.get(0),
    ))?;
    let local_root = projected
        .first()
        .map(|(node, _)| node.as_str())
        .unwrap_or(root_node.as_str());
    let local_manifest = dependency_manifest(connection, head)?;
    let manifest_digest = if let Some(parent) = parent_base.as_deref() {
        let inherited = base_row(connection, parent)?;
        union_manifests(connection, &inherited.manifest_digest, &local_manifest)?
    } else {
        local_manifest
    };

    let content_digest = hash_parts(
        b"aw-history-span-content-v1\0",
        projected
            .iter()
            .flat_map(|(node, digest)| [node.as_bytes().to_vec(), digest.as_bytes().to_vec()]),
    );
    let provenance_digest = hash_strings(
        b"aw-history-span-provenance-v1\0",
        [
            &source_agent,
            session_id,
            branch_id,
            local_root,
            head,
            &content_digest,
            &manifest_digest,
        ],
    );
    let span_seal_key = hash_strings(
        b"aw-history-span-seal-v1\0",
        [
            &source_agent,
            session_id,
            branch_id,
            local_root,
            head,
            &content_digest,
            &provenance_digest,
            &manifest_digest,
        ],
    );
    let span_id = format!("hsp_{span_seal_key}");
    let total_turns = next_turn
        .checked_sub(1)
        .ok_or(Error::Storage("Session next-turn invariant violated"))?;
    db(connection.execute(
        "INSERT OR IGNORE INTO history_spans(
             span_id,seal_key,source_agent_ref,backing_session_id,backing_branch_id,
             local_root_node,exact_head_node,content_digest,provenance_digest,
             manifest_digest,total_turns,next_turn,created_at
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        params![
            span_id,
            span_seal_key,
            source_agent,
            session_id,
            branch_id,
            local_root,
            head,
            content_digest,
            provenance_digest,
            manifest_digest,
            total_turns,
            next_turn,
            now()
        ],
    ))?;
    for (ordinal, (node, digest)) in projected.iter().enumerate() {
        db(connection.execute(
            "INSERT OR IGNORE INTO history_span_nodes(
                 span_id,ordinal,node_id,node_digest
             ) VALUES(?1,?2,?3,?4)",
            params![span_id, ordinal as u64, node, digest],
        ))?;
    }

    let parent_content = parent_base
        .as_deref()
        .map(|parent| base_row(connection, parent).map(|base| base.content_digest))
        .transpose()?
        .unwrap_or_default();
    let base_content_digest = hash_strings(
        b"aw-history-base-content-v1\0",
        [
            parent_content.as_str(),
            content_digest.as_str(),
            provenance_digest.as_str(),
            manifest_digest.as_str(),
        ],
    );
    let base_seal_key = hash_strings(
        b"aw-history-base-seal-v1\0",
        [
            parent_base.as_deref().unwrap_or(""),
            &span_id,
            &base_content_digest,
            &manifest_digest,
            &total_turns.to_string(),
            &next_turn.to_string(),
        ],
    );
    let base_id = format!("hb_{base_seal_key}");
    db(connection.execute(
        "INSERT OR IGNORE INTO history_bases(
             base_id,seal_key,parent_base_id,local_span_id,total_turns,next_turn,
             history_content_digest,manifest_digest,created_at
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            base_id,
            base_seal_key,
            parent_base,
            span_id,
            total_turns,
            next_turn,
            base_content_digest,
            manifest_digest,
            now()
        ],
    ))?;
    base_row(connection, &base_id)
}

pub(crate) fn seal_in(
    connection: &Connection,
    session_id: &str,
    branch_id: &str,
) -> Result<HistoryBaseRef> {
    let session = session_in(connection, session_id)?;
    let branch = branch_in(connection, session_id, branch_id)?;
    if branch.active_run.is_some() {
        return Err(Error::Conflict(
            "cannot seal current history while the Agent has active work",
        ));
    }
    seal_boundary_in(
        connection,
        session_id,
        branch_id,
        &branch.head,
        session.next_turn,
    )
}

pub(crate) fn seal_latest_committed_in(
    connection: &Connection,
    session_id: &str,
    branch_id: &str,
) -> Result<HistoryBaseRef> {
    let session = session_in(connection, session_id)?;
    let branch = branch_in(connection, session_id, branch_id)?;
    let (head, next_turn) = if let Some(run) = branch.active_run.as_deref() {
        db(connection.query_row(
            "SELECT produced_from_head,turn_id FROM runs
             WHERE id=?1 AND session_id=?2 AND branch_id=?3 AND status='running'",
            params![run, session_id, branch_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
        ))?
    } else {
        (branch.head, session.next_turn)
    };
    seal_boundary_in(connection, session_id, branch_id, &head, next_turn)
}

fn canonical_turn_id(handle: &str) -> Result<u64> {
    handle
        .parse::<u64>()
        .ok()
        .filter(|value| *value != 0 && value.to_string() == handle)
        .ok_or(Error::Denied("Turn handle is not canonical decimal"))
}

fn inherited_turn_boundary(
    connection: &Connection,
    base_id: &str,
    turn_id: u64,
) -> Result<Option<HistoryBaseRef>> {
    let mut cursor = Some(base_id.to_owned());
    let mut depth = 0usize;
    while let Some(base) = cursor {
        depth += 1;
        if depth > crate::MAX_SESSION_NODES {
            return Err(Error::Limit(
                "HistoryBase ancestry exceeds turn lookup bound",
            ));
        }
        let (parent, span): (Option<String>, String) = db(connection.query_row(
            "SELECT parent_base_id,local_span_id FROM history_bases WHERE base_id=?1",
            [&base],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ))?;
        let (backing_session, backing_branch): (String, String) = db(connection.query_row(
            "SELECT backing_session_id,backing_branch_id FROM history_spans WHERE span_id=?1",
            [&span],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ))?;
        let mut query = db(connection.prepare(
            "SELECT h.node_id
             FROM history_span_nodes h
             JOIN nodes n ON n.id=h.node_id AND n.session_id=?2
             WHERE h.span_id=?1 AND n.kind='turn'
               AND json_extract(n.payload,'$.id')=?3
             ORDER BY h.ordinal LIMIT 2",
        ))?;
        let rows = db(
            query.query_map(params![span, backing_session, turn_id], |row| {
                row.get::<_, String>(0)
            }),
        )?;
        let matches = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        match matches.as_slice() {
            [node] => {
                let base = seal_boundary_in(
                    connection,
                    &backing_session,
                    &backing_branch,
                    node,
                    turn_id
                        .checked_add(1)
                        .ok_or(Error::Limit("Turn ordinal exhausted"))?,
                )?;
                return Ok(Some(base));
            }
            [] => {}
            _ => {
                return Err(Error::Storage(
                    "HistoryBase contains duplicate Turn ordinal",
                ))
            }
        }
        cursor = parent;
    }
    Ok(None)
}

fn seal_settled_turn_in(
    connection: &Connection,
    session_id: &str,
    branch_id: &str,
    turn_id: u64,
) -> Result<HistoryBaseRef> {
    let mut query = db(connection.prepare(
        "SELECT final_node FROM runs
         WHERE session_id=?1 AND branch_id=?2 AND turn_id=?3
           AND status<>'running' AND attach_status='attached' AND final_node IS NOT NULL
         ORDER BY ended_at DESC,id DESC LIMIT 2",
    ))?;
    let rows = db(
        query.query_map(params![session_id, branch_id, turn_id], |row| {
            row.get::<_, String>(0)
        }),
    )?;
    let local = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    match local.as_slice() {
        [node] => {
            return seal_boundary_in(
                connection,
                session_id,
                branch_id,
                node,
                turn_id
                    .checked_add(1)
                    .ok_or(Error::Limit("Turn ordinal exhausted"))?,
            )
        }
        [] => {}
        _ => {
            return Err(Error::Storage(
                "multiple attached results exist for one Turn",
            ))
        }
    }

    let current_base: Option<String> = db(connection.query_row(
        "SELECT history_base_id FROM agent_state_current
         WHERE session_id=?1 AND branch_id=?2",
        params![session_id, branch_id],
        |row| row.get(0),
    ))?;
    if let Some(base) = current_base {
        if let Some(boundary) = inherited_turn_boundary(connection, &base, turn_id)? {
            return Ok(boundary);
        }
    }
    Err(Error::Denied("Turn is not a settled history boundary"))
}

fn resolve_canonical_history_source_in(
    connection: &Connection,
    actor: &PrincipalId,
    reference: &str,
) -> Result<ResolvedHistorySource> {
    let parsed = ExecutionRef::parse(reference)
        .map_err(|_| Error::Denied("invalid exact-history source ref"))?;
    if !parsed.is_canonical() {
        return Err(Error::Denied("history source ref must be canonical"));
    }
    let resolved = parsed
        .resolve(None, None)
        .map_err(|_| Error::Denied("invalid canonical history source ref"))?;
    let agent_ref = resolved.agent_ref().canonical();
    let (session_id, branch_id) = agent_owner::key_for_ref(connection, &agent_ref)?;
    let source_agent = agent_owner::AgentKey::new(&session_id, &branch_id);
    agent_owner::authorize_permission(
        connection,
        actor,
        source_agent,
        agent_owner::AgentPermission::Derive,
    )?;
    let base = match resolved.suffix() {
        [] => seal_latest_committed_in(connection, &session_id, &branch_id)?,
        [ExecutionSegment::Turn(handle)] => {
            let turn = canonical_turn_id(handle.as_str())?;
            seal_settled_turn_in(connection, &session_id, &branch_id, turn)?
        }
        _ => {
            return Err(Error::Denied(
                "history source must be an Agent ref or settled Turn ref",
            ))
        }
    };
    resolved_history_source_in(connection, source_agent, agent_ref, base)
}

fn resolved_history_source_in(
    connection: &Connection,
    source: agent_owner::AgentKey<'_>,
    agent_ref: String,
    base: HistoryBaseRef,
) -> Result<ResolvedHistorySource> {
    let current_grant_id: Option<String> = db(connection.query_row(
        "SELECT history_grant_id FROM agent_state_current
         WHERE session_id=?1 AND branch_id=?2",
        params![source.session, source.branch],
        |row| row.get(0),
    ))?;
    Ok(ResolvedHistorySource {
        session_id: source.session.to_owned(),
        branch_id: source.branch.to_owned(),
        agent_ref,
        current_grant_id,
        base,
    })
}

/// Only a retention owner may supply a retained base ID, after verifying
/// that it belongs to the source Agent's authorized immutable resource.
/// Never reseal the Agent's current head for such a history source.
pub(crate) fn resolve_retained_history_source_in(
    connection: &Connection,
    actor: &PrincipalId,
    source: agent_owner::AgentKey<'_>,
    base_id: &str,
) -> Result<ResolvedHistorySource> {
    agent_owner::authorize_permission(
        connection,
        actor,
        source,
        agent_owner::AgentPermission::Derive,
    )?;
    let reference = agent_owner::canonical_ref(connection, source)?;
    let base = base_row(connection, base_id)?;
    resolved_history_source_in(connection, source, reference, base)
}

pub(crate) fn preflight_canonical_history_source_in(
    connection: &Connection,
    actor: &PrincipalId,
    reference: &str,
) -> Result<()> {
    resolve_canonical_history_source_in(connection, actor, reference).map(|_| ())
}

pub(crate) fn authorize_canonical_history_share_in(
    connection: &Connection,
    actor: &PrincipalId,
    reference: &str,
) -> Result<()> {
    let parsed = ExecutionRef::parse(reference)
        .map_err(|_| Error::Denied("invalid exact-history source ref"))?;
    if !parsed.is_canonical() {
        return Err(Error::Denied("history source ref must be canonical"));
    }
    let resolved = parsed
        .resolve(None, None)
        .map_err(|_| Error::Denied("invalid canonical history source ref"))?;
    let agent_ref = resolved.agent_ref().canonical();
    let (session, branch) = agent_owner::key_for_ref(connection, &agent_ref)?;
    agent_owner::authorize_permission(
        connection,
        actor,
        agent_owner::AgentKey::new(&session, &branch),
        agent_owner::AgentPermission::Share,
    )
    .map(|_| ())
}

fn manifest_authority_roots(
    connection: &Connection,
    manifest: &str,
) -> Result<Vec<(String, String, String, String)>> {
    let mut stack = vec![manifest.to_owned()];
    let mut visited = BTreeSet::new();
    let mut roots = BTreeSet::new();
    while let Some(node) = stack.pop() {
        if !visited.insert(node.clone()) {
            continue;
        }
        if visited.len() > MAX_MANIFEST_WALK_NODES {
            return Err(Error::Limit("disclosure manifest walk bound exceeded"));
        }
        let kind: String = db(connection.query_row(
            "SELECT node_kind FROM disclosure_manifest_nodes WHERE node_digest=?1",
            [&node],
            |row| row.get(0),
        ))?;
        match kind.as_str() {
            "leaf" => {
                let mut query = db(connection.prepare(
                    "SELECT entry_ref FROM disclosure_manifest_entries
                     WHERE node_digest=?1 AND entry_kind='authority_root'
                     ORDER BY ordinal LIMIT ?2",
                ))?;
                let rows = db(query
                    .query_map(params![node, (MANIFEST_FANOUT + 1) as i64], |row| {
                        row.get::<_, String>(0)
                    }))?;
                let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
                if rows.len() > MANIFEST_FANOUT {
                    return Err(Error::Storage("disclosure manifest leaf fanout corrupted"));
                }
                roots.extend(rows);
            }
            "branch" => {
                let mut query = db(connection.prepare(
                    "SELECT child_digest FROM disclosure_manifest_children
                     WHERE node_digest=?1 ORDER BY ordinal LIMIT ?2",
                ))?;
                let rows = db(query
                    .query_map(params![node, (MANIFEST_FANOUT + 1) as i64], |row| {
                        row.get::<_, String>(0)
                    }))?;
                let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
                if rows.len() > MANIFEST_FANOUT {
                    return Err(Error::Storage(
                        "disclosure manifest branch fanout corrupted",
                    ));
                }
                stack.extend(rows.into_iter().rev());
            }
            _ => return Err(Error::Storage("unknown disclosure manifest node kind")),
        }
    }
    roots
        .into_iter()
        .map(|root| {
            db(connection.query_row(
                "SELECT kind,stable_key,evidence_json
                 FROM disclosure_authority_roots WHERE root_digest=?1",
                [&root],
                |row| Ok((root.clone(), row.get(0)?, row.get(1)?, row.get(2)?)),
            ))
        })
        .collect()
}

fn grant_covers(
    connection: &Connection,
    grant: &str,
    principal_kind: &str,
    principal_handle: &str,
) -> Result<bool> {
    db(connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM history_grant_principals
             WHERE grant_id=?1 AND principal_kind=?2 AND principal_handle=?3
         )",
        params![grant, principal_kind, principal_handle],
        |row| row.get(0),
    ))
}

fn current_agent_audience(
    connection: &Connection,
    agent: agent_owner::AgentKey<'_>,
) -> Result<BTreeSet<(String, String)>> {
    let mut principals = BTreeSet::new();
    let (owner_kind, owner_handle): (String, String) = db(connection.query_row(
        "SELECT owner_kind,owner_handle FROM agent_state_current
         WHERE session_id=?1 AND branch_id=?2",
        params![agent.session, agent.branch],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    principals.insert((owner_kind, owner_handle));
    let mut query = db(connection.prepare(
        "SELECT principal_kind,principal_handle FROM agent_acl_current
         WHERE session_id=?1 AND branch_id=?2 AND can_view=1
         ORDER BY principal_kind,principal_handle LIMIT ?3",
    ))?;
    let rows = db(query.query_map(
        params![
            agent.session,
            agent.branch,
            (MAX_MANIFEST_INPUTS + 1) as i64
        ],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    ))?;
    let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if rows.len() > MAX_MANIFEST_INPUTS {
        return Err(Error::Limit("Agent audience bound exceeded"));
    }
    principals.extend(rows);
    Ok(principals)
}

fn current_disclosed_principals(
    connection: &Connection,
    agent: agent_owner::AgentKey<'_>,
    current_grant: Option<&str>,
) -> Result<BTreeSet<(String, String)>> {
    if let Some(grant) = current_grant {
        let mut principals = BTreeSet::new();
        let mut query = db(connection.prepare(
            "SELECT principal_kind,principal_handle FROM history_grant_principals
             WHERE grant_id=?1 ORDER BY ordinal LIMIT ?2",
        ))?;
        let rows = db(
            query.query_map(params![grant, (MAX_MANIFEST_INPUTS + 1) as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            }),
        )?;
        let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        if rows.len() > MAX_MANIFEST_INPUTS {
            return Err(Error::Limit("HistoryGrant principal bound exceeded"));
        }
        principals.extend(rows);
        return Ok(principals);
    }
    current_agent_audience(connection, agent)
}

fn validate_agent_source_expansion(
    connection: &Connection,
    actor: &PrincipalId,
    source_agent_ref: &str,
    principal_kind: &str,
    principal_handle: &str,
) -> Result<()> {
    let (source_session, source_branch) = agent_owner::key_for_ref(connection, source_agent_ref)?;
    let source_agent = agent_owner::AgentKey::new(&source_session, &source_branch);
    if current_agent_audience(connection, source_agent)?
        .contains(&(principal_kind.to_owned(), principal_handle.to_owned()))
    {
        return Ok(());
    }
    agent_owner::authorize_permission(
        connection,
        actor,
        source_agent,
        agent_owner::AgentPermission::Share,
    )
    .map(|_| ())
}

fn validate_legacy_session_expansion(
    connection: &Connection,
    actor: &PrincipalId,
    source_session: &str,
    principal_kind: &str,
    principal_handle: &str,
) -> Result<()> {
    if principal_kind == "user" {
        let target = stable_user_principal(principal_handle)?;
        match context::authorize_tree(connection, source_session, &target) {
            Ok(()) => return Ok(()),
            Err(Error::Denied(_)) => {}
            Err(error) => return Err(error),
        }
    }
    let private: bool = db(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM context_private_sessions WHERE session_id=?1)",
        [source_session],
        |row| row.get(0),
    ))?;
    if private || !principal_is_session_owner(connection, source_session, actor)? {
        return Err(Error::Denied(
            "history disclosure dependency does not permit the expanded audience",
        ));
    }
    Ok(())
}

fn validate_manifest_expansion(
    connection: &Connection,
    actor: &PrincipalId,
    manifest: &str,
    principal_kind: &str,
    principal_handle: &str,
) -> Result<()> {
    let roots = manifest_authority_roots(connection, manifest)?;
    let has_source_root = roots.iter().any(|(_, kind, _, _)| kind == "agent_source");
    for (_root, kind, stable_key, _evidence) in roots {
        match kind.as_str() {
            "agent_source" => validate_agent_source_expansion(
                connection,
                actor,
                &stable_key,
                principal_kind,
                principal_handle,
            )?,
            "legacy_session" => validate_legacy_session_expansion(
                connection,
                actor,
                &stable_key,
                principal_kind,
                principal_handle,
            )?,
            "materialized_policy" => {
                return Err(Error::Denied(
                    "materialized external history cannot expand audience without source share authority",
                ));
            }
            "legacy_protected" => {
                if !legacy_protected_principal(
                    connection,
                    &stable_key,
                    principal_kind,
                    principal_handle,
                )? && !has_source_root
                {
                    return Err(Error::Denied(
                        "legacy protected Event input cannot expand without reconstructed source authority",
                    ));
                }
            }
            _ => {
                return Err(Error::Denied(
                    "history disclosure authority root has no reviewed share contract",
                ));
            }
        }
    }
    Ok(())
}

fn legacy_protected_principal(
    connection: &Connection,
    input_request_id: &str,
    principal_kind: &str,
    principal_handle: &str,
) -> Result<bool> {
    if !matches!(principal_kind, "user" | "group") {
        return Ok(false);
    }
    db(connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM legacy_event_input_compat_audience
              WHERE input_request_id=?1
                AND principal_kind=?2
                AND principal_handle=?3
         )",
        params![input_request_id, principal_kind, principal_handle],
        |row| row.get(0),
    ))
}

fn legacy_protected_reader(
    connection: &Connection,
    actor: &PrincipalId,
    input_request_id: &str,
) -> Result<bool> {
    let Some(user) = user_for_principal(connection, actor)? else {
        return Ok(false);
    };
    db(connection.query_row(
        "SELECT EXISTS(
             SELECT 1
             FROM legacy_event_input_compat_audience a
             WHERE a.input_request_id=?1
               AND (
                   (a.principal_kind='user' AND a.principal_handle=?2)
                   OR (
                       a.principal_kind='group'
                       AND EXISTS(
                           SELECT 1 FROM principal_group_members g
                            WHERE g.domain_id=a.domain_handle
                              AND g.group_handle=a.principal_handle
                              AND g.member_handle=?2
                              AND g.state='active'
                       )
                   )
               )
         )",
        params![input_request_id, user],
        |row| row.get(0),
    ))
}

fn materialized_policy_state(
    connection: &Connection,
    stable_key: &str,
) -> Result<Option<(String, String, u64)>> {
    let Some((policy_id, expected_revision)) = stable_key.rsplit_once('@') else {
        return Err(Error::Storage(
            "materialized disclosure authority root has invalid stable key",
        ));
    };
    let expected_revision = expected_revision
        .parse::<u64>()
        .map_err(|_| Error::Storage("materialized disclosure policy revision is invalid"))?;
    db(connection
        .query_row(
            "SELECT status,federation_id,revision
             FROM semantic_materialization_policies WHERE policy_id=?1",
            [policy_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u64>(2)?,
                ))
            },
        )
        .optional())
    .map(|row| row.filter(|(_, _, revision)| *revision == expected_revision))
}

fn classify_disclosure_check(result: Result<()>) -> Result<Option<DisclosureAuthorization>> {
    match result {
        Ok(()) => Ok(None),
        Err(Error::Denied(_)) => Ok(Some(DisclosureAuthorization::Deny)),
        Err(Error::Conflict(_)) => Ok(Some(DisclosureAuthorization::Unknown)),
        Err(error) => Err(error),
    }
}

pub(crate) fn authorize_event_manifest_reader(
    connection: &Connection,
    actor: &PrincipalId,
    manifest: &str,
) -> Result<DisclosureAuthorization> {
    let Some(user) = user_for_principal(connection, actor)? else {
        return Ok(DisclosureAuthorization::Deny);
    };
    let roots = manifest_authority_roots(connection, manifest)?;
    let has_source_root = roots.iter().any(|(_, kind, _, _)| kind == "agent_source");
    for (_root, kind, stable_key, _evidence) in roots {
        match kind.as_str() {
            "agent_source" => match agent_owner::key_for_ref_optional(connection, &stable_key)? {
                Some((session, branch)) => match agent_owner::authorize_permission(
                    connection,
                    actor,
                    agent_owner::AgentKey::new(&session, &branch),
                    agent_owner::AgentPermission::View,
                ) {
                    Ok(_) => {}
                    Err(Error::Denied(_)) => return Ok(DisclosureAuthorization::Deny),
                    Err(Error::Conflict(_)) => return Ok(DisclosureAuthorization::Unknown),
                    Err(error) => return Err(error),
                },
                None => return Ok(DisclosureAuthorization::Unknown),
            },
            "legacy_session" => {
                if let Some(decision) =
                    classify_disclosure_check(validate_legacy_session_expansion(
                        connection,
                        actor,
                        &stable_key,
                        "user",
                        &user,
                    ))?
                {
                    return Ok(decision);
                }
            }
            "legacy_protected" => {
                if !legacy_protected_reader(connection, actor, &stable_key)? && !has_source_root {
                    return Ok(DisclosureAuthorization::Deny);
                }
            }
            "materialized_policy" => {
                let Some((status, federation, _revision)) =
                    materialized_policy_state(connection, &stable_key)?
                else {
                    return Ok(DisclosureAuthorization::Deny);
                };
                if status != "active" {
                    return Ok(DisclosureAuthorization::Deny);
                }
                match crate::durability::semantic_materialization_authority(
                    connection,
                    actor,
                    &federation,
                    false,
                    true,
                ) {
                    Ok(_) => {}
                    Err(Error::Denied(_)) => return Ok(DisclosureAuthorization::Deny),
                    Err(Error::Conflict(_)) => return Ok(DisclosureAuthorization::Unknown),
                    Err(error) => return Err(error),
                }
            }
            _ => return Ok(DisclosureAuthorization::Unknown),
        }
    }
    Ok(DisclosureAuthorization::Allow {
        audience: vec![("user".to_owned(), user)],
    })
}

pub(crate) fn authorize_event_delivery(
    connection: &Connection,
    actor: &PrincipalId,
    source_agent_ref: &str,
    recipient_agent_ref: &str,
    manifest: Option<&str>,
) -> Result<DisclosureAuthorization> {
    let Some((source_session, source_branch)) =
        agent_owner::key_for_ref_optional(connection, source_agent_ref)?
    else {
        return Ok(DisclosureAuthorization::Unknown);
    };
    let Some((recipient_session, recipient_branch)) =
        agent_owner::key_for_ref_optional(connection, recipient_agent_ref)?
    else {
        return Ok(DisclosureAuthorization::Deny);
    };
    let source_agent = agent_owner::AgentKey::new(&source_session, &source_branch);
    let recipient_agent = agent_owner::AgentKey::new(&recipient_session, &recipient_branch);
    let source_audience = current_agent_audience(connection, source_agent)?;
    let recipient_audience = current_agent_audience(connection, recipient_agent)?;

    if recipient_audience
        .iter()
        .any(|principal| !source_audience.contains(principal))
    {
        match agent_owner::authorize_permission(
            connection,
            actor,
            source_agent,
            agent_owner::AgentPermission::Share,
        ) {
            Ok(_) => {}
            Err(Error::Denied(_)) => return Ok(DisclosureAuthorization::Deny),
            Err(Error::Conflict(_)) => return Ok(DisclosureAuthorization::Unknown),
            Err(error) => return Err(error),
        }
    }

    if let Some(manifest) = manifest {
        let roots = manifest_authority_roots(connection, manifest)?;
        let has_source_root = roots.iter().any(|(_, kind, _, _)| kind == "agent_source");
        for (_root, kind, stable_key, _evidence) in roots {
            match kind.as_str() {
                "agent_source" => {
                    for (principal_kind, principal_handle) in &recipient_audience {
                        if let Some(decision) =
                            classify_disclosure_check(validate_agent_source_expansion(
                                connection,
                                actor,
                                &stable_key,
                                principal_kind,
                                principal_handle,
                            ))?
                        {
                            return Ok(decision);
                        }
                    }
                }
                "legacy_session" => {
                    for (principal_kind, principal_handle) in &recipient_audience {
                        if let Some(decision) =
                            classify_disclosure_check(validate_legacy_session_expansion(
                                connection,
                                actor,
                                &stable_key,
                                principal_kind,
                                principal_handle,
                            ))?
                        {
                            return Ok(decision);
                        }
                    }
                }
                "legacy_protected" => {
                    if !has_source_root {
                        for (principal_kind, principal_handle) in &recipient_audience {
                            if !legacy_protected_principal(
                                connection,
                                &stable_key,
                                principal_kind,
                                principal_handle,
                            )? {
                                return Ok(DisclosureAuthorization::Deny);
                            }
                        }
                    }
                }
                "materialized_policy" => {
                    let Some((status, _federation, _revision)) =
                        materialized_policy_state(connection, &stable_key)?
                    else {
                        return Ok(DisclosureAuthorization::Deny);
                    };
                    if status != "active" {
                        return Ok(DisclosureAuthorization::Deny);
                    }
                    // Current AW owns the exact materialization policy, but it has
                    // no principal-level provider/share fence for a recipient
                    // Agent's whole audience. Keep this retryable and pinned.
                    return Ok(DisclosureAuthorization::Unknown);
                }
                _ => return Ok(DisclosureAuthorization::Unknown),
            }
        }
    }

    Ok(DisclosureAuthorization::Allow {
        audience: recipient_audience.into_iter().collect(),
    })
}

fn validate_fork_manifest_for_principal(
    connection: &Connection,
    actor: &PrincipalId,
    new_manifest: &str,
    prior_grant: Option<&str>,
    principal_kind: &str,
    principal_handle: &str,
) -> Result<()> {
    let prior_roots = if let Some(grant) = prior_grant {
        if !grant_covers(connection, grant, principal_kind, principal_handle)? {
            BTreeSet::new()
        } else {
            let prior_manifest: String = db(connection.query_row(
                "SELECT manifest_digest FROM history_grants WHERE grant_id=?1",
                [grant],
                |row| row.get(0),
            ))?;
            manifest_authority_roots(connection, &prior_manifest)?
                .into_iter()
                .map(|(root, _, _, _)| root)
                .collect()
        }
    } else {
        BTreeSet::new()
    };

    let roots = manifest_authority_roots(connection, new_manifest)?;
    let has_source_root = roots.iter().any(|(_, kind, _, _)| kind == "agent_source");
    for (root, kind, stable_key, _evidence) in roots {
        match kind.as_str() {
            "agent_source" => validate_agent_source_expansion(
                connection,
                actor,
                &stable_key,
                principal_kind,
                principal_handle,
            )?,
            // Explicitly revocable/materialized source policy is stronger than
            // immutable prior disclosure and must be revalidated on every new
            // destination. No principal-level source-share contract exists yet.
            "materialized_policy" => {
                return Err(Error::Denied(
                    "materialized external history cannot be forked without source share authority",
                ));
            }
            "legacy_protected" => {
                if !legacy_protected_principal(
                    connection,
                    &stable_key,
                    principal_kind,
                    principal_handle,
                )? && !has_source_root
                {
                    return Err(Error::Denied(
                        "legacy protected Event input cannot expand without reconstructed source authority",
                    ));
                }
            }
            "legacy_session" if prior_roots.contains(&root) => {}
            "legacy_session" => validate_legacy_session_expansion(
                connection,
                actor,
                &stable_key,
                principal_kind,
                principal_handle,
            )?,
            _ => {
                return Err(Error::Denied(
                    "history disclosure authority root has no reviewed fork/share contract",
                ));
            }
        }
    }
    Ok(())
}

fn direct_acl_rows(
    connection: &Connection,
    agent: agent_owner::AgentKey<'_>,
) -> Result<Vec<(String, String, agent_owner::AgentPermissions)>> {
    let mut query = db(connection.prepare(
        "SELECT principal_kind,principal_handle,
                can_view,can_contribute,can_submit,can_control,can_derive,can_share,can_manage
         FROM agent_acl_current
         WHERE session_id=?1 AND branch_id=?2
         ORDER BY principal_kind,principal_handle LIMIT ?3",
    ))?;
    let rows = db(query.query_map(
        params![
            agent.session,
            agent.branch,
            (MAX_FORK_ACL_ENTRIES + 1) as i64
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                agent_owner::AgentPermissions {
                    view: row.get::<_, i64>(2)? != 0,
                    contribute: row.get::<_, i64>(3)? != 0,
                    submit: row.get::<_, i64>(4)? != 0,
                    control: row.get::<_, i64>(5)? != 0,
                    derive: row.get::<_, i64>(6)? != 0,
                    share: row.get::<_, i64>(7)? != 0,
                    manage: row.get::<_, i64>(8)? != 0,
                },
            ))
        },
    ))?;
    let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if rows.len() > MAX_FORK_ACL_ENTRIES {
        return Err(Error::Limit("Agent fork ACL bound exceeded"));
    }
    rows.into_iter()
        .map(|(kind, handle, permissions)| Ok((kind, handle, permissions.validate()?)))
        .collect()
}

fn mint_destination_grant(
    connection: &Connection,
    actor: &PrincipalId,
    base: &HistoryBaseRef,
    source_agent_ref: &str,
    destination_agent_ref: &str,
    audience: &BTreeSet<(String, String)>,
) -> Result<String> {
    if audience.len() > MAX_MANIFEST_INPUTS {
        return Err(Error::Limit("HistoryGrant principal bound exceeded"));
    }
    let grant_id = id("hgrant")?;
    let created_by = user_for_principal(connection, actor)?
        .ok_or(Error::Denied("HistoryGrant creator identity is inactive"))?;
    db(connection.execute(
        "INSERT INTO history_grants(
             grant_id,base_id,source_agent_ref,destination_agent_ref,manifest_digest,
             created_by_handle,supersedes_grant_id,created_at
         ) VALUES(?1,?2,?3,?4,?5,?6,NULL,?7)",
        params![
            grant_id,
            base.id,
            source_agent_ref,
            destination_agent_ref,
            base.manifest_digest,
            created_by,
            now()
        ],
    ))?;
    for (ordinal, (kind, handle)) in audience.iter().enumerate() {
        db(connection.execute(
            "INSERT INTO history_grant_principals(
                 grant_id,ordinal,principal_kind,principal_handle
             ) VALUES(?1,?2,?3,?4)",
            params![grant_id, ordinal as u64, kind, handle],
        ))?;
    }
    Ok(grant_id)
}

/// Validate and durably record one genuinely new history audience principal.
/// Returns the new grant id when history existed and a new disclosure was minted.
pub(crate) fn prepare_disclosure_expansion(
    connection: &Connection,
    actor: &PrincipalId,
    agent: agent_owner::AgentKey<'_>,
    principal_kind: &str,
    principal_handle: &str,
) -> Result<Option<String>> {
    let (current_base, current_grant, destination_agent): (Option<String>, Option<String>, String) =
        db(connection.query_row(
            "SELECT history_base_id,history_grant_id,
                    '/d:' || domain_handle || '/i:' || identity_handle ||
                    '/p:' || project_handle || '/a:' || agent_handle
             FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
            params![agent.session, agent.branch],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ))?;
    if let Some(grant) = current_grant.as_deref() {
        if grant_covers(connection, grant, principal_kind, principal_handle)? {
            return Ok(None);
        }
    }

    let session = session_in(connection, agent.session)?;
    let branch = branch_in(connection, agent.session, agent.branch)?;
    let local_history = !context::projected(connection, &session.id, &branch.head)?.is_empty();
    if current_base.is_none() && !local_history {
        return Ok(None);
    }

    agent_owner::authorize_permission(
        connection,
        actor,
        agent,
        agent_owner::AgentPermission::Share,
    )?;
    let base = seal_in(connection, agent.session, agent.branch)?;
    validate_manifest_expansion(
        connection,
        actor,
        &base.manifest_digest,
        principal_kind,
        principal_handle,
    )?;

    let mut principals = current_disclosed_principals(connection, agent, current_grant.as_deref())?;
    principals.insert((principal_kind.to_owned(), principal_handle.to_owned()));
    if principals.len() > MAX_MANIFEST_INPUTS {
        return Err(Error::Limit("HistoryGrant principal bound exceeded"));
    }
    let grant_id = id("hgrant")?;
    let created_by = user_for_principal(connection, actor)?
        .ok_or(Error::Denied("HistoryGrant creator identity is inactive"))?;
    db(connection.execute(
        "INSERT INTO history_grants(
             grant_id,base_id,source_agent_ref,destination_agent_ref,manifest_digest,
             created_by_handle,supersedes_grant_id,created_at
         ) VALUES(?1,?2,?3,?3,?4,?5,?6,?7)",
        params![
            grant_id,
            base.id,
            destination_agent,
            base.manifest_digest,
            created_by,
            current_grant,
            now()
        ],
    ))?;
    for (ordinal, (kind, handle)) in principals.iter().enumerate() {
        db(connection.execute(
            "INSERT INTO history_grant_principals(
                 grant_id,ordinal,principal_kind,principal_handle
             ) VALUES(?1,?2,?3,?4)",
            params![grant_id, ordinal as u64, kind, handle],
        ))?;
    }
    let changed = db(connection.execute(
        "UPDATE agent_state_current SET history_base_id=?1,history_grant_id=?2
         WHERE session_id=?3 AND branch_id=?4",
        params![base.id, grant_id, agent.session, agent.branch],
    ))?;
    if changed != 1 {
        return Err(Error::Storage("Agent history grant pointer update failed"));
    }
    Ok(Some(grant_id))
}

type InheritedAcl = Vec<(String, String, agent_owner::AgentPermissions)>;

struct ForkAudience {
    inherited_acl: InheritedAcl,
    audience: BTreeSet<(String, String)>,
}

fn fork_audience_in(
    connection: &Connection,
    actor: &PrincipalId,
    source: &ResolvedHistorySource,
    destination_owner: &str,
    access: AgentHistoryForkAccess,
) -> Result<ForkAudience> {
    let source_agent = agent_owner::AgentKey::new(&source.session_id, &source.branch_id);
    let (owner_kind, _owner_handle): (String, String) = db(connection.query_row(
        "SELECT owner_kind,owner_handle
         FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
        params![source.session_id, source.branch_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    if owner_kind != "user" {
        return Err(Error::Denied(
            "exact-history constructor currently requires a user-owned source Project",
        ));
    }
    let inherited_acl = match access {
        AgentHistoryForkAccess::Private => Vec::new(),
        AgentHistoryForkAccess::Inherit => direct_acl_rows(connection, source_agent)?,
    };
    let mut audience = BTreeSet::from([("user".to_owned(), destination_owner.to_owned())]);
    for (kind, handle, permissions) in &inherited_acl {
        if permissions.view {
            audience.insert((kind.clone(), handle.clone()));
        }
    }
    for (kind, handle) in &audience {
        validate_fork_manifest_for_principal(
            connection,
            actor,
            &source.base.manifest_digest,
            source.current_grant_id.as_deref(),
            kind,
            handle,
        )?;
    }

    Ok(ForkAudience {
        inherited_acl,
        audience,
    })
}

fn attach_resolved_history_to_destination_in(
    connection: &Connection,
    actor: &PrincipalId,
    source: &ResolvedHistorySource,
    destination_session: &str,
    destination_branch: &str,
    access: AgentHistoryForkAccess,
) -> Result<()> {
    let (destination_owner_kind, destination_owner): (String, String) = db(connection.query_row(
        "SELECT owner_kind,owner_handle
             FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
        params![destination_session, destination_branch],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    if destination_owner_kind != "user" {
        return Err(Error::Denied(
            "exact-history destination currently requires a user-owned Project",
        ));
    }

    let ForkAudience {
        inherited_acl,
        audience,
    } = fork_audience_in(connection, actor, source, &destination_owner, access)?;

    let destination_ref: String = db(connection.query_row(
        "SELECT '/d:' || domain_handle || '/i:' || identity_handle ||
                '/p:' || project_handle || '/a:' || agent_handle
         FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
        params![destination_session, destination_branch],
        |row| row.get(0),
    ))?;
    let created_by = user_for_principal(connection, actor)?
        .ok_or(Error::Denied("Agent fork creator identity is inactive"))?;
    for (kind, handle, permissions) in &inherited_acl {
        db(connection.execute(
            "INSERT INTO agent_acl_current(
                 session_id,branch_id,principal_kind,principal_handle,
                 can_view,can_contribute,can_submit,can_control,can_derive,can_share,can_manage,
                 grant_rev,granted_by_handle,updated_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,1,?12,?13)",
            params![
                destination_session,
                destination_branch,
                kind,
                handle,
                i64::from(permissions.view),
                i64::from(permissions.contribute),
                i64::from(permissions.submit),
                i64::from(permissions.control),
                i64::from(permissions.derive),
                i64::from(permissions.share),
                i64::from(permissions.manage),
                created_by,
                now(),
            ],
        ))?;
    }
    let grant_id = mint_destination_grant(
        connection,
        actor,
        &source.base,
        &source.agent_ref,
        &destination_ref,
        &audience,
    )?;
    let changed = db(connection.execute(
        "UPDATE agent_state_current
         SET history_base_id=?1,history_grant_id=?2,updated_at=?3
         WHERE session_id=?4 AND branch_id=?5",
        params![
            source.base.id,
            grant_id,
            now(),
            destination_session,
            destination_branch
        ],
    ))?;
    if changed != 1 {
        return Err(Error::Storage(
            "destination Agent history pointer initialization failed",
        ));
    }
    let session_changed = db(connection.execute(
        "UPDATE sessions SET next_turn=?1,updated_at=?2 WHERE id=?3",
        params![source.base.next_turn, now(), destination_session],
    ))?;
    if session_changed != 1 {
        return Err(Error::Storage(
            "destination Session inherited Turn cursor initialization failed",
        ));
    }
    crate::audit(
        connection,
        destination_session,
        actor,
        "agent.history_inherited",
        serde_json::json!({
            "source_agent":source.agent_ref,
            "destination_agent":destination_ref,
            "history_content_digest":source.base.content_digest,
            "access":access,
        }),
    )?;
    let txid = crate::durability::tx_id()?;
    crate::durability::commit_event(
        connection,
        &txid,
        "agent.history_inherited",
        "agent",
        &destination_ref,
        Some(actor),
        serde_json::json!({
            "session_id":destination_session,
            "branch_id":destination_branch,
            "source_agent":source.agent_ref,
            "history_content_digest":source.base.content_digest,
        }),
    )?;
    Ok(())
}

pub(crate) fn attach_canonical_history_to_existing_agent_in(
    connection: &Connection,
    actor: &PrincipalId,
    canonical_source: &str,
    destination_session: &str,
    destination_branch: &str,
    access: AgentHistoryForkAccess,
) -> Result<()> {
    let source = resolve_canonical_history_source_in(connection, actor, canonical_source)?;
    attach_resolved_history_to_destination_in(
        connection,
        actor,
        &source,
        destination_session,
        destination_branch,
        access,
    )
}

pub(crate) fn create_agent_from_history_ref_in(
    connection: &Connection,
    actor: &PrincipalId,
    canonical_source: &str,
    title: &str,
    binding: &SessionBinding,
    access: AgentHistoryForkAccess,
) -> Result<(crate::SessionInfo, crate::BranchInfo)> {
    create_agent_from_history_ref_with_ids_in(
        connection,
        actor,
        canonical_source,
        title,
        binding,
        access,
        resources::mint_session_candidate_ids()?,
    )
}

fn authorize_fork_destination_in(
    connection: &Connection,
    actor: &PrincipalId,
    source: &ResolvedHistorySource,
    destination_owner: &PrincipalId,
    destination_workspace: &str,
) -> Result<String> {
    let (source_owner, source_workspace): (String, Option<String>) = db(connection.query_row(
        "SELECT owner_user_id,workspace_id FROM sessions WHERE id=?1",
        [&source.session_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    let source_workspace = source_workspace.ok_or(Error::Storage(
        "source Agent is missing its authoritative Project binding",
    ))?;
    let destination_user = user_for_principal(connection, destination_owner)?.ok_or(
        Error::Denied("destination Agent owner identity is inactive"),
    )?;
    if source_owner != destination_user || source_workspace != destination_workspace {
        agent_owner::authorize_permission(
            connection,
            actor,
            agent_owner::AgentKey::new(&source.session_id, &source.branch_id),
            agent_owner::AgentPermission::Share,
        )?;
    }
    Ok(destination_user)
}

/// Read-only authorization for the same exact-history constructor. Used
/// before external preparation; final construction repeats current checks.
pub(crate) fn preflight_resolved_history_in_project(
    connection: &Connection,
    actor: &PrincipalId,
    source: &ResolvedHistorySource,
    access: AgentHistoryForkAccess,
    destination_owner: &PrincipalId,
    destination_workspace: &str,
) -> Result<()> {
    let destination_user = authorize_fork_destination_in(
        connection,
        actor,
        source,
        destination_owner,
        destination_workspace,
    )?;
    fork_audience_in(connection, actor, source, &destination_user, access)?;
    Ok(())
}

pub(crate) fn create_agent_from_resolved_history_with_ids_in(
    connection: &Connection,
    actor: &PrincipalId,
    source: &ResolvedHistorySource,
    title: &str,
    binding: &SessionBinding,
    access: AgentHistoryForkAccess,
    destination_owner: &PrincipalId,
    destination_workspace: &str,
    ids: resources::SessionCandidateIds,
    fresh: Option<&crate::sandbox_owner::FreshSandbox>,
) -> Result<(crate::SessionInfo, crate::BranchInfo)> {
    authorize_fork_destination_in(
        connection,
        actor,
        source,
        destination_owner,
        destination_workspace,
    )?;
    let (destination_session, destination_branch) =
        resources::create_in_workspace_with_ids_and_sandbox_tx(
            connection,
            destination_owner,
            title,
            binding,
            destination_workspace,
            None,
            ids,
            fresh,
        )?;
    attach_resolved_history_to_destination_in(
        connection,
        actor,
        source,
        &destination_session.id,
        &destination_branch.id,
        access,
    )?;
    Ok((
        session_in(connection, &destination_session.id)?,
        branch_in(connection, &destination_session.id, &destination_branch.id)?,
    ))
}

pub(crate) fn create_agent_from_history_ref_with_ids_in(
    connection: &Connection,
    actor: &PrincipalId,
    canonical_source: &str,
    title: &str,
    binding: &SessionBinding,
    access: AgentHistoryForkAccess,
    ids: resources::SessionCandidateIds,
) -> Result<(crate::SessionInfo, crate::BranchInfo)> {
    crate::label(title, 160)?;
    crate::validate_binding(binding)?;
    let source = resolve_canonical_history_source_in(connection, actor, canonical_source)?;
    let (owner_kind, owner_handle, workspace): (String, String, Option<String>) = db(connection
        .query_row(
            "SELECT a.owner_kind,a.owner_handle,s.workspace_id
             FROM agent_state_current a JOIN sessions s ON s.id=a.session_id
             WHERE a.session_id=?1 AND a.branch_id=?2",
            params![source.session_id, source.branch_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ))?;
    if owner_kind != "user" {
        return Err(Error::Denied(
            "exact-history constructor currently requires a user-owned source Project",
        ));
    }
    let workspace = workspace.ok_or(Error::Storage(
        "source Agent is missing its authoritative Project binding",
    ))?;
    let destination_owner = stable_user_principal(&owner_handle)?;
    create_agent_from_resolved_history_with_ids_in(
        connection,
        actor,
        &source,
        title,
        binding,
        access,
        &destination_owner,
        &workspace,
        ids,
        None,
    )
}

pub(crate) fn create_agent_from_history_ref_in_project(
    connection: &Connection,
    actor: &PrincipalId,
    canonical_source: &str,
    title: &str,
    binding: &SessionBinding,
    access: AgentHistoryForkAccess,
    destination_owner: &PrincipalId,
    destination_workspace: &str,
) -> Result<(crate::SessionInfo, crate::BranchInfo)> {
    create_agent_from_history_ref_with_ids_in_project(
        connection,
        actor,
        canonical_source,
        title,
        binding,
        access,
        destination_owner,
        destination_workspace,
        resources::mint_session_candidate_ids()?,
    )
}

pub(crate) fn create_agent_from_history_ref_with_ids_in_project(
    connection: &Connection,
    actor: &PrincipalId,
    canonical_source: &str,
    title: &str,
    binding: &SessionBinding,
    access: AgentHistoryForkAccess,
    destination_owner: &PrincipalId,
    destination_workspace: &str,
    ids: resources::SessionCandidateIds,
) -> Result<(crate::SessionInfo, crate::BranchInfo)> {
    crate::label(title, 160)?;
    crate::validate_binding(binding)?;
    let source = resolve_canonical_history_source_in(connection, actor, canonical_source)?;
    create_agent_from_resolved_history_with_ids_in(
        connection,
        actor,
        &source,
        title,
        binding,
        access,
        destination_owner,
        destination_workspace,
        ids,
        None,
    )
}

impl SessionStore {
    /// Trusted lower constructor for a structurally shared exact-history Agent.
    /// History ancestry is independent from execution/Sandbox ancestry: callers
    /// supply the destination binding explicitly. This convenience API preserves
    /// the source Project/owner default; host redirect uses the same lower core
    /// with an explicit authorized destination.
    pub fn create_agent_from_history_ref(
        &self,
        actor: &PrincipalId,
        canonical_source: &str,
        title: &str,
        binding: SessionBinding,
        access: AgentHistoryForkAccess,
    ) -> Result<(crate::SessionInfo, crate::BranchInfo)> {
        let mut connection = self.connection()?;
        let tx =
            db(connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        let result = create_agent_from_history_ref_in(
            &tx,
            actor,
            canonical_source,
            title,
            &binding,
            access,
        )?;
        db(tx.commit())?;
        Ok(result)
    }

    pub fn create_agent_from_agent_history(
        &self,
        actor: &PrincipalId,
        source_session: &str,
        source_branch: &str,
        title: &str,
        binding: SessionBinding,
        access: AgentHistoryForkAccess,
    ) -> Result<(crate::SessionInfo, crate::BranchInfo)> {
        let canonical_source = {
            let connection = self.connection()?;
            db(connection.query_row(
                "SELECT '/d:' || domain_handle || '/i:' || identity_handle ||
                        '/p:' || project_handle || '/a:' || agent_handle
                 FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
                params![source_session, source_branch],
                |row| row.get::<_, String>(0),
            ))?
        };
        self.create_agent_from_history_ref(actor, &canonical_source, title, binding, access)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PrincipalId, SessionBinding, SessionStore};
    use aw_agent_core::{
        AccountOwner, BillingOwner, CredentialCustody, InvocationIdentity, Outcome, Turn,
        TurnJournal, TurnStart,
    };
    use std::path::PathBuf;

    fn binding() -> SessionBinding {
        SessionBinding {
            working_directory: PathBuf::from("/tmp"),
            tool_root: PathBuf::from("/tmp"),
            system_prompt: "system".into(),
            profile_hint: "fixture".into(),
            model_hint: "model".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    fn identity(actor: &PrincipalId) -> InvocationIdentity {
        InvocationIdentity {
            actor: actor.clone(),
            account_id: "fixture".into(),
            account_owner: AccountOwner::User(actor.clone()),
            payer: BillingOwner::User(actor.clone()),
            credential_custody: CredentialCustody::UserDevice("fixture".into()),
            credential_version: None,
            inference_executor: "local".into(),
            model: "fixture".into(),
            grant_revision: 1,
        }
    }

    #[test]
    fn lazy_seal_is_idempotent_and_revision_neutral() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("internal:fixture:history-owner").unwrap();
        let (session, branch) = store.create(&actor, "history", binding()).unwrap();
        let rev = store
            .agent_public_revision(&actor, &session.id, &branch.id)
            .unwrap();

        let connection = store.connection().unwrap();
        let first = seal_in(&connection, &session.id, &branch.id).unwrap();
        let second = seal_in(&connection, &session.id, &branch.id).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.total_turns, 0);
        assert_eq!(first.next_turn, 1);
        drop(connection);
        assert_eq!(
            store
                .agent_public_revision(&actor, &session.id, &branch.id)
                .unwrap(),
            rev,
            "lazy sealing cannot manufacture an Agent revision"
        );
    }

    #[test]
    fn full_agent_access_mints_and_reuses_history_disclosure_grants() {
        let store = SessionStore::in_memory().unwrap();
        let owner = PrincipalId::new("internal:fixture:access-owner").unwrap();
        let manager = PrincipalId::new("internal:fixture:access-manager").unwrap();
        let bob = PrincipalId::new("internal:fixture:access-bob").unwrap();
        let newcomer = PrincipalId::new("internal:fixture:access-newcomer").unwrap();
        let (session, branch) = store.create(&owner, "history access", binding()).unwrap();
        let domain = store.default_domain(&owner).unwrap();
        let (manager_ref, bob_ref, newcomer_ref) = {
            let connection = store.connection().unwrap();
            let manager_user = crate::ensure_user_for_principal(&connection, &manager).unwrap();
            let bob_user = crate::ensure_user_for_principal(&connection, &bob).unwrap();
            let newcomer_user = crate::ensure_user_for_principal(&connection, &newcomer).unwrap();
            (
                format!("/d:{}/i:{manager_user}", domain.id),
                format!("/d:{}/i:{bob_user}", domain.id),
                format!("/d:{}/i:{newcomer_user}", domain.id),
            )
        };
        let checkpoint = store
            .checkpoint(
                &owner,
                &session.id,
                &branch.id,
                branch.revision,
                "sealed history",
            )
            .unwrap();
        assert_eq!(checkpoint.id, branch.id);

        let grant_pointer = || {
            let connection = store.connection().unwrap();
            connection
                .query_row(
                    "SELECT history_grant_id FROM agent_state_current
                     WHERE session_id=?1 AND branch_id=?2",
                    params![session.id, branch.id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .unwrap()
        };
        let rev0 = store
            .agent_public_revision(&owner, &session.id, &branch.id)
            .unwrap();
        let rev1 = store
            .agent_access_set(
                &owner,
                &session.id,
                &branch.id,
                &manager_ref,
                Some(crate::AgentPermissions {
                    view: true,
                    manage: true,
                    ..Default::default()
                }),
                rev0,
            )
            .unwrap();
        let first_grant =
            grant_pointer().expect("first history audience expansion must mint grant");

        let bob_permissions = crate::AgentPermissions {
            view: true,
            derive: true,
            ..Default::default()
        };
        let rev2 = store
            .agent_access_set(
                &owner,
                &session.id,
                &branch.id,
                &bob_ref,
                Some(bob_permissions),
                rev1,
            )
            .unwrap();
        let second_grant = grant_pointer().expect("second expansion must retain a current grant");
        assert_ne!(second_grant, first_grant);
        let supersedes: Option<String> = {
            let connection = store.connection().unwrap();
            connection
                .query_row(
                    "SELECT supersedes_grant_id FROM history_grants WHERE grant_id=?1",
                    [&second_grant],
                    |row| row.get(0),
                )
                .unwrap()
        };
        assert_eq!(supersedes.as_deref(), Some(first_grant.as_str()));

        let snapshot = store
            .agent_access_list(&owner, &session.id, &branch.id)
            .unwrap();
        let bob_entry = snapshot
            .entries
            .iter()
            .find(|entry| entry.principal_ref == bob_ref)
            .expect("Bob grant must be listed");
        assert_eq!(bob_entry.permissions, bob_permissions);
        assert!(store
            .authorize_agent_permission(
                &bob,
                &session.id,
                &branch.id,
                crate::AgentPermission::Derive,
            )
            .is_ok());
        assert!(store
            .authorize_agent_permission(
                &bob,
                &session.id,
                &branch.id,
                crate::AgentPermission::Share,
            )
            .is_err());

        let rev3 = store
            .agent_access_set(&manager, &session.id, &branch.id, &bob_ref, None, rev2)
            .unwrap();
        assert_eq!(grant_pointer().as_deref(), Some(second_grant.as_str()));
        let rev4 = store
            .agent_access_set(
                &manager,
                &session.id,
                &branch.id,
                &bob_ref,
                Some(bob_permissions),
                rev3,
            )
            .expect("already-disclosed Bob can be re-added without share authority");
        assert_eq!(grant_pointer().as_deref(), Some(second_grant.as_str()));
        assert!(store
            .agent_access_set(
                &manager,
                &session.id,
                &branch.id,
                &newcomer_ref,
                Some(crate::AgentPermissions::VIEW_ONLY),
                rev4,
            )
            .is_err());
        assert_eq!(
            store
                .agent_public_revision(&owner, &session.id, &branch.id)
                .unwrap(),
            rev4,
            "failed new disclosure cannot advance Agent revision"
        );
    }

    #[tokio::test]
    async fn exact_history_fork_uses_fresh_session_and_latest_committed_boundary() {
        let store = SessionStore::in_memory().unwrap();
        let owner = PrincipalId::new("internal:fixture:fork-owner").unwrap();
        let bob = PrincipalId::new("internal:fixture:fork-bob").unwrap();
        let (source, source_branch) = store.create(&owner, "source", binding()).unwrap();
        let domain = store.default_domain(&owner).unwrap();
        let (bob_ref, source_ref) = {
            let connection = store.connection().unwrap();
            let bob_user = crate::ensure_user_for_principal(&connection, &bob).unwrap();
            let source_ref: String = connection
                .query_row(
                    "SELECT '/d:' || domain_handle || '/i:' || identity_handle ||
                            '/p:' || project_handle || '/a:' || agent_handle
                     FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
                    params![source.id, source_branch.id],
                    |row| row.get(0),
                )
                .unwrap();
            (format!("/d:{}/i:{bob_user}", domain.id), source_ref)
        };
        let source_rev = store
            .agent_public_revision(&owner, &source.id, &source_branch.id)
            .unwrap();
        store
            .agent_access_set(
                &owner,
                &source.id,
                &source_branch.id,
                &bob_ref,
                Some(crate::AgentPermissions {
                    view: true,
                    derive: true,
                    ..Default::default()
                }),
                source_rev,
            )
            .unwrap();

        let (_, source_handle) = store
            .activate(&owner, &source.id, &source_branch.id)
            .unwrap();
        let identity = identity(&owner);
        source_handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "committed seed".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        source_handle
            .finish(Turn {
                id: 1,
                prompt: "committed seed".into(),
                answer: "seed answer".into(),
                model: identity.model.clone(),
                profile: identity.account_id.clone(),
                outcome: Outcome::Completed,
                meta: None,
                error: None,
                actor: Some(owner.clone()),
                tool_records: Vec::new(),
                invocations: Vec::new(),
                context_messages: Vec::new(),
                needs_reconciliation: false,
            })
            .await
            .unwrap();
        {
            let connection = store.connection().unwrap();
            let payload: String = connection
                .query_row(
                    "SELECT payload_json FROM commit_log
                     WHERE kind='llm.attempt_settled'
                       AND json_extract(payload_json,'$.session_id')=?1
                     ORDER BY seq DESC LIMIT 1",
                    [&source.id],
                    |row| row.get(0),
                )
                .unwrap();
            let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
            assert_eq!(payload["agent_ref"], source_ref);
            assert_eq!(payload["turn"], 1);
        }

        let (inherited, inherited_branch) = store
            .create_agent_from_agent_history(
                &owner,
                &source.id,
                &source_branch.id,
                "inherited",
                binding(),
                AgentHistoryForkAccess::Inherit,
            )
            .unwrap();
        assert_ne!(inherited.id, source.id);
        assert_eq!(inherited.workspace_id, source.workspace_id);
        assert_eq!(inherited.next_turn, 2);
        assert_eq!(inherited_branch.name, "main");
        assert!(inherited_branch.active_run.is_none());
        let inherited_view = store
            .read_agent_branch_view(&owner, &inherited.id, &inherited_branch.id)
            .unwrap();
        assert_eq!(inherited_view.history.turns.len(), 1);
        assert_eq!(inherited_view.history.turns[0].id, 1);
        assert_eq!(inherited_view.history.turns[0].answer, "seed answer");
        assert_eq!(inherited_view.history.next_turn_id, 2);
        let source_provenance = store
            .agent_turn_provenance(&owner, &source.id, &source_branch.id, &[1])
            .unwrap()
            .remove(&1)
            .expect("source Turn provenance");
        let inherited_provenance = store
            .agent_turn_provenance(&owner, &inherited.id, &inherited_branch.id, &[1])
            .unwrap()
            .remove(&1)
            .expect("inherited Turn provenance");
        assert_eq!(source_provenance, inherited_provenance);
        assert_eq!(source_provenance.original_agent, source_ref);
        assert_eq!(source_provenance.original_turn, 1);
        assert_eq!(source_provenance.actor, owner);
        let (activated_view, _) = store
            .activate(&owner, &inherited.id, &inherited_branch.id)
            .unwrap();
        assert_eq!(activated_view.history.turns.len(), 1);
        assert_eq!(activated_view.history.next_turn_id, 2);
        assert!(store
            .authorize_agent_permission(
                &bob,
                &inherited.id,
                &inherited_branch.id,
                crate::AgentPermission::Derive,
            )
            .is_ok());
        let inherited_rev = store
            .agent_public_revision(&owner, &inherited.id, &inherited_branch.id)
            .unwrap();
        assert_eq!(inherited_rev, 1, "fork starts with fresh Agent revision");
        let inherited_ref: String = {
            let connection = store.connection().unwrap();
            connection
                .query_row(
                    "SELECT '/d:' || domain_handle || '/i:' || identity_handle ||
                            '/p:' || project_handle || '/a:' || agent_handle
                     FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
                    params![inherited.id, inherited_branch.id],
                    |row| row.get(0),
                )
                .unwrap()
        };
        let (reforked_turn, reforked_turn_branch) = store
            .create_agent_from_history_ref(
                &owner,
                &format!("{inherited_ref}/t:1"),
                "inherited turn prefix",
                binding(),
                AgentHistoryForkAccess::Private,
            )
            .unwrap();
        assert_eq!(reforked_turn.next_turn, 2);
        let reforked_turn_view = store
            .read_agent_branch_view(&owner, &reforked_turn.id, &reforked_turn_branch.id)
            .unwrap();
        assert_eq!(reforked_turn_view.history.turns.len(), 1);
        assert_eq!(reforked_turn_view.history.turns[0].id, 1);
        let reforked_provenance = store
            .agent_turn_provenance(&owner, &reforked_turn.id, &reforked_turn_branch.id, &[1])
            .unwrap()
            .remove(&1)
            .expect("reforked Turn provenance");
        assert_eq!(reforked_provenance, source_provenance);
        {
            let connection = store.connection().unwrap();
            let local_nodes: u64 = connection
                .query_row(
                    "SELECT count(*) FROM nodes WHERE session_id=?1",
                    [&inherited.id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                local_nodes, 1,
                "inherited history is not copied into the new Session"
            );
        }

        let current_source_rev = store
            .agent_public_revision(&owner, &source.id, &source_branch.id)
            .unwrap();
        store
            .agent_access_set(
                &owner,
                &source.id,
                &source_branch.id,
                &bob_ref,
                None,
                current_source_rev,
            )
            .unwrap();
        assert!(store
            .authorize_agent_permission(
                &bob,
                &source.id,
                &source_branch.id,
                crate::AgentPermission::View,
            )
            .is_err());
        assert!(store
            .authorize_agent_permission(
                &bob,
                &inherited.id,
                &inherited_branch.id,
                crate::AgentPermission::View,
            )
            .is_ok());

        source_handle
            .begin(TurnStart {
                turn_id: 2,
                prompt: "still running".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        assert!(store
            .create_agent_from_history_ref(
                &owner,
                &format!("{source_ref}/t:2"),
                "unsettled turn fork",
                binding(),
                AgentHistoryForkAccess::Private,
            )
            .is_err());
        let (private, private_branch) = store
            .create_agent_from_agent_history(
                &owner,
                &source.id,
                &source_branch.id,
                "private busy fork",
                binding(),
                AgentHistoryForkAccess::Private,
            )
            .unwrap();
        assert_eq!(private.next_turn, 2);
        let private_view = store
            .read_agent_branch_view(&owner, &private.id, &private_branch.id)
            .unwrap();
        assert_eq!(private_view.history.turns.len(), 1);
        assert_eq!(private_view.history.turns[0].id, 1);
        assert_eq!(private_view.history.next_turn_id, 2);
        assert!(store
            .authorize_agent_permission(
                &bob,
                &private.id,
                &private_branch.id,
                crate::AgentPermission::View,
            )
            .is_err());
        {
            let connection = store.connection().unwrap();
            let local_nodes: u64 = connection
                .query_row(
                    "SELECT count(*) FROM nodes WHERE session_id=?1",
                    [&private.id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(local_nodes, 1);
        }

        source_handle
            .finish(Turn {
                id: 2,
                prompt: "still running".into(),
                answer: "second answer".into(),
                model: identity.model.clone(),
                profile: identity.account_id.clone(),
                outcome: Outcome::Completed,
                meta: None,
                error: None,
                actor: Some(owner.clone()),
                tool_records: Vec::new(),
                invocations: Vec::new(),
                context_messages: Vec::new(),
                needs_reconciliation: false,
            })
            .await
            .unwrap();
        let (turn_one, turn_one_branch) = store
            .create_agent_from_history_ref(
                &owner,
                &format!("{source_ref}/t:1"),
                "turn one prefix",
                binding(),
                AgentHistoryForkAccess::Private,
            )
            .unwrap();
        assert_eq!(turn_one.next_turn, 2);
        let turn_one_view = store
            .read_agent_branch_view(&owner, &turn_one.id, &turn_one_branch.id)
            .unwrap();
        assert_eq!(turn_one_view.history.turns.len(), 1);
        assert_eq!(turn_one_view.history.turns[0].id, 1);
        assert_eq!(turn_one_view.history.turns[0].answer, "seed answer");
        assert_eq!(turn_one_view.history.next_turn_id, 2);
    }

    #[test]
    fn manifest_builder_is_bounded_fanout_and_content_addressed() {
        let store = SessionStore::in_memory().unwrap();
        let connection = store.connection().unwrap();
        let entries = (0..130)
            .map(|index| ManifestEntry {
                kind: "fact",
                reference: format!("fact:{index:04}"),
            })
            .collect::<BTreeSet<_>>();
        let first = manifest_from_entries(&connection, &entries).unwrap();
        let second = manifest_from_entries(&connection, &entries).unwrap();
        assert_eq!(first, second);
        assert_eq!(manifest_count(&connection, &first).unwrap(), 130);
        let kind: String = connection
            .query_row(
                "SELECT node_kind FROM disclosure_manifest_nodes WHERE node_digest=?1",
                [&first],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(kind, "branch");
        let max_leaf: u64 = connection
            .query_row(
                "SELECT COALESCE(MAX(item_count),0) FROM disclosure_manifest_nodes
                 WHERE node_kind='leaf'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(max_leaf <= MANIFEST_FANOUT as u64);
    }
}
