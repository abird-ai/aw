-- Phase 41: bounded child/subagent Sessions.
-- A subagent is an ordinary durable Session plus this explicit parent relation.
-- The relation stores secret-free invocation/delegation evidence only.

CREATE TABLE subagent_sessions(
    id TEXT PRIMARY KEY,
    parent_session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    parent_branch_id TEXT NOT NULL REFERENCES branches(id) ON DELETE CASCADE,
    parent_head TEXT NOT NULL REFERENCES nodes(id),
    parent_branch_revision INTEGER NOT NULL CHECK(parent_branch_revision>=0),
    parent_turn_id INTEGER NOT NULL CHECK(parent_turn_id>=1),
    child_session_id TEXT NOT NULL UNIQUE REFERENCES sessions(id) ON DELETE CASCADE,
    created_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    mode TEXT NOT NULL CHECK(mode IN ('foreground','background')),
    state TEXT NOT NULL CHECK(state IN ('admitted','running','completed','failed','cancelled','interrupted')),
    task_sha256 TEXT NOT NULL CHECK(length(task_sha256)=64),
    invocation_identity_json TEXT NOT NULL
        CHECK(json_valid(invocation_identity_json) AND length(invocation_identity_json)<=16384),
    tool_mode TEXT NOT NULL CHECK(tool_mode IN ('off','read-only','workspace','trusted')),
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    settled_at INTEGER,
    error TEXT,
    CHECK(
      (state='admitted' AND started_at IS NULL AND settled_at IS NULL) OR
      (state='running' AND started_at IS NOT NULL AND settled_at IS NULL) OR
      (state IN ('completed','failed','cancelled','interrupted')
       AND started_at IS NOT NULL AND settled_at IS NOT NULL)
    )
);
CREATE INDEX subagent_sessions_parent
    ON subagent_sessions(parent_session_id,parent_branch_id,created_at);
CREATE INDEX subagent_sessions_state
    ON subagent_sessions(state,created_at);
CREATE INDEX subagent_sessions_child ON subagent_sessions(child_session_id);

UPDATE store_meta SET value='25' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=25,activation_epoch=25,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=25,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;
PRAGMA user_version=25;
