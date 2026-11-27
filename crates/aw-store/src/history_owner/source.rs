//! Conservative disclosure ceiling for actual source custody. The source
//! grant currently authorizes one stable actor, not an expanded audience.
//! No provider/share contract is inferred from Agent or Project ACL.
use super::*;
use crate::sandbox_owner::history_disclosure_manifests;

pub(super) const KIND: &str = "source_actor_v1";
// Private manual context reuse does not authorize a new inference
// destination. Carry that stronger ceiling in the same immutable DAG.
pub(super) const CONTEXT_KIND: &str = "source_context_manual_v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ceiling {
    user: String,
}

fn require_ceiling(evidence: &str, kind: &str, user: &str) -> Result<()> {
    let ceiling: Ceiling = crate::decode(evidence)?;
    if kind != "user" || ceiling.user != user {
        return Err(Error::Denied(
            "protected source disclosure is actor-private",
        ));
    }
    crate::stable_user_principal(user)?;
    Ok(())
}

pub(super) fn require_principal(
    connection: &Connection,
    evidence: &str,
    kind: &str,
    user: &str,
) -> Result<()> {
    require_ceiling(evidence, kind, user)?;
    let active: bool = db(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM users WHERE user_id=?1 AND status='active')",
        [user],
        |row| row.get(0),
    ))?;
    if !active {
        return Err(Error::Denied("protected source audience is inactive"));
    }
    Ok(())
}

/// A source claim may expose content immediately to the live Agent/Job
/// and all of its views. Until an independent source share policy exists,
/// allow only one stable user across the Session's legacy and Agent
/// audiences. This is checked in the actual claim transaction.
pub(crate) fn record_claim(
    connection: &Connection,
    session: &str,
    actor: &PrincipalId,
    facts: impl IntoIterator<Item = String>,
) -> Result<String> {
    let user = user_for_principal(connection, actor)?
        .ok_or(Error::Denied("source disclosure actor is inactive"))?;
    require_private_audience(connection, session, &user)?;
    let evidence = json(&Ceiling { user })?;
    let root = authority_root(connection, KIND, session, &evidence)?;
    let mut entries: BTreeSet<ManifestEntry> = facts
        .into_iter()
        .map(|reference| ManifestEntry {
            kind: "fact",
            reference,
        })
        .collect();
    entries.insert(ManifestEntry {
        kind: "authority_root",
        reference: root,
    });
    manifest_from_entries(connection, &entries)
}

pub(crate) fn require_private_audience(
    connection: &Connection,
    session: &str,
    user: &str,
) -> Result<()> {
    let principal = crate::stable_user_principal(user)?;
    if crate::session_owner_user(connection, session)? != user
        || context::audience(connection, session)? != BTreeSet::from([principal])
    {
        return Err(Error::Denied(
            "source disclosure requires a private Session audience",
        ));
    }
    let shared: bool = db(connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM agent_state_current
            WHERE session_id=?1 AND (owner_kind!='user' OR owner_handle!=?2)
         ) OR EXISTS(
            SELECT 1 FROM agent_acl_current
            WHERE session_id=?1 AND can_view=1
              AND (principal_kind!='user' OR principal_handle!=?2)
         )",
        params![session, user],
        |row| row.get(0),
    ))?;
    if shared {
        return Err(Error::Denied(
            "source disclosure requires a private Agent audience",
        ));
    }
    #[cfg(feature = "multiplayer")]
    {
        let shared: bool = db(connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM session_draft_grants
              WHERE session_id=?1 AND status='active' AND user_id!=?2 AND can_read=1)",
            params![session, user],
            |row| row.get(0),
        ))?;
        if shared {
            return Err(Error::Denied(
                "source disclosure excludes shared draft readers",
            ));
        }
    }
    require_session_principal(connection, session, "user", user)?;
    Ok(())
}

/// Conservative whole-Session read/expansion gate, including inherited
/// history. The indexed local roots are immutable exposure ceilings, not
/// current ACL grants. Check current stable identity at the existing
/// authorization owner; no per-read manifest creation or journal replay.
pub(crate) fn require_session_principal(
    connection: &Connection,
    session: &str,
    kind: &str,
    user: &str,
) -> Result<()> {
    if let Some(ceiling) = session_ceiling(connection, session)? {
        if kind != "user" || ceiling != user {
            return Err(Error::Denied(
                "protected source disclosure is actor-private",
            ));
        }
    }
    Ok(())
}

