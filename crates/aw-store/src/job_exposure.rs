//! Durable mixed Job exposure ordering over the existing immutable commit log.
//!
//! Agent roots reuse their authoritative Session/branch creation sequence.
//! Finite Jobs reserve one common list position before effects, then publish or
//! release that exact reservation. This module stores routing/list evidence only;
//! Agent lifecycle/history stay in SessionStore and finite lifecycle stays in
//! JobManager.

use crate::store_scope::StorePos;
use crate::{
    agent_binding_in, db, durability, workspace_writer, AgentBindingInfo, Error, PrincipalId,
    Result, SessionStore,
};
use aw_tools::execution_ref::{ExecutionRef, ExecutionSegment};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

const ENTITY: &str = "job_exposure";
const RESERVED: &str = "job.exposure.reserved";
const PUBLISHED: &str = "job.exposure.published";
const RELEASED: &str = "job.exposure.released";
const MAX_EXPOSURES: usize = 8_192;

#[derive(Clone, Debug, PartialEq, Eq)]
struct AgentExposureSeed {
    canonical_ref: String,
    domain_handle: String,
    identity_handle: String,
    project_handle: String,
    session_id: String,
    branch_id: String,
    position: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FiniteExposureState {
    Reserved,
    Published,
    Released,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FiniteExposure {
    pub canonical_ref: String,
    pub position: u64,
    pub state: FiniteExposureState,
    pub generation: String,
    pub domain_handle: String,
    pub identity_handle: String,
    pub project_handle: String,
    pub session_id: String,
    pub branch_id: String,
    pub writer_epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FiniteExposureProvenance {
    pub canonical_job_ref: String,
    pub original_agent_ref: String,
    pub original_turn: u64,
    pub initiating_actor: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobExposureListItem {
    pub canonical_ref: String,
    pub position: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    generation: String,
    canonical_ref: String,
    domain_handle: String,
    identity_handle: String,
    project_handle: String,
    session_id: String,
    branch_id: String,
    writer_epoch: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provenance: Option<FiniteExposureProvenance>,
    #[serde(default)]
    reservation_position: Option<u64>,
}

fn validate_generation(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value.is_ascii()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
    {
        return Err(Error::Denied("invalid finite Job exposure generation"));
    }
    Ok(())
}

fn agent_ref(
    domain_handle: &str,
    identity_handle: &str,
    project_handle: &str,
    agent_handle: &str,
) -> String {
    format!("/d:{domain_handle}/i:{identity_handle}/p:{project_handle}/a:{agent_handle}")
}

fn root_ref(binding: &AgentBindingInfo) -> String {
    agent_ref(
        &binding.domain_handle,
        &binding.identity_handle,
        &binding.project_handle,
        &binding.agent_handle,
    )
}

fn validate_ref(binding: &AgentBindingInfo, canonical_ref: &str) -> Result<()> {
    let parsed = ExecutionRef::parse(canonical_ref)
        .map_err(|_| Error::Denied("invalid finite Job exposure ref"))?;
    if !parsed.is_canonical() {
        return Err(Error::Denied("finite Job exposure ref must be canonical"));
    }
    let resolved = parsed
        .resolve(None, None)
        .map_err(|_| Error::Denied("invalid canonical finite Job exposure ref"))?;
    let agent = resolved.agent_ref();
    if agent.domain.as_str() != binding.domain_handle
        || agent.identity.as_str() != binding.identity_handle
        || agent.project.as_str() != binding.project_handle
        || agent.agent.as_str() != binding.agent_handle
        || !matches!(
            resolved.suffix(),
            [ExecutionSegment::Turn(_), ExecutionSegment::Job(_)]
        )
    {
        return Err(Error::Denied(
            "finite Job exposure ref is outside the exact authoritative Agent Turn/Job owner",
        ));
    }
    Ok(())
}

fn provenance_from_ref(
    canonical_ref: &str,
    initiating_actor: &str,
) -> Result<FiniteExposureProvenance> {
    let parsed = ExecutionRef::parse(canonical_ref)
        .map_err(|_| Error::Denied("invalid finite Job provenance ref"))?;
    let resolved = parsed
        .resolve(None, None)
        .map_err(|_| Error::Denied("invalid canonical finite Job provenance ref"))?;
    let [ExecutionSegment::Turn(turn), ExecutionSegment::Job(_)] = resolved.suffix() else {
        return Err(Error::Denied(
            "finite Job provenance requires exact Agent/Turn/Job ref",
        ));
    };
    let original_turn = turn
        .as_str()
        .parse::<u64>()
        .ok()
        .filter(|turn| *turn > 0)
        .ok_or(Error::Denied("finite Job provenance Turn is invalid"))?;
    if initiating_actor.is_empty()
        || initiating_actor.len() > 256
        || initiating_actor.chars().any(char::is_control)
    {
        return Err(Error::Denied("finite Job initiating actor is invalid"));
    }
    Ok(FiniteExposureProvenance {
        canonical_job_ref: canonical_ref.to_owned(),
        original_agent_ref: resolved.agent_ref().canonical(),
        original_turn,
        initiating_actor: initiating_actor.to_owned(),
    })
}

fn entity_key(generation: &str, canonical_ref: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"aw-job-exposure-entity-v1\0");
    hash.update((generation.len() as u64).to_be_bytes());
    hash.update(generation.as_bytes());
    hash.update((canonical_ref.len() as u64).to_be_bytes());
    hash.update(canonical_ref.as_bytes());
    format!("jx_{:x}", hash.finalize())
}

fn kind_state(kind: &str) -> Result<FiniteExposureState> {
    match kind {
        RESERVED => Ok(FiniteExposureState::Reserved),
        PUBLISHED => Ok(FiniteExposureState::Published),
        RELEASED => Ok(FiniteExposureState::Released),
        _ => Err(Error::Storage("invalid finite Job exposure event kind")),
    }
}

fn projection_state(state: &str) -> Result<FiniteExposureState> {
    match state {
        "reserved" => Ok(FiniteExposureState::Reserved),
        "published" => Ok(FiniteExposureState::Published),
        "released" => Ok(FiniteExposureState::Released),
        _ => Err(Error::Storage(
            "invalid finite Job exposure projection state",
        )),
    }
}

fn project_agent(
    c: &rusqlite::Connection,
    session_id: &str,
    branch_id: &str,
    position: StorePos,
) -> Result<()> {
    let binding = crate::legacy_agent_binding_unchecked(c, session_id, branch_id)?;
    insert_agent_projection(
        c,
        &AgentExposureSeed {
            canonical_ref: root_ref(&binding),
            domain_handle: binding.domain_handle,
            identity_handle: binding.identity_handle,
            project_handle: binding.project_handle,
            session_id: binding.session_id,
            branch_id: binding.branch_id,
            position: position.get(),
        },
    )
}

fn insert_agent_projection(c: &rusqlite::Connection, seed: &AgentExposureSeed) -> Result<()> {
    let changed = db(c.execute(
        "INSERT INTO job_exposure_current(
             kind,generation,canonical_ref,domain_handle,identity_handle,project_handle,
             session_id,branch_id,position,state,writer_epoch
         ) VALUES('agent','',?1,?2,?3,?4,?5,?6,?7,'published',NULL)
         ON CONFLICT(kind,generation,canonical_ref) DO UPDATE SET
             canonical_ref=excluded.canonical_ref
         WHERE job_exposure_current.session_id=excluded.session_id
           AND job_exposure_current.branch_id=excluded.branch_id
           AND job_exposure_current.position=excluded.position
           AND job_exposure_current.domain_handle=excluded.domain_handle
           AND job_exposure_current.identity_handle=excluded.identity_handle
           AND job_exposure_current.project_handle=excluded.project_handle",
        params![
            seed.canonical_ref,
            seed.domain_handle,
            seed.identity_handle,
            seed.project_handle,
            seed.session_id,
            seed.branch_id,
            seed.position,
        ],
    ))?;
    if changed != 1 {
        return Err(Error::Storage(
            "Agent exposure projection conflicts with immutable creation evidence",
        ));
    }
    Ok(())
}

fn project_finite(
    c: &rusqlite::Connection,
    kind: &str,
    seq: StorePos,
    payload: &Payload,
) -> Result<()> {
    let state = kind_state(kind)?;
    let position = match state {
        FiniteExposureState::Reserved => seq,
        FiniteExposureState::Published | FiniteExposureState::Released => {
            StorePos::new(payload.reservation_position.ok_or(Error::Storage(
                "finite Job exposure closure lacks reservation position",
            ))?)
        }
    };
    let state = match state {
        FiniteExposureState::Reserved => "reserved",
        FiniteExposureState::Published => "published",
        FiniteExposureState::Released => "released",
    };
    let changed = db(c.execute(
        "INSERT INTO job_exposure_current(
             kind,generation,canonical_ref,domain_handle,identity_handle,project_handle,
             session_id,branch_id,position,state,writer_epoch
         ) VALUES('finite',?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
         ON CONFLICT(kind,generation,canonical_ref) DO UPDATE SET state=excluded.state
         WHERE job_exposure_current.domain_handle=excluded.domain_handle
           AND job_exposure_current.identity_handle=excluded.identity_handle
           AND job_exposure_current.project_handle=excluded.project_handle
           AND job_exposure_current.session_id=excluded.session_id
           AND job_exposure_current.branch_id=excluded.branch_id
           AND job_exposure_current.position=excluded.position
           AND job_exposure_current.writer_epoch=excluded.writer_epoch",
        params![
            payload.generation,
            payload.canonical_ref,
            payload.domain_handle,
            payload.identity_handle,
            payload.project_handle,
            payload.session_id,
            payload.branch_id,
            position.get(),
            state,
            payload.writer_epoch,
        ],
    ))?;
    if changed != 1 {
        return Err(Error::Storage(
            "finite Job exposure projection conflicts with immutable reservation evidence",
        ));
    }
    Ok(())
}

pub(crate) fn publish_agent_branch_created(
    c: &rusqlite::Connection,
    txid: &str,
    actor: Option<&PrincipalId>,
    session_id: &str,
    branch_id: &str,
    head: &str,
    name: &str,
) -> Result<()> {
    durability::commit_event(
        c,
        txid,
        "branch.created",
        "branch",
        branch_id,
        actor,
        serde_json::json!({"session_id":session_id,"head":head,"name":name}),
    )
}

pub(crate) fn repair_missing_agent_exposures(c: &rusqlite::Connection) -> Result<usize> {
    // Pre-v38 migration repair only. Once agent_state_current is activated,
    // authoritative rebuilds must use normalize_agent_exposures() below.
    let mut query = db(c.prepare(
        "SELECT b.session_id,b.id,b.head,b.name,s.owner
         FROM branches b JOIN sessions s ON s.id=b.session_id
         LEFT JOIN job_exposure_current e
           ON e.kind='agent' AND e.generation=''
          AND e.session_id=b.session_id AND e.branch_id=b.id
         WHERE e.canonical_ref IS NULL
         ORDER BY b.created_at,b.session_id,b.id",
    ))?;
    let rows = db(query.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    }))?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(|_| Error::Storage("cannot enumerate missing Agent exposure bindings"))?;
    if rows.is_empty() {
        return Ok(0);
    }
    let txid = durability::tx_id()?;
    for (session_id, branch_id, head, name, owner) in &rows {
        let actor = PrincipalId::new(owner.clone())?;
        publish_agent_branch_created(c, &txid, Some(&actor), session_id, branch_id, head, name)?;
    }
    Ok(rows.len())
}

