-- Phase 40: bounded main-target routing and per-user branch-lineage affinity.
-- Definitions and receipts contain only secret-free target/account/model refs.

CREATE TABLE routing_strategies(
    id TEXT PRIMARY KEY,
    owner_user_id TEXT NOT NULL REFERENCES users(user_id),
    name TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('active','archived')),
    created_at INTEGER NOT NULL,
    UNIQUE(owner_user_id,name)
);

CREATE TABLE routing_strategy_revisions(
    revision_id TEXT PRIMARY KEY,
    strategy_id TEXT NOT NULL REFERENCES routing_strategies(id) ON DELETE CASCADE,
    revision_no INTEGER NOT NULL CHECK(revision_no>=1),
    definition_sha256 TEXT NOT NULL CHECK(length(definition_sha256)=64),
    definition_json TEXT NOT NULL CHECK(json_valid(definition_json) AND length(definition_json)<=262144),
    created_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    created_at INTEGER NOT NULL,
    UNIQUE(strategy_id,revision_no),
    UNIQUE(strategy_id,definition_sha256)
);
CREATE TRIGGER routing_strategy_revisions_immutable_update
BEFORE UPDATE ON routing_strategy_revisions BEGIN
    SELECT RAISE(ABORT,'routing strategy revisions are immutable');
END;
CREATE TRIGGER routing_strategy_revisions_immutable_delete
BEFORE DELETE ON routing_strategy_revisions BEGIN
    SELECT RAISE(ABORT,'routing strategy revisions are immutable');
END;

CREATE TABLE routing_bindings(
    id TEXT PRIMARY KEY,
    strategy_id TEXT NOT NULL REFERENCES routing_strategies(id) ON DELETE CASCADE,
    revision_id TEXT NOT NULL REFERENCES routing_strategy_revisions(revision_id),
    scope_kind TEXT NOT NULL CHECK(scope_kind IN ('user','session')),
    scope_user_id TEXT REFERENCES users(user_id),
    scope_session_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK(length(role) BETWEEN 1 AND 64),
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
CREATE UNIQUE INDEX routing_bindings_active_user
    ON routing_bindings(scope_user_id,role)
    WHERE scope_kind='user' AND status='active';
CREATE UNIQUE INDEX routing_bindings_active_session
    ON routing_bindings(scope_session_id,role)
    WHERE scope_kind='session' AND status='active';
CREATE INDEX routing_bindings_strategy ON routing_bindings(strategy_id,revision_id,status);

CREATE TABLE routing_operations(
    id TEXT PRIMARY KEY,
    strategy_id TEXT NOT NULL REFERENCES routing_strategies(id),
    revision_id TEXT NOT NULL REFERENCES routing_strategy_revisions(revision_id),
    actor_user_id TEXT NOT NULL REFERENCES users(user_id),
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    branch_id TEXT NOT NULL REFERENCES branches(id) ON DELETE CASCADE,
    branch_head TEXT NOT NULL,
    branch_revision INTEGER NOT NULL CHECK(branch_revision>=0),
    role TEXT NOT NULL CHECK(length(role) BETWEEN 1 AND 64),
    lineage TEXT NOT NULL CHECK(length(lineage) BETWEEN 1 AND 128),
    mode TEXT NOT NULL CHECK(mode IN ('fixed','route_once','route_each_prompt')),
    selection_source TEXT NOT NULL CHECK(selection_source IN ('fixed','affinity','classifier')),
    input_sha256 TEXT NOT NULL CHECK(length(input_sha256)=64),
    candidate_ids_sha256 TEXT NOT NULL CHECK(length(candidate_ids_sha256)=64),
    state TEXT NOT NULL CHECK(state IN ('admitted','classifier_started','selected','failed','cancelled','unknown_external_effect','conflict')),
    selected_target_id TEXT,
    working_identity_json TEXT CHECK(working_identity_json IS NULL OR (json_valid(working_identity_json) AND length(working_identity_json)<=16384)),
    consumed_at INTEGER,
    error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK((state='selected' AND selected_target_id IS NOT NULL) OR state<>'selected')
);
CREATE INDEX routing_operations_actor_state ON routing_operations(actor_user_id,state,updated_at);
CREATE INDEX routing_operations_session ON routing_operations(session_id,branch_id,created_at);

CREATE TABLE routing_attempts(
    id TEXT PRIMARY KEY,
    operation_id TEXT NOT NULL UNIQUE REFERENCES routing_operations(id) ON DELETE CASCADE,
    attempt_no INTEGER NOT NULL CHECK(attempt_no=1),
    classifier_identity_json TEXT NOT NULL CHECK(json_valid(classifier_identity_json) AND length(classifier_identity_json)<=16384),
    state TEXT NOT NULL CHECK(state IN ('started','completed','failed','cancelled','unknown_external_effect')),
    metadata_json TEXT CHECK(metadata_json IS NULL OR (json_valid(metadata_json) AND length(metadata_json)<=32768)),
    selected_target_id TEXT,
    error TEXT,
    owner_process_id TEXT NOT NULL REFERENCES local_process_incarnations(process_id),
    owner_store_generation INTEGER NOT NULL CHECK(owner_store_generation>0),
    started_at INTEGER NOT NULL,
    settled_at INTEGER,
    CHECK((state='started' AND settled_at IS NULL) OR (state<>'started' AND settled_at IS NOT NULL))
);
CREATE INDEX routing_attempts_owner ON routing_attempts(owner_process_id,owner_store_generation,state);

CREATE TABLE routing_affinities(
    actor_user_id TEXT NOT NULL REFERENCES users(user_id),
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    branch_id TEXT NOT NULL REFERENCES branches(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK(length(role) BETWEEN 1 AND 64),
    lineage TEXT NOT NULL CHECK(length(lineage) BETWEEN 1 AND 128),
    strategy_revision_id TEXT NOT NULL REFERENCES routing_strategy_revisions(revision_id),
    target_id TEXT NOT NULL CHECK(length(target_id) BETWEEN 1 AND 128),
    established_operation_id TEXT NOT NULL REFERENCES routing_operations(id),
    revision INTEGER NOT NULL CHECK(revision>=1),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(actor_user_id,session_id,branch_id,role,lineage)
);
CREATE INDEX routing_affinities_strategy ON routing_affinities(strategy_revision_id,target_id);

-- Link the working Chat logical operation to the exact Phase-40 route receipt.
-- Existing rows remain NULL; retries of routed operations can reconstruct the
-- same pinned target while direct operations remain unchanged.
ALTER TABLE llm_operations ADD COLUMN routing_operation_id TEXT REFERENCES routing_operations(id);
CREATE INDEX llm_operations_routing_operation ON llm_operations(routing_operation_id);

UPDATE store_meta SET value='24' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=24,activation_epoch=24,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=24,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;
PRAGMA user_version=24;
