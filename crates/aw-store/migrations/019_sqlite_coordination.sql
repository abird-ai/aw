-- Phase 34: local SQLite process/store-generation coordination.
-- Production owner mode still acquires owner.lock before opening SQLite. These
-- durable records are the proof/fencing layer required before any trusted
-- opt-in multi-process store mode can be considered.

CREATE TABLE local_process_incarnations (
    process_id TEXT PRIMARY KEY,
    store_id TEXT NOT NULL,
    store_generation INTEGER NOT NULL CHECK(store_generation>0),
    process_nonce TEXT NOT NULL UNIQUE,
    process_kind TEXT NOT NULL CHECK(process_kind IN ('owner_lock','trusted_local')),
    os_pid INTEGER NOT NULL CHECK(os_pid>0),
    state TEXT NOT NULL CHECK(state IN ('active','stopped','fenced')),
    started_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    lease_expires_at INTEGER,
    stopped_at INTEGER,
    CHECK((process_kind='owner_lock' AND lease_expires_at IS NULL) OR
          (process_kind='trusted_local' AND lease_expires_at IS NOT NULL))
);
CREATE INDEX local_process_incarnations_active
    ON local_process_incarnations(store_generation,state,lease_expires_at,process_id);

CREATE TABLE local_coordination_leases (
    scope_kind TEXT NOT NULL CHECK(scope_kind IN ('write','execution')),
    scope_key TEXT NOT NULL CHECK(length(scope_key) BETWEEN 1 AND 512),
    holder_process_id TEXT NOT NULL REFERENCES local_process_incarnations(process_id),
    store_generation INTEGER NOT NULL CHECK(store_generation>0),
    generation INTEGER NOT NULL CHECK(generation>0),
    state TEXT NOT NULL CHECK(state IN ('active','released','hazard')),
    run_id TEXT,
    expires_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(scope_kind,scope_key)
);
CREATE INDEX local_coordination_leases_holder
    ON local_coordination_leases(holder_process_id,state,scope_kind);

CREATE TABLE local_maintenance_lease (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    holder_process_id TEXT REFERENCES local_process_incarnations(process_id),
    store_generation INTEGER,
    generation INTEGER NOT NULL DEFAULT 0 CHECK(generation>=0),
    purpose TEXT CHECK(purpose IN ('migration','backup','restore','maintenance')),
    state TEXT NOT NULL CHECK(state IN ('idle','active')),
    expires_at INTEGER,
    updated_at INTEGER NOT NULL,
    CHECK((state='idle' AND holder_process_id IS NULL AND store_generation IS NULL
                         AND purpose IS NULL AND expires_at IS NULL)
       OR (state='active' AND holder_process_id IS NOT NULL
                           AND store_generation IS NOT NULL
                           AND purpose IS NOT NULL AND expires_at IS NOT NULL))
);
INSERT INTO local_maintenance_lease(singleton,generation,state,updated_at)
VALUES(1,0,'idle',0);

CREATE TABLE local_coordination_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL UNIQUE,
    store_id TEXT NOT NULL,
    store_generation INTEGER NOT NULL CHECK(store_generation>0),
    process_id TEXT,
    kind TEXT NOT NULL,
    scope_kind TEXT,
    scope_key TEXT,
    lease_generation INTEGER,
    payload_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(payload_json) AND length(payload_json)<=16384),
    created_at INTEGER NOT NULL
);
CREATE TRIGGER local_coordination_events_immutable_update
BEFORE UPDATE ON local_coordination_events
BEGIN SELECT RAISE(ABORT,'local coordination events immutable'); END;
CREATE TRIGGER local_coordination_events_immutable_delete
BEFORE DELETE ON local_coordination_events
BEGIN SELECT RAISE(ABORT,'local coordination events immutable'); END;

INSERT INTO store_meta(key,value) VALUES('coordination_schema','1')
ON CONFLICT(key) DO UPDATE SET value=excluded.value;
UPDATE store_meta SET value='19' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=19,activation_epoch=19,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=19,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;

PRAGMA user_version=19;
