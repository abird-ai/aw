use super::{db, id, json, now};
use aw_agent_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const MAX_SCOPE_BYTES: usize = 512;
const MIN_LEASE_MICROS: i64 = 10_000;
const MAX_LEASE_MICROS: i64 = 6 * 60 * 60 * 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalLeaseKind {
    Write,
    Execution,
}

impl LocalLeaseKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Execution => "execution",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalProcessInfo {
    pub process_id: String,
    pub store_generation: u64,
    pub process_kind: String,
    pub os_pid: u32,
    pub state: String,
    pub started_at: i64,
    pub last_seen_at: i64,
    pub lease_expires_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalCoordinationLease {
    pub kind: LocalLeaseKind,
    pub scope_key: String,
    pub holder_process_id: String,
    pub store_generation: u64,
    pub generation: u64,
    pub state: String,
    pub run_id: Option<String>,
    pub expires_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalMaintenanceLease {
    pub holder_process_id: String,
    pub store_generation: u64,
    pub generation: u64,
    pub purpose: String,
    pub expires_at: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct ProcessIdentity {
    pub process_id: String,
    pub store_generation: u64,
    pub heartbeat_ttl_micros: Option<i64>,
}

fn duration_micros(ttl: Duration) -> Result<i64> {
    let micros = ttl.as_micros();
    if micros < MIN_LEASE_MICROS as u128 || micros > MAX_LEASE_MICROS as u128 {
        return Err(Error::Config(
            "local coordination lease TTL must be between 10ms and 6h".into(),
        ));
    }
    Ok(micros as i64)
}

fn validate_scope(scope_key: &str) -> Result<()> {
    if scope_key.is_empty()
        || scope_key.len() > MAX_SCOPE_BYTES
        || scope_key.chars().any(char::is_control)
    {
        return Err(Error::Config(
            "local coordination scope must be a bounded printable key".into(),
        ));
    }
    Ok(())
}

fn store_identity(c: &Connection) -> Result<(String, u64)> {
    let store_id: String = db(c.query_row(
        "SELECT value FROM store_meta WHERE key='store_id'",
        [],
        |r| r.get(0),
    ))?;
    let generation: String = db(c.query_row(
        "SELECT value FROM store_meta WHERE key='restore_generation'",
        [],
        |r| r.get(0),
    ))?;
    let generation = generation
        .parse::<u64>()
        .map_err(|_| Error::Storage("invalid restore generation"))?;
    if generation == 0 {
        return Err(Error::Storage("invalid restore generation"));
    }
    Ok((store_id, generation))
}

fn event(
    c: &Connection,
    process: Option<&ProcessIdentity>,
    kind: &str,
    scope_kind: Option<&str>,
    scope_key: Option<&str>,
    lease_generation: Option<u64>,
    payload: serde_json::Value,
) -> Result<()> {
    let (store_id, store_generation) = store_identity(c)?;
    db(c.execute(
        "INSERT INTO local_coordination_events(event_id,store_id,store_generation,process_id,kind,scope_kind,scope_key,lease_generation,payload_json,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            id("coord")?,
            store_id,
            store_generation,
            process.map(|p| p.process_id.as_str()),
            kind,
            scope_kind,
            scope_key,
            lease_generation,
            json(&payload)?,
            now()
        ],
    ))?;
    Ok(())
}

fn validate_process_in(c: &Connection, process: &ProcessIdentity, at: i64) -> Result<()> {
    let (_, store_generation) = store_identity(c)?;
    if store_generation != process.store_generation {
        return Err(Error::Conflict(
            "store generation changed; stale local process is fenced",
        ));
    }
    let row: Option<(u64, String, Option<i64>)> = db(c
        .query_row(
            "SELECT store_generation,state,lease_expires_at
             FROM local_process_incarnations WHERE process_id=?1",
            [&process.process_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional())?;
    let Some((generation, state, expires_at)) = row else {
        return Err(Error::Conflict(
            "local process incarnation is no longer registered",
        ));
    };
    if generation != process.store_generation || state != "active" {
        return Err(Error::Conflict(
            "local process incarnation is stale or fenced",
        ));
    }
    if expires_at.is_some_and(|expires| expires <= at) {
        return Err(Error::Conflict(
            "local process lease expired; reopen before writing",
        ));
    }
    Ok(())
}

pub(crate) fn validate_process(c: &Connection, process: &ProcessIdentity) -> Result<()> {
    validate_process_in(c, process, now())
}

pub(crate) fn validate_ordinary_access(c: &Connection) -> Result<()> {
    let at = now();
    let blocked: bool = db(c.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM local_maintenance_lease
            WHERE singleton=1 AND state='active'
              AND expires_at IS NOT NULL AND expires_at>?1
        )",
        [at],
        |r| r.get(0),
    ))?;
    if blocked {
        return Err(Error::Conflict(
            "store migration/maintenance is active; ordinary access is temporarily fenced",
        ));
    }
    Ok(())
}

fn register_process(
    c: &mut Connection,
    process_kind: &str,
    heartbeat_ttl_micros: Option<i64>,
) -> Result<ProcessIdentity> {
    let (store_id, store_generation) = store_identity(c)?;
    let at = now();
    let expires_at = heartbeat_ttl_micros.map(|ttl| at.saturating_add(ttl));
    let process = ProcessIdentity {
        process_id: id("proc")?,
        store_generation,
        heartbeat_ttl_micros,
    };
    let nonce = id("inc")?;
    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
    if process_kind == "owner_lock" {
        // In Phase 35 owner/service mode holds owner.lock exclusively while every
        // trusted-direct client holds it shared. Acquiring the exclusive lock
        // therefore proves every earlier persistent process has released its
        // authoritative file-lock claim. Never use PID liveness for this: PIDs
        // are reusable and not authority.
        db(tx.execute(
            "UPDATE local_process_incarnations SET state='fenced',stopped_at=?1
             WHERE state='active'",
            [at],
        ))?;
    }
    reap_stale_in(&tx, at)?;
    db(tx.execute(
        "INSERT INTO local_process_incarnations(
            process_id,store_id,store_generation,process_nonce,process_kind,os_pid,state,
            started_at,last_seen_at,lease_expires_at)
         VALUES(?1,?2,?3,?4,?5,?6,'active',?7,?7,?8)",
        params![
            process.process_id,
            store_id,
            store_generation,
            nonce,
            process_kind,
            std::process::id(),
            at,
            expires_at
        ],
    ))?;
    event(
        &tx,
        Some(&process),
        "process.registered",
        None,
        None,
        None,
        serde_json::json!({"process_kind":process_kind,"os_pid":std::process::id()}),
    )?;
    db(tx.commit())?;
    Ok(process)
}

pub(crate) fn register_owner_process(c: &mut Connection) -> Result<ProcessIdentity> {
    register_process(c, "owner_lock", None)
}

pub(crate) fn register_trusted_process(
    c: &mut Connection,
    ttl: Duration,
    owner_lock_proven_free: bool,
) -> Result<ProcessIdentity> {
    if owner_lock_proven_free {
        let at = now();
        let tx = begin_coordination(c)?;
        db(tx.execute(
            "UPDATE local_process_incarnations SET state='fenced',stopped_at=?1
             WHERE process_kind='owner_lock' AND state='active'",
            [at],
        ))?;
        reap_stale_in(&tx, at)?;
        db(tx.commit())?;
    }
    register_process(c, "trusted_local", Some(duration_micros(ttl)?))
}

#[cfg(test)]
pub(crate) fn register_stress_process(
    c: &mut Connection,
    ttl: Duration,
) -> Result<ProcessIdentity> {
    register_trusted_process(c, ttl, false)
}

pub(crate) fn stop_process(c: &Connection, process: &ProcessIdentity) -> Result<()> {
    let at = now();
    let changed = db(c.execute(
        "UPDATE local_process_incarnations
         SET state='stopped',last_seen_at=?1,stopped_at=?1
         WHERE process_id=?2 AND store_generation=?3 AND state='active'",
        params![at, process.process_id, process.store_generation],
    ))?;
    if changed == 1 {
        event(
            c,
            Some(process),
            "process.stopped",
            None,
            None,
            None,
            serde_json::json!({}),
        )?;
    }
    Ok(())
}

pub(crate) fn heartbeat_process(
    c: &Connection,
    process: &ProcessIdentity,
) -> Result<LocalProcessInfo> {
    let at = now();
    validate_process_in(c, process, at)?;
    let expires_at = process
        .heartbeat_ttl_micros
        .map(|ttl| at.saturating_add(ttl));
    let changed = db(c.execute(
        "UPDATE local_process_incarnations
         SET last_seen_at=?1,lease_expires_at=?2
         WHERE process_id=?3 AND store_generation=?4 AND state='active'",
        params![at, expires_at, process.process_id, process.store_generation],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict(
            "local process lease changed while heartbeating",
        ));
    }
    process_info(c, process)
}

pub(crate) fn process_info(c: &Connection, process: &ProcessIdentity) -> Result<LocalProcessInfo> {
    db(c.query_row(
        "SELECT process_id,store_generation,process_kind,os_pid,state,started_at,last_seen_at,lease_expires_at
         FROM local_process_incarnations WHERE process_id=?1",
        [&process.process_id],
        |r| {
            Ok(LocalProcessInfo {
                process_id: r.get(0)?,
                store_generation: r.get(1)?,
                process_kind: r.get(2)?,
                os_pid: r.get(3)?,
                state: r.get(4)?,
                started_at: r.get(5)?,
                last_seen_at: r.get(6)?,
                lease_expires_at: r.get(7)?,
            })
        },
    ))
}

fn reap_stale_in(c: &Connection, at: i64) -> Result<()> {
    db(c.execute(
        "UPDATE local_process_incarnations
         SET state='fenced',stopped_at=?1
         WHERE state='active' AND lease_expires_at IS NOT NULL AND lease_expires_at<=?1",
        [at],
    ))?;
    db(c.execute(
        "UPDATE local_coordination_leases
         SET state='released',updated_at=?1
         WHERE scope_kind='write' AND state='active'
           AND (expires_at<=?1 OR EXISTS(
               SELECT 1 FROM local_process_incarnations p
               WHERE p.process_id=holder_process_id
                 AND (p.state<>'active' OR p.store_generation<>local_coordination_leases.store_generation)))",
        [at],
    ))?;
    db(c.execute(
        "UPDATE local_coordination_leases
         SET state='hazard',updated_at=?1
         WHERE scope_kind='execution' AND state='active'
           AND (expires_at<=?1 OR EXISTS(
               SELECT 1 FROM local_process_incarnations p
               WHERE p.process_id=holder_process_id
                 AND (p.state<>'active' OR p.store_generation<>local_coordination_leases.store_generation)))",
        [at],
    ))?;
    db(c.execute(
        "UPDATE local_maintenance_lease
         SET holder_process_id=NULL,store_generation=NULL,purpose=NULL,state='idle',
             expires_at=NULL,updated_at=?1
         WHERE singleton=1 AND state='active'
           AND (expires_at<=?1 OR EXISTS(
               SELECT 1 FROM local_process_incarnations p
               WHERE p.process_id=holder_process_id
                 AND (p.state<>'active' OR p.store_generation<>local_maintenance_lease.store_generation)))",
        [at],
    ))?;
    Ok(())
}

fn begin_coordination(c: &mut Connection) -> Result<Transaction<'_>> {
    db(c.transaction_with_behavior(TransactionBehavior::Immediate))
}

