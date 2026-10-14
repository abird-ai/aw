-- Phase 19: source-side RemoteSessionRefs and remote-side idempotent delegated Session origins.
-- PeerWorkspace origins already exist in schema 4.

CREATE TABLE remote_session_refs (
    ref_id TEXT PRIMARY KEY,
    operation_id TEXT NOT NULL UNIQUE,
    request_sha256 TEXT NOT NULL,
    source_workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id),
    source_session_id TEXT REFERENCES sessions(id),
    remote_peer_id TEXT NOT NULL REFERENCES awnet_peers(peer_id),
    remote_store_id TEXT,
    remote_workspace_id TEXT,
    remote_session_id TEXT,
    remote_branch_id TEXT,
    state TEXT NOT NULL CHECK(state IN ('pending','active','unknown','completed','revoked')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK(
        (state IN ('pending','unknown') AND remote_store_id IS NULL AND remote_workspace_id IS NULL AND remote_session_id IS NULL AND remote_branch_id IS NULL)
        OR
        (state IN ('active','completed','revoked') AND remote_store_id IS NOT NULL AND remote_workspace_id IS NOT NULL AND remote_session_id IS NOT NULL AND remote_branch_id IS NOT NULL)
    )
);
CREATE INDEX remote_session_refs_by_workspace
    ON remote_session_refs(source_workspace_id,state,created_at,ref_id);
CREATE INDEX remote_session_refs_by_peer
    ON remote_session_refs(remote_peer_id,state,created_at,ref_id);

CREATE TABLE remote_session_origins (
    session_id TEXT PRIMARY KEY REFERENCES sessions(id),
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id),
    source_store_id TEXT NOT NULL,
    source_domain_id TEXT NOT NULL,
    source_workspace_id TEXT NOT NULL,
    source_session_id TEXT,
    source_branch_id TEXT,
    source_operation_id TEXT NOT NULL,
    request_sha256 TEXT NOT NULL,
    source_node_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(source_store_id,source_operation_id)
);
CREATE INDEX remote_session_origins_by_source_workspace
    ON remote_session_origins(source_store_id,source_domain_id,source_workspace_id,created_at);

PRAGMA user_version=5;