fn authoritative_agent_exposure_seeds(c: &rusqlite::Connection) -> Result<Vec<AgentExposureSeed>> {
    let mut query = db(c.prepare(
        "SELECT
             '/d:' || a.domain_handle || '/i:' || a.identity_handle ||
             '/p:' || a.project_handle || '/a:' || a.agent_handle,
             a.domain_handle,a.identity_handle,a.project_handle,
             a.session_id,a.branch_id,
             (
                 SELECT MIN(e.seq)
                 FROM commit_log e
                 WHERE (
                     e.kind='session.created'
                     AND e.entity_type='session'
                     AND e.entity_id=a.session_id
                     AND json_extract(e.payload_json,'$.branch_id')=a.branch_id
                 ) OR (
                     e.kind='branch.created'
                     AND e.entity_type='branch'
                     AND e.entity_id=a.branch_id
                     AND json_extract(e.payload_json,'$.session_id')=a.session_id
                 )
             )
         FROM agent_state_current a
         ORDER BY a.domain_handle,a.identity_handle,a.project_handle,a.agent_handle",
    ))?;
    let rows = db(query.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<u64>>(6)?,
        ))
    }))?;
    rows.map(|row| {
        let (canonical_ref, domain, identity, project, session, branch, position) = db(row)?;
        Ok(AgentExposureSeed {
            canonical_ref,
            domain_handle: domain,
            identity_handle: identity,
            project_handle: project,
            session_id: session,
            branch_id: branch,
            position: position.ok_or(Error::Storage(
                "authoritative Agent lacks immutable creation position",
            ))?,
        })
    })
    .collect()
}

