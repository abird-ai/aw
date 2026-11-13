//! Durable mixed Job exposure ordering over the existing immutable commit log.
//!
//! Agent roots reuse their authoritative Session/branch creation sequence.
//! Finite Jobs reserve one common list position before effects, then publish or
//! release that exact reservation. This module stores routing/list evidence only;
//! Agent lifecycle/history stay in SessionStore and finite lifecycle stays in
//! JobManager.

use crate::{
    agent_binding_in, context, db, durability, workspace_writer, AgentBindingInfo, Error,
    PrincipalId, Result, SessionStore,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const ENTITY: &str = "job_exposure";
const RESERVED: &str = "job.exposure.reserved";
const PUBLISHED: &str = "job.exposure.published";
const RELEASED: &str = "job.exposure.released";
const MAX_EXPOSURES: usize = 8_192;

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

fn root_ref(binding: &AgentBindingInfo) -> String {
    format!(
        "/d:{}/i:{}/p:{}/a:{}",
        binding.domain_handle,
        binding.identity_handle,
        binding.project_handle,
        binding.agent_handle,
    )
}

fn validate_ref(binding: &AgentBindingInfo, canonical_ref: &str) -> Result<()> {
    let root = root_ref(binding);
    if canonical_ref.len() > 2048
        || !canonical_ref.is_ascii()
        || canonical_ref.chars().any(char::is_control)
        || !canonical_ref.starts_with(&(root + "/t:"))
        || !canonical_ref.contains("/j:")
    {
        return Err(Error::Denied(
            "finite Job exposure ref is outside the authoritative Agent owner",
        ));
    }
    Ok(())
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

fn decode_event(kind: &str, seq: u64, encoded: &str) -> Result<FiniteExposure> {
    let payload: Payload = crate::decode(encoded)?;
    let state = kind_state(kind)?;
    let position = match state {
        FiniteExposureState::Reserved => seq,
        FiniteExposureState::Published | FiniteExposureState::Released => {
            payload.reservation_position.ok_or(Error::Storage(
                "finite Job exposure closure lacks reservation position",
            ))?
        }
    };
    if position == 0 || payload.writer_epoch == 0 {
        return Err(Error::Storage("invalid finite Job exposure event"));
    }
    Ok(FiniteExposure {
        canonical_ref: payload.canonical_ref,
        position,
        state,
        generation: payload.generation,
        domain_handle: payload.domain_handle,
        identity_handle: payload.identity_handle,
        project_handle: payload.project_handle,
        session_id: payload.session_id,
        branch_id: payload.branch_id,
        writer_epoch: payload.writer_epoch,
    })
}

fn latest(
    c: &rusqlite::Connection,
    generation: &str,
    canonical_ref: &str,
) -> Result<Option<FiniteExposure>> {
    let key = entity_key(generation, canonical_ref);
    let row: Option<(u64, String, String)> = db(c
        .query_row(
            "SELECT seq,kind,payload_json FROM commit_log
             WHERE entity_type=?1 AND entity_id=?2
             ORDER BY seq DESC LIMIT 1",
            params![ENTITY, key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional())?;
    row.map(|(seq, kind, payload)| decode_event(&kind, seq, &payload))
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

fn payload(
    binding: &AgentBindingInfo,
    generation: &str,
    canonical_ref: &str,
    writer_epoch: u64,
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
        reservation_position,
    }
}

impl SessionStore {
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
            return match existing.state {
                FiniteExposureState::Reserved | FiniteExposureState::Published => Ok(existing),
                FiniteExposureState::Released => Err(Error::Conflict(
                    "released finite Job exposure ref cannot be reserved again",
                )),
            };
        }

        let active: u64 = db(tx.query_row(
            "WITH latest AS (
                 SELECT entity_id,MAX(seq) AS seq
                 FROM commit_log
                 WHERE entity_type=?1
                   AND json_extract(payload_json,'$.generation')=?2
                   AND json_extract(payload_json,'$.domain_handle')=?3
                   AND json_extract(payload_json,'$.identity_handle')=?4
                   AND json_extract(payload_json,'$.project_handle')=?5
                 GROUP BY entity_id
             )
             SELECT COUNT(*)
             FROM commit_log c JOIN latest l ON c.seq=l.seq
             WHERE c.kind IN (?6,?7)",
            params![
                ENTITY,
                generation,
                actual.domain_handle,
                actual.identity_handle,
                actual.project_handle,
                RESERVED,
                PUBLISHED,
            ],
            |row| row.get(0),
        ))?;
        if active >= capacity as u64 {
            return Err(Error::Limit(
                "project finite Job exposure recoverability capacity exhausted",
            ));
        }

        let txid = durability::tx_id()?;
        let encoded = serde_json::to_value(payload(
            &actual,
            generation,
            canonical_ref,
            writer.writer_epoch,
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
            position,
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

        let txid = durability::tx_id()?;
        let encoded = serde_json::to_value(payload(
            &actual,
            generation,
            canonical_ref,
            existing.writer_epoch,
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
        let txid = durability::tx_id()?;
        let encoded = serde_json::to_value(payload(
            &actual,
            generation,
            canonical_ref,
            existing.writer_epoch,
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
        context::authorize_tree(&connection, &actual.session_id, actor)?;
        let mut query = db(connection.prepare(
            "WITH latest AS (
                 SELECT entity_id,MAX(seq) AS seq
                 FROM commit_log
                 WHERE entity_type=?1
                   AND json_extract(payload_json,'$.generation')=?2
                   AND json_extract(payload_json,'$.domain_handle')=?3
                   AND json_extract(payload_json,'$.identity_handle')=?4
                   AND json_extract(payload_json,'$.project_handle')=?5
                 GROUP BY entity_id
             )
             SELECT c.seq,c.kind,c.payload_json
             FROM commit_log c JOIN latest l ON c.seq=l.seq
             WHERE c.kind=?6
             ORDER BY CAST(json_extract(c.payload_json,'$.reservation_position') AS INTEGER) DESC,
                      json_extract(c.payload_json,'$.canonical_ref') ASC
             LIMIT ?7",
        ))?;
        let rows = db(query.query_map(
            params![
                ENTITY,
                generation,
                actual.domain_handle,
                actual.identity_handle,
                actual.project_handle,
                PUBLISHED,
                limit as i64,
            ],
            |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        ))?;
        let mut result = Vec::new();
        for row in rows {
            let (seq, kind, payload) = db(row)?;
            result.push(decode_event(&kind, seq, &payload)?);
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
                "SELECT seq FROM commit_log
                 WHERE (kind='session.created'
                        AND entity_type='session'
                        AND entity_id=?1
                        AND json_extract(payload_json,'$.branch_id')=?2)
                    OR (kind='branch.created'
                        AND entity_type='branch'
                        AND entity_id=?2)
                 ORDER BY seq LIMIT 1",
                params![actual.session_id, actual.branch_id],
                |row| row.get(0),
            )
            .optional())?;
        position.ok_or(Error::Storage(
            "Agent creation exposure position is unavailable",
        ))
    }
}
