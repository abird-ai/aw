-- Phase 67: bounded governed pipeline schedules and durable occurrence claims.

CREATE TABLE pipeline_schedules(
    id TEXT PRIMARY KEY,
    actor_user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK(length(name)>=1 AND length(name)<=128),
    pipeline_id TEXT NOT NULL REFERENCES pipelines(id) ON DELETE RESTRICT,
    revision_id TEXT NOT NULL REFERENCES pipeline_revisions(revision_id) ON DELETE RESTRICT,
    binding_id TEXT NOT NULL REFERENCES pipeline_bindings(id) ON DELETE RESTRICT,
    input_utf8 BLOB NOT NULL CHECK(length(input_utf8)<=262144),
    interval_seconds INTEGER NOT NULL CHECK(interval_seconds BETWEEN 60 AND 2592000),
    overlap_policy TEXT NOT NULL CHECK(overlap_policy IN ('skip','serial')),
    state TEXT NOT NULL CHECK(state IN ('active','paused')),
    generation INTEGER NOT NULL CHECK(generation>=1),
    next_due_at INTEGER NOT NULL,
    next_occurrence_no INTEGER NOT NULL CHECK(next_occurrence_no>=1),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(actor_user_id,name)
);

CREATE INDEX pipeline_schedules_due
    ON pipeline_schedules(state,next_due_at,id);

CREATE TABLE pipeline_schedule_occurrences(
    id TEXT PRIMARY KEY,
    schedule_id TEXT NOT NULL REFERENCES pipeline_schedules(id) ON DELETE CASCADE,
    occurrence_no INTEGER NOT NULL CHECK(occurrence_no>=1),
    due_at INTEGER NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('claimed','admitted','completed','failed','skipped','blocked')),
    claim_generation INTEGER NOT NULL CHECK(claim_generation>=1),
    claimed_by TEXT NOT NULL CHECK(length(claimed_by)>=1 AND length(claimed_by)<=128),
    request_id TEXT NOT NULL CHECK(length(request_id)>=1 AND length(request_id)<=128),
    pipeline_run_id TEXT REFERENCES pipeline_runs(id) ON DELETE RESTRICT,
    reason TEXT CHECK(reason IS NULL OR length(reason)<=1024),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(schedule_id,occurrence_no),
    UNIQUE(request_id)
);

CREATE INDEX pipeline_schedule_occurrences_schedule
    ON pipeline_schedule_occurrences(schedule_id,occurrence_no);
CREATE INDEX pipeline_schedule_occurrences_run
    ON pipeline_schedule_occurrences(pipeline_run_id)
    WHERE pipeline_run_id IS NOT NULL;

CREATE TRIGGER pipeline_schedule_binding_matches_authority
BEFORE INSERT ON pipeline_schedules
WHEN NOT EXISTS (
    SELECT 1 FROM pipeline_bindings b
    WHERE b.id=NEW.binding_id
      AND b.pipeline_id=NEW.pipeline_id
      AND b.revision_id=NEW.revision_id
      AND b.scope_kind='user'
      AND b.scope_user_id=NEW.actor_user_id
      AND b.mode='task'
      AND b.status='active'
)
BEGIN SELECT RAISE(ABORT,'schedule requires exact active user task binding'); END;

CREATE TRIGGER pipeline_schedule_authority_immutable
BEFORE UPDATE OF name,actor_user_id,pipeline_id,revision_id,binding_id,input_utf8,
                 interval_seconds,overlap_policy,created_at
ON pipeline_schedules
BEGIN SELECT RAISE(ABORT,'schedule authority snapshot is immutable'); END;

CREATE TRIGGER pipeline_schedule_occurrence_identity_immutable
BEFORE UPDATE OF schedule_id,occurrence_no,due_at,claim_generation,claimed_by,request_id,created_at
ON pipeline_schedule_occurrences
BEGIN SELECT RAISE(ABORT,'schedule occurrence identity is immutable'); END;

UPDATE store_meta SET value='33' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=33,activation_epoch=33,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=33,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;
PRAGMA user_version=33;