fn projected_agent_exposure_seeds(c: &rusqlite::Connection) -> Result<Vec<AgentExposureSeed>> {
    let mut query = db(c.prepare(
        "SELECT canonical_ref,domain_handle,identity_handle,project_handle,
                session_id,branch_id,position
         FROM job_exposure_current
         WHERE kind='agent' AND generation='' AND state='published' AND writer_epoch IS NULL
         ORDER BY domain_handle,identity_handle,project_handle,canonical_ref",
    ))?;
    let rows = db(query.query_map([], |row| {
        Ok(AgentExposureSeed {
            canonical_ref: row.get(0)?,
            domain_handle: row.get(1)?,
            identity_handle: row.get(2)?,
            project_handle: row.get(3)?,
            session_id: row.get(4)?,
            branch_id: row.get(5)?,
            position: row.get(6)?,
        })
    }))?;
    db(rows.collect::<rusqlite::Result<Vec<_>>>())
}

/// Owner-lock/open-time self-heal for the rebuildable Agent exposure index.
///
/// agent_state_current is the sole Agent-envelope authority after v38.
/// commit_log contributes only the immutable original publication position.
/// This emits no event/audit and never mutates Agent owner state.
pub(crate) fn normalize_agent_exposures(c: &mut rusqlite::Connection) -> Result<usize> {
    let expected = authoritative_agent_exposure_seeds(c)?;
    if projected_agent_exposure_seeds(c)? == expected {
        return Ok(0);
    }

    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let expected = authoritative_agent_exposure_seeds(&tx)?;
    db(tx.execute(
        "DELETE FROM job_exposure_current WHERE kind='agent' AND generation=''",
        [],
    ))?;
    for seed in &expected {
        insert_agent_projection(&tx, seed)?;
    }
    if projected_agent_exposure_seeds(&tx)? != expected {
        return Err(Error::Storage(
            "authoritative Agent exposure rebuild did not converge",
        ));
    }
    db(tx.commit())?;
    Ok(expected.len())
}