fn acquire_scope_lease_in(
    c: &Connection,
    process: &ProcessIdentity,
    kind: LocalLeaseKind,
    scope_key: &str,
    run_id: Option<&str>,
    ttl: Duration,
) -> Result<LocalCoordinationLease> {
    validate_scope(scope_key)?;
    let ttl = duration_micros(ttl)?;
    let at = now();
    reap_stale_in(c, at)?;
    validate_process_in(c, process, at)?;
    let maintenance: bool = db(c.query_row(
        "SELECT state='active' AND expires_at>?1 FROM local_maintenance_lease WHERE singleton=1",
        [at],
        |r| r.get(0),
    ))?;
    if maintenance {
        return Err(Error::Conflict(
            "store maintenance lease is active; local write/effect admission is fenced",
        ));
    }
    let existing: Option<(String, String, u64, u64, Option<String>, i64)> = db(c
        .query_row(
            "SELECT holder_process_id,state,store_generation,generation,run_id,expires_at
             FROM local_coordination_leases WHERE scope_kind=?1 AND scope_key=?2",
            params![kind.as_str(), scope_key],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .optional())?;
    let generation = match existing {
        Some((holder, state, store_generation, generation, existing_run, expires))
            if state == "active"
                && holder == process.process_id
                && store_generation == process.store_generation
                && existing_run.as_deref() == run_id
                && expires > at =>
        {
            let expires_at = at.saturating_add(ttl);
            let changed = db(c.execute(
                "UPDATE local_coordination_leases SET expires_at=?1,updated_at=?2
                 WHERE scope_kind=?3 AND scope_key=?4 AND holder_process_id=?5
                   AND store_generation=?6 AND generation=?7 AND state='active'",
                params![
                    expires_at,
                    at,
                    kind.as_str(),
                    scope_key,
                    process.process_id,
                    process.store_generation,
                    generation
                ],
            ))?;
            if changed != 1 {
                return Err(Error::Conflict(
                    "local coordination lease changed during renewal",
                ));
            }
            return Ok(LocalCoordinationLease {
                kind,
                scope_key: scope_key.into(),
                holder_process_id: process.process_id.clone(),
                store_generation: process.store_generation,
                generation,
                state: "active".into(),
                run_id: run_id.map(str::to_owned),
                expires_at,
            });
        }
        Some((_, state, _, _, _, _)) if state == "active" => {
            return Err(Error::Conflict(
                "local coordination lease is held by another live process",
            ));
        }
        Some((_, state, _, _, _, _)) if state == "hazard" => {
            return Err(Error::Conflict(
                "local execution lease is a hazard; reconcile it explicitly before reuse",
            ));
        }
        Some((_, _, _, generation, _, _)) => generation
            .checked_add(1)
            .ok_or(Error::Limit("local lease generation exhausted"))?,
        None => 1,
    };
    let expires_at = at.saturating_add(ttl);
    db(c.execute(
        "INSERT INTO local_coordination_leases(
            scope_kind,scope_key,holder_process_id,store_generation,generation,state,run_id,expires_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,'active',?6,?7,?8)
         ON CONFLICT(scope_kind,scope_key) DO UPDATE SET
            holder_process_id=excluded.holder_process_id,
            store_generation=excluded.store_generation,
            generation=excluded.generation,
            state='active',
            run_id=excluded.run_id,
            expires_at=excluded.expires_at,
            updated_at=excluded.updated_at",
        params![
            kind.as_str(),
            scope_key,
            process.process_id,
            process.store_generation,
            generation,
            run_id,
            expires_at,
            at
        ],
    ))?;
    event(
        c,
        Some(process),
        "lease.acquired",
        Some(kind.as_str()),
        Some(scope_key),
        Some(generation),
        serde_json::json!({"run_id":run_id}),
    )?;
    Ok(LocalCoordinationLease {
        kind,
        scope_key: scope_key.into(),
        holder_process_id: process.process_id.clone(),
        store_generation: process.store_generation,
        generation,
        state: "active".into(),
        run_id: run_id.map(str::to_owned),
        expires_at,
    })
}

pub(crate) fn acquire_scope_lease(
    c: &mut Connection,
    process: &ProcessIdentity,
    kind: LocalLeaseKind,
    scope_key: &str,
    run_id: Option<&str>,
    ttl: Duration,
) -> Result<LocalCoordinationLease> {
    let tx = begin_coordination(c)?;
    let lease = acquire_scope_lease_in(&tx, process, kind, scope_key, run_id, ttl)?;
    db(tx.commit())?;
    Ok(lease)
}

pub(crate) fn acquire_execution_lease_in(
    c: &Connection,
    process: &ProcessIdentity,
    scope_key: &str,
    run_id: &str,
    ttl: Duration,
) -> Result<LocalCoordinationLease> {
    acquire_scope_lease_in(
        c,
        process,
        LocalLeaseKind::Execution,
        scope_key,
        Some(run_id),
        ttl,
    )
}

fn settle_scope_lease_in(
    c: &Connection,
    process: &ProcessIdentity,
    lease: &LocalCoordinationLease,
    uncertain_external_effect: bool,
) -> Result<()> {
    let at = now();
    validate_process_in(c, process, at)?;
    if lease.holder_process_id != process.process_id
        || lease.store_generation != process.store_generation
    {
        return Err(Error::Conflict(
            "local coordination lease belongs to another process generation",
        ));
    }
    let state = if uncertain_external_effect && lease.kind == LocalLeaseKind::Execution {
        "hazard"
    } else {
        "released"
    };
    let changed = db(c.execute(
        "UPDATE local_coordination_leases SET state=?1,updated_at=?2
         WHERE scope_kind=?3 AND scope_key=?4 AND holder_process_id=?5
           AND store_generation=?6 AND generation=?7 AND state='active'",
        params![
            state,
            at,
            lease.kind.as_str(),
            lease.scope_key,
            process.process_id,
            process.store_generation,
            lease.generation
        ],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict(
            "local coordination lease changed before settlement",
        ));
    }
    event(
        c,
        Some(process),
        if state == "hazard" {
            "lease.hazard"
        } else {
            "lease.released"
        },
        Some(lease.kind.as_str()),
        Some(&lease.scope_key),
        Some(lease.generation),
        serde_json::json!({"uncertain_external_effect":uncertain_external_effect}),
    )?;
    Ok(())
}

pub(crate) fn settle_scope_lease(
    c: &mut Connection,
    process: &ProcessIdentity,
    lease: &LocalCoordinationLease,
    uncertain_external_effect: bool,
) -> Result<()> {
    let tx = begin_coordination(c)?;
    settle_scope_lease_in(&tx, process, lease, uncertain_external_effect)?;
    db(tx.commit())?;
    Ok(())
}

pub(crate) fn settle_execution_scope_in(
    c: &Connection,
    process: &ProcessIdentity,
    scope_key: &str,
    run_id: &str,
) -> Result<()> {
    let row: Option<(u64, i64)> = db(c
        .query_row(
            "SELECT generation,expires_at FROM local_coordination_leases
             WHERE scope_kind='execution' AND scope_key=?1
               AND holder_process_id=?2 AND store_generation=?3
               AND run_id=?4 AND state='active'",
            params![
                scope_key,
                process.process_id,
                process.store_generation,
                run_id
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional())?;
    let Some((generation, expires_at)) = row else {
        return Err(Error::Conflict(
            "provider execution lease is missing or no longer owned by this process",
        ));
    };
    settle_scope_lease_in(
        c,
        process,
        &LocalCoordinationLease {
            kind: LocalLeaseKind::Execution,
            scope_key: scope_key.into(),
            holder_process_id: process.process_id.clone(),
            store_generation: process.store_generation,
            generation,
            state: "active".into(),
            run_id: Some(run_id.into()),
            expires_at,
        },
        false,
    )
}

pub(crate) fn reconcile_provider_run_leases(c: &Connection, run_id: &str) -> Result<usize> {
    let leases = {
        let mut q = db(c.prepare(
            "SELECT scope_key,generation,state FROM local_coordination_leases
             WHERE scope_kind='execution' AND run_id=?1 AND scope_key LIKE 'provider:%'
               AND state IN ('active','hazard')
             ORDER BY scope_key",
        ))?;
        let rows = db(q.query_map([run_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, u64>(1)?,
                r.get::<_, String>(2)?,
            ))
        }))?;
        db(rows.collect::<rusqlite::Result<Vec<_>>>())?
    };
    for (scope_key, generation, old_state) in &leases {
        let changed = db(c.execute(
            "UPDATE local_coordination_leases SET state='released',updated_at=?1
             WHERE scope_kind='execution' AND scope_key=?2 AND generation=?3
               AND run_id=?4 AND state=?5",
            params![now(), scope_key, generation, run_id, old_state],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict(
                "provider execution lease changed during durable reconciliation",
            ));
        }
        event(
            c,
            None,
            "lease.provider_reconciled",
            Some("execution"),
            Some(scope_key),
            Some(*generation),
            serde_json::json!({"run_id":run_id,"prior_state":old_state}),
        )?;
    }
    Ok(leases.len())
}

