-- Phase 46: immutable bounded team templates + durable candidate/join receipts.

CREATE TABLE team_templates(
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    owner_user_id TEXT NOT NULL REFERENCES users(user_id),
    status TEXT NOT NULL CHECK(status IN ('active','disabled')),
    created_at INTEGER NOT NULL,
    UNIQUE(owner_user_id,name)
);

CREATE TABLE team_template_revisions(
    revision_id TEXT PRIMARY KEY,
    team_id TEXT NOT NULL REFERENCES team_templates(id) ON DELETE CASCADE,
    revision_no INTEGER NOT NULL CHECK(revision_no>=1),
    definition_sha256 TEXT NOT NULL CHECK(length(definition_sha256)=64),
    definition_json TEXT NOT NULL CHECK(json_valid(definition_json) AND length(definition_json)<=65536),
    created_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    created_at INTEGER NOT NULL,
    UNIQUE(team_id,revision_no)
);
CREATE INDEX team_template_revisions_team ON team_template_revisions(team_id,revision_no);

CREATE TABLE team_runs(
    id TEXT PRIMARY KEY,
    revision_id TEXT NOT NULL REFERENCES team_template_revisions(revision_id) ON DELETE RESTRICT,
    actor_user_id TEXT NOT NULL REFERENCES users(user_id),
    parent_session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    parent_branch_id TEXT NOT NULL REFERENCES branches(id) ON DELETE CASCADE,
    parent_turn_id INTEGER NOT NULL CHECK(parent_turn_id>=1),
    state TEXT NOT NULL CHECK(state IN ('admitted','running','joined','failed','interrupted')),
    task_sha256 TEXT NOT NULL CHECK(length(task_sha256)=64),
    publisher_kind TEXT NOT NULL CHECK(publisher_kind='parent_agent'),
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    settled_at INTEGER,
    failure_code TEXT CHECK(failure_code IS NULL OR length(failure_code)<=64),
    CHECK(
      (state='admitted' AND started_at IS NULL AND settled_at IS NULL) OR
      (state='running' AND started_at IS NOT NULL AND settled_at IS NULL) OR
      (state IN ('joined','failed','interrupted') AND started_at IS NOT NULL AND settled_at IS NOT NULL)
    )
);
CREATE INDEX team_runs_parent ON team_runs(parent_session_id,parent_branch_id,parent_turn_id,created_at);
CREATE INDEX team_runs_state ON team_runs(state,created_at);

CREATE TABLE team_run_members(
    team_run_id TEXT NOT NULL REFERENCES team_runs(id) ON DELETE CASCADE,
    member_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal>=0),
    role TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','running','completed','failed','interrupted')),
    subagent_id TEXT UNIQUE,
    child_session_id TEXT UNIQUE REFERENCES sessions(id) ON DELETE SET NULL,
    answer_sha256 TEXT CHECK(answer_sha256 IS NULL OR length(answer_sha256)=64),
    answer_truncated INTEGER NOT NULL DEFAULT 0 CHECK(answer_truncated IN (0,1)),
    started_at INTEGER,
    settled_at INTEGER,
    error TEXT CHECK(error IS NULL OR length(error)<=4096),
    PRIMARY KEY(team_run_id,member_id),
    UNIQUE(team_run_id,ordinal),
    CHECK(
      (state='pending' AND started_at IS NULL AND settled_at IS NULL AND child_session_id IS NULL) OR
      (state='running' AND started_at IS NOT NULL AND settled_at IS NULL) OR
      (state IN ('completed','failed','interrupted') AND started_at IS NOT NULL AND settled_at IS NOT NULL)
    )
);
CREATE INDEX team_run_members_state ON team_run_members(state,started_at);

CREATE TABLE team_join_receipts(
    team_run_id TEXT PRIMARY KEY REFERENCES team_runs(id) ON DELETE CASCADE,
    policy TEXT NOT NULL CHECK(policy='all_required'),
    manifest_json TEXT NOT NULL CHECK(json_valid(manifest_json) AND length(manifest_json)<=65536),
    joined_sha256 TEXT NOT NULL CHECK(length(joined_sha256)=64),
    created_at INTEGER NOT NULL
);

CREATE TRIGGER team_template_revisions_immutable_update
BEFORE UPDATE ON team_template_revisions
BEGIN SELECT RAISE(ABORT,'team template revisions are immutable'); END;
CREATE TRIGGER team_template_revisions_immutable_delete
BEFORE DELETE ON team_template_revisions
BEGIN SELECT RAISE(ABORT,'team template revisions are immutable'); END;
CREATE TRIGGER team_join_receipts_immutable_update
BEFORE UPDATE ON team_join_receipts
BEGIN SELECT RAISE(ABORT,'team join receipts are immutable'); END;
CREATE TRIGGER team_join_receipts_immutable_delete
BEFORE DELETE ON team_join_receipts
BEGIN SELECT RAISE(ABORT,'team join receipts are immutable'); END;

UPDATE store_meta SET value='30' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=30,activation_epoch=30,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=30,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;
PRAGMA user_version=30;
