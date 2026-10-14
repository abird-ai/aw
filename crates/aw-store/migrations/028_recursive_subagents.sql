-- Phase 44: bounded recursive subagent lineage over the shared Phase-43 root envelope.
-- Historical v27 max_depth/depth columns remain compatibility evidence fixed at 1;
-- recursive_* columns are authoritative for Phase-44 execution.

ALTER TABLE delegation_roots
ADD COLUMN recursive_max_depth INTEGER NOT NULL DEFAULT 1
    CHECK(recursive_max_depth BETWEEN 1 AND 4);

ALTER TABLE delegation_child_allocations
ADD COLUMN recursive_depth INTEGER NOT NULL DEFAULT 1
    CHECK(recursive_depth BETWEEN 1 AND 4);

ALTER TABLE delegation_child_allocations
ADD COLUMN parent_allocation_id TEXT REFERENCES delegation_child_allocations(id) ON DELETE SET NULL;

ALTER TABLE subagent_sessions
ADD COLUMN recursive_depth INTEGER NOT NULL DEFAULT 1
    CHECK(recursive_depth BETWEEN 1 AND 4);

CREATE INDEX delegation_child_allocations_parent
    ON delegation_child_allocations(parent_allocation_id,created_at);
CREATE INDEX subagent_sessions_recursive_depth
    ON subagent_sessions(delegation_root_id,recursive_depth,created_at);

CREATE TRIGGER delegation_root_recursive_depth_immutable
BEFORE UPDATE OF recursive_max_depth ON delegation_roots
BEGIN
  SELECT RAISE(ABORT,'delegation recursive max depth is immutable');
END;

CREATE TRIGGER delegation_child_recursive_lineage_insert
BEFORE INSERT ON delegation_child_allocations
BEGIN
  SELECT CASE
    WHEN NEW.recursive_depth=1 AND NEW.parent_allocation_id IS NOT NULL
      THEN RAISE(ABORT,'root child cannot have parent allocation')
    WHEN NEW.recursive_depth>1 AND NEW.parent_allocation_id IS NULL
      THEN RAISE(ABORT,'recursive child requires parent allocation')
  END;
  SELECT CASE WHEN NOT EXISTS(
    SELECT 1 FROM delegation_roots r
    WHERE r.id=NEW.root_id AND NEW.recursive_depth<=r.recursive_max_depth
  ) THEN RAISE(ABORT,'recursive child exceeds root max depth') END;
  SELECT CASE WHEN NEW.parent_allocation_id IS NOT NULL AND NOT EXISTS(
    SELECT 1 FROM delegation_child_allocations p
    WHERE p.id=NEW.parent_allocation_id
      AND p.root_id=NEW.root_id
      AND p.recursive_depth+1=NEW.recursive_depth
      AND p.state='running'
  ) THEN RAISE(ABORT,'recursive child parent allocation mismatch') END;
END;

CREATE TRIGGER subagent_sessions_recursive_lineage_insert
BEFORE INSERT ON subagent_sessions
BEGIN
  SELECT CASE WHEN NOT EXISTS(
    SELECT 1 FROM delegation_child_allocations a
    WHERE a.id=NEW.delegation_allocation_id
      AND a.root_id=NEW.delegation_root_id
      AND a.recursive_depth=NEW.recursive_depth
  ) THEN RAISE(ABORT,'subagent recursive depth does not match allocation') END;
END;

CREATE TRIGGER subagent_sessions_recursive_depth_immutable
BEFORE UPDATE OF recursive_depth ON subagent_sessions
BEGIN
  SELECT RAISE(ABORT,'subagent recursive depth is immutable');
END;

UPDATE store_meta SET value='28' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=28,activation_epoch=28,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=28,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;
PRAGMA user_version=28;
