-- Phase 23: append-only semantic cross-store event inbox and per-federation cursors.

CREATE TABLE semantic_sync_cursors (
    federation_id TEXT NOT NULL REFERENCES domain_federations(federation_id),
    source_store_id TEXT NOT NULL,
    last_source_seq INTEGER NOT NULL CHECK(last_source_seq>=0),
    last_event_id TEXT,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(federation_id,source_store_id)
);

CREATE TABLE semantic_sync_inbox (
    federation_id TEXT NOT NULL REFERENCES domain_federations(federation_id),
    source_store_id TEXT NOT NULL,
    source_seq INTEGER NOT NULL CHECK(source_seq>0),
    source_event_id TEXT NOT NULL,
    event_sha256 TEXT NOT NULL CHECK(length(event_sha256)=64),
    kind TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    actor TEXT,
    payload_json TEXT NOT NULL CHECK(json_valid(payload_json)),
    source_created_at INTEGER NOT NULL,
    imported_at INTEGER NOT NULL,
    PRIMARY KEY(federation_id,source_store_id,source_seq),
    UNIQUE(federation_id,source_store_id,source_event_id)
);
CREATE INDEX semantic_sync_inbox_by_entity
    ON semantic_sync_inbox(federation_id,entity_type,entity_id,source_seq);

CREATE TRIGGER semantic_sync_inbox_immutable_update
BEFORE UPDATE ON semantic_sync_inbox
BEGIN SELECT RAISE(ABORT,'semantic sync inbox immutable'); END;

CREATE TRIGGER semantic_sync_inbox_immutable_delete
BEFORE DELETE ON semantic_sync_inbox
BEGIN SELECT RAISE(ABORT,'semantic sync inbox immutable'); END;

PRAGMA user_version=8;
