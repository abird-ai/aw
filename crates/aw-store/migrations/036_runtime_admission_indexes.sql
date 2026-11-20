-- Runtime admission replay projections.
--
-- Audit remains immutable historical evidence. These tables are the indexed
-- current/replay owner so operational correctness never depends on scanning a
-- Session's lifetime audit history.

CREATE TABLE policy_admission_current(
    admission_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    branch_id TEXT NOT NULL,
    turn_id INTEGER NOT NULL CHECK(turn_id>=1),
    call_id TEXT NOT NULL,
    canonical_target TEXT NOT NULL,
    subject_actor TEXT NOT NULL,
    request_sha256 TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision>=1),
    fact_json TEXT NOT NULL CHECK(json_valid(fact_json)),
    updated_at INTEGER NOT NULL
);
CREATE INDEX policy_admission_current_turn
    ON policy_admission_current(session_id,branch_id,turn_id,admission_id);
CREATE INDEX policy_admission_current_replay
    ON policy_admission_current(session_id,branch_id,turn_id,call_id,canonical_target);

CREATE TABLE remote_start_current(
    attempt_key BLOB PRIMARY KEY CHECK(length(attempt_key)=32),
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    actor TEXT NOT NULL,
    original_invocation_id TEXT NOT NULL,
    effect_attempt_id TEXT NOT NULL,
    turn_id INTEGER NOT NULL CHECK(turn_id>=1),
    authorization_json TEXT NOT NULL CHECK(json_valid(authorization_json)),
    created_at INTEGER NOT NULL,
    UNIQUE(session_id,actor,original_invocation_id,effect_attempt_id)
);
CREATE INDEX remote_start_current_turn
    ON remote_start_current(session_id,actor,turn_id,effect_attempt_id);

-- Durable Agent/Turn Job event sources. The source owns subscription
-- generations and factual event revisions; receiver authority stays opaque and
-- is revalidated by the Agent Job router at delivery time.
CREATE TABLE agent_job_event_sources(
    source_ref TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    branch_id TEXT NOT NULL REFERENCES branches(id) ON DELETE CASCADE,
    turn_id INTEGER CHECK(turn_id IS NULL OR turn_id>=1),
    event_book_json TEXT NOT NULL CHECK(json_valid(event_book_json)),
    last_result_turn INTEGER NOT NULL DEFAULT 0 CHECK(last_result_turn>=0),
    settled INTEGER NOT NULL DEFAULT 0 CHECK(settled IN (0,1)),
    pending_deliveries_json TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(pending_deliveries_json)),
    updated_at INTEGER NOT NULL
);
CREATE INDEX agent_job_event_sources_session
    ON agent_job_event_sources(session_id,branch_id,turn_id);

-- Current receiver-side wake authority. This is a bounded projection, not an
-- event log: regrant for one (recipient, actor) replaces the prior receipt so a
-- stale source rule cannot revive an old membership/tool generation.
CREATE TABLE agent_job_wake_admissions(
    recipient_ref TEXT NOT NULL,
    actor TEXT NOT NULL,
    authority_ref TEXT NOT NULL UNIQUE,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    membership_epoch INTEGER NOT NULL CHECK(membership_epoch>=1),
    audience_epoch INTEGER NOT NULL CHECK(audience_epoch>=1),
    is_owner INTEGER NOT NULL CHECK(is_owner IN (0,1)),
    tool_name TEXT NOT NULL,
    source_kind TEXT NOT NULL CHECK(source_kind IN ('host','mcp','extension')),
    source_id TEXT NOT NULL,
    source_epoch INTEGER NOT NULL CHECK(source_epoch>=1),
    execution_site TEXT NOT NULL,
    is_visible INTEGER NOT NULL CHECK(is_visible IN (0,1)),
    registry_revision INTEGER CHECK(registry_revision IS NULL OR registry_revision>=1),
    authority_revision INTEGER NOT NULL CHECK(authority_revision>=1),
    effect TEXT NOT NULL CHECK(effect IN ('read','write','process','control','unknown')),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(recipient_ref,actor),
    CHECK((is_visible=1 AND registry_revision IS NOT NULL)
       OR (is_visible=0 AND registry_revision IS NULL))
);
CREATE INDEX agent_job_wake_admissions_session
    ON agent_job_wake_admissions(session_id,recipient_ref);

-- Query-only mixed Agent/finite Job exposure projection. SessionStore remains
-- the Agent owner, JobManager remains the finite Job owner, and commit_log is
-- the immutable ordering evidence. This table only makes canonical lookup/list
-- paging bounded and index-seekable.
CREATE TABLE job_exposure_current(
    kind TEXT NOT NULL CHECK(kind IN ('agent','finite')),
    generation TEXT NOT NULL,
    canonical_ref TEXT NOT NULL,
    domain_handle TEXT NOT NULL,
    identity_handle TEXT NOT NULL,
    project_handle TEXT NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    branch_id TEXT NOT NULL REFERENCES branches(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position>=1),
    state TEXT NOT NULL CHECK(state IN ('published','reserved','released')),
    writer_epoch INTEGER CHECK(writer_epoch IS NULL OR writer_epoch>=1),
    PRIMARY KEY(kind,generation,canonical_ref),
    CHECK((kind='agent' AND generation='' AND state='published' AND writer_epoch IS NULL)
       OR (kind='finite' AND generation<>'' AND writer_epoch IS NOT NULL))
);
CREATE UNIQUE INDEX job_exposure_current_position
    ON job_exposure_current(position);
CREATE INDEX job_exposure_agent_page
    ON job_exposure_current(
        domain_handle,identity_handle,project_handle,position DESC,canonical_ref
    ) WHERE kind='agent' AND state='published';
CREATE INDEX job_exposure_finite_page
    ON job_exposure_current(
        domain_handle,identity_handle,project_handle,generation,state,position DESC,canonical_ref
    ) WHERE kind='finite';
CREATE INDEX sessions_agent_namespace
    ON sessions(workspace_id,owner_user_id,id);
CREATE INDEX branches_agent_namespace
    ON branches(session_id,created_at,id);

UPDATE store_meta SET value='36' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=36,activation_epoch=36,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=36,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;
PRAGMA user_version=36;
