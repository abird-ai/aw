-- DL-200 Phase 1: one bounded replay owner for staged Agent input mutations.

CREATE TABLE agent_input_mutation_replay(
    actor_user_id TEXT NOT NULL,
    request_id TEXT NOT NULL,
    operation TEXT NOT NULL CHECK(operation IN ('add','edit','delete','send')),
    fingerprint TEXT NOT NULL CHECK(length(fingerprint)=64),
    session_id TEXT NOT NULL,
    branch_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('prepared','applied','rejected','unknown')),
    resulting_inputs_rev INTEGER,
    result_json TEXT NOT NULL,
    note TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(actor_user_id,request_id),
    FOREIGN KEY(actor_user_id) REFERENCES users(user_id) ON DELETE RESTRICT,
    FOREIGN KEY(session_id,branch_id)
        REFERENCES agent_input_plans(session_id,branch_id) ON DELETE CASCADE
);

CREATE INDEX agent_input_mutation_replay_target
    ON agent_input_mutation_replay(session_id,branch_id,updated_at,request_id);