pub(crate) fn resolve_execution_hazard(
    c: &mut Connection,
    process: &ProcessIdentity,
    scope_key: &str,
    expected_generation: u64,
    note: &str,
) -> Result<()> {
    validate_scope(scope_key)?;
    if note.trim().is_empty() || note.len() > 2048 || note.chars().any(char::is_control) {
        return Err(Error::Config(
            "hazard reconciliation note must be bounded printable text".into(),
        ));
    }
    let at = now();
    let tx = begin_coordination(c)?;
    reap_stale_in(&tx, at)?;
    validate_process_in(&tx, process, at)?;
    let changed = db(tx.execute(
        "UPDATE local_coordination_leases SET state='released',updated_at=?1
         WHERE scope_kind='execution' AND scope_key=?2 AND generation=?3 AND state='hazard'",
        params![at, scope_key, expected_generation],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict(
            "execution hazard generation changed or is no longer unresolved",
        ));
    }
    event(
        &tx,
        Some(process),
        "lease.hazard_resolved",
        Some("execution"),
        Some(scope_key),
        Some(expected_generation),
        serde_json::json!({"note":note}),
    )?;
    db(tx.commit())?;
    Ok(())
}

pub(crate) fn acquire_maintenance(
    c: &mut Connection,
    process: &ProcessIdentity,
    purpose: &str,
    ttl: Duration,
) -> Result<LocalMaintenanceLease> {
    if !matches!(purpose, "migration" | "backup" | "restore" | "maintenance") {
        return Err(Error::Config("invalid local maintenance purpose".into()));
    }
    let ttl = duration_micros(ttl)?;
    let at = now();
    let tx = begin_coordination(c)?;
    reap_stale_in(&tx, at)?;
    validate_process_in(&tx, process, at)?;
    let current: (String, u64) = db(tx.query_row(
        "SELECT state,generation FROM local_maintenance_lease WHERE singleton=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ))?;
    if current.0 == "active" {
        return Err(Error::Conflict(
            "another local migration/maintenance lease is active",
        ));
    }
    let blockers: i64 = db(tx.query_row(
        "SELECT count(*) FROM local_coordination_leases
         WHERE state IN ('active','hazard')",
        [],
        |r| r.get(0),
    ))?;
    if blockers != 0 {
        return Err(Error::Conflict(
            "active write/execution lease or unresolved effect hazard blocks maintenance",
        ));
    }
    let generation = current
        .1
        .checked_add(1)
        .ok_or(Error::Limit("maintenance lease generation exhausted"))?;
    let expires_at = at.saturating_add(ttl);
    let changed = db(tx.execute(
        "UPDATE local_maintenance_lease
         SET holder_process_id=?1,store_generation=?2,generation=?3,purpose=?4,
             state='active',expires_at=?5,updated_at=?6
         WHERE singleton=1 AND state='idle'",
        params![
            process.process_id,
            process.store_generation,
            generation,
            purpose,
            expires_at,
            at
        ],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict(
            "maintenance lease changed during acquisition",
        ));
    }
    event(
        &tx,
        Some(process),
        "maintenance.acquired",
        None,
        None,
        Some(generation),
        serde_json::json!({"purpose":purpose}),
    )?;
    db(tx.commit())?;
    Ok(LocalMaintenanceLease {
        holder_process_id: process.process_id.clone(),
        store_generation: process.store_generation,
        generation,
        purpose: purpose.into(),
        expires_at,
    })
}

pub(crate) fn release_maintenance(
    c: &mut Connection,
    process: &ProcessIdentity,
    lease: &LocalMaintenanceLease,
) -> Result<()> {
    let at = now();
    let tx = begin_coordination(c)?;
    validate_process_in(&tx, process, at)?;
    let changed = db(tx.execute(
        "UPDATE local_maintenance_lease
         SET holder_process_id=NULL,store_generation=NULL,purpose=NULL,state='idle',
             expires_at=NULL,updated_at=?1
         WHERE singleton=1 AND state='active' AND holder_process_id=?2
           AND store_generation=?3 AND generation=?4 AND purpose=?5",
        params![
            at,
            process.process_id,
            process.store_generation,
            lease.generation,
            lease.purpose
        ],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict("maintenance lease changed before release"));
    }
    event(
        &tx,
        Some(process),
        "maintenance.released",
        None,
        None,
        Some(lease.generation),
        serde_json::json!({"purpose":lease.purpose}),
    )?;
    db(tx.commit())?;
    Ok(())
}

/// ORIGINAL restoration fencing shared by the existing online owner
/// backup restore and the S OFFLINE cold-restored quarantine. It advances
/// the same original Store generation, fences stale Processes, preserves
/// execution UNKNOWN as hazards and releases old SQL maintenance authority.
/// No paid/Tool/Queue effect can be executed from this transaction.
fn fence_previous_restore_evidence(
    tx: &rusqlite::Transaction<'_>,
    next_generation: u64,
    at: i64,
) -> Result<()> {
    db(tx.execute(
        "UPDATE store_meta SET value=?1 WHERE key='restore_generation'",
        [next_generation.to_string()],
    ))?;
    db(tx.execute(
        "UPDATE local_process_incarnations
         SET state='fenced',stopped_at=?1
         WHERE state='active'",
        [at],
    ))?;
    db(tx.execute(
        "UPDATE local_coordination_leases
         SET state=CASE WHEN scope_kind='execution' AND state IN ('active','hazard')
                        THEN 'hazard' ELSE 'released' END,
             updated_at=?1
         WHERE state IN ('active','hazard')",
        [at],
    ))?;
    db(tx.execute(
        "UPDATE local_maintenance_lease
         SET holder_process_id=NULL,store_generation=NULL,purpose=NULL,state='idle',
             expires_at=NULL,updated_at=?1 WHERE singleton=1",
        [at],
    ))?;
    Ok(())
}

/// AW-P6-10T: persistent *denial* evidence INSIDE copied SQLite; the OS file
/// marker alone must never determine whether a copied owner can become live.
/// This value is NOT human consent and never grants Process/Writer/Tool rights.
#[cfg(target_os = "linux")]
pub(crate) fn cold_quarantine_identity(store_id: &str, source: u64, current: u64) -> String {
    format!("aw.cold.quarantined.v1:{store_id}:{source}:{current}")
}

/// AW-P6-10S: OFFLINE restore of one source-independent VERIFIED V1 archive.
/// Advances the original generation and fences EVERY prior active Process and
/// original execution/write lease, but deliberately does NOT register any new
/// owner/Actor process or authorize model/Tool/Queue replay. All Workspace
/// writer epochs become explicit READ_ONLY and advance: a fresh owner later
/// cannot interpret copied source-era writer epochs as current authority.
/// A separate owner-private quarantine marker makes normal Store opening fail
/// before Process registration; this SQL pre-fence is defense in depth.
#[cfg(target_os = "linux")]
pub(crate) fn prepare_offline_cold_restore(
    c: &mut Connection,
    expected_store_id: &str,
    expected_generation: u64,
) -> Result<u64> {
    let (store_id, generation) = store_identity(c)?;
    if store_id != expected_store_id || generation != expected_generation {
        return Err(Error::Denied(
            "offline cold restore source Store identity/generation changed",
        ));
    }
    let next = generation
        .checked_add(1)
        .ok_or(Error::Limit("offline restore generation exhausted"))?;
    let at = now();
    let tx = begin_coordination(c)?;
    // Fail BEFORE mutation if any original writer epoch/revision cannot safely
    // advance, or if a historical Store has impossible writer metadata.
    let cannot_fence: i64 = db(tx.query_row(
        "SELECT count(*) FROM workspace_writer_state
         WHERE writer_epoch>=9223372036854775807 OR revision>=9223372036854775807",
        [],
        |row| row.get(0),
    ))?;
    if cannot_fence != 0 {
        return Err(Error::Limit(
            "original Workspace writer epoch/revision cannot be fenced",
        ));
    }
    fence_previous_restore_evidence(&tx, next, at)?;
    // Add only original immutable workspace_writer_events, with the same
    // monotonic epochs and original node IDs. NO new active writer is minted.
    db(tx.execute(
        "INSERT INTO workspace_writer_events(
           event_id,workspace_id,domain_id,old_writer_node_id,old_writer_epoch,
           new_writer_node_id,new_writer_epoch,authority_term,actor,reason,created_at)
         SELECT 'wwe_'||lower(hex(randomblob(16))),workspace_id,domain_id,
                writer_node_id,writer_epoch,writer_node_id,writer_epoch+1,
                authority_term,'system','cold_restore_quarantine',?1
           FROM workspace_writer_state",
        [at],
    ))?;
    db(tx.execute(
        "UPDATE workspace_writer_state SET writer_epoch=writer_epoch+1,
             state='read_only',revision=revision+1,updated_at=?1",
        [at],
    ))?;
    // The second original independent quarantine guard is persistent SQL.
    // Directly deleting the external 0600 marker must NEVER make the copied
    // Store runnable. No migration, extra table, lease or new actor grant.
    db(tx.execute(
        "INSERT INTO store_meta(key,value) VALUES('cold_restore_quarantine',?1)",
        [cold_quarantine_identity(&store_id, generation, next)],
    ))?;
    event(
        &tx,
        None,
        "store.offline_cold_restore_quarantined",
        None,
        None,
        None,
        serde_json::json!({"source_generation":generation,"new_generation":next,
                          "owner_process_created":false,"workspace_writers":"read_only"}),
    )?;
    db(tx.commit())?;
    Ok(next)
}

pub(crate) fn prepare_restore_snapshot(
    c: &mut Connection,
    process: &ProcessIdentity,
    previous_live_generation: u64,
    source_generation: u64,
) -> Result<ProcessIdentity> {
    let next_generation = previous_live_generation
        .max(source_generation)
        .checked_add(1)
        .ok_or(Error::Limit("restore generation exhausted"))?;
    let at = now();
    let (store_id, copied_generation) = store_identity(c)?;
    if copied_generation != source_generation {
        return Err(Error::Storage(
            "restore staging snapshot generation changed during preparation",
        ));
    }
    let tx = begin_coordination(c)?;
    fence_previous_restore_evidence(&tx, next_generation, at)?;
    let next = ProcessIdentity {
        process_id: id("proc")?,
        store_generation: next_generation,
        heartbeat_ttl_micros: process.heartbeat_ttl_micros,
    };
    let process_kind = if process.heartbeat_ttl_micros.is_some() {
        "trusted_local"
    } else {
        "owner_lock"
    };
    let expires_at = process
        .heartbeat_ttl_micros
        .map(|ttl| at.saturating_add(ttl));
    db(tx.execute(
        "INSERT INTO local_process_incarnations(
            process_id,store_id,store_generation,process_nonce,process_kind,os_pid,state,
            started_at,last_seen_at,lease_expires_at)
         VALUES(?1,?2,?3,?4,?5,?6,'active',?7,?7,?8)",
        params![
            next.process_id,
            store_id,
            next_generation,
            id("inc")?,
            process_kind,
            std::process::id(),
            at,
            expires_at
        ],
    ))?;
    event(
        &tx,
        Some(&next),
        "store.restore_generation_advanced",
        None,
        None,
        None,
        serde_json::json!({
            "previous_live_generation":previous_live_generation,
            "source_generation":source_generation,
            "new_generation":next_generation
        }),
    )?;
    db(tx.commit())?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ApplyContext, ApplyMode, PlanKind, PrepareContext, Selection, SessionBinding, SessionStore,
        SourceSelection, TargetHead,
    };
    use aw_agent_core::{
        AccountOwner, BillingOwner, CredentialCustody, InvocationIdentity, JournalEntry, Outcome,
        PrincipalId, Turn, TurnJournal, TurnStart,
    };
    use std::{
        fs,
        path::{Path, PathBuf},
        process::{Child, Command},
        sync::{Arc, Barrier},
        thread,
        time::Duration,
    };

    fn temp_state(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "aw-phase34-{label}-{}-{}",
            std::process::id(),
            crate::id("t").unwrap()
        ));
        let _ = fs::remove_dir_all(&path);
        path
    }

    fn binding() -> SessionBinding {
        SessionBinding {
            working_directory: PathBuf::from("/fixture"),
            tool_root: PathBuf::from("/fixture"),
            system_prompt: String::new(),
            profile_hint: "phase34".into(),
            model_hint: "fixture".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    fn actor() -> PrincipalId {
        PrincipalId::new("unix:1000").unwrap()
    }

    fn identity(actor: &PrincipalId) -> InvocationIdentity {
        InvocationIdentity {
            actor: actor.clone(),
            account_id: "phase34-account".into(),
            account_owner: AccountOwner::User(actor.clone()),
            payer: BillingOwner::User(actor.clone()),
            credential_custody: CredentialCustody::UserDevice("phase34-local".into()),
            credential_version: None,
            inference_executor: "phase34-local".into(),
            model: "fixture".into(),
            grant_revision: 1,
        }
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
    }

    fn turn(actor: &PrincipalId, prompt: &str) -> Turn {
        Turn {
            id: 1,
            prompt: prompt.into(),
            answer: "phase34 result".into(),
            model: "fixture".into(),
            profile: "phase34-account".into(),
            outcome: Outcome::Completed,
            meta: None,
            error: None,
            actor: Some(actor.clone()),
            tool_records: Vec::new(),
            invocations: Vec::new(),
            context_messages: Vec::new(),
            needs_reconciliation: false,
        }
    }

    fn open_stress(path: &Path, ttl: Duration) -> SessionStore {
        SessionStore::open_coordinated_for_phase34_stress(path, ttl).unwrap()
    }

    #[test]
    fn process_write_execution_and_hazard_generations_are_explicit() {
        let path = temp_state("lease-basics");
        let store = SessionStore::open(&path).unwrap();
        let info = store.local_process_info().unwrap();
        assert_eq!(info.process_kind, "owner_lock");
        assert_eq!(info.store_generation, 1);
        assert!(info.lease_expires_at.is_none());

        let write = store
            .acquire_local_write_lease("fs:/fixture", Duration::from_secs(1))
            .unwrap();
        assert_eq!(write.generation, 1);
        store.settle_local_lease(&write, false).unwrap();

        let execution = store
            .acquire_local_execution_lease(
                "provider:phase34",
                Some("run-phase34"),
                Duration::from_secs(1),
            )
            .unwrap();
        store.settle_local_lease(&execution, true).unwrap();
        assert!(store
            .acquire_local_execution_lease(
                "provider:phase34",
                Some("run-phase34-next"),
                Duration::from_secs(1)
            )
            .is_err());
        store
            .resolve_local_execution_hazard(
                "provider:phase34",
                execution.generation,
                "operator inspected the uncertain external effect",
            )
            .unwrap();
        let next = store
            .acquire_local_execution_lease(
                "provider:phase34",
                Some("run-phase34-next"),
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(next.generation, execution.generation + 1);
        store.settle_local_lease(&next, false).unwrap();

        let events: i64 = store
            .connection()
            .unwrap()
            .query_row("SELECT count(*) FROM local_coordination_events", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(events >= 7);
        drop(store);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn stale_process_releases_write_but_execution_becomes_hazard_and_blocks_maintenance() {
        let path = temp_state("stale");
        drop(SessionStore::open(&path).unwrap());

        let a = open_stress(&path, Duration::from_millis(40));
        let write = a
            .acquire_local_write_lease("workspace:write", Duration::from_millis(40))
            .unwrap();
        let execution = a
            .acquire_local_execution_lease(
                "effect:unknown",
                Some("run-stale"),
                Duration::from_millis(40),
            )
            .unwrap();
        assert_eq!(write.generation, 1);
        assert_eq!(execution.generation, 1);
        thread::sleep(Duration::from_millis(80));

        let b = open_stress(&path, Duration::from_secs(2));
        let write2 = b
            .acquire_local_write_lease("workspace:write", Duration::from_secs(1))
            .unwrap();
        assert_eq!(write2.generation, 2);
        assert!(b
            .acquire_local_execution_lease(
                "effect:unknown",
                Some("run-after-crash"),
                Duration::from_secs(1)
            )
            .is_err());

        assert!(b
            .acquire_local_maintenance_lease("migration", Duration::from_secs(1))
            .is_err());
        b.settle_local_lease(&write2, false).unwrap();
        assert!(b
            .acquire_local_maintenance_lease("migration", Duration::from_secs(1))
            .is_err());
        b.resolve_local_execution_hazard(
            "effect:unknown",
            execution.generation,
            "effect inspected after stale process fencing",
        )
        .unwrap();

        let maintenance = b
            .acquire_local_maintenance_lease("migration", Duration::from_secs(1))
            .unwrap();
        assert!(b
            .acquire_local_write_lease("new-write", Duration::from_secs(1))
            .is_err());
        b.release_local_maintenance_lease(&maintenance).unwrap();
        drop(a);
        drop(b);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn sqlite_busy_is_bounded_and_never_turns_into_an_effect_replay() {
        let path = temp_state("busy");
        drop(SessionStore::open(&path).unwrap());
        let a = open_stress(&path, Duration::from_secs(5));
        let b = open_stress(&path, Duration::from_secs(5));

        let mut locked = a.connection().unwrap();
        let tx = locked
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let error = b
            .acquire_local_write_lease("busy:scope", Duration::from_secs(1))
            .unwrap_err();
        assert!(matches!(
            error,
            Error::Conflict(message) if message.contains("SQLite busy/locked")
        ));
        tx.rollback().unwrap();
        drop(locked);

        let count: i64 = b
            .connection()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM local_coordination_leases WHERE scope_key='busy:scope'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        drop(a);
        drop(b);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn backup_restore_advances_generation_and_fences_pre_restore_handles() {
        let path = temp_state("restore");
        let backup = path.with_extension("backup.sqlite3");
        let store = SessionStore::open(&path).unwrap();
        let actor = actor();
        store.create(&actor, "before backup", binding()).unwrap();
        store.backup(&backup).unwrap();
        store.create(&actor, "after backup", binding()).unwrap();
        assert_eq!(store.list(&actor).unwrap().len(), 2);

        let stale = open_stress(&path, Duration::from_secs(5));
        assert_eq!(stale.local_process_info().unwrap().store_generation, 1);
        store.restore_backup(&backup).unwrap();

        assert_eq!(store.local_process_info().unwrap().store_generation, 2);
        assert_eq!(store.list(&actor).unwrap().len(), 1);
        assert!(matches!(
            stale.local_process_info(),
            Err(Error::Conflict(message)) if message.contains("store generation changed")
        ));

        drop(stale);
        drop(store);
        fs::remove_file(backup).unwrap();
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn active_writer_excludes_migration_and_backup_maintenance() {
        let path = temp_state("maintenance-writer");
        let backup = path.with_extension("maintenance-backup.sqlite3");
        let owner = SessionStore::open(&path).unwrap();
        let writer = open_stress(&path, Duration::from_secs(5));
        let write = writer
            .acquire_local_write_lease("workspace:maintenance-race", Duration::from_secs(2))
            .unwrap();

        assert!(owner.backup(&backup).is_err());
        assert!(!backup.exists());
        assert!(owner
            .acquire_local_maintenance_lease("migration", Duration::from_secs(1))
            .is_err());

        writer.settle_local_lease(&write, false).unwrap();
        let migration = owner
            .acquire_local_maintenance_lease("migration", Duration::from_secs(1))
            .unwrap();
        assert!(writer
            .acquire_local_write_lease("workspace:new-write", Duration::from_secs(1))
            .is_err());
        assert!(writer
            .create(&actor(), "must be fenced by maintenance", binding())
            .is_err());
        owner.release_local_maintenance_lease(&migration).unwrap();
        owner.backup(&backup).unwrap();

        drop(writer);
        drop(owner);
        fs::remove_file(backup).unwrap();
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn competing_exact_request_receipt_is_single_and_idempotent() {
        let path = temp_state("request-cas");
        let actor = actor();
        let owner = SessionStore::open(&path).unwrap();
        let (session, branch) = owner
            .create(&actor, "request contention", binding())
            .unwrap();
        drop(owner);

        let a = open_stress(&path, Duration::from_secs(5));
        let b = open_stress(&path, Duration::from_secs(5));
        let fingerprint = "a".repeat(64);
        let held = a.connection().unwrap();
        let blocked = b.reserve_web(
            &actor,
            "phase34-exact-request",
            &fingerprint,
            &session.id,
            &branch.id,
            branch.revision,
        );
        assert!(
            matches!(
                blocked,
                Err(Error::Conflict(
                    "local coordination lease is held by another live process"
                ))
            ),
            "{blocked:?}"
        );
        let before: i64 = held
            .query_row("SELECT count(*) FROM web_requests", [], |r| r.get(0))
            .unwrap();
        assert_eq!(before, 0);
        drop(held);
        let gate = Arc::new(Barrier::new(3));
        let ga = gate.clone();
        let aa = actor.clone();
        let sa = session.id.clone();
        let ba = branch.id.clone();
        let fa = fingerprint.clone();
        let ta = thread::spawn(move || {
            ga.wait();
            let result =
                a.reserve_web(&aa, "phase34-exact-request", &fa, &sa, &ba, branch.revision);
            (a, result)
        });
        let gb = gate.clone();
        let ab = actor.clone();
        let sb = session.id.clone();
        let bb = branch.id.clone();
        let fb = fingerprint.clone();
        let revision = branch.revision;
        let tb = thread::spawn(move || {
            gb.wait();
            let result = b.reserve_web(&ab, "phase34-exact-request", &fb, &sb, &bb, revision);
            (b, result)
        });
        gate.wait();
        let mut attempts = [ta.join().unwrap(), tb.join().unwrap()];
        // The ordinary-access lease may reject before either local transaction
        // begins. Reacquire only that exact rejection once both guards settled.
        for (store, result) in &mut attempts {
            if matches!(
                result,
                Err(Error::Conflict(
                    "local coordination lease is held by another live process"
                ))
            ) {
                *result = store.reserve_web(
                    &actor,
                    "phase34-exact-request",
                    &fingerprint,
                    &session.id,
                    &branch.id,
                    revision,
                );
            }
        }
        let results = attempts.map(|(_, result)| result);
        assert!(results.iter().all(Result::is_ok), "{results:?}");
        let [first, second] = results.map(Result::unwrap);
        assert_eq!(first.run_id, second.run_id);
        assert_eq!(first.state, "accepted");
        assert_eq!(second.state, "accepted");

        let inspect = open_stress(&path, Duration::from_secs(5));
        let receipts: i64 = inspect
            .connection()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM web_requests WHERE actor=?1 AND request_id='phase34-exact-request'",
                [actor.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(receipts, 1);
        drop(inspect);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn killed_restore_after_atomic_copy_is_already_generation_fenced() {
        let path = temp_state("restore-crash");
        let backup = path.with_extension("restore-crash-backup.sqlite3");
        let ready = path.with_extension("restore-crash-ready");
        let actor = actor();
        let owner = SessionStore::open(&path).unwrap();
        owner
            .create(&actor, "before restore snapshot", binding())
            .unwrap();
        owner.backup(&backup).unwrap();
        owner
            .create(&actor, "after restore snapshot", binding())
            .unwrap();
        drop(owner);

        let mut child = spawn_child(
            "restore_committed",
            &path,
            &ready,
            &[
                ("AW_PHASE34_RESTORE_SOURCE", backup.to_str().unwrap()),
                ("AW_PHASE34_RESTORE_COMMIT_READY", ready.to_str().unwrap()),
            ],
        );
        wait_ready(&mut child, &ready);
        child.kill().unwrap();
        child.wait().unwrap();

        // The child died after sqlite3_backup reported DONE but before its
        // in-memory SessionStore process slot could be updated. Because the
        // generation fence was staged into the copied image, restart already
        // observes generation 2 and the restored snapshot.
        let recovered = SessionStore::open(&path).unwrap();
        assert_eq!(recovered.local_process_info().unwrap().store_generation, 2);
        assert_eq!(recovered.list(&actor).unwrap().len(), 1);
        let generation: String = recovered
            .connection()
            .unwrap()
            .query_row(
                "SELECT value FROM store_meta WHERE key='restore_generation'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(generation, "2");

        drop(recovered);
        let _ = fs::remove_file(ready);
        fs::remove_file(backup).unwrap();
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn competing_prompt_admission_keeps_one_exact_branch_run() {
        let path = temp_state("prompt-cas");
        let actor = actor();
        let owner = SessionStore::open(&path).unwrap();
        let (session, branch) = owner
            .create(&actor, "prompt contention", binding())
            .unwrap();
        drop(owner);

        let a = open_stress(&path, Duration::from_secs(5));
        let b = open_stress(&path, Duration::from_secs(5));
        let (_, ha) = a.activate(&actor, &session.id, &branch.id).unwrap();
        let (_, hb) = b.activate(&actor, &session.id, &branch.id).unwrap();
        let start = TurnStart {
            turn_id: 1,
            prompt: "one prompt".into(),
            identity: identity(&actor),
        };
        let gate = Arc::new(Barrier::new(3));
        let ga = gate.clone();
        let sa = start.clone();
        let ta = thread::spawn(move || {
            let rt = runtime();
            ga.wait();
            rt.block_on(ha.begin(sa))
        });
        let gb = gate.clone();
        let sb = start.clone();
        let tb = thread::spawn(move || {
            let rt = runtime();
            gb.wait();
            rt.block_on(hb.begin(sb))
        });
        gate.wait();
        let results = [ta.join().unwrap(), tb.join().unwrap()];
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);

        let c = a.connection().unwrap();
        let runs: i64 = c
            .query_row("SELECT count(*) FROM runs", [], |r| r.get(0))
            .unwrap();
        let attempts: i64 = c
            .query_row("SELECT count(*) FROM llm_attempts", [], |r| r.get(0))
            .unwrap();
        let active: i64 = c
            .query_row(
                "SELECT count(*) FROM branches WHERE active_run IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!((runs, attempts, active), (1, 1, 1));
        drop(c);
        drop(a);
        drop(b);
        let recovered = SessionStore::open(&path).unwrap();
        let attempts_after: i64 = recovered
            .connection()
            .unwrap()
            .query_row("SELECT count(*) FROM llm_attempts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(attempts_after, 1);
        drop(recovered);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn competing_result_settlement_preserves_one_paid_result_and_one_branch_advance() {
        let path = temp_state("result-cas");
        let actor = actor();
        let owner = SessionStore::open(&path).unwrap();
        let (session, branch) = owner
            .create(&actor, "result contention", binding())
            .unwrap();
        drop(owner);

        let a = open_stress(&path, Duration::from_secs(5));
        let b = open_stress(&path, Duration::from_secs(5));
        let (_, ha) = a.activate(&actor, &session.id, &branch.id).unwrap();
        let (_, hb) = b.activate(&actor, &session.id, &branch.id).unwrap();
        runtime()
            .block_on(ha.begin(TurnStart {
                turn_id: 1,
                prompt: "settle once".into(),
                identity: identity(&actor),
            }))
            .unwrap();

        let final_turn = turn(&actor, "settle once");
        let gate = Arc::new(Barrier::new(3));
        let ga = gate.clone();
        let fa = final_turn.clone();
        let ta = thread::spawn(move || {
            let rt = runtime();
            ga.wait();
            rt.block_on(ha.finish(fa))
        });
        let gb = gate.clone();
        let fb = final_turn.clone();
        let tb = thread::spawn(move || {
            let rt = runtime();
            gb.wait();
            rt.block_on(hb.finish(fb))
        });
        gate.wait();
        let results = [ta.join().unwrap(), tb.join().unwrap()];
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);

        let c = a.connection().unwrap();
        let completed: i64 = c
            .query_row(
                "SELECT count(*) FROM runs WHERE status='completed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let final_nodes: i64 = c
            .query_row("SELECT count(*) FROM nodes WHERE kind='turn'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let attempts: i64 = c
            .query_row("SELECT count(*) FROM llm_attempts", [], |r| r.get(0))
            .unwrap();
        let (revision, active_run): (u64, Option<String>) = c
            .query_row(
                "SELECT revision,active_run FROM branches WHERE id=?1",
                [&branch.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(completed, 1);
        assert_eq!(final_nodes, 1);
        assert_eq!(attempts, 1);
        assert_eq!(revision, branch.revision + 2);
        assert!(active_run.is_none());
        drop(c);
        drop(a);
        drop(b);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn competing_context_apply_same_request_is_lookup_only_after_one_exact_head_cas() {
        let path = temp_state("context-cas");
        let actor = actor();
        let owner = SessionStore::open(&path).unwrap();
        let (source, source_branch) = owner.create(&actor, "context source", binding()).unwrap();
        let source_branch = owner
            .checkpoint(
                &actor,
                &source.id,
                &source_branch.id,
                source_branch.revision,
                "source observation",
            )
            .unwrap();
        let (target, target_branch) = owner.create(&actor, "context target", binding()).unwrap();
        let plan = owner
            .context_prepare(
                &actor,
                PrepareContext {
                    target: TargetHead::from(&target_branch),
                    sources: vec![SourceSelection {
                        session_id: source.id.clone(),
                        branch: source_branch.id.clone(),
                        head: None,
                        selection: Selection::Standalone,
                    }],
                    kind: PlanKind::Import,
                    focus: "phase34 exact apply race".into(),
                    max_input_bytes: 128 * 1024,
                    max_output_bytes: 4096,
                    timeout_seconds: 10,
                },
            )
            .unwrap();
        let artifact = owner
            .context_manual(
                &actor,
                &plan.id,
                "phase34-manual",
                "derived source observation",
                None,
            )
            .unwrap();
        drop(owner);

        let a = open_stress(&path, Duration::from_secs(5));
        let b = open_stress(&path, Duration::from_secs(5));
        let req_a = ApplyContext {
            artifact_id: artifact.id.clone(),
            target: TargetHead::from(&target_branch),
            mode: ApplyMode::Append,
            request_id: "phase34-apply-a".into(),
            confirmed_disclosure: true,
        };
        let req_b = req_a.clone();
        let held = a.connection().unwrap();
        let blocked = b.context_apply(&actor, req_a.clone());
        assert!(
            matches!(
                blocked,
                Err(Error::Conflict(
                    "local coordination lease is held by another live process"
                ))
            ),
            "{blocked:?}"
        );
        let before: i64 = held
            .query_row("SELECT count(*) FROM context_applications", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(before, 0);
        drop(held);
        let gate = Arc::new(Barrier::new(3));
        let retry_request = req_a.clone();
        let ga = gate.clone();
        let aa = actor.clone();
        let ta = thread::spawn(move || {
            ga.wait();
            let result = a.context_apply(&aa, req_a);
            (a, result)
        });
        let gb = gate.clone();
        let ab = actor.clone();
        let tb = thread::spawn(move || {
            gb.wait();
            let result = b.context_apply(&ab, req_b);
            (b, result)
        });
        gate.wait();
        let mut attempts = [ta.join().unwrap(), tb.join().unwrap()];
        // This exact conflict precedes context_apply admission. After joining
        // both callers, one explicit reacquisition can look up the same receipt.
        for (store, result) in &mut attempts {
            if matches!(
                result,
                Err(Error::Conflict(
                    "local coordination lease is held by another live process"
                ))
            ) {
                *result = store.context_apply(&actor, retry_request.clone());
            }
        }
        let results = attempts.map(|(_, result)| result);
        assert_eq!(
            results.iter().filter(|r| r.is_ok()).count(),
            2,
            "{results:?}"
        );
        let first = results[0].as_ref().unwrap();
        let second = results[1].as_ref().unwrap();
        assert_eq!(first.node_id, second.node_id);
        assert_eq!(first.branch.head, second.branch.head);
        assert_eq!(first.branch.revision, second.branch.revision);

        let inspect = open_stress(&path, Duration::from_secs(5));
        let branch_now = inspect
            .branches(&actor, &target.id)
            .unwrap()
            .into_iter()
            .find(|branch| branch.id == target_branch.id)
            .unwrap();
        assert_eq!(branch_now.revision, target_branch.revision + 1);
        let receipts: i64 = inspect
            .connection()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM context_applications WHERE request_id LIKE 'phase34-apply-%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(receipts, 1);
        drop(inspect);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn competing_workspace_writer_failover_uses_exact_writer_epoch_cas() {
        let path = temp_state("writer-cas");
        let actor = actor();
        let owner = SessionStore::open(&path).unwrap();
        let (session, _) = owner
            .create(&actor, "writer contention", binding())
            .unwrap();
        let domain = owner.default_domain(&actor).unwrap();
        let workspace = owner
            .workspace(&actor, Some(&domain.id), &session.workspace_id)
            .unwrap();
        let remote_node = "node_phase34_remote";
        {
            let c = owner.connection().unwrap();
            let store_id: String = c
                .query_row(
                    "SELECT value FROM store_meta WHERE key='store_id'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            c.execute(
                "INSERT INTO awnet_peers(peer_id,node_id,public_key,display_name,trust_state,created_at)
                 VALUES('peer_phase34',?1,?2,'phase34 remote','trusted',1)",
                params![remote_node, "11".repeat(32)],
            )
            .unwrap();
            c.execute(
                "INSERT INTO machines(machine_id,authority_store_id,owning_domain_id,node_id,display_name,kind,key_fingerprint,status,labels_json,capabilities_json,created_at,last_seen_at)
                 VALUES('machine_phase34',?1,?2,?3,'phase34 remote','remote',?4,'active','{}','{}',1,1)",
                params![store_id, domain.id, remote_node, "22".repeat(32)],
            )
            .unwrap();
        }
        drop(owner);

        let a = open_stress(&path, Duration::from_secs(5));
        let b = open_stress(&path, Duration::from_secs(5));
        let gate = Arc::new(Barrier::new(3));
        let ga = gate.clone();
        let aa = actor.clone();
        let domain_a = domain.id.clone();
        let ws_a = workspace.id.clone();
        let ta = thread::spawn(move || {
            ga.wait();
            a.failover_workspace_writer(
                &aa,
                Some(&domain_a),
                &ws_a,
                1,
                remote_node,
                "phase34_race_a",
            )
        });
        let gb = gate.clone();
        let ab = actor.clone();
        let domain_b = domain.id.clone();
        let ws_b = workspace.id.clone();
        let tb = thread::spawn(move || {
            gb.wait();
            b.failover_workspace_writer(
                &ab,
                Some(&domain_b),
                &ws_b,
                1,
                remote_node,
                "phase34_race_b",
            )
        });
        gate.wait();
        let results = [ta.join().unwrap(), tb.join().unwrap()];
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);

        let inspect = open_stress(&path, Duration::from_secs(5));
        let writer = inspect
            .workspace_writer(&actor, Some(&domain.id), &workspace.id)
            .unwrap();
        assert_eq!(writer.writer_epoch, 2);
        assert_eq!(writer.writer_node_id, remote_node);
        let events = inspect
            .workspace_writer_events(&actor, Some(&domain.id), &workspace.id)
            .unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|e| e.reason.starts_with("phase34_race_"))
                .count(),
            1
        );
        drop(inspect);
        fs::remove_dir_all(path).unwrap();
    }

    fn spawn_child(action: &str, path: &Path, ready: &Path, extra: &[(&str, &str)]) -> Child {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .arg("coordination::tests::phase34_child_process_helper")
            .arg("--exact")
            .arg("--nocapture")
            .env("AW_PHASE34_CHILD_ACTION", action)
            .env("AW_PHASE34_CHILD_STATE", path)
            .env("AW_PHASE34_CHILD_READY", ready);
        for (key, value) in extra {
            command.env(key, value);
        }
        command.spawn().unwrap()
    }

    fn wait_ready(child: &mut Child, ready: &Path) -> String {
        for _ in 0..1000 {
            if let Ok(value) = fs::read_to_string(ready) {
                if !value.trim().is_empty() {
                    return value;
                }
            }
            if let Some(status) = child.try_wait().unwrap() {
                panic!("phase34 child exited before ready: {status}");
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("phase34 child never reached durable crash boundary");
    }

    fn publish_ready(ready: &Path, payload: &[u8]) -> std::io::Result<()> {
        publish_ready_with_hook(ready, payload, || {})
    }

    fn publish_ready_with_hook<F: FnOnce()>(
        ready: &Path,
        payload: &[u8],
        after_create: F,
    ) -> std::io::Result<()> {
        use std::io::Write;
        // These paths already include the fixture's random identity. Include
        // the publishing child too; create_new never adopts an old partial.
        let staged = ready.with_extension(format!("ready-partial-{}", std::process::id()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)?;
        after_create();
        file.write_all(payload)?;
        drop(file);
        fs::rename(staged, ready)?;
        Ok(())
    }

    #[test]
    fn aw_p6_w8e_readiness_is_hidden_until_payload_is_complete() {
        let parent = temp_state("readiness-publication");
        aw_platform::ensure_private_dir(&parent).unwrap();
        let ready = parent.join("ready");
        let result = std::panic::catch_unwind(|| {
            publish_ready_with_hook(&ready, b"123456789", || {
                assert!(
                    !ready.exists(),
                    "empty readiness file became visible before publication"
                );
            })
            .unwrap();
            assert_eq!(fs::read(&ready).unwrap(), b"123456789");
        });
        fs::remove_dir_all(parent).unwrap();
        result.unwrap();
    }

    #[test]
    fn aw_p6_w8e_parent_waits_for_complete_child_readiness() {
        let path = temp_state("readiness-empty");
        let ready = path.with_extension("execution-ready");
        drop(SessionStore::open(&path).unwrap());
        fs::write(&ready, b"").unwrap();
        let mut child = spawn_child("execution_lease", &path, &ready, &[]);
        let staged = ready.with_extension(format!("ready-partial-{}", child.id()));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            wait_ready(&mut child, &ready)
        }));
        // Reap the disposable child even when the readiness assertion fails.
        let _ = child.kill();
        child.wait().unwrap();
        fs::remove_file(&ready).unwrap();
        let _ = fs::remove_file(staged);
        fs::remove_dir_all(path).unwrap();
        assert_eq!(result.unwrap().trim(), "1");
    }

    #[test]
    fn killed_trusted_process_execution_lease_becomes_hazard_until_reconciled() {
        let path = temp_state("execution-crash");
        let ready = path.with_extension("execution-ready");
        drop(SessionStore::open(&path).unwrap());

        let mut child = spawn_child("execution_lease", &path, &ready, &[]);
        let generation: u64 = wait_ready(&mut child, &ready).trim().parse().unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
        thread::sleep(Duration::from_millis(120));

        let owner = SessionStore::open(&path).unwrap();
        assert!(owner
            .acquire_local_execution_lease(
                "effect:child-crash",
                Some("run-after-child-crash"),
                Duration::from_secs(1),
            )
            .is_err());
        owner
            .resolve_local_execution_hazard(
                "effect:child-crash",
                generation,
                "killed process effect was explicitly reconciled",
            )
            .unwrap();
        let next = owner
            .acquire_local_execution_lease(
                "effect:child-crash",
                Some("run-after-reconcile"),
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(next.generation, generation + 1);
        owner.settle_local_lease(&next, false).unwrap();

        drop(owner);
        let _ = fs::remove_file(ready);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn killed_process_after_provider_dispatch_intent_recovers_unknown_without_replay() {
        let path = temp_state("provider-crash");
        let ready = path.with_extension("provider-ready");
        let effect = path.with_extension("provider-effect");
        let actor = actor();
        let owner = SessionStore::open(&path).unwrap();
        let (session, branch) = owner.create(&actor, "provider crash", binding()).unwrap();
        drop(owner);

        let mut child = spawn_child(
            "provider_started",
            &path,
            &ready,
            &[
                ("AW_PHASE34_SESSION", &session.id),
                ("AW_PHASE34_BRANCH", &branch.id),
                ("AW_PHASE34_ACTOR", actor.as_str()),
                ("AW_PHASE34_EXTERNAL_EFFECT", effect.to_str().unwrap()),
            ],
        );
        wait_ready(&mut child, &ready);
        child.kill().unwrap();
        child.wait().unwrap();

        let recovered = SessionStore::open(&path).unwrap();
        let c = recovered.connection().unwrap();
        let attempts: i64 = c
            .query_row("SELECT count(*) FROM llm_attempts", [], |r| r.get(0))
            .unwrap();
        let dispatch: String = c
            .query_row("SELECT dispatch_state FROM llm_attempts LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        let operation_state: String = c
            .query_row("SELECT state FROM llm_operations LIMIT 1", [], |r| r.get(0))
            .unwrap();
        let retry_state: String = c
            .query_row("SELECT state FROM retry_schedules LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        let provider_lease: (String, String) = c
            .query_row(
                "SELECT scope_key,state FROM local_coordination_leases
                 WHERE scope_kind='execution' AND run_id IS NOT NULL AND scope_key LIKE 'provider:%'
                 LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let reconciled: i64 = c
            .query_row(
                "SELECT count(*) FROM local_coordination_events
                 WHERE kind='lease.provider_reconciled'
                   AND json_extract(payload_json,'$.prior_state')='hazard'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(attempts, 1);
        assert_eq!(dispatch, "unknown");
        assert_eq!(operation_state, "interrupted");
        assert_eq!(retry_state, "waiting_user");
        assert!(provider_lease.0.starts_with("provider:"));
        assert_eq!(provider_lease.1, "released");
        assert_eq!(reconciled, 1);
        assert_eq!(fs::read_to_string(&effect).unwrap(), "provider-effect-once");
        drop(c);
        drop(recovered);
        let _ = fs::remove_file(ready);
        let _ = fs::remove_file(effect);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn killed_process_after_result_commit_before_branch_attach_recovers_without_replay() {
        let path = temp_state("result-commit-crash");
        let ready = path.with_extension("result-commit-ready");
        let actor = actor();
        let owner = SessionStore::open(&path).unwrap();
        let (session, branch) = owner
            .create(&actor, "result commit crash", binding())
            .unwrap();
        drop(owner);

        let mut child = spawn_child(
            "result_committed",
            &path,
            &ready,
            &[
                ("AW_PHASE34_SESSION", &session.id),
                ("AW_PHASE34_BRANCH", &branch.id),
                ("AW_PHASE34_ACTOR", actor.as_str()),
                ("AW_PHASE34_RESULT_COMMIT_READY", ready.to_str().unwrap()),
            ],
        );
        wait_ready(&mut child, &ready);
        child.kill().unwrap();
        child.wait().unwrap();

        // The result transaction committed before the branch-attachment
        // transaction. Restart must attach/reconcile the durable result, not
        // dispatch a second attempt.
        let recovered = SessionStore::open(&path).unwrap();
        let c = recovered.connection().unwrap();
        let (status, attach_status): (String, String) = c
            .query_row("SELECT status,attach_status FROM runs LIMIT 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        let attempts: i64 = c
            .query_row("SELECT count(*) FROM llm_attempts", [], |r| r.get(0))
            .unwrap();
        let final_nodes: i64 = c
            .query_row("SELECT count(*) FROM nodes WHERE kind='turn'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let retry_rows: i64 = c
            .query_row("SELECT count(*) FROM retry_schedules", [], |r| r.get(0))
            .unwrap();
        let current: (String, u64, Option<String>) = c
            .query_row(
                "SELECT head,revision,active_run FROM branches WHERE id=?1",
                [&branch.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(status, "completed");
        assert_eq!(attach_status, "attached");
        assert_eq!(attempts, 1);
        assert_eq!(final_nodes, 1);
        assert_eq!(retry_rows, 0);
        assert_eq!(current.1, branch.revision + 2);
        assert!(current.2.is_none());
        drop(c);
        drop(recovered);
        let _ = fs::remove_file(ready);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn killed_process_before_wal_commit_rolls_back_partial_branch_mutation() {
        let path = temp_state("wal-crash");
        let ready = path.with_extension("wal-ready");
        let actor = actor();
        let owner = SessionStore::open(&path).unwrap();
        let (session, branch) = owner.create(&actor, "wal crash", binding()).unwrap();
        drop(owner);

        let mut child = spawn_child(
            "wal_uncommitted",
            &path,
            &ready,
            &[("AW_PHASE34_BRANCH", &branch.id)],
        );
        wait_ready(&mut child, &ready);
        child.kill().unwrap();
        child.wait().unwrap();

        let recovered = SessionStore::open(&path).unwrap();
        let current = recovered
            .branches(&actor, &session.id)
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == branch.id)
            .unwrap();
        assert_eq!(current.revision, branch.revision);
        assert_eq!(current.head, branch.head);
        let check: String = recovered
            .connection()
            .unwrap()
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .unwrap();
        assert_eq!(check, "ok");
        drop(recovered);
        let _ = fs::remove_file(ready);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn phase35_direct_mode_refuses_non_current_schema_without_migrating() {
        let path = temp_state("phase35-current-schema-only");
        let owner = SessionStore::open(&path).unwrap();
        drop(owner);

        {
            let db_path = path.join("sessions.sqlite3");
            let conn = Connection::open(&db_path).unwrap();
            conn.pragma_update(None, "user_version", 19).unwrap();
        }
        let error = match SessionStore::open_trusted_local_direct(
            &path,
            crate::TrustedLocalDirectOptions {
                heartbeat_ttl: Duration::from_secs(3),
            },
        ) {
            Ok(_) => panic!("direct mode unexpectedly migrated/opened an old schema"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("current aw schema"));
        let conn = Connection::open(path.join("sessions.sqlite3")).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 19);
        drop(conn);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn phase35_direct_peer_does_not_recover_live_run_but_recovers_stopped_owner() {
        let path = temp_state("phase35-live-run-owner");
        let actor = actor();
        let owner = SessionStore::open(&path).unwrap();
        let (session, branch) = owner
            .create(&actor, "phase35 direct recovery", binding())
            .unwrap();
        drop(owner);

        let direct_a = SessionStore::open_trusted_local_direct(
            &path,
            crate::TrustedLocalDirectOptions {
                heartbeat_ttl: Duration::from_secs(3),
            },
        )
        .unwrap();
        let process_a = direct_a.local_process_info().unwrap();
        let (_, handle) = direct_a.activate(&actor, &session.id, &branch.id).unwrap();
        let who = identity(&actor);
        runtime().block_on(async {
            handle
                .begin(TurnStart {
                    turn_id: 1,
                    prompt: "phase35 live owner".into(),
                    identity: who.clone(),
                })
                .await
                .unwrap();
            handle
                .append(
                    1,
                    JournalEntry::InvocationStarted {
                        ordinal: 1,
                        identity: who,
                    },
                )
                .await
                .unwrap();
        });

        let direct_b = SessionStore::open_trusted_local_direct(
            &path,
            crate::TrustedLocalDirectOptions {
                heartbeat_ttl: Duration::from_secs(3),
            },
        )
        .unwrap();
        let (status, owner_process): (String, String) = direct_b
            .connection()
            .unwrap()
            .query_row(
                "SELECT status,owner_process_id FROM runs WHERE session_id=?1",
                [&session.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "running");
        assert_eq!(owner_process, process_a.process_id);

        direct_b.recover().unwrap();
        let still_running: i64 = direct_b
            .connection()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM runs WHERE session_id=?1 AND status='running'",
                [&session.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(still_running, 1, "live peer run must not be recovered");

        drop(handle);
        drop(direct_a);
        direct_b.recover().unwrap();

        let recovered: (String, String, String) = direct_b
            .connection()
            .unwrap()
            .query_row(
                "SELECT r.status,i.state,a.dispatch_state
                 FROM runs r
                 JOIN invocation_receipts i ON i.run_id=r.id
                 JOIN llm_attempts a ON a.run_id=r.id
                 WHERE r.session_id=?1",
                [&session.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(recovered.0, "interrupted");
        assert_eq!(recovered.1, "unknown");
        assert_eq!(recovered.2, "unknown");
        let provider_leases: i64 = direct_b
            .connection()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM local_coordination_leases
                 WHERE scope_kind='execution' AND scope_key LIKE 'provider:%'
                   AND state IN ('active','hazard')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(provider_leases, 0);
        drop(direct_b);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn phase35_direct_clients_share_access_fencing_and_exclude_owner() {
        let path = temp_state("phase35-access-fence");
        let actor = actor();
        let owner = SessionStore::open(&path).unwrap();
        owner.create(&actor, "phase35 access", binding()).unwrap();
        drop(owner);

        let direct_a = SessionStore::open_trusted_local_direct(
            &path,
            crate::TrustedLocalDirectOptions {
                heartbeat_ttl: Duration::from_secs(3),
            },
        )
        .unwrap();
        let direct_b = SessionStore::open_trusted_local_direct(
            &path,
            crate::TrustedLocalDirectOptions {
                heartbeat_ttl: Duration::from_secs(3),
            },
        )
        .unwrap();

        assert!(
            SessionStore::open(&path).is_err(),
            "owner/service mode must be excluded while shared direct holders exist"
        );

        let held = direct_a.connection().unwrap();
        assert!(direct_b.list(&actor).is_err());
        assert!(direct_b
            .acquire_local_maintenance_lease("maintenance", Duration::from_secs(1))
            .is_err());
        drop(held);

        assert_eq!(direct_b.list(&actor).unwrap().len(), 1);
        let maintenance = direct_a
            .acquire_local_maintenance_lease("maintenance", Duration::from_secs(1))
            .unwrap();
        assert!(direct_b.list(&actor).is_err());
        direct_a
            .release_local_maintenance_lease(&maintenance)
            .unwrap();
        assert_eq!(direct_b.list(&actor).unwrap().len(), 1);

        drop(direct_b);
        drop(direct_a);
        let owner = SessionStore::open(&path).unwrap();
        assert_eq!(owner.list(&actor).unwrap().len(), 1);
        drop(owner);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn phase35_direct_peer_preserves_live_web_receipt_and_interrupts_only_after_stop() {
        let path = temp_state("phase35-web-owner");
        let actor = actor();
        let owner = SessionStore::open(&path).unwrap();
        let (session, branch) = owner
            .create(&actor, "phase35 web ownership", binding())
            .unwrap();
        drop(owner);

        let direct_a = SessionStore::open_trusted_local_direct(
            &path,
            crate::TrustedLocalDirectOptions {
                heartbeat_ttl: Duration::from_secs(3),
            },
        )
        .unwrap();
        let receipt = direct_a
            .reserve_web(
                &actor,
                "phase35-request",
                &"a".repeat(64),
                &session.id,
                &branch.id,
                branch.revision,
            )
            .unwrap();
        assert_eq!(receipt.state, "accepted");

        let direct_b = SessionStore::open_trusted_local_direct(
            &path,
            crate::TrustedLocalDirectOptions {
                heartbeat_ttl: Duration::from_secs(3),
            },
        )
        .unwrap();
        direct_b.recover().unwrap();
        let live = direct_b
            .lookup_web(&actor, "phase35-request", &"a".repeat(64))
            .unwrap()
            .unwrap();
        assert_eq!(live.state, "accepted");

        drop(direct_a);
        direct_b.recover().unwrap();
        let interrupted = direct_b
            .lookup_web(&actor, "phase35-request", &"a".repeat(64))
            .unwrap()
            .unwrap();
        assert_eq!(interrupted.state, "interrupted");

        drop(direct_b);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn phase35_owner_immediately_recovers_killed_direct_process_from_lock_proof() {
        let path = temp_state("phase35-owner-after-direct-kill");
        let ready = path.with_extension("phase35-direct-ready");
        let actor = actor();
        let bootstrap = SessionStore::open(&path).unwrap();
        let (session, branch) = bootstrap
            .create(&actor, "phase35 killed direct", binding())
            .unwrap();
        drop(bootstrap);

        let mut child = spawn_child(
            "phase35_direct_provider_started",
            &path,
            &ready,
            &[
                ("AW_PHASE34_ACTOR", actor.as_str()),
                ("AW_PHASE34_SESSION", &session.id),
                ("AW_PHASE34_BRANCH", &branch.id),
            ],
        );
        wait_ready(&mut child, &ready);
        child.kill().unwrap();
        child.wait().unwrap();

        let owner = SessionStore::open(&path).unwrap();
        let recovered: (String, String, String, i64) = owner
            .connection()
            .unwrap()
            .query_row(
                "SELECT r.status,i.state,a.dispatch_state,
                        (SELECT count(*) FROM llm_attempts aa WHERE aa.run_id=r.id)
                 FROM runs r
                 JOIN invocation_receipts i ON i.run_id=r.id
                 JOIN llm_attempts a ON a.run_id=r.id
                 WHERE r.session_id=?1",
                [&session.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(recovered.0, "interrupted");
        assert_eq!(recovered.1, "unknown");
        assert_eq!(recovered.2, "unknown");
        assert_eq!(recovered.3, 1);
        let fenced_direct: i64 = owner
            .connection()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM local_process_incarnations
                 WHERE process_kind='trusted_local' AND state='fenced'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(fenced_direct >= 1);

        drop(owner);
        let _ = fs::remove_file(ready);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn phase35_direct_heartbeat_renews_process_incarnation() {
        let path = temp_state("phase35-heartbeat");
        let owner = SessionStore::open(&path).unwrap();
        drop(owner);
        let direct = SessionStore::open_trusted_local_direct(
            &path,
            crate::TrustedLocalDirectOptions {
                heartbeat_ttl: Duration::from_secs(3),
            },
        )
        .unwrap();
        let before = direct.local_process_info().unwrap();
        thread::sleep(Duration::from_millis(1500));
        let after = direct.local_process_info().unwrap();
        assert_eq!(before.process_id, after.process_id);
        assert!(after.last_seen_at > before.last_seen_at);
        assert!(after.lease_expires_at > before.lease_expires_at);
        drop(direct);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn phase34_child_process_helper() {
        let Ok(action) = std::env::var("AW_PHASE34_CHILD_ACTION") else {
            return;
        };
        let path = PathBuf::from(std::env::var("AW_PHASE34_CHILD_STATE").unwrap());
        let ready = PathBuf::from(std::env::var("AW_PHASE34_CHILD_READY").unwrap());
        match action.as_str() {
            "execution_lease" => {
                let store = SessionStore::open_coordinated_for_phase34_stress(
                    &path,
                    Duration::from_millis(60),
                )
                .unwrap();
                let lease = store
                    .acquire_local_execution_lease(
                        "effect:child-crash",
                        Some("run-child-crash"),
                        Duration::from_millis(60),
                    )
                    .unwrap();
                publish_ready(&ready, lease.generation.to_string().as_bytes()).unwrap();
                thread::sleep(Duration::from_secs(60));
            }
            "phase35_direct_provider_started" => {
                let store = SessionStore::open_trusted_local_direct(
                    &path,
                    crate::TrustedLocalDirectOptions {
                        heartbeat_ttl: Duration::from_secs(30),
                    },
                )
                .unwrap();
                let actor = PrincipalId::new(std::env::var("AW_PHASE34_ACTOR").unwrap()).unwrap();
                let session = std::env::var("AW_PHASE34_SESSION").unwrap();
                let branch = std::env::var("AW_PHASE34_BRANCH").unwrap();
                let (_, handle) = store.activate(&actor, &session, &branch).unwrap();
                let who = identity(&actor);
                runtime().block_on(async {
                    handle
                        .begin(TurnStart {
                            turn_id: 1,
                            prompt: "phase35 direct crash boundary".into(),
                            identity: who.clone(),
                        })
                        .await
                        .unwrap();
                    handle
                        .append(
                            1,
                            JournalEntry::InvocationStarted {
                                ordinal: 1,
                                identity: who,
                            },
                        )
                        .await
                        .unwrap();
                });
                publish_ready(&ready, b"phase35_direct_provider_started").unwrap();
                thread::sleep(Duration::from_secs(60));
            }
            "provider_started" => {
                let store = SessionStore::open(&path).unwrap();
                let actor = PrincipalId::new(std::env::var("AW_PHASE34_ACTOR").unwrap()).unwrap();
                let session = std::env::var("AW_PHASE34_SESSION").unwrap();
                let branch = std::env::var("AW_PHASE34_BRANCH").unwrap();
                let (_, handle) = store.activate(&actor, &session, &branch).unwrap();
                let who = identity(&actor);
                runtime().block_on(async {
                    handle
                        .begin(TurnStart {
                            turn_id: 1,
                            prompt: "provider crash boundary".into(),
                            identity: who.clone(),
                        })
                        .await
                        .unwrap();
                    handle
                        .append(
                            1,
                            JournalEntry::InvocationStarted {
                                ordinal: 1,
                                identity: who,
                            },
                        )
                        .await
                        .unwrap();
                });
                if let Ok(effect) = std::env::var("AW_PHASE34_EXTERNAL_EFFECT") {
                    fs::write(effect, b"provider-effect-once").unwrap();
                }
                publish_ready(&ready, b"provider_started").unwrap();
                thread::sleep(Duration::from_secs(60));
            }
            "result_committed" => {
                let store = SessionStore::open(&path).unwrap();
                let actor = PrincipalId::new(std::env::var("AW_PHASE34_ACTOR").unwrap()).unwrap();
                let session = std::env::var("AW_PHASE34_SESSION").unwrap();
                let branch = std::env::var("AW_PHASE34_BRANCH").unwrap();
                let (_, handle) = store.activate(&actor, &session, &branch).unwrap();
                let who = identity(&actor);
                runtime().block_on(async {
                    handle
                        .begin(TurnStart {
                            turn_id: 1,
                            prompt: "result commit boundary".into(),
                            identity: who.clone(),
                        })
                        .await
                        .unwrap();
                    handle
                        .append(
                            1,
                            JournalEntry::InvocationStarted {
                                ordinal: 1,
                                identity: who.clone(),
                            },
                        )
                        .await
                        .unwrap();
                    // A durable response in both the ORIGINAL immutable
                    // InvocationFinished and its final Turn projection is
                    // required for the killed-after-commit/attachment crash
                    // witness. Omitting it would erase already-paid usage.
                    let received = aw_agent_core::InvocationRecord {
                        ordinal: 1,
                        identity: who,
                        meta: Some(aw_agent_core::CompletionMeta {
                            finish_reason: "stop".into(),
                            ..Default::default()
                        }),
                        outcome: "response_received".into(),
                        usage_verified: false,
                    };
                    handle
                        .append(
                            1,
                            JournalEntry::InvocationFinished {
                                record: received.clone(),
                            },
                        )
                        .await
                        .unwrap();
                    let mut finished = turn(&actor, "result commit boundary");
                    finished.invocations.push(received);
                    handle.finish(finished).await.unwrap();
                });
                panic!("result-commit crash hook unexpectedly returned");
            }
            "restore_committed" => {
                let source = PathBuf::from(std::env::var("AW_PHASE34_RESTORE_SOURCE").unwrap());
                let store = SessionStore::open(&path).unwrap();
                store.restore_backup(&source).unwrap();
                panic!("restore-commit crash hook unexpectedly returned");
            }
            "wal_uncommitted" => {
                let store = SessionStore::open(&path).unwrap();
                let branch = std::env::var("AW_PHASE34_BRANCH").unwrap();
                let mut c = store.connection().unwrap();
                let tx = c
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .unwrap();
                tx.execute(
                    "UPDATE branches SET revision=revision+77 WHERE id=?1",
                    [&branch],
                )
                .unwrap();
                publish_ready(&ready, b"wal_uncommitted").unwrap();
                thread::sleep(Duration::from_secs(60));
                drop(tx);
            }
            other => panic!("unknown Phase34 child action: {other}"),
        }
    }
}