pub(crate) fn project_commit_event(
    c: &rusqlite::Connection,
    kind: &str,
    entity_type: &str,
    entity_id: &str,
    payload: &Value,
    position: StorePos,
) -> Result<()> {
    match (kind, entity_type) {
        ("session.created", "session") => {
            let branch = payload
                .get("branch_id")
                .and_then(Value::as_str)
                .ok_or(Error::Storage(
                    "Session creation event lacks Agent branch identity",
                ))?;
            project_agent(c, entity_id, branch, position)
        }
        ("branch.created", "branch") => {
            let session =
                payload
                    .get("session_id")
                    .and_then(Value::as_str)
                    .ok_or(Error::Storage(
                        "branch creation event lacks Agent Session identity",
                    ))?;
            project_agent(c, session, entity_id, position)
        }
        (RESERVED | PUBLISHED | RELEASED, ENTITY) => {
            let payload: Payload = serde_json::from_value(payload.clone())
                .map_err(|_| Error::Storage("invalid finite Job exposure event payload"))?;
            project_finite(c, kind, position, &payload)
        }
        _ => Ok(()),
    }
}

pub(crate) fn backfill_current(c: &rusqlite::Connection) -> Result<()> {
    let mut query = db(c.prepare(
        "SELECT seq,kind,entity_type,entity_id,payload_json
         FROM commit_log
         WHERE kind IN ('session.created','branch.created',?1,?2,?3)
         ORDER BY seq",
    ))?;
    let rows = db(
        query.query_map(params![RESERVED, PUBLISHED, RELEASED], |row| {
            Ok((
                row.get::<_, u64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        }),
    )?;
    for row in rows {
        let (position, kind, entity_type, entity_id, encoded) = db(row)?;
        let payload: Value = crate::decode(&encoded)?;
        project_commit_event(
            c,
            &kind,
            &entity_type,
            &entity_id,
            &payload,
            StorePos::new(position),
        )?;
    }
    Ok(())
}

fn latest(
    c: &rusqlite::Connection,
    generation: &str,
    canonical_ref: &str,
) -> Result<Option<FiniteExposure>> {
    let row: Option<(
        String,
        u64,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        u64,
    )> = db(c
        .query_row(
            "SELECT canonical_ref,position,state,generation,domain_handle,identity_handle,
                    project_handle,session_id,branch_id,writer_epoch
             FROM job_exposure_current
             WHERE kind='finite' AND generation=?1 AND canonical_ref=?2",
            params![generation, canonical_ref],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                ))
            },
        )
        .optional())?;
    row.map(
        |(
            canonical_ref,
            position,
            state,
            generation,
            domain_handle,
            identity_handle,
            project_handle,
            session_id,
            branch_id,
            writer_epoch,
        )| {
            Ok(FiniteExposure {
                canonical_ref,
                position,
                state: projection_state(&state)?,
                generation,
                domain_handle,
                identity_handle,
                project_handle,
                session_id,
                branch_id,
                writer_epoch,
            })
        },
    )
    .transpose()
}

fn authoritative_binding(
    c: &rusqlite::Connection,
    actor: &PrincipalId,
    binding: &AgentBindingInfo,
) -> Result<AgentBindingInfo> {
    let actual = agent_binding_in(c, actor, &binding.session_id, &binding.branch_id)?;
    if &actual != binding {
        return Err(Error::Denied(
            "finite Job exposure Agent binding changed before publication",
        ));
    }
    Ok(actual)
}

