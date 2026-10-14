-- Run with foreign_keys OFF before BEGIN IMMEDIATE; foreign_key_check before
-- COMMIT, then restore foreign_keys ON even on error. Rebuild, do not edit
-- sqlite_schema or reinterpret legacy checkpoint payloads.
CREATE TABLE nodes_next (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    parent_id TEXT,
    kind TEXT NOT NULL CHECK(kind IN ('root','prompt','turn','checkpoint','context_import','context_compaction')),
    actor TEXT NOT NULL,
    payload TEXT NOT NULL CHECK(json_valid(payload)),
    created_at INTEGER NOT NULL,
    UNIQUE(session_id,id),
    CHECK((kind='root' AND parent_id IS NULL) OR (kind<>'root' AND parent_id IS NOT NULL)),
    FOREIGN KEY(session_id,parent_id) REFERENCES nodes(session_id,id)
);
INSERT INTO nodes_next SELECT seq,id,session_id,parent_id,kind,actor,payload,created_at FROM nodes;
DROP TABLE nodes;
ALTER TABLE nodes_next RENAME TO nodes;
CREATE UNIQUE INDEX one_root_per_session ON nodes(session_id) WHERE kind='root';
CREATE INDEX nodes_by_session ON nodes(session_id,seq);
CREATE TRIGGER node_parent_first BEFORE INSERT ON nodes WHEN NEW.parent_id IS NOT NULL BEGIN
    SELECT CASE WHEN NOT EXISTS(SELECT 1 FROM nodes WHERE id=NEW.parent_id AND session_id=NEW.session_id)
        THEN RAISE(ABORT,'missing or cross-session parent') END;
    SELECT CASE WHEN NEW.kind='turn' AND (SELECT kind FROM nodes WHERE id=NEW.parent_id)<>'prompt'
        THEN RAISE(ABORT,'turn must finish a prompt') END;
    SELECT CASE WHEN NEW.kind IN ('prompt','checkpoint','context_import','context_compaction')
        AND (SELECT kind FROM nodes WHERE id=NEW.parent_id)='prompt'
        THEN RAISE(ABORT,'cannot append inside an unresolved prompt') END;
END;
CREATE TRIGGER nodes_immutable_update BEFORE UPDATE ON nodes BEGIN SELECT RAISE(ABORT,'nodes are immutable'); END;
CREATE TRIGGER nodes_immutable_delete BEFORE DELETE ON nodes BEGIN SELECT RAISE(ABORT,'nodes are immutable'); END;
CREATE TABLE context_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
INSERT INTO context_meta VALUES('authority','a_' || lower(hex(randomblob(16))));
CREATE TABLE context_plans (
    id TEXT PRIMARY KEY, actor TEXT NOT NULL,
    target_session TEXT NOT NULL REFERENCES sessions(id),
    body TEXT NOT NULL CHECK(json_valid(body)),
    digest TEXT NOT NULL CHECK(length(digest)=64), created_at INTEGER NOT NULL
);
CREATE TRIGGER context_plans_immutable_update BEFORE UPDATE ON context_plans BEGIN SELECT RAISE(ABORT,'plans are immutable'); END;
CREATE TRIGGER context_plans_immutable_delete BEFORE DELETE ON context_plans BEGIN SELECT RAISE(ABORT,'plans are immutable'); END;
CREATE TABLE context_operations (
    id TEXT PRIMARY KEY, actor TEXT NOT NULL, request_id TEXT NOT NULL,
    plan_id TEXT NOT NULL REFERENCES context_plans(id), fingerprint TEXT NOT NULL,
    turn_id INTEGER NOT NULL, identity_json TEXT CHECK(identity_json IS NULL OR json_valid(identity_json)),
    state TEXT NOT NULL CHECK(state IN ('started','ready','failed','cancelled','incomplete','interrupted')),
    metadata_json TEXT CHECK(metadata_json IS NULL OR json_valid(metadata_json)),
    artifact_id TEXT REFERENCES context_artifacts(id), error TEXT, created_at INTEGER NOT NULL, settled_at INTEGER,
    UNIQUE(actor,request_id),
    CHECK((state='ready' AND artifact_id IS NOT NULL) OR state<>'ready')
);
CREATE TABLE context_artifacts (
    id TEXT PRIMARY KEY, owner TEXT NOT NULL,
    plan_id TEXT REFERENCES context_plans(id),
    operation_id TEXT REFERENCES context_operations(id),
    body TEXT NOT NULL CHECK(json_valid(body)), created_at INTEGER NOT NULL
);
CREATE TRIGGER context_artifacts_immutable_update BEFORE UPDATE ON context_artifacts BEGIN SELECT RAISE(ABORT,'artifacts are immutable'); END;
CREATE TRIGGER context_artifacts_immutable_delete BEFORE DELETE ON context_artifacts BEGIN SELECT RAISE(ABORT,'artifacts are immutable'); END;
CREATE TABLE context_artifact_sources (
    artifact_id TEXT NOT NULL REFERENCES context_artifacts(id),
    session_id TEXT NOT NULL, node_id TEXT NOT NULL, digest TEXT NOT NULL,
    PRIMARY KEY(artifact_id,session_id,node_id),
    FOREIGN KEY(session_id,node_id) REFERENCES nodes(session_id,id)
);
CREATE TRIGGER context_sources_immutable_update BEFORE UPDATE ON context_artifact_sources BEGIN SELECT RAISE(ABORT,'sources are immutable'); END;
CREATE TRIGGER context_sources_immutable_delete BEFORE DELETE ON context_artifact_sources BEGIN SELECT RAISE(ABORT,'sources are immutable'); END;
-- Materialized conservative transitive dependency closure; never discarded by
-- compaction/checkpoint. Each child inherits its parent's dependencies.
CREATE TABLE node_dependencies (
    node_id TEXT NOT NULL REFERENCES nodes(id),
    source_session TEXT NOT NULL, source_node TEXT NOT NULL,
    PRIMARY KEY(node_id,source_session,source_node),
    FOREIGN KEY(source_session,source_node) REFERENCES nodes(session_id,id)
);
CREATE TRIGGER node_dependencies_immutable_update BEFORE UPDATE ON node_dependencies BEGIN SELECT RAISE(ABORT,'dependencies are immutable'); END;
CREATE TRIGGER node_dependencies_immutable_delete BEFORE DELETE ON node_dependencies BEGIN SELECT RAISE(ABORT,'dependencies are immutable'); END;
CREATE TRIGGER inherit_node_dependencies AFTER INSERT ON nodes WHEN NEW.parent_id IS NOT NULL BEGIN
    INSERT OR IGNORE INTO node_dependencies(node_id,source_session,source_node)
        SELECT NEW.id,source_session,source_node FROM node_dependencies WHERE node_id=NEW.parent_id;
