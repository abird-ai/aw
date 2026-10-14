-- Phase 49: governed generative planner operations and validated bounded task DAG receipts.

ALTER TABLE delegation_roots
ADD COLUMN planner_routing_revision_id TEXT REFERENCES routing_strategy_revisions(revision_id) ON DELETE RESTRICT;

CREATE INDEX delegation_roots_planner_route
    ON delegation_roots(planner_routing_revision_id)
    WHERE planner_routing_revision_id IS NOT NULL;

CREATE TABLE planner_operations(
    id TEXT PRIMARY KEY,
    root_id TEXT NOT NULL REFERENCES delegation_roots(id) ON DELETE CASCADE,
    actor_user_id TEXT NOT NULL REFERENCES users(user_id),
    parent_session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    parent_branch_id TEXT NOT NULL REFERENCES branches(id) ON DELETE CASCADE,
    parent_turn_id INTEGER NOT NULL CHECK(parent_turn_id>=1),
    parent_branch_head TEXT NOT NULL,
    parent_branch_revision INTEGER NOT NULL CHECK(parent_branch_revision>=0),
    strategy_revision_id TEXT NOT NULL REFERENCES routing_strategy_revisions(revision_id) ON DELETE RESTRICT,
    planner_identity_json TEXT NOT NULL CHECK(json_valid(planner_identity_json) AND length(planner_identity_json)<=16384),
    input_sha256 TEXT NOT NULL CHECK(length(input_sha256)=64),
    evidence_json TEXT NOT NULL CHECK(json_valid(evidence_json) AND length(evidence_json)<=262144),
    state TEXT NOT NULL CHECK(state IN ('admitted','started','planned','running','joined','failed','cancelled','unknown','interrupted')),
    proposal_sha256 TEXT CHECK(proposal_sha256 IS NULL OR length(proposal_sha256)=64),
    proposal_json TEXT CHECK(proposal_json IS NULL OR (json_valid(proposal_json) AND length(proposal_json)<=65536)),
    metadata_json TEXT CHECK(metadata_json IS NULL OR (json_valid(metadata_json) AND length(metadata_json)<=16384)),
    error TEXT CHECK(error IS NULL OR length(error)<=4096),
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    planned_at INTEGER,
    settled_at INTEGER,
    CHECK(
      (state='admitted' AND started_at IS NULL AND planned_at IS NULL AND settled_at IS NULL AND proposal_json IS NULL) OR
      (state='started' AND started_at IS NOT NULL AND planned_at IS NULL AND settled_at IS NULL AND proposal_json IS NULL) OR
      (state IN ('planned','running') AND started_at IS NOT NULL AND planned_at IS NOT NULL AND settled_at IS NULL AND proposal_json IS NOT NULL) OR
      (state='joined' AND started_at IS NOT NULL AND planned_at IS NOT NULL AND settled_at IS NOT NULL AND proposal_json IS NOT NULL) OR
      (state IN ('failed','cancelled','unknown','interrupted') AND settled_at IS NOT NULL)
    )
);
CREATE INDEX planner_operations_parent
    ON planner_operations(parent_session_id,parent_branch_id,parent_turn_id,created_at);
CREATE INDEX planner_operations_state ON planner_operations(state,created_at);

CREATE TABLE planner_task_runs(
    planner_operation_id TEXT NOT NULL REFERENCES planner_operations(id) ON DELETE CASCADE,
    task_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal>=0),
    role TEXT NOT NULL,
    dependencies_json TEXT NOT NULL CHECK(json_valid(dependencies_json) AND length(dependencies_json)<=4096),
    state TEXT NOT NULL CHECK(state IN ('pending','running','completed','failed','interrupted')),
    subagent_id TEXT UNIQUE,
    child_session_id TEXT UNIQUE REFERENCES sessions(id) ON DELETE SET NULL,
    answer_sha256 TEXT CHECK(answer_sha256 IS NULL OR length(answer_sha256)=64),
    answer_truncated INTEGER NOT NULL DEFAULT 0 CHECK(answer_truncated IN (0,1)),
    started_at INTEGER,
    settled_at INTEGER,
    error TEXT CHECK(error IS NULL OR length(error)<=4096),
    PRIMARY KEY(planner_operation_id,task_id),
    UNIQUE(planner_operation_id,ordinal),
    CHECK(
      (state='pending' AND started_at IS NULL AND settled_at IS NULL AND child_session_id IS NULL) OR
      (state='running' AND started_at IS NOT NULL AND settled_at IS NULL) OR
      (state IN ('completed','failed','interrupted') AND started_at IS NOT NULL AND settled_at IS NOT NULL)
    )
);
CREATE INDEX planner_task_runs_state ON planner_task_runs(state,started_at);

CREATE TABLE planner_join_receipts(
    planner_operation_id TEXT PRIMARY KEY REFERENCES planner_operations(id) ON DELETE CASCADE,
    manifest_json TEXT NOT NULL CHECK(json_valid(manifest_json) AND length(manifest_json)<=65536),
    joined_sha256 TEXT NOT NULL CHECK(length(joined_sha256)=64),
    created_at INTEGER NOT NULL
);

CREATE TRIGGER delegation_root_planner_route_pin_once
BEFORE UPDATE OF planner_routing_revision_id ON delegation_roots
WHEN OLD.planner_routing_revision_id IS NOT NEW.planner_routing_revision_id
     AND OLD.planner_routing_revision_id IS NOT NULL
BEGIN
  SELECT RAISE(ABORT,'delegation planner routing revision is immutable once pinned');
END;

CREATE TRIGGER planner_join_receipts_immutable_update
BEFORE UPDATE ON planner_join_receipts
BEGIN SELECT RAISE(ABORT,'planner join receipts are immutable'); END;
CREATE TRIGGER planner_join_receipts_immutable_delete
BEFORE DELETE ON planner_join_receipts
BEGIN SELECT RAISE(ABORT,'planner join receipts are immutable'); END;

UPDATE store_meta SET value='31' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=31,activation_epoch=31,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=31,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;
PRAGMA user_version=31;