fn reservation_provenance(
    c: &rusqlite::Connection,
    generation: &str,
    canonical_ref: &str,
) -> Result<FiniteExposureProvenance> {
    let key = entity_key(generation, canonical_ref);
    let (actor, payload_json): (Option<String>, String) = db(c.query_row(
        "SELECT actor,payload_json FROM commit_log
         WHERE kind=?1 AND entity_type=?2 AND entity_id=?3
         ORDER BY seq LIMIT 1",
        params![RESERVED, ENTITY, key],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    let actor = actor.ok_or(Error::Storage(
        "finite Job exposure reservation lacks initiating actor",
    ))?;
    let payload: Payload = crate::decode(&payload_json)?;
    if payload.generation != generation || payload.canonical_ref != canonical_ref {
        return Err(Error::Storage(
            "finite Job exposure reservation identity changed",
        ));
    }
    let derived = provenance_from_ref(canonical_ref, &actor)?;
    if let Some(stored) = payload.provenance {
        if stored != derived {
            return Err(Error::Storage(
                "finite Job exposure reservation provenance changed",
            ));
        }
    }
    Ok(derived)
}

fn payload(
    binding: &AgentBindingInfo,
    generation: &str,
    canonical_ref: &str,
    writer_epoch: u64,
    provenance: Option<FiniteExposureProvenance>,
    reservation_position: Option<u64>,
) -> Payload {
    Payload {
        generation: generation.to_owned(),
        canonical_ref: canonical_ref.to_owned(),
        domain_handle: binding.domain_handle.clone(),
        identity_handle: binding.identity_handle.clone(),
        project_handle: binding.project_handle.clone(),
        session_id: binding.session_id.clone(),
        branch_id: binding.branch_id.clone(),
        writer_epoch,
        provenance,
        reservation_position,
    }
}

fn visible_projection_page(
    c: &rusqlite::Connection,
    actor: &PrincipalId,
    binding: &AgentBindingInfo,
    kind: &str,
    generation: &str,
    before: Option<u64>,
    limit: usize,
) -> Result<Vec<JobExposureListItem>> {
    let actor_user = crate::user_for_principal(c, actor)?;
    let mut scan_before = before;
    let batch = limit.clamp(32, 257);
    let mut out = Vec::with_capacity(limit);
    while out.len() < limit {
        let mut query = db(c.prepare(
            "SELECT e.canonical_ref,e.position,e.session_id,e.branch_id
             FROM job_exposure_current e
             JOIN agent_state_current a
               ON a.session_id=e.session_id AND a.branch_id=e.branch_id
             WHERE e.domain_handle=?1
               AND e.identity_handle=?2
               AND e.project_handle=?3
               AND e.kind=?4 AND e.generation=?5 AND e.state='published'
               AND (?6 IS NULL OR e.position<?6)
               AND ?7 IS NOT NULL
               AND (
                    (a.owner_kind='user' AND a.owner_handle=?7)
                    OR (a.owner_kind='group' AND EXISTS(
                        SELECT 1
                        FROM principal_group_members m
                        JOIN principal_groups g
                          ON g.domain_id=m.domain_id AND g.principal_handle=m.group_handle
                         AND g.deleted_at IS NULL
                        JOIN domain_principals dp
                          ON dp.domain_id=m.domain_id AND dp.principal_handle=m.group_handle
                         AND dp.kind='group' AND dp.status='active'
                        WHERE m.domain_id=a.domain_handle AND m.group_handle=a.owner_handle
                          AND m.member_handle=?7 AND m.state='active'
                    ))
                    OR EXISTS(
                        SELECT 1 FROM agent_acl_current p
                        WHERE p.session_id=e.session_id AND p.branch_id=e.branch_id
                          AND p.principal_kind='user' AND p.principal_handle=?7
                          AND p.can_view=1
                    )
                    OR EXISTS(
                        SELECT 1
                        FROM agent_acl_current p
                        JOIN principal_group_members m
                          ON m.domain_id=a.domain_handle AND m.group_handle=p.principal_handle
                         AND m.member_handle=?7 AND m.state='active'
                        JOIN principal_groups g
                          ON g.domain_id=m.domain_id AND g.principal_handle=m.group_handle
                         AND g.deleted_at IS NULL
                        JOIN domain_principals dp
                          ON dp.domain_id=m.domain_id AND dp.principal_handle=m.group_handle
                         AND dp.kind='group' AND dp.status='active'
                        WHERE p.session_id=e.session_id AND p.branch_id=e.branch_id
                          AND p.principal_kind='group' AND p.can_view=1
                    )
               )
             ORDER BY e.position DESC,e.canonical_ref
             LIMIT ?8",
        ))?;
        let scan_before_i64 = scan_before
            .map(i64::try_from)
            .transpose()
            .map_err(|_| Error::Denied("invalid mixed Job exposure page cursor"))?;
        let rows = db(query.query_map(
            params![
                binding.domain_handle,
                binding.identity_handle,
                binding.project_handle,
                kind,
                generation,
                scan_before_i64,
                actor_user,
                batch as i64,
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        ))?;
        let mut fetched = 0usize;
        let mut last_position = None;
        for row in rows {
            let (canonical_ref, position, session_id, branch_id) = db(row)?;
            fetched += 1;
            last_position = Some(position);
            match crate::agent_owner::authorize_access(
                c,
                actor,
                crate::agent_owner::AgentKey::new(&session_id, &branch_id),
                crate::SessionParticipationAction::View,
            ) {
                Ok(_) => out.push(JobExposureListItem {
                    canonical_ref,
                    position,
                }),
                Err(Error::Denied(_)) => {}
                Err(error) => return Err(error),
            }
            if out.len() == limit {
                break;
            }
        }
        if out.len() == limit || fetched < batch {
            break;
        }
        let Some(position) = last_position else {
            break;
        };
        scan_before = Some(position);
    }
    Ok(out)
}

impl SessionStore {
    pub fn agent_binding_by_handles(
        &self,
        actor: &PrincipalId,
        domain_handle: &str,
        identity_handle: &str,
        project_handle: &str,
        agent_handle: &str,
    ) -> Result<AgentBindingInfo> {
        for value in [domain_handle, identity_handle, project_handle, agent_handle] {
            crate::validate_agent_handle_component(value)?;
        }
        let canonical_ref = agent_ref(domain_handle, identity_handle, project_handle, agent_handle);
        let connection = self.connection()?;
        let candidate: Option<(String, String)> = db(connection
            .query_row(
                "SELECT session_id,branch_id
                 FROM job_exposure_current
                 WHERE kind='agent' AND generation='' AND canonical_ref=?1
                   AND state='published'",
                [&canonical_ref],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional())?;
        let (session_id, branch_id) =
            candidate.ok_or(Error::Denied("Agent ref is unavailable or not authorized"))?;
        let binding = crate::agent_binding_in(&connection, actor, &session_id, &branch_id)?;
        if binding.domain_handle != domain_handle
            || binding.identity_handle != identity_handle
            || binding.project_handle != project_handle
            || binding.agent_handle != agent_handle
        {
            return Err(Error::Denied("Agent ref is unavailable or not authorized"));
        }
        Ok(binding)
    }

    pub fn job_exposure_page(
        &self,
        actor: &PrincipalId,
        binding: &AgentBindingInfo,
        generation: &str,
        before: Option<u64>,
        limit: usize,
    ) -> Result<Vec<JobExposureListItem>> {
        validate_generation(generation)?;
        if limit == 0 || limit > 257 {
            return Err(Error::Limit("invalid mixed Job exposure page limit"));
        }
        if before == Some(0) {
            return Err(Error::Denied("invalid mixed Job exposure page cursor"));
        }
        let connection = self.connection()?;
        let actual = authoritative_binding(&connection, actor, binding)?;
        let mut rows =
            visible_projection_page(&connection, actor, &actual, "agent", "", before, limit)?;
        rows.extend(visible_projection_page(
            &connection,
            actor,
            &actual,
            "finite",
            generation,
            before,
            limit,
        )?);
        rows.sort_by(|a, b| {
            b.position
                .cmp(&a.position)
                .then_with(|| a.canonical_ref.cmp(&b.canonical_ref))
        });
        rows.truncate(limit);
        Ok(rows)
    }

    pub fn finite_job_exposure_reserve(
        &self,
        actor: &PrincipalId,
        binding: &AgentBindingInfo,
        generation: &str,
        canonical_ref: &str,
        capacity: usize,
    ) -> Result<FiniteExposure> {
        validate_generation(generation)?;
        if capacity == 0 || capacity > MAX_EXPOSURES {
            return Err(Error::Limit("invalid finite Job exposure capacity"));
        }
        validate_ref(binding, canonical_ref)?;
        let key = entity_key(generation, canonical_ref);
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let actual = authoritative_binding(&tx, actor, binding)?;
        let writer =
            workspace_writer::require_workspace_writer_for_session(&tx, &actual.session_id)?;

        if let Some(existing) = latest(&tx, generation, canonical_ref)? {
            if existing.domain_handle != actual.domain_handle
                || existing.identity_handle != actual.identity_handle
                || existing.project_handle != actual.project_handle
                || existing.session_id != actual.session_id
                || existing.branch_id != actual.branch_id
            {
                return Err(Error::Denied(
                    "finite Job exposure identity was reused across owners",
                ));
            }
            let provenance = reservation_provenance(&tx, generation, canonical_ref)?;
            if provenance.initiating_actor != actor.as_str() {
                return Err(Error::Denied(
                    "finite Job exposure initiating actor cannot be rewritten",
                ));
            }
            return match existing.state {
                FiniteExposureState::Reserved | FiniteExposureState::Published => Ok(existing),
                FiniteExposureState::Released => Err(Error::Conflict(
                    "released finite Job exposure ref cannot be reserved again",
                )),
            };
        }

        let active: u64 = db(tx.query_row(
            "SELECT COUNT(*) FROM job_exposure_current
             WHERE kind='finite' AND generation=?1
               AND domain_handle=?2 AND identity_handle=?3 AND project_handle=?4
               AND state IN ('reserved','published')",
            params![
                generation,
                actual.domain_handle,
                actual.identity_handle,
                actual.project_handle,
            ],
            |row| row.get(0),
        ))?;
        if active >= capacity as u64 {
            return Err(Error::Limit(
                "project finite Job exposure recoverability capacity exhausted",
            ));
        }

        let txid = durability::tx_id()?;
        let provenance = provenance_from_ref(canonical_ref, actor.as_str())?;
        let encoded = serde_json::to_value(payload(
            &actual,
            generation,
            canonical_ref,
            writer.writer_epoch,
            Some(provenance),
            None,
        ))
        .map_err(|_| Error::Storage("cannot encode finite Job exposure reservation"))?;
        let position = durability::commit_event_position(
            &tx,
            &txid,
            RESERVED,
            ENTITY,
            &key,
            Some(actor),
            encoded,
        )?;
        db(tx.commit())?;
        Ok(FiniteExposure {
            canonical_ref: canonical_ref.to_owned(),
            position: position.get(),
            state: FiniteExposureState::Reserved,
            generation: generation.to_owned(),
            domain_handle: actual.domain_handle,
            identity_handle: actual.identity_handle,
            project_handle: actual.project_handle,
            session_id: actual.session_id,
            branch_id: actual.branch_id,
            writer_epoch: writer.writer_epoch,
        })
    }

    pub fn finite_job_exposure_publish(
        &self,
        actor: &PrincipalId,
        binding: &AgentBindingInfo,
        generation: &str,
        canonical_ref: &str,
    ) -> Result<FiniteExposure> {
        validate_generation(generation)?;
        validate_ref(binding, canonical_ref)?;
        let key = entity_key(generation, canonical_ref);
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let actual = authoritative_binding(&tx, actor, binding)?;
        let writer =
            workspace_writer::require_workspace_writer_for_session(&tx, &actual.session_id)?;
        let existing = latest(&tx, generation, canonical_ref)?
            .ok_or(Error::Denied("finite Job exposure reservation is missing"))?;
        if existing.session_id != actual.session_id
            || existing.branch_id != actual.branch_id
            || existing.writer_epoch != writer.writer_epoch
        {
            return Err(Error::Denied(
                "finite Job exposure writer/owner changed before publication",
            ));
        }
        match existing.state {
            FiniteExposureState::Published => return Ok(existing),
            FiniteExposureState::Released => {
                return Err(Error::Conflict(
                    "released finite Job exposure cannot be published",
                ))
            }
            FiniteExposureState::Reserved => {}
        }
        let provenance = reservation_provenance(&tx, generation, canonical_ref)?;
        if provenance.initiating_actor != actor.as_str() {
            return Err(Error::Denied(
                "finite Job exposure initiating actor changed before publication",
            ));
        }

        let txid = durability::tx_id()?;
        let encoded = serde_json::to_value(payload(
            &actual,
            generation,
            canonical_ref,
            existing.writer_epoch,
            Some(provenance),
            Some(existing.position),
        ))
        .map_err(|_| Error::Storage("cannot encode finite Job exposure publication"))?;
        durability::commit_event(&tx, &txid, PUBLISHED, ENTITY, &key, Some(actor), encoded)?;
        db(tx.commit())?;
        Ok(FiniteExposure {
            state: FiniteExposureState::Published,
            ..existing
        })
    }

    pub fn finite_job_exposure_release_unpublished(
        &self,
        actor: &PrincipalId,
        binding: &AgentBindingInfo,
        generation: &str,
        canonical_ref: &str,
    ) -> Result<()> {
        validate_generation(generation)?;
        validate_ref(binding, canonical_ref)?;
        let key = entity_key(generation, canonical_ref);
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let actual = authoritative_binding(&tx, actor, binding)?;
        let existing = match latest(&tx, generation, canonical_ref)? {
            None => {
                db(tx.commit())?;
                return Ok(());
            }
            Some(existing) => existing,
        };
        match existing.state {
            FiniteExposureState::Released => {
                db(tx.commit())?;
                return Ok(());
            }
            FiniteExposureState::Published => {
                return Err(Error::Conflict(
                    "published finite Job exposure cannot be released as hidden",
                ))
            }
            FiniteExposureState::Reserved => {}
        }
        if existing.session_id != actual.session_id || existing.branch_id != actual.branch_id {
            return Err(Error::Denied("finite Job exposure release owner changed"));
        }
        let provenance = reservation_provenance(&tx, generation, canonical_ref)?;
        if provenance.initiating_actor != actor.as_str() {
            return Err(Error::Denied(
                "finite Job exposure initiating actor changed before release",
            ));
        }
        let txid = durability::tx_id()?;
        let encoded = serde_json::to_value(payload(
            &actual,
            generation,
            canonical_ref,
            existing.writer_epoch,
            Some(provenance),
            Some(existing.position),
        ))
        .map_err(|_| Error::Storage("cannot encode finite Job exposure release"))?;
        durability::commit_event(&tx, &txid, RELEASED, ENTITY, &key, Some(actor), encoded)?;
        db(tx.commit())?;
        Ok(())
    }

    pub fn finite_job_exposures(
        &self,
        actor: &PrincipalId,
        binding: &AgentBindingInfo,
        generation: &str,
        limit: usize,
    ) -> Result<Vec<FiniteExposure>> {
        validate_generation(generation)?;
        if limit == 0 || limit > MAX_EXPOSURES {
            return Err(Error::Limit("invalid finite Job exposure list limit"));
        }
        let connection = self.connection()?;
        let actual = authoritative_binding(&connection, actor, binding)?;
        let mut query = db(connection.prepare(
            "SELECT canonical_ref,position,state,generation,domain_handle,identity_handle,
                    project_handle,session_id,branch_id,writer_epoch
             FROM job_exposure_current
             WHERE kind='finite' AND generation=?1
               AND domain_handle=?2 AND identity_handle=?3 AND project_handle=?4
               AND state='published'
             ORDER BY position DESC,canonical_ref ASC
             LIMIT ?5",
        ))?;
        let rows = db(query.query_map(
            params![
                generation,
                actual.domain_handle,
                actual.identity_handle,
                actual.project_handle,
                limit as i64,
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, u64>(9)?,
                ))
            },
        ))?;
        let mut result = Vec::new();
        for row in rows {
            let (
                canonical_ref,
                position,
                state,
                generation,
                domain_handle,
                identity_handle,
                project_handle,
                session_id,
                branch_id,
                writer_epoch,
            ) = db(row)?;
            result.push(FiniteExposure {
                canonical_ref,
                position,
                state: projection_state(&state)?,
                generation,
                domain_handle,
                identity_handle,
                project_handle,
                session_id,
                branch_id,
                writer_epoch,
            });
        }
        Ok(result)
    }

    pub fn agent_job_exposure_position(
        &self,
        actor: &PrincipalId,
        binding: &AgentBindingInfo,
    ) -> Result<u64> {
        let connection = self.connection()?;
        let actual = authoritative_binding(&connection, actor, binding)?;
        let position: Option<u64> = db(connection
            .query_row(
                "SELECT position FROM job_exposure_current
                 WHERE kind='agent' AND generation='' AND state='published'
                   AND session_id=?1 AND branch_id=?2",
                params![actual.session_id, actual.branch_id],
                |row| row.get(0),
            )
            .optional())?;
        position.ok_or(Error::Storage(
            "Agent creation exposure position is unavailable",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentAdmission, SessionBinding};
    use aw_store_api::SessionBackend;
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
    fn authoritative_agent_exposure_rebuild_preserves_owner_state_and_publication_order() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("internal:fixture:agent-exposure-rebuild").unwrap();
        let (first_session, first_branch) = store.create(&actor, "first", binding()).unwrap();
        let first_binding = store
            .agent_binding(&actor, &first_session.id, &first_branch.id)
            .unwrap();
        let first_ref = root_ref(&first_binding);
        let first_position = store
            .agent_job_exposure_position(&actor, &first_binding)
            .unwrap();

        let first = store.agent_get(&actor, &first_ref).unwrap();
        let edited = store
            .agent_edit(&actor, &first_ref, first.rev, "renamed first")
            .unwrap();
        let held = store
            .agent_admission(&actor, &first_ref, AgentAdmission::Held, Some(edited.rev))
            .unwrap();

        let (second_session, second_branch) = store.create(&actor, "second", binding()).unwrap();
        let second_binding = store
            .agent_binding(&actor, &second_session.id, &second_branch.id)
            .unwrap();
        let second_ref = root_ref(&second_binding);
        let second_position = store
            .agent_job_exposure_position(&actor, &second_binding)
            .unwrap();
        let second = store.agent_get(&actor, &second_ref).unwrap();

        let finite_ref = format!("{first_ref}/t:1/j:finite-rebuild");
        store
            .finite_job_exposure_reserve(
                &actor,
                &first_binding,
                "rebuild-generation",
                &finite_ref,
                8,
            )
            .unwrap();
        store
            .finite_job_exposure_publish(&actor, &first_binding, "rebuild-generation", &finite_ref)
            .unwrap();
        let finite_before = store
            .finite_job_exposures(&actor, &first_binding, "rebuild-generation", 8)
            .unwrap();
        let commits_before: u64 = {
            let connection = store.connection().unwrap();
            connection
                .query_row("SELECT count(*) FROM commit_log", [], |row| row.get(0))
                .unwrap()
        };

        {
            let mut connection = store.connection().unwrap();
            connection
                .execute(
                    "DELETE FROM job_exposure_current
                     WHERE kind='agent' AND generation=''",
                    [],
                )
                .unwrap();
            assert_eq!(normalize_agent_exposures(&mut connection).unwrap(), 2);
            assert_eq!(
                normalize_agent_exposures(&mut connection).unwrap(),
                0,
                "already-exact projection must not churn on Store open"
            );
        }

        assert_eq!(store.agent_get(&actor, &first_ref).unwrap(), held);
        assert_eq!(store.agent_get(&actor, &second_ref).unwrap(), second);
        assert_eq!(
            store
                .agent_job_exposure_position(&actor, &first_binding)
                .unwrap(),
            first_position
        );
        assert_eq!(
            store
                .agent_job_exposure_position(&actor, &second_binding)
                .unwrap(),
            second_position
        );
        assert_eq!(
            store
                .finite_job_exposures(&actor, &first_binding, "rebuild-generation", 8)
                .unwrap(),
            finite_before,
            "Agent projection rebuild must not disturb finite Job rows"
        );
        let commits_after: u64 = {
            let connection = store.connection().unwrap();
            connection
                .query_row("SELECT count(*) FROM commit_log", [], |row| row.get(0))
                .unwrap()
        };
        assert_eq!(
            commits_after, commits_before,
            "projection rebuild must not manufacture publication evidence"
        );
        assert_eq!(
            store
                .agent_binding_by_handles(
                    &actor,
                    &first_binding.domain_handle,
                    &first_binding.identity_handle,
                    &first_binding.project_handle,
                    &first_binding.agent_handle,
                )
                .unwrap(),
            first_binding
        );
    }
}
