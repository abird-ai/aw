-- DL-200 Phase 2: typed/custom Event publication and durable model per-Turn bound.

ALTER TABLE agent_event_inputs
    ADD COLUMN event_schema_json TEXT
    CHECK(event_schema_json IS NULL OR json_valid(event_schema_json));

CREATE TABLE event_custom_turn_counts(
    session_id TEXT NOT NULL,
    branch_id TEXT NOT NULL,
    turn_id INTEGER NOT NULL CHECK(turn_id>=1),
    published_count INTEGER NOT NULL CHECK(published_count BETWEEN 1 AND 32),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(session_id,branch_id,turn_id),
    FOREIGN KEY(session_id,branch_id)
        REFERENCES agent_state_current(session_id,branch_id) ON DELETE RESTRICT
);
