-- DL-200 Phase 2: protected EventInput disclosure closure + causal Turn binding.

ALTER TABLE agent_event_inputs
    ADD COLUMN disclosure_manifest TEXT
    REFERENCES disclosure_manifest_nodes(node_digest) ON DELETE RESTRICT
    CHECK(disclosure_manifest IS NULL OR length(disclosure_manifest)=64);

CREATE TRIGGER agent_event_inputs_immutable_update
BEFORE UPDATE ON agent_event_inputs
BEGIN SELECT RAISE(ABORT,'EventInputs are immutable'); END;

CREATE TRIGGER agent_event_inputs_immutable_delete
BEFORE DELETE ON agent_event_inputs
BEGIN SELECT RAISE(ABORT,'EventInputs are immutable'); END;

CREATE TABLE agent_event_input_audience(
    delivery_id TEXT NOT NULL
        REFERENCES agent_event_inputs(delivery_id) ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL CHECK(ordinal>=0),
    principal_kind TEXT NOT NULL CHECK(principal_kind IN ('user','group')),
    principal_handle TEXT NOT NULL CHECK(length(principal_handle) BETWEEN 1 AND 256),
    PRIMARY KEY(delivery_id,ordinal),
    UNIQUE(delivery_id,principal_kind,principal_handle)
);

CREATE TRIGGER agent_event_input_audience_immutable_update
BEFORE UPDATE ON agent_event_input_audience
BEGIN SELECT RAISE(ABORT,'EventInput audience is immutable'); END;

CREATE TRIGGER agent_event_input_audience_immutable_delete
BEFORE DELETE ON agent_event_input_audience
BEGIN SELECT RAISE(ABORT,'EventInput audience is immutable'); END;

ALTER TABLE runs
    ADD COLUMN event_input_delivery_id TEXT
    REFERENCES agent_event_inputs(delivery_id) ON DELETE RESTRICT;
ALTER TABLE runs
    ADD COLUMN event_causal_root TEXT
    CHECK(event_causal_root IS NULL OR length(event_causal_root) BETWEEN 1 AND 256);
ALTER TABLE runs
    ADD COLUMN event_causal_parent_hop INTEGER
    CHECK(event_causal_parent_hop IS NULL OR event_causal_parent_hop BETWEEN 0 AND 16);

CREATE UNIQUE INDEX runs_event_input_delivery
    ON runs(event_input_delivery_id)
    WHERE event_input_delivery_id IS NOT NULL;

CREATE TABLE event_causal_attention(
    root_id TEXT PRIMARY KEY
        REFERENCES event_causal_roots_current(root_id) ON DELETE RESTRICT,
    reason TEXT NOT NULL CHECK(reason IN ('delivery_limit','hop_limit')),
    created_at INTEGER NOT NULL
);

CREATE TRIGGER event_causal_attention_immutable_update
BEFORE UPDATE ON event_causal_attention
BEGIN SELECT RAISE(ABORT,'causal attention facts are immutable'); END;

CREATE TRIGGER event_causal_attention_immutable_delete
BEFORE DELETE ON event_causal_attention
BEGIN SELECT RAISE(ABORT,'causal attention facts are immutable'); END;
