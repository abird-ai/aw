-- DL-200 Phase 1: authoritative durable Agent envelope and direct ACL owner.
-- Seed/activation is performed by the Rust migration in the same exclusive
-- transaction; this file only creates the durable owner shape.

CREATE TABLE agent_state_current(
    domain_handle TEXT NOT NULL,
    identity_handle TEXT NOT NULL,
    project_handle TEXT NOT NULL,
    agent_handle TEXT NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE RESTRICT,
    branch_id TEXT NOT NULL REFERENCES branches(id) ON DELETE RESTRICT,
    owner_kind TEXT NOT NULL CHECK(owner_kind IN ('user','group')),
    owner_handle TEXT NOT NULL,
    title TEXT NOT NULL CHECK(length(title) BETWEEN 1 AND 160),
    rev INTEGER NOT NULL CHECK(rev>=1),
    acl_rev INTEGER NOT NULL CHECK(acl_rev>=1),
    lifecycle TEXT NOT NULL CHECK(lifecycle IN ('open','closing','closed')),
    admission TEXT NOT NULL CHECK(admission IN ('open','held')),
    history_base_id TEXT,
    history_grant_id TEXT,
    parent_agent_ref TEXT,
    sandbox_id TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(domain_handle,identity_handle,project_handle,agent_handle),
    UNIQUE(session_id,branch_id),
    CHECK((lifecycle='open') OR admission='held')
);

CREATE INDEX agent_state_by_project
    ON agent_state_current(domain_handle,identity_handle,project_handle,created_at,agent_handle);
CREATE INDEX agent_state_by_owner
    ON agent_state_current(owner_kind,owner_handle,domain_handle,project_handle,agent_handle);

CREATE TABLE agent_acl_current(
    session_id TEXT NOT NULL,
    branch_id TEXT NOT NULL,
    principal_kind TEXT NOT NULL CHECK(principal_kind IN ('user','group')),
    principal_handle TEXT NOT NULL,
    can_view INTEGER NOT NULL CHECK(can_view IN (0,1)),
    can_contribute INTEGER NOT NULL CHECK(can_contribute IN (0,1)),
    can_submit INTEGER NOT NULL CHECK(can_submit IN (0,1)),
    can_control INTEGER NOT NULL CHECK(can_control IN (0,1)),
    can_derive INTEGER NOT NULL CHECK(can_derive IN (0,1)),
    can_share INTEGER NOT NULL CHECK(can_share IN (0,1)),
    can_manage INTEGER NOT NULL CHECK(can_manage IN (0,1)),
    grant_rev INTEGER NOT NULL CHECK(grant_rev>=1),
    granted_by_handle TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(session_id,branch_id,principal_kind,principal_handle),
    FOREIGN KEY(session_id,branch_id)
        REFERENCES agent_state_current(session_id,branch_id) ON DELETE CASCADE,
    CHECK(can_view=1 OR (
        can_contribute=0 AND can_submit=0 AND can_control=0 AND
        can_derive=0 AND can_share=0 AND can_manage=0
    ))
);

CREATE INDEX agent_acl_by_principal
    ON agent_acl_current(principal_kind,principal_handle,session_id,branch_id);
