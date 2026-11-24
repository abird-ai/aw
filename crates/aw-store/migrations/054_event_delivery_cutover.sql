-- DL-200 Phase 2 final Event cutover: legacy Job delivery compatibility.
-- Event source ownership is already in event_sources_current; these tables only
-- retain the narrow compatibility semantics required to drain pre-cutover work.

CREATE TABLE event_legacy_subscription_compat(
    recipient_ref TEXT NOT NULL,
    subject TEXT NOT NULL,
    source_ref TEXT NOT NULL CHECK(length(source_ref) BETWEEN 1 AND 2048),
    event_name TEXT NOT NULL CHECK(event_name IN ('result','settled','attention')),
    legacy_generation INTEGER NOT NULL CHECK(legacy_generation>=1),
    mode TEXT NOT NULL CHECK(mode IN ('auto','queue','steer','interrupt')),
    cutover_rev INTEGER NOT NULL CHECK(cutover_rev>=1),
    shadow_existing INTEGER NOT NULL CHECK(shadow_existing IN (0,1)),
    legacy_created_by TEXT NOT NULL CHECK(length(legacy_created_by) BETWEEN 1 AND 2048),
    legacy_authority_ref TEXT NOT NULL CHECK(length(legacy_authority_ref) BETWEEN 1 AND 256),
    created_at INTEGER NOT NULL,
    PRIMARY KEY(recipient_ref,subject),
    FOREIGN KEY(recipient_ref,subject)
        REFERENCES event_subscriptions_current(recipient_ref,subject)
        ON DELETE CASCADE
);

CREATE TABLE event_legacy_input_compat(
    delivery_id TEXT PRIMARY KEY
        REFERENCES agent_event_inputs(delivery_id) ON DELETE RESTRICT,
    source_ref TEXT NOT NULL CHECK(length(source_ref) BETWEEN 1 AND 2048),
    event_rev INTEGER NOT NULL CHECK(event_rev>=1),
    recipient_ref TEXT NOT NULL CHECK(length(recipient_ref) BETWEEN 1 AND 2048),
    legacy_generation INTEGER NOT NULL CHECK(legacy_generation>=1),
    mode TEXT NOT NULL CHECK(mode IN ('auto','queue','steer','interrupt')),
    created_at INTEGER NOT NULL,
    UNIQUE(source_ref,event_rev,recipient_ref,legacy_generation,mode)
);

CREATE TABLE legacy_event_input_compat(
    input_request_id TEXT PRIMARY KEY CHECK(length(input_request_id) BETWEEN 1 AND 128),
    queue_request_id TEXT NOT NULL UNIQUE CHECK(length(queue_request_id) BETWEEN 1 AND 128),
    session_id TEXT NOT NULL,
    branch_id TEXT NOT NULL,
    turn_id INTEGER NOT NULL CHECK(turn_id>=1),
    source_agent_ref TEXT CHECK(source_agent_ref IS NULL OR length(source_agent_ref) BETWEEN 1 AND 2048),
    manifest_digest TEXT NOT NULL
        REFERENCES disclosure_manifest_nodes(node_digest) ON DELETE RESTRICT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(session_id,branch_id)
        REFERENCES agent_state_current(session_id,branch_id) ON DELETE RESTRICT
);

CREATE TABLE legacy_event_input_compat_audience(
    input_request_id TEXT NOT NULL
        REFERENCES legacy_event_input_compat(input_request_id) ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL CHECK(ordinal>=0),
    domain_handle TEXT NOT NULL CHECK(length(domain_handle) BETWEEN 1 AND 256),
    principal_kind TEXT NOT NULL CHECK(principal_kind IN ('user','group')),
    principal_handle TEXT NOT NULL CHECK(length(principal_handle) BETWEEN 1 AND 256),
    PRIMARY KEY(input_request_id,ordinal),
    UNIQUE(input_request_id,domain_handle,principal_kind,principal_handle)
);

CREATE INDEX legacy_event_input_compat_turn
    ON legacy_event_input_compat(session_id,branch_id,turn_id,input_request_id);
