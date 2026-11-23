-- DL-200 Phase 2: typed immutable EventInput + replay-safe causal budget.
-- Accepted EventInputs are Agent input facts, not a second pending Event queue.

CREATE TABLE agent_event_inputs(
    delivery_id TEXT PRIMARY KEY CHECK(length(delivery_id)=68),
    recipient_session_id TEXT NOT NULL,
    recipient_branch_id TEXT NOT NULL,
    recipient_ref TEXT NOT NULL CHECK(length(recipient_ref) BETWEEN 1 AND 2048),
    subscription_subject TEXT NOT NULL CHECK(length(subscription_subject) BETWEEN 1 AND 2048),
    subscription_generation INTEGER NOT NULL CHECK(subscription_generation>=1),
    event_id TEXT NOT NULL REFERENCES commit_log(event_id) ON DELETE RESTRICT,
    event_position INTEGER NOT NULL CHECK(event_position>=1),
    source_ref TEXT NOT NULL CHECK(length(source_ref) BETWEEN 1 AND 2048),
    source_agent_ref TEXT NOT NULL CHECK(length(source_agent_ref) BETWEEN 1 AND 2048),
    subject TEXT NOT NULL CHECK(length(subject) BETWEEN 1 AND 2048),
    event_rev INTEGER NOT NULL CHECK(event_rev>=1),
    source_actor TEXT,
    source_turn INTEGER CHECK(source_turn IS NULL OR source_turn>=1),
    causal_root TEXT NOT NULL CHECK(length(causal_root) BETWEEN 1 AND 256),
    causal_hop INTEGER NOT NULL CHECK(causal_hop BETWEEN 0 AND 16),
    data_json TEXT CHECK(data_json IS NULL OR (json_valid(data_json) AND length(data_json)<=4096)),
    created_by TEXT NOT NULL CHECK(length(created_by) BETWEEN 1 AND 2048),
    source_access_generation BLOB NOT NULL CHECK(length(source_access_generation)=32),
    recipient_access_generation BLOB NOT NULL CHECK(length(recipient_access_generation)=32),
    created_at INTEGER NOT NULL,
    FOREIGN KEY(recipient_session_id,recipient_branch_id)
        REFERENCES agent_state_current(session_id,branch_id) ON DELETE RESTRICT,
    UNIQUE(recipient_session_id,recipient_branch_id,event_id,subscription_subject,subscription_generation)
);

CREATE INDEX agent_event_inputs_recipient
    ON agent_event_inputs(recipient_session_id,recipient_branch_id,created_at,delivery_id);
CREATE INDEX agent_event_inputs_causal
    ON agent_event_inputs(causal_root,created_at,delivery_id);

CREATE TABLE event_causal_roots_current(
    root_id TEXT PRIMARY KEY CHECK(length(root_id) BETWEEN 1 AND 256),
    max_event_hop INTEGER NOT NULL CHECK(max_event_hop BETWEEN 0 AND 16),
    accepted_event_input_count INTEGER NOT NULL
        CHECK(accepted_event_input_count BETWEEN 0 AND 256),
    state TEXT NOT NULL CHECK(state IN ('active','exhausted')),
    updated_at INTEGER NOT NULL,
    CHECK((accepted_event_input_count<256 AND state='active')
       OR (accepted_event_input_count=256 AND state='exhausted'))
);