END;
CREATE TABLE context_applications (
    actor TEXT NOT NULL, request_id TEXT NOT NULL, fingerprint TEXT NOT NULL,
    artifact_id TEXT NOT NULL REFERENCES context_artifacts(id),
    result_json TEXT NOT NULL CHECK(json_valid(result_json)), created_at INTEGER NOT NULL,
    PRIMARY KEY(actor,request_id)
);
CREATE TABLE context_pending (
    id TEXT PRIMARY KEY, actor TEXT NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id), branch_id TEXT NOT NULL REFERENCES branches(id),
    head TEXT NOT NULL REFERENCES nodes(id), revision INTEGER NOT NULL,
    artifact_id TEXT NOT NULL REFERENCES context_artifacts(id),
    state TEXT NOT NULL CHECK(state IN ('pending','consumed','discarded')),
    created_at INTEGER NOT NULL
);
CREATE INDEX pending_by_branch ON context_pending(session_id,branch_id,actor,state);
-- External released copies may be used only in the import owner's private
-- audience. Their old source IDs are provenance, never local access grants.
CREATE TABLE context_private_sessions (session_id TEXT PRIMARY KEY REFERENCES sessions(id));
PRAGMA user_version=2;
-- Reading shared history alone does not grant permission to summarize it at
-- an arbitrary recipient account. These are owner-managed explicit releases.
CREATE TABLE context_source_grants (
    session_id TEXT NOT NULL REFERENCES sessions(id), principal TEXT NOT NULL,
    accounts_json TEXT NOT NULL CHECK(json_valid(accounts_json)), grant_id TEXT NOT NULL,
    PRIMARY KEY(session_id,principal)
);
CREATE TABLE context_derivations (
    node_id TEXT NOT NULL REFERENCES nodes(id),
    source_session TEXT NOT NULL REFERENCES sessions(id), requester TEXT NOT NULL, grant_id TEXT NOT NULL,
    PRIMARY KEY(node_id,source_session,requester,grant_id)
);
CREATE TRIGGER context_derivations_immutable_update BEFORE UPDATE ON context_derivations BEGIN SELECT RAISE(ABORT,'derivations immutable'); END;
CREATE TRIGGER context_derivations_immutable_delete BEFORE DELETE ON context_derivations BEGIN SELECT RAISE(ABORT,'derivations immutable'); END;
CREATE TRIGGER inherit_context_derivations AFTER INSERT ON nodes WHEN NEW.parent_id IS NOT NULL BEGIN
    INSERT OR IGNORE INTO context_derivations(node_id,source_session,requester,grant_id)
    SELECT NEW.id,source_session,requester,grant_id FROM context_derivations WHERE node_id=NEW.parent_id;
END;
CREATE TABLE context_prompt_attachments (
    prompt_node TEXT NOT NULL REFERENCES nodes(id), artifact_id TEXT NOT NULL REFERENCES context_artifacts(id),
    PRIMARY KEY(prompt_node,artifact_id)
);
CREATE TRIGGER prompt_context_immutable_update BEFORE UPDATE ON context_prompt_attachments BEGIN SELECT RAISE(ABORT,'prompt context immutable'); END;
CREATE TRIGGER prompt_context_immutable_delete BEFORE DELETE ON context_prompt_attachments BEGIN SELECT RAISE(ABORT,'prompt context immutable'); END;

CREATE TRIGGER context_applications_immutable_update BEFORE UPDATE ON context_applications BEGIN SELECT RAISE(ABORT,'application receipts immutable'); END;
CREATE TRIGGER context_applications_immutable_delete BEFORE DELETE ON context_applications BEGIN SELECT RAISE(ABORT,'application receipts immutable'); END;
CREATE TRIGGER context_operation_identity_immutable BEFORE UPDATE ON context_operations
WHEN NEW.id<>OLD.id OR NEW.actor<>OLD.actor OR NEW.request_id<>OLD.request_id OR NEW.plan_id<>OLD.plan_id
 OR NEW.fingerprint<>OLD.fingerprint OR NEW.turn_id<>OLD.turn_id OR NEW.identity_json IS NOT OLD.identity_json
BEGIN SELECT RAISE(ABORT,'operation identity immutable'); END;
CREATE TRIGGER context_operation_terminal_immutable BEFORE UPDATE ON context_operations
WHEN OLD.state<>'started' BEGIN SELECT RAISE(ABORT,'settled operation immutable'); END;
