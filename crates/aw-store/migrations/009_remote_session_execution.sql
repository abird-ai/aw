-- Phase 24: exact remote Session execution receipts.
-- A peer execution ID is durable before local-owner dispatch. Unknown outcomes
-- remain lookup-only; the same logical request is never silently regenerated.

CREATE TABLE remote_session_executions (
    source_node_id TEXT NOT NULL,
    execution_id TEXT NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    branch_id TEXT NOT NULL REFERENCES branches(id),
    prompt_sha256 TEXT NOT NULL CHECK(length(prompt_sha256)=64),
    owner_request_json TEXT NOT NULL CHECK(json_valid(owner_request_json) AND length(owner_request_json)<=524288),
    state TEXT NOT NULL CHECK(state IN ('prepared','unknown','active','settled','cancelled')),
    run_id INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(source_node_id,execution_id),
    CHECK(run_id IS NULL OR run_id>0)
);
CREATE INDEX remote_session_executions_by_session
    ON remote_session_executions(session_id,state,updated_at,execution_id);

PRAGMA user_version=9;
