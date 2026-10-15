use super::*;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

const MAX_SCHEDULES_PER_USER: i64 = 1024;
const MAX_OCCURRENCES_PER_SCHEDULE: i64 = 100_000;
const MAX_DUE_SCAN: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleOverlapPolicy {
    Skip,
    Serial,
}

impl ScheduleOverlapPolicy {
    fn as_str(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Serial => "serial",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "skip" => Ok(Self::Skip),
            "serial" => Ok(Self::Serial),
            _ => Err(Error::Storage("invalid stored schedule overlap policy")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineScheduleInfo {
    pub id: String,
    pub name: String,
    pub pipeline_id: String,
    pub revision_id: String,
    pub binding_id: String,
    pub input: String,
    pub interval_seconds: u64,
    pub overlap_policy: ScheduleOverlapPolicy,
    pub state: String,
    pub generation: u64,
    pub next_due_at: i64,
    pub next_occurrence_no: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineScheduleOccurrenceInfo {
    pub id: String,
    pub schedule_id: String,
    pub occurrence_no: u64,
    pub due_at: i64,
    pub state: String,
    pub claim_generation: u64,
    pub claimed_by: String,
    pub request_id: String,
    pub pipeline_run_id: Option<String>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineScheduleClaim {
    pub schedule: PipelineScheduleInfo,
    pub occurrence: PipelineScheduleOccurrenceInfo,
}

fn validate_schedule_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(Error::Config(
            "schedule names must be portable 1..128 byte labels".into(),
        ));
    }
    Ok(())
}

fn advance_interval_due(current_due: i64, interval_seconds: u64, at: i64) -> i64 {
    let interval = (interval_seconds as i64).saturating_mul(1_000_000);
    let nominal = current_due.saturating_add(interval);
    if nominal <= at {
        at.saturating_add(interval)
    } else {
        nominal
    }
}

fn validate_client_id(client: &str) -> Result<()> {
    if client.is_empty()
        || client.len() > 128
        || !client
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
    {
        return Err(Error::Config("invalid schedule claim client ID".into()));
    }
    Ok(())
}

fn schedule_row(c: &Connection, user: &str, selector: &str) -> Result<PipelineScheduleInfo> {
    let row: Option<(
        String,
        String,
        String,
        String,
        String,
        Vec<u8>,
        i64,
        String,
        String,
        i64,
        i64,
        i64,
    )> = db(c
        .query_row(
            "SELECT id,name,pipeline_id,revision_id,binding_id,input_utf8,interval_seconds,             overlap_policy,state,generation,next_due_at,next_occurrence_no              FROM pipeline_schedules WHERE actor_user_id=?1 AND (id=?2 OR name=?2)",
            params![user, selector],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                    r.get(11)?,
                ))
            },
        )
        .optional())?;
    let Some((
        id,
        name,
        pipeline_id,
        revision_id,
        binding_id,
        input,
        interval_seconds,
        overlap_policy,
        state,
        generation,
        next_due_at,
        next_occurrence_no,
    )) = row
    else {
        return Err(Error::Denied("schedule unavailable or not permitted"));
    };
    let input = String::from_utf8(input)
        .map_err(|_| Error::Storage("stored schedule input is not UTF-8"))?;
    Ok(PipelineScheduleInfo {
        id,
        name,
        pipeline_id,
        revision_id,
        binding_id,
        input,
        interval_seconds: interval_seconds as u64,
        overlap_policy: ScheduleOverlapPolicy::parse(&overlap_policy)?,
        state,
        generation: generation as u64,
        next_due_at,
        next_occurrence_no: next_occurrence_no as u64,
    })
}

fn occurrence_row(c: &Connection, id: &str) -> Result<PipelineScheduleOccurrenceInfo> {
    let row: Option<(String, i64, i64, String, i64, String, String, Option<String>, Option<String>)> =
        db(c
            .query_row(
                "SELECT schedule_id,occurrence_no,due_at,state,claim_generation,claimed_by,                 request_id,pipeline_run_id,reason                  FROM pipeline_schedule_occurrences WHERE id=?1",
                [id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                    ))
                },
            )
            .optional())?;
    let Some((
        schedule_id,
        occurrence_no,
        due_at,
        state,
        claim_generation,
        claimed_by,
        request_id,
        pipeline_run_id,
        reason,
    )) = row
    else {
        return Err(Error::Denied("schedule occurrence unavailable"));
    };
    Ok(PipelineScheduleOccurrenceInfo {
        id: id.to_owned(),
        schedule_id,
        occurrence_no: occurrence_no as u64,
        due_at,
        state,
        claim_generation: claim_generation as u64,
        claimed_by,
        request_id,
        pipeline_run_id,
        reason,
    })
}

