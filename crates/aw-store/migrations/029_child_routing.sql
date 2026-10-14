-- Phase 45: governed child target routing over Phase-40 immutable strategy revisions.

ALTER TABLE delegation_roots
ADD COLUMN child_routing_revision_id TEXT REFERENCES routing_strategy_revisions(revision_id) ON DELETE RESTRICT;

ALTER TABLE delegation_child_allocations
ADD COLUMN routing_operation_id TEXT REFERENCES routing_operations(id) ON DELETE SET NULL;
ALTER TABLE delegation_child_allocations
ADD COLUMN routing_target_id TEXT;
ALTER TABLE delegation_child_allocations
ADD COLUMN routing_selection_source TEXT
    CHECK(routing_selection_source IS NULL OR routing_selection_source IN ('inherit','fixed','affinity','classifier'));
ALTER TABLE delegation_child_allocations
ADD COLUMN selected_identity_json TEXT
    CHECK(selected_identity_json IS NULL OR (json_valid(selected_identity_json) AND length(selected_identity_json)<=16384));
ALTER TABLE delegation_child_allocations
ADD COLUMN routing_bound_at INTEGER;

CREATE UNIQUE INDEX delegation_child_allocations_route_operation
    ON delegation_child_allocations(routing_operation_id)
    WHERE routing_operation_id IS NOT NULL;
CREATE INDEX delegation_roots_child_route
    ON delegation_roots(child_routing_revision_id)
    WHERE child_routing_revision_id IS NOT NULL;

CREATE TABLE delegation_routing_attempts(
    root_id TEXT NOT NULL REFERENCES delegation_roots(id) ON DELETE CASCADE,
    allocation_id TEXT NOT NULL UNIQUE REFERENCES delegation_child_allocations(id) ON DELETE CASCADE,
    routing_operation_id TEXT NOT NULL UNIQUE REFERENCES routing_operations(id) ON DELETE CASCADE,
    classifier_identity_json TEXT NOT NULL CHECK(json_valid(classifier_identity_json) AND length(classifier_identity_json)<=16384),
    state TEXT NOT NULL CHECK(state IN ('started','finished','unknown')),
    outcome TEXT,
    metadata_json TEXT CHECK(metadata_json IS NULL OR (json_valid(metadata_json) AND length(metadata_json)<=16384)),
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    PRIMARY KEY(root_id,allocation_id,routing_operation_id)
);
CREATE INDEX delegation_routing_attempts_state
    ON delegation_routing_attempts(state,started_at);

CREATE TRIGGER delegation_root_child_route_immutable
BEFORE UPDATE OF child_routing_revision_id ON delegation_roots
BEGIN
  SELECT RAISE(ABORT,'delegation child routing revision is immutable');
END;

CREATE TRIGGER subagent_execution_phase45_route_bound
BEFORE INSERT ON subagent_execution_bindings
WHEN EXISTS(
  SELECT 1
  FROM delegation_child_allocations a
  JOIN delegation_roots r ON r.id=a.root_id
  WHERE a.id=NEW.delegation_allocation_id
    AND r.child_routing_revision_id IS NOT NULL
    AND a.selected_identity_json IS NULL
)
BEGIN
  SELECT RAISE(ABORT,'routed child identity must be selected before worktree planning');
END;

CREATE TRIGGER subagent_sessions_phase45_selected_identity
BEFORE INSERT ON subagent_sessions
WHEN EXISTS(
  SELECT 1
  FROM delegation_child_allocations a
  JOIN delegation_roots r ON r.id=a.root_id
  WHERE a.id=NEW.delegation_allocation_id
    AND r.child_routing_revision_id IS NOT NULL
    AND (
      a.selected_identity_json IS NULL OR
      a.routing_operation_id IS NULL OR
      a.routing_target_id IS NULL OR
      a.routing_selection_source NOT IN ('fixed','affinity','classifier') OR
      a.selected_identity_json<>NEW.invocation_identity_json
    )
)
BEGIN
  SELECT RAISE(ABORT,'routed child Session identity does not match selected route');
END;

UPDATE store_meta SET value='29' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=29,activation_epoch=29,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=29,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;
PRAGMA user_version=29;
