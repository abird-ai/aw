-- Agent-owned references only. Providers retain all credential custody.
CREATE TABLE agent_vault_grants_current(
    session_id TEXT NOT NULL,
    branch_id TEXT NOT NULL,
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 64),
    grant_rev INTEGER NOT NULL CHECK(grant_rev>=1),
    provider_id TEXT NOT NULL CHECK(json_valid(provider_id) AND length(provider_id)<=512),
    entry_ref TEXT NOT NULL CHECK(length(entry_ref) BETWEEN 1 AND 512),
    projection TEXT NOT NULL CHECK(projection IN ('broker','socket','file','env')),
    scope TEXT NOT NULL CHECK(length(scope) BETWEEN 1 AND 512),
    authority_ref TEXT NOT NULL CHECK(length(authority_ref) BETWEEN 1 AND 512),
    authority_generation INTEGER NOT NULL CHECK(authority_generation>=1),
    actor_principal TEXT NOT NULL CHECK(length(actor_principal) BETWEEN 1 AND 128),
    actor_user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE RESTRICT,
    access_generation BLOB NOT NULL CHECK(length(access_generation)=32),
    store_id TEXT NOT NULL,
    restore_generation INTEGER NOT NULL CHECK(restore_generation>=1),
    PRIMARY KEY(session_id,branch_id,name),
    FOREIGN KEY(session_id,branch_id)
        REFERENCES agent_state_current(session_id,branch_id) ON DELETE RESTRICT
) WITHOUT ROWID;
