-- Independent host grant evidence, not another Sandbox policy or ACL owner.
-- Revoked rows retain the revision so an old admission cannot be revived.
CREATE TABLE agent_sandbox_grants_current(
    session_id TEXT NOT NULL,
    branch_id TEXT NOT NULL,
    sandbox_id TEXT NOT NULL REFERENCES sandboxes_current(sandbox_id) ON DELETE RESTRICT,
    grant_rev INTEGER NOT NULL CHECK(grant_rev>=1),
    state TEXT NOT NULL CHECK(state IN ('active','revoked')),
    policy_ref TEXT NOT NULL CHECK(length(CAST(policy_ref AS BLOB)) BETWEEN 1 AND 512),
    policy_digest BLOB NOT NULL CHECK(length(policy_digest)=32),
    authority_ref TEXT NOT NULL CHECK(length(CAST(authority_ref AS BLOB)) BETWEEN 1 AND 512),
    authority_generation INTEGER NOT NULL CHECK(authority_generation>=1),
    actor_principal TEXT NOT NULL CHECK(length(actor_principal) BETWEEN 1 AND 128),
    actor_user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE RESTRICT,
    access_generation BLOB NOT NULL CHECK(length(access_generation)=32),
    source_pins_json TEXT NOT NULL CHECK(json_valid(source_pins_json)
        AND json_type(source_pins_json)='array'
        AND json_array_length(source_pins_json)<=1024
        AND length(CAST(source_pins_json AS BLOB))<=6291456),
    PRIMARY KEY(session_id,branch_id,sandbox_id),
    FOREIGN KEY(session_id,branch_id)
        REFERENCES agent_state_current(session_id,branch_id) ON DELETE RESTRICT
) WITHOUT ROWID;
