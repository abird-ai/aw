-- DL-200 Phase 2: durable EventHandler activation + at-least-once receipt owner.

CREATE TABLE event_handler_cursors_current(
    extension_id TEXT NOT NULL CHECK(length(extension_id) BETWEEN 1 AND 48),
    handler_id TEXT NOT NULL CHECK(length(handler_id) BETWEEN 1 AND 48),
    handler_schema_version INTEGER NOT NULL CHECK(handler_schema_version>=1),
    registration_revision INTEGER NOT NULL CHECK(registration_revision>=1),
    pattern TEXT NOT NULL CHECK(length(pattern) BETWEEN 1 AND 2048),
    selector_sha256 BLOB NOT NULL CHECK(length(selector_sha256)=32),
    activation_generation INTEGER NOT NULL CHECK(activation_generation>=1),
    state TEXT NOT NULL CHECK(state IN ('active','suspended','lagged')),
    service_principal TEXT NOT NULL CHECK(length(service_principal) BETWEEN 1 AND 2048),
    authority_ref TEXT NOT NULL CHECK(length(authority_ref) BETWEEN 1 AND 256),
    authority_generation INTEGER NOT NULL CHECK(authority_generation>=1),
    store_id TEXT NOT NULL,
    restore_generation INTEGER NOT NULL CHECK(restore_generation>=1),
    placement_kind TEXT NOT NULL CHECK(placement_kind IN ('agent','project')),
    placement_ref TEXT NOT NULL CHECK(length(placement_ref) BETWEEN 1 AND 2048),
    placement_token BLOB NOT NULL CHECK(length(placement_token)=32),
    baseline_position INTEGER NOT NULL CHECK(baseline_position>=0),
    scan_position INTEGER NOT NULL CHECK(scan_position>=0),
    last_acked_receipt_id TEXT CHECK(last_acked_receipt_id IS NULL OR length(last_acked_receipt_id)=68),
    lag_reason TEXT CHECK(lag_reason IS NULL OR length(lag_reason)<=256),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(extension_id,handler_id)
);

CREATE INDEX event_handler_cursors_state_scan
    ON event_handler_cursors_current(state,scan_position,extension_id,handler_id);

CREATE TABLE event_handler_receipts_current(
    extension_id TEXT NOT NULL,
    handler_id TEXT NOT NULL,
    activation_generation INTEGER NOT NULL CHECK(activation_generation>=1),
    receipt_id TEXT NOT NULL UNIQUE CHECK(length(receipt_id)=68),
    event_id TEXT NOT NULL REFERENCES commit_log(event_id) ON DELETE RESTRICT,
    event_position INTEGER NOT NULL CHECK(event_position>=1),
    created_at INTEGER NOT NULL,
    PRIMARY KEY(extension_id,handler_id),
    FOREIGN KEY(extension_id,handler_id)
        REFERENCES event_handler_cursors_current(extension_id,handler_id)
        ON DELETE CASCADE
);
