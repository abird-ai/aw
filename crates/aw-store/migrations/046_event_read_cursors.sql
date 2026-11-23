-- DL-200 Phase 2: opaque host/SDK Event read cursors.
-- Private positions never cross the public API boundary.

CREATE TABLE event_read_cursors_current(
    cursor_id TEXT PRIMARY KEY CHECK(length(cursor_id) BETWEEN 1 AND 96),
    principal TEXT NOT NULL CHECK(length(principal) BETWEEN 1 AND 2048),
    selector_sha256 BLOB NOT NULL CHECK(length(selector_sha256)=32),
    agent_ref TEXT NOT NULL CHECK(length(agent_ref) BETWEEN 1 AND 2048),
    placement_token BLOB NOT NULL CHECK(length(placement_token)=32),
    store_id TEXT NOT NULL CHECK(length(store_id)>=1),
    restore_generation INTEGER NOT NULL CHECK(restore_generation>=1),
    position INTEGER NOT NULL CHECK(position>=0),
    created_at INTEGER NOT NULL,
    last_used_at INTEGER NOT NULL
);
CREATE INDEX event_read_cursors_principal_used
    ON event_read_cursors_current(principal,last_used_at,cursor_id);
