-- DL-200 Phase 1: mutable host-staged Agent input plan.
-- Runtime custody remains in ActorQueue/AuthoritySteer after Send.

CREATE TABLE agent_input_plans(
    session_id TEXT NOT NULL,
    branch_id TEXT NOT NULL,
    inputs_rev INTEGER NOT NULL CHECK(inputs_rev>=1),
    PRIMARY KEY(session_id,branch_id),
    FOREIGN KEY(session_id,branch_id)
        REFERENCES agent_state_current(session_id,branch_id) ON DELETE CASCADE
);

CREATE TABLE agent_input_plan_items(
    session_id TEXT NOT NULL,
    branch_id TEXT NOT NULL,
    input_id TEXT NOT NULL,
    intent_kind TEXT NOT NULL CHECK(intent_kind IN ('followup','steer','interrupt')),
    target_turn INTEGER,
    order_key INTEGER NOT NULL CHECK(order_key>0),
    creator_handle TEXT NOT NULL,
    source_kind TEXT NOT NULL CHECK(source_kind='host'),
    last_editor_handle TEXT NOT NULL,
    payload_ref TEXT NOT NULL,
    payload_sha256 TEXT NOT NULL CHECK(length(payload_sha256)=64),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(session_id,branch_id,input_id),
    UNIQUE(session_id,branch_id,order_key),
    FOREIGN KEY(session_id,branch_id)
        REFERENCES agent_input_plans(session_id,branch_id) ON DELETE CASCADE,
    CHECK(
        (intent_kind='followup' AND (target_turn IS NULL OR target_turn>=1))
        OR
        (intent_kind IN ('steer','interrupt') AND target_turn>=1)
    )
);

CREATE INDEX agent_input_plan_items_order
    ON agent_input_plan_items(session_id,branch_id,order_key,input_id);

INSERT INTO agent_input_plans(session_id,branch_id,inputs_rev)
SELECT session_id,branch_id,1 FROM agent_state_current;
