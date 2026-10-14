-- Phase 42: durable intent/receipt state for host-managed isolated subagent worktrees.
-- Path identity is stored as exact platform bytes (BLOB), never lossy display text.

CREATE TABLE subagent_execution_bindings(
    id TEXT PRIMARY KEY,
    parent_session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    parent_branch_id TEXT NOT NULL REFERENCES branches(id) ON DELETE CASCADE,
    parent_turn_id INTEGER NOT NULL CHECK(parent_turn_id>=1),
    created_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    kind TEXT NOT NULL CHECK(kind='managed_worktree'),
    tool_mode TEXT NOT NULL CHECK(tool_mode IN ('read-only','workspace')),
    parent_repo_root BLOB NOT NULL CHECK(length(parent_repo_root)>0 AND length(parent_repo_root)<=16384),
    execution_root BLOB NOT NULL UNIQUE CHECK(length(execution_root)>0 AND length(execution_root)<=16384),
    base_revision TEXT NOT NULL CHECK(length(base_revision) IN (40,64)),
    state TEXT NOT NULL CHECK(state IN ('planned','ready','bound','cleanup_planned','unknown','removed')),
    child_session_id TEXT UNIQUE REFERENCES sessions(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    ready_at INTEGER,
    bound_at INTEGER,
    cleanup_started_at INTEGER,
    removed_at INTEGER,
    error TEXT CHECK(error IS NULL OR length(error)<=4096),
    CHECK(
      (state='planned' AND ready_at IS NULL AND bound_at IS NULL AND cleanup_started_at IS NULL AND removed_at IS NULL AND child_session_id IS NULL) OR
      (state='ready' AND ready_at IS NOT NULL AND bound_at IS NULL AND cleanup_started_at IS NULL AND removed_at IS NULL AND child_session_id IS NULL) OR
      (state='bound' AND ready_at IS NOT NULL AND bound_at IS NOT NULL AND cleanup_started_at IS NULL AND removed_at IS NULL AND child_session_id IS NOT NULL) OR
      (state='cleanup_planned' AND ready_at IS NOT NULL AND cleanup_started_at IS NOT NULL AND removed_at IS NULL) OR
      (state='unknown' AND removed_at IS NULL) OR
      (state='removed' AND ready_at IS NOT NULL AND cleanup_started_at IS NOT NULL AND removed_at IS NOT NULL)
    )
);
CREATE INDEX subagent_execution_bindings_parent
    ON subagent_execution_bindings(parent_session_id,parent_branch_id,created_at);
CREATE INDEX subagent_execution_bindings_child
    ON subagent_execution_bindings(child_session_id);
CREATE INDEX subagent_execution_bindings_state
    ON subagent_execution_bindings(state,created_at);

UPDATE store_meta SET value='26' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=26,activation_epoch=26,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=26,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;
PRAGMA user_version=26;
