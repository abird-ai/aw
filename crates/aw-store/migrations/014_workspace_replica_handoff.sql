-- Phase 29: semantic Workspace replica snapshots and writer handoff provenance.
-- Replication is application-level immutable objects + exact ref projections.
-- Raw SQLite pages/WAL and generic row replication are not protocols.

-- A transaction-scoped marker suppresses the normal local epoch-1 Workspace
-- bootstrap trigger while a signed replica is being materialized with its exact
-- writer state. Rows should never survive a successful import transaction.
CREATE TABLE workspace_replica_import_intents (
    workspace_id TEXT PRIMARY KEY,
    source_node_id TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE workspace_replica_origins (
    workspace_id TEXT PRIMARY KEY REFERENCES workspaces(workspace_id),
    source_domain_id TEXT NOT NULL,
    source_workspace_id TEXT NOT NULL,
    source_authority_store_id TEXT NOT NULL,
    control_node_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(source_authority_store_id,source_workspace_id)
);

-- Domain rows materialized only to satisfy the local object/ref graph are marked
-- explicitly. They carry no local membership/identity binding or AuthoritySet and
-- therefore cannot become a local Domain control authority accidentally.
CREATE TABLE workspace_replica_domains (
    domain_id TEXT PRIMARY KEY REFERENCES domains(domain_id),
    source_authority_store_id TEXT NOT NULL,
    source_node_id TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE workspace_replica_snapshots (
    snapshot_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id),
    source_store_id TEXT NOT NULL,
    source_node_id TEXT NOT NULL,
    writer_epoch INTEGER NOT NULL CHECK(writer_epoch>0),
    authority_term INTEGER NOT NULL CHECK(authority_term>=0),
    snapshot_sha256 TEXT NOT NULL CHECK(length(snapshot_sha256)=64),
    body_json TEXT NOT NULL CHECK(json_valid(body_json)),
    source_signature TEXT NOT NULL,
    imported_at INTEGER NOT NULL,
    UNIQUE(source_store_id,workspace_id,snapshot_sha256)
);
CREATE INDEX workspace_replica_snapshots_by_workspace
    ON workspace_replica_snapshots(workspace_id,writer_epoch,imported_at);
CREATE TRIGGER workspace_replica_snapshots_immutable_update
BEFORE UPDATE ON workspace_replica_snapshots
BEGIN SELECT RAISE(ABORT,'Workspace replica snapshot immutable'); END;
CREATE TRIGGER workspace_replica_snapshots_immutable_delete
BEFORE DELETE ON workspace_replica_snapshots
BEGIN SELECT RAISE(ABORT,'Workspace replica snapshot immutable'); END;

CREATE TABLE workspace_replica_divergences (
    divergence_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id),
    session_id TEXT NOT NULL REFERENCES sessions(id),
    canonical_branch_id TEXT NOT NULL REFERENCES branches(id),
    preserved_branch_id TEXT NOT NULL REFERENCES branches(id),
    local_writer_epoch INTEGER NOT NULL CHECK(local_writer_epoch>0),
    incoming_writer_epoch INTEGER NOT NULL CHECK(incoming_writer_epoch>0),
    local_head TEXT NOT NULL REFERENCES nodes(id),
    incoming_head TEXT NOT NULL REFERENCES nodes(id),
    detected_at INTEGER NOT NULL,
    UNIQUE(workspace_id,session_id,canonical_branch_id,local_writer_epoch,local_head,incoming_writer_epoch,incoming_head)
);
CREATE TRIGGER workspace_replica_divergences_immutable_update
BEFORE UPDATE ON workspace_replica_divergences
BEGIN SELECT RAISE(ABORT,'Workspace replica divergence immutable'); END;
CREATE TRIGGER workspace_replica_divergences_immutable_delete
BEFORE DELETE ON workspace_replica_divergences
BEGIN SELECT RAISE(ABORT,'Workspace replica divergence immutable'); END;

DROP TRIGGER workspace_writer_state_after_workspace_insert;
CREATE TRIGGER workspace_writer_state_after_workspace_insert
AFTER INSERT ON workspaces
WHEN NOT EXISTS(
    SELECT 1 FROM workspace_replica_import_intents i
    WHERE i.workspace_id=NEW.workspace_id
)
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

PRAGMA user_version=14;
