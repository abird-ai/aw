-- Phase 39: bounded immutable pipeline revisions and durable manual execution.
-- External effects are represented by started step intents before host dispatch.

CREATE TABLE pipelines(
    id TEXT PRIMARY KEY,
    owner_user_id TEXT NOT NULL REFERENCES users(user_id),
    name TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('active','archived')),
    created_at INTEGER NOT NULL,
    UNIQUE(owner_user_id,name)
);

CREATE TABLE pipeline_revisions(
    revision_id TEXT PRIMARY KEY,
    pipeline_id TEXT NOT NULL REFERENCES pipelines(id) ON DELETE CASCADE,
    revision_no INTEGER NOT NULL CHECK(revision_no>=1),
    definition_sha256 TEXT NOT NULL CHECK(length(definition_sha256)=64),
    definition_json TEXT NOT NULL,
    created_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    created_at INTEGER NOT NULL,
    UNIQUE(pipeline_id,revision_no),
    UNIQUE(pipeline_id,definition_sha256)
);
CREATE TRIGGER pipeline_revisions_immutable_update
BEFORE UPDATE ON pipeline_revisions BEGIN
    SELECT RAISE(ABORT,'pipeline revisions are immutable');
END;
CREATE TRIGGER pipeline_revisions_immutable_delete
BEFORE DELETE ON pipeline_revisions BEGIN
    SELECT RAISE(ABORT,'pipeline revisions are immutable');
END;

CREATE TABLE pipeline_bindings(
    id TEXT PRIMARY KEY,
    pipeline_id TEXT NOT NULL REFERENCES pipelines(id) ON DELETE CASCADE,
    revision_id TEXT NOT NULL REFERENCES pipeline_revisions(revision_id),
    scope_kind TEXT NOT NULL CHECK(scope_kind IN ('user','session')),
    scope_user_id TEXT REFERENCES users(user_id),
    scope_session_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
    mode TEXT NOT NULL CHECK(mode IN ('gate','transform','observer','task')),
    trigger_kind TEXT NOT NULL CHECK(trigger_kind='manual'),
    revision INTEGER NOT NULL CHECK(revision>=1),
    status TEXT NOT NULL CHECK(status IN ('active','disabled')),
    created_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK(
      (scope_kind='user' AND scope_user_id IS NOT NULL AND scope_session_id IS NULL) OR
      (scope_kind='session' AND scope_user_id IS NULL AND scope_session_id IS NOT NULL)
    )
);
CREATE INDEX pipeline_bindings_user ON pipeline_bindings(scope_user_id,status,mode,trigger_kind);
CREATE INDEX pipeline_bindings_session ON pipeline_bindings(scope_session_id,status,mode,trigger_kind);

CREATE TABLE pipeline_runs(
    id TEXT PRIMARY KEY,
    pipeline_id TEXT NOT NULL REFERENCES pipelines(id),
    revision_id TEXT NOT NULL REFERENCES pipeline_revisions(revision_id),
    binding_id TEXT REFERENCES pipeline_bindings(id),
    actor_user_id TEXT NOT NULL REFERENCES users(user_id),
    session_id TEXT REFERENCES sessions(id),
    branch_id TEXT REFERENCES branches(id),
    request_id TEXT NOT NULL,
    request_fingerprint TEXT NOT NULL CHECK(length(request_fingerprint)=64),
    state TEXT NOT NULL CHECK(state IN ('admitted','running_external','waiting_approval','completed','failed','cancelled')),
    run_revision INTEGER NOT NULL CHECK(run_revision>=1),
    next_step_index INTEGER NOT NULL CHECK(next_step_index>=0),
    input_utf8 BLOB NOT NULL,
    current_utf8 BLOB NOT NULL,
    output_utf8 BLOB,
    deadline_at INTEGER NOT NULL,
    failure_code TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(actor_user_id,request_id)
);
CREATE INDEX pipeline_runs_actor_state ON pipeline_runs(actor_user_id,state,updated_at);
CREATE INDEX pipeline_runs_pipeline ON pipeline_runs(pipeline_id,revision_id,created_at);

CREATE TABLE pipeline_step_attempts(
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES pipeline_runs(id) ON DELETE CASCADE,
    step_index INTEGER NOT NULL CHECK(step_index>=0),
    step_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('check','transform','approval','agent','emit')),
    state TEXT NOT NULL CHECK(state IN ('started','waiting_approval','completed','failed')),
    input_sha256 TEXT NOT NULL CHECK(length(input_sha256)=64),
    output_sha256 TEXT CHECK(output_sha256 IS NULL OR length(output_sha256)=64),
    receipt_json TEXT,
    started_at INTEGER NOT NULL,
    settled_at INTEGER,
    UNIQUE(run_id,step_index)
);

CREATE TABLE pipeline_approvals(
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES pipeline_runs(id) ON DELETE CASCADE,
    step_attempt_id TEXT NOT NULL REFERENCES pipeline_step_attempts(id) ON DELETE CASCADE,
    message TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('waiting','approved','denied')),
    requested_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    decided_by_user_id TEXT REFERENCES users(user_id),
    created_at INTEGER NOT NULL,
    decided_at INTEGER,
    UNIQUE(run_id,step_attempt_id)
);

UPDATE store_meta SET value='22' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=22,activation_epoch=22,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=22,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;
PRAGMA user_version=22;
