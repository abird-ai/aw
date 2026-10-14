-- Phase 43: durable root delegation envelopes and shared child/model budgets.

CREATE TABLE delegation_roots(
    id TEXT PRIMARY KEY,
    root_session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    root_branch_id TEXT NOT NULL REFERENCES branches(id) ON DELETE CASCADE,
    root_turn_id INTEGER NOT NULL CHECK(root_turn_id>=1),
    created_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    state TEXT NOT NULL CHECK(state IN ('active','exhausted','cancelled')),
    max_children_created INTEGER NOT NULL CHECK(max_children_created BETWEEN 1 AND 32),
    max_active_children INTEGER NOT NULL CHECK(max_active_children BETWEEN 1 AND 4),
    max_model_attempts INTEGER NOT NULL CHECK(max_model_attempts BETWEEN 1 AND 256),
    max_depth INTEGER NOT NULL CHECK(max_depth=1),
    deadline_seconds INTEGER NOT NULL CHECK(deadline_seconds BETWEEN 1 AND 86400),
    deadline_at INTEGER NOT NULL,
    children_created INTEGER NOT NULL DEFAULT 0 CHECK(children_created>=0 AND children_created<=max_children_created),
    active_children INTEGER NOT NULL DEFAULT 0 CHECK(active_children>=0 AND active_children<=max_active_children),
    model_attempts_started INTEGER NOT NULL DEFAULT 0 CHECK(model_attempts_started>=0 AND model_attempts_started<=max_model_attempts),
    model_attempts_finished INTEGER NOT NULL DEFAULT 0 CHECK(model_attempts_finished>=0 AND model_attempts_finished<=model_attempts_started),
    input_tokens_known INTEGER NOT NULL DEFAULT 0 CHECK(input_tokens_known>=0),
    output_tokens_known INTEGER NOT NULL DEFAULT 0 CHECK(output_tokens_known>=0),
    calls_without_complete_usage INTEGER NOT NULL DEFAULT 0 CHECK(calls_without_complete_usage>=0 AND calls_without_complete_usage<=model_attempts_finished),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    cancelled_at INTEGER,
    UNIQUE(root_session_id,root_branch_id,root_turn_id),
    CHECK(max_active_children<=max_children_created)
);
CREATE INDEX delegation_roots_state ON delegation_roots(state,deadline_at);

CREATE TABLE delegation_child_allocations(
    id TEXT PRIMARY KEY,
    root_id TEXT NOT NULL REFERENCES delegation_roots(id) ON DELETE CASCADE,
    parent_session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    parent_branch_id TEXT NOT NULL REFERENCES branches(id) ON DELETE CASCADE,
    parent_turn_id INTEGER NOT NULL CHECK(parent_turn_id>=1),
    depth INTEGER NOT NULL CHECK(depth=1),
    state TEXT NOT NULL CHECK(state IN ('reserved','bound','running','completed','failed','cancelled','interrupted')),
    child_session_id TEXT UNIQUE REFERENCES sessions(id) ON DELETE SET NULL,
    execution_binding_id TEXT UNIQUE REFERENCES subagent_execution_bindings(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    bound_at INTEGER,
    running_at INTEGER,
    settled_at INTEGER,
    error TEXT CHECK(error IS NULL OR length(error)<=4096),
    CHECK(
      (state='reserved' AND child_session_id IS NULL AND bound_at IS NULL AND running_at IS NULL AND settled_at IS NULL) OR
      (state='bound' AND child_session_id IS NOT NULL AND bound_at IS NOT NULL AND running_at IS NULL AND settled_at IS NULL) OR
      (state='running' AND child_session_id IS NOT NULL AND bound_at IS NOT NULL AND running_at IS NOT NULL AND settled_at IS NULL) OR
      (state IN ('completed','cancelled','interrupted') AND child_session_id IS NOT NULL AND bound_at IS NOT NULL AND settled_at IS NOT NULL) OR
      (state='failed' AND settled_at IS NOT NULL)
    )
);
CREATE INDEX delegation_child_allocations_root ON delegation_child_allocations(root_id,created_at);
CREATE INDEX delegation_child_allocations_state ON delegation_child_allocations(state,created_at);

CREATE TABLE delegation_model_attempts(
    root_id TEXT NOT NULL REFERENCES delegation_roots(id) ON DELETE CASCADE,
    child_session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn_id INTEGER NOT NULL CHECK(turn_id>=1),
    ordinal INTEGER NOT NULL CHECK(ordinal>=1),
    invocation_identity_json TEXT NOT NULL CHECK(json_valid(invocation_identity_json) AND length(invocation_identity_json)<=16384),
    state TEXT NOT NULL CHECK(state IN ('started','finished','unknown')),
    outcome TEXT,
    input_tokens INTEGER CHECK(input_tokens IS NULL OR input_tokens>=0),
    output_tokens INTEGER CHECK(output_tokens IS NULL OR output_tokens>=0),
    usage_complete INTEGER NOT NULL DEFAULT 0 CHECK(usage_complete IN (0,1)),
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    PRIMARY KEY(root_id,child_session_id,turn_id,ordinal)
);
CREATE INDEX delegation_model_attempts_state ON delegation_model_attempts(state,started_at);

ALTER TABLE subagent_sessions ADD COLUMN delegation_root_id TEXT REFERENCES delegation_roots(id) ON DELETE CASCADE;
ALTER TABLE subagent_sessions ADD COLUMN delegation_allocation_id TEXT REFERENCES delegation_child_allocations(id) ON DELETE CASCADE;
ALTER TABLE subagent_sessions ADD COLUMN depth INTEGER CHECK(depth IS NULL OR depth=1);
ALTER TABLE subagent_execution_bindings ADD COLUMN delegation_root_id TEXT REFERENCES delegation_roots(id) ON DELETE CASCADE;
ALTER TABLE subagent_execution_bindings ADD COLUMN delegation_allocation_id TEXT REFERENCES delegation_child_allocations(id) ON DELETE CASCADE;

CREATE UNIQUE INDEX subagent_sessions_delegation_allocation
    ON subagent_sessions(delegation_allocation_id)
    WHERE delegation_allocation_id IS NOT NULL;
CREATE UNIQUE INDEX subagent_execution_delegation_allocation
    ON subagent_execution_bindings(delegation_allocation_id)
    WHERE delegation_allocation_id IS NOT NULL;

CREATE TRIGGER subagent_sessions_phase43_require_delegation
BEFORE INSERT ON subagent_sessions
WHEN NEW.delegation_root_id IS NULL OR NEW.delegation_allocation_id IS NULL OR NEW.depth IS NULL
BEGIN
  SELECT RAISE(ABORT,'new subagents require Phase43 delegation root/allocation/depth');
END;

CREATE TRIGGER subagent_execution_phase43_require_delegation
BEFORE INSERT ON subagent_execution_bindings
WHEN NEW.delegation_root_id IS NULL OR NEW.delegation_allocation_id IS NULL
BEGIN
  SELECT RAISE(ABORT,'new subagent execution binding requires Phase43 delegation allocation');
END;

CREATE TRIGGER subagent_sessions_delegation_immutable
BEFORE UPDATE OF delegation_root_id,delegation_allocation_id,depth ON subagent_sessions
BEGIN
  SELECT RAISE(ABORT,'subagent delegation linkage is immutable');
END;

UPDATE store_meta SET value='27' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=27,activation_epoch=27,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=27,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;
PRAGMA user_version=27;
