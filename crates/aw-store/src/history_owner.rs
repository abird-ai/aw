//! Immutable DL-200 history/disclosure owner.
//!
//! Existing Session nodes remain the physical local journal. This module seals an
//! exact settled Agent boundary into structurally shared immutable HistorySpan /
//! HistoryBase rows and a content-addressed disclosure-manifest DAG.

use crate::{branch_in, context, db, json, now, session_in, Error, Result};
use rusqlite::{params, Connection};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;

const MANIFEST_FANOUT: usize = 64;
const MAX_MANIFEST_INPUTS: usize = 16_384;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoryBaseRef {
    pub(crate) id: String,
    pub(crate) content_digest: String,
    pub(crate) manifest_digest: String,
    pub(crate) total_turns: u64,
    pub(crate) next_turn: u64,
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

    manifest_from_entries(connection, &entries)
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

fn projected_node_digests(
    connection: &Connection,
    session: &crate::SessionInfo,
    branch: &crate::BranchInfo,
) -> Result<Vec<(String, String)>> {
    context::projected(connection, &session.id, &branch.head)?
        .into_iter()
        .map(|node| {
            let digest = raw_node_digest(connection, &session.id, &node.id)?;
            Ok((node.id, digest))
        })
        .collect()
}

pub(crate) fn seal_in(
    connection: &Connection,
    session_id: &str,
    branch_id: &str,
) -> Result<HistoryBaseRef> {
    let session = session_in(connection, session_id)?;
    let branch = branch_in(connection, session_id, branch_id)?;
    if branch.id != branch_id {
        return Err(Error::Denied(
            "history sealing requires exact branch identity",
        ));
    }
    if branch.active_run.is_some() {
        return Err(Error::Conflict(
            "cannot seal history while the Agent has active work",
        ));
    }
    let head_kind: String = db(connection.query_row(
        "SELECT kind FROM nodes WHERE session_id=?1 AND id=?2",
        params![session_id, branch.head],
        |row| row.get(0),
    ))?;
    if head_kind == "prompt" {
        return Err(Error::Conflict(
            "cannot seal history at an unresolved prompt boundary",
        ));
    }

    let (parent_base, source_agent): (Option<String>, String) = db(connection.query_row(
        "SELECT history_base_id,
                '/d:' || domain_handle || '/i:' || identity_handle ||
                '/p:' || project_handle || '/a:' || agent_handle
         FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
        params![session_id, branch_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;

    let projected = projected_node_digests(connection, &session, &branch)?;
    if projected.is_empty() {
        if let Some(parent) = parent_base {
            return base_row(connection, &parent);
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
    let local_manifest = dependency_manifest(connection, &branch.head)?;
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
            &branch.head,
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
            &branch.head,
            &content_digest,
            &provenance_digest,
            &manifest_digest,
        ],
    );
    let span_id = format!("hsp_{span_seal_key}");
    let total_turns = session
        .next_turn
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
            branch.head,
            content_digest,
            provenance_digest,
            manifest_digest,
            total_turns,
            session.next_turn,
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
            &session.next_turn.to_string(),
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
            session.next_turn,
            base_content_digest,
            manifest_digest,
            now()
        ],
    ))?;
    base_row(connection, &base_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PrincipalId, SessionBinding, SessionStore};
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
