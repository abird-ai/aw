-- DL-200 Phase 2: rebuildable Event subject projection over immutable commit_log.
-- Event truth remains in commit_log; these rows own no resource/Event lifecycle.

CREATE TABLE event_projection_state(
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    store_id TEXT NOT NULL,
    restore_generation INTEGER NOT NULL CHECK(restore_generation>=1),
    generation INTEGER NOT NULL CHECK(generation>=1),
    indexed_through INTEGER NOT NULL CHECK(indexed_through>=0),
    dirty INTEGER NOT NULL CHECK(dirty IN (0,1))
);

CREATE TABLE event_projection_index(
    commit_position INTEGER PRIMARY KEY,
    event_id TEXT NOT NULL UNIQUE,
    subject TEXT NOT NULL CHECK(length(subject) BETWEEN 1 AND 2048),
    source_ref TEXT NOT NULL CHECK(length(source_ref) BETWEEN 1 AND 2048),
    source_kind TEXT NOT NULL CHECK(
        source_kind IN ('domain','principal','project','sandbox','agent','turn','job','node')
    ),
    domain_handle TEXT NOT NULL,
    identity_handle TEXT,
    project_handle TEXT,
    sandbox_handle TEXT,
    agent_handle TEXT,
    turn_handle TEXT,
    job_handle TEXT,
    node_coordinate INTEGER,
    event_name TEXT NOT NULL CHECK(length(event_name) BETWEEN 1 AND 64),
    event_rev INTEGER NOT NULL CHECK(event_rev>=1),
    FOREIGN KEY(commit_position) REFERENCES commit_log(seq) ON DELETE RESTRICT,
    FOREIGN KEY(event_id) REFERENCES commit_log(event_id) ON DELETE RESTRICT
);

CREATE INDEX event_projection_by_subject
    ON event_projection_index(subject,commit_position);
CREATE INDEX event_projection_by_domain
    ON event_projection_index(domain_handle,commit_position);
CREATE INDEX event_projection_by_source
    ON event_projection_index(source_ref,event_rev,commit_position);

CREATE TRIGGER event_projection_insert_dirty
AFTER INSERT ON event_projection_index BEGIN
    UPDATE event_projection_state SET dirty=1 WHERE singleton=1;
END;

CREATE TRIGGER event_projection_update_dirty
AFTER UPDATE ON event_projection_index BEGIN
    UPDATE event_projection_state SET dirty=1 WHERE singleton=1;
END;

CREATE TRIGGER event_projection_delete_dirty
AFTER DELETE ON event_projection_index BEGIN
    UPDATE event_projection_state SET dirty=1 WHERE singleton=1;
END;