fn merge_ceiling(current: &mut Option<String>, evidence: &str) -> Result<()> {
    let ceiling: Ceiling = crate::decode(evidence)?;
    crate::stable_user_principal(&ceiling.user)?;
    if current.as_ref().is_some_and(|user| user != &ceiling.user) {
        return Err(Error::Conflict(
            "incompatible protected source disclosure ceilings",
        ));
    }
    *current = Some(ceiling.user);
    Ok(())
}

/// Read the immutable local and carried protected roots. Source-free
/// Stores take one indexed existence check. No manifest writes on reads.
pub(crate) fn session_ceiling(connection: &Connection, session: &str) -> Result<Option<String>> {
    let exists: bool = db(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM disclosure_authority_roots WHERE kind=?1)",
        [KIND],
        |row| row.get(0),
    ))?;
    if !exists {
        return Ok(None);
    }
    let mut ceiling = None;
    let mut query = db(connection.prepare_cached(
        "SELECT evidence_json FROM disclosure_authority_roots
         WHERE kind=?1 AND stable_key=?2 LIMIT 2",
    ))?;
    let mut rows = db(query.query(params![KIND, session]))?;
    while let Some(row) = db(rows.next())? {
        merge_ceiling(&mut ceiling, &db(row.get::<_, String>(0))?)?;
    }

    // Traverse shared DAG nodes once, even across many carried manifests.
    for (_, root_kind, _, evidence) in session_manifest_roots(connection, session)? {
        if matches!(root_kind.as_str(), KIND | CONTEXT_KIND) {
            merge_ceiling(&mut ceiling, &evidence)?;
        }
    }
    Ok(ceiling)
}

fn session_manifest_roots(
    connection: &Connection,
    session: &str,
) -> Result<Vec<(String, String, String, String)>> {
    let mut query = db(connection.prepare_cached(
        "SELECT b.manifest_digest
         FROM agent_state_current a JOIN history_bases b ON b.base_id=a.history_base_id
         WHERE a.session_id=?1
         UNION
         SELECT disclosure_manifest FROM agent_event_inputs
         WHERE recipient_session_id=?1 AND disclosure_manifest IS NOT NULL
         UNION
         SELECT manifest_digest FROM legacy_event_input_compat WHERE session_id=?1
         UNION
         SELECT d.manifest_digest FROM node_context_disclosure d
         JOIN nodes n ON n.id=d.node_id WHERE n.session_id=?1
         LIMIT ?2",
    ))?;
    let manifests = db(db(
        query.query_map(params![session, MAX_MANIFEST_INPUTS + 1], |row| {
            row.get::<_, String>(0)
        }),
    )?
    .collect::<rusqlite::Result<Vec<_>>>())?;
    if manifests.len() > MAX_MANIFEST_INPUTS {
        return Err(Error::Limit("protected inherited history bound exceeded"));
    }
    manifest_authority_roots_from(connection, manifests)
}

pub(crate) fn require_session_reader(
    connection: &Connection,
    session: &str,
    actor: &PrincipalId,
) -> Result<()> {
    if let Some(ceiling) = session_ceiling(connection, session)? {
        if user_for_principal(connection, actor)?.as_deref() != Some(ceiling.as_str()) {
            return Err(Error::Denied(
                "protected source disclosure is actor-private",
            ));
        }
    }
    Ok(())
}

