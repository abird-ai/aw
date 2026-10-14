-- Version 1. No credential material, account grants or executable config is stored.
-- Parent rows must already exist: immutable ancestry cannot acquire a cycle.
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    owner TEXT NOT NULL,
    title TEXT NOT NULL CHECK(length(title) BETWEEN 1 AND 160),
    binding_json TEXT NOT NULL CHECK(json_valid(binding_json)),
    root_key TEXT NOT NULL,
    root_node TEXT NOT NULL,
    next_turn INTEGER NOT NULL DEFAULT 1 CHECK(next_turn > 0),
    import_review_needed INTEGER NOT NULL DEFAULT 0 CHECK(import_review_needed IN (0,1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(id, root_node) REFERENCES nodes(session_id, id) DEFERRABLE INITIALLY DEFERRED
);
CREATE TABLE nodes (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    parent_id TEXT,
    kind TEXT NOT NULL CHECK(kind IN ('root','prompt','turn','checkpoint')),
    actor TEXT NOT NULL,
    payload TEXT NOT NULL CHECK(json_valid(payload)),
    created_at INTEGER NOT NULL,
    UNIQUE(session_id,id),
    CHECK((kind='root' AND parent_id IS NULL) OR (kind<>'root' AND parent_id IS NOT NULL)),
    FOREIGN KEY(session_id,parent_id) REFERENCES nodes(session_id,id)
);
CREATE UNIQUE INDEX one_root_per_session ON nodes(session_id) WHERE kind='root';
CREATE INDEX nodes_by_session ON nodes(session_id,seq);
CREATE TRIGGER node_parent_first BEFORE INSERT ON nodes WHEN NEW.parent_id IS NOT NULL BEGIN
    SELECT CASE WHEN NOT EXISTS(SELECT 1 FROM nodes WHERE id=NEW.parent_id AND session_id=NEW.session_id)
        THEN RAISE(ABORT,'missing or cross-session parent') END;
    SELECT CASE WHEN NEW.kind='turn' AND (SELECT kind FROM nodes WHERE id=NEW.parent_id)<>'prompt'
        THEN RAISE(ABORT,'turn must finish a prompt') END;
    SELECT CASE WHEN NEW.kind IN ('prompt','checkpoint') AND (SELECT kind FROM nodes WHERE id=NEW.parent_id)='prompt'
        THEN RAISE(ABORT,'cannot append inside an unresolved prompt') END;
END;
CREATE TRIGGER nodes_immutable_update BEFORE UPDATE ON nodes BEGIN SELECT RAISE(ABORT,'nodes are immutable'); END;
CREATE TRIGGER nodes_immutable_delete BEFORE DELETE ON nodes BEGIN SELECT RAISE(ABORT,'nodes are immutable'); END;
CREATE TABLE members (
    session_id TEXT NOT NULL REFERENCES sessions(id),
    principal TEXT NOT NULL,
    can_append INTEGER NOT NULL CHECK(can_append IN (0,1)),
    PRIMARY KEY(session_id,principal)
);
CREATE TABLE branches (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 80),
    head TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0 CHECK(revision>=0),
    active_run TEXT,
    created_at INTEGER NOT NULL,
    UNIQUE(session_id,name),
    FOREIGN KEY(session_id,head) REFERENCES nodes(session_id,id)
);
CREATE TABLE runs (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    branch_id TEXT NOT NULL REFERENCES branches(id),
    turn_id INTEGER NOT NULL CHECK(turn_id>0),
    prompt_node TEXT NOT NULL REFERENCES nodes(id),
    identity_json TEXT NOT NULL CHECK(json_valid(identity_json)),
    root_key TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('running','completed','failed','cancelled','incomplete','interrupted')),
    possible_effect INTEGER NOT NULL DEFAULT 0 CHECK(possible_effect IN (0,1)),
    final_node TEXT REFERENCES nodes(id),
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    UNIQUE(session_id,turn_id)
);
CREATE UNIQUE INDEX one_active_run_per_root ON runs(root_key) WHERE status='running';
CREATE TABLE run_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL REFERENCES runs(id),
    kind TEXT NOT NULL,
    payload TEXT NOT NULL CHECK(json_valid(payload)),
    created_at INTEGER NOT NULL
);
CREATE INDEX events_by_run ON run_events(run_id,seq);
CREATE TRIGGER events_immutable_update BEFORE UPDATE ON run_events BEGIN SELECT RAISE(ABORT,'events are immutable'); END;
CREATE TRIGGER events_immutable_delete BEFORE DELETE ON run_events BEGIN SELECT RAISE(ABORT,'events are immutable'); END;
CREATE TABLE invocation_receipts (
    run_id TEXT NOT NULL REFERENCES runs(id),
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 1 AND 32),
    identity_json TEXT NOT NULL CHECK(json_valid(identity_json)),
    metadata_json TEXT CHECK(metadata_json IS NULL OR json_valid(metadata_json)),
    state TEXT NOT NULL CHECK(state IN ('started','received','unknown')),
    PRIMARY KEY(run_id,ordinal)
);
CREATE TABLE hazards (
    run_id TEXT PRIMARY KEY REFERENCES runs(id),
    session_id TEXT NOT NULL REFERENCES sessions(id),
    root_key TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('running','needs_inspection','settled')),
    note TEXT,
    updated_at INTEGER NOT NULL
);
CREATE INDEX hazards_by_root ON hazards(root_key,state);
CREATE TABLE audit (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    data_json TEXT NOT NULL CHECK(json_valid(data_json)),
    created_at INTEGER NOT NULL
);
CREATE INDEX audit_by_session ON audit(session_id,seq);
CREATE TRIGGER audit_immutable_update BEFORE UPDATE ON audit BEGIN SELECT RAISE(ABORT,'audit is immutable'); END;
CREATE TRIGGER audit_immutable_delete BEFORE DELETE ON audit BEGIN SELECT RAISE(ABORT,'audit is immutable'); END;
CREATE TABLE web_requests (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    actor TEXT NOT NULL,
    request_id TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    branch_id TEXT NOT NULL REFERENCES branches(id),
    view_revision INTEGER NOT NULL,
    native_run_id TEXT REFERENCES runs(id),
    state TEXT NOT NULL CHECK(state IN ('accepted','settled','interrupted')),
    created_at INTEGER NOT NULL,
    UNIQUE(actor,request_id)
);
PRAGMA user_version=1;
