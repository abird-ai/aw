-- Phase 25: explicit local projection of selected remote semantic evidence.
-- Imported inbox rows remain immutable evidence; these tables record a separately
-- authorized local materialization policy and the immutable fact produced by it.

CREATE TABLE semantic_materialization_policies (
    policy_id TEXT PRIMARY KEY,
    federation_id TEXT NOT NULL REFERENCES domain_federations(federation_id),
    local_domain_id TEXT NOT NULL REFERENCES domains(domain_id),
    source_store_id TEXT NOT NULL,
    event_kind TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('active','revoked')),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
    created_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    revoked_at INTEGER
);
CREATE UNIQUE INDEX semantic_materialization_policy_active_selector
    ON semantic_materialization_policies(
        federation_id,source_store_id,event_kind,entity_type,entity_id
    ) WHERE status='active';
CREATE INDEX semantic_materialization_policies_by_domain
    ON semantic_materialization_policies(local_domain_id,status,created_at);

CREATE TABLE semantic_materialized_facts (
    materialization_id TEXT PRIMARY KEY,
    local_domain_id TEXT NOT NULL REFERENCES domains(domain_id),
    federation_id TEXT NOT NULL REFERENCES domain_federations(federation_id),
    source_store_id TEXT NOT NULL,
    source_seq INTEGER NOT NULL CHECK(source_seq>0),
    source_event_id TEXT NOT NULL,
    source_event_sha256 TEXT NOT NULL CHECK(length(source_event_sha256)=64),
    source_kind TEXT NOT NULL,
    source_entity_type TEXT NOT NULL,
    source_entity_id TEXT NOT NULL,
    source_actor TEXT,
    source_payload_json TEXT NOT NULL CHECK(json_valid(source_payload_json)),
    source_created_at INTEGER NOT NULL,
    policy_id TEXT NOT NULL REFERENCES semantic_materialization_policies(policy_id),
    policy_revision INTEGER NOT NULL CHECK(policy_revision>0),
    materialized_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    materialized_at INTEGER NOT NULL,
    UNIQUE(federation_id,source_store_id,source_seq),
    UNIQUE(federation_id,source_store_id,source_event_id)
);
CREATE INDEX semantic_materialized_facts_by_entity
    ON semantic_materialized_facts(local_domain_id,source_entity_type,source_entity_id,materialized_at);

CREATE TRIGGER semantic_materialized_facts_immutable_update
BEFORE UPDATE ON semantic_materialized_facts
BEGIN SELECT RAISE(ABORT,'semantic materialized fact immutable'); END;

CREATE TRIGGER semantic_materialized_facts_immutable_delete
BEFORE DELETE ON semantic_materialized_facts
BEGIN SELECT RAISE(ABORT,'semantic materialized fact immutable'); END;

PRAGMA user_version=10;
