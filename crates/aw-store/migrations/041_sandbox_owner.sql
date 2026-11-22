-- DL-200 Phase 1: logical Project-scoped Sandbox identity/layout owner.
-- SourceBindings, SandboxGrant policy and driver realization remain separate
-- later owners; this table grants no filesystem/network/device authority.

CREATE TABLE sandboxes_current(
    sandbox_id TEXT PRIMARY KEY,
    domain_handle TEXT NOT NULL,
    identity_handle TEXT NOT NULL,
    project_handle TEXT NOT NULL,
    sandbox_handle TEXT NOT NULL,
    parent_sandbox_id TEXT REFERENCES sandboxes_current(sandbox_id) ON DELETE RESTRICT,
    working_directory TEXT NOT NULL,
    tool_root TEXT NOT NULL,
    layout_digest TEXT NOT NULL CHECK(length(layout_digest)=64),
    rev INTEGER NOT NULL CHECK(rev>=1),
    lifecycle TEXT NOT NULL CHECK(lifecycle IN ('open','closed')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(domain_handle,identity_handle,project_handle,sandbox_handle),
    FOREIGN KEY(domain_handle,identity_handle)
        REFERENCES domain_principals(domain_id,principal_handle) ON DELETE RESTRICT,
    FOREIGN KEY(project_handle)
        REFERENCES workspaces(workspace_id) ON DELETE RESTRICT,
    CHECK(length(sandbox_handle) BETWEEN 1 AND 64),
    CHECK(
        sandbox_id =
        '/d:' || domain_handle ||
        '/i:' || identity_handle ||
        '/p:' || project_handle ||
        '/s:' || sandbox_handle
    )
);

CREATE INDEX sandboxes_by_project
    ON sandboxes_current(
        domain_handle,identity_handle,project_handle,lifecycle,created_at,sandbox_handle
    );
CREATE INDEX sandboxes_by_layout
    ON sandboxes_current(
        domain_handle,identity_handle,project_handle,lifecycle,layout_digest
    );
