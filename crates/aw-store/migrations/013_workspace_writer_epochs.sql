-- Phase 28: Workspace mutable-ref writer epochs and failover fencing.
-- This remains inside the existing single-owner SQLite process. It does not
-- enable generic remote row writes or direct multi-process database ownership.

CREATE TABLE workspace_writer_state (
    workspace_id TEXT PRIMARY KEY REFERENCES workspaces(workspace_id),
    domain_id TEXT NOT NULL REFERENCES domains(domain_id),
    writer_node_id TEXT NOT NULL,
    writer_epoch INTEGER NOT NULL CHECK(writer_epoch>0),
    authority_term INTEGER NOT NULL CHECK(authority_term>=0),
    state TEXT NOT NULL CHECK(state IN ('active','read_only')),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
    updated_at INTEGER NOT NULL
);
CREATE INDEX workspace_writer_state_by_domain
    ON workspace_writer_state(domain_id,state,writer_node_id,workspace_id);

CREATE TABLE workspace_writer_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL UNIQUE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id),
    domain_id TEXT NOT NULL REFERENCES domains(domain_id),
    old_writer_node_id TEXT,
    old_writer_epoch INTEGER,
    new_writer_node_id TEXT NOT NULL,
    new_writer_epoch INTEGER NOT NULL CHECK(new_writer_epoch>0),
    authority_term INTEGER NOT NULL CHECK(authority_term>=0),
    actor TEXT NOT NULL,
    reason TEXT NOT NULL CHECK(length(reason) BETWEEN 1 AND 64),
    created_at INTEGER NOT NULL,
    CHECK(old_writer_epoch IS NULL OR old_writer_epoch < new_writer_epoch)
);
CREATE INDEX workspace_writer_events_by_workspace
    ON workspace_writer_events(workspace_id,seq);
CREATE TRIGGER workspace_writer_events_immutable_update
BEFORE UPDATE ON workspace_writer_events
BEGIN SELECT RAISE(ABORT,'Workspace writer event immutable'); END;
CREATE TRIGGER workspace_writer_events_immutable_delete
BEFORE DELETE ON workspace_writer_events
BEGIN SELECT RAISE(ABORT,'Workspace writer event immutable'); END;

-- Existing Workspaces are locally authoritative before Phase 28.
INSERT INTO workspace_writer_state(
    workspace_id,domain_id,writer_node_id,writer_epoch,authority_term,state,revision,updated_at
)
SELECT w.workspace_id,w.domain_id,
       (SELECT value FROM store_meta WHERE key='local_node_id'),
       1,
       COALESCE((SELECT current_term FROM domain_authority_sets a WHERE a.domain_id=w.domain_id),0),
       'active',1,w.updated_at
FROM workspaces w;

INSERT INTO workspace_writer_events(
    event_id,workspace_id,domain_id,old_writer_node_id,old_writer_epoch,
    new_writer_node_id,new_writer_epoch,authority_term,actor,reason,created_at
)
SELECT 'wwe_' || lower(hex(randomblob(16))),w.workspace_id,w.domain_id,NULL,NULL,
       s.writer_node_id,1,s.authority_term,'migration','bootstrap',w.updated_at
FROM workspaces w JOIN workspace_writer_state s ON s.workspace_id=w.workspace_id;

-- New local durable Workspaces bootstrap to the current store's AwNode. Any
-- subsequent transfer/failover is explicit through the Phase 28 store API.
CREATE TRIGGER workspace_writer_state_after_workspace_insert
AFTER INSERT ON workspaces
BEGIN
  INSERT INTO workspace_writer_state(
      workspace_id,domain_id,writer_node_id,writer_epoch,authority_term,state,revision,updated_at
  ) VALUES(
      NEW.workspace_id,NEW.domain_id,
      (SELECT value FROM store_meta WHERE key='local_node_id'),
      1,
      COALESCE((SELECT current_term FROM domain_authority_sets WHERE domain_id=NEW.domain_id),0),
      'active',1,NEW.updated_at
  );
  INSERT INTO workspace_writer_events(
      event_id,workspace_id,domain_id,old_writer_node_id,old_writer_epoch,
      new_writer_node_id,new_writer_epoch,authority_term,actor,reason,created_at
  ) VALUES(
      'wwe_' || lower(hex(randomblob(16))),NEW.workspace_id,NEW.domain_id,NULL,NULL,
      (SELECT value FROM store_meta WHERE key='local_node_id'),1,
      COALESCE((SELECT current_term FROM domain_authority_sets WHERE domain_id=NEW.domain_id),0),
      'system','workspace_created',NEW.updated_at
  );
END;

ALTER TABLE branch_ref_events ADD COLUMN writer_node_id TEXT;
ALTER TABLE branch_ref_events ADD COLUMN writer_epoch INTEGER NOT NULL DEFAULT 1 CHECK(writer_epoch>0);

ALTER TABLE llm_operations ADD COLUMN origin_writer_epoch INTEGER NOT NULL DEFAULT 1 CHECK(origin_writer_epoch>0);
ALTER TABLE runs ADD COLUMN produced_from_writer_epoch INTEGER NOT NULL DEFAULT 1 CHECK(produced_from_writer_epoch>0);

PRAGMA user_version=13;
