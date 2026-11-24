-- DL-200 Phase 2: allow authority-scoped same-store Event universes.
-- Existing Agent/Project rows and all private progress remain byte-for-byte values.

CREATE TABLE event_read_cursors_next(
    cursor_id TEXT PRIMARY KEY CHECK(length(cursor_id) BETWEEN 1 AND 96),
    principal TEXT NOT NULL CHECK(length(principal) BETWEEN 1 AND 2048),
    selector_sha256 BLOB NOT NULL CHECK(length(selector_sha256)=32),
    placement_ref TEXT NOT NULL CHECK(length(placement_ref) BETWEEN 1 AND 2048),
    placement_token BLOB NOT NULL CHECK(length(placement_token)=32),
    store_id TEXT NOT NULL CHECK(length(store_id)>=1),
    restore_generation INTEGER NOT NULL CHECK(restore_generation>=1),
    position INTEGER NOT NULL CHECK(position>=0),
    created_at INTEGER NOT NULL,
    last_used_at INTEGER NOT NULL,
    placement_kind TEXT NOT NULL CHECK(placement_kind IN ('agent','project','universe'))
);
INSERT INTO event_read_cursors_next
SELECT cursor_id,principal,selector_sha256,placement_ref,placement_token,
       store_id,restore_generation,position,created_at,last_used_at,placement_kind
FROM event_read_cursors_current;
DROP INDEX event_read_cursors_principal_used;
DROP TABLE event_read_cursors_current;
ALTER TABLE event_read_cursors_next RENAME TO event_read_cursors_current;
CREATE INDEX event_read_cursors_principal_used
    ON event_read_cursors_current(principal,last_used_at,cursor_id);

CREATE TABLE event_subscriptions_next(
    recipient_ref TEXT NOT NULL CHECK(length(recipient_ref) BETWEEN 1 AND 2048),
    subject TEXT NOT NULL CHECK(length(subject) BETWEEN 1 AND 2048),
    placement_kind TEXT NOT NULL CHECK(placement_kind IN ('agent','project','universe')),
    placement_ref TEXT NOT NULL CHECK(length(placement_ref) BETWEEN 1 AND 2048),
    placement_token BLOB NOT NULL CHECK(length(placement_token)=32),
    store_id TEXT NOT NULL CHECK(length(store_id)>=1),
    restore_generation INTEGER NOT NULL CHECK(restore_generation>=1),
    generation INTEGER NOT NULL CHECK(generation>=1),
    baseline_position INTEGER NOT NULL CHECK(baseline_position>=0),
    scan_position INTEGER NOT NULL CHECK(scan_position>=baseline_position),
    created_by TEXT NOT NULL CHECK(length(created_by) BETWEEN 1 AND 2048),
    authority_ref TEXT NOT NULL CHECK(length(authority_ref) BETWEEN 1 AND 256),
    recipient_permission TEXT NOT NULL CHECK(recipient_permission IN ('control','manage')),
    recipient_access_generation BLOB NOT NULL CHECK(length(recipient_access_generation)=32),
    recipient_grant_revision INTEGER NOT NULL CHECK(recipient_grant_revision>=1),
    recipient_acl_revision INTEGER NOT NULL CHECK(recipient_acl_revision>=1),
    recipient_is_owner INTEGER NOT NULL CHECK(recipient_is_owner IN (0,1)),
    state TEXT NOT NULL CHECK(state IN ('active','lagged','disabled')),
    lag_reason TEXT CHECK(lag_reason IS NULL OR length(lag_reason) BETWEEN 1 AND 256),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(recipient_ref,subject),
    CHECK((state='lagged' AND lag_reason IS NOT NULL)
       OR (state<>'lagged' AND lag_reason IS NULL))
);
INSERT INTO event_subscriptions_next
SELECT recipient_ref,subject,placement_kind,placement_ref,placement_token,
       store_id,restore_generation,generation,baseline_position,scan_position,
       created_by,authority_ref,recipient_permission,recipient_access_generation,
       recipient_grant_revision,recipient_acl_revision,recipient_is_owner,
       state,lag_reason,created_at,updated_at
FROM event_subscriptions_current;
DROP INDEX event_subscriptions_recipient_state;
DROP TABLE event_subscriptions_current;
ALTER TABLE event_subscriptions_next RENAME TO event_subscriptions_current;
CREATE INDEX event_subscriptions_recipient_state
    ON event_subscriptions_current(recipient_ref,state,subject);

CREATE TABLE event_handler_cursors_next(
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
    placement_kind TEXT NOT NULL CHECK(placement_kind IN ('agent','project','universe')),
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
INSERT INTO event_handler_cursors_next
SELECT extension_id,handler_id,handler_schema_version,registration_revision,
       pattern,selector_sha256,activation_generation,state,service_principal,
       authority_ref,authority_generation,store_id,restore_generation,
       placement_kind,placement_ref,placement_token,baseline_position,
       scan_position,last_acked_receipt_id,lag_reason,created_at,updated_at
FROM event_handler_cursors_current;

CREATE TABLE event_handler_receipts_next(
    extension_id TEXT NOT NULL,
    handler_id TEXT NOT NULL,
    activation_generation INTEGER NOT NULL CHECK(activation_generation>=1),
    receipt_id TEXT NOT NULL UNIQUE CHECK(length(receipt_id)=68),
    event_id TEXT NOT NULL REFERENCES commit_log(event_id) ON DELETE RESTRICT,
    event_position INTEGER NOT NULL CHECK(event_position>=1),
    created_at INTEGER NOT NULL,
    PRIMARY KEY(extension_id,handler_id),
    FOREIGN KEY(extension_id,handler_id)
        REFERENCES event_handler_cursors_next(extension_id,handler_id)
        ON DELETE CASCADE
);
INSERT INTO event_handler_receipts_next
SELECT extension_id,handler_id,activation_generation,receipt_id,
       event_id,event_position,created_at
FROM event_handler_receipts_current;

DROP TABLE event_handler_receipts_current;
DROP INDEX event_handler_cursors_state_scan;
DROP TABLE event_handler_cursors_current;
ALTER TABLE event_handler_cursors_next RENAME TO event_handler_cursors_current;
ALTER TABLE event_handler_receipts_next RENAME TO event_handler_receipts_current;
CREATE INDEX event_handler_cursors_state_scan
    ON event_handler_cursors_current(state,scan_position,extension_id,handler_id);
