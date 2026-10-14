-- Phase 20: immutable Fleet fan-out operation membership snapshots and per-member durable state.

CREATE TABLE fleet_operations (
    operation_id TEXT PRIMARY KEY,
    fleet_id TEXT NOT NULL REFERENCES fleets(fleet_id),
    source_workspace_id TEXT REFERENCES workspaces(workspace_id),
    request_sha256 TEXT NOT NULL,
    membership_revision INTEGER NOT NULL CHECK(membership_revision>0),
    execution_policy TEXT NOT NULL CHECK(execution_policy IN ('all','one','any_healthy','quorum')),
    state TEXT NOT NULL CHECK(state IN ('pending','running','partial','completed','unknown','revoked')),
    created_by TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX fleet_operations_by_fleet
    ON fleet_operations(fleet_id,state,created_at,operation_id);

CREATE TABLE fleet_operation_members (
    operation_id TEXT NOT NULL REFERENCES fleet_operations(operation_id),
    machine_id TEXT NOT NULL REFERENCES machines(machine_id),
    ordinal INTEGER NOT NULL CHECK(ordinal>0),
    state TEXT NOT NULL CHECK(state IN ('pending','active','completed','failed','unknown','skipped')),
    remote_ref_id TEXT REFERENCES remote_session_refs(ref_id),
    result_sha256 TEXT,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(operation_id,machine_id),
    UNIQUE(operation_id,ordinal),
    UNIQUE(remote_ref_id),
    CHECK(result_sha256 IS NULL OR length(result_sha256)=64)
);
CREATE INDEX fleet_operation_members_by_state
    ON fleet_operation_members(operation_id,state,ordinal);

PRAGMA user_version=6;
