-- Phase 33: explicit projection of already materialized remote semantic facts
-- into an immutable context artifact candidate. Projection/attachment are local
-- operations; no remote fetch, provider call, or generic replication occurs.

CREATE TABLE materialized_context_projections (
    projection_id TEXT PRIMARY KEY,
    actor_user_id TEXT NOT NULL REFERENCES users(user_id),
    request_id TEXT NOT NULL,
    request_sha256 TEXT NOT NULL CHECK(length(request_sha256)=64),
    target_session_id TEXT NOT NULL REFERENCES sessions(id),
    target_branch_id TEXT NOT NULL REFERENCES branches(id),
    target_head TEXT NOT NULL REFERENCES nodes(id),
    target_revision INTEGER NOT NULL CHECK(target_revision>=0),
    artifact_id TEXT NOT NULL UNIQUE REFERENCES context_artifacts(id),
    created_at INTEGER NOT NULL,
    UNIQUE(actor_user_id,request_id)
);

CREATE TABLE context_artifact_materialized_facts (
    artifact_id TEXT NOT NULL REFERENCES context_artifacts(id),
    materialization_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal>=0),
    fact_json TEXT NOT NULL CHECK(json_valid(fact_json)),
    fact_sha256 TEXT NOT NULL CHECK(length(fact_sha256)=64),
    authorization_policy_id TEXT NOT NULL,
    authorization_policy_revision INTEGER NOT NULL CHECK(authorization_policy_revision>0),
    PRIMARY KEY(artifact_id,materialization_id),
    UNIQUE(artifact_id,ordinal)
);

CREATE TRIGGER materialized_context_projections_immutable_update
BEFORE UPDATE ON materialized_context_projections
BEGIN SELECT RAISE(ABORT,'materialized context projections are immutable'); END;
CREATE TRIGGER materialized_context_projections_immutable_delete
BEFORE DELETE ON materialized_context_projections
BEGIN SELECT RAISE(ABORT,'materialized context projections are immutable'); END;
CREATE TRIGGER context_artifact_materialized_facts_immutable_update
BEFORE UPDATE ON context_artifact_materialized_facts
BEGIN SELECT RAISE(ABORT,'materialized context fact links are immutable'); END;
CREATE TRIGGER context_artifact_materialized_facts_immutable_delete
BEFORE DELETE ON context_artifact_materialized_facts
BEGIN SELECT RAISE(ABORT,'materialized context fact links are immutable'); END;

-- Conservative transitive provenance closure. Once a model-visible descendant
-- is derived while a materialized fact is in context, later descendants retain
-- that fact fence even if projection/compaction hides the original block.
CREATE TABLE node_materialized_facts (
    node_id TEXT NOT NULL REFERENCES nodes(id),
    materialization_id TEXT NOT NULL,
    fact_json TEXT NOT NULL CHECK(json_valid(fact_json)),
    fact_sha256 TEXT NOT NULL CHECK(length(fact_sha256)=64),
    authorization_policy_id TEXT NOT NULL,
    authorization_policy_revision INTEGER NOT NULL CHECK(authorization_policy_revision>0),
    PRIMARY KEY(node_id,materialization_id)
);
CREATE TRIGGER node_materialized_facts_immutable_update
BEFORE UPDATE ON node_materialized_facts
BEGIN SELECT RAISE(ABORT,'node materialized fact provenance is immutable'); END;
CREATE TRIGGER node_materialized_facts_immutable_delete
BEFORE DELETE ON node_materialized_facts
BEGIN SELECT RAISE(ABORT,'node materialized fact provenance is immutable'); END;
CREATE TRIGGER inherit_node_materialized_facts AFTER INSERT ON nodes
WHEN NEW.parent_id IS NOT NULL
BEGIN
    INSERT OR IGNORE INTO node_materialized_facts(node_id,materialization_id,fact_json,fact_sha256,authorization_policy_id,authorization_policy_revision)
        SELECT NEW.id,materialization_id,fact_json,fact_sha256,authorization_policy_id,authorization_policy_revision
        FROM node_materialized_facts WHERE node_id=NEW.parent_id;
END;

PRAGMA user_version=18;
