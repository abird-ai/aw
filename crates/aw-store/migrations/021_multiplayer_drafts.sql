-- Phase 38: shared prompt drafts. Mutable CRDT state is explicitly separate
-- from immutable Session history. Presence is intentionally not durable.

CREATE TABLE session_draft_policies(
    session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    mode TEXT NOT NULL CHECK(mode IN ('off','auto','on')),
    revision INTEGER NOT NULL CHECK(revision>=1),
    updated_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    updated_at INTEGER NOT NULL
);

CREATE TABLE session_draft_grants(
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    can_read INTEGER NOT NULL CHECK(can_read IN (0,1)),
    can_edit INTEGER NOT NULL CHECK(can_edit IN (0,1)),
    can_submit INTEGER NOT NULL CHECK(can_submit IN (0,1)),
    can_manage INTEGER NOT NULL CHECK(can_manage IN (0,1)),
    presence_read INTEGER NOT NULL CHECK(presence_read IN (0,1)),
    scope_revision INTEGER NOT NULL CHECK(scope_revision>=1),
    status TEXT NOT NULL CHECK(status IN ('active','revoked')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(session_id,user_id)
);
CREATE INDEX session_draft_grants_by_user
    ON session_draft_grants(user_id,status,session_id);

CREATE TABLE shared_drafts(
    draft_id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK(generation>=1),
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    branch_id TEXT NOT NULL REFERENCES branches(id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK(state IN ('active','sealed','admitted','rejected','abandoned')),
    document_epoch INTEGER NOT NULL CHECK(document_epoch>=1),
    accepted_revision INTEGER NOT NULL CHECK(accepted_revision>=0),
    metadata_revision INTEGER NOT NULL CHECK(metadata_revision>=1),
    audience_scope_revision INTEGER NOT NULL CHECK(audience_scope_revision>=1),
    target_head TEXT NOT NULL REFERENCES nodes(id),
    target_branch_revision INTEGER NOT NULL CHECK(target_branch_revision>=0),
    update_v1 BLOB NOT NULL,
    text_utf8 BLOB NOT NULL,
    created_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(draft_id,generation)
);
CREATE UNIQUE INDEX one_active_shared_draft_per_branch
    ON shared_drafts(session_id,branch_id) WHERE state='active';
CREATE INDEX shared_drafts_by_session
    ON shared_drafts(session_id,branch_id,generation DESC);

CREATE TABLE shared_draft_updates(
    draft_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    accepted_revision INTEGER NOT NULL CHECK(accepted_revision>=1),
    update_id TEXT NOT NULL,
    contributor_user_id TEXT NOT NULL REFERENCES users(user_id),
    update_v1 BLOB NOT NULL,
    accepted_at INTEGER NOT NULL,
    PRIMARY KEY(draft_id,generation,accepted_revision),
    UNIQUE(draft_id,generation,update_id),
    FOREIGN KEY(draft_id,generation) REFERENCES shared_drafts(draft_id,generation) ON DELETE CASCADE
);
CREATE INDEX shared_draft_updates_by_contributor
    ON shared_draft_updates(contributor_user_id,accepted_at);

CREATE TABLE shared_draft_submissions(
    submission_id TEXT PRIMARY KEY,
    draft_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    accepted_revision INTEGER NOT NULL,
    metadata_revision INTEGER NOT NULL,
    target_head TEXT NOT NULL REFERENCES nodes(id),
    target_branch_revision INTEGER NOT NULL,
    candidate_text_utf8 BLOB NOT NULL,
    candidate_update_v1 BLOB NOT NULL,
    candidate_sha256 TEXT NOT NULL CHECK(length(candidate_sha256)=64),
    submitted_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    contributor_user_ids_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('sealed','admitted','rejected')),
    prompt_request_id TEXT,
    admitted_node_id TEXT REFERENCES nodes(id),
    rejection_code TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(draft_id,generation),
    FOREIGN KEY(draft_id,generation) REFERENCES shared_drafts(draft_id,generation) ON DELETE CASCADE
);
CREATE UNIQUE INDEX shared_draft_submissions_by_prompt_request
    ON shared_draft_submissions(prompt_request_id)
    WHERE prompt_request_id IS NOT NULL;

UPDATE store_meta SET value='21' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=21,activation_epoch=21,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=21,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;
PRAGMA user_version=21;