/// The legacy selector identifies local nodes, not inherited base spans.
/// Build a private manual-transfer manifest only for exact local evidence.
/// Session-wide ceilings are conservative; at least one selected node in
/// every protected source Session must actually carry original provenance.
pub(crate) fn context_manifest(
    connection: &Connection,
    actor: &PrincipalId,
    refs: &[aw_agent_core::EvidenceRef],
    target_session: &str,
) -> Result<Option<String>> {
    let mut sessions = BTreeMap::new();
    for reference in refs {
        if !sessions.contains_key(reference.session_id.as_str()) {
            sessions.insert(
                reference.session_id.as_str(),
                session_ceiling(connection, &reference.session_id)?,
            );
        }
    }
    if sessions.values().all(Option::is_none) {
        return Ok(None);
    }
    let user = user_for_principal(connection, actor)?
        .ok_or(Error::Denied("context transfer actor unavailable"))?;
    require_private_audience(connection, target_session, &user)?;
    let workspace = session_in(connection, target_session)?.workspace_id;
    for (session, ceiling) in &sessions {
        if ceiling.as_ref().is_some_and(|ceiling| ceiling != &user)
            || session_in(connection, session)?.workspace_id != workspace
            || crate::session_owner_user(connection, session)? != user
        {
            return Err(Error::Denied(
                "protected context requires the same private actor and Workspace",
            ));
        }
        // The legacy selection only names local nodes. A restored
        // history base with no disclosure dependencies is fine; a
        // protected inherited prefix needs its own exact selector.
        let inherited: bool = db(connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_state_current a
             JOIN history_bases b ON b.base_id=a.history_base_id
             JOIN disclosure_manifest_nodes m ON m.node_digest=b.manifest_digest
             WHERE a.session_id=?1 AND m.item_count<>0)",
            [session],
            |row| row.get(0),
        ))?;
        if inherited {
            return Err(Error::Denied(
                "protected inherited context requires exact history transfer",
            ));
        }
    }
    let mut observed = BTreeSet::new();
    let mut selected = BTreeSet::new();
    let mut entries = BTreeSet::new();
    for reference in refs {
        let manifest = dependency_manifest(connection, &reference.node_id)?;
        let roots = manifest_authority_roots(connection, &manifest)?;
        if roots.iter().any(|(_, kind, _, _)| kind == KIND) {
            observed.insert(reference.session_id.as_str());
        }
        selected.insert(manifest);
        entries.insert(ManifestEntry {
            kind: "fact",
            reference: format!(
                "legacy-node:{}:{}:{}",
                reference.session_id,
                reference.node_id,
                raw_node_digest(connection, &reference.session_id, &reference.node_id)?
            ),
        });
    }
    if sessions
        .iter()
        .any(|(session, ceiling)| ceiling.is_some() && !observed.contains(session))
    {
        return Err(Error::Denied(
            "selected context lacks exact protected provenance",
        ));
    }
    let root = authority_root(
        connection,
        CONTEXT_KIND,
        &user,
        &json(&Ceiling { user: user.clone() })?,
    )?;
    entries.insert(ManifestEntry {
        kind: "authority_root",
        reference: root,
    });
    let mut manifest = manifest_from_entries(connection, &entries)?;
    for next in selected {
        manifest = union_manifests(connection, &manifest, &next)?;
    }
    validate_manifest_expansion(connection, actor, &manifest, "user", &user)?;
    Ok(Some(manifest))
}

pub(crate) fn validate_context_manifest(
    connection: &Connection,
    actor: &PrincipalId,
    manifest: &str,
) -> Result<()> {
    let user = user_for_principal(connection, actor)?
        .ok_or(Error::Denied("context transfer actor unavailable"))?;
    let roots = manifest_authority_roots(connection, manifest)?;
    if !roots.iter().any(|(_, kind, _, _)| kind == CONTEXT_KIND)
        || !roots.iter().any(|(_, kind, _, _)| kind == KIND)
    {
        return Err(Error::Storage(
            "protected context manifest lacks original authority",
        ));
    }
    validate_manifest_expansion(connection, actor, manifest, "user", &user)
}

/// Hold model/provider admission for manual-only context, including
/// inherited and Event-carried manifests. No new provider release policy
/// is inferred from the already-authorized local actor read.
pub(crate) fn require_context_dispatch(connection: &Connection, session: &str) -> Result<()> {
    let exists: bool = db(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM disclosure_authority_roots WHERE kind=?1)",
        [CONTEXT_KIND],
        |row| row.get(0),
    ))?;
    if exists
        && session_manifest_roots(connection, session)?
            .iter()
            .any(|(_, kind, _, _)| kind == CONTEXT_KIND)
    {
        return Err(Error::Denied(
            "protected manual context requires independent provider disclosure authority",
        ));
    }
    Ok(())
}

/// Preserve exact custody provenance of the run's potentially exposed
/// source set, even if output/terminal is lost or the claim was no-start.
pub(super) fn add_history_claims(
    connection: &Connection,
    head: &str,
    mut manifest: String,
) -> Result<String> {
    for claimed in history_disclosure_manifests(connection, head)? {
        manifest = union_manifests(connection, &manifest, &claimed)?;
    }
    Ok(manifest)
}