fn stable_actor_user(c: &Connection, actor: &PrincipalId) -> Result<String> {
    ensure_user_for_principal(c, actor)
}

fn claimant_process_live(c: &Connection, claimant: &str, at: i64) -> Result<Option<bool>> {
    let row: Option<(u64, String, Option<i64>)> = db(c
        .query_row(
            "SELECT store_generation,state,lease_expires_at FROM local_process_incarnations WHERE process_id=?1",
            [claimant],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional())?;
    let Some((generation, state, expires_at)) = row else {
        return Ok(None);
    };
    let current_generation: String = db(c.query_row(
        "SELECT value FROM store_meta WHERE key='restore_generation'",
        [],
        |r| r.get(0),
    ))?;
    let current_generation = current_generation
        .parse::<u64>()
        .map_err(|_| Error::Storage("invalid restore generation"))?;
    Ok(Some(
        generation == current_generation
            && state == "active"
            && expires_at.is_none_or(|expires| expires > at),
    ))
}

fn claim_recoverable(
    c: &Connection,
    claimed_by: &str,
    current_client: &str,
    at: i64,
    owner_may_recover_unknown: bool,
) -> Result<bool> {
    if claimed_by == current_client {
        return Ok(true);
    }
    match claimant_process_live(c, claimed_by, at)? {
        Some(live) => Ok(!live),
        None => Ok(owner_may_recover_unknown),
    }
}

fn schedule_has_active_occurrence(c: &Connection, schedule_id: &str) -> Result<bool> {
    let row: Option<(String, String, Option<String>)> = db(c
        .query_row(
            "SELECT id,state,pipeline_run_id FROM pipeline_schedule_occurrences WHERE schedule_id=?1 AND state IN ('claimed','admitted') ORDER BY occurrence_no DESC LIMIT 1",
            [schedule_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional())?;
    let Some((occurrence_id, state, run_id)) = row else {
        return Ok(false);
    };
    if state == "claimed" {
        return Ok(true);
    }
    let Some(run_id) = run_id else {
        return Ok(true);
    };
    let run_state: Option<String> = db(c
        .query_row(
            "SELECT state FROM pipeline_runs WHERE id=?1",
            [run_id],
            |r| r.get(0),
        )
        .optional())?;
    match run_state.as_deref() {
        Some("admitted" | "running_external" | "waiting_approval") => Ok(true),
        Some("completed") => {
            db(c.execute(
                "UPDATE pipeline_schedule_occurrences SET state='completed',updated_at=?1 WHERE id=?2 AND state='admitted'",
                params![now(), occurrence_id],
            ))?;
            Ok(false)
        }
        Some("failed" | "cancelled") => {
            db(c.execute(
                "UPDATE pipeline_schedule_occurrences SET state='failed',reason=?1,updated_at=?2 WHERE id=?3 AND state='admitted'",
                params![run_state, now(), occurrence_id],
            ))?;
            Ok(false)
        }
        _ => Ok(true),
    }
}

impl SessionStore {
    pub fn schedule_define(
        &self,
        actor: &PrincipalId,
        name: &str,
        binding_id: &str,
        input: &str,
        interval_seconds: u64,
        overlap_policy: ScheduleOverlapPolicy,
        delay_seconds: u64,
    ) -> Result<PipelineScheduleInfo> {
        validate_schedule_name(name)?;
        if !(60..=2_592_000).contains(&interval_seconds) {
            return Err(Error::Config(
                "schedule interval_seconds must be 60..=2592000".into(),
            ));
        }
        if delay_seconds > 2_592_000 {
            return Err(Error::Config(
                "schedule delay_seconds must be 0..=2592000".into(),
            ));
        }

        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = stable_actor_user(&tx, actor)?;
        let row: Option<(String, String, String, Option<String>, String, String, String)> = db(tx
            .query_row(
                "SELECT b.pipeline_id,b.revision_id,b.scope_kind,b.scope_user_id,b.mode,b.status,                 p.owner_user_id                  FROM pipeline_bindings b JOIN pipelines p ON p.id=b.pipeline_id WHERE b.id=?1",
                [binding_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                },
            )
            .optional())?;
        let Some((pipeline_id, revision_id, scope_kind, scope_user_id, mode, status, owner)) = row
        else {
            return Err(Error::Denied("pipeline binding unavailable"));
        };
        if owner != user
            || scope_kind != "user"
            || scope_user_id.as_deref() != Some(user.as_str())
            || mode != "task"
            || status != "active"
        {
            return Err(Error::Denied(
                "schedule requires an active caller-owned user-scoped task binding",
            ));
        }

        let definition_json: String = db(tx.query_row(
            "SELECT definition_json FROM pipeline_revisions WHERE revision_id=?1 AND pipeline_id=?2",
            params![revision_id, pipeline_id],
            |r| r.get(0),
        ))?;
        let definition: aw_pipeline::PipelineDefinition = serde_json::from_str(&definition_json)
            .map_err(|_| Error::Storage("stored pipeline definition is invalid"))?;
        if input.len() > definition.limits.max_input_bytes as usize {
            return Err(Error::Limit(
                "schedule input exceeds pipeline revision limit",
            ));
        }

        let count: i64 = db(tx.query_row(
            "SELECT count(*) FROM pipeline_schedules WHERE actor_user_id=?1",
            [user.as_str()],
            |r| r.get(0),
        ))?;
        if count >= MAX_SCHEDULES_PER_USER {
            return Err(Error::Limit("schedule retention limit reached"));
        }

        let id = id("sched")?;
        let at = now();
        let next_due_at = at.saturating_add((delay_seconds as i64).saturating_mul(1_000_000));
        db(tx.execute(
            "INSERT INTO pipeline_schedules(             id,actor_user_id,name,pipeline_id,revision_id,binding_id,input_utf8,interval_seconds,             overlap_policy,state,generation,next_due_at,next_occurrence_no,created_at,updated_at)             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'active',1,?10,1,?11,?11)",
            params![
                id,
                user,
                name,
                pipeline_id,
                revision_id,
                binding_id,
                input.as_bytes(),
                interval_seconds as i64,
                overlap_policy.as_str(),
                next_due_at,
                at
            ],
        ))?;
        let out = schedule_row(&tx, &stable_actor_user(&tx, actor)?, &id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn schedules(&self, actor: &PrincipalId) -> Result<Vec<PipelineScheduleInfo>> {
        let c = self.connection()?;
        let user = stable_actor_user(&c, actor)?;
        let mut q = db(c.prepare(
            "SELECT id FROM pipeline_schedules WHERE actor_user_id=?1 ORDER BY created_at,id LIMIT 1024",
        ))?;
        let ids = db(q.query_map([user.as_str()], |r| r.get::<_, String>(0)))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| Error::Storage("cannot read schedules"))?;
        ids.into_iter()
            .map(|id| schedule_row(&c, &user, &id))
            .collect()
    }

    pub fn schedule(&self, actor: &PrincipalId, selector: &str) -> Result<PipelineScheduleInfo> {
        let c = self.connection()?;
        let user = stable_actor_user(&c, actor)?;
        schedule_row(&c, &user, selector)
    }

    pub fn schedule_set_paused(
        &self,
        actor: &PrincipalId,
        selector: &str,
        expected_generation: u64,
        paused: bool,
    ) -> Result<PipelineScheduleInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = stable_actor_user(&tx, actor)?;
        let current = schedule_row(&tx, &user, selector)?;
        if current.generation != expected_generation {
            return Err(Error::Conflict("schedule generation changed"));
        }
        let already_requested =
            (paused && current.state == "paused") || (!paused && current.state == "active");
        if already_requested {
            db(tx.commit())?;
            return Ok(current);
        }
        let at = now();
        let next_state = if paused { "paused" } else { "active" };
        // Pause means missed intervals are intentionally not occurrences. When
        // resuming after the old due time, restart cadence from now instead of
        // silently backfilling potentially expensive autonomous work. A still-
        // future due time is preserved.
        let next_due_at = if !paused && current.next_due_at <= at {
            at.saturating_add((current.interval_seconds as i64).saturating_mul(1_000_000))
        } else {
            current.next_due_at
        };
        let changed = db(tx.execute(
            "UPDATE pipeline_schedules SET state=?1,generation=generation+1,next_due_at=?2,updated_at=?3 \
             WHERE id=?4 AND actor_user_id=?5 AND generation=?6",
            params![
                next_state,
                next_due_at,
                at,
                current.id,
                user,
                expected_generation as i64
            ],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("schedule generation changed"));
        }
        let out = schedule_row(&tx, &stable_actor_user(&tx, actor)?, &current.id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn schedule_occurrences(
        &self,
        actor: &PrincipalId,
        selector: &str,
    ) -> Result<Vec<PipelineScheduleOccurrenceInfo>> {
        let c = self.connection()?;
        let user = stable_actor_user(&c, actor)?;
        let schedule = schedule_row(&c, &user, selector)?;
        let mut q = db(c.prepare(
            "SELECT id FROM pipeline_schedule_occurrences WHERE schedule_id=?1              ORDER BY occurrence_no DESC LIMIT 1024",
        ))?;
        let ids = db(q.query_map([schedule.id], |r| r.get::<_, String>(0)))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| Error::Storage("cannot read schedule occurrences"))?;
        ids.into_iter().map(|id| occurrence_row(&c, &id)).collect()
    }

    pub fn schedule_claim_due(
        &self,
        actor: &PrincipalId,
        client_id: &str,
        at: i64,
    ) -> Result<Option<PipelineScheduleClaim>> {
        validate_client_id(client_id)?;
        let process = self.process_identity()?;
        if let Some(process) = process.as_ref() {
            if process.process_id != client_id {
                return Err(Error::Denied(
                    "persistent schedule claims must use the store process identity",
                ));
            }
        }
        let owner_may_recover_unknown = process.is_some() && !self.trusted_local_direct();
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = stable_actor_user(&tx, actor)?;

        // A claim is persisted before pipeline admission. If that process dies,
        // the next valid process must reconcile the same occurrence rather than
        // deadlocking serial schedules or manufacturing later skips forever.
        let mut claimed = db(tx.prepare(
            "SELECT o.id FROM pipeline_schedule_occurrences o \
             JOIN pipeline_schedules s ON s.id=o.schedule_id \
             WHERE s.actor_user_id=?1 AND s.state='active' AND o.state='claimed' \
             ORDER BY o.due_at,o.occurrence_no LIMIT 64",
        ))?;
        let claimed_ids = db(claimed.query_map([user.as_str()], |r| r.get::<_, String>(0)))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| Error::Storage("cannot scan claimed schedule occurrences"))?;
        drop(claimed);
        for occurrence_id in claimed_ids {
            let occurrence = occurrence_row(&tx, &occurrence_id)?;
            if claim_recoverable(
                &tx,
                &occurrence.claimed_by,
                client_id,
                at,
                owner_may_recover_unknown,
            )? {
                let schedule = schedule_row(&tx, &user, &occurrence.schedule_id)?;
                db(tx.commit())?;
                return Ok(Some(PipelineScheduleClaim {
                    schedule,
                    occurrence,
                }));
            }
        }

        let mut q = db(tx.prepare(
            "SELECT id FROM pipeline_schedules              WHERE actor_user_id=?1 AND state='active' AND next_due_at<=?2              ORDER BY next_due_at,id LIMIT 64",
        ))?;
        let ids = db(q.query_map(params![user.as_str(), at], |r| r.get::<_, String>(0)))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| Error::Storage("cannot scan due schedules"))?;
        drop(q);

        for schedule_id in ids.into_iter().take(MAX_DUE_SCAN) {
            let schedule = schedule_row(&tx, &user, &schedule_id)?;
            let active = schedule_has_active_occurrence(&tx, &schedule.id)?;
            let occurrence_no = schedule.next_occurrence_no;
            let request_id = format!("sched:{}:{}", schedule.id, occurrence_no);
            let occurrence_id = id("socc")?;
            let updated = now();
            let next_due_at =
                advance_interval_due(schedule.next_due_at, schedule.interval_seconds, at);

            if active {
                if schedule.overlap_policy == ScheduleOverlapPolicy::Serial {
                    continue;
                }
                let count: i64 = db(tx.query_row(
                    "SELECT count(*) FROM pipeline_schedule_occurrences WHERE schedule_id=?1",
                    [schedule.id.as_str()],
                    |r| r.get(0),
                ))?;
                if count >= MAX_OCCURRENCES_PER_SCHEDULE {
                    return Err(Error::Limit("schedule occurrence retention limit reached"));
                }
                db(tx.execute(
                    "INSERT INTO pipeline_schedule_occurrences(                     id,schedule_id,occurrence_no,due_at,state,claim_generation,claimed_by,                     request_id,reason,created_at,updated_at)                     VALUES(?1,?2,?3,?4,'skipped',?5,?6,?7,'overlap policy skip',?8,?8)",
                    params![
                        occurrence_id,
                        schedule.id,
                        occurrence_no as i64,
                        schedule.next_due_at,
                        schedule.generation as i64,
                        client_id,
                        request_id,
                        updated
                    ],
                ))?;
                db(tx.execute(
                    "UPDATE pipeline_schedules SET next_due_at=?1,                     next_occurrence_no=next_occurrence_no+1,updated_at=?2 WHERE id=?3",
                    params![next_due_at, updated, schedule.id],
                ))?;
                continue;
            }

            let count: i64 = db(tx.query_row(
                "SELECT count(*) FROM pipeline_schedule_occurrences WHERE schedule_id=?1",
                [schedule.id.as_str()],
                |r| r.get(0),
            ))?;
            if count >= MAX_OCCURRENCES_PER_SCHEDULE {
                return Err(Error::Limit("schedule occurrence retention limit reached"));
            }
            db(tx.execute(
                "INSERT INTO pipeline_schedule_occurrences(                 id,schedule_id,occurrence_no,due_at,state,claim_generation,claimed_by,                 request_id,created_at,updated_at)                 VALUES(?1,?2,?3,?4,'claimed',?5,?6,?7,?8,?8)",
                params![
                    occurrence_id,
                    schedule.id,
                    occurrence_no as i64,
                    schedule.next_due_at,
                    schedule.generation as i64,
                    client_id,
                    request_id,
                    updated
                ],
            ))?;
            db(tx.execute(
                "UPDATE pipeline_schedules SET next_due_at=?1,                 next_occurrence_no=next_occurrence_no+1,updated_at=?2 WHERE id=?3",
                params![next_due_at, updated, schedule.id],
            ))?;
            let updated_schedule = schedule_row(&tx, &user, &schedule.id)?;
            let occurrence = occurrence_row(&tx, &occurrence_id)?;
            db(tx.commit())?;
            return Ok(Some(PipelineScheduleClaim {
                schedule: updated_schedule,
                occurrence,
            }));
        }

        db(tx.commit())?;
        Ok(None)
    }

    pub fn schedule_admit_claim(
        &self,
        actor: &PrincipalId,
        occurrence_id: &str,
        client_id: &str,
        claim_generation: u64,
    ) -> Result<PipelineRunInfo> {
        validate_client_id(client_id)?;
        let process = self.process_identity()?;
        if let Some(process) = process.as_ref() {
            if process.process_id != client_id {
                return Err(Error::Denied(
                    "persistent schedule admission must use the store process identity",
                ));
            }
        }
        let owner_may_recover_unknown = process.is_some() && !self.trusted_local_direct();
        let at = now();
        let (schedule, occurrence, existing_run_id) = {
            let c = self.connection()?;
            let user = stable_actor_user(&c, actor)?;
            let occurrence = occurrence_row(&c, occurrence_id)?;
            let schedule = schedule_row(&c, &user, &occurrence.schedule_id)?;
            let expected_request_id = format!("sched:{}:{}", schedule.id, occurrence.occurrence_no);
            if occurrence.request_id != expected_request_id {
                return Err(Error::Storage(
                    "schedule occurrence request identity does not match its immutable schedule/number",
                ));
            }

            if occurrence.state == "admitted" {
                let run_id = occurrence
                    .pipeline_run_id
                    .clone()
                    .ok_or(Error::Storage("admitted schedule occurrence has no run"))?;
                // pipeline_run performs its own authorized lookup and acquires
                // the store connection; do not recursively lock this guard.
                drop(c);
                return self.pipeline_run(actor, &run_id);
            }
            if occurrence.state != "claimed"
                || occurrence.claim_generation != claim_generation
                || !claim_recoverable(
                    &c,
                    &occurrence.claimed_by,
                    client_id,
                    at,
                    owner_may_recover_unknown,
                )?
            {
                return Err(Error::Conflict("schedule occurrence claim changed"));
            }

            // The process may have crashed after pipeline admission committed but
            // before the occurrence was linked to that run. Reconcile the exact
            // deterministic request ID first, even if the binding was disabled
            // afterwards; an already-admitted run is durable evidence, not new
            // authority to dispatch again.
            let existing: Option<(String, String, String, Option<String>, Vec<u8>)> = db(c
                .query_row(
                    "SELECT id,pipeline_id,revision_id,binding_id,input_utf8 FROM pipeline_runs \
                     WHERE actor_user_id=?1 AND request_id=?2",
                    params![user, occurrence.request_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .optional())?;
            let existing_run_id =
                if let Some((run_id, pipeline_id, revision_id, binding_id, input)) = existing {
                    if pipeline_id != schedule.pipeline_id
                        || revision_id != schedule.revision_id
                        || binding_id.as_deref() != Some(schedule.binding_id.as_str())
                        || input != schedule.input.as_bytes()
                    {
                        return Err(Error::Conflict(
                            "schedule request ID belongs to a different pipeline admission",
                        ));
                    }
                    Some(run_id)
                } else {
                    None
                };
            if existing_run_id.is_none() && schedule.state != "active" {
                return Err(Error::Conflict(
                    "schedule paused before claimed occurrence was admitted",
                ));
            }
            (schedule, occurrence, existing_run_id)
        };

        let run = if let Some(run_id) = existing_run_id {
            self.pipeline_run(actor, &run_id)?
        } else {
            match self.pipeline_admit_scheduled(
                actor,
                &schedule.pipeline_id,
                Some(&schedule.revision_id),
                Some(&schedule.binding_id),
                &occurrence.request_id,
                &schedule.input,
            ) {
                Ok(run) => run,
                Err(error) => {
                    let mut c = self.connection()?;
                    let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
                    let _ = tx.execute(
                        "UPDATE pipeline_schedule_occurrences SET state='blocked',reason='pipeline admission rejected', \
                         updated_at=?1 WHERE id=?2 AND state='claimed' AND claimed_by=?3 AND claim_generation=?4",
                        params![
                            now(),
                            occurrence_id,
                            occurrence.claimed_by,
                            claim_generation as i64
                        ],
                    );
                    db(tx.commit())?;
                    return Err(error);
                }
            }
        };

        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let changed = db(tx.execute(
            "UPDATE pipeline_schedule_occurrences SET state='admitted',pipeline_run_id=?1,updated_at=?2 \
             WHERE id=?3 AND state='claimed' AND claimed_by=?4 AND claim_generation=?5",
            params![
                run.id,
                now(),
                occurrence_id,
                occurrence.claimed_by,
                claim_generation as i64
            ],
        ))?;
        if changed != 1 {
            let latest = occurrence_row(&tx, occurrence_id)?;
            if latest.state != "admitted"
                || latest.pipeline_run_id.as_deref() != Some(run.id.as_str())
            {
                return Err(Error::Conflict(
                    "schedule occurrence claim changed after admission",
                ));
            }
        }
        db(tx.commit())?;
        Ok(run)
    }

    pub fn schedule_settle_occurrence(
        &self,
        actor: &PrincipalId,
        occurrence_id: &str,
    ) -> Result<PipelineScheduleOccurrenceInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = stable_actor_user(&tx, actor)?;
        let occurrence = occurrence_row(&tx, occurrence_id)?;
        let _schedule = schedule_row(&tx, &user, &occurrence.schedule_id)?;
        let Some(run_id) = occurrence.pipeline_run_id.as_deref() else {
            db(tx.commit())?;
            return Ok(occurrence);
        };
        let run_state: String = db(tx.query_row(
            "SELECT state FROM pipeline_runs WHERE id=?1",
            [run_id],
            |r| r.get(0),
        ))?;
        let terminal = match run_state.as_str() {
            "completed" => Some(("completed", None::<&str>)),
            "failed" | "cancelled" => Some(("failed", Some(run_state.as_str()))),
            _ => None,
        };
        if let Some((state, reason)) = terminal {
            db(tx.execute(
                "UPDATE pipeline_schedule_occurrences SET state=?1,reason=?2,updated_at=?3                  WHERE id=?4 AND state='admitted'",
                params![state, reason, now(), occurrence_id],
            ))?;
        }
        let out = occurrence_row(&tx, occurrence_id)?;
        db(tx.commit())?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_pipeline::{CheckSpec, PipelineDefinition, PipelineLimits, PipelineStep, TransformSpec};

    fn setup() -> (SessionStore, PrincipalId, PipelineBindingInfo) {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("internal:schedule:alice").unwrap();
        let def = PipelineDefinition {
            name: "scheduled".into(),
            description: String::new(),
            limits: PipelineLimits::default(),
            steps: vec![
                PipelineStep::Check {
                    id: "nonempty".into(),
                    check: CheckSpec::NonEmpty,
                },
                PipelineStep::Transform {
                    id: "trim".into(),
                    transform: TransformSpec::Trim,
                },
                PipelineStep::Emit { id: "emit".into() },
            ],
        };
        let pipeline = store.pipeline_define(&actor, def).unwrap();
        let binding = store
            .pipeline_bind(
                &actor,
                &pipeline.pipeline_id,
                Some(&pipeline.revision_id),
                PipelineBindingScope::User,
                "self",
                PipelineBindingMode::Task,
            )
            .unwrap();
        (store, actor, binding)
    }

    #[test]
    fn interval_advance_coalesces_long_downtime_without_burst_backfill() {
        let interval = 60;
        assert_eq!(
            advance_interval_due(100_000_000, interval, 120_000_000),
            160_000_000
        );
        assert_eq!(
            advance_interval_due(100_000_000, interval, 500_000_000),
            560_000_000
        );
    }

    #[test]
    fn schedule_claim_is_generation_fenced_and_pipeline_admission_is_idempotent() {
        let (store, actor, binding) = setup();
        let schedule = store
            .schedule_define(
                &actor,
                "daily",
                &binding.id,
                "  hello  ",
                60,
                ScheduleOverlapPolicy::Serial,
                0,
            )
            .unwrap();
        assert_eq!(schedule.generation, 1);

        let claim = store
            .schedule_claim_due(&actor, "test-client", i64::MAX)
            .unwrap()
            .unwrap();
        assert_eq!(claim.occurrence.state, "claimed");
        assert!(store
            .schedule_admit_claim(
                &actor,
                &claim.occurrence.id,
                "wrong-client",
                claim.occurrence.claim_generation
            )
            .is_err());

        let run = store
            .schedule_admit_claim(
                &actor,
                &claim.occurrence.id,
                "test-client",
                claim.occurrence.claim_generation,
            )
            .unwrap();
        let (completed, result) = std::sync::mpsc::channel();
        let repeat_store = store.clone();
        let repeat_actor = actor.clone();
        let occurrence_id = claim.occurrence.id.clone();
        let generation = claim.occurrence.claim_generation;
        let repeat = std::thread::spawn(move || {
            let admitted = repeat_store.schedule_admit_claim(
                &repeat_actor,
                &occurrence_id,
                "test-client",
                generation,
            );
            let _ = completed.send(admitted);
        });
        let again = result
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("idempotent admission must not recursively lock the store")
            .unwrap();
        repeat.join().unwrap();
        assert_eq!(run.id, again.id);
        let other_actor = PrincipalId::new("internal:schedule:other").unwrap();
        assert!(store
            .schedule_admit_claim(
                &other_actor,
                &claim.occurrence.id,
                "test-client",
                generation,
            )
            .is_err());

        match store.pipeline_next(&actor, &run.id).unwrap() {
            PipelineDirective::Completed { .. } => {}
            PipelineDirective::Failed { .. } => panic!("deterministic scheduled pipeline failed"),
            PipelineDirective::Agent { .. }
            | PipelineDirective::Approval { .. }
            | PipelineDirective::UnknownExternal { .. } => {
                panic!("fixture pipeline should remain deterministic")
            }
        }
        let settled = store
            .schedule_settle_occurrence(&actor, &claim.occurrence.id)
            .unwrap();
        assert_eq!(settled.state, "completed");
    }

    #[test]
    fn pause_is_cas_idempotent_and_resume_does_not_backfill_missed_intervals() {
        let (store, actor, binding) = setup();
        let schedule = store
            .schedule_define(
                &actor,
                "hourly",
                &binding.id,
                "hello",
                3600,
                ScheduleOverlapPolicy::Skip,
                0,
            )
            .unwrap();
        assert!(store
            .schedule_set_paused(&actor, &schedule.id, 9, true)
            .is_err());
        let paused = store
            .schedule_set_paused(&actor, &schedule.id, 1, true)
            .unwrap();
        assert_eq!(paused.state, "paused");
        assert_eq!(paused.generation, 2);
        let same = store
            .schedule_set_paused(&actor, &schedule.id, 2, true)
            .unwrap();
        assert_eq!(same.generation, 2);
        assert!(store
            .schedule_claim_due(&actor, "test-client", i64::MAX)
            .unwrap()
            .is_none());

        {
            let c = store.connection().unwrap();
            c.execute(
                "UPDATE pipeline_schedules SET next_due_at=0 WHERE id=?1",
                [&schedule.id],
            )
            .unwrap();
        }
        let before = now();
        let resumed = store
            .schedule_set_paused(&actor, &schedule.id, 2, false)
            .unwrap();
        assert_eq!(resumed.state, "active");
        assert_eq!(resumed.generation, 3);
        assert!(resumed.next_due_at >= before + 3_599_000_000);
        assert!(store
            .schedule_claim_due(&actor, "test-client", before)
            .unwrap()
            .is_none());
    }

    #[test]
    fn paused_schedule_cannot_admit_unadmitted_claim_through_store_api() {
        let (store, actor, binding) = setup();
        let schedule = store
            .schedule_define(
                &actor,
                "pause-before-admit",
                &binding.id,
                "hello",
                60,
                ScheduleOverlapPolicy::Serial,
                0,
            )
            .unwrap();
        let claim = store
            .schedule_claim_due(&actor, "test-client", i64::MAX)
            .unwrap()
            .unwrap();
        let paused = store
            .schedule_set_paused(&actor, &schedule.id, schedule.generation, true)
            .unwrap();
        assert_eq!(paused.state, "paused");
        assert!(store
            .schedule_admit_claim(
                &actor,
                &claim.occurrence.id,
                "test-client",
                claim.occurrence.claim_generation,
            )
            .is_err());
    }

    fn persistent_setup(label: &str) -> (std::path::PathBuf, PrincipalId, String, String) {
        let root = std::env::temp_dir().join(format!(
            "aw-schedule-review-{label}-{}-{}",
            std::process::id(),
            id("t").unwrap()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let store = SessionStore::open(&root).unwrap();
        let actor = PrincipalId::new(format!("internal:schedule:{label}")).unwrap();
        let def = PipelineDefinition {
            name: format!("scheduled-{label}"),
            description: String::new(),
            limits: PipelineLimits::default(),
            steps: vec![
                PipelineStep::Check {
                    id: "nonempty".into(),
                    check: CheckSpec::NonEmpty,
                },
                PipelineStep::Emit { id: "emit".into() },
            ],
        };
        let pipeline = store.pipeline_define(&actor, def).unwrap();
        let binding = store
            .pipeline_bind(
                &actor,
                &pipeline.pipeline_id,
                Some(&pipeline.revision_id),
                PipelineBindingScope::User,
                "self",
                PipelineBindingMode::Task,
            )
            .unwrap();
        let schedule = store
            .schedule_define(
                &actor,
                "restart",
                &binding.id,
                "hello",
                60,
                ScheduleOverlapPolicy::Serial,
                0,
            )
            .unwrap();
        drop(store);
        (root, actor, binding.id, schedule.id)
    }

    #[test]
    fn owner_restart_recovers_orphaned_pre_admission_claim() {
        let (root, actor, _binding, schedule_id) = persistent_setup("orphan-claim");
        let first = SessionStore::open(&root).unwrap();
        let old_client = first.local_process_info().unwrap().process_id;
        let claim = first
            .schedule_claim_due(&actor, &old_client, i64::MAX)
            .unwrap()
            .unwrap();
        drop(first);

        let second = SessionStore::open(&root).unwrap();
        let new_client = second.local_process_info().unwrap().process_id;
        assert_ne!(old_client, new_client);
        let recovered = second
            .schedule_claim_due(&actor, &new_client, i64::MAX)
            .unwrap()
            .unwrap();
        assert_eq!(recovered.schedule.id, schedule_id);
        assert_eq!(recovered.occurrence.id, claim.occurrence.id);
        assert_eq!(recovered.occurrence.claimed_by, old_client);
        let run = second
            .schedule_admit_claim(
                &actor,
                &recovered.occurrence.id,
                &new_client,
                recovered.occurrence.claim_generation,
            )
            .unwrap();
        assert_eq!(run.request_id, recovered.occurrence.request_id);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn owner_restart_reconciles_run_committed_before_occurrence_link() {
        let (root, actor, binding_id, schedule_id) = persistent_setup("admit-window");
        let first = SessionStore::open(&root).unwrap();
        let old_client = first.local_process_info().unwrap().process_id;
        let claim = first
            .schedule_claim_due(&actor, &old_client, i64::MAX)
            .unwrap()
            .unwrap();
        let existing = first
            .pipeline_admit_scheduled(
                &actor,
                &claim.schedule.pipeline_id,
                Some(&claim.schedule.revision_id),
                Some(&binding_id),
                &claim.occurrence.request_id,
                &claim.schedule.input,
            )
            .unwrap();
        drop(first);

        let second = SessionStore::open(&root).unwrap();
        let new_client = second.local_process_info().unwrap().process_id;
        let recovered = second
            .schedule_claim_due(&actor, &new_client, i64::MAX)
            .unwrap()
            .unwrap();
        assert_eq!(recovered.schedule.id, schedule_id);
        let run = second
            .schedule_admit_claim(
                &actor,
                &recovered.occurrence.id,
                &new_client,
                recovered.occurrence.claim_generation,
            )
            .unwrap();
        assert_eq!(run.id, existing.id);
        let occurrence = second
            .schedule_occurrences(&actor, &schedule_id)
            .unwrap()
            .into_iter()
            .find(|item| item.id == recovered.occurrence.id)
            .unwrap();
        assert_eq!(occurrence.state, "admitted");
        assert_eq!(
            occurrence.pipeline_run_id.as_deref(),
            Some(existing.id.as_str())
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
